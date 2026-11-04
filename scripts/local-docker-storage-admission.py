#!/usr/bin/env python3
"""Check actual storage headroom before allocating local database fixture copies."""
import argparse
import base64
import hashlib
import json
import os
import re
import stat
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

GIB = 1024 ** 3


def run(argv):
    return subprocess.check_output(argv, timeout=180).decode('utf-8-sig').strip()


def evaluate(copy_bytes, copies, reserve_bytes, linux_free, windows_free=None, wsl=False):
    if copy_bytes <= 0 or copies <= 0 or reserve_bytes < 0:
        raise ValueError('Invalid copy budget')
    required = copy_bytes * copies + reserve_bytes
    if wsl and windows_free is None:
        raise ValueError('WSL requires a measured Windows backing-drive free-space value')
    admitted = linux_free >= required and (not wsl or windows_free >= required)
    return required, admitted


def windows_backing_space(distro):
    if not distro:
        raise RuntimeError('WSL distribution name is unavailable; refusing Linux-only admission')
    quoted = distro.replace("'", "''")
    script = r"""
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$distroEntries=@(Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Lxss\*' | Where-Object {$_.DistributionName -eq 'DISTRO'})
if($distroEntries.Count -ne 1) {throw 'Cannot identify one WSL backing disk'}
$base=$distroEntries[0].BasePath
if($base.StartsWith('\\?\')) {$base=$base.Substring(4)}
$root=[IO.Path]::GetPathRoot([IO.Path]::GetFullPath($base))
$drive=[IO.DriveInfo]::new($root)
if(-not $drive.IsReady) {throw 'Backing drive is unavailable'}
[pscustomobject]@{distribution=$distroEntries[0].DistributionName;backingDrive=$root;availableBytes=$drive.AvailableFreeSpace} | ConvertTo-Json -Compress
""".replace('DISTRO', quoted)
    encoded = base64.b64encode(script.encode('utf-16le')).decode('ascii')
    output = run(['/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe',
                  '-NoProfile', '-NonInteractive', '-EncodedCommand', encoded])
    result = json.loads(output)
    if result.get('distribution') != distro or not isinstance(result.get('availableBytes'), int):
        raise RuntimeError('Invalid Windows capacity measurement')
    return result


def single_record(output, key, subject):
    # require one record
    payload = json.loads(output)
    records = payload.get(key)
    if not isinstance(records, list) or len(records) != 1 or not isinstance(records[0], dict):
        raise RuntimeError(f'Cannot identify one {subject}')
    return records[0]


def target_mount_record(output, target):
    # validate target mount
    record = single_record(output, 'filesystems', 'target filesystem')
    option_text = record.get('options')
    options = set(option_text.split(',')) if isinstance(option_text, str) else set()
    device = record.get('source')
    uuid = record.get('uuid')
    if record.get('target') != str(target):
        raise RuntimeError('Target root is not the filesystem mountpoint')
    # require writable ext4
    if record.get('fstype') != 'ext4' or 'rw' not in options:
        raise RuntimeError('Target filesystem must be writable ext4')
    # require a loop device
    if not isinstance(device, str) or not re.fullmatch(r'/dev/loop[0-9]+', device):
        raise RuntimeError('Target filesystem must use a loop device')
    # require stable identity
    if not isinstance(uuid, str) or not re.fullmatch(r'[A-Za-z0-9.-]{1,128}', uuid):
        raise RuntimeError('Target filesystem UUID is unavailable')
    return device, uuid


def loop_backing_file(output, device):
    # validate loop binding
    record = single_record(output, 'loopdevices', 'loop-device binding')
    backing = record.get('back-file')
    if record.get('name') != device:
        raise RuntimeError('Loop-device identity changed during admission')
    # require an absolute path
    if not isinstance(backing, str) or not Path(backing).is_absolute():
        raise RuntimeError('Loop backing file is unavailable')
    return Path(backing)


def backing_mount_record(output, backing_file):
    # validate host mount
    record = single_record(output, 'filesystems', 'backing-file filesystem')
    target = record.get('target')
    option_text = record.get('options')
    options = set(option_text.split(',')) if isinstance(option_text, str) else set()
    source = record.get('source')
    if not isinstance(target, str) or not Path(target).is_absolute():
        raise RuntimeError('Backing-file filesystem mountpoint is invalid')
    mountpoint = Path(target)
    # identify drvfs
    drvfs = record.get('fstype') == 'drvfs' or (
        record.get('fstype') == '9p' and any(option.startswith('aname=drvfs;') for option in options)
    )
    if not drvfs or 'rw' not in options:
        raise RuntimeError('Loop backing file must be on writable drvfs')
    # derive the drive
    match = re.fullmatch(r'([A-Za-z]):\\', source or '')
    if not match:
        raise RuntimeError('Cannot derive the Windows drive for the loop backing file')
    # require the backing mount
    try:
        backing_file.relative_to(mountpoint)
    except ValueError as error:
        raise RuntimeError('Loop backing file is outside its reported filesystem') from error
    return mountpoint, f'{match.group(1).upper()}:\\'


def validated_windows_drive(result, expected_drive):
    # validate physical capacity
    if not isinstance(result, dict):
        raise TypeError('Invalid Windows backing-drive response')
    available = result.get('availableBytes')
    total = result.get('totalBytes')
    if result.get('backingDrive', '').upper() != expected_drive.upper() or result.get('isReady') is not True:
        raise RuntimeError('Invalid Windows backing-drive identity')
    # validate capacity values
    if (not isinstance(available, int) or isinstance(available, bool) or available < 0 or
            not isinstance(total, int) or isinstance(total, bool) or total <= 0 or available > total):
        raise RuntimeError('Invalid Windows backing-drive capacity measurement')
    return result


def windows_drive_space(drive_root):
    # measure the derived drive
    quoted = drive_root.replace("'", "''")
    script = r"""
$ErrorActionPreference='Stop'
$ProgressPreference='SilentlyContinue'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$requested='DRIVE_ROOT'
$root=[IO.Path]::GetPathRoot([IO.Path]::GetFullPath($requested))
$drive=[IO.DriveInfo]::new($root)
if(-not $drive.IsReady) {throw 'Backing drive is unavailable'}
[pscustomobject]@{backingDrive=$drive.RootDirectory.FullName;isReady=$drive.IsReady;availableBytes=$drive.AvailableFreeSpace;totalBytes=$drive.TotalSize} | ConvertTo-Json -Compress
""".replace('DRIVE_ROOT', quoted)
    encoded = base64.b64encode(script.encode('utf-16le')).decode('ascii')
    output = run(['/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe',
                  '-NoProfile', '-NonInteractive', '-EncodedCommand', encoded])
    return validated_windows_drive(json.loads(output), drive_root)


def target_backing_space(target, docker_root):
    # require a canonical target
    if not target.is_absolute() or target.resolve(strict=True) != target or not target.is_dir():
        raise RuntimeError('Target root must be an existing canonical directory without symlinks')
    target_stat = os.stat(target)
    # require storage isolation
    if target_stat.st_dev == os.stat(docker_root).st_dev:
        raise RuntimeError('Target root must use a filesystem separate from Docker root')
    target_output = run(['/usr/bin/findmnt', '--json', '--target', str(target),
                         '--output', 'TARGET,SOURCE,FSTYPE,OPTIONS,UUID'])
    device, uuid = target_mount_record(target_output, target)
    device_path = Path(device)
    # validate the device node
    device_stat = os.stat(device_path)
    if (device_path.resolve(strict=True) != device_path or not stat.S_ISBLK(device_stat.st_mode) or
            target_stat.st_dev != device_stat.st_rdev):
        raise RuntimeError('Target loop device is not a canonical block device')
    loop_output = run(['/usr/sbin/losetup', '--json', '--list',
                       '--output', 'NAME,BACK-FILE', device])
    backing_file = loop_backing_file(loop_output, device)
    # validate the backing file
    if backing_file.resolve(strict=True) != backing_file or not stat.S_ISREG(os.stat(backing_file).st_mode):
        raise RuntimeError('Loop backing file must be an existing regular file without symlinks')
    backing_output = run(['/usr/bin/findmnt', '--json', '--target', str(backing_file),
                          '--output', 'TARGET,SOURCE,FSTYPE,OPTIONS,UUID'])
    backing_mount, drive_root = backing_mount_record(backing_output, backing_file)
    # validate the backing mount
    if (backing_mount.resolve(strict=True) != backing_mount or not backing_mount.is_dir() or
            os.stat(backing_mount).st_dev != os.stat(backing_file).st_dev):
        raise RuntimeError('Loop backing filesystem identity is invalid')
    stats = os.statvfs(target)
    return {
        'targetRoot': str(target),
        'targetDevice': device,
        'targetUuid': uuid,
        'targetFilesystemType': 'ext4',
        'targetLinuxAvailableBytes': stats.f_bavail * stats.f_frsize,
        'targetBackingFileSha256': hashlib.sha256(os.fsencode(str(backing_file))).hexdigest(),
        'targetBackingFilesystem': {'mountpoint': str(backing_mount), 'type': 'drvfs',
                                    'source': drive_root},
        'windowsBackingDrive': windows_drive_space(drive_root),
    }


def source_measurement_path(source, source_volume, docker_root, target_root=None):
    # validate docker identity
    if not isinstance(source, dict) or not isinstance(source.get('Mountpoint'), str):
        raise TypeError('Unsupported source volume identity or storage driver')
    mount = Path(source['Mountpoint'])
    expected = docker_root / 'volumes' / source_volume / '_data'
    if source.get('Name') != source_volume or source.get('Driver') != 'local' or mount != expected:
        raise RuntimeError('Unsupported source volume identity or storage driver')
    options = source.get('Options')
    # preserve physical volumes
    if options is None:
        # validate physical mount
        if mount.resolve() != mount or os.stat(mount).st_dev != os.stat(docker_root).st_dev:
            raise RuntimeError('Unexpected source mount or filesystem')
        return mount, {'sourceStorageType': 'docker-volume'}
    # require exact bind options
    if (not isinstance(options, dict) or set(options) != {'type', 'o', 'device'} or
            options.get('type') != 'none' or options.get('o') != 'bind'):
        raise RuntimeError('Unsupported local volume options')
    device_text = options.get('device')
    if not isinstance(device_text, str):
        raise TypeError('Local bind source device is invalid')
    device = Path(device_text)
    # require the proven target
    if target_root is None:
        raise RuntimeError('Local bind source requires a verified target root')
    # require a canonical directory
    if not device.is_absolute() or device.resolve(strict=True) != device or not device.is_dir():
        raise RuntimeError('Local bind source must be an existing canonical directory without symlinks')
    # require a strict descendant
    try:
        relative = device.relative_to(target_root)
    except ValueError as error:
        raise RuntimeError('Local bind source is outside the verified target root') from error
    # exclude the target root
    if relative == Path('.'):
        raise RuntimeError('Local bind source must be below the verified target root')
    # require the target filesystem
    if os.stat(device).st_dev != os.stat(target_root).st_dev:
        raise RuntimeError('Local bind source crosses the verified target filesystem')
    return device, {
        'sourceStorageType': 'local-bind',
        'sourceBindDeviceSha256': hashlib.sha256(os.fsencode(str(device))).hexdigest(),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-volume', required=True)
    parser.add_argument('--copies', required=True, type=int)
    parser.add_argument('--reserve-gib', type=int, default=32)
    parser.add_argument('--target-root')
    args = parser.parse_args()
    if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9_.-]{0,254}', args.source_volume):
        parser.error('Invalid Docker volume name')
    if not 1 <= args.copies <= 100 or not 4 <= args.reserve_gib <= 1024:
        parser.error('Copies must be 1..100 and reserve must be 4..1024 GiB')
    if os.geteuid() != 0:
        distro = os.environ.get('WSL_DISTRO_NAME')
        if not distro or 'microsoft' not in Path('/proc/sys/kernel/osrelease').read_text().lower():
            raise RuntimeError('Root access is required to measure Docker source storage')
        # The Windows owner can read their WSL Docker metadata through WSL root.
        # This performs the same read-only admission, without changing volume data.
        return subprocess.run([
            '/mnt/c/Windows/System32/wsl.exe', '--distribution', distro,
            '--user', 'root', '--exec', '/usr/bin/python3',
            str(Path(__file__).resolve()), *sys.argv[1:],
        ], timeout=180, check=False).returncode
    docker = ['/usr/bin/docker', '--host', 'unix:///var/run/docker.sock']
    root = Path(run(docker + ['info', '--format', '{{.DockerRootDir}}']))
    source = json.loads(run(docker + ['volume', 'inspect', args.source_volume]))[0]
    wsl = 'microsoft' in Path('/proc/sys/kernel/osrelease').read_text().lower()
    if wsl and os.stat(root).st_dev != os.stat('/').st_dev:
        raise RuntimeError('Docker is on a separate filesystem; backing-drive mapping requires review')
    # select the proven target
    if args.target_root:
        # require wsl target proof
        if not wsl:
            raise RuntimeError('Target-root admission is supported only for verified WSL storage')
        target_root = Path(args.target_root)
        target = target_backing_space(target_root, root)
    else:
        target_root = None
        target = None
    measurement_path, source_evidence = source_measurement_path(
        source, args.source_volume, root, target_root,
    )
    actual = int(run(['/usr/bin/du', '-s', '--block-size=1', str(measurement_path)]).split()[0])
    apparent = int(run([
        '/usr/bin/du', '-s', '--block-size=1', '--apparent-size', str(measurement_path),
    ]).split()[0])
    budget = max(actual, apparent)
    # use target capacity
    if target:
        linux_free = target['targetLinuxAvailableBytes']
        host = target['windowsBackingDrive']
    else:
        stats = os.statvfs(root)
        linux_free = stats.f_bavail * stats.f_frsize
        host = windows_backing_space(os.environ.get('WSL_DISTRO_NAME')) if wsl else None
    required, admitted = evaluate(budget, args.copies, args.reserve_gib * GIB,
                                 linux_free, host['availableBytes'] if host else None, wsl)
    receipt = {'capturedAt':datetime.now(timezone.utc).isoformat(), 'sourceVolume':args.source_volume,
               'copiesPlanned':args.copies, 'sourceActualBytes':actual, 'sourceApparentBytes':apparent,
               'safetyReserveBytes':args.reserve_gib * GIB, 'requiredBytes':required,
               'linuxAvailableBytes':linux_free, 'windowsBackingDrive':host,
               'admitted':admitted}
    receipt.update(source_evidence)
    # bind target evidence
    if target:
        receipt.update({key:value for key, value in target.items()
                        if key not in ('targetLinuxAvailableBytes', 'windowsBackingDrive')})
    print(json.dumps(receipt, indent=2), flush=True)
    if not admitted:
        print('STORAGE ADMISSION DENIED: do not allocate or copy fixture volumes.', file=sys.stderr)
        return 2
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, TypeError, ValueError, RuntimeError, subprocess.SubprocessError, KeyError) as error:
        print(f'STORAGE ADMISSION FAILED CLOSED: {error}', file=sys.stderr)
        raise SystemExit(2)
