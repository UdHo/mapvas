use log::debug;
use mapvas::map::map_event::{Layer, Shape};
use mapvas::{MapEvent, DEFAULT_PORT};
use std::process::Stdio;

use async_std::task::block_on;
use std::collections::{HashMap, LinkedList};
use std::time::Duration;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

#[derive(Clone)]
pub struct MapSender {
  sender: UnboundedSender<MapEvent>,
}

struct SenderInner {
  receiver: UnboundedReceiver<MapEvent>,
  queue: LinkedList<MapEvent>,
}

impl SenderInner {
  pub fn start(receiver: UnboundedReceiver<MapEvent>) {
    tokio::spawn({
      Self {
        receiver,
        queue: LinkedList::new(),
      }
      .run()
    });
  }

  async fn run(mut self) {
    let mut interval = tokio::time::interval(Duration::from_millis(100));
    loop {
      tokio::select! {
       Some(event) = self.receiver.recv() => self.receive(event),
           _ = interval.tick() => self.send_queue().await,
      }
    }
  }

  fn receive(&mut self, event: MapEvent) {
    self.queue.push_back(event);
  }

  async fn send_queue(&mut self) {
    let mut queue = LinkedList::new();
    std::mem::swap(&mut queue, &mut self.queue);
    rayon::spawn(|| {
      block_on(Self::compact_and_send(queue));
    });
  }

  async fn compact_and_send(queue: LinkedList<MapEvent>) {
    let mut layers: HashMap<String, Vec<Shape>> = HashMap::new();

    for event in queue {
      match event {
        MapEvent::Layer(Layer { id, mut shapes }) => {
          layers
            .entry(id)
            .and_modify(|e| e.append(&mut shapes))
            .or_insert(shapes);
        }
        e => Self::send_event(&e).await,
      }
    }

    for (id, shapes) in layers {
      Self::send_event(&MapEvent::Layer(Layer { id, shapes })).await;
    }
  }
  async fn send_event(event: &MapEvent) {
    let _ = surf::post(format!("http://localhost:{DEFAULT_PORT}/"))
      .body_json(&event)
      .expect("cannot serialize json")
      .await;
  }
}

impl MapSender {
  pub async fn new() -> MapSender {
    let (rx, tx) = unbounded_channel();
    let sender = Self { sender: rx };

    sender.spawn_mapvas_if_needed().await;

    SenderInner::start(tx);

    sender
  }

  async fn spawn_mapvas_if_needed(&self) {
    if surf::get(format!("http://localhost:{DEFAULT_PORT}/healthcheck"))
      .send()
      .await
      .is_ok()
    {
      return;
    }

    let _ = std::process::Command::new("mapvas")
      .stderr(Stdio::null())
      .stdout(Stdio::null())
      .spawn();
    while let Err(e) = surf::get(format!("http://localhost:{DEFAULT_PORT}/healthcheck",))
      .send()
      .await
    {
      debug!("Healthcheck {}", e);
    }
  }

  pub fn send_event(&self, event: MapEvent) {
    let _ = self.sender.send(event);
  }
}
