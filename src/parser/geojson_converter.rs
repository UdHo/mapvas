use crate::map::{
  coordinates::{Coordinate, PixelPosition},
  geometry_collection::{Geometry, Metadata},
};

use super::geojson::{self, GeoJson};

impl From<GeoJson> for Geometry<PixelPosition> {
  fn from(geojson: GeoJson) -> Self {
    match geojson {
      GeoJson::FeatureCollection(fc) => {
        let mut geometries = Vec::new();
        for feature in fc.features {
          geometries.push(feature.into());
        }
        Geometry::GeometryCollection(geometries, Metadata::default())
      }
      GeoJson::Feature(feature) => {
        Geometry::GeometryCollection(vec![feature.into()], Metadata::default())
      }
      GeoJson::Geometry(geometry) => Geometry::GeometryCollection(vec![], Metadata::default()),
    }
  }
}

impl TryFrom<geojson::Feature> for Geometry<PixelPosition> {
  type Error = ();

  fn try_from(value: geojson::Feature) -> Result<Self, Self::Error> {
    // TODO parse more stuff
    Ok(Geometry::try_from(value.geometry)?)
  }
}

impl TryFrom<geojson::Geometry> for Geometry<PixelPosition> {
  fn try_from(value: geojson::Geometry) -> Result<Self, Self::Error> {
    match value {
      geojson::Geometry::Point { coordinates } => Ok(Geometry::Point(
        coordinates.as_pixel_position(),
        Metadata::default(),
      )),
      geojson::Geometry::LineString { coordinates } => Ok(Geometry::LineString(
        coordinates
          .into_iter()
          .map(|c| c.as_pixel_position())
          .collect(),
        Metadata::default(),
      )),
      geojson::Geometry::Polygon { coordinates } => Ok(Geometry::Polygon(
        // TODO: Support holes?
        coordinates
          .first()
          .ok_or(())?
          .iter()
          .map(Coordinate::as_pixel_position)
          .collect(),
        Metadata::default(),
      )),
      geojson::Geometry::MultiPoint { coordinates } => Ok(Geometry::GeometryCollection(
        coordinates
          .iter()
          .map(|c| Geometry::Point(c.as_pixel_position(), Metadata::default()))
          .collect(),
        Metadata::default(),
      )),
      geojson::Geometry::MultiLineString { coordinates } => Ok(Geometry::GeometryCollection(
        coordinates
          .iter()
          .map(|c| {
            Geometry::LineString(
              c.iter().map(Coordinate::as_pixel_position).collect(),
              Metadata::default(),
            )
          })
          .collect(),
        Metadata::default(),
      )),
      geojson::Geometry::MultiPolygon { coordinates } => Ok(Geometry::GeometryCollection(
        coordinates
          .into_iter()
          .filter_map(|poly| {
            geojson::Geometry::Polygon { coordinates: poly }
              .try_into()
              .ok()
          })
          .collect(),
        Metadata::default(),
      )),
      geojson::Geometry::GeometryCollection { geometries } => Ok(Geometry::GeometryCollection(
        geometries
          .into_iter()
          .filter_map(|g| g.try_into().ok())
          .collect(),
        Metadata::default(),
      )),
    }
  }

  type Error = ();
}

/*impl From<&Feature> for Box<dyn Iterator<Item = Shape>> {
  fn from(feature: &Feature) -> Self {
    match feature {
        Feature::

    }
  }
}*/
