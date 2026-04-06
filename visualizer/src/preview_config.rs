use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

const MIN_PREVIEW_DIMENSION: u32 = 1;
const MAX_PREVIEW_DIMENSION: u32 = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreviewSize {
    pub width: u32,
    pub height: u32,
}

impl PreviewSize {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width: clamp_preview_dimension(width),
            height: clamp_preview_dimension(height),
        }
    }
}

#[derive(Debug)]
pub struct RuntimePreviewConfig {
    default_size: PreviewSize,
    active_width: AtomicU32,
    active_height: AtomicU32,
    connected_clients: AtomicUsize,
}

impl RuntimePreviewConfig {
    pub fn new(default_width: u32, default_height: u32) -> Self {
        let default_size = PreviewSize::new(default_width, default_height);
        Self {
            default_size,
            active_width: AtomicU32::new(default_size.width),
            active_height: AtomicU32::new(default_size.height),
            connected_clients: AtomicUsize::new(0),
        }
    }

    pub fn current_size(&self) -> PreviewSize {
        PreviewSize {
            width: self.active_width.load(Ordering::Relaxed),
            height: self.active_height.load(Ordering::Relaxed),
        }
    }

    pub fn set_requested_size(&self, width: u32, height: u32) -> PreviewSize {
        let next = PreviewSize::new(width, height);
        self.active_width.store(next.width, Ordering::Relaxed);
        self.active_height.store(next.height, Ordering::Relaxed);
        next
    }

    pub fn reset_to_default(&self) -> PreviewSize {
        self.set_requested_size(self.default_size.width, self.default_size.height)
    }

    pub fn client_connected(&self) {
        self.connected_clients.fetch_add(1, Ordering::Relaxed);
    }

    pub fn client_disconnected(&self) -> Option<PreviewSize> {
        let mut current = self.connected_clients.load(Ordering::Relaxed);
        loop {
            if current == 0 {
                return None;
            }
            match self.connected_clients.compare_exchange(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    if current == 1 {
                        return Some(self.reset_to_default());
                    }
                    return None;
                }
                Err(observed) => current = observed,
            }
        }
    }
}

fn clamp_preview_dimension(value: u32) -> u32 {
    value.clamp(MIN_PREVIEW_DIMENSION, MAX_PREVIEW_DIMENSION)
}

#[cfg(test)]
mod tests {
    use super::{PreviewSize, RuntimePreviewConfig};

    #[test]
    fn preview_size_clamps_invalid_values() {
        assert_eq!(
            PreviewSize::new(0, 10_000),
            PreviewSize {
                width: 1,
                height: 4096,
            }
        );
    }

    #[test]
    fn reset_only_happens_when_last_client_disconnects() {
        let config = RuntimePreviewConfig::new(320, 240);
        config.client_connected();
        config.client_connected();
        config.set_requested_size(800, 600);

        assert_eq!(config.client_disconnected(), None);
        assert_eq!(config.current_size(), PreviewSize::new(800, 600));
        assert_eq!(
            config.client_disconnected(),
            Some(PreviewSize::new(320, 240))
        );
        assert_eq!(config.current_size(), PreviewSize::new(320, 240));
    }
}
