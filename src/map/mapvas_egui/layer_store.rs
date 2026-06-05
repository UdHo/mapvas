use std::{cell::RefCell, rc::Rc, sync::Mutex};

use super::layer::{EguiLayer, TimelineLayer};
use crate::map::layer::Layer as NeutralLayer;

#[derive(Clone)]
pub(super) struct MapLayerStore {
  layers: Rc<Mutex<Vec<Box<dyn EguiLayer>>>>,
}

impl MapLayerStore {
  pub(super) fn new(layers: Vec<Box<dyn EguiLayer>>) -> Self {
    Self {
      layers: Rc::new(Mutex::new(layers)),
    }
  }

  pub(super) fn for_each_neutral_mut(&self, mut f: impl FnMut(&mut dyn NeutralLayer)) {
    if let Ok(mut layers) = self.layers.lock() {
      for layer in layers.iter_mut() {
        f(layer.as_mut());
      }
    }
  }

  pub(super) fn fold_neutral<R>(
    &self,
    initial: R,
    mut f: impl FnMut(R, &dyn NeutralLayer) -> R,
  ) -> R {
    let Ok(layers) = self.layers.lock() else {
      return initial;
    };
    layers
      .iter()
      .fold(initial, |acc, layer| f(acc, layer.as_ref()))
  }

  pub(super) fn find_neutral<R>(
    &self,
    mut f: impl FnMut(&dyn NeutralLayer) -> Option<R>,
  ) -> Option<R> {
    let Ok(layers) = self.layers.lock() else {
      return None;
    };
    layers.iter().find_map(|layer| f(layer.as_ref()))
  }

  pub(super) fn find_neutral_mut<R>(
    &self,
    mut f: impl FnMut(&mut dyn NeutralLayer) -> Option<R>,
  ) -> Option<R> {
    let Ok(mut layers) = self.layers.lock() else {
      return None;
    };
    layers.iter_mut().find_map(|layer| f(layer.as_mut()))
  }

  pub(super) fn any_neutral(&self, mut predicate: impl FnMut(&dyn NeutralLayer) -> bool) -> bool {
    self
      .find_neutral(|layer| predicate(layer).then_some(()))
      .is_some()
  }

  pub(super) fn try_with_egui_layers_mut(
    &self,
    context: &str,
    f: impl FnOnce(&mut [Box<dyn EguiLayer>]),
  ) {
    let Ok(mut layers) = self.layers.try_lock().inspect_err(|e| {
      log::error!("Failed to lock layers for {context}: {e:?}");
    }) else {
      return;
    };
    f(layers.as_mut_slice());
  }

  pub(super) fn layer_egui_controls(&self, ui: &mut egui::Ui) {
    let mut layers = self
      .layers
      .lock()
      .unwrap_or_else(std::sync::PoisonError::into_inner);
    for layer in layers.iter_mut() {
      layer.ui_egui_controls(ui);
    }
  }
}

pub trait MapLayerHolder {
  /// Render egui sidebar controls for map layers.
  fn layer_egui_controls(&self, ui: &mut egui::Ui);
  /// Read temporal range from map layers without exposing the egui layer reader.
  fn layer_temporal_range(
    &self,
  ) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
  );
  /// Render the timeline's egui sidebar controls.
  fn timeline_egui_controls(&self, ui: &mut egui::Ui);
  /// Read temporal range from the timeline (used by the temporal-controls scan).
  fn timeline_temporal_range(
    &self,
  ) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
  );
}

pub(super) struct MapLayerHolderImpl {
  layers: MapLayerStore,
  timeline: Rc<RefCell<TimelineLayer>>,
}

impl MapLayerHolderImpl {
  pub(super) fn new(layers: MapLayerStore, timeline: Rc<RefCell<TimelineLayer>>) -> Self {
    Self { layers, timeline }
  }
}

impl MapLayerHolder for MapLayerHolderImpl {
  fn layer_egui_controls(&self, ui: &mut egui::Ui) {
    self.layers.layer_egui_controls(ui);
  }

  fn layer_temporal_range(
    &self,
  ) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
  ) {
    self
      .layers
      .fold_neutral((None, None), |(earliest, latest), layer| {
        let (layer_start, layer_end) = layer.get_temporal_range();
        (
          match (earliest, layer_start) {
            (Some(current), Some(layer_start)) => Some(current.min(layer_start)),
            (None, Some(layer_start)) => Some(layer_start),
            (current, None) => current,
          },
          match (latest, layer_end) {
            (Some(current), Some(layer_end)) => Some(current.max(layer_end)),
            (None, Some(layer_end)) => Some(layer_end),
            (current, None) => current,
          },
        )
      })
  }

  fn timeline_egui_controls(&self, ui: &mut egui::Ui) {
    self.timeline.borrow_mut().ui_egui_controls(ui);
  }

  fn timeline_temporal_range(
    &self,
  ) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
  ) {
    self.timeline.borrow().get_temporal_range()
  }
}
