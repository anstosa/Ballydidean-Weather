import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  symlinkSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import ts from "typescript";

import * as generatorModule from "./generate-rust-contracts.mjs";

const { generateRustContracts } = generatorModule;
const CONFIG = freezeRepositoryFixture(JSON.parse(
  readFileSync(new URL("../config/rust-contract-roots.json", import.meta.url), "utf8"),
));
const SOURCES = Object.freeze(repositorySources(CONFIG));
let sharedRepositoryProjection;

// keep the exact shared input immutable across all contract assertions
function freezeRepositoryFixture(value) {
  // leave json scalars untouched
  if (value !== null && typeof value === "object") {
    // freeze every nested config record and array
    Object.values(value).forEach(freezeRepositoryFixture);
    Object.freeze(value);
  }
  return value;
}

const REPOSITORY_ROOT_COUNT = 424;
const REPOSITORY_SOURCE_COUNT = 85;
const REPOSITORY_IMPORT_COUNT = 61;
const TEMPERATURE_DATA_WIRE_ROOTS = [
  "ForecastAdjustmentPerformancePackageInputs",
  "TemperaturePerformanceScoredPairMetadata",
  "TemperaturePerformanceDataRow",
  "TemperaturePerformanceDataWire",
].map((name) => ({
  source: "apps/worker/src/forecast-adjustment-performance-cli.ts",
  exportName: name,
  rustName: name,
  kind: "readonly-interface",
}));

// generate one immutable repository projection per test process
function repositoryProjection() {
  sharedRepositoryProjection ??= Object.freeze(generateRustContracts(CONFIG, SOURCES));
  return sharedRepositoryProjection;
}

// read one repository json document without changing its bytes
function readRepositoryJson(path) {
  return JSON.parse(readFileSync(new URL(`../${path}`, import.meta.url), "utf8"));
}

// parse one in-memory module with the pinned typescript parser
function parseModuleSource(source, fileName, scriptKind) {
  const sourceFile = ts.createSourceFile(
    fileName,
    source,
    ts.ScriptTarget.ES2023,
    true,
    scriptKind,
  );
  assert.equal(sourceFile.parseDiagnostics.length, 0, fileName);
  return sourceFile;
}

// read one static module specifier from an import or export declaration
function moduleSpecifierValue(declaration) {
  assert.ok(ts.isStringLiteral(declaration.moduleSpecifier));
  return declaration.moduleSpecifier.text;
}

// list every named import binding while preserving imported and local identities
function namedImportBindings(sourceFile, specifier) {
  const bindings = [];

  // inspect every declaration because one specifier may have type and value rows
  for (const statement of sourceFile.statements) {
    // skip declarations that cannot own named import bindings
    if (
      !ts.isImportDeclaration(statement) ||
      moduleSpecifierValue(statement) !== specifier
    ) {
      continue;
    }
    const clause = statement.importClause;
    const named = clause?.namedBindings;

    // leave default, namespace, side-effect, and empty named imports distinct
    if (named === undefined || !ts.isNamedImports(named)) {
      continue;
    }

    // retain aliases so erasure checks use the actual local binding
    for (const element of named.elements) {
      bindings.push({
        importedName: element.propertyName?.text ?? element.name.text,
        localName: element.name.text,
        typeOnly: clause.isTypeOnly || element.isTypeOnly,
      });
    }
  }
  return bindings;
}

// project one import declaration to the runtime signature preserved by verbatim emit
function runtimeImportSignature(declaration) {
  const specifier = moduleSpecifierValue(declaration);
  const clause = declaration.importClause;

  // preserve an ordinary bare module evaluation import
  if (clause === undefined) {
    return { specifier, kind: "side-effect" };
  }

  // erase a clause-level type-only declaration completely
  if (clause.isTypeOnly) {
    return null;
  }
  const namedBindings = clause.namedBindings;
  const named = namedBindings !== undefined && ts.isNamedImports(namedBindings)
    ? namedBindings.elements.filter(
      // erase only inline type specifiers while retaining an empty named clause
      (element) => !element.isTypeOnly,
    ).map(
      // preserve each imported name and its possible local alias
      (element) => ({
        importedName: element.propertyName?.text ?? element.name.text,
        localName: element.name.text,
      }),
    )
    : null;
  const namespaceBinding =
    namedBindings !== undefined && ts.isNamespaceImport(namedBindings)
    ? namedBindings.name.text
    : null;

  // fail closed if a future parser shape is neither named nor namespace syntax
  if (
    namedBindings !== undefined &&
    named === null &&
    namespaceBinding === null
  ) {
    assert.fail(`unsupported import binding syntax for ${specifier}`);
  }
  return {
    specifier,
    kind: "bindings",
    defaultBinding: clause.name?.text ?? null,
    namespaceBinding,
    namedBindings: named,
  };
}

// retain runtime import signatures only for one bounded configured specifier set
function runtimeImportSignatures(sourceFile, relevantSpecifiers) {
  const signatures = [];

  // preserve source declaration order across every audited specifier
  for (const statement of sourceFile.statements) {
    // skip non-import statements before reading a module specifier
    if (!ts.isImportDeclaration(statement)) {
      continue;
    }
    const specifier = moduleSpecifierValue(statement);

    // exclude imports outside the configured contract edge boundary
    if (!relevantSpecifiers.has(specifier)) {
      continue;
    }
    const signature = runtimeImportSignature(statement);

    // omit only whole clause-level type declarations from runtime expectations
    if (signature !== null) {
      signatures.push(signature);
    }
  }
  return signatures;
}

// transpile one virtual mts module entirely in memory
function transpileVirtualEsm(source, fileName, compilerOptions) {
  assert.match(fileName, /\.mts$/u);
  const result = ts.transpileModule(source, {
    compilerOptions,
    fileName: resolve(fileName),
    reportDiagnostics: true,
  });
  const diagnosticCodes = (result.diagnostics ?? []).map(
    // retain stable numeric codes without exposing source text
    (diagnostic) => diagnostic.code,
  );
  assert.equal(
    diagnosticCodes.length,
    0,
    `${fileName}: diagnostic codes ${diagnosticCodes.join(", ")}`,
  );
  return result.outputText;
}

// pin the appended runtime-evidence roots for focal and full manifest assertions
const RUNTIME_EVIDENCE_ROOTS = [
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentPairedScore",
    rustName: "ForecastAdjustmentPairedScore",
    kind: "readonly-interface",
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastSeasonDaypartKey",
    rustName: "ForecastSeasonDaypartKey",
    kind: "string-literal-enum",
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentCriticalSliceScore",
    rustName: "ForecastAdjustmentCriticalSliceScore",
    kind: "tagged-readonly-object-union",
    base: "ForecastAdjustmentPairedScore",
    discriminator: {
      field: "kind",
      variants: [
        {
          rustName: "NearestThree",
          payloadRustName: "ForecastAdjustmentCriticalSliceScoreNearestThree",
          wireValue: "nearest_three",
        },
        {
          rustName: "ProviderFamily",
          payloadRustName: "ForecastAdjustmentCriticalSliceScoreProviderFamily",
          wireValue: "provider_family",
        },
        {
          rustName: "SeasonDaypart",
          payloadRustName: "ForecastAdjustmentCriticalSliceScoreSeasonDaypart",
          wireValue: "season_daypart",
        },
        {
          rustName: "Station",
          payloadRustName: "ForecastAdjustmentCriticalSliceScoreStation",
          wireValue: "station",
        },
      ],
    },
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentMetricBandEvaluation",
    rustName: "ForecastAdjustmentMetricBandEvaluation",
    kind: "readonly-interface",
  },
  ...[
    ["algorithm-v1", "SanitizedTrainingMetrics"],
    ["evaluate", "DevelopmentLosoStationScoreV1"],
    ["evaluate", "DevelopmentLosoFoldResultV1"],
    ["evaluate", "ForecastAdjustmentDevelopmentReportV1"],
    ["evidence", "ForecastAdjustmentInsufficientDataReportV1"],
  ].map(([module, name]) => ({
    source: `packages/forecast-adjustment/src/${module}.ts`,
    exportName: name,
    rustName: name,
    kind: "readonly-interface",
  })),
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentEvaluationReportV2",
    rustName: "ForecastAdjustmentEvaluationReportV2",
    kind: "readonly-interface",
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentQualificationGateName",
    rustName: "ForecastAdjustmentQualificationGateName",
    kind: "string-literal-enum",
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentQualificationGate",
    rustName: "ForecastAdjustmentQualificationGate",
    kind: "readonly-interface",
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentEvidenceRedundancy",
    rustName: "ForecastAdjustmentEvidenceRedundancy",
    kind: "readonly-interface",
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentQualificationReceiptV2",
    rustName: "ForecastAdjustmentQualificationReceiptV2",
    kind: "readonly-interface",
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentEvidenceTripleV2",
    rustName: "ForecastAdjustmentEvidenceTripleV2",
    kind: "readonly-interface",
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentRuntimeBundleV2",
    rustName: "ForecastAdjustmentRuntimeBundleV2",
    kind: "readonly-interface",
    base: "ForecastAdjustmentEvidenceTripleV2",
  },
];

// pin the appended wind-canary runtime roots for focal and full manifest assertions
const WIND_CANARY_RUNTIME_ROOTS = [
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentWindCanaryBridgeScore",
    rustName: "ForecastAdjustmentWindCanaryBridgeScore",
    kind: "readonly-interface",
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentWindCanaryBridgeEvaluation",
    rustName: "ForecastAdjustmentWindCanaryBridgeEvaluation",
    kind: "readonly-interface",
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentWindCanaryTransferReportV1",
    rustName: "ForecastAdjustmentWindCanaryTransferReportV1",
    kind: "readonly-interface",
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentWindCanaryAuthorizationV1",
    rustName: "ForecastAdjustmentWindCanaryAuthorizationV1",
    kind: "readonly-interface",
  },
  {
    source: "packages/domain/src/forecast-adjustment.ts",
    exportName: "ForecastAdjustmentWindCanaryRuntimeBundleV1",
    rustName: "ForecastAdjustmentWindCanaryRuntimeBundleV1",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/wind-canary.ts",
    exportName: "ForecastAdjustmentWindCanaryAuthorizationV2",
    rustName: "ForecastAdjustmentWindCanaryAuthorizationV2",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/wind-canary.ts",
    exportName: "ForecastAdjustmentWindCanaryRuntimeBundleV2",
    rustName: "ForecastAdjustmentWindCanaryRuntimeBundleV2",
    kind: "readonly-interface",
    base: "ForecastAdjustmentWindCanaryRuntimeBundleV1",
    omitBaseFields: ["authorization", "contractVersion"],
  },
  {
    source: "packages/forecast-adjustment/src/wind-canary.ts",
    exportName: "ForecastAdjustmentWindCanaryRuntimeBundle",
    rustName: "ForecastAdjustmentWindCanaryRuntimeBundle",
    kind: "named-readonly-object-union",
    importedObjectUnion: true,
    objectUnion: {
      variants: [
        {
          rustName: "TimeLimited",
          payloadRustName: "ForecastAdjustmentWindCanaryRuntimeBundleV1",
        },
        {
          rustName: "Permanent",
          payloadRustName: "ForecastAdjustmentWindCanaryRuntimeBundleV2",
        },
      ],
    },
  },
  {
    source: "packages/forecast-adjustment/src/wind-canary.ts",
    exportName: "ForecastAdjustmentWindCanaryCandidateInputV1",
    rustName: "ForecastAdjustmentWindCanaryCandidateInputV1",
    kind: "readonly-interface",
  },
];

// pin the ECMWF repository roots without relying on global tail position
const ECMWF_TEMPERATURE_CANARY_ROOTS = [
  {
    source: "packages/database/src/repositories.ts",
    exportName: "EcmwfTemperatureCanaryRecentErrorState",
    rustName: "EcmwfTemperatureCanaryRecentErrorState",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "EcmwfTemperatureCanaryHour",
    rustName: "EcmwfTemperatureCanaryHour",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "EcmwfTemperatureCanaryRun",
    rustName: "EcmwfTemperatureCanaryRun",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "EcmwfTemperatureCanarySidecar",
    rustName: "EcmwfTemperatureCanarySidecar",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "EcmwfTemperatureCanaryStatus",
    rustName: "EcmwfTemperatureCanaryStatus",
    kind: "readonly-interface",
    indexedLiteralSources: {
      EcmwfTemperatureCanaryRun: "packages/database/src/repositories.ts",
    },
    fieldProjections: {
      stateStatus: {
        kind: "indexed-literals",
        root: "EcmwfTemperatureCanaryRun",
      },
    },
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "EcmwfTemperatureCanaryAuditRow",
    rustName: "EcmwfTemperatureCanaryAuditRow",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "EcmwfTemperatureCanaryPriorHour",
    rustName: "EcmwfTemperatureCanaryPriorHour",
    kind: "readonly-interface",
    base: "EcmwfTemperatureCanaryHour",
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "PersistEcmwfTemperatureCanaryRunInput",
    rustName: "PersistEcmwfTemperatureCanaryRunInput",
    kind: "readonly-interface",
    indexedLiteralSources: {
      EcmwfTemperatureCanaryRun: "packages/database/src/repositories.ts",
    },
    fieldProjections: {
      adapterVersion: {
        kind: "indexed-literals",
        root: "EcmwfTemperatureCanaryRun",
      },
      hours: {
        kind: "omit-interface-array",
        root: "EcmwfTemperatureCanaryHour",
      },
      modelCycle: {
        kind: "indexed-literals",
        root: "EcmwfTemperatureCanaryRun",
      },
      stateStatus: {
        kind: "indexed-literals",
        root: "EcmwfTemperatureCanaryRun",
      },
      upstreamModel: {
        kind: "indexed-literals",
        root: "EcmwfTemperatureCanaryRun",
      },
    },
  },
];

// pin the original temperature mos inference roots without relying on tail position
const TEMPERATURE_MOS_MODEL_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    exportName: "TemperatureMosRuntimeForecast",
    rustName: "TemperatureMosRuntimeForecast",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    exportName: "TemperatureMosRuntimeRecentErrorState",
    rustName: "TemperatureMosRuntimeRecentErrorState",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    exportName: "TemperatureMosRuntimeStrengthBand",
    rustName: "TemperatureMosRuntimeStrengthBand",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    exportName: "TemperatureMosRuntimeModelMaterial",
    rustName: "TemperatureMosRuntimeModelMaterial",
    kind: "readonly-interface",
    localInterface: true,
    wireFieldNames: {
      "strengthBands.1-6": "hours1To6",
      "strengthBands.7-12": "hours7To12",
    },
  },
  {
    source: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    exportName: "TemperatureMosRuntimeModel",
    rustName: "TemperatureMosRuntimeModel",
    kind: "readonly-interface",
    base: "TemperatureMosRuntimeModelMaterial",
  },
  {
    source: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    exportName: "TemperatureMosPermanentRuntimeModel",
    rustName: "TemperatureMosPermanentRuntimeModel",
    kind: "readonly-interface",
    base: "TemperatureMosRuntimeModelMaterial",
  },
  {
    source: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    exportName: "TemperatureMosServingModel",
    rustName: "TemperatureMosServingModel",
    kind: "named-readonly-object-union",
    objectUnion: {
      variants: [
        {
          rustName: "Monthly",
          payloadRustName: "TemperatureMosRuntimeModel",
        },
        {
          rustName: "Permanent",
          payloadRustName: "TemperatureMosPermanentRuntimeModel",
        },
      ],
    },
  },
  {
    source: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    exportName: "TemperatureMosRuntimeInput",
    rustName: "TemperatureMosRuntimeInput",
    kind: "readonly-interface",
  },
];

// pin the temperature-canary runtime roots without relying on tail position
const TEMPERATURE_CANARY_RUNTIME_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/temperature-canary.ts",
    exportName: "ForecastAdjustmentTemperatureCanaryAuthorizationV1",
    rustName: "ForecastAdjustmentTemperatureCanaryAuthorizationV1",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/temperature-canary.ts",
    exportName: "ForecastAdjustmentTemperatureCanaryAuthorizationV2",
    rustName: "ForecastAdjustmentTemperatureCanaryAuthorizationV2",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/temperature-canary.ts",
    exportName: "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1",
    rustName: "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/temperature-canary.ts",
    exportName: "ForecastAdjustmentTemperatureCanaryRuntimeBundleV2",
    rustName: "ForecastAdjustmentTemperatureCanaryRuntimeBundleV2",
    kind: "readonly-interface",
    base: "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1",
    omitBaseFields: ["authorization", "contractVersion", "model"],
  },
  {
    source: "packages/forecast-adjustment/src/temperature-canary.ts",
    exportName: "ForecastAdjustmentTemperatureCanaryRuntimeBundle",
    rustName: "ForecastAdjustmentTemperatureCanaryRuntimeBundle",
    kind: "named-readonly-object-union",
    objectUnion: {
      variants: [
        {
          rustName: "TimeLimited",
          payloadRustName: "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1",
        },
        {
          rustName: "Permanent",
          payloadRustName: "ForecastAdjustmentTemperatureCanaryRuntimeBundleV2",
        },
      ],
    },
  },
  {
    source: "packages/forecast-adjustment/src/temperature-canary.ts",
    exportName: "ForecastAdjustmentTemperatureMaintenanceMaterial",
    rustName: "ForecastAdjustmentTemperatureMaintenanceMaterial",
    kind: "readonly-interface",
    fieldProjections: {
      servedForecastIdentity: {
        kind: "interface-field",
        root: "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1",
      },
      trainingForecastIdentity: {
        kind: "interface-field",
        root: "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1",
      },
    },
  },
  {
    source: "packages/forecast-adjustment/src/temperature-canary.ts",
    exportName: "ForecastAdjustmentTemperatureCanaryRegistryV1",
    rustName: "ForecastAdjustmentTemperatureCanaryRegistryV1",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/temperature-canary.ts",
    exportName: "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1",
    rustName: "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1",
    kind: "tagged-readonly-object-union",
    discriminator: {
      field: "state",
      variants: [
        {
          rustName: "Active",
          payloadRustName: "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Active",
          wireValue: "active",
        },
        {
          rustName: "Disabled",
          payloadRustName: "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Disabled",
          wireValue: "disabled",
        },
      ],
    },
  },
];

// pin the temperature inference outputs and canary call input
const TEMPERATURE_INFERENCE_RESULT_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    exportName: "TemperatureMosRuntimeAppliedResult",
    rustName: "TemperatureMosRuntimeAppliedResult",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    exportName: "TemperatureMosRuntimeRawResult",
    rustName: "TemperatureMosRuntimeRawResult",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    exportName: "TemperatureMosRuntimeResult",
    rustName: "TemperatureMosRuntimeResult",
    kind: "named-readonly-object-union",
    objectUnion: {
      variants: [
        {
          rustName: "Applied",
          payloadRustName: "TemperatureMosRuntimeAppliedResult",
        },
        {
          rustName: "Raw",
          payloadRustName: "TemperatureMosRuntimeRawResult",
        },
      ],
    },
  },
  {
    source: "packages/forecast-adjustment/src/temperature-canary.ts",
    exportName: "ApplyTemperatureCanaryInputV1",
    rustName: "ApplyTemperatureCanaryInputV1",
    kind: "readonly-interface",
  },
];

// pin the public repository read-query inputs
const PUBLIC_READ_QUERY_ROOTS = [
  {
    source: "packages/database/src/repositories.ts",
    exportName: "HistoryQuery",
    rustName: "HistoryQuery",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "TrendQuery",
    rustName: "TrendQuery",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "DailyPrecipitationQuery",
    rustName: "DailyPrecipitationQuery",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "TideQuery",
    rustName: "TideQuery",
    kind: "readonly-interface",
  },
];

// pin the database rain capture and adjustment records
const RAIN_CAPTURE_ADJUSTMENT_ROOTS = [
  {
    source: "packages/database/src/rain-adjustment.ts",
    exportName: "RainAdjustmentCapture",
    rustName: "RainAdjustmentCapture",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/rain-adjustment.ts",
    exportName: "RainAdjustmentHour",
    rustName: "RainAdjustmentHour",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/rain-adjustment.ts",
    exportName: "RainAdjustmentRun",
    rustName: "RainAdjustmentRun",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/rain-adjustment.ts",
    exportName: "PersistedRainAdjustmentRevision",
    rustName: "PersistedRainAdjustmentRevision",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/rain-collection.ts",
    exportName: "RainFixedGaugeTargetCapture",
    rustName: "RainFixedGaugeTargetCapture",
    kind: "readonly-interface",
  },
];

// pin the appended receipt and fixed-gauge revision records
const RAIN_CAPTURE_RECEIPT_ROOTS = [
  {
    source: "packages/domain/src/rain-collection.ts",
    exportName: "RainCaptureReceipt",
    rustName: "RainCaptureReceipt",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/rain-collection.ts",
    exportName: "RainFixedGaugeTargetRevisionRow",
    rustName: "RainFixedGaugeTargetRevisionRow",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/rain-collection.ts",
    exportName: "RainFixedGaugeTargetRevisionGroup",
    rustName: "RainFixedGaugeTargetRevisionGroup",
    kind: "readonly-interface",
  },
];

// pin the terminal readonly-wrapped rain capture request union
const RAIN_CAPTURE_REQUEST_ROOTS = [
  {
    source: "packages/domain/src/rain-collection.ts",
    exportName: "RainCaptureRequest",
    rustName: "RainCaptureRequest",
    kind: "tagged-readonly-object-union",
    readonlyTaggedUnion: true,
    discriminator: {
      field: "kind",
      variants: [
        {
          rustName: "Forecast",
          payloadRustName: "RainCaptureRequestForecast",
          wireValue: "forecast",
        },
        {
          rustName: "Station",
          payloadRustName: "RainCaptureRequestStation",
          wireValue: "station",
        },
      ],
    },
  },
];

// pin the archived revision receipts and fixed-gauge result boundary
const FIXED_GAUGE_RESULT_ROOTS = [
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentRevisionCommitReceipt",
    rustName: "AdjustmentRevisionCommitReceipt",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentRevisionRowReceipt",
    rustName: "AdjustmentRevisionRowReceipt",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentRevisionGap",
    rustName: "AdjustmentRevisionGap",
    kind: "readonly-interface",
    fieldProjections: {
      projectionKind: {
        kind: "interface-field",
        root: "AdjustmentRevisionCommitReceipt",
      },
    },
  },
  {
    source: "packages/database/src/rain-collection.ts",
    exportName: "RainFixedGaugeTargetSource",
    rustName: "RainFixedGaugeTargetSource",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/rain-collection.ts",
    exportName: "RainFixedGaugeTargetSourceCatalog",
    rustName: "RainFixedGaugeTargetSourceCatalog",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/rain-collection.ts",
    exportName: "RainFixedGaugeTargetRevisionResult",
    rustName: "RainFixedGaugeTargetRevisionResult",
    kind: "tagged-readonly-object-union",
    readonlyTaggedUnion: true,
    discriminator: {
      field: "state",
      variants: [
        {
          rustName: "Committed",
          payloadRustName: "RainFixedGaugeTargetRevisionResultCommitted",
          wireValue: "committed",
        },
        {
          rustName: "Gap",
          payloadRustName: "RainFixedGaugeTargetRevisionResultGap",
          wireValue: "gap",
        },
      ],
    },
  },
];

// pin the original revision pointers and their exact admission keys
const REVISION_ADMISSION_ROOTS = [
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentWeatherRevisionPointer",
    rustName: "AdjustmentWeatherRevisionPointer",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentForecastAnchorRevisionPointer",
    rustName: "AdjustmentForecastAnchorRevisionPointer",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentRainGateRevisionPointer",
    rustName: "AdjustmentRainGateRevisionPointer",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentEcmwfTemperatureRevisionPointer",
    rustName: "AdjustmentEcmwfTemperatureRevisionPointer",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentWeatherRevisionAdmissionKey",
    rustName: "AdjustmentWeatherRevisionAdmissionKey",
    kind: "picked-readonly-interface",
    pick: {
      root: "AdjustmentWeatherRevisionPointer",
      fields: [
        "productRunAt",
        "sourceId",
        "sourceKind",
        "storedContentSha256",
        "validAt",
      ],
    },
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentForecastAnchorRevisionAdmissionKey",
    rustName: "AdjustmentForecastAnchorRevisionAdmissionKey",
    kind: "picked-readonly-interface",
    pick: {
      root: "AdjustmentForecastAnchorRevisionPointer",
      fields: ["leadHours", "sourceId", "storedContentSha256", "validAt"],
    },
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentRainGateRevisionAdmissionKey",
    rustName: "AdjustmentRainGateRevisionAdmissionKey",
    kind: "picked-readonly-interface",
    pick: {
      root: "AdjustmentRainGateRevisionPointer",
      fields: [
        "inputSha256",
        "modelSha256",
        "runInitializedAt",
        "storedContentSha256",
      ],
    },
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentEcmwfTemperatureRevisionAdmissionKey",
    rustName: "AdjustmentEcmwfTemperatureRevisionAdmissionKey",
    kind: "picked-readonly-interface",
    pick: {
      root: "AdjustmentEcmwfTemperatureRevisionPointer",
      fields: [
        "providerResponseSha256",
        "runInitializedAt",
        "siteId",
        "storedContentSha256",
      ],
    },
  },
];

// pin the shadow snapshot, receipt, availability, and export contracts
const ADJUSTMENT_SNAPSHOT_ROOTS = [
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentShadowFamily",
    rustName: "AdjustmentShadowFamily",
    kind: "string-literal-enum",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentShadowPrediction",
    rustName: "AdjustmentShadowPrediction",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentRevisionServingSnapshot",
    rustName: "AdjustmentRevisionServingSnapshot",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentShadowRegistrationReceipt",
    rustName: "AdjustmentShadowRegistrationReceipt",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentShadowRegistrationSlot",
    rustName: "AdjustmentShadowRegistrationSlot",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentShadowPredictionReceipt",
    rustName: "AdjustmentShadowPredictionReceipt",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentHotPrediction",
    rustName: "AdjustmentHotPrediction",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentConfirmationAvailability",
    rustName: "AdjustmentConfirmationAvailability",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/adjustment-maintenance.ts",
    exportName: "AdjustmentConfirmationChunkAuthorization",
    rustName: "AdjustmentConfirmationChunkAuthorization",
    kind: "readonly-interface",
  },
];

// pin the original archive relay enums, receipts, gaps, and status inputs
const ARCHIVE_RELAY_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    exportName: "AdjustmentRevisionProjectionKind",
    rustName: "AdjustmentRevisionProjectionKind",
    kind: "string-literal-enum",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "ApiMaintenanceShadowFamily",
    rustName: "ApiMaintenanceShadowFamily",
    kind: "string-literal-enum",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "MaintenanceShadowGapReason",
    rustName: "MaintenanceShadowGapReason",
    kind: "string-literal-enum",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "AdjustmentRevisionGapReason",
    rustName: "AdjustmentRevisionGapReason",
    kind: "string-literal-enum",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "AdjustmentRevisionStageReceipt",
    rustName: "AdjustmentRevisionStageReceipt",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "RainMaintenanceControlStateStageReceipt",
    rustName: "RainMaintenanceControlStateStageReceipt",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "RainMaintenanceControlStateGapInput",
    rustName: "RainMaintenanceControlStateGapInput",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "RainFixedGaugeTargetGapInput",
    rustName: "RainFixedGaugeTargetGapInput",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "RainFixedGaugeTargetGapReceipt",
    rustName: "RainFixedGaugeTargetGapReceipt",
    kind: "readonly-interface",
    base: "RainFixedGaugeTargetGapInput",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "RainFixedGaugeTargetGapStatusInput",
    rustName: "RainFixedGaugeTargetGapStatusInput",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "AdjustmentRevisionPublishReceipt",
    rustName: "AdjustmentRevisionPublishReceipt",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "AdjustmentRevisionBatchPublishReceipt",
    rustName: "AdjustmentRevisionBatchPublishReceipt",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "AdjustmentRevisionGapInput",
    rustName: "AdjustmentRevisionGapInput",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "AdjustmentMaintenanceStageReceipt",
    rustName: "AdjustmentMaintenanceStageReceipt",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "AdjustmentMaintenancePublishReceipt",
    rustName: "AdjustmentMaintenancePublishReceipt",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "AdjustmentMaintenanceGapInput",
    rustName: "AdjustmentMaintenanceGapInput",
    kind: "readonly-interface",
  },
];

// pin the archive relay input, metadata, capture, and nested gap-status roots
const ARCHIVE_RELAY_INPUT_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/maintenance-shadow-values.ts",
    exportName: "MaintenanceShadowSourceIdentity",
    rustName: "MaintenanceShadowSourceIdentity",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/maintenance-shadow-values.ts",
    exportName: "MaintenanceShadowPredictionMetadata",
    rustName: "MaintenanceShadowPredictionMetadata",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "AdjustmentMaintenanceStageInput",
    rustName: "AdjustmentMaintenanceStageInput",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "AdjustmentMaintenancePublishInput",
    rustName: "AdjustmentMaintenancePublishInput",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "AdjustmentMaintenanceCaptureRequest",
    rustName: "AdjustmentMaintenanceCaptureRequest",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "RainFixedGaugeTargetGapStatus",
    rustName: "RainFixedGaugeTargetGapStatus",
    kind: "tagged-readonly-object-union",
    readonlyTaggedUnion: true,
    discriminator: {
      field: "state",
      variants: [
        {
          rustName: "Absent",
          payloadRustName: "RainFixedGaugeTargetGapStatusAbsent",
          wireValue: "absent",
        },
        {
          rustName: "Present",
          payloadRustName: "RainFixedGaugeTargetGapStatusPresent",
          wireValue: "present",
        },
      ],
    },
  },
];

// pin the revision projection family, state, and complete projection bodies
const REVISION_PROJECTION_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    exportName: "AdjustmentRevisionProjectionFamily",
    rustName: "AdjustmentRevisionProjectionFamily",
    kind: "string-literal-enum",
  },
  {
    source: "packages/forecast-adjustment/src/maintenance-shadow-values.ts",
    exportName: "MaintenanceShadowTemperatureRecentErrorState",
    rustName: "MaintenanceShadowTemperatureRecentErrorState",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    exportName: "AdjustmentRevisionProjection",
    rustName: "AdjustmentRevisionProjection",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    exportName: "AdjustmentRevisionBatchProjection",
    rustName: "AdjustmentRevisionBatchProjection",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    exportName: "AdjustmentTemperatureNativeSourceProjection",
    rustName: "AdjustmentTemperatureNativeSourceProjection",
    kind: "readonly-interface",
    fieldProjections: {
      logicalKey: {
        kind: "interface-field",
        root: "AdjustmentRevisionProjection",
      },
      rows: {
        kind: "interface-field",
        root: "AdjustmentRevisionProjection",
      },
      source: {
        kind: "interface-field",
        root: "AdjustmentRevisionProjection",
      },
    },
  },
  {
    source: "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    exportName: "AdjustmentRainGateFeatureProjection",
    rustName: "AdjustmentRainGateFeatureProjection",
    kind: "readonly-interface",
  },
];

// pin the persistence target, complete rain control, and closed document union
const REVISION_DOCUMENT_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    exportName: "RainMaintenancePersistenceCaptureMember",
    rustName: "RainMaintenancePersistenceCaptureMember",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    exportName: "RainMaintenancePersistenceTargetRow",
    rustName: "RainMaintenancePersistenceTargetRow",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    exportName: "RainMaintenancePersistenceTarget",
    rustName: "RainMaintenancePersistenceTarget",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    exportName: "AdjustmentRainGateControlProjection",
    rustName: "AdjustmentRainGateControlProjection",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    exportName: "AdjustmentRevisionProjectionDocument",
    rustName: "AdjustmentRevisionProjectionDocument",
    kind: "named-readonly-object-union",
    objectUnion: {
      variants: [
        {
          rustName: "Single",
          payloadRustName: "AdjustmentRevisionProjection",
        },
        {
          rustName: "Batch",
          payloadRustName: "AdjustmentRevisionBatchProjection",
        },
        {
          rustName: "NativeTemperature",
          payloadRustName: "AdjustmentTemperatureNativeSourceProjection",
        },
        {
          rustName: "RainFeature",
          payloadRustName: "AdjustmentRainGateFeatureProjection",
        },
        {
          rustName: "RainControl",
          payloadRustName: "AdjustmentRainGateControlProjection",
        },
      ],
    },
  },
];

// pin the complete fixed-gauge target document and derived actual value
const FIXED_GAUGE_TARGET_DOCUMENT_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts",
    exportName: "AdjustmentRainFixedGaugeTargetClaim",
    rustName: "AdjustmentRainFixedGaugeTargetClaim",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts",
    exportName: "AdjustmentRainFixedGaugeTargetCaptureBody",
    rustName: "AdjustmentRainFixedGaugeTargetCaptureBody",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts",
    exportName: "AdjustmentRainFixedGaugeTargetCaptureMember",
    rustName: "AdjustmentRainFixedGaugeTargetCaptureMember",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts",
    exportName: "AdjustmentRainFixedGaugeTargetInterval",
    rustName: "AdjustmentRainFixedGaugeTargetInterval",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts",
    exportName: "AdjustmentRainFixedGaugeTargetRow",
    rustName: "AdjustmentRainFixedGaugeTargetRow",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts",
    exportName: "AdjustmentRainFixedGaugeTargetProjection",
    rustName: "AdjustmentRainFixedGaugeTargetProjection",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts",
    exportName: "AdjustmentRainFixedGaugeTargetLogicalKey",
    rustName: "AdjustmentRainFixedGaugeTargetLogicalKey",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts",
    exportName: "AdjustmentRainFixedGaugeTargetActual",
    rustName: "AdjustmentRainFixedGaugeTargetActual",
    kind: "readonly-interface",
  },
];

// pin the maintenance evaluation rows and closed capture outcome
const MAINTENANCE_EVALUATION_ROOTS = [
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "TemperatureMaintenanceCandidateRow",
    rustName: "TemperatureMaintenanceCandidateRow",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "WindMaintenanceCandidateRow",
    rustName: "WindMaintenanceCandidateRow",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "TemperatureMaintenanceIncumbentRow",
    rustName: "TemperatureMaintenanceIncumbentRow",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "WindMaintenanceIncumbentRow",
    rustName: "WindMaintenanceIncumbentRow",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/adjustment-maintenance-shadow.ts",
    exportName: "CaptureApiMaintenanceShadowResult",
    rustName: "CaptureApiMaintenanceShadowResult",
    kind: "tagged-readonly-object-union",
    readonlyTaggedUnion: true,
    discriminator: {
      field: "status",
      variants: [
        {
          rustName: "Published",
          payloadRustName: "CaptureApiMaintenanceShadowResultPublished",
          wireValue: "published",
        },
        {
          rustName: "Gap",
          payloadRustName: "CaptureApiMaintenanceShadowResultGap",
          wireValue: "gap",
        },
      ],
    },
  },
];

// pin the api, worker, and migration readiness snapshots
const READINESS_SNAPSHOT_ROOTS = [
  {
    source: "apps/api/src/index.ts",
    exportName: "HealthSnapshot",
    rustName: "HealthSnapshot",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/health.ts",
    exportName: "WorkerHealth",
    rustName: "WorkerHealth",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/migrate.ts",
    exportName: "MigrationReadiness",
    rustName: "MigrationReadiness",
    kind: "readonly-interface",
  },
];

// pin worker iteration, collection, and diagnostic result declarations
const WORKER_RESULT_ROOTS = [
  {
    source: "apps/worker/src/worker.ts",
    exportName: "SourceRunResult",
    rustName: "SourceRunResult",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/worker.ts",
    exportName: "WorkerIterationResult",
    rustName: "WorkerIterationResult",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/worker.ts",
    exportName: "EcmwfTemperatureCanaryCollectionResult",
    rustName: "EcmwfTemperatureCanaryCollectionResult",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/rain-collection.ts",
    exportName: "RainCollectionResult",
    rustName: "RainCollectionResult",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/errors.ts",
    exportName: "WorkerDiagnosticEvent",
    rustName: "WorkerDiagnosticEvent",
    kind: "string-literal-enum",
  },
  {
    source: "apps/worker/src/errors.ts",
    exportName: "WorkerDiagnostic",
    rustName: "WorkerDiagnostic",
    kind: "readonly-interface",
  },
];

// pin Open Meteo location, archive, prior-run, and single-run request contracts
const OPEN_METEO_REQUEST_ROOTS = [
  {
    source: "packages/providers/src/open-meteo.ts",
    exportName: "OpenMeteoLocation",
    rustName: "OpenMeteoLocation",
    kind: "readonly-interface",
  },
  {
    source: "packages/providers/src/open-meteo.ts",
    exportName: "OpenMeteoArchiveRequest",
    rustName: "OpenMeteoArchiveRequest",
    kind: "readonly-interface",
    base: "OpenMeteoLocation",
  },
  {
    source: "packages/providers/src/open-meteo-previous-runs.ts",
    exportName: "OpenMeteoPreviousRunsLocation",
    rustName: "OpenMeteoPreviousRunsLocation",
    kind: "readonly-interface",
  },
  {
    source: "packages/providers/src/open-meteo-previous-runs.ts",
    exportName: "OpenMeteoPreviousRunsRequest",
    rustName: "OpenMeteoPreviousRunsRequest",
    kind: "readonly-interface",
  },
  {
    source: "packages/providers/src/open-meteo-single-runs.ts",
    exportName: "OpenMeteoEcmwfSingleRunRequest",
    rustName: "OpenMeteoEcmwfSingleRunRequest",
    kind: "readonly-interface",
  },
  {
    source: "packages/providers/src/open-meteo-single-runs.ts",
    exportName: "OpenMeteoEcmwfSingleRunHour",
    rustName: "OpenMeteoEcmwfSingleRunHour",
    kind: "readonly-interface",
  },
  {
    source: "packages/providers/src/open-meteo-single-runs.ts",
    exportName: "OpenMeteoEcmwfSingleRunBatch",
    rustName: "OpenMeteoEcmwfSingleRunBatch",
    kind: "readonly-interface",
  },
];

// pin provider metadata, Ecowitt cursor snapshots, and resolved Tempest stations
const STATION_ADAPTER_ROOTS = [
  {
    source: "packages/providers/src/contract.ts",
    exportName: "ProviderCapability",
    rustName: "ProviderCapability",
    kind: "string-literal-enum",
  },
  {
    source: "packages/providers/src/contract.ts",
    exportName: "ProviderAttribution",
    rustName: "ProviderAttribution",
    kind: "readonly-interface",
  },
  {
    source: "packages/providers/src/ecowitt.ts",
    exportName: "EcowittLiveRequest",
    rustName: "EcowittLiveRequest",
    kind: "readonly-interface",
  },
  {
    source: "packages/providers/src/ecowitt.ts",
    exportName: "EcowittSensorReadings",
    rustName: "EcowittSensorReadings",
    kind: "readonly-interface",
  },
  {
    source: "packages/providers/src/ecowitt.ts",
    exportName: "EcowittSensorSnapshot",
    rustName: "EcowittSensorSnapshot",
    kind: "readonly-interface",
  },
  {
    source: "packages/providers/src/tempest.ts",
    exportName: "ResolvedTempestStation",
    rustName: "ResolvedTempestStation",
    kind: "readonly-interface",
  },
];

// pin the private NOAA range base and both public request arms
const NOAA_TIDE_REQUEST_ROOTS = [
  {
    source: "packages/providers/src/noaa-tides.ts",
    exportName: "NoaaTideRangeBase",
    rustName: "NoaaTideRangeBase",
    kind: "readonly-interface",
    localInterface: true,
  },
  {
    source: "packages/providers/src/noaa-tides.ts",
    exportName: "NoaaTideObservationRequest",
    rustName: "NoaaTideObservationRequest",
    kind: "readonly-interface",
    base: "NoaaTideRangeBase",
  },
  {
    source: "packages/providers/src/noaa-tides.ts",
    exportName: "NoaaTidePredictionRequest",
    rustName: "NoaaTidePredictionRequest",
    kind: "readonly-interface",
    base: "NoaaTideRangeBase",
  },
  {
    source: "packages/providers/src/noaa-tides.ts",
    exportName: "NoaaTideRangeRequest",
    rustName: "NoaaTideRangeRequest",
    kind: "named-readonly-object-union",
    objectUnion: {
      variants: [
        {
          rustName: "Observation",
          payloadRustName: "NoaaTideObservationRequest",
        },
        {
          rustName: "Prediction",
          payloadRustName: "NoaaTidePredictionRequest",
        },
      ],
    },
  },
];

// pin the worker tide backfill arguments, chunks, and report rows
const TIDE_BACKFILL_ROOTS = [
  {
    source: "apps/worker/src/tide-backfill-cli.ts",
    exportName: "TideBackfillArguments",
    rustName: "TideBackfillArguments",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/tide-backfill-cli.ts",
    exportName: "PlannedTideChunk",
    rustName: "PlannedTideChunk",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/tide-backfill-cli.ts",
    exportName: "TideBackfillReport",
    rustName: "TideBackfillReport",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/tide-backfill-cli.ts",
    exportName: "TideBackfillSourceReport",
    rustName: "TideBackfillSourceReport",
    kind: "readonly-interface",
  },
];

// pin general, Tempest, and public-station backfill data contracts
const BACKFILL_DATA_ROOTS = [
  {
    source: "apps/worker/src/backfill-cli.ts",
    exportName: "BackfillArguments",
    rustName: "BackfillArguments",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/backfill-cli.ts",
    exportName: "PlannedBackfillChunk",
    rustName: "PlannedBackfillChunk",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/backfill-cli.ts",
    exportName: "BackfillChunkReport",
    rustName: "BackfillChunkReport",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/backfill-cli.ts",
    exportName: "BackfillReport",
    rustName: "BackfillReport",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/tempest-backfill-cli.ts",
    exportName: "TempestBackfillArguments",
    rustName: "TempestBackfillArguments",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/tempest-backfill-cli.ts",
    exportName: "PlannedTempestChunk",
    rustName: "PlannedTempestChunk",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/tempest-backfill-cli.ts",
    exportName: "TempestBackfillChunkReport",
    rustName: "TempestBackfillChunkReport",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/tempest-backfill-cli.ts",
    exportName: "TempestStationBackfillReport",
    rustName: "TempestStationBackfillReport",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/tempest-backfill-cli.ts",
    exportName: "TempestBulkBackfillReport",
    rustName: "TempestBulkBackfillReport",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/public-stations-backfill-cli.ts",
    exportName: "PublicStationBackfillArguments",
    rustName: "PublicStationBackfillArguments",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/public-stations-backfill-cli.ts",
    exportName: "PlannedPublicStationChunk",
    rustName: "PlannedPublicStationChunk",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/public-stations-backfill-cli.ts",
    exportName: "PublicStationBackfillSourceReport",
    rustName: "PublicStationBackfillSourceReport",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/public-stations-backfill-cli.ts",
    exportName: "PublicStationBackfillReport",
    rustName: "PublicStationBackfillReport",
    kind: "readonly-interface",
  },
];

// pin the original version-one widget forecast response contracts
const WIDGET_FORECAST_ROOTS = [
  {
    source: "apps/web/src/widget-forecast.ts",
    exportName: "WidgetForecastMode",
    rustName: "WidgetForecastMode",
    kind: "string-literal-enum",
  },
  {
    source: "apps/web/src/widget-forecast.ts",
    exportName: "WidgetForecastReason",
    rustName: "WidgetForecastReason",
    kind: "string-literal-enum",
  },
  {
    source: "apps/web/src/widget-forecast.ts",
    exportName: "WidgetForecastStatus",
    rustName: "WidgetForecastStatus",
    kind: "string-literal-enum",
    catalog: {
      kind: "extend-local-string-enum",
      base: "WidgetForecastMode",
    },
  },
  {
    source: "apps/web/src/widget-forecast.ts",
    exportName: "WidgetForecastSource",
    rustName: "WidgetForecastSource",
    kind: "readonly-interface",
  },
  {
    source: "apps/web/src/widget-forecast.ts",
    exportName: "WidgetForecastField",
    rustName: "WidgetForecastField",
    kind: "readonly-interface",
  },
  {
    source: "apps/web/src/widget-forecast.ts",
    exportName: "WidgetForecastHour",
    rustName: "WidgetForecastHour",
    kind: "readonly-interface",
  },
  {
    source: "apps/web/src/widget-forecast.ts",
    exportName: "WidgetForecastSnapshot",
    rustName: "WidgetForecastSnapshot",
    kind: "readonly-interface",
  },
];

// pin the imported widget v2 and v3 extension contracts
const WIDGET_FORECAST_EXTENSION_ROOTS = [
  {
    source: "apps/web/src/widget-forecast-v2.ts",
    exportName: "WidgetForecastV2Hour",
    rustName: "WidgetForecastV2Hour",
    kind: "readonly-interface",
    base: "WidgetForecastHour",
    importedBase: true,
  },
  {
    source: "apps/web/src/widget-forecast-v2.ts",
    exportName: "WidgetForecastV2Snapshot",
    rustName: "WidgetForecastV2Snapshot",
    kind: "readonly-interface",
    base: "WidgetForecastSnapshot",
    omitBaseFields: ["hours", "schemaVersion"],
  },
  {
    source: "apps/web/src/widget-forecast-v3.ts",
    exportName: "WidgetForecastV3Snapshot",
    rustName: "WidgetForecastV3Snapshot",
    kind: "readonly-interface",
    base: "WidgetForecastV2Snapshot",
    omitBaseFields: ["calendar", "schemaVersion"],
    omitInheritedBaseFields: ["calendar"],
    fieldProjections: {
      calendar: {
        kind: "indexed-interface-field-extension",
        root: "WidgetForecastSnapshot",
      },
    },
  },
];

// pin the original generic provider batch and its default record root
const PROVIDER_BATCH_ROOTS = [
  {
    source: "packages/providers/src/contract.ts",
    exportName: "ProviderBatch",
    rustName: "ProviderBatch",
    kind: "readonly-interface",
    defaultedListParameter: {
      name: "RecordType",
      defaultRoot: "NormalizedWeatherRecord",
      field: "records",
    },
  },
];

// pin the migration result from the database producer
const MIGRATION_RESULT_ROOT = {
  source: "packages/database/src/migrate.ts",
  exportName: "MigrationResult",
  rustName: "MigrationResult",
  kind: "readonly-interface",
};

// pin the named database projection and installed marker evidence
const WEATHER_RECORD_ROW_ROOT = {
  source: "packages/database/src/repositories.ts",
  exportName: "WeatherRecordRow",
  rustName: "WeatherRecordRow",
  kind: "readonly-interface",
  pgRowNamedFields: true,
};

// pin the current row extension and its complete flattened weather base
const CURRENT_WEATHER_RECORD_ROW_ROOT = {
  source: "packages/database/src/repositories.ts",
  exportName: "CurrentWeatherRecordRow",
  rustName: "CurrentWeatherRecordRow",
  kind: "readonly-interface",
  base: "WeatherRecordRow",
};

// pin the Date-only trend root without adding a wider timestamp alternative
const TREND_POINT_ROW_ROOT = {
  source: "packages/database/src/repositories.ts",
  exportName: "TrendPointRow",
  rustName: "TrendPointRow",
  kind: "readonly-interface",
  pgRowNamedFields: true,
};

// pin the hourly station metrics and their original repository row
const FORECAST_OBSERVATION_HOURLY_ROOTS = [
  {
    source: "packages/database/src/repositories.ts",
    exportName: "ForecastObservationHourlyMetrics",
    rustName: "ForecastObservationHourlyMetrics",
    kind: "readonly-nullable-metric-record",
    readonlyNullableMetricRecord: "ForecastAdjustmentMetric",
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "ForecastObservationHourlyStationRow",
    rustName: "ForecastObservationHourlyStationRow",
    kind: "readonly-interface",
  },
];

// pin the persisted ECMWF revision archive projection
const PERSISTED_ECMWF_TEMPERATURE_REVISION_ROOT = {
  source: "packages/database/src/repositories.ts",
  exportName: "PersistedEcmwfTemperatureRevision",
  rustName: "PersistedEcmwfTemperatureRevision",
  kind: "readonly-interface",
  fieldProjections: {
    adapterVersion: {
      kind: "interface-field",
      root: "EcmwfTemperatureCanaryRun",
    },
    modelCycle: {
      kind: "interface-field",
      root: "EcmwfTemperatureCanaryRun",
    },
    upstreamModel: {
      kind: "interface-field",
      root: "EcmwfTemperatureCanaryRun",
    },
  },
};

// pin the site and provider bootstrap catalog projections in file order
const BOOTSTRAP_CATALOG_ROOTS = [
  {
    source: "packages/database/src/config.ts",
    exportName: "SiteConfigurationSource",
    rustName: "SiteConfigurationSource",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/config.ts",
    exportName: "SiteConfiguration",
    rustName: "SiteConfiguration",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/ecowitt-config.ts",
    exportName: "EcowittStationConfiguration",
    rustName: "EcowittStationConfiguration",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/ecowitt-config.ts",
    exportName: "EcowittConfiguration",
    rustName: "EcowittConfiguration",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/tempest-config.ts",
    exportName: "TempestStationConfiguration",
    rustName: "TempestStationConfiguration",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/tempest-config.ts",
    exportName: "TempestConfiguration",
    rustName: "TempestConfiguration",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/public-stations-config.ts",
    exportName: "PublicStationAdapter",
    rustName: "PublicStationAdapter",
    kind: "string-literal-enum",
  },
  {
    source: "packages/database/src/public-stations-config.ts",
    exportName: "PublicStationProviderConfiguration",
    rustName: "PublicStationProviderConfiguration",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/public-stations-config.ts",
    exportName: "PublicStationSourceConfiguration",
    rustName: "PublicStationSourceConfiguration",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/public-stations-config.ts",
    exportName: "PublicStationConfigurationStation",
    rustName: "PublicStationConfigurationStation",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/public-stations-config.ts",
    exportName: "PublicStationConfiguration",
    rustName: "PublicStationConfiguration",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/tides-config.ts",
    exportName: "TideProviderConfiguration",
    rustName: "TideProviderConfiguration",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/tides-config.ts",
    exportName: "TideSourceConfiguration",
    rustName: "TideSourceConfiguration",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/tides-config.ts",
    exportName: "TideStationConfiguration",
    rustName: "TideStationConfiguration",
    kind: "readonly-interface",
  },
  {
    source: "packages/database/src/tides-config.ts",
    exportName: "TideConfiguration",
    rustName: "TideConfiguration",
    kind: "readonly-interface",
  },
];

// pin the five closed domain edges consumed by bootstrap catalogs
const BOOTSTRAP_CATALOG_DOMAIN_IMPORTS = [
  {
    consumer: "packages/database/src/config.ts",
    specifier: "@weather/domain",
    packageJson: "packages/domain/package.json",
    entry: "packages/domain/src/index.ts",
    leaves: [
      "packages/domain/src/forecast-adjustment.ts",
      "packages/domain/src/forecast-anchor-record.ts",
      "packages/domain/src/ingestion.ts",
      "packages/domain/src/provenance.ts",
      "packages/domain/src/rain-collection.ts",
      "packages/domain/src/weather-record.ts",
    ],
    exports: {
      JsonValue: "packages/domain/src/provenance.ts",
      SourceCapability: "packages/domain/src/provenance.ts",
      SourceKind: "packages/domain/src/provenance.ts",
      StationKind: "packages/domain/src/provenance.ts",
    },
  },
  {
    consumer: "packages/database/src/ecowitt-config.ts",
    specifier: "@weather/domain",
    packageJson: "packages/domain/package.json",
    entry: "packages/domain/src/index.ts",
    leaves: [
      "packages/domain/src/forecast-adjustment.ts",
      "packages/domain/src/forecast-anchor-record.ts",
      "packages/domain/src/ingestion.ts",
      "packages/domain/src/provenance.ts",
      "packages/domain/src/rain-collection.ts",
      "packages/domain/src/weather-record.ts",
    ],
    exports: {
      JsonValue: "packages/domain/src/provenance.ts",
    },
  },
  {
    consumer: "packages/database/src/tempest-config.ts",
    specifier: "@weather/domain",
    packageJson: "packages/domain/package.json",
    entry: "packages/domain/src/index.ts",
    leaves: [
      "packages/domain/src/forecast-adjustment.ts",
      "packages/domain/src/forecast-anchor-record.ts",
      "packages/domain/src/ingestion.ts",
      "packages/domain/src/provenance.ts",
      "packages/domain/src/rain-collection.ts",
      "packages/domain/src/weather-record.ts",
    ],
    exports: {
      JsonValue: "packages/domain/src/provenance.ts",
    },
  },
  {
    consumer: "packages/database/src/public-stations-config.ts",
    specifier: "@weather/domain",
    packageJson: "packages/domain/package.json",
    entry: "packages/domain/src/index.ts",
    leaves: [
      "packages/domain/src/forecast-adjustment.ts",
      "packages/domain/src/forecast-anchor-record.ts",
      "packages/domain/src/ingestion.ts",
      "packages/domain/src/provenance.ts",
      "packages/domain/src/rain-collection.ts",
      "packages/domain/src/weather-record.ts",
    ],
    exports: {
      JsonValue: "packages/domain/src/provenance.ts",
      SourceCapability: "packages/domain/src/provenance.ts",
    },
  },
  {
    consumer: "packages/database/src/tides-config.ts",
    specifier: "@weather/domain",
    packageJson: "packages/domain/package.json",
    entry: "packages/domain/src/index.ts",
    leaves: [
      "packages/domain/src/forecast-adjustment.ts",
      "packages/domain/src/forecast-anchor-record.ts",
      "packages/domain/src/ingestion.ts",
      "packages/domain/src/provenance.ts",
      "packages/domain/src/rain-collection.ts",
      "packages/domain/src/weather-record.ts",
    ],
    exports: {
      JsonValue: "packages/domain/src/provenance.ts",
      SourceCapability: "packages/domain/src/provenance.ts",
      StationKind: "packages/domain/src/provenance.ts",
    },
  },
];

// pin the serving adjustment and calendar projections in producer order
const SERVING_FORECAST_ADJUSTMENT_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/calendar.ts",
    exportName: "LocalMeteorologicalSeason",
    rustName: "LocalMeteorologicalSeason",
    kind: "string-literal-enum",
  },
  {
    source: "packages/forecast-adjustment/src/calendar.ts",
    exportName: "LocalDaypart",
    rustName: "LocalDaypart",
    kind: "string-literal-enum",
  },
  {
    source: "packages/forecast-adjustment/src/calendar.ts",
    exportName: "RuntimeCalendarFingerprint",
    rustName: "RuntimeCalendarFingerprint",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/calendar.ts",
    exportName: "LocalCalendarFeatures",
    rustName: "LocalCalendarFeatures",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/algorithm-v1.ts",
    exportName: "CoreAdjustmentApplicationInput",
    rustName: "CoreAdjustmentApplicationInput",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/algorithm-v1.ts",
    exportName: "CoreAdjustmentApplicationResult",
    rustName: "CoreAdjustmentApplicationResult",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/apply.ts",
    exportName: "ApplyForecastAdjustmentInputV1",
    rustName: "ApplyForecastAdjustmentInputV1",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/apply.ts",
    exportName: "ForecastAdjustmentWindMaintenanceDecision",
    rustName: "ForecastAdjustmentWindMaintenanceDecision",
    kind: "readonly-interface",
    fieldProjections: {
      appliedMetrics: {
        kind: "const-catalog-array",
        root: "ForecastAdjustmentWindCanaryMetric",
      },
      leadBand: {
        kind: "return-enum",
        root: "ForecastLeadBandKey",
      },
    },
  },
];

// pin the extended domain edge consumed by the serving application input
const APPLY_FORECAST_ADJUSTMENT_DOMAIN_IMPORT = {
  consumer: "packages/forecast-adjustment/src/apply.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [
    "packages/domain/src/forecast-adjustment.ts",
    "packages/domain/src/forecast-anchor-record.ts",
    "packages/domain/src/ingestion.ts",
    "packages/domain/src/provenance.ts",
    "packages/domain/src/rain-collection.ts",
    "packages/domain/src/weather-record.ts",
  ],
  exports: {
    CanonicalWeatherMetrics: "packages/domain/src/weather-record.ts",
    ForecastAdjustmentMetric:
      "packages/domain/src/forecast-adjustment.ts",
    ForecastAdjustmentRawForecastProvenance:
      "packages/domain/src/forecast-adjustment.ts",
  },
};

// pin the two new nominal edges introduced by serving projections
const SERVING_FORECAST_ADJUSTMENT_IMPORTS = [
  {
    consumer: "packages/forecast-adjustment/src/algorithm-v1.ts",
    specifier: "@weather/domain",
    packageJson: "packages/domain/package.json",
    entry: "packages/domain/src/index.ts",
    leaves: [
      "packages/domain/src/forecast-adjustment.ts",
      "packages/domain/src/forecast-anchor-record.ts",
      "packages/domain/src/ingestion.ts",
      "packages/domain/src/provenance.ts",
      "packages/domain/src/rain-collection.ts",
      "packages/domain/src/weather-record.ts",
    ],
    exports: {
      ForecastAdjustmentMetric:
        "packages/domain/src/forecast-adjustment.ts",
    },
  },
  {
    consumer: "packages/forecast-adjustment/src/apply.ts",
    specifier: "./calendar.js",
    entry: "packages/forecast-adjustment/src/calendar.ts",
    exports: {
      RuntimeCalendarFingerprint:
        "packages/forecast-adjustment/src/calendar.ts",
    },
  },
];

// pin the newly projected loader authority, states, and raw registry
const FORECAST_ADJUSTMENT_LOADER_ROOTS = [
  {
    source:
      "packages/forecast-adjustment/src/maintenance-shadow-comparator.ts",
    exportName: "MaintenanceShadowComparatorAuthorityKind",
    rustName: "MaintenanceShadowComparatorAuthorityKind",
    kind: "string-literal-enum",
  },
  {
    source:
      "packages/forecast-adjustment/src/maintenance-shadow-comparator.ts",
    exportName: "MaintenanceShadowServingAuthority",
    rustName: "MaintenanceShadowServingAuthority",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/runtime-loader.ts",
    exportName: "LoadedForecastAdjustmentRuntimeV1",
    rustName: "LoadedForecastAdjustmentRuntimeV1",
    kind: "tagged-readonly-object-union",
    disabledReasonExtract: "ForecastAdjustmentReasonCode",
    discriminator: {
      field: "state",
      variants: [
        {
          rustName: "Active",
          payloadRustName: "LoadedForecastAdjustmentRuntimeV1Active",
          wireValue: "active",
        },
        {
          rustName: "Disabled",
          payloadRustName: "LoadedForecastAdjustmentRuntimeV1Disabled",
          wireValue: "disabled",
        },
      ],
    },
  },
  {
    source: "packages/forecast-adjustment/src/runtime-loader.ts",
    exportName: "LoadedForecastAdjustmentWindCanaryRuntimeV1",
    rustName: "LoadedForecastAdjustmentWindCanaryRuntimeV1",
    kind: "tagged-readonly-object-union",
    disabledReasonExtract: "ForecastAdjustmentReasonCode",
    discriminator: {
      field: "state",
      variants: [
        {
          rustName: "Active",
          payloadRustName:
            "LoadedForecastAdjustmentWindCanaryRuntimeV1Active",
          wireValue: "active",
        },
        {
          rustName: "Disabled",
          payloadRustName:
            "LoadedForecastAdjustmentWindCanaryRuntimeV1Disabled",
          wireValue: "disabled",
        },
      ],
    },
  },
  {
    source: "packages/forecast-adjustment/src/runtime-loader.ts",
    exportName: "ForecastAdjustmentTemperatureCanaryRegistryV2",
    rustName: "ForecastAdjustmentTemperatureCanaryRegistryV2",
    kind: "readonly-interface",
  },
];

// pin the extended reason and active-bundle domain edge for loader roots
const RUNTIME_LOADER_DOMAIN_IMPORT = {
  consumer: "packages/forecast-adjustment/src/runtime-loader.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [
    "packages/domain/src/forecast-adjustment.ts",
    "packages/domain/src/forecast-anchor-record.ts",
    "packages/domain/src/ingestion.ts",
    "packages/domain/src/provenance.ts",
    "packages/domain/src/rain-collection.ts",
    "packages/domain/src/weather-record.ts",
  ],
  exports: {
    ForecastAdjustmentReasonCode:
      "packages/domain/src/forecast-adjustment.ts",
    ForecastAdjustmentRuntimeBundleV2:
      "packages/domain/src/forecast-adjustment.ts",
  },
};

// pin the two direct nominal bundle and authority edges used by loader arms
const FORECAST_ADJUSTMENT_LOADER_IMPORTS = [
  {
    consumer: "packages/forecast-adjustment/src/runtime-loader.ts",
    specifier: "./wind-canary.js",
    entry: "packages/forecast-adjustment/src/wind-canary.ts",
    exports: {
      ForecastAdjustmentWindCanaryRuntimeBundle:
        "packages/forecast-adjustment/src/wind-canary.ts",
    },
  },
  {
    consumer: "packages/forecast-adjustment/src/runtime-loader.ts",
    specifier: "./maintenance-shadow-comparator.js",
    entry:
      "packages/forecast-adjustment/src/maintenance-shadow-comparator.ts",
    exports: {
      MaintenanceShadowServingAuthority:
        "packages/forecast-adjustment/src/maintenance-shadow-comparator.ts",
    },
  },
];

// pin the temperature, wind, and family maintenance runtime packages
const FORECAST_ADJUSTMENT_MAINTENANCE_PACKAGE_ROOTS = [
  {
    source:
      "packages/forecast-adjustment/src/maintenance-runtime-package.ts",
    exportName: "ForecastAdjustmentTemperatureMaintenanceRuntimePackage",
    rustName: "ForecastAdjustmentTemperatureMaintenanceRuntimePackage",
    kind: "readonly-interface",
  },
  {
    source:
      "packages/forecast-adjustment/src/maintenance-runtime-package.ts",
    exportName: "ForecastAdjustmentWindMaintenanceRuntimePackage",
    rustName: "ForecastAdjustmentWindMaintenanceRuntimePackage",
    kind: "readonly-interface",
  },
  {
    source:
      "packages/forecast-adjustment/src/maintenance-runtime-package.ts",
    exportName: "ForecastAdjustmentMaintenanceRuntimePackage",
    rustName: "ForecastAdjustmentMaintenanceRuntimePackage",
    kind: "named-readonly-object-union",
    objectUnion: {
      variants: [
        {
          rustName: "Temperature",
          payloadRustName:
            "ForecastAdjustmentTemperatureMaintenanceRuntimePackage",
        },
        {
          rustName: "Wind",
          payloadRustName:
            "ForecastAdjustmentWindMaintenanceRuntimePackage",
        },
      ],
    },
  },
];

// pin the nominal domain and temperature model edges used by package fields
const FORECAST_ADJUSTMENT_MAINTENANCE_PACKAGE_IMPORTS = [
  {
    consumer:
      "packages/forecast-adjustment/src/maintenance-runtime-package.ts",
    specifier: "@weather/domain",
    packageJson: "packages/domain/package.json",
    entry: "packages/domain/src/index.ts",
    leaves: [
      "packages/domain/src/forecast-adjustment.ts",
      "packages/domain/src/forecast-anchor-record.ts",
      "packages/domain/src/ingestion.ts",
      "packages/domain/src/provenance.ts",
      "packages/domain/src/rain-collection.ts",
      "packages/domain/src/weather-record.ts",
    ],
    exports: {
      ForecastAdjustmentCandidateV2:
        "packages/domain/src/forecast-adjustment.ts",
      ForecastAdjustmentForecastIdentity:
        "packages/domain/src/forecast-adjustment.ts",
    },
  },
  {
    consumer:
      "packages/forecast-adjustment/src/maintenance-runtime-package.ts",
    specifier: "./temperature-mos-runtime.js",
    entry: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    exports: {
      TemperatureMosPermanentRuntimeModel:
        "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    },
  },
];

// pin the closed nullable wind registry and exact metric-band tuple
const FORECAST_ADJUSTMENT_WIND_REGISTRY_ROOT = {
  source: "packages/forecast-adjustment/src/runtime-loader.ts",
  exportName: "ForecastAdjustmentWindCanaryRegistryV2",
  rustName: "ForecastAdjustmentWindCanaryRegistryV2",
  kind: "readonly-interface",
};

// pin the direct value identity used to freeze the selected tuple
const FORECAST_ADJUSTMENT_WIND_REGISTRY_IMPORT = {
  consumer: "packages/forecast-adjustment/src/runtime-loader.ts",
  specifier: "./candidate.js",
  entry: "packages/forecast-adjustment/src/candidate.ts",
  exports: {},
  tupleIdentityWrapper: "deepFreeze",
};

// pin both complete maintenance-extended loader outcome unions
const FORECAST_ADJUSTMENT_MAINTENANCE_LOADER_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/runtime-loader.ts",
    exportName: "LoadedForecastAdjustmentWindCanaryRuntime",
    rustName: "LoadedForecastAdjustmentWindCanaryRuntime",
    kind: "maintenance-loader-extension-union",
    maintenanceLoaderExtension: {
      legacy: "LoadedForecastAdjustmentWindCanaryRuntimeV1",
      package: "ForecastAdjustmentWindMaintenanceRuntimePackage",
      bundleField: "candidate",
    },
  },
  {
    source: "packages/forecast-adjustment/src/runtime-loader.ts",
    exportName: "LoadedForecastAdjustmentTemperatureCanaryRuntime",
    rustName: "LoadedForecastAdjustmentTemperatureCanaryRuntime",
    kind: "maintenance-loader-extension-union",
    maintenanceLoaderExtension: {
      legacy: "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1",
      package: "ForecastAdjustmentTemperatureMaintenanceRuntimePackage",
      bundleField: "model",
    },
  },
];

// pin the package types selected through maintenance bundle indexed fields
const FORECAST_ADJUSTMENT_MAINTENANCE_LOADER_IMPORT = {
  consumer: "packages/forecast-adjustment/src/runtime-loader.ts",
  specifier: "./maintenance-runtime-package.js",
  entry:
    "packages/forecast-adjustment/src/maintenance-runtime-package.ts",
  exports: {
    ForecastAdjustmentTemperatureMaintenanceRuntimePackage:
      "packages/forecast-adjustment/src/maintenance-runtime-package.ts",
    ForecastAdjustmentWindMaintenanceRuntimePackage:
      "packages/forecast-adjustment/src/maintenance-runtime-package.ts",
  },
};

// pin the complete loaded rain runtime registry state union
const LOADED_RAIN_RUNTIME_REGISTRY_ROOT = {
  source: "packages/forecast-adjustment/src/rain-runtime-registry.ts",
  exportName: "LoadedForecastAdjustmentRainRuntimeRegistryV1",
  rustName: "LoadedForecastAdjustmentRainRuntimeRegistryV1",
  kind: "tagged-readonly-object-union",
  activeImportedConstString: "artifactSha256",
  discriminator: {
    field: "state",
    variants: [
      {
        rustName: "Active",
        payloadRustName:
          "LoadedForecastAdjustmentRainRuntimeRegistryV1Active",
        wireValue: "active",
      },
      {
        rustName: "Disabled",
        payloadRustName:
          "LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled",
        wireValue: "disabled",
      },
    ],
  },
};

// pin the imported digest value and comparator authority used by registry arms
const LOADED_RAIN_RUNTIME_REGISTRY_IMPORTS = [
  {
    consumer: "packages/forecast-adjustment/src/rain-runtime-registry.ts",
    specifier: "./rain-hurdle-wind-artifact.js",
    entry:
      "packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts",
    exports: {},
    literalStringValue: "RAIN_HURDLE_WIND_ARTIFACT_SHA256",
  },
  {
    consumer: "packages/forecast-adjustment/src/rain-runtime-registry.ts",
    specifier: "./maintenance-shadow-comparator.js",
    entry:
      "packages/forecast-adjustment/src/maintenance-shadow-comparator.ts",
    exports: {
      MaintenanceShadowServingAuthority:
        "packages/forecast-adjustment/src/maintenance-shadow-comparator.ts",
    },
  },
];

// pin the private loader union and four startup snapshot interfaces
const API_STARTUP_SNAPSHOT_ROOTS = [
  {
    source: "apps/api/src/main.ts",
    exportName: "LoadedApiForecastAdjustmentRuntime",
    rustName: "LoadedApiForecastAdjustmentRuntime",
    kind: "named-readonly-object-union",
    localAlias: true,
    objectUnion: {
      variants: [
        {
          rustName: "Standard",
          payloadRustName: "LoadedForecastAdjustmentRuntimeV1",
        },
        {
          rustName: "Wind",
          payloadRustName: "LoadedForecastAdjustmentWindCanaryRuntime",
        },
      ],
    },
  },
  {
    source: "apps/api/src/main.ts",
    exportName: "ForecastAdjustmentStartupSnapshot",
    rustName: "ForecastAdjustmentStartupSnapshot",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/main.ts",
    exportName: "ForecastTemperatureAdjustmentStartupSnapshot",
    rustName: "ForecastTemperatureAdjustmentStartupSnapshot",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/main.ts",
    exportName: "ForecastRainAdjustmentStartupSnapshot",
    rustName: "ForecastRainAdjustmentStartupSnapshot",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/rain-adjustment.ts",
    exportName: "ApiRainAdjustmentStartupSnapshot",
    rustName: "ApiRainAdjustmentStartupSnapshot",
    kind: "readonly-interface",
  },
];

// pin the four generated loader types consumed by the API startup module
const API_MAIN_FORECAST_ADJUSTMENT_IMPORT = {
  consumer: "apps/api/src/main.ts",
  specifier: "@weather/forecast-adjustment",
  packageJson: "packages/forecast-adjustment/package.json",
  entry: "packages/forecast-adjustment/src/index.ts",
  leaves: [
    "packages/forecast-adjustment/src/algorithm-v1.ts",
    "packages/forecast-adjustment/src/apply.ts",
    "packages/forecast-adjustment/src/bootstrap-v1.ts",
    "packages/forecast-adjustment/src/calendar.ts",
    "packages/forecast-adjustment/src/candidate.ts",
    "packages/forecast-adjustment/src/evaluate.ts",
    "packages/forecast-adjustment/src/evidence.ts",
    "packages/forecast-adjustment/src/holdout-ledger.ts",
    "packages/forecast-adjustment/src/maintenance-capture-epoch.ts",
    "packages/forecast-adjustment/src/maintenance-policy.ts",
    "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    "packages/forecast-adjustment/src/maintenance-runtime-package.ts",
    "packages/forecast-adjustment/src/maintenance-shadow-catalog.ts",
    "packages/forecast-adjustment/src/maintenance-shadow-comparator.ts",
    "packages/forecast-adjustment/src/maintenance-shadow-values.ts",
    "packages/forecast-adjustment/src/performance-scorecard.ts",
    "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts",
    "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    "packages/forecast-adjustment/src/rain-maintenance-controls.ts",
    "packages/forecast-adjustment/src/rain-runtime-registry.ts",
    "packages/forecast-adjustment/src/runtime-bundle.ts",
    "packages/forecast-adjustment/src/runtime-loader.ts",
    "packages/forecast-adjustment/src/temperature-canary.ts",
    "packages/forecast-adjustment/src/temperature-lead-research.ts",
    "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    "packages/forecast-adjustment/src/temperature-weather-research.ts",
    "packages/forecast-adjustment/src/wind-canary.ts",
  ],
  exports: {
    LoadedForecastAdjustmentRainRuntimeRegistryV1:
      "packages/forecast-adjustment/src/rain-runtime-registry.ts",
    LoadedForecastAdjustmentRuntimeV1:
      "packages/forecast-adjustment/src/runtime-loader.ts",
    LoadedForecastAdjustmentTemperatureCanaryRuntime:
      "packages/forecast-adjustment/src/runtime-loader.ts",
    LoadedForecastAdjustmentWindCanaryRuntime:
      "packages/forecast-adjustment/src/runtime-loader.ts",
  },
};

// pin the complete rain inference input, output, and feature-vector boundary
const RAIN_INFERENCE_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainWindForecastHour",
    rustName: "RainWindForecastHour",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainWindRunProfile",
    rustName: "RainWindRunProfile",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainWindStationHour",
    rustName: "RainWindStationHour",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainWindPredictionInput",
    rustName: "RainWindPredictionInput",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainWindPredictionHour",
    rustName: "RainWindPredictionHour",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainWindPredictionResult",
    rustName: "RainWindPredictionResult",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainHurdleWindFeaturePerformance",
    rustName: "RainHurdleWindFeaturePerformance",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainWindPerformanceHour",
    rustName: "RainWindPerformanceHour",
    kind: "readonly-interface",
    base: "RainWindPredictionHour",
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainWindPerformanceResult",
    rustName: "RainWindPerformanceResult",
    kind: "readonly-interface",
    base: "RainWindPredictionResult",
    omitBaseFields: ["hours"],
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainHurdleWindFeatureRow",
    rustName: "RainHurdleWindFeatureRow",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainHurdleWindFeatureProjection",
    rustName: "RainHurdleWindFeatureProjection",
    kind: "readonly-interface",
  },
];

// pin the portable rain runtime artifact and its exact projection catalog
const PORTABLE_RAIN_ARTIFACT_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainHurdleWindProjectionId",
    rustName: "RainHurdleWindProjectionId",
    kind: "string-literal-enum",
    catalog: { kind: "direct-const-string-tuple" },
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainHurdleWindPortableArtifactV1",
    rustName: "RainHurdleWindPortableArtifactV1",
    kind: "readonly-interface",
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainHurdleWindPortableArtifactV2",
    rustName: "RainHurdleWindPortableArtifactV2",
    kind: "readonly-interface",
    base: "RainHurdleWindPortableArtifactV1",
    omitBaseFields: ["contractVersion"],
    addedAfterOmit: ["projectionId"],
  },
  {
    source: "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    exportName: "RainHurdleWindPortableArtifact",
    rustName: "RainHurdleWindPortableArtifact",
    kind: "named-readonly-object-union",
    objectUnion: {
      variants: [
        {
          rustName: "V1",
          payloadRustName: "RainHurdleWindPortableArtifactV1",
        },
        {
          rustName: "V2",
          payloadRustName: "RainHurdleWindPortableArtifactV2",
        },
      ],
    },
  },
];

// pin the shared worker source identity, concrete backfill sources, and deadlines
const WORKER_BACKFILL_SOURCE_ROOTS = [
  {
    source: "apps/worker/src/source-identity.ts",
    exportName: "RuntimeSourceIdentity",
    rustName: "RuntimeSourceIdentity",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/backfill-cli.ts",
    exportName: "BackfillSource",
    rustName: "BackfillSource",
    kind: "readonly-interface",
    base: "RuntimeSourceIdentity",
    importedBase: true,
    identicalImportedStringFields: ["timezone"],
  },
  {
    source: "apps/worker/src/tide-backfill-cli.ts",
    exportName: "TideBackfillSource",
    rustName: "TideBackfillSource",
    kind: "readonly-interface",
    base: "RuntimeSourceIdentity",
    importedBase: true,
    identicalImportedStringFields: ["sourceConfigFingerprint"],
  },
  {
    source: "apps/worker/src/public-stations-backfill-cli.ts",
    exportName: "PublicStationBackfillSource",
    rustName: "PublicStationBackfillSource",
    kind: "readonly-interface",
    base: "RuntimeSourceIdentity",
    importedBase: true,
    identicalImportedStringFields: ["sourceConfigFingerprint"],
  },
  {
    source: "apps/worker/src/tempest-backfill-cli.ts",
    exportName: "TempestBackfillSource",
    rustName: "TempestBackfillSource",
    kind: "readonly-interface",
  },
  {
    source: "apps/worker/src/run-deadline.ts",
    exportName: "IngestionDeadlines",
    rustName: "IngestionDeadlines",
    kind: "readonly-interface",
  },
];

// pin private forecast query rows to their reviewed repository owners
const PRIVATE_FORECAST_REPOSITORY_ROW_ROOTS = [
  {
    source: "packages/database/src/repositories.ts",
    exportName: "ForecastAnchorStorageRow",
    rustName: "ForecastAnchorStorageRow",
    kind: "readonly-interface",
    pgRowNamedFields: true,
    pgQueryOwners: ["listForecastTrainingCohorts"],
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "LegacyV4ForecastStorageRow",
    rustName: "LegacyV4ForecastStorageRow",
    kind: "readonly-interface",
    pgRowNamedFields: true,
    pgQueryOwners: ["listForecastTrainingCohorts"],
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "ForecastObservationStorageRow",
    rustName: "ForecastObservationStorageRow",
    kind: "readonly-interface",
    pgRowNamedFields: true,
    pgQueryOwners: [
      "listForecastObservationHourlyStations",
      "listCausalForecastObservationHourlyStations",
    ],
  },
];

// pin the complete public scorecard and fixed-gauge declaration surface
const FORECAST_SCORECARD_ROOTS = [
  ["ForecastAdjustmentEvidenceClass", "string-literal-enum"],
  ["ForecastAdjustmentSupportState", "string-literal-enum"],
  ["ForecastAdjustmentComparisonState", "string-literal-enum"],
  ["ForecastAdjustmentQualificationState", "string-literal-enum"],
  ["ForecastAdjustmentServingState", "string-literal-enum"],
  ["ForecastAdjustmentPerformancePair", "readonly-interface"],
  ["ForecastAdjustmentWeightedPair", "readonly-interface", "ForecastAdjustmentPerformancePair"],
  ["ForecastAdjustmentMetricSummary", "readonly-interface"],
  ["ForecastAdjustmentPairedMetricSummary", "readonly-interface"],
  ["ForecastAdjustmentBootstrapSummary", "readonly-interface"],
  ["ForecastAdjustmentPerformanceEvaluation", "readonly-interface"],
  ["ForecastAdjustmentStationTargetValue", "readonly-interface"],
  ["ForecastAdjustmentRainGaugeValue", "readonly-interface"],
  ["ForecastAdjustmentRainGaugeTarget", "readonly-interface"],
  ["ForecastAdjustmentRainGaugeInterval", "readonly-interface"],
  ["ForecastAdjustmentRainHourlyTarget", "readonly-interface", "ForecastAdjustmentRainGaugeTarget"],
  ["ForecastAdjustmentRainProbability", "readonly-interface"],
  ["ForecastAdjustmentRainPerformancePair", "readonly-interface", "ForecastAdjustmentPerformancePair"],
  ["ForecastAdjustmentReliabilityBin", "readonly-interface"],
  ["ForecastAdjustmentRainThresholdSummary", "readonly-interface"],
  ["ForecastAdjustmentRainAccumulationSummary", "readonly-interface"],
  ["ForecastAdjustmentRainDiagnostics", "readonly-interface"],
].map(
  // expand the compact ordered catalog into exact manifest root records
  ([exportName, kind, base]) => ({
    source: "packages/forecast-adjustment/src/performance-scorecard.ts",
    exportName,
    rustName: exportName,
    kind,
    ...(base === undefined ? {} : { base }),
  }),
);

// pin installed maintenance receipt history and its immutable capture witness
const INSTALLED_MAINTENANCE_RECEIPT_ROOTS = [
  {
    source:
      "packages/forecast-adjustment/src/maintenance-shadow-catalog.ts",
    exportName: "InstalledMaintenanceShadowReceiptV1",
    rustName: "InstalledMaintenanceShadowReceiptV1",
    kind: "readonly-interface",
  },
  {
    source:
      "packages/forecast-adjustment/src/maintenance-shadow-catalog.ts",
    exportName: "InstalledMaintenanceShadowReceiptV2",
    rustName: "InstalledMaintenanceShadowReceiptV2",
    kind: "readonly-interface",
    base: "InstalledMaintenanceShadowReceiptV1",
    omitBaseFields: ["contractVersion"],
    ownFieldOrderAfterOmit: [
      "actionKind",
      "contractVersion",
      "fullMemberRootSha256",
      "lifecycleHeadSha256",
      "policyDecision",
      "policyReportSha256",
    ],
  },
  {
    source:
      "packages/forecast-adjustment/src/maintenance-shadow-catalog.ts",
    exportName: "InstalledMaintenanceShadowReceipt",
    rustName: "InstalledMaintenanceShadowReceipt",
    kind: "named-readonly-object-union",
    objectUnion: {
      variants: [
        {
          rustName: "V1",
          payloadRustName: "InstalledMaintenanceShadowReceiptV1",
        },
        {
          rustName: "V2",
          payloadRustName: "InstalledMaintenanceShadowReceiptV2",
        },
      ],
    },
  },
  {
    source:
      "packages/forecast-adjustment/src/maintenance-shadow-catalog.ts",
    exportName: "InstalledRainMaintenanceControlReference",
    rustName: "InstalledRainMaintenanceControlReference",
    kind: "readonly-interface",
  },
  {
    source:
      "packages/forecast-adjustment/src/maintenance-capture-epoch.ts",
    exportName: "AdjustmentRevisionCaptureEpochWitness",
    rustName: "AdjustmentRevisionCaptureEpochWitness",
    kind: "readonly-interface",
  },
];

// pin prepared scorecard counters and their closed local reason catalogs
const FORECAST_PREPARED_COUNTER_ROOTS = [
  {
    source: "packages/forecast-adjustment/src/performance-scorecard.ts",
    exportName: "ForecastAdjustmentExclusionReason",
    rustName: "ForecastAdjustmentExclusionReason",
    kind: "string-literal-enum",
  },
  {
    source: "packages/forecast-adjustment/src/performance-scorecard.ts",
    exportName: "ForecastAdjustmentDiagnosticReason",
    rustName: "ForecastAdjustmentDiagnosticReason",
    kind: "string-literal-enum",
  },
  {
    source: "packages/forecast-adjustment/src/performance-scorecard.ts",
    exportName: "ForecastAdjustmentPreparedPairs",
    rustName: "ForecastAdjustmentPreparedPairs",
    kind: "readonly-interface",
  },
];

// pin every newly named public api response root in producer order
const PUBLIC_API_RESPONSE_ROOTS = [
  {
    source: "packages/contracts/src/sites.ts",
    exportName: "ApiSitesResponse",
    rustName: "ApiSitesResponse",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/index.ts",
    exportName: "ApiHealthResponse",
    rustName: "ApiHealthResponse",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/index.ts",
    exportName: "ApiRainCollectionResponse",
    rustName: "ApiRainCollectionResponse",
    kind: "readonly-interface",
  },
  {
    source: "packages/contracts/src/weather-records.ts",
    exportName: "ApiCurrentWeatherResponse",
    rustName: "ApiCurrentWeatherResponse",
    kind: "readonly-interface",
  },
  {
    source: "packages/contracts/src/precipitation-and-tides.ts",
    exportName: "ApiDailyPrecipitationResponse",
    rustName: "ApiDailyPrecipitationResponse",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/index.ts",
    exportName: "ApiTrendsResponse",
    rustName: "ApiTrendsResponse",
    kind: "readonly-interface",
  },
  {
    source: "packages/contracts/src/precipitation-and-tides.ts",
    exportName: "ApiTidesResponse",
    rustName: "ApiTidesResponse",
    kind: "readonly-interface",
  },
  {
    source: "packages/contracts/src/weather-records.ts",
    exportName: "ApiHistoryResponse",
    rustName: "ApiHistoryResponse",
    kind: "readonly-interface",
  },
  {
    source: "apps/api/src/index.ts",
    exportName: "ApiErrorResponse",
    rustName: "ApiErrorResponse",
    kind: "readonly-interface",
  },
];

// pin the public diagnostic and its optional operation catalog
const API_DIAGNOSTIC_ROOTS = [
  {
    source: "apps/api/src/index.ts",
    exportName: "TemperatureCanaryOperation",
    rustName: "TemperatureCanaryOperation",
    kind: "string-literal-enum",
  },
  {
    source: "apps/api/src/index.ts",
    exportName: "ApiDiagnostic",
    rustName: "ApiDiagnostic",
    kind: "readonly-interface",
  },
];

// pin the scheduling, discovery, and forecast observation repository projections
const SCHEDULING_DISCOVERY_FORECAST_ROW_ROOTS = [
  {
    source: "packages/database/src/repositories.ts",
    exportName: "DueSource",
    rustName: "DueSource",
    kind: "readonly-interface",
    pgRowNamedFields: true,
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "ActiveSiteRow",
    rustName: "ActiveSiteRow",
    kind: "readonly-interface",
    pgRowNamedFields: true,
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "DailyPrecipitationRow",
    rustName: "DailyPrecipitationRow",
    kind: "readonly-interface",
    pgRowNamedFields: true,
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "TideRecordRow",
    rustName: "TideRecordRow",
    kind: "readonly-interface",
    pgRowNamedFields: true,
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "LatestWorkerHeartbeat",
    rustName: "LatestWorkerHeartbeat",
    kind: "readonly-interface",
    pgRowNamedFields: true,
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "ForecastObservationInputRow",
    rustName: "ForecastObservationInputRow",
    kind: "readonly-interface",
    pgRowNamedFields: true,
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "ForecastObservationInput",
    rustName: "ForecastObservationInput",
    kind: "readonly-interface",
  },
];
const PG_ROW_MARKER_INPUT_SHA256 = {
  "pg/package.json": "e42dd36cba6e9dd8dbb6f773a2f7be8a8c3c273e18b155e42e75961a4cb8bc28",
  "@types/pg/package.json": "1137081ebb228493e9c81dd699d0d79b7ba0019be75a10272b05399de8cb49e6",
  "@types/pg/index.d.mts": "5aea76ab98173f2c230b1f78dc010da403da622c105c468ace9fe24e3b77883c",
  "@types/pg/index.d.ts": "1f7da5b623c2b1fc4f6fbb56a19df7f6fe2f3e3e7e8b526ec6892062f0f1f82e",
};

// pin the direct sibling type edge consumed by the native temperature projection
const REVISION_PROJECTION_SHADOW_VALUES_IMPORT = {
  consumer: "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
  specifier: "./maintenance-shadow-values.js",
  entry: "packages/forecast-adjustment/src/maintenance-shadow-values.ts",
  exports: {
    MaintenanceShadowTemperatureRecentErrorState:
      "packages/forecast-adjustment/src/maintenance-shadow-values.ts",
  },
};

// pin the domain record edge consumed by the fixed-gauge target document
const FIXED_GAUGE_TARGET_DOMAIN_IMPORT = {
  consumer: "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [
    "packages/domain/src/forecast-adjustment.ts",
    "packages/domain/src/forecast-anchor-record.ts",
    "packages/domain/src/ingestion.ts",
    "packages/domain/src/provenance.ts",
    "packages/domain/src/rain-collection.ts",
    "packages/domain/src/weather-record.ts",
  ],
  exports: {
    NormalizedWeatherRecord: "packages/domain/src/weather-record.ts",
  },
};

// pin the domain JSON edge consumed by the Ecowitt cursor snapshot
const ECOWITT_DOMAIN_IMPORT = {
  consumer: "packages/providers/src/ecowitt.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [
    "packages/domain/src/forecast-adjustment.ts",
    "packages/domain/src/forecast-anchor-record.ts",
    "packages/domain/src/ingestion.ts",
    "packages/domain/src/provenance.ts",
    "packages/domain/src/rain-collection.ts",
    "packages/domain/src/weather-record.ts",
  ],
  exports: {
    JsonValue: "packages/domain/src/provenance.ts",
  },
};

// pin the domain chunk identity consumed by the tide backfill plan
const TIDE_BACKFILL_DOMAIN_IMPORT = {
  consumer: "apps/worker/src/tide-backfill-cli.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [
    "packages/domain/src/forecast-adjustment.ts",
    "packages/domain/src/forecast-anchor-record.ts",
    "packages/domain/src/ingestion.ts",
    "packages/domain/src/provenance.ts",
    "packages/domain/src/rain-collection.ts",
    "packages/domain/src/weather-record.ts",
  ],
  exports: {
    BackfillChunkIdentity: "packages/domain/src/ingestion.ts",
  },
};

// pin the domain chunk identity consumed by the general backfill plan
const BACKFILL_DOMAIN_IMPORT = {
  consumer: "apps/worker/src/backfill-cli.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [
    "packages/domain/src/forecast-adjustment.ts",
    "packages/domain/src/forecast-anchor-record.ts",
    "packages/domain/src/ingestion.ts",
    "packages/domain/src/provenance.ts",
    "packages/domain/src/rain-collection.ts",
    "packages/domain/src/weather-record.ts",
  ],
  exports: {
    BackfillChunkIdentity: "packages/domain/src/ingestion.ts",
  },
};

// pin the domain chunk identity consumed by the Tempest backfill plan
const TEMPEST_BACKFILL_DOMAIN_IMPORT = {
  consumer: "apps/worker/src/tempest-backfill-cli.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [
    "packages/domain/src/forecast-adjustment.ts",
    "packages/domain/src/forecast-anchor-record.ts",
    "packages/domain/src/ingestion.ts",
    "packages/domain/src/provenance.ts",
    "packages/domain/src/rain-collection.ts",
    "packages/domain/src/weather-record.ts",
  ],
  exports: {
    BackfillChunkIdentity: "packages/domain/src/ingestion.ts",
    JsonValue: "packages/domain/src/provenance.ts",
    SourceKind: "packages/domain/src/provenance.ts",
  },
};

// pin the domain chunk identity consumed by the public-station backfill plan
const PUBLIC_STATION_BACKFILL_DOMAIN_IMPORT = {
  consumer: "apps/worker/src/public-stations-backfill-cli.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [
    "packages/domain/src/forecast-adjustment.ts",
    "packages/domain/src/forecast-anchor-record.ts",
    "packages/domain/src/ingestion.ts",
    "packages/domain/src/provenance.ts",
    "packages/domain/src/rain-collection.ts",
    "packages/domain/src/weather-record.ts",
  ],
  exports: {
    BackfillChunkIdentity: "packages/domain/src/ingestion.ts",
  },
};

// pin the v1 widget types consumed by the v2 extensions
const WIDGET_FORECAST_V2_IMPORT = {
  consumer: "apps/web/src/widget-forecast-v2.ts",
  specifier: "./widget-forecast.js",
  entry: "apps/web/src/widget-forecast.ts",
  exports: {
    WidgetForecastField: "apps/web/src/widget-forecast.ts",
    WidgetForecastHour: "apps/web/src/widget-forecast.ts",
    WidgetForecastSnapshot: "apps/web/src/widget-forecast.ts",
  },
};

// pin the v1 calendar type consumed by the v3 indexed extension
const WIDGET_FORECAST_V3_V1_IMPORT = {
  consumer: "apps/web/src/widget-forecast-v3.ts",
  specifier: "./widget-forecast.js",
  entry: "apps/web/src/widget-forecast.ts",
  exports: {
    WidgetForecastSnapshot: "apps/web/src/widget-forecast.ts",
  },
};

// pin the v2 snapshot consumed as the v3 omitted base
const WIDGET_FORECAST_V3_V2_IMPORT = {
  consumer: "apps/web/src/widget-forecast-v3.ts",
  specifier: "./widget-forecast-v2.js",
  entry: "apps/web/src/widget-forecast-v2.ts",
  exports: {
    WidgetForecastV2Snapshot: "apps/web/src/widget-forecast-v2.ts",
  },
};

// pin the domain json and default record types consumed by the provider batch
const PROVIDER_BATCH_DOMAIN_IMPORT = {
  consumer: "packages/providers/src/contract.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [
    "packages/domain/src/forecast-adjustment.ts",
    "packages/domain/src/forecast-anchor-record.ts",
    "packages/domain/src/ingestion.ts",
    "packages/domain/src/provenance.ts",
    "packages/domain/src/rain-collection.ts",
    "packages/domain/src/weather-record.ts",
  ],
  exports: {
    JsonValue: "packages/domain/src/provenance.ts",
    NormalizedWeatherRecord: "packages/domain/src/weather-record.ts",
  },
};

// pin the package edge used by archive relay projection and metadata types
const ARCHIVE_RELAY_FORECAST_ADJUSTMENT_IMPORT = {
  consumer: "apps/api/src/adjustment-maintenance-shadow.ts",
  specifier: "@weather/forecast-adjustment",
  packageJson: "packages/forecast-adjustment/package.json",
  entry: "packages/forecast-adjustment/src/index.ts",
  leaves: [
    "packages/forecast-adjustment/src/algorithm-v1.ts",
    "packages/forecast-adjustment/src/apply.ts",
    "packages/forecast-adjustment/src/bootstrap-v1.ts",
    "packages/forecast-adjustment/src/calendar.ts",
    "packages/forecast-adjustment/src/candidate.ts",
    "packages/forecast-adjustment/src/evaluate.ts",
    "packages/forecast-adjustment/src/evidence.ts",
    "packages/forecast-adjustment/src/holdout-ledger.ts",
    "packages/forecast-adjustment/src/maintenance-capture-epoch.ts",
    "packages/forecast-adjustment/src/maintenance-policy.ts",
    "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    "packages/forecast-adjustment/src/maintenance-runtime-package.ts",
    "packages/forecast-adjustment/src/maintenance-shadow-catalog.ts",
    "packages/forecast-adjustment/src/maintenance-shadow-comparator.ts",
    "packages/forecast-adjustment/src/maintenance-shadow-values.ts",
    "packages/forecast-adjustment/src/performance-scorecard.ts",
    "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts",
    "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
    "packages/forecast-adjustment/src/rain-maintenance-controls.ts",
    "packages/forecast-adjustment/src/rain-runtime-registry.ts",
    "packages/forecast-adjustment/src/runtime-bundle.ts",
    "packages/forecast-adjustment/src/runtime-loader.ts",
    "packages/forecast-adjustment/src/temperature-canary.ts",
    "packages/forecast-adjustment/src/temperature-lead-research.ts",
    "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
    "packages/forecast-adjustment/src/temperature-weather-research.ts",
    "packages/forecast-adjustment/src/wind-canary.ts",
  ],
  exports: {
    AdjustmentRevisionProjectionKind:
      "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
    MaintenanceShadowPredictionMetadata:
      "packages/forecast-adjustment/src/maintenance-shadow-values.ts",
  },
};

// pin the database edge consumed by the archive relay publish input
const ARCHIVE_RELAY_DATABASE_IMPORT = {
  consumer: "apps/api/src/adjustment-maintenance-shadow.ts",
  specifier: "@weather/database",
  packageJson: "packages/database/package.json",
  entry: "packages/database/src/index.ts",
  leaves: [
    "packages/database/src/adjustment-maintenance.ts",
    "packages/database/src/config.ts",
    "packages/database/src/ecowitt-config.ts",
    "packages/database/src/migrate.ts",
    "packages/database/src/pool.ts",
    "packages/database/src/public-stations-config.ts",
    "packages/database/src/rain-adjustment.ts",
    "packages/database/src/rain-collection.ts",
    "packages/database/src/repositories.ts",
    "packages/database/src/tempest-config.ts",
    "packages/database/src/tides-config.ts",
  ],
  compiledSource: {
    tsconfig: "packages/database/tsconfig.json",
    base: "tsconfig.base.json",
  },
  exports: {
    AdjustmentRevisionCommitReceipt:
      "packages/database/src/adjustment-maintenance.ts",
  },
};

// pin the worker source identity and database configuration type graph
const WORKER_BACKFILL_SOURCE_IMPORTS = [
  {
    consumer: "apps/worker/src/source-identity.ts",
    specifier: "@weather/domain",
    packageJson: "packages/domain/package.json",
    entry: "packages/domain/src/index.ts",
    leaves: [...TIDE_BACKFILL_DOMAIN_IMPORT.leaves],
    exports: {
      JsonValue: "packages/domain/src/provenance.ts",
      SourceKind: "packages/domain/src/provenance.ts",
    },
  },
  {
    consumer: "apps/worker/src/backfill-cli.ts",
    specifier: "./source-identity.js",
    entry: "apps/worker/src/source-identity.ts",
    exports: {
      RuntimeSourceIdentity: "apps/worker/src/source-identity.ts",
    },
  },
  {
    consumer: "apps/worker/src/tide-backfill-cli.ts",
    specifier: "./source-identity.js",
    entry: "apps/worker/src/source-identity.ts",
    exports: {
      RuntimeSourceIdentity: "apps/worker/src/source-identity.ts",
    },
  },
  {
    consumer: "apps/worker/src/public-stations-backfill-cli.ts",
    specifier: "./source-identity.js",
    entry: "apps/worker/src/source-identity.ts",
    exports: {
      RuntimeSourceIdentity: "apps/worker/src/source-identity.ts",
    },
  },
  {
    consumer: "apps/worker/src/tide-backfill-cli.ts",
    specifier: "@weather/database",
    packageJson: ARCHIVE_RELAY_DATABASE_IMPORT.packageJson,
    entry: ARCHIVE_RELAY_DATABASE_IMPORT.entry,
    leaves: [...ARCHIVE_RELAY_DATABASE_IMPORT.leaves],
    compiledSource: { ...ARCHIVE_RELAY_DATABASE_IMPORT.compiledSource },
    exports: {
      TideStationConfiguration: "packages/database/src/tides-config.ts",
    },
  },
  {
    consumer: "apps/worker/src/public-stations-backfill-cli.ts",
    specifier: "@weather/database",
    packageJson: ARCHIVE_RELAY_DATABASE_IMPORT.packageJson,
    entry: ARCHIVE_RELAY_DATABASE_IMPORT.entry,
    leaves: [...ARCHIVE_RELAY_DATABASE_IMPORT.leaves],
    compiledSource: { ...ARCHIVE_RELAY_DATABASE_IMPORT.compiledSource },
    exports: {
      PublicStationConfigurationStation:
        "packages/database/src/public-stations-config.ts",
      PublicStationSourceConfiguration:
        "packages/database/src/public-stations-config.ts",
    },
  },
  {
    consumer: "apps/worker/src/tempest-backfill-cli.ts",
    specifier: "@weather/database",
    packageJson: ARCHIVE_RELAY_DATABASE_IMPORT.packageJson,
    entry: ARCHIVE_RELAY_DATABASE_IMPORT.entry,
    leaves: [...ARCHIVE_RELAY_DATABASE_IMPORT.leaves],
    compiledSource: { ...ARCHIVE_RELAY_DATABASE_IMPORT.compiledSource },
    exports: {
      TempestStationConfiguration:
        "packages/database/src/tempest-config.ts",
    },
  },
];

// pin the sole nominal station key edge consumed by the scorecard
const FORECAST_SCORECARD_DOMAIN_IMPORT = {
  consumer: "packages/forecast-adjustment/src/performance-scorecard.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [...TIDE_BACKFILL_DOMAIN_IMPORT.leaves],
  exports: {
    ForecastObservationStationKey:
      "packages/domain/src/forecast-adjustment.ts",
  },
};

// pin the domain package edge consumed by rain revision storage
const RAIN_COLLECTION_DOMAIN_IMPORT = {
  consumer: "packages/database/src/rain-collection.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [
    "packages/domain/src/forecast-adjustment.ts",
    "packages/domain/src/forecast-anchor-record.ts",
    "packages/domain/src/ingestion.ts",
    "packages/domain/src/provenance.ts",
    "packages/domain/src/rain-collection.ts",
    "packages/domain/src/weather-record.ts",
  ],
  exports: {
    NormalizedWeatherRecord: "packages/domain/src/weather-record.ts",
  },
};

// pin the sibling receipt edge consumed by fixed-gauge revision storage
const RAIN_COLLECTION_ADJUSTMENT_IMPORT = {
  consumer: "packages/database/src/rain-collection.ts",
  specifier: "./adjustment-maintenance.js",
  entry: "packages/database/src/adjustment-maintenance.ts",
  exports: {
    AdjustmentRevisionCommitReceipt:
      "packages/database/src/adjustment-maintenance.ts",
    AdjustmentRevisionGap:
      "packages/database/src/adjustment-maintenance.ts",
  },
};

// pin the source-only package edge consumed by the wind-canary roots
const WIND_CANARY_DOMAIN_IMPORT = {
  consumer: "packages/forecast-adjustment/src/wind-canary.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [
    "packages/domain/src/forecast-adjustment.ts",
    "packages/domain/src/forecast-anchor-record.ts",
    "packages/domain/src/ingestion.ts",
    "packages/domain/src/provenance.ts",
    "packages/domain/src/rain-collection.ts",
    "packages/domain/src/weather-record.ts",
  ],
  exports: {
    ForecastAdjustmentCoefficient:
      "packages/domain/src/forecast-adjustment.ts",
    ForecastAdjustmentForecastIdentity:
      "packages/domain/src/forecast-adjustment.ts",
    ForecastAdjustmentMetricBand:
      "packages/domain/src/forecast-adjustment.ts",
    ForecastAdjustmentTrainingEnvelope:
      "packages/domain/src/forecast-adjustment.ts",
    ForecastAdjustmentTrainingProvenance:
      "packages/domain/src/forecast-adjustment.ts",
    ForecastAdjustmentWindCanaryRuntimeBundleV1:
      "packages/domain/src/forecast-adjustment.ts",
    ForecastAdjustmentWindCanaryTrainingIdentity:
      "packages/domain/src/forecast-adjustment.ts",
  },
};

// pin the provenance enums consumed by the canonical site contracts
const CONTRACTS_DOMAIN_IMPORT = {
  consumer: "packages/contracts/src/sites.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [...TIDE_BACKFILL_DOMAIN_IMPORT.leaves],
  exports: {
    SourceKind: "packages/domain/src/provenance.ts",
    StationKind: "packages/domain/src/provenance.ts",
  },
};

// pin the API type-only edge to the canonical site contract package
const API_CONTRACTS_IMPORT = {
  consumer: "apps/api/src/index.ts",
  specifier: "@weather/contracts",
  packageJson: "packages/contracts/package.json",
  entry: "packages/contracts/src/index.ts",
  leaves: [
    "packages/contracts/src/precipitation-and-tides.ts",
    "packages/contracts/src/sites.ts",
    "packages/contracts/src/weather-records.ts",
  ],
  exports: {
    ApiSite: "packages/contracts/src/sites.ts",
    ApiWeatherRecord: "packages/contracts/src/weather-records.ts",
  },
};

// pin the shared site identity consumed by public hydrology responses
const CONTRACTS_SITE_IMPORT = {
  consumer: "packages/contracts/src/precipitation-and-tides.ts",
  specifier: "./sites.js",
  entry: "packages/contracts/src/sites.ts",
  exports: {
    ApiSite: "packages/contracts/src/sites.ts",
  },
};

// pin provenance used by the canonical public weather records
const CONTRACTS_WEATHER_DOMAIN_IMPORT = {
  consumer: "packages/contracts/src/weather-records.ts",
  specifier: "@weather/domain",
  packageJson: "packages/domain/package.json",
  entry: "packages/domain/src/index.ts",
  leaves: [...TIDE_BACKFILL_DOMAIN_IMPORT.leaves],
  exports: {
    SourceKind: "packages/domain/src/provenance.ts",
  },
};

// pin the site envelope used by current and history responses
const CONTRACTS_WEATHER_SITE_IMPORT = {
  consumer: "packages/contracts/src/weather-records.ts",
  specifier: "./sites.js",
  entry: "packages/contracts/src/sites.ts",
  exports: {
    ApiSite: "packages/contracts/src/sites.ts",
  },
};

// retain feature root order without relying on global tail position
function assertManifestRootGroup(roots, expectedRoots) {
  const identities = new Set(expectedRoots.map(
    // use the original module and export together
    ({ source, exportName }) => `${source}\0${exportName}`,
  ));
  assert.deepEqual(
    roots.filter(
      // retain only the exact roots selected for this feature
      ({ source, exportName }) => identities.has(`${source}\0${exportName}`),
    ),
    expectedRoots,
  );
}

// select the exact type imports while preserving their manifest order
function assertManifestImportGroup(imports, expectedImports) {
  const key = ({ consumer, specifier }) => `${consumer}\0${specifier}`;
  const identities = new Set(expectedImports.map(key));
  assert.deepEqual(imports.filter((entry) => identities.has(key(entry))), expectedImports);
}

// hash exact utf-8 contract bytes
function sha256(value) {
  return createHash("sha256").update(value, "utf8").digest("hex");
}

// canonicalize json object keys recursively
function canonicalJson(value) {
  // preserve json scalar spelling
  if (value === null || typeof value !== "object") {
    return JSON.stringify(value);
  }
  // preserve semantic array order
  if (Array.isArray(value)) {
    return `[${value.map(
      // canonicalize one array member
      (entry) => canonicalJson(entry),
    ).join(",")}]`;
  }
  const keys = Object.keys(value).sort();
  return `{${keys.map(
    // canonicalize one keyed member
    (key) => `${JSON.stringify(key)}:${canonicalJson(value[key])}`,
  ).join(",")}}`;
}

// load every configured source as exact text
function repositorySources(config) {
  const sources = {};
  // retain one copy of each shared source
  for (const root of config.roots) {
    // avoid replacing an identical shared source
    if (!(root.source in sources)) {
      sources[root.source] = readFileSync(
        new URL(`../${root.source}`, import.meta.url),
        "utf8",
      );
    }
    // retain every explicit indexed-literal alias source
    for (const source of Object.values(root.indexedLiteralSources ?? {})) {
      // avoid replacing an identical root or alias source
      if (!(source in sources)) {
        sources[source] = readFileSync(
          new URL(`../${source}`, import.meta.url),
          "utf8",
        );
      }
    }
  }
  // retain every explicit package and barrel proof input
  for (const typeImport of config.typeImports ?? []) {
    for (const source of [
      ...(typeImport.packageJson === undefined ? [] : [typeImport.packageJson]),
      typeImport.entry,
      ...(typeImport.leaves ?? []),
      // retain the selected source compiler proof files
      ...(typeImport.compiledSource === undefined
        ? []
        : [typeImport.compiledSource.tsconfig, typeImport.compiledSource.base]),
    ]) {
      // avoid replacing a selected root or shared proof input
      if (!(source in sources)) {
        sources[source] = readFileSync(
          new URL("../" + source, import.meta.url),
          "utf8",
        );
      }
    }
  }
  return sources;
}

// build one closed fixture configuration
function fixtureConfig(exportName, fields = {}) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [{ source: "fixture.ts", exportName, fields }],
  };
}

// build one explicitly recursive json fixture configuration
function recursiveJsonFixtureConfig(extraRoots = []) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [
      {
        source: "fixture.ts",
        exportName: "JsonValue",
        recursiveJson: "JsonPrimitive",
        fields: {},
      },
      ...extraRoots,
    ],
  };
}

test("keeps temperature fallback records as an array of recursive JSON objects", () => {
  const { rust, manifest } = repositoryProjection();
  assertManifestRootGroup(JSON.parse(manifest).roots, TEMPERATURE_DATA_WIRE_ROOTS);
  assert.deepEqual(rustStructFieldEntries(rust, "TemperaturePerformanceDataWire"), [
    ["actualServingFallbacks", "Vec<Vec<(Vec<u16>, JsonValue)>>"],
    ["inputs", "ForecastAdjustmentPerformancePackageInputs"],
    ["rows", "Vec<TemperaturePerformanceDataRow>"],
  ]);
  const config = recursiveJsonFixtureConfig([{
    source: "fixture.ts",
    exportName: "Metadata",
    fields: { values: "ordered-json-object-array" },
  }]);
  // retain object-only elements rather than widening an arbitrary json array
  assert.throws(() => generateRustContracts(config, {
    "fixture.ts": `export type JsonPrimitive = boolean | number | string | null;
export type JsonValue = JsonPrimitive | readonly JsonValue[] | { readonly [key: string]: JsonValue };
export interface Metadata { readonly values: readonly JsonValue[]; }`,
  }), /ordered-json-object-array requires .*Metadata\.values/u);
});

// build one explicitly catalog-backed fixture configuration
function catalogFixtureConfig(exportName, catalog) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [{ source: "fixture.ts", exportName, catalog, fields: {} }],
  };
}

// build one exact single-literal exclusion fixture
function excludeLiteralFixtureConfig() {
  return {
    ...fixtureConfig("Metric"),
    roots: [
      { source: "fixture.ts", exportName: "Metric", fields: {} },
      {
        source: "fixture.ts",
        exportName: "Root",
        fields: {},
        fieldProjections: {
          metric: { kind: "exclude-literal", root: "Metric" },
        },
      },
    ],
  };
}

// build one explicitly tagged fixture configuration
function taggedFixtureConfig(exportName, discriminator, fields = {}) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [{
      source: "fixture.ts",
      exportName,
      discriminator,
      fields,
    }],
  };
}

// build one explicitly readonly-wrapped tagged fixture configuration
function readonlyTaggedFixtureConfig(fields = {}) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [{
      source: "fixture.ts",
      exportName: "Root",
      discriminator: "kind",
      readonlyTaggedUnion: true,
      fields,
    }],
  };
}

// build one tagged intersection with a configured flat base
function taggedBaseFixtureConfig(fields = { "[first].key": "const-string" }) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [
      {
        source: "fixture.ts",
        exportName: "Base",
        fields: { count: "safe-unsigned-integer" },
      },
      { source: "fixture.ts", exportName: "OtherKey", fields: {} },
      {
        source: "fixture.ts",
        exportName: "Root",
        base: "Base",
        discriminator: "kind",
        fields,
      },
    ],
  };
}

// build one explicitly named object-union fixture configuration
function objectUnionFixtureConfig(objectUnion, fields = {}) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [
      {
        source: "fixture.ts",
        exportName: "Root",
        objectUnion,
        fields,
      },
      { source: "fixture.ts", exportName: "FirstPayload", fields: {} },
      { source: "fixture.ts", exportName: "SecondPayload", fields: {} },
      { source: "fixture.ts", exportName: "ThirdPayload", fields: {} },
    ],
  };
}

// build one explicitly projected literal-union fixture configuration
function literalUnionFixtureConfig(literalUnion, extraRoots = []) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [
      {
        source: "fixture.ts",
        exportName: "Root",
        literalUnion,
        fields: {},
      },
      ...extraRoots,
    ],
  };
}

// build one same-source multi-root fixture configuration
function fixtureRoots(...exportNames) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: exportNames.map(
      // configure one named fixture root
      (exportName) => ({ source: "fixture.ts", exportName, fields: {} }),
    ),
  };
}

// build one configured metric enum and mapped record fixture
function metricRecordFixtureConfig(mapping) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [
      { source: "fixture.ts", exportName: "Metric", fields: {} },
      {
        source: "fixture.ts",
        exportName: "Root",
        fields: { adjustedMetrics: mapping },
      },
    ],
  };
}

// build one configured same-source interface inheritance fixture
function inheritedFixtureConfig(base = "Base", fields = {}) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [
      { source: "fixture.ts", exportName: "Base", fields: {} },
      { source: "fixture.ts", exportName: "Root", base, fields },
    ],
  };
}

// build one exact same-source pick fixture configuration
function pickFixtureConfig(fields = ["count", "kind"]) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [
      {
        source: "fixture.ts",
        exportName: "Base",
        fields: { count: "safe-unsigned-integer" },
      },
      {
        source: "fixture.ts",
        exportName: "Root",
        fields: {},
        pick: { root: "Base", fields },
      },
    ],
  };
}

// read one generated struct's fields in emitted order
function rustStructFieldEntries(rust, name, declarationName = name) {
  const block = rust.match(
    new RegExp("^pub struct " + declarationName + " \\{\\n([\\s\\S]+?)^\\}", "mu"),
  );
  assert.ok(block, name);
  return Array.from(
    block[1].matchAll(/^    pub (\w+): (.+),$/gmu),
    // retain one exact field entry
    ([, field, type]) => [field, type],
  );
}

// read one complete generated field block including its metadata
function rustStructFieldBlock(rust, name, field, declarationName = name) {
  const block = rust.match(
    new RegExp("^pub struct " + declarationName + " \\{\\n([\\s\\S]+?)^\\}", "mu"),
  );
  assert.ok(block, name);
  const fieldBlock = block[1].match(
    new RegExp(
      "((?:^    #\\[doc = [^\\n]+\\n)*)^    pub " + field + ": [^\\n]+,$",
      "mu",
    ),
  );
  assert.ok(fieldBlock, `${name}.${field}`);
  return fieldBlock[0];
}

// read one generated string enum's exact wire mappings
function rustStringEnumWireEntries(rust, name) {
  const block = rust.match(
    new RegExp(
      "^impl " + name + " \\{\\n([\\s\\S]+?)^\\}",
      "mu",
    ),
  );
  assert.ok(block, name);
  return Array.from(
    block[1].matchAll(/^        \("([^"]+)", Self::(\w+)\),$/gmu),
    // retain one exact wire value and Rust variant
    ([, wireValue, variant]) => [wireValue, variant],
  );
}

// read one generated unsigned integer enum's exact wire mappings
function rustUnsignedIntegerEnumWireEntries(rust, name) {
  const block = rust.match(
    new RegExp(
      "^impl " + name + " \\{\\n([\\s\\S]+?)^\\}",
      "mu",
    ),
  );
  assert.ok(block, name);
  return Array.from(
    block[1].matchAll(/^        \(([0-9]+), Self::(\w+)\),$/gmu),
    // retain one exact numeric wire value and Rust variant
    ([, wireValue, variant]) => [Number(wireValue), variant],
  );
}

// build one closed type-only package import fixture
function importedFixtureConfig(
  consumerSource = "fixture.ts",
  importedName = "Child",
) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [
      { source: "fixture-domain/src/types.ts", exportName: importedName, fields: {} },
      { source: consumerSource, exportName: "Root", fields: {} },
    ],
    typeImports: [{
      consumer: consumerSource,
      specifier: "@fixture/domain",
      packageJson: "fixture-domain/package.json",
      entry: "fixture-domain/src/index.ts",
      leaves: ["fixture-domain/src/types.ts"],
      exports: { [importedName]: "fixture-domain/src/types.ts" },
    }],
  };
}

// provide one package proof graph and consumer to the fixture generator
function importedFixtureSources(
  consumerSource = "fixture.ts",
  importStatement = "import type { Child } from \"@fixture/domain\";",
  rootField = "readonly child: Child;",
) {
  return {
    [consumerSource]: importStatement + "\nexport interface Root { " + rootField + " }\n",
    "fixture-domain/package.json": JSON.stringify({
      name: "@fixture/domain",
      version: "0.0.0",
      private: true,
      type: "module",
      exports: {
        ".": {
          types: "./src/index.ts",
          default: "./dist/index.js",
        },
      },
      types: "./src/index.ts",
    }, null, 2),
    "fixture-domain/src/index.ts": "export * from \"./types.js\";\n",
    "fixture-domain/src/types.ts": "export interface Child { readonly value: string; }\n",
  };
}

// build one closed package graph for the v13 imported projection forms
function projectedFixtureConfig(fieldProjections = {
  appliedMetrics: { kind: "const-catalog-array", root: "Metric" },
  leadBand: { kind: "return-enum", root: "Band" },
}, leaves = ["fixture-domain/src/types.ts"]) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [
      {
        source: "fixture-domain/src/types.ts",
        exportName: "Metric",
        catalog: { kind: "satisfies-string-tuple" },
        fields: {},
      },
      {
        source: "fixture-domain/src/types.ts",
        exportName: "Band",
        fields: {},
      },
      {
        source: "fixture.ts",
        exportName: "Root",
        fields: { adjustedMetrics: "finite-partial-metric-record" },
        fieldProjections,
      },
    ],
    typeImports: [{
      consumer: "fixture.ts",
      specifier: "@fixture/domain",
      packageJson: "fixture-domain/package.json",
      entry: "fixture-domain/src/index.ts",
      leaves,
      exports: {
        Band: "fixture-domain/src/types.ts",
        Metric: "fixture-domain/src/types.ts",
      },
    }],
  };
}

// provide exact inert source text for the v13 imported projection forms
function projectedFixtureSources(
  importStatement = 'import { METRICS, bandFor, type Band, type Metric } from "@fixture/domain";',
  rootFields = `readonly adjustedMetrics: Partial<Record<Metric, number>>;
  readonly appliedMetrics: readonly (typeof METRICS)[number][];
  readonly band: Band;
  readonly leadBand: ReturnType<typeof bandFor>;
  readonly metric: Metric;`,
  providerSource = `type MetricName = string;
export const METRICS = ["windSpeed", "windGust"] as const satisfies readonly MetricName[];
export type Metric = (typeof METRICS)[number];
export type Band = "short" | "long";
// return one annotated band without evaluating the body
export function bandFor(value: number): Band { throw new Error(String(value)); }
`,
) {
  return {
    "fixture.ts": `${importStatement}
export interface Root {
  ${rootFields}
}
`,
    "fixture-domain/package.json": JSON.stringify({
      name: "@fixture/domain",
      version: "0.0.0",
      private: true,
      type: "module",
      exports: {
        ".": {
          types: "./src/index.ts",
          default: "./dist/index.js",
        },
      },
      types: "./src/index.ts",
    }, null, 2),
    "fixture-domain/src/index.ts": 'export * from "./types.js";\n',
    "fixture-domain/src/types.ts": providerSource,
  };
}

// build one closed indexed-literal projection fixture
function indexedLiteralFixtureConfig(
  indexedLiteralSources = {
    LoadedRuntime: "fixture.ts",
    LoadedRuntimeV1: "fixture.ts",
  },
  fieldProjections = {
    reasonCode: { kind: "indexed-literals", root: "LoadedRuntime" },
    state: { kind: "indexed-literals", root: "LoadedRuntime" },
  },
) {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [
      { source: "fixture.ts", exportName: "ReasonCode", fields: {} },
      {
        source: "fixture.ts",
        exportName: "Root",
        fields: {},
        indexedLiteralSources,
        fieldProjections,
      },
    ],
  };
}

// provide one same-source alias graph with an exact Extract subset
function indexedLiteralFixtureSource(
  runtimeDeclaration = `type LoadedRuntimeV1 =
  | { readonly ignored: "not_projected"; readonly reasonCode: null; readonly state: "active" }
  | { readonly reasonCode: Extract<ReasonCode, "first" | "second">; readonly state: "disabled" };
type LoadedRuntime =
  | (LoadedRuntimeV1 & { readonly comparator?: "also_not_projected" })
  | { readonly reasonCode: "third"; readonly state: "disabled" };`,
  rootFields = `readonly reasonCode: LoadedRuntime["reasonCode"];
  readonly state: LoadedRuntime["state"];`,
) {
  return `export type ReasonCode = "first" | "second" | "third" | "unobserved";
${runtimeDeclaration}
export interface Root {
  ${rootFields}
}
`;
}

// build one same-source interface-array omission fixture
function omitInterfaceArrayFixtureConfig(root = "Hour") {
  return {
    version: 1,
    typescriptVersion: "6.0.3",
    roots: [
      {
        source: "fixture.ts",
        exportName: "Hour",
        fields: {
          count: "safe-unsigned-integer",
          value: "finite-f64",
        },
      },
      {
        source: "fixture.ts",
        exportName: "Root",
        fields: {},
        fieldProjections: {
          hours: { kind: "omit-interface-array", root },
        },
      },
    ],
  };
}

// provide one direct readonly Omit array over a configured interface
function omitInterfaceArrayFixtureSource(
  hoursType = 'readonly Omit<Hour, "contentHash">[]',
  prefix = "",
) {
  return `${prefix}export interface Hour {
  readonly contentHash: string;
  readonly count: number;
  readonly value: number;
  readonly validAt: string;
}
export interface Root {
  readonly hours: ${hoursType};
}
`;
}

// build one source-only compiled-package proof fixture
function compiledProofFixture({
  baseCompilerOptions = {
    allowJs: false,
    exactOptionalPropertyTypes: true,
    forceConsistentCasingInFileNames: true,
    isolatedModules: true,
    module: "NodeNext",
    moduleResolution: "NodeNext",
    noFallthroughCasesInSwitch: true,
    noImplicitOverride: true,
    noUncheckedIndexedAccess: true,
    resolveJsonModule: true,
    skipLibCheck: true,
    strict: true,
    target: "ES2023",
    verbatimModuleSyntax: true,
  },
  collectorSource = `export interface CollectorRun {
  readonly stateStatus: "supported" | "cold" | "insufficient" | "invalid";
}
export interface CollectorStatus {
  readonly stateStatus: CollectorRun["stateStatus"];
}
`,
  compiledSource = {
    tsconfig: "fixture-database/tsconfig.json",
    base: "tsconfig.base.json",
  },
  extraRoots = [],
  importOverrides = {},
  packageOverrides = {},
  proofExports = {
    CollectorStatus: "fixture-database/src/collector.ts",
  },
  rootOverrides = {},
  rootSource = `import type { CollectorStatus } from "@fixture/database";
export interface Root {
  readonly source: {
    readonly stateStatus: CollectorStatus["stateStatus"];
  };
}
`,
  sourceOverrides = {},
  tsconfigCompilerOptions = {
    declaration: true,
    outDir: "dist",
    rootDir: "src",
    types: ["node"],
  },
  tsconfigOverrides = {},
} = {}) {
  const root = {
    source: "fixture.ts",
    exportName: "Root",
    fields: {},
    indexedLiteralSources: {
      CollectorRun: "fixture-database/src/collector.ts",
      CollectorStatus: "fixture-database/src/collector.ts",
    },
    fieldProjections: {
      "source.stateStatus": {
        kind: "indexed-literals",
        root: "CollectorStatus",
      },
    },
    ...rootOverrides,
  };
  const typeImport = {
    consumer: "fixture.ts",
    specifier: "@fixture/database",
    packageJson: "fixture-database/package.json",
    entry: "fixture-database/src/index.ts",
    leaves: ["fixture-database/src/collector.ts"],
    // omit the compiled-source proof only when a rejection fixture requests it
    ...(compiledSource === null ? {} : { compiledSource }),
    exports: {},
    // omit the proof export only when a rejection fixture requests it
    ...(proofExports === null ? {} : { proofExports }),
    ...importOverrides,
  };
  const packageMetadata = {
    name: "@fixture/database",
    version: "0.0.0",
    private: true,
    type: "module",
    exports: "./dist/index.js",
    types: "./dist/index.d.ts",
    files: ["dist"],
    ...packageOverrides,
  };
  const tsconfig = {
    extends: "../tsconfig.base.json",
    compilerOptions: tsconfigCompilerOptions,
    include: ["src/**/*.ts"],
    ...tsconfigOverrides,
  };
  return {
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [root, ...extraRoots],
      typeImports: [typeImport],
    },
    sources: {
      "fixture.ts": rootSource,
      "fixture-database/package.json": JSON.stringify(packageMetadata, null, 2),
      // include compiler inputs only when the fixture selects their proof
      ...(compiledSource === null ? {} : {
        [compiledSource.tsconfig]: JSON.stringify(tsconfig, null, 2),
        [compiledSource.base]: JSON.stringify({
          compilerOptions: baseCompilerOptions,
        }, null, 2),
      }),
      "fixture-database/src/index.ts": 'export * from "./collector.js";\n',
      "fixture-database/src/collector.ts": collectorSource,
      ...sourceOverrides,
    },
  };
}

test(
  "projects the configured TypeScript roots without changing their wire literals",
  // verify the checked repository contract
  () => {
    assert.deepEqual(Object.keys(generatorModule), ["generateRustContracts"]);
    const { rust, manifest } = repositoryProjection();

    assert.equal(rust.includes("\r"), false);
    assert.equal(manifest.includes("\r"), false);
    assert.equal(rust.endsWith("\n"), true);
    assert.equal(manifest.endsWith("\n"), true);
    assert.match(
      rust,
      /pub enum RainCaptureOutcome \{[\s\S]*\bValid\b[\s\S]*\bInvalid\b[\s\S]*\bTransportError\b[\s\S]*\bRateLimited\b[\s\S]*\bUnauthorized\b[\s\S]*\}/,
    );
    // retain each exact string-union wire mapping
    for (const [wireValue, variant] of [
      ["valid", "Valid"],
      ["invalid", "Invalid"],
      ["transport_error", "TransportError"],
      ["rate_limited", "RateLimited"],
      ["unauthorized", "Unauthorized"],
    ]) {
      assert.match(rust, new RegExp(`wire value: ${wireValue}`));
      assert.match(rust, new RegExp(`\\("${wireValue}", Self::${variant}\\)`));
    }
    assert.match(
      rust,
      /pub struct RainCollectionStatusContractVersionLiteral;\s*impl RainCollectionStatusContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "rain-prospective-capture\/v1";\s*\}/,
    );
    assert.match(
      rust,
      /pub struct RainCollectionStatusModelEnabledLiteral;\s*impl RainCollectionStatusModelEnabledLiteral \{\s*pub const WIRE_VALUE: bool = false;\s*\}/,
    );
    assert.match(
      rust,
      /pub struct RainCollectionStatusQualificationEnabledLiteral;\s*impl RainCollectionStatusQualificationEnabledLiteral \{\s*pub const WIRE_VALUE: bool = false;\s*\}/,
    );
    assert.match(
      rust,
      /pub enum ForecastDays \{[\s\S]*\bValue1\b[\s\S]*\bValue5\b[\s\S]*\bValue10\b[\s\S]*\}/,
    );
    assert.match(
      rust,
      /impl ForecastDays \{\s*pub const WIRE_VALUES: &'static \[\(u64, Self\)\] = &\[\s*\(1, Self::Value1\),\s*\(5, Self::Value5\),\s*\(10, Self::Value10\),\s*\];\s*\}/,
    );
    assert.match(
      rust,
      /pub enum ApiRainAdjustmentDecisionStateEnum \{[\s\S]*\bActive\b[\s\S]*\bDisabled\b[\s\S]*\bRawFallback\b[\s\S]*\}/,
    );
    // retain every exact decision-state wire mapping
    for (const [wireValue, variant] of [
      ["active", "Active"],
      ["disabled", "Disabled"],
      ["raw_fallback", "RawFallback"],
    ]) {
      assert.match(rust, new RegExp(`wire value: ${wireValue}`));
      assert.match(rust, new RegExp(`\\("${wireValue}", Self::${variant}\\)`));
    }
    assert.match(
      rust,
      /pub contractVersion: RainCollectionStatusContractVersionLiteral/,
    );
    assert.match(
      rust,
      /pub modelEnabled: RainCollectionStatusModelEnabledLiteral/,
    );
    assert.match(
      rust,
      /pub qualificationEnabled: RainCollectionStatusQualificationEnabledLiteral/,
    );
    // retain every safe unsigned representation
    for (const fieldName of [
      "claims",
      "receipts",
      "validForecasts",
      "timelyForecasts",
      "validStationWindows",
      "stationsSeen",
      "failedRequests",
      "pendingRequests",
      "unknownRequests",
      "compressedBytes",
    ]) {
      assert.match(rust, new RegExp(`pub ${fieldName}: u64`));
    }
    // retain every required nullable timestamp
    for (const fieldName of [
      "lastClaimAt",
      "lastReceiptAt",
      "lastForecastReceiptAt",
      "lastStationReceiptAt",
      "pausedUntil",
    ]) {
      assert.match(rust, new RegExp(`pub ${fieldName}: Nullable<String>`));
    }
    assert.match(
      rust,
      /pub struct ApiRainAdjustmentDecisionContractVersionLiteral;\s*impl ApiRainAdjustmentDecisionContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-rain-adjustment-decision\/v1";\s*\}/,
    );
    assert.match(rust, /pub state: ApiRainAdjustmentDecisionStateEnum/);
    assert.match(rust, /pub reasonCode: Nullable<String>/);
    assert.match(rust, /pub bundleSha256: Nullable<String>/);
    assert.match(rust, /pub correctedPrecipitationMm: Nullable<f64>/);
    assert.match(rust, /pub rawBestMatchPrecipitationMm: Nullable<f64>/);
    assert.match(
      rust,
      /pub sourceForecast: Nullable<ApiRainAdjustmentDecisionSourceForecast>/,
    );
    assert.match(
      rust,
      /pub struct ApiRainAdjustmentDecisionSourceForecast \{[\s\S]*pub runInitializedAt: String,[\s\S]*pub firstReceivedAt: String,[\s\S]*pub validAt: String,[\s\S]*pub rawPrecipitationMm: f64,[\s\S]*pub modelLeadHours: u64,[\s\S]*pub decisionAt: String,[\s\S]*pub upstreamModel: ApiRainAdjustmentDecisionSourceForecastUpstreamModelLiteral,[\s\S]*pub providerKey: ApiRainAdjustmentDecisionSourceForecastProviderKeyLiteral,[\s\S]*\}/,
    );
    assert.match(
      rust,
      /pub struct ApiRainAdjustmentDecisionSourceForecastUpstreamModelLiteral;\s*impl ApiRainAdjustmentDecisionSourceForecastUpstreamModelLiteral \{\s*pub const WIRE_VALUE: &'static str = "ecmwf_ifs";\s*\}/,
    );
    assert.match(
      rust,
      /pub struct ApiRainAdjustmentDecisionSourceForecastProviderKeyLiteral;\s*impl ApiRainAdjustmentDecisionSourceForecastProviderKeyLiteral \{\s*pub const WIRE_VALUE: &'static str = "open-meteo";\s*\}/,
    );
    assert.match(
      rust,
      /pub struct ForecastObservationQualityRule \{[\s\S]*pub allowedFlags: Vec<String>,[\s\S]*pub statusRule: ForecastObservationQualityRuleStatusRuleEnum,[\s\S]*\}/,
    );
    assert.match(
      rust,
      /impl ForecastObservationQualityRuleStatusRuleEnum \{\s*pub const WIRE_VALUES: &'static \[\(&'static str, Self\)\] = &\[\s*\("absent", Self::Absent\),\s*\("absent_or_provider_qc_1", Self::AbsentOrProviderQc1\),\s*\];\s*\}/,
    );
    assert.match(
      rust,
      /pub enum ForecastObservationStationKey \{[\s\S]*\bAmbientMaxweather\b[\s\S]*\bAmbientMerlin\b[\s\S]*\bBallydideanEcowitt\b[\s\S]*\bNetatmoNearby\b[\s\S]*\bTempest126537\b[\s\S]*\bTempest168853\b[\s\S]*\bTempest201058\b[\s\S]*\bTempest203055\b[\s\S]*\bTempest225947\b[\s\S]*\bTempest38270\b[\s\S]*\bTempest64255\b[\s\S]*\}/,
    );
    assert.match(
      rust,
      /impl ForecastObservationStationKey \{\s*pub const WIRE_VALUES: &'static \[\(&'static str, Self\)\] = &\[\s*\("ambient-maxweather", Self::AmbientMaxweather\),\s*\("ambient-merlin", Self::AmbientMerlin\),\s*\("ballydidean-ecowitt", Self::BallydideanEcowitt\),\s*\("netatmo-nearby", Self::NetatmoNearby\),\s*\("tempest-126537", Self::Tempest126537\),\s*\("tempest-168853", Self::Tempest168853\),\s*\("tempest-201058", Self::Tempest201058\),\s*\("tempest-203055", Self::Tempest203055\),\s*\("tempest-225947", Self::Tempest225947\),\s*\("tempest-38270", Self::Tempest38270\),\s*\("tempest-64255", Self::Tempest64255\),\s*\];\s*\}/,
    );
    assert.match(
      rust,
      /pub struct ForecastObservationSourceLineage \{[\s\S]*pub acceptedEndExclusive: Nullable<String>,[\s\S]*pub acceptedStartInclusive: Nullable<String>,[\s\S]*pub adapterContract: String,[\s\S]*pub checkedFingerprint: String,[\s\S]*pub physicalStationKey: ForecastObservationStationKey,[\s\S]*pub qualityRule: ForecastObservationQualityRule,[\s\S]*pub sourceKey: String,[\s\S]*pub supersededSourceKeys: Vec<String>,[\s\S]*\}/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentRainActiveArtifactV1 \{\s*pub artifactSha256: String,\s*\}/,
    );
    assert.match(
      rust,
      /pub enum ForecastAdjustmentRainRuntimeRegistryV1 \{\s*#\[doc = "wire discriminant rawReason: \\"policy_raw\\""\]\s*PolicyRaw\(ForecastAdjustmentRainRuntimeRegistryV1PolicyRaw\),\s*#\[doc = "wire discriminant rawReason: null"\]\s*Null\(ForecastAdjustmentRainRuntimeRegistryV1Null\),\s*\}/,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentRainRuntimeRegistryV1 \{\s*pub const DISCRIMINATOR_FIELD: &'static str = "rawReason";\s*\}/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentRainRuntimeRegistryV1PolicyRaw \{\s*pub activeArtifact: ForecastAdjustmentRainRuntimeRegistryV1PolicyRawActiveArtifactNull,\s*pub contractVersion: ForecastAdjustmentRainRuntimeRegistryV1PolicyRawContractVersionLiteral,\s*pub rawReason: ForecastAdjustmentRainRuntimeRegistryV1PolicyRawRawReasonLiteral,\s*pub siteKey: ForecastAdjustmentRainRuntimeRegistryV1PolicyRawSiteKeyLiteral,\s*\}/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentRainRuntimeRegistryV1Null \{\s*pub activeArtifact: ForecastAdjustmentRainActiveArtifactV1,\s*pub contractVersion: ForecastAdjustmentRainRuntimeRegistryV1NullContractVersionLiteral,\s*pub rawReason: ForecastAdjustmentRainRuntimeRegistryV1NullRawReasonNull,\s*pub siteKey: ForecastAdjustmentRainRuntimeRegistryV1NullSiteKeyLiteral,\s*\}/,
    );
    assert.match(
      rust,
      /#\[doc = "exact null wire literal"\]\s*#\[derive\(Debug, Clone, Copy, PartialEq, Eq\)\]\s*pub struct ForecastAdjustmentRainRuntimeRegistryV1PolicyRawActiveArtifactNull;/,
    );
    assert.match(
      rust,
      /#\[doc = "exact null wire literal"\]\s*#\[derive\(Debug, Clone, Copy, PartialEq, Eq\)\]\s*pub struct ForecastAdjustmentRainRuntimeRegistryV1NullRawReasonNull;/,
    );
  },
);

test(
  "projects the forecast adjustment catalogs and live source payloads exactly",
  // retain catalog order and original producer storage contracts
  () => {
    const { rust } = repositoryProjection();

    assert.deepEqual(rustStringEnumWireEntries(rust, "ForecastAdjustmentMetric"), [
      ["relativeHumidityPercent", "RelativeHumidityPercent"],
      ["temperatureC", "TemperatureC"],
      ["windDirectionDegrees", "WindDirectionDegrees"],
      ["windGustMps", "WindGustMps"],
      ["windSpeedMps", "WindSpeedMps"],
    ]);
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ForecastAdjustmentWindCanaryMetric"),
      [
        ["windDirectionDegrees", "WindDirectionDegrees"],
        ["windGustMps", "WindGustMps"],
        ["windSpeedMps", "WindSpeedMps"],
      ],
    );
    assert.deepEqual(rustStringEnumWireEntries(rust, "ForecastLeadBandKey"), [
      ["001-024", "Value001024"],
      ["025-048", "Value025048"],
      ["049-072", "Value049072"],
      ["073-096", "Value073096"],
      ["097-120", "Value097120"],
      ["121-144", "Value121144"],
      ["145-168", "Value145168"],
    ]);
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ForecastAdjustmentReasonCode"),
      [
        ["adjustment_error", "AdjustmentError"],
        ["bundle_invalid", "BundleInvalid"],
        ["bundle_missing", "BundleMissing"],
        ["canary_expired", "CanaryExpired"],
        ["canary_killed", "CanaryKilled"],
        ["coefficient_missing", "CoefficientMissing"],
        ["cross_link_mismatch", "CrossLinkMismatch"],
        ["direction_calm", "DirectionCalm"],
        ["evidence_redundancy_missing", "EvidenceRedundancyMissing"],
        ["hash_mismatch", "HashMismatch"],
        ["identity_mismatch", "IdentityMismatch"],
        ["insufficient_data", "InsufficientData"],
        ["metric_not_enabled", "MetricNotEnabled"],
        ["metric_out_of_bounds", "MetricOutOfBounds"],
        ["policy_raw", "PolicyRaw"],
        ["qualification_failed", "QualificationFailed"],
        ["registry_inactive", "RegistryInactive"],
        ["registry_invalid", "RegistryInvalid"],
        ["runtime_fingerprint_mismatch", "RuntimeFingerprintMismatch"],
        ["training_envelope_mismatch", "TrainingEnvelopeMismatch"],
        ["unsupported_lead", "UnsupportedLead"],
        ["wrong_cohort", "WrongCohort"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentRawForecastProvenance"),
      [
        ["adapterVersion", "String"],
        ["cohort", "ForecastAdjustmentRawForecastProvenanceCohortLiteral"],
        ["contractEpoch", "String"],
        ["dataset", "String"],
        ["referenceAt", "String"],
        ["referenceKind", "ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral"],
        ["sourceConfigFingerprint", "String"],
        ["sourceKey", "String"],
        ["targetLeadHours", "u64"],
        ["upstreamModel", "String"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "TemperatureCanarySourceForecast"),
      [
        ["adapterVersion", "String"],
        ["dataset", "TemperatureCanarySourceForecastDatasetLiteral"],
        ["firstReceivedAt", "String"],
        ["modelCycle", "TemperatureCanarySourceForecastModelCycleEnum"],
        ["modelLeadHours", "u64"],
        ["providerKey", "TemperatureCanarySourceForecastProviderKeyLiteral"],
        ["providerResponseSha256", "String"],
        ["rawRelativeHumidityPercent", "Nullable<f64>"],
        ["rawTemperatureC", "f64"],
        ["rawWindSpeedMps", "Nullable<f64>"],
        ["runInitializedAt", "String"],
        ["upstreamModel", "TemperatureCanarySourceForecastUpstreamModelLiteral"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "TemperatureCanarySourceForecastModelCycleEnum"),
      [["49r1", "Value49r1"], ["50r1", "Value50r1"]],
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentRawForecastProvenanceCohortLiteral;\s*impl ForecastAdjustmentRawForecastProvenanceCohortLiteral \{\s*pub const WIRE_VALUE: &'static str = "legacy_v4_retrieval_snapshot";\s*\}/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral;\s*impl ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral \{\s*pub const WIRE_VALUE: &'static str = "retrieval_snapshot";\s*\}/,
    );
    assert.match(
      rust,
      /pub struct TemperatureCanarySourceForecastDatasetLiteral;\s*impl TemperatureCanarySourceForecastDatasetLiteral \{\s*pub const WIRE_VALUE: &'static str = "single_run";\s*\}/,
    );
    assert.match(
      rust,
      /pub struct TemperatureCanarySourceForecastProviderKeyLiteral;\s*impl TemperatureCanarySourceForecastProviderKeyLiteral \{\s*pub const WIRE_VALUE: &'static str = "open-meteo";\s*\}/,
    );
    assert.match(
      rust,
      /pub struct TemperatureCanarySourceForecastUpstreamModelLiteral;\s*impl TemperatureCanarySourceForecastUpstreamModelLiteral \{\s*pub const WIRE_VALUE: &'static str = "ecmwf_ifs";\s*\}/,
    );
    const rawProvenanceDefinition = rust.match(
      /^pub struct ForecastAdjustmentRawForecastProvenance \{\n[\s\S]+?^\}/mu,
    );
    const temperatureSourceDefinition = rust.match(
      /^pub struct TemperatureCanarySourceForecast \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(rawProvenanceDefinition);
    assert.ok(temperatureSourceDefinition);
    assert.doesNotMatch(rawProvenanceDefinition[0], /wire rule: utc-iso/);
    assert.doesNotMatch(temperatureSourceDefinition[0], /wire rule: utc-iso/);
    assert.match(rawProvenanceDefinition[0], /wire rule: safe-unsigned-integer/);
    assert.match(temperatureSourceDefinition[0], /wire rule: safe-unsigned-integer/);
    assert.equal(
      Array.from(temperatureSourceDefinition[0].matchAll(/wire rule: finite-f64/gmu)).length,
      3,
    );
  },
);

test(
  "projects every temperature canary reason from its declared sources",
  // retain source order while separating exact null from the string values
  () => {
    const { rust } = repositoryProjection();

    assert.deepEqual(
      rustStringEnumWireEntries(rust, "TemperatureMosRuntimeRawReason"),
      [
        ["invalid_forecast", "InvalidForecast"],
        ["inference_error", "InferenceError"],
        ["invalid_model", "InvalidModel"],
        ["invalid_recent_error_state", "InvalidRecentErrorState"],
        ["model_identity_mismatch", "ModelIdentityMismatch"],
        ["model_not_supported", "ModelNotSupported"],
        ["model_not_yet_available", "ModelNotYetAvailable"],
        ["outside_assumed_delay6_next12", "OutsideAssumedDelay6Next12"],
        ["outside_initialization_first12", "OutsideInitializationFirst12"],
        ["recent_error_state_as_of_mismatch", "RecentErrorStateAsOfMismatch"],
        ["recent_error_state_contains_future_data", "RecentErrorStateContainsFutureData"],
        ["recent_error_state_outside_window", "RecentErrorStateOutsideWindow"],
        ["recent_error_state_run_mismatch", "RecentErrorStateRunMismatch"],
        ["recent_error_state_source_run_mismatch", "RecentErrorStateSourceRunMismatch"],
        ["strength_band_not_supported", "StrengthBandNotSupported"],
        ["unsupported_cohort", "UnsupportedCohort"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "TemperatureCanaryDecisionReasonValue"),
      [
        ["bundle_invalid", "BundleInvalid"],
        ["bundle_missing", "BundleMissing"],
        ["canary_expired", "CanaryExpired"],
        ["canary_killed", "CanaryKilled"],
        ["registry_inactive", "RegistryInactive"],
        ["registry_invalid", "RegistryInvalid"],
        ["policy_raw", "PolicyRaw"],
        ["invalid_forecast", "InvalidForecast"],
        ["inference_error", "InferenceError"],
        ["invalid_model", "InvalidModel"],
        ["invalid_recent_error_state", "InvalidRecentErrorState"],
        ["model_identity_mismatch", "ModelIdentityMismatch"],
        ["model_not_supported", "ModelNotSupported"],
        ["model_not_yet_available", "ModelNotYetAvailable"],
        ["outside_assumed_delay6_next12", "OutsideAssumedDelay6Next12"],
        ["outside_initialization_first12", "OutsideInitializationFirst12"],
        ["recent_error_state_as_of_mismatch", "RecentErrorStateAsOfMismatch"],
        ["recent_error_state_contains_future_data", "RecentErrorStateContainsFutureData"],
        ["recent_error_state_outside_window", "RecentErrorStateOutsideWindow"],
        ["recent_error_state_run_mismatch", "RecentErrorStateRunMismatch"],
        ["recent_error_state_source_run_mismatch", "RecentErrorStateSourceRunMismatch"],
        ["strength_band_not_supported", "StrengthBandNotSupported"],
        ["unsupported_cohort", "UnsupportedCohort"],
        ["missing_source_forecast", "MissingSourceForecast"],
        ["outside_operational_window", "OutsideOperationalWindow"],
        ["source_identity_mismatch", "SourceIdentityMismatch"],
        ["source_not_available", "SourceNotAvailable"],
        ["source_stale", "SourceStale"],
        ["source_time_mismatch", "SourceTimeMismatch"],
      ],
    );
    assert.match(
      rust,
      /pub type TemperatureCanaryDecisionReason = Nullable<TemperatureCanaryDecisionReasonValue>;/,
    );
  },
);

test(
  "projects the temperature canary decision and flat source forecast exactly",
  // retain the exact decision fields and reuse the configured base field identities
  () => {
    const { rust } = repositoryProjection();

    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastTemperatureCanaryDecisionV1"),
      [
        ["branch", "Nullable<ForecastTemperatureCanaryDecisionV1BranchEnum>"],
        ["bundleSha256", "Nullable<String>"],
        [
          "contractVersion",
          "ForecastTemperatureCanaryDecisionV1ContractVersionLiteral",
        ],
        ["correctedTemperatureC", "Nullable<f64>"],
        ["rawBestMatchTemperatureC", "Nullable<f64>"],
        ["reasonCode", "TemperatureCanaryDecisionReason"],
        ["recentErrorStateSha256", "Nullable<String>"],
        [
          "sourceForecast",
          "Nullable<ForecastTemperatureCanaryDecisionV1SourceForecast>",
        ],
        ["state", "ForecastTemperatureCanaryDecisionV1StateEnum"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ForecastTemperatureCanaryDecisionV1BranchEnum",
      ),
      [["adaptive", "Adaptive"], ["direct", "Direct"]],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ForecastTemperatureCanaryDecisionV1StateEnum",
      ),
      [
        ["active", "Active"],
        ["disabled", "Disabled"],
        ["raw_fallback", "RawFallback"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastTemperatureCanaryDecisionV1SourceForecast",
      ),
      [
        ["adapterVersion", "String"],
        ["dataset", "TemperatureCanarySourceForecastDatasetLiteral"],
        ["firstReceivedAt", "String"],
        ["modelCycle", "TemperatureCanarySourceForecastModelCycleEnum"],
        ["modelLeadHours", "u64"],
        ["providerKey", "TemperatureCanarySourceForecastProviderKeyLiteral"],
        ["providerResponseSha256", "String"],
        ["rawRelativeHumidityPercent", "Nullable<f64>"],
        ["rawTemperatureC", "f64"],
        ["rawWindSpeedMps", "Nullable<f64>"],
        ["runInitializedAt", "String"],
        ["upstreamModel", "TemperatureCanarySourceForecastUpstreamModelLiteral"],
        ["validAt", "String"],
        ["operationalHorizonHours", "u64"],
      ],
    );
    assert.match(
      rust,
      /pub struct ForecastTemperatureCanaryDecisionV1ContractVersionLiteral;\s*impl ForecastTemperatureCanaryDecisionV1ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-temperature-canary-decision\/v1";\s*\}/,
    );
    assert.match(
      rust,
      /wire rule: safe-unsigned-integer, range 0\.\.=9007199254740991"\]\s*pub operationalHorizonHours: u64,/,
    );
    const decisionDefinition = rust.match(
      /^pub struct ForecastTemperatureCanaryDecisionV1 \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(decisionDefinition);
    assert.equal(
      Array.from(decisionDefinition[0].matchAll(/wire rule: finite-f64/gmu)).length,
      2,
    );
    assert.equal(
      rust.match(/pub struct TemperatureCanarySourceForecast \{/gu)?.length,
      1,
    );
    assert.equal(
      rust.match(/pub struct ForecastTemperatureCanaryDecisionV1SourceForecast \{/gu)?.length,
      1,
    );
  },
);

test(
  "projects the fail-raw and active forecast adjustment decisions without widening",
  // retain exact decision identities and ordered metric storage
  () => {
    const { rust } = repositoryProjection();

    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentFailRawDecision"),
      [
        [
          "adjustedMetrics",
          "ForecastAdjustmentFailRawDecisionAdjustedMetricsRecord",
        ],
        [
          "appliedMetrics",
          "ForecastAdjustmentFailRawDecisionAppliedMetricsEmptyArray",
        ],
        [
          "contractVersion",
          "ForecastAdjustmentFailRawDecisionContractVersionLiteral",
        ],
        ["reasonCode", "ForecastAdjustmentReasonCode"],
        ["state", "ForecastAdjustmentFailRawDecisionStateEnum"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ForecastAdjustmentFailRawDecisionStateEnum"),
      [["disabled", "Disabled"], ["not_applicable", "NotApplicable"]],
    );
    assert.match(
      rust,
      /source shape: Readonly<Partial<Record<ForecastAdjustmentMetric, never>>>; wire shape: empty JSON object[\s\S]*pub struct ForecastAdjustmentFailRawDecisionAdjustedMetricsRecord;/,
    );
    assert.match(
      rust,
      /source shape: readonly \[\]; wire shape: empty JSON array[\s\S]*pub struct ForecastAdjustmentFailRawDecisionAppliedMetricsEmptyArray;/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentFailRawDecisionContractVersionLiteral;\s*impl ForecastAdjustmentFailRawDecisionContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-decision\/v1";\s*\}/,
    );

    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentActiveDecision"),
      [
        [
          "adjustedMetrics",
          "ForecastAdjustmentActiveDecisionAdjustedMetricsRecord",
        ],
        [
          "algorithmContractVersion",
          "ForecastAdjustmentActiveDecisionAlgorithmContractVersionLiteral",
        ],
        ["appliedMetrics", "Vec<ForecastAdjustmentMetric>"],
        ["candidateArtifactSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentActiveDecisionContractVersionLiteral",
        ],
        ["evaluationReportSha256", "String"],
        ["leadBand", "ForecastLeadBandKey"],
        ["qualificationReceiptSha256", "String"],
        [
          "rawForecastProvenance",
          "ForecastAdjustmentRawForecastProvenance",
        ],
        ["reasonCode", "ForecastAdjustmentActiveDecisionReasonCodeNull"],
        ["state", "ForecastAdjustmentActiveDecisionStateLiteral"],
      ],
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentActiveDecisionAdjustedMetricsRecord\(pub Vec<\(ForecastAdjustmentMetric, f64\)>\);/,
    );
    assert.match(
      rust,
      /source shape: Readonly<Partial<Record<ForecastAdjustmentMetric, number>>>; wire shape: JSON object/,
    );
    assert.match(
      rust,
      /producer validation: ordered unique keys and finite values/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentActiveDecisionAlgorithmContractVersionLiteral;\s*impl ForecastAdjustmentActiveDecisionAlgorithmContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "robust-hierarchical-median\/v1";\s*\}/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentActiveDecisionContractVersionLiteral;\s*impl ForecastAdjustmentActiveDecisionContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-decision\/v1";\s*\}/,
    );
    assert.match(
      rust,
      /#\[doc = "exact null wire literal"\][\s\S]*pub struct ForecastAdjustmentActiveDecisionReasonCodeNull;/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentActiveDecisionStateLiteral;\s*impl ForecastAdjustmentActiveDecisionStateLiteral \{\s*pub const WIRE_VALUE: &'static str = "active";\s*\}/,
    );

    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentWindCanaryActiveDecision"),
      [
        [
          "activationKind",
          "ForecastAdjustmentWindCanaryActiveDecisionActivationKindLiteral",
        ],
        [
          "adjustedMetrics",
          "ForecastAdjustmentWindCanaryActiveDecisionAdjustedMetricsRecord",
        ],
        [
          "algorithmContractVersion",
          "ForecastAdjustmentWindCanaryActiveDecisionAlgorithmContractVersionLiteral",
        ],
        ["appliedMetrics", "Vec<ForecastAdjustmentWindCanaryMetric>"],
        ["authorizationSha256", "String"],
        ["candidateArtifactSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentWindCanaryActiveDecisionContractVersionLiteral",
        ],
        ["leadBand", "ForecastLeadBandKey"],
        [
          "rawForecastProvenance",
          "ForecastAdjustmentRawForecastProvenance",
        ],
        [
          "reasonCode",
          "ForecastAdjustmentWindCanaryActiveDecisionReasonCodeNull",
        ],
        ["state", "ForecastAdjustmentWindCanaryActiveDecisionStateLiteral"],
        ["transferReportSha256", "String"],
      ],
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentWindCanaryActiveDecisionAdjustedMetricsRecord\(pub Vec<\(ForecastAdjustmentWindCanaryMetric, f64\)>\);/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentWindCanaryActiveDecisionActivationKindLiteral;\s*impl ForecastAdjustmentWindCanaryActiveDecisionActivationKindLiteral \{\s*pub const WIRE_VALUE: &'static str = "wind_transfer_canary";\s*\}/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentWindCanaryActiveDecisionAlgorithmContractVersionLiteral;\s*impl ForecastAdjustmentWindCanaryActiveDecisionAlgorithmContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "robust-hierarchical-median\/v1";\s*\}/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentWindCanaryActiveDecisionContractVersionLiteral;\s*impl ForecastAdjustmentWindCanaryActiveDecisionContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-decision\/v1";\s*\}/,
    );
    assert.match(
      rust,
      /#\[doc = "exact null wire literal"\][\s\S]*pub struct ForecastAdjustmentWindCanaryActiveDecisionReasonCodeNull;/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentWindCanaryActiveDecisionStateLiteral;\s*impl ForecastAdjustmentWindCanaryActiveDecisionStateLiteral \{\s*pub const WIRE_VALUE: &'static str = "active";\s*\}/,
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindQualifiedMaintenanceDecision",
      ),
      [
        ["actionSha256", "String"],
        [
          "activationKind",
          "ForecastAdjustmentWindQualifiedMaintenanceDecisionActivationKindLiteral",
        ],
        [
          "adjustedMetrics",
          "ForecastAdjustmentWindQualifiedMaintenanceDecisionAdjustedMetricsRecord",
        ],
        [
          "algorithmContractVersion",
          "ForecastAdjustmentWindQualifiedMaintenanceDecisionAlgorithmContractVersionLiteral",
        ],
        ["appliedMetrics", "Vec<ForecastAdjustmentWindCanaryMetric>"],
        ["candidateArtifactSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentWindQualifiedMaintenanceDecisionContractVersionLiteral",
        ],
        ["fullMemberRootSha256", "String"],
        ["leadBand", "ForecastLeadBandKey"],
        ["policyReportSha256", "String"],
        [
          "rawForecastProvenance",
          "ForecastAdjustmentRawForecastProvenance",
        ],
        [
          "reasonCode",
          "ForecastAdjustmentWindQualifiedMaintenanceDecisionReasonCodeNull",
        ],
        [
          "state",
          "ForecastAdjustmentWindQualifiedMaintenanceDecisionStateLiteral",
        ],
      ],
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentWindQualifiedMaintenanceDecisionAdjustedMetricsRecord\(pub Vec<\(ForecastAdjustmentMetric, f64\)>\);/,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentWindQualifiedMaintenanceDecisionActivationKindLiteral;\s*impl ForecastAdjustmentWindQualifiedMaintenanceDecisionActivationKindLiteral \{\s*pub const WIRE_VALUE: &'static str = "maintenance_qualified";\s*\}/,
    );
    assert.match(
      rust,
      /#\[doc = "exact null wire literal"\][\s\S]*pub struct ForecastAdjustmentWindQualifiedMaintenanceDecisionReasonCodeNull;/,
    );
    assert.doesNotMatch(
      rust,
      /pub (?:adjustedMetrics|appliedMetrics): (?:OrderedFiniteNumberRecord|Vec<\(String, f64\)>)/,
    );
    const decisionDefinition = rust.match(
      /^pub enum ForecastAdjustmentDecision \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(decisionDefinition);
    assert.equal(
      decisionDefinition[0],
      `pub enum ForecastAdjustmentDecision {
    Active(ForecastAdjustmentActiveDecision),
    WindCanaryActive(ForecastAdjustmentWindCanaryActiveDecision),
    FailRaw(ForecastAdjustmentFailRawDecision),
}`,
    );
    assert.doesNotMatch(
      rust,
      /impl ForecastAdjustmentDecision \{[\s\S]*(?:DISCRIMINATOR_FIELD|WIRE_VALUES)/,
    );
    assert.equal(
      Array.from(rust.matchAll(/^pub struct ForecastAdjustmentActiveDecision \{/gmu)).length,
      1,
    );
    assert.equal(
      Array.from(rust.matchAll(/^pub struct ForecastAdjustmentWindCanaryActiveDecision \{/gmu)).length,
      1,
    );
    assert.equal(
      Array.from(rust.matchAll(/^pub struct ForecastAdjustmentFailRawDecision \{/gmu)).length,
      1,
    );
  },
);

test(
  "projects public weather records from the producer declarations",
  // verify the server payload shape rather than the browser's permissive reader
  () => {
    const { rust } = repositoryProjection();

    // compare one generated struct's exact field names and types
    function assertFields(name, expected) {
      const block = rust.match(new RegExp("^pub struct " + name + " \\{\\n([\\s\\S]+?)^\\}", "mu"));
      assert.ok(block, name);
      const fields = Array.from(
        block[1].matchAll(/^    pub (\w+): (.+),$/gmu),
        // return one field and its complete type
        ([, field, type]) => [field, type],
      );
      assert.equal(fields.length, Object.keys(expected).length, name);
      assert.deepEqual(Object.fromEntries(fields), expected, name);
    }

    assertFields("ApiDailyPrecipitation", {
      accumulationMm: "f64",
      source: "ApiDailyPrecipitationSource",
      validThrough: "String",
    });
    assertFields("ApiDailyPrecipitationSource", { sourceId: "String", stationSlug: "String" });
    assertFields("ApiTideRecord", {
      eventType: "Nullable<ApiTideRecordEventTypeEnum>",
      kind: "ApiTideRecordKindEnum",
      source: "ApiTideRecordSource",
      validAt: "String",
      waterLevelM: "f64",
    });
    assertFields("ApiTideRecordSource", {
      attribution: "ApiTideRecordSourceAttribution",
      providerKey: "String",
      stationName: "String",
      stationSlug: "String",
    });
    assertFields("ApiTideRecordSourceAttribution", { label: "String", url: "String" });
    assertFields("ApiTrendPoint", { metrics: "ApiTrendPointMetrics", validAt: "String" });
    assertFields("ApiTrendPointMetrics", {
      apparentTemperatureC: "Nullable<f64>",
      pm25MicrogramsPerCubicMeter: "Nullable<f64>",
      precipitationMm: "Nullable<f64>",
      pressureHpa: "Nullable<f64>",
      relativeHumidityPercent: "Nullable<f64>",
      temperatureC: "Nullable<f64>",
      temperatureMaximumC: "Nullable<f64>",
      temperatureMinimumC: "Nullable<f64>",
      windDirectionDegrees: "Nullable<f64>",
      windGustMps: "Nullable<f64>",
      windSpeedMps: "Nullable<f64>",
    });
    assert.match(
      rust,
      /impl ApiTideRecordEventTypeEnum \{\s*pub const WIRE_VALUES: &'static \[\(&'static str, Self\)\] = &\[\s*\("high", Self::High\),\s*\("low", Self::Low\),\s*\];\s*\}/,
    );
    assert.match(
      rust,
      /impl ApiTideRecordKindEnum \{\s*pub const WIRE_VALUES: &'static \[\(&'static str, Self\)\] = &\[\s*\("observation", Self::Observation\),\s*\("prediction", Self::Prediction\),\s*\];\s*\}/,
    );
    assert.match(
      rust,
      /#\[doc = "wire rule: utc-iso, canonical UTC ISO-8601 string"\]\s*pub validThrough: String,/,
    );
    assert.match(
      rust,
      /#\[doc = "wire rule: finite-f64, NaN and infinities excluded"\]\s*pub pm25MicrogramsPerCubicMeter: Nullable<f64>,/,
    );
  },
);

test(
  "preserves every hourly PM2.5 slot from the current-weather producer type",
  // retain the element nullability and canonical timestamp metadata
  () => {
    const { rust } = repositoryProjection();
    assert.match(
      rust,
      /pub struct ApiAirQualityHourlyContext \{\s*#\[doc = "wire rule: finite-f64, NaN and infinities excluded"\]\s*pub hourlyPm25: Vec<Nullable<f64>>,\s*#\[doc = "wire rule: utc-iso, canonical UTC ISO-8601 string"\]\s*pub through: String,\s*\}/,
    );
  },
);

test(
  "projects the producer's ordered finite property-sensor readings",
  // preserve optional air quality, channel nullability, and insertion-ordered readings
  () => {
    const { rust } = repositoryProjection();
    assert.match(
      rust,
      /#\[doc = "wire rule: ordered-finite-f64-record, ordered unique string keys and finite values"\]\s*#\[derive\(Debug, Clone, PartialEq\)\]\s*pub struct OrderedFiniteNumberRecord\(pub Vec<\(String, f64\)>\);/,
    );
    assert.match(
      rust,
      /pub struct ApiPropertySensorSnapshot \{\s*pub airQuality: FieldPresence<Nullable<ApiAirQualityHourlyContext>>,\s*#\[doc = "wire rule: safe-unsigned-integer, range 0\.\.=9007199254740991"\]\s*pub channel: Nullable<u64>,\s*pub key: String,\s*pub model: String,\s*#\[doc = "wire rule: ordered-finite-f64-record, ordered unique string keys and finite values"\]\s*pub readings: OrderedFiniteNumberRecord,\s*\}/,
    );
  },
);

test(
  "projects domain type imports through the exact public package graph",
  // preserve producer field shapes while reusing canonical domain enums
  () => {
    const { rust } = repositoryProjection();

    // read one complete generated field map without depending on helper order
    function fields(name) {
      const block = rust.match(new RegExp("^pub struct " + name + " \\{\\n([\\s\\S]+?)^\\}", "mu"));
      assert.ok(block, name);
      return Object.fromEntries(Array.from(
        block[1].matchAll(/^    pub (\w+): (.+),$/gmu),
        // retain the exact field and nominal type
        ([, field, type]) => [field, type],
      ));
    }

    assert.match(
      rust,
      /impl SourceKind \{\s*pub const WIRE_VALUES: &'static \[\(&'static str, Self\)\] = &\[\s*\("physical_sensor", Self::PhysicalSensor\),\s*\("model_current", Self::ModelCurrent\),\s*\("reanalysis", Self::Reanalysis\),\s*\("forecast", Self::Forecast\),\s*\("tide_observation", Self::TideObservation\),\s*\("tide_prediction", Self::TidePrediction\),\s*\];\s*\}/,
    );
    assert.match(
      rust,
      /impl StationKind \{\s*pub const WIRE_VALUES: &'static \[\(&'static str, Self\)\] = &\[\s*\("physical", Self::Physical\),\s*\("virtual", Self::Virtual\),\s*\];\s*\}/,
    );
    assert.deepEqual(fields("ApiSource"), {
      attribution: "ApiSourceAttribution",
      id: "String",
      key: "String",
      kind: "SourceKind",
      providerKey: "String",
      providerName: "String",
      provenanceLabel: "String",
    });
    assert.deepEqual(fields("ApiSourceAttribution"), {
      label: "String",
      url: "String",
    });
    assert.deepEqual(fields("ApiStation"), {
      kind: "StationKind",
      latitude: "f64",
      longitude: "f64",
      name: "String",
      slug: "String",
      sources: "Vec<ApiSource>",
    });
    assert.deepEqual(fields("ApiSite"), {
      latitude: "f64",
      longitude: "f64",
      name: "String",
      slug: "String",
      stations: "Vec<ApiStation>",
      timezone: "String",
    });
    assert.deepEqual(fields("ApiWeatherRecord"), {
      freshness: "ApiWeatherRecordFreshness",
      id: "String",
      metadata: "ApiWeatherRecordMetadata",
      metrics: "ApiWeatherRecordMetrics",
      productRunAt: "Nullable<String>",
      provenance: "ApiWeatherRecordProvenance",
      receivedAt: "String",
      revisionCount: "u64",
      validAt: "String",
    });
    assert.deepEqual(fields("ApiWeatherRecordFreshness"), {
      ageSeconds: "u64",
      label: "String",
      status: "ApiWeatherRecordFreshnessStatusEnum",
    });
    assert.deepEqual(fields("ApiWeatherRecordMetadata"), {
      device: "Nullable<ApiWeatherRecordMetadataDevice>",
      provider: "Nullable<ApiWeatherRecordMetadataProvider>",
      quality: "Nullable<ApiWeatherRecordMetadataQuality>",
      upstream: "ApiWeatherRecordMetadataUpstream",
    });
    assert.deepEqual(fields("ApiWeatherRecordMetadataProvider"), {
      dataset: "Nullable<String>",
      elevationM: "Nullable<f64>",
      gridCell: "Nullable<String>",
      propertySensors: "Nullable<Vec<ApiPropertySensorSnapshot>>",
    });
    assert.deepEqual(fields("ApiWeatherRecordProvenance"), {
      attribution: "ApiWeatherRecordProvenanceAttribution",
      label: "String",
      providerKey: "String",
      sourceId: "String",
      sourceKey: "String",
      sourceKind: "SourceKind",
      stationSlug: "String",
    });
    assert.deepEqual(fields("ApiWeatherRecordMetrics"), {
      apparentTemperatureC: "Nullable<f64>",
      blackGlobeTemperatureC: "Nullable<f64>",
      cloudCoverPercent: "Nullable<f64>",
      pm25MicrogramsPerCubicMeter: "Nullable<f64>",
      precipitationMm: "Nullable<f64>",
      precipitationRateMmPerHour: "Nullable<f64>",
      pressureHpa: "Nullable<f64>",
      relativeHumidityPercent: "Nullable<f64>",
      soilElectricalConductivityMicrosiemensPerCm: "Nullable<f64>",
      soilMoisturePercent: "Nullable<f64>",
      solarRadiationWm2: "Nullable<f64>",
      temperatureC: "Nullable<f64>",
      uvIndex: "Nullable<f64>",
      windDirectionDegrees: "Nullable<f64>",
      windGustMps: "Nullable<f64>",
      windSpeedMps: "Nullable<f64>",
      wetBulbGlobeTemperatureC: "Nullable<f64>",
    });
    assert.match(
      rust,
      /#\[doc = "wire rule: utc-iso, canonical UTC ISO-8601 string"\]\s*pub productRunAt: Nullable<String>,/,
    );
    assert.match(
      rust,
      /#\[doc = "wire rule: utc-iso, canonical UTC ISO-8601 string"\]\s*pub receivedAt: String,/,
    );
    assert.match(
      rust,
      /#\[doc = "wire rule: utc-iso, canonical UTC ISO-8601 string"\]\s*pub validAt: String,/,
    );
  },
);

test(
  "flattens the current-weather producer onto its exact common API base",
  // preserve the public top-level payload order without inventing a base field
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "ApiCurrentWeatherRecord"), [
      ["freshness", "ApiWeatherRecordFreshness"],
      ["id", "String"],
      ["metadata", "ApiWeatherRecordMetadata"],
      ["metrics", "ApiWeatherRecordMetrics"],
      ["productRunAt", "Nullable<String>"],
      ["provenance", "ApiWeatherRecordProvenance"],
      ["receivedAt", "String"],
      ["revisionCount", "u64"],
      ["validAt", "String"],
      ["airQuality", "Nullable<ApiAirQualityHourlyContext>"],
      ["pressureChange3hHpa", "Nullable<f64>"],
    ]);
    assert.equal(
      rust.match(/pub struct ApiWeatherRecordMetadata \{/gu)?.length,
      1,
    );
    assert.equal(
      rust.match(/pub struct ApiWeatherRecordMetrics \{/gu)?.length,
      1,
    );
    assert.equal(
      rust.match(/pub struct ApiWeatherRecordFreshness \{/gu)?.length,
      1,
    );
    assert.equal(
      rust.match(/pub struct ApiWeatherRecordProvenance \{/gu)?.length,
      1,
    );
    assert.doesNotMatch(rust, /pub base:/u);
    const currentWeather = manifest.roots.find(
      // select the producer independently of later catalog roots
      (root) =>
        root.source === "packages/contracts/src/weather-records.ts" &&
        root.exportName === "ApiCurrentWeatherRecord",
    );
    assert.deepEqual(currentWeather, {
      source: "packages/contracts/src/weather-records.ts",
      exportName: "ApiCurrentWeatherRecord",
      rustName: "ApiCurrentWeatherRecord",
      kind: "readonly-interface",
      base: "ApiWeatherRecord",
    });
  },
);

test(
  "projects the private API adjustment union and complete forecast row",
  // retain public forecast field order without inventing a union discriminator
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const decisionDefinition = rust.match(
      /^pub enum ApiForecastAdjustmentDecision \{\n[\s\S]+?^\}/mu,
    );

    assert.ok(decisionDefinition);
    assert.equal(
      decisionDefinition[0],
      `pub enum ApiForecastAdjustmentDecision {
    Domain(ForecastAdjustmentDecision),
    MaintenanceQualified(ForecastAdjustmentWindQualifiedMaintenanceDecision),
}`,
    );
    assert.doesNotMatch(
      rust,
      /impl ApiForecastAdjustmentDecision \{[\s\S]*(?:DISCRIMINATOR_FIELD|WIRE_VALUES)/,
    );
    assert.deepEqual(rustStructFieldEntries(rust, "ApiForecastWeatherRecord"), [
      ["freshness", "ApiWeatherRecordFreshness"],
      ["id", "String"],
      ["metadata", "ApiWeatherRecordMetadata"],
      ["metrics", "ApiWeatherRecordMetrics"],
      ["productRunAt", "Nullable<String>"],
      ["provenance", "ApiWeatherRecordProvenance"],
      ["receivedAt", "String"],
      ["revisionCount", "u64"],
      ["validAt", "String"],
      ["rainAdjustment", "ApiRainAdjustmentDecision"],
      ["adjustment", "ApiForecastAdjustmentDecision"],
      ["temperatureAdjustment", "ForecastTemperatureCanaryDecisionV1"],
    ]);
    assert.equal(
      rust.match(/pub struct ApiWeatherRecordMetadata \{/gu)?.length,
      1,
    );
    assert.equal(
      rust.match(/pub struct ApiWeatherRecordMetrics \{/gu)?.length,
      1,
    );
    assert.deepEqual(
      manifest.roots.filter(
        // select the two forecast producer roots independently of later additions
        (root) => root.source === "packages/contracts/src/forecast.ts" &&
          (root.exportName === "ApiForecastAdjustmentDecision" ||
            root.exportName === "ApiForecastWeatherRecord"),
      ),
      [
        {
          source: "packages/contracts/src/forecast.ts",
          exportName: "ApiForecastAdjustmentDecision",
          rustName: "ApiForecastAdjustmentDecision",
          kind: "named-readonly-object-union",
          importedObjectUnion: true,
          objectUnion: {
            variants: [
              {
                rustName: "Domain",
                payloadRustName: "ForecastAdjustmentDecision",
              },
              {
                rustName: "MaintenanceQualified",
                payloadRustName:
                  "ForecastAdjustmentWindQualifiedMaintenanceDecision",
              },
            ],
          },
        },
        {
          source: "packages/contracts/src/forecast.ts",
          exportName: "ApiForecastWeatherRecord",
          rustName: "ApiForecastWeatherRecord",
          kind: "readonly-interface",
          base: "ApiWeatherRecord",
          importedBase: true,
        },
      ],
    );
  },
);

test(
  "projects the exact public forecast adjustment runtime status",
  // retain the bounded startup contract without widening internal diagnostics
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiForecastAdjustmentRuntime"),
      [
        ["actionSha256", "FieldPresence<String>"],
        [
          "activationMode",
          "Nullable<ApiForecastAdjustmentRuntimeActivationModeEnum>",
        ],
        ["activeBundle", "Nullable<String>"],
        ["authorizationSha256", "Nullable<String>"],
        ["candidateArtifactSha256", "Nullable<String>"],
        ["enabledMetrics", "Vec<ForecastAdjustmentMetric>"],
        ["evaluationReportSha256", "Nullable<String>"],
        ["expiresAt", "Nullable<String>"],
        ["fullMemberRootSha256", "FieldPresence<String>"],
        ["loadedAt", "String"],
        ["qualificationReceiptSha256", "Nullable<String>"],
        [
          "reasonCode",
          "Nullable<ApiForecastAdjustmentRuntimeReasonCodeEnum>",
        ],
        ["state", "ApiForecastAdjustmentRuntimeStateEnum"],
        ["policyReportSha256", "FieldPresence<String>"],
        ["transferReportSha256", "Nullable<String>"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ApiForecastAdjustmentRuntimeActivationModeEnum",
      ),
      [
        ["maintenance_qualified", "MaintenanceQualified"],
        ["qualified", "Qualified"],
        ["wind_canary", "WindCanary"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ApiForecastAdjustmentRuntimeReasonCodeEnum",
      ),
      [
        ["bundle_invalid", "BundleInvalid"],
        ["bundle_missing", "BundleMissing"],
        ["registry_inactive", "RegistryInactive"],
        ["registry_invalid", "RegistryInvalid"],
        ["canary_expired", "CanaryExpired"],
        ["canary_killed", "CanaryKilled"],
        ["policy_raw", "PolicyRaw"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ApiForecastAdjustmentRuntimeStateEnum"),
      [["active", "Active"], ["disabled", "Disabled"]],
    );
    assert.match(
      rust,
      /#\[doc = "wire rule: utc-iso, canonical UTC ISO-8601 string"\]\s*pub loadedAt: String,/,
    );
    const runtimeRoot = manifest.roots.find(
      // select the exact public runtime root
      (root) => root.exportName === "ApiForecastAdjustmentRuntime",
    );
    assert.deepEqual(runtimeRoot, {
      source: "packages/contracts/src/forecast.ts",
      exportName: "ApiForecastAdjustmentRuntime",
      rustName: "ApiForecastAdjustmentRuntime",
      kind: "readonly-interface",
    });
  },
);

test(
  "projects the exact public temperature adjustment runtime status",
  // retain required nullability and nested collector state without widening
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiForecastTemperatureAdjustmentRuntime"),
      [
        ["actionSha256", "Nullable<String>"],
        [
          "activationMode",
          "Nullable<ApiForecastTemperatureAdjustmentRuntimeActivationModeEnum>",
        ],
        ["activeBundle", "Nullable<String>"],
        ["authorizationSha256", "Nullable<String>"],
        ["expiresAt", "Nullable<String>"],
        ["fullMemberRootSha256", "Nullable<String>"],
        ["loadedAt", "String"],
        ["policyReportSha256", "Nullable<String>"],
        [
          "reasonCode",
          "Nullable<ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum>",
        ],
        [
          "source",
          "Nullable<ApiForecastTemperatureAdjustmentRuntimeSource>",
        ],
        ["state", "ApiForecastTemperatureAdjustmentRuntimeStateEnum"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ApiForecastTemperatureAdjustmentRuntimeSource",
      ),
      [
        ["adaptiveReady", "bool"],
        ["firstReceivedAt", "String"],
        ["hourCount", "u64"],
        ["latestRunInitializedAt", "String"],
        ["stateReason", "Nullable<String>"],
        [
          "stateStatus",
          "ApiForecastTemperatureAdjustmentRuntimeSourceStateStatusEnum",
        ],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ApiForecastTemperatureAdjustmentRuntimeActivationModeEnum",
      ),
      [
        ["maintenance_qualified", "MaintenanceQualified"],
        ["temperature_canary", "TemperatureCanary"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum",
      ),
      [
        ["bundle_invalid", "BundleInvalid"],
        ["bundle_missing", "BundleMissing"],
        ["canary_expired", "CanaryExpired"],
        ["canary_killed", "CanaryKilled"],
        ["registry_inactive", "RegistryInactive"],
        ["registry_invalid", "RegistryInvalid"],
        ["policy_raw", "PolicyRaw"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ApiForecastTemperatureAdjustmentRuntimeSourceStateStatusEnum",
      ),
      [
        ["supported", "Supported"],
        ["cold", "Cold"],
        ["insufficient", "Insufficient"],
        ["invalid", "Invalid"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ApiForecastTemperatureAdjustmentRuntimeStateEnum",
      ),
      [["active", "Active"], ["disabled", "Disabled"]],
    );
    const runtimeDefinition = rust.match(
      /^pub struct ApiForecastTemperatureAdjustmentRuntime \{\n[\s\S]+?^\}/mu,
    );
    const sourceDefinition = rust.match(
      /^pub struct ApiForecastTemperatureAdjustmentRuntimeSource \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(runtimeDefinition);
    assert.ok(sourceDefinition);
    assert.doesNotMatch(runtimeDefinition[0], /FieldPresence/u);
    assert.doesNotMatch(sourceDefinition[0], /FieldPresence/u);
    assert.equal(
      Array.from(
        sourceDefinition[0].matchAll(
          /wire rule: utc-iso, canonical UTC ISO-8601 string/gmu,
        ),
      ).length,
      2,
    );
    assert.match(
      sourceDefinition[0],
      /wire rule: safe-unsigned-integer, range 0\.\.=9007199254740991/,
    );
    assert.match(
      runtimeDefinition[0],
      /wire rule: utc-iso, canonical UTC ISO-8601 string/,
    );
    const runtimeRoot = manifest.roots.find(
      // select the exact public temperature runtime root
      (root) => root.exportName === "ApiForecastTemperatureAdjustmentRuntime",
    );
    assert.deepEqual(runtimeRoot, {
      source: "packages/contracts/src/forecast.ts",
      exportName: "ApiForecastTemperatureAdjustmentRuntime",
      rustName: "ApiForecastTemperatureAdjustmentRuntime",
      kind: "readonly-interface",
    });
  },
);

test(
  "projects the original forecast response and rain runtime contracts",
  // keep the producer response exact without adopting the normalized browser type
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const apiForecastDays = SOURCES["packages/contracts/src/forecast.ts"].match(
      /^export type ForecastDays = ([^;]+);$/mu,
    );
    const webForecastDays = readFileSync(
      new URL("../apps/web/src/index.ts", import.meta.url),
      "utf8",
    ).match(/^export type ForecastDays = ([^;]+);$/mu);

    assert.ok(apiForecastDays);
    assert.ok(webForecastDays);
    assert.equal(apiForecastDays[1], "1 | 5 | 10");
    assert.equal(webForecastDays[1], apiForecastDays[1]);
    assert.equal(
      SOURCES["apps/api/src/index.ts"].match(/satisfies ApiForecastResponse/gu)?.length,
      1,
    );
    assert.match(
      SOURCES["apps/api/src/rain-adjustment.ts"],
      /export function rainAdjustmentRuntime\([\s\S]+?\): ApiRainAdjustmentRuntime \{/u,
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiRainAdjustmentRuntime"),
      [
        ["state", "ApiRainAdjustmentRuntimeStateEnum"],
        ["activeBundle", "Nullable<String>"],
        ["reasonCode", "Nullable<ApiRainAdjustmentRuntimeReasonCodeEnum>"],
        ["loadedAt", "String"],
        ["source", "Nullable<ApiRainAdjustmentRuntimeSource>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiRainAdjustmentRuntimeSource"),
      [
        ["runInitializedAt", "String"],
        ["firstReceivedAt", "String"],
        ["decisionAt", "String"],
        ["hourCount", "u64"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ApiRainAdjustmentRuntimeStateEnum"),
      [["active", "Active"], ["disabled", "Disabled"]],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ApiRainAdjustmentRuntimeReasonCodeEnum",
      ),
      [
        ["policy_raw", "PolicyRaw"],
        ["registry_invalid", "RegistryInvalid"],
        ["model_unavailable", "ModelUnavailable"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiForecastResponse"),
      [
        ["adjustmentRuntime", "ApiForecastAdjustmentRuntime"],
        ["airQualityContext", "Vec<ApiForecastWeatherRecord>"],
        ["rainAdjustmentRuntime", "ApiRainAdjustmentRuntime"],
        ["data", "Vec<ApiForecastWeatherRecord>"],
        ["days", "ForecastDays"],
        ["generatedAt", "String"],
        ["pressureContext", "Vec<ApiWeatherRecord>"],
        ["site", "ApiSite"],
        [
          "temperatureAdjustmentRuntime",
          "ApiForecastTemperatureAdjustmentRuntime",
        ],
      ],
    );
    const rainRuntimeDefinition = rust.match(
      /^pub struct ApiRainAdjustmentRuntime \{\n[\s\S]+?^\}/mu,
    );
    const rainSourceDefinition = rust.match(
      /^pub struct ApiRainAdjustmentRuntimeSource \{\n[\s\S]+?^\}/mu,
    );
    const responseDefinition = rust.match(
      /^pub struct ApiForecastResponse \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(rainRuntimeDefinition);
    assert.ok(rainSourceDefinition);
    assert.ok(responseDefinition);
    assert.doesNotMatch(rainRuntimeDefinition[0], /FieldPresence/u);
    assert.doesNotMatch(rainSourceDefinition[0], /FieldPresence/u);
    assert.doesNotMatch(responseDefinition[0], /FieldPresence/u);
    assert.doesNotMatch(rainRuntimeDefinition[0], /wire rule: utc-iso/u);
    assert.doesNotMatch(rainSourceDefinition[0], /wire rule: utc-iso/u);
    assert.match(
      rainSourceDefinition[0],
      /wire rule: safe-unsigned-integer, range 0\.\.=9007199254740991/u,
    );
    assert.doesNotMatch(responseDefinition[0], /wire rule: utc-iso/u);
    const rainRuntimeRoot = manifest.roots.find(
      // select the exact producer root independently of later additions
      (root) => root.source === "packages/contracts/src/forecast.ts" &&
        root.exportName === "ApiRainAdjustmentRuntime",
    );
    const forecastResponseRoot = manifest.roots.find(
      // select the exact producer root independently of later additions
      (root) => root.source === "packages/contracts/src/forecast.ts" &&
        root.exportName === "ApiForecastResponse",
    );
    assert.deepEqual([rainRuntimeRoot, forecastResponseRoot], [
      {
        source: "packages/contracts/src/forecast.ts",
        exportName: "ApiRainAdjustmentRuntime",
        rustName: "ApiRainAdjustmentRuntime",
        kind: "readonly-interface",
      },
      {
        source: "packages/contracts/src/forecast.ts",
        exportName: "ApiForecastResponse",
        rustName: "ApiForecastResponse",
        kind: "readonly-interface",
      },
    ]);
  },
);

test(
  "projects every named public response envelope from its producer declaration",
  // keep the Rust declarations tied to the exact server-side response checks
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const apiSource = SOURCES["apps/api/src/index.ts"];

    // require one producer check for every ordinary public response envelope
    for (const name of [
      "ApiSitesResponse",
      "ApiRainCollectionResponse",
      "ApiCurrentWeatherResponse",
      "ApiDailyPrecipitationResponse",
      "ApiTrendsResponse",
      "ApiTidesResponse",
      "ApiHistoryResponse",
    ]) {
      assert.equal(
        apiSource.match(new RegExp(`satisfies ${name}`, "gu"))?.length,
        1,
        name,
      );
    }
    assert.equal(
      apiSource.match(/const body: ApiHealthResponse =/gu)?.length,
      1,
    );
    assert.equal(
      apiSource.match(/satisfies ApiErrorResponse/gu)?.length,
      2,
    );

    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiSitesResponse"),
      [["data", "Vec<ApiSite>"]],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiHealthResponse"),
      [["data", "ApiHealthResponseData"]],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiHealthResponseData"),
      [
        ["adjustmentRuntime", "ApiForecastAdjustmentRuntime"],
        [
          "temperatureAdjustmentRuntime",
          "ApiForecastTemperatureAdjustmentRuntime",
        ],
        ["database", "ApiHealthResponseDataDatabaseEnum"],
        ["live", "ApiHealthResponseDataLiveLiteral"],
        ["migration", "ApiHealthResponseDataMigration"],
        ["ready", "bool"],
        ["version", "String"],
        ["worker", "ApiHealthResponseDataWorker"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiHealthResponseDataMigration"),
      [
        ["status", "ApiHealthResponseDataMigrationStatusEnum"],
        ["version", "Nullable<String>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiHealthResponseDataWorker"),
      [["freshness", "ApiHealthResponseDataWorkerFreshnessEnum"]],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ApiHealthResponseDataDatabaseEnum"),
      [["ready", "Ready"], ["unavailable", "Unavailable"]],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ApiHealthResponseDataMigrationStatusEnum",
      ),
      [
        ["current", "Current"],
        ["outdated", "Outdated"],
        ["unavailable", "Unavailable"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ApiHealthResponseDataWorkerFreshnessEnum",
      ),
      [["fresh", "Fresh"], ["stale", "Stale"], ["unknown", "Unknown"]],
    );
    assert.match(
      rust,
      /impl ApiHealthResponseDataLiveLiteral \{\s*pub const WIRE_VALUE: bool = true;\s*\}/u,
    );

    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiRainCollectionResponse"),
      [
        ["data", "ApiRainCollectionResponseData"],
        ["generatedAt", "String"],
        ["version", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiRainCollectionResponseData"),
      [
        [
          "contractVersion",
          "ApiRainCollectionResponseDataContractVersionLiteral",
        ],
        ["modelEnabled", "bool"],
        [
          "qualificationEnabled",
          "ApiRainCollectionResponseDataQualificationEnabledLiteral",
        ],
        ["stationAccessAuthorized", "bool"],
        ["expectedStations", "u64"],
        ["startsAt", "String"],
        ["expiresAt", "String"],
        ["claims", "u64"],
        ["receipts", "u64"],
        ["validForecasts", "u64"],
        ["timelyForecasts", "u64"],
        ["validStationWindows", "u64"],
        ["stationsSeen", "u64"],
        ["failedRequests", "u64"],
        ["pendingRequests", "u64"],
        ["unknownRequests", "u64"],
        ["compressedBytes", "u64"],
        ["lastClaimAt", "Nullable<String>"],
        ["lastReceiptAt", "Nullable<String>"],
        ["lastForecastReceiptAt", "Nullable<String>"],
        ["lastStationReceiptAt", "Nullable<String>"],
        ["pausedUntil", "Nullable<String>"],
      ],
    );
    assert.match(
      rust,
      /impl ApiRainCollectionResponseDataContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "rain-prospective-capture\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ApiRainCollectionResponseDataQualificationEnabledLiteral \{\s*pub const WIRE_VALUE: bool = false;\s*\}/u,
    );
    // retain all producer-validated rain counts as safe wire integers
    for (const field of [
      "expectedStations",
      "claims",
      "receipts",
      "validForecasts",
      "timelyForecasts",
      "validStationWindows",
      "stationsSeen",
      "failedRequests",
      "pendingRequests",
      "unknownRequests",
      "compressedBytes",
    ]) {
      assert.match(
        rustStructFieldBlock(rust, "ApiRainCollectionResponseData", field),
        /wire rule: safe-unsigned-integer/u,
      );
    }
    // retain canonical UTC metadata on every rain response timestamp
    for (const field of [
      "startsAt",
      "expiresAt",
      "lastClaimAt",
      "lastReceiptAt",
      "lastForecastReceiptAt",
      "lastStationReceiptAt",
      "pausedUntil",
    ]) {
      assert.match(
        rustStructFieldBlock(rust, "ApiRainCollectionResponseData", field),
        /wire rule: utc-iso/u,
      );
    }

    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiCurrentWeatherResponse"),
      [
        ["data", "Vec<ApiCurrentWeatherRecord>"],
        ["generatedAt", "String"],
        ["site", "ApiSite"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiDailyPrecipitationResponse"),
      [
        ["data", "Nullable<ApiDailyPrecipitation>"],
        ["generatedAt", "String"],
        ["site", "ApiSite"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiTrendsResponse"),
      [
        ["data", "Vec<ApiTrendPoint>"],
        ["generatedAt", "String"],
        ["site", "ApiSite"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiTidesResponse"),
      [
        ["data", "Vec<ApiTideRecord>"],
        ["generatedAt", "String"],
        ["site", "ApiSite"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiHistoryResponse"),
      [
        ["data", "Vec<ApiWeatherRecord>"],
        ["generatedAt", "String"],
        ["page", "ApiHistoryResponsePage"],
        ["site", "ApiSite"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiHistoryResponsePage"),
      [["limit", "u64"], ["nextCursor", "Nullable<String>"]],
    );
    assert.match(
      rustStructFieldBlock(rust, "ApiHistoryResponsePage", "limit"),
      /wire rule: safe-unsigned-integer/u,
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiErrorResponse"),
      [["error", "ApiErrorResponseError"]],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiErrorResponseError"),
      [["code", "String"], ["message", "String"]],
    );

    assertManifestRootGroup(manifest.roots, PUBLIC_API_RESPONSE_ROOTS);
  },
);

test(
  "projects the existing api diagnostic log contract",
  // preserve the producer's optional operation and bounded status representation
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const apiSource = SOURCES["apps/api/src/index.ts"];

    assert.equal(
      apiSource.match(/export type TemperatureCanaryOperation =/gu)?.length,
      1,
    );
    assert.equal(
      apiSource.match(/export interface ApiDiagnostic \{/gu)?.length,
      1,
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "TemperatureCanaryOperation"),
      [
        ["sidecar_read", "SidecarRead"],
        ["status_read", "StatusRead"],
        ["inference", "Inference"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApiDiagnostic"),
      [
        ["errorCode", "Nullable<String>"],
        ["errorName", "String"],
        ["event", "ApiDiagnosticEventEnum"],
        ["method", "String"],
        ["operation", "FieldPresence<TemperatureCanaryOperation>"],
        ["status", "u64"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ApiDiagnosticEventEnum"),
      [
        ["api_request_failed", "ApiRequestFailed"],
        ["temperature_canary_fallback", "TemperatureCanaryFallback"],
        ["rain_adjustment_fallback", "RainAdjustmentFallback"],
      ],
    );
    assert.match(
      rustStructFieldBlock(rust, "ApiDiagnostic", "status"),
      /wire rule: safe-unsigned-integer/u,
    );
    assertManifestRootGroup(manifest.roots, API_DIAGNOSTIC_ROOTS);
  },
);

test(
  "projects the original backfill chunk identity and outcome contracts",
  // keep persisted resume identity and its terminal outcome in one flat order
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const identityFields = [
      ["adapterVersion", "String"],
      ["chunkPlanVersion", "String"],
      ["intervalEndExclusive", "String"],
      ["intervalStart", "String"],
      ["requestedFromDate", "String"],
      ["requestedToDate", "String"],
      ["sourceConfigFingerprint", "String"],
      ["sourceId", "String"],
    ];
    const outcomeFields = [
      ...identityFields,
      ["completedAt", "String"],
      ["ingestionRunId", "String"],
      ["outcome", "BackfillChunkOutcomeOutcomeEnum"],
    ];

    assert.deepEqual(
      rustStructFieldEntries(rust, "BackfillChunkIdentity"),
      identityFields,
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "BackfillChunkOutcome"),
      outcomeFields,
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "BackfillChunkOutcome").slice(
        0,
        identityFields.length,
      ),
      rustStructFieldEntries(rust, "BackfillChunkIdentity"),
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "BackfillChunkOutcomeOutcomeEnum"),
      [["failed", "Failed"], ["succeeded", "Succeeded"]],
    );
    const identityDefinition = rust.match(
      /^pub struct BackfillChunkIdentity \{\n[\s\S]+?^\}/mu,
    );
    const outcomeDefinition = rust.match(
      /^pub struct BackfillChunkOutcome \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(identityDefinition);
    assert.ok(outcomeDefinition);
    assert.doesNotMatch(identityDefinition[0], /FieldPresence/u);
    assert.doesNotMatch(outcomeDefinition[0], /FieldPresence/u);
    assert.equal(
      identityDefinition[0].match(/wire rule: utc-iso/gu)?.length,
      2,
    );
    assert.equal(
      outcomeDefinition[0].match(/wire rule: utc-iso/gu)?.length,
      2,
    );
    // retain both inherited interval mappings on the base and flattened child
    for (const definition of [identityDefinition[0], outcomeDefinition[0]]) {
      assert.match(
        definition,
        /#\[doc = "wire rule: utc-iso[^"]*"\]\s*pub intervalEndExclusive: String,/u,
      );
      assert.match(
        definition,
        /#\[doc = "wire rule: utc-iso[^"]*"\]\s*pub intervalStart: String,/u,
      );
    }
    assert.match(outcomeDefinition[0], /pub completedAt: String,/u);
    assert.doesNotMatch(
      outcomeDefinition[0],
      /#\[doc = "wire rule: utc-iso[^"]*"\]\s*pub completedAt:/u,
    );
    const identityIndex = manifest.roots.findIndex(
      // select the exact base root independently of later additions
      (root) => root.source === "packages/domain/src/ingestion.ts" &&
        root.exportName === "BackfillChunkIdentity",
    );
    const outcomeIndex = manifest.roots.findIndex(
      // select the exact child root independently of later additions
      (root) => root.source === "packages/domain/src/ingestion.ts" &&
        root.exportName === "BackfillChunkOutcome",
    );
    assert.equal(outcomeIndex, identityIndex + 1);
    assert.deepEqual(
      [manifest.roots[identityIndex], manifest.roots[outcomeIndex]],
      [
        {
          source: "packages/domain/src/ingestion.ts",
          exportName: "BackfillChunkIdentity",
          rustName: "BackfillChunkIdentity",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/ingestion.ts",
          exportName: "BackfillChunkOutcome",
          rustName: "BackfillChunkOutcome",
          kind: "readonly-interface",
          base: "BackfillChunkIdentity",
        },
      ],
    );
  },
);

test(
  "projects the exact recursive JSON subset without a serializer",
  // retain JSON recursion and every JavaScript string code unit explicitly
  () => {
    const source = `export type JsonPrimitive = boolean | number | string | null;
export type JsonValue =
  | JsonPrimitive
  | readonly JsonValue[]
  | { readonly [key: string]: JsonValue };
export interface Metadata {
  readonly value: Readonly<Record<string, JsonValue>> | null;
}
`;
    const config = recursiveJsonFixtureConfig([{
      source: "fixture.ts",
      exportName: "Metadata",
      fields: { value: "ordered-json-object" },
    }]);
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);
    const definition = rust.match(/^pub enum JsonValue \{\n[\s\S]+?^\}/mu);

    assert.ok(definition);
    assert.equal(
      definition[0],
      `pub enum JsonValue {
    Boolean(bool),
    Number(f64),
    String(Vec<u16>),
    Null,
    Array(Vec<JsonValue>),
    Object(Vec<(Vec<u16>, JsonValue)>),
}`,
    );
    assert.deepEqual(rustStructFieldEntries(rust, "Metadata"), [
      ["value", "Nullable<Vec<(Vec<u16>, JsonValue)>>"],
    ]);
    assert.match(
      rust,
      /wire rule: ordered-json-object[^\n]*\n\s*pub value: Nullable<Vec<\(Vec<u16>, JsonValue\)>>,/u,
    );
    assert.deepEqual(manifest.roots, [
      {
        source: "fixture.ts",
        exportName: "JsonValue",
        rustName: "JsonValue",
        kind: "recursive-json-value",
        recursiveJson: "JsonPrimitive",
      },
      {
        source: "fixture.ts",
        exportName: "Metadata",
        rustName: "Metadata",
        kind: "readonly-interface",
      },
    ]);
  },
);

test(
  "projects the original canonical weather record contracts",
  // preserve complete metrics, metadata presence and normalized output times
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const metricFields = [
      "apparentTemperatureC",
      "blackGlobeTemperatureC",
      "cloudCoverPercent",
      "pm25MicrogramsPerCubicMeter",
      "precipitationMm",
      "precipitationRateMmPerHour",
      "pressureHpa",
      "relativeHumidityPercent",
      "soilElectricalConductivityMicrosiemensPerCm",
      "soilMoisturePercent",
      "solarRadiationWm2",
      "temperatureC",
      "uvIndex",
      "waterLevelM",
      "windDirectionDegrees",
      "windGustMps",
      "windSpeedMps",
      "wetBulbGlobeTemperatureC",
    ];
    const expectedRoots = [
      {
        source: "packages/domain/src/provenance.ts",
        exportName: "JsonValue",
        rustName: "JsonValue",
        kind: "recursive-json-value",
        recursiveJson: "JsonPrimitive",
      },
      {
        source: "packages/domain/src/weather-record.ts",
        exportName: "CanonicalWeatherMetrics",
        rustName: "CanonicalWeatherMetrics",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/weather-record.ts",
        exportName: "DeviceMetadata",
        rustName: "DeviceMetadata",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/weather-record.ts",
        exportName: "WeatherRecordMetadata",
        rustName: "WeatherRecordMetadata",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/weather-record.ts",
        exportName: "NormalizedWeatherRecord",
        rustName: "NormalizedWeatherRecord",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/weather-record.ts",
        exportName: "NormalizedWeatherRecordInput",
        rustName: "NormalizedWeatherRecordInput",
        kind: "readonly-interface",
      },
    ];
    const selectedRoots = expectedRoots.map(
      // select each exact producer declaration independently of later roots
      (expected) => manifest.roots.find(
        // compare one exact source and exported identity
        (root) => root.source === expected.source &&
          root.exportName === expected.exportName,
      ),
    );

    assert.deepEqual(
      rustStructFieldEntries(rust, "CanonicalWeatherMetrics"),
      metricFields.map(
        // preserve required null separately for each finite metric
        (field) => [field, "Nullable<f64>"],
      ),
    );
    assert.deepEqual(rustStructFieldEntries(rust, "DeviceMetadata"), [
      ["model", "FieldPresence<String>"],
      ["serial", "FieldPresence<String>"],
      ["vendor", "FieldPresence<String>"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "WeatherRecordMetadata"), [
      ["device", "Nullable<DeviceMetadata>"],
      ["model", "Nullable<String>"],
      ["provider", "Nullable<Vec<(Vec<u16>, JsonValue)>>"],
      ["quality", "Nullable<Vec<(Vec<u16>, JsonValue)>>"],
      ["upstreamTimezone", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "NormalizedWeatherRecord"), [
      ["metadata", "WeatherRecordMetadata"],
      ["metrics", "CanonicalWeatherMetrics"],
      ["productRunAt", "Nullable<String>"],
      ["receivedAt", "String"],
      ["sourceId", "String"],
      ["sourceKind", "SourceKind"],
      ["validAt", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "NormalizedWeatherRecordInput"), [
      ["metadata", "WeatherRecordMetadata"],
      ["metrics", "CanonicalWeatherMetrics"],
      ["productRunAt", "FieldPresence<Nullable<String>>"],
      ["receivedAt", "String"],
      ["sourceId", "String"],
      ["sourceKind", "SourceKind"],
      ["validAt", "String"],
    ]);
    const outputDefinition = rust.match(
      /^pub struct NormalizedWeatherRecord \{\n[\s\S]+?^\}/mu,
    );
    const inputDefinition = rust.match(
      /^pub struct NormalizedWeatherRecordInput \{\n[\s\S]+?^\}/mu,
    );
    const metadataDefinition = rust.match(
      /^pub struct WeatherRecordMetadata \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(outputDefinition);
    assert.ok(inputDefinition);
    assert.ok(metadataDefinition);
    assert.equal(outputDefinition[0].match(/wire rule: utc-iso/gu)?.length, 3);
    // bind each normalized output instant to its exact source field
    for (const field of ["productRunAt", "receivedAt", "validAt"]) {
      assert.match(
        outputDefinition[0],
        new RegExp(
          '#\\[doc = "wire rule: utc-iso[^"\\n]*"\\]\\s*pub ' +
            field + ': (?:Nullable<)?String>?,',
          "u",
        ),
      );
    }
    assert.doesNotMatch(inputDefinition[0], /wire rule: utc-iso/u);
    assert.equal(metadataDefinition[0].match(/wire rule: ordered-json-object/gu)?.length, 2);
    assert.deepEqual(selectedRoots, expectedRoots);
    const firstIndex = manifest.roots.indexOf(selectedRoots[0]);
    assert.equal(
      manifest.roots.indexOf(selectedRoots[expectedRoots.length - 1]),
      firstIndex + expectedRoots.length - 1,
    );
  },
);

test(
  "projects the original ingestion provenance anchor and checkpoint contracts",
  // preserve exact wire types values and producer-backed time normalization
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStringEnumWireEntries(rust, "IngestionMode"), [
      ["scheduled", "Scheduled"],
      ["backfill", "Backfill"],
    ]);
    assert.deepEqual(rustStringEnumWireEntries(rust, "IngestionState"), [
      ["running", "Running"],
      ["succeeded", "Succeeded"],
      ["failed", "Failed"],
      ["abandoned", "Abandoned"],
    ]);
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "IngestionErrorClassification"),
      [
        ["retryable", "Retryable"],
        ["rate_limited", "RateLimited"],
        ["permanent", "Permanent"],
        ["invalid_payload", "InvalidPayload"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "IngestionError"), [
      ["classification", "IngestionErrorClassification"],
      ["code", "String"],
      ["message", "String"],
      ["metadata", "FieldPresence<Vec<(Vec<u16>, JsonValue)>>"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "IngestionRun"), [
      ["adapterVersion", "String"],
      ["attempts", "u64"],
      ["chunkPlanVersion", "Nullable<String>"],
      ["deadlineAt", "String"],
      ["error", "Nullable<IngestionError>"],
      ["id", "String"],
      ["mode", "IngestionMode"],
      ["recordCount", "u64"],
      ["requestedEndExclusive", "String"],
      ["requestedStart", "String"],
      ["sourceConfigFingerprint", "String"],
      ["sourceId", "String"],
      ["startedAt", "String"],
      ["state", "IngestionState"],
    ]);
    assert.deepEqual(rustStringEnumWireEntries(rust, "SourceCapability"), [
      ["current", "Current"],
      ["historical", "Historical"],
      ["forecast", "Forecast"],
      ["stream", "Stream"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "SourceMaterialConfiguration"),
      [
        ["adapterConfig", "JsonValue"],
        ["location", "SourceMaterialConfigurationLocation"],
        ["providerKey", "String"],
        ["sourceKey", "String"],
        ["sourceKind", "SourceKind"],
        ["stationKey", "String"],
        ["version", "u64"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "SourceMaterialConfigurationLocation"),
      [
        ["latitude", "f64"],
        ["longitude", "f64"],
        ["siteKey", "String"],
        ["timezone", "String"],
      ],
    );
    assert.match(
      rust,
      /impl FixedForecastLeadHours \{\s*pub const WIRE_VALUES: &'static \[\(u64, Self\)\] = &\[\s*\(24, Self::Value24\),\s*\(48, Self::Value48\),\s*\(72, Self::Value72\),\s*\(96, Self::Value96\),\s*\(120, Self::Value120\),\s*\(144, Self::Value144\),\s*\(168, Self::Value168\),\s*\];\s*\}/u,
    );
    assert.match(
      rust,
      /impl NormalizedForecastAnchorRecordContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-anchor-record\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl NormalizedForecastAnchorRecordDatasetLiteral \{\s*pub const WIRE_VALUE: &'static str = "previous_runs";\s*\}/u,
    );
    assert.match(
      rust,
      /impl NormalizedForecastAnchorRecordSourceKindLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast";\s*\}/u,
    );
    assert.match(
      rust,
      /impl NormalizedForecastAnchorRecordUpstreamModelLiteral \{\s*pub const WIRE_VALUE: &'static str = "best_match";\s*\}/u,
    );
    assert.match(
      rust,
      /impl NormalizedForecastAnchorRecordInputDatasetLiteral \{\s*pub const WIRE_VALUE: &'static str = "previous_runs";\s*\}/u,
    );
    assert.match(
      rust,
      /impl NormalizedForecastAnchorRecordInputUpstreamModelLiteral \{\s*pub const WIRE_VALUE: &'static str = "best_match";\s*\}/u,
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "NormalizedForecastAnchorRecord"),
      [
        ["adapterVersion", "String"],
        ["contractEpoch", "String"],
        [
          "contractVersion",
          "NormalizedForecastAnchorRecordContractVersionLiteral",
        ],
        ["dataset", "NormalizedForecastAnchorRecordDatasetLiteral"],
        ["leadHours", "FixedForecastLeadHours"],
        ["metadata", "WeatherRecordMetadata"],
        ["metrics", "CanonicalWeatherMetrics"],
        ["receivedAt", "String"],
        ["sourceConfigFingerprint", "String"],
        ["sourceId", "String"],
        ["sourceKind", "NormalizedForecastAnchorRecordSourceKindLiteral"],
        [
          "upstreamModel",
          "NormalizedForecastAnchorRecordUpstreamModelLiteral",
        ],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "NormalizedForecastAnchorRecordInput"),
      [
        ["adapterVersion", "String"],
        ["contractEpoch", "String"],
        ["dataset", "NormalizedForecastAnchorRecordInputDatasetLiteral"],
        ["leadHours", "FixedForecastLeadHours"],
        ["metadata", "WeatherRecordMetadata"],
        ["metrics", "CanonicalWeatherMetrics"],
        ["receivedAt", "String"],
        ["sourceConfigFingerprint", "String"],
        ["sourceId", "String"],
        [
          "upstreamModel",
          "NormalizedForecastAnchorRecordInputUpstreamModelLiteral",
        ],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "ScheduledCheckpointState"), [
      ["lastCommittedAt", "String"],
      ["lastValidAt", "String"],
      ["providerCursor", "Nullable<Vec<(Vec<u16>, JsonValue)>>"],
      ["sourceId", "String"],
      ["version", "f64"],
      ["windowEndExclusive", "String"],
      ["windowStart", "String"],
    ]);

    const ingestionRun = rust.match(/^pub struct IngestionRun \{\n[\s\S]+?^\}/mu);
    const anchorRecord = rust.match(
      /^pub struct NormalizedForecastAnchorRecord \{\n[\s\S]+?^\}/mu,
    );
    const anchorInput = rust.match(
      /^pub struct NormalizedForecastAnchorRecordInput \{\n[\s\S]+?^\}/mu,
    );
    const repositoryCheckpoint = rust.match(
      /^pub struct ScheduledCheckpointState \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(ingestionRun);
    assert.ok(anchorRecord);
    assert.ok(anchorInput);
    assert.ok(repositoryCheckpoint);
    assert.equal(ingestionRun[0].match(/wire rule: utc-iso/gu)?.length, 4);
    // bind every validated ingestion instant to its source field
    for (const field of [
      "deadlineAt",
      "requestedEndExclusive",
      "requestedStart",
      "startedAt",
    ]) {
      assert.match(
        ingestionRun[0],
        new RegExp(
          '#\\[doc = "wire rule: utc-iso[^"\\n]*"\\]\\s*pub ' +
            field + ": String,",
          "u",
        ),
      );
    }
    assert.equal(anchorRecord[0].match(/wire rule: utc-iso/gu)?.length, 2);
    // bind each normalized anchor instant to its output field
    for (const field of ["receivedAt", "validAt"]) {
      assert.match(
        anchorRecord[0],
        new RegExp(
          '#\\[doc = "wire rule: utc-iso[^"\\n]*"\\]\\s*pub ' +
            field + ": String,",
          "u",
        ),
      );
    }
    assert.doesNotMatch(anchorInput[0], /wire rule: utc-iso/u);
    assert.equal(repositoryCheckpoint[0].match(/wire rule: utc-iso/gu)?.length, 4);
    // bind each database-normalized instant to its repository field
    for (const field of [
      "lastCommittedAt",
      "lastValidAt",
      "windowEndExclusive",
      "windowStart",
    ]) {
      assert.match(
        repositoryCheckpoint[0],
        new RegExp(
          '#\\[doc = "wire rule: utc-iso[^"\\n]*"\\]\\s*pub ' +
            field + ": String,",
          "u",
        ),
      );
    }
    assertManifestRootGroup(manifest.roots, [
      {
        source: "packages/domain/src/ingestion.ts",
        exportName: "IngestionMode",
        rustName: "IngestionMode",
        kind: "string-literal-enum",
      },
      {
        source: "packages/domain/src/ingestion.ts",
        exportName: "IngestionState",
        rustName: "IngestionState",
        kind: "string-literal-enum",
      },
      {
        source: "packages/domain/src/ingestion.ts",
        exportName: "IngestionErrorClassification",
        rustName: "IngestionErrorClassification",
        kind: "string-literal-enum",
      },
      {
        source: "packages/domain/src/ingestion.ts",
        exportName: "IngestionError",
        rustName: "IngestionError",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/ingestion.ts",
        exportName: "IngestionRun",
        rustName: "IngestionRun",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/provenance.ts",
        exportName: "SourceCapability",
        rustName: "SourceCapability",
        kind: "string-literal-enum",
      },
      {
        source: "packages/domain/src/provenance.ts",
        exportName: "SourceMaterialConfiguration",
        rustName: "SourceMaterialConfiguration",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-anchor-record.ts",
        exportName: "FixedForecastLeadHours",
        rustName: "FixedForecastLeadHours",
        kind: "unsigned-safe-integer-literal-enum",
        catalog: { kind: "unsigned-safe-integer-tuple" },
      },
      {
        source: "packages/domain/src/forecast-anchor-record.ts",
        exportName: "NormalizedForecastAnchorRecord",
        rustName: "NormalizedForecastAnchorRecord",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-anchor-record.ts",
        exportName: "NormalizedForecastAnchorRecordInput",
        rustName: "NormalizedForecastAnchorRecordInput",
        kind: "readonly-interface",
      },
      {
        source: "packages/database/src/repositories.ts",
        exportName: "ScheduledCheckpointState",
        rustName: "ScheduledCheckpointState",
        kind: "readonly-interface",
      },
    ]);
  },
);

test(
  "projects the repository ingestion commands and forecast query contracts",
  // preserve readonly repository inputs and their imported domain identities
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "StartIngestionRunInput"), [
      ["adapterVersion", "String"],
      ["attempts", "FieldPresence<u64>"],
      ["chunkPlanVersion", "FieldPresence<Nullable<String>>"],
      ["deadlineAt", "String"],
      ["mode", "IngestionMode"],
      [
        "requestMetadata",
        "FieldPresence<Nullable<Vec<(Vec<u16>, JsonValue)>>>",
      ],
      ["requestedEndExclusive", "String"],
      ["requestedStart", "String"],
      ["sourceConfigFingerprint", "String"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "PersistedWeatherAdjustmentRevision"),
      [
        ["adapterVersion", "String"],
        ["contentHash", "String"],
        ["logicalReceivedAt", "String"],
        ["providerKey", "String"],
        ["record", "NormalizedWeatherRecord"],
        ["sourceConfigFingerprint", "String"],
        ["sourceKey", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "CompleteBackfillIngestionInput"),
      [
        ["attempts", "u64"],
        ["identity", "BackfillChunkIdentity"],
        ["records", "Vec<NormalizedWeatherRecord>"],
        [
          "responseMetadata",
          "FieldPresence<Nullable<Vec<(Vec<u16>, JsonValue)>>>",
        ],
        ["runId", "String"],
        ["upstreamResponseChecksum", "FieldPresence<Nullable<String>>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "CompleteForecastAnchorBackfillIngestionInput",
      ),
      [
        ["attempts", "u64"],
        ["identity", "BackfillChunkIdentity"],
        ["records", "Vec<NormalizedForecastAnchorRecord>"],
        [
          "responseMetadata",
          "FieldPresence<Nullable<Vec<(Vec<u16>, JsonValue)>>>",
        ],
        ["runId", "String"],
        ["upstreamResponseChecksum", "FieldPresence<Nullable<String>>"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "FailIngestionRunInput"), [
      ["attempts", "u64"],
      ["backfillIdentity", "FieldPresence<BackfillChunkIdentity>"],
      ["error", "IngestionError"],
      [
        "responseMetadata",
        "FieldPresence<Nullable<Vec<(Vec<u16>, JsonValue)>>>",
      ],
      ["runId", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "CurrentQuery"), [
      ["asOf", "FieldPresence<String>"],
      ["sourceId", "FieldPresence<String>"],
      ["stationSlug", "FieldPresence<String>"],
    ]);
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ForecastProductSelection"),
      [
        ["anchor-containing", "AnchorContaining"],
        ["latest", "Latest"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "ForecastQuery"), [
      ["asOf", "String"],
      ["hours", "u64"],
      ["productSelection", "FieldPresence<ForecastProductSelection>"],
      ["siteSlug", "String"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastPressureContextQuery"),
      [
        ["asOf", "String"],
        ["siteSlug", "String"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "ForecastTrainingQuery"), [
      ["from", "String"],
      ["siteSlug", "String"],
      ["to", "String"],
    ]);

    const startInput = rust.match(
      /^pub struct StartIngestionRunInput \{\n[\s\S]+?^\}/mu,
    );
    const persistedRevision = rust.match(
      /^pub struct PersistedWeatherAdjustmentRevision \{\n[\s\S]+?^\}/mu,
    );
    const completeBackfill = rust.match(
      /^pub struct CompleteBackfillIngestionInput \{\n[\s\S]+?^\}/mu,
    );
    const completeAnchorBackfill = rust.match(
      /^pub struct CompleteForecastAnchorBackfillIngestionInput \{\n[\s\S]+?^\}/mu,
    );
    const failedInput = rust.match(
      /^pub struct FailIngestionRunInput \{\n[\s\S]+?^\}/mu,
    );
    const currentQuery = rust.match(
      /^pub struct CurrentQuery \{\n[\s\S]+?^\}/mu,
    );
    const forecastQuery = rust.match(
      /^pub struct ForecastQuery \{\n[\s\S]+?^\}/mu,
    );
    const pressureQuery = rust.match(
      /^pub struct ForecastPressureContextQuery \{\n[\s\S]+?^\}/mu,
    );
    const trainingQuery = rust.match(
      /^pub struct ForecastTrainingQuery \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(startInput);
    assert.ok(persistedRevision);
    assert.ok(completeBackfill);
    assert.ok(completeAnchorBackfill);
    assert.ok(failedInput);
    assert.ok(currentQuery);
    assert.ok(forecastQuery);
    assert.ok(pressureQuery);
    assert.ok(trainingQuery);
    assert.match(
      startInput[0],
      /wire rule: safe-unsigned-integer[^\n]*\n\s*pub attempts: FieldPresence<u64>,/u,
    );
    assert.match(
      startInput[0],
      /wire rule: ordered-json-object[^\n]*\n\s*pub requestMetadata: FieldPresence<Nullable<Vec<\(Vec<u16>, JsonValue\)>>>,/u,
    );
    assert.doesNotMatch(startInput[0], /wire rule: utc-iso/u);
    assert.match(
      persistedRevision[0],
      /wire rule: utc-iso[^\n]*\n\s*pub logicalReceivedAt: String,/u,
    );
    assert.equal(
      persistedRevision[0].match(/wire rule: utc-iso/gu)?.length,
      1,
    );
    // bind each completion command rule to its exact repository field
    for (const definition of [completeBackfill[0], completeAnchorBackfill[0]]) {
      assert.match(
        definition,
        /wire rule: safe-unsigned-integer[^\n]*\n\s*pub attempts: u64,/u,
      );
      assert.match(
        definition,
        /wire rule: ordered-json-object[^\n]*\n\s*pub responseMetadata: FieldPresence<Nullable<Vec<\(Vec<u16>, JsonValue\)>>>,/u,
      );
    }
    assert.match(
      failedInput[0],
      /wire rule: safe-unsigned-integer[^\n]*\n\s*pub attempts: u64,/u,
    );
    assert.match(
      failedInput[0],
      /wire rule: ordered-json-object[^\n]*\n\s*pub responseMetadata: FieldPresence<Nullable<Vec<\(Vec<u16>, JsonValue\)>>>,/u,
    );
    assert.match(
      forecastQuery[0],
      /wire rule: safe-unsigned-integer[^\n]*\n\s*pub hours: u64,/u,
    );
    assert.doesNotMatch(currentQuery[0], /wire rule: utc-iso/u);
    assert.doesNotMatch(forecastQuery[0], /wire rule: utc-iso/u);
    assert.doesNotMatch(pressureQuery[0], /wire rule: utc-iso/u);
    assert.doesNotMatch(trainingQuery[0], /wire rule: utc-iso/u);

    assertManifestRootGroup(manifest.roots, [
      {
        source: "packages/database/src/repositories.ts",
        exportName: "StartIngestionRunInput",
        rustName: "StartIngestionRunInput",
        kind: "readonly-interface",
      },
      {
        source: "packages/database/src/repositories.ts",
        exportName: "PersistedWeatherAdjustmentRevision",
        rustName: "PersistedWeatherAdjustmentRevision",
        kind: "readonly-interface",
      },
      {
        source: "packages/database/src/repositories.ts",
        exportName: "CompleteBackfillIngestionInput",
        rustName: "CompleteBackfillIngestionInput",
        kind: "readonly-interface",
      },
      {
        source: "packages/database/src/repositories.ts",
        exportName: "CompleteForecastAnchorBackfillIngestionInput",
        rustName: "CompleteForecastAnchorBackfillIngestionInput",
        kind: "readonly-interface",
      },
      {
        source: "packages/database/src/repositories.ts",
        exportName: "FailIngestionRunInput",
        rustName: "FailIngestionRunInput",
        kind: "readonly-interface",
      },
      {
        source: "packages/database/src/repositories.ts",
        exportName: "CurrentQuery",
        rustName: "CurrentQuery",
        kind: "readonly-interface",
      },
      {
        source: "packages/database/src/repositories.ts",
        exportName: "ForecastProductSelection",
        rustName: "ForecastProductSelection",
        kind: "string-literal-enum",
      },
      {
        source: "packages/database/src/repositories.ts",
        exportName: "ForecastQuery",
        rustName: "ForecastQuery",
        kind: "readonly-interface",
      },
      {
        source: "packages/database/src/repositories.ts",
        exportName: "ForecastPressureContextQuery",
        rustName: "ForecastPressureContextQuery",
        kind: "readonly-interface",
      },
      {
        source: "packages/database/src/repositories.ts",
        exportName: "ForecastTrainingQuery",
        rustName: "ForecastTrainingQuery",
        kind: "readonly-interface",
      },
    ]);
    const repositoryBinding = manifest.typeImports.find(
      // select the repository package edge independently of later graphs
      ({ consumer, specifier }) =>
        consumer === "packages/database/src/repositories.ts" &&
        specifier === "@weather/domain",
    );
    assert.ok(repositoryBinding);
    assert.deepEqual({
      ...repositoryBinding,
      exports: {
        BackfillChunkIdentity:
          repositoryBinding.exports.BackfillChunkIdentity,
        ForecastAdjustmentMetric:
          repositoryBinding.exports.ForecastAdjustmentMetric,
        ForecastObservationProviderFamily:
          repositoryBinding.exports.ForecastObservationProviderFamily,
        ForecastObservationStationKey:
          repositoryBinding.exports.ForecastObservationStationKey,
        IngestionError: repositoryBinding.exports.IngestionError,
        IngestionMode: repositoryBinding.exports.IngestionMode,
        JsonValue: repositoryBinding.exports.JsonValue,
        NormalizedForecastAnchorRecord:
          repositoryBinding.exports.NormalizedForecastAnchorRecord,
        NormalizedWeatherRecord:
          repositoryBinding.exports.NormalizedWeatherRecord,
        StationKind: repositoryBinding.exports.StationKind,
      },
    }, {
      consumer: "packages/database/src/repositories.ts",
      specifier: "@weather/domain",
      packageJson: "packages/domain/package.json",
      entry: "packages/domain/src/index.ts",
      leaves: [
        "packages/domain/src/forecast-adjustment.ts",
        "packages/domain/src/forecast-anchor-record.ts",
        "packages/domain/src/ingestion.ts",
        "packages/domain/src/provenance.ts",
        "packages/domain/src/rain-collection.ts",
        "packages/domain/src/weather-record.ts",
      ],
      exports: {
        BackfillChunkIdentity: "packages/domain/src/ingestion.ts",
        ForecastAdjustmentMetric:
          "packages/domain/src/forecast-adjustment.ts",
        ForecastObservationProviderFamily:
          "packages/domain/src/forecast-adjustment.ts",
        ForecastObservationStationKey:
          "packages/domain/src/forecast-adjustment.ts",
        IngestionError: "packages/domain/src/ingestion.ts",
        IngestionMode: "packages/domain/src/ingestion.ts",
        JsonValue: "packages/domain/src/provenance.ts",
        NormalizedForecastAnchorRecord:
          "packages/domain/src/forecast-anchor-record.ts",
        NormalizedWeatherRecord: "packages/domain/src/weather-record.ts",
        StationKind: "packages/domain/src/provenance.ts",
      },
    });
  },
);

test(
  "projects public repository read queries without strengthening accepted timestamps",
  // preserve optional filters, cursor shape, and repository-owned validation
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "HistoryQuery"), [
      ["cursor", "FieldPresence<HistoryQueryCursor>"],
      ["from", "FieldPresence<String>"],
      ["limit", "FieldPresence<u64>"],
      ["siteSlug", "String"],
      ["sourceId", "FieldPresence<String>"],
      ["sourceKind", "FieldPresence<SourceKind>"],
      ["stationSlug", "FieldPresence<String>"],
      ["to", "FieldPresence<String>"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "HistoryQueryCursor"), [
      ["id", "String"],
      ["validAt", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "TrendQuery"), [
      ["from", "String"],
      ["siteSlug", "String"],
      ["to", "String"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "DailyPrecipitationQuery"),
      [
        ["from", "String"],
        ["siteSlug", "String"],
        ["to", "String"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "TideQuery"), [
      ["from", "String"],
      ["limit", "FieldPresence<u64>"],
      ["siteSlug", "String"],
      ["to", "String"],
    ]);

    const historyQuery = rust.match(
      /^pub struct HistoryQuery \{\n[\s\S]+?^\}/mu,
    );
    const historyCursor = rust.match(
      /^pub struct HistoryQueryCursor \{\n[\s\S]+?^\}/mu,
    );
    const trendQuery = rust.match(/^pub struct TrendQuery \{\n[\s\S]+?^\}/mu);
    const precipitationQuery = rust.match(
      /^pub struct DailyPrecipitationQuery \{\n[\s\S]+?^\}/mu,
    );
    const tideQuery = rust.match(/^pub struct TideQuery \{\n[\s\S]+?^\}/mu);
    assert.ok(historyQuery);
    assert.ok(historyCursor);
    assert.ok(trendQuery);
    assert.ok(precipitationQuery);
    assert.ok(tideQuery);
    assert.match(
      historyQuery[0],
      /wire rule: readonly-inline-object, pinned shallow Readonly wrapper over explicit properties"\]\s*pub cursor: FieldPresence<HistoryQueryCursor>,/u,
    );
    assert.match(
      historyQuery[0],
      /wire rule: safe-unsigned-integer[^\n]*\n\s*pub limit: FieldPresence<u64>,/u,
    );
    assert.match(
      tideQuery[0],
      /wire rule: safe-unsigned-integer[^\n]*\n\s*pub limit: FieldPresence<u64>,/u,
    );
    // keep offset-accepting repository inputs free of canonical-utc metadata
    for (const definition of [
      historyQuery[0],
      historyCursor[0],
      trendQuery[0],
      precipitationQuery[0],
      tideQuery[0],
    ]) {
      assert.doesNotMatch(definition, /wire rule: utc-iso/u);
    }

    assertManifestRootGroup(manifest.roots, PUBLIC_READ_QUERY_ROOTS);
    const repositoryBinding = manifest.typeImports.find(
      // select the exact source package edge for provenance identity
      ({ consumer, specifier }) =>
        consumer === "packages/database/src/repositories.ts" &&
        specifier === "@weather/domain",
    );
    assert.ok(repositoryBinding);
    assert.equal(
      repositoryBinding.exports.SourceKind,
      "packages/domain/src/provenance.ts",
    );
  },
);

test(
  "projects rain capture bytes and adjustment records without runtime behavior",
  // preserve owned bodies, nullable capture identity, and distinct timestamp rules
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "RainAdjustmentCapture"),
      [
        ["claimId", "String"],
        ["kind", "RainAdjustmentCaptureKindEnum"],
        ["stationId", "Nullable<u64>"],
        ["runInitializedAt", "Nullable<String>"],
        ["windowStart", "Nullable<String>"],
        ["windowEndExclusive", "Nullable<String>"],
        ["completedAt", "String"],
        ["bodySha256", "String"],
        ["body", "Vec<u8>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainAdjustmentHour"),
      [
        ["validAt", "String"],
        ["modelLeadHours", "u64"],
        ["rawPrecipitationMm", "f64"],
        ["correctedPrecipitationMm", "f64"],
        ["applied", "bool"],
        ["reasonCode", "Nullable<String>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainAdjustmentRun"),
      [
        ["runInitializedAt", "String"],
        ["modelSha256", "String"],
        ["inputSha256", "String"],
        ["forecastClaimId", "String"],
        ["firstReceivedAt", "String"],
        ["decisionAt", "String"],
        ["generatedAt", "String"],
        ["hours", "Vec<RainAdjustmentHour>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "PersistedRainAdjustmentRevision"),
      [
        ["generatedAt", "String"],
        ["hours", "Vec<RainAdjustmentHour>"],
        ["inputSha256", "String"],
        ["modelSha256", "String"],
        ["runInitializedAt", "String"],
        ["storedContentSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainFixedGaugeTargetCapture"),
      [
        ["body", "Vec<u8>"],
        ["bodySha256", "String"],
        ["claimId", "String"],
        ["completedAt", "String"],
        ["stationId", "u64"],
        ["windowEndExclusive", "String"],
        ["windowStart", "String"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "RainAdjustmentCaptureKindEnum"),
      [
        ["forecast", "Forecast"],
        ["station", "Station"],
      ],
    );

    const capture = rust.match(
      /^pub struct RainAdjustmentCapture \{\n[\s\S]+?^\}/mu,
    );
    const hour = rust.match(
      /^pub struct RainAdjustmentHour \{\n[\s\S]+?^\}/mu,
    );
    const run = rust.match(
      /^pub struct RainAdjustmentRun \{\n[\s\S]+?^\}/mu,
    );
    const persisted = rust.match(
      /^pub struct PersistedRainAdjustmentRevision \{\n[\s\S]+?^\}/mu,
    );
    const fixedGauge = rust.match(
      /^pub struct RainFixedGaugeTargetCapture \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(capture);
    assert.ok(hour);
    assert.ok(run);
    assert.ok(persisted);
    assert.ok(fixedGauge);
    assert.equal(capture[0].match(/wire rule: utc-iso/gu)?.length, 4);
    assert.equal(persisted[0].match(/wire rule: utc-iso/gu)?.length, 2);
    assert.equal(fixedGauge[0].match(/wire rule: utc-iso/gu)?.length, 3);
    assert.equal(hour[0].match(/wire rule: finite-f64/gu)?.length, 2);
    assert.doesNotMatch(hour[0], /wire rule: utc-iso/u);
    assert.doesNotMatch(run[0], /wire rule: utc-iso/u);
    for (const definition of [capture[0], fixedGauge[0]]) {
      assert.match(
        definition,
        /storage rule: uint8array-bytes, owned Uint8Array bytes; not a JSON wire type"\]\s*pub body: Vec<u8>,/u,
      );
    }

    assertManifestRootGroup(manifest.roots, RAIN_CAPTURE_ADJUSTMENT_ROOTS);
  },
);

test(
  "projects rain capture receipts and fixed-gauge revision groups",
  // preserve transport evidence, scalar metadata, and staged revision identity
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "RainCaptureReceipt"), [
      ["startedAt", "String"],
      ["completedAt", "String"],
      ["httpStatus", "Nullable<u64>"],
      ["body", "Nullable<Vec<u8>>"],
      ["bodySha256", "Nullable<String>"],
      ["outcome", "RainCaptureOutcome"],
      ["errorCode", "Nullable<String>"],
      ["parserVersion", "RainCaptureReceiptParserVersionLiteral"],
      ["rowCount", "u64"],
      ["availableByDecision", "Nullable<bool>"],
      [
        "metadata",
        "Vec<(Vec<u16>, RainCaptureReceiptMetadataValue)>",
      ],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainFixedGaugeTargetRevisionRow"),
      [
        ["normalizedRecord", "NormalizedWeatherRecord"],
        ["stationId", "u64"],
        ["storedContentSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainFixedGaugeTargetRevisionGroup"),
      [
        ["logicalKeySha256", "String"],
        ["projectionIdentitySha256", "String"],
        ["projectionSha256", "String"],
        ["rows", "Vec<RainFixedGaugeTargetRevisionRow>"],
        ["stageReceiptSha256", "Nullable<String>"],
        ["validAt", "String"],
      ],
    );
    assert.match(
      rust,
      /pub enum RainCaptureReceiptMetadataValue \{\s*Null,\s*Boolean\(bool\),\s*Number\(f64\),\s*String\(Vec<u16>\),\s*\}/u,
    );
    assert.match(
      rust,
      /impl RainCaptureReceiptParserVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "rain-prospective-capture\/v1";\s*\}/u,
    );

    const receipt = rust.match(
      /^pub struct RainCaptureReceipt \{\n[\s\S]+?^\}/mu,
    );
    const row = rust.match(
      /^pub struct RainFixedGaugeTargetRevisionRow \{\n[\s\S]+?^\}/mu,
    );
    const group = rust.match(
      /^pub struct RainFixedGaugeTargetRevisionGroup \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(receipt);
    assert.ok(row);
    assert.ok(group);
    assert.equal(receipt[0].match(/wire rule: utc-iso/gu)?.length, 2);
    assert.equal(
      receipt[0].match(/wire rule: safe-unsigned-integer/gu)?.length,
      2,
    );
    assert.equal(
      row[0].match(/wire rule: safe-unsigned-integer/gu)?.length,
      1,
    );
    assert.match(
      receipt[0],
      /storage rule: uint8array-bytes, owned Uint8Array bytes; not a JSON wire type"\]\s*pub body: Nullable<Vec<u8>>,/u,
    );
    assert.match(
      receipt[0],
      /wire rule: ordered-json-scalar-object, ordered unique UTF-16 keys and scalar values; finite numbers only"\]\s*pub metadata: Vec<\(Vec<u16>, RainCaptureReceiptMetadataValue\)>,/u,
    );
    assert.doesNotMatch(group[0], /wire rule: utc-iso/u);

    assertManifestRootGroup(manifest.roots, RAIN_CAPTURE_RECEIPT_ROOTS);
    assert.deepEqual(
      manifest.typeImports.find(
        // select the rain revision package edge
        ({ consumer, specifier }) =>
          consumer === "packages/database/src/rain-collection.ts" &&
          specifier === "@weather/domain",
      ),
      RAIN_COLLECTION_DOMAIN_IMPORT,
    );
  },
);

test(
  "projects the readonly-wrapped rain capture request arms exactly",
  // preserve scheduling identity and the reviewed per-arm scalar rules
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "RainCaptureRequestForecast"),
      [
        ["kind", "RainCaptureRequestForecastKindLiteral"],
        ["slotKey", "String"],
        ["runInitializedAt", "String"],
        ["attempt", "RainCaptureRequestForecastAttemptEnum"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainCaptureRequestStation"),
      [
        ["kind", "RainCaptureRequestStationKindLiteral"],
        ["slotKey", "String"],
        ["stationId", "u64"],
        ["start", "String"],
        ["endExclusive", "String"],
      ],
    );
    assert.deepEqual(
      rustUnsignedIntegerEnumWireEntries(
        rust,
        "RainCaptureRequestForecastAttemptEnum",
      ),
      [
        [1, "Value1"],
        [2, "Value2"],
      ],
    );
    assert.match(
      rust,
      /impl RainCaptureRequestForecastKindLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast";\s*\}/u,
    );
    assert.match(
      rust,
      /impl RainCaptureRequestStationKindLiteral \{\s*pub const WIRE_VALUE: &'static str = "station";\s*\}/u,
    );

    const forecast = rust.match(
      /^pub struct RainCaptureRequestForecast \{\n[\s\S]+?^\}/mu,
    );
    const station = rust.match(
      /^pub struct RainCaptureRequestStation \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(forecast);
    assert.ok(station);
    assert.equal(forecast[0].match(/wire rule: utc-iso/gu)?.length, 1);
    assert.equal(
      forecast[0].match(/wire rule: unsigned-safe-integer-enum/gu)?.length,
      1,
    );
    assert.equal(station[0].match(/wire rule: utc-iso/gu)?.length, 2);
    assert.equal(
      station[0].match(/wire rule: safe-unsigned-integer/gu)?.length,
      1,
    );

    assertManifestRootGroup(manifest.roots, RAIN_CAPTURE_REQUEST_ROOTS);
  },
);

test(
  "projects archived revision receipts and the fixed-gauge result boundary exactly",
  // preserve archive identity and the complete-or-gap result payloads
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRevisionCommitReceipt"),
      [
        ["archiveCommitOrdinal", "String"],
        ["archiveCommittedAt", "String"],
        [
          "contractVersion",
          "AdjustmentRevisionCommitReceiptContractVersionLiteral",
        ],
        ["frontierSha256", "String"],
        ["predecessorFrontierSha256", "String"],
        ["projectionIdentitySha256", "String"],
        [
          "projectionKind",
          "AdjustmentRevisionCommitReceiptProjectionKindEnum",
        ],
        ["projectionSha256", "String"],
        ["receiptSha256", "String"],
        ["stageReceiptSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRevisionRowReceipt"),
      [
        [
          "contractVersion",
          "AdjustmentRevisionRowReceiptContractVersionLiteral",
        ],
        ["revisionReceipt", "AdjustmentRevisionCommitReceipt"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "AdjustmentRevisionGap"), [
      ["logicalKeySha256", "String"],
      ["projectionIdentitySha256", "Nullable<String>"],
      [
        "projectionKind",
        "AdjustmentRevisionCommitReceiptProjectionKindEnum",
      ],
      ["projectionSha256", "Nullable<String>"],
      ["reason", "AdjustmentRevisionGapReasonEnum"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainFixedGaugeTargetSource"),
      [
        ["locationId", "u64"],
        ["sourceConfigFingerprint", "String"],
        ["sourceId", "String"],
        ["sourceKey", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainFixedGaugeTargetSourceCatalog"),
      [
        ["catalogSha256", "String"],
        [
          "contractVersion",
          "RainFixedGaugeTargetSourceCatalogContractVersionLiteral",
        ],
        ["runContractSha256", "String"],
        ["sources", "Vec<RainFixedGaugeTargetSource>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "RainFixedGaugeTargetRevisionResultCommitted",
      ),
      [
        [
          "state",
          "RainFixedGaugeTargetRevisionResultCommittedStateLiteral",
        ],
        [
          "revisionReceipts",
          "Vec<AdjustmentRevisionCommitReceipt>",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainFixedGaugeTargetRevisionResultGap"),
      [
        ["state", "RainFixedGaugeTargetRevisionResultGapStateLiteral"],
        ["gap", "AdjustmentRevisionGap"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "AdjustmentRevisionCommitReceiptProjectionKindEnum",
      ),
      [
        ["actual_best_match", "ActualBestMatch"],
        ["native_source", "NativeSource"],
        ["rain_gate_input", "RainGateInput"],
        ["shadow_prediction", "ShadowPrediction"],
        ["target_revision", "TargetRevision"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "AdjustmentRevisionGapReasonEnum"),
      [
        ["archive_stage_failed", "ArchiveStageFailed"],
        ["database_bind_failed", "DatabaseBindFailed"],
        ["database_admission_failed", "DatabaseAdmissionFailed"],
        ["archive_publish_failed", "ArchivePublishFailed"],
      ],
    );
    assert.match(
      rust,
      /impl AdjustmentRevisionCommitReceiptContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "adjustment-revision-commit-receipt\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl AdjustmentRevisionRowReceiptContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "adjustment-revision-row-receipt\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl RainFixedGaugeTargetSourceCatalogContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "adjustment-rain-fixed-gauge-target-source-catalog\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl RainFixedGaugeTargetRevisionResultCommittedStateLiteral \{\s*pub const WIRE_VALUE: &'static str = "committed";\s*\}/u,
    );
    assert.match(
      rust,
      /impl RainFixedGaugeTargetRevisionResultGapStateLiteral \{\s*pub const WIRE_VALUE: &'static str = "gap";\s*\}/u,
    );

    const commitReceipt = rust.match(
      /^pub struct AdjustmentRevisionCommitReceipt \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(commitReceipt);
    assert.equal(commitReceipt[0].match(/wire rule: utc-iso/gu)?.length, 1);
    assert.equal(
      rust.match(
        /^pub enum AdjustmentRevisionCommitReceiptProjectionKindEnum \{/gmu,
      )?.length,
      1,
    );

    assertManifestRootGroup(manifest.roots, FIXED_GAUGE_RESULT_ROOTS);
    assert.deepEqual(
      manifest.typeImports.find(
        // select the fixed-gauge sibling receipt edge
        ({ consumer, specifier }) =>
          consumer === "packages/database/src/rain-collection.ts" &&
          specifier === "./adjustment-maintenance.js",
      ),
      RAIN_COLLECTION_ADJUSTMENT_IMPORT,
    );
  },
);

test(
  "projects original revision pointers and their exact admission keys",
  // retain original pointer evidence while narrowing each lookup key in source order
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentWeatherRevisionPointer"),
      [
        ["productRunAt", "Nullable<String>"],
        ["projectionIdentitySha256", "String"],
        ["projectionSha256", "String"],
        ["sourceId", "String"],
        [
          "sourceKind",
          "AdjustmentWeatherRevisionPointerSourceKindEnum",
        ],
        ["stageReceiptSha256", "String"],
        ["storedContentSha256", "String"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentForecastAnchorRevisionPointer"),
      [
        ["leadHours", "u64"],
        ["projectionIdentitySha256", "String"],
        ["projectionSha256", "String"],
        ["sourceId", "String"],
        ["stageReceiptSha256", "String"],
        ["storedContentSha256", "String"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainGateRevisionPointer"),
      [
        ["inputSha256", "String"],
        ["modelSha256", "String"],
        ["projectionIdentitySha256", "String"],
        ["projectionSha256", "String"],
        ["runInitializedAt", "String"],
        ["stageReceiptSha256", "String"],
        ["storedContentSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentEcmwfTemperatureRevisionPointer"),
      [
        ["projectionIdentitySha256", "String"],
        ["projectionSha256", "String"],
        ["providerResponseSha256", "String"],
        ["runInitializedAt", "String"],
        ["siteId", "String"],
        ["stageReceiptSha256", "String"],
        ["storedContentSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentWeatherRevisionAdmissionKey"),
      [
        ["productRunAt", "Nullable<String>"],
        ["sourceId", "String"],
        [
          "sourceKind",
          "AdjustmentWeatherRevisionPointerSourceKindEnum",
        ],
        ["storedContentSha256", "String"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "AdjustmentForecastAnchorRevisionAdmissionKey",
      ),
      [
        ["leadHours", "u64"],
        ["sourceId", "String"],
        ["storedContentSha256", "String"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainGateRevisionAdmissionKey"),
      [
        ["inputSha256", "String"],
        ["modelSha256", "String"],
        ["runInitializedAt", "String"],
        ["storedContentSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "AdjustmentEcmwfTemperatureRevisionAdmissionKey",
      ),
      [
        ["providerResponseSha256", "String"],
        ["runInitializedAt", "String"],
        ["siteId", "String"],
        ["storedContentSha256", "String"],
      ],
    );

    const selectedFields = [
      [
        "AdjustmentWeatherRevisionPointer",
        "AdjustmentWeatherRevisionAdmissionKey",
        ["productRunAt", "sourceId", "sourceKind", "storedContentSha256", "validAt"],
      ],
      [
        "AdjustmentForecastAnchorRevisionPointer",
        "AdjustmentForecastAnchorRevisionAdmissionKey",
        ["leadHours", "sourceId", "storedContentSha256", "validAt"],
      ],
      [
        "AdjustmentRainGateRevisionPointer",
        "AdjustmentRainGateRevisionAdmissionKey",
        ["inputSha256", "modelSha256", "runInitializedAt", "storedContentSha256"],
      ],
      [
        "AdjustmentEcmwfTemperatureRevisionPointer",
        "AdjustmentEcmwfTemperatureRevisionAdmissionKey",
        ["providerResponseSha256", "runInitializedAt", "siteId", "storedContentSha256"],
      ],
    ];
    // compare every selected field against its complete generated pointer block
    for (const [pointerName, keyName, fields] of selectedFields) {
      // preserve each selected field's metadata and nominal type exactly
      for (const field of fields) {
        assert.equal(
          rustStructFieldBlock(rust, keyName, field),
          rustStructFieldBlock(rust, pointerName, field),
        );
      }
    }
    assert.equal(
      rust.match(
        /^pub enum AdjustmentWeatherRevisionPointerSourceKindEnum \{/gmu,
      )?.length,
      1,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "AdjustmentWeatherRevisionPointer",
        "productRunAt",
      ),
      /wire rule: utc-iso/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "AdjustmentWeatherRevisionPointer", "validAt"),
      /wire rule: utc-iso/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "AdjustmentForecastAnchorRevisionPointer",
        "leadHours",
      ),
      /wire rule: safe-unsigned-integer/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "AdjustmentForecastAnchorRevisionPointer",
        "validAt",
      ),
      /wire rule: utc-iso/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "AdjustmentRainGateRevisionPointer",
        "runInitializedAt",
      ),
      /wire rule: utc-iso/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "AdjustmentEcmwfTemperatureRevisionPointer",
        "runInitializedAt",
      ),
      /wire rule: utc-iso/u,
    );

    assertManifestRootGroup(manifest.roots, REVISION_ADMISSION_ROOTS);
  },
);

test(
  "projects shadow snapshots, receipts, availability, and export authorization exactly",
  // preserve the nine tail roots and their reviewed wire metadata
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStringEnumWireEntries(rust, "AdjustmentShadowFamily"),
      [
        ["temperature", "Temperature"],
        ["wind", "Wind"],
        ["rain", "Rain"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentShadowPrediction"),
      [
        ["bodyByteCount", "u64"],
        ["candidateSha256", "String"],
        ["dueKey", "String"],
        ["inputSha256", "String"],
        ["issuedAt", "String"],
        ["maxValidAt", "String"],
        ["minValidAt", "String"],
        ["predictionBodySha256", "String"],
        ["predictionSchemaSha256", "String"],
        ["predictionSha256", "String"],
        ["registrationSha256", "String"],
        ["rowCount", "u64"],
        ["sourceReceiptSha256", "String"],
        ["sourceSha256", "String"],
        ["stageReceiptSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRevisionServingSnapshot"),
      [
        ["archiveCommitOrdinal", "String"],
        [
          "contractVersion",
          "AdjustmentRevisionServingSnapshotContractVersionLiteral",
        ],
        ["cutoffAt", "String"],
        ["entries", "Vec<AdjustmentRevisionServingSnapshotEntries>"],
        ["entryCount", "u64"],
        ["frontierSha256", "String"],
        ["snapshotSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "AdjustmentRevisionServingSnapshotEntries",
      ),
      [
        ["logicalReceivedAt", "String"],
        ["receipt", "AdjustmentRevisionCommitReceipt"],
        [
          "relation",
          "AdjustmentRevisionServingSnapshotEntriesRelationEnum",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentShadowRegistrationReceipt"),
      [
        ["inserted", "bool"],
        ["registrationSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentShadowRegistrationSlot"),
      [
        [
          "contractVersion",
          "AdjustmentShadowRegistrationSlotContractVersionLiteral",
        ],
        ["epochWitnessSha256", "String"],
        ["family", "AdjustmentShadowFamily"],
        ["horizonEndAt", "String"],
        ["registrationSha256", "Nullable<String>"],
        ["scheduleContractSha256", "String"],
        ["state", "AdjustmentShadowRegistrationSlotStateEnum"],
        ["terminalAt", "Nullable<String>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentShadowPredictionReceipt"),
      [
        ["committedAt", "String"],
        ["inserted", "bool"],
        ["predictionSha256", "String"],
        ["revisionReceipt", "AdjustmentRevisionCommitReceipt"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentHotPrediction"),
      [
        ["dueKey", "String"],
        ["maxValidAt", "String"],
        ["minValidAt", "String"],
        ["predictionSha256", "String"],
        ["rowCount", "u64"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentConfirmationAvailability"),
      [
        [
          "contractVersion",
          "AdjustmentConfirmationAvailabilityContractVersionLiteral",
        ],
        ["expectedKeySetSha256", "String"],
        ["family", "AdjustmentShadowFamily"],
        ["finalizedMetadataRootSha256", "String"],
        ["finalizedPredictionCount", "u64"],
        ["finalizedThroughAt", "Nullable<String>"],
        ["hotPredictionCount", "u64"],
        ["hotPredictions", "Vec<AdjustmentHotPrediction>"],
        ["hotSetRootSha256", "String"],
        ["intervalEndAt", "String"],
        ["intervalStartAt", "String"],
        ["metadataGeneration", "u64"],
        [
          "missingExpectedDueKeys",
          "AdjustmentConfirmationAvailabilityMissingExpectedDueKeysNull",
        ],
        [
          "missingExpectedDueKeysStatus",
          "AdjustmentConfirmationAvailabilityMissingExpectedDueKeysStatusLiteral",
        ],
        ["registrationSha256", "String"],
        ["targetCutoffAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "AdjustmentConfirmationChunkAuthorization",
      ),
      [
        ["accessSha256", "String"],
        [
          "chunkCount",
          "AdjustmentConfirmationChunkAuthorizationChunkCountEnum",
        ],
        ["chunkIndex", "u64"],
        [
          "contractVersion",
          "AdjustmentConfirmationChunkAuthorizationContractVersionLiteral",
        ],
        ["eligiblePredictionSetSha256", "String"],
        ["expectedKeySetSha256", "String"],
        ["family", "AdjustmentShadowFamily"],
        ["fromLocalDate", "String"],
        ["metadataRootSha256", "String"],
        ["registrationSha256", "String"],
        ["revisionCatalogWatermarkSha256", "String"],
        ["targetComparatorSnapshotRootSha256", "String"],
        ["targetCutoffAt", "String"],
        ["toLocalDateExclusive", "String"],
      ],
    );

    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "AdjustmentRevisionServingSnapshotEntriesRelationEnum",
      ),
      [
        ["ecmwf_temperature_canary_runs", "EcmwfTemperatureCanaryRuns"],
        ["forecast_anchor_records", "ForecastAnchorRecords"],
        ["rain_adjustment_runs", "RainAdjustmentRuns"],
        ["weather_records", "WeatherRecords"],
      ],
    );
    assert.deepEqual(
      rustUnsignedIntegerEnumWireEntries(
        rust,
        "AdjustmentConfirmationChunkAuthorizationChunkCountEnum",
      ),
      [
        [24, "Value24"],
        [27, "Value27"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "AdjustmentShadowRegistrationSlotStateEnum",
      ),
      [
        ["busy_v2_legacy", "BusyV2Legacy"],
        ["busy_v3", "BusyV3"],
        ["free", "Free"],
      ],
    );

    const prediction = rust.match(
      /^pub struct AdjustmentShadowPrediction \{\n[\s\S]+?^\}/mu,
    );
    const snapshot = rust.match(
      /^pub struct AdjustmentRevisionServingSnapshot \{\n[\s\S]+?^\}/mu,
    );
    const slot = rust.match(
      /^pub struct AdjustmentShadowRegistrationSlot \{\n[\s\S]+?^\}/mu,
    );
    const predictionReceipt = rust.match(
      /^pub struct AdjustmentShadowPredictionReceipt \{\n[\s\S]+?^\}/mu,
    );
    const hotPrediction = rust.match(
      /^pub struct AdjustmentHotPrediction \{\n[\s\S]+?^\}/mu,
    );
    const availability = rust.match(
      /^pub struct AdjustmentConfirmationAvailability \{\n[\s\S]+?^\}/mu,
    );
    const authorization = rust.match(
      /^pub struct AdjustmentConfirmationChunkAuthorization \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(prediction);
    assert.ok(snapshot);
    assert.ok(slot);
    assert.ok(predictionReceipt);
    assert.ok(hotPrediction);
    assert.ok(availability);
    assert.ok(authorization);
    assert.equal(prediction[0].match(/wire rule: utc-iso/gu)?.length, 3);
    assert.equal(
      prediction[0].match(/wire rule: safe-unsigned-integer/gu)?.length,
      2,
    );
    assert.equal(snapshot[0].match(/wire rule: utc-iso/gu)?.length, 1);
    assert.equal(
      snapshot[0].match(/wire rule: safe-unsigned-integer/gu)?.length,
      1,
    );
    assert.equal(slot[0].match(/wire rule: utc-iso/gu)?.length, 2);
    assert.equal(
      predictionReceipt[0].match(/wire rule: utc-iso/gu)?.length,
      1,
    );
    assert.equal(hotPrediction[0].match(/wire rule: utc-iso/gu)?.length, 2);
    assert.equal(
      hotPrediction[0].match(/wire rule: safe-unsigned-integer/gu)?.length,
      1,
    );
    assert.equal(availability[0].match(/wire rule: utc-iso/gu)?.length, 4);
    assert.equal(
      availability[0].match(/wire rule: safe-unsigned-integer/gu)?.length,
      3,
    );
    assert.equal(authorization[0].match(/wire rule: utc-iso/gu)?.length, 1);
    assert.equal(
      authorization[0].match(/wire rule: safe-unsigned-integer/gu)?.length,
      1,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "AdjustmentRevisionServingSnapshot",
        "entries",
      ),
      /wire rule: readonly-inline-object-array, ordered shallow Readonly inline objects/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "AdjustmentRevisionServingSnapshotEntries",
        "logicalReceivedAt",
      ),
      /wire rule: utc-iso/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "AdjustmentConfirmationAvailability",
        "missingExpectedDueKeys",
      ),
      /wire rule: exact-null, only JSON null/u,
    );
    assert.match(
      rust,
      /impl AdjustmentConfirmationAvailabilityMissingExpectedDueKeysStatusLiteral \{\s*pub const WIRE_VALUE: &'static str = "requires_anchored_cold_reconstruction";\s*\}/u,
    );
    assert.doesNotMatch(
      rustStructFieldBlock(
        rust,
        "AdjustmentConfirmationChunkAuthorization",
        "fromLocalDate",
      ),
      /wire rule: utc-iso/u,
    );
    assert.doesNotMatch(
      rustStructFieldBlock(
        rust,
        "AdjustmentConfirmationChunkAuthorization",
        "toLocalDateExclusive",
      ),
      /wire rule: utc-iso/u,
    );

    assertManifestRootGroup(manifest.roots, ADJUSTMENT_SNAPSHOT_ROOTS);
  },
);

test(
  "projects original archive relay receipts without widening wire identities",
  // preserve every original relay enum, structure, representation, and import proof
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const enumCases = [
      [
        "AdjustmentRevisionProjectionKind",
        [
          ["actual_best_match", "ActualBestMatch"],
          ["native_source", "NativeSource"],
          ["rain_gate_input", "RainGateInput"],
          ["target_revision", "TargetRevision"],
        ],
      ],
      [
        "ApiMaintenanceShadowFamily",
        [
          ["temperature", "Temperature"],
          ["wind", "Wind"],
        ],
      ],
      [
        "MaintenanceShadowGapReason",
        [
          ["candidate_unavailable", "CandidateUnavailable"],
          ["comparator_unavailable", "ComparatorUnavailable"],
          ["source_incomplete", "SourceIncomplete"],
          ["archive_stage_failed", "ArchiveStageFailed"],
          ["database_append_failed", "DatabaseAppendFailed"],
          ["database_admission_failed", "DatabaseAdmissionFailed"],
          ["archive_publish_failed", "ArchivePublishFailed"],
        ],
      ],
      [
        "AdjustmentRevisionGapReason",
        [
          ["archive_stage_failed", "ArchiveStageFailed"],
          ["database_bind_failed", "DatabaseBindFailed"],
          ["database_admission_failed", "DatabaseAdmissionFailed"],
          ["archive_publish_failed", "ArchivePublishFailed"],
        ],
      ],
      [
        "AdjustmentRevisionBatchPublishReceiptProjectionKindEnum",
        [
          ["actual_best_match", "ActualBestMatch"],
          ["target_revision", "TargetRevision"],
        ],
      ],
      [
        "AdjustmentMaintenanceStageReceiptContractVersionEnum",
        [
          ["adjustment-shadow-stage-receipt/v1", "AdjustmentShadowStageReceiptV1"],
          ["adjustment-shadow-stage-receipt/v2", "AdjustmentShadowStageReceiptV2"],
        ],
      ],
      [
        "AdjustmentMaintenancePublishReceiptContractVersionEnum",
        [
          ["adjustment-shadow-publish-receipt/v1", "AdjustmentShadowPublishReceiptV1"],
          ["adjustment-shadow-publish-receipt/v2", "AdjustmentShadowPublishReceiptV2"],
        ],
      ],
      [
        "AdjustmentMaintenanceGapInputFamilyEnum",
        [
          ["temperature", "Temperature"],
          ["wind", "Wind"],
          ["rain", "Rain"],
        ],
      ],
    ];
    const structCases = [
      [
        "AdjustmentRevisionStageReceipt",
        [
          [
            "contractVersion",
            "AdjustmentRevisionStageReceiptContractVersionLiteral",
          ],
          ["durable", "AdjustmentRevisionStageReceiptDurableLiteral"],
          ["durableAt", "String"],
          ["projectionIdentitySha256", "String"],
          ["projectionKind", "AdjustmentRevisionProjectionKind"],
          ["projectionSha256", "String"],
          ["stageReceiptSha256", "String"],
        ],
      ],
      [
        "RainMaintenanceControlStateStageReceipt",
        [
          [
            "contractVersion",
            "RainMaintenanceControlStateStageReceiptContractVersionLiteral",
          ],
          [
            "durable",
            "RainMaintenanceControlStateStageReceiptDurableLiteral",
          ],
          ["durableAt", "String"],
          ["stateSha256", "String"],
          ["stageReceiptSha256", "String"],
        ],
      ],
      [
        "RainMaintenanceControlStateGapInput",
        [
          ["reason", "RainMaintenanceControlStateGapInputReasonLiteral"],
          ["stateSha256", "String"],
          ["stageReceiptSha256", "String"],
        ],
      ],
      [
        "RainFixedGaugeTargetGapInput",
        [
          ["logicalHourAt", "String"],
          ["logicalKeySha256", "String"],
          ["reason", "RainFixedGaugeTargetGapInputReasonLiteral"],
        ],
      ],
      [
        "RainFixedGaugeTargetGapReceipt",
        [
          ["logicalHourAt", "String"],
          ["logicalKeySha256", "String"],
          ["reason", "RainFixedGaugeTargetGapInputReasonLiteral"],
          [
            "contractVersion",
            "RainFixedGaugeTargetGapReceiptContractVersionLiteral",
          ],
          ["gapAt", "String"],
          ["gapSha256", "String"],
          [
            "qualificationDisposition",
            "RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral",
          ],
        ],
      ],
      [
        "RainFixedGaugeTargetGapStatusInput",
        [
          ["logicalHourAt", "String"],
          ["logicalKeySha256", "String"],
        ],
      ],
      [
        "AdjustmentRevisionPublishReceipt",
        [
          ["committed", "AdjustmentRevisionPublishReceiptCommittedLiteral"],
          ["committedAt", "String"],
          [
            "contractVersion",
            "AdjustmentRevisionPublishReceiptContractVersionLiteral",
          ],
          ["projectionIdentitySha256", "String"],
          ["projectionKind", "AdjustmentRevisionProjectionKind"],
          ["projectionSha256", "String"],
          ["publishReceiptSha256", "String"],
          ["revisionReceiptSha256", "String"],
        ],
      ],
      [
        "AdjustmentRevisionBatchPublishReceipt",
        [
          [
            "committed",
            "AdjustmentRevisionBatchPublishReceiptCommittedLiteral",
          ],
          ["committedAt", "String"],
          [
            "contractVersion",
            "AdjustmentRevisionBatchPublishReceiptContractVersionLiteral",
          ],
          ["projectionIdentitySha256", "String"],
          [
            "projectionKind",
            "AdjustmentRevisionBatchPublishReceiptProjectionKindEnum",
          ],
          ["projectionSha256", "String"],
          ["publishReceiptSha256", "String"],
          ["revisionReceiptSha256s", "Vec<String>"],
        ],
      ],
      [
        "AdjustmentRevisionGapInput",
        [
          ["logicalKeySha256", "String"],
          ["projectionIdentitySha256", "Nullable<String>"],
          ["projectionKind", "AdjustmentRevisionProjectionKind"],
          ["projectionSha256", "Nullable<String>"],
          ["reason", "AdjustmentRevisionGapReason"],
        ],
      ],
      [
        "AdjustmentMaintenanceStageReceipt",
        [
          ["comparatorSha256", "FieldPresence<String>"],
          [
            "contractVersion",
            "AdjustmentMaintenanceStageReceiptContractVersionEnum",
          ],
          ["durable", "AdjustmentMaintenanceStageReceiptDurableLiteral"],
          ["durableAt", "String"],
          ["dueKey", "String"],
          ["predictionBodySha256", "String"],
          ["registrationSha256", "String"],
          ["sourceProjectionSha256", "String"],
          ["stageReceiptSha256", "String"],
        ],
      ],
      [
        "AdjustmentMaintenancePublishReceipt",
        [
          [
            "committed",
            "AdjustmentMaintenancePublishReceiptCommittedLiteral",
          ],
          ["committedAt", "String"],
          ["comparatorSha256", "FieldPresence<String>"],
          [
            "contractVersion",
            "AdjustmentMaintenancePublishReceiptContractVersionEnum",
          ],
          ["predictionSha256", "String"],
          ["publishReceiptSha256", "String"],
        ],
      ],
      [
        "AdjustmentMaintenanceGapInput",
        [
          ["dueKey", "String"],
          ["family", "AdjustmentMaintenanceGapInputFamilyEnum"],
          ["reason", "MaintenanceShadowGapReason"],
          ["registrationSha256", "String"],
        ],
      ],
    ];
    const literalCases = [
      [
        "AdjustmentRevisionStageReceiptContractVersionLiteral",
        "adjustment-revision-stage-receipt/v1",
      ],
      ["AdjustmentRevisionStageReceiptDurableLiteral", true],
      [
        "RainMaintenanceControlStateStageReceiptContractVersionLiteral",
        "adjustment-rain-control-state-stage-receipt/v1",
      ],
      ["RainMaintenanceControlStateStageReceiptDurableLiteral", true],
      [
        "RainMaintenanceControlStateGapInputReasonLiteral",
        "projection_stage_failed",
      ],
      ["RainFixedGaugeTargetGapInputReasonLiteral", "target_source_oversized"],
      [
        "RainFixedGaugeTargetGapReceiptContractVersionLiteral",
        "adjustment-rain-fixed-gauge-target-gap/v1",
      ],
      [
        "RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral",
        "forever_unqualified",
      ],
      ["AdjustmentRevisionPublishReceiptCommittedLiteral", true],
      [
        "AdjustmentRevisionPublishReceiptContractVersionLiteral",
        "adjustment-revision-publish-receipt/v1",
      ],
      ["AdjustmentRevisionBatchPublishReceiptCommittedLiteral", true],
      [
        "AdjustmentRevisionBatchPublishReceiptContractVersionLiteral",
        "adjustment-revision-batch-publish-receipt/v2",
      ],
      ["AdjustmentMaintenanceStageReceiptDurableLiteral", true],
      ["AdjustmentMaintenancePublishReceiptCommittedLiteral", true],
    ];
    const utcFields = [
      ["AdjustmentRevisionStageReceipt", "durableAt"],
      ["RainMaintenanceControlStateStageReceipt", "durableAt"],
      ["RainFixedGaugeTargetGapInput", "logicalHourAt"],
      ["RainFixedGaugeTargetGapReceipt", "logicalHourAt"],
      ["RainFixedGaugeTargetGapReceipt", "gapAt"],
      ["RainFixedGaugeTargetGapStatusInput", "logicalHourAt"],
      ["AdjustmentRevisionPublishReceipt", "committedAt"],
      ["AdjustmentRevisionBatchPublishReceipt", "committedAt"],
      ["AdjustmentMaintenanceStageReceipt", "durableAt"],
      ["AdjustmentMaintenancePublishReceipt", "committedAt"],
    ];

    // compare every enum against its source declaration order and wire spelling
    for (const [name, expectedEntries] of enumCases) {
      assert.deepEqual(rustStringEnumWireEntries(rust, name), expectedEntries);
    }
    // compare every relay structure against its exact flattened Rust fields
    for (const [name, expectedEntries] of structCases) {
      assert.deepEqual(rustStructFieldEntries(rust, name), expectedEntries);
    }
    // retain exact single-value markers instead of widening them to primitives
    for (const [name, wireValue] of literalCases) {
      const wireType = typeof wireValue === "boolean" ? "bool" : "&'static str";
      const encodedWireValue = typeof wireValue === "boolean"
        ? String(wireValue)
        : JSON.stringify(wireValue);
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: ${wireType} = ${encodedWireValue};\n}`,
        ),
        name,
      );
    }
    // retain utc metadata on every configured relay timestamp including base fields
    for (const [name, field] of utcFields) {
      assert.match(rustStructFieldBlock(rust, name, field), /wire rule: utc-iso/u);
    }

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, ARCHIVE_RELAY_ROOTS);
    assert.deepEqual(
      manifest.typeImports[17],
      ARCHIVE_RELAY_FORECAST_ADJUSTMENT_IMPORT,
    );
    assert.deepEqual(manifest.typeImports[18], ARCHIVE_RELAY_DATABASE_IMPORT);
  },
);

test(
  "projects archive relay inputs and nested gap status without wire widening",
  // preserve source order, byte presence, imported identities, clocks, counts, and markers
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "MaintenanceShadowSourceIdentity"),
      [
        ["inputSha256", "String"],
        ["sourceReceiptSha256", "String"],
        ["sourceRowSha256", "Vec<String>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "MaintenanceShadowPredictionMetadata"),
      [
        ["bodyByteCount", "u64"],
        ["candidateSha256", "String"],
        ["dueKey", "String"],
        ["inputSha256", "String"],
        ["issuedAt", "String"],
        ["maxValidAt", "String"],
        ["minValidAt", "String"],
        ["predictionBodySha256", "String"],
        ["predictionSchemaSha256", "String"],
        ["predictionSha256", "String"],
        ["registrationSha256", "String"],
        ["rowCount", "u64"],
        ["sourceReceiptSha256", "String"],
        ["sourceSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentMaintenanceStageInput"),
      [
        ["body", "Vec<u8>"],
        ["comparator", "FieldPresence<Vec<u8>>"],
        ["metadata", "MaintenanceShadowPredictionMetadata"],
        ["sourceProjection", "Vec<u8>"],
        ["sourceProjectionSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentMaintenancePublishInput"),
      [
        ["body", "Vec<u8>"],
        ["comparator", "FieldPresence<Vec<u8>>"],
        ["metadata", "MaintenanceShadowPredictionMetadata"],
        ["predictionCommittedAt", "FieldPresence<String>"],
        ["revisionReceipt", "AdjustmentRevisionCommitReceipt"],
        ["sourceProjection", "Vec<u8>"],
        ["sourceProjectionSha256", "String"],
        ["stageReceipt", "AdjustmentMaintenanceStageReceipt"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentMaintenanceCaptureRequest"),
      [
        ["dueKey", "String"],
        ["issuedAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainFixedGaugeTargetGapStatusAbsent"),
      [
        [
          "contractVersion",
          "RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral",
        ],
        ["state", "RainFixedGaugeTargetGapStatusAbsentStateLiteral"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainFixedGaugeTargetGapStatusPresent"),
      [
        [
          "contractVersion",
          "RainFixedGaugeTargetGapStatusPresentContractVersionLiteral",
        ],
        ["gap", "RainFixedGaugeTargetGapStatusPresentGap"],
        ["state", "RainFixedGaugeTargetGapStatusPresentStateLiteral"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainFixedGaugeTargetGapStatusPresentGap"),
      [
        [
          "contractVersion",
          "RainFixedGaugeTargetGapStatusPresentGapContractVersionLiteral",
        ],
        ["gapAt", "String"],
        ["gapSha256", "String"],
        ["logicalHourAt", "String"],
        ["logicalKeySha256", "String"],
        [
          "qualificationDisposition",
          "RainFixedGaugeTargetGapStatusPresentGapQualificationDispositionLiteral",
        ],
        ["reason", "RainFixedGaugeTargetGapStatusPresentGapReasonLiteral"],
      ],
    );
    assert.match(
      rust,
      /pub enum RainFixedGaugeTargetGapStatus \{\s*#\[doc = "wire discriminant state: \\"absent\\""\]\s*Absent\(RainFixedGaugeTargetGapStatusAbsent\),\s*#\[doc = "wire discriminant state: \\"present\\""\]\s*Present\(RainFixedGaugeTargetGapStatusPresent\),\s*\}/u,
    );

    const literalCases = [
      [
        "RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral",
        "adjustment-rain-fixed-gauge-target-gap-status/v1",
      ],
      ["RainFixedGaugeTargetGapStatusAbsentStateLiteral", "absent"],
      [
        "RainFixedGaugeTargetGapStatusPresentContractVersionLiteral",
        "adjustment-rain-fixed-gauge-target-gap-status/v1",
      ],
      [
        "RainFixedGaugeTargetGapStatusPresentGapContractVersionLiteral",
        "adjustment-rain-fixed-gauge-target-gap/v1",
      ],
      [
        "RainFixedGaugeTargetGapStatusPresentGapQualificationDispositionLiteral",
        "forever_unqualified",
      ],
      [
        "RainFixedGaugeTargetGapStatusPresentGapReasonLiteral",
        "target_source_oversized",
      ],
      ["RainFixedGaugeTargetGapStatusPresentStateLiteral", "present"],
    ];
    // retain every version, discriminator, qualification, and reason marker exactly
    for (const [name, wireValue] of literalCases) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: &'static str = ${JSON.stringify(wireValue)};\n}`,
        ),
        name,
      );
    }

    const utcFields = [
      ["MaintenanceShadowPredictionMetadata", "issuedAt"],
      ["MaintenanceShadowPredictionMetadata", "maxValidAt"],
      ["MaintenanceShadowPredictionMetadata", "minValidAt"],
      ["AdjustmentMaintenancePublishInput", "predictionCommittedAt"],
      ["AdjustmentMaintenanceCaptureRequest", "issuedAt"],
      ["RainFixedGaugeTargetGapStatusPresentGap", "gapAt"],
      ["RainFixedGaugeTargetGapStatusPresentGap", "logicalHourAt"],
    ];
    // retain utc metadata on every newly configured clock
    for (const [name, field] of utcFields) {
      assert.match(rustStructFieldBlock(rust, name, field), /wire rule: utc-iso/u);
    }
    // retain the reviewed unsigned representation on both prediction counts
    for (const field of ["bodyByteCount", "rowCount"]) {
      assert.match(
        rustStructFieldBlock(rust, "MaintenanceShadowPredictionMetadata", field),
        /wire rule: safe-unsigned-integer/u,
      );
    }
    // retain raw byte storage independently of outer optional presence
    for (const [name, field] of [
      ["AdjustmentMaintenanceStageInput", "body"],
      ["AdjustmentMaintenanceStageInput", "comparator"],
      ["AdjustmentMaintenanceStageInput", "sourceProjection"],
      ["AdjustmentMaintenancePublishInput", "body"],
      ["AdjustmentMaintenancePublishInput", "comparator"],
      ["AdjustmentMaintenancePublishInput", "sourceProjection"],
    ]) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /storage rule: uint8array-bytes/u,
      );
    }
    assert.match(
      rustStructFieldBlock(rust, "RainFixedGaugeTargetGapStatusPresent", "gap"),
      /wire rule: readonly-inline-object/u,
    );

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, ARCHIVE_RELAY_INPUT_ROOTS);
    assert.deepEqual(
      manifest.typeImports.slice(17, 19),
      [ARCHIVE_RELAY_FORECAST_ADJUSTMENT_IMPORT, ARCHIVE_RELAY_DATABASE_IMPORT],
    );
  },
);

test(
  "projects complete revision bodies without widening ordered wire values",
  // preserve projection order, scalar arms, imported state, markers, clocks, and counts
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStringEnumWireEntries(rust, "AdjustmentRevisionProjectionFamily"),
      [
        ["rain", "Rain"],
        ["shared", "Shared"],
        ["temperature", "Temperature"],
        ["wind", "Wind"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "MaintenanceShadowTemperatureRecentErrorState"),
      [
        ["b24C", "Nullable<f64>"],
        ["b72C", "Nullable<f64>"],
        [
          "cohort",
          "MaintenanceShadowTemperatureRecentErrorStateCohortLiteral",
        ],
        ["localDates", "u64"],
        ["mad72C", "Nullable<f64>"],
        ["maximumSourceRunInitializedAt", "Nullable<String>"],
        ["maximumSourceValidAt", "Nullable<String>"],
        ["n24", "u64"],
        ["n72", "u64"],
        ["sourceKeys", "Vec<String>"],
        ["supported", "bool"],
        ["targetRunInitializedAt", "String"],
        ["windowEndValidAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRevisionProjection"),
      [
        ["contractVersion", "AdjustmentRevisionProjectionContractVersionLiteral"],
        ["family", "AdjustmentRevisionProjectionFamily"],
        [
          "logicalKey",
          "Vec<(Vec<u16>, AdjustmentRevisionProjectionLogicalKeyValue)>",
        ],
        ["logicalReceivedAt", "String"],
        ["projectionKind", "AdjustmentRevisionProjectionKind"],
        [
          "rows",
          "Vec<Vec<(Vec<u16>, AdjustmentRevisionProjectionRowsValue)>>",
        ],
        ["source", "Vec<(Vec<u16>, Nullable<Vec<u16>>)>"],
        ["storedContentSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRevisionBatchProjection"),
      [
        [
          "contractVersion",
          "AdjustmentRevisionBatchProjectionContractVersionLiteral",
        ],
        ["family", "AdjustmentRevisionBatchProjectionFamilyEnum"],
        [
          "logicalKey",
          "Vec<(Vec<u16>, AdjustmentRevisionBatchProjectionLogicalKeyValue)>",
        ],
        ["logicalReceivedAt", "String"],
        [
          "projectionKind",
          "AdjustmentRevisionBatchProjectionProjectionKindEnum",
        ],
        [
          "rows",
          "Vec<Vec<(Vec<u16>, AdjustmentRevisionBatchProjectionRowsValue)>>",
        ],
        ["source", "Vec<(Vec<u16>, Nullable<Vec<u16>>)>"],
        ["storedContentSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentTemperatureNativeSourceProjection"),
      [
        [
          "contractVersion",
          "AdjustmentTemperatureNativeSourceProjectionContractVersionLiteral",
        ],
        [
          "family",
          "AdjustmentTemperatureNativeSourceProjectionFamilyLiteral",
        ],
        [
          "logicalKey",
          "Vec<(Vec<u16>, AdjustmentRevisionProjectionLogicalKeyValue)>",
        ],
        ["logicalReceivedAt", "String"],
        [
          "projectionKind",
          "AdjustmentTemperatureNativeSourceProjectionProjectionKindLiteral",
        ],
        [
          "recentErrorState",
          "MaintenanceShadowTemperatureRecentErrorState",
        ],
        ["recentErrorStateSha256", "String"],
        [
          "rows",
          "Vec<Vec<(Vec<u16>, AdjustmentRevisionProjectionRowsValue)>>",
        ],
        ["source", "Vec<(Vec<u16>, Nullable<Vec<u16>>)>"],
        ["storedContentSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainGateFeatureProjection"),
      [
        [
          "contractVersion",
          "AdjustmentRainGateFeatureProjectionContractVersionLiteral",
        ],
        ["family", "AdjustmentRainGateFeatureProjectionFamilyLiteral"],
        ["logicalKey", "AdjustmentRainGateFeatureProjectionLogicalKey"],
        ["logicalReceivedAt", "String"],
        [
          "projectionKind",
          "AdjustmentRainGateFeatureProjectionProjectionKindLiteral",
        ],
        ["rows", "Vec<AdjustmentRainGateFeatureProjectionRows>"],
        ["source", "Vec<(Vec<u16>, Nullable<Vec<u16>>)>"],
        ["storedContentSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainGateFeatureProjectionLogicalKey"),
      [
        ["inputSha256", "String"],
        ["modelSha256", "String"],
        ["runInitializedAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainGateFeatureProjectionRows"),
      [
        ["features64", "Vec<Nullable<String>>"],
        ["modelLeadHours", "u64"],
        ["rawPrecipitationMm64", "String"],
        ["rawTargetHourTemperatureC64", "String"],
        ["validAt", "String"],
      ],
    );

    const scalarEnums = [
      ["AdjustmentRevisionProjectionLogicalKeyValue", false],
      ["AdjustmentRevisionProjectionRowsValue", true],
      ["AdjustmentRevisionBatchProjectionLogicalKeyValue", false],
      ["AdjustmentRevisionBatchProjectionRowsValue", true],
    ];
    // preserve distinct logical-key and row scalar domains for both body forms
    for (const [name, hasBoolean] of scalarEnums) {
      const booleanArm = hasBoolean ? "\\s*Boolean\\(bool\\)," : "";
      assert.match(
        rust,
        new RegExp(
          `pub enum ${name} \\{\\s*Null,${booleanArm}\\s*Number\\(f64\\),\\s*String\\(Vec<u16>\\),\\s*\\}`,
          "u",
        ),
      );
    }
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "AdjustmentRevisionBatchProjectionFamilyEnum",
      ),
      [
        ["shared", "Shared"],
        ["wind", "Wind"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "AdjustmentRevisionBatchProjectionProjectionKindEnum",
      ),
      [
        ["actual_best_match", "ActualBestMatch"],
        ["target_revision", "TargetRevision"],
      ],
    );

    const literalCases = [
      [
        "MaintenanceShadowTemperatureRecentErrorStateCohortLiteral",
        "ecmwf_single_run_hindcast",
      ],
      [
        "AdjustmentRevisionProjectionContractVersionLiteral",
        "adjustment-revision-projection/v1",
      ],
      [
        "AdjustmentRevisionBatchProjectionContractVersionLiteral",
        "adjustment-revision-batch-projection/v2",
      ],
      [
        "AdjustmentTemperatureNativeSourceProjectionContractVersionLiteral",
        "adjustment-temperature-native-source-projection/v2",
      ],
      ["AdjustmentTemperatureNativeSourceProjectionFamilyLiteral", "temperature"],
      [
        "AdjustmentTemperatureNativeSourceProjectionProjectionKindLiteral",
        "native_source",
      ],
      [
        "AdjustmentRainGateFeatureProjectionContractVersionLiteral",
        "adjustment-rain-gate-feature-projection/v2",
      ],
      ["AdjustmentRainGateFeatureProjectionFamilyLiteral", "rain"],
      [
        "AdjustmentRainGateFeatureProjectionProjectionKindLiteral",
        "rain_gate_input",
      ],
    ];
    // retain every version, family, kind, and cohort marker exactly
    for (const [name, wireValue] of literalCases) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: &'static str = ${JSON.stringify(wireValue)};\n}`,
        ),
        name,
      );
    }

    const mappingDocs = [
      [
        "AdjustmentRevisionProjection",
        "logicalKey",
        /wire rule: ordered-json-key-object, ordered unique UTF-16 keys and string, finite number, or null values/u,
      ],
      [
        "AdjustmentRevisionProjection",
        "rows",
        /wire rule: ordered-json-scalar-object-array, ordered array of scalar objects with UTF-16 keys and finite numbers/u,
      ],
      [
        "AdjustmentRevisionProjection",
        "source",
        /wire rule: ordered-json-nullable-string-object, ordered unique UTF-16 keys and nullable UTF-16 strings/u,
      ],
      [
        "AdjustmentRevisionBatchProjection",
        "logicalKey",
        /wire rule: ordered-json-key-object, ordered unique UTF-16 keys and string, finite number, or null values/u,
      ],
      [
        "AdjustmentRevisionBatchProjection",
        "rows",
        /wire rule: ordered-json-scalar-object-array, ordered array of scalar objects with UTF-16 keys and finite numbers/u,
      ],
      [
        "AdjustmentRevisionBatchProjection",
        "source",
        /wire rule: ordered-json-nullable-string-object, ordered unique UTF-16 keys and nullable UTF-16 strings/u,
      ],
      [
        "AdjustmentRainGateFeatureProjection",
        "source",
        /wire rule: ordered-json-nullable-string-object, ordered unique UTF-16 keys and nullable UTF-16 strings/u,
      ],
    ];
    // retain the exact ordered dynamic-key wire rule on every configured field
    for (const [name, field, expected] of mappingDocs) {
      assert.match(rustStructFieldBlock(rust, name, field), expected);
    }

    const utcFields = [
      ["MaintenanceShadowTemperatureRecentErrorState", "maximumSourceRunInitializedAt"],
      ["MaintenanceShadowTemperatureRecentErrorState", "maximumSourceValidAt"],
      ["MaintenanceShadowTemperatureRecentErrorState", "targetRunInitializedAt"],
      ["MaintenanceShadowTemperatureRecentErrorState", "windowEndValidAt"],
      ["AdjustmentRevisionProjection", "logicalReceivedAt"],
      ["AdjustmentRevisionBatchProjection", "logicalReceivedAt"],
      ["AdjustmentTemperatureNativeSourceProjection", "logicalReceivedAt"],
      ["AdjustmentRainGateFeatureProjection", "logicalReceivedAt"],
      ["AdjustmentRainGateFeatureProjectionLogicalKey", "runInitializedAt"],
      ["AdjustmentRainGateFeatureProjectionRows", "validAt"],
    ];
    // retain utc metadata on every projection clock and nullable state bound
    for (const [name, field] of utcFields) {
      assert.match(rustStructFieldBlock(rust, name, field), /wire rule: utc-iso/u);
    }
    // retain finite nullable error values without widening their null state
    for (const field of ["b24C", "b72C", "mad72C"]) {
      assert.match(
        rustStructFieldBlock(
          rust,
          "MaintenanceShadowTemperatureRecentErrorState",
          field,
        ),
        /wire rule: finite-f64/u,
      );
    }
    // retain reviewed unsigned counts and lead hours
    for (const [name, field] of [
      ["MaintenanceShadowTemperatureRecentErrorState", "localDates"],
      ["MaintenanceShadowTemperatureRecentErrorState", "n24"],
      ["MaintenanceShadowTemperatureRecentErrorState", "n72"],
      ["AdjustmentRainGateFeatureProjectionRows", "modelLeadHours"],
    ]) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /wire rule: safe-unsigned-integer/u,
      );
    }

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, REVISION_PROJECTION_ROOTS);
    assert.deepEqual(
      manifest.typeImports[19],
      REVISION_PROJECTION_SHADOW_VALUES_IMPORT,
    );
  },
);

test(
  "projects the complete rain control document without persistence or probability widening",
  // preserve persistence, control, probability, union, marker, clock, and count identities
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "RainMaintenancePersistenceCaptureMember"),
      [
        ["bodySha256", "String"],
        ["claimId", "String"],
        ["completedAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainMaintenancePersistenceTargetRow"),
      [
        ["captureMembers", "Vec<RainMaintenancePersistenceCaptureMember>"],
        ["precipitationMm64", "Nullable<String>"],
        ["receivedAt", "Nullable<String>"],
        ["stationId", "u64"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainMaintenancePersistenceTarget"),
      [
        [
          "contractVersion",
          "RainMaintenancePersistenceTargetContractVersionLiteral",
        ],
        ["decisionAt", "String"],
        ["prediction64", "Nullable<String>"],
        ["reason", "RainMaintenancePersistenceTargetReasonEnum"],
        ["rowCount", "RainMaintenancePersistenceTargetRowCountLiteral"],
        ["rows", "Vec<RainMaintenancePersistenceTargetRow>"],
        ["targetMemberSha256", "String"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainGateControlProjection"),
      [
        [
          "contractVersion",
          "AdjustmentRainGateControlProjectionContractVersionLiteral",
        ],
        ["family", "AdjustmentRainGateControlProjectionFamilyLiteral"],
        ["logicalKey", "AdjustmentRainGateControlProjectionLogicalKey"],
        ["logicalReceivedAt", "String"],
        ["ordinalArtifactSha256", "String"],
        ["persistenceTarget", "RainMaintenancePersistenceTarget"],
        [
          "projectionKind",
          "AdjustmentRainGateControlProjectionProjectionKindLiteral",
        ],
        ["rows", "Vec<AdjustmentRainGateControlProjectionRows>"],
        ["source", "Vec<(Vec<u16>, Nullable<Vec<u16>>)>"],
        ["stateSha256", "String"],
        ["stateStageReceiptSha256", "String"],
        ["storedContentSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainGateControlProjectionLogicalKey"),
      [
        ["inputSha256", "String"],
        ["modelSha256", "String"],
        ["runInitializedAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainGateControlProjectionRows"),
      [
        ["features64", "Vec<Nullable<String>>"],
        ["incumbentArtifactIdentitySha256", "Nullable<String>"],
        ["incumbentPrediction64", "String"],
        [
          "incumbentProbability",
          "AdjustmentRainGateControlProjectionRowsIncumbentProbability",
        ],
        ["incumbentReceiptMemberSha256", "String"],
        ["modelLeadHours", "u64"],
        [
          "nativeSourceProbability",
          "AdjustmentRainGateControlProjectionRowsNativeSourceProbability",
        ],
        ["persistencePrediction64", "String"],
        [
          "persistenceReason",
          "AdjustmentRainGateControlProjectionRowsPersistenceReasonEnum",
        ],
        ["persistenceTargetMemberSha256", "Nullable<String>"],
        ["rawPrecipitationMm64", "String"],
        ["rawTargetHourTemperatureC64", "String"],
        ["recentVolumeScalePrediction64", "String"],
        ["sameWindowVolumeScalePrediction64", "String"],
        ["sourceRowSha256", "String"],
        ["unchangedOrdinalPrediction64", "String"],
        ["validAt", "String"],
        ["volumeScalePrediction64", "String"],
      ],
    );

    const probabilityFields = [
      ["atLeast0_1", "String"],
      ["atLeast1_0", "String"],
      ["atLeast2_5", "String"],
    ];
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "AdjustmentRainGateControlProjectionRowsIncumbentProbability",
      ),
      probabilityFields,
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "AdjustmentRainGateControlProjectionRowsNativeSourceProbability",
      ),
      probabilityFields,
    );

    assert.deepEqual(
      rustStringEnumWireEntries(rust, "RainMaintenancePersistenceTargetReasonEnum"),
      [
        ["causal_target", "CausalTarget"],
        ["raw_fallback_unavailable", "RawFallbackUnavailable"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "AdjustmentRainGateControlProjectionRowsPersistenceReasonEnum",
      ),
      [
        ["causal_target", "CausalTarget"],
        ["raw_fallback_unavailable", "RawFallbackUnavailable"],
      ],
    );

    const documentDefinition = rust.match(
      /^pub enum AdjustmentRevisionProjectionDocument \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(documentDefinition);
    assert.equal(
      documentDefinition[0],
      `pub enum AdjustmentRevisionProjectionDocument {
    Single(AdjustmentRevisionProjection),
    Batch(AdjustmentRevisionBatchProjection),
    NativeTemperature(AdjustmentTemperatureNativeSourceProjection),
    RainFeature(AdjustmentRainGateFeatureProjection),
    RainControl(AdjustmentRainGateControlProjection),
}`,
    );

    const literalCases = [
      [
        "RainMaintenancePersistenceTargetContractVersionLiteral",
        "rain-maintenance-persistence-target/v1",
      ],
      [
        "AdjustmentRainGateControlProjectionContractVersionLiteral",
        "adjustment-rain-gate-control-projection/v3",
      ],
      ["AdjustmentRainGateControlProjectionFamilyLiteral", "rain"],
      [
        "AdjustmentRainGateControlProjectionProjectionKindLiteral",
        "rain_gate_input",
      ],
    ];
    // retain every persistence and control marker exactly
    for (const [name, wireValue] of literalCases) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: &'static str = ${JSON.stringify(wireValue)};\n}`,
        ),
        name,
      );
    }
    assert.ok(
      rust.includes(
        "impl RainMaintenancePersistenceTargetRowCountLiteral {\n    pub const WIRE_VALUE: u64 = 12;\n}",
      ),
    );

    const utcFields = [
      ["RainMaintenancePersistenceCaptureMember", "completedAt"],
      ["RainMaintenancePersistenceTargetRow", "receivedAt"],
      ["RainMaintenancePersistenceTarget", "decisionAt"],
      ["RainMaintenancePersistenceTarget", "validAt"],
      ["AdjustmentRainGateControlProjection", "logicalReceivedAt"],
      ["AdjustmentRainGateControlProjectionLogicalKey", "runInitializedAt"],
      ["AdjustmentRainGateControlProjectionRows", "validAt"],
    ];
    // retain utc metadata on each persistence and control clock
    for (const [name, field] of utcFields) {
      assert.match(rustStructFieldBlock(rust, name, field), /wire rule: utc-iso/u);
    }
    // retain safe unsigned storage on the reviewed station and lead values
    for (const [name, field] of [
      ["RainMaintenancePersistenceTargetRow", "stationId"],
      ["AdjustmentRainGateControlProjectionRows", "modelLeadHours"],
    ]) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /wire rule: safe-unsigned-integer/u,
      );
    }

    const probabilityDoc =
      "wire rule: readonly-string-literal-record, exact string values at declared keys in source order";
    // retain the same fixed-key wire rule independently on both probability maps
    for (const field of ["incumbentProbability", "nativeSourceProbability"]) {
      assert.equal(
        rustStructFieldBlock(
          rust,
          "AdjustmentRainGateControlProjectionRows",
          field,
        ),
        `    #[doc = ${JSON.stringify(probabilityDoc)}]\n    pub ${field}: AdjustmentRainGateControlProjectionRows${field[0].toUpperCase()}${field.slice(1)},`,
      );
    }
    assert.match(
      rustStructFieldBlock(rust, "AdjustmentRainGateControlProjection", "source"),
      /wire rule: ordered-json-nullable-string-object, ordered unique UTF-16 keys and nullable UTF-16 strings/u,
    );

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, REVISION_DOCUMENT_ROOTS);
    assert.deepEqual(
      manifest.typeImports[19],
      REVISION_PROJECTION_SHADOW_VALUES_IMPORT,
    );
  },
);

test(
  "projects the complete fixed gauge target document without decoding evidence strings",
  // preserve the target graph, imported record, encoded evidence, markers, clocks, and counts
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainFixedGaugeTargetClaim"),
      [
        ["claimId", "String"],
        ["completedAt", "String"],
        ["stationId", "u64"],
        ["windowEndExclusive", "String"],
        ["windowStart", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainFixedGaugeTargetCaptureBody"),
      [
        ["bodyBase64", "String"],
        ["bodySha256", "String"],
        ["claims", "Vec<AdjustmentRainFixedGaugeTargetClaim>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainFixedGaugeTargetCaptureMember"),
      [
        ["bodySha256", "String"],
        ["claimId", "String"],
        ["completedAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainFixedGaugeTargetInterval"),
      [
        ["bodySha256", "String"],
        ["claimId", "String"],
        ["completedAt", "String"],
        ["durationMinutes", "u64"],
        ["precipitationMm64", "String"],
        ["temperatureC64", "Nullable<String>"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainFixedGaugeTargetRow"),
      [
        ["captureMembers", "Vec<AdjustmentRainFixedGaugeTargetCaptureMember>"],
        ["intervals", "Vec<AdjustmentRainFixedGaugeTargetInterval>"],
        ["normalizedRecord", "NormalizedWeatherRecord"],
        ["stationId", "u64"],
        ["storedContentSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainFixedGaugeTargetProjection"),
      [
        ["captureBodies", "Vec<AdjustmentRainFixedGaugeTargetCaptureBody>"],
        [
          "contractVersion",
          "AdjustmentRainFixedGaugeTargetProjectionContractVersionLiteral",
        ],
        ["family", "AdjustmentRainFixedGaugeTargetProjectionFamilyLiteral"],
        ["logicalReceivedAt", "String"],
        [
          "projectionKind",
          "AdjustmentRainFixedGaugeTargetProjectionProjectionKindLiteral",
        ],
        ["rows", "Vec<AdjustmentRainFixedGaugeTargetRow>"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainFixedGaugeTargetLogicalKey"),
      [
        [
          "contractVersion",
          "AdjustmentRainFixedGaugeTargetLogicalKeyContractVersionLiteral",
        ],
        ["sourceIds", "Vec<String>"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRainFixedGaugeTargetActual"),
      [
        ["firstEdgeCommittedAt", "String"],
        ["gaugeCount", "u64"],
        ["target", "f64"],
      ],
    );

    const literalCases = [
      [
        "AdjustmentRainFixedGaugeTargetProjectionContractVersionLiteral",
        "adjustment-rain-fixed-gauge-target-projection/v1",
      ],
      ["AdjustmentRainFixedGaugeTargetProjectionFamilyLiteral", "rain"],
      [
        "AdjustmentRainFixedGaugeTargetProjectionProjectionKindLiteral",
        "target_revision",
      ],
      [
        "AdjustmentRainFixedGaugeTargetLogicalKeyContractVersionLiteral",
        "adjustment-rain-fixed-gauge-target-logical-key/v1",
      ],
    ];
    // retain all direct version and target identity markers exactly
    for (const [name, wireValue] of literalCases) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: &'static str = ${JSON.stringify(wireValue)};\n}`,
        ),
        name,
      );
    }

    const utcFields = [
      ["AdjustmentRainFixedGaugeTargetClaim", "completedAt"],
      ["AdjustmentRainFixedGaugeTargetClaim", "windowEndExclusive"],
      ["AdjustmentRainFixedGaugeTargetClaim", "windowStart"],
      ["AdjustmentRainFixedGaugeTargetCaptureMember", "completedAt"],
      ["AdjustmentRainFixedGaugeTargetInterval", "completedAt"],
      ["AdjustmentRainFixedGaugeTargetInterval", "validAt"],
      ["AdjustmentRainFixedGaugeTargetProjection", "logicalReceivedAt"],
      ["AdjustmentRainFixedGaugeTargetProjection", "validAt"],
      ["AdjustmentRainFixedGaugeTargetLogicalKey", "validAt"],
      ["AdjustmentRainFixedGaugeTargetActual", "firstEdgeCommittedAt"],
    ];
    // retain utc metadata on every target document clock
    for (const [name, field] of utcFields) {
      assert.match(rustStructFieldBlock(rust, name, field), /wire rule: utc-iso/u);
    }
    // retain exact unsigned storage on each source count or physical identity
    for (const [name, field] of [
      ["AdjustmentRainFixedGaugeTargetClaim", "stationId"],
      ["AdjustmentRainFixedGaugeTargetInterval", "durationMinutes"],
      ["AdjustmentRainFixedGaugeTargetRow", "stationId"],
      ["AdjustmentRainFixedGaugeTargetActual", "gaugeCount"],
    ]) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /wire rule: safe-unsigned-integer/u,
      );
    }
    assert.match(
      rustStructFieldBlock(rust, "AdjustmentRainFixedGaugeTargetActual", "target"),
      /wire rule: finite-f64/u,
    );

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, FIXED_GAUGE_TARGET_DOCUMENT_ROOTS);
    assert.deepEqual(
      manifest.typeImports[20],
      FIXED_GAUGE_TARGET_DOMAIN_IMPORT,
    );
  },
);

test(
  "projects original maintenance evaluation rows and the capture outcome",
  // preserve runtime numbers, null states, fallback order, source fields, and status arms
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "TemperatureMaintenanceCandidateRow"),
      [
        ["candidateTemperatureC", "f64"],
        ["fallbackCode", "TemperatureMaintenanceCandidateRowFallbackCodeEnum"],
        ["validAt", "String"],
        ["wouldApply", "bool"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "WindMaintenanceCandidateRow"),
      [
        ["candidateGustMps", "Nullable<f64>"],
        ["candidateSpeedMps", "f64"],
        ["gustWouldApply", "bool"],
        ["speedWouldApply", "bool"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "TemperatureMaintenanceIncumbentRow"),
      [
        ["applied", "bool"],
        ["incumbentTemperatureC", "f64"],
        ["reasonCode", "Nullable<String>"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "WindMaintenanceIncumbentRow"),
      [
        ["gustApplied", "bool"],
        ["incumbentGustMps", "Nullable<f64>"],
        ["incumbentSpeedMps", "f64"],
        ["reasonCode", "Nullable<String>"],
        ["speedApplied", "bool"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "CaptureApiMaintenanceShadowResultPublished"),
      [
        ["predictionSha256", "String"],
        ["publishReceiptSha256", "String"],
        ["status", "CaptureApiMaintenanceShadowResultPublishedStatusLiteral"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "CaptureApiMaintenanceShadowResultGap"),
      [
        ["reason", "MaintenanceShadowGapReason"],
        ["status", "CaptureApiMaintenanceShadowResultGapStatusLiteral"],
      ],
    );
    assert.match(
      rust,
      /pub enum CaptureApiMaintenanceShadowResult \{\s*#\[doc = "wire discriminant status: \\"published\\""\]\s*Published\(CaptureApiMaintenanceShadowResultPublished\),\s*#\[doc = "wire discriminant status: \\"gap\\""\]\s*Gap\(CaptureApiMaintenanceShadowResultGap\),\s*\}/u,
    );

    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "TemperatureMaintenanceCandidateRowFallbackCodeEnum",
      ),
      [
        ["none", "None"],
        ["missing_source", "MissingSource"],
        ["ineligible", "Ineligible"],
        ["physical_cap", "PhysicalCap"],
        ["model_unavailable", "ModelUnavailable"],
      ],
    );
    const literalCases = [
      ["CaptureApiMaintenanceShadowResultPublishedStatusLiteral", "published"],
      ["CaptureApiMaintenanceShadowResultGapStatusLiteral", "gap"],
    ];
    // retain both status markers instead of widening them to strings
    for (const [name, wireValue] of literalCases) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: &'static str = ${JSON.stringify(wireValue)};\n}`,
        ),
        name,
      );
    }

    // keep evaluation numbers in the runtime binary64 domain
    for (const [name, field] of [
      ["TemperatureMaintenanceCandidateRow", "candidateTemperatureC"],
      ["WindMaintenanceCandidateRow", "candidateGustMps"],
      ["WindMaintenanceCandidateRow", "candidateSpeedMps"],
      ["TemperatureMaintenanceIncumbentRow", "incumbentTemperatureC"],
      ["WindMaintenanceIncumbentRow", "incumbentGustMps"],
      ["WindMaintenanceIncumbentRow", "incumbentSpeedMps"],
    ]) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /storage rule: runtime-f64, JavaScript number including non-finite values; not a JSON wire type/u,
      );
    }
    // retain utc metadata on the evaluated row validity clocks
    for (const name of [
      "TemperatureMaintenanceCandidateRow",
      "WindMaintenanceCandidateRow",
      "TemperatureMaintenanceIncumbentRow",
      "WindMaintenanceIncumbentRow",
    ]) {
      assert.match(rustStructFieldBlock(rust, name, "validAt"), /wire rule: utc-iso/u);
    }

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, MAINTENANCE_EVALUATION_ROOTS);
    assertManifestRootGroup(manifest.roots, FIXED_GAUGE_TARGET_DOCUMENT_ROOTS);
    assert.deepEqual(
      manifest.typeImports[20],
      FIXED_GAUGE_TARGET_DOMAIN_IMPORT,
    );
  },
);

test(
  "projects original API worker and migration readiness snapshots",
  // preserve snapshot fields, nullable clocks, readiness states, and migration version
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "HealthSnapshot"), [
      ["database", "HealthSnapshotDatabaseEnum"],
      ["migration", "HealthSnapshotMigration"],
      ["workerLastLoopAt", "Nullable<String>"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "HealthSnapshotMigration"), [
      ["status", "HealthSnapshotMigrationStatusEnum"],
      ["version", "Nullable<String>"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "WorkerHealth"), [
      ["lastLoopAt", "Nullable<String>"],
      ["lastSuccessAt", "Nullable<String>"],
      ["live", "bool"],
      ["ready", "bool"],
      ["stale", "bool"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "MigrationReadiness"), [
      ["version", "String"],
    ]);
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "HealthSnapshotDatabaseEnum"),
      [
        ["ready", "Ready"],
        ["unavailable", "Unavailable"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "HealthSnapshotMigrationStatusEnum"),
      [
        ["current", "Current"],
        ["outdated", "Outdated"],
        ["unavailable", "Unavailable"],
      ],
    );

    // retain utc metadata independently on each nullable heartbeat clock
    for (const [name, field] of [
      ["HealthSnapshot", "workerLastLoopAt"],
      ["WorkerHealth", "lastLoopAt"],
      ["WorkerHealth", "lastSuccessAt"],
    ]) {
      assert.match(rustStructFieldBlock(rust, name, field), /wire rule: utc-iso/u);
    }

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, READINESS_SNAPSHOT_ROOTS);
    assertManifestRootGroup(manifest.roots, MAINTENANCE_EVALUATION_ROOTS);
    assert.deepEqual(
      manifest.typeImports[20],
      FIXED_GAUGE_TARGET_DOMAIN_IMPORT,
    );
  },
);

test(
  "projects original worker iteration collection and diagnostic outcomes",
  // preserve result fields, runtime duration, counts, clocks, nulls, and enum order
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "SourceRunResult"), [
      ["durationMs", "f64"],
      ["recordCount", "u64"],
      ["reason", "Nullable<String>"],
      ["runId", "Nullable<String>"],
      ["secondaryError", "Nullable<String>"],
      ["sourceId", "String"],
      ["status", "SourceRunResultStatusEnum"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "WorkerIterationResult"), [
      ["completedAt", "String"],
      ["lastSuccessAt", "Nullable<String>"],
      ["sources", "Vec<SourceRunResult>"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "EcmwfTemperatureCanaryCollectionResult"),
      [
        ["failedRuns", "u64"],
        ["persistedRuns", "u64"],
        ["requestedRuns", "Vec<String>"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "RainCollectionResult"), [
      ["attempted", "u64"],
      ["valid", "u64"],
      ["failed", "u64"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "WorkerDiagnostic"), [
      ["count", "u64"],
      ["duration_ms", "u64"],
      ["error_code", "Nullable<String>"],
      ["event", "WorkerDiagnosticEvent"],
      ["release", "String"],
      ["run_id", "Nullable<String>"],
      ["source_id", "Nullable<String>"],
    ]);
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "SourceRunResultStatusEnum"),
      [
        ["failed", "Failed"],
        ["skipped", "Skipped"],
        ["succeeded", "Succeeded"],
      ],
    );
    assert.deepEqual(rustStringEnumWireEntries(rust, "WorkerDiagnosticEvent"), [
      ["source_run", "SourceRun"],
      ["worker_iteration", "WorkerIteration"],
    ]);

    assert.match(
      rustStructFieldBlock(rust, "SourceRunResult", "durationMs"),
      /storage rule: runtime-f64, JavaScript number including non-finite values; not a JSON wire type/u,
    );
    // retain utc metadata on completion and nullable successful-loop clocks
    for (const field of ["completedAt", "lastSuccessAt"]) {
      assert.match(
        rustStructFieldBlock(rust, "WorkerIterationResult", field),
        /wire rule: utc-iso/u,
      );
    }
    // retain safe unsigned storage on every original row or diagnostic count
    for (const [name, field] of [
      ["SourceRunResult", "recordCount"],
      ["EcmwfTemperatureCanaryCollectionResult", "failedRuns"],
      ["EcmwfTemperatureCanaryCollectionResult", "persistedRuns"],
      ["RainCollectionResult", "attempted"],
      ["RainCollectionResult", "valid"],
      ["RainCollectionResult", "failed"],
      ["WorkerDiagnostic", "count"],
      ["WorkerDiagnostic", "duration_ms"],
    ]) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /wire rule: safe-unsigned-integer/u,
      );
    }

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, WORKER_RESULT_ROOTS);
    assertManifestRootGroup(manifest.roots, READINESS_SNAPSHOT_ROOTS);
    assert.deepEqual(
      manifest.typeImports[20],
      FIXED_GAUGE_TARGET_DOMAIN_IMPORT,
    );
  },
);

test(
  "projects original Open Meteo adapter requests and single-run response",
  // preserve inheritance, provider fields, finite values, clocks, counts, and markers
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "OpenMeteoLocation"), [
      ["latitude", "f64"],
      ["longitude", "f64"],
      ["sourceId", "String"],
      ["timezone", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "OpenMeteoArchiveRequest"), [
      ["latitude", "f64"],
      ["longitude", "f64"],
      ["sourceId", "String"],
      ["timezone", "String"],
      ["endDate", "String"],
      ["startDate", "String"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "OpenMeteoPreviousRunsLocation"),
      [
        ["latitude", "f64"],
        ["longitude", "f64"],
        ["sourceConfigFingerprint", "String"],
        ["sourceId", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "OpenMeteoPreviousRunsRequest"),
      [
        ["contractEpoch", "String"],
        ["endDate", "String"],
        ["locations", "Vec<OpenMeteoPreviousRunsLocation>"],
        ["startDate", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "OpenMeteoEcmwfSingleRunRequest"),
      [
        ["latitude", "f64"],
        ["longitude", "f64"],
        ["runInitializedAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "OpenMeteoEcmwfSingleRunHour"),
      [
        ["modelLeadHours", "u64"],
        ["rawRelativeHumidityPercent", "Nullable<f64>"],
        ["rawTemperatureC", "f64"],
        ["rawWindSpeedMps", "Nullable<f64>"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "OpenMeteoEcmwfSingleRunBatch"),
      [
        [
          "adapterVersion",
          "OpenMeteoEcmwfSingleRunBatchAdapterVersionLiteral",
        ],
        ["attempts", "u64"],
        ["hours", "Vec<OpenMeteoEcmwfSingleRunHour>"],
        ["modelCycle", "OpenMeteoEcmwfSingleRunBatchModelCycleEnum"],
        ["providerResponseSha256", "String"],
        ["receivedAt", "String"],
        ["runInitializedAt", "String"],
        ["upstreamModel", "OpenMeteoEcmwfSingleRunBatchUpstreamModelLiteral"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "OpenMeteoEcmwfSingleRunBatchModelCycleEnum"),
      [
        ["49r1", "Value49r1"],
        ["50r1", "Value50r1"],
      ],
    );

    const literalCases = [
      [
        "OpenMeteoEcmwfSingleRunBatchAdapterVersionLiteral",
        "open-meteo-ecmwf-single-run/v1",
      ],
      ["OpenMeteoEcmwfSingleRunBatchUpstreamModelLiteral", "ecmwf_ifs"],
    ];
    // retain the original adapter and upstream model identities exactly
    for (const [name, wireValue] of literalCases) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: &'static str = ${JSON.stringify(wireValue)};\n}`,
        ),
        name,
      );
    }
    // retain finite JSON storage on every configured coordinate or observation
    for (const [name, field] of [
      ["OpenMeteoLocation", "latitude"],
      ["OpenMeteoLocation", "longitude"],
      ["OpenMeteoArchiveRequest", "latitude"],
      ["OpenMeteoArchiveRequest", "longitude"],
      ["OpenMeteoPreviousRunsLocation", "latitude"],
      ["OpenMeteoPreviousRunsLocation", "longitude"],
      ["OpenMeteoEcmwfSingleRunRequest", "latitude"],
      ["OpenMeteoEcmwfSingleRunRequest", "longitude"],
      ["OpenMeteoEcmwfSingleRunHour", "rawRelativeHumidityPercent"],
      ["OpenMeteoEcmwfSingleRunHour", "rawTemperatureC"],
      ["OpenMeteoEcmwfSingleRunHour", "rawWindSpeedMps"],
    ]) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /wire rule: finite-f64/u,
      );
    }
    // retain utc metadata on the run, hour, receipt, and response clocks
    for (const [name, field] of [
      ["OpenMeteoEcmwfSingleRunRequest", "runInitializedAt"],
      ["OpenMeteoEcmwfSingleRunHour", "validAt"],
      ["OpenMeteoEcmwfSingleRunBatch", "receivedAt"],
      ["OpenMeteoEcmwfSingleRunBatch", "runInitializedAt"],
    ]) {
      assert.match(rustStructFieldBlock(rust, name, field), /wire rule: utc-iso/u);
    }
    // retain safe unsigned storage on the lead and bounded attempt count
    for (const [name, field] of [
      ["OpenMeteoEcmwfSingleRunHour", "modelLeadHours"],
      ["OpenMeteoEcmwfSingleRunBatch", "attempts"],
    ]) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /wire rule: safe-unsigned-integer/u,
      );
    }

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, OPEN_METEO_REQUEST_ROOTS);
    assertManifestRootGroup(manifest.roots, WORKER_RESULT_ROOTS);
    assert.deepEqual(
      manifest.typeImports[20],
      FIXED_GAUGE_TARGET_DOMAIN_IMPORT,
    );
  },
);

test(
  "projects original station adapter metadata snapshots and cursor",
  // preserve provider wires, nullable cursor entries, optional readings, and station identity
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStringEnumWireEntries(rust, "ProviderCapability"), [
      ["current", "Current"],
      ["historical", "Historical"],
      ["forecast", "Forecast"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "ProviderAttribution"), [
      ["label", "String"],
      ["url", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "EcowittLiveRequest"), [
      ["expectedMac", "String"],
      ["gatewayHost", "String"],
      ["model", "String"],
      ["previousCursor", "Nullable<Vec<(Vec<u16>, JsonValue)>>"],
      ["sourceId", "String"],
      ["timezone", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "EcowittSensorReadings"), [
      ["batteryExternalPower", "FieldPresence<u64>"],
      ["batteryLevel", "FieldPresence<u64>"],
      ["batteryLow", "FieldPresence<u64>"],
      ["blackGlobeTemperatureC", "FieldPresence<f64>"],
      ["dailyPrecipitationMm", "FieldPresence<f64>"],
      ["pm25MicrogramsPerCubicMeter", "FieldPresence<f64>"],
      ["precipitationRateMmPerHour", "FieldPresence<f64>"],
      ["pressureHpa", "FieldPresence<f64>"],
      ["relativeHumidityPercent", "FieldPresence<f64>"],
      [
        "soilElectricalConductivityMicrosiemensPerCm",
        "FieldPresence<f64>",
      ],
      ["soilMoisturePercent", "FieldPresence<f64>"],
      ["solarRadiationWm2", "FieldPresence<f64>"],
      ["temperatureC", "FieldPresence<f64>"],
      ["uvIndex", "FieldPresence<f64>"],
      ["wetBulbGlobeTemperatureC", "FieldPresence<f64>"],
      ["windDirectionDegrees", "FieldPresence<f64>"],
      ["windGustMps", "FieldPresence<f64>"],
      ["windSpeedMps", "FieldPresence<f64>"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "EcowittSensorSnapshot"), [
      ["channel", "Nullable<u64>"],
      ["key", "String"],
      ["model", "String"],
      ["readings", "EcowittSensorReadings"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "ResolvedTempestStation"), [
      ["deviceId", "u64"],
      ["displayName", "String"],
      ["latitude", "f64"],
      ["locationId", "u64"],
      ["longitude", "f64"],
      ["serial", "String"],
      ["timezone", "String"],
    ]);

    assert.match(
      rustStructFieldBlock(rust, "EcowittLiveRequest", "previousCursor"),
      /wire rule: ordered-json-object/u,
    );
    // retain unsigned integer storage under optional or nullable wrappers
    for (const [name, field] of [
      ["EcowittSensorReadings", "batteryExternalPower"],
      ["EcowittSensorReadings", "batteryLevel"],
      ["EcowittSensorReadings", "batteryLow"],
      ["EcowittSensorSnapshot", "channel"],
      ["ResolvedTempestStation", "deviceId"],
      ["ResolvedTempestStation", "locationId"],
    ]) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /wire rule: safe-unsigned-integer/u,
      );
    }
    // retain finite JSON storage on every Ecowitt reading and station coordinate
    for (const [name, field] of [
      ["EcowittSensorReadings", "blackGlobeTemperatureC"],
      ["EcowittSensorReadings", "dailyPrecipitationMm"],
      ["EcowittSensorReadings", "pm25MicrogramsPerCubicMeter"],
      ["EcowittSensorReadings", "precipitationRateMmPerHour"],
      ["EcowittSensorReadings", "pressureHpa"],
      ["EcowittSensorReadings", "relativeHumidityPercent"],
      ["EcowittSensorReadings", "soilElectricalConductivityMicrosiemensPerCm"],
      ["EcowittSensorReadings", "soilMoisturePercent"],
      ["EcowittSensorReadings", "solarRadiationWm2"],
      ["EcowittSensorReadings", "temperatureC"],
      ["EcowittSensorReadings", "uvIndex"],
      ["EcowittSensorReadings", "wetBulbGlobeTemperatureC"],
      ["EcowittSensorReadings", "windDirectionDegrees"],
      ["EcowittSensorReadings", "windGustMps"],
      ["EcowittSensorReadings", "windSpeedMps"],
      ["ResolvedTempestStation", "latitude"],
      ["ResolvedTempestStation", "longitude"],
    ]) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /wire rule: finite-f64/u,
      );
    }

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, STATION_ADAPTER_ROOTS);
    assertManifestRootGroup(manifest.roots, OPEN_METEO_REQUEST_ROOTS);
    assert.deepEqual(
      manifest.typeImports[20],
      FIXED_GAUGE_TARGET_DOMAIN_IMPORT,
    );
    assert.deepEqual(manifest.typeImports[21], ECOWITT_DOMAIN_IMPORT);
  },
);

test(
  "projects original NOAA tide observation and prediction requests",
  // preserve the private base, flat arm fields, exact markers, and union order
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "NoaaTideRangeBase"), [
      ["datum", "NoaaTideRangeBaseDatumLiteral"],
      ["endExclusive", "String"],
      ["sourceId", "String"],
      ["start", "String"],
      ["stationId", "String"],
      ["timezone", "String"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "NoaaTideObservationRequest"),
      [
        ["datum", "NoaaTideRangeBaseDatumLiteral"],
        ["endExclusive", "String"],
        ["sourceId", "String"],
        ["start", "String"],
        ["stationId", "String"],
        ["timezone", "String"],
        ["product", "NoaaTideObservationRequestProductLiteral"],
        ["sourceKind", "NoaaTideObservationRequestSourceKindLiteral"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "NoaaTidePredictionRequest"),
      [
        ["datum", "NoaaTideRangeBaseDatumLiteral"],
        ["endExclusive", "String"],
        ["sourceId", "String"],
        ["start", "String"],
        ["stationId", "String"],
        ["timezone", "String"],
        ["interval", "NoaaTidePredictionRequestIntervalLiteral"],
        ["product", "NoaaTidePredictionRequestProductLiteral"],
        ["sourceKind", "NoaaTidePredictionRequestSourceKindLiteral"],
      ],
    );
    assert.match(
      rust,
      /pub enum NoaaTideRangeRequest \{\s*Observation\(NoaaTideObservationRequest\),\s*Prediction\(NoaaTidePredictionRequest\),\s*\}/u,
    );

    const literalCases = [
      ["NoaaTideRangeBaseDatumLiteral", "MLLW"],
      ["NoaaTideObservationRequestProductLiteral", "water_level"],
      [
        "NoaaTideObservationRequestSourceKindLiteral",
        "tide_observation",
      ],
      ["NoaaTidePredictionRequestIntervalLiteral", "hilo"],
      ["NoaaTidePredictionRequestProductLiteral", "predictions"],
      ["NoaaTidePredictionRequestSourceKindLiteral", "tide_prediction"],
    ];
    // retain all six direct literal identities without duplicating the base marker
    for (const [name, wireValue] of literalCases) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: &'static str = ${JSON.stringify(wireValue)};\n}`,
        ),
        name,
      );
    }
    // reuse every complete inherited field block on both flattened request arms
    for (const field of [
      "datum",
      "endExclusive",
      "sourceId",
      "start",
      "stationId",
      "timezone",
    ]) {
      const baseField = rustStructFieldBlock(rust, "NoaaTideRangeBase", field);
      assert.equal(
        rustStructFieldBlock(rust, "NoaaTideObservationRequest", field),
        baseField,
      );
      assert.equal(
        rustStructFieldBlock(rust, "NoaaTidePredictionRequest", field),
        baseField,
      );
    }
    assert.equal(
      rustStructFieldBlock(rust, "NoaaTideObservationRequest", "datum"),
      "    pub datum: NoaaTideRangeBaseDatumLiteral,",
    );
    // keep Date.parse-compatible request strings free of an ISO-only wire rule
    for (const name of [
      "NoaaTideRangeBase",
      "NoaaTideObservationRequest",
      "NoaaTidePredictionRequest",
    ]) {
      const definition = rust.match(
        new RegExp(`^pub struct ${name} \\{\\n[\\s\\S]+?^\\}`, "mu"),
      );
      assert.ok(definition, name);
      assert.doesNotMatch(definition[0], /wire rule: utc-iso/u);
    }

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, NOAA_TIDE_REQUEST_ROOTS);
    assertManifestRootGroup(manifest.roots, STATION_ADAPTER_ROOTS);
    assert.deepEqual(manifest.typeImports[21], ECOWITT_DOMAIN_IMPORT);
  },
);

test(
  "projects original tide backfill arguments chunks and per-source reports",
  // preserve nullable paths, imported chunk identity, exact exits, counts, and report rows
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "TideBackfillArguments"), [
      ["dryRun", "bool"],
      ["from", "String"],
      ["reportPath", "Nullable<String>"],
      ["resume", "bool"],
      ["site", "String"],
      ["sourceKeys", "Vec<String>"],
      ["to", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "PlannedTideChunk"), [
      ["endDate", "String"],
      ["identity", "BackfillChunkIdentity"],
      ["startDate", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "TideBackfillReport"), [
      ["dryRun", "bool"],
      ["exitCode", "TideBackfillReportExitCodeEnum"],
      ["resume", "bool"],
      ["site", "String"],
      ["sources", "Vec<TideBackfillSourceReport>"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "TideBackfillSourceReport"),
      [
        ["completedChunks", "u64"],
        ["exitCode", "TideBackfillSourceReportExitCodeEnum"],
        ["failedChunk", "Nullable<TideBackfillSourceReportFailedChunk>"],
        ["from", "String"],
        ["plannedChunks", "u64"],
        ["records", "u64"],
        ["skippedChunks", "u64"],
        ["source", "String"],
        ["to", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "TideBackfillSourceReportFailedChunk"),
      [
        ["errorCode", "String"],
        ["startDate", "String"],
      ],
    );
    assert.deepEqual(
      rustUnsignedIntegerEnumWireEntries(rust, "TideBackfillReportExitCodeEnum"),
      [
        [0, "Value0"],
        [1, "Value1"],
      ],
    );
    assert.deepEqual(
      rustUnsignedIntegerEnumWireEntries(
        rust,
        "TideBackfillSourceReportExitCodeEnum",
      ),
      [
        [0, "Value0"],
        [1, "Value1"],
      ],
    );
    // retain the safe integer enum boundary on both exit statuses
    for (const name of ["TideBackfillReport", "TideBackfillSourceReport"]) {
      assert.match(
        rustStructFieldBlock(rust, name, "exitCode"),
        /wire rule: unsigned-safe-integer-enum/u,
      );
    }
    assert.match(
      rustStructFieldBlock(rust, "TideBackfillSourceReport", "failedChunk"),
      /wire rule: readonly-inline-object/u,
    );
    // retain reviewed unsigned storage on every per-source progress count
    for (const field of [
      "completedChunks",
      "plannedChunks",
      "records",
      "skippedChunks",
    ]) {
      assert.match(
        rustStructFieldBlock(rust, "TideBackfillSourceReport", field),
        /wire rule: safe-unsigned-integer/u,
      );
    }
    // keep all accepted backfill dates as ordinary strings rather than ISO-only clocks
    for (const [name, field] of [
      ["TideBackfillArguments", "from"],
      ["TideBackfillArguments", "to"],
      ["PlannedTideChunk", "endDate"],
      ["PlannedTideChunk", "startDate"],
      ["TideBackfillSourceReport", "from"],
      ["TideBackfillSourceReport", "to"],
      ["TideBackfillSourceReportFailedChunk", "startDate"],
    ]) {
      assert.doesNotMatch(
        rustStructFieldBlock(rust, name, field),
        /wire rule: utc-iso/u,
      );
    }

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, TIDE_BACKFILL_ROOTS);
    assertManifestRootGroup(manifest.roots, NOAA_TIDE_REQUEST_ROOTS);
    assert.deepEqual(manifest.typeImports[21], ECOWITT_DOMAIN_IMPORT);
    assert.deepEqual(manifest.typeImports[22], TIDE_BACKFILL_DOMAIN_IMPORT);
  },
);

test(
  "projects original general Tempest and public station backfill data",
  // preserve all three backfill graphs, distinct statuses, counts, nulls, and dates
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const structCases = [
      ["BackfillArguments", [
        ["chunkDays", "u64"],
        ["dryRun", "bool"],
        ["from", "String"],
        ["reportPath", "Nullable<String>"],
        ["resume", "bool"],
        ["site", "String"],
        ["source", "Nullable<String>"],
        ["to", "String"],
      ]],
      ["PlannedBackfillChunk", [
        ["endDate", "String"],
        ["identity", "BackfillChunkIdentity"],
        ["key", "String"],
        ["startDate", "String"],
      ]],
      ["BackfillChunkReport", [
        ["errorCode", "Nullable<String>"],
        ["identity", "BackfillChunkIdentity"],
        ["secondaryError", "Nullable<String>"],
        ["status", "BackfillChunkReportStatusEnum"],
      ]],
      ["BackfillReport", [
        ["chunks", "Vec<BackfillChunkReport>"],
        ["dryRun", "bool"],
        ["exitCode", "BackfillReportExitCodeEnum"],
        ["from", "String"],
        ["resume", "bool"],
        ["site", "String"],
        ["source", "String"],
        ["to", "String"],
      ]],
      ["TempestBackfillArguments", [
        ["chunkDays", "u64"],
        ["dryRun", "bool"],
        ["from", "Nullable<String>"],
        ["reportPath", "Nullable<String>"],
        ["resume", "bool"],
        ["site", "String"],
        ["stationIds", "Vec<u64>"],
        ["to", "String"],
      ]],
      ["PlannedTempestChunk", [
        ["endDate", "String"],
        ["identity", "BackfillChunkIdentity"],
        ["key", "String"],
        ["startDate", "String"],
      ]],
      ["TempestBackfillChunkReport", [
        ["errorCode", "Nullable<String>"],
        ["identity", "BackfillChunkIdentity"],
        ["recordCount", "u64"],
        ["secondaryError", "Nullable<String>"],
        ["status", "TempestBackfillChunkReportStatusEnum"],
      ]],
      ["TempestStationBackfillReport", [
        ["chunks", "Vec<TempestBackfillChunkReport>"],
        ["exitCode", "TempestStationBackfillReportExitCodeEnum"],
        ["from", "String"],
        ["locationId", "u64"],
        ["source", "String"],
        ["to", "String"],
      ]],
      ["TempestBulkBackfillReport", [
        ["dryRun", "bool"],
        ["exitCode", "TempestBulkBackfillReportExitCodeEnum"],
        ["resume", "bool"],
        ["site", "String"],
        ["stations", "Vec<TempestStationBackfillReport>"],
      ]],
      ["PublicStationBackfillArguments", [
        ["dryRun", "bool"],
        ["from", "Nullable<String>"],
        ["reportPath", "Nullable<String>"],
        ["resume", "bool"],
        ["site", "String"],
        ["sourceKeys", "Vec<String>"],
        ["to", "String"],
      ]],
      ["PlannedPublicStationChunk", [
        ["endDate", "String"],
        ["identity", "BackfillChunkIdentity"],
        ["startDate", "String"],
      ]],
      ["PublicStationBackfillSourceReport", [
        ["completedChunks", "u64"],
        ["exitCode", "PublicStationBackfillSourceReportExitCodeEnum"],
        [
          "failedChunk",
          "Nullable<PublicStationBackfillSourceReportFailedChunk>",
        ],
        ["from", "String"],
        ["plannedChunks", "u64"],
        ["records", "u64"],
        ["skippedChunks", "u64"],
        ["source", "String"],
        ["to", "String"],
      ]],
      ["PublicStationBackfillReport", [
        ["dryRun", "bool"],
        ["exitCode", "PublicStationBackfillReportExitCodeEnum"],
        ["resume", "bool"],
        ["site", "String"],
        ["sources", "Vec<PublicStationBackfillSourceReport>"],
      ]],
      ["PublicStationBackfillSourceReportFailedChunk", [
        ["errorCode", "String"],
        ["startDate", "String"],
      ]],
    ];
    // compare every declaration and the pinned nested failure in exact source order
    for (const [name, fields] of structCases) {
      assert.deepEqual(rustStructFieldEntries(rust, name), fields);
    }

    assert.deepEqual(
      rustStringEnumWireEntries(rust, "BackfillChunkReportStatusEnum"),
      [
        ["abandoned", "Abandoned"],
        ["completed", "Completed"],
        ["failed", "Failed"],
        ["planned", "Planned"],
        ["remaining", "Remaining"],
        ["skipped", "Skipped"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "TempestBackfillChunkReportStatusEnum"),
      [
        ["abandoned", "Abandoned"],
        ["completed", "Completed"],
        ["failed", "Failed"],
        ["planned", "Planned"],
        ["skipped", "Skipped"],
      ],
    );
    const exitEnums = [
      ["BackfillReport", "BackfillReportExitCodeEnum"],
      [
        "TempestStationBackfillReport",
        "TempestStationBackfillReportExitCodeEnum",
      ],
      ["TempestBulkBackfillReport", "TempestBulkBackfillReportExitCodeEnum"],
      [
        "PublicStationBackfillSourceReport",
        "PublicStationBackfillSourceReportExitCodeEnum",
      ],
      [
        "PublicStationBackfillReport",
        "PublicStationBackfillReportExitCodeEnum",
      ],
    ];
    // retain the same closed safe 0/1 exit contract on all report levels
    for (const [name, enumName] of exitEnums) {
      assert.deepEqual(rustUnsignedIntegerEnumWireEntries(rust, enumName), [
        [0, "Value0"],
        [1, "Value1"],
      ]);
      assert.match(
        rustStructFieldBlock(rust, name, "exitCode"),
        /wire rule: unsigned-safe-integer-enum/u,
      );
    }
    // retain reviewed unsigned storage for arguments, stations, and progress counts
    for (const [name, field] of [
      ["BackfillArguments", "chunkDays"],
      ["TempestBackfillArguments", "chunkDays"],
      ["TempestBackfillArguments", "stationIds"],
      ["TempestBackfillChunkReport", "recordCount"],
      ["TempestStationBackfillReport", "locationId"],
      ["PublicStationBackfillSourceReport", "completedChunks"],
      ["PublicStationBackfillSourceReport", "plannedChunks"],
      ["PublicStationBackfillSourceReport", "records"],
      ["PublicStationBackfillSourceReport", "skippedChunks"],
    ]) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /wire rule: safe-unsigned-integer/u,
      );
    }
    assert.match(
      rustStructFieldBlock(
        rust,
        "PublicStationBackfillSourceReport",
        "failedChunk",
      ),
      /wire rule: readonly-inline-object/u,
    );
    // keep every accepted date as its original plain or nullable string
    for (const [name, field] of [
      ["BackfillArguments", "from"],
      ["BackfillArguments", "to"],
      ["PlannedBackfillChunk", "endDate"],
      ["PlannedBackfillChunk", "startDate"],
      ["BackfillReport", "from"],
      ["BackfillReport", "to"],
      ["TempestBackfillArguments", "from"],
      ["TempestBackfillArguments", "to"],
      ["PlannedTempestChunk", "endDate"],
      ["PlannedTempestChunk", "startDate"],
      ["TempestStationBackfillReport", "from"],
      ["TempestStationBackfillReport", "to"],
      ["PublicStationBackfillArguments", "from"],
      ["PublicStationBackfillArguments", "to"],
      ["PlannedPublicStationChunk", "endDate"],
      ["PlannedPublicStationChunk", "startDate"],
      ["PublicStationBackfillSourceReport", "from"],
      ["PublicStationBackfillSourceReport", "to"],
      ["PublicStationBackfillSourceReportFailedChunk", "startDate"],
    ]) {
      assert.doesNotMatch(
        rustStructFieldBlock(rust, name, field),
        /wire rule: utc-iso/u,
      );
    }

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, BACKFILL_DATA_ROOTS);
    assertManifestRootGroup(manifest.roots, TIDE_BACKFILL_ROOTS);
    assert.deepEqual(manifest.typeImports[22], TIDE_BACKFILL_DOMAIN_IMPORT);
    assert.deepEqual(manifest.typeImports[23], BACKFILL_DOMAIN_IMPORT);
    assert.deepEqual(manifest.typeImports[24], TEMPEST_BACKFILL_DOMAIN_IMPORT);
    assert.deepEqual(
      manifest.typeImports[25],
      PUBLIC_STATION_BACKFILL_DOMAIN_IMPORT,
    );
  },
);

test(
  "projects the original version one widget forecast snapshot exactly",
  // preserve the closed widget modes, nested response, clocks, literals, and float bits
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStringEnumWireEntries(rust, "WidgetForecastMode"), [
      ["adjusted", "Adjusted"],
      ["raw", "Raw"],
      ["unavailable", "Unavailable"],
    ]);
    assert.deepEqual(rustStringEnumWireEntries(rust, "WidgetForecastReason"), [
      ["deadline_expired", "DeadlineExpired"],
      ["deadline_unavailable", "DeadlineUnavailable"],
      ["generic_adjustment", "GenericAdjustment"],
      ["independent_adjustment", "IndependentAdjustment"],
      ["missing", "Missing"],
      ["rain_adjustment", "RainAdjustment"],
      ["raw_forecast", "RawForecast"],
    ]);
    assert.deepEqual(rustStringEnumWireEntries(rust, "WidgetForecastStatus"), [
      ["adjusted", "Adjusted"],
      ["raw", "Raw"],
      ["unavailable", "Unavailable"],
      ["mixed", "Mixed"],
    ]);

    const structCases = [
      ["WidgetForecastSource", [
        ["runAt", "Nullable<String>"],
        ["receivedAt", "String"],
      ]],
      ["WidgetForecastField", [
        ["mode", "WidgetForecastMode"],
        ["raw", "Nullable<f64>"],
        ["rawSource", "Nullable<WidgetForecastSource>"],
        ["reason", "WidgetForecastReason"],
        ["selected", "Nullable<f64>"],
        ["selectedSource", "Nullable<WidgetForecastSource>"],
        ["selectedUntil", "Nullable<String>"],
      ]],
      ["WidgetForecastHour", [
        ["end", "String"],
        ["rainMmPerHour", "WidgetForecastField"],
        ["start", "String"],
        ["temperatureC", "WidgetForecastField"],
      ]],
      ["WidgetForecastSnapshotAttribution", [
        ["label", "WidgetForecastSnapshotAttributionLabelLiteral"],
        [
          "licenseUrl",
          "WidgetForecastSnapshotAttributionLicenseUrlLiteral",
        ],
        [
          "providerUrl",
          "WidgetForecastSnapshotAttributionProviderUrlLiteral",
        ],
      ]],
      ["WidgetForecastSnapshotCalendar", [
        ["cutoff", "String"],
        ["date", "String"],
        ["dayEnd", "String"],
        ["dayStart", "String"],
        ["sunset", "Nullable<String>"],
      ]],
      ["WidgetForecastSnapshotSite", [
        ["latitude", "WidgetForecastSnapshotSiteLatitudeLiteral"],
        ["longitude", "WidgetForecastSnapshotSiteLongitudeLiteral"],
        ["name", "WidgetForecastSnapshotSiteNameLiteral"],
        ["slug", "WidgetForecastSnapshotSiteSlugLiteral"],
        ["timezone", "WidgetForecastSnapshotSiteTimezoneLiteral"],
      ]],
      ["WidgetForecastSnapshot", [
        ["attribution", "WidgetForecastSnapshotAttribution"],
        ["calendar", "WidgetForecastSnapshotCalendar"],
        ["generatedAt", "String"],
        ["hours", "Vec<WidgetForecastHour>"],
        ["receivedAt", "String"],
        ["schemaVersion", "WidgetForecastSnapshotSchemaVersionLiteral"],
        ["site", "WidgetForecastSnapshotSite"],
        ["status", "WidgetForecastStatus"],
      ]],
    ];
    // compare every public and nested widget structure in exact source order
    for (const [name, fields] of structCases) {
      assert.deepEqual(rustStructFieldEntries(rust, name), fields);
    }

    const stringLiteralCases = [
      [
        "WidgetForecastSnapshotAttributionLabelLiteral",
        "Open-Meteo · CC BY 4.0",
      ],
      [
        "WidgetForecastSnapshotAttributionLicenseUrlLiteral",
        "https://creativecommons.org/licenses/by/4.0/",
      ],
      [
        "WidgetForecastSnapshotAttributionProviderUrlLiteral",
        "https://open-meteo.com/",
      ],
      ["WidgetForecastSnapshotSchemaVersionLiteral", "weather-widget/v1"],
      ["WidgetForecastSnapshotSiteNameLiteral", "Ballydidean"],
      ["WidgetForecastSnapshotSiteSlugLiteral", "ballydidean"],
      [
        "WidgetForecastSnapshotSiteTimezoneLiteral",
        "America/Los_Angeles",
      ],
    ];
    // retain every direct and const-backed string marker exactly
    for (const [name, wireValue] of stringLiteralCases) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: &'static str = ${JSON.stringify(wireValue)};\n}`,
        ),
        name,
      );
    }
    const coordinateCases = [
      ["WidgetForecastSnapshotSiteLatitudeLiteral", 47.950429954185445],
      ["WidgetForecastSnapshotSiteLongitudeLiteral", -122.42797012608193],
    ];
    // derive independent IEEE-754 bit strings with Node's big-endian buffer writer
    for (const [name, wireValue] of coordinateCases) {
      const bytes = Buffer.allocUnsafe(8);
      bytes.writeDoubleBE(wireValue);
      const bits = bytes.toString("hex");
      assert.match(bits, /^[0-9a-f]{16}$/u);
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: f64 = f64::from_bits(0x${bits});\n}`,
        ),
        name,
      );
    }
    // retain the scalar-property provenance on all five frozen site members
    for (const field of ["latitude", "longitude", "name", "slug", "timezone"]) {
      assert.match(
        rustStructFieldBlock(rust, "WidgetForecastSnapshotSite", field),
        /wire rule: const-scalar-property/u,
      );
    }
    // retain finite nullable weather values independently of unavailable sources
    for (const field of ["raw", "selected"]) {
      assert.match(
        rustStructFieldBlock(rust, "WidgetForecastField", field),
        /wire rule: finite-f64/u,
      );
    }
    // retain every actual instant while leaving the civil calendar date plain
    for (const [name, field] of [
      ["WidgetForecastSource", "runAt"],
      ["WidgetForecastSource", "receivedAt"],
      ["WidgetForecastField", "selectedUntil"],
      ["WidgetForecastHour", "end"],
      ["WidgetForecastHour", "start"],
      ["WidgetForecastSnapshotCalendar", "cutoff"],
      ["WidgetForecastSnapshotCalendar", "dayEnd"],
      ["WidgetForecastSnapshotCalendar", "dayStart"],
      ["WidgetForecastSnapshotCalendar", "sunset"],
      ["WidgetForecastSnapshot", "generatedAt"],
      ["WidgetForecastSnapshot", "receivedAt"],
    ]) {
      assert.match(rustStructFieldBlock(rust, name, field), /wire rule: utc-iso/u);
    }
    assert.doesNotMatch(
      rustStructFieldBlock(rust, "WidgetForecastSnapshotCalendar", "date"),
      /wire rule: utc-iso/u,
    );

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, WIDGET_FORECAST_ROOTS);
    assertManifestRootGroup(manifest.roots, BACKFILL_DATA_ROOTS);
    assert.deepEqual(
      manifest.typeImports[25],
      PUBLIC_STATION_BACKFILL_DOMAIN_IMPORT,
    );
  },
);

test(
  "admits only exact widget-style scalar const properties and one local enum extension",
  // admit one direct frozen scalar object and one direct local enum extension only
  () => {
    const source = `const SITE = {
  latitude: 47.25,
  longitude: -122.5,
  name: "Example",
} as const;
export type Mode = "first" | "second";
export type Status = Mode | "mixed";
export interface Root {
  readonly latitude: typeof SITE.latitude;
  readonly longitude: typeof SITE.longitude;
  readonly name: typeof SITE.name;
}
`;
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Mode", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Status",
          catalog: { kind: "extend-local-string-enum", base: "Mode" },
          fields: {},
        },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {
            latitude: "const-scalar-property",
            longitude: "const-scalar-property",
            name: "const-scalar-property",
          },
        },
      ],
    };
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStringEnumWireEntries(rust, "Status"), [
      ["first", "First"],
      ["second", "Second"],
      ["mixed", "Mixed"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["latitude", "RootLatitudeLiteral"],
      ["longitude", "RootLongitudeLiteral"],
      ["name", "RootNameLiteral"],
    ]);
    assert.ok(
      rust.includes(
        'impl RootNameLiteral {\n    pub const WIRE_VALUE: &\'static str = "Example";\n}',
      ),
    );
    // compare both numeric markers through an independent IEEE-754 encoding path
    for (const [name, wireValue] of [
      ["RootLatitudeLiteral", 47.25],
      ["RootLongitudeLiteral", -122.5],
    ]) {
      const bytes = Buffer.allocUnsafe(8);
      bytes.writeDoubleBE(wireValue);
      const bits = bytes.toString("hex");
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: f64 = f64::from_bits(0x${bits});\n}`,
        ),
        name,
      );
    }
    // retain the opt-in documentation on each exact scalar property query
    for (const field of ["latitude", "longitude", "name"]) {
      assert.match(
        rustStructFieldBlock(rust, "Root", field),
        /wire rule: const-scalar-property/u,
      );
    }
    assert.deepEqual(manifest.roots, [
      {
        source: "fixture.ts",
        exportName: "Mode",
        rustName: "Mode",
        kind: "string-literal-enum",
      },
      {
        source: "fixture.ts",
        exportName: "Status",
        rustName: "Status",
        kind: "string-literal-enum",
        catalog: { kind: "extend-local-string-enum", base: "Mode" },
      },
      {
        source: "fixture.ts",
        exportName: "Root",
        rustName: "Root",
        kind: "readonly-interface",
      },
    ]);

    const scalarConfig = fixtureConfig("Root", {
      value: "const-scalar-property",
    });
    const invalidScalarSources = [
      [
        "an object without as const",
        `const SITE = { value: "first" };
export interface Root { readonly value: typeof SITE.value; }
`,
      ],
      [
        "an aliased scalar initializer",
        `const VALUE = "first" as const;
const SITE = { value: VALUE } as const;
export interface Root { readonly value: typeof SITE.value; }
`,
      ],
      [
        "a computed scalar member",
        `const SITE = { ["value"]: "first" } as const;
export interface Root { readonly value: typeof SITE.value; }
`,
      ],
      [
        "a spread scalar member",
        `const BASE = { other: "first" } as const;
const SITE = { ...BASE, value: "second" } as const;
export interface Root { readonly value: typeof SITE.value; }
`,
      ],
      [
        "duplicate scalar members",
        `const SITE = { value: "first", value: "second" } as const;
export interface Root { readonly value: typeof SITE.value; }
`,
      ],
      [
        "a scalar expression",
        `const SITE = { value: 47 + 0.25 } as const;
export interface Root { readonly value: typeof SITE.value; }
`,
      ],
      [
        "an exponential numeric spelling",
        `const SITE = { value: 1e2 } as const;
export interface Root { readonly value: typeof SITE.value; }
`,
      ],
      [
        "a non-scalar member",
        `const SITE = { value: { nested: "first" } } as const;
export interface Root { readonly value: typeof SITE.value; }
`,
      ],
    ];
    // reject every widened, redirected, computed, repeated, or non-scalar object form
    for (const [name, invalidSource] of invalidScalarSources) {
      assert.throws(
        // exercise the same selected property under each invalid object shape
        () => generateRustContracts(
          scalarConfig,
          { "fixture.ts": invalidSource },
        ),
        /const[- ]scalar[- ]property.*(?:requires|unsupported)/u,
        name,
      );
    }

    const invalidExtensionCases = [
      {
        name: "a new literal before the base",
        source: `export type Mode = "first" | "second";
export type Status = "mixed" | Mode;
`,
        roots: config.roots.slice(0, 2),
      },
      {
        name: "more than one new literal",
        source: `export type Mode = "first" | "second";
export type Status = Mode | "mixed" | "other";
`,
        roots: config.roots.slice(0, 2),
      },
      {
        name: "an indirect base alias",
        source: `export type Mode = "first" | "second";
type Alias = Mode;
export type Status = Alias | "mixed";
`,
        roots: config.roots.slice(0, 2),
      },
      {
        name: "an unconfigured base",
        source: `export type Mode = "first" | "second";
export type Status = Mode | "mixed";
`,
        roots: [config.roots[1]],
      },
      {
        name: "a const-tuple base instead of a direct enum",
        source: `const MODES = ["first", "second"] as const;
export type Mode = (typeof MODES)[number];
export type Status = Mode | "mixed";
`,
        roots: config.roots.slice(0, 2),
      },
      {
        name: "an imported base from another source",
        sources: {
          "base.ts": 'export type Mode = "first" | "second";\n',
          "fixture.ts": 'import type { Mode } from "./base.js";\nexport type Status = Mode | "mixed";\n',
        },
        roots: [
          { source: "base.ts", exportName: "Mode", fields: {} },
          {
            source: "fixture.ts",
            exportName: "Status",
            catalog: { kind: "extend-local-string-enum", base: "Mode" },
            fields: {},
          },
        ],
      },
    ];
    // reject every alias shape except one local configured enum followed by one literal
    for (const invalid of invalidExtensionCases) {
      assert.throws(
        // preserve each case's exact source graph while varying only the alias shape
        () => generateRustContracts(
          {
            version: 1,
            typescriptVersion: "6.0.3",
            roots: invalid.roots,
          },
          invalid.sources ?? { "fixture.ts": invalid.source },
        ),
        /(?:extend-local-string-enum|extended local string enum|local string enum extension).*(?:requires|unsupported|configured|same-source)/u,
        invalid.name,
      );
    }
  },
);

test(
  "projects the original version two and three widget forecasts exactly",
  // preserve imported inheritance, exact omissions, calendar extension, and versions
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "WidgetForecastV2Hour"), [
      ["end", "String"],
      ["rainMmPerHour", "WidgetForecastField"],
      ["start", "String"],
      ["temperatureC", "WidgetForecastField"],
      ["cloudCoverPercent", "WidgetForecastField"],
      ["windSpeedMps", "WidgetForecastField"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "WidgetForecastV2Snapshot"), [
      ["attribution", "WidgetForecastSnapshotAttribution"],
      ["calendar", "WidgetForecastSnapshotCalendar"],
      ["generatedAt", "String"],
      ["receivedAt", "String"],
      ["site", "WidgetForecastSnapshotSite"],
      ["status", "WidgetForecastStatus"],
      ["hours", "Vec<WidgetForecastV2Hour>"],
      ["schemaVersion", "WidgetForecastV2SnapshotSchemaVersionLiteral"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "WidgetForecastV3Snapshot"), [
      ["attribution", "WidgetForecastSnapshotAttribution"],
      ["generatedAt", "String"],
      ["receivedAt", "String"],
      ["site", "WidgetForecastSnapshotSite"],
      ["status", "WidgetForecastStatus"],
      ["hours", "Vec<WidgetForecastV2Hour>"],
      ["calendar", "WidgetForecastV3SnapshotCalendar"],
      ["schemaVersion", "WidgetForecastV3SnapshotSchemaVersionLiteral"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "WidgetForecastV3SnapshotCalendar"),
      [
        ["cutoff", "String"],
        ["date", "String"],
        ["dayEnd", "String"],
        ["dayStart", "String"],
        ["sunset", "Nullable<String>"],
        ["overnightEnd", "String"],
      ],
    );

    // retain every complete v1 hour field block before the two v2 additions
    for (const field of ["end", "rainMmPerHour", "start", "temperatureC"]) {
      assert.equal(
        rustStructFieldBlock(rust, "WidgetForecastV2Hour", field),
        rustStructFieldBlock(rust, "WidgetForecastHour", field),
      );
    }
    // retain all nonreplaced v1 snapshot blocks in the v2 flattened base
    for (const field of [
      "attribution",
      "calendar",
      "generatedAt",
      "receivedAt",
      "site",
      "status",
    ]) {
      assert.equal(
        rustStructFieldBlock(rust, "WidgetForecastV2Snapshot", field),
        rustStructFieldBlock(rust, "WidgetForecastSnapshot", field),
      );
    }
    // remove only calendar and schemaVersion while retaining every other v2 block
    for (const field of [
      "attribution",
      "generatedAt",
      "receivedAt",
      "site",
      "status",
      "hours",
    ]) {
      assert.equal(
        rustStructFieldBlock(rust, "WidgetForecastV3Snapshot", field),
        rustStructFieldBlock(rust, "WidgetForecastV2Snapshot", field),
      );
    }
    // copy the original calendar blocks byte for byte before overnightEnd
    for (const field of ["cutoff", "date", "dayEnd", "dayStart", "sunset"]) {
      assert.equal(
        rustStructFieldBlock(rust, "WidgetForecastV3SnapshotCalendar", field),
        rustStructFieldBlock(rust, "WidgetForecastSnapshotCalendar", field),
      );
    }
    assert.match(
      rustStructFieldBlock(rust, "WidgetForecastV3Snapshot", "calendar"),
      /field projection: indexed-interface-field-extension -> WidgetForecastSnapshot/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "WidgetForecastV3SnapshotCalendar",
        "overnightEnd",
      ),
      /wire rule: utc-iso/u,
    );

    const versionCases = [
      ["WidgetForecastV2SnapshotSchemaVersionLiteral", "weather-widget/v2"],
      ["WidgetForecastV3SnapshotSchemaVersionLiteral", "weather-widget/v3"],
    ];
    // retain the exact version markers owned by each replacement declaration
    for (const [name, wireValue] of versionCases) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: &'static str = ${JSON.stringify(wireValue)};\n}`,
        ),
        name,
      );
    }

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, WIDGET_FORECAST_EXTENSION_ROOTS);
    assertManifestRootGroup(manifest.roots, WIDGET_FORECAST_ROOTS);
    assert.deepEqual(manifest.typeImports[26], WIDGET_FORECAST_V2_IMPORT);
    assert.deepEqual(manifest.typeImports[27], WIDGET_FORECAST_V3_V1_IMPORT);
    assert.deepEqual(manifest.typeImports[28], WIDGET_FORECAST_V3_V2_IMPORT);
  },
);

test(
  "admits only exact imported bases and indexed inline extensions",
  // admit one named type-only base, one indexed field extension, and one inherited omission
  () => {
    const sources = {
      "base.ts": `export interface Base {
  readonly receivedAt: string;
}
export interface Snapshot {
  readonly calendar: {
    readonly cutoff: string;
    readonly date: string;
  };
  readonly other: {
    readonly value: string;
  };
}
export interface Ancestor {
  readonly inherited: string;
  readonly retained: string;
}
export interface Middle extends Ancestor {
  readonly own: string;
}
`,
      "child.ts": `import type { Base, Middle, Snapshot } from "./base.js";
export interface Child extends Base {
  readonly status: string;
}
export interface Root {
  readonly calendar: Snapshot["calendar"] & {
    readonly overnightEnd: string;
  };
}
export interface Replaced extends Omit<Middle, "inherited"> {
  readonly inherited: boolean;
}
`,
    };
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        {
          source: "base.ts",
          exportName: "Base",
          fields: { receivedAt: "utc-iso" },
        },
        {
          source: "base.ts",
          exportName: "Snapshot",
          fields: { "calendar.cutoff": "utc-iso" },
        },
        { source: "base.ts", exportName: "Ancestor", fields: {} },
        {
          source: "base.ts",
          exportName: "Middle",
          base: "Ancestor",
          fields: {},
        },
        {
          source: "child.ts",
          exportName: "Child",
          base: "Base",
          importedBase: true,
          fields: {},
        },
        {
          source: "child.ts",
          exportName: "Root",
          fields: { "calendar.overnightEnd": "utc-iso" },
          fieldProjections: {
            calendar: {
              kind: "indexed-interface-field-extension",
              root: "Snapshot",
            },
          },
        },
        {
          source: "child.ts",
          exportName: "Replaced",
          base: "Middle",
          omitBaseFields: ["inherited"],
          omitInheritedBaseFields: ["inherited"],
          fields: {},
        },
      ],
      typeImports: [{
        consumer: "child.ts",
        specifier: "./base.js",
        entry: "base.ts",
        exports: {
          Base: "base.ts",
          Middle: "base.ts",
          Snapshot: "base.ts",
        },
      }],
    };
    const { rust, manifest: manifestText } = generateRustContracts(config, sources);
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Child"), [
      ["receivedAt", "String"],
      ["status", "String"],
    ]);
    assert.equal(
      rustStructFieldBlock(rust, "Child", "receivedAt"),
      rustStructFieldBlock(rust, "Base", "receivedAt"),
    );
    assert.deepEqual(rustStructFieldEntries(rust, "RootCalendar"), [
      ["cutoff", "String"],
      ["date", "String"],
      ["overnightEnd", "String"],
    ]);
    assert.equal(
      rustStructFieldBlock(rust, "RootCalendar", "cutoff"),
      rustStructFieldBlock(rust, "SnapshotCalendar", "cutoff"),
    );
    assert.equal(
      rustStructFieldBlock(rust, "RootCalendar", "date"),
      rustStructFieldBlock(rust, "SnapshotCalendar", "date"),
    );
    assert.match(
      rustStructFieldBlock(rust, "RootCalendar", "overnightEnd"),
      /wire rule: utc-iso/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "Root", "calendar"),
      /field projection: indexed-interface-field-extension -> Snapshot/u,
    );
    assert.deepEqual(rustStructFieldEntries(rust, "Replaced"), [
      ["retained", "String"],
      ["own", "String"],
      ["inherited", "bool"],
    ]);
    assert.deepEqual(manifest.roots[4], {
      source: "child.ts",
      exportName: "Child",
      rustName: "Child",
      kind: "readonly-interface",
      base: "Base",
      importedBase: true,
    });
    assert.deepEqual(manifest.roots[5], {
      source: "child.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-interface",
      fieldProjections: {
        calendar: {
          kind: "indexed-interface-field-extension",
          root: "Snapshot",
        },
      },
    });
    assert.deepEqual(manifest.roots[6], {
      source: "child.ts",
      exportName: "Replaced",
      rustName: "Replaced",
      kind: "readonly-interface",
      base: "Middle",
      omitBaseFields: ["inherited"],
      omitInheritedBaseFields: ["inherited"],
    });
    assert.deepEqual(manifest.typeImports, [{
      consumer: "child.ts",
      specifier: "./base.js",
      entry: "base.ts",
      exports: {
        Base: "base.ts",
        Middle: "base.ts",
        Snapshot: "base.ts",
      },
    }]);

    const importedBaseConfig = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [config.roots[0], config.roots[4]],
      typeImports: [{
        consumer: "child.ts",
        specifier: "./base.js",
        entry: "base.ts",
        exports: { Base: "base.ts" },
      }],
    };
    const invalidImportedBases = [
      {
        name: "a value import",
        source: 'import { Base } from "./base.js";\nexport interface Child extends Base { readonly status: string; }\n',
        config: importedBaseConfig,
      },
      {
        name: "a renamed inherited base",
        source: 'import type { Base as Alias } from "./base.js";\nexport interface Child extends Alias { readonly status: string; }\n',
        config: importedBaseConfig,
      },
      {
        name: "an indirect inherited base",
        source: 'import type { Base } from "./base.js";\ntype Alias = Base;\nexport interface Child extends Alias { readonly status: string; }\n',
        config: importedBaseConfig,
      },
      {
        name: "a missing import proof",
        source: 'import type { Base } from "./base.js";\nexport interface Child extends Base { readonly status: string; }\n',
        config: {
          version: 1,
          typescriptVersion: "6.0.3",
          roots: importedBaseConfig.roots,
        },
      },
      {
        name: "a proof for the wrong sibling source",
        error: /config\.typeImports\[0\]\.exports\.Base must name a configured entry root/u,
        source: 'import type { Base } from "./base.js";\nexport interface Child extends Base { readonly status: string; }\n',
        config: {
          ...importedBaseConfig,
          typeImports: [{
            consumer: "child.ts",
            specifier: "./other.js",
            entry: "other.ts",
            exports: { Base: "other.ts" },
          }],
        },
        extraSources: {
          "other.ts": "export interface Base { readonly receivedAt: string; }\n",
        },
      },
    ];
    // reject value, renamed, indirect, unproved, and wrong-source inheritance
    for (const invalid of invalidImportedBases) {
      assert.throws(
        // retain the same configured foreign declaration while varying its proof
        () => generateRustContracts(
          invalid.config,
          {
            "base.ts": sources["base.ts"],
            "child.ts": invalid.source,
            ...invalid.extraSources,
          },
        ),
        invalid.error ?? /(?:configured imported interface base|configured type import identity|type import export identity).*(?:unsupported|root)/u,
        invalid.name,
      );
    }

    const projectionConfig = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [config.roots[1], config.roots[5]],
      typeImports: [{
        consumer: "child.ts",
        specifier: "./base.js",
        entry: "base.ts",
        exports: { Snapshot: "base.ts" },
      }],
    };
    const invalidProjections = [
      [
        "a different indexed field",
        `import type { Snapshot } from "./base.js";
export interface Root {
  readonly calendar: Snapshot["other"] & { readonly overnightEnd: string };
}
`,
      ],
      [
        "an indirect indexed field",
        `import type { Snapshot } from "./base.js";
type Calendar = Snapshot["calendar"];
export interface Root {
  readonly calendar: Calendar & { readonly overnightEnd: string };
}
`,
      ],
      [
        "an inherited field duplicate",
        `import type { Snapshot } from "./base.js";
export interface Root {
  readonly calendar: Snapshot["calendar"] & { readonly date: string };
}
`,
      ],
      [
        "a mutable extension field",
        `import type { Snapshot } from "./base.js";
export interface Root {
  readonly calendar: Snapshot["calendar"] & { overnightEnd: string };
}
`,
      ],
      [
        "an optional extension field",
        `import type { Snapshot } from "./base.js";
export interface Root {
  readonly calendar: Snapshot["calendar"] & { readonly overnightEnd?: string };
}
`,
      ],
      [
        "multiple extension fields",
        `import type { Snapshot } from "./base.js";
export interface Root {
  readonly calendar: Snapshot["calendar"] & {
    readonly overnightEnd: string;
    readonly other: string;
  };
}
`,
      ],
      [
        "a value import for the indexed root",
        `import { Snapshot } from "./base.js";
export interface Root {
  readonly calendar: Snapshot["calendar"] & { readonly overnightEnd: string };
}
`,
      ],
    ];
    // reject every widened indexed field or extension member shape
    for (const [name, source] of invalidProjections) {
      assert.throws(
        // exercise one exact projection configuration under each invalid syntax
        () => generateRustContracts(
          projectionConfig,
          { "base.ts": sources["base.ts"], "child.ts": source },
        ),
        /(?:indexed interface field extension|configured type import identity|inherited property redeclaration).*unsupported/u,
        name,
      );
    }
    assert.throws(
      // require explicit proof before removing a field inherited through Middle
      () => generateRustContracts(
        {
          ...config,
          roots: config.roots.map(
            // remove only the inherited omission proof from Replaced
            (root) => root.exportName === "Replaced"
              ? Object.fromEntries(
                Object.entries(root).filter(
                  // retain every unrelated root metadata key
                  ([key]) => key !== "omitInheritedBaseFields",
                ),
              )
              : root,
          ),
        },
        sources,
      ),
      /configured omitted interface fields unsupported at root Replaced/u,
    );
  },
);

test(
  "projects the original generic provider batch and default record",
  // retain the source field order, generic default, and exact wire storage
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const declaration = "ProviderBatch<RecordType = NormalizedWeatherRecord>";

    assert.deepEqual(
      rustStructFieldEntries(rust, "ProviderBatch", declaration),
      [
        ["attempts", "u64"],
        ["checksum", "String"],
        ["providerCursor", "Nullable<Vec<(Vec<u16>, JsonValue)>>"],
        ["records", "Vec<RecordType>"],
        ["responseMetadata", "Vec<(Vec<u16>, JsonValue)>"],
      ],
    );
    assert.match(
      rustStructFieldBlock(rust, "ProviderBatch", "attempts", declaration),
      /wire rule: safe-unsigned-integer, range 0\.\.=9007199254740991/u,
    );
    // preserve ordered json objects independently across nullable and required fields
    for (const field of ["providerCursor", "responseMetadata"]) {
      assert.match(
        rustStructFieldBlock(rust, "ProviderBatch", field, declaration),
        /wire rule: ordered-json-object/u,
      );
    }

    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assertManifestRootGroup(manifest.roots, PROVIDER_BATCH_ROOTS);
    assertManifestRootGroup(manifest.roots, WIDGET_FORECAST_EXTENSION_ROOTS);
    assert.deepEqual(manifest.typeImports[28], WIDGET_FORECAST_V3_V2_IMPORT);
    assert.deepEqual(manifest.typeImports[29], PROVIDER_BATCH_DOMAIN_IMPORT);
  },
);

test(
  "projects the original database migration result",
  // preserve ordered names, missing receipt metadata, and validated version storage
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    assert.deepEqual(rustStructFieldEntries(rust, "MigrationResult"), [
      ["applied", "Vec<String>"],
      ["current", "Vec<String>"],
      ["executionVariant", "FieldPresence<String>"],
      ["historySha256", "FieldPresence<String>"],
      ["revised20Sha256", "FieldPresence<String>"],
      ["revised23Sha256", "FieldPresence<String>"],
      ["serverVersionNum", "u64"],
    ]);
    assert.match(
      rustStructFieldBlock(rust, "MigrationResult", "serverVersionNum"),
      /wire rule: safe-unsigned-integer, range 0\.\.=9007199254740991/u,
    );
    assertManifestRootGroup(manifest.roots, [MIGRATION_RESULT_ROOT]);
  },
);

test(
  "projects all named fields of the original database weather row",
  // keep raw dates, string spelling, nulls, and binary64 before api normalization
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    assert.deepEqual(rustStructFieldEntries(rust, "WeatherRecordRow"), [
      ["adapterVersion", "Nullable<String>"],
      ["apparentTemperatureC", "Nullable<f64>"],
      ["blackGlobeTemperatureC", "Nullable<f64>"],
      ["cloudCoverPercent", "Nullable<f64>"],
      ["contentHash", "String"],
      ["contractEpoch", "Nullable<String>"],
      ["deviceModel", "Nullable<String>"],
      ["deviceSerial", "Nullable<String>"],
      ["deviceVendor", "Nullable<String>"],
      ["firstReceivedAt", "JsDateOrString"],
      ["id", "String"],
      ["lastReceivedAt", "JsDateOrString"],
      ["pm25MicrogramsPerCubicMeter", "Nullable<f64>"],
      ["precipitationMm", "Nullable<f64>"],
      ["precipitationRateMmPerHour", "Nullable<f64>"],
      ["pressureHpa", "Nullable<f64>"],
      ["productRunAt", "Nullable<JsDateOrString>"],
      ["providerKey", "String"],
      ["providerMetadata", "Nullable<Vec<(Vec<u16>, JsonValue)>>"],
      ["qualityMetadata", "Nullable<Vec<(Vec<u16>, JsonValue)>>"],
      ["relativeHumidityPercent", "Nullable<f64>"],
      ["revisionCount", "u64"],
      ["siteSlug", "String"],
      ["sourceId", "String"],
      ["sourceConfigFingerprint", "Nullable<String>"],
      ["sourceKey", "String"],
      ["sourceKind", "SourceKind"],
      ["stationSlug", "String"],
      ["soilElectricalConductivityMicrosiemensPerCm", "Nullable<f64>"],
      ["soilMoisturePercent", "Nullable<f64>"],
      ["solarRadiationWm2", "Nullable<f64>"],
      ["temperatureC", "Nullable<f64>"],
      ["upstreamModel", "Nullable<String>"],
      ["upstreamTimezone", "String"],
      ["uvIndex", "Nullable<f64>"],
      ["validAt", "JsDateOrString"],
      ["waterLevelM", "Nullable<f64>"],
      ["windDirectionDegrees", "Nullable<f64>"],
      ["windGustMps", "Nullable<f64>"],
      ["windSpeedMps", "Nullable<f64>"],
      ["wetBulbGlobeTemperatureC", "Nullable<f64>"],
    ]);
    assert.match(
      rust,
      /pub enum JsDateOrString \{[\s\S]*?DateEpochMilliseconds\(i64\),\n    InvalidDate,[\s\S]*?String\(Vec<u16>\),\n\}/u,
    );
    assert.doesNotMatch(rust, /pub (?:enum|struct) QueryResultRow\b/u);
    assertManifestRootGroup(manifest.roots, [WEATHER_RECORD_ROW_ROOT]);
    assert.deepEqual(manifest.generator.pgRowMarkerInputSha256, PG_ROW_MARKER_INPUT_SHA256);
  },
);

test(
  "projects the original current weather repository row and particulate samples",
  // append the four current fields after the exact previously pinned weather row
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const weatherFields = rustStructFieldEntries(rust, "WeatherRecordRow");

    assert.equal(weatherFields.length, 41);
    assert.deepEqual(rustStructFieldEntries(rust, "CurrentWeatherRecordRow"), [
      ...weatherFields,
      [
        "pm25HourlyBySensor",
        "Nullable<Vec<(Vec<u16>, Vec<Nullable<f64>>)>>",
      ],
      ["pm25HourlyConcentrations", "Nullable<Vec<Nullable<f64>>>"],
      ["pm25HourlyThrough", "JsDateOrString"],
      ["pressureChange3hHpa", "Nullable<f64>"],
    ]);
    assert.match(
      rustStructFieldBlock(rust, "CurrentWeatherRecordRow", "pm25HourlyBySensor"),
      /storage rule: ordered-runtime-f64-nullable-array-record/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "CurrentWeatherRecordRow", "pm25HourlyConcentrations"),
      /storage rule: runtime-f64-nullable-array/u,
    );

    // keep both pre-normalization particulate projections free of finite wire claims
    for (const field of ["pm25HourlyBySensor", "pm25HourlyConcentrations"]) {
      assert.doesNotMatch(
        rustStructFieldBlock(rust, "CurrentWeatherRecordRow", field),
        /wire rule: finite-f64/u,
      );
    }
    assertManifestRootGroup(manifest.roots, [CURRENT_WEATHER_RECORD_ROW_ROOT]);
  },
);

test(
  "projects original scheduling discovery and forecast observation repository rows",
  // preserve exact repository names, row storage, literal markers, and source order
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "DueSource"), [
      ["active", "bool"],
      ["cadenceSeconds", "u64"],
      ["id", "String"],
      ["materialProviderConfig", "JsonValue"],
      ["providerKey", "String"],
      ["siteSlug", "String"],
      ["sourceConfigFingerprint", "String"],
      ["sourceKey", "String"],
      ["sourceKind", "SourceKind"],
      ["stationSlug", "String"],
      ["timezone", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "ActiveSiteRow"), [
      ["attributionLabel", "String"],
      ["attributionUrl", "String"],
      ["latitude", "f64"],
      ["longitude", "f64"],
      ["providerKey", "String"],
      ["providerName", "String"],
      ["siteName", "String"],
      ["siteSlug", "String"],
      ["sourceId", "String"],
      ["sourceKey", "String"],
      ["sourceKind", "SourceKind"],
      ["stationKind", "StationKind"],
      ["stationLatitude", "f64"],
      ["stationLongitude", "f64"],
      ["stationName", "String"],
      ["stationSlug", "String"],
      ["timezone", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "DailyPrecipitationRow"), [
      ["accumulationMm", "f64"],
      ["sourceId", "String"],
      ["stationSlug", "String"],
      ["validThrough", "JsDateOrString"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "TideRecordRow"), [
      ["attributionLabel", "String"],
      ["attributionUrl", "String"],
      ["predictionType", "Nullable<String>"],
      ["providerKey", "String"],
      ["sourceId", "String"],
      ["sourceKind", "TideRecordRowSourceKindEnum"],
      ["stationName", "String"],
      ["stationSlug", "String"],
      ["validAt", "JsDateOrString"],
      ["waterLevelM", "f64"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "LatestWorkerHeartbeat"),
      [["lastLoopAt", "JsDateOrString"]],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastObservationInputRow"),
      [
        ["apparentTemperatureC", "Nullable<f64>"],
        ["cloudCoverPercent", "Nullable<f64>"],
        ["id", "String"],
        ["lastReceivedAt", "JsDateOrString"],
        ["pm25MicrogramsPerCubicMeter", "Nullable<f64>"],
        ["precipitationRateMmPerHour", "Nullable<f64>"],
        ["pressureHpa", "Nullable<f64>"],
        ["providerKey", "String"],
        ["relativeHumidityPercent", "Nullable<f64>"],
        ["solarRadiationWm2", "Nullable<f64>"],
        ["sourceId", "String"],
        ["sourceKey", "String"],
        ["sourceKind", "ForecastObservationInputRowSourceKindEnum"],
        ["stationSlug", "String"],
        ["temperatureC", "Nullable<f64>"],
        ["uvIndex", "Nullable<f64>"],
        ["validAt", "JsDateOrString"],
        ["windDirectionDegrees", "Nullable<f64>"],
        ["windGustMps", "Nullable<f64>"],
        ["windSpeedMps", "Nullable<f64>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastObservationInput"),
      [
        ["projectionFrom", "JsDateOrString"],
        ["queryFrom", "JsDateOrString"],
        ["records", "Vec<ForecastObservationInputRow>"],
        ["siteSlug", "ForecastObservationInputSiteSlugLiteral"],
        ["timezone", "ForecastObservationInputTimezoneLiteral"],
        ["through", "JsDateOrString"],
      ],
    );

    assert.deepEqual(
      rustStringEnumWireEntries(rust, "TideRecordRowSourceKindEnum"),
      [
        ["tide_observation", "TideObservation"],
        ["tide_prediction", "TidePrediction"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ForecastObservationInputRowSourceKindEnum",
      ),
      [
        ["model_current", "ModelCurrent"],
        ["physical_sensor", "PhysicalSensor"],
      ],
    );

    const literalCases = [
      ["ForecastObservationInputSiteSlugLiteral", "ballydidean"],
      ["ForecastObservationInputTimezoneLiteral", "America/Los_Angeles"],
    ];
    // retain both exact forecast-query marker values
    for (const [name, wireValue] of literalCases) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: &'static str = ${JSON.stringify(wireValue)};\n}`,
        ),
        name,
      );
    }

    assert.match(
      rustStructFieldBlock(rust, "DueSource", "cadenceSeconds"),
      /wire rule: safe-unsigned-integer/u,
    );
    // keep all site and station coordinates finite on the JSON wire
    for (const field of [
      "latitude",
      "longitude",
      "stationLatitude",
      "stationLongitude",
    ]) {
      assert.match(
        rustStructFieldBlock(rust, "ActiveSiteRow", field),
        /wire rule: finite-f64/u,
      );
    }
    assert.match(
      rustStructFieldBlock(rust, "DailyPrecipitationRow", "accumulationMm"),
      /storage rule: runtime-f64/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "TideRecordRow", "waterLevelM"),
      /storage rule: runtime-f64/u,
    );

    const forecastRuntimeFields = [
      "apparentTemperatureC",
      "cloudCoverPercent",
      "pm25MicrogramsPerCubicMeter",
      "precipitationRateMmPerHour",
      "pressureHpa",
      "relativeHumidityPercent",
      "solarRadiationWm2",
      "temperatureC",
      "uvIndex",
      "windDirectionDegrees",
      "windGustMps",
      "windSpeedMps",
    ];
    // preserve all nullable observation numbers as unrestricted runtime storage
    for (const field of forecastRuntimeFields) {
      const fieldBlock = rustStructFieldBlock(
        rust,
        "ForecastObservationInputRow",
        field,
      );
      assert.match(fieldBlock, /storage rule: runtime-f64/u);
      assert.doesNotMatch(fieldBlock, /wire rule: finite-f64/u);
    }

    const rawDateFields = [
      ["DailyPrecipitationRow", "validThrough"],
      ["TideRecordRow", "validAt"],
      ["LatestWorkerHeartbeat", "lastLoopAt"],
      ["ForecastObservationInputRow", "lastReceivedAt"],
      ["ForecastObservationInputRow", "validAt"],
      ["ForecastObservationInput", "projectionFrom"],
      ["ForecastObservationInput", "queryFrom"],
      ["ForecastObservationInput", "through"],
    ];
    // retain every raw database clock as a JavaScript Date-or-string storage value
    for (const [name, field] of rawDateFields) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /storage rule: js-date-or-string/u,
      );
    }

    assertManifestRootGroup(manifest.roots, SCHEDULING_DISCOVERY_FORECAST_ROW_ROOTS);
  },
);

test(
  "projects the original chart trend row with exact Date storage",
  // preserve every ordered chart field before api normalization
  () => {
    const { rust, manifest } = repositoryProjection();
    assert.deepEqual(rustStructFieldEntries(rust, "TrendPointRow"), [
      ["apparentTemperatureC", "Nullable<f64>"],
      ["pm25MicrogramsPerCubicMeter", "Nullable<f64>"],
      ["precipitationMm", "Nullable<f64>"],
      ["pressureHpa", "Nullable<f64>"],
      ["relativeHumidityPercent", "Nullable<f64>"],
      ["temperatureC", "Nullable<f64>"],
      ["temperatureMaximumC", "Nullable<f64>"],
      ["temperatureMinimumC", "Nullable<f64>"],
      ["validAt", "JsDate"],
      ["windDirectionDegrees", "Nullable<f64>"],
      ["windGustMps", "Nullable<f64>"],
      ["windSpeedMps", "Nullable<f64>"],
    ]);
    assert.match(rust, /pub enum JsDate \{\n[^}]*EpochMilliseconds\(i64\),\n    InvalidDate,\n\}/u);
    assert.match(
      rustStructFieldBlock(rust, "TrendPointRow", "validAt"),
      /storage rule: js-date, valid epoch milliseconds or invalid Date/u,
    );
    assertManifestRootGroup(JSON.parse(manifest).roots, [TREND_POINT_ROW_ROOT]);
  },
);

test(
  "projects original forecast observation hourly station metrics",
  // retain every metric and repository field in their original catalog order
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastObservationHourlyMetrics"),
      [
        ["relativeHumidityPercent", "Nullable<f64>"],
        ["temperatureC", "Nullable<f64>"],
        ["windDirectionDegrees", "Nullable<f64>"],
        ["windGustMps", "Nullable<f64>"],
        ["windSpeedMps", "Nullable<f64>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastObservationHourlyStationRow"),
      [
        ["metrics", "ForecastObservationHourlyMetrics"],
        ["physicalStationKey", "ForecastObservationStationKey"],
        ["providerFamily", "ForecastObservationProviderFamily"],
        ["sourceKeys", "Vec<String>"],
        ["validAt", "String"],
      ],
    );
    assert.match(
      rust,
      /#\[doc = "storage rule: readonly-nullable-metric-record, all catalog keys required; binary64 values or explicit null"\]\s*#\[allow\(non_snake_case\)\]\s*#\[derive\(Debug, Clone, PartialEq\)\]\s*pub struct ForecastObservationHourlyMetrics/u,
    );
    assert.doesNotMatch(
      rust.match(
        /^pub struct ForecastObservationHourlyMetrics \{\n[\s\S]+?^\}/mu,
      )?.[0] ?? "",
      /FieldPresence/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "ForecastObservationHourlyStationRow",
        "validAt",
      ),
      /wire rule: utc-iso/u,
    );
    assertManifestRootGroup(manifest.roots, FORECAST_OBSERVATION_HOURLY_ROOTS);
  },
);

test(
  "projects original persisted ECMWF temperature revision archive data",
  // retain the original archive fields and reuse the existing run field types
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "PersistedEcmwfTemperatureRevision"),
      [
        [
          "adapterVersion",
          "EcmwfTemperatureCanaryRunAdapterVersionLiteral",
        ],
        ["bestMatchRows", "Vec<WeatherRecordRow>"],
        ["contentHash", "String"],
        ["hours", "Vec<EcmwfTemperatureCanaryHour>"],
        ["logicalReceivedAt", "String"],
        ["modelCycle", "EcmwfTemperatureCanaryRunModelCycleEnum"],
        ["providerResponseSha256", "String"],
        ["recentErrorState", "EcmwfTemperatureCanaryRecentErrorState"],
        ["recentErrorStateSha256", "String"],
        ["runInitializedAt", "String"],
        ["siteId", "String"],
        ["siteLatitude", "f64"],
        ["siteLongitude", "f64"],
        ["siteSlug", "String"],
        ["upstreamModel", "EcmwfTemperatureCanaryRunUpstreamModelLiteral"],
      ],
    );

    // bind all three indexed source fields to their established nominal types
    for (const field of ["adapterVersion", "modelCycle", "upstreamModel"]) {
      assert.match(
        rustStructFieldBlock(
          rust,
          "PersistedEcmwfTemperatureRevision",
          field,
        ),
        /field projection: interface-field -> EcmwfTemperatureCanaryRun/u,
      );
    }
    assert.equal(
      rust.match(/pub struct EcmwfTemperatureCanaryRunAdapterVersionLiteral;/gu)
        ?.length,
      1,
    );
    assert.equal(
      rust.match(/pub enum EcmwfTemperatureCanaryRunModelCycleEnum \{/gu)
        ?.length,
      1,
    );
    assert.equal(
      rust.match(/pub struct EcmwfTemperatureCanaryRunUpstreamModelLiteral;/gu)
        ?.length,
      1,
    );
    assert.doesNotMatch(
      rust,
      /PersistedEcmwfTemperatureRevision(?:AdapterVersion|ModelCycle|UpstreamModel)(?:Literal|Enum)/u,
    );

    // preserve canonical archive instants at both revision boundaries
    for (const field of ["logicalReceivedAt", "runInitializedAt"]) {
      assert.match(
        rustStructFieldBlock(
          rust,
          "PersistedEcmwfTemperatureRevision",
          field,
        ),
        /wire rule: utc-iso/u,
      );
    }
    // retain both persisted site coordinates as finite wire numbers
    for (const field of ["siteLatitude", "siteLongitude"]) {
      assert.match(
        rustStructFieldBlock(
          rust,
          "PersistedEcmwfTemperatureRevision",
          field,
        ),
        /wire rule: finite-f64/u,
      );
    }

    assertManifestRootGroup(manifest.roots, [PERSISTED_ECMWF_TEMPERATURE_REVISION_ROOT]);
  },
);

test(
  "projects original site and provider bootstrap catalog data",
  // preserve every original catalog field, nested shape, mapping, and domain edge
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const providerFields = [
      ["active", "bool"],
      ["attributionLabel", "String"],
      ["attributionUrl", "String"],
      ["displayName", "String"],
      ["key", "String"],
    ];
    const structCases = [
      ["SiteConfigurationSource", [
        ["active", "bool"],
        ["adapterConfig", "JsonValue"],
        ["cadenceSeconds", "Nullable<u64>"],
        ["capabilities", "Vec<SourceCapability>"],
        ["fingerprint", "String"],
        ["key", "String"],
        ["sourceKind", "SourceKind"],
      ]],
      ["SiteConfigurationProvider", providerFields],
      ["SiteConfigurationSite", [
        ["active", "bool"],
        ["displayName", "String"],
        ["key", "String"],
        ["latitude", "f64"],
        ["longitude", "f64"],
        ["timezone", "String"],
      ]],
      ["SiteConfigurationStation", [
        ["active", "bool"],
        ["displayName", "String"],
        ["key", "String"],
        ["kind", "StationKind"],
        ["latitude", "f64"],
        ["longitude", "f64"],
        ["model", "Nullable<String>"],
        ["serial", "Nullable<String>"],
        ["vendor", "Nullable<String>"],
      ]],
      ["SiteConfiguration", [
        ["provider", "SiteConfigurationProvider"],
        ["site", "SiteConfigurationSite"],
        ["sources", "Vec<SiteConfigurationSource>"],
        ["station", "SiteConfigurationStation"],
        ["version", "u64"],
      ]],
      ["EcowittStationConfiguration", [
        ["active", "bool"],
        ["adapterConfig", "JsonValue"],
        ["cadenceSeconds", "u64"],
        ["displayName", "String"],
        ["expectedMac", "String"],
        ["fingerprint", "String"],
        ["gatewayHost", "String"],
        ["key", "String"],
        ["latitude", "f64"],
        ["longitude", "f64"],
        ["model", "String"],
        ["sourceKey", "String"],
        ["timezone", "String"],
      ]],
      ["EcowittConfigurationProvider", providerFields],
      ["EcowittConfiguration", [
        ["provider", "EcowittConfigurationProvider"],
        ["siteKey", "String"],
        ["stations", "Vec<EcowittStationConfiguration>"],
        ["version", "u64"],
      ]],
      ["TempestStationConfiguration", [
        ["active", "bool"],
        ["adapterConfig", "JsonValue"],
        ["cadenceSeconds", "u64"],
        ["deviceId", "u64"],
        ["displayName", "String"],
        ["fingerprint", "String"],
        ["historyStartDate", "String"],
        ["key", "String"],
        ["latitude", "f64"],
        ["locationId", "u64"],
        ["longitude", "f64"],
        ["serial", "String"],
        ["sourceKey", "String"],
        ["timezone", "String"],
      ]],
      ["TempestConfigurationProvider", providerFields],
      ["TempestConfiguration", [
        ["provider", "TempestConfigurationProvider"],
        ["siteKey", "String"],
        ["stations", "Vec<TempestStationConfiguration>"],
        ["version", "u64"],
      ]],
      ["PublicStationProviderConfiguration", providerFields],
      ["PublicStationSourceConfiguration", [
        ["active", "bool"],
        ["adapter", "PublicStationAdapter"],
        ["adapterConfig", "Vec<(Vec<u16>, JsonValue)>"],
        ["cadenceSeconds", "Nullable<u64>"],
        ["capabilities", "Vec<SourceCapability>"],
        ["fingerprint", "String"],
        ["historyEndDate", "Nullable<String>"],
        ["historyStartDate", "Nullable<String>"],
        ["key", "String"],
        ["maximumChunkDays", "u64"],
        ["providerKey", "String"],
      ]],
      ["PublicStationConfigurationStation", [
        ["active", "bool"],
        ["displayName", "String"],
        ["key", "String"],
        ["latitude", "f64"],
        ["longitude", "f64"],
        ["model", "Nullable<String>"],
        ["serial", "Nullable<String>"],
        ["sources", "Vec<PublicStationSourceConfiguration>"],
        ["timezone", "String"],
        ["vendor", "Nullable<String>"],
      ]],
      ["PublicStationConfiguration", [
        ["siteKey", "String"],
        ["providers", "Vec<PublicStationProviderConfiguration>"],
        ["stations", "Vec<PublicStationConfigurationStation>"],
        ["version", "u64"],
      ]],
      ["TideProviderConfiguration", providerFields],
      ["TideSourceConfiguration", [
        ["active", "bool"],
        ["adapterConfig", "Vec<(Vec<u16>, JsonValue)>"],
        ["cadenceSeconds", "u64"],
        ["capabilities", "Vec<SourceCapability>"],
        ["fingerprint", "String"],
        ["historyStartDate", "String"],
        ["key", "String"],
        ["maximumChunkDays", "u64"],
        ["sourceKind", "TideSourceConfigurationSourceKindEnum"],
      ]],
      ["TideStationConfiguration", [
        ["active", "bool"],
        ["displayName", "String"],
        ["key", "String"],
        ["kind", "StationKind"],
        ["latitude", "f64"],
        ["longitude", "f64"],
        ["model", "String"],
        ["serial", "String"],
        ["source", "TideSourceConfiguration"],
        ["timezone", "String"],
        ["vendor", "String"],
      ]],
      ["TideConfiguration", [
        ["provider", "TideProviderConfiguration"],
        ["siteKey", "String"],
        ["stations", "Vec<TideStationConfiguration>"],
        ["version", "u64"],
      ]],
    ];

    // compare every named and inline bootstrap structure in source order
    for (const [name, fields] of structCases) {
      assert.deepEqual(rustStructFieldEntries(rust, name), fields);
    }
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "PublicStationAdapter"),
      [
        ["ambient-weather", "AmbientWeather"],
        ["weather-underground", "WeatherUnderground"],
        ["netatmo", "Netatmo"],
        ["purpleair", "Purpleair"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "TideSourceConfigurationSourceKindEnum"),
      [
        ["tide_observation", "TideObservation"],
        ["tide_prediction", "TidePrediction"],
      ],
    );

    const unsignedFields = [
      ["SiteConfigurationSource", ["cadenceSeconds"]],
      ["SiteConfiguration", ["version"]],
      ["EcowittStationConfiguration", ["cadenceSeconds"]],
      ["EcowittConfiguration", ["version"]],
      [
        "TempestStationConfiguration",
        ["cadenceSeconds", "deviceId", "locationId"],
      ],
      ["TempestConfiguration", ["version"]],
      [
        "PublicStationSourceConfiguration",
        ["cadenceSeconds", "maximumChunkDays"],
      ],
      ["PublicStationConfiguration", ["version"]],
      [
        "TideSourceConfiguration",
        ["cadenceSeconds", "maximumChunkDays"],
      ],
      ["TideConfiguration", ["version"]],
    ];
    // retain every catalog version, cadence, device, location, and chunk bound
    for (const [name, fields] of unsignedFields) {
      // inspect every safe unsigned field on this bootstrap structure
      for (const field of fields) {
        assert.match(
          rustStructFieldBlock(rust, name, field),
          /wire rule: safe-unsigned-integer/u,
        );
      }
    }

    const coordinateStructs = [
      "SiteConfigurationSite",
      "SiteConfigurationStation",
      "EcowittStationConfiguration",
      "TempestStationConfiguration",
      "PublicStationConfigurationStation",
      "TideStationConfiguration",
    ];
    // retain finite wire validation for all catalog geometry
    for (const name of coordinateStructs) {
      // inspect both ordered coordinates on this structure
      for (const field of ["latitude", "longitude"]) {
        assert.match(
          rustStructFieldBlock(rust, name, field),
          /wire rule: finite-f64/u,
        );
      }
    }

    // preserve ordered dynamic adapter objects only where explicitly configured
    for (const name of [
      "PublicStationSourceConfiguration",
      "TideSourceConfiguration",
    ]) {
      assert.match(
        rustStructFieldBlock(rust, name, "adapterConfig"),
        /wire rule: ordered-json-object/u,
      );
    }
    // retain the other adapter payloads as the nominal recursive JSON value
    for (const name of [
      "SiteConfigurationSource",
      "EcowittStationConfiguration",
      "TempestStationConfiguration",
    ]) {
      assert.doesNotMatch(
        rustStructFieldBlock(rust, name, "adapterConfig"),
        /wire rule: ordered-json-object/u,
      );
    }

    const historyDateFields = [
      ["TempestStationConfiguration", "historyStartDate"],
      ["PublicStationSourceConfiguration", "historyEndDate"],
      ["PublicStationSourceConfiguration", "historyStartDate"],
      ["TideSourceConfiguration", "historyStartDate"],
    ];
    // keep calendar dates distinct from canonical UTC instants
    for (const [name, field] of historyDateFields) {
      assert.doesNotMatch(
        rustStructFieldBlock(rust, name, field),
        /wire rule: utc-iso/u,
      );
    }

    assertManifestRootGroup(manifest.roots, BOOTSTRAP_CATALOG_ROOTS);
    assert.deepEqual(
      manifest.typeImports.slice(30, 35),
      BOOTSTRAP_CATALOG_DOMAIN_IMPORTS,
    );
  },
);

test(
  "projects original serving forecast adjustment and calendar data",
  // preserve serving inputs, calendar labels, fail-raw values, and nominal edges
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStringEnumWireEntries(rust, "LocalMeteorologicalSeason"),
      [
        ["autumn", "Autumn"],
        ["spring", "Spring"],
        ["summer", "Summer"],
        ["winter", "Winter"],
      ],
    );
    assert.deepEqual(rustStringEnumWireEntries(rust, "LocalDaypart"), [
      ["afternoon", "Afternoon"],
      ["evening", "Evening"],
      ["morning", "Morning"],
      ["night", "Night"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "RuntimeCalendarFingerprint"),
      [
        ["icuVersion", "String"],
        ["tzdataVersion", "String"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "LocalCalendarFeatures"), [
      ["daypart", "LocalDaypart"],
      ["hour", "u64"],
      ["localDate", "String"],
      ["month", "u64"],
      ["season", "LocalMeteorologicalSeason"],
      ["timezone", "LocalCalendarFeaturesTimezoneLiteral"],
      ["validAt", "String"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "CoreAdjustmentApplicationInputEnvelope",
      ),
      [
        ["maximum", "f64"],
        ["minimum", "f64"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "CoreAdjustmentApplicationInput"),
      [
        ["calendarFingerprintMatches", "bool"],
        ["coefficient", "Nullable<f64>"],
        ["enabled", "bool"],
        ["envelope", "Nullable<CoreAdjustmentApplicationInputEnvelope>"],
        ["identityMatches", "bool"],
        ["metric", "ForecastAdjustmentMetric"],
        ["rawForecastValue", "f64"],
        ["rawWindSpeedMps", "Nullable<f64>"],
        ["rootAvailable", "bool"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "CoreAdjustmentApplicationResult"),
      [
        ["adjustedValue", "f64"],
        ["applied", "bool"],
        ["reason", "CoreAdjustmentApplicationResultReasonEnum"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "CoreAdjustmentApplicationResultReasonEnum",
      ),
      [
        ["adjusted", "Adjusted"],
        [
          "calendar_fingerprint_mismatch",
          "CalendarFingerprintMismatch",
        ],
        ["coefficient_missing", "CoefficientMissing"],
        ["disabled_metric_band", "DisabledMetricBand"],
        ["forecast_identity_mismatch", "ForecastIdentityMismatch"],
        ["raw_direction_calm", "RawDirectionCalm"],
        ["raw_value_invalid", "RawValueInvalid"],
        ["raw_value_ood", "RawValueOod"],
        ["root_missing", "RootMissing"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApplyForecastAdjustmentInputV1"),
      [
        ["evaluatedAt", "FieldPresence<String>"],
        ["metrics", "CanonicalWeatherMetrics"],
        [
          "rawForecastProvenance",
          "ForecastAdjustmentRawForecastProvenance",
        ],
        [
          "runtimeFingerprint",
          "FieldPresence<RuntimeCalendarFingerprint>",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindMaintenanceDecision",
      ),
      [
        [
          "adjustedMetrics",
          "ForecastAdjustmentWindMaintenanceDecisionAdjustedMetricsRecord",
        ],
        ["appliedMetrics", "Vec<ForecastAdjustmentWindCanaryMetric>"],
        ["bundleSha256", "String"],
        ["candidateArtifactSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentWindMaintenanceDecisionContractVersionLiteral",
        ],
        ["leadBand", "ForecastLeadBandKey"],
        [
          "rawForecastProvenance",
          "ForecastAdjustmentRawForecastProvenance",
        ],
        [
          "reasonCode",
          "ForecastAdjustmentWindMaintenanceDecisionReasonCodeNull",
        ],
        ["state", "ForecastAdjustmentWindMaintenanceDecisionStateLiteral"],
      ],
    );

    assert.ok(
      rust.includes(
        'impl LocalCalendarFeaturesTimezoneLiteral {\n    pub const WIRE_VALUE: &\'static str = "America/Los_Angeles";\n}',
      ),
    );
    // retain safe calendar coordinates without treating local dates as UTC
    for (const field of ["hour", "month"]) {
      assert.match(
        rustStructFieldBlock(rust, "LocalCalendarFeatures", field),
        /wire rule: safe-unsigned-integer/u,
      );
    }
    assert.match(
      rustStructFieldBlock(rust, "LocalCalendarFeatures", "validAt"),
      /wire rule: utc-iso/u,
    );
    assert.doesNotMatch(
      rustStructFieldBlock(rust, "LocalCalendarFeatures", "localDate"),
      /wire rule: utc-iso/u,
    );

    const runtimeFields = [
      ["CoreAdjustmentApplicationInput", "coefficient"],
      ["CoreAdjustmentApplicationInput", "rawForecastValue"],
      ["CoreAdjustmentApplicationInput", "rawWindSpeedMps"],
      ["CoreAdjustmentApplicationInputEnvelope", "maximum"],
      ["CoreAdjustmentApplicationInputEnvelope", "minimum"],
      ["CoreAdjustmentApplicationResult", "adjustedValue"],
    ];
    // retain unrestricted binary64 values at the original fail-raw boundary
    for (const [name, field] of runtimeFields) {
      const fieldBlock = rustStructFieldBlock(rust, name, field);
      assert.match(fieldBlock, /storage rule: runtime-f64/u);
      assert.doesNotMatch(fieldBlock, /wire rule: finite-f64/u);
    }

    assert.doesNotMatch(
      rustStructFieldBlock(
        rust,
        "ApplyForecastAdjustmentInputV1",
        "evaluatedAt",
      ),
      /wire rule: utc-iso/u,
    );
    assert.match(
      rust,
      /pub struct ForecastAdjustmentWindMaintenanceDecisionAdjustedMetricsRecord\(pub Vec<\(ForecastAdjustmentMetric, f64\)>\);/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "ForecastAdjustmentWindMaintenanceDecision",
        "adjustedMetrics",
      ),
      /wire rule: finite-partial-metric-record/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "ForecastAdjustmentWindMaintenanceDecision",
        "appliedMetrics",
      ),
      /field projection: const-catalog-array -> ForecastAdjustmentWindCanaryMetric/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "ForecastAdjustmentWindMaintenanceDecision",
        "leadBand",
      ),
      /field projection: return-enum -> ForecastLeadBandKey/u,
    );
    assert.match(
      rust,
      /#\[doc = "exact null wire literal"\][\s\S]*pub struct ForecastAdjustmentWindMaintenanceDecisionReasonCodeNull;/u,
    );
    assert.ok(
      rust.includes(
        'impl ForecastAdjustmentWindMaintenanceDecisionContractVersionLiteral {\n    pub const WIRE_VALUE: &\'static str = "forecast-adjustment-maintenance-decision/v1";\n}',
      ),
    );
    assert.ok(
      rust.includes(
        'impl ForecastAdjustmentWindMaintenanceDecisionStateLiteral {\n    pub const WIRE_VALUE: &\'static str = "active";\n}',
      ),
    );

    assert.deepEqual(
      manifest.typeImports.find(
        // select the extended apply domain edge independently of tail imports
        ({ consumer, specifier }) =>
          consumer === "packages/forecast-adjustment/src/apply.ts" &&
          specifier === "@weather/domain",
      ),
      APPLY_FORECAST_ADJUSTMENT_DOMAIN_IMPORT,
    );
    assertManifestRootGroup(manifest.roots, SERVING_FORECAST_ADJUSTMENT_ROOTS);
    assert.deepEqual(
      manifest.typeImports.slice(35, 37),
      SERVING_FORECAST_ADJUSTMENT_IMPORTS,
    );
  },
);

test(
  "projects original forecast adjustment loader state contracts",
  // preserve exact active bundles, fail-raw reasons, authority, and registry bytes
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "MaintenanceShadowComparatorAuthorityKind",
      ),
      [
        ["legacy_active", "LegacyActive"],
        ["maintenance_qualified", "MaintenanceQualified"],
        ["policy_raw", "PolicyRaw"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "MaintenanceShadowServingAuthority"),
      [
        ["artifactBase64", "Nullable<String>"],
        ["artifactIdentitySha256", "Nullable<String>"],
        ["artifactMemberSha256", "Nullable<String>"],
        ["authorityKind", "MaintenanceShadowComparatorAuthorityKind"],
        ["receiptBase64", "String"],
        ["receiptMemberSha256", "String"],
      ],
    );

    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Active",
      ),
      [
        ["bundle", "ForecastAdjustmentTemperatureCanaryRuntimeBundle"],
        [
          "reasonCode",
          "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1ActiveReasonCodeNull",
        ],
        [
          "state",
          "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1ActiveStateLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Disabled",
      ),
      [
        [
          "bundle",
          "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledBundleNull",
        ],
        [
          "reasonCode",
          "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum",
        ],
        [
          "state",
          "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledStateLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum",
      ),
      [
        ["bundle_invalid", "BundleInvalid"],
        ["bundle_missing", "BundleMissing"],
        ["canary_expired", "CanaryExpired"],
        ["canary_killed", "CanaryKilled"],
        ["registry_inactive", "RegistryInactive"],
        ["registry_invalid", "RegistryInvalid"],
      ],
    );

    assert.deepEqual(
      rustStructFieldEntries(rust, "LoadedForecastAdjustmentRuntimeV1Active"),
      [
        ["bundle", "ForecastAdjustmentRuntimeBundleV2"],
        [
          "comparatorAuthority",
          "FieldPresence<MaintenanceShadowServingAuthority>",
        ],
        [
          "reasonCode",
          "LoadedForecastAdjustmentRuntimeV1ActiveReasonCodeNull",
        ],
        ["state", "LoadedForecastAdjustmentRuntimeV1ActiveStateLiteral"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "LoadedForecastAdjustmentRuntimeV1Disabled",
      ),
      [
        [
          "bundle",
          "LoadedForecastAdjustmentRuntimeV1DisabledBundleNull",
        ],
        [
          "reasonCode",
          "LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum",
        ],
        ["state", "LoadedForecastAdjustmentRuntimeV1DisabledStateLiteral"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum",
      ),
      [
        ["bundle_invalid", "BundleInvalid"],
        ["bundle_missing", "BundleMissing"],
        ["registry_inactive", "RegistryInactive"],
        ["registry_invalid", "RegistryInvalid"],
      ],
    );

    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "LoadedForecastAdjustmentWindCanaryRuntimeV1Active",
      ),
      [
        ["bundle", "ForecastAdjustmentWindCanaryRuntimeBundle"],
        [
          "reasonCode",
          "LoadedForecastAdjustmentWindCanaryRuntimeV1ActiveReasonCodeNull",
        ],
        [
          "state",
          "LoadedForecastAdjustmentWindCanaryRuntimeV1ActiveStateLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "LoadedForecastAdjustmentWindCanaryRuntimeV1Disabled",
      ),
      [
        [
          "bundle",
          "LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledBundleNull",
        ],
        [
          "reasonCode",
          "LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum",
        ],
        [
          "state",
          "LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledStateLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum",
      ),
      [
        ["bundle_invalid", "BundleInvalid"],
        ["bundle_missing", "BundleMissing"],
        ["canary_expired", "CanaryExpired"],
        ["canary_killed", "CanaryKilled"],
        ["registry_inactive", "RegistryInactive"],
        ["registry_invalid", "RegistryInvalid"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureCanaryRegistryV2",
      ),
      [
        [
          "activeBundle",
          "ForecastAdjustmentTemperatureCanaryRegistryV2ActiveBundleNull",
        ],
        [
          "contractVersion",
          "ForecastAdjustmentTemperatureCanaryRegistryV2ContractVersionLiteral",
        ],
        [
          "rawReason",
          "ForecastAdjustmentTemperatureCanaryRegistryV2RawReasonLiteral",
        ],
        [
          "siteKey",
          "ForecastAdjustmentTemperatureCanaryRegistryV2SiteKeyLiteral",
        ],
      ],
    );

    const scalarMemberships = [
      ["MaintenanceShadowComparatorAuthorityKind", "legacy_active"],
      [
        "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum",
        "canary_expired",
      ],
      [
        "LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum",
        "registry_inactive",
      ],
      [
        "LoadedForecastAdjustmentWindCanaryRuntimeV1DisabledReasonCodeEnum",
        "canary_killed",
      ],
    ];
    // retain source membership beside each selected scalar variant
    for (const [name, wireValue] of scalarMemberships) {
      const enumBlock = rust.match(
        new RegExp(`^pub enum ${name} \\{\\n([\\s\\S]+?)^\\}`, "mu"),
      );
      assert.ok(enumBlock, name);
      assert.ok(
        enumBlock[0].includes(`#[doc = "wire value: ${wireValue}"]`),
        wireValue,
      );
    }
    assert.match(
      rust,
      /pub enum LoadedForecastAdjustmentRuntimeV1 \{\s*#\[doc = "wire discriminant state: \\"active\\""\]\s*Active\(LoadedForecastAdjustmentRuntimeV1Active\),\s*#\[doc = "wire discriminant state: \\"disabled\\""\]\s*Disabled\(LoadedForecastAdjustmentRuntimeV1Disabled\),\s*\}/u,
    );
    assert.match(
      rust,
      /pub enum LoadedForecastAdjustmentWindCanaryRuntimeV1 \{\s*#\[doc = "wire discriminant state: \\"active\\""\]\s*Active\(LoadedForecastAdjustmentWindCanaryRuntimeV1Active\),\s*#\[doc = "wire discriminant state: \\"disabled\\""\]\s*Disabled\(LoadedForecastAdjustmentWindCanaryRuntimeV1Disabled\),\s*\}/u,
    );
    assert.ok(
      rust.includes(
        'impl ForecastAdjustmentTemperatureCanaryRegistryV2ContractVersionLiteral {\n    pub const WIRE_VALUE: &\'static str = "forecast-adjustment-temperature-canary-registry/v2";\n}',
      ),
    );
    assert.ok(
      rust.includes(
        'impl ForecastAdjustmentTemperatureCanaryRegistryV2RawReasonLiteral {\n    pub const WIRE_VALUE: &\'static str = "policy_raw";\n}',
      ),
    );
    assert.ok(
      rust.includes(
        'impl ForecastAdjustmentTemperatureCanaryRegistryV2SiteKeyLiteral {\n    pub const WIRE_VALUE: &\'static str = "ballydidean";\n}',
      ),
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "ForecastAdjustmentTemperatureCanaryRegistryV2",
        "activeBundle",
      ),
      /wire rule: exact-null, only JSON null/u,
    );
    assert.match(
      rust,
      /#\[doc = "exact null wire literal"\]\s*#\[derive\(Debug, Clone, Copy, PartialEq, Eq\)\]\s*pub struct LoadedForecastAdjustmentRuntimeV1DisabledBundleNull;/u,
    );

    // select the earlier temperature loader by its export rather than group order
    const temperatureLoader = "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1";
    const priorTemperatureLoader = TEMPERATURE_CANARY_RUNTIME_ROOTS.find(
      // match the exact previously pinned export
      ({ exportName }) => exportName === temperatureLoader,
    );
    assert.ok(priorTemperatureLoader);
    assert.deepEqual(
      manifest.roots.find(
        // retain that manifest row without changing its discriminator or source
        ({ exportName }) => exportName === temperatureLoader,
      ),
      priorTemperatureLoader,
    );
    assert.deepEqual(
      manifest.typeImports.find(
        // select the extended loader domain edge independently of tail imports
        ({ consumer, specifier }) =>
          consumer === "packages/forecast-adjustment/src/runtime-loader.ts" &&
          specifier === "@weather/domain",
      ),
      RUNTIME_LOADER_DOMAIN_IMPORT,
    );
    assertManifestRootGroup(manifest.roots, FORECAST_ADJUSTMENT_LOADER_ROOTS);
    assert.deepEqual(
      manifest.typeImports.slice(37, 39),
      FORECAST_ADJUSTMENT_LOADER_IMPORTS,
    );
  },
);

test(
  "admits only an exact disabled loader reason Extract",
  // keep the opt-in on one exact two-arm loader and configured reason enum
  () => {
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Reason", fields: {} },
        { source: "fixture.ts", exportName: "ActiveBundle", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {},
          discriminator: "state",
          disabledReasonExtract: "Reason",
        },
      ],
    };
    const source = `export type Reason = "bundle_invalid" | "bundle_missing" | "registry_invalid";
export interface ActiveBundle { readonly identity: string; }
export type Root =
  | { readonly bundle: ActiveBundle; readonly comparatorAuthority?: string; readonly reasonCode: null; readonly state: "active" }
  | { readonly bundle: null; readonly reasonCode: Extract<Reason, "bundle_invalid" | "registry_invalid">; readonly state: "disabled" };
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "RootActive"), [
      ["bundle", "ActiveBundle"],
      ["comparatorAuthority", "FieldPresence<String>"],
      ["reasonCode", "RootActiveReasonCodeNull"],
      ["state", "RootActiveStateLiteral"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootDisabled"), [
      ["bundle", "RootDisabledBundleNull"],
      ["reasonCode", "RootDisabledReasonCodeEnum"],
      ["state", "RootDisabledStateLiteral"],
    ]);
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "RootDisabledReasonCodeEnum"),
      [
        ["bundle_invalid", "BundleInvalid"],
        ["registry_invalid", "RegistryInvalid"],
      ],
    );
    assert.deepEqual(manifest.roots[2], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "tagged-readonly-object-union",
      disabledReasonExtract: "Reason",
      discriminator: {
        field: "state",
        variants: [
          {
            rustName: "Active",
            payloadRustName: "RootActive",
            wireValue: "active",
          },
          {
            rustName: "Disabled",
            payloadRustName: "RootDisabled",
            wireValue: "disabled",
          },
        ],
      },
    });

    const syntaxFailures = [
      source.replace("Extract<Reason", "Exclude<Reason"),
      source.replace("readonly reasonCode: null", 'readonly reasonCode: "active"'),
      source.replace(
        "{ readonly bundle: null; readonly reasonCode:",
        "{ readonly bundle: ActiveBundle; readonly reasonCode:",
      ),
      source.replace(
        '; readonly state: "disabled" };',
        '; readonly state: "inactive" };',
      ),
      source.replace(
        '; readonly state: "disabled" };',
        '; readonly state: "disabled"; readonly extra: string };',
      ),
      source.replace(
        '"bundle_invalid" | "registry_invalid"',
        '"bundle_invalid" | "unknown"',
      ),
      source.replace(
        '"bundle_invalid" | "registry_invalid"',
        '"bundle_invalid" | "bundle_invalid"',
      ),
      `type Extract<T, U> = T;\n${source}`,
    ];
    // reject every structural, utility-identity, and subset relaxation
    for (const invalidSource of syntaxFailures) {
      assert.throws(
        // keep all opted-in syntax errors on the dedicated diagnostic
        () => generateRustContracts(config, { "fixture.ts": invalidSource }),
        /disabled-reason-extract/u,
        invalidSource,
      );
    }

    assert.throws(
      // reject a dependency cycle even when the Extract syntax is exact
      () => generateRustContracts(config, {
        "fixture.ts": source.replace(
          "readonly identity: string",
          "readonly root: Root",
        ),
      }),
      /recursive configured type reference cycle: ActiveBundle -> Root -> ActiveBundle/u,
    );
    assert.throws(
      // retain the default tagged grammar's rejection without the opt-in
      () => generateRustContracts(
        {
          ...config,
          roots: config.roots.map(
            // remove only the dedicated metadata from the loader root
            (root) => root.exportName === "Root"
              ? {
                source: root.source,
                exportName: root.exportName,
                fields: root.fields,
                discriminator: root.discriminator,
              }
              : root,
          ),
        },
        { "fixture.ts": source },
      ),
      /configured type reference unsupported at Root\.reasonCode/u,
    );
    assert.throws(
      // bind Extract to the exact configured enum named by metadata
      () => generateRustContracts(
        {
          ...config,
          roots: [
            config.roots[0],
            config.roots[1],
            { ...config.roots[2], disabledReasonExtract: "ActiveBundle" },
          ],
        },
        { "fixture.ts": source },
      ),
      /disabled-reason-extract enum root/u,
    );

    const metadataFailures = [
      { ...config.roots[2], disabledReasonExtract: "Root" },
      { ...config.roots[2], discriminator: "kind" },
      { ...config.roots[2], fields: { reasonCode: "exact-null" } },
    ];
    // reject every invalid opt-in before source grammar traversal
    for (const invalidRoot of metadataFailures) {
      assert.throws(
        // require one exact metadata identity and otherwise empty root
        () => generateRustContracts(
          { ...config, roots: [config.roots[0], config.roots[1], invalidRoot] },
          { "fixture.ts": source },
        ),
        /disabledReasonExtract/u,
      );
    }
  },
);

test(
  "projects original forecast adjustment maintenance runtime packages",
  // retain exact family payloads, fixed temperature source, and nominal edges
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackage",
      ),
      [
        ["bundleSha256", "String"],
        ["candidateSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentTemperatureMaintenanceRuntimePackageContractVersionLiteral",
        ],
        ["dueMonth", "String"],
        [
          "family",
          "ForecastAdjustmentTemperatureMaintenanceRuntimePackageFamilyLiteral",
        ],
        ["model", "TemperatureMosPermanentRuntimeModel"],
        [
          "source",
          "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceObject",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceObject",
      ),
      [
        [
          "adapterVersion",
          "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceAdapterVersionLiteral",
        ],
        [
          "cohort",
          "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceCohortLiteral",
        ],
        [
          "dataset",
          "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceDatasetLiteral",
        ],
        [
          "maximumReceiptAgeHours",
          "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceMaximumReceiptAgeHoursLiteral",
        ],
        [
          "providerKey",
          "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceProviderKeyLiteral",
        ],
        [
          "scope",
          "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceScopeLiteral",
        ],
        [
          "sourceDelayHours",
          "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceSourceDelayHoursLiteral",
        ],
        [
          "upstreamModel",
          "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceUpstreamModelLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindMaintenanceRuntimePackage",
      ),
      [
        ["bundleSha256", "String"],
        ["candidate", "ForecastAdjustmentCandidateV2"],
        ["candidateSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentWindMaintenanceRuntimePackageContractVersionLiteral",
        ],
        ["dueMonth", "String"],
        [
          "family",
          "ForecastAdjustmentWindMaintenanceRuntimePackageFamilyLiteral",
        ],
        ["source", "ForecastAdjustmentForecastIdentity"],
      ],
    );

    const stringLiterals = [
      [
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackageContractVersionLiteral",
        "forecast-adjustment-maintenance-runtime-package/v1",
      ],
      [
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackageFamilyLiteral",
        "temperature",
      ],
      [
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceAdapterVersionLiteral",
        "open-meteo-ecmwf-single-run/v1",
      ],
      [
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceCohortLiteral",
        "ecmwf_single_run_hindcast",
      ],
      [
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceDatasetLiteral",
        "single_run",
      ],
      [
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceProviderKeyLiteral",
        "open-meteo",
      ],
      [
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceScopeLiteral",
        "assumed_delay6_next12",
      ],
      [
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceUpstreamModelLiteral",
        "ecmwf_ifs",
      ],
      [
        "ForecastAdjustmentWindMaintenanceRuntimePackageContractVersionLiteral",
        "forecast-adjustment-maintenance-runtime-package/v1",
      ],
      [
        "ForecastAdjustmentWindMaintenanceRuntimePackageFamilyLiteral",
        "wind",
      ],
    ];
    // retain every original string token as one exact scalar marker
    for (const [name, value] of stringLiterals) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: &'static str = "${value}";\n}`,
        ),
        name,
      );
    }
    const unsignedLiterals = [
      [
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceMaximumReceiptAgeHoursLiteral",
        12,
      ],
      [
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackageSourceSourceDelayHoursLiteral",
        6,
      ],
    ];
    // retain both original unsigned values without widening to runtime numbers
    for (const [name, value] of unsignedLiterals) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: u64 = ${value};\n}`,
        ),
        name,
      );
    }
    assert.match(
      rustStructFieldBlock(
        rust,
        "ForecastAdjustmentTemperatureMaintenanceRuntimePackage",
        "source",
      ),
      /wire rule: frozen-const-literal-object, exact local strings and unsigned safe integers in source order/u,
    );
    assert.match(
      rust,
      /pub enum ForecastAdjustmentMaintenanceRuntimePackage \{\s*Temperature\(ForecastAdjustmentTemperatureMaintenanceRuntimePackage\),\s*Wind\(ForecastAdjustmentWindMaintenanceRuntimePackage\),\s*\}/u,
    );
    assertManifestRootGroup(manifest.roots, FORECAST_ADJUSTMENT_MAINTENANCE_PACKAGE_ROOTS);
    assertManifestImportGroup(manifest.typeImports, FORECAST_ADJUSTMENT_MAINTENANCE_PACKAGE_IMPORTS);
  },
);

test(
  "admits only an exact frozen const literal object",
  // bind one required field to a pinned local frozen scalar object
  () => {
    const config = fixtureConfig("Root", {
      source: "frozen-const-literal-object",
    });
    const source = `const SOURCE = Object.freeze({
  adapterVersion: "adapter/v1",
  maximumReceiptAgeHours: 12,
  providerKey: "provider",
  sourceDelayHours: 6,
} as const);
export interface Root { readonly source: typeof SOURCE; }
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "RootSourceObject"), [
      ["adapterVersion", "RootSourceAdapterVersionLiteral"],
      [
        "maximumReceiptAgeHours",
        "RootSourceMaximumReceiptAgeHoursLiteral",
      ],
      ["providerKey", "RootSourceProviderKeyLiteral"],
      ["sourceDelayHours", "RootSourceSourceDelayHoursLiteral"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["source", "RootSourceObject"],
    ]);
    assert.ok(
      rust.includes(
        'impl RootSourceAdapterVersionLiteral {\n    pub const WIRE_VALUE: &\'static str = "adapter/v1";\n}',
      ),
    );
    assert.ok(
      rust.includes(
        'impl RootSourceMaximumReceiptAgeHoursLiteral {\n    pub const WIRE_VALUE: u64 = 12;\n}',
      ),
    );
    assert.match(
      rustStructFieldBlock(rust, "Root", "source"),
      /wire rule: frozen-const-literal-object/u,
    );
    assert.deepEqual(manifest.roots[0], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-interface",
    });

    const sourceCases = [
      {
        name: "optional root field",
        source: source.replace("readonly source:", "readonly source?:"),
        error:
          /frozen-const-literal-object requires a required readonly typeof LOCAL_CONST field at Root\.source/u,
      },
      {
        name: "mutable root field",
        source: source.replace("readonly source:", "source:"),
        error: /non-readonly property unsupported at Root/u,
      },
      {
        name: "nullable type query",
        source: source.replace("typeof SOURCE;", "typeof SOURCE | null;"),
        error:
          /frozen-const-literal-object requires a required readonly typeof LOCAL_CONST field at Root\.source/u,
      },
      {
        name: "missing const assertion",
        source: source.replace("} as const);", "});"),
        error: /frozen-const-literal-object source unsupported at root Root/u,
      },
      {
        name: "empty frozen object",
        source: source.replace(
          /Object\.freeze\(\{[\s\S]+?\} as const\)/u,
          "Object.freeze({} as const)",
        ),
        error: /frozen-const-literal-object source unsupported at root Root/u,
      },
      {
        name: "boolean data value",
        source: source.replace('providerKey: "provider"', "providerKey: true"),
        error: /frozen-const-literal-object source unsupported at root Root/u,
      },
      {
        name: "nested data value",
        source: source.replace(
          'providerKey: "provider"',
          'providerKey: { key: "provider" }',
        ),
        error: /frozen-const-literal-object source unsupported at root Root/u,
      },
      {
        name: "quoted property",
        source: source.replace("providerKey:", '"providerKey":'),
        error: /frozen-const-literal-object source unsupported at root Root/u,
      },
      {
        name: "duplicate property",
        source: source.replace(
          'providerKey: "provider"',
          'providerKey: "provider",\n  providerKey: "other"',
        ),
        error: /frozen-const-literal-object source unsupported at root Root/u,
      },
      {
        name: "fractional number token",
        source: source.replace("sourceDelayHours: 6", "sourceDelayHours: 1.5"),
        error: /unsigned decimal integer literal unsupported/u,
      },
      {
        name: "exponent number token",
        source: source.replace("sourceDelayHours: 6", "sourceDelayHours: 1e2"),
        error: /unsigned decimal integer literal unsupported/u,
      },
      {
        name: "unsafe number token",
        source: source.replace(
          "sourceDelayHours: 6",
          "sourceDelayHours: 9007199254740992",
        ),
        error: /unsafe integer literal/u,
      },
      {
        name: "shadowed Object utility",
        source: `const Object = { freeze: 1 };\n${source}`,
        error:
          /frozen-const-literal-object utility identity unsupported at Root\.source/u,
      },
    ];
    // reject every widened field, malformed initializer, and unsupported scalar
    for (const fixture of sourceCases) {
      assert.throws(
        // retain the mapping-specific diagnostic for each rejected source form
        () => generateRustContracts(config, { "fixture.ts": fixture.source }),
        fixture.error,
        fixture.name,
      );
    }
    assert.throws(
      // retain the ordinary grammar's rejection without mapping opt-in
      () => generateRustContracts(
        fixtureConfig("Root"),
        { "fixture.ts": source },
      ),
      /TypeScript type unsupported at Root\.source: TypeQuery/u,
    );
  },
);

test(
  "projects the exact wind maintenance registry metric band tuple",
  // retain the closed raw registry and every ordered wind band pair
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentWindCanaryRegistryV2"),
      [
        [
          "activeBundle",
          "ForecastAdjustmentWindCanaryRegistryV2ActiveBundleNull",
        ],
        [
          "contractVersion",
          "ForecastAdjustmentWindCanaryRegistryV2ContractVersionLiteral",
        ],
        [
          "enabledMetricBands",
          "ForecastAdjustmentWindCanaryRegistryV2EnabledMetricBandsTuple",
        ],
        [
          "rawReason",
          "ForecastAdjustmentWindCanaryRegistryV2RawReasonLiteral",
        ],
        [
          "siteKey",
          "ForecastAdjustmentWindCanaryRegistryV2SiteKeyLiteral",
        ],
      ],
    );
    const tupleName =
      "ForecastAdjustmentWindCanaryRegistryV2EnabledMetricBandsTuple";
    const tupleImpl = rust.match(
      new RegExp(`^impl ${tupleName} \\{\\n([\\s\\S]+?)^\\}`, "mu"),
    );
    assert.ok(tupleImpl, tupleName);
    assert.match(
      rust,
      /pub struct ForecastAdjustmentWindCanaryRegistryV2EnabledMetricBandsTuple;/u,
    );
    assert.match(
      tupleImpl[0],
      /pub const WIRE_KEYS: \[&'static str; 2\] = \["leadBand", "metric"\];/u,
    );
    assert.match(
      tupleImpl[0],
      /pub const WIRE_VALUES: &'static \[\(&'static str, &'static str\); 13\] = &\[/u,
    );
    assert.deepEqual(
      Array.from(
        tupleImpl[1].matchAll(
          /^        \("([^"]+)", "([^"]+)"\),$/gmu,
        ),
        // retain each exact lead band and metric in source order
        ([, leadBand, metric]) => [leadBand, metric],
      ),
      [
        ["001-024", "windGustMps"],
        ["025-048", "windGustMps"],
        ["073-096", "windGustMps"],
        ["097-120", "windGustMps"],
        ["121-144", "windGustMps"],
        ["145-168", "windGustMps"],
        ["001-024", "windSpeedMps"],
        ["025-048", "windSpeedMps"],
        ["049-072", "windSpeedMps"],
        ["073-096", "windSpeedMps"],
        ["097-120", "windSpeedMps"],
        ["121-144", "windSpeedMps"],
        ["145-168", "windSpeedMps"],
      ],
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "ForecastAdjustmentWindCanaryRegistryV2",
        "activeBundle",
      ),
      /wire rule: exact-null, only JSON null/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "ForecastAdjustmentWindCanaryRegistryV2",
        "enabledMetricBands",
      ),
      /wire rule: frozen-wind-metric-band-tuple, exact ordered leadBand and metric pairs/u,
    );
    assert.ok(
      rust.includes(
        'impl ForecastAdjustmentWindCanaryRegistryV2ContractVersionLiteral {\n    pub const WIRE_VALUE: &\'static str = "forecast-adjustment-wind-canary-registry/v2";\n}',
      ),
    );
    assert.ok(
      rust.includes(
        'impl ForecastAdjustmentWindCanaryRegistryV2RawReasonLiteral {\n    pub const WIRE_VALUE: &\'static str = "policy_raw";\n}',
      ),
    );
    assert.ok(
      rust.includes(
        'impl ForecastAdjustmentWindCanaryRegistryV2SiteKeyLiteral {\n    pub const WIRE_VALUE: &\'static str = "ballydidean";\n}',
      ),
    );
    assertManifestRootGroup(manifest.roots, [FORECAST_ADJUSTMENT_WIND_REGISTRY_ROOT]);
    assertManifestImportGroup(manifest.typeImports, [FORECAST_ADJUSTMENT_WIND_REGISTRY_IMPORT]);
  },
);

test(
  "admits only an exact frozen wind metric band tuple",
  // bind one required tuple field to a proven named identity wrapper
  () => {
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        fields: { enabledMetricBands: "frozen-wind-metric-band-tuple" },
      }],
      typeImports: [{
        consumer: "fixture.ts",
        specifier: "./candidate.js",
        entry: "candidate.ts",
        exports: {},
        tupleIdentityWrapper: "deepFreeze",
      }],
    };
    const source = `import { deepFreeze } from "./candidate.js";
const BANDS = deepFreeze([
  { leadBand: "001-024", metric: "windGustMps" },
] as const);
export interface Root {
  readonly enabledMetricBands: typeof BANDS;
}
`;
    const wrapper = `// return one typed value unchanged
export function deepFreeze<T>(value: T): T { return value; }
`;
    const sources = { "fixture.ts": source, "candidate.ts": wrapper };
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      sources,
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["enabledMetricBands", "RootEnabledMetricBandsTuple"],
    ]);
    assert.match(
      rust,
      /pub struct RootEnabledMetricBandsTuple;/u,
    );
    assert.match(
      rust,
      /impl RootEnabledMetricBandsTuple \{\s*pub const WIRE_KEYS: \[&'static str; 2\] = \["leadBand", "metric"\];\s*pub const WIRE_VALUES: &'static \[\(&'static str, &'static str\); 1\] = &\[\s*\("001-024", "windGustMps"\),\s*\];\s*\}/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "Root", "enabledMetricBands"),
      /wire rule: frozen-wind-metric-band-tuple/u,
    );
    assert.deepEqual(manifest.typeImports, config.typeImports);

    const sourceCases = [
      {
        name: "unexpected tuple payload field",
        source: source.replace(
          'metric: "windGustMps"',
          'metric: "windGustMps", extra: "unsupported"',
        ),
        wrapper,
        error: /frozen-wind-metric-band-tuple/u,
      },
      {
        name: "reversed tuple payload fields",
        source: source.replace(
          '{ leadBand: "001-024", metric: "windGustMps" }',
          '{ metric: "windGustMps", leadBand: "001-024" }',
        ),
        wrapper,
        error: /frozen-wind-metric-band-tuple/u,
      },
      {
        name: "widened non-tuple array",
        source: source.replace("] as const);", "]);"),
        wrapper,
        error: /frozen-wind-metric-band-tuple/u,
      },
      {
        name: "optional tuple field",
        source: source.replace(
          "readonly enabledMetricBands:",
          "readonly enabledMetricBands?:",
        ),
        wrapper,
        error: /frozen-wind-metric-band-tuple/u,
      },
      {
        name: "nullable tuple field",
        source: source.replace("typeof BANDS;", "typeof BANDS | null;"),
        wrapper,
        error: /frozen-wind-metric-band-tuple/u,
      },
      {
        name: "empty tuple",
        source: source.replace(
          '[\n  { leadBand: "001-024", metric: "windGustMps" },\n]',
          "[]",
        ),
        wrapper,
        error: /frozen-wind-metric-band-tuple/u,
      },
      {
        name: "bad wrapper signature",
        source,
        wrapper: wrapper.replace("(value: T)", "(value: T, extra: T)"),
        error: /frozen-wind-metric-band-tuple/u,
      },
      {
        name: "bad wrapper return type",
        source,
        wrapper: wrapper.replace(": T {", ": unknown {"),
        error: /frozen-wind-metric-band-tuple/u,
      },
    ];
    // reject every widened tuple, payload extension, and wrapper signature
    for (const fixture of sourceCases) {
      assert.throws(
        // keep mapping-owned validation on its dedicated diagnostic
        () => generateRustContracts(config, {
          "fixture.ts": fixture.source,
          "candidate.ts": fixture.wrapper,
        }),
        fixture.error,
        fixture.name,
      );
    }
    assert.throws(
      // require the sibling graph proof for the imported wrapper value
      () => generateRustContracts(
        { ...config, typeImports: undefined },
        { "fixture.ts": source },
      ),
      /configured value import identity unsupported/u,
    );
    assert.throws(
      // reject a configured wrapper proof when no selected field consumes it
      () => generateRustContracts(
        {
          ...config,
          roots: [{
            ...config.roots[0],
            fields: {},
          }],
        },
        {
          ...sources,
          "fixture.ts": source.replace(
            "typeof BANDS",
            "readonly string[]",
          ),
        },
      ),
      /unused configured tuple identity wrapper/u,
    );
    assert.throws(
      // retain ordinary TypeQuery rejection without the field mapping
      () => generateRustContracts(
        {
          ...config,
          roots: [{ ...config.roots[0], fields: {} }],
          typeImports: undefined,
        },
        { "fixture.ts": source },
      ),
      /TypeScript type unsupported at Root\.enabledMetricBands: TypeQuery/u,
    );
  },
);

test(
  "projects the complete wind and temperature maintenance loader outcomes",
  // retain the legacy wrapper and both maintenance payloads without wire tags
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const wind = "LoadedForecastAdjustmentWindCanaryRuntime";
    const temperature = "LoadedForecastAdjustmentTemperatureCanaryRuntime";

    assert.match(
      rust,
      /pub enum LoadedForecastAdjustmentWindCanaryRuntime \{\s*Legacy \{\s*value: LoadedForecastAdjustmentWindCanaryRuntimeV1,\s*comparatorAuthority: FieldPresence<MaintenanceShadowServingAuthority>,\s*\},\s*MaintenanceActive\(LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActive\),\s*MaintenanceDisabled\(LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceDisabled\),\s*\}/u,
    );
    assert.match(
      rust,
      /pub enum LoadedForecastAdjustmentTemperatureCanaryRuntime \{\s*Legacy \{\s*value: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1,\s*comparatorAuthority: FieldPresence<MaintenanceShadowServingAuthority>,\s*\},\s*MaintenanceActive\(LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActive\),\s*MaintenanceDisabled\(LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceDisabled\),\s*\}/u,
    );
    // keep storage-only Rust variants free of invented wire discriminants
    for (const rootName of [wind, temperature]) {
      const definition = rust.match(
        new RegExp(`^pub enum ${rootName} \\{\\n([\\s\\S]+?)^\\}`, "mu"),
      );
      assert.ok(definition, rootName);
      assert.doesNotMatch(definition[0], /wire discriminant|DISCRIMINATOR_FIELD/u);
      assert.deepEqual(
        rustStructFieldEntries(rust, `${rootName}MaintenanceDisabled`),
        [
          ["bundle", `${rootName}MaintenanceDisabledBundleNull`],
          ["comparatorAuthority", "MaintenanceShadowServingAuthority"],
          ["reasonCode", `${rootName}MaintenanceDisabledReasonCodeLiteral`],
          ["state", `${rootName}MaintenanceDisabledStateLiteral`],
        ],
      );
    }
    assert.deepEqual(
      rustStructFieldEntries(rust, `${wind}MaintenanceActive`),
      [
        ["bundle", `${wind}MaintenanceActiveBundle`],
        ["comparatorAuthority", "MaintenanceShadowServingAuthority"],
        ["reasonCode", `${wind}MaintenanceActiveReasonCodeNull`],
        ["state", `${wind}MaintenanceActiveStateLiteral`],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, `${wind}MaintenanceActiveBundle`),
      [
        ["bundleSha256", "String"],
        ["candidate", "ForecastAdjustmentCandidateV2"],
        [
          "maintenanceAuthority",
          `${wind}MaintenanceActiveBundleMaintenanceAuthority`,
        ],
        ["maintenanceBundleSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        `${wind}MaintenanceActiveBundleMaintenanceAuthority`,
      ),
      [
        ["actionSha256", "String"],
        ["fullMemberRootSha256", "String"],
        ["policyReportSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, `${temperature}MaintenanceActive`),
      [
        ["bundle", `${temperature}MaintenanceActiveBundle`],
        ["comparatorAuthority", "MaintenanceShadowServingAuthority"],
        ["reasonCode", `${temperature}MaintenanceActiveReasonCodeNull`],
        ["state", `${temperature}MaintenanceActiveStateLiteral`],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, `${temperature}MaintenanceActiveBundle`),
      [
        ["bundleSha256", "String"],
        [
          "maintenanceAuthority",
          `${temperature}MaintenanceActiveBundleMaintenanceAuthority`,
        ],
        [
          "maintenancePackage",
          `${temperature}MaintenanceActiveBundleMaintenancePackageLiteral`,
        ],
        ["model", "TemperatureMosPermanentRuntimeModel"],
        [
          "servedForecastIdentity",
          `${temperature}MaintenanceActiveBundleServedForecastIdentity`,
        ],
        [
          "trainingForecastIdentity",
          `${temperature}MaintenanceActiveBundleTrainingForecastIdentity`,
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        `${temperature}MaintenanceActiveBundleMaintenanceAuthority`,
      ),
      [
        ["actionSha256", "String"],
        ["fullMemberRootSha256", "String"],
        ["policyReportSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        `${temperature}MaintenanceActiveBundleServedForecastIdentity`,
      ),
      [
        [
          "adapterVersion",
          `${temperature}MaintenanceActiveBundleServedForecastIdentityAdapterVersionLiteral`,
        ],
        [
          "dataset",
          `${temperature}MaintenanceActiveBundleServedForecastIdentityDatasetLiteral`,
        ],
        [
          "maximumReceiptAgeHours",
          `${temperature}MaintenanceActiveBundleServedForecastIdentityMaximumReceiptAgeHoursLiteral`,
        ],
        [
          "providerKey",
          `${temperature}MaintenanceActiveBundleServedForecastIdentityProviderKeyLiteral`,
        ],
        [
          "sourceDelayHours",
          `${temperature}MaintenanceActiveBundleServedForecastIdentitySourceDelayHoursLiteral`,
        ],
        [
          "upstreamModel",
          `${temperature}MaintenanceActiveBundleServedForecastIdentityUpstreamModelLiteral`,
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        `${temperature}MaintenanceActiveBundleTrainingForecastIdentity`,
      ),
      [
        [
          "cohort",
          `${temperature}MaintenanceActiveBundleTrainingForecastIdentityCohortLiteral`,
        ],
        [
          "scope",
          `${temperature}MaintenanceActiveBundleTrainingForecastIdentityScopeLiteral`,
        ],
      ],
    );

    const stringLiterals = [
      [`${wind}MaintenanceActiveStateLiteral`, "active"],
      [`${wind}MaintenanceDisabledReasonCodeLiteral`, "policy_raw"],
      [`${wind}MaintenanceDisabledStateLiteral`, "disabled"],
      [`${temperature}MaintenanceActiveStateLiteral`, "active"],
      [`${temperature}MaintenanceDisabledReasonCodeLiteral`, "policy_raw"],
      [`${temperature}MaintenanceDisabledStateLiteral`, "disabled"],
      [
        `${temperature}MaintenanceActiveBundleServedForecastIdentityAdapterVersionLiteral`,
        "open-meteo-ecmwf-single-run/v1",
      ],
      [
        `${temperature}MaintenanceActiveBundleServedForecastIdentityDatasetLiteral`,
        "single_run",
      ],
      [
        `${temperature}MaintenanceActiveBundleServedForecastIdentityProviderKeyLiteral`,
        "open-meteo",
      ],
      [
        `${temperature}MaintenanceActiveBundleServedForecastIdentityUpstreamModelLiteral`,
        "ecmwf_ifs",
      ],
      [
        `${temperature}MaintenanceActiveBundleTrainingForecastIdentityCohortLiteral`,
        "ecmwf_single_run_hindcast",
      ],
      [
        `${temperature}MaintenanceActiveBundleTrainingForecastIdentityScopeLiteral`,
        "assumed_delay6_next12",
      ],
    ];
    // retain every source string as one exact marker value
    for (const [name, value] of stringLiterals) {
      assert.ok(
        rust.includes(
          `impl ${name} {\n    pub const WIRE_VALUE: &'static str = "${value}";\n}`,
        ),
        name,
      );
    }
    assert.ok(
      rust.includes(
        `impl ${temperature}MaintenanceActiveBundleMaintenancePackageLiteral {\n    pub const WIRE_VALUE: bool = true;\n}`,
      ),
    );
    assert.ok(
      rust.includes(
        `impl ${temperature}MaintenanceActiveBundleServedForecastIdentityMaximumReceiptAgeHoursLiteral {\n    pub const WIRE_VALUE: u64 = 12;\n}`,
      ),
    );
    assert.ok(
      rust.includes(
        `impl ${temperature}MaintenanceActiveBundleServedForecastIdentitySourceDelayHoursLiteral {\n    pub const WIRE_VALUE: u64 = 6;\n}`,
      ),
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        `${wind}MaintenanceActiveBundle`,
        "candidate",
      ),
      /field projection: interface-field -> ForecastAdjustmentWindMaintenanceRuntimePackage/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        `${temperature}MaintenanceActiveBundle`,
        "model",
      ),
      /field projection: interface-field -> ForecastAdjustmentTemperatureMaintenanceRuntimePackage/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        `${wind}MaintenanceActiveBundle`,
        "maintenanceAuthority",
      ),
      /wire rule: readonly-inline-object/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        `${temperature}MaintenanceActiveBundle`,
        "maintenanceAuthority",
      ),
      /wire rule: readonly-inline-object/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        `${temperature}MaintenanceActiveBundleServedForecastIdentity`,
        "maximumReceiptAgeHours",
      ),
      /wire rule: unsigned-safe-integer-literal/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        `${temperature}MaintenanceActiveBundleServedForecastIdentity`,
        "sourceDelayHours",
      ),
      /wire rule: unsigned-safe-integer-literal/u,
    );
    assertManifestRootGroup(manifest.roots, FORECAST_ADJUSTMENT_MAINTENANCE_LOADER_ROOTS);
    assertManifestImportGroup(manifest.typeImports, [FORECAST_ADJUSTMENT_MAINTENANCE_LOADER_IMPORT]);
  },
);

test(
  "admits only an exact maintenance loader extension",
  // bind one legacy loader and one imported package member without reshaping
  () => {
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Authority", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Legacy",
          fields: {},
          discriminator: "state",
        },
        { source: "package.ts", exportName: "Candidate", fields: {} },
        { source: "package.ts", exportName: "Package", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {
            "bundle.maintenanceAuthority": "readonly-inline-object",
          },
          maintenanceLoaderExtension: {
            legacy: "Legacy",
            package: "Package",
            bundleField: "candidate",
          },
        },
      ],
      typeImports: [{
        consumer: "fixture.ts",
        specifier: "./package.js",
        entry: "package.ts",
        exports: { Package: "package.ts" },
      }],
    };
    const source = `import type { Package } from "./package.js";
export interface Authority { readonly authority: string; }
export type Legacy =
  | {
      readonly bundle: { readonly legacyId: string };
      readonly reasonCode: null;
      readonly state: "active";
    }
  | {
      readonly bundle: null;
      readonly reasonCode: "legacy_raw";
      readonly state: "disabled";
    };
export type Root =
  | (Legacy & {
      readonly comparatorAuthority?: Authority;
    })
  | {
      readonly bundle: {
        readonly candidate: Package["candidate"];
        readonly maintenanceAuthority: Readonly<{
          readonly digest: string;
        }>;
      };
      readonly comparatorAuthority: Authority;
      readonly reasonCode: null;
      readonly state: "active";
    }
  | {
      readonly bundle: null;
      readonly comparatorAuthority: Authority;
      readonly reasonCode: "policy_raw";
      readonly state: "disabled";
    };
`;
    const packageSource = `export interface Candidate { readonly value: string; }
export interface Package { readonly candidate: Candidate; }
`;
    const sources = {
      "fixture.ts": source,
      "package.ts": packageSource,
    };
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      sources,
    );
    const manifest = JSON.parse(manifestText);

    assert.match(
      rust,
      /pub enum Root \{\s*Legacy \{\s*value: Legacy,\s*comparatorAuthority: FieldPresence<Authority>,\s*\},\s*MaintenanceActive\(RootMaintenanceActive\),\s*MaintenanceDisabled\(RootMaintenanceDisabled\),\s*\}/u,
    );
    assert.deepEqual(rustStructFieldEntries(rust, "RootMaintenanceActive"), [
      ["bundle", "RootMaintenanceActiveBundle"],
      ["comparatorAuthority", "Authority"],
      ["reasonCode", "RootMaintenanceActiveReasonCodeNull"],
      ["state", "RootMaintenanceActiveStateLiteral"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "RootMaintenanceActiveBundle"),
      [
        ["candidate", "Candidate"],
        [
          "maintenanceAuthority",
          "RootMaintenanceActiveBundleMaintenanceAuthority",
        ],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "RootMaintenanceDisabled"), [
      ["bundle", "RootMaintenanceDisabledBundleNull"],
      ["comparatorAuthority", "Authority"],
      ["reasonCode", "RootMaintenanceDisabledReasonCodeLiteral"],
      ["state", "RootMaintenanceDisabledStateLiteral"],
    ]);
    assert.deepEqual(manifest.roots.at(-1), {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "maintenance-loader-extension-union",
      maintenanceLoaderExtension: {
        legacy: "Legacy",
        package: "Package",
        bundleField: "candidate",
      },
    });

    const importedLegacySource = source.replace(
      `export type Legacy =
  | {
      readonly bundle: { readonly legacyId: string };
      readonly reasonCode: null;
      readonly state: "active";
    }
  | {
      readonly bundle: null;
      readonly reasonCode: "legacy_raw";
      readonly state: "disabled";
    };
`,
      `import type { Legacy } from "./legacy.js";
`,
    );
    const legacySource = `export type Legacy =
  | {
      readonly bundle: { readonly legacyId: string };
      readonly reasonCode: null;
      readonly state: "active";
    }
  | {
      readonly bundle: null;
      readonly reasonCode: "legacy_raw";
      readonly state: "disabled";
    };
`;
    const importedLegacyConfig = {
      ...config,
      roots: config.roots.map(
        // move only the nominated public legacy root to its proven owner
        (root) => root.exportName === "Legacy"
          ? { ...root, source: "legacy.ts" }
          : root,
      ),
      typeImports: [
        ...config.typeImports,
        {
          consumer: "fixture.ts",
          specifier: "./legacy.js",
          entry: "legacy.ts",
          exports: {},
          proofExports: { Legacy: "legacy.ts" },
        },
      ],
    };
    const importedLegacy = generateRustContracts(
      importedLegacyConfig,
      {
        ...sources,
        "fixture.ts": importedLegacySource,
        "legacy.ts": legacySource,
      },
    );
    const importedLegacyManifest = JSON.parse(importedLegacy.manifest);
    assert.match(
      importedLegacy.rust,
      /Legacy \{\s*value: Legacy,\s*comparatorAuthority: FieldPresence<Authority>,\s*\}/u,
    );
    assert.deepEqual(importedLegacyManifest.typeImports[1], {
      consumer: "fixture.ts",
      specifier: "./legacy.js",
      entry: "legacy.ts",
      exports: {},
      proofExports: { Legacy: "legacy.ts" },
    });

    const sourceCases = [
      {
        name: "wrong indexed package field",
        source: source.replace(
          'Package["candidate"]',
          'Package["different"]',
        ),
        error: /interface-field projection shape unsupported/u,
      },
      {
        name: "missing active package field",
        source: source.replace(
          'readonly candidate: Package["candidate"];',
          "readonly material: string;",
        ),
        error: /maintenance loader active package field missing/u,
      },
      {
        name: "required legacy comparator",
        source: source.replace(
          "readonly comparatorAuthority?: Authority;",
          "readonly comparatorAuthority: Authority;",
        ),
        error: /maintenance loader legacy intersection unsupported/u,
      },
      {
        name: "legacy comparator collision",
        source: source.replace(
          "readonly bundle: { readonly legacyId: string };",
          `readonly bundle: { readonly legacyId: string };
      readonly comparatorAuthority: Authority;`,
        ),
        error: /maintenance loader legacy comparator collision/u,
      },
      {
        name: "optional active comparator",
        source: source.replace(
          `readonly comparatorAuthority: Authority;
      readonly reasonCode: null;`,
          `readonly comparatorAuthority?: Authority;
      readonly reasonCode: null;`,
        ),
        error: /maintenance loader active payload unsupported/u,
      },
      {
        name: "optional disabled comparator",
        source: source.replace(
          `readonly comparatorAuthority: Authority;
      readonly reasonCode: "policy_raw";`,
          `readonly comparatorAuthority?: Authority;
      readonly reasonCode: "policy_raw";`,
        ),
        error: /maintenance loader disabled payload unsupported/u,
      },
      {
        name: "extra result arm",
        source: source.replace(
          `readonly reasonCode: "policy_raw";
      readonly state: "disabled";
    };`,
          `readonly reasonCode: "policy_raw";
      readonly state: "disabled";
    }
  | { readonly state: "extra" };`,
        ),
        error: /maintenance loader extension requires three direct arms/u,
      },
    ];
    // reject every indexed, legacy, payload, and union-shape widening
    for (const fixture of sourceCases) {
      assert.throws(
        // retain the production diagnostic for each rejected source shape
        () => generateRustContracts(config, {
          ...sources,
          "fixture.ts": fixture.source,
        }),
        fixture.error,
        fixture.name,
      );
    }
    assert.throws(
      // keep the extension grammar isolated from tagged-union metadata
      () => generateRustContracts(
        {
          ...config,
          roots: config.roots.map(
            // add the conflicting selector only to the extension root
            (root) => root.exportName === "Root"
              ? { ...root, discriminator: "state" }
              : root,
          ),
        },
        sources,
      ),
      /config\.roots\[4\] has unsupported key "discriminator"/u,
    );

    const ordinarySource = `export interface Authority { readonly authority: string; }
export type Legacy =
  | { readonly bundle: string; readonly reasonCode: null; readonly state: "active" }
  | { readonly bundle: null; readonly reasonCode: "legacy_raw"; readonly state: "disabled" };
export type Root =
  | (Legacy & { readonly comparatorAuthority?: Authority })
  | {
      readonly bundle: { readonly candidate: string };
      readonly comparatorAuthority: Authority;
      readonly reasonCode: null;
      readonly state: "active";
    }
  | {
      readonly bundle: null;
      readonly comparatorAuthority: Authority;
      readonly reasonCode: "policy_raw";
      readonly state: "disabled";
    };
`;
    assert.throws(
      // keep the ordinary union unsupported without importing unused proofs
      () => generateRustContracts(
        {
          version: 1,
          typescriptVersion: "6.0.3",
          roots: [
            { source: "ordinary.ts", exportName: "Authority", fields: {} },
            {
              source: "ordinary.ts",
              exportName: "Legacy",
              fields: {},
              discriminator: "state",
            },
            { source: "ordinary.ts", exportName: "Root", fields: {} },
          ],
        },
        { "ordinary.ts": ordinarySource },
      ),
      /root type unsupported at Root/u,
    );
  },
);

test(
  "projects the original loaded rain runtime registry",
  // preserve the imported active digest and both original registry payloads
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const root = "LoadedForecastAdjustmentRainRuntimeRegistryV1";

    assert.deepEqual(rustStructFieldEntries(rust, `${root}Active`), [
      ["artifactSha256", `${root}ActiveArtifactSha256Literal`],
      ["comparatorAuthority", "MaintenanceShadowServingAuthority"],
      ["reasonCode", `${root}ActiveReasonCodeNull`],
      ["state", `${root}ActiveStateLiteral`],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, `${root}Disabled`), [
      ["artifactSha256", `${root}DisabledArtifactSha256Null`],
      [
        "comparatorAuthority",
        "FieldPresence<MaintenanceShadowServingAuthority>",
      ],
      ["reasonCode", `${root}DisabledReasonCodeEnum`],
      ["state", `${root}DisabledStateLiteral`],
    ]);
    assert.deepEqual(
      rustStringEnumWireEntries(rust, `${root}DisabledReasonCodeEnum`),
      [
        ["policy_raw", "PolicyRaw"],
        ["registry_invalid", "RegistryInvalid"],
      ],
    );
    assert.match(
      rust,
      /pub enum LoadedForecastAdjustmentRainRuntimeRegistryV1 \{\s*#\[doc = "wire discriminant state: \\"active\\""\]\s*Active\(LoadedForecastAdjustmentRainRuntimeRegistryV1Active\),\s*#\[doc = "wire discriminant state: \\"disabled\\""\]\s*Disabled\(LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled\),\s*\}/u,
    );
    assert.ok(
      rust.includes(
        `impl ${root}ActiveArtifactSha256Literal {\n    pub const WIRE_VALUE: &'static str = "dfba22520eb66944046f999e96ee51378913ef926a2da27f0562490d9fb7ef64";\n}`,
      ),
    );
    assert.ok(
      rust.includes(
        `impl ${root}ActiveStateLiteral {\n    pub const WIRE_VALUE: &'static str = "active";\n}`,
      ),
    );
    assert.ok(
      rust.includes(
        `impl ${root}DisabledStateLiteral {\n    pub const WIRE_VALUE: &'static str = "disabled";\n}`,
      ),
    );
    assert.match(
      rust,
      new RegExp(
        `#\\[doc = "exact null wire literal"\\]\\s*#\\[derive\\(Debug, Clone, Copy, PartialEq, Eq\\)\\]\\s*pub struct ${root}DisabledArtifactSha256Null;`,
        "u",
      ),
    );
    assertManifestRootGroup(manifest.roots, [LOADED_RAIN_RUNTIME_REGISTRY_ROOT]);
    assertManifestImportGroup(manifest.typeImports, LOADED_RAIN_RUNTIME_REGISTRY_IMPORTS);
  },
);

test(
  "admits only an exact active imported const string",
  // bind one active field to a configured direct sibling const value
  () => {
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Authority", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {},
          discriminator: "state",
          activeImportedConstString: "artifactSha256",
        },
      ],
      typeImports: [{
        consumer: "fixture.ts",
        specifier: "./marker.js",
        entry: "marker.ts",
        exports: {},
        literalStringValue: "MARKER",
      }],
    };
    const source = `import { MARKER } from "./marker.js";
export interface Authority { readonly value: string; }
export type Root =
  | {
      readonly artifactSha256: typeof MARKER;
      readonly comparatorAuthority: Authority;
      readonly reasonCode: null;
      readonly state: "active";
    }
  | {
      readonly artifactSha256: null;
      readonly comparatorAuthority?: Authority;
      readonly reasonCode: "policy_raw" | "registry_invalid";
      readonly state: "disabled";
    };
`;
    const markerSource = `export const MARKER = "artifact-digest" as const;
`;
    const sources = {
      "fixture.ts": source,
      "marker.ts": markerSource,
    };
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      sources,
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "RootActive"), [
      ["artifactSha256", "RootActiveArtifactSha256Literal"],
      ["comparatorAuthority", "Authority"],
      ["reasonCode", "RootActiveReasonCodeNull"],
      ["state", "RootActiveStateLiteral"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootDisabled"), [
      ["artifactSha256", "RootDisabledArtifactSha256Null"],
      ["comparatorAuthority", "FieldPresence<Authority>"],
      ["reasonCode", "RootDisabledReasonCodeEnum"],
      ["state", "RootDisabledStateLiteral"],
    ]);
    assert.ok(
      rust.includes(
        `impl RootActiveArtifactSha256Literal {\n    pub const WIRE_VALUE: &'static str = "artifact-digest";\n}`,
      ),
    );
    assert.deepEqual(manifest.roots[1], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "tagged-readonly-object-union",
      activeImportedConstString: "artifactSha256",
      discriminator: {
        field: "state",
        variants: [
          {
            rustName: "Active",
            payloadRustName: "RootActive",
            wireValue: "active",
          },
          {
            rustName: "Disabled",
            payloadRustName: "RootDisabled",
            wireValue: "disabled",
          },
        ],
      },
    });
    assert.deepEqual(manifest.typeImports, config.typeImports);

    const invalidFixtures = [
      {
        name: "wrong active marker type",
        source: source.replace("typeof MARKER", "string"),
        markerSource,
        error:
          /active-imported-const-string requires typeof IMPORTED_CONST at Root\.artifactSha256/u,
      },
      {
        name: "nonconstant exported marker",
        source,
        markerSource: markerSource.replace("export const", "export let"),
        error: /active-imported-const-string source unsupported at root Root/u,
      },
      {
        name: "widened exported marker",
        source,
        markerSource:
          'export const MARKER: string = "artifact-digest";\n',
        error: /active-imported-const-string source unsupported at root Root/u,
      },
      {
        name: "ambient exported marker",
        source,
        markerSource: markerSource.replace("export const", "export declare const"),
        error:
          /active-imported-const-string requires a direct exported const at Root\.artifactSha256/u,
      },
      {
        name: "computed exported marker",
        source,
        markerSource:
          'export const MARKER = "artifact-" + "digest";\n',
        error:
          /active-imported-const-string requires an exact string literal at Root\.artifactSha256/u,
      },
      {
        name: "non-null disabled marker",
        source: source.replace(
          "readonly artifactSha256: null;",
          "readonly artifactSha256: typeof MARKER;",
        ),
        markerSource,
        error:
          /active-imported-const-string requires active const and disabled null at root Root/u,
      },
    ];
    // reject every widened source marker and disabled non-null alternative
    for (const fixture of invalidFixtures) {
      assert.throws(
        // retain the dedicated diagnostic for each invalid source pair
        () => generateRustContracts(
          config,
          {
            "fixture.ts": fixture.source,
            "marker.ts": fixture.markerSource,
          },
        ),
        fixture.error,
        fixture.name,
      );
    }
    assert.throws(
      // require the configured direct sibling value proof
      () => generateRustContracts(
        { ...config, typeImports: undefined },
        { "fixture.ts": source },
      ),
      /configured value import identity unsupported at Root\.artifactSha256/u,
    );
    assert.throws(
      // retain ordinary TypeQuery rejection without the root opt-in or edge
      () => generateRustContracts(
        {
          ...config,
          roots: [
            config.roots[0],
            {
              source: "fixture.ts",
              exportName: "Root",
              fields: {},
              discriminator: "state",
            },
          ],
          typeImports: undefined,
        },
        { "fixture.ts": source },
      ),
      /TypeScript type unsupported at Root\.artifactSha256: TypeQuery/u,
    );
  },
);

test(
  "projects the original API startup snapshots",
  // retain each startup loader nominal without inferring timestamp semantics
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.match(
      rust,
      /pub enum LoadedApiForecastAdjustmentRuntime \{\s*Standard\(LoadedForecastAdjustmentRuntimeV1\),\s*Wind\(LoadedForecastAdjustmentWindCanaryRuntime\),\s*\}/u,
    );
    const snapshots = [
      [
        "ForecastAdjustmentStartupSnapshot",
        "LoadedApiForecastAdjustmentRuntime",
      ],
      [
        "ForecastTemperatureAdjustmentStartupSnapshot",
        "LoadedForecastAdjustmentTemperatureCanaryRuntime",
      ],
      [
        "ForecastRainAdjustmentStartupSnapshot",
        "LoadedForecastAdjustmentRainRuntimeRegistryV1",
      ],
      [
        "ApiRainAdjustmentStartupSnapshot",
        "LoadedForecastAdjustmentRainRuntimeRegistryV1",
      ],
    ];
    // preserve the original two fields and plain loaded-at string in every snapshot
    for (const [name, runtime] of snapshots) {
      assert.deepEqual(rustStructFieldEntries(rust, name), [
        ["loadedAt", "String"],
        ["runtime", runtime],
      ]);
      assert.doesNotMatch(
        rustStructFieldBlock(rust, name, "loadedAt"),
        /wire rule: utc-iso/u,
      );
    }
    assertManifestRootGroup(manifest.roots, API_STARTUP_SNAPSHOT_ROOTS);
    assertManifestImportGroup(manifest.typeImports, [API_MAIN_FORECAST_ADJUSTMENT_IMPORT]);
    const rainImport = manifest.typeImports.find(
      // select the reclassified rain consumer edge independently of its index
      ({ consumer, specifier }) =>
        consumer === "apps/api/src/rain-adjustment.ts" &&
        specifier === "@weather/forecast-adjustment",
    );
    assert.ok(rainImport);
    assert.deepEqual(rainImport.exports, {
      LoadedForecastAdjustmentRainRuntimeRegistryV1:
        "packages/forecast-adjustment/src/rain-runtime-registry.ts",
    });
    assert.equal("proofExports" in rainImport, false);
  },
);

test(
  "admits only configured imported loader arms in a startup union",
  // admit one imported reviewed loader beside one local interface payload
  () => {
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "loader.ts", exportName: "Reason", fields: {} },
        { source: "loader.ts", exportName: "ActiveBundle", fields: {} },
        {
          source: "loader.ts",
          exportName: "Loader",
          fields: {},
          discriminator: "state",
          disabledReasonExtract: "Reason",
        },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {},
          localAlias: true,
          objectUnion: {
            Loader: "Imported",
            LocalPayload: "Local",
          },
        },
        { source: "fixture.ts", exportName: "LocalPayload", fields: {} },
        { source: "fixture.ts", exportName: "Startup", fields: {} },
      ],
      typeImports: [{
        consumer: "fixture.ts",
        specifier: "./loader.js",
        entry: "loader.ts",
        exports: { Loader: "loader.ts" },
      }],
    };
    const source = `import type { Loader } from "./loader.js";
export interface LocalPayload { readonly local: string; }
type Root = Loader | LocalPayload;
export interface Startup { readonly runtime: Root; }
`;
    const loaderSource = `export type Reason = "bundle_invalid" | "registry_invalid";
export interface ActiveBundle { readonly identity: string; }
export type Loader =
  | {
      readonly bundle: ActiveBundle;
      readonly reasonCode: null;
      readonly state: "active";
    }
  | {
      readonly bundle: null;
      readonly reasonCode: Extract<Reason, "registry_invalid">;
      readonly state: "disabled";
    };
`;
    const sources = {
      "fixture.ts": source,
      "loader.ts": loaderSource,
    };
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      sources,
    );
    const manifest = JSON.parse(manifestText);

    assert.match(
      rust,
      /pub enum Root \{\s*Imported\(Loader\),\s*Local\(LocalPayload\),\s*\}/u,
    );
    assert.deepEqual(rustStructFieldEntries(rust, "Startup"), [
      ["runtime", "Root"],
    ]);
    assert.deepEqual(manifest.roots[3], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "named-readonly-object-union",
      localAlias: true,
      objectUnion: {
        variants: [
          { rustName: "Imported", payloadRustName: "Loader" },
          { rustName: "Local", payloadRustName: "LocalPayload" },
        ],
      },
    });
    assert.deepEqual(manifest.typeImports, config.typeImports);

    assert.throws(
      // reject an ordinary imported tagged alias without reviewed loader metadata
      () => generateRustContracts(
        {
          ...config,
          roots: config.roots.map(
            // remove only the disabled-reason loader proof
            (root) => root.exportName === "Loader"
              ? {
                source: root.source,
                exportName: root.exportName,
                fields: root.fields,
                discriminator: root.discriminator,
              }
              : root,
          ),
        },
        {
          ...sources,
          "loader.ts": loaderSource.replace(
            'Extract<Reason, "registry_invalid">',
            '"registry_invalid"',
          ),
        },
      ),
      /local alias object union arm unsupported for Loader at root Root/u,
    );
  },
);

test(
  "projects the original rain inference boundaries",
  // retain inference numbers, vectors, timestamps, and fitted feature storage
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "RainWindForecastHour"), [
      ["leadHours", "f64"],
      ["precipitationMm", "Nullable<f64>"],
      ["temperatureC", "Nullable<f64>"],
      ["relativeHumidityPercent", "Nullable<f64>"],
      ["cloudCoverPercent", "Nullable<f64>"],
      ["pressureHpa", "Nullable<f64>"],
      ["windSpeedMps", "Nullable<f64>"],
      ["windDirectionDegrees", "Nullable<f64>"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RainWindRunProfile"), [
      ["runInitializedAt", "String"],
      ["completedAt", "String"],
      ["hours", "Vec<RainWindForecastHour>"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RainWindStationHour"), [
      ["hourAt", "String"],
      ["receivedAt", "String"],
      ["stationId", "f64"],
      ["precipitationMm", "Nullable<f64>"],
      ["temperatureC", "Nullable<f64>"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RainWindPredictionInput"), [
      ["currentRun", "RainWindRunProfile"],
      ["priorRuns", "Vec<RainWindRunProfile>"],
      ["stationHours", "Vec<RainWindStationHour>"],
      ["nowUtc", "String"],
    ]);
    const predictionFields = [
      ["validAt", "String"],
      ["modelLeadHours", "f64"],
      ["rawPrecipitationMm", "f64"],
      ["correctedPrecipitationMm", "f64"],
      ["applied", "bool"],
      ["reasonCode", "Nullable<RainWindPredictionHourReasonCodeEnum>"],
    ];
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainWindPredictionHour"),
      predictionFields,
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "RainWindPredictionHourReasonCodeEnum"),
      [
        ["phase_unsupported", "PhaseUnsupported"],
        ["prediction_invalid", "PredictionInvalid"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "RainWindPredictionResult"), [
      ["modelSha256", "String"],
      ["modelMonth", "String"],
      ["decisionAt", "String"],
      ["hours", "Vec<RainWindPredictionHour>"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainHurdleWindFeaturePerformance"),
      [
        ["correctedPrecipitationMm", "f64"],
        ["occurrenceProbabilityAtLeast0_1", "f64"],
        ["occurrenceProbabilityAtLeast1_0", "f64"],
        ["occurrenceProbabilityAtLeast2_5", "f64"],
        ["positiveAmountMm", "f64"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainWindPerformanceHour"),
      [
        ...predictionFields,
        [
          "occurrenceProbabilities",
          "Nullable<RainWindPerformanceHourOccurrenceProbabilities>",
        ],
        ["positiveAmountMm", "Nullable<f64>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "RainWindPerformanceHourOccurrenceProbabilities",
      ),
      [
        ["atLeast0_1", "f64"],
        ["atLeast1_0", "f64"],
        ["atLeast2_5", "f64"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainWindPerformanceResult"),
      [
        ["modelSha256", "String"],
        ["modelMonth", "String"],
        ["decisionAt", "String"],
        ["hours", "Vec<RainWindPerformanceHour>"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "RainHurdleWindFeatureRow"), [
      ["features", "Vec<f32>"],
      ["modelLeadHours", "f64"],
      ["rawPrecipitationMm", "f64"],
      ["rawTargetHourTemperatureC", "f64"],
      ["validAt", "String"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainHurdleWindFeatureProjection"),
      [
        ["decisionAt", "String"],
        ["rows", "Vec<RainHurdleWindFeatureRow>"],
        ["runInitializedAt", "String"],
      ],
    );
    assert.match(
      rustStructFieldBlock(rust, "RainWindPredictionResult", "modelMonth"),
      /storage rule: numeric-pair-template, source uses two number placeholders separated by a hyphen; not a calendar validator/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "RainHurdleWindFeatureRow", "features"),
      /storage rule: float32array-values, owned binary32 values; not a JSON wire type/u,
    );
    const timestampFields = [
      ["RainWindRunProfile", "runInitializedAt"],
      ["RainWindRunProfile", "completedAt"],
      ["RainWindStationHour", "hourAt"],
      ["RainWindStationHour", "receivedAt"],
      ["RainWindPredictionInput", "nowUtc"],
      ["RainWindPredictionHour", "validAt"],
      ["RainWindPredictionResult", "decisionAt"],
      ["RainWindPerformanceHour", "validAt"],
      ["RainWindPerformanceResult", "decisionAt"],
      ["RainHurdleWindFeatureRow", "validAt"],
      ["RainHurdleWindFeatureProjection", "decisionAt"],
      ["RainHurdleWindFeatureProjection", "runInitializedAt"],
    ];
    // keep every timestamp-like source field as an uninterpreted string
    for (const [name, field] of timestampFields) {
      assert.doesNotMatch(
        rustStructFieldBlock(rust, name, field),
        /wire rule: utc-iso/u,
      );
    }
    assertManifestRootGroup(manifest.roots, RAIN_INFERENCE_ROOTS);
  },
);

test(
  "admits only exact rain month strings and Float32Array values",
  // admit only the pinned textual month shape and builtin binary32 vector
  () => {
    const config = fixtureConfig("Root", {
      modelMonth: "numeric-pair-template",
      features: "float32array-values",
    });
    const source = `export interface Root {
  readonly modelMonth: \`\${number}-\${number}\`;
  readonly features: Float32Array;
}
`;
    const { rust } = generateRustContracts(config, { "fixture.ts": source });

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["modelMonth", "String"],
      ["features", "Vec<f32>"],
    ]);
    assert.match(
      rustStructFieldBlock(rust, "Root", "modelMonth"),
      /storage rule: numeric-pair-template, source uses two number placeholders separated by a hyphen; not a calendar validator/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "Root", "features"),
      /storage rule: float32array-values, owned binary32 values; not a JSON wire type/u,
    );

    const shapeCases = [
      {
        name: "month separator",
        source: `export interface Root {
  readonly modelMonth: \`\${number}/\${number}\`;
  readonly features: Float32Array;
}
`,
        error: /numeric-pair-template requires a required direct two-number hyphen template at Root\.modelMonth/u,
      },
      {
        name: "month placeholder",
        source: `export interface Root {
  readonly modelMonth: \`\${string}-\${number}\`;
  readonly features: Float32Array;
}
`,
        error: /numeric-pair-template requires a required direct two-number hyphen template at Root\.modelMonth/u,
      },
      {
        name: "escaped month separator",
        source: 'export interface Root {\n  readonly modelMonth: `${number}\\u002d${number}`;\n  readonly features: Float32Array;\n}\n',
        error: /numeric-pair-template requires a required direct two-number hyphen template at Root\.modelMonth/u,
      },
      {
        name: "ordinary month string",
        source: "export interface Root { readonly modelMonth: string; readonly features: Float32Array; }\n",
        error: /numeric-pair-template requires a required direct two-number hyphen template at Root\.modelMonth/u,
      },
      {
        name: "optional month",
        source: `export interface Root {
  readonly modelMonth?: \`\${number}-\${number}\`;
  readonly features: Float32Array;
}
`,
        error: /numeric-pair-template requires a required direct two-number hyphen template at Root\.modelMonth/u,
      },
      {
        name: "nullable month",
        source: `export interface Root {
  readonly modelMonth: \`\${number}-\${number}\` | null;
  readonly features: Float32Array;
}
`,
        error: /numeric-pair-template requires a required direct two-number hyphen template at Root\.modelMonth/u,
      },
      {
        name: "binary64 vector",
        source: `export interface Root {
  readonly modelMonth: \`\${number}-\${number}\`;
  readonly features: Float64Array;
}
`,
        error: /float32array-values requires a required direct Float32Array field at Root\.features/u,
      },
      {
        name: "generic binary32 vector",
        source: `export interface Root {
  readonly modelMonth: \`\${number}-\${number}\`;
  readonly features: Float32Array<number>;
}
`,
        error: /float32array-values requires a required direct Float32Array field at Root\.features/u,
      },
      {
        name: "nullable binary32 vector",
        source: `export interface Root {
  readonly modelMonth: \`\${number}-\${number}\`;
  readonly features: Float32Array | null;
}
`,
        error: /float32array-values requires a required direct Float32Array field at Root\.features/u,
      },
      {
        name: "aliased binary32 vector",
        source: `type Values = Float32Array;
export interface Root {
  readonly modelMonth: \`\${number}-\${number}\`;
  readonly features: Values;
}
`,
        error: /float32array-values requires a required direct Float32Array field at Root\.features/u,
      },
      {
        name: "optional binary32 vector",
        source: `export interface Root {
  readonly modelMonth: \`\${number}-\${number}\`;
  readonly features?: Float32Array;
}
`,
        error: /float32array-values requires a required direct Float32Array field at Root\.features/u,
      },
      {
        name: "shadowed binary32 vector",
        source: `export interface Float32Array { readonly fake: string; }
export interface Root {
  readonly modelMonth: \`\${number}-\${number}\`;
  readonly features: Float32Array;
}
`,
        error: /float32array-values builtin identity unsupported at Root\.features/u,
      },
    ];
    // reject every wider syntax before it can change Rust storage semantics
    for (const { name, source: invalidSource, error } of shapeCases) {
      assert.throws(
        () => generateRustContracts(config, { "fixture.ts": invalidSource }),
        error,
        name,
      );
    }

    assert.throws(
      // reject the numeric template without its explicit storage opt-in
      () => generateRustContracts(
        fixtureConfig("Root"),
        {
          "fixture.ts": `export interface Root {
  readonly modelMonth: \`\${number}-\${number}\`;
}
`,
        },
      ),
      /TypeScript type unsupported at Root\.modelMonth: TemplateLiteralType/u,
    );
    assert.throws(
      // reject the typed array as an unconfigured nominal reference
      () => generateRustContracts(
        fixtureConfig("Root"),
        {
          "fixture.ts": "export interface Root { readonly features: Float32Array; }\n",
        },
      ),
      /configured type reference identity unsupported at Root\.features/u,
    );
  },
);

test(
  "projects the original portable rain model artifact",
  // retain the closed artifact versions, numerical heads, and projection catalog
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStringEnumWireEntries(rust, "RainHurdleWindProjectionId"),
      [
        ["R0_exact_refit", "R0ExactRefit"],
        ["R1_winter_scale_0_90", "R1WinterScale090"],
        ["R2_winter_scale_0_95", "R2WinterScale095"],
        ["R3_spring_wet_logit_plus_0_20", "R3SpringWetLogitPlus020"],
        ["R4_summer_wet_logit_plus_0_20", "R4SummerWetLogitPlus020"],
        ["R5_nested_cumulative_min", "R5NestedCumulativeMin"],
        ["R6_heavy_raw_blend_0_25", "R6HeavyRawBlend025"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainHurdleWindPortableArtifactV1"),
      [
        [
          "contractVersion",
          "RainHurdleWindPortableArtifactV1ContractVersionLiteral",
        ],
        ["modelMonth", "String"],
        ["featureNames", "Vec<String>"],
        ["rules", "Vec<RainHurdleWindPortableArtifactV1Rules>"],
        ["categoryScales", "Vec<f64>"],
        ["heads", "RainHurdleWindPortableArtifactV1Heads"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainHurdleWindPortableArtifactV1Rules"),
      [
        ["threshold", "f64"],
        ["cutoff", "Nullable<f64>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainHurdleWindPortableArtifactV1Heads"),
      [
        ["atLeast0_1", "RainHurdleWindPortableArtifactV1HeadsNativeHead"],
        ["atLeast1_0", "RainHurdleWindPortableArtifactV1HeadsNativeHead"],
        ["atLeast2_5", "RainHurdleWindPortableArtifactV1HeadsNativeHead"],
        ["amount", "RainHurdleWindPortableArtifactV1HeadsNativeHead"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "RainHurdleWindPortableArtifactV1HeadsNativeHead",
      ),
      [
        [
          "objective",
          "RainHurdleWindPortableArtifactV1HeadsObjectiveEnum",
        ],
        ["baseScore", "f64"],
        ["trees", "Vec<[Vec<f64>; 5]>"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "RainHurdleWindPortableArtifactV1HeadsObjectiveEnum",
      ),
      [
        ["binary:logistic", "BinaryLogistic"],
        ["reg:gamma", "RegGamma"],
      ],
    );
    assert.match(
      rust,
      /impl RainHurdleWindPortableArtifactV1Heads \{\s*pub const WIRE_KEYS: \[&'static str; 4\] = \["0\.1", "1\.0", "2\.5", "amount"\];\s*\}/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "RainHurdleWindPortableArtifactV1",
        "categoryScales",
      ),
      /storage rule: runtime-f64-array, ordered binary64 values; not a JSON wire type/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "RainHurdleWindPortableArtifactV1",
        "heads",
      ),
      /storage rule: rain-hurdle-head-record, four exact head keys and five ordered binary64 arrays per tree/u,
    );
    assert.match(
      rust,
      /impl RainHurdleWindPortableArtifactV1ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "rain-hurdle-wind-runtime\/v1";\s*\}/u,
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "RainHurdleWindPortableArtifactV2"),
      [
        ["modelMonth", "String"],
        ["featureNames", "Vec<String>"],
        ["rules", "Vec<RainHurdleWindPortableArtifactV1Rules>"],
        ["categoryScales", "Vec<f64>"],
        ["heads", "RainHurdleWindPortableArtifactV1Heads"],
        [
          "contractVersion",
          "RainHurdleWindPortableArtifactV2ContractVersionLiteral",
        ],
        ["projectionId", "RainHurdleWindProjectionId"],
      ],
    );
    assert.match(
      rust,
      /impl RainHurdleWindPortableArtifactV2ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "rain-hurdle-wind-runtime\/v2";\s*\}/u,
    );
    assert.match(
      rust,
      /pub enum RainHurdleWindPortableArtifact \{\s*V1\(RainHurdleWindPortableArtifactV1\),\s*V2\(RainHurdleWindPortableArtifactV2\),\s*\}/u,
    );
    assertManifestRootGroup(manifest.roots, PORTABLE_RAIN_ARTIFACT_ROOTS);
  },
);

test(
  "admits only exact fixed rain artifact head and tree data",
  // admit one fixed head record backed by private exact tree geometry
  () => {
    const config = fixtureConfig("Root", {
      categoryScales: "runtime-f64-array",
      heads: "rain-hurdle-head-record",
    });
    const source = `type Tree = readonly [
  readonly number[],
  readonly number[],
  readonly number[],
  readonly number[],
  readonly number[],
];
interface NativeHead {
  readonly objective: "binary:logistic" | "reg:gamma";
  readonly baseScore: number;
  readonly trees: readonly Tree[];
}
export interface Root {
  readonly categoryScales: readonly number[];
  readonly heads: Readonly<Record<"0.1" | "1.0" | "2.5" | "amount", NativeHead>>;
}
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["categoryScales", "Vec<f64>"],
      ["heads", "RootHeads"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootHeads"), [
      ["atLeast0_1", "RootHeadsNativeHead"],
      ["atLeast1_0", "RootHeadsNativeHead"],
      ["atLeast2_5", "RootHeadsNativeHead"],
      ["amount", "RootHeadsNativeHead"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootHeadsNativeHead"), [
      ["objective", "RootHeadsObjectiveEnum"],
      ["baseScore", "f64"],
      ["trees", "Vec<[Vec<f64>; 5]>"],
    ]);
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "RootHeadsObjectiveEnum"),
      [
        ["binary:logistic", "BinaryLogistic"],
        ["reg:gamma", "RegGamma"],
      ],
    );
    assert.match(
      rust,
      /impl RootHeads \{\s*pub const WIRE_KEYS: \[&'static str; 4\] = \["0\.1", "1\.0", "2\.5", "amount"\];\s*\}/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "Root", "categoryScales"),
      /storage rule: runtime-f64-array, ordered binary64 values; not a JSON wire type/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "Root", "heads"),
      /storage rule: rain-hurdle-head-record, four exact head keys and five ordered binary64 arrays per tree/u,
    );
    assert.deepEqual(manifest.roots, [{
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-interface",
    }]);

    const invalidCases = [
      {
        name: "changed head key",
        source: source.replace('"0.1" | "1.0"', '"0.2" | "1.0"'),
        error: /rain-hurdle-head-record requires exact ordered head keys at Root\.heads/u,
      },
      {
        name: "reordered head keys",
        source: source.replace(
          '"0.1" | "1.0" | "2.5" | "amount"',
          '"amount" | "0.1" | "1.0" | "2.5"',
        ),
        error: /rain-hurdle-head-record requires exact ordered head keys at Root\.heads/u,
      },
      {
        name: "exported private head",
        source: source.replace("interface NativeHead", "export interface NativeHead"),
        error: /rain-hurdle-head-record private type identity unsupported at Root\.heads/u,
      },
      {
        name: "generic private head",
        source: source.replace("interface NativeHead", "interface NativeHead<Unused>"),
        error: /rain-hurdle-head-record private type identity unsupported at Root\.heads/u,
      },
      {
        name: "renamed private head",
        source: source.replaceAll("NativeHead", "OtherHead"),
        error: /rain-hurdle-head-record private type identity unsupported at Root\.heads/u,
      },
      {
        name: "reordered head fields",
        source: source.replace(
          '  readonly objective: "binary:logistic" | "reg:gamma";\n  readonly baseScore: number;',
          '  readonly baseScore: number;\n  readonly objective: "binary:logistic" | "reg:gamma";',
        ),
        error: /rain-hurdle-head-record private head fields unsupported at Root\.heads/u,
      },
      {
        name: "optional base score",
        source: source.replace("readonly baseScore: number", "readonly baseScore?: number"),
        error: /rain-hurdle-head-record private head fields unsupported at Root\.heads/u,
      },
      {
        name: "reordered objectives",
        source: source.replace(
          '"binary:logistic" | "reg:gamma"',
          '"reg:gamma" | "binary:logistic"',
        ),
        error: /rain-hurdle-head-record private head types unsupported at Root\.heads/u,
      },
      {
        name: "nullable base score",
        source: source.replace("readonly baseScore: number", "readonly baseScore: number | null"),
        error: /rain-hurdle-head-record private head types unsupported at Root\.heads/u,
      },
      {
        name: "mutable tree list",
        source: source.replace("readonly trees: readonly Tree[]", "readonly trees: Tree[]"),
        error: /rain-hurdle-head-record private head types unsupported at Root\.heads/u,
      },
      {
        name: "four-array tree",
        source: source.replace("  readonly number[],\n];", "];"),
        error: /rain-hurdle-head-record requires five direct readonly number\[\] tree members at Root\.heads/u,
      },
      {
        name: "mutable tree member",
        source: source.replace("  readonly number[],", "  number[],"),
        error: /rain-hurdle-head-record requires five direct readonly number\[\] tree members at Root\.heads/u,
      },
      {
        name: "exported private tree",
        source: source.replace("type Tree", "export type Tree"),
        error: /rain-hurdle-head-record private type identity unsupported at Root\.heads/u,
      },
      {
        name: "renamed private tree",
        source: source.replaceAll("Tree", "OtherTree"),
        error: /rain-hurdle-head-record private type identity unsupported at Root\.heads/u,
      },
      {
        name: "mutable category scales",
        source: source.replace(
          "readonly categoryScales: readonly number[]",
          "readonly categoryScales: number[]",
        ),
        error: /runtime-f64-array requires a required direct readonly number\[\] field at Root\.categoryScales/u,
      },
      {
        name: "nullable category scales",
        source: source.replace(
          "readonly categoryScales: readonly number[]",
          "readonly categoryScales: readonly number[] | null",
        ),
        error: /runtime-f64-array requires a required direct readonly number\[\] field at Root\.categoryScales/u,
      },
    ];
    // reject every changed private identity, fixed key, head field, or vector shape
    for (const { name, source: invalidSource, error } of invalidCases) {
      assert.throws(
        () => generateRustContracts(config, { "fixture.ts": invalidSource }),
        error,
        name,
      );
    }

    const extensionRoot = {
      source: "fixture.ts",
      exportName: "Extended",
      base: "Base",
      omitBaseFields: ["replaced"],
      addedAfterOmit: ["appended"],
      fields: {},
    };
    const extensionConfig = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        extensionRoot,
      ],
    };
    const extensionSource = `export interface Base {
  readonly retained: string;
  readonly replaced: string;
}
export interface Extended extends Omit<Base, "replaced"> {
  readonly replaced: string;
  readonly appended: string;
}
`;
    const { rust: extensionRust, manifest: extensionManifestText } =
      generateRustContracts(extensionConfig, { "fixture.ts": extensionSource });
    assert.deepEqual(rustStructFieldEntries(extensionRust, "Extended"), [
      ["retained", "String"],
      ["replaced", "String"],
      ["appended", "String"],
    ]);
    assert.deepEqual(JSON.parse(extensionManifestText).roots[1], {
      source: "fixture.ts",
      exportName: "Extended",
      rustName: "Extended",
      kind: "readonly-interface",
      base: "Base",
      omitBaseFields: ["replaced"],
      addedAfterOmit: ["appended"],
    });

    const extensionCases = [
      {
        name: "addition without opt-in",
        root: {
          source: extensionRoot.source,
          exportName: extensionRoot.exportName,
          base: extensionRoot.base,
          omitBaseFields: extensionRoot.omitBaseFields,
          fields: extensionRoot.fields,
        },
        source: extensionSource,
        error: /configured omitted interface overrides unsupported at root Extended/u,
      },
      {
        name: "stale added field metadata",
        root: { ...extensionRoot, addedAfterOmit: ["stale"] },
        source: extensionSource,
        error: /configured omitted interface overrides unsupported at root Extended/u,
      },
      {
        name: "mismatched added source field",
        root: extensionRoot,
        source: extensionSource.replace("readonly appended: string", "readonly other: string"),
        error: /configured omitted interface overrides unsupported at root Extended/u,
      },
      {
        name: "added field before replacement",
        root: extensionRoot,
        source: extensionSource.replace(
          "  readonly replaced: string;\n  readonly appended: string;",
          "  readonly appended: string;\n  readonly replaced: string;",
        ),
        error: /configured omitted interface overrides unsupported at root Extended/u,
      },
      {
        name: "overlapping replacement and addition",
        root: { ...extensionRoot, addedAfterOmit: ["replaced"] },
        source: extensionSource,
        error: /config\.roots\[1\]\.addedAfterOmit requires distinct names not in omitBaseFields/u,
      },
    ];
    // bind every allowed addition to exact metadata and source declaration order
    for (const fixture of extensionCases) {
      assert.throws(
        () => generateRustContracts(
          { ...extensionConfig, roots: [extensionConfig.roots[0], fixture.root] },
          { "fixture.ts": fixture.source },
        ),
        fixture.error,
        fixture.name,
      );
    }

    const catalogConfig = fixtureConfig("Projection");
    catalogConfig.roots[0].catalog = { kind: "direct-const-string-tuple" };
    const catalogSource = `const PROJECTIONS = ["first", "second"] as const;
export type Projection = typeof PROJECTIONS[number];
`;
    const { rust: catalogRust, manifest: catalogManifestText } =
      generateRustContracts(catalogConfig, { "fixture.ts": catalogSource });
    assert.deepEqual(
      rustStringEnumWireEntries(catalogRust, "Projection"),
      [["first", "First"], ["second", "Second"]],
    );
    assert.deepEqual(JSON.parse(catalogManifestText).roots[0].catalog, {
      kind: "direct-const-string-tuple",
    });
    assert.throws(
      // reject the older parenthesized grammar under the direct catalog opt-in
      () => generateRustContracts(
        catalogConfig,
        {
          "fixture.ts": catalogSource.replace(
            "typeof PROJECTIONS[number]",
            "(typeof PROJECTIONS)[number]",
          ),
        },
      ),
      /const string tuple root unsupported at Projection: expected typeof LOCAL_CONST\[number\]/u,
    );
    assert.throws(
      // reject direct query syntax through the ordinary parenthesized grammar
      () => generateRustContracts(
        fixtureConfig("Projection"),
        { "fixture.ts": catalogSource },
      ),
      /const string tuple root unsupported at Projection: expected \(typeof LOCAL_CONST\)\[number\]/u,
    );
  },
);

test(
  "projects the original worker backfill source data",
  // retain one shared identity, exact concrete additions, and canonical deadlines
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const identityFields = [
      ["materialProviderConfig", "JsonValue"],
      ["providerKey", "String"],
      ["siteSlug", "String"],
      ["sourceConfigFingerprint", "String"],
      ["sourceKey", "String"],
      ["sourceKind", "SourceKind"],
      ["stationSlug", "String"],
      ["timezone", "String"],
    ];

    assert.deepEqual(
      rustStructFieldEntries(rust, "RuntimeSourceIdentity"),
      identityFields,
    );
    assert.deepEqual(rustStructFieldEntries(rust, "BackfillSource"), [
      ...identityFields,
      ["id", "String"],
      ["key", "String"],
      ["latitude", "f64"],
      ["longitude", "f64"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "TideBackfillSource"), [
      ...identityFields,
      ["id", "String"],
      ["station", "TideStationConfiguration"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "PublicStationBackfillSource"),
      [
        ...identityFields,
        ["id", "String"],
        ["source", "PublicStationSourceConfiguration"],
        ["station", "PublicStationConfigurationStation"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "TempestBackfillSource"), [
      ["id", "String"],
      ["materialProviderConfig", "JsonValue"],
      ["providerKey", "String"],
      ["siteSlug", "String"],
      ["sourceConfigFingerprint", "String"],
      ["sourceKey", "String"],
      ["sourceKind", "SourceKind"],
      ["station", "TempestStationConfiguration"],
      ["stationSlug", "String"],
      ["timezone", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "IngestionDeadlines"), [
      ["providerDeadlineAt", "String"],
      ["runDeadlineAt", "String"],
    ]);
    // retain canonical UTC documentation on both toISOString results
    for (const field of ["providerDeadlineAt", "runDeadlineAt"]) {
      assert.match(
        rustStructFieldBlock(rust, "IngestionDeadlines", field),
        /wire rule: utc-iso, canonical UTC ISO-8601 string/u,
      );
    }
    // keep unrestricted coordinates distinct from finite wire-number storage
    for (const field of ["latitude", "longitude"]) {
      assert.match(
        rustStructFieldBlock(rust, "BackfillSource", field),
        /storage rule: runtime-f64/u,
      );
    }
    assertManifestRootGroup(manifest.roots, WORKER_BACKFILL_SOURCE_ROOTS);
    assertManifestImportGroup(manifest.typeImports, WORKER_BACKFILL_SOURCE_IMPORTS);
    assert.deepEqual(
      manifest.typeImports[24],
      TEMPEST_BACKFILL_DOMAIN_IMPORT,
    );
  },
);

test(
  "admits only exact repeated imported readonly strings",
  // fold one direct identical imported string while retaining base field order
  () => {
    const baseRoot = {
      source: "base.ts",
      exportName: "Base",
      fields: {},
    };
    const root = {
      source: "fixture.ts",
      exportName: "Root",
      base: "Base",
      importedBase: true,
      identicalImportedStringFields: ["repeated"],
      fields: {},
    };
    const typeImport = {
      consumer: "fixture.ts",
      specifier: "./base.js",
      entry: "base.ts",
      exports: { Base: "base.ts" },
    };
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [baseRoot, root],
      typeImports: [typeImport],
    };
    const baseSource = `export interface Base {
  readonly first: string;
  readonly repeated: string;
  readonly last: string;
}
`;
    const source = `import type { Base } from "./base.js";
export interface Root extends Base {
  readonly repeated: string;
  readonly own: string;
}
`;
    const sources = { "base.ts": baseSource, "fixture.ts": source };
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      sources,
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["first", "String"],
      ["repeated", "String"],
      ["last", "String"],
      ["own", "String"],
    ]);
    assert.deepEqual(manifest.roots[1], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-interface",
      base: "Base",
      importedBase: true,
      identicalImportedStringFields: ["repeated"],
    });
    assert.deepEqual(manifest.typeImports, [typeImport]);

    const invalidCases = [
      {
        name: "unconfigured duplicate",
        root: {
          source: root.source,
          exportName: root.exportName,
          base: root.base,
          importedBase: root.importedBase,
          fields: root.fields,
        },
        source,
        error: /inherited property redeclaration unsupported at Root\.repeated/u,
      },
      {
        name: "mismatched duplicate type",
        root,
        source: source.replace("readonly repeated: string", "readonly repeated: number"),
        error: /identical imported readonly string unsupported at Root\.repeated/u,
      },
      {
        name: "optional duplicate",
        root,
        source: source.replace("readonly repeated: string", "readonly repeated?: string"),
        error: /identical imported readonly string unsupported at Root\.repeated/u,
      },
      {
        name: "mutable duplicate",
        root,
        source: source.replace("readonly repeated: string", "repeated: string"),
        error: /identical imported readonly string unsupported at Root\.repeated/u,
      },
      {
        name: "mapped duplicate",
        root: { ...root, fields: { repeated: "utc-iso" } },
        source,
        error: /config\.roots\[1\]\.identicalImportedStringFields requires an imported base and unmapped fields/u,
      },
      {
        name: "stale repeated name",
        root: { ...root, identicalImportedStringFields: ["missing"] },
        source,
        error: /identical imported readonly string unsupported at Root\.missing/u,
      },
    ];
    // reject every unproved, changed, optional, mutable, mapped, or stale repeat
    for (const fixture of invalidCases) {
      assert.throws(
        () => generateRustContracts(
          { ...config, roots: [baseRoot, fixture.root] },
          { ...sources, "fixture.ts": fixture.source },
        ),
        fixture.error,
        fixture.name,
      );
    }
    assert.throws(
      // reject an inherited string whose base storage mapping adds metadata
      () => generateRustContracts(
        {
          ...config,
          roots: [{ ...baseRoot, fields: { repeated: "utc-iso" } }, root],
        },
        sources,
      ),
      /identical imported readonly string unsupported at Root\.repeated/u,
    );
  },
);

test(
  "projects the original private forecast repository rows",
  // retain raw database storage only at its reviewed private query boundaries
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "ForecastAnchorStorageRow"), [
      ["adapterVersion", "String"],
      ["apparentTemperatureC", "Nullable<f64>"],
      ["cloudCoverPercent", "Nullable<f64>"],
      ["contractEpoch", "String"],
      ["dataset", "ForecastAnchorStorageRowDatasetLiteral"],
      ["leadHours", "f64"],
      ["pressureHpa", "Nullable<f64>"],
      ["precipitationMm", "Nullable<f64>"],
      ["providerMetadata", "Nullable<Vec<(Vec<u16>, JsonValue)>>"],
      ["qualityMetadata", "Nullable<Vec<(Vec<u16>, JsonValue)>>"],
      ["receivedAt", "JsDateOrString"],
      ["relativeHumidityPercent", "Nullable<f64>"],
      ["sourceConfigFingerprint", "String"],
      ["sourceId", "String"],
      ["temperatureC", "Nullable<f64>"],
      ["upstreamModel", "ForecastAnchorStorageRowUpstreamModelLiteral"],
      ["upstreamTimezone", "String"],
      ["validAt", "JsDateOrString"],
      ["windDirectionDegrees", "Nullable<f64>"],
      ["windGustMps", "Nullable<f64>"],
      ["windSpeedMps", "Nullable<f64>"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "LegacyV4ForecastStorageRow"),
      [
        ["adapterVersion", "Nullable<String>"],
        ["apparentTemperatureC", "Nullable<f64>"],
        ["blackGlobeTemperatureC", "Nullable<f64>"],
        ["cloudCoverPercent", "Nullable<f64>"],
        ["dataset", "Nullable<String>"],
        ["pm25MicrogramsPerCubicMeter", "Nullable<f64>"],
        ["precipitationMm", "Nullable<f64>"],
        ["precipitationRateMmPerHour", "Nullable<f64>"],
        ["pressureHpa", "Nullable<f64>"],
        ["referenceAt", "JsDateOrString"],
        ["relativeHumidityPercent", "Nullable<f64>"],
        ["soilElectricalConductivityMicrosiemensPerCm", "Nullable<f64>"],
        ["soilMoisturePercent", "Nullable<f64>"],
        ["solarRadiationWm2", "Nullable<f64>"],
        ["sourceConfigFingerprint", "String"],
        ["sourceId", "String"],
        ["stableRecordId", "String"],
        ["temperatureC", "Nullable<f64>"],
        ["upstreamModel", "Nullable<String>"],
        ["uvIndex", "Nullable<f64>"],
        ["validAt", "JsDateOrString"],
        ["waterLevelM", "Nullable<f64>"],
        ["wetBulbGlobeTemperatureC", "Nullable<f64>"],
        ["windDirectionDegrees", "Nullable<f64>"],
        ["windGustMps", "Nullable<f64>"],
        ["windSpeedMps", "Nullable<f64>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastObservationStorageRow"),
      [
        ["adapterContract", "Nullable<String>"],
        ["id", "String"],
        ["qualityMetadata", "Nullable<Vec<(Vec<u16>, JsonValue)>>"],
        ["relativeHumidityPercent", "Nullable<f64>"],
        ["sourceConfigFingerprint", "String"],
        ["sourceKey", "String"],
        ["stationSlug", "String"],
        ["temperatureC", "Nullable<f64>"],
        ["validAt", "JsDateOrString"],
        ["windDirectionDegrees", "Nullable<f64>"],
        ["windGustMps", "Nullable<f64>"],
        ["windSpeedMps", "Nullable<f64>"],
      ],
    );
    assert.match(
      rust,
      /impl ForecastAnchorStorageRowDatasetLiteral \{\s*pub const WIRE_VALUE: &'static str = "previous_runs";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAnchorStorageRowUpstreamModelLiteral \{\s*pub const WIRE_VALUE: &'static str = "best_match";\s*\}/u,
    );
    const metadataFields = [
      ["ForecastAnchorStorageRow", "providerMetadata"],
      ["ForecastAnchorStorageRow", "qualityMetadata"],
      ["ForecastObservationStorageRow", "qualityMetadata"],
    ];
    // retain ordered nullable JSON storage on every raw metadata column
    for (const [name, field] of metadataFields) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /wire rule: ordered-json-object/u,
      );
    }
    const timestampFields = [
      ["ForecastAnchorStorageRow", "receivedAt"],
      ["ForecastAnchorStorageRow", "validAt"],
      ["LegacyV4ForecastStorageRow", "referenceAt"],
      ["LegacyV4ForecastStorageRow", "validAt"],
      ["ForecastObservationStorageRow", "validAt"],
    ];
    // retain raw Date or string alternatives without UTC normalization
    for (const [name, field] of timestampFields) {
      assert.match(
        rustStructFieldBlock(rust, name, field),
        /storage rule: js-date-or-string/u,
      );
    }
    assert.match(
      rustStructFieldBlock(rust, "ForecastAnchorStorageRow", "leadHours"),
      /storage rule: runtime-f64/u,
    );
    assertManifestRootGroup(manifest.roots, PRIVATE_FORECAST_REPOSITORY_ROW_ROOTS);
  },
);

test(
  "admits only private rows bound to original select owners",
  // bind one private row to an exact exported async Pool SELECT owner
  () => {
    const root = {
      source: "fixture.ts",
      exportName: "Root",
      fields: {},
      pgRowNamedFields: true,
      pgQueryOwners: ["read"],
    };
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [root],
    };
    const query = "SELECT t.value FROM data t WHERE t.id = $1";
    const source = `import type { Pool, QueryResultRow } from "pg";
interface Root extends QueryResultRow {
  readonly value: string;
}
export async function read(pool: Pool) {
  const rows = await pool.query<Root>("${query}", ["id"]);
  return rows.rows;
}
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["value", "String"],
    ]);
    assert.deepEqual(manifest.roots[0], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-interface",
      pgRowNamedFields: true,
      pgQueryOwners: ["read"],
    });
    assert.deepEqual(
      manifest.generator.pgRowMarkerInputSha256,
      PG_ROW_MARKER_INPUT_SHA256,
    );

    const invalidCases = [
      {
        name: "off-owner configuration",
        root: { ...root, pgQueryOwners: ["write"] },
        source,
        error: /private PostgreSQL row query owners unsupported at Root/u,
      },
      {
        name: "missing configured owner",
        root: { ...root, pgQueryOwners: ["read", "missing"] },
        source,
        error: /private PostgreSQL row query owners unsupported at Root/u,
      },
      {
        name: "unlisted valid use",
        root,
        source: source + `export async function readAgain(pool: Pool) {
  const rows = await pool.query<Root>("${query}", ["id"]);
  return rows.rows;
}
`,
        error: /private PostgreSQL row query owners unsupported at Root/u,
      },
      {
        name: "wrong Pool source",
        root,
        source: source.replace(
          'import type { Pool, QueryResultRow } from "pg";',
          'import type { QueryResultRow } from "pg";\nimport type { Pool } from "other";',
        ),
        error: /private PostgreSQL row query site unsupported at Root/u,
      },
      {
        name: "missing Pool annotation",
        root,
        source: source.replace("read(pool: Pool)", "read(pool)"),
        error: /private PostgreSQL row query site unsupported at Root/u,
      },
      {
        name: "nonliteral query",
        root,
        source: source
          .replace(`pool.query<Root>("${query}"`, "pool.query<Root>(SQL")
          .replace(
            "interface Root extends QueryResultRow",
            `const SQL = "${query}";\ninterface Root extends QueryResultRow`,
          ),
        error: /private PostgreSQL row query site unsupported at Root/u,
      },
      {
        name: "query config object",
        root,
        source: source.replace(
          `"${query}"`,
          `{ text: "${query}" }`,
        ),
        error: /private PostgreSQL row query site unsupported at Root/u,
      },
      {
        name: "unawaited query",
        root,
        source: source.replace("const rows = await pool.query", "const rows = pool.query"),
        error: /private PostgreSQL row query site unsupported at Root/u,
      },
      {
        name: "arrow-function owner",
        root,
        source: source
          .replace(
            "export async function read(pool: Pool) {",
            "export const read = async (pool: Pool) => {",
          )
          .replace("  return rows.rows;\n}\n", "  return rows.rows;\n};\n"),
        error: /private PostgreSQL row query site unsupported at Root/u,
      },
      {
        name: "wrong output alias",
        root,
        source: source.replace(
          query,
          "SELECT t.value AS other FROM data t WHERE t.id = $1",
        ),
        error: /private PostgreSQL row SELECT fields unsupported at Root/u,
      },
      {
        name: "duplicate output field",
        root,
        source: source.replace(
          query,
          "SELECT t.value, t.value FROM data t WHERE t.id = $1",
        ),
        error: /private PostgreSQL row SELECT fields unsupported at Root/u,
      },
      {
        name: "subquery projection",
        root,
        source: source.replace(
          query,
          "SELECT (SELECT t.value) AS value FROM data t WHERE t.id = $1",
        ),
        error: /private PostgreSQL row SELECT fields unsupported at Root/u,
      },
      {
        name: "empty bindings",
        root,
        source: source.replace('["id"]', "[]"),
        error: /private PostgreSQL row SELECT bindings unsupported at Root/u,
      },
      {
        name: "extra binding",
        root,
        source: source.replace('["id"]', '["id", "extra"]'),
        error: /private PostgreSQL row SELECT bindings unsupported at Root/u,
      },
      {
        name: "missing first placeholder",
        root,
        source: source.replace("t.id = $1", "t.id = $2"),
        error: /private PostgreSQL row SELECT bindings unsupported at Root/u,
      },
      {
        name: "noncontiguous placeholders",
        root,
        source: source
          .replace("t.id = $1", "t.id = $1 AND t.other = $3")
          .replace('["id"]', '["id", "other"]'),
        error: /private PostgreSQL row SELECT bindings unsupported at Root/u,
      },
      {
        name: "exported private row",
        root,
        source: source.replace(
          "interface Root extends QueryResultRow",
          "export interface Root extends QueryResultRow",
        ),
        error: /private PostgreSQL query row must not be exported fixture\.ts#Root/u,
      },
    ];
    // reject every unowned, unbound, reshaped, or exported private query row
    for (const fixture of invalidCases) {
      assert.throws(
        () => generateRustContracts(
          { ...config, roots: [fixture.root] },
          { "fixture.ts": fixture.source },
        ),
        fixture.error,
        fixture.name,
      );
    }
  },
);

test(
  "projects the original forecast scorecard and fixed-gauge contracts",
  // retain every supported public scorecard declaration in source order
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const enumValues = [
      ["ForecastAdjustmentEvidenceClass", [
        ["as_issued", "AsIssued"],
        ["prospective_receipt", "ProspectiveReceipt"],
        ["retrospective_counterfactual", "RetrospectiveCounterfactual"],
        ["development", "Development"],
      ]],
      ["ForecastAdjustmentSupportState", [
        ["sufficient", "Sufficient"],
        ["insufficient", "Insufficient"],
        ["invalid", "Invalid"],
      ]],
      ["ForecastAdjustmentComparisonState", [
        ["unscored", "Unscored"],
        ["better", "Better"],
        ["mixed", "Mixed"],
        ["worse", "Worse"],
      ]],
      ["ForecastAdjustmentQualificationState", [
        ["development_only", "DevelopmentOnly"],
        ["counterfactual_only", "CounterfactualOnly"],
        ["pending_support", "PendingSupport"],
        ["supported", "Supported"],
        ["rejected", "Rejected"],
      ]],
      ["ForecastAdjustmentServingState", [
        ["authorized_active", "AuthorizedActive"],
        ["admin_disabled", "AdminDisabled"],
        ["fail_raw", "FailRaw"],
        ["pending_review", "PendingReview"],
      ]],
    ];
    // preserve exact wire values for every closed scorecard state catalog
    for (const [name, values] of enumValues) {
      assert.deepEqual(rustStringEnumWireEntries(rust, name), values);
    }

    const pairFields = [
      ["adjustedPrediction", "f64"],
      ["evidenceClass", "ForecastAdjustmentEvidenceClass"],
      ["fallback", "bool"],
      ["firstEdgeCommittedAt", "Nullable<String>"],
      ["horizonHours", "f64"],
      ["key", "String"],
      ["localDate", "String"],
      ["provenanceComplete", "bool"],
      ["rawPrediction", "f64"],
      ["rowIdentity", "String"],
      ["sourceReceiptAt", "Nullable<String>"],
      ["target", "Nullable<f64>"],
      ["targetKey", "String"],
      ["validAt", "String"],
      ["vintageKey", "String"],
    ];
    const structFields = [
      ["ForecastAdjustmentPerformancePair", pairFields],
      ["ForecastAdjustmentWeightedPair", [...pairFields, ["weight", "f64"]]],
      ["ForecastAdjustmentMetricSummary", [
        ["bias", "f64"],
        ["mae", "f64"],
        ["p95AbsoluteError", "f64"],
        ["rmse", "f64"],
      ]],
      ["ForecastAdjustmentPairedMetricSummary", [
        ["adjusted", "ForecastAdjustmentMetricSummary"],
        ["delta", "ForecastAdjustmentMetricSummary"],
        ["raw", "ForecastAdjustmentMetricSummary"],
        ["skill", "f64"],
      ]],
      ["ForecastAdjustmentBootstrapSummary", [
        [
          "contractVersion",
          "ForecastAdjustmentBootstrapSummaryContractVersionLiteral",
        ],
        ["lowerSkill", "f64"],
        ["replicates", "ForecastAdjustmentBootstrapSummaryReplicatesLiteral"],
        ["upperSkill", "f64"],
      ]],
      ["ForecastAdjustmentPerformanceEvaluation", [
        ["bootstrap", "Nullable<ForecastAdjustmentBootstrapSummary>"],
        ["comparisonState", "ForecastAdjustmentComparisonState"],
        ["dateCount", "u64"],
        ["evidenceClass", "ForecastAdjustmentEvidenceClass"],
        ["eventCount", "u64"],
        ["metrics", "Nullable<ForecastAdjustmentPairedMetricSummary>"],
        ["qualificationState", "ForecastAdjustmentQualificationState"],
        ["servingState", "ForecastAdjustmentServingState"],
        ["supportState", "ForecastAdjustmentSupportState"],
        ["weightedRows", "Vec<ForecastAdjustmentWeightedPair>"],
      ]],
      ["ForecastAdjustmentStationTargetValue", [
        ["physicalStationKey", "ForecastObservationStationKey"],
        ["value", "f64"],
      ]],
      ["ForecastAdjustmentRainGaugeValue", [
        ["precipitationMm", "Nullable<f64>"],
        ["stationId", "f64"],
      ]],
      ["ForecastAdjustmentRainGaugeTarget", [
        ["complete", "bool"],
        ["precipitationMm", "Nullable<f64>"],
        ["stationCount", "u64"],
      ]],
      ["ForecastAdjustmentRainGaugeInterval", [
        ["precipitationMm", "f64"],
        ["reportIntervalMinutes", "f64"],
        ["reportedAt", "String"],
        ["stationId", "f64"],
      ]],
      ["ForecastAdjustmentRainHourlyTarget", [
        ["complete", "bool"],
        ["precipitationMm", "Nullable<f64>"],
        ["stationCount", "u64"],
        ["eligible", "bool"],
        ["reason", "ForecastAdjustmentRainHourlyTargetReasonEnum"],
      ]],
      ["ForecastAdjustmentRainProbability", [
        ["atLeast0_1", "f64"],
        ["atLeast1_0", "f64"],
        ["atLeast2_5", "f64"],
      ]],
      ["ForecastAdjustmentRainPerformancePair", [
        ...pairFields,
        ["adjustedProbability", "ForecastAdjustmentRainProbability"],
        ["amountParity", "bool"],
        ["runKey", "String"],
        ["targetTilingComplete", "bool"],
      ]],
      ["ForecastAdjustmentReliabilityBin", [
        ["count", "u64"],
        ["meanProbability", "Nullable<f64>"],
        ["observedFrequency", "Nullable<f64>"],
      ]],
      ["ForecastAdjustmentRainThresholdSummary", [
        ["adjustedBrier", "f64"],
        ["csi", "Nullable<f64>"],
        ["falseAlarms", "u64"],
        ["far", "Nullable<f64>"],
        ["hits", "u64"],
        ["misses", "u64"],
        ["pod", "Nullable<f64>"],
        ["rawBrier", "ForecastAdjustmentRainThresholdSummaryRawBrierNull"],
        ["reliability", "Vec<ForecastAdjustmentReliabilityBin>"],
        [
          "thresholdMmPerHour",
          "ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum",
        ],
      ]],
      ["ForecastAdjustmentRainAccumulationSummary", [
        ["adjustedMae", "Nullable<f64>"],
        ["completeWindows", "u64"],
        ["hours", "ForecastAdjustmentRainAccumulationSummaryHoursEnum"],
        ["rawMae", "Nullable<f64>"],
      ]],
      ["ForecastAdjustmentRainDiagnostics", [
        ["accumulations", "Vec<ForecastAdjustmentRainAccumulationSummary>"],
        ["annualBalancedVolumeRatio", "Nullable<f64>"],
        ["heavyAdjustedMae", "Nullable<f64>"],
        ["heavyRawMae", "Nullable<f64>"],
        ["probabilityOrderViolationCount", "u64"],
        ["thresholds", "Vec<ForecastAdjustmentRainThresholdSummary>"],
        ["wetAdjustedMae", "Nullable<f64>"],
        ["wetRawMae", "Nullable<f64>"],
        ["winterBalancedVolumeRatio", "Nullable<f64>"],
      ]],
    ];
    // require every struct to retain exact flattened source field order and storage
    for (const [name, fields] of structFields) {
      assert.deepEqual(rustStructFieldEntries(rust, name), fields);
    }

    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ForecastAdjustmentRainHourlyTargetReasonEnum"),
      [
        ["eligible", "Eligible"],
        ["cold_or_unknown_forecast_phase", "ColdOrUnknownForecastPhase"],
        ["unsupported_target", "UnsupportedTarget"],
      ],
    );
    assert.deepEqual(
      rustUnsignedIntegerEnumWireEntries(
        rust,
        "ForecastAdjustmentRainAccumulationSummaryHoursEnum",
      ),
      [[6, "Value6"], [12, "Value12"], [23, "Value23"]],
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentBootstrapSummaryContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "moving-block-bootstrap\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentBootstrapSummaryReplicatesLiteral \{\s*pub const WIRE_VALUE: u64 = 2000;\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentRainThresholdSummaryThresholdMmPerHourEnum \{\s*pub const WIRE_VALUES: &'static \[\(f64, Self\)\] = &\[\s*\(f64::from_bits\(0x3fb999999999999a\), Self::Value0Point1\),\s*\(f64::from_bits\(0x3ff0000000000000\), Self::Value1\),\s*\(f64::from_bits\(0x4004000000000000\), Self::Value2Point5\),\s*\];\s*\}/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "ForecastAdjustmentBootstrapSummary",
        "replicates",
      ),
      /wire rule: grouped-unsigned-safe-integer-literal/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "ForecastAdjustmentRainThresholdSummary",
        "thresholdMmPerHour",
      ),
      /wire rule: unsigned-decimal-enum/u,
    );
    const chronologyFields = [
      ["ForecastAdjustmentPerformancePair", "firstEdgeCommittedAt"],
      ["ForecastAdjustmentPerformancePair", "sourceReceiptAt"],
      ["ForecastAdjustmentPerformancePair", "validAt"],
      ["ForecastAdjustmentRainGaugeInterval", "reportedAt"],
    ];
    // keep original chronology strings free of inferred UTC normalization
    for (const [name, field] of chronologyFields) {
      assert.doesNotMatch(
        rustStructFieldBlock(rust, name, field),
        /wire rule: utc-iso/u,
      );
    }
    assertManifestRootGroup(manifest.roots, FORECAST_SCORECARD_ROOTS);
    assert.deepEqual(
      manifest.typeImports.find(
        // select the scorecard edge without coupling the fixture to unrelated imports
        ({ consumer, specifier }) =>
          consumer === "packages/forecast-adjustment/src/performance-scorecard.ts" &&
          specifier === "@weather/domain",
      ),
      FORECAST_SCORECARD_DOMAIN_IMPORT,
    );
  },
);

test(
  "requires exact grouped integer and unsigned decimal number literals",
  // preserve one grouped integer marker and one ordered decimal enum
  () => {
    const config = fixtureConfig("Root", {
      replicates: "grouped-unsigned-safe-integer-literal",
      thresholds: "unsigned-decimal-enum",
    });
    const source = `export interface Root {
  readonly replicates: 2_000;
  readonly thresholds: 0.1 | 1 | 2.5;
}
`;
    const { rust } = generateRustContracts(config, { "fixture.ts": source });

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["replicates", "RootReplicatesLiteral"],
      ["thresholds", "RootThresholdsEnum"],
    ]);
    assert.match(
      rust,
      /impl RootReplicatesLiteral \{\s*pub const WIRE_VALUE: u64 = 2000;\s*\}/u,
    );
    assert.match(
      rust,
      /impl RootThresholdsEnum \{\s*pub const WIRE_VALUES: &'static \[\(f64, Self\)\] = &\[\s*\(f64::from_bits\(0x3fb999999999999a\), Self::Value0Point1\),\s*\(f64::from_bits\(0x3ff0000000000000\), Self::Value1\),\s*\(f64::from_bits\(0x4004000000000000\), Self::Value2Point5\),\s*\];\s*\}/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "Root", "replicates"),
      /wire rule: grouped-unsigned-safe-integer-literal, one source integer grouped by thousands with an exact safe unsigned value/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "Root", "thresholds"),
      /wire rule: unsigned-decimal-enum, exact nonnegative binary64 literal values/u,
    );

    const groupedError =
      /grouped-unsigned-safe-integer-literal requires a required direct grouped unsigned safe integer literal at Root\.value/u;
    const groupedCases = [
      "readonly value?: 2_000;",
      "readonly value: 2000;",
      "readonly value: 20_00;",
      "readonly value: 2_00_0;",
      "readonly value: 2_000.0;",
      "readonly value: -2_000;",
      "readonly value: 2_000 | null;",
      "readonly value: typeof VALUE;",
      "readonly value: 9_007_199_254_740_992;",
    ];
    // reject every indirect, malformed, optional, signed, decimal, or unsafe group
    for (const field of groupedCases) {
      // declare the selected value only for the indirect type-query case
      const prefix = field.includes("typeof VALUE")
        ? "const VALUE = 2_000 as const;\n"
        : "";
      assert.throws(
        () => generateRustContracts(
          fixtureConfig("Root", {
            value: "grouped-unsigned-safe-integer-literal",
          }),
          { "fixture.ts": `${prefix}export interface Root { ${field} }\n` },
        ),
        groupedError,
        field,
      );
    }

    const decimalError =
      /unsigned-decimal-enum requires a required direct canonical unsigned decimal union with a fractional member at Root\.value/u;
    const decimalCases = [
      "readonly value?: 0.1 | 1 | 2.5;",
      "readonly value: 0.1;",
      "readonly value: 0 | 1 | 2;",
      "readonly value: -0.1 | 1 | 2.5;",
      "readonly value: 0.1 | 1 | 9007199254740992;",
      "readonly value: 0.1 | 1.0 | 2.5;",
      "readonly value: 1e-1 | 1 | 2.5;",
      "readonly value: 0.1 | 0.10 | 2.5;",
      "readonly value: 0.1 | 1_000 | 2.5;",
      "readonly value: 0.1 | 1 | null;",
      "readonly value: DecimalValues;",
    ];
    // reject widened, indirect, signed, unsafe, duplicate, or noncanonical decimals
    for (const field of decimalCases) {
      // declare the selected alias only for the indirect union case
      const prefix = field.includes("DecimalValues")
        ? "type DecimalValues = 0.1 | 1 | 2.5;\n"
        : "";
      assert.throws(
        () => generateRustContracts(
          fixtureConfig("Root", { value: "unsigned-decimal-enum" }),
          { "fixture.ts": `${prefix}export interface Root { ${field} }\n` },
        ),
        decimalError,
        field,
      );
    }
  },
);

test(
  "projects the original installed maintenance receipts and capture witness",
  // retain both receipt revisions, their closed union, and capture authority data
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const v1Fields = [
      ["actionSha256", "String"],
      ["bundleSha256", "String"],
      ["candidateGraphSha256", "String"],
      ["candidateSha256", "String"],
      [
        "contractVersion",
        "InstalledMaintenanceShadowReceiptV1ContractVersionLiteral",
      ],
      ["controlPlaneSha256", "String"],
      ["deployedCommit", "String"],
      ["deployedImageDigest", "String"],
      ["deployedRelease", "String"],
      ["deployedSettingsSha256", "String"],
      ["fencingToken", "String"],
      ["installedAt", "String"],
      ["registrationSha256", "String"],
      ["sourceSha256", "String"],
    ];

    assert.deepEqual(
      rustStructFieldEntries(rust, "InstalledMaintenanceShadowReceiptV1"),
      v1Fields,
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "InstalledMaintenanceShadowReceiptV2"),
      [
        // retain inherited V1 fields except its replaced version marker
        ...v1Fields.filter(([field]) => field !== "contractVersion"),
        ["actionKind", "InstalledMaintenanceShadowReceiptV2ActionKindEnum"],
        [
          "contractVersion",
          "InstalledMaintenanceShadowReceiptV2ContractVersionLiteral",
        ],
        ["fullMemberRootSha256", "Nullable<String>"],
        ["lifecycleHeadSha256", "String"],
        [
          "policyDecision",
          "InstalledMaintenanceShadowReceiptV2PolicyDecisionEnum",
        ],
        ["policyReportSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "InstalledMaintenanceShadowReceiptV2ActionKindEnum",
      ),
      [
        ["shadow", "Shadow"],
        ["promote", "Promote"],
        ["rollback_prior", "RollbackPrior"],
        ["raw", "Raw"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "InstalledMaintenanceShadowReceiptV2PolicyDecisionEnum",
      ),
      [
        ["pending", "Pending"],
        ["qualified", "Qualified"],
        ["regressed", "Regressed"],
      ],
    );
    assert.match(
      rust,
      /impl InstalledMaintenanceShadowReceiptV1ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "adjustment-installed-candidate-receipt\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl InstalledMaintenanceShadowReceiptV2ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "adjustment-installed-candidate-receipt\/v2";\s*\}/u,
    );
    assert.match(
      rust,
      /pub enum InstalledMaintenanceShadowReceipt \{\s*V1\(InstalledMaintenanceShadowReceiptV1\),\s*V2\(InstalledMaintenanceShadowReceiptV2\),\s*\}/u,
    );

    assert.deepEqual(
      rustStructFieldEntries(rust, "InstalledRainMaintenanceControlReference"),
      [
        ["controlStateBytes", "Vec<u8>"],
        ["controlStateSha256", "String"],
        ["ordinalArtifactBytes", "Vec<u8>"],
        ["ordinalArtifactSha256", "String"],
      ],
    );
    // preserve direct owned bytes at both independent control references
    for (const field of ["controlStateBytes", "ordinalArtifactBytes"]) {
      assert.match(
        rustStructFieldBlock(
          rust,
          "InstalledRainMaintenanceControlReference",
          field,
        ),
        /storage rule: uint8array-bytes, owned Uint8Array bytes; not a JSON wire type/u,
      );
    }

    assert.deepEqual(
      rustStructFieldEntries(rust, "AdjustmentRevisionCaptureEpochWitness"),
      [
        [
          "activationKind",
          "AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral",
        ],
        [
          "archiveCommitOrdinal",
          "AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral",
        ],
        ["catalogFrontierSha256", "String"],
        [
          "contractVersion",
          "AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral",
        ],
        ["controlPlaneSha256", "String"],
        [
          "controlPlaneVersion",
          "AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum",
        ],
        ["databaseMigrationHistorySha256", "String"],
        ["epochAt", "String"],
        ["servingSnapshotSha256", "String"],
        ["sourceCommit", "String"],
        ["sourceRelease", "String"],
        ["sourceServerImageDigest", "String"],
        ["sourceWebImageDigest", "String"],
        ["witnessSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum",
      ),
      [["14", "Value14"], ["17", "Value17"]],
    );
    assert.match(
      rust,
      /impl AdjustmentRevisionCaptureEpochWitnessActivationKindLiteral \{\s*pub const WIRE_VALUE: &'static str = "inert_v14_pre_activation";\s*\}/u,
    );
    assert.match(
      rust,
      /impl AdjustmentRevisionCaptureEpochWitnessArchiveCommitOrdinalLiteral \{\s*pub const WIRE_VALUE: &'static str = "0";\s*\}/u,
    );
    assert.match(
      rust,
      /impl AdjustmentRevisionCaptureEpochWitnessContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "adjustment-revision-capture-epoch-witness\/v1";\s*\}/u,
    );
    assert.match(
      rustStructFieldBlock(
        rust,
        "AdjustmentRevisionCaptureEpochWitness",
        "contractVersion",
      ),
      /wire rule: const-string, exact local const string value/u,
    );
    const chronologyFields = [
      ["InstalledMaintenanceShadowReceiptV1", "installedAt"],
      ["InstalledMaintenanceShadowReceiptV2", "installedAt"],
      ["AdjustmentRevisionCaptureEpochWitness", "epochAt"],
    ];
    // keep stored timestamp strings free of inferred UTC normalization
    for (const [name, field] of chronologyFields) {
      assert.doesNotMatch(
        rustStructFieldBlock(rust, name, field),
        /wire rule: utc-iso/u,
      );
    }
    assertManifestRootGroup(manifest.roots, INSTALLED_MAINTENANCE_RECEIPT_ROOTS);
  },
);

test(
  "requires exact direct field order after omission and replacement",
  // bind interleaved replacements and additions to the complete source order
  () => {
    const baseRoot = {
      source: "fixture.ts",
      exportName: "Base",
      fields: {},
    };
    const root = {
      source: "fixture.ts",
      exportName: "Root",
      base: "Base",
      omitBaseFields: ["replaced"],
      ownFieldOrderAfterOmit: ["addedBefore", "replaced", "addedAfter"],
      fields: {},
    };
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [baseRoot, root],
    };
    const source = `export interface Base {
  readonly retained: string;
  readonly replaced: string;
}
export interface Root extends Omit<Base, "replaced"> {
  readonly addedBefore: string;
  readonly replaced: string;
  readonly addedAfter: string;
}
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["retained", "String"],
      ["addedBefore", "String"],
      ["replaced", "String"],
      ["addedAfter", "String"],
    ]);
    assert.deepEqual(manifest.roots[1], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-interface",
      base: "Base",
      omitBaseFields: ["replaced"],
      ownFieldOrderAfterOmit: ["addedBefore", "replaced", "addedAfter"],
    });

    const metadataError =
      /config\.roots\[1\]\.ownFieldOrderAfterOmit requires omitBaseFields, all replacements, additions, and no addedAfterOmit/u;
    const invalidCases = [
      {
        name: "missing order opt-in",
        root: {
          source: root.source,
          exportName: root.exportName,
          base: root.base,
          omitBaseFields: root.omitBaseFields,
          fields: root.fields,
        },
        source,
        error: /configured omitted interface overrides unsupported at root Root/u,
      },
      {
        name: "stale configured field",
        root: {
          ...root,
          ownFieldOrderAfterOmit: ["addedBefore", "replaced", "missing"],
        },
        source,
        error: /configured omitted interface overrides unsupported at root Root/u,
      },
      {
        name: "reordered configured fields",
        root: {
          ...root,
          ownFieldOrderAfterOmit: ["replaced", "addedBefore", "addedAfter"],
        },
        source,
        error: /configured omitted interface overrides unsupported at root Root/u,
      },
      {
        name: "reordered source fields",
        root,
        source: source.replace(
          "  readonly addedBefore: string;\n  readonly replaced: string;",
          "  readonly replaced: string;\n  readonly addedBefore: string;",
        ),
        error: /configured omitted interface overrides unsupported at root Root/u,
      },
      {
        name: "missing replacement metadata",
        root: {
          ...root,
          ownFieldOrderAfterOmit: ["addedBefore", "addedAfter"],
        },
        source,
        error: metadataError,
      },
      {
        name: "no added field metadata",
        root: { ...root, ownFieldOrderAfterOmit: ["replaced"] },
        source,
        error: metadataError,
      },
      {
        name: "added-after metadata collision",
        root: { ...root, addedAfterOmit: ["addedAfter"] },
        source,
        error: metadataError,
      },
      {
        name: "missing omission metadata",
        root: {
          ...root,
          omitBaseFields: undefined,
        },
        source,
        error: metadataError,
      },
      {
        name: "duplicate ordered field",
        root: {
          ...root,
          ownFieldOrderAfterOmit: ["addedBefore", "replaced", "replaced"],
        },
        source,
        error:
          /config\.roots\[1\]\.ownFieldOrderAfterOmit has duplicate field replaced/u,
      },
    ];
    // reject incomplete, stale, reordered, overlapping, or duplicate field proofs
    for (const fixture of invalidCases) {
      assert.throws(
        () => generateRustContracts(
          { ...config, roots: [baseRoot, fixture.root] },
          { "fixture.ts": fixture.source },
        ),
        fixture.error,
        fixture.name,
      );
    }
  },
);

test(
  "projects the original forecast scorecard preparation counters",
  // retain prepared scorecard rows and every fixed counter reason in source order
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ForecastAdjustmentExclusionReason"),
      [
        ["invalid_target", "InvalidTarget"],
        ["issued_after_valid", "IssuedAfterValid"],
        ["missing_target", "MissingTarget"],
        ["receipt_chronology_missing", "ReceiptChronologyMissing"],
        ["source_late", "SourceLate"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ForecastAdjustmentDiagnosticReason"),
      [["provenance_incomplete", "ProvenanceIncomplete"]],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentPreparedPairs"),
      [
        ["coalescedDuplicateCount", "u64"],
        ["diagnostics", "ForecastAdjustmentPreparedPairsDiagnostics"],
        ["exclusions", "ForecastAdjustmentPreparedPairsExclusions"],
        ["fallbackCount", "u64"],
        ["rows", "Vec<ForecastAdjustmentPerformancePair>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentPreparedPairsDiagnostics",
      ),
      [["provenance_incomplete", "u64"]],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentPreparedPairsExclusions",
      ),
      [
        ["invalid_target", "u64"],
        ["issued_after_valid", "u64"],
        ["missing_target", "u64"],
        ["receipt_chronology_missing", "u64"],
        ["source_late", "u64"],
      ],
    );
    // preserve fixed required counter storage on both closed reason catalogs
    for (const field of ["diagnostics", "exclusions"]) {
      assert.match(
        rustStructFieldBlock(rust, "ForecastAdjustmentPreparedPairs", field),
        /wire rule: readonly-unsigned-counter-record, all named keys required with safe unsigned integer counts/u,
      );
    }
    // retain primitive scorecard counts as safe unsigned integers
    for (const field of ["coalescedDuplicateCount", "fallbackCount"]) {
      assert.match(
        rustStructFieldBlock(rust, "ForecastAdjustmentPreparedPairs", field),
        /wire rule: safe-unsigned-integer/u,
      );
    }
    assertManifestRootGroup(manifest.roots, FORECAST_PREPARED_COUNTER_ROOTS);
  },
);

test(
  "requires pinned readonly records with finite named counter keys",
  // admit one local configured key catalog and direct required counter record
  () => {
    const keyRoot = {
      source: "fixture.ts",
      exportName: "Key",
      fields: {},
    };
    const root = {
      source: "fixture.ts",
      exportName: "Root",
      fields: { counters: "readonly-unsigned-counter-record" },
    };
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [keyRoot, root],
    };
    const source = `export type Key = "first" | "second";
export interface Root {
  readonly counters: Readonly<Record<Key, number>>;
}
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["counters", "RootCounters"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootCounters"), [
      ["first", "u64"],
      ["second", "u64"],
    ]);
    assert.match(
      rustStructFieldBlock(rust, "Root", "counters"),
      /wire rule: readonly-unsigned-counter-record, all named keys required with safe unsigned integer counts/u,
    );
    assert.deepEqual(manifest.roots, [
      {
        source: "fixture.ts",
        exportName: "Key",
        rustName: "Key",
        kind: "string-literal-enum",
      },
      {
        source: "fixture.ts",
        exportName: "Root",
        rustName: "Root",
        kind: "readonly-interface",
      },
    ]);

    const shapeError =
      /readonly-unsigned-counter-record requires a required Readonly<Record<ConfiguredKey, number>> at Root\.counters/u;
    const invalidShapes = [
      "readonly counters?: Readonly<Record<Key, number>>;",
      "readonly counters: Record<Key, number>;",
      "readonly counters: Readonly<Partial<Record<Key, number>>>;",
      "readonly counters: Readonly<Record<Key, number>> | null;",
      "readonly counters: Readonly<Record<Key, number | string>>;",
      "readonly counters: Readonly<Record<Key, number | null>>;",
      "readonly counters: CounterRecord;",
    ];
    // reject every optional, mutable, partial, nullable, widened, or aliased record
    for (const field of invalidShapes) {
      // declare the selected alias only for the indirect record case
      const prefix = field.includes("CounterRecord")
        ? "type CounterRecord = Readonly<Record<Key, number>>;\n"
        : "";
      assert.throws(
        () => generateRustContracts(
          config,
          {
            "fixture.ts":
              `export type Key = "first" | "second";\n${prefix}` +
              `export interface Root { ${field} }\n`,
          },
        ),
        shapeError,
        field,
      );
    }

    const utilityCases = [
      "type Readonly<T> = T;",
      "type Record<K, V> = { readonly value: V };",
    ];
    // reject source-owned substitutes for either pinned TypeScript utility
    for (const utility of utilityCases) {
      assert.throws(
        () => generateRustContracts(
          config,
          { "fixture.ts": `${utility}\n${source}` },
        ),
        /readonly-unsigned-counter-record utility identity unsupported at Root\.counters/u,
        utility,
      );
    }

    assert.throws(
      // reject an otherwise valid local key that is not configured as a root
      () => generateRustContracts(
        { ...config, roots: [root] },
        { "fixture.ts": source },
      ),
      /readonly-unsigned-counter-record key identity unsupported at Root\.counters/u,
    );
    assert.throws(
      // reject an alias between the record and its configured key declaration
      () => generateRustContracts(
        config,
        {
          "fixture.ts": `export type Key = "first" | "second";
type KeyAlias = Key;
export interface Root {
  readonly counters: Readonly<Record<KeyAlias, number>>;
}
`,
        },
      ),
      /readonly-unsigned-counter-record key identity unsupported at Root\.counters/u,
    );
    assert.throws(
      // reject a configured key root whose wire catalog is not a string enum
      () => generateRustContracts(
        config,
        {
          "fixture.ts": `export type Key = 1 | 2;
export interface Root {
  readonly counters: Readonly<Record<Key, number>>;
}
`,
        },
      ),
      /readonly-unsigned-counter-record key root must render string-literal-enum at Root\.counters/u,
    );
    assert.throws(
      // reject a generic key reference before treating it as a configured catalog
      () => generateRustContracts(
        { ...config, roots: [root, keyRoot] },
        {
          "fixture.ts": `export type Key<T> = "first" | "second";
export interface Root {
  readonly counters: Readonly<Record<Key<string>, number>>;
}
`,
        },
      ),
      /readonly-unsigned-counter-record key identity unsupported at Root\.counters/u,
    );
    assert.throws(
      // reject a configured imported key even when its relative edge is explicit
      () => generateRustContracts(
        {
          ...config,
          roots: [
            { ...keyRoot, source: "key.ts" },
            root,
          ],
          typeImports: [{
            consumer: "fixture.ts",
            specifier: "./key.js",
            entry: "key.ts",
            exports: { Key: "key.ts" },
          }],
        },
        {
          "key.ts": "export type Key = \"first\" | \"second\";\n",
          "fixture.ts": `import type { Key } from "./key.js";
export interface Root {
  readonly counters: Readonly<Record<Key, number>>;
}
`,
        },
      ),
      /readonly-unsigned-counter-record key identity unsupported at Root\.counters/u,
    );
    assert.throws(
      // preserve the existing property-name rejection for nonportable wire keys
      () => generateRustContracts(
        config,
        {
          "fixture.ts": `export type Key = "first" | "not-portable";
export interface Root {
  readonly counters: Readonly<Record<Key, number>>;
}
`,
        },
      ),
      /unsupported property name "not-portable" at Root\.counters\.not-portable/u,
    );
  },
);

test(
  "admits only exact required nullable metric record aliases",
  // keep the root opt-in on one exact configured enum and pinned utility shape
  () => {
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Key", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {},
          readonlyNullableMetricRecord: "Key",
        },
      ],
    };
    const source = `export type Key = "first" | "second";
export type Root = Readonly<Record<Key, number | null>>;
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["first", "Nullable<f64>"],
      ["second", "Nullable<f64>"],
    ]);
    assert.match(
      rust,
      /#\[doc = "storage rule: readonly-nullable-metric-record, all catalog keys required; binary64 values or explicit null"\][\s\S]*pub struct Root/u,
    );
    assert.deepEqual(manifest.roots[1], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-nullable-metric-record",
      readonlyNullableMetricRecord: "Key",
    });

    const sourceCases = [
      {
        name: "mutable record",
        source: `export type Key = "first" | "second";
export type Root = Record<Key, number | null>;
`,
        error: /readonly-nullable-metric-record requires/u,
      },
      {
        name: "partial record",
        source: `export type Key = "first" | "second";
export type Root = Readonly<Partial<Record<Key, number | null>>>;
`,
        error: /readonly-nullable-metric-record/u,
      },
      {
        name: "optional object fields",
        source: `export type Key = "first" | "second";
export type Root = { readonly first?: number | null; readonly second: number | null };
`,
        error: /readonly-nullable-metric-record/u,
      },
      {
        name: "widened record value",
        source: `export type Key = "first" | "second";
export type Root = Readonly<Record<Key, number | string | null>>;
`,
        error: /readonly-nullable-metric-record requires/u,
      },
      {
        name: "aliased record value",
        source: `export type Key = "first" | "second";
type Value = number | null;
export type Root = Readonly<Record<Key, Value>>;
`,
        error: /readonly-nullable-metric-record requires/u,
      },
      {
        name: "aliased key root",
        source: `export type Key = "first" | "second";
type KeyAlias = Key;
export type Root = Readonly<Record<KeyAlias, number | null>>;
`,
        error: /readonly-nullable-metric-record key identity unsupported/u,
      },
      {
        name: "shadowed readonly utility",
        source: `type Readonly<T> = T;
export type Key = "first" | "second";
export type Root = Readonly<Record<Key, number | null>>;
`,
        error: /readonly-nullable-metric-record utility identity unsupported/u,
      },
      {
        name: "shadowed record utility",
        source: `type Record<K, V> = { readonly value: V };
export type Key = "first" | "second";
export type Root = Readonly<Record<Key, number | null>>;
`,
        error: /readonly-nullable-metric-record utility identity unsupported/u,
      },
    ];
    // reject every widened, optional, indirect, or shadowed source spelling
    for (const fixture of sourceCases) {
      assert.throws(
        () => generateRustContracts(
          config,
          { "fixture.ts": fixture.source },
        ),
        fixture.error,
        fixture.name,
      );
    }

    assert.throws(
      () => generateRustContracts(
        {
          ...config,
          roots: [config.roots[1]],
        },
        { "fixture.ts": source },
      ),
      /readonly-nullable-metric-record key identity unsupported/u,
    );
    assert.throws(
      () => generateRustContracts(
        {
          ...config,
          roots: [
            config.roots[0],
            { source: "fixture.ts", exportName: "Other", fields: {} },
            config.roots[1],
          ],
        },
        {
          "fixture.ts": `export type Key = "first" | "second";
export type Other = "third" | "fourth";
export type Root = Readonly<Record<Other, number | null>>;
`,
        },
      ),
      /readonly-nullable-metric-record key root/u,
    );
    assert.throws(
      () => generateRustContracts(
        config,
        {
          "fixture.ts": `export type Key = "not-portable" | "fine";
export type Root = Readonly<Record<Key, number | null>>;
`,
        },
      ),
      /readonly-nullable-metric-record key is not a portable identifier/u,
    );
    assert.throws(
      () => generateRustContracts(
        {
          ...config,
          roots: [
            config.roots[0],
            {
              source: "fixture.ts",
              exportName: "Root",
              fields: {},
            },
          ],
        },
        { "fixture.ts": source },
      ),
      /configured type reference unsupported at root Root/u,
    );
    assert.throws(
      () => generateRustContracts(
        {
          ...config,
          roots: [
            config.roots[0],
            {
              ...config.roots[1],
              fields: { value: "runtime-f64" },
            },
          ],
        },
        { "fixture.ts": source },
      ),
      /readonlyNullableMetricRecord requires a distinct configured key root and empty fields/u,
    );
  },
);

test(
  "admits only a required direct pinned Date for exact date storage",
  // retain the Date arm and no string or null alternative
  () => {
    const config = fixtureConfig("Root", { validAt: "js-date" });
    const { rust } = generateRustContracts(config, {
      "fixture.ts": "export interface Root { readonly validAt: Date; }",
    });
    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [["validAt", "JsDate"]]);
    assert.doesNotMatch(rust, /pub enum JsDateOrString/u);

    // reject optional, aliased, widened, collection, and shadowed date types
    for (const [prefix, field, message] of [
      ["", "validAt?: Date", /js-date requires a required direct Date/u],
      ["", "validAt: Date | null", /js-date requires a required direct Date/u],
      ["", "validAt: Date | string", /js-date requires a required direct Date/u],
      ["", "validAt: Date<string>", /js-date requires a required direct Date/u],
      ["", "validAt: readonly Date[]", /js-date requires a required direct Date/u],
      ["type Clock = Date;", "validAt: Clock", /js-date requires a required direct Date/u],
      ["interface Date { readonly fake: string; }", "validAt: Date", /js-date builtin identity unsupported/u],
      ["declare global { interface Date { readonly fake: string; } }", "validAt: Date", /js-date builtin identity unsupported/u],
    ]) {
      const source = prefix + "\nexport interface Root { readonly " + field + "; }";
      assert.throws(() => generateRustContracts(config, { "fixture.ts": source }), message);
    }
  },
);

test(
  "admits only exact readonly nullable number series and sensor records",
  // preserve element nulls and outer null independently for both storage forms
  () => {
    const config = fixtureConfig("Root", {
      requiredSensors: "ordered-runtime-f64-nullable-array-record",
      requiredValues: "runtime-f64-nullable-array",
      sensors: "ordered-runtime-f64-nullable-array-record",
      values: "runtime-f64-nullable-array",
    });
    const { rust } = generateRustContracts(config, {
      "fixture.ts": `export interface Root {
  readonly requiredSensors: Readonly<Record<string, readonly (null | number)[]>>;
  readonly requiredValues: readonly (number | null)[];
  readonly sensors: Readonly<Record<string, readonly (number | null)[]>> | null;
  readonly values: readonly (null | number)[] | null;
}
`,
    });

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["requiredSensors", "Vec<(Vec<u16>, Vec<Nullable<f64>>)>"],
      ["requiredValues", "Vec<Nullable<f64>>"],
      ["sensors", "Nullable<Vec<(Vec<u16>, Vec<Nullable<f64>>)>>"],
      ["values", "Nullable<Vec<Nullable<f64>>>"],
    ]);
    assert.match(
      rustStructFieldBlock(rust, "Root", "sensors"),
      /storage rule: ordered-runtime-f64-nullable-array-record/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "Root", "values"),
      /storage rule: runtime-f64-nullable-array/u,
    );

    const arrayCases = [
      ["values?: readonly (number | null)[]", /runtime-f64-nullable-array requires/u],
      ["values: (number | null)[]", /runtime-f64-nullable-array requires/u],
      ["values: readonly (number | null | string)[]", /runtime-f64-nullable-array requires/u],
      ["values: readonly null[]", /runtime-f64-nullable-array requires/u],
      ["values: readonly number[]", /runtime-f64-nullable-array requires/u],
      ["values: readonly Sample[]", /runtime-f64-nullable-array requires/u],
      ["values: readonly (number | null)[] | undefined", /runtime-f64-nullable-array requires/u],
    ];
    // reject every widened or indirect array grammar
    for (const [field, error] of arrayCases) {
      const prefix = field.includes("Sample") ? "type Sample = number | null;\n" : "";
      assert.throws(
        () => generateRustContracts(
          fixtureConfig("Root", { values: "runtime-f64-nullable-array" }),
          { "fixture.ts": `${prefix}export interface Root { readonly ${field}; }\n` },
        ),
        error,
      );
    }

    const recordCases = [
      [
        "Record<string, readonly (number | null)[]>",
        /ordered-runtime-f64-nullable-array-record requires/u,
      ],
      [
        "Readonly<Record<string, readonly (number | null)[]>>",
        /ordered-runtime-f64-nullable-array-record requires/u,
        true,
      ],
      [
        "Readonly<Record<number, readonly (number | null)[]>>",
        /ordered-runtime-f64-nullable-array-record requires/u,
      ],
      [
        "Readonly<Record<string, (number | null)[]>>",
        /ordered-runtime-f64-nullable-array-record requires/u,
      ],
      [
        "Readonly<Record<string, readonly number[]>>",
        /ordered-runtime-f64-nullable-array-record requires/u,
      ],
      [
        "Readonly<Record<string, readonly (number | null | boolean)[]>>",
        /ordered-runtime-f64-nullable-array-record requires/u,
      ],
      [
        "Readonly<Record<string, readonly Sample[]>>",
        /ordered-runtime-f64-nullable-array-record requires/u,
      ],
    ];
    // reject alternate keys, values, mutability, and aliases inside the record
    for (const [type, error, optional = false] of recordCases) {
      const prefix = type.includes("Sample") ? "type Sample = number | null;\n" : "";
      const fieldName = optional ? "sensors?" : "sensors";
      assert.throws(
        () => generateRustContracts(
          fixtureConfig("Root", {
            sensors: "ordered-runtime-f64-nullable-array-record",
          }),
          {
            "fixture.ts":
              `${prefix}export interface Root { readonly ${fieldName}: ${type}; }\n`,
          },
        ),
        error,
      );
    }

    // reject local utility aliases before their spelling can be treated as proof
    for (const fixtureSource of [
      `type Readonly<T> = T;
export interface Root {
  readonly sensors: Readonly<Record<string, readonly (number | null)[]>>;
}
`,
      `type Record<K, V> = { readonly value: V };
export interface Root {
  readonly sensors: Readonly<Record<string, readonly (number | null)[]>>;
}
`,
    ]) {
      assert.throws(
        () => generateRustContracts(
          fixtureConfig("Root", {
            sensors: "ordered-runtime-f64-nullable-array-record",
          }),
          { "fixture.ts": fixtureSource },
        ),
        /utility identity unsupported/u,
      );
    }

    assert.throws(
      // keep the existing scalar runtime mapping closed over direct numbers
      () => generateRustContracts(
        fixtureConfig("Root", { values: "runtime-f64" }),
        {
          "fixture.ts":
            "export interface Root { readonly values: readonly (number | null)[]; }\n",
        },
      ),
      /runtime-f64 requires a required direct number or number \| null field/u,
    );
  },
);

test(
  "admits only the exact pinned pg row marker and named projection",
  // reject unreviewed bases and preserve both marker entry digests
  () => {
    const config = fixtureConfig("Root");
    config.roots[0].pgRowNamedFields = true;
    const base = 'import type { QueryResultRow } from "pg";';
    const row = "export interface Root extends QueryResultRow { readonly id: string; }";
    const { rust, manifest } = generateRustContracts(config, {
      "fixture.ts": base + "\n" + row,
    });
    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [["id", "String"]]);
    assert.deepEqual(JSON.parse(manifest).generator.pgRowMarkerInputSha256, PG_ROW_MARKER_INPUT_SHA256);

    // require the exact import, base clause, and readonly member grammar
    for (const [source, error] of [
      [base + row.replace(" extends QueryResultRow", ""), /marker base unsupported/],
      [base + row.replace("QueryResultRow", "QueryResultRow<string>"), /marker base unsupported/],
      [base + row.replace("QueryResultRow", "QueryResultRow, Other"), /marker base unsupported/],
      [base.replace('"pg"', '"other"') + row, /marker import unsupported/],
      [base.replace("import type ", "import ") + row, /marker import unsupported/],
      [base.replace("{ QueryResultRow }", "{ Other as QueryResultRow }") + row, /marker import unsupported/],
      ["interface QueryResultRow { readonly fake: number; }" + row, /marker import unsupported/],
      [base + row.replace("readonly id: string;", "[key: string]: any;"), /unsupported/],
    ]) {
      assert.throws(() => generateRustContracts(config, { "fixture.ts": source }), error);
    }

    // make the closed external proof respond to changes at either entry
    for (const name of ["index.d.mts", "index.d.ts"]) {
      assert.throws(
        () => generateRustContracts(config, { "fixture.ts": base + row },
          // simulate an updated installed declaration without editing node_modules
          (path) => path.endsWith("/" + name) ? Buffer.from("changed\n") : readFileSync(path)),
        /pinned PostgreSQL row marker input mismatch/,
      );
    }
    const withoutOptIn = fixtureConfig("Root");
    assert.throws(
      () => generateRustContracts(withoutOptIn, { "fixture.ts": base + row }),
      /interface inheritance unsupported/,
    );
  },
);

test(
  "admits only the exact builtin date and raw string storage union",
  // distinguish raw text, invalid dates, and null without normalizing any input
  () => {
    const config = fixtureConfig("Root", { timestamp: "js-date-or-string" });
    const { rust } = generateRustContracts(config, {
      "fixture.ts": "export interface Root { readonly timestamp: string | null | Date; }",
    });
    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["timestamp", "Nullable<JsDateOrString>"],
    ]);
    assert.match(rustStructFieldBlock(rust, "Root", "timestamp"), /storage rule: js-date-or-string/u);

    // reject alias, widened, optional, and shadowed forms
    for (const [prefix, field, error] of [
      ["", "timestamp: string", /requires a required direct Date/],
      ["", "timestamp: Date | string | undefined", /requires a required direct Date/],
      ["", "timestamp: Date | string | null | null", /requires a required direct Date/],
      ["", "timestamp?: Date | string", /requires a required direct Date/],
      ["", "timestamp: Date<string> | string", /requires a required direct Date/],
      ["type Clock = Date;", "timestamp: Clock | string", /requires a required direct Date/],
      ["interface Date { readonly epoch: number; }", "timestamp: Date | string", /builtin identity unsupported/],
      ["declare global { interface Date { readonly epoch: number; } }", "timestamp: Date | string", /builtin identity unsupported/],
    ]) {
      const source = prefix + "\nexport interface Root { readonly " + field + "; }";
      assert.throws(() => generateRustContracts(config, { "fixture.ts": source }), error);
    }
  },
);

test(
  "admits only one explicitly defaulted readonly list type parameter",
  // admit one direct type-only default and one exact readonly parameter list
  () => {
    const defaultSource = `export interface DefaultRecord {
  readonly value: string;
}
`;
    const batchSource = `import type { DefaultRecord } from "./default.js";
export interface Batch<Item = DefaultRecord> {
  readonly records: readonly Item[];
  readonly label: string;
}
`;
    const defaultRoot = {
      source: "default.ts",
      exportName: "DefaultRecord",
      fields: {},
    };
    const batchRoot = {
      source: "batch.ts",
      exportName: "Batch",
      fields: {},
      defaultedListParameter: {
        name: "Item",
        defaultRoot: "DefaultRecord",
        field: "records",
      },
    };
    const typeImport = {
      consumer: "batch.ts",
      specifier: "./default.js",
      entry: "default.ts",
      exports: { DefaultRecord: "default.ts" },
    };
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [defaultRoot, batchRoot],
      typeImports: [typeImport],
    };
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "default.ts": defaultSource, "batch.ts": batchSource },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "Batch",
        "Batch<Item = DefaultRecord>",
      ),
      [
        ["records", "Vec<Item>"],
        ["label", "String"],
      ],
    );
    assert.deepEqual(manifest.roots[1], {
      source: "batch.ts",
      exportName: "Batch",
      rustName: "Batch",
      kind: "readonly-interface",
      defaultedListParameter: {
        name: "Item",
        defaultRoot: "DefaultRecord",
        field: "records",
      },
    });
    assert.deepEqual(manifest.typeImports, [typeImport]);

    const invalidCases = [
      {
        name: "an unconfigured generic root",
        config: {
          ...config,
          roots: [
            defaultRoot,
            { source: "batch.ts", exportName: "Batch", fields: {} },
          ],
        },
        source: batchSource,
      },
      {
        name: "a fixed Rust storage type parameter name",
        error: /config\.roots\[1\]\.defaultedListParameter\.name collides with a generated Rust type/u,
        config: {
          ...config,
          roots: [
            defaultRoot,
            {
              ...batchRoot,
              defaultedListParameter: {
                ...batchRoot.defaultedListParameter,
                name: "Vec",
              },
            },
          ],
        },
        source: `import type { DefaultRecord } from "./default.js";
export interface Batch<Vec = DefaultRecord> {
  readonly records: readonly Vec[];
}
`,
      },
      {
        name: "a configured root type parameter name",
        error: /defaulted list parameter Rust type collision Collision/u,
        config: {
          ...config,
          roots: [
            defaultRoot,
            { source: "default.ts", exportName: "Collision", fields: {} },
            {
              ...batchRoot,
              defaultedListParameter: {
                ...batchRoot.defaultedListParameter,
                name: "Collision",
              },
            },
          ],
        },
        sources: {
          "default.ts": `${defaultSource}export interface Collision {
  readonly value: string;
}
`,
          "batch.ts": `import type { DefaultRecord } from "./default.js";
export interface Batch<Collision = DefaultRecord> {
  readonly records: readonly Collision[];
}
`,
        },
      },
      {
        name: "a constrained parameter",
        source: `import type { DefaultRecord } from "./default.js";
export interface Batch<Item extends DefaultRecord = DefaultRecord> {
  readonly records: readonly Item[];
}
`,
      },
      {
        name: "a second parameter",
        source: `import type { DefaultRecord } from "./default.js";
export interface Batch<Item = DefaultRecord, Extra = string> {
  readonly records: readonly Item[];
}
`,
      },
      {
        name: "a generic type alias",
        source: `import type { DefaultRecord } from "./default.js";
export type Batch<Item = DefaultRecord> = {
  readonly records: readonly Item[];
};
`,
      },
      {
        name: "a value-imported default",
        source: `import { DefaultRecord } from "./default.js";
export interface Batch<Item = DefaultRecord> {
  readonly records: readonly Item[];
}
`,
      },
      {
        name: "a renamed default import",
        config: {
          ...config,
          roots: [
            defaultRoot,
            {
              ...batchRoot,
              defaultedListParameter: {
                ...batchRoot.defaultedListParameter,
                defaultRoot: "ImportedRecord",
              },
            },
          ],
        },
        source: `import type { DefaultRecord as ImportedRecord } from "./default.js";
export interface Batch<Item = ImportedRecord> {
  readonly records: readonly Item[];
}
`,
      },
      {
        name: "an indirect default alias",
        source: `import type { DefaultRecord } from "./default.js";
type Alias = DefaultRecord;
export interface Batch<Item = Alias> {
  readonly records: readonly Item[];
}
`,
      },
      {
        name: "a same-source forged default",
        config: {
          version: 1,
          typescriptVersion: "6.0.3",
          roots: [
            { source: "batch.ts", exportName: "DefaultRecord", fields: {} },
            batchRoot,
          ],
        },
        sources: {
          "batch.ts": `export interface DefaultRecord {
  readonly value: string;
}
export interface Batch<Item = DefaultRecord> {
  readonly records: readonly Item[];
}
`,
        },
      },
      {
        name: "a different selected field",
        config: {
          ...config,
          roots: [
            defaultRoot,
            {
              ...batchRoot,
              defaultedListParameter: {
                ...batchRoot.defaultedListParameter,
                field: "items",
              },
            },
          ],
        },
        source: batchSource,
      },
      {
        name: "a mutable list",
        source: `import type { DefaultRecord } from "./default.js";
export interface Batch<Item = DefaultRecord> {
  readonly records: Item[];
}
`,
      },
      {
        name: "a mutable field",
        source: `import type { DefaultRecord } from "./default.js";
export interface Batch<Item = DefaultRecord> {
  records: readonly Item[];
}
`,
      },
      {
        name: "an optional field",
        source: `import type { DefaultRecord } from "./default.js";
export interface Batch<Item = DefaultRecord> {
  readonly records?: readonly Item[];
}
`,
      },
      {
        name: "a ReadonlyArray spelling",
        source: `import type { DefaultRecord } from "./default.js";
export interface Batch<Item = DefaultRecord> {
  readonly records: ReadonlyArray<Item>;
}
`,
      },
      {
        name: "a widened list element",
        source: `import type { DefaultRecord } from "./default.js";
export interface Batch<Item = DefaultRecord> {
  readonly records: readonly (Item | string)[];
}
`,
      },
      {
        name: "a raw parameter field",
        source: `import type { DefaultRecord } from "./default.js";
export interface Batch<Item = DefaultRecord> {
  readonly records: readonly Item[];
  readonly last: Item;
}
`,
      },
      {
        name: "a second parameter list",
        source: `import type { DefaultRecord } from "./default.js";
export interface Batch<Item = DefaultRecord> {
  readonly records: readonly Item[];
  readonly extras: readonly Item[];
}
`,
      },
      {
        name: "a reference to the generic root",
        config: {
          ...config,
          roots: [
            ...config.roots,
            { source: "batch.ts", exportName: "UsesBatch", fields: {} },
          ],
        },
        source: `${batchSource}export interface UsesBatch {
  readonly batch: Batch;
}
`,
      },
      {
        name: "an imported reference to the generic root",
        error: /generic configured type import unsupported at UsesBatch\.batch/u,
        config: {
          ...config,
          roots: [
            ...config.roots,
            { source: "consumer.ts", exportName: "UsesBatch", fields: {} },
          ],
          typeImports: [
            typeImport,
            {
              consumer: "consumer.ts",
              specifier: "./batch.js",
              entry: "batch.ts",
              exports: { Batch: "batch.ts" },
            },
          ],
        },
        sources: {
          "default.ts": defaultSource,
          "batch.ts": batchSource,
          "consumer.ts": `import type { Batch } from "./batch.js";
export interface UsesBatch {
  readonly batch: Batch;
}
`,
        },
      },
    ];
    // reject every unproved, widened, repeated, or indirectly referenced parameter shape
    for (const invalid of invalidCases) {
      assert.throws(
        // retain the valid foreign default while varying one generic boundary
        () => generateRustContracts(
          invalid.config ?? config,
          invalid.sources ?? {
            "default.ts": defaultSource,
            "batch.ts": invalid.source,
          },
        ),
        invalid.error ?? /(?:generic type unsupported|defaulted list parameter (?:root|declaration|field|identity|default root) unsupported|configured type (?:import|reference) identity unsupported|generic configured type (?:reference|import) unsupported)/u,
        invalid.name,
      );
    }
  },
);

test(
  "admits only exact configured Pick roots and rejects grammar drift",
  // pin utility, base, field-order, and metadata identities without widening aliases
  () => {
    const source = `export interface Base {
  readonly first: string;
  readonly count: number;
  readonly kind: "first" | "second";
  readonly last: string;
}
export type Root = Pick<Base, "count" | "kind">;
export type Single = Pick<Base, "last">;
`;
    const config = pickFixtureConfig();
    config.roots.push({
      source: "fixture.ts",
      exportName: "Single",
      fields: {},
      pick: { root: "Base", fields: ["last"] },
    });
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["count", "u64"],
      ["kind", "BaseKindEnum"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "Single"), [
      ["last", "String"],
    ]);
    assert.equal(
      rustStructFieldBlock(rust, "Root", "count"),
      rustStructFieldBlock(rust, "Base", "count"),
    );
    assert.equal(
      rustStructFieldBlock(rust, "Root", "kind"),
      rustStructFieldBlock(rust, "Base", "kind"),
    );
    assert.equal(
      rustStructFieldBlock(rust, "Single", "last"),
      rustStructFieldBlock(rust, "Base", "last"),
    );
    assert.equal(rust.match(/^pub enum BaseKindEnum \{/gmu)?.length, 1);
    assert.deepEqual(manifest.roots, [
      {
        source: "fixture.ts",
        exportName: "Base",
        rustName: "Base",
        kind: "readonly-interface",
      },
      {
        source: "fixture.ts",
        exportName: "Root",
        rustName: "Root",
        kind: "picked-readonly-interface",
        pick: { root: "Base", fields: ["count", "kind"] },
      },
      {
        source: "fixture.ts",
        exportName: "Single",
        rustName: "Single",
        kind: "picked-readonly-interface",
        pick: { root: "Base", fields: ["last"] },
      },
    ]);

    const baseSource = `export interface Base {
  readonly count: number;
  readonly kind: "first" | "second";
}
`;
    const mismatchedFields = [
      {
        name: "stale configured fields",
        source: `${baseSource}export type Root = Pick<Base, "count" | "kind">;\n`,
        config: pickFixtureConfig(["count", "kind", "missing"]),
      },
      {
        name: "reordered source fields",
        source: `${baseSource}export type Root = Pick<Base, "kind" | "count">;\n`,
        config: pickFixtureConfig(),
      },
      {
        name: "a nonliteral source key type",
        source: `${baseSource}export type Root = Pick<Base, keyof Base>;\n`,
        config: pickFixtureConfig(),
      },
      {
        name: "a shortened source key list",
        source: `${baseSource}export type Root = Pick<Base, "count">;\n`,
        config: pickFixtureConfig(),
      },
      {
        name: "a duplicate source key",
        source: `${baseSource}export type Root = Pick<Base, "count" | "count">;\n`,
        config: pickFixtureConfig(),
      },
      {
        name: "an aliased source key list",
        source: `${baseSource}type Keys = "count" | "kind";\nexport type Root = Pick<Base, Keys>;\n`,
        config: pickFixtureConfig(),
      },
    ];
    // reject source keys that no longer exactly match the configured sequence
    for (const scenario of mismatchedFields) {
      assert.throws(
        // exercise one mismatched source field grammar
        () => generateRustContracts(
          scenario.config,
          { "fixture.ts": scenario.source },
        ),
        /pick fields mismatch at root Root/u,
        scenario.name,
      );
    }

    const requiredShapeCases = [
      `${baseSource}export interface Root { readonly count: number; }\n`,
      `${baseSource}type Select<T, K extends keyof T> = Pick<T, K>;\nexport type Root = Select<Base, "count" | "kind">;\n`,
      `${baseSource}export type Root = Pick<Base>;\n`,
    ];
    // reject root declarations and aliases outside the exact utility shape
    for (const invalidSource of requiredShapeCases) {
      assert.throws(
        // exercise one non-Pick root declaration
        () => generateRustContracts(
          pickFixtureConfig(),
          { "fixture.ts": invalidSource },
        ),
        /pick requires Pick<ConfiguredInterface, "field" \| \.\.\.> at root Root/u,
      );
    }

    assert.throws(
      // reject a local utility with the same spelling as the pinned builtin
      () => generateRustContracts(
        pickFixtureConfig(),
        {
          "fixture.ts": `type Pick<T, K extends keyof T> = { readonly [P in K]: T[P] };
${baseSource}export type Root = Pick<Base, "count" | "kind">;
`,
        },
      ),
      /pick utility identity unsupported at root Root/u,
    );

    const wrongBaseCases = [
      {
        name: "a different source base",
        source: `${baseSource}export interface Other { readonly count: number; readonly kind: "first" | "second"; }\nexport type Root = Pick<Other, "count" | "kind">;\n`,
        config: pickFixtureConfig(),
      },
      {
        name: "an unconfigured source base",
        source: `${baseSource}export type Root = Pick<Base, "count" | "kind">;\n`,
        config: {
          version: 1,
          typescriptVersion: "6.0.3",
          roots: [{
            source: "fixture.ts",
            exportName: "Root",
            fields: {},
            pick: { root: "Base", fields: ["count", "kind"] },
          }],
        },
      },
      {
        name: "an inherited source base",
        source: `export interface Parent { readonly inherited: string; }
export interface Base extends Parent {
  readonly count: number;
  readonly kind: "first" | "second";
}
export type Root = Pick<Base, "count" | "kind">;
`,
        config: {
          version: 1,
          typescriptVersion: "6.0.3",
          roots: [
            {
              source: "fixture.ts",
              exportName: "Root",
              fields: {},
              pick: { root: "Base", fields: ["count", "kind"] },
            },
            { source: "fixture.ts", exportName: "Parent", fields: {} },
            {
              source: "fixture.ts",
              exportName: "Base",
              base: "Parent",
              fields: { count: "safe-unsigned-integer" },
            },
          ],
        },
      },
    ];
    // reject every base that is not the configured direct public interface
    for (const scenario of wrongBaseCases) {
      assert.throws(
        // exercise one unsupported base identity
        () => generateRustContracts(
          scenario.config,
          { "fixture.ts": scenario.source },
        ),
        /pick base identity unsupported at root Root/u,
        scenario.name,
      );
    }

    assert.throws(
      // reject a configured and source-selected field absent from the base
      () => generateRustContracts(
        pickFixtureConfig(["missing"]),
        {
          "fixture.ts": `${baseSource}export type Root = Pick<Base, "missing">;\n`,
        },
      ),
      /pick field unsupported at Root\.missing/u,
    );

    const combinedMetadata = [
      { fields: { count: "safe-unsigned-integer" } },
      { fields: {}, base: "Base" },
      { fields: {}, localAlias: true },
    ];
    // reject every attempt to override or combine the selected base fields
    for (const metadata of combinedMetadata) {
      const ordinaryConfig = pickFixtureConfig();
      const invalidConfig = {
        ...ordinaryConfig,
        roots: [
          { ...ordinaryConfig.roots[1], ...metadata },
          ordinaryConfig.roots[0],
        ],
      };
      assert.throws(
        // reject config metadata before inspecting source syntax
        () => generateRustContracts(invalidConfig, { "fixture.ts": source }),
        /pick metadata cannot be combined with other root metadata at config\.roots\[0\]/u,
      );
    }
  },
);

test(
  "projects forecast training, observation, and network event contracts",
  // preserve cohort provenance and observation identities across domain edges
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStringEnumWireEntries(rust, "ForecastTrainingCohort"), [
      ["fixed_lead_anchor", "FixedLeadAnchor"],
      ["legacy_v4_retrieval_snapshot", "LegacyV4RetrievalSnapshot"],
    ]);
    assert.deepEqual(rustStringEnumWireEntries(rust, "ForecastReferenceKind"), [
      ["fixed_lead_anchor", "FixedLeadAnchor"],
      ["retrieval_snapshot", "RetrievalSnapshot"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "FixedLeadAnchorTrainingRow"),
      [
        ["adapterVersion", "String"],
        ["cohort", "FixedLeadAnchorTrainingRowCohortLiteral"],
        ["contractEpoch", "String"],
        [
          "contractVersion",
          "FixedLeadAnchorTrainingRowContractVersionLiteral",
        ],
        ["continuousLeadHours", "u64"],
        ["dataset", "FixedLeadAnchorTrainingRowDatasetLiteral"],
        ["metrics", "CanonicalWeatherMetrics"],
        [
          "referenceKind",
          "FixedLeadAnchorTrainingRowReferenceKindLiteral",
        ],
        ["sourceConfigFingerprint", "String"],
        ["sourceId", "String"],
        ["targetLeadHours", "u64"],
        ["upstreamModel", "FixedLeadAnchorTrainingRowUpstreamModelLiteral"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "LegacyV4RetrievalSnapshotTrainingRow"),
      [
        ["adapterVersion", "String"],
        [
          "cohort",
          "LegacyV4RetrievalSnapshotTrainingRowCohortLiteral",
        ],
        ["contractEpoch", "String"],
        [
          "contractVersion",
          "LegacyV4RetrievalSnapshotTrainingRowContractVersionLiteral",
        ],
        ["continuousLeadHours", "f64"],
        ["dataset", "String"],
        ["metrics", "CanonicalWeatherMetrics"],
        ["referenceAt", "String"],
        [
          "referenceKind",
          "LegacyV4RetrievalSnapshotTrainingRowReferenceKindLiteral",
        ],
        ["sourceConfigFingerprint", "String"],
        ["sourceId", "String"],
        ["stableRecordId", "String"],
        ["targetLeadHours", "u64"],
        ["upstreamModel", "String"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "LegacyV4RetrievalSnapshotTrainingRowInput",
      ),
      [
        ["adapterVersion", "String"],
        ["contractEpoch", "String"],
        ["dataset", "String"],
        ["metrics", "CanonicalWeatherMetrics"],
        ["referenceAt", "String"],
        ["sourceConfigFingerprint", "String"],
        ["sourceId", "String"],
        ["stableRecordId", "String"],
        ["upstreamModel", "String"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "ForecastTrainingCohorts"), [
      ["fixedLeadAnchors", "Vec<FixedLeadAnchorTrainingRow>"],
      [
        "legacyV4RetrievalSnapshots",
        "Vec<LegacyV4RetrievalSnapshotTrainingRow>",
      ],
    ]);
    const fixedTrainingDefinition = rust.match(
      /^pub struct FixedLeadAnchorTrainingRow \{\n[\s\S]+?^\}/mu,
    );
    const legacyTrainingDefinition = rust.match(
      /^pub struct LegacyV4RetrievalSnapshotTrainingRow \{\n[\s\S]+?^\}/mu,
    );
    const legacyInputDefinition = rust.match(
      /^pub struct LegacyV4RetrievalSnapshotTrainingRowInput \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(fixedTrainingDefinition);
    assert.ok(legacyTrainingDefinition);
    assert.ok(legacyInputDefinition);
    assert.match(
      fixedTrainingDefinition[0],
      /wire rule: utc-iso[^\n]*\n\s*pub validAt: String,/u,
    );
    assert.equal(
      fixedTrainingDefinition[0].match(/wire rule: utc-iso/gu)?.length,
      1,
    );
    assert.match(
      legacyTrainingDefinition[0],
      /wire rule: utc-iso[^\n]*\n\s*pub referenceAt: String,[\s\S]*wire rule: utc-iso[^\n]*\n\s*pub validAt: String,/u,
    );
    assert.equal(
      legacyTrainingDefinition[0].match(/wire rule: utc-iso/gu)?.length,
      2,
    );
    assert.doesNotMatch(legacyInputDefinition[0], /wire rule: utc-iso/u);
    assert.match(
      rust,
      /impl FixedLeadAnchorTrainingRowContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-training-row\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl LegacyV4RetrievalSnapshotTrainingRowContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-training-row\/v1";\s*\}/u,
    );
    const trainingRowDefinition = rust.match(
      /^pub enum ForecastTrainingRow \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(trainingRowDefinition);
    assert.equal(
      trainingRowDefinition[0],
      `pub enum ForecastTrainingRow {
    FixedLeadAnchor(FixedLeadAnchorTrainingRow),
    LegacyV4RetrievalSnapshot(LegacyV4RetrievalSnapshotTrainingRow),
}`,
    );

    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ForecastObservationProviderFamily"),
      [
        ["ambient", "Ambient"],
        ["ecowitt", "Ecowitt"],
        ["netatmo", "Netatmo"],
        ["tempest", "Tempest"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastObservationExcludedSourceLineage"),
      [
        [
          "acceptedIntervals",
          "ForecastObservationExcludedSourceLineageAcceptedIntervalsEmptyArray",
        ],
        ["physicalStationKey", "ForecastObservationStationKey"],
        [
          "reasonCode",
          "ForecastObservationExcludedSourceLineageReasonCodeLiteral",
        ],
        ["sourceKey", "String"],
        ["successorSourceKey", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastObservationStation"),
      [
        ["acceptedSourceKeys", "Vec<String>"],
        ["distanceMeters", "f64"],
        ["eligibleMetrics", "Vec<ForecastAdjustmentMetric>"],
        ["key", "ForecastObservationStationKey"],
        ["latitude", "f64"],
        ["longitude", "f64"],
        ["nearestRank", "u64"],
        ["providerFamily", "ForecastObservationProviderFamily"],
        ["unnormalizedSpatialWeight", "f64"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastNetworkEventIdentity"),
      [
        ["cohort", "ForecastTrainingCohort"],
        [
          "contractVersion",
          "ForecastNetworkEventIdentityContractVersionLiteral",
        ],
        ["leadBand", "ForecastLeadBandKey"],
        ["metric", "ForecastAdjustmentMetric"],
        ["referenceKind", "ForecastReferenceKind"],
        ["siteKey", "ForecastNetworkEventIdentitySiteKeyLiteral"],
        ["targetLeadHours", "u64"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentTrainingProvenance"),
      [
        ["aggregationContractSha256", "String"],
        ["coordinateManifestSha256", "String"],
        ["metricEligibilitySha256", "String"],
        ["observationSourceLineageSha256", "String"],
        ["observationStationManifestSha256", "String"],
        ["spatialWeightSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentMetricBand"),
      [
        ["leadBand", "ForecastLeadBandKey"],
        ["metric", "ForecastAdjustmentMetric"],
      ],
    );
    assert.match(
      rust,
      /wire rule: empty-json-array[^\n]*\n\s*pub acceptedIntervals: ForecastObservationExcludedSourceLineageAcceptedIntervalsEmptyArray,/u,
    );
    assert.match(
      rust,
      /impl ForecastNetworkEventIdentityContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-network-event\/v1";\s*\}/u,
    );
    const networkEventDefinition = rust.match(
      /^pub struct ForecastNetworkEventIdentity \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(networkEventDefinition);
    assert.match(
      networkEventDefinition[0],
      /wire rule: utc-iso[^\n]*\n\s*pub validAt: String,/u,
    );
    assert.equal(
      networkEventDefinition[0].match(/wire rule: utc-iso/gu)?.length,
      1,
    );

    assertManifestRootGroup(manifest.roots, [
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastTrainingCohort",
        rustName: "ForecastTrainingCohort",
        kind: "string-literal-enum",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastReferenceKind",
        rustName: "ForecastReferenceKind",
        kind: "string-literal-enum",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "FixedLeadAnchorTrainingRow",
        rustName: "FixedLeadAnchorTrainingRow",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "LegacyV4RetrievalSnapshotTrainingRow",
        rustName: "LegacyV4RetrievalSnapshotTrainingRow",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "LegacyV4RetrievalSnapshotTrainingRowInput",
        rustName: "LegacyV4RetrievalSnapshotTrainingRowInput",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastTrainingRow",
        rustName: "ForecastTrainingRow",
        kind: "named-readonly-object-union",
        objectUnion: {
          variants: [
            {
              rustName: "FixedLeadAnchor",
              payloadRustName: "FixedLeadAnchorTrainingRow",
            },
            {
              rustName: "LegacyV4RetrievalSnapshot",
              payloadRustName: "LegacyV4RetrievalSnapshotTrainingRow",
            },
          ],
        },
      },
      {
        source: "packages/database/src/repositories.ts",
        exportName: "ForecastTrainingCohorts",
        rustName: "ForecastTrainingCohorts",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastObservationProviderFamily",
        rustName: "ForecastObservationProviderFamily",
        kind: "string-literal-enum",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastObservationExcludedSourceLineage",
        rustName: "ForecastObservationExcludedSourceLineage",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastObservationStation",
        rustName: "ForecastObservationStation",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastNetworkEventIdentity",
        rustName: "ForecastNetworkEventIdentity",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentTrainingProvenance",
        rustName: "ForecastAdjustmentTrainingProvenance",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentMetricBand",
        rustName: "ForecastAdjustmentMetricBand",
        kind: "readonly-interface",
      },
    ]);
    const repositoryTrainingBinding = manifest.typeImports.find(
      // select the repository edge by its exact consumer and specifier
      ({ consumer, specifier }) =>
        consumer === "packages/database/src/repositories.ts" &&
        specifier === "@weather/domain",
    );
    assert.ok(repositoryTrainingBinding);
    assert.deepEqual({
      FixedLeadAnchorTrainingRow:
        repositoryTrainingBinding.exports.FixedLeadAnchorTrainingRow,
      LegacyV4RetrievalSnapshotTrainingRow:
        repositoryTrainingBinding.exports.LegacyV4RetrievalSnapshotTrainingRow,
    }, {
      FixedLeadAnchorTrainingRow:
        "packages/domain/src/forecast-adjustment.ts",
      LegacyV4RetrievalSnapshotTrainingRow:
        "packages/domain/src/forecast-adjustment.ts",
    });
    const metricsBinding = manifest.typeImports.find(
      // select the direct sibling edge by its exact consumer and specifier
      ({ consumer, specifier }) =>
        consumer === "packages/domain/src/forecast-adjustment.ts" &&
        specifier === "./weather-record.js",
    );
    assert.deepEqual(metricsBinding, {
      consumer: "packages/domain/src/forecast-adjustment.ts",
      specifier: "./weather-record.js",
      entry: "packages/domain/src/weather-record.ts",
      exports: {
        CanonicalWeatherMetrics: "packages/domain/src/weather-record.ts",
      },
    });
  },
);

test(
  "projects forecast adjustment candidate and registry contracts",
  // preserve fitted artifact identities without widening exact metadata
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const coefficientConfig = CONFIG.roots.find(
      // select the exact coefficient root independently of later roots
      ({ source, exportName }) =>
        source === "packages/domain/src/forecast-adjustment.ts" &&
        exportName === "ForecastAdjustmentCoefficient",
    );
    const envelopeConfig = CONFIG.roots.find(
      // select the exact envelope root independently of later roots
      ({ source, exportName }) =>
        source === "packages/domain/src/forecast-adjustment.ts" &&
        exportName === "ForecastAdjustmentTrainingEnvelope",
    );

    assert.deepEqual(coefficientConfig?.fields, {
      coefficient: "finite-f64",
      effectiveEventCount: "finite-f64",
      level: "unsigned-safe-integer-enum",
      month: "safe-unsigned-integer",
    });
    assert.deepEqual(envelopeConfig?.fieldProjections, {
      metric: {
        kind: "exclude-literal",
        root: "ForecastAdjustmentMetric",
      },
    });
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentForecastIdentity"),
      [
        ["adapterVersion", "String"],
        [
          "cohort",
          "ForecastAdjustmentForecastIdentityCohortLiteral",
        ],
        ["contractEpoch", "String"],
        ["dataset", "String"],
        [
          "referenceKind",
          "ForecastAdjustmentForecastIdentityReferenceKindLiteral",
        ],
        ["sourceConfigFingerprint", "String"],
        ["sourceKey", "String"],
        ["upstreamModel", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentCoefficient"),
      [
        ["coefficient", "f64"],
        ["daypart", "Nullable<ForecastAdjustmentCoefficientDaypartEnum>"],
        ["effectiveEventCount", "f64"],
        ["leadBand", "ForecastLeadBandKey"],
        ["level", "ForecastAdjustmentCoefficientLevelEnum"],
        ["metric", "ForecastAdjustmentMetric"],
        ["month", "Nullable<u64>"],
        ["season", "Nullable<ForecastAdjustmentCoefficientSeasonEnum>"],
      ],
    );
    assert.match(
      rust,
      /pub enum ForecastAdjustmentCoefficientLevelEnum \{\s*#\[doc = "wire value: 1"\]\s*Value1,\s*#\[doc = "wire value: 2"\]\s*Value2,\s*#\[doc = "wire value: 3"\]\s*Value3,\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentCoefficientLevelEnum \{\s*pub const WIRE_VALUES: &'static \[\(u64, Self\)\] = &\[\s*\(1, Self::Value1\),\s*\(2, Self::Value2\),\s*\(3, Self::Value3\),\s*\];\s*\}/u,
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ForecastAdjustmentTrainingEnvelopeMetricEnum",
      ),
      [
        ["relativeHumidityPercent", "RelativeHumidityPercent"],
        ["temperatureC", "TemperatureC"],
        ["windGustMps", "WindGustMps"],
        ["windSpeedMps", "WindSpeedMps"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentTrainingEnvelope"),
      [
        ["leadBand", "ForecastLeadBandKey"],
        ["maximum", "f64"],
        ["metric", "ForecastAdjustmentTrainingEnvelopeMetricEnum"],
        ["minimum", "f64"],
      ],
    );
    assert.match(
      rust,
      /field projection: exclude-literal -> ForecastAdjustmentMetric[^\n]*\n\s*pub metric: ForecastAdjustmentTrainingEnvelopeMetricEnum,/u,
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentMetricPolicy"),
      [
        ["correctionMaximum", "f64"],
        ["correctionMinimum", "f64"],
        ["finalMaximum", "f64"],
        ["finalMaximumExclusive", "bool"],
        ["finalMinimum", "f64"],
        ["metric", "ForecastAdjustmentMetric"],
        ["wrapsFinalValue", "bool"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentCandidateV2"),
      [
        [
          "algorithmContractVersion",
          "ForecastAdjustmentCandidateV2AlgorithmContractVersionLiteral",
        ],
        ["candidateArtifactSha256", "String"],
        ["coefficientPayloadSha256", "String"],
        ["coefficients", "Vec<ForecastAdjustmentCoefficient>"],
        [
          "contractVersion",
          "ForecastAdjustmentCandidateV2ContractVersionLiteral",
        ],
        ["developmentReportSha256", "String"],
        ["enabledMetricBands", "Vec<ForecastAdjustmentMetricBand>"],
        ["evaluationEpochId", "String"],
        ["exportManifestSha256", "String"],
        ["finalTrainingCutoff", "String"],
        ["forecastIdentity", "ForecastAdjustmentForecastIdentity"],
        ["metricPolicies", "Vec<ForecastAdjustmentMetricPolicy>"],
        [
          "runtimeFingerprint",
          "ForecastAdjustmentCandidateV2RuntimeFingerprint",
        ],
        ["siteKey", "ForecastAdjustmentCandidateV2SiteKeyLiteral"],
        ["timezone", "ForecastAdjustmentCandidateV2TimezoneLiteral"],
        [
          "trainingEnvelopes",
          "Vec<ForecastAdjustmentTrainingEnvelope>",
        ],
        ["trainingProvenance", "ForecastAdjustmentTrainingProvenance"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentCandidateV2RuntimeFingerprint",
      ),
      [
        ["icuVersion", "String"],
        ["tzdataVersion", "String"],
      ],
    );
    const candidateDefinition = rust.match(
      /^pub struct ForecastAdjustmentCandidateV2 \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(candidateDefinition);
    assert.equal(
      rustStructFieldEntries(rust, "ForecastAdjustmentCandidateV2").length,
      17,
    );
    assert.doesNotMatch(candidateDefinition[0], /wire rule: utc-iso/u);
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindCanaryTrainingIdentity",
      ),
      [
        ["adapterVersion", "String"],
        [
          "cohort",
          "ForecastAdjustmentWindCanaryTrainingIdentityCohortLiteral",
        ],
        ["contractEpoch", "String"],
        [
          "dataset",
          "ForecastAdjustmentWindCanaryTrainingIdentityDatasetLiteral",
        ],
        [
          "referenceKind",
          "ForecastAdjustmentWindCanaryTrainingIdentityReferenceKindLiteral",
        ],
        ["sourceConfigFingerprint", "String"],
        ["sourceKey", "String"],
        [
          "upstreamModel",
          "ForecastAdjustmentWindCanaryTrainingIdentityUpstreamModelLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentWindCanaryCandidateV1"),
      [
        [
          "algorithmContractVersion",
          "ForecastAdjustmentWindCanaryCandidateV1AlgorithmContractVersionLiteral",
        ],
        [
          "artifactKind",
          "ForecastAdjustmentWindCanaryCandidateV1ArtifactKindLiteral",
        ],
        ["candidateArtifactSha256", "String"],
        ["coefficientPayloadSha256", "String"],
        ["coefficients", "Vec<ForecastAdjustmentCoefficient>"],
        [
          "contractVersion",
          "ForecastAdjustmentWindCanaryCandidateV1ContractVersionLiteral",
        ],
        ["enabledMetricBands", "Vec<ForecastAdjustmentMetricBand>"],
        ["exportManifestSha256", "String"],
        ["finalTrainingCutoff", "String"],
        [
          "runtimeFingerprint",
          "ForecastAdjustmentWindCanaryCandidateV1RuntimeFingerprint",
        ],
        ["servedForecastIdentity", "ForecastAdjustmentForecastIdentity"],
        [
          "siteKey",
          "ForecastAdjustmentWindCanaryCandidateV1SiteKeyLiteral",
        ],
        [
          "timezone",
          "ForecastAdjustmentWindCanaryCandidateV1TimezoneLiteral",
        ],
        [
          "trainingEnvelopes",
          "Vec<ForecastAdjustmentTrainingEnvelope>",
        ],
        [
          "trainingForecastIdentity",
          "ForecastAdjustmentWindCanaryTrainingIdentity",
        ],
        ["trainingProvenance", "ForecastAdjustmentTrainingProvenance"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindCanaryCandidateV1RuntimeFingerprint",
      ),
      [
        ["icuVersion", "String"],
        ["tzdataVersion", "String"],
      ],
    );
    const windCandidateDefinition = rust.match(
      /^pub struct ForecastAdjustmentWindCanaryCandidateV1 \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(windCandidateDefinition);
    assert.doesNotMatch(windCandidateDefinition[0], /wire rule: utc-iso/u);
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentRegistryActiveBundleV1",
      ),
      [
        ["bundleSha256", "String"],
        ["candidateArtifactSha256", "String"],
        ["evaluationReportSha256", "String"],
        ["path", "String"],
        ["qualificationReceiptSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentRegistryV1"),
      [
        [
          "activeBundle",
          "Nullable<ForecastAdjustmentRegistryActiveBundleV1>",
        ],
        [
          "contractVersion",
          "ForecastAdjustmentRegistryV1ContractVersionLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindCanaryRegistryActiveBundleV1",
      ),
      [
        ["authorizationSha256", "String"],
        ["bundleSha256", "String"],
        ["candidateArtifactSha256", "String"],
        ["path", "String"],
        ["transferReportSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentWindCanaryRegistryV1"),
      [
        [
          "activeBundle",
          "Nullable<ForecastAdjustmentWindCanaryRegistryActiveBundleV1>",
        ],
        [
          "contractVersion",
          "ForecastAdjustmentWindCanaryRegistryV1ContractVersionLiteral",
        ],
      ],
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentCandidateV2ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-candidate\/v2";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentWindCanaryCandidateV1ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-wind-canary-candidate\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentRegistryV1ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-registry\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentWindCanaryRegistryV1ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-wind-canary-registry\/v1";\s*\}/u,
    );

    assertManifestRootGroup(manifest.roots, [
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentForecastIdentity",
        rustName: "ForecastAdjustmentForecastIdentity",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentCoefficient",
        rustName: "ForecastAdjustmentCoefficient",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentTrainingEnvelope",
        rustName: "ForecastAdjustmentTrainingEnvelope",
        kind: "readonly-interface",
        fieldProjections: {
          metric: {
            kind: "exclude-literal",
            root: "ForecastAdjustmentMetric",
          },
        },
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentMetricPolicy",
        rustName: "ForecastAdjustmentMetricPolicy",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentCandidateV2",
        rustName: "ForecastAdjustmentCandidateV2",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentWindCanaryTrainingIdentity",
        rustName: "ForecastAdjustmentWindCanaryTrainingIdentity",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentWindCanaryCandidateV1",
        rustName: "ForecastAdjustmentWindCanaryCandidateV1",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentRegistryActiveBundleV1",
        rustName: "ForecastAdjustmentRegistryActiveBundleV1",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentRegistryV1",
        rustName: "ForecastAdjustmentRegistryV1",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentWindCanaryRegistryActiveBundleV1",
        rustName: "ForecastAdjustmentWindCanaryRegistryActiveBundleV1",
        kind: "readonly-interface",
      },
      {
        source: "packages/domain/src/forecast-adjustment.ts",
        exportName: "ForecastAdjustmentWindCanaryRegistryV1",
        rustName: "ForecastAdjustmentWindCanaryRegistryV1",
        kind: "readonly-interface",
      },
    ]);
  },
);

test(
  "projects forecast adjustment evaluation, qualification, and runtime evidence",
  // preserve the immutable evidence graph and flat critical-score payloads
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const pairedScoreConfig = CONFIG.roots.find(
      // select the exact score root independently of global root position
      ({ source, exportName }) =>
        source === "packages/domain/src/forecast-adjustment.ts" &&
        exportName === "ForecastAdjustmentPairedScore",
    );
    const criticalSliceConfig = CONFIG.roots.find(
      // select the exact tagged score root independently of global root position
      ({ source, exportName }) =>
        source === "packages/domain/src/forecast-adjustment.ts" &&
        exportName === "ForecastAdjustmentCriticalSliceScore",
    );

    assert.deepEqual(pairedScoreConfig?.fields, {
      adjustedLoss: "finite-f64",
      bootstrapLowerBound: "finite-f64",
      bootstrapUpperBound: "finite-f64",
      rawLoss: "finite-f64",
      skill: "finite-f64",
      eventCount: "safe-unsigned-integer",
    });
    assert.deepEqual(criticalSliceConfig, {
      source: "packages/domain/src/forecast-adjustment.ts",
      exportName: "ForecastAdjustmentCriticalSliceScore",
      fields: { "[nearest_three].key": "const-string" },
      base: "ForecastAdjustmentPairedScore",
      discriminator: "kind",
    });
    const pairedScoreFields = [
      ["adjustedLoss", "f64"],
      ["bootstrapLowerBound", "f64"],
      ["bootstrapUpperBound", "f64"],
      ["eventCount", "u64"],
      ["rawLoss", "f64"],
      ["skill", "f64"],
    ];
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentPairedScore"),
      pairedScoreFields,
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "ForecastSeasonDaypartKey"),
      [
        ["autumn-afternoon", "AutumnAfternoon"],
        ["autumn-evening", "AutumnEvening"],
        ["autumn-morning", "AutumnMorning"],
        ["autumn-night", "AutumnNight"],
        ["spring-afternoon", "SpringAfternoon"],
        ["spring-evening", "SpringEvening"],
        ["spring-morning", "SpringMorning"],
        ["spring-night", "SpringNight"],
        ["summer-afternoon", "SummerAfternoon"],
        ["summer-evening", "SummerEvening"],
        ["summer-morning", "SummerMorning"],
        ["summer-night", "SummerNight"],
        ["winter-afternoon", "WinterAfternoon"],
        ["winter-evening", "WinterEvening"],
        ["winter-morning", "WinterMorning"],
        ["winter-night", "WinterNight"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentCriticalSliceScoreNearestThree",
      ),
      [
        ...pairedScoreFields,
        ["key", "ForecastAdjustmentCriticalSliceScoreNearestThreeKeyLiteral"],
        ["kind", "ForecastAdjustmentCriticalSliceScoreNearestThreeKindLiteral"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentCriticalSliceScoreProviderFamily",
      ),
      [
        ...pairedScoreFields,
        ["key", "ForecastObservationProviderFamily"],
        ["kind", "ForecastAdjustmentCriticalSliceScoreProviderFamilyKindLiteral"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentCriticalSliceScoreSeasonDaypart",
      ),
      [
        ...pairedScoreFields,
        ["key", "ForecastSeasonDaypartKey"],
        ["kind", "ForecastAdjustmentCriticalSliceScoreSeasonDaypartKindLiteral"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentCriticalSliceScoreStation",
      ),
      [
        ...pairedScoreFields,
        ["key", "ForecastObservationStationKey"],
        ["kind", "ForecastAdjustmentCriticalSliceScoreStationKindLiteral"],
      ],
    );
    assert.match(
      rust,
      /pub enum ForecastAdjustmentCriticalSliceScore \{\s*#\[doc = "wire discriminant kind: \\"nearest_three\\""\]\s*NearestThree\(ForecastAdjustmentCriticalSliceScoreNearestThree\),\s*#\[doc = "wire discriminant kind: \\"provider_family\\""\]\s*ProviderFamily\(ForecastAdjustmentCriticalSliceScoreProviderFamily\),\s*#\[doc = "wire discriminant kind: \\"season_daypart\\""\]\s*SeasonDaypart\(ForecastAdjustmentCriticalSliceScoreSeasonDaypart\),\s*#\[doc = "wire discriminant kind: \\"station\\""\]\s*Station\(ForecastAdjustmentCriticalSliceScoreStation\),\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentCriticalSliceScoreNearestThreeKeyLiteral \{\s*pub const WIRE_VALUE: &'static str = "nearest-three";\s*\}/u,
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentMetricBandEvaluation"),
      [
        ["criticalSlices", "Vec<ForecastAdjustmentCriticalSliceScore>"],
        ["evaluatedSeasonDaypartKeys", "Vec<ForecastSeasonDaypartKey>"],
        ["metricBand", "ForecastAdjustmentMetricBand"],
        ["network", "ForecastAdjustmentPairedScore"],
        ["providerBalanced", "ForecastAdjustmentPairedScore"],
        ["scoreableStationKeys", "Vec<ForecastObservationStationKey>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentEvaluationReportV2"),
      [
        ["candidateArtifactSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentEvaluationReportV2ContractVersionLiteral",
        ],
        ["enabledMetricBands", "Vec<ForecastAdjustmentMetricBand>"],
        ["evaluationEpochId", "String"],
        ["evaluationReportSha256", "String"],
        ["holdoutAccessMarkerSha256", "String"],
        ["holdoutEndExclusive", "String"],
        ["holdoutEndLocalDate", "String"],
        ["holdoutStartInclusive", "String"],
        ["holdoutStartLocalDate", "String"],
        [
          "metricBandEvaluations",
          "Vec<ForecastAdjustmentMetricBandEvaluation>",
        ],
        ["preregistrationSha256", "String"],
        ["trainingProvenance", "ForecastAdjustmentTrainingProvenance"],
      ],
    );
    const evaluationDefinition = rust.match(
      /^pub struct ForecastAdjustmentEvaluationReportV2 \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(evaluationDefinition);
    assert.doesNotMatch(evaluationDefinition[0], /wire rule: utc-iso/u);
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ForecastAdjustmentQualificationGateName",
      ),
      [
        ["pooled_network_improvement", "PooledNetworkImprovement"],
        ["bootstrap_lower_bound", "BootstrapLowerBound"],
        ["development_fold_skill", "DevelopmentFoldSkill"],
        ["critical_slice_no_harm", "CriticalSliceNoHarm"],
        ["coefficient_coverage_and_caps", "CoefficientCoverageAndCaps"],
        ["locked_holdout", "LockedHoldout"],
        ["production_identity", "ProductionIdentity"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentQualificationGate"),
      [
        ["metricBand", "ForecastAdjustmentMetricBand"],
        ["name", "ForecastAdjustmentQualificationGateName"],
        ["passed", "bool"],
        ["reasonCode", "Nullable<ForecastAdjustmentReasonCode>"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentEvidenceRedundancy"),
      [
        ["attestationSha256", "String"],
        ["status", "ForecastAdjustmentEvidenceRedundancyStatusEnum"],
        ["verified", "bool"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ForecastAdjustmentEvidenceRedundancyStatusEnum",
      ),
      [
        [
          "independent_content_addressed_copy",
          "IndependentContentAddressedCopy",
        ],
        ["restorable_encrypted_backup", "RestorableEncryptedBackup"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentQualificationReceiptV2"),
      [
        ["candidateArtifactSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentQualificationReceiptV2ContractVersionLiteral",
        ],
        ["enabledMetricBands", "Vec<ForecastAdjustmentMetricBand>"],
        ["evaluationEpochId", "String"],
        ["evaluationReportSha256", "String"],
        ["evidenceRedundancy", "ForecastAdjustmentEvidenceRedundancy"],
        ["gates", "Vec<ForecastAdjustmentQualificationGate>"],
        ["holdoutAccessMarkerSha256", "String"],
        [
          "lifecycleState",
          "ForecastAdjustmentQualificationReceiptV2LifecycleStateEnum",
        ],
        ["passed", "bool"],
        ["preregistrationSha256", "String"],
        ["qualificationReceiptSha256", "String"],
        ["trainingProvenance", "ForecastAdjustmentTrainingProvenance"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentEvidenceTripleV2"),
      [
        ["candidate", "ForecastAdjustmentCandidateV2"],
        ["evaluationReport", "ForecastAdjustmentEvaluationReportV2"],
        [
          "qualificationReceipt",
          "ForecastAdjustmentQualificationReceiptV2",
        ],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "ForecastAdjustmentQualificationReceiptV2LifecycleStateEnum",
      ),
      [
        ["qualified", "Qualified"],
        ["rejected", "Rejected"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentRuntimeBundleV2"),
      [
        ["candidate", "ForecastAdjustmentCandidateV2"],
        ["evaluationReport", "ForecastAdjustmentEvaluationReportV2"],
        [
          "qualificationReceipt",
          "ForecastAdjustmentQualificationReceiptV2",
        ],
        ["bundleSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentRuntimeBundleV2ContractVersionLiteral",
        ],
        ["siteKey", "ForecastAdjustmentRuntimeBundleV2SiteKeyLiteral"],
        ["timezone", "ForecastAdjustmentRuntimeBundleV2TimezoneLiteral"],
      ],
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentEvaluationReportV2ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-evaluation-report\/v2";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentQualificationReceiptV2ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-qualification-receipt\/v2";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentRuntimeBundleV2ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-runtime-bundle\/v2";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentRuntimeBundleV2SiteKeyLiteral \{\s*pub const WIRE_VALUE: &'static str = "ballydidean";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentRuntimeBundleV2TimezoneLiteral \{\s*pub const WIRE_VALUE: &'static str = "America\/Los_Angeles";\s*\}/u,
    );

    assertManifestRootGroup(manifest.roots, RUNTIME_EVIDENCE_ROOTS);
  },
);

test(
  "projects wind-canary transfer, authorization, runtime, and candidate contracts",
  // preserve both runtime generations and the public union without widening wire fields
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const bridgeScoreConfig = CONFIG.roots.find(
      // select the exact score root across shared domain roots
      ({ source, exportName }) =>
        source === "packages/domain/src/forecast-adjustment.ts" &&
        exportName === "ForecastAdjustmentWindCanaryBridgeScore",
    );
    const authorizationV2Config = CONFIG.roots.find(
      // select the exact permanent authorization root
      ({ source, exportName }) =>
        source === "packages/forecast-adjustment/src/wind-canary.ts" &&
        exportName === "ForecastAdjustmentWindCanaryAuthorizationV2",
    );
    const runtimeV2Config = CONFIG.roots.find(
      // select the exact omitted-base runtime root
      ({ source, exportName }) =>
        source === "packages/forecast-adjustment/src/wind-canary.ts" &&
        exportName === "ForecastAdjustmentWindCanaryRuntimeBundleV2",
    );
    const runtimeUnionConfig = CONFIG.roots.find(
      // select the exact public runtime union root
      ({ source, exportName }) =>
        source === "packages/forecast-adjustment/src/wind-canary.ts" &&
        exportName === "ForecastAdjustmentWindCanaryRuntimeBundle",
    );

    assert.deepEqual(bridgeScoreConfig?.fields, {
      adjustedLoss: "finite-f64",
      eventCount: "safe-unsigned-integer",
      rawLoss: "finite-f64",
      skill: "finite-f64",
    });
    assert.deepEqual(authorizationV2Config?.fields, {
      contractVersion: "const-string",
      expiresAt: "exact-null",
    });
    assert.deepEqual(runtimeV2Config, {
      source: "packages/forecast-adjustment/src/wind-canary.ts",
      exportName: "ForecastAdjustmentWindCanaryRuntimeBundleV2",
      fields: { contractVersion: "const-string" },
      base: "ForecastAdjustmentWindCanaryRuntimeBundleV1",
      omitBaseFields: ["authorization", "contractVersion"],
    });
    assert.deepEqual(runtimeUnionConfig, {
      source: "packages/forecast-adjustment/src/wind-canary.ts",
      exportName: "ForecastAdjustmentWindCanaryRuntimeBundle",
      fields: {},
      importedObjectUnion: true,
      objectUnion: {
        ForecastAdjustmentWindCanaryRuntimeBundleV1: "TimeLimited",
        ForecastAdjustmentWindCanaryRuntimeBundleV2: "Permanent",
      },
    });
    assert.deepEqual(
      rustStructFieldEntries(rust, "ForecastAdjustmentWindCanaryBridgeScore"),
      [
        ["adjustedLoss", "f64"],
        ["eventCount", "u64"],
        ["rawLoss", "f64"],
        ["skill", "f64"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindCanaryBridgeEvaluation",
      ),
      [
        ["metricBand", "ForecastAdjustmentMetricBand"],
        ["network", "ForecastAdjustmentWindCanaryBridgeScore"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindCanaryTransferReportV1",
      ),
      [
        [
          "artifactKind",
          "ForecastAdjustmentWindCanaryTransferReportV1ArtifactKindLiteral",
        ],
        ["bridgeEndExclusive", "String"],
        [
          "bridgeEvaluations",
          "Vec<ForecastAdjustmentWindCanaryBridgeEvaluation>",
        ],
        ["bridgeStartInclusive", "String"],
        ["candidateArtifactSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentWindCanaryTransferReportV1ContractVersionLiteral",
        ],
        ["enabledMetricBands", "Vec<ForecastAdjustmentMetricBand>"],
        [
          "passed",
          "ForecastAdjustmentWindCanaryTransferReportV1PassedLiteral",
        ],
        ["servedForecastIdentity", "ForecastAdjustmentForecastIdentity"],
        [
          "trainingForecastIdentity",
          "ForecastAdjustmentWindCanaryTrainingIdentity",
        ],
        ["transferReportSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindCanaryAuthorizationV1",
      ),
      [
        ["activatedAt", "String"],
        [
          "artifactKind",
          "ForecastAdjustmentWindCanaryAuthorizationV1ArtifactKindLiteral",
        ],
        ["authorizationReason", "String"],
        ["authorizationSha256", "String"],
        [
          "authorized",
          "ForecastAdjustmentWindCanaryAuthorizationV1AuthorizedLiteral",
        ],
        ["authorizedAt", "String"],
        ["authorizedBy", "String"],
        ["candidateArtifactSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentWindCanaryAuthorizationV1ContractVersionLiteral",
        ],
        ["enabledMetricBands", "Vec<ForecastAdjustmentMetricBand>"],
        ["expiresAt", "String"],
        ["transferReportSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindCanaryRuntimeBundleV1",
      ),
      [
        [
          "artifactKind",
          "ForecastAdjustmentWindCanaryRuntimeBundleV1ArtifactKindLiteral",
        ],
        [
          "authorization",
          "ForecastAdjustmentWindCanaryAuthorizationV1",
        ],
        ["bundleSha256", "String"],
        ["candidate", "ForecastAdjustmentWindCanaryCandidateV1"],
        [
          "contractVersion",
          "ForecastAdjustmentWindCanaryRuntimeBundleV1ContractVersionLiteral",
        ],
        ["siteKey", "ForecastAdjustmentWindCanaryRuntimeBundleV1SiteKeyLiteral"],
        [
          "timezone",
          "ForecastAdjustmentWindCanaryRuntimeBundleV1TimezoneLiteral",
        ],
        [
          "transferReport",
          "ForecastAdjustmentWindCanaryTransferReportV1",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindCanaryAuthorizationV2",
      ),
      [
        ["activatedAt", "String"],
        [
          "artifactKind",
          "ForecastAdjustmentWindCanaryAuthorizationV2ArtifactKindLiteral",
        ],
        ["authorizationReason", "String"],
        ["authorizationSha256", "String"],
        [
          "authorized",
          "ForecastAdjustmentWindCanaryAuthorizationV2AuthorizedLiteral",
        ],
        ["authorizedAt", "String"],
        ["authorizedBy", "String"],
        ["candidateArtifactSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentWindCanaryAuthorizationV2ContractVersionLiteral",
        ],
        ["enabledMetricBands", "Vec<ForecastAdjustmentMetricBand>"],
        ["expiresAt", "ForecastAdjustmentWindCanaryAuthorizationV2ExpiresAtNull"],
        [
          "permanent",
          "ForecastAdjustmentWindCanaryAuthorizationV2PermanentLiteral",
        ],
        ["transferReportSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindCanaryRuntimeBundleV2",
      ),
      [
        [
          "artifactKind",
          "ForecastAdjustmentWindCanaryRuntimeBundleV1ArtifactKindLiteral",
        ],
        ["bundleSha256", "String"],
        ["candidate", "ForecastAdjustmentWindCanaryCandidateV1"],
        ["siteKey", "ForecastAdjustmentWindCanaryRuntimeBundleV1SiteKeyLiteral"],
        [
          "timezone",
          "ForecastAdjustmentWindCanaryRuntimeBundleV1TimezoneLiteral",
        ],
        [
          "transferReport",
          "ForecastAdjustmentWindCanaryTransferReportV1",
        ],
        [
          "authorization",
          "ForecastAdjustmentWindCanaryAuthorizationV2",
        ],
        [
          "contractVersion",
          "ForecastAdjustmentWindCanaryRuntimeBundleV2ContractVersionLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindCanaryCandidateInputV1",
      ),
      [
        ["coefficients", "Vec<ForecastAdjustmentCoefficient>"],
        ["enabledMetricBands", "Vec<ForecastAdjustmentMetricBand>"],
        ["exportManifestSha256", "String"],
        ["finalTrainingCutoff", "String"],
        [
          "runtimeFingerprint",
          "ForecastAdjustmentWindCanaryCandidateInputV1RuntimeFingerprint",
        ],
        ["servedForecastIdentity", "ForecastAdjustmentForecastIdentity"],
        [
          "trainingEnvelopes",
          "Vec<ForecastAdjustmentTrainingEnvelope>",
        ],
        [
          "trainingForecastIdentity",
          "ForecastAdjustmentWindCanaryTrainingIdentity",
        ],
        ["trainingProvenance", "ForecastAdjustmentTrainingProvenance"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentWindCanaryCandidateInputV1RuntimeFingerprint",
      ),
      [
        ["icuVersion", "String"],
        ["tzdataVersion", "String"],
      ],
    );
    assert.match(
      rust,
      /pub enum ForecastAdjustmentWindCanaryRuntimeBundle \{\s*TimeLimited\(ForecastAdjustmentWindCanaryRuntimeBundleV1\),\s*Permanent\(ForecastAdjustmentWindCanaryRuntimeBundleV2\),\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentWindCanaryTransferReportV1ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-wind-canary-transfer-report\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentWindCanaryAuthorizationV1ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-wind-canary-authorization\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentWindCanaryRuntimeBundleV1ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-wind-canary-runtime-bundle\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentWindCanaryAuthorizationV2ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-wind-canary-authorization\/v2";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentWindCanaryRuntimeBundleV2ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-wind-canary-runtime-bundle\/v2";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentWindCanaryAuthorizationV2AuthorizedLiteral \{\s*pub const WIRE_VALUE: bool = true;\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentWindCanaryAuthorizationV2PermanentLiteral \{\s*pub const WIRE_VALUE: bool = true;\s*\}/u,
    );
    assert.match(
      rust,
      /#\[doc = "exact null wire literal"\]\s*#\[derive\(Debug, Clone, Copy, PartialEq, Eq\)\]\s*pub struct ForecastAdjustmentWindCanaryAuthorizationV2ExpiresAtNull;/u,
    );
    // retain plain strings for source fields that do not opt into utc metadata
    for (const rootName of [
      "ForecastAdjustmentWindCanaryTransferReportV1",
      "ForecastAdjustmentWindCanaryAuthorizationV1",
      "ForecastAdjustmentWindCanaryAuthorizationV2",
      "ForecastAdjustmentWindCanaryCandidateInputV1",
    ]) {
      const definition = rust.match(
        new RegExp("^pub struct " + rootName + " \\{\\n[\\s\\S]+?^\\}", "mu"),
      );
      assert.ok(definition, rootName);
      assert.doesNotMatch(definition[0], /wire rule: utc-iso/u);
    }

    assertManifestRootGroup(manifest.roots, WIND_CANARY_RUNTIME_ROOTS);
    assert.deepEqual(
      manifest.typeImports.find(
        // select the exact wind-canary package edge
        ({ consumer, specifier }) =>
          consumer === "packages/forecast-adjustment/src/wind-canary.ts" &&
          specifier === "@weather/domain",
      ),
      WIND_CANARY_DOMAIN_IMPORT,
    );
  },
);

test(
  "projects ECMWF temperature-canary repository persistence and readback contracts",
  // keep raw persistence input separate from stored hours and timestamp validation
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);
    const recentErrorConfig = CONFIG.roots.find(
      // select the exact rolling-error root
      ({ source, exportName }) =>
        source === "packages/database/src/repositories.ts" &&
        exportName === "EcmwfTemperatureCanaryRecentErrorState",
    );
    const hourConfig = CONFIG.roots.find(
      // select the exact stored-hour root
      ({ source, exportName }) =>
        source === "packages/database/src/repositories.ts" &&
        exportName === "EcmwfTemperatureCanaryHour",
    );
    const inputConfig = CONFIG.roots.find(
      // select the exact persistence input root
      ({ source, exportName }) =>
        source === "packages/database/src/repositories.ts" &&
        exportName === "PersistEcmwfTemperatureCanaryRunInput",
    );

    assert.deepEqual(recentErrorConfig?.fields, {
      b24C: "finite-f64",
      b72C: "finite-f64",
      mad72C: "finite-f64",
      localDates: "safe-unsigned-integer",
      n24: "safe-unsigned-integer",
      n72: "safe-unsigned-integer",
      targetRunInitializedAt: "utc-iso",
      windowEndValidAt: "utc-iso",
    });
    assert.deepEqual(hourConfig?.fields, {
      modelLeadHours: "safe-unsigned-integer",
      rawTemperatureC: "finite-f64",
      rawRelativeHumidityPercent: "finite-f64",
      rawWindSpeedMps: "finite-f64",
    });
    assert.deepEqual(inputConfig, {
      source: "packages/database/src/repositories.ts",
      exportName: "PersistEcmwfTemperatureCanaryRunInput",
      fields: {},
      indexedLiteralSources: {
        EcmwfTemperatureCanaryRun: "packages/database/src/repositories.ts",
      },
      fieldProjections: {
        adapterVersion: {
          kind: "indexed-literals",
          root: "EcmwfTemperatureCanaryRun",
        },
        hours: {
          kind: "omit-interface-array",
          root: "EcmwfTemperatureCanaryHour",
        },
        modelCycle: {
          kind: "indexed-literals",
          root: "EcmwfTemperatureCanaryRun",
        },
        stateStatus: {
          kind: "indexed-literals",
          root: "EcmwfTemperatureCanaryRun",
        },
        upstreamModel: {
          kind: "indexed-literals",
          root: "EcmwfTemperatureCanaryRun",
        },
      },
    });
    assert.deepEqual(
      rustStructFieldEntries(rust, "EcmwfTemperatureCanaryRecentErrorState"),
      [
        ["b24C", "Nullable<f64>"],
        ["b72C", "Nullable<f64>"],
        [
          "cohort",
          "EcmwfTemperatureCanaryRecentErrorStateCohortLiteral",
        ],
        ["localDates", "u64"],
        ["mad72C", "Nullable<f64>"],
        ["maximumSourceRunInitializedAt", "Nullable<String>"],
        ["maximumSourceValidAt", "Nullable<String>"],
        ["n24", "u64"],
        ["n72", "u64"],
        ["sourceKeys", "Vec<String>"],
        ["supported", "bool"],
        ["targetRunInitializedAt", "String"],
        ["windowEndValidAt", "String"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "EcmwfTemperatureCanaryHour"), [
      ["contentHash", "String"],
      ["modelLeadHours", "u64"],
      ["rawRelativeHumidityPercent", "Nullable<f64>"],
      ["rawTemperatureC", "f64"],
      ["rawWindSpeedMps", "Nullable<f64>"],
      ["validAt", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "EcmwfTemperatureCanaryRun"), [
      ["adapterVersion", "EcmwfTemperatureCanaryRunAdapterVersionLiteral"],
      ["firstReceivedAt", "String"],
      ["id", "String"],
      ["modelCycle", "EcmwfTemperatureCanaryRunModelCycleEnum"],
      ["providerResponseSha256", "String"],
      ["recentErrorState", "EcmwfTemperatureCanaryRecentErrorState"],
      ["runInitializedAt", "String"],
      ["stateReason", "String"],
      ["stateStatus", "EcmwfTemperatureCanaryRunStateStatusEnum"],
      ["upstreamModel", "EcmwfTemperatureCanaryRunUpstreamModelLiteral"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "EcmwfTemperatureCanarySidecar"),
      [
        ["hours", "Vec<EcmwfTemperatureCanaryHour>"],
        ["run", "EcmwfTemperatureCanaryRun"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "EcmwfTemperatureCanaryStatus"),
      [
        ["firstReceivedAt", "String"],
        ["hourCount", "u64"],
        ["latestRunInitializedAt", "String"],
        ["stateReason", "String"],
        ["stateStatus", "EcmwfTemperatureCanaryStatusStateStatusEnum"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "EcmwfTemperatureCanaryAuditRow"),
      [
        ["hour", "EcmwfTemperatureCanaryHour"],
        ["run", "EcmwfTemperatureCanaryRun"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "EcmwfTemperatureCanaryPriorHour"),
      [
        ["contentHash", "String"],
        ["modelLeadHours", "u64"],
        ["rawRelativeHumidityPercent", "Nullable<f64>"],
        ["rawTemperatureC", "f64"],
        ["rawWindSpeedMps", "Nullable<f64>"],
        ["validAt", "String"],
        ["key", "String"],
        ["runInitializedAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "PersistEcmwfTemperatureCanaryRunInputHoursEntry",
      ),
      [
        ["modelLeadHours", "u64"],
        ["rawRelativeHumidityPercent", "Nullable<f64>"],
        ["rawTemperatureC", "f64"],
        ["rawWindSpeedMps", "Nullable<f64>"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "PersistEcmwfTemperatureCanaryRunInput"),
      [
        [
          "adapterVersion",
          "PersistEcmwfTemperatureCanaryRunInputAdapterVersionEnum",
        ],
        [
          "hours",
          "Vec<PersistEcmwfTemperatureCanaryRunInputHoursEntry>",
        ],
        ["modelCycle", "PersistEcmwfTemperatureCanaryRunInputModelCycleEnum"],
        ["providerResponseSha256", "String"],
        ["receivedAt", "String"],
        ["recentErrorState", "EcmwfTemperatureCanaryRecentErrorState"],
        ["runInitializedAt", "String"],
        ["siteSlug", "String"],
        ["stateReason", "String"],
        ["stateStatus", "PersistEcmwfTemperatureCanaryRunInputStateStatusEnum"],
        ["upstreamModel", "PersistEcmwfTemperatureCanaryRunInputUpstreamModelEnum"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "EcmwfTemperatureCanaryRunModelCycleEnum"),
      [
        ["49r1", "Value49r1"],
        ["50r1", "Value50r1"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "EcmwfTemperatureCanaryRunStateStatusEnum"),
      [
        ["supported", "Supported"],
        ["cold", "Cold"],
        ["insufficient", "Insufficient"],
        ["invalid", "Invalid"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "EcmwfTemperatureCanaryStatusStateStatusEnum",
      ),
      [
        ["supported", "Supported"],
        ["cold", "Cold"],
        ["insufficient", "Insufficient"],
        ["invalid", "Invalid"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "PersistEcmwfTemperatureCanaryRunInputAdapterVersionEnum",
      ),
      [["open-meteo-ecmwf-single-run/v1", "OpenMeteoEcmwfSingleRunV1"]],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "PersistEcmwfTemperatureCanaryRunInputModelCycleEnum",
      ),
      [
        ["49r1", "Value49r1"],
        ["50r1", "Value50r1"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "PersistEcmwfTemperatureCanaryRunInputStateStatusEnum",
      ),
      [
        ["supported", "Supported"],
        ["cold", "Cold"],
        ["insufficient", "Insufficient"],
        ["invalid", "Invalid"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "PersistEcmwfTemperatureCanaryRunInputUpstreamModelEnum",
      ),
      [["ecmwf_ifs", "EcmwfIfs"]],
    );
    assert.match(
      rust,
      /impl EcmwfTemperatureCanaryRecentErrorStateCohortLiteral \{\s*pub const WIRE_VALUE: &'static str = "ecmwf_single_run_hindcast";\s*\}/u,
    );
    assert.match(
      rust,
      /impl EcmwfTemperatureCanaryRunAdapterVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "open-meteo-ecmwf-single-run\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl EcmwfTemperatureCanaryRunUpstreamModelLiteral \{\s*pub const WIRE_VALUE: &'static str = "ecmwf_ifs";\s*\}/u,
    );

    const recentErrorDefinition = rust.match(
      /^pub struct EcmwfTemperatureCanaryRecentErrorState \{\n[\s\S]+?^\}/mu,
    );
    const hourDefinition = rust.match(
      /^pub struct EcmwfTemperatureCanaryHour \{\n[\s\S]+?^\}/mu,
    );
    const runDefinition = rust.match(
      /^pub struct EcmwfTemperatureCanaryRun \{\n[\s\S]+?^\}/mu,
    );
    const statusDefinition = rust.match(
      /^pub struct EcmwfTemperatureCanaryStatus \{\n[\s\S]+?^\}/mu,
    );
    const priorDefinition = rust.match(
      /^pub struct EcmwfTemperatureCanaryPriorHour \{\n[\s\S]+?^\}/mu,
    );
    const entryDefinition = rust.match(
      /^pub struct PersistEcmwfTemperatureCanaryRunInputHoursEntry \{\n[\s\S]+?^\}/mu,
    );
    const inputDefinition = rust.match(
      /^pub struct PersistEcmwfTemperatureCanaryRunInput \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(recentErrorDefinition);
    assert.ok(hourDefinition);
    assert.ok(runDefinition);
    assert.ok(statusDefinition);
    assert.ok(priorDefinition);
    assert.ok(entryDefinition);
    assert.ok(inputDefinition);
    assert.equal(recentErrorDefinition[0].match(/wire rule: utc-iso/gu)?.length, 2);
    assert.equal(runDefinition[0].match(/wire rule: utc-iso/gu)?.length, 2);
    assert.equal(statusDefinition[0].match(/wire rule: utc-iso/gu)?.length, 2);
    assert.equal(priorDefinition[0].match(/wire rule: utc-iso/gu)?.length, 1);
    assert.doesNotMatch(hourDefinition[0], /wire rule: utc-iso/u);
    assert.doesNotMatch(entryDefinition[0], /wire rule: utc-iso/u);
    assert.doesNotMatch(inputDefinition[0], /wire rule: utc-iso/u);
    assert.doesNotMatch(entryDefinition[0], /contentHash/u);
    assert.equal(entryDefinition[0].match(/wire rule: finite-f64/gu)?.length, 3);
    assert.match(
      entryDefinition[0],
      /wire rule: safe-unsigned-integer[^\n]*\n\s*pub modelLeadHours: u64,/u,
    );

    assertManifestRootGroup(manifest.roots, ECMWF_TEMPERATURE_CANARY_ROOTS);
  },
);

test(
  "projects the original temperature mos inference model contracts",
  // preserve source order, fitted identities, exact time rules and serving variants
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "TemperatureMosRuntimeForecast"), [
      ["cohort", "TemperatureMosRuntimeForecastCohortLiteral"],
      ["key", "String"],
      ["modelCycle", "TemperatureMosRuntimeForecastModelCycleEnum"],
      ["modelLeadHours", "u64"],
      ["rawRelativeHumidityPercent", "Nullable<f64>"],
      ["rawTemperatureC", "f64"],
      ["rawWindSpeedMps", "Nullable<f64>"],
      ["runInitializedAt", "String"],
      ["validAt", "String"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "TemperatureMosRuntimeRecentErrorState"),
      [
        ["b24C", "Nullable<f64>"],
        ["b72C", "Nullable<f64>"],
        ["cohort", "TemperatureMosRuntimeRecentErrorStateCohortLiteral"],
        ["localDates", "u64"],
        ["mad72C", "Nullable<f64>"],
        ["maximumSourceRunInitializedAt", "Nullable<String>"],
        ["maximumSourceValidAt", "Nullable<String>"],
        ["n24", "u64"],
        ["n72", "u64"],
        ["sourceKeys", "Vec<String>"],
        ["supported", "bool"],
        ["targetRunInitializedAt", "String"],
        ["windowEndValidAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "TemperatureMosRuntimeStrengthBand"),
      [
        ["alpha", "f64"],
        ["supported", "bool"],
        ["trainingCutoffUtc", "String"],
      ],
    );
    const materialFields = [
      ["adaptiveCoefficients", "Nullable<Vec<f64>>"],
      ["cohort", "TemperatureMosRuntimeModelMaterialCohortLiteral"],
      ["directCoefficients", "Nullable<Vec<f64>>"],
      [
        "learnedStrengthContractVersion",
        "TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral",
      ],
      ["scope", "TemperatureMosRuntimeModelMaterialScopeEnum"],
      [
        "strengthBands",
        "TemperatureMosRuntimeModelMaterialStrengthBands",
      ],
      ["supported", "bool"],
      ["trainingCutoffUtc", "String"],
    ];
    assert.deepEqual(
      rustStructFieldEntries(rust, "TemperatureMosRuntimeModelMaterial"),
      materialFields,
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "TemperatureMosRuntimeModel"),
      [
        ...materialFields,
        ["contractVersion", "TemperatureMosRuntimeModelContractVersionLiteral"],
        ["month", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "TemperatureMosPermanentRuntimeModel"),
      [
        ...materialFields,
        [
          "contractVersion",
          "TemperatureMosPermanentRuntimeModelContractVersionLiteral",
        ],
        ["effectiveFrom", "String"],
        ["latestTrainingValidAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "TemperatureMosRuntimeModelMaterialStrengthBands"),
      [
        ["hours1To6", "TemperatureMosRuntimeStrengthBand"],
        ["hours7To12", "TemperatureMosRuntimeStrengthBand"],
      ],
    );
    assert.deepEqual(rustStructFieldEntries(rust, "TemperatureMosRuntimeInput"), [
      ["forecast", "TemperatureMosRuntimeForecast"],
      ["model", "TemperatureMosServingModel"],
      ["recentErrorState", "TemperatureMosRuntimeRecentErrorState"],
    ]);
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "TemperatureMosRuntimeForecastModelCycleEnum"),
      [
        ["49r1", "Value49r1"],
        ["50r1", "Value50r1"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "TemperatureMosRuntimeModelMaterialScopeEnum"),
      [
        ["initialization_first12", "InitializationFirst12"],
        ["assumed_delay6_next12", "AssumedDelay6Next12"],
      ],
    );
    assert.match(
      rust,
      /pub enum TemperatureMosServingModel \{\s*Monthly\(TemperatureMosRuntimeModel\),\s*Permanent\(TemperatureMosPermanentRuntimeModel\),\s*\}/u,
    );
    assert.match(
      rust,
      /impl TemperatureMosRuntimeForecastCohortLiteral \{\s*pub const WIRE_VALUE: &'static str = "ecmwf_single_run_hindcast";\s*\}/u,
    );
    assert.match(
      rust,
      /impl TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "temperature-winner-extensions-research\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl TemperatureMosRuntimeModelContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "temperature-shortlead-models-research\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl TemperatureMosPermanentRuntimeModelContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "temperature-permanent-model\/v1";\s*\}/u,
    );
    const forecastDefinition = rust.match(
      /^pub struct TemperatureMosRuntimeForecast \{\n[\s\S]+?^\}/mu,
    );
    const recentDefinition = rust.match(
      /^pub struct TemperatureMosRuntimeRecentErrorState \{\n[\s\S]+?^\}/mu,
    );
    const strengthDefinition = rust.match(
      /^pub struct TemperatureMosRuntimeStrengthBand \{\n[\s\S]+?^\}/mu,
    );
    const materialDefinition = rust.match(
      /^pub struct TemperatureMosRuntimeModelMaterial \{\n[\s\S]+?^\}/mu,
    );
    const permanentDefinition = rust.match(
      /^pub struct TemperatureMosPermanentRuntimeModel \{\n[\s\S]+?^\}/mu,
    );
    const bandsDefinition = rust.match(
      /^pub struct TemperatureMosRuntimeModelMaterialStrengthBands \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(forecastDefinition);
    assert.ok(recentDefinition);
    assert.ok(strengthDefinition);
    assert.ok(materialDefinition);
    assert.ok(permanentDefinition);
    assert.ok(bandsDefinition);
    assert.equal(forecastDefinition[0].match(/wire rule: finite-f64/gu)?.length, 3);
    assert.equal(forecastDefinition[0].match(/wire rule: utc-iso/gu)?.length, 2);
    assert.equal(
      forecastDefinition[0].match(/wire rule: safe-unsigned-integer/gu)?.length,
      1,
    );
    assert.match(
      forecastDefinition[0],
      /wire rule: frozen-const-string-property, exact builtin-frozen local const string value"\]\s*pub cohort: TemperatureMosRuntimeForecastCohortLiteral,/u,
    );
    assert.equal(recentDefinition[0].match(/wire rule: finite-f64/gu)?.length, 3);
    assert.equal(recentDefinition[0].match(/wire rule: utc-iso/gu)?.length, 4);
    assert.equal(
      recentDefinition[0].match(/wire rule: safe-unsigned-integer/gu)?.length,
      3,
    );
    assert.equal(strengthDefinition[0].match(/wire rule: finite-f64/gu)?.length, 1);
    assert.equal(strengthDefinition[0].match(/wire rule: utc-iso/gu)?.length, 1);
    assert.equal(materialDefinition[0].match(/wire rule: finite-f64/gu)?.length, 2);
    assert.equal(materialDefinition[0].match(/wire rule: utc-iso/gu)?.length, 1);
    assert.match(
      materialDefinition[0],
      /wire rule: frozen-const-string-property-union, two distinct builtin-frozen local const string values"\]\s*pub scope: TemperatureMosRuntimeModelMaterialScopeEnum,/u,
    );
    assert.equal(permanentDefinition[0].match(/wire rule: utc-iso/gu)?.length, 3);
    assert.match(
      bandsDefinition[0],
      /#\[doc = "wire name: 1-6"\]\s*pub hours1To6: TemperatureMosRuntimeStrengthBand,\s*#\[doc = "wire name: 7-12"\]\s*pub hours7To12: TemperatureMosRuntimeStrengthBand,/u,
    );

    assertManifestRootGroup(manifest.roots, TEMPERATURE_MOS_MODEL_ROOTS);
  },
);

test(
  "projects temperature-canary authorization, runtime, maintenance, and registry contracts",
  // preserve both bundle generations and reuse their exact nested identity fields
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureCanaryAuthorizationV1",
      ),
      [
        ["activatedAt", "String"],
        ["authorizationReason", "String"],
        ["authorizationSha256", "String"],
        [
          "authorized",
          "ForecastAdjustmentTemperatureCanaryAuthorizationV1AuthorizedLiteral",
        ],
        ["authorizedAt", "String"],
        ["authorizedBy", "String"],
        ["expiresAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureCanaryAuthorizationV2",
      ),
      [
        ["activatedAt", "String"],
        [
          "artifactKind",
          "ForecastAdjustmentTemperatureCanaryAuthorizationV2ArtifactKindLiteral",
        ],
        ["authorizationReason", "String"],
        ["authorizationSha256", "String"],
        [
          "authorized",
          "ForecastAdjustmentTemperatureCanaryAuthorizationV2AuthorizedLiteral",
        ],
        ["authorizedAt", "String"],
        ["authorizedBy", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentTemperatureCanaryAuthorizationV2ContractVersionLiteral",
        ],
        [
          "expiresAt",
          "ForecastAdjustmentTemperatureCanaryAuthorizationV2ExpiresAtNull",
        ],
        [
          "permanent",
          "ForecastAdjustmentTemperatureCanaryAuthorizationV2PermanentLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1",
      ),
      [
        [
          "artifactKind",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ArtifactKindLiteral",
        ],
        [
          "authorization",
          "ForecastAdjustmentTemperatureCanaryAuthorizationV1",
        ],
        ["bundleSha256", "String"],
        [
          "contractVersion",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ContractVersionLiteral",
        ],
        [
          "evidence",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1Evidence",
        ],
        ["model", "TemperatureMosRuntimeModel"],
        [
          "runtimeFingerprint",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1RuntimeFingerprint",
        ],
        [
          "servedForecastIdentity",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentity",
        ],
        [
          "siteKey",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1SiteKeyLiteral",
        ],
        [
          "timezone",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TimezoneLiteral",
        ],
        [
          "trainingForecastIdentity",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentity",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentity",
      ),
      [
        [
          "adapterVersion",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityAdapterVersionLiteral",
        ],
        [
          "dataset",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityDatasetLiteral",
        ],
        ["maximumReceiptAgeHours", "u64"],
        [
          "providerKey",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityProviderKeyLiteral",
        ],
        [
          "sourceDelayHours",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentitySourceDelayHoursLiteral",
        ],
        [
          "upstreamModel",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityUpstreamModelLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1Evidence",
      ),
      [
        ["modelSourceSha256", "String"],
        ["researchSummarySha256", "String"],
        ["retentionManifestSha256", "String"],
        ["strengthSourceSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1RuntimeFingerprint",
      ),
      [
        ["icuVersion", "String"],
        ["tzdataVersion", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentity",
      ),
      [
        [
          "cohort",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentityCohortLiteral",
        ],
        [
          "scope",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentityScopeLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureCanaryRuntimeBundleV2",
      ),
      [
        [
          "artifactKind",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ArtifactKindLiteral",
        ],
        ["bundleSha256", "String"],
        [
          "evidence",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1Evidence",
        ],
        [
          "runtimeFingerprint",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1RuntimeFingerprint",
        ],
        [
          "servedForecastIdentity",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentity",
        ],
        [
          "siteKey",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1SiteKeyLiteral",
        ],
        [
          "timezone",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TimezoneLiteral",
        ],
        [
          "trainingForecastIdentity",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentity",
        ],
        [
          "authorization",
          "ForecastAdjustmentTemperatureCanaryAuthorizationV2",
        ],
        [
          "contractVersion",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV2ContractVersionLiteral",
        ],
        ["model", "TemperatureMosPermanentRuntimeModel"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureMaintenanceMaterial",
      ),
      [
        ["bundleSha256", "String"],
        [
          "maintenanceAuthority",
          "Nullable<ForecastAdjustmentTemperatureMaintenanceMaterialMaintenanceAuthority>",
        ],
        ["model", "TemperatureMosPermanentRuntimeModel"],
        [
          "servedForecastIdentity",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentity",
        ],
        [
          "trainingForecastIdentity",
          "ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentity",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureMaintenanceMaterialMaintenanceAuthority",
      ),
      [
        ["actionSha256", "String"],
        ["fullMemberRootSha256", "String"],
        ["policyReportSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureCanaryRegistryV1",
      ),
      [
        [
          "activeBundle",
          "Nullable<ForecastAdjustmentTemperatureCanaryRegistryV1ActiveBundle>",
        ],
        [
          "contractVersion",
          "ForecastAdjustmentTemperatureCanaryRegistryV1ContractVersionLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "ForecastAdjustmentTemperatureCanaryRegistryV1ActiveBundle",
      ),
      [
        ["authorizationSha256", "String"],
        ["bundleSha256", "String"],
        ["modelSourceSha256", "String"],
        ["path", "String"],
        ["strengthSourceSha256", "String"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Active",
      ),
      [
        ["bundle", "ForecastAdjustmentTemperatureCanaryRuntimeBundle"],
        [
          "reasonCode",
          "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1ActiveReasonCodeNull",
        ],
        [
          "state",
          "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1ActiveStateLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(
        rust,
        "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Disabled",
      ),
      [
        [
          "bundle",
          "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledBundleNull",
        ],
        [
          "reasonCode",
          "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum",
        ],
        [
          "state",
          "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledStateLiteral",
        ],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum",
      ),
      [
        ["bundle_invalid", "BundleInvalid"],
        ["bundle_missing", "BundleMissing"],
        ["canary_expired", "CanaryExpired"],
        ["canary_killed", "CanaryKilled"],
        ["registry_inactive", "RegistryInactive"],
        ["registry_invalid", "RegistryInvalid"],
      ],
    );
    assert.match(
      rust,
      /pub enum ForecastAdjustmentTemperatureCanaryRuntimeBundle \{\s*TimeLimited\(ForecastAdjustmentTemperatureCanaryRuntimeBundleV1\),\s*Permanent\(ForecastAdjustmentTemperatureCanaryRuntimeBundleV2\),\s*\}/u,
    );
    assert.match(
      rust,
      /pub enum LoadedForecastAdjustmentTemperatureCanaryRuntimeV1 \{\s*#\[doc = "wire discriminant state: \\"active\\""\]\s*Active\(LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Active\),\s*#\[doc = "wire discriminant state: \\"disabled\\""\]\s*Disabled\(LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Disabled\),\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentitySourceDelayHoursLiteral \{\s*pub const WIRE_VALUE: u64 = 6;\s*\}/u,
    );
    assert.match(
      rust,
      /wire rule: unsigned-safe-integer-literal, one canonical decimal value in 0\.\.=9007199254740991"\]\s*pub sourceDelayHours: ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentitySourceDelayHoursLiteral,/u,
    );
    assert.match(
      rust,
      /wire rule: readonly-inline-object, pinned shallow Readonly wrapper over explicit properties"\]\s*pub maintenanceAuthority: Nullable<ForecastAdjustmentTemperatureMaintenanceMaterialMaintenanceAuthority>,/u,
    );
    assert.match(
      rust,
      /field projection: interface-field -> ForecastAdjustmentTemperatureCanaryRuntimeBundleV1"\]\s*pub servedForecastIdentity: ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentity,/u,
    );
    assert.match(
      rust,
      /field projection: interface-field -> ForecastAdjustmentTemperatureCanaryRuntimeBundleV1"\]\s*pub trainingForecastIdentity: ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentity,/u,
    );
    assert.equal(
      rust.match(/pub struct ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentity \{/gu)?.length,
      1,
    );
    assert.equal(
      rust.match(/pub struct ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentity \{/gu)?.length,
      1,
    );

    const authorizationV1Definition = rust.match(
      /^pub struct ForecastAdjustmentTemperatureCanaryAuthorizationV1 \{\n[\s\S]+?^\}/mu,
    );
    const authorizationV2Definition = rust.match(
      /^pub struct ForecastAdjustmentTemperatureCanaryAuthorizationV2 \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(authorizationV1Definition);
    assert.ok(authorizationV2Definition);
    assert.equal(authorizationV1Definition[0].match(/wire rule: utc-iso/gu)?.length, 3);
    assert.equal(authorizationV2Definition[0].match(/wire rule: utc-iso/gu)?.length, 2);
    assert.match(
      rust,
      /impl ForecastAdjustmentTemperatureCanaryAuthorizationV1AuthorizedLiteral \{\s*pub const WIRE_VALUE: bool = true;\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentTemperatureCanaryAuthorizationV2AuthorizedLiteral \{\s*pub const WIRE_VALUE: bool = true;\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentTemperatureCanaryAuthorizationV2PermanentLiteral \{\s*pub const WIRE_VALUE: bool = true;\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentTemperatureCanaryAuthorizationV2ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-temperature-canary-authorization\/v2";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-temperature-canary-bundle\/v1";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentTemperatureCanaryRuntimeBundleV2ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-temperature-canary-bundle\/v2";\s*\}/u,
    );
    assert.match(
      rust,
      /impl ForecastAdjustmentTemperatureCanaryRegistryV1ContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "forecast-adjustment-temperature-canary-registry\/v1";\s*\}/u,
    );

    assertManifestRootGroup(manifest.roots, TEMPERATURE_CANARY_RUNTIME_ROOTS);
  },
);

test(
  "projects temperature inference results and the canary call input",
  // distinguish validated finite outputs from raw runtime-number preservation
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(
      rustStructFieldEntries(rust, "TemperatureMosRuntimeAppliedResult"),
      [
        ["alpha", "f64"],
        ["applied", "TemperatureMosRuntimeAppliedResultAppliedLiteral"],
        ["branch", "TemperatureMosRuntimeAppliedResultBranchEnum"],
        ["incumbentHalfStrengthTemperatureC", "f64"],
        ["predictionTemperatureC", "f64"],
        ["rawTemperatureC", "f64"],
        ["reason", "TemperatureMosRuntimeAppliedResultReasonNull"],
        ["unscaledCorrectionC", "f64"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "TemperatureMosRuntimeRawResult"),
      [
        ["applied", "TemperatureMosRuntimeRawResultAppliedLiteral"],
        ["predictionTemperatureC", "f64"],
        ["rawTemperatureC", "f64"],
        ["reason", "TemperatureMosRuntimeRawReason"],
      ],
    );
    assert.deepEqual(
      rustStructFieldEntries(rust, "ApplyTemperatureCanaryInputV1"),
      [
        ["evaluatedAt", "String"],
        ["rawBestMatchTemperatureC", "Nullable<f64>"],
        [
          "recentErrorState",
          "Nullable<TemperatureMosRuntimeRecentErrorState>",
        ],
        ["sourceForecast", "Nullable<TemperatureCanarySourceForecast>"],
        ["validAt", "String"],
      ],
    );
    assert.deepEqual(
      rustStringEnumWireEntries(
        rust,
        "TemperatureMosRuntimeAppliedResultBranchEnum",
      ),
      [
        ["adaptive", "Adaptive"],
        ["direct", "Direct"],
      ],
    );
    assert.match(
      rust,
      /pub enum TemperatureMosRuntimeResult \{\s*Applied\(TemperatureMosRuntimeAppliedResult\),\s*Raw\(TemperatureMosRuntimeRawResult\),\s*\}/u,
    );
    assert.match(
      rust,
      /impl TemperatureMosRuntimeAppliedResultAppliedLiteral \{\s*pub const WIRE_VALUE: bool = true;\s*\}/u,
    );
    assert.match(
      rust,
      /impl TemperatureMosRuntimeRawResultAppliedLiteral \{\s*pub const WIRE_VALUE: bool = false;\s*\}/u,
    );
    const appliedDefinition = rust.match(
      /^pub struct TemperatureMosRuntimeAppliedResult \{\n[\s\S]+?^\}/mu,
    );
    const rawDefinition = rust.match(
      /^pub struct TemperatureMosRuntimeRawResult \{\n[\s\S]+?^\}/mu,
    );
    const inputDefinition = rust.match(
      /^pub struct ApplyTemperatureCanaryInputV1 \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(appliedDefinition);
    assert.ok(rawDefinition);
    assert.ok(inputDefinition);
    assert.equal(
      appliedDefinition[0].match(/wire rule: finite-f64/gu)?.length,
      5,
    );
    assert.equal(
      rawDefinition[0].match(
        /storage rule: runtime-f64, JavaScript number including non-finite values; not a JSON wire type/gu,
      )?.length,
      2,
    );
    assert.equal(
      inputDefinition[0].match(
        /storage rule: runtime-f64, JavaScript number including non-finite values; not a JSON wire type/gu,
      )?.length,
      1,
    );
    assert.doesNotMatch(inputDefinition[0], /wire rule: utc-iso/u);
    assert.match(
      appliedDefinition[0],
      /pub reason: TemperatureMosRuntimeAppliedResultReasonNull,/u,
    );

    assertManifestRootGroup(manifest.roots, TEMPERATURE_INFERENCE_RESULT_ROOTS);
  },
);

test(
  "flattens one selected private interface only as an exported ordinary base",
  // retain the private base fields without widening private declaration support
  () => {
    const source = `interface Material { readonly value: string; }
export interface Root extends Material { readonly enabled: boolean; }
`;
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        {
          source: "fixture.ts",
          exportName: "Material",
          localInterface: true,
          fields: {},
        },
        {
          source: "fixture.ts",
          exportName: "Root",
          base: "Material",
          fields: {},
        },
      ],
    };
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Material"), [
      ["value", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["value", "String"],
      ["enabled", "bool"],
    ]);
    assert.deepEqual(manifest.roots, [
      {
        source: "fixture.ts",
        exportName: "Material",
        rustName: "Material",
        kind: "readonly-interface",
        localInterface: true,
      },
      {
        source: "fixture.ts",
        exportName: "Root",
        rustName: "Root",
        kind: "readonly-interface",
        base: "Material",
      },
    ]);
  },
);

test(
  "renames selected quoted range fields while retaining their wire names",
  // render only configured nested wire names and document both original keys
  () => {
    const source = `export interface Band { readonly supported: boolean; }
interface Material {
  readonly strengthBands: {
    readonly "1-6": Band;
    readonly "7-12": Band;
  };
}
export interface Root extends Material { readonly month: string; }
`;
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Band", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Material",
          localInterface: true,
          wireFieldNames: {
            "strengthBands.1-6": "hours1To6",
            "strengthBands.7-12": "hours7To12",
          },
          fields: {},
        },
        {
          source: "fixture.ts",
          exportName: "Root",
          base: "Material",
          fields: {},
        },
      ],
    };
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);
    const definition = rust.match(
      /^pub struct MaterialStrengthBands \{\n[\s\S]+?^\}/mu,
    );

    assert.ok(definition);
    assert.deepEqual(rustStructFieldEntries(rust, "MaterialStrengthBands"), [
      ["hours1To6", "Band"],
      ["hours7To12", "Band"],
    ]);
    assert.match(
      definition[0],
      /#\[doc = "wire name: 1-6"\]\s*pub hours1To6: Band,\s*#\[doc = "wire name: 7-12"\]\s*pub hours7To12: Band,/u,
    );
    assert.deepEqual(manifest.roots[1], {
      source: "fixture.ts",
      exportName: "Material",
      rustName: "Material",
      kind: "readonly-interface",
      localInterface: true,
      wireFieldNames: {
        "strengthBands.1-6": "hours1To6",
        "strengthBands.7-12": "hours7To12",
      },
    });
  },
);

test(
  "projects exact builtin-frozen const string properties and their direct union",
  // bind literal identities to pinned Object.freeze calls without evaluating them
  () => {
    const source = `const FIRST_POLICY = Object.freeze({
  cohort: "ecmwf_single_run_hindcast",
  minimumC: -100,
  scope: "initialization_first12",
} as const);
const SECOND_POLICY = Object.freeze({
  enabled: true,
  maximumC: 70,
  scope: "assumed_delay6_next12",
} as const);
export interface Root {
  readonly cohort: typeof FIRST_POLICY.cohort;
  readonly scope:
    | typeof FIRST_POLICY.scope
    | typeof SECOND_POLICY.scope;
}
`;
    const config = fixtureConfig("Root", {
      cohort: "frozen-const-string-property",
      scope: "frozen-const-string-property-union",
    });
    const { rust } = generateRustContracts(config, { "fixture.ts": source });
    const definition = rust.match(/^pub struct Root \{\n[\s\S]+?^\}/mu);

    assert.ok(definition);
    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["cohort", "RootCohortLiteral"],
      ["scope", "RootScopeEnum"],
    ]);
    assert.deepEqual(rustStringEnumWireEntries(rust, "RootScopeEnum"), [
      ["initialization_first12", "InitializationFirst12"],
      ["assumed_delay6_next12", "AssumedDelay6Next12"],
    ]);
    assert.match(
      rust,
      /impl RootCohortLiteral \{\s*pub const WIRE_VALUE: &'static str = "ecmwf_single_run_hindcast";\s*\}/u,
    );
    assert.match(
      definition[0],
      /wire rule: frozen-const-string-property, exact builtin-frozen local const string value"\]\s*pub cohort: RootCohortLiteral,/u,
    );
    assert.match(
      definition[0],
      /wire rule: frozen-const-string-property-union, two distinct builtin-frozen local const string values"\]\s*pub scope: RootScopeEnum,/u,
    );
  },
);

test(
  "flattens an exact same-source Omit base in retained and override order",
  // remove only configured base fields before appending exact replacements
  () => {
    const source = `const ROOT_VERSION = "v2" as const;
export interface Base {
  readonly first: string;
  readonly authorization: string;
  readonly contractVersion: "v1";
  readonly last: string;
}
export interface Root
  extends Omit<Base, "authorization" | "contractVersion"> {
  readonly authorization: boolean;
  readonly contractVersion: typeof ROOT_VERSION;
}
`;
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: { contractVersion: "const-string" },
          base: "Base",
          omitBaseFields: ["authorization", "contractVersion"],
        },
      ],
    };
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["first", "String"],
      ["last", "String"],
      ["authorization", "bool"],
      ["contractVersion", "RootContractVersionLiteral"],
    ]);
    assert.match(
      rust,
      /impl RootContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "v2";\s*\}/u,
    );
    assert.deepEqual(manifest.roots[1], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-interface",
      base: "Base",
      omitBaseFields: ["authorization", "contractVersion"],
    });
  },
);

test(
  "accepts only an unannotated direct local const string literal",
  // admit the source catalog form without widening the local declaration grammar
  () => {
    const config = fixtureConfig("Root", {
      contractVersion: "const-string",
    });
    const { rust } = generateRustContracts(config, {
      "fixture.ts": `const VERSION = "v1";
export interface Root { readonly contractVersion: typeof VERSION; }
`,
    });

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["contractVersion", "RootContractVersionLiteral"],
    ]);
    assert.match(
      rust,
      /impl RootContractVersionLiteral \{\s*pub const WIRE_VALUE: &'static str = "v1";\s*\}/u,
    );

    const invalidDeclarations = [
      [
        "an alias initializer",
        `const BASE_VERSION = "v1";
const VERSION = BASE_VERSION;`,
      ],
      ["a type annotation", `const VERSION: string = "v1";`],
      ["a let binding", `let VERSION = "v1";`],
      ["an interpolated template", 'const VERSION = `v${1}`;'],
      ["a widened assertion", `const VERSION = "v1" as string;`],
    ];

    // reject every syntax that can widen or redirect the direct literal identity
    for (const [name, declaration] of invalidDeclarations) {
      assert.throws(
        // exercise the same exact field query for each local declaration form
        () => generateRustContracts(config, {
          "fixture.ts": `${declaration}
export interface Root { readonly contractVersion: typeof VERSION; }
`,
        }),
        /const string source unsupported at root Root/,
        name,
      );
    }
  },
);

test(
  "reuses flattened base fields across forward source and config order",
  // preserve inherited mappings and nominal nested identities exactly once
  () => {
    const source = `export interface Child extends Base { readonly optional?: string; }
export interface Base {
  readonly count: number;
  readonly details: { readonly label: string };
  readonly nullable: string | null;
}
`;
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Child", base: "Base", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Base",
          fields: { count: "safe-unsigned-integer" },
        },
      ],
    };
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Child"), [
      ["count", "u64"],
      ["details", "BaseDetails"],
      ["nullable", "Nullable<String>"],
      ["optional", "FieldPresence<String>"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "Child").slice(0, -1),
      rustStructFieldEntries(rust, "Base"),
    );
    assert.equal(rust.match(/pub struct BaseDetails \{/gu)?.length, 1);
    assert.match(
      rust,
      /pub struct Child \{\s*#\[doc = "wire rule: safe-unsigned-integer, range 0\.\.=9007199254740991"\]\s*pub count: u64,/,
    );
    assert.deepEqual(manifest.roots[0], {
      source: "fixture.ts",
      exportName: "Child",
      rustName: "Child",
      kind: "readonly-interface",
      base: "Base",
    });
  },
);

test(
  "flattens a checked same-source interface chain in ancestor order",
  // retain each ancestor exactly once before the final child field
  () => {
    const source = `export interface Base { readonly baseValue: string; }
export interface Middle extends Base { readonly middleValue: string; }
export interface Child extends Middle { readonly childValue: string; }
`;
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Child", base: "Middle", fields: {} },
        { source: "fixture.ts", exportName: "Middle", base: "Base", fields: {} },
        { source: "fixture.ts", exportName: "Base", fields: {} },
      ],
    };
    const { rust } = generateRustContracts(config, { "fixture.ts": source });

    assert.deepEqual(rustStructFieldEntries(rust, "Child"), [
      ["baseValue", "String"],
      ["middleValue", "String"],
      ["childValue", "String"],
    ]);
  },
);

test(
  "rejects a configured imported interface as a flat base",
  // keep flat inheritance inside one exact source module
  () => {
    const config = importedFixtureConfig();
    config.roots[1].base = "Child";
    const sources = importedFixtureSources(
      "fixture.ts",
      'import type { Child } from "@fixture/domain";',
      "",
    );
    sources["fixture.ts"] = sources["fixture.ts"].replace(
      "export interface Root {  }",
      "export interface Root extends Child {}",
    );

    assert.throws(
      // reject a cross-module base even when its import is otherwise proven
      () => generateRustContracts(config, sources),
      /configured interface base identity unsupported at root Root/,
    );
  },
);

test(
  "rejects a recursive configured interface base chain",
  // reuse the generated dependency graph for inheritance cycles
  () => {
    const source = `export interface A extends B { readonly a: string; }
export interface B extends A { readonly b: string; }
`;
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "A", base: "B", fields: {} },
        { source: "fixture.ts", exportName: "B", base: "A", fields: {} },
      ],
    };

    assert.throws(
      // reject before either recursive shape is emitted
      () => generateRustContracts(config, { "fixture.ts": source }),
      /recursive configured type reference cycle: A -> B -> A/,
    );
  },
);

test(
  "emits one ordered finite record helper for required and optional fields",
  // preserve field order and missing independently from the record contents
  () => {
    const source = `export interface Root {
  readonly first: Readonly<Record<string, number>>;
  readonly optional?: Readonly<Record<string, number>>;
  readonly second: Readonly<Record<string, number>>;
}
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      fixtureConfig("Root", {
        first: "ordered-finite-f64-record",
        optional: "ordered-finite-f64-record",
        second: "ordered-finite-f64-record",
      }),
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.equal(
      rust.match(/pub struct OrderedFiniteNumberRecord\(pub Vec<\(String, f64\)>\);/gu)?.length,
      1,
    );
    assert.match(rust, /pub first: OrderedFiniteNumberRecord,/);
    assert.match(rust, /pub optional: FieldPresence<OrderedFiniteNumberRecord>,/);
    assert.match(rust, /pub second: OrderedFiniteNumberRecord,/);
    assert.equal(
      manifest.generator.utilityDeclarationsSha256,
      "bcd24271a113971ba9eb71ff8cb01bc6b0f872a85c23fdbe5d93065b375933cd",
    );
  },
);

test(
  "shares one checker between configured references and pinned utility identities",
  // prove both supported symbolic forms retain their exact declaration identity
  () => {
    const source = `export interface Child { readonly value: string; }
export interface Root {
  readonly child: Child;
  readonly readings: Readonly<Record<string, number>>;
}
`;
    const config = fixtureRoots("Child", "Root");
    config.roots[1].fields.readings = "ordered-finite-f64-record";
    const { rust } = generateRustContracts(config, { "fixture.ts": source });

    assert.match(rust, /pub child: Child,/);
    assert.match(rust, /pub readings: OrderedFiniteNumberRecord,/);
  },
);

test(
  "reverifies the library bytes across calls and isolates per-source checker ASTs",
  // keep a matching manifest digest tied to the current opt-in input
  () => {
    const config = fixtureConfig("First", { readings: "ordered-finite-f64-record" });
    config.roots.push({
      source: "second.ts",
      exportName: "Second",
      fields: { readings: "ordered-finite-f64-record" },
    });
    const sources = {
      "fixture.ts": "export interface First { readonly readings: Readonly<Record<string, number>>; }\n",
      "second.ts": "export interface Second { readonly readings: Readonly<Record<string, number>>; }\n",
    };
    let reads = 0;

    // leave the installed dependency untouched while simulating changed input
    function readUtilityFile(path) {
      reads += 1;
      return reads === 1 ? readFileSync(path) : Buffer.from("different utility declarations\n");
    }

    const { rust } = generateRustContracts(config, sources, readUtilityFile);
    assert.match(rust, /pub struct First \{[\s\S]*?pub readings: OrderedFiniteNumberRecord,/);
    assert.match(rust, /pub struct Second \{[\s\S]*?pub readings: OrderedFiniteNumberRecord,/);
    assert.equal(reads, 1);
    assert.throws(
      // reject changed library input on a later call in the same process
      () => generateRustContracts(config, sources, readUtilityFile),
      /TypeScript utility declarations mismatch/,
    );
    assert.equal(reads, 2);
  },
);

test(
  "resolves one configured type-only package import beside same-module references",
  // preserve both exact declaration identities in one consumer checker
  () => {
    const config = importedFixtureConfig();
    config.roots.splice(1, 0, {
      source: "fixture.ts",
      exportName: "Local",
      fields: {},
    });
    const sources = importedFixtureSources(
      "fixture.ts",
      'import type { Child } from "@fixture/domain";',
      "readonly child: Child; readonly local: Local;",
    );
    sources["fixture.ts"] = sources["fixture.ts"].replace(
      "export interface Root",
      "export interface Local { readonly name: string; }\nexport interface Root",
    );
    const { rust, manifest: manifestText } = generateRustContracts(config, sources);
    const manifest = JSON.parse(manifestText);

    assert.match(rust, /pub struct Root \{\s*pub child: Child,\s*pub local: Local,\s*\}/);
    assert.deepEqual(manifest.typeImports, [{
      consumer: "fixture.ts",
      specifier: "@fixture/domain",
      packageJson: "fixture-domain/package.json",
      entry: "fixture-domain/src/index.ts",
      leaves: ["fixture-domain/src/types.ts"],
      exports: { Child: "fixture-domain/src/types.ts" },
    }]);
  },
);

test(
  "resolves a renamed specifier-level type import in a mixed import",
  // use local spelling only at the consumer while ignoring unrelated runtime bindings
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources(
      "fixture.ts",
      'import { runtimeValue, type Child as LocalChild } from "@fixture/domain";',
      "readonly child: LocalChild;",
    );
    sources["fixture-domain/src/types.ts"] += "export const runtimeValue = 1;\n";
    const { rust } = generateRustContracts(config, sources);
    assert.match(rust, /pub struct Root \{\s*pub child: Child,\s*\}/);
  },
);

test(
  "rejects a value import at a configured type-only edge",
  // require explicit type-only syntax at the consumer boundary
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources(
      "fixture.ts",
      'import { Child } from "@fixture/domain";',
    );
    assert.throws(
      // reject the value-bearing import before emitting Rust
      () => generateRustContracts(config, sources),
      /configured type import identity unsupported at Root\.child/,
    );
  },
);

test(
  "rejects a namespace import at a configured type-only edge",
  // avoid widening one reviewed named export into a package namespace
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources(
      "fixture.ts",
      'import type * as Domain from "@fixture/domain";',
      "readonly child: Domain.Child;",
    );
    assert.throws(
      // reject namespace lookup instead of inferring one export
      () => generateRustContracts(config, sources),
      /configured type (?:import|reference) .*unsupported at Root\.child/,
    );
  },
);

test(
  "rejects an imported package name absent from the configured exports",
  // do not traverse package exports beyond the explicit import inventory
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources(
      "fixture.ts",
      'import type { Other } from "@fixture/domain";',
      "readonly child: Other;",
    );
    sources["fixture-domain/src/types.ts"] +=
      "export interface Other { readonly value: string; }\n";
    assert.throws(
      // reject the package symbol that is absent from configuration
      () => generateRustContracts(config, sources),
      /configured type import identity unsupported at Root\.child/,
    );
  },
);

test(
  "rejects a configured type export that no root field consumes",
  // prevent stale package proof rows from accumulating silently
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources(
      "fixture.ts",
      'import type { Child } from "@fixture/domain";',
      "readonly value: string;",
    );
    assert.throws(
      // reject the configured export even though its import syntax is valid
      () => generateRustContracts(config, sources),
      /unused configured type import @fixture\/domain#Child for fixture\.ts/,
    );
  },
);

test(
  "rejects duplicate package exports across configured leaves",
  // prove one exported symbol has exactly one package declaration identity
  () => {
    const config = importedFixtureConfig();
    config.typeImports[0].leaves.push("fixture-domain/src/duplicate.ts");
    const sources = importedFixtureSources();
    sources["fixture-domain/src/index.ts"] += "export * from \"./duplicate.js\";\n";
    sources["fixture-domain/src/duplicate.ts"] =
      "export interface Child { readonly duplicate: string; }\n";
    assert.throws(
      // reject the ambiguous package export identity
      () => generateRustContracts(config, sources),
      /type import export identity unsupported for Child from @fixture\/domain/,
    );
  },
);

test(
  "resolves a package through a type-only export-star entry",
  // keep a types-only package barrel absent from emitted runtime graphs
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources();
    sources["fixture-domain/src/index.ts"] = 'export type * from "./types.js";\n';
    const { rust } = generateRustContracts(config, sources);
    assert.match(rust, /pub struct Root \{\s*pub child: Child,\s*\}/);
  },
);

test(
  "rejects package metadata whose direct types entry is mismatched",
  // keep the package-level declaration entry bound to the configured barrel
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources();
    const metadata = JSON.parse(sources["fixture-domain/package.json"]);
    metadata.types = "./src/other.ts";
    sources["fixture-domain/package.json"] = JSON.stringify(metadata);
    assert.throws(
      // reject a direct types entry that points outside the configured graph
      () => generateRustContracts(config, sources),
      /type import package metadata unsupported at @fixture\/domain/,
    );
  },
);

test(
  "rejects package export metadata that does not bind the configured type entry",
  // keep the conditional types target bound to the configured barrel
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources();
    const metadata = JSON.parse(sources["fixture-domain/package.json"]);
    metadata.exports["."].types = "./src/other.ts";
    sources["fixture-domain/package.json"] = JSON.stringify(metadata);
    assert.throws(
      // reject metadata whose declared types entry does not match
      () => generateRustContracts(config, sources),
      /type import package metadata unsupported at @fixture\/domain/,
    );
  },
);

test(
  "rejects a package entry containing behavior beyond direct star exports",
  // keep the proof barrel one-hop and declaration-only
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources();
    sources["fixture-domain/src/index.ts"] += "export const runtimeValue = 1;\n";
    assert.throws(
      // reject executable or additional entry statements
      () => generateRustContracts(config, sources),
      /type import entry unsupported at fixture-domain\/src\/index\.ts/,
    );
  },
);

test(
  "rejects an extra configured leaf absent from the package entry",
  // bind the configured leaf inventory to the entry's complete direct exports
  () => {
    const config = importedFixtureConfig();
    config.typeImports[0].leaves.push("fixture-domain/src/extra.ts");
    const sources = importedFixtureSources();
    sources["fixture-domain/src/extra.ts"] =
      "export interface Extra { readonly value: string; }\n";
    assert.throws(
      // reject an unexported trailing leaf
      () => generateRustContracts(config, sources),
      /type import leaves mismatch at fixture-domain\/src\/index\.ts/,
    );
  },
);

test(
  "rejects a script file inside the configured package graph",
  // require every proof participant to have an isolated external-module scope
  () => {
    const config = importedFixtureConfig();
    config.typeImports[0].leaves.push("fixture-domain/src/script.ts");
    const sources = importedFixtureSources();
    sources["fixture-domain/src/index.ts"] += "export * from \"./script.js\";\n";
    sources["fixture-domain/src/script.ts"] = "const localOnly = 1;\n";
    assert.throws(
      // reject global script scope even when an entry names it
      () => generateRustContracts(config, sources),
      /type import graph source unsupported at fixture-domain\/src\/script\.ts/,
    );
  },
);

test(
  "rejects a global augmentation inside a configured package leaf",
  // keep all import proof declarations out of the ambient global scope
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources();
    sources["fixture-domain/src/types.ts"] +=
      "declare global { interface Injected { readonly value: string; } }\n";
    assert.throws(
      // reject an ambient contribution despite the leaf remaining a module
      () => generateRustContracts(config, sources),
      /type import graph source unsupported at fixture-domain\/src\/types\.ts/,
    );
  },
);

test(
  "rejects an omitted type-import proof input",
  // require every configured package leaf as exact source evidence
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources();
    delete sources["fixture-domain/src/types.ts"];
    assert.throws(
      // reject an incomplete proof input set
      () => generateRustContracts(config, sources),
      /sources must contain exactly the selected config source paths/,
    );
  },
);

test(
  "rejects a re-export from a configured package leaf",
  // stop the package proof graph after its explicitly inventoried first hop
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources();
    sources["fixture-domain/src/types.ts"] += "export * from \"./deeper.js\";\n";
    assert.throws(
      // reject arbitrary traversal beyond the configured leaves
      () => generateRustContracts(config, sources),
      /type import leaf re-export unsupported at fixture-domain\/src\/types\.ts/,
    );
  },
);

test(
  "rejects a consumer module augmentation beside a configured type import",
  // prevent the consumer from changing the reviewed package export identity
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources();
    sources["fixture.ts"] = sources["fixture.ts"].replace(
      "export interface Root",
      'declare module "@fixture/domain" { interface Injected {} }\nexport interface Root',
    );
    assert.throws(
      // reject a source-owned change to package declaration identity
      () => generateRustContracts(config, sources),
      /type import module augmentation unsupported at fixture\.ts/,
    );
  },
);

test(
  "rejects a module augmentation in the configured package entry",
  // keep the proof barrel from changing any external package surface
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources();
    sources["fixture-domain/src/index.ts"] +=
      'declare module "@fixture/other" { interface Injected {} }\n';
    assert.throws(
      // reject augmentation before interpreting barrel exports
      () => generateRustContracts(config, sources),
      /type import module augmentation unsupported at fixture-domain\/src\/index\.ts/,
    );
  },
);

test(
  "rejects a module augmentation in a configured package leaf",
  // keep declaration owners from changing any external package surface
  () => {
    const config = importedFixtureConfig();
    const sources = importedFixtureSources();
    sources["fixture-domain/src/types.ts"] +=
      'declare module "@fixture/other" { interface Injected {} }\n';
    assert.throws(
      // reject augmentation before proving leaf exports
      () => generateRustContracts(config, sources),
      /type import module augmentation unsupported at fixture-domain\/src\/types\.ts/,
    );
  },
);

test(
  "rejects a recursive configured type cycle across two package imports",
  // include cross-module edges in the existing nominal dependency graph
  () => {
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "a/src/types.ts", exportName: "A", fields: {} },
        { source: "b/src/types.ts", exportName: "B", fields: {} },
      ],
      typeImports: [
        {
          consumer: "a/src/types.ts",
          specifier: "@fixture/b",
          packageJson: "b/package.json",
          entry: "b/src/index.ts",
          leaves: ["b/src/types.ts"],
          exports: { B: "b/src/types.ts" },
        },
        {
          consumer: "b/src/types.ts",
          specifier: "@fixture/a",
          packageJson: "a/package.json",
          entry: "a/src/index.ts",
          leaves: ["a/src/types.ts"],
          exports: { A: "a/src/types.ts" },
        },
      ],
    };
    // build one exact fixture package manifest
    const packageMetadata = (name) => JSON.stringify({
      name,
      private: true,
      type: "module",
      types: "./src/index.ts",
      exports: {
        ".": {
          types: "./src/index.ts",
          default: "./dist/index.js",
        },
      },
    });
    const sources = {
      "a/package.json": packageMetadata("@fixture/a"),
      "a/src/index.ts": "export * from \"./types.js\";\n",
      "a/src/types.ts": 'import type { B } from "@fixture/b";\nexport interface A { readonly b: B; }\n',
      "b/package.json": packageMetadata("@fixture/b"),
      "b/src/index.ts": "export * from \"./types.js\";\n",
      "b/src/types.ts": 'import type { A } from "@fixture/a";\nexport interface B { readonly a: A; }\n',
    };
    assert.throws(
      // reject the closed dependency cycle after resolving both imports
      () => generateRustContracts(config, sources),
      /recursive configured type reference cycle: A -> B -> A/,
    );
  },
);

test(
  "omits the utility declaration identity when no field opts in",
  // retain the earlier manifest shape for unrelated roots
  () => {
    const { manifest: manifestText } = generateRustContracts(
      fixtureConfig("Root"),
      { "fixture.ts": "export interface Root { readonly value: string; }\n" },
    );
    const manifest = JSON.parse(manifestText);
    assert.deepEqual(manifest.generator, {
      typescriptVersion: "6.0.3",
      subsetVersion: "weather-ts-rust-contract-subset/v55",
    });
    assert.equal("typeImports" in manifest, false);
  },
);

test(
  "emits stable bytes independent of metadata insertion order",
  // compare semantically identical inputs
  () => {
    const reorderedConfig = {
      roots: CONFIG.roots.map(
        // reorder only non-semantic field metadata
        (root) => ({
          fields: Object.fromEntries(Object.entries(root.fields).reverse()),
          exportName: root.exportName,
          source: root.source,
          // preserve optional flattened inheritance metadata
          ...(root.base === undefined ? {} : { base: root.base }),
          // preserve the exact ordinary imported-base opt-in
          ...(root.importedBase === undefined
            ? {}
            : { importedBase: root.importedBase }),
          // preserve exact imported string fields folded into their base positions
          ...(root.identicalImportedStringFields === undefined
            ? {}
            : {
              identicalImportedStringFields:
                root.identicalImportedStringFields,
            }),
          // preserve optional exact omitted-base field order
          ...(root.omitBaseFields === undefined
            ? {}
            : { omitBaseFields: root.omitBaseFields }),
          // preserve the exact ordered fields appended after omitted replacements
          ...(root.addedAfterOmit === undefined
            ? {}
            : { addedAfterOmit: root.addedAfterOmit }),
          // preserve the complete direct own-field order after one omission
          ...(root.ownFieldOrderAfterOmit === undefined
            ? {}
            : { ownFieldOrderAfterOmit: root.ownFieldOrderAfterOmit }),
          // preserve the ordered inherited omission proof
          ...(root.omitInheritedBaseFields === undefined
            ? {}
            : { omitInheritedBaseFields: root.omitInheritedBaseFields }),
          // preserve exact pick field order while reordering its record keys
          ...(root.pick === undefined
            ? {}
            : { pick: Object.fromEntries(Object.entries(root.pick).reverse()) }),
          // preserve optional tagged-union metadata
          ...(root.discriminator === undefined
            ? {}
            : { discriminator: root.discriminator }),
          // preserve the explicit readonly-wrapped tagged-union opt-in
          ...(root.readonlyTaggedUnion === undefined
            ? {}
            : { readonlyTaggedUnion: root.readonlyTaggedUnion }),
          // preserve optional literal catalog metadata
          ...(root.catalog === undefined ? {} : { catalog: root.catalog }),
          // preserve optional named object-union metadata
          ...(root.objectUnion === undefined
            ? {}
            : { objectUnion: Object.fromEntries(
              Object.entries(root.objectUnion).reverse(),
            ) }),
          // preserve the explicit public imported-union opt-in
          ...(root.importedObjectUnion === undefined
            ? {}
            : { importedObjectUnion: root.importedObjectUnion }),
          // preserve optional literal-union projection metadata
          ...(root.literalUnion === undefined
            ? {}
            : { literalUnion: Object.fromEntries(
              Object.entries(root.literalUnion).reverse(),
            ) }),
          // preserve the explicit private-alias opt-in
          ...(root.localAlias === undefined ? {} : { localAlias: root.localAlias }),
          // preserve the explicit private-interface opt-in
          ...(root.localInterface === undefined
            ? {}
            : { localInterface: root.localInterface }),
          // reorder only non-semantic nested wire-name metadata
          ...(root.wireFieldNames === undefined
            ? {}
            : { wireFieldNames: Object.fromEntries(
              Object.entries(root.wireFieldNames).reverse(),
            ) }),
          // preserve optional recursive json metadata
          ...(root.recursiveJson === undefined
            ? {}
            : { recursiveJson: root.recursiveJson }),
          // reorder only non-semantic projection metadata
          ...(root.fieldProjections === undefined
            ? {}
            : { fieldProjections: Object.fromEntries(
              Object.entries(root.fieldProjections).reverse().map(
                // reverse one projection record without changing its meaning
                ([fieldName, projection]) => [
                  fieldName,
                  Object.fromEntries(Object.entries(projection).reverse()),
                ],
              ),
            ) }),
          // preserve the exact defaulted list-parameter proof
          ...(root.defaultedListParameter === undefined
            ? {}
            : { defaultedListParameter: root.defaultedListParameter }),
          // preserve the explicit projection of named pg row fields
          ...(root.pgRowNamedFields === undefined
            ? {}
            : { pgRowNamedFields: root.pgRowNamedFields }),
          // preserve exact private PostgreSQL query owners in source order
          ...(root.pgQueryOwners === undefined
            ? {}
            : { pgQueryOwners: root.pgQueryOwners }),
          // preserve the explicit required nullable metric-record root opt-in
          ...(root.readonlyNullableMetricRecord === undefined
            ? {}
            : {
              readonlyNullableMetricRecord:
                root.readonlyNullableMetricRecord,
            }),
          // preserve the exact disabled loader reason enum opt-in
          ...(root.disabledReasonExtract === undefined
            ? {}
            : { disabledReasonExtract: root.disabledReasonExtract }),
          // preserve the exact active imported const-string field selector
          ...(root.activeImportedConstString === undefined
            ? {}
            : {
              activeImportedConstString:
                root.activeImportedConstString,
            }),
          // reorder the maintenance loader extension metadata record only
          ...(root.maintenanceLoaderExtension === undefined
            ? {}
            : { maintenanceLoaderExtension: Object.fromEntries(
              Object.entries(root.maintenanceLoaderExtension).reverse(),
            ) }),
          // reorder only non-semantic indexed alias-source metadata
          ...(root.indexedLiteralSources === undefined
            ? {}
            : { indexedLiteralSources: Object.fromEntries(
              Object.entries(root.indexedLiteralSources).reverse(),
            ) }),
        }),
      ),
      typeImports: CONFIG.typeImports,
      typescriptVersion: CONFIG.typescriptVersion,
      version: CONFIG.version,
    };
    const first = generateRustContracts(CONFIG, SOURCES);
    const repeated = generateRustContracts(CONFIG, SOURCES);
    const reordered = generateRustContracts(reorderedConfig, SOURCES);

    assert.deepEqual(repeated, first);
    assert.deepEqual(reordered, first);
    assert.deepEqual(repositoryProjection(), first);
  },
);

test(
  "checks generated contracts byte for byte without rewriting outputs",
  // exercise the real cli with isolated inputs and exact bytes
  () => {
    const scratch = mkdtempSync(join(tmpdir(), "weather-rust-contract-cli-"));
    const config = "config/rust-contract-roots.json";
    const source = "contract-�.ts";
    const rust = "rust/crates/weather-contracts/src/generated.rs";
    const manifest = "rust/crates/weather-contracts/generated.contracts.json";
    const outputs = [rust, manifest];

    // resolve only paths in this disposable fixture
    const fixturePath = (relative) => join(scratch, relative);
    // read exact contents and modification clocks for both artifacts
    const snapshot = () => outputs.map(
      // retain bytes and mtime independently from the cli report
      (output) => ({
        bytes: readFileSync(fixturePath(output)),
        mtime: statSync(fixturePath(output), { bigint: true }).mtimeNs,
      }),
    );
    // run one child to completion with a fixed timeout and no shell
    const runCli = (mode) => {
      const result = spawnSync(
        process.execPath,
        ["--max-old-space-size=128", "scripts/generate-rust-contracts.mjs", mode],
        {
          cwd: scratch,
          encoding: "utf8",
          killSignal: "SIGKILL",
          maxBuffer: 128 * 1024,
          timeout: 3_500,
        },
      );
      assert.equal(result.error, undefined);
      assert.equal(result.signal, null);
      return result;
    };
    // make equal-text but unequal-bytes input by breaking one utf-8 sequence
    const invalidUtf8 = (valid) => {
      const replacement = Buffer.from("�");
      const offset = valid.indexOf(replacement);
      assert.notEqual(offset, -1);
      const invalid = Buffer.concat([
        valid.subarray(0, offset),
        Buffer.from([0xff]),
        valid.subarray(offset + replacement.length),
      ]);
      assert.equal(invalid.toString("utf8"), valid.toString("utf8"));
      assert.notDeepEqual(invalid, valid);
      return invalid;
    };
    // use old distinct mtime sentinels to detect even byte-identical writes
    const markOutputs = () => {
      utimesSync(fixturePath(rust), 946684800, 946684801);
      utimesSync(fixturePath(manifest), 946684800, 946684802);
    };

    try {
      mkdirSync(fixturePath("scripts"));
      mkdirSync(fixturePath("scripts/lib"));
      mkdirSync(fixturePath("config"));
      mkdirSync(fixturePath("node_modules"));
      copyFileSync(
        fileURLToPath(new URL("./generate-rust-contracts.mjs", import.meta.url)),
        fixturePath("scripts/generate-rust-contracts.mjs"),
      );
      copyFileSync(
        fileURLToPath(new URL("./lib/forecast-observation-rust.mjs", import.meta.url)),
        fixturePath("scripts/lib/forecast-observation-rust.mjs"),
      );
      const typescriptEntry = fileURLToPath(import.meta.resolve("typescript"));
      symlinkSync(
        resolve(dirname(typescriptEntry), ".."),
        fixturePath("node_modules/typescript"),
        "junction",
      );
      writeFileSync(fixturePath(config), JSON.stringify({
        version: 1,
        typescriptVersion: CONFIG.typescriptVersion,
        roots: [{ source, exportName: "Contract", fields: {} }],
      }));
      writeFileSync(
        fixturePath(source),
        'export interface Contract { readonly marker: "replacement �"; }\n',
      );

      const first = runCli("--write");
      assert.equal(first.status, 0, first.stderr);
      const firstOutput = snapshot();
      const second = runCli("--write");
      assert.equal(second.status, 0, second.stderr);
      assert.deepEqual(
        // compare output contents without requiring write mode to preserve mtimes
        snapshot().map(({ bytes }) => bytes),
        firstOutput.map(({ bytes }) => bytes),
      );

      markOutputs();
      const before = snapshot();
      const current = runCli("--check");
      assert.equal(current.status, 0, current.stderr);
      assert.equal(current.stdout, "generated Rust contracts are current\n");
      assert.equal(current.stderr, "");
      assert.deepEqual(snapshot(), before);

      const driftCases = [
        [rust, `generated contract drift: ${rust}; run node scripts/generate-rust-contracts.mjs --write`],
        [source, `contract input is not valid UTF-8: ${source}`],
        [config, `contract input is not valid UTF-8: ${config}`],
      ];
      // require exact diagnostic and nonmutation for each byte-only change
      for (const [relative, diagnostic] of driftCases) {
        const input = fixturePath(relative);
        const valid = readFileSync(input);
        const invalid = invalidUtf8(valid);
        writeFileSync(input, invalid);
        markOutputs();
        const changed = snapshot();
        const checked = runCli("--check");
        assert.equal(checked.status, 1, `check accepted byte drift in ${relative}`);
        assert.equal(checked.stdout, "");
        assert.equal(checked.stderr, `generate-rust-contracts: ${diagnostic}\n`);
        assert.deepEqual(snapshot(), changed);
        assert.deepEqual(readFileSync(input), invalid);
        writeFileSync(input, valid);
      }
    } finally {
      // remove the fixture without touching the installed dependency
      rmSync(scratch, { recursive: true, force: true });
    }
  },
);

test(
  "records pinned source, configuration, root, and output identities",
  // validate the generated manifest contract
  () => {
    const { rust, manifest: manifestText } = repositoryProjection();
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(Object.keys(manifest), [
      "schemaVersion",
      "generator",
      "config",
      "sources",
      "roots",
      "typeImports",
      "output",
    ]);
    assert.equal(
      manifest.schemaVersion,
      "weather-rust-contracts-manifest/v1",
    );
    assert.deepEqual(manifest.generator, {
      typescriptVersion: "6.0.3",
      subsetVersion: "weather-ts-rust-contract-subset/v55",
      pgRowMarkerInputSha256: PG_ROW_MARKER_INPUT_SHA256,
      utilityDeclarationsSha256:
        "bcd24271a113971ba9eb71ff8cb01bc6b0f872a85c23fdbe5d93065b375933cd",
    });
    assert.equal(manifest.config.sha256, sha256(canonicalJson(CONFIG)));
    assert.equal(manifest.roots.length, REPOSITORY_ROOT_COUNT);
    assert.equal(Object.keys(SOURCES).length, REPOSITORY_SOURCE_COUNT);
    assert.equal(manifest.typeImports.length, REPOSITORY_IMPORT_COUNT);
    assert.deepEqual(Object.keys(SOURCES).sort(), [
      "apps/api/src/adjustment-maintenance-shadow.ts",
      "apps/api/src/index.ts",
      "apps/api/src/main.ts",
      "apps/api/src/rain-adjustment.ts",
      "apps/web/src/widget-forecast-v2.ts",
      "apps/web/src/widget-forecast-v3.ts",
      "apps/web/src/widget-forecast.ts",
      "apps/worker/src/backfill-cli.ts",
      "apps/worker/src/errors.ts",
      "apps/worker/src/forecast-adjustment-performance-cli.ts",
      "apps/worker/src/health.ts",
      "apps/worker/src/public-stations-backfill-cli.ts",
      "apps/worker/src/rain-collection.ts",
      "apps/worker/src/run-deadline.ts",
      "apps/worker/src/source-identity.ts",
      "apps/worker/src/tempest-backfill-cli.ts",
      "apps/worker/src/tide-backfill-cli.ts",
      "apps/worker/src/worker.ts",
      "packages/contracts/package.json",
      "packages/contracts/src/forecast-archive.ts",
      "packages/contracts/src/forecast.ts",
      "packages/contracts/src/index.ts",
      "packages/contracts/src/precipitation-and-tides.ts",
      "packages/contracts/src/sites.ts",
      "packages/contracts/src/weather-records.ts",
      "packages/database/package.json",
      "packages/database/src/adjustment-maintenance.ts",
      "packages/database/src/config.ts",
      "packages/database/src/ecowitt-config.ts",
      "packages/database/src/index.ts",
      "packages/database/src/migrate.ts",
      "packages/database/src/pool.ts",
      "packages/database/src/public-stations-config.ts",
      "packages/database/src/rain-adjustment.ts",
      "packages/database/src/rain-collection.ts",
      "packages/database/src/repositories.ts",
      "packages/database/src/tempest-config.ts",
      "packages/database/src/tides-config.ts",
      "packages/database/tsconfig.json",
      "packages/domain/package.json",
      "packages/domain/src/forecast-adjustment.ts",
      "packages/domain/src/forecast-anchor-record.ts",
      "packages/domain/src/index.ts",
      "packages/domain/src/ingestion.ts",
      "packages/domain/src/provenance.ts",
      "packages/domain/src/rain-collection.ts",
      "packages/domain/src/weather-record.ts",
      "packages/forecast-adjustment/package.json",
      "packages/forecast-adjustment/src/algorithm-v1.ts",
      "packages/forecast-adjustment/src/apply.ts",
      "packages/forecast-adjustment/src/bootstrap-v1.ts",
      "packages/forecast-adjustment/src/calendar.ts",
      "packages/forecast-adjustment/src/candidate.ts",
      "packages/forecast-adjustment/src/evaluate.ts",
      "packages/forecast-adjustment/src/evidence.ts",
      "packages/forecast-adjustment/src/holdout-ledger.ts",
      "packages/forecast-adjustment/src/index.ts",
      "packages/forecast-adjustment/src/maintenance-capture-epoch.ts",
      "packages/forecast-adjustment/src/maintenance-policy.ts",
      "packages/forecast-adjustment/src/maintenance-revision-projection.ts",
      "packages/forecast-adjustment/src/maintenance-runtime-package.ts",
      "packages/forecast-adjustment/src/maintenance-shadow-catalog.ts",
      "packages/forecast-adjustment/src/maintenance-shadow-comparator.ts",
      "packages/forecast-adjustment/src/maintenance-shadow-values.ts",
      "packages/forecast-adjustment/src/performance-scorecard.ts",
      "packages/forecast-adjustment/src/rain-fixed-gauge-target.ts",
      "packages/forecast-adjustment/src/rain-hurdle-wind-artifact.ts",
      "packages/forecast-adjustment/src/rain-hurdle-wind.ts",
      "packages/forecast-adjustment/src/rain-maintenance-controls.ts",
      "packages/forecast-adjustment/src/rain-runtime-registry.ts",
      "packages/forecast-adjustment/src/runtime-bundle.ts",
      "packages/forecast-adjustment/src/runtime-loader.ts",
      "packages/forecast-adjustment/src/temperature-canary.ts",
      "packages/forecast-adjustment/src/temperature-lead-research.ts",
      "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
      "packages/forecast-adjustment/src/temperature-weather-research.ts",
      "packages/forecast-adjustment/src/wind-canary.ts",
      "packages/providers/src/contract.ts",
      "packages/providers/src/ecowitt.ts",
      "packages/providers/src/noaa-tides.ts",
      "packages/providers/src/open-meteo-previous-runs.ts",
      "packages/providers/src/open-meteo-single-runs.ts",
      "packages/providers/src/open-meteo.ts",
      "packages/providers/src/tempest.ts",
      "tsconfig.base.json",
    ]);
    assert.deepEqual(
      manifest.sources,
      Object.keys(SOURCES).sort().map(
        // project one exact source identity
        (sourcePath) => ({
          path: sourcePath,
          sha256: sha256(SOURCES[sourcePath]),
        }),
      ),
    );
    assert.deepEqual(
      manifest.roots,
      [
        {
          source: "packages/domain/src/rain-collection.ts",
          exportName: "RainCaptureOutcome",
          rustName: "RainCaptureOutcome",
          kind: "string-literal-enum",
        },
        {
          source: "packages/domain/src/rain-collection.ts",
          exportName: "RainCollectionStatus",
          rustName: "RainCollectionStatus",
          kind: "readonly-interface",
        },
        {
          source: "packages/contracts/src/forecast.ts",
          exportName: "ForecastDays",
          rustName: "ForecastDays",
          kind: "unsigned-safe-integer-literal-enum",
        },
        ...[
          "ForecastArchiveProjection",
          "ForecastArchivePageHeaderV1",
          "ArchiveTransferPageEnvelopeV1",
          "ArchiveTransferIdleEnvelopeV1",
          "ForecastArchivePageAcknowledgementV1",
          "ForecastArchiveTransferAcknowledgementV1",
          "ForecastArchiveFinalAcknowledgementV1",
          "ForecastArchiveRequiredInputRoot",
          "ForecastArchiveRequiredInputs",
          "ForecastArchiveInputSealV1",
          "ForecastArchiveFinalPage",
          "ForecastArchiveFinalManifestV1",
          "ForecastArchiveFinalManifestV2",
        ].map((name) => ({
          source: "packages/contracts/src/forecast-archive.ts",
          exportName: name,
          rustName: name,
          kind: "readonly-interface",
        })),
        {
          source: "packages/contracts/src/forecast-archive.ts",
          exportName: "ForecastArchiveFinalManifest",
          rustName: "ForecastArchiveFinalManifest",
          kind: "named-readonly-object-union",
          objectUnion: {
            variants: [
              { rustName: "V1", payloadRustName: "ForecastArchiveFinalManifestV1" },
              { rustName: "V2", payloadRustName: "ForecastArchiveFinalManifestV2" },
            ],
          },
        },
        {
          source: "packages/contracts/src/forecast-archive.ts",
          exportName: "ArchiveTransferFinalEnvelopeV1",
          rustName: "ArchiveTransferFinalEnvelopeV1",
          kind: "readonly-interface",
        },
        {
          source: "packages/contracts/src/forecast-archive.ts",
          exportName: "ArchiveTransferEnvelope",
          rustName: "ArchiveTransferEnvelope",
          kind: "named-readonly-object-union",
          objectUnion: {
            variants: [
              { rustName: "Final", payloadRustName: "ArchiveTransferFinalEnvelopeV1" },
              { rustName: "Page", payloadRustName: "ArchiveTransferPageEnvelopeV1" },
              { rustName: "Idle", payloadRustName: "ArchiveTransferIdleEnvelopeV1" },
            ],
          },
        },
        {
          source: "packages/contracts/src/forecast.ts",
          exportName: "ApiRainAdjustmentDecision",
          rustName: "ApiRainAdjustmentDecision",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastObservationQualityRule",
          rustName: "ForecastObservationQualityRule",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastObservationStationKey",
          rustName: "ForecastObservationStationKey",
          kind: "string-literal-enum",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastObservationSourceLineage",
          rustName: "ForecastObservationSourceLineage",
          kind: "readonly-interface",
        },
        {
          source: "packages/forecast-adjustment/src/rain-runtime-registry.ts",
          exportName: "ForecastAdjustmentRainActiveArtifactV1",
          rustName: "ForecastAdjustmentRainActiveArtifactV1",
          kind: "readonly-interface",
        },
        {
          source: "packages/forecast-adjustment/src/rain-runtime-registry.ts",
          exportName: "ForecastAdjustmentRainRuntimeRegistryV1",
          rustName: "ForecastAdjustmentRainRuntimeRegistryV1",
          kind: "tagged-readonly-object-union",
          discriminator: {
            field: "rawReason",
            variants: [
              {
                rustName: "PolicyRaw",
                payloadRustName: "ForecastAdjustmentRainRuntimeRegistryV1PolicyRaw",
                wireValue: "policy_raw",
              },
              {
                rustName: "Null",
                payloadRustName: "ForecastAdjustmentRainRuntimeRegistryV1Null",
                wireValue: null,
              },
            ],
          },
        },
        {
          source: "packages/contracts/src/precipitation-and-tides.ts",
          exportName: "ApiDailyPrecipitation",
          rustName: "ApiDailyPrecipitation",
          kind: "readonly-interface",
        },
        {
          source: "packages/contracts/src/precipitation-and-tides.ts",
          exportName: "ApiTideRecord",
          rustName: "ApiTideRecord",
          kind: "readonly-interface",
        },
        {
          source: "apps/api/src/index.ts",
          exportName: "ApiTrendPoint",
          rustName: "ApiTrendPoint",
          kind: "readonly-interface",
        },
        {
          source: "packages/contracts/src/weather-records.ts",
          exportName: "ApiAirQualityHourlyContext",
          rustName: "ApiAirQualityHourlyContext",
          kind: "readonly-interface",
        },
        {
          source: "packages/contracts/src/weather-records.ts",
          exportName: "ApiPropertySensorSnapshot",
          rustName: "ApiPropertySensorSnapshot",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/provenance.ts",
          exportName: "SourceKind",
          rustName: "SourceKind",
          kind: "string-literal-enum",
        },
        {
          source: "packages/domain/src/provenance.ts",
          exportName: "StationKind",
          rustName: "StationKind",
          kind: "string-literal-enum",
        },
        {
          source: "packages/contracts/src/sites.ts",
          exportName: "ApiSource",
          rustName: "ApiSource",
          kind: "readonly-interface",
        },
        {
          source: "packages/contracts/src/sites.ts",
          exportName: "ApiStation",
          rustName: "ApiStation",
          kind: "readonly-interface",
        },
        {
          source: "packages/contracts/src/sites.ts",
          exportName: "ApiSite",
          rustName: "ApiSite",
          kind: "readonly-interface",
        },
        {
          source: "packages/contracts/src/weather-records.ts",
          exportName: "ApiWeatherRecord",
          rustName: "ApiWeatherRecord",
          kind: "readonly-interface",
        },
        {
          source: "packages/contracts/src/weather-records.ts",
          exportName: "ApiCurrentWeatherRecord",
          rustName: "ApiCurrentWeatherRecord",
          kind: "readonly-interface",
          base: "ApiWeatherRecord",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentMetric",
          rustName: "ForecastAdjustmentMetric",
          kind: "string-literal-enum",
          catalog: { kind: "satisfies-string-tuple" },
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentWindCanaryMetric",
          rustName: "ForecastAdjustmentWindCanaryMetric",
          kind: "string-literal-enum",
          catalog: { kind: "satisfies-string-tuple" },
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastLeadBandKey",
          rustName: "ForecastLeadBandKey",
          kind: "string-literal-enum",
          catalog: { kind: "string-property", key: "key" },
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentReasonCode",
          rustName: "ForecastAdjustmentReasonCode",
          kind: "string-literal-enum",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentRawForecastProvenance",
          rustName: "ForecastAdjustmentRawForecastProvenance",
          kind: "readonly-interface",
        },
        {
          source: "packages/forecast-adjustment/src/temperature-canary.ts",
          exportName: "TemperatureCanarySourceForecast",
          rustName: "TemperatureCanarySourceForecast",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentFailRawDecision",
          rustName: "ForecastAdjustmentFailRawDecision",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentActiveDecision",
          rustName: "ForecastAdjustmentActiveDecision",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentWindCanaryActiveDecision",
          rustName: "ForecastAdjustmentWindCanaryActiveDecision",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentDecision",
          rustName: "ForecastAdjustmentDecision",
          kind: "named-readonly-object-union",
          objectUnion: {
            variants: [
              {
                rustName: "Active",
                payloadRustName: "ForecastAdjustmentActiveDecision",
              },
              {
                rustName: "WindCanaryActive",
                payloadRustName: "ForecastAdjustmentWindCanaryActiveDecision",
              },
              {
                rustName: "FailRaw",
                payloadRustName: "ForecastAdjustmentFailRawDecision",
              },
            ],
          },
        },
        {
          source: "packages/forecast-adjustment/src/temperature-mos-runtime.ts",
          exportName: "TemperatureMosRuntimeRawReason",
          rustName: "TemperatureMosRuntimeRawReason",
          kind: "string-literal-enum",
        },
        {
          source: "packages/forecast-adjustment/src/temperature-canary.ts",
          exportName: "TemperatureCanaryDecisionReason",
          rustName: "TemperatureCanaryDecisionReason",
          kind: "nullable-string-literal-enum",
          literalUnion: {
            sourceAlias: "LoadedForecastAdjustmentTemperatureCanaryRuntimeV1",
            field: "reasonCode",
          },
        },
        {
          source: "packages/forecast-adjustment/src/temperature-canary.ts",
          exportName: "ForecastTemperatureCanaryDecisionV1",
          rustName: "ForecastTemperatureCanaryDecisionV1",
          kind: "readonly-interface",
        },
        {
          source: "packages/forecast-adjustment/src/apply.ts",
          exportName: "ForecastAdjustmentWindQualifiedMaintenanceDecision",
          rustName: "ForecastAdjustmentWindQualifiedMaintenanceDecision",
          kind: "readonly-interface",
          fieldProjections: {
            appliedMetrics: {
              kind: "const-catalog-array",
              root: "ForecastAdjustmentWindCanaryMetric",
            },
            leadBand: {
              kind: "return-enum",
              root: "ForecastLeadBandKey",
            },
          },
        },
        {
          source: "packages/contracts/src/forecast.ts",
          exportName: "ApiForecastAdjustmentDecision",
          rustName: "ApiForecastAdjustmentDecision",
          kind: "named-readonly-object-union",
          importedObjectUnion: true,
          objectUnion: {
            variants: [
              {
                rustName: "Domain",
                payloadRustName: "ForecastAdjustmentDecision",
              },
              {
                rustName: "MaintenanceQualified",
                payloadRustName:
                  "ForecastAdjustmentWindQualifiedMaintenanceDecision",
              },
            ],
          },
        },
        {
          source: "packages/contracts/src/forecast.ts",
          exportName: "ApiForecastWeatherRecord",
          rustName: "ApiForecastWeatherRecord",
          kind: "readonly-interface",
          base: "ApiWeatherRecord",
          importedBase: true,
        },
        {
          source: "packages/contracts/src/forecast.ts",
          exportName: "ApiForecastAdjustmentRuntime",
          rustName: "ApiForecastAdjustmentRuntime",
          kind: "readonly-interface",
        },
        {
          source: "packages/contracts/src/forecast.ts",
          exportName: "ApiForecastTemperatureAdjustmentRuntime",
          rustName: "ApiForecastTemperatureAdjustmentRuntime",
          kind: "readonly-interface",
        },
        {
          source: "packages/contracts/src/forecast.ts",
          exportName: "ApiRainAdjustmentRuntime",
          rustName: "ApiRainAdjustmentRuntime",
          kind: "readonly-interface",
        },
        {
          source: "packages/contracts/src/forecast.ts",
          exportName: "ApiForecastResponse",
          rustName: "ApiForecastResponse",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/ingestion.ts",
          exportName: "BackfillChunkIdentity",
          rustName: "BackfillChunkIdentity",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/ingestion.ts",
          exportName: "BackfillChunkOutcome",
          rustName: "BackfillChunkOutcome",
          kind: "readonly-interface",
          base: "BackfillChunkIdentity",
        },
        {
          source: "packages/domain/src/provenance.ts",
          exportName: "JsonValue",
          rustName: "JsonValue",
          kind: "recursive-json-value",
          recursiveJson: "JsonPrimitive",
        },
        {
          source: "packages/domain/src/weather-record.ts",
          exportName: "CanonicalWeatherMetrics",
          rustName: "CanonicalWeatherMetrics",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/weather-record.ts",
          exportName: "DeviceMetadata",
          rustName: "DeviceMetadata",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/weather-record.ts",
          exportName: "WeatherRecordMetadata",
          rustName: "WeatherRecordMetadata",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/weather-record.ts",
          exportName: "NormalizedWeatherRecord",
          rustName: "NormalizedWeatherRecord",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/weather-record.ts",
          exportName: "NormalizedWeatherRecordInput",
          rustName: "NormalizedWeatherRecordInput",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/ingestion.ts",
          exportName: "IngestionMode",
          rustName: "IngestionMode",
          kind: "string-literal-enum",
        },
        {
          source: "packages/domain/src/ingestion.ts",
          exportName: "IngestionState",
          rustName: "IngestionState",
          kind: "string-literal-enum",
        },
        {
          source: "packages/domain/src/ingestion.ts",
          exportName: "IngestionErrorClassification",
          rustName: "IngestionErrorClassification",
          kind: "string-literal-enum",
        },
        {
          source: "packages/domain/src/ingestion.ts",
          exportName: "IngestionError",
          rustName: "IngestionError",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/ingestion.ts",
          exportName: "IngestionRun",
          rustName: "IngestionRun",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/provenance.ts",
          exportName: "SourceCapability",
          rustName: "SourceCapability",
          kind: "string-literal-enum",
        },
        {
          source: "packages/domain/src/provenance.ts",
          exportName: "SourceMaterialConfiguration",
          rustName: "SourceMaterialConfiguration",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-anchor-record.ts",
          exportName: "FixedForecastLeadHours",
          rustName: "FixedForecastLeadHours",
          kind: "unsigned-safe-integer-literal-enum",
          catalog: { kind: "unsigned-safe-integer-tuple" },
        },
        {
          source: "packages/domain/src/forecast-anchor-record.ts",
          exportName: "NormalizedForecastAnchorRecord",
          rustName: "NormalizedForecastAnchorRecord",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-anchor-record.ts",
          exportName: "NormalizedForecastAnchorRecordInput",
          rustName: "NormalizedForecastAnchorRecordInput",
          kind: "readonly-interface",
        },
        {
          source: "packages/database/src/repositories.ts",
          exportName: "ScheduledCheckpointState",
          rustName: "ScheduledCheckpointState",
          kind: "readonly-interface",
        },
        {
          source: "packages/database/src/repositories.ts",
          exportName: "StartIngestionRunInput",
          rustName: "StartIngestionRunInput",
          kind: "readonly-interface",
        },
        {
          source: "packages/database/src/repositories.ts",
          exportName: "PersistedWeatherAdjustmentRevision",
          rustName: "PersistedWeatherAdjustmentRevision",
          kind: "readonly-interface",
        },
        {
          source: "packages/database/src/repositories.ts",
          exportName: "CompleteBackfillIngestionInput",
          rustName: "CompleteBackfillIngestionInput",
          kind: "readonly-interface",
        },
        {
          source: "packages/database/src/repositories.ts",
          exportName: "CompleteForecastAnchorBackfillIngestionInput",
          rustName: "CompleteForecastAnchorBackfillIngestionInput",
          kind: "readonly-interface",
        },
        {
          source: "packages/database/src/repositories.ts",
          exportName: "FailIngestionRunInput",
          rustName: "FailIngestionRunInput",
          kind: "readonly-interface",
        },
        {
          source: "packages/database/src/repositories.ts",
          exportName: "CurrentQuery",
          rustName: "CurrentQuery",
          kind: "readonly-interface",
        },
        {
          source: "packages/database/src/repositories.ts",
          exportName: "ForecastProductSelection",
          rustName: "ForecastProductSelection",
          kind: "string-literal-enum",
        },
        {
          source: "packages/database/src/repositories.ts",
          exportName: "ForecastQuery",
          rustName: "ForecastQuery",
          kind: "readonly-interface",
        },
        {
          source: "packages/database/src/repositories.ts",
          exportName: "ForecastPressureContextQuery",
          rustName: "ForecastPressureContextQuery",
          kind: "readonly-interface",
        },
        {
          source: "packages/database/src/repositories.ts",
          exportName: "ForecastTrainingQuery",
          rustName: "ForecastTrainingQuery",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastTrainingCohort",
          rustName: "ForecastTrainingCohort",
          kind: "string-literal-enum",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastReferenceKind",
          rustName: "ForecastReferenceKind",
          kind: "string-literal-enum",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "FixedLeadAnchorTrainingRow",
          rustName: "FixedLeadAnchorTrainingRow",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "LegacyV4RetrievalSnapshotTrainingRow",
          rustName: "LegacyV4RetrievalSnapshotTrainingRow",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "LegacyV4RetrievalSnapshotTrainingRowInput",
          rustName: "LegacyV4RetrievalSnapshotTrainingRowInput",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastTrainingRow",
          rustName: "ForecastTrainingRow",
          kind: "named-readonly-object-union",
          objectUnion: {
            variants: [
              {
                rustName: "FixedLeadAnchor",
                payloadRustName: "FixedLeadAnchorTrainingRow",
              },
              {
                rustName: "LegacyV4RetrievalSnapshot",
                payloadRustName: "LegacyV4RetrievalSnapshotTrainingRow",
              },
            ],
          },
        },
        {
          source: "packages/database/src/repositories.ts",
          exportName: "ForecastTrainingCohorts",
          rustName: "ForecastTrainingCohorts",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastObservationProviderFamily",
          rustName: "ForecastObservationProviderFamily",
          kind: "string-literal-enum",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastObservationExcludedSourceLineage",
          rustName: "ForecastObservationExcludedSourceLineage",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastObservationStation",
          rustName: "ForecastObservationStation",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastNetworkEventIdentity",
          rustName: "ForecastNetworkEventIdentity",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentTrainingProvenance",
          rustName: "ForecastAdjustmentTrainingProvenance",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentMetricBand",
          rustName: "ForecastAdjustmentMetricBand",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentForecastIdentity",
          rustName: "ForecastAdjustmentForecastIdentity",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentCoefficient",
          rustName: "ForecastAdjustmentCoefficient",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentTrainingEnvelope",
          rustName: "ForecastAdjustmentTrainingEnvelope",
          kind: "readonly-interface",
          fieldProjections: {
            metric: {
              kind: "exclude-literal",
              root: "ForecastAdjustmentMetric",
            },
          },
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentMetricPolicy",
          rustName: "ForecastAdjustmentMetricPolicy",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentCandidateV2",
          rustName: "ForecastAdjustmentCandidateV2",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentWindCanaryTrainingIdentity",
          rustName: "ForecastAdjustmentWindCanaryTrainingIdentity",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentWindCanaryCandidateV1",
          rustName: "ForecastAdjustmentWindCanaryCandidateV1",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentRegistryActiveBundleV1",
          rustName: "ForecastAdjustmentRegistryActiveBundleV1",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentRegistryV1",
          rustName: "ForecastAdjustmentRegistryV1",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentWindCanaryRegistryActiveBundleV1",
          rustName: "ForecastAdjustmentWindCanaryRegistryActiveBundleV1",
          kind: "readonly-interface",
        },
        {
          source: "packages/domain/src/forecast-adjustment.ts",
          exportName: "ForecastAdjustmentWindCanaryRegistryV1",
          rustName: "ForecastAdjustmentWindCanaryRegistryV1",
          kind: "readonly-interface",
        },
        ...RUNTIME_EVIDENCE_ROOTS,
        ...WIND_CANARY_RUNTIME_ROOTS,
        ...ECMWF_TEMPERATURE_CANARY_ROOTS,
        ...TEMPERATURE_MOS_MODEL_ROOTS,
        ...TEMPERATURE_CANARY_RUNTIME_ROOTS,
        ...TEMPERATURE_INFERENCE_RESULT_ROOTS,
        ...PUBLIC_READ_QUERY_ROOTS,
        ...RAIN_CAPTURE_ADJUSTMENT_ROOTS,
        ...RAIN_CAPTURE_RECEIPT_ROOTS,
        ...RAIN_CAPTURE_REQUEST_ROOTS,
        ...FIXED_GAUGE_RESULT_ROOTS,
        ...REVISION_ADMISSION_ROOTS,
        ...ADJUSTMENT_SNAPSHOT_ROOTS,
        ...ARCHIVE_RELAY_ROOTS,
        ...ARCHIVE_RELAY_INPUT_ROOTS,
        ...REVISION_PROJECTION_ROOTS,
        ...REVISION_DOCUMENT_ROOTS,
        ...FIXED_GAUGE_TARGET_DOCUMENT_ROOTS,
        ...MAINTENANCE_EVALUATION_ROOTS,
        ...READINESS_SNAPSHOT_ROOTS,
        ...WORKER_RESULT_ROOTS,
        ...OPEN_METEO_REQUEST_ROOTS,
        ...STATION_ADAPTER_ROOTS,
        ...NOAA_TIDE_REQUEST_ROOTS,
        ...TIDE_BACKFILL_ROOTS,
        ...BACKFILL_DATA_ROOTS,
        ...WIDGET_FORECAST_ROOTS,
        ...WIDGET_FORECAST_EXTENSION_ROOTS,
        ...PROVIDER_BATCH_ROOTS,
        MIGRATION_RESULT_ROOT,
        WEATHER_RECORD_ROW_ROOT,
        CURRENT_WEATHER_RECORD_ROW_ROOT,
        ...SCHEDULING_DISCOVERY_FORECAST_ROW_ROOTS,
        TREND_POINT_ROW_ROOT,
        ...FORECAST_OBSERVATION_HOURLY_ROOTS,
        PERSISTED_ECMWF_TEMPERATURE_REVISION_ROOT,
        ...BOOTSTRAP_CATALOG_ROOTS,
        ...SERVING_FORECAST_ADJUSTMENT_ROOTS,
        ...FORECAST_ADJUSTMENT_LOADER_ROOTS,
        ...FORECAST_ADJUSTMENT_MAINTENANCE_PACKAGE_ROOTS,
        FORECAST_ADJUSTMENT_WIND_REGISTRY_ROOT,
        ...FORECAST_ADJUSTMENT_MAINTENANCE_LOADER_ROOTS,
        LOADED_RAIN_RUNTIME_REGISTRY_ROOT,
        ...API_STARTUP_SNAPSHOT_ROOTS,
        ...RAIN_INFERENCE_ROOTS,
        ...PORTABLE_RAIN_ARTIFACT_ROOTS,
        ...["RainMaintenanceControlSupport", "RainMaintenanceControlState"].map((name) => ({
          source: "packages/forecast-adjustment/src/rain-maintenance-controls.ts",
          exportName: name,
          rustName: name,
          kind: "readonly-interface",
        })),
        ...WORKER_BACKFILL_SOURCE_ROOTS,
        ...PRIVATE_FORECAST_REPOSITORY_ROW_ROOTS,
        ...FORECAST_SCORECARD_ROOTS,
        ...INSTALLED_MAINTENANCE_RECEIPT_ROOTS,
        ...FORECAST_PREPARED_COUNTER_ROOTS,
        ...PUBLIC_API_RESPONSE_ROOTS,
        ...API_DIAGNOSTIC_ROOTS,
        ...TEMPERATURE_DATA_WIRE_ROOTS,
      ],
    );
    assert.deepEqual(manifest.typeImports, CONFIG.typeImports);
    assert.match(rust, /pub mod retained_forecast_observation \{/u);
    assert.equal(
      manifest.output.path,
      "rust/crates/weather-contracts/src/generated.rs",
    );
    assert.equal(manifest.output.sha256, sha256(rust));
  },
);

test(
  "erases the original configured type bindings from emitted JavaScript",
  // audit the bounded configured graph with each workspace's actual esm options
  () => {
    assert.equal(ts.version, CONFIG.typescriptVersion);
    assert.equal(CONFIG.typeImports.length, REPOSITORY_IMPORT_COUNT);
    const consumers = [...new Set(
      CONFIG.typeImports.map(
        // retain one copy of every configured consumer source
        (edge) => edge.consumer,
      ),
    )].sort();
    assert.equal(consumers.length, 40);

    // bind one consumer path to its workspace package root
    const packageRootFor = (consumer) => {
      const match = /^(?:apps|packages)\/[^/]+/u.exec(consumer);
      assert.ok(match, consumer);
      return match[0];
    };
    const packageRoots = [...new Set(
      consumers.map(
        // retain one compiler configuration per workspace package
        (consumer) => packageRootFor(consumer),
      ),
    )].sort();
    assert.deepEqual(packageRoots, [
      "apps/api",
      "apps/web",
      "apps/worker",
      "packages/contracts",
      "packages/database",
      "packages/domain",
      "packages/forecast-adjustment",
      "packages/providers",
    ]);

    const baseConfig = readRepositoryJson("tsconfig.base.json");
    assert.equal(baseConfig.compilerOptions.module, "NodeNext");
    assert.equal(baseConfig.compilerOptions.moduleResolution, "NodeNext");
    assert.equal(baseConfig.compilerOptions.verbatimModuleSyntax, true);
    const compilerOptionsByPackage = new Map();

    // verify and convert every actual workspace compiler configuration once
    for (const packageRoot of packageRoots) {
      const workspaceConfig = readRepositoryJson(`${packageRoot}/tsconfig.json`);
      const packageJson = readRepositoryJson(`${packageRoot}/package.json`);
      assert.equal(workspaceConfig.extends, "../../tsconfig.base.json");
      assert.equal(packageJson.type, "module");
      const converted = ts.convertCompilerOptionsFromJson(
        {
          ...baseConfig.compilerOptions,
          ...workspaceConfig.compilerOptions,
        },
        fileURLToPath(new URL(`../${packageRoot}/`, import.meta.url)),
      );
      assert.equal(converted.errors.length, 0, packageRoot);
      assert.equal(converted.options.module, ts.ModuleKind.NodeNext);
      assert.equal(
        converted.options.moduleResolution,
        ts.ModuleResolutionKind.NodeNext,
      );
      assert.equal(converted.options.verbatimModuleSyntax, true);
      compilerOptionsByPackage.set(packageRoot, converted.options);
    }

    const sourceModules = new Map();
    const emittedModules = new Map();

    // transpile every bounded consumer once and compare all runtime import rows
    for (const consumer of consumers) {
      const consumerEdges = CONFIG.typeImports.filter(
        // retain only edges owned by this consumer module
        (edge) => edge.consumer === consumer,
      );
      const relevantSpecifiers = new Set(consumerEdges.map(
        // limit the import audit to configured graph edges
        (edge) => edge.specifier,
      ));
      assert.equal(relevantSpecifiers.size, consumerEdges.length, consumer);
      const source = SOURCES[consumer];
      assert.equal(typeof source, "string", consumer);
      const virtualFileName = consumer.replace(/\.ts$/u, ".mts");
      assert.notEqual(virtualFileName, consumer);
      const emitted = transpileVirtualEsm(
        source,
        virtualFileName,
        compilerOptionsByPackage.get(packageRootFor(consumer)),
      );
      const sourceModule = parseModuleSource(
        source,
        virtualFileName,
        ts.ScriptKind.TS,
      );
      const emittedModule = parseModuleSource(
        emitted,
        virtualFileName.replace(/\.mts$/u, ".mjs"),
        ts.ScriptKind.JS,
      );
      assert.deepEqual(
        runtimeImportSignatures(emittedModule, relevantSpecifiers),
        runtimeImportSignatures(sourceModule, relevantSpecifiers),
        consumer,
      );
      sourceModules.set(consumer, sourceModule);
      emittedModules.set(consumer, emittedModule);
    }

    let typeBearingEdges = 0;
    let configuredTypeBindings = 0;
    const auditedTypeBindings = [];

    // require every configured type export to use explicit erased syntax
    for (const edge of CONFIG.typeImports) {
      const names = [
        ...Object.keys(edge.exports),
        ...Object.keys(edge.proofExports ?? {}),
      ];

      // leave the two independently proven runtime-value edges for their audit
      if (names.length === 0) {
        continue;
      }
      typeBearingEdges += 1;
      const sourceBindings = namedImportBindings(
        sourceModules.get(edge.consumer),
        edge.specifier,
      );
      const emittedBindings = namedImportBindings(
        emittedModules.get(edge.consumer),
        edge.specifier,
      );

      // bind each configured exported name to one exact source import specifier
      for (const importedName of names) {
        const matches = sourceBindings.filter(
          // match the exported identity rather than a possible local alias
          (binding) => binding.importedName === importedName,
        );
        assert.equal(
          matches.length,
          1,
          `${edge.consumer}#${edge.specifier}#${importedName}`,
        );
        assert.equal(matches[0].typeOnly, true, importedName);
        assert.equal(
          emittedBindings.some(
            // prove the configured imported and local binding pair was erased
            (binding) =>
              binding.importedName === matches[0].importedName &&
              binding.localName === matches[0].localName,
          ),
          false,
          `${edge.consumer}#${edge.specifier}#${matches[0].localName}`,
        );
        auditedTypeBindings.push({
          consumer: edge.consumer,
          specifier: edge.specifier,
          importedName,
          localName: matches[0].localName,
        });
        configuredTypeBindings += 1;
      }
    }
    assert.equal(typeBearingEdges, 59);
    assert.equal(configuredTypeBindings, 112);
    assert.equal(auditedTypeBindings.length, 112);

    const valueProofs = [];

    // identify the two configured runtime proofs independently of type exports
    for (const edge of CONFIG.typeImports) {
      // retain the exact tuple identity wrapper value import
      if (edge.tupleIdentityWrapper !== undefined) {
        valueProofs.push({
          consumer: edge.consumer,
          specifier: edge.specifier,
          option: "tupleIdentityWrapper",
          importedName: edge.tupleIdentityWrapper,
        });
      }

      // retain the exact literal string value import
      if (edge.literalStringValue !== undefined) {
        valueProofs.push({
          consumer: edge.consumer,
          specifier: edge.specifier,
          option: "literalStringValue",
          importedName: edge.literalStringValue,
        });
      }
    }
    assert.deepEqual(valueProofs, [
      {
        consumer: "packages/forecast-adjustment/src/runtime-loader.ts",
        specifier: "./candidate.js",
        option: "tupleIdentityWrapper",
        importedName: "deepFreeze",
      },
      {
        consumer: "packages/forecast-adjustment/src/rain-runtime-registry.ts",
        specifier: "./rain-hurdle-wind-artifact.js",
        option: "literalStringValue",
        importedName: "RAIN_HURDLE_WIND_ARTIFACT_SHA256",
      },
    ]);

    // require both value proofs to remain runtime imports and outside type counts
    for (const proof of valueProofs) {
      const sourceMatches = namedImportBindings(
        sourceModules.get(proof.consumer),
        proof.specifier,
      ).filter(
        // match one configured runtime proof by its imported identity
        (binding) => binding.importedName === proof.importedName,
      );
      const emittedMatches = namedImportBindings(
        emittedModules.get(proof.consumer),
        proof.specifier,
      ).filter(
        // match the emitted value by both imported and local identity
        (binding) =>
          binding.importedName === proof.importedName &&
          binding.localName === sourceMatches[0]?.localName,
      );
      assert.equal(sourceMatches.length, 1, proof.importedName);
      assert.equal(sourceMatches[0].typeOnly, false, proof.importedName);
      assert.equal(emittedMatches.length, 1, proof.importedName);
      assert.equal(
        auditedTypeBindings.some(
          // keep value proofs out of the configured erased-type inventory
          (binding) =>
            binding.consumer === proof.consumer &&
            binding.specifier === proof.specifier &&
            binding.importedName === proof.importedName,
        ),
        false,
        proof.importedName,
      );
    }
  },
);

test(
  "distinguishes erased type edges from empty and mixed runtime imports",
  // exercise every erasure boundary with one isolated virtual esm module
  () => {
    const baseConfig = readRepositoryJson("tsconfig.base.json");
    const workspaceConfig = readRepositoryJson("packages/domain/tsconfig.json");
    const converted = ts.convertCompilerOptionsFromJson(
      {
        ...baseConfig.compilerOptions,
        ...workspaceConfig.compilerOptions,
      },
      fileURLToPath(
        new URL("../packages/domain/", import.meta.url),
      ),
    );
    assert.equal(converted.errors.length, 0);
    const source = `import type { X as Alias } from "./types.js";
export type { X as ReExported } from "./types.js";
import { type InlineOnly } from "./inline.js";
import { type MixedType, runtimeValue as localValue } from "./mixed.js";
import "./side-effect.js";
export type AliasUse = Alias;
void localValue;
`;
    const fileName = "packages/domain/src/type-erasure-fixture.mts";
    const emitted = transpileVirtualEsm(
      source,
      fileName,
      converted.options,
    );
    const sourceModule = parseModuleSource(
      source,
      fileName,
      ts.ScriptKind.TS,
    );
    const emittedModule = parseModuleSource(
      emitted,
      fileName.replace(/\.mts$/u, ".mjs"),
      ts.ScriptKind.JS,
    );
    const relevantSpecifiers = new Set([
      "./types.js",
      "./inline.js",
      "./mixed.js",
      "./side-effect.js",
    ]);
    const expectedRuntime = [
      {
        specifier: "./inline.js",
        kind: "bindings",
        defaultBinding: null,
        namespaceBinding: null,
        namedBindings: [],
      },
      {
        specifier: "./mixed.js",
        kind: "bindings",
        defaultBinding: null,
        namespaceBinding: null,
        namedBindings: [{
          importedName: "runtimeValue",
          localName: "localValue",
        }],
      },
      { specifier: "./side-effect.js", kind: "side-effect" },
    ];
    assert.deepEqual(
      runtimeImportSignatures(sourceModule, relevantSpecifiers),
      expectedRuntime,
    );
    assert.deepEqual(
      runtimeImportSignatures(emittedModule, relevantSpecifiers),
      expectedRuntime,
    );
    assert.deepEqual(namedImportBindings(sourceModule, "./types.js"), [{
      importedName: "X",
      localName: "Alias",
      typeOnly: true,
    }]);
    assert.deepEqual(namedImportBindings(emittedModule, "./types.js"), []);
    assert.deepEqual(namedImportBindings(sourceModule, "./inline.js"), [{
      importedName: "InlineOnly",
      localName: "InlineOnly",
      typeOnly: true,
    }]);
    assert.deepEqual(namedImportBindings(emittedModule, "./inline.js"), []);
    assert.deepEqual(namedImportBindings(sourceModule, "./mixed.js"), [
      {
        importedName: "MixedType",
        localName: "MixedType",
        typeOnly: true,
      },
      {
        importedName: "runtimeValue",
        localName: "localValue",
        typeOnly: false,
      },
    ]);
    assert.deepEqual(namedImportBindings(emittedModule, "./mixed.js"), [{
      importedName: "runtimeValue",
      localName: "localValue",
      typeOnly: false,
    }]);

    const emptyRuntimeImports = emittedModule.statements.filter(
      // retain the all-inline import that must still evaluate its module
      (statement) =>
        ts.isImportDeclaration(statement) &&
        moduleSpecifierValue(statement) === "./inline.js",
    );
    assert.equal(emptyRuntimeImports.length, 1);
    assert.ok(
      emptyRuntimeImports[0].importClause !== undefined &&
      emptyRuntimeImports[0].importClause.isTypeOnly === false &&
      emptyRuntimeImports[0].importClause.namedBindings !== undefined &&
      ts.isNamedImports(emptyRuntimeImports[0].importClause.namedBindings) &&
      emptyRuntimeImports[0].importClause.namedBindings.elements.length === 0,
    );
    const sourceTypeExports = sourceModule.statements.filter(
      // retain the one explicit aliased type-only re-export
      (statement) =>
        ts.isExportDeclaration(statement) &&
        statement.isTypeOnly &&
        statement.moduleSpecifier !== undefined &&
        moduleSpecifierValue(statement) === "./types.js",
    );
    assert.equal(sourceTypeExports.length, 1);
    const emittedTypeExports = emittedModule.statements.filter(
      // find any surviving re-export from the erased type-only source edge
      (statement) =>
        ts.isExportDeclaration(statement) &&
        statement.moduleSpecifier !== undefined &&
        moduleSpecifierValue(statement) === "./types.js",
    );
    assert.deepEqual(emittedTypeExports, []);
  },
);

test(
  "represents required nullability separately from optional presence",
  // compare the three field-state contracts
  () => {
    const source = `export interface StateContract {
  readonly requiredMaybe: string | null;
  readonly optional?: string;
  readonly optionalMaybe?: string | null;
}
`;
    const { rust } = generateRustContracts(
      fixtureConfig("StateContract"),
      { "fixture.ts": source },
    );

    assert.match(rust, /pub enum Nullable<T>/);
    assert.match(rust, /pub enum FieldPresence<T>/);
    assert.match(rust, /pub requiredMaybe: Nullable<String>/);
    assert.match(rust, /pub optional: FieldPresence<String>/);
    assert.match(
      rust,
      /pub optionalMaybe: FieldPresence<Nullable<String>>/,
    );
  },
);

test(
  "extracts declarations without evaluating TypeScript module code",
  // keep module statements inert during generation
  () => {
    const source = `throw new Error("module code was evaluated");
export type Outcome = "ok" | "not_ok";
`;
    const { rust } = generateRustContracts(
      fixtureConfig("Outcome"),
      { "fixture.ts": source },
    );

    assert.match(rust, /\("ok", Self::Ok\)/);
    assert.match(rust, /\("not_ok", Self::NotOk\)/);
  },
);

test(
  "keeps explicit readonly object and array syntax",
  // preserve only syntax carried by the selected declaration
  () => {
    const source = `export interface Root {
  readonly child: { readonly value: string };
  readonly values: readonly string[];
}
`;
    const { rust } = generateRustContracts(
      fixtureConfig("Root"),
      { "fixture.ts": source },
    );

    assert.match(rust, /pub struct RootChild/);
    assert.match(rust, /pub child: RootChild/);
    assert.match(rust, /pub values: Vec<String>/);
  },
);

test(
  "retains distinct nullable arrays and nullable elements through explicit grouping",
  // preserve null at the level where it was declared
  () => {
    const source = "export interface Root { readonly values: readonly ((number | null))[] | null; }\n";
    const { rust } = generateRustContracts(
      fixtureConfig("Root", { values: "finite-f64" }),
      { "fixture.ts": source },
    );
    assert.match(rust, /pub values: Nullable<Vec<Nullable<f64>>>,/);
  },
);

test(
  "projects homogeneous string-literal fields without widening null or missing",
  // preserve exact field unions under each wrapper
  () => {
    const source = `export interface Root {
  readonly state: "active" | "disabled" | "raw_fallback";
  readonly optional?: "first" | "second";
  readonly nullable: "ready" | "waiting" | null;
  readonly optionalNullable?: "present" | "absent" | null;
}
`;
    const { rust } = generateRustContracts(
      fixtureConfig("Root"),
      { "fixture.ts": source },
    );

    assert.match(rust, /pub state: RootStateEnum/);
    assert.match(rust, /pub optional: FieldPresence<RootOptionalEnum>/);
    assert.match(rust, /pub nullable: Nullable<RootNullableEnum>/);
    assert.match(
      rust,
      /pub optionalNullable: FieldPresence<Nullable<RootOptionalNullableEnum>>/,
    );
    assert.match(
      rust,
      /impl RootStateEnum \{\s*pub const WIRE_VALUES: &'static \[\(&'static str, Self\)\] = &\[\s*\("active", Self::Active\),\s*\("disabled", Self::Disabled\),\s*\("raw_fallback", Self::RawFallback\),\s*\];\s*\}/,
    );
  },
);

test(
  "projects ordinary unsigned-decimal safe integer roots with exact wire values",
  // retain zero and the JavaScript safe-integer ceiling
  () => {
    const source = "export type SafeUnsigned = 0 | 1 | 9007199254740991;\n";
    const { rust, manifest: manifestText } = generateRustContracts(
      fixtureConfig("SafeUnsigned"),
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.match(
      rust,
      /pub enum SafeUnsigned \{\s*#\[doc = "wire value: 0"\]\s*Value0,\s*#\[doc = "wire value: 1"\]\s*Value1,\s*#\[doc = "wire value: 9007199254740991"\]\s*Value9007199254740991,\s*\}/,
    );
    assert.match(
      rust,
      /impl SafeUnsigned \{\s*pub const WIRE_VALUES: &'static \[\(u64, Self\)\] = &\[\s*\(0, Self::Value0\),\s*\(1, Self::Value1\),\s*\(9007199254740991, Self::Value9007199254740991\),\s*\];\s*\}/,
    );
    assert.equal(
      manifest.roots[0].kind,
      "unsigned-safe-integer-literal-enum",
    );
  },
);

test(
  "projects one required unsigned safe integer literal field",
  // retain one exact decimal value without admitting a number field
  () => {
    const config = fixtureConfig("Root", {
      delayHours: "unsigned-safe-integer-literal",
    });
    const { rust } = generateRustContracts(
      config,
      { "fixture.ts": "export interface Root { readonly delayHours: 6; }\n" },
    );

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["delayHours", "RootDelayHoursLiteral"],
    ]);
    assert.match(
      rust,
      /#\[doc = "exact unsigned integer wire literal: 6"\]\s*#\[derive\(Debug, Clone, Copy, PartialEq, Eq\)\]\s*pub struct RootDelayHoursLiteral;\s*impl RootDelayHoursLiteral \{\s*pub const WIRE_VALUE: u64 = 6;\s*\}/u,
    );
    assert.match(
      rust,
      /wire rule: unsigned-safe-integer-literal, one canonical decimal value in 0\.\.=9007199254740991"\]\s*pub delayHours: RootDelayHoursLiteral,/u,
    );
    const requiredLiteral =
      /unsigned-safe-integer-literal requires a required direct canonical decimal integer literal at Root\.delayHours/u;
    const invalidFields = [
      ["", "delayHours: number", requiredLiteral],
      ["type Delay = 6; ", "delayHours: Delay", requiredLiteral],
      ["", "delayHours: 6 | 7", requiredLiteral],
      ["", "delayHours?: 6", requiredLiteral],
      ["", "delayHours: 6.0", /unsigned decimal integer literal unsupported "6\.0" at root Root/u],
      ["", "delayHours: 9007199254740992", /unsafe integer literal "9007199254740992" at root Root/u],
    ];

    // reject values and forms outside the exact literal contract
    for (const [prefix, field, expected] of invalidFields) {
      assert.throws(
        // evaluate the same explicit mapping for each source form
        () => generateRustContracts(config, {
          "fixture.ts": `${prefix}export interface Root { readonly ${field}; }\n`,
        }),
        expected,
        field,
      );
    }
  },
);

test(
  "preserves direct runtime number storage without claiming a JSON wire value",
  // admit only required number storage and one exact nullable alternative
  () => {
    const config = fixtureConfig("Root", {
      direct: "runtime-f64",
      nullable: "runtime-f64",
      reversed: "runtime-f64",
    });
    const source = `export interface Root {
  readonly direct: number;
  readonly nullable: number | null;
  readonly reversed: null | number;
}
`;
    const { rust } = generateRustContracts(config, { "fixture.ts": source });

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["direct", "f64"],
      ["nullable", "Nullable<f64>"],
      ["reversed", "Nullable<f64>"],
    ]);
    const rootDefinition = rust.match(/^pub struct Root \{\n[\s\S]+?^\}/mu);
    assert.ok(rootDefinition);
    assert.equal(
      rootDefinition[0].match(
        /storage rule: runtime-f64, JavaScript number including non-finite values; not a JSON wire type/gu,
      )?.length,
      3,
    );
    const expected =
      /runtime-f64 requires a required direct number or number \| null field at Root\.value/u;
    const invalidFields = [
      ["", "value?: number"],
      ["", "value: readonly number[]"],
      ["type Value = number; ", "value: Value"],
      ["", "value: string"],
      ["", "value: number | null | string"],
      ["", "value: number | undefined"],
    ];

    // reject syntax that could widen the runtime-number boundary
    for (const [prefix, field] of invalidFields) {
      assert.throws(
        // evaluate each source shape through the same explicit mapping
        () => generateRustContracts(
          fixtureConfig("Root", { value: "runtime-f64" }),
          {
            "fixture.ts":
              `${prefix}export interface Root { readonly ${field}; }\n`,
          },
        ),
        expected,
        field,
      );
    }
  },
);

test(
  "preserves direct builtin Uint8Array bytes, presence, and nullability",
  // bind direct byte storage to the pinned builtin without JSON semantics
  () => {
    const config = fixtureConfig("Root", {
      body: "uint8array-bytes",
      nullable: "uint8array-bytes",
      optional: "uint8array-bytes",
      optionalNullable: "uint8array-bytes",
      reversed: "uint8array-bytes",
    });
    const source = `export interface Root {
  readonly body: Uint8Array;
  readonly nullable: Uint8Array | null;
  readonly optional?: Uint8Array;
  readonly optionalNullable?: Uint8Array | null;
  readonly reversed: null | Uint8Array;
}
`;
    const { rust } = generateRustContracts(config, { "fixture.ts": source });

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["body", "Vec<u8>"],
      ["nullable", "Nullable<Vec<u8>>"],
      ["optional", "FieldPresence<Vec<u8>>"],
      ["optionalNullable", "FieldPresence<Nullable<Vec<u8>>>"],
      ["reversed", "Nullable<Vec<u8>>"],
    ]);
    assert.equal(
      rust.match(
        /storage rule: uint8array-bytes, owned Uint8Array bytes; not a JSON wire type/gu,
      )?.length,
      5,
    );
    assert.match(
      rust,
      /storage rule: uint8array-bytes, owned Uint8Array bytes; not a JSON wire type"\]\s*pub nullable: Nullable<Vec<u8>>,/u,
    );
    assert.match(
      rust,
      /storage rule: uint8array-bytes, owned Uint8Array bytes; not a JSON wire type"\]\s*pub optional: FieldPresence<Vec<u8>>,/u,
    );
    assert.match(
      rust,
      /storage rule: uint8array-bytes, owned Uint8Array bytes; not a JSON wire type"\]\s*pub optionalNullable: FieldPresence<Nullable<Vec<u8>>>,/u,
    );
    assert.match(
      rust,
      /storage rule: uint8array-bytes, owned Uint8Array bytes; not a JSON wire type"\]\s*pub reversed: Nullable<Vec<u8>>,/u,
    );
    const syntaxError =
      /uint8array-bytes requires a direct Uint8Array or Uint8Array \| null field at Root\.body/u;
    const invalidFields = [
      ["type Bytes = Uint8Array; ", "body: Bytes", syntaxError],
      ["", "body: Int8Array", syntaxError],
      ["", "body: Uint8Array<ArrayBuffer>", syntaxError],
      ["", "body: Uint8Array | Uint8Array | null", syntaxError],
      ["", "body: Uint8Array | null | null", syntaxError],
      ["", "body: Uint8Array | null | string", syntaxError],
      [
        "declare namespace Bytes { type Uint8Array = string; } ",
        "body: Bytes.Uint8Array",
        syntaxError,
      ],
      [
        "interface Uint8Array { readonly marker: string; } ",
        "body: Uint8Array",
        /uint8array-bytes builtin identity unsupported at Root\.body/u,
      ],
    ];

    // reject syntax or declaration identities that are not the pinned builtin
    for (const [prefix, field, expected] of invalidFields) {
      assert.throws(
        // evaluate every rejected source form through the same explicit mapping
        () => generateRustContracts(
          fixtureConfig("Root", { body: "uint8array-bytes" }),
          {
            "fixture.ts":
              `${prefix}export interface Root { readonly ${field}; }\n`,
          },
        ),
        expected,
        field,
      );
    }
  },
);

test(
  "preserves one ordered scalar JSON metadata object",
  // keep dynamic keys ordered while closing values to four direct scalars
  () => {
    const config = fixtureConfig("Root", {
      metadata: "ordered-json-scalar-object",
    });
    const source = `export interface Root {
  readonly metadata: Readonly<Record<string, boolean | null | string | number>>;
}
`;
    const { rust } = generateRustContracts(config, { "fixture.ts": source });

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["metadata", "Vec<(Vec<u16>, RootMetadataValue)>"],
    ]);
    assert.match(
      rust,
      /pub enum RootMetadataValue \{\s*Null,\s*Boolean\(bool\),\s*Number\(f64\),\s*String\(Vec<u16>\),\s*\}/u,
    );
    assert.match(
      rust,
      /wire rule: ordered-json-scalar-object, ordered unique UTF-16 keys and scalar values; finite numbers only"\]\s*pub metadata: Vec<\(Vec<u16>, RootMetadataValue\)>,/u,
    );

    const shapeError =
      /ordered-json-scalar-object requires Readonly<Record<string, Value>> at Root\.metadata/u;
    const scalarError =
      /ordered-json-scalar-object requires exactly string \| number \| boolean \| null at Root\.metadata/u;
    const invalidSources = [
      [
        "export interface Root { readonly metadata: Record<string, string | number | boolean | null>; }\n",
        shapeError,
      ],
      [
        "export interface Root { readonly metadata: Readonly<Record<number, string | number | boolean | null>>; }\n",
        shapeError,
      ],
      [
        "type Metadata = Readonly<Record<string, string | number | boolean | null>>; export interface Root { readonly metadata: Metadata; }\n",
        shapeError,
      ],
      [
        "export interface Root { readonly metadata?: Readonly<Record<string, string | number | boolean | null>>; }\n",
        /ordered-json-scalar-object requires a required field at Root\.metadata/u,
      ],
      [
        "type Readonly<T> = T; export interface Root { readonly metadata: Readonly<Record<string, string | number | boolean | null>>; }\n",
        /ordered-json-scalar-object utility identity unsupported at Root\.metadata/u,
      ],
      [
        "type Scalar = string | number | boolean | null; export interface Root { readonly metadata: Readonly<Record<string, Scalar>>; }\n",
        scalarError,
      ],
      [
        "export interface Root { readonly metadata: Readonly<Record<string, string | number | boolean | null | string>>; }\n",
        scalarError,
      ],
      [
        "export interface Root { readonly metadata: Readonly<Record<string, string | number | null>>; }\n",
        scalarError,
      ],
      [
        "export interface Root { readonly metadata: Readonly<Record<string, string | number | boolean | null | readonly string[]>>; }\n",
        scalarError,
      ],
    ];

    // reject aliases, widened shapes, and changed utility identities
    for (const [invalidSource, expected] of invalidSources) {
      assert.throws(
        // evaluate each rejected grammar through the explicit scalar mapping
        () => generateRustContracts(config, { "fixture.ts": invalidSource }),
        expected,
        invalidSource,
      );
    }
  },
);

test(
  "keeps revision record mappings on their exact direct grammar",
  // separate key, nullable-string, and row scalar domains without aliases or widening
  () => {
    const config = fixtureConfig("Root", {
      key: "ordered-json-key-object",
      rows: "ordered-json-scalar-object-array",
      source: "ordered-json-nullable-string-object",
    });
    const source = `export interface Root {
  readonly key: Readonly<Record<string, string | number | null>>;
  readonly rows: readonly Readonly<Record<string, string | number | boolean | null>>[];
  readonly source: Readonly<Record<string, string | null>>;
}
`;
    const { rust } = generateRustContracts(config, { "fixture.ts": source });

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["key", "Vec<(Vec<u16>, RootKeyValue)>"],
      ["rows", "Vec<Vec<(Vec<u16>, RootRowsValue)>>"],
      ["source", "Vec<(Vec<u16>, Nullable<Vec<u16>>)>"],
    ]);
    assert.match(
      rust,
      /pub enum RootKeyValue \{\s*Null,\s*Number\(f64\),\s*String\(Vec<u16>\),\s*\}/u,
    );
    assert.match(
      rust,
      /pub enum RootRowsValue \{\s*Null,\s*Boolean\(bool\),\s*Number\(f64\),\s*String\(Vec<u16>\),\s*\}/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "Root", "key"),
      /wire rule: ordered-json-key-object, ordered unique UTF-16 keys and string, finite number, or null values/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "Root", "rows"),
      /wire rule: ordered-json-scalar-object-array, ordered array of scalar objects with UTF-16 keys and finite numbers/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "Root", "source"),
      /wire rule: ordered-json-nullable-string-object, ordered unique UTF-16 keys and nullable UTF-16 strings/u,
    );

    const invalidCases = [
      [
        "ordered-json-key-object",
        "metadata: Readonly<Record<string, string | number | boolean | null>>",
        /ordered-json-key-object requires exactly string \| number \| null at Root\.metadata/u,
      ],
      [
        "ordered-json-nullable-string-object",
        "metadata: Readonly<Record<string, string | number | null>>",
        /ordered-json-nullable-string-object requires exactly string \| null at Root\.metadata/u,
      ],
      [
        "ordered-json-scalar-object-array",
        "metadata: readonly Readonly<Record<string, string | number | null>>[]",
        /ordered-json-scalar-object-array requires exactly string \| number \| boolean \| null at Root\.metadata/u,
      ],
      [
        "ordered-json-key-object",
        "metadata?: Readonly<Record<string, string | number | null>>",
        /ordered-json-key-object requires a required field at Root\.metadata/u,
      ],
      [
        "ordered-json-nullable-string-object",
        "metadata?: Readonly<Record<string, string | null>>",
        /ordered-json-nullable-string-object requires a required field at Root\.metadata/u,
      ],
      [
        "ordered-json-scalar-object-array",
        "metadata?: readonly Readonly<Record<string, string | number | boolean | null>>[]",
        /ordered-json-scalar-object-array requires a required field at Root\.metadata/u,
      ],
      [
        "ordered-json-scalar-object-array",
        "metadata: ReadonlyArray<Readonly<Record<string, string | number | boolean | null>>>",
        /ordered-json-scalar-object-array requires readonly Readonly<Record<string, Value>>\[\] at Root\.metadata/u,
      ],
      [
        "ordered-json-key-object",
        "metadata: Readonly<Record<string, string | number | null>>",
        /ordered-json-key-object utility identity unsupported at Root\.metadata/u,
        "type Readonly<T> = T; ",
      ],
      [
        "ordered-json-nullable-string-object",
        "metadata: Readonly<Record<string, string | null>>",
        /ordered-json-nullable-string-object utility identity unsupported at Root\.metadata/u,
        "type Readonly<T> = T; ",
      ],
      [
        "ordered-json-scalar-object-array",
        "metadata: readonly Readonly<Record<string, string | number | boolean | null>>[]",
        /ordered-json-scalar-object-array utility identity unsupported at Root\.metadata/u,
        "type Readonly<T> = T; ",
      ],
    ];

    // reject widened arms, optional fields, alternate arrays, and changed utilities
    for (const [mapping, field, expected, prefix = ""] of invalidCases) {
      assert.throws(
        // evaluate every rejected source form through its explicit mapping
        () => generateRustContracts(
          fixtureConfig("Root", { metadata: mapping }),
          {
            "fixture.ts":
              `${prefix}export interface Root { readonly ${field}; }\n`,
          },
        ),
        expected,
        field,
      );
    }
  },
);

test(
  "projects only required direct readonly string-literal records",
  // retain fixed key order and reject every widened or indirect record spelling
  () => {
    const config = fixtureConfig("Root", {
      probability: "readonly-string-literal-record",
    });
    const source = `export interface Root {
  readonly probability: Readonly<Record<"atLeast0_1" | "atLeast1_0" | "atLeast2_5", string>>;
}
`;
    const { rust } = generateRustContracts(config, { "fixture.ts": source });

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["probability", "RootProbability"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootProbability"), [
      ["atLeast0_1", "String"],
      ["atLeast1_0", "String"],
      ["atLeast2_5", "String"],
    ]);
    assert.equal(
      rustStructFieldBlock(rust, "Root", "probability"),
      `    #[doc = "wire rule: readonly-string-literal-record, exact string values at declared keys in source order"]
    pub probability: RootProbability,`,
    );

    const invalidCases = [
      [
        `export interface Root {
  readonly probability?: Readonly<Record<"atLeast0_1", string>>;
}
`,
        /readonly-string-literal-record requires a required field at Root\.probability/u,
      ],
      [
        `export interface Root {
  readonly probability: Record<"atLeast0_1", string>;
}
`,
        /readonly-string-literal-record requires Readonly<Record<"field" \| \.\.\., string>> at Root\.probability/u,
      ],
      [
        `type Readonly<T> = T;
export interface Root {
  readonly probability: Readonly<Record<"atLeast0_1", string>>;
}
`,
        /readonly-string-literal-record utility identity unsupported at Root\.probability/u,
      ],
      [
        `type Record<K extends keyof any, T> = { [P in K]: T };
export interface Root {
  readonly probability: Readonly<Record<"atLeast0_1", string>>;
}
`,
        /readonly-string-literal-record utility identity unsupported at Root\.probability/u,
      ],
      [
        `export interface Root {
  readonly probability: Readonly<Record<string, string>>;
}
`,
        /readonly-string-literal-record requires distinct direct string-literal keys at Root\.probability/u,
      ],
      [
        `type ProbabilityKey = "atLeast0_1" | "atLeast1_0";
export interface Root {
  readonly probability: Readonly<Record<ProbabilityKey, string>>;
}
`,
        /readonly-string-literal-record requires distinct direct string-literal keys at Root\.probability/u,
      ],
      [
        `export interface Root {
  readonly probability: Readonly<Record<"atLeast0_1" | "atLeast0_1", string>>;
}
`,
        /readonly-string-literal-record requires distinct direct string-literal keys at Root\.probability/u,
      ],
      [
        `export interface Root {
  readonly probability: Readonly<Record<"atLeast0_1", number>>;
}
`,
        /readonly-string-literal-record requires Readonly<Record<"field" \| \.\.\., string>> at Root\.probability/u,
      ],
      [
        `export interface Root {
  readonly probability: Readonly<Record<"at-least-0.1", string>>;
}
`,
        /readonly-string-literal-record requires distinct direct string-literal keys at Root\.probability/u,
      ],
    ];

    // evaluate each rejected spelling through the new fixed-key mapping
    for (const [invalidSource, expected] of invalidCases) {
      assert.throws(
        // retain one focused failure boundary per source grammar
        () => generateRustContracts(config, { "fixture.ts": invalidSource }),
        expected,
        invalidSource,
      );
    }
  },
);

test(
  "projects one pinned readonly inline object",
  // retain shallow utility projection, outer presence, and null independently
  () => {
    const config = fixtureConfig("Root", {
      authority: "readonly-inline-object",
      optional: "readonly-inline-object",
      optionalNullable: "readonly-inline-object",
    });
    const source = `export interface Root {
  readonly authority: Readonly<{
    readonly first: string;
    readonly second: boolean;
  }> | null;
  readonly optional?: Readonly<{ first: string }>;
  readonly optionalNullable?: Readonly<{ first: string }> | null;
}
`;
    const { rust } = generateRustContracts(config, { "fixture.ts": source });

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["authority", "Nullable<RootAuthority>"],
      ["optional", "FieldPresence<RootOptional>"],
      ["optionalNullable", "FieldPresence<Nullable<RootOptionalNullable>>"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootAuthority"), [
      ["first", "String"],
      ["second", "bool"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootOptional"), [
      ["first", "String"],
    ]);
    assert.deepEqual(
      rustStructFieldEntries(rust, "RootOptionalNullable"),
      [["first", "String"]],
    );
    assert.match(
      rust,
      /wire rule: readonly-inline-object, pinned shallow Readonly wrapper over explicit properties"\]\s*pub authority: Nullable<RootAuthority>,/u,
    );
    assert.match(
      rust,
      /wire rule: readonly-inline-object, pinned shallow Readonly wrapper over explicit properties"\]\s*pub optional: FieldPresence<RootOptional>,/u,
    );
    assert.match(
      rust,
      /wire rule: readonly-inline-object, pinned shallow Readonly wrapper over explicit properties"\]\s*pub optionalNullable: FieldPresence<Nullable<RootOptionalNullable>>,/u,
    );
    const exactWrapper =
      /readonly-inline-object requires Readonly<\{ \.\.\. \}> or exact null union at Root\.authority/u;
    const invalidAuthorities = [
      ["", "{ readonly first: string } | null", exactWrapper],
      ["", "Readonly<{ readonly first: string }, string>", exactWrapper],
      [
        "type Readonly<T> = T; ",
        "Readonly<{ readonly first: string }>",
        /readonly-inline-object utility identity unsupported at Root\.authority/u,
      ],
      [
        "",
        "Readonly<{ first: { nested: string } }>",
        /non-readonly property unsupported at Root/u,
      ],
    ];

    // reject different wrappers or mutability below the shallow projection
    for (const [prefix, authority, expected] of invalidAuthorities) {
      assert.throws(
        // evaluate the same readonly mapping for each source form
        () => generateRustContracts(config, {
          "fixture.ts": `${prefix}export interface Root { readonly authority: ${authority}; }\n`,
        }),
        expected,
        authority,
      );
    }
  },
);

test(
  "admits only required readonly arrays of pinned shallow Readonly objects",
  // preserve ordered shallow element projection without widening array grammar
  () => {
    const config = fixtureConfig("Root", {
      entries: "readonly-inline-object-array",
    });
    const source = `export interface Root {
  readonly entries: readonly Readonly<{
    first: string;
    second: boolean;
  }>[];
}
`;
    const { rust } = generateRustContracts(config, { "fixture.ts": source });

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["entries", "Vec<RootEntries>"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootEntries"), [
      ["first", "String"],
      ["second", "bool"],
    ]);
    assert.match(
      rustStructFieldBlock(rust, "Root", "entries"),
      /wire rule: readonly-inline-object-array, ordered shallow Readonly inline objects/u,
    );

    const requiredArray =
      /readonly-inline-object-array requires a required readonly Readonly<\{ \.\.\. \}>\[\] field at Root\.entries/u;
    const wrappedElements =
      /readonly-inline-object-array requires Readonly<\{ \.\.\. \}> elements at Root\.entries/u;
    const invalidSources = [
      {
        name: "an optional field",
        source: `export interface Root {
  readonly entries?: readonly Readonly<{ first: string }>[];
}
`,
        error: requiredArray,
      },
      {
        name: "an aliased array",
        source: `type Entries = readonly Readonly<{ first: string }>[];
export interface Root { readonly entries: Entries; }
`,
        error: requiredArray,
      },
      {
        name: "a mutable array",
        source: `export interface Root {
  readonly entries: Readonly<{ first: string }>[];
}
`,
        error: requiredArray,
      },
      {
        name: "a ReadonlyArray wrapper",
        source: `export interface Root {
  readonly entries: ReadonlyArray<Readonly<{ first: string }>>;
}
`,
        error: requiredArray,
      },
      {
        name: "a nullable array union",
        source: `export interface Root {
  readonly entries: readonly Readonly<{ first: string }>[] | null;
}
`,
        error: requiredArray,
      },
      {
        name: "an aliased element",
        source: `type Entry = Readonly<{ first: string }>;
export interface Root { readonly entries: readonly Entry[]; }
`,
        error: wrappedElements,
      },
      {
        name: "an unwrapped element",
        source: `export interface Root {
  readonly entries: readonly { readonly first: string }[];
}
`,
        error: wrappedElements,
      },
      {
        name: "mixed element alternatives",
        source: `export interface Root {
  readonly entries: readonly (
    Readonly<{ first: string }> | Readonly<{ second: string }>
  )[];
}
`,
        error: wrappedElements,
      },
      {
        name: "a local Readonly shadow",
        source: `type Readonly<T> = T;
export interface Root {
  readonly entries: readonly Readonly<{ first: string }>[];
}
`,
        error:
          /readonly-inline-object-array utility identity unsupported at Root\.entries/u,
      },
      {
        name: "nested mutable state",
        source: `export interface Root {
  readonly entries: readonly Readonly<{
    nested: { value: string };
  }>[];
}
`,
        error: /non-readonly property unsupported at Root/u,
      },
      {
        name: "a nested Readonly wrapper",
        source: `export interface Root {
  readonly entries: readonly Readonly<{
    nested: Readonly<{ value: string }>;
  }>[];
}
`,
        error: /configured type reference unsupported at Root\.entries\.nested/u,
      },
    ];

    // reject every optional, aliased, mutable, widened, or deeper wrapper form
    for (const invalid of invalidSources) {
      assert.throws(
        // evaluate one rejected source through the exact array mapping
        () => generateRustContracts(config, { "fixture.ts": invalid.source }),
        invalid.error,
        invalid.name,
      );
    }
  },
);

test(
  "reuses one configured direct readonly interface field type",
  // bind the lookup to the same required field on one exact public interface
  () => {
    const config = {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {},
          fieldProjections: {
            item: { kind: "interface-field", root: "Base" },
          },
        },
      ],
    };
    const source = `export interface Base {
  readonly item: { readonly code: string };
  readonly other: string;
}
export interface Root {
  readonly item: Base["item"];
}
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "BaseItem"), [
      ["code", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["item", "BaseItem"],
    ]);
    assert.match(
      rust,
      /field projection: interface-field -> Base"\]\s*pub item: BaseItem,/u,
    );
    assert.equal(rust.match(/pub struct BaseItem \{/gu)?.length, 1);
    assert.deepEqual(manifest.roots[1], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-interface",
      fieldProjections: {
        item: { kind: "interface-field", root: "Base" },
      },
    });
    assert.throws(
      // reject an indexed lookup that does not select the current field
      () => generateRustContracts(
        config,
        {
          "fixture.ts": source.replace('Base["item"]', 'Base["other"]'),
        },
      ),
      /interface-field projection shape unsupported at Root\.item/u,
    );
    const baseField = "readonly item: { readonly code: string };";
    const projectionFirst = { ...config, roots: config.roots.toReversed() };
    const targetField = /interface-field target field unsupported at Root\.item/u;
    const invalidTargets = [
      [source.replace(baseField, "item: { readonly code: string };"), targetField],
      [source.replace(baseField, "readonly item?: { readonly code: string };"), targetField],
      [
        source
          .replace("export interface Base {", `interface Parent { ${baseField} }\nexport interface Base extends Parent {`)
          .replace(`\n  ${baseField}`, ""),
        targetField,
      ],
      [
        source.replace("export interface Base {", "export type Base = {"),
        /interface-field target identity unsupported at Root\.item/u,
      ],
    ];

    // exercise the projection before rendering an invalid target as a root
    for (const [invalidSource, expected] of invalidTargets) {
      assert.throws(
        // reject targets that cannot supply the exact reusable field
        () => generateRustContracts(projectionFirst, { "fixture.ts": invalidSource }),
        expected,
        invalidSource,
      );
    }
  },
);

test(
  "projects the same literal values from a separately configured tagged union",
  // preserve the existing projection while allowing an exact tagged source root
  () => {
    const projection = { sourceAlias: "Loader", field: "reason" };
    const source = `export type Loader =
  | { readonly state: "active"; readonly reason: null }
  | { readonly state: "disabled"; readonly reason: "loader_failure" };
export type Root = Loader["reason"] | "direct";
`;
    const loader = { source: "fixture.ts", exportName: "Loader", fields: {} };
    const tagged = literalUnionFixtureConfig(projection, [
      { ...loader, discriminator: "state" },
    ]);
    const { rust: onlyProjection } = generateRustContracts(
      literalUnionFixtureConfig(projection),
      { "fixture.ts": source },
    );
    const { rust } = generateRustContracts(tagged, { "fixture.ts": source });

    assert.deepEqual(rustStringEnumWireEntries(rust, "RootValue"), [
      ["loader_failure", "LoaderFailure"],
      ["direct", "Direct"],
    ]);
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "RootValue"),
      rustStringEnumWireEntries(onlyProjection, "RootValue"),
    );
    assert.match(rust, /pub type Root = Nullable<RootValue>;/u);
    assert.match(rust, /pub enum Loader \{\s*#\[doc = "wire discriminant state:/u);
    assert.throws(
      // retain the existing rejection of ordinary configured source aliases
      () => generateRustContracts(
        literalUnionFixtureConfig(projection, [loader]),
        { "fixture.ts": source },
      ),
      /literal union source alias identity unsupported at root Root/u,
    );
    assert.throws(
      // record the projected source dependency so feedback to this root is rejected
      () => generateRustContracts(tagged, {
        "fixture.ts": source.replace(
          'readonly reason: "loader_failure"',
          'readonly payload: Root; readonly reason: "loader_failure"',
        ),
      }),
      /recursive configured type reference cycle: Root -> Loader -> Root/u,
    );
  },
);

test(
  "emits the Rust 2024-compatible raw field name for gen",
  // preserve the wire field without emitting a reserved identifier
  () => {
    const source = "export interface Root { readonly gen: string; }\n";
    const { rust } = generateRustContracts(
      fixtureConfig("Root"),
      { "fixture.ts": source },
    );

    assert.match(rust, /pub r#gen: String/);
  },
);

test(
  "preserves own JSON __proto__ field metadata through normalization",
  // keep the metadata key as ordinary configuration data
  () => {
    const fields = JSON.parse('{"__proto__":"utc-iso"}');
    const config = fixtureConfig("Root", fields);
    const source = "export interface Root { readonly __proto__: string; }\n";
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.match(rust, /wire rule: utc-iso/);
    assert.match(rust, /pub __proto__: String/);
    assert.equal(manifest.config.sha256, sha256(canonicalJson(config)));
  },
);

test(
  "resolves acyclic configured same-module references through field wrappers",
  // preserve declaration identity under every supported wrapper
  () => {
    const source = `export interface Leaf {
  readonly state: "ready" | "waiting";
}
export interface Branch {
  readonly leaf: Leaf;
}
export interface Root {
  readonly branch: Branch;
  readonly nullableBranch: Branch | null;
  readonly optionalBranch?: Branch;
  readonly optionalNullableBranch?: Branch | null;
  readonly branches: readonly Branch[];
}
`;
    const { rust } = generateRustContracts(
      fixtureRoots("Leaf", "Branch", "Root"),
      { "fixture.ts": source },
    );

    assert.match(rust, /pub struct Branch \{\s*pub leaf: Leaf,\s*\}/);
    assert.match(rust, /pub branch: Branch/);
    assert.match(rust, /pub nullableBranch: Nullable<Branch>/);
    assert.match(rust, /pub optionalBranch: FieldPresence<Branch>/);
    assert.match(
      rust,
      /pub optionalNullableBranch: FieldPresence<Nullable<Branch>>/,
    );
    assert.match(rust, /pub branches: Vec<Branch>/);
  },
);

test(
  "projects one local closed const string tuple in exact source order",
  // retain the tuple's literal identities without widening or sorting
  () => {
    const source = `const ROOT_VALUES = [
  "zeta",
  "alpha-beta",
  "gamma_3",
] as const;
export type Root = (typeof ROOT_VALUES)[number];
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      fixtureConfig("Root"),
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.match(
      rust,
      /pub enum Root \{\s*#\[doc = "wire value: zeta"\]\s*Zeta,\s*#\[doc = "wire value: alpha-beta"\]\s*AlphaBeta,\s*#\[doc = "wire value: gamma_3"\]\s*Gamma3,\s*\}/,
    );
    assert.match(
      rust,
      /impl Root \{\s*pub const WIRE_VALUES: &'static \[\(&'static str, Self\)\] = &\[\s*\("zeta", Self::Zeta\),\s*\("alpha-beta", Self::AlphaBeta\),\s*\("gamma_3", Self::Gamma3\),\s*\];\s*\}/,
    );
    assert.equal(manifest.roots[0].kind, "string-literal-enum");
  },
);

test(
  "projects an opted-in satisfies string tuple without interpreting its named guard",
  // preserve only the exact tuple literals and their syntactic guard
  () => {
    const source = `type ValueName = string;
const ROOT_VALUES = [
  "zeta",
  "AlphaCase",
  "gamma_3",
] as const satisfies readonly ValueName[];
export type Root = (typeof ROOT_VALUES)[number];
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStringEnumWireEntries(rust, "Root"), [
      ["zeta", "Zeta"],
      ["AlphaCase", "AlphaCase"],
      ["gamma_3", "Gamma3"],
    ]);
    assert.deepEqual(manifest.roots[0], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "string-literal-enum",
      catalog: { kind: "satisfies-string-tuple" },
    });
  },
);

test(
  "projects an opted-in object catalog string property in exact row order",
  // retain selected strings while accepting only closed scalar row metadata
  () => {
    const source = `const ROOT_ROWS = [
  { key: "zeta", label: "first", maximumHours: 24, minimumHours: 0 },
  { key: "AlphaCase", label: "second", maximumHours: 9007199254740991, minimumHours: 25 },
  { key: "gamma_3", label: "third", maximumHours: 72, minimumHours: 49 },
] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStringEnumWireEntries(rust, "Root"), [
      ["zeta", "Zeta"],
      ["AlphaCase", "AlphaCase"],
      ["gamma_3", "Gamma3"],
    ]);
    assert.deepEqual(manifest.roots[0], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "string-literal-enum",
      catalog: { kind: "string-property", key: "key" },
    });
  },
);

test(
  "projects explicitly tagged readonly arms as distinct nominal payloads",
  // preserve each arm shape and exact null semantics
  () => {
    const source = `export type Root =
  | {
      readonly exactNull: null;
      readonly kind: "with_value";
      readonly value: string;
    }
  | {
      readonly enabled: boolean;
      readonly kind: null;
      readonly nullable: string | null;
    };
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      taggedFixtureConfig("Root", "kind"),
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.match(
      rust,
      /pub enum Root \{\s*#\[doc = "wire discriminant kind: \\"with_value\\""\]\s*WithValue\(RootWithValue\),\s*#\[doc = "wire discriminant kind: null"\]\s*Null\(RootNull\),\s*\}/,
    );
    assert.match(
      rust,
      /pub struct RootWithValue \{\s*pub exactNull: RootWithValueExactNullNull,\s*pub kind: RootWithValueKindLiteral,\s*pub value: String,\s*\}/,
    );
    assert.match(
      rust,
      /pub struct RootNull \{\s*pub enabled: bool,\s*pub kind: RootNullKindNull,\s*pub nullable: Nullable<String>,\s*\}/,
    );
    assert.match(
      rust,
      /#\[doc = "exact null wire literal"\]\s*#\[derive\(Debug, Clone, Copy, PartialEq, Eq\)\]\s*pub struct RootWithValueExactNullNull;/,
    );
    assert.doesNotMatch(rust, /pub exactNull: Nullable</);
    assert.deepEqual(manifest.roots[0], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "tagged-readonly-object-union",
      discriminator: {
        field: "kind",
        variants: [
          {
            rustName: "WithValue",
            payloadRustName: "RootWithValue",
            wireValue: "with_value",
          },
          {
            rustName: "Null",
            payloadRustName: "RootNull",
            wireValue: null,
          },
        ],
      },
    });
  },
);

test(
  "projects only pinned shallow Readonly arms for opted-in tagged unions",
  // admit the three reviewed scalar mappings without widening the tagged grammar
  () => {
    const config = readonlyTaggedFixtureConfig({
      "[first].createdAt": "utc-iso",
      "[first].count": "safe-unsigned-integer",
      "[first].attempt": "unsigned-safe-integer-enum",
      "[second].finishedAt": "utc-iso",
    });
    const source = `export type Root =
  | Readonly<{
      kind: "first";
      createdAt: string;
      count: number;
      attempt: 1 | 2;
      child: { readonly value: string };
    }>
  | Readonly<{
      kind: "second";
      finishedAt: string;
    }>;
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "RootFirst"), [
      ["kind", "RootFirstKindLiteral"],
      ["createdAt", "String"],
      ["count", "u64"],
      ["attempt", "RootFirstAttemptEnum"],
      ["child", "RootFirstChild"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootFirstChild"), [
      ["value", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootSecond"), [
      ["kind", "RootSecondKindLiteral"],
      ["finishedAt", "String"],
    ]);
    assert.deepEqual(
      rustUnsignedIntegerEnumWireEntries(rust, "RootFirstAttemptEnum"),
      [
        [1, "Value1"],
        [2, "Value2"],
      ],
    );
    assert.deepEqual(manifest.roots[0], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "tagged-readonly-object-union",
      readonlyTaggedUnion: true,
      discriminator: {
        field: "kind",
        variants: [
          {
            rustName: "First",
            payloadRustName: "RootFirst",
            wireValue: "first",
          },
          {
            rustName: "Second",
            payloadRustName: "RootSecond",
            wireValue: "second",
          },
        ],
      },
    });

    const simpleSource = `export type Root =
  | Readonly<{ kind: "first"; value: string }>
  | Readonly<{ kind: "second"; reason: string }>;
`;
    const invalidSources = [
      {
        name: "one unwrapped arm",
        source: simpleSource.replace(
          'Readonly<{ kind: "second"; reason: string }>',
          '{ kind: "second"; reason: string }',
        ),
        error: /readonly-tagged-union requires Readonly<\{ \.\.\. \}> arms at Root/u,
      },
      {
        name: "one named wrapped arm",
        source: `type First = { kind: "first"; value: string };
export type Root =
  | Readonly<First>
  | Readonly<{ kind: "second"; reason: string }>;
`,
        error: /readonly-tagged-union requires Readonly<\{ \.\.\. \}> arms at Root/u,
      },
      {
        name: "a local Readonly shadow",
        source: `type Readonly<T> = T;
${simpleSource}`,
        error: /readonly-tagged-union utility identity unsupported at Root/u,
      },
      {
        name: "nested mutable state",
        source: `export type Root =
  | Readonly<{ kind: "first"; nested: { value: string } }>
  | Readonly<{ kind: "second"; reason: string }>;
`,
        error: /non-readonly property unsupported at Root/u,
      },
      {
        name: "a missing discriminator",
        source: `export type Root =
  | Readonly<{ kind: "first"; value: string }>
  | Readonly<{ reason: string }>;
`,
        error: /tagged union requires exactly one discriminator property at Root\.kind/u,
      },
      {
        name: "an exact null discriminator",
        source: `export type Root =
  | Readonly<{ kind: "first"; value: string }>
  | Readonly<{ kind: null; reason: string }>;
`,
        error: /readonly-tagged union discriminator must be one exact string literal at root Root/u,
      },
    ];

    // reject wrappers or nested mutability outside the exact shallow grammar
    for (const invalid of invalidSources) {
      assert.throws(
        // generate each invalid source with the same empty opted-in grammar
        () => generateRustContracts(
          readonlyTaggedFixtureConfig(),
          { "fixture.ts": invalid.source },
        ),
        invalid.error,
        invalid.name,
      );
    }

    const routingSource = `export type Root =
  | Readonly<{ kind: "first"; value: string }>
  | Readonly<{ kind: "second"; reason: string }>;
`;
    const invalidConfigs = [
      {
        name: "a false opt-in",
        config: {
          ...readonlyTaggedFixtureConfig(),
          roots: [{
            source: "fixture.ts",
            exportName: "Root",
            discriminator: "kind",
            readonlyTaggedUnion: false,
            fields: {},
          }],
        },
        error: /config\.roots\[0\]\.readonlyTaggedUnion requires true, a discriminator, and no base/u,
      },
      {
        name: "an opt-in without a discriminator",
        config: {
          ...readonlyTaggedFixtureConfig(),
          roots: [{
            source: "fixture.ts",
            exportName: "Root",
            readonlyTaggedUnion: true,
            fields: {},
          }],
        },
        error: /config\.roots\[0\]\.readonlyTaggedUnion requires true, a discriminator, and no base/u,
      },
      {
        name: "an opt-in with a base",
        config: {
          ...readonlyTaggedFixtureConfig(),
          roots: [{
            source: "fixture.ts",
            exportName: "Root",
            discriminator: "kind",
            readonlyTaggedUnion: true,
            base: "Base",
            fields: {},
          }],
        },
        error: /config\.roots\[0\]\.readonlyTaggedUnion requires true, a discriminator, and no base/u,
      },
      {
        name: "an unreviewed arm mapping",
        config: readonlyTaggedFixtureConfig({ "[first].value": "finite-f64" }),
        error: /config\.roots\[0\]\.fields\.\[first\]\.value must use utc-iso, safe-unsigned-integer, unsigned-safe-integer-enum, or readonly-inline-object/u,
      },
      {
        name: "an unqualified arm mapping",
        config: readonlyTaggedFixtureConfig({ value: "utc-iso" }),
        error: /config\.roots\[0\]\.fields has invalid field path "value"/u,
      },
      {
        name: "a mapping bound to the wrong arm",
        config: readonlyTaggedFixtureConfig({ "[second].value": "utc-iso" }),
        error: /tagged union field mapping targets no field at Root\.\[second\]\.value/u,
      },
      {
        name: "a mapping bound to no arm",
        config: readonlyTaggedFixtureConfig({ "[missing].value": "utc-iso" }),
        error: /tagged union field mapping targets no arm at Root\.\[missing\]\.value/u,
      },
    ];

    // reject invalid opt-ins and any mapping outside one exact qualified arm
    for (const invalid of invalidConfigs) {
      assert.throws(
        // generate the same ordinary tagged source under each invalid configuration
        () => generateRustContracts(
          invalid.config,
          { "fixture.ts": routingSource },
        ),
        invalid.error,
        invalid.name,
      );
    }
  },
);

test(
  "anchors nested readonly tagged mappings to one explicit inline object",
  // preserve one shallow wrapper and only the three reviewed scalar mappings below it
  () => {
    const config = readonlyTaggedFixtureConfig({
      "[present].gap": "readonly-inline-object",
      "[present].gap.createdAt": "utc-iso",
      "[present].gap.count": "safe-unsigned-integer",
      "[present].gap.attempt": "unsigned-safe-integer-enum",
    });
    const source = `export type Root =
  | Readonly<{ kind: "absent" }>
  | Readonly<{
      kind: "present";
      gap: Readonly<{
        createdAt: string;
        count: number;
        attempt: 1 | 2;
        note: string;
      }>;
    }>;
`;
    const { rust } = generateRustContracts(config, { "fixture.ts": source });

    assert.deepEqual(rustStructFieldEntries(rust, "RootPresent"), [
      ["kind", "RootPresentKindLiteral"],
      ["gap", "RootPresentGap"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootPresentGap"), [
      ["createdAt", "String"],
      ["count", "u64"],
      ["attempt", "RootPresentGapAttemptEnum"],
      ["note", "String"],
    ]);
    assert.deepEqual(
      rustUnsignedIntegerEnumWireEntries(rust, "RootPresentGapAttemptEnum"),
      [
        [1, "Value1"],
        [2, "Value2"],
      ],
    );
    assert.match(
      rustStructFieldBlock(rust, "RootPresent", "gap"),
      /wire rule: readonly-inline-object/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "RootPresentGap", "createdAt"),
      /wire rule: utc-iso/u,
    );
    assert.match(
      rustStructFieldBlock(rust, "RootPresentGap", "count"),
      /wire rule: safe-unsigned-integer/u,
    );

    assert.throws(
      // reject a nested mapping without its exact same-arm parent wrapper
      () => generateRustContracts(
        readonlyTaggedFixtureConfig({
          "[present].gap.createdAt": "utc-iso",
        }),
        { "fixture.ts": source },
      ),
      /requires one direct readonly-inline-object parent/u,
    );
    assert.throws(
      // reject any attempt to open a second mapped wrapper level
      () => generateRustContracts(
        readonlyTaggedFixtureConfig({
          "[present].gap": "readonly-inline-object",
          "[present].gap.createdAt": "readonly-inline-object",
        }),
        { "fixture.ts": source },
      ),
      /cannot wrap a nested tagged field/u,
    );
  },
);

test(
  "projects a configured base into every tagged intersection payload",
  // preserve base-first field order and arm-qualified mappings
  () => {
    const source = `export interface Base {
  readonly count: number;
  readonly label: string;
}
export type OtherKey = "other" | "alternate";
const FIRST_KEY = "first-key" as const;
export type Root =
  Base &
    (
      | {
          readonly key: typeof FIRST_KEY;
          readonly kind: "first";
        }
      | {
          readonly key: OtherKey;
          readonly kind: "second";
        }
    );
`;
    const { rust, manifest: manifestText } = generateRustContracts(
      taggedBaseFixtureConfig(),
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);
    const rootManifest = manifest.roots.find(
      // select the tagged root by its exact source identity
      ({ source: rootSource, exportName }) =>
        rootSource === "fixture.ts" && exportName === "Root",
    );

    assert.deepEqual(rustStructFieldEntries(rust, "Base"), [
      ["count", "u64"],
      ["label", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootFirst"), [
      ["count", "u64"],
      ["label", "String"],
      ["key", "RootFirstKeyLiteral"],
      ["kind", "RootFirstKindLiteral"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootSecond"), [
      ["count", "u64"],
      ["label", "String"],
      ["key", "OtherKey"],
      ["kind", "RootSecondKindLiteral"],
    ]);
    assert.match(
      rust,
      /impl RootFirstKeyLiteral \{\s*pub const WIRE_VALUE: &'static str = "first-key";\s*\}/u,
    );
    assert.deepEqual(rootManifest, {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "tagged-readonly-object-union",
      base: "Base",
      discriminator: {
        field: "kind",
        variants: [
          {
            rustName: "First",
            payloadRustName: "RootFirst",
            wireValue: "first",
          },
          {
            rustName: "Second",
            payloadRustName: "RootSecond",
            wireValue: "second",
          },
        ],
      },
    });
  },
);

test(
  "projects a forward-configured named readonly object union in source arm order",
  // reuse configured payload identities without inferring a wire tag
  () => {
    const source = `export interface FirstPayload {
  readonly first: string;
}
export interface SecondPayload {
  readonly second: boolean;
}
export interface ThirdPayload {
  readonly third: string;
}
export type Root = ThirdPayload | FirstPayload | SecondPayload;
`;
    const config = objectUnionFixtureConfig({
      SecondPayload: "Second",
      ThirdPayload: "Third",
      FirstPayload: "First",
    });
    const { rust, manifest: manifestText } = generateRustContracts(
      config,
      { "fixture.ts": source },
    );
    const manifest = JSON.parse(manifestText);
    const rootDefinition = rust.match(/^pub enum Root \{\n[\s\S]+?^\}/mu);

    assert.ok(rootDefinition);
    assert.equal(
      rootDefinition[0],
      `pub enum Root {
    Third(ThirdPayload),
    First(FirstPayload),
    Second(SecondPayload),
}`,
    );
    assert.equal(
      Array.from(rust.matchAll(/^pub struct FirstPayload \{/gmu)).length,
      1,
    );
    assert.equal(
      Array.from(rust.matchAll(/^pub struct SecondPayload \{/gmu)).length,
      1,
    );
    assert.equal(
      Array.from(rust.matchAll(/^pub struct ThirdPayload \{/gmu)).length,
      1,
    );
    assert.doesNotMatch(
      rust,
      /impl Root \{[\s\S]*(?:DISCRIMINATOR_FIELD|WIRE_VALUES)/,
    );
    assert.deepEqual(manifest.roots[0], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "named-readonly-object-union",
      objectUnion: {
        variants: [
          { rustName: "Third", payloadRustName: "ThirdPayload" },
          { rustName: "First", payloadRustName: "FirstPayload" },
          { rustName: "Second", payloadRustName: "SecondPayload" },
        ],
      },
    });
  },
);

test(
  "emits stable named object-union bytes across mapping insertion order",
  // canonicalize mapping keys while retaining source union arm order
  () => {
    const source = `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: boolean; }
export interface ThirdPayload { readonly third: string; }
export type Root = ThirdPayload | FirstPayload | SecondPayload;
`;
    const forward = objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
      ThirdPayload: "Third",
    });
    const permuted = objectUnionFixtureConfig({
      ThirdPayload: "Third",
      FirstPayload: "First",
      SecondPayload: "Second",
    });

    assert.deepEqual(
      generateRustContracts(permuted, { "fixture.ts": source }),
      generateRustContracts(forward, { "fixture.ts": source }),
    );
  },
);

test(
  "projects imported catalog arrays, return enums, and direct partial metric records",
  // prove named value imports syntactically without executing their declarations
  () => {
    const { rust, manifest: manifestText } = generateRustContracts(
      projectedFixtureConfig(),
      projectedFixtureSources(),
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["adjustedMetrics", "RootAdjustedMetricsRecord"],
      ["appliedMetrics", "Vec<Metric>"],
      ["band", "Band"],
      ["leadBand", "Band"],
      ["metric", "Metric"],
    ]);
    assert.match(
      rust,
      /pub struct RootAdjustedMetricsRecord\(pub Vec<\(Metric, f64\)>\);/,
    );
    assert.match(
      rust,
      /source shape: Partial<Record<Metric, number>>; wire shape: JSON object/,
    );
    assert.deepEqual(manifest.roots[2], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-interface",
      fieldProjections: {
        appliedMetrics: { kind: "const-catalog-array", root: "Metric" },
        leadBand: { kind: "return-enum", root: "Band" },
      },
    });
  },
);

test(
  "projects a direct readonly Omit interface array with retained field metadata",
  // omit one storage-owned field while preserving source order and field rules
  () => {
    const { rust, manifest: manifestText } = generateRustContracts(
      omitInterfaceArrayFixtureConfig(),
      { "fixture.ts": omitInterfaceArrayFixtureSource() },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "RootHoursEntry"), [
      ["count", "u64"],
      ["value", "f64"],
      ["validAt", "String"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["hours", "Vec<RootHoursEntry>"],
    ]);
    const entryDefinition = rust.match(
      /^pub struct RootHoursEntry \{\n[\s\S]+?^\}/mu,
    );
    assert.ok(entryDefinition);
    assert.doesNotMatch(entryDefinition[0], /contentHash/u);
    assert.match(
      entryDefinition[0],
      /wire rule: safe-unsigned-integer[^\n]*\n\s*pub count: u64,/u,
    );
    assert.match(
      entryDefinition[0],
      /wire rule: finite-f64[^\n]*\n\s*pub value: f64,/u,
    );
    assert.deepEqual(manifest.roots[1], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-interface",
      fieldProjections: {
        hours: { kind: "omit-interface-array", root: "Hour" },
      },
    });
  },
);

test(
  "projects indexed literals through named aliases, intersections, and Extract",
  // retain observed source order while deduplicating repeated literals and null
  () => {
    const { rust, manifest: manifestText } = generateRustContracts(
      indexedLiteralFixtureConfig(),
      { "fixture.ts": indexedLiteralFixtureSource() },
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["reasonCode", "Nullable<RootReasonCodeEnum>"],
      ["state", "RootStateEnum"],
    ]);
    assert.deepEqual(rustStringEnumWireEntries(rust, "RootReasonCodeEnum"), [
      ["first", "First"],
      ["second", "Second"],
      ["third", "Third"],
    ]);
    assert.deepEqual(rustStringEnumWireEntries(rust, "RootStateEnum"), [
      ["active", "Active"],
      ["disabled", "Disabled"],
    ]);
    assert.deepEqual(rustStringEnumWireEntries(rust, "ReasonCode"), [
      ["first", "First"],
      ["second", "Second"],
      ["third", "Third"],
      ["unobserved", "Unobserved"],
    ]);
    assert.doesNotMatch(rust, /not_projected|also_not_projected/u);
    assert.deepEqual(manifest.roots[1], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-interface",
      indexedLiteralSources: {
        LoadedRuntime: "fixture.ts",
        LoadedRuntimeV1: "fixture.ts",
      },
      fieldProjections: {
        reasonCode: { kind: "indexed-literals", root: "LoadedRuntime" },
        state: { kind: "indexed-literals", root: "LoadedRuntime" },
      },
    });
  },
);

test(
  "projects a nested indexed literal through source-only compiled package proof",
  // prove the package graph and interface selector without reading built declarations
  () => {
    const fixture = compiledProofFixture();
    const { rust, manifest: manifestText } = generateRustContracts(
      fixture.config,
      fixture.sources,
    );
    const manifest = JSON.parse(manifestText);

    assert.deepEqual(rustStructFieldEntries(rust, "Root"), [
      ["source", "RootSource"],
    ]);
    assert.deepEqual(rustStructFieldEntries(rust, "RootSource"), [
      ["stateStatus", "RootSourceStateStatusEnum"],
    ]);
    assert.deepEqual(
      rustStringEnumWireEntries(rust, "RootSourceStateStatusEnum"),
      [
        ["supported", "Supported"],
        ["cold", "Cold"],
        ["insufficient", "Insufficient"],
        ["invalid", "Invalid"],
      ],
    );
    assert.doesNotMatch(rust, /pub struct Collector(?:Run|Status)\b/u);
    assert.deepEqual(manifest.roots[0], {
      source: "fixture.ts",
      exportName: "Root",
      rustName: "Root",
      kind: "readonly-interface",
      indexedLiteralSources: {
        CollectorRun: "fixture-database/src/collector.ts",
        CollectorStatus: "fixture-database/src/collector.ts",
      },
      fieldProjections: {
        "source.stateStatus": {
          kind: "indexed-literals",
          root: "CollectorStatus",
        },
      },
    });
    assert.deepEqual(manifest.typeImports, [{
      consumer: "fixture.ts",
      specifier: "@fixture/database",
      packageJson: "fixture-database/package.json",
      entry: "fixture-database/src/index.ts",
      leaves: ["fixture-database/src/collector.ts"],
      compiledSource: {
        tsconfig: "fixture-database/tsconfig.json",
        base: "tsconfig.base.json",
      },
      exports: {},
      proofExports: {
        CollectorStatus: "fixture-database/src/collector.ts",
      },
    }]);
  },
);

const REJECTED_SUBSET_CASES = [
  {
    name: "recursive JSON metadata naming its own root",
    source: "export type JsonValue = null;\n",
    config: {
      ...recursiveJsonFixtureConfig(),
      roots: [{
        source: "fixture.ts",
        exportName: "JsonValue",
        recursiveJson: "JsonValue",
        fields: {},
      }],
    },
    error: /config\.roots\[0\]\.recursiveJson must name a distinct ASCII identifier/,
  },
  {
    name: "recursive JSON metadata combined with a field mapping",
    source: "export type JsonValue = null;\n",
    config: {
      ...recursiveJsonFixtureConfig(),
      roots: [{
        source: "fixture.ts",
        exportName: "JsonValue",
        recursiveJson: "JsonPrimitive",
        fields: { value: "finite-f64" },
      }],
    },
    error: /recursiveJson metadata cannot be combined with other root metadata at config\.roots\[0\]/,
  },
  {
    name: "a recursive JSON primitive alias missing null",
    source: `export type JsonPrimitive = boolean | number | string;
export type JsonValue =
  | JsonPrimitive
  | readonly JsonValue[]
  | { readonly [key: string]: JsonValue };
`,
    config: recursiveJsonFixtureConfig(),
    error: /recursive JSON primitive alias unsupported at root JsonValue/,
  },
  {
    name: "a direct recursive JSON union arm",
    source: `export type JsonPrimitive = boolean | number | string | null;
export type JsonValue =
  | JsonPrimitive
  | JsonValue
  | { readonly [key: string]: JsonValue };
`,
    config: recursiveJsonFixtureConfig(),
    error: /recursive JSON shape unsupported at root JsonValue/,
  },
  {
    name: "a mutable recursive JSON array",
    source: `export type JsonPrimitive = boolean | number | string | null;
export type JsonValue =
  | JsonPrimitive
  | JsonValue[]
  | { readonly [key: string]: JsonValue };
`,
    config: recursiveJsonFixtureConfig(),
    error: /recursive JSON shape unsupported at root JsonValue/,
  },
  {
    name: "an ordered JSON object with the primitive value target",
    source: `export type JsonPrimitive = boolean | number | string | null;
export type JsonValue =
  | JsonPrimitive
  | readonly JsonValue[]
  | { readonly [key: string]: JsonValue };
export interface Metadata {
  readonly value: Readonly<Record<string, JsonPrimitive>>;
}
`,
    config: recursiveJsonFixtureConfig([{
      source: "fixture.ts",
      exportName: "Metadata",
      fields: { value: "ordered-json-object" },
    }]),
    error: /ordered-json-object value root unsupported at Metadata\.value/,
  },
  {
    name: "a shadowed ordered JSON object utility",
    source: `type Readonly<T> = T;
type Record<K extends string, V> = { readonly [P in K]: V };
export type JsonPrimitive = boolean | number | string | null;
export type JsonValue =
  | JsonPrimitive
  | readonly JsonValue[]
  | { readonly [key: string]: JsonValue };
export interface Metadata {
  readonly value: Readonly<Record<string, JsonValue>>;
}
`,
    config: recursiveJsonFixtureConfig([{
      source: "fixture.ts",
      exportName: "Metadata",
      fields: { value: "ordered-json-object" },
    }]),
    error: /ordered-json-object utility identity unsupported at Metadata\.value/,
  },
  {
    name: "false private-alias metadata",
    source: `interface FirstPayload { readonly first: string; }
interface SecondPayload { readonly second: string; }
type Root = FirstPayload | SecondPayload;
`,
    config: {
      ...objectUnionFixtureConfig({ FirstPayload: "First", SecondPayload: "Second" }),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        localAlias: false,
        objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
        fields: {},
      }],
    },
    error: /config\.roots\[0\]\.localAlias must be true when present/,
  },
  {
    name: "false private-interface metadata",
    source: `interface Material { readonly value: string; }
export interface Root extends Material { readonly enabled: boolean; }
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        {
          source: "fixture.ts",
          exportName: "Material",
          localInterface: false,
          fields: {},
        },
        {
          source: "fixture.ts",
          exportName: "Root",
          base: "Material",
          fields: {},
        },
      ],
    },
    error: /config\.roots\[0\]\.localInterface must be true when present/,
  },
  {
    name: "an exported interface opted into private-interface handling",
    source: "export interface Root { readonly value: string; }\n",
    config: {
      ...fixtureConfig("Root"),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        localInterface: true,
        fields: {},
      }],
    },
    error: /local interface must not be exported fixture\.ts#Root/,
  },
  {
    name: "a private interface without an exported ordinary-base descendant",
    source: "interface Root { readonly value: string; }\nexport {};\n",
    config: {
      ...fixtureConfig("Root"),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        localInterface: true,
        fields: {},
      }],
    },
    error: /local interface is not an ordinary base of an exported root fixture\.ts#Root/,
  },
  {
    name: "a private interface reused as an ordinary field type",
    source: `interface Material { readonly value: string; }
export interface Root { readonly material: Material; }
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        {
          source: "fixture.ts",
          exportName: "Material",
          localInterface: true,
          fields: {},
        },
        { source: "fixture.ts", exportName: "Root", fields: {} },
      ],
    },
    error: /configured type reference identity unsupported at Root\.material/,
  },
  {
    name: "duplicate quoted wire fields targeting one Rust name",
    source: `export interface Band { readonly supported: boolean; }
interface Material {
  readonly strengthBands: {
    readonly "1-6": Band;
    readonly "7-12": Band;
  };
}
export interface Root extends Material { readonly month: string; }
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Band", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Material",
          localInterface: true,
          wireFieldNames: {
            "strengthBands.1-6": "hours1To6",
            "strengthBands.7-12": "hours1To6",
          },
          fields: {},
        },
        {
          source: "fixture.ts",
          exportName: "Root",
          base: "Material",
          fields: {},
        },
      ],
    },
    error: /duplicate wire field Rust name hours1To6 at config\.roots\[1\]\.wireFieldNames/,
  },
  {
    name: "a stale quoted wire-field path",
    source: `export interface Band { readonly supported: boolean; }
interface Material {
  readonly strengthBands: {
    readonly "1-6": Band;
    readonly "7-12": Band;
  };
}
export interface Root extends Material { readonly month: string; }
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Band", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Material",
          localInterface: true,
          wireFieldNames: {
            "strengthBands.1-6": "hours1To6",
            "strengthBands.2-3": "hours2To3",
            "strengthBands.7-12": "hours7To12",
          },
          fields: {},
        },
        {
          source: "fixture.ts",
          exportName: "Root",
          base: "Material",
          fields: {},
        },
      ],
    },
    error: /unused wire field mapping at Material\.strengthBands\.2-3/,
  },
  {
    name: "a quoted wire-field rename colliding with an ordinary Rust field",
    source: `export interface Band { readonly supported: boolean; }
interface Material {
  readonly strengthBands: {
    readonly hours1To6: Band;
    readonly "1-6": Band;
    readonly "7-12": Band;
  };
}
export interface Root extends Material { readonly month: string; }
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Band", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Material",
          localInterface: true,
          wireFieldNames: {
            "strengthBands.1-6": "hours1To6",
            "strengthBands.7-12": "hours7To12",
          },
          fields: {},
        },
        {
          source: "fixture.ts",
          exportName: "Root",
          base: "Material",
          fields: {},
        },
      ],
    },
    error: /Rust field name collision hours1To6 at Material\.strengthBands\.1-6/,
  },
  {
    name: "an unconfigured quoted range field",
    source: `export interface Root {
  readonly strengthBands: { readonly "1-6": string };
}
`,
    config: fixtureConfig("Root"),
    error: /unsupported property name at Root/,
  },
  {
    name: "private-alias metadata without a named object union",
    source: "type Root = \"first\" | \"second\";\n",
    config: {
      ...fixtureConfig("Root"),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        localAlias: true,
        fields: {},
      }],
    },
    error: /config\.roots\[0\]\.localAlias requires objectUnion metadata/,
  },
  {
    name: "an exported alias opted into private-alias handling",
    source: `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export type Root = FirstPayload | SecondPayload;
`,
    config: {
      ...objectUnionFixtureConfig({ FirstPayload: "First", SecondPayload: "Second" }),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        localAlias: true,
        objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
        fields: {},
      }],
    },
    error: /local alias must not be exported fixture\.ts#Root/,
  },
  {
    name: "a private alias re-exported through a named export list",
    source: `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
type Root = FirstPayload | SecondPayload;
export { Root };
`,
    config: {
      ...objectUnionFixtureConfig({ FirstPayload: "First", SecondPayload: "Second" }),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        localAlias: true,
        objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
        fields: {},
      }],
    },
    error: /local alias must not be exported fixture\.ts#Root/,
  },
  {
    name: "an empty field-projection record",
    source: "export interface Root { readonly value: string; }\n",
    config: {
      ...fixtureConfig("Root"),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        fields: {},
        fieldProjections: {},
      }],
    },
    error: /config\.roots\[0\]\.fieldProjections must be a non-empty plain object/,
  },
  {
    name: "a nested field-projection path",
    source: "export interface Root { readonly nested: { readonly value: string }; }\n",
    config: {
      ...fixtureConfig("Root"),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        fields: {},
        fieldProjections: {
          "nested.value": { kind: "return-enum", root: "Band" },
        },
      }],
    },
    error: /dotted field projection requires indexed-literals/,
  },
  {
    name: "an invalid dotted field-projection path",
    source: "export interface Root { readonly source: string; }\n",
    config: {
      ...fixtureConfig("Root"),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        fields: {},
        indexedLiteralSources: { CollectorStatus: "fixture.ts" },
        fieldProjections: {
          "source..stateStatus": {
            kind: "indexed-literals",
            root: "CollectorStatus",
          },
        },
      }],
    },
    error: /fieldProjections has invalid field path "source\.\.stateStatus"/,
  },
  {
    name: "an exact nested field mapping and projection overlap",
    ...compiledProofFixture({
      rootOverrides: {
        fields: { "source.stateStatus": "utc-iso" },
      },
    }),
    error: /field mapping conflicts with field projection/,
  },
  {
    name: "a nested field mapping prefix and projection overlap",
    ...compiledProofFixture({
      rootOverrides: {
        fields: { source: "utc-iso" },
      },
    }),
    error: /field mapping conflicts with field projection/,
  },
  {
    name: "a stale dotted indexed field projection",
    source: `interface CollectorStatus {
  readonly stateStatus: "supported" | "cold";
}
export interface Root {
  readonly source: { readonly stateStatus: "supported" | "cold" };
}
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        fields: {},
        indexedLiteralSources: { CollectorStatus: "fixture.ts" },
        fieldProjections: {
          "source.missing": {
            kind: "indexed-literals",
            root: "CollectorStatus",
          },
        },
      }],
    },
    error: /unused field projection at Root\.source\.missing/,
  },
  {
    name: "an inherited dotted indexed field projection",
    source: `interface CollectorStatus {
  readonly stateStatus: "supported" | "cold";
}
export interface Base {
  readonly source: {
    readonly stateStatus: CollectorStatus["stateStatus"];
  };
}
export interface Root extends Base { readonly own: string; }
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          base: "Base",
          fields: {},
          indexedLiteralSources: { CollectorStatus: "fixture.ts" },
          fieldProjections: {
            "source.stateStatus": {
              kind: "indexed-literals",
              root: "CollectorStatus",
            },
          },
        },
      ],
    },
    error: /unused field projection at Root\.source\.stateStatus/,
  },
  {
    name: "an unknown field-projection kind",
    source: "export interface Root { readonly value: string; }\n",
    config: {
      ...fixtureConfig("Root"),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        fields: {},
        fieldProjections: {
          value: { kind: "copied-enum", root: "Band" },
        },
      }],
    },
    error: /config\.roots\[0\]\.fieldProjections\.value\.kind is unsupported/,
  },
  {
    name: "an ordinary named enum without an exclude-literal source shape",
    source: `export type Metric = "first" | "second";
export interface Root { readonly metric: Metric; }
`,
    config: excludeLiteralFixtureConfig(),
    error: /exclude-literal projection shape unsupported at Root\.metric/,
  },
  {
    name: "a local alias instead of the pinned Exclude utility",
    source: `type Exclude<A, B> = A;
export type Metric = "first" | "second";
export interface Root { readonly metric: Exclude<Metric, "first">; }
`,
    config: excludeLiteralFixtureConfig(),
    error: /exclude-literal utility identity unsupported at Root\.metric/,
  },
  {
    name: "an exclusion literal that is not in the source enum",
    source: `export type Metric = "first" | "second";
export interface Root { readonly metric: Exclude<Metric, "third">; }
`,
    config: excludeLiteralFixtureConfig(),
    error: /exclude-literal value "third" is outside configured enum at Root\.metric/,
  },
  {
    name: "field metadata and a projection for the same direct property",
    source: "export interface Root { readonly value: string; }\n",
    config: {
      ...fixtureConfig("Root"),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        fields: { value: "utc-iso" },
        fieldProjections: {
          value: { kind: "return-enum", root: "Band" },
        },
      }],
    },
    error: /field mapping conflicts with field projection at config\.roots\[0\]\.value/,
  },
  {
    name: "field projections on a named object union",
    source: "export type Root = FirstPayload | SecondPayload;\n",
    config: {
      ...objectUnionFixtureConfig({ FirstPayload: "First", SecondPayload: "Second" }),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
        fields: {},
        fieldProjections: {
          value: { kind: "return-enum", root: "Band" },
        },
      }],
    },
    error: /object union field projections unsupported at config\.roots\[0\]/,
  },
  {
    name: "an unknown omitted interface-array field",
    source: omitInterfaceArrayFixtureSource(
      'readonly Omit<Hour, "missing">[]',
    ),
    config: omitInterfaceArrayFixtureConfig(),
    error: /omit-interface-array field unsupported at Root\.hours/,
  },
  {
    name: "an interface-array omission through a local base alias",
    source: omitInterfaceArrayFixtureSource(
      'readonly Omit<HourAlias, "contentHash">[]',
      "type HourAlias = Hour;\n",
    ),
    config: omitInterfaceArrayFixtureConfig(),
    error: /omit-interface-array base identity unsupported at Root\.hours/,
  },
  {
    name: "an interface-array omission naming the wrong configured base",
    source: omitInterfaceArrayFixtureSource(
      'readonly Omit<Other, "contentHash">[]',
      "export interface Other { readonly contentHash: string; readonly retained: string; }\n",
    ),
    config: omitInterfaceArrayFixtureConfig(),
    error: /omit-interface-array base identity unsupported at Root\.hours/,
  },
  {
    name: "a shadowed Omit utility for an interface array",
    source: omitInterfaceArrayFixtureSource(
      undefined,
      "type Omit<T, K> = T;\n",
    ),
    config: omitInterfaceArrayFixtureConfig(),
    error: /omit-interface-array utility identity unsupported at Root\.hours/,
  },
  {
    name: "a mutable Omit interface array",
    source: omitInterfaceArrayFixtureSource(
      'Omit<Hour, "contentHash">[]',
    ),
    config: omitInterfaceArrayFixtureConfig(),
    error: /omit-interface-array projection shape unsupported at Root\.hours/,
  },
  {
    name: "an interface-array projection omitting its only field",
    source: `export interface Hour { readonly contentHash: string; }
export interface Root {
  readonly hours: readonly Omit<Hour, "contentHash">[];
}
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Hour", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {},
          fieldProjections: {
            hours: { kind: "omit-interface-array", root: "Hour" },
          },
        },
      ],
    },
    error: /omit-interface-array projection removes every field at Root\.hours/,
  },
  {
    name: "an indexed projection without a configured source alias",
    source: indexedLiteralFixtureSource(),
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "ReasonCode", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {},
          fieldProjections: {
            reasonCode: { kind: "indexed-literals", root: "LoadedRuntime" },
          },
        },
      ],
    },
    error: /fieldProjections\.reasonCode\.root must name an indexed literal source/,
  },
  {
    name: "an empty indexed-literal source record",
    source: indexedLiteralFixtureSource(),
    config: indexedLiteralFixtureConfig({}),
    error: /indexedLiteralSources must be a non-empty plain object/,
  },
  {
    name: "an invalid indexed-literal alias name",
    source: indexedLiteralFixtureSource(),
    config: indexedLiteralFixtureConfig({
      "Loaded.Runtime": "fixture.ts",
      LoadedRuntime: "fixture.ts",
    }),
    error: /indexedLiteralSources has invalid alias name "Loaded\.Runtime"/,
  },
  {
    name: "indexed-literal sources without an indexed projection",
    source: indexedLiteralFixtureSource(),
    config: indexedLiteralFixtureConfig(
      { LoadedRuntime: "fixture.ts" },
      { reasonCode: { kind: "return-enum", root: "ReasonCode" } },
    ),
    error: /indexedLiteralSources requires an indexed-literals projection/,
  },
  {
    name: "an unused indexed-literal source alias",
    source: `${indexedLiteralFixtureSource()}
type NeverVisited = { readonly reasonCode: "unobserved"; readonly state: "disabled" };
`,
    config: indexedLiteralFixtureConfig({
      LoadedRuntime: "fixture.ts",
      LoadedRuntimeV1: "fixture.ts",
      NeverVisited: "fixture.ts",
    }),
    error: /unused indexed literal source at Root\.NeverVisited/,
  },
  {
    name: "an indexed selector for a different direct field",
    source: indexedLiteralFixtureSource(
      undefined,
      `readonly reasonCode: LoadedRuntime["state"];
  readonly state: LoadedRuntime["state"];`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed-literals projection shape unsupported at Root\.reasonCode/,
  },
  {
    name: "an unconfigured indexed-literal alias arm",
    source: indexedLiteralFixtureSource(),
    config: indexedLiteralFixtureConfig({ LoadedRuntime: "fixture.ts" }),
    error: /indexed literal alias is not configured at Root\.reasonCode: LoadedRuntimeV1/,
  },
  {
    name: "an optional indexed-literal selected field",
    source: indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 = { readonly reasonCode?: "first"; readonly state: "active" };
type LoadedRuntime = LoadedRuntimeV1;`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal selected field unsupported at Root\.reasonCode/,
  },
  {
    name: "a mutable indexed-literal selected field",
    source: indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 = { reasonCode: "first"; readonly state: "active" };
type LoadedRuntime = LoadedRuntimeV1;`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal object arm unsupported at Root\.reasonCode/,
  },
  {
    name: "a computed indexed-literal selected field",
    source: indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 = { readonly ["reasonCode"]: "first"; readonly state: "active" };
type LoadedRuntime = LoadedRuntimeV1;`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal object arm unsupported at Root\.reasonCode/,
  },
  {
    name: "a duplicate indexed-literal selected field",
    source: indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 = {
  readonly reasonCode: "first";
  readonly reasonCode: "second";
  readonly state: "active";
};
type LoadedRuntime = LoadedRuntimeV1;`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal object arm unsupported at Root\.reasonCode/,
  },
  {
    name: "an index signature in an indexed-literal object arm",
    source: indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 = {
  readonly [name: string]: "first" | "active";
  readonly reasonCode: "first";
  readonly state: "active";
};
type LoadedRuntime = LoadedRuntimeV1;`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal object arm unsupported at Root\.reasonCode/,
  },
  {
    name: "an indexed-literal union arm missing the selected field",
    source: indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 =
  | { readonly reasonCode: "first"; readonly state: "active" }
  | { readonly ignored: "missing"; readonly state: "disabled" };
type LoadedRuntime = LoadedRuntimeV1;`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal selected field unsupported at Root\.reasonCode/,
  },
  {
    name: "an indexed-literal intersection with the augmentation first",
    source: indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 = { readonly reasonCode: "first"; readonly state: "active" };
type LoadedRuntime = { readonly ignored: "first" } & LoadedRuntimeV1;`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal intersection unsupported at Root\.reasonCode/,
  },
  {
    name: "an indexed-literal intersection that redefines the selected field",
    source: indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 = { readonly reasonCode: "first"; readonly state: "active" };
type LoadedRuntime = LoadedRuntimeV1 & { readonly reasonCode: "second" };`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal intersection touches selected field at Root\.reasonCode/,
  },
  {
    name: "an indexed-literal alias cycle",
    source: indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 = LoadedRuntime;
type LoadedRuntime = LoadedRuntimeV1;`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal alias cycle at Root\.reasonCode: LoadedRuntime/,
  },
  {
    name: "a non-string indexed-literal property",
    source: indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 = { readonly reasonCode: true; readonly state: "active" };
type LoadedRuntime = LoadedRuntimeV1;`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal property type unsupported at Root\.reasonCode/,
  },
  {
    name: "a shadowed Extract utility in an indexed-literal property",
    source: `type Extract<T, U> = U;
${indexedLiteralFixtureSource()}`,
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal Extract utility identity unsupported at Root\.reasonCode/,
  },
  {
    name: "a nullable enum subject for indexed-literal Extract",
    source: `export type ReasonCode = "first" | "second" | "third";
export type NullableReasonCode = "first" | null;
type LoadedRuntimeV1 = {
  readonly reasonCode: Extract<NullableReasonCode, "first">;
  readonly state: "active";
};
type LoadedRuntime = LoadedRuntimeV1;
export interface Root {
  readonly reasonCode: LoadedRuntime["reasonCode"];
  readonly state: LoadedRuntime["state"];
}
`,
    config: {
      ...indexedLiteralFixtureConfig(),
      roots: [
        { source: "fixture.ts", exportName: "ReasonCode", fields: {} },
        indexedLiteralFixtureConfig().roots[1],
        { source: "fixture.ts", exportName: "NullableReasonCode", fields: {} },
      ],
    },
    error: /indexed literal Extract enum unsupported at Root\.reasonCode/,
  },
  {
    name: "a nonliteral indexed-literal Extract constraint",
    source: `type SelectedReason = "first";
${indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 = {
  readonly reasonCode: Extract<ReasonCode, SelectedReason>;
  readonly state: "active";
};
type LoadedRuntime = LoadedRuntimeV1;`,
    )}`,
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal Extract constraint unsupported at Root\.reasonCode/,
  },
  {
    name: "an indexed-literal Extract with the wrong arity",
    source: indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 = {
  readonly reasonCode: Extract<ReasonCode>;
  readonly state: "active";
};
type LoadedRuntime = LoadedRuntimeV1;`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal property type unsupported at Root\.reasonCode/,
  },
  {
    name: "an indexed-literal Extract value outside its configured enum",
    source: indexedLiteralFixtureSource(
      `type LoadedRuntimeV1 = {
  readonly reasonCode: Extract<ReasonCode, "outside">;
  readonly state: "active";
};
type LoadedRuntime = LoadedRuntimeV1;`,
    ),
    config: indexedLiteralFixtureConfig(),
    error: /indexed literal Extract value "outside" is outside configured enum at Root\.reasonCode/,
  },
  {
    name: "a value import for an indexed-literal alias",
    sources: {
      "fixture.ts": `import { LoadedRuntime } from "./producer.js";
export interface Root {
  readonly reasonCode: LoadedRuntime["reasonCode"];
}
`,
      "producer.ts": `export type LoadedRuntime =
  | { readonly reasonCode: null }
  | { readonly reasonCode: "disabled" };
export type ReasonCode = "first" | "second";
`,
    },
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "producer.ts", exportName: "ReasonCode", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {},
          indexedLiteralSources: { LoadedRuntime: "producer.ts" },
          fieldProjections: {
            reasonCode: { kind: "indexed-literals", root: "LoadedRuntime" },
          },
        },
      ],
      typeImports: [{
        consumer: "fixture.ts",
        specifier: "./producer.js",
        entry: "producer.ts",
        exports: { ReasonCode: "producer.ts" },
      }],
    },
    error: /indexed literal alias identity unsupported at Root\.reasonCode/,
  },
  {
    name: "an empty proof-only export record",
    ...compiledProofFixture({
      proofExports: {},
    }),
    error: /proofExports must be a non-empty plain object when present/,
  },
  {
    name: "a proof-only export outside the configured leaf inventory",
    ...compiledProofFixture({
      proofExports: {
        CollectorStatus: "fixture-database/src/missing.ts",
      },
    }),
    error: /proofExports\.CollectorStatus must name a configured proof entry/,
  },
  {
    name: "a type import name that is both ordinary and proof-only",
    ...compiledProofFixture({
      importOverrides: {
        exports: {
          CollectorStatus: "fixture-database/src/collector.ts",
        },
      },
    }),
    error: /type import name cannot be both exported and proof-only/,
  },
  {
    name: "a stale proof-only package export",
    ...compiledProofFixture({
      proofExports: {
        CollectorRun: "fixture-database/src/collector.ts",
        CollectorStatus: "fixture-database/src/collector.ts",
      },
    }),
    error: /unused configured proof export .*#CollectorRun for fixture\.ts/,
  },
  {
    name: "a proof-only package export used as an ordinary field type",
    ...compiledProofFixture({
      rootSource: `import type { CollectorStatus } from "@fixture/database";
export interface Root {
  readonly status: CollectorStatus;
  readonly source: {
    readonly stateStatus: CollectorStatus["stateStatus"];
  };
}
`,
    }),
    error: /configured type import identity unsupported at Root\.status/,
  },
  {
    name: "a value import for an indexed proof-only package export",
    ...compiledProofFixture({
      rootSource: `import { CollectorStatus } from "@fixture/database";
export interface Root {
  readonly source: {
    readonly stateStatus: CollectorStatus["stateStatus"];
  };
}
`,
    }),
    error: /proof-only type import used as a value at Root\.source\.stateStatus/,
  },
  {
    name: "a proof-only package interface used by a value projection",
    ...compiledProofFixture({
      extraRoots: [{
        source: "fixture.ts",
        exportName: "Band",
        fields: {},
      }],
      rootOverrides: {
        fieldProjections: {
          leadBand: { kind: "return-enum", root: "Band" },
          "source.stateStatus": {
            kind: "indexed-literals",
            root: "CollectorStatus",
          },
        },
      },
      rootSource: `import { CollectorStatus } from "@fixture/database";
export type Band = "short" | "long";
export interface Root {
  readonly leadBand: ReturnType<typeof CollectorStatus>;
  readonly source: {
    readonly stateStatus: CollectorStatus["stateStatus"];
  };
}
`,
    }),
    error: /configured value import identity unsupported at Root\.leadBand/,
  },
  {
    name: "compiled package metadata without compiled-source proof",
    ...compiledProofFixture({
      compiledSource: null,
    }),
    error: /compiled source package metadata unsupported at @fixture\/database/,
  },
  {
    name: "a non-normalized compiled-source tsconfig path",
    ...compiledProofFixture({
      compiledSource: {
        tsconfig: "fixture-database/../tsconfig.json",
        base: "tsconfig.base.json",
      },
    }),
    error: /compiledSource\.tsconfig/,
  },
  {
    name: "a compiled-source tsconfig relocated below the package root",
    ...compiledProofFixture({
      compiledSource: {
        tsconfig: "fixture-database/config/tsconfig.json",
        base: "tsconfig.base.json",
      },
    }),
    error: /compiledSource\.tsconfig must belong to the configured package/,
  },
  {
    name: "a compiled-source base config relocated into the package",
    ...compiledProofFixture({
      compiledSource: {
        tsconfig: "fixture-database/tsconfig.json",
        base: "fixture-database/tsconfig.base.json",
      },
    }),
    error: /compiled source tsconfig unsupported at fixture-database\/tsconfig\.json/,
  },
  {
    name: "compiled package metadata with a source exports target",
    ...compiledProofFixture({
      packageOverrides: { exports: "./src/index.js" },
    }),
    error: /compiled source package metadata unsupported at @fixture\/database/,
  },
  {
    name: "compiled package metadata with a mismatched declaration target",
    ...compiledProofFixture({
      packageOverrides: { types: "./types/index.d.ts" },
    }),
    error: /compiled source package metadata unsupported at @fixture\/database/,
  },
  {
    name: "a compiled source tsconfig extending the wrong base path",
    ...compiledProofFixture({
      tsconfigOverrides: { extends: "./tsconfig.base.json" },
    }),
    error: /compiled source tsconfig unsupported at fixture-database\/tsconfig\.json/,
  },
  {
    name: "a compiled source base config with semantic drift",
    ...compiledProofFixture({
      baseCompilerOptions: { strict: false },
    }),
    error: /compiled source base config unsupported at tsconfig\.base\.json/,
  },
  {
    name: "a compiled source tsconfig with an emission override",
    ...compiledProofFixture({
      tsconfigCompilerOptions: {
        declaration: true,
        noEmit: true,
        outDir: "dist",
        rootDir: "src",
        types: ["node"],
      },
    }),
    error: /compiled source tsconfig unsupported at fixture-database\/tsconfig\.json/,
  },
  {
    name: "an indexed interface selector with heritage",
    ...compiledProofFixture({
      collectorSource: `export interface CollectorRun {
  readonly stateStatus: "supported" | "cold";
}
export interface CollectorStatus extends CollectorRun {}
`,
    }),
    error: /indexed literal alias declaration unsupported at Root\.source\.stateStatus/,
  },
  {
    name: "an indexed interface selector with an optional selected property",
    ...compiledProofFixture({
      collectorSource: `export interface CollectorRun {
  readonly stateStatus: "supported" | "cold";
}
export interface CollectorStatus {
  readonly stateStatus?: CollectorRun["stateStatus"];
}
`,
    }),
    error: /indexed literal selected field unsupported at Root\.source\.stateStatus/,
  },
  {
    name: "an indexed interface selector with a mutable selected property",
    ...compiledProofFixture({
      collectorSource: `export interface CollectorRun {
  readonly stateStatus: "supported" | "cold";
}
export interface CollectorStatus {
  stateStatus: CollectorRun["stateStatus"];
}
`,
    }),
    error: /indexed literal object arm unsupported at Root\.source\.stateStatus/,
  },
  {
    name: "an indexed interface selector naming a different field",
    ...compiledProofFixture({
      rootSource: `import type { CollectorStatus } from "@fixture/database";
export interface Root {
  readonly source: {
    readonly stateStatus: CollectorStatus["other"];
  };
}
`,
    }),
    error: /indexed-literals projection shape unsupported at Root\.source\.stateStatus/,
  },
  {
    name: "an indexed interface selector redirecting to a different field",
    ...compiledProofFixture({
      collectorSource: `export interface CollectorRun {
  readonly other: "supported" | "cold";
}
export interface CollectorStatus {
  readonly stateStatus: CollectorRun["other"];
}
`,
    }),
    error: /indexed literal indexed access unsupported at Root\.source\.stateStatus/,
  },
  {
    name: "a cycle across indexed interface selectors",
    ...compiledProofFixture({
      collectorSource: `export interface CollectorRun {
  readonly stateStatus: CollectorStatus["stateStatus"];
}
export interface CollectorStatus {
  readonly stateStatus: CollectorRun["stateStatus"];
}
`,
    }),
    error: /indexed literal alias cycle/,
  },
  {
    name: "a generic indexed interface selector",
    ...compiledProofFixture({
      collectorSource: `export interface CollectorRun {
  readonly stateStatus: "supported" | "cold";
}
export interface CollectorStatus<T> {
  readonly stateStatus: CollectorRun["stateStatus"];
}
`,
    }),
    error: /indexed literal alias declaration unsupported at Root\.source\.stateStatus/,
  },
  {
    name: "a Readonly-wrapped direct partial metric record",
    config: projectedFixtureConfig(),
    sources: projectedFixtureSources(
      undefined,
      `readonly adjustedMetrics: Readonly<Partial<Record<Metric, number>>>;
  readonly appliedMetrics: readonly (typeof METRICS)[number][];
  readonly band: Band;
  readonly leadBand: ReturnType<typeof bandFor>;
  readonly metric: Metric;`,
    ),
    error: /finite-partial-metric-record requires Partial<Record<Key, number>> at Root\.adjustedMetrics/,
  },
  {
    name: "a direct partial metric record key hidden behind a local alias",
    config: projectedFixtureConfig(),
    sources: projectedFixtureSources(
      `import { METRICS, bandFor, type Band, type Metric } from "@fixture/domain";
type MetricAlias = Metric;`,
      `readonly adjustedMetrics: Partial<Record<MetricAlias, number>>;
  readonly appliedMetrics: readonly (typeof METRICS)[number][];
  readonly band: Band;
  readonly leadBand: ReturnType<typeof bandFor>;
  readonly metric: Metric;`,
    ),
    error: /finite-partial-metric-record key identity unsupported at Root\.adjustedMetrics/,
  },
  {
    name: "a value-only import for a direct partial metric record key",
    config: projectedFixtureConfig(),
    sources: projectedFixtureSources(
      'import { METRICS, Metric, bandFor, type Band } from "@fixture/domain";',
    ),
    error: /configured type import identity unsupported at Root\.adjustedMetrics/,
  },
  {
    name: "a private alias that no exported configured root uses",
    source: `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
type Root = FirstPayload | SecondPayload;
`,
    config: {
      ...objectUnionFixtureConfig({ FirstPayload: "First", SecondPayload: "Second" }),
      roots: [
        {
          source: "fixture.ts",
          exportName: "Root",
          localAlias: true,
          objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
          fields: {},
        },
        { source: "fixture.ts", exportName: "FirstPayload", fields: {} },
        { source: "fixture.ts", exportName: "SecondPayload", fields: {} },
      ],
    },
    error: /local alias is not reachable from an exported root fixture\.ts#Root/,
  },
  {
    name: "private-alias metadata on an interface declaration",
    source: `interface Root { readonly value: string; }
export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
`,
    config: {
      ...objectUnionFixtureConfig({ FirstPayload: "First", SecondPayload: "Second" }),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        localAlias: true,
        objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
        fields: {},
      }],
    },
    error: /local alias declaration unsupported fixture\.ts#Root/,
  },
  {
    name: "a private object union wrapping string-enum arms",
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        {
          source: "fixture-domain/src/types.ts",
          exportName: "Metric",
          catalog: { kind: "satisfies-string-tuple" },
          fields: {},
        },
        {
          source: "fixture-domain/src/types.ts",
          exportName: "Band",
          fields: {},
        },
        {
          source: "fixture.ts",
          exportName: "PrivateDecision",
          localAlias: true,
          objectUnion: { Band: "Band", Metric: "Metric" },
          fields: {},
        },
        { source: "fixture.ts", exportName: "PublicRecord", fields: {} },
      ],
      typeImports: [{
        consumer: "fixture.ts",
        specifier: "@fixture/domain",
        packageJson: "fixture-domain/package.json",
        entry: "fixture-domain/src/index.ts",
        leaves: ["fixture-domain/src/types.ts"],
        exports: {
          Band: "fixture-domain/src/types.ts",
          Metric: "fixture-domain/src/types.ts",
        },
      }],
    },
    sources: {
      ...projectedFixtureSources(),
      "fixture.ts": `import type { Band, Metric } from "@fixture/domain";
type PrivateDecision = Metric | Band;
export interface PublicRecord { readonly decision: PrivateDecision; }
`,
    },
    error: /local alias object union arm unsupported for Metric at root PrivateDecision/,
  },
  {
    name: "a stale direct field projection",
    config: projectedFixtureConfig({
      appliedMetrics: { kind: "const-catalog-array", root: "Metric" },
      leadBand: { kind: "return-enum", root: "Band" },
      missing: { kind: "return-enum", root: "Band" },
    }),
    sources: projectedFixtureSources(),
    error: /unused field projection at Root\.missing/,
  },
  {
    name: "a field projection to an unconfigured target",
    config: projectedFixtureConfig({
      appliedMetrics: { kind: "const-catalog-array", root: "MissingMetric" },
      leadBand: { kind: "return-enum", root: "Band" },
    }),
    sources: projectedFixtureSources(),
    error: /field projection target unsupported at Root\.appliedMetrics/,
  },
  {
    name: "a mutable catalog array projection",
    config: projectedFixtureConfig(),
    sources: projectedFixtureSources(
      undefined,
      `readonly adjustedMetrics: Partial<Record<Metric, number>>;
  readonly appliedMetrics: (typeof METRICS)[number][];
  readonly band: Band;
  readonly leadBand: ReturnType<typeof bandFor>;
  readonly metric: Metric;`,
    ),
    error: /const-catalog-array projection shape unsupported at Root\.appliedMetrics/,
  },
  {
    name: "a catalog array projection aimed at an unrelated enum",
    config: projectedFixtureConfig({
      appliedMetrics: { kind: "const-catalog-array", root: "Band" },
      leadBand: { kind: "return-enum", root: "Band" },
    }),
    sources: projectedFixtureSources(),
    error: /const-catalog-array projection target unsupported at Root\.appliedMetrics/,
  },
  {
    name: "a catalog array projection from a different imported catalog",
    config: projectedFixtureConfig(),
    sources: projectedFixtureSources(
      'import { OTHER_METRICS, bandFor, type Band, type Metric } from "@fixture/domain";',
      `readonly adjustedMetrics: Partial<Record<Metric, number>>;
  readonly appliedMetrics: readonly (typeof OTHER_METRICS)[number][];
  readonly band: Band;
  readonly leadBand: ReturnType<typeof bandFor>;
  readonly metric: Metric;`,
      `type MetricName = string;
export const METRICS = ["windSpeed", "windGust"] as const satisfies readonly MetricName[];
export const OTHER_METRICS = ["windSpeed"] as const satisfies readonly MetricName[];
export type Metric = (typeof METRICS)[number];
export type Band = "short" | "long";
// return one annotated band without evaluating the body
export function bandFor(value: number): Band { throw new Error(String(value)); }
`,
    ),
    error: /const-catalog-array projection catalog identity unsupported at Root\.appliedMetrics/,
  },
  {
    name: "a renamed imported catalog projection",
    config: projectedFixtureConfig(),
    sources: projectedFixtureSources(
      'import { METRICS as LOCAL_METRICS, bandFor, type Band, type Metric } from "@fixture/domain";',
      `readonly adjustedMetrics: Partial<Record<Metric, number>>;
  readonly appliedMetrics: readonly (typeof LOCAL_METRICS)[number][];
  readonly band: Band;
  readonly leadBand: ReturnType<typeof bandFor>;
  readonly metric: Metric;`,
    ),
    error: /configured value import identity unsupported at Root\.appliedMetrics/,
  },
  {
    name: "an imported catalog whose barrel provenance differs from its leaf",
    config: projectedFixtureConfig(undefined, [
      "fixture-domain/src/duplicate.ts",
      "fixture-domain/src/types.ts",
    ]),
    sources: {
      ...projectedFixtureSources(),
      "fixture-domain/src/index.ts": `export * from "./types.js";
export * from "./duplicate.js";
`,
      "fixture-domain/src/duplicate.ts":
        'export const METRICS = ["shadow"] as const;\n',
    },
    error: /configured value import provenance unsupported at Root\.appliedMetrics/,
  },
  {
    name: "a direct enum field mapped as a return-enum projection",
    config: projectedFixtureConfig(),
    sources: projectedFixtureSources(
      undefined,
      `readonly adjustedMetrics: Partial<Record<Metric, number>>;
  readonly appliedMetrics: readonly (typeof METRICS)[number][];
  readonly band: Band;
  readonly leadBand: Band;
  readonly metric: Metric;`,
    ),
    error: /return-enum projection shape unsupported at Root\.leadBand/,
  },
  {
    name: "a return-enum projection from an imported arrow function",
    config: projectedFixtureConfig(),
    sources: projectedFixtureSources(
      undefined,
      undefined,
      `type MetricName = string;
export const METRICS = ["windSpeed", "windGust"] as const satisfies readonly MetricName[];
export type Metric = (typeof METRICS)[number];
export type Band = "short" | "long";
export const bandFor = (value: number): Band => { throw new Error(String(value)); };
`,
    ),
    error: /return-enum function declaration unsupported at Root\.leadBand/,
  },
  {
    name: "a return-enum projection from a generic function",
    config: projectedFixtureConfig(),
    sources: projectedFixtureSources(
      undefined,
      undefined,
      `type MetricName = string;
export const METRICS = ["windSpeed", "windGust"] as const satisfies readonly MetricName[];
export type Metric = (typeof METRICS)[number];
export type Band = "short" | "long";
// keep one generic declaration outside the accepted projection grammar
export function bandFor<T>(value: T): Band { throw new Error(String(value)); }
`,
    ),
    error: /return-enum function declaration unsupported at Root\.leadBand/,
  },
  {
    name: "a locally shadowed ReturnType projection utility",
    config: projectedFixtureConfig(),
    sources: projectedFixtureSources(
      `import { METRICS, bandFor, type Band, type Metric } from "@fixture/domain";
type ReturnType<T> = Band;`,
    ),
    error: /return-enum utility identity unsupported at Root\.leadBand/,
  },
  {
    name: "a return-enum function annotated with a different configured enum",
    config: projectedFixtureConfig(),
    sources: projectedFixtureSources(
      undefined,
      undefined,
      `type MetricName = string;
export const METRICS = ["windSpeed", "windGust"] as const satisfies readonly MetricName[];
export type Metric = (typeof METRICS)[number];
export type Band = "short" | "long";
// return one differently annotated enum without evaluating the body
export function bandFor(value: number): Metric { throw new Error(String(value)); }
`,
    ),
    error: /return-enum function return identity unsupported at Root\.leadBand/,
  },
  {
    name: "a literal-union projection that differs from its configured field",
    source: `export type Loader =
  | { readonly reason: null; readonly state: "active" }
  | { readonly reason: "loader_failure"; readonly state: "disabled" };
export type Root = Loader["state"] | "direct";
`,
    config: literalUnionFixtureConfig({ sourceAlias: "Loader", field: "reason" }),
    error: /literal union source projection unsupported at root Root/,
  },
  {
    name: "a literal-union source alias with a referenced object arm",
    source: `interface Active { readonly reason: null; }
export type Loader = Active | { readonly reason: "loader_failure" };
export type Root = Loader["reason"] | "direct";
`,
    config: literalUnionFixtureConfig({ sourceAlias: "Loader", field: "reason" }),
    error: /literal union source alias arm unsupported at root Root/,
  },
  {
    name: "an optional literal-union source alias field",
    source: `export type Loader =
  | { readonly reason: null }
  | { readonly reason?: "loader_failure" };
export type Root = Loader["reason"] | "direct";
`,
    config: literalUnionFixtureConfig({ sourceAlias: "Loader", field: "reason" }),
    error: /literal union source alias field unsupported at root Root/,
  },
  {
    name: "an unconfigured literal-union referenced alias",
    source: `export type Loader =
  | { readonly reason: null }
  | { readonly reason: "loader_failure" };
type Extra = "first" | "second";
export type Root = Loader["reason"] | Extra;
`,
    config: literalUnionFixtureConfig({ sourceAlias: "Loader", field: "reason" }),
    error: /literal union referenced alias at root Root identity unsupported/,
  },
  {
    name: "a configured literal-union alias with a non-union source",
    source: `export type Loader =
  | { readonly reason: null }
  | { readonly reason: "loader_failure" };
export type Extra = "only";
export type Root = Loader["reason"] | Extra;
`,
    config: literalUnionFixtureConfig(
      { sourceAlias: "Loader", field: "reason" },
      [{ source: "fixture.ts", exportName: "Extra", fields: {} }],
    ),
    error: /literal union referenced alias at root Root unsupported/,
  },
  {
    name: "a literal-union composition without exact null",
    source: `export type Loader =
  | { readonly reason: "first" }
  | { readonly reason: "second" };
export type Root = Loader["reason"] | "direct";
`,
    config: literalUnionFixtureConfig({ sourceAlias: "Loader", field: "reason" }),
    error: /literal union composition unsupported at root Root/,
  },
  {
    name: "literal-union metadata with field mappings",
    source: `export type Loader =
  | { readonly reason: null }
  | { readonly reason: "loader_failure" };
export type Root = Loader["reason"] | "direct";
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        literalUnion: { sourceAlias: "Loader", field: "reason" },
        fields: { value: "finite-f64" },
      }],
    },
    error: /literal union field mappings unsupported at config\.roots\[0\]/,
  },
  {
    name: "a direct literal mapped as a const string",
    source: `export interface Root { readonly contractVersion: "v1"; }
`,
    config: fixtureConfig("Root", { contractVersion: "const-string" }),
    error: /const-string requires typeof LOCAL_CONST at Root\.contractVersion/,
  },
  {
    name: "a concatenated const string",
    source: `const VERSION = "v" + "1";
export interface Root { readonly contractVersion: typeof VERSION; }
`,
    config: fixtureConfig("Root", { contractVersion: "const-string" }),
    error: /const string source unsupported at root Root/,
  },
  {
    name: "an imported const string identity",
    source: `import { VERSION } from "./foreign.js";
export interface Root { readonly contractVersion: typeof VERSION; }
`,
    config: fixtureConfig("Root", { contractVersion: "const-string" }),
    error: /const string identity unsupported at root Root/,
  },
  {
    name: "a reversed flat interface extension",
    source: `export interface Base { readonly inherited: string; }
export interface Root {
  readonly value: null | { readonly own: string } & Base;
}
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: { value: "flat-interface-extension" },
        },
      ],
    },
    error: /flat-interface-extension requires ExactConfiguredInterface & \{ \.\.\. \} or null union at Root\.value/,
  },
  {
    name: "an unconfigured flat interface extension base",
    source: `export interface Base { readonly inherited: string; }
export interface Root {
  readonly value: null | Base & { readonly own: string };
}
`,
    config: fixtureConfig("Root", { value: "flat-interface-extension" }),
    error: /flat-interface-extension base identity unsupported at Root\.value/,
  },
  {
    name: "a flat interface extension with three intersection members",
    source: `export interface Base { readonly inherited: string; }
export interface Extra { readonly extra: string; }
export interface Root {
  readonly value: null | Base & Extra & { readonly own: string };
}
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        { source: "fixture.ts", exportName: "Extra", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: { value: "flat-interface-extension" },
        },
      ],
    },
    error: /flat-interface-extension requires ExactConfiguredInterface & \{ \.\.\. \} or null union at Root\.value/,
  },
  {
    name: "a flat interface extension that redeclares an inherited field",
    source: `export interface Base { readonly inherited: string; }
export interface Root {
  readonly value: null | Base & { readonly inherited: string };
}
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: { value: "flat-interface-extension" },
        },
      ],
    },
    error: /inherited property redeclaration unsupported at Root\.value\.inherited/,
  },
  {
    name: "a direct sibling type import with package-only configuration",
    sources: {
      "fixture.ts": `import type { Child } from "./sibling.js";
export interface Root { readonly child: Child; }
`,
      "sibling.ts": "export interface Child { readonly value: string; }\n",
    },
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Root", fields: {} },
        { source: "sibling.ts", exportName: "Child", fields: {} },
      ],
      typeImports: [{
        consumer: "fixture.ts",
        specifier: "./sibling.js",
        packageJson: "fixture-package.json",
        entry: "sibling.ts",
        exports: { Child: "sibling.ts" },
      }],
    },
    error: /config\.typeImports\[0\] has unsupported key "packageJson"/,
  },
  {
    name: "a direct sibling type import whose entry differs from its specifier",
    sources: {
      "fixture.ts": `import type { Child } from "./sibling.js";
export interface Root { readonly child: Child; }
`,
      "other.ts": "export interface Child { readonly value: string; }\n",
    },
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Root", fields: {} },
        { source: "other.ts", exportName: "Child", fields: {} },
      ],
      typeImports: [{
        consumer: "fixture.ts",
        specifier: "./sibling.js",
        entry: "other.ts",
        exports: { Child: "other.ts" },
      }],
    },
    error: /config\.typeImports\[0\]\.entry must match the direct sibling specifier/,
  },
  {
    name: "a re-export in a direct sibling type import entry",
    sources: {
      "fixture.ts": `import type { Child } from "./sibling.js";
export interface Root { readonly child: Child; }
`,
      "sibling.ts": `export interface Child { readonly value: string; }
export * from "./deeper.js";
`,
    },
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Root", fields: {} },
        { source: "sibling.ts", exportName: "Child", fields: {} },
      ],
      typeImports: [{
        consumer: "fixture.ts",
        specifier: "./sibling.js",
        entry: "sibling.ts",
        exports: { Child: "sibling.ts" },
      }],
    },
    error: /type import entry re-export unsupported at sibling\.ts/,
  },
  {
    name: "a value import at a direct sibling type-only edge",
    sources: {
      "fixture.ts": `import { Child } from "./sibling.js";
export interface Root { readonly child: Child; }
`,
      "sibling.ts": "export interface Child { readonly value: string; }\n",
    },
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Root", fields: {} },
        { source: "sibling.ts", exportName: "Child", fields: {} },
      ],
      typeImports: [{
        consumer: "fixture.ts",
        specifier: "./sibling.js",
        entry: "sibling.ts",
        exports: { Child: "sibling.ts" },
      }],
    },
    error: /configured type import identity unsupported at Root\.child/,
  },
  {
    name: "non-object named object-union metadata",
    source: "export type Root = FirstPayload | SecondPayload;\n",
    config: objectUnionFixtureConfig([]),
    error: /config\.roots\[0\]\.objectUnion must be a plain object/,
  },
  {
    name: "a named object union with only one configured variant",
    source: "export type Root = FirstPayload | SecondPayload;\n",
    config: objectUnionFixtureConfig({ FirstPayload: "First" }),
    error: /config\.roots\[0\]\.objectUnion must have at least two variants/,
  },
  {
    name: "an invalid named object-union source interface name",
    source: "export type Root = FirstPayload | SecondPayload;\n",
    config: objectUnionFixtureConfig({
      "first-payload": "First",
      SecondPayload: "Second",
    }),
    error: /config\.roots\[0\]\.objectUnion has invalid source interface name "first-payload"/,
  },
  {
    name: "a lowercase named object-union Rust variant",
    source: "export type Root = FirstPayload | SecondPayload;\n",
    config: objectUnionFixtureConfig({
      FirstPayload: "first",
      SecondPayload: "Second",
    }),
    error: /config\.roots\[0\]\.objectUnion\.FirstPayload must be a Pascal-start ASCII Rust identifier/,
  },
  {
    name: "a named object-union Rust keyword variant",
    source: "export type Root = FirstPayload | SecondPayload;\n",
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Self",
    }),
    error: /config\.roots\[0\]\.objectUnion\.SecondPayload is a Rust keyword/,
  },
  {
    name: "duplicate named object-union Rust variants",
    source: "export type Root = FirstPayload | SecondPayload;\n",
    config: objectUnionFixtureConfig({
      FirstPayload: "Payload",
      SecondPayload: "Payload",
    }),
    error: /duplicate object union Rust variant Payload at config\.roots\[0\]\.objectUnion/,
  },
  {
    name: "named object-union metadata combined with a base",
    source: "export type Root = FirstPayload | SecondPayload;\n",
    config: {
      ...objectUnionFixtureConfig({ FirstPayload: "First", SecondPayload: "Second" }),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        base: "FirstPayload",
        objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
        fields: {},
      }],
    },
    error: /object union metadata cannot be combined with base, catalog, or discriminator at config\.roots\[0\]/,
  },
  {
    name: "named object-union metadata combined with a catalog",
    source: "export type Root = FirstPayload | SecondPayload;\n",
    config: {
      ...objectUnionFixtureConfig({ FirstPayload: "First", SecondPayload: "Second" }),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        catalog: { kind: "satisfies-string-tuple" },
        objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
        fields: {},
      }],
    },
    error: /object union metadata cannot be combined with base, catalog, or discriminator at config\.roots\[0\]/,
  },
  {
    name: "named object-union metadata combined with a discriminator",
    source: "export type Root = FirstPayload | SecondPayload;\n",
    config: {
      ...objectUnionFixtureConfig({ FirstPayload: "First", SecondPayload: "Second" }),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        discriminator: "kind",
        objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
        fields: {},
      }],
    },
    error: /object union metadata cannot be combined with base, catalog, or discriminator at config\.roots\[0\]/,
  },
  {
    name: "field mappings on a named object union",
    source: "export type Root = FirstPayload | SecondPayload;\n",
    config: {
      ...objectUnionFixtureConfig({ FirstPayload: "First", SecondPayload: "Second" }),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
        fields: { value: "utc-iso" },
      }],
    },
    error: /object union field mappings unsupported at config\.roots\[0\]/,
  },
  {
    name: "a named interface union without explicit object-union metadata",
    source: `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export type Root = FirstPayload | SecondPayload;
`,
    config: fixtureRoots("Root", "FirstPayload", "SecondPayload"),
    error: /root type unsupported at Root/,
  },
  {
    name: "named object-union metadata on an interface root",
    source: `export interface Root { readonly value: string; }
export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
`,
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
      ThirdPayload: "Third",
    }),
    error: /named object union requires a type alias at root Root/,
  },
  {
    name: "a named object union with one direct arm",
    source: `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
export type Root = FirstPayload;
`,
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
      ThirdPayload: "Third",
    }),
    error: /named object union requires at least two direct type reference arms at root Root/,
  },
  {
    name: "an inline named object-union arm",
    source: `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
export type Root = FirstPayload | { readonly inline: string };
`,
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
      ThirdPayload: "Third",
    }),
    error: /named object union arm must be an argument-free identifier reference at root Root/,
  },
  {
    name: "a mixed numeric named object-union arm",
    source: `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
export type Root = FirstPayload | 1;
`,
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
      ThirdPayload: "Third",
    }),
    error: /named object union arm must be an argument-free identifier reference at root Root/,
  },
  {
    name: "a generic named object-union arm reference",
    source: `export interface FirstPayload<T> { readonly first: T; }
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
export type Root = FirstPayload<string> | SecondPayload;
`,
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
      ThirdPayload: "Third",
    }),
    error: /named object union arm must be an argument-free identifier reference at root Root/,
  },
  {
    name: "a duplicate named object-union source arm",
    source: `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
export type Root = FirstPayload | FirstPayload | SecondPayload;
`,
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
      ThirdPayload: "Third",
    }),
    error: /duplicate named object union arm FirstPayload at root Root/,
  },
  {
    name: "an unconfigured named object-union payload interface",
    source: `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export type Root = FirstPayload | SecondPayload;
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        {
          source: "fixture.ts",
          exportName: "Root",
          objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
          fields: {},
        },
        { source: "fixture.ts", exportName: "FirstPayload", fields: {} },
      ],
    },
    error: /named object union arm identity unsupported for SecondPayload at root Root/,
  },
  {
    name: "an imported package named object-union payload alias",
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        {
          source: "fixture.ts",
          exportName: "Root",
          objectUnion: { Child: "Child", LocalPayload: "Local" },
          fields: {},
        },
        {
          source: "fixture-domain/src/types.ts",
          exportName: "Child",
          fields: {},
        },
        { source: "fixture.ts", exportName: "LocalPayload", fields: {} },
      ],
      typeImports: [{
        consumer: "fixture.ts",
        specifier: "@fixture/domain",
        packageJson: "fixture-domain/package.json",
        entry: "fixture-domain/src/index.ts",
        leaves: ["fixture-domain/src/types.ts"],
        exports: { Child: "fixture-domain/src/types.ts" },
      }],
    },
    sources: {
      ...importedFixtureSources(),
      "fixture.ts": `import type { Child } from "@fixture/domain";
export interface LocalPayload { readonly local: string; }
export type Root = Child | LocalPayload;
`,
    },
    error: /named object union arm identity unsupported for Child at root Root/,
  },
  {
    name: "an imported-object-union type-alias arm",
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        {
          source: "fixture.ts",
          exportName: "Root",
          importedObjectUnion: true,
          objectUnion: { Child: "Child", LocalPayload: "Local" },
          fields: {},
        },
        {
          source: "fixture-domain/src/types.ts",
          exportName: "Child",
          fields: {},
        },
        { source: "fixture.ts", exportName: "LocalPayload", fields: {} },
      ],
      typeImports: [{
        consumer: "fixture.ts",
        specifier: "@fixture/domain",
        packageJson: "fixture-domain/package.json",
        entry: "fixture-domain/src/index.ts",
        leaves: ["fixture-domain/src/types.ts"],
        exports: { Child: "fixture-domain/src/types.ts" },
      }],
    },
    sources: {
      ...importedFixtureSources(),
      "fixture.ts": `import type { Child } from "@fixture/domain";
export interface LocalPayload { readonly local: string; }
export type Root = Child | LocalPayload;
`,
      "fixture-domain/src/types.ts":
        "export type Child = { readonly value: string };\n",
    },
    error: /named object union arm identity unsupported for Child at root Root/,
  },
  {
    name: "imported-object-union metadata without a public object union",
    source: "export interface Root { readonly value: string; }\n",
    config: {
      ...fixtureConfig("Root"),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        fields: {},
        importedObjectUnion: true,
      }],
    },
    error: /config\.roots\[0\]\.importedObjectUnion requires a public objectUnion/,
  },
  {
    name: "an imported-object-union opt-in without an imported arm",
    source: `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export type Root = FirstPayload | SecondPayload;
`,
    config: {
      ...objectUnionFixtureConfig({ FirstPayload: "First", SecondPayload: "Second" }),
      roots: [
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {},
          importedObjectUnion: true,
          objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
        },
        { source: "fixture.ts", exportName: "FirstPayload", fields: {} },
        { source: "fixture.ts", exportName: "SecondPayload", fields: {} },
      ],
    },
    error: /imported object union has no imported arm at root Root/,
  },
  {
    name: "a local type alias named object-union payload",
    source: `export type FirstPayload = { readonly first: string };
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
export type Root = FirstPayload | SecondPayload;
`,
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
      ThirdPayload: "Third",
    }),
    error: /named object union arm identity unsupported for FirstPayload at root Root/,
  },
  {
    name: "a named union alias object-union payload",
    source: `export interface FirstVariant { readonly first: string; }
export interface SecondVariant { readonly second: string; }
export type FirstPayload = FirstVariant | SecondVariant;
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
export type Root = FirstPayload | SecondPayload;
`,
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
      ThirdPayload: "Third",
    }),
    error: /named object union arm identity unsupported for FirstPayload at root Root/,
  },
  {
    name: "a generic named object-union payload interface",
    source: `export interface FirstPayload<T = string> { readonly first: T; }
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
export type Root = FirstPayload | SecondPayload;
`,
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
      ThirdPayload: "Third",
    }),
    error: /named object union arm identity unsupported for FirstPayload at root Root/,
  },
  {
    name: "an unexported configured named object-union payload",
    source: `interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
export type Root = FirstPayload | SecondPayload;
`,
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
      ThirdPayload: "Third",
    }),
    error: /root must be a named export fixture\.ts#FirstPayload/,
  },
  {
    name: "a missing named object-union arm mapping",
    source: `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
export type Root = FirstPayload | SecondPayload | ThirdPayload;
`,
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
    }),
    error: /missing named object union mapping for ThirdPayload at root Root/,
  },
  {
    name: "a stale named object-union arm mapping",
    source: `export interface FirstPayload { readonly first: string; }
export interface SecondPayload { readonly second: string; }
export interface ThirdPayload { readonly third: string; }
export type Root = FirstPayload | SecondPayload;
`,
    config: objectUnionFixtureConfig({
      FirstPayload: "First",
      SecondPayload: "Second",
      ThirdPayload: "Third",
    }),
    error: /unused named object union mapping ThirdPayload at root Root/,
  },
  {
    name: "a recursive named object-union payload cycle",
    source: `export interface FirstPayload { readonly root: Root; }
export interface SecondPayload { readonly second: string; }
export type Root = FirstPayload | SecondPayload;
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        {
          source: "fixture.ts",
          exportName: "Root",
          objectUnion: { FirstPayload: "First", SecondPayload: "Second" },
          fields: {},
        },
        { source: "fixture.ts", exportName: "FirstPayload", fields: {} },
        { source: "fixture.ts", exportName: "SecondPayload", fields: {} },
      ],
    },
    error: /recursive configured type reference cycle: Root -> FirstPayload -> Root/,
  },
  {
    name: "an unannotated ordered finite number record",
    source: "export interface Root { readonly readings: Readonly<Record<string, number>>; }\n",
    config: fixtureConfig("Root"),
    error: /configured type reference unsupported at Root\.readings/,
  },
  {
    name: "a mutable record without its readonly wrapper",
    source: "export interface Root { readonly readings: Record<string, number>; }\n",
    config: fixtureConfig("Root", { readings: "ordered-finite-f64-record" }),
    error: /ordered finite number record requires Readonly<Record<string, number>> at Root\.readings/,
  },
  {
    name: "an ordered record hidden behind a local alias",
    source: `type Readings = Readonly<Record<string, number>>;
export interface Root { readonly readings: Readings; }
`,
    config: fixtureConfig("Root", { readings: "ordered-finite-f64-record" }),
    error: /ordered finite number record requires Readonly<Record<string, number>> at Root\.readings/,
  },
  {
    name: "a local Readonly utility shadow",
    source: `type Readonly<T> = T;
export interface Root { readonly readings: Readonly<Record<string, number>>; }
`,
    config: fixtureConfig("Root", { readings: "ordered-finite-f64-record" }),
    error: /ordered finite number record utility identity unsupported at Root\.readings/,
  },
  {
    name: "a local Record utility shadow",
    source: `type Record<K, T> = T;
export interface Root { readonly readings: Readonly<Record<string, number>>; }
`,
    config: fixtureConfig("Root", { readings: "ordered-finite-f64-record" }),
    error: /ordered finite number record utility identity unsupported at Root\.readings/,
  },
  {
    name: "imported ordered record utility aliases",
    source: `import type { Readonly, Record } from "./foreign";
export interface Root { readonly readings: Readonly<Record<string, number>>; }
`,
    config: fixtureConfig("Root", { readings: "ordered-finite-f64-record" }),
    error: /ordered finite number record utility identity unsupported at Root\.readings/,
  },
  {
    name: "ambient global augmentation beside an ordered record",
    source: `export {};
declare global { interface Intruder { readonly value: string; } }
export interface Root { readonly readings: Readonly<Record<string, number>>; }
`,
    config: fixtureConfig("Root", { readings: "ordered-finite-f64-record" }),
    error: /ordered finite number record utility identity unsupported at Root\.readings/,
  },
  {
    name: "an ordered record with number keys",
    source: "export interface Root { readonly readings: Readonly<Record<number, number>>; }\n",
    config: fixtureConfig("Root", { readings: "ordered-finite-f64-record" }),
    error: /ordered finite number record requires Readonly<Record<string, number>> at Root\.readings/,
  },
  {
    name: "an ordered record with string values",
    source: "export interface Root { readonly readings: Readonly<Record<string, string>>; }\n",
    config: fixtureConfig("Root", { readings: "ordered-finite-f64-record" }),
    error: /ordered finite number record requires Readonly<Record<string, number>> at Root\.readings/,
  },
  {
    name: "a nullable ordered finite record",
    source: "export interface Root { readonly readings: Readonly<Record<string, number>> | null; }\n",
    config: fixtureConfig("Root", { readings: "ordered-finite-f64-record" }),
    error: /ordered finite number record requires Readonly<Record<string, number>> at Root\.readings/,
  },
  {
    name: "an array of ordered finite records",
    source: "export interface Root { readonly readings: readonly Readonly<Record<string, number>>[]; }\n",
    config: fixtureConfig("Root", { readings: "ordered-finite-f64-record" }),
    error: /ordered finite number record requires Readonly<Record<string, number>> at Root\.readings/,
  },
  {
    name: "an OrderedFiniteNumberRecord helper and root collision",
    source: "export interface OrderedFiniteNumberRecord { readonly value: string; }\n",
    config: fixtureConfig("OrderedFiniteNumberRecord"),
    error: /exportName collides with a generated Rust type/,
  },
  {
    name: "a finite metric record without its Readonly wrapper",
    source: `export type Metric = "a" | "b";
export interface Root { readonly adjustedMetrics: Partial<Record<Metric, number>>; }
`,
    config: metricRecordFixtureConfig("finite-metric-record"),
    error: /finite-metric-record requires Readonly<Partial<Record<Key, number>>> at Root\.adjustedMetrics/,
  },
  {
    name: "a finite metric record without its Partial wrapper",
    source: `export type Metric = "a" | "b";
export interface Root { readonly adjustedMetrics: Readonly<Record<Metric, number>>; }
`,
    config: metricRecordFixtureConfig("finite-metric-record"),
    error: /finite-metric-record requires Readonly<Partial<Record<Key, number>>> at Root\.adjustedMetrics/,
  },
  {
    name: "a shadowed finite metric record utility",
    source: `type Partial<T> = T;
export type Metric = "a" | "b";
export interface Root { readonly adjustedMetrics: Readonly<Partial<Record<Metric, number>>>; }
`,
    config: metricRecordFixtureConfig("finite-metric-record"),
    error: /finite-metric-record utility identity unsupported at Root\.adjustedMetrics/,
  },
  {
    name: "a finite metric record with primitive string keys",
    source: `export type Metric = "a" | "b";
export interface Root { readonly adjustedMetrics: Readonly<Partial<Record<string, number>>>; }
`,
    config: metricRecordFixtureConfig("finite-metric-record"),
    error: /finite-metric-record key identity unsupported at Root\.adjustedMetrics/,
  },
  {
    name: "a finite metric record key hidden behind a local alias",
    source: `export type Metric = "a" | "b";
type MetricAlias = Metric;
export interface Root { readonly adjustedMetrics: Readonly<Partial<Record<MetricAlias, number>>>; }
`,
    config: metricRecordFixtureConfig("finite-metric-record"),
    error: /finite-metric-record key identity unsupported at Root\.adjustedMetrics/,
  },
  {
    name: "an imported finite metric record key",
    source: `import type { Metric as ImportedMetric } from "./foreign";
export type Metric = "a" | "b";
export interface Root { readonly adjustedMetrics: Readonly<Partial<Record<ImportedMetric, number>>>; }
`,
    config: metricRecordFixtureConfig("finite-metric-record"),
    error: /finite-metric-record key identity unsupported at Root\.adjustedMetrics/,
  },
  {
    name: "a finite metric record key with type arguments",
    source: `export type Metric = "a" | "b";
export interface Root { readonly adjustedMetrics: Readonly<Partial<Record<Metric<never>, number>>>; }
`,
    config: metricRecordFixtureConfig("finite-metric-record"),
    error: /finite-metric-record key identity unsupported at Root\.adjustedMetrics/,
  },
  {
    name: "a finite metric record with a configured object key root",
    source: `export interface Metric { readonly key: string; }
export interface Root { readonly adjustedMetrics: Readonly<Partial<Record<Metric, number>>>; }
`,
    config: metricRecordFixtureConfig("finite-metric-record"),
    error: /finite-metric-record key root must render string-literal-enum at Root\.adjustedMetrics/,
  },
  {
    name: "a finite metric record with never values",
    source: `export type Metric = "a" | "b";
export interface Root { readonly adjustedMetrics: Readonly<Partial<Record<Metric, never>>>; }
`,
    config: metricRecordFixtureConfig("finite-metric-record"),
    error: /finite-metric-record requires Readonly<Partial<Record<Key, number>>> at Root\.adjustedMetrics/,
  },
  {
    name: "an empty metric record with number values",
    source: `export type Metric = "a" | "b";
export interface Root { readonly adjustedMetrics: Readonly<Partial<Record<Metric, number>>>; }
`,
    config: metricRecordFixtureConfig("empty-metric-record"),
    error: /empty-metric-record requires Readonly<Partial<Record<Key, never>>> at Root\.adjustedMetrics/,
  },
  {
    name: "an empty metric record without its Partial wrapper",
    source: `export type Metric = "a" | "b";
export interface Root { readonly adjustedMetrics: Readonly<Record<Metric, never>>; }
`,
    config: metricRecordFixtureConfig("empty-metric-record"),
    error: /empty-metric-record requires Readonly<Partial<Record<Key, never>>> at Root\.adjustedMetrics/,
  },
  {
    name: "an empty metric record with primitive string keys",
    source: `export type Metric = "a" | "b";
export interface Root { readonly adjustedMetrics: Readonly<Partial<Record<string, never>>>; }
`,
    config: metricRecordFixtureConfig("empty-metric-record"),
    error: /empty-metric-record key identity unsupported at Root\.adjustedMetrics/,
  },
  {
    name: "a mutable empty tuple mapped as an empty JSON array",
    source: "export interface Root { readonly appliedMetrics: []; }\n",
    config: fixtureConfig("Root", { appliedMetrics: "empty-json-array" }),
    error: /empty-json-array requires readonly \[\] at Root\.appliedMetrics/,
  },
  {
    name: "a nonempty tuple mapped as an empty JSON array",
    source: "export interface Root { readonly appliedMetrics: readonly [never]; }\n",
    config: fixtureConfig("Root", { appliedMetrics: "empty-json-array" }),
    error: /empty-json-array requires readonly \[\] at Root\.appliedMetrics/,
  },
  {
    name: "an empty JSON array mapping applied to exact null",
    source: "export interface Root { readonly appliedMetrics: null; }\n",
    config: fixtureConfig("Root", { appliedMetrics: "empty-json-array" }),
    error: /empty-json-array requires readonly \[\] at Root\.appliedMetrics/,
  },
  {
    name: "a string literal mapped as a const string property",
    source: "export interface Root { readonly contractVersion: \"v1\"; }\n",
    config: fixtureConfig("Root", { contractVersion: "const-string-property" }),
    error: /const-string-property requires typeof LOCAL_CONST\.member at Root\.contractVersion/,
  },
  {
    name: "a builtin-frozen string property without an exact const assertion",
    source: `const POLICY = Object.freeze({ cohort: "first" });
export interface Root { readonly cohort: typeof POLICY.cohort; }
`,
    config: fixtureConfig("Root", {
      cohort: "frozen-const-string-property",
    }),
    error: /frozen const string property source unsupported at root Root/,
  },
  {
    name: "a frozen string property through a shadowed Object value",
    source: `const Object = {
  freeze<T>(value: T): T { return value; },
};
const POLICY = Object.freeze({ cohort: "first" } as const);
export interface Root { readonly cohort: typeof POLICY.cohort; }
`,
    config: fixtureConfig("Root", {
      cohort: "frozen-const-string-property",
    }),
    error: /frozen const string property utility identity unsupported at Root\.cohort/,
  },
  {
    name: "a frozen string-property union with only one direct query",
    source: `const POLICY = Object.freeze({ scope: "first" } as const);
export interface Root { readonly scope: typeof POLICY.scope; }
`,
    config: fixtureConfig("Root", {
      scope: "frozen-const-string-property-union",
    }),
    error: /frozen-const-string-property-union requires two direct typeof queries at Root\.scope/,
  },
  {
    name: "a const string property hidden behind a local type alias",
    source: `const VERSIONS = { decision: "v1" } as const;
type Version = typeof VERSIONS.decision;
export interface Root { readonly contractVersion: Version; }
`,
    config: fixtureConfig("Root", { contractVersion: "const-string-property" }),
    error: /const-string-property requires typeof LOCAL_CONST\.member at Root\.contractVersion/,
  },
  {
    name: "an unresolved const string property object name",
    source: "export interface Root { readonly contractVersion: typeof MISSING.decision; }\n",
    config: fixtureConfig("Root", { contractVersion: "const-string-property" }),
    error: /const string property identity unsupported at root Root/,
  },
  {
    name: "an imported const string property object",
    source: `import { VERSIONS } from "./foreign";
export interface Root { readonly contractVersion: typeof VERSIONS.decision; }
`,
    config: fixtureConfig("Root", { contractVersion: "const-string-property" }),
    error: /const string property identity unsupported at root Root/,
  },
  {
    name: "a mutable const string property object binding",
    source: `let VERSIONS = { decision: "v1" } as const;
export interface Root { readonly contractVersion: typeof VERSIONS.decision; }
`,
    config: fixtureConfig("Root", { contractVersion: "const-string-property" }),
    error: /const string property source unsupported at root Root/,
  },
  {
    name: "a const string property object without as const",
    source: `const VERSIONS = { decision: "v1" };
export interface Root { readonly contractVersion: typeof VERSIONS.decision; }
`,
    config: fixtureConfig("Root", { contractVersion: "const-string-property" }),
    error: /const string property source unsupported at root Root/,
  },
  {
    name: "a const string property object with a non-string sibling",
    source: `const VERSIONS = { decision: "v1", epoch: 1 } as const;
export interface Root { readonly contractVersion: typeof VERSIONS.decision; }
`,
    config: fixtureConfig("Root", { contractVersion: "const-string-property" }),
    error: /const string property source unsupported at root Root/,
  },
  {
    name: "a const string property object with a spread",
    source: `const BASE = { candidate: "v1" } as const;
const VERSIONS = { ...BASE, decision: "v1" } as const;
export interface Root { readonly contractVersion: typeof VERSIONS.decision; }
`,
    config: fixtureConfig("Root", { contractVersion: "const-string-property" }),
    error: /const string property source unsupported at root Root/,
  },
  {
    name: "a const string property object with a computed member",
    source: `const VERSIONS = { ["decision"]: "v1" } as const;
export interface Root { readonly contractVersion: typeof VERSIONS.decision; }
`,
    config: fixtureConfig("Root", { contractVersion: "const-string-property" }),
    error: /const string property source unsupported at root Root/,
  },
  {
    name: "a const string property object with a duplicate member",
    source: `const VERSIONS = { decision: "v1", decision: "v2" } as const;
export interface Root { readonly contractVersion: typeof VERSIONS.decision; }
`,
    config: fixtureConfig("Root", { contractVersion: "const-string-property" }),
    error: /const string property source unsupported at root Root/,
  },
  {
    name: "a missing const string property member",
    source: `const VERSIONS = { candidate: "v1" } as const;
export interface Root { readonly contractVersion: typeof VERSIONS.decision; }
`,
    config: fixtureConfig("Root", { contractVersion: "const-string-property" }),
    error: /const string property source unsupported at root Root/,
  },
  {
    name: "a nullable union mapped as exact null",
    source: "export interface Root { readonly reasonCode: null | \"none\"; }\n",
    config: fixtureConfig("Root", { reasonCode: "exact-null" }),
    error: /exact-null requires null literal at Root\.reasonCode/,
  },
  {
    name: "an empty readonly tuple mapped as exact null",
    source: "export interface Root { readonly reasonCode: readonly []; }\n",
    config: fixtureConfig("Root", { reasonCode: "exact-null" }),
    error: /exact-null requires null literal at Root\.reasonCode/,
  },
  {
    name: "a finite metric record nominal and root collision",
    source: `export type Metric = "a" | "b";
export interface RootAdjustedMetricsRecord { readonly value: string; }
export interface Root { readonly adjustedMetrics: Readonly<Partial<Record<Metric, number>>>; }
`,
    config: {
      ...metricRecordFixtureConfig("finite-metric-record"),
      roots: [
        { source: "fixture.ts", exportName: "Metric", fields: {} },
        { source: "fixture.ts", exportName: "RootAdjustedMetricsRecord", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: { adjustedMetrics: "finite-metric-record" },
        },
      ],
    },
    error: /generated Rust type collision RootAdjustedMetricsRecord/,
  },
  {
    name: "an empty JSON array nominal and root collision",
    source: `export interface RootAppliedMetricsEmptyArray { readonly value: string; }
export interface Root { readonly appliedMetrics: readonly []; }
`,
    config: {
      ...fixtureConfig("Root"),
      roots: [
        { source: "fixture.ts", exportName: "RootAppliedMetricsEmptyArray", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: { appliedMetrics: "empty-json-array" },
        },
      ],
    },
    error: /generated Rust type collision RootAppliedMetricsEmptyArray/,
  },
  {
    name: "a grouped numeric array element without a field mapping",
    source: "export interface Root { readonly values: readonly (number | null)[]; }\n",
    config: fixtureConfig("Root"),
    error: /unmapped numeric field at Root\.values/,
  },
  {
    name: "an ordinary number mapped as an unsigned safe integer enum",
    source: "export interface Root { readonly level: number; }\n",
    config: fixtureConfig("Root", {
      level: "unsigned-safe-integer-enum",
    }),
    error: /unsigned-safe-integer-enum requires a required direct numeric-literal union at Root\.level/,
  },
  {
    name: "an optional field mapped as a required unsigned integer enum",
    source: "export interface Root { readonly level?: 1 | 2; }\n",
    config: fixtureConfig("Root", {
      level: "unsigned-safe-integer-enum",
    }),
    error: /unsigned-safe-integer-enum requires a required direct numeric-literal union at Root\.level/,
  },
  {
    name: "an unsafe member of an unsigned integer field enum",
    source: "export interface Root { readonly level: 1 | 9007199254740992; }\n",
    config: fixtureConfig("Root", {
      level: "unsigned-safe-integer-enum",
    }),
    error: /unsafe integer literal "9007199254740992" at root RootLevelEnum/,
  },
  {
    name: "a grouped mixed array union",
    source: "export interface Root { readonly values: readonly (number | string)[]; }\n",
    config: fixtureConfig("Root", { values: "finite-f64" }),
    error: /union type unsupported at Root\.values/,
  },
  {
    name: "a mutable array with grouped elements",
    source: "export interface Root { readonly values: (number | null)[]; }\n",
    config: fixtureConfig("Root", { values: "finite-f64" }),
    error: /mutable array or tuple unsupported at Root\.values/,
  },
  {
    name: "an exact null field outside a tagged payload",
    source: `export interface Root { readonly value: null; }`,
    config: fixtureConfig("Root"),
    error: /literal type unsupported at Root\.value/,
  },
  {
    name: "an object union without an explicit discriminator configuration",
    source: `export type Root =
  | { readonly state: "active"; readonly value: string }
  | { readonly state: "disabled"; readonly reason: string };
`,
    config: fixtureConfig("Root"),
    error: /root type unsupported at Root/,
  },
  {
    name: "an unconfigured tagged intersection base",
    source: `interface Base { readonly inherited: string; }
export type Root = Base & (
  | { readonly kind: "first"; readonly value: string }
  | { readonly kind: "second"; readonly value: string }
);
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        base: "Base",
        discriminator: "kind",
        fields: {},
      }],
    },
    error: /configured tagged union base identity unsupported at root Root/,
  },
  {
    name: "a tagged intersection with the union before its base",
    source: `export interface Base {
  readonly count: number;
  readonly label: string;
}
export type OtherKey = "other" | "alternate";
export type Root = (
  | { readonly kind: "first"; readonly value: string }
  | { readonly kind: "second"; readonly value: string }
) & Base;
`,
    config: taggedBaseFixtureConfig({}),
    error: /configured tagged union base shape unsupported at root Root/,
  },
  {
    name: "a null discriminator in a tagged intersection",
    source: `export interface Base {
  readonly count: number;
  readonly label: string;
}
export type OtherKey = "other" | "alternate";
export type Root = Base & (
  | { readonly kind: null; readonly value: string }
  | { readonly kind: "second"; readonly value: string }
);
`,
    config: taggedBaseFixtureConfig({}),
    error: /base-tagged union discriminator must be one exact string literal at root Root/,
  },
  {
    name: "a stale qualified tagged intersection field mapping",
    source: `export interface Base {
  readonly count: number;
  readonly label: string;
}
export type OtherKey = "other" | "alternate";
export type Root = Base & (
  | { readonly kind: "first"; readonly value: string }
  | { readonly kind: "second"; readonly value: string }
);
`,
    config: taggedBaseFixtureConfig({ "[first].missing": "const-string" }),
    error: /tagged union field mapping targets no field at Root\.\[first\]\.missing/,
  },
  {
    name: "a non-const qualified tagged intersection mapping",
    source: `export interface Base {
  readonly count: number;
  readonly label: string;
}
export type OtherKey = "other" | "alternate";
export type Root = Base & (
  | { readonly kind: "first"; readonly value: number }
  | { readonly kind: "second"; readonly value: number }
);
`,
    config: taggedBaseFixtureConfig({ "[first].value": "finite-f64" }),
    error: /config\.roots\[2\]\.fields\.\[first\]\.value must use const-string/,
  },
  {
    name: "a tagged intersection arm redeclaring an inherited property",
    source: `export interface Base { readonly inherited: string; }
export type Root = Base & (
  | { readonly inherited: string; readonly kind: "first" }
  | { readonly kind: "second"; readonly value: string }
);
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          base: "Base",
          discriminator: "kind",
          fields: {},
        },
      ],
    },
    error: /inherited property redeclaration unsupported at Root\.inherited/,
  },
  {
    name: "a tagged intersection missing its arm-qualified mapping",
    source: `export interface Base {
  readonly count: number;
  readonly label: string;
}
export type OtherKey = "other" | "alternate";
const FIRST_KEY = "first-key" as const;
export type Root = Base & (
  | { readonly key: typeof FIRST_KEY; readonly kind: "first" }
  | { readonly key: OtherKey; readonly kind: "second" }
);
`,
    config: taggedBaseFixtureConfig({}),
    error: /TypeScript type unsupported at Root\.key: TypeQuery/,
  },
  {
    name: "a tagged intersection mapping bound to the wrong arm",
    source: `export interface Base {
  readonly count: number;
  readonly label: string;
}
export type OtherKey = "other" | "alternate";
const FIRST_KEY = "first-key" as const;
export type Root = Base & (
  | { readonly key: OtherKey; readonly kind: "second" }
  | { readonly key: typeof FIRST_KEY; readonly kind: "first" }
);
`,
    config: taggedBaseFixtureConfig({ "[second].key": "const-string" }),
    error: /const-string requires typeof LOCAL_CONST at Root\.key/,
  },
  {
    name: "a tagged union arm missing the discriminator",
    source: `export type Root =
  | { readonly kind: "active"; readonly value: string }
  | { readonly reason: string };
`,
    config: taggedFixtureConfig("Root", "kind"),
    error: /tagged union requires exactly one discriminator property at Root\.kind/,
  },
  {
    name: "an optional tagged union discriminator",
    source: `export type Root =
  | { readonly kind?: "active"; readonly value: string }
  | { readonly kind: "disabled"; readonly reason: string };
`,
    config: taggedFixtureConfig("Root", "kind"),
    error: /tagged union discriminator must be required at Root\.kind/,
  },
  {
    name: "a duplicate tagged union discriminator property",
    source: `export type Root =
  | { readonly kind: "active"; readonly kind: "enabled"; readonly value: string }
  | { readonly kind: "disabled"; readonly reason: string };
`,
    config: taggedFixtureConfig("Root", "kind"),
    error: /tagged union requires exactly one discriminator property at Root\.kind/,
  },
  {
    name: "a widened tagged union discriminator",
    source: `export type Root =
  | { readonly kind: string; readonly value: string }
  | { readonly kind: "disabled"; readonly reason: string };
`,
    config: taggedFixtureConfig("Root", "kind"),
    error: /tagged union discriminator must be one exact string literal or null at Root\.kind/,
  },
  {
    name: "a non-string tagged union discriminator literal",
    source: `export type Root =
  | { readonly kind: 1; readonly value: string }
  | { readonly kind: "disabled"; readonly reason: string };
`,
    config: taggedFixtureConfig("Root", "kind"),
    error: /tagged union discriminator must be one exact string literal or null at Root\.kind/,
  },
  {
    name: "an indirect tagged union discriminator type",
    source: `type ActiveKind = "active";
export type Root =
  | { readonly kind: ActiveKind; readonly value: string }
  | { readonly kind: "disabled"; readonly reason: string };
`,
    config: taggedFixtureConfig("Root", "kind"),
    error: /tagged union discriminator must be one exact string literal or null at Root\.kind/,
  },
  {
    name: "overlapping tagged union wire values",
    source: `export type Root =
  | { readonly kind: "same"; readonly value: string }
  | { readonly kind: "same"; readonly reason: string };
`,
    config: taggedFixtureConfig("Root", "kind"),
    error: /duplicate tagged union wire value "same" at root Root/,
  },
  {
    name: "mixed tagged union branch forms",
    source: `export type Root =
  | { readonly kind: "active"; readonly value: string }
  | "disabled";
`,
    config: taggedFixtureConfig("Root", "kind"),
    error: /tagged union arm must be an inline object at root Root/,
  },
  {
    name: "a tagged union Rust variant collision",
    source: `export type Root =
  | { readonly kind: "a-b"; readonly value: string }
  | { readonly kind: "a_b"; readonly reason: string };
`,
    config: taggedFixtureConfig("Root", "kind"),
    error: /tagged union Rust variant collision AB at root Root/,
  },
  {
    name: "a tagged union payload and configured type collision",
    source: `export interface RootActive { readonly value: string; }
export type Root =
  | { readonly kind: "active"; readonly value: string }
  | { readonly kind: "disabled"; readonly reason: string };
`,
    config: {
      ...taggedFixtureConfig("Root", "kind"),
      roots: [
        {
          source: "fixture.ts",
          exportName: "Root",
          discriminator: "kind",
          fields: {},
        },
        { source: "fixture.ts", exportName: "RootActive", fields: {} },
      ],
    },
    error: /generated Rust type collision RootActive/,
  },
  {
    name: "a named tagged union arm",
    source: `interface Active { readonly kind: "active"; readonly value: string; }
export type Root =
  | Active
  | { readonly kind: "disabled"; readonly reason: string };
`,
    config: taggedFixtureConfig("Root", "kind"),
    error: /tagged union arm must be an inline object at root Root/,
  },
  {
    name: "a utility-wrapped tagged union arm",
    source: `export type Root =
  | Readonly<{ readonly kind: "active"; readonly value: string }>
  | { readonly kind: "disabled"; readonly reason: string };
`,
    config: taggedFixtureConfig("Root", "kind"),
    error: /tagged union arm must be an inline object at root Root/,
  },
  {
    name: "a negative numeric literal root",
    source: "export type Root = 1 | -2;\n",
    config: fixtureConfig("Root"),
    error: /unsigned decimal integer literal unsupported "-2" at root Root/,
  },
  {
    name: "a leading invalid numeric member before a mixed root member",
    source: "export type Root = -1 | \"x\";\n",
    config: fixtureConfig("Root"),
    error: /unsigned decimal integer literal unsupported "-1" at root Root/,
  },
  {
    name: "a fractional numeric literal root",
    source: "export type Root = 1 | 1.5;\n",
    config: fixtureConfig("Root"),
    error: /unsigned decimal integer literal unsupported "1\.5" at root Root/,
  },
  {
    name: "an exponent numeric literal root",
    source: "export type Root = 1 | 1e3;\n",
    config: fixtureConfig("Root"),
    error: /unsigned decimal integer literal unsupported "1e3" at root Root/,
  },
  {
    name: "an alternate-base numeric literal root",
    source: "export type Root = 1 | 0x10;\n",
    config: fixtureConfig("Root"),
    error: /unsigned decimal integer literal unsupported "0x10" at root Root/,
  },
  {
    name: "an unsafe numeric literal root",
    source: "export type Root = 1 | 9007199254740992;\n",
    config: fixtureConfig("Root"),
    error: /unsafe integer literal "9007199254740992" at root Root/,
  },
  {
    name: "a mixed string and numeric literal root",
    source: "export type Root = 1 | \"one\";\n",
    config: fixtureConfig("Root"),
    error: /unsigned decimal integer union unsupported at root Root/,
  },
  {
    name: "a duplicate numeric literal root",
    source: "export type Root = 1 | 1;\n",
    config: fixtureConfig("Root"),
    error: /duplicate numeric literal "1" at root Root/,
  },
  {
    name: "an unmapped number",
    source: "export interface Root { readonly count: number; }\n",
    config: fixtureConfig("Root"),
    error: /unmapped numeric field/,
  },
  {
    name: "an unpinned TypeScript version",
    source: "export interface Root { readonly value: string; }\n",
    config: {
      ...fixtureConfig("Root"),
      typescriptVersion: "6.0.2",
    },
    error: /TypeScript compiler version mismatch/,
  },
  {
    name: "a generic root",
    source: "export interface Root<T> { readonly value: T; }\n",
    config: fixtureConfig("Root"),
    error: /generic type unsupported/,
  },
  {
    name: "a Readonly utility alias",
    source: "export type Root = Readonly<{ readonly value: string }>;\n",
    config: fixtureConfig("Root"),
    error: /type reference unsupported at root Root/,
  },
  {
    name: "a ReadonlyArray utility field",
    source: "export interface Root { readonly values: ReadonlyArray<string>; }\n",
    config: fixtureConfig("Root"),
    error: /configured type reference unsupported at Root\.values/,
  },
  {
    name: "interface inheritance",
    source: "interface Base { readonly value: string; }\nexport interface Root extends Base {}\n",
    config: fixtureConfig("Root"),
    error: /interface inheritance unsupported/,
  },
  {
    name: "configured inheritance without an extends clause",
    source: "export interface Base { readonly value: string; }\nexport interface Root {}\n",
    config: inheritedFixtureConfig(),
    error: /configured interface base unsupported at root Root/,
  },
  {
    name: "omitted-base metadata without an ordinary base",
    source: "export interface Root { readonly value: string; }\n",
    config: {
      ...fixtureConfig("Root"),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        fields: {},
        omitBaseFields: ["value"],
      }],
    },
    error: /config\.roots\[0\]\.omitBaseFields requires an ordinary base/,
  },
  {
    name: "omitted-base fields in an order different from the source Omit",
    source: `export interface Base {
  readonly authorization: string;
  readonly contractVersion: string;
}
export interface Root
  extends Omit<Base, "authorization" | "contractVersion"> {
  readonly authorization: boolean;
  readonly contractVersion: "v2";
}
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {},
          base: "Base",
          omitBaseFields: ["contractVersion", "authorization"],
        },
      ],
    },
    error: /configured omitted interface fields unsupported at root Root/,
  },
  {
    name: "a locally shadowed Omit interface base",
    source: `type Omit<T, K> = T;
export interface Base { readonly value: string; }
export interface Root extends Omit<Base, "value"> {
  readonly value: boolean;
}
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          fields: {},
          base: "Base",
          omitBaseFields: ["value"],
        },
      ],
    },
    error: /configured omitted interface utility identity unsupported at root Root/,
  },
  {
    name: "an invalid configured base identifier",
    source: "export interface Root {}\n",
    config: {
      ...fixtureConfig("Root"),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        base: "bad-name",
        fields: {},
      }],
    },
    error: /config\.roots\[0\]\.base must be a non-underscore ASCII identifier/,
  },
  {
    name: "an unconfigured interface base",
    source: "export interface Base { readonly value: string; }\nexport interface Root extends Base {}\n",
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [{ source: "fixture.ts", exportName: "Root", base: "Base", fields: {} }],
    },
    error: /configured interface base identity unsupported at root Root/,
  },
  {
    name: "a configured base that differs from the extends target",
    source: `export interface Base { readonly value: string; }
export interface Other { readonly other: string; }
export interface Root extends Base {}
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        { source: "fixture.ts", exportName: "Other", fields: {} },
        { source: "fixture.ts", exportName: "Root", base: "Other", fields: {} },
      ],
    },
    error: /configured interface base identity unsupported at root Root/,
  },
  {
    name: "an interface base hidden behind a local alias",
    source: `export interface Base { readonly value: string; }
type BaseAlias = Base;
export interface Root extends BaseAlias {}
`,
    config: inheritedFixtureConfig(),
    error: /configured interface base identity unsupported at root Root/,
  },
  {
    name: "a merged configured interface base",
    source: `export interface Base { readonly first: string; }
export interface Base { readonly second: string; }
export interface Root extends Base {}
`,
    config: inheritedFixtureConfig(),
    error: /duplicate root declaration fixture\.ts#Base/,
  },
  {
    name: "a generic configured interface base",
    source: `export interface Base<T> { readonly value: T; }
export interface Root extends Base {}
`,
    config: inheritedFixtureConfig(),
    error: /generic type unsupported at root Base/,
  },
  {
    name: "a utility-applied interface base",
    source: `export interface Base { readonly value: string; }
export interface Root extends Readonly<Base> {}
`,
    config: inheritedFixtureConfig(),
    error: /configured interface base unsupported at root Root/,
  },
  {
    name: "multiple interface bases",
    source: `export interface Base { readonly value: string; }
export interface Other { readonly other: string; }
export interface Root extends Base, Other {}
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        { source: "fixture.ts", exportName: "Other", fields: {} },
        { source: "fixture.ts", exportName: "Root", base: "Base", fields: {} },
      ],
    },
    error: /configured interface base unsupported at root Root/,
  },
  {
    name: "a configured non-interface base",
    source: `export type Base = { readonly value: string };
export interface Root extends Base {}
`,
    config: inheritedFixtureConfig(),
    error: /configured interface base identity unsupported at root Root/,
  },
  {
    name: "an inherited property redeclaration",
    source: `export interface Base { readonly value: string; }
export interface Root extends Base { readonly value: string; }
`,
    config: inheritedFixtureConfig(),
    error: /inherited property redeclaration unsupported at Root\.value/,
  },
  {
    name: "child metadata targeting an inherited field",
    source: `export interface Base { readonly value: number; }
export interface Root extends Base {}
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        {
          source: "fixture.ts",
          exportName: "Base",
          fields: { value: "finite-f64" },
        },
        {
          source: "fixture.ts",
          exportName: "Root",
          base: "Base",
          fields: { value: "finite-f64" },
        },
      ],
    },
    error: /unused field mapping at Root\.value/,
  },
  {
    name: "base metadata on a type alias root",
    source: `export interface Base { readonly value: string; }
export type Root = { readonly child: string };
`,
    config: inheritedFixtureConfig(),
    error: /configured interface base unsupported at root Root/,
  },
  {
    name: "base metadata on a tagged union root",
    source: `export interface Base { readonly value: string; }
export type Root =
  | { readonly kind: "ready"; readonly value: string }
  | { readonly kind: "empty"; readonly value: null };
`,
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "fixture.ts", exportName: "Base", fields: {} },
        {
          source: "fixture.ts",
          exportName: "Root",
          base: "Base",
          discriminator: "kind",
          fields: {},
        },
      ],
    },
    error: /configured interface base unsupported at root Root/,
  },
  {
    name: "an index signature",
    source: "export interface Root { readonly [key: string]: string; }\n",
    config: fixtureConfig("Root"),
    error: /index signature unsupported/,
  },
  {
    name: "a mutable property",
    source: "export interface Root { value: string; }\n",
    config: fixtureConfig("Root"),
    error: /non-readonly property unsupported/,
  },
  {
    name: "an invalid underscore Rust field",
    source: "export interface Root { readonly _: string; }\n",
    config: fixtureConfig("Root"),
    error: /unsupported property name "_"/,
  },
  {
    name: "an invalid underscore Rust root",
    source: "export type _ = \"literal\";\n",
    config: fixtureConfig("_"),
    error: /exportName must be a non-underscore ASCII identifier/,
  },
  {
    name: "a non-root type reference",
    source: "interface Child { readonly value: string; }\nexport interface Root { readonly child: Child; }\n",
    config: fixtureConfig("Root"),
    error: /configured type reference identity unsupported at Root\.child/,
  },
  {
    name: "a widened string-literal field union",
    source: "export interface Root { readonly state: string | \"active\"; }\n",
    config: fixtureConfig("Root"),
    error: /union type unsupported at Root\.state/,
  },
  {
    name: "a nested generated string enum collision",
    source: `export interface Root {
  readonly aB: "one" | "two";
  readonly a: { readonly b: "three" | "four" };
}
`,
    config: fixtureConfig("Root"),
    error: /generated Rust type collision RootABEnum/,
  },
  {
    name: "a direct recursive configured reference",
    source: "export interface Root { readonly next: Root; }\n",
    config: fixtureConfig("Root"),
    error: /recursive configured type reference cycle: Root -> Root/,
  },
  {
    name: "a transitive recursive configured reference",
    source: `export interface First { readonly seconds: readonly Second[]; }
export interface Second { readonly third: Third | null; }
export interface Third { readonly first: First; }
`,
    config: fixtureRoots("First", "Second", "Third"),
    error: /recursive configured type reference cycle: First -> Second -> Third -> First/,
  },
  {
    name: "an imported same-name cross-source configured reference",
    sources: {
      "first.ts": "import type { Second } from \"./second\";\nexport interface First { readonly second: Second; }\n",
      "second.ts": "export interface Second { readonly value: string; }\n",
    },
    config: {
      version: 1,
      typescriptVersion: "6.0.3",
      roots: [
        { source: "first.ts", exportName: "First", fields: {} },
        { source: "second.ts", exportName: "Second", fields: {} },
      ],
    },
    error: /configured type reference identity unsupported at First\.second/,
  },
  {
    name: "an unresolved configured type reference",
    source: "export interface Root { readonly missing: Missing; }\n",
    config: fixtureConfig("Root"),
    error: /configured type reference identity unsupported at Root\.missing/,
  },
  {
    name: "a qualified configured type reference",
    source: `namespace Types {
  export interface Child { readonly value: string; }
}
export interface Root { readonly child: Types.Child; }
`,
    config: fixtureConfig("Root"),
    error: /configured type reference unsupported at Root\.child/,
  },
  {
    name: "a configured type reference with type arguments",
    source: `export interface Child<T> { readonly value: T; }
export interface Root { readonly child: Child<string>; }
`,
    config: fixtureRoots("Root", "Child"),
    error: /configured type reference unsupported at Root\.child/,
  },
  {
    name: "a generic configured type reference target",
    source: `export interface Child<T> { readonly value: T; }
export interface Root { readonly child: Child; }
`,
    config: fixtureRoots("Root", "Child"),
    error: /generic configured type reference unsupported at Root\.child/,
  },
  {
    name: "a duplicate configured root declaration",
    source: `export interface Child { readonly first: string; }
export interface Child { readonly second: string; }
export interface Root { readonly child: Child; }
`,
    config: fixtureRoots("Root", "Child"),
    error: /duplicate root declaration fixture\.ts#Child/,
  },
  {
    name: "a duplicate root",
    source: "export interface Root { readonly value: string; }\n",
    config: {
      ...fixtureConfig("Root"),
      roots: [
        { source: "fixture.ts", exportName: "Root", fields: {} },
        { source: "fixture.ts", exportName: "Root", fields: {} },
      ],
    },
    error: /duplicate root/,
  },
  {
    name: "a null catalog configuration",
    source: "export type Root = \"a\" | \"b\";\n",
    config: catalogFixtureConfig("Root", null),
    error: /config\.roots\[0\]\.catalog must be a plain object/,
  },
  {
    name: "an unknown catalog configuration kind",
    source: "export type Root = \"a\" | \"b\";\n",
    config: catalogFixtureConfig("Root", { kind: "other" }),
    error: /config\.roots\[0\]\.catalog\.kind must be satisfies-string-tuple, unsigned-safe-integer-tuple, extend-local-string-enum, string-property, or direct-const-string-tuple/,
  },
  {
    name: "an unsupported satisfies catalog configuration key",
    source: "export type Root = \"a\" | \"b\";\n",
    config: catalogFixtureConfig("Root", {
      kind: "satisfies-string-tuple",
      key: "key",
    }),
    error: /config\.roots\[0\]\.catalog has unsupported key "key"/,
  },
  {
    name: "a string-property catalog without a selected key",
    source: "export type Root = \"a\" | \"b\";\n",
    config: catalogFixtureConfig("Root", { kind: "string-property" }),
    error: /config\.roots\[0\]\.catalog\.key must be an ASCII property identifier/,
  },
  {
    name: "a string-property catalog with a non-identifier key",
    source: "export type Root = \"a\" | \"b\";\n",
    config: catalogFixtureConfig("Root", {
      kind: "string-property",
      key: "not-a-key",
    }),
    error: /config\.roots\[0\]\.catalog\.key must be an ASCII property identifier/,
  },
  {
    name: "catalog metadata combined with a configured base",
    source: "export type Root = \"a\" | \"b\";\n",
    config: {
      ...catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        base: "Base",
        catalog: { kind: "satisfies-string-tuple" },
        fields: {},
      }],
    },
    error: /catalog metadata cannot be combined with base or discriminator at config\.roots\[0\]/,
  },
  {
    name: "catalog metadata combined with a discriminator",
    source: "export type Root = \"a\" | \"b\";\n",
    config: {
      ...catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
      roots: [{
        source: "fixture.ts",
        exportName: "Root",
        discriminator: "kind",
        catalog: { kind: "satisfies-string-tuple" },
        fields: {},
      }],
    },
    error: /catalog metadata cannot be combined with base or discriminator at config\.roots\[0\]/,
  },
  {
    name: "a catalog-backed interface root",
    source: "export interface Root { readonly value: string; }\n",
    config: catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
    error: /catalog root requires a type alias at root Root/,
  },
  {
    name: "a direct string union with catalog metadata",
    source: "export type Root = \"a\" | \"b\";\n",
    config: catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
    error: /catalog type unsupported at root Root/,
  },
  {
    name: "a computed index for a satisfies string tuple catalog",
    source: `type ValueName = string;
const ROOT_VALUES = ["a", "b"] as const satisfies readonly ValueName[];
export type Root = (typeof ROOT_VALUES)[0 | 1];
`,
    config: catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
    error: /catalog type unsupported at root Root/,
  },
  {
    name: "an opted-in tuple without a satisfies guard",
    source: `const ROOT_VALUES = ["a", "b"] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
    error: /satisfies string tuple source unsupported at root Root/,
  },
  {
    name: "a satisfies tuple with a primitive element guard",
    source: `const ROOT_VALUES = ["a", "b"] as const satisfies readonly string[];
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
    error: /satisfies string tuple source unsupported at root Root/,
  },
  {
    name: "a satisfies tuple with a qualified named guard",
    source: `namespace Names { export type ValueName = string; }
const ROOT_VALUES = ["a", "b"] as const satisfies readonly Names.ValueName[];
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
    error: /satisfies string tuple source unsupported at root Root/,
  },
  {
    name: "an empty satisfies string tuple catalog",
    source: `type ValueName = string;
const ROOT_VALUES = [] as const satisfies readonly ValueName[];
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
    error: /satisfies string tuple source unsupported at root Root/,
  },
  {
    name: "a spread satisfies string tuple catalog",
    source: `type ValueName = string;
const SOURCE_VALUES = ["a"] as const;
const ROOT_VALUES = [...SOURCE_VALUES, "b"] as const satisfies readonly ValueName[];
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
    error: /satisfies string tuple source unsupported at root Root/,
  },
  {
    name: "an elided satisfies string tuple catalog member",
    source: `type ValueName = string;
const ROOT_VALUES = ["a", , "b"] as const satisfies readonly ValueName[];
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
    error: /satisfies string tuple source unsupported at root Root/,
  },
  {
    name: "a dynamic satisfies string tuple catalog member",
    source: `type ValueName = string;
const DYNAMIC_VALUE = "a";
const ROOT_VALUES = [DYNAMIC_VALUE, "b"] as const satisfies readonly ValueName[];
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
    error: /satisfies string tuple source unsupported at root Root/,
  },
  {
    name: "an imported satisfies string tuple catalog identity",
    source: `import { ROOT_VALUES } from "./foreign";
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
    error: /const string tuple identity unsupported at root Root/,
  },
  {
    name: "a satisfies string tuple catalog Rust variant collision",
    source: `type ValueName = string;
const ROOT_VALUES = ["a-b", "a_b"] as const satisfies readonly ValueName[];
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "satisfies-string-tuple" }),
    error: /Rust enum variant collision AB at root Root/,
  },
  {
    name: "a string-property catalog accessed through a different key",
    source: `const ROOT_ROWS = [{ key: "a", label: "first" }] as const;
export type Root = (typeof ROOT_ROWS)[number]["label"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /catalog type unsupported at root Root/,
  },
  {
    name: "a string-property catalog without an as const assertion",
    source: `const ROOT_ROWS = [{ key: "a", label: "first" }];
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "an empty string-property catalog",
    source: `const ROOT_ROWS = [] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "an elided string-property catalog row",
    source: `const ROOT_ROWS = [{ key: "a" }, , { key: "b" }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a spread string-property catalog row",
    source: `const SOURCE_ROWS = [{ key: "a" }] as const;
const ROOT_ROWS = [...SOURCE_ROWS, { key: "b" }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "an imported string-property catalog identity",
    source: `import { ROOT_ROWS } from "./foreign";
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /const string tuple identity unsupported at root Root/,
  },
  {
    name: "an empty string-property catalog object row",
    source: `const ROOT_ROWS = [{}] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a computed string-property catalog property",
    source: `const ROOT_ROWS = [{ ["key"]: "a", label: "first" }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a quoted string-property catalog property",
    source: `const ROOT_ROWS = [{ "key": "a", label: "first" }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "an object spread string-property catalog property",
    source: `const BASE_ROW = { label: "first" } as const;
const ROOT_ROWS = [{ key: "a", ...BASE_ROW }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a shorthand string-property catalog property",
    source: `const key = "a";
const ROOT_ROWS = [{ key, label: "first" }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a method string-property catalog property",
    source: `const ROOT_ROWS = [{ key() { return "a"; }, label: "first" }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a duplicate string-property catalog property",
    source: `const ROOT_ROWS = [{ key: "a", key: "b" }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a __proto__ string-property catalog property",
    source: `const ROOT_ROWS = [{ key: "a", __proto__: "blocked" }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a string-property catalog row missing the selected property",
    source: `const ROOT_ROWS = [{ label: "first", maximumHours: 24 }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a numeric selected string-property catalog value",
    source: `const ROOT_ROWS = [{ key: 1, label: "first" }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a dynamic nonselected string-property catalog value",
    source: `const LABEL = "first";
const ROOT_ROWS = [{ key: "a", label: LABEL }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a boolean nonselected string-property catalog value",
    source: `const ROOT_ROWS = [{ key: "a", enabled: true }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a negative string-property catalog integer",
    source: `const ROOT_ROWS = [{ key: "a", minimumHours: -1 }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "an exponent string-property catalog integer",
    source: `const ROOT_ROWS = [{ key: "a", maximumHours: 1e3 }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a fractional string-property catalog integer",
    source: `const ROOT_ROWS = [{ key: "a", maximumHours: 1.5 }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "an unsafe string-property catalog integer",
    source: `const ROOT_ROWS = [{ key: "a", maximumHours: 9007199254740992 }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /string property catalog source unsupported at root Root/,
  },
  {
    name: "a duplicate selected string-property catalog value",
    source: `const ROOT_ROWS = [{ key: "same" }, { key: "same" }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /duplicate string literal "same" at root Root/,
  },
  {
    name: "a string-property catalog Rust variant collision",
    source: `const ROOT_ROWS = [{ key: "a-b" }, { key: "a_b" }] as const;
export type Root = (typeof ROOT_ROWS)[number]["key"];
`,
    config: catalogFixtureConfig("Root", { kind: "string-property", key: "key" }),
    error: /Rust enum variant collision AB at root Root/,
  },
  {
    name: "a computed unsigned safe integer tuple index",
    source: `const ROOT_VALUES = [24, 48] as const;
export type Root = (typeof ROOT_VALUES)[0 | 1];
`,
    config: catalogFixtureConfig("Root", { kind: "unsigned-safe-integer-tuple" }),
    error: /unsigned safe integer tuple type unsupported at root Root/,
  },
  {
    name: "an unsigned safe integer tuple through a const alias",
    source: `const SOURCE_VALUES = [24, 48] as const;
const ROOT_VALUES = SOURCE_VALUES;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "unsigned-safe-integer-tuple" }),
    error: /unsigned safe integer tuple source unsupported at root Root/,
  },
  {
    name: "an empty unsigned safe integer tuple",
    source: `const ROOT_VALUES = [] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "unsigned-safe-integer-tuple" }),
    error: /unsigned safe integer tuple source unsupported at root Root/,
  },
  {
    name: "a signed unsigned safe integer tuple member",
    source: `const ROOT_VALUES = [24, -48] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "unsigned-safe-integer-tuple" }),
    error: /unsigned safe integer tuple source unsupported at root Root/,
  },
  {
    name: "a fractional unsigned safe integer tuple member",
    source: `const ROOT_VALUES = [24, 48.5] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "unsigned-safe-integer-tuple" }),
    error: /unsigned decimal integer literal unsupported "48\.5" at root Root/,
  },
  {
    name: "an earlier invalid numeric tuple token before invalid syntax",
    source: `const ROOT_VALUES = [48.5, -1] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "unsigned-safe-integer-tuple" }),
    error: /unsigned decimal integer literal unsupported "48\.5" at root Root/,
  },
  {
    name: "an unsafe unsigned safe integer tuple member",
    source: `const ROOT_VALUES = [24, 9007199254740992] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "unsigned-safe-integer-tuple" }),
    error: /unsafe integer literal "9007199254740992" at root Root/,
  },
  {
    name: "a duplicate unsigned safe integer tuple member",
    source: `const ROOT_VALUES = [24, 24] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "unsigned-safe-integer-tuple" }),
    error: /duplicate numeric literal "24" at root Root/,
  },
  {
    name: "an imported unsigned safe integer tuple identity",
    source: `import { ROOT_VALUES } from "./foreign";
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: catalogFixtureConfig("Root", { kind: "unsigned-safe-integer-tuple" }),
    error: /unsigned safe integer tuple identity unsupported at root Root/,
  },
  {
    name: "a computed const string tuple index",
    source: `const ROOT_VALUES = ["a", "b"] as const;
export type Root = (typeof ROOT_VALUES)[0 | 1];
`,
    config: fixtureConfig("Root"),
    error: /const string tuple root unsupported at Root: expected \(typeof LOCAL_CONST\)\[number\]/,
  },
  {
    name: "a const string tuple through a const alias",
    source: `const SOURCE_VALUES = ["a", "b"] as const;
const ROOT_VALUES = SOURCE_VALUES;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: fixtureConfig("Root"),
    error: /const string tuple source unsupported at root Root/,
  },
  {
    name: "an annotated const string tuple",
    source: `const ROOT_VALUES: readonly ["a", "b"] = ["a", "b"] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: fixtureConfig("Root"),
    error: /const string tuple source unsupported at root Root/,
  },
  {
    name: "a non-const string tuple declaration",
    source: `let ROOT_VALUES = ["a", "b"] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: fixtureConfig("Root"),
    error: /const string tuple source unsupported at root Root/,
  },
  {
    name: "a spread const string tuple",
    source: `const SOURCE_VALUES = ["a"] as const;
const ROOT_VALUES = [...SOURCE_VALUES, "b"] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: fixtureConfig("Root"),
    error: /const string tuple source unsupported at root Root/,
  },
  {
    name: "an elided const string tuple member",
    source: `const ROOT_VALUES = ["a", , "b"] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: fixtureConfig("Root"),
    error: /const string tuple source unsupported at root Root/,
  },
  {
    name: "a non-string const tuple member",
    source: `const ROOT_VALUES = ["a", 1] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: fixtureConfig("Root"),
    error: /const string tuple source unsupported at root Root/,
  },
  {
    name: "a dynamic const string tuple member",
    source: `const DYNAMIC_VALUE = "a";
const ROOT_VALUES = [DYNAMIC_VALUE, "b"] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: fixtureConfig("Root"),
    error: /const string tuple source unsupported at root Root/,
  },
  {
    name: "a satisfies const string tuple",
    source: `const ROOT_VALUES = ["a", "b"] as const satisfies readonly string[];
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: fixtureConfig("Root"),
    error: /const string tuple source unsupported at root Root/,
  },
  {
    name: "a duplicate const string tuple literal",
    source: `const ROOT_VALUES = ["same", "same"] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: fixtureConfig("Root"),
    error: /duplicate string literal "same" at root Root/,
  },
  {
    name: "a duplicate const string tuple declaration identity",
    source: `const ROOT_VALUES = ["a"] as const;
const ROOT_VALUES = ["b"] as const;
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: fixtureConfig("Root"),
    error: /const string tuple identity unsupported at root Root/,
  },
  {
    name: "an imported const string tuple identity",
    source: `import { ROOT_VALUES } from "./foreign";
export type Root = (typeof ROOT_VALUES)[number];
`,
    config: fixtureConfig("Root"),
    error: /const string tuple identity unsupported at root Root/,
  },
  {
    name: "a default-only named root",
    source: "export default interface Root { readonly value: string; }\n",
    config: fixtureConfig("Root"),
    error: /root must be a named export fixture\.ts#Root/,
  },
  {
    name: "unused field metadata",
    source: "export interface Root { readonly value: string; }\n",
    config: fixtureConfig("Root", { missing: "utc-iso" }),
    error: /unused field mapping/,
  },
  {
    name: "incompatible field metadata",
    source: "export interface Root { readonly value: string; }\n",
    config: fixtureConfig("Root", { value: "safe-unsigned-integer" }),
    error: /wrong field mapping/,
  },
  {
    name: "a Nullable helper and root collision",
    source: `export type Nullable = "literal";
export interface Root { readonly value: string | null; }
`,
    config: {
      ...fixtureConfig("Nullable"),
      roots: [
        { source: "fixture.ts", exportName: "Nullable", fields: {} },
        { source: "fixture.ts", exportName: "Root", fields: {} },
      ],
    },
    error: /exportName collides with a generated Rust type/,
  },
  {
    name: "a FieldPresence helper and root collision",
    source: `export type FieldPresence = "literal";
export interface Root { readonly value?: string; }
`,
    config: {
      ...fixtureConfig("FieldPresence"),
      roots: [
        { source: "fixture.ts", exportName: "FieldPresence", fields: {} },
        { source: "fixture.ts", exportName: "Root", fields: {} },
      ],
    },
    error: /exportName collides with a generated Rust type/,
  },
  {
    name: "a String storage type and root collision",
    source: "export type String = \"literal\";\n",
    config: fixtureConfig("String"),
    error: /exportName collides with a generated Rust type/,
  },
  {
    name: "an explicit undefined union",
    source: "export interface Root { readonly value: string | undefined; }\n",
    config: fixtureConfig("Root"),
    error: /undefined type unsupported/,
  },
  {
    name: "an isolated UTF-16 surrogate literal",
    source: "export interface Root { readonly value: \"\\uD800\"; }\n",
    config: fixtureConfig("Root"),
    error: /wire string contains an isolated surrogate code point/,
  },
];

// register one focused rejection test per unsupported boundary
for (const scenario of REJECTED_SUBSET_CASES) {
  test(
    `rejects ${scenario.name}`,
    // exercise one closed-subset failure
    () => {
      assert.throws(
        // reject before emitting partial output
        () => generateRustContracts(
          scenario.config,
          scenario.sources ?? { "fixture.ts": scenario.source },
        ),
        scenario.error,
      );
    },
  );
}
