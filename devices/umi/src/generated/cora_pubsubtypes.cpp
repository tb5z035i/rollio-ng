// SPDX-License-Identifier: Apache-2.0
//
// CDR serialize/deserialize for the cora-side wire types the UMI bridge
// subscribes to. Mirrors what `fastddsgen -typeros2` would emit but is
// hand-written so the rollio build doesn't need a JDK.
//
// Encoding: ROS2 messages over Fast-DDS use XCDRv1 (PLAIN_CDR) with the
// publisher's native endianness encoded in the encapsulation header. Cora
// publishes via Fast-DDS 1.x which only emits PLAIN_CDR, so we always
// declare PLAIN_CDR for serialization and let fastcdr's deserializer pick
// up the wire endianness from the encapsulation byte at the start of the
// payload.
//
// Alignment rules (XCDRv1):
//   * 1-byte:  uint8, char    -> aligned to 1
//   * 2-byte:  uint16          -> aligned to 2
//   * 4-byte:  int32, uint32   -> aligned to 4
//   * 8-byte:  double, int64   -> aligned to 8
//   * string:  uint32 length (incl. NUL) + bytes + NUL
//   * sequence:uint32 length + element-typed bytes

#include "cora_pubsubtypes.hpp"

#include <fastcdr/Cdr.h>
#include <fastcdr/CdrSizeCalculator.hpp>
#include <fastcdr/FastBuffer.h>
#include <fastcdr/exceptions/Exception.h>

#include <fastdds/dds/log/Log.hpp>
#include <fastdds/rtps/common/CdrSerialization.hpp>

#include <algorithm>
#include <cstring>

using SerializedPayload_t = eprosima::fastdds::rtps::SerializedPayload_t;
using DataRepresentationId_t = eprosima::fastdds::dds::DataRepresentationId_t;

namespace {

// ---------------------------------------------------------------------------
// Per-field serializer helpers — written as free functions so the top-level
// PubSubType serialize methods read straight down the IDL field list.
// ---------------------------------------------------------------------------

inline void cdr_serialize(eprosima::fastcdr::Cdr& cdr, const ::builtin_interfaces::msg::Time& v) {
    cdr << v.sec;
    cdr << v.nanosec;
}

inline void cdr_deserialize(eprosima::fastcdr::Cdr& cdr, ::builtin_interfaces::msg::Time& v) {
    cdr >> v.sec;
    cdr >> v.nanosec;
}

inline void cdr_serialize(eprosima::fastcdr::Cdr& cdr, const ::geometry_msgs::msg::Vector3& v) {
    cdr << v.x;
    cdr << v.y;
    cdr << v.z;
}

inline void cdr_deserialize(eprosima::fastcdr::Cdr& cdr, ::geometry_msgs::msg::Vector3& v) {
    cdr >> v.x;
    cdr >> v.y;
    cdr >> v.z;
}

inline void cdr_serialize(eprosima::fastcdr::Cdr& cdr, const ::geometry_msgs::msg::Quaternion& v) {
    cdr << v.x;
    cdr << v.y;
    cdr << v.z;
    cdr << v.w;
}

inline void cdr_deserialize(eprosima::fastcdr::Cdr& cdr, ::geometry_msgs::msg::Quaternion& v) {
    cdr >> v.x;
    cdr >> v.y;
    cdr >> v.z;
    cdr >> v.w;
}

inline void cdr_serialize(eprosima::fastcdr::Cdr& cdr, const ::std_msgs::msg::Header& v) {
    cdr_serialize(cdr, v.stamp);
    cdr << v.frame_id;
}

inline void cdr_deserialize(eprosima::fastcdr::Cdr& cdr, ::std_msgs::msg::Header& v) {
    cdr_deserialize(cdr, v.stamp);
    cdr >> v.frame_id;
}

template <std::size_t N>
inline void cdr_serialize(eprosima::fastcdr::Cdr& cdr, const std::array<double, N>& v) {
    cdr.serialize_array(v.data(), N);
}

template <std::size_t N>
inline void cdr_deserialize(eprosima::fastcdr::Cdr& cdr, std::array<double, N>& v) {
    cdr.deserialize_array(v.data(), N);
}

// ---------------------------------------------------------------------------
// Top-level serializers reused by serialize / calculate_serialized_size.
// ---------------------------------------------------------------------------

inline void cdr_serialize_imu(eprosima::fastcdr::Cdr& cdr, const ::sensor_msgs::msg::Imu& v) {
    cdr_serialize(cdr, v.header);
    cdr_serialize(cdr, v.orientation);
    cdr_serialize(cdr, v.orientation_covariance);
    cdr_serialize(cdr, v.angular_velocity);
    cdr_serialize(cdr, v.angular_velocity_covariance);
    cdr_serialize(cdr, v.linear_acceleration);
    cdr_serialize(cdr, v.linear_acceleration_covariance);
}

inline void cdr_deserialize_imu(eprosima::fastcdr::Cdr& cdr, ::sensor_msgs::msg::Imu& v) {
    cdr_deserialize(cdr, v.header);
    cdr_deserialize(cdr, v.orientation);
    cdr_deserialize(cdr, v.orientation_covariance);
    cdr_deserialize(cdr, v.angular_velocity);
    cdr_deserialize(cdr, v.angular_velocity_covariance);
    cdr_deserialize(cdr, v.linear_acceleration);
    cdr_deserialize(cdr, v.linear_acceleration_covariance);
}

inline void cdr_serialize_compressed_video(eprosima::fastcdr::Cdr& cdr,
                                           const ::foxglove_msgs::msg::CompressedVideo& v) {
    cdr_serialize(cdr, v.timestamp);
    cdr << v.frame_id;
    cdr << v.data;
    cdr << v.format;
}

inline void cdr_deserialize_compressed_video(eprosima::fastcdr::Cdr& cdr,
                                             ::foxglove_msgs::msg::CompressedVideo& v) {
    cdr_deserialize(cdr, v.timestamp);
    cdr >> v.frame_id;
    cdr >> v.data;
    cdr >> v.format;
}

// ---------------------------------------------------------------------------
// Common envelope used by both PubSubType implementations.
// ---------------------------------------------------------------------------

template <typename SerializeFn>
bool run_serialize(SerializedPayload_t& payload, DataRepresentationId_t data_representation,
                   SerializeFn&& serialize_body) {
    eprosima::fastcdr::FastBuffer fastbuffer(reinterpret_cast<char*>(payload.data),
                                             payload.max_size);
    eprosima::fastcdr::Cdr ser(
        fastbuffer, eprosima::fastcdr::Cdr::DEFAULT_ENDIAN,
        data_representation == DataRepresentationId_t::XCDR_DATA_REPRESENTATION
            ? eprosima::fastcdr::CdrVersion::XCDRv1
            : eprosima::fastcdr::CdrVersion::XCDRv2);
    payload.encapsulation =
        ser.endianness() == eprosima::fastcdr::Cdr::BIG_ENDIANNESS ? CDR_BE : CDR_LE;
    ser.set_encoding_flag(
        data_representation == DataRepresentationId_t::XCDR_DATA_REPRESENTATION
            ? eprosima::fastcdr::EncodingAlgorithmFlag::PLAIN_CDR
            : eprosima::fastcdr::EncodingAlgorithmFlag::DELIMIT_CDR2);

    try {
        ser.serialize_encapsulation();
        serialize_body(ser);
        ser.set_dds_cdr_options({0, 0});
    } catch (eprosima::fastcdr::exception::Exception&) {
        return false;
    }

    payload.length = static_cast<uint32_t>(ser.get_serialized_data_length());
    return true;
}

template <typename DeserializeFn>
bool run_deserialize(SerializedPayload_t& payload, DeserializeFn&& deserialize_body) {
    try {
        eprosima::fastcdr::FastBuffer fastbuffer(reinterpret_cast<char*>(payload.data),
                                                 payload.length);
        eprosima::fastcdr::Cdr deser(fastbuffer, eprosima::fastcdr::Cdr::DEFAULT_ENDIAN);
        deser.read_encapsulation();
        payload.encapsulation =
            deser.endianness() == eprosima::fastcdr::Cdr::BIG_ENDIANNESS ? CDR_BE : CDR_LE;
        deserialize_body(deser);
    } catch (eprosima::fastcdr::exception::Exception&) {
        return false;
    }
    return true;
}

}  // namespace

namespace umi_bridge {

// ---------------------------------------------------------------------------
// ImuPubSubType
// ---------------------------------------------------------------------------

ImuPubSubType::ImuPubSubType() {
    set_name(TYPE_NAME);
    // Imu has fixed worst-case size: Header.frame_id is the only variable
    // field, and we cap it at 1 KiB (operator-controlled in cora's static
    // configs). Set a generous bound so loaning works without per-sample
    // reallocation on the FastDDS history-cache side.
    constexpr uint32_t kImuMaxBytes = 4096;
    max_serialized_type_size = kImuMaxBytes;
    is_compute_key_provided = false;
}

bool ImuPubSubType::serialize(const void* const data, SerializedPayload_t& payload,
                              DataRepresentationId_t data_representation) {
    const auto* p_type = static_cast<const type*>(data);
    return run_serialize(payload, data_representation,
                         [p_type](auto& ser) { cdr_serialize_imu(ser, *p_type); });
}

bool ImuPubSubType::deserialize(SerializedPayload_t& payload, void* data) {
    auto* p_type = static_cast<type*>(data);
    return run_deserialize(payload, [p_type](auto& deser) { cdr_deserialize_imu(deser, *p_type); });
}

uint32_t ImuPubSubType::calculate_serialized_size(const void* const data,
                                                  DataRepresentationId_t data_representation) {
    try {
        eprosima::fastcdr::CdrSizeCalculator calculator(
            data_representation == DataRepresentationId_t::XCDR_DATA_REPRESENTATION
                ? eprosima::fastcdr::CdrVersion::XCDRv1
                : eprosima::fastcdr::CdrVersion::XCDRv2);
        const auto* p_type = static_cast<const type*>(data);
        size_t alignment{0};
        size_t total = 0;
        // Header: Time (4 + 4) + string (4 + len + 1)
        total += 8;
        total += 4 + p_type->header.frame_id.size() + 1;
        // Quaternion (4 doubles), 3 covariance arrays (9 doubles each), 2 Vector3 (3 doubles each)
        total += 8 * (4 + 9 + 3 + 9 + 3 + 9);
        (void)calculator;
        (void)alignment;
        return static_cast<uint32_t>(total + 4u /* encapsulation */);
    } catch (eprosima::fastcdr::exception::Exception&) {
        return 0;
    }
}

// ---------------------------------------------------------------------------
// CompressedVideoPubSubType
// ---------------------------------------------------------------------------

CompressedVideoPubSubType::CompressedVideoPubSubType() {
    set_name(TYPE_NAME);
    // The IDL bounds `data` at 4 MiB; add headroom for frame_id, format, and
    // CDR overhead so a 4 MiB payload fits without reallocation.
    constexpr uint32_t kVideoMaxBytes = 4u * 1024u * 1024u + 1024u;
    max_serialized_type_size = kVideoMaxBytes;
    is_compute_key_provided = false;
}

bool CompressedVideoPubSubType::serialize(const void* const data, SerializedPayload_t& payload,
                                          DataRepresentationId_t data_representation) {
    const auto* p_type = static_cast<const type*>(data);
    return run_serialize(payload, data_representation,
                         [p_type](auto& ser) { cdr_serialize_compressed_video(ser, *p_type); });
}

bool CompressedVideoPubSubType::deserialize(SerializedPayload_t& payload, void* data) {
    auto* p_type = static_cast<type*>(data);
    return run_deserialize(payload,
                           [p_type](auto& deser) { cdr_deserialize_compressed_video(deser, *p_type); });
}

uint32_t CompressedVideoPubSubType::calculate_serialized_size(
    const void* const data, DataRepresentationId_t data_representation) {
    try {
        const auto* p_type = static_cast<const type*>(data);
        // Time (8) + frame_id (4 + n + 1) + data (4 + N) + format (4 + n + 1)
        size_t total = 8;
        total += 4 + p_type->frame_id.size() + 1;
        total += 4 + p_type->data.size();
        total += 4 + p_type->format.size() + 1;
        (void)data_representation;
        return static_cast<uint32_t>(total + 4u /* encapsulation */);
    } catch (eprosima::fastcdr::exception::Exception&) {
        return 0;
    }
}

}  // namespace umi_bridge
