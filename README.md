# audio_thread_priority

[![](https://img.shields.io/crates/v/audio_thread_priority.svg)](https://crates.io/crates/audio_thread_priority)
[![](https://docs.rs/audio_thread_priority/badge.svg)](https://docs.rs/audio_thread_priority)


Synopsis:

```rust

use audio_thread_priority::{promote_current_thread_to_real_time, demote_current_thread_from_real_time};

// ... on a thread that will compute audio and has to be real-time:
match promote_current_thread_to_real_time(512, 44100) {
  Ok(h) => {
    println!("this thread is now bumped to real-time priority.");

    // Do some real-time work...

    match demote_current_thread_from_real_time(h) {
      Ok(_) => {
        println!("this thread is now bumped back to normal.")
      }
      Err(_) => {
        println!("Could not bring the thread back to normal priority.")
      }
    };
  }
  Err(e) => {
    eprintln!("Error promoting thread to real-time: {}", e);
  }
}

```

A thread other than the calling one can also be promoted or demoted. This is
useful when the thread that needs to become real-time cannot promote itself
directly, for example because it is sandboxed:

```rust
use audio_thread_priority::{get_current_thread_info, promote_thread_to_real_time, demote_thread_from_real_time};

// ... on the thread that will compute audio and has to be real-time, gather
// serializable information about it:
let thread_info = get_current_thread_info().unwrap();
// ... send `thread_info` to another thread, or another process, via any IPC
// mechanism, then:
let handle = promote_thread_to_real_time(thread_info, 512, 44100).unwrap();
// ... and later, from anywhere:
demote_thread_from_real_time(thread_info).unwrap();
```

Promoting a thread other than the caller's, may require elevated privileges
depending on the platform (for example, Linux uses a privileged rtkit D-Bus
service, and macOS/iOS require `task_for_pid` rights).

This library can also be used from C or C++ using the included header and
compiling the rust code in the application. By default, a `.a` is compiled to
ease linking.

# License

MPL-2
