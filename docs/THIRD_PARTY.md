# Third-party software and assets

Rust dependencies and exact versions are locked in `Cargo.lock`. Their package metadata contains the authoritative license expressions; run `cargo metadata --locked --format-version 1` to audit the full graph. Principal libraries are gtk-rs/libadwaita-rs (MIT), Tokio (MIT), reqwest (MIT/Apache-2.0), rusqlite (MIT), secret-service-rs (MIT/Apache-2.0), and tzf-rs (MIT).

Offline coordinate-to-country lookup uses reverse_geocoder (MIT/Apache-2.0) and its GeoNames-derived city data. GeoNames data is licensed under CC BY 4.0.

Forecasts and location search from Open-Meteo are provided under CC BY 4.0. Air-quality forecasts use CAMS ENSEMBLE data and are attributed in the interface.

Radar tiles are provided by RainViewer under its public personal/educational-use API terms. Base-map tiles are © OpenStreetMap contributors under ODbL. Both attributions are visible in the radar window.

The Weatherglass launcher icon was generated specifically for this project with OpenAI’s built-in image generation tool from an original prompt. It contains no Apple marks or copied symbols and is distributed with Weatherglass under GPL-3.0-or-later. Apple’s Weather attribution mark is not packaged: it is fetched at runtime from WeatherKit’s official attribution response and remains Apple property.
