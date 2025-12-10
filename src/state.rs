use crate::macro_play;
use crate::schema;
use crate::schema::TimedEvent;
use enigo;
use rdev::{Event, EventType, Key};
use std::sync::{
  atomic::{AtomicBool, Ordering},
  Arc, Mutex,
};
use std::collections::HashMap;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct Sample {
  pub name: String,
  pub events: Vec<TimedEvent>,
}

#[derive(Clone)]
pub struct AppState {
  pub recording: Arc<AtomicBool>,
  pub start: Arc<Mutex<Option<Instant>>>,
  pub current_events: Arc<Mutex<Vec<TimedEvent>>>,
  pub samples: Arc<Mutex<Vec<Sample>>>,
  pub playback_stop: Arc<AtomicBool>,
  pub playback_handle: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
  pub playback_offset_ms: Arc<Mutex<i64>>,
  pub playing: Arc<AtomicBool>,
  pub tracked_keys: Arc<Mutex<Vec<enigo::Key>>>,
  pub key_states: Arc<Mutex<HashMap<enigo::Key, bool>>>,
}

impl AppState {
  pub fn new() -> Self {
    Self {
      recording: Arc::new(AtomicBool::new(false)),
      start: Arc::new(Mutex::new(None)),
      current_events: Arc::new(Mutex::new(Vec::new())),
      samples: Arc::new(Mutex::new(Vec::new())),
      playback_stop: Arc::new(AtomicBool::new(false)),
      playback_handle: Arc::new(Mutex::new(None)),
      playback_offset_ms: Arc::new(Mutex::new(0)),
      playing: Arc::new(AtomicBool::new(false)),
      tracked_keys: Arc::new(Mutex::new(Vec::new())),
      key_states: Arc::new(Mutex::new(HashMap::new())),
    }
  }

  pub fn spawn_global_listener(&self) {
    let state = self.clone();
    thread::spawn(move || {
      if let Err(error) = rdev::listen(move |event| handle_event(&state, event)) {
        eprintln!("Listener error: {:?}", error);
      }
    });
  }

  pub fn start_recording(&self) {
    self.current_events.lock().unwrap().clear();
    *self.start.lock().unwrap() = Some(Instant::now());
    self.recording.store(true, Ordering::SeqCst);
  }

  pub fn stop_recording(&self) {
    let was_recording = self.recording.swap(false, Ordering::SeqCst);
    if !was_recording {
      return;
    }
    let snapshot = self.current_events.lock().unwrap().clone();
    if !snapshot.is_empty() {
      let mut samples = self.samples.lock().unwrap();
      let name = format!("Sample {}", samples.len() + 1);
      samples.push(Sample { name, events: snapshot });
    }
  }

  pub fn playback_latest(&self) {
    let samples = self.samples.lock().unwrap();
    if let Some(last) = samples.last() {
      self.playback_sample(&last.events);
    } else {
      println!("No samples to play.");
    }
  }

  pub fn playback_sample(&self, sample: &[TimedEvent]) {
    log_recorded_events(&sample);
    if sample.is_empty() {
      println!("No events recorded; nothing to play back.");
      return;
    }
    self.stop_playback(); // stop any ongoing playback before starting new
    self.playback_stop.store(false, Ordering::SeqCst);
    self.playing.store(true, Ordering::SeqCst);
    let offset_ms = *self.playback_offset_ms.lock().unwrap();
    let max_at = sample
      .iter()
      .map(|e| apply_offset(e.at, offset_ms))
      .max()
      .unwrap_or(Duration::from_millis(0));
    let stop_flag = self.playback_stop.clone();
    let handle = macro_play::play_timeline_async(sample.to_vec(), stop_flag, offset_ms);
    *self.playback_handle.lock().unwrap() = Some(handle);
    // Schedule a watcher thread to auto-clear the handle after expected duration.
    let handle_ref = self.playback_handle.clone();
    let playing_flag = self.playing.clone();
    thread::spawn(move || {
      thread::sleep(max_at + Duration::from_millis(300));
      if let Some(joined) = handle_ref.lock().unwrap().take() {
        let _ = joined.join();
      }
      playing_flag.store(false, Ordering::SeqCst);
    });
  }

  pub fn merge_samples(&self) {
    let samples = self.samples.lock().unwrap();
    if samples.is_empty() {
      println!("No samples to merge.");
      return;
    }
    let mut merged: Vec<TimedEvent> = samples.iter().flat_map(|s| s.events.clone()).collect();
    merged.sort_by_key(|e| e.at);
    drop(samples);

    if merged.is_empty() {
      println!("Merged result is empty.");
      return;
    }
    let mut samples = self.samples.lock().unwrap();
    let name = format!("Merged {}", samples.len() + 1);
    println!("Merged samples into one timeline with {} events.", merged.len());
    samples.push(Sample { name, events: merged });
  }

  pub fn stop_playback(&self) {
    self.playback_stop.store(true, Ordering::SeqCst);
    if let Some(handle) = self.playback_handle.lock().unwrap().take() {
      let _ = handle.join();
    }
    self.playing.store(false, Ordering::SeqCst);
  }

  pub fn tracked_keys(&self) -> Vec<enigo::Key> {
    self.tracked_keys.lock().unwrap().clone()
  }

  pub fn add_tracked_keys_from_text(&self, text: &str) {
    let mut keys = self.tracked_keys.lock().unwrap();
    for ch in text.chars().filter(|c| !c.is_whitespace()) {
      let key = enigo::Key::Layout(ch);
      if !keys.contains(&key) {
        keys.push(key);
      }
    }
  }

  pub fn remove_tracked_key(&self, idx: usize) {
    let mut keys = self.tracked_keys.lock().unwrap();
    if idx < keys.len() {
      keys.remove(idx);
    }
  }

  pub fn tracked_key_states(&self) -> Vec<(enigo::Key, bool)> {
    let keys = self.tracked_keys.lock().unwrap();
    let states = self.key_states.lock().unwrap();
    keys
      .iter()
      .map(|k| (*k, *states.get(k).unwrap_or(&false)))
      .collect()
  }
}

pub fn handle_event(state: &AppState, event: Event) {
  // Hotkeys: F9 toggle record, F10 toggle playback.
  if let EventType::KeyPress(key) = event.event_type {
    match key {
      Key::F9 => {
        if state.recording.load(Ordering::SeqCst) {
          state.stop_recording();
          println!("Recording stopped via F9");
        } else {
          state.start_recording();
          println!("Recording started via F9");
        }
        return;
      }
      Key::F10 => {
        if state.playing.load(Ordering::SeqCst) {
          state.stop_playback();
          println!("Playback stopped via F10");
        } else {
          state.stop_recording();
          state.playback_latest();
          println!("Playback started via F10");
        }
        return;
      }
      _ => {}
    }
  }

  if !state.recording.load(Ordering::SeqCst) {
    return;
  }

  let Some(start_at) = *state.start.lock().unwrap() else {
    return;
  };

  match event.event_type {
    EventType::KeyPress(key) => {
      if let Some(mapped) = convert_key(key) {
        // Always update key state for overlay
        state.key_states.lock().unwrap().insert(mapped, true);
        // Record only when recording is active
        if state.recording.load(Ordering::SeqCst) {
          push_event(schema::KeyAction::Down(mapped), start_at, &state.current_events);
        }
      } else {
        println!("record: unmapped keypress {:?}", key);
      }
    }
    EventType::KeyRelease(key) => {
      if let Some(mapped) = convert_key(key) {
        state.key_states.lock().unwrap().insert(mapped, false);
        if state.recording.load(Ordering::SeqCst) {
          push_event(schema::KeyAction::Up(mapped), start_at, &state.current_events);
        }
      } else {
        println!("record: unmapped keyrelease {:?}", key);
      }
    }
    _ => {}
  }
}

fn push_event(action: schema::KeyAction, start: Instant, sink: &Arc<Mutex<Vec<TimedEvent>>>) {
  let elapsed = Instant::now().duration_since(start);
  sink.lock()
      .unwrap()
      .push(TimedEvent { at: elapsed, action });
}

fn log_recorded_events(events: &[TimedEvent]) {
  println!("Recorded {} events:", events.len());
  for (i, ev) in events.iter().enumerate() {
    println!("  #{:<3} at {:>6} ms => {:?}", i, ev.at.as_millis(), ev.action);
  }
}

fn apply_offset(at: Duration, offset_ms: i64) -> Duration {
  if offset_ms >= 0 {
    at + Duration::from_millis(offset_ms as u64)
  } else {
    at.saturating_sub(Duration::from_millis(offset_ms.unsigned_abs()))
  }
}

fn convert_key(key: Key) -> Option<enigo::Key> {
  // rdev Key -> enigo Key mapping. Return None if unknown to avoid sending spaces.
  let mapped = match key {
    Key::KeyA => enigo::Key::Layout('a'),
    Key::KeyB => enigo::Key::Layout('b'),
    Key::KeyC => enigo::Key::Layout('c'),
    Key::KeyD => enigo::Key::Layout('d'),
    Key::KeyE => enigo::Key::Layout('e'),
    Key::KeyF => enigo::Key::Layout('f'),
    Key::KeyG => enigo::Key::Layout('g'),
    Key::KeyH => enigo::Key::Layout('h'),
    Key::KeyI => enigo::Key::Layout('i'),
    Key::KeyJ => enigo::Key::Layout('j'),
    Key::KeyK => enigo::Key::Layout('k'),
    Key::KeyL => enigo::Key::Layout('l'),
    Key::KeyM => enigo::Key::Layout('m'),
    Key::KeyN => enigo::Key::Layout('n'),
    Key::KeyO => enigo::Key::Layout('o'),
    Key::KeyP => enigo::Key::Layout('p'),
    Key::KeyQ => enigo::Key::Layout('q'),
    Key::KeyR => enigo::Key::Layout('r'),
    Key::KeyS => enigo::Key::Layout('s'),
    Key::KeyT => enigo::Key::Layout('t'),
    Key::KeyU => enigo::Key::Layout('u'),
    Key::KeyV => enigo::Key::Layout('v'),
    Key::KeyW => enigo::Key::Layout('w'),
    Key::KeyX => enigo::Key::Layout('x'),
    Key::KeyY => enigo::Key::Layout('y'),
    Key::KeyZ => enigo::Key::Layout('z'),
    Key::Num0 => enigo::Key::Layout('0'),
    Key::Num1 => enigo::Key::Layout('1'),
    Key::Num2 => enigo::Key::Layout('2'),
    Key::Num3 => enigo::Key::Layout('3'),
    Key::Num4 => enigo::Key::Layout('4'),
    Key::Num5 => enigo::Key::Layout('5'),
    Key::Num6 => enigo::Key::Layout('6'),
    Key::Num7 => enigo::Key::Layout('7'),
    Key::Num8 => enigo::Key::Layout('8'),
    Key::Num9 => enigo::Key::Layout('9'),
    Key::Space => enigo::Key::Space,
    Key::Return => enigo::Key::Return,
    Key::Backspace => enigo::Key::Backspace,
    Key::Tab => enigo::Key::Tab,
    Key::Escape => enigo::Key::Escape,
    Key::UpArrow => enigo::Key::UpArrow,
    Key::DownArrow => enigo::Key::DownArrow,
    Key::LeftArrow => enigo::Key::LeftArrow,
    Key::RightArrow => enigo::Key::RightArrow,
    Key::ShiftLeft | Key::ShiftRight => enigo::Key::Shift,
    Key::ControlLeft | Key::ControlRight => enigo::Key::Control,
    Key::Alt | Key::AltGr => enigo::Key::Alt,
    _ => return None,
  };
  Some(mapped)
}

pub fn key_label(key: &enigo::Key) -> String {
  match key {
    enigo::Key::Layout(c) => format!("{}", c),
    enigo::Key::Space => "Space".into(),
    enigo::Key::Return => "Enter".into(),
    enigo::Key::Backspace => "Backspace".into(),
    enigo::Key::Tab => "Tab".into(),
    enigo::Key::Escape => "Esc".into(),
    enigo::Key::UpArrow => "Up".into(),
    enigo::Key::DownArrow => "Down".into(),
    enigo::Key::LeftArrow => "Left".into(),
    enigo::Key::RightArrow => "Right".into(),
    enigo::Key::Shift => "Shift".into(),
    enigo::Key::Control => "Ctrl".into(),
    enigo::Key::Alt => "Alt".into(),
    other => format!("{:?}", other),
  }
}
