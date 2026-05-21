// Internal: abstract subscription handle + Imu subscription factory.

#ifndef IMU_CORA_SUBSCRIBER_H_
#define IMU_CORA_SUBSCRIBER_H_

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

std::unique_ptr<CoraSubscription> make_imu_subscription(
    const std::string& topic, bool qos_reliable,
    cora_imu_cb_t cb, void* user);

#endif  // IMU_CORA_SUBSCRIBER_H_
