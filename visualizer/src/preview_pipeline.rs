use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use rollio_types::messages::CameraFrameHeader;
use tokio::sync::broadcast;

use crate::jpeg::JpegCompressor;
use crate::preview_config::RuntimePreviewConfig;
use crate::protocol;
use crate::stream_info::StreamInfoRegistry;
use crate::websocket::BroadcastMessage;

const WORKER_POLL_TIMEOUT: Duration = Duration::from_millis(50);

pub struct PreviewPipeline {
    camera_states: Arc<Mutex<HashMap<String, CameraPreviewState>>>,
    work_tx: Sender<String>,
    shutdown: Arc<AtomicBool>,
    worker_handles: Vec<JoinHandle<()>>,
}

#[derive(Default)]
struct CameraPreviewState {
    latest_frame: Option<PendingPreviewFrame>,
    queued_or_processing: bool,
}

struct PendingPreviewFrame {
    header: CameraFrameHeader,
    data: Vec<u8>,
}

impl PreviewPipeline {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        camera_names: &[String],
        worker_count: usize,
        preview_config: Arc<RuntimePreviewConfig>,
        jpeg_quality: i32,
        broadcast_tx: broadcast::Sender<BroadcastMessage>,
        stream_info: Arc<Mutex<StreamInfoRegistry>>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut initial_states = HashMap::with_capacity(camera_names.len());
        for camera_name in camera_names {
            initial_states.insert(camera_name.clone(), CameraPreviewState::default());
        }

        let camera_states = Arc::new(Mutex::new(initial_states));
        let (work_tx, work_rx) = crossbeam_channel::unbounded::<String>();
        let shutdown = Arc::new(AtomicBool::new(false));
        let mut worker_handles = Vec::with_capacity(worker_count.max(1));

        for worker_idx in 0..worker_count.max(1) {
            let worker_name = format!("rollio-visualizer-preview-{worker_idx}");
            let worker_states = Arc::clone(&camera_states);
            let worker_tx = work_tx.clone();
            let worker_rx = work_rx.clone();
            let worker_shutdown = Arc::clone(&shutdown);
            let worker_preview_config = Arc::clone(&preview_config);
            let worker_broadcast_tx = broadcast_tx.clone();
            let worker_stream_info = Arc::clone(&stream_info);

            let handle = thread::Builder::new().name(worker_name).spawn(move || {
                preview_worker_loop(
                    worker_idx,
                    worker_rx,
                    worker_tx,
                    worker_states,
                    worker_shutdown,
                    worker_preview_config,
                    jpeg_quality,
                    worker_broadcast_tx,
                    worker_stream_info,
                );
            })?;
            worker_handles.push(handle);
        }

        Ok(Self {
            camera_states,
            work_tx,
            shutdown,
            worker_handles,
        })
    }

    pub fn submit_frame(&self, camera_name: String, header: CameraFrameHeader, data: Vec<u8>) {
        let mut should_enqueue = false;
        {
            let mut camera_states = self
                .camera_states
                .lock()
                .expect("preview camera state mutex poisoned");
            let camera_state = camera_states.entry(camera_name.clone()).or_default();
            camera_state.latest_frame = Some(PendingPreviewFrame { header, data });
            if !camera_state.queued_or_processing {
                camera_state.queued_or_processing = true;
                should_enqueue = true;
            }
        }

        if should_enqueue {
            let _ = self.work_tx.send(camera_name);
        }
    }
}

impl Drop for PreviewPipeline {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        for handle in self.worker_handles.drain(..) {
            let _ = handle.join();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn preview_worker_loop(
    worker_idx: usize,
    work_rx: Receiver<String>,
    work_tx: Sender<String>,
    camera_states: Arc<Mutex<HashMap<String, CameraPreviewState>>>,
    shutdown: Arc<AtomicBool>,
    preview_config: Arc<RuntimePreviewConfig>,
    jpeg_quality: i32,
    broadcast_tx: broadcast::Sender<BroadcastMessage>,
    stream_info: Arc<Mutex<StreamInfoRegistry>>,
) {
    let mut compressor = match JpegCompressor::new(jpeg_quality) {
        Ok(compressor) => compressor,
        Err(error) => {
            log::error!("preview worker {worker_idx} failed to initialize compressor: {error}");
            return;
        }
    };

    while !shutdown.load(Ordering::Relaxed) {
        let camera_name = match work_rx.recv_timeout(WORKER_POLL_TIMEOUT) {
            Ok(camera_name) => camera_name,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        };

        let pending_frame = take_latest_frame(&camera_states, &camera_name);
        if let Some(pending_frame) = pending_frame {
            let preview_size = preview_config.current_size();
            match compressor.compress(
                &pending_frame.data,
                pending_frame.header.width,
                pending_frame.header.height,
                preview_size.width,
                preview_size.height,
            ) {
                Ok(preview) => {
                    let encoded = protocol::encode_camera_frame(
                        &camera_name,
                        pending_frame.header.timestamp_ns,
                        pending_frame.header.frame_index,
                        preview.width,
                        preview.height,
                        preview.jpeg_data,
                    );
                    if let Ok(mut info) = stream_info.lock() {
                        info.observe_published_frame(&camera_name);
                    }
                    let _ = broadcast_tx.send(BroadcastMessage::Binary(Arc::new(encoded)));
                }
                Err(error) => {
                    log::warn!(
                        "preview worker {worker_idx} failed to compress {camera_name}: {error}"
                    );
                }
            }
        }

        finish_processing(&camera_states, &work_tx, &shutdown, camera_name);
    }
}

fn take_latest_frame(
    camera_states: &Arc<Mutex<HashMap<String, CameraPreviewState>>>,
    camera_name: &str,
) -> Option<PendingPreviewFrame> {
    let mut camera_states = camera_states
        .lock()
        .expect("preview camera state mutex poisoned");
    let camera_state = camera_states.get_mut(camera_name)?;
    camera_state.latest_frame.take()
}

fn finish_processing(
    camera_states: &Arc<Mutex<HashMap<String, CameraPreviewState>>>,
    work_tx: &Sender<String>,
    shutdown: &Arc<AtomicBool>,
    camera_name: String,
) {
    let mut should_requeue = false;
    {
        let mut camera_states = camera_states
            .lock()
            .expect("preview camera state mutex poisoned");
        if let Some(camera_state) = camera_states.get_mut(&camera_name) {
            if camera_state.latest_frame.is_some() {
                should_requeue = true;
            } else {
                camera_state.queued_or_processing = false;
            }
        }
    }

    if should_requeue && !shutdown.load(Ordering::Relaxed) {
        let _ = work_tx.send(camera_name);
    }
}
