#!/usr/bin/env python3
"""Produce one detached native ARM64 D-loop PostgreSQL cold witness."""

import argparse
import ctypes
import hashlib
import json
import os
import platform
import re
import stat
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from datetime import datetime, timedelta, timezone
from pathlib import Path

CONTRACT_VERSION = 'weather-page-d-loop-cold-proof/v1'
FIXTURE_KIND = 'weather-native-rust-rewrite-fixture'
FIXTURE_ID_PATTERN = re.compile(r'rust-rewrite-[a-f0-9]{16}')
LOOP_DEVICE_PATTERN = re.compile(r'/dev/loop[0-9]+')
UUID_PATTERN = re.compile(r'[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}')
VOLUME_PATTERN = re.compile(r'[A-Za-z0-9][A-Za-z0-9_.-]{0,127}')
MARKER_KEYS = {
    'backingFile',
    'fixtureId',
    'fixtureRoot',
    'kind',
    'mountpoint',
    'schemaVersion',
    'volumeNamespacePrefix',
    'volumeRoot',
}
STORAGE_KEYS = {
    'backingFile',
    'backingFileBytes',
    'loopDevice',
    'loopUuid',
    'mount',
    'mountSource',
    'volumeNames',
}
GIB = 1024 ** 3
ADMISSION_KEYS = {
    'actualAdditionalPeakBytes', 'actualAdmissionRequired', 'admissionReceiptPath',
    'admitted', 'allocationAuthorized', 'approvedCapExpiresAt', 'approvedCapPath',
    'approvedCapSha256', 'backingAllocatedBytes', 'backingFile',
    'backingLogicalBytes', 'backingBytes', 'capturedAt', 'copyBytes',
    'estimatePassed', 'fixtureFilesystem', 'fixtureId', 'fixtureRoot',
    'fixtureUseAuthorized', 'fixtureOuterAllocatedBytes',
    'fixtureOuterEntriesSha256', 'fixtureOuterEntryCount', 'innerAvailableBytes',
    'innerRequiredBytes', 'innerReserveBytes', 'innerTotalBytes', 'loopBinding',
    'markerPath', 'markerSha256', 'maxAdditionalBytes', 'mountpoint',
    'otherAllocatedBytes', 'otherPeakBytes', 'otherRemainingBytes',
    'peakRequiredBytes', 'phase', 'receiptVersion', 'rootAdmissionRequiredBytes',
    'rootAvailableBytes', 'rootFilesystem', 'rootReserveBytes', 'rootTotalBytes',
    'sourceArchiveAllocatedBytes', 'sourceArchiveLogicalBytes',
    'sourceArchivePresent', 'sourceArchiveSha256', 'unmeasuredOtherReserveBytes',
    'volumeNamespacePrefix', 'volumeRoot', 'backingAllocationRemainingBytes',
    'backingPresent', 'conservativeAdditionalPeakBytes',
    'externalCleanupCreditBytes',
}
FILESYSTEM_KEYS = {'filesystemType', 'options', 'source', 'target', 'uuid'}
LOOP_BINDING_KEYS = {
    'backingFile', 'device', 'offsetBytes', 'readOnly', 'sizeLimitBytes',
}
APPROVAL_KEYS = {
    'approvalAuthority', 'approvedAt', 'backingBytes', 'copyBytes', 'expiresAt',
    'fixtureId', 'fixtureRoot', 'innerReserveBytes', 'kind',
    'maxAdditionalBytes', 'otherPeakBytes', 'retentionSeconds',
    'rootFilesystemSource', 'rootFilesystemUuid', 'rootReserveBytes',
    'schemaVersion',
}


@dataclass(frozen=True)
class Policy:
    """Hold fixed fixture ownership boundaries."""

    base_root: Path = Path('/var/lib/weather-native-benchmark')
    runtime_root: Path = Path('/run')
    owner_uid: int = 0


@dataclass(frozen=True)
class FileFingerprint:
    """Identify one immutable private input."""

    device: int
    inode: int
    mode: int
    links: int
    owner_uid: int
    size: int
    modified_ns: int
    changed_ns: int


@dataclass(frozen=True)
class InventoryEntry:
    """Identify one stopped PGDATA file."""

    relative_path: str
    device: int
    inode: int
    mode: int
    links: int
    owner_uid: int
    owner_gid: int
    size: int
    modified_ns: int
    changed_ns: int


@dataclass(frozen=True)
class Inventory:
    """Retain one deterministic PGDATA inventory."""

    entries: tuple
    sha256: str
    total_bytes: int


class NativeHost:
    """Observe the native host and page cache."""

    # require the intended platform
    def require_platform(self):
        machine = platform.machine().lower()
        # reject other kernels and architectures
        if platform.system() != 'Linux' or machine not in {'aarch64', 'arm64'}:
            raise RuntimeError('Cold proof requires native Linux ARM64')

    # run one bounded JSON command
    def command_json(self, argv):
        completed = subprocess.run(
            argv,
            capture_output=True,
            check=True,
            text=True,
            timeout=60,
        )
        return json.loads(completed.stdout)

    # run one bounded text command
    def command_text(self, argv):
        completed = subprocess.run(
            argv,
            capture_output=True,
            check=True,
            text=True,
            timeout=60,
        )
        return completed.stdout.strip()

    # observe the exact ext4 loop mount
    def observe_storage(self, storage):
        mount_output = self.command_json([
            '/usr/bin/findmnt',
            '--json',
            '--target',
            storage['mount'],
            '--output',
            'TARGET,SOURCE,FSTYPE,OPTIONS,UUID',
        ])
        mount_record = single_record(mount_output, 'filesystems', 'fixture mount')
        option_text = mount_record.get('options')
        options = set(option_text.split(',')) if isinstance(option_text, str) else set()
        # bind every mount field
        if (
            mount_record.get('target') != storage['mount']
            or mount_record.get('source') != storage['mountSource']
            or mount_record.get('fstype') != 'ext4'
            or mount_record.get('uuid') != storage['loopUuid']
            or 'rw' not in options
        ):
            raise RuntimeError('Fixture mount identity changed')
        loop_output = self.command_json([
            '/usr/sbin/losetup',
            '--json',
            '--list',
            '--output',
            'NAME,BACK-FILE',
            storage['loopDevice'],
        ])
        loop_record = single_record(loop_output, 'loopdevices', 'loop binding')
        # bind the loop backing file
        if (
            loop_record.get('name') != storage['loopDevice']
            or loop_record.get('back-file') != storage['backingFile']
        ):
            raise RuntimeError('Loop backing identity changed')
        device_path = Path(storage['loopDevice'])
        device_metadata = os.stat(device_path, follow_symlinks=False)
        mount_metadata = os.stat(storage['mount'], follow_symlinks=False)
        # bind the block device
        if (
            device_path.resolve(strict=True) != device_path
            or not stat.S_ISBLK(device_metadata.st_mode)
            or device_metadata.st_rdev != mount_metadata.st_dev
        ):
            raise RuntimeError('Loop block-device identity changed')

    # inspect one Docker volume
    def volume_record(self, name):
        payload = self.command_json(['/usr/bin/docker', 'volume', 'inspect', name])
        # require one volume record
        if not isinstance(payload, list) or len(payload) != 1 or not isinstance(payload[0], dict):
            raise RuntimeError(f'Cannot identify fixture volume {name}')
        return payload[0]

    # list running volume attachments
    def running_attachments(self, name):
        output = self.command_text([
            '/usr/bin/docker',
            'ps',
            '--quiet',
            '--no-trunc',
            '--filter',
            f'volume={name}',
        ])
        return [] if output == '' else output.splitlines()

    # evict and measure one validated file
    def evict_and_measure(self, path, expected, flush_before_eviction=False):
        flags = os.O_RDONLY | os.O_CLOEXEC | getattr(os, 'O_NOFOLLOW', 0)
        descriptor = os.open(path, flags)
        # close every opened file
        try:
            metadata = os.fstat(descriptor)
            # bind the opened inode
            if not same_inventory_file(metadata, expected):
                raise RuntimeError(f'Fixture file changed before eviction: {path}')
            # require explicit fadvise support
            if not hasattr(os, 'posix_fadvise') or not hasattr(os, 'POSIX_FADV_DONTNEED'):
                raise RuntimeError('POSIX_FADV_DONTNEED is unavailable')
            # flush only the selected validated descriptor
            if flush_before_eviction:
                os.fsync(descriptor)
            os.posix_fadvise(descriptor, 0, 0, os.POSIX_FADV_DONTNEED)
            return mincore_residency(descriptor, metadata.st_size)
        # release the descriptor
        finally:
            os.close(descriptor)

    # return one UTC proof clock
    def now(self):
        return datetime.now(timezone.utc).isoformat(timespec='milliseconds').replace('+00:00', 'Z')


# select one exact command record
def single_record(payload, key, subject):
    records = payload.get(key) if isinstance(payload, dict) else None
    # require one record
    if not isinstance(records, list) or len(records) != 1 or not isinstance(records[0], dict):
        raise RuntimeError(f'Cannot identify one {subject}')
    return records[0]


# retain one private-file identity
def file_fingerprint(metadata):
    return FileFingerprint(
        device=metadata.st_dev,
        inode=metadata.st_ino,
        mode=metadata.st_mode,
        links=metadata.st_nlink,
        owner_uid=metadata.st_uid,
        size=metadata.st_size,
        modified_ns=metadata.st_mtime_ns,
        changed_ns=metadata.st_ctime_ns,
    )


# require one canonical path
def require_canonical(path, kind, owner_uid=None, mode=None, single_link=False):
    # reject aliases and links
    if not path.is_absolute() or path.resolve(strict=True) != path:
        raise RuntimeError(f'{path} must be an existing canonical path without symlinks')
    metadata = os.stat(path, follow_symlinks=False)
    # require regular files
    if kind == 'file' and not stat.S_ISREG(metadata.st_mode):
        raise RuntimeError(f'{path} must be a regular file')
    # require directories
    if kind == 'directory' and not stat.S_ISDIR(metadata.st_mode):
        raise RuntimeError(f'{path} must be a directory')
    # bind the owner
    if owner_uid is not None and metadata.st_uid != owner_uid:
        raise RuntimeError(f'{path} has the wrong owner')
    # bind the mode
    if mode is not None and stat.S_IMODE(metadata.st_mode) != mode:
        raise RuntimeError(f'{path} must have mode {mode:04o}')
    # exclude linked files
    if single_link and metadata.st_nlink != 1:
        raise RuntimeError(f'{path} must have one link')
    return metadata


# read one stable private JSON file
def read_private_json(path, owner_uid):
    metadata = require_canonical(path, 'file', owner_uid=owner_uid, mode=0o600, single_link=True)
    # bound the input size
    if metadata.st_size > 1024 * 1024:
        raise RuntimeError(f'{path} exceeds the 1 MiB input limit')
    flags = os.O_RDONLY | os.O_CLOEXEC | getattr(os, 'O_NOFOLLOW', 0)
    descriptor = os.open(path, flags)
    # bind one stable read
    try:
        before = file_fingerprint(os.fstat(descriptor))
        chunks = []
        remaining = metadata.st_size + 1
        # read every input byte
        while remaining > 0:
            chunk = os.read(descriptor, remaining)
            # stop only at eof
            if chunk == b'':
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        payload = b''.join(chunks)
        after = file_fingerprint(os.fstat(descriptor))
    # release the descriptor
    finally:
        os.close(descriptor)
    # reject read races
    if before != after or before != file_fingerprint(metadata) or len(payload) != metadata.st_size:
        raise RuntimeError(f'{path} changed while it was read')
    # parse only json
    try:
        value = json.loads(payload)
    # normalize parse failures
    except json.JSONDecodeError as error:
        raise RuntimeError(f'{path} is not valid JSON') from error
    # require one object
    if not isinstance(value, dict):
        raise TypeError(f'{path} must contain one JSON object')
    return value, before, hashlib.sha256(payload).hexdigest()


# recheck one private input
def require_unchanged(path, expected):
    current = require_canonical(
        path,
        'file',
        owner_uid=expected.owner_uid,
        mode=0o600,
        single_link=True,
    )
    # bind the final inode
    if file_fingerprint(current) != expected:
        raise RuntimeError(f'{path} changed during cold proof')


# require exact JSON keys
def require_keys(value, expected, label):
    # reject schema expansion
    if not isinstance(value, dict) or set(value) != expected:
        raise RuntimeError(f'{label} must contain exactly {sorted(expected)}')


# require one bounded integer
def require_integer(value, label, minimum=0):
    # reject coerced values
    if not isinstance(value, int) or isinstance(value, bool) or value < minimum:
        raise RuntimeError(f'{label} must be an integer of at least {minimum}')
    return value


# parse one fixed utc clock
def parse_clock(value, label):
    # parse only whole utc seconds
    try:
        return datetime.strptime(value, '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc)
    # normalize invalid clocks
    except (TypeError, ValueError) as error:
        raise RuntimeError(f'{label} is invalid') from error


# validate the outer fixture marker
def validate_marker(marker, marker_path, policy):
    require_keys(marker, MARKER_KEYS, 'Fixture marker')
    fixture_id = marker.get('fixtureId')
    # require the isolated id
    if not isinstance(fixture_id, str) or FIXTURE_ID_PATTERN.fullmatch(fixture_id) is None:
        raise RuntimeError('Fixture marker ID is invalid')
    fixture_root = policy.base_root / fixture_id
    expected = {
        'schemaVersion': 1,
        'kind': FIXTURE_KIND,
        'fixtureId': fixture_id,
        'fixtureRoot': str(fixture_root),
        'backingFile': str(fixture_root / 'fixture.ext4'),
        'mountpoint': str(fixture_root / 'mnt'),
        'volumeRoot': str(fixture_root / 'mnt' / 'docker-volumes'),
        'volumeNamespacePrefix': f'weather-native-benchmark-{fixture_id}-',
    }
    # bind every layout value
    if marker != expected:
        raise RuntimeError('Fixture marker does not match the fixed native layout')
    # bind the marker location
    if marker_path != fixture_root / '.fixture-marker.json':
        raise RuntimeError('Fixture marker path is invalid')
    return expected


# bind one actual admission receipt
def storage_from_admission(admission, admission_path, marker, marker_sha256, volume_names, policy):
    require_keys(admission, ADMISSION_KEYS, 'Storage admission')
    fixture_root = Path(marker['fixtureRoot'])
    expected_path = fixture_root / '.storage-admission.json'
    # bind the handoff location
    if admission_path != expected_path or admission.get('admissionReceiptPath') != str(expected_path):
        raise RuntimeError('Storage admission path is invalid')
    fixed_values = {
        'receiptVersion': 'weather-native-benchmark-storage/v1',
        'phase': 'actual-admission',
        'fixtureId': marker['fixtureId'],
        'fixtureRoot': marker['fixtureRoot'],
        'backingFile': marker['backingFile'],
        'mountpoint': marker['mountpoint'],
        'volumeRoot': marker['volumeRoot'],
        'volumeNamespacePrefix': marker['volumeNamespacePrefix'],
        'markerPath': str(fixture_root / '.fixture-marker.json'),
        'markerSha256': marker_sha256,
        'maxAdditionalBytes': 20 * GIB,
        'rootReserveBytes': 8 * GIB,
        'innerReserveBytes': 4 * GIB,
        'estimatePassed': None,
        'actualAdmissionRequired': False,
        'allocationAuthorized': False,
        'fixtureUseAuthorized': True,
        'admitted': True,
    }
    # bind every fixed receipt field
    for key, expected in fixed_values.items():
        # reject substituted fields
        if admission.get(key) != expected:
            raise RuntimeError(f'Storage admission does not bind {key}')
    integer_fields = {
        'copyBytes': 1,
        'backingBytes': 1,
        'otherPeakBytes': 0,
        'innerRequiredBytes': 1,
        'peakRequiredBytes': 1,
        'rootAvailableBytes': 0,
        'rootTotalBytes': 1,
        'backingLogicalBytes': 1,
        'backingAllocatedBytes': 1,
        'actualAdditionalPeakBytes': 1,
        'innerAvailableBytes': 0,
        'innerTotalBytes': 1,
        'fixtureOuterAllocatedBytes': 1,
        'fixtureOuterEntryCount': 1,
        'backingAllocationRemainingBytes': 0,
        'otherAllocatedBytes': 0,
        'otherRemainingBytes': 0,
        'unmeasuredOtherReserveBytes': 0,
        'externalCleanupCreditBytes': 0,
        'rootAdmissionRequiredBytes': 1,
        'conservativeAdditionalPeakBytes': 1,
        'sourceArchiveLogicalBytes': 1,
        'sourceArchiveAllocatedBytes': 1,
    }
    # validate every byte field
    for key, minimum in integer_fields.items():
        require_integer(admission.get(key), key, minimum)
    # retain exact byte accounting
    if (
        admission['backingBytes'] != admission['backingLogicalBytes']
        or admission['peakRequiredBytes'] != admission['backingBytes'] + admission['otherPeakBytes']
        or admission['innerRequiredBytes'] != admission['copyBytes'] + admission['innerReserveBytes']
        or admission['actualAdditionalPeakBytes']
        != admission['backingAllocatedBytes'] + admission['otherPeakBytes']
        or admission['actualAdditionalPeakBytes'] > admission['maxAdditionalBytes']
        or admission['actualAdditionalPeakBytes'] != admission['conservativeAdditionalPeakBytes']
        or admission['innerAvailableBytes'] < admission['innerRequiredBytes']
        or admission['innerAvailableBytes'] > admission['innerTotalBytes']
        or admission['innerTotalBytes'] > admission['backingLogicalBytes']
        or admission['backingAllocatedBytes'] < admission['backingLogicalBytes']
        or admission['rootAvailableBytes'] > admission['rootTotalBytes']
        or admission['backingPresent'] is not True
        or admission['backingAllocationRemainingBytes'] != 0
        or admission['fixtureOuterAllocatedBytes'] < admission['backingAllocatedBytes']
        or admission['otherAllocatedBytes']
        != admission['fixtureOuterAllocatedBytes'] - admission['backingAllocatedBytes']
        or admission['otherAllocatedBytes'] + admission['otherRemainingBytes']
        != admission['otherPeakBytes']
        or admission['unmeasuredOtherReserveBytes'] != admission['otherRemainingBytes']
        or admission['externalCleanupCreditBytes'] != 0
        or admission['rootAdmissionRequiredBytes']
        != admission['otherRemainingBytes'] + admission['rootReserveBytes']
        or admission['rootAvailableBytes'] < admission['rootAdmissionRequiredBytes']
        or admission['conservativeAdditionalPeakBytes']
        != admission['backingAllocatedBytes'] + admission['backingAllocationRemainingBytes']
        + admission['otherAllocatedBytes'] + admission['otherRemainingBytes']
        or admission['sourceArchivePresent'] is not True
        or admission['sourceArchiveAllocatedBytes'] > admission['otherAllocatedBytes']
    ):
        raise RuntimeError('Storage admission byte accounting is invalid')
    digest_fields = ('fixtureOuterEntriesSha256', 'sourceArchiveSha256')
    # validate every retained digest
    for key in digest_fields:
        value = admission.get(key)
        # require lowercase sha256
        if not isinstance(value, str) or re.fullmatch(r'[a-f0-9]{64}', value) is None:
            raise RuntimeError(f'Storage admission {key} is invalid')
    root_filesystem = admission.get('rootFilesystem')
    fixture_filesystem = admission.get('fixtureFilesystem')
    loop_binding = admission.get('loopBinding')
    require_keys(root_filesystem, FILESYSTEM_KEYS, 'Root filesystem identity')
    require_keys(fixture_filesystem, FILESYSTEM_KEYS, 'Fixture filesystem identity')
    require_keys(loop_binding, LOOP_BINDING_KEYS, 'Loop binding identity')
    root_options = root_filesystem.get('options')
    # bind the approved host root
    if (
        root_filesystem.get('target') != '/'
        or root_filesystem.get('filesystemType') != 'ext4'
        or not isinstance(root_options, list)
        or 'rw' not in root_options
        or 'ro' in root_options
        or not isinstance(root_filesystem.get('source'), str)
        or not root_filesystem['source'].startswith('/dev/')
        or LOOP_DEVICE_PATTERN.fullmatch(root_filesystem['source']) is not None
        or not isinstance(root_filesystem.get('uuid'), str)
        or root_filesystem['uuid'] == ''
    ):
        raise RuntimeError('Admitted root filesystem identity is invalid')
    fixture_options = fixture_filesystem.get('options')
    # bind the admitted ext4 mount
    if (
        fixture_filesystem.get('target') != marker['mountpoint']
        or fixture_filesystem.get('filesystemType') != 'ext4'
        or not isinstance(fixture_options, list)
        or 'rw' not in fixture_options
        or 'ro' in fixture_options
        or not isinstance(fixture_filesystem.get('source'), str)
        or LOOP_DEVICE_PATTERN.fullmatch(fixture_filesystem['source']) is None
        or not isinstance(fixture_filesystem.get('uuid'), str)
        or UUID_PATTERN.fullmatch(fixture_filesystem['uuid']) is None
    ):
        raise RuntimeError('Admitted fixture filesystem identity is invalid')
    # bind the admitted loop
    if (
        loop_binding.get('device') != fixture_filesystem['source']
        or loop_binding.get('backingFile') != marker['backingFile']
        or loop_binding.get('offsetBytes') != 0
        or loop_binding.get('sizeLimitBytes') not in (0, admission['backingBytes'])
        or loop_binding.get('readOnly') is not False
    ):
        raise RuntimeError('Admitted loop binding identity is invalid')
    approved_path = fixture_root / '.approved-cap.json'
    # bind the approval location
    if admission.get('approvedCapPath') != str(approved_path):
        raise RuntimeError('Storage admission approved-cap path is invalid')
    approval, _, approved_sha256 = read_private_json(approved_path, policy.owner_uid)
    # reject changed approval bytes
    if admission.get('approvedCapSha256') != approved_sha256:
        raise RuntimeError('Approved-cap receipt changed after storage admission')
    require_keys(approval, APPROVAL_KEYS, 'Storage approval')
    approval_values = {
        'schemaVersion': 1,
        'kind': 'weather-native-rust-rewrite-storage-approval',
        'approvalAuthority': 'ansel',
        'fixtureId': marker['fixtureId'],
        'fixtureRoot': marker['fixtureRoot'],
        'copyBytes': admission['copyBytes'],
        'backingBytes': admission['backingBytes'],
        'otherPeakBytes': admission['otherPeakBytes'],
        'maxAdditionalBytes': admission['maxAdditionalBytes'],
        'rootReserveBytes': admission['rootReserveBytes'],
        'innerReserveBytes': admission['innerReserveBytes'],
        'retentionSeconds': 24 * 60 * 60,
        'rootFilesystemSource': root_filesystem['source'],
        'rootFilesystemUuid': root_filesystem['uuid'],
        'expiresAt': admission['approvedCapExpiresAt'],
    }
    # bind every approved value
    for key, expected in approval_values.items():
        # reject changed authority
        if approval.get(key) != expected:
            raise RuntimeError(f'Storage approval does not bind {key}')
    approved_at = parse_clock(approval.get('approvedAt'), 'approvedAt')
    captured_at = parse_clock(admission.get('capturedAt'), 'capturedAt')
    expires = parse_clock(admission.get('approvedCapExpiresAt'), 'approvedCapExpiresAt')
    # retain the exact approval window
    if (
        expires - approved_at != timedelta(seconds=24 * 60 * 60)
        or captured_at < approved_at
        or captured_at >= expires
    ):
        raise RuntimeError('Storage admission is outside its approval window')
    # require current authority
    if datetime.now(timezone.utc) >= expires:
        raise RuntimeError('Storage approval has expired')
    return {
        'backingFile': marker['backingFile'],
        'backingFileBytes': admission['backingLogicalBytes'],
        'loopDevice': fixture_filesystem['source'],
        'loopUuid': fixture_filesystem['uuid'],
        'mount': marker['mountpoint'],
        'mountSource': fixture_filesystem['source'],
        'volumeNames': volume_names,
    }


# validate one frozen storage identity
def validate_storage(storage, marker):
    require_keys(storage, STORAGE_KEYS, 'Storage identity')
    volume_names = storage.get('volumeNames')
    # bind the loop identity
    if (
        storage.get('backingFile') != marker['backingFile']
        or storage.get('mount') != marker['mountpoint']
        or storage.get('mountSource') != storage.get('loopDevice')
        or not isinstance(storage.get('loopDevice'), str)
        or LOOP_DEVICE_PATTERN.fullmatch(storage['loopDevice']) is None
        or not isinstance(storage.get('loopUuid'), str)
        or UUID_PATTERN.fullmatch(storage['loopUuid']) is None
        or not isinstance(storage.get('backingFileBytes'), int)
        or isinstance(storage.get('backingFileBytes'), bool)
        or storage['backingFileBytes'] < 1
    ):
        raise RuntimeError('Storage identity does not match the fixture marker')
    # require five volume names
    if (
        not isinstance(volume_names, list)
        or len(volume_names) != 5
        or len(set(volume_names)) != 5
    ):
        raise RuntimeError('Storage identity must name five distinct fixture volumes')
    # build the frozen slot names
    expected_names = [f"{marker['volumeNamespacePrefix']}slot-{index:02d}" for index in range(1, 6)]
    # bind the five run slots
    if volume_names != expected_names:
        raise RuntimeError('Storage identity must use exact slot-01 through slot-05 volumes')
    # validate each fixture name
    for name in volume_names:
        # constrain the namespace
        if (
            not isinstance(name, str)
            or VOLUME_PATTERN.fullmatch(name) is None
            or not name.startswith(marker['volumeNamespacePrefix'])
            or name == marker['volumeNamespacePrefix']
        ):
            raise RuntimeError('Storage identity contains an invalid fixture volume name')


# validate fixed fixture filesystem paths
def validate_fixture_layout(marker, storage, policy):
    fixture_root = Path(marker['fixtureRoot'])
    backing_file = Path(marker['backingFile'])
    mountpoint = Path(marker['mountpoint'])
    volume_root = Path(marker['volumeRoot'])
    require_canonical(policy.base_root, 'directory', owner_uid=policy.owner_uid, mode=0o700)
    require_canonical(fixture_root, 'directory', owner_uid=policy.owner_uid, mode=0o700)
    backing_metadata = require_canonical(
        backing_file,
        'file',
        owner_uid=policy.owner_uid,
        mode=0o600,
        single_link=True,
    )
    require_canonical(mountpoint, 'directory', owner_uid=policy.owner_uid, mode=0o700)
    require_canonical(volume_root, 'directory', owner_uid=policy.owner_uid, mode=0o700)
    # bind the backing size
    if backing_metadata.st_size != storage['backingFileBytes']:
        raise RuntimeError('Loop backing-file size changed')
    volume_metadata = []
    # prove each volume descendant
    for name in storage['volumeNames']:
        device = volume_root / name
        metadata = require_canonical(device, 'directory')
        # bind the loop filesystem
        if device.parent != volume_root or metadata.st_dev != os.stat(mountpoint).st_dev:
            raise RuntimeError(f'Fixture volume {name} is outside its dedicated loop descendant')
        volume_metadata.append((metadata.st_dev, metadata.st_ino))
    # reject shared descendants
    if len(set(volume_metadata)) != len(volume_metadata):
        raise RuntimeError('Fixture volumes do not have distinct storage descendants')


# require one populated run slot
def validate_exclusive_target(marker, storage, target_volume):
    volume_root = Path(marker['volumeRoot'])
    # inspect every non-target slot
    for name in storage['volumeNames']:
        # preserve the selected slot
        if name == target_volume:
            continue
        device = volume_root / name
        # reject residual fixture bytes
        if any(device.iterdir()):
            raise RuntimeError(f'Non-target fixture volume is not empty: {name}')


# validate five detached Docker bindings
def validate_detached_volumes(host, marker, storage):
    volumes = []
    volume_root = Path(marker['volumeRoot'])
    expected_labels = {
        'weather.native-benchmark.fixture-id': marker['fixtureId'],
        'weather.native-benchmark.kind': 'rust-rewrite',
    }
    # inspect every frozen volume
    for name in storage['volumeNames']:
        record = host.volume_record(name)
        expected_options = {
            'device': str(volume_root / name),
            'o': 'bind',
            'type': 'none',
        }
        # bind options and labels
        if (
            record.get('Name') != name
            or record.get('Driver') != 'local'
            or record.get('Options') != expected_options
            or record.get('Labels') != expected_labels
        ):
            raise RuntimeError(f'Fixture volume {name} binding is invalid')
        attachments = host.running_attachments(name)
        # require detached volumes
        if attachments:
            raise RuntimeError(f'Fixture volume {name} has a running attachment')
        volumes.append({'name': name, 'runningAttachments': []})
    return volumes


# compare one file to inventory
def same_inventory_file(metadata, expected):
    return (
        stat.S_ISREG(metadata.st_mode)
        and metadata.st_dev == expected.device
        and metadata.st_ino == expected.inode
        and metadata.st_mode == expected.mode
        and metadata.st_nlink == expected.links
        and metadata.st_uid == expected.owner_uid
        and metadata.st_gid == expected.owner_gid
        and metadata.st_size == expected.size
        and metadata.st_mtime_ns == expected.modified_ns
        and metadata.st_ctime_ns == expected.changed_ns
    )


# inventory one stopped PGDATA tree
def inventory_pgdata(root, mountpoint):
    root_metadata = require_canonical(root, 'directory')
    mount_metadata = os.stat(mountpoint, follow_symlinks=False)
    # bind the loop filesystem
    if root_metadata.st_dev != mount_metadata.st_dev:
        raise RuntimeError('PGDATA is outside the fixture loop filesystem')
    entries = []
    # walk without following links
    for directory, names, filenames in os.walk(root, topdown=True, followlinks=False):
        names.sort()
        filenames.sort()
        directory_path = Path(directory)
        directory_metadata = os.lstat(directory_path)
        # retain one filesystem
        if not stat.S_ISDIR(directory_metadata.st_mode) or directory_metadata.st_dev != root_metadata.st_dev:
            raise RuntimeError(f'PGDATA directory is unsafe: {directory_path}')
        # validate each directory
        for name in names:
            child = directory_path / name
            child_metadata = os.lstat(child)
            # reject linked directories
            if not stat.S_ISDIR(child_metadata.st_mode) or stat.S_ISLNK(child_metadata.st_mode):
                raise RuntimeError(f'PGDATA contains a linked or special directory: {child}')
            # reject mounted descendants
            if child_metadata.st_dev != root_metadata.st_dev:
                raise RuntimeError(f'PGDATA crosses a filesystem boundary: {child}')
        # inventory each file
        for name in filenames:
            child = directory_path / name
            metadata = os.lstat(child)
            # reject links and special files
            if not stat.S_ISREG(metadata.st_mode) or stat.S_ISLNK(metadata.st_mode):
                raise RuntimeError(f'PGDATA contains a linked or special file: {child}')
            # require dedicated inodes
            if metadata.st_dev != root_metadata.st_dev or metadata.st_nlink != 1:
                raise RuntimeError(f'PGDATA file is not dedicated to the fixture: {child}')
            entries.append(InventoryEntry(
                relative_path=child.relative_to(root).as_posix(),
                device=metadata.st_dev,
                inode=metadata.st_ino,
                mode=metadata.st_mode,
                links=metadata.st_nlink,
                owner_uid=metadata.st_uid,
                owner_gid=metadata.st_gid,
                size=metadata.st_size,
                modified_ns=metadata.st_mtime_ns,
                changed_ns=metadata.st_ctime_ns,
            ))
    encoded = json.dumps(
        [entry.__dict__ for entry in entries],
        sort_keys=True,
        separators=(',', ':'),
    ).encode()
    return Inventory(
        entries=tuple(entries),
        sha256=hashlib.sha256(encoded).hexdigest(),
        total_bytes=sum(entry.size for entry in entries),
    )


# require stopped PGDATA
def require_stopped_pgdata(pgdata_root):
    pid_path = pgdata_root / 'postmaster.pid'
    # reject any live marker
    if os.path.lexists(pid_path):
        raise RuntimeError('PGDATA still contains postmaster.pid')


# count Linux page residency
def mincore_residency(descriptor, length):
    # accept empty files
    if length == 0:
        return 0, 0
    page_size = os.sysconf('SC_PAGE_SIZE')
    pages = (length + page_size - 1) // page_size
    libc = ctypes.CDLL(None, use_errno=True)
    libc.mmap.argtypes = [
        ctypes.c_void_p,
        ctypes.c_size_t,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_int,
        ctypes.c_longlong,
    ]
    libc.mmap.restype = ctypes.c_void_p
    libc.mincore.argtypes = [ctypes.c_void_p, ctypes.c_size_t, ctypes.c_void_p]
    libc.mincore.restype = ctypes.c_int
    libc.munmap.argtypes = [ctypes.c_void_p, ctypes.c_size_t]
    libc.munmap.restype = ctypes.c_int
    address = libc.mmap(None, length, 0, 1, descriptor, 0)
    failed_address = ctypes.c_void_p(-1).value
    # reject mapping failure
    if address == failed_address:
        error = ctypes.get_errno()
        raise OSError(error, os.strerror(error))
    vector = (ctypes.c_ubyte * pages)()
    # always unmap the probe
    try:
        # require a complete observation
        if libc.mincore(address, length, vector) != 0:
            error = ctypes.get_errno()
            raise OSError(error, os.strerror(error))
        resident = sum(1 for value in vector if value & 1)
        return pages, resident
    # release the mapping
    finally:
        # surface unmap failure
        if libc.munmap(address, length) != 0:
            error = ctypes.get_errno()
            raise OSError(error, os.strerror(error))


# flush and evict every inventoried file
def evict_inventory(host, root, inventory):
    total_pages = 0
    # evict each inventoried file
    for entry in inventory.entries:
        pages, resident = host.evict_and_measure(
            root / entry.relative_path,
            entry,
            flush_before_eviction=True,
        )
        # validate the observation
        if not isinstance(pages, int) or pages < 0 or not isinstance(resident, int) or resident < 0:
            raise RuntimeError('Invalid mincore observation')
        # require zero residency
        if resident != 0:
            raise RuntimeError(f'PGDATA remains resident after fadvise: {entry.relative_path}')
        total_pages += pages
    # require measurable pages
    if total_pages < 1:
        raise RuntimeError('PGDATA has no measurable file pages')
    return total_pages


# flush and evict the outer loop backing file
def evict_backing_file(host, storage, policy):
    backing = Path(storage['backingFile'])
    metadata = require_canonical(
        backing,
        'file',
        owner_uid=policy.owner_uid,
        mode=0o600,
        single_link=True,
    )
    # bind the outer file size immediately before opening it
    if metadata.st_size != storage['backingFileBytes']:
        raise RuntimeError('Loop backing-file size changed')
    expected = InventoryEntry(
        relative_path=backing.name,
        device=metadata.st_dev,
        inode=metadata.st_ino,
        mode=metadata.st_mode,
        links=metadata.st_nlink,
        owner_uid=metadata.st_uid,
        owner_gid=metadata.st_gid,
        size=metadata.st_size,
        modified_ns=metadata.st_mtime_ns,
        changed_ns=metadata.st_ctime_ns,
    )
    pages, resident = host.evict_and_measure(
        backing,
        expected,
        flush_before_eviction=True,
    )
    # require zero outer residency
    if pages < 1 or resident != 0:
        raise RuntimeError('Loop backing file remains resident after fadvise')
    return pages


# produce one exact v1 cold proof
def produce_proof(marker, storage, marker_path, target_volume, pgdata_root, policy, host):
    host.require_platform()
    validate_marker(marker, marker_path, policy)
    validate_storage(storage, marker)
    validate_fixture_layout(marker, storage, policy)
    host.observe_storage(storage)
    # bind the target volume
    if target_volume not in storage['volumeNames']:
        raise RuntimeError('Target volume is outside the frozen five-volume set')
    target_device = Path(marker['volumeRoot']) / target_volume
    # constrain PGDATA to its device
    if pgdata_root != target_device and target_device not in pgdata_root.parents:
        raise RuntimeError('PGDATA is outside the selected fixture volume')
    require_canonical(pgdata_root, 'directory')
    require_stopped_pgdata(pgdata_root)
    validate_exclusive_target(marker, storage, target_volume)
    volumes = validate_detached_volumes(host, marker, storage)
    before = inventory_pgdata(pgdata_root, Path(marker['mountpoint']))
    # require a real database tree
    if len(before.entries) < 1 or before.total_bytes < 1:
        raise RuntimeError('PGDATA inventory is empty')
    started_at = host.now()
    inner_pages = evict_inventory(host, pgdata_root, before)
    require_stopped_pgdata(pgdata_root)
    after = inventory_pgdata(pgdata_root, Path(marker['mountpoint']))
    # require immutable stopped bytes
    if after != before:
        raise RuntimeError('PGDATA inventory changed during cold proof')
    validate_exclusive_target(marker, storage, target_volume)
    validate_detached_volumes(host, marker, storage)
    host.observe_storage(storage)
    inner_completed_at = host.now()
    outer_pages = evict_backing_file(host, storage, policy)
    completed_at = host.now()
    return {
        'completedAt': completed_at,
        'contractVersion': CONTRACT_VERSION,
        'inner': {
            'completedAt': inner_completed_at,
            'completedFiles': len(before.entries),
            'fadviseCompleted': True,
            'fallbackUsed': False,
            'mincoreCompleted': True,
            'startedAt': started_at,
            'totalPages': inner_pages,
            'totalResidentAfter': 0,
        },
        'outer': {
            'backingFileBytes': storage['backingFileBytes'],
            'completedAt': completed_at,
            'completedFiles': 1,
            'fadviseCompleted': True,
            'fallbackUsed': False,
            'mincoreCompleted': True,
            'startedAt': inner_completed_at,
            'totalPages': outer_pages,
            'totalResidentAfter': 0,
        },
        'startedAt': started_at,
        'storageIdentity': storage,
        'target': {
            'fileCount': len(before.entries),
            'inventoryAfterSha256': after.sha256,
            'inventoryBeforeSha256': before.sha256,
            'pgdataRoot': str(pgdata_root),
            'postmasterPidAbsent': True,
            'totalBytes': before.total_bytes,
            'volumeName': target_volume,
        },
        'volumes': volumes,
    }


# write one immutable private proof
def write_private_proof(path, proof, owner_uid, output_root, forbidden_mount):
    # require an absolute output
    if not path.is_absolute():
        raise RuntimeError('Proof output path must be absolute')
    parent = path.parent
    require_canonical(output_root, 'directory', owner_uid=owner_uid, mode=0o700)
    # constrain the transient output
    if parent != output_root:
        raise RuntimeError('Proof output must be a direct child of its private runtime root')
    # avoid warming the loop
    if path == forbidden_mount or forbidden_mount in path.parents:
        raise RuntimeError('Proof output must be outside the loop mount')
    # refuse replacement
    if os.path.lexists(path):
        raise RuntimeError('Proof output already exists')
    payload = (json.dumps(proof, sort_keys=True, separators=(',', ':')) + '\n').encode()
    descriptor, temporary_name = tempfile.mkstemp(prefix=f'.{path.name}.', dir=parent)
    temporary = Path(temporary_name)
    # clean every temporary path
    try:
        os.fchmod(descriptor, 0o600)
        offset = 0
        # write every proof byte
        while offset < len(payload):
            written = os.write(descriptor, payload[offset:])
            # reject stalled writes
            if written < 1:
                raise OSError('Proof write made no progress')
            offset += written
        os.fsync(descriptor)
        os.close(descriptor)
        descriptor = -1
        os.link(temporary, path, follow_symlinks=False)
        os.unlink(temporary)
        directory_descriptor = os.open(parent, os.O_RDONLY | os.O_CLOEXEC)
        # sync the containing directory
        try:
            os.fsync(directory_descriptor)
        # release the directory
        finally:
            os.close(directory_descriptor)
    # remove incomplete output state
    finally:
        # close failed writes
        if descriptor >= 0:
            os.close(descriptor)
        # remove failed temporaries
        if temporary.exists():
            temporary.unlink()


# parse command-line inputs
def parse_args(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture-marker', required=True, type=Path,
                        help='fixed root-owned .fixture-marker.json')
    parser.add_argument('--storage-admission', required=True, type=Path,
                        help='actual root-owned .storage-admission.json')
    parser.add_argument('--volume-name', required=True, action='append',
                        help='repeat in exact slot-01 through slot-05 order')
    parser.add_argument('--target-volume', required=True,
                        help='one populated member of the frozen five-volume set')
    parser.add_argument('--pgdata-root', required=True, type=Path,
                        help='stopped PGDATA inside the target volume device')
    parser.add_argument('--output', required=True, type=Path,
                        help='new proof under /run/weather-native-benchmark-<fixture-id>')
    return parser.parse_args(argv)


# execute one guarded proof run
def execute(arguments, policy=None, host=None):
    selected_policy = Policy() if policy is None else policy
    selected_host = NativeHost() if host is None else host
    # require fixture ownership
    if os.geteuid() != selected_policy.owner_uid:
        raise RuntimeError('Cold proof must run as the fixture owner')
    marker, marker_fingerprint, marker_sha256 = read_private_json(
        arguments.fixture_marker,
        selected_policy.owner_uid,
    )
    validate_marker(marker, arguments.fixture_marker, selected_policy)
    admission, admission_fingerprint, _ = read_private_json(
        arguments.storage_admission,
        selected_policy.owner_uid,
    )
    approved_path = Path(marker['fixtureRoot']) / '.approved-cap.json'
    _, approval_fingerprint, _ = read_private_json(
        approved_path,
        selected_policy.owner_uid,
    )
    storage = storage_from_admission(
        admission,
        arguments.storage_admission,
        marker,
        marker_sha256,
        arguments.volume_name,
        selected_policy,
    )
    proof = produce_proof(
        marker,
        storage,
        arguments.fixture_marker,
        arguments.target_volume,
        arguments.pgdata_root,
        selected_policy,
        selected_host,
    )
    require_unchanged(arguments.fixture_marker, marker_fingerprint)
    require_unchanged(arguments.storage_admission, admission_fingerprint)
    require_unchanged(approved_path, approval_fingerprint)
    write_private_proof(
        arguments.output,
        proof,
        selected_policy.owner_uid,
        selected_policy.runtime_root / f"weather-native-benchmark-{marker['fixtureId']}",
        Path(marker['mountpoint']),
    )
    return proof


# report one fail-closed result
def main(argv=None):
    # fail closed on every expected error
    try:
        arguments = parse_args(sys.argv[1:] if argv is None else argv)
        execute(arguments)
    # report without a proof
    except (OSError, RuntimeError, subprocess.SubprocessError, TypeError, ValueError) as error:
        print(f'native-benchmark-cold-proof: {error}', file=sys.stderr)
        return 1
    return 0


# run the command directly
if __name__ == '__main__':
    raise SystemExit(main())
