use weatherglass::map_layers::MapLayerClient;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let layers = MapLayerClient::new()
        .map_err(anyhow::Error::msg)?
        .fetch(45.8202, -88.0649, 6, 1)
        .await
        .map_err(anyhow::Error::msg)?;
    println!(
        "grid={}x{} temperature={} wind={} air_quality={} future_precipitation_frames={}",
        layers.width,
        layers.height,
        layers.temperature_c.iter().flatten().count(),
        layers.wind_kmh.iter().flatten().count(),
        layers.air_quality.iter().flatten().count(),
        layers.precipitation_forecast.len()
    );
    Ok(())
}
