use crate::{APP_NAME, VERSION, models::SavedLocation};
use async_trait::async_trait;
use once_cell::sync::Lazy;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{path::PathBuf, str::FromStr, sync::Arc, time::Duration};
use thiserror::Error;
use tokio::sync::Mutex;

static COUNTRY_FINDER: Lazy<reverse_geocoder::ReverseGeocoder> =
    Lazy::new(reverse_geocoder::ReverseGeocoder::new);

#[derive(Debug, Error)]
pub enum GeocodeError {
    #[error("enter a city or latitude, longitude")]
    Empty,
    #[error("coordinates must be latitude, longitude within valid ranges")]
    InvalidCoordinates,
    #[error("location search is temporarily unavailable: {0}")]
    Network(String),
    #[error("location search returned malformed data")]
    Malformed,
}
#[async_trait]
pub trait Geocoder: Send + Sync {
    async fn search(&self, query: &str) -> Result<Vec<SavedLocation>, GeocodeError>;
}

#[derive(Clone)]
pub struct OpenMeteoGeocoder {
    client: reqwest::Client,
}

impl OpenMeteoGeocoder {
    pub fn new() -> Result<Self, GeocodeError> {
        let client = reqwest::Client::builder()
            .user_agent(format!("{APP_NAME}/{VERSION} (desktop location search)"))
            .timeout(Duration::from_secs(12))
            .build()
            .map_err(|error| GeocodeError::Network(error.to_string()))?;
        Ok(Self { client })
    }
}

#[derive(Deserialize)]
struct OpenMeteoSearch {
    #[serde(default)]
    results: Vec<OpenMeteoPlace>,
}
#[derive(Deserialize)]
struct OpenMeteoPlace {
    name: String,
    latitude: f64,
    longitude: f64,
    #[serde(default)]
    country_code: String,
    timezone: Option<String>,
}

#[async_trait]
impl Geocoder for OpenMeteoGeocoder {
    async fn search(&self, query: &str) -> Result<Vec<SavedLocation>, GeocodeError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(GeocodeError::Empty);
        }
        if let Some((latitude, longitude)) = parse_coordinates(query) {
            return Ok(vec![coordinate_location(latitude, longitude)]);
        }
        let response = self
            .client
            .get("https://geocoding-api.open-meteo.com/v1/search")
            .query(&[
                ("name", query),
                ("count", "8"),
                ("language", "en"),
                ("format", "json"),
            ])
            .send()
            .await
            .map_err(|error| GeocodeError::Network(error.to_string()))?;
        if !response.status().is_success() {
            return Err(GeocodeError::Network(format!(
                "HTTP {}",
                response.status().as_u16()
            )));
        }
        let response: OpenMeteoSearch =
            response.json().await.map_err(|_| GeocodeError::Malformed)?;
        Ok(response
            .results
            .into_iter()
            .map(|place| {
                let timezone = place
                    .timezone
                    .unwrap_or_else(|| timezone_for(place.latitude, place.longitude));
                let mut location = SavedLocation::new(
                    place.name,
                    place.country_code.to_uppercase(),
                    timezone,
                    place.latitude,
                    place.longitude,
                );
                location.sort_order = -1;
                location
            })
            .collect())
    }
}

#[derive(Clone)]
pub struct NominatimGeocoder {
    client: reqwest::Client,
    endpoint: String,
    last_request: Arc<Mutex<Option<tokio::time::Instant>>>,
    cache_root: Option<PathBuf>,
}
impl NominatimGeocoder {
    pub fn new(endpoint: impl Into<String>) -> Result<Self, GeocodeError> {
        let client = reqwest::Client::builder()
            .user_agent(format!(
                "{APP_NAME}/{VERSION} (desktop weather app; io.github.weatherglass.Weatherglass)"
            ))
            .timeout(Duration::from_secs(12))
            .build()
            .map_err(|e| GeocodeError::Network(e.to_string()))?;
        Ok(Self {
            client,
            endpoint: endpoint.into(),
            last_request: Arc::new(Mutex::new(None)),
            cache_root: directories::ProjectDirs::from("io", "Weatherglass", "Weatherglass")
                .map(|d| d.cache_dir().join("geocoder")),
        })
    }
    async fn rate_limit(&self) {
        let mut last = self.last_request.lock().await;
        if let Some(at) = *last {
            let elapsed = at.elapsed();
            if elapsed < Duration::from_secs(1) {
                tokio::time::sleep(Duration::from_secs(1) - elapsed).await;
            }
        }
        *last = Some(tokio::time::Instant::now());
    }
}
#[derive(Deserialize)]
struct NominatimResult {
    display_name: String,
    lat: String,
    lon: String,
    address: Option<NominatimAddress>,
}
#[derive(Deserialize)]
struct NominatimAddress {
    country_code: Option<String>,
    city: Option<String>,
    town: Option<String>,
    village: Option<String>,
    municipality: Option<String>,
}

#[async_trait]
impl Geocoder for NominatimGeocoder {
    async fn search(&self, query: &str) -> Result<Vec<SavedLocation>, GeocodeError> {
        let q = query.trim();
        if q.is_empty() {
            return Err(GeocodeError::Empty);
        }
        if let Some((lat, lon)) = parse_coordinates(q) {
            return Ok(vec![coordinate_location(lat, lon)]);
        }
        let cache_path = self.cache_root.as_ref().map(|root| {
            root.join(format!(
                "{:x}.json",
                Sha256::digest(q.to_lowercase().as_bytes())
            ))
        });
        if let Some(path) = &cache_path
            && let Ok(bytes) = tokio::fs::read(path).await
            && let Ok(rows) = serde_json::from_slice::<Vec<SavedLocation>>(&bytes)
        {
            return Ok(rows);
        }
        self.rate_limit().await;
        let response = self
            .client
            .get(format!("{}/search", self.endpoint.trim_end_matches('/')))
            .query(&[
                ("q", q),
                ("format", "jsonv2"),
                ("addressdetails", "1"),
                ("limit", "8"),
                ("featuretype", "city"),
            ])
            .send()
            .await
            .map_err(|e| GeocodeError::Network(e.to_string()))?;
        if !response.status().is_success() {
            return Err(GeocodeError::Network(format!(
                "HTTP {}",
                response.status().as_u16()
            )));
        }
        let rows: Vec<NominatimResult> =
            response.json().await.map_err(|_| GeocodeError::Malformed)?;
        let mapped = rows
            .into_iter()
            .filter_map(|r| {
                let lat = f64::from_str(&r.lat).ok()?;
                let lon = f64::from_str(&r.lon).ok()?;
                let a = r.address;
                let name = a
                    .as_ref()
                    .and_then(|a| {
                        a.city
                            .as_ref()
                            .or(a.town.as_ref())
                            .or(a.village.as_ref())
                            .or(a.municipality.as_ref())
                    })
                    .cloned()
                    .unwrap_or_else(|| {
                        r.display_name
                            .split(',')
                            .next()
                            .unwrap_or(&r.display_name)
                            .to_string()
                    });
                let country = a
                    .and_then(|a| a.country_code)
                    .unwrap_or_default()
                    .to_uppercase();
                let mut l = SavedLocation::new(name, country, timezone_for(lat, lon), lat, lon);
                l.sort_order = -1;
                Some(l)
            })
            .collect::<Vec<_>>();
        if let Some(path) = cache_path {
            if let Some(parent) = path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            if let Ok(bytes) = serde_json::to_vec(&mapped) {
                let _ = tokio::fs::write(path, bytes).await;
            }
        }
        Ok(mapped)
    }
}
pub fn parse_coordinates(value: &str) -> Option<(f64, f64)> {
    let normalized = value.replace(';', ",");
    let p: Vec<_> = normalized.split(',').map(str::trim).collect();
    if p.len() != 2 {
        return None;
    }
    let lat = p[0].parse().ok()?;
    let lon = p[1].parse().ok()?;
    if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) {
        Some((lat, lon))
    } else {
        None
    }
}
pub fn timezone_for(lat: f64, lon: f64) -> String {
    let finder = tzf_rs::DefaultFinder::new();
    let name = finder.get_tz_name(lon, lat);
    if name.is_empty() {
        "Etc/UTC".to_string()
    } else {
        name.to_string()
    }
}
pub fn country_for(lat: f64, lon: f64) -> String {
    COUNTRY_FINDER.search((lat, lon)).record.cc.clone()
}
fn coordinate_location(lat: f64, lon: f64) -> SavedLocation {
    let mut l = SavedLocation::new(
        format!("{lat:.3}°, {lon:.3}°"),
        country_for(lat, lon),
        timezone_for(lat, lon),
        lat,
        lon,
    );
    l.sort_order = -1;
    l
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coordinates_and_timezone() {
        assert_eq!(parse_coordinates("41.88, -87.63"), Some((41.88, -87.63)));
        assert!(parse_coordinates("91, 4").is_none());
        assert_eq!(timezone_for(41.88, -87.63), "America/Chicago");
        assert_eq!(country_for(41.88, -87.63), "US");
    }
}
