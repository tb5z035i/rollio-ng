#include "cora_subscriber.hpp"

#include <condition_variable>
#include <iostream>
#include <mutex>
#include <queue>
#include <stdexcept>
#include <string_view>
#include <vector>

// Fast-DDS RTPS-layer headers only.
// We deliberately avoid the DDS-layer DomainParticipant / DataReader path
// because Fast-DDS 3.x runs XTypes TypeObject validation during EDP endpoint
// matching: a DataReader backed by a raw-bytes TypeSupport (no TypeObject)
// against a DataWriter that advertises a full TypeIdentifier will be silently
// rejected even when TypeConsistencyEnforcementQos is maximally relaxed.
//
// The RTPS-layer registerReader() path compares only topic-name + type-name
// strings during EDP, completely bypassing XTypes — same mechanism used by
// the airrtc CdrRtpsReader reference implementation.
#include <fastdds/dds/subscriber/qos/ReaderQos.hpp>
#include <fastdds/rtps/RTPSDomain.hpp>
#include <fastdds/rtps/attributes/HistoryAttributes.hpp>
#include <fastdds/rtps/attributes/RTPSParticipantAttributes.hpp>
#include <fastdds/rtps/attributes/ReaderAttributes.hpp>
#include <fastdds/rtps/builtin/data/PublicationBuiltinTopicData.hpp>
#include <fastdds/rtps/builtin/data/TopicDescription.hpp>
#include <fastdds/rtps/common/CacheChange.hpp>
#include <fastdds/rtps/common/MatchingInfo.hpp>
#include <fastdds/rtps/history/ReaderHistory.hpp>
#include <fastdds/rtps/participant/RTPSParticipant.hpp>
#include <fastdds/rtps/participant/RTPSParticipantListener.hpp>
#include <fastdds/rtps/reader/RTPSReader.hpp>
#include <fastdds/rtps/reader/ReaderListener.hpp>
#include <fastdds/rtps/writer/WriterDiscoveryStatus.hpp>

using namespace eprosima::fastdds::rtps;
using namespace eprosima::fastdds::dds;

namespace rollio::coracam {

namespace {

auto reliability_to_string(ReliabilityQosPolicyKind kind) -> const char* {
    return kind == RELIABLE_RELIABILITY_QOS ? "reliable" : "best-effort";
}

auto durability_to_string(DurabilityQosPolicyKind kind) -> const char* {
    return kind == TRANSIENT_LOCAL_DURABILITY_QOS ? "transient-local" : "volatile";
}

auto writer_status_to_string(WriterDiscoveryStatus status) -> const char* {
    switch (status) {
        case WriterDiscoveryStatus::DISCOVERED_WRITER:
            return "discovered";
        case WriterDiscoveryStatus::CHANGED_QOS_WRITER:
            return "changed-qos";
        case WriterDiscoveryStatus::REMOVED_WRITER:
            return "removed";
        case WriterDiscoveryStatus::IGNORED_WRITER:
            return "ignored";
    }
    return "unknown";
}

auto topic_matches_interest(std::string_view subscribed, std::string_view discovered) -> bool {
    if (subscribed == discovered) {
        return true;
    }
    if (!subscribed.empty() && subscribed.front() != '/' &&
        discovered.size() == subscribed.size() + 1U && discovered.front() == '/' &&
        discovered.substr(1) == subscribed) {
        return true;
    }
    if (!discovered.empty() && discovered.front() != '/' &&
        subscribed.size() == discovered.size() + 1U && subscribed.front() == '/' &&
        subscribed.substr(1) == discovered) {
        return true;
    }
    return false;
}

}  // namespace

// ---------------------------------------------------------------------------
// Impl
// ---------------------------------------------------------------------------

struct CoraSubscriber::Impl {
    class ParticipantListener : public RTPSParticipantListener {
    public:
        explicit ParticipantListener(CoraSubscriber::Impl* owner) : owner_(owner) {}

        void on_writer_discovery(RTPSParticipant* /*participant*/, WriterDiscoveryStatus reason,
                                 const PublicationBuiltinTopicData& info,
                                 bool& should_be_ignored) override {
            should_be_ignored = false;
            const auto topic = info.topic_name.to_string();
            if (!topic_matches_interest(owner_->topic_name_, topic)) {
                return;
            }
            std::cerr << "[coracam] writer " << writer_status_to_string(reason)
                      << " subscribed_topic=" << owner_->topic_name_ << " writer_topic=" << topic
                      << " writer_type=" << info.type_name.to_string()
                      << " reliability=" << reliability_to_string(info.reliability.kind)
                      << " durability=" << durability_to_string(info.durability.kind) << '\n';
        }

    private:
        CoraSubscriber::Impl* owner_;
    };

    // -----------------------------------------------------------------------
    // ReaderListener: called by Fast-DDS from its own thread when new data
    // arrives on the RTPS reader. Copies the payload into the queue and wakes
    // the take_next() caller, then removes the change from history.
    // -----------------------------------------------------------------------
    class Listener : public ReaderListener {
    public:
        explicit Listener(CoraSubscriber::Impl* owner) : owner_(owner) {}

        void on_new_cache_change_added(RTPSReader* reader,
                                       const CacheChange_t* const change) override {
            if (!change || !change->serializedPayload.data ||
                change->serializedPayload.length == 0) {
                if (reader && change) {
                    reader->get_history()->remove_change(const_cast<CacheChange_t*>(change));
                }
                return;
            }

            // Compute source timestamp (UNIX microseconds).
            const int64_t ts_ns = change->sourceTimestamp.to_ns();
            const uint64_t ts_us = (ts_ns > 0) ? static_cast<uint64_t>(ts_ns / 1000) : 0;

            // Copy payload bytes.
            CoraSample sample;
            sample.payload.assign(
                change->serializedPayload.data,
                change->serializedPayload.data + change->serializedPayload.length);
            sample.source_timestamp_us = ts_us;

            // Release the cache change before notifying the consumer so
            // the RTPS reader history doesn't accumulate unbounded entries.
            reader->get_history()->remove_change(const_cast<CacheChange_t*>(change));

            {
                std::lock_guard<std::mutex> lock(owner_->queue_mutex_);
                owner_->queue_.push(std::move(sample));
            }
            owner_->queue_cv_.notify_one();
        }

        void on_reader_matched(RTPSReader* /*reader*/, const MatchingInfo& info) override {
            if (info.status == MATCHED_MATCHING) {
                std::cerr << "[coracam] matched publisher topic=" << owner_->topic_name_ << '\n';
            } else {
                std::cerr << "[coracam] unmatched publisher topic=" << owner_->topic_name_ << '\n';
            }
        }

        void on_requested_incompatible_qos(RTPSReader* /*reader*/, PolicyMask qos) override {
            std::cerr << "[coracam] incompatible writer QoS topic=" << owner_->topic_name_
                      << " mask=" << qos << '\n';
        }

    private:
        CoraSubscriber::Impl* owner_;
    };

    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    Impl(const std::string& topic_name, const std::string& type_name, uint32_t domain_id)
        : topic_name_(topic_name), participant_listener_(this), listener_(this) {
        // Participant
        RTPSParticipantAttributes pattrs;
        pattrs.setName("rollio_coracam");
        participant_ = RTPSDomain::createParticipant(domain_id, pattrs, &participant_listener_);
        if (!participant_) {
            throw std::runtime_error("[coracam] failed to create RTPSParticipant for topic " +
                                     topic_name);
        }

        // History — generous 8 MiB payload cap, keep a shallow ring of 4.
        HistoryAttributes hattrs;
        hattrs.memoryPolicy = PREALLOCATED_WITH_REALLOC_MEMORY_MODE;
        hattrs.payloadMaxSize = 8U * 1024U * 1024U + 4U;
        hattrs.initialReservedCaches = 4;
        hattrs.maximumReservedCaches = 4;
        history_ = new ReaderHistory(hattrs);

        // Reader attributes
        ReaderAttributes rattrs;
        rattrs.endpoint.reliabilityKind = BEST_EFFORT;
        rattrs.endpoint.durabilityKind = VOLATILE;
        rattrs.endpoint.topicKind = NO_KEY;

        reader_ = RTPSDomain::createRTPSReader(participant_, rattrs, history_, &listener_);
        if (!reader_) {
            RTPSDomain::removeRTPSParticipant(participant_);
            participant_ = nullptr;
            delete history_;
            history_ = nullptr;
            throw std::runtime_error("[coracam] failed to create RTPSReader for topic " +
                                     topic_name);
        }

        // Register reader: only topic-name + type-name strings.
        // type_information is left default (assigned_=false) so Fast-DDS skips
        // XTypes TypeObject validation and matches purely on name strings.
        TopicDescription topic_desc;
        topic_desc.topic_name = topic_name.c_str();
        topic_desc.type_name = type_name.c_str();
        // topic_desc.type_information is default-constructed (unassigned)

        ReaderQos rqos;
        rqos.m_reliability.kind = BEST_EFFORT_RELIABILITY_QOS;
        rqos.m_durability.kind = VOLATILE_DURABILITY_QOS;
        rqos.representation.m_value = {
            XCDR_DATA_REPRESENTATION,
            XCDR2_DATA_REPRESENTATION,
        };

        if (!participant_->register_reader(reader_, topic_desc, rqos)) {
            RTPSDomain::removeRTPSReader(reader_);
            reader_ = nullptr;
            RTPSDomain::removeRTPSParticipant(participant_);
            participant_ = nullptr;
            delete history_;
            history_ = nullptr;
            throw std::runtime_error("[coracam] failed to register RTPSReader for topic " +
                                     topic_name);
        }

        std::cerr << "[coracam] subscriber ready topic=" << topic_name << " type=" << type_name
                  << " domain=" << domain_id << '\n';
    }

    ~Impl() {
        cleanup();
    }

    void cleanup() {
        if (reader_) {
            RTPSDomain::removeRTPSReader(reader_);
            reader_ = nullptr;
        }
        if (participant_) {
            RTPSDomain::removeRTPSParticipant(participant_);
            participant_ = nullptr;
        }
        // ReaderHistory is owned by the participant/reader stack; after
        // removeRTPSReader it must not be used, but the memory is ours to free.
        delete history_;
        history_ = nullptr;
    }

    std::string topic_name_;
    ParticipantListener participant_listener_;
    Listener listener_;
    RTPSParticipant* participant_{nullptr};
    ReaderHistory* history_{nullptr};
    RTPSReader* reader_{nullptr};

    std::mutex queue_mutex_;
    std::condition_variable queue_cv_;
    std::queue<CoraSample> queue_;
};

// ---------------------------------------------------------------------------
// CoraSubscriber
// ---------------------------------------------------------------------------

CoraSubscriber::CoraSubscriber(const std::string& topic_name, const std::string& type_name,
                               uint32_t domain_id)
    : impl_(std::make_unique<Impl>(topic_name, type_name, domain_id)) {}

CoraSubscriber::~CoraSubscriber() = default;

bool CoraSubscriber::take_next(CoraSample& out, std::chrono::milliseconds timeout) {
    std::unique_lock<std::mutex> lock(impl_->queue_mutex_);
    const bool got = impl_->queue_cv_.wait_for(lock, timeout, [this] {
        return !impl_->queue_.empty() || stop_flag_.load(std::memory_order_acquire);
    });
    if (!got || stop_flag_.load(std::memory_order_acquire)) {
        return false;
    }
    out = std::move(impl_->queue_.front());
    impl_->queue_.pop();
    return true;
}

void CoraSubscriber::stop() {
    stop_flag_.store(true, std::memory_order_release);
    impl_->queue_cv_.notify_all();
}

}  // namespace rollio::coracam
