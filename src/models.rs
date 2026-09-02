use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    #[serde(default)]
    pub attribution_url: Option<String>,
    #[serde(default)]
    pub expire_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub latitude: Option<f64>,
    #[serde(default)]
    pub longitude: Option<f64>,
    #[serde(default)]
    pub read_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub reported_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub units: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurrentWeather {
    pub as_of: DateTime<Utc>,
    #[serde(default)]
    pub cloud_cover: Option<f64>,
    pub condition_code: String,
    pub daylight: bool,
    #[serde(default)]
    pub humidity: Option<f64>,
    #[serde(default)]
    pub precipitation_intensity: Option<f64>,
    #[serde(default)]
    pub pressure: Option<f64>,
    #[serde(default)]
    pub pressure_trend: Option<String>,
    pub temperature: f64,
    #[serde(default)]
    pub temperature_apparent: Option<f64>,
    #[serde(default)]
    pub temperature_dew_point: Option<f64>,
    #[serde(default)]
    pub uv_index: Option<i32>,
    #[serde(default)]
    pub visibility: Option<f64>,
    #[serde(default)]
    pub wind_direction: Option<i32>,
    #[serde(default)]
    pub wind_gust: Option<f64>,
    #[serde(default)]
    pub wind_speed: Option<f64>,
    pub metadata: Metadata,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HourCondition {
    pub forecast_start: DateTime<Utc>,
    #[serde(default)]
    pub cloud_cover: Option<f64>,
    pub condition_code: String,
    #[serde(default)]
    pub daylight: Option<bool>,
    #[serde(default)]
    pub humidity: Option<f64>,
    #[serde(default)]
    pub precipitation_chance: Option<f64>,
    #[serde(default)]
    pub precipitation_intensity: Option<f64>,
    #[serde(default)]
    pub precipitation_type: Option<String>,
    #[serde(default)]
    pub pressure: Option<f64>,
    #[serde(default)]
    pub pressure_trend: Option<String>,
    pub temperature: f64,
    #[serde(default)]
    pub temperature_apparent: Option<f64>,
    #[serde(default)]
    pub temperature_dew_point: Option<f64>,
    #[serde(default)]
    pub uv_index: Option<i32>,
    #[serde(default)]
    pub visibility: Option<f64>,
    #[serde(default)]
    pub wind_direction: Option<i32>,
    #[serde(default)]
    pub wind_gust: Option<f64>,
    #[serde(default)]
    pub wind_speed: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HourlyForecast {
    pub name: String,
    pub metadata: Metadata,
    pub hours: Vec<HourCondition>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayCondition {
    pub forecast_start: DateTime<Utc>,
    pub condition_code: String,
    #[serde(default)]
    pub max_uv_index: Option<i32>,
    #[serde(default)]
    pub moon_phase: Option<String>,
    #[serde(default)]
    pub moonrise: Option<DateTime<Utc>>,
    #[serde(default)]
    pub moonset: Option<DateTime<Utc>>,
    #[serde(default)]
    pub precipitation_amount: Option<f64>,
    #[serde(default)]
    pub precipitation_chance: Option<f64>,
    #[serde(default)]
    pub precipitation_type: Option<String>,
    #[serde(default)]
    pub snowfall_amount: Option<f64>,
    #[serde(default)]
    pub sunrise: Option<DateTime<Utc>>,
    #[serde(default)]
    pub sunset: Option<DateTime<Utc>>,
    pub temperature_max: f64,
    pub temperature_min: f64,
    #[serde(default)]
    pub daytime_forecast: Option<DayPart>,
    #[serde(default)]
    pub overnight_forecast: Option<DayPart>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayPart {
    #[serde(default)]
    pub cloud_cover: Option<f64>,
    #[serde(default)]
    pub condition_code: Option<String>,
    #[serde(default)]
    pub humidity: Option<f64>,
    #[serde(default)]
    pub precipitation_amount: Option<f64>,
    #[serde(default)]
    pub precipitation_chance: Option<f64>,
    #[serde(default)]
    pub wind_direction: Option<i32>,
    #[serde(default)]
    pub wind_gust_speed: Option<f64>,
    #[serde(default)]
    pub wind_speed: Option<f64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DailyForecast {
    pub name: String,
    pub metadata: Metadata,
    pub days: Vec<DayCondition>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ForecastMinute {
    pub start_time: DateTime<Utc>,
    #[serde(default)]
    pub precipitation_chance: Option<f64>,
    #[serde(default)]
    pub precipitation_intensity: Option<f64>,
    #[serde(default)]
    pub precipitation_type: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NextHourForecast {
    pub name: String,
    pub metadata: Metadata,
    pub forecast_start: DateTime<Utc>,
    pub forecast_end: DateTime<Utc>,
    pub minutes: Vec<ForecastMinute>,
    #[serde(default)]
    pub summary: Vec<Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AlertSummary {
    pub id: String,
    #[serde(default)]
    pub area_id: Option<String>,
    #[serde(default)]
    pub area_name: Option<String>,
    #[serde(default)]
    pub certainty: Option<String>,
    #[serde(default)]
    pub country_code: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub details_url: Option<String>,
    #[serde(default)]
    pub effective_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub expire_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub issued_time: Option<DateTime<Utc>>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub event_source: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub urgency: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeatherAlerts {
    pub name: String,
    pub metadata: Metadata,
    #[serde(default)]
    pub details: Vec<AlertSummary>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeatherResponse {
    #[serde(default)]
    pub current_weather: Option<CurrentWeather>,
    #[serde(default)]
    pub forecast_hourly: Option<HourlyForecast>,
    #[serde(default)]
    pub forecast_daily: Option<DailyForecast>,
    #[serde(default)]
    pub forecast_next_hour: Option<NextHourForecast>,
    #[serde(default)]
    pub weather_alerts: Option<WeatherAlerts>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl WeatherResponse {
    pub fn expires_at(&self) -> Option<DateTime<Utc>> {
        [
            self.current_weather
                .as_ref()
                .and_then(|x| x.metadata.expire_time),
            self.forecast_hourly
                .as_ref()
                .and_then(|x| x.metadata.expire_time),
            self.forecast_daily
                .as_ref()
                .and_then(|x| x.metadata.expire_time),
            self.forecast_next_hour
                .as_ref()
                .and_then(|x| x.metadata.expire_time),
            self.weather_alerts
                .as_ref()
                .and_then(|x| x.metadata.expire_time),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attribution {
    pub service_name: String,
    #[serde(default, rename = "logoLight@2x")]
    pub logo_light_2x: Option<String>,
    #[serde(default, rename = "logoDark@2x")]
    pub logo_dark_2x: Option<String>,
    #[serde(default, rename = "logoSquare@2x")]
    pub logo_square_2x: Option<String>,
    #[serde(default)]
    pub legal_page_url: Option<String>,
    #[serde(default)]
    pub legal_attribution_text: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedLocation {
    pub id: String,
    pub display_name: String,
    pub country_code: String,
    pub timezone: String,
    pub latitude: f64,
    pub longitude: f64,
    pub sort_order: i64,
    pub last_selected: bool,
}

impl SavedLocation {
    pub fn new(
        name: impl Into<String>,
        country: impl Into<String>,
        timezone: impl Into<String>,
        latitude: f64,
        longitude: f64,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            display_name: name.into(),
            country_code: country.into(),
            timezone: timezone.into(),
            latitude,
            longitude,
            sort_order: 0,
            last_selected: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_optionals_and_unknown_fields_decode() {
        let json = r#"{"currentWeather":{"name":"CurrentWeather","asOf":"2026-08-20T12:00:00Z","conditionCode":"Clear","daylight":true,"temperature":23.5,"metadata":{"expireTime":"2026-08-20T12:05:00Z","futureMetadata":9},"newAppleField":{"x":1}},"topLevelFuture":true}"#;
        let data: WeatherResponse = serde_json::from_str(json).unwrap();
        let current = data.current_weather.unwrap();
        assert_eq!(current.temperature, 23.5);
        assert!(current.humidity.is_none());
        assert!(current.extra.contains_key("newAppleField"));
        assert!(current.metadata.extra.contains_key("futureMetadata"));
    }
    #[test]
    fn alert_text_source_and_link_are_unmodified() {
        let json = r#"{"weatherAlerts":{"name":"WeatherAlerts","metadata":{},"details":[{"id":"A1","description":"Exact agency headline — do not rewrite","source":"National Weather Service","severity":"severe","detailsUrl":"https://weatherkit.apple.com/alert/A1","future":"ok"}]}}"#;
        let data: WeatherResponse = serde_json::from_str(json).unwrap();
        let alert = &data.weather_alerts.unwrap().details[0];
        assert_eq!(
            alert.description.as_deref(),
            Some("Exact agency headline — do not rewrite")
        );
        assert_eq!(alert.source.as_deref(), Some("National Weather Service"));
        assert_eq!(
            alert.details_url.as_deref(),
            Some("https://weatherkit.apple.com/alert/A1")
        );
        assert!(alert.extra.contains_key("future"));
    }
}
