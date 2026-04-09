use std::{str::FromStr, sync::LazyLock};

use egui::Color32;
use log::{debug, error, info};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use super::Parser;
use crate::{
  map::{
    coordinates::WGS84Coordinate,
    geometry_collection::{Geometry, Metadata, Style},
    map_event::{Color, FillStyle, Layer, MapEvent},
  },
  profile_scope,
};

static COLOR_RE: LazyLock<Regex> = LazyLock::new(|| {
  RegexBuilder::new(r"\b(?:)?(darkBlue|blue|darkRed|red|darkGreen|green|darkYellow|yellow|Black|White|darkGrey|dark|Brown)\b")
        .case_insensitive(true)
        .build()
        .expect("Color regex must compile")
});
static FILL_RE: std::sync::LazyLock<Regex> = LazyLock::new(|| {
  RegexBuilder::new(r"(solid|transparent|nofill)")
    .case_insensitive(true)
    .build()
    .expect("Fill regex must compile")
});
static COORD_RE: std::sync::LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(-?\d*\.\d*), ?(-?\d*\.\d*)").expect("Coordinate regex must compile")
});
static CLEAR_RE: std::sync::LazyLock<Regex> = LazyLock::new(|| {
  RegexBuilder::new("clear")
    .case_insensitive(true)
    .build()
    .expect("Clear regex must compile")
});
static FLEXPOLY_RE: std::sync::LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"^(B[A-Za-z0-9_\-]{4,})$").expect("Flexpolyline regex must compile")
});
static GOOGLEPOLY_RE: std::sync::LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^([?-~]{4,})$").expect("Google polyline regex must compile"));

#[allow(clippy::module_name_repetitions)]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GrepParser {
  invert_coordinates: bool,
  color: Color,
  fill: FillStyle,
  #[serde(skip, default)]
  label_re: Option<Regex>,
  #[serde(skip)]
  layer_name: String,
}

impl Parser for GrepParser {
  fn parse_line(&mut self, line: &str) -> Option<MapEvent> {
    profile_scope!("GrepParser::parse_line");
    if let Some(event) = Self::parse_clear(line) {
      return Some(event);
    }

    let mut layer = Layer::new(self.layer_name.clone());

    for l in line.split('\n') {
      self.parse_color(l);
      self.parse_fill(l);
      let label = self.parse_label(l);
      let coordinates = self.parse_geometry(l);
      match coordinates.len() {
        0 => (),
        1 => layer.geometries.push(Geometry::Point(
          coordinates[0].into(),
          Metadata {
            label: label.clone().map(std::convert::Into::into),
            style: Some(Style::default().with_color(self.color.into())),
            heading: None,
            time_data: None,
          },
        )),
        _ => {
          if coordinates.len() == 2 || self.fill == FillStyle::NoFill {
            layer.geometries.push(Geometry::LineString(
              coordinates.into_iter().map(Into::into).collect(),
              Metadata {
                label: label.clone().map(std::convert::Into::into),
                style: Some(Style::default().with_color(self.color.into())),
                heading: None,
                time_data: None,
              },
            ));
          } else {
            layer.geometries.push(Geometry::Polygon(
              coordinates.into_iter().map(Into::into).collect(),
              Metadata {
                label: label.clone().map(std::convert::Into::into),
                style: Some(
                  Style::default()
                    .with_color(self.color.into())
                    .with_fill_color(Into::<Color32>::into(self.color).gamma_multiply(0.4)),
                ),
                heading: None,
                time_data: None,
              },
            ));
          }
        }
      }

      layer
        .geometries
        .extend(Self::parse_flexpolyline(l).map(|c| {
          Geometry::LineString(
            c.into_iter().map(Into::into).collect(),
            Metadata {
              label: label.clone().map(std::convert::Into::into),
              style: Some(Style::default().with_color(self.color.into())),
              heading: None,
              time_data: None,
            },
          )
        }));

      layer
        .geometries
        .extend(Self::parse_googlepolyline(l).map(|c| {
          Geometry::LineString(
            c.into_iter().map(Into::into).collect(),
            Metadata {
              label: label.clone().map(std::convert::Into::into),
              style: Some(Style::default().with_color(self.color.into())),
              heading: None,
              time_data: None,
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

  fn set_layer_name(&mut self, layer_name: String) {
    self.layer_name = layer_name;
  }
}

impl GrepParser {
  #[must_use]
  /// # Panics
  /// If there is a typo in some regex.
  pub fn new(invert_coordinates: bool) -> Self {
    Self {
      invert_coordinates,
      color: Color::default(),
      fill: FillStyle::default(),
      label_re: None,
      layer_name: "coordinates".to_string(), // Default name for grep parser
    }
  }

  #[must_use]
  pub fn with_color(mut self, color: Color) -> Self {
    self.color = color;
    self
  }

  #[must_use]
  pub fn with_label_pattern(mut self, label_pattern: &str) -> Self {
    self.label_re = Some(Regex::new(label_pattern).expect("Label pattern regex must compile"));
    self
  }

  fn parse_clear(line: &str) -> Option<MapEvent> {
    CLEAR_RE.is_match(line).then_some(MapEvent::Clear)
  }

  fn parse_color(&mut self, line: &str) {
    for (_, [color]) in COLOR_RE.captures_iter(line).map(|c| c.extract()) {
      let _ = Color::from_str(color)
        .map(|parsed_color| self.color = parsed_color)
        .map_err(|()| error!("Failed parsing {color}"));
    }
  }

  fn parse_fill(&mut self, line: &str) {
    for (_, [fill]) in FILL_RE.captures_iter(line).map(|c| c.extract()) {
      let _ = FillStyle::from_str(fill)
        .map(|parsed_fill| self.fill = parsed_fill)
        .map_err(|()| error!("Failed parsing {fill}"));
    }
  }

  fn parse_geometry(&self, line: &str) -> Vec<WGS84Coordinate> {
    let mut coordinates = vec![];
    for (_, [lat, lon]) in COORD_RE.captures_iter(line).map(|c| c.extract()) {
      if let Some(coord) = self.parse_coordinate(lat, lon) {
        coordinates.push(coord);
      }
    }
    coordinates
  }

  #[expect(clippy::cast_possible_truncation)]
  fn parse_flexpolyline(line: &str) -> impl Iterator<Item = Vec<WGS84Coordinate>> {
    let mut v = Vec::new();
    for (_, [poly]) in FLEXPOLY_RE.captures_iter(line).map(|c| c.extract()) {
      if let Ok(flexpolyline::Polyline::Data2d {
        coordinates,
        precision2d: _,
      }) = flexpolyline::Polyline::decode(poly)
        .inspect_err(|e| info!("Could not parse possible flexpolyline {e:?}"))
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
  fn parse_googlepolyline(line: &str) -> impl Iterator<Item = Vec<WGS84Coordinate>> {
    let mut v = Vec::new();
    for (_, [poly]) in GOOGLEPOLY_RE.captures_iter(line).map(|c| c.extract()) {
      let unescaped = poly.replace("\\\\", "\\");
      let candidates: [&str; 2] = if unescaped.len() < poly.len() {
        [&unescaped, poly]
      } else {
        [poly, &unescaped]
      };
      for candidate in candidates {
        let decoded = polyline::decode_polyline(candidate, 5)
          .inspect_err(|e| info!("Could not parse possible googlepolyline {e:?}"));
        if let Ok(ls) = decoded {
          v.push(
            ls.into_iter()
              .map(|c| WGS84Coordinate {
                lat: c.y as f32,
                lon: c.x as f32,
              })
              .collect(),
          );
          break;
        }
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
        debug!("Could not parse {x} {e:?}.");
        return None;
      }
    };
    let lon = match y.parse::<f32>() {
      Ok(v) => v,
      Err(e) => {
        debug!("Could not parse {y} {e:?}");
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

#[cfg(test)]
mod tests {
  use super::*;

  fn parser() -> GrepParser {
    GrepParser::new(false)
  }

  #[test]
  fn parse_single_coordinate_produces_point() {
    let mut p = parser();
    let event = p.parse_line("52.5, 13.4");
    let MapEvent::Layer(layer) = event.unwrap() else {
      panic!("Expected Layer event");
    };
    assert_eq!(layer.geometries.len(), 1);
    assert!(matches!(layer.geometries[0], Geometry::Point(_, _)));
  }

  #[test]
  fn parse_two_coordinates_produces_linestring() {
    let mut p = parser();
    let event = p.parse_line("52.5, 13.4 53.0, 14.0");
    let MapEvent::Layer(layer) = event.unwrap() else {
      panic!("Expected Layer event");
    };
    assert_eq!(layer.geometries.len(), 1);
    assert!(matches!(layer.geometries[0], Geometry::LineString(_, _)));
  }

  #[test]
  fn parse_three_plus_coords_with_fill_produces_polygon() {
    let mut p = parser();
    p.fill = FillStyle::Solid;
    let event = p.parse_line("52.0, 13.0 53.0, 14.0 54.0, 15.0");
    let MapEvent::Layer(layer) = event.unwrap() else {
      panic!("Expected Layer event");
    };
    assert_eq!(layer.geometries.len(), 1);
    assert!(matches!(layer.geometries[0], Geometry::Polygon(_, _)));
  }

  #[test]
  fn parse_three_coords_nofill_produces_linestring() {
    let mut p = parser();
    let event = p.parse_line("52.0, 13.0 53.0, 14.0 54.0, 15.0");
    let MapEvent::Layer(layer) = event.unwrap() else {
      panic!("Expected Layer event");
    };
    assert!(matches!(layer.geometries[0], Geometry::LineString(_, _)));
  }

  #[test]
  fn parse_coordinate_normal() {
    let p = parser();
    let coord = p.parse_coordinate("52.5", "13.4").unwrap();
    assert!((coord.lat - 52.5).abs() < 0.001);
    assert!((coord.lon - 13.4).abs() < 0.001);
  }

  #[test]
  fn parse_coordinate_inverted() {
    let p = GrepParser::new(true);
    let coord = p.parse_coordinate("13.4", "52.5").unwrap();
    assert!((coord.lat - 52.5).abs() < 0.001);
    assert!((coord.lon - 13.4).abs() < 0.001);
  }

  #[test]
  fn parse_coordinate_invalid_out_of_range() {
    let p = parser();
    assert!(p.parse_coordinate("91.0", "13.0").is_none());
    assert!(p.parse_coordinate("52.0", "181.0").is_none());
  }

  #[test]
  fn parse_coordinate_non_numeric() {
    let p = parser();
    assert!(p.parse_coordinate("abc", "13.0").is_none());
    assert!(p.parse_coordinate("52.0", "xyz").is_none());
  }

  #[test]
  fn parse_color_sets_color() {
    let mut p = parser();
    p.parse_color("some text red here");
    assert_eq!(p.color, Color::from_str("red").unwrap());
  }

  #[test]
  fn parse_color_case_insensitive() {
    let mut p = parser();
    p.parse_color("GREEN coordinates");
    assert_eq!(p.color, Color::from_str("green").unwrap());
  }

  #[test]
  fn parse_fill_sets_fill() {
    let mut p = parser();
    p.parse_fill("solid fill here");
    assert_eq!(p.fill, FillStyle::Solid);
  }

  #[test]
  fn parse_clear_detected() {
    assert_eq!(GrepParser::parse_clear("CLEAR"), Some(MapEvent::Clear));
    assert_eq!(GrepParser::parse_clear("clear"), Some(MapEvent::Clear));
  }

  #[test]
  fn parse_clear_not_detected() {
    assert!(GrepParser::parse_clear("some text without keyword").is_none());
  }

  #[test]
  fn parse_label_with_pattern() {
    let p = parser().with_label_pattern(r"name: (\w+)");
    let label = p.parse_label("name: Berlin");
    assert_eq!(label, Some("Berlin".to_string()));
  }

  #[test]
  fn parse_label_no_pattern() {
    let p = parser();
    assert!(p.parse_label("anything").is_none());
  }

  #[test]
  fn parse_label_no_match() {
    let p = parser().with_label_pattern(r"name: (\w+)");
    assert!(p.parse_label("no match here").is_none());
  }

  #[test]
  fn no_match_line_returns_none() {
    let mut p = parser();
    assert!(p.parse_line("no coordinates here").is_none());
  }

  #[test]
  fn empty_line_returns_none() {
    let mut p = parser();
    assert!(p.parse_line("").is_none());
  }

  #[test]
  fn with_color_builder() {
    let p = GrepParser::new(false).with_color(Color::from_str("red").unwrap());
    assert_eq!(p.color, Color::from_str("red").unwrap());
  }

  #[test]
  fn clear_line_returns_clear_event() {
    let mut p = parser();
    assert_eq!(p.parse_line("clear"), Some(MapEvent::Clear));
  }

  use rstest::rstest;

  // --- Google polyline regex tests ---

  #[rstest]
  // valid: minimal 4-char alphanumeric
  #[case("AAAA", true)]
  // valid: classic Google docs example — [(38.5,-120.2),(40.7,-120.95),(43.252,-126.453)]
  #[case(r"_p~iF~ps|U_ulLnnqC_mqNvxq`E", true)]
  // valid: (0,0)->(1,1), no special chars
  #[case("??_ibE_ibE", true)]
  // valid: contains single backslash — (0,0)->(0,-0.00015)->(0,-0.00030)
  #[case(r"???\?\", true)]
  // valid: same route, backslashes doubled as in CSV-escaped form
  #[case(r"???\\?\\", true)]
  // invalid: too short
  #[case("abc", false)]
  // invalid: empty
  #[case("", false)]
  // invalid: space (ASCII 32, below 63)
  #[case("ab cd ef", false)]
  // invalid: # (ASCII 35, below 63)
  #[case("abc#xyz>?@", false)]
  fn googlepoly_re_matches(#[case] input: &str, #[case] expected: bool) {
    assert_eq!(GOOGLEPOLY_RE.is_match(input), expected);
  }

  // --- Google polyline decode tests ---

  #[rstest]
  // classic Google docs example — [(38.5,-120.2),(40.7,-120.95),(43.252,-126.453)]
  #[case(r"_p~iF~ps|U_ulLnnqC_mqNvxq`E")]
  // (0,0)->(1,1)
  #[case("??_ibE_ibE")]
  // (0,0)->(0,-0.00015)->(0,-0.00030) with single backslash — decoded directly
  #[case(r"???\?\")]
  // same route with doubled backslashes — unescaped first then decoded
  #[case(r"???\\?\\")]
  fn parse_googlepolyline_decodes(#[case] poly: &str) {
    let coords: Vec<_> = GrepParser::parse_googlepolyline(poly).collect();
    assert_eq!(coords.len(), 1, "expected polyline to decode: {poly}");
    assert!(!coords[0].is_empty());
  }
}
