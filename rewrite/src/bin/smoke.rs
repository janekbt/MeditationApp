//! Manual smoke test: drives a real 5-second countdown, exercises pause/resume.
//! Demonstrates that the core works end-to-end with a real shell-side clock.
//! Run with: `cargo run --bin smoke`

use meditate_core::timer::{Countdown, CountdownTimer, Stopwatch};
use std::thread::sleep;
use std::time::{Duration, Instant};

fn main() {
    let shell_origin = Instant::now();
    let now = || shell_origin.elapsed();

    let countdown = Countdown::new(
        CountdownTimer::new(Duration::from_secs(5)),
        Stopwatch::started_at(now()),
    );
    println!("Starting 5-second countdown...");

    let phase1_end = now() + Duration::from_secs(2);
    while now() < phase1_end {
        print_remaining(&countdown, now());
        sleep(Duration::from_millis(500));
    }

    println!("[pause]");
    let countdown = countdown.pause(now());
    for _ in 0..3 {
        sleep(Duration::from_millis(500));
        print_remaining(&countdown, now());
    }

    println!("[resume]");
    let countdown = countdown.resume(now());
    while !countdown.is_finished(now()) {
        print_remaining(&countdown, now());
        sleep(Duration::from_millis(500));
    }

    println!("Done.");
}

fn print_remaining(c: &Countdown, t: Duration) {
    let r = c.remaining(t);
    println!("  remaining: {}.{:03}s", r.as_secs(), r.subsec_millis());
}
