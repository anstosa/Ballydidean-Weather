import assert from "node:assert/strict";
import test from "node:test";

import * as occurrenceModule from "./backend-clock-occurrences.mjs";

const {
  BACKEND_CLOCK_OCCURRENCES_VERSION,
  extractBackendClockOccurrences,
} = occurrenceModule;

const RESULT_KEYS = [
  "contractVersion",
  "source",
  "coverage",
  "occurrences",
  "diagnostics",
  "canonicalEligible",
  "referenceFlowProven",
  "physicalCoverageProven",
  "operationReachabilityProven",
  "readyForMetrics",
];
const OCCURRENCE_KEYS = [
  "id",
  "sourcePath",
  "language",
  "surface",
  "kind",
  "syntaxKind",
  "enclosingSymbol",
  "confidence",
  "aliasTrace",
  "normalizedExpressionSha256",
  "physicalSpan",
  "embeddedSql",
  "duplicateOrdinal",
  "countsAsClockCandidate",
];
const SPAN_KEYS = [
  "startOffsetUtf16",
  "endOffsetUtf16",
  "startLine",
  "startColumn",
  "endLine",
  "endColumn",
];
const DIAGNOSTIC_KEYS = [
  "code",
  "severity",
  "sourcePath",
  "physicalSpan",
  "relatedOccurrenceIds",
];
const EMBEDDED_SQL_KEYS = [
  "containerKind",
  "segmentKind",
  "containerPhysicalSpan",
  "payloadPhysicalSpan",
  "decodedRange",
  "decodedDomain",
  "hostMapping",
  "interpolationPresent",
];
const SURFACES = new Set([
  "ecmascript.date-now",
  "ecmascript.date-call",
  "ecmascript.date-construct-current",
  "ecmascript.date-construct-supplied",
  "ecmascript.date-parse",
  "ecmascript.date-utc",
  "ecmascript.wrapper-now",
  "ecmascript.wrapper-current-date",
  "ecmascript.wrapper-default-now",
  "node.performance-now",
  "node.performance-time-origin",
  "node.process-hrtime",
  "node.process-hrtime-bigint",
  "node.process-uptime",
  "node.os-uptime",
  "node.set-timeout",
  "node.set-interval",
  "node.set-immediate",
  "node.abort-signal-timeout",
  "node.process-cpu-usage",
  "node.process-resource-usage",
  "node.process-memory-usage",
  "node.process-memory-usage-rss",
  "postgres.current-date",
  "postgres.current-time",
  "postgres.current-timestamp",
  "postgres.localtime",
  "postgres.localtimestamp",
  "postgres.transaction-timestamp",
  "postgres.statement-timestamp",
  "postgres.clock-timestamp",
  "postgres.now",
  "postgres.timeofday",
  "postgres.timestamp-now-literal",
  "postgres.dollar-quoted-body",
]);
const KINDS = new Set([
  "current-time-read",
  "relative-clock-read",
  "timer-schedule",
  "resource-sample",
  "parse-only",
  "parse-time-current-value",
  "wrapper-candidate",
  "nested-language-candidate",
]);
const SYNTAX_KINDS = new Set([
  "call-expression",
  "new-expression",
  "property-read",
  "identifier-call",
  "property-call",
  "aliased-call",
  "sql-keyword",
  "sql-function-call",
  "sql-special-literal",
  "sql-dollar-quoted-body",
]);
const CONFIDENCES = new Set([
  "syntactic-direct",
  "one-hop-candidate",
  "name-only-candidate",
  "nested-language-unresolved",
]);
const DIAGNOSTIC_CODES = new Set([
  "source.parse-error",
  "source.size-limit",
  "javascript.intrinsic-shadowed",
  "javascript.clock-binding-reassigned",
  "javascript.wrapper-name-unresolved",
  "javascript.alias-second-hop-unresolved",
  "javascript.alias-property-unresolved",
  "javascript.computed-clock-member-unresolved",
  "javascript.dynamic-code-unresolved",
  "sql.context-ambiguous",
  "sql.lex-error",
  "sql.dynamic-fragment-unresolved",
  "sql.dollar-body-unresolved",
  "sql.dynamic-execution-unresolved",
  "sql.typed-current-context-unresolved",
]);

// extract one inline fixture
function extract(sourcePath, sourceText) {
  return extractBackendClockOccurrences({ sourcePath, sourceText });
}

// select occurrences by one exact surface
function occurrencesFor(result, surface) {
  return result.occurrences.filter(
    // match one closed surface
    (occurrence) => occurrence.surface === surface,
  );
}

// project diagnostic codes in output order
function diagnosticCodes(result) {
  return result.diagnostics.map(
    // retain one fixed code
    (diagnostic) => diagnostic.code,
  );
}

// require exact key order
function assertExactKeys(value, keys) {
  assert.deepEqual(Object.keys(value), keys);
}

// require recursive immutable values
function assertDeepFrozen(value, seen = new Set()) {
  // stop at primitives and repeated values
  if (value === null || typeof value !== "object" || seen.has(value)) {
    return;
  }
  seen.add(value);
  assert.equal(Object.isFrozen(value), true);
  // inspect every retained child
  for (const child of Object.values(value)) {
    assertDeepFrozen(child, seen);
  }
}

// require one full occurrence contract
function assertOccurrenceContract(occurrence) {
  assertExactKeys(occurrence, OCCURRENCE_KEYS);
  assert.match(occurrence.id, /^clock-occurrence-v1:[0-9a-f]{64}$/);
  assert.equal(SURFACES.has(occurrence.surface), true);
  assert.equal(KINDS.has(occurrence.kind), true);
  assert.equal(SYNTAX_KINDS.has(occurrence.syntaxKind), true);
  assert.equal(CONFIDENCES.has(occurrence.confidence), true);
  assert.match(occurrence.normalizedExpressionSha256, /^[0-9a-f]{64}$/);
  assertExactKeys(occurrence.physicalSpan, SPAN_KEYS);
  assert.equal(Number.isInteger(occurrence.duplicateOrdinal), true);
  assert.ok(occurrence.duplicateOrdinal > 0);
  // validate embedded coordinates when present
  if (occurrence.embeddedSql !== null) {
    assertExactKeys(occurrence.embeddedSql, EMBEDDED_SQL_KEYS);
    assertExactKeys(occurrence.embeddedSql.containerPhysicalSpan, SPAN_KEYS);
    assertExactKeys(occurrence.embeddedSql.payloadPhysicalSpan, SPAN_KEYS);
    assertExactKeys(occurrence.embeddedSql.decodedRange, [
      "startOffsetUtf16",
      "endOffsetUtf16",
    ]);
  }
}

test(
  "exports only the frozen occurrence API and validates its exact input boundary",
  // verify the closed public surface
  () => {
    assert.deepEqual(Object.keys(occurrenceModule), [
      "BACKEND_CLOCK_OCCURRENCES_VERSION",
      "extractBackendClockOccurrences",
    ]);
    assert.equal(
      BACKEND_CLOCK_OCCURRENCES_VERSION,
      "weather-backend-clock-occurrences/v1",
    );
    assert.equal(typeof extractBackendClockOccurrences, "function");
    assert.throws(
      // reject a missing input
      () => extractBackendClockOccurrences(),
      TypeError,
    );
    assert.throws(
      // reject an extra argument
      () => extractBackendClockOccurrences(
        { sourcePath: "fixture.ts", sourceText: "" },
        null,
      ),
      TypeError,
    );
    assert.throws(
      // reject an extra key
      () => extractBackendClockOccurrences({
        sourcePath: "fixture.ts",
        sourceText: "",
        language: "typescript",
      }),
      TypeError,
    );
    assert.throws(
      // reject a nonplain input
      () => extractBackendClockOccurrences([
        "fixture.ts",
        "Date.now()",
      ]),
      TypeError,
    );

    let getterRead = false;
    const accessorInput = {};
    Object.defineProperties(accessorInput, {
      sourcePath: {
        enumerable: true,
        // detect forbidden accessor evaluation
        get() {
          getterRead = true;
          return "fixture.ts";
        },
      },
      sourceText: {
        enumerable: true,
        value: "Date.now()",
      },
    });
    assert.throws(
      // reject accessors without invoking them
      () => extractBackendClockOccurrences(accessorInput),
      TypeError,
    );
    assert.equal(getterRead, false);
  },
);

test(
  "returns the exact noncanonical detached and recursively frozen envelope",
  // verify one complete result contract
  () => {
    const input = {
      sourcePath: "fixtures/simple.ts",
      sourceText: "Date.now()",
    };
    const result = extractBackendClockOccurrences(input);
    input.sourcePath = "fixtures/mutated.ts";
    input.sourceText = "secret marker";

    assertExactKeys(result, RESULT_KEYS);
    assert.deepEqual(Object.keys(result.source), [
      "path",
      "language",
      "byteLength",
      "sha256",
    ]);
    assert.deepEqual(Object.keys(result.coverage), [
      "sourceParseCompleted",
      "sqlLexingCompleted",
      "candidateExtractionCompleted",
    ]);
    assert.equal(result.contractVersion, BACKEND_CLOCK_OCCURRENCES_VERSION);
    assert.equal(result.source.path, "fixtures/simple.ts");
    assert.equal(result.source.language, "typescript");
    assert.equal(result.source.byteLength, 10);
    assert.match(result.source.sha256, /^[0-9a-f]{64}$/);
    assert.deepEqual(result.coverage, {
      sourceParseCompleted: true,
      sqlLexingCompleted: true,
      candidateExtractionCompleted: true,
    });
    assert.equal(result.canonicalEligible, false);
    assert.equal(result.referenceFlowProven, false);
    assert.equal(result.physicalCoverageProven, false);
    assert.equal(result.operationReachabilityProven, false);
    assert.equal(result.readyForMetrics, false);
    assert.equal(result.occurrences.length, 1);
    assertOccurrenceContract(result.occurrences[0]);
    assertDeepFrozen(result);
    assert.throws(
      // reject result mutation
      () => result.occurrences.push(null),
      TypeError,
    );
  },
);

test(
  "classifies every Date form and still visits nested clock arguments",
  // verify current and parse-only distinctions
  () => {
    const sourceText = [
      "Date.now();",
      "Date();",
      "Date(1);",
      "new Date();",
      "new Date(1, 2);",
      "Date.parse('2026-01-01');",
      "Date.UTC(2026, 0, 1);",
      "new Date(Date.now());",
    ].join("\n");
    const result = extract("fixtures/date-forms.ts", sourceText);

    assert.deepEqual(
      result.occurrences.map(
        // retain one surface and kind pair
        ({ surface, kind, countsAsClockCandidate }) => ({
          surface,
          kind,
          countsAsClockCandidate,
        }),
      ),
      [
        { surface: "ecmascript.date-now", kind: "current-time-read", countsAsClockCandidate: true },
        { surface: "ecmascript.date-call", kind: "current-time-read", countsAsClockCandidate: true },
        { surface: "ecmascript.date-call", kind: "current-time-read", countsAsClockCandidate: true },
        { surface: "ecmascript.date-construct-current", kind: "current-time-read", countsAsClockCandidate: true },
        { surface: "ecmascript.date-construct-supplied", kind: "parse-only", countsAsClockCandidate: false },
        { surface: "ecmascript.date-parse", kind: "parse-only", countsAsClockCandidate: false },
        { surface: "ecmascript.date-utc", kind: "parse-only", countsAsClockCandidate: false },
        { surface: "ecmascript.date-construct-supplied", kind: "parse-only", countsAsClockCandidate: false },
        { surface: "ecmascript.date-now", kind: "current-time-read", countsAsClockCandidate: true },
      ],
    );
    // prove each javascript span covers its complete provider expression
    for (const occurrence of result.occurrences) {
      const text = sourceText.slice(
        occurrence.physicalSpan.startOffsetUtf16,
        occurrence.physicalSpan.endOffsetUtf16,
      );
      assert.match(text, /^(?:Date|new Date)/);
      assert.equal(text.endsWith(")"), true);
    }
  },
);

test(
  "ignores clock-looking javascript text and visits every sibling provider",
  // verify syntax-only traversal without sibling short-circuiting
  () => {
    const inert = extract(
      "fixtures/inert.ts",
      [
        "// Date.now()",
        "const DateNow = 1;",
        "const value = 'new Date()';",
        "const object = { now: 1, currentDate: 2 };",
      ].join("\n"),
    );
    assert.deepEqual(inert.occurrences, []);

    const siblings = extract(
      "fixtures/siblings.ts",
      "consume(Date.now(), performance.now(), setTimeout(task, 1), process.cpuUsage());",
    );
    assert.deepEqual(
      siblings.occurrences.map(
        // retain each sibling surface
        (occurrence) => occurrence.surface,
      ),
      [
        "ecmascript.date-now",
        "node.performance-now",
        "node.set-timeout",
        "node.process-cpu-usage",
      ],
    );
  },
);

test(
  "tracks lexical shadowing and writes across their complete scopes",
  // verify conservative intrinsic confidence
  () => {
    const shadowed = extract(
      "fixtures/shadowed.ts",
      [
        "function parameter(Date) { Date.now(); }",
        "function hoisted() { Date.now(); var Date; }",
        "{ Date.now(); let Date; }",
      ].join("\n"),
    );
    assert.equal(occurrencesFor(shadowed, "ecmascript.date-now").length, 3);
    assert.deepEqual(
      occurrencesFor(shadowed, "ecmascript.date-now").map(
        // retain one downgraded confidence
        (occurrence) => occurrence.confidence,
      ),
      ["name-only-candidate", "name-only-candidate", "name-only-candidate"],
    );
    assert.equal(
      diagnosticCodes(shadowed).filter(
        // count intrinsic-shadow diagnostics
        (code) => code === "javascript.intrinsic-shadowed",
      ).length,
      3,
    );

    const reassigned = extract(
      "fixtures/reassigned.ts",
      "Date.now(); Date = replacement; Date.now(); process.hrtime(); process.hrtime = replacement;",
    );
    assert.deepEqual(
      reassigned.occurrences.map(
        // retain each reassigned confidence
        (occurrence) => occurrence.confidence,
      ),
      ["name-only-candidate", "name-only-candidate", "name-only-candidate"],
    );
    assert.equal(
      diagnosticCodes(reassigned).includes("javascript.clock-binding-reassigned"),
      true,
    );
  },
);

test(
  "review repair: keeps loop catch and named expression bindings in their exact scopes",
  // verify scoped bindings and aliases never leak
  () => {
    const sourceText = [
      "for (let Date = replacement; condition; step()) { Date.now(); }",
      "Date.now();",
      "for (const now = Date.now; condition; step()) { now(); }",
      "now();",
      "for (const Date in values) { Date.now(); }",
      "Date.now();",
      "for (let Date of values) { Date.now(); }",
      "Date.now();",
      "try { throw replacement; } catch (Date) { Date.now(); }",
      "Date.now();",
      "const namedFunction = function Date() { return Date.now(); };",
      "Date.now();",
      "const namedClass = class Date { method() { return Date.now(); } };",
      "Date.now();",
    ].join("\n");
    const result = extract("fixtures/scoped-bindings.ts", sourceText);
    const dateOccurrences = occurrencesFor(result, "ecmascript.date-now");

    assert.equal(dateOccurrences.length, 13);
    assert.equal(
      dateOccurrences.filter(
        // retain only local shadow candidates
        (occurrence) => occurrence.confidence === "name-only-candidate",
      ).length,
      6,
    );
    assert.equal(
      dateOccurrences.filter(
        // retain only direct outer clocks
        (occurrence) => occurrence.confidence === "syntactic-direct",
      ).length,
      6,
    );
    assert.equal(
      dateOccurrences.filter(
        // retain the loop-local immutable alias
        (occurrence) => occurrence.confidence === "one-hop-candidate",
      ).length,
      1,
    );
    const outsideNow = occurrencesFor(result, "ecmascript.wrapper-now");
    assert.equal(outsideNow.length, 1);
    assert.equal(outsideNow[0].confidence, "name-only-candidate");
    assert.equal(
      diagnosticCodes(result).filter(
        // count only truly shadowed Date expressions
        (code) => code === "javascript.intrinsic-shadowed",
      ).length,
      6,
    );
  },
);

test(
  "review repair: computed and imported performance writes downgrade only affected clocks",
  // verify exact root and member write effects
  () => {
    const computedDate = extract(
      "fixtures/computed-date-write.ts",
      "Date[\"now\"] = replacement; Date.now();",
    );
    const dateOccurrence = occurrencesFor(computedDate, "ecmascript.date-now")[0];
    assert.equal(dateOccurrence.confidence, "name-only-candidate");
    assert.equal(
      diagnosticCodes(computedDate).includes("javascript.clock-binding-reassigned"),
      true,
    );

    const performanceSource = [
      "import { performance as perf } from 'node:perf_hooks';",
      "function rootWrite() {",
      "  perf.now(); perf.timeOrigin; perf = replacement; perf.now(); perf.timeOrigin;",
      "}",
      "function memberWrite() {",
      "  perf.now(); perf.timeOrigin; perf.now = replacement; perf.now(); perf.timeOrigin;",
      "}",
      "function originWrite() {",
      "  perf.now(); perf.timeOrigin; perf.timeOrigin++; perf.now(); perf.timeOrigin;",
      "}",
    ].join("\n");
    const performanceResult = extract(
      "fixtures/imported-performance.mts",
      performanceSource,
    );
    const rootOccurrences = performanceResult.occurrences.filter(
      // retain root-write function clocks
      (occurrence) => occurrence.enclosingSymbol === "rootWrite",
    );
    const memberNowOccurrences = occurrencesFor(
      performanceResult,
      "node.performance-now",
    ).filter(
      // retain member-write calls
      (occurrence) => occurrence.enclosingSymbol === "memberWrite",
    );
    const memberOriginOccurrences = occurrencesFor(
      performanceResult,
      "node.performance-time-origin",
    ).filter(
      // retain unaffected member-write properties
      (occurrence) => occurrence.enclosingSymbol === "memberWrite",
    );
    const originNowOccurrences = occurrencesFor(
      performanceResult,
      "node.performance-now",
    ).filter(
      // retain unaffected origin-write calls
      (occurrence) => occurrence.enclosingSymbol === "originWrite",
    );
    const originPropertyOccurrences = occurrencesFor(
      performanceResult,
      "node.performance-time-origin",
    ).filter(
      // retain affected origin-write properties
      (occurrence) => occurrence.enclosingSymbol === "originWrite",
    );

    assert.equal(rootOccurrences.length, 4);
    assert.deepEqual(
      rootOccurrences.map(
        // retain each root-write confidence
        (occurrence) => occurrence.confidence,
      ),
      [
        "name-only-candidate",
        "name-only-candidate",
        "name-only-candidate",
        "name-only-candidate",
      ],
    );
    assert.deepEqual(
      memberNowOccurrences.map(
        // retain affected member-call confidence
        (occurrence) => occurrence.confidence,
      ),
      ["name-only-candidate", "name-only-candidate"],
    );
    assert.deepEqual(
      memberOriginOccurrences.map(
        // retain unaffected sibling property confidence
        (occurrence) => occurrence.confidence,
      ),
      ["one-hop-candidate", "one-hop-candidate"],
    );
    assert.deepEqual(
      originNowOccurrences.map(
        // retain unaffected sibling call confidence
        (occurrence) => occurrence.confidence,
      ),
      ["one-hop-candidate", "one-hop-candidate"],
    );
    assert.deepEqual(
      originPropertyOccurrences.map(
        // retain affected property confidence
        (occurrence) => occurrence.confidence,
      ),
      [
        "name-only-candidate",
        "name-only-candidate",
        "name-only-candidate",
      ],
    );
    const affectedOccurrences = [
      ...rootOccurrences,
      ...memberNowOccurrences,
      ...originPropertyOccurrences,
    ];
    // require a reassignment diagnostic for every affected provider
    for (const occurrence of affectedOccurrences) {
      assert.equal(
        performanceResult.diagnostics.some(
          // match the affected provider span
          (diagnostic) => diagnostic.code === "javascript.clock-binding-reassigned"
            && diagnostic.physicalSpan?.startOffsetUtf16
              === occurrence.physicalSpan.startOffsetUtf16
            && diagnostic.physicalSpan?.endOffsetUtf16
              === occurrence.physicalSpan.endOffsetUtf16,
        ),
        true,
      );
    }
    // reject reassignment overstatement on unaffected timeOrigin reads
    for (const occurrence of [...memberOriginOccurrences, ...originNowOccurrences]) {
      assert.equal(
        performanceResult.diagnostics.some(
          // match one forbidden diagnostic span
          (diagnostic) => diagnostic.code === "javascript.clock-binding-reassigned"
            && diagnostic.physicalSpan?.startOffsetUtf16
              === occurrence.physicalSpan.startOffsetUtf16
            && diagnostic.physicalSpan?.endOffsetUtf16
              === occurrence.physicalSpan.endOffsetUtf16,
        ),
        false,
      );
    }
  },
);

test(
  "review repair two: preserves exact computed member path segments without dot collisions",
  // verify segmented write identity and unrelated paths
  () => {
    const sourceText = [
      "function mixedExact() {",
      "  process[\"hrtime\"].bigint = replacement;",
      "  process.hrtime.bigint(); process.hrtime(); process.uptime();",
      "}",
      "function mixedExactReverse() {",
      "  process.hrtime[\"bigint\"] = replacement;",
      "  process.hrtime.bigint();",
      "}",
      "function literalDotCollision() {",
      "  process[\"hrtime.bigint\"] = replacement;",
      "  process.hrtime.bigint();",
      "}",
      "function unrelatedMember() {",
      "  process[\"hrtime\"].other = replacement;",
      "  process.hrtime.bigint(); process.hrtime();",
      "}",
    ].join("\n");
    const result = extract("fixtures/exact-member-paths.ts", sourceText);
    const bigintOccurrences = occurrencesFor(
      result,
      "node.process-hrtime-bigint",
    );

    assert.deepEqual(
      bigintOccurrences.map(
        // retain owner and exact write effect
        (occurrence) => ({
          enclosingSymbol: occurrence.enclosingSymbol,
          confidence: occurrence.confidence,
        }),
      ),
      [
        { enclosingSymbol: "mixedExact", confidence: "name-only-candidate" },
        { enclosingSymbol: "mixedExactReverse", confidence: "name-only-candidate" },
        { enclosingSymbol: "literalDotCollision", confidence: "syntactic-direct" },
        { enclosingSymbol: "unrelatedMember", confidence: "syntactic-direct" },
      ],
    );
    assert.deepEqual(
      occurrencesFor(result, "node.process-hrtime").map(
        // retain unaffected parent-member confidence
        (occurrence) => occurrence.confidence,
      ),
      ["syntactic-direct", "syntactic-direct"],
    );
    assert.deepEqual(
      occurrencesFor(result, "node.process-uptime").map(
        // retain unaffected sibling confidence
        (occurrence) => occurrence.confidence,
      ),
      ["syntactic-direct"],
    );
    const affectedOccurrences = bigintOccurrences.slice(0, 2);
    // require diagnostics on both exact computed writes
    for (const occurrence of affectedOccurrences) {
      assert.equal(
        result.diagnostics.some(
          // match one affected provider span
          (diagnostic) => diagnostic.code === "javascript.clock-binding-reassigned"
            && diagnostic.physicalSpan?.startOffsetUtf16
              === occurrence.physicalSpan.startOffsetUtf16
            && diagnostic.physicalSpan?.endOffsetUtf16
              === occurrence.physicalSpan.endOffsetUtf16,
        ),
        true,
      );
    }
    const unaffectedOccurrences = bigintOccurrences.slice(2);
    // reject diagnostics on literal-dot and unrelated paths
    for (const occurrence of unaffectedOccurrences) {
      assert.equal(
        result.diagnostics.some(
          // match one forbidden provider span
          (diagnostic) => diagnostic.code === "javascript.clock-binding-reassigned"
            && diagnostic.physicalSpan?.startOffsetUtf16
              === occurrence.physicalSpan.startOffsetUtf16
            && diagnostic.physicalSpan?.endOffsetUtf16
              === occurrence.physicalSpan.endOffsetUtf16,
        ),
        false,
      );
    }
  },
);

test(
  "review repair three: ancestor writes affect descendant providers and alias admission only",
  // verify prefix writes without child or sibling overstatement
  () => {
    const sourceText = [
      "function dotAncestor() {",
      "  process.hrtime = replacement; process.hrtime.bigint();",
      "}",
      "function elementAncestor() {",
      "  process[\"memoryUsage\"] = replacement; process.memoryUsage.rss();",
      "}",
      "function aliasAncestor() {",
      "  const now = process.hrtime.bigint; process.hrtime = replacement; now();",
      "}",
      "function unrelatedSibling() {",
      "  process.hrtime.other = replacement; process.hrtime.bigint();",
      "}",
      "function childWrite() {",
      "  process.hrtime.bigint.extra = replacement; process.hrtime.bigint(); process.hrtime();",
      "}",
      "function exactChildWrite() {",
      "  process.hrtime.bigint = replacement; process.hrtime();",
      "}",
    ].join("\n");
    const result = extract("fixtures/ancestor-write-paths.ts", sourceText);
    assert.deepEqual(
      occurrencesFor(result, "node.process-hrtime-bigint").map(
        // retain exact descendant write effects
        (occurrence) => ({
          enclosingSymbol: occurrence.enclosingSymbol,
          confidence: occurrence.confidence,
        }),
      ),
      [
        { enclosingSymbol: "dotAncestor", confidence: "name-only-candidate" },
        { enclosingSymbol: "unrelatedSibling", confidence: "syntactic-direct" },
        { enclosingSymbol: "childWrite", confidence: "syntactic-direct" },
      ],
    );
    assert.deepEqual(
      occurrencesFor(result, "node.process-memory-usage-rss").map(
        // retain the computed ancestor effect
        (occurrence) => ({
          enclosingSymbol: occurrence.enclosingSymbol,
          confidence: occurrence.confidence,
        }),
      ),
      [{ enclosingSymbol: "elementAncestor", confidence: "name-only-candidate" }],
    );
    assert.deepEqual(
      occurrencesFor(result, "node.process-hrtime").map(
        // retain unaffected parent providers
        (occurrence) => ({
          enclosingSymbol: occurrence.enclosingSymbol,
          confidence: occurrence.confidence,
        }),
      ),
      [
        { enclosingSymbol: "childWrite", confidence: "syntactic-direct" },
        { enclosingSymbol: "exactChildWrite", confidence: "syntactic-direct" },
      ],
    );
    assert.deepEqual(
      occurrencesFor(result, "ecmascript.wrapper-now").map(
        // retain the rejected callable alias fallback
        (occurrence) => ({
          enclosingSymbol: occurrence.enclosingSymbol,
          confidence: occurrence.confidence,
        }),
      ),
      [{ enclosingSymbol: "aliasAncestor", confidence: "name-only-candidate" }],
    );
    assert.equal(
      diagnosticCodes(result).filter(
        // count two providers and one rejected alias
        (code) => code === "javascript.clock-binding-reassigned",
      ).length,
      3,
    );
  },
);

test(
  "review repair three: transparent assignment wrappers preserve exact provider paths",
  // verify only approved wrappers reveal assignment targets
  () => {
    const sourceText = [
      "function parenthesized() {",
      "  (process.hrtime).bigint = replacement; process.hrtime.bigint();",
      "}",
      "function asAssertion() {",
      "  (process as typeof process).hrtime.bigint = replacement; process.hrtime.bigint();",
      "}",
      "function nonNullAssertion() {",
      "  process!.hrtime.bigint = replacement; process.hrtime.bigint();",
      "}",
      "function satisfiesExpression() {",
      "  (process satisfies object).hrtime.bigint = replacement; process.hrtime.bigint();",
      "}",
      "function angleAssertion() {",
      "  (<any>process).hrtime.bigint = replacement; process.hrtime.bigint();",
      "}",
      "function commaControl() {",
      "  (other, process).hrtime.bigint = replacement; process.hrtime.bigint();",
      "}",
      "function callControl() {",
      "  getProcess().hrtime.bigint = replacement; process.hrtime.bigint();",
      "}",
      "function dynamicControl() {",
      "  process[key].hrtime.bigint = replacement; process.hrtime.bigint();",
      "}",
      "function unrelatedControl() {",
      "  (process as typeof process).hrtime.other = replacement; process.hrtime.bigint();",
      "}",
    ].join("\n");
    const result = extract("fixtures/transparent-write-paths.ts", sourceText);
    const occurrences = occurrencesFor(result, "node.process-hrtime-bigint");

    assert.deepEqual(
      occurrences.map(
        // retain wrapper and control effects
        (occurrence) => ({
          enclosingSymbol: occurrence.enclosingSymbol,
          confidence: occurrence.confidence,
        }),
      ),
      [
        { enclosingSymbol: "parenthesized", confidence: "name-only-candidate" },
        { enclosingSymbol: "asAssertion", confidence: "name-only-candidate" },
        { enclosingSymbol: "nonNullAssertion", confidence: "name-only-candidate" },
        { enclosingSymbol: "satisfiesExpression", confidence: "name-only-candidate" },
        { enclosingSymbol: "angleAssertion", confidence: "name-only-candidate" },
        { enclosingSymbol: "commaControl", confidence: "syntactic-direct" },
        { enclosingSymbol: "callControl", confidence: "syntactic-direct" },
        { enclosingSymbol: "dynamicControl", confidence: "syntactic-direct" },
        { enclosingSymbol: "unrelatedControl", confidence: "syntactic-direct" },
      ],
    );
    const affectedOccurrences = occurrences.slice(0, 5);
    // require reassignment gaps on every transparent exact target
    for (const occurrence of affectedOccurrences) {
      assert.equal(
        result.diagnostics.some(
          // match one affected provider span
          (diagnostic) => diagnostic.code === "javascript.clock-binding-reassigned"
            && diagnostic.physicalSpan?.startOffsetUtf16
              === occurrence.physicalSpan.startOffsetUtf16
            && diagnostic.physicalSpan?.endOffsetUtf16
              === occurrence.physicalSpan.endOffsetUtf16,
        ),
        true,
      );
    }
    const controlOccurrences = occurrences.slice(5);
    // reject reassignment gaps on conservative controls
    for (const occurrence of controlOccurrences) {
      assert.equal(
        result.diagnostics.some(
          // match one forbidden provider span
          (diagnostic) => diagnostic.code === "javascript.clock-binding-reassigned"
            && diagnostic.physicalSpan?.startOffsetUtf16
              === occurrence.physicalSpan.startOffsetUtf16
            && diagnostic.physicalSpan?.endOffsetUtf16
              === occurrence.physicalSpan.endOffsetUtf16,
        ),
        false,
      );
    }
  },
);

test(
  "walks object and array assignment targets without losing conservative clock writes",
  // verify both destructuring assignment node families
  () => {
    const sourceText = [
      "function objectTargets(value, fallback) {",
      "  Date.now();",
      "  ({ Date = fallback, renamed: currentDate, nested: { defaultNow = fallback }, ...objectRest } = value);",
      "  Date.now(); currentDate(); defaultNow(); performance.now();",
      "}",
      "function arrayTargets(value, fallback) {",
      "  const now = Date.now;",
      "  now();",
      "  [now = fallback, [currentDate], ...arrayRest] = value;",
      "  now(); currentDate(); performance.now();",
      "}",
    ].join("\n");
    let result;
    assert.doesNotThrow(
      // extract assignment targets without an ast-shape crash
      () => {
        result = extract("fixtures/assignment-targets.ts", sourceText);
      },
    );
    assert.notEqual(result, undefined);

    const dateCandidates = occurrencesFor(result, "ecmascript.date-now");
    assert.equal(dateCandidates.length, 4);
    assert.deepEqual(
      dateCandidates.map(
        // retain each conservatively downgraded clock
        (occurrence) => occurrence.confidence,
      ),
      [
        "name-only-candidate",
        "name-only-candidate",
        "name-only-candidate",
        "name-only-candidate",
      ],
    );
    assert.equal(
      diagnosticCodes(result).filter(
        // count affected binding writes
        (code) => code === "javascript.clock-binding-reassigned",
      ).length,
      4,
    );
    assert.deepEqual(
      occurrencesFor(result, "ecmascript.wrapper-current-date").map(
        // retain property and nested array targets
        (occurrence) => occurrence.confidence,
      ),
      ["name-only-candidate", "name-only-candidate"],
    );
    assert.deepEqual(
      occurrencesFor(result, "ecmascript.wrapper-default-now").map(
        // retain one nested object default target
        (occurrence) => occurrence.confidence,
      ),
      ["name-only-candidate"],
    );
    assert.deepEqual(
      occurrencesFor(result, "node.performance-now").map(
        // retain unaffected sibling clocks
        (occurrence) => occurrence.confidence,
      ),
      ["syntactic-direct", "syntactic-direct"],
    );
  },
);

test(
  "admits only the frozen one-hop alias grammar",
  // verify immutable identifier destructuring and import aliases
  () => {
    const admitted = extract(
      "fixtures/aliases.mts",
      [
        "import { setTimeout as delay } from 'node:timers';",
        "import { hrtime as high } from 'node:process';",
        "import { uptime as osUp } from 'node:os';",
        "import { performance as perf } from 'node:perf_hooks';",
        "const dateNow = Date.now;",
        "const { now: destructuredNow } = Date;",
        "dateNow(); destructuredNow(); delay(task, 1); high(); osUp(); perf.now(); perf.timeOrigin;",
      ].join("\n"),
    );
    assert.deepEqual(
      admitted.occurrences.map(
        // retain admitted target surfaces
        (occurrence) => occurrence.surface,
      ),
      [
        "ecmascript.date-now",
        "ecmascript.date-now",
        "node.set-timeout",
        "node.process-hrtime",
        "node.os-uptime",
        "node.performance-now",
        "node.performance-time-origin",
      ],
    );
    // prove every admitted alias is explicitly traced
    for (const occurrence of admitted.occurrences) {
      assert.equal(occurrence.confidence, "one-hop-candidate");
      assert.ok(occurrence.aliasTrace.length > 0);
      // validate every alias trace object
      for (const step of occurrence.aliasTrace) {
        assertExactKeys(step, ["step", "binding", "targetSurface"]);
        assert.equal(SURFACES.has(step.targetSurface), true);
      }
    }

    const rejected = extract(
      "fixtures/rejected-aliases.ts",
      [
        "import * as timers from 'node:timers';",
        "const direct = Date.now;",
        "const second = direct;",
        "const arbitrary = service.now;",
        "const computed = service['now'];",
        "const { now: defaulted = fallback } = Date;",
        "const { now: rested, ...rest } = Date;",
        "let mutable = Date.now;",
        "second(); arbitrary(); computed(); defaulted(); rested(); mutable(); timers.setTimeout(task, 1);",
      ].join("\n"),
    );
    const codes = new Set(diagnosticCodes(rejected));
    assert.equal(codes.has("javascript.alias-second-hop-unresolved"), true);
    assert.equal(codes.has("javascript.alias-property-unresolved"), true);
    assert.equal(codes.has("javascript.computed-clock-member-unresolved"), true);
    assert.equal(
      rejected.occurrences.some(
        // forbid a false direct classification
        (occurrence) => occurrence.confidence === "syntactic-direct",
      ),
      false,
    );
  },
);

test(
  "review repair four: destructured aliases honor selected member writes independently",
  // verify selected roots siblings and order-insensitive writes
  () => {
    const sourceText = [
      "function processDotBefore() {",
      "  process.hrtime = replacement; const { hrtime: clock } = process; clock();",
      "}",
      "function processComputedAfter() {",
      "  const { hrtime: clock } = process; process[\"hrtime\"] = replacement; clock();",
      "}",
      "function dateDotBefore() {",
      "  Date.now = replacement; const { now: clock } = Date; clock();",
      "}",
      "function dateComputedAfter() {",
      "  const { now: clock } = Date; Date[\"now\"] = replacement; clock();",
      "}",
      "function processSibling() {",
      "  process.hrtime = replacement; const { hrtime: h, uptime: u } = process; h(); u();",
      "}",
      "function dateSibling() {",
      "  Date.now = replacement; const { now: n, parse: p } = Date; n(); p('2026');",
      "}",
      "function rootWrite() {",
      "  process = replacement; const { hrtime: h, uptime: u } = process; h(); u();",
      "}",
      "function unrelatedMember() {",
      "  process.cpuUsage = replacement; const { hrtime: h, uptime: u } = process; h(); u();",
      "}",
    ].join("\n");
    const result = extract("fixtures/destructured-member-writes.ts", sourceText);

    assert.deepEqual(
      result.occurrences.map(
        // retain only aliases that remain admissible
        (occurrence) => ({
          enclosingSymbol: occurrence.enclosingSymbol,
          surface: occurrence.surface,
          confidence: occurrence.confidence,
        }),
      ),
      [
        {
          enclosingSymbol: "processSibling",
          surface: "node.process-uptime",
          confidence: "one-hop-candidate",
        },
        {
          enclosingSymbol: "dateSibling",
          surface: "ecmascript.date-parse",
          confidence: "one-hop-candidate",
        },
        {
          enclosingSymbol: "unrelatedMember",
          surface: "node.process-hrtime",
          confidence: "one-hop-candidate",
        },
        {
          enclosingSymbol: "unrelatedMember",
          surface: "node.process-uptime",
          confidence: "one-hop-candidate",
        },
      ],
    );
    const reassignmentDiagnostics = result.diagnostics.filter(
      // retain only affected alias bindings
      (diagnostic) => diagnostic.code === "javascript.clock-binding-reassigned",
    );
    assert.equal(reassignmentDiagnostics.length, 7);
    // require every reassignment gap to identify a destructuring binding
    for (const diagnostic of reassignmentDiagnostics) {
      assert.match(
        sourceText.slice(
          diagnostic.physicalSpan.startOffsetUtf16,
          diagnostic.physicalSpan.endOffsetUtf16,
        ),
        /^\{[^}]+\}\s*=\s*(?:process|Date)$/u,
      );
    }
  },
);

test(
  "separates relative clocks timers resources and unresolved wrappers",
  // verify every nonwall javascript family
  () => {
    const result = extract(
      "fixtures/nonwall.ts",
      [
        "performance.now(); performance.timeOrigin;",
        "process.hrtime(); process.hrtime.bigint(); process.uptime(); os.uptime();",
        "setTimeout(task, 1); setInterval(task, 1); setImmediate(task); AbortSignal.timeout(1);",
        "process.cpuUsage(); process.resourceUsage(); process.memoryUsage(); process.memoryUsage.rss();",
        "now(); currentDate(); defaultNow();",
      ].join("\n"),
    );
    assert.deepEqual(
      result.occurrences.map(
        // retain exact surface and kind
        ({ surface, kind }) => ({ surface, kind }),
      ),
      [
        { surface: "node.performance-now", kind: "relative-clock-read" },
        { surface: "node.performance-time-origin", kind: "relative-clock-read" },
        { surface: "node.process-hrtime", kind: "relative-clock-read" },
        { surface: "node.process-hrtime-bigint", kind: "relative-clock-read" },
        { surface: "node.process-uptime", kind: "relative-clock-read" },
        { surface: "node.os-uptime", kind: "relative-clock-read" },
        { surface: "node.set-timeout", kind: "timer-schedule" },
        { surface: "node.set-interval", kind: "timer-schedule" },
        { surface: "node.set-immediate", kind: "timer-schedule" },
        { surface: "node.abort-signal-timeout", kind: "timer-schedule" },
        { surface: "node.process-cpu-usage", kind: "resource-sample" },
        { surface: "node.process-resource-usage", kind: "resource-sample" },
        { surface: "node.process-memory-usage", kind: "resource-sample" },
        { surface: "node.process-memory-usage-rss", kind: "resource-sample" },
        { surface: "ecmascript.wrapper-now", kind: "wrapper-candidate" },
        { surface: "ecmascript.wrapper-current-date", kind: "wrapper-candidate" },
        { surface: "ecmascript.wrapper-default-now", kind: "wrapper-candidate" },
      ],
    );
    assert.equal(
      diagnosticCodes(result).filter(
        // count unresolved wrapper names
        (code) => code === "javascript.wrapper-name-unresolved",
      ).length,
      3,
    );
  },
);

test(
  "derives all supported script kinds and rejects unsafe paths and extensions",
  // verify the closed language map
  () => {
    const cases = [
      ["fixture.js", "javascript", "Date.now()"],
      ["fixture.mjs", "javascript", "Date.now()"],
      ["fixture.cjs", "javascript", "Date.now()"],
      ["fixture.jsx", "javascript", "const node = <div />; Date.now();"],
      ["fixture.ts", "typescript", "const value: number = Date.now();"],
      ["fixture.mts", "typescript", "const value: number = Date.now();"],
      ["fixture.cts", "typescript", "const value: number = Date.now();"],
      ["fixture.tsx", "typescript", "const node = <div />; Date.now();"],
      ["fixture.sql", "sql", "SELECT now();"],
    ];
    // validate every supported extension
    for (const [sourcePath, language, sourceText] of cases) {
      const result = extract(sourcePath, sourceText);
      assert.equal(result.source.language, language);
      assert.equal(result.coverage.sourceParseCompleted, true);
    }

    const invalidPaths = [
      "",
      "/absolute.ts",
      "dir\\file.ts",
      "dir//file.ts",
      "dir/./file.ts",
      "dir/../file.ts",
      "file.txt",
      "nul\0file.ts",
    ];
    // reject every unsafe path form
    for (const sourcePath of invalidPaths) {
      assert.throws(
        // invoke one invalid path
        () => extract(sourcePath, "Date.now()"),
        TypeError,
      );
    }
  },
);

test(
  "fails closed on malformed source and the one-mebibyte byte ceiling",
  // verify incomplete coverage combinations
  () => {
    const malformed = extract(
      "fixtures/malformed.ts",
      "function broken( { Date.now();",
    );
    assert.deepEqual(malformed.coverage, {
      sourceParseCompleted: false,
      sqlLexingCompleted: false,
      candidateExtractionCompleted: false,
    });
    assert.deepEqual(malformed.occurrences, []);
    assert.deepEqual(diagnosticCodes(malformed), ["source.parse-error"]);
    assertExactKeys(malformed.diagnostics[0], DIAGNOSTIC_KEYS);
    assert.equal(malformed.diagnostics[0].severity, "error");

    const oversized = extract(
      "fixtures/oversized.ts",
      "a".repeat(1_048_577),
    );
    assert.deepEqual(oversized.coverage, {
      sourceParseCompleted: false,
      sqlLexingCompleted: false,
      candidateExtractionCompleted: false,
    });
    assert.deepEqual(oversized.occurrences, []);
    assert.deepEqual(diagnosticCodes(oversized), ["source.size-limit"]);
    assert.equal(oversized.diagnostics[0].severity, "error");
  },
);

test(
  "lexes the complete frozen PostgreSQL surface set without quoted-data false positives",
  // verify raw sql lexical policy
  () => {
    const sourceText = [
      "SELECT CURRENT_DATE, current_time(3), CURRENT_TIMESTAMP(6), LOCALTIME, LOCALTIMESTAMP(2);",
      "SELECT transaction_timestamp(), statement_timestamp(), clock_timestamp(), now(), timeofday();",
      "SELECT pg_catalog.\"now\"(), \"pg_catalog\".\"clock_timestamp\"();",
      "SELECT TIMESTAMP 'now';",
      "SELECT 'now()', 'CURRENT_TIMESTAMP', \"CURRENT_TIMESTAMP\", \"NOW\"();",
      "-- now() CURRENT_DATE",
      "/* outer now() /* nested clock_timestamp() */ statement_timestamp() */",
    ].join("\n");
    const result = extract("fixtures/providers.sql", sourceText);
    assert.deepEqual(
      result.occurrences.map(
        // retain raw sql surfaces
        (occurrence) => occurrence.surface,
      ),
      [
        "postgres.current-date",
        "postgres.current-time",
        "postgres.current-timestamp",
        "postgres.localtime",
        "postgres.localtimestamp",
        "postgres.transaction-timestamp",
        "postgres.statement-timestamp",
        "postgres.clock-timestamp",
        "postgres.now",
        "postgres.timeofday",
        "postgres.now",
        "postgres.clock-timestamp",
        "postgres.timestamp-now-literal",
      ],
    );
    assert.deepEqual(result.diagnostics, []);
    assert.deepEqual(result.coverage, {
      sourceParseCompleted: true,
      sqlLexingCompleted: true,
      candidateExtractionCompleted: true,
    });
    assert.equal(
      occurrencesFor(result, "postgres.timestamp-now-literal")[0].kind,
      "parse-time-current-value",
    );
    // prove every raw sql span is half-open and nonempty
    for (const occurrence of result.occurrences) {
      assert.ok(occurrence.physicalSpan.startOffsetUtf16 < occurrence.physicalSpan.endOffsetUtf16);
      assert.equal(occurrence.embeddedSql, null);
    }
  },
);

test(
  "review repair: keeps escaped PostgreSQL E strings complete and inert",
  // verify backslash-escaped apostrophes remain quoted data
  () => {
    const sourceText = String.raw`SELECT E'escaped \' now() \' remains data', e'more \' clock_timestamp() \' data';`;
    const result = extract("fixtures/escaped-e-strings.sql", sourceText);
    assert.deepEqual(result.coverage, {
      sourceParseCompleted: true,
      sqlLexingCompleted: true,
      candidateExtractionCompleted: true,
    });
    assert.deepEqual(result.occurrences, []);
    assert.deepEqual(result.diagnostics, []);
  },
);

test(
  "review repair two: retains typed E now cast gaps without decoding escaped spellings",
  // verify direct quoted-now casts and opaque escape contents
  () => {
    const rawDirect = extract(
      "fixtures/typed-e-now.sql",
      "SELECT E'now'::timestamp, e'NOW'::DATE, E'NoW'::time;",
    );
    assert.deepEqual(rawDirect.occurrences, []);
    assert.deepEqual(diagnosticCodes(rawDirect), [
      "sql.typed-current-context-unresolved",
      "sql.typed-current-context-unresolved",
      "sql.typed-current-context-unresolved",
    ]);

    const embeddedDirect = extract(
      "fixtures/typed-e-now.ts",
      "client.query(\"SELECT E'now'::timestamp, e'NOW'::DATE, E'NoW'::time\");",
    );
    assert.deepEqual(embeddedDirect.occurrences, []);
    assert.deepEqual(diagnosticCodes(embeddedDirect), [
      "sql.typed-current-context-unresolved",
      "sql.typed-current-context-unresolved",
      "sql.typed-current-context-unresolved",
    ]);

    const escapedSpellings = extract(
      "fixtures/typed-e-escaped.sql",
      String.raw`SELECT E'n\x6fw'::timestamp, e'\x6eow'::date;`,
    );
    assert.deepEqual(escapedSpellings.occurrences, []);
    assert.deepEqual(escapedSpellings.diagnostics, []);
  },
);

test(
  "review repair: normalizes SQL qualification comments and precision without collapsing syntax",
  // verify provider-token normalization shape
  () => {
    const unqualified = occurrencesFor(
      extract("fixtures/sql-normalization.sql", "SELECT now();"),
      "postgres.now",
    )[0];
    const qualified = occurrencesFor(
      extract("fixtures/sql-normalization.sql", "SELECT x.now();"),
      "postgres.now",
    )[0];
    const padded = occurrencesFor(
      extract("fixtures/sql-normalization.sql", "SELECT now/*padding*/();"),
      "postgres.now",
    )[0];
    const precisionThree = occurrencesFor(
      extract("fixtures/sql-normalization.sql", "SELECT CURRENT_TIMESTAMP(3);"),
      "postgres.current-timestamp",
    )[0];
    const precisionSix = occurrencesFor(
      extract("fixtures/sql-normalization.sql", "SELECT CURRENT_TIMESTAMP(6);"),
      "postgres.current-timestamp",
    )[0];
    const noPrecision = occurrencesFor(
      extract("fixtures/sql-normalization.sql", "SELECT CURRENT_TIMESTAMP;"),
      "postgres.current-timestamp",
    )[0];

    assert.notEqual(
      unqualified.normalizedExpressionSha256,
      qualified.normalizedExpressionSha256,
    );
    assert.equal(
      unqualified.normalizedExpressionSha256,
      padded.normalizedExpressionSha256,
    );
    assert.equal(
      precisionThree.normalizedExpressionSha256,
      precisionSix.normalizedExpressionSha256,
    );
    assert.notEqual(
      precisionThree.normalizedExpressionSha256,
      noPrecision.normalizedExpressionSha256,
    );
  },
);

test(
  "retains PostgreSQL typed-context dollar-body and lexical uncertainty explicitly",
  // verify sql error and gap outcomes
  () => {
    const typed = extract(
      "fixtures/typed-context.sql",
      "SELECT 'now'::timestamp;",
    );
    assert.deepEqual(typed.occurrences, []);
    assert.deepEqual(diagnosticCodes(typed), ["sql.typed-current-context-unresolved"]);

    const dollar = extract(
      "fixtures/dollar.sql",
      "CREATE FUNCTION f() RETURNS void AS $body$ SELECT now(); $body$ LANGUAGE sql;",
    );
    assert.equal(dollar.occurrences.length, 1);
    assert.equal(dollar.occurrences[0].surface, "postgres.dollar-quoted-body");
    assert.equal(dollar.occurrences[0].kind, "nested-language-candidate");
    assert.equal(dollar.occurrences[0].confidence, "nested-language-unresolved");
    assert.deepEqual(diagnosticCodes(dollar), ["sql.dollar-body-unresolved"]);

    const malformed = extract(
      "fixtures/unterminated.sql",
      "SELECT 'unterminated",
    );
    assert.deepEqual(malformed.coverage, {
      sourceParseCompleted: false,
      sqlLexingCompleted: false,
      candidateExtractionCompleted: false,
    });
    assert.deepEqual(diagnosticCodes(malformed), ["sql.lex-error"]);
    assert.equal(malformed.diagnostics[0].severity, "error");
  },
);

test(
  "admits only fixed-context or anchored embedded SQL and marks dynamic construction",
  // verify bounded embedded sql admission
  () => {
    const ordinary = extract(
      "fixtures/ordinary.ts",
      "const prose = \"an apostrophe's ordinary sentence\";",
    );
    assert.deepEqual(ordinary.occurrences, []);
    assert.deepEqual(ordinary.diagnostics, []);

    const fixed = extract(
      "fixtures/fixed.ts",
      "client.query(\"SELECT now()\"); const tagged = sql`SELECT statement_timestamp()`;",
    );
    assert.deepEqual(
      fixed.occurrences.map(
        // retain fixed-context confidence
        ({ surface, confidence }) => ({ surface, confidence }),
      ),
      [
        { surface: "postgres.now", confidence: "syntactic-direct" },
        { surface: "postgres.statement-timestamp", confidence: "syntactic-direct" },
      ],
    );

    const anchored = extract(
      "fixtures/anchored.ts",
      "const prose = \"please call now() when convenient\";",
    );
    assert.equal(anchored.occurrences[0].surface, "postgres.now");
    assert.equal(anchored.occurrences[0].confidence, "name-only-candidate");
    assert.deepEqual(diagnosticCodes(anchored), ["sql.context-ambiguous"]);

    const dynamic = extract(
      "fixtures/dynamic.ts",
      "client.query(`SELECT now() ${value} statement_timestamp() ${other} clock_timestamp()`); client.query('SELECT ' + value); client.query(queryText); eval(value); Function(value);",
    );
    assert.deepEqual(
      dynamic.occurrences.map(
        // retain admitted template segment kinds
        (occurrence) => occurrence.embeddedSql?.segmentKind ?? null,
      ),
      ["head", "middle", "tail"],
    );
    const dynamicCodes = new Set(diagnosticCodes(dynamic));
    assert.equal(dynamicCodes.has("sql.dynamic-fragment-unresolved"), true);
    assert.equal(dynamicCodes.has("sql.dynamic-execution-unresolved"), true);
    assert.equal(dynamicCodes.has("javascript.dynamic-code-unresolved"), true);

    const lexicalFailure = extract(
      "fixtures/query-error.ts",
      "client.query(\"SELECT 'unterminated\");",
    );
    assert.deepEqual(lexicalFailure.coverage, {
      sourceParseCompleted: true,
      sqlLexingCompleted: false,
      candidateExtractionCompleted: false,
    });
    assert.deepEqual(diagnosticCodes(lexicalFailure), ["sql.lex-error"]);
  },
);

test(
  "keeps escaped embedded SQL in decoded coordinates and orders tied host spans deterministically",
  // verify dual coordinate domains
  () => {
    const sourceText = "client.query(\"SELECT now\\x28\\x29, now\\x28\\x29\");";
    const result = extract("fixtures/escaped.ts", sourceText);
    const providers = occurrencesFor(result, "postgres.now");
    assert.equal(providers.length, 2);
    assert.deepEqual(
      providers.map(
        // retain decoded positions and ordinals
        (occurrence) => ({
          decodedRange: occurrence.embeddedSql.decodedRange,
          duplicateOrdinal: occurrence.duplicateOrdinal,
        }),
      ),
      [
        { decodedRange: { startOffsetUtf16: 7, endOffsetUtf16: 12 }, duplicateOrdinal: 1 },
        { decodedRange: { startOffsetUtf16: 14, endOffsetUtf16: 19 }, duplicateOrdinal: 2 },
      ],
    );
    const containerStart = sourceText.indexOf('"');
    const containerEnd = sourceText.lastIndexOf('"') + 1;
    // require both escaped providers to retain only the container host range
    for (const occurrence of providers) {
      assert.equal(occurrence.embeddedSql.hostMapping, "container-only-escaped");
      assert.equal(occurrence.physicalSpan.startOffsetUtf16, containerStart);
      assert.equal(occurrence.physicalSpan.endOffsetUtf16, containerEnd);
      assert.deepEqual(
        occurrence.physicalSpan,
        occurrence.embeddedSql.containerPhysicalSpan,
      );
    }
    assert.equal(JSON.stringify(extract("fixtures/escaped.ts", sourceText)), JSON.stringify(result));
  },
);

test(
  "reports exact literal and template container ranges without confusing UTF-16 with bytes",
  // verify delimiter payload and nonbmp coordinates
  () => {
    const sourceText = [
      "const emoji = '😀';",
      "client.query('SELECT now()');",
      "client.query(`SELECT statement_timestamp()`);",
      "client.query(`SELECT now() ${value} clock_timestamp() ${other} timeofday()`);",
    ].join("\n");
    const result = extract("fixtures/coordinates.ts", sourceText);
    const stringOccurrence = occurrencesFor(result, "postgres.now")[0];
    const templateOccurrence = occurrencesFor(result, "postgres.statement-timestamp")[0];
    const segments = result.occurrences.filter(
      // retain interpolated template providers
      (occurrence) => occurrence.embeddedSql?.interpolationPresent === true,
    );

    assert.equal(stringOccurrence.embeddedSql.containerKind, "string-literal");
    assert.equal(
      stringOccurrence.embeddedSql.payloadPhysicalSpan.startOffsetUtf16,
      stringOccurrence.embeddedSql.containerPhysicalSpan.startOffsetUtf16 + 1,
    );
    assert.equal(
      stringOccurrence.embeddedSql.payloadPhysicalSpan.endOffsetUtf16,
      stringOccurrence.embeddedSql.containerPhysicalSpan.endOffsetUtf16 - 1,
    );
    assert.equal(templateOccurrence.embeddedSql.containerKind, "no-substitution-template");
    assert.equal(
      templateOccurrence.embeddedSql.payloadPhysicalSpan.startOffsetUtf16,
      templateOccurrence.embeddedSql.containerPhysicalSpan.startOffsetUtf16 + 1,
    );
    assert.equal(
      templateOccurrence.embeddedSql.payloadPhysicalSpan.endOffsetUtf16,
      templateOccurrence.embeddedSql.containerPhysicalSpan.endOffsetUtf16 - 1,
    );
    assert.deepEqual(
      segments.map(
        // retain head middle and tail kinds
        (occurrence) => occurrence.embeddedSql.segmentKind,
      ),
      ["head", "middle", "tail"],
    );
    assert.equal(stringOccurrence.physicalSpan.startLine, 2);
    assert.equal(
      stringOccurrence.physicalSpan.startOffsetUtf16,
      sourceText.indexOf("now()", sourceText.indexOf("😀")),
    );
    assert.ok(Buffer.byteLength(sourceText.slice(0, stringOccurrence.physicalSpan.startOffsetUtf16)) > stringOccurrence.physicalSpan.startOffsetUtf16);
  },
);

test(
  "redacts literals keys owners and expression payloads from hashes identifiers and serialized output",
  // verify privacy-preserving normalization
  () => {
    const firstSource = [
      "class Box {",
      "  #alpha() {",
      "    service['methodA'].now('secretAA', { 'keyAlpha': obj['indexAAA'] }, /regexAAA/, `templateA`);",
      "  }",
      "}",
    ].join("\n");
    const secondSource = [
      "class Box {",
      "  #bravo() {",
      "    service['methodB'].now('secretBB', { 'keyBravo': obj['indexBBB'] }, /regexBBB/, `templateB`);",
      "  }",
      "}",
    ].join("\n");
    const first = extract("fixtures/privacy.ts", firstSource);
    const second = extract("fixtures/privacy.ts", secondSource);
    assert.equal(first.occurrences.length, 1);
    assert.equal(second.occurrences.length, 1);
    assert.notEqual(first.source.sha256, second.source.sha256);
    assert.equal(first.occurrences[0].enclosingSymbol, "<private>");
    assert.equal(second.occurrences[0].enclosingSymbol, "<private>");
    assert.equal(
      first.occurrences[0].normalizedExpressionSha256,
      second.occurrences[0].normalizedExpressionSha256,
    );
    assert.equal(first.occurrences[0].id, second.occurrences[0].id);
    const serialized = `${JSON.stringify(first)}${JSON.stringify(second)}`;
    const privateMarkers = [
      "alpha",
      "bravo",
      "methodA",
      "methodB",
      "secretAA",
      "secretBB",
      "keyAlpha",
      "keyBravo",
      "indexAAA",
      "indexBBB",
      "regexAAA",
      "regexBBB",
      "templateA",
      "templateB",
    ];
    // reject every private source marker from output
    for (const marker of privateMarkers) {
      assert.equal(serialized.includes(marker), false, marker);
    }
  },
);

test(
  "review repair: redacts object method getter and setter identifier keys from occurrence preimages",
  // verify object member key privacy
  () => {
    const firstSource = [
      "now({",
      "  secretAlpha() { return 1; },",
      "  get secretAlpha() { return 2; },",
      "  set secretAlpha(value) { sink(value); },",
      "});",
    ].join("\n");
    const secondSource = [
      "now({",
      "  secretBravo() { return 1; },",
      "  get secretBravo() { return 2; },",
      "  set secretBravo(value) { sink(value); },",
      "});",
    ].join("\n");
    const first = extract("fixtures/object-member-privacy.ts", firstSource);
    const second = extract("fixtures/object-member-privacy.ts", secondSource);
    const firstOccurrence = occurrencesFor(first, "ecmascript.wrapper-now")[0];
    const secondOccurrence = occurrencesFor(second, "ecmascript.wrapper-now")[0];

    assert.notEqual(first.source.sha256, second.source.sha256);
    assert.equal(
      firstOccurrence.normalizedExpressionSha256,
      secondOccurrence.normalizedExpressionSha256,
    );
    assert.equal(firstOccurrence.id, secondOccurrence.id);
    const serialized = `${JSON.stringify(first)}${JSON.stringify(second)}`;
    assert.equal(serialized.includes("secretAlpha"), false);
    assert.equal(serialized.includes("secretBravo"), false);
  },
);

test(
  "uses deterministic source-order sorting coordinates diagnostics and occurrence identifiers",
  // verify stable repeated bytes and finite records
  () => {
    const sourceText = [
      "const later = () => Date.now();",
      "const first = () => performance.now();",
      "const text = \"now()\";",
    ].join("\n");
    const first = extract("fixtures/stable.ts", sourceText);
    const second = extract("fixtures/stable.ts", sourceText);
    const reversedInput = {};
    reversedInput.sourceText = sourceText;
    reversedInput.sourcePath = "fixtures/stable.ts";
    const reversed = extractBackendClockOccurrences(reversedInput);
    assert.equal(JSON.stringify(first), JSON.stringify(second));
    assert.equal(JSON.stringify(reversed), JSON.stringify(first));
    assert.deepEqual(
      first.occurrences.map(
        // retain physical start order
        (occurrence) => occurrence.physicalSpan.startOffsetUtf16,
      ),
      [...first.occurrences]
        .map(
          // retain one comparable offset
          (occurrence) => occurrence.physicalSpan.startOffsetUtf16,
        )
        .sort(
          // sort numeric offsets
          (left, right) => left - right,
        ),
    );
    // validate every occurrence and diagnostic finite schema
    for (const occurrence of first.occurrences) {
      assertOccurrenceContract(occurrence);
    }
    // validate every diagnostic fixed schema
    for (const diagnostic of first.diagnostics) {
      assertExactKeys(diagnostic, DIAGNOSTIC_KEYS);
      assert.equal(DIAGNOSTIC_CODES.has(diagnostic.code), true);
      assert.equal(["error", "gap"].includes(diagnostic.severity), true);
      assert.deepEqual(diagnostic.relatedOccurrenceIds, [...diagnostic.relatedOccurrenceIds].sort());
    }
  },
);
