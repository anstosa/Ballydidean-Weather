//! Bounded native Docker observation for the current family release runtime.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::NativeWirePrimitives;
use crate::maintenance_canonical_json::{field, text};
use crate::native_family_release_current::{
    NativeFamilyReleaseCurrentError, NativeFamilyReleaseLiveRuntime,
};

const COMPOSE_OUTPUT_LIMIT: usize = 4 * 1024;
const INSPECT_OUTPUT_LIMIT: usize = 1024 * 1024;

/// Concrete subprocess owner for the predecessor's three live container checks.
pub struct NativeFamilyReleaseLiveRuntimeProcess {
    program: PathBuf,
    compose: PathBuf,
    releases: PathBuf,
}

impl NativeFamilyReleaseLiveRuntimeProcess {
    /// Bind the production Docker executable and fixed deployed release paths.
    pub fn native() -> Self {
        Self::for_program(
            PathBuf::from("docker"),
            PathBuf::from("/opt/weather/current/deploy/compose.yaml"),
            PathBuf::from("/opt/weather/current/deploy/releases"),
        )
    }

    /// Bind a disposable command fixture without changing production argument construction.
    #[cfg(test)]
    pub(crate) fn for_fixture_program(
        program: PathBuf,
        compose: PathBuf,
        releases: PathBuf,
    ) -> Self {
        Self::for_program(program, compose, releases)
    }

    /// Retain executable selection independently of all command operands.
    fn for_program(program: PathBuf, compose: PathBuf, releases: PathBuf) -> Self {
        Self {
            program,
            compose,
            releases,
        }
    }

    /// Resolve one exact running container identifier through Docker Compose.
    fn container_id(
        &self,
        release: &str,
        service: &str,
    ) -> Result<String, NativeFamilyReleaseCurrentError> {
        let environment = self.releases.join(format!("{release}.env"));
        let output = run_bounded(
            &self.program,
            &[
                "compose",
                "--project-name",
                "weather",
                "--env-file",
                path_text(&environment)?,
                "--file",
                path_text(&self.compose)?,
                "ps",
                "--quiet",
                service,
            ],
            COMPOSE_OUTPUT_LIMIT,
        )?;
        let container =
            String::from_utf8(output).map_err(|_| NativeFamilyReleaseCurrentError::Runtime)?;
        let container = container.trim();
        valid_container_id(container)
            .then(|| container.to_owned())
            .ok_or(NativeFamilyReleaseCurrentError::Runtime)
    }

    /// Inspect one selected container and require its exact image and healthy running state.
    fn verify_container(
        &self,
        container: &str,
        expected_image: &str,
    ) -> Result<(), NativeFamilyReleaseCurrentError> {
        let output = run_bounded(
            &self.program,
            &["inspect", "--format", "{{json .}}", container],
            INSPECT_OUTPUT_LIMIT,
        )?;
        let value = NativeWirePrimitives
            .decode_json(&output)
            .ok_or(NativeFamilyReleaseCurrentError::Runtime)?;
        let config = field(&value, "Config").ok_or(NativeFamilyReleaseCurrentError::Runtime)?;
        let state = field(&value, "State").ok_or(NativeFamilyReleaseCurrentError::Runtime)?;
        let health = field(state, "Health").ok_or(NativeFamilyReleaseCurrentError::Runtime)?;
        let running = matches!(field(state, "Running"), Some(JsonValue::Boolean(true)));
        // require the configured immutable image plus the source's running and healthy states
        if text(config, "Image").as_deref() != Some(expected_image)
            || !running
            || text(health, "Status").as_deref() != Some("healthy")
        {
            return Err(NativeFamilyReleaseCurrentError::Runtime);
        }
        Ok(())
    }
}

impl Default for NativeFamilyReleaseLiveRuntimeProcess {
    /// Select the production observation paths without starting a process.
    fn default() -> Self {
        Self::native()
    }
}

impl NativeFamilyReleaseLiveRuntime for NativeFamilyReleaseLiveRuntimeProcess {
    /// Inspect API, worker, then web independently through exact immutable image bindings.
    fn verify_running_images(
        &mut self,
        release: &str,
        environment: &JsonValue,
    ) -> Result<(), NativeFamilyReleaseCurrentError> {
        // reject a substituted release before it can become an environment filename
        if !valid_release(release)
            || text(environment, "WEATHER_RELEASE").as_deref() != Some(release)
        {
            return Err(NativeFamilyReleaseCurrentError::Runtime);
        }
        let server = text(environment, "WEATHER_SERVER_IMAGE")
            .ok_or(NativeFamilyReleaseCurrentError::Runtime)?;
        let web = text(environment, "WEATHER_WEB_IMAGE")
            .ok_or(NativeFamilyReleaseCurrentError::Runtime)?;
        // preserve the original independent api, worker, web observation order
        for (service, image) in [
            ("api", server.as_str()),
            ("worker", server.as_str()),
            ("web", web.as_str()),
        ] {
            let container = self.container_id(release, service)?;
            self.verify_container(&container, image)?;
        }
        Ok(())
    }
}

/// One independently drained subprocess stream outcome.
enum Captured {
    Output(io::Result<Vec<u8>>),
    Diagnostic(io::Result<()>),
}

/// Run one exact command while bounding combined stdout and stderr allocation.
fn run_bounded(
    program: &Path,
    arguments: &[&str],
    maximum: usize,
) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| NativeFamilyReleaseCurrentError::Runtime)?;
    capture_bounded(&mut child, maximum)
}

/// Drain both child pipes, terminate on overflow, and always reap the selected child.
fn capture_bounded(
    child: &mut Child,
    maximum: usize,
) -> Result<Vec<u8>, NativeFamilyReleaseCurrentError> {
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(NativeFamilyReleaseCurrentError::Runtime);
    };
    let (sender, receiver) = mpsc::channel();
    let output_sender = sender.clone();
    let total = AtomicUsize::new(0);
    let combined = &total;
    let captured = thread::scope(|scope| {
        // drain both pipes concurrently so neither can block the child
        scope.spawn(move || {
            let mut bytes = Vec::with_capacity(maximum.min(4096));
            let result = read_pipe(stdout, maximum, combined, Some(&mut bytes)).map(|()| bytes);
            let _ = output_sender.send(Captured::Output(result));
        });
        scope.spawn(move || {
            let result = read_pipe(stderr, maximum, combined, None);
            let _ = sender.send(Captured::Diagnostic(result));
        });
        let mut output = None;
        let mut failed = false;
        // collect exactly one result from each owned stream
        for _ in 0..2 {
            match receiver.recv() {
                Ok(Captured::Output(Ok(bytes))) => output = Some(bytes),
                Ok(Captured::Diagnostic(Ok(()))) => {}
                _ => {
                    failed = true;
                    let _ = child.kill();
                }
            }
        }
        if failed { Err(()) } else { output.ok_or(()) }
    });
    let status = child
        .wait()
        .map_err(|_| NativeFamilyReleaseCurrentError::Runtime)?;
    // publish stdout only after both bounds and ordinary exit success
    if !status.success() {
        return Err(NativeFamilyReleaseCurrentError::Runtime);
    }
    captured.map_err(|_| NativeFamilyReleaseCurrentError::Runtime)
}

/// Read one pipe within the aggregate allowance without retaining diagnostics.
fn read_pipe(
    mut stream: impl Read,
    maximum: usize,
    total: &AtomicUsize,
    mut output: Option<&mut Vec<u8>>,
) -> io::Result<()> {
    let mut buffer = [0_u8; 8192];
    // stop only after EOF or the first aggregate overflow
    loop {
        let remaining = maximum.saturating_sub(total.load(Ordering::Relaxed));
        let requested = (remaining + 1).min(buffer.len());
        let count = match stream.read(&mut buffer[..requested]) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        // an empty read ends only this owned stream
        if count == 0 {
            return Ok(());
        }
        let prior = total.fetch_add(count, Ordering::Relaxed);
        // preserve one combined deterministic bound across both pipes
        if prior + count > maximum {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "family runtime command output exceeds bound",
            ));
        }
        // retain stdout only while continuing to drain stderr
        if let Some(bytes) = output.as_deref_mut() {
            bytes.extend_from_slice(&buffer[..count]);
        }
    }
}

/// Borrow one UTF-8 path exactly as a process argument.
fn path_text(path: &Path) -> Result<&str, NativeFamilyReleaseCurrentError> {
    path.to_str()
        .ok_or(NativeFamilyReleaseCurrentError::Runtime)
}

/// Recognize one source release before deriving its environment filename.
fn valid_release(value: &str) -> bool {
    let bytes = value.as_bytes();
    (12..=14).contains(&bytes.len())
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'.'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'.'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && bytes[10] == b'-'
        && matches!(bytes[11], b'1'..=b'9')
        && bytes[12..].iter().all(u8::is_ascii_digit)
}

/// Recognize one Docker container identifier accepted by the source observer.
fn valid_container_id(value: &str) -> bool {
    (12..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "native_family_release_live_runtime_test.rs"]
mod tests;
