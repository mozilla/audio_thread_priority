use mach::kern_return::kern_return_t;
use mach::port::mach_port_t;
use mach_sys::*;
use libc::{pthread_self, pthread_t};
use std::mem::size_of;

extern "C" {
    fn pthread_mach_thread_np(tid: pthread_t) -> mach_port_t;
    // This is commented out in thread_policy.h !?
    fn thread_policy_set(thread: thread_t,
                         flavor: thread_policy_flavor_t,
                         policy_info: thread_policy_t,
                         count: mach_msg_type_number_t)
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

pub fn promote_current_thread_to_real_time(audio_buffer_frames: u32,
                                           audio_samplerate_hz: u32)
                                           -> Result<(), ()> {
    unsafe {
        let tid: mach_port_t = pthread_mach_thread_np(pthread_self());
        let mut rv: kern_return_t;
        let mut policy = thread_extended_policy_data_t { timeshare: 0 };

        rv = thread_policy_set(tid,
                               THREAD_EXTENDED_POLICY,
                               (&mut policy) as *mut _ as thread_policy_t,
                               THREAD_EXTENDED_POLICY_COUNT!());

        if rv != KERN_SUCCESS as i32 {
            println!("error: thread_policy_set: extended");
            return Err(());
        }

        let mut precedence = thread_precedence_policy_data_t { importance: 63 };
        rv = thread_policy_set(tid,
                               THREAD_PRECEDENCE_POLICY,
                               (&mut precedence) as *mut _ as thread_policy_t,
                               THREAD_PRECEDENCE_POLICY_COUNT!());

        if rv != KERN_SUCCESS as i32 {
            println!("error: thread_policy_set: precedence");
            return Err(());
        }

        let cb_duration = audio_buffer_frames as f32 / (audio_samplerate_hz as f32) * 1000.;
        let computation = 0.6 * cb_duration;
        let constraint = 0.85 * cb_duration;

        let mut timebase_info = mach_timebase_info_data_t { denom: 0, numer: 0 };
        mach_timebase_info(&mut timebase_info);

        let ms2abs: f32 = ((timebase_info.denom as f32) / timebase_info.numer as f32) * 1000000.;

        let mut time_constraints = thread_time_constraint_policy_data_t {
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
            println!("error: thread_policy_set: RT");
            return Err(());
        }
    }
    Ok(())
}
