// Imu ChannelReader: subscribes to a sensor_msgs::msg::Imu topic and delivers
// decoded fields to the registered C callback via the SDK's CallbackExecutor.

#include "subscriber.h"

#include <chrono>
#include <exception>
#include <utility>

#include <cora/channel.h>
#include <cora/dds/dds_qos.h>

#include <sensor_msgs/msg/Imu.h>
#include <sensor_msgs/msg/ImuPubSubTypes.h>

namespace {

inline framework::dds::QoSConfig select_qos(bool reliable) {
    return reliable ? framework::dds::QoSConfig::reliableQoS()
                    : framework::dds::QoSConfig::bestEffortQoS();
}

inline uint64_t pick_ts_us(int32_t sec, uint32_t nanosec, uint64_t fallback_ns) {
    if (sec != 0 || nanosec != 0) {
        return static_cast<uint64_t>(sec) * 1'000'000ULL +
               static_cast<uint64_t>(nanosec) / 1'000ULL;
    }
    return fallback_ns / 1'000ULL;
}

class ImuSubscription : public CoraSubscription {
 public:
    using Msg = sensor_msgs::msg::Imu;
    using PST = sensor_msgs::msg::ImuPubSubType;
    using Reader = framework::ChannelReader<Msg, PST>;

    ImuSubscription(const std::string& topic, bool qos_reliable,
                    cora_imu_cb_t cb, void* user)
        : cb_(cb), user_(user),
          reader_(std::make_unique<Reader>(topic, select_qos(qos_reliable))) {}

    ~ImuSubscription() override { clear(); }

    void install_callback() {
        cora_imu_cb_t cb = cb_;
        void* user = user_;
        uint32_t* id_ptr = &id_;
        reader_->setCallback([cb, user, id_ptr](Reader::MessagePtr msg) {
            try {
                const Msg& m = msg->data();
                uint64_t ts_us = pick_ts_us(m.header().stamp().sec(),
                                            m.header().stamp().nanosec(),
                                            msg->timestamp());
                cb(*id_ptr, ts_us,
                   m.linear_acceleration().x(),
                   m.linear_acceleration().y(),
                   m.linear_acceleration().z(),
                   m.angular_velocity().x(),
                   m.angular_velocity().y(),
                   m.angular_velocity().z(),
                   m.orientation().x(),
                   m.orientation().y(),
                   m.orientation().z(),
                   m.orientation().w(),
                   user);
            } catch (const std::exception&) {
                // Swallow: one bad message must not bring down the executor.
            } catch (...) {
            }
        });
    }

    void clear() override {
        if (reader_) reader_->clearCallback();
    }

 private:
    cora_imu_cb_t cb_;
    void* user_;
    std::unique_ptr<Reader> reader_;
};

}  // namespace

std::unique_ptr<CoraSubscription> make_imu_subscription(
    const std::string& topic, bool qos_reliable,
    cora_imu_cb_t cb, void* user) {
    try {
        auto sub = std::make_unique<ImuSubscription>(topic, qos_reliable, cb, user);
        sub->install_callback();
        return sub;
    } catch (...) {
        return nullptr;
    }
}
