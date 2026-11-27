import argparse
import hashlib
import importlib.util
import json
import os
import tempfile
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest import mock

SCRIPT = Path(__file__).with_name('native-benchmark-cold-proof.py')
SPEC = importlib.util.spec_from_file_location('native_benchmark_cold_proof', SCRIPT)
cold = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(cold)
ADMISSION_SCRIPT = Path(__file__).with_name('native-benchmark-storage-admission.py')
ADMISSION_SPEC = importlib.util.spec_from_file_location(
    'native_benchmark_storage_admission_for_cold_proof', ADMISSION_SCRIPT,
)
storage_admission = importlib.util.module_from_spec(ADMISSION_SPEC)
ADMISSION_SPEC.loader.exec_module(storage_admission)


class FakeHost:
    """Mock system and residency observations."""

    # retain one fixture model
    def __init__(
        self,
        marker,
        residents=None,
        attachments=None,
        mutate=None,
        flush_error_path=None,
    ):
        self.marker = marker
        self.residents = {} if residents is None else residents
        self.attachments = {} if attachments is None else attachments
        self.mutate = mutate
        self.flush_error_path = flush_error_path
        self.evictions = []
        self.fixture_events = []
        self.storage_observations = 0
        self.clocks = iter([
            '2026-11-27T15:00:00.000Z',
            '2026-11-27T15:00:01.000Z',
            '2026-11-27T15:00:02.000Z',
        ])

    # accept the mocked platform
    def require_platform(self):
        return None

    # record the mocked storage observation
    def observe_storage(self, storage):
        self.storage_observations += 1
        self.fixture_events.append(('storage', storage['loopDevice']))

    # return one exact mocked volume
    def volume_record(self, name):
        self.fixture_events.append(('volume', name))
        return {
            'Name': name,
            'Driver': 'local',
            'Options': {
                'device': str(Path(self.marker['volumeRoot']) / name),
                'o': 'bind',
                'type': 'none',
            },
            'Labels': {
                'weather.native-benchmark.fixture-id': self.marker['fixtureId'],
                'weather.native-benchmark.kind': 'rust-rewrite',
            },
        }

    # return mocked running attachments
    def running_attachments(self, name):
        self.fixture_events.append(('attachments', name))
        return self.attachments.get(name, [])

    # return mocked zero-residency evidence
    def evict_and_measure(self, path, expected, flush_before_eviction=False):
        # record only an explicitly requested targeted flush
        if flush_before_eviction:
            self.fixture_events.append(('flush', path))
            # fail before the mocked eviction
            if path == self.flush_error_path:
                raise OSError(f'flush failed: {path}')
        self.evictions.append(path)
        self.fixture_events.append(('evict', path))
        # apply one requested mutation
        if self.mutate is not None:
            self.mutate(path)
        page_size = os.sysconf('SC_PAGE_SIZE')
        pages = 0 if expected.size == 0 else (expected.size + page_size - 1) // page_size
        return pages, self.residents.get(str(path), 0)

    # return deterministic clocks
    def now(self):
        return next(self.clocks)


class BadVolumeHost(FakeHost):
    """Return one substituted Docker binding."""

    # substitute one volume path
    def volume_record(self, name):
        record = super().volume_record(name)
        record['Options']['device'] = str(Path(self.marker['volumeRoot']) / 'wrong')
        return record


class ColdProofRegression(unittest.TestCase):
    # build one tiny private fixture
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.owner_uid = os.geteuid()
        self.base_root = Path(self.temporary.name) / 'weather-native-benchmark'
        self.base_root.mkdir(mode=0o700)
        self.fixture_id = 'rust-rewrite-0123456789abcdef'
        self.fixture_root = self.base_root / self.fixture_id
        self.fixture_root.mkdir(mode=0o700)
        self.backing = self.fixture_root / 'fixture.ext4'
        self.backing.write_bytes(b'outer-cache-pages')
        self.backing.chmod(0o600)
        self.mount = self.fixture_root / 'mnt'
        self.mount.mkdir(mode=0o700)
        self.volume_root = self.mount / 'docker-volumes'
        self.volume_root.mkdir(mode=0o700)
        self.prefix = f'weather-native-benchmark-{self.fixture_id}-'
        # build the five run-slot names
        self.volume_names = [f'{self.prefix}slot-{index:02d}' for index in range(1, 6)]
        # create each dedicated descendant
        for name in self.volume_names:
            (self.volume_root / name).mkdir(mode=0o700)
        self.target_volume = self.volume_names[0]
        self.pgdata = self.volume_root / self.target_volume
        (self.pgdata / 'base').mkdir(mode=0o700)
        (self.pgdata / 'PG_VERSION').write_text('16\n')
        (self.pgdata / 'base' / '1').write_bytes(b'database pages')
        self.marker = {
            'schemaVersion': 1,
            'kind': cold.FIXTURE_KIND,
            'fixtureId': self.fixture_id,
            'fixtureRoot': str(self.fixture_root),
            'backingFile': str(self.backing),
            'mountpoint': str(self.mount),
            'volumeRoot': str(self.volume_root),
            'volumeNamespacePrefix': self.prefix,
        }
        self.storage = {
            'backingFile': str(self.backing),
            'backingFileBytes': self.backing.stat().st_size,
            'loopDevice': '/dev/loop7',
            'loopUuid': '12345678-1234-1234-1234-123456789abc',
            'mount': str(self.mount),
            'mountSource': '/dev/loop7',
            'volumeNames': self.volume_names,
        }
        self.marker_path = self.fixture_root / '.fixture-marker.json'
        self.write_private_json(self.marker_path, self.marker)
        self.approved_path = self.fixture_root / '.approved-cap.json'
        now = datetime.now(timezone.utc).replace(microsecond=0)
        self.approved_at = now - timedelta(minutes=1)
        self.expires_at = self.approved_at + timedelta(hours=24)
        self.captured_at = now
        self.approval = self.make_approval()
        self.write_private_json(self.approved_path, self.approval)
        self.admission_path = self.fixture_root / '.storage-admission.json'
        self.admission = self.make_admission()
        self.write_private_json(self.admission_path, self.admission)
        self.run_root = Path(self.temporary.name) / 'run'
        self.run_root.mkdir(mode=0o700)
        self.output_root = self.run_root / f'weather-native-benchmark-{self.fixture_id}'
        self.output_root.mkdir(mode=0o700)
        self.policy = cold.Policy(
            base_root=self.base_root,
            runtime_root=self.run_root,
            owner_uid=self.owner_uid,
        )

    # write one private fixture file
    def write_private_json(self, path, value):
        path.write_text(json.dumps(value) + '\n')
        path.chmod(0o600)

    # format one approval clock
    def format_clock(self, value):
        return value.strftime('%Y-%m-%dT%H:%M:%SZ')

    # build one exact approval
    def make_approval(self):
        return {
            'schemaVersion': 1,
            'kind': 'weather-native-rust-rewrite-storage-approval',
            'approvalAuthority': 'ansel',
            'fixtureId': self.fixture_id,
            'fixtureRoot': str(self.fixture_root),
            'copyBytes': cold.GIB,
            'backingBytes': 5 * cold.GIB,
            'otherPeakBytes': cold.GIB,
            'maxAdditionalBytes': 20 * cold.GIB,
            'rootReserveBytes': 8 * cold.GIB,
            'innerReserveBytes': 4 * cold.GIB,
            'retentionSeconds': 24 * 60 * 60,
            'rootFilesystemSource': '/dev/nvme0n1p2',
            'rootFilesystemUuid': 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
            'approvedAt': self.format_clock(self.approved_at),
            'expiresAt': self.format_clock(self.expires_at),
        }

    # build one admitted handoff
    def make_admission(self):
        backing_bytes = 5 * cold.GIB
        other_bytes = cold.GIB
        copy_bytes = cold.GIB
        marker_sha256 = hashlib.sha256(self.marker_path.read_bytes()).hexdigest()
        approved_sha256 = hashlib.sha256(self.approved_path.read_bytes()).hexdigest()
        archive_bytes = cold.GIB // 2
        plan = {
            'copyBytes': copy_bytes,
            'backingBytes': backing_bytes,
            'otherPeakBytes': other_bytes,
        }
        observation = {
            'system': 'Linux',
            'machine': 'aarch64',
            'kernelRelease': '6.12.0-v8+',
            'effectiveUid': 0,
            'fixtureRootOwnerPrivate': True,
            'fixtureRootOnHostRoot': True,
            'rootMount': {
                'target': '/',
                'source': '/dev/nvme0n1p2',
                'filesystemType': 'ext4',
                'options': ['relatime', 'rw'],
                'uuid': 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
            },
            'rootSpace': {
                'availableBytes': 17 * cold.GIB // 2,
                'totalBytes': 80 * cold.GIB,
            },
            'fixtureOuterAllocatedBytes': backing_bytes + archive_bytes,
            'fixtureOuterEntryCount': 7,
            'fixtureOuterEntriesSha256': 'c' * 64,
            'backingPresent': True,
            'backingLogicalBytes': backing_bytes,
            'backingAllocatedBytes': backing_bytes,
            'sourceArchive': {
                'present': True,
                'sha256': 'd' * 64,
                'logicalBytes': archive_bytes,
                'allocatedBytes': archive_bytes,
            },
            'marker': self.marker,
            'markerPath': str(self.marker_path),
            'markerSha256': marker_sha256,
            'approval': self.approval,
            'approvalPath': str(self.approved_path),
            'approvalSha256': approved_sha256,
            'backingFileOwnerPrivate': True,
            'backingFileOnHostRoot': True,
            'mountpointOwnerPrivate': True,
            'fixtureMount': {
                'target': str(self.mount),
                'source': '/dev/loop7',
                'filesystemType': 'ext4',
                'options': ['relatime', 'rw'],
                'uuid': '12345678-1234-1234-1234-123456789abc',
            },
            'fixtureLoop': {
                'device': '/dev/loop7',
                'backingFile': str(self.backing),
                'offsetBytes': 0,
                'sizeLimitBytes': 0,
                'readOnly': False,
            },
            'mountDeviceMatchesLoop': True,
            'innerSpace': {
                'availableBytes': 5 * cold.GIB,
                'totalBytes': 5 * cold.GIB,
            },
        }
        return storage_admission.evaluate(
            'admit', self.marker, plan, observation, self.captured_at,
        )

    # produce one proof with a mocked host
    def produce(self, host=None):
        selected_host = FakeHost(self.marker) if host is None else host
        proof = cold.produce_proof(
            self.marker,
            self.storage,
            self.marker_path,
            self.target_volume,
            self.pgdata,
            self.policy,
            selected_host,
        )
        return proof, selected_host

    # accept one exact successful witness
    def test_produces_exact_v1_private_proof_shape(self):
        proof, host = self.produce()
        self.assertEqual(set(proof), {
            'completedAt', 'contractVersion', 'inner', 'outer', 'startedAt',
            'storageIdentity', 'target', 'volumes',
        })
        self.assertEqual(set(proof['inner']), {
            'completedAt', 'completedFiles', 'fadviseCompleted', 'fallbackUsed',
            'mincoreCompleted', 'startedAt', 'totalPages', 'totalResidentAfter',
        })
        self.assertEqual(set(proof['outer']), set(proof['inner']) | {'backingFileBytes'})
        self.assertEqual(set(proof['target']), {
            'fileCount', 'inventoryAfterSha256', 'inventoryBeforeSha256',
            'pgdataRoot', 'postmasterPidAbsent', 'totalBytes', 'volumeName',
        })
        self.assertEqual(proof['contractVersion'], cold.CONTRACT_VERSION)
        self.assertEqual(proof['inner']['startedAt'], proof['startedAt'])
        self.assertEqual(proof['outer']['startedAt'], proof['inner']['completedAt'])
        self.assertEqual(proof['outer']['completedAt'], proof['completedAt'])
        self.assertEqual(proof['target']['inventoryBeforeSha256'],
                         proof['target']['inventoryAfterSha256'])
        self.assertEqual(proof['inner']['completedFiles'], proof['target']['fileCount'])
        self.assertEqual(proof['inner']['totalResidentAfter'], 0)
        self.assertEqual(proof['outer']['totalResidentAfter'], 0)
        self.assertFalse(proof['inner']['fallbackUsed'])
        self.assertFalse(proof['outer']['fallbackUsed'])
        self.assertEqual(host.storage_observations, 2)
        self.assertEqual(host.evictions[-1], self.backing)
        self.assertEqual(
            [path for event, path in host.fixture_events if event == 'flush'],
            host.evictions,
        )
        self.assertEqual(host.fixture_events[-3:], [
            ('storage', self.storage['loopDevice']),
            ('flush', self.backing),
            ('evict', self.backing),
        ])

    # flush a validated descriptor before its eviction and observation
    def test_native_targeted_flush_precedes_fadvise_and_mincore(self):
        metadata = self.backing.stat()
        expected = cold.InventoryEntry(
            relative_path=self.backing.name,
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
        events = []
        with mock.patch.object(os, 'fsync', side_effect=lambda descriptor: events.append('fsync')), \
                mock.patch.object(
                    os,
                    'posix_fadvise',
                    side_effect=lambda *arguments: events.append('fadvise'),
                ), \
                mock.patch.object(
                    cold,
                    'mincore_residency',
                    side_effect=lambda *arguments: (events.append('mincore') or (1, 0)),
                ):
            result = cold.NativeHost().evict_and_measure(
                self.backing,
                expected,
                flush_before_eviction=True,
            )
        self.assertEqual(result, (1, 0))
        self.assertEqual(events, ['fsync', 'fadvise', 'mincore'])

    # stop the operation when its targeted flush fails
    def test_native_targeted_flush_failure_prevents_eviction_and_observation(self):
        metadata = self.backing.stat()
        expected = cold.InventoryEntry(
            relative_path=self.backing.name,
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
        with mock.patch.object(os, 'fsync', side_effect=OSError('flush failed')), \
                mock.patch.object(os, 'posix_fadvise') as fadvise, \
                mock.patch.object(cold, 'mincore_residency') as mincore, \
                self.assertRaisesRegex(OSError, 'flush failed'):
            cold.NativeHost().evict_and_measure(
                self.backing,
                expected,
                flush_before_eviction=True,
            )
        fadvise.assert_not_called()
        mincore.assert_not_called()

    # stop before outer work when an inner targeted flush fails
    def test_inner_flush_failure_prevents_outer_eviction(self):
        failed_path = self.pgdata / 'PG_VERSION'
        host = FakeHost(self.marker, flush_error_path=failed_path)
        with self.assertRaisesRegex(OSError, 'flush failed'):
            self.produce(host)
        self.assertEqual(host.fixture_events[-1], ('flush', failed_path))
        self.assertNotIn(failed_path, host.evictions)
        self.assertNotIn(self.backing, host.evictions)

    # reject a running volume before cache work
    def test_rejects_running_attachment_without_eviction(self):
        host = FakeHost(self.marker, attachments={self.volume_names[2]: ['container-id']})
        with self.assertRaisesRegex(RuntimeError, 'running attachment'):
            self.produce(host)
        self.assertEqual(host.evictions, [])

    # reject substituted Docker bind devices
    def test_rejects_substituted_volume_binding(self):
        host = BadVolumeHost(self.marker)
        with self.assertRaisesRegex(RuntimeError, 'binding is invalid'):
            self.produce(host)
        self.assertEqual(host.evictions, [])

    # reject residual non-target bytes
    def test_rejects_populated_non_target_slot(self):
        residual = self.volume_root / self.volume_names[1] / 'residual'
        residual.write_bytes(b'old fixture')
        host = FakeHost(self.marker)
        with self.assertRaisesRegex(RuntimeError, 'Non-target fixture volume is not empty'):
            self.produce(host)
        self.assertEqual(host.evictions, [])

    # reject PGDATA symbolic links
    def test_rejects_pgdata_symlink(self):
        (self.pgdata / 'linked').symlink_to(self.pgdata / 'PG_VERSION')
        host = FakeHost(self.marker)
        with self.assertRaisesRegex(RuntimeError, 'linked or special file'):
            self.produce(host)
        self.assertEqual(host.evictions, [])

    # reject a live PostgreSQL marker
    def test_rejects_postmaster_pid(self):
        (self.pgdata / 'postmaster.pid').write_text('123\n')
        host = FakeHost(self.marker)
        with self.assertRaisesRegex(RuntimeError, 'postmaster.pid'):
            self.produce(host)
        self.assertEqual(host.evictions, [])

    # reject inner page residency
    def test_residency_failure_emits_no_output(self):
        resident_path = self.pgdata / 'PG_VERSION'
        host = FakeHost(self.marker, residents={str(resident_path): 1})
        output = self.output_root / 'cold-proof.json'
        arguments = argparse.Namespace(
            fixture_marker=self.marker_path,
            storage_admission=self.admission_path,
            volume_name=self.volume_names,
            target_volume=self.target_volume,
            pgdata_root=self.pgdata,
            output=output,
        )
        with mock.patch.object(cold, 'storage_from_admission', return_value=self.storage), \
                self.assertRaisesRegex(RuntimeError, 'remains resident'):
            cold.execute(arguments, policy=self.policy, host=host)
        self.assertFalse(output.exists())
        self.assertNotIn(self.backing, host.evictions)

    # accept only the pinned actual admission
    def test_actual_admission_constructs_frozen_storage_identity(self):
        marker_sha256 = hashlib.sha256(self.marker_path.read_bytes()).hexdigest()
        self.assertEqual(set(self.admission), cold.ADMISSION_KEYS)
        self.assertEqual(self.admission['otherAllocatedBytes'], cold.GIB // 2)
        self.assertEqual(self.admission['otherRemainingBytes'], cold.GIB // 2)
        self.assertEqual(
            self.admission['rootAvailableBytes'],
            self.admission['rootAdmissionRequiredBytes'],
        )
        self.assertLess(
            self.admission['rootAvailableBytes'],
            self.admission['otherPeakBytes'] + self.admission['rootReserveBytes'],
        )
        storage = cold.storage_from_admission(
            self.admission,
            self.admission_path,
            self.marker,
            marker_sha256,
            self.volume_names,
            self.policy,
        )
        self.assertEqual(storage['loopDevice'], '/dev/loop7')
        self.assertEqual(storage['loopUuid'], '12345678-1234-1234-1234-123456789abc')
        self.assertEqual(storage['volumeNames'], self.volume_names)
        self.assertEqual(self.target_volume, f'{self.prefix}slot-01')
        self.assertEqual(self.admission['markerSha256'], marker_sha256)

    # reject estimate-only admission receipts
    def test_rejects_estimate_receipt_before_storage_observation(self):
        estimate = dict(self.admission)
        estimate['phase'] = 'preallocation-estimate'
        marker_sha256 = hashlib.sha256(self.marker_path.read_bytes()).hexdigest()
        with self.assertRaisesRegex(RuntimeError, 'phase'):
            cold.storage_from_admission(
                estimate,
                self.admission_path,
                self.marker,
                marker_sha256,
                self.volume_names,
                self.policy,
            )

    # reject renamed benchmark slots
    def test_rejects_noncanonical_volume_slot_names(self):
        changed = dict(self.storage)
        changed['volumeNames'] = list(self.volume_names)
        changed['volumeNames'][-1] = f'{self.prefix}slot-06'
        with self.assertRaisesRegex(RuntimeError, 'slot-01 through slot-05'):
            cold.validate_storage(changed, self.marker)

    # reject outer page residency
    def test_rejects_resident_outer_backing_file(self):
        host = FakeHost(self.marker, residents={str(self.backing): 1})
        with self.assertRaisesRegex(RuntimeError, 'backing file remains resident'):
            self.produce(host)
        self.assertEqual(host.evictions[-1], self.backing)

    # reject changed approval bytes
    def test_rejects_approval_changed_after_admission(self):
        self.write_private_json(self.approved_path, {**self.approval, 'approvalAuthority': 'other'})
        marker_sha256 = hashlib.sha256(self.marker_path.read_bytes()).hexdigest()
        with self.assertRaisesRegex(RuntimeError, 'changed after storage admission'):
            cold.storage_from_admission(
                self.admission,
                self.admission_path,
                self.marker,
                marker_sha256,
                self.volume_names,
                self.policy,
            )

    # reject inventory mutation after eviction
    def test_rejects_changed_stopped_inventory(self):
        changed = False

        # mutate only one inner file
        def mutate(path):
            nonlocal changed
            # change the selected file once
            if path == self.pgdata / 'PG_VERSION' and not changed:
                path.write_text('17\n')
                changed = True

        host = FakeHost(self.marker, mutate=mutate)
        with self.assertRaisesRegex(RuntimeError, 'inventory changed'):
            self.produce(host)
        self.assertNotIn(self.backing, host.evictions)

    # reject a marker outside the fixed root
    def test_rejects_marker_namespace_substitution(self):
        substituted = dict(self.marker)
        substituted['fixtureRoot'] = str(Path(self.temporary.name) / 'production')
        with self.assertRaisesRegex(RuntimeError, 'fixed native layout'):
            cold.validate_marker(substituted, self.marker_path, self.policy)

    # write one owner-private proof atomically
    def test_writes_private_proof_outside_loop_mount(self):
        proof, _ = self.produce()
        output = self.output_root / 'cold-proof.json'
        cold.write_private_proof(
            output, proof, self.owner_uid, self.output_root, self.mount,
        )
        self.assertEqual(output.stat().st_mode & 0o777, 0o600)
        self.assertEqual(json.loads(output.read_text()), proof)
        with self.assertRaisesRegex(RuntimeError, 'already exists'):
            cold.write_private_proof(
                output, proof, self.owner_uid, self.output_root, self.mount,
            )

    # reject output that would warm the loop again
    def test_rejects_output_inside_loop_mount(self):
        proof, _ = self.produce()
        output = self.mount / 'cold-proof.json'
        with self.assertRaisesRegex(RuntimeError, 'private runtime root'):
            cold.write_private_proof(
                output, proof, self.owner_uid, self.output_root, self.mount,
            )
        self.assertFalse(output.exists())


# run the focused regression suite
if __name__ == '__main__':
    unittest.main()
