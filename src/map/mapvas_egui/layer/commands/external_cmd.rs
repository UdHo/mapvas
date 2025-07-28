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
    ui.label("Executable:");
    ui.add_sized(
      [ui.available_width(), 0.0],
      egui::TextEdit::singleline(&mut self.executable),
    );
    
    ui.separator();
    ui.label("Coordinates:");
    for (key, coord) in &self.coordinates {
      match coord {
        OoMCoordinates::Coordinate(c) => {
          if c.is_valid() {
            let wgs84 = c.as_wgs84();
            ui.label(format!("{}: {:.4}, {:.4}", key, wgs84.lat, wgs84.lon));
          } else {
            ui.label(format!("{key}: (not set)"));
          }
        }
        OoMCoordinates::Coordinates(coords) => {
          if coords.is_empty() {
            ui.label(format!("{key}: (no coordinates)"));
          } else {
            ui.label(format!("{}: {} coordinates", key, coords.len()));
            for (i, c) in coords.iter().enumerate() {
              let wgs84 = c.as_wgs84();
              ui.small(format!("  {}: {:.4}, {:.4}", i + 1, wgs84.lat, wgs84.lon));
            }
          }
        }
      }
    }
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
      if let Ok(response) = response {
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
    ui.add_sized(
      [ui.available_width(), 0.0],
      egui::TextEdit::singleline(&mut self.url_template),
    );
    if let Some(pdt) = &mut self.post_data_template {
      ui.add_sized([ui.available_width(), 0.0], egui::TextEdit::singleline(pdt));
    }
    ui.add_sized(
      [ui.available_width(), 0.0],
      egui::TextEdit::singleline(&mut self.coordinate_template),
    );
    
    ui.separator();
    ui.label("Coordinates:");
    for (key, coord) in &self.coordinates {
      match coord {
        OoMCoordinates::Coordinate(c) => {
          if c.is_valid() {
            let wgs84 = c.as_wgs84();
            ui.label(format!("{}: {:.4}, {:.4}", key, wgs84.lat, wgs84.lon));
          } else {
            ui.label(format!("{key}: (not set)"));
          }
        }
        OoMCoordinates::Coordinates(coords) => {
          if coords.is_empty() {
            ui.label(format!("{key}: (no coordinates)"));
          } else {
            ui.label(format!("{}: {} coordinates", key, coords.len()));
            for (i, c) in coords.iter().enumerate() {
              let wgs84 = c.as_wgs84();
              ui.small(format!("  {}: {:.4}, {:.4}", i + 1, wgs84.lat, wgs84.lon));
            }
          }
        }
      }
    }
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

  fn bounding_box(&self) -> Option<BoundingBox> {
    self
      .result
      .as_ref()
      .map(|r| r.bounding_box())
      .unwrap_or_default()
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

impl ExternalCommand {
  fn calculate_line_distance(coords: &[crate::map::coordinates::PixelCoordinate]) -> f64 {
    if coords.len() < 2 {
      return 0.0;
    }
    
    coords.windows(2)
      .map(|window| {
        let p1 = window[0].as_wgs84();
        let p2 = window[1].as_wgs84();
        let lat1 = p1.lat as f64 * std::f64::consts::PI / 180.0;
        let lat2 = p2.lat as f64 * std::f64::consts::PI / 180.0;
        let dlat = lat2 - lat1;
        let dlon = (p2.lon - p1.lon) as f64 * std::f64::consts::PI / 180.0;
        
        let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
        6371000.0 * c
      })
      .sum()
  }
  
  fn calculate_polygon_area(coords: &[crate::map::coordinates::PixelCoordinate]) -> f64 {
    if coords.len() < 3 {
      return 0.0;
    }
    
    let wgs84_coords: Vec<_> = coords.iter().map(|c| c.as_wgs84()).collect();
    let mut area = 0.0;
    
    for i in 0..wgs84_coords.len() {
      let j = (i + 1) % wgs84_coords.len();
      area += (wgs84_coords[j].lon as f64) * (wgs84_coords[i].lat as f64);
      area -= (wgs84_coords[i].lon as f64) * (wgs84_coords[j].lat as f64);
    }
    
    (area.abs() / 2.0) * 111320.0 * 110540.0
  }
  
  fn calculate_polygon_perimeter(coords: &[crate::map::coordinates::PixelCoordinate]) -> f64 {
    if coords.len() < 2 {
      return 0.0;
    }
    
    let mut perimeter = Self::calculate_line_distance(coords);
    
    if coords.len() > 2 {
      let first = coords[0].as_wgs84();
      let last = coords[coords.len() - 1].as_wgs84();
      let lat1 = first.lat as f64 * std::f64::consts::PI / 180.0;
      let lat2 = last.lat as f64 * std::f64::consts::PI / 180.0;
      let dlat = lat2 - lat1;
      let dlon = (last.lon - first.lon) as f64 * std::f64::consts::PI / 180.0;
      
      let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
      let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
      perimeter += 6371000.0 * c;
    }
    
    perimeter
  }
  
  fn count_geometry_types(geometries: &[Geometry<crate::map::coordinates::PixelCoordinate>]) -> (usize, usize, usize) {
    let mut points = 0;
    let mut lines = 0;
    let mut polygons = 0;
    
    for geom in geometries {
      match geom {
        Geometry::Point(_, _) => points += 1,
        Geometry::LineString(_, _) => lines += 1,
        Geometry::Polygon(_, _) => polygons += 1,
        Geometry::GeometryCollection(nested, _) => {
          let (p, l, poly) = Self::count_geometry_types(nested);
          points += p;
          lines += l;
          polygons += poly;
        }
      }
    }
    
    (points, lines, polygons)
  }

  fn display_geometry_info(ui: &mut egui::Ui, drawable: &dyn Drawable) {
    if let Some(geometry) = drawable.as_geometry() {
      Self::display_geometry_details(ui, geometry);
    } else {
      if let Some(bbox) = drawable.bounding_box() {
        if bbox.is_valid() {
          let center = bbox.center().as_wgs84();
          let bbox_text = "Geometry bounding box:";
          let available_width = (ui.available_width() - 80.0).max(30.0);
          let (truncated_bbox, _) = super::truncate_label_by_width(ui, bbox_text, available_width);
          ui.label(truncated_bbox);
          
          let center_text = format!("  Center: {:.3}, {:.3}", center.lat, center.lon);
          let (truncated_center, _) = super::truncate_label_by_width(ui, &center_text, available_width);
          ui.small(truncated_center);
          
          let size_text = format!("  Size: {:.0}m × {:.0}m", bbox.width(), bbox.height());
          let (truncated_size, _) = super::truncate_label_by_width(ui, &size_text, available_width);
          ui.small(truncated_size);
        } else {
          let error_text = "Invalid geometry bounding box";
          let available_width = (ui.available_width() - 80.0).max(30.0);
          let (truncated_error, _) = super::truncate_label_by_width(ui, error_text, available_width);
          ui.label(truncated_error);
        }
      } else {
        let no_data_text = "No geometry data available";
        let available_width = (ui.available_width() - 80.0).max(30.0);
        let (truncated_no_data, _) = super::truncate_label_by_width(ui, no_data_text, available_width);
        ui.label(truncated_no_data);
      }
    }
  }
  
  fn display_geometry_details(ui: &mut egui::Ui, geometry: &Geometry<crate::map::coordinates::PixelCoordinate>) {
    match geometry {
      Geometry::Point(coord, metadata) => {
        let wgs84 = coord.as_wgs84();
        let point_text = format!("📍 Point at {:.3}°N, {:.3}°E", wgs84.lat, wgs84.lon);
        let available_width = (ui.available_width() - 80.0).max(30.0);
        let (truncated_point, _was_truncated) = super::truncate_label_by_width(ui, &point_text, available_width);
        let response = ui.label(truncated_point);
        if response.clicked() {
          let popup_id = egui::Id::new("external_point_popup");
          let full_text = format!("📍 Point Location\nLatitude: {:.6}°\nLongitude: {:.6}°\nCoordinates: {:.4}°N, {:.4}°E\n\nFull Coordinates:\n{:.6}, {:.6}", wgs84.lat, wgs84.lon, wgs84.lat, wgs84.lon, wgs84.lat, wgs84.lon);
          ui.memory_mut(|mem| mem.data.insert_temp(popup_id, full_text));
        }
        if let Some(label) = &metadata.label {
          let available_width = (ui.available_width() - 100.0).max(30.0);
          let (truncated_label, _was_label_truncated) = super::truncate_label_by_width(ui, label, available_width);
          let label_response = ui.small(format!("  Label: {truncated_label}"));
          if label_response.clicked() {
            let popup_id = egui::Id::new("external_point_label_popup");
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, format!("Point Label: {}", label)));
          }
        }
      }
      Geometry::LineString(coords, metadata) => {
        let total_distance = Self::calculate_line_distance(coords);
        let linestring_text = format!("📏 Line {} pts ({:.0}m)", coords.len(), total_distance);
        let available_width = (ui.available_width() - 80.0).max(30.0);
        let (truncated_linestring, _was_truncated) = super::truncate_label_by_width(ui, &linestring_text, available_width);
        let response = ui.label(truncated_linestring);
        if response.clicked() {
          let popup_id = egui::Id::new("external_linestring_popup");
          let coords_text = coords.iter()
            .enumerate()
            .map(|(i, coord)| {
              let wgs84 = coord.as_wgs84();
              format!("{:2}: {:.6},{:.6}", i + 1, wgs84.lat, wgs84.lon)
            })
            .collect::<Vec<_>>()
            .join("\n");
          let full_text = format!("📏 LineString Details\nTotal Points: {}\nApproximate Length: {:.2} meters\nSegments: {}\n\nAll Coordinates:\n{}", coords.len(), total_distance, coords.len().saturating_sub(1), coords_text);
          ui.memory_mut(|mem| mem.data.insert_temp(popup_id, full_text));
        }
        if let Some(label) = &metadata.label {
          let available_width = (ui.available_width() - 100.0).max(30.0);
          let (truncated_label, _was_label_truncated) = super::truncate_label_by_width(ui, label, available_width);
          let label_response = ui.small(format!("  Label: {truncated_label}"));
          if label_response.clicked() {
            let popup_id = egui::Id::new("external_linestring_label_popup");
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, format!("LineString Label: {}", label)));
          }
        }
        if let (Some(first), Some(last)) = (coords.first(), coords.last()) {
          let first_wgs84 = first.as_wgs84();
          let last_wgs84 = last.as_wgs84();
          let start_text = format!("  Start: {:.3}°N, {:.3}°E", first_wgs84.lat, first_wgs84.lon);
          let end_text = format!("  End: {:.3}°N, {:.3}°E", last_wgs84.lat, last_wgs84.lon);
          let available_width = (ui.available_width() - 80.0).max(30.0);
          let (truncated_start, _start_truncated) = super::truncate_label_by_width(ui, &start_text, available_width);
          let (truncated_end, _end_truncated) = super::truncate_label_by_width(ui, &end_text, available_width);
          let start_response = ui.small(truncated_start);
          if start_response.clicked() {
            let popup_id = egui::Id::new("external_linestring_start_popup");
            let full_text = format!("📏 LineString Start Point\nLatitude: {:.6}°\nLongitude: {:.6}°\nCoordinates: {:.4}°N, {:.4}°E\n\nFull Coordinates:\n{:.6}, {:.6}", first_wgs84.lat, first_wgs84.lon, first_wgs84.lat, first_wgs84.lon, first_wgs84.lat, first_wgs84.lon);
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, full_text));
          }
          let end_response = ui.small(truncated_end);
          if end_response.clicked() {
            let popup_id = egui::Id::new("external_linestring_end_popup");
            let full_text = format!("📏 LineString End Point\nLatitude: {:.6}°\nLongitude: {:.6}°\nCoordinates: {:.4}°N, {:.4}°E\n\nFull Coordinates:\n{:.6}, {:.6}", last_wgs84.lat, last_wgs84.lon, last_wgs84.lat, last_wgs84.lon, last_wgs84.lat, last_wgs84.lon);
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, full_text));
          }
        }
      }
      Geometry::Polygon(coords, metadata) => {
        let area = Self::calculate_polygon_area(coords);
        let polygon_text = format!("⬟ Polygon {} vtx ({:.0}m²)", coords.len(), area);
        let available_width = (ui.available_width() - 80.0).max(30.0);
        let (truncated_polygon, _was_truncated) = super::truncate_label_by_width(ui, &polygon_text, available_width);
        let response = ui.label(truncated_polygon);
        if response.clicked() {
          let popup_id = egui::Id::new("external_polygon_popup");
          let coords_text = coords.iter()
            .enumerate()
            .map(|(i, coord)| {
              let wgs84 = coord.as_wgs84();
              format!("{:2}: {:.6},{:.6}", i + 1, wgs84.lat, wgs84.lon)
            })
            .collect::<Vec<_>>()
            .join("\n");
          let full_text = format!("⬟ Polygon Details\nVertices: {}\nApproximate Area: {:.2} square meters\nPerimeter: {:.2} meters\n\nAll Vertices:\n{}", coords.len(), area, Self::calculate_polygon_perimeter(coords), coords_text);
          ui.memory_mut(|mem| mem.data.insert_temp(popup_id, full_text));
        }
        if let Some(label) = &metadata.label {
          let available_width = (ui.available_width() - 100.0).max(30.0);
          let (truncated_label, _was_label_truncated) = super::truncate_label_by_width(ui, label, available_width);
          let label_response = ui.small(format!("  Label: {truncated_label}"));
          if label_response.clicked() {
            let popup_id = egui::Id::new("external_polygon_label_popup");
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, format!("Polygon Label: {}", label)));
          }
        }
        if !coords.is_empty() {
          let wgs84_coords: Vec<_> = coords.iter().map(crate::map::coordinates::Coordinate::as_wgs84).collect();
          let min_lat = wgs84_coords.iter().map(|c| c.lat).fold(f32::INFINITY, f32::min);
          let max_lat = wgs84_coords.iter().map(|c| c.lat).fold(f32::NEG_INFINITY, f32::max);
          let min_lon = wgs84_coords.iter().map(|c| c.lon).fold(f32::INFINITY, f32::min);
          let max_lon = wgs84_coords.iter().map(|c| c.lon).fold(f32::NEG_INFINITY, f32::max);
          let bounds_text = format!("  Bounds: {min_lat:.2}°N-{max_lat:.2}°N, {min_lon:.2}°E-{max_lon:.2}°E");
          let available_width = (ui.available_width() - 80.0).max(30.0);
          let (truncated_bounds, _bounds_truncated) = super::truncate_label_by_width(ui, &bounds_text, available_width);
          let bounds_response = ui.small(truncated_bounds);
          if bounds_response.clicked() {
            let popup_id = egui::Id::new("external_polygon_bounds_popup");
            let full_text = format!("⬟ Polygon Bounding Box\nNorth: {:.6}°\nSouth: {:.6}°\nEast: {:.6}°\nWest: {:.6}°\nWidth: {:.1}m\nHeight: {:.1}m", max_lat, min_lat, max_lon, min_lon, (max_lon - min_lon) as f64 * 111320.0, (max_lat - min_lat) as f64 * 110540.0);
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, full_text));
          }
        }
      }
      Geometry::GeometryCollection(geometries, metadata) => {
        let (points, lines, polygons) = Self::count_geometry_types(geometries);
        let collection_text = format!("📦 {}p {}l {}poly", points, lines, polygons);
        let available_width = (ui.available_width() - 80.0).max(30.0);
        let (truncated_collection, _was_truncated) = super::truncate_label_by_width(ui, &collection_text, available_width);
        let response = ui.label(truncated_collection);
        if response.clicked() {
          let popup_id = egui::Id::new("external_collection_popup");
          let geometries_text = geometries.iter()
            .enumerate()
            .map(|(i, geom)| {
              match geom {
                Geometry::Point(coord, _) => {
                  let wgs84 = coord.as_wgs84();
                  format!("  {}: Point ({:.6}, {:.6})", i + 1, wgs84.lat, wgs84.lon)
                },
                Geometry::LineString(coords, _) => {
                  format!("  {}: LineString ({} points)", i + 1, coords.len())
                },
                Geometry::Polygon(coords, _) => {
                  format!("  {}: Polygon ({} vertices)", i + 1, coords.len())
                },
                Geometry::GeometryCollection(nested, _) => {
                  format!("  {}: Collection ({} items)", i + 1, nested.len())
                }
              }
            })
            .collect::<Vec<_>>()
            .join("\n");
          let full_text = format!("📦 Geometry Collection\nTotal Items: {}\nPoints: {}\nLineStrings: {}\nPolygons: {}\nNested Collections: {}\n\nAll Geometries:\n{}", 
            geometries.len(), points, lines, polygons, 
            geometries.iter().filter(|g| matches!(g, Geometry::GeometryCollection(_, _))).count(),
            geometries_text);
          ui.memory_mut(|mem| mem.data.insert_temp(popup_id, full_text));
        }
        if let Some(label) = &metadata.label {
          let available_width = (ui.available_width() - 100.0).max(30.0);
          let (truncated_label, _was_label_truncated) = super::truncate_label_by_width(ui, label, available_width);
          let label_response = ui.small(format!("  Label: {truncated_label}"));
          if label_response.clicked() {
            let popup_id = egui::Id::new("external_collection_label_popup");
            ui.memory_mut(|mem| mem.data.insert_temp(popup_id, format!("Collection Label: {}", label)));
          }
        }
        for (i, geom) in geometries.iter().enumerate() {
          ui.horizontal(|ui| {
            ui.small(format!("  {}:", i + 1));
            Self::display_geometry_details(ui, geom);
          });
        }
      }
    }
    
    let external_popup_ids = [
      "external_point_popup",
      "external_point_label_popup", 
      "external_linestring_popup",
      "external_linestring_label_popup",
      "external_linestring_start_popup",
      "external_linestring_end_popup",
      "external_polygon_popup",
      "external_polygon_label_popup",
      "external_polygon_bounds_popup",
      "external_collection_popup",
      "external_collection_label_popup",
    ];

    for popup_id_str in external_popup_ids {
      let popup_id = egui::Id::new(popup_id_str);
      if let Some(full_text) = ui.memory(|mem| mem.data.get_temp::<String>(popup_id)) {
        let mut is_open = true;
        egui::Window::new("Full Geometry Info")
          .id(popup_id)
          .open(&mut is_open)
          .collapsible(false)
          .resizable(true)
          .movable(true)
          .default_width(500.0)
          .min_width(400.0)
          .max_width(800.0)
          .max_height(400.0)
          .show(ui.ctx(), |ui| {
            egui::ScrollArea::vertical()
              .max_height(300.0)
              .show(ui, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                  ui.add(egui::Label::new(&full_text).wrap());
                });
              });
          });
        
        if !is_open {
          ui.memory_mut(|mem| mem.data.remove::<String>(popup_id));
        }
      }
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

  fn result(&self) -> Box<dyn Iterator<Item = Rc<dyn Drawable>>> {
    Box::new(
      self
        .common
        .result
        .clone()
        .into_iter()
        .chain(self.cmd.coordinates().iter().map(|(_, coord)| {
          let drawable: Rc<dyn Drawable> = match coord {
            OoMCoordinates::Coordinate(c) => Rc::new(Geometry::Point(*c, Metadata::default())),
            OoMCoordinates::Coordinates(coords) => Rc::new(Geometry::GeometryCollection(
              coords
                .iter()
                .map(|c| Geometry::Point(*c, Metadata::default()))
                .collect(),
              Metadata::default(),
            )),
          };
          drawable
        }))
        .collect::<Vec<_>>()
        .into_iter(),
    )
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
    
    // Show response geometry if available
    if let Some(result) = &self.common.result {
      ui.separator();
      let response_header_id = egui::Id::new(format!("response_geometry_{}", self.name()));
      egui::CollapsingHeader::new("Response Geometry")
        .id_salt(response_header_id)
        .default_open(false)
        .show(ui, |ui| {
          Self::display_geometry_info(ui, result.as_ref());
        });
    }
  }

  fn name(&self) -> &str {
    self.cmd.name()
  }

  fn bounding_box(&self) -> BoundingBox {
    self
      .cmd
      .bounding_box()
      .extend(&self.common.bounding_box().unwrap_or_default())
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
