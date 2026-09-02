use crate::{
    APP_ID,
    geocode::{country_for, timezone_for},
    models::SavedLocation,
};
use thiserror::Error;
use zbus::zvariant::OwnedObjectPath;

#[derive(Debug, Error)]
pub enum LocationError {
    #[error("current location is unavailable or permission was denied: {0}")]
    Unavailable(String),
}

/// Requests a single fix from GeoClue. Callers must obtain explicit user consent first.
pub async fn current_location() -> Result<SavedLocation, LocationError> {
    let connection = zbus::Connection::system().await.map_err(err)?;
    let manager = zbus::Proxy::new(
        &connection,
        "org.freedesktop.GeoClue2",
        "/org/freedesktop/GeoClue2/Manager",
        "org.freedesktop.GeoClue2.Manager",
    )
    .await
    .map_err(err)?;
    let client_path: OwnedObjectPath = manager.call("GetClient", &()).await.map_err(err)?;
    let client = zbus::Proxy::new(
        &connection,
        "org.freedesktop.GeoClue2",
        client_path.as_str(),
        "org.freedesktop.GeoClue2.Client",
    )
    .await
    .map_err(err)?;
    client
        .set_property("DesktopId", APP_ID)
        .await
        .map_err(err)?;
    client
        .set_property("RequestedAccuracyLevel", 4u32)
        .await
        .map_err(err)?;
    let _: () = client.call("Start", &()).await.map_err(err)?;
    let location_path: OwnedObjectPath = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        client.get_property("Location"),
    )
    .await
    .map_err(|_| LocationError::Unavailable("timed out".into()))?
    .map_err(err)?;
    let location = zbus::Proxy::new(
        &connection,
        "org.freedesktop.GeoClue2",
        location_path.as_str(),
        "org.freedesktop.GeoClue2.Location",
    )
    .await
    .map_err(err)?;
    let lat: f64 = location.get_property("Latitude").await.map_err(err)?;
    let lon: f64 = location.get_property("Longitude").await.map_err(err)?;
    let mut saved = SavedLocation::new(
        "Current Location",
        country_for(lat, lon),
        timezone_for(lat, lon),
        lat,
        lon,
    );
    saved.sort_order = -1;
    Ok(saved)
}
fn err(e: impl std::fmt::Display) -> LocationError {
    LocationError::Unavailable(e.to_string())
}
