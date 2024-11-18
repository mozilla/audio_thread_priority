extern crate libc;
use crate::AudioThreadPriorityError;
use jni::objects::JClass;
use jni::sys::jint;
use jni::JNIEnv;
use log::info;

#[derive(Debug)]
pub struct RtPriorityHandleInternal {
    previous_priority: libc::c_int,
}

pub fn promote_current_thread_to_real_time_with_jvm(
    jvm: &jni::JavaVM,
) -> Result<RtPriorityHandleInternal, AudioThreadPriorityError> {
    let mut env = match jvm.attach_current_thread() {
        Ok(env) => env,
        Err(e) => {
            return Err(AudioThreadPriorityError {
                message: "Couldn't attach to JVM".into(),
                inner: Some(Box::new(e)),
            });
        }
    };
    let class = env
        .find_class("java/sdk/Process")
        .expect("Failed to load the target class");
    let rv = env.call_static_method(&class, "getThreadPriority", "()I", &[]);
    let previous_priority: jint = match rv {
        Ok(p) => p.i().unwrap(),
        Err(_) => {
            return Err(AudioThreadPriorityError {
                message: "Couldn't get previous thread priority".into(),
                inner: None,
            });
        }
    };

    // From the SDK
    let THREAD_PRIORITY_URGENT_AUDIO = -19 as jint;
    match env.call_static_method(
        class,
        "setThreadPriority",
        "(I)V",
        &[THREAD_PRIORITY_URGENT_AUDIO.into()],
    ) {
        Ok(v) => {
            return Ok(RtPriorityHandleInternal {
                previous_priority: previous_priority,
            })
        }
        Err(_) => {
            return Err(AudioThreadPriorityError {
                message: "Couldn't get previous thread priority".into(),
                inner: None,
            })
        }
    }
}

pub fn demote_current_thread_from_real_time_internal(
    rt_priority_handle: RtPriorityHandleInternal,
) -> Result<(), AudioThreadPriorityError> {
    Err(AudioThreadPriorityError {
        message: "Not implemented".into(),
        inner: None,
    })
}
