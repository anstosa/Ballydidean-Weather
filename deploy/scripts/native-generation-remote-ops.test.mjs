import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const gateway = fileURLToPath(new URL("./native-generation-remote-ops.sh", import.meta.url));
const commit = "a".repeat(40);

// execute the actual gateway dispatch with isolated application owners
function run(body, args = []) {
  return spawnSync("bash", ["-c", `source "$1"\n${body}`, "native-gateway-test", gateway, ...args], {
    encoding: "utf8",
  });
}

// select the exact application generation while retaining the chosen lock mode
const context = String.raw`
native_gateway_context() {
  # identify the exact context used by the gateway
  if (($#)); then printf 'context:%s\n' "$1"; else printf 'context:ordinary\n'; fi
  native_active_generation=rust-v1
  native_active_commit=$expected_commit
}
`;

// dashboard mutations and ordinary reads use distinct contexts from core staging
test("native gateway selects dashboard and committed contexts separately from core actions", () => {
  const body = `${context}\n${String.raw`
expected_commit=$2
dashboard_override_present() { return 0; }
native_dashboard_context_admission() { printf 'dashboard-admission\n'; }
native_release_control_handles() { return 0; }
native_release_control_dispatch() { printf 'dispatch:%s\n' "$1"; }
native_gateway_execute "$expected_commit" dashboard-release 2099.01.01-2 2099.01.01-1 2099.01.01-1
native_gateway_execute "$expected_commit" dashboard-rollback
native_gateway_execute "$expected_commit" dashboard-rollback-core
native_gateway_execute "$expected_commit" status
native_gateway_execute "$expected_commit" stage 2099.01.01-3
`}`;
  const result = run(body, [commit]);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout,
    "context:--dashboard-override\ndashboard-admission\ndispatch:dashboard-release\n" +
    "context:--dashboard-override\ndashboard-admission\ndispatch:dashboard-rollback\n" +
    "context:--dashboard-override\ndashboard-admission\ndispatch:dashboard-rollback-core\n" +
    "context:--committed-dashboard-context\ndashboard-admission\ndispatch:status\n" +
    "context:ordinary\ndispatch:stage\n");
});

// boot recovery selects the authenticated dashboard controller for the current native commit
test("native gateway recovers dashboard web under the inherited lock", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-native-dashboard-gateway-"));
  try {
    await writeFile(join(root, "native-dashboard-release-control.sh"),
      "native_dashboard_system_recover() { printf 'recover-web\\n'; }\n");
    const body = `${context}\n${String.raw`
expected_commit=$2
fixture_directory=$3
native_gateway_package() { printf 'package:%s\n' "$1"; native_evidence_package=$fixture_directory; }
native_gateway_dashboard_recover
`}`;
    const result = run(body, [commit, root]);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, `context:--inherited-dashboard-lock\npackage:${commit}\nrecover-web\n`);
  } finally {
    await rm(root, { force: true, recursive: true });
  }
});
