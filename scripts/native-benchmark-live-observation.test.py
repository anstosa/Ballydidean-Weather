import importlib.util
import json
import os
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

SCRIPT = Path(__file__).with_name('native-benchmark-live-observation.py')
SPEC = importlib.util.spec_from_file_location('native_live_observation', SCRIPT)
observer = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(observer)


class LiveObservationTests(unittest.TestCase):
    # create one fixed installation-shaped observation fixture
    def setUp(self):
        self.ledger = b'0001_initial.sql|abcdef\n0002_next.sql|123456\n'
        self.expected = {
            'contractVersion': observer.EXPECTATION_CONTRACT,
            'release': '2026.11.04-6',
            'schemaRelease': '2026.11.03-3',
            'sourceRevision': 'a' * 40,
            'migrationLedgerSha256': observer.digest(self.ledger),
            'migrationCount': 2,
            'imageConfigIds': {},
        }
        self.environment = {
            'WEATHER_RELEASE': self.expected['release'],
            'WEATHER_SERVER_IMAGE': 'weather/server@sha256:' + '1' * 64,
            'WEATHER_WEB_IMAGE': 'weather/web@sha256:' + '2' * 64,
            'POSTGRES_IMAGE': 'postgres@sha256:' + '3' * 64,
            'CLOUDFLARED_IMAGE': 'cloudflared@sha256:' + '4' * 64,
            'WEATHER_CONTROL_PLANE_SHA256': '5' * 64,
            'WEATHER_CONTROL_PLANE_VERSION': '17',
        }
        self.containers = []
        self.images = {}
        self.files = {
            '/proc/sys/kernel/random/boot_id': b'a' * 8 + b'-aaaa-aaaa-aaaa-' + b'a' * 12,
            '/etc/machine-id': b'b' * 32 + b'\n',
            str(observer.ROOT / 'deploy/state/current-release'): b'2026.11.04-6\n',
            str(observer.ROOT / 'deploy/state/schema-release'): b'2026.11.03-3\n',
            str(observer.ROOT / 'deploy/releases/2026.11.04-6.env'): (
                ''.join(f'{key}={value}\n' for key, value in self.environment.items())
                + 'UNRELATED_VALUE=never-emit-this\n'
            ).encode(),
        }
        # set up each fixed service with separate cgroup and process identity
        for index, (role, (name, key)) in enumerate(observer.ROLES.items(), 1):
            image_id = 'sha256:' + str(1 if role == 'worker' else index) * 64
            # retain the same application image for API and worker
            if role in ('api', 'worker'):
                image_id = 'sha256:' + '6' * 64
            self.expected['imageConfigIds'][role] = image_id
            container_id = str(index) * 64
            self.containers.append({
                'Name': '/' + name, 'Id': container_id, 'Image': image_id,
                'Config': {'Image': self.environment[key],
                           'Labels': {'com.docker.compose.project': 'weather'}},
                'State': {'Pid': index, 'Running': True, 'Health': {'Status': 'healthy'},
                          'StartedAt': '2026-11-27T00:00:00Z'},
                'HostConfig': dict.fromkeys(observer.LIMIT_KEYS, 0),
            })
            self.images[image_id] = {
                'Id': image_id, 'Architecture': 'arm64', 'Os': 'linux',
                'RepoDigests': [self.environment[key]],
                'Config': {'Labels': {'org.opencontainers.image.revision': 'a' * 40}},
            }
            self.files[f'/proc/{index}/cgroup'] = f'0::/docker/{container_id}.scope\n'.encode()
            self.files[f'/proc/{index}/stat'] = (
                f'{index} (process) ' + ' '.join(['0'] * 19 + ['12345'])
            ).encode()
        self.commands = []
        self.inspect_count = 0

    # return only the exact fixed read-only command responses
    def run_command(self, *args):
        self.commands.append(args)
        if args[1] == 'inspect':
            self.inspect_count += 1
            return json.dumps(self.containers).encode()
        if args[1:3] == ('image', 'inspect'):
            return json.dumps([self.images[args[3]]]).encode()
        if args[1] == 'exec':
            self.assertEqual(args[-1], observer.LEDGER_QUERY)
            return self.ledger
        if args[1] == 'version':
            return b'26.1.5\n'
        if args[1] == 'info':
            return b'2\n'
        self.fail(f'unexpected command {args}')

    # read the bounded installation fixture
    def read(self, path):
        return self.files[str(path)]

    # retain one native host identity
    def host(self):
        return SimpleNamespace(nodename='blueberry', machine='aarch64', release='6.12.75')

    # invoke the shared observation function using isolated inputs
    def collect(self):
        return observer.collect(
            self.expected, run=self.run_command, read=self.read, host=self.host,
            clock=lambda: '2026-11-27T00:00:00+00:00',
        )

    # prove the actual read-only query and stable service identity shape
    def test_collect_exact_observation(self):
        result = self.collect()
        self.assertEqual(result['contractVersion'], observer.CONTRACT)
        self.assertEqual(result['migrationLedgerSha256'], self.expected['migrationLedgerSha256'])
        self.assertEqual(set(result['containers']), set(observer.ROLES))
        self.assertEqual(result['releaseImages'], self.environment)
        self.assertTrue(result['allIdentitiesStable'])
        self.assertNotIn('never-emit-this', json.dumps(result))
        self.assertEqual(self.inspect_count, 2)
        self.assertEqual(sum(args[1] == 'exec' for args in self.commands), 1)

    # reject a changed installed image or migration ledger
    def test_reject_image_or_ledger_drift(self):
        self.containers[0]['Image'] = 'sha256:' + '9' * 64
        with self.assertRaisesRegex(observer.ObservationError, 'identity'):
            self.collect()
        self.containers[0]['Image'] = self.expected['imageConfigIds']['web']
        self.ledger += b'0003_extra.sql|123\n'
        with self.assertRaisesRegex(observer.ObservationError, 'ledger'):
            self.collect()

    # reject changed process identity during the same observation
    def test_reject_process_drift(self):
        original = self.run_command

        # change the second process inspection only
        def changed(*args):
            result = original(*args)
            if args[1] == 'inspect' and self.inspect_count == 2:
                self.files['/proc/1/stat'] = self.files['/proc/1/stat'].replace(b'12345', b'45678')
            return result

        with self.assertRaisesRegex(observer.ObservationError, 'changed'):
            observer.collect(self.expected, run=changed, read=self.read, host=self.host)

    # reject an ambiguous selected release identity
    def test_reject_duplicate_environment_entry(self):
        path = str(observer.ROOT / 'deploy/releases/2026.11.04-6.env')
        self.files[path] += b'WEATHER_RELEASE=2026.11.04-6\n'
        with self.assertRaisesRegex(observer.ObservationError, 'repeats'):
            self.collect()

    # bind a private exact expectation file to its digest
    def test_expectation_file_custody(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'expected.json'
            data = json.dumps(self.expected).encode()
            path.write_bytes(data)
            os.chmod(path, 0o600)
            self.assertEqual(observer.read_expectation(path, observer.digest(data)), self.expected)
            with self.assertRaisesRegex(observer.ObservationError, 'digest'):
                observer.read_expectation(path, '0' * 64)


if __name__ == '__main__':
    unittest.main()
