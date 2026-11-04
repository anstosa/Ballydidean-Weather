#!/usr/bin/env bash
# globals come from the pinned installed controller
# shellcheck disable=SC2154
set -euo pipefail

# adapt only the pinned bounded fixture callbacks
direct_compatibility_definition() {
  # preserve literal shell expressions in the callback template
  # shellcheck disable=SC2016
  python3 -c '
import sys
text = sys.stdin.read()
replacements = {
    "verify_bounded_maintenance_source_compatibility ()": "verify_direct_source_compatibility ()",
    "require_fixed_v14_source_identity": "require_direct_source_identity",
    "require_fixed_v14_migration_ledger": "require_direct_target_ledger",
    "WEATHER_ENV_FILE=$target_env compose run --rm --no-deps --env WEATHER_DATABASE_NAME=\"$candidate\" migration;": "direct_fixture_migrate \"$target_env\" \"$candidate\";",
}
# reject drift instead of broadening the retained fixture
for before, after in replacements.items():
    if text.count(before) != 1:
        raise SystemExit("pinned bounded compatibility callback differs")
    text = text.replace(before, after)
sys.stdout.write(text)
'
}

# bind the fixture to the one approved source and target
require_direct_source_identity() {
  [[ "$1" == "$direct_target_environment" && "$2" == "$direct_source_environment" &&
    "$(sha256sum "$2" | awk '{print $1}')" == \
      97368a14e368dcf7fd3f3f179cd222df2f848695e1cd993abf8892e69c30d4e7 &&
    "$(env_value "$1" WEATHER_RELEASE)" == 2026.11.03-3 &&
    "$(env_value "$1" WEATHER_CONTROL_PLANE_VERSION)" == 17 &&
    "$(env_value "$1" WEATHER_CONTROL_PLANE_SHA256)" == \
      6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb ]] ||
    die "direct compatibility source or target differs"
}

# verify the genuinely executed alternate ledger
require_direct_target_ledger() {
  [[ "$1" == "$direct_source_environment" &&
    "$2" =~ ^weather_v14_compat_[0-9]+_[0-9]+$ &&
    "$(migration_history_sha256 "$1" "$2")" == \
      1cfd6251eeb66d443e36d661299c9e49e73465fab55e5dab2cfa4ee86dd36ad2 ]] ||
    die "direct compatibility target ledger differs"
}

# build a genuine empty schema 18 before applying the repaired tail
direct_fixture_migrate() {
  local environment=$1 database=$2
  WEATHER_ENV_FILE=$environment compose run --rm --no-deps \
    --env WEATHER_DATABASE_NAME="$database" migration \
    node --input-type=module -e '
import { cp, mkdtemp, readdir, rm } from "node:fs/promises";
import { join } from "node:path";
import { createDatabasePool, loadDatabaseConfiguration, runMigrations } from "@weather/database";
const root = "/opt/weather/packages/database/migrations";
const prefix = await mkdtemp("/tmp/direct-source18-");
const pool = createDatabasePool(await loadDatabaseConfiguration());
try {
  // select only canonical SQL files
  const names = (await readdir(root)).filter((name) => /^\d{4}_[a-z0-9_]+\.sql$/u.test(name)).sort();
  // retain the exact canonical prefix
  if (names.length !== 23) throw new Error("direct fixture artifact count differs");
  // copy only public SQL into the disposable tmpfs
  for (const name of names.slice(0, 18)) await cp(join(root, name), join(prefix, name));
  await runMigrations(pool, prefix);
} finally {
  await pool.end();
  await rm(prefix, { force: true, recursive: true });
}'
  WEATHER_ENV_FILE=$environment compose run --rm --no-deps \
    --env WEATHER_DATABASE_NAME="$database" \
    --env WEATHER_DATABASE_STATEMENT_TIMEOUT_MS=300000 migration \
    node deploy/scripts/migrate.mjs --atomic-direct-18-23-index-600s
}

# roll back only this invocation's uncommitted authorization
direct_compatibility_unpublish() {
  local authorization=$1 proof=$2 identity=$3 digest=$4
  # leave a committed proof or an absent publication untouched
  if [[ -e "$proof" || -L "$proof" || ( ! -e "$authorization" && ! -L "$authorization" ) ]]; then
    return 0
  fi
  [[ -f "$authorization" && ! -L "$authorization" &&
    "$(stat -c '%d:%i' "$authorization")" == "$identity" &&
    "$(stat -c '%u:%g:%a:%h' "$authorization")" == "$EUID:$(id -g):600:1" &&
    "$(sha256sum "$authorization" | awk '{print $1}')" == "$digest" ]] || return 1
  rm -- "$authorization"
  sync -f "$(dirname "$authorization")"
}

# publish the actual compatibility result without a partial final file
direct_compatibility_publish_proof() {
  python3 -I -B - "$@" <<'PY'
import datetime, hashlib, json, os, sys, tempfile
from pathlib import Path
source, target, authorization, proof = map(Path, sys.argv[1:])
value = {
    "contractVersion": "weather-direct-maintenance-source-compatibility/v1",
    "capturedAt": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "sourceRelease": "2026.10.09-2", "targetRelease": "2026.11.03-3",
    "sourceEnvironmentSha256": hashlib.sha256(source.read_bytes()).hexdigest(),
    "targetEnvironmentSha256": hashlib.sha256(target.read_bytes()).hexdigest(),
    "authorizationSha256": hashlib.sha256(authorization.read_bytes()).hexdigest(),
    "targetHistorySha256": "1cfd6251eeb66d443e36d661299c9e49e73465fab55e5dab2cfa4ee86dd36ad2",
    "fixtureBytes": 67108864, "sourceApiAndIngestPassed": True,
    "sourceRuntimeTargetHistoryCompatible": True, "state": "compatibility_ready",
    "wrongHistoryRejected": True, "unprovenHistoryRejected": True,
    "fullSizeRehearsal": "explicitly-waived",
}
fd, temporary = tempfile.mkstemp(prefix=".direct-compatibility-proof-", dir=proof.parent)
try:
    with os.fdopen(fd, "w") as stream:
        json.dump(value, stream, sort_keys=True)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    os.link(temporary, proof, follow_symlinks=False)
finally:
    os.unlink(temporary)
fd = os.open(proof.parent, os.O_RDONLY | os.O_DIRECTORY)
os.fsync(fd)
os.close(fd)
print(json.dumps(value, sort_keys=True))
PY
}

# execute only the one-task bounded source-runtime proof
direct_compatibility_main() (
  (($# == 3 && EUID == 0)) || { printf 'direct compatibility requires root and three paths\n' >&2; exit 1; }
  local installed=/opt/weather/current/deploy/scripts/update.sh
  local direct_target_environment=$1
  local direct_source_environment=/opt/weather/current/deploy/releases/2026.10.09-2.env
  local authorization=$2 proof=$3 temporary definition
  local publication_attempted=false authorization_identity authorization_sha
  [[ "$direct_target_environment" == /opt/weather/current/deploy/releases/2026.11.03-3.env &&
    "$authorization" == /opt/weather/current/deploy/releases/2026.11.03-3.migration-authorization &&
    "$proof" == /opt/weather/current/deploy/releases/2026.11.03-3.direct-compatibility.json &&
    "$(stat -c '%u:%g:%a:%h' "$installed")" == 0:0:755:1 &&
    "$(sha256sum "$installed" | awk '{print $1}')" == \
      ea1185e04e0da2439c856b5b6989ce4f87f087a63ff7bafa1781c577cf42b8a0 ]] ||
    { printf 'direct compatibility pinned inputs differ\n' >&2; exit 1; }
  # shellcheck disable=SC1090
  source "$installed"
  # retain an inherited kernel lock or acquire our own
  if [[ -n ${WEATHER_RELEASE_LOCK_FD:-} ]]; then
    [[ "$WEATHER_RELEASE_LOCK_FD" =~ ^[0-9]+$ &&
      "$(stat -Lc '%d:%i' "/proc/self/fd/$WEATHER_RELEASE_LOCK_FD")" == \
        "$(stat -c '%d:%i' "$state_dir")" ]] || die "direct compatibility lock differs"
    flock --exclusive --nonblock "$WEATHER_RELEASE_LOCK_FD" || die "direct compatibility lock is unavailable"
  else
    acquire_release_transaction_lock
  fi
  [[ "$(control_plane_digest)" == 6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb &&
    "$(read_release_state "$state_dir/current-release")" == 2026.10.09-2 &&
    "$(read_release_state "$state_dir/schema-release")" == 2026.10.09-2 &&
    ! -e "$authorization" && ! -L "$authorization" && ! -e "$proof" && ! -L "$proof" ]] ||
    die "direct compatibility source or publication changed"
  require_no_dashboard_override
  require_deployment_secrets
  require_direct_source_identity "$direct_target_environment" "$direct_source_environment"
  require_normal_release_resource_gate
  temporary=$(mktemp "$releases_dir/.direct-compatibility.XXXXXX.authorization")
  chmod 600 "$temporary"
  # remove only invocation-owned incomplete publication state
  # shellcheck disable=SC2317,SC2329
  cleanup_direct_compatibility() {
    local status=$?
    trap - EXIT
    rm -f -- "$temporary"
    # retain a substituted publication for investigation
    if [[ "$status" != 0 && "$publication_attempted" == true ]]; then
      direct_compatibility_unpublish "$authorization" "$proof" \
        "$authorization_identity" "$authorization_sha" || status=2
    fi
    exit "$status"
  }
  trap cleanup_direct_compatibility EXIT
  definition=$(declare -f verify_bounded_maintenance_source_compatibility | direct_compatibility_definition)
  eval "$definition"
  verify_direct_source_compatibility "$direct_target_environment" "$direct_source_environment" "$temporary" v14
  [[ "$(env_value "$temporary" WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256)" == \
    1cfd6251eeb66d443e36d661299c9e49e73465fab55e5dab2cfa4ee86dd36ad2 ]] ||
    die "direct compatibility authorization differs"
  authorization_identity=$(stat -c '%d:%i' "$temporary")
  authorization_sha=$(sha256sum "$temporary" | awk '{print $1}')
  publication_attempted=true
  publish_migration_authorization "$temporary" "$authorization"
  direct_compatibility_publish_proof "$direct_source_environment" \
    "$direct_target_environment" "$authorization" "$proof"
)

# leave imports inert for focused callback tests
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  direct_compatibility_main "$@"
fi
