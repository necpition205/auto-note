use crate::schema::{KeyAction, TimedEvent};
use enigo::{Enigo, KeyboardControllable};
use std::sync::{
  atomic::{AtomicBool, Ordering},
  Arc,
};
use std::thread;
use std::time::{Duration, Instant};

/// Play recorded key timeline asynchronously.
pub fn play_timeline_async(
  events: Vec<TimedEvent>,
  stop: Arc<AtomicBool>,
  offset_ms: i64,
) -> thread::JoinHandle<()> {
  thread::spawn(move || {
    let start = Instant::now();
    let mut enigo = Enigo::new();

    for ev in events {
      if stop.load(Ordering::SeqCst) {
        println!("playback stopped");
        break;
      }
      let scheduled = apply_offset(ev.at, offset_ms);
      wait_until(start, scheduled, &stop);

      match ev.action {
        KeyAction::Down(k) => {
          println!(
            "play: {:?} DOWN at {} ms (offset {} ms)",
            k,
            scheduled.as_millis(),
            offset_ms
          );
          enigo.key_down(k);
        }
        KeyAction::Up(k) => {
          println!(
            "play: {:?} UP at {} ms (offset {} ms)",
            k,
            scheduled.as_millis(),
            offset_ms
          );
          enigo.key_up(k);
        }
      }
    }
  })
}

fn apply_offset(at: Duration, offset_ms: i64) -> Duration {
  if offset_ms >= 0 {
    at + Duration::from_millis(offset_ms as u64)
  } else {
    at.saturating_sub(Duration::from_millis(offset_ms.unsigned_abs()))
  }
}

/// Hybrid sleep+spin to hit the scheduled time more tightly.
fn wait_until(start: Instant, scheduled: Duration, stop: &Arc<AtomicBool>) {
  loop {
    let elapsed = Instant::now().duration_since(start);
    if stop.load(Ordering::SeqCst) {
      break;
    }
    if elapsed >= scheduled {
      break;
    }
    let remaining = scheduled - elapsed;
    // Sleep for coarse remaining minus a small guard, then spin for the rest.
    if remaining > Duration::from_micros(500) {
      let sleep_dur = remaining - Duration::from_micros(200);
      thread::sleep(sleep_dur);
    } else {
      // Spin for sub-500us windows to reduce jitter.
      std::hint::spin_loop();
    }
  }
}
