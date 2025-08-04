use log::debug;
use mapvas::map::coordinates::PixelCoordinate;
use mapvas::map::geometry_collection::Geometry;
use mapvas::map::map_event::{Layer, MapEvent};
use mapvas::remote::DEFAULT_PORT;
use std::process::Stdio;

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::sync::{Mutex, Notify};

pub struct MapSender {
  sender: UnboundedSender<Option<MapEvent>>,
  inner_join_handle: tokio::task::JoinHandle<()>,
}

struct SenderInner {
  receiver: UnboundedReceiver<Option<MapEvent>>,
  queue: VecDeque<MapEvent>,
  send_counter: Arc<Mutex<usize>>,
  notify: Arc<Notify>,
}

impl SenderInner {
  pub fn start(receiver: UnboundedReceiver<Option<MapEvent>>) -> tokio::task::JoinHandle<()> {
    tokio::spawn({
      Self {
        receiver,
        queue: VecDeque::new(),
        send_counter: Arc::new(Mutex::new(0)),
        notify: Arc::new(Notify::new()),
      }
      .run()
    })
  }

  async fn run(mut self) {
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    loop {
      tokio::select! {
        Some(event) = self.receiver.recv() => {
           if let Some(event) = event {self.receive(event);} else {
                 self.send_queue().await;
                 self.wait_for_completion().await;
                 return;
              }
            }
        _ = interval.tick() => {
            self.send_queue().await;
          },
      }
    }
  }

  fn receive(&mut self, event: MapEvent) {
    self.queue.push_back(event);
  }

  async fn add_task(&self) {
    let mut send_count = self.send_counter.lock().await;
    *send_count += 1;
  }

  async fn send_queue(&mut self) {
    self.add_task().await;
    let mut queue = VecDeque::new();
    std::mem::swap(&mut queue, &mut self.queue);

    let send_counter = self.send_counter.clone();
    let notify = self.notify.clone();
    tokio::spawn(async move {
      Self::compact_and_send(queue).await;
      let mut count = send_counter.lock().await;
      *count -= 1;
      if *count == 0 {
        notify.notify_waiters();
      }
    });
  }

  async fn wait_for_completion(&self) {
    loop {
      let count = *self.send_counter.lock().await;
      if count == 0 {
        break;
      }
      self.notify.notified().await;
    }
  }

  async fn compact_and_send(queue: VecDeque<MapEvent>) {
    let mut layers: BTreeMap<String, Vec<Geometry<PixelCoordinate>>> = BTreeMap::new();
    let mut non_layer_events = Vec::new();
    let mut has_layer_events = false;

    for event in queue {
      match event {
        MapEvent::Layer(Layer { id, mut geometries }) => {
          layers
            .entry(id)
            .and_modify(|e| e.append(&mut geometries))
            .or_insert(geometries);
          has_layer_events = true;
        }
        e => non_layer_events.push(e),
      }
    }

    for event in &non_layer_events {
      if !matches!(event, MapEvent::Focus | MapEvent::Screenshot(_)) {
        Self::send_event(event).await;
      }
    }

    if has_layer_events {
      for (id, geometries) in layers {
        Self::send_event(&MapEvent::Layer(Layer { id, geometries })).await;
      }
    }

    for event in &non_layer_events {
      if matches!(event, MapEvent::Focus | MapEvent::Screenshot(_)) {
        Self::send_event(event).await;
      }
    }
  }

  async fn send_event(event: &MapEvent) {
    let _r = surf::post(format!("http://localhost:{DEFAULT_PORT}/"))
      .body_json(&event)
      .expect("cannot serialize json")
      .send()
      .await;
  }
}

impl MapSender {
  pub async fn new() -> (MapSender, bool) {
    let (rx, tx) = unbounded_channel();
    let sender = Self {
      sender: rx,
      inner_join_handle: SenderInner::start(tx),
    };
    let was_spawned = sender.spawn_mapvas_if_needed().await;

    (sender, was_spawned)
  }

  async fn spawn_mapvas_if_needed(&self) -> bool {
    if surf::get(format!("http://localhost:{DEFAULT_PORT}/healthcheck"))
      .send()
      .await
      .is_ok()
    {
      return false;
    }

    let _ = std::process::Command::new("mapvas")
      .stderr(Stdio::null())
      .stdout(Stdio::null())
      .spawn();
    while let Err(e) = surf::get(format!("http://localhost:{DEFAULT_PORT}/healthcheck",))
      .send()
      .await
    {
      debug!("Healthcheck {e}");
    }

    true
  }

  pub fn send_event(&self, event: MapEvent) {
    let _ = self.sender.send(Some(event));
  }

  pub async fn finalize(self) {
    let _ = self.sender.send(None);
    let _ = self.inner_join_handle.await;
  }
}
