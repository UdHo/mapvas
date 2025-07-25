use std::{
  rc::Rc,
  sync::mpsc::{Receiver, Sender},
};

use external_cmd::ExternalCommand;
use log::error;
use serde::{Deserialize, Serialize};

use crate::map::coordinates::{BoundingBox, PixelCoordinate, PixelPosition, Transform};
use egui::Pos2;

use super::{Layer, LayerProperties, drawable::Drawable};

mod external_cmd;
mod ruler;

pub struct CommandLayer {
  commands: Vec<Box<dyn Command>>,
  layer_properties: LayerProperties,
  recv: Receiver<ParameterUpdate>,
  highlighted_command: Option<usize>,
  just_highlighted: bool,
}

impl CommandLayer {
  pub fn new() -> (Self, Sender<ParameterUpdate>) {
    let (send, recv) = std::sync::mpsc::channel();

    let config_commands = Self::from_config()
      .into_iter()
      .flatten()
      .collect::<Vec<_>>();

    let mut commands: Vec<Box<dyn Command>> = vec![Box::new(ruler::Ruler::default())];
    commands.extend(config_commands);

    (
      Self {
        commands,
        layer_properties: LayerProperties::default(),
        recv,
        highlighted_command: None,
        just_highlighted: false,
      },
      send,
    )
  }

  pub fn register_keys(&self) -> Box<dyn Iterator<Item = &str> + '_> {
    Box::new(
      self
        .commands
        .iter()
        .flat_map(|command| command.register_keys()),
    )
  }

  fn from_config() -> Option<impl Iterator<Item = Box<dyn Command>>> {
    let config_dir = dirs::home_dir().map(|d| d.join(".config"));
    if let Some(config_dir) = config_dir {
      let mapvas_dir = config_dir.join("mapvas").join("commands");
      let x = mapvas_dir
        .read_dir()
        .ok()
        .into_iter()
        .flat_map(|dir| dir.map(|f| Some(f.ok()?.file_name())))
        .flatten()
        .map(std::ffi::OsString::into_string)
        .filter_map(move |s| {
          if let Ok(s) = s {
            let p = std::path::Path::new(&s);
            p.extension()
              .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
              .then(|| mapvas_dir.clone().join(p))
          } else {
            None
          }
        })
        .filter_map(|file| Self::file_to_command(&file));
      return Some(Box::new(x));
    }
    None
  }

  fn file_to_command(file: &std::path::PathBuf) -> Option<Box<dyn Command>> {
    let content = std::fs::read_to_string(file);
    if let Ok(content) = content {
      let command: ExternalCommand = serde_json::from_str(&content)
        .inspect_err(|e| error!("Cannot parse command {}: {e}.", file.display()))
        .ok()?;
      return Some(Box::new(command));
    }
    None
  }
}

#[derive(Clone)]
pub enum ParameterUpdate {
  Update(String, Option<PixelCoordinate>),
  DragUpdate(PixelPosition, PixelPosition, Transform),
}

fn truncate_label(label: &str, max_len: usize) -> String {
  if label.len() > max_len {
    format!("{}...", &label[..max_len])
  } else {
    label.to_string()
  }
}

const NAME: &str = "Command Layer";

trait Command {
  fn update_paramters(&mut self, parameters: ParameterUpdate);
  fn run(&mut self);
  fn result(&self) -> Box<dyn Iterator<Item = Rc<dyn Drawable>>>;
  fn is_locked(&self) -> bool;
  fn is_visible(&self) -> bool;
  fn locked(&mut self) -> &mut bool;
  fn visible(&mut self) -> &mut bool;
  fn register_keys(&self) -> Box<dyn Iterator<Item = &str> + '_>;
  fn ui(&mut self, ui: &mut egui::Ui);
  fn name(&self) -> &str;
  fn bounding_box(&self) -> BoundingBox;
}

impl Layer for CommandLayer {
  fn draw(
    &mut self,
    ui: &mut egui::Ui,
    transform: &crate::map::coordinates::Transform,
    _rect: egui::Rect,
  ) {
    self.recv.try_iter().for_each(|update| {
      for command in self
        .commands
        .iter_mut()
        .filter(|command| !command.is_locked())
      {
        command.update_paramters(update.clone());
      }
    });

    for command in &mut self.commands {
      command.run();
    }

    if self.visible() {
      for command in self.commands.iter().filter(|command| command.is_visible()) {
        let drawable = command.result();
        for d in drawable {
          d.draw(ui.painter(), transform);
        }
      }
    }
  }

  fn name(&self) -> &str {
    NAME
  }

  fn visible(&self) -> bool {
    self.layer_properties.visible
  }

  fn visible_mut(&mut self) -> &mut bool {
    &mut self.layer_properties.visible
  }

  fn ui(&mut self, ui: &mut egui::Ui) {
    let has_highlighted_command = self.highlighted_command.is_some();
    let layer_id = egui::Id::new("command_layer_header");

    let mut layer_header = egui::CollapsingHeader::new(self.name().to_owned())
      .id_salt(layer_id)
      .default_open(has_highlighted_command);

    // Only force open once when just highlighted, then let user control it
    if self.just_highlighted && has_highlighted_command {
      layer_header = layer_header.open(Some(true));
    }

    layer_header.show(ui, |ui| {
      ui.checkbox(self.visible_mut(), "visible");
      self.ui_content(ui);
    });
  }

  fn ui_content(&mut self, ui: &mut egui::Ui) {
    let has_highlighted_command = self.highlighted_command.is_some();
    let commands_header_id = egui::Id::new("commands_header");

    let mut commands_header = egui::CollapsingHeader::new("Commands")
      .id_salt(commands_header_id)
      .default_open(has_highlighted_command);

    // Only force open once when just highlighted
    if self.just_highlighted && has_highlighted_command {
      commands_header = commands_header.open(Some(true));
    }

    commands_header.show(ui, |ui| {
      for (cmd_idx, command) in self.commands.iter_mut().enumerate() {
        let is_highlighted = self.highlighted_command == Some(cmd_idx);

        let bg_color = if is_highlighted {
          Some(egui::Color32::from_rgb(100, 149, 237))
        } else {
          None
        };

        let frame = if let Some(color) = bg_color {
          egui::Frame::default()
            .fill(color)
            .corner_radius(egui::CornerRadius::same(4))
            .inner_margin(egui::Margin::same(4))
        } else {
          egui::Frame::default()
        };

        frame.show(ui, |ui| {
          let command_header_id = egui::Id::new(format!("command_header_{cmd_idx}"));

          let mut command_header = egui::CollapsingHeader::new(truncate_label(command.name(), 40))
            .id_salt(command_header_id)
            .default_open(is_highlighted);

          // Only force open once when just highlighted
          if is_highlighted && self.just_highlighted {
            command_header = command_header.open(Some(true));
          }

          command_header.show(ui, |ui| {
            visible_locking_ui(ui, command.as_mut());
            command.ui(ui);
          });
        });
      }
    });

    self.just_highlighted = false;
  }

  fn bounding_box(&self) -> Option<BoundingBox> {
    let bb = self
      .commands
      .iter()
      .filter(|command| command.is_visible())
      .map(|command| command.bounding_box())
      .filter(BoundingBox::is_valid)
      .fold(BoundingBox::default(), |acc, b| acc.extend(&b));

    bb.is_valid().then_some(bb)
  }

  fn has_highlighted_geometry(&self) -> bool {
    self.highlighted_command.is_some()
  }

  fn closest_geometry_with_selection(&mut self, pos: Pos2, transform: &Transform) -> Option<f64> {
    let click_coord = transform.invert().apply(pos.into());
    let tolerance_map_coords = f64::from(5.0 / transform.zoom);
    let mut closest_distance = f64::INFINITY;
    let mut found_command: Option<usize> = None;

    for (cmd_idx, command) in self.commands.iter().enumerate() {
      if !command.is_visible() {
        continue;
      }

      if let Some(distance) = Self::calculate_distance_to_command(&**command, click_coord) {
        if distance < closest_distance && distance < tolerance_map_coords {
          closest_distance = distance;
          found_command = Some(cmd_idx);
        }
      }
    }

    if let Some(cmd_idx) = found_command {
      let was_different = self.highlighted_command != Some(cmd_idx);
      self.highlighted_command = Some(cmd_idx);
      self.just_highlighted = was_different;
      Some(closest_distance)
    } else {
      self.highlighted_command = None;
      self.just_highlighted = false;
      None
    }
  }
}

impl CommandLayer {
  fn calculate_distance_to_command(
    command: &dyn Command,
    click_coord: PixelCoordinate,
  ) -> Option<f64> {
    let mut min_distance: Option<f64> = None;
    for drawable in command.result() {
      if let Some(drawable_distance) = drawable.distance_to_point(click_coord) {
        min_distance = match min_distance {
          None => Some(drawable_distance),
          Some(current_min) => Some(drawable_distance.min(current_min)),
        };
      }
    }

    min_distance
  }
}

fn visible_locking_ui(ui: &mut egui::Ui, command: &mut dyn Command) {
  ui.horizontal(|ui| {
    ui.checkbox(command.visible(), "Visible");
    ui.checkbox(command.locked(), "Locked");
  });
}

/// Updates the closest point to the mouse position when dragging.
pub fn update_closest(
  pos: PixelPosition,
  trans: Transform,
  delta: PixelPosition,
  coords: &mut Vec<&mut PixelCoordinate>,
) -> bool {
  let mut closest = None;
  let mut min_dist = f32::MAX;
  for (i, coord) in coords.iter().enumerate() {
    let pp = trans.apply(**coord) + delta;

    let dist = (pp.x - pos.x).abs() + (pp.y - pos.y).abs();
    if dist < min_dist {
      min_dist = dist;
      closest = Some(i);
    }
  }
  if min_dist > 30. {
    return false;
  }
  if let Some(closest) = closest {
    *coords[closest] = trans.invert().apply(pos);
    return true;
  }
  false
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OoMCoordinates {
  Coordinate(PixelCoordinate),
  Coordinates(Vec<PixelCoordinate>),
}
