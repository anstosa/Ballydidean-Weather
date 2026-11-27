import hashlib
import importlib.util
import json
import os
import stat
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace

SCRIPT = Path(__file__).with_name('native-benchmark-service-resources.py')
SPEC = importlib.util.spec_from_file_location('native_benchmark_service_resources', SCRIPT)
resources = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(resources)


class FakeClock:
    # create one bounded deterministic clock
    def __init__(self, step=10):
        self.current = 1_000
        self.step = step

    # advance one paired monotonic and UTC sample
    def __call__(self):
        self.current += self.step
        return {'monotonicMs': self.current, 'utc': '2026-11-27T21:00:00+00:00'}


class BackwardUtcClock(FakeClock):
    # advance monotonic time while moving UTC backward after opening
    def __call__(self):
        self.current += self.step
        utc = ('2026-11-27T21:00:01+00:00' if self.current == 1_010
               else '2026-11-27T21:00:00+00:00')
        return {'monotonicMs': self.current, 'utc': utc}


class ServiceResourceSessionTests(unittest.TestCase):
    # create one complete source observation and fixed physical filesystem
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.source_path = self.root / 'live-observation.json'
        self.boot = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa'
        self.machine = b'b' * 32 + b'\n'
        self.release = '2026.11.04-6'
        self.schema = '2026.11.03-3'
        self.revision = 'c' * 40
        self.environment = {
            'WEATHER_RELEASE': self.release,
            'WEATHER_SERVER_IMAGE': 'weather/server@sha256:' + '1' * 64,
            'WEATHER_WEB_IMAGE': 'weather/web@sha256:' + '2' * 64,
            'POSTGRES_IMAGE': 'postgres@sha256:' + '3' * 64,
            'CLOUDFLARED_IMAGE': 'cloudflared@sha256:' + '4' * 64,
            'WEATHER_CONTROL_PLANE_SHA256': '5' * 64,
            'WEATHER_CONTROL_PLANE_VERSION': '17',
        }
        self.environment_bytes = ''.join(
            # retain every allowlisted release identity
            f'{key}={value}\n' for key, value in self.environment.items()
        ).encode()
        self.files = {
            '/proc/sys/kernel/random/boot_id': (self.boot + '\n').encode(),
            '/etc/machine-id': self.machine,
            str(resources.live.ROOT / 'deploy/state/current-release'):
                (self.release + '\n').encode(),
            str(resources.live.ROOT / 'deploy/state/schema-release'):
                (self.schema + '\n').encode(),
            str(resources.live.ROOT / 'deploy/releases' / f'{self.release}.env'):
                self.environment_bytes,
            '/proc/stat': b'cpu  100 0 20 1000 0 0 0 0 0 0\ncpu0 50 0 10 500 0 0 0 0 0 0\n',
            '/proc/meminfo': (
                b'MemTotal:        8000000 kB\nMemFree:         3000000 kB\n'
                b'MemAvailable:    6000000 kB\nSwapTotal:             0 kB\n'
            ),
            '/proc/self/mountinfo': (
                b'29 23 0:26 / /sys/fs/cgroup rw,nosuid,nodev,noexec,relatime'
                b' - cgroup2 cgroup rw\n'
            ),
        }
        self.containers = []
        self.images = {}
        self.source_containers = {}
        self.cgroup_children = []
        # create each exact deployment identity and cgroup sample
        for index, (role, (name, env_key)) in enumerate(resources.live.ROLES.items(), 1):
            container_id = f'{index:x}' * 64
            image_id = 'sha256:' + ('6' if role in ('api', 'worker') else f'{index:x}') * 64
            pid = 100 + index
            cgroup_name = f'docker-{container_id}.scope'
            cgroup = f'0::/docker.slice/docker-workload.slice/{cgroup_name}'
            memory = {
                'api': 268_435_456, 'worker': 268_435_456,
                'web': 402_653_184, 'postgres': 536_870_912,
                'cloudflared': 134_217_728,
            }[role]
            nano_cpus = {
                'api': 500_000_000, 'worker': 500_000_000,
                'web': 250_000_000, 'postgres': 1_000_000_000,
                'cloudflared': 250_000_000,
            }[role]
            pids = {'api': 96, 'worker': 96, 'web': 64, 'postgres': 128, 'cloudflared': 64}[role]
            limits = {
                'Memory': memory, 'MemorySwap': memory * 2, 'NanoCpus': nano_cpus,
                'CpuQuota': 0, 'CpuPeriod': 0, 'PidsLimit': pids,
            }
            revision = self.revision if role == 'web' else None
            source = {
                'name': name, 'id': container_id, 'pid': pid, 'running': True,
                'health': 'healthy', 'startedAt': '2026-11-27T00:00:00Z',
                'imageConfigId': image_id, 'repoDigest': self.environment[env_key],
                'imageArchitecture': 'arm64', 'imageOs': 'linux',
                'revisionLabel': revision, 'cgroup': cgroup,
                'processStartTicks': str(20_000 + index), 'limits': limits,
            }
            self.source_containers[role] = source
            self.containers.append({
                'Name': '/' + name, 'Id': container_id, 'Image': image_id,
                'Config': {'Image': self.environment[env_key],
                           'Labels': {'com.docker.compose.project': 'weather'}},
                'State': {'Pid': pid, 'Running': True, 'Health': {'Status': 'healthy'},
                          'StartedAt': source['startedAt']},
                'HostConfig': limits,
            })
            self.images[image_id] = {
                'Id': image_id, 'Architecture': 'arm64', 'Os': 'linux',
                'RepoDigests': [self.environment[env_key]],
                'Config': {'Labels': {'org.opencontainers.image.revision': revision}},
            }
            self.files[f'/proc/{pid}/cgroup'] = (cgroup + '\n').encode()
            fields = ['0'] * 20
            fields[19] = source['processStartTicks']
            self.files[f'/proc/{pid}/stat'] = f'{pid} (fixture) {" ".join(fields)}'.encode()
            cgroup_path = resources.CGROUP_ROOT / cgroup[4:]
            quota = nano_cpus * 100_000 // 1_000_000_000
            self.files.update({
                str(cgroup_path / 'cpu.max'): f'{quota} 100000\n'.encode(),
                str(cgroup_path / 'cpu.stat'): (
                    b'usage_usec 1200\nuser_usec 1000\nsystem_usec 200\n'
                    b'nr_periods 20\nnr_throttled 2\nthrottled_usec 50\n'
                ),
                str(cgroup_path / 'memory.current'): b'1000000\n',
                str(cgroup_path / 'memory.events'):
                    b'low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\noom_group_kill 0\n',
                str(cgroup_path / 'memory.max'): f'{memory}\n'.encode(),
                str(cgroup_path / 'memory.peak'): b'2000000\n',
                str(cgroup_path / 'memory.swap.max'): f'{memory}\n'.encode(),
                str(cgroup_path / 'pids.current'): b'5\n',
                str(cgroup_path / 'pids.max'): f'{pids}\n'.encode(),
            })
            self.cgroup_children.append(cgroup_name)
        self.source = {
            'contractVersion': resources.live.CONTRACT,
            'evidenceClass': 'actual-read-only-host-observation',
            'host': {
                'hostname': 'blueberry', 'architecture': 'aarch64',
                'kernel': '6.12.75', 'bootId': self.boot,
                'machineIdSha256': hashlib.sha256(self.machine).hexdigest(),
                'dockerVersion': '26.1.5', 'cgroupVersion': '2',
            },
            'capturedStartedAt': '2026-11-27T00:00:00+00:00',
            'capturedCompletedAt': '2026-11-27T00:00:01+00:00',
            'release': self.release, 'schemaRelease': self.schema,
            'sourceRevision': self.revision,
            'observedReleaseEnvironmentSha256':
                hashlib.sha256(self.environment_bytes).hexdigest(),
            'releaseImages': self.environment,
            'migrationLedgerSha256': '7' * 64, 'migrationCount': 2,
            'containers': self.source_containers, 'readOnly': True,
            'allIdentitiesStable': True, 'expectationSha256': '8' * 64,
            'collectorSha256': '9' * 64,
        }
        self.write_source()
        self.clock = FakeClock()
        self.commands = []
        self.stat_offsets = {}
        self.docker_version = '26.1.5'
        self.cgroup_version = '2'

    # remove only this test's private directory
    def tearDown(self):
        self.temporary.cleanup()

    # replace the private source bytes and digest
    def write_source(self):
        data = json.dumps(self.source, sort_keys=True).encode()
        self.source_path.write_bytes(data)
        os.chmod(self.source_path, 0o600)
        self.source_sha = hashlib.sha256(data).hexdigest()

    # return one fixed read-only Docker observation
    def run_command(self, *args):
        self.commands.append(args)
        # serve the exact retained Docker daemon identities
        if args[1:] == ('version', '--format', '{{.Server.Version}}'):
            return f'{self.docker_version}\n'.encode()
        if args[1:] == ('info', '--format', '{{.CgroupVersion}}'):
            return f'{self.cgroup_version}\n'.encode()
        # serve only Docker inspect and image inspect
        if args[1] == 'inspect':
            return json.dumps(self.containers).encode()
        if args[1:3] == ('image', 'inspect'):
            return json.dumps([self.images[args[3]]]).encode()
        self.fail(f'unexpected command: {args}')

    # read one synthetic physical source
    def read(self, path):
        return self.files[str(path)]

    # retain one stable synthetic directory identity
    def stat(self, path):
        encoded = str(path).encode()
        return SimpleNamespace(
            st_mode=stat.S_IFDIR | 0o755, st_dev=42,
            st_ino=(int.from_bytes(hashlib.sha256(encoded).digest()[:4], 'big')
                    + self.stat_offsets.get(str(path), 0)),
        )

    # list the shared workload-slice children
    def listdir(self, path):
        self.assertEqual(str(path), '/sys/fs/cgroup/docker.slice/docker-workload.slice')
        return list(self.cgroup_children)

    # build the complete isolated adapter set
    def adapters(self, clock=None):
        return {
            'clock': clock or self.clock,
            'host': lambda: SimpleNamespace(
                nodename='blueberry', machine='aarch64', release='6.12.75'),
            'listdir': self.listdir, 'loader': resources.live.read_private_json,
            'read': self.read, 'realpath': lambda path: str(path),
            'run': self.run_command, 'sleep': lambda _: None, 'stat': self.stat,
        }

    # open one isolated resource session
    def open(self, clock=None):
        return resources.ServiceResourceSession.open(
            str(self.source_path), self.source_sha, 'test-session',
            self.adapters(clock),
        )

    # prove all fixed scopes are sampled without aggregate authority
    def test_collects_bounded_read_only_waves_without_promoting_them(self):
        result = resources.collect_preflight(
            str(self.source_path), self.source_sha, self.adapters(),
        )
        self.assertEqual(len(result['waves']), 3)
        self.assertEqual(set(result['waves'][0]['samples']), set(resources.RESOURCE_ROLES))
        self.assertFalse(result['authority']['nativeEligible'])
        self.assertFalse(result['authority']['promotable'])
        self.assertFalse(result['authority']['physicalCapture'])
        self.assertEqual(result['authority']['captureKind'], 'injected-test-only')
        self.assertIsNone(result['collectorBindings'])
        self.assertFalse(result['topology']['applicationAggregatePeakSupported'])
        self.assertFalse(result['topology']['parentExclusiveToApplication'])
        self.assertEqual(result['topology']['applicationRoles'], ['api', 'web', 'worker'])
        self.assertEqual(result['topology']['excludedSiblingRoles'], ['cloudflared'])
        self.assertIsNone(result['workload'])
        self.assertEqual(result['measuredScopes']['api']['scopeClass'], 'application')
        self.assertEqual(result['measuredScopes']['postgres']['scopeClass'], 'database')
        api_identity = result['waves'][0]['samples']['api']['identity']
        self.assertEqual(api_identity['before']['binding'], api_identity['after']['binding'])
        self.assertEqual(
            api_identity['before']['binding']['pid'], self.source_containers['api']['pid'])
        self.assertEqual(
            result['installationObservations']['opened']['host']['dockerVersion'], '26.1.5')
        self.assertEqual(
            result['installationObservations']['closed']['release']['release'], self.release)
        self.assertTrue(all(
            # retain distinct per-service lifetime peaks without summing them
            wave['samples']['api']['parsed']['memoryPeakBytes'] == 2_000_000
            for wave in result['waves']
        ))
        self.assertTrue(all(args[1] in ('version', 'info', 'inspect', 'image')
                            for args in self.commands))

    # reject omitted source roles before touching resource counters
    def test_rejects_incomplete_source_service_set(self):
        del self.source['containers']['worker']
        self.write_source()
        with self.assertRaisesRegex(resources.ResourceSessionError, 'container set'):
            self.open()

    # reject a Docker daemon identity that differs from the retained host
    def test_rejects_docker_host_identity_drift(self):
        self.docker_version = '26.1.6'
        with self.assertRaisesRegex(resources.ResourceSessionError, 'Docker host identity'):
            self.open()

    # reject a non-cgroup2 filesystem mounted over the retained root
    def test_rejects_wrong_cgroup_mount(self):
        self.files['/proc/self/mountinfo'] = (
            b'29 23 0:26 / /sys/fs/cgroup rw - tmpfs tmpfs rw\n'
        )
        with self.assertRaisesRegex(resources.ResourceSessionError, 'cgroup2 root mount'):
            self.open()

    # reject installed service identity drift when the session closes
    def test_rejects_service_identity_drift_at_close(self):
        session = self.open()
        session.sample_wave()
        self.containers[0]['State']['StartedAt'] = '2026-11-27T00:00:02Z'
        with self.assertRaisesRegex(resources.ResourceSessionError, 'container identity'):
            session.close()

    # reject transient membership drift before reading a wave's counters
    def test_rejects_cgroup_membership_drift_during_wave(self):
        session = self.open()
        pid = self.source_containers['api']['pid']
        self.files[f'/proc/{pid}/cgroup'] = b'0::/wrong.scope\n'
        with self.assertRaisesRegex(resources.ResourceSessionError, 'during a wave'):
            session.sample_wave()

    # reject transient cgroup directory replacement during a wave
    def test_rejects_cgroup_directory_drift_during_wave(self):
        session = self.open()
        api_path = resources.CGROUP_ROOT / self.source_containers['api']['cgroup'][4:]
        self.stat_offsets[str(api_path)] = 1
        with self.assertRaisesRegex(resources.ResourceSessionError, 'during a wave'):
            session.sample_wave()

    # reject a cgroup ceiling that differs from retained Docker limits
    def test_rejects_limit_drift(self):
        api_path = resources.CGROUP_ROOT / self.source_containers['api']['cgroup'][4:]
        self.files[str(api_path / 'memory.max')] = b'999999\n'
        session = self.open()
        with self.assertRaisesRegex(resources.ResourceSessionError, 'ceilings differ'):
            session.sample_wave()

    # reject a sampling wave whose source span exceeds the fixed bound
    def test_rejects_wave_clock_overrun(self):
        session = self.open(FakeClock(step=6_000))
        with self.assertRaisesRegex(resources.ResourceSessionError, 'clocks are not ordered'):
            session.sample_wave()

    # reject a gap beyond the fixed primitive's maximum sampling spacing
    def test_rejects_sample_spacing_overrun(self):
        session = self.open()
        session.sample_wave()
        self.clock.current += resources.MAX_SPACING_MS + 1
        with self.assertRaisesRegex(resources.ResourceSessionError, 'clocks are not ordered'):
            session.sample_wave()

    # reject UTC regression even while monotonic time continues forward
    def test_rejects_backward_utc(self):
        session = self.open(BackwardUtcClock())
        with self.assertRaisesRegex(resources.ResourceSessionError, 'UTC clocks'):
            session.sample_wave()

    # reject a cumulative CPU counter reset within the retained process lifetime
    def test_rejects_cumulative_cpu_counter_regression(self):
        session = self.open()
        session.sample_wave()
        api_path = resources.CGROUP_ROOT / self.source_containers['api']['cgroup'][4:]
        self.files[str(api_path / 'cpu.stat')] = (
            b'usage_usec 1100\nuser_usec 1000\nsystem_usec 200\n'
            b'nr_periods 20\nnr_throttled 2\nthrottled_usec 50\n'
        )
        with self.assertRaisesRegex(resources.ResourceSessionError, 'cpuStat counter regressed'):
            session.sample_wave()

    # reject a lifetime peak reset while the retained process remains unchanged
    def test_rejects_cumulative_memory_peak_regression(self):
        session = self.open()
        session.sample_wave()
        api_path = resources.CGROUP_ROOT / self.source_containers['api']['cgroup'][4:]
        self.files[str(api_path / 'memory.peak')] = b'1500000\n'
        with self.assertRaisesRegex(resources.ResourceSessionError, 'memory peak regressed'):
            session.sample_wave()

    # reject changed cumulative memory-event keys across waves
    def test_rejects_cumulative_counter_key_drift(self):
        session = self.open()
        session.sample_wave()
        api_path = resources.CGROUP_ROOT / self.source_containers['api']['cgroup'][4:]
        self.files[str(api_path / 'memory.events')] = b'low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\n'
        with self.assertRaisesRegex(resources.ResourceSessionError, 'memoryEvents counter keys'):
            session.sample_wave()

    # reject a cumulative memory-event counter reset within one lifetime
    def test_rejects_cumulative_memory_event_regression(self):
        session = self.open()
        api_path = resources.CGROUP_ROOT / self.source_containers['api']['cgroup'][4:]
        self.files[str(api_path / 'memory.events')] = (
            b'low 1\nhigh 0\nmax 0\noom 0\noom_kill 0\noom_group_kill 0\n'
        )
        session.sample_wave()
        self.files[str(api_path / 'memory.events')] = (
            b'low 0\nhigh 0\nmax 0\noom 0\noom_kill 0\noom_group_kill 0\n'
        )
        with self.assertRaisesRegex(resources.ResourceSessionError, 'memoryEvents counter regressed'):
            session.sample_wave()

    # reject an unbounded number of resource samples
    def test_rejects_sample_count_overrun(self):
        session = self.open()
        session.waves = [{'closed': {'monotonicMs': self.clock.current}}] * resources.MAX_SAMPLES
        with self.assertRaisesRegex(resources.ResourceSessionError, 'sample count'):
            session.sample_wave()

    # preserve a closed receipt and reject repeated lifecycle transitions
    def test_closed_session_cannot_be_finished_or_aborted_again(self):
        session = self.open()
        session.sample_wave()
        receipt = session.close()
        with self.assertRaisesRegex(resources.ResourceSessionError, 'no complete wave'):
            session.close()
        with self.assertRaisesRegex(resources.ResourceSessionError, 'only an open'):
            session.abort()
        self.assertEqual(len(receipt['waves']), 1)

    # reject source replacement even when the submitted digest is later reused
    def test_private_source_reader_rejects_digest_or_mode_change(self):
        with self.assertRaisesRegex(resources.live.ObservationError, 'digest differs'):
            resources.live.read_private_json(str(self.source_path), '0' * 64)
        os.chmod(self.source_path, 0o644)
        with self.assertRaisesRegex(resources.live.ObservationError, 'private owner-held'):
            resources.live.read_private_json(str(self.source_path), self.source_sha)


if __name__ == '__main__':
    unittest.main()
