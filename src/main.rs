use eframe::egui::{self, Color32};
use std::sync::atomic::Ordering;
use std::time::Duration;

mod macro_play;
mod schema;
mod state;
use state::{key_label, AppState};

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
    Box::new(move |_cc| {
      Box::new(RecorderApp {
        state: state.clone(),
        overlay_open: true,
        key_input: String::new(),
      })
    }),
  )
}

struct RecorderApp {
  state: AppState,
  overlay_open: bool,
  key_input: String,
}

impl eframe::App for RecorderApp {
  fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    // Keep UI refreshing so counters update even without mouse movement.
    ctx.request_repaint_after(Duration::from_millis(16));
    egui::CentralPanel::default().show(ctx, |ui| {
      ui.heading("Auto Note Recorder");
      ui.separator();

      let is_rec = self.state.recording.load(Ordering::SeqCst);
      ui.horizontal(|ui| {
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
      ui.heading("Tracked Keys");
      ui.horizontal(|ui| {
        ui.label("Add keys (characters):");
        ui.add(egui::TextEdit::singleline(&mut self.key_input).desired_width(140.0));
        if ui.button("Add").clicked() {
          self.state.add_tracked_keys_from_text(&self.key_input);
          self.key_input.clear();
        }
      });
      ui.horizontal_wrapped(|ui| {
        ui.label("Registered:");
        let keys = self.state.tracked_keys();
        for (i, k) in keys.iter().enumerate() {
          ui.horizontal(|ui| {
            ui.label(format!("[{}]", key_label(k)));
            if ui.small_button("x").clicked() {
              self.state.remove_tracked_key(i);
            }
          });
        }
      });
      ui.horizontal_wrapped(|ui| {
        let label = if self.overlay_open { "Hide Overlay" } else { "Show Overlay" };
        if ui.button(label).clicked() {
          self.overlay_open = !self.overlay_open;
        }
      });

      ui.separator();
      ui.heading("Recorded Samples");
      if ui.button("Merge All Samples").clicked() {
        self.state.merge_samples();
      }
      let mut to_delete: Option<usize> = None;
      let mut play_events: Option<Vec<schema::TimedEvent>> = None;
      egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
        let mut samples = self.state.samples.lock().unwrap();
        for idx in 0..samples.len() {
          ui.horizontal(|ui| {
            ui.label(format!("#{}:", idx + 1));
            ui.add(
              egui::TextEdit::singleline(&mut samples[idx].name)
                .desired_width(160.0),
            );
            ui.label(format!("{} events", samples[idx].events.len()));
            if ui.add_sized([36.0, 22.0], egui::Button::new("Up")).clicked() && idx > 0 {
              samples.swap(idx - 1, idx);
            }
            if ui
              .add_sized([36.0, 22.0], egui::Button::new("Down"))
              .clicked()
              && idx + 1 < samples.len()
            {
              samples.swap(idx, idx + 1);
            }
            if ui.button("Play").clicked() {
              play_events = Some(samples[idx].events.clone());
            }
            if ui.button("Delete").clicked() {
              to_delete = Some(idx);
            }
          });
        }
        if let Some(idx) = to_delete {
          samples.remove(idx);
        }
      });
      if let Some(evs) = play_events {
        self.state.playback_sample(&evs);
      }
    });

    // Overlay window for key viewer
    if self.overlay_open {
      ctx.show_viewport_immediate(
        egui::ViewportId::from_hash_of("key-overlay"),
        egui::ViewportBuilder::default()
          .with_title("Key Overlay")
          .with_always_on_top()
          .with_decorations(false)
          .with_transparent(true)
          .with_inner_size([260.0, 160.0]),
        |overlay_ctx, _class| {
          let mut style: egui::Style = (*overlay_ctx.style()).clone();
          style.visuals.window_fill = Color32::TRANSPARENT;
          style.visuals.panel_fill = Color32::TRANSPARENT;
          style.visuals.extreme_bg_color = Color32::TRANSPARENT;
          style.visuals.faint_bg_color = Color32::TRANSPARENT;
          style.visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
          style.visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
          overlay_ctx.set_style(style);
          egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::TRANSPARENT))
            .show(overlay_ctx, |ui| {
              egui::ScrollArea::horizontal()
                .max_height(80.0)
                .show(ui, |ui| {
                  ui.horizontal(|ui| {
                    for (key, pressed) in self.state.tracked_key_states() {
                      let fill = if pressed {
                        Color32::from_rgb(120, 220, 120)
                      } else {
                        Color32::from_rgb(60, 60, 60)
                      };
                      let stroke = egui::Stroke::new(1.0, Color32::from_rgb(200, 200, 200));
                      let (rect, _resp) =
                        ui.allocate_exact_size(egui::vec2(60.0, 34.0), egui::Sense::hover());
                      ui.painter()
                        .rect(rect, 6.0, fill, stroke);
                      ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        key_label(&key),
                        egui::TextStyle::Button.resolve(ui.style()),
                        Color32::BLACK,
                      );
                    }
                  });
                });
            });
        },
      );
    }
  }
}
