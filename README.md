# audio_thread_priority

Synopsis:

```rust
  // ... on a thread that will compute audio and has to be real-time:
  RtPriorityHandle handle;

  match promote_current_thread_to_real_time(512, 44100) {
    Ok(h) => {
      handle = h;
      println!("this thread is now bumped to real-time priority.") 
    }
    Err(...) => { println!("could not bump to real time.") }
  }

  // do some real-time work.

  match demote_current_thread_from_real_time(h) {
    Ok(...) => {
      println!("this thread is now bumped back to normal.")
    }
    Err(...) => {
      println!("Could not bring the thread back to normal priority.")
    }
  }

```

# License

MPL-2

