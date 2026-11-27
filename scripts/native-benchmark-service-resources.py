#!/usr/bin/env python3
"""Capture read-only, source-bound Blueberry service and host resource waves."""

import argparse
import importlib.util
import json
import os
import re
import stat
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path
from types import SimpleNamespace

SCRIPT_DIR = Path(__file__).resolve().parent
SCRIPT_PATH = Path(__file__).resolve()
LIVE_SCRIPT = SCRIPT_DIR / 'native-benchmark-live-observation.py'
SPEC = importlib.util.spec_from_file_location('native_benchmark_live_observation', LIVE_SCRIPT)
live = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(live)

CONTRACT = 'weather-backend-service-resource-waves/v1'
CGROUP_ROOT = Path('/sys/fs/cgroup')
RESOURCE_ROLES = ('api', 'web', 'worker', 'postgres')
APPLICATION_ROLES = ('api', 'web', 'worker')
ALL_ROLES = tuple(live.ROLES)
SOURCE_KEYS = {
    'contractVersion', 'evidenceClass', 'host', 'capturedStartedAt',
    'capturedCompletedAt', 'release', 'schemaRelease', 'sourceRevision',
    'observedReleaseEnvironmentSha256', 'releaseImages',
    'migrationLedgerSha256', 'migrationCount', 'containers', 'readOnly',
    'allIdentitiesStable', 'expectationSha256', 'collectorSha256',
}
CONTAINER_KEYS = {
    'name', 'id', 'pid', 'running', 'health', 'startedAt', 'imageConfigId',
    'repoDigest', 'imageArchitecture', 'imageOs', 'revisionLabel', 'cgroup',
    'processStartTicks', 'limits',
}
HOST_KEYS = {
    'hostname', 'architecture', 'kernel', 'bootId', 'machineIdSha256',
    'dockerVersion', 'cgroupVersion',
}
COUNTER_FILES = (
    'cpu.max', 'cpu.stat', 'memory.current', 'memory.events', 'memory.max',
    'memory.peak', 'memory.swap.max', 'pids.current', 'pids.max',
)
MAX_READ = 1024 * 1024
MAX_SAMPLES = 4096
MAX_SPACING_MS = 60_000
MAX_DURATION_MS = 60 * 60 * 1000
MAX_WAVE_MS = 5_000
SESSION_ID = re.compile(r'[A-Za-z0-9._:-]{1,160}')
DECIMAL = re.compile(r'(?:0|[1-9][0-9]*)')


class ResourceSessionError(RuntimeError):
    """Report one incomplete or changed resource session."""


# enforce one resource session invariant
def require(condition, message):
    # fail closed on malformed or drifting evidence
    if not condition:
        raise ResourceSessionError(message)


# sample one paired UTC and monotonic instant
def clock():
    return {
        'monotonicMs': time.monotonic_ns() // 1_000_000,
        'utc': datetime.now(timezone.utc).isoformat(),
    }


# validate one paired UTC and monotonic instant
def read_clock(sample_clock):
    value = sample_clock()
    require(isinstance(value, dict) and set(value) == {'monotonicMs', 'utc'}
            and type(value['monotonicMs']) is int and value['monotonicMs'] >= 0
            and isinstance(value['utc'], str),
            'resource clock sample is invalid')
    instant = datetime.fromisoformat(value['utc'])
    require(instant.utcoffset() is not None and instant.utcoffset().total_seconds() == 0,
            'resource clock is not UTC')
    return value


# require monotonic and UTC ordering for one retained clock bracket
def require_clock_order(before, after, label, max_elapsed=None):
    elapsed = after['monotonicMs'] - before['monotonicMs']
    bounded = max_elapsed is None or elapsed <= max_elapsed
    require(elapsed >= 0 and bounded
            and datetime.fromisoformat(after['utc']) >= datetime.fromisoformat(before['utc']),
            f'{label} monotonic or UTC clocks are not ordered')


# read one bounded local file
def read_file(path):
    with Path(path).open('rb') as handle:
        data = handle.read(MAX_READ + 1)
    require(0 < len(data) <= MAX_READ, f'{path} is empty or exceeds its bound')
    return data


# decode one canonical decimal or max ceiling
def parse_ceiling(value, label):
    # retain an explicit unlimited cgroup ceiling
    if value == 'max':
        return None
    require(DECIMAL.fullmatch(value) is not None, f'{label} is not a canonical ceiling')
    return int(value)


# parse one named cgroup counter file
def parse_named_counters(data, label):
    result = {}
    # retain each unique kernel counter
    for line in data.splitlines():
        fields = line.split(' ')
        require(len(fields) == 2 and fields[0] not in result
                and DECIMAL.fullmatch(fields[1]) is not None,
                f'{label} contains an invalid counter')
        result[fields[0]] = int(fields[1])
    require(bool(result), f'{label} has no counters')
    return result


# parse one fixed cgroup sample without writing peak state
def read_cgroup_sample(target, read, sample_clock):
    opened = read_clock(sample_clock)
    raw = {name: read(target['path'] / name).decode('ascii').strip()
           for name in COUNTER_FILES}
    closed = read_clock(sample_clock)
    require_clock_order(opened, closed, f"{target['role']} resource sample", MAX_WAVE_MS)
    cpu_max = raw['cpu.max'].split(' ')
    require(len(cpu_max) == 2, f"{target['role']} cpu.max is invalid")
    parsed = {
        'cpuMax': {
            'quotaUsec': parse_ceiling(cpu_max[0], 'cpu.max quota'),
            'periodUsec': parse_ceiling(cpu_max[1], 'cpu.max period'),
        },
        'cpuStat': parse_named_counters(raw['cpu.stat'], 'cpu.stat'),
        'memoryCurrentBytes': parse_ceiling(raw['memory.current'], 'memory.current'),
        'memoryEvents': parse_named_counters(raw['memory.events'], 'memory.events'),
        'memoryMaxBytes': parse_ceiling(raw['memory.max'], 'memory.max'),
        'memoryPeakBytes': parse_ceiling(raw['memory.peak'], 'memory.peak'),
        'memorySwapMaxBytes': parse_ceiling(raw['memory.swap.max'], 'memory.swap.max'),
        'pidsCurrent': parse_ceiling(raw['pids.current'], 'pids.current'),
        'pidsMax': parse_ceiling(raw['pids.max'], 'pids.max'),
    }
    require(parsed['memoryCurrentBytes'] is not None
            and parsed['memoryPeakBytes'] is not None
            and parsed['pidsCurrent'] is not None,
            f"{target['role']} current counters cannot be unlimited")
    require(parsed['memoryPeakBytes'] >= parsed['memoryCurrentBytes'],
            f"{target['role']} memory peak precedes current memory")
    return {'closed': closed, 'opened': opened, 'parsed': parsed, 'raw': raw}


# capture one bounded host counter source
def read_host_sample(read, sample_clock):
    opened = read_clock(sample_clock)
    proc_stat = read('/proc/stat').decode('ascii')
    meminfo = read('/proc/meminfo').decode('ascii')
    closed = read_clock(sample_clock)
    require(proc_stat.startswith('cpu ') and '\nMemTotal:' not in proc_stat
            and 'MemTotal:' in meminfo and 'MemAvailable:' in meminfo,
            'host counter sources are invalid')
    require_clock_order(opened, closed, 'host resource sample', MAX_WAVE_MS)
    return {'closed': closed, 'opened': opened, 'meminfo': meminfo, 'procStat': proc_stat}


# prove the one canonical cgroup v2 mount without trusting the source receipt
def validate_cgroup_mount(read, realpath, path_stat):
    require(Path(realpath(CGROUP_ROOT)) == CGROUP_ROOT,
            'cgroup root is not canonical')
    lines = read('/proc/self/mountinfo').decode('utf-8').splitlines()
    matches = []
    # retain only the exact cgroup root mount
    for line in lines:
        halves = line.split(' - ')
        fields = halves[0].split(' ') if len(halves) == 2 else []
        kind = halves[1].split(' ')[0] if len(halves) == 2 else ''
        # retain every mount stacked on the exact cgroup root
        if len(fields) >= 6 and fields[4] == str(CGROUP_ROOT):
            matches.append((kind, line))
    require(len(matches) == 1 and matches[0][0] == 'cgroup2',
            'exactly one cgroup2 root mount is required')
    metadata = path_stat(CGROUP_ROOT)
    require(stat.S_ISDIR(metadata.st_mode), 'cgroup root is not a directory')
    return {
        'device': str(metadata.st_dev), 'inode': str(metadata.st_ino),
        'mountLineSha256': live.digest(matches[0][1].encode()),
        'path': str(CGROUP_ROOT),
    }


# validate one retained live observation before deriving targets
def validate_source(value):
    require(isinstance(value, dict) and set(value) == SOURCE_KEYS
            and value['contractVersion'] == live.CONTRACT
            and value['evidenceClass'] == 'actual-read-only-host-observation'
            and value['readOnly'] is True and value['allIdentitiesStable'] is True,
            'source observation contract differs')
    require(isinstance(value['host'], dict) and set(value['host']) == HOST_KEYS
            and value['host']['hostname'] == 'blueberry'
            and value['host']['architecture'] == 'aarch64'
            and value['host']['cgroupVersion'] == '2'
            and live.BOOT_ID.fullmatch(value['host']['bootId']) is not None
            and live.DIGEST.fullmatch(value['host']['machineIdSha256']) is not None,
            'source host identity differs')
    require(isinstance(value['containers'], dict)
            and set(value['containers']) == set(ALL_ROLES),
            'source container set differs')
    # validate every fixed deployment identity, including excluded cloudflared
    for role in ALL_ROLES:
        container = value['containers'][role]
        require(isinstance(container, dict) and set(container) == CONTAINER_KEYS
                and container['name'] == live.ROLES[role][0]
                and live.DIGEST.fullmatch(container['id']) is not None
                and type(container['pid']) is int and container['pid'] > 0
                and container['running'] is True and container['health'] == 'healthy'
                and container['cgroup'].startswith('0::/')
                and container['id'] in container['cgroup']
                and DECIMAL.fullmatch(container['processStartTicks']) is not None,
                f'{role} source identity differs')
        require(isinstance(container['limits'], dict)
                and set(container['limits']) == set(live.LIMIT_KEYS)
                and all(type(limit) is int for limit in container['limits'].values()),
                f'{role} source limits differ')
    return value


# revalidate the source-owned deployment identity without querying application data
def validate_installation(source, run, read, host):
    identity = host()
    expected_host = source['host']
    require(identity.nodename == expected_host['hostname']
            and identity.machine == expected_host['architecture']
            and identity.release == expected_host['kernel'],
            'host identity changed')
    docker_version = run(
        live.DOCKER, 'version', '--format', '{{.Server.Version}}').decode().strip()
    cgroup_version = run(
        live.DOCKER, 'info', '--format', '{{.CgroupVersion}}').decode().strip()
    require(docker_version == expected_host['dockerVersion']
            and cgroup_version == expected_host['cgroupVersion'],
            'Docker host identity changed')
    boot_id = read('/proc/sys/kernel/random/boot_id').decode('ascii').strip()
    machine_id_sha256 = live.digest(read('/etc/machine-id'))
    require(boot_id == expected_host['bootId']
            and machine_id_sha256 == expected_host['machineIdSha256'],
            'host boot or machine identity changed')
    release = read(live.ROOT / 'deploy/state/current-release').decode('ascii').strip()
    schema = read(live.ROOT / 'deploy/state/schema-release').decode('ascii').strip()
    environment_path = live.ROOT / 'deploy/releases' / f'{release}.env'
    environment = read(environment_path)
    env = live.parse_release_environment(environment, release)
    require(release == source['release'] and schema == source['schemaRelease']
            and live.digest(environment) == source['observedReleaseEnvironmentSha256']
            and env == source['releaseImages'],
            'installed release environment changed')
    expected = {
        'imageConfigIds': {
            role: source['containers'][role]['imageConfigId'] for role in ALL_ROLES
        },
        'sourceRevision': source['sourceRevision'],
    }
    observed = live.inspect_containers(expected, env, run, read)
    require(observed == source['containers'], 'installed container identity changed')
    return {
        'containers': observed,
        'host': {
            'architecture': identity.machine, 'bootId': boot_id,
            'cgroupVersion': cgroup_version, 'dockerVersion': docker_version,
            'hostname': identity.nodename, 'kernel': identity.release,
            'machineIdSha256': machine_id_sha256,
        },
        'release': {
            'environmentSha256': live.digest(environment), 'images': env,
            'release': release, 'schemaRelease': schema,
        },
    }


# derive one canonical cgroup target and stable directory identity
def cgroup_target(role, source, realpath, path_stat):
    cgroup = source['containers'][role]['cgroup']
    require(cgroup.startswith('0::/'), f'{role} cgroup membership is invalid')
    relative = cgroup[4:]
    require(relative and not relative.startswith('/') and '//' not in relative
            and all(part not in ('', '.', '..') for part in relative.split('/')),
            f'{role} cgroup path is not normalized')
    path = CGROUP_ROOT / relative
    require(Path(realpath(path)) == path, f'{role} cgroup path is not canonical')
    metadata = path_stat(path)
    root_metadata = path_stat(CGROUP_ROOT)
    require(stat.S_ISDIR(metadata.st_mode) and metadata.st_dev == root_metadata.st_dev,
            f'{role} cgroup is not a directory on the cgroup mount')
    return {
        'directory': {
            'device': str(metadata.st_dev), 'inode': str(metadata.st_ino),
            'mode': oct(stat.S_IMODE(metadata.st_mode)),
            'mountDevice': str(root_metadata.st_dev),
        },
        'path': path, 'role': role,
    }


# observe and prove one process owns the exact retained cgroup directory
def observe_wave_identity(target, source, read, path_stat, sample_clock):
    container = source['containers'][target['role']]
    cgroup = read(f"/proc/{container['pid']}/cgroup").decode('ascii').strip()
    fields = read(f"/proc/{container['pid']}/stat").decode('ascii').rsplit(')', 1)
    require(len(fields) == 2 and len(fields[1].split()) > 19,
            f"{target['role']} process stat is invalid")
    ticks = fields[1].split()[19]
    metadata = path_stat(target['path'])
    root_metadata = path_stat(CGROUP_ROOT)
    current_directory = {
        'device': str(metadata.st_dev), 'inode': str(metadata.st_ino),
        'mode': oct(stat.S_IMODE(metadata.st_mode)),
        'mountDevice': str(root_metadata.st_dev),
    }
    require(cgroup == container['cgroup'] and ticks == container['processStartTicks']
            and current_directory == target['directory'],
            f"{target['role']} process or cgroup identity changed during a wave")
    return {
        'binding': {
            'cgroupMembership': cgroup, 'directory': current_directory,
            'pid': container['pid'], 'processStartTicks': ticks,
        },
        'observedAt': read_clock(sample_clock),
    }


# derive and validate fixed cgroup ceilings from Docker limits
def validate_limits(target, source, sample):
    limits = source['containers'][target['role']]['limits']
    expected_memory = limits['Memory'] or None
    expected_swap = (limits['MemorySwap'] - limits['Memory']) if limits['MemorySwap'] > 0 else None
    period = limits['CpuPeriod'] or 100_000
    quota = limits['CpuQuota'] or (
        limits['NanoCpus'] * period // 1_000_000_000 if limits['NanoCpus'] > 0 else None
    )
    parsed = sample['parsed']
    require(parsed['memoryMaxBytes'] == expected_memory
            and parsed['memorySwapMaxBytes'] == expected_swap
            and parsed['pidsMax'] == (limits['PidsLimit'] or None)
            and parsed['cpuMax'] == {'quotaUsec': quota, 'periodUsec': period},
            f"{target['role']} cgroup ceilings differ from the source observation")


# require cumulative counters to continue within one retained process lifetime
def validate_cumulative_sample(previous, current, role):
    require(previous['identity']['after']['binding'] == current['identity']['before']['binding'],
            f'{role} resource sample lifetime changed')
    # compare every retained cumulative counter family
    for field in ('cpuStat', 'memoryEvents'):
        before = previous['parsed'][field]
        after = current['parsed'][field]
        require(set(after) == set(before), f'{role} {field} counter keys changed')
        require(all(after[key] >= before[key] for key in before),
                f'{role} cumulative {field} counter regressed')
    require(current['parsed']['memoryPeakBytes'] >= previous['parsed']['memoryPeakBytes'],
            f'{role} cumulative memory peak regressed')


# retain the shared parent without claiming an exclusive application aggregate
def parent_topology(targets, realpath, path_stat, list_directory):
    parents = {target['path'].parent for target in targets.values()}
    require(len(parents) == 1, 'fixed service cgroups do not share one observed parent')
    parent = parents.pop()
    require(Path(realpath(parent)) == parent, 'service cgroup parent is not canonical')
    metadata = path_stat(parent)
    children = sorted(name for name in list_directory(parent) if name.endswith('.scope'))
    role_children = {role: targets[role]['path'].name for role in ALL_ROLES}
    require(all(child in children for child in role_children.values()),
            'service cgroup parent omits a source-owned child')
    return {
        'applicationAggregatePeakSupported': False,
        'applicationRoles': list(APPLICATION_ROLES),
        'databaseRole': 'postgres',
        'directory': {'device': str(metadata.st_dev), 'inode': str(metadata.st_ino)},
        'excludedSiblingRoles': ['cloudflared'],
        'limitation': 'shared parent includes PostgreSQL and cloudflared; service peaks are distinct lifetimes and are never summed',
        'parentExclusiveToApplication': False,
        'path': str(parent),
        'roleChildren': role_children,
        'scopeChildren': children,
    }


# bind the exact executable collector and imported helper bytes
def collector_bindings():
    return {
        'liveObservationHelperSha256': live.digest(read_file(LIVE_SCRIPT)),
        'resourceWaveCollectorSha256': live.digest(read_file(SCRIPT_PATH)),
    }


# describe measured scopes without conflating their independent lifetimes
def measured_scopes(targets, source):
    result = {}
    # retain exact process and directory identity for every measured scope
    for role in RESOURCE_ROLES:
        container = source['containers'][role]
        result[role] = {
            'cgroupMembership': container['cgroup'],
            'directory': targets[role]['directory'],
            'path': str(targets[role]['path']), 'pid': container['pid'],
            'processStartTicks': container['processStartTicks'],
            'scopeClass': 'application' if role in APPLICATION_ROLES else 'database',
            'peakSemantics': 'individual-service-lifetime-not-additive',
        }
    return result


# construct actual adapters or one closed test substitution set
def adapters(overrides=None):
    values = {
        'clock': clock, 'host': os.uname, 'listdir': os.listdir,
        'loader': live.read_private_json, 'read': read_file,
        'realpath': os.path.realpath, 'run': live.command, 'sleep': time.sleep,
        'stat': os.stat,
    }
    # admit only known isolated seams
    if overrides is not None:
        require(isinstance(overrides, dict) and set(overrides) <= set(values),
                'resource session adapters differ')
        values.update(overrides)
    return SimpleNamespace(**values)


class ServiceResourceSession:
    """Own one exact source-bound sequence of ambient resource waves."""

    # open one physical but non-promotable observation session
    @classmethod
    def open(cls, source_path, source_sha256, session_id, injected=None):
        require(SESSION_ID.fullmatch(session_id) is not None, 'session identity is invalid')
        ports = adapters(injected)
        source_document = ports.loader(source_path, source_sha256)
        source = validate_source(source_document['value'])
        opening_installation = validate_installation(
            source, ports.run, ports.read, ports.host)
        targets = {
            # derive every fixed service target from source-owned identities
            role: cgroup_target(role, source, ports.realpath, ports.stat)
            for role in ALL_ROLES
        }
        mount = validate_cgroup_mount(ports.read, ports.realpath, ports.stat)
        topology = parent_topology(targets, ports.realpath, ports.stat, ports.listdir)
        bindings = None if injected is not None else collector_bindings()
        opened = read_clock(ports.clock)
        return cls(source_path, source_sha256, session_id, source_document, source,
                   targets, topology, mount, bindings, opening_installation,
                   opened, ports, injected is not None)

    # retain only session-owned fixed state
    def __init__(self, source_path, source_sha256, session_id, source_document,
                 source, targets, topology, mount, bindings, opening_installation,
                 opened, ports, injected):
        self.source_path = source_path
        self.source_sha256 = source_sha256
        self.session_id = session_id
        self.source_document = source_document
        self.source = source
        self.targets = targets
        self.topology = topology
        self.mount = mount
        self.bindings = bindings
        self.opening_installation = opening_installation
        self.opened = opened
        self.ports = ports
        self.injected = injected
        self.waves = []
        self.state = 'open'

    # sample all four measured scopes and host sources inside one coordinator wave
    def sample_wave(self):
        require(self.state == 'open', 'resource session is not open')
        require(len(self.waves) < MAX_SAMPLES,
                'resource session exceeded its sample count bound')
        wave_opened = read_clock(self.ports.clock)
        require_clock_order(self.opened, wave_opened, 'resource session', MAX_DURATION_MS)
        # bound spacing from the preceding completed wave
        if self.waves:
            require_clock_order(
                self.waves[-1]['closed'], wave_opened,
                'resource wave spacing', MAX_SPACING_MS,
            )
        require(validate_cgroup_mount(
            self.ports.read, self.ports.realpath, self.ports.stat) == self.mount,
            'cgroup mount identity changed during the resource session')
        samples = {}
        previous_clock = wave_opened
        # sample every fixed service and database cgroup independently
        for role in RESOURCE_ROLES:
            before = observe_wave_identity(
                self.targets[role], self.source, self.ports.read,
                self.ports.stat, self.ports.clock,
            )
            require_clock_order(previous_clock, before['observedAt'], 'resource source')
            sample = read_cgroup_sample(self.targets[role], self.ports.read, self.ports.clock)
            require_clock_order(before['observedAt'], sample['opened'], 'resource source')
            validate_limits(self.targets[role], self.source, sample)
            after = observe_wave_identity(
                self.targets[role], self.source, self.ports.read,
                self.ports.stat, self.ports.clock,
            )
            require_clock_order(sample['closed'], after['observedAt'], 'resource source')
            require(before['binding'] == after['binding'],
                    f'{role} resource sample lifetime changed')
            sample['identity'] = {'after': after, 'before': before}
            # retain cumulative continuity with the preceding wave
            if self.waves:
                validate_cumulative_sample(self.waves[-1]['samples'][role], sample, role)
            samples[role] = sample
            previous_clock = after['observedAt']
        host_sample = read_host_sample(self.ports.read, self.ports.clock)
        require_clock_order(previous_clock, host_sample['opened'], 'host resource source')
        wave_closed = read_clock(self.ports.clock)
        require_clock_order(host_sample['closed'], wave_closed, 'resource wave')
        require_clock_order(wave_opened, wave_closed, 'resource wave', MAX_WAVE_MS)
        require_clock_order(self.opened, wave_closed, 'resource session', MAX_DURATION_MS)
        require(validate_cgroup_mount(
            self.ports.read, self.ports.realpath, self.ports.stat) == self.mount,
            'cgroup mount identity changed during the resource wave')
        wave = {
            'closed': wave_closed, 'host': host_sample,
            'opened': wave_opened, 'samples': samples,
            'waveId': f'{self.session_id}:wave-{len(self.waves) + 1:04d}',
        }
        self.waves.append(wave)
        return wave

    # close only after revalidating source bytes, installation and directory identities
    def close(self):
        require(self.state == 'open' and self.waves, 'resource session has no complete wave')
        closing_document = self.ports.loader(self.source_path, self.source_sha256)
        require(closing_document['bytes'] == self.source_document['bytes']
                and closing_document['identity'] == self.source_document['identity'],
                'source observation changed during the resource session')
        closing_installation = validate_installation(
            self.source, self.ports.run, self.ports.read, self.ports.host)
        closing_targets = {
            # rederive every cgroup identity rather than trusting opening state
            role: cgroup_target(role, self.source, self.ports.realpath, self.ports.stat)
            for role in ALL_ROLES
        }
        require(all(closing_targets[role]['directory'] == self.targets[role]['directory']
                    for role in ALL_ROLES),
                'cgroup directory identity changed during the resource session')
        closing_topology = parent_topology(
            closing_targets, self.ports.realpath, self.ports.stat, self.ports.listdir,
        )
        require(closing_topology == self.topology,
                'cgroup parent topology changed during the resource session')
        require(validate_cgroup_mount(
            self.ports.read, self.ports.realpath, self.ports.stat) == self.mount,
            'cgroup mount identity changed during the resource session')
        # fail if physical collector provenance changed after opening
        if self.bindings is not None:
            require(collector_bindings() == self.bindings,
                    'resource collector bytes changed during the session')
        closed = read_clock(self.ports.clock)
        require_clock_order(self.waves[-1]['closed'], closed, 'resource session close')
        require_clock_order(self.opened, closed, 'resource session', MAX_DURATION_MS)
        self.state = 'closed'
        return {
            'authority': {
                'captureKind': 'injected-test-only' if self.injected
                else 'physical-linux-read-only-ambient-preflight',
                'physicalCapture': not self.injected,
                'nativeEligible': False, 'promotable': False, 'readOnly': True,
                'workloadTrafficGenerated': False,
            },
            'bounds': {
                'maxDurationMs': MAX_DURATION_MS, 'maxSampleCount': MAX_SAMPLES,
                'maxSampleSpacingMs': MAX_SPACING_MS, 'maxWaveMs': MAX_WAVE_MS,
            },
            'closed': closed, 'contractVersion': CONTRACT,
            'collectorBindings': self.bindings,
            'installationObservations': {
                'closed': closing_installation, 'opened': self.opening_installation,
            },
            'limitations': [
                'ambient preflight only; no workload denominator or result digest',
                'service lifetime peaks are independent and are never summed',
                'shared parent is not an exclusive application aggregate',
                'no cgroup peak counter was reset',
            ],
            'opened': self.opened, 'sessionId': self.session_id,
            'measuredScopes': measured_scopes(self.targets, self.source),
            'sourceBinding': {
                'fileIdentity': self.source_document['identity'],
                'kind': 'exact-private-live-observation-file',
                'sha256': self.source_sha256,
            },
            'cgroupMount': self.mount, 'topology': self.topology, 'waves': self.waves,
            'workload': None,
        }

    # abort without emitting a receipt that could be promoted
    def abort(self):
        require(self.state == 'open', 'only an open resource session can be aborted')
        self.state = 'aborted'
        self.waves.clear()


# run the fixed three-wave ambient preflight only
def collect_preflight(source_path, source_sha256, injected=None):
    session_id = f"ambient-preflight:{source_sha256[:16]}"
    session = ServiceResourceSession.open(
        source_path, source_sha256, session_id, injected,
    )
    try:
        # keep the CLI workload-free and finite
        for index in range(3):
            session.sample_wave()
            # avoid sleeping after the terminal sample
            if index < 2:
                session.ports.sleep(0.1)
        return session.close()
    except Exception:
        # preserve an already-terminal lifecycle error
        if session.state == 'open':
            session.abort()
        raise


# expose only the fixed read-only preflight command
def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-observation', required=True)
    parser.add_argument('--source-sha256', required=True)
    args = parser.parse_args(argv)
    try:
        require(os.geteuid() == 0, 'root is required for physical resource observation')
        result = collect_preflight(args.source_observation, args.source_sha256)
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0
    except (ResourceSessionError, live.ObservationError, OSError, KeyError,
            TypeError, ValueError, subprocess.SubprocessError) as error:
        message = str(error) if isinstance(error, (ResourceSessionError, live.ObservationError)) \
            else type(error).__name__
        print(f'native-benchmark-service-resources: {message}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
