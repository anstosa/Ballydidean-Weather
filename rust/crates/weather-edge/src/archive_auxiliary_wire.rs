//! concrete canonical wire ownership for auxiliary revision archive artifacts.

use weather_contracts::{
    AdjustmentMaintenanceGapInput, AdjustmentMaintenanceGapInputFamilyEnum,
    AdjustmentRevisionGapInput, AdjustmentRevisionGapReason, AdjustmentRevisionProjectionKind,
    JsonValue, MaintenanceShadowGapReason, Nullable, RainFixedGaugeTargetGapInput,
    RainFixedGaugeTargetGapInputReasonLiteral, RainFixedGaugeTargetGapReceipt,
    RainFixedGaugeTargetGapReceiptContractVersionLiteral,
    RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral, RainFixedGaugeTargetGapStatus,
    RainFixedGaugeTargetGapStatusAbsent, RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
    RainFixedGaugeTargetGapStatusAbsentStateLiteral, RainFixedGaugeTargetGapStatusInput,
    RainFixedGaugeTargetGapStatusPresent,
    RainFixedGaugeTargetGapStatusPresentContractVersionLiteral,
    RainFixedGaugeTargetGapStatusPresentGap,
    RainFixedGaugeTargetGapStatusPresentGapContractVersionLiteral,
    RainFixedGaugeTargetGapStatusPresentGapQualificationDispositionLiteral,
    RainFixedGaugeTargetGapStatusPresentGapReasonLiteral,
    RainFixedGaugeTargetGapStatusPresentStateLiteral, RainMaintenanceControlStateGapInput,
    RainMaintenanceControlStateGapInputReasonLiteral, RainMaintenanceControlStateStageReceipt,
    RainMaintenanceControlStateStageReceiptContractVersionLiteral,
    RainMaintenanceControlStateStageReceiptDurableLiteral,
};
use weather_domain::{RainMaintenanceControlPrimitives, admit_rain_maintenance_control_state};

use crate::archive_wire::{
    ArchiveWireCodec, ArchiveWirePrimitives, bool_value, exact_keys, field, hash, member, string,
    text,
};
use crate::native_revision_spool::NativeRevisionSpoolArtifactCodec;
use crate::permanent_archive_gap::{PermanentArchiveGapCodec, PermanentArchiveGapInput};
use crate::private_archive_service::{
    AdjustmentMaintenanceGapReceipt, AdjustmentRevisionGapReceipt,
    RainMaintenanceControlStateGapReceipt,
};
use crate::rain_control_state_archive::RainControlStateWireCodec;
use crate::rain_fixed_gauge_gap_archive::RainFixedGaugeGapCodec;
use crate::revision_archive_canonical::{
    RevisionArchiveCanonicalCodec, instant, is_sha256, lower_hex,
};

const RAIN_GAP_VERSION: &str = "adjustment-rain-control-state-gap/v1";
const REVISION_GAP_VERSION: &str = "adjustment-revision-gap/v1";
const SHADOW_GAP_VERSION: &str = "adjustment-shadow-gap/v1";
const GAP_DISPOSITION: &str = "forever_unqualified";

impl<P: ArchiveWirePrimitives> ArchiveWireCodec<P> {
    /// decode a parsed rain-control stage receipt
    pub fn decode_rain_stage_value(
        &self,
        value: &JsonValue,
    ) -> Option<RainMaintenanceControlStateStageReceipt> {
        parse_rain_stage_receipt(self, value)
    }

    /// decode a parsed fixed-gauge gap receipt
    pub fn decode_fixed_gauge_gap_value(
        &self,
        value: &JsonValue,
    ) -> Option<RainFixedGaugeTargetGapReceipt> {
        parse_fixed_gauge_gap_receipt(value)
    }

    /// decode a parsed fixed-gauge status
    pub fn decode_fixed_gauge_status_value(
        &self,
        value: &JsonValue,
    ) -> Option<RainFixedGaugeTargetGapStatus> {
        parse_fixed_gauge_status(value)
    }
}

impl<P: ArchiveWirePrimitives> RainFixedGaugeGapCodec for ArchiveWireCodec<P> {
    /// Decode the closed receipt before the archive owner rebinds its exact key and digest.
    fn decode_receipt(&self, bytes: &[u8]) -> Option<RainFixedGaugeTargetGapReceipt> {
        let value = self.decode_canonical(bytes)?;
        parse_fixed_gauge_gap_receipt(&value)
    }
}

/// decode the generated fixed-gauge receipt fields
fn parse_fixed_gauge_gap_receipt(value: &JsonValue) -> Option<RainFixedGaugeTargetGapReceipt> {
    // retain the closed literal grammar
    if !exact_keys(
        value,
        &[
            "contractVersion",
            "gapAt",
            "gapSha256",
            "logicalHourAt",
            "logicalKeySha256",
            "qualificationDisposition",
            "reason",
        ],
    ) || text(value, "contractVersion")?
        != RainFixedGaugeTargetGapReceiptContractVersionLiteral::WIRE_VALUE
        || text(value, "qualificationDisposition")?
            != RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral::WIRE_VALUE
        || text(value, "reason")? != RainFixedGaugeTargetGapInputReasonLiteral::WIRE_VALUE
    {
        return None;
    }
    Some(RainFixedGaugeTargetGapReceipt {
        logicalHourAt: text(value, "logicalHourAt")?,
        logicalKeySha256: hash(value, "logicalKeySha256")?,
        reason: RainFixedGaugeTargetGapInputReasonLiteral,
        contractVersion: RainFixedGaugeTargetGapReceiptContractVersionLiteral,
        gapAt: text(value, "gapAt")?,
        gapSha256: hash(value, "gapSha256")?,
        qualificationDisposition: RainFixedGaugeTargetGapReceiptQualificationDispositionLiteral,
    })
}

/// decode the generated fixed-gauge status variants
fn parse_fixed_gauge_status(value: &JsonValue) -> Option<RainFixedGaugeTargetGapStatus> {
    let state = text(value, "state")?;
    // select only the generated status discriminants
    match state.as_str() {
        RainFixedGaugeTargetGapStatusAbsentStateLiteral::WIRE_VALUE => {
            exact_keys(value, &["contractVersion", "state"]).then_some(())?;
            (text(value, "contractVersion")?
                == RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral::WIRE_VALUE)
                .then_some(RainFixedGaugeTargetGapStatus::Absent(
                    RainFixedGaugeTargetGapStatusAbsent {
                        contractVersion: RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
                        state: RainFixedGaugeTargetGapStatusAbsentStateLiteral,
                    },
                ))
        }
        RainFixedGaugeTargetGapStatusPresentStateLiteral::WIRE_VALUE => {
            exact_keys(value, &["contractVersion", "gap", "state"]).then_some(())?;
            (text(value, "contractVersion")?
                == RainFixedGaugeTargetGapStatusPresentContractVersionLiteral::WIRE_VALUE)
                .then_some(())?;
            let gap = parse_fixed_gauge_gap_receipt(field(value, "gap")?)?;
            Some(RainFixedGaugeTargetGapStatus::Present(
                RainFixedGaugeTargetGapStatusPresent {
                    contractVersion: RainFixedGaugeTargetGapStatusPresentContractVersionLiteral,
                    gap: RainFixedGaugeTargetGapStatusPresentGap {
                        contractVersion:
                            RainFixedGaugeTargetGapStatusPresentGapContractVersionLiteral,
                        gapAt: gap.gapAt,
                        gapSha256: gap.gapSha256,
                        logicalHourAt: gap.logicalHourAt,
                        logicalKeySha256: gap.logicalKeySha256,
                        qualificationDisposition:
                            RainFixedGaugeTargetGapStatusPresentGapQualificationDispositionLiteral,
                        reason: RainFixedGaugeTargetGapStatusPresentGapReasonLiteral,
                    },
                    state: RainFixedGaugeTargetGapStatusPresentStateLiteral,
                },
            ))
        }
        _ => None,
    }
}

/// bind the shared codec to the domain's complete control-state admission contract.
struct ControlAdmission<'a, P> {
    codec: &'a ArchiveWireCodec<P>,
}

impl<P: ArchiveWirePrimitives> RainMaintenanceControlPrimitives for ControlAdmission<'_, P> {
    /// decode without changing source key order or scalar spellings.
    fn decode_control_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.codec.decode(bytes)
    }

    /// reproduce the producer-defined state member order.
    fn encode_control_ordered_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.codec.encode_ordered(value)
    }

    /// reproduce the recursively sorted identity preimage.
    fn encode_control_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.codec.encode(value)
    }

    /// compare one identity through the shared digest primitive.
    fn control_sha256_matches(&self, bytes: &[u8], expected: &str) -> bool {
        lower_hex(self.codec.sha256(bytes)) == expected
    }

    /// resolve one canonical UTC instant through the shared calendar primitive.
    fn control_utc_milliseconds(&self, utc: &str) -> Option<i64> {
        self.codec.canonical_utc_milliseconds(utc)
    }
}

/// decode one self-addressed rain-control stage receipt
fn parse_rain_stage_receipt<P: ArchiveWirePrimitives>(
    codec: &ArchiveWireCodec<P>,
    value: &JsonValue,
) -> Option<RainMaintenanceControlStateStageReceipt> {
    // retain the complete closed receipt grammar
    exact_keys(
        value,
        &[
            "contractVersion",
            "durable",
            "durableAt",
            "stageReceiptSha256",
            "stateSha256",
        ],
    )
    .then_some(())?;
    let durable_at = text(value, "durableAt")?;
    let stage_receipt_sha256 = hash(value, "stageReceiptSha256")?;
    let state_sha256 = hash(value, "stateSha256")?;
    // retain literals and canonical clock
    (text(value, "contractVersion")?
        == RainMaintenanceControlStateStageReceiptContractVersionLiteral::WIRE_VALUE
        && bool_value(value, "durable")?
        && instant(codec, &durable_at).is_some())
    .then_some(())?;
    let unsigned = rain_stage_value(&durable_at, &state_sha256, None);
    let expected = lower_hex(codec.sha256(&codec.encode(&unsigned)?));
    (stage_receipt_sha256 == expected).then_some(RainMaintenanceControlStateStageReceipt {
        contractVersion: RainMaintenanceControlStateStageReceiptContractVersionLiteral,
        durable: RainMaintenanceControlStateStageReceiptDurableLiteral,
        durableAt: durable_at,
        stateSha256: state_sha256,
        stageReceiptSha256: stage_receipt_sha256,
    })
}

impl<P: ArchiveWirePrimitives> RainControlStateWireCodec for ArchiveWireCodec<P> {
    /// decode the original state for complete domain admission by its owning parser.
    fn decode_control_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.decode(bytes)
    }

    /// preserve the exact producer order used by the retained state bytes.
    fn encode_control_ordered_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.encode_ordered(value)
    }

    /// preserve the exact sorted state identity preimage.
    fn encode_control_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.encode(value)
    }

    /// decode one canonical self-addressed rain-control stage receipt.
    fn decode_stage_receipt(
        &self,
        bytes: &[u8],
    ) -> Option<RainMaintenanceControlStateStageReceipt> {
        let value = self.decode_canonical(bytes)?;
        let receipt = parse_rain_stage_receipt(self, &value)?;
        // retain the canonical persisted representation while parsed API values stay order-neutral
        (self.encode(&rain_stage_receipt_value(&receipt))?.as_slice() == bytes).then_some(receipt)
    }

    /// decode one canonical self-addressed rain-control abandonment receipt.
    fn decode_gap_receipt(&self, bytes: &[u8]) -> Option<RainMaintenanceControlStateGapReceipt> {
        let value = self.decode_canonical(bytes)?;
        // require the complete closed abandonment before reading typed members
        exact_keys(
            &value,
            &[
                "contractVersion",
                "gapAt",
                "gapSha256",
                "reason",
                "stageReceiptSha256",
                "stateSha256",
            ],
        )
        .then_some(())?;
        let gap_at = text(&value, "gapAt")?;
        let gap_sha256 = hash(&value, "gapSha256")?;
        let stage_receipt_sha256 = hash(&value, "stageReceiptSha256")?;
        let state_sha256 = hash(&value, "stateSha256")?;
        // retain the exact source contract, category, and canonical clock
        (text(&value, "contractVersion")? == RAIN_GAP_VERSION
            && text(&value, "reason")?
                == RainMaintenanceControlStateGapInputReasonLiteral::WIRE_VALUE
            && instant(self, &gap_at).is_some())
        .then_some(())?;
        let unsigned = rain_gap_value(&gap_at, &stage_receipt_sha256, &state_sha256, None);
        let expected = lower_hex(self.sha256(&self.encode(&unsigned)?));
        // bind the self-address and the exact signed canonical representation
        (gap_sha256 == expected
            && self
                .encode(&rain_gap_value(
                    &gap_at,
                    &stage_receipt_sha256,
                    &state_sha256,
                    Some(&gap_sha256),
                ))?
                .as_slice()
                == bytes)
            .then_some(RainMaintenanceControlStateGapReceipt {
                contract_version: RAIN_GAP_VERSION.to_owned(),
                gap_at,
                gap_sha256,
                reason: RainMaintenanceControlStateGapInputReasonLiteral,
                stage_receipt_sha256,
                state_sha256,
            })
    }
}

impl<P: ArchiveWirePrimitives> PermanentArchiveGapCodec for ArchiveWireCodec<P> {
    /// encode one validated canonical permanent-gap locator.
    fn encode_gap_key(&self, input: PermanentArchiveGapInput<'_>) -> Option<Vec<u8>> {
        input.validated_kind().ok()?;
        self.encode(&match input {
            PermanentArchiveGapInput::Revision(value) => JsonValue::Object(vec![
                member("logicalKeySha256", string(&value.logicalKeySha256)),
                member(
                    "projectionIdentitySha256",
                    nullable_string(&value.projectionIdentitySha256),
                ),
            ]),
            PermanentArchiveGapInput::Shadow(value) => JsonValue::Object(vec![
                member("dueKey", string(&value.dueKey)),
                member("registrationSha256", string(&value.registrationSha256)),
            ]),
        })
    }

    /// encode the exact unsigned preimage or signed permanent-gap receipt.
    fn encode_gap_receipt(
        &self,
        input: PermanentArchiveGapInput<'_>,
        recorded_at: &str,
        digest: Option<&str>,
    ) -> Option<Vec<u8>> {
        input.validated_kind().ok()?;
        instant(self, recorded_at)?;
        // reject every noncanonical optional receipt identity before serialization
        digest.is_none_or(is_sha256).then_some(())?;
        self.encode(&match input {
            PermanentArchiveGapInput::Revision(value) => {
                revision_gap_value(value, recorded_at, digest)
            }
            PermanentArchiveGapInput::Shadow(value) => shadow_gap_value(value, recorded_at, digest),
        })
    }

    /// decode one exact canonical permanent revision classification.
    fn accepted_revision_gap(&self, bytes: &[u8]) -> Option<AdjustmentRevisionGapReceipt> {
        let value = self.decode_canonical(bytes)?;
        accepted_gap_envelope(&value, REVISION_GAP_VERSION, self)?;
        let input = revision_gap_input(field(&value, "input")?)?;
        input_gap_valid(PermanentArchiveGapInput::Revision(&input))?;
        let recorded_at = text(&value, "recordedAt")?;
        let gap_sha256 = hash(&value, "gapSha256")?;
        let unsigned = self.encode_gap_receipt(
            PermanentArchiveGapInput::Revision(&input),
            &recorded_at,
            None,
        )?;
        // bind the unsigned identity and exact typed signed mapping
        (gap_sha256 == lower_hex(self.sha256(&unsigned))
            && self
                .encode_gap_receipt(
                    PermanentArchiveGapInput::Revision(&input),
                    &recorded_at,
                    Some(&gap_sha256),
                )?
                .as_slice()
                == bytes)
            .then_some(AdjustmentRevisionGapReceipt {
                contract_version: REVISION_GAP_VERSION.to_owned(),
                input,
                qualification_disposition: GAP_DISPOSITION.to_owned(),
                recorded_at,
                gap_sha256,
            })
    }

    /// decode one exact canonical permanent shadow classification.
    fn accepted_shadow_gap(&self, bytes: &[u8]) -> Option<AdjustmentMaintenanceGapReceipt> {
        let value = self.decode_canonical(bytes)?;
        accepted_gap_envelope(&value, SHADOW_GAP_VERSION, self)?;
        let input = shadow_gap_input(field(&value, "input")?)?;
        input_gap_valid(PermanentArchiveGapInput::Shadow(&input))?;
        let recorded_at = text(&value, "recordedAt")?;
        let gap_sha256 = hash(&value, "gapSha256")?;
        let unsigned =
            self.encode_gap_receipt(PermanentArchiveGapInput::Shadow(&input), &recorded_at, None)?;
        // bind the unsigned identity and exact typed signed mapping
        (gap_sha256 == lower_hex(self.sha256(&unsigned))
            && self
                .encode_gap_receipt(
                    PermanentArchiveGapInput::Shadow(&input),
                    &recorded_at,
                    Some(&gap_sha256),
                )?
                .as_slice()
                == bytes)
            .then_some(AdjustmentMaintenanceGapReceipt {
                contract_version: SHADOW_GAP_VERSION.to_owned(),
                input,
                qualification_disposition: GAP_DISPOSITION.to_owned(),
                recorded_at,
                gap_sha256,
            })
    }
}

impl<P: ArchiveWirePrimitives> NativeRevisionSpoolArtifactCodec for ArchiveWireCodec<P> {
    /// return the ordinal identity only after complete retained-state admission.
    fn admitted_rain_control_ordinal_sha256(&self, bytes: &[u8]) -> Option<String> {
        admit_rain_maintenance_control_state(&ControlAdmission { codec: self }, bytes)
            .map(|state| state.ordinal_artifact_sha256)
    }
}

/// map one typed stage receipt to the exact canonical JSON member set.
pub(crate) fn rain_stage_receipt_value(
    value: &RainMaintenanceControlStateStageReceipt,
) -> JsonValue {
    rain_stage_value(
        &value.durableAt,
        &value.stateSha256,
        Some(&value.stageReceiptSha256),
    )
}

/// map one typed revision disposition to its unsigned or signed canonical document.
pub(crate) fn revision_gap_value(
    input: &AdjustmentRevisionGapInput,
    recorded_at: &str,
    digest: Option<&str>,
) -> JsonValue {
    gap_envelope_value(
        REVISION_GAP_VERSION,
        revision_gap_input_value(input),
        recorded_at,
        digest,
    )
}

/// map one typed shadow disposition to its unsigned or signed canonical document.
pub(crate) fn shadow_gap_value(
    input: &AdjustmentMaintenanceGapInput,
    recorded_at: &str,
    digest: Option<&str>,
) -> JsonValue {
    gap_envelope_value(
        SHADOW_GAP_VERSION,
        shadow_gap_input_value(input),
        recorded_at,
        digest,
    )
}

/// build one unsigned or signed rain-control stage document.
fn rain_stage_value(durable_at: &str, state_sha256: &str, digest: Option<&str>) -> JsonValue {
    let mut fields = vec![
        member(
            "contractVersion",
            string(RainMaintenanceControlStateStageReceiptContractVersionLiteral::WIRE_VALUE),
        ),
        member("durable", JsonValue::Boolean(true)),
        member("durableAt", string(durable_at)),
    ];
    // insert the optional identity at its canonical key position
    if let Some(digest) = digest {
        fields.push(member("stageReceiptSha256", string(digest)));
    }
    fields.push(member("stateSha256", string(state_sha256)));
    JsonValue::Object(fields)
}

/// build one unsigned or signed rain-control gap document.
pub(crate) fn rain_gap_value(
    gap_at: &str,
    stage_receipt_sha256: &str,
    state_sha256: &str,
    digest: Option<&str>,
) -> JsonValue {
    let mut fields = vec![
        member("contractVersion", string(RAIN_GAP_VERSION)),
        member("gapAt", string(gap_at)),
    ];
    // insert the optional identity at its canonical key position
    if let Some(digest) = digest {
        fields.push(member("gapSha256", string(digest)));
    }
    fields.extend([
        member(
            "reason",
            string(RainMaintenanceControlStateGapInputReasonLiteral::WIRE_VALUE),
        ),
        member("stageReceiptSha256", string(stage_receipt_sha256)),
        member("stateSha256", string(state_sha256)),
    ]);
    JsonValue::Object(fields)
}

/// build one shared unsigned or signed permanent-gap envelope.
fn gap_envelope_value(
    contract_version: &str,
    input: JsonValue,
    recorded_at: &str,
    digest: Option<&str>,
) -> JsonValue {
    let mut fields = vec![member("contractVersion", string(contract_version))];
    // insert the optional identity at its canonical key position
    if let Some(digest) = digest {
        fields.push(member("gapSha256", string(digest)));
    }
    fields.extend([
        member("input", input),
        member("qualificationDisposition", string(GAP_DISPOSITION)),
        member("recordedAt", string(recorded_at)),
    ]);
    JsonValue::Object(fields)
}

/// map one typed rain-control gap input
pub fn rain_control_gap_input_value(input: &RainMaintenanceControlStateGapInput) -> JsonValue {
    JsonValue::Object(vec![
        member(
            "reason",
            string(RainMaintenanceControlStateGapInputReasonLiteral::WIRE_VALUE),
        ),
        member("stateSha256", string(&input.stateSha256)),
        member("stageReceiptSha256", string(&input.stageReceiptSha256)),
    ])
}

/// map one typed fixed-gauge gap input
pub fn fixed_gauge_gap_input_value(input: &RainFixedGaugeTargetGapInput) -> JsonValue {
    JsonValue::Object(vec![
        member("logicalHourAt", string(&input.logicalHourAt)),
        member("logicalKeySha256", string(&input.logicalKeySha256)),
        member(
            "reason",
            string(RainFixedGaugeTargetGapInputReasonLiteral::WIRE_VALUE),
        ),
    ])
}

/// map one typed fixed-gauge status input
pub fn fixed_gauge_status_input_value(input: &RainFixedGaugeTargetGapStatusInput) -> JsonValue {
    JsonValue::Object(vec![
        member("logicalHourAt", string(&input.logicalHourAt)),
        member("logicalKeySha256", string(&input.logicalKeySha256)),
    ])
}

/// map one typed revision-gap input
pub fn revision_gap_input_value(input: &AdjustmentRevisionGapInput) -> JsonValue {
    JsonValue::Object(vec![
        member("logicalKeySha256", string(&input.logicalKeySha256)),
        member(
            "projectionIdentitySha256",
            nullable_string(&input.projectionIdentitySha256),
        ),
        member(
            "projectionKind",
            string(projection_kind_text(input.projectionKind)),
        ),
        member("projectionSha256", nullable_string(&input.projectionSha256)),
        member("reason", string(revision_reason_text(input.reason))),
    ])
}

/// map one typed scheduled shadow-gap input
pub fn shadow_gap_input_value(input: &AdjustmentMaintenanceGapInput) -> JsonValue {
    JsonValue::Object(vec![
        member("dueKey", string(&input.dueKey)),
        member("family", string(shadow_family_text(input.family))),
        member("reason", string(shadow_reason_text(input.reason))),
        member("registrationSha256", string(&input.registrationSha256)),
    ])
}

/// decode one exact revision-gap input through generated enums and nullable values.
pub(crate) fn revision_gap_input(value: &JsonValue) -> Option<AdjustmentRevisionGapInput> {
    // admit exactly the frozen five-member input grammar
    exact_keys(
        value,
        &[
            "logicalKeySha256",
            "projectionIdentitySha256",
            "projectionKind",
            "projectionSha256",
            "reason",
        ],
    )
    .then_some(())?;
    Some(AdjustmentRevisionGapInput {
        logicalKeySha256: hash(value, "logicalKeySha256")?,
        projectionIdentitySha256: nullable_hash(value, "projectionIdentitySha256")?,
        projectionKind: parse_projection_kind(&text(value, "projectionKind")?)?,
        projectionSha256: nullable_hash(value, "projectionSha256")?,
        reason: parse_revision_reason(&text(value, "reason")?)?,
    })
}

/// decode one exact scheduled shadow-gap input through generated enums.
pub(crate) fn shadow_gap_input(value: &JsonValue) -> Option<AdjustmentMaintenanceGapInput> {
    // admit exactly the frozen four-member input grammar
    exact_keys(value, &["dueKey", "family", "reason", "registrationSha256"]).then_some(())?;
    Some(AdjustmentMaintenanceGapInput {
        dueKey: text(value, "dueKey")?,
        family: parse_shadow_family(&text(value, "family")?)?,
        reason: parse_shadow_reason(&text(value, "reason")?)?,
        registrationSha256: hash(value, "registrationSha256")?,
    })
}

/// verify the shared permanent-gap envelope before decoding its categorical input.
fn accepted_gap_envelope(
    value: &JsonValue,
    contract_version: &str,
    codec: &impl RevisionArchiveCanonicalCodec,
) -> Option<()> {
    // require all and only the five durable receipt members
    exact_keys(
        value,
        &[
            "contractVersion",
            "gapSha256",
            "input",
            "qualificationDisposition",
            "recordedAt",
        ],
    )
    .then_some(())?;
    let recorded_at = text(value, "recordedAt")?;
    hash(value, "gapSha256")?;
    // retain the exact source version, disposition, and canonical clock
    (text(value, "contractVersion")? == contract_version
        && text(value, "qualificationDisposition")? == GAP_DISPOSITION
        && instant(codec, &recorded_at).is_some())
    .then_some(())
}

/// convert the internal gap validation result into an option parser boundary.
fn input_gap_valid(input: PermanentArchiveGapInput<'_>) -> Option<()> {
    input.validated_kind().ok().map(|_| ())
}

/// decode one strict nullable digest member.
fn nullable_hash(value: &JsonValue, name: &str) -> Option<Nullable<String>> {
    match field(value, name)? {
        JsonValue::Null => Some(Nullable::Null),
        JsonValue::String(_) => hash(value, name).map(Nullable::Value),
        _ => None,
    }
}

/// encode one generated nullable string without introducing an alternate form.
fn nullable_string(value: &Nullable<String>) -> JsonValue {
    match value {
        Nullable::Null => JsonValue::Null,
        Nullable::Value(value) => string(value),
    }
}

/// decode one generated projection-kind wire spelling.
fn parse_projection_kind(value: &str) -> Option<AdjustmentRevisionProjectionKind> {
    enum_value(AdjustmentRevisionProjectionKind::WIRE_VALUES, value)
}

/// decode one generated revision failure-category spelling.
fn parse_revision_reason(value: &str) -> Option<AdjustmentRevisionGapReason> {
    enum_value(AdjustmentRevisionGapReason::WIRE_VALUES, value)
}

/// decode one generated shadow-family spelling.
fn parse_shadow_family(value: &str) -> Option<AdjustmentMaintenanceGapInputFamilyEnum> {
    enum_value(AdjustmentMaintenanceGapInputFamilyEnum::WIRE_VALUES, value)
}

/// decode one generated shadow failure-category spelling.
fn parse_shadow_reason(value: &str) -> Option<MaintenanceShadowGapReason> {
    enum_value(MaintenanceShadowGapReason::WIRE_VALUES, value)
}

/// encode one generated projection-kind wire spelling.
fn projection_kind_text(value: AdjustmentRevisionProjectionKind) -> &'static str {
    enum_text(AdjustmentRevisionProjectionKind::WIRE_VALUES, value)
}

/// encode one generated revision failure-category spelling.
fn revision_reason_text(value: AdjustmentRevisionGapReason) -> &'static str {
    enum_text(AdjustmentRevisionGapReason::WIRE_VALUES, value)
}

/// encode one generated shadow-family wire spelling.
fn shadow_family_text(value: AdjustmentMaintenanceGapInputFamilyEnum) -> &'static str {
    enum_text(AdjustmentMaintenanceGapInputFamilyEnum::WIRE_VALUES, value)
}

/// encode one generated shadow failure-category spelling.
fn shadow_reason_text(value: MaintenanceShadowGapReason) -> &'static str {
    enum_text(MaintenanceShadowGapReason::WIRE_VALUES, value)
}

/// select one generated closed-enum value without copying its contract table.
fn enum_value<T: Copy>(values: &[(&str, T)], wire: &str) -> Option<T> {
    values
        .iter()
        .find_map(|(candidate, value)| (*candidate == wire).then_some(*value))
}

/// select one generated closed-enum spelling for an already typed value.
fn enum_text<T: Copy + PartialEq>(values: &'static [(&'static str, T)], value: T) -> &'static str {
    values
        .iter()
        .find_map(|(wire, candidate)| (*candidate == value).then_some(*wire))
        .expect("generated enum value is represented in its wire table")
}
