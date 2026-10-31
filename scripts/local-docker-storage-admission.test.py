import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

path = Path(__file__).with_name('local-docker-storage-admission.py')
spec = importlib.util.spec_from_file_location('storage_admission', path)
admission = importlib.util.module_from_spec(spec)
spec.loader.exec_module(admission)
GIB = admission.GIB


class StorageAdmissionRegression(unittest.TestCase):
    def test_virtual_free_space_cannot_authorize_copies_that_fill_windows(self):
        required, allowed = admission.evaluate(15 * GIB, 5, 32 * GIB, 213 * GIB, 43 * GIB, True)
        self.assertEqual(required, 107 * GIB)
        self.assertFalse(allowed)

    def test_wsl_host_measurement_must_succeed(self):
        with self.assertRaisesRegex(ValueError, 'Windows backing-drive'):
            admission.evaluate(15 * GIB, 5, 32 * GIB, 213 * GIB, None, True)

    def test_both_filesystems_must_have_enough_space(self):
        self.assertFalse(admission.evaluate(15 * GIB, 5, 32 * GIB, 106 * GIB, 200 * GIB, True)[1])
        self.assertFalse(admission.evaluate(15 * GIB, 5, 32 * GIB, 200 * GIB, 107 * GIB - 1, True)[1])
        self.assertTrue(admission.evaluate(15 * GIB, 5, 32 * GIB, 107 * GIB, 107 * GIB, True)[1])

    def test_native_linux_does_not_require_windows_probe(self):
        self.assertTrue(admission.evaluate(15 * GIB, 5, 32 * GIB, 213 * GIB)[1])

    def test_target_mount_requires_exact_writable_ext4_loop_mount(self):
        # accept the bounded target shape
        payload = json.dumps({'filesystems': [{
            'target': '/mnt/weather-bench',
            'source': '/dev/loop7',
            'fstype': 'ext4',
            'options': 'rw,relatime',
            'uuid': '12345678-1234-1234-1234-123456789abc',
        }]})
        self.assertEqual(
            admission.target_mount_record(payload, Path('/mnt/weather-bench')),
            ('/dev/loop7', '12345678-1234-1234-1234-123456789abc'),
        )

    def test_target_mount_rejects_inherited_mount(self):
        # reject the default filesystem
        payload = json.dumps({'filesystems': [{
            'target': '/',
            'source': '/dev/sdc',
            'fstype': 'ext4',
            'options': 'rw,relatime',
            'uuid': '12345678-1234-1234-1234-123456789abc',
        }]})
        with self.assertRaisesRegex(RuntimeError, 'mountpoint'):
            admission.target_mount_record(payload, Path('/mnt/weather-bench'))

    def test_target_mount_rejects_non_loop_device(self):
        # reject a physical device
        payload = json.dumps({'filesystems': [{
            'target': '/mnt/weather-bench',
            'source': '/dev/sdc',
            'fstype': 'ext4',
            'options': 'rw,relatime',
            'uuid': '12345678-1234-1234-1234-123456789abc',
        }]})
        with self.assertRaisesRegex(RuntimeError, 'loop device'):
            admission.target_mount_record(payload, Path('/mnt/weather-bench'))

    def test_target_mount_rejects_read_only_ext4(self):
        # reject incomplete proof
        payload = json.dumps({'filesystems': [{
            'target': '/mnt/weather-bench',
            'source': '/dev/loop7',
            'fstype': 'ext4',
            'options': 'ro,relatime',
            'uuid': None,
        }]})
        with self.assertRaisesRegex(RuntimeError, 'writable ext4'):
            admission.target_mount_record(payload, Path('/mnt/weather-bench'))

    def test_target_mount_rejects_missing_uuid(self):
        # reject unstable identity
        payload = json.dumps({'filesystems': [{
            'target': '/mnt/weather-bench',
            'source': '/dev/loop7',
            'fstype': 'ext4',
            'options': 'rw,relatime',
            'uuid': None,
        }]})
        with self.assertRaisesRegex(RuntimeError, 'UUID'):
            admission.target_mount_record(payload, Path('/mnt/weather-bench'))

    def test_loop_binding_requires_the_exact_device_and_absolute_backing_file(self):
        # accept the exact loop binding
        payload = json.dumps({'loopdevices': [{
            'name': '/dev/loop7',
            'back-file': '/mnt/d/weather/bench.ext4',
        }]})
        self.assertEqual(
            admission.loop_backing_file(payload, '/dev/loop7'),
            Path('/mnt/d/weather/bench.ext4'),
        )
        # reject changed identity
        with self.assertRaisesRegex(RuntimeError, 'identity changed'):
            admission.loop_backing_file(payload, '/dev/loop8')

    def test_backing_mount_derives_the_verified_windows_drive(self):
        # derive capacity scope
        payload = json.dumps({'filesystems': [{
            'target': '/mnt/d',
            'source': 'D:\\',
            'fstype': '9p',
            'options': 'rw,aname=drvfs;path=D:\\',
            'uuid': None,
        }]})
        self.assertEqual(
            admission.backing_mount_record(payload, Path('/mnt/d/weather/bench.ext4')),
            (Path('/mnt/d'), 'D:\\'),
        )

    def test_backing_mount_rejects_non_drvfs_or_unrelated_paths(self):
        # reject an unproven backing filesystem
        payload = json.dumps({'filesystems': [{
            'target': '/mnt/d',
            'source': '/dev/sdd',
            'fstype': 'ext4',
            'options': 'rw,relatime',
            'uuid': '1234',
        }]})
        with self.assertRaisesRegex(RuntimeError, 'writable drvfs'):
            admission.backing_mount_record(payload, Path('/mnt/d/weather/bench.ext4'))
        # reject an unrelated path
        payload = json.dumps({'filesystems': [{
            'target': '/mnt/e',
            'source': 'E:\\',
            'fstype': '9p',
            'options': 'rw,aname=drvfs;path=E:\\',
            'uuid': None,
        }]})
        with self.assertRaisesRegex(RuntimeError, 'outside'):
            admission.backing_mount_record(payload, Path('/mnt/d/weather/bench.ext4'))

    def test_windows_drive_measurement_requires_ready_matching_physical_capacity(self):
        # accept measured physical capacity
        measurement = {
            'backingDrive': 'D:\\',
            'isReady': True,
            'availableBytes': 600 * GIB,
            'totalBytes': 900 * GIB,
        }
        self.assertIs(admission.validated_windows_drive(measurement, 'D:\\'), measurement)
        # reject caller-like fake capacity
        measurement['availableBytes'] = 901 * GIB
        with self.assertRaisesRegex(RuntimeError, 'capacity measurement'):
            admission.validated_windows_drive(measurement, 'D:\\')

    def test_default_wsl_mapping_still_fails_without_distribution_identity(self):
        # preserve default fail-closed behavior
        with self.assertRaisesRegex(RuntimeError, 'distribution name'):
            admission.windows_backing_space('')

    def test_default_source_volume_still_measures_the_docker_mountpoint(self):
        # preserve physical source mapping
        with tempfile.TemporaryDirectory() as directory:
            docker_root = Path(directory) / 'docker'
            mount = docker_root / 'volumes' / 'source-db' / '_data'
            mount.mkdir(parents=True)
            source = {
                'Name': 'source-db',
                'Driver': 'local',
                'Mountpoint': str(mount),
                'Options': None,
            }
            path, evidence = admission.source_measurement_path(source, 'source-db', docker_root)
            self.assertEqual(path, mount)
            self.assertEqual(evidence, {'sourceStorageType': 'docker-volume'})

    def test_exact_local_bind_measures_the_device_under_the_verified_target(self):
        # accept the bounded bind mapping
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            docker_root = root / 'docker'
            mount = docker_root / 'volumes' / 'source-db' / '_data'
            target_root = root / 'target'
            device = target_root / 'source-db'
            mount.mkdir(parents=True)
            device.mkdir(parents=True)
            source = {
                'Name': 'source-db',
                'Driver': 'local',
                'Mountpoint': str(mount),
                'Options': {'type': 'none', 'o': 'bind', 'device': str(device)},
            }
            path, evidence = admission.source_measurement_path(
                source, 'source-db', docker_root, target_root,
            )
            self.assertEqual(path, device)
            self.assertEqual(evidence['sourceStorageType'], 'local-bind')
            self.assertRegex(evidence['sourceBindDeviceSha256'], r'^[a-f0-9]{64}$')

    def test_local_bind_rejects_arbitrary_options_and_missing_target_proof(self):
        # reject an expanded option surface
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            docker_root = root / 'docker'
            mount = docker_root / 'volumes' / 'source-db' / '_data'
            device = root / 'target' / 'source-db'
            mount.mkdir(parents=True)
            device.mkdir(parents=True)
            source = {
                'Name': 'source-db',
                'Driver': 'local',
                'Mountpoint': str(mount),
                'Options': {'type': 'none', 'o': 'bind', 'device': str(device), 'extra': 'denied'},
            }
            with self.assertRaisesRegex(RuntimeError, 'options'):
                admission.source_measurement_path(source, 'source-db', docker_root, root / 'target')
            # reject missing target proof
            source['Options'].pop('extra')
            with self.assertRaisesRegex(RuntimeError, 'verified target'):
                admission.source_measurement_path(source, 'source-db', docker_root)

    def test_local_bind_rejects_symlink_traversal_and_outside_paths(self):
        # reject ambiguous device paths
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            docker_root = root / 'docker'
            mount = docker_root / 'volumes' / 'source-db' / '_data'
            target_root = root / 'target'
            device = target_root / 'source-db'
            nested = target_root / 'nested'
            outside = root / 'outside'
            mount.mkdir(parents=True)
            device.mkdir(parents=True)
            nested.mkdir()
            outside.mkdir()
            link = target_root / 'linked-source'
            link.symlink_to(device, target_is_directory=True)
            source = {
                'Name': 'source-db',
                'Driver': 'local',
                'Mountpoint': str(mount),
                'Options': {'type': 'none', 'o': 'bind', 'device': str(link)},
            }
            with self.assertRaisesRegex(RuntimeError, 'without symlinks'):
                admission.source_measurement_path(source, 'source-db', docker_root, target_root)
            # reject lexical traversal
            source['Options']['device'] = str(nested / '..' / 'source-db')
            with self.assertRaisesRegex(RuntimeError, 'without symlinks'):
                admission.source_measurement_path(source, 'source-db', docker_root, target_root)
            # reject paths outside target
            source['Options']['device'] = str(outside)
            with self.assertRaisesRegex(RuntimeError, 'outside'):
                admission.source_measurement_path(source, 'source-db', docker_root, target_root)

    def test_local_bind_rejects_a_cross_filesystem_source(self):
        # reject a nested foreign filesystem
        with tempfile.TemporaryDirectory() as directory:
            docker_root = Path(directory) / 'docker'
            mount = docker_root / 'volumes' / 'source-db' / '_data'
            mount.mkdir(parents=True)
            source = {
                'Name': 'source-db',
                'Driver': 'local',
                'Mountpoint': str(mount),
                'Options': {'type': 'none', 'o': 'bind', 'device': '/proc'},
            }
            with self.assertRaisesRegex(RuntimeError, 'crosses'):
                admission.source_measurement_path(source, 'source-db', docker_root, Path('/'))


if __name__ == '__main__':
    unittest.main()
