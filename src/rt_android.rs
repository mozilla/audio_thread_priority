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
    // The thread this handle was captured for. Always the calling thread for
    // `promote_current_thread_to_real_time_internal`, but the promoted thread's tid for
    // `promote_thread_to_real_time_internal` -- storing it here (rather than assuming "the
    // calling thread" on demotion) means demoting always affects the thread that was actually
    // promoted, even if the caller mixes up `demote_current_thread_from_real_time` with
    // `demote_thread_from_real_time`.
    pid: libc::pid_t,
    tid: libc::pid_t,
    // See `RtPriorityThreadInfoInternal::start_time`: carried here too so that even a misuse of
    // this handle for demotion re-validates thread identity rather than trusting a possibly-
    // recycled `tid`.
    start_time: u64,
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
///
/// `start_time` (from `/proc/<pid>/task/<tid>/stat`) is captured alongside `tid` because Linux/
/// Android thread ids are reused once a thread exits: without it, a `tid` captured here could, by
/// the time `promote_thread_to_real_time`/`demote_thread_from_real_time` runs, refer to a
/// completely different, unrelated thread that happened to be assigned the same id in the
/// meantime.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RtPriorityThreadInfoInternal {
    pid: libc::pid_t,
    tid: libc::pid_t,
    previous_priority: libc::c_int,
    start_time: u64,
}

impl RtPriorityThreadInfoInternal {
    /// Serialize to a byte buffer. The fields are packed explicitly rather than transmuting the
    /// struct, matching the approach used on the other platforms, so the format doesn't silently
    /// start reading uninitialized padding if a field is ever reordered or resized.
    pub fn serialize(&self) -> [u8; std::mem::size_of::<Self>()] {
        let pid = self.pid.to_ne_bytes();
        let tid = self.tid.to_ne_bytes();
        let previous_priority = self.previous_priority.to_ne_bytes();
        let start_time = self.start_time.to_ne_bytes();

        let mut bytes = [0u8; std::mem::size_of::<Self>()];
        let fields = pid
            .iter()
            .chain(&tid)
            .chain(&previous_priority)
            .chain(&start_time);
        for (dst, &src) in bytes.iter_mut().zip(fields) {
            *dst = src;
        }
        bytes
    }
    /// Reconstruct from a byte buffer produced by `serialize`.
    pub fn deserialize(bytes: [u8; std::mem::size_of::<Self>()]) -> Self {
        fn take<const N: usize>(src: &mut impl Iterator<Item = u8>) -> [u8; N] {
            let mut chunk = [0u8; N];
            for slot in &mut chunk {
                *slot = src.next().unwrap();
            }
            chunk
        }
        let mut src = bytes.iter().copied();
        RtPriorityThreadInfoInternal {
            pid: libc::pid_t::from_ne_bytes(take(&mut src)),
            tid: libc::pid_t::from_ne_bytes(take(&mut src)),
            previous_priority: libc::c_int::from_ne_bytes(take(&mut src)),
            start_time: u64::from_ne_bytes(take(&mut src)),
        }
    }
    /// Returns the PID of the process containing the thread.
    pub fn pid(&self) -> libc::pid_t {
        self.pid
    }
}

impl PartialEq for RtPriorityThreadInfoInternal {
    // Compares identity only (which thread, in which process, started when), not the captured
    // `previous_priority`, matching `rt_mach.rs`/`rt_linux.rs`. `start_time` is included because
    // it's exactly what distinguishes the original thread from an unrelated one that later reused
    // the same `tid`.
    fn eq(&self, other: &Self) -> bool {
        self.pid == other.pid && self.tid == other.tid && self.start_time == other.start_time
    }
}

/// Read thread `tid` (in process `pid`)'s start time from `/proc/<pid>/task/<tid>/stat`, field 22
/// ("starttime", clock ticks since boot -- see `proc(5)`). Two threads never share a start time,
/// and a given `tid` gets a new one once it's reused by a different thread after the original
/// exits, so comparing this is how thread identity is re-validated across the gap between
/// capturing `RtPriorityThreadInfoInternal` and acting on it later. Reading another process'
/// `/proc/<pid>/task/<tid>/stat` is subject to the same permission rules as `setpriority()` on it
/// (same uid, or `CAP_SYS_NICE`/ptrace access), so this fails closed exactly where the underlying
/// promote/demote would have failed anyway.
fn thread_start_time(pid: libc::pid_t, tid: libc::pid_t) -> Result<u64, AudioThreadPriorityError> {
    let path = format!("/proc/{pid}/task/{tid}/stat");
    let contents = std::fs::read_to_string(&path).map_err(|_| {
        AudioThreadPriorityError::new(&format!("could not read {path} (has the thread exited?)"))
    })?;
    // Format: "<tid> (<comm>) <state> <ppid> ... <starttime> ...". `comm` can itself contain
    // spaces or parentheses, so only split into fields after the last ')'.
    let after_comm = contents
        .rfind(')')
        .map(|i| &contents[i + 1..])
        .ok_or_else(|| AudioThreadPriorityError::new("unexpected /proc/.../stat format"))?;
    // `state` is field 3, the first field after `comm`; `starttime` is field 22, i.e. the token at
    // index 22 - 3 = 19 (zero-based) among the fields following `comm`.
    after_comm
        .split_whitespace()
        .nth(19)
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| AudioThreadPriorityError::new("unexpected /proc/.../stat format"))
}

fn get_thread_priority(tid: libc::pid_t) -> Result<libc::c_int, AudioThreadPriorityError> {
    let who = tid
        .try_into()
        .map_err(|_| AudioThreadPriorityError::new("Invalid thread id"))?;
    unsafe { (*libc::__errno()) = 0 };
    let priority = unsafe { libc::getpriority(libc::PRIO_PROCESS, who) };
    if priority == -1 && unsafe { *libc::__errno() } != 0 {
        return Err(AudioThreadPriorityError::new(
            "Failed to get thread priority",
        ));
    }
    Ok(priority)
}

/// Set a thread's scheduling priority via `setpriority()`, after confirming via
/// `thread_start_time` that `tid` still refers to the same thread `expected_start_time` was
/// captured for. `tid`/`expected_start_time` may come from another process' (deserialized)
/// `RtPriorityThreadInfoInternal`, so this reports an error instead of panicking on an
/// out-of-range value rather than trusting it implicitly. Note there's an inherent, narrow race
/// between the check and `setpriority()` below (the thread could exit and its tid be reused in
/// between); this closes the much wider gap between capture and use, but isn't a full guarantee.
fn set_thread_priority(
    pid: libc::pid_t,
    tid: libc::pid_t,
    expected_start_time: u64,
    priority: libc::c_int,
) -> Result<(), AudioThreadPriorityError> {
    let start_time = thread_start_time(pid, tid)?;
    if start_time != expected_start_time {
        return Err(AudioThreadPriorityError::new(&format!(
            "thread {tid} in process {pid} has exited and its id was reused by another thread"
        )));
    }

    let who = tid
        .try_into()
        .map_err(|_| AudioThreadPriorityError::new("Invalid thread id"))?;
    let r = unsafe { libc::setpriority(libc::PRIO_PROCESS, who, priority) };
    if r < 0 {
        return Err(AudioThreadPriorityError::new(
            "Failed to set thread priority",
        ));
    }
    Ok(())
}

/// Get the current thread information, as an opaque struct, that can be serialized and sent
/// across processes, to have another thread promoted to real-time.
pub fn get_current_thread_info_internal(
) -> Result<RtPriorityThreadInfoInternal, AudioThreadPriorityError> {
    let pid = unsafe { libc::getpid() };
    let tid = unsafe { libc::gettid() };
    let previous_priority = get_thread_priority(tid)?;
    let start_time = thread_start_time(pid, tid)?;

    Ok(RtPriorityThreadInfoInternal {
        pid,
        tid,
        previous_priority,
        start_time,
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
    //
    // Uses the identity captured in the handle (rather than assuming "the calling thread") so
    // that this still demotes the correct thread even if `h` came from `promote_thread_to_real_time`
    // for a thread other than the caller.
    set_thread_priority(h.pid, h.tid, h.start_time, h.previous_priority)
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
    set_thread_priority(
        thread_info.pid,
        thread_info.tid,
        thread_info.start_time,
        THREAD_PRIORITY_URGENT_AUDIO,
    )?;

    Ok(RtPriorityHandleInternal {
        pid: thread_info.pid,
        tid: thread_info.tid,
        start_time: thread_info.start_time,
        previous_priority: thread_info.previous_priority,
    })
}

/// This can be called by sandboxed code, it restores the priority the thread had when
/// `get_current_thread_info` captured `thread_info`.
pub fn demote_thread_from_real_time_internal(
    thread_info: RtPriorityThreadInfoInternal,
) -> Result<(), AudioThreadPriorityError> {
    set_thread_priority(
        thread_info.pid,
        thread_info.tid,
        thread_info.start_time,
        thread_info.previous_priority,
    )
}
