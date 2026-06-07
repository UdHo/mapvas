use std::sync::{
  Arc, Mutex,
  mpsc::{self, Receiver},
};

use crate::remote::RepaintSignal;
use bevy::{
  prelude::*,
  window::RequestRedraw,
  winit::{EventLoopProxyWrapper, WinitUserEvent},
};
use winit::event_loop::EventLoopProxy;

pub struct BevyRepaintPlugin;

impl Plugin for BevyRepaintPlugin {
  fn build(&self, app: &mut App) {
    app
      .init_resource::<BevyWakeup>()
      .add_systems(Startup, install_bevy_wakeup_proxy)
      .add_systems(Update, issue_bevy_repaints);
  }
}

#[derive(Clone, Default, Resource)]
pub struct BevyWakeup {
  event_loop_proxy: Arc<Mutex<Option<EventLoopProxy<WinitUserEvent>>>>,
}

impl BevyWakeup {
  pub fn set_event_loop_proxy(&self, proxy: EventLoopProxy<WinitUserEvent>) {
    if let Ok(mut event_loop_proxy) = self.event_loop_proxy.lock() {
      *event_loop_proxy = Some(proxy);
    }
  }

  pub fn wake(&self) {
    let proxy = self
      .event_loop_proxy
      .lock()
      .ok()
      .and_then(|event_loop_proxy| event_loop_proxy.clone());
    if let Some(proxy) = proxy {
      let _ = proxy.send_event(WinitUserEvent::WakeUp);
    }
  }
}

#[derive(Resource)]
pub struct BevyRepaintRequests {
  receiver: Mutex<Receiver<()>>,
}

impl BevyRepaintRequests {
  pub fn channel(wakeup: BevyWakeup) -> (RepaintSignal, Self) {
    let (sender, receiver) = mpsc::channel();
    let signal_wakeup = wakeup.clone();
    (
      RepaintSignal::from_fn(move || {
        let _ = sender.send(());
        signal_wakeup.wake();
      }),
      Self {
        receiver: Mutex::new(receiver),
      },
    )
  }
}

fn install_bevy_wakeup_proxy(
  wakeup: Res<BevyWakeup>,
  event_loop_proxy: Option<Res<EventLoopProxyWrapper>>,
) {
  let Some(event_loop_proxy) = event_loop_proxy else {
    return;
  };
  wakeup.set_event_loop_proxy((*event_loop_proxy).clone());
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
