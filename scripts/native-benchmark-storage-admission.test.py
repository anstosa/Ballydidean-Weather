import importlib.util
import json
import os
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

SCRIPT = Path(__file__).with_name('native-benchmark-storage-admission.py')
SPEC = importlib.util.spec_from_file_location('native_benchmark_storage_admission', SCRIPT)
admission = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(admission)
GIB = admission.GIB
FIXTURE_ID = 'rust-rewrite-0123456789abcdef'
NOW = datetime(2026, 11, 27, 12, 0, 0, tzinfo=timezone.utc)
PLAN = {
    'copyBytes': 14685855744,
    'backingBytes': 19864223744,
    'otherPeakBytes': 1610612736,
}


# build one exact approval
def make_approval(contract, **changes):
    approval = {
        'schemaVersion': 1,
        'kind': 'weather-native-rust-rewrite-storage-approval',
        'approvalAuthority': 'ansel',
        'fixtureId': contract['fixtureId'],
        'fixtureRoot': contract['fixtureRoot'],
        'copyBytes': PLAN['copyBytes'],
        'backingBytes': PLAN['backingBytes'],
        'otherPeakBytes': PLAN['otherPeakBytes'],
        'maxAdditionalBytes': 20 * GIB,
        'rootReserveBytes': 8 * GIB,
        'innerReserveBytes': 4 * GIB,
        'retentionSeconds': 86400,
        'rootFilesystemSource': '/dev/mmcblk0p2',
        'rootFilesystemUuid': '11111111-2222-3333-4444-555555555555',
        'approvedAt': '2026-11-27T00:00:00Z',
        'expiresAt': '2026-11-28T00:00:00Z',
    }
    approval.update(changes)
    return approval


# build controlled os evidence
def make_observation(actual=False, **changes):
    contract = admission.fixture_contract(FIXTURE_ID)
    archive_allocated = 5 * GIB // 4
    other_remaining = PLAN['otherPeakBytes'] - archive_allocated
    observation = {
        'system': 'Linux',
        'machine': 'aarch64',
        'kernelRelease': '6.12.0-v8+',
        'effectiveUid': 0,
        'fixtureRootOwnerPrivate': True,
        'fixtureRootOnHostRoot': True,
        'rootMount': {
            'target': '/',
            'source': '/dev/mmcblk0p2',
            'filesystemType': 'ext4',
            'options': ['relatime', 'rw'],
            'uuid': '11111111-2222-3333-4444-555555555555',
        },
        'rootSpace': {
            'availableBytes': PLAN['backingBytes'] + other_remaining + 8 * GIB,
            'totalBytes': 64 * GIB,
        },
        'fixtureOuterAllocatedBytes': archive_allocated,
        'fixtureOuterEntryCount': 4,
        'fixtureOuterEntriesSha256': 'c' * 64,
        'backingPresent': False,
        'backingLogicalBytes': 0,
        'backingAllocatedBytes': 0,
        'sourceArchive': {
            'present': True,
            'sha256': 'd' * 64,
            'logicalBytes': archive_allocated,
            'allocatedBytes': archive_allocated,
        },
        'marker': contract.copy(),
        'markerPath': f"{contract['fixtureRoot']}/.fixture-marker.json",
        'markerSha256': 'a' * 64,
        'approval': make_approval(contract),
        'approvalPath': f"{contract['fixtureRoot']}/.approved-cap.json",
        'approvalSha256': 'b' * 64,
    }
    # add mounted evidence
    if actual:
        observation.update({
            'backingFileOwnerPrivate': True,
            'backingFileOnHostRoot': True,
            'backingPresent': True,
            'backingLogicalBytes': PLAN['backingBytes'],
            'backingAllocatedBytes': PLAN['backingBytes'],
            'fixtureOuterAllocatedBytes': PLAN['backingBytes'] + archive_allocated,
            'mountpointOwnerPrivate': True,
            'fixtureMount': {
                'target': contract['mountpoint'],
                'source': '/dev/loop7',
                'filesystemType': 'ext4',
                'options': ['relatime', 'rw'],
                'uuid': 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
            },
            'fixtureLoop': {
                'device': '/dev/loop7',
                'backingFile': contract['backingFile'],
                'offsetBytes': 0,
                'sizeLimitBytes': 0,
                'readOnly': False,
            },
            'mountDeviceMatchesLoop': True,
            'innerSpace': {
                'availableBytes': PLAN['copyBytes'] + 4 * GIB,
                'totalBytes': PLAN['backingBytes'] - 1024 * 1024,
            },
            'rootSpace': {
                'availableBytes': other_remaining + 8 * GIB,
                'totalBytes': 64 * GIB,
            },
        })
    observation.update(changes)
    return contract, observation


class NativeBenchmarkStorageAdmissionTest(unittest.TestCase):
    # verify the fixed namespace
    def test_fixture_contract_is_narrow_and_production_disjoint(self):
        contract = admission.fixture_contract(FIXTURE_ID)
        self.assertEqual(
            contract['fixtureRoot'],
            '/var/lib/weather-native-benchmark/rust-rewrite-0123456789abcdef',
        )
        self.assertEqual(contract['volumeRoot'], f"{contract['mountpoint']}/docker-volumes")
        self.assertEqual(
            contract['volumeNamespacePrefix'],
            'weather-native-benchmark-rust-rewrite-0123456789abcdef-',
        )
        self.assertNotIn('/var/lib/weather/postgres', json.dumps(contract))
        with self.assertRaisesRegex(ValueError, '16 lowercase hex'):
            admission.fixture_contract('rust-rewrite-latest')

    # verify estimate semantics
    def test_estimate_passes_without_authorizing_allocation_or_use(self):
        contract, observation = make_observation()
        receipt = admission.evaluate('estimate', contract, PLAN, observation, NOW)
        self.assertTrue(receipt['estimatePassed'])
        self.assertFalse(receipt['allocationAuthorized'])
        self.assertFalse(receipt['fixtureUseAuthorized'])
        self.assertFalse(receipt['admitted'])
        self.assertTrue(receipt['actualAdmissionRequired'])
        self.assertEqual(receipt['phase'], 'preallocation-estimate')
        self.assertEqual(
            receipt['preallocationRequiredBytes'],
            PLAN['backingBytes'] + PLAN['otherPeakBytes'] - 5 * GIB // 4 + 8 * GIB,
        )
        self.assertEqual(receipt['otherAllocatedBytes'], 5 * GIB // 4)
        self.assertEqual(receipt['unmeasuredOtherReserveBytes'], GIB // 4)
        self.assertEqual(receipt['externalCleanupCreditBytes'], 0)

    # verify outer reserve accounting
    def test_estimate_requires_peak_plus_root_reserve(self):
        contract, observation = make_observation()
        observation['rootSpace']['availableBytes'] -= 1
        receipt = admission.evaluate('estimate', contract, PLAN, observation, NOW)
        self.assertFalse(receipt['estimatePassed'])
        self.assertFalse(receipt['admitted'])

    # verify precapture estimate
    def test_estimate_allows_missing_archive_only_before_backing(self):
        contract, observation = make_observation()
        observation['fixtureOuterAllocatedBytes'] = 0
        observation['sourceArchive'] = {
            'present': False,
            'sha256': None,
            'logicalBytes': 0,
            'allocatedBytes': 0,
        }
        observation['rootSpace']['availableBytes'] = 28 * GIB
        receipt = admission.evaluate('estimate', contract, PLAN, observation, NOW)
        self.assertTrue(receipt['estimatePassed'])
        self.assertFalse(receipt['sourceArchivePresent'])
        self.assertEqual(receipt['rootAdmissionRequiredBytes'], 28 * GIB)

    # verify capture ordering
    def test_backing_cannot_precede_verified_archive(self):
        contract, observation = make_observation(actual=True)
        observation['sourceArchive'] = {
            'present': False,
            'sha256': None,
            'logicalBytes': 0,
            'allocatedBytes': 0,
        }
        with self.assertRaisesRegex(RuntimeError, 'cannot precede'):
            admission.evaluate('estimate', contract, PLAN, observation, NOW)

    # verify other allocation cap
    def test_fixture_other_allocations_cannot_exceed_other_peak(self):
        contract, observation = make_observation()
        observation['fixtureOuterAllocatedBytes'] = PLAN['otherPeakBytes'] + 1
        with self.assertRaisesRegex(RuntimeError, 'otherPeakBytes'):
            admission.evaluate('estimate', contract, PLAN, observation, NOW)

    # verify complete admission
    def test_actual_admission_binds_loop_and_real_inner_headroom(self):
        contract, observation = make_observation(actual=True)
        receipt = admission.evaluate('admit', contract, PLAN, observation, NOW)
        self.assertTrue(receipt['admitted'])
        self.assertTrue(receipt['fixtureUseAuthorized'])
        self.assertFalse(receipt['allocationAuthorized'])
        self.assertEqual(receipt['fixtureFilesystem']['source'], '/dev/loop7')
        self.assertEqual(receipt['loopBinding']['backingFile'], contract['backingFile'])
        self.assertEqual(receipt['backingAllocatedBytes'], PLAN['backingBytes'])
        self.assertEqual(receipt['actualAdditionalPeakBytes'], 20 * GIB)
        self.assertEqual(receipt['markerSha256'], 'a' * 64)
        self.assertEqual(receipt['approvedCapSha256'], 'b' * 64)

    # verify exact actual schema
    def test_actual_receipt_has_one_pinned_handoff_schema(self):
        contract, observation = make_observation(actual=True)
        receipt = admission.evaluate('admit', contract, PLAN, observation, NOW)
        self.assertEqual(set(receipt), {
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
        })

    # verify inner headroom
    def test_actual_admission_denies_one_byte_short_inner_space(self):
        contract, observation = make_observation(actual=True)
        observation['innerSpace']['availableBytes'] = PLAN['copyBytes'] + 4 * GIB - 1
        receipt = admission.evaluate('admit', contract, PLAN, observation, NOW)
        self.assertFalse(receipt['admitted'])
        self.assertFalse(receipt['fixtureUseAuthorized'])

    # verify physical preallocation
    def test_actual_admission_denies_sparse_backing(self):
        contract, observation = make_observation(actual=True)
        observation['backingAllocatedBytes'] = PLAN['backingBytes'] - 512
        with self.assertRaisesRegex(RuntimeError, 'fully allocated plan'):
            admission.evaluate('admit', contract, PLAN, observation, NOW)

    # verify postallocation reserve
    def test_actual_admission_reserves_remaining_peak_on_root(self):
        contract, observation = make_observation(actual=True)
        observation['rootSpace']['availableBytes'] -= 1
        receipt = admission.evaluate('admit', contract, PLAN, observation, NOW)
        self.assertFalse(receipt['admitted'])

    # verify actual cap
    def test_actual_admission_denies_allocated_bytes_beyond_cap(self):
        contract, observation = make_observation(actual=True)
        observation['backingAllocatedBytes'] = PLAN['backingBytes'] + 512
        observation['fixtureOuterAllocatedBytes'] += 512
        with self.assertRaisesRegex(RuntimeError, '20 GiB cap'):
            admission.evaluate('admit', contract, PLAN, observation, NOW)

    # verify native host requirement
    def test_host_rejects_x86_and_wsl_evidence(self):
        contract, observation = make_observation(machine='x86_64')
        with self.assertRaisesRegex(RuntimeError, 'Linux/aarch64'):
            admission.evaluate('estimate', contract, PLAN, observation, NOW)
        contract, observation = make_observation(kernelRelease='5.15.0-microsoft-standard-WSL2')
        with self.assertRaisesRegex(RuntimeError, 'WSL'):
            admission.evaluate('estimate', contract, PLAN, observation, NOW)

    # verify root identity binding
    def test_host_rejects_wrong_root_ext4_identity(self):
        contract, observation = make_observation()
        observation['rootMount']['uuid'] = 'ffffffff-ffff-ffff-ffff-ffffffffffff'
        with self.assertRaisesRegex(RuntimeError, 'approved-cap'):
            admission.evaluate('estimate', contract, PLAN, observation, NOW)

    # verify marker exactness
    def test_marker_rejects_extra_or_changed_fields(self):
        contract, observation = make_observation()
        observation['marker']['unexpected'] = True
        with self.assertRaisesRegex(RuntimeError, 'schema'):
            admission.evaluate('estimate', contract, PLAN, observation, NOW)
        contract, observation = make_observation()
        observation['marker']['mountpoint'] = '/var/lib/weather/postgres'
        with self.assertRaisesRegex(RuntimeError, 'exact fixture'):
            admission.evaluate('estimate', contract, PLAN, observation, NOW)

    # verify cap approval lifetime
    def test_approval_requires_exact_current_24_hour_window(self):
        contract, observation = make_observation()
        observation['approval']['expiresAt'] = '2026-11-27T23:59:59Z'
        with self.assertRaisesRegex(RuntimeError, 'exactly 24 hours'):
            admission.evaluate('estimate', contract, PLAN, observation, NOW)
        contract, observation = make_observation()
        with self.assertRaisesRegex(RuntimeError, 'currently valid'):
            admission.evaluate(
                'estimate', contract, PLAN, observation,
                datetime(2026, 11, 28, 0, 0, 0, tzinfo=timezone.utc),
            )

    # verify total requested cap
    def test_plan_rejects_more_than_20_gib_or_less_than_inner_need(self):
        contract, observation = make_observation()
        oversized = {**PLAN, 'otherPeakBytes': PLAN['otherPeakBytes'] + 1}
        observation['approval'] = make_approval(contract, otherPeakBytes=oversized['otherPeakBytes'])
        with self.assertRaisesRegex(RuntimeError, '20 GiB'):
            admission.evaluate('estimate', contract, oversized, observation, NOW)
        undersized = {**PLAN, 'backingBytes': PLAN['copyBytes'] + 4 * GIB - 1}
        observation['approval'] = make_approval(contract, backingBytes=undersized['backingBytes'])
        with self.assertRaisesRegex(RuntimeError, '4 GiB'):
            admission.evaluate('estimate', contract, undersized, observation, NOW)

    # verify mount parser
    def test_mount_parser_rejects_nonloop_or_readonly_fixture(self):
        payload = json.dumps({'filesystems': [{
            'target': '/var/lib/weather-native-benchmark/rust-rewrite-0123456789abcdef/mnt',
            'source': '/dev/mmcblk0p2',
            'fstype': 'ext4',
            'options': 'rw,relatime',
            'uuid': 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
        }]})
        with self.assertRaisesRegex(RuntimeError, 'loop device'):
            admission.mount_record(payload, Path(json.loads(payload)['filesystems'][0]['target']), True)
        payload = payload.replace('rw,relatime', 'ro,relatime').replace('/dev/mmcblk0p2', '/dev/loop7')
        with self.assertRaisesRegex(RuntimeError, 'writable ext4'):
            admission.mount_record(payload, Path(json.loads(payload)['filesystems'][0]['target']), True)

    # verify loop parser
    def test_loop_parser_requires_exact_whole_writable_backing(self):
        contract = admission.fixture_contract(FIXTURE_ID)
        payload = json.dumps({'loopdevices': [{
            'name': '/dev/loop7',
            'back-file': contract['backingFile'],
            'offset': 0,
            'sizelimit': 0,
            'ro': False,
        }]})
        parsed = admission.loop_record(
            payload, '/dev/loop7', Path(contract['backingFile']), PLAN['backingBytes'],
        )
        self.assertEqual(parsed['backingFile'], contract['backingFile'])
        with self.assertRaisesRegex(RuntimeError, 'complete backing'):
            admission.loop_record(
                payload.replace('"offset": 0', '"offset": 4096'),
                '/dev/loop7', Path(contract['backingFile']), PLAN['backingBytes'],
            )

    # verify blueberry-compatible probe argv
    def test_loop_probe_uses_supported_util_linux_241_arguments(self):
        self.assertEqual(admission.loop_probe_command('/dev/loop7'), [
            '/usr/sbin/losetup', '--json', '--list',
            '--output', 'NAME,BACK-FILE,OFFSET,SIZELIMIT,RO', '/dev/loop7',
        ])
        self.assertNotIn('--bytes', admission.loop_probe_command('/dev/loop7'))
        with self.assertRaisesRegex(ValueError, 'exact loop device'):
            admission.loop_probe_command('/dev/mmcblk0p2')

    # verify allocated tree parsing
    def test_allocated_tree_parser_requires_exact_root(self):
        root = Path('/var/lib/weather-native-benchmark') / FIXTURE_ID
        self.assertEqual(admission.allocated_tree_bytes(f'4096\t{root}', root), 4096)
        with self.assertRaisesRegex(RuntimeError, 'measurement'):
            admission.allocated_tree_bytes('4096\t/var/lib/weather/postgres', root)

    # verify fixed archive pair
    def test_source_archive_pair_is_hashed_and_capped(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / 'source-fixture.dump.age'
            checksum = root / 'source-fixture.dump.age.sha256'
            archive.write_bytes(b'encrypted-fixture')
            archive.chmod(0o600)
            digest = hashlib_sha256(archive.read_bytes())
            checksum.write_text(f'{digest}  source-fixture.dump.age\n', encoding='ascii')
            checksum.chmod(0o600)
            observation = admission.source_archive_observation(root, os.getuid())
            self.assertTrue(observation['present'])
            self.assertEqual(observation['sha256'], digest)
            checksum.write_text(f'{"0" * 64}  source-fixture.dump.age\n', encoding='ascii')
            with self.assertRaisesRegex(RuntimeError, 'does not match'):
                admission.source_archive_observation(root, os.getuid())

    # verify outer allowlist
    def test_outer_tree_rejects_unlisted_or_nonprivate_entries(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            root.chmod(0o700)
            mountpoint = root / 'mnt'
            mountpoint.mkdir(mode=0o700)
            marker = root / '.fixture-marker.json'
            marker.write_text('{}', encoding='utf-8')
            marker.chmod(0o600)
            inventory = admission.validate_outer_tree(root, mountpoint, os.getuid())
            self.assertEqual(inventory['entryCount'], 2)
            unexpected = root / 'plaintext.dump'
            unexpected.write_bytes(b'forbidden')
            unexpected.chmod(0o600)
            with self.assertRaisesRegex(RuntimeError, 'Unexpected fixture-root entry'):
                admission.validate_outer_tree(root, mountpoint, os.getuid())

    # verify private input reads
    def test_private_json_reader_rejects_links_and_broad_permissions(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            private = root / 'private.json'
            private.write_text('{"ok":true}', encoding='utf-8')
            private.chmod(0o600)
            payload, digest = admission.read_private_json(private, os.getuid())
            self.assertEqual(payload, {'ok': True})
            self.assertEqual(digest, hashlib_sha256(private.read_bytes()))
            private.chmod(0o644)
            with self.assertRaisesRegex(RuntimeError, 'owner-private'):
                admission.read_private_json(private, os.getuid())
            link = root / 'link.json'
            link.symlink_to(private)
            with self.assertRaisesRegex(RuntimeError, 'owner-private'):
                admission.read_private_json(link, os.getuid())


# hash one tiny fixture
def hashlib_sha256(value):
    import hashlib
    return hashlib.sha256(value).hexdigest()


# run focused tests
if __name__ == '__main__':
    unittest.main()
