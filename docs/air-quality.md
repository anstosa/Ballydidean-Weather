# App-calculated air quality

Air-quality displays use the US **PM2.5 AQI**, calculated by Weather from raw
PM2.5 concentrations. They do not use the gateway's `PM25_RealAQI`,
`PM25_24HAQI`, or `PM25_24H` fields. Raw API metrics, stored measurements, and
forecast correction inputs remain in µg/m³. This is a local sensor/model
estimate for PM2.5, not a regulatory monitor or an index of every pollutant.

## Current conditions, sensors, and stations

Current displays use the EPA PM NowCast: twelve completed hourly concentration
means, newest first, with at least two valid hours in the latest three. The
adaptive weight is `max(0.5, minimum / maximum)`; all-zero history uses weight
one. Missing terms are omitted without compressing their actual hourly ages.
The calculation stays anchored to the latest completed hour, not the newest
available sample, so expired history cannot appear current. Each source and
WH41 sensor retains its own history. Nearby fallback concentrations are
combined before calculating NowCast, never by averaging AQI scores. Raw mode
can use the regional model's past hourly forecasts for a modeled NowCast.

Missing or insufficient history displays unavailable, not zero and not an
instantaneous concentration converted to AQI.

## Forecasts and Trends

Forecast AQI uses the mean of all **24 predicted hourly PM2.5 concentrations**
in a standard-time day. The chart repeats that daily forecast across the day;
it is not an hourly AQI forecast. Incomplete forecast days are unavailable.
Recorded observations are not mixed into the predicted daily mean.

Trends uses completed daily farm PM2.5 means with at least **18 valid hourly
means out of 24**. Hourly means have equal weight, independent of polling
frequency. Existing weather trend source precedence is unchanged. Conversion
to daily AQI occurs before optional seven-day smoothing or historical
percentiles.

Daily boundaries use **local standard time**, not daylight-saving time. At
Ballydidean this is UTC−08:00 year-round, so every AQI day contains 24 hours.

## Conversion and display

The averaged concentration is truncated to one decimal place, interpolated
using the EPA PM2.5 breakpoints effective May 6, 2024, and rounded to the
nearest integer. Values above AQI 500 continue the final interpolation segment
rather than being clamped. All displayed scores include `AQI`, independently
of weather unit preferences. Health categories use AQI thresholds 50, 100,
150, 200, and 300.

References: [EPA AQI technical assistance document](https://document.airnow.gov/technical-assistance-document-for-the-reporting-of-daily-air-quailty.pdf),
[AirNow NowCast clarification](https://forum.airnowtech.org/t/the-nowcast-for-pm2-5-and-pm10/172),
[AirNow daily PM2.5 completeness](https://forum.airnowtech.org/t/daily-and-hourly-aqi-pm2-5/171),
and [AQS time handling](https://aqs.epa.gov/aqsweb/documents/about_aqs_data.html).
