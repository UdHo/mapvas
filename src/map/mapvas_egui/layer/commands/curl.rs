use std::{
  collections::HashMap,
  io::BufRead,
  rc::Rc,
  sync::mpsc::{Receiver, Sender},
};

use itertools::Either;
use log::error;
use serde::{Deserialize, Serialize};

use crate::{
  map::{
    coordinates::Coordinate as _,
    geometry_collection::{Geometry, Metadata},
    map_event::{Layer as EventLayer, MapEvent},
    mapvas_egui::layer::drawable::Drawable,
  },
  parser::{FileParser as _, Parsers},
};

use super::{Command, OoMCoordinates, ParameterUpdate, update_closest};

#[derive(Serialize, Deserialize)]
struct CurlCfg {
  name: String,
  url_template: String,
  #[serde(skip_serializing_if = "Option::is_none", default)]
  post_data_template: Option<String>,
  coordinates: HashMap<String, OoMCoordinates>,
  parser: Parsers,
  coordinate_template: String,
}

impl CurlCfg {}

enum ExternalCommand {
  Curl(CurlCommand),
}

impl ExternalCommand {
  fn prepare_request(&mut self, external_command_common: &mut ExternalCommandCommon) {}
  fn run(&mut self, external_command_common: &mut ExternalCommandCommon) {}
  fn update(&mut self, external_command_common: &mut ExternalCommandCommon) {}
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

#[derive(Serialize, Deserialize)]
pub struct CurlCommand {
  pub name: String,
  pub url_template: String,
  #[serde(skip_serializing_if = "Option::is_none", default)]
  pub post_data_template: Option<String>,
  pub coordinates: HashMap<String, OoMCoordinates>,
  pub coordinate_template: String,
  pub locked: bool,
  pub visible: bool,
  pub parser: Parsers,
  #[serde(skip)]
  pub send_rcv: Option<(Sender<MapEvent>, Receiver<MapEvent>)>,
  #[serde(skip, default = "std::time::Instant::now")]
  last_request: std::time::Instant,
  #[serde(skip, default = "std::time::Instant::now")]
  last_update: std::time::Instant,
  #[serde(skip, default)]
  result: Option<Rc<dyn Drawable>>,
}

impl Command for CurlCommand {
  fn update_paramters(&mut self, parameters: ParameterUpdate) {
    match parameters {
      ParameterUpdate::Update(key, origin) => {
        if let Some(origin) = origin {
          if let Some(OoMCoordinates::Coordinate(coord)) = self.coordinates.get_mut(&key) {
            *coord = origin;
            self.last_update = std::time::Instant::now();
          }
        }
      }
      ParameterUpdate::DragUpdate(pos, delta, trans) => {
        let mut coords = vec![];
        for coord in self.coordinates.values_mut() {
          match coord {
            OoMCoordinates::Coordinate(c) => coords.push(c),
            OoMCoordinates::Coordinates(c) => {
              for coord in c.iter_mut() {
                coords.push(coord);
              }
            }
          }
        }
        update_closest(pos, trans, delta, &mut coords);
        self.last_update = std::time::Instant::now();
      }
    }
  }

  fn run(&mut self) {
    if self.send_rcv.is_none() {
      let (send, recv) = std::sync::mpsc::channel();
      self.send_rcv = Some((send, recv));
    }

    if let Some((ref mut send, ref mut recv)) = self.send_rcv {
      for el in recv.try_iter() {
        if let MapEvent::Layer(EventLayer { id: _, geometries }) = el {
          if geometries.len() == 1 {
            self.result = Some(Rc::new(geometries[0].clone()));
          }
          if geometries.len() > 1 {
            self.result = Some(Rc::new(Geometry::GeometryCollection(
              geometries,
              Metadata::default(),
            )));
          }
        }
      }

      for coord in self.coordinates.values() {
        if let OoMCoordinates::Coordinate(c) = coord {
          if c.x.is_nan() || c.y.is_nan() {
            return;
          }
        }
      }

      if self.last_request > self.last_update || self.last_request.elapsed().as_millis() < 1_000 {
        return;
      }

      let mut url = self.url_template.clone();
      for (key, coord) in &self.coordinates {
        match coord {
          OoMCoordinates::Coordinate(c) => {
            let coord = c.as_wgs84();
            let coord_str = self
              .coordinate_template
              .clone()
              .replace("{lat}", &coord.lat.to_string())
              .replace("{lon}", &coord.lon.to_string());
            url = url.replace(&format!("{{{key}}}"), &coord_str);
          }
          OoMCoordinates::Coordinates(_) => {}
        }
      }

      let send = send.clone();
      let mut parser = self.parser.clone();

      self.last_request = std::time::Instant::now();
      tokio::spawn(async move {
        let response = surf::get(url)
          .recv_string()
          .await
          .inspect_err(|e| error!("Could not fetch data: {e}"));
        if let Ok(response) = response {
          let cursor = std::io::Cursor::new(response.into_bytes());
          let buf_read: Box<dyn BufRead> = Box::new(cursor);
          let parsed = parser.parse(buf_read);
          for el in parsed {
            let _ = send.send(el);
          }
        }
      });
    }
  }

  fn result(&self) -> Option<Rc<dyn Drawable>> {
    self.result.clone()
  }

  fn is_locked(&self) -> bool {
    self.locked
  }

  fn is_visible(&self) -> bool {
    self.visible
  }

  fn locked(&mut self) -> &mut bool {
    &mut self.locked
  }

  fn visible(&mut self) -> &mut bool {
    &mut self.visible
  }

  fn register_keys(&self) -> Box<dyn Iterator<Item = &str> + '_> {
    Box::new(self.coordinates.keys().map(std::string::String::as_str))
  }

  fn ui(&mut self, ui: &mut egui::Ui) {
    ui.label("Curl Command");
    ui.text_edit_singleline(&mut self.url_template);
    if let Some(pdt) = &mut self.post_data_template {
      ui.text_edit_singleline(pdt);
    }
    ui.text_edit_singleline(&mut self.coordinate_template);
  }

  fn name(&self) -> &str {
    self.name.as_str()
  }

  fn bounding_box(&self) -> crate::map::coordinates::BoundingBox {
    crate::map::coordinates::BoundingBox::from_iterator(self.coordinates.values().flat_map(
      |coord| match coord {
        OoMCoordinates::Coordinate(c) => Either::Left(std::iter::once(*c)),
        OoMCoordinates::Coordinates(c) => Either::Right(c.iter().copied()),
      },
    ))
  }
}

#[cfg(test)]
mod tests {

  use crate::{
    map::{coordinates::PixelCoordinate, mapvas_egui::layer::commands::OoMCoordinates},
    parser::{Parsers, TTJsonParser},
  };

  #[test]
  fn test_serialize_curl() {
    let curl = super::CurlCommand {
      name: "TomTom Route".to_string(),
      url_template:
        "https://api.tomtom.com/routing/1/calculateRoute/{origin}:{destination}/json?key=<key>"
          .to_string(),
      post_data_template: Some("data".to_string()),
      coordinates: [
        (
          "origin".to_string(),
          OoMCoordinates::Coordinate(PixelCoordinate {
            x: f32::NAN,
            y: f32::NAN,
          }),
        ),
        (
          "destination".to_string(),
          OoMCoordinates::Coordinate(PixelCoordinate {
            x: f32::NAN,
            y: f32::NAN,
          }),
        ),
      ]
      .into(),
      coordinate_template: "{lat},{lon}".to_string(),
      locked: false,
      visible: true,
      parser: Parsers::TTJson(TTJsonParser::new()),
      send_rcv: None,
      last_request: std::time::Instant::now(),
      last_update: std::time::Instant::now(),
      result: None,
    };
    let serialized = serde_yaml::to_string(&curl).unwrap();
    println!("{serialized}");
    assert_eq!(
      serialized,
      "name = \"TomTom Route\"\nurl_template = \"https://api.tomtom.com/routing/1/calculateRoute/{origin}:{destination}/json?key=<key>\"\npost_data_template = \"data\"\ncoordinate_template = \"{lat},{lon}\"\nlocked = false\nvisible = true\n\n[coordinates.destination.Coordinate]\nx = nan\ny = nan\n\n[coordinates.origin.Coordinate]\nx = nan\ny = nan\n\n[parser.TTJson]\ncolor = [0, 0, 255, 255]\n"
    );
    //   let deserialized: super::CurlCommand = serde_json::from_str(&serialized).unwrap();
  }
}
