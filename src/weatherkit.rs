use crate::{
    auth::TokenProvider,
    models::{Attribution, WeatherResponse},
};
use async_trait::async_trait;
use reqwest::StatusCode;
use std::{sync::Arc, time::Duration};
use thiserror::Error;

pub const DATASETS: &[&str] = &[
    "currentWeather",
    "forecastHourly",
    "forecastDaily",
    "forecastNextHour",
    "weatherAlerts",
];

#[derive(Debug, Error)]
pub enum WeatherError {
    #[error(
        "WeatherKit credentials were rejected (401); check Team ID, Key ID, Service ID, key capability, and system clock"
    )]
    Unauthorized,
    #[error("WeatherKit request limit reached (429); wait before retrying")]
    RateLimited,
    #[error("WeatherKit server error ({0}); retry later")]
    Server(u16),
    #[error("WeatherKit request timed out")]
    Timeout,
    #[error("WeatherKit returned malformed data: {0}")]
    Malformed(String),
    #[error("Weather data is unavailable for this location")]
    Unavailable,
    #[error("network is unavailable: {0}")]
    Network(String),
    #[error(transparent)]
    Auth(#[from] crate::auth::AuthError),
}
#[async_trait]
pub trait WeatherProvider: Send + Sync {
    async fn availability(
        &self,
        lat: f64,
        lon: f64,
        country: &str,
    ) -> Result<Vec<String>, WeatherError>;
    async fn weather(
        &self,
        language: &str,
        lat: f64,
        lon: f64,
        country: &str,
        timezone: &str,
    ) -> Result<WeatherResponse, WeatherError>;
    async fn attribution(&self, language: &str) -> Result<Attribution, WeatherError>;
}

pub struct WeatherKitClient {
    http: reqwest::Client,
    token: Arc<dyn TokenProvider>,
    base: String,
}
impl WeatherKitClient {
    pub fn new(token: Arc<dyn TokenProvider>) -> Result<Self, WeatherError> {
        Self::with_base(token, "https://weatherkit.apple.com")
    }
    pub fn with_base(
        token: Arc<dyn TokenProvider>,
        base: impl Into<String>,
    ) -> Result<Self, WeatherError> {
        Self::with_base_and_timeout(token, base, Duration::from_secs(20))
    }
    pub fn with_base_and_timeout(
        token: Arc<dyn TokenProvider>,
        base: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, WeatherError> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent(format!("Weatherglass/{}", crate::VERSION))
            .build()
            .map_err(|e| WeatherError::Network(e.to_string()))?;
        Ok(Self {
            http,
            token,
            base: base.into(),
        })
    }
    async fn get(
        &self,
        url: String,
        query: &[(&str, String)],
    ) -> Result<reqwest::Response, WeatherError> {
        let jwt = self.token.token().await?;
        let result = self
            .http
            .get(url)
            .bearer_auth(jwt)
            .query(query)
            .send()
            .await
            .map_err(map_reqwest)?;
        match result.status() {
            StatusCode::UNAUTHORIZED => Err(WeatherError::Unauthorized),
            StatusCode::TOO_MANY_REQUESTS => Err(WeatherError::RateLimited),
            s if s.is_server_error() => Err(WeatherError::Server(s.as_u16())),
            s if !s.is_success() => Err(WeatherError::Unavailable),
            _ => Ok(result),
        }
    }
    pub async fn attribution_mark(
        &self,
        attribution: &Attribution,
        dark: bool,
    ) -> Result<Vec<u8>, WeatherError> {
        let partial = if dark {
            attribution.logo_dark_2x.as_ref()
        } else {
            attribution.logo_light_2x.as_ref()
        }
        .ok_or(WeatherError::Unavailable)?;
        let url = if partial.starts_with("http") {
            partial.clone()
        } else {
            format!("{}{}", self.base, partial)
        };
        let response = self.http.get(url).send().await.map_err(map_reqwest)?;
        if !response.status().is_success() {
            return Err(WeatherError::Unavailable);
        }
        Ok(response.bytes().await.map_err(map_reqwest)?.to_vec())
    }
}
fn map_reqwest(e: reqwest::Error) -> WeatherError {
    if e.is_timeout() {
        WeatherError::Timeout
    } else {
        WeatherError::Network(e.to_string())
    }
}

#[async_trait]
impl WeatherProvider for WeatherKitClient {
    async fn availability(
        &self,
        lat: f64,
        lon: f64,
        country: &str,
    ) -> Result<Vec<String>, WeatherError> {
        self.get(
            format!("{}/api/v1/availability/{lat}/{lon}", self.base),
            &[("country", country.to_string())],
        )
        .await?
        .json()
        .await
        .map_err(|e| WeatherError::Malformed(e.to_string()))
    }
    async fn weather(
        &self,
        language: &str,
        lat: f64,
        lon: f64,
        country: &str,
        timezone: &str,
    ) -> Result<WeatherResponse, WeatherError> {
        let available = self.availability(lat, lon, country).await?;
        let requested = DATASETS
            .iter()
            .filter(|x| available.iter().any(|a| a == **x))
            .copied()
            .collect::<Vec<_>>();
        if requested.is_empty() {
            return Err(WeatherError::Unavailable);
        }
        let response = self
            .get(
                format!("{}/api/v1/weather/{language}/{lat}/{lon}", self.base),
                &[
                    ("countryCode", country.to_string()),
                    ("timezone", timezone.to_string()),
                    ("dataSets", requested.join(",")),
                ],
            )
            .await?;
        response
            .json()
            .await
            .map_err(|e| WeatherError::Malformed(e.to_string()))
    }
    async fn attribution(&self, language: &str) -> Result<Attribution, WeatherError> {
        self.get(format!("{}/attribution/{language}", self.base), &[])
            .await?
            .json()
            .await
            .map_err(|e| WeatherError::Malformed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::AuthError;
    struct Token;
    #[async_trait]
    impl TokenProvider for Token {
        async fn token(&self) -> Result<String, AuthError> {
            Ok("test".into())
        }
    }
    async fn client(server: &httpmock::MockServer) -> WeatherKitClient {
        WeatherKitClient::with_base(Arc::new(Token), server.base_url()).unwrap()
    }
    #[tokio::test]
    async fn requests_only_available_datasets() {
        let s = httpmock::MockServer::start_async().await;
        s.mock_async(|w, t| {
            w.method("GET").path("/api/v1/availability/1/2");
            t.status(200)
                .json_body(serde_json::json!(["currentWeather", "forecastHourly"]));
        })
        .await;
        s.mock_async(|w, t| {
            w.method("GET")
                .path("/api/v1/weather/en-US/1/2")
                .query_param("dataSets", "currentWeather,forecastHourly");
            t.status(200).json_body(serde_json::json!({}));
        })
        .await;
        let got = client(&s)
            .await
            .weather("en-US", 1., 2., "US", "Etc/UTC")
            .await
            .unwrap();
        assert!(got.current_weather.is_none());
    }
    #[tokio::test]
    async fn errors_are_actionable() {
        for (status, kind) in [(401, "auth"), (429, "rate"), (500, "server")] {
            let s = httpmock::MockServer::start_async().await;
            s.mock_async(move |w, t| {
                w.path("/api/v1/availability/1/2");
                t.status(status);
            })
            .await;
            let e = client(&s)
                .await
                .availability(1., 2., "US")
                .await
                .unwrap_err();
            match (kind, e) {
                ("auth", WeatherError::Unauthorized)
                | ("rate", WeatherError::RateLimited)
                | ("server", WeatherError::Server(_)) => (),
                _ => panic!("wrong error"),
            }
        }
    }
    #[tokio::test]
    async fn malformed_response() {
        let s = httpmock::MockServer::start_async().await;
        s.mock_async(|_, t| {
            t.status(200).body("not-json");
        })
        .await;
        assert!(matches!(
            client(&s).await.availability(1., 2., "US").await,
            Err(WeatherError::Malformed(_))
        ));
    }
    #[tokio::test]
    async fn timeout_is_classified() {
        let s = httpmock::MockServer::start_async().await;
        s.mock_async(|_, t| {
            t.status(200)
                .delay(std::time::Duration::from_millis(100))
                .json_body(serde_json::json!([]));
        })
        .await;
        let c = WeatherKitClient::with_base_and_timeout(
            Arc::new(Token),
            s.base_url(),
            std::time::Duration::from_millis(10),
        )
        .unwrap();
        assert!(matches!(
            c.availability(1., 2., "US").await,
            Err(WeatherError::Timeout)
        ));
    }
}
