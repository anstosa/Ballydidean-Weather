//! Native isolated fit execution over verified immutable code and data snapshots.

use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::native_maintenance_fit_candidate::{
    NativeMaintenanceFitCandidate, admit_native_maintenance_fit_candidate,
};
use crate::native_maintenance_fit_runtime_readiness::{
    NativeMaintenanceFitRuntimeManifest, NativeMaintenanceFitRuntimeProcess,
    NativeMaintenanceFitRuntimeReadiness,
};
use crate::native_maintenance_fit_snapshot::{
    NativeMaintenanceFitFamily, NativeMaintenanceFitSnapshotKind, NativeMaintenanceFitSnapshots,
    inspect_native_maintenance_fit_snapshot,
};
use crate::{
    NativeMaintenanceCaptureReleaseGeneration,
    native_maintenance_retirement_files_v2::format_digest,
};

const LOG_MAX: usize = 1024 * 1024;
const SYSTEMD_RUN: &str = "/usr/bin/systemd-run";
const SYSTEMCTL: &str = "/usr/bin/systemctl";
const BWRAP: &str = "/usr/bin/bwrap";

/// Digest-only bounded process log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceFitLogReport {
    pub bytes: usize,
    pub label: &'static str,
    pub overflow: bool,
    pub sha256: String,
}

/// Complete isolated execution receipt before controller projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceSandboxFitReceipt {
    pub candidate: NativeMaintenanceFitCandidate,
    pub family: NativeMaintenanceFitFamily,
    pub code_snapshot_sha256: String,
    pub input_snapshot_sha256: String,
    pub runtime_readiness_sha256: String,
    pub stderr: NativeMaintenanceFitLogReport,
    pub stdout: NativeMaintenanceFitLogReport,
}

/// Closed execution failures carry no raw fitter output.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceFitSandboxError {
    Runtime,
    Snapshot,
    Process,
    ResourceRefused,
    LogLimit,
    Candidate,
}

struct TransientUnit(String);

impl TransientUnit {
    /// Issue an independently random service name for this execution only.
    fn new() -> Result<Self, NativeMaintenanceFitSandboxError> {
        let mut entropy = [0_u8; 16];
        getrandom::fill(&mut entropy).map_err(|_| NativeMaintenanceFitSandboxError::Process)?;
        let name = entropy
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        Ok(Self(format!("weather-adjustment-fit-{name}.service")))
    }

    /// Stop only the exact issued unit without retaining command output.
    fn kill(&self) {
        let mut command = Command::new(SYSTEMCTL);
        command
            .args([
                "--user",
                "kill",
                "--kill-whom=all",
                "--signal=KILL",
                &self.0,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        fixed_environment(&mut command);
        if let Ok(mut child) = command.spawn() {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(20))
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
    }
}

impl Drop for TransientUnit {
    /// Stop the owned fit cgroup before the caller closes the private snapshots.
    fn drop(&mut self) {
        self.kill();
    }
}

/// Execute only the native fit generation after recapturing the fixed source readiness.
pub fn execute_native_maintenance_fit_sandbox(
    snapshots: &NativeMaintenanceFitSnapshots,
    readiness: &mut NativeMaintenanceFitRuntimeReadiness<NativeMaintenanceFitRuntimeProcess>,
    expected_readiness_sha256: &str,
) -> Result<NativeMaintenanceSandboxFitReceipt, NativeMaintenanceFitSandboxError> {
    use NativeMaintenanceFitSandboxError as Error;
    let verified = readiness.capture().map_err(|_| Error::Runtime)?;
    // bind exact installed code, runtime and both live snapshot identities
    if verified.readiness_sha256 != expected_readiness_sha256
        || verified.installed_closure.release.source_commit != snapshots.code_proof().source_commit
        || verified.installed_closure.release.manifest_sha256
            != snapshots.code_proof().manifest_sha256
        || snapshots.code_proof().generation != NativeMaintenanceCaptureReleaseGeneration::Native
        || inspect_native_maintenance_fit_snapshot(
            snapshots.code_root(),
            NativeMaintenanceFitSnapshotKind::Code,
        )
        .map_err(|_| Error::Snapshot)?
            != *snapshots.code_snapshot()
        || inspect_native_maintenance_fit_snapshot(
            snapshots.input_root(),
            NativeMaintenanceFitSnapshotKind::Input,
        )
        .map_err(|_| Error::Snapshot)?
            != *snapshots.input_snapshot()
    {
        return Err(Error::Snapshot);
    }
    let unit = TransientUnit::new()?;
    let arguments = build_arguments(snapshots, &verified.manifest, &unit.0)?;
    let mut command = Command::new(SYSTEMD_RUN);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    fixed_environment(&mut command);
    let mut child = command.spawn().map_err(|_| Error::Process)?;
    let stdout = child.stdout.take().ok_or(Error::Process)?;
    let stderr = child.stderr.take().ok_or(Error::Process)?;
    let exceeded = AtomicBool::new(false);
    // read both independent pipes concurrently while supervising the bounded service
    let (stdout, stderr, successful) = thread::scope(|scope| {
        let a = scope.spawn(|| capture(stdout, "stdout", true, &exceeded));
        let b = scope.spawn(|| capture(stderr, "stderr", false, &exceeded));
        let deadline = Instant::now() + Duration::from_secs(4 * 60 * 60 + 60);
        let successful = loop {
            if exceeded.load(Ordering::Relaxed) || Instant::now() >= deadline {
                unit.kill();
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            match child.try_wait() {
                Ok(Some(status)) => break status.success(),
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(_) => {
                    unit.kill();
                    let _ = child.kill();
                    let _ = child.wait();
                    break false;
                }
            }
        };
        Ok::<_, Error>((
            a.join().map_err(|_| Error::Process)??,
            b.join().map_err(|_| Error::Process)??,
            successful,
        ))
    })?;
    if stdout.0.overflow {
        return Err(Error::ResourceRefused);
    }
    if stderr.0.overflow {
        return Err(Error::LogLimit);
    }
    if !successful {
        return Err(Error::Process);
    }
    let candidate = admit_native_maintenance_fit_candidate(&stdout.1).ok_or(Error::Candidate)?;
    Ok(NativeMaintenanceSandboxFitReceipt {
        candidate,
        family: snapshots.family(),
        code_snapshot_sha256: snapshots.code_snapshot().root_sha256.clone(),
        input_snapshot_sha256: snapshots.input_snapshot().root_sha256.clone(),
        runtime_readiness_sha256: verified.readiness_sha256,
        stderr: stderr.0,
        stdout: stdout.0,
    })
}

/// Project the fixed inherited environment for the user service manager.
fn fixed_environment(command: &mut Command) {
    // getuid has no pointer arguments and no memory side effects
    let uid = unsafe { libc::getuid() };
    command.env_clear().envs([
        ("HOME", "/home/ubuntu".to_owned()),
        ("LC_ALL", "C.UTF-8".to_owned()),
        ("PATH", "/usr/bin:/bin".to_owned()),
        ("XDG_RUNTIME_DIR", format!("/run/user/{uid}")),
    ]);
}

/// Construct the source-reviewed fixed process envelope with native fitter entrypoints.
fn build_arguments(
    snapshots: &NativeMaintenanceFitSnapshots,
    manifest: &NativeMaintenanceFitRuntimeManifest,
    unit: &str,
) -> Result<Vec<String>, NativeMaintenanceFitSandboxError> {
    // native execution must never consume the predecessor interpreter readiness receipt
    let NativeMaintenanceFitRuntimeManifest::Native(manifest) = manifest else {
        return Err(NativeMaintenanceFitSandboxError::Runtime);
    };
    let mut args: Vec<String> = ["--user", "--wait", "--collect", "--quiet", "--pipe"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    args.push(format!("--unit={unit}"));
    // keep the original fitter resource envelope unchanged
    for property in [
        "MemoryMax=8G",
        "MemorySwapMax=0",
        "CPUQuota=800%",
        "TasksMax=64",
        "RuntimeMaxSec=4h",
        "LimitFSIZE=67108864",
        "LimitNOFILE=64",
        "Nice=10",
        "IOSchedulingClass=best-effort",
        "IOSchedulingPriority=7",
    ] {
        args.push(format!("--property={property}"));
    }
    let mut extend = |values: &[&str]| args.extend(values.iter().map(|value| (*value).to_owned()));
    extend(&[
        BWRAP,
        "--die-with-parent",
        "--new-session",
        "--unshare-user",
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-net",
        "--unshare-uts",
        "--unshare-cgroup",
        "--clearenv",
        "--cap-drop",
        "ALL",
    ]);
    for directory in [
        "/usr",
        "/lib",
        "/lib64",
        "/home",
        "/home/sandbox",
        "/run",
        "/tmp",
        "/proc",
        "/dev",
        "/runtime",
        "/runtime/xgboost",
        "/runtime/xgboost/lib",
        "/runtime/xgboost_cpu.libs",
        "/runtime/numpy.libs",
        "/input",
        "/input/code",
        "/input/data",
        "/output",
    ] {
        extend(&["--dir", directory]);
    }
    extend(&[
        "--proc",
        "/proc",
        "--dev",
        "/dev",
        "--tmpfs",
        "/run",
        "--tmpfs",
        "/tmp",
        "--perms",
        "0700",
        "--tmpfs",
        "/home/sandbox",
        "--ro-bind",
        "/usr",
        "/usr",
        "--ro-bind",
        "/usr/lib",
        "/lib",
        "--ro-bind",
        "/usr/lib64",
        "/lib64",
        "--size",
        "67108864",
        "--perms",
        "0700",
        "--tmpfs",
        "/output",
        "--ro-bind",
        snapshots
            .code_root()
            .to_str()
            .ok_or(NativeMaintenanceFitSandboxError::Snapshot)?,
        "/input/code",
        "--ro-bind",
        snapshots
            .input_root()
            .to_str()
            .ok_or(NativeMaintenanceFitSandboxError::Snapshot)?,
        "/input/data",
        "--chdir",
        "/input/code",
    ]);
    // mount only the selected fitter’s preexisting numerical library closure
    if snapshots.family() == NativeMaintenanceFitFamily::Rain {
        extend(&[
            "--ro-bind",
            &manifest.xgboost.path,
            "/runtime/xgboost/lib/libxgboost.so",
            "--ro-bind",
            &manifest.openmp.path,
            "/runtime/xgboost_cpu.libs/libgomp-e985bcbb.so.1.0.0",
        ]);
    }
    // both learned fitters use the predecessor’s identical dense reductions
    if snapshots.family() != NativeMaintenanceFitFamily::Wind {
        extend(&[
            "--ro-bind",
            &manifest.openblas.path,
            "/runtime/numpy.libs/libscipy_openblas64_-f48b354e.so",
            "--ro-bind",
            &manifest.gfortran.path,
            "/runtime/numpy.libs/libgfortran-83c28eba-468e71e5.so.5.0.0",
            "--ro-bind",
            &manifest.quadmath.path,
            "/runtime/numpy.libs/libquadmath-2284e583-a9307bba.so.0.0.0",
        ]);
    }
    // preserve the source's numerical single-worker environment
    for (key, value) in [
        ("HOME", "/home/sandbox"),
        ("PATH", "/runtime:/usr/bin:/bin"),
        ("TZ", "UTC"),
        ("LC_ALL", "C.UTF-8"),
        ("OMP_NUM_THREADS", "1"),
        ("OPENBLAS_NUM_THREADS", "1"),
        ("MKL_NUM_THREADS", "1"),
        ("NUMEXPR_NUM_THREADS", "1"),
        ("VECLIB_MAXIMUM_THREADS", "1"),
        ("BLIS_NUM_THREADS", "1"),
        ("PWD", "/input/code"),
    ] {
        extend(&["--setenv", key, value]);
    }
    let executable = format!("/input/code/{}", snapshots.family().entry_relative_path());
    // execute the manifest-bound native fitter with its closed original family selection
    extend(&[&executable, "--fit-only"]);
    if snapshots.family() != NativeMaintenanceFitFamily::Wind {
        extend(&[snapshots.family().as_str()]);
    }
    Ok(args)
}

/// Digest only the original bounded prefix and optionally retain candidate stdout.
fn capture(
    mut reader: impl Read,
    label: &'static str,
    retain: bool,
    exceeded: &AtomicBool,
) -> Result<(NativeMaintenanceFitLogReport, Vec<u8>), NativeMaintenanceFitSandboxError> {
    let mut bytes = 0;
    let mut overflow = false;
    let mut hash = Sha256::new();
    let mut kept = Vec::new();
    let mut buffer = [0; 16 * 1024];
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|_| NativeMaintenanceFitSandboxError::Process)?;
        if count == 0 {
            break;
        }
        let allowed = count.min(LOG_MAX - bytes);
        hash.update(&buffer[..allowed]);
        bytes += allowed;
        if retain {
            kept.extend_from_slice(&buffer[..allowed]);
        }
        if allowed < count {
            overflow = true;
            exceeded.store(true, Ordering::Relaxed);
        }
    }
    Ok((
        NativeMaintenanceFitLogReport {
            bytes,
            label,
            overflow,
            sha256: format_digest(hash.finalize().into()),
        },
        kept,
    ))
}

/// Run the actual native generation for a controller-authorized immutable fit request.
pub fn run_native_maintenance_installed_fit(
    source_commit: &str,
    request: crate::NativeMaintenanceFitRequest<'_>,
) -> Result<crate::NativeMaintenanceFitResult, NativeMaintenanceFitSandboxError> {
    use crate::{
        NativeMaintenanceCaptureFamily, NativeMaintenanceCaptureNativeInstallation,
        NativeMaintenanceFitSnapshotOwner, NativeWirePrimitives,
    };
    use NativeMaintenanceFitSandboxError as Error;
    use weather_edge::ArchiveWirePrimitives;
    let family = match request.family {
        NativeMaintenanceCaptureFamily::Temperature => NativeMaintenanceFitFamily::Temperature,
        NativeMaintenanceCaptureFamily::Wind => NativeMaintenanceFitFamily::Wind,
        NativeMaintenanceCaptureFamily::Rain => NativeMaintenanceFitFamily::Rain,
    };
    let wire = NativeWirePrimitives;
    let canonical = wire
        .encode_sorted_json(&request.runtime_readiness.value)
        .ok_or(Error::Runtime)?;
    // bind the exact original no-final-LF runtime receipt before allocating snapshots
    if format_digest(Sha256::digest(canonical.strip_suffix(b"\n").ok_or(Error::Runtime)?).into())
        != request.runtime_readiness_sha256
    {
        return Err(Error::Runtime);
    }
    let installed = NativeMaintenanceCaptureNativeInstallation::native_release(source_commit)
        .and_then(|reader| reader.verify())
        .map_err(|_| Error::Runtime)?;
    let owner =
        NativeMaintenanceFitSnapshotOwner::native(&installed).map_err(|_| Error::Snapshot)?;
    let mut readiness = NativeMaintenanceFitRuntimeReadiness::native_installed(source_commit)
        .map_err(|_| Error::Runtime)?;
    let snapshots = owner
        .create_snapshots(family, &request.input.value, &mut NativeWirePrimitives)
        .map_err(|_| Error::Snapshot)?;
    let result = execute_native_maintenance_fit_sandbox(
        &snapshots,
        &mut readiness,
        request.runtime_readiness_sha256,
    );
    // always close both issued snapshots even when the fitter failed
    snapshots.close().map_err(|_| Error::Snapshot)?;
    result.map(|receipt| crate::NativeMaintenanceFitResult {
        candidate_json: receipt.candidate.canonical_bytes,
        candidate_sha256: receipt.candidate.candidate_sha256,
        code_snapshot_sha256: receipt.code_snapshot_sha256,
        contract_version: "adjustment-fit-sandbox/v1".to_owned(),
        family: request.family,
        input_snapshot_sha256: receipt.input_snapshot_sha256,
        runtime_readiness_sha256: receipt.runtime_readiness_sha256,
        stderr: receipt.stderr,
        stdout: receipt.stdout,
    })
}
