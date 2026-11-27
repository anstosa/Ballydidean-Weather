#!/usr/bin/env python3
"""Restore one approved native benchmark PostgreSQL fixture from standard input."""

import argparse
import base64
import hashlib
import hmac
import io
import json
import os
import re
import secrets
import selectors
import stat
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path

FIXTURE_BASE = Path('/var/lib/weather-native-benchmark')
MARKER_NAME = '.fixture-marker.json'
APPROVAL_NAME = '.approved-cap.json'
ADMISSION_NAME = '.storage-admission.json'
HANDOFF_NAME = '.restore-handoff.json'
FAILURE_NAME = '.restore-failure.json'
FAILURE_KIND = 'weather-native-rust-rewrite-restore-failure'
MARKER_KIND = 'weather-native-rust-rewrite-fixture'
APPROVAL_KIND = 'weather-native-rust-rewrite-storage-approval'
EXPECTATION_KIND = 'weather-native-rust-rewrite-restore-expectation'
HANDOFF_KIND = 'weather-native-rust-rewrite-restore'
VOLUME_KIND = 'rust-rewrite'
CONTAINER_ROLE = 'fixture-restore'
DOCKER = '/usr/bin/docker'
DATABASE_UID = 999
DATABASE_GID = 999
SECRET_NAMES = (
    'weather_admin_password',
    'weather_owner_password',
    'weather_api_password',
    'weather_ingest_password',
    'weather_training_export_password',
)
# define the five fixed slots
VOLUME_SUFFIXES = tuple(f'slot-{index:02d}' for index in range(1, 6))
SHA256_RE = re.compile(r'[0-9a-f]{64}')
IMAGE_ID_RE = re.compile(r'sha256:[0-9a-f]{64}')
FIXTURE_ID_RE = re.compile(r'rust-rewrite-[0-9a-f]{16}')
MIGRATION_RE = re.compile(r'[0-9]{4}_[a-z0-9_]+\.sql')
SAMPLE_HASH_RE = re.compile(r'[0-9a-f]{32}')
VERIFICATION_COUNT_TIMEOUT_SECONDS = 900
RFC3339_RE = re.compile(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z')
ADMISSION_KEYS = {
    'receiptVersion', 'capturedAt', 'phase', 'fixtureId', 'fixtureRoot',
    'backingFile', 'mountpoint', 'volumeRoot', 'volumeNamespacePrefix',
    'admissionReceiptPath', 'copyBytes', 'backingBytes', 'otherPeakBytes',
    'maxAdditionalBytes', 'rootReserveBytes', 'innerReserveBytes',
    'innerRequiredBytes', 'peakRequiredBytes', 'rootAvailableBytes',
    'rootTotalBytes', 'rootFilesystem', 'markerPath', 'markerSha256',
    'approvedCapPath', 'approvedCapSha256', 'approvedCapExpiresAt',
    'fixtureOuterAllocatedBytes', 'fixtureOuterEntryCount',
    'fixtureOuterEntriesSha256', 'backingPresent',
    'backingAllocationRemainingBytes', 'otherAllocatedBytes',
    'otherRemainingBytes', 'unmeasuredOtherReserveBytes',
    'externalCleanupCreditBytes',
    'rootAdmissionRequiredBytes', 'conservativeAdditionalPeakBytes',
    'sourceArchivePresent', 'sourceArchiveSha256',
    'sourceArchiveLogicalBytes', 'sourceArchiveAllocatedBytes',
    'fixtureFilesystem', 'loopBinding', 'backingLogicalBytes',
    'backingAllocatedBytes', 'actualAdditionalPeakBytes',
    'innerAvailableBytes', 'innerTotalBytes', 'estimatePassed',
    'actualAdmissionRequired', 'allocationAuthorized',
    'fixtureUseAuthorized', 'admitted',
}
APPROVAL_KEYS = {
    'schemaVersion', 'kind', 'approvalAuthority', 'fixtureId', 'fixtureRoot',
    'copyBytes', 'backingBytes', 'otherPeakBytes', 'maxAdditionalBytes',
    'rootReserveBytes', 'innerReserveBytes', 'retentionSeconds',
    'rootFilesystemSource', 'rootFilesystemUuid', 'approvedAt', 'expiresAt',
}


class RestoreError(RuntimeError):
    """Report one bounded fixture restore failure."""


CLEANUP_EXCEPTIONS = (
    OSError,
    RestoreError,
    subprocess.SubprocessError,
    TypeError,
    UnicodeError,
    ValueError,
)


# produce one utc timestamp
def utc_now():
    return datetime.now(timezone.utc)


# parse one utc timestamp
def parse_timestamp(value, description):
    # reject malformed timestamp values
    if not isinstance(value, str) or not RFC3339_RE.fullmatch(value):
        raise RestoreError(f'{description} must use YYYY-MM-DDTHH:MM:SSZ')
    try:
        parsed = datetime.strptime(value, '%Y-%m-%dT%H:%M:%SZ').replace(tzinfo=timezone.utc)
    except ValueError as error:
        raise RestoreError(f'{description} must use YYYY-MM-DDTHH:MM:SSZ') from error
    return parsed


# hash one regular file
def file_sha256(path):
    digest = hashlib.sha256()
    with path.open('rb') as source:
        # bound memory use
        while True:
            chunk = source.read(1024 * 1024)
            # stop at eof
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


# read one stable private json file
def read_private_json(path, description, owner=(0, 0)):
    metadata = require_path(path, description, 'file', owner=owner, mode=0o600)
    # reject aliases and oversized authority files
    if metadata.st_nlink != 1 or metadata.st_size > 65536 or path.resolve(strict=True) != path:
        raise RestoreError(f'{description} must be one canonical private file')
    descriptor = os.open(path, os.O_RDONLY | os.O_CLOEXEC | os.O_NOFOLLOW)
    try:
        opened = os.fstat(descriptor)
        # bind the opened inode
        if ((opened.st_dev, opened.st_ino, opened.st_size) !=
                (metadata.st_dev, metadata.st_ino, metadata.st_size)):
            raise RestoreError(f'{description} changed during validation')
        raw = os.read(descriptor, 65537)
        final = os.fstat(descriptor)
        # reject concurrent mutation
        if (len(raw) != metadata.st_size or len(raw) > 65536 or
                (final.st_dev, final.st_ino, final.st_size, final.st_mtime_ns,
                 final.st_ctime_ns) !=
                (metadata.st_dev, metadata.st_ino, metadata.st_size,
                 metadata.st_mtime_ns, metadata.st_ctime_ns)):
            raise RestoreError(f'{description} changed during validation')
    finally:
        os.close(descriptor)
    try:
        payload = json.loads(raw.decode('utf-8'))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise RestoreError(f'{description} is not valid JSON') from error
    # require one object
    if not isinstance(payload, dict):
        raise RestoreError(f'{description} must be a JSON object')
    return payload, hashlib.sha256(raw).hexdigest()


# compare exact object keys
def require_exact_keys(payload, expected, description):
    # reject schema drift
    if set(payload) != set(expected):
        missing = sorted(set(expected) - set(payload))
        extra = sorted(set(payload) - set(expected))
        raise RestoreError(f'{description} keys differ: missing={missing} extra={extra}')


# inspect one path without following links
def require_path(path, description, kind, owner=None, mode=None):
    try:
        metadata = path.lstat()
    except OSError as error:
        raise RestoreError(f'{description} is unavailable') from error
    # reject every symlink
    if stat.S_ISLNK(metadata.st_mode):
        raise RestoreError(f'{description} must not be a symlink')
    # require the requested type
    if kind == 'directory' and not stat.S_ISDIR(metadata.st_mode):
        raise RestoreError(f'{description} must be a directory')
    # require a regular file
    if kind == 'file' and not stat.S_ISREG(metadata.st_mode):
        raise RestoreError(f'{description} must be a regular file')
    # bind the owner
    if owner is not None and (metadata.st_uid, metadata.st_gid) != owner:
        raise RestoreError(f'{description} ownership differs')
    # bind the permission bits
    if mode is not None and stat.S_IMODE(metadata.st_mode) != mode:
        raise RestoreError(f'{description} mode differs')
    return metadata


# require a canonical absolute path
def require_canonical(path, description):
    # reject relative and aliased paths
    if not path.is_absolute() or path.resolve(strict=True) != path:
        raise RestoreError(f'{description} must be an existing canonical absolute path')
    return path


# validate the fixture authority marker
def validate_marker(fixture_root, fixture_base=FIXTURE_BASE, authority_owner=(0, 0)):
    require_path(fixture_base, 'fixture base', 'directory', owner=authority_owner, mode=0o700)
    require_canonical(fixture_base, 'fixture base')
    require_path(fixture_root, 'fixture root', 'directory', owner=authority_owner, mode=0o700)
    require_canonical(fixture_root, 'fixture root')
    # require one direct approved fixture descendant
    if fixture_root.parent != fixture_base or not FIXTURE_ID_RE.fullmatch(fixture_root.name):
        raise RestoreError('fixture root is outside the approved namespace')
    marker_path = fixture_root / MARKER_NAME
    require_path(marker_path, 'fixture marker', 'file', owner=authority_owner, mode=0o600)
    marker, marker_sha256 = read_private_json(marker_path, 'fixture marker', authority_owner)
    expected_keys = {
        'schemaVersion', 'kind', 'fixtureId', 'fixtureRoot', 'backingFile',
        'mountpoint', 'volumeRoot', 'volumeNamespacePrefix',
    }
    require_exact_keys(marker, expected_keys, 'fixture marker')
    fixture_id = fixture_root.name
    expected = {
        'schemaVersion': 1,
        'kind': MARKER_KIND,
        'fixtureId': fixture_id,
        'fixtureRoot': str(fixture_root),
        'backingFile': str(fixture_root / 'fixture.ext4'),
        'mountpoint': str(fixture_root / 'mnt'),
        'volumeRoot': str(fixture_root / 'mnt' / 'docker-volumes'),
        'volumeNamespacePrefix': f'weather-native-benchmark-{fixture_id}-',
    }
    # require every fixed marker value
    if marker != expected:
        raise RestoreError('fixture marker values differ from the approved namespace')
    mountpoint = Path(marker['mountpoint'])
    volume_root = Path(marker['volumeRoot'])
    require_path(mountpoint, 'fixture mountpoint', 'directory', owner=authority_owner, mode=0o700)
    require_canonical(mountpoint, 'fixture mountpoint')
    require_path(volume_root, 'fixture volume root', 'directory', owner=authority_owner, mode=0o700)
    require_canonical(volume_root, 'fixture volume root')
    # reject mount escapes
    if volume_root.parent != mountpoint:
        raise RestoreError('fixture volume root is outside the approved mountpoint')
    return marker, marker_path, marker_sha256


# parse one findmnt record
def filesystem_record(result, description):
    try:
        payload = json.loads(result.stdout.decode('utf-8'))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise RestoreError(f'{description} returned invalid JSON') from error
    # select object records only
    records = payload.get('filesystems') if isinstance(payload, dict) else None
    # require one exact filesystem
    if not isinstance(records, list) or len(records) != 1 or not isinstance(records[0], dict):
        raise RestoreError(f'{description} did not identify one filesystem')
    return records[0]


# parse one losetup record
def loop_record(result):
    try:
        payload = json.loads(result.stdout.decode('utf-8'))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise RestoreError('loop inspection returned invalid JSON') from error
    # select object records only
    records = payload.get('loopdevices') if isinstance(payload, dict) else None
    # require one exact loop binding
    if not isinstance(records, list) or len(records) != 1 or not isinstance(records[0], dict):
        raise RestoreError('loop inspection did not identify one binding')
    return records[0]


# inspect one loop binding with supported util-linux flags
def inspect_loop_binding(device, command=None):
    # select the real probe runner
    if command is None:
        command = run_command
    result = command([
        '/usr/sbin/losetup', '--json', '--list',
        '--output', 'NAME,BACK-FILE,OFFSET,SIZELIMIT,RO', device,
    ], timeout=30)
    return loop_record(result)


# require one positive json integer
def receipt_integer(value, description, allow_zero=False):
    # choose the exact lower bound
    minimum = 0 if allow_zero else 1
    # reject booleans and coercion
    if not isinstance(value, int) or isinstance(value, bool) or value < minimum:
        raise RestoreError(f'{description} is invalid')
    return value


# select the lifecycle-specific live storage headroom
def required_storage_headroom(copy_bytes, root_admission_required, root_reserve,
                              inner_reserve, restored_fixture):
    # retain only approved reserves after the admitted copy already exists
    if restored_fixture:
        return root_reserve, inner_reserve
    return root_admission_required, copy_bytes + inner_reserve


# validate live backing allocation against the applicable lifecycle authority
def validate_backing_allocation(metadata, logical_bytes, admitted_allocated_bytes,
                                other_peak_bytes, max_additional_bytes,
                                restored_fixture):
    current_allocated_bytes = metadata.st_blocks * 512
    # always bind the canonical logical file and reject aliases
    if metadata.st_nlink != 1 or metadata.st_size != logical_bytes:
        raise RestoreError('fixture backing file allocation differs from admission')
    # completed restores may consume additional preallocated filesystem blocks
    if restored_fixture:
        if (current_allocated_bytes < logical_bytes or
                current_allocated_bytes + other_peak_bytes > max_additional_bytes):
            raise RestoreError('restored fixture backing allocation exceeds approved cap')
        return current_allocated_bytes
    # initial empty restores retain exact admission-time allocation identity
    if current_allocated_bytes != admitted_allocated_bytes:
        raise RestoreError('fixture backing file allocation differs from admission')
    return current_allocated_bytes


# validate current storage admission and approval
def validate_admission(marker, marker_sha256, fixture_root, max_age_seconds,
                       now=None, command=None, authority_owner=(0, 0),
                       restored_fixture=False):
    # select the real probe runner
    if command is None:
        command = run_command
    captured_now = now or utc_now()
    approval_path = fixture_root / APPROVAL_NAME
    receipt_path = fixture_root / ADMISSION_NAME
    approval, approval_sha256 = read_private_json(
        approval_path, 'approved storage cap', authority_owner,
    )
    receipt, receipt_sha256 = read_private_json(
        receipt_path, 'storage admission receipt', authority_owner,
    )
    require_exact_keys(approval, APPROVAL_KEYS, 'approved storage cap')
    require_exact_keys(receipt, ADMISSION_KEYS, 'storage admission receipt')
    # require the exact approval identity
    if (approval.get('schemaVersion') != 1 or approval.get('kind') != APPROVAL_KIND or
            approval.get('approvalAuthority') != 'ansel' or
            approval.get('fixtureId') != marker['fixtureId'] or
            approval.get('fixtureRoot') != marker['fixtureRoot'] or
            approval.get('maxAdditionalBytes') != 20 * 1024 ** 3 or
            approval.get('rootReserveBytes') != 8 * 1024 ** 3 or
            approval.get('innerReserveBytes') != 4 * 1024 ** 3 or
            approval.get('retentionSeconds') != 86400):
        raise RestoreError('approved storage cap values differ')
    root_source = approval.get('rootFilesystemSource')
    root_uuid = approval.get('rootFilesystemUuid')
    # require the approved physical root identity
    if (not isinstance(root_source, str) or not root_source.startswith('/dev/') or
            re.fullmatch(r'/dev/loop[0-9]+', root_source) or
            not isinstance(root_uuid, str) or not re.fullmatch(r'[A-Za-z0-9.-]{1,128}', root_uuid)):
        raise RestoreError('approved root filesystem identity is invalid')
    approved_at = parse_timestamp(approval.get('approvedAt'), 'approvedAt')
    expires_at = parse_timestamp(approval.get('expiresAt'), 'expiresAt')
    # enforce exact approval lifetime and current validity
    if ((expires_at - approved_at).total_seconds() != 86400 or
            captured_now < approved_at or captured_now >= expires_at):
        raise RestoreError('approved storage cap is not currently valid')
    expected_receipt_values = {
        'receiptVersion': 'weather-native-benchmark-storage/v1',
        'phase': 'actual-admission',
        'fixtureId': marker['fixtureId'],
        'fixtureRoot': marker['fixtureRoot'],
        'backingFile': marker['backingFile'],
        'mountpoint': marker['mountpoint'],
        'volumeRoot': marker['volumeRoot'],
        'volumeNamespacePrefix': marker['volumeNamespacePrefix'],
        'admissionReceiptPath': str(receipt_path),
        'markerPath': str(fixture_root / MARKER_NAME),
        'markerSha256': marker_sha256,
        'approvedCapPath': str(approval_path),
        'approvedCapSha256': approval_sha256,
        'approvedCapExpiresAt': approval['expiresAt'],
        'estimatePassed': None,
        'actualAdmissionRequired': False,
        'allocationAuthorized': False,
        'fixtureUseAuthorized': True,
        'admitted': True,
    }
    # bind every fixed receipt value
    for name, value in expected_receipt_values.items():
        # reject any mismatch
        if receipt.get(name) != value:
            raise RestoreError(f'storage admission receipt does not bind {name}')
    # bind the approved byte plan
    for name in ('copyBytes', 'backingBytes', 'otherPeakBytes', 'maxAdditionalBytes',
                 'rootReserveBytes', 'innerReserveBytes'):
        # require approval equality
        if receipt.get(name) != approval.get(name):
            raise RestoreError(f'storage admission receipt does not bind approved {name}')
    captured_at = parse_timestamp(receipt.get('capturedAt'), 'capturedAt')
    # require a fresh nonfuture admission
    if (captured_at < approved_at or captured_at >= expires_at or
            captured_at > captured_now or
            (not restored_fixture and
             (captured_now - captured_at).total_seconds() > max_age_seconds)):
        raise RestoreError('storage admission receipt is stale or outside approval')
    copy_bytes = receipt_integer(receipt.get('copyBytes'), 'copyBytes')
    backing_bytes = receipt_integer(receipt.get('backingBytes'), 'backingBytes')
    other_peak_bytes = receipt_integer(receipt.get('otherPeakBytes'), 'otherPeakBytes', True)
    inner_reserve = receipt_integer(receipt.get('innerReserveBytes'), 'innerReserveBytes')
    root_reserve = receipt_integer(receipt.get('rootReserveBytes'), 'rootReserveBytes')
    max_additional = receipt_integer(receipt.get('maxAdditionalBytes'), 'maxAdditionalBytes')
    backing_logical = receipt_integer(receipt.get('backingLogicalBytes'), 'backingLogicalBytes')
    backing_allocated = receipt_integer(
        receipt.get('backingAllocatedBytes'), 'backingAllocatedBytes',
    )
    root_available = receipt_integer(receipt.get('rootAvailableBytes'), 'rootAvailableBytes')
    root_total = receipt_integer(receipt.get('rootTotalBytes'), 'rootTotalBytes')
    inner_available = receipt_integer(receipt.get('innerAvailableBytes'), 'innerAvailableBytes')
    inner_total = receipt_integer(receipt.get('innerTotalBytes'), 'innerTotalBytes')
    outer_allocated = receipt_integer(
        receipt.get('fixtureOuterAllocatedBytes'), 'fixtureOuterAllocatedBytes', True,
    )
    receipt_integer(receipt.get('fixtureOuterEntryCount'), 'fixtureOuterEntryCount')
    outer_entries_sha256 = receipt.get('fixtureOuterEntriesSha256')
    other_allocated = receipt_integer(
        receipt.get('otherAllocatedBytes'), 'otherAllocatedBytes', True,
    )
    other_remaining = receipt_integer(
        receipt.get('otherRemainingBytes'), 'otherRemainingBytes', True,
    )
    root_admission_required = receipt_integer(
        receipt.get('rootAdmissionRequiredBytes'), 'rootAdmissionRequiredBytes',
    )
    receipt_integer(
        receipt.get('sourceArchiveLogicalBytes'), 'sourceArchiveLogicalBytes',
    )
    source_archive_allocated = receipt_integer(
        receipt.get('sourceArchiveAllocatedBytes'), 'sourceArchiveAllocatedBytes',
    )
    # recompute all receipt arithmetic
    if (receipt.get('innerRequiredBytes') != copy_bytes + inner_reserve or
            receipt.get('peakRequiredBytes') != backing_bytes + other_peak_bytes or
            receipt.get('actualAdditionalPeakBytes') != backing_allocated + other_peak_bytes or
            receipt.get('conservativeAdditionalPeakBytes') != backing_allocated + other_peak_bytes or
            receipt.get('actualAdditionalPeakBytes') > max_additional or
            backing_bytes < copy_bytes + inner_reserve or backing_logical != backing_bytes or
            backing_allocated < backing_logical or root_available > root_total or
            receipt.get('backingPresent') is not True or
            receipt.get('backingAllocationRemainingBytes') != 0 or
            outer_allocated < backing_allocated or
            other_allocated != outer_allocated - backing_allocated or
            other_remaining != other_peak_bytes - other_allocated or
            receipt.get('unmeasuredOtherReserveBytes') != other_remaining or
            receipt.get('externalCleanupCreditBytes') != 0 or
            root_admission_required != other_remaining + root_reserve or
            root_available < root_admission_required or
            inner_available > inner_total or inner_total > backing_logical or
            inner_available < copy_bytes + inner_reserve or
            not isinstance(outer_entries_sha256, str) or
            not SHA256_RE.fullmatch(outer_entries_sha256) or
            receipt.get('sourceArchivePresent') is not True or
            not isinstance(receipt.get('sourceArchiveSha256'), str) or
            not SHA256_RE.fullmatch(receipt['sourceArchiveSha256']) or
            source_archive_allocated > other_allocated):
        raise RestoreError('storage admission receipt arithmetic differs')
    backing_file = Path(marker['backingFile'])
    backing_metadata = require_path(
        backing_file, 'fixture backing file', 'file', owner=authority_owner, mode=0o600,
    )
    require_canonical(backing_file, 'fixture backing file')
    validate_backing_allocation(
        backing_metadata,
        backing_bytes,
        backing_allocated,
        other_peak_bytes,
        max_additional,
        restored_fixture,
    )
    # retain exact backing filesystem identity
    if (backing_metadata.st_dev != os.stat('/').st_dev or
            os.stat(fixture_root).st_dev != os.stat('/').st_dev):
        raise RestoreError('fixture backing file allocation differs from admission')
    root_result = command([
        '/usr/bin/findmnt', '--json', '--target', '/',
        '--output', 'TARGET,SOURCE,FSTYPE,OPTIONS,UUID',
    ], timeout=30)
    root_record = filesystem_record(root_result, 'root filesystem inspection')
    fixture_result = command([
        '/usr/bin/findmnt', '--json', '--target', marker['mountpoint'],
        '--output', 'TARGET,SOURCE,FSTYPE,OPTIONS,UUID',
    ], timeout=30)
    fixture_record = filesystem_record(fixture_result, 'fixture filesystem inspection')
    root_receipt = receipt.get('rootFilesystem')
    fixture_receipt = receipt.get('fixtureFilesystem')
    expected_filesystem_keys = {'target', 'source', 'filesystemType', 'options', 'uuid'}
    expected_loop_keys = {
        'device', 'backingFile', 'offsetBytes', 'sizeLimitBytes', 'readOnly',
    }
    # bind the live filesystem identities
    if (not isinstance(root_receipt, dict) or set(root_receipt) != expected_filesystem_keys or
            not isinstance(fixture_receipt, dict) or
            set(fixture_receipt) != expected_filesystem_keys or
            not isinstance(root_receipt.get('options'), list) or
            not isinstance(fixture_receipt.get('options'), list) or
            root_receipt.get('source') != approval.get('rootFilesystemSource') or
            root_receipt.get('uuid') != approval.get('rootFilesystemUuid') or
            root_record.get('target') != root_receipt.get('target') or
            root_record.get('source') != root_receipt.get('source') or
            root_record.get('fstype') != root_receipt.get('filesystemType') or
            root_record.get('uuid') != root_receipt.get('uuid') or
            fixture_record.get('target') != fixture_receipt.get('target') or
            fixture_record.get('source') != fixture_receipt.get('source') or
            fixture_record.get('fstype') != fixture_receipt.get('filesystemType') or
            fixture_record.get('uuid') != fixture_receipt.get('uuid')):
        raise RestoreError('live filesystem identity differs from admission')
    root_options = set((root_record.get('options') or '').split(','))
    fixture_options = set((fixture_record.get('options') or '').split(','))
    # require writable ext4 at both layers
    if (root_record.get('fstype') != 'ext4' or fixture_record.get('fstype') != 'ext4' or
            'rw' not in root_options or 'ro' in root_options or
            'rw' not in fixture_options or 'ro' in fixture_options or
            root_options != set(root_receipt['options']) or
            fixture_options != set(fixture_receipt['options'])):
        raise RestoreError('live fixture storage must remain writable ext4')
    live_loop = inspect_loop_binding(fixture_record['source'], command)
    receipt_loop = receipt.get('loopBinding')
    # bind the exact live loop device
    if (not isinstance(receipt_loop, dict) or set(receipt_loop) != expected_loop_keys or
            live_loop.get('name') != receipt_loop.get('device') or
            live_loop.get('back-file') != receipt_loop.get('backingFile') or
            str(live_loop.get('offset')) != str(receipt_loop.get('offsetBytes')) or
            str(live_loop.get('sizelimit')) != str(receipt_loop.get('sizeLimitBytes')) or
            live_loop.get('ro') not in (False, 0, '0')):
        raise RestoreError('live loop binding differs from admission')
    root_space = os.statvfs('/')
    inner_space = os.statvfs(marker['mountpoint'])
    current_root_available = root_space.f_bavail * root_space.f_frsize
    current_inner_available = inner_space.f_bavail * inner_space.f_frsize
    # recheck both current reserves
    required_root_available, required_inner_available = required_storage_headroom(
        copy_bytes,
        root_admission_required,
        root_reserve,
        inner_reserve,
        restored_fixture,
    )
    # reject current storage below the applicable lifecycle reserves
    if (current_root_available < required_root_available or
            current_inner_available < required_inner_available):
        raise RestoreError('current fixture storage headroom no longer satisfies admission')
    return receipt, receipt_path, receipt_sha256, approval


# validate one immutable restore expectation
def validate_expectation(path, expected_sha256, owner=(0, 0)):
    payload, actual_file_sha256 = read_private_json(path, 'restore expectation', owner)
    # bind the exact caller-reviewed bytes
    if not SHA256_RE.fullmatch(expected_sha256) or actual_file_sha256 != expected_sha256:
        raise RestoreError('restore expectation SHA-256 differs')
    require_exact_keys(
        payload,
        {'schemaVersion', 'kind', 'databaseName', 'archiveSha256', 'migrations',
         'counts', 'sampleHash'},
        'restore expectation',
    )
    # require the expected schema
    if payload['schemaVersion'] != 1 or payload['kind'] != EXPECTATION_KIND:
        raise RestoreError('restore expectation schema differs')
    database_name = payload['databaseName']
    # restrict database identifiers
    if not isinstance(database_name, str) or not re.fullmatch(r'[a-z][a-z0-9_]{0,62}', database_name):
        raise RestoreError('restore expectation database name is invalid')
    # bind the source stream digest
    if not isinstance(payload['archiveSha256'], str) or not SHA256_RE.fullmatch(payload['archiveSha256']):
        raise RestoreError('restore expectation archive SHA-256 is invalid')
    migrations = payload['migrations']
    # require a nonempty ordered ledger
    if not isinstance(migrations, list) or not migrations:
        raise RestoreError('restore expectation migrations must be a nonempty array')
    normalized_migrations = []
    seen_names = set()
    # validate every migration pair
    for entry in migrations:
        # require the exact pair
        if not isinstance(entry, dict) or set(entry) != {'name', 'checksum'}:
            raise RestoreError('restore expectation migration entry differs')
        name = entry['name']
        checksum = entry['checksum']
        # restrict ledger values
        if (not isinstance(name, str) or not MIGRATION_RE.fullmatch(name) or
                not isinstance(checksum, str) or not SHA256_RE.fullmatch(checksum)):
            raise RestoreError('restore expectation migration value is invalid')
        # reject duplicate names
        if name in seen_names:
            raise RestoreError('restore expectation migration names must be unique')
        seen_names.add(name)
        normalized_migrations.append((name, checksum))
    # require query order
    if normalized_migrations != sorted(normalized_migrations):
        raise RestoreError('restore expectation migrations must be ordered by name')
    counts = payload['counts']
    require_exact_keys(counts, {'sites', 'sources', 'runs', 'records'}, 'restore expectation counts')
    # validate every representative count
    for name, value in counts.items():
        # reject booleans and negative counts
        if not isinstance(value, int) or isinstance(value, bool) or value < 0:
            raise RestoreError(f'restore expectation count is invalid: {name}')
    # require a representative hash
    if not isinstance(payload['sampleHash'], str) or not SAMPLE_HASH_RE.fullmatch(payload['sampleHash']):
        raise RestoreError('restore expectation sample hash is invalid')
    return payload, normalized_migrations, actual_file_sha256


# run one bounded docker command
def run_command(argv, timeout=120, input_bytes=None, check=True, redact_output=False):
    result = subprocess.run(
        argv,
        input=input_bytes,
        capture_output=True,
        timeout=timeout,
        check=False,
    )
    # fail without exposing command input
    if check and result.returncode != 0:
        # keep sensitive stdin and server diagnostics out of failure evidence
        if redact_output:
            raise RestoreError(
                f'command failed ({Path(argv[0]).name} {argv[1]}): sensitive output redacted'
            )
        detail = result.stderr.decode('utf-8', errors='replace').strip()[-4096:]
        raise RestoreError(f'command failed ({Path(argv[0]).name} {argv[1]}): {detail}')
    # prevent sensitive command output from escaping the bounded runner
    if redact_output:
        return subprocess.CompletedProcess(
            result.args,
            result.returncode,
            stdout=b'',
            stderr=b'',
        )
    return result


# parse one docker json record
def one_docker_record(result, description):
    try:
        payload = json.loads(result.stdout.decode('utf-8'))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise RestoreError(f'{description} returned invalid JSON') from error
    # require exactly one record
    if not isinstance(payload, list) or len(payload) != 1 or not isinstance(payload[0], dict):
        raise RestoreError(f'{description} did not identify exactly one object')
    return payload[0]


# verify the local immutable image
def inspect_image(image_reference, expected_image_id, command=run_command):
    # require a digest-only reference
    if not re.fullmatch(r'[^\s:@/]+(?:/[^\s:@/]+)*@sha256:[0-9a-f]{64}', image_reference):
        raise RestoreError('PostgreSQL image must be an exact sha256 repo digest')
    # require the reviewed local image id
    if not IMAGE_ID_RE.fullmatch(expected_image_id):
        raise RestoreError('expected PostgreSQL image id is invalid')
    result = command([DOCKER, 'image', 'inspect', image_reference], timeout=60)
    record = one_docker_record(result, 'PostgreSQL image inspection')
    repo_digests = record.get('RepoDigests')
    # bind id platform and reference
    if (record.get('Id') != expected_image_id or record.get('Os') != 'linux' or
            record.get('Architecture') != 'arm64' or not isinstance(repo_digests, list) or
            image_reference not in repo_digests):
        raise RestoreError('local PostgreSQL image identity or ARM64 platform differs')
    return record


# verify one exact fixture volume
def inspect_volume(marker, volume_name, command=run_command,
                   database_owner=(DATABASE_UID, DATABASE_GID), require_empty=True):
    prefix = marker['volumeNamespacePrefix']
    # derive the fixed slot names
    expected_names = {f'{prefix}{suffix}' for suffix in VOLUME_SUFFIXES}
    # restrict the caller-selected slot
    if volume_name not in expected_names:
        raise RestoreError('fixture volume name is outside the approved namespace')
    volume_device = Path(marker['volumeRoot']) / volume_name
    require_path(
        volume_device,
        'fixture volume device',
        'directory',
        owner=database_owner,
        mode=0o700,
    )
    require_canonical(volume_device, 'fixture volume device')
    # require the dedicated loop filesystem
    if os.stat(volume_device).st_dev != os.stat(marker['mountpoint']).st_dev:
        raise RestoreError('fixture volume device crosses the dedicated filesystem')
    # require one empty fresh target
    if require_empty and any(volume_device.iterdir()):
        raise RestoreError('fixture volume device must be empty before restore')
    result = command([DOCKER, 'volume', 'inspect', volume_name], timeout=60)
    record = one_docker_record(result, 'fixture volume inspection')
    expected_labels = {
        'weather.native-benchmark.fixture-id': marker['fixtureId'],
        'weather.native-benchmark.kind': VOLUME_KIND,
    }
    expected_options = {'type': 'none', 'o': 'bind', 'device': str(volume_device)}
    # bind exact driver options and labels
    if (record.get('Name') != volume_name or record.get('Driver') != 'local' or
            record.get('Scope') != 'local' or record.get('Options') != expected_options or
            record.get('Labels') != expected_labels):
        raise RestoreError('fixture volume identity differs')
    attached = command(
        [DOCKER, 'container', 'ls', '--all', '--quiet', '--filter', f'volume={volume_name}'],
        timeout=60,
    ).stdout.decode('utf-8', errors='strict').strip()
    # require a detached volume
    if attached:
        raise RestoreError('fixture volume is already attached to a container')
    return volume_device, record


# verify the complete sequential slot set
def inspect_volume_set(marker, target_volume, allow_target_data=False,
                       command=run_command,
                       target_owner=(DATABASE_UID, DATABASE_GID)):
    prefix = marker['volumeNamespacePrefix']
    # derive the fixed slot names
    expected_names = [f'{prefix}{suffix}' for suffix in VOLUME_SUFFIXES]
    # restrict the selected slot
    if target_volume not in expected_names:
        raise RestoreError('fixture target volume is outside the fixed five-slot set')
    listed = command([
        DOCKER, 'volume', 'ls', '--quiet',
        '--filter', f'label=weather.native-benchmark.fixture-id={marker["fixtureId"]}',
        '--filter', f'label=weather.native-benchmark.kind={VOLUME_KIND}',
    ], timeout=60).stdout.decode('utf-8', errors='strict').splitlines()
    # require exactly five distinct labeled volumes
    if len(listed) != len(expected_names) or set(listed) != set(expected_names):
        raise RestoreError('fixture must contain exactly the five fixed labeled slot volumes')
    selected_device = None
    # inspect every slot independently
    for volume_name in expected_names:
        is_target = volume_name == target_volume
        # bind ownership only on the selected database slot
        required_owner = target_owner if is_target else None
        device, _ = inspect_volume(
            marker,
            volume_name,
            command,
            required_owner,
            require_empty=not (is_target and allow_target_data),
        )
        # retain only the selected device
        if is_target:
            selected_device = device
    return selected_device


# generate independent fixture credentials
def create_credentials(fixture_root):
    # choose a new private directory
    for _ in range(32):
        directory = fixture_root / f'.restore-secrets-{secrets.token_hex(8)}'
        try:
            directory.mkdir(mode=0o700)
            break
        except FileExistsError:
            continue
    else:
        raise RestoreError('could not allocate a unique fixture credential directory')
    files = {}
    digests = {}
    try:
        # create every independent secret
        for name in SECRET_NAMES:
            path = directory / name
            value = f'{secrets.token_urlsafe(48)}\n'.encode('ascii')
            descriptor = os.open(
                path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o400,
            )
            files[name] = path
            try:
                offset = 0
                # complete every bounded write
                while offset < len(value):
                    written = os.write(descriptor, value[offset:])
                    # reject a stalled write
                    if written <= 0:
                        raise RestoreError('fixture credential write made no progress')
                    offset += written
                os.fsync(descriptor)
            finally:
                os.close(descriptor)
            os.chown(path, DATABASE_UID, DATABASE_GID, follow_symlinks=False)
            digests[name] = hashlib.sha256(value).hexdigest()
        directory_fd = os.open(directory, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    except BaseException:
        remove_partial_credentials(directory, files)
        raise
    # require distinct credentials
    if len(set(digests.values())) != len(SECRET_NAMES):
        remove_credentials(directory, files)
        raise RestoreError('generated fixture credentials are not distinct')
    # hash a value-free credential manifest
    manifest = ''.join(f'{name}|{digests[name]}\n' for name in sorted(digests))
    return directory, files, hashlib.sha256(manifest.encode('ascii')).hexdigest()


# remove only a partial generated credential set
def remove_partial_credentials(directory, files):
    require_path(directory, 'fixture credential directory', 'directory', owner=(0, 0), mode=0o700)
    expected = set(files.values())
    # reject unexpected deletion scope
    if set(directory.iterdir()) != expected:
        raise RestoreError('partial credential contents differ; refusing cleanup')
    # remove only known regular files
    for path in expected:
        require_path(path, 'partial credential file', 'file')
        path.unlink()
    directory.rmdir()


# remove only generated credential paths
def remove_credentials(directory, files):
    require_path(directory, 'fixture credential directory', 'directory', owner=(0, 0), mode=0o700)
    # derive the exact cleanup set
    expected = {directory / name for name in SECRET_NAMES}
    # reject unexpected deletion scope
    if set(files.values()) != expected or set(directory.iterdir()) != expected:
        raise RestoreError('fixture credential directory contents differ; refusing cleanup')
    # remove only proven secret files
    for path in expected:
        require_path(path, 'fixture credential file', 'file',
                     owner=(DATABASE_UID, DATABASE_GID), mode=0o400)
        path.unlink()
    directory.rmdir()


# read one generated credential without exposing it in arguments or output
def read_credential(path, description):
    metadata = require_path(
        path,
        f'{description} credential',
        'file',
        owner=(DATABASE_UID, DATABASE_GID),
        mode=0o400,
    )
    # reject aliased or unexpectedly large secret files
    if metadata.st_nlink != 1 or metadata.st_size < 2 or metadata.st_size > 1024:
        raise RestoreError(f'{description} credential file is invalid')
    value = path.read_bytes()
    # require one newline-terminated printable ascii secret
    if (not value.endswith(b'\n') or value.count(b'\n') != 1 or
            b'\r' in value or any(byte < 0x21 or byte > 0x7e for byte in value[:-1])):
        raise RestoreError(f'{description} credential value is invalid')
    return value[:-1].decode('ascii')


# quote one generated PostgreSQL string literal
def sql_literal(value):
    return "'" + value.replace("'", "''") + "'"


# derive one PostgreSQL SCRAM verifier without sending the plaintext password
def postgres_scram_verifier(password):
    iterations = 4096
    salt = secrets.token_bytes(16)
    salted_password = hashlib.pbkdf2_hmac(
        'sha256', password.encode('utf-8'), salt, iterations,
    )
    client_key = hmac.new(salted_password, b'Client Key', hashlib.sha256).digest()
    stored_key = hashlib.sha256(client_key).digest()
    server_key = hmac.new(salted_password, b'Server Key', hashlib.sha256).digest()

    # encode each verifier component without retaining plaintext in SQL
    def encode(value):
        return base64.b64encode(value).decode('ascii')

    return (
        f'SCRAM-SHA-256${iterations}:{encode(salt)}$'
        f'{encode(stored_key)}:{encode(server_key)}'
    )


# rotate only the fixed fixture roles to one new generated credential set
def rotate_fixture_roles(container_id, database_name, credentials, command=run_command):
    role_credentials = {
        'postgres': read_credential(
            credentials['weather_admin_password'], 'administrator',
        ),
        'weather_owner': read_credential(
            credentials['weather_owner_password'], 'owner',
        ),
        'weather_api': read_credential(credentials['weather_api_password'], 'api'),
        'weather_ingest': read_credential(
            credentials['weather_ingest_password'], 'ingest',
        ),
        'weather_training_export': read_credential(
            credentials['weather_training_export_password'], 'training-export',
        ),
    }
    # require every generated authority to remain independent
    if len(set(role_credentials.values())) != len(role_credentials):
        raise RestoreError('fixture role credentials must be distinct')
    # keep plaintext credentials out of PostgreSQL statements and server logs
    role_verifiers = {
        role_name: postgres_scram_verifier(password)
        for role_name, password in role_credentials.items()
    }
    statements = [
        'BEGIN;',
        "SET LOCAL log_statement = 'none';",
        "SET LOCAL log_min_error_statement = 'panic';",
        'SET LOCAL log_parameter_max_length_on_error = 0;',
    ]
    # change passwords only on the exact pre-existing fixture role names
    for role_name in (
            'postgres', 'weather_owner', 'weather_api', 'weather_ingest',
            'weather_training_export'):
        statements.append(
            f'ALTER ROLE {role_name} PASSWORD {sql_literal(role_verifiers[role_name])};'
        )
    statements.extend([
        'COMMIT;',
        '',
    ])
    sql = '\n'.join(statements).encode('utf-8')
    command([
        DOCKER, 'exec', '--interactive', container_id,
        'psql', '--quiet', '--no-password', '--no-psqlrc',
        '--host', '/var/run/postgresql', '--username', 'postgres',
        '--dbname', database_name, '--set', 'ON_ERROR_STOP=1',
    ], timeout=300, input_bytes=sql, redact_output=True)


# validate one mounted configuration file
def config_file(path, description, executable=False):
    require_path(path, description, 'file')
    require_canonical(path, description)
    # reject mutable shared configuration
    if path.stat().st_mode & 0o022:
        raise RestoreError(f'{description} must not be group or world writable')
    # require container-user execution access
    if executable and path.stat().st_mode & 0o005 != 0o005:
        raise RestoreError(f'{description} must be readable and executable by the fixture user')
    # reject executable bind sources on a noexec filesystem
    if executable and os.statvfs(path).f_flag & os.ST_NOEXEC:
        raise RestoreError(f'{description} must not reside on a noexec filesystem')
    return file_sha256(path)


# construct the isolated container command
def build_create_command(args, marker, volume_name, credentials):
    container_name = f'weather-native-restore-{marker["fixtureId"]}'
    command = [
        DOCKER, 'create', '--pull', 'never', '--name', container_name,
        '--label', f'weather.native-benchmark.fixture-id={marker["fixtureId"]}',
        '--label', f'weather.native-benchmark.kind={VOLUME_KIND}',
        '--label', f'weather.native-benchmark.role={CONTAINER_ROLE}',
        '--network', 'none', '--restart', 'no', '--read-only', '--cap-drop', 'ALL',
        '--security-opt', 'no-new-privileges', '--pids-limit', '128',
        '--memory', '512m', '--cpus', '1', '--user', '999:999',
        '--log-driver', 'json-file', '--log-opt', 'max-file=3', '--log-opt', 'max-size=10m',
        '--tmpfs', '/tmp:size=64m,mode=1777',
        '--tmpfs', '/var/run/postgresql:size=16m,mode=0755,uid=999,gid=999',
        '--mount', f'type=volume,src={volume_name},dst=/var/lib/postgresql/data,volume-nocopy',
        '--mount', f'type=bind,src={args.entrypoint},dst=/usr/local/bin/weather-postgres-entrypoint,readonly',
        '--mount', f'type=bind,src={args.roles_init},dst=/docker-entrypoint-initdb.d/010-create-runtime-roles.sh,readonly',
    ]
    # mount every fixture-only secret
    for name in SECRET_NAMES:
        command.extend([
            '--mount', f'type=bind,src={credentials[name]},dst=/run/secrets/{name},readonly',
        ])
    command.extend([
        '--env', f'POSTGRES_DB={args.database_name}',
        '--env', 'POSTGRES_USER=postgres',
        '--env', 'PGDATA=/var/lib/postgresql/data',
        '--env', 'POSTGRES_PASSWORD_FILE=/run/secrets/weather_admin_password',
        '--env', 'WEATHER_ADMIN_PASSWORD_FILE=/run/secrets/weather_admin_password',
        '--env', 'WEATHER_OWNER_PASSWORD_FILE=/run/secrets/weather_owner_password',
        '--env', 'WEATHER_API_PASSWORD_FILE=/run/secrets/weather_api_password',
        '--env', 'WEATHER_INGEST_PASSWORD_FILE=/run/secrets/weather_ingest_password',
        '--env', 'WEATHER_TRAINING_EXPORT_PASSWORD_FILE=/run/secrets/weather_training_export_password',
        '--entrypoint', '/usr/local/bin/weather-postgres-entrypoint',
        args.image, 'postgres', '-c', 'listen_addresses=', '-c',
        'unix_socket_directories=/var/run/postgresql',
    ])
    return container_name, command


# construct one existing-cluster verification container command
def build_resume_create_command(args, marker, volume_name, credentials):
    container_name, command = build_create_command(
        args, marker, volume_name, credentials,
    )
    filtered_command = []
    index = 0
    # omit recovery secrets because protected stdin performs role rotation
    while index < len(command):
        option = command[index]
        # remove only the fixed secret mounts and file-variable pairs
        if (option in ('--mount', '--env') and index + 1 < len(command) and
                (('dst=/run/secrets/' in command[index + 1]) or
                 ('PASSWORD_FILE=' in command[index + 1]))):
            index += 2
            continue
        filtered_command.append(option)
        index += 1
    command = filtered_command
    entrypoint_index = command.index('--entrypoint') + 1
    # bypass first-boot and retained-cluster reconciliation during verification resume
    command[entrypoint_index] = '/usr/local/bin/docker-entrypoint.sh'
    return container_name, command


# bind one retained restore-verification failure to the reviewed recovery
def validate_resume_failure(path, expected_sha256, marker, volume_name,
                            expectation, approval, now=None, owner=(0, 0)):
    failure, actual_sha256 = read_private_json(path, 'restore failure evidence', owner)
    # bind the exact operator-reviewed failure bytes
    if not SHA256_RE.fullmatch(expected_sha256) or actual_sha256 != expected_sha256:
        raise RestoreError('restore failure evidence SHA-256 differs')
    require_exact_keys(
        failure,
        {'schemaVersion', 'kind', 'fixtureId', 'volumeName', 'stage', 'error',
         'cleanupErrors', 'containerLogTail', 'observedArchiveSha256', 'failedAt'},
        'restore failure evidence',
    )
    expected_count_query = (
        'SELECT (SELECT count(*) FROM sites),(SELECT count(*) FROM sources),'
        '(SELECT count(*) FROM ingestion_runs),(SELECT count(*) FROM weather_records)'
    )
    error = failure.get('error')
    log_tail = failure.get('containerLogTail')
    # admit only the exact completed-stream verification timeout boundary
    if (failure.get('schemaVersion') != 1 or failure.get('kind') != FAILURE_KIND or
            failure.get('fixtureId') != marker['fixtureId'] or
            failure.get('volumeName') != volume_name or
            failure.get('stage') != 'restore-verification' or
            failure.get('cleanupErrors') != [] or
            failure.get('observedArchiveSha256') != expectation['archiveSha256'] or
            not isinstance(error, str) or expected_count_query not in error or
            not error.endswith('timed out after 120 seconds') or
            not isinstance(log_tail, (str, type(None))) or
            (isinstance(log_tail, str) and len(log_tail.encode('utf-8')) > 32768)):
        raise RestoreError('restore failure evidence is not resumable')
    failed_at = parse_timestamp(failure.get('failedAt'), 'failedAt')
    captured_now = now or utc_now()
    approved_at = parse_timestamp(approval['approvedAt'], 'approvedAt')
    expires_at = parse_timestamp(approval['expiresAt'], 'expiresAt')
    # require a past failure inside the still-active approved window
    if (failed_at < approved_at or failed_at > captured_now or
            failed_at >= expires_at or captured_now >= expires_at):
        raise RestoreError('restore failure evidence is outside the active approval')
    return failure, actual_sha256


# validate the stopped populated PostgreSQL tree used by resume verification
def validate_resume_pgdata(volume_device, database_owner=(DATABASE_UID, DATABASE_GID)):
    pg_version = volume_device / 'PG_VERSION'
    pg_control = volume_device / 'global' / 'pg_control'
    require_path(
        pg_version, 'restored PG_VERSION', 'file',
        owner=database_owner,
    )
    require_path(
        pg_control, 'restored pg_control', 'file',
        owner=database_owner,
    )
    require_canonical(pg_version, 'restored PG_VERSION')
    require_canonical(pg_control, 'restored pg_control')
    # reject a running or incompletely stopped cluster
    postmaster = volume_device / 'postmaster.pid'
    if postmaster.exists() or postmaster.is_symlink():
        raise RestoreError('restored fixture retains postmaster.pid')
    version = pg_version.read_text(encoding='ascii').strip()
    # bind the restored cluster to the reviewed PostgreSQL major version
    if version != '17' or pg_control.stat().st_size < 1:
        raise RestoreError('restored PostgreSQL control files are invalid')


# validate one owned restore container
def inspect_owned_container(container_id, container_name, fixture_id, expected_image_id,
                            command=run_command):
    # require a full immutable id
    if not re.fullmatch(r'[0-9a-f]{64}', container_id):
        raise RestoreError('created container id is invalid')
    result = command([DOCKER, 'container', 'inspect', container_id], timeout=60)
    record = one_docker_record(result, 'restore container inspection')
    labels = record.get('Config', {}).get('Labels')
    expected_labels = {
        'weather.native-benchmark.fixture-id': fixture_id,
        'weather.native-benchmark.kind': VOLUME_KIND,
        'weather.native-benchmark.role': CONTAINER_ROLE,
    }
    expected_log_config = {
        'Type': 'json-file',
        'Config': {'max-file': '3', 'max-size': '10m'},
    }
    # prove ownership before control
    if (record.get('Id') != container_id or record.get('Name') != f'/{container_name}' or
            record.get('Image') != expected_image_id or labels != expected_labels or
            record.get('HostConfig', {}).get('LogConfig') != expected_log_config):
        raise RestoreError('restore container ownership proof differs')
    return record


# wait for the isolated socket server
def wait_ready(container_id, database_name, timeout_seconds, command=run_command,
               monotonic=time.monotonic, sleeper=time.sleep):
    deadline = monotonic() + timeout_seconds
    # retry one local socket probe
    while monotonic() < deadline:
        result = command([
            DOCKER, 'exec', container_id, 'sh', '-c',
            'test "$(cat /proc/1/comm)" = postgres && exec pg_isready "$@"',
            'fixture-readiness', '--host', '/var/run/postgresql',
            '--username', 'postgres', '--dbname', database_name,
        ], timeout=10, check=False)
        # accept only the documented ready code
        if result.returncode == 0:
            return
        sleeper(0.5)
    raise RestoreError('isolated PostgreSQL fixture did not become ready')


# terminate one owned restore client
def terminate_restore_client(process):
    # kill only the owned docker exec client
    if process.poll() is None:
        process.kill()
    try:
        return process.wait(timeout=10)
    except subprocess.TimeoutExpired as error:
        raise RestoreError('pg_restore client did not terminate after kill') from error


# stream one plaintext archive directly to pg_restore
def stream_restore(container_id, database_name, source, expected_sha256, timeout_seconds,
                   popen_factory=subprocess.Popen, monotonic=time.monotonic):
    process = popen_factory([
        DOCKER, 'exec', '--interactive', container_id,
        'pg_restore', '--username', 'postgres', '--dbname', database_name,
        '--no-owner', '--role', 'weather_owner', '--exit-on-error',
    ], stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    # require real interruptible pipes
    if process.stdin is None or process.stderr is None:
        terminate_restore_client(process)
        raise RestoreError('pg_restore pipes are unavailable')
    try:
        source_fd = source.fileno()
    except (AttributeError, io.UnsupportedOperation, OSError) as error:
        terminate_restore_client(process)
        raise RestoreError('plaintext archive input must be an interruptible pipe') from error
    source_mode = os.fstat(source_fd).st_mode
    # prohibit plaintext regular-file input
    if not stat.S_ISFIFO(source_mode) and not stat.S_ISSOCK(source_mode):
        terminate_restore_client(process)
        raise RestoreError('plaintext archive input must be a pipe or socket')
    sink_fd = process.stdin.fileno()
    stderr_fd = process.stderr.fileno()
    source_blocking = os.get_blocking(source_fd)
    os.set_blocking(source_fd, False)
    os.set_blocking(sink_fd, False)
    os.set_blocking(stderr_fd, False)
    deadline = monotonic() + timeout_seconds
    stderr_tail = bytearray()
    digest = hashlib.sha256()
    pending = bytearray()
    source_eof = False
    sink_registered = False
    source_registered = True
    stderr_eof = False
    input_failed = None
    selector = selectors.DefaultSelector()
    selector.register(source_fd, selectors.EVENT_READ, 'source')
    selector.register(stderr_fd, selectors.EVENT_READ, 'stderr')
    try:
        # multiplex the complete bounded stream lifecycle
        while True:
            remaining = deadline - monotonic()
            # enforce one total deadline
            if remaining <= 0:
                raise subprocess.TimeoutExpired(getattr(process, 'args', 'pg_restore'), timeout_seconds)
            return_code = process.poll()
            input_complete = source_eof and not pending
            # close input exactly after the complete stream
            if input_complete and process.stdin is not None:
                # remove a pending write watch
                if sink_registered:
                    selector.unregister(sink_fd)
                    sink_registered = False
                process.stdin.close()
                process.stdin = None
            # stop reading after early child exit
            if return_code is not None and not input_complete and input_failed is None:
                input_failed = RestoreError('pg_restore closed the archive stream early')
                # remove the source watch
                if source_registered:
                    selector.unregister(source_fd)
                    source_registered = False
                # remove the sink watch
                if sink_registered:
                    selector.unregister(sink_fd)
                    sink_registered = False
                pending.clear()
                # close only the owned child input pipe
                if process.stdin is not None:
                    process.stdin.close()
                    process.stdin = None
            # finish after exit and stderr eof
            if return_code is not None and stderr_eof:
                break
            events = selector.select(min(remaining, 0.5))
            # process every ready descriptor
            for key, _ in events:
                # read bounded source data
                if key.data == 'source':
                    try:
                        chunk = os.read(source_fd, 1024 * 1024 - len(pending))
                    except BlockingIOError:
                        continue
                    # close the source watch at eof
                    if not chunk:
                        source_eof = True
                        selector.unregister(source_fd)
                        source_registered = False
                        continue
                    digest.update(chunk)
                    pending.extend(chunk)
                    # pause source reads at the memory bound
                    if len(pending) >= 1024 * 1024:
                        selector.unregister(source_fd)
                        source_registered = False
                    # enable child writes only with data
                    if not sink_registered:
                        selector.register(sink_fd, selectors.EVENT_WRITE, 'sink')
                        sink_registered = True
                    continue
                # write bounded child input
                if key.data == 'sink':
                    try:
                        written = os.write(sink_fd, pending)
                    except BlockingIOError:
                        continue
                    except (BrokenPipeError, OSError) as error:
                        input_failed = error
                        selector.unregister(sink_fd)
                        sink_registered = False
                        # stop consuming incomplete plaintext
                        if source_registered:
                            selector.unregister(source_fd)
                            source_registered = False
                        pending.clear()
                        # close only the owned child input pipe
                        if process.stdin is not None:
                            process.stdin.close()
                            process.stdin = None
                        continue
                    # reject a stalled writable pipe
                    if written <= 0:
                        input_failed = RestoreError('pg_restore input write made no progress')
                        selector.unregister(sink_fd)
                        sink_registered = False
                        # close only the owned child input pipe
                        if process.stdin is not None:
                            process.stdin.close()
                            process.stdin = None
                        continue
                    del pending[:written]
                    # disable writes until more data exists
                    if not pending:
                        selector.unregister(sink_fd)
                        sink_registered = False
                    # resume bounded source reads
                    if (not source_eof and not source_registered and
                            len(pending) < 1024 * 1024 and input_failed is None):
                        selector.register(source_fd, selectors.EVENT_READ, 'source')
                        source_registered = True
                    continue
                # drain one bounded stderr chunk
                try:
                    error_chunk = os.read(stderr_fd, 8192)
                except BlockingIOError:
                    continue
                # close the stderr watch at eof
                if not error_chunk:
                    selector.unregister(stderr_fd)
                    stderr_eof = True
                    continue
                stderr_tail.extend(error_chunk)
                # retain only the newest stderr bytes
                if len(stderr_tail) > 65536:
                    del stderr_tail[:-65536]
        return_code = process.wait(timeout=max(0.0, deadline - monotonic()))
    except subprocess.TimeoutExpired as error:
        try:
            terminate_restore_client(process)
        except RestoreError as wait_error:
            raise RestoreError('pg_restore did not terminate after its total timeout') from wait_error
        raise RestoreError('pg_restore exceeded its total streaming timeout') from error
    except BaseException:
        terminate_restore_client(process)
        raise
    finally:
        selector.close()
        os.set_blocking(source_fd, source_blocking)
        # close any remaining child input
        if process.stdin:
            process.stdin.close()
        # close the drained child stderr pipe
        if process.stderr:
            process.stderr.close()
    detail = bytes(stderr_tail).decode('utf-8', errors='replace').strip()[-4096:]
    # surface an incomplete input stream
    if input_failed is not None:
        raise RestoreError(f'pg_restore closed the archive stream early: {detail}') from input_failed
    actual_sha256 = digest.hexdigest()
    # verify the complete source bytes
    if actual_sha256 != expected_sha256:
        raise RestoreError(f'plaintext archive SHA-256 differs: observed={actual_sha256}')
    # require complete restore success
    if return_code != 0:
        raise RestoreError(f'pg_restore failed: {detail}')
    return actual_sha256


# run one fixed psql query
def psql(container_id, database_name, sql, command=run_command, timeout=120):
    result = command([
        DOCKER, 'exec', container_id, 'psql', '--no-password', '--no-psqlrc',
        '--host', '/var/run/postgresql', '--username', 'postgres', '--dbname', database_name,
        '--set', 'ON_ERROR_STOP=1', '--tuples-only', '--no-align',
        '--field-separator=|', '--command', sql,
    ], timeout=timeout)
    return result.stdout.decode('utf-8', errors='strict').strip()


# apply the deployed runtime acl
def apply_runtime_acl(container_id, database_name, runtime_acl, expected_sha256,
                      command=run_command):
    acl_bytes = runtime_acl.read_bytes()
    # bind the exact reviewed acl bytes
    if hashlib.sha256(acl_bytes).hexdigest() != expected_sha256:
        raise RestoreError('PostgreSQL runtime ACL changed before application')
    command([
        DOCKER, 'exec', '--interactive', container_id,
        'psql', '--no-password', '--no-psqlrc',
        '--host', '/var/run/postgresql', '--username', 'postgres', '--dbname', database_name,
        '--set', 'ON_ERROR_STOP=1',
    ], timeout=300, input_bytes=acl_bytes)


# verify the fixed deployed runtime ACL with actionable predicate names
def verify_runtime_acl(container_id, database_name, command=run_command):
    failed_checks = psql(
        container_id,
        database_name,
        "WITH acl_checks(name,passed) AS (VALUES "
        "('api_database_connect',has_database_privilege('weather_api',current_database(),'CONNECT')),"
        "('api_database_create_absent',NOT has_database_privilege('weather_api',current_database(),'CREATE')),"
        "('api_database_temp_absent',NOT has_database_privilege('weather_api',current_database(),'TEMP')),"
        "('api_public_usage',has_schema_privilege('weather_api','public','USAGE')),"
        "('api_public_create_absent',NOT has_schema_privilege('weather_api','public','CREATE')),"
        "('api_sites_select',has_table_privilege('weather_api','sites','SELECT')),"
        "('api_sites_write_absent',NOT has_table_privilege('weather_api','sites','INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')),"
        "('api_sources_key_select',has_column_privilege('weather_api','sources','source_key','SELECT')),"
        "('api_sources_provider_config_absent',NOT has_column_privilege('weather_api','sources','material_provider_config','SELECT')),"
        "('ingest_database_connect',has_database_privilege('weather_ingest',current_database(),'CONNECT')),"
        "('ingest_database_create_absent',NOT has_database_privilege('weather_ingest',current_database(),'CREATE')),"
        "('ingest_database_temp_absent',NOT has_database_privilege('weather_ingest',current_database(),'TEMP')),"
        "('ingest_public_usage',has_schema_privilege('weather_ingest','public','USAGE')),"
        "('ingest_public_create_absent',NOT has_schema_privilege('weather_ingest','public','CREATE')),"
        "('ingest_weather_records_table_select_absent',NOT has_table_privilege('weather_ingest','weather_records','SELECT')),"
        "('ingest_weather_records_table_insert_absent',NOT has_table_privilege('weather_ingest','weather_records','INSERT')),"
        "('ingest_weather_records_source_id_select',has_column_privilege('weather_ingest','weather_records','source_id','SELECT')),"
        "('ingest_weather_records_source_id_insert',has_column_privilege('weather_ingest','weather_records','source_id','INSERT')),"
        "('ingest_weather_records_receipt_select_absent',NOT has_column_privilege('weather_ingest','weather_records','adjustment_revision_receipt','SELECT')),"
        "('ingest_weather_records_receipt_insert_absent',NOT has_column_privilege('weather_ingest','weather_records','adjustment_revision_receipt','INSERT')),"
        "('ingest_weather_records_other_write_absent',NOT has_table_privilege('weather_ingest','weather_records','UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')),"
        "('ingest_weather_records_sequence_usage',has_sequence_privilege('weather_ingest','weather_records_id_seq','USAGE')),"
        "('training_database_connect',has_database_privilege('weather_training_export',current_database(),'CONNECT')),"
        "('training_database_create_absent',NOT has_database_privilege('weather_training_export',current_database(),'CREATE')),"
        "('training_database_temp_absent',NOT has_database_privilege('weather_training_export',current_database(),'TEMP')),"
        "('training_public_usage',has_schema_privilege('weather_training_export','public','USAGE')),"
        "('training_public_create_absent',NOT has_schema_privilege('weather_training_export','public','CREATE')),"
        "('training_rows_select',has_table_privilege('weather_training_export','forecast_training_export_rows_v1','SELECT')),"
        "('training_manifest_select',has_table_privilege('weather_training_export','forecast_training_export_manifest_v1','SELECT')),"
        "('runtime_role_memberships_absent',NOT EXISTS (SELECT 1 FROM pg_auth_members WHERE member IN ('weather_owner'::regrole,'weather_api'::regrole,'weather_ingest'::regrole,'weather_training_export'::regrole))),"
        "('training_database_settings_absent',NOT EXISTS (SELECT 1 FROM pg_db_role_setting WHERE setrole='weather_training_export'::regrole AND setdatabase<>0)),"
        "('runtime_schema_create_absent',NOT EXISTS (SELECT 1 FROM pg_namespace namespace CROSS JOIN LATERAL unnest(ARRAY['weather_api','weather_ingest','weather_training_export']) consumer(name) WHERE namespace.nspname<>'information_schema' AND namespace.nspname NOT LIKE 'pg_%' AND has_schema_privilege(consumer.name,namespace.oid,'CREATE')))"
        ") SELECT name FROM acl_checks WHERE NOT passed ORDER BY name",
        command,
    ).splitlines()
    # surface only fixed predicate identifiers rather than one opaque boolean
    if failed_checks:
        if any(not re.fullmatch(r'[a-z0-9_]{1,96}', name) for name in failed_checks):
            raise RestoreError('restored runtime ACL verification returned invalid diagnostics')
        raise RestoreError(
            f'restored runtime ACL verification failed: {",".join(failed_checks)}'
        )


# verify the restored ledger roles and data
def verify_restore(container_id, expectation, expected_migrations, command=run_command,
                   count_timeout_seconds=VERIFICATION_COUNT_TIMEOUT_SECONDS):
    # bound the only full-table verification query independently
    if (not isinstance(count_timeout_seconds, int) or
            isinstance(count_timeout_seconds, bool) or
            not 120 <= count_timeout_seconds <= VERIFICATION_COUNT_TIMEOUT_SECONDS):
        raise RestoreError('restore count verification timeout is invalid')
    database_name = expectation['databaseName']
    server_version = psql(container_id, database_name, 'SHOW server_version_num', command)
    # require supported postgres
    if not re.fullmatch(r'[0-9]+', server_version) or int(server_version) < 150000:
        raise RestoreError('restored fixture requires PostgreSQL 15 or newer')
    ledger_output = psql(
        container_id,
        database_name,
        'SELECT name, checksum FROM schema_migrations ORDER BY name',
        command,
    )
    actual_migrations = []
    # parse every exact ledger row
    for line in ledger_output.splitlines():
        parts = line.split('|')
        # reject ambiguous query output
        if len(parts) != 2:
            raise RestoreError('restored migration ledger output is invalid')
        actual_migrations.append((parts[0], parts[1]))
    # bind the exact immutable ledger
    if actual_migrations != expected_migrations:
        raise RestoreError('restored migration ledger differs')
    unsafe_roles = psql(
        container_id,
        database_name,
        "SELECT rolname FROM pg_roles WHERE rolname IN "
        "('weather_owner','weather_api','weather_ingest','weather_training_export') "
        'AND (rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls) '
        'ORDER BY rolname',
        command,
    )
    # reject administrative fixture roles
    if unsafe_roles:
        raise RestoreError('restored runtime roles gained administrative authority')
    role_state = psql(
        container_id,
        database_name,
        "SELECT rolname,rolcanlogin::int,rolinherit::int,rolsuper::int,"
        "rolcreatedb::int,rolcreaterole::int,rolreplication::int,rolbypassrls::int,"
        "COALESCE(array_to_string(rolconfig,','),'') FROM pg_roles WHERE rolname IN "
        "('weather_owner','weather_api','weather_ingest','weather_training_export') "
        'ORDER BY rolname',
        command,
    ).splitlines()
    expected_role_state = [
        'weather_api|1|0|0|0|0|0|0|',
        'weather_ingest|1|0|0|0|0|0|0|',
        'weather_owner|1|1|0|0|0|0|0|',
        'weather_training_export|1|0|0|0|0|0|0|default_transaction_read_only=on',
    ]
    # require every deployed role property and global setting exactly
    if role_state != expected_role_state:
        raise RestoreError('restored runtime role state differs')
    verify_runtime_acl(container_id, database_name, command)
    counts_output = psql(
        container_id,
        database_name,
        "SELECT (SELECT count(*) FROM sites),(SELECT count(*) FROM sources),"
        "(SELECT count(*) FROM ingestion_runs),(SELECT count(*) FROM weather_records)",
        command,
        timeout=count_timeout_seconds,
    )
    parts = counts_output.split('|')
    # require four integer counts
    if len(parts) != 4 or any(not re.fullmatch(r'[0-9]+', part) for part in parts):
        raise RestoreError('restored representative counts are invalid')
    counts = dict(zip(('sites', 'sources', 'runs', 'records'), map(int, parts)))
    # bind the exact expected counts
    if counts != expectation['counts']:
        raise RestoreError('restored representative counts differ')
    sample_hash = psql(
        container_id,
        database_name,
        "SELECT md5(COALESCE(string_agg(id::text || ':' || content_hash, ',' ORDER BY id),'empty')) "
        'FROM (SELECT id, content_hash FROM weather_records ORDER BY id LIMIT 10) sample',
        command,
    )
    # bind the exact representative rows
    if sample_hash != expectation['sampleHash']:
        raise RestoreError('restored representative sample hash differs')
    # hash the verified ledger
    canonical_ledger = ''.join(f'{name}|{checksum}\n' for name, checksum in actual_migrations)
    return {
        'serverVersionNum': int(server_version),
        'actualLedgerSha256': hashlib.sha256(canonical_ledger.encode('utf-8')).hexdigest(),
        'counts': counts,
        'sampleHash': sample_hash,
    }


# recheck every immutable fixture authority artifact
def require_unchanged_authority(fixture_root, expectation_path, marker_sha256,
                                approval_sha256, admission_sha256,
                                expectation_sha256, approval):
    _, current_marker_sha256 = read_private_json(
        fixture_root / MARKER_NAME, 'fixture marker',
    )
    _, current_approval_sha256 = read_private_json(
        fixture_root / APPROVAL_NAME, 'approved storage cap',
    )
    _, current_admission_sha256 = read_private_json(
        fixture_root / ADMISSION_NAME, 'storage admission receipt',
    )
    _, current_expectation_sha256 = read_private_json(
        expectation_path, 'restore expectation',
    )
    # reject mutable authority or an expired fixture
    if (current_marker_sha256 != marker_sha256 or
            current_approval_sha256 != approval_sha256 or
            current_admission_sha256 != admission_sha256 or
            current_expectation_sha256 != expectation_sha256 or
            utc_now() >= parse_timestamp(approval['expiresAt'], 'expiresAt')):
        raise RestoreError('fixture authority changed or expired during restore')


# construct the fixed successful restore handoff
def build_handoff(args, marker, volume_device, expectation, observed_archive_sha256,
                  admission, admission_sha256, approval, marker_sha256,
                  expectation_sha256, expected_ledger_sha256, verification,
                  config_hashes, credential_directory, credential_manifest_sha256):
    return {
        'schemaVersion': 1,
        'kind': HANDOFF_KIND,
        'fixtureId': marker['fixtureId'],
        'fixtureRoot': marker['fixtureRoot'],
        'volumeName': args.target_volume,
        'volumeDevice': str(volume_device),
        'databaseName': expectation['databaseName'],
        'imageId': args.expected_image_id,
        'imageRepoDigest': args.image,
        'archiveSha256': observed_archive_sha256,
        'sourceArchiveCiphertextSha256': admission['sourceArchiveSha256'],
        'markerSha256': marker_sha256,
        'admissionReceiptSha256': admission_sha256,
        'approvedCapSha256': admission['approvedCapSha256'],
        'approvedCapExpiresAt': approval['expiresAt'],
        'expectationSha256': expectation_sha256,
        'expectedLedgerSha256': expected_ledger_sha256,
        'actualLedgerSha256': verification['actualLedgerSha256'],
        'counts': verification['counts'],
        'sampleHash': verification['sampleHash'],
        'serverVersionNum': verification['serverVersionNum'],
        'configSha256': config_hashes,
        'credentialDirectory': str(credential_directory),
        'credentialManifestSha256': credential_manifest_sha256,
        'completedAt': utc_now().strftime('%Y-%m-%dT%H:%M:%SZ'),
    }


# write one atomic private json artifact
def write_private_json(path, payload, replace=False, authority_owner=(0, 0)):
    parent = path.parent
    require_path(parent, 'artifact parent', 'directory', owner=authority_owner, mode=0o700)
    temporary = parent / f'.{path.name}.tmp-{os.getpid()}-{secrets.token_hex(4)}'
    data = (json.dumps(payload, indent=2, sort_keys=True) + '\n').encode('utf-8')
    descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        offset = 0
        # complete every durable artifact write
        while offset < len(data):
            written = os.write(descriptor, data[offset:])
            # reject a stalled write
            if written <= 0:
                raise RestoreError('private artifact write made no progress')
            offset += written
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    try:
        # replace only an explicitly replaceable diagnostic artifact
        if replace:
            os.replace(temporary, path)
        else:
            os.link(temporary, path, follow_symlinks=False)
            temporary.unlink()
        directory_fd = os.open(parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    except BaseException:
        # remove only the known temporary file
        if temporary.exists():
            temporary.unlink()
        raise


# prove ownership before stopping and removing
def cleanup_container(container_id, container_name, fixture_id, image_id, command=run_command):
    inspect_owned_container(container_id, container_name, fixture_id, image_id, command)
    command([DOCKER, 'stop', '--time', '60', container_id], timeout=90)
    stopped = inspect_owned_container(container_id, container_name, fixture_id, image_id, command)
    # require a stopped fixture
    if stopped.get('State', {}).get('Running') is not False:
        raise RestoreError('restore container did not stop')
    command([DOCKER, 'rm', container_id], timeout=60)


# collect one bounded fixture log tail
def container_log_tail(container_id, container_name, fixture_id, image_id, command=run_command):
    inspect_owned_container(container_id, container_name, fixture_id, image_id, command)
    result = command([DOCKER, 'logs', '--tail', '200', container_id], timeout=30, check=False)
    return result.stderr.decode('utf-8', errors='replace')[-32768:]


# resume only the verification phase of one completed archive restore
def resume_verification(args):
    fixture_root = Path(args.fixture_root)
    marker, _, marker_sha256 = validate_marker(fixture_root)
    admission, _, admission_sha256, approval = validate_admission(
        marker,
        marker_sha256,
        fixture_root,
        args.max_admission_age_seconds,
        restored_fixture=True,
    )
    handoff_path = fixture_root / HANDOFF_NAME
    failure_path = fixture_root / FAILURE_NAME
    # require the original failure and no successful handoff
    if (handoff_path.exists() or handoff_path.is_symlink() or
            not failure_path.exists() or failure_path.is_symlink()):
        raise RestoreError('verification resume requires only the original failure evidence')
    expectation_path = Path(args.expectation)
    expectation, expected_migrations, expectation_sha256 = validate_expectation(
        expectation_path, args.expectation_sha256,
    )
    failure, failure_sha256 = validate_resume_failure(
        failure_path,
        args.resume_verification_failure_sha256,
        marker,
        args.target_volume,
        expectation,
        approval,
    )
    expected_ledger = ''.join(
        f'{name}|{checksum}\n' for name, checksum in expected_migrations
    )
    expected_ledger_sha256 = hashlib.sha256(expected_ledger.encode('utf-8')).hexdigest()
    args.database_name = expectation['databaseName']
    entrypoint = Path(args.entrypoint)
    roles_init = Path(args.roles_init)
    runtime_acl = Path(args.runtime_acl)
    args.entrypoint = str(entrypoint)
    args.roles_init = str(roles_init)
    args.runtime_acl = str(runtime_acl)
    config_hashes = {
        'entrypointSha256': config_file(
            entrypoint, 'PostgreSQL entrypoint', executable=True,
        ),
        'rolesInitSha256': config_file(
            roles_init, 'PostgreSQL roles initializer', executable=True,
        ),
        'runtimeAclSha256': config_file(runtime_acl, 'PostgreSQL runtime ACL'),
    }
    inspect_image(args.image, args.expected_image_id)
    volume_device = inspect_volume_set(
        marker,
        args.target_volume,
        allow_target_data=True,
    )
    validate_resume_pgdata(volume_device)
    credential_directory = None
    credentials = None
    credential_manifest_sha256 = None
    container_id = None
    container_name = None
    cleanup_errors = []
    stage = 'resume-credentials'
    try:
        credential_directory, credentials, credential_manifest_sha256 = create_credentials(
            fixture_root,
        )
        container_name, create_command = build_resume_create_command(
            args, marker, args.target_volume, credentials,
        )
        stage = 'resume-container-create'
        created = run_command(create_command, timeout=120)
        container_id = created.stdout.decode('ascii', errors='strict').strip()
        inspect_owned_container(
            container_id, container_name, marker['fixtureId'], args.expected_image_id,
        )
        stage = 'resume-container-start'
        run_command([DOCKER, 'start', container_id], timeout=120)
        stage = 'resume-postgres-ready'
        wait_ready(container_id, expectation['databaseName'], args.ready_timeout_seconds)
        stage = 'resume-role-rotation'
        rotate_fixture_roles(
            container_id,
            expectation['databaseName'],
            credentials,
        )
        # the exact failure proves the full ACL application completed before its count timeout
        stage = 'resume-verification'
        verification = verify_restore(
            container_id,
            expectation,
            expected_migrations,
            count_timeout_seconds=VERIFICATION_COUNT_TIMEOUT_SECONDS,
        )
        require_unchanged_authority(
            fixture_root,
            expectation_path,
            marker_sha256,
            admission['approvedCapSha256'],
            admission_sha256,
            expectation_sha256,
            approval,
        )
        _, current_failure_sha256 = read_private_json(
            failure_path, 'restore failure evidence',
        )
        # preserve the exact original recovery authority
        if current_failure_sha256 != failure_sha256:
            raise RestoreError('restore failure evidence changed during verification resume')
        # recheck every mounted configuration byte
        if config_hashes != {
                'entrypointSha256': config_file(
                    entrypoint, 'PostgreSQL entrypoint', executable=True,
                ),
                'rolesInitSha256': config_file(
                    roles_init, 'PostgreSQL roles initializer', executable=True,
                ),
                'runtimeAclSha256': config_file(runtime_acl, 'PostgreSQL runtime ACL'),
        }:
            raise RestoreError('mounted PostgreSQL configuration changed during verification resume')
        stage = 'resume-container-cleanup'
        cleanup_container(
            container_id, container_name, marker['fixtureId'], args.expected_image_id,
        )
        container_id = None
        inspect_volume_set(marker, args.target_volume, allow_target_data=True)
        validate_resume_pgdata(volume_device)
        stage = 'resume-handoff'
        handoff = build_handoff(
            args,
            marker,
            volume_device,
            expectation,
            failure['observedArchiveSha256'],
            admission,
            admission_sha256,
            approval,
            marker_sha256,
            expectation_sha256,
            expected_ledger_sha256,
            verification,
            config_hashes,
            credential_directory,
            credential_manifest_sha256,
        )
        write_private_json(handoff_path, handoff)
        try:
            print(json.dumps(handoff, indent=2, sort_keys=True), flush=True)
        except BrokenPipeError:
            # keep the durable verified handoff authoritative
            return 0
        return 0
    except BaseException as error:
        # clean only the exact owned recovery container
        if container_id and container_name:
            try:
                cleanup_container(
                    container_id, container_name, marker['fixtureId'], args.expected_image_id,
                )
                container_id = None
            except CLEANUP_EXCEPTIONS as cleanup_error:
                cleanup_errors.append(f'container cleanup: {cleanup_error}')
        # remove only an incomplete generated recovery credential set
        if credential_directory and credentials:
            try:
                remove_credentials(credential_directory, credentials)
                credential_directory = None
            except CLEANUP_EXCEPTIONS as cleanup_error:
                cleanup_errors.append(f'credential cleanup: {cleanup_error}')
        raise RestoreError(
            f'{stage} failed: {error}; originalFailureEvidence={failure_path}; '
            f'cleanupErrors={cleanup_errors}'
        ) from error


# parse restore arguments
def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture-root', required=True)
    parser.add_argument('--target-volume', required=True)
    parser.add_argument('--image', required=True)
    parser.add_argument('--expected-image-id', required=True)
    parser.add_argument('--expectation', required=True)
    parser.add_argument('--expectation-sha256', required=True)
    parser.add_argument('--entrypoint', required=True)
    parser.add_argument('--roles-init', required=True)
    parser.add_argument('--runtime-acl', required=True)
    parser.add_argument('--restore-timeout-seconds', type=int, default=3600)
    parser.add_argument('--ready-timeout-seconds', type=int, default=120)
    parser.add_argument('--max-admission-age-seconds', type=int, default=300)
    parser.add_argument('--resume-verification-failure-sha256')
    return parser.parse_args(argv)


# run the isolated restore workflow
def main(argv=None):
    args = parse_args(argv)
    # require root-owned lifecycle operations
    if os.geteuid() != 0:
        raise RestoreError('root is required for fixture ownership validation')
    # bound operator-selected timeouts
    if (not 60 <= args.restore_timeout_seconds <= 7200 or
            not 10 <= args.ready_timeout_seconds <= 600 or
            not 60 <= args.max_admission_age_seconds <= 900):
        raise RestoreError('restore or readiness timeout is outside the supported bounds')
    # route explicit recovery without consuming archive stdin
    if args.resume_verification_failure_sha256 is not None:
        return resume_verification(args)
    fixture_root = Path(args.fixture_root)
    marker, _, marker_sha256 = validate_marker(fixture_root)
    admission, _, admission_sha256, approval = validate_admission(
        marker,
        marker_sha256,
        fixture_root,
        args.max_admission_age_seconds,
    )
    handoff_path = fixture_root / HANDOFF_NAME
    failure_path = fixture_root / FAILURE_NAME
    # refuse to overwrite evidence
    if (handoff_path.exists() or handoff_path.is_symlink() or
            failure_path.exists() or failure_path.is_symlink()):
        raise RestoreError('restore handoff or failure evidence already exists')
    expectation, expected_migrations, expectation_sha256 = validate_expectation(
        Path(args.expectation), args.expectation_sha256,
    )
    # hash the immutable expected ledger
    expected_ledger = ''.join(
        f'{name}|{checksum}\n' for name, checksum in expected_migrations
    )
    expected_ledger_sha256 = hashlib.sha256(expected_ledger.encode('utf-8')).hexdigest()
    args.database_name = expectation['databaseName']
    entrypoint = Path(args.entrypoint)
    roles_init = Path(args.roles_init)
    runtime_acl = Path(args.runtime_acl)
    args.entrypoint = str(entrypoint)
    args.roles_init = str(roles_init)
    args.runtime_acl = str(runtime_acl)
    config_hashes = {
        'entrypointSha256': config_file(
            entrypoint, 'PostgreSQL entrypoint', executable=True,
        ),
        'rolesInitSha256': config_file(
            roles_init, 'PostgreSQL roles initializer', executable=True,
        ),
        'runtimeAclSha256': config_file(runtime_acl, 'PostgreSQL runtime ACL'),
    }
    inspect_image(args.image, args.expected_image_id)
    volume_device = inspect_volume_set(marker, args.target_volume)
    credential_directory = None
    credentials = None
    credential_manifest_sha256 = None
    container_id = None
    container_name = None
    stage = 'credentials'
    cleanup_errors = []
    log_tail = None
    observed_archive_sha256 = None
    try:
        credential_directory, credentials, credential_manifest_sha256 = create_credentials(
            fixture_root,
        )
        container_name, create_command = build_create_command(
            args, marker, args.target_volume, credentials,
        )
        stage = 'container-create'
        created = run_command(create_command, timeout=120)
        container_id = created.stdout.decode('ascii', errors='strict').strip()
        inspect_owned_container(
            container_id, container_name, marker['fixtureId'], args.expected_image_id,
        )
        stage = 'container-start'
        run_command([DOCKER, 'start', container_id], timeout=120)
        stage = 'postgres-ready'
        wait_ready(container_id, expectation['databaseName'], args.ready_timeout_seconds)
        stage = 'archive-restore'
        observed_archive_sha256 = stream_restore(
            container_id,
            expectation['databaseName'],
            sys.stdin.buffer,
            expectation['archiveSha256'],
            args.restore_timeout_seconds,
        )
        stage = 'runtime-acl'
        apply_runtime_acl(
            container_id,
            expectation['databaseName'],
            runtime_acl,
            config_hashes['runtimeAclSha256'],
        )
        stage = 'restore-verification'
        verification = verify_restore(container_id, expectation, expected_migrations)
        _, current_marker_sha256 = read_private_json(
            fixture_root / MARKER_NAME, 'fixture marker',
        )
        _, current_approval_sha256 = read_private_json(
            fixture_root / APPROVAL_NAME, 'approved storage cap',
        )
        _, current_admission_sha256 = read_private_json(
            fixture_root / ADMISSION_NAME, 'storage admission receipt',
        )
        _, current_expectation_sha256 = read_private_json(
            Path(args.expectation), 'restore expectation',
        )
        # recheck every immutable authority artifact
        if (current_marker_sha256 != marker_sha256 or
                current_approval_sha256 != admission['approvedCapSha256'] or
                current_admission_sha256 != admission_sha256 or
                current_expectation_sha256 != expectation_sha256 or
                utc_now() >= parse_timestamp(approval['expiresAt'], 'expiresAt')):
            raise RestoreError('fixture authority changed or expired during restore')
        # recheck mounted configuration bytes
        if config_hashes != {
                'entrypointSha256': config_file(
                    entrypoint, 'PostgreSQL entrypoint', executable=True,
                ),
                'rolesInitSha256': config_file(
                    roles_init, 'PostgreSQL roles initializer', executable=True,
                ),
                'runtimeAclSha256': config_file(runtime_acl, 'PostgreSQL runtime ACL'),
        }:
            raise RestoreError('mounted PostgreSQL configuration changed during restore')
        stage = 'container-cleanup'
        cleanup_container(
            container_id, container_name, marker['fixtureId'], args.expected_image_id,
        )
        container_id = None
        inspect_volume_set(
            marker,
            args.target_volume,
            allow_target_data=True,
        )
        # ensure the stopped data directory has no server marker
        if (volume_device / 'postmaster.pid').exists() or (volume_device / 'postmaster.pid').is_symlink():
            raise RestoreError('stopped fixture retained postmaster.pid')
        stage = 'handoff'
        handoff = {
            'schemaVersion': 1,
            'kind': HANDOFF_KIND,
            'fixtureId': marker['fixtureId'],
            'fixtureRoot': marker['fixtureRoot'],
            'volumeName': args.target_volume,
            'volumeDevice': str(volume_device),
            'databaseName': expectation['databaseName'],
            'imageId': args.expected_image_id,
            'imageRepoDigest': args.image,
            'archiveSha256': observed_archive_sha256,
            'sourceArchiveCiphertextSha256': admission['sourceArchiveSha256'],
            'markerSha256': marker_sha256,
            'admissionReceiptSha256': admission_sha256,
            'approvedCapSha256': admission['approvedCapSha256'],
            'approvedCapExpiresAt': approval['expiresAt'],
            'expectationSha256': expectation_sha256,
            'expectedLedgerSha256': expected_ledger_sha256,
            'actualLedgerSha256': verification['actualLedgerSha256'],
            'counts': verification['counts'],
            'sampleHash': verification['sampleHash'],
            'serverVersionNum': verification['serverVersionNum'],
            'configSha256': config_hashes,
            'credentialDirectory': str(credential_directory),
            'credentialManifestSha256': credential_manifest_sha256,
            'completedAt': utc_now().strftime('%Y-%m-%dT%H:%M:%SZ'),
        }
        write_private_json(handoff_path, handoff)
        try:
            print(json.dumps(handoff, indent=2, sort_keys=True), flush=True)
        except BrokenPipeError:
            # keep the durable verified handoff authoritative
            return 0
        return 0
    except BaseException as error:
        # retain a bounded owned-container log
        if container_id and container_name:
            try:
                log_tail = container_log_tail(
                    container_id, container_name, marker['fixtureId'], args.expected_image_id,
                )
            except CLEANUP_EXCEPTIONS as log_error:
                cleanup_errors.append(f'log collection: {log_error}')
            try:
                cleanup_container(
                    container_id, container_name, marker['fixtureId'], args.expected_image_id,
                )
                container_id = None
            except CLEANUP_EXCEPTIONS as cleanup_error:
                cleanup_errors.append(f'container cleanup: {cleanup_error}')
        # remove only incomplete fixture credentials
        if credential_directory and credentials:
            try:
                remove_credentials(credential_directory, credentials)
                credential_directory = None
            except CLEANUP_EXCEPTIONS as cleanup_error:
                cleanup_errors.append(f'credential cleanup: {cleanup_error}')
        failure = {
            'schemaVersion': 1,
            'kind': FAILURE_KIND,
            'fixtureId': marker['fixtureId'],
            'volumeName': args.target_volume,
            'stage': stage,
            'error': str(error),
            'cleanupErrors': cleanup_errors,
            'containerLogTail': log_tail,
            'observedArchiveSha256': observed_archive_sha256,
            'failedAt': utc_now().strftime('%Y-%m-%dT%H:%M:%SZ'),
        }
        try:
            write_private_json(failure_path, failure)
        except CLEANUP_EXCEPTIONS as evidence_error:
            cleanup_errors.append(f'failure evidence: {evidence_error}')
        raise RestoreError(
            f'{stage} failed: {error}; failureEvidence={failure_path}; '
            f'cleanupErrors={cleanup_errors}'
        ) from error


# report one bounded cli failure
if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except RestoreError as error:
        print(f'native fixture restore failed: {error}', file=sys.stderr)
        raise SystemExit(1)
