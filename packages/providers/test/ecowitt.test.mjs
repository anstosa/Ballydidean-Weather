import assert from "node:assert/strict";
import test from "node:test";

import {
  ProviderFailure,
  buildEcowittLiveRequest,
  fetchEcowittLive,
  normalizeEcowittLivePayload,
} from "../dist/index.js";

const receivedAt = "2026-08-29T14:00:00.000Z";
const request = {
  expectedMac: "88:F1:55:05:D8:9F",
  gatewayHost: "192.168.11.137",
  model: "GW3000",
  previousCursor: null,
  sourceId: "source-ecowitt",
  timezone: "America/Los_Angeles",
};
const livePayload = {
  ch_aisle: [
    { battery: "0", channel: "1", humidity: "65%", name: "", temp: "68.0", unit: "F" },
  ],
  ch_ec: [
    {
      battery: "5",
      channel: "1",
      ec: "10 uS/cm",
      humidity: "42%",
      name: "",
      temp: "63.9",
      unit: "F",
    },
  ],
  ch_pm25: [
    { PM25: "12.4", battery: "6", channel: "1" },
  ],
  common_list: [
    { id: "0x02", unit: "F", val: "54.3" },
    { id: "0x07", val: "96%" },
    { id: "3", unit: "F", val: "53.6" },
    { battery: "4", id: "0xA1", unit: "F", val: "54.3" },
    { id: "0xA2", unit: "F", val: "53.6" },
    { id: "0x0B", val: "5.00 mph" },
    { id: "0x0C", val: "10.00 mph" },
    { id: "0x15", val: "21.94 W/m2" },
    { id: "0x17", val: "2" },
    { id: "0x0A", val: "47" },
  ],
  piezoRain: [
    { id: "0x0E", val: "0.02 in/Hr" },
    { id: "0x10", val: "0.15 in" },
    { battery: "3", id: "0x13" },
  ],
  wh25: [
    { abs: "29.80 inHg", inhumi: "70%", intemp: "65.3", rel: "29.88 inHg", unit: "F" },
  ],
};

// create deterministic local HTTP responses
function responseFetcher(networkPayload = { mac: request.expectedMac }) {
  return async (input) => {
    const url = new URL(String(input));

    // return the configured gateway identity
    if (url.pathname === "/get_network_info") {
      return Response.json(networkPayload);
    }

    // return one live gateway snapshot
    if (url.pathname === "/get_livedata_info") {
      return Response.json(livePayload);
    }

    return new Response("not found", { status: 404 });
  };
}

// require one normalized property sensor by key
function propertySensor(record, key) {
  const sensor = record.metadata.provider.property_sensors.find(
    // match one stable hardware key
    (candidate) => candidate.key === key,
  );
  assert.ok(sensor);
  return sensor;
}

// constrain the adapter to the configured private gateway
test("Ecowitt request targets the LAN live-data endpoint", () => {
  const plan = buildEcowittLiveRequest(request);

  assert.equal(plan.adapterVersion, "ecowitt-local-live/v1");
  assert.equal(plan.capability, "current");
  assert.equal(plan.sourceKind, "physical_sensor");
  assert.equal(plan.url.href, "http://192.168.11.137/get_livedata_info");
  assert.throws(
    () => buildEcowittLiveRequest({ ...request, gatewayHost: "weather.example" }),
    /private IPv4/u,
  );
});

// normalize the installed gateway's unique measurement set
test("Ecowitt live payload normalizes core, heat, air, and soil measurements", () => {
  const record = normalizeEcowittLivePayload(livePayload, request, receivedAt);

  assert.equal(record.validAt, receivedAt);
  assert.equal(record.sourceKind, "physical_sensor");
  assert.ok(Math.abs(record.metrics.temperatureC - 12.38888888888889) < 1e-12);
  assert.ok(Math.abs(record.metrics.apparentTemperatureC - 12) < 1e-12);
  assert.equal(record.metrics.relativeHumidityPercent, 96);
  assert.ok(Math.abs(record.metrics.pressureHpa - 1011.8529336009959) < 1e-9);
  assert.ok(Math.abs(record.metrics.windSpeedMps - 2.2352) < 1e-12);
  assert.ok(Math.abs(record.metrics.windGustMps - 4.4704) < 1e-12);
  assert.equal(record.metrics.windDirectionDegrees, 47);
  assert.ok(Math.abs(record.metrics.precipitationMm - 3.81) < 1e-12);
  assert.ok(
    Math.abs(record.metrics.precipitationRateMmPerHour - 0.508) < 1e-12,
  );
  assert.ok(
    Math.abs(record.metrics.blackGlobeTemperatureC - 12.38888888888889) < 1e-12,
  );
  assert.ok(Math.abs(record.metrics.wetBulbGlobeTemperatureC - 12) < 1e-12);
  assert.equal(record.metrics.solarRadiationWm2, 21.94);
  assert.equal(record.metrics.uvIndex, 2);
  assert.equal(record.metrics.pm25MicrogramsPerCubicMeter, 12.4);
  assert.equal(record.metrics.soilMoisturePercent, 42);
  assert.equal(record.metrics.soilElectricalConductivityMicrosiemensPerCm, 10);
  assert.deepEqual(record.metadata.device, {
    model: "GW3000",
    serial: "88:F1:55:05:D8:9F",
    vendor: "Ecowitt",
  });
  assert.deepEqual(record.metadata.provider.property_sensors, [
    {
      channel: null,
      key: "gateway",
      model: "GW3000",
      readings: {
        pressureHpa: record.metrics.pressureHpa,
        relativeHumidityPercent: 70,
        temperatureC: 18.5,
      },
    },
    {
      channel: null,
      key: "weather-array",
      model: "WS90",
      readings: {
        batteryLevel: 3,
        dailyPrecipitationMm: record.metrics.precipitationMm,
        precipitationRateMmPerHour: 0.508,
        relativeHumidityPercent: 96,
        solarRadiationWm2: 21.94,
        temperatureC: record.metrics.temperatureC,
        uvIndex: 2,
        windDirectionDegrees: 47,
        windGustMps: record.metrics.windGustMps,
        windSpeedMps: record.metrics.windSpeedMps,
      },
    },
    {
      channel: null,
      key: "black-globe",
      model: "WN38",
      readings: {
        batteryLevel: 4,
        blackGlobeTemperatureC: record.metrics.blackGlobeTemperatureC,
        wetBulbGlobeTemperatureC: record.metrics.wetBulbGlobeTemperatureC,
      },
    },
    {
      channel: 1,
      key: "temperature-1",
      model: "WN31",
      readings: {
        batteryLow: 0,
        relativeHumidityPercent: 65,
        temperatureC: 20,
      },
    },
    {
      channel: 1,
      key: "soil-1",
      model: "WH52",
      readings: {
        batteryLevel: 5,
        soilElectricalConductivityMicrosiemensPerCm: 10,
        soilMoisturePercent: 42,
        temperatureC: 17.72222222222222,
      },
    },
    {
      channel: 1,
      key: "air-quality-1",
      model: "WH41",
      readings: {
        batteryExternalPower: 1,
        pm25MicrogramsPerCubicMeter: 12.4,
      },
    },
  ]);
});

// preserve only the gateway's documented battery state ranges
test("Ecowitt property sensors retain bounded battery states", () => {
  const variants = [
    { soil: 0, temperature: 1 },
    { soil: 5, temperature: 0 },
  ];

  // exercise numeric boundaries independently from the string fixture
  for (const variant of variants) {
    const payload = {
      ...livePayload,
      ch_aisle: [{ ...livePayload.ch_aisle[0], battery: variant.temperature }],
      ch_ec: [{ ...livePayload.ch_ec[0], battery: variant.soil }],
    };
    const record = normalizeEcowittLivePayload(payload, request, receivedAt);
    const temperature = propertySensor(record, "temperature-1");
    const soil = propertySensor(record, "soil-1");

    assert.equal(temperature.readings.batteryLow, variant.temperature);
    assert.equal(soil.readings.batteryLevel, variant.soil);
  }
});

// omit malformed battery metadata without losing healthy weather readings
test("Ecowitt property sensors omit unknown battery states", () => {
  const invalidStates = [
    undefined,
    null,
    "",
    " ",
    "9",
    9,
    "1.0",
    1.5,
    true,
    false,
    [],
    {},
    Number.NaN,
    Number.POSITIVE_INFINITY,
    "-1",
    "1 low",
  ];

  // require every invalid shape to remain non-fatal
  for (const invalidState of invalidStates) {
    const payload = {
      ...livePayload,
      ch_aisle: [{ ...livePayload.ch_aisle[0], battery: invalidState }],
      ch_ec: [{ ...livePayload.ch_ec[0], battery: invalidState }],
      ch_pm25: [{ ...livePayload.ch_pm25[0], battery: invalidState }],
      common_list: livePayload.common_list.map(
        // replace only the black-globe battery state
        (entry) => entry.id === "0xA1"
          ? { ...entry, battery: invalidState }
          : entry,
      ),
      piezoRain: livePayload.piezoRain.map(
        // replace only the weather-array battery state
        (entry) => entry.id === "0x13"
          ? { ...entry, battery: invalidState }
          : entry,
      ),
      rain: [
        { id: "0x0E", val: "0.01 in/Hr" },
        { id: "0x10", val: "0.10 in" },
        { battery: invalidState, id: "0x13" },
      ],
    };
    const record = normalizeEcowittLivePayload(payload, request, receivedAt);
    const temperature = propertySensor(record, "temperature-1");
    const soil = propertySensor(record, "soil-1");
    const weatherArray = propertySensor(record, "weather-array");
    const globe = propertySensor(record, "black-globe");
    const rain = propertySensor(record, "rain-gauge");
    const airQuality = propertySensor(record, "air-quality-1");

    assert.equal("batteryLow" in temperature.readings, false);
    assert.equal("batteryLevel" in soil.readings, false);
    assert.equal("batteryLevel" in weatherArray.readings, false);
    assert.equal("batteryLevel" in globe.readings, false);
    assert.equal("batteryLevel" in rain.readings, false);
    assert.equal("batteryLevel" in airQuality.readings, false);
    assert.equal("batteryExternalPower" in airQuality.readings, false);
    assert.equal(temperature.readings.temperatureC, 20);
    assert.equal(soil.readings.soilMoisturePercent, 42);
  }
});

// associate coexisting rain batteries with their exact hardware groups
test("Ecowitt rain sensors retain independent battery levels", () => {
  const payload = {
    ...livePayload,
    rain: [
      { id: "0x0E", val: "0.01 in/Hr" },
      { id: "0x10", val: "0.10 in" },
      { battery: "2", id: "0x13" },
    ],
  };
  const record = normalizeEcowittLivePayload(payload, request, receivedAt);
  const weatherArray = propertySensor(record, "weather-array");
  const rain = propertySensor(record, "rain-gauge");

  assert.equal(weatherArray.readings.batteryLevel, 3);
  assert.equal(rain.readings.batteryLevel, 2);
  assert.equal("dailyPrecipitationMm" in weatherArray.readings, false);
  assert.equal(rain.readings.dailyPrecipitationMm, 2.54);
});

// reserve battery state six for externally powered WH41 sensors
test("Ecowitt external-power sentinel remains specific to air quality", () => {
  const payload = {
    ...livePayload,
    ch_aisle: [{ ...livePayload.ch_aisle[0], battery: 6 }],
    ch_ec: [{ ...livePayload.ch_ec[0], battery: 6 }],
    ch_pm25: [{ ...livePayload.ch_pm25[0], battery: 6 }],
    common_list: livePayload.common_list.map(
      // replace only the black-globe battery state
      (entry) => entry.id === "0xA1" ? { ...entry, battery: 6 } : entry,
    ),
    piezoRain: livePayload.piezoRain.map(
      // replace only the weather-array battery state
      (entry) => entry.id === "0x13" ? { ...entry, battery: 6 } : entry,
    ),
    rain: [
      { id: "0x0E", val: "0.01 in/Hr" },
      { id: "0x10", val: "0.10 in" },
      { battery: 6, id: "0x13" },
    ],
  };
  const record = normalizeEcowittLivePayload(payload, request, receivedAt);
  const keys = [
    "temperature-1",
    "soil-1",
    "weather-array",
    "black-globe",
    "rain-gauge",
  ];

  // reject the sentinel from every battery-level family
  for (const key of keys) {
    const sensor = propertySensor(record, key);
    assert.equal("batteryLevel" in sensor.readings, false);
    assert.equal("batteryExternalPower" in sensor.readings, false);
  }

  const airQuality = propertySensor(record, "air-quality-1");
  assert.equal("batteryLevel" in airQuality.readings, false);
  assert.equal(airQuality.readings.batteryExternalPower, 1);
});

// retain interval rain without summing the gateway's daily total repeatedly
test("Ecowitt rain cursor stores only the daily-counter delta", () => {
  const record = normalizeEcowittLivePayload(
    livePayload,
    {
      ...request,
      previousCursor: {
        rain_daily_total_mm: 2.54,
        rain_day: "2026-08-29",
      },
    },
    receivedAt,
  );
  const reset = normalizeEcowittLivePayload(
    livePayload,
    {
      ...request,
      previousCursor: {
        rain_daily_total_mm: 5,
        rain_day: "2026-08-29",
      },
    },
    receivedAt,
  );

  assert.ok(Math.abs(record.metrics.precipitationMm - 1.27) < 1e-9);
  assert.equal(reset.metrics.precipitationMm, null);
});

// verify gateway identity before accepting local data
test("Ecowitt fetch binds snapshots to the configured MAC", async () => {
  const requests = [];
  const fetcher = responseFetcher();
  // record the complete gateway request surface
  const trackedFetcher = async (input, init) => {
    requests.push(new URL(String(input)).pathname);
    return await fetcher(input, init);
  };
  const batch = await fetchEcowittLive(request, {
    fetch: trackedFetcher,
    maxAttempts: 1,
    now: () => new Date(receivedAt),
  });

  assert.equal(batch.attempts, 2);
  assert.equal(batch.records.length, 1);
  assert.equal(batch.providerCursor.rain_day, "2026-08-29");
  assert.ok(Math.abs(batch.providerCursor.rain_daily_total_mm - 3.81) < 1e-12);
  assert.equal(batch.responseMetadata.rain_gauge, "piezo");
  assert.equal(batch.responseMetadata.soil_channel_count, 1);
  assert.deepEqual(requests, ["/get_network_info", "/get_livedata_info"]);
  await assert.rejects(
    fetchEcowittLive(request, {
      fetch: responseFetcher({ mac: "00:11:22:33:44:55" }),
      maxAttempts: 1,
      now: () => new Date(receivedAt),
    }),
    (error) =>
      error instanceof ProviderFailure &&
      error.ingestionError.code === "ecowitt_invalid_payload",
  );
});
