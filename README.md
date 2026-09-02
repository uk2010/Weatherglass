# Weatherglass

Weatherglass is a native GTK 4/libadwaita desktop forecast for Ubuntu 26.04. It follows a calm, dense weather-dashboard information order while using original branding and interface assets. Free, no-key Open-Meteo forecast and CAMS air-quality data are enabled by default. Apple WeatherKit remains an optional forecast/alert source. RainViewer provides the separately attributed radar layer.

## Ubuntu prerequisites

```bash
sudo apt install build-essential cargo pkg-config libgtk-4-dev libadwaita-1-dev libssl-dev libsecret-1-0
```

The checked environment uses Rust 1.98, GTK 4.22, and libadwaita 1.9. Ubuntu 26.04’s equivalent or newer packages are expected.

## Run from source

```bash
./scripts/run-dev.sh
```

Or `cargo run --locked`. Data is stored under `$XDG_DATA_HOME/weatherglass` (or the platform XDG default), settings under the XDG config directory, and temporary forecasts/geocoder results under the XDG cache directory.

Until a preference is saved, measurement units follow the selected location: the US, Liberia, and Myanmar use the imperial set, while other countries use metric. Open Settings to change individual units, forecast provider, or System/Light/Dark theme, then press **Save**. Saved choices override location-based defaults on future launches.

## Free provider and radar

Open-Meteo requires no account or API key. It supplies current, 15-minute, hourly and ten-day forecast fields plus astronomy; its separate Air Quality API supplies CAMS-based AQI and pollutants. Location search uses Open-Meteo’s GeoNames-backed Geocoding API. Data is licensed under CC BY 4.0 and attributed in the app.

Radar uses RainViewer’s public personal-use API over OpenStreetMap base tiles. It uses RainViewer’s high-resolution 512px tiles and one aligned mosaic, avoiding independently stretched tile seams. The layer menu selects Precipitation, Temperature, Air Quality, or Wind; the latter three are generated from real Open-Meteo grid samples, and wind includes direction indicators. Drag with a mouse or use two-finger trackpad scrolling to pan either map without visible scrollbars. Play spans sampled observed radar history and then clearly labelled future Open-Meteo precipitation forecasts; RainViewer discontinued its own future nowcast in 2026.

## WeatherKit credentials

An active Apple Developer Program membership is required. In Certificates, Identifiers & Profiles, enable WeatherKit, create a WeatherKit key, download its one-time `.p8` file, and register a Service ID. Open Settings in Weatherglass and provide Team ID, 10-character Key ID, Service ID, and import the `.p8`. Press **Save and Test WeatherKit Connection**. A success toast is shown only after a real authenticated forecast succeeds; otherwise the message distinguishes missing keyring, 401, 429, timeout, malformed response, and server failure.

The private key is stored in GNOME Keyring, never in the database. Development-only key injection is possible with `WEATHERGLASS_WEATHERKIT_PRIVATE_KEY`; identifiers still come from Settings.

Official documentation: [overview and attribution](https://developer.apple.com/weatherkit/), [REST API](https://developer.apple.com/documentation/weatherkitrestapi), [authentication](https://developer.apple.com/documentation/weatherkitrestapi/request-authentication-for-weatherkit-rest-api), [weather endpoint](https://developer.apple.com/documentation/weatherkitrestapi/get-api-v1-weather-_language_-_latitude_-_longitude_), and [datasets](https://developer.apple.com/documentation/weatherkitrestapi/dataset).

## Location search and privacy

City search is deliberately separate from weather and uses the [Open-Meteo Geocoding API](https://open-meteo.com/en/docs/geocoding-api). Weatherglass sends nothing while typing: press Search explicitly. Direct `latitude, longitude` entry works without geocoding. “Use Current Location” asks first, then requests one fix through GeoClue; denial does not reduce other functionality. Timezones are resolved offline.

## Tests and smoke checks

```bash
cargo test --locked
cargo run --locked --bin smoke
cargo run --locked --bin openmeteo_smoke
cargo run --locked --bin radar_smoke
cargo run --locked --bin map_layers_smoke
```

The smoke binary uses an isolated temporary XDG-like store and exercises first migration, three adds, selection, reorder, deletion, restart persistence, unit change, refresh fixture, and offline cached startup. It never uses credentials or a live service.

## Build and install packages

```bash
./packaging/build-deb.sh
sudo apt install ./dist/weatherglass_0.0.1_amd64.deb
weatherglass
```

To create an RPM, install the unprivileged Rust packaging helper and run:

```bash
cargo install cargo-generate-rpm --locked
./packaging/build-rpm.sh
```

Both scripts detect native AMD64 or ARM64 automatically. The tagged GitHub release workflow builds `.deb` and `.rpm` packages natively on Ubuntu 26.04 runners for both architectures and attaches a source archive. Set `SOURCE_DATE_EPOCH` to reproduce Debian timestamps. Packages contain no credentials. Inspect the Debian artifact with `dpkg-deb --contents dist/weatherglass_0.0.1_amd64.deb`.

## Current limitations

Open-Meteo does not provide official severe-weather alert text or radar tiles. Alerts are therefore available only when WeatherKit is selected and supports them; radar is a separately attributed RainViewer observation layer. Neither provider supplies weather news. See [capability matrix](docs/CAPABILITY_MATRIX.md), [architecture](docs/ARCHITECTURE.md), and [security notes](docs/SECURITY.md).
