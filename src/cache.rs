use crate::models::WeatherResponse;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub fetched_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub data: WeatherResponse,
}
#[derive(Debug, Clone)]
pub struct ForecastCache {
    root: PathBuf,
}
impl ForecastCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn xdg() -> Result<Self> {
        Ok(Self::new(
            directories::ProjectDirs::from("io", "Weatherglass", "Weatherglass")
                .context("XDG cache directory unavailable")?
                .cache_dir()
                .join("forecast"),
        ))
    }
    pub fn scoped(self, provider: &str) -> Self {
        Self::new(self.root.join(provider))
    }
    fn path(&self, key: &str) -> PathBuf {
        let digest = Sha256::digest(key.as_bytes());
        self.root.join(format!("{:x}.json", digest))
    }
    pub async fn put(
        &self,
        key: &str,
        data: WeatherResponse,
        now: DateTime<Utc>,
    ) -> Result<CacheEntry> {
        tokio::fs::create_dir_all(&self.root).await?;
        let expires_at = data
            .expires_at()
            .unwrap_or(now + chrono::Duration::minutes(5));
        let entry = CacheEntry {
            fetched_at: now,
            expires_at,
            data,
        };
        let path = self.path(key);
        let tmp = path.with_extension("tmp");
        tokio::fs::write(&tmp, serde_json::to_vec(&entry)?).await?;
        tokio::fs::rename(tmp, path).await?;
        Ok(entry)
    }
    pub async fn get(&self, key: &str) -> Result<Option<CacheEntry>> {
        match tokio::fs::read(self.path(key)).await {
            Ok(v) => Ok(Some(serde_json::from_slice(&v)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    pub async fn fresh(&self, key: &str, now: DateTime<Utc>) -> Result<Option<CacheEntry>> {
        Ok(self.get(key).await?.filter(|x| x.expires_at > now))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn expiry_and_offline_stale_read() {
        let t = tempfile::tempdir().unwrap();
        let c = ForecastCache::new(t.path());
        let now = Utc::now();
        let mut w = WeatherResponse {
            current_weather: None,
            forecast_hourly: None,
            forecast_daily: None,
            forecast_next_hour: None,
            weather_alerts: None,
            extra: Default::default(),
        };
        let e = c.put("x", w.clone(), now).await.unwrap();
        assert!(c.fresh("x", now).await.unwrap().is_some());
        assert!(
            c.fresh("x", e.expires_at + chrono::Duration::seconds(1))
                .await
                .unwrap()
                .is_none()
        );
        assert!(c.get("x").await.unwrap().is_some());
        w.extra.insert("new".into(), true.into());
    }
}
