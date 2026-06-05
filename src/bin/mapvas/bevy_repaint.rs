use std::sync::{
  Mutex,
  mpsc::{self, Receiver},
};

use bevy::{prelude::*, window::RequestRedraw};
use mapvas::remote::RepaintSignal;

pub struct BevyRepaintPlugin;

impl Plugin for BevyRepaintPlugin {
  fn build(&self, app: &mut App) {
    app.add_systems(Update, issue_bevy_repaints);
  }
}

#[derive(Resource)]
pub struct BevyRepaintRequests {
  receiver: Mutex<Receiver<()>>,
}

impl BevyRepaintRequests {
  pub fn channel() -> (RepaintSignal, Self) {
    let (sender, receiver) = mpsc::channel();
    (
      RepaintSignal::channel(sender),
      Self {
        receiver: Mutex::new(receiver),
      },
    )
  }
}

fn issue_bevy_repaints(
  requests: Res<BevyRepaintRequests>,
  mut redraw: MessageWriter<RequestRedraw>,
) {
  let mut requested = false;
  if let Ok(receiver) = requests.receiver.lock() {
    while receiver.try_recv().is_ok() {
      requested = true;
    }
  }

  if requested {
    redraw.write(RequestRedraw);
  }
}
