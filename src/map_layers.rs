use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde_json::Value;
use std::time::Duration;

const GRID_SIZE: usize = 5;
const FUTURE_HOURS: usize = 8;

#[derive(Clone, Debug)]
pub struct MapLayerData {
    pub width: usize,
    pub height: usize,
    pub temperature_c: Vec<Option<f64>>,
    pub wind_kmh: Vec<Option<f64>>,
    pub wind_direction: Vec<Option<f64>>,
    pub air_quality: Vec<Option<f64>>,
    pub precipitation_forecast: Vec<PrecipitationFrame>,
}

#[derive(Clone, Debug)]
pub struct PrecipitationFrame {
    pub at: DateTime<Utc>,
    pub millimetres: Vec<Option<f64>>,
    pub probability: Vec<Option<f64>>,
}

#[derive(Clone)]
pub struct MapLayerClient {
    http: reqwest::Client,
}

impl MapLayerClient {
    pub fn new() -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(format!("Weatherglass/{} map layers", crate::VERSION))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { http })
    }

    pub async fn fetch(
        &self,
        latitude: f64,
        longitude: f64,
        zoom: u8,
        radius: i32,
    ) -> Result<MapLayerData, String> {
        let coordinates = sample_coordinates(latitude, longitude, zoom, radius);
        let latitudes = coordinates
            .iter()
            .map(|(lat, _)| format!("{lat:.5}"))
            .collect::<Vec<_>>()
            .join(",");
        let longitudes = coordinates
            .iter()
            .map(|(_, lon)| format!("{lon:.5}"))
            .collect::<Vec<_>>()
            .join(",");
        let forecast_query = [
            ("latitude", latitudes.clone()),
            ("longitude", longitudes.clone()),
            (
                "current",
                "temperature_2m,wind_speed_10m,wind_direction_10m".into(),
            ),
            ("hourly", "precipitation,precipitation_probability".into()),
            ("forecast_hours", FUTURE_HOURS.to_string()),
            ("timeformat", "unixtime".into()),
            ("timezone", "UTC".into()),
        ];
        let air_query = [
            ("latitude", latitudes),
            ("longitude", longitudes),
            ("current", "us_aqi,european_aqi".into()),
            ("timezone", "UTC".into()),
        ];
        let forecast = self.get("https://api.open-meteo.com/v1/forecast", &forecast_query);
        let air_quality = self.get(
            "https://air-quality-api.open-meteo.com/v1/air-quality",
            &air_query,
        );
        let (forecast, air_quality) = tokio::join!(forecast, air_quality);
        decode(forecast?, air_quality.ok())
    }

    async fn get(&self, url: &str, query: &[(&str, String)]) -> Result<Value, String> {
        let response = self
            .http
            .get(url)
            .query(query)
            .send()
            .await
            .map_err(|error| error.to_string())?;
        match response.status() {
            StatusCode::TOO_MANY_REQUESTS => Err("map layers are temporarily rate limited".into()),
            status if !status.is_success() => Err(format!(
                "map layer service returned HTTP {}",
                status.as_u16()
            )),
            _ => response.json().await.map_err(|error| error.to_string()),
        }
    }
}

fn decode(forecast: Value, air_quality: Option<Value>) -> Result<MapLayerData, String> {
    let forecast = forecast
        .as_array()
        .ok_or_else(|| "map forecast response was not a coordinate list".to_string())?;
    if forecast.len() != GRID_SIZE * GRID_SIZE {
        return Err("map forecast response omitted grid points".into());
    }
    let air = air_quality.as_ref().and_then(Value::as_array);
    let mut temperature_c = Vec::with_capacity(forecast.len());
    let mut wind_kmh = Vec::with_capacity(forecast.len());
    let mut wind_direction = Vec::with_capacity(forecast.len());
    let mut air_quality_values = Vec::with_capacity(forecast.len());
    for (index, point) in forecast.iter().enumerate() {
        let current = point.get("current").unwrap_or(&Value::Null);
        temperature_c.push(number(current, "temperature_2m"));
        wind_kmh.push(number(current, "wind_speed_10m"));
        wind_direction.push(number(current, "wind_direction_10m"));
        let air_current = air
            .and_then(|points| points.get(index))
            .and_then(|point| point.get("current"));
        air_quality_values.push(air_current.and_then(|current| {
            number(current, "us_aqi").or_else(|| number(current, "european_aqi"))
        }));
    }
    let times = forecast[0]
        .pointer("/hourly/time")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let now = Utc::now().timestamp();
    let precipitation_forecast = times
        .iter()
        .enumerate()
        .filter_map(|(time_index, time)| {
            let timestamp = time.as_i64()?;
            if timestamp <= now {
                return None;
            }
            let at = DateTime::from_timestamp(timestamp, 0)?;
            let millimetres = forecast
                .iter()
                .map(|point| array_number(point, "/hourly/precipitation", time_index))
                .collect();
            let probability = forecast
                .iter()
                .map(|point| array_number(point, "/hourly/precipitation_probability", time_index))
                .collect();
            Some(PrecipitationFrame {
                at,
                millimetres,
                probability,
            })
        })
        .collect();
    Ok(MapLayerData {
        width: GRID_SIZE,
        height: GRID_SIZE,
        temperature_c,
        wind_kmh,
        wind_direction,
        air_quality: air_quality_values,
        precipitation_forecast,
    })
}

fn number(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn array_number(value: &Value, pointer: &str, index: usize) -> Option<f64> {
    value.pointer(pointer)?.get(index)?.as_f64()
}

fn sample_coordinates(latitude: f64, longitude: f64, zoom: u8, radius: i32) -> Vec<(f64, f64)> {
    let scale = (1_u32 << zoom.min(7)) as f64;
    let latitude = latitude.clamp(-85.051_128_78, 85.051_128_78);
    let tile_x = (longitude + 180.0) / 360.0 * scale;
    let radians = latitude.to_radians();
    let tile_y =
        (1.0 - (radians.tan() + 1.0 / radians.cos()).ln() / std::f64::consts::PI) / 2.0 * scale;
    let start_x = tile_x.floor() - radius as f64;
    let start_y = tile_y.floor() - radius as f64;
    let span = (radius * 2 + 1) as f64;
    let mut result = Vec::with_capacity(GRID_SIZE * GRID_SIZE);
    for row in 0..GRID_SIZE {
        for column in 0..GRID_SIZE {
            let fraction_x = column as f64 / (GRID_SIZE - 1) as f64;
            let fraction_y = row as f64 / (GRID_SIZE - 1) as f64;
            let x = start_x + span * fraction_x;
            let y = (start_y + span * fraction_y).clamp(0.0, scale);
            let lon = x.rem_euclid(scale) / scale * 360.0 - 180.0;
            let mercator = std::f64::consts::PI * (1.0 - 2.0 * y / scale);
            let lat = mercator.sinh().atan().to_degrees();
            result.push((lat, lon));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_grid_has_stable_shape_and_bounds() {
        let points = sample_coordinates(45.8202, -88.0649, 6, 1);
        assert_eq!(points.len(), 25);
        assert!(points.iter().all(|(lat, lon)| {
            (-85.051_128_78..=85.051_128_78).contains(lat) && (-180.0..=180.0).contains(lon)
        }));
    }
}
