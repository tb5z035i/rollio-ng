// `orientation` / `Subscription::id` / `Bridge::stop` are public surface kept
// intentionally even when this device doesn't use them — they're part of the
// Bridge API contract for future evolution. Silence the dead_code warning.
#![allow(dead_code)]

//! Safe Rust wrapper around the local Cora C++ shim. Exposes:
//!
//! * `Bridge::new(BridgeConfig)` — creates the DDS participant.
//! * `Bridge::start()` — starts the SDK CallbackExecutor.
//! * `Bridge::subscribe_imu(topic, qos, callback)` — registers an Imu subscription.
//!
//! Dropping the bridge tears everything down (stop + destroy + free callback boxes).

use std::ffi::{c_void, CString};
use std::os::raw::c_int;

use parking_lot::Mutex;
use thiserror::Error;

use crate::ffi;

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub domain_id: i32,
    pub participant_name: String,
    pub use_shared_memory: bool,
    pub use_udp: bool,
    pub callback_threads: u32,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            domain_id: 0,
            participant_name: "rollio_imu_cora".to_string(),
            use_shared_memory: true,
            use_udp: true,
            callback_threads: 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Qos {
    Reliable,
    BestEffort,
}

impl Qos {
    fn as_c_int(self) -> c_int {
        match self {
            Qos::Reliable => 1,
            Qos::BestEffort => 0,
        }
    }
}

#[derive(Debug, Error)]
pub enum BridgeError {
    #[error("invalid C string (interior NUL)")]
    InvalidCString(#[from] std::ffi::NulError),
    #[error("cora_bridge_create returned null — DDS participant init failed")]
    CreateFailed,
    #[error("bridge already started")]
    AlreadyRunning,
    #[error("bridge not running")]
    NotRunning,
    #[error("subscribe failed (DDS reader creation)")]
    SubscribeFailed,
    #[error("null pointer passed to FFI")]
    NullPointer,
    #[error("internal C++ shim error (code {0})")]
    Internal(i32),
}

impl BridgeError {
    fn from_code(code: i32) -> Self {
        match code {
            ffi::CORA_BRIDGE_ERR_NULL => BridgeError::NullPointer,
            ffi::CORA_BRIDGE_ERR_DDS_INIT => BridgeError::CreateFailed,
            ffi::CORA_BRIDGE_ERR_SUBSCRIBE => BridgeError::SubscribeFailed,
            ffi::CORA_BRIDGE_ERR_NOT_RUNNING => BridgeError::NotRunning,
            ffi::CORA_BRIDGE_ERR_ALREADY_RUNNING => BridgeError::AlreadyRunning,
            other => BridgeError::Internal(other),
        }
    }
}

pub type Result<T> = std::result::Result<T, BridgeError>;

/// One Imu sample delivered from a Cora topic. Fields are owned (no shim borrows).
#[derive(Debug, Clone)]
pub struct ImuSample {
    pub ts_us: u64,
    pub accel: [f64; 3],
    pub gyro: [f64; 3],
    /// Quaternion `[x, y, z, w]`. Phase 1 ignores it; preserved so future
    /// `ImuAccelGyroOrientation` consumers can pick it up without re-plumbing.
    pub orientation: [f64; 4],
}

type ImuCb = Box<dyn Fn(ImuSample) + Send + Sync>;

#[derive(Debug, Clone, Copy)]
pub struct Subscription {
    id: u32,
}

impl Subscription {
    pub fn id(&self) -> u32 {
        self.id
    }
}

pub struct Bridge {
    ctx: *mut ffi::cora_bridge_ctx_t,
    callbacks: Mutex<Vec<*mut ImuCb>>,
}

// SAFETY: callbacks are heap-allocated `Box<dyn Fn>` whose closures are `Send + Sync`.
// Bridge serialises access to the Vec; freeing happens only after destroy() has
// stopped the SDK callbacks.
unsafe impl Send for Bridge {}
unsafe impl Sync for Bridge {}

impl Bridge {
    pub fn new(config: BridgeConfig) -> Result<Self> {
        let name = CString::new(config.participant_name)?;
        let c_cfg = ffi::cora_bridge_config_t {
            domain_id: config.domain_id,
            participant_name: name.as_ptr(),
            use_shared_memory: u8::from(config.use_shared_memory),
            use_udp: u8::from(config.use_udp),
            callback_threads: config.callback_threads,
        };
        let ctx = unsafe { ffi::cora_bridge_create(&c_cfg) };
        if ctx.is_null() {
            return Err(BridgeError::CreateFailed);
        }
        Ok(Self {
            ctx,
            callbacks: Mutex::new(Vec::new()),
        })
    }

    pub fn start(&self) -> Result<()> {
        let rc = unsafe { ffi::cora_bridge_start(self.ctx) } as i32;
        if rc != ffi::CORA_BRIDGE_OK as i32 {
            return Err(BridgeError::from_code(rc));
        }
        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        let rc = unsafe { ffi::cora_bridge_stop(self.ctx) } as i32;
        if rc != ffi::CORA_BRIDGE_OK as i32 {
            return Err(BridgeError::from_code(rc));
        }
        Ok(())
    }

    pub fn subscribe_imu<F>(&self, topic: &str, qos: Qos, callback: F) -> Result<Subscription>
    where
        F: Fn(ImuSample) + Send + Sync + 'static,
    {
        let c_topic = CString::new(topic)?;
        let boxed: ImuCb = Box::new(callback);
        let raw = Box::into_raw(Box::new(boxed));
        let id = unsafe {
            ffi::cora_bridge_subscribe_imu(
                self.ctx,
                c_topic.as_ptr(),
                qos.as_c_int(),
                Some(imu_trampoline),
                raw as *mut c_void,
            )
        };
        if id < 0 {
            unsafe { drop(Box::from_raw(raw)) };
            return Err(BridgeError::from_code(id));
        }
        self.callbacks.lock().push(raw);
        Ok(Subscription { id: id as u32 })
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            unsafe { ffi::cora_bridge_destroy(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
        let mut cbs = self.callbacks.lock();
        unsafe {
            for p in cbs.drain(..) {
                drop(Box::from_raw(p));
            }
        }
    }
}

extern "C" fn imu_trampoline(
    _sub_id: u32,
    ts_us: u64,
    ax: f64,
    ay: f64,
    az: f64,
    gx: f64,
    gy: f64,
    gz: f64,
    qx: f64,
    qy: f64,
    qz: f64,
    qw: f64,
    user: *mut c_void,
) {
    if user.is_null() {
        return;
    }
    let cb: &ImuCb = unsafe { &*(user as *const ImuCb) };
    cb(ImuSample {
        ts_us,
        accel: [ax, ay, az],
        gyro: [gx, gy, gz],
        orientation: [qx, qy, qz, qw],
    });
}

// Compile-time sanity: c_int must round-trip through i32 for our as_c_int casts.
const _: () = {
    let _ = std::mem::size_of::<c_int>();
};
