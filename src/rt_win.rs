use self::avrt_lib::AvRtLibrary;
use crate::AudioThreadPriorityError;
use log::info;
use std::sync::OnceLock;
use windows_sys::{
    w,
    Win32::Foundation::{CloseHandle, HANDLE, WIN32_ERROR},
    Win32::System::Threading::{
        GetCurrentProcessId, GetCurrentThreadId, GetThreadPriority, OpenThread, SetThreadPriority,
        THREAD_PRIORITY_TIME_CRITICAL, THREAD_QUERY_INFORMATION, THREAD_SET_INFORMATION,
    },
};

// `GetThreadPriority` returns this sentinel value on failure. It's defined as
// `THREAD_PRIORITY_ERROR_RETURN` in `winbase.h`, but `windows-sys` only exposes it under
// `Win32::System::WindowsProgramming`, a module this crate otherwise has no use for.
const THREAD_PRIORITY_ERROR_RETURN: i32 = i32::MAX;

/// Two different mechanisms are used, depending on whether the thread being promoted is the
/// calling thread or not:
/// - The calling thread is registered with MMCSS (`avrt.dll`), which is the mechanism
///   recommended by Microsoft for pro-audio applications, but that can only ever be used by a
///   thread on itself: none of the `avrt.dll` APIs take a thread handle or id.
/// - A thread other than the caller (possibly in another process) is bumped using the ordinary
///   Win32 thread priority APIs instead, which do accept a `HANDLE` to an arbitrary thread.
#[derive(Debug)]
pub enum RtPriorityHandleInternal {
    Mmcss {
        mmcss_task_index: u32,
        task_handle: HANDLE,
    },
    ThreadPriority {
        tid: u32,
        previous_priority: i32,
    },
}

fn avrt() -> Result<&'static AvRtLibrary, AudioThreadPriorityError> {
    static AV_RT_LIBRARY: OnceLock<Result<AvRtLibrary, WIN32_ERROR>> = OnceLock::new();
    AV_RT_LIBRARY
        .get_or_init(AvRtLibrary::try_new)
        .as_ref()
        .map_err(|win32_error| {
            AudioThreadPriorityError::new(&format!("Unable to load avrt.dll ({win32_error})"))
        })
}

pub fn promote_current_thread_to_real_time_internal(
    _audio_buffer_frames: u32,
    _audio_samplerate_hz: u32,
) -> Result<RtPriorityHandleInternal, AudioThreadPriorityError> {
    avrt()?
        .set_mm_thread_characteristics(w!("Audio"))
        .map(|(mmcss_task_index, task_handle)| {
            info!("task {mmcss_task_index} bumped to real time priority.");
            RtPriorityHandleInternal::Mmcss {
                mmcss_task_index,
                task_handle,
            }
        })
        .map_err(|win32_error| {
            AudioThreadPriorityError::new(&format!(
                "Unable to bump the thread priority ({win32_error})"
            ))
        })
}

pub fn demote_current_thread_from_real_time_internal(
    rt_priority_handle: RtPriorityHandleInternal,
) -> Result<(), AudioThreadPriorityError> {
    match rt_priority_handle {
        RtPriorityHandleInternal::Mmcss {
            mmcss_task_index,
            task_handle,
        } => avrt()?
            .revert_mm_thread_characteristics(task_handle)
            .map(|_| {
                info!("task {mmcss_task_index} priority restored.");
            })
            .map_err(|win32_error| {
                AudioThreadPriorityError::new(&format!(
                    "Unable to restore the thread priority for task {mmcss_task_index} ({win32_error})"
                ))
            }),
        RtPriorityHandleInternal::ThreadPriority {
            tid,
            previous_priority,
        } => set_thread_priority(tid, previous_priority),
    }
}

/// Opaque, serializable information about a thread, possibly running in another process,
/// sufficient to promote or demote it to/from real-time priority.
#[repr(C)]
#[derive(Clone, Copy, PartialEq)]
pub struct RtPriorityThreadInfoInternal {
    pid: u32,
    tid: u32,
    previous_priority: i32,
}

impl RtPriorityThreadInfoInternal {
    /// Serialize a RtPriorityThreadInfoInternal to a byte buffer.
    pub fn serialize(&self) -> [u8; std::mem::size_of::<Self>()] {
        unsafe { std::mem::transmute::<Self, [u8; std::mem::size_of::<Self>()]>(*self) }
    }
    /// Get an RtPriorityThreadInfoInternal from a byte buffer.
    pub fn deserialize(bytes: [u8; std::mem::size_of::<Self>()]) -> Self {
        unsafe { std::mem::transmute::<[u8; std::mem::size_of::<Self>()], Self>(bytes) }
    }
    /// Returns the PID of the process containing the thread.
    pub fn pid(&self) -> i32 {
        self.pid as i32
    }
}

fn open_thread(tid: u32) -> Result<HANDLE, AudioThreadPriorityError> {
    let handle = unsafe { OpenThread(THREAD_SET_INFORMATION | THREAD_QUERY_INFORMATION, 0, tid) };
    if handle.is_null() {
        return Err(AudioThreadPriorityError::new(&format!(
            "OpenThread failed for thread {tid}"
        )));
    }
    Ok(handle)
}

fn get_thread_priority(tid: u32) -> Result<i32, AudioThreadPriorityError> {
    let handle = open_thread(tid)?;
    let priority = unsafe { GetThreadPriority(handle) };
    unsafe { CloseHandle(handle) };
    if priority == THREAD_PRIORITY_ERROR_RETURN {
        return Err(AudioThreadPriorityError::new(&format!(
            "GetThreadPriority failed for thread {tid}"
        )));
    }
    Ok(priority)
}

fn set_thread_priority(tid: u32, priority: i32) -> Result<(), AudioThreadPriorityError> {
    let handle = open_thread(tid)?;
    let rv = unsafe { SetThreadPriority(handle, priority) };
    unsafe { CloseHandle(handle) };
    if rv == 0 {
        return Err(AudioThreadPriorityError::new(&format!(
            "SetThreadPriority failed for thread {tid}"
        )));
    }
    Ok(())
}

/// Get the current thread information, as an opaque struct, that can be serialized and sent
/// accross processes, to have another thread promoted to real-time.
pub fn get_current_thread_info_internal(
) -> Result<RtPriorityThreadInfoInternal, AudioThreadPriorityError> {
    let tid = unsafe { GetCurrentThreadId() };
    let previous_priority = get_thread_priority(tid)?;

    Ok(RtPriorityThreadInfoInternal {
        pid: unsafe { GetCurrentProcessId() },
        tid,
        previous_priority,
    })
}

/// Promote a thread (possibly in another process) identified by its thread info, to real-time.
///
/// Unlike `promote_current_thread_to_real_time`, this can't use MMCSS: none of the `avrt.dll`
/// APIs accept a thread id or handle, they only ever act on the calling thread. This instead
/// raises the target thread's priority to `THREAD_PRIORITY_TIME_CRITICAL` via the ordinary Win32
/// thread priority APIs, using a handle obtained with `OpenThread`, which works for a thread in
/// another process too, given sufficient access rights.
pub fn promote_thread_to_real_time_internal(
    thread_info: RtPriorityThreadInfoInternal,
    _audio_buffer_frames: u32,
    _audio_samplerate_hz: u32,
) -> Result<RtPriorityHandleInternal, AudioThreadPriorityError> {
    set_thread_priority(thread_info.tid, THREAD_PRIORITY_TIME_CRITICAL)?;

    info!(
        "thread {} (pid {}) bumped to real time priority.",
        thread_info.tid, thread_info.pid
    );

    Ok(RtPriorityHandleInternal::ThreadPriority {
        tid: thread_info.tid,
        previous_priority: thread_info.previous_priority,
    })
}

/// This can be called by sandboxed code, it restores the priority the thread had when
/// `get_current_thread_info` captured `thread_info`.
pub fn demote_thread_from_real_time_internal(
    thread_info: RtPriorityThreadInfoInternal,
) -> Result<(), AudioThreadPriorityError> {
    set_thread_priority(thread_info.tid, thread_info.previous_priority)
}

mod avrt_lib {
    use super::win32_utils::{win32_error_if, OwnedLibrary};
    use std::sync::Once;
    use windows_sys::{
        core::PCWSTR,
        s, w,
        Win32::Foundation::{FALSE, HANDLE, WIN32_ERROR},
    };

    type AvSetMmThreadCharacteristicsWFn = unsafe extern "system" fn(PCWSTR, *mut u32) -> HANDLE;
    type AvRevertMmThreadCharacteristicsFn = unsafe extern "system" fn(HANDLE) -> i32;

    #[derive(Debug)]
    pub(super) struct AvRtLibrary {
        // This field is never read because only used for its Drop behavior
        #[allow(dead_code)]
        module: OwnedLibrary,

        av_set_mm_thread_characteristics_w: AvSetMmThreadCharacteristicsWFn,
        av_revert_mm_thread_characteristics: AvRevertMmThreadCharacteristicsFn,
    }

    impl AvRtLibrary {
        pub(super) fn try_new() -> Result<Self, WIN32_ERROR> {
            let module = OwnedLibrary::try_new(w!("avrt.dll"))?;
            let av_set_mm_thread_characteristics_w = unsafe {
                std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    AvSetMmThreadCharacteristicsWFn,
                >(module.get_proc(s!("AvSetMmThreadCharacteristicsW"))?)
            };
            let av_revert_mm_thread_characteristics = unsafe {
                std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    AvRevertMmThreadCharacteristicsFn,
                >(module.get_proc(s!("AvRevertMmThreadCharacteristics"))?)
            };
            Ok(Self {
                module,
                av_set_mm_thread_characteristics_w,
                av_revert_mm_thread_characteristics,
            })
        }

        pub(super) fn set_mm_thread_characteristics(
            &self,
            task_name: PCWSTR,
        ) -> Result<(u32, HANDLE), WIN32_ERROR> {
            // Ensure that the first call never runs in parallel with other calls. This
            // seems necessary to guarantee the success of these other calls. We saw them
            // fail with an error code of ERROR_PATH_NOT_FOUND in tests, presumably on a
            // machine where the MMCSS service was initially inactive.
            static FIRST_CALL: Once = Once::new();
            let mut first_call_result = None;
            FIRST_CALL.call_once(|| {
                first_call_result = Some(self.set_mm_thread_characteristics_internal(task_name))
            });
            first_call_result
                .unwrap_or_else(|| self.set_mm_thread_characteristics_internal(task_name))
        }

        fn set_mm_thread_characteristics_internal(
            &self,
            task_name: PCWSTR,
        ) -> Result<(u32, HANDLE), WIN32_ERROR> {
            let mut mmcss_task_index = 0u32;
            let task_handle = unsafe {
                (self.av_set_mm_thread_characteristics_w)(task_name, &mut mmcss_task_index)
            };
            win32_error_if(task_handle.is_null())?;
            Ok((mmcss_task_index, task_handle))
        }

        pub(super) fn revert_mm_thread_characteristics(
            &self,
            handle: HANDLE,
        ) -> Result<(), WIN32_ERROR> {
            let rv = unsafe { (self.av_revert_mm_thread_characteristics)(handle) };
            win32_error_if(rv == FALSE)
        }
    }
}

mod win32_utils {
    use windows_sys::{
        core::{PCSTR, PCWSTR},
        Win32::{
            Foundation::{FreeLibrary, GetLastError, HMODULE, WIN32_ERROR},
            System::LibraryLoader::{GetProcAddress, LoadLibraryW},
        },
    };

    pub(super) fn win32_error_if(condition: bool) -> Result<(), WIN32_ERROR> {
        if condition {
            Err(unsafe { GetLastError() })
        } else {
            Ok(())
        }
    }

    #[derive(Debug)]
    pub(super) struct OwnedLibrary(HMODULE);

    impl OwnedLibrary {
        pub(super) fn try_new(lib_file_name: PCWSTR) -> Result<Self, WIN32_ERROR> {
            let module = unsafe { LoadLibraryW(lib_file_name) };
            win32_error_if(module.is_null())?;
            Ok(Self(module))
        }

        fn raw(&self) -> HMODULE {
            self.0
        }

        /// SAFETY: The caller must transmute the value wrapped in a Ok(_) to the correct
        ///         function type, with the correct extern specifier.
        pub(super) unsafe fn get_proc(
            &self,
            proc_name: PCSTR,
        ) -> Result<unsafe extern "system" fn() -> isize, WIN32_ERROR> {
            let proc = unsafe { GetProcAddress(self.raw(), proc_name) };
            win32_error_if(proc.is_none())?;
            Ok(proc.unwrap())
        }
    }

    unsafe impl Send for OwnedLibrary {}
    unsafe impl Sync for OwnedLibrary {}

    impl Drop for OwnedLibrary {
        fn drop(&mut self) {
            unsafe {
                FreeLibrary(self.raw());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        avrt, demote_current_thread_from_real_time_internal,
        promote_current_thread_to_real_time_internal,
    };

    #[test]
    fn test_successful_avrt_library_load() {
        assert!(avrt().is_ok())
    }

    #[test]
    fn test_successful_api_use() {
        let handle = promote_current_thread_to_real_time_internal(512, 44100);
        println!("handle: {handle:?}");
        assert!(handle.is_ok());

        let result = demote_current_thread_from_real_time_internal(handle.unwrap());
        println!("result: {result:?}");
        assert!(result.is_ok());
    }
}
