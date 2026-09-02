use weatherglass::{openmeteo::OpenMeteoClient, weatherkit::WeatherProvider};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let weather = OpenMeteoClient::new()?
        .weather("en-US", 41.8781, -87.6298, "US", "America/Chicago")
        .await?;
    println!(
        "PASS Open-Meteo live: current={}, hourly={}, daily={}, minutely={}, air_quality={}",
        weather.current_weather.is_some(),
        weather
            .forecast_hourly
            .as_ref()
            .map_or(0, |forecast| forecast.hours.len()),
        weather
            .forecast_daily
            .as_ref()
            .map_or(0, |forecast| forecast.days.len()),
        weather
            .forecast_next_hour
            .as_ref()
            .map_or(0, |forecast| forecast.minutes.len()),
        weather.extra.contains_key("airQuality")
    );
    Ok(())
}
