use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use weather_contracts::JsonValue;

use super::NativeFamilyReleaseLiveRuntimeProcess;
use crate::native_family_release_current::NativeFamilyReleaseLiveRuntime;

const RELEASE: &str = "2026.10.09-1";
const SERVER: &str = "ghcr.io/anstosa/weather-server@sha256:1111111111111111111111111111111111111111111111111111111111111111";
const WEB: &str = "ghcr.io/anstosa/weather-web@sha256:2222222222222222222222222222222222222222222222222222222222222222";
static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Disposable mock-process fixture removed after one ordinary observation.
struct FixtureRoot(PathBuf);

impl FixtureRoot {
    /// Create one process-unique canonical temporary directory.
    fn create() -> Self {
        let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-family-live-runtime-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
}

impl Drop for FixtureRoot {
    /// Remove only this process-unique fixture tree.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Verify exact compose and inspect calls for all three healthy application services.
#[test]
fn observes_api_worker_and_web_through_exact_bounded_commands() {
    let root = FixtureRoot::create();
    let program = root.0.join("docker-fixture.sh");
    let log = root.0.join("arguments.log");
    let script = format!(
        "#!/bin/sh\n\
         printf '%s\\n' \"$*\" >> '{}'\n\
         if [ \"$1\" = compose ]; then\n\
           case \"${{10}}\" in\n\
             api) printf '%s\\n' {} ;;\n\
             worker) printf '%s\\n' {} ;;\n\
             web) printf '%s\\n' {} ;;\n\
             *) exit 8 ;;\n\
           esac\n\
         elif [ \"$1\" = inspect ]; then\n\
           case \"$4\" in\n\
             {}) printf '%s\\n' '{}' ;;\n\
             {}) printf '%s\\n' '{}' ;;\n\
             {}) printf '%s\\n' '{}' ;;\n\
             *) exit 9 ;;\n\
           esac\n\
         else\n\
           exit 10\n\
         fi\n",
        log.display(),
        "a".repeat(64),
        "b".repeat(64),
        "c".repeat(64),
        "a".repeat(64),
        inspect_json(SERVER),
        "b".repeat(64),
        inspect_json(SERVER),
        "c".repeat(64),
        inspect_json(WEB),
    );
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&program)
        .unwrap();
    file.write_all(script.as_bytes()).unwrap();
    drop(file);
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();

    let compose = root.0.join("compose.yaml");
    let releases = root.0.join("releases");
    fs::write(&compose, b"fixture\n").unwrap();
    fs::create_dir(&releases).unwrap();
    let mut runtime = NativeFamilyReleaseLiveRuntimeProcess::for_fixture_program(
        program,
        compose.clone(),
        releases.clone(),
    );
    runtime
        .verify_running_images(RELEASE, &environment())
        .unwrap();

    let observed = fs::read_to_string(log).unwrap();
    let lines = observed.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 6);
    assert_eq!(
        lines[0],
        format!(
            "compose --project-name weather --env-file {}/{RELEASE}.env --file {} ps --quiet api",
            releases.display(),
            compose.display()
        )
    );
    assert_eq!(
        lines[1],
        format!("inspect --format {{{{json .}}}} {}", "a".repeat(64))
    );
    assert!(lines[2].ends_with("ps --quiet worker"));
    assert_eq!(
        lines[3],
        format!("inspect --format {{{{json .}}}} {}", "b".repeat(64))
    );
    assert!(lines[4].ends_with("ps --quiet web"));
    assert_eq!(
        lines[5],
        format!("inspect --format {{{{json .}}}} {}", "c".repeat(64))
    );
}

/// Construct one exact environment subset consumed by the runtime observer.
fn environment() -> JsonValue {
    JsonValue::Object(vec![
        (
            "WEATHER_RELEASE".encode_utf16().collect(),
            json_text(RELEASE),
        ),
        (
            "WEATHER_SERVER_IMAGE".encode_utf16().collect(),
            json_text(SERVER),
        ),
        ("WEATHER_WEB_IMAGE".encode_utf16().collect(), json_text(WEB)),
    ])
}

/// Construct one healthy Docker inspect document.
fn inspect_json(image: &str) -> String {
    format!(
        "{{\"Config\":{{\"Image\":\"{image}\"}},\"State\":{{\"Health\":{{\"Status\":\"healthy\"}},\"Running\":true}}}}"
    )
}

/// Construct one UTF-16 JSON string value.
fn json_text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}
