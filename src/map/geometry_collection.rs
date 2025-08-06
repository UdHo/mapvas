use std::iter::once;

use chrono::{DateTime, Utc};
use egui::Color32;
use itertools::Either;
use serde::{Deserialize, Serialize};

use super::coordinates::{BoundingBox, Coordinate, PixelCoordinate, WGS84Coordinate};

type Color = Color32;

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Style {
  visible: bool,
  color: Option<Color>,
  fill_color: Option<Color>,
}

pub const DEFAULT_STYLE: Style = Style {
  color: Some(Color32::BLUE),
  fill_color: None,
  visible: true,
};

impl Default for Style {
  fn default() -> Self {
    DEFAULT_STYLE.clone()
  }
}

impl Style {
  #[must_use]
  pub fn with_color(mut self, color: Color) -> Self {
    self.color = Some(color);
    self
  }

  #[must_use]
  pub fn with_fill_color(mut self, fill_color: Color) -> Self {
    self.fill_color = Some(fill_color);
    self
  }

  #[must_use]
  pub fn with_visible(mut self, visible: bool) -> Self {
    self.visible = visible;
    self
  }

  fn overwrite_with(&self, style: &Style) -> Style {
    Style {
      color: style.color.or(self.color),
      fill_color: style.fill_color.or(self.fill_color),
      visible: style.visible && self.visible,
    }
  }

  fn optional_overwrite_with(&self, style: Option<&Style>) -> Style {
    style.map_or_else(|| self.clone(), |s| self.overwrite_with(s))
  }

  #[must_use]
  pub fn color(&self) -> Color {
    self.color.unwrap_or(Color32::BLUE)
  }

  #[must_use]
  pub fn fill_color(&self) -> Color {
    self.fill_color.unwrap_or(Color32::TRANSPARENT)
  }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct TimeData {
  pub timestamp: Option<DateTime<Utc>>,
  pub time_span: Option<TimeSpan>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct TimeSpan {
  pub begin: Option<DateTime<Utc>>,
  pub end: Option<DateTime<Utc>>,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Label {
  pub name: String,
  pub description: Option<String>,
}

impl Label {
  #[must_use]
  pub fn new(name: String) -> Self {
    Self {
      name,
      description: None,
    }
  }

  #[must_use]
  pub fn with_description(mut self, description: String) -> Self {
    self.description = Some(description);
    self
  }

  /// Get a short display version (just the name, truncated if too long)
  #[must_use]
  pub fn short(&self) -> String {
    // For very long names, create a more compact version
    if self.name.len() > 25 {
      if let Some(arrow_pos) = self.name.find(" → ") {
        // Extract just the NodeId part: "Label NodeId: 284325" -> "NodeId: 284325"
        let before_arrow = &self.name[..arrow_pos];
        if let Some(node_pos) = before_arrow.find("NodeId: ") {
          let node_part = &before_arrow[node_pos..];
          if node_part.len() <= 20 {
            return node_part.to_string();
          }
        }
        if before_arrow.len() <= 20 {
          return before_arrow.to_string();
        }
      }

      // If no arrow or still too long, just truncate
      format!("{}...", &self.name[..22])
    } else {
      self.name.clone()
    }
  }

  /// Get a full display version (name + description if available)
  #[must_use]
  pub fn full(&self) -> String {
    if let Some(ref desc) = self.description {
      format!("{}\n\n{}", self.name, desc)
    } else {
      self.name.clone()
    }
  }
}

impl std::fmt::Display for Label {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.name)
  }
}

impl From<String> for Label {
  fn from(name: String) -> Self {
    Self::new(name)
  }
}

impl From<&str> for Label {
  fn from(name: &str) -> Self {
    Self::new(name.to_string())
  }
}

#[derive(Clone, Default, PartialEq, Debug, Serialize, Deserialize)]
pub struct Metadata {
  pub label: Option<Label>,
  pub style: Option<Style>,
  pub heading: Option<f32>,
  pub time_data: Option<TimeData>,
}

impl Metadata {
  #[must_use]
  pub fn with_label(mut self, label: impl Into<Label>) -> Self {
    self.label = Some(label.into());
    self
  }

  #[must_use]
  pub fn with_structured_label(mut self, label: Label) -> Self {
    self.label = Some(label);
    self
  }

  #[must_use]
  pub fn with_style(mut self, style: Style) -> Self {
    self.style = Some(style);
    self
  }

  #[must_use]
  pub fn with_heading(mut self, heading: f32) -> Self {
    self.heading = Some(heading);
    self
  }

  #[must_use]
  pub fn with_timestamp(mut self, timestamp: DateTime<Utc>) -> Self {
    if let Some(ref mut time_data) = self.time_data {
      time_data.timestamp = Some(timestamp);
    } else {
      self.time_data = Some(TimeData {
        timestamp: Some(timestamp),
        time_span: None,
      });
    }
    self
  }

  #[must_use]
  pub fn with_time_span(
    mut self,
    begin: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
  ) -> Self {
    if let Some(ref mut time_data) = self.time_data {
      time_data.time_span = Some(TimeSpan { begin, end });
    } else {
      self.time_data = Some(TimeData {
        timestamp: None,
        time_span: Some(TimeSpan { begin, end }),
      });
    }
    self
  }

  #[must_use]
  pub fn with_time_data(mut self, time_data: TimeData) -> Self {
    self.time_data = Some(time_data);
    self
  }

  /// Check if this geometry should be visible at the given time
  #[must_use]
  pub fn is_visible_at_time(&self, current_time: DateTime<Utc>) -> bool {
    if let Some(time_data) = &self.time_data {
      // Check timestamp - exact match or very close (within 1 second)
      if let Some(timestamp) = time_data.timestamp {
        let diff = (current_time - timestamp).num_seconds().abs();
        return diff <= 1;
      }

      // Check time span - current time is within the span
      if let Some(time_span) = &time_data.time_span {
        let after_begin = time_span.begin.is_none_or(|begin| current_time >= begin);
        let before_end = time_span.end.is_none_or(|end| current_time <= end);
        return after_begin && before_end;
      }
    }

    // No temporal data means always visible
    true
  }

  /// Check if this geometry should be visible within a time window around the current time
  #[must_use]
  pub fn is_visible_in_time_window(
    &self,
    current_time: DateTime<Utc>,
    window_duration: chrono::Duration,
  ) -> bool {
    if let Some(time_data) = &self.time_data {
      let window_start = current_time - window_duration / 2;
      let window_end = current_time + window_duration / 2;

      // Check timestamp
      if let Some(timestamp) = time_data.timestamp {
        return timestamp >= window_start && timestamp <= window_end;
      }

      // Check time span - geometry is visible if any part of its time span overlaps with the window
      if let Some(time_span) = &time_data.time_span {
        let span_start = time_span.begin.unwrap_or(DateTime::<Utc>::MIN_UTC);
        let span_end = time_span.end.unwrap_or(DateTime::<Utc>::MAX_UTC);

        // Check for overlap: spans overlap if start1 <= end2 && start2 <= end1
        return window_start <= span_end && span_start <= window_end;
      }
    }
    // If no temporal data, always visible
    true
  }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub enum Geometry<C: Coordinate> {
  GeometryCollection(Vec<Geometry<C>>, Metadata),
  Point(C, Metadata),
  LineString(Vec<C>, Metadata),
  Polygon(Vec<C>, Metadata),
}

impl From<Geometry<WGS84Coordinate>> for Geometry<PixelCoordinate> {
  fn from(value: Geometry<WGS84Coordinate>) -> Self {
    match value {
      Geometry::GeometryCollection(geometries, metadata) => Geometry::GeometryCollection(
        geometries.into_iter().map(Geometry::from).collect(),
        metadata,
      ),
      Geometry::Point(coord, metadata) => Geometry::Point(coord.into(), metadata),
      Geometry::LineString(coords, metadata) => Geometry::LineString(
        coords
          .into_iter()
          .map(|c| c.as_pixel_coordinate())
          .collect(),
        metadata,
      ),
      Geometry::Polygon(coords, metadata) => Geometry::Polygon(
        coords
          .into_iter()
          .map(|c| c.as_pixel_coordinate())
          .collect(),
        metadata,
      ),
    }
  }
}

impl<C: Coordinate> Geometry<C> {
  #[must_use]
  pub fn from_coords(coords: Vec<C>) -> Option<Self> {
    match coords.len() {
      0 => None,
      1 => Some(Geometry::Point(coords[0], Metadata::default())),
      _ => Some(Geometry::LineString(coords, Metadata::default())),
    }
  }

  pub fn bounding_box(&self) -> BoundingBox {
    match self {
      Geometry::GeometryCollection(geometries, _) => geometries
        .iter()
        .map(Geometry::bounding_box)
        .fold(BoundingBox::default(), |acc, b| acc.extend(&b)),
      Geometry::Point(coord, _) => BoundingBox::from_iterator(once(*coord)),
      Geometry::LineString(coords, _) => coords
        .iter()
        .map(|c| BoundingBox::from_iterator(once(*c)))
        .fold(BoundingBox::default(), |acc, b| acc.extend(&b)),
      Geometry::Polygon(coords, _) => coords
        .iter()
        .map(|c| BoundingBox::from_iterator(once(*c)))
        .fold(BoundingBox::default(), |acc, b| acc.extend(&b)),
    }
  }

  pub fn is_visible(&self) -> bool {
    match self {
      Geometry::GeometryCollection(_, metadata)
      | Geometry::Point(_, metadata)
      | Geometry::Polygon(_, metadata)
      | Geometry::LineString(_, metadata) => metadata.style.as_ref().is_none_or(|s| s.visible),
    }
  }

  /// Check if this geometry should be visible at the given time
  pub fn is_visible_at_time(&self, current_time: DateTime<Utc>) -> bool {
    match self {
      Geometry::GeometryCollection(geometries, metadata) => {
        // For collections, check both the collection metadata and that at least one child is visible
        metadata.is_visible_at_time(current_time)
          && geometries
            .iter()
            .any(|g| g.is_visible_at_time(current_time))
      }
      Geometry::Point(_, metadata)
      | Geometry::LineString(_, metadata)
      | Geometry::Polygon(_, metadata) => {
        self.is_visible() && metadata.is_visible_at_time(current_time)
      }
    }
  }

  pub fn flat_iterate_with_merged_style(
    &self,
    base_style: &Style,
  ) -> impl Iterator<Item = Geometry<C>> + use<'_, C> {
    if let Geometry::GeometryCollection(geometries, metadata) = self {
      let style = base_style.optional_overwrite_with(metadata.style.as_ref());

      Either::Left(geometries.iter().cloned().flat_map(move |geometry| {
        geometry
          .flat_iterate_with_merged_style(&style)
          .collect::<Vec<_>>()
      }))
    } else {
      let style = base_style.optional_overwrite_with(self.get_style().as_ref());
      Either::Right(std::iter::once(self.clone().with_style(&style)))
    }
  }

  #[must_use]
  pub fn with_style(mut self, style: &Style) -> Self {
    match &mut self {
      Geometry::GeometryCollection(_, metadata)
      | Geometry::Point(_, metadata)
      | Geometry::Polygon(_, metadata)
      | Geometry::LineString(_, metadata) => {
        metadata.style = Some(style.clone());
      }
    }
    self
  }

  #[must_use]
  pub fn get_style(&self) -> &Option<Style> {
    match self {
      Geometry::GeometryCollection(_, metadata)
      | Geometry::Point(_, metadata)
      | Geometry::Polygon(_, metadata)
      | Geometry::LineString(_, metadata) => &metadata.style,
    }
  }
}

#[cfg(test)]
mod tests {

  use crate::map::coordinates::PixelCoordinate;

  use super::*;

  #[test]
  fn test_geometry_style_iterator() {
    let red = Metadata {
      label: None,
      style: Some(Style {
        color: Some(Color32::RED),
        fill_color: None,
        visible: false,
      }),
      heading: None,
      time_data: None,
    };
    let blue = Metadata {
      label: None,
      style: Some(Style {
        color: Some(Color32::BLUE),
        fill_color: Some(Color32::BLUE),
        visible: true,
      }),
      heading: None,
      time_data: None,
    };
    let green = Metadata {
      label: None,
      style: Some(Style {
        color: Some(Color32::GREEN),
        fill_color: Some(Color32::GREEN),
        visible: true,
      }),
      heading: None,
      time_data: None,
    };

    let geom_coll = Geometry::GeometryCollection(
      vec![
        Geometry::Point(PixelCoordinate::new(1., 2.), red.clone()),
        Geometry::LineString(
          vec![PixelCoordinate::new(3., 4.), PixelCoordinate::new(5., 6.)],
          blue.clone(),
        ),
        Geometry::GeometryCollection(
          vec![Geometry::Point(PixelCoordinate::new(3., 4.), red.clone())],
          Metadata::default(),
        ),
      ],
      green.clone(),
    );

    let elements = geom_coll
      .flat_iterate_with_merged_style(&Style::default())
      .collect::<Vec<_>>();

    let expected = vec![
      Geometry::Point(
        PixelCoordinate::new(1., 2.),
        Metadata {
          label: None,
          style: Some(Style {
            color: Some(Color::RED),
            fill_color: Some(Color::GREEN),
            visible: false,
          }),
          heading: None,
          time_data: None,
        },
      ),
      Geometry::LineString(
        vec![PixelCoordinate::new(3., 4.), PixelCoordinate::new(5., 6.)],
        Metadata {
          label: None,
          style: Some(Style {
            color: Some(Color::BLUE),
            fill_color: Some(Color::BLUE),
            visible: true,
          }),
          heading: None,
          time_data: None,
        },
      ),
      Geometry::Point(
        PixelCoordinate::new(3., 4.),
        Metadata {
          label: None,
          style: Some(Style {
            color: Some(Color::RED),
            fill_color: Some(Color::GREEN),
            visible: false,
          }),
          heading: None,
          time_data: None,
        },
      ),
    ];

    assert_eq!(elements, expected);
  }
}
