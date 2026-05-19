use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;

const ERROR_BUF_SIZE: usize = 1024;

#[repr(C)]
pub struct DlUploadConfig {
    pub max_retries: c_int,
    pub retry_delay_seconds: f64,
    pub multipart_threshold: usize,
    pub part_size: usize,
    pub max_workers: c_int,
}

impl Default for DlUploadConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            retry_delay_seconds: 1.0,
            multipart_threshold: 8 * 1024 * 1024,
            part_size: 8 * 1024 * 1024,
            max_workers: 4,
        }
    }
}

#[repr(C)]
pub struct DlUploadResult {
    pub episode_id: *const c_char,
    pub file_url: *const c_char,
    pub file_md5: *const c_char,
    pub file_size: usize,
    pub skipped: c_int,
}

#[repr(C)]
pub struct DlEpisodeInfo {
    pub episode_id: *const c_char,
    pub project_id: *const c_char,
    pub episode_type: *const c_char,
    pub create_user: *const c_char,
    pub creation_time: *const c_char,
    pub update_time: *const c_char,
    pub datapoints_count: i64,
    pub tags_json: *const c_char,
    pub bucket: *const c_char,
    pub episode_path: *const c_char,
    pub file_url: *const c_char,
    pub file_md5: *const c_char,
    pub file_size: usize,
}

// PLACEHOLDER_FFI_EXTERN

extern "C" {
    pub fn dl_client_create(
        base_url: *const c_char,
        token: *const c_char,
        config: *const DlUploadConfig,
    ) -> *mut c_void;

    pub fn dl_client_destroy(handle: *mut c_void);

    pub fn dl_client_create_episode(
        handle: *mut c_void,
        file_path: *const c_char,
        project_id: *const c_char,
        tags_json: *const c_char,
        labels_json: *const c_char,
        force_overwrite: c_int,
        callback: Option<unsafe extern "C" fn(*const c_void, *mut c_void)>,
        user_data: *mut c_void,
        out_result: *mut DlUploadResult,
        error_buf: *mut c_char,
        error_buf_size: usize,
    ) -> c_int;

    pub fn dl_client_get_episode(
        handle: *mut c_void,
        episode_id: *const c_char,
        out_episode: *mut DlEpisodeInfo,
        error_buf: *mut c_char,
        error_buf_size: usize,
    ) -> c_int;

    pub fn dl_client_upload_to_episode(
        handle: *mut c_void,
        paths: *const *const c_char,
        path_count: c_int,
        episode_path: *const c_char,
        bucket: *const c_char,
        sub_dir: *const c_char,
        remote_filename: *const c_char,
        remote_dirname: *const c_char,
        force_overwrite: c_int,
        callback: Option<unsafe extern "C" fn(*const c_void, *mut c_void)>,
        user_data: *mut c_void,
        out_succeeded: *mut c_int,
        out_failed: *mut c_int,
        out_skipped: *mut c_int,
        error_buf: *mut c_char,
        error_buf_size: usize,
    ) -> c_int;

    pub fn dl_free_episode_info(episode: *mut DlEpisodeInfo);

    pub fn dl_get_last_error() -> *const c_char;
}

// PLACEHOLDER_FFI_WRAPPER

pub struct DataloopClient {
    handle: *mut c_void,
}

unsafe impl Send for DataloopClient {}

impl DataloopClient {
    pub fn new(base_url: &str, token: &str) -> Result<Self, String> {
        let base_url_c = CString::new(base_url).map_err(|e| e.to_string())?;
        let token_c = CString::new(token).map_err(|e| e.to_string())?;
        let config = DlUploadConfig::default();
        let handle = unsafe { dl_client_create(base_url_c.as_ptr(), token_c.as_ptr(), &config) };
        if handle.is_null() {
            return Err(last_error_string());
        }
        Ok(Self { handle })
    }

    pub fn create_episode(
        &self,
        file_path: &Path,
        project_id: &str,
        tags_json: &str,
    ) -> Result<CreateEpisodeResult, String> {
        let file_path_c = path_to_cstring(file_path)?;
        let project_id_c = CString::new(project_id).map_err(|e| e.to_string())?;
        let tags_json_c = CString::new(tags_json).map_err(|e| e.to_string())?;
        let mut out_result = std::mem::MaybeUninit::<DlUploadResult>::zeroed();
        let mut error_buf = vec![0u8; ERROR_BUF_SIZE];

        let rc = unsafe {
            dl_client_create_episode(
                self.handle,
                file_path_c.as_ptr(),
                project_id_c.as_ptr(),
                tags_json_c.as_ptr(),
                std::ptr::null(),
                0,
                None,
                std::ptr::null_mut(),
                out_result.as_mut_ptr(),
                error_buf.as_mut_ptr() as *mut c_char,
                ERROR_BUF_SIZE,
            )
        };

        // PLACEHOLDER_FFI_WRAPPER_CONT

        if rc != 0 {
            return Err(error_from_buf(&error_buf));
        }
        let result = unsafe { out_result.assume_init() };
        let episode_id = unsafe { cstr_to_string(result.episode_id) };
        Ok(CreateEpisodeResult { episode_id })
    }

    pub fn get_episode(&self, episode_id: &str) -> Result<EpisodeMetadata, String> {
        let episode_id_c = CString::new(episode_id).map_err(|e| e.to_string())?;
        let mut out_episode = std::mem::MaybeUninit::<DlEpisodeInfo>::zeroed();
        let mut error_buf = vec![0u8; ERROR_BUF_SIZE];

        let rc = unsafe {
            dl_client_get_episode(
                self.handle,
                episode_id_c.as_ptr(),
                out_episode.as_mut_ptr(),
                error_buf.as_mut_ptr() as *mut c_char,
                ERROR_BUF_SIZE,
            )
        };
        if rc != 0 {
            return Err(error_from_buf(&error_buf));
        }
        let info = unsafe { out_episode.assume_init() };
        let metadata = EpisodeMetadata {
            episode_path: unsafe { cstr_to_string(info.episode_path) },
            bucket: unsafe { cstr_to_string(info.bucket) },
        };
        unsafe { dl_free_episode_info(&mut { info } as *mut DlEpisodeInfo) };
        Ok(metadata)
    }

    // PLACEHOLDER_FFI_UPLOAD_TO

    pub fn upload_to_episode(
        &self,
        paths: &[&Path],
        episode_path: &str,
        bucket: &str,
    ) -> Result<UploadToEpisodeResult, String> {
        let path_cstrings: Vec<CString> = paths
            .iter()
            .map(|p| path_to_cstring(p))
            .collect::<Result<Vec<_>, _>>()?;
        let path_ptrs: Vec<*const c_char> = path_cstrings.iter().map(|s| s.as_ptr()).collect();
        let episode_path_c = CString::new(episode_path).map_err(|e| e.to_string())?;
        let bucket_c = CString::new(bucket).map_err(|e| e.to_string())?;
        let mut succeeded: c_int = 0;
        let mut failed: c_int = 0;
        let mut skipped: c_int = 0;
        let mut error_buf = vec![0u8; ERROR_BUF_SIZE];

        let rc = unsafe {
            dl_client_upload_to_episode(
                self.handle,
                path_ptrs.as_ptr(),
                path_ptrs.len() as c_int,
                episode_path_c.as_ptr(),
                bucket_c.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null(),
                0,
                None,
                std::ptr::null_mut(),
                &mut succeeded,
                &mut failed,
                &mut skipped,
                error_buf.as_mut_ptr() as *mut c_char,
                ERROR_BUF_SIZE,
            )
        };
        if rc != 0 {
            return Err(error_from_buf(&error_buf));
        }
        Ok(UploadToEpisodeResult {
            succeeded: succeeded as u32,
            failed: failed as u32,
        })
    }
}

impl Drop for DataloopClient {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { dl_client_destroy(self.handle) };
        }
    }
}

pub struct CreateEpisodeResult {
    pub episode_id: String,
}

pub struct EpisodeMetadata {
    pub episode_path: String,
    pub bucket: String,
}

pub struct UploadToEpisodeResult {
    pub succeeded: u32,
    pub failed: u32,
}

fn path_to_cstring(path: &Path) -> Result<CString, String> {
    let s = path.to_str().ok_or("path contains non-UTF-8 characters")?;
    CString::new(s).map_err(|e| e.to_string())
}

unsafe fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    CStr::from_ptr(ptr).to_string_lossy().into_owned()
}

fn error_from_buf(buf: &[u8]) -> String {
    let nul_pos = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..nul_pos]).into_owned()
}

fn last_error_string() -> String {
    unsafe {
        let ptr = dl_get_last_error();
        if ptr.is_null() {
            "unknown dataloop error".into()
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}
