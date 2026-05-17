// JointState ChannelReader: subscribes to a sensor_msgs::msg::JointState topic
// and delivers decoded fields to the registered C callback via the SDK's
// CallbackExecutor.

#include "subscriber.h"

#include <exception>
#include <utility>
#include <vector>

#include <cora/channel.h>
#include <cora/dds/dds_qos.h>

#include <sensor_msgs/msg/JointState.h>
#include <sensor_msgs/msg/JointStatePubSubTypes.h>

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

class JointStateSubscription : public CoraSubscription {
 public:
    using Msg = sensor_msgs::msg::JointState;
    using PST = sensor_msgs::msg::JointStatePubSubType;
    using Reader = framework::ChannelReader<Msg, PST>;

    JointStateSubscription(const std::string& topic, bool qos_reliable,
                           cora_joint_state_cb_t cb, void* user)
        : cb_(cb), user_(user),
          reader_(std::make_unique<Reader>(topic, select_qos(qos_reliable))) {}

    ~JointStateSubscription() override { clear(); }

    void install_callback() {
        cora_joint_state_cb_t cb = cb_;
        void* user = user_;
        uint32_t* id_ptr = &id_;
        reader_->setCallback([cb, user, id_ptr](Reader::MessagePtr msg) {
            try {
                const Msg& m = msg->data();
                uint64_t ts_us = pick_ts_us(m.header().stamp().sec(),
                                            m.header().stamp().nanosec(),
                                            msg->timestamp());

                const auto& names_vec = m.name();
                std::vector<const char*> name_ptrs;
                name_ptrs.reserve(names_vec.size());
                for (const auto& s : names_vec) name_ptrs.push_back(s.c_str());

                const auto& positions = m.position();
                const auto& velocities = m.velocity();
                const auto& efforts = m.effort();

                cb(*id_ptr, ts_us,
                   name_ptrs.data(), name_ptrs.size(),
                   positions.data(), positions.size(),
                   velocities.data(), velocities.size(),
                   efforts.data(), efforts.size(),
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
    cora_joint_state_cb_t cb_;
    void* user_;
    std::unique_ptr<Reader> reader_;
};

}  // namespace

std::unique_ptr<CoraSubscription> make_joint_state_subscription(
    const std::string& topic, bool qos_reliable,
    cora_joint_state_cb_t cb, void* user) {
    try {
        auto sub = std::make_unique<JointStateSubscription>(topic, qos_reliable, cb, user);
        sub->install_callback();
        return sub;
    } catch (...) {
        return nullptr;
    }
}
