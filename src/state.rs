use crate::{
    cache::{CacheEntry, ForecastCache},
    models::{SavedLocation, WeatherResponse},
    weatherkit::{WeatherError, WeatherProvider},
};
use chrono::Utc;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub enum ForecastResult {
    Fresh(WeatherResponse),
    Cached { entry: CacheEntry, stale: bool },
}
#[derive(Clone)]
pub struct RefreshCoordinator {
    provider: Arc<dyn WeatherProvider>,
    cache: ForecastCache,
    inflight: Arc<Mutex<HashMap<String, CancellationToken>>>,
    request_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}
impl RefreshCoordinator {
    pub fn new(provider: Arc<dyn WeatherProvider>, cache: ForecastCache) -> Self {
        Self {
            provider,
            cache,
            inflight: Default::default(),
            request_locks: Default::default(),
        }
    }
    fn key(l: &SavedLocation) -> String {
        format!("{:.5}:{:.5}:{}", l.latitude, l.longitude, l.country_code)
    }
    pub async fn cancel_all_except(&self, id: &str) {
        let mut map = self.inflight.lock().await;
        for (k, t) in map.iter() {
            if k != id {
                t.cancel();
            }
        }
        map.retain(|k, _| k == id);
    }
    pub async fn refresh(
        &self,
        l: &SavedLocation,
        force: bool,
    ) -> Result<ForecastResult, WeatherError> {
        let key = Self::key(l);
        if !force {
            if let Ok(Some(x)) = self.cache.fresh(&key, Utc::now()).await {
                return Ok(ForecastResult::Cached {
                    entry: x,
                    stale: false,
                });
            }
        }
        let (lock, follower) = {
            let mut locks = self.request_locks.lock().await;
            if let Some(lock) = locks.get(&l.id) {
                (lock.clone(), true)
            } else {
                let lock = Arc::new(Mutex::new(()));
                locks.insert(l.id.clone(), lock.clone());
                (lock, false)
            }
        };
        let _guard = lock.lock().await;
        if follower {
            if let Ok(Some(x)) = self.cache.fresh(&key, Utc::now()).await {
                return Ok(ForecastResult::Cached {
                    entry: x,
                    stale: false,
                });
            }
        }
        let token = {
            let mut m = self.inflight.lock().await;
            if let Some(existing) = m.get(&l.id) {
                existing.clone()
            } else {
                let t = CancellationToken::new();
                m.insert(l.id.clone(), t.clone());
                t
            }
        };
        let future = self.provider.weather(
            "en-US",
            l.latitude,
            l.longitude,
            &l.country_code,
            &l.timezone,
        );
        let result = tokio::select! {_ = token.cancelled()=>Err(WeatherError::Network("request cancelled".into())),r=future=>r};
        self.inflight.lock().await.remove(&l.id);
        self.request_locks.lock().await.remove(&l.id);
        match result {
            Ok(data) => {
                let _ = self.cache.put(&key, data.clone(), Utc::now()).await;
                Ok(ForecastResult::Fresh(data))
            }
            Err(e) => {
                if let Ok(Some(entry)) = self.cache.get(&key).await {
                    return Ok(ForecastResult::Cached {
                        stale: entry.expires_at <= Utc::now(),
                        entry,
                    });
                }
                Err(e)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Attribution;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Slow {
        calls: AtomicUsize,
        delay: u64,
    }
    #[async_trait]
    impl WeatherProvider for Slow {
        async fn availability(&self, _: f64, _: f64, _: &str) -> Result<Vec<String>, WeatherError> {
            Ok(vec![])
        }
        async fn weather(
            &self,
            _: &str,
            _: f64,
            _: f64,
            _: &str,
            _: &str,
        ) -> Result<WeatherResponse, WeatherError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(self.delay)).await;
            Ok(WeatherResponse {
                current_weather: None,
                forecast_hourly: None,
                forecast_daily: None,
                forecast_next_hour: None,
                weather_alerts: None,
                extra: Default::default(),
            })
        }
        async fn attribution(&self, _: &str) -> Result<Attribution, WeatherError> {
            Err(WeatherError::Unavailable)
        }
    }
    #[tokio::test]
    async fn overlapping_refreshes_are_deduplicated() {
        let p = Arc::new(Slow {
            calls: AtomicUsize::new(0),
            delay: 80,
        });
        let t = tempfile::tempdir().unwrap();
        let c = RefreshCoordinator::new(p.clone(), ForecastCache::new(t.path()));
        let l = SavedLocation::new("X", "US", "Etc/UTC", 1., 2.);
        let (a, b) = tokio::join!(c.refresh(&l, true), c.refresh(&l, true));
        assert!(a.is_ok() && b.is_ok());
        assert_eq!(p.calls.load(Ordering::SeqCst), 1);
    }
    #[tokio::test]
    async fn switching_cancels_obsolete_request() {
        let p = Arc::new(Slow {
            calls: AtomicUsize::new(0),
            delay: 500,
        });
        let t = tempfile::tempdir().unwrap();
        let c = RefreshCoordinator::new(p, ForecastCache::new(t.path()));
        let l = SavedLocation::new("A", "US", "Etc/UTC", 1., 2.);
        let id = l.id.clone();
        let c2 = c.clone();
        let task = tokio::spawn(async move { c2.refresh(&l, true).await });
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        c.cancel_all_except("different").await;
        let result = task.await.unwrap();
        assert!(
            matches!(result, Err(WeatherError::Network(_))),
            "{result:?}"
        );
        assert!(!id.is_empty());
    }
}
