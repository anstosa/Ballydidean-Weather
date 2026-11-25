use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::NativeFamilyTargetImage;
use crate::{FamilyTargetImageRead, MaintenanceAnchorStatusError};

const IMAGE: &str = "ghcr.io/anstosa/weather-server@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const MEMBER: &str = "config/forecast-adjustments/ballydidean/adjustments.json";

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct FixtureDirectory(PathBuf);

impl FixtureDirectory {
    /// create one isolated subprocess fixture directory
    fn new() -> Self {
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "weather-family-target-image-process-{}-{sequence}",
            std::process::id(),
        ));
        fs::create_dir(&path).expect("create family image process fixture");
        Self(path)
    }

    /// borrow the fixture directory
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for FixtureDirectory {
    /// remove only this disposable subprocess fixture
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// write one executable child-process shim
fn write_program(root: &Path, name: &str, body: &str) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).expect("write process shim");
    let mut permissions = fs::metadata(&path)
        .expect("read process shim metadata")
        .permissions();
    permissions.set_mode(0o700);
    fs::set_permissions(&path, permissions).expect("make process shim executable");
    path
}

/// preserve the exact original Docker argv and return stdout without diagnostics
#[test]
fn passes_exact_original_arguments_and_returns_only_stdout() {
    let fixture = FixtureDirectory::new();
    let program = write_program(
        fixture.path(),
        "record-arguments",
        "printf '%s\\n' \"$@\" >\"$0.args\"\nprintf 'ok'\nprintf 'diagnostic' >&2",
    );
    let mut image = NativeFamilyTargetImage::for_program(IMAGE, &program).expect("valid image");

    let output = image.read_member(MEMBER, 16).expect("bounded member");

    assert_eq!(output, b"ok");
    assert_eq!(
        fs::read_to_string(program.with_extension("args")).expect("recorded arguments"),
        format!(
            "run\n--rm\n--network\nnone\n--read-only\n--pids-limit\n32\n--memory\n128m\n--entrypoint\n/bin/cat\n{IMAGE}\n/opt/weather/{MEMBER}\n"
        ),
    );
}

/// accept exactly two bytes and reject a shorter member
#[test]
fn enforces_the_original_minimum_member_size() {
    let fixture = FixtureDirectory::new();
    let exact = write_program(fixture.path(), "minimum", "printf 'ok'");
    let short = write_program(fixture.path(), "short", "printf 'x'");
    let mut exact_image = NativeFamilyTargetImage::for_program(IMAGE, &exact).expect("valid image");
    let mut short_image = NativeFamilyTargetImage::for_program(IMAGE, &short).expect("valid image");

    assert_eq!(exact_image.read_member(MEMBER, 2), Ok(b"ok".to_vec()));
    assert_eq!(
        short_image.read_member(MEMBER, 2),
        Err(MaintenanceAnchorStatusError::Slot),
    );
}

/// enforce the original member and combined process-output ceilings
#[test]
fn enforces_member_and_combined_output_maximums() {
    let fixture = FixtureDirectory::new();
    let exact_member = write_program(fixture.path(), "maximum-member", "printf '12345678'");
    let exact_combined = write_program(
        fixture.path(),
        "maximum-combined",
        "printf '12345'\nprintf 'abcd' >&2",
    );
    let combined_overflow = write_program(
        fixture.path(),
        "combined-overflow",
        "printf '12345'\nprintf 'abcde' >&2",
    );
    let member_overflow = write_program(fixture.path(), "member-overflow", "printf '123456789'");
    let mut exact_member_image =
        NativeFamilyTargetImage::for_program(IMAGE, &exact_member).expect("valid image");
    let mut exact_combined_image =
        NativeFamilyTargetImage::for_program(IMAGE, &exact_combined).expect("valid image");
    let mut combined_overflow_image =
        NativeFamilyTargetImage::for_program(IMAGE, &combined_overflow).expect("valid image");
    let mut member_overflow_image =
        NativeFamilyTargetImage::for_program(IMAGE, &member_overflow).expect("valid image");

    assert_eq!(
        exact_member_image.read_member(MEMBER, 8),
        Ok(b"12345678".to_vec()),
    );
    assert_eq!(
        exact_combined_image.read_member(MEMBER, 8),
        Ok(b"12345".to_vec()),
    );
    assert_eq!(
        combined_overflow_image.read_member(MEMBER, 8),
        Err(MaintenanceAnchorStatusError::Slot),
    );
    assert_eq!(
        member_overflow_image.read_member(MEMBER, 8),
        Err(MaintenanceAnchorStatusError::Slot),
    );
}

/// reject an ordinary nonzero child status without returning partial output
#[test]
fn rejects_an_ordinary_child_failure() {
    let fixture = FixtureDirectory::new();
    let program = write_program(
        fixture.path(),
        "failure",
        "printf 'ok'\nprintf 'no' >&2\nexit 7",
    );
    let mut image = NativeFamilyTargetImage::for_program(IMAGE, &program).expect("valid image");

    assert_eq!(
        image.read_member(MEMBER, 4),
        Err(MaintenanceAnchorStatusError::Slot),
    );
}
