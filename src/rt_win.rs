use windows::Win32::Foundation::GetLastError;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Foundation::PSTR;
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsA,
};

use crate::AudioThreadPriorityError;

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
    let rv = unsafe { AvRevertMmThreadCharacteristics(rt_priority_handle.task_handle) };
    if !rv.as_bool() {
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

    let handle =
        unsafe { AvSetMmThreadCharacteristicsA(PSTR("Audio\0".as_ptr()), &mut task_index) };
    let handle = RtPriorityHandleInternal::new(task_index, handle);

    if handle.task_handle.is_invalid() {
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
