use crate::{
    models::{
        Attribution, CurrentWeather, DailyForecast, DayCondition, ForecastMinute, HourCondition,
        HourlyForecast, Metadata, NextHourForecast, WeatherResponse,
    },
    weatherkit::{WeatherError, WeatherProvider},
};
use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::StatusCode;
use serde_json::{Map, Value};
use std::time::Duration;

const CURRENT: &str = "temperature_2m,relative_humidity_2m,apparent_temperature,is_day,precipitation,rain,showers,snowfall,weather_code,cloud_cover,pressure_msl,dew_point_2m,visibility,wind_speed_10m,wind_direction_10m,wind_gusts_10m";
const HOURLY: &str = "temperature_2m,relative_humidity_2m,dew_point_2m,apparent_temperature,precipitation_probability,precipitation,weather_code,pressure_msl,cloud_cover,visibility,wind_speed_10m,wind_direction_10m,wind_gusts_10m,is_day";
const DAILY: &str = "weather_code,temperature_2m_max,temperature_2m_min,precipitation_sum,precipitation_probability_max,snowfall_sum,uv_index_max,sunrise,sunset,moonrise,moonset,moon_phase";

#[derive(Clone)]
pub struct OpenMeteoClient {
    http: reqwest::Client,
    forecast_base: String,
    air_base: String,
}

impl OpenMeteoClient {
    pub fn new() -> Result<Self, WeatherError> {
        Self::with_bases(
            "https://api.open-meteo.com",
            "https://air-quality-api.open-meteo.com",
        )
    }

    pub fn with_bases(
        forecast_base: impl Into<String>,
        air_base: impl Into<String>,
    ) -> Result<Self, WeatherError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(format!("Weatherglass/{}", crate::VERSION))
            .build()
            .map_err(|e| WeatherError::Network(e.to_string()))?;
        Ok(Self {
            http,
            forecast_base: forecast_base.into(),
            air_base: air_base.into(),
        })
    }

    async fn json(&self, url: String, query: &[(&str, String)]) -> Result<Value, WeatherError> {
        let response = self.http.get(url).query(query).send().await.map_err(|e| {
            if e.is_timeout() {
                WeatherError::Timeout
            } else {
                WeatherError::Network(e.to_string())
            }
        })?;
        match response.status() {
            StatusCode::TOO_MANY_REQUESTS => return Err(WeatherError::RateLimited),
            status if status.is_server_error() => {
                return Err(WeatherError::Server(status.as_u16()));
            }
            status if !status.is_success() => return Err(WeatherError::Unavailable),
            _ => {}
        }
        response
            .json()
            .await
            .map_err(|e| WeatherError::Malformed(e.to_string()))
    }

    async fn air_quality(&self, lat: f64, lon: f64) -> Result<Value, WeatherError> {
        self.json(
            format!("{}/v1/air-quality", self.air_base),
            &[
                ("latitude", lat.to_string()),
                ("longitude", lon.to_string()),
                (
                    "current",
                    "us_aqi,european_aqi,pm10,pm2_5,carbon_monoxide,nitrogen_dioxide,ozone".into(),
                ),
                ("timezone", "UTC".into()),
            ],
        )
        .await
    }
}

#[async_trait]
impl WeatherProvider for OpenMeteoClient {
    async fn availability(
        &self,
        _lat: f64,
        _lon: f64,
        _country: &str,
    ) -> Result<Vec<String>, WeatherError> {
        Ok(vec![
            "currentWeather".into(),
            "forecastHourly".into(),
            "forecastDaily".into(),
            "forecastNextHour".into(),
            "airQuality".into(),
        ])
    }

    async fn weather(
        &self,
        _language: &str,
        lat: f64,
        lon: f64,
        _country: &str,
        _timezone: &str,
    ) -> Result<WeatherResponse, WeatherError> {
        let forecast_query = [
            ("latitude", lat.to_string()),
            ("longitude", lon.to_string()),
            ("current", CURRENT.into()),
            ("hourly", HOURLY.into()),
            ("minutely_15", "precipitation".into()),
            ("daily", DAILY.into()),
            ("forecast_days", "10".into()),
            ("timezone", "UTC".into()),
            ("timeformat", "unixtime".into()),
        ];
        let forecast = self.json(
            format!("{}/v1/forecast", self.forecast_base),
            &forecast_query,
        );
        let air = self.air_quality(lat, lon);
        let (forecast, air) = tokio::join!(forecast, air);
        let mut result = decode(forecast?, lat, lon)?;
        if let Ok(air) = air
            && let Some(current) = air.get("current")
        {
            result.extra.insert("airQuality".into(), current.clone());
        }
        Ok(result)
    }

    async fn attribution(&self, _language: &str) -> Result<Attribution, WeatherError> {
        Ok(Attribution {
            service_name: "Open-Meteo".into(),
            logo_light_2x: None,
            logo_dark_2x: None,
            logo_square_2x: None,
            legal_page_url: Some("https://open-meteo.com/en/license".into()),
            legal_attribution_text: Some(
                "Weather data by Open-Meteo; air quality from CAMS ENSEMBLE".into(),
            ),
            extra: Map::new(),
        })
    }
}

fn decode(root: Value, lat: f64, lon: f64) -> Result<WeatherResponse, WeatherError> {
    let now = Utc::now();
    let metadata = Metadata {
        attribution_url: Some("https://open-meteo.com/en/license".into()),
        expire_time: Some(now + chrono::Duration::minutes(15)),
        latitude: Some(lat),
        longitude: Some(lon),
        read_time: Some(now),
        reported_time: Some(now),
        units: Some("metric".into()),
        extra: Map::new(),
    };
    let current = root.get("current").ok_or_else(|| {
        WeatherError::Malformed("Open-Meteo response has no current block".into())
    })?;
    let as_of = timestamp(current.get("time").and_then(Value::as_i64))?;
    let current_weather = CurrentWeather {
        as_of,
        cloud_cover: number(current, "cloud_cover").map(|v| v / 100.0),
        condition_code: wmo(number(current, "weather_code").unwrap_or(-1.0) as i64).into(),
        daylight: number(current, "is_day").unwrap_or(1.0) > 0.0,
        humidity: number(current, "relative_humidity_2m").map(|v| v / 100.0),
        precipitation_intensity: number(current, "precipitation"),
        pressure: number(current, "pressure_msl"),
        pressure_trend: None,
        temperature: number(current, "temperature_2m").ok_or_else(|| {
            WeatherError::Malformed("Open-Meteo response has no current temperature".into())
        })?,
        temperature_apparent: number(current, "apparent_temperature"),
        temperature_dew_point: number(current, "dew_point_2m"),
        uv_index: None,
        visibility: number(current, "visibility").map(|v| v / 1000.0),
        wind_direction: number(current, "wind_direction_10m").map(|v| v.round() as i32),
        wind_gust: number(current, "wind_gusts_10m"),
        wind_speed: number(current, "wind_speed_10m"),
        metadata: metadata.clone(),
        extra: Map::new(),
    };

    let hourly_value = root.get("hourly").unwrap_or(&Value::Null);
    let hourly_times = integers(hourly_value, "time");
    let mut hours = Vec::with_capacity(hourly_times.len());
    for (index, seconds) in hourly_times.into_iter().enumerate() {
        let Some(temperature) = at(hourly_value, "temperature_2m", index) else {
            continue;
        };
        hours.push(HourCondition {
            forecast_start: timestamp(Some(seconds))?,
            cloud_cover: at(hourly_value, "cloud_cover", index).map(|v| v / 100.0),
            condition_code: wmo(at(hourly_value, "weather_code", index).unwrap_or(-1.0) as i64)
                .into(),
            daylight: at(hourly_value, "is_day", index).map(|v| v > 0.0),
            humidity: at(hourly_value, "relative_humidity_2m", index).map(|v| v / 100.0),
            precipitation_chance: at(hourly_value, "precipitation_probability", index)
                .map(|v| v / 100.0),
            precipitation_intensity: at(hourly_value, "precipitation", index),
            precipitation_type: None,
            pressure: at(hourly_value, "pressure_msl", index),
            pressure_trend: None,
            temperature,
            temperature_apparent: at(hourly_value, "apparent_temperature", index),
            temperature_dew_point: at(hourly_value, "dew_point_2m", index),
            uv_index: None,
            visibility: at(hourly_value, "visibility", index).map(|v| v / 1000.0),
            wind_direction: at(hourly_value, "wind_direction_10m", index).map(|v| v.round() as i32),
            wind_gust: at(hourly_value, "wind_gusts_10m", index),
            wind_speed: at(hourly_value, "wind_speed_10m", index),
            extra: Map::new(),
        });
    }

    let daily_value = root.get("daily").unwrap_or(&Value::Null);
    let daily_times = integers(daily_value, "time");
    let mut days = Vec::with_capacity(daily_times.len());
    for (index, seconds) in daily_times.into_iter().enumerate() {
        let (Some(min), Some(max)) = (
            at(daily_value, "temperature_2m_min", index),
            at(daily_value, "temperature_2m_max", index),
        ) else {
            continue;
        };
        let moon_phase_value = at(daily_value, "moon_phase", index);
        let mut day_extra = Map::new();
        if let Some(value) = moon_phase_value {
            day_extra.insert("moonPhaseFraction".into(), Value::from(value));
        }
        days.push(DayCondition {
            forecast_start: timestamp(Some(seconds))?,
            condition_code: wmo(at(daily_value, "weather_code", index).unwrap_or(-1.0) as i64)
                .into(),
            max_uv_index: at(daily_value, "uv_index_max", index).map(|v| v.round() as i32),
            moon_phase: moon_phase_value.map(moon_phase),
            moonrise: time_at(daily_value, "moonrise", index),
            moonset: time_at(daily_value, "moonset", index),
            precipitation_amount: at(daily_value, "precipitation_sum", index),
            precipitation_chance: at(daily_value, "precipitation_probability_max", index)
                .map(|v| v / 100.0),
            precipitation_type: None,
            snowfall_amount: at(daily_value, "snowfall_sum", index),
            sunrise: time_at(daily_value, "sunrise", index),
            sunset: time_at(daily_value, "sunset", index),
            temperature_max: max,
            temperature_min: min,
            daytime_forecast: None,
            overnight_forecast: None,
            extra: day_extra,
        });
    }

    let minutely = root.get("minutely_15").unwrap_or(&Value::Null);
    let minute_times = integers(minutely, "time");
    let minutes = minute_times
        .into_iter()
        .take(4)
        .enumerate()
        .filter_map(|(index, seconds)| {
            Some(ForecastMinute {
                start_time: timestamp(Some(seconds)).ok()?,
                precipitation_chance: None,
                precipitation_intensity: at(minutely, "precipitation", index),
                precipitation_type: None,
                extra: Map::new(),
            })
        })
        .collect::<Vec<_>>();
    let next_hour = if minutes.is_empty() {
        None
    } else {
        Some(NextHourForecast {
            name: "Next hour".into(),
            metadata: metadata.clone(),
            forecast_start: minutes.first().unwrap().start_time,
            forecast_end: minutes.last().unwrap().start_time + chrono::Duration::minutes(15),
            minutes,
            summary: vec![],
            extra: Map::new(),
        })
    };

    Ok(WeatherResponse {
        current_weather: Some(current_weather),
        forecast_hourly: Some(HourlyForecast {
            name: "Hourly forecast".into(),
            metadata: metadata.clone(),
            hours,
            extra: Map::new(),
        }),
        forecast_daily: Some(DailyForecast {
            name: "Daily forecast".into(),
            metadata,
            days,
            extra: Map::new(),
        }),
        forecast_next_hour: next_hour,
        weather_alerts: None,
        extra: Map::new(),
    })
}

fn number(value: &Value, name: &str) -> Option<f64> {
    value.get(name)?.as_f64()
}
fn integers(value: &Value, name: &str) -> Vec<i64> {
    value
        .get(name)
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default()
}
fn at(value: &Value, name: &str, index: usize) -> Option<f64> {
    value.get(name)?.as_array()?.get(index)?.as_f64()
}
fn time_at(value: &Value, name: &str, index: usize) -> Option<DateTime<Utc>> {
    timestamp(value.get(name)?.as_array()?.get(index)?.as_i64()).ok()
}
fn timestamp(value: Option<i64>) -> Result<DateTime<Utc>, WeatherError> {
    Utc.timestamp_opt(
        value.ok_or_else(|| WeatherError::Malformed("missing timestamp".into()))?,
        0,
    )
    .single()
    .ok_or_else(|| WeatherError::Malformed("invalid timestamp".into()))
}
fn moon_phase(value: f64) -> String {
    match value {
        x if x < 0.03 || x > 0.97 => "newMoon",
        x if x < 0.22 => "waxingCrescent",
        x if x < 0.28 => "firstQuarter",
        x if x < 0.47 => "waxingGibbous",
        x if x < 0.53 => "fullMoon",
        x if x < 0.72 => "waningGibbous",
        x if x < 0.78 => "lastQuarter",
        _ => "waningCrescent",
    }
    .into()
}
fn wmo(code: i64) -> &'static str {
    match code {
        0 => "Clear",
        1 => "MostlyClear",
        2 => "PartlyCloudy",
        3 => "Cloudy",
        45 | 48 => "Foggy",
        51 | 53 | 55 => "Drizzle",
        56 | 57 => "FreezingDrizzle",
        61 | 63 | 80 | 81 => "Rain",
        65 | 82 => "HeavyRain",
        66 | 67 => "FreezingRain",
        71 | 73 | 77 | 85 => "Snow",
        75 | 86 => "HeavySnow",
        95 => "Thunderstorms",
        96 | 99 => "StrongStorms",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_missing_optional_fields_and_air_quality_extension() {
        let fixture = serde_json::json!({
            "current":{"time":1700000000,"temperature_2m":10.0,"weather_code":61,"is_day":1},
            "hourly":{"time":[1700000000],"temperature_2m":[10.0],"weather_code":[61]},
            "daily":{"time":[1700000000],"temperature_2m_min":[5.0],"temperature_2m_max":[12.0],"weather_code":[61]},
            "minutely_15":{"time":[1700000000],"precipitation":[0.2]},
            "futureField":{"safe":true}
        });
        let weather = decode(fixture, 1.0, 2.0).unwrap();
        assert_eq!(weather.current_weather.unwrap().condition_code, "Rain");
        assert_eq!(weather.forecast_next_hour.unwrap().minutes.len(), 1);
    }

    #[test]
    fn maps_wmo_and_moon_phase_codes() {
        assert_eq!(wmo(0), "Clear");
        assert_eq!(wmo(95), "Thunderstorms");
        assert_eq!(moon_phase(0.5), "fullMoon");
    }
}
