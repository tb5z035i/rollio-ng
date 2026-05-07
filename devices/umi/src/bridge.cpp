// SPDX-License-Identifier: Apache-2.0

#include "bridge.hpp"
#include "generated/cora_pubsubtypes.hpp"

#include "iox2/iceoryx2.hpp"
#include "rollio/topic_names.hpp"
#include "rollio/types.h"

#include <fastdds/dds/domain/DomainParticipant.hpp>
#include <fastdds/dds/domain/DomainParticipantFactory.hpp>
#include <fastdds/dds/domain/qos/DomainParticipantQos.hpp>
#include <fastdds/dds/subscriber/DataReader.hpp>
#include <fastdds/dds/subscriber/DataReaderListener.hpp>
#include <fastdds/dds/subscriber/SampleInfo.hpp>
#include <fastdds/dds/subscriber/Subscriber.hpp>
#include <fastdds/dds/subscriber/qos/DataReaderQos.hpp>
#include <fastdds/dds/topic/Topic.hpp>
#include <fastdds/dds/topic/TypeSupport.hpp>

#include <atomic>
#include <chrono>
#include <iostream>
#include <memory>
#include <mutex>
#include <string>
#include <thread>
#include <utility>
#include <vector>

namespace umi_bridge {

namespace {

using SteadyClock = std::chrono::steady_clock;

constexpr std::chrono::seconds kInitialMatchWarning{5};
constexpr std::chrono::milliseconds kPollInterval{2};

uint64_t imu_unix_us(const ::sensor_msgs::msg::Imu& imu) {
    // ROS2 stamps are sec + nanosec UNIX epoch. Truncate to microseconds.
    return static_cast<uint64_t>(imu.header.stamp.sec) * 1'000'000ull +
           static_cast<uint64_t>(imu.header.stamp.nanosec) / 1'000ull;
}

uint64_t compressed_video_unix_us(const ::foxglove_msgs::msg::CompressedVideo& cv) {
    return static_cast<uint64_t>(cv.timestamp.sec) * 1'000'000ull +
           static_cast<uint64_t>(cv.timestamp.nanosec) / 1'000ull;
}

eprosima::fastdds::dds::DataReaderQos reliable_keep_last_qos() {
    auto qos = eprosima::fastdds::dds::DATAREADER_QOS_DEFAULT;
    qos.reliability().kind = eprosima::fastdds::dds::RELIABLE_RELIABILITY_QOS;
    qos.durability().kind = eprosima::fastdds::dds::VOLATILE_DURABILITY_QOS;
    qos.history().kind = eprosima::fastdds::dds::KEEP_LAST_HISTORY_QOS;
    qos.history().depth = 1;
    return qos;
}

// Listener that simply notes when the matched-publisher count goes
// non-zero, so the bridge can log a "waiting" message after 5 seconds
// without a writer match. Per-topic instance owned by each ChannelThread.
class MatchListener : public eprosima::fastdds::dds::DataReaderListener {
 public:
    void on_subscription_matched(eprosima::fastdds::dds::DataReader* /*reader*/,
                                 const eprosima::fastdds::dds::SubscriptionMatchedStatus& status)
        override {
        matched_writers_.store(status.current_count);
    }
    int matched_writers() const { return matched_writers_.load(); }

 private:
    std::atomic<int> matched_writers_{0};
};

}  // namespace

// ---------------------------------------------------------------------------
// Camera bridge: FastDDS CompressedVideo -> iceoryx2 [u8] + CameraFrameHeader
// ---------------------------------------------------------------------------

class CameraChannelThread {
 public:
    CameraChannelThread(const CameraBridge& cfg, const std::string& bus_root,
                        eprosima::fastdds::dds::DomainParticipant* participant,
                        iox2::Node<iox2::ServiceType::Ipc>& iox_node,
                        std::atomic<bool>& stop_flag)
        : cfg_(cfg), bus_root_(bus_root), participant_(participant), stop_flag_(stop_flag) {
        // FastDDS subscriber side: register type, create topic, create
        // DataReader.
        type_ = eprosima::fastdds::dds::TypeSupport(new CompressedVideoPubSubType());
        type_.register_type(participant_);
        topic_ = participant_->create_topic(cfg_.dds_topic, type_->get_name(),
                                            eprosima::fastdds::dds::TOPIC_QOS_DEFAULT);
        if (!topic_) {
            throw std::runtime_error("UMI bridge: failed to create FastDDS topic " +
                                     cfg_.dds_topic);
        }
        subscriber_ = participant_->create_subscriber(eprosima::fastdds::dds::SUBSCRIBER_QOS_DEFAULT);
        listener_ = std::make_unique<MatchListener>();
        reader_ = subscriber_->create_datareader(topic_, reliable_keep_last_qos(), listener_.get());
        if (!reader_) {
            throw std::runtime_error("UMI bridge: failed to create FastDDS DataReader on " +
                                     cfg_.dds_topic);
        }

        // iceoryx2 publisher side.
        const auto service_name_str =
            ::rollio::channel_frames_service_name(bus_root_, cfg_.channel_type);
        const auto service_name = iox2::ServiceName::create(service_name_str.c_str()).value();
        auto service = iox_node.service_builder(service_name)
                           .publish_subscribe<iox2::bb::Slice<uint8_t>>()
                           .user_header<::rollio::CameraFrameHeader>()
                           .open_or_create()
                           .value();
        constexpr uint64_t kInitialSlotBytes = 2u * 1024u * 1024u;  // 2 MiB
        publisher_ = service.publisher_builder()
                         .initial_max_slice_len(kInitialSlotBytes)
                         .allocation_strategy(iox2::AllocationStrategy::PowerOfTwo)
                         .create()
                         .value();
    }

    void start() {
        thread_ = std::thread([this] { run(); });
    }

    void join() {
        if (thread_.joinable()) {
            thread_.join();
        }
    }

    ~CameraChannelThread() {
        join();
        if (reader_) {
            subscriber_->delete_datareader(reader_);
        }
        if (subscriber_) {
            participant_->delete_subscriber(subscriber_);
        }
        if (topic_) {
            participant_->delete_topic(topic_);
        }
    }

 private:
    void run() {
        ::foxglove_msgs::msg::CompressedVideo sample;  // long-lived to reuse vector capacity
        eprosima::fastdds::dds::SampleInfo info;
        const auto start_time = SteadyClock::now();
        bool warned_no_match = false;

        while (!stop_flag_.load()) {
            const auto take_rc = reader_->take_next_sample(&sample, &info);
            if (take_rc == eprosima::fastdds::dds::RETCODE_OK) {
                if (info.valid_data) {
                    publish_to_iceoryx(sample);
                }
                continue;
            }
            // No data — sleep briefly and possibly log the "still waiting"
            // diagnostic after 5 seconds.
            if (!warned_no_match && listener_->matched_writers() == 0 &&
                SteadyClock::now() - start_time >= kInitialMatchWarning) {
                warned_no_match = true;
                std::cerr << "rollio-device-umi: waiting for publisher on topic " << cfg_.dds_topic
                          << " (no writer matched after 5s)\n";
            }
            std::this_thread::sleep_for(kPollInterval);
        }
    }

    void publish_to_iceoryx(const ::foxglove_msgs::msg::CompressedVideo& sample) {
        const auto payload_size = sample.data.size();
        if (payload_size == 0) {
            return;
        }
        try {
            auto loaned = publisher_.loan_slice_uninit(payload_size).expect("loan_slice_uninit");
            auto& header = loaned.user_header_mut();
            header.timestamp_us = compressed_video_unix_us(sample);
            header.width = cfg_.width;
            header.height = cfg_.height;
            header.pixel_format = ::rollio::PixelFormat::H264;
            header.frame_index = local_frame_index_++;
            auto frame_slice =
                iox2::bb::ImmutableSlice<uint8_t>(sample.data.data(), payload_size);
            auto initialized = loaned.write_from_slice(frame_slice);
            initialized.send().expect("iceoryx2 send");
        } catch (const std::exception& e) {
            std::cerr << "rollio-device-umi: iceoryx2 publish failed for channel "
                      << cfg_.channel_type << ": " << e.what() << "\n";
        }
    }

    CameraBridge cfg_;
    std::string bus_root_;
    eprosima::fastdds::dds::DomainParticipant* participant_;
    eprosima::fastdds::dds::TypeSupport type_;
    eprosima::fastdds::dds::Topic* topic_{nullptr};
    eprosima::fastdds::dds::Subscriber* subscriber_{nullptr};
    std::unique_ptr<MatchListener> listener_;
    eprosima::fastdds::dds::DataReader* reader_{nullptr};
    iox2::Publisher<iox2::ServiceType::Ipc, iox2::bb::Slice<uint8_t>, ::rollio::CameraFrameHeader>
        publisher_;
    std::thread thread_;
    std::atomic<bool>& stop_flag_;
    uint64_t local_frame_index_{0};
};

// ---------------------------------------------------------------------------
// IMU bridge: FastDDS sensor_msgs::Imu -> iceoryx2 rollio::Imu
// ---------------------------------------------------------------------------

class ImuChannelThread {
 public:
    ImuChannelThread(const ImuBridge& cfg, const std::string& bus_root,
                     eprosima::fastdds::dds::DomainParticipant* participant,
                     iox2::Node<iox2::ServiceType::Ipc>& iox_node,
                     std::atomic<bool>& stop_flag)
        : cfg_(cfg), bus_root_(bus_root), participant_(participant), stop_flag_(stop_flag) {
        type_ = eprosima::fastdds::dds::TypeSupport(new ImuPubSubType());
        type_.register_type(participant_);
        topic_ = participant_->create_topic(cfg_.dds_topic, type_->get_name(),
                                            eprosima::fastdds::dds::TOPIC_QOS_DEFAULT);
        if (!topic_) {
            throw std::runtime_error("UMI bridge: failed to create FastDDS topic " +
                                     cfg_.dds_topic);
        }
        subscriber_ = participant_->create_subscriber(eprosima::fastdds::dds::SUBSCRIBER_QOS_DEFAULT);
        listener_ = std::make_unique<MatchListener>();
        reader_ = subscriber_->create_datareader(topic_, reliable_keep_last_qos(), listener_.get());
        if (!reader_) {
            throw std::runtime_error("UMI bridge: failed to create FastDDS DataReader on " +
                                     cfg_.dds_topic);
        }

        const auto service_name_str =
            ::rollio::channel_imu_service_name(bus_root_, cfg_.channel_type);
        const auto service_name = iox2::ServiceName::create(service_name_str.c_str()).value();
        auto service = iox_node.service_builder(service_name)
                           .publish_subscribe<::rollio::Imu>()
                           .open_or_create()
                           .value();
        publisher_ = service.publisher_builder().create().value();
    }

    void start() {
        thread_ = std::thread([this] { run(); });
    }

    void join() {
        if (thread_.joinable()) {
            thread_.join();
        }
    }

    ~ImuChannelThread() {
        join();
        if (reader_) {
            subscriber_->delete_datareader(reader_);
        }
        if (subscriber_) {
            participant_->delete_subscriber(subscriber_);
        }
        if (topic_) {
            participant_->delete_topic(topic_);
        }
    }

 private:
    void run() {
        ::sensor_msgs::msg::Imu sample;
        eprosima::fastdds::dds::SampleInfo info;
        const auto start_time = SteadyClock::now();
        bool warned_no_match = false;

        while (!stop_flag_.load()) {
            const auto take_rc = reader_->take_next_sample(&sample, &info);
            if (take_rc == eprosima::fastdds::dds::RETCODE_OK) {
                if (info.valid_data) {
                    publish_to_iceoryx(sample);
                }
                continue;
            }
            if (!warned_no_match && listener_->matched_writers() == 0 &&
                SteadyClock::now() - start_time >= kInitialMatchWarning) {
                warned_no_match = true;
                std::cerr << "rollio-device-umi: waiting for publisher on topic " << cfg_.dds_topic
                          << " (no writer matched after 5s)\n";
            }
            std::this_thread::sleep_for(kPollInterval);
        }
    }

    void publish_to_iceoryx(const ::sensor_msgs::msg::Imu& sample) {
        try {
            auto loaned = publisher_.loan_uninit().expect("loan_uninit");
            ::rollio::Imu out{};
            out.timestamp_us = imu_unix_us(sample);
            out.orientation[0] = sample.orientation.x;
            out.orientation[1] = sample.orientation.y;
            out.orientation[2] = sample.orientation.z;
            out.orientation[3] = sample.orientation.w;
            out.angular_velocity[0] = sample.angular_velocity.x;
            out.angular_velocity[1] = sample.angular_velocity.y;
            out.angular_velocity[2] = sample.angular_velocity.z;
            out.linear_acceleration[0] = sample.linear_acceleration.x;
            out.linear_acceleration[1] = sample.linear_acceleration.y;
            out.linear_acceleration[2] = sample.linear_acceleration.z;
            for (size_t i = 0; i < 9; ++i) {
                out.orientation_covariance[i] = sample.orientation_covariance[i];
                out.angular_velocity_covariance[i] = sample.angular_velocity_covariance[i];
                out.linear_acceleration_covariance[i] = sample.linear_acceleration_covariance[i];
            }
            auto initialized = loaned.write_payload(out);
            initialized.send().expect("iceoryx2 send");
        } catch (const std::exception& e) {
            std::cerr << "rollio-device-umi: iceoryx2 publish failed for imu "
                      << cfg_.channel_type << ": " << e.what() << "\n";
        }
    }

    ImuBridge cfg_;
    std::string bus_root_;
    eprosima::fastdds::dds::DomainParticipant* participant_;
    eprosima::fastdds::dds::TypeSupport type_;
    eprosima::fastdds::dds::Topic* topic_{nullptr};
    eprosima::fastdds::dds::Subscriber* subscriber_{nullptr};
    std::unique_ptr<MatchListener> listener_;
    eprosima::fastdds::dds::DataReader* reader_{nullptr};
    iox2::Publisher<iox2::ServiceType::Ipc, ::rollio::Imu> publisher_;
    std::thread thread_;
    std::atomic<bool>& stop_flag_;
};

// ---------------------------------------------------------------------------
// Top-level run loop
// ---------------------------------------------------------------------------

int run_bridge(const UmiBridgeConfig& config, std::atomic<bool>& stop_flag) {
    // Set up the FastDDS DomainParticipant. We share one across all
    // bridged topics; per-topic state lives in the ChannelThread classes
    // above.
    auto& factory = *eprosima::fastdds::dds::DomainParticipantFactory::get_instance();
    eprosima::fastdds::dds::DomainParticipantQos pqos =
        eprosima::fastdds::dds::PARTICIPANT_QOS_DEFAULT;
    pqos.name("rollio-device-umi");
    auto* participant = factory.create_participant(static_cast<uint32_t>(config.dds.domain_id), pqos);
    if (!participant) {
        std::cerr << "rollio-device-umi: failed to create FastDDS DomainParticipant on domain "
                  << config.dds.domain_id << "\n";
        return 1;
    }

    // iceoryx2 node — shared by every channel publisher.
    auto iox_node = iox2::NodeBuilder().create<iox2::ServiceType::Ipc>().expect("iceoryx2 node");

    std::vector<std::unique_ptr<CameraChannelThread>> camera_threads;
    std::vector<std::unique_ptr<ImuChannelThread>> imu_threads;

    try {
        for (const auto& cam : config.cameras) {
            camera_threads.push_back(std::make_unique<CameraChannelThread>(
                cam, config.bus_root, participant, iox_node, stop_flag));
        }
        for (const auto& imu : config.imus) {
            imu_threads.push_back(std::make_unique<ImuChannelThread>(
                imu, config.bus_root, participant, iox_node, stop_flag));
        }
    } catch (const std::exception& e) {
        std::cerr << "rollio-device-umi: bridge setup failed: " << e.what() << "\n";
        factory.delete_participant(participant);
        return 1;
    }

    // Subscribe to the rollio control-plane shutdown event so the
    // controller can stop us promptly during preview-runtime swaps.
    const auto control_service_name =
        iox2::ServiceName::create(::rollio::CONTROL_EVENTS_SERVICE).expect("control svc name");
    auto control_service = iox_node.service_builder(control_service_name)
                               .publish_subscribe<::rollio::ControlEvent>()
                               .open_or_create()
                               .expect("control service");
    auto control_subscriber =
        control_service.subscriber_builder().create().expect("control subscriber");

    for (auto& t : camera_threads) {
        t->start();
    }
    for (auto& t : imu_threads) {
        t->start();
    }

    while (!stop_flag.load()) {
        auto event = control_subscriber.receive().expect("control receive");
        while (event.has_value()) {
            if (event->payload().tag == ::rollio::ControlEventTag::Shutdown) {
                stop_flag.store(true);
                break;
            }
            event = control_subscriber.receive().expect("control receive");
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(50));
    }

    // Workers join in their destructors when the unique_ptrs go out of scope.
    camera_threads.clear();
    imu_threads.clear();
    factory.delete_participant(participant);
    std::cerr << "rollio-device-umi: shutdown complete\n";
    return 0;
}

}  // namespace umi_bridge
