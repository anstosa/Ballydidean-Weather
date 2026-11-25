import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const script = fileURLToPath(
  new URL("./native-evidence-remote-dispatch.sh", import.meta.url),
);
const shaA = "a".repeat(64);
const shaB = "b".repeat(64);

// replace only process boundaries
function dispatch(action, operands = [], environment = {}) {
  const shell = `
source "$1"
native_evidence_binary=/opt/weather/current/bin/weather-evidence
native_evidence_remote_exec_root() { printf 'root'; printf '\\t%s' "$@"; printf '\\n'; }
native_evidence_remote_exec_service() { printf 'service'; printf '\\t%s' "$@"; printf '\\n'; }
shift
native_evidence_remote_dispatch "$@"
`;
  return spawnSync("bash", ["-c", shell, "fixture", script, action, ...operands], {
    encoding: "utf8",
    env: { PATH: process.env.PATH, ...environment },
  });
}

// replace export and process effects
function dispatchExport(action, operands = []) {
  const root = mkdtempSync(join(tmpdir(), "native-evidence-dispatch-"));
  const events = join(root, "events");
  const sql = join(root, "query.sql");
  writeFileSync(sql, "");
  const shell = `
source "$1"
native_evidence_binary=/opt/weather/current/bin/weather-evidence
native_active_environment=/opt/weather/current/deploy/releases/current.env
native_evidence_remote_prepare_export() {
  printf 'prepare:%s\\n' "$1" >>"$FIXTURE_EVENTS"
  native_evidence_export_environment=$native_active_environment
  native_evidence_export_database=weather
  native_evidence_export_temporary=$FIXTURE_ROOT
}
native_evidence_remote_cleanup_export() { printf 'cleanup\\n' >>"$FIXTURE_EVENTS"; }
native_evidence_remote_run_function_export() {
  printf 'psql' >>"$FIXTURE_EVENTS"
  printf ':%s' "$@" >>"$FIXTURE_EVENTS"
  printf '\\n' >>"$FIXTURE_EVENTS"
  cat "$native_evidence_export_temporary/function-export.sql" >"$FIXTURE_SQL"
  printf '{"databaseManifest":true}\\n'
}
native_evidence_remote_run_observation_query() {
  printf 'query:%s:%s\\n' "$1" "$2" >>"$FIXTURE_EVENTS"
  printf 'SQL-LINES\\n'
}
native_evidence_remote_exec_root() {
  local action=$1 input
  shift
  input=$(cat)
  printf 'exec:%s:%s\\n' "$action" "$*" >>"$FIXTURE_EVENTS"
  printf 'root:%s\\n' "\${WEATHER_ADJUSTMENT_EVIDENCE_ROOT-}" >>"$FIXTURE_EVENTS"
  printf 'stdin:%s\\n' "$input" >>"$FIXTURE_EVENTS"
  # return exact boundary fixtures for multi-step export verbs
  case "$action" in
    freeze-adjustment-evidence-snapshot) printf '{"snapshot":true}\\n' ;;
    frame-v2-authorization) printf 'AUTHORIZATION\\n' ;;
    v2-authorization-dates) printf '2026-11-01 2026-11-02\\n' ;;
    stream-archive) printf 'TAR-V1' ;;
    stream-v2-envelope) printf 'ENVELOPE-V2' ;;
    verify-v2-readiness) ;;
    *) printf '{"native":"%s"}\\n' "$action" ;;
  esac
}
native_host_shadow_finalize() {
  printf 'shadow:%s\\n' "$1" >>"$FIXTURE_EVENTS"
  printf '{"native":"shadow-finalize"}\\n'
}
native_host_registration_initialize() {
  printf 'registration:%s\\n' "$1" >>"$FIXTURE_EVENTS"
  printf '{"native":"registration-initialize"}\\n'
}
native_host_registration_lifecycle() {
  printf 'lifecycle\\n' >>"$FIXTURE_EVENTS"
  printf '{"native":"registration-lifecycle"}\\n'
}
native_host_owner_access() {
  printf 'access:%s\\n' "$1" >>"$FIXTURE_EVENTS"
  printf '{"native":"owner-access"}\\n'
}
native_host_owner_operation() {
  printf 'owner:%s:%s\\n' "$1" "$2" >>"$FIXTURE_EVENTS"
  printf '{"native":"owner-terminal"}\\n'
}
native_host_scorecard_install() {
  printf 'scorecard:%s\\n' "$*" >>"$FIXTURE_EVENTS"
  printf '{"native":"scorecard-install"}\\n'
}
native_host_family_release() {
  printf 'family:%s\\n' "$*" >>"$FIXTURE_EVENTS"
  printf '{"native":"family-release"}\\n'
}
native_host_forecast_training_export() {
  printf 'training:%s:%s\\n' "$1" "$2" >>"$FIXTURE_EVENTS"
  printf 'TRAINING-ARCHIVE'
}
validate_calendar_date_range() {
  printf 'dates:%s:%s:%s\\n' "$1" "$2" "$3" >>"$FIXTURE_EVENTS"
}
shift
native_evidence_remote_dispatch "$@"
`;
  const result = spawnSync(
    "bash",
    ["-c", shell, "fixture", script, action, ...operands],
    {
      encoding: "utf8",
      env: {
        PATH: process.env.PATH,
        FIXTURE_EVENTS: events,
        FIXTURE_ROOT: root,
        FIXTURE_SQL: sql,
      },
    },
  );
  return {
    events: readFileSync(events, "utf8"),
    result,
    root,
    sql: readFileSync(sql, "utf8"),
  };
}

// remove one export fixture
function cleanupExport(fixture) {
  rmSync(fixture.root, { force: true, recursive: true });
}

// test exact command translations
test("maps direct evidence operations to the native CLI", () => {
  const authority = {
    WEATHER_ANCHOR_CONTROL_SHA256: shaB,
    WEATHER_ANCHOR_CONTROL_VERSION: "14",
  };
  const cases = [
    ["adjustment-family-release-status", [shaA], `root\tfamily-release-status\t${shaA}\n`],
    ["adjustment-family-release-current-v1", ["rain"], "root\tfamily-release-current\train\n"],
    ["adjustment-family-release-lineage-v2", [], "root\tfamily-release-lineage-v2\n"],
    ["adjustment-revision-capture-epoch-v1", [], "root\trevision-capture-epoch-read-v1\n"],
    ["adjustment-revision-capture-epoch-snapshot-v1", [], "root\trevision-capture-epoch-snapshot-read-v1\n"],
    ["adjustment-shadow-metadata-custody-status-v1", [], "service\tshadow-metadata-custody-status-v1\n"],
    ["adjustment-revision-gap-start-v1", [], "service\trevision-gap-start-v1\n"],
    ["adjustment-revision-gap-page-v1", [shaA, shaB], `service\trevision-gap-page-v1\t${shaA}\t${shaB}\n`],
    ["adjustment-revision-gap-ack-v1", [shaA, shaB], `service\trevision-gap-ack-v1\t${shaA}\t${shaB}\n`],
    ["adjustment-maintenance-anchor-status-v2", [], "root\tmaintenance-anchor-status-v2\n"],
    ["adjustment-development-custody-anchor-current-v1", [], "root\tdevelopment-custody-anchor-current-v1\n"],
    ["adjustment-rain-control-custody-anchor-current-v1", [], "root\train-control-custody-anchor-current-v1\n"],
    ["adjustment-development-custody-anchor-install-v1", [shaA], `root\tdevelopment-custody-anchor-install-v1\t${shaA}\n`, authority],
    ["adjustment-rain-control-custody-anchor-install-v1", [shaA], `root\train-control-custody-anchor-install-v1\t${shaA}\n`, authority],
    ["adjustment-unsupported-terminal-proof-install-v1", [shaA], `root\tunsupported-terminal-proof-install-v1\t${shaA}\n`, authority],
    ["adjustment-unsupported-terminal-proof-current-v1", [], "root\tunsupported-terminal-proof-current-v1\n"],
    ["adjustment-future-input-seal-current-v2", [], "root\tfuture-input-seal-current-v2\n", authority],
    ["adjustment-maintenance-anchor-current-v3", [], "root\tmaintenance-anchor-current-v3\n", authority],
    ["adjustment-future-input-seal-install-v2", [shaA], `root\tfuture-input-seal-install-v2\t${shaA}\n`],
    ["adjustment-maintenance-anchor-install-v2", [shaA], `root\tmaintenance-anchor-install-v2\t${shaA}\n`, authority],
    ["adjustment-maintenance-anchor-install-v3", [shaA], `root\tmaintenance-anchor-install-v3\t${shaA}\n`, authority],
    ["adjustment-maintenance-anchor-finalize-v2", [shaB], `root\tmaintenance-anchor-finalize-v2\t${shaB}\n`],
    ["adjustment-maintenance-anchor-finalize-v3", [shaB], `root\tmaintenance-anchor-finalize-v3\t${shaB}\n`],
    ["adjustment-archive-next", [], "service\tarchive-next\n"],
    ["adjustment-archive-ack", [shaA, shaB], `service\tarchive-ack\t${shaA}\t${shaB}\n`],
    ["adjustment-archive-ack-final", [shaA, shaB], `service\tarchive-ack-final\t${shaA}\t${shaB}\n`],
  ];
  // compare every non-custody action
  for (const [action, operands, expected, environment] of cases) {
    const result = dispatch(action, operands, environment);
    assert.equal(result.status, 0, `${action}: ${result.stderr}`);
    assert.equal(result.stdout, expected, action);
  }
});

// test complete custody forwarding
test("maps the full cold custody acknowledgement", () => {
  const operands = ["17", shaA, shaB, "18", shaB, shaA, shaA, shaB, shaA, "none"];
  // preserve both installed versions
  for (const version of ["v1", "v2"]) {
    const result = dispatch(`adjustment-revision-custody-ack-${version}`, operands);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(
      result.stdout,
      `service\trevision-cold-custody-ack-${version}\t${operands.join("\t")}\n`,
    );
  }
});

// test anchor authority admission
test("passes authority-bound operations only with the original environment", () => {
  const refused = dispatch("adjustment-maintenance-anchor-install-v3", [shaA]);
  assert.equal(refused.status, 69);
  assert.match(refused.stderr, /anchor authority is unavailable/);

  const admitted = dispatch(
    "adjustment-maintenance-anchor-install-v3",
    [shaA],
    {
      WEATHER_ANCHOR_CONTROL_SHA256: shaB,
      WEATHER_ANCHOR_CONTROL_VERSION: "14",
    },
  );
  assert.equal(admitted.status, 0, admitted.stderr);
  assert.equal(admitted.stdout, `root\tmaintenance-anchor-install-v3\t${shaA}\n`);
});

// test closed refusal statuses
test("fails closed without widening the release-control allowlist", () => {
  const invalid = dispatch("adjustment-archive-ack", [shaA, "not-a-hash"]);
  assert.equal(invalid.status, 2);
  assert.match(invalid.stderr, /invalid native evidence arguments/);

  const incomplete = dispatch("adjustment-family-release");
  assert.equal(incomplete.status, 2);
  assert.match(incomplete.stderr, /invalid native evidence arguments/);

  const unhandled = dispatch("dashboard-release", ["2026.11.25-1"]);
  assert.equal(unhandled.status, 64);
  assert.equal(unhandled.stdout, "");
});

// test explicit wrapper branching
test("classifies mapped, refused and unrelated operations explicitly", () => {
  const shell = `
source "$1"
native_evidence_handles adjustment-archive-next; printf '%s\\n' "$?"
native_evidence_handles adjustment-database-ledger-v3; printf '%s\\n' "$?"
native_evidence_handles dashboard-release; printf '%s\\n' "$?"
`;
  const result = spawnSync("bash", ["-c", shell, "fixture", script], {
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, "0\n0\n1\n");
});

// test database ledger projection sequencing
test("pipes the fixed ledger query into the native projector", () => {
  const fixture = dispatchExport("adjustment-database-ledger-v3");
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    assert.equal(
      fixture.events,
      "prepare:0\npsql:\nexec:project-maintenance-ledger-v3:\nroot:\n" +
        'stdin:{"databaseManifest":true}\ncleanup\n',
    );
    assert.match(fixture.sql, /BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;/);
    assert.match(fixture.sql, /SET LOCAL statement_timeout = '5min';/);
    assert.match(fixture.sql, /'payload', NULL::jsonb,/);
    assert.match(fixture.sql, /FROM adjustment_evaluation_export_manifest_v1 manifest/);
  } finally {
    cleanupExport(fixture);
  }
});

// test registration schedule projection sequencing
test("pipes the fixed family schedule into the native projector", () => {
  const fixture = dispatchExport("adjustment-registration-schedule-status-v3");
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    assert.equal(
      fixture.events,
      "prepare:1\npsql:\nexec:project-registration-schedule-status-v3:\nroot:\n" +
        'stdin:{"databaseManifest":true}\ncleanup\n',
    );
    assert.match(
      fixture.sql,
      /jsonb_build_array\(adjustment_shadow_registration_slot_v3\('temperature'\), adjustment_shadow_registration_slot_v3\('wind'\), adjustment_shadow_registration_slot_v3\('rain'\)\)/,
    );
  } finally {
    cleanupExport(fixture);
  }
});

// test both revision start projections
test("pipes exact revision snapshot functions into native cold start", () => {
  const cutoff = "2026-11-25T12:34:56.789Z";
  const explicit = dispatchExport("adjustment-revision-catalog-start-v1", [cutoff, "17"]);
  const current = dispatchExport("adjustment-revision-catalog-current-start-v1", [cutoff]);
  try {
    assert.equal(explicit.result.status, 0, explicit.result.stderr);
    assert.match(explicit.events, /^prepare:1\npsql::::2026-11-25T12:34:56\.789Z:17\n/);
    assert.match(explicit.events, /exec:revision-cold-start-v1:/);
    assert.match(
      explicit.events,
      /root:\/var\/lib\/weather\/xweather\/adjustment-evidence/,
    );
    assert.match(
      explicit.sql,
      /adjustment_revision_serving_snapshot_v1\(:'revision_cutoff'::timestamptz, :'revision_ordinal'::bigint\)/,
    );

    assert.equal(current.result.status, 0, current.result.stderr);
    assert.match(current.events, /^prepare:1\npsql::::2026-11-25T12:34:56\.789Z:\n/);
    assert.match(current.sql, /adjustment_revision_frontier_v1\(\)/);
    assert.match(current.sql, /observed\.frontier->>'archiveCommitOrdinal'/);
  } finally {
    cleanupExport(explicit);
    cleanupExport(current);
  }
});

// test direct-successor page forwarding
test("reads revision pages from the fixed native spool", () => {
  const operands = ["17", shaA, shaB, "18", shaB, shaA];
  const fixture = dispatchExport("adjustment-revision-catalog-page-v1", operands);
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    assert.equal(
      fixture.events,
      `prepare:1\nexec:revision-cold-page-v1:${operands.join(" ")}\n` +
        "root:/var/lib/weather/xweather/adjustment-evidence\nstdin:\ncleanup\n",
    );
  } finally {
    cleanupExport(fixture);
  }
});

// test gateway-owned shadow finalization
test("delegates exact shadow finalization to the verified host owner", () => {
  const fixture = dispatchExport("adjustment-shadow-metadata-custody-finalize-v1", [shaA]);
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    assert.equal(fixture.events, `shadow:${shaA}\n`);
    assert.equal(fixture.result.stdout, '{"native":"shadow-finalize"}\n');
  } finally {
    cleanupExport(fixture);
  }
});

// test gateway-owned registration initialization
test("delegates exact registration initialization to the verified host owner", () => {
  const fixture = dispatchExport("adjustment-registration-schedule-initialize-v3", [shaB]);
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    assert.equal(fixture.events, `registration:${shaB}\n`);
    assert.equal(fixture.result.stdout, '{"native":"registration-initialize"}\n');
  } finally {
    cleanupExport(fixture);
  }
});

// test remaining gateway-owned host operations
test("delegates lifecycle and access operations to verified host owners", () => {
  const lifecycle = dispatchExport("adjustment-registration-lifecycle-status-v4");
  const access = dispatchExport("adjustment-confirmation-access-burn-v3", [shaA]);
  try {
    assert.equal(lifecycle.result.status, 0, lifecycle.result.stderr);
    assert.equal(lifecycle.events, "lifecycle\n");
    assert.equal(lifecycle.result.stdout, '{"native":"registration-lifecycle"}\n');
    assert.equal(access.result.status, 0, access.result.stderr);
    assert.equal(access.events, `access:${shaA}\n`);
    assert.equal(access.result.stdout, '{"native":"owner-access"}\n');
  } finally {
    cleanupExport(lifecycle);
    cleanupExport(access);
  }
});

// test the original distinct unsupported terminal owner operations
test("delegates unsupported terminal records and retirement to verified host owners", () => {
  const record = dispatchExport("adjustment-shadow-unsupported-terminal-record-v1", [shaA]);
  const retire = dispatchExport("adjustment-shadow-unsupported-terminal-retire-v1", [shaB]);
  try {
    assert.equal(record.result.status, 0, record.result.stderr);
    assert.equal(record.events, `owner:unsupported-terminal:${shaA}\n`);
    assert.equal(retire.result.status, 0, retire.result.stderr);
    assert.equal(retire.events, `owner:unsupported-retire:${shaB}\n`);
  } finally {
    cleanupExport(record);
    cleanupExport(retire);
  }
});

// exercise the qualified terminal variants over the same installed owner boundary
test("delegates the qualified record and retirement to their distinct native owners", () => {
  const record = dispatchExport("adjustment-shadow-terminal-record-v3", [shaA]);
  const retire = dispatchExport("adjustment-shadow-terminal-retire-v3", [shaB]);
  try {
    assert.equal(record.result.status, 0, record.result.stderr);
    assert.equal(record.events, `owner:terminal:${shaA}\n`);
    assert.equal(retire.result.status, 0, retire.result.stderr);
    assert.equal(retire.events, `owner:retire:${shaB}\n`);
  } finally {
    cleanupExport(record);
    cleanupExport(retire);
  }
});

// retain the two source scorecard installation mode grammars
test("delegates scorecard installation to the version-bound native owner", () => {
  const legacy = dispatchExport("install-adjustment-scorecard", [shaA]);
  const current = dispatchExport("install-adjustment-scorecard-v2", [shaB]);
  try {
    assert.equal(legacy.result.status, 0, legacy.result.stderr);
    assert.equal(legacy.events, `scorecard:${shaA}\n`);
    assert.equal(current.result.status, 0, current.result.stderr);
    assert.equal(current.events, `scorecard:--v2 ${shaB}\n`);
  } finally {
    cleanupExport(legacy);
    cleanupExport(current);
  }
});

// preserve exact original family transaction operand ordering
test("delegates the native family release through the known host transaction owner", () => {
  const operands = ["2026.11.25-1", "2026.11.25-2", "2026.11.24-1",
    "2026.11.24-1", shaA, "temperature", shaB, shaA, "42"];
  const fixture = dispatchExport("adjustment-family-release", operands);
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    assert.equal(fixture.events, `family:${operands.join(" ")}\n`);
  } finally {
    cleanupExport(fixture);
  }
});

// retain the exact forecast training date window and native archive output
test("streams forecast training through the native host owner", () => {
  const fixture = dispatchExport("forecast-training-export", ["2026-10-01", "2026-10-31"]);
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    assert.equal(fixture.events, "dates:2026-10-01:2026-10-31:450\ntraining:2026-10-01:2026-10-31\n");
    assert.equal(fixture.result.stdout, "TRAINING-ARCHIVE");
  } finally {
    cleanupExport(fixture);
  }
});

// test value-free availability projection
test("pipes confirmation availability through the native frame", () => {
  const fixture = dispatchExport("adjustment-confirmation-availability-v2", [shaA]);
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    assert.match(fixture.events, new RegExp(`^prepare:0\\npsql:${shaA}\\n`));
    assert.match(fixture.events, new RegExp(`exec:frame-v2-availability:${shaA}`));
    assert.match(
      fixture.sql,
      /adjustment_confirmation_availability_v2\(:'registration_sha256'\)/,
    );
    assert.match(fixture.events, /cleanup\n$/);
  } finally {
    cleanupExport(fixture);
  }
});

// test byte-compatible legacy archive sequencing
test("streams the legacy archive through native snapshot and package owners", () => {
  const fixture = dispatchExport("adjustment-evaluation-export", [
    "2026-11-01",
    "2026-11-02",
  ]);
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    assert.match(fixture.events, /^dates:2026-11-01:2026-11-02:14\nprepare:1\n/);
    assert.match(fixture.events, /exec:freeze-adjustment-evidence-snapshot:/);
    assert.match(fixture.events, /query:2026-11-01:2026-11-02/);
    assert.match(fixture.events, /exec:stream-archive:/);
    assert.match(fixture.events, /stdin:SQL-LINES/);
    assert.match(fixture.events, /cleanup\n$/);
    assert.equal(fixture.result.stdout, "TAR-V1");
    const sql = readFileSync(join(fixture.root, "export.sql"), "utf8");
    assert.match(sql, /BEGIN TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY;/);
    assert.match(sql, /LIMIT 8193/);
    assert.match(sql, /adjustment_evaluation_export_rows_v1/);
  } finally {
    cleanupExport(fixture);
  }
});

// test daily v2 archive sequencing
test("checks readiness and wraps the staged native v1 archive", () => {
  const fixture = dispatchExport("adjustment-evaluation-export-v2", [
    "2026-11-01",
    "2026-11-02",
  ]);
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    assert.match(fixture.events, /psql:\nexec:verify-v2-readiness:/);
    assert.match(fixture.events, /exec:freeze-adjustment-evidence-snapshot:/);
    assert.match(fixture.events, /exec:stream-archive:/);
    assert.match(fixture.events, /exec:stream-v2-envelope:daily_monitoring /);
    assert.match(fixture.events, /cleanup\n$/);
    assert.equal(fixture.result.stdout, "ENVELOPE-V2");
  } finally {
    cleanupExport(fixture);
  }
});

// test post-burn authorization sequencing
test("derives dates and wraps a post-burn confirmation archive", () => {
  const fixture = dispatchExport("adjustment-confirmation-export-v2", [shaA, shaB, "3"]);
  try {
    assert.equal(fixture.result.status, 0, fixture.result.stderr);
    assert.match(fixture.events, new RegExp(`^prepare:1\\npsql:${shaA}:${shaB}:3\\n`));
    assert.match(fixture.events, /exec:frame-v2-authorization:/);
    assert.match(fixture.events, /exec:v2-authorization-dates:/);
    assert.match(fixture.events, /dates:2026-11-01:2026-11-02:14/);
    assert.match(fixture.events, /query:2026-11-01:2026-11-02/);
    assert.match(fixture.events, /exec:stream-v2-envelope:post_burn_confirmation /);
    assert.match(fixture.events, /cleanup\n$/);
    assert.equal(fixture.result.stdout, "ENVELOPE-V2");
  } finally {
    cleanupExport(fixture);
  }
});

// test fixed psql role and session variables
test("runs function SQL through the original training role", () => {
  const root = mkdtempSync(join(tmpdir(), "native-evidence-psql-"));
  writeFileSync(join(root, "function-export.sql"), "SELECT fixed;\n");
  const shell = `
source "$1"
native_evidence_export_environment=/opt/weather/current/deploy/releases/current.env
native_evidence_export_database=weather
native_evidence_export_temporary=$FIXTURE_ROOT
compose() { printf 'env:%s\\n' "$WEATHER_ENV_FILE"; printf 'argv:'; printf '%s|' "$@"; printf '\\n'; cat; }
native_evidence_remote_run_function_export '' '' '' '2026-11-25T12:34:56.789Z' 17
`;
  const result = spawnSync("bash", ["-c", shell, "fixture", script], {
    encoding: "utf8",
    env: { PATH: process.env.PATH, FIXTURE_ROOT: root },
  });
  try {
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /env:\/opt\/weather\/current\/deploy\/releases\/current\.env/);
    assert.match(result.stdout, /--username weather_training_export/);
    assert.match(result.stdout, /--set=revision_cutoff="\$5"/);
    assert.match(result.stdout, /--set=revision_ordinal="\$6"/);
    assert.match(result.stdout, /adjustment-maintenance-v2-export\|weather\|\|\|\|2026-11-25T12:34:56\.789Z\|17\|/);
    assert.match(result.stdout, /SELECT fixed;/);
  } finally {
    rmSync(root, { force: true, recursive: true });
  }
});
