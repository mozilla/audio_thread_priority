use crate::AudioThreadPriorityError;
use libc::{pthread_mach_thread_np, pthread_self, pthread_threadid_np, thread_policy_t};
use log::info;
use mach2::boolean::boolean_t;
use mach2::kern_return::{kern_return_t, KERN_SUCCESS};
use mach2::mach_port::mach_port_deallocate;
use mach2::mach_time::{mach_timebase_info, mach_timebase_info_data_t};
use mach2::mach_types::{task_t, thread_act_array_t, thread_act_t};
use mach2::message::mach_msg_type_number_t;
use mach2::port::{mach_port_t, MACH_PORT_NULL};
use mach2::task::task_threads;
use mach2::thread_policy::{
    thread_policy_get, thread_policy_set, thread_time_constraint_policy_data_t,
    THREAD_TIME_CONSTRAINT_POLICY, THREAD_TIME_CONSTRAINT_POLICY_COUNT,
};
use mach2::traps::{mach_task_self, task_for_pid};
use mach2::vm::mach_vm_deallocate;
use mach2::vm_types::natural_t;

// Not exposed by the `mach2` crate: mirrors `mach/thread_info.h`.
const THREAD_IDENTIFIER_INFO: natural_t = 4;
const THREAD_IDENTIFIER_INFO_COUNT: mach_msg_type_number_t = 6;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ThreadIdentifierInfo {
    thread_id: u64,
    thread_handle: u64,
    dispatch_qaddr: u64,
}

extern "C" {
    fn thread_info(
        target_act: thread_act_t,
        flavor: natural_t,
        thread_info_out: *mut i32,
        thread_info_out_count: *mut mach_msg_type_number_t,
    ) -> kern_return_t;
}

#[derive(Debug)]
pub struct RtPriorityHandleInternal {
    tid: mach_port_t,
    previous_time_constraint_policy: thread_time_constraint_policy_data_t,
}

impl Default for RtPriorityHandleInternal {
    fn default() -> Self {
        Self::new()
    }
}

impl RtPriorityHandleInternal {
    pub fn new() -> RtPriorityHandleInternal {
        RtPriorityHandleInternal {
            tid: 0,
            previous_time_constraint_policy: thread_time_constraint_policy_data_t {
                period: 0,
                computation: 0,
                constraint: 0,
                preemptible: 0,
            },
        }
    }
}

/// Opaque, serializable information about a thread sufficient to promote or demote it to/from
/// real-time priority.
///
/// This doesn't contain a mach port: port names are only meaningful within the task that owns
/// them, so a raw port name can't be shipped across processes. Instead, this carries a stable,
/// system-wide thread identifier (from `pthread_threadid_np`) and the owning process' pid, and
/// the port is (re-)resolved via `task_threads`/`thread_info` whenever it's needed, in whichever
/// process ends up doing the promotion.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RtPriorityThreadInfoInternal {
    pid: libc::pid_t,
    thread_id: u64,
    previous_time_constraint_policy: thread_time_constraint_policy_data_t,
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
        self.pid == other.pid && self.thread_id == other.thread_id
    }
}

/// Resolve a `(pid, thread_id)` pair, as captured by `get_current_thread_info_internal`, into a
/// thread port that can be passed to `thread_policy_set`/`thread_policy_get`.
///
/// This works unconditionally for a thread running in the current process. For a thread in
/// another process, this requires being able to get a send right to that process' task port
/// (via `task_for_pid`), which needs elevated privileges or the
/// `com.apple.security.get-task-allow` entitlement.
///
/// The returned port is a right owned by the caller, and must be released with
/// `mach_port_deallocate` once done with it.
fn resolve_thread_port(
    pid: libc::pid_t,
    thread_id: u64,
) -> Result<mach_port_t, AudioThreadPriorityError> {
    let is_current_process = pid == unsafe { libc::getpid() };

    let task: task_t = if is_current_process {
        unsafe { mach_task_self() }
    } else {
        let mut task_port: mach_port_t = MACH_PORT_NULL;
        let kr = unsafe { task_for_pid(mach_task_self(), pid, &mut task_port) };
        if kr != KERN_SUCCESS {
            return Err(AudioThreadPriorityError::new(&format!(
                "task_for_pid failed for pid {pid} (kern_return_t {kr}): promoting a thread in \
                 another process requires elevated privileges or the \
                 com.apple.security.get-task-allow entitlement"
            )));
        }
        task_port
    };

    let mut thread_list: thread_act_array_t = std::ptr::null_mut();
    let mut thread_count: mach_msg_type_number_t = 0;

    let kr = unsafe { task_threads(task, &mut thread_list, &mut thread_count) };
    if kr != KERN_SUCCESS {
        if !is_current_process {
            unsafe { mach_port_deallocate(mach_task_self(), task) };
        }
        return Err(AudioThreadPriorityError::new("task_threads failed"));
    }

    let mut found: Option<thread_act_t> = None;
    for i in 0..thread_count {
        let port = unsafe { *thread_list.add(i as usize) };
        let mut ident = ThreadIdentifierInfo::default();
        let mut count = THREAD_IDENTIFIER_INFO_COUNT;
        let kr = unsafe {
            thread_info(
                port,
                THREAD_IDENTIFIER_INFO,
                &mut ident as *mut _ as *mut i32,
                &mut count,
            )
        };
        if found.is_none() && kr == KERN_SUCCESS && ident.thread_id == thread_id {
            found = Some(port);
        } else {
            unsafe { mach_port_deallocate(mach_task_self(), port) };
        }
    }

    unsafe {
        mach_vm_deallocate(
            mach_task_self(),
            thread_list as u64,
            (thread_count as usize * std::mem::size_of::<thread_act_t>()) as u64,
        );
    }

    if !is_current_process {
        unsafe { mach_port_deallocate(mach_task_self(), task) };
    }

    found.ok_or_else(|| {
        AudioThreadPriorityError::new(&format!(
            "could not find thread {thread_id} in process {pid} (has it exited?)"
        ))
    })
}

fn get_time_constraint_policy(
    port: mach_port_t,
) -> Result<thread_time_constraint_policy_data_t, AudioThreadPriorityError> {
    let mut policy = thread_time_constraint_policy_data_t {
        period: 0,
        computation: 0,
        constraint: 0,
        preemptible: 0,
    };
    let mut get_default: boolean_t = 0;
    let mut count: mach_msg_type_number_t = THREAD_TIME_CONSTRAINT_POLICY_COUNT;
    let rv = unsafe {
        thread_policy_get(
            port,
            THREAD_TIME_CONSTRAINT_POLICY,
            (&mut policy) as *mut _ as thread_policy_t,
            &mut count,
            &mut get_default,
        )
    };
    if rv != KERN_SUCCESS {
        return Err(AudioThreadPriorityError::new(
            "thread_policy_get: time_constraint",
        ));
    }
    Ok(policy)
}

fn set_time_constraint_policy(
    port: mach_port_t,
    mut policy: thread_time_constraint_policy_data_t,
) -> Result<(), AudioThreadPriorityError> {
    let rv = unsafe {
        thread_policy_set(
            port,
            THREAD_TIME_CONSTRAINT_POLICY,
            (&mut policy) as *mut _ as thread_policy_t,
            THREAD_TIME_CONSTRAINT_POLICY_COUNT,
        )
    };
    if rv != KERN_SUCCESS {
        return Err(AudioThreadPriorityError::new(
            "thread_policy_set: time_constraint",
        ));
    }
    Ok(())
}

/// The time constraint calculations are somewhat arbitrary for now.
fn compute_time_constraint_policy(
    audio_buffer_frames: u32,
    audio_samplerate_hz: u32,
) -> thread_time_constraint_policy_data_t {
    let buffer_frames = if audio_buffer_frames > 0 {
        audio_buffer_frames
    } else {
        audio_samplerate_hz / 20
    };

    let mut timebase_info = mach_timebase_info_data_t { denom: 0, numer: 0 };
    unsafe { mach_timebase_info(&mut timebase_info) };

    let ms2abs: f32 = ((timebase_info.denom as f32) / timebase_info.numer as f32) * 1000000.;

    let cb_duration = buffer_frames as f32 / (audio_samplerate_hz as f32) * 1000.;

    // Computation time is half of constraint, per macOS 12 behaviour.  And capped at 50ms per
    // macOS limits:
    // https://github.com/apple-oss-distributions/xnu/blob/e3723e1f17661b24996789d8afc084c0c3303b26/osfmk/kern/thread_policy.c#L408
    // https://github.com/apple-oss-distributions/xnu/blob/e3723e1f17661b24996789d8afc084c0c3303b26/osfmk/kern/sched_prim.c#L822
    const MAX_RT_QUANTUM: f32 = 50.0;
    let computation = cb_duration / 2.0;
    let computation = if computation > MAX_RT_QUANTUM {
        info!("thread computation time capped at {MAX_RT_QUANTUM}ms ({computation}ms requested).");
        MAX_RT_QUANTUM
    } else {
        computation
    };

    thread_time_constraint_policy_data_t {
        period: (cb_duration * ms2abs) as u32,
        computation: (computation * ms2abs) as u32,
        constraint: (cb_duration * ms2abs) as u32,
        preemptible: 1, // true
    }
}

pub fn demote_current_thread_from_real_time_internal(
    rt_priority_handle: RtPriorityHandleInternal,
) -> Result<(), AudioThreadPriorityError> {
    set_time_constraint_policy(
        rt_priority_handle.tid,
        rt_priority_handle.previous_time_constraint_policy,
    )?;

    info!("thread {} priority restored.", rt_priority_handle.tid);

    Ok(())
}

/// Get the current thread information, as an opaque struct, that can be serialized and sent
/// accross processes. This is enough to capture the current state of the scheduling policy, and
/// an identifier to have another thread promoted to real-time.
pub fn get_current_thread_info_internal(
) -> Result<RtPriorityThreadInfoInternal, AudioThreadPriorityError> {
    let pid = unsafe { libc::getpid() };

    let mut thread_id: u64 = 0;
    // A null (0) `pthread_t` means "the current thread".
    if unsafe { pthread_threadid_np(0, &mut thread_id) } != 0 {
        return Err(AudioThreadPriorityError::new("pthread_threadid_np failed"));
    }

    let tid: mach_port_t = unsafe { pthread_mach_thread_np(pthread_self()) };
    let previous_time_constraint_policy = get_time_constraint_policy(tid)?;

    Ok(RtPriorityThreadInfoInternal {
        pid,
        thread_id,
        previous_time_constraint_policy,
    })
}

pub fn promote_current_thread_to_real_time_internal(
    audio_buffer_frames: u32,
    audio_samplerate_hz: u32,
) -> Result<RtPriorityHandleInternal, AudioThreadPriorityError> {
    let thread_info = get_current_thread_info_internal()?;
    let tid: mach_port_t = unsafe { pthread_mach_thread_np(pthread_self()) };

    let policy = compute_time_constraint_policy(audio_buffer_frames, audio_samplerate_hz);
    set_time_constraint_policy(tid, policy)?;

    info!("thread {tid} bumped to real time priority.");

    Ok(RtPriorityHandleInternal {
        tid,
        previous_time_constraint_policy: thread_info.previous_time_constraint_policy,
    })
}

/// Promote a thread (possibly in another process) identified by its thread info, to real-time.
pub fn promote_thread_to_real_time_internal(
    thread_info: RtPriorityThreadInfoInternal,
    audio_buffer_frames: u32,
    audio_samplerate_hz: u32,
) -> Result<RtPriorityHandleInternal, AudioThreadPriorityError> {
    let port = resolve_thread_port(thread_info.pid, thread_info.thread_id)?;

    let policy = compute_time_constraint_policy(audio_buffer_frames, audio_samplerate_hz);
    let rv = set_time_constraint_policy(port, policy);

    unsafe { mach_port_deallocate(mach_task_self(), port) };
    rv?;

    info!(
        "thread {} (pid {}) bumped to real time priority.",
        thread_info.thread_id, thread_info.pid
    );

    // The returned handle isn't used to demote this thread: `demote_thread_from_real_time`
    // resolves the thread again from `thread_info` instead, since the promoting and demoting
    // call may not happen on the same thread, or even in the same process.
    Ok(RtPriorityHandleInternal {
        tid: MACH_PORT_NULL,
        previous_time_constraint_policy: thread_info.previous_time_constraint_policy,
    })
}

/// This can be called by sandboxed code, it only restores priority to what they were.
pub fn demote_thread_from_real_time_internal(
    thread_info: RtPriorityThreadInfoInternal,
) -> Result<(), AudioThreadPriorityError> {
    let port = resolve_thread_port(thread_info.pid, thread_info.thread_id)?;

    let rv = set_time_constraint_policy(port, thread_info.previous_time_constraint_policy);

    unsafe { mach_port_deallocate(mach_task_self(), port) };
    rv
}
