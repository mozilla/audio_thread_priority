/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this file,
 * You can obtain one at http://mozilla.org/MPL/2.0/. */

extern crate libc;
use crate::AudioThreadPriorityError;
use std::convert::TryInto;

// https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/core/java/android/os/Process.java#474
const THREAD_PRIORITY_URGENT_AUDIO: libc::c_int = -19;

#[derive(Debug)]
pub struct RtPriorityHandleInternal {
    previous_priority: libc::c_int,
}

/// Opaque, serializable information about a thread, possibly running in another process,
/// sufficient to promote or demote it to/from real-time priority.
///
/// Android runs on the Linux kernel, and `setpriority()`/`getpriority()` with `PRIO_PROCESS`
/// already operate on an individual kernel task (thread) given its `tid`, not just the calling
/// thread, so no daemon or extra IPC is needed to target another therad. Doing so across
/// processes additionally requires the caller to have the right privileges (matching uid, or
/// `CAP_SYS_NICE`) to renice that thread, which is not generally available to sandboxed Android
/// apps targeting another process, but works for another thread in the same process.
#[repr(C)]
#[derive(Clone, Copy, PartialEq)]
pub struct RtPriorityThreadInfoInternal {
    pid: libc::pid_t,
    tid: libc::pid_t,
    previous_priority: libc::c_int,
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
    pub fn pid(&self) -> libc::pid_t {
        self.pid
    }
}

fn get_thread_priority(tid: libc::pid_t) -> Result<libc::c_int, AudioThreadPriorityError> {
    let who = tid.try_into().unwrap();
    unsafe { (*libc::__errno()) = 0 };
    let priority = unsafe { libc::getpriority(libc::PRIO_PROCESS, who) };
    if priority == -1 && unsafe { *libc::__errno() } != 0 {
        return Err(AudioThreadPriorityError::new(
            "Failed to get thread priority",
        ));
    }
    Ok(priority)
}

/// Get the current thread information, as an opaque struct, that can be serialized and sent
/// across processes, to have another thread promoted to real-time.
pub fn get_current_thread_info_internal(
) -> Result<RtPriorityThreadInfoInternal, AudioThreadPriorityError> {
    let tid = unsafe { libc::gettid() };
    let previous_priority = get_thread_priority(tid)?;

    Ok(RtPriorityThreadInfoInternal {
        pid: unsafe { libc::getpid() },
        tid,
        previous_priority,
    })
}

pub fn promote_current_thread_to_real_time_internal(
    audio_buffer_frames: u32,
    audio_samplerate_hz: u32,
) -> Result<RtPriorityHandleInternal, AudioThreadPriorityError> {
    let thread_info = get_current_thread_info_internal()?;
    promote_thread_to_real_time_internal(thread_info, audio_buffer_frames, audio_samplerate_hz)
}

pub fn demote_current_thread_from_real_time_internal(
    h: RtPriorityHandleInternal,
) -> Result<(), AudioThreadPriorityError> {
    // Per https://github.com/android/ndk/issues/1255
    // and https://android.googlesource.com/platform/bionic/+/master/libc/include/pthread.h#388,
    // it's acceptable to call setpriority() directly for native threads.
    let who = unsafe { libc::gettid().try_into().unwrap() };
    let r = unsafe { libc::setpriority(libc::PRIO_PROCESS, who, h.previous_priority) };
    if r < 0 {
        return Err(AudioThreadPriorityError::new(
            "Failed to demote thread priority",
        ));
    }
    Ok(())
}

/// Promote a thread (possibly in another process) identified by its thread info, to real-time.
pub fn promote_thread_to_real_time_internal(
    thread_info: RtPriorityThreadInfoInternal,
    _audio_buffer_frames: u32,
    _audio_samplerate_hz: u32,
) -> Result<RtPriorityHandleInternal, AudioThreadPriorityError> {
    // Android's Process.setThreadPriority() ultimately calls setpriority().
    // See https://android.googlesource.com/platform/frameworks/base/+/master/core/jni/android_util_Process.cpp#543
    // and https://android.googlesource.com/platform/system/core/+/master/libutils/Threads.cpp#312
    let who = thread_info.tid.try_into().unwrap();

    let r = unsafe { libc::setpriority(libc::PRIO_PROCESS, who, THREAD_PRIORITY_URGENT_AUDIO) };
    if r < 0 {
        return Err(AudioThreadPriorityError::new(
            "Failed to set thread priority",
        ));
    }

    Ok(RtPriorityHandleInternal {
        previous_priority: thread_info.previous_priority,
    })
}

/// This can be called by sandboxed code, it restores the priority the thread had when
/// `get_current_thread_info` captured `thread_info`.
pub fn demote_thread_from_real_time_internal(
    thread_info: RtPriorityThreadInfoInternal,
) -> Result<(), AudioThreadPriorityError> {
    let who = thread_info.tid.try_into().unwrap();
    let r = unsafe { libc::setpriority(libc::PRIO_PROCESS, who, thread_info.previous_priority) };
    if r < 0 {
        return Err(AudioThreadPriorityError::new(
            "Failed to demote thread priority",
        ));
    }
    Ok(())
}
