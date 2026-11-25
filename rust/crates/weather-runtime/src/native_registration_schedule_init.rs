//! Native Blueberry registration schedule owner request and result boundary.

use std::io::{self, Read};

use weather_edge::ArchiveWirePrimitives;

use crate::capture_epoch_witness::verify_operator_capture_epoch_witness;
use crate::native_maintenance_registration_lifecycle_remote::bootstrap_value;
use crate::{
    CaptureEpochOperatorFile, CaptureEpochOperatorFiles, MaintenanceCanonicalJson,
    NativeAdjustmentRollingScheduleBootstrapInput, NativeWirePrimitives,
    build_adjustment_rolling_schedule_bootstrap, decode_registration_schedule_initialize_ack,
};

const ROLLING_LEDGER: &str = "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";

/// Admit the exact retained rolling epoch before generating the fixed owner SQL.
pub fn build_native_registration_schedule_initialize_sql(
    input: impl Read,
    requested_sha256: &str,
    files: &mut impl CaptureEpochOperatorFiles,
) -> io::Result<Vec<u8>> {
    let witness_bytes = files
        .read(CaptureEpochOperatorFile::Witness)
        .map_err(|_| invalid("retained rolling epoch unavailable"))?;
    let witness = verify_operator_capture_epoch_witness(
        &witness_bytes,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .map_err(|_| invalid("retained rolling epoch differs"))?
    .witness;
    build_initialize_sql(
        input,
        requested_sha256,
        &witness.epochAt,
        &witness.witnessSha256,
        &witness.databaseMigrationHistorySha256,
    )
}

/// Recompute the canonical predecessor-authorized finite horizon from the retained epoch.
fn build_initialize_sql(
    input: impl Read,
    requested_sha256: &str,
    epoch_at: &str,
    epoch_witness_sha256: &str,
    migration_history_sha256: &str,
) -> io::Result<Vec<u8>> {
    // retain the original bootstrap witness's exact 0021 history
    if migration_history_sha256 != ROLLING_LEDGER {
        return Err(invalid("retained rolling epoch ledger differs"));
    }
    let bootstrap = build_adjustment_rolling_schedule_bootstrap(
        NativeAdjustmentRollingScheduleBootstrapInput {
            epoch_at,
            epoch_witness_sha256,
        },
    )
    .map_err(|_| invalid("registration schedule bootstrap differs"))?;
    let bytes = bounded(input, 2_048)?;
    let expected = NativeWirePrimitives
        .encode_canonical_json(&bootstrap_value(&bootstrap))
        .ok_or_else(|| invalid("registration schedule bootstrap encoding differs"))?;
    // require the exact caller bytes, canonical digest and independently derived horizon
    if requested_sha256 != bootstrap.bootstrap_sha256 || bytes != expected {
        return Err(invalid("registration schedule input is not canonical"));
    }
    let document = std::str::from_utf8(
        expected
            .strip_suffix(b"\n")
            .ok_or_else(|| invalid("registration schedule framing differs"))?,
    )
    .map_err(|_| invalid("registration schedule encoding differs"))?;
    Ok(format!(
        "BEGIN;\nSET LOCAL statement_timeout = '30s';\nSET LOCAL lock_timeout = '5s';\nSELECT weather_initialize_adjustment_registration_schedule_v3($bootstrap${document}$bootstrap$::jsonb);\nCOMMIT;\n"
    ).into_bytes())
}

/// Verify the actual JSON owner response before exposing the canonical acknowledgement.
pub fn project_native_registration_schedule_initialize_result(
    input: impl Read,
    requested_sha256: &str,
) -> io::Result<Vec<u8>> {
    let raw = bounded(input, 4_096)?;
    let value = NativeWirePrimitives
        .decode_json(&raw)
        .ok_or_else(|| invalid("registration schedule result differs"))?;
    let encoded = NativeWirePrimitives
        .encode_canonical_json(&value)
        .ok_or_else(|| invalid("registration schedule result encoding differs"))?;
    decode_registration_schedule_initialize_ack(
        &mut NativeWirePrimitives,
        &encoded,
        requested_sha256,
    )
    .map_err(|_| invalid("registration schedule result differs"))?;
    Ok(encoded)
}

/// Read at most one byte beyond the original owner limit.
fn bounded(input: impl Read, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    input.take(limit + 1).read_to_end(&mut bytes)?;
    // reject an incomplete or oversized input before decoding
    if bytes.is_empty() || bytes.len() as u64 > limit {
        return Err(invalid(
            "registration schedule owner input exceeds its bound",
        ));
    }
    Ok(bytes)
}

/// Keep diagnostics categorical.
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Match the retained Node finite bootstrap SQL and canonical retry acknowledgement.
    #[test]
    fn native_registration_schedule_initializer_matches_source() {
        let witness = "a".repeat(64);
        let bootstrap = build_adjustment_rolling_schedule_bootstrap(
            NativeAdjustmentRollingScheduleBootstrapInput {
                epoch_at: "2026-10-09T04:10:45.000Z",
                epoch_witness_sha256: &witness,
            },
        )
        .unwrap();
        let bytes = NativeWirePrimitives
            .encode_canonical_json(&bootstrap_value(&bootstrap))
            .unwrap();
        let expected_sql = format!(
            "BEGIN;\nSET LOCAL statement_timeout = '30s';\nSET LOCAL lock_timeout = '5s';\nSELECT weather_initialize_adjustment_registration_schedule_v3($bootstrap${}$bootstrap$::jsonb);\nCOMMIT;\n",
            std::str::from_utf8(&bytes).unwrap().trim_end(),
        );
        assert_eq!(
            bootstrap.bootstrap_sha256,
            "83f1eabf954aa278012d6006a68266a8865f27ce95dfaba24aa8c9d8119351f4"
        );
        assert_eq!(
            build_initialize_sql(
                bytes.as_slice(),
                &bootstrap.bootstrap_sha256,
                &bootstrap.epoch_at,
                &witness,
                ROLLING_LEDGER
            )
            .unwrap(),
            expected_sql.as_bytes()
        );
        let raw = format!(
            "{{\"initialized\": false, \"scheduleContractSha256\": \"{}\", \"bootstrapSha256\": \"{}\"}}\n",
            bootstrap.schedule_contract_sha256, bootstrap.bootstrap_sha256
        );
        let canonical = format!(
            "{{\"bootstrapSha256\":\"{}\",\"initialized\":false,\"scheduleContractSha256\":\"{}\"}}\n",
            bootstrap.bootstrap_sha256, bootstrap.schedule_contract_sha256
        );
        assert_eq!(
            project_native_registration_schedule_initialize_result(
                raw.as_bytes(),
                &bootstrap.bootstrap_sha256
            )
            .unwrap(),
            canonical.as_bytes()
        );
    }
}
