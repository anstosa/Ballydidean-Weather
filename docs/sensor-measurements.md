# Sensor measurement inventory

The normalized weather point supports every unique environmental measurement in
the purchased Ecowitt sensor set. A measurement has one canonical field even
when several devices report it; the source record identifies the originating
gateway or sensor.

The first-party GW3000 is polled from its local LAN API once per minute. The
normalized live record selects the primary outdoor readings and the first soil
channel so repeated measurements from the purchased multi-channel sensors do
not become duplicate fields. Additional channel-aware storage is a separate
telemetry/modeling concern.

## Canonical point measurements

| API field | Database column | Canonical unit | Supplied by |
| --- | --- | --- | --- |
| `temperatureC` | `temperature_c` | °C | GW3000, WS90, WH52, WN30, WN31 |
| `apparentTemperatureC` | `apparent_temperature_c` | °C | Open-Meteo |
| `blackGlobeTemperatureC` | `black_globe_temperature_c` | °C | WN38 |
| `wetBulbGlobeTemperatureC` | `wet_bulb_globe_temperature_c` | °C | WN38 with a compatible outdoor temperature/humidity source |
| `relativeHumidityPercent` | `relative_humidity_percent` | % | GW3000, WS90, WN31 |
| `pressureHpa` | `pressure_hpa` | hPa | GW3000, Open-Meteo |
| `precipitationMm` | `precipitation_mm` | mm per observation interval | WH40H, WS90, Open-Meteo |
| `precipitationRateMmPerHour` | `precipitation_rate_mm_per_hour` | mm/h | WH40H, WS90 |
| `windSpeedMps` | `wind_speed_mps` | m/s | WS90, Open-Meteo |
| `windGustMps` | `wind_gust_mps` | m/s | WS90, Open-Meteo |
| `windDirectionDegrees` | `wind_direction_degrees` | degrees | WS90, Open-Meteo |
| `solarRadiationWm2` | `solar_radiation_wm2` | W/m² | WS90 |
| `uvIndex` | `uv_index` | UV index | WS90 |
| `pm25MicrogramsPerCubicMeter` | `pm25_micrograms_per_cubic_meter` | µg/m³ | WH41 |
| `soilMoisturePercent` | `soil_moisture_percent` | % | WH52 |
| `soilElectricalConductivityMicrosiemensPerCm` | `soil_electrical_conductivity_us_cm` | µS/cm | WH52 |
| `cloudCoverPercent` | `cloud_cover_percent` | % | Open-Meteo |

## Deduplication boundary

- Temperature and humidity are not duplicated by device model or channel.
- WH40H and WS90 rainfall use the same precipitation fields.
- Daily, weekly, monthly, yearly, event, and lifetime rainfall totals are
  derived from normalized interval amounts rather than stored as separate point
  measurements.
- [App-calculated AQI](air-quality.md) uses the normalized PM2.5 series:
  current values use the EPA NowCast and daily values use standard-time
  24-hour averages. Vendor AQI and vendor 24-hour averages are not ingested.
- Relative pressure is derived from the canonical measured pressure and site
  elevation rather than stored as a second pressure measurement.
- Battery condition and radio health are device telemetry, not weather point
  measurements, and belong in a separate telemetry record.

## Device references

- [GW3000 gateway](https://shop.ecowitt.com/products/gw3000)
- [WS90 sensor array](https://shop.ecowitt.com/products/ws90)
- [WH41 PM2.5 sensor](https://shop.ecowitt.com/products/wh41)
- [WH52 soil sensor](https://shop.ecowitt.com/products/wh52)
- [WN30 probe thermometer](https://shop.ecowitt.com/products/wn30)
- [WN31 temperature and humidity sensor](https://shop.ecowitt.com/products/wn31)
- [WN38 black globe thermometer](https://shop.ecowitt.com/products/wn38)
- [WH40H rain gauge](https://shop.ecowitt.com/products/wh40h)

## Tide data

| Measurement | Canonical unit | Source |
| --- | --- | --- |
| Water level relative to MLLW | meter | NOAA CO-OPS observed water levels and tide predictions |
