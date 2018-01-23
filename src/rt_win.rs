use winapi::um::avrt::*;

pub fn promote_current_thread_to_real_time(_audio_buffer_frames: u32,
                                           _audio_samplerate_hz: u32)
                                           -> Result<(), ()> {
    unsafe {
        let mut mmcss_task_index = 0;

        let mmcss_handle = AvSetMmThreadCharacteristicsA("Audio".as_ptr() as _,
                                                         &mut mmcss_task_index);

        if mmcss_handle.is_null() {
            /* This is not fatal, but we might glitch under heavy load. */
            error!("Unable to use mmcss to bump the render thread priority");
            return Err(());
        }
    }

    Ok(())
}
