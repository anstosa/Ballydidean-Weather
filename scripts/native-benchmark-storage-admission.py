#!/usr/bin/env python3
r"""Read-only admission for one disposable native ARM64 benchmark fixture.

Fixed fixture contract:
  ID              rust-rewrite-<16 lowercase hex>
  root            /var/lib/weather-native-benchmark/<fixture-id>
  backing         <root>/fixture.ext4
  mount           <root>/mnt
  volume root     <root>/mnt/docker-volumes
  volume prefix   weather-native-benchmark-<fixture-id>-
  marker          <root>/.fixture-marker.json
  approved cap    <root>/.approved-cap.json
  source archive  <root>/source-fixture.dump.age (<= 1.25 GiB)
  archive digest  <root>/source-fixture.dump.age.sha256
  actual receipt  <root>/.storage-admission.json (operator captures stdout)
  restore handoff <root>/.restore-handoff.json
  restore secrets <root>/.restore-secrets-<16 lowercase hex>

Preallocation estimate (never authorizes allocation or fixture use):
  sudo python3 scripts/native-benchmark-storage-admission.py estimate \
    --fixture-id rust-rewrite-0123456789abcdef \
    --copy-bytes 14685855744 --backing-bytes 19864223744 \
    --other-peak-bytes 1610612736

After an operator separately allocates and mounts the approved fixture, replace
``estimate`` with ``admit``. Only ``admit`` can emit ``admitted: true`` and
``fixtureUseAuthorized: true``. Both phases charge every allocated byte in the
exact outer fixture tree to ``otherPeakBytes``, except a verified fully
allocated backing file. The remaining other budget conservatively covers
unmeasured Docker metadata and any later bounded fixture-local growth. Root
headroom never includes prospective cleanup credit. The helper never mutates
storage and ``du -x`` never descends into the mounted loop filesystem.
"""

import argparse
import hashlib
import json
import os
import platform
import re
import stat
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

GIB = 1024 ** 3
MAX_ADDITIONAL_BYTES = 20 * GIB
ROOT_RESERVE_BYTES = 8 * GIB
INNER_RESERVE_BYTES = 4 * GIB
RETENTION_SECONDS = 24 * 60 * 60
SOURCE_ARCHIVE_MAX_BYTES = 5 * GIB // 4
NAMESPACE_ROOT = Path('/var/lib/weather-native-benchmark')
FIXTURE_ID_PATTERN = re.compile(r'rust-rewrite-[0-9a-f]{16}')
UUID_PATTERN = re.compile(r'[A-Za-z0-9.-]{1,128}')
RFC3339_PATTERN = re.compile(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z')
MARKER_KEYS = {
    'schemaVersion', 'kind', 'fixtureId', 'fixtureRoot', 'backingFile',
    'mountpoint', 'volumeRoot', 'volumeNamespacePrefix',
}
APPROVAL_KEYS = {
    'schemaVersion', 'kind', 'approvalAuthority', 'fixtureId', 'fixtureRoot',
    'copyBytes', 'backingBytes', 'otherPeakBytes', 'maxAdditionalBytes',
    'rootReserveBytes', 'innerReserveBytes', 'retentionSeconds',
    'rootFilesystemSource', 'rootFilesystemUuid', 'approvedAt', 'expiresAt',
}
OUTER_FIXED_NAMES = {
    '.fixture-marker.json', '.approved-cap.json', '.storage-admission.json',
    '.restore-handoff.json', '.restore-failure.json',
    'source-fixture.dump.age', 'source-fixture.dump.age.sha256',
    'fixture.ext4', 'mnt',
}
RESTORE_SECRET_NAMES = {
    'weather_admin_password', 'weather_owner_password', 'weather_api_password',
    'weather_ingest_password', 'weather_training_export_password',
}
RESTORE_SECRET_DIRECTORY_PATTERN = re.compile(r'\.restore-secrets-[0-9a-f]{16}')


# run one read-only probe
def run(argv):
    return subprocess.check_output(argv, timeout=60).decode('utf-8').strip()


# build the fixed layout
def fixture_contract(fixture_id):
    # require the narrow id
    if not isinstance(fixture_id, str) or FIXTURE_ID_PATTERN.fullmatch(fixture_id) is None:
        raise ValueError('Fixture ID must be rust-rewrite- followed by 16 lowercase hex characters')
    root = NAMESPACE_ROOT / fixture_id
    mountpoint = root / 'mnt'
    return {
        'schemaVersion': 1,
        'kind': 'weather-native-rust-rewrite-fixture',
        'fixtureId': fixture_id,
        'fixtureRoot': str(root),
        'backingFile': str(root / 'fixture.ext4'),
        'mountpoint': str(mountpoint),
        'volumeRoot': str(mountpoint / 'docker-volumes'),
        'volumeNamespacePrefix': f'weather-native-benchmark-{fixture_id}-',
    }


# require one integer
def exact_integer(value, field, allow_zero=False):
    minimum = 0 if allow_zero else 1
    # reject coerced values
    if not isinstance(value, int) or isinstance(value, bool) or value < minimum:
        raise ValueError(f'{field} must be an integer of at least {minimum}')
    return value


# parse canonical utc
def parse_rfc3339(value, field):
    # require the stable form
    if not isinstance(value, str) or RFC3339_PATTERN.fullmatch(value) is None:
        raise ValueError(f'{field} must use YYYY-MM-DDTHH:MM:SSZ')
    return datetime.strptime(value, '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc)


# validate the exact marker
def validate_marker(marker, contract):
    # reject schema expansion
    if not isinstance(marker, dict) or set(marker) != MARKER_KEYS:
        raise RuntimeError('Fixture marker has an unexpected schema')
    # bind all marker values
    if marker != contract:
        raise RuntimeError('Fixture marker does not match the exact fixture contract')


# validate the approved cap
def validate_approval(approval, contract, plan, now):
    # reject schema expansion
    if not isinstance(approval, dict) or set(approval) != APPROVAL_KEYS:
        raise RuntimeError('Approved-cap receipt has an unexpected schema')
    expected = {
        'schemaVersion': 1,
        'kind': 'weather-native-rust-rewrite-storage-approval',
        'approvalAuthority': 'ansel',
        'fixtureId': contract['fixtureId'],
        'fixtureRoot': contract['fixtureRoot'],
        'copyBytes': plan['copyBytes'],
        'backingBytes': plan['backingBytes'],
        'otherPeakBytes': plan['otherPeakBytes'],
        'maxAdditionalBytes': MAX_ADDITIONAL_BYTES,
        'rootReserveBytes': ROOT_RESERVE_BYTES,
        'innerReserveBytes': INNER_RESERVE_BYTES,
        'retentionSeconds': RETENTION_SECONDS,
    }
    # bind all approved values
    for key, value in expected.items():
        # reject mismatches
        if approval.get(key) != value:
            raise RuntimeError(f'Approved-cap receipt does not bind {key}')
    source = approval.get('rootFilesystemSource')
    root_uuid = approval.get('rootFilesystemUuid')
    # require a physical source
    if (not isinstance(source, str) or not source.startswith('/dev/') or
            re.fullmatch(r'/dev/loop[0-9]+', source) is not None):
        raise RuntimeError('Approved-cap receipt has an invalid root filesystem source')
    # require stable identity
    if not isinstance(root_uuid, str) or UUID_PATTERN.fullmatch(root_uuid) is None:
        raise RuntimeError('Approved-cap receipt has an invalid root filesystem UUID')
    approved_at = parse_rfc3339(approval.get('approvedAt'), 'approvedAt')
    expires_at = parse_rfc3339(approval.get('expiresAt'), 'expiresAt')
    # enforce exact retention
    if expires_at - approved_at != timedelta(seconds=RETENTION_SECONDS):
        raise RuntimeError('Approved-cap receipt must expire exactly 24 hours after approval')
    # require current approval
    if now < approved_at or now >= expires_at:
        raise RuntimeError('Approved-cap receipt is not currently valid')


# read one command record
def single_record(output, key, subject):
    payload = json.loads(output)
    records = payload.get(key)
    # require one record
    if not isinstance(records, list) or len(records) != 1 or not isinstance(records[0], dict):
        raise RuntimeError(f'Cannot identify one {subject}')
    return records[0]


# validate one ext4 mount
def mount_record(output, target, require_loop):
    record = single_record(output, 'filesystems', 'filesystem mount')
    option_text = record.get('options')
    options = set(option_text.split(',')) if isinstance(option_text, str) else set()
    source = record.get('source')
    # require the exact target
    if record.get('target') != str(target):
        raise RuntimeError('Filesystem target is not the exact mountpoint')
    # require writable ext4
    if record.get('fstype') != 'ext4' or 'rw' not in options or 'ro' in options:
        raise RuntimeError('Filesystem must be writable ext4')
    # require stable identity
    if not isinstance(record.get('uuid'), str) or UUID_PATTERN.fullmatch(record['uuid']) is None:
        raise RuntimeError('Filesystem UUID is unavailable')
    # select the device class
    if require_loop:
        # require a loop device
        if not isinstance(source, str) or re.fullmatch(r'/dev/loop[0-9]+', source) is None:
            raise RuntimeError('Fixture mount must use a loop device')
    else:
        # reject a loop host root
        if (not isinstance(source, str) or not source.startswith('/dev/') or
                re.fullmatch(r'/dev/loop[0-9]+', source) is not None):
            raise RuntimeError('Host root must use a non-loop /dev block device')
    return {
        'target': record['target'],
        'source': source,
        'filesystemType': 'ext4',
        'options': sorted(options),
        'uuid': record['uuid'],
    }


# parse one loop integer
def loop_integer(value, field):
    # accept json integers
    if isinstance(value, int) and not isinstance(value, bool) and value >= 0:
        return value
    # accept digit strings
    if isinstance(value, str) and re.fullmatch(r'[0-9]+', value) is not None:
        return int(value)
    raise RuntimeError(f'Loop-device {field} is invalid')


# require one false flag
def loop_false(value, field):
    # accept documented encodings
    if value is False or value == 0 or value == '0':
        return False
    raise RuntimeError(f'Loop-device {field} must be false')


# validate the exact loop binding
def loop_record(output, device, backing_file, backing_bytes):
    record = single_record(output, 'loopdevices', 'loop-device binding')
    # bind the device and file
    if record.get('name') != device or record.get('back-file') != str(backing_file):
        raise RuntimeError('Loop-device identity does not match the fixture backing file')
    offset = loop_integer(record.get('offset'), 'offset')
    size_limit = loop_integer(record.get('sizelimit'), 'size limit')
    loop_false(record.get('ro'), 'read-only flag')
    # expose the whole file
    if offset != 0 or size_limit not in (0, backing_bytes):
        raise RuntimeError('Loop device must expose the complete backing file')
    return {
        'device': device,
        'backingFile': str(backing_file),
        'offsetBytes': offset,
        'sizeLimitBytes': size_limit,
        'readOnly': False,
    }


# build the supported loop probe
def loop_probe_command(device):
    # require the already selected loop shape
    if not isinstance(device, str) or re.fullmatch(r'/dev/loop[0-9]+', device) is None:
        raise ValueError('Loop probe requires one exact loop device')
    return [
        '/usr/sbin/losetup', '--json', '--list',
        '--output', 'NAME,BACK-FILE,OFFSET,SIZELIMIT,RO', device,
    ]


# prove one private directory
def private_directory(path, owner_uid=0):
    metadata = os.lstat(path)
    # reject links and broad modes
    if (not stat.S_ISDIR(metadata.st_mode) or stat.S_IMODE(metadata.st_mode) != 0o700 or
            metadata.st_uid != owner_uid or path.resolve(strict=True) != path):
        raise RuntimeError(f'{path} must be a canonical owner-private directory')
    return metadata


# read one private json file
def read_private_json(path, owner_uid=0):
    metadata = os.lstat(path)
    # reject links and broad files
    if (not stat.S_ISREG(metadata.st_mode) or stat.S_IMODE(metadata.st_mode) != 0o600 or
            metadata.st_uid != owner_uid or metadata.st_nlink != 1 or
            metadata.st_size > 16384 or path.resolve(strict=True) != path):
        raise RuntimeError(f'{path} must be one canonical owner-private regular file')
    descriptor = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
    try:
        opened = os.fstat(descriptor)
        # bind the opened inode
        if ((opened.st_dev, opened.st_ino) != (metadata.st_dev, metadata.st_ino) or
                opened.st_size != metadata.st_size):
            raise RuntimeError(f'{path} changed during validation')
        raw = os.read(descriptor, 16385)
        final = os.fstat(descriptor)
        # reject truncation or growth
        if (len(raw) != metadata.st_size or len(raw) > 16384 or
                (final.st_dev, final.st_ino, final.st_size, final.st_mtime_ns, final.st_ctime_ns) !=
                (metadata.st_dev, metadata.st_ino, metadata.st_size,
                 metadata.st_mtime_ns, metadata.st_ctime_ns)):
            raise RuntimeError(f'{path} changed during validation')
    finally:
        os.close(descriptor)
    payload = json.loads(raw.decode('utf-8'))
    # require one object
    if not isinstance(payload, dict):
        raise TypeError(f'{path} must contain one JSON object')
    return payload, hashlib.sha256(raw).hexdigest()


# prove one block device
def block_device_identity(device):
    path = Path(device)
    metadata = os.stat(path)
    # reject aliases and nonblocks
    if path.resolve(strict=True) != path or not stat.S_ISBLK(metadata.st_mode):
        raise RuntimeError(f'{device} must be one canonical block device')
    return metadata.st_rdev


# measure one filesystem
def filesystem_space(path):
    measurement = os.statvfs(path)
    return {
        'availableBytes': measurement.f_bavail * measurement.f_frsize,
        'totalBytes': measurement.f_blocks * measurement.f_frsize,
    }


# parse one du measurement
def allocated_tree_bytes(output, root):
    fields = output.split('\t')
    # require one exact result
    if len(fields) != 2 or fields[1] != str(root) or re.fullmatch(r'[0-9]+', fields[0]) is None:
        raise RuntimeError('Fixture-root allocated-byte measurement is invalid')
    return int(fields[0])


# validate the outer tree
def validate_outer_tree(root, mountpoint, owner_uid=0):
    entries = []
    root_device = os.lstat(root).st_dev

    # inspect one literal entry
    def inspect(path, credential_root=None):
        metadata = os.lstat(path)
        relative = path.relative_to(root).as_posix()
        # allow only the exact mounted boundary
        if path == mountpoint and stat.S_ISDIR(metadata.st_mode) and metadata.st_dev != root_device:
            entries.append({'path': relative, 'type': 'mounted-boundary'})
            return
        # reject foreign filesystems
        if metadata.st_dev != root_device:
            raise RuntimeError(f'Unexpected foreign filesystem in fixture root: {relative}')
        # inspect private directories
        if stat.S_ISDIR(metadata.st_mode):
            # require private ownership
            if stat.S_IMODE(metadata.st_mode) != 0o700 or metadata.st_uid != owner_uid:
                raise RuntimeError(f'Unexpected non-private fixture directory: {relative}')
            entries.append({'path': relative, 'type': 'directory'})
            # keep the unmounted target empty
            if path == mountpoint and any(path.iterdir()):
                raise RuntimeError('Unmounted fixture mountpoint must be empty')
            # require exact credential members
            if credential_root is not None and path != credential_root:
                raise RuntimeError(f'Unexpected nested fixture directory: {relative}')
            # inspect every child
            for child in sorted(path.iterdir(), key=lambda value: value.name):
                inspect(child, credential_root)
            return
        # inspect private regular files
        if stat.S_ISREG(metadata.st_mode):
            # admit exact database credential files
            if credential_root is not None:
                # require one known database-owned secret
                if (path.parent != credential_root or path.name not in RESTORE_SECRET_NAMES or
                        stat.S_IMODE(metadata.st_mode) != 0o400 or metadata.st_uid != 999 or
                        metadata.st_gid != 999 or metadata.st_nlink != 1):
                    raise RuntimeError(f'Unexpected fixture credential file: {relative}')
                entries.append({'path': relative, 'type': 'credential'})
                return
            # require one private inode
            if (stat.S_IMODE(metadata.st_mode) != 0o600 or metadata.st_uid != owner_uid or
                    metadata.st_nlink != 1):
                raise RuntimeError(f'Unexpected non-private fixture file: {relative}')
            entries.append({'path': relative, 'type': 'file'})
            return
        raise RuntimeError(f'Unexpected fixture-root entry type: {relative}')

    # inspect every direct child
    for child in sorted(root.iterdir(), key=lambda value: value.name):
        credential_directory = RESTORE_SECRET_DIRECTORY_PATTERN.fullmatch(child.name) is not None
        # require one allowlisted name
        if child.name not in OUTER_FIXED_NAMES and not credential_directory:
            raise RuntimeError(f'Unexpected fixture-root entry: {child.name}')
        inspect(child, child if credential_directory else None)
        # require the complete credential set
        if credential_directory and {value.name for value in child.iterdir()} != RESTORE_SECRET_NAMES:
            raise RuntimeError('Fixture credential directory members differ')
    encoded = json.dumps(entries, sort_keys=True, separators=(',', ':')).encode('utf-8')
    return {
        'entryCount': len(entries),
        'entriesSha256': hashlib.sha256(encoded).hexdigest(),
    }


# observe a verified backing file
def backing_observation(backing_file, expected_bytes, root_device):
    # preserve genuine absence
    if not backing_file.exists() and not backing_file.is_symlink():
        return {
            'present': False,
            'logicalBytes': 0,
            'allocatedBytes': 0,
        }
    metadata = os.lstat(backing_file)
    # require the exact complete backing
    if (not stat.S_ISREG(metadata.st_mode) or stat.S_IMODE(metadata.st_mode) != 0o600 or
            metadata.st_uid != 0 or metadata.st_nlink != 1 or
            backing_file.resolve(strict=True) != backing_file or
            metadata.st_dev != root_device or metadata.st_size != expected_bytes or
            metadata.st_blocks * 512 < expected_bytes):
        raise RuntimeError('Present fixture backing file is not the exact fully allocated plan')
    return {
        'present': True,
        'logicalBytes': metadata.st_size,
        'allocatedBytes': metadata.st_blocks * 512,
    }


# hash one bounded archive
def file_sha256(path):
    digest = hashlib.sha256()
    descriptor = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
    try:
        opened = os.fstat(descriptor)
        total = 0
        # consume bounded chunks
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            # stop at eof
            if not chunk:
                break
            total += len(chunk)
            # retain the archive cap
            if total > SOURCE_ARCHIVE_MAX_BYTES:
                raise RuntimeError('Source archive grew beyond the 1.25 GiB cap')
            digest.update(chunk)
        final = os.fstat(descriptor)
        # reject concurrent mutation
        if ((opened.st_dev, opened.st_ino, opened.st_size, opened.st_mtime_ns,
             opened.st_ctime_ns) !=
                (final.st_dev, final.st_ino, final.st_size, final.st_mtime_ns,
                 final.st_ctime_ns) or total != opened.st_size):
            raise RuntimeError('Source archive changed during hashing')
    finally:
        os.close(descriptor)
    return digest.hexdigest()


# verify the fixed archive pair
def source_archive_observation(root, owner_uid=0):
    archive = root / 'source-fixture.dump.age'
    checksum = root / 'source-fixture.dump.age.sha256'
    archive_exists = archive.exists() or archive.is_symlink()
    checksum_exists = checksum.exists() or checksum.is_symlink()
    # preserve genuine pair absence
    if not archive_exists and not checksum_exists:
        return {'present': False, 'sha256': None, 'logicalBytes': 0, 'allocatedBytes': 0}
    # reject incomplete publication
    if not archive_exists or not checksum_exists:
        raise RuntimeError('Source archive and checksum must appear as one complete pair')
    archive_metadata = os.lstat(archive)
    checksum_metadata = os.lstat(checksum)
    # require bounded private files
    for path, metadata in ((archive, archive_metadata), (checksum, checksum_metadata)):
        # reject links and aliases
        if (not stat.S_ISREG(metadata.st_mode) or stat.S_IMODE(metadata.st_mode) != 0o600 or
                metadata.st_uid != owner_uid or metadata.st_nlink != 1 or
                path.resolve(strict=True) != path):
            raise RuntimeError('Source archive pair must be canonical owner-private files')
    allocated = archive_metadata.st_blocks * 512
    # enforce the archive cap
    if (archive_metadata.st_size <= 0 or archive_metadata.st_size > SOURCE_ARCHIVE_MAX_BYTES or
            allocated > SOURCE_ARCHIVE_MAX_BYTES):
        raise RuntimeError('Source archive exceeds the 1.25 GiB cap')
    expected_text = checksum.read_text(encoding='ascii')
    match = re.fullmatch(r'([0-9a-f]{64})  source-fixture\.dump\.age\n', expected_text)
    # require the published checksum form
    if match is None:
        raise RuntimeError('Source archive checksum file has an invalid format')
    actual_sha256 = file_sha256(archive)
    # verify the published pair
    if actual_sha256 != match.group(1):
        raise RuntimeError('Source archive checksum does not match its bytes')
    return {
        'present': True,
        'sha256': actual_sha256,
        'logicalBytes': archive_metadata.st_size,
        'allocatedBytes': allocated,
    }


# collect common evidence
def collect_common(contract, plan):
    root = Path(contract['fixtureRoot'])
    namespace_metadata = private_directory(NAMESPACE_ROOT)
    root_metadata = private_directory(root)
    marker_path = root / '.fixture-marker.json'
    approval_path = root / '.approved-cap.json'
    marker, marker_sha256 = read_private_json(marker_path)
    approval, approval_sha256 = read_private_json(approval_path)
    root_output = run([
        '/usr/bin/findmnt', '--json', '--target', str(root),
        '--output', 'TARGET,SOURCE,FSTYPE,OPTIONS,UUID',
    ])
    root_mount = mount_record(root_output, Path('/'), False)
    block_device_identity(root_mount['source'])
    slash_metadata = os.stat('/')
    # require root-backed isolation
    if (namespace_metadata.st_dev != slash_metadata.st_dev or
            root_metadata.st_dev != slash_metadata.st_dev):
        raise RuntimeError('Fixture namespace must be on the host root filesystem')
    tree = validate_outer_tree(root, Path(contract['mountpoint']))
    backing = backing_observation(
        Path(contract['backingFile']), plan['backingBytes'], slash_metadata.st_dev,
    )
    archive = source_archive_observation(root)
    allocated_output = run([
        '/usr/bin/du', '--summarize', '--block-size=1', '-x', str(root),
    ])
    return {
        'system': platform.system(),
        'machine': platform.machine(),
        'kernelRelease': platform.release(),
        'effectiveUid': os.geteuid(),
        'fixtureRootOwnerPrivate': True,
        'fixtureRootOnHostRoot': True,
        'rootMount': root_mount,
        'rootStatDevice': slash_metadata.st_dev,
        'rootSpace': filesystem_space(Path('/')),
        'fixtureOuterAllocatedBytes': allocated_tree_bytes(allocated_output, root),
        'fixtureOuterEntryCount': tree['entryCount'],
        'fixtureOuterEntriesSha256': tree['entriesSha256'],
        'backingPresent': backing['present'],
        'backingLogicalBytes': backing['logicalBytes'],
        'backingAllocatedBytes': backing['allocatedBytes'],
        'sourceArchive': archive,
        'marker': marker,
        'markerPath': str(marker_path),
        'markerSha256': marker_sha256,
        'approval': approval,
        'approvalPath': str(approval_path),
        'approvalSha256': approval_sha256,
    }


# collect actual evidence
def collect_actual(contract, plan, common):
    backing_file = Path(contract['backingFile'])
    mountpoint = Path(contract['mountpoint'])
    backing_metadata = os.lstat(backing_file)
    # require one private backing
    if (not stat.S_ISREG(backing_metadata.st_mode) or
            stat.S_IMODE(backing_metadata.st_mode) != 0o600 or
            backing_metadata.st_uid != 0 or backing_metadata.st_nlink != 1 or
            backing_file.resolve(strict=True) != backing_file):
        raise RuntimeError('Fixture backing file must be one canonical owner-private regular file')
    # reject backing changes
    if (common['backingPresent'] is not True or
            common['backingLogicalBytes'] != backing_metadata.st_size or
            common['backingAllocatedBytes'] != backing_metadata.st_blocks * 512):
        raise RuntimeError('Fixture backing file changed during admission')
    # require the verified source pair first
    if common['sourceArchive']['present'] is not True:
        raise RuntimeError('Actual admission requires the verified source archive pair')
    private_directory(mountpoint)
    # bind backing storage
    if backing_metadata.st_dev != common['rootStatDevice']:
        raise RuntimeError('Fixture backing file must be on the host root filesystem')
    mount_output = run([
        '/usr/bin/findmnt', '--json', '--target', str(mountpoint),
        '--output', 'TARGET,SOURCE,FSTYPE,OPTIONS,UUID',
    ])
    mounted = mount_record(mount_output, mountpoint, True)
    loop_identity = block_device_identity(mounted['source'])
    mount_metadata = os.stat(mountpoint)
    # bind mount to loop
    if mount_metadata.st_dev != loop_identity:
        raise RuntimeError('Fixture mount device does not match the loop device')
    loop_output = run(loop_probe_command(mounted['source']))
    loop = loop_record(loop_output, mounted['source'], backing_file, plan['backingBytes'])
    return {
        **common,
        'backingFileOwnerPrivate': True,
        'backingFileOnHostRoot': True,
        'backingLogicalBytes': backing_metadata.st_size,
        'backingAllocatedBytes': backing_metadata.st_blocks * 512,
        'mountpointOwnerPrivate': True,
        'fixtureMount': mounted,
        'fixtureLoop': loop,
        'mountDeviceMatchesLoop': True,
        'innerSpace': filesystem_space(mountpoint),
    }


# validate the native host
def validate_host(observation, approval):
    # require native arm64 linux
    if (observation.get('system') != 'Linux' or observation.get('machine') != 'aarch64' or
            observation.get('effectiveUid') != 0):
        raise RuntimeError('Admission requires root on native Linux/aarch64')
    release = observation.get('kernelRelease')
    # reject wsl and unknown text
    if not isinstance(release, str) or 'microsoft' in release.lower() or 'wsl' in release.lower():
        raise RuntimeError('Admission refuses WSL or an unknown native kernel identity')
    # require fixed isolation
    if (observation.get('fixtureRootOwnerPrivate') is not True or
            observation.get('fixtureRootOnHostRoot') is not True):
        raise RuntimeError('Fixture root isolation is not proven')
    root_mount = observation.get('rootMount')
    # bind approved root ext4
    if (not isinstance(root_mount, dict) or root_mount.get('target') != '/' or
            root_mount.get('filesystemType') != 'ext4' or
            root_mount.get('source') != approval['rootFilesystemSource'] or
            root_mount.get('uuid') != approval['rootFilesystemUuid'] or
            'rw' not in root_mount.get('options', []) or
            re.fullmatch(r'/dev/loop[0-9]+', root_mount.get('source', '')) is not None):
        raise RuntimeError('Host root ext4 identity does not match the approved-cap receipt')


# validate the byte plan
def validate_plan(plan):
    copy_bytes = exact_integer(plan.get('copyBytes'), 'copyBytes')
    backing_bytes = exact_integer(plan.get('backingBytes'), 'backingBytes')
    other_peak_bytes = exact_integer(plan.get('otherPeakBytes'), 'otherPeakBytes', True)
    inner_required = copy_bytes + INNER_RESERVE_BYTES
    peak_required = backing_bytes + other_peak_bytes
    # enforce the total cap
    if peak_required > MAX_ADDITIONAL_BYTES:
        raise RuntimeError('Requested backing plus other peak bytes exceeds the 20 GiB cap')
    # require plausible inner space
    if backing_bytes < inner_required:
        raise RuntimeError('Requested backing cannot hold the copy plus 4 GiB inner reserve')
    return {
        **plan,
        'innerRequiredBytes': inner_required,
        'peakRequiredBytes': peak_required,
    }


# calculate conservative root accounting
def storage_accounting(plan, observation):
    outer_allocated = exact_integer(
        observation.get('fixtureOuterAllocatedBytes'), 'fixtureOuterAllocatedBytes', True,
    )
    entry_count = exact_integer(
        observation.get('fixtureOuterEntryCount'), 'fixtureOuterEntryCount', True,
    )
    entries_sha256 = observation.get('fixtureOuterEntriesSha256')
    # require a stable tree inventory
    if not isinstance(entries_sha256, str) or re.fullmatch(r'[0-9a-f]{64}', entries_sha256) is None:
        raise RuntimeError('Fixture outer-tree inventory digest is invalid')
    backing_present = observation.get('backingPresent')
    # require an explicit backing state
    if not isinstance(backing_present, bool):
        raise TypeError('Fixture backing presence is unknown')
    backing_logical = exact_integer(
        observation.get('backingLogicalBytes'), 'backingLogicalBytes', not backing_present,
    )
    backing_allocated = exact_integer(
        observation.get('backingAllocatedBytes'), 'backingAllocatedBytes', not backing_present,
    )
    # bind absence exactly
    if not backing_present and (backing_logical != 0 or backing_allocated != 0):
        raise RuntimeError('Absent fixture backing has allocated bytes')
    # bind a fully allocated backing
    if backing_present and (backing_logical != plan['backingBytes'] or
                            backing_allocated < backing_logical):
        raise RuntimeError('Present fixture backing is not the exact fully allocated plan')
    # require a contained measurement
    if outer_allocated < backing_allocated:
        raise RuntimeError('Fixture-root allocation is smaller than its verified backing file')
    other_allocated = outer_allocated - backing_allocated
    # reject other-budget overflow
    if other_allocated > plan['otherPeakBytes']:
        raise RuntimeError('Fixture-root other allocations exceed otherPeakBytes')
    archive = observation.get('sourceArchive')
    # require exact archive evidence
    if not isinstance(archive, dict) or not isinstance(archive.get('present'), bool):
        raise TypeError('Source archive observation is incomplete')
    # reject backing before archive
    if backing_present and archive['present'] is not True:
        raise RuntimeError('Backing allocation cannot precede the verified source archive pair')
    # validate a present archive
    if archive['present']:
        # require its bounded identity
        if (not isinstance(archive.get('sha256'), str) or
                re.fullmatch(r'[0-9a-f]{64}', archive['sha256']) is None or
                exact_integer(archive.get('logicalBytes'), 'sourceArchiveLogicalBytes') >
                SOURCE_ARCHIVE_MAX_BYTES or
                exact_integer(archive.get('allocatedBytes'), 'sourceArchiveAllocatedBytes') >
                SOURCE_ARCHIVE_MAX_BYTES or
                archive['allocatedBytes'] > other_allocated):
            raise RuntimeError('Source archive observation exceeds the approved fixture accounting')
    else:
        # bind genuine absence
        if (archive.get('sha256') is not None or archive.get('logicalBytes') != 0 or
                archive.get('allocatedBytes') != 0):
            raise RuntimeError('Absent source archive has retained identity or bytes')
    other_remaining = plan['otherPeakBytes'] - other_allocated
    backing_remaining = 0 if backing_present else plan['backingBytes']
    conservative_peak = (
        backing_allocated + backing_remaining + other_allocated + other_remaining
    )
    # enforce actual allocation cap
    if conservative_peak > MAX_ADDITIONAL_BYTES:
        raise RuntimeError('Actual plus remaining fixture allocation exceeds the 20 GiB cap')
    return {
        'fixtureOuterAllocatedBytes': outer_allocated,
        'fixtureOuterEntryCount': entry_count,
        'fixtureOuterEntriesSha256': entries_sha256,
        'backingPresent': backing_present,
        'backingLogicalBytes': backing_logical,
        'backingAllocatedBytes': backing_allocated,
        'backingAllocationRemainingBytes': backing_remaining,
        'otherAllocatedBytes': other_allocated,
        'otherRemainingBytes': other_remaining,
        'unmeasuredOtherReserveBytes': other_remaining,
        'externalCleanupCreditBytes': 0,
        'rootAdmissionRequiredBytes': backing_remaining + other_remaining + ROOT_RESERVE_BYTES,
        'conservativeAdditionalPeakBytes': conservative_peak,
        'sourceArchivePresent': archive['present'],
        'sourceArchiveSha256': archive['sha256'],
        'sourceArchiveLogicalBytes': archive['logicalBytes'],
        'sourceArchiveAllocatedBytes': archive['allocatedBytes'],
    }


# build shared receipt fields
def base_receipt(contract, plan, observation, accounting, phase, captured_at):
    return {
        'receiptVersion': 'weather-native-benchmark-storage/v1',
        'capturedAt': captured_at.strftime('%Y-%m-%dT%H:%M:%SZ'),
        'phase': phase,
        'fixtureId': contract['fixtureId'],
        'fixtureRoot': contract['fixtureRoot'],
        'backingFile': contract['backingFile'],
        'mountpoint': contract['mountpoint'],
        'volumeRoot': contract['volumeRoot'],
        'volumeNamespacePrefix': contract['volumeNamespacePrefix'],
        'admissionReceiptPath': str(Path(contract['fixtureRoot']) / '.storage-admission.json'),
        'copyBytes': plan['copyBytes'],
        'backingBytes': plan['backingBytes'],
        'otherPeakBytes': plan['otherPeakBytes'],
        'maxAdditionalBytes': MAX_ADDITIONAL_BYTES,
        'rootReserveBytes': ROOT_RESERVE_BYTES,
        'innerReserveBytes': INNER_RESERVE_BYTES,
        'innerRequiredBytes': plan['innerRequiredBytes'],
        'peakRequiredBytes': plan['peakRequiredBytes'],
        'rootAvailableBytes': observation['rootSpace']['availableBytes'],
        'rootTotalBytes': observation['rootSpace']['totalBytes'],
        'rootFilesystem': observation['rootMount'],
        'markerPath': observation['markerPath'],
        'markerSha256': observation['markerSha256'],
        'approvedCapPath': observation['approvalPath'],
        'approvedCapSha256': observation['approvalSha256'],
        'approvedCapExpiresAt': observation['approval']['expiresAt'],
        **accounting,
    }


# evaluate collected evidence
def evaluate(phase, contract, plan, observation, now=None):
    captured_at = now or datetime.now(timezone.utc)
    normalized = validate_plan(plan)
    validate_marker(observation.get('marker'), contract)
    validate_approval(observation.get('approval'), contract, normalized, captured_at)
    validate_host(observation, observation['approval'])
    accounting = storage_accounting(normalized, observation)
    receipt = base_receipt(contract, normalized, observation, accounting, phase, captured_at)
    # keep estimates nonauthorizing
    if phase == 'estimate':
        required = accounting['rootAdmissionRequiredBytes']
        passed = observation['rootSpace']['availableBytes'] >= required
        return {
            **receipt,
            'phase': 'preallocation-estimate',
            'preallocationRequiredBytes': required,
            'estimatePassed': passed,
            'actualAdmissionRequired': True,
            'allocationAuthorized': False,
            'fixtureUseAuthorized': False,
            'admitted': False,
        }
    # reject unknown phases
    if phase != 'admit':
        raise ValueError('Phase must be estimate or admit')
    allocated = accounting['backingAllocatedBytes']
    logical = accounting['backingLogicalBytes']
    inner = observation.get('innerSpace')
    mounted = observation.get('fixtureMount')
    loop = observation.get('fixtureLoop')
    # bind actual storage identity
    identity_ok = (
        observation.get('backingFileOwnerPrivate') is True and
        observation.get('backingFileOnHostRoot') is True and
        observation.get('mountpointOwnerPrivate') is True and
        observation.get('mountDeviceMatchesLoop') is True and
        isinstance(mounted, dict) and
        mounted.get('target') == contract['mountpoint'] and
        mounted.get('filesystemType') == 'ext4' and
        re.fullmatch(r'/dev/loop[0-9]+', mounted.get('source', '')) is not None and
        'rw' in mounted.get('options', []) and
        isinstance(mounted.get('uuid'), str) and
        UUID_PATTERN.fullmatch(mounted['uuid']) is not None and
        isinstance(loop, dict) and
        loop.get('device') == mounted.get('source') and
        loop.get('backingFile') == contract['backingFile'] and
        loop.get('offsetBytes') == 0 and
        loop.get('sizeLimitBytes') in (0, normalized['backingBytes']) and
        loop.get('readOnly') is False
    )
    # validate actual inner values
    if not isinstance(inner, dict):
        raise TypeError('Mounted inner filesystem capacity is unavailable')
    inner_total = exact_integer(inner.get('totalBytes'), 'innerTotalBytes')
    inner_available = exact_integer(inner.get('availableBytes'), 'innerAvailableBytes')
    actual_peak = accounting['conservativeAdditionalPeakBytes']
    admitted = (
        identity_ok and
        inner_available <= inner_total <= logical and
        accounting['backingPresent'] is True and
        accounting['sourceArchivePresent'] is True and
        logical == normalized['backingBytes'] and
        allocated >= logical and
        actual_peak <= MAX_ADDITIONAL_BYTES and
        observation['rootSpace']['availableBytes'] >= accounting['rootAdmissionRequiredBytes'] and
        inner_available >= normalized['innerRequiredBytes']
    )
    return {
        **receipt,
        'phase': 'actual-admission',
        'fixtureFilesystem': mounted,
        'loopBinding': loop,
        'backingLogicalBytes': logical,
        'backingAllocatedBytes': allocated,
        'actualAdditionalPeakBytes': actual_peak,
        'innerAvailableBytes': inner_available,
        'innerTotalBytes': inner_total,
        'estimatePassed': None,
        'actualAdmissionRequired': False,
        'allocationAuthorized': False,
        'fixtureUseAuthorized': admitted,
        'admitted': admitted,
    }


# parse the fixed cli
def parse_args(argv=None):
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument('phase', choices=('estimate', 'admit'))
    parser.add_argument('--fixture-id', required=True)
    parser.add_argument('--copy-bytes', required=True, type=int)
    parser.add_argument('--backing-bytes', required=True, type=int)
    parser.add_argument('--other-peak-bytes', required=True, type=int)
    return parser.parse_args(argv)


# run one admission phase
def main(argv=None):
    args = parse_args(argv)
    contract = fixture_contract(args.fixture_id)
    plan = {
        'copyBytes': args.copy_bytes,
        'backingBytes': args.backing_bytes,
        'otherPeakBytes': args.other_peak_bytes,
    }
    common = collect_common(contract, plan)
    observation = collect_actual(contract, plan, common) if args.phase == 'admit' else common
    receipt = evaluate(args.phase, contract, plan, observation)
    print(json.dumps(receipt, sort_keys=True, indent=2), flush=True)
    # handle estimate outcome
    if args.phase == 'estimate':
        # accept only enough headroom
        if receipt['estimatePassed']:
            return 0
        print('PREALLOCATION ESTIMATE DENIED: do not allocate the benchmark fixture.', file=sys.stderr)
        return 2
    # accept only actual admission
    if receipt['admitted']:
        return 0
    print('NATIVE BENCHMARK STORAGE ADMISSION DENIED: do not copy or start the fixture.', file=sys.stderr)
    return 2


# expose the command
if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (json.JSONDecodeError, OSError, TypeError, ValueError, RuntimeError,
            subprocess.SubprocessError, KeyError) as error:
        print(f'NATIVE BENCHMARK STORAGE CHECK FAILED CLOSED: {error}', file=sys.stderr)
        raise SystemExit(2)
