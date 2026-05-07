// SPDX-License-Identifier: Apache-2.0
//
// Fast-DDS TopicDataType subclasses for the cora-side wire types the UMI
// bridge subscribes to. Hand-written to keep fastddsgen out of the build
// dependency closure.
//
// We only define PubSubTypes for the top-level subscribed types
// (`sensor_msgs::msg::Imu` and `foxglove_msgs::msg::CompressedVideo`).
// The dependent types (Header, Time, Quaternion, Vector3) are serialized
// inline via fastcdr operators in the .cpp.

#pragma once

#include "cora_types.hpp"

#include <fastdds/dds/topic/TopicDataType.hpp>

namespace umi_bridge {

class ImuPubSubType : public eprosima::fastdds::dds::TopicDataType {
 public:
    using type = ::sensor_msgs::msg::Imu;
    static constexpr const char* TYPE_NAME = "sensor_msgs::msg::dds_::Imu_";

    ImuPubSubType();
    ~ImuPubSubType() override = default;

    bool serialize(const void* const data, eprosima::fastdds::rtps::SerializedPayload_t& payload,
                   eprosima::fastdds::dds::DataRepresentationId_t data_representation) override;
    bool deserialize(eprosima::fastdds::rtps::SerializedPayload_t& payload, void* data) override;
    uint32_t calculate_serialized_size(
        const void* const data,
        eprosima::fastdds::dds::DataRepresentationId_t data_representation) override;

    void* create_data() override { return new type(); }
    void delete_data(void* data) override { delete static_cast<type*>(data); }

    bool compute_key(eprosima::fastdds::rtps::SerializedPayload_t&,
                     eprosima::fastdds::rtps::InstanceHandle_t&, bool = false) override {
        return false;
    }
    bool compute_key(const void* const, eprosima::fastdds::rtps::InstanceHandle_t&,
                     bool = false) override {
        return false;
    }
    void register_type_object_representation() override {}
};

class CompressedVideoPubSubType : public eprosima::fastdds::dds::TopicDataType {
 public:
    using type = ::foxglove_msgs::msg::CompressedVideo;
    static constexpr const char* TYPE_NAME = "foxglove_msgs::msg::dds_::CompressedVideo_";

    CompressedVideoPubSubType();
    ~CompressedVideoPubSubType() override = default;

    bool serialize(const void* const data, eprosima::fastdds::rtps::SerializedPayload_t& payload,
                   eprosima::fastdds::dds::DataRepresentationId_t data_representation) override;
    bool deserialize(eprosima::fastdds::rtps::SerializedPayload_t& payload, void* data) override;
    uint32_t calculate_serialized_size(
        const void* const data,
        eprosima::fastdds::dds::DataRepresentationId_t data_representation) override;

    void* create_data() override { return new type(); }
    void delete_data(void* data) override { delete static_cast<type*>(data); }

    bool compute_key(eprosima::fastdds::rtps::SerializedPayload_t&,
                     eprosima::fastdds::rtps::InstanceHandle_t&, bool = false) override {
        return false;
    }
    bool compute_key(const void* const, eprosima::fastdds::rtps::InstanceHandle_t&,
                     bool = false) override {
        return false;
    }
    void register_type_object_representation() override {}
};

}  // namespace umi_bridge
