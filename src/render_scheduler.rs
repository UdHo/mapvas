use std::{
  collections::{BTreeMap, HashMap},
  sync::{Arc, LazyLock, Mutex},
};

use crate::map::coordinates::TilePriority;

pub static RENDER_SCHEDULER: LazyLock<RenderScheduler> = LazyLock::new(|| {
  RenderScheduler::new(crate::render_pool::RENDER_POOL.current_num_threads())
});

/// Priority-aware scheduler that feeds CPU-bound render tasks into `RENDER_POOL`.
///
/// Tasks are queued and dispatched in priority order (Current before Adjacent before `ZoomLevel`).
/// Up to `max_concurrent` tasks run simultaneously — one per rayon thread. While a task is still
/// queued (not yet executing), its priority can be raised via [`TaskHandle::bump`].
#[derive(Clone)]
pub struct RenderScheduler {
  inner: Arc<Mutex<Inner>>,
}

struct Inner {
  /// Key: (`priority_value`, `task_id`). `BTreeMap` ascending order = lowest `priority_value` first
  /// = `TilePriority::Current` (0) before Adjacent (1) before `ZoomLevel` (2).
  queue: BTreeMap<(u8, u64), Box<dyn FnOnce() + Send + 'static>>,
  /// Maps `task_id` -> current `priority_value`, for O(log n) bump lookup.
  priority_by_id: HashMap<u64, u8>,
  next_id: u64,
  active: usize,
  max_concurrent: usize,
}

/// Returned by [`RenderScheduler::submit`]. Allows raising a queued task's priority.
pub struct TaskHandle {
  id: u64,
  inner: Arc<Mutex<Inner>>,
}

impl RenderScheduler {
  fn new(max_concurrent: usize) -> Self {
    Self {
      inner: Arc::new(Mutex::new(Inner {
        queue: BTreeMap::new(),
        priority_by_id: HashMap::new(),
        next_id: 0,
        active: 0,
        max_concurrent,
      })),
    }
  }

  /// Enqueue a CPU-bound task at the given priority.
  ///
  /// Returns a oneshot receiver that resolves when the task completes, and a handle
  /// that can be used to raise the task's priority while it is still queued.
  pub fn submit<F, R>(
    &self,
    priority: TilePriority,
    f: F,
  ) -> (tokio::sync::oneshot::Receiver<R>, TaskHandle)
  where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
  {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let boxed: Box<dyn FnOnce() + Send + 'static> = Box::new(move || {
      let _ = tx.send(f());
    });

    let id = {
      let mut guard = self.inner.lock().unwrap();
      let id = guard.next_id;
      guard.next_id = guard.next_id.wrapping_add(1);
      let prio = priority as u8;
      guard.queue.insert((prio, id), boxed);
      guard.priority_by_id.insert(id, prio);
      id
    };
    dispatch_pending(&self.inner);

    (rx, TaskHandle { id, inner: Arc::clone(&self.inner) })
  }
}

impl TaskHandle {
  /// Raise this task's priority. No-op if the task is already executing or already
  /// at an equal or higher priority than `new_priority`.
  pub fn bump(&self, new_priority: TilePriority) {
    let new_prio = new_priority as u8;
    let mut guard = self.inner.lock().unwrap();
    if let Some(&old_prio) = guard.priority_by_id.get(&self.id) {
      if new_prio >= old_prio {
        return; // already at least as important
      }
      if let Some(task) = guard.queue.remove(&(old_prio, self.id)) {
        guard.queue.insert((new_prio, self.id), task);
        guard.priority_by_id.insert(self.id, new_prio);
      }
      // If not in queue the task is already executing — nothing to do.
    }
  }
}

/// Try to fill available rayon slots from the queue. Called after every submit and
/// after every task completion.
fn dispatch_pending(inner: &Arc<Mutex<Inner>>) {
  loop {
    let task = {
      let mut guard = inner.lock().unwrap();
      if guard.active >= guard.max_concurrent || guard.queue.is_empty() {
        return;
      }
      let (&key, _) = guard.queue.iter().next().unwrap();
      let task = guard.queue.remove(&key).unwrap();
      guard.priority_by_id.remove(&key.1);
      guard.active += 1;
      task
    };
    let inner_clone = Arc::clone(inner);
    crate::render_pool::RENDER_POOL.spawn(move || {
      task();
      inner_clone.lock().unwrap().active -= 1;
      dispatch_pending(&inner_clone);
    });
  }
}
