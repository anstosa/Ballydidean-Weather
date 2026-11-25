//! Native dual-layer capacity evidence for the fixed WSL maintenance workstation.

use std::ffi::CString;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::io::Read;
use std::mem::MaybeUninit;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crate::{measure_native_maintenance_backing_census, native_maintenance_backing_roots};

const EXT4_MAGIC: i64 = 0xef53;
const FIXED_WSL_DISTRIBUTION: &str = "Ubuntu";
const FIXED_WSL_HOME: &str = "/home/ubuntu";
const WINDOWS_CAPACITY_OUTPUT_MAXIMUM_BYTES: usize = 512;
const WINDOWS_CAPACITY_TIMEOUT: Duration = Duration::from_secs(15);
const WINDOWS_POWERSHELL: &str = "/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe";
const WINDOWS_WSL: &str = "/mnt/c/Windows/System32/wsl.exe";
const WINDOWS_CAPACITY_COMMAND: &str = concat!(
    "$ErrorActionPreference='Stop'; ",
    "$distros=@(Get-ChildItem HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Lxss | ",
    "ForEach-Object { Get-ItemProperty $_.PSPath } | ",
    "Where-Object { $_.DistributionName -eq 'Ubuntu' }); ",
    "if ($distros.Count -ne 1) { throw 'wsl distro mapping unavailable' }; ",
    "$vhd=[IO.Path]::GetFullPath((Join-Path $distros[0].BasePath 'ext4.vhdx')); ",
    "if (-not $vhd.StartsWith('C:\\',[StringComparison]::OrdinalIgnoreCase) -or ",
    "-not (Test-Path -LiteralPath $vhd -PathType Leaf)) { throw 'wsl vhd is not c backed' }; ",
    "$drive=Get-CimInstance Win32_LogicalDisk -Filter \"DeviceID='C:'\"; ",
    "if ($null -eq $drive) { throw 'missing drive' }; ",
    "[Console]::Out.Write(('weather-backing-c/v1|{0}|{1}|ubuntu-vhd-on-c' -f ",
    "[uint64]$drive.Size,[uint64]$drive.FreeSpace))",
);

/// The only admitted physical root behind the fixed Ubuntu WSL distribution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceBackingRootKind {
    BackingC,
}

impl NativeMaintenanceBackingRootKind {
    /// Project the exact predecessor response spelling.
    pub const fn as_str(self) -> &'static str {
        // retain a closed typed root rather than accepting caller-supplied evidence text
        match self {
            Self::BackingC => "backing_c",
        }
    }
}

/// Exact backing allocation and free-capacity evidence used by archive admission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceBackingCapacity {
    pub allocated_bytes: u64,
    pub free_bytes: u64,
    pub free_inodes: u64,
    pub root_kind: NativeMaintenanceBackingRootKind,
}

/// Dual-layer native ext4 and physical Windows backing-capacity evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceCapacityEvidence {
    pub backing: NativeMaintenanceBackingCapacity,
    pub ext4_free_bytes: u64,
    pub ext4_free_inodes: u64,
    pub ext4_block_size: u64,
    pub windows_total_bytes: u64,
}

/// Closed failure categories for fixed-host admission and bounded capacity probes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceBackingCapacityError {
    BackingFilesystemInvalid,
    BackingCProbeUnavailable,
    WslIdentityProbeInvalid,
    BackingCProbeInvalid,
    BackingCensusInvalid,
    TransportSpawnRefused,
    TransportOutputRefused,
    TransportTimeout,
    TransportRemoteRefused,
}

impl Display for NativeMaintenanceBackingCapacityError {
    /// Preserve the predecessor's bounded failure vocabulary without host details.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // project only fixed source diagnostics
        formatter.write_str(match self {
            Self::BackingFilesystemInvalid => "archive backing filesystem is invalid",
            Self::BackingCProbeUnavailable => "archive_backing_c_probe_unavailable",
            Self::WslIdentityProbeInvalid => "archive_wsl_identity_probe_invalid",
            Self::BackingCProbeInvalid => "archive_backing_c_probe_invalid",
            Self::BackingCensusInvalid => "archive backing census is invalid",
            Self::TransportSpawnRefused => "transport_spawn_refused",
            Self::TransportOutputRefused => "transport_output_refused",
            Self::TransportTimeout => "transport_timeout",
            Self::TransportRemoteRefused => "transport_remote_refused",
        })
    }
}

impl std::error::Error for NativeMaintenanceBackingCapacityError {}

/// Stateless production owner for the fixed host and bounded process effects.
#[derive(Clone, Copy, Debug, Default)]
pub struct NativeMaintenanceBackingCapacityOwner;

impl NativeMaintenanceBackingCapacityOwner {
    /// Construct the production capacity owner without observing the host.
    pub const fn new() -> Self {
        Self
    }

    /// Measure the exact ext4, backing-C, and deduplicated legacy census evidence.
    pub fn measure(
        &mut self,
    ) -> Result<NativeMaintenanceCapacityEvidence, NativeMaintenanceBackingCapacityError> {
        measure_native_maintenance_capacity_evidence()
    }
}

/// Measure the complete fixed-workstation source evidence without allocating storage.
pub fn measure_native_maintenance_capacity_evidence()
-> Result<NativeMaintenanceCapacityEvidence, NativeMaintenanceBackingCapacityError> {
    let home = Path::new(FIXED_WSL_HOME);
    let details = fs::symlink_metadata(home)
        .map_err(|_| NativeMaintenanceBackingCapacityError::BackingFilesystemInvalid)?;
    let canonical = fs::canonicalize(home)
        .map_err(|_| NativeMaintenanceBackingCapacityError::BackingFilesystemInvalid)?;
    let filesystem = statfs(home)?;
    // safety: getuid has no preconditions and only reads the current process identity
    let current_uid = unsafe { libc::getuid() };
    // grant workstation authority only to the fixed current-user native ext4 home
    if !details.is_dir()
        || details.file_type().is_symlink()
        || details.uid() != current_uid
        || canonical != home
        || filesystem.kind != EXT4_MAGIC
    {
        return Err(NativeMaintenanceBackingCapacityError::BackingFilesystemInvalid);
    }
    let windows = measure_backing_c_capacity(details.dev(), details.ino())?;
    let roots = native_maintenance_backing_roots(home);
    let census = measure_native_maintenance_backing_census(&roots, details.dev())
        .map_err(|_| NativeMaintenanceBackingCapacityError::BackingCensusInvalid)?;
    let ext4_free_bytes = filesystem
        .available_blocks
        .checked_mul(filesystem.block_size)
        .ok_or(NativeMaintenanceBackingCapacityError::BackingFilesystemInvalid)?;
    Ok(NativeMaintenanceCapacityEvidence {
        backing: NativeMaintenanceBackingCapacity {
            allocated_bytes: census.allocated_bytes,
            free_bytes: windows.free_bytes,
            free_inodes: filesystem.free_inodes,
            root_kind: NativeMaintenanceBackingRootKind::BackingC,
        },
        ext4_free_bytes,
        ext4_free_inodes: filesystem.free_inodes,
        ext4_block_size: filesystem.block_size,
        windows_total_bytes: windows.total_bytes,
    })
}

/// Measure only the source's exported physical backing-C capacity projection.
pub fn measure_native_maintenance_backing_capacity()
-> Result<NativeMaintenanceBackingCapacity, NativeMaintenanceBackingCapacityError> {
    Ok(measure_native_maintenance_capacity_evidence()?.backing)
}

/// One admitted Linux filesystem sample retaining the filesystem identity field.
struct NativeMaintenanceStatfs {
    kind: i64,
    available_blocks: u64,
    block_size: u64,
    free_inodes: u64,
}

/// Sample one fixed path through the approved libc `statfs(2)` boundary.
fn statfs(path: &Path) -> Result<NativeMaintenanceStatfs, NativeMaintenanceBackingCapacityError> {
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| NativeMaintenanceBackingCapacityError::BackingFilesystemInvalid)?;
    let mut filesystem = MaybeUninit::<libc::statfs>::uninit();
    // safety: the nul-terminated path remains live and libc initializes the complete output
    let status = unsafe { libc::statfs(path.as_ptr(), filesystem.as_mut_ptr()) };
    // reject every failed native sample before reading its output
    if status != 0 {
        return Err(NativeMaintenanceBackingCapacityError::BackingFilesystemInvalid);
    }
    // safety: a successful statfs call initialized the complete output
    let filesystem = unsafe { filesystem.assume_init() };
    let kind = filesystem.f_type;
    let block_size = u64::try_from(filesystem.f_bsize)
        .map_err(|_| NativeMaintenanceBackingCapacityError::BackingFilesystemInvalid)?;
    Ok(NativeMaintenanceStatfs {
        kind,
        available_blocks: filesystem.f_bavail,
        block_size,
        free_inodes: filesystem.f_ffree,
    })
}

/// Physical Windows capacity decoded from one exact fixed response.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WindowsBackingCapacity {
    free_bytes: u64,
    total_bytes: u64,
}

/// Bind the fixed Ubuntu home inode to its C-backed VHD and measure physical free space.
fn measure_backing_c_capacity(
    home_device: u64,
    home_inode: u64,
) -> Result<WindowsBackingCapacity, NativeMaintenanceBackingCapacityError> {
    // require both literal Windows executables before starting either probe
    for program in [WINDOWS_WSL, WINDOWS_POWERSHELL] {
        require_literal_program(Path::new(program))?;
    }
    let identity = run_bounded(
        Path::new(WINDOWS_WSL),
        &[
            "-d",
            FIXED_WSL_DISTRIBUTION,
            "--exec",
            "/usr/bin/stat",
            "-Lc",
            "%d|%i",
            FIXED_WSL_HOME,
        ],
    )?;
    // reject a substituted distribution before requesting physical capacity
    if !valid_identity_output(&identity, home_device, home_inode) {
        return Err(NativeMaintenanceBackingCapacityError::WslIdentityProbeInvalid);
    }
    let output = run_bounded(
        Path::new(WINDOWS_POWERSHELL),
        &[
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            WINDOWS_CAPACITY_COMMAND,
        ],
    )?;
    parse_backing_c_output(&output)
        .ok_or(NativeMaintenanceBackingCapacityError::BackingCProbeInvalid)
}

/// Require one ordinary fixed executable without symlink or realpath substitution.
fn require_literal_program(program: &Path) -> Result<(), NativeMaintenanceBackingCapacityError> {
    let details = fs::symlink_metadata(program)
        .map_err(|_| NativeMaintenanceBackingCapacityError::BackingCProbeUnavailable)?;
    let canonical = fs::canonicalize(program)
        .map_err(|_| NativeMaintenanceBackingCapacityError::BackingCProbeUnavailable)?;
    // preserve the source's literal ordinary-file and no-link admission
    if !details.is_file() || details.file_type().is_symlink() || canonical != program {
        return Err(NativeMaintenanceBackingCapacityError::BackingCProbeUnavailable);
    }
    Ok(())
}

/// Execute one exact process with separate 512-byte stdout and stderr ceilings.
fn run_bounded(
    program: &Path,
    arguments: &[&str],
) -> Result<Vec<u8>, NativeMaintenanceBackingCapacityError> {
    let mut child = Command::new(program)
        .args(arguments)
        .env_clear()
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| NativeMaintenanceBackingCapacityError::TransportSpawnRefused)?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(NativeMaintenanceBackingCapacityError::TransportSpawnRefused);
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(NativeMaintenanceBackingCapacityError::TransportSpawnRefused);
    };
    let overflow = Arc::new(AtomicBool::new(false));
    let stdout_overflow = Arc::clone(&overflow);
    let stderr_overflow = Arc::clone(&overflow);
    thread::scope(|scope| {
        // drain both pipes concurrently so a bounded diagnostic cannot block stdout
        let stdout = scope.spawn(move || read_bounded(stdout, stdout_overflow));
        let stderr = scope.spawn(move || read_bounded(stderr, stderr_overflow));
        let deadline = Instant::now() + WINDOWS_CAPACITY_TIMEOUT;
        let terminal = loop {
            // terminate promptly once either independent stream exceeds its source ceiling
            if overflow.load(Ordering::Acquire) {
                let _ = child.kill();
                let _ = child.wait();
                break Err(NativeMaintenanceBackingCapacityError::TransportOutputRefused);
            }
            // preserve the source's fixed poll-interval process deadline
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break Err(NativeMaintenanceBackingCapacityError::TransportTimeout);
            }
            // poll without relinquishing ownership of the child before either bounded reader ends
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(NativeMaintenanceBackingCapacityError::TransportRemoteRefused);
                }
            }
        };
        let output = stdout
            .join()
            .map_err(|_| NativeMaintenanceBackingCapacityError::TransportRemoteRefused)??;
        let _diagnostic = stderr
            .join()
            .map_err(|_| NativeMaintenanceBackingCapacityError::TransportRemoteRefused)??;
        let status = terminal?;
        // publish stdout only after both streams stayed bounded and the child exited normally
        if !status.success() {
            return Err(NativeMaintenanceBackingCapacityError::TransportRemoteRefused);
        }
        Ok(output)
    })
}

/// Drain one process pipe while retaining no bytes beyond its independent ceiling.
fn read_bounded(
    mut stream: impl Read,
    overflow: Arc<AtomicBool>,
) -> Result<Vec<u8>, NativeMaintenanceBackingCapacityError> {
    let mut bytes = Vec::with_capacity(WINDOWS_CAPACITY_OUTPUT_MAXIMUM_BYTES);
    let mut chunk = [0_u8; 256];
    // continue until EOF or the fixed output ceiling is crossed
    loop {
        let count = stream
            .read(&mut chunk)
            .map_err(|_| NativeMaintenanceBackingCapacityError::TransportRemoteRefused)?;
        // finish only on an ordinary pipe EOF
        if count == 0 {
            return Ok(bytes);
        }
        // set the shared terminal flag before discarding an oversized chunk
        if bytes.len().saturating_add(count) > WINDOWS_CAPACITY_OUTPUT_MAXIMUM_BYTES {
            overflow.store(true, Ordering::Release);
            return Err(NativeMaintenanceBackingCapacityError::TransportOutputRefused);
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

/// Admit only the exact decimal native-device and inode line from fixed Ubuntu.
fn valid_identity_output(output: &[u8], device: u64, inode: u64) -> bool {
    let expected_lf = format!("{device}|{inode}\n");
    let expected_crlf = format!("{device}|{inode}\r\n");
    output == expected_lf.as_bytes() || output == expected_crlf.as_bytes()
}

/// Decode only `weather-backing-c/v1|total|free|ubuntu-vhd-on-c` ASCII.
fn parse_backing_c_output(output: &[u8]) -> Option<WindowsBackingCapacity> {
    let output = std::str::from_utf8(output).ok()?;
    // split exactly the four fields admitted by the predecessor's anchored expression
    let mut fields = output.split('|');
    let version = fields.next()?;
    let total = fields.next()?;
    let free = fields.next()?;
    let binding = fields.next()?;
    // reject extra separators and every localized or alternate framing
    if fields.next().is_some()
        || version != "weather-backing-c/v1"
        || binding != "ubuntu-vhd-on-c"
        || !decimal(total)
        || !decimal(free)
    {
        return None;
    }
    let total_bytes = total.parse::<u64>().ok()?;
    let free_bytes = free.parse::<u64>().ok()?;
    // require one positive physical capacity and a possible free result
    if total_bytes == 0 || free_bytes > total_bytes {
        return None;
    }
    Some(WindowsBackingCapacity {
        free_bytes,
        total_bytes,
    })
}

/// Match one nonempty ASCII decimal field exactly like `[0-9]+`.
fn decimal(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
#[path = "native_maintenance_backing_capacity_test.rs"]
mod tests;
