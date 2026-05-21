// PointCloud2 ChannelReader: subscribes to a sensor_msgs::msg::PointCloud2
// topic and delivers decoded fields to the registered C callback via the SDK's
// CallbackExecutor.

#include "subscriber.h"

#include <cstdint>
#include <exception>
#include <utility>
#include <vector>

#include <cora/channel.h>
#include <cora/dds/dds_qos.h>

#include <sensor_msgs/msg/PointCloud2.h>
#include <sensor_msgs/msg/PointCloud2PubSubTypes.h>
#include <sensor_msgs/msg/PointField.h>

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

class PointCloud2Subscription : public CoraSubscription {
 public:
    using Msg = sensor_msgs::msg::PointCloud2;
    using PST = sensor_msgs::msg::PointCloud2PubSubType;
    using Reader = framework::ChannelReader<Msg, PST>;

    PointCloud2Subscription(const std::string& topic, bool qos_reliable,
                            cora_pointcloud_cb_t cb, void* user)
        : cb_(cb), user_(user),
          reader_(std::make_unique<Reader>(topic, select_qos(qos_reliable))) {}

    ~PointCloud2Subscription() override { clear(); }

    void install_callback() {
        cora_pointcloud_cb_t cb = cb_;
        void* user = user_;
        uint32_t* id_ptr = &id_;
        reader_->setCallback([cb, user, id_ptr](Reader::MessagePtr msg) {
            try {
                const Msg& m = msg->data();
                uint64_t ts_us = pick_ts_us(m.header().stamp().sec(),
                                            m.header().stamp().nanosec(),
                                            msg->timestamp());

                const auto& fields_vec = m.fields();
                std::vector<cora_point_field_t> c_fields;
                c_fields.reserve(fields_vec.size());
                for (const auto& f : fields_vec) {
                    cora_point_field_t cf{};
                    cf.name = f.name().c_str();
                    cf.offset = f.offset();
                    cf.datatype = f.datatype();
                    cf.count = f.count();
                    c_fields.push_back(cf);
                }

                const auto& bytes = m.data();
                cb(*id_ptr, ts_us,
                   m.width(), m.height(),
                   m.point_step(), m.row_step(),
                   c_fields.data(), c_fields.size(),
                   bytes.data(), bytes.size(),
                   m.is_bigendian() ? 1 : 0,
                   m.is_dense() ? 1 : 0,
                   user);
            } catch (const std::exception&) {
            } catch (...) {
            }
        });
    }

    void clear() override {
        if (reader_) reader_->clearCallback();
    }

 private:
    cora_pointcloud_cb_t cb_;
    void* user_;
    std::unique_ptr<Reader> reader_;
};

}  // namespace

std::unique_ptr<CoraSubscription> make_point_cloud2_subscription(
    const std::string& topic, bool qos_reliable,
    cora_pointcloud_cb_t cb, void* user) {
    try {
        auto sub = std::make_unique<PointCloud2Subscription>(topic, qos_reliable, cb, user);
        sub->install_callback();
        return sub;
    } catch (...) {
        return nullptr;
    }
}
