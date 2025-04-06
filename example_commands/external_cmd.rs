use std::{
  io::BufRead,
  rc::Rc,
  sync::mpsc::{Receiver, Sender},
};

use itertools::Either;
use log::error;
use serde::{Deserialize, Serialize};

use crate::{
  map::{
    coordinates::{BoundingBox, Coordinate},
    geometry_collection::{Geometry, Metadata},
    map_event::{Layer as EventLayer, MapEvent},
    mapvas_egui::layer::drawable::Drawable,
  },
  parser::{FileParser as _, Parsers},
};

use super::{Command, OoMCoordinates, ParameterUpdate, update_closest};

#[derive(Serialize, Deserialize)]
struct ExeCfg {
  name: String,
  executable: String,
  args_templates: Vec<String>,
  coordinates: Vec<(String, OoMCoordinates)>,
  parser: Parsers,
  coordinate_template: String,
  coordinates_template: String,
}

impl ExeCfg {
  fn update_paramters(&mut self, parameters: ParameterUpdate) -> bool {
    match parameters {
      ParameterUpdate::Update(key, coord) => {
        if let Some(coord) = coord {
          if let Some(c) = self
            .coordinates
            .iter_mut()
            .filter(|(k, _)| k == &key)
            .map(|(_, coord)| coord)
            .next()
          {
            match c {
              OoMCoordinates::Coordinate(c) => {
                *c = coord;
              }
              OoMCoordinates::Coordinates(c) => {
                c.push(coord);
              }
            }
            return true;
          }
        }
      }
      ParameterUpdate::DragUpdate(pos, delta, trans) => {
        let mut coords = vec![];
        for coord in self.coordinates.iter_mut().map(|(_, coord)| coord) {
          match coord {
            OoMCoordinates::Coordinate(c) => coords.push(c),
            OoMCoordinates::Coordinates(c) => {
              for coord in c.iter_mut() {
                coords.push(coord);
              }
            }
          }
        }
        return update_closest(pos, trans, delta, &mut coords);
      }
    }
    false
  }

  fn keys(&self) -> Box<dyn Iterator<Item = &str> + '_> {
    Box::new(
      self
        .coordinates
        .iter()
        .map(|(x, _)| x)
        .map(std::string::String::as_str),
    )
  }

  fn run(&self, sender: Sender<MapEvent>) {
    let mut cmd = self.executable.clone();
    let mut args = self.args_templates.clone();
    for (key, coord) in &self.coordinates {
      let coord_str = match coord {
        OoMCoordinates::Coordinate(c) => {
          let coord = c.as_wgs84();
          self
            .coordinate_template
            .replace("{lat}", &coord.lat.to_string())
            .replace("{lon}", &coord.lon.to_string())
        }
        OoMCoordinates::Coordinates(coords) => {
          let coords_string: String = coords
            .iter()
            .map(Coordinate::as_wgs84)
            .map(|c| {
              self
                .coordinates_template
                .replace("{lat}", &c.lat.to_string())
                .replace("{lon}", &c.lon.to_string())
            })
            .collect();
          coords_string
        }
      };

      cmd = cmd.replace(&format!("{{{key}}}"), &coord_str);
      for arg in &mut args {
        *arg = arg.replace(&format!("{{{key}}}"), &coord_str);
      }
    }

    let mut parser = self.parser.clone();

    tokio::spawn(async move {
      let response = std::process::Command::new(cmd).args(args).output();

      if let Ok(response) = response {
        let cursor = std::io::Cursor::new(response.stdout);
        let buf_read: Box<dyn BufRead> = Box::new(cursor);
        let parsed = parser.parse(buf_read);
        for el in parsed {
          let _ = sender.send(el);
        }
      }
    });
  }

  fn ui(&mut self, ui: &mut egui::Ui) {
    ui.label(format!("Exe: {}", self.executable));
  }

  fn bounding_box(&self) -> BoundingBox {
    BoundingBox::from_iterator(self.coordinates.iter().flat_map(|(_, coord)| match coord {
      OoMCoordinates::Coordinate(c) => Either::Left(std::iter::once(*c)),
      OoMCoordinates::Coordinates(c) => Either::Right(c.iter().copied()),
    }))
  }

  fn name(&self) -> &str {
    self.name.as_str()
  }
}

#[derive(Serialize, Deserialize)]
struct CurlCfg {
  name: String,
  url_template: String,
  coordinates: Vec<(String, OoMCoordinates)>,
  parser: Parsers,
  coordinate_template: String,
  coordinates_template: String,
  #[serde(skip_serializing_if = "Option::is_none", default)]
  post_data_template: Option<String>,
}

impl CurlCfg {
  fn update_paramters(&mut self, parameters: ParameterUpdate) -> bool {
    match parameters {
      ParameterUpdate::Update(key, coord) => {
        if let Some(coord) = coord {
          if let Some(c) = self
            .coordinates
            .iter_mut()
            .filter(|(k, _)| k == &key)
            .map(|(_, coord)| coord)
            .next()
          {
            match c {
              OoMCoordinates::Coordinate(c) => {
                *c = coord;
              }
              OoMCoordinates::Coordinates(c) => {
                c.push(coord);
              }
            }
            return true;
          }
        }
      }
      ParameterUpdate::DragUpdate(pos, delta, trans) => {
        let mut coords = vec![];
        for coord in self.coordinates.iter_mut().map(|(_, coord)| coord) {
          match coord {
            OoMCoordinates::Coordinate(c) => coords.push(c),
            OoMCoordinates::Coordinates(c) => {
              for coord in c.iter_mut() {
                coords.push(coord);
              }
            }
          }
        }
        return update_closest(pos, trans, delta, &mut coords);
      }
    }
    false
  }

  fn keys(&self) -> Box<dyn Iterator<Item = &str> + '_> {
    Box::new(
      self
        .coordinates
        .iter()
        .map(|(x, _)| x)
        .map(std::string::String::as_str),
    )
  }

  fn run(&self, sender: Sender<MapEvent>) {
    let mut url = self.url_template.clone();
    for (key, coord) in &self.coordinates {
      match coord {
        OoMCoordinates::Coordinate(c) => {
          let coord = c.as_wgs84();
          let coord_str = self
            .coordinate_template
            .replace("{lat}", &coord.lat.to_string())
            .replace("{lon}", &coord.lon.to_string());
          url = url.replace(&format!("{{{key}}}"), &coord_str);
        }
        OoMCoordinates::Coordinates(coords) => {
          let coords_string: String = coords
            .iter()
            .map(Coordinate::as_wgs84)
            .map(|c| {
              self
                .coordinates_template
                .replace("{lat}", &c.lat.to_string())
                .replace("{lon}", &c.lon.to_string())
            })
            .collect();
          url = url.replace(&format!("{{{key}}}"), &coords_string);
        }
      }
    }

    let mut parser = self.parser.clone();

    tokio::spawn(async move {
      let response = surf::get(url)
        .recv_string()
        .await
        .inspect_err(|e| error!("Could not fetch data: {e}"));
      if let Ok(response) = dbg!(response) {
        let cursor = std::io::Cursor::new(response.into_bytes());
        let buf_read: Box<dyn BufRead> = Box::new(cursor);
        let parsed = parser.parse(buf_read);
        for el in parsed {
          let _ = sender.send(el);
        }
      }
    });
  }

  fn ui(&mut self, ui: &mut egui::Ui) {
    ui.label("Curl Command");
    ui.text_edit_singleline(&mut self.url_template);
    if let Some(pdt) = &mut self.post_data_template {
      ui.text_edit_singleline(pdt);
    }
    ui.text_edit_singleline(&mut self.coordinate_template);
  }

  fn bounding_box(&self) -> BoundingBox {
    BoundingBox::from_iterator(self.coordinates.iter().flat_map(|(_, coord)| match coord {
      OoMCoordinates::Coordinate(c) => Either::Left(std::iter::once(*c)),
      OoMCoordinates::Coordinates(c) => Either::Right(c.iter().copied()),
    }))
  }

  fn name(&self) -> &str {
    self.name.as_str()
  }
}

#[derive(Serialize, Deserialize)]
enum ExCommand {
  Curl(CurlCfg),
  Exe(ExeCfg),
}

impl ExCommand {
  fn bounding_box(&self) -> BoundingBox {
    match self {
      ExCommand::Curl(curl) => curl.bounding_box(),
      ExCommand::Exe(exe) => exe.bounding_box(),
    }
  }

  fn coordinates(&self) -> &[(String, OoMCoordinates)] {
    match self {
      ExCommand::Curl(curl) => &curl.coordinates,
      ExCommand::Exe(exe) => &exe.coordinates,
    }
  }

  fn run(&self, sender: Sender<MapEvent>) -> bool {
    for coord in self.coordinates().iter().map(|(_, coord)| coord) {
      if let OoMCoordinates::Coordinate(c) = coord {
        if !c.is_valid() {
          return false;
        }
      }
    }
    match self {
      ExCommand::Curl(curl) => curl.run(sender),
      ExCommand::Exe(exe) => exe.run(sender),
    }
    true
  }
  fn update(&mut self, parameters: ParameterUpdate) -> bool {
    match self {
      ExCommand::Curl(curl) => curl.update_paramters(parameters),
      ExCommand::Exe(exe) => exe.update_paramters(parameters),
    }
  }

  fn keys(&self) -> Box<dyn Iterator<Item = &str> + '_> {
    match self {
      ExCommand::Curl(curl) => curl.keys(),
      ExCommand::Exe(exe) => exe.keys(),
    }
  }

  fn ui(&mut self, ui: &mut egui::Ui) {
    match self {
      ExCommand::Curl(curl) => curl.ui(ui),
      ExCommand::Exe(exe) => exe.ui(ui),
    }
  }

  fn name(&self) -> &str {
    match self {
      ExCommand::Curl(curl) => curl.name(),
      ExCommand::Exe(exe) => exe.name(),
    }
  }
}

struct ExternalCommandCommon {
  locked: bool,
  visible: bool,
  send: Sender<MapEvent>,
  rcv: Receiver<MapEvent>,
  last_request: std::time::Instant,
  last_update: std::time::Instant,
  result: Option<Rc<dyn Drawable>>,
}

impl ExternalCommandCommon {
  fn new() -> Self {
    let (send, rcv) = std::sync::mpsc::channel();
    Self {
      locked: false,
      visible: true,
      send,
      rcv,
      last_request: std::time::Instant::now(),
      last_update: std::time::Instant::now(),
      result: None,
    }
  }
}

#[derive(Serialize, Deserialize)]
pub struct ExternalCommand {
  #[serde(skip, default = "ExternalCommandCommon::new")]
  common: ExternalCommandCommon,
  cmd: ExCommand,
}

impl ExternalCommand {
  #[cfg(test)]
  fn new(cmd: ExCommand) -> Self {
    Self {
      common: ExternalCommandCommon::new(),
      cmd,
    }
  }
}

impl Command for ExternalCommand {
  fn update_paramters(&mut self, parameters: ParameterUpdate) {
    if self.cmd.update(parameters) {
      self.common.last_update = std::time::Instant::now();
    }
  }

  fn run(&mut self) {
    for el in self.common.rcv.try_iter() {
      if let MapEvent::Layer(EventLayer { id: _, geometries }) = el {
        if geometries.len() == 1 {
          self.common.result = Some(Rc::new(geometries[0].clone()));
        }
        if geometries.len() > 1 {
          self.common.result = Some(Rc::new(Geometry::GeometryCollection(
            geometries,
            Metadata::default(),
          )));
        }
      }
    }
    if self.common.last_request > self.common.last_update
      || self.common.last_request.elapsed().as_millis() < 1_000
    {
      return;
    }

    if self.cmd.run(self.common.send.clone()) {
      self.common.last_request = std::time::Instant::now();
    }
  }

  fn result(&self) -> Option<Rc<dyn Drawable>> {
    self.common.result.clone()
  }

  fn is_locked(&self) -> bool {
    self.common.locked
  }

  fn is_visible(&self) -> bool {
    self.common.visible
  }

  fn locked(&mut self) -> &mut bool {
    &mut self.common.locked
  }

  fn visible(&mut self) -> &mut bool {
    &mut self.common.visible
  }

  fn register_keys(&self) -> Box<dyn Iterator<Item = &str> + '_> {
    self.cmd.keys()
  }

  fn ui(&mut self, ui: &mut egui::Ui) {
    self.cmd.ui(ui);
  }

  fn name(&self) -> &str {
    self.cmd.name()
  }

  fn bounding_box(&self) -> BoundingBox {
    self.cmd.bounding_box()
  }
}

#[cfg(test)]
mod tests {

  use egui::Color32;

  use crate::{
    map::{
      coordinates::PixelCoordinate,
      map_event::Color,
      mapvas_egui::layer::commands::{
        OoMCoordinates,
        external_cmd::{ExCommand, ExternalCommand},
      },
    },
    parser::{Parsers, TTJsonParser},
  };

  #[test]
  fn test_serialize_curl() {
    let curl = super::CurlCfg {
      name: "TomTom Route".to_string(),
      url_template:
        "https://api.tomtom.com/routing/1/calculateRoute/{origin}:{destination}/json?key=<key>"
          .to_string(),
      post_data_template: None,
      coordinates: [
        (
          "origin".to_string(),
          OoMCoordinates::Coordinate(PixelCoordinate {
            x: f32::MAX,
            y: f32::MAX,
          }),
        ),
        (
          "destination".to_string(),
          OoMCoordinates::Coordinate(PixelCoordinate {
            x: f32::MAX,
            y: f32::MAX,
          }),
        ),
        ("via".to_string(), OoMCoordinates::Coordinates(vec![])),
      ]
      .into(),
      coordinate_template: "{lat},{lon}".to_string(),
      coordinates_template: "{lat},{lon}:".to_string(),
      parser: Parsers::TTJson(TTJsonParser::new().with_color(Color(Color32::DARK_GREEN))),
    };

    let cmd = ExternalCommand::new(ExCommand::Curl(curl));

    let serialized = serde_json::to_string_pretty(&cmd).unwrap();
    println!("{serialized}");
    let exp = r#"{
  "cmd": {
    "Curl": {
      "name": "TomTom Route",
      "url_template": "https://api.tomtom.com/routing/1/calculateRoute/{origin}:{destination}/json?key=<key>",
      "coordinates": [
        [
          "origin",
          {
            "Coordinate": {
              "x": 3.4028235e38,
              "y": 3.4028235e38
            }
          }
        ],
        [
          "destination",
          {
            "Coordinate": {
              "x": 3.4028235e38,
              "y": 3.4028235e38
            }
          }
        ],
        [
          "via",
          {
            "Coordinates": []
          }
        ]
      ],
      "parser": {
        "TTJson": {
          "color": [
            0,
            100,
            0,
            255
          ]
        }
      },
      "coordinate_template": "{lat},{lon}",
      "coordinates_template": "{lat},{lon}:"
    }
  }
}"#;
    assert_eq!(serialized, exp);
  }

  #[test]
  fn test_serialize_exe() {
    let executable = super::ExeCfg {
      name: "Echo".to_string(),
      coordinates: [
        (
          "origin".to_string(),
          OoMCoordinates::Coordinate(PixelCoordinate {
            x: f32::MAX,
            y: f32::MAX,
          }),
        ),
        (
          "destination".to_string(),
          OoMCoordinates::Coordinate(PixelCoordinate {
            x: f32::MAX,
            y: f32::MAX,
          }),
        ),
        ("via".to_string(), OoMCoordinates::Coordinates(vec![])),
      ]
      .into(),
      coordinate_template: "{lat},{lon}".to_string(),
      coordinates_template: "{lat},{lon}:".to_string(),
      parser: Parsers::Grep(
        crate::parser::GrepParser::new(false).with_color(Color(Color32::DARK_GREEN)),
      ),
      executable: "echo".to_string(),
      args_templates: vec!["{origin}\n {destination}\n {via}".to_string()],
    };

    let cmd = ExternalCommand::new(ExCommand::Exe(executable));

    let serialized = serde_json::to_string_pretty(&cmd).unwrap();
    println!("{serialized}");
    let exp = r#"{
  "cmd": {
    "Exe": {
      "name": "Echo",
      "executable": "echo",
      "args_templates": [
        "{origin}\n {destination}\n {via}"
      ],
      "coordinates": [
        [
          "origin",
          {
            "Coordinate": {
              "x": 3.4028235e38,
              "y": 3.4028235e38
            }
          }
        ],
        [
          "destination",
          {
            "Coordinate": {
              "x": 3.4028235e38,
              "y": 3.4028235e38
            }
          }
        ],
        [
          "via",
          {
            "Coordinates": []
          }
        ]
      ],
      "parser": {
        "Grep": {
          "invert_coordinates": false,
          "color": [
            0,
            100,
            0,
            255
          ],
          "fill": "NoFill"
        }
      },
      "coordinate_template": "{lat},{lon}",
      "coordinates_template": "{lat},{lon}:"
    }
  }
}"#;
    assert_eq!(serialized, exp);
  }
}
