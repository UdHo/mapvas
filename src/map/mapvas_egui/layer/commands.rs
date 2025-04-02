use std::{
  rc::Rc,
  sync::mpsc::{Receiver, Sender},
};

use external_cmd::ExternalCommand;
use log::error;
use serde::{Deserialize, Serialize};

use crate::map::coordinates::{BoundingBox, PixelCoordinate, PixelPosition, Transform};

use super::{Layer, LayerProperties, drawable::Drawable};

mod external_cmd;
mod ruler;

pub struct CommandLayer {
  commands: Vec<Box<dyn Command>>,
  layer_properties: LayerProperties,
  recv: Receiver<ParameterUpdate>,
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
      let mapvas_dir = config_dir.join("mapvas").join("curl_commands");
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
        .inspect_err(|e| error!("Cannot parse command {file:?}: {e}."))
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

const NAME: &str = "Command Layer";

trait Command {
  fn update_paramters(&mut self, parameters: ParameterUpdate);
  fn run(&mut self);
  fn result(&self) -> Option<Rc<dyn Drawable>>;
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
    // Update.
    self.recv.try_iter().for_each(|update| {
      for command in self
        .commands
        .iter_mut()
        .filter(|command| !command.is_locked())
      {
        command.update_paramters(update.clone());
      }
    });

    // Run.
    for command in &mut self.commands {
      command.run();
    }

    // Draw.
    if self.visible() {
      for command in self.commands.iter().filter(|command| command.is_visible()) {
        let drawable = command.result();
        if let Some(drawable) = drawable {
          drawable.draw(ui.painter(), transform);
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

  fn ui_content(&mut self, ui: &mut egui::Ui) {
    ui.collapsing("Commands", |ui| {
      for command in &mut self.commands {
        ui.collapsing(command.name().to_owned(), |ui| {
          visible_locking_ui(ui, command.as_mut());
          command.ui(ui);
        });
      }
    });
  }

  fn bounding_box(&self) -> Option<BoundingBox> {
    let bb = self
      .commands
      .iter()
      .filter(|command| command.is_visible())
      .map(|command| command.bounding_box())
      .fold(BoundingBox::default(), |acc, b| acc.extend(&b));

    bb.is_valid().then_some(bb)
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
