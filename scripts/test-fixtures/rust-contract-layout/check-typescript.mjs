import assert from "node:assert/strict";
import { Buffer } from "node:buffer";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";

import ts from "typescript";

const TYPESCRIPT_VERSION = "6.0.3";
const LIB_ES5_SHA256 =
  "bcd24271a113971ba9eb71ff8cb01bc6b0f872a85c23fdbe5d93065b375933cd";
const PG_INDEX_SHA256 =
  "1f7da5b623c2b1fc4f6fbb56a19df7f6fe2f3e3e7e8b526ec6892062f0f1f82e";
const VIRTUAL_LIB_PATH = "/__rust_contract_layout__/lib.es5.d.ts";
const VIRTUAL_SOURCE_PATH = "/__rust_contract_layout__/fixture.ts";
const SELECTIONS = [
  {
    source: "apps/api/src/index.ts",
    exportName: "ApiAirQualityHourlyContext",
    kind: "interface",
  },
  {
    source: "apps/api/src/index.ts",
    exportName: "ApiPropertySensorSnapshot",
    kind: "interface",
  },
  {
    source: "apps/api/src/index.ts",
    exportName: "ForecastDays",
    kind: "type",
  },
  {
    source: "packages/forecast-adjustment/src/performance-scorecard.ts",
    exportName: "ForecastAdjustmentBootstrapSummary",
    kind: "interface",
  },
  {
    source: "packages/forecast-adjustment/src/performance-scorecard.ts",
    exportName: "ForecastAdjustmentReliabilityBin",
    kind: "interface",
  },
  {
    source: "packages/forecast-adjustment/src/performance-scorecard.ts",
    exportName: "ForecastAdjustmentRainThresholdSummary",
    kind: "interface",
  },
  {
    source:
      "packages/forecast-adjustment/src/maintenance-shadow-catalog.ts",
    exportName: "InstalledRainMaintenanceControlReference",
    kind: "interface",
  },
  {
    source: "node_modules/@types/pg/index.d.ts",
    exportName: "QueryResultRow",
    kind: "interface",
  },
  {
    source: "packages/database/src/repositories.ts",
    exportName: "DailyPrecipitationRow",
    kind: "interface",
  },
  {
    source: "packages/domain/src/provenance.ts",
    exportName: "JsonPrimitive",
    kind: "type",
  },
  {
    source: "packages/domain/src/provenance.ts",
    exportName: "JsonValue",
    kind: "type",
  },
  {
    source:
      "packages/forecast-adjustment/src/maintenance-shadow-catalog.ts",
    exportName: "InstalledMaintenanceShadowReceiptV1",
    kind: "interface",
  },
  {
    source:
      "packages/forecast-adjustment/src/maintenance-shadow-catalog.ts",
    exportName: "InstalledMaintenanceShadowReceiptV2",
    kind: "interface",
  },
  {
    source:
      "packages/forecast-adjustment/src/maintenance-shadow-catalog.ts",
    exportName: "InstalledMaintenanceShadowReceipt",
    kind: "type",
  },
];
const EXPECTED_INVALID_CASES = [
  { id: "invalid-air-quality", code: 2322 },
  { id: "missing-channel", code: 2741 },
  { id: "invalid-forecast-days", code: 2322 },
  { id: "invalid-bootstrap-count", code: 2322 },
  { id: "invalid-raw-brier", code: 2322 },
  { id: "invalid-rain-threshold", code: 2322 },
  { id: "invalid-control-bytes", code: 2322 },
  { id: "invalid-precipitation-timestamp", code: 2322 },
  { id: "invalid-json-member", code: 2322 },
  { id: "invalid-install-receipt-version", code: 2820 },
  { id: "missing-receipt-member-root", code: 2741 },
];

// read one hard-coded repository path relative to this checker
function readRepositoryText(path) {
  return readFileSync(new URL(`../../../${path}`, import.meta.url), "utf8");
}

// parse one closed typescript input and reject syntax drift
function parseTypeScript(source, fileName) {
  const sourceFile = ts.createSourceFile(
    fileName,
    source,
    ts.ScriptTarget.ES2023,
    true,
    ts.ScriptKind.TS,
  );
  assert.equal(sourceFile.parseDiagnostics.length, 0, fileName);
  return sourceFile;
}

// require one exact exported nongeneric interface or type alias
function selectedDeclarationText(selection) {
  const source = readRepositoryText(selection.source);
  const sourceFile = parseTypeScript(source, selection.source);
  const matches = sourceFile.statements.filter(
    // select only the requested top-level declaration kind and name
    (statement) => {
      const kindMatches = selection.kind === "interface"
        ? ts.isInterfaceDeclaration(statement)
        : ts.isTypeAliasDeclaration(statement);
      return kindMatches && statement.name.text === selection.exportName;
    },
  );
  assert.equal(matches.length, 1, `${selection.source}#${selection.exportName}`);
  const declaration = matches[0];
  const modifiers = declaration.modifiers ?? [];
  assert.equal(
    modifiers.filter(
      // require exactly one ordinary export modifier
      (modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword,
    ).length,
    1,
    `${selection.source}#${selection.exportName}`,
  );
  assert.equal(
    modifiers.some(
      // reject default-exported declaration identity
      (modifier) => modifier.kind === ts.SyntaxKind.DefaultKeyword,
    ),
    false,
    `${selection.source}#${selection.exportName}`,
  );
  assert.equal(
    declaration.typeParameters?.length ?? 0,
    0,
    `${selection.source}#${selection.exportName}`,
  );
  return declaration.getText(sourceFile);
}

const selectedDeclarations = SELECTIONS.map(
  // retain only exact source declaration text in configured order
  (selection) => selectedDeclarationText(selection),
);
assert.equal(SELECTIONS.length, 14);
assert.equal(selectedDeclarations.length, SELECTIONS.length);
assert.equal(new Set(selectedDeclarations).size, SELECTIONS.length);
assert.equal(ts.version, TYPESCRIPT_VERSION);

const libBytes = readFileSync(
  new URL("../../../node_modules/typescript/lib/lib.es5.d.ts", import.meta.url),
);
assert.equal(createHash("sha256").update(libBytes).digest("hex"), LIB_ES5_SHA256);
const libText = libBytes.toString("utf8");
assert.deepEqual(Buffer.from(libText, "utf8"), libBytes);
const pgBytes = readFileSync(
  new URL("../../../node_modules/@types/pg/index.d.ts", import.meta.url),
);
assert.equal(createHash("sha256").update(pgBytes).digest("hex"), PG_INDEX_SHA256);
assert.deepEqual(Buffer.from(pgBytes.toString("utf8"), "utf8"), pgBytes);

// compile one fixture against only selected declarations and pinned lib.es5
function compileFixture(path) {
  const fixtureText = readFileSync(new URL(path, import.meta.url), "utf8");
  assert.doesNotMatch(fixtureText, /@ts-(?:ignore|expect-error)/u);
  const combinedSource = `${selectedDeclarations.join("\n\n")}\n\n${fixtureText}`;
  const sourceFile = parseTypeScript(combinedSource, VIRTUAL_SOURCE_PATH);
  const libSourceFile = parseTypeScript(libText, VIRTUAL_LIB_PATH);
  const sources = new Map([
    [VIRTUAL_SOURCE_PATH, sourceFile],
    [VIRTUAL_LIB_PATH, libSourceFile],
  ]);
  const options = {
    exactOptionalPropertyTypes: true,
    module: ts.ModuleKind.ESNext,
    noEmit: true,
    noLib: true,
    noResolve: true,
    skipLibCheck: false,
    strict: true,
    target: ts.ScriptTarget.ES2023,
    types: [],
  };
  const host = {
    // expose only the two explicit in-memory source files
    getSourceFile(fileName) {
      return sources.get(fileName);
    },
    // name the pinned virtual library without loading a default
    getDefaultLibFileName() {
      return VIRTUAL_LIB_PATH;
    },
    // keep every virtual identity independent of the process cwd
    getCurrentDirectory() {
      return "/__rust_contract_layout__";
    },
    // preserve exact case-sensitive virtual paths
    getCanonicalFileName(fileName) {
      return fileName;
    },
    // compile with repository case-sensitive identity rules
    useCaseSensitiveFileNames() {
      return true;
    },
    // stabilize compiler-created diagnostic formatting
    getNewLine() {
      return "\n";
    },
    // expose only explicitly selected virtual inputs
    fileExists(fileName) {
      return sources.has(fileName);
    },
    // return exact in-memory text without filesystem fallback
    readFile(fileName) {
      return sources.get(fileName)?.text;
    },
    // fail if noEmit ever reaches an output boundary
    writeFile() {
      assert.fail("unexpected TypeScript emit");
    },
  };
  const program = ts.createProgram(
    [VIRTUAL_SOURCE_PATH, VIRTUAL_LIB_PATH],
    options,
    host,
  );
  assert.equal(program.getSourceFile(VIRTUAL_SOURCE_PATH), sourceFile);
  assert.equal(program.getSourceFile(VIRTUAL_LIB_PATH), libSourceFile);
  return {
    diagnostics: ts.getPreEmitDiagnostics(program),
    fixtureText,
    sourceFile,
  };
}

// read exact unique mismatch markers in fixture order
function invalidFixtureMarkers(fixtureText) {
  const fixtureSource = parseTypeScript(fixtureText, "invalid.ts");
  assert.equal(fixtureSource.statements.length, EXPECTED_INVALID_CASES.length);
  const markers = [];

  // retain one marker from every nonempty offending statement line
  for (const line of fixtureText.split(/\r?\n/u)) {
    const match = /\/\/ mismatch:([a-z0-9-]+)$/u.exec(line);

    // skip only the final empty line
    if (line.length === 0) {
      continue;
    }
    assert.ok(match, line);
    markers.push(match[1]);
  }
  assert.equal(markers.length, EXPECTED_INVALID_CASES.length);
  assert.equal(new Set(markers).size, EXPECTED_INVALID_CASES.length);
  return markers;
}

const valid = compileFixture("./valid.ts");
assert.deepEqual(valid.diagnostics, []);

const invalid = compileFixture("./invalid.ts");
const markerOrder = invalidFixtureMarkers(invalid.fixtureText);
assert.deepEqual(
  markerOrder,
  EXPECTED_INVALID_CASES.map(
    // compare exact fixture order independently of compiler diagnostic order
    ({ id }) => id,
  ),
);
assert.equal(invalid.diagnostics.length, EXPECTED_INVALID_CASES.length);
const expectedById = new Map(EXPECTED_INVALID_CASES.map(
  // bind each mismatch id to one exact diagnostic code
  ({ id, code }) => [id, code],
));
const observedById = new Map();
const sourceLines = invalid.sourceFile.text.split(/\r?\n/u);

// require every diagnostic to land on one exact marked fixture statement
for (const diagnostic of invalid.diagnostics) {
  assert.equal(diagnostic.category, ts.DiagnosticCategory.Error);
  assert.equal(diagnostic.file, invalid.sourceFile);
  assert.notEqual(diagnostic.start, undefined);
  const { line } = invalid.sourceFile.getLineAndCharacterOfPosition(
    diagnostic.start,
  );
  const marker = /\/\/ mismatch:([a-z0-9-]+)$/u.exec(sourceLines[line]);
  assert.ok(marker, `unmarked diagnostic TS${diagnostic.code} at line ${line + 1}`);
  const id = marker[1];
  assert.equal(diagnostic.code, expectedById.get(id), id);
  assert.equal(observedById.has(id), false, id);
  observedById.set(id, diagnostic.code);
}

// reject missing or additional diagnostics before reporting success
for (const { id, code } of EXPECTED_INVALID_CASES) {
  assert.equal(observedById.get(id), code, id);
}

process.stdout.write(`${JSON.stringify({
  typescriptVersion: ts.version,
  selectedDeclarations: selectedDeclarations.length,
  validPassed: true,
  invalidCases: EXPECTED_INVALID_CASES,
})}\n`);
