use crate::map::map_event::{Layer, Shape};

use super::geojson::{Feature, GeoJson};

impl From<GeoJson> for Layer {
  fn from(geojson: GeoJson) -> Self {
    let mut layer = Self::new("geojson".to_string());

    for feature in geojson.features() {}
    layer
  }
}

/*impl From<&Feature> for Box<dyn Iterator<Item = Shape>> {
  fn from(feature: &Feature) -> Self {
    match feature {
        Feature::

    }
  }
}*/
