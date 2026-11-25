import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const repositoryRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const source = readFileSync(resolve(repositoryRoot, "deploy/scripts/web-server.mjs"), "utf8");
const widgetV1 = await import(resolve(repositoryRoot, "apps/web/dist/widget-forecast.js"));
const widgetV2 = await import(resolve(repositoryRoot, "apps/web/dist/widget-forecast-v2.js"));
const widgetV3 = await import(resolve(repositoryRoot, "apps/web/dist/widget-forecast-v3.js"));
const widgetFixtures = await import(resolve(repositoryRoot, "scripts/widget-fixtures.mjs"));

// extract one source span between stable neighboring declarations
function sourceSpan(startMarker, endMarker) {
  const start = source.indexOf(startMarker);
  const end = source.indexOf(endMarker, start);
  // fail if either exact production declaration moved
  if (start < 0 || end < 0) {
    throw new Error(`unable to extract ${startMarker}`);
  }
  return source.slice(start, end);
}

const operation = process.argv[2];
// execute the actual cache and timeout functions from the Node edge
if (operation === "api") {
  const pathname = process.argv[3];
  const cacheSource = sourceSpan(
    "function apiCacheControl(pathname)",
    "// allow one bounded cold annual aggregation",
  );
  const timeoutSource = sourceSpan(
    "function apiRequestTimeoutMs(pathname)",
    "const XWEATHER_LAYERS",
  );
  const result = vm.runInNewContext(
    `${cacheSource}\n${timeoutSource}\napiCacheControl(pathname) + "\\t" + apiRequestTimeoutMs(pathname)`,
    { pathname },
  );
  process.stdout.write(result);
} else if (operation === "preload") {
  const pathname = process.argv[3];
  const epochMs = Number(process.argv[4]);
  const frameEpochMs = Number(process.argv[5]);
  const validTime = process.argv[6];
  const latitude = Number(process.argv[7]);
  const longitude = Number(process.argv[8]);
  const preloadSource = sourceSpan(
    "function forecastMapPreloadLink(pathname)",
    "// load only the public map center",
  );
  const result = vm.runInNewContext(
    `const NativeDate = globalThis.Date;
     class Date extends NativeDate { static now() { return epochMs; } }
     const forecastMapPreloadSite = { latitude, longitude };
     ${preloadSource}
     function formatXweatherValidTime(instant) {
       if (instant.getTime() !== frameEpochMs) throw new Error("unexpected frame");
       return validTime;
     }
     forecastMapPreloadLink(pathname);`,
    { epochMs, frameEpochMs, latitude, longitude, pathname, validTime },
  );
  process.stdout.write(result);
} else if (operation === "widget-v2-hour") {
  const cloudCoverPercent = nullableNumber(process.argv[3]);
  const windSpeedMps = nullableNumber(process.argv[4]);
  const adjustedWindSpeedMps = nullableNumber(process.argv[5]);
  const expiry = process.argv[6];
  const rawField = {
    mode: "raw",
    raw: 1,
    rawSource: null,
    reason: "raw_forecast",
    selected: 1,
    selectedSource: null,
    selectedUntil: null,
  };
  const hour = {
    end: "2026-09-12T16:00:00.000Z",
    rainMmPerHour: rawField,
    start: "2026-09-12T15:00:00.000Z",
    temperatureC: rawField,
  };
  const record = {
    adjustment: adjustedWindSpeedMps === null
      ? undefined
      : {
          adjustedMetrics: { windSpeedMps: adjustedWindSpeedMps },
          appliedMetrics: ["windSpeedMps"],
          state: "active",
        },
    metrics: { cloudCoverPercent, windSpeedMps },
    productRunAt: "2026-09-12T12:00:00.000Z",
    receivedAt: "2026-09-12T15:00:00.000Z",
  };
  const runtime = expiry === "unavailable"
    ? {}
    : { expiresAt: expiry === "permanent" ? null : expiry };
  const projected = widgetV2.projectWidgetForecastV2Hour(
    hour,
    record,
    "2026-09-12T15:15:00.000Z",
    runtime,
  );
  const result = [
    fieldSignature(projected.cloudCoverPercent),
    fieldSignature(projected.windSpeedMps),
    widgetV2.summarizeWidgetForecastV2Status([projected]),
  ].join("\n");
  process.stdout.write(result);
} else if (operation === "widget-v2-status") {
  const mode = process.argv[3];
  const fields = mode === "empty"
    ? []
    : [{
        temperatureC: { mode },
        rainMmPerHour: { mode },
        cloudCoverPercent: { mode },
        windSpeedMps: { mode },
      }];
  process.stdout.write(widgetV2.summarizeWidgetForecastV2Status(fields));
} else if (operation === "widget-v1-hour") {
  const rainRate = nullableNumber(process.argv[3]);
  const rainAmount = nullableNumber(process.argv[4]);
  const temperature = nullableNumber(process.argv[5]);
  const branch = process.argv[6];
  const selected = nullableNumber(process.argv[7]);
  const deadline = process.argv[8];
  const ordering = process.argv[9];
  const generatedAt = "2026-09-12T15:15:00.000Z";
  const rawReceivedAt = ordering === "raw-future"
    ? "2026-09-12T15:16:00.000Z"
    : "2026-09-12T15:00:00.000Z";
  const selectedReceivedAt = ordering === "selected-future"
    ? "2026-09-12T15:16:00.000Z"
    : "2026-09-12T14:50:00.000Z";
  const sourceForecast = ordering === "selected-incomplete"
    ? null
    : {
        decisionAt: deadline === "unavailable" ? undefined : deadline,
        firstReceivedAt: selectedReceivedAt,
        runInitializedAt: "2026-09-12T12:00:00.000Z",
      };
  const record = branch === "missing"
    ? null
    : {
        adjustment: branch === "generic-temperature"
          ? {
              adjustedMetrics: { temperatureC: selected },
              appliedMetrics: ["temperatureC"],
              state: "active",
            }
          : { state: "disabled" },
        metrics: {
          precipitationMm: rainAmount,
          precipitationRateMmPerHour: rainRate,
          temperatureC: temperature,
        },
        productRunAt: "2026-09-12T12:00:00.000Z",
        rainAdjustment: branch === "rain"
          ? {
              correctedPrecipitationMm: selected,
              sourceForecast,
              state: "active",
            }
          : { state: "disabled" },
        receivedAt: rawReceivedAt,
        temperatureAdjustment: branch === "independent-temperature"
          ? {
              correctedTemperatureC: selected,
              sourceForecast,
              state: "active",
            }
          : { state: "disabled" },
      };
  const runtime = deadlineValue(deadline);

  try {
    const projected = widgetV1.projectWidgetForecastHour(
      record,
      "2026-09-12T15:00:00.000Z",
      {
        generatedAt,
        parsed: {
          adjustmentRuntime: runtime,
          temperatureAdjustmentRuntime: runtime,
        },
      },
    );
    process.stdout.write([
      "ok",
      projected.start,
      projected.end,
      fieldSignature(projected.rainMmPerHour),
      fieldSignature(projected.temperatureC),
    ].join("\n"));
  } catch (error) {
    // expose the stable projector diagnostic for Rust error parity
    process.stdout.write(`error\t${error.message}`);
  }
} else if (operation === "widget-v3-snapshot") {
  const mode = process.argv[3];
  const row = (index, record) => ({
    ...record,
    metrics: {
      ...record.metrics,
      cloudCoverPercent: 20 + index,
      precipitationMm: null,
      precipitationRateMmPerHour: null,
      temperatureC: null,
      windSpeedMps: 4 + index / 10,
    },
  });
  const fixture = widgetFixtures.createForecastFixture({
    date: "2026-09-12",
    generic: true,
    row,
  });
  const next = widgetFixtures.createForecastFixture({
    date: "2026-09-13",
    generic: true,
    row,
  });
  fixture.input.data.push(...next.input.data.slice(0, 7));
  fixture.input.adjustmentRuntime.expiresAt = null;
  fixture.input.adjustmentRuntime.enabledMetrics = ["windSpeedMps"];

  // retarget every generic decision to the v3 wind field
  for (const record of fixture.input.data) {
    record.adjustment.adjustedMetrics = {
      windSpeedMps: record.metrics.windSpeedMps + 3,
    };
    record.adjustment.appliedMetrics = ["windSpeedMps"];
  }

  // exercise one genuine missing interval in the successful response
  if (mode === "gap") {
    fixture.input.data.splice(27, 1);
  } else if (mode === "duplicate") {
    fixture.input.data.push(structuredClone(fixture.input.data[0]));
  } else if (mode === "foreign") {
    const final = fixture.input.data.at(-1);
    final.validAt = new Date(Date.parse(final.validAt) + 7 * 60 * 60_000).toISOString();
  }
  const receivedAt = mode === "future-generation"
    ? "2026-09-12T15:14:59.999Z"
    : fixture.receivedAt;

  try {
    const projected = widgetV3.projectWidgetForecastV3(fixture.input, receivedAt);
    process.stdout.write(widgetV3Signature(projected));
  } catch (error) {
    // expose the stable projector diagnostic for Rust response parity
    process.stdout.write(`error\t${error.message}`);
  }
} else if (operation === "widget-path") {
  const pathname = process.argv[3];
  const widgetSource = sourceSpan(
    "function isWidgetForecastPath(pathname)",
    "// serve one bounded public widget snapshot",
  );
  const result = vm.runInNewContext(
    `${widgetSource}\nisWidgetForecastPath(pathname);`,
    {
      pathname,
      widgetForecastPath: "/api/v1/sites/ballydidean/widget-forecast",
      widgetForecastV2Path: "/api/v2/sites/ballydidean/widget-forecast",
      widgetForecastV3Path: "/api/v3/sites/ballydidean/widget-forecast",
    },
  );
  process.stdout.write(String(result));
} else if (operation === "asset") {
  const release = process.argv[3];
  const pathname = process.argv[4];
  const catalogSource = sourceSpan(
    "const assetPrefix =",
    "// bind every immutable executable and stylesheet",
  );
  const resolverSource = sourceSpan(
    "function resolveAsset(pathname)",
    "// redirect one validated RemoteAgents preview target",
  );
  const result = vm.runInNewContext(
    `${catalogSource}\n${resolverSource}
     const asset = resolveAsset(pathname);
     asset === undefined ? "missing" : [
       asset.path,
       asset.cache,
       asset.type,
       String(asset.template === true),
       String(asset.analytics !== false),
     ].join("\t");`,
    {
      compiledRoot: "compiled",
      join: (...parts) => parts.join("/"),
      pathname,
      publicRoot: "public",
      release,
    },
  );
  process.stdout.write(result);
} else {
  throw new Error("unknown oracle operation");
}

// decode one command-line nullable number
function nullableNumber(value) {
  return value === "null" || value === "none" ? null : Number(value);
}

// preserve the runtime expiry field's absent, null, and finite states
function deadlineValue(value) {
  // omit the field when the runtime could not provide a deadline
  if (value === "unavailable") {
    return {};
  }
  return { expiresAt: value === "permanent" ? null : value };
}

// flatten one canonical field for dependency-free Rust comparison
function fieldSignature(field) {
  return [
    field.mode,
    field.raw ?? "null",
    field.rawSource?.runAt ?? "null",
    field.rawSource?.receivedAt ?? "null",
    field.reason,
    field.selected ?? "null",
    field.selectedSource?.runAt ?? "null",
    field.selectedSource?.receivedAt ?? "null",
    field.selectedUntil ?? "null",
  ].join("\t");
}

// flatten one complete canonical v3 snapshot without JSON serialization
function widgetV3Signature(snapshot) {
  const header = [
    "ok",
    snapshot.schemaVersion,
    snapshot.attribution.label,
    snapshot.attribution.licenseUrl,
    snapshot.attribution.providerUrl,
    snapshot.calendar.date,
    snapshot.calendar.dayStart,
    snapshot.calendar.cutoff,
    snapshot.calendar.dayEnd,
    snapshot.calendar.overnightEnd,
    snapshot.calendar.sunset ?? "null",
    snapshot.generatedAt,
    snapshot.receivedAt,
    snapshot.site.name,
    snapshot.site.slug,
    snapshot.site.timezone,
    String(snapshot.site.latitude),
    String(snapshot.site.longitude),
    snapshot.status,
    String(snapshot.hours.length),
  ];
  const hours = snapshot.hours.flatMap(
    // retain every ordered hour and its four public fields
    (hour) => [
      hour.start,
      hour.end,
      fieldSignature(hour.rainMmPerHour),
      fieldSignature(hour.temperatureC),
      fieldSignature(hour.cloudCoverPercent),
      fieldSignature(hour.windSpeedMps),
    ],
  );
  return [...header, ...hours].join("\n");
}
