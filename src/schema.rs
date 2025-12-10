
use enigo::Key;
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub enum KeyAction {
  Down(Key),
  Up(Key),
}

#[derive(Clone, Copy, Debug)]
pub struct TimedEvent {
  pub at: Duration,
  pub action: KeyAction,
}
