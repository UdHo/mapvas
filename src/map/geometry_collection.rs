use egui::Color32;
use itertools::Either;

use super::coordinates::Coordinate;

type Color = Color32;

#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct Style {
  color: Option<Color>,
  fill_color: Option<Color>,
}

impl Style {
  fn overwrite_with(&self, style: &Style) -> Style {
    Style {
      color: style.color.or(self.color),
      fill_color: style.fill_color.or(self.fill_color),
    }
  }
  fn optional_overwrite_with(&self, style: Option<&Style>) -> Style {
    style.map_or_else(|| self.clone(), |s| self.overwrite_with(s))
  }
}

#[derive(Clone, Default, Eq, PartialEq, Debug)]
pub struct Metadata {
  pub label: Option<String>,
  pub style: Option<Style>,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub enum Geometry<C: Coordinate> {
  GeometryCollection(Vec<Geometry<C>>, Metadata),
  Point(C, Metadata),
  LineString(Vec<C>, Metadata),
}

impl<C: Coordinate> Geometry<C> {
  pub fn flat_iterate_with_merged_style(
    &self,
    base_style: &Style,
  ) -> impl Iterator<Item = Geometry<C>> + use<'_, C> {
    match self {
      Geometry::GeometryCollection(geometries, metadata) => {
        let style = base_style.optional_overwrite_with(metadata.style.as_ref());
        let res = Either::Left(geometries.iter().cloned().flat_map(move |geometry| {
          geometry
            .flat_iterate_with_merged_style(&style)
            .collect::<Vec<_>>()
        }));
        res
      }
      Geometry::Point(_, _) | Geometry::LineString(_, _) => {
        let style = base_style.optional_overwrite_with(self.get_style().as_ref());
        Either::Right(std::iter::once(self.clone().with_style(&style)))
      }
    }
  }

  #[must_use]
  pub fn with_style(mut self, style: &Style) -> Self {
    match &mut self {
      Geometry::GeometryCollection(_, metadata)
      | Geometry::Point(_, metadata)
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
      | Geometry::LineString(_, metadata) => &metadata.style,
    }
  }
}

#[cfg(test)]
mod tests {

  use crate::map::coordinates::PixelPosition;

  use super::*;

  #[test]
  fn test_geometry_style_iterator() {
    let red = Metadata {
      label: None,
      style: Some(Style {
        color: Some(Color32::RED),
        fill_color: None,
      }),
    };
    let blue = Metadata {
      label: None,
      style: Some(Style {
        color: Some(Color32::BLUE),
        fill_color: Some(Color32::BLUE),
      }),
    };
    let green = Metadata {
      label: None,
      style: Some(Style {
        color: Some(Color32::GREEN),
        fill_color: Some(Color32::GREEN),
      }),
    };

    let geom_coll = Geometry::GeometryCollection(
      vec![
        Geometry::Point(PixelPosition::new(1., 2.), red.clone()),
        Geometry::LineString(
          vec![PixelPosition::new(3., 4.), PixelPosition::new(5., 6.)],
          blue.clone(),
        ),
        Geometry::GeometryCollection(
          vec![Geometry::Point(PixelPosition::new(3., 4.), red.clone())],
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
        PixelPosition::new(1., 2.),
        Metadata {
          label: None,
          style: Some(Style {
            color: Some(Color::RED),
            fill_color: Some(Color::GREEN),
          }),
        },
      ),
      Geometry::LineString(
        vec![PixelPosition::new(3., 4.), PixelPosition::new(5., 6.)],
        Metadata {
          label: None,
          style: Some(Style {
            color: Some(Color::BLUE),
            fill_color: Some(Color::BLUE),
          }),
        },
      ),
      Geometry::Point(
        PixelPosition::new(3., 4.),
        Metadata {
          label: None,
          style: Some(Style {
            color: Some(Color::RED),
            fill_color: Some(Color::GREEN),
          }),
        },
      ),
    ];

    assert_eq!(elements, expected);
  }
}
