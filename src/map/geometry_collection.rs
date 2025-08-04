use std::iter::once;

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

#[derive(Clone, Default, PartialEq, Debug, Serialize, Deserialize)]
pub struct Metadata {
  pub label: Option<String>,
  pub style: Option<Style>,
  pub heading: Option<f32>,
}

impl Metadata {
  #[must_use]
  pub fn with_label(mut self, label: String) -> Self {
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
    };
    let blue = Metadata {
      label: None,
      style: Some(Style {
        color: Some(Color32::BLUE),
        fill_color: Some(Color32::BLUE),
        visible: true,
      }),
      heading: None,
    };
    let green = Metadata {
      label: None,
      style: Some(Style {
        color: Some(Color32::GREEN),
        fill_color: Some(Color32::GREEN),
        visible: true,
      }),
      heading: None,
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
        },
      ),
    ];

    assert_eq!(elements, expected);
  }
}
