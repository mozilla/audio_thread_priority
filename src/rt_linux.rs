/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this file,
 * You can obtain one at http://mozilla.org/MPL/2.0/. */

/* Widely copied from dbus-rs/dbus/examples/rtkit.rs */

extern crate dbus;
extern crate libc;

use std::cmp;
use std::convert::TryInto;

use dbus::{Connection, BusType, Props, MessageItem, Message};

const RT_PRIO_DEFAULT: u32 = 10;
// This is different from libc::pid_t, which is 32 bits, and is defined in sys/types.h.
#[allow(non_camel_case_types)]
type kernel_pid_t = libc::c_long;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct RtPriorityThreadInfoInternal {
    /// System-wise thread id, use to promote the thread via dbus.
    thread_id: kernel_pid_t,
    /// Process-local thread id, used to restore scheduler characteristics. This information is not
    /// useful in another process, but is useful tied to the `thread_id`, when back into the first
    /// process.
    pthread_id: libc::pthread_t,
    /// ...
    policy: libc::c_int,
    /// ...
    param: libc::sched_param,
}

impl RtPriorityThreadInfoInternal {
    /// Serialize a RtPriorityThreadInfoInternal to a byte buffer.
    pub fn serialize(&self) -> [u8; std::mem::size_of::<Self>()] {
        let mut bytes = [0; std::mem::size_of::<Self>()];

        bytes[..8].copy_from_slice(&self.thread_id.to_ne_bytes());
        bytes[8..16].copy_from_slice(&self.pthread_id.to_ne_bytes());
        bytes[16..20].copy_from_slice(&self.policy.to_ne_bytes());
        bytes[20..].copy_from_slice(&self.param.sched_priority.to_ne_bytes());

        bytes
    }
    /// Get an RtPriorityThreadInfoInternal from a byte buffer.
    pub fn deserialize(bytes: [u8; std::mem::size_of::<Self>()]) -> Self {
        Self {
            thread_id: kernel_pid_t::from_ne_bytes(bytes[..8].try_into().unwrap()),
            pthread_id: libc::pthread_t::from_ne_bytes(bytes[8..16].try_into().unwrap()),
            policy: libc::c_int::from_ne_bytes(bytes[16..20].try_into().unwrap()),
            param: libc::sched_param { sched_priority: libc::c_int::from_ne_bytes(bytes[20..].try_into().unwrap()) },
        }
    }
}

impl PartialEq for RtPriorityThreadInfoInternal {
    fn eq(&self, other: &Self) -> bool {
        self.thread_id == other.thread_id &&
            self.pthread_id == other.pthread_id
    }
}

/*#[derive(Debug)]*/
pub struct RtPriorityHandleInternal {
    thread_info: RtPriorityThreadInfoInternal,
}

fn item_as_i64(i: MessageItem) -> Result<i64, Box<std::error::Error>> {
    match i {
        MessageItem::Int32(i) => Ok(i as i64),
        MessageItem::Int64(i) => Ok(i),
        _ => Err(Box::from(&*format!("Property is not integer ({:?})", i)))
    }
}

fn rtkit_set_realtime(c: &Connection, thread: u64, prio: u32) -> Result<(), ::dbus::Error> {
    let mut m = Message::new_method_call("org.freedesktop.RealtimeKit1",
                                         "/org/freedesktop/RealtimeKit1",
                                         "org.freedesktop.RealtimeKit1",
                                         "MakeThreadRealtime").unwrap();
    m.append_items(&[thread.into(), prio.into()]);
    let mut r = try!(c.send_with_reply_and_block(m, 10000));
    r.as_result().map(|_| ())
}

fn make_realtime(tid: kernel_pid_t, max_slice_us: u64, prio: u32) -> Result<u32, Box<std::error::Error>> {
    let c = try!(Connection::get_private(BusType::System));

    let p = Props::new(&c, "org.freedesktop.RealtimeKit1", "/org/freedesktop/RealtimeKit1",
        "org.freedesktop.RealtimeKit1", 10000);

    // Make sure we don't fail by wanting too much
    let max_prio = try!(item_as_i64(try!(p.get("MaxRealtimePriority")))) as u32;
    let prio = cmp::min(prio, max_prio);

    // Enforce RLIMIT_RTPRIO, also a must before asking rtkit for rtprio
    let max_rttime = try!(item_as_i64(try!(p.get("RTTimeUSecMax")))) as u64;

    // Only take what we need, or cap at the system limit, no further.
    let rttime_request = cmp::min(max_slice_us, max_rttime) as u64;

    let new_limit = libc::rlimit64 { rlim_cur: rttime_request,
                                     rlim_max: rttime_request };
    let mut old_limit = new_limit;
    if unsafe { libc::getrlimit64(libc::RLIMIT_RTTIME, &mut old_limit) } < 0 {
        return Err(Box::from("getrlimit failed"));
    }
    if unsafe { libc::setrlimit64(libc::RLIMIT_RTTIME, &new_limit) } < 0 {
        return Err(Box::from("setrlimit failed"));
    }

    // Finally, let's ask rtkit to make us realtime
    let r = rtkit_set_realtime(&c, tid as u64, prio);

    if r.is_err() {
        unsafe { libc::setrlimit64(libc::RLIMIT_RTTIME, &old_limit) };
    }

    try!(r);
    Ok(prio)
}

pub fn promote_current_thread_to_real_time_internal(audio_buffer_frames: u32,
                                                    audio_samplerate_hz: u32)
                                           -> Result<RtPriorityHandleInternal, ()> {
    let thread_info = get_current_thread_info_internal()?;
    promote_thread_to_real_time_internal(thread_info, audio_buffer_frames, audio_samplerate_hz)
}

pub fn demote_current_thread_from_real_time_internal(rt_priority_handle: RtPriorityHandleInternal)
                                            -> Result<(), ()> {
    assert!(unsafe { libc::pthread_self() } == rt_priority_handle.thread_info.pthread_id);

    if unsafe { libc::pthread_setschedparam(rt_priority_handle.thread_info.pthread_id,
                                            rt_priority_handle.thread_info.policy,
                                            &rt_priority_handle.thread_info.param) } < 0 {
        error!("could not demote thread {}", rt_priority_handle.thread_info.pthread_id);
        return Err(());
    }
    return Ok(());
}

/// This can be called by sandboxed code, it only restores priority to what they were.
pub fn demote_thread_from_real_time_internal(rt_priority_handle: RtPriorityHandleInternal)
                                            -> Result<(), ()> {
    if unsafe { libc::pthread_setschedparam(rt_priority_handle.thread_info.pthread_id,
                                            rt_priority_handle.thread_info.policy,
                                            &rt_priority_handle.thread_info.param) } < 0 {
        error!("could not demote thread {}", rt_priority_handle.thread_info.pthread_id);
        return Err(());
    }
    return Ok(());
}

/// Get the current thread information, as an opaque struct, that can be serialized and sent
/// accross processes. This is enough to capture the current state of the scheduling policy, and
/// an identifier to have another thread promoted to real-time.
pub fn get_current_thread_info_internal() -> Result<RtPriorityThreadInfoInternal, ()> {
    let thread_id = unsafe { libc::syscall(libc::SYS_gettid) };
    let pthread_id = unsafe { libc::pthread_self() };
    let mut policy = 0;
    let mut param = libc::sched_param { sched_priority: 0 };
    if unsafe { libc::pthread_getschedparam(pthread_id, &mut policy, &mut param) } < 0 {
        error!("pthread_getschedparam error {}", pthread_id);
        return Err(());
    }
    Ok(RtPriorityThreadInfoInternal {
        thread_id,
        pthread_id,
        policy,
        param
    })
}

/// Promote a thread (possibly in another process) identified by its tid, to real-time.
pub fn promote_thread_to_real_time_internal(thread_info: RtPriorityThreadInfoInternal,
                                            audio_buffer_frames: u32,
                                            audio_samplerate_hz: u32) -> Result<RtPriorityHandleInternal, ()>
{
    let RtPriorityThreadInfoInternal { thread_id, .. } = thread_info;

    let mut buffer_frames = audio_buffer_frames;
    if buffer_frames == 0 {
        // 50ms slice. This "ought to be enough for anybody".
        buffer_frames = audio_samplerate_hz / 20;
    }
    let budget_us = (buffer_frames * 1_000_000 / audio_samplerate_hz) as u64;
    let handle = RtPriorityHandleInternal { thread_info };
    let r = make_realtime(thread_id, budget_us, RT_PRIO_DEFAULT);
    if r.is_err() {
        error!("Could not make thread real-time.");
        return Err(());
    }
    return Ok(handle);
}
