use chrono::Utc;
use weatherglass::{
    cache::ForecastCache,
    models::{SavedLocation, WeatherResponse},
    settings::Settings,
    storage::LocationStore,
    units::{self, TemperatureUnit},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let store = LocationStore::new(temp.path().join("state.db"));
    store.migrate().await?;
    let mut cities = vec![
        SavedLocation::new("Chicago", "US", "America/Chicago", 41.8781, -87.6298),
        SavedLocation::new("London", "GB", "Europe/London", 51.5072, -0.1276),
        SavedLocation::new("Tokyo", "JP", "Asia/Tokyo", 35.6762, 139.6503),
    ];
    for (i, l) in cities.iter_mut().enumerate() {
        l.sort_order = i as i64;
        l.last_selected = i == 0;
        store.upsert(l.clone()).await?;
    }
    store.select(cities[1].id.clone()).await?;
    store
        .reorder(vec![
            cities[2].id.clone(),
            cities[1].id.clone(),
            cities[0].id.clone(),
        ])
        .await?;
    store.delete(cities[0].id.clone()).await?;
    let reopened = LocationStore::new(temp.path().join("state.db"));
    reopened.migrate().await?;
    let rows = reopened.list().await?;
    anyhow::ensure!(
        rows.len() == 2 && rows[0].display_name == "Tokyo" && rows.iter().any(|x| x.last_selected),
        "persistence smoke failed"
    );
    let mut settings = Settings::default();
    settings.temperature = TemperatureUnit::Fahrenheit;
    anyhow::ensure!(
        (units::temperature(0.0, settings.temperature) - 32.0).abs() < 0.01,
        "unit switch failed"
    );
    let weather: WeatherResponse =
        serde_json::from_str(include_str!("../../tests/fixtures/demo_weather.json"))?;
    let cache = ForecastCache::new(temp.path().join("cache"));
    let entry = cache.put("tokyo", weather, Utc::now()).await?;
    anyhow::ensure!(
        cache.fresh("tokyo", Utc::now()).await?.is_some(),
        "online cache failed"
    );
    anyhow::ensure!(
        cache.get("tokyo").await?.is_some(),
        "offline cache startup failed"
    );
    println!(
        "PASS first-launch migration; 3 adds; select; reorder; delete; restart persistence; unit switch; refresh fixture; offline cache"
    );
    println!("Cache expiry: {}", entry.expires_at);
    Ok(())
}
