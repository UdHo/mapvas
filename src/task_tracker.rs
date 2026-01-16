//! Task tracking for displaying active tokio tasks

use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::time::Instant;

/// Information about a tracked task
#[derive(Clone, Debug)]
pub struct TaskInfo {
  pub name: String,
  pub started_at: Instant,
  pub category: TaskCategory,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TaskCategory {
  TileLoad,
  TileSuperRes,
  Server,
  Search,
  External,
  Other,
}

impl TaskInfo {
  #[must_use]
  pub fn elapsed(&self) -> std::time::Duration {
    self.started_at.elapsed()
  }
}

/// Global task tracker
pub struct TaskTracker {
  tasks: Mutex<HashMap<u64, TaskInfo>>,
  next_id: Mutex<u64>,
}

impl TaskTracker {
  fn new() -> Self {
    Self {
      tasks: Mutex::new(HashMap::new()),
      next_id: Mutex::new(0),
    }
  }

  /// Register a new task and return its ID
  pub fn register(&self, name: String, category: TaskCategory) -> u64 {
    let mut next_id = self.next_id.lock().unwrap();
    let id = *next_id;
    *next_id += 1;
    drop(next_id);

    let info = TaskInfo {
      name,
      started_at: Instant::now(),
      category,
    };

    self.tasks.lock().unwrap().insert(id, info);
    id
  }

  /// Unregister a task when it completes
  pub fn unregister(&self, id: u64) {
    self.tasks.lock().unwrap().remove(&id);
  }

  /// Get a snapshot of all active tasks
  pub fn snapshot(&self) -> Vec<(u64, TaskInfo)> {
    self.tasks
      .lock()
      .unwrap()
      .iter()
      .map(|(id, info)| (*id, info.clone()))
      .collect()
  }

  /// Get count of tasks by category
  pub fn count_by_category(&self, category: &TaskCategory) -> usize {
    self.tasks
      .lock()
      .unwrap()
      .values()
      .filter(|info| &info.category == category)
      .count()
  }
}

/// Global task tracker instance
static TASK_TRACKER: std::sync::OnceLock<Arc<TaskTracker>> = std::sync::OnceLock::new();

/// Get the global task tracker
pub fn task_tracker() -> Arc<TaskTracker> {
  TASK_TRACKER
    .get_or_init(|| Arc::new(TaskTracker::new()))
    .clone()
}

/// RAII guard that automatically unregisters a task when dropped
pub struct TaskGuard {
  id: u64,
  tracker: Arc<TaskTracker>,
}

impl TaskGuard {
  #[must_use]
  pub fn new(name: String, category: TaskCategory) -> Self {
    let tracker = task_tracker();
    let id = tracker.register(name, category);
    Self { id, tracker }
  }
}

impl Drop for TaskGuard {
  fn drop(&mut self) {
    self.tracker.unregister(self.id);
  }
}
