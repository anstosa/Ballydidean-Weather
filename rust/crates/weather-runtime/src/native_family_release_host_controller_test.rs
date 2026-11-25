use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::run_native_family_release_switch_fixture;

static FIXTURE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Invoke only the fixed native-generation verb with all eight proof-bound operands in order.
#[test]
fn native_family_release_host_uses_exact_native_switch_contract() {
    let root = fixture_root();
    fs::create_dir(&root).unwrap();
    let program = root.join("weather-remote-ops");
    let output = root.join("arguments");
    fs::write(
        &program,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" >'{}'\nprintf 'native switch admitted\\n'\n",
            output.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&program, fs::Permissions::from_mode(0o700)).unwrap();
    let arguments = [
        "2026.10.10-1",
        "2026.10.09-1",
        "2026.10.09-1",
        "2222222222222222222222222222222222222222",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc",
        "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
    ];

    run_native_family_release_switch_fixture(&program, arguments).unwrap();

    assert_eq!(
        fs::read_to_string(output).unwrap(),
        ["--family-apply-native-unlocked"]
            .into_iter()
            .chain(arguments)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    );
    fs::remove_dir_all(root).unwrap();
}

/// Allocate one unique disposable command fixture root.
fn fixture_root() -> PathBuf {
    let sequence = FIXTURE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "native-family-release-host-{}-{sequence}",
        std::process::id()
    ))
}
