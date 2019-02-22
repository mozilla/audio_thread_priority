#[macro_use]
extern crate cfg_if;
#[cfg(feature = "terminal-logging")]
extern crate simple_logger;

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
        mod rt_linux;
        extern crate dbus;
        extern crate libc;
        pub use rt_linux::promote_current_thread_to_real_time;
        pub use rt_linux::demote_current_thread_from_real_time;
        pub use rt_linux::RtPriorityHandle;
    }
}

#[cfg(test)]
mod tests {
    use promote_current_thread_to_real_time;
    use demote_current_thread_from_real_time;
    use RtPriorityHandle;
    #[cfg(feature = "terminal-logging")]
    use simple_logger;

    #[test]
    fn it_works() {
        #[cfg(feature = "terminal-logging")]
        simple_logger::init().unwrap();
        let rt_prio_handle = RtPriorityHandle::new();
        let rt_prio_handle = promote_current_thread_to_real_time(512, 44100).unwrap();
        demote_current_thread_from_real_time(rt_prio_handle).unwrap();
    }
}
