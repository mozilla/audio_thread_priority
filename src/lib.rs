//! # audio_thread_priority
//!
//! Promote the current thread, or another thread (possibly in another process), to real-time
//! priority, suitable for low-latency audio processing.
//!
//! # Platforms
//!
//! - **macOS**: the Mach time-constraint scheduling policy.
//! - **Windows**: the Multimedia Class Scheduler Service (MMCSS), "Pro Audio" task.
//! - **Linux** (default, `dbus` feature enabled): rtkit, over D-Bus. This suits unprivileged,
//!   sandboxed desktop processes, since rtkit performs the privileged scheduling change on their
//!   behalf.
//! - **Linux** (`dbus` feature disabled): direct promotion with `pthread_setschedparam` and the
//!   `SCHED_FIFO` policy. This needs no D-Bus daemon, and works whenever the process may request
//!   real-time scheduling: running as root, holding `CAP_SYS_NICE`, or with an `RLIMIT_RTPRIO`
//!   limit configured (e.g. systemd `LimitRTPRIO` or `/etc/security/limits.conf`). The requested
//!   priority defaults to 10 and can be changed with `set_rt_priority` (Linux, no-`dbus` only).
//! - **Other platforms**: a no-op that reports success.
//!
//! # Example
//!
//! ```rust
//!
//! use audio_thread_priority::{promote_current_thread_to_real_time, demote_current_thread_from_real_time};
//!
//! // ... on a thread that will compute audio and has to be real-time:
//! match promote_current_thread_to_real_time(512, 44100) {
//!   Ok(h) => {
//!     println!("this thread is now bumped to real-time priority.");
//!
//!     // Do some real-time work...
//!
//!     match demote_current_thread_from_real_time(h) {
//!       Ok(_) => {
//!         println!("this thread is now bumped back to normal.")
//!       }
//!       Err(_) => {
//!         println!("Could not bring the thread back to normal priority.")
//!       }
//!     };
//!   }
//!   Err(e) => {
//!     eprintln!("Error promoting thread to real-time: {}", e);
//!   }
//! }
//!
//! ```

#![warn(missing_docs)]

use cfg_if::cfg_if;
use std::error::Error;
use std::fmt;

/// The OS-specific issue is available as `inner`
#[derive(Debug)]
pub struct AudioThreadPriorityError {
    message: String,
    inner: Option<Box<dyn Error + 'static>>,
}

impl AudioThreadPriorityError {
    cfg_if! {
        if #[cfg(all(target_os = "linux", feature = "dbus"))] {
            fn new_with_inner(message: &str, inner: Box<dyn Error>) -> AudioThreadPriorityError {
                AudioThreadPriorityError {
                    message: message.into(),
                    inner: Some(inner),
                }
            }
        }
    }
    fn new(message: &str) -> AudioThreadPriorityError {
        AudioThreadPriorityError {
            message: message.into(),
            inner: None,
        }
    }
}

impl fmt::Display for AudioThreadPriorityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut rv = write!(f, "AudioThreadPriorityError: {}", self.message);
        if let Some(inner) = &self.inner {
            rv = write!(f, " ({inner})");
        }
        rv
    }
}

impl Error for AudioThreadPriorityError {
    fn description(&self) -> &str {
        &self.message
    }

    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.inner.as_ref().map(|e| e.as_ref())
    }
}

cfg_if! {
    if #[cfg(any(target_os = "macos", target_os = "ios"))] {
        mod rt_mach;
        extern crate mach2;
        extern crate libc;
        use rt_mach::promote_current_thread_to_real_time_internal;
        use rt_mach::demote_current_thread_from_real_time_internal;
        use rt_mach::get_current_thread_info_internal;
        use rt_mach::promote_thread_to_real_time_internal;
        use rt_mach::demote_thread_from_real_time_internal;
        use rt_mach::RtPriorityThreadInfoInternal;
        use rt_mach::RtPriorityHandleInternal;
    } else if #[cfg(target_os = "windows")] {
        mod rt_win;
        use rt_win::promote_current_thread_to_real_time_internal;
        use rt_win::demote_current_thread_from_real_time_internal;
        use rt_win::get_current_thread_info_internal;
        use rt_win::promote_thread_to_real_time_internal;
        use rt_win::demote_thread_from_real_time_internal;
        use rt_win::RtPriorityThreadInfoInternal;
        use rt_win::RtPriorityHandleInternal;
    } else if #[cfg(all(target_os = "linux", feature = "dbus"))] {
        mod rt_linux;
        extern crate dbus;
        extern crate libc;
        use rt_linux::promote_current_thread_to_real_time_internal;
        use rt_linux::demote_current_thread_from_real_time_internal;
        use rt_linux::set_real_time_hard_limit_internal as set_real_time_hard_limit;
        use rt_linux::get_current_thread_info_internal;
        use rt_linux::promote_thread_to_real_time_internal;
        use rt_linux::demote_thread_from_real_time_internal;
        use rt_linux::RtPriorityThreadInfoInternal;
        use rt_linux::RtPriorityHandleInternal;
    } else if #[cfg(target_os = "linux")] {
        // Linux without the `dbus` feature: promote directly with SCHED_FIFO instead of no-oping.
        mod rt_linux_native;
        extern crate libc;
        use rt_linux_native::promote_current_thread_to_real_time_internal;
        use rt_linux_native::demote_current_thread_from_real_time_internal;
        use rt_linux_native::set_real_time_hard_limit_internal as set_real_time_hard_limit;
        use rt_linux_native::get_current_thread_info_internal;
        use rt_linux_native::promote_thread_to_real_time_internal;
        use rt_linux_native::demote_thread_from_real_time_internal;
        use rt_linux_native::RtPriorityThreadInfoInternal;
        use rt_linux_native::RtPriorityHandleInternal;
        pub use rt_linux_native::set_rt_priority;
    } else if #[cfg(target_os = "android")] {
        mod rt_android;
        use rt_android::promote_current_thread_to_real_time_internal;
        use rt_android::demote_current_thread_from_real_time_internal;
        use rt_android::get_current_thread_info_internal;
        use rt_android::promote_thread_to_real_time_internal;
        use rt_android::demote_thread_from_real_time_internal;
        use rt_android::RtPriorityThreadInfoInternal;
        use rt_android::RtPriorityHandleInternal;
    } else {
        // blanket no-op implementations for platforms without a real-time backend
        /// Fallback priority handle that performs no-op operations on unsupported platforms.
        pub struct RtPriorityHandleInternal {}
        #[derive(Clone, Copy, PartialEq)]
        /// Fallback thread information structure for unsupported platforms.
        pub struct RtPriorityThreadInfoInternal {
            _dummy: u8
        }

        impl RtPriorityThreadInfoInternal {
            /// Serialize the thread info to a byte array (fallback implementation).
            pub fn serialize(&self) -> [u8; 1] {
                [0]
            }
            /// Deserialize thread info from a byte array (fallback implementation).
            pub fn deserialize(_: [u8; 1]) -> Self {
                RtPriorityThreadInfoInternal{_dummy: 0}
            }
            /// Returns the PID of the process containing the thread (fallback: always -1).
            pub fn pid(&self) -> i32 {
                -1
            }
        }
        /// Fallback implementation that performs no operation for unsupported platforms.
        pub fn promote_current_thread_to_real_time_internal(_: u32, audio_samplerate_hz: u32) -> Result<RtPriorityHandleInternal, AudioThreadPriorityError> {
            if audio_samplerate_hz == 0 {
                return Err(AudioThreadPriorityError{message: "sample rate is zero".to_string(), inner: None});
            }
            // no-op
            Ok(RtPriorityHandleInternal{})
        }
        /// Fallback implementation that performs no operation for unsupported platforms.
        pub fn demote_current_thread_from_real_time_internal(_: RtPriorityHandleInternal) -> Result<(), AudioThreadPriorityError> {
            // no-op
            Ok(())
        }
        /// Fallback implementation that performs no operation for unsupported platforms.
        pub fn set_real_time_hard_limit(
            _: u32,
            _: u32,
        ) -> Result<(), AudioThreadPriorityError> {
            Ok(())
        }
        /// Fallback implementation that returns dummy thread info for unsupported platforms.
        pub fn get_current_thread_info_internal() -> Result<RtPriorityThreadInfoInternal, AudioThreadPriorityError> {
            Ok(RtPriorityThreadInfoInternal{_dummy: 0})
        }
        /// Fallback implementation that performs no operation for unsupported platforms.
        pub fn promote_thread_to_real_time_internal(
            _: RtPriorityThreadInfoInternal,
            _: u32,
            audio_samplerate_hz: u32,
        ) -> Result<RtPriorityHandleInternal, AudioThreadPriorityError> {
            if audio_samplerate_hz == 0 {
                return Err(AudioThreadPriorityError::new("sample rate is zero"));
            }
            Ok(RtPriorityHandleInternal{})
        }

        /// Fallback implementation that performs no operation for unsupported platforms.
        pub fn demote_thread_from_real_time_internal(_: RtPriorityThreadInfoInternal) -> Result<(), AudioThreadPriorityError> {
            Ok(())
        }
    }
}

/// Opaque handle to a thread handle structure.
pub type RtPriorityHandle = RtPriorityHandleInternal;

/// Opaque handle to a thread's scheduling information.
///
/// This can be serialized to raw bytes and sent to another thread or process via IPC, so that
/// thread or process can promote or demote the thread to/from real-time priority on its behalf.
pub type RtPriorityThreadInfo = RtPriorityThreadInfoInternal;

#[no_mangle]
/// Size of a RtPriorityThreadInfo or atp_thread_info struct, for use in FFI.
pub static ATP_THREAD_INFO_SIZE: usize = std::mem::size_of::<RtPriorityThreadInfo>();

/// Get the calling thread's information, to be able to promote it to real-time from somewhere
/// else, later on another thread, or in another process.
///
/// This is useful when the thread that needs to become real-time cannot promote itself directly,
/// for example because it is sandboxed.
///
/// # Return value
///
/// Ok in case of success, with an opaque structure containing relevant info for the platform, Err
/// otherwise.
pub fn get_current_thread_info() -> Result<RtPriorityThreadInfo, AudioThreadPriorityError> {
    get_current_thread_info_internal()
}

/// Return a byte buffer containing serialized information about a thread, to promote it to
/// real-time from elsewhere.
pub fn thread_info_serialize(
    thread_info: RtPriorityThreadInfo,
) -> [u8; std::mem::size_of::<RtPriorityThreadInfo>()] {
    thread_info.serialize()
}

/// From a byte buffer, return a `RtPriorityThreadInfo`.
///
/// # Arguments
///
/// A byte buffer containing a serialized `RtPriorityThreadInfo`.
pub fn thread_info_deserialize(
    bytes: [u8; std::mem::size_of::<RtPriorityThreadInfo>()],
) -> RtPriorityThreadInfo {
    RtPriorityThreadInfoInternal::deserialize(bytes)
}

/// Get the calling thread's information, to promote it from another process or thread, with a C
/// API.
///
/// This is intended to be called on the thread that will be promoted to real-time priority, when
/// that thread cannot do so itself (for example because it is sandboxed).
///
/// After use, it MUST be freed by calling `atp_free_thread_info`.
///
/// # Return value
///
/// A pointer to a struct that can be serialized and deserialized, and that can be passed to
/// `atp_promote_thread_to_real_time`, even from another process.
#[no_mangle]
pub extern "C" fn atp_get_current_thread_info() -> *mut atp_thread_info {
    match get_current_thread_info() {
        Ok(thread_info) => Box::into_raw(Box::new(atp_thread_info(thread_info))),
        _ => std::ptr::null_mut(),
    }
}

/// Frees a thread info, with a c api.
///
/// # Arguments
///
/// thread_info: the `atp_thread_info` structure to free.
///
/// # Return value
///
/// 0 in case of success, 1 otherwise (if `thread_info` is NULL).
///
/// # Safety
///
/// This function is safe only and only if the pointer comes from this library, of if is null.
#[no_mangle]
pub unsafe extern "C" fn atp_free_thread_info(thread_info: *mut atp_thread_info) -> i32 {
    if thread_info.is_null() {
        return 1;
    }
    drop(Box::from_raw(thread_info));
    0
}

/// Return a byte buffer containing serialized information about a thread, to promote it to
/// real-time from elsewhere, with a C API.
///
/// `bytes` MUST be `std::mem::size_of<RtPriorityThreadInfo>()` bytes long.
///
/// This is exposed in the C API as `ATP_THREAD_INFO_SIZE`.
///
/// # Safety
///
/// This function is safe only and only if the first pointer comes from this library, and the
/// second pointer is at least ATP_THREAD_INFO_SIZE bytes long.
#[no_mangle]
pub unsafe extern "C" fn atp_serialize_thread_info(
    thread_info: *mut atp_thread_info,
    bytes: *mut std::ffi::c_void,
) {
    let thread_info = &mut *thread_info;
    let source = thread_info.0.serialize();
    std::ptr::copy(source.as_ptr(), bytes as *mut u8, source.len());
}

/// From a byte buffer, return a `RtPriorityThreadInfo`, with a C API.
///
/// # Arguments
///
/// A byte buffer containing a serialized `RtPriorityThreadInfo`.
///
/// # Safety
///
/// This function is safe only and only if pointer is at least ATP_THREAD_INFO_SIZE bytes long.
#[no_mangle]
pub unsafe extern "C" fn atp_deserialize_thread_info(in_bytes: *mut u8) -> *mut atp_thread_info {
    let bytes = *(in_bytes as *mut [u8; std::mem::size_of::<RtPriorityThreadInfoInternal>()]);
    let thread_info = RtPriorityThreadInfoInternal::deserialize(bytes);
    Box::into_raw(Box::new(atp_thread_info(thread_info)))
}

/// Promote a particular thread to real-time priority.
///
/// This is useful when the thread to promote is not the calling thread, is possibly in another
/// process, and/or cannot promote itself directly (for example because of sandboxing).
///
/// Promoting a thread other than the caller's, especially in another process, may require
/// elevated privileges depending on the platform (for example, Linux uses a privileged rtkit
/// D-Bus service, and macOS/iOS require `task_for_pid` rights).
///
/// # Arguments
///
/// * `thread_info` - informations about the thread to promote, gathered using
///   `get_current_thread_info`.
/// * `audio_buffer_frames` - the exact or an upper limit on the number of frames that have to be
///   rendered each callback, or 0 for a sensible default value.
/// * `audio_samplerate_hz` - the sample-rate for this audio stream, in Hz.
///
/// # Return value
///
/// This function returns a `Result<RtPriorityHandle>`. Unlike `promote_current_thread_to_real_time`,
/// this handle should NOT be passed to `demote_current_thread_from_real_time`: use
/// `demote_thread_from_real_time(thread_info)` instead, with the same `thread_info` used here, to
/// revert to the previous thread priority (the promoting and demoting call may not happen on the
/// same thread, or even in the same process, so the handle alone isn't sufficient on every
/// platform).
pub fn promote_thread_to_real_time(
    thread_info: RtPriorityThreadInfo,
    audio_buffer_frames: u32,
    audio_samplerate_hz: u32,
) -> Result<RtPriorityHandle, AudioThreadPriorityError> {
    if audio_samplerate_hz == 0 {
        return Err(AudioThreadPriorityError::new("sample rate is zero"));
    }
    promote_thread_to_real_time_internal(thread_info, audio_buffer_frames, audio_samplerate_hz)
}

/// Demotes a thread from real-time priority.
///
/// # Arguments
///
/// * `thread_info` - An opaque struct returned from a successful call to
///   `get_current_thread_info`.
///
/// # Return value
///
/// `Ok` in case of success, `Err` otherwise.
pub fn demote_thread_from_real_time(
    thread_info: RtPriorityThreadInfo,
) -> Result<(), AudioThreadPriorityError> {
    demote_thread_from_real_time_internal(thread_info)
}

/// Opaque info to a particular thread.
#[allow(non_camel_case_types)]
pub struct atp_thread_info(RtPriorityThreadInfo);

/// Promote a specific thread to real-time, with a C API.
///
/// This is useful when the thread to promote cannot make some system calls necessary to promote
/// it.
///
/// # Arguments
///
/// * `thread_info` - the information of the thread to promote to real-time, gather from calling
///   `atp_get_current_thread_info` on the thread to promote.
/// * `audio_buffer_frames` - the exact or an upper limit on the number of frames that have to be
///   rendered each callback, or 0 for a sensible default value.
/// * `audio_samplerate_hz` - the sample-rate for this audio stream, in Hz.
///
/// # Return value
///
/// A pointer to an `atp_handle` in case of success, NULL otherwise.
///
/// # Safety
///
/// This function is safe as long as the first pointer comes from this library.
#[no_mangle]
pub unsafe extern "C" fn atp_promote_thread_to_real_time(
    thread_info: *mut atp_thread_info,
    audio_buffer_frames: u32,
    audio_samplerate_hz: u32,
) -> *mut atp_handle {
    let thread_info = &mut *thread_info;
    match promote_thread_to_real_time(thread_info.0, audio_buffer_frames, audio_samplerate_hz) {
        Ok(handle) => Box::into_raw(Box::new(atp_handle(handle))),
        _ => std::ptr::null_mut(),
    }
}

/// Demote a thread promoted to from real-time, with a C API.
///
/// # Arguments
///
/// `handle` -  an opaque struct received from a promoting function.
///
/// # Return value
///
/// 0 in case of success, non-zero otherwise.
///
/// # Safety
///
/// This function is safe as long as the first pointer comes from this library, or is null.
#[no_mangle]
pub unsafe extern "C" fn atp_demote_thread_from_real_time(
    thread_info: *mut atp_thread_info,
) -> i32 {
    if thread_info.is_null() {
        return 1;
    }
    let thread_info = (*thread_info).0;

    match demote_thread_from_real_time(thread_info) {
        Ok(_) => 0,
        _ => 1,
    }
}

cfg_if! {
    if #[cfg(target_os = "linux")] {
/// Set a real-time limit for the calling thread.
///
/// This is only necessary and available on Linux desktop, and allows remoting the rtkit D-Bus
/// call to a process that has access to D-Bus. This function has to be called before attempting
/// to promote threads from another process.
///
/// # Arguments
///
/// `audio_buffer_frames` - the number of frames the audio callback has to render each quantum. 0
/// picks a rather high default value.
/// `audio_samplerate_hz` - the sample-rate of the audio stream.
///
/// # Return value
///
/// 0 in case of success, 1 otherwise.
#[no_mangle]
pub extern "C" fn atp_set_real_time_limit(audio_buffer_frames: u32,
                                          audio_samplerate_hz: u32) -> i32 {
    let r = set_real_time_hard_limit(audio_buffer_frames, audio_samplerate_hz);
    if r.is_err() {
        return 1;
    }
    0
}
    }
}

/// Promote the calling thread to real-time priority.
///
/// # Arguments
///
/// * `audio_buffer_frames` - the exact or an upper limit on the number of frames that have to be
///   rendered each callback, or 0 for a sensible default value.
/// * `audio_samplerate_hz` - the sample-rate for this audio stream, in Hz.
///
/// # Return value
///
/// This function returns a `Result<RtPriorityHandle>`, which is an opaque struct to be passed to
/// `demote_current_thread_from_real_time` to revert to the previous thread priority.
pub fn promote_current_thread_to_real_time(
    audio_buffer_frames: u32,
    audio_samplerate_hz: u32,
) -> Result<RtPriorityHandle, AudioThreadPriorityError> {
    if audio_samplerate_hz == 0 {
        return Err(AudioThreadPriorityError::new("sample rate is zero"));
    }
    promote_current_thread_to_real_time_internal(audio_buffer_frames, audio_samplerate_hz)
}

/// Demotes the calling thread from real-time priority.
///
/// # Arguments
///
/// * `handle` - An opaque struct returned from a successful call to
///   `promote_current_thread_to_real_time`.
///
/// # Return value
///
/// `Ok` in case of success, `Err` otherwise.
pub fn demote_current_thread_from_real_time(
    handle: RtPriorityHandle,
) -> Result<(), AudioThreadPriorityError> {
    demote_current_thread_from_real_time_internal(handle)
}

/// Opaque handle for the C API
#[allow(non_camel_case_types)]
pub struct atp_handle(RtPriorityHandle);

/// Promote the calling thread to real-time priority, with a C API.
///
/// # Arguments
///
/// * `audio_buffer_frames` - the exact or an upper limit on the number of frames that have to be
///   rendered each callback, or 0 for a sensible default value.
/// * `audio_samplerate_hz` - the sample-rate for this audio stream, in Hz.
///
/// # Return value
///
/// This function returns `NULL` in case of error: if it couldn't bump the thread, or if the
/// `audio_samplerate_hz` is zero. It returns an opaque handle, to be passed to
/// `atp_demote_current_thread_from_real_time` to demote the thread.
///
/// Additionally, on Linux this returns NULL when the current thread cannot be promoted directly.
/// With the default rtkit/D-Bus backend that happens in sandboxed processes where D-Bus is
/// unreachable (for example because the socket to D-Bus cannot be created); without the `dbus`
/// feature it happens when the process lacks permission to set real-time scheduling. In that case,
/// gather the thread's information with `atp_get_current_thread_info` and have another (privileged)
/// process promote it via `atp_promote_thread_to_real_time`.
#[no_mangle]
pub extern "C" fn atp_promote_current_thread_to_real_time(
    audio_buffer_frames: u32,
    audio_samplerate_hz: u32,
) -> *mut atp_handle {
    match promote_current_thread_to_real_time(audio_buffer_frames, audio_samplerate_hz) {
        Ok(handle) => Box::into_raw(Box::new(atp_handle(handle))),
        _ => std::ptr::null_mut(),
    }
}
/// Demotes the calling thread from real-time priority, with a C API.
///
/// # Arguments
///
/// * `atp_handle` - An opaque struct returned from a successful call to
///   `atp_promote_current_thread_to_real_time`.
///
/// # Return value
///
/// 0 in case of success, non-zero in case of error.
///
/// # Safety
///
/// Only to be used with a valid pointer from this library -- not after having released it via
/// atp_free_handle.
#[no_mangle]
pub unsafe extern "C" fn atp_demote_current_thread_from_real_time(handle: *mut atp_handle) -> i32 {
    assert!(!handle.is_null());
    let handle = Box::from_raw(handle);

    match demote_current_thread_from_real_time(handle.0) {
        Ok(_) => 0,
        _ => 1,
    }
}

/// Frees a handle, with a C API.
///
/// This is useful when it is impractical to call `atp_demote_current_thread_from_real_time` on the
/// right thread. Access to the handle must be synchronized externally, or the thread that was
/// promoted to real-time priority must have exited.
///
/// # Arguments
///
/// * `atp_handle` - An opaque struct returned from a successful call to
///   `atp_promote_current_thread_to_real_time`.
///
/// # Return value
///
/// 0 in case of success, non-zero in case of error.
///
/// # Safety
///
/// Should only be called to free something from this crate.
#[no_mangle]
pub unsafe extern "C" fn atp_free_handle(handle: *mut atp_handle) -> i32 {
    if handle.is_null() {
        return 1;
    }
    let _handle = Box::from_raw(handle);
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "terminal-logging")]
    use simple_logger;

    // On the native (no-dbus) Linux build, promotion actually changes the scheduler, so it needs
    // permission to request real-time scheduling. When the environment does not grant it (no
    // RLIMIT_RTPRIO budget and not privileged), the promotion tests have nothing to exercise and
    // skip rather than fail; CI raises the limit so they run for real.
    #[cfg(all(target_os = "linux", not(feature = "dbus")))]
    fn rt_scheduling_available() -> bool {
        match promote_current_thread_to_real_time(0, 44100) {
            Ok(handle) => {
                // Demotion must succeed after a successful promotion; otherwise the thread would
                // stay real-time and perturb later tests, so treat a failure as fatal here.
                demote_current_thread_from_real_time(handle)
                    .expect("demotion after a successful promotion should succeed");
                true
            }
            Err(_) => false,
        }
    }

    #[test]
    fn it_works() {
        #[cfg(feature = "terminal-logging")]
        simple_logger::init().unwrap();
        #[cfg(all(target_os = "linux", not(feature = "dbus")))]
        if !rt_scheduling_available() {
            eprintln!("skipping it_works: real-time scheduling is not permitted here");
            return;
        }
        {
            assert!(promote_current_thread_to_real_time(0, 0).is_err());
        }
        {
            match promote_current_thread_to_real_time(0, 44100) {
                Ok(rt_prio_handle) => {
                    let rv = demote_current_thread_from_real_time(rt_prio_handle);
                    assert!(rv.is_ok());
                }
                Err(e) => {
                    panic!("{}", e);
                }
            }
        }
        {
            match promote_current_thread_to_real_time(512, 44100) {
                Ok(rt_prio_handle) => {
                    let rv = demote_current_thread_from_real_time(rt_prio_handle);
                    assert!(rv.is_ok());
                }
                Err(e) => {
                    panic!("{}", e);
                }
            }
        }
        {
            // Try larger values to test https://github.com/mozilla/audio_thread_priority/pull/23
            match promote_current_thread_to_real_time(0, 192000) {
                Ok(rt_prio_handle) => {
                    let rv = demote_current_thread_from_real_time(rt_prio_handle);
                    assert!(rv.is_ok());
                }
                Err(e) => {
                    panic!("{}", e);
                }
            }
        }
        {
            // Try larger values to test https://github.com/mozilla/audio_thread_priority/pull/23
            match promote_current_thread_to_real_time(8192, 48000) {
                Ok(rt_prio_handle) => {
                    let rv = demote_current_thread_from_real_time(rt_prio_handle);
                    assert!(rv.is_ok());
                }
                Err(e) => {
                    panic!("{}", e);
                }
            }
        }
        {
            match promote_current_thread_to_real_time(512, 44100) {
                Ok(_) => {}
                Err(e) => {
                    panic!("{}", e);
                }
            }
            // automatically deallocated, but not demoted until the thread exits.
        }
    }

    #[test]
    fn it_works_in_different_threads() {
        let handles: Vec<_> = (0..32).map(|_| std::thread::spawn(it_works)).collect();
        for handle in handles {
            handle.join().unwrap()
        }
    }

    #[test]
    fn test_thread_info_serialization() {
        let info = get_current_thread_info().unwrap();
        let bytes = info.serialize();
        let info2 = RtPriorityThreadInfo::deserialize(bytes);
        assert!(info == info2);

        let bytes = thread_info_serialize(info);
        let info2 = thread_info_deserialize(bytes);
        assert!(info == info2);
    }

    #[test]
    fn test_promote_another_thread() {
        use std::sync::mpsc::channel;

        #[cfg(all(target_os = "linux", not(feature = "dbus")))]
        if !rt_scheduling_available() {
            eprintln!(
                "skipping test_promote_another_thread: real-time scheduling is not permitted here"
            );
            return;
        }

        let (info_tx, info_rx) = channel();
        let (done_tx, done_rx) = channel();

        let handle = std::thread::spawn(move || {
            let info = get_current_thread_info().unwrap();
            info_tx.send(info).unwrap();
            // Keep this thread alive while the main thread promotes and demotes it.
            done_rx.recv().unwrap();
        });

        let info = info_rx.recv().unwrap();

        match promote_thread_to_real_time(info, 512, 44100) {
            Ok(_) => {}
            Err(e) => panic!("{}", e),
        }

        match demote_thread_from_real_time(info) {
            Ok(_) => {}
            Err(e) => panic!("{}", e),
        }

        done_tx.send(()).unwrap();
        handle.join().unwrap();
    }

    cfg_if! {
        if #[cfg(target_os = "linux")] {
            use nix::unistd::*;
            use nix::sys::signal::*;
            #[cfg(not(feature = "dbus"))]
            use nix::sys::wait::*;

            #[test]
            fn test_remote_promotion() {
                let (rd, wr) = pipe().unwrap();

                match unsafe { fork().expect("fork failed") } {
                    ForkResult::Parent{ child } => {
                        eprintln!("Parent PID: {}", getpid());
                        let mut bytes = [0_u8; std::mem::size_of::<RtPriorityThreadInfo>()];
                        match read(rd, &mut bytes) {
                             Ok(_) => {
                                let info = RtPriorityThreadInfo::deserialize(bytes);
                                match promote_thread_to_real_time(info, 0, 44100) {
                                    Ok(_) => {
                                        eprintln!("thread promotion in the child from the parent succeeded");
                                    }
                                    Err(e) => {
                                        kill(child, SIGKILL).expect("Could not kill the child?");
                                        // Promoting a thread in another process can need privilege
                                        // beyond RLIMIT_RTPRIO (CAP_SYS_NICE) that an unprivileged
                                        // CI does not grant. With the native (no-dbus) backend,
                                        // treat that as a skip rather than a failure.
                                        if cfg!(feature = "dbus") {
                                            panic!("{}", e);
                                        } else {
                                            eprintln!("skipping test_remote_promotion: promoting a thread in another process needs elevated privilege ({e})");
                                            return;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                eprintln!("could not read from the pipe: {}", e);
                            }
                        }
                        kill(child, SIGKILL).expect("Could not kill the child?");
                    }
                    ForkResult::Child => {
                        let r = set_real_time_hard_limit(0, 44100);
                        if r.is_err() {
                            eprintln!("Could not set RT limit, the test will fail.");
                        }
                        eprintln!("Child pid: {}", getpid());
                        let info = get_current_thread_info().unwrap();
                        let bytes = info.serialize();
                        match write(wr, &bytes) {
                            Ok(_) => {
                                loop {
                                    std::thread::sleep(std::time::Duration::from_millis(1000));
                                    eprintln!("child sleeping, waiting to be promoted...");
                                }
                            }
                            Err(_) => {
                                eprintln!("write error on the pipe.");
                            }
                        }
                    }
                }
            }

            // Native (no-dbus) path only. These tests change the process-wide RLIMIT_RTPRIO and, for
            // the override test, the priority set via `set_rt_priority`, so they run in a forked
            // child to avoid racing with the other (parallel) promotion tests.
            cfg_if! {
                if #[cfg(not(feature = "dbus"))] {
                    const SCHED_RESET_ON_FORK: libc::c_int = 0x4000_0000;
                    // Exit codes the forked child reports back to the parent.
                    const PASSED: i32 = 0;
                    const FAILED: i32 = 1;
                    const SKIPPED: i32 = 2;

                    fn rtprio_limit() -> libc::rlimit {
                        let mut lim = unsafe { std::mem::zeroed::<libc::rlimit>() };
                        assert_eq!(unsafe { libc::getrlimit(libc::RLIMIT_RTPRIO, &mut lim) }, 0);
                        lim
                    }

                    fn set_rtprio_soft(soft: libc::rlim_t) {
                        let mut lim = rtprio_limit();
                        lim.rlim_cur = soft;
                        assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_RTPRIO, &lim) }, 0);
                    }

                    fn current_scheduler() -> (libc::c_int, libc::c_int) {
                        let mut policy = 0;
                        let mut param = unsafe { std::mem::zeroed::<libc::sched_param>() };
                        assert_eq!(
                            unsafe {
                                libc::pthread_getschedparam(
                                    libc::pthread_self(),
                                    &mut policy,
                                    &mut param,
                                )
                            },
                            0
                        );
                        (policy, param.sched_priority)
                    }

                    // Run `checks` in a forked child so its RLIMIT_RTPRIO and environment changes
                    // do not affect the other tests, and turn the child's exit code into a pass, a
                    // skip, or a panic.
                    fn run_in_child(name: &str, checks: impl FnOnce() -> i32) {
                        match unsafe { fork().expect("fork failed") } {
                            ForkResult::Parent { child } => {
                                match waitpid(child, None).expect("waitpid") {
                                    WaitStatus::Exited(_, PASSED) => {}
                                    WaitStatus::Exited(_, SKIPPED) => {
                                        eprintln!("skipping {}: needs an unprivileged process with a real-time budget", name);
                                    }
                                    other => panic!("{} child reported a failure: {:?}", name, other),
                                }
                            }
                            ForkResult::Child => std::process::exit(checks()),
                        }
                    }

                    // Promotion honours RLIMIT_RTPRIO: with the soft limit below the requested
                    // priority it must be denied, and at the priority it must succeed and actually
                    // move the thread to SCHED_FIFO. Skipped as root (which bypasses RLIMIT_RTPRIO)
                    // or when no real-time budget was granted (a plain developer machine; CI raises
                    // it with `prlimit`).
                    #[test]
                    fn test_native_promotion_honours_rlimit() {
                        const RT_PRIO: libc::c_int = 10;
                        run_in_child("test_native_promotion_honours_rlimit", || {
                            if unsafe { libc::geteuid() } == 0 {
                                return SKIPPED;
                            }
                            if rtprio_limit().rlim_max < RT_PRIO as libc::rlim_t {
                                return SKIPPED;
                            }

                            // Below the requested priority: promotion must be denied.
                            set_rtprio_soft(RT_PRIO as libc::rlim_t - 1);
                            if promote_current_thread_to_real_time(0, 44100).is_ok() {
                                eprintln!("promotion succeeded below the RLIMIT_RTPRIO ceiling");
                                return FAILED;
                            }

                            // At the requested priority: promotion must succeed and take effect.
                            set_rtprio_soft(RT_PRIO as libc::rlim_t);
                            let handle = match promote_current_thread_to_real_time(0, 44100) {
                                Ok(handle) => handle,
                                Err(e) => {
                                    eprintln!("promotion denied at the RLIMIT_RTPRIO ceiling: {e}");
                                    return FAILED;
                                }
                            };
                            let (policy, prio) = current_scheduler();
                            if policy & !SCHED_RESET_ON_FORK != libc::SCHED_FIFO || prio != RT_PRIO {
                                eprintln!("unexpected scheduler after promotion: policy={policy} prio={prio}");
                                return FAILED;
                            }
                            if demote_current_thread_from_real_time(handle).is_err() {
                                eprintln!("demotion failed");
                                return FAILED;
                            }
                            PASSED
                        });
                    }

                    // The requested priority can be overridden with set_rt_priority. Uses a value
                    // that differs from the default (10) and fits a modest RLIMIT_RTPRIO, and checks
                    // the thread lands on exactly that priority.
                    #[test]
                    fn test_native_priority_override() {
                        const OVERRIDE: libc::c_int = 7;
                        run_in_child("test_native_priority_override", || {
                            if unsafe { libc::geteuid() } == 0 {
                                return SKIPPED;
                            }
                            let hard = rtprio_limit().rlim_max;
                            if hard < OVERRIDE as libc::rlim_t {
                                return SKIPPED;
                            }
                            set_rtprio_soft(hard);
                            set_rt_priority(Some(OVERRIDE as u8));

                            let handle = match promote_current_thread_to_real_time(0, 44100) {
                                Ok(handle) => handle,
                                Err(e) => {
                                    eprintln!("promotion denied at priority {OVERRIDE}: {e}");
                                    return FAILED;
                                }
                            };
                            let (_, prio) = current_scheduler();
                            let result = if prio == OVERRIDE {
                                PASSED
                            } else {
                                eprintln!("expected priority {OVERRIDE}, got {prio}");
                                FAILED
                            };
                            let _ = demote_current_thread_from_real_time(handle);
                            result
                        });
                    }
                }
            }
        }
    }
}
