use crate::AudioThreadPriorityError;
use once_cell::sync;
use windows_sys::core::PCWSTR;
use windows_sys::s;
use windows_sys::w;
use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Foundation::BOOL;
use windows_sys::Win32::Foundation::FALSE;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::Foundation::WIN32_ERROR;
use windows_sys::Win32::System::LibraryLoader::GetProcAddress;
use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;

use log::info;

#[derive(Debug)]
pub struct RtPriorityHandleInternal {
    mmcss_task_index: u32,
    task_handle: HANDLE,
}

impl RtPriorityHandleInternal {
    pub fn new(mmcss_task_index: u32, task_handle: HANDLE) -> RtPriorityHandleInternal {
        RtPriorityHandleInternal {
            mmcss_task_index,
            task_handle,
        }
    }
}

pub fn demote_current_thread_from_real_time_internal(
    rt_priority_handle: RtPriorityHandleInternal,
) -> Result<(), AudioThreadPriorityError> {
    let rv = unsafe {
        (av_rt_library()?.av_revert_mm_thread_characteristics)(rt_priority_handle.task_handle)
    };
    if rv == FALSE {
        return Err(AudioThreadPriorityError::new(&format!(
            "Unable to restore the thread priority ({:?})",
            unsafe { GetLastError() }
        )));
    }

    info!(
        "task {} priority restored.",
        rt_priority_handle.mmcss_task_index
    );

    Ok(())
}

pub fn promote_current_thread_to_real_time_internal(
    _audio_buffer_frames: u32,
    _audio_samplerate_hz: u32,
) -> Result<RtPriorityHandleInternal, AudioThreadPriorityError> {
    let mut task_index = 0u32;
    let handle = unsafe {
        (av_rt_library()?.av_set_mm_thread_characteristics_w)(w!("Audio"), &mut task_index)
    };
    let handle = RtPriorityHandleInternal::new(task_index, handle);

    if handle.task_handle == 0 {
        return Err(AudioThreadPriorityError::new(&format!(
            "Unable to restore the thread priority ({:?})",
            unsafe { GetLastError() }
        )));
    }

    info!(
        "task {} bumped to real time priority.",
        handle.mmcss_task_index
    );

    Ok(handle)
}

// We don't expect to see API failures on test machines
#[test]
fn test_successful_api_use() {
    let handle = promote_current_thread_to_real_time_internal(0, 0);
    assert!(handle.is_ok());
    assert!(demote_current_thread_from_real_time_internal(handle.unwrap()).is_ok());
}

fn av_rt_library() -> Result<&'static AvRtLibrary, AudioThreadPriorityError> {
    static AV_RT_LIBRARY: sync::OnceCell<Result<AvRtLibrary, WIN32_ERROR>> = sync::OnceCell::new();
    AV_RT_LIBRARY
        .get_or_init(AvRtLibrary::try_new)
        .as_ref()
        .map_err(|win32_error| {
            AudioThreadPriorityError::new(&format!("Unable to load avrt.dll ({win32_error})"))
        })
}

// We don't expect to fail to load the library on test machines
#[test]
fn test_successful_avrt_library_load_as_static_ref() {
    assert!(av_rt_library().is_ok())
}

#[derive(Debug)]
struct AvRtLibrary {
    module: HMODULE,
    av_set_mm_thread_characteristics_w: unsafe fn(PCWSTR, *mut u32) -> HANDLE,
    av_revert_mm_thread_characteristics: unsafe fn(HANDLE) -> BOOL,
}

impl AvRtLibrary {
    fn try_new() -> Result<Self, WIN32_ERROR> {
        let module = unsafe { LoadLibraryW(w!("avrt.dll")) };
        if module != 0 {
            let set_fn = unsafe { GetProcAddress(module, s!("AvSetMmThreadCharacteristicsW")) };
            if let Some(set_fn) = set_fn {
                let revert_fn =
                    unsafe { GetProcAddress(module, s!("AvRevertMmThreadCharacteristics")) };
                if let Some(revert_fn) = revert_fn {
                    let av_set_mm_thread_characteristics_w = unsafe {
                        std::mem::transmute::<_, unsafe fn(PCWSTR, *mut u32) -> HANDLE>(set_fn)
                    };
                    let av_revert_mm_thread_characteristics =
                        unsafe { std::mem::transmute::<_, unsafe fn(HANDLE) -> BOOL>(revert_fn) };
                    return Ok(AvRtLibrary {
                        module,
                        av_set_mm_thread_characteristics_w,
                        av_revert_mm_thread_characteristics,
                    });
                }
            }
        }
        let win32_error = unsafe { GetLastError() };
        if module != 0 {
            unsafe { FreeLibrary(module) };
        }
        Err(win32_error)
    }
}

impl Drop for AvRtLibrary {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.module);
        }
    }
}

// We don't expect to fail to load the library on test machines
#[test]
fn test_successful_temporary_avrt_library_load() {
    assert!(AvRtLibrary::try_new().is_ok())
}
