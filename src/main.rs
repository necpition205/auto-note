mod recorder;

use eframe::{egui, NativeOptions};
use enigo::Key;
use rdev::Key as RdevKey;
use recorder::MacroRecorder;
use std::{
  collections::{BTreeMap, HashMap},
  sync::{Arc, Mutex},
};

struct OverlayApp {
  recorder: Arc<Mutex<MacroRecorder>>,
  allowed_input: String,
  last_parse_error: Option<String>,
  hotkey_toggle_input: String,
  hotkey_playback_input: String,
  rain_anchor_ms: f64,
  save_path: String,
  rain_speed: f32,
}

impl OverlayApp {
  fn new() -> Self {
    let recorder = Arc::new(Mutex::new(MacroRecorder::default()));
    if let Ok(mut guard) = recorder.lock() {
      guard.start_listener();
      let _ = guard.load_from_disk(std::path::Path::new("samples.json"));
    }
    Self {
      recorder,
      allowed_input: String::from("a,s,d,f,j,k,l,space"),
      last_parse_error: None,
      hotkey_toggle_input: String::from("F9"),
      hotkey_playback_input: String::from("F10"),
      rain_anchor_ms: 0.0,
      save_path: String::from("samples.json"),
      rain_speed: 1.2,
    }
  }
}

fn parse_key_token(token: &str) -> Result<Key, String> {
  let t = token.trim().to_lowercase();
  if t.is_empty() {
    return Err("empty token".into());
  }
  if t.len() == 1 {
    let ch = t.chars().next().unwrap();
    return Ok(Key::Layout(ch));
  }
  let key = match t.as_str() {
    "space" => Key::Space,
    "enter" | "return" => Key::Return,
    "tab" => Key::Tab,
    "esc" | "escape" => Key::Escape,
    "up" => Key::UpArrow,
    "down" => Key::DownArrow,
    "left" => Key::LeftArrow,
    "right" => Key::RightArrow,
    "shift" => Key::Shift,
    "ctrl" | "control" => Key::Control,
    "alt" => Key::Alt,
    "meta" | "win" | "cmd" => Key::Meta,
    other => return Err(format!("unsupported key: {other}")),
  };
  Ok(key)
}

fn parse_allowed(input: &str) -> Result<Vec<Key>, String> {
  let mut keys = Vec::new();
  for tok in input.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
    keys.push(parse_key_token(tok)?);
  }
  if keys.is_empty() {
    return Err("no keys provided".into());
  }
  Ok(keys)
}

fn parse_rdev_key(input: &str) -> Option<RdevKey> {
  match input.trim().to_uppercase().as_str() {
    "F1" => Some(RdevKey::F1),
    "F2" => Some(RdevKey::F2),
    "F3" => Some(RdevKey::F3),
    "F4" => Some(RdevKey::F4),
    "F5" => Some(RdevKey::F5),
    "F6" => Some(RdevKey::F6),
    "F7" => Some(RdevKey::F7),
    "F8" => Some(RdevKey::F8),
    "F9" => Some(RdevKey::F9),
    "F10" => Some(RdevKey::F10),
    "F11" => Some(RdevKey::F11),
    "F12" => Some(RdevKey::F12),
    "SPACE" => Some(RdevKey::Space),
    other => {
      if other.len() == 1 {
        match other.chars().next().unwrap() {
          'A' => Some(RdevKey::KeyA),
          'B' => Some(RdevKey::KeyB),
          'C' => Some(RdevKey::KeyC),
          'D' => Some(RdevKey::KeyD),
          'E' => Some(RdevKey::KeyE),
          'F' => Some(RdevKey::KeyF),
          'G' => Some(RdevKey::KeyG),
          'H' => Some(RdevKey::KeyH),
          'I' => Some(RdevKey::KeyI),
          'J' => Some(RdevKey::KeyJ),
          'K' => Some(RdevKey::KeyK),
          'L' => Some(RdevKey::KeyL),
          'M' => Some(RdevKey::KeyM),
          'N' => Some(RdevKey::KeyN),
          'O' => Some(RdevKey::KeyO),
          'P' => Some(RdevKey::KeyP),
          'Q' => Some(RdevKey::KeyQ),
          'R' => Some(RdevKey::KeyR),
          'S' => Some(RdevKey::KeyS),
          'T' => Some(RdevKey::KeyT),
          'U' => Some(RdevKey::KeyU),
          'V' => Some(RdevKey::KeyV),
          'W' => Some(RdevKey::KeyW),
          'X' => Some(RdevKey::KeyX),
          'Y' => Some(RdevKey::KeyY),
          'Z' => Some(RdevKey::KeyZ),
          _ => None,
        }
      } else {
        None
      }
    }
  }
}

impl eframe::App for OverlayApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    let now_ms = ctx.input(|i| i.time) * 1000.0;
    if self.rain_anchor_ms <= 0.0 {
      self.rain_anchor_ms = now_ms;
    }
    if let Ok(mut rec) = self.recorder.lock() {
      rec.poll_events();
    }

    egui::TopBottomPanel::top("controls").show(ctx, |ui| {
      ui.heading("Macro Overlay");
      ui.label("Record global key input and play it back.");
    });

    egui::CentralPanel::default().show(ctx, |ui| {
      if let Ok(mut rec) = self.recorder.lock() {
        let timing_avg = rec.timing_map();
        ui.group(|ui| {
          ui.label("Allowed keys (comma separated, e.g., a,s,d,f,j,k,l,space)");
          let edit = ui.text_edit_singleline(&mut self.allowed_input);
          if edit.lost_focus() {
            self.last_parse_error = None;
          }
          if ui.button("Apply").clicked() {
            match parse_allowed(&self.allowed_input) {
              Ok(list) => {
                rec.set_allowed_keys(list);
                self.last_parse_error = None;
              }
              Err(err) => {
                self.last_parse_error = Some(err);
              }
            }
          }
          if let Some(err) = &self.last_parse_error {
            ui.colored_label(egui::Color32::RED, err);
          }
          ui.label("Current allowed:");
          let allowed = rec.allowed_keys_snapshot();
          ui.horizontal_wrapped(|ui| {
            for k in allowed {
              ui.label(format!("{k:?}"));
            }
          });
        });

        ui.horizontal(|ui| {
          if ui
            .button(if rec.is_recording {
              "Stop Recording"
            } else {
              "Start Recording"
            })
            .clicked()
          {
            if rec.is_recording {
              rec.finish_recording();
            } else {
              rec.start_recording();
              self.rain_anchor_ms = now_ms;
            }
          }
          if ui.button("Clear").clicked() {
            rec.clear();
            self.rain_anchor_ms = now_ms;
          }
          if ui.button("Play Back (avg)").clicked() {
            if !timing_avg.is_empty() {
              rec.play_timing_map(timing_avg.clone());
            }
          }
          ui.label("Save path:");
          ui.text_edit_singleline(&mut self.save_path);
          if ui.button("Save samples").clicked() {
            let _ = rec.save_to_disk(std::path::Path::new(&self.save_path));
          }
          if ui.button("Load samples").clicked() {
            let _ = rec.load_from_disk(std::path::Path::new(&self.save_path));
          }
        });

        ui.separator();
        ui.label(format!("Live events: {}", rec.live_events.len()));
        ui.label(format!("Samples stored: {}", rec.samples.len()));
        if rec.is_recording {
            ui.colored_label(egui::Color32::LIGHT_GREEN, "Recording...");
        } else if !rec.samples.is_empty() {
            ui.colored_label(egui::Color32::LIGHT_BLUE, "Samples ready.");
        }

        ui.separator();
        ui.label("Key viewer (currently pressed):");
        let pressed = rec.pressed_keys_snapshot();
        egui::Frame::none()
          .fill(egui::Color32::TRANSPARENT)
          .show(ui, |ui| {
            if pressed.is_empty() {
              ui.label("None");
            } else {
              ui.horizontal_wrapped(|ui| {
                for k in pressed {
                  ui.colored_label(egui::Color32::LIGHT_YELLOW, format!("{k:?}"));
                }
              });
            }
          });

        ui.separator();
        ui.label("Hotkeys (press Enter to apply):");
        let toggle_resp = ui.text_edit_singleline(&mut self.hotkey_toggle_input);
        let playback_resp = ui.text_edit_singleline(&mut self.hotkey_playback_input);
        let apply_hotkey = toggle_resp.lost_focus() || playback_resp.lost_focus();
        if apply_hotkey {
          if let (Some(t), Some(p)) = (
            parse_rdev_key(&self.hotkey_toggle_input),
            parse_rdev_key(&self.hotkey_playback_input),
          ) {
            rec.set_hotkeys(t, p);
          }
        }

        ui.horizontal(|ui| {
          ui.label("Outlier k:");
          let mut k = rec.outlier_k;
          if ui.add(egui::DragValue::new(&mut k).clamp_range(0.5..=5.0)).changed() {
            rec.outlier_k = k;
          }
          ui.checkbox(&mut rec.use_median, "Use median instead of mean");
        });

        ui.horizontal(|ui| {
          ui.label("Playback offset (ms):");
          ui.add(egui::Slider::new(&mut rec.playback_offset_ms, -50..=50));
        });

        ui.horizontal(|ui| {
          ui.label("Rain speed:");
          ui.add(egui::Slider::new(&mut self.rain_speed, 0.5..=3.0).show_value(true));
          if ui.button("Reset rain").clicked() {
            self.rain_anchor_ms = now_ms;
          }
        });

        ui.horizontal(|ui| {
          ui.checkbox(&mut rec.autosave, "Autosave samples");
          ui.label("Per-key offset (ms) for allowed keys:");
        });
        egui::ScrollArea::horizontal()
          .max_height(80.0)
          .show(ui, |ui| {
            for key in rec.allowed_keys_snapshot() {
              let mut entry = rec
                .key_offsets_snapshot()
                .into_iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v)
                .unwrap_or(0);
              ui.vertical(|ui| {
                ui.label(format!("{key:?}"));
                if ui
                  .add(egui::Slider::new(&mut entry, -30..=30).show_value(true))
                  .changed()
                {
                  rec.set_key_offset(key, entry);
                }
              });
              ui.add_space(6.0);
            }
          });

        ui.separator();
        ui.label("Samples:");
        egui::ScrollArea::vertical()
          .max_height(140.0)
          .show(ui, |ui| {
            for sample in rec.samples_snapshot() {
              let mut key_count: HashMap<String, usize> = HashMap::new();
              for e in &sample.events {
                *key_count.entry(format!("{:?}", e.key)).or_default() += 1;
              }
              let length_ms = sample.events.iter().map(|e| e.delta_ms).max().unwrap_or(0);
              ui.horizontal(|ui| {
                ui.label(format!("ID {}", sample.id));
                let mut name = sample.name.clone();
                if ui.text_edit_singleline(&mut name).lost_focus() && name != sample.name {
                  rec.rename_sample(sample.id, name);
                }
                ui.label(format!("events {} | len {} ms", sample.events.len(), length_ms));
                if !key_count.is_empty() {
                  ui.label(format!(
                    " keys: {}",
                    key_count.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(", ")
                  ));
                }
                if ui.button("Play").clicked() {
                  rec.play_sample(sample.id);
                }
                if ui.button("Delete").clicked() {
                  rec.delete_sample(sample.id);
                }
              });
            }
          });

        egui::ScrollArea::vertical()
          .max_height(200.0)
          .show(ui, |ui| {
            for (idx, evt) in rec.live_events.iter().enumerate() {
              let label = format!("{idx}: {:?} {:?} {}ms", evt.mode, evt.key, evt.delta_ms);
              ui.monospace(label);
            }
          });

        if !rec.samples.is_empty() {
          ui.separator();
          ui.label("Aggregated timing (avg):");
          for (idx, evt) in timing_avg.iter().enumerate() {
            ui.monospace(format!("{idx}: {:?} {:?} {}ms", evt.mode, evt.key, evt.delta_ms));
          }

          // Ghost rain preview (static): show bars per key with relative timing.
          if !timing_avg.is_empty() {
            ui.separator();
            ui.label("Rain / Ghost preview:");
            // lanes: key -> (current live presses, ghost presses from last sample)
            let lane_width = 60.0;
            let lane_height = 160.0;
            let mut lanes: BTreeMap<String, (Vec<u128>, Vec<u128>)> = BTreeMap::new();
            for evt in rec.live_events.iter().filter(|e| e.mode == recorder::RecordedMode::Press) {
              lanes.entry(format!("{:?}", evt.key)).or_default().0.push(evt.delta_ms);
            }
            if let Some(last_sample) = rec.samples_snapshot().last() {
              for evt in last_sample
                .events
                .iter()
                .filter(|e| e.mode == recorder::RecordedMode::Press)
              {
                lanes
                  .entry(format!("{:?}", evt.key))
                  .or_default()
                  .1
                  .push(evt.delta_ms);
              }
            }
            let max_ms = lanes
              .values()
              .flat_map(|(live, ghost)| live.iter().chain(ghost.iter()))
              .copied()
              .max()
              .unwrap_or(1);
            let fall_speed = self.rain_speed.max(0.1); // higher is faster

            egui::ScrollArea::horizontal().show(ui, |ui| {
              for (key_label, (live, ghost)) in lanes {
                ui.vertical(|ui| {
                  ui.label(&key_label);
                  let (rect, _resp) = ui.allocate_exact_size(egui::vec2(lane_width, lane_height), egui::Sense::hover());
                  let painter = ui.painter_at(rect);
                  let live_color = egui::Color32::from_rgba_unmultiplied(120, 200, 255, 180);
                  let ghost_color = egui::Color32::from_rgba_unmultiplied(200, 120, 255, 100);
                  for t in ghost {
                    let anim = (t as f32) - ((now_ms - self.rain_anchor_ms) as f32 * fall_speed);
                    if anim < 0.0 {
                      continue;
                    }
                    let y = rect.bottom() - (anim / max_ms as f32 * lane_height);
                    let bar = egui::Rect::from_min_max(
                      egui::pos2(rect.left(), y - 3.0),
                      egui::pos2(rect.right(), y + 3.0),
                    );
                    painter.rect_filled(bar, 2.0, ghost_color);
                  }
                  for t in live {
                    let anim = (t as f32) - ((now_ms - self.rain_anchor_ms) as f32 * fall_speed);
                    if anim < 0.0 {
                      continue;
                    }
                    let y = rect.bottom() - (anim / max_ms as f32 * lane_height);
                    let bar = egui::Rect::from_min_max(
                      egui::pos2(rect.left(), y - 4.0),
                      egui::pos2(rect.right(), y + 4.0),
                    );
                    painter.rect_filled(bar, 3.0, live_color);
                  }
                });
                ui.add_space(8.0);
              }
            });
          }
        }
      }
    });
  }
}

fn main() -> eframe::Result<()> {
  let native_options = NativeOptions {
    viewport: egui::ViewportBuilder::default()
      .with_always_on_top()
      .with_decorations(true)
      .with_transparent(true)
      .with_inner_size([420.0, 360.0]),
    ..Default::default()
  };

  eframe::run_native(
    "Auto Note Macro Overlay",
    native_options,
    Box::new(|_| Box::new(OverlayApp::new())),
  )
}
