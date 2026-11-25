use super::{
    NativeMaintenanceBackingRootKind, WindowsBackingCapacity, parse_backing_c_output,
    valid_identity_output,
};

/// Accept only the two newline spellings emitted by the fixed Ubuntu identity probe.
#[test]
fn native_identity_parser_requires_exact_device_and_inode() {
    assert!(valid_identity_output(b"650|42\n", 650, 42));
    assert!(valid_identity_output(b"650|42\r\n", 650, 42));
    assert!(!valid_identity_output(b"650|42", 650, 42));
    assert!(!valid_identity_output(b"650|43\n", 650, 42));
    assert!(!valid_identity_output(b"650|42\nignored", 650, 42));
}

/// Decode the exact backing-C frame while retaining full unsigned capacity range.
#[test]
fn native_backing_capacity_parser_accepts_exact_source_grammar() {
    assert_eq!(
        parse_backing_c_output(b"weather-backing-c/v1|18446744073709551615|00042|ubuntu-vhd-on-c"),
        Some(WindowsBackingCapacity {
            free_bytes: 42,
            total_bytes: u64::MAX,
        }),
    );
    assert_eq!(
        NativeMaintenanceBackingRootKind::BackingC.as_str(),
        "backing_c"
    );
}

/// Reject ext4-shaped, localized, partial, trailing, and impossible responses.
#[test]
fn native_backing_capacity_parser_rejects_substitute_frames() {
    // preserve the predecessor regression against ext4-only JSON impersonation
    for invalid in [
        br#"{"freeBytes":"68719476736","rootKind":"home_native_ext4_primary"}"#.as_slice(),
        b"weather-backing-c/v1|0|0|ubuntu-vhd-on-c",
        b"weather-backing-c/v1|9|10|ubuntu-vhd-on-c",
        b"weather-backing-c/v1|10|9|ubuntu-vhd-on-c\n",
        b"weather-backing-c/v1|10|9|ubuntu-vhd-on-c|extra",
        b"weather-backing-c/v1|1,000|9|ubuntu-vhd-on-c",
        b"weather-backing-c/v1|10||ubuntu-vhd-on-c",
    ] {
        assert_eq!(parse_backing_c_output(invalid), None);
    }
}
