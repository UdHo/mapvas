use log::{debug, error, warn};
use mapvas::map::map_event::{Layer, MapEvent};
use mapvas::remote::DEFAULT_PORT;
use std::process::Stdio;

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinSet;

pub struct MapSender {
  sender: UnboundedSender<Option<MapEvent>>,
  inner_join_handle: tokio::task::JoinHandle<()>,
}

struct SenderInner {
  receiver: UnboundedReceiver<Option<MapEvent>>,
  queue: VecDeque<MapEvent>,
  send_tasks: JoinSet<()>,
}

impl SenderInner {
  pub fn start(receiver: UnboundedReceiver<Option<MapEvent>>) -> tokio::task::JoinHandle<()> {
    tokio::spawn({
      Self {
        receiver,
        queue: VecDeque::new(),
        send_tasks: JoinSet::new(),
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
                 self.send_queue();
                 self.wait_for_completion().await;
                 return;
              }
            }
        _ = interval.tick() => {
            self.send_queue();
          },
      }
    }
  }

  fn receive(&mut self, event: MapEvent) {
    self.queue.push_back(event);
  }

  fn send_queue(&mut self) {
    let queue = std::mem::take(&mut self.queue);

    debug!("Sending queue:\n{queue:?}");

    self.send_tasks.spawn(async move {
      Self::compact_and_send(queue).await;
    });
  }

  async fn wait_for_completion(&mut self) {
    while self.send_tasks.join_next().await.is_some() {}
  }

  async fn compact_and_send(queue: VecDeque<MapEvent>) {
    let mut layers = BTreeMap::new();
    let mut non_layer_events = Vec::new();

    for event in queue {
      match event {
        MapEvent::Layer(Layer { id, mut geometries }) => {
          layers
            .entry(id)
            .and_modify(|e: &mut Vec<_>| e.append(&mut geometries))
            .or_insert(geometries);
        }
        e => non_layer_events.push(e),
      }
    }

    for event in &non_layer_events {
      if !matches!(event, MapEvent::Focus | MapEvent::Screenshot(_)) {
        Self::send_event(event).await;
      }
    }

    for (id, geometries) in layers {
      Self::send_event(&MapEvent::Layer(Layer { id, geometries })).await;
    }

    for event in &non_layer_events {
      if matches!(event, MapEvent::Focus | MapEvent::Screenshot(_)) {
        Self::send_event(event).await;
      }
    }
  }

  async fn send_event(event: &MapEvent) {
    let request = match surf::post(format!("http://localhost:{DEFAULT_PORT}/")).body_json(&event) {
      Ok(req) => req,
      Err(e) => {
        error!("Failed to serialize event to JSON: {e}");
        return;
      }
    };

    if let Err(e) = request.send().await {
      error!("Failed to send event to mapvas: {e}");
    }
  }
}

impl MapSender {
  pub async fn new() -> (MapSender, bool) {
    let (tx, rx) = unbounded_channel();
    let sender = Self {
      sender: tx,
      inner_join_handle: SenderInner::start(rx),
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

    if let Err(e) = std::process::Command::new("mapvas")
      .stderr(Stdio::null())
      .stdout(Stdio::null())
      .spawn()
    {
      error!("Failed to spawn mapvas process: {e}");
      return false;
    }

    while let Err(e) = surf::get(format!("http://localhost:{DEFAULT_PORT}/healthcheck",))
      .send()
      .await
    {
      debug!("Healthcheck {e}");
      tokio::time::sleep(Duration::from_millis(100)).await;
    }

    true
  }

  pub fn send_event(&self, event: MapEvent) {
    self.sender.send(Some(event)).inspect_err(|e| warn!("Failed to send event to queue: {e}")).ok();
  }

  pub async fn finalize(self) {
    self.sender.send(None).inspect_err(|e| warn!("Failed to send finalize signal: {e}")).ok();
    self.inner_join_handle.await.inspect_err(|e| error!("Sender task failed: {e}")).ok();
  }
}
