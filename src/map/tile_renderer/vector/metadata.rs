use std::collections::HashMap;

use mvt_reader::feature::{Feature, Value};

pub(crate) fn feature_string_property<'a>(feature: &'a Feature, keys: &[&str]) -> Option<&'a str> {
  let properties = feature.properties.as_ref()?;
  keys.iter().find_map(|key| match properties.get(*key) {
    Some(Value::String(value)) => Some(value.as_str()),
    _ => None,
  })
}

pub(crate) fn label_feature_kind_detail<'a>(
  layer_name: &str,
  feature_kind_detail: Option<&'a str>,
  feature_kind: Option<&'a str>,
  feature_class: &'a str,
) -> Option<&'a str> {
  feature_kind_detail.or(feature_kind).or_else(|| {
    if layer_name != "place" {
      return None;
    }
    match feature_class {
      "country" | "city" | "locality" | "town" | "village" => Some(feature_class),
      "hamlet" => Some("village"),
      "suburb" | "neighbourhood" | "quarter" => Some("locality"),
      _ => None,
    }
  })
}

pub(crate) fn label_population_rank(feature: &Feature, feature_class: &str) -> Option<i64> {
  let properties = feature.properties.as_ref()?;
  feature_i64_property(properties, &["population_rank", "pop_rank"])
    .map(normalized_population_rank)
    .or_else(|| {
      feature_i64_property(properties, &["rank"])
        .map(|rank| normalized_openfreemap_place_rank(feature_class, rank))
    })
    .or_else(|| {
      feature_i64_property(properties, &["scalerank"])
        .map(|rank| normalized_openfreemap_place_rank(feature_class, rank))
    })
    .or_else(|| {
      feature_i64_property(properties, &["population", "pop_max", "pop_min"])
        .and_then(population_to_place_rank)
    })
}

pub(crate) fn feature_is_national_capital(feature: &Feature) -> bool {
  let Some(properties) = feature.properties.as_ref() else {
    return false;
  };
  ["capital", "capital:type", "capital_type", "is_capital"]
    .iter()
    .any(|key| {
      properties
        .get(*key)
        .is_some_and(feature_value_is_national_capital)
    })
}

fn feature_i64_property(properties: &HashMap<String, Value>, keys: &[&str]) -> Option<i64> {
  keys
    .iter()
    .find_map(|key| properties.get(*key).and_then(feature_value_as_i64))
}

fn normalized_openfreemap_place_rank(feature_class: &str, rank: i64) -> i64 {
  if !matches!(feature_class, "city" | "locality") {
    return rank;
  }

  match rank {
    i64::MIN..=3 => 13,
    4..=7 => 14,
    8..=11 => 15,
    12..=14 => 16,
    _ => 17,
  }
}

fn normalized_population_rank(rank: i64) -> i64 {
  match rank {
    14..=i64::MAX => 13,
    13 => 14,
    12 => 15,
    11 => 16,
    _ => 17,
  }
}

fn population_to_place_rank(population: i64) -> Option<i64> {
  match population {
    i64::MIN..=0 => None,
    10_000_000.. => Some(13),
    5_000_000..=9_999_999 => Some(14),
    1_000_000..=4_999_999 => Some(15),
    500_000..=999_999 => Some(16),
    _ => Some(17),
  }
}

fn feature_value_as_i64(value: &Value) -> Option<i64> {
  match value {
    Value::UInt(value) => i64::try_from(*value).ok(),
    Value::Int(value) | Value::SInt(value) => Some(*value),
    Value::Float(value) => finite_number_to_i64(f64::from(*value)),
    Value::Double(value) => finite_number_to_i64(*value),
    Value::String(value) => parse_number_string(value).and_then(finite_number_to_i64),
    _ => None,
  }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
fn finite_number_to_i64(value: f64) -> Option<i64> {
  if value.is_finite() && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
    Some(value.round() as i64)
  } else {
    None
  }
}

fn parse_number_string(value: &str) -> Option<f64> {
  let normalized = value
    .trim()
    .chars()
    .filter(|character| !matches!(character, ',' | '_' | ' '))
    .collect::<String>();
  if normalized.is_empty() {
    return None;
  }
  normalized.parse::<f64>().ok()
}

fn feature_value_is_national_capital(value: &Value) -> bool {
  match value {
    Value::Bool(value) => *value,
    Value::UInt(value) => matches!(*value, 1 | 2),
    Value::Int(value) | Value::SInt(value) => matches!(*value, 1 | 2),
    Value::String(value) => {
      let value = value.trim().to_ascii_lowercase();
      matches!(
        value.as_str(),
        "yes"
          | "true"
          | "capital"
          | "national"
          | "national_capital"
          | "country"
          | "country_capital"
          | "primary"
      ) || value.parse::<i64>().is_ok_and(|rank| matches!(rank, 1 | 2))
    }
    _ => false,
  }
}

#[cfg(test)]
mod tests {
  use std::collections::HashMap;

  use super::*;

  fn feature_with_properties(
    properties: impl IntoIterator<Item = (&'static str, Value)>,
  ) -> Feature {
    Feature {
      geometry: geo_types::Geometry::Point(geo_types::Point::new(0.0, 0.0)),
      id: None,
      properties: Some(
        properties
          .into_iter()
          .map(|(key, value)| (key.to_string(), value))
          .collect::<HashMap<_, _>>(),
      ),
    }
  }

  #[test]
  fn openfreemap_city_rank_maps_to_place_size_buckets() {
    assert_eq!(normalized_openfreemap_place_rank("city", 2), 13);
    assert_eq!(normalized_openfreemap_place_rank("city", 11), 15);
    assert_eq!(normalized_openfreemap_place_rank("city", 15), 17);
    assert_eq!(normalized_openfreemap_place_rank("town", 2), 2);
  }

  #[test]
  fn population_property_falls_back_to_place_rank() {
    let berlin = feature_with_properties([("population", Value::UInt(3_850_809))]);
    let kaiserslautern =
      feature_with_properties([("population", Value::String("99,662".to_string()))]);

    assert_eq!(label_population_rank(&berlin, "city"), Some(15));
    assert_eq!(label_population_rank(&kaiserslautern, "city"), Some(17));
  }

  #[test]
  fn population_rank_uses_higher_values_as_more_important() {
    let berlin = feature_with_properties([
      ("population_rank", Value::UInt(12)),
      ("population", Value::UInt(3_850_809)),
    ]);
    let kaiserslautern = feature_with_properties([
      ("population_rank", Value::UInt(8)),
      ("population", Value::UInt(97_162)),
    ]);

    assert_eq!(label_population_rank(&berlin, "city"), Some(15));
    assert_eq!(label_population_rank(&kaiserslautern, "city"), Some(17));
  }

  #[test]
  fn capital_rank_distinguishes_national_capitals_from_admin_centers() {
    assert!(feature_value_is_national_capital(&Value::UInt(2)));
    assert!(feature_value_is_national_capital(&Value::String(
      "yes".to_string()
    )));
    assert!(!feature_value_is_national_capital(&Value::UInt(4)));
    assert!(!feature_value_is_national_capital(&Value::UInt(6)));
    assert!(!feature_value_is_national_capital(&Value::String(
      "4".to_string()
    )));
  }

  #[test]
  fn numeric_strings_and_float_values_parse_as_ranks() {
    assert_eq!(
      feature_value_as_i64(&Value::String("15.0".to_string())),
      Some(15)
    );
    assert_eq!(
      feature_value_as_i64(&Value::String("3,850,809".to_string())),
      Some(3_850_809)
    );
    assert_eq!(feature_value_as_i64(&Value::Double(16.0)), Some(16));
  }
}
