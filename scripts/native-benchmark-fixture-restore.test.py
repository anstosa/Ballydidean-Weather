import argparse
import hashlib
import importlib.util
import json
import os
import subprocess
import tempfile
import threading
import time
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest.mock import patch

SCRIPT = Path(__file__).with_name('native-benchmark-fixture-restore.py')
SPEC = importlib.util.spec_from_file_location('native_fixture_restore', SCRIPT)
restore = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(restore)
STORAGE_SCRIPT = Path(__file__).with_name('native-benchmark-storage-admission.py')
STORAGE_SPEC = importlib.util.spec_from_file_location(
    'native_storage_admission_for_restore', STORAGE_SCRIPT,
)
storage = importlib.util.module_from_spec(STORAGE_SPEC)
STORAGE_SPEC.loader.exec_module(storage)


# build one completed command
def result(stdout=b'', stderr=b'', returncode=0):
    return subprocess.CompletedProcess([], returncode, stdout=stdout, stderr=stderr)


class FixtureRestoreRegression(unittest.TestCase):
    # prepare one owner-private fixture namespace
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.base = Path(self.temporary.name) / 'base'
        self.base.mkdir(mode=0o700)
        self.fixture_id = 'rust-rewrite-0123456789abcdef'
        self.fixture_root = self.base / self.fixture_id
        self.mountpoint = self.fixture_root / 'mnt'
        self.volume_root = self.mountpoint / 'docker-volumes'
        self.volume_root.mkdir(parents=True, mode=0o700)
        self.fixture_root.chmod(0o700)
        self.mountpoint.chmod(0o700)
        self.volume_root.chmod(0o700)
        self.owner = (os.geteuid(), os.getegid())
        self.marker = {
            'schemaVersion': 1,
            'kind': restore.MARKER_KIND,
            'fixtureId': self.fixture_id,
            'fixtureRoot': str(self.fixture_root),
            'backingFile': str(self.fixture_root / 'fixture.ext4'),
            'mountpoint': str(self.mountpoint),
            'volumeRoot': str(self.volume_root),
            'volumeNamespacePrefix': f'weather-native-benchmark-{self.fixture_id}-',
        }
        self.write_private(self.fixture_root / restore.MARKER_NAME, self.marker)

    # remove the private namespace
    def tearDown(self):
        self.temporary.cleanup()

    # write one private json object
    def write_private(self, path, payload):
        path.write_text(json.dumps(payload, sort_keys=True) + '\n', encoding='utf-8')
        path.chmod(0o600)

    # verify the exact approved marker contract
    def test_marker_binds_fixed_namespace_without_symlinks(self):
        marker, marker_path, marker_sha256 = restore.validate_marker(
            self.fixture_root,
            self.base,
            self.owner,
        )
        self.assertEqual(marker, self.marker)
        self.assertEqual(marker_path, self.fixture_root / restore.MARKER_NAME)
        self.assertEqual(marker_sha256, hashlib.sha256(marker_path.read_bytes()).hexdigest())

    # reject an aliased marker root
    def test_marker_rejects_symlink_fixture_root(self):
        link = self.base / 'rust-rewrite-fedcba9876543210'
        link.symlink_to(self.fixture_root, target_is_directory=True)
        with self.assertRaisesRegex(restore.RestoreError, 'symlink|canonical|namespace'):
            restore.validate_marker(link, self.base, self.owner)

    # reject stale actual admission before fixture use
    def test_admission_must_be_fresh_and_current(self):
        marker_path = self.fixture_root / restore.MARKER_NAME
        marker_sha256 = hashlib.sha256(marker_path.read_bytes()).hexdigest()
        approved_at = datetime(2026, 11, 27, 15, 0, tzinfo=timezone.utc)
        expires_at = approved_at + timedelta(days=1)
        approval = {
            'schemaVersion': 1,
            'kind': restore.APPROVAL_KIND,
            'approvalAuthority': 'ansel',
            'fixtureId': self.fixture_id,
            'fixtureRoot': str(self.fixture_root),
            'copyBytes': 1,
            'backingBytes': 5 * 1024 ** 3,
            'otherPeakBytes': 0,
            'maxAdditionalBytes': 20 * 1024 ** 3,
            'rootReserveBytes': 8 * 1024 ** 3,
            'innerReserveBytes': 4 * 1024 ** 3,
            'retentionSeconds': 86400,
            'rootFilesystemSource': '/dev/mmcblk0p2',
            'rootFilesystemUuid': 'root-uuid',
            'approvedAt': approved_at.strftime('%Y-%m-%dT%H:%M:%SZ'),
            'expiresAt': expires_at.strftime('%Y-%m-%dT%H:%M:%SZ'),
        }
        approval_path = self.fixture_root / restore.APPROVAL_NAME
        self.write_private(approval_path, approval)
        approval_sha256 = hashlib.sha256(approval_path.read_bytes()).hexdigest()
        receipt = {
            'receiptVersion': 'weather-native-benchmark-storage/v1',
            'capturedAt': approved_at.strftime('%Y-%m-%dT%H:%M:%SZ'),
            'phase': 'actual-admission',
            'fixtureId': self.fixture_id,
            'fixtureRoot': str(self.fixture_root),
            'backingFile': self.marker['backingFile'],
            'mountpoint': self.marker['mountpoint'],
            'volumeRoot': self.marker['volumeRoot'],
            'volumeNamespacePrefix': self.marker['volumeNamespacePrefix'],
            'admissionReceiptPath': str(self.fixture_root / restore.ADMISSION_NAME),
            'copyBytes': 1,
            'backingBytes': 5 * 1024 ** 3,
            'otherPeakBytes': 0,
            'maxAdditionalBytes': 20 * 1024 ** 3,
            'rootReserveBytes': 8 * 1024 ** 3,
            'innerReserveBytes': 4 * 1024 ** 3,
            'innerRequiredBytes': 4 * 1024 ** 3 + 1,
            'peakRequiredBytes': 5 * 1024 ** 3,
            'rootAvailableBytes': 30 * 1024 ** 3,
            'rootTotalBytes': 64 * 1024 ** 3,
            'rootFilesystem': {},
            'markerPath': str(marker_path),
            'markerSha256': marker_sha256,
            'approvedCapPath': str(approval_path),
            'approvedCapSha256': approval_sha256,
            'approvedCapExpiresAt': approval['expiresAt'],
            'fixtureOuterAllocatedBytes': 5 * 1024 ** 3,
            'fixtureOuterEntryCount': 5,
            'fixtureOuterEntriesSha256': 'e' * 64,
            'backingPresent': True,
            'backingAllocationRemainingBytes': 0,
            'otherAllocatedBytes': 0,
            'otherRemainingBytes': 0,
            'unmeasuredOtherReserveBytes': 0,
            'externalCleanupCreditBytes': 0,
            'rootAdmissionRequiredBytes': 8 * 1024 ** 3,
            'conservativeAdditionalPeakBytes': 5 * 1024 ** 3,
            'sourceArchivePresent': True,
            'sourceArchiveSha256': 'f' * 64,
            'sourceArchiveLogicalBytes': 1,
            'sourceArchiveAllocatedBytes': 1,
            'fixtureFilesystem': {},
            'loopBinding': {},
            'backingLogicalBytes': 5 * 1024 ** 3,
            'backingAllocatedBytes': 5 * 1024 ** 3,
            'actualAdditionalPeakBytes': 5 * 1024 ** 3,
            'innerAvailableBytes': 5 * 1024 ** 3,
            'innerTotalBytes': 5 * 1024 ** 3,
            'estimatePassed': None,
            'actualAdmissionRequired': False,
            'allocationAuthorized': False,
            'fixtureUseAuthorized': True,
            'admitted': True,
        }
        self.write_private(self.fixture_root / restore.ADMISSION_NAME, receipt)
        with self.assertRaisesRegex(restore.RestoreError, 'stale'):
            restore.validate_admission(
                self.marker,
                marker_sha256,
                self.fixture_root,
                300,
                now=approved_at + timedelta(seconds=301),
                command=lambda *_args, **_kwargs: self.fail('stale receipt reached host probes'),
                authority_owner=self.owner,
            )

    # retain exact approved reserves for an already populated resume target
    def test_resume_headroom_does_not_readmit_the_completed_copy(self):
        gib = 1024 ** 3
        self.assertEqual(
            restore.required_storage_headroom(13 * gib, 9 * gib, 8 * gib, 4 * gib, True),
            (8 * gib, 4 * gib),
        )
        self.assertEqual(
            restore.required_storage_headroom(13 * gib, 9 * gib, 8 * gib, 4 * gib, False),
            (9 * gib, 17 * gib),
        )

    # admit only bounded allocation drift after the completed restore
    def test_resume_backing_allocation_rechecks_current_cap(self):
        logical = 19_864_223_744
        admitted = 19_864_252_416
        current = 19_864_256_512
        other_peak = 1_476_395_008
        cap = 20 * 1024 ** 3
        metadata = argparse.Namespace(
            st_nlink=1,
            st_size=logical,
            st_blocks=current // 512,
        )
        self.assertEqual(
            restore.validate_backing_allocation(
                metadata, logical, admitted, other_peak, cap, True,
            ),
            current,
        )
        with self.assertRaisesRegex(restore.RestoreError, 'approved cap'):
            restore.validate_backing_allocation(
                metadata,
                logical,
                admitted,
                cap - current + 512,
                cap,
                True,
            )
        with self.assertRaisesRegex(restore.RestoreError, 'differs from admission'):
            restore.validate_backing_allocation(
                metadata, logical, admitted, other_peak, cap, False,
            )

    # pin the actual admission producer interface and shared root allowlist
    def test_actual_storage_producer_matches_restore_receipt_contract(self):
        gib = 1024 ** 3
        contract = storage.fixture_contract(self.fixture_id)
        plan = {'copyBytes': gib, 'backingBytes': 5 * gib, 'otherPeakBytes': gib}
        now = datetime(2026, 11, 27, 15, 0, tzinfo=timezone.utc)
        approval = {
            'schemaVersion': 1,
            'kind': restore.APPROVAL_KIND,
            'approvalAuthority': 'ansel',
            'fixtureId': self.fixture_id,
            'fixtureRoot': contract['fixtureRoot'],
            **plan,
            'maxAdditionalBytes': 20 * gib,
            'rootReserveBytes': 8 * gib,
            'innerReserveBytes': 4 * gib,
            'retentionSeconds': 86400,
            'rootFilesystemSource': '/dev/mmcblk0p2',
            'rootFilesystemUuid': 'root-uuid',
            'approvedAt': '2026-11-27T15:00:00Z',
            'expiresAt': '2026-11-28T15:00:00Z',
        }
        observation = {
            'system': 'Linux',
            'machine': 'aarch64',
            'kernelRelease': '6.12.0-v8+',
            'effectiveUid': 0,
            'fixtureRootOwnerPrivate': True,
            'fixtureRootOnHostRoot': True,
            'rootMount': {
                'target': '/', 'source': '/dev/mmcblk0p2', 'filesystemType': 'ext4',
                'options': ['relatime', 'rw'], 'uuid': 'root-uuid',
            },
            'rootSpace': {'availableBytes': 30 * gib, 'totalBytes': 64 * gib},
            'marker': contract,
            'markerPath': f'{contract["fixtureRoot"]}/{restore.MARKER_NAME}',
            'markerSha256': 'a' * 64,
            'approval': approval,
            'approvalPath': f'{contract["fixtureRoot"]}/{restore.APPROVAL_NAME}',
            'approvalSha256': 'b' * 64,
            'fixtureOuterAllocatedBytes': 5 * gib + gib // 2,
            'fixtureOuterEntryCount': 8,
            'fixtureOuterEntriesSha256': 'c' * 64,
            'backingPresent': True,
            'backingLogicalBytes': 5 * gib,
            'backingAllocatedBytes': 5 * gib,
            'sourceArchive': {
                'present': True, 'sha256': 'd' * 64,
                'logicalBytes': gib // 2, 'allocatedBytes': gib // 2,
            },
            'backingFileOwnerPrivate': True,
            'backingFileOnHostRoot': True,
            'mountpointOwnerPrivate': True,
            'mountDeviceMatchesLoop': True,
            'fixtureMount': {
                'target': contract['mountpoint'], 'source': '/dev/loop7',
                'filesystemType': 'ext4', 'options': ['relatime', 'rw'],
                'uuid': 'fixture-uuid',
            },
            'fixtureLoop': {
                'device': '/dev/loop7', 'backingFile': contract['backingFile'],
                'offsetBytes': 0, 'sizeLimitBytes': 5 * gib, 'readOnly': False,
            },
            'innerSpace': {'availableBytes': 5 * gib, 'totalBytes': 5 * gib},
        }
        receipt = storage.evaluate('admit', contract, plan, observation, now)
        self.assertEqual(set(receipt), restore.ADMISSION_KEYS)
        self.assertEqual(receipt['externalCleanupCreditBytes'], 0)
        self.assertIn(restore.HANDOFF_NAME, storage.OUTER_FIXED_NAMES)
        self.assertIn(restore.FAILURE_NAME, storage.OUTER_FIXED_NAMES)
        self.assertIsNotNone(storage.RESTORE_SECRET_DIRECTORY_PATTERN.fullmatch(
            '.restore-secrets-0123456789abcdef',
        ))

    # use the exact util-linux 2.41 compatible loop inspection command
    def test_loop_inspection_omits_unsupported_bytes_flag(self):
        expected = [
            '/usr/sbin/losetup', '--json', '--list',
            '--output', 'NAME,BACK-FILE,OFFSET,SIZELIMIT,RO', '/dev/loop7',
        ]

        # return one supported default-byte record
        def command(argv, **kwargs):
            self.assertEqual(argv, expected)
            self.assertEqual(kwargs['timeout'], 30)
            payload = {'loopdevices': [{
                'name': '/dev/loop7',
                'back-file': '/fixture/fixture.ext4',
                'offset': 0,
                'sizelimit': 19864223744,
                'ro': False,
            }]}
            return result(json.dumps(payload).encode('utf-8'))

        record = restore.inspect_loop_binding('/dev/loop7', command)
        self.assertEqual(record['sizelimit'], 19864223744)
        self.assertNotIn('--bytes', expected)

    # bind immutable migration and data evidence
    def test_expectation_requires_exact_digest_and_ordered_ledger(self):
        expectation = {
            'schemaVersion': 1,
            'kind': restore.EXPECTATION_KIND,
            'databaseName': 'weather',
            'archiveSha256': 'a' * 64,
            'migrations': [
                {'name': '0001_initial.sql', 'checksum': 'b' * 64},
                {'name': '0002_more.sql', 'checksum': 'c' * 64},
            ],
            'counts': {'sites': 1, 'sources': 2, 'runs': 3, 'records': 4},
            'sampleHash': 'd' * 32,
        }
        path = self.base / 'expectation.json'
        self.write_private(path, expectation)
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        payload, migrations, observed = restore.validate_expectation(path, digest, self.owner)
        self.assertEqual(payload, expectation)
        self.assertEqual(migrations[1][0], '0002_more.sql')
        self.assertEqual(observed, digest)
        with self.assertRaisesRegex(restore.RestoreError, 'SHA-256'):
            restore.validate_expectation(path, '0' * 64, self.owner)

    # require an existing exact arm64 digest image
    def test_image_inspection_binds_repo_digest_id_and_platform(self):
        digest = '1' * 64
        image = f'ghcr.io/example/postgres@sha256:{digest}'
        image_id = f'sha256:{"2" * 64}'

        # return one local image record
        def command(argv, **_kwargs):
            self.assertEqual(argv, [restore.DOCKER, 'image', 'inspect', image])
            payload = [{'Id': image_id, 'Os': 'linux', 'Architecture': 'arm64',
                        'RepoDigests': [image]}]
            return result(json.dumps(payload).encode('utf-8'))

        self.assertEqual(restore.inspect_image(image, image_id, command)['Id'], image_id)
        # accept the exact short official repository digest recorded by docker
        image = f'postgres@sha256:{digest}'
        self.assertEqual(restore.inspect_image(image, image_id, command)['Id'], image_id)
        with self.assertRaisesRegex(restore.RestoreError, 'digest'):
            restore.inspect_image('postgres:17', image_id, command)

    # reject executable bind sources on noexec mounts
    def test_config_file_rejects_noexec_mount(self):
        script = self.base / 'entrypoint.sh'
        script.write_text('#!/bin/sh\n')
        script.chmod(0o755)
        with patch.object(restore.os, 'statvfs') as filesystem:
            filesystem.return_value.f_flag = os.ST_NOEXEC
            with self.assertRaisesRegex(restore.RestoreError, 'noexec'):
                restore.config_file(script, 'entrypoint', executable=True)

    # wait for the final server rather than the temporary initialization socket
    def test_readiness_checks_final_postmaster(self):
        calls = []

        # accept only after the final server replaces the entrypoint
        def command(argv, **_kwargs):
            calls.append(argv)
            return result(returncode=1 if len(calls) == 1 else 0)

        restore.wait_ready('container', 'weather', 10, command=command,
                           monotonic=lambda: 0, sleeper=lambda _: None)
        self.assertEqual(len(calls), 2)
        self.assertIn('test "$(cat /proc/1/comm)" = postgres && exec pg_isready "$@"',
                      calls[0])

    # reject any production or mismatched volume binding
    def test_volume_must_be_exact_detached_fixture_bind(self):
        name = f'{self.marker["volumeNamespacePrefix"]}slot-01'
        device = self.volume_root / name
        device.mkdir(mode=0o700)
        calls = []

        # return only fixture-scoped metadata
        def command(argv, **_kwargs):
            calls.append(argv)
            # select the exact inspection
            if argv[1:3] == ['volume', 'inspect']:
                payload = [{
                    'Name': name,
                    'Driver': 'local',
                    'Scope': 'local',
                    'Options': {'type': 'none', 'o': 'bind', 'device': str(device)},
                    'Labels': {
                        'weather.native-benchmark.fixture-id': self.fixture_id,
                        'weather.native-benchmark.kind': restore.VOLUME_KIND,
                    },
                }]
                return result(json.dumps(payload).encode('utf-8'))
            return result()

        observed, _ = restore.inspect_volume(self.marker, name, command, self.owner)
        self.assertEqual(observed, device)
        self.assertEqual(calls[-1][-1], f'volume={name}')

    # reject residual data in every non-target sequential slot
    def test_volume_set_rejects_non_target_residual_data(self):
        prefix = self.marker['volumeNamespacePrefix']
        # derive the fixed slot names
        names = [f'{prefix}{suffix}' for suffix in restore.VOLUME_SUFFIXES]
        # create the complete empty slot layout
        for name in names:
            (self.volume_root / name).mkdir(mode=0o700)
        (self.volume_root / names[1] / 'residual').write_text('stale')
        calls = []

        # expose only the five fixture-owned volumes
        def command(argv, **_kwargs):
            calls.append(argv)
            # list the complete labeled set
            if argv[1:3] == ['volume', 'ls']:
                return result(('\n'.join(names) + '\n').encode('ascii'))
            # inspect one exact bind
            if argv[1:3] == ['volume', 'inspect']:
                name = argv[-1]
                payload = [{
                    'Name': name,
                    'Driver': 'local',
                    'Scope': 'local',
                    'Options': {
                        'type': 'none', 'o': 'bind',
                        'device': str(self.volume_root / name),
                    },
                    'Labels': {
                        'weather.native-benchmark.fixture-id': self.fixture_id,
                        'weather.native-benchmark.kind': restore.VOLUME_KIND,
                    },
                }]
                return result(json.dumps(payload).encode('utf-8'))
            return result()

        with self.assertRaisesRegex(restore.RestoreError, 'must be empty'):
            restore.inspect_volume_set(
                self.marker,
                names[0],
                command=command,
                target_owner=self.owner,
            )
        # exclude production data paths
        self.assertTrue(all('/var/lib/weather/postgres' not in ' '.join(call) for call in calls))

    # ensure container construction has no ports or network
    def test_create_command_is_networkless_pull_never_and_uses_no_secret_values(self):
        name = f'{self.marker["volumeNamespacePrefix"]}slot-01'
        arguments = argparse.Namespace(
            entrypoint='/fixture/entrypoint.sh',
            roles_init='/fixture/roles.sh',
            runtime_acl='/fixture/acl.sql',
            database_name='weather',
            image=f'ghcr.io/example/postgres@sha256:{"1" * 64}',
        )
        # map fixed names to fixture-only paths
        credentials = {
            secret_name: Path('/fixture/secrets') / secret_name
            for secret_name in restore.SECRET_NAMES
        }
        _, command = restore.build_create_command(arguments, self.marker, name, credentials)
        joined = ' '.join(map(str, command))
        self.assertIn('--network none', joined)
        self.assertIn('--pull never', joined)
        self.assertIn('listen_addresses=', joined)
        self.assertIn('--log-driver json-file', joined)
        self.assertIn('--log-opt max-file=3 --log-opt max-size=10m', joined)
        self.assertNotIn('--publish', command)
        self.assertNotIn('-p', command)
        self.assertNotIn('/var/lib/weather/postgres', joined)

    # bypass initialization and reconciliation only for populated verification resume
    def test_resume_create_command_uses_standard_existing_cluster_entrypoint(self):
        name = f'{self.marker["volumeNamespacePrefix"]}slot-01'
        arguments = argparse.Namespace(
            entrypoint='/fixture/entrypoint.sh',
            roles_init='/fixture/roles.sh',
            runtime_acl='/fixture/acl.sql',
            database_name='weather',
            image=f'postgres@sha256:{"1" * 64}',
        )
        credentials = {
            secret_name: Path('/fixture/secrets') / secret_name
            for secret_name in restore.SECRET_NAMES
        }
        _, command = restore.build_resume_create_command(
            arguments, self.marker, name, credentials,
        )
        entrypoint = command[command.index('--entrypoint') + 1]
        self.assertEqual(entrypoint, '/usr/local/bin/docker-entrypoint.sh')
        self.assertIn('--network', command)
        self.assertEqual(command[command.index('--network') + 1], 'none')
        self.assertNotIn('/run/secrets/', ' '.join(map(str, command)))
        self.assertNotIn('PASSWORD_FILE=', ' '.join(map(str, command)))

    # admit only the exact completed-stream count-timeout evidence
    def test_resume_failure_binds_digest_stage_archive_and_active_approval(self):
        path = self.fixture_root / restore.FAILURE_NAME
        count_query = (
            'SELECT (SELECT count(*) FROM sites),(SELECT count(*) FROM sources),'
            '(SELECT count(*) FROM ingestion_runs),(SELECT count(*) FROM weather_records)'
        )
        failure = {
            'schemaVersion': 1,
            'kind': restore.FAILURE_KIND,
            'fixtureId': self.fixture_id,
            'volumeName': f'{self.marker["volumeNamespacePrefix"]}slot-01',
            'stage': 'restore-verification',
            'error': f"Command ['psql', '{count_query}'] timed out after 120 seconds",
            'cleanupErrors': [],
            'containerLogTail': '',
            'observedArchiveSha256': 'a' * 64,
            'failedAt': '2026-11-27T16:00:00Z',
        }
        self.write_private(path, failure)
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        expectation = {'archiveSha256': 'a' * 64}
        approval = {'approvedAt': '2026-11-27T15:00:00Z',
                    'expiresAt': '2026-11-28T15:00:00Z'}
        observed, observed_sha256 = restore.validate_resume_failure(
            path,
            digest,
            self.marker,
            failure['volumeName'],
            expectation,
            approval,
            now=datetime(2026, 11, 27, 17, 0, tzinfo=timezone.utc),
            owner=self.owner,
        )
        self.assertEqual(observed, failure)
        self.assertEqual(observed_sha256, digest)
        # reject a failure from before the active approval window
        with self.assertRaisesRegex(restore.RestoreError, 'active approval'):
            restore.validate_resume_failure(
                path, digest, self.marker, failure['volumeName'], expectation,
                {**approval, 'approvedAt': '2026-11-27T16:01:00Z'},
                now=datetime(2026, 11, 27, 17, 0, tzinfo=timezone.utc),
                owner=self.owner,
            )
        with self.assertRaisesRegex(restore.RestoreError, 'SHA-256'):
            restore.validate_resume_failure(
                path, '0' * 64, self.marker, failure['volumeName'], expectation,
                approval, now=datetime(2026, 11, 27, 17, 0, tzinfo=timezone.utc),
                owner=self.owner,
            )
        with self.assertRaisesRegex(restore.RestoreError, 'not resumable'):
            restore.validate_resume_failure(
                path, digest, self.marker, failure['volumeName'],
                {'archiveSha256': 'b' * 64}, approval,
                now=datetime(2026, 11, 27, 17, 0, tzinfo=timezone.utc),
                owner=self.owner,
            )
        with self.assertRaisesRegex(restore.RestoreError, 'active approval'):
            restore.validate_resume_failure(
                path, digest, self.marker, failure['volumeName'], expectation,
                approval, now=datetime(2026, 11, 28, 15, 0, tzinfo=timezone.utc),
                owner=self.owner,
            )

    # require one populated stopped PostgreSQL control tree
    def test_resume_pgdata_rejects_empty_or_running_target(self):
        target = self.volume_root / 'slot-01'
        target.mkdir(mode=0o700)
        with self.assertRaisesRegex(restore.RestoreError, 'PG_VERSION'):
            restore.validate_resume_pgdata(target, self.owner)
        (target / 'global').mkdir(mode=0o700)
        (target / 'PG_VERSION').write_text('17\n')
        (target / 'global' / 'pg_control').write_bytes(b'control')
        restore.validate_resume_pgdata(target, self.owner)
        (target / 'postmaster.pid').write_text('123\n')
        with self.assertRaisesRegex(restore.RestoreError, 'postmaster.pid'):
            restore.validate_resume_pgdata(target, self.owner)

    # rotate only fixed roles while keeping secret values out of argv
    def test_resume_role_rotation_pipes_bounded_exact_role_sql(self):
        credentials = {
            secret_name: Path('/fixture/secrets') / secret_name
            for secret_name in restore.SECRET_NAMES
        }
        plaintext_values = [
            'admin-plain-secret',
            'owner-plain-secret',
            'api-plain-secret',
            'ingest-plain-secret',
            'training-plain-secret',
        ]
        values = iter(plaintext_values)
        calls = []

        # capture the one isolated psql invocation
        def command(argv, **kwargs):
            calls.append((argv, kwargs))
            return result()

        with patch.object(restore, 'read_credential', side_effect=lambda *_args: next(values)):
            restore.rotate_fixture_roles('f' * 64, 'weather', credentials, command)
        argv, kwargs = calls[0]
        joined = ' '.join(argv)
        sql = kwargs['input_bytes'].decode('utf-8')
        self.assertNotIn('admin', joined)
        self.assertNotIn('training', joined)
        self.assertEqual(kwargs['timeout'], 300)
        self.assertTrue(kwargs['redact_output'])
        self.assertIn("ALTER ROLE postgres PASSWORD 'SCRAM-SHA-256$4096:", sql)
        self.assertIn('ALTER ROLE weather_training_export', sql)
        self.assertNotIn('CREATE ROLE', sql)
        self.assertNotIn('RESET ALL', sql)
        self.assertNotIn(' NOINHERIT', sql)
        self.assertNotIn(' LOGIN', sql)
        self.assertIn("SET LOCAL log_min_error_statement = 'panic';", sql)
        # never send the fresh plaintext password bytes to PostgreSQL
        for plaintext in plaintext_values:
            self.assertNotIn(plaintext, sql)

    # redact sensitive stdin and PostgreSQL diagnostics from the caller
    def test_sensitive_command_failure_never_surfaces_secret_output(self):
        secret = 'fixture-secret-value'
        completed = subprocess.CompletedProcess(
            ['psql'],
            1,
            stdout=f'echo {secret}'.encode('ascii'),
            stderr=f'LINE 1: PASSWORD {secret}'.encode('ascii'),
        )
        with (
            patch.object(restore.subprocess, 'run', return_value=completed),
            self.assertRaises(restore.RestoreError) as raised,
        ):
            restore.run_command(
                ['psql', '--interactive'],
                input_bytes=secret.encode('ascii'),
                redact_output=True,
            )
        self.assertNotIn(secret, str(raised.exception))
        self.assertIn('sensitive output redacted', str(raised.exception))

    # stream archive bytes without a plaintext file
    def test_archive_is_streamed_and_hashed_in_memory(self):
        archive = b'custom-format-archive\x00payload'
        source_read, source_write = os.pipe()
        os.write(source_write, archive)
        os.close(source_write)
        child_read, child_write = os.pipe()
        stderr_read, stderr_write = os.pipe()
        received = bytearray()
        completed = threading.Event()

        # drain the emulated child input
        def consume():
            with os.fdopen(child_read, 'rb', buffering=0) as child_source:
                # collect every child chunk
                while True:
                    chunk = child_source.read(8192)
                    # stop at eof
                    if not chunk:
                        break
                    received.extend(chunk)
            os.close(stderr_write)
            completed.set()

        consumer = threading.Thread(target=consume)
        consumer.start()

        # emulate one successful docker exec client
        class Process:
            # initialize one process
            def __init__(self):
                self.args = ['docker', 'exec']
                self.stdin = os.fdopen(child_write, 'wb', buffering=0)
                self.stderr = os.fdopen(stderr_read, 'rb', buffering=0)

            # report current completion
            def poll(self):
                # expose completed state
                if completed.is_set():
                    return 0
                return None

            # return successful completion
            def wait(self, timeout=None):
                # require completion before timeout
                if not completed.wait(timeout):
                    raise subprocess.TimeoutExpired(self.args, timeout)
                return 0

            # reject unnecessary termination
            def kill(self):
                raise AssertionError('unexpected kill')

        process = Process()

        # capture the exact exec command
        def popen(argv, **kwargs):
            self.assertEqual(kwargs['stdin'], subprocess.PIPE)
            self.assertNotIn('/var/lib/weather/postgres', ' '.join(argv))
            return process

        digest = hashlib.sha256(archive).hexdigest()
        with os.fdopen(source_read, 'rb', buffering=0) as source:
            observed = restore.stream_restore(
                'f' * 64,
                'weather',
                source,
                digest,
                60,
                popen,
            )
        consumer.join(timeout=1)
        self.assertFalse(consumer.is_alive())
        self.assertEqual(observed, digest)
        self.assertEqual(bytes(received), archive)

    # enforce the total deadline while the plaintext source stalls
    def test_archive_stream_timeout_interrupts_stalled_source(self):
        source_read, source_write = os.pipe()
        child_read, child_write = os.pipe()
        stderr_read, stderr_write = os.pipe()

        # emulate one stalled docker exec client
        class Process:
            # initialize one process
            def __init__(self):
                self.args = ['docker', 'exec']
                self.stdin = os.fdopen(child_write, 'wb', buffering=0)
                self.stderr = os.fdopen(stderr_read, 'rb', buffering=0)
                self.killed = False

            # report a running client
            def poll(self):
                # expose killed state
                if self.killed:
                    return -9
                return None

            # terminate the owned client
            def kill(self):
                self.killed = True
                os.close(stderr_write)
                os.close(child_read)

            # wait only after termination
            def wait(self, timeout=None):
                # reject premature waits
                if not self.killed:
                    raise subprocess.TimeoutExpired(self.args, timeout)
                return -9

        process = Process()
        started = time.monotonic()
        try:
            with (
                os.fdopen(source_read, 'rb', buffering=0) as source,
                self.assertRaisesRegex(restore.RestoreError, 'total streaming timeout'),
            ):
                restore.stream_restore(
                    'f' * 64,
                    'weather',
                    source,
                    'a' * 64,
                    0.05,
                    lambda *_args, **_kwargs: process,
                )
        finally:
            os.close(source_write)
        self.assertTrue(process.killed)
        self.assertLess(time.monotonic() - started, 1)

    # enforce the total deadline while child input backpressure stalls
    def test_archive_stream_timeout_interrupts_stalled_backpressure(self):
        source_read, source_write = os.pipe()
        child_read, child_write = os.pipe()
        stderr_read, stderr_write = os.pipe()
        producer_done = threading.Event()

        # continuously feed more than both pipe capacities
        def produce():
            try:
                # write a bounded oversized stream
                for _ in range(256):
                    os.write(source_write, b'x' * 65536)
            except BrokenPipeError:
                pass
            finally:
                os.close(source_write)
                producer_done.set()

        producer = threading.Thread(target=produce)
        producer.start()

        # emulate a child that never reads stdin
        class Process:
            # initialize one process
            def __init__(self):
                self.args = ['docker', 'exec']
                self.stdin = os.fdopen(child_write, 'wb', buffering=0)
                self.stderr = os.fdopen(stderr_read, 'rb', buffering=0)
                self.killed = False

            # report a running client
            def poll(self):
                # expose killed state
                if self.killed:
                    return -9
                return None

            # terminate the owned client
            def kill(self):
                self.killed = True
                os.close(stderr_write)
                os.close(child_read)

            # wait only after termination
            def wait(self, timeout=None):
                # reject premature waits
                if not self.killed:
                    raise subprocess.TimeoutExpired(self.args, timeout)
                return -9

        process = Process()
        started = time.monotonic()
        with (
            os.fdopen(source_read, 'rb', buffering=0) as source,
            self.assertRaisesRegex(restore.RestoreError, 'total streaming timeout'),
        ):
            restore.stream_restore(
                'f' * 64,
                'weather',
                source,
                'a' * 64,
                0.05,
                lambda *_args, **_kwargs: process,
            )
        producer.join(timeout=1)
        self.assertTrue(producer_done.is_set())
        self.assertFalse(producer.is_alive())
        self.assertTrue(process.killed)
        self.assertLess(time.monotonic() - started, 1)

    # pipe the reviewed runtime acl without relying on container file permissions
    def test_runtime_acl_is_piped_as_exact_reviewed_bytes(self):
        acl = self.fixture_root / 'runtime-acl.sql'
        acl.write_bytes(b'\\set ON_ERROR_STOP on\nSELECT 1;\n')
        acl.chmod(0o600)
        digest = hashlib.sha256(acl.read_bytes()).hexdigest()

        # capture only the isolated docker exec input
        def command(argv, **kwargs):
            self.assertEqual(argv[0:3], [restore.DOCKER, 'exec', '--interactive'])
            self.assertEqual(kwargs['input_bytes'], acl.read_bytes())
            self.assertNotIn('--file', argv)
            return result()

        restore.apply_runtime_acl('f' * 64, 'weather', acl, digest, command)

    # complete durable publication across short writes
    def test_private_json_retries_short_writes(self):
        destination = self.fixture_root / restore.HANDOFF_NAME
        payload = {'schemaVersion': 1, 'kind': restore.HANDOFF_KIND}
        real_write = os.write

        # force every write to make partial progress
        def short_write(descriptor, data):
            return real_write(descriptor, data[:max(1, len(data) // 3)])

        with patch.object(restore.os, 'write', side_effect=short_write):
            restore.write_private_json(
                destination,
                payload,
                authority_owner=self.owner,
            )
        self.assertEqual(json.loads(destination.read_text(encoding='utf-8')), payload)
        self.assertEqual(destination.stat().st_mode & 0o777, 0o600)

    # verify exact ledger counts hash and safe roles
    def test_restore_verification_matches_immutable_evidence(self):
        migrations = [('0001_initial.sql', 'a' * 64)]
        expectation = {
            'databaseName': 'weather',
            'counts': {'sites': 1, 'sources': 2, 'runs': 3, 'records': 4},
            'sampleHash': 'b' * 32,
        }

        # answer only fixed verification queries
        def command(argv, **_kwargs):
            sql = argv[-1]
            # select the server version
            if sql == 'SHOW server_version_num':
                return result(b'170010\n')
            # select the ledger
            if sql.startswith('SELECT name, checksum'):
                return result(f'0001_initial.sql|{"a" * 64}\n'.encode('ascii'))
            # select unsafe role flags
            if 'AND (rolsuper' in sql:
                return result()
            # select the exact role properties
            if sql.startswith('SELECT rolname,rolcanlogin'):
                return result(
                    b'weather_api|1|0|0|0|0|0|0|\n'
                    b'weather_ingest|1|0|0|0|0|0|0|\n'
                    b'weather_owner|1|1|0|0|0|0|0|\n'
                    b'weather_training_export|1|0|0|0|0|0|0|'
                    b'default_transaction_read_only=on\n'
                )
            # select the read-only core runtime ACL proof
            if sql.startswith('WITH acl_checks'):
                self.assertIn('ingest_weather_records_table_select_absent', sql)
                self.assertIn(
                    "has_column_privilege('weather_ingest','weather_records',"
                    "'source_id','SELECT')",
                    sql,
                )
                return result()
            # select representative counts
            if sql.startswith('SELECT (SELECT count(*)'):
                self.assertEqual(
                    _kwargs['timeout'], restore.VERIFICATION_COUNT_TIMEOUT_SECONDS,
                )
                return result(b'1|2|3|4\n')
            # select representative rows
            if sql.startswith('SELECT md5'):
                return result(f'{"b" * 32}\n'.encode('ascii'))
            self.fail(f'unexpected SQL: {sql}')

        evidence = restore.verify_restore('f' * 64, expectation, migrations, command)
        self.assertEqual(evidence['counts']['records'], 4)
        self.assertEqual(evidence['sampleHash'], 'b' * 32)

    # identify the exact sanitized ACL predicate that failed
    def test_runtime_acl_failure_names_exact_predicate(self):
        # return one fixed failed predicate from the deployed ACL query
        def command(argv, **_kwargs):
            sql = argv[-1]
            self.assertTrue(sql.startswith('WITH acl_checks'))
            return result(b'ingest_weather_records_source_id_insert\n')

        with self.assertRaisesRegex(
                restore.RestoreError,
                'ingest_weather_records_source_id_insert'):
            restore.verify_runtime_acl('f' * 64, 'weather', command)

    # reject an owned-label container without the bounded log policy
    def test_container_ownership_requires_bounded_log_rotation(self):
        container_id = 'f' * 64
        container_name = f'weather-native-restore-{self.fixture_id}'
        image_id = f'sha256:{"e" * 64}'
        payload = [{
            'Id': container_id,
            'Name': f'/{container_name}',
            'Image': image_id,
            'Config': {'Labels': {
                'weather.native-benchmark.fixture-id': self.fixture_id,
                'weather.native-benchmark.kind': restore.VOLUME_KIND,
                'weather.native-benchmark.role': restore.CONTAINER_ROLE,
            }},
            'HostConfig': {'LogConfig': {
                'Type': 'json-file',
                'Config': {'max-file': '1', 'max-size': '10m'},
            }},
        }]

        # return one otherwise owned container
        def command(_argv, **_kwargs):
            return result(json.dumps(payload).encode('utf-8'))

        with self.assertRaisesRegex(restore.RestoreError, 'ownership proof'):
            restore.inspect_owned_container(
                container_id,
                container_name,
                self.fixture_id,
                image_id,
                command,
            )

    # touch only the exact proven container id
    def test_cleanup_revalidates_owned_container_before_stop_and_remove(self):
        container_id = 'f' * 64
        container_name = f'weather-native-restore-{self.fixture_id}'
        image_id = f'sha256:{"e" * 64}'
        calls = []
        inspections = 0

        # emulate two ownership inspections
        def command(argv, **_kwargs):
            nonlocal inspections
            calls.append(argv)
            # return the current owned state
            if argv[1:3] == ['container', 'inspect']:
                inspections += 1
                payload = [{
                    'Id': container_id,
                    'Name': f'/{container_name}',
                    'Image': image_id,
                    'Config': {'Labels': {
                        'weather.native-benchmark.fixture-id': self.fixture_id,
                        'weather.native-benchmark.kind': restore.VOLUME_KIND,
                        'weather.native-benchmark.role': restore.CONTAINER_ROLE,
                    }},
                    'HostConfig': {'LogConfig': {
                        'Type': 'json-file',
                        'Config': {'max-file': '3', 'max-size': '10m'},
                    }},
                    'State': {'Running': inspections == 1},
                }]
                return result(json.dumps(payload).encode('utf-8'))
            return result()

        restore.cleanup_container(
            container_id,
            container_name,
            self.fixture_id,
            image_id,
            command,
        )
        self.assertEqual(calls[-2], [restore.DOCKER, 'container', 'inspect', container_id])
        self.assertEqual(calls[-1], [restore.DOCKER, 'rm', container_id])
        # exclude every production data path
        self.assertTrue(all('/var/lib/weather/postgres' not in ' '.join(call) for call in calls))


# run the focused regression suite
if __name__ == '__main__':
    unittest.main()
