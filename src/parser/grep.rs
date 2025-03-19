use std::str::FromStr;

use log::{debug, error, info};
use regex::{Regex, RegexBuilder};

use crate::map::{
  coordinates::WGS84Coordinate,
  geometry_collection::{Geometry, Metadata, Style},
  map_event::{Color, FillStyle, Layer, MapEvent},
};

use super::Parser;

#[allow(clippy::module_name_repetitions)]
#[derive(Clone, Debug)]
pub struct GrepParser {
  invert_coordinates: bool,
  color: Color,
  fill: FillStyle,
  color_re: Regex,
  fill_re: Regex,
  coord_re: Regex,
  clear_re: Regex,
  flexpoly_re: Regex,
  googlepoly_re: Regex,
  label_re: Option<Regex>,
}

impl Parser for GrepParser {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    if let Some(event) = self.parse_clear(line) {
      return Some(event);
    }

    let mut layer = Layer::new("test".to_string());

    for l in line.split('\n') {
      self.parse_color(l);
      self.parse_fill(l);
      let label = self.parse_label(l);
      let coordinates = self.parse_shape(l);
      match coordinates.len() {
        0 => (),
        1 => layer.geometries.push(Geometry::Point(
          coordinates[0].into(),
          Metadata {
            label: label.clone(),
            style: Some(Style::default().with_color(self.color.into())),
          },
        )),
        _ => todo!(),
      }

      layer.geometries.extend(self.parse_flexpolyline(l).map(|c| {
        Geometry::LineString(
          c.into_iter().map(Into::into).collect(),
          Metadata {
            label: label.clone(),
            style: Some(Style::default().with_color(self.color.into())),
          },
        )
      }));

      layer
        .geometries
        .extend(self.parse_googlepolyline(l).map(|c| {
          Geometry::LineString(
            c.into_iter().map(Into::into).collect(),
            Metadata {
              label: label.clone(),
              style: Some(Style::default().with_color(self.color.into())),
            },
          )
        }));
    }
    if layer.geometries.is_empty() {
      None
    } else {
      Some(MapEvent::Layer(layer))
    }
  }
}

impl GrepParser {
  #[must_use]
  /// # Panics
  /// If there is a typo in some regex.
  pub fn new(invert_coordinates: bool) -> Self {
    let color_re =
      RegexBuilder::new(r"\b(?:)?(darkBlue|blue|darkRed|red|darkGreen|green|darkYellow|yellow|Black|White|darkGrey|dark|Brown)\b")
        .case_insensitive(true)
        .build()
        .unwrap();
    let fill_re = RegexBuilder::new(r"(solid|transparent|nofill)")
      .case_insensitive(true)
      .build()
      .unwrap();
    let coord_re = Regex::new(r"(-?\d*\.\d*), ?(-?\d*\.\d*)").unwrap();
    let clear_re = RegexBuilder::new("clear")
      .case_insensitive(true)
      .build()
      .unwrap();
    let flexpoly_re = Regex::new(r"^(B[A-Za-z0-9_\-]{4,})$").unwrap();
    let googlepoly_re = Regex::new(r"^([A-Za-z0-9_\^\|\~\@\?><\:\.\,\;\-\\\!\(\)]{4,})$")
      .expect("Invalid regex pattern");

    Self {
      invert_coordinates,
      color: Color::default(),
      fill: FillStyle::default(),
      color_re,
      fill_re,
      coord_re,
      clear_re,
      flexpoly_re,
      googlepoly_re,
      label_re: None,
    }
  }

  #[must_use]
  pub fn with_color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }

  /// # Panics
  /// If the given regex is invalid.
  #[must_use]
  pub fn with_label_pattern(mut self, label_pattern: &str) -> Self {
    self.label_re = Some(Regex::new(label_pattern).expect("Cannot build label regex."));
    self
  }

  fn parse_clear(&self, line: &str) -> Option<MapEvent> {
    self.clear_re.is_match(line).then_some(MapEvent::Clear)
  }

  fn parse_color(&mut self, line: &str) {
    for (_, [color]) in self.color_re.captures_iter(line).map(|c| c.extract()) {
      let _ = Color::from_str(color)
        .map(|parsed_color| self.color = parsed_color)
        .map_err(|()| error!("Failed parsing {}", color));
    }
  }

  fn parse_fill(&mut self, line: &str) {
    for (_, [fill]) in self.fill_re.captures_iter(line).map(|c| c.extract()) {
      let _ = FillStyle::from_str(fill)
        .map(|parsed_fill| self.fill = parsed_fill)
        .map_err(|()| error!("Failed parsing {}", fill));
    }
  }

  fn parse_shape(&self, line: &str) -> Vec<WGS84Coordinate> {
    let mut coordinates = vec![];
    for (_, [lat, lon]) in self.coord_re.captures_iter(line).map(|c| c.extract()) {
      if let Some(coord) = self.parse_coordinate(lat, lon) {
        coordinates.push(coord);
      }
    }
    coordinates
  }

  #[expect(clippy::cast_possible_truncation)]
  fn parse_flexpolyline(&self, line: &str) -> impl Iterator<Item = Vec<WGS84Coordinate>> {
    let mut v = Vec::new();
    for (_, [poly]) in self.flexpoly_re.captures_iter(line).map(|c| c.extract()) {
      if let Ok(flexpolyline::Polyline::Data2d {
        coordinates,
        precision2d: _,
      }) = flexpolyline::Polyline::decode(poly)
        .inspect_err(|e| info!("Could not parse possible flexpolyline {:?}", e))
      {
        v.push(
          coordinates
            .into_iter()
            .map(|c| WGS84Coordinate {
              lat: c.0 as f32,
              lon: c.1 as f32,
            })
            .collect(),
        );
      }
    }
    v.into_iter()
  }

  #[expect(clippy::cast_possible_truncation)]
  fn parse_googlepolyline(&self, line: &str) -> impl Iterator<Item = Vec<WGS84Coordinate>> {
    let mut v = Vec::new();
    for (_, [poly]) in self.googlepoly_re.captures_iter(line).map(|c| c.extract()) {
      let decoded = polyline::decode_polyline(poly, 5)
        .inspect_err(|e| info!("Could not parse possible googlepolyline {:?}", e));
      if let Ok(ls) = decoded {
        v.push(
          ls.into_iter()
            .map(|c| WGS84Coordinate {
              lat: c.y as f32,
              lon: c.x as f32,
            })
            .collect(),
        );
      }
    }
    v.into_iter()
  }

  fn parse_label(&self, line: &str) -> Option<String> {
    if let Some(re) = &self.label_re {
      let res: Vec<_> = re
        .captures_iter(line)
        .map(|c| c.extract::<1>().1[0])
        .collect();

      if res.is_empty() {
        None
      } else {
        Some(res.join(" | "))
      }
    } else {
      None
    }
  }

  fn parse_coordinate(&self, x: &str, y: &str) -> Option<WGS84Coordinate> {
    let lat = match x.parse::<f32>() {
      Ok(v) => v,
      Err(e) => {
        debug!("Could not parse {} {:?}.", x, e);
        return None;
      }
    };
    let lon = match y.parse::<f32>() {
      Ok(v) => v,
      Err(e) => {
        debug!("Could not parse {} {:?}", y, e);
        return None;
      }
    };
    let mut coordinates = WGS84Coordinate { lat, lon };
    if self.invert_coordinates {
      std::mem::swap(&mut coordinates.lat, &mut coordinates.lon);
    }
    coordinates.is_valid().then_some(coordinates)
  }
}
