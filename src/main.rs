use eframe::egui;
use rdev::{listen, Event, EventType, Key};
use std::sync::{
  atomic::{AtomicBool, Ordering},
  Arc, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

mod macro_play;
mod schema;

fn main() -> eframe::Result<()> {
  let state = AppState::new();
  state.spawn_global_listener();

  let options = eframe::NativeOptions {
    viewport: egui::ViewportBuilder::default()
      .with_inner_size([360.0, 240.0])
      .with_always_on_top(),
    ..Default::default()
  };

  eframe::run_native(
    "Auto Note Recorder",
    options,
    Box::new(move |_cc| Box::new(RecorderApp { state: state.clone() })),
  )
}

#[derive(Clone)]
struct AppState {
  recording: Arc<AtomicBool>,
  start: Arc<Mutex<Option<Instant>>>,
  current_events: Arc<Mutex<Vec<schema::TimedEvent>>>,
  samples: Arc<Mutex<Vec<Vec<schema::TimedEvent>>>>,
  playback_stop: Arc<AtomicBool>,
  playback_handle: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
  playback_offset_ms: Arc<Mutex<i64>>,
  playing: Arc<AtomicBool>,
}

impl AppState {
  fn new() -> Self {
    Self {
      recording: Arc::new(AtomicBool::new(false)),
      start: Arc::new(Mutex::new(None)),
      current_events: Arc::new(Mutex::new(Vec::new())),
      samples: Arc::new(Mutex::new(Vec::new())),
      playback_stop: Arc::new(AtomicBool::new(false)),
      playback_handle: Arc::new(Mutex::new(None)),
      playback_offset_ms: Arc::new(Mutex::new(0)),
      playing: Arc::new(AtomicBool::new(false)),
    }
  }

  fn spawn_global_listener(&self) {
    let state = self.clone();
    thread::spawn(move || {
      if let Err(error) = listen(move |event| handle_event(&state, event)) {
        eprintln!("Listener error: {:?}", error);
      }
    });
  }

  fn start_recording(&self) {
    self.current_events.lock().unwrap().clear();
    *self.start.lock().unwrap() = Some(Instant::now());
    self.recording.store(true, Ordering::SeqCst);
  }

  fn stop_recording(&self) {
    let was_recording = self.recording.swap(false, Ordering::SeqCst);
    if !was_recording {
      return;
    }
    let snapshot = self.current_events.lock().unwrap().clone();
    if !snapshot.is_empty() {
      self.samples.lock().unwrap().push(snapshot);
    }
  }

  fn playback_latest(&self) {
    let samples = self.samples.lock().unwrap();
    if let Some(last) = samples.last() {
      self.playback_sample(last.clone());
    } else {
      println!("No samples to play.");
    }
  }

  fn playback_sample(&self, sample: Vec<schema::TimedEvent>) {
    log_recorded_events(&sample);
    if sample.is_empty() {
      println!("No events recorded; nothing to play back.");
      return;
    }
    println!("Focus the target window within 500ms...");
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
    let handle = macro_play::play_timeline_async(sample, stop_flag, offset_ms);
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

  fn delete_sample(&self, idx: usize) {
    let mut samples = self.samples.lock().unwrap();
    if idx < samples.len() {
      samples.remove(idx);
    }
  }

  fn stop_playback(&self) {
    self.playback_stop.store(true, Ordering::SeqCst);
    if let Some(handle) = self.playback_handle.lock().unwrap().take() {
      let _ = handle.join();
    }
    self.playing.store(false, Ordering::SeqCst);
  }
}

struct RecorderApp {
  state: AppState,
}

impl eframe::App for RecorderApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    // Keep UI refreshing so counters update even without mouse movement.
    ctx.request_repaint_after(Duration::from_millis(16));
    egui::CentralPanel::default().show(ctx, |ui| {
      ui.heading("Auto Note Recorder");
      ui.separator();

      let is_rec = self.state.recording.load(Ordering::SeqCst);
      ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(!is_rec, egui::Button::new("Start Recording")).clicked() {
          self.state.start_recording();
        }
        if ui.add_enabled(is_rec, egui::Button::new("Stop Recording")).clicked() {
          self.state.stop_recording();
        }
        if ui.button("Playback Latest").clicked() {
          self.state.stop_recording();
          self.state.playback_latest();
        }
        if ui.button("Stop Playback").clicked() {
          self.state.stop_playback();
        }
      });

      ui.separator();
      ui.horizontal_wrapped(|ui| {
        ui.label("Playback offset (ms):");
        let mut offset_ms = *self.state.playback_offset_ms.lock().unwrap();
        if ui.add(egui::DragValue::new(&mut offset_ms).speed(1)).changed() {
          *self.state.playback_offset_ms.lock().unwrap() = offset_ms;
        }
      });
      ui.separator();
      let ev_len = self.state.current_events.lock().unwrap().len();
      ui.label(format!("Recording: {}", if is_rec { "ON" } else { "OFF" }));
      let is_playing = self.state.playing.load(Ordering::SeqCst);
      ui.label(format!("Playing: {}", if is_playing { "ON" } else { "OFF" }));
      ui.label(format!("Events captured (current): {}", ev_len));

      ui.separator();
      ui.heading("Recorded Samples");
      let mut to_delete: Option<usize> = None;
      let samples = self.state.samples.lock().unwrap().clone();
      for (idx, sample) in samples.iter().enumerate() {
        ui.horizontal(|ui| {
          ui.label(format!("#{}: {} events", idx + 1, sample.len()));
          if ui.button("Play").clicked() {
            self.state.playback_sample(sample.clone());
          }
          if ui.button("Delete").clicked() {
            to_delete = Some(idx);
          }
        });
      }
      drop(samples);
      if let Some(idx) = to_delete {
        self.state.delete_sample(idx);
      }
    });
  }
}

fn handle_event(state: &AppState, event: Event) {
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
        push_event(schema::KeyAction::Down(mapped), start_at, &state.current_events);
      } else {
        println!("record: unmapped keypress {:?}", key);
      }
    }
    EventType::KeyRelease(key) => {
      if let Some(mapped) = convert_key(key) {
        push_event(schema::KeyAction::Up(mapped), start_at, &state.current_events);
      } else {
        println!("record: unmapped keyrelease {:?}", key);
      }
    }
    _ => {}
  }
}

fn push_event(action: schema::KeyAction, start: Instant, sink: &Arc<Mutex<Vec<schema::TimedEvent>>>) {
  let elapsed = Instant::now().duration_since(start);
  sink.lock()
      .unwrap()
      .push(schema::TimedEvent { at: elapsed, action });
}

fn log_recorded_events(events: &[schema::TimedEvent]) {
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
