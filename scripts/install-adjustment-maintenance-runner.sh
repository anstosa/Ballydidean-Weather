#!/usr/bin/env bash

set -euo pipefail

readonly ADJUSTMENT_RUNNER_NODE="${HOME}/n/bin/node"
readonly ADJUSTMENT_RUNNER_INSTALL_ROOT="${HOME}/.weather/adjustment-maintenance/runner"
readonly ADJUSTMENT_RUNNER_UNIT_ROOT="/etc/systemd/system"
readonly ADJUSTMENT_RUNNER_USER="ubuntu"
readonly ADJUSTMENT_RUNNER_UID="1000"
readonly ADJUSTMENT_RUNNER_GID="1000"
readonly ADJUSTMENT_RUNNER_RUNTIME_ROOT="/run/user/1000"
readonly ADJUSTMENT_RUNNER_AGENT_SOCKET="/run/user/1000/openssh_agent"
readonly -a ADJUSTMENT_RUNNER_UNIT_FILES=(
  "scripts/systemd/system/weather-adjustment-archive.service"
  "scripts/systemd/system/weather-adjustment-daily.service"
  "scripts/systemd/system/weather-adjustment-daily.timer"
  "scripts/systemd/system/weather-adjustment-monthly.service"
  "scripts/systemd/system/weather-adjustment-monthly.timer"
)
readonly -a ADJUSTMENT_RUNNER_NATIVE_UNIT_FILES=(
  "scripts/systemd/native/weather-adjustment-archive.service"
  "scripts/systemd/native/weather-adjustment-daily.service"
  "scripts/systemd/native/weather-adjustment-daily.timer"
  "scripts/systemd/native/weather-adjustment-monthly.service"
  "scripts/systemd/native/weather-adjustment-monthly.timer"
)
readonly -a ADJUSTMENT_RUNNER_NATIVE_SOURCE_FILES=(
  "deploy/config/ssh_config.example"
  "rust/Cargo.lock"
  "rust/Cargo.toml"
  "rust/crates/weather-runtime/Cargo.toml"
  "rust/crates/weather-runtime/src/bin/weather-maintenance-capture.rs"
  "rust/crates/weather-runtime/src/bin/weather-maintenance-fit.rs"
  "rust/crates/weather-runtime/src/bin/weather-maintenance-wind-fit.rs"
  "rust/rust-toolchain.toml"
  "rust/crates/weather-runtime/src/lib.rs"
  "rust/crates/weather-runtime/src/native_maintenance_candidate_owner.rs"
  "rust/crates/weather-runtime/src/native_maintenance_capture_host_readiness.rs"
  "rust/crates/weather-runtime/src/native_maintenance_capture_native_installation.rs"
  "rust/crates/weather-runtime/src/native_maintenance_fit_candidate.rs"
  "rust/crates/weather-runtime/src/native_maintenance_fit_runtime_readiness.rs"
  "rust/crates/weather-runtime/src/native_maintenance_fit_sandbox.rs"
  "rust/crates/weather-runtime/src/native_maintenance_fit_snapshot.rs"
  "rust/crates/weather-runtime/src/native_maintenance_rain_fit.rs"
  "rust/crates/weather-runtime/src/native_maintenance_temperature_fit.rs"
  "${ADJUSTMENT_RUNNER_NATIVE_UNIT_FILES[@]}"
)
readonly -a ADJUSTMENT_RUNNER_NATIVE_EXECUTABLES=(
  "weather-maintenance-capture"
  "weather-maintenance-fit"
  "weather-maintenance-wind-fit"
)
readonly -a ADJUSTMENT_RUNNER_FILES=(
  "deploy/config/ssh_config.example"
  "deploy/scripts/adjustment-evaluation-package.mjs"
  "deploy/scripts/adjustment-evidence-store.mjs"
  "deploy/scripts/forecast-adjustment-scorecard-contract.mjs"
  "apps/worker/package.json"
  "packages/domain/package.json"
  "packages/forecast-adjustment/package.json"
  "scripts/await-check.mjs"
  "scripts/research/adjustment_archive_job.mjs"
  "scripts/research/adjustment_capture_readiness.mjs"
  "scripts/research/adjustment_confirmation_values.mjs"
  "scripts/research/adjustment_rain_confirmation_values.mjs"
  "scripts/research/adjustment_cycle_pages.mjs"
  "scripts/research/adjustment_fit_inputs.mjs"
  "scripts/research/adjustment_fit_sandbox.mjs"
  "scripts/research/adjustment_historical_archive.mjs"
  "scripts/research/adjustment_historical_fit_assembler.mjs"
  "scripts/research/adjustment_monthly_projection.mjs"
  "scripts/research/adjustment_daily_evaluation.mjs"
  "scripts/research/adjustment_maintenance_capture_job.mjs"
  "scripts/research/adjustment_maintenance_runtime_adapter.mjs"
  "scripts/research/adjustment_maintenance_runtime_manifest.mjs"
  "scripts/research/adjustment_maintenance_controller.mjs"
  "scripts/research/adjustment_maintenance_state.mjs"
  "scripts/research/adjustment_model_release.mjs"
  "scripts/research/adjustment_plaintext_archive.mjs"
  "scripts/research/adjustment_private_directory.mjs"
  "scripts/research/adjustment_rain_control_reference.mjs"
  "scripts/research/adjustment_rain_monthly_projection.mjs"
  "scripts/research/adjustment_rain_model_package.mjs"
  "scripts/research/adjustment_revision_custody_pack.mjs"
  "scripts/research/adjustment_revision_cold_drain.mjs"
  "scripts/research/adjustment_rolling_schedule.mjs"
  "scripts/research/build_moisture_targets.py"
  "scripts/research/build_rain_sub24.py"
  "scripts/research/export_moisture_history.py"
  "scripts/research/export_rain_wind_runtime.py"
  "scripts/research/rain_context.py"
  "scripts/research/rain_context_features.py"
  "scripts/research/rain_event_guard.py"
  "scripts/research/rain_hurdle.py"
  "scripts/research/rain_hurdle_calibration.py"
  "scripts/research/rain_ordinal.py"
  "scripts/research/rain_recency.py"
  "scripts/research/rain_recency_calibration.py"
  "scripts/research/rain_refresh.py"
  "scripts/research/rain_residual.py"
  "scripts/research/rain_search.py"
  "scripts/research/rain_sub24.py"
  "scripts/research/rain_trajectory.py"
  "scripts/research/rain_trajectory_features.py"
  "scripts/research/rain_wind.py"
  "scripts/research/rain_wind_features.py"
  "scripts/research/rain_wind_source.py"
  "scripts/research/retain_moisture_research.py"
  "scripts/research/run_rain_sub24.py"
  "scripts/research/temperature_refresh.py"
  "scripts/research/temperature_seasonal_ridge.py"
  "scripts/research/temperature_shortlead_models.py"
  "scripts/research/adjustment-maintenance-runtime/forecast/package.json"
  "scripts/research/adjustment-maintenance-runtime/forecast/algorithm-v1.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/apply.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/bootstrap-v1.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/calendar.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/candidate.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/evaluate.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/evidence.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/holdout-ledger.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/index.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-capture-epoch.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-policy.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-runtime-package.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-revision-projection.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-shadow-catalog.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-shadow-comparator.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/maintenance-shadow-values.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/performance-scorecard.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/rain-hurdle-wind-artifact.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/rain-hurdle-wind.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/rain-maintenance-controls.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/rain-fixed-gauge-target.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/rain-runtime-registry.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/runtime-bundle.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/runtime-loader.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/temperature-canary.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/temperature-analog-research.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/temperature-analog-shrinkage.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/temperature-analog-stress-validation.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/temperature-analog-validation.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/temperature-causal-guard.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/temperature-frozen-replay.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/temperature-lead-research.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/temperature-live-replay-events.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/temperature-mos-runtime.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/temperature-nowcast-research.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/temperature-weather-research.js"
  "scripts/research/adjustment-maintenance-runtime/forecast/wind-canary.js"
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/package.json"
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/forecast-adjustment.js"
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/forecast-anchor-record.js"
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/index.js"
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/ingestion.js"
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/provenance.js"
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/rain-collection.js"
  "scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/weather-record.js"
  "scripts/research/adjustment-maintenance-runtime/worker/forecast-adjustment-wind-refresh-cli.js"
  "${ADJUSTMENT_RUNNER_UNIT_FILES[@]}"
)

# print one reviewed source manifest
adjustment_runner_manifest_lines() {
  local source_root="$1"
  local generation="${2:-node}"
  local source_real
  local -a reviewed_files=("${ADJUSTMENT_RUNNER_FILES[@]}")
  source_real="$(readlink -f -- "${source_root}")"

  # extend only the explicit native candidate with checked-in build provenance and unit templates
  if [[ "${generation}" == "native" ]]; then
    reviewed_files=("${ADJUSTMENT_RUNNER_NATIVE_SOURCE_FILES[@]}")
  elif [[ "${generation}" != "node" ]]; then
    printf '%s\n' "runner generation is invalid" >&2
    return 1
  fi

  # require a literal owner-controlled checkout root
  if [[ ! -d "${source_root}" || -L "${source_root}" || "${source_real}" != "${source_root}" ||
    "$(stat -c '%u' -- "${source_root}")" != "$(id -u)" ]]; then
    printf '%s\n' "source root is invalid" >&2
    return 1
  fi

  local relative
  # hash only the closed public-code allowlist
  for relative in "${reviewed_files[@]}"; do
    local path="${source_root}/${relative}"
    local resolved
    resolved="$(readlink -f -- "${path}")"
    # prohibit missing, linked, foreign or writable reviewed members
    if [[ ! -f "${path}" || -L "${path}" || "${resolved}" != "${path}" ||
      "$(stat -c '%u' -- "${path}")" != "$(id -u)" ||
      $((8#$(stat -c '%a' -- "${path}") & 8#022)) -ne 0 ]]; then
      printf 'reviewed source member is not a regular owner file: %s\n' "${relative}" >&2
      return 1
    fi
    printf '%s  %s\n' "$(sha256sum -- "${path}" | cut -d' ' -f1)" "${relative}"
  done
}

# print the closed source manifest identity
adjustment_runner_manifest_digest() {
  local source_root="$1"
  local generation="${2:-node}"
  adjustment_runner_manifest_lines "${source_root}" "${generation}" | sha256sum | cut -d' ' -f1
}

# bind every reviewed member to one literal source commit
require_source_commit() {
  local source_root="$1"
  local source_commit="$2"
  local generation="${3:-node}"
  local -a reviewed_files=("${ADJUSTMENT_RUNNER_FILES[@]}")
  # add native source and unit provenance only for an explicit candidate install
  if [[ "${generation}" == "native" ]]; then
    reviewed_files=("${ADJUSTMENT_RUNNER_NATIVE_SOURCE_FILES[@]}")
  elif [[ "${generation}" != "node" ]]; then
    printf '%s\n' "runner generation is invalid" >&2
    return 1
  fi
  # require one full sha-1 commit matching the checkout head
  if [[ ! "${source_commit}" =~ ^[a-f0-9]{40}$ ||
    "$(/usr/bin/git -C "${source_root}" rev-parse --verify HEAD)" != "${source_commit}" ||
    "$(/usr/bin/git -C "${source_root}" rev-parse --verify "${source_commit}^{commit}")" != \
      "${source_commit}" ]]; then
    printf '%s\n' "source commit identity is invalid" >&2
    return 1
  fi
  # require the whole tracked Rust build tree to match the committed native source
  if [[ "${generation}" == "native" ]]; then
    local untracked_rust
    # refuse tracked or untracked build source drift
    if ! /usr/bin/git -C "${source_root}" diff --quiet "${source_commit}" -- rust ||
      ! untracked_rust="$(/usr/bin/git -C "${source_root}" ls-files --others --exclude-standard -- rust)" ||
      [[ -n "${untracked_rust}" ]]; then
      printf '%s\n' "native Rust build source differs from source commit" >&2
      return 1
    fi
  fi
  local relative
  # compare every installed byte to the immutable commit object
  for relative in "${reviewed_files[@]}"; do
    local committed_sha256
    if ! committed_sha256="$(/usr/bin/git -C "${source_root}" show \
      "${source_commit}:${relative}" | sha256sum | cut -d' ' -f1)"; then
      printf 'source commit member is unavailable: %s\n' "${relative}" >&2
      return 1
    fi
    # reject dirty, untracked or substituted checkout members
    if [[ "${committed_sha256}" != \
      "$(sha256sum -- "${source_root}/${relative}" | cut -d' ' -f1)" ]]; then
      printf 'source commit member differs: %s\n' "${relative}" >&2
      return 1
    fi
  done
}

# print the exact three-file native binary manifest from one controlled build directory
adjustment_native_binary_manifest_lines() {
  local binary_root="$1"
  local binary_real
  binary_real="$(readlink -f -- "${binary_root}")"
  # prohibit linked, foreign or externally writable build roots
  if [[ ! -d "${binary_root}" || -L "${binary_root}" || "${binary_real}" != "${binary_root}" ||
    "$(stat -c '%u' -- "${binary_root}")" != "$(id -u)" ||
    $((8#$(stat -c '%a' -- "${binary_root}") & 8#022)) -ne 0 ]]; then
    printf '%s\n' "native binary root is invalid" >&2
    return 1
  fi
  local entry_count
  entry_count="$(find "${binary_root}" -mindepth 1 -maxdepth 1 -printf '.' | wc -c)"
  # refuse hidden receipts, nested directories and every undeclared build artifact
  if [[ "${entry_count}" -ne "${#ADJUSTMENT_RUNNER_NATIVE_EXECUTABLES[@]}" ]]; then
    printf '%s\n' "native binary inventory differs" >&2
    return 1
  fi
  local executable
  # bind every executable byte to its fixed installed path
  for executable in "${ADJUSTMENT_RUNNER_NATIVE_EXECUTABLES[@]}"; do
    local path="${binary_root}/${executable}"
    local resolved
    local mode
    resolved="$(readlink -f -- "${path}")"
    mode="$(stat -c '%a' -- "${path}" 2>/dev/null || true)"
    # require a literal owner executable without group or other write authority
    if [[ ! -f "${path}" || -L "${path}" || "${resolved}" != "${path}" ||
      "$(stat -c '%u' -- "${path}")" != "$(id -u)" ||
      -z "${mode}" || $((8#${mode} & 8#100)) -eq 0 || $((8#${mode} & 8#022)) -ne 0 ]]; then
      printf 'native executable is invalid: %s\n' "${executable}" >&2
      return 1
    fi
    printf '%s  bin/%s\n' "$(sha256sum -- "${path}" | cut -d' ' -f1)" "${executable}"
  done
}

# print the closed native binary manifest identity
adjustment_native_binary_manifest_digest() {
  local binary_root="$1"
  adjustment_native_binary_manifest_lines "${binary_root}" | sha256sum | cut -d' ' -f1
}

# require one literal owner-controlled directory
adjustment_require_controlled_directory() {
  local path="$1"
  local privacy="$2"
  local mode
  # reject missing, linked, foreign or path-drifted directories
  if [[ ! -d "${path}" || -L "${path}" || "$(readlink -f -- "${path}")" != "${path}" ||
    "$(stat -c '%u' -- "${path}")" != "$(id -u)" ]]; then
    printf 'controlled directory is invalid: %s\n' "${path}" >&2
    return 1
  fi
  mode="$(stat -c '%a' -- "${path}")"
  # require exact privacy for dedicated roots and no external writes elsewhere
  if [[ "${privacy}" == "private" ]]; then
    if [[ "${mode}" != "700" ]]; then
      printf 'private directory mode is invalid: %s\n' "${path}" >&2
      return 1
    fi
  elif (( (8#${mode} & 8#022) != 0 )); then
    printf 'controlled directory is externally writable: %s\n' "${path}" >&2
    return 1
  fi
}

# require every private runtime ancestor below home to reject broad parent drift
require_private_install_ancestors() {
  local target="$1"
  # prohibit targets outside the fixed current-user home
  if [[ "${target}" != "${HOME}"/* ]]; then
    printf 'install ancestor is outside home: %s\n' "${target}" >&2
    return 1
  fi
  adjustment_require_controlled_directory "${HOME}" "controlled"
  local current="${HOME}"
  local suffix="${target#"${HOME}"/}"
  local segment
  local -a segments=()
  IFS='/' read -r -a segments <<<"${suffix}"
  # inspect every literal ancestor without repairing existing permissions
  for segment in "${segments[@]}"; do
    current="${current}/${segment}"
    # create only the dedicated terminal parent after its ancestors pass
    if [[ ! -e "${current}" && "${current}" == "${target}" ]]; then
      mkdir -m 0700 -- "${current}"
    fi
    # require exact privacy for weather state and the dedicated parent
    if [[ "${current}" == "${HOME}/.weather" || "${current}" == "${target}" ]]; then
      adjustment_require_controlled_directory "${current}" "private"
    else
      adjustment_require_controlled_directory "${current}" "controlled"
    fi
  done
}

# require one root-owned public system-unit directory without repairing it
adjustment_require_public_unit_root() {
  local target="$1"
  # require one absolute canonical production directory
  if [[ "${target}" != /* || ! -d "${target}" || -L "${target}" ||
    "$(readlink -f -- "${target}")" != "${target}" ]]; then
    printf 'public unit directory is invalid: %s\n' "${target}" >&2
    return 1
  fi
  local current=""
  local segment
  local -a segments=()
  IFS='/' read -r -a segments <<<"${target#/}"
  # bind every literal ancestor to root ownership and closed writes
  for segment in "${segments[@]}"; do
    current="${current}/${segment}"
    local mode
    # reject links, foreign ownership and external writes without mutation
    if [[ ! -d "${current}" || -L "${current}" ||
      "$(stat -c '%u:%g' -- "${current}")" != "0:0" ]]; then
      printf 'public unit directory is invalid: %s\n' "${current}" >&2
      return 1
    fi
    mode="$(stat -c '%a' -- "${current}")"
    # reject group or other write authority
    if (( (8#${mode} & 8#022) != 0 )); then
      printf 'public unit directory is externally writable: %s\n' "${current}" >&2
      return 1
    fi
  done
}

# require one immutable root-owned public system unit
adjustment_require_public_unit_file() {
  local path="$1"
  # reject links, foreign ownership, broad modes and path substitution
  if [[ ! -f "${path}" || -L "${path}" || "$(readlink -f -- "${path}")" != "${path}" ||
    "$(stat -c '%u:%g:%a:%h' -- "${path}")" != "0:0:644:1" ]]; then
    printf 'public unit file is invalid: %s\n' "${path}" >&2
    return 1
  fi
}

# expose one no-op copy verification fault hook
runner_after_copy() {
  :
}

# expose one no-op unit installation fault hook
runner_before_unit_install() {
  :
}

# install one root-owned public file through the fixed privilege boundary
adjustment_public_install() {
  /usr/bin/sudo -- /usr/bin/install -o root -g root -m 0644 -- "$1" "$2"
}

# publish one root-owned public file through the fixed privilege boundary
adjustment_public_move() {
  /usr/bin/sudo -- /usr/bin/mv -Tf -- "$1" "$2"
}

# remove one named public transaction file through the fixed privilege boundary
adjustment_public_remove() {
  /usr/bin/sudo -- /usr/bin/rm -f -- "$1"
}

# atomically install the public unit from one frozen release
install_runner_unit() {
  local source="$1"
  local destination="$2"
  local temporary="${destination}.incoming-$$"
  runner_before_unit_install "${source}" "${destination}" || return $?
  adjustment_public_install "${source}" "${temporary}" || return $?
  adjustment_public_move "${temporary}" "${destination}" || return $?
  adjustment_require_public_unit_file "${destination}"
}

# restore one public unit without invoking the installation fault hook
restore_runner_unit() {
  local source="$1"
  local destination="$2"
  local temporary="${destination}.incoming-$$"
  adjustment_public_install "${source}" "${temporary}" || return $?
  adjustment_public_move "${temporary}" "${destination}" || return $?
  adjustment_require_public_unit_file "${destination}"
}

# install every reviewed public unit from one frozen release
install_runner_units() {
  local release_root="$1"
  local unit_root="$2"
  local generation="${3:-node}"
  local -a unit_files=("${ADJUSTMENT_RUNNER_UNIT_FILES[@]}")
  # select native templates only for the explicit native generation
  if [[ "${generation}" == "native" ]]; then
    unit_files=("${ADJUSTMENT_RUNNER_NATIVE_UNIT_FILES[@]}")
  fi
  local relative
  # publish the closed unit set before one manager reload
  for relative in "${unit_files[@]}"; do
    install_runner_unit \
      "${release_root}/${relative}" "${unit_root}/$(basename -- "${relative}")"
  done
}

# require every reviewed public unit to be loaded from its root-owned fragment
require_loaded_runner_units() {
  local unit_root="$1"
  local generation="${2:-node}"
  local -a unit_files=("${ADJUSTMENT_RUNNER_UNIT_FILES[@]}")
  # inspect the same generation-specific unit set that was just installed
  if [[ "${generation}" == "native" ]]; then
    unit_files=("${ADJUSTMENT_RUNNER_NATIVE_UNIT_FILES[@]}")
  fi
  local relative
  # bind the closed unit set after one manager reload
  for relative in "${unit_files[@]}"; do
    require_loaded_public_unit "${unit_root}/$(basename -- "${relative}")"
  done
}

# require the fixed unprivileged runtime identity and active user manager
require_user_manager_readiness() {
  # bind the fixed unit identity to the existing local account
  if [[ "$(id -u "${ADJUSTMENT_RUNNER_USER}")" != "${ADJUSTMENT_RUNNER_UID}" ||
    "$(id -g "${ADJUSTMENT_RUNNER_USER}")" != "${ADJUSTMENT_RUNNER_GID}" ||
    "$(id -u)" != "${ADJUSTMENT_RUNNER_UID}" || "${HOME}" != "/home/ubuntu" ]]; then
    printf '%s\n' "adjustment runner identity is unproven" >&2
    return 1
  fi
  # require one literal owner-private runtime directory and owner agent socket
  if [[ ! -d "${ADJUSTMENT_RUNNER_RUNTIME_ROOT}" ||
    -L "${ADJUSTMENT_RUNNER_RUNTIME_ROOT}" ||
    "$(readlink -f -- "${ADJUSTMENT_RUNNER_RUNTIME_ROOT}")" != \
      "${ADJUSTMENT_RUNNER_RUNTIME_ROOT}" ||
    "$(stat -c '%u:%g:%a' -- "${ADJUSTMENT_RUNNER_RUNTIME_ROOT}")" != \
      "${ADJUSTMENT_RUNNER_UID}:${ADJUSTMENT_RUNNER_GID}:700" ||
    ! -S "${ADJUSTMENT_RUNNER_RUNTIME_ROOT}/bus" ||
    "$(stat -c '%u:%g' -- "${ADJUSTMENT_RUNNER_RUNTIME_ROOT}/bus")" != \
      "${ADJUSTMENT_RUNNER_UID}:${ADJUSTMENT_RUNNER_GID}" ||
    ! -S "${ADJUSTMENT_RUNNER_AGENT_SOCKET}" ||
    "$(stat -c '%u:%g' -- "${ADJUSTMENT_RUNNER_AGENT_SOCKET}")" != \
      "${ADJUSTMENT_RUNNER_UID}:${ADJUSTMENT_RUNNER_GID}" ]]; then
    printf '%s\n' "adjustment user runtime is unproven" >&2
    return 1
  fi
  # require both managers and the user bus to be active before installation
  if ! /usr/bin/systemctl is-active --quiet "user@${ADJUSTMENT_RUNNER_UID}.service" ||
    ! XDG_RUNTIME_DIR="${ADJUSTMENT_RUNNER_RUNTIME_ROOT}" \
      /usr/bin/systemctl --user is-active --quiet dbus.socket; then
    printf '%s\n' "adjustment user manager is unavailable" >&2
    return 1
  fi
}

# require the pinned runtime and installed capture readiness
require_runtime_readiness() {
  local release_root="$1"
  local generation="${2:-node}"
  local ssh_config="${release_root}/deploy/config/ssh_config.example"
  # require the common fixed host executables and public SSH configuration
  if [[ ! -x /usr/bin/ssh || ! -x /usr/bin/flock ||
    ! -f "${ssh_config}" || -L "${ssh_config}" ]]; then
    printf '%s\n' "adjustment archive runtime readiness is unproven" >&2
    return 1
  fi
  require_user_manager_readiness
  # prove the selected installed closure and every read-only capture authority
  if [[ "${generation}" == "native" ]]; then
    "${release_root}/bin/weather-maintenance-capture" --readiness >/dev/null
  elif [[ "${generation}" == "node" && -x "${ADJUSTMENT_RUNNER_NODE}" &&
    "$("${ADJUSTMENT_RUNNER_NODE}" --version)" == "v24.16.0" ]]; then
    "${ADJUSTMENT_RUNNER_NODE}" \
      "${release_root}/scripts/research/adjustment_maintenance_capture_job.mjs" \
        --readiness >/dev/null
  else
    printf '%s\n' "adjustment archive runtime readiness is unproven" >&2
    return 1
  fi
}

# reload only the root-owned system unit definitions
reload_systemd() {
  /usr/bin/sudo -- /usr/bin/systemctl daemon-reload
}

# require the system manager to load the root-owned public fragment
require_loaded_public_unit() {
  local unit_file="$1"
  local fragment
  fragment="$(/usr/bin/systemctl show \
    --property=FragmentPath --value "$(basename -- "${unit_file}")")"
  # reject missing, shadowed or path-normalized manager state
  if [[ "${fragment}" != "${unit_file}" ]]; then
    printf 'loaded public unit differs: %s\n' "${fragment}" >&2
    return 1
  fi
}

# install one immutable disabled runner release
install_adjustment_maintenance_runner() {
  local source_root="$1"
  local source_commit="$2"
  local expected_manifest_sha256="$3"
  local install_root="$4"
  local unit_root="$5"
  local generation="${6:-node}"
  local native_binary_root="${7:-}"
  local expected_native_binary_manifest_sha256="${8:-}"
  local source_manifest_sha256
  local -a reviewed_files=("${ADJUSTMENT_RUNNER_FILES[@]}")
  local -a unit_files=("${ADJUSTMENT_RUNNER_UNIT_FILES[@]}")

  # select native source, unit and binary inputs only through the explicit candidate mode
  if [[ "${generation}" == "native" ]]; then
    reviewed_files=("${ADJUSTMENT_RUNNER_NATIVE_SOURCE_FILES[@]}")
    unit_files=("${ADJUSTMENT_RUNNER_NATIVE_UNIT_FILES[@]}")
  elif [[ "${generation}" != "node" || -n "${native_binary_root}" ||
    -n "${expected_native_binary_manifest_sha256}" ]]; then
    printf '%s\n' "runner generation is invalid" >&2
    return 1
  fi
  # reject caller-selected identities outside lowercase sha-256
  if [[ ! "${expected_manifest_sha256}" =~ ^[a-f0-9]{64}$ ||
    ( "${generation}" == "native" &&
      ! "${expected_native_binary_manifest_sha256}" =~ ^[a-f0-9]{64}$ ) ]]; then
    printf '%s\n' "expected source manifest is invalid" >&2
    return 1
  fi
  require_source_commit "${source_root}" "${source_commit}" "${generation}"
  source_manifest_sha256="$(adjustment_runner_manifest_digest "${source_root}" "${generation}")"
  # stop source drift before any installation mutation
  if [[ "${source_manifest_sha256}" != "${expected_manifest_sha256}" ]]; then
    printf '%s\n' "source manifest differs" >&2
    return 1
  fi
  # bind native build output before creating any private installation directory
  if [[ "${generation}" == "native" &&
    "$(adjustment_native_binary_manifest_digest "${native_binary_root}")" != \
      "${expected_native_binary_manifest_sha256}" ]]; then
    printf '%s\n' "native binary manifest differs" >&2
    return 1
  fi
  local releases_root="${install_root}/releases"
  local release_root="${releases_root}/${source_commit}"
  local incoming_root="${releases_root}/.incoming-${source_commit}-$$"
  local primary_unit_file="${unit_root}/weather-adjustment-archive.service"
  local current_link="${install_root}/current"
  local current_temporary="${install_root}/.current-$$"
  local transaction_root="${install_root}/.transaction-$$"
  local install_parent
  install_parent="$(dirname -- "${install_root}")"

  umask 0077
  require_private_install_ancestors "${install_parent}"
  adjustment_require_public_unit_root "${unit_root}"
  local relative
  adjustment_require_controlled_directory "${install_parent}" "controlled"
  # create only a missing dedicated root under its already proven parent
  if [[ ! -e "${install_root}" ]]; then
    mkdir -m 0700 -- "${install_root}"
  fi
  adjustment_require_controlled_directory "${install_root}" "private"
  # create only a missing dedicated releases child without permission repair
  if [[ ! -e "${releases_root}" ]]; then
    mkdir -m 0700 -- "${releases_root}"
  fi
  adjustment_require_controlled_directory "${releases_root}" "private"
  # reject replacement of a non-link release pointer
  if [[ -e "${current_link}" && ! -L "${current_link}" ]]; then
    printf '%s\n' "current release pointer is invalid" >&2
    return 1
  fi

  local release_created="false"
  # publish only when the immutable release is absent
  if [[ ! -e "${release_root}" && ! -L "${release_root}" ]]; then
    mkdir -m 0700 -- "${incoming_root}"
    # copy every verified member into a regular-file-only layout
    for relative in "${reviewed_files[@]}"; do
      mkdir -p -- "${incoming_root}/$(dirname -- "${relative}")"
      install -m 0400 -- "${source_root}/${relative}" "${incoming_root}/${relative}"
      runner_after_copy "${relative}" "${incoming_root}" "${source_root}"
    done
    # reject any source race by hashing only the copied frozen bytes
    if [[ "$(adjustment_runner_manifest_digest "${incoming_root}" "${generation}")" != \
      "${expected_manifest_sha256}" ]]; then
      chmod -R u+w -- "${incoming_root}"
      rm -rf -- "${incoming_root}"
      printf '%s\n' "copied release manifest differs" >&2
      return 1
    fi
    # add only digest-bound native executables after all checked-in source bytes are frozen
    if [[ "${generation}" == "native" ]]; then
      mkdir -m 0700 -- "${incoming_root}/bin"
      local executable
      for executable in "${ADJUSTMENT_RUNNER_NATIVE_EXECUTABLES[@]}"; do
        install -m 0500 -- \
          "${native_binary_root}/${executable}" "${incoming_root}/bin/${executable}"
      done
      # re-hash installed bytes so a build-directory race cannot enter the release
      if [[ "$(adjustment_native_binary_manifest_digest "${incoming_root}/bin")" != \
        "${expected_native_binary_manifest_sha256}" ]]; then
        chmod -R u+w -- "${incoming_root}"
        rm -rf -- "${incoming_root}"
        printf '%s\n' "copied native binary manifest differs" >&2
        return 1
      fi
    fi
    {
      adjustment_runner_manifest_lines "${incoming_root}" "${generation}"
      # append native executable identities only for the explicit candidate generation
      if [[ "${generation}" == "native" ]]; then
        adjustment_native_binary_manifest_lines "${incoming_root}/bin"
      fi
    } >"${incoming_root}/MANIFEST.sha256"
    chmod 0400 -- "${incoming_root}/MANIFEST.sha256"
    # freeze every release directory after all files exist
    while IFS= read -r directory; do
      chmod 0500 -- "${directory}"
    done < <(find "${incoming_root}" -type d -print)
    mv -- "${incoming_root}" "${release_root}"
    release_created="true"
  else
    adjustment_require_controlled_directory "${release_root}" "controlled"
    # reuse only the exact immutable release bytes
    if [[ "$(adjustment_runner_manifest_digest "${release_root}" "${generation}")" != \
      "${expected_manifest_sha256}" ]]; then
      printf '%s\n' "installed release manifest differs" >&2
      return 1
    fi
    # require a reused native release to retain the same exact executable closure
    if [[ "${generation}" == "native" &&
      "$(adjustment_native_binary_manifest_digest "${release_root}/bin")" != \
        "${expected_native_binary_manifest_sha256}" ]]; then
      printf '%s\n' "installed native binary manifest differs" >&2
      return 1
    fi
  fi

  # run readiness only from the frozen installed release before public mutation
  if ! require_runtime_readiness "${release_root}" "${generation}"; then
    printf '%s\n' "adjustment capture readiness is unproven" >&2
    # remove only the unpublished release created by this transaction
    if [[ "${release_created}" == "true" ]]; then
      chmod -R u+w -- "${release_root}"
      rm -rf -- "${release_root}"
    fi
    return 1
  fi

  mkdir -m 0700 -- "${transaction_root}"
  local previous_current="absent"
  # retain one validated prior release pointer
  if [[ -L "${current_link}" ]]; then
    previous_current="$(readlink -- "${current_link}")"
    if [[ ! "${previous_current}" =~ ^releases/[a-f0-9]{40}$ ]]; then
      printf '%s\n' "current release target is invalid" >&2
      rm -rf -- "${transaction_root}"
      # remove only the unpublished release created by this transaction
      if [[ "${release_created}" == "true" ]]; then
        chmod -R u+w -- "${release_root}"
        rm -rf -- "${release_root}"
      fi
      return 1
    fi
  fi
  local -A previous_units=()
  local unit_file
  local unit_name
  # retain every root-owned public unit for rollback
  for relative in "${unit_files[@]}"; do
    unit_name="$(basename -- "${relative}")"
    unit_file="${unit_root}/${unit_name}"
    previous_units["${unit_name}"]="absent"
    # snapshot only a valid existing public unit
    if [[ -e "${unit_file}" || -L "${unit_file}" ]]; then
      if ! adjustment_require_public_unit_file "${unit_file}"; then
        printf 'existing runner unit is invalid: %s\n' "${unit_name}" >&2
        rm -rf -- "${transaction_root}"
        # remove only the unpublished release created by this transaction
        if [[ "${release_created}" == "true" ]]; then
          chmod -R u+w -- "${release_root}"
          rm -rf -- "${release_root}"
        fi
        return 1
      fi
      cp -- "${unit_file}" "${transaction_root}/previous-${unit_name}"
      chmod 0600 -- "${transaction_root}/previous-${unit_name}"
      previous_units["${unit_name}"]="present"
    fi
  done

  local activation_status=0
  # activate the pointer and unit as one rollback-capable transaction
  if ln -s -- "releases/${source_commit}" "${current_temporary}" &&
    mv -Tf -- "${current_temporary}" "${current_link}" &&
    install_runner_units "${release_root}" "${unit_root}" "${generation}" &&
    reload_systemd \
      "${source_root}" "${expected_manifest_sha256}" "${install_root}" \
      "${release_root}" "${primary_unit_file}" "${unit_root}" &&
    require_loaded_runner_units "${unit_root}" "${generation}"; then
    activation_status=0
  else
    activation_status=$?
  fi

  # restore both activation files after any install or reload failure
  if [[ "${activation_status}" -ne 0 ]]; then
    rm -f -- "${current_temporary}"
    # clear only this transaction's named public temporary files
    for relative in "${unit_files[@]}"; do
      adjustment_public_remove \
        "${unit_root}/$(basename -- "${relative}").incoming-$$" >/dev/null 2>&1 || true
    done
    if [[ "${previous_current}" == "absent" ]]; then
      rm -f -- "${current_link}"
    else
      ln -s -- "${previous_current}" "${current_temporary}"
      mv -Tf -- "${current_temporary}" "${current_link}"
    fi
    # restore the exact prior state of every public unit
    for relative in "${unit_files[@]}"; do
      unit_name="$(basename -- "${relative}")"
      unit_file="${unit_root}/${unit_name}"
      if [[ "${previous_units["${unit_name}"]}" == "absent" ]]; then
        adjustment_public_remove "${unit_file}"
      else
        restore_runner_unit \
          "${transaction_root}/previous-${unit_name}" "${unit_file}"
      fi
    done
    reload_systemd \
      "${source_root}" "${expected_manifest_sha256}" "${install_root}" \
      "${release_root}" "${primary_unit_file}" "${unit_root}" >/dev/null 2>&1 || true
    rm -rf -- "${transaction_root}"
    # remove only the release created and rolled back by this transaction
    if [[ "${release_created}" == "true" ]]; then
      chmod -R u+w -- "${release_root}"
      rm -rf -- "${release_root}"
    fi
    return "${activation_status}"
  fi
  rm -rf -- "${transaction_root}"
}

# execute only the fixed production install surface
main() {
  local generation="node"
  local source_commit
  local native_binary_root=""
  local native_binary_manifest_sha256=""
  # preserve the predecessor default and admit native only through the explicit four-operand mode
  if [[ "$#" -eq 1 && "$1" != "--native" ]]; then
    source_commit="$1"
  elif [[ "$#" -eq 4 && "$1" == "--native" ]]; then
    generation="native"
    source_commit="$2"
    native_binary_root="$3"
    native_binary_manifest_sha256="$4"
  else
    printf 'usage: %s SOURCE_COMMIT | --native SOURCE_COMMIT BINARY_DIR BINARY_MANIFEST_SHA256\n' \
      "$0" >&2
    return 2
  fi
  local source_root
  source_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
  require_source_commit "${source_root}" "${source_commit}" "${generation}"
  local manifest_sha256
  manifest_sha256="$(adjustment_runner_manifest_digest "${source_root}" "${generation}")"
  install_adjustment_maintenance_runner \
    "${source_root}" "${source_commit}" "${manifest_sha256}" \
    "${ADJUSTMENT_RUNNER_INSTALL_ROOT}" "${ADJUSTMENT_RUNNER_UNIT_ROOT}" \
    "${generation}" "${native_binary_root}" "${native_binary_manifest_sha256}"
}

# avoid execution when regression tests source this file
if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  main "$@"
fi
