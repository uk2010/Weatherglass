use chrono::{DateTime, Utc};
use futures::future::try_join_all;
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::{collections::HashMap, time::Duration};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum RadarError {
    #[error("radar network is unavailable: {0}")]
    Network(String),
    #[error("radar service returned no frames")]
    NoFrames,
}

#[derive(Clone, Debug)]
pub struct RadarTile {
    pub column: i32,
    pub row: i32,
    pub base_png: Vec<u8>,
    pub radar_png: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct RadarMap {
    pub tiles: Vec<RadarTile>,
    pub observed_at: DateTime<Utc>,
    pub focus_x: f64,
    pub focus_y: f64,
}

type RadarCacheKey = (u64, u64, u8, i32, usize);
type RadarCache = HashMap<RadarCacheKey, (std::time::Instant, Vec<RadarMap>)>;
static RADAR_CACHE: Lazy<Mutex<RadarCache>> = Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Clone)]
pub struct RadarClient {
    http: reqwest::Client,
    metadata_url: String,
    osm_base: String,
}

#[derive(Deserialize)]
struct Metadata {
    host: String,
    radar: RadarFrames,
}
#[derive(Deserialize)]
struct RadarFrames {
    past: Vec<Frame>,
}
#[derive(Deserialize)]
struct Frame {
    time: i64,
    path: String,
}

impl RadarClient {
    pub fn new() -> Result<Self, RadarError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(18))
            .user_agent(format!(
                "Weatherglass/{} (personal desktop radar viewer)",
                crate::VERSION
            ))
            .build()
            .map_err(|error| RadarError::Network(error.to_string()))?;
        Ok(Self {
            http,
            metadata_url: "https://api.rainviewer.com/public/weather-maps.json".into(),
            osm_base: "https://tile.openstreetmap.org".into(),
        })
    }

    pub async fn map(
        &self,
        latitude: f64,
        longitude: f64,
        zoom: u8,
        radius: i32,
    ) -> Result<RadarMap, RadarError> {
        self.animation(latitude, longitude, zoom, radius, 1)
            .await?
            .pop()
            .ok_or(RadarError::NoFrames)
    }

    pub async fn animation(
        &self,
        latitude: f64,
        longitude: f64,
        zoom: u8,
        radius: i32,
        frame_count: usize,
    ) -> Result<Vec<RadarMap>, RadarError> {
        let cache_key = (
            latitude.to_bits(),
            longitude.to_bits(),
            zoom,
            radius,
            frame_count,
        );
        // Keep the guard during the request so simultaneous startup renders share
        // one completed result instead of multiplying tile traffic.
        let mut radar_cache = RADAR_CACHE.lock().await;
        if let Some((stored_at, maps)) = radar_cache.get(&cache_key)
            && stored_at.elapsed() < Duration::from_secs(300)
        {
            return Ok(maps.clone());
        }
        let metadata: Metadata = self
            .http
            .get(&self.metadata_url)
            .send()
            .await
            .map_err(network)?
            .error_for_status()
            .map_err(network)?
            .json()
            .await
            .map_err(network)?;
        if metadata.radar.past.is_empty() {
            return Err(RadarError::NoFrames);
        }
        let selected = sampled_frame_indices(metadata.radar.past.len(), frame_count.max(1))
            .into_iter()
            .map(|index| &metadata.radar.past[index])
            .map(|frame| (frame.time, frame.path.clone()))
            .collect::<Vec<_>>();
        let zoom = zoom.min(7);
        let (tile_x, tile_y) = latlon_to_tile_position(latitude, longitude, zoom);
        let center_x = tile_x.floor() as i32;
        let center_y = tile_y.floor() as i32;
        let focus_x = (radius as f64 + tile_x.fract()) * 512.0;
        let focus_y = (radius as f64 + tile_y.fract()) * 512.0;
        let tile_count = 1_i32 << zoom;
        let mut coordinates = Vec::new();
        for row in -radius..=radius {
            for column in -radius..=radius {
                let x = (center_x + column).rem_euclid(tile_count);
                let y = (center_y + row).clamp(0, tile_count - 1);
                coordinates.push((column, row, x, y));
            }
        }
        let base_urls = coordinates
            .iter()
            .map(|(_, _, x, y)| format!("{}/{zoom}/{x}/{y}.png", self.osm_base));
        let base_tiles =
            try_join_all(base_urls.map(|url| download_png(self.http.clone(), url))).await?;
        let host = metadata.host;
        let http = self.http.clone();
        let maps = try_join_all(selected.into_iter().map(|(time, path)| {
            let http = http.clone();
            let host = host.clone();
            let coordinates = coordinates.clone();
            let base_tiles = base_tiles.clone();
            async move {
                let observed_at = DateTime::from_timestamp(time, 0).ok_or(RadarError::NoFrames)?;
                let radar_urls = coordinates
                    .iter()
                    .map(|(_, _, x, y)| format!("{host}{path}/512/{zoom}/{x}/{y}/2/1_1.png"));
                let radar_tiles =
                    try_join_all(radar_urls.map(|url| download_png(http.clone(), url))).await?;
                let tiles = coordinates
                    .into_iter()
                    .zip(base_tiles)
                    .zip(radar_tiles)
                    .map(|(((column, row, _, _), base_png), radar_png)| RadarTile {
                        column,
                        row,
                        base_png,
                        radar_png,
                    })
                    .collect();
                Ok(RadarMap {
                    tiles,
                    observed_at,
                    focus_x,
                    focus_y,
                })
            }
        }))
        .await?;
        radar_cache.insert(cache_key, (std::time::Instant::now(), maps.clone()));
        Ok(maps)
    }
}

fn sampled_frame_indices(available: usize, requested: usize) -> Vec<usize> {
    let count = requested.max(1).min(available);
    if count == available {
        return (0..available).collect();
    }
    if count == 1 {
        return vec![available - 1];
    }
    (0..count)
        .map(|index| index * (available - 1) / (count - 1))
        .collect()
}

async fn download_png(http: reqwest::Client, url: String) -> Result<Vec<u8>, RadarError> {
    Ok(http
        .get(url)
        .send()
        .await
        .map_err(network)?
        .error_for_status()
        .map_err(network)?
        .bytes()
        .await
        .map_err(network)?
        .to_vec())
}

fn network(error: reqwest::Error) -> RadarError {
    RadarError::Network(error.to_string())
}

fn latlon_to_tile(latitude: f64, longitude: f64, zoom: u8) -> (i32, i32) {
    let (x, y) = latlon_to_tile_position(latitude, longitude, zoom);
    (x.floor() as i32, y.floor() as i32)
}

fn latlon_to_tile_position(latitude: f64, longitude: f64, zoom: u8) -> (f64, f64) {
    let scale = (1_u32 << zoom) as f64;
    let latitude = latitude.clamp(-85.051_128_78, 85.051_128_78);
    let x = (longitude + 180.0) / 360.0 * scale;
    let radians = latitude.to_radians();
    let y = (1.0 - (radians.tan() + 1.0 / radians.cos()).ln() / std::f64::consts::PI) / 2.0 * scale;
    (x, y)
}

pub fn nudge(latitude: f64, longitude: f64, zoom: u8, dx: i32, dy: i32) -> (f64, f64) {
    let (x, y) = latlon_to_tile(latitude, longitude, zoom);
    let scale = (1_u32 << zoom) as f64;
    let shifted_x = (x + dx) as f64 + 0.5;
    let shifted_y = (y + dy) as f64 + 0.5;
    let longitude = shifted_x / scale * 360.0 - 180.0;
    let n = std::f64::consts::PI * (1.0 - 2.0 * shifted_y / scale);
    let latitude = n.sinh().atan().to_degrees();
    (latitude, longitude)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_math_is_stable_and_bounded() {
        assert_eq!(latlon_to_tile(0.0, 0.0, 2), (2, 2));
        let (lat, lon) = nudge(41.8781, -87.6298, 7, 1, 0);
        assert!(lat.is_finite() && lon > -87.6298);
    }

    #[test]
    fn animation_samples_the_whole_available_history() {
        assert_eq!(sampled_frame_indices(13, 5), vec![0, 3, 6, 9, 12]);
        assert_eq!(sampled_frame_indices(13, 1), vec![12]);
    }
}
