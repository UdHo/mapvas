use super::{SearchProvider, SearchResult};
use crate::map::coordinates::WGS84Coordinate;
use anyhow::{Result, anyhow};
use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;

/// Built-in coordinate parser that handles various coordinate formats
pub struct CoordinateParser {
  // Common coordinate patterns
  decimal_regex: Regex,
  dms_regex: Regex,
}

impl Default for CoordinateParser {
  fn default() -> Self {
    Self::new()
  }
}

impl CoordinateParser {
  #[must_use]
  pub fn new() -> Self {
    Self {
      // Matches: "52.5, 13.4" or "52.5,13.4" or "52.5 13.4"
      decimal_regex: Regex::new(r"^\s*(-?\d+\.?\d*)\s*[,\s]\s*(-?\d+\.?\d*)\s*$").unwrap(),
      // Matches: "52°30'N 13°24'E" or "52° 30' N, 13° 24' E"
      dms_regex: Regex::new(r"^\s*(\d+)°\s*(\d+)'\s*([NS])\s*[,\s]\s*(\d+)°\s*(\d+)'\s*([EW])\s*$")
        .unwrap(),
    }
  }

  /// Try to parse a coordinate from various formats
  #[must_use]
  pub fn parse_coordinate(&self, input: &str) -> Option<SearchResult> {
    log::debug!("Attempting to parse coordinate: '{input}'");

    // Try decimal degrees first
    if let Some(coord) = self.parse_decimal(input) {
      log::debug!(
        "Successfully parsed decimal coordinate: {:.4}, {:.4}",
        coord.lat,
        coord.lon
      );
      return Some(SearchResult {
        name: format!("{:.4}°, {:.4}°", coord.lat, coord.lon),
        coordinate: coord,
        address: Some("Direct coordinate input".to_string()),
        country: None,
        relevance: 1.0,
      });
    }

    // Try degrees/minutes/seconds
    if let Some(coord) = self.parse_dms(input) {
      return Some(SearchResult {
        name: format!("{:.4}°, {:.4}°", coord.lat, coord.lon),
        coordinate: coord,
        address: Some("DMS coordinate input".to_string()),
        country: None,
        relevance: 1.0,
      });
    }

    log::debug!("Could not parse '{input}' as coordinate");
    None
  }

  fn parse_decimal(&self, input: &str) -> Option<WGS84Coordinate> {
    let caps = self.decimal_regex.captures(input)?;

    let lat: f32 = caps.get(1)?.as_str().parse().ok()?;
    let lon: f32 = caps.get(2)?.as_str().parse().ok()?;

    // Validate coordinate ranges
    if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) {
      Some(WGS84Coordinate::new(lat, lon))
    } else {
      None
    }
  }

  fn parse_dms(&self, input: &str) -> Option<WGS84Coordinate> {
    let caps = self.dms_regex.captures(input)?;

    let lat_deg: f32 = caps.get(1)?.as_str().parse().ok()?;
    let lat_min: f32 = caps.get(2)?.as_str().parse().ok()?;
    let lat_dir = caps.get(3)?.as_str();

    let lon_deg: f32 = caps.get(4)?.as_str().parse().ok()?;
    let lon_min: f32 = caps.get(5)?.as_str().parse().ok()?;
    let lon_dir = caps.get(6)?.as_str();

    let mut lat = lat_deg + lat_min / 60.0;
    let mut lon = lon_deg + lon_min / 60.0;

    if lat_dir == "S" {
      lat = -lat;
    }
    if lon_dir == "W" {
      lon = -lon;
    }

    Some(WGS84Coordinate::new(lat, lon))
  }
}

/// OpenStreetMap Nominatim provider (free)
pub struct NominatimProvider {
  base_url: String,
  client: surf::Client,
}

impl NominatimProvider {
  #[must_use]
  pub fn new(base_url: Option<String>) -> Self {
    Self {
      base_url: base_url.unwrap_or_else(|| "https://nominatim.openstreetmap.org".to_string()),
      client: surf::Client::new(),
    }
  }
}

#[async_trait::async_trait]
impl SearchProvider for NominatimProvider {
  fn name(&self) -> &'static str {
    "OpenStreetMap Nominatim"
  }

  async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
    let url = format!(
      "{}/search?format=json&limit=5&addressdetails=1&q={}",
      self.base_url,
      urlencoding::encode(query)
    );

    let response = self
      .client
      .get(&url)
      .header(
        "User-Agent",
        "MapVas/0.2.4 (https://github.com/UdHo/mapvas)",
      )
      .recv_json::<Value>()
      .await
      .map_err(|e| anyhow!("Nominatim API request failed: {}", e))?;

    let mut results = Vec::new();

    if let Some(items) = response.as_array() {
      for item in items {
        if let (Some(lat), Some(lon), Some(display_name)) = (
          item["lat"].as_str().and_then(|s| s.parse::<f32>().ok()),
          item["lon"].as_str().and_then(|s| s.parse::<f32>().ok()),
          item["display_name"].as_str(),
        ) {
          let coordinate = WGS84Coordinate::new(lat, lon);

          // Extract address components
          let address = item["address"].as_object();
          let country = address
            .and_then(|a| a["country"].as_str())
            .map(std::string::ToString::to_string);

          // Calculate relevance based on importance
          #[allow(clippy::cast_possible_truncation)]
          let relevance = item["importance"].as_f64().unwrap_or(0.5) as f32;

          results.push(SearchResult {
            name: display_name.to_string(),
            coordinate,
            address: Some(display_name.to_string()),
            country,
            relevance,
          });
        }
      }
    }

    Ok(results)
  }

  fn supports_reverse(&self) -> bool {
    true
  }

  async fn reverse(&self, coord: WGS84Coordinate) -> Result<Option<SearchResult>> {
    let url = format!(
      "{}/reverse?format=json&lat={}&lon={}&addressdetails=1",
      self.base_url, coord.lat, coord.lon
    );

    let response = self
      .client
      .get(&url)
      .header(
        "User-Agent",
        "MapVas/0.2.4 (https://github.com/UdHo/mapvas)",
      )
      .recv_json::<Value>()
      .await
      .map_err(|e| anyhow!("Nominatim reverse API request failed: {}", e))?;

    if let Some(display_name) = response["display_name"].as_str() {
      let address = response["address"].as_object();
      let country = address
        .and_then(|a| a["country"].as_str())
        .map(std::string::ToString::to_string);

      Ok(Some(SearchResult {
        name: display_name.to_string(),
        coordinate: coord,
        address: Some(display_name.to_string()),
        country,
        relevance: 1.0,
      }))
    } else {
      Ok(None)
    }
  }
}

/// Custom API provider for other geocoding services
pub struct CustomProvider {
  name: String,
  url_template: String,
  headers: HashMap<String, String>,
  client: surf::Client,
}

impl CustomProvider {
  #[must_use]
  pub fn new(name: String, url_template: String, headers: Option<HashMap<String, String>>) -> Self {
    Self {
      name,
      url_template,
      headers: headers.unwrap_or_default(),
      client: surf::Client::new(),
    }
  }
}

#[async_trait::async_trait]
impl SearchProvider for CustomProvider {
  fn name(&self) -> &str {
    &self.name
  }

  async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
    let url = self
      .url_template
      .replace("{query}", &urlencoding::encode(query));

    let mut request = self.client.get(&url);

    // Add custom headers
    for (key, value) in &self.headers {
      request = request.header(key.as_str(), value.as_str());
    }

    let response = request
      .recv_json::<Value>()
      .await
      .map_err(|e| anyhow!("Custom API request failed: {}", e))?;

    // This is a basic implementation - could be extended to support
    // different response formats based on configuration
    let mut results = Vec::new();

    // Try to parse as GeoJSON-like structure
    if let Some(features) = response["features"].as_array() {
      for feature in features {
        if let (Some(coords), Some(props)) = (
          feature["geometry"]["coordinates"].as_array(),
          feature["properties"].as_object(),
        ) && coords.len() >= 2
        {
          #[allow(clippy::cast_possible_truncation)]
          if let (Some(lon), Some(lat)) = (
            coords[0].as_f64().map(|f| f as f32),
            coords[1].as_f64().map(|f| f as f32),
          ) {
            let coordinate = WGS84Coordinate::new(lat, lon);

            let name = props["name"]
              .as_str()
              .unwrap_or("Unknown location")
              .to_string();

            let address = props["address"]
              .as_str()
              .map(std::string::ToString::to_string);
            let country = props["country"]
              .as_str()
              .map(std::string::ToString::to_string);
            #[allow(clippy::cast_possible_truncation)]
            let relevance = props["relevance"].as_f64().unwrap_or(0.5) as f32;

            results.push(SearchResult {
              name,
              coordinate,
              address,
              country,
              relevance,
            });
          }
        }
      }
    }

    Ok(results)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_coordinate_parsing() {
    let parser = CoordinateParser::new();

    // Test decimal degrees
    assert!(parser.parse_coordinate("52.5, 13.4").is_some());
    assert!(parser.parse_coordinate("52.5,13.4").is_some());
    assert!(parser.parse_coordinate("52.5 13.4").is_some());
    assert!(parser.parse_coordinate("-52.5, -13.4").is_some());

    // Test invalid coordinates
    assert!(parser.parse_coordinate("invalid").is_none());
    assert!(parser.parse_coordinate("200, 13.4").is_none()); // Lat > 90
    assert!(parser.parse_coordinate("52.5, 200").is_none()); // Lon > 180

    // Test DMS format
    assert!(parser.parse_coordinate("52°30'N 13°24'E").is_some());
    assert!(parser.parse_coordinate("52°30'N, 13°24'E").is_some());
  }

  #[test]
  fn test_decimal_coordinate_values() {
    let parser = CoordinateParser::new();

    if let Some(result) = parser.parse_coordinate("52.5, 13.4") {
      assert!((result.coordinate.lat - 52.5).abs() < 0.001);
      assert!((result.coordinate.lon - 13.4).abs() < 0.001);
      assert!((result.relevance - 1.0).abs() < f32::EPSILON);
    } else {
      panic!("Should have parsed coordinate");
    }
  }
}
