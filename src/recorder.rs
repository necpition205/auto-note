use enigo::{Enigo, Key, KeyboardControllable};
use rdev::{listen, Event, EventType, Key as RdevKey};
use serde::{Deserialize, Serialize};
use std::{
  collections::HashSet,
  fs,
  path::Path,
  collections::HashMap,
  sync::mpsc,
  thread,
  time::{Duration, Instant},
};

/// Map rdev key to enigo key where possible (limited set for rhythm keys).
fn map_key(key: RdevKey) -> Option<Key> {
  use RdevKey::*;
  match key {
    KeyA => Some(Key::Layout('a')),
    KeyS => Some(Key::Layout('s')),
    KeyD => Some(Key::Layout('d')),
    KeyF => Some(Key::Layout('f')),
    KeyJ => Some(Key::Layout('j')),
    KeyK => Some(Key::Layout('k')),
    KeyL => Some(Key::Layout('l')),
    Space => Some(Key::Space),
    Return => Some(Key::Return),
    Tab => Some(Key::Tab),
    Escape => Some(Key::Escape),
    UpArrow => Some(Key::UpArrow),
    DownArrow => Some(Key::DownArrow),
    LeftArrow => Some(Key::LeftArrow),
    RightArrow => Some(Key::RightArrow),
    ShiftLeft => Some(Key::Shift),
    ShiftRight => Some(Key::Shift),
    ControlLeft => Some(Key::Control),
    ControlRight => Some(Key::Control),
    Alt => Some(Key::Alt),
    AltGr => Some(Key::Alt),
    MetaLeft => Some(Key::Meta),
    MetaRight => Some(Key::Meta),
    _ => None,
  }
}

fn key_to_string(key: Key) -> String {
  match key {
    Key::Layout(c) => format!("char:{c}"),
    Key::Space => "Space".into(),
    Key::Return => "Return".into(),
    Key::Tab => "Tab".into(),
    Key::Escape => "Escape".into(),
    Key::UpArrow => "UpArrow".into(),
    Key::DownArrow => "DownArrow".into(),
    Key::LeftArrow => "LeftArrow".into(),
    Key::RightArrow => "RightArrow".into(),
    Key::Shift => "Shift".into(),
    Key::Control => "Control".into(),
    Key::Alt => "Alt".into(),
    Key::Meta => "Meta".into(),
    _ => format!("other:{:?}", key),
  }
}

fn string_to_key(s: &str) -> Option<Key> {
  if let Some(ch) = s.strip_prefix("char:").and_then(|v| v.chars().next()) {
    return Some(Key::Layout(ch));
  }
  match s {
    "Space" => Some(Key::Space),
    "Return" => Some(Key::Return),
    "Tab" => Some(Key::Tab),
    "Escape" => Some(Key::Escape),
    "UpArrow" => Some(Key::UpArrow),
    "DownArrow" => Some(Key::DownArrow),
    "LeftArrow" => Some(Key::LeftArrow),
    "RightArrow" => Some(Key::RightArrow),
    "Shift" => Some(Key::Shift),
    "Control" => Some(Key::Control),
    "Alt" => Some(Key::Alt),
    "Meta" => Some(Key::Meta),
    _ => None,
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RecordedMode {
  Press,
  Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SerialMode {
  Press,
  Release,
}

#[derive(Clone, Debug)]
pub struct TimedEvent {
  pub key: Key,
  pub mode: RecordedMode,
  pub delta_ms: u128,
}

#[derive(Clone, Debug)]
pub struct Sample {
  pub id: u64,
  pub started_at: u128,
  pub name: String,
  pub events: Vec<TimedEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SerializableTimedEvent {
  pub key: String,
  pub mode: SerialMode,
  pub delta_ms: u128,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SerializableSample {
  pub id: u64,
  pub started_at: u128,
  pub name: String,
  pub events: Vec<SerializableTimedEvent>,
}

pub struct MacroRecorder {
  pub live_events: Vec<TimedEvent>,
  pub samples: Vec<Sample>,
  pub is_recording: bool,
  allowed_keys: Vec<Key>,
  pressed_keys: HashSet<Key>,
  recording_start: Option<Instant>,
  sample_counter: u64,
  per_key_offset: HashMap<String, i32>,
  receiver: Option<mpsc::Receiver<(RecordedMode, RdevKey, Instant)>>,
  pub hotkey_toggle: RdevKey,
  pub hotkey_playback: RdevKey,
  pub outlier_k: f64,
  pub use_median: bool,
  pub playback_offset_ms: i64,
  pub autosave: bool,
}

impl Default for MacroRecorder {
  fn default() -> Self {
    Self {
      live_events: Vec::new(),
      samples: Vec::new(),
      is_recording: false,
      allowed_keys: Vec::new(),
      pressed_keys: HashSet::new(),
      recording_start: None,
      sample_counter: 0,
      per_key_offset: HashMap::new(),
      receiver: None,
      hotkey_toggle: RdevKey::F9,
      hotkey_playback: RdevKey::F10,
      outlier_k: 2.0,
      use_median: false,
      playback_offset_ms: 0,
      autosave: true,
    }
  }
}

impl MacroRecorder {
  fn to_serializable(&self) -> Vec<SerializableSample> {
    self
      .samples
      .iter()
      .map(|s| SerializableSample {
        id: s.id,
        started_at: s.started_at,
        name: s.name.clone(),
        events: s
          .events
          .iter()
          .map(|e| SerializableTimedEvent {
            key: key_to_string(e.key),
            mode: match e.mode {
              RecordedMode::Press => SerialMode::Press,
              RecordedMode::Release => SerialMode::Release,
            },
            delta_ms: e.delta_ms,
          })
          .collect(),
      })
      .collect()
  }

  fn from_serializable(serial: Vec<SerializableSample>) -> Vec<Sample> {
    let mut res = Vec::new();
    for s in serial {
      let mut evts = Vec::new();
      for e in s.events {
        if let Some(key) = string_to_key(&e.key) {
          let mode = match e.mode {
            SerialMode::Press => RecordedMode::Press,
            SerialMode::Release => RecordedMode::Release,
          };
          evts.push(TimedEvent {
            key,
            mode,
            delta_ms: e.delta_ms,
          });
        }
      }
      res.push(Sample {
        id: s.id,
        started_at: s.started_at,
        name: s.name,
        events: evts,
      });
    }
    res
  }

  /// Spawn background global keyboard listener.
  pub fn start_listener(&mut self) {
    if self.receiver.is_some() {
      return;
    }
    let (tx, rx) = mpsc::channel::<(RecordedMode, RdevKey, Instant)>();
    self.receiver = Some(rx);

    thread::spawn(move || {
      let callback = move |event: Event| {
        if let EventType::KeyPress(k) | EventType::KeyRelease(k) = event.event_type {
          let mode = match event.event_type {
            EventType::KeyPress(_) => RecordedMode::Press,
            EventType::KeyRelease(_) => RecordedMode::Release,
            _ => return,
          };
          let _ = tx.send((mode, k, Instant::now()));
        }
      };

      if let Err(err) = listen(callback) {
        eprintln!("keyboard listener error: {:?}", err);
      }
    });
  }

  /// Poll channel and update in-memory buffers.
  pub fn poll_events(&mut self) {
    loop {
      let Some((mode, raw_key, ts)) = self.receiver.as_ref().and_then(|rx| rx.try_recv().ok())
      else {
        break;
      };

      // Toggle recording hotkey (handled regardless of mapping).
      if raw_key == self.hotkey_toggle && mode == RecordedMode::Press {
        if self.is_recording {
          self.finish_recording();
        } else {
          self.start_recording();
        }
        continue;
      }

      // Playback hotkey.
      if raw_key == self.hotkey_playback && mode == RecordedMode::Press {
        let events = self.timing_map();
        if !events.is_empty() {
          MacroRecorder::play_events(events, self.playback_offset_ms, self.per_key_offset.clone());
        }
        continue;
      }

      // Only record mapped keys while recording.
      if self.is_recording {
        if let Some(mapped) = map_key(raw_key) {
          if !self.allowed_keys.is_empty() && !self.allowed_keys.contains(&mapped) {
            continue;
          }
          if let Some(start) = self.recording_start {
            let delta_ms = ts.saturating_duration_since(start).as_millis();
            self.live_events.push(TimedEvent {
              key: mapped,
              mode,
              delta_ms,
            });
          }
          match mode {
            RecordedMode::Press => {
              self.pressed_keys.insert(mapped);
            }
            RecordedMode::Release => {
              self.pressed_keys.remove(&mapped);
            }
          }
        }
      }
    }
  }

  pub fn start_recording(&mut self) {
    self.is_recording = true;
    self.recording_start = Some(Instant::now());
    self.live_events.clear();
  }

  pub fn finish_recording(&mut self) {
    if self.is_recording {
      self.sample_counter += 1;
      self.samples.push(Sample {
        id: self.sample_counter,
        started_at: Instant::now().elapsed().as_millis(),
        name: format!("sample-{}", self.sample_counter),
        events: self.live_events.clone(),
      });
      if self.autosave {
        let _ = self.save_to_disk(Path::new("samples.json"));
      }
    }
    self.is_recording = false;
    self.recording_start = None;
    self.live_events.clear();
  }

  pub fn clear(&mut self) {
    self.live_events.clear();
    self.pressed_keys.clear();
  }

  /// Compute simple average timing map per (key, mode, index position).
  pub fn timing_map(&self) -> Vec<TimedEvent> {
    use std::collections::HashMap;

    fn key_id(key: Key) -> String {
      format!("{:?}", key)
    }

    let mut per_slot: HashMap<(String, RecordedMode, usize), Vec<(u128, u128)>> = HashMap::new();
    let expected_len = self
      .samples
      .iter()
      .filter(|s| !s.events.is_empty())
      .map(|s| s.events.len())
      .min()
      .unwrap_or(0);
    for sample in &self.samples {
      let weight = sample.events.len().max(1) as u128;
      for (idx, evt) in sample.events.iter().enumerate() {
        if idx >= expected_len {
          // Drop extras beyond shortest sample length.
          break;
        }
        per_slot
          .entry((key_id(evt.key), evt.mode, idx))
          .or_default()
          .push((evt.delta_ms, weight));
      }
    }

    let mut aggregated: Vec<TimedEvent> = Vec::new();
    for ((k_id, mode, _idx), values) in per_slot {
      if values.is_empty() {
        continue;
      }
      let mut vals: Vec<u128> = values.iter().map(|(v, _w)| *v).collect();
      vals.sort();
      let avg = if self.use_median {
        let mid = vals.len() / 2;
        vals[mid]
      } else {
        let (sum_w, sum_vw): (u128, u128) = values.iter().fold((0, 0), |acc, (v, w)| {
          (acc.0 + w, acc.1 + v * w)
        });
        let mean = if sum_w == 0 { 0.0 } else { sum_vw as f64 / sum_w as f64 };
        let var = values
          .iter()
          .map(|(v, w)| {
            let d = *v as f64 - mean;
            d * d * *w as f64
          })
          .sum::<f64>()
          / sum_w.max(1) as f64;
        let std = var.sqrt();
        let filtered: Vec<u128> = values
          .iter()
          .copied()
          .filter(|(v, _w)| ((*v as f64 - mean).abs()) <= self.outlier_k * std)
          .map(|(v, _w)| v)
          .collect();
        let use_vals: Vec<u128> = if filtered.is_empty() { vals.clone() } else { filtered };
        use_vals.iter().copied().sum::<u128>() / use_vals.len().max(1) as u128
      };

      let key = match k_id.as_str() {
        other => {
          if let Some(ch) = other.strip_prefix("Layout(").and_then(|s| s.chars().nth(0)) {
            Key::Layout(ch)
          } else {
            Key::Layout('?')
          }
        }
      };
      aggregated.push(TimedEvent {
        key,
        mode,
        delta_ms: avg,
      });
    }

    aggregated.sort_by_key(|e| e.delta_ms);
    aggregated
  }

  /// Playback a list of timed events with sleeping to target deltas.
  pub fn play_events(
    events: Vec<TimedEvent>,
    offset_ms: i64,
    per_key_offset: HashMap<String, i32>,
  ) {
    thread::spawn(move || {
      let mut enigo = Enigo::new();
      let start = Instant::now();
      for evt in events {
        let key_adj = per_key_offset
          .get(&key_to_string(evt.key))
          .copied()
          .unwrap_or(0);
        let base = evt.delta_ms as i64 + offset_ms + key_adj as i64;
        let adjusted = if base >= 0 { base as u128 } else { 0 };
        let target = Duration::from_millis(adjusted as u64);
        let now = Instant::now();
        let elapsed = now.saturating_duration_since(start);
        if target > elapsed {
          let remain = target - elapsed;
          // Sleep for the bulk, spin for the last ~1ms to reduce overshoot.
          if remain > Duration::from_millis(1) {
            thread::sleep(remain - Duration::from_millis(1));
          }
          while Instant::now().saturating_duration_since(start) < target {
            std::hint::spin_loop();
          }
        }
        match evt.mode {
          RecordedMode::Press => {
            let _ = enigo.key_down(evt.key);
          }
          RecordedMode::Release => {
            let _ = enigo.key_up(evt.key);
          }
        }
      }
    });
  }

  pub fn play_sample(&self, sample_id: u64) {
    if let Some(sample) = self.samples.iter().find(|s| s.id == sample_id) {
      MacroRecorder::play_events(
        sample.events.clone(),
        self.playback_offset_ms,
        self.per_key_offset.clone(),
      );
    }
  }

  pub fn play_timing_map(&self, events: Vec<TimedEvent>) {
    MacroRecorder::play_events(events, self.playback_offset_ms, self.per_key_offset.clone());
  }

  pub fn set_key_offset(&mut self, key: Key, offset: i32) {
    self.per_key_offset.insert(key_to_string(key), offset);
  }

  pub fn key_offsets_snapshot(&self) -> Vec<(Key, i32)> {
    self
      .per_key_offset
      .iter()
      .filter_map(|(k, v)| string_to_key(k).map(|real_key| (real_key, *v)))
      .collect()
  }

  pub fn set_allowed_keys(&mut self, keys: Vec<Key>) {
    self.allowed_keys = keys;
  }

  pub fn allowed_keys_snapshot(&self) -> Vec<Key> {
    self.allowed_keys.clone()
  }

  pub fn pressed_keys_snapshot(&self) -> HashSet<Key> {
    self.pressed_keys.clone()
  }

  pub fn set_hotkeys(&mut self, toggle: RdevKey, playback: RdevKey) {
    self.hotkey_toggle = toggle;
    self.hotkey_playback = playback;
  }

  pub fn samples_snapshot(&self) -> Vec<Sample> {
    self.samples.clone()
  }

  pub fn delete_sample(&mut self, id: u64) {
    self.samples.retain(|s| s.id != id);
    let _ = self.save_to_disk(Path::new("samples.json"));
  }

  pub fn rename_sample(&mut self, id: u64, name: String) {
    if let Some(s) = self.samples.iter_mut().find(|s| s.id == id) {
      s.name = name;
      if self.autosave {
        let _ = self.save_to_disk(Path::new("samples.json"));
      }
    }
  }

  pub fn save_to_disk(&self, path: &Path) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(&self.to_serializable()).unwrap_or_default();
    fs::write(path, json)
  }

  pub fn load_from_disk(&mut self, path: &Path) -> std::io::Result<()> {
    if !path.exists() {
      return Ok(());
    }
    let data = fs::read_to_string(path)?;
    if data.trim().is_empty() {
      return Ok(());
    }
    let loaded: Vec<SerializableSample> = serde_json::from_str(&data).unwrap_or_default();
    let converted = MacroRecorder::from_serializable(loaded);
    self.sample_counter = converted.iter().map(|s| s.id).max().unwrap_or(0);
    self.samples = converted;
    Ok(())
  }
}
