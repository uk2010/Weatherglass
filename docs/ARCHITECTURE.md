# Architecture

The GTK process owns widgets only on the main thread. A four-thread Tokio runtime handles HTTP, GeoClue/Secret Service D-Bus calls, authentication, cache I/O, and SQLite work. Results cross back through async channels consumed by GLib-local futures.

- `ui.rs` and `widgets.rs`: adaptive split view, actions, reusable cards, accessible summaries.
- `state.rs`: refresh coordination, cancellation, request deduplication, stale-cache fallback.
- `openmeteo.rs`, `weatherkit.rs`, and `models.rs`: interchangeable forecast providers and forward-compatible internal models.
- `radar.rs`: separately attributed RainViewer/OpenStreetMap radar tile client and native GTK map.
- `auth.rs`: keyring interface and local short-lived ES256 JWT provider; ready for a future broker implementation.
- `geocode.rs` and `location.rs`: Open-Meteo/GeoNames place search, offline timezone lookup, and consent-gated GeoClue.
- `storage.rs`, `cache.rs`, `settings.rs`: SQLite migrations, expiry-aware temporary forecasts, and XDG configuration.
- `conditions.rs` and `units.rs`: original condition presentation and conversions.

Unknown WeatherKit JSON fields are retained in flattened maps. Optional datasets and fields never gate decoding of the rest of a response.
