#!/usr/bin/bash

set -euo pipefail

readonly INSTALL_ROOT="/var/lib/weather/maintenance/page-read-index-v1"
readonly RELEASES_ROOT="${INSTALL_ROOT}/releases"
readonly RESTORE_CONTEXTS_ROOT="${INSTALL_ROOT}/restore-contexts"
readonly PYTHON="/usr/bin/python3"
readonly PACKAGE_LIMIT=$((1024 * 1024))
readonly RESTORE_LIMIT=$((64 * 1024 * 1024))
readonly SHA256_PATTERN='^[a-f0-9]{64}$'
readonly SHA1_PATTERN='^[a-f0-9]{40}$'
readonly RELEASE_PATTERN='^([0-9]{4})\.([0-9]{2})\.([0-9]{2})-([1-9][0-9]?)$'

stage_root=""
context_stage_root=""

# print one fatal error
fail() {
  printf 'install-weather-page-indexes: %s\n' "$1" >&2
  exit 1
}

# validate one sha-256
require_sha256() {
  local value="$1"

  # reject malformed trusted hashes
  if [[ ! "${value}" =~ ${SHA256_PATTERN} ]]; then
    fail "expected SHA-256 is invalid"
  fi
}

# validate release identity
require_release_identity() {
  local release="$1"
  local commit="$2"

  # reject unsupported release tags
  if [[ ! "${release}" =~ ${RELEASE_PATTERN} ]]; then
    fail "release tag must use YYYY.MM.DD-N"
  fi
  local release_date="${BASH_REMATCH[1]}-${BASH_REMATCH[2]}-${BASH_REMATCH[3]}"

  # reject normalized invalid dates
  if [[ "$(/usr/bin/date --date "${release_date}" +%Y-%m-%d 2>/dev/null || true)" != \
    "${release_date}" ]]; then
    fail "release tag contains an invalid date"
  fi

  # reject malformed commits
  if [[ ! "${commit}" =~ ${SHA1_PATTERN} ]]; then
    fail "release commit is invalid"
  fi
}

# resolve one literal path
canonical_path() {
  /usr/bin/readlink -f -- "$1"
}

# require one trusted directory
require_root_directory() {
  local path="$1"
  local exact_mode="${2:-}"
  local resolved
  resolved="$(canonical_path "${path}")"

  # reject linked, foreign or path-drifted roots
  if [[ ! -d "${path}" || -L "${path}" || "${resolved}" != "${path}" || \
    "$(/usr/bin/stat -c '%u' -- "${path}")" != "0" ]]; then
    fail "trusted directory is invalid: ${path}"
  fi
  local mode
  mode="$(/usr/bin/stat -c '%a' -- "${path}")"

  # require exact private mode when declared
  if [[ -n "${exact_mode}" ]]; then
    if [[ "${mode}" != "${exact_mode}" ]]; then
      fail "trusted directory mode is invalid: ${path}"
    fi
  elif (( (8#${mode} & 8#022) != 0 )); then
    fail "trusted directory is externally writable: ${path}"
  fi
}

# require trusted fixed ancestry
require_install_ancestry() {
  local path

  # verify every fixed administrator ancestor
  for path in / /usr /usr/bin /var /var/lib /var/lib/weather /var/lib/weather/maintenance; do
    require_root_directory "${path}"
  done
  require_root_directory "${INSTALL_ROOT}" "700"
  local python_resolved
  python_resolved="$(canonical_path "${PYTHON}")"

  # reject a substituted root interpreter
  if [[ ! -f "${PYTHON}" || "$(/usr/bin/stat -c '%u' -- "${PYTHON}")" != "0" || \
    ! -f "${python_resolved}" || "$(/usr/bin/stat -c '%u' -- "${python_resolved}")" != "0" || \
    $((8#$(/usr/bin/stat -c '%a' -- "${python_resolved}") & 8#022)) -ne 0 ]]; then
    fail "system Python is not trusted"
  fi
}

# require one private incoming file
require_incoming_file() {
  local path="$1"
  local maximum="$2"
  local resolved
  resolved="$(canonical_path "${path}")"

  # reject links, aliases, hardlinks and foreign bytes
  if [[ ! -f "${path}" || -L "${path}" || "${resolved}" != "${path}" || \
    "$(/usr/bin/stat -c '%u' -- "${path}")" != "0" || \
    "$(/usr/bin/stat -c '%a' -- "${path}")" != "600" || \
    "$(/usr/bin/stat -c '%h' -- "${path}")" != "1" ]]; then
    fail "incoming file is not private root-owned regular data: ${path}"
  fi
  local size
  size="$(/usr/bin/stat -c '%s' -- "${path}")"

  # enforce finite source bytes
  if (( size <= 0 || size > maximum )); then
    fail "incoming file size is invalid: ${path}"
  fi
}

# bind one incoming inode
file_identity() {
  /usr/bin/stat -c '%d:%i:%s:%Y' -- "$1"
}

# verify one expected hash
verify_hash() {
  local path="$1"
  local expected="$2"
  local actual
  actual="$(/usr/bin/sha256sum -- "${path}" | /usr/bin/cut -d' ' -f1)"

  # reject substituted bytes
  if [[ "${actual}" != "${expected}" ]]; then
    fail "SHA-256 differs: ${path}"
  fi
}

# clean one exact owned stage
cleanup_owned_stage() {
  local path="$1"
  local parent="$2"

  # retain absent or published stages
  if [[ -z "${path}" || ! -e "${path}" ]]; then
    return
  fi

  # restrict recursive cleanup
  if [[ "${path}" != "${parent}"/.incoming-* || \
    -L "${path}" || "$(canonical_path "${path}")" != "${path}" || \
    "$(/usr/bin/stat -c '%u' -- "${path}")" != "0" ]]; then
    printf '%s\n' "install-weather-page-indexes: retained untrusted stage" >&2
    return
  fi
  /usr/bin/rm -rf --one-file-system -- "${path}"
  /usr/bin/sync -f -- "${parent}"
}

# clean only owned stages
cleanup_stage() {
  cleanup_owned_stage "${stage_root}" "${RELEASES_ROOT}"
  cleanup_owned_stage "${context_stage_root}" "${RESTORE_CONTEXTS_ROOT}"
}

trap cleanup_stage EXIT

# verify and extract finite archives
verify_and_extract() {
  local action="$1"
  local archive="$2"
  local archive_sha256="$3"
  local restore_archive="$4"
  local restore_sha256="$5"
  local release="$6"
  local commit="$7"
  local destination="$8"
  local restore_destination="${9}"

  "${PYTHON}" - "${action}" "${archive}" "${archive_sha256}" \
    "${restore_archive}" "${restore_sha256}" "${release}" "${commit}" \
    "${destination}" "${restore_destination}" <<'PYTHON'
import gzip
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import sys

PACKAGE_MEMBER_LIMIT = 256 * 1024
PACKAGE_DECODED_LIMIT = 768 * 1024
PACKAGE_ARCHIVE_LIMIT = 1024 * 1024
RESTORE_DECODED_LIMIT = 64 * 1024 * 1024
RESTORE_ARCHIVE_LIMIT = 64 * 1024 * 1024
RESTORE_FILE_LIMIT = 2048
SHA1_LENGTH = 40
SHA256_LENGTH = 64
B1_REPLACEMENT_SHA256 = "ac165391de09e1490a5832108fd54bba2dda97bf67a6225377c2a2e8e5e91c82"
PACKAGE_NAMES = [
    "release-manifest.json",
    "weather-page-indexes.json",
    "weather-page-indexes.mjs",
]
RELEASE_FILE_NAMES = [
    "weather-page-indexes.mjs",
    "weather-page-indexes.json",
]
FROZEN_RESTORE = {
    "repository": "anstosa/weather",
    "tag": "2026.10.09-1",
    "commit": "56b327d9c750946f6f6963b6fe1fa5c9bba791ca",
    "tree": "ecd9599ebf4907c67b912c742eb8fc3d8e6ee8df",
    "fileCount": 813,
    "decodedBytes": 17_238_740,
}
MIGRATIONS = [
    "packages/database/migrations/0001_initial_weather.sql",
    "packages/database/migrations/0002_worker_migration_readiness.sql",
    "packages/database/migrations/0003_ecowitt_measurements.sql",
    "packages/database/migrations/0004_tempest_metadata.sql",
    "packages/database/migrations/0005_source_supersession.sql",
    "packages/database/migrations/0006_station_coordinates.sql",
    "packages/database/migrations/0007_tide_sources.sql",
    "packages/database/migrations/0008_ecowitt_property_sensors.sql",
    "packages/database/migrations/0009_forecast_anchor_records.sql",
    "packages/database/migrations/0010_forecast_training_export.sql",
    "packages/database/migrations/0011_forecast_runtime_provenance.sql",
    "packages/database/migrations/0012_hide_archive_only_forecasts_from_live_reads.sql",
    "packages/database/migrations/0013_ecmwf_temperature_canary.sql",
    "packages/database/migrations/0014_rain_collection.sql",
    "packages/database/migrations/0015_rain_station_access.sql",
    "packages/database/migrations/0016_rain_adjustment.sql",
    "packages/database/migrations/0017_adjustment_evaluation_export.sql",
    "packages/database/migrations/0018_adjustment_maintenance_v2.sql",
]
CRITICAL = {
    "restore": "deploy/scripts/restore.sh",
    "common": "deploy/scripts/common.sh",
    "compose": "deploy/compose.yaml",
    "acl": "deploy/postgres/runtime-acl-v2.sql",
}


# stop with one bounded error
def fail(message):
    raise ValueError(message)


# hash finite bytes
def digest(data):
    return hashlib.sha256(data).hexdigest()


# validate lowercase hexadecimal
def hexadecimal(value, length):
    return (
        isinstance(value, str)
        and len(value) == length
        and all(character in "0123456789abcdef" for character in value)
    )


# parse one tar octal field
def tar_octal(header, start, length, label):
    raw = header[start:start + length].split(b"\0", 1)[0].strip()

    # reject base-256 and malformed fields
    if not raw or any(byte not in b"01234567" for byte in raw):
        fail(f"invalid tar {label}")
    return int(raw, 8)


# parse one tar text field
def tar_text(header, start, length):
    raw = header[start:start + length].split(b"\0", 1)[0]
    return raw.decode("utf-8", "strict")


# validate one canonical member path
def safe_path(value, nested):
    # require a normalized relative posix path
    if (
        not isinstance(value, str)
        or not value
        or value.startswith("/")
        or "\\" in value
        or any(ord(character) < 32 or ord(character) == 127 for character in value)
    ):
        fail("archive path is invalid")
    path = PurePosixPath(value)

    # reject traversal and root aliases
    if any(part in ("", ".", "..") for part in path.parts) or str(path) != value:
        fail("archive path is not canonical")

    # require root-only package members
    if not nested and len(path.parts) != 1:
        fail("package member is not root-level")


# decompress within a fixed ceiling
def decompress(path, limit):
    chunks = []
    total = 0

    # read bounded gzip output
    with gzip.open(path, "rb") as source:
        while True:
            chunk = source.read(min(1024 * 1024, limit + 1 - total))

            # stop at gzip eof
            if not chunk:
                break
            chunks.append(chunk)
            total += len(chunk)

            # reject decompression bombs
            if total > limit:
                fail("archive tar bytes exceed their limit")
    return b"".join(chunks)


# scan raw ustar without extensions
def scan_archive(path, nested, maximum_entries, maximum_member, maximum_decoded):
    tar_limit = maximum_decoded + maximum_entries * 1024 + 10240
    raw = decompress(path, tar_limit)
    entries = []
    names = set()
    offset = 0
    zero_blocks = 0
    decoded = 0

    # walk every raw header
    while offset + 512 <= len(raw):
        header = raw[offset:offset + 512]

        # retain trailer blocks
        if header == bytes(512):
            zero_blocks += 1
            offset += 512
            continue

        # reject bytes after the trailer
        if zero_blocks:
            fail("archive contains data after its trailer")
        expected_checksum = tar_octal(header, 148, 8, "checksum")
        checksum_header = bytearray(header)
        checksum_header[148:156] = b"        "

        # reject corrupted headers
        if sum(checksum_header) != expected_checksum:
            fail("archive header checksum differs")

        # require plain ustar
        if header[257:263] not in (b"ustar\0", b"ustar "):
            fail("archive is not plain ustar")

        # reject links, devices and pax/gnu extensions
        if header[156] not in (0, ord("0")):
            fail("archive member is not a plain regular file")
        name = tar_text(header, 0, 100)
        prefix = tar_text(header, 345, 155)
        member_path = f"{prefix}/{name}" if prefix else name
        safe_path(member_path, nested)

        # reject duplicate members
        if member_path in names:
            fail("archive contains a duplicate member")
        names.add(member_path)
        size = tar_octal(header, 124, 12, "size")
        mode = tar_octal(header, 100, 8, "mode")

        # enforce member size
        if size > maximum_member:
            fail("archive member exceeds its size limit")
        content_offset = offset + 512
        next_offset = content_offset + ((size + 511) // 512) * 512

        # reject truncated data
        if next_offset > len(raw):
            fail("archive member is truncated")
        content = raw[content_offset:content_offset + size]
        entries.append({
            "path": member_path,
            "size": size,
            "mode": mode,
            "digest": digest(content),
            "content": content,
        })
        decoded += size
        offset = next_offset

    # require a complete trailer
    if zero_blocks < 2 or offset != len(raw):
        fail("archive trailer is incomplete")

    # enforce aggregate bounds
    if len(entries) > maximum_entries or decoded > maximum_decoded:
        fail("archive aggregate bounds are exceeded")
    return entries, decoded


# parse one strict json object
def json_object(entry, label):
    try:
        value = json.loads(entry["content"])
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        fail(f"{label} is invalid JSON: {error}")

    # require one object root
    if not isinstance(value, dict):
        fail(f"{label} is not an object")
    return value


# require an exact key set
def exact_keys(value, expected, label):
    # reject missing or extension keys
    if not isinstance(value, dict) or set(value) != set(expected):
        fail(f"{label} fields differ")


# validate the closed b1 replacement manifest
def validate_index_manifest(manifest):
    exact_keys(
        manifest,
        ["schema", "operation", "admission", "restoreContext", "replacement"],
        "index manifest",
    )

    # reject v1 additive packages
    if manifest["schema"] != "weather-page-indexes/v2":
        fail("index manifest is not the closed B1 replacement")
    replacement = manifest["replacement"]
    exact_keys(
        replacement,
        ["schema", "target", "predecessor", "covering", "protected"],
        "replacement manifest",
    )
    canonical = json.dumps(
        replacement,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")

    # bind all object names order and sql definitions
    if (
        replacement["schema"] != "weather-page-index-replacement/v1"
        or digest(canonical) != B1_REPLACEMENT_SHA256
    ):
        fail("B1 replacement manifest differs")


# write one verified entry
def extract_entry(root, entry):
    destination = root.joinpath(*PurePosixPath(entry["path"]).parts)
    destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)

    # retain private directory modes
    current = destination.parent
    while current != root.parent and current.is_relative_to(root):
        os.chmod(current, 0o700)

        # stop at extraction root
        if current == root:
            break
        current = current.parent
    descriptor = os.open(
        destination,
        os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0),
        0o600,
    )
    try:
        remaining = memoryview(entry["content"])

        # write every verified byte
        while remaining:
            written = os.write(descriptor, remaining)
            remaining = remaining[written:]
        os.fchmod(descriptor, 0o600)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


# require one critical entry
def critical_entry(value, expected_path, files_by_path, label):
    exact_keys(value, ["path", "digest"], label)

    # require the declared path and digest
    if value["path"] != expected_path:
        fail(f"{label} path differs")
    source = files_by_path.get(expected_path)

    # bind the critical digest to the full descriptor
    if source is None or value["digest"] != source["digest"]:
        fail(f"{label} digest differs")


# validate one complete restore descriptor
def validate_restore(descriptor, restore_entries, restore_path, restore_hash, strict):
    fields = [
        "schema",
        "repository",
        "tag",
        "commit",
        "tree",
        "fileCount",
        "decodedBytes",
        "filesDigest",
        "archive",
        "files",
        "critical",
        "maximumFileCount",
        "maximumDecodedBytes",
        "criticalPaths",
        "migrations",
        "candidatePrefixMaximumLength",
        "forbiddenProjectNames",
        "forbiddenDatabaseNames",
        "forbiddenDataMountSources",
    ]
    exact_keys(descriptor, fields, "restoreContext")

    # require the descriptor schema
    if descriptor["schema"] != "weather-page-index-restore-context/v1":
        fail("restoreContext schema differs")

    # require valid git identities
    if not hexadecimal(descriptor["commit"], SHA1_LENGTH) or not hexadecimal(
        descriptor["tree"], SHA1_LENGTH
    ):
        fail("restoreContext Git identity is invalid")

    # enforce the frozen production source
    if strict:
        for key, expected in FROZEN_RESTORE.items():
            # reject frozen source drift
            if descriptor.get(key) != expected:
                fail(f"restoreContext {key} differs")

        # require the frozen operational bounds
        if (
            descriptor["maximumFileCount"] != RESTORE_FILE_LIMIT
            or descriptor["maximumDecodedBytes"] != RESTORE_DECODED_LIMIT
            or descriptor["criticalPaths"] != list(CRITICAL.values())
            or descriptor["migrations"] != MIGRATIONS
            or descriptor["candidatePrefixMaximumLength"] != 32
            or descriptor["forbiddenProjectNames"] != ["weather"]
            or descriptor["forbiddenDatabaseNames"] != ["weather"]
            or descriptor["forbiddenDataMountSources"] != ["/var/lib/weather/postgres"]
        ):
            fail("restoreContext operational template differs")
    archive = descriptor["archive"]
    exact_keys(archive, ["name", "digest", "size"], "restoreContext archive")

    # bind the separate archive
    if (
        archive["name"] != Path(restore_path).name
        or archive["digest"] != restore_hash
        or archive["size"] != os.stat(restore_path).st_size
    ):
        fail("restoreContext archive identity differs")
    files = descriptor["files"]

    # require one bounded descriptor list
    if (
        not isinstance(files, list)
        or len(files) != descriptor["fileCount"]
        or len(files) > RESTORE_FILE_LIMIT
    ):
        fail("restoreContext file count differs")
    paths = []
    files_by_path = {}
    decoded = 0

    # validate every descriptor member
    for entry in files:
        exact_keys(entry, ["path", "mode", "blob", "digest", "size"], "restore file")
        safe_path(entry["path"], True)

        # require finite regular git metadata
        if (
            entry["mode"] not in ("100644", "100755")
            or not hexadecimal(entry["blob"], SHA1_LENGTH)
            or not hexadecimal(entry["digest"], SHA256_LENGTH)
            or not isinstance(entry["size"], int)
            or isinstance(entry["size"], bool)
            or entry["size"] < 0
        ):
            fail("restore file metadata is invalid")
        paths.append(entry["path"])
        files_by_path[entry["path"]] = entry
        decoded += entry["size"]

    # require sorted unique descriptor paths
    if paths != sorted(paths, key=lambda path: path.encode("utf-8")) or len(set(paths)) != len(paths):
        fail("restoreContext paths are not sorted and unique")

    # require declared aggregate bytes
    if decoded != descriptor["decodedBytes"] or decoded > RESTORE_DECODED_LIMIT:
        fail("restoreContext decoded bytes differ")
    canonical_files = json.dumps(
        files,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    )

    # bind the complete descriptor list
    if descriptor["filesDigest"] != digest(canonical_files.encode("utf-8")):
        fail("restoreContext filesDigest differs")

    # bind every archive member to the descriptor
    if [entry["path"] for entry in restore_entries] != paths:
        fail("restore archive paths differ")
    for archived, declared in zip(restore_entries, files, strict=True):
        expected_mode = 0o755 if declared["mode"] == "100755" else 0o644

        # reject transformed restore bytes
        if (
            archived["digest"] != declared["digest"]
            or archived["size"] != declared["size"]
            or archived["mode"] != expected_mode
        ):
            fail(f"restore archive member differs: {archived['path']}")
    critical = descriptor["critical"]
    exact_keys(critical, ["restore", "common", "compose", "acl", "migrations"], "critical")

    # bind finite critical dependencies
    for key, expected_path in CRITICAL.items():
        critical_entry(critical[key], expected_path, files_by_path, f"critical {key}")
    migrations = critical["migrations"]

    # require the production migration sequence
    if not isinstance(migrations, list) or not all(
        isinstance(entry, dict) for entry in migrations
    ):
        fail("critical migrations are invalid")
    if strict and [entry.get("path") for entry in migrations] != MIGRATIONS:
        fail("critical migrations differ")

    # bind every declared migration
    for index, migration in enumerate(migrations):
        expected_path = MIGRATIONS[index] if strict else migration.get("path")
        critical_entry(migration, expected_path, files_by_path, "critical migration")


# validate both archives and extract private bytes
def validate(
    action,
    archive_path,
    archive_hash,
    restore_path,
    restore_hash,
    release,
    commit,
    destination,
    restore_destination,
):
    strict = action == "install"

    # require bounded compressed sources
    if os.stat(archive_path).st_size > PACKAGE_ARCHIVE_LIMIT:
        fail("package archive exceeds 1MiB")
    if os.stat(restore_path).st_size > RESTORE_ARCHIVE_LIMIT:
        fail("restore archive exceeds 64MiB")

    # recheck both trusted hashes
    for path, expected in ((archive_path, archive_hash), (restore_path, restore_hash)):
        with open(path, "rb") as source:
            actual = hashlib.file_digest(source, "sha256").hexdigest()

        # reject substituted archives
        if actual != expected:
            fail("archive SHA-256 differs")
    package_entries, package_decoded = scan_archive(
        archive_path,
        False,
        3,
        PACKAGE_MEMBER_LIMIT,
        PACKAGE_DECODED_LIMIT,
    )

    # require exact finite package members
    if [entry["path"] for entry in package_entries] != PACKAGE_NAMES:
        fail("package archive members differ")
    package_by_path = {entry["path"]: entry for entry in package_entries}
    release_manifest = json_object(package_by_path["release-manifest.json"], "release manifest")
    exact_keys(release_manifest, [
        "schema",
        "repository",
        "tag",
        "commit",
        "tree",
        "files",
        "restoreContext",
    ], "release manifest")
    strict = strict or release_manifest.get("repository") == "anstosa/weather"

    # require release identity
    if (
        release_manifest["schema"] != "weather-page-index-release/v1"
        or release_manifest["tag"] != release
        or release_manifest["commit"] != commit
        or not hexadecimal(release_manifest["tree"], SHA1_LENGTH)
        or (strict and release_manifest["repository"] != "anstosa/weather")
    ):
        fail("release manifest identity differs")
    files = release_manifest["files"]

    # require only helper and index manifest
    if not isinstance(files, list) or [entry.get("path") for entry in files] != RELEASE_FILE_NAMES:
        fail("release manifest files differ")

    # bind both payload files
    for entry in files:
        exact_keys(entry, ["path", "digest", "size", "mode"], "release file")
        archived = package_by_path[entry["path"]]

        # require finite source metadata
        if (
            entry["mode"] not in ("100644", "100755")
            or not hexadecimal(entry["digest"], SHA256_LENGTH)
            or not isinstance(entry["size"], int)
            or isinstance(entry["size"], bool)
            or entry["size"] < 0
        ):
            fail("release file metadata is invalid")
        expected_mode = 0o755 if entry["mode"] == "100755" else 0o644

        # reject transformed payload bytes
        if (
            archived["digest"] != entry["digest"]
            or archived["size"] != entry["size"]
            or archived["mode"] != expected_mode
        ):
            fail(f"release file differs: {entry['path']}")

    # require regular manifest archive mode
    if package_by_path["release-manifest.json"]["mode"] != 0o644:
        fail("release manifest archive mode differs")
    index_manifest = json_object(package_by_path["weather-page-indexes.json"], "index manifest")
    validate_index_manifest(index_manifest)
    restore_entries, restore_decoded = scan_archive(
        restore_path,
        True,
        RESTORE_FILE_LIMIT,
        RESTORE_DECODED_LIMIT,
        RESTORE_DECODED_LIMIT,
    )
    restore_descriptor = index_manifest.get("restoreContext")
    release_restore_descriptor = release_manifest.get("restoreContext")

    # require embedded restore provenance
    if not isinstance(restore_descriptor, dict) or not isinstance(
        release_restore_descriptor, dict
    ):
        fail("index manifest lacks restoreContext")
    canonical_index_restore = json.dumps(
        restore_descriptor,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    )
    canonical_release_restore = json.dumps(
        release_restore_descriptor,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
    )

    # require one identical embedded descriptor
    if canonical_index_restore != canonical_release_restore:
        fail("release and index restoreContext differ")
    validate_restore(
        release_restore_descriptor,
        restore_entries,
        restore_path,
        restore_hash,
        strict,
    )

    # require observed aggregate bytes
    if package_decoded != sum(entry["size"] for entry in package_entries):
        fail("package decoded bytes differ")
    if restore_decoded != restore_descriptor["decodedBytes"]:
        fail("restore decoded bytes differ")
    destination_root = Path(destination)

    # extract only into an empty private root
    if any(destination_root.iterdir()):
        fail("extraction destination is not empty")
    for entry in package_entries:
        extract_entry(destination_root, entry)
    restore_root = Path(restore_destination)

    # extract restore bytes only into a separate empty root
    if any(restore_root.iterdir()):
        fail("restore extraction destination is not empty")
    for entry in restore_entries:
        extract_entry(restore_root, entry)


(
    action,
    archive,
    archive_hash,
    restore_archive,
    restore_hash,
    release,
    commit,
    destination,
    restore_destination,
) = sys.argv[1:]

# execute one finite validation action
if action not in ("verify", "install"):
    fail("unsupported validation action")

try:
    validate(
        action,
        archive,
        archive_hash,
        restore_archive,
        restore_hash,
        release,
        commit,
        destination,
        restore_destination,
    )
except (OSError, ValueError) as error:
    print(f"install-weather-page-indexes: {error}", file=sys.stderr)
    raise SystemExit(1) from error
PYTHON
}

# fsync one complete tree
fsync_tree() {
  local root="$1"

  "${PYTHON}" - "${root}" <<'PYTHON'
import os
from pathlib import Path
import stat
import sys

root = Path(sys.argv[1])
directories = []

# walk without links
for current, names, files in os.walk(root, topdown=True, followlinks=False):
    current_path = Path(current)
    directories.append(current_path)

    # require every directory entry
    for name in [*names, *files]:
        path = current_path / name
        metadata = path.lstat()

        # reject linked or foreign entries
        if stat.S_ISLNK(metadata.st_mode) or metadata.st_uid != 0 or metadata.st_gid != 0:
            raise SystemExit("installed tree contains an untrusted entry")

    # sync every regular file
    for name in files:
        path = current_path / name
        metadata = path.lstat()

        # require private regular files
        if not stat.S_ISREG(metadata.st_mode) or stat.S_IMODE(metadata.st_mode) != 0o600:
            raise SystemExit("installed tree file mode differs")
        descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)

# sync directories from leaves upward
for path in sorted(directories, key=lambda value: len(value.parts), reverse=True):
    metadata = path.lstat()

    # require private root-owned directories
    if (
        not stat.S_ISDIR(metadata.st_mode)
        or stat.S_IMODE(metadata.st_mode) != 0o700
        or metadata.st_uid != 0
        or metadata.st_gid != 0
    ):
        raise SystemExit("installed tree directory mode differs")
    descriptor = os.open(path, os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
PYTHON
}

# compare exact installed trees
trees_are_identical() {
  local left="$1"
  local right="$2"

  "${PYTHON}" - "${left}" "${right}" <<'PYTHON'
import hashlib
import os
from pathlib import Path
import stat
import sys


# describe one immutable tree
def describe(root):
    root_path = Path(root)
    entries = []

    # inspect every entry without following links
    for current, names, files in os.walk(root_path, topdown=True, followlinks=False):
        current_path = Path(current)

        # reject linked directories
        for name in names:
            path = current_path / name
            metadata = path.lstat()

            # require private root-owned directories
            if (
                not stat.S_ISDIR(metadata.st_mode)
                or stat.S_IMODE(metadata.st_mode) != 0o700
                or metadata.st_uid != 0
                or metadata.st_gid != 0
            ):
                raise ValueError("directory metadata differs")
            entries.append((str(path.relative_to(root_path)), "directory"))

        # hash every private regular file
        for name in files:
            path = current_path / name
            metadata = path.lstat()

            # require private root-owned regular files
            if (
                not stat.S_ISREG(metadata.st_mode)
                or stat.S_IMODE(metadata.st_mode) != 0o600
                or metadata.st_uid != 0
                or metadata.st_gid != 0
                or metadata.st_nlink != 1
            ):
                raise ValueError("file metadata differs")
            with open(path, "rb") as source:
                file_digest = hashlib.file_digest(source, "sha256").hexdigest()
            entries.append((str(path.relative_to(root_path)), "file", metadata.st_size, file_digest))
    return sorted(entries)


try:
    identical = describe(sys.argv[1]) == describe(sys.argv[2])
except (OSError, ValueError):
    identical = False
raise SystemExit(0 if identical else 1)
PYTHON
}

# parse paired command flags
parse_flags() {
  archive=""
  archive_sha256=""
  restore_archive=""
  restore_sha256=""
  installer_sha256=""
  incoming_root=""
  release=""
  commit=""

  # consume the finite flag set
  while (( $# > 0 )); do
    local flag="$1"
    shift

    # reject missing values
    if (( $# == 0 )); then
      fail "missing value for ${flag}"
    fi
    local value="$1"
    shift

    # bind one supported input
    case "${flag}" in
      --archive)
        archive="${value}"
        ;;
      --archive-sha256)
        archive_sha256="${value}"
        ;;
      --restore-archive)
        restore_archive="${value}"
        ;;
      --restore-sha256)
        restore_sha256="${value}"
        ;;
      --installer-sha256)
        installer_sha256="${value}"
        ;;
      --incoming-root)
        incoming_root="${value}"
        ;;
      --release)
        release="${value}"
        ;;
      --commit)
        commit="${value}"
        ;;
      *)
        fail "unsupported argument: ${flag}"
        ;;
    esac
  done
}

# run non-mutating archive verification
verify_action() {
  parse_flags "$@"

  # require the complete verify interface
  if [[ -z "${archive}" || -z "${archive_sha256}" || -z "${restore_archive}" || \
    -z "${restore_sha256}" || -z "${release}" || -z "${commit}" || \
    -n "${installer_sha256}" || -n "${incoming_root}" ]]; then
    fail "verify arguments are incomplete"
  fi
  require_sha256 "${archive_sha256}"
  require_sha256 "${restore_sha256}"
  require_release_identity "${release}" "${commit}"

  # require finite local verification inputs
  if [[ ! -f "${archive}" || -L "${archive}" || ! -f "${restore_archive}" || \
    -L "${restore_archive}" ]]; then
    fail "verification archive is not a regular file"
  fi
  verify_hash "${archive}" "${archive_sha256}"
  verify_hash "${restore_archive}" "${restore_sha256}"
  local verify_root
  verify_root="$(/usr/bin/mktemp -d)"
  /usr/bin/chmod 0700 -- "${verify_root}"
  local verify_package_root="${verify_root}/package"
  local verify_restore_root="${verify_root}/restore-context"
  /usr/bin/mkdir -m 0700 -- "${verify_package_root}" "${verify_restore_root}"

  # retain the validation status through cleanup
  local status=0
  verify_and_extract \
    "verify" \
    "$(canonical_path "${archive}")" \
    "${archive_sha256}" \
    "$(canonical_path "${restore_archive}")" \
    "${restore_sha256}" \
    "${release}" \
    "${commit}" \
    "${verify_package_root}" \
    "${verify_restore_root}" || status=$?
  /usr/bin/rm -rf --one-file-system -- "${verify_root}"

  # propagate validation failure
  if (( status != 0 )); then
    return "${status}"
  fi
  printf '%s\n' "weather page index package verified"
}

# run the root installation
install_action() {
  parse_flags "$@"

  # require actual root authority
  if (( EUID != 0 )); then
    fail "installer requires root"
  fi

  # require the complete install interface
  if [[ -z "${incoming_root}" || -z "${archive_sha256}" || -z "${restore_sha256}" || \
    -z "${installer_sha256}" || -z "${release}" || -z "${commit}" || \
    -n "${archive}" || -n "${restore_archive}" ]]; then
    fail "install arguments are incomplete"
  fi
  require_sha256 "${archive_sha256}"
  require_sha256 "${restore_sha256}"
  require_sha256 "${installer_sha256}"
  require_release_identity "${release}" "${commit}"
  require_install_ancestry
  local expected_incoming_pattern="${INSTALL_ROOT}/.incoming."
  local incoming_suffix="${incoming_root#"${expected_incoming_pattern}"}"

  # require one direct canonical incoming root
  if [[ "${incoming_root}" != "${expected_incoming_pattern}"* || \
    "${incoming_suffix}" == *[^A-Za-z0-9._-]* || \
    -z "${incoming_suffix}" || "${incoming_suffix}" == .* ]]; then
    fail "incoming root name is invalid"
  fi
  require_root_directory "${incoming_root}" "700"
  local installer_path
  installer_path="$(canonical_path "${BASH_SOURCE[0]}")"
  local expected_installer_name="install-weather-page-indexes-${release}-${commit:0:12}-${installer_sha256}.sh"

  # require this exact installed script
  if [[ "${installer_path}" != "${incoming_root}/${expected_installer_name}" ]]; then
    fail "installer path does not match its trusted identity"
  fi
  local archive_name="weather-page-indexes-${release}-${commit:0:12}.tar.gz"
  archive="${incoming_root}/${archive_name}"
  local -a incoming_entries=()
  mapfile -d '' incoming_entries < <(
    /usr/bin/find -P "${incoming_root}" -mindepth 1 -maxdepth 1 -print0
  )

  # require exactly archive, restore archive and installer
  if (( ${#incoming_entries[@]} != 3 )); then
    fail "incoming root entries differ"
  fi
  local entry
  restore_archive=""

  # classify the finite incoming entries
  for entry in "${incoming_entries[@]}"; do
    local name
    name="$(/usr/bin/basename -- "${entry}")"

    # accept only the three expected name classes
    if [[ "${name}" == "${archive_name}" || "${name}" == "${expected_installer_name}" ]]; then
      :
    elif [[ "${name}" =~ ^restore-context-[a-f0-9]{12}\.tar\.gz$ && \
      -z "${restore_archive}" ]]; then
      restore_archive="${entry}"
    else
      fail "incoming root contains an unexpected entry"
    fi
  done

  # require one restore archive
  if [[ -z "${restore_archive}" ]]; then
    fail "incoming restore archive is missing"
  fi
  require_incoming_file "${installer_path}" "${PACKAGE_LIMIT}"
  require_incoming_file "${archive}" "${PACKAGE_LIMIT}"
  require_incoming_file "${restore_archive}" "${RESTORE_LIMIT}"
  verify_hash "${installer_path}" "${installer_sha256}"
  verify_hash "${archive}" "${archive_sha256}"
  verify_hash "${restore_archive}" "${restore_sha256}"
  local archive_identity
  local restore_identity
  local installer_identity
  archive_identity="$(file_identity "${archive}")"
  restore_identity="$(file_identity "${restore_archive}")"
  installer_identity="$(file_identity "${installer_path}")"

  # create only a missing releases directory
  if [[ ! -e "${RELEASES_ROOT}" ]]; then
    /usr/bin/mkdir -m 0700 -- "${RELEASES_ROOT}"
  fi
  require_root_directory "${RELEASES_ROOT}" "700"
  # create only a missing restore-context root
  if [[ ! -e "${RESTORE_CONTEXTS_ROOT}" ]]; then
    /usr/bin/mkdir -m 0700 -- "${RESTORE_CONTEXTS_ROOT}"
  fi
  require_root_directory "${RESTORE_CONTEXTS_ROOT}" "700"
  /usr/bin/sync -f -- "${INSTALL_ROOT}"
  stage_root="$(/usr/bin/mktemp -d "${RELEASES_ROOT}/.incoming-${release}-${commit:0:12}.XXXXXX")"
  context_stage_root="$(/usr/bin/mktemp -d "${RESTORE_CONTEXTS_ROOT}/.incoming-2026.10.09-1-56b327d9c750.XXXXXX")"
  /usr/bin/chown 0:0 -- "${stage_root}"
  /usr/bin/chown 0:0 -- "${context_stage_root}"
  /usr/bin/chmod 0700 -- "${stage_root}"
  /usr/bin/chmod 0700 -- "${context_stage_root}"
  verify_and_extract \
    "install" \
    "${archive}" \
    "${archive_sha256}" \
    "${restore_archive}" \
    "${restore_sha256}" \
    "${release}" \
    "${commit}" \
    "${stage_root}" \
    "${context_stage_root}"

  # reject source swaps during extraction
  if [[ "$(file_identity "${archive}")" != "${archive_identity}" || \
    "$(file_identity "${restore_archive}")" != "${restore_identity}" || \
    "$(file_identity "${installer_path}")" != "${installer_identity}" ]]; then
    fail "incoming file identity changed"
  fi
  verify_hash "${archive}" "${archive_sha256}"
  verify_hash "${restore_archive}" "${restore_sha256}"
  verify_hash "${installer_path}" "${installer_sha256}"
  /usr/bin/chown -R 0:0 -- "${stage_root}"

  # freeze all extracted files
  while IFS= read -r -d '' path; do
    /usr/bin/chmod 0600 -- "${path}"
  done < <(/usr/bin/find -P "${stage_root}" -type f -print0)

  # freeze all extracted directories
  while IFS= read -r -d '' path; do
    /usr/bin/chmod 0700 -- "${path}"
  done < <(/usr/bin/find -P "${stage_root}" -type d -print0)
  fsync_tree "${stage_root}"
  /usr/bin/chown -R 0:0 -- "${context_stage_root}"

  # freeze all restore files
  while IFS= read -r -d '' path; do
    /usr/bin/chmod 0600 -- "${path}"
  done < <(/usr/bin/find -P "${context_stage_root}" -type f -print0)

  # freeze all restore directories
  while IFS= read -r -d '' path; do
    /usr/bin/chmod 0700 -- "${path}"
  done < <(/usr/bin/find -P "${context_stage_root}" -type d -print0)
  fsync_tree "${context_stage_root}"
  local destination="${RELEASES_ROOT}/${release}-${commit:0:12}"
  local destination_identical="false"

  # reject a foreign package before publishing restore bytes
  if [[ -e "${destination}" || -L "${destination}" ]]; then
    if trees_are_identical "${stage_root}" "${destination}"; then
      destination_identical="true"
    else
      fail "installed release already exists with different bytes"
    fi
  fi
  local context_destination="${RESTORE_CONTEXTS_ROOT}/2026.10.09-1-56b327d9c750"

  # retain only an exact existing restore context
  if [[ -e "${context_destination}" || -L "${context_destination}" ]]; then
    if trees_are_identical "${context_stage_root}" "${context_destination}"; then
      cleanup_owned_stage "${context_stage_root}" "${RESTORE_CONTEXTS_ROOT}"
      context_stage_root=""
    else
      fail "installed restore context already exists with different bytes"
    fi
  else
    /usr/bin/mv --no-clobber --no-target-directory -- \
      "${context_stage_root}" "${context_destination}"

    # handle a concurrent restore-context race
    if [[ -e "${context_stage_root}" ]]; then
      if trees_are_identical "${context_stage_root}" "${context_destination}"; then
        cleanup_owned_stage "${context_stage_root}" "${RESTORE_CONTEXTS_ROOT}"
        context_stage_root=""
      else
        fail "concurrent installed restore context differs"
      fi
    else
      context_stage_root=""
    fi
    /usr/bin/sync -f -- "${RESTORE_CONTEXTS_ROOT}"
  fi

  # retain the exact package checked before context publication
  if [[ "${destination_identical}" == "true" ]]; then
    cleanup_owned_stage "${stage_root}" "${RELEASES_ROOT}"
    stage_root=""
    printf 'weather page index package already installed: %s\n' "${destination}"
    return
  fi
  /usr/bin/mv --no-clobber --no-target-directory -- "${stage_root}" "${destination}"

  # handle a concurrent no-overwrite race
  if [[ -e "${stage_root}" ]]; then
    if trees_are_identical "${stage_root}" "${destination}"; then
      cleanup_owned_stage "${stage_root}" "${RELEASES_ROOT}"
      stage_root=""
      printf 'weather page index package already installed: %s\n' "${destination}"
      return
    fi
    fail "concurrent installed release differs"
  fi
  stage_root=""
  /usr/bin/sync -f -- "${RELEASES_ROOT}"
  printf 'weather page index package installed: %s\n' "${destination}"
}

readonly requested_action="${1:-}"

# dispatch one finite action
case "${requested_action}" in
  verify)
    shift
    verify_action "$@"
    ;;
  install)
    shift
    install_action "$@"
    ;;
  *)
    fail "usage: install-weather-page-indexes.sh verify|install ..."
    ;;
esac
