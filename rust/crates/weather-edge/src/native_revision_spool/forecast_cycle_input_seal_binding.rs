//! Bind typed required producer roots to the original canonical seal framing.

use weather_contracts::JsonValue;

use crate::ForecastEvidenceReceiptCodec;

use super::forecast_cycle_input_seal::{
    FORECAST_CYCLE_INPUT_SEAL_CONTRACT_VERSION, ForecastCycleInputSeal,
    ForecastCycleInputSealCodec, ForecastCycleRequiredInputRoot, ForecastCycleRequiredInputs,
    valid_input_seal,
};
use super::forecast_cycle_page_binding::{
    BoundForecastCyclePageCodec, digest, object, read_digest, read_field, read_generation,
    read_number, read_object, read_text, text,
};

impl<Codec: ForecastEvidenceReceiptCodec> ForecastCycleInputSealCodec
    for BoundForecastCyclePageCodec<'_, Codec>
{
    /// Persist the exact existing producer-declaration schema.
    fn encode_input_seal(&self, seal: &ForecastCycleInputSeal) -> Option<Vec<u8>> {
        self.frame(&seal_value(seal)?)
    }

    /// Decode only the exact canonical document, including every mandatory population.
    fn decode_input_seal(&self, bytes: &[u8]) -> Option<ForecastCycleInputSeal> {
        let value = self.byte_codec().decode_retained_json(bytes)?;
        // verify original framing before checking the admitted sealed fields
        if bytes.len() > 2_048 || self.frame(&value)? != bytes {
            return None;
        }
        self.admit_input_seal_value(&value)
    }

    /// Retain the source filename identity over its declared decimal generation.
    fn encode_input_seal_identity(&self, due_key: &str, generation: u64) -> Option<Vec<u8>> {
        self.frame(&object([
            ("dueKey", text(due_key)),
            ("generation", text(&generation.to_string())),
        ]))
    }

    /// The empty population uses the same sorted canonical array framing.
    fn encode_empty_input_population(&self) -> Option<Vec<u8>> {
        self.frame(&JsonValue::Array(Vec::new()))
    }
}

impl<Codec: ForecastEvidenceReceiptCodec> BoundForecastCyclePageCodec<'_, Codec> {
    /// Validate the exact same seal fields when a final record embeds the declaration.
    pub(super) fn admit_input_seal_value(
        &self,
        value: &JsonValue,
    ) -> Option<ForecastCycleInputSeal> {
        let seal = read_seal_value(value)?;
        (valid_input_seal(&seal, self)? && self.frame(value)?.len() <= 2_048).then_some(seal)
    }
}

/// Project the full existing ten-field schema with no implicit population.
pub(super) fn seal_value(value: &ForecastCycleInputSeal) -> Option<JsonValue> {
    let required = &value.required_inputs;
    Some(object([
        (
            "acknowledgedProjectionCount",
            count_value(value.acknowledged_projection_count)?,
        ),
        (
            "acknowledgedProjectionRootSha256",
            digest(value.acknowledged_projection_root_sha256),
        ),
        (
            "acknowledgementRootSha256",
            digest(value.acknowledgement_root_sha256),
        ),
        ("contractVersion", text(value.contract_version)),
        ("dueKey", text(&value.due_key)),
        (
            "evidenceGapRootSha256",
            digest(value.evidence_gap_root_sha256),
        ),
        ("generation", text(&value.generation.to_string())),
        ("pageRootSha256", digest(value.page_root_sha256)),
        (
            "requiredInputs",
            object([
                ("actual_best_match", input_root(required.actual_best_match)?),
                ("native_source", input_root(required.native_source)?),
                ("rain_gate_input", input_root(required.rain_gate_input)?),
                ("serving_evidence", input_root(required.serving_evidence)?),
                ("shadow_body", input_root(required.shadow_body)?),
                ("shadow_metadata", input_root(required.shadow_metadata)?),
                ("target_revision", input_root(required.target_revision)?),
            ]),
        ),
        ("sealedAt", text(&value.sealed_at)),
    ]))
}

/// Keep a bounded provider population as the original exact JSON integer.
fn count_value(count: usize) -> Option<JsonValue> {
    let count: u32 = count.try_into().ok()?;
    Some(JsonValue::Number(f64::from(count)))
}

/// Encode one exact source population count and content identity.
fn input_root(root: ForecastCycleRequiredInputRoot) -> Option<JsonValue> {
    Some(object([
        ("count", count_value(root.count)?),
        ("rootSha256", digest(root.root_sha256)),
    ]))
}

/// Read the complete closed source schema before applying typed admission.
fn read_seal_value(value: &JsonValue) -> Option<ForecastCycleInputSeal> {
    let fields = read_object(value)?;
    // reject anything outside the exact producer-declaration schema
    if fields.len() != 10
        || read_text(read_field(fields, "contractVersion")?)?
            != FORECAST_CYCLE_INPUT_SEAL_CONTRACT_VERSION
    {
        return None;
    }
    let required = read_object(read_field(fields, "requiredInputs")?)?;
    // every population must be explicit
    if required.len() != 7 {
        return None;
    }
    Some(ForecastCycleInputSeal {
        acknowledged_projection_count: read_number(read_field(
            fields,
            "acknowledgedProjectionCount",
        )?)?
        .try_into()
        .ok()?,
        acknowledged_projection_root_sha256: read_digest(read_field(
            fields,
            "acknowledgedProjectionRootSha256",
        )?)?,
        acknowledgement_root_sha256: read_digest(read_field(fields, "acknowledgementRootSha256")?)?,
        contract_version: FORECAST_CYCLE_INPUT_SEAL_CONTRACT_VERSION,
        due_key: read_text(read_field(fields, "dueKey")?)?,
        evidence_gap_root_sha256: read_digest(read_field(fields, "evidenceGapRootSha256")?)?,
        generation: read_generation(read_field(fields, "generation")?)?,
        page_root_sha256: read_digest(read_field(fields, "pageRootSha256")?)?,
        required_inputs: ForecastCycleRequiredInputs {
            actual_best_match: read_input_root(read_field(required, "actual_best_match")?)?,
            native_source: read_input_root(read_field(required, "native_source")?)?,
            rain_gate_input: read_input_root(read_field(required, "rain_gate_input")?)?,
            serving_evidence: read_input_root(read_field(required, "serving_evidence")?)?,
            shadow_body: read_input_root(read_field(required, "shadow_body")?)?,
            shadow_metadata: read_input_root(read_field(required, "shadow_metadata")?)?,
            target_revision: read_input_root(read_field(required, "target_revision")?)?,
        },
        sealed_at: read_text(read_field(fields, "sealedAt")?)?,
    })
}

/// Decode one full required-population record without aliases.
fn read_input_root(value: &JsonValue) -> Option<ForecastCycleRequiredInputRoot> {
    let root = read_object(value)?;
    // require the original two-member record before admitting either field
    if root.len() != 2 {
        return None;
    }
    Some(ForecastCycleRequiredInputRoot {
        count: read_number(read_field(root, "count")?)?.try_into().ok()?,
        root_sha256: read_digest(read_field(root, "rootSha256")?)?,
    })
}
