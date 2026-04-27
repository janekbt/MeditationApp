//! Manual smoke test: drives a real box breath cycle, prints phase + progress.
//! Run with: `cargo run --bin breath_smoke`

use meditate_core::breath::{BreathPattern, BreathSession};
use meditate_core::timer::Stopwatch;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn main() {
    let shell_origin = Instant::now();
    let now = || shell_origin.elapsed();

    let session = BreathSession::new(BreathPattern::box_breath(), Stopwatch::started_at(now()));

    println!("Box breath: 4s inhale / 4s hold / 4s exhale / 4s hold");
    println!();

    while now() < Duration::from_secs(17) {
        let phase = session.current_phase(now());
        let progress = session.current_progress(now());
        println!(
            "{:18?}  {:5.1}%  {}",
            phase,
            progress * 100.0,
            render_bar(progress, 20)
        );
        sleep(Duration::from_millis(500));
    }
}

fn render_bar(progress: f64, width: usize) -> String {
    let filled = (progress * width as f64) as usize;
    let mut s = String::with_capacity(width + 2);
    s.push('[');
    for i in 0..width {
        s.push(if i < filled { '#' } else { ' ' });
    }
    s.push(']');
    s
}
