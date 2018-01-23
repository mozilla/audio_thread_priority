#[macro_use]
extern crate cfg_if;

cfg_if! {
    if #[cfg(macos)] {
        mod rt_mach;
#[allow(unused, non_camel_case_types, non_snake_case, non_upper_case_globals)]
        mod mach_sys;
        extern crate mach;
        extern crate libc;
        pub use rt_mach::promote_current_thread_to_real_time;
    } else if #[cfg(windows)] {
        extern crate winapi;
        mod rt_win;
        pub use rt_win::promote_current_thread_to_real_time;
    }
}

#[cfg(test)]
mod tests {
    use promote_current_thread_to_real_time;
    #[test]
    fn it_works() {
        promote_current_thread_to_real_time(512, 44100).unwrap();
    }
}
