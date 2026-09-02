# Security and privacy

The `.p8` private key is written only to GNOME Secret Service over encrypted session D-Bus and is wrapped as a secret in memory. Team ID, Key ID, and Service ID are non-secret settings. Credentials never enter SQLite, the forecast cache, logs, package, fixtures, or crash text. JWTs use ES256 and expire after 15 minutes.

`WEATHERGLASS_WEATHERKIT_PRIVATE_KEY` is supported only for local development; do not put it in shell history. There are intentionally no environment fallbacks for the other identifiers.

Forecast requests go to the provider selected in Settings: `api.open-meteo.com`/`air-quality-api.open-meteo.com`, or `weatherkit.apple.com`. User-triggered place queries go to `geocoding-api.open-meteo.com`. Radar mode requests public metadata/tiles from RainViewer and base-map tiles from OpenStreetMap. GeoClue is contacted only after an explicit “Allow Once” confirmation.

WeatherKit data may be inaccurate and must not be used for emergency or life-saving decisions. Use is at the user’s sole risk. WeatherKit data is retained only temporarily to improve application performance and is visibly marked stale after expiry.
