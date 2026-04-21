//! Loss-point-A regression: verify the bumped iceoryx2 ring depth
//! actually buffers 250 Hz state samples while the consumer is stalled.
//!
//! This is the integration-level proof that Phase 6a + Phase 6b together
//! eliminate the silent-overwrite window we measured before the fix:
//!
//! * Producer publishes ~250 samples back-to-back into a `JointVector15`
//!   service opened with `STATE_BUFFER = 1024` (matching the helpers used
//!   by every robot driver and the assembler).
//! * Consumer, running in another thread, sleeps for 1 second to mimic
//!   `stage_episode` blocking the main loop, then drains the subscriber.
//!
//! With the iceoryx2 default `subscriber_max_buffer_size = 2`, we would
//! see at most 2 samples after the sleep. With 1024 we expect to see all
//! 250.
//!
//! The test is gated behind `--test-threads=1` indirectly: iceoryx2's
//! shared-memory backplane is per-host and per-service-name, so to avoid
//! collisions with other tests we use a per-test unique service name.

use iceoryx2::prelude::*;
use rollio_bus::{STATE_BUFFER, STATE_MAX_NODES, STATE_MAX_PUBLISHERS, STATE_MAX_SUBSCRIBERS};
use rollio_types::messages::JointVector15;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TARGET_SAMPLES: usize = 250;

#[test]
fn state_buffer_absorbs_250_hz_burst_while_consumer_sleeps_one_second() {
    let unique_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let service_name_str = format!("test/loss_point_a/{unique_suffix}");

    let (ready_tx, ready_rx) = mpsc::channel::<()>();
    let (count_tx, count_rx) = mpsc::channel::<usize>();

    // ----------------------------------------------------------------------
    // Consumer: create its OWN node + subscriber (the iceoryx2 subscriber
    // handle is `!Send`, so it cannot move across threads). The consumer
    // signals readiness, sleeps for 1 second to mimic `stage_episode`
    // blocking the assembler main loop, then drains the queue.
    // ----------------------------------------------------------------------
    let consumer_service_name = service_name_str.clone();
    let consumer_handle = thread::Builder::new()
        .name("consumer".into())
        .spawn(move || -> usize {
            let node = NodeBuilder::new()
                .signal_handling_mode(SignalHandlingMode::Disabled)
                .create::<ipc::Service>()
                .expect("consumer node should create");
            let service_name: ServiceName = consumer_service_name
                .as_str()
                .try_into()
                .expect("service name should validate");
            let service = node
                .service_builder(&service_name)
                .publish_subscribe::<JointVector15>()
                .subscriber_max_buffer_size(STATE_BUFFER)
                .history_size(STATE_BUFFER)
                .max_publishers(STATE_MAX_PUBLISHERS)
                .max_subscribers(STATE_MAX_SUBSCRIBERS)
                .max_nodes(STATE_MAX_NODES)
                .open_or_create()
                .expect("consumer service should open or create");
            let subscriber = service
                .subscriber_builder()
                .create()
                .expect("subscriber should create");

            ready_tx.send(()).expect("ready signal should send");

            // Mimic `stage_episode` blocking the main loop for 1 s.
            thread::sleep(Duration::from_secs(1));

            // Drain. Loop briefly past the sleep to catch any in-flight
            // samples (publisher's writes may race the wake-up).
            let drain_deadline = std::time::Instant::now() + Duration::from_secs(2);
            let mut total = 0_usize;
            while std::time::Instant::now() < drain_deadline {
                let mut drained = 0_usize;
                loop {
                    let sample = subscriber
                        .receive()
                        .expect("subscriber receive should not error");
                    if sample.is_none() {
                        break;
                    }
                    drained += 1;
                }
                total += drained;
                if total >= TARGET_SAMPLES {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            count_tx.send(total).expect("count send should succeed");
            total
        })
        .expect("consumer thread should spawn");

    // Wait for the consumer to open the service before publishing.
    ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("consumer should signal readiness");

    // ----------------------------------------------------------------------
    // Producer (main thread): publish 250 samples back-to-back. Same caps.
    // ----------------------------------------------------------------------
    let producer_node = NodeBuilder::new()
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<ipc::Service>()
        .expect("producer node should create");
    let producer_service_name: ServiceName = service_name_str
        .as_str()
        .try_into()
        .expect("service name should validate");
    let producer_service = producer_node
        .service_builder(&producer_service_name)
        .publish_subscribe::<JointVector15>()
        .subscriber_max_buffer_size(STATE_BUFFER)
        .history_size(STATE_BUFFER)
        .max_publishers(STATE_MAX_PUBLISHERS)
        .max_subscribers(STATE_MAX_SUBSCRIBERS)
        .max_nodes(STATE_MAX_NODES)
        .open_or_create()
        .expect("producer service should open or create");
    let publisher = producer_service
        .publisher_builder()
        .create()
        .expect("publisher should create");

    for index in 0..TARGET_SAMPLES {
        let sample = JointVector15::from_slice(index as u64, &[index as f64]);
        publisher
            .send_copy(sample)
            .expect("publisher send_copy should succeed");
    }

    // Wait for the consumer to report its drained count, then join.
    let drained = count_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("consumer should report a count");
    consumer_handle.join().expect("consumer thread should join");

    // With the old default `subscriber_max_buffer_size = 2` the consumer
    // would only see at most 2 samples here. With STATE_BUFFER = 1024 we
    // expect every published sample to make it through.
    assert_eq!(
        drained, TARGET_SAMPLES,
        "consumer should drain every {TARGET_SAMPLES} published samples; got {drained}"
    );
}
