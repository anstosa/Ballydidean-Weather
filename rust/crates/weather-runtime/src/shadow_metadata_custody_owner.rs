//! The existing evaluation-package owner commands for fixed shadow metadata custody.

use std::io::Read;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::shadow_metadata_custody::{MAXIMUM_DOCUMENT_BYTES, read_custody_status};
use crate::shadow_metadata_custody_mutation::mutate_custody_value;
use crate::shadow_metadata_custody_owner_sql::{
    build_shadow_metadata_finalization_sql, build_shadow_metadata_preparation_sql,
};
use crate::{MaintenanceAnchorStatusJson, ShadowMetadataCustodyError, ShadowMetadataCustodyWrites};

use ShadowMetadataCustodyError::{Arguments, Checkpoint, ProofMismatch};

/// The genuine owner result parser admits ordinary JSON before canonical store validation.
pub trait ShadowMetadataCustodyOwnerJson: MaintenanceAnchorStatusJson {
    /// Decode the original owner query output with JSON.parse semantics.
    fn parse_owner_json(&mut self, bytes: &[u8]) -> Option<JsonValue>;
}

/// Execute the fixed evaluation-package custody bridge without opening a database connection.
pub fn shadow_metadata_custody_owner_command(
    action: &str,
    args: &[&str],
    input: &mut impl Read,
    files: &mut impl ShadowMetadataCustodyWrites,
    codec: &mut impl ShadowMetadataCustodyOwnerJson,
    time: &dyn CurrentTimeResolver,
    now: &str,
) -> Result<Option<Vec<u8>>, ShadowMetadataCustodyError> {
    // admit only the five original owner actions before touching input or storage
    if !matches!(
        action,
        "prepare-shadow-metadata-custody-v1"
            | "consume-shadow-metadata-custody-v1"
            | "shadow-metadata-preparation-identity-v1"
            | "shadow-metadata-preparation-sql-v1"
            | "shadow-metadata-finalization-sql-v1"
    ) {
        return Ok(None);
    }
    let [argument] = args else {
        return Err(Arguments);
    };
    // the operator supplies only one exact proof or preparation identity
    if !valid_hash(argument) {
        return Err(Arguments);
    }
    if matches!(
        action,
        "prepare-shadow-metadata-custody-v1" | "consume-shadow-metadata-custody-v1"
    ) {
        let is_preparation = action == "prepare-shadow-metadata-custody-v1";
        let key = if is_preparation {
            "proofSha256"
        } else {
            "preparationSha256"
        };
        let mut bytes = Vec::new();
        input
            .take(MAXIMUM_DOCUMENT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Arguments)?;
        // admit genuine database JSON whitespace and field order within the original bound
        if bytes.len() > MAXIMUM_DOCUMENT_BYTES {
            return Err(Arguments);
        }
        let value = codec.parse_owner_json(&bytes).ok_or(Arguments)?;
        // only the outer operand comparison precedes the store's interrupted recovery
        if text(&value, key).as_deref() != Some(*argument) {
            return Err(Arguments);
        }
        return mutate_custody_value(&value, is_preparation, files, codec, time, now).map(Some);
    }

    // inspect the original fully validated status and preserve its recovery semantics
    let (status, _) = read_custody_status(files, codec, time)?;
    let proof = field(&status, "proof")
        .filter(|value| !matches!(value, JsonValue::Null))
        .ok_or(ProofMismatch)?;
    let preparation =
        field(&status, "preparation").filter(|value| !matches!(value, JsonValue::Null));
    // finalization selects an exact durable preparation instead of a raw caller-supplied operand
    if action == "shadow-metadata-finalization-sql-v1" {
        let preparation = preparation.ok_or(Checkpoint)?;
        if text(preparation, "preparationSha256").as_deref() != Some(*argument) {
            return Err(Checkpoint);
        }
        return build_shadow_metadata_finalization_sql(preparation, codec, time).map(Some);
    }
    // both remaining queries refer to the current exact proof
    if text(proof, "proofSha256").as_deref() != Some(*argument) {
        return Err(ProofMismatch);
    }
    if action == "shadow-metadata-preparation-identity-v1" {
        let identity = preparation
            .and_then(|value| text(value, "preparationSha256"))
            .unwrap_or_else(|| "unprepared".to_owned());
        return Ok(Some(format!("{identity}\n").into_bytes()));
    }
    // never emit fresh preparation SQL after one durable preparation already exists
    if preparation.is_some() {
        return Err(Checkpoint);
    }
    build_shadow_metadata_preparation_sql(proof, codec, time).map(Some)
}
