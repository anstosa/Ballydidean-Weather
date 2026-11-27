#!/usr/bin/env python3
"""Run one isolated read-only Node API against a restored benchmark slot.

The helper never runs migrations or workers. It starts one PostgreSQL container
from an already verified restored volume and one API container from the exact
currently installed release. Both containers use a fixture-only internal
network. The API keeps a loopback publication request, while the handoff uses
its separately verified internal bridge endpoint.
"""

import argparse
import hashlib
import http.client
import ipaddress
import json
import os
import re
import stat
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timezone
from pathlib import Path

DOCKER = "/usr/bin/docker"
FIXTURE_BASE = Path("/var/lib/weather-native-benchmark")
INSTALLED_ROOT = Path("/opt/weather/current")
RUNTIME_BASE = Path("/run")
MARKER_NAME = ".fixture-marker.json"
APPROVAL_NAME = ".approved-cap.json"
ADMISSION_NAME = ".storage-admission.json"
RESTORE_HANDOFF_NAME = ".restore-handoff.json"
SESSION_DIRECTORY_NAME = "node-api"
JOURNAL_NAME = "journal.json"
HANDOFF_NAME = "handoff.json"
KIND = "rust-rewrite"
VOLUME_KIND = "rust-rewrite"
SESSION_KIND = "weather-native-node-api/v1"
PROFILE_API_ONLY = "api-only"
PROFILE_API_WEB = "api-web"
FIXTURE_ID_RE = re.compile(r"rust-rewrite-[0-9a-f]{16}")
SHA256_RE = re.compile(r"[0-9a-f]{64}")
IMAGE_ID_RE = re.compile(r"sha256:[0-9a-f]{64}")
IMAGE_REFERENCE_RE = re.compile(
    r"[^\s:@/]+(?:/[^\s:@/]+)*@sha256:[0-9a-f]{64}",
)
RELEASE_RE = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}")
ENVIRONMENT_NAME_RE = re.compile(r"[A-Z][A-Z0-9_]*")
VOLUME_SUFFIXES = tuple(f"slot-{index:02d}" for index in range(1, 6))
SECRET_NAMES = (
    "weather_admin_password",
    "weather_owner_password",
    "weather_api_password",
    "weather_ingest_password",
    "weather_training_export_password",
)
HANDOFF_KEYS = {
    "schemaVersion",
    "kind",
    "fixtureId",
    "fixtureRoot",
    "volumeName",
    "volumeDevice",
    "databaseName",
    "imageId",
    "imageRepoDigest",
    "archiveSha256",
    "sourceArchiveCiphertextSha256",
    "markerSha256",
    "admissionReceiptSha256",
    "approvedCapSha256",
    "approvedCapExpiresAt",
    "expectationSha256",
    "expectedLedgerSha256",
    "actualLedgerSha256",
    "counts",
    "sampleHash",
    "serverVersionNum",
    "configSha256",
    "credentialDirectory",
    "credentialManifestSha256",
    "completedAt",
}


class FixtureError(RuntimeError):
    """Report one bounded fixture lifecycle failure."""


# build the exact fixture-owned label namespace
def fixture_labels(marker, role):
    return {
        "weather.native-benchmark.fixture-id": marker["fixtureId"],
        "weather.native-benchmark.kind": KIND,
        "weather.native-benchmark.role": role,
    }


# return one current utc instant
def utc_now():
    return datetime.now(timezone.utc)


# hash one byte sequence
def bytes_sha256(value):
    return hashlib.sha256(value).hexdigest()


# run one bounded command without a shell
def run_command(argv, timeout=60, check=True, input_bytes=None):
    result = subprocess.run(
        argv,
        input=input_bytes,
        capture_output=True,
        timeout=timeout,
        check=False,
    )
    # fail with bounded diagnostics
    if check and result.returncode != 0:
        detail = result.stderr.decode("utf-8", errors="replace")[-4096:].strip()
        raise FixtureError(f"command failed ({Path(argv[0]).name}): {detail}")
    return result


# require one exact path type ownership and mode
def require_path(path, description, kind, owner=None, mode=None):
    try:
        metadata = path.lstat()
    except OSError as error:
        raise FixtureError(f"{description} is unavailable") from error
    # reject path aliases
    if stat.S_ISLNK(metadata.st_mode):
        raise FixtureError(f"{description} must not be a symlink")
    # require one directory
    if kind == "directory" and not stat.S_ISDIR(metadata.st_mode):
        raise FixtureError(f"{description} must be a directory")
    # require one regular file
    if kind == "file" and not stat.S_ISREG(metadata.st_mode):
        raise FixtureError(f"{description} must be a regular file")
    # bind requested ownership
    if owner is not None and (metadata.st_uid, metadata.st_gid) != owner:
        raise FixtureError(f"{description} ownership differs")
    # bind requested permission bits
    if mode is not None and stat.S_IMODE(metadata.st_mode) != mode:
        raise FixtureError(f"{description} mode differs")
    return metadata


# require one canonical absolute path
def require_canonical(path, description):
    # reject relative paths and aliases
    if not path.is_absolute() or path.resolve(strict=True) != path:
        raise FixtureError(f"{description} must be a canonical absolute path")
    return path


# read one bounded regular file without following aliases
def read_regular_bytes(path, description, maximum_bytes, owner=None):
    metadata = require_path(path, description, "file", owner=owner)
    require_canonical(path, description)
    # reject shared mutation authority and oversized inputs
    if (
        metadata.st_mode & 0o022
        or metadata.st_nlink != 1
        or metadata.st_size > maximum_bytes
    ):
        raise FixtureError(f"{description} permissions or size differ")
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        opened = os.fstat(descriptor)
        raw = os.read(descriptor, maximum_bytes + 1)
        final = os.fstat(descriptor)
        # reject substitution or concurrent mutation
        if (
            (opened.st_dev, opened.st_ino, opened.st_size, opened.st_mtime_ns)
            != (
                metadata.st_dev,
                metadata.st_ino,
                metadata.st_size,
                metadata.st_mtime_ns,
            )
            or (final.st_dev, final.st_ino, final.st_size, final.st_mtime_ns)
            != (opened.st_dev, opened.st_ino, opened.st_size, opened.st_mtime_ns)
            or len(raw) != metadata.st_size
        ):
            raise FixtureError(f"{description} changed during validation")
    finally:
        os.close(descriptor)
    return raw


# read one bounded private json object
def read_json(path, description, maximum_bytes=65536, owner=None, mode=None):
    raw = read_regular_bytes(path, description, maximum_bytes, owner)
    # bind exact private permission when requested
    if mode is not None and stat.S_IMODE(path.stat().st_mode) != mode:
        raise FixtureError(f"{description} mode differs")
    try:
        value = json.loads(raw.decode("utf-8"))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise FixtureError(f"{description} is not valid JSON") from error
    # require one object
    if not isinstance(value, dict):
        raise FixtureError(f"{description} must be a JSON object")
    return value, bytes_sha256(raw)


# write one private crash-safe json object
def write_private_json(path, value):
    raw = (json.dumps(value, sort_keys=True, indent=2) + "\n").encode("utf-8")
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.",
        dir=path.parent,
    )
    temporary = Path(temporary_name)
    try:
        os.fchmod(descriptor, 0o600)
        offset = 0
        # complete every bounded write
        while offset < len(raw):
            written = os.write(descriptor, raw[offset:])
            # reject a stalled write
            if written <= 0:
                raise FixtureError("private json write made no progress")
            offset += written
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    try:
        # never overwrite lifecycle authority
        if path.exists() or path.is_symlink():
            raise FixtureError(f"{path.name} already exists")
        temporary.replace(path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    except BaseException:
        # remove only the owned temporary file
        if temporary.exists():
            temporary.unlink()
        raise


# replace one owned journal atomically
def replace_private_json(path, value):
    temporary = path.with_name(f".{path.name}.next")
    # reject a stale temporary journal
    if temporary.exists() or temporary.is_symlink():
        raise FixtureError("temporary lifecycle journal already exists")
    write_private_json(temporary, value)
    temporary.replace(path)
    directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


# validate one fixed fixture marker
def validate_marker(fixture_root, fixture_base=FIXTURE_BASE, owner=(0, 0)):
    require_path(fixture_base, "fixture base", "directory", owner=owner, mode=0o700)
    require_canonical(fixture_base, "fixture base")
    require_path(fixture_root, "fixture root", "directory", owner=owner, mode=0o700)
    require_canonical(fixture_root, "fixture root")
    # bind one direct fixture descendant
    if fixture_root.parent != fixture_base or not FIXTURE_ID_RE.fullmatch(
        fixture_root.name
    ):
        raise FixtureError("fixture root is outside the approved namespace")
    marker, marker_sha256 = read_json(
        fixture_root / MARKER_NAME,
        "fixture marker",
        owner=owner,
        mode=0o600,
    )
    expected = {
        "schemaVersion": 1,
        "kind": "weather-native-rust-rewrite-fixture",
        "fixtureId": fixture_root.name,
        "fixtureRoot": str(fixture_root),
        "backingFile": str(fixture_root / "fixture.ext4"),
        "mountpoint": str(fixture_root / "mnt"),
        "volumeRoot": str(fixture_root / "mnt" / "docker-volumes"),
        "volumeNamespacePrefix": f"weather-native-benchmark-{fixture_root.name}-",
    }
    # require the exact marker contract
    if marker != expected:
        raise FixtureError("fixture marker values differ")
    mountpoint = Path(marker["mountpoint"])
    volume_root = Path(marker["volumeRoot"])
    require_path(mountpoint, "fixture mountpoint", "directory", owner=owner, mode=0o700)
    require_path(
        volume_root, "fixture volume root", "directory", owner=owner, mode=0o700
    )
    require_canonical(mountpoint, "fixture mountpoint")
    require_canonical(volume_root, "fixture volume root")
    # bind the dedicated filesystem boundary
    if (
        volume_root.parent != mountpoint
        or os.stat(volume_root).st_dev != os.stat(mountpoint).st_dev
    ):
        raise FixtureError("fixture volume root escapes its mountpoint")
    return marker, marker_sha256


# validate current approval reserves and restored handoff
def validate_restore_handoff(
    fixture_root, marker, marker_sha256, owner=(0, 0), now=None
):
    approval, approval_sha256 = read_json(
        fixture_root / APPROVAL_NAME,
        "approved storage cap",
        owner=owner,
        mode=0o600,
    )
    admission, admission_sha256 = read_json(
        fixture_root / ADMISSION_NAME,
        "storage admission receipt",
        owner=owner,
        mode=0o600,
    )
    handoff, handoff_sha256 = read_json(
        fixture_root / RESTORE_HANDOFF_NAME,
        "restore handoff",
        owner=owner,
        mode=0o600,
    )
    # require the closed restore handoff schema
    if set(handoff) != HANDOFF_KEYS:
        raise FixtureError("restore handoff keys differ")
    current = now or utc_now()
    try:
        expires_at = datetime.strptime(
            handoff["approvedCapExpiresAt"],
            "%Y-%m-%dT%H:%M:%SZ",
        ).replace(tzinfo=timezone.utc)
    except (TypeError, ValueError) as error:
        raise FixtureError("restore handoff expiry is invalid") from error
    # bind approval identity and current retention authority
    if (
        current >= expires_at
        or approval.get("schemaVersion") != 1
        or approval.get("kind") != "weather-native-rust-rewrite-storage-approval"
        or approval.get("approvalAuthority") != "ansel"
        or approval.get("fixtureId") != marker["fixtureId"]
        or approval.get("fixtureRoot") != marker["fixtureRoot"]
        or admission.get("receiptVersion") != "weather-native-benchmark-storage/v1"
        or admission.get("phase") != "actual-admission"
        or admission.get("fixtureId") != marker["fixtureId"]
        or admission.get("fixtureRoot") != marker["fixtureRoot"]
        or admission.get("fixtureUseAuthorized") is not True
        or admission.get("admitted") is not True
        or approval.get("expiresAt") != handoff["approvedCapExpiresAt"]
        or approval.get("rootReserveBytes") != 8 * 1024**3
        or approval.get("innerReserveBytes") != 4 * 1024**3
        or handoff.get("schemaVersion") != 1
        or handoff.get("kind") != "weather-native-rust-rewrite-restore"
        or handoff.get("fixtureId") != marker["fixtureId"]
        or handoff.get("fixtureRoot") != marker["fixtureRoot"]
        or handoff.get("markerSha256") != marker_sha256
        or handoff.get("approvedCapSha256") != approval_sha256
        or handoff.get("admissionReceiptSha256") != admission_sha256
    ):
        raise FixtureError("restore handoff authority differs or expired")
    # bind the retained database identity
    if (
        handoff.get("databaseName") != "weather"
        or not IMAGE_ID_RE.fullmatch(handoff.get("imageId", ""))
        or not IMAGE_REFERENCE_RE.fullmatch(handoff.get("imageRepoDigest", ""))
        or not SHA256_RE.fullmatch(handoff.get("archiveSha256", ""))
        or not SHA256_RE.fullmatch(handoff.get("sourceArchiveCiphertextSha256", ""))
        or not SHA256_RE.fullmatch(handoff.get("expectationSha256", ""))
        or not SHA256_RE.fullmatch(handoff.get("credentialManifestSha256", ""))
        or not SHA256_RE.fullmatch(handoff.get("actualLedgerSha256", ""))
        or handoff.get("actualLedgerSha256") != handoff.get("expectedLedgerSha256")
        or re.fullmatch(r"[0-9a-f]{32}", handoff.get("sampleHash", "")) is None
        or not isinstance(handoff.get("serverVersionNum"), int)
        or handoff["serverVersionNum"] < 170000
        or not isinstance(handoff.get("counts"), dict)
        or set(handoff["counts"]) != {"sites", "sources", "runs", "records"}
        or any(
            not isinstance(value, int) or isinstance(value, bool) or value < 0
            for value in handoff["counts"].values()
        )
        or not isinstance(handoff.get("configSha256"), dict)
        or set(handoff["configSha256"])
        != {"entrypointSha256", "rolesInitSha256", "runtimeAclSha256"}
        or any(
            SHA256_RE.fullmatch(value) is None
            for value in handoff["configSha256"].values()
        )
    ):
        raise FixtureError("restore handoff database identity differs")
    root_space = os.statvfs(fixture_root)
    inner_space = os.statvfs(marker["mountpoint"])
    root_available = root_space.f_bavail * root_space.f_frsize
    inner_available = inner_space.f_bavail * inner_space.f_frsize
    # preserve both approved reserves before runtime creation
    if (
        root_available < approval["rootReserveBytes"]
        or inner_available < approval["innerReserveBytes"]
    ):
        raise FixtureError("fixture disk reserve is unavailable")
    return handoff, handoff_sha256


# validate all retained fixture-only credentials without exposing values
def validate_credentials(handoff, fixture_root):
    directory = Path(handoff["credentialDirectory"])
    require_path(
        directory, "fixture credential directory", "directory", owner=(0, 0), mode=0o700
    )
    require_canonical(directory, "fixture credential directory")
    # bind one retained fixture credential directory
    if (
        directory.parent != fixture_root
        or re.fullmatch(r"\.restore-secrets-[0-9a-f]{16}", directory.name) is None
    ):
        raise FixtureError(
            "fixture credential directory is outside the restored fixture"
        )
    # require the complete credential set
    if {path.name for path in directory.iterdir()} != set(SECRET_NAMES):
        raise FixtureError("fixture credential set differs")
    digests = {}
    # hash each credential through a bounded regular-file read
    for name in SECRET_NAMES:
        path = directory / name
        raw = read_regular_bytes(
            path, f"fixture credential {name}", 1024, owner=(999, 999)
        )
        # bind database-user-only secret permissions and content shape
        if (
            stat.S_IMODE(path.stat().st_mode) != 0o400
            or not raw.endswith(b"\n")
            or b"\n" in raw[:-1]
        ):
            raise FixtureError(f"fixture credential {name} differs")
        digests[name] = bytes_sha256(raw)
    manifest = "".join(f"{name}|{digests[name]}\n" for name in sorted(digests))
    # bind the value-free restore manifest
    if bytes_sha256(manifest.encode("ascii")) != handoff["credentialManifestSha256"]:
        raise FixtureError("fixture credential manifest differs")
    return directory


# parse one root-owned installed environment file
def read_deployment_environment(path):
    raw = read_regular_bytes(path, "deployment environment", 65536, owner=(0, 0))
    try:
        text = raw.decode("utf-8")
    except UnicodeError as error:
        raise FixtureError("deployment environment is not utf-8") from error
    values = {}
    # parse every simple deployment assignment once
    for line in text.splitlines():
        stripped = line.strip()
        # ignore comments and empty lines
        if not stripped or stripped.startswith("#"):
            continue
        name, separator, value = stripped.partition("=")
        # reject shell syntax and duplicate keys
        if (
            separator != "="
            or ENVIRONMENT_NAME_RE.fullmatch(name) is None
            or name in values
            or any(character in value for character in "\r\n\x00")
        ):
            raise FixtureError("deployment environment syntax differs")
        values[name] = value
    required = {"WEATHER_RELEASE", "WEATHER_SERVER_IMAGE", "POSTGRES_IMAGE"}
    # require the installed runtime identity
    if not required.issubset(values):
        raise FixtureError("deployment environment lacks runtime identity")
    release = values["WEATHER_RELEASE"]
    # require digest-only images and a bounded release
    if (
        RELEASE_RE.fullmatch(release) is None
        or IMAGE_REFERENCE_RE.fullmatch(values["WEATHER_SERVER_IMAGE"]) is None
        or IMAGE_REFERENCE_RE.fullmatch(values["POSTGRES_IMAGE"]) is None
    ):
        raise FixtureError("deployment runtime identity is invalid")
    database_name = values.get("WEATHER_DATABASE_NAME", "weather")
    # retain only the restored database name
    if database_name != "weather":
        raise FixtureError("installed database name differs from restored fixture")
    return values, bytes_sha256(raw)


# validate the exact current immutable deployment root
def validate_deployment_root(deployment_root, installed_root=INSTALLED_ROOT):
    require_path(deployment_root, "deployment root", "directory", owner=(0, 0))
    require_canonical(deployment_root, "deployment root")
    # bind the fixed root-owned installed tree
    if deployment_root != installed_root or deployment_root.stat().st_mode & 0o002:
        raise FixtureError("deployment root is not the exact installed tree")
    release_state_path = deployment_root / "deploy" / "state" / "current-release"
    release_state = read_regular_bytes(
        release_state_path,
        "current release state",
        256,
        owner=(0, 0),
    )
    try:
        release = release_state.decode("ascii").removesuffix("\n")
    except UnicodeError as error:
        raise FixtureError("current release state is not ascii") from error
    # require one newline-terminated release identity
    if (
        release_state != f"{release}\n".encode("ascii")
        or RELEASE_RE.fullmatch(release) is None
    ):
        raise FixtureError("current release state is invalid")
    environment_path = deployment_root / "deploy" / "releases" / f"{release}.env"
    active_environment = deployment_root / "deploy" / "state" / "active.env"
    try:
        active_metadata = active_environment.lstat()
        active_target = os.readlink(active_environment)
    except OSError as error:
        raise FixtureError("active release environment is unavailable") from error
    # bind the installed active symlink to the selected release environment
    if (
        not stat.S_ISLNK(active_metadata.st_mode)
        or active_target != f"../releases/{release}.env"
        or active_environment.resolve(strict=True) != environment_path
    ):
        raise FixtureError("active release environment differs from current release")
    environment, environment_sha256 = read_deployment_environment(environment_path)
    # bind the environment to the selected current release
    if environment["WEATHER_RELEASE"] != release:
        raise FixtureError("current release environment identity differs")
    state_paths = {
        "candidateCatalog": deployment_root
        / "deploy"
        / "state"
        / "adjustment-candidate-catalog.json",
        "captureEpoch": deployment_root
        / "deploy"
        / "state"
        / "adjustment-capture-epoch.json",
    }
    state = {}
    state["currentRelease"] = {
        "bytes": release_state,
        "path": release_state_path,
        "sha256": bytes_sha256(release_state),
    }
    state["activeEnvironment"] = {
        "path": active_environment,
        "target": active_target,
    }
    # snapshot both installed non-secret state inputs
    for name, path in state_paths.items():
        raw = read_regular_bytes(path, f"installed {name}", 1024 * 1024, owner=(0, 0))
        state[name] = {"bytes": raw, "path": path, "sha256": bytes_sha256(raw)}
    return environment, environment_sha256, state


# parse one docker inspection response
def one_docker_record(result, description):
    try:
        payload = json.loads(result.stdout.decode("utf-8"))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise FixtureError(f"{description} returned invalid JSON") from error
    # require one exact object
    if (
        not isinstance(payload, list)
        or len(payload) != 1
        or not isinstance(payload[0], dict)
    ):
        raise FixtureError(f"{description} did not identify one object")
    return payload[0]


# validate one local immutable arm64 image
def inspect_image(image_reference, command=run_command):
    # reject tags and mutable references
    if IMAGE_REFERENCE_RE.fullmatch(image_reference) is None:
        raise FixtureError("container image must be an exact sha256 repository digest")
    result = command([DOCKER, "image", "inspect", image_reference], timeout=60)
    record = one_docker_record(result, "image inspection")
    repo_digests = record.get("RepoDigests")
    # bind image id platform and repository digest
    if (
        not IMAGE_ID_RE.fullmatch(record.get("Id", ""))
        or record.get("Os") != "linux"
        or record.get("Architecture") != "arm64"
        or not isinstance(repo_digests, list)
        or image_reference not in repo_digests
    ):
        raise FixtureError("local image identity or ARM64 platform differs")
    return record


# bind one production container to the selected installed image without mutation
def inspect_production_container(name, image_reference, image_id, command=run_command):
    record = one_docker_record(
        command([DOCKER, "container", "inspect", name], timeout=60),
        "production container inspection",
    )
    state = record.get("State")
    # require the exact running healthy installed container identity
    if (
        record.get("Name") != f"/{name}"
        or record.get("Image") != image_id
        or record.get("Config", {}).get("Image") != image_reference
        or not isinstance(state, dict)
        or state.get("Running") is not True
        or state.get("Health", {}).get("Status") != "healthy"
    ):
        raise FixtureError("production container differs from installed image identity")
    return record


# validate the complete detached five-slot fixture set
def inspect_volume_set(marker, handoff, command=run_command):
    prefix = marker["volumeNamespacePrefix"]
    expected_names = [f"{prefix}{suffix}" for suffix in VOLUME_SUFFIXES]
    target = handoff["volumeName"]
    # bind the restored target to one fixed slot
    if target not in expected_names:
        raise FixtureError("restore handoff volume is outside the five-slot set")
    listed = (
        command(
            [
                DOCKER,
                "volume",
                "ls",
                "--quiet",
                "--filter",
                f"label=weather.native-benchmark.fixture-id={marker['fixtureId']}",
                "--filter",
                f"label=weather.native-benchmark.kind={VOLUME_KIND}",
            ],
            timeout=60,
        )
        .stdout.decode("utf-8", errors="strict")
        .splitlines()
    )
    # require exactly the expected five volumes
    if len(listed) != 5 or set(listed) != set(expected_names):
        raise FixtureError("fixture must contain exactly five fixed volumes")
    target_device = None
    # inspect each volume and prove it detached
    for name in expected_names:
        device = Path(marker["volumeRoot"]) / name
        require_path(device, f"fixture volume {name}", "directory")
        require_canonical(device, f"fixture volume {name}")
        # bind every volume to the dedicated filesystem
        if (
            device.parent != Path(marker["volumeRoot"])
            or os.stat(device).st_dev != os.stat(marker["mountpoint"]).st_dev
        ):
            raise FixtureError("fixture volume crosses the dedicated filesystem")
        result = command([DOCKER, "volume", "inspect", name], timeout=60)
        record = one_docker_record(result, "fixture volume inspection")
        expected_labels = {
            "weather.native-benchmark.fixture-id": marker["fixtureId"],
            "weather.native-benchmark.kind": VOLUME_KIND,
        }
        expected_options = {"type": "none", "o": "bind", "device": str(device)}
        # bind labels and the exact host directory
        if (
            record.get("Name") != name
            or record.get("Driver") != "local"
            or record.get("Scope") != "local"
            or record.get("Labels") != expected_labels
            or record.get("Options") != expected_options
        ):
            raise FixtureError("fixture volume identity differs")
        attached = (
            command(
                [
                    DOCKER,
                    "container",
                    "ls",
                    "--all",
                    "--quiet",
                    "--filter",
                    f"volume={name}",
                ],
                timeout=60,
            )
            .stdout.decode("utf-8", errors="strict")
            .strip()
        )
        # require every slot detached before startup
        if attached:
            raise FixtureError("fixture volume is already attached")
        # retain only the restored target
        if name == target:
            target_device = device
        # require every non-target slot empty
        if name != target and any(device.iterdir()):
            raise FixtureError("unused fixture volume is not empty")
    # require one stopped initialized cluster
    if (
        target_device is None
        or str(target_device) != handoff["volumeDevice"]
        or not (target_device / "PG_VERSION").is_file()
        or (target_device / "postmaster.pid").exists()
        or (target_device / "postmaster.pid").is_symlink()
    ):
        raise FixtureError("restored target is not one stopped PostgreSQL cluster")
    return target_device


# ensure one fixture has no runtime objects before startup
def require_empty_runtime(marker, network_name, command=run_command):
    containers = (
        command(
            [
                DOCKER,
                "container",
                "ls",
                "--all",
                "--quiet",
                "--filter",
                f"label=weather.native-benchmark.fixture-id={marker['fixtureId']}",
            ],
            timeout=60,
        )
        .stdout.decode("utf-8", errors="strict")
        .strip()
    )
    networks = (
        command(
            [
                DOCKER,
                "network",
                "ls",
                "--quiet",
                "--filter",
                f"name=^{network_name}$",
            ],
            timeout=60,
        )
        .stdout.decode("utf-8", errors="strict")
        .strip()
    )
    # reject any stale or concurrent fixture runtime
    if containers or networks:
        raise FixtureError("fixture runtime is not empty")


# create one API-readable copy without retaining production mounts
def write_snapshot(path, raw, uid, gid):
    descriptor = os.open(
        path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o400
    )
    try:
        offset = 0
        # complete every bounded snapshot write
        while offset < len(raw):
            written = os.write(descriptor, raw[offset:])
            # reject a stalled snapshot write
            if written <= 0:
                raise FixtureError("snapshot write made no progress")
            offset += written
        os.fsync(descriptor)
        os.fchown(descriptor, uid, gid)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


# build one constrained PostgreSQL create command
def build_postgres_command(marker, handoff, image, network_name, container_name):
    labels = [
        "--label",
        f"weather.native-benchmark.fixture-id={marker['fixtureId']}",
        "--label",
        f"weather.native-benchmark.kind={KIND}",
        "--label",
        "weather.native-benchmark.role=node-baseline-postgres",
    ]
    return [
        DOCKER,
        "create",
        "--pull",
        "never",
        "--name",
        container_name,
        *labels,
        "--network",
        network_name,
        "--network-alias",
        "postgres",
        "--restart",
        "no",
        "--read-only",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "--pids-limit",
        "128",
        "--memory",
        "512m",
        "--cpus",
        "1",
        "--user",
        "999:999",
        "--log-driver",
        "json-file",
        "--log-opt",
        "max-file=3",
        "--log-opt",
        "max-size=10m",
        "--tmpfs",
        "/tmp:size=64m,mode=1777",
        "--tmpfs",
        "/var/run/postgresql:size=16m,mode=0755,uid=999,gid=999",
        "--mount",
        f"type=volume,src={handoff['volumeName']},dst=/var/lib/postgresql/data,volume-nocopy",
        "--env",
        f"POSTGRES_DB={handoff['databaseName']}",
        "--env",
        "POSTGRES_USER=postgres",
        "--env",
        "PGDATA=/var/lib/postgresql/data",
        "--entrypoint",
        "/usr/local/bin/docker-entrypoint.sh",
        image,
        "postgres",
        "-c",
        "listen_addresses=*",
    ]


# build one constrained read-only API create command
def build_api_command(
    marker,
    handoff,
    environment,
    image,
    network_name,
    container_name,
    host_port,
    api_secret,
    catalog,
    capture,
):
    variables = {
        "WEATHER_API_PORT": "3001",
        "WEATHER_DATABASE_APPLICATION_NAME": "weather-native-node-baseline-api",
        "WEATHER_DATABASE_HOST": "postgres",
        "WEATHER_DATABASE_PORT": "5432",
        "WEATHER_DATABASE_NAME": handoff["databaseName"],
        "WEATHER_DATABASE_SSL": "false",
        "WEATHER_DATABASE_LOCK_TIMEOUT_MS": "10000",
        "WEATHER_DATABASE_STATEMENT_TIMEOUT_MS": "30000",
        "WEATHER_DATABASE_USER": "weather_api",
        "WEATHER_DATABASE_PASSWORD_FILE": "/run/secrets/weather_api_password",
        "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH": environment.get(
            "WEATHER_FORECAST_ADJUSTMENT_TEMPERATURE_CANARY_KILL_SWITCH",
            "1",
        ),
        "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH": environment.get(
            "WEATHER_FORECAST_ADJUSTMENT_WIND_CANARY_KILL_SWITCH",
            "0",
        ),
        "WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256": environment.get(
            "WEATHER_MIGRATION_AUTHORIZATION_HISTORY_SHA256",
            "",
        ),
        "WEATHER_MIGRATION_AUTHORIZATION_RELEASE": environment.get(
            "WEATHER_MIGRATION_AUTHORIZATION_RELEASE",
            "",
        ),
        "WEATHER_PUBLIC_STATIONS_CONFIG_PATH": "/opt/weather/config/public-stations/stations.json",
        "WEATHER_RELEASE": environment["WEATHER_RELEASE"],
    }
    command = [
        DOCKER,
        "create",
        "--pull",
        "never",
        "--name",
        container_name,
        "--label",
        f"weather.native-benchmark.fixture-id={marker['fixtureId']}",
        "--label",
        f"weather.native-benchmark.kind={KIND}",
        "--label",
        "weather.native-benchmark.role=node-baseline-api",
        "--network",
        network_name,
        "--network-alias",
        "api",
        "--restart",
        "no",
        "--read-only",
        "--init",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "--pids-limit",
        "96",
        "--memory",
        "256m",
        "--cpus",
        "0.5",
        "--user",
        "10002:10002",
        "--log-driver",
        "json-file",
        "--log-opt",
        "max-file=3",
        "--log-opt",
        "max-size=10m",
        "--tmpfs",
        "/tmp:size=32m,mode=1777",
        "--publish",
        f"127.0.0.1:{host_port}:3001",
        "--mount",
        f"type=bind,src={api_secret},dst=/run/secrets/weather_api_password,readonly",
        "--mount",
        f"type=bind,src={catalog},dst=/run/weather/adjustment-candidate-catalog.json,readonly",
        "--mount",
        f"type=bind,src={capture},dst=/run/weather/adjustment-capture-epoch.json,readonly",
    ]
    # append the closed environment in stable order
    for name in sorted(variables):
        command.extend(["--env", f"{name}={variables[name]}"])
    command.extend([image, "node", "apps/api/dist/main.js"])
    return command


# build one constrained credential-free Node web create command
def build_web_command(marker, environment, image, network_name, container_name):
    variables = {
        "HOST": "0.0.0.0",
        "PORT": "3000",
        "WEATHER_ADJUSTMENT_EVIDENCE_ROOT": (
            "/run/weather-web-state/adjustment-evidence"
        ),
        "WEATHER_ADMIN_AUTH_PATH": "/run/weather-web-state/admin-auth.json",
        "WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH": (
            "/run/weather-web-state/admin-bootstrap-disabled"
        ),
        "WEATHER_API_ORIGIN": "http://api:3001",
        "WEATHER_PROPERTY_SENSOR_LAYOUT_PATH": (
            "/run/weather-web-state/property-sensor-layout.json"
        ),
        "WEATHER_RELEASE": environment["WEATHER_RELEASE"],
        "WEATHER_SITE_CONFIG_PATH": "/opt/weather/config/sites/ballydidean.json",
        "WEATHER_XWEATHER_CLIENT_ID_FILE": (
            "/run/weather-web-state/xweather-client-id-disabled"
        ),
        "WEATHER_XWEATHER_CLIENT_SECRET_FILE": (
            "/run/weather-web-state/xweather-client-secret-disabled"
        ),
        "WEATHER_XWEATHER_MAP_ORIGIN": "http://127.0.0.1:9",
        "WEATHER_XWEATHER_USAGE_PATH": "/run/weather-web-state/xweather-usage.json",
    }
    command = [
        DOCKER,
        "create",
        "--pull",
        "never",
        "--name",
        container_name,
        "--label",
        f"weather.native-benchmark.fixture-id={marker['fixtureId']}",
        "--label",
        f"weather.native-benchmark.kind={KIND}",
        "--label",
        "weather.native-benchmark.role=node-baseline-web",
        "--network",
        network_name,
        "--restart",
        "no",
        "--read-only",
        "--init",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "--pids-limit",
        "64",
        "--memory",
        "384m",
        "--memory-reservation",
        "64m",
        "--cpus",
        "0.25",
        "--user",
        "10002:10002",
        "--log-driver",
        "json-file",
        "--log-opt",
        "max-file=3",
        "--log-opt",
        "max-size=10m",
        "--tmpfs",
        "/tmp:size=16m,mode=1777",
        "--tmpfs",
        "/run/weather-web-state:size=16m,mode=0700,uid=10002,gid=10002",
    ]
    # append the closed credential-free environment in stable order
    for name in sorted(variables):
        command.extend(["--env", f"{name}={variables[name]}"])
    command.extend([image, "node", "deploy/scripts/web-server.mjs"])
    return command


# bind image production and fixture web command inheritance before creation
def validate_web_launch_identity(
    image_record, production_record, create_command, image_reference
):
    expected_command = ["node", "deploy/scripts/web-server.mjs"]
    image_config = (
        image_record.get("Config") if isinstance(image_record, dict) else None
    )
    production_config = (
        production_record.get("Config") if isinstance(production_record, dict) else None
    )
    image_config_record = image_config if isinstance(image_config, dict) else {}
    production_config_record = (
        production_config if isinstance(production_config, dict) else {}
    )
    image_entrypoint = image_config_record.get("Entrypoint")
    # require one concrete immutable image entrypoint to inherit
    if (
        not isinstance(image_entrypoint, list)
        or not image_entrypoint
        or any(not isinstance(value, str) or not value for value in image_entrypoint)
    ):
        raise FixtureError("installed web image entrypoint identity differs")
    require_fixed_controls(
        "fixture web launch identity differs",
        (
            ("Image.Config.Cmd", image_config_record.get("Cmd"), (expected_command,)),
            (
                "Production.Config.Cmd",
                production_config_record.get("Cmd"),
                (expected_command,),
            ),
            (
                "Production.Config.Entrypoint",
                production_config_record.get("Entrypoint"),
                (image_entrypoint,),
            ),
            (
                "Fixture.CommandTail",
                create_command[-3:],
                ([image_reference, *expected_command],),
            ),
            ("Fixture.EntrypointOverride", "--entrypoint" in create_command, (False,)),
        ),
    )
    return image_entrypoint


# validate one fixture container inspection record
def validate_container_record(record, container_id, name, marker, image_id, role):
    expected_labels = fixture_labels(marker, role)
    labels = record.get("Config", {}).get("Labels")
    benchmark_labels = (
        {
            label: value
            for label, value in labels.items()
            if label.startswith("weather.native-benchmark.")
        }
        if isinstance(labels, dict)
        else None
    )
    # bind immutable ownership before lifecycle actions
    if (
        record.get("Id") != container_id
        or record.get("Name") != f"/{name}"
        or record.get("Image") != image_id
        or benchmark_labels != expected_labels
    ):
        raise FixtureError("fixture container ownership proof differs")
    return record


# validate one created fixture container identity
def inspect_container(container_id, name, marker, image_id, role, command=run_command):
    # require one immutable container id
    if re.fullmatch(r"[0-9a-f]{64}", container_id) is None:
        raise FixtureError("created container id is invalid")
    record = one_docker_record(
        command([DOCKER, "container", "inspect", container_id], timeout=60),
        "fixture container inspection",
    )
    return validate_container_record(record, container_id, name, marker, image_id, role)


# require exact values for a closed set of nonsecret controls
def require_fixed_controls(message, controls):
    mismatches = {}
    # report only explicitly named fixed controls that differ
    for field, observed, accepted in controls:
        # retain every documented exact equivalent for diagnostics
        if not any(observed == expected for expected in accepted):
            expected = accepted[0] if len(accepted) == 1 else {"oneOf": accepted}
            mismatches[field] = {"expected": expected, "observed": observed}
    # fail with one stable machine-readable difference object
    if mismatches:
        raise FixtureError(
            f"{message}: {json.dumps(mismatches, sort_keys=True, separators=(',', ':'))}"
        )


# require one constrained container host configuration
def validate_host_constraints(
    record, network_name, memory_bytes, nano_cpus, pids_limit
):
    host = record.get("HostConfig")
    # diagnose an invalid host object without exposing the inspection record
    if not isinstance(host, dict):
        raise FixtureError(
            "fixture container isolation or resource limits differ: "
            + json.dumps(
                {
                    "HostConfig": {
                        "expected": "object",
                        "observed": type(host).__name__,
                    }
                },
                sort_keys=True,
                separators=(",", ":"),
            )
        )
    expected_log = {
        "Type": "json-file",
        "Config": {"max-file": "3", "max-size": "10m"},
    }
    security_equivalents = (
        ["no-new-privileges"],
        ["no-new-privileges=true"],
        ["no-new-privileges:true"],
    )
    require_fixed_controls(
        "fixture container isolation or resource limits differ",
        (
            ("HostConfig.CapDrop", host.get("CapDrop"), (["ALL"],)),
            ("HostConfig.LogConfig", host.get("LogConfig"), (expected_log,)),
            ("HostConfig.Memory", host.get("Memory"), (memory_bytes,)),
            ("HostConfig.NanoCpus", host.get("NanoCpus"), (nano_cpus,)),
            ("HostConfig.NetworkMode", host.get("NetworkMode"), (network_name,)),
            ("HostConfig.PidsLimit", host.get("PidsLimit"), (pids_limit,)),
            ("HostConfig.ReadonlyRootfs", host.get("ReadonlyRootfs"), (True,)),
            (
                "HostConfig.RestartPolicy",
                host.get("RestartPolicy"),
                ({"Name": "no", "MaximumRetryCount": 0},),
            ),
            ("HostConfig.SecurityOpt", host.get("SecurityOpt"), security_equivalents),
        ),
    )
    return host


# require the created PostgreSQL container to expose only its restored volume
def validate_postgres_container(record, network_name, volume_name):
    host = validate_host_constraints(
        record,
        network_name,
        512 * 1024**2,
        1_000_000_000,
        128,
    )
    mounts = record.get("Mounts")
    mount = mounts[0] if isinstance(mounts, list) and mounts else None
    mount_record = mount if isinstance(mount, dict) else {}
    config = record.get("Config")
    config_record = config if isinstance(config, dict) else {}
    require_fixed_controls(
        "fixture PostgreSQL mounts or command differ",
        (
            ("Args", record.get("Args"), (["postgres", "-c", "listen_addresses=*"],)),
            ("Config.User", config_record.get("User"), ("999:999",)),
            ("HostConfig.PortBindings", host.get("PortBindings"), ({}, None)),
            (
                "Mounts.count",
                len(mounts) if isinstance(mounts, list) else type(mounts).__name__,
                (1,),
            ),
            (
                "Mounts[0].Destination",
                mount_record.get("Destination"),
                ("/var/lib/postgresql/data",),
            ),
            ("Mounts[0].Name", mount_record.get("Name"), (volume_name,)),
            ("Mounts[0].RW", mount_record.get("RW"), (True,)),
            ("Mounts[0].Type", mount_record.get("Type"), ("volume",)),
            ("Path", record.get("Path"), ("/usr/local/bin/docker-entrypoint.sh",)),
        ),
    )


# require the created API container to expose only loopback and snapshots
def validate_api_container(
    record, network_name, host_port, api_secret, catalog, capture
):
    host = validate_host_constraints(
        record,
        network_name,
        256 * 1024**2,
        500_000_000,
        96,
    )
    expected_ports = {
        "3001/tcp": [{"HostIp": "127.0.0.1", "HostPort": str(host_port)}],
    }
    expected_mounts = [
        {
            "Destination": "/run/secrets/weather_api_password",
            "RW": False,
            "Source": str(api_secret),
            "Type": "bind",
        },
        {
            "Destination": "/run/weather/adjustment-candidate-catalog.json",
            "RW": False,
            "Source": str(catalog),
            "Type": "bind",
        },
        {
            "Destination": "/run/weather/adjustment-capture-epoch.json",
            "RW": False,
            "Source": str(capture),
            "Type": "bind",
        },
    ]
    mounts = record.get("Mounts")
    observed_mounts = []
    # retain only fixed nonsecret mount-control fields for diagnostics
    if isinstance(mounts, list):
        # normalize every mount without exposing any mounted content
        for mount in mounts:
            # report an invalid mount element by type only
            if not isinstance(mount, dict):
                observed_mounts.append({"InvalidType": type(mount).__name__})
                continue
            observed_mounts.append(
                {
                    "Destination": mount.get("Destination"),
                    "RW": mount.get("RW"),
                    "Source": mount.get("Source"),
                    "Type": mount.get("Type"),
                }
            )
    config = record.get("Config")
    config_record = config if isinstance(config, dict) else {}
    environment = config_record.get("Env")
    forbidden_environment = []
    # expose names only for prohibited write-side listener controls
    if isinstance(environment, list):
        # inspect only string environment assignments
        for value in environment:
            # retain prohibited variable names without their values
            if isinstance(value, str) and value.startswith(
                (
                    "WEATHER_ADJUSTMENT_MAINTENANCE_INTERNAL_PORT=",
                    "WEATHER_ADJUSTMENT_MAINTENANCE_ARCHIVE_ORIGIN=",
                )
            ):
                forbidden_environment.append(value.split("=", 1)[0])
    require_fixed_controls(
        "fixture API publication mounts or command differ",
        (
            (
                "Config.Cmd",
                config_record.get("Cmd"),
                (["node", "apps/api/dist/main.js"],),
            ),
            ("Config.Env.type", type(environment).__name__, ("list",)),
            (
                "Config.Env.writeSideControls",
                sorted(forbidden_environment),
                ([],),
            ),
            ("Config.User", config_record.get("User"), ("10002:10002",)),
            ("HostConfig.Init", host.get("Init"), (True,)),
            ("HostConfig.PortBindings", host.get("PortBindings"), (expected_ports,)),
            (
                "Mounts.controls",
                sorted(
                    observed_mounts, key=lambda item: json.dumps(item, sort_keys=True)
                ),
                (
                    sorted(
                        expected_mounts,
                        key=lambda item: json.dumps(item, sort_keys=True),
                    ),
                ),
            ),
            (
                "Mounts.count",
                len(mounts) if isinstance(mounts, list) else type(mounts).__name__,
                (3,),
            ),
        ),
    )


# require the Node web fixture to remain credential-free and tmpfs-only
def validate_web_container(record, network_name, environment):
    host = validate_host_constraints(
        record,
        network_name,
        384 * 1024**2,
        250_000_000,
        64,
    )
    expected_environment = {
        "WEATHER_ADJUSTMENT_EVIDENCE_ROOT": (
            "/run/weather-web-state/adjustment-evidence"
        ),
        "WEATHER_ADMIN_AUTH_PATH": "/run/weather-web-state/admin-auth.json",
        "WEATHER_ADMIN_BOOTSTRAP_TOKEN_PATH": (
            "/run/weather-web-state/admin-bootstrap-disabled"
        ),
        "WEATHER_API_ORIGIN": "http://api:3001",
        "WEATHER_PROPERTY_SENSOR_LAYOUT_PATH": (
            "/run/weather-web-state/property-sensor-layout.json"
        ),
        "WEATHER_RELEASE": environment["WEATHER_RELEASE"],
        "WEATHER_SITE_CONFIG_PATH": "/opt/weather/config/sites/ballydidean.json",
        "WEATHER_XWEATHER_CLIENT_ID_FILE": (
            "/run/weather-web-state/xweather-client-id-disabled"
        ),
        "WEATHER_XWEATHER_CLIENT_SECRET_FILE": (
            "/run/weather-web-state/xweather-client-secret-disabled"
        ),
        "WEATHER_XWEATHER_MAP_ORIGIN": "http://127.0.0.1:9",
        "WEATHER_XWEATHER_USAGE_PATH": "/run/weather-web-state/xweather-usage.json",
    }
    config = record.get("Config")
    config_record = config if isinstance(config, dict) else {}
    assignments = config_record.get("Env")
    weather_environment = {}
    runtime_environment = {}
    # retain only nonsecret Weather configuration for exact comparison
    if isinstance(assignments, list):
        # parse every well-formed Weather assignment once
        for assignment in assignments:
            # ignore malformed entries before selecting fixed runtime names
            if not isinstance(assignment, str):
                continue
            name, separator, value = assignment.partition("=")
            # retain the listener and production-mode controls separately
            if name in {"HOST", "NODE_ENV", "PORT"} and separator == "=":
                runtime_environment[name] = value
            # ignore image runtime variables outside the Weather namespace
            if not name.startswith("WEATHER_"):
                continue
            # make malformed or duplicate Weather variables fail comparison
            if separator != "=" or name in weather_environment:
                weather_environment[name] = None
                continue
            weather_environment[name] = value
    revision = weather_environment.pop("WEATHER_WEB_REVISION", None)
    revision_valid = revision == "development" or (
        isinstance(revision, str)
        and re.fullmatch(r"[0-9a-f]{40}", revision) is not None
    )
    mounts = record.get("Mounts")
    expected_tmpfs = {
        "/run/weather-web-state": ("size=16m,mode=0700,uid=10002,gid=10002"),
        "/tmp": "size=16m,mode=1777",
    }
    # allow only Docker's optional explicit read-write tmpfs normalization
    expected_tmpfs_rw = {
        path: f"rw,{options}" for path, options in expected_tmpfs.items()
    }
    require_fixed_controls(
        "fixture web isolation mounts or command differ",
        (
            (
                "Config.Cmd",
                config_record.get("Cmd"),
                (["node", "deploy/scripts/web-server.mjs"],),
            ),
            ("Config.Env.fixtureWeather", weather_environment, (expected_environment,)),
            (
                "Config.Env.runtime",
                runtime_environment,
                ({"HOST": "0.0.0.0", "NODE_ENV": "production", "PORT": "3000"},),
            ),
            ("Config.Env.type", type(assignments).__name__, ("list",)),
            ("Config.Env.webRevisionValid", revision_valid, (True,)),
            ("Config.User", config_record.get("User"), ("10002:10002",)),
            ("HostConfig.Init", host.get("Init"), (True,)),
            (
                "HostConfig.MemoryReservation",
                host.get("MemoryReservation"),
                (64 * 1024**2,),
            ),
            ("HostConfig.PortBindings", host.get("PortBindings"), ({}, None)),
            (
                "HostConfig.Tmpfs",
                host.get("Tmpfs"),
                (expected_tmpfs, expected_tmpfs_rw),
            ),
            ("Mounts", mounts, ([],)),
        ),
    )


# validate one created fixture network identity
def inspect_network(network_id, network_name, fixture_id, command=run_command):
    # require one immutable network id
    if re.fullmatch(r"[0-9a-f]{64}", network_id) is None:
        raise FixtureError("created network id is invalid")
    record = one_docker_record(
        command([DOCKER, "network", "inspect", network_id], timeout=60),
        "fixture network inspection",
    )
    expected_labels = {
        "weather.native-benchmark.fixture-id": fixture_id,
        "weather.native-benchmark.kind": KIND,
        "weather.native-benchmark.role": "node-baseline-network",
    }
    # bind an internal local bridge with exact labels
    if (
        record.get("Id") != network_id
        or record.get("Name") != network_name
        or record.get("Driver") != "bridge"
        or record.get("Scope") != "local"
        or record.get("Internal") is not True
        or record.get("Labels") != expected_labels
    ):
        raise FixtureError("fixture network ownership proof differs")
    return record


# wait for the final PostgreSQL server
def wait_postgres(
    container_id,
    timeout_seconds,
    command=run_command,
    monotonic=time.monotonic,
    sleeper=time.sleep,
):
    deadline = monotonic() + timeout_seconds
    # poll only the owned container socket
    while monotonic() < deadline:
        result = command(
            [
                DOCKER,
                "exec",
                container_id,
                "sh",
                "-c",
                'test "$(cat /proc/1/comm)" = postgres && exec pg_isready "$@"',
                "fixture-readiness",
                "--host",
                "/var/run/postgresql",
                "--username",
                "postgres",
                "--dbname",
                "weather",
            ],
            timeout=10,
            check=False,
        )
        # accept only the final ready postmaster
        if result.returncode == 0:
            return
        sleeper(0.5)
    raise FixtureError("fixture PostgreSQL did not become ready")


# wait for API readiness backed by the restored database
def wait_api(
    container_id,
    timeout_seconds,
    command=run_command,
    monotonic=time.monotonic,
    sleeper=time.sleep,
):
    probe = (
        "fetch('http://127.0.0.1:3001/api/v1/health')"
        ".then(async r=>{const j=await r.json();"
        "if(j?.data?.database!=='ready'||j?.data?.migration?.status!=='current')process.exit(2)})"
        ".catch(()=>process.exit(3))"
    )
    deadline = monotonic() + timeout_seconds
    # poll only the owned API listener
    while monotonic() < deadline:
        result = command(
            [DOCKER, "exec", container_id, "node", "-e", probe],
            timeout=10,
            check=False,
        )
        # accept only a database-backed ready API
        if result.returncode == 0:
            return
        sleeper(0.5)
    raise FixtureError("fixture Node API did not become database-ready")


# validate one running container endpoint on the exact owned bridge
def validate_internal_endpoint(
    record,
    container_id,
    container_name,
    port,
    network_record,
    network_id,
    network_name,
    expected_members,
    required_alias=None,
):
    networks = record.get("NetworkSettings", {}).get("Networks")
    # require the running container to join only the owned fixture network
    if (
        record.get("State", {}).get("Running") is not True
        or not isinstance(networks, dict)
        or set(networks) != {network_name}
        or not isinstance(networks.get(network_name), dict)
    ):
        raise FixtureError("fixture internal network attachment differs")
    endpoint = networks[network_name]
    address_text = endpoint.get("IPAddress")
    prefix_length = endpoint.get("IPPrefixLen")
    endpoint_id = endpoint.get("EndpointID")
    aliases = endpoint.get("Aliases")
    # parse only one concrete Docker-assigned IPv4 endpoint
    try:
        address = ipaddress.ip_address(address_text)
    except ValueError as error:
        raise FixtureError("fixture internal IPv4 address is invalid") from error
    # bind the endpoint to the inspected immutable network
    if (
        address.version != 4
        or endpoint.get("NetworkID") != network_id
        or not isinstance(prefix_length, int)
        or not 0 <= prefix_length <= 32
        or re.fullmatch(r"[0-9a-f]{64}", endpoint_id or "") is None
        or (
            required_alias is not None
            and (not isinstance(aliases, list) or required_alias not in aliases)
        )
    ):
        raise FixtureError("fixture internal network endpoint differs")
    ipam = network_record.get("IPAM")
    configurations = ipam.get("Config") if isinstance(ipam, dict) else None
    matching_subnets = []
    # parse only explicit IPv4 subnets from the owned bridge IPAM record
    if isinstance(configurations, list):
        # test every declared IPAM subnet for the exact endpoint address
        for configuration in configurations:
            # ignore malformed entries so the closed match count fails
            if not isinstance(configuration, dict):
                continue
            subnet_text = configuration.get("Subnet")
            # ignore absent or non-string subnet controls
            if not isinstance(subnet_text, str):
                continue
            try:
                subnet = ipaddress.ip_network(subnet_text, strict=True)
            except ValueError:
                continue
            # retain only the matching IPv4 allocation subnet
            if subnet.version == 4 and address in subnet:
                matching_subnets.append(subnet)
    # require one unambiguous subnet whose prefix matches the endpoint
    if len(matching_subnets) != 1 or matching_subnets[0].prefixlen != prefix_length:
        raise FixtureError("fixture address is outside exact bridge IPAM subnet")
    containers = network_record.get("Containers")
    # require only the expected immutable services on the fixture bridge
    if not isinstance(containers, dict) or set(containers) != set(expected_members):
        raise FixtureError("fixture internal network membership differs")
    member = containers.get(container_id)
    expected_cidr = f"{address}/{prefix_length}"
    # bind every immutable member name and this address to network inspection
    if (
        not isinstance(member, dict)
        or member.get("Name") != container_name
        or member.get("IPv4Address") != expected_cidr
        or any(
            not isinstance(containers.get(identity), dict)
            or containers[identity].get("Name") != name
            for identity, name in expected_members.items()
        )
    ):
        raise FixtureError("fixture internal network member identity differs")
    return {
        "endpointId": endpoint_id,
        "ipv4": str(address),
        "port": port,
        "subnet": str(matching_subnets[0]),
    }


# prove the running API has one exact internal bridge endpoint
def inspect_api_internal_endpoint(
    api_id,
    api_name,
    postgres_id,
    postgres_name,
    marker,
    image_id,
    network_id,
    network_name,
    command=run_command,
):
    api_record = inspect_container(
        api_id,
        api_name,
        marker,
        image_id,
        "node-baseline-api",
        command,
    )
    network_record = inspect_network(
        network_id, network_name, marker["fixtureId"], command
    )
    return validate_internal_endpoint(
        api_record,
        api_id,
        api_name,
        3001,
        network_record,
        network_id,
        network_name,
        {api_id: api_name, postgres_id: postgres_name},
        required_alias="api",
    )


# prove the running web and API have exact internal bridge endpoints
def inspect_web_internal_endpoint(
    web_id,
    web_name,
    web_image_id,
    api_id,
    api_name,
    api_image_id,
    postgres_id,
    postgres_name,
    marker,
    network_id,
    network_name,
    command=run_command,
):
    web_record = inspect_container(
        web_id,
        web_name,
        marker,
        web_image_id,
        "node-baseline-web",
        command,
    )
    api_record = inspect_container(
        api_id,
        api_name,
        marker,
        api_image_id,
        "node-baseline-api",
        command,
    )
    network_record = inspect_network(
        network_id, network_name, marker["fixtureId"], command
    )
    members = {
        api_id: api_name,
        postgres_id: postgres_name,
        web_id: web_name,
    }
    validate_internal_endpoint(
        api_record,
        api_id,
        api_name,
        3001,
        network_record,
        network_id,
        network_name,
        members,
        required_alias="api",
    )
    return validate_internal_endpoint(
        web_record,
        web_id,
        web_name,
        3000,
        network_record,
        network_id,
        network_name,
        members,
    )


# prove the host can read the database-backed API over its internal IPv4
def probe_internal_api_from_host(
    address, connection_factory=http.client.HTTPConnection
):
    connection = connection_factory(address, 3001, timeout=5)
    try:
        connection.request(
            "GET",
            "/api/v1/health",
            headers={"Accept": "application/json", "Connection": "close"},
        )
        response = connection.getresponse()
        raw = response.read(65537)
    except (OSError, http.client.HTTPException) as error:
        raise FixtureError("fixture API internal host health probe failed") from error
    finally:
        connection.close()
    # reject unbounded or malformed health responses
    if len(raw) > 65536:
        raise FixtureError("fixture API internal host health response is too large")
    try:
        payload = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise FixtureError(
            "fixture API internal host health response is invalid"
        ) from error
    data = payload.get("data") if isinstance(payload, dict) else None
    migration = data.get("migration") if isinstance(data, dict) else None
    # require the same database-backed readiness contract as the in-container probe
    if (
        response.status != 200
        or not isinstance(data, dict)
        or data.get("database") != "ready"
        or not isinstance(migration, dict)
        or migration.get("status") != "current"
    ):
        raise FixtureError("fixture API internal host health is not ready")
    return {
        "database": "ready",
        "migrationStatus": "current",
        "path": "/api/v1/health",
        "status": response.status,
    }


# read one bounded internal web response without proxy or DNS resolution
def read_internal_web_response(
    address, path, maximum_bytes, connection_factory=http.client.HTTPConnection
):
    connection = connection_factory(address, 3000, timeout=5)
    try:
        connection.request(
            "GET",
            path,
            headers={"Accept": "*/*", "Connection": "close"},
        )
        response = connection.getresponse()
        raw = response.read(maximum_bytes + 1)
        content_type = response.getheader("Content-Type")
    except (OSError, http.client.HTTPException) as error:
        raise FixtureError("fixture web internal host probe failed") from error
    finally:
        connection.close()
    # reject every response beyond its fixed public-read ceiling
    if len(raw) > maximum_bytes:
        raise FixtureError("fixture web internal host response is too large")
    return response.status, content_type, raw


# prove the API edge GET without warming any ordinary page target
def probe_internal_web_from_host(
    address, connection_factory=http.client.HTTPConnection
):
    edge_status, edge_content_type, edge_raw = read_internal_web_response(
        address, "/api/v1/health", 65536, connection_factory
    )
    try:
        edge_payload = json.loads(edge_raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise FixtureError("fixture web API edge response is invalid") from error
    edge_data = edge_payload.get("data") if isinstance(edge_payload, dict) else None
    edge_migration = edge_data.get("migration") if isinstance(edge_data, dict) else None
    # require the edge GET to reach the fixture API alias and restored database
    if (
        edge_status != 200
        or edge_content_type != "application/json; charset=utf-8"
        or not isinstance(edge_data, dict)
        or edge_data.get("database") != "ready"
        or not isinstance(edge_migration, dict)
        or edge_migration.get("status") != "current"
    ):
        raise FixtureError("fixture web API edge is not ready")
    return {
        "apiEdgePath": "/api/v1/health",
        "apiEdgeStatus": edge_status,
        "database": "ready",
        "migrationStatus": "current",
    }


# wait boundedly for only the credential-free API edge surface
def wait_internal_web_from_host(
    address,
    timeout_seconds,
    connection_factory=http.client.HTTPConnection,
    monotonic=time.monotonic,
    sleeper=time.sleep,
):
    deadline = monotonic() + timeout_seconds
    last_error = None
    # poll only the verified internal web IPv4
    while monotonic() < deadline:
        try:
            return probe_internal_web_from_host(address, connection_factory)
        except FixtureError as error:
            last_error = error
        sleeper(0.5)
    raise FixtureError("fixture web did not become publicly readable") from last_error


# stop and remove one proven owned container
def remove_container(
    container_id, name, marker, image_id, role, stop_seconds, command=run_command
):
    record = inspect_container(container_id, name, marker, image_id, role, command)
    # stop only a running owned container
    if record.get("State", {}).get("Running") is True:
        command(
            [DOCKER, "stop", "--time", str(stop_seconds), container_id],
            timeout=stop_seconds + 30,
        )
    stopped = inspect_container(container_id, name, marker, image_id, role, command)
    # require a stopped container before removal
    if stopped.get("State", {}).get("Running") is not False:
        raise FixtureError("fixture container did not stop")
    command([DOCKER, "rm", container_id], timeout=60)


# resolve one planned or created fixture container to its immutable id
def resolve_container_resource(resource, marker, expected_role, command=run_command):
    role_suffixes = {
        "node-baseline-api": "api",
        "node-baseline-postgres": "postgres",
        "node-baseline-web": "web",
    }
    suffix = role_suffixes.get(expected_role)
    # reject every role outside the fixed lifecycle set
    if suffix is None:
        raise FixtureError("fixture container journal role is unsupported")
    expected_name = f"weather-native-{marker['fixtureId']}-{suffix}"
    expected_keys = {"state", "name", "imageId", "labels", "role"}
    # require one closed planned or created journal record
    if (
        not isinstance(resource, dict)
        or resource.get("state") not in {"planned", "created"}
        or resource.get("name") != expected_name
        or resource.get("role") != expected_role
        or resource.get("labels") != fixture_labels(marker, expected_role)
        or IMAGE_ID_RE.fullmatch(resource.get("imageId", "")) is None
        or set(resource) not in (expected_keys, expected_keys | {"id"})
        or (resource["state"] == "planned" and "id" in resource)
        or (resource["state"] == "created" and set(resource) != expected_keys | {"id"})
    ):
        raise FixtureError("fixture container journal identity differs")
    # use the returned immutable id when create completed normally
    if resource["state"] == "created":
        inspect_container(
            resource["id"],
            resource["name"],
            marker,
            resource["imageId"],
            expected_role,
            command,
        )
        return resource["id"]
    listed = (
        command(
            [
                DOCKER,
                "container",
                "ls",
                "--all",
                "--quiet",
                "--no-trunc",
                "--filter",
                f"name=^/{resource['name']}$",
            ],
            timeout=60,
        )
        .stdout.decode("ascii", errors="strict")
        .splitlines()
    )
    # accept genuine absence after a create failure
    if not listed:
        return None
    # reject ambiguous or malformed deterministic-name matches
    if len(listed) != 1 or re.fullmatch(r"[0-9a-f]{64}", listed[0]) is None:
        raise FixtureError("planned fixture container lookup is ambiguous")
    record = one_docker_record(
        command([DOCKER, "container", "inspect", listed[0]], timeout=60),
        "planned fixture container inspection",
    )
    validate_container_record(
        record,
        listed[0],
        resource["name"],
        marker,
        resource["imageId"],
        expected_role,
    )
    return listed[0]


# resolve one planned or created fixture network to its immutable id
def resolve_network_resource(resource, marker, command=run_command):
    expected_name = f"weather-native-benchmark-{marker['fixtureId']}-node-api"
    expected_keys = {"state", "name", "labels", "role"}
    # require one closed planned or created journal record
    if (
        not isinstance(resource, dict)
        or resource.get("state") not in {"planned", "created"}
        or resource.get("name") != expected_name
        or resource.get("role") != "node-baseline-network"
        or resource.get("labels") != fixture_labels(marker, "node-baseline-network")
        or set(resource) not in (expected_keys, expected_keys | {"id"})
        or (resource["state"] == "planned" and "id" in resource)
        or (resource["state"] == "created" and set(resource) != expected_keys | {"id"})
    ):
        raise FixtureError("fixture network journal identity differs")
    # use the returned immutable id when create completed normally
    if resource["state"] == "created":
        inspect_network(resource["id"], resource["name"], marker["fixtureId"], command)
        return resource["id"]
    listed = (
        command(
            [
                DOCKER,
                "network",
                "ls",
                "--quiet",
                "--no-trunc",
                "--filter",
                f"name=^{resource['name']}$",
            ],
            timeout=60,
        )
        .stdout.decode("ascii", errors="strict")
        .splitlines()
    )
    # accept genuine absence after a create failure
    if not listed:
        return None
    # reject ambiguous or malformed deterministic-name matches
    if len(listed) != 1 or re.fullmatch(r"[0-9a-f]{64}", listed[0]) is None:
        raise FixtureError("planned fixture network lookup is ambiguous")
    inspect_network(listed[0], resource["name"], marker["fixtureId"], command)
    return listed[0]


# remove the exact owned runtime objects in dependency order
def cleanup_runtime(journal, marker, command=run_command):
    errors = []
    resources = journal.get("resources", {})
    web = resources.get("web")
    api = resources.get("api")
    postgres = resources.get("postgres")
    network = resources.get("network")
    # remove the web edge before its API upstream
    if isinstance(web, dict):
        try:
            web_id = resolve_container_resource(
                web, marker, "node-baseline-web", command
            )
            # remove only a proven present web object
            if web_id is not None:
                remove_container(
                    web_id,
                    web["name"],
                    marker,
                    web["imageId"],
                    "node-baseline-web",
                    30,
                    command,
                )
        except (KeyError, OSError, subprocess.SubprocessError, FixtureError) as error:
            errors.append(f"web cleanup: {error}")
    # remove the API before its database
    if isinstance(api, dict):
        try:
            api_id = resolve_container_resource(
                api, marker, "node-baseline-api", command
            )
            # remove only a proven present API object
            if api_id is not None:
                remove_container(
                    api_id,
                    api["name"],
                    marker,
                    api["imageId"],
                    "node-baseline-api",
                    45,
                    command,
                )
        except (KeyError, OSError, subprocess.SubprocessError, FixtureError) as error:
            errors.append(f"api cleanup: {error}")
    # remove PostgreSQL after the API is gone
    if isinstance(postgres, dict):
        try:
            postgres_id = resolve_container_resource(
                postgres, marker, "node-baseline-postgres", command
            )
            # remove only a proven present PostgreSQL object
            if postgres_id is not None:
                remove_container(
                    postgres_id,
                    postgres["name"],
                    marker,
                    postgres["imageId"],
                    "node-baseline-postgres",
                    60,
                    command,
                )
        except (KeyError, OSError, subprocess.SubprocessError, FixtureError) as error:
            errors.append(f"postgres cleanup: {error}")
    # remove only the proven fixture network
    if isinstance(network, dict):
        try:
            network_id = resolve_network_resource(network, marker, command)
            # remove only a proven present network object
            if network_id is not None:
                command([DOCKER, "network", "rm", network_id], timeout=60)
        except (KeyError, OSError, subprocess.SubprocessError, FixtureError) as error:
            errors.append(f"network cleanup: {error}")
    return errors


# remove one owned session directory after runtime cleanup
def remove_session_directory(session_directory):
    require_path(
        session_directory,
        "node API session directory",
        "directory",
        owner=(0, 0),
        mode=0o700,
    )
    # reject nested directories and unexpected files
    allowed = {
        JOURNAL_NAME,
        HANDOFF_NAME,
        "weather_api_password",
        "adjustment-candidate-catalog.json",
        "adjustment-capture-epoch.json",
    }
    # refuse every path outside the closed session file set
    if any(
        path.is_dir() or path.name not in allowed
        for path in session_directory.iterdir()
    ):
        raise FixtureError("node API session contents differ; refusing cleanup")
    # remove only the closed session file set
    for path in list(session_directory.iterdir()):
        require_path(path, "node API session file", "file")
        path.unlink()
    session_directory.rmdir()


# start one isolated fixture runtime
def start(args, command=run_command):
    # require root for Docker and credential custody
    if os.geteuid() != 0:
        raise FixtureError("root is required for fixture lifecycle operations")
    fixture_root = Path(args.fixture_root)
    deployment_root = Path(args.deployment_root)
    profile = args.profile
    # accept only the closed API-only or API-plus-web lifecycle profiles
    if profile not in {PROFILE_API_ONLY, PROFILE_API_WEB}:
        raise FixtureError("fixture runtime profile is unsupported")
    marker, marker_sha256 = validate_marker(fixture_root)
    handoff, restore_handoff_sha256 = validate_restore_handoff(
        fixture_root,
        marker,
        marker_sha256,
    )
    credential_directory = validate_credentials(handoff, fixture_root)
    target_device = inspect_volume_set(marker, handoff, command)
    environment, environment_sha256, installed_state = validate_deployment_root(
        deployment_root
    )
    # require the restored PostgreSQL image to equal the installed image
    if handoff["imageRepoDigest"] != environment["POSTGRES_IMAGE"]:
        raise FixtureError(
            "restored PostgreSQL image differs from the installed release"
        )
    postgres_image = inspect_image(environment["POSTGRES_IMAGE"], command)
    server_image = inspect_image(environment["WEATHER_SERVER_IMAGE"], command)
    web_image = None
    web_production_record = None
    # bind the optional web profile to the installed digest-only image
    if profile == PROFILE_API_WEB:
        web_reference = environment.get("WEATHER_WEB_IMAGE")
        # refuse a tag or missing installed web identity
        if (
            not isinstance(web_reference, str)
            or IMAGE_REFERENCE_RE.fullmatch(web_reference) is None
        ):
            raise FixtureError("deployment web image identity is invalid")
        web_image = inspect_image(web_reference, command)
    # bind the restored image config id
    if postgres_image["Id"] != handoff["imageId"]:
        raise FixtureError("restored PostgreSQL image id differs")
    inspect_production_container(
        "weather-postgres-1",
        environment["POSTGRES_IMAGE"],
        postgres_image["Id"],
        command,
    )
    inspect_production_container(
        "weather-api-1",
        environment["WEATHER_SERVER_IMAGE"],
        server_image["Id"],
        command,
    )
    # bind the optional profile to the exact healthy deployed Node web image
    if web_image is not None:
        web_production_record = inspect_production_container(
            "weather-web-1",
            environment["WEATHER_WEB_IMAGE"],
            web_image["Id"],
            command,
        )
    runtime_root = RUNTIME_BASE / f"weather-native-benchmark-{marker['fixtureId']}"
    require_path(
        runtime_root, "fixture runtime root", "directory", owner=(0, 0), mode=0o700
    )
    require_canonical(runtime_root, "fixture runtime root")
    session_directory = runtime_root / SESSION_DIRECTORY_NAME
    # refuse to overwrite any prior session evidence
    if session_directory.exists() or session_directory.is_symlink():
        raise FixtureError("node API session already exists")
    network_name = f"weather-native-benchmark-{marker['fixtureId']}-node-api"
    postgres_name = f"weather-native-{marker['fixtureId']}-postgres"
    api_name = f"weather-native-{marker['fixtureId']}-api"
    web_name = f"weather-native-{marker['fixtureId']}-web"
    require_empty_runtime(marker, network_name, command)
    session_directory.mkdir(mode=0o700)
    journal_path = session_directory / JOURNAL_NAME
    journal = {
        "contractVersion": SESSION_KIND,
        "fixtureId": marker["fixtureId"],
        "fixtureRoot": str(fixture_root),
        "profile": profile,
        "resources": {},
        "state": "starting",
    }
    write_private_json(journal_path, journal)
    try:
        api_secret = session_directory / "weather_api_password"
        catalog = session_directory / "adjustment-candidate-catalog.json"
        capture = session_directory / "adjustment-capture-epoch.json"
        write_snapshot(
            api_secret,
            read_regular_bytes(
                credential_directory / "weather_api_password",
                "fixture API credential",
                1024,
                owner=(999, 999),
            ),
            10002,
            10002,
        )
        write_snapshot(
            catalog, installed_state["candidateCatalog"]["bytes"], 10002, 10002
        )
        write_snapshot(capture, installed_state["captureEpoch"]["bytes"], 10002, 10002)
        journal["resources"]["network"] = {
            "labels": fixture_labels(marker, "node-baseline-network"),
            "name": network_name,
            "role": "node-baseline-network",
            "state": "planned",
        }
        replace_private_json(journal_path, journal)
        network_result = command(
            [
                DOCKER,
                "network",
                "create",
                "--driver",
                "bridge",
                "--internal",
                "--label",
                f"weather.native-benchmark.fixture-id={marker['fixtureId']}",
                "--label",
                f"weather.native-benchmark.kind={KIND}",
                "--label",
                "weather.native-benchmark.role=node-baseline-network",
                network_name,
            ],
            timeout=60,
        )
        network_id = network_result.stdout.decode("ascii", errors="strict").strip()
        journal["resources"]["network"] = {
            "id": network_id,
            "labels": fixture_labels(marker, "node-baseline-network"),
            "name": network_name,
            "role": "node-baseline-network",
            "state": "created",
        }
        replace_private_json(journal_path, journal)
        inspect_network(network_id, network_name, marker["fixtureId"], command)
        journal["resources"]["postgres"] = {
            "imageId": postgres_image["Id"],
            "labels": fixture_labels(marker, "node-baseline-postgres"),
            "name": postgres_name,
            "role": "node-baseline-postgres",
            "state": "planned",
        }
        replace_private_json(journal_path, journal)
        postgres_result = command(
            build_postgres_command(
                marker,
                handoff,
                environment["POSTGRES_IMAGE"],
                network_name,
                postgres_name,
            ),
            timeout=120,
        )
        postgres_id = postgres_result.stdout.decode("ascii", errors="strict").strip()
        journal["resources"]["postgres"] = {
            "id": postgres_id,
            "imageId": postgres_image["Id"],
            "labels": fixture_labels(marker, "node-baseline-postgres"),
            "name": postgres_name,
            "role": "node-baseline-postgres",
            "state": "created",
        }
        replace_private_json(journal_path, journal)
        postgres_record = inspect_container(
            postgres_id,
            postgres_name,
            marker,
            postgres_image["Id"],
            "node-baseline-postgres",
            command,
        )
        validate_postgres_container(
            postgres_record, network_name, handoff["volumeName"]
        )
        command([DOCKER, "start", postgres_id], timeout=120)
        wait_postgres(postgres_id, args.ready_timeout_seconds, command)
        journal["resources"]["api"] = {
            "imageId": server_image["Id"],
            "labels": fixture_labels(marker, "node-baseline-api"),
            "name": api_name,
            "role": "node-baseline-api",
            "state": "planned",
        }
        replace_private_json(journal_path, journal)
        api_result = command(
            build_api_command(
                marker,
                handoff,
                environment,
                environment["WEATHER_SERVER_IMAGE"],
                network_name,
                api_name,
                args.listen_port,
                api_secret,
                catalog,
                capture,
            ),
            timeout=120,
        )
        api_id = api_result.stdout.decode("ascii", errors="strict").strip()
        journal["resources"]["api"] = {
            "id": api_id,
            "imageId": server_image["Id"],
            "labels": fixture_labels(marker, "node-baseline-api"),
            "name": api_name,
            "role": "node-baseline-api",
            "state": "created",
        }
        replace_private_json(journal_path, journal)
        api_record = inspect_container(
            api_id,
            api_name,
            marker,
            server_image["Id"],
            "node-baseline-api",
            command,
        )
        validate_api_container(
            api_record,
            network_name,
            args.listen_port,
            api_secret,
            catalog,
            capture,
        )
        command([DOCKER, "start", api_id], timeout=120)
        wait_api(api_id, args.ready_timeout_seconds, command)
        internal_endpoint = inspect_api_internal_endpoint(
            api_id,
            api_name,
            postgres_id,
            postgres_name,
            marker,
            server_image["Id"],
            network_id,
            network_name,
            command,
        )
        internal_health = probe_internal_api_from_host(internal_endpoint["ipv4"])
        web_endpoint = None
        web_health = None
        web_id = None
        # add the optional credential-free web edge only after API readiness
        if web_image is not None:
            web_command = build_web_command(
                marker,
                environment,
                environment["WEATHER_WEB_IMAGE"],
                network_name,
                web_name,
            )
            validate_web_launch_identity(
                web_image,
                web_production_record,
                web_command,
                environment["WEATHER_WEB_IMAGE"],
            )
            journal["resources"]["web"] = {
                "imageId": web_image["Id"],
                "labels": fixture_labels(marker, "node-baseline-web"),
                "name": web_name,
                "role": "node-baseline-web",
                "state": "planned",
            }
            replace_private_json(journal_path, journal)
            web_result = command(
                web_command,
                timeout=120,
            )
            web_id = web_result.stdout.decode("ascii", errors="strict").strip()
            journal["resources"]["web"] = {
                "id": web_id,
                "imageId": web_image["Id"],
                "labels": fixture_labels(marker, "node-baseline-web"),
                "name": web_name,
                "role": "node-baseline-web",
                "state": "created",
            }
            replace_private_json(journal_path, journal)
            web_record = inspect_container(
                web_id,
                web_name,
                marker,
                web_image["Id"],
                "node-baseline-web",
                command,
            )
            validate_web_container(web_record, network_name, environment)
            command([DOCKER, "start", web_id], timeout=120)
            web_endpoint = inspect_web_internal_endpoint(
                web_id,
                web_name,
                web_image["Id"],
                api_id,
                api_name,
                server_image["Id"],
                postgres_id,
                postgres_name,
                marker,
                network_id,
                network_name,
                command,
            )
            web_health = wait_internal_web_from_host(
                web_endpoint["ipv4"],
                args.ready_timeout_seconds,
            )
        # recheck the selected installed release and state after startup
        if (
            os.readlink(installed_state["activeEnvironment"]["path"])
            != installed_state["activeEnvironment"]["target"]
            or installed_state["activeEnvironment"]["path"].resolve(strict=True)
            != deployment_root
            / "deploy"
            / "releases"
            / f"{environment['WEATHER_RELEASE']}.env"
            or bytes_sha256(
                read_regular_bytes(
                    installed_state["currentRelease"]["path"],
                    "current release state",
                    256,
                    owner=(0, 0),
                )
            )
            != installed_state["currentRelease"]["sha256"]
            or bytes_sha256(
                read_regular_bytes(
                    deployment_root
                    / "deploy"
                    / "releases"
                    / f"{environment['WEATHER_RELEASE']}.env",
                    "deployment environment",
                    65536,
                    owner=(0, 0),
                )
            )
            != environment_sha256
            or bytes_sha256(
                read_regular_bytes(
                    installed_state["candidateCatalog"]["path"],
                    "installed candidate catalog",
                    1024 * 1024,
                    owner=(0, 0),
                )
            )
            != installed_state["candidateCatalog"]["sha256"]
            or bytes_sha256(
                read_regular_bytes(
                    installed_state["captureEpoch"]["path"],
                    "installed capture epoch",
                    1024 * 1024,
                    owner=(0, 0),
                )
            )
            != installed_state["captureEpoch"]["sha256"]
        ):
            raise FixtureError("installed release changed during fixture startup")
        handoff_output = {
            "contractVersion": SESSION_KIND,
            "fixtureId": marker["fixtureId"],
            "fixtureRoot": str(fixture_root),
            "profile": profile,
            "volumeName": handoff["volumeName"],
            "volumeDevice": str(target_device),
            "restoreHandoffSha256": restore_handoff_sha256,
            "deploymentRoot": str(deployment_root),
            "currentReleaseStateSha256": installed_state["currentRelease"]["sha256"],
            "deploymentEnvironmentSha256": environment_sha256,
            "activeEnvironmentTarget": installed_state["activeEnvironment"]["target"],
            "release": environment["WEATHER_RELEASE"],
            "postgresImage": environment["POSTGRES_IMAGE"],
            "postgresImageId": postgres_image["Id"],
            "serverImage": environment["WEATHER_SERVER_IMAGE"],
            "serverImageId": server_image["Id"],
            "candidateCatalogSha256": installed_state["candidateCatalog"]["sha256"],
            "captureEpochSha256": installed_state["captureEpoch"]["sha256"],
            "hostLoopbackOrigin": f"http://127.0.0.1:{args.listen_port}",
            "hostLoopbackPublicationVerified": False,
            "networkId": network_id,
            "networkName": network_name,
            "postgresContainerId": postgres_id,
            "postgresContainerName": postgres_name,
            "apiContainerId": api_id,
            "apiContainerName": api_name,
            "apiInternalHostHealth": internal_health,
            "apiInternalIPv4": internal_endpoint["ipv4"],
            "apiInternalNetworkEndpointId": internal_endpoint["endpointId"],
            "apiInternalOrigin": (
                f"http://{internal_endpoint['ipv4']}:{internal_endpoint['port']}"
            ),
            "apiInternalPort": internal_endpoint["port"],
            "apiInternalSubnet": internal_endpoint["subnet"],
            "maintenanceListenerConfigured": False,
            "migrationExecuted": False,
            "workerExecuted": False,
            "startedAt": utc_now().strftime("%Y-%m-%dT%H:%M:%SZ"),
        }
        # expose optional web evidence only when that profile was admitted
        if (
            web_image is not None
            and web_endpoint is not None
            and web_health is not None
        ):
            handoff_output.update(
                {
                    "webContainerId": web_id,
                    "webContainerName": web_name,
                    "webImage": environment["WEATHER_WEB_IMAGE"],
                    "webImageId": web_image["Id"],
                    "webInternalHostHealth": web_health,
                    "webInternalIPv4": web_endpoint["ipv4"],
                    "webInternalNetworkEndpointId": web_endpoint["endpointId"],
                    "webInternalOrigin": (
                        f"http://{web_endpoint['ipv4']}:{web_endpoint['port']}"
                    ),
                    "webInternalPort": web_endpoint["port"],
                    "webInternalSubnet": web_endpoint["subnet"],
                    "webMapProviderAvailable": False,
                    "webMapProviderStatus": "unavailable-no-credentials-or-egress",
                    "webOrdinaryPagePrefetched": False,
                    "webProductionMountsUsed": False,
                    "webReadinessPaths": ["/api/v1/health"],
                }
            )
        write_private_json(session_directory / HANDOFF_NAME, handoff_output)
        journal["state"] = "running"
        replace_private_json(journal_path, journal)
        print(json.dumps(handoff_output, sort_keys=True, indent=2), flush=True)
        return 0
    except BaseException as error:
        cleanup_errors = cleanup_runtime(journal, marker, command)
        # remove the session only after complete resource cleanup
        if not cleanup_errors:
            journal["resources"] = {}
            journal["state"] = "cleanup-complete"
            replace_private_json(journal_path, journal)
            remove_session_directory(session_directory)
        detail = f"{error}"
        # retain exact cleanup failures
        if cleanup_errors:
            detail += f"; cleanupErrors={cleanup_errors}"
        raise FixtureError(detail) from error


# stop one isolated fixture runtime
def stop(args, command=run_command):
    # require root for exact lifecycle custody
    if os.geteuid() != 0:
        raise FixtureError("root is required for fixture lifecycle operations")
    fixture_root = Path(args.fixture_root)
    marker, _ = validate_marker(fixture_root)
    runtime_root = RUNTIME_BASE / f"weather-native-benchmark-{marker['fixtureId']}"
    require_path(
        runtime_root, "fixture runtime root", "directory", owner=(0, 0), mode=0o700
    )
    session_directory = runtime_root / SESSION_DIRECTORY_NAME
    require_path(
        session_directory,
        "node API session directory",
        "directory",
        owner=(0, 0),
        mode=0o700,
    )
    journal, _ = read_json(
        session_directory / JOURNAL_NAME,
        "node API lifecycle journal",
        owner=(0, 0),
        mode=0o600,
    )
    # bind the journal to this exact fixture
    if (
        journal.get("contractVersion") != SESSION_KIND
        or journal.get("fixtureId") != marker["fixtureId"]
        or journal.get("fixtureRoot") != str(fixture_root)
        or not isinstance(journal.get("resources"), dict)
    ):
        raise FixtureError("node API lifecycle journal identity differs")
    errors = cleanup_runtime(journal, marker, command)
    # preserve the journal for a safe retry after any cleanup failure
    if errors:
        raise FixtureError(f"fixture runtime cleanup failed: {errors}")
    journal["resources"] = {}
    journal["state"] = "cleanup-complete"
    replace_private_json(session_directory / JOURNAL_NAME, journal)
    remove_session_directory(session_directory)
    print(
        json.dumps(
            {
                "contractVersion": SESSION_KIND,
                "fixtureId": marker["fixtureId"],
                "status": "stopped",
                "stoppedAt": utc_now().strftime("%Y-%m-%dT%H:%M:%SZ"),
            },
            sort_keys=True,
        ),
        flush=True,
    )
    return 0


# parse the closed lifecycle command line
def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="action", required=True)
    start_parser = subparsers.add_parser("start")
    start_parser.add_argument("--fixture-root", required=True)
    start_parser.add_argument("--deployment-root", required=True)
    start_parser.add_argument("--listen-port", required=True, type=int)
    start_parser.add_argument("--ready-timeout-seconds", type=int, default=180)
    start_parser.add_argument(
        "--profile",
        choices=(PROFILE_API_ONLY, PROFILE_API_WEB),
        default=PROFILE_API_ONLY,
    )
    stop_parser = subparsers.add_parser("stop")
    stop_parser.add_argument("--fixture-root", required=True)
    return parser.parse_args(argv)


# execute one closed lifecycle action
def main(argv=None):
    args = parse_args(argv)
    # restrict the loopback publication and readiness bound
    if args.action == "start" and (
        not 1024 <= args.listen_port <= 65535
        or not 30 <= args.ready_timeout_seconds <= 600
    ):
        raise FixtureError(
            "listen port or readiness timeout is outside supported bounds"
        )
    # dispatch one exact action
    if args.action == "start":
        return start(args)
    return stop(args)


# emit only bounded lifecycle failures
if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (FixtureError, OSError, subprocess.SubprocessError) as error:
        print(f"native Node API fixture failed: {error}", file=sys.stderr)
        raise SystemExit(1) from None
