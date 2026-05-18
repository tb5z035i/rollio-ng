#ifndef ROLLIO_DEVICES_CORACAM_CORA_SUBSCRIBER_HPP
#define ROLLIO_DEVICES_CORACAM_CORA_SUBSCRIBER_HPP

// cora_subscriber.hpp — Fast-DDS RTPS-layer subscriber for a single Cora topic.
//
// CoraSubscriber creates one RTPSParticipant per channel and registers an
// RTPSReader via register_reader() (RTPS layer, not DDS layer). This bypasses
// XTypes TypeObject validation that would otherwise silently reject a raw-bytes
// reader against a publisher that advertises a full TypeIdentifier.
//
// Received payloads (full CDR bytes including 4-byte encapsulation header) are
// queued via ReaderListener callback and delivered to the worker thread through
// take_next().
//
// Thread safety: take_next() is called from the worker thread only; stop()
// may be called from any thread.

#include <atomic>
#include <chrono>
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

namespace rollio::coracam {

// Received DDS sample.
struct CoraSample {
    // Full CDR bytes, including the 4-byte encapsulation header.
    // Parse with parse_cora_raw_image() or parse_cora_h264_packet().
    std::vector<uint8_t> payload;
    // Source timestamp from the DDS SampleInfo, UNIX microseconds.
    // May be zero if the publisher did not set it.
    uint64_t source_timestamp_us{0};
};

// RAII wrapper around a Fast-DDS DataReader for one Cora topic.
class CoraSubscriber {
public:
    // Construct and open a DDS subscriber.
    //
    // topic_name : DDS wire topic name, e.g. "rt/robot/camera/head/left/image"
    // type_name  : DDS type name that must match the publisher, e.g.
    //              "sensor_msgs::msg::dds_::Image_"
    // domain_id  : DDS domain (cora default is 0)
    //
    // Throws std::runtime_error if the DDS entities cannot be created.
    CoraSubscriber(const std::string& topic_name, const std::string& type_name, uint32_t domain_id);

    ~CoraSubscriber();

    CoraSubscriber(const CoraSubscriber&) = delete;
    CoraSubscriber& operator=(const CoraSubscriber&) = delete;

    // Block until a sample is available (or timeout / stop).
    // Returns true and fills 'out' when a valid sample was received.
    // Returns false on timeout or if stop() was called.
    bool take_next(CoraSample& out,
                   std::chrono::milliseconds timeout = std::chrono::milliseconds(200));

    // Signal the subscriber to stop. Thread-safe. After this call take_next()
    // will return false.
    void stop();

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
    std::atomic<bool> stop_flag_{false};
};

}  // namespace rollio::coracam

#endif  // ROLLIO_DEVICES_CORACAM_CORA_SUBSCRIBER_HPP
