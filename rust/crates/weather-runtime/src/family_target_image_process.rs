//! Read an already selected immutable family image through the predecessor's bounded subprocess.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use crate::family_release_images::FamilyReleaseBoundImageRead;
use crate::maintenance_canonical_json::valid_hash;
use crate::{FamilyTargetImageRead, MaintenanceAnchorStatusError};

use MaintenanceAnchorStatusError::{Root, Slot};

const MAXIMUM_IMAGE_MEMBER: usize = 8 * 1024 * 1024;

/// Standard-library process owner for the original read-only immutable image operation.
pub struct NativeFamilyTargetImage {
    image: String,
    program: PathBuf,
}

impl NativeFamilyTargetImage {
    /// Bind the original Docker executable and one exact immutable Weather image reference.
    pub fn native(image: &str) -> Result<Self, MaintenanceAnchorStatusError> {
        Self::for_program(image, Path::new("docker"))
    }

    // expose executable selection only to in-crate disposable process fixtures
    #[cfg(test)]
    pub(crate) fn for_fixture_program(
        image: &str,
        program: &Path,
    ) -> Result<Self, MaintenanceAnchorStatusError> {
        Self::for_program(image, program)
    }

    // keep executable selection separate from image and member operands
    fn for_program(image: &str, program: &Path) -> Result<Self, MaintenanceAnchorStatusError> {
        // preserve the exact two original digest-pinned Weather image names
        if !valid_image_reference(image) {
            return Err(Slot);
        }
        Ok(Self {
            image: image.to_owned(),
            program: program.to_owned(),
        })
    }
}

impl FamilyTargetImageRead for NativeFamilyTargetImage {
    /// Read the exact selected member with the original resource and result bounds.
    fn read_member(
        &mut self,
        path: &str,
        maximum_bytes: usize,
    ) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
        // preserve the original relative member grammar and maximum actual caller bound
        if path.starts_with('/') || path.contains("..") || maximum_bytes > MAXIMUM_IMAGE_MEMBER {
            return Err(Slot);
        }
        let limit = maximum_bytes + 1;
        let image_path = format!("/opt/weather/{path}");
        let mut child = Command::new(&self.program)
            .args([
                "run",
                "--rm",
                "--network",
                "none",
                "--read-only",
                "--pids-limit",
                "32",
                "--memory",
                "128m",
                "--entrypoint",
                "/bin/cat",
                &self.image,
                &image_path,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| Root)?;
        let output = capture_bounded(&mut child, limit)?;
        // keep the predecessor's two-byte minimum and exact requested member ceiling
        if !(2..=maximum_bytes).contains(&output.len()) {
            return Err(Slot);
        }
        Ok(output)
    }
}

impl FamilyReleaseBoundImageRead for NativeFamilyTargetImage {
    /// Retain the exact immutable image selected by the process owner.
    fn image_reference(&self) -> &str {
        &self.image
    }
}

// require a literal immutable digest-pinned Weather server or web image
fn valid_image_reference(value: &str) -> bool {
    let Some(digest) = value
        .strip_prefix("ghcr.io/anstosa/weather-server@sha256:")
        .or_else(|| value.strip_prefix("ghcr.io/anstosa/weather-web@sha256:"))
    else {
        return false;
    };
    valid_hash(digest)
}

enum Captured {
    Output(io::Result<Vec<u8>>),
    Diagnostic(io::Result<()>),
}

// share the original spawnSync combined stdout and stderr buffer limit
fn read_pipe(
    mut stream: impl Read,
    limit: usize,
    total: &AtomicUsize,
    mut output: Option<&mut Vec<u8>>,
) -> io::Result<()> {
    let mut buffer = [0_u8; 8192];
    // read at most the remaining aggregate allowance plus one byte
    loop {
        let remaining = limit.saturating_sub(total.load(Ordering::Relaxed));
        let requested = (remaining + 1).min(buffer.len());
        let count = match stream.read(&mut buffer[..requested]) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        // an empty read ends just this stream
        if count == 0 {
            return Ok(());
        }
        let previous = total.fetch_add(count, Ordering::Relaxed);
        // enforce the actual synchronous Node aggregate cap across both readers
        if previous + count > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "image output exceeds bound",
            ));
        }
        // retain stdout only and discard diagnostic contents
        if let Some(bytes) = output.as_deref_mut() {
            bytes.extend_from_slice(&buffer[..count]);
        }
    }
}

// capture both original pipe bounds and always reap the owned subprocess
fn capture_bounded(
    child: &mut Child,
    limit: usize,
) -> Result<Vec<u8>, MaintenanceAnchorStatusError> {
    // pipe ownership belongs only to this subprocess and its two bounded readers
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Root);
    };
    let (sender, receiver) = mpsc::channel();
    let output_sender = sender.clone();
    let total = AtomicUsize::new(0);
    let combined = &total;
    let captured = thread::scope(|scope| {
        // drain both streams independently while accounting for the original combined bound
        scope.spawn(move || {
            let mut bytes = Vec::with_capacity(limit.min(4096));
            let result = read_pipe(stdout, limit, combined, Some(&mut bytes)).map(|()| bytes);
            let _ = output_sender.send(Captured::Output(result));
        });
        scope.spawn(move || {
            let result = read_pipe(stderr, limit, combined, None);
            let _ = sender.send(Captured::Diagnostic(result));
        });
        let mut output = None;
        let mut failure = false;
        // terminate the original process on either stream's bounded-buffer failure
        for _ in 0..2 {
            match receiver.recv() {
                Ok(Captured::Output(Ok(bytes))) => output = Some(bytes),
                Ok(Captured::Diagnostic(Ok(()))) => {}
                _ => {
                    failure = true;
                    let _ = child.kill();
                }
            }
        }
        if failure {
            return Err(Slot);
        }
        output.ok_or(Slot)
    });
    let status = child.wait().map_err(|_| Root)?;
    // preserve ordinary subprocess failure without publishing partial stdout
    let output = captured?;
    if !status.success() {
        return Err(Slot);
    }
    Ok(output)
}

#[cfg(test)]
#[path = "family_target_image_process_test.rs"]
mod tests;
