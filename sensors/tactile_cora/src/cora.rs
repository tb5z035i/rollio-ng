#![allow(dead_code)]

//! Safe Rust wrapper around the local Cora C++ shim. Exposes:
//!
//! * `Bridge::new(BridgeConfig)` — creates the DDS participant.
//! * `Bridge::start()` — starts the SDK CallbackExecutor.
//! * `Bridge::subscribe_point_cloud2(topic, qos, callback)` — registers a
//!   PointCloud2 subscription.
//!
//! Dropping the bridge tears everything down (stop + destroy + free callback boxes).

use std::ffi::{c_void, CString};
use std::os::raw::c_int;
use std::slice;

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
            participant_name: "rollio_tactile_cora".to_string(),
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

#[derive(Debug, Clone)]
pub struct PointField {
    pub name: String,
    pub offset: u32,
    /// `sensor_msgs::msg::PointField` datatype: INT8=1, UINT8=2, INT16=3,
    /// UINT16=4, INT32=5, UINT32=6, FLOAT32=7, FLOAT64=8.
    pub datatype: u8,
    pub count: u32,
}

#[derive(Debug, Clone)]
pub struct PointCloud2Sample {
    pub ts_us: u64,
    pub width: u32,
    pub height: u32,
    pub point_step: u32,
    pub row_step: u32,
    pub fields: Vec<PointField>,
    pub data: Vec<u8>,
    pub is_bigendian: bool,
    pub is_dense: bool,
}

type PointCloud2Cb = Box<dyn Fn(PointCloud2Sample) + Send + Sync>;

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
    callbacks: Mutex<Vec<*mut PointCloud2Cb>>,
}

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

    pub fn subscribe_point_cloud2<F>(
        &self,
        topic: &str,
        qos: Qos,
        callback: F,
    ) -> Result<Subscription>
    where
        F: Fn(PointCloud2Sample) + Send + Sync + 'static,
    {
        let c_topic = CString::new(topic)?;
        let boxed: PointCloud2Cb = Box::new(callback);
        let raw = Box::into_raw(Box::new(boxed));
        let id = unsafe {
            ffi::cora_bridge_subscribe_point_cloud2(
                self.ctx,
                c_topic.as_ptr(),
                qos.as_c_int(),
                Some(point_cloud2_trampoline),
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

extern "C" fn point_cloud2_trampoline(
    _sub_id: u32,
    ts_us: u64,
    width: u32,
    height: u32,
    point_step: u32,
    row_step: u32,
    fields: *const ffi::cora_point_field_t,
    n_fields: usize,
    data: *const u8,
    len: usize,
    is_bigendian: u8,
    is_dense: u8,
    user: *mut c_void,
) {
    if user.is_null() {
        return;
    }
    let cb: &PointCloud2Cb = unsafe { &*(user as *const PointCloud2Cb) };

    let mut owned_fields = Vec::with_capacity(n_fields);
    if !fields.is_null() && n_fields > 0 {
        let raw_fields = unsafe { slice::from_raw_parts(fields, n_fields) };
        for f in raw_fields {
            let name = if f.name.is_null() {
                String::new()
            } else {
                unsafe { std::ffi::CStr::from_ptr(f.name) }
                    .to_string_lossy()
                    .into_owned()
            };
            owned_fields.push(PointField {
                name,
                offset: f.offset,
                datatype: f.datatype,
                count: f.count,
            });
        }
    }

    let bytes = if data.is_null() || len == 0 {
        Vec::new()
    } else {
        unsafe { slice::from_raw_parts(data, len) }.to_vec()
    };

    cb(PointCloud2Sample {
        ts_us,
        width,
        height,
        point_step,
        row_step,
        fields: owned_fields,
        data: bytes,
        is_bigendian: is_bigendian != 0,
        is_dense: is_dense != 0,
    });
}
