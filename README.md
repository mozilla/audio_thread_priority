# audio_thread_priority

Synopsis:

```rust
  // ... on a thread that will compute audio and has to be real-time:
  match promote_current_thread_to_real_time(512, 44100) {
    Ok(...) => { println!("this thread is now bumped to real-time priority.") }
    Err(...) => { println!("could not bump to real time.") }
  }
```

# License

MPL-2

