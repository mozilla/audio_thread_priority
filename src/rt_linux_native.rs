/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this file,
 * You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Native Linux real-time promotion, used when the crate is built without the `dbus` feature.
//!
//! Instead of asking rtkit over D-Bus, this promotes the thread directly with
//! `pthread_setschedparam(SCHED_FIFO)`. It needs no D-Bus and no rtkit daemon, and works whenever
//! the process is allowed to request real-time scheduling: running as root, holding `CAP_SYS_NICE`,
//! or with an `RLIMIT_RTPRIO` limit configured (e.g. systemd `LimitRTPRIO` or
//! `/etc/security/limits.conf`). This is the mechanism JACK and PipeWire's direct mode use.

extern crate libc;

use std::io::Error as OSError;

use crate::AudioThreadPriorityError;

/// Default real-time priority to request, when `AUDIO_RT_PRIORITY` is unset. Matches the value the
/// rtkit path already asks for.
const RT_PRIO_DEFAULT: libc::c_int = 10;

/// Environment variable to override the requested real-time priority. Accepts an integer 1-99.
/// Higher values preempt more work but must stay below the audio interface's IRQ threads, or they
/// starve the very threads that deliver the audio.
const RT_PRIORITY_ENV: &str = "AUDIO_RT_PRIORITY";

/// The real-time priority to request, from `AUDIO_RT_PRIORITY` or the default.
fn requested_priority() -> libc::c_int {
    match std::env::var(RT_PRIORITY_ENV) {
        Ok(value) => match value.trim().parse::<libc::c_int>() {
            Ok(priority) if (1..=99).contains(&priority) => priority,
            _ => {
                log::warn!(
                    "Ignoring invalid {RT_PRIORITY_ENV}=\"{value}\", expected an integer 1-99. \
                     Using default {RT_PRIO_DEFAULT}."
                );
                RT_PRIO_DEFAULT
            }
        },
        Err(_) => RT_PRIO_DEFAULT,
    }
}

/// Prevents threads/processes forked from a real-time thread from inheriting real-time scheduling.
/// Not exposed by libc: <https://github.com/rust-lang/libc/issues/1511>
const SCHED_RESET_ON_FORK: libc::c_int = 0x4000_0000;

// This is different from libc::pid_t, which is 32 bits, and is defined in sys/types.h.
#[allow(non_camel_case_types)]
type kernel_pid_t = libc::c_long;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RtPriorityThreadInfoInternal {
    /// System-wide thread id (tid), used to promote a thread by id.
    thread_id: kernel_pid_t,
    /// Process-local thread id, used to restore scheduler characteristics.
    pthread_id: libc::pthread_t,
    /// The PID of the process containing `thread_id`.
    pid: libc::pid_t,
    /// The scheduling policy in place before promotion, to restore on demotion.
    policy: libc::c_int,
    /// The scheduling parameters in place before promotion, to restore on demotion.
    param: libc::sched_param,
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

impl PartialEq for RtPriorityThreadInfoInternal {
    fn eq(&self, other: &Self) -> bool {
        self.thread_id == other.thread_id && self.pthread_id == other.pthread_id
    }
}

pub struct RtPriorityHandleInternal {
    thread_info: RtPriorityThreadInfoInternal,
}

/// The POSIX `pthread_*` functions return the error number directly and do not set `errno`, so the
/// return code must be converted with `Error::from_raw_os_error`, not read via `last_os_error`.
fn pthread_error(context: &str, rc: libc::c_int) -> AudioThreadPriorityError {
    AudioThreadPriorityError::new(&format!("{}: {}", context, OSError::from_raw_os_error(rc)))
}

/// The `sched_*` functions are thin syscall wrappers: they return -1 and set `errno`.
fn sched_error(context: &str) -> AudioThreadPriorityError {
    AudioThreadPriorityError::new(&format!("{}: {}", context, OSError::last_os_error()))
}

/// Get the current thread information, capturing enough to promote or demote it later. This mirrors
/// the rtkit path so the same public API works, but the returned struct is only meaningful within
/// this process (the native path promotes directly rather than via a privileged helper).
pub fn get_current_thread_info_internal(
) -> Result<RtPriorityThreadInfoInternal, AudioThreadPriorityError> {
    let thread_id = unsafe { libc::syscall(libc::SYS_gettid) };
    let pthread_id = unsafe { libc::pthread_self() };
    let pid = unsafe { libc::getpid() };
    let mut policy = 0;
    let mut param = unsafe { std::mem::zeroed::<libc::sched_param>() };

    let rc = unsafe { libc::pthread_getschedparam(pthread_id, &mut policy, &mut param) };
    if rc != 0 {
        return Err(pthread_error("pthread_getschedparam", rc));
    }

    Ok(RtPriorityThreadInfoInternal {
        thread_id,
        pthread_id,
        pid,
        policy,
        param,
    })
}

/// Promote the calling thread to real-time priority using `SCHED_FIFO`.
///
/// The buffer size and sample rate are unused here (they matter only for the rtkit path, which
/// derives an `RLIMIT_RTTIME` budget from them); the signature is kept to match the other backends.
pub fn promote_current_thread_to_real_time_internal(
    _audio_buffer_frames: u32,
    _audio_samplerate_hz: u32,
) -> Result<RtPriorityHandleInternal, AudioThreadPriorityError> {
    let thread_info = get_current_thread_info_internal()?;

    let mut param = unsafe { std::mem::zeroed::<libc::sched_param>() };
    param.sched_priority = requested_priority();

    let rc = unsafe {
        libc::pthread_setschedparam(
            thread_info.pthread_id,
            libc::SCHED_FIFO | SCHED_RESET_ON_FORK,
            &param,
        )
    };
    if rc != 0 {
        return Err(pthread_error("could not promote thread", rc));
    }

    Ok(RtPriorityHandleInternal { thread_info })
}

/// Restore the calling thread to the scheduling policy and parameters it had before promotion.
pub fn demote_current_thread_from_real_time_internal(
    rt_priority_handle: RtPriorityHandleInternal,
) -> Result<(), AudioThreadPriorityError> {
    let RtPriorityThreadInfoInternal {
        pthread_id,
        policy,
        param,
        ..
    } = rt_priority_handle.thread_info;

    // Keep SCHED_RESET_ON_FORK set: the kernel forbids an unprivileged thread from clearing that
    // flag once set (and promotion set it), so restoring the bare saved policy would fail with
    // EPERM. The flag is harmless on a non-real-time thread.
    let rc =
        unsafe { libc::pthread_setschedparam(pthread_id, policy | SCHED_RESET_ON_FORK, &param) };
    if rc != 0 {
        return Err(pthread_error("could not demote thread", rc));
    }
    Ok(())
}

/// Promote a thread identified by its tid to real-time priority. Promoting a thread other than the
/// caller (in particular in another process) requires the caller to be privileged.
pub fn promote_thread_to_real_time_internal(
    thread_info: RtPriorityThreadInfoInternal,
    _audio_buffer_frames: u32,
    _audio_samplerate_hz: u32,
) -> Result<RtPriorityHandleInternal, AudioThreadPriorityError> {
    let mut param = unsafe { std::mem::zeroed::<libc::sched_param>() };
    param.sched_priority = requested_priority();

    let rc = unsafe {
        libc::sched_setscheduler(
            thread_info.thread_id as libc::pid_t,
            libc::SCHED_FIFO | SCHED_RESET_ON_FORK,
            &param,
        )
    };
    if rc < 0 {
        return Err(sched_error("could not promote thread"));
    }

    Ok(RtPriorityHandleInternal { thread_info })
}

/// Restore a thread identified by its tid to the policy and parameters it had before promotion.
pub fn demote_thread_from_real_time_internal(
    thread_info: RtPriorityThreadInfoInternal,
) -> Result<(), AudioThreadPriorityError> {
    // Keep SCHED_RESET_ON_FORK set (see demote_current_thread_from_real_time_internal): clearing it
    // as an unprivileged thread would fail with EPERM.
    let rc = unsafe {
        libc::sched_setscheduler(
            thread_info.thread_id as libc::pid_t,
            thread_info.policy | SCHED_RESET_ON_FORK,
            &thread_info.param,
        )
    };
    if rc < 0 {
        return Err(sched_error("could not demote thread"));
    }
    Ok(())
}

/// Setting an `RLIMIT_RTTIME` budget is only needed by the rtkit path. The native path relies on
/// the kernel's real-time throttling (`sched_rt_runtime_us`) instead, so this is a no-op.
pub fn set_real_time_hard_limit_internal(
    _audio_buffer_frames: u32,
    _audio_samplerate_hz: u32,
) -> Result<(), AudioThreadPriorityError> {
    Ok(())
}
