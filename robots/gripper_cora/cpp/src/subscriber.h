// Internal: abstract subscription handle + JointState subscription factory.

#ifndef GRIPPER_CORA_SUBSCRIBER_H_
#define GRIPPER_CORA_SUBSCRIBER_H_

#include <cstdint>
#include <memory>
#include <string>

#include "cora_bridge.h"

class CoraSubscription {
 public:
    virtual ~CoraSubscription() = default;
    virtual void clear() = 0;
    void set_id(uint32_t id) { id_ = id; }
    uint32_t id() const { return id_; }

 protected:
    uint32_t id_ = 0;
};

std::unique_ptr<CoraSubscription> make_joint_state_subscription(
    const std::string& topic, bool qos_reliable,
    cora_joint_state_cb_t cb, void* user);

#endif  // GRIPPER_CORA_SUBSCRIBER_H_
