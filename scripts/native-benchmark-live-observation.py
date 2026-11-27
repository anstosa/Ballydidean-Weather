#!/usr/bin/env python3
"""Collect one read-only, source-bound native Blueberry installation observation."""

import argparse
import hashlib
import json
import os
import re
import stat
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

CONTRACT = 'weather-backend-direct-live-observation/v1'
EXPECTATION_CONTRACT = 'weather-backend-direct-live-observation-expectation/v1'
ROOT = Path('/opt/weather/current')
DOCKER = '/usr/bin/docker'
ROLES = {
    'web': ('weather-web-1', 'WEATHER_WEB_IMAGE'),
    'api': ('weather-api-1', 'WEATHER_SERVER_IMAGE'),
    'worker': ('weather-worker-1', 'WEATHER_SERVER_IMAGE'),
    'postgres': ('weather-postgres-1', 'POSTGRES_IMAGE'),
    'cloudflared': ('weather-cloudflared-1', 'CLOUDFLARED_IMAGE'),
}
ENV_KEYS = {
    'WEATHER_RELEASE', 'WEATHER_SERVER_IMAGE', 'WEATHER_WEB_IMAGE', 'POSTGRES_IMAGE',
    'CLOUDFLARED_IMAGE', 'WEATHER_CONTROL_PLANE_SHA256', 'WEATHER_CONTROL_PLANE_VERSION',
}
LIMIT_KEYS = ('Memory', 'MemorySwap', 'NanoCpus', 'CpuQuota', 'CpuPeriod', 'PidsLimit')
DIGEST = re.compile(r'[0-9a-f]{64}')
IMAGE_ID = re.compile(r'sha256:[0-9a-f]{64}')
IMAGE_REF = re.compile(r'[a-zA-Z0-9._/-]+@sha256:[0-9a-f]{64}')
COMMIT = re.compile(r'[0-9a-f]{40}')
RELEASE = re.compile(r'[0-9]{4}\.[0-9]{2}\.[0-9]{2}-[0-9]+')
BOOT_ID = re.compile(r'[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}')
MAX_READ = 1024 * 1024
LEDGER_QUERY = 'BEGIN READ ONLY; SELECT name,checksum FROM schema_migrations ORDER BY name; COMMIT;'


class ObservationError(RuntimeError):
    """Report an incomplete or changed observation."""


# enforce one bounded observation invariant
def require(condition, message):
    if not condition:
        raise ObservationError(message)


# digest exact retained bytes
def digest(data):
    return hashlib.sha256(data).hexdigest()


# sample actual UTC time
def now():
    return datetime.now(timezone.utc).isoformat()


# run one fixed read-only host command
def command(*args):
    result = subprocess.run(args, capture_output=True, timeout=30, check=False)
    require(result.returncode == 0 and len(result.stdout) <= MAX_READ,
            'read-only observation command failed or exceeded its bound')
    return result.stdout


# read one bounded local file
def read_file(path):
    with Path(path).open('rb') as handle:
        data = handle.read(MAX_READ + 1)
    require(len(data) <= MAX_READ, 'observation file exceeds its bound')
    return data


# read one private JSON document without following aliases
def read_private_json(path, expected_sha):
    path = Path(path)
    require(path.is_absolute() and path.resolve(strict=True) == path,
            'expectation path must be canonical and absolute')
    descriptor = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        before = os.fstat(descriptor)
        require(stat.S_ISREG(before.st_mode) and before.st_uid == os.geteuid()
                and stat.S_IMODE(before.st_mode) == 0o600 and before.st_nlink == 1
                and 0 < before.st_size <= MAX_READ,
                'expectation must be a private owner-held regular file')
        data = b''
        # read exactly the bounded expectation
        while len(data) < before.st_size:
            chunk = os.read(descriptor, before.st_size - len(data))
            require(bool(chunk), 'expectation read ended early')
            data += chunk
        after = os.fstat(descriptor)
        require((before.st_dev, before.st_ino, before.st_size, before.st_mtime_ns, before.st_ctime_ns)
                == (after.st_dev, after.st_ino, after.st_size, after.st_mtime_ns, after.st_ctime_ns),
                'expectation changed during read')
    finally:
        os.close(descriptor)
    require(DIGEST.fullmatch(expected_sha) and digest(data) == expected_sha,
            'expectation digest differs')
    return {
        'bytes': data,
        'identity': {
            'device': str(before.st_dev), 'inode': str(before.st_ino),
            'mode': oct(stat.S_IMODE(before.st_mode)), 'ownerUserId': str(before.st_uid),
            'linkCount': before.st_nlink, 'sizeBytes': before.st_size,
            'modifiedNanoseconds': str(before.st_mtime_ns),
            'changedNanoseconds': str(before.st_ctime_ns),
        },
        'sha256': expected_sha,
        'value': json.loads(data),
    }


# read and validate one bounded expectation
def read_expectation(path, expected_sha):
    document = read_private_json(path, expected_sha)
    return validate_expectation(document['value'])


# validate the closed expected release identity
def validate_expectation(value):
    keys = {
        'contractVersion', 'release', 'schemaRelease', 'sourceRevision',
        'migrationLedgerSha256', 'migrationCount', 'imageConfigIds',
    }
    require(isinstance(value, dict) and set(value) == keys
            and value['contractVersion'] == EXPECTATION_CONTRACT,
            'expectation contract differs')
    require(isinstance(value['release'], str) and RELEASE.fullmatch(value['release'])
            and isinstance(value['schemaRelease'], str) and RELEASE.fullmatch(value['schemaRelease'])
            and isinstance(value['sourceRevision'], str) and COMMIT.fullmatch(value['sourceRevision'])
            and isinstance(value['migrationLedgerSha256'], str)
            and DIGEST.fullmatch(value['migrationLedgerSha256'])
            and type(value['migrationCount']) is int and 1 <= value['migrationCount'] <= 10000,
            'expectation release or ledger identity is invalid')
    images = value['imageConfigIds']
    require(isinstance(images, dict) and set(images) == set(ROLES)
            and all(isinstance(image, str) and IMAGE_ID.fullmatch(image) for image in images.values())
            and images['api'] == images['worker'],
            'expectation image identities differ')
    return value


# retain only nonsecret release environment fields
def parse_release_environment(data, release):
    values = {}
    # ignore unrelated environment entries without exposing them
    for line in data.decode('utf-8').splitlines():
        key, separator, value = line.partition('=')
        if separator and key in ENV_KEYS:
            require(key not in values, 'release environment repeats an identity')
            values[key] = value.strip('"')
    require(set(values) == ENV_KEYS and values['WEATHER_RELEASE'] == release,
            'release environment identity differs')
    require(all(IMAGE_REF.fullmatch(values[key]) for _, key in ROLES.values())
            and DIGEST.fullmatch(values['WEATHER_CONTROL_PLANE_SHA256'])
            and re.fullmatch(r'[0-9]+', values['WEATHER_CONTROL_PLANE_VERSION']),
            'release environment contains an invalid immutable identity')
    return values


# decode one bounded exact Docker object list
def records(data, count):
    value = json.loads(data)
    require(isinstance(value, list) and len(value) == count
            and all(isinstance(item, dict) for item in value),
            'Docker inspection object count differs')
    return value


# observe fixed deployment containers and their exact process identities
def inspect_containers(expected, env, run, read):
    names = [name for name, _ in ROLES.values()]
    rows = records(run(DOCKER, 'inspect', *names), len(names))
    result = {}
    # bind every fixed service to its release image and running process
    for (role, (name, env_key)), container in zip(ROLES.items(), rows, strict=True):
        config, state, host = container['Config'], container['State'], container['HostConfig']
        require(container['Name'] == '/' + name
                and IMAGE_ID.fullmatch(container['Image'])
                and container['Image'] == expected['imageConfigIds'][role]
                and config['Image'] == env[env_key]
                and config['Labels']['com.docker.compose.project'] == 'weather'
                and state['Running'] is True and state['Health']['Status'] == 'healthy',
                f'{role} installed container identity or health differs')
        image = records(run(DOCKER, 'image', 'inspect', container['Image']), 1)[0]
        require(image['Architecture'] == 'arm64' and image['Os'] == 'linux'
                and image['Id'] == container['Image'] and config['Image'] in image['RepoDigests'],
                f'{role} installed image identity differs')
        pid = state['Pid']
        require(type(pid) is int and pid > 0
                and DIGEST.fullmatch(container['Id']), f'{role} process identity is invalid')
        cgroup = read(f'/proc/{pid}/cgroup').decode('ascii').strip()
        ticks = read(f'/proc/{pid}/stat').decode('ascii').rsplit(')', 1)[1].split()[19]
        require(cgroup.startswith('0::/') and container['Id'] in cgroup and ticks.isdigit(),
                f'{role} cgroup or process start identity differs')
        limits = {key: host[key] for key in LIMIT_KEYS}
        require(all(type(value) is int for value in limits.values()),
                f'{role} resource limit identity is invalid')
        revision = (image.get('Config', {}).get('Labels') or {}).get('org.opencontainers.image.revision')
        # bind the deployed web artifact to the expected source revision
        if role == 'web':
            require(revision == expected['sourceRevision'], 'deployed web source revision differs')
        result[role] = {
            'name': name, 'id': container['Id'], 'pid': pid, 'running': True,
            'health': 'healthy', 'startedAt': state['StartedAt'], 'imageConfigId': image['Id'],
            'repoDigest': config['Image'], 'imageArchitecture': image['Architecture'],
            'imageOs': image['Os'], 'revisionLabel': revision, 'cgroup': cgroup,
            'processStartTicks': ticks, 'limits': limits,
        }
    return result


# collect an exact stable read-only observation
def collect(expected, run=command, read=read_file, host=os.uname, clock=now):
    expected = validate_expectation(expected)
    started = clock()
    identity = host()
    require(identity.nodename == 'blueberry' and identity.machine == 'aarch64',
            'observation requires the native Blueberry host')
    boot = read('/proc/sys/kernel/random/boot_id').decode('ascii').strip()
    require(BOOT_ID.fullmatch(boot), 'host boot identity is invalid')
    state = ROOT / 'deploy/state'
    release_path = state / 'current-release'
    schema_path = state / 'schema-release'
    release = read(release_path).decode('ascii').strip()
    schema = read(schema_path).decode('ascii').strip()
    require((release, schema) == (expected['release'], expected['schemaRelease']),
            'installed release or schema release differs')
    environment_path = ROOT / 'deploy/releases' / f'{release}.env'
    environment = read(environment_path)
    env = parse_release_environment(environment, release)
    before = inspect_containers(expected, env, run, read)
    ledger = run(
        DOCKER, 'exec', '-u', '999:999', 'weather-postgres-1', 'psql',
        '--no-password', '--no-psqlrc', '--quiet', '--username', 'postgres',
        '--dbname', 'weather', '--tuples-only', '--no-align', '--field-separator', '|',
        '--set', 'ON_ERROR_STOP=1', '--command', LEDGER_QUERY,
    )
    require(len(ledger.splitlines()) == expected['migrationCount']
            and digest(ledger) == expected['migrationLedgerSha256'],
            'actual migration ledger differs from retained source')
    after = inspect_containers(expected, env, run, read)
    require(before == after and host() == identity
            and read('/proc/sys/kernel/random/boot_id').decode('ascii').strip() == boot
            and read(release_path).decode('ascii').strip() == release
            and read(schema_path).decode('ascii').strip() == schema
            and read(environment_path) == environment,
            'installed identity changed during observation')
    return {
        'contractVersion': CONTRACT,
        'evidenceClass': 'actual-read-only-host-observation',
        'host': {
            'hostname': identity.nodename, 'architecture': identity.machine,
            'kernel': identity.release, 'bootId': boot,
            'machineIdSha256': digest(read('/etc/machine-id')),
            'dockerVersion': run(DOCKER, 'version', '--format', '{{.Server.Version}}').decode().strip(),
            'cgroupVersion': run(DOCKER, 'info', '--format', '{{.CgroupVersion}}').decode().strip(),
        },
        'capturedStartedAt': started, 'capturedCompletedAt': clock(),
        'release': release, 'schemaRelease': schema, 'sourceRevision': expected['sourceRevision'],
        'observedReleaseEnvironmentSha256': digest(environment), 'releaseImages': env,
        'migrationLedgerSha256': digest(ledger), 'migrationCount': len(ledger.splitlines()),
        'containers': before, 'readOnly': True, 'allIdentitiesStable': True,
    }


# collect only one fixed live observation from a reviewed expectation
def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--expectation', required=True)
    parser.add_argument('--expectation-sha256', required=True)
    args = parser.parse_args(argv)
    try:
        require(os.geteuid() == 0, 'root is required for installed identity observation')
        expected = read_expectation(args.expectation, args.expectation_sha256)
        result = collect(expected)
        result['expectationSha256'] = args.expectation_sha256
        result['collectorSha256'] = digest(read_file(__file__))
        print(json.dumps(result, indent=2, sort_keys=True))
        return 0
    except (ObservationError, OSError, KeyError, TypeError, ValueError, IndexError,
            subprocess.SubprocessError) as error:
        # avoid exposing raw Docker output or environment bytes on failure
        message = str(error) if isinstance(error, ObservationError) else type(error).__name__
        print(f'native-benchmark-live-observation: {message}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
