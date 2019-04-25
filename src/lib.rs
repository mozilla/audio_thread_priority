#[macro_use]
extern crate cfg_if;
#[cfg(feature = "terminal-logging")]
extern crate simple_logger;
#[macro_use]
extern crate log;

cfg_if! {
    if #[cfg(target_os = "macos")] {
        mod rt_mach;
#[allow(unused, non_camel_case_types, non_snake_case, non_upper_case_globals)]
        mod mach_sys;
        extern crate mach;
        extern crate libc;
        pub use rt_mach::promote_current_thread_to_real_time;
        pub use rt_mach::demote_current_thread_from_real_time;
        pub use rt_mach::RtPriorityHandle;
    } else if #[cfg(target_os = "windows")] {
        extern crate winapi;
        extern crate kernel32;
        mod rt_win;
        pub use rt_win::promote_current_thread_to_real_time;
        pub use rt_win::demote_current_thread_from_real_time;
        pub use rt_win::RtPriorityHandle;
    } else if #[cfg(target_os = "linux")] {
        pub mod rt_linux;
        extern crate dbus;
        extern crate libc;
        pub use rt_linux::promote_current_thread_to_real_time;
        pub use rt_linux::demote_current_thread_from_real_time;
        pub use rt_linux::RtPriorityHandle;
    }
}

#[allow(non_camel_case_types)]
pub struct atp_handle(RtPriorityHandle);

#[no_mangle]
pub extern "C" fn atp_promote_current_thread_to_real_time(audio_buffer_frames: u32,
                                           audio_samplerate_hz: u32) -> *mut atp_handle{
    match promote_current_thread_to_real_time(audio_buffer_frames, audio_samplerate_hz) {
        Ok(handle) => {
            Box::into_raw(Box::new(atp_handle(handle)))
        },
        _ => {
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn atp_demote_current_thread_from_real_time(handle: *mut atp_handle) -> i32 {
    assert!(!handle.is_null());
    let handle = unsafe { Box::from_raw(handle) };

    match demote_current_thread_from_real_time(handle.0) {
        Ok(_) => {
            0
        }
        _ => {
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use demote_current_thread_from_real_time;
    use promote_current_thread_to_real_time;
    #[cfg(feature = "terminal-logging")]
    use simple_logger;

    #[test]
    fn it_works() {
        #[cfg(feature = "terminal-logging")]
        simple_logger::init().unwrap();
        let rt_prio_handle = promote_current_thread_to_real_time(512, 44100).unwrap();
        demote_current_thread_from_real_time(rt_prio_handle).unwrap();
    }
}
