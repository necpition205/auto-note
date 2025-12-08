# Rhythm Macro Overlay PRD

## 1. Goal
Record global key input samples from multiplayer rhythm gameplay, aggregate timing maps (press/release) to maximize judgment accuracy, and auto-playback according to the derived timing map. Provide an overlay GUI with key viewer and optional raining-note visualization for recording and comparison.

## 2. Users / Context
- Player testing and building auto macros for a specific multiplayer rhythm game.
- Runs on Windows desktop; needs global keyboard hook and simulated key press/release.

## 3. Core Flow
1) Start recording (UI button + configurable hotkey).
2) Capture key events from the start timestamp: `{key, mode: press|release, delta_ms}`.
3) Stop recording ⇒ store as one sample.
4) Repeat multiple plays to accumulate samples.
5) Build timing map: aggregate per key and mode, average (and optionally median/weighted) timings, handling missing/extra events.
6) Run playback: send keys based on timing map.

## 4. Functional Requirements
- Global keyboard capture (press/release), timestamped relative to recording start.
- Configurable recording toggle hotkey (e.g., F9) and UI button.
- Sample storage: append each run as a named sample; show count and summary.
- Timing map generation:
  - Aggregate per key+mode sequences; compute average/median with weighting toward runs with more complete data.
  - Handle missing events (e.g., fill with average, or drop outlier runs) and extra events (ignore extras beyond expected length or flag to user).
  - Provide adjustable smoothing window/outlier rejection (basic: discard extremes beyond configurable threshold).
- Playback engine:
  - Uses aggregated timing map to schedule key down/up.
  - Optional jitter compensation offset per key.
- UI/UX (overlay):
  - Start/Stop recording, Clear, Play Back buttons.
  - Status indicators (recording, sample count, last sample length).
  - Scrollable event list (for debugging) or summary.
  - Always-on-top, draggable window.
- Key viewer & raining-note (nice-to-have v1 visuals):
  - Register tracked keys (e.g., ASDF, arrows).
  - When pressed during recording, show note rising; previous runs shown semi-transparent for comparison in later runs.
- Persistence (nice-to-have):
  - Save samples/timing map to disk (JSON) and load on start.

## 5. Non-Functional Requirements
- Low latency capture (<5 ms overhead if possible).
- Playback timing precision within a few ms (subject to OS scheduling).
- Runs without requiring game focus loss; overlay should not consume inputs.

## 6. Data Model (proposed)
- Event: `{key: String, mode: "press"|"release", delta_ms: u64}`.
- Sample: `{id: UUID, started_at: timestamp, events: Vec<Event>}`.
- TimingMapEntry: `{key, mode, avg_ms: f64, count: u32, std_ms: f64}` (extendable with median, iqr).
- TimingMap: `HashMap<(key, mode), Vec<TimingMapEntry>>` or simplified per index position.

## 7. Algorithms
- Aggregation: align events per key+mode by index position across samples; compute average/median per position.
- Weighting: favor samples with full sequences; optionally weight by recency.
- Missing/extra handling: drop extras beyond expected length; for missing, impute with running average or nearest neighbor.
- Outlier rejection: discard if |delta - mean| > k * std (configurable).

## 8. Controls & Config
- Hotkeys: start/stop recording (default F9), playback (optional), clear (optional).
- Smoothing/outlier threshold, weighting factor.
- Playback delay offset per key.

## 9. Risks / Edge Cases
- Global hook permissions on Windows; may require admin.
- Game anti-cheat sensitivity to hooks/simulation.
- OS scheduling jitter; need small sleeps and buffered scheduling.
- Key mapping differences (IME, layouts) — keep layout-based mapping with override table.

## 10. Milestones (phased)
1) Build overlay with record/playback UI and global capture with timestamps.
2) Sample storage and timing map aggregation with basic averaging and playback.
3) Persistence (save/load JSON) and configurable hotkeys.
4) Visual key viewer + raining-note overlay with previous-run ghost notes.
5) Refinements: outlier handling, weighting controls, per-key offsets.
