import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync, realpathSync, statSync } from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const EXPECTED = [
  { id: "invalid-air-quality", code: "E0308" },
  { id: "missing-channel", code: "E0063" },
  { id: "invalid-forecast-days", code: "E0599" },
  { id: "invalid-bootstrap-count", code: "E0308" },
  { id: "invalid-raw-brier", code: "E0308" },
  { id: "invalid-rain-threshold", code: "E0599" },
  { id: "invalid-control-bytes", code: "E0308" },
  { id: "invalid-precipitation-timestamp", code: "E0308" },
  { id: "invalid-json-member", code: "E0599" },
  { id: "invalid-install-receipt-version", code: "E0308" },
  { id: "missing-receipt-member-root", code: "E0063" },
];

// compile one external-metadata consumer without linking or running it
function checkFixture(rustc, generated, scratch, name) {
  const input = join(HERE, `${name}.rs`);
  const result = spawnSync(rustc, [
    "--edition=2024",
    "--crate-type=lib",
    "--crate-name", `contract_layout_${name}`,
    "--emit=metadata",
    "--error-format=json",
    "--extern", `weather_contracts_generated=${generated}`,
    "-o", join(scratch, `${name}.rmeta`),
    input,
  ], {
    encoding: "utf8",
    input: "",
    maxBuffer: 512 * 1024,
    timeout: 15_000,
  });
  assert.equal(result.error, undefined);
  assert.equal(result.signal, null);
  assert.equal(result.stdout, "");
  return result;
}

// match each actual primary error to its separately named fixture line
function rejectedCases(stderr) {
  const input = join(HERE, "invalid.rs");
  const lines = readFileSync(input, "utf8").split("\n");
  const byLine = new Map();

  // bind the expected fixture case order to the actual source markers
  for (const [index, line] of lines.entries()) {
    const match = /\/\/ mismatch:([a-z-]+)$/u.exec(line);
    // non-case lines have no diagnostic marker
    if (match !== null) {
      byLine.set(index + 1, match[1]);
    }
  }
  assert.deepEqual([...byLine.values()], EXPECTED.map(
    // preserve one independent rejecting declaration per case
    ({ id }) => id,
  ));

  const observed = new Map();
  let summaries = 0;

  // reject uncategorized errors rather than trusting a compiler exit code
  for (const line of stderr.trim().split("\n")) {
    const entry = JSON.parse(line);
    assert.equal(entry.$message_type, "diagnostic");
    // allow the ordinary rustc explanatory footer
    if (entry.level === "failure-note") {
      continue;
    }
    assert.equal(entry.level, "error");
    // require exactly the expected aggregate diagnostic footer
    if (entry.code === null) {
      assert.deepEqual(entry.spans, []);
      assert.equal(entry.message, `aborting due to ${EXPECTED.length} previous errors`);
      summaries += 1;
      continue;
    }
    const spans = entry.spans.filter(
      // locate the specific broken type assignment
      (span) => span.is_primary,
    );
    assert.equal(spans.length, 1);
    const span = spans[0];
    assert.equal(resolve(span.file_name), input);
    assert.ok(byLine.has(span.line_start));
    assert.ok(!observed.has(span.line_start));
    observed.set(span.line_start, entry.code.code);
  }

  assert.equal(summaries, 1);
  assert.equal(observed.size, EXPECTED.length);
  return [...byLine].map(
    // report each verified diagnostic only in the declared fixture order
    ([line, id]) => ({ id, code: observed.get(line) }),
  );
}

// use one explicitly supplied rustc binary and disposable metadata directory
function main(args) {
  assert.equal(args.length, 2, "expected absolute rustc path and scratch directory");
  const [rustc, scratch] = args;
  assert.ok(isAbsolute(rustc));
  assert.ok(isAbsolute(scratch));
  assert.equal(realpathSync(rustc), rustc);
  assert.equal(realpathSync(scratch), scratch);
  const generated = join(scratch, "libweather_contracts_generated.rmeta");
  assert.ok(statSync(generated).isFile());
  const valid = checkFixture(rustc, generated, scratch, "valid");
  assert.equal(valid.status, 0, valid.stderr);
  assert.equal(valid.stderr, "");
  const invalid = checkFixture(rustc, generated, scratch, "invalid");
  assert.equal(invalid.status, 1);
  const invalidCases = rejectedCases(invalid.stderr);
  assert.deepEqual(invalidCases, EXPECTED);
  process.stdout.write(`${JSON.stringify({ validPassed: true, invalidCases })}\n`);
}

main(process.argv.slice(2));
