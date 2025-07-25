pub mod providers;
pub mod ui;

use crate::map::coordinates::WGS84Coordinate;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fmt::Display;

/// A search result containing location information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub name: String,
    pub coordinate: WGS84Coordinate,
    pub address: Option<String>,
    pub country: Option<String>,
    pub relevance: f32, // 0.0 to 1.0
}

impl Display for SearchResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.address {
            Some(addr) => write!(f, "{} - {}", self.name, addr),
            None => write!(f, "{}", self.name),
        }
    }
}

/// Trait for different location search providers
#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    /// Human-readable name of the provider
    fn name(&self) -> &str;
    
    /// Search for locations based on query string
    async fn search(&self, query: &str) -> Result<Vec<SearchResult>>;
    
    /// Whether this provider supports reverse geocoding
    fn supports_reverse(&self) -> bool {
        false
    }
    
    /// Reverse geocode a coordinate to get place information
    async fn reverse(&self, coord: WGS84Coordinate) -> Result<Option<SearchResult>> {
        let _ = coord;
        Ok(None)
    }
}

/// Configuration for search providers
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SearchProviderConfig {
    /// Built-in coordinate parser (no API required)
    Coordinate,
    /// OpenStreetMap Nominatim (free, no API key)
    Nominatim { base_url: Option<String> },
    /// Custom API provider
    Custom { 
        name: String,
        url_template: String,
        headers: Option<std::collections::HashMap<String, String>>,
    },
}

impl Default for SearchProviderConfig {
    fn default() -> Self {
        Self::Nominatim { base_url: None }
    }
}

/// Main search manager that coordinates different providers
pub struct SearchManager {
    providers: Vec<Box<dyn SearchProvider>>,
    coordinate_parser: providers::CoordinateParser,
}

impl SearchManager {
    /// Create a new search manager with default providers
    pub fn new() -> Self {
        Self {
            providers: vec![
                Box::new(providers::NominatimProvider::new(None)),
            ],
            coordinate_parser: providers::CoordinateParser::new(),
        }
    }
    
    /// Create search manager with custom configuration
    pub fn with_config(configs: Vec<SearchProviderConfig>) -> Result<Self> {
        let mut providers: Vec<Box<dyn SearchProvider>> = Vec::new();
        
        for config in configs {
            match config {
                SearchProviderConfig::Nominatim { base_url } => {
                    providers.push(Box::new(providers::NominatimProvider::new(base_url)));
                }
                SearchProviderConfig::Custom { name, url_template, headers } => {
                    providers.push(Box::new(providers::CustomProvider::new(name, url_template, headers)));
                }
                SearchProviderConfig::Coordinate => {
                    // Coordinate parser is always available, no need to add
                }
            }
        }
        
        // Always include at least one provider
        if providers.is_empty() {
            providers.push(Box::new(providers::NominatimProvider::new(None)));
        }
        
        Ok(Self {
            providers,
            coordinate_parser: providers::CoordinateParser::new(),
        })
    }
    
    /// Search across all providers, with coordinate parsing taking priority
    pub async fn search(&self, query: &str) -> Result<Vec<SearchResult>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(vec![]);
        }
        
        // Try coordinate parsing first (fastest and most precise)
        if let Some(result) = self.coordinate_parser.parse_coordinate(query) {
            log::info!("Parsed coordinate input: '{}' -> {:.4}, {:.4}", 
                query, result.coordinate.lat, result.coordinate.lon);
            return Ok(vec![result]);
        }
        
        // Search through configured providers
        log::debug!("Starting provider search for: '{}'", query);
        let mut all_results = Vec::new();
        
        for provider in &self.providers {
            log::debug!("Searching with provider: {}", provider.name());
            match provider.search(query).await {
                Ok(mut results) => {
                    log::debug!("Provider '{}' returned {} results", provider.name(), results.len());
                    // Limit results per provider to avoid overwhelming
                    results.truncate(5);
                    all_results.extend(results);
                }
                Err(e) => {
                    log::warn!("Search provider '{}' failed: {}", provider.name(), e);
                }
            }
        }
        
        // Sort by relevance (highest first)
        all_results.sort_by(|a, b| b.relevance.partial_cmp(&a.relevance).unwrap_or(std::cmp::Ordering::Equal));
        
        // Limit total results
        all_results.truncate(10);
        
        log::debug!("Search completed for '{}': {} total results", query, all_results.len());
        Ok(all_results)
    }
    
    /// Get list of available provider names
    pub fn provider_names(&self) -> Vec<String> {
        let mut names = vec!["Coordinates".to_string()];
        names.extend(self.providers.iter().map(|p| p.name().to_string()));
        names
    }
}

impl Default for SearchManager {
    fn default() -> Self {
        Self::new()
    }
}