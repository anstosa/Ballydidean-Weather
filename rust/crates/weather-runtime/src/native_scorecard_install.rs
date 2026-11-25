//! Native scorecard installation input projection with original immutable wire bytes.

use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    FORECAST_SCORECARD_MAX_BYTES, FORECAST_SCORECARD_V1, FORECAST_SCORECARD_V2,
    NativeWirePrimitives,
    maintenance_canonical_json::{text, valid_hash},
    native_maintenance_retirement_files_v2::format_digest,
    parse_forecast_adjustment_scorecard,
};

/// Admit one bounded installation request and return its original bytes only after exact validation.
pub fn project_native_scorecard_install_document(
    input: impl Read,
    mut output: impl Write,
    version: &str,
    expected: &str,
    now_ms: i64,
) -> Result<(), String> {
    // bind the original two version selections and canonical digest grammar
    let contract = match version {
        "v1" => FORECAST_SCORECARD_V1,
        "v2" => FORECAST_SCORECARD_V2,
        _ => return Err("scorecard installation version differs".to_owned()),
    };
    if !valid_hash(expected) {
        return Err("scorecard installation digest differs".to_owned());
    }
    let mut bytes = Vec::new();
    input
        .take(FORECAST_SCORECARD_MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let document =
        parse_forecast_adjustment_scorecard(&bytes, Some(now_ms), &mut NativeWirePrimitives)
            .map_err(|error| error.to_string())?;
    // preserve the original raw bytes rather than reserializing the public aggregate
    if text(&document, "contractVersion").as_deref() != Some(contract)
        || format_digest(NativeWirePrimitives.sha256(&bytes)) != expected
    {
        return Err("scorecard installation identity differs".to_owned());
    }
    output.write_all(&bytes).map_err(|error| error.to_string())
}

/// Read one of the fixed scorecard hash-only pointers from the installed evidence directory.
pub fn read_native_scorecard_install_pointer(root: &Path, name: &str) -> Result<String, String> {
    // restrict the native command to the exact pointer set read by the predecessor installer
    if !matches!(
        name,
        "current.json" | "v2-current.json" | "v2-previous.json" | "v2-pending.json"
    ) {
        return Err("scorecard pointer differs".to_owned());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(root.join(name))
        .map_err(|error| error.to_string())?;
    let properties = file.metadata().map_err(|error| error.to_string())?;
    // retain the source file-type, mode and bounded read checks
    if !properties.is_file() || properties.mode() & 0o777 != 0o600 || properties.len() > 128 {
        return Err("scorecard pointer differs".to_owned());
    }
    let mut bytes = Vec::new();
    file.take(129)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let Some(document @ JsonValue::Object(_)) = NativeWirePrimitives.decode_json(&bytes) else {
        return Err("scorecard pointer differs".to_owned());
    };
    let JsonValue::Object(fields) = &document else {
        unreachable!("object admitted")
    };
    // reject any additional pointer fields or noncanonical hash spelling
    if bytes.len() > 128 || fields.len() != 1 {
        return Err("scorecard pointer differs".to_owned());
    }
    text(&document, "sha256")
        .filter(|value| valid_hash(value))
        .ok_or_else(|| "scorecard pointer differs".to_owned())
}
