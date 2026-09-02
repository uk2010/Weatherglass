# Capability matrix

Checked against Apple WeatherKit, Open-Meteo, and RainViewer public documentation on 2026-09-02.

| Visible capability | Default/free source | Weatherglass 0.0.1 |
|---|---|---|
| Current conditions and apparent temperature | Open-Meteo Forecast API | Supported |
| Hourly forecast and weather metrics | Open-Meteo Forecast API | Supported; compact timeline and on-demand accessible trend chart |
| Ten-day forecast | Open-Meteo Forecast API | Supported |
| Next-hour precipitation | Open-Meteo 15-minute forecast; WeatherKit has minute data | Retrieved but intentionally omitted from the compact main layout; precipitation remains visible hourly |
| Severe-weather alerts | No Open-Meteo alert feed; optional WeatherKit `weatherAlerts` | Supported only with WeatherKit; original text/link retained |
| Sunrise/sunset and moon details | Open-Meteo daily fields | Supported when present; moon illumination uses clear fractional terms |
| Saved-place list | Local application feature | Supported in SQLite; add, preview, select, rename, delete, drag/button reorder |
| Place search | Open-Meteo Geocoding API (GeoNames) | Supported; direct coordinates and GeoClue also supported |
| Precipitation radar and forecast map | RainViewer observations + Open-Meteo forecast grid + OpenStreetMap base | Observed-to-forecast animation with explicit labels; draggable preview and full pan/zoom map |
| Temperature map layer | Open-Meteo forecast grid | Supported with interpolated colour layer and values |
| Wind map layer | Open-Meteo forecast grid | Supported with interpolated speed layer, values, and direction marks |
| Air-quality readings and map layer | Open-Meteo Air Quality API / CAMS | Supported with regional AQI scale and interpolated map layer |
| Weather news | **No documented public REST dataset** | Hidden |
| Historical climate averages | No field in the requested current public datasets | Hidden; no bulk collection |

Apple’s REST datasets still do not expose Apple’s proprietary map tiles, air quality, news, or place search. Weatherglass does not scrape them. Open-Meteo is the clearly attributed default forecast and modelled-map provider; WeatherKit is optional; RainViewer supplies observed precipitation radar only.

Sources: [Open-Meteo forecast](https://open-meteo.com/en/docs), [air quality](https://open-meteo.com/en/docs/air-quality-api), [geocoding](https://open-meteo.com/en/docs/geocoding-api), [licence](https://open-meteo.com/en/license), [RainViewer API](https://www.rainviewer.com/api.html), [WeatherKit datasets](https://developer.apple.com/documentation/weatherkitrestapi/dataset).
