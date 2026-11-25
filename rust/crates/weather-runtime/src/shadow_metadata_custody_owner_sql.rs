//! Exact closed PostgreSQL owner statements for compact shadow metadata custody.

use std::collections::HashSet;

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::MaintenanceAnchorStatusJson;
use crate::maintenance_canonical_json::{field, text};
use crate::shadow_metadata_custody::ShadowMetadataCustodyError;
use crate::shadow_metadata_custody_preparation::validate_shadow_custody_preparation;
use crate::shadow_metadata_custody_proof::validate_shadow_custody_proof;

/// Build the original read-only owner query that derives exact custody finalization arguments.
pub(crate) fn build_shadow_metadata_preparation_sql(
    proof: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Vec<u8>, ShadowMetadataCustodyError> {
    use ShadowMetadataCustodyError::Slot;

    validate_shadow_custody_proof(proof, codec, time).ok_or(Slot)?;
    let JsonValue::Array(entries) = field(proof, "entries").ok_or(Slot)? else {
        return Err(Slot);
    };
    let mut predictions = HashSet::with_capacity(entries.len());
    let mut expected = Vec::with_capacity(entries.len());
    // remove only the capsule identity that the native row cannot reproduce
    for entry in entries {
        let prediction = text(entry, "predictionSha256").ok_or(Slot)?;
        // reject repeated native-row identities before constructing owner SQL
        if !predictions.insert(prediction) {
            return Err(Slot);
        }
        let JsonValue::Object(fields) = entry else {
            return Err(Slot);
        };
        expected.push(JsonValue::Object(
            fields
                .iter()
                .filter(|(name, _)| !name.iter().copied().eq("capsuleSha256".encode_utf16()))
                .cloned()
                .collect(),
        ));
    }
    let literal = canonical_value(&JsonValue::Array(expected), codec)?;
    let custody = text(proof, "custodyCheckpointSha256").ok_or(Slot)?;
    let proof = text(proof, "proofSha256").ok_or(Slot)?;
    let receipt_object = "jsonb_build_object('bodyByteCount', prediction.body_byte_count, 'dueKey', prediction.due_key, 'inputSha256', prediction.input_sha256, 'issuedAt', to_char(prediction.issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'maxValidAt', to_char(prediction.max_valid_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'minValidAt', to_char(prediction.min_valid_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'predictionBodySha256', prediction.prediction_body_sha256, 'predictionSchemaSha256', prediction.prediction_schema_sha256, 'predictionSha256', prediction.prediction_sha256, 'rowCount', prediction.row_count, 'sourceReceiptSha256', prediction.source_receipt_sha256, 'archiveCommitOrdinal', prediction.archive_commit_ordinal::text, 'archiveCommittedAt', to_char(prediction.archive_committed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'frontierSha256', prediction.revision_frontier_sha256, 'predictionCommittedAt', to_char(prediction.committed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'predecessorFrontierSha256', prediction.predecessor_frontier_sha256, 'receiptSha256', prediction.revision_receipt_sha256, 'registrationSha256', prediction.registration_sha256, 'stageReceiptSha256', prediction.stage_receipt_sha256)";
    let compact_object = "jsonb_build_object('bodyByteCount', prediction.body_byte_count, 'committedAt', to_char(prediction.committed_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'dueKey', prediction.due_key, 'inputSha256', prediction.input_sha256, 'issuedAt', to_char(prediction.issued_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'maxValidAt', to_char(prediction.max_valid_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'minValidAt', to_char(prediction.min_valid_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"'), 'predictionBodySha256', prediction.prediction_body_sha256, 'predictionSchemaSha256', prediction.prediction_schema_sha256, 'predictionSha256', prediction.prediction_sha256, 'rowCount', prediction.row_count, 'sourceReceiptSha256', prediction.source_receipt_sha256)";
    let sql = format!(
        r#"BEGIN ISOLATION LEVEL REPEATABLE READ READ ONLY;
SET LOCAL statement_timeout = '30s';
SET LOCAL lock_timeout = '5s';
DO $verify$
DECLARE expected jsonb := $proof${literal}$proof$::jsonb; entry jsonb; group_row record; selected_count integer;
BEGIN
  -- require each capsule proof to match the actual compact row and receipt pointers
  FOR entry IN SELECT value FROM jsonb_array_elements(expected) LOOP
    IF NOT EXISTS (SELECT 1 FROM adjustment_shadow_predictions_v2 prediction
      JOIN adjustment_shadow_registration_windows_v3 window_row USING (registration_sha256)
      WHERE prediction.prediction_sha256 = entry->>'predictionSha256'
        AND prediction.revision_contract_epoch = 1
        AND {receipt_object} = entry) THEN
      RAISE EXCEPTION 'shadow metadata custody row differs';
    END IF;
  END LOOP;
  -- prohibit deleting an unacknowledged sibling sharing a selected time range
  FOR group_row IN SELECT value->>'registrationSha256' AS registration,
    min((value->>'predictionCommittedAt')::timestamptz) AS first_at,
    max((value->>'predictionCommittedAt')::timestamptz) AS last_at, count(*) AS row_count
    FROM jsonb_array_elements(expected) GROUP BY value->>'registrationSha256' LOOP
    SELECT count(*) INTO selected_count FROM adjustment_shadow_predictions_v2 prediction
      WHERE prediction.registration_sha256 = group_row.registration
        AND prediction.committed_at BETWEEN group_row.first_at AND group_row.last_at;
    IF selected_count <> group_row.row_count THEN
      RAISE EXCEPTION 'shadow metadata custody range differs';
    END IF;
  END LOOP;
END;
$verify$;
WITH expected AS (SELECT value FROM jsonb_array_elements($proof${literal}$proof$::jsonb)),
groups AS (SELECT value->>'registrationSha256' AS registration,
  min((value->>'predictionCommittedAt')::timestamptz) AS first_at,
  max((value->>'predictionCommittedAt')::timestamptz) AS last_at
  FROM expected GROUP BY value->>'registrationSha256'),
selected AS (SELECT groups.registration, groups.first_at, groups.last_at, count(*)::integer AS row_count,
  encode(sha256(convert_to(jsonb_agg({compact_object} ORDER BY prediction.prediction_sha256)::text, 'UTF8')), 'hex') AS manifest
  FROM groups JOIN adjustment_shadow_predictions_v2 prediction ON prediction.registration_sha256 = groups.registration
    AND prediction.committed_at BETWEEN groups.first_at AND groups.last_at
  GROUP BY groups.registration, groups.first_at, groups.last_at),
arguments AS (SELECT jsonb_build_object(
  'coldCommitSha256', '{custody}',
  'expectedPreviousMetadataRootSha256', registration.finalized_metadata_root_sha256,
  'finalDisposition', 'retain', 'fromCommittedAt', to_char(selected.first_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"'),
  'generation', registration.metadata_generation + 1, 'maintenanceAnchorSha256', '{proof}',
  'metadataManifestSha256', selected.manifest,
  'newMetadataRootSha256', encode(sha256(convert_to(registration.finalized_metadata_root_sha256 || E'\n'
    || selected.manifest || E'\n' || (registration.metadata_generation + 1)::text || E'\n'
    || selected.row_count::text || E'\n' || to_char(selected.last_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"') || E'\n'
    || '{custody}' || E'\n' || '{proof}', 'UTF8')), 'hex'),
  'registrationSha256', selected.registration, 'rowCount', selected.row_count,
  'throughCommittedAt', to_char(selected.last_at AT TIME ZONE 'UTC', 'YYYY-MM-DD"T"HH24:MI:SS.MS"Z"')) AS argument
  FROM selected JOIN adjustment_shadow_registrations_v2 registration ON registration.registration_sha256 = selected.registration)
SELECT json_build_object('proofSha256', '{proof}',
  'finalizations', jsonb_agg(argument ORDER BY argument->>'registrationSha256')) FROM arguments;
COMMIT;
"#
    );
    // the closed SQL contains no caller-selected identifier or delimiter
    Ok(sql.into_bytes())
}

/// Build the original owner transaction that consumes one durable preparation.
pub(crate) fn build_shadow_metadata_finalization_sql(
    preparation: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Vec<u8>, ShadowMetadataCustodyError> {
    use ShadowMetadataCustodyError::Slot;

    validate_shadow_custody_preparation(preparation, codec, time).ok_or(Slot)?;
    let finalizations = field(preparation, "finalizations").ok_or(Slot)?;
    let literal = canonical_value(finalizations, codec)?;
    let proof = text(preparation, "proofSha256").ok_or(Slot)?;
    let preparation = text(preparation, "preparationSha256").ok_or(Slot)?;
    Ok(format!(
        r#"BEGIN;
SET LOCAL statement_timeout = '30s';
SET LOCAL lock_timeout = '5s';
SET LOCAL weather.adjustment_maintenance_anchor_kind = 'shadow_metadata_transfer';
SET LOCAL weather.adjustment_maintenance_anchor_sha256 = '{proof}';
WITH results AS MATERIALIZED (SELECT argument,
  weather_finalize_adjustment_shadow_metadata_v2(argument) AS result
  FROM jsonb_array_elements($arguments${literal}$arguments$::jsonb) argument)
SELECT json_build_object('preparationSha256', '{preparation}',
  'proofSha256', '{proof}', 'results', jsonb_agg(result || jsonb_build_object(
    'metadataManifestSha256', argument->>'metadataManifestSha256',
    'newMetadataRootSha256', argument->>'newMetadataRootSha256') ORDER BY argument->>'registrationSha256'))
FROM results;
COMMIT;
"#
    )
    .into_bytes())
}

// convert the shared LF-terminated canonical wire encoding to one SQL JSON literal
fn canonical_value(
    value: &JsonValue,
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<String, ShadowMetadataCustodyError> {
    let mut bytes = codec
        .encode_canonical_json(value)
        .ok_or(ShadowMetadataCustodyError::Codec)?;
    // remove exactly the wire terminator rather than trimming valid JSON content
    if bytes.pop() != Some(b'\n') {
        return Err(ShadowMetadataCustodyError::Codec);
    }
    String::from_utf8(bytes).map_err(|_| ShadowMetadataCustodyError::Codec)
}
