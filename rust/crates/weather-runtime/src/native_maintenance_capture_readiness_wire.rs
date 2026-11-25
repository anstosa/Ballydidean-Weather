//! Exact predecessor JSON.stringify response for the native capture readiness CLI.

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::{
    NativeMaintenanceCaptureArchiveStatus, NativeMaintenanceCaptureReadiness, NativeWirePrimitives,
};

/// Encode the complete path-free readiness document in original producer field order.
pub fn encode_native_maintenance_capture_readiness(
    readiness: &NativeMaintenanceCaptureReadiness,
) -> Option<Vec<u8>> {
    let archive = match &readiness.archive_status {
        Some(status) => archive_status(status)?,
        None => JsonValue::Null,
    };
    let value = JsonValue::Object(vec![
        member("archiveStatus", archive),
        member("contractVersion", string(readiness.contract_version)),
        member(
            "databaseMigrationHistorySha256",
            optional(&readiness.database_migration_history_sha256),
        ),
        member("ready", JsonValue::Boolean(readiness.ready)),
        member("reason", string(readiness.reason)),
        member(
            "releaseManifestSha256",
            optional(&readiness.release_manifest_sha256),
        ),
        member("sourceCommit", optional(&readiness.source_commit)),
        member("sourceRelease", optional(&readiness.source_release)),
    ]);
    NativeWirePrimitives.encode_ordered_json(&value)
}

/// Preserve exact per-archive field order and number versus decimal-string representation.
fn archive_status(status: &NativeMaintenanceCaptureArchiveStatus) -> Option<JsonValue> {
    Some(JsonValue::Object(vec![
        member("allocatedBytes", string(&status.allocated_bytes)),
        member("fileCount", js_integer(status.file_count)?),
        member("incomingCount", js_integer(status.incoming_count)?),
        member("objectCount", js_integer(status.object_count)?),
        member("taskInodes", string(&status.task_inodes)),
    ]))
}

/// Admit count values exactly representable by the original JSON safe integer.
fn js_integer(value: u64) -> Option<JsonValue> {
    (value <= 9_007_199_254_740_991).then_some(JsonValue::Number(value as f64))
}

/// Preserve explicit null for unavailable source authorities.
fn optional(value: &Option<String>) -> JsonValue {
    value.as_deref().map_or(JsonValue::Null, string)
}

/// Construct one producer-order JSON member using native UTF-16 identifiers.
fn member(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

/// Preserve one exact UTF-16 source string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the source CLI's literal installed-release refusal bytes without host effects.
    #[test]
    fn unavailable_readiness_preserves_original_cli_bytes() {
        let value = NativeMaintenanceCaptureReadiness {
            archive_status: None,
            contract_version: "adjustment-maintenance-capture-readiness/v1",
            database_migration_history_sha256: None,
            ready: false,
            reason: "installed_release_unavailable",
            release_manifest_sha256: None,
            source_commit: None,
            source_release: None,
        };
        let bytes = encode_native_maintenance_capture_readiness(&value).unwrap();
        assert_eq!(bytes,b"{\"archiveStatus\":null,\"contractVersion\":\"adjustment-maintenance-capture-readiness/v1\",\"databaseMigrationHistorySha256\":null,\"ready\":false,\"reason\":\"installed_release_unavailable\",\"releaseManifestSha256\":null,\"sourceCommit\":null,\"sourceRelease\":null}\n");
    }
}
