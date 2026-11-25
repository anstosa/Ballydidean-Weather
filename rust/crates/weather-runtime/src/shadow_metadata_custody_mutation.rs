//! Prepare and consume exact compact custody arguments around the separate database owner.

use std::collections::BTreeMap;
use std::io::Read;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::maintenance_anchor_status::hash_field;
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::shadow_metadata_custody::{
    CONSUMPTION_VERSION, MAXIMUM_DOCUMENT_BYTES, finish_consumption, read_document,
    validate_consumption,
};
use crate::shadow_metadata_custody_preparation::{
    CONTRACT_VERSION, positive_safe_integer, validate_finalization,
    validate_shadow_custody_preparation,
};
use crate::{
    MaintenanceAnchorStatusJson, ShadowMetadataCompletedMarker, ShadowMetadataCustodyError,
    ShadowMetadataCustodyFiles, ShadowMetadataCustodySlot,
};

use ShadowMetadataCustodyError::{Arguments, Checkpoint, Codec, ProofMismatch, Write};
use ShadowMetadataCustodySlot::{
    Current, Preparation, Previous, PreviousConsumption, PreviousPreparation,
};

/// Fixed native persistence operations for the existing service-owner commands.
pub trait ShadowMetadataCustodyWrites: ShadowMetadataCustodyFiles {
    /// Require the owner-private evidence directory and fixed proof child.
    fn ensure_root(&mut self) -> Result<(), ShadowMetadataCustodyError>;

    /// Publish the first complete preparation exclusively or observe a racing existing slot.
    fn create_preparation(&mut self, bytes: &[u8]) -> Result<bool, ShadowMetadataCustodyError>;

    /// Durably replace one of the three retained terminal-history slots.
    fn replace_previous(
        &mut self,
        slot: ShadowMetadataCustodySlot,
        bytes: &[u8],
    ) -> Result<(), ShadowMetadataCustodyError>;
}

/// Dispatch the exact predecessor owner commands with bounded canonical stdin and the selected clock.
pub fn mutate_shadow_metadata_custody_command(
    action: &str,
    args: &[&str],
    input: &mut impl Read,
    files: &mut impl ShadowMetadataCustodyWrites,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
    now: &str,
) -> Result<Option<Vec<u8>>, ShadowMetadataCustodyError> {
    // reject unrecognized operands before reading any input
    let (keys, identity): (&[&str], &str) = match action {
        "shadow-metadata-custody-prepare-v1" => (&["finalizations", "proofSha256"], "proofSha256"),
        "shadow-metadata-custody-consume-v1" => (
            &["preparationSha256", "proofSha256", "results"],
            "preparationSha256",
        ),
        _ => return Ok(None),
    };
    let [argument] = args else {
        return Err(Arguments);
    };
    // bound both admission and canonical output to the original command envelope size
    if !valid_hash(argument) {
        return Err(Arguments);
    }
    let mut bytes = Vec::new();
    input
        .take(MAXIMUM_DOCUMENT_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Arguments)?;
    if !(2..=MAXIMUM_DOCUMENT_BYTES).contains(&bytes.len()) {
        return Err(Arguments);
    }
    let value = codec.parse_canonical_object(&bytes).ok_or(Arguments)?;
    // outer command validation precedes the store's recovery operation
    if !exact_keys(&value, keys)
        || text(&value, identity).as_deref() != Some(*argument)
        || codec.encode_canonical_json(&value).as_deref() != Some(bytes.as_slice())
    {
        return Err(Arguments);
    }
    mutate_custody_value(&value, identity == "proofSha256", files, codec, time, now).map(Some)
}

// share the original store recovery and validation order with the raw owner JSON bridge
pub(crate) fn mutate_custody_value(
    input: &JsonValue,
    is_preparation: bool,
    files: &mut impl ShadowMetadataCustodyWrites,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
    now: &str,
) -> Result<Vec<u8>, ShadowMetadataCustodyError> {
    finish_consumption(files, codec, time)?;
    // the original store validates its closed input after interrupted-consumption recovery
    let keys: &[&str] = if is_preparation {
        &["finalizations", "proofSha256"]
    } else {
        &["preparationSha256", "proofSha256", "results"]
    };
    if !exact_keys(input, keys) {
        return Err(Arguments);
    }
    let value = if is_preparation {
        prepare(input, files, codec, time, now)?
    } else {
        consume(input, files, codec, time, now)?
    };
    encode(&value, codec)
}

// encode one already validated bounded canonical document
fn encode(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<Vec<u8>, ShadowMetadataCustodyError> {
    let bytes = codec.encode_canonical_json(value).ok_or(Codec)?;
    // keep all output and durable documents within the shared bound
    if !(2..=MAXIMUM_DOCUMENT_BYTES).contains(&bytes.len()) {
        return Err(Codec);
    }
    Ok(bytes)
}

// project one exact string field without introducing codec-specific constructors
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

// build a field using the generated UTF-16 key storage
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

// return only the predecessor's bounded nonempty owner argument arrays
fn bounded_array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    let JsonValue::Array(values) = field(value, key)? else {
        return None;
    };
    (1..=2).contains(&values.len()).then_some(values)
}

// bind each sorted finalization to the complete corresponding proof registration and commit range
fn check_ranges(proof: &JsonValue, ordered: &[(String, JsonValue)]) -> Option<()> {
    let entries = bounded_array(proof, "entries")?;
    let mut registrations: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // preserve the exact proof commit clocks for each represented registration
    for entry in entries {
        registrations
            .entry(text(entry, "registrationSha256")?)
            .or_default()
            .push(text(entry, "predictionCommittedAt")?);
    }
    // require one and only one prepared finalization per sorted registration
    if registrations.len() != ordered.len() {
        return None;
    }
    for ((registration, mut clocks), (argument_registration, argument)) in
        registrations.into_iter().zip(ordered)
    {
        clocks.sort_unstable();
        // compare original string clocks and proof-bound identities exactly
        if &registration != argument_registration
            || field(argument, "coldCommitSha256") != field(proof, "custodyCheckpointSha256")
            || field(argument, "maintenanceAnchorSha256") != field(proof, "proofSha256")
            || positive_safe_integer(argument, "rowCount")? != clocks.len() as u64
            || text(argument, "fromCommittedAt").as_ref() != clocks.first()
            || text(argument, "throughCommittedAt").as_ref() != clocks.last()
        {
            return None;
        }
    }
    Some(())
}

// converge only the same proof and canonical argument list, retaining the initial timestamp
fn equivalent_preparation(
    existing: JsonValue,
    proof: &str,
    finalizations: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<JsonValue, ShadowMetadataCustodyError> {
    let existing_finalizations = field(&existing, "finalizations").ok_or(Checkpoint)?;
    // compare canonical arrays instead of depending on decoded object member order
    if text(&existing, "proofSha256").as_deref() != Some(proof)
        || encode(existing_finalizations, codec)? != encode(finalizations, codec)?
    {
        return Err(Checkpoint);
    }
    Ok(existing)
}

// durably prepare one exact owner argument list before database finalization
fn prepare(
    input: &JsonValue,
    files: &mut impl ShadowMetadataCustodyWrites,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
    now: &str,
) -> Result<JsonValue, ShadowMetadataCustodyError> {
    let proof = text(input, "proofSha256").ok_or(Arguments)?;
    let finalizations = bounded_array(input, "finalizations").ok_or(Arguments)?;
    let current = read_document(Current, files, codec, time)?.ok_or(ProofMismatch)?;
    // do not prepare against a substituted or absent proof
    if text(&current, "proofSha256").as_deref() != Some(proof.as_str()) {
        return Err(ProofMismatch);
    }
    let mut ordered = finalizations
        .iter()
        .map(|value| {
            // retain the independently validated native argument unchanged
            validate_finalization(value, codec, time).map(|key| (key, value.clone()))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(Arguments)?;
    ordered.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    check_ranges(&current, &ordered).ok_or(ProofMismatch)?;
    let finalizations = JsonValue::Array(ordered.into_iter().map(|(_, value)| value).collect());
    files.ensure_root()?;
    // return the first durable preparation on a matching retry
    if let Some(existing) = read_document(Preparation, files, codec, time)? {
        return equivalent_preparation(existing, &proof, &finalizations, codec);
    }
    let mut fields = vec![
        member("authority", string("shadow_metadata_transfer")),
        member("contractVersion", string(CONTRACT_VERSION)),
        member("finalizations", finalizations.clone()),
        member("preparedAt", string(now)),
        member("proofSha256", string(&proof)),
    ];
    let unsigned = encode(&JsonValue::Object(fields.clone()), codec)?;
    let checksum = codec.sha256(&unsigned).ok_or(Codec)?;
    fields.push(member("preparationSha256", string(checksum.as_str())));
    let preparation = JsonValue::Object(fields);
    validate_shadow_custody_preparation(&preparation, codec, time).ok_or(Arguments)?;
    let bytes = encode(&preparation, codec)?;
    // only the exclusively published first complete file fixes the preparation time
    if files.create_preparation(&bytes)? {
        return Ok(preparation);
    }
    let raced = read_document(Preparation, files, codec, time)?.ok_or(Write)?;
    equivalent_preparation(raced, &proof, &finalizations, codec)
}

// admit only genuine native success or exact replay identities
fn valid_result(value: &JsonValue) -> bool {
    exact_keys(
        value,
        &[
            "generation",
            "metadataManifestSha256",
            "newMetadataRootSha256",
            "registrationSha256",
            "status",
        ],
    ) && positive_safe_integer(value, "generation").is_some()
        && [
            "metadataManifestSha256",
            "newMetadataRootSha256",
            "registrationSha256",
        ]
        .iter()
        .all(|key| hash_field(value, key))
        && matches!(
            text(value, "status").as_deref(),
            Some("already_finalized" | "finalized")
        )
}

// match every result to its prepared argument in the original order
fn matching_results(results: &[JsonValue], finalizations: &[JsonValue]) -> bool {
    results.len() == finalizations.len()
        && results.iter().zip(finalizations).all(|(result, expected)| {
            // compare exactly the four identities returned by the native owner
            [
                "generation",
                "metadataManifestSha256",
                "newMetadataRootSha256",
                "registrationSha256",
            ]
            .iter()
            .all(|key| field(result, key) == field(expected, key))
        })
}

// commit terminal evidence before removing either of the active markers
fn consume(
    input: &JsonValue,
    files: &mut impl ShadowMetadataCustodyWrites,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
    now: &str,
) -> Result<JsonValue, ShadowMetadataCustodyError> {
    let proof = text(input, "proofSha256")
        .filter(|value| valid_hash(value))
        .ok_or(Arguments)?;
    let preparation_hash = text(input, "preparationSha256").ok_or(Arguments)?;
    let results = bounded_array(input, "results").ok_or(Arguments)?;
    // validate even on the terminal idempotency path
    if !results.iter().all(valid_result) {
        return Err(Arguments);
    }
    let prior = read_document(PreviousConsumption, files, codec, time)?;
    let current = read_document(Current, files, codec, time)?;
    let preparation = read_document(Preparation, files, codec, time)?;
    // preserve the durable first receipt on an already finished exact retry
    if current.is_none()
        && preparation.is_none()
        && let Some(prior) = prior
        && text(&prior, "proofSha256").as_deref() == Some(proof.as_str())
        && text(&prior, "preparationSha256").as_deref() == Some(preparation_hash.as_str())
    {
        return Ok(prior);
    }
    let current = current.ok_or(ProofMismatch)?;
    let preparation = preparation.ok_or(Checkpoint)?;
    // match the proof and exact preparation before accepting owner-returned results
    if text(&current, "proofSha256").as_deref() != Some(proof.as_str())
        || text(&preparation, "proofSha256").as_deref() != Some(proof.as_str())
        || text(&preparation, "preparationSha256").as_deref() != Some(preparation_hash.as_str())
        || !matching_results(
            results,
            bounded_array(&preparation, "finalizations").ok_or(Checkpoint)?,
        )
    {
        return Err(Checkpoint);
    }
    let consumption = JsonValue::Object(vec![
        member("consumedAt", string(now)),
        member("contractVersion", string(CONSUMPTION_VERSION)),
        member("preparationSha256", string(&preparation_hash)),
        member("proofSha256", string(&proof)),
        member("state", string("consumed")),
    ]);
    validate_consumption(&consumption, time).ok_or(Arguments)?;
    let preparation_bytes = encode(&preparation, codec)?;
    let current_bytes = encode(&current, codec)?;
    let consumption_bytes = encode(&consumption, codec)?;
    files.replace_previous(PreviousPreparation, &preparation_bytes)?;
    files.replace_previous(Previous, &current_bytes)?;
    files.replace_previous(PreviousConsumption, &consumption_bytes)?;
    files.remove_completed_marker(ShadowMetadataCompletedMarker::Current)?;
    files.remove_completed_marker(ShadowMetadataCompletedMarker::Preparation)?;
    files.sync_root()?;
    Ok(consumption)
}
