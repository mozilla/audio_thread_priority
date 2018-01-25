use mach::kern_return::kern_return_t;
use mach::port::mach_port_t;
use mach_sys::*;
use libc::{pthread_self, pthread_t};
use std::mem::size_of;

extern "C" {
    fn pthread_mach_thread_np(tid: pthread_t) -> mach_port_t;
    // Those functions are commented out in thread_policy.h but somehow it works just fine !?
    fn thread_policy_set(thread: thread_t,
                         flavor: thread_policy_flavor_t,
                         policy_info: thread_policy_t,
                         count: mach_msg_type_number_t)
                         -> kern_return_t;
    fn thread_policy_get(thread: thread_t,
                         flavor: thread_policy_flavor_t,
                         policy_info: thread_policy_t,
                         count: &mut mach_msg_type_number_t,
                         get_default: &mut boolean_t)
                         -> kern_return_t;
}

// can't use size_of in const fn just now in stable, use a macro for now.
macro_rules! THREAD_EXTENDED_POLICY_COUNT {
    () => {
        (size_of::<thread_extended_policy_data_t>() / size_of::<integer_t>()) as u32;
    }
}

macro_rules! THREAD_PRECEDENCE_POLICY_COUNT {
    () => {
        (size_of::<thread_precedence_policy_data_t>() / size_of::<integer_t>()) as u32;
    }
}

macro_rules! THREAD_TIME_CONSTRAINT_POLICY_COUNT {
    () => {
        (size_of::<thread_time_constraint_policy_data_t>() / size_of::<integer_t>()) as u32;
    }
}

#[derive(Debug)]
pub struct RtPriorityHandle {
    tid: mach_port_t,
    previous_time_share: thread_extended_policy_data_t,
    previous_precedence_policy: thread_precedence_policy_data_t,
    previous_time_constraint_policy: thread_time_constraint_policy_data_t,
}

impl RtPriorityHandle {
    pub fn new() -> RtPriorityHandle {
        return RtPriorityHandle {
            tid: 0,
            previous_time_share: thread_extended_policy_data_t { timeshare: 0 },
            previous_precedence_policy: thread_precedence_policy_data_t { importance: 0},
            previous_time_constraint_policy: thread_time_constraint_policy_data_t {
                period: 0,
                computation: 0,
                constraint: 0,
                preemptible: 0
            }
        }
    }
}

pub fn demote_current_thread_from_real_time(rt_priority_handle: RtPriorityHandle)
                                            -> Result<(), ()> {
    unsafe {
        let mut rv: kern_return_t;
        let mut h = rt_priority_handle;
        rv = thread_policy_set(h.tid,
                               THREAD_EXTENDED_POLICY,
                               (&mut h.previous_time_share) as *mut _ as thread_policy_t,
                               THREAD_EXTENDED_POLICY_COUNT!());

        if rv != KERN_SUCCESS as i32 {
            error!("thread demotion error: thread_policy_set: extended");
            return Err(());
        }

        rv = thread_policy_set(h.tid,
                               THREAD_PRECEDENCE_POLICY,
                               (&mut h.previous_precedence_policy) as *mut _ as thread_policy_t,
                               THREAD_PRECEDENCE_POLICY_COUNT!());

        if rv != KERN_SUCCESS as i32 {
            error!("thread demotion error: thread_policy_set: precedence");
            return Err(());
        }

        rv = thread_policy_set(h.tid,
                               THREAD_TIME_CONSTRAINT_POLICY,
                               (&mut h.previous_time_constraint_policy) as *mut _ as
                               thread_policy_t,
                               THREAD_TIME_CONSTRAINT_POLICY_COUNT!());
        if rv != KERN_SUCCESS as i32 {
            error!("thread demotion error: thread_policy_set: RT");
            return Err(());
        }

        info!("thread {} priority restored.", h.tid);
    }

    return Ok(());
}

pub fn promote_current_thread_to_real_time(audio_buffer_frames: u32,
                                           audio_samplerate_hz: u32)
                                           -> Result<RtPriorityHandle, ()> {

    let mut rt_priority_handle = RtPriorityHandle::new();

    // Get current thread attributes, to revert back to the correct setting later if needed.

    unsafe {
        let tid: mach_port_t = pthread_mach_thread_np(pthread_self());
        let mut rv: kern_return_t;
        let mut policy = thread_extended_policy_data_t { timeshare: 0 };
        let mut precedence = thread_precedence_policy_data_t { importance: 0 };
        let mut time_constraints = thread_time_constraint_policy_data_t {
            period: 0,
            computation: 0,
            constraint: 0,
            preemptible: 0,
        };
        let mut count: mach_msg_type_number_t;


        rt_priority_handle.tid = tid;

        // false: we want to get the current value, not the default value. If this is `false` after
        // returning, it means there are no current settings because of other factor, and the
        // default was returned instead.
        let mut get_default: boolean_t = 0;
        count = THREAD_EXTENDED_POLICY_COUNT!();
        rv = thread_policy_get(tid,
                               THREAD_EXTENDED_POLICY,
                               (&mut policy) as *mut _ as thread_policy_t,
                               &mut count,
                               &mut get_default);

        if rv != KERN_SUCCESS as i32 {
            error!("thread promotion error: thread_policy_get: extended");
            return Err(());
        }

        rt_priority_handle.previous_time_share = policy;

        get_default = 0;
        count = THREAD_PRECEDENCE_POLICY_COUNT!();
        rv = thread_policy_get(tid,
                               THREAD_PRECEDENCE_POLICY,
                               (&mut precedence) as *mut _ as thread_policy_t,
                               &mut count,
                               &mut get_default);

        if rv != KERN_SUCCESS as i32 {
            error!("thread promotion error: thread_policy_get: precedence");
            return Err(());
        }

        rt_priority_handle.previous_precedence_policy = precedence;

        get_default = 0;
        count = THREAD_TIME_CONSTRAINT_POLICY_COUNT!();
        rv = thread_policy_get(tid,
                               THREAD_TIME_CONSTRAINT_POLICY,
                               (&mut time_constraints) as *mut _ as thread_policy_t,
                               &mut count,
                               &mut get_default);

        if rv != KERN_SUCCESS as i32 {
            error!("thread promotion error: thread_policy_get: time_constraint");
            return Err(());
        }

        rt_priority_handle.previous_time_constraint_policy = time_constraints;

        // Now, that we have all the previous values to be able to restore,
        // set the thread to real-time.
        rv = thread_policy_set(tid,
                               THREAD_EXTENDED_POLICY,
                               (&mut policy) as *mut _ as thread_policy_t,
                               THREAD_EXTENDED_POLICY_COUNT!());

        if rv != KERN_SUCCESS as i32 {
            error!("thread promotion error: thread_policy_set: extended");
            return Err(());
        }

        let mut precedence = thread_precedence_policy_data_t { importance: 63 };
        rv = thread_policy_set(tid,
                               THREAD_PRECEDENCE_POLICY,
                               (&mut precedence) as *mut _ as thread_policy_t,
                               THREAD_PRECEDENCE_POLICY_COUNT!());

        if rv != KERN_SUCCESS as i32 {
            error!("thread promotion error: thread_policy_set: precedence");
            return Err(());
        }

        let cb_duration = audio_buffer_frames as f32 / (audio_samplerate_hz as f32) * 1000.;
        let computation = 0.6 * cb_duration;
        let constraint = 0.85 * cb_duration;

        let mut timebase_info = mach_timebase_info_data_t { denom: 0, numer: 0 };
        mach_timebase_info(&mut timebase_info);

        let ms2abs: f32 = ((timebase_info.denom as f32) / timebase_info.numer as f32) * 1000000.;

        time_constraints = thread_time_constraint_policy_data_t {
            period: (cb_duration * ms2abs) as u32,
            computation: (computation * ms2abs) as u32,
            constraint: (constraint * ms2abs) as u32,
            preemptible: 0,
        };

        rv = thread_policy_set(tid,
                               THREAD_TIME_CONSTRAINT_POLICY,
                               (&mut time_constraints) as *mut _ as thread_policy_t,
                               THREAD_TIME_CONSTRAINT_POLICY_COUNT!());
        if rv != KERN_SUCCESS as i32 {
            error!("thread promotion error: thread_policy_set: time_constraint");
            return Err(());
        }

        info!("thread {} bumped to real time priority.", tid);
    }

    Ok(rt_priority_handle)
}
