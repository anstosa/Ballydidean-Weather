//! Preserve the original installed family-action authority dispatcher and its distinct stores.

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::custody_anchor_validation::{valid_rain_due_month, validate_custody_anchor};
use crate::maintenance_anchor_status::{
    MAXIMUM_ANCHOR_BYTES, hash_field, regex_text, uint64_field, valid_source_commit,
};
use crate::maintenance_anchor_status_v2::validate_v2_anchor;
use crate::maintenance_anchor_status_v3::validate_v3_anchor;
use crate::maintenance_canonical_json::{field, text, valid_hash};
use crate::{CustodyAnchorKind, MaintenanceAnchorStatusError, MaintenanceAnchorStatusJson};

use MaintenanceAnchorStatusError::{Codec, Slot};

const RAIN_ACTION_VERSION: &str = "forecast-adjustment-rain-control-reference-action/v1";
const FUTURE_ANCHOR_VERSION: &str = "adjustment-maintenance-anchor/v3";

/// The three fixed authority stores selected by the original action-execution path.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FamilyActionAnchorKind {
    Development,
    RainControl,
    Maintenance,
}

/// The two distinct predecessor reads retained by the original family-action dispatcher.
pub trait FamilyActionAuthorityFiles {
    /// Read the first host-owned current file used to select and construct authority.
    fn read_host_current(
        &mut self,
        kind: FamilyActionAnchorKind,
    ) -> Result<Vec<u8>, MaintenanceAnchorStatusError>;

    /// Read the current store slot independently after the action projection is validated.
    fn read_validated_current(
        &mut self,
        kind: FamilyActionAnchorKind,
    ) -> Result<Option<Vec<u8>>, MaintenanceAnchorStatusError>;
}

/// Bind the already selected model or rain-control action to its exact installed anchor.
pub fn authorize_family_action(
    action: &JsonValue,
    action_sha256: &str,
    files: &mut impl FamilyActionAuthorityFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    use FamilyActionAnchorKind::{Development, Maintenance};

    let action_kind = text(action, "actionKind");
    let action_hash = JsonValue::String(action_sha256.encode_utf16().collect());
    // the rain-control action has its own custody store irrespective of model action kinds
    if text(action, "contractVersion").as_deref() == Some(RAIN_ACTION_VERSION) {
        return authorize_rain(action, &action_hash, files, codec, time);
    }
    // read the original first host slot before deriving development or maintenance operands
    let kind = if action_kind.as_deref() == Some("shadow") {
        Development
    } else {
        Maintenance
    };
    let first = canonical_object(&files.read_host_current(kind)?, codec)?;
    if kind == Development {
        return authorize_development(action, &action_hash, &first, files, codec, time);
    }
    // select future-only authority from the first maintenance slot rather than the action label
    if text(&first, "contractVersion").as_deref() == Some(FUTURE_ANCHOR_VERSION) {
        return authorize_future(action, &action_hash, files, codec, time);
    }
    let current = read_current(Maintenance, files, codec, time)?;
    // the legacy compensation path retains transferred or finalized graph authority
    if matches!(action_kind.as_deref(), Some("shadow" | "compensate_shadow")) {
        require_fields(
            &current,
            &first,
            &[
                ("catalogRootSha256", "catalogRootSha256"),
                ("controlSha256", "controlSha256"),
                ("controlVersion", "controlVersion"),
                ("graphManifestSha256", "graphManifestSha256"),
            ],
        )?;
        require_fields(
            &current,
            action,
            &[("sourceCommit", "expectedSourceCommit")],
        )?;
        return Ok(current);
    }
    // legacy qualification binds the burn, action, and finalized full graph identities
    if text(&current, "ctfState").as_deref() != Some("finalized") {
        return Err(Slot);
    }
    require_value(&current, "actionSha256", Some(&action_hash))?;
    require_fields(
        &current,
        &first,
        &[
            ("burnSha256", "burnSha256"),
            ("controlSha256", "controlSha256"),
            ("controlVersion", "controlVersion"),
            ("revisionSnapshotRootSha256", "revisionSnapshotRootSha256"),
            (
                "workstationJournalHeadSha256",
                "workstationJournalHeadSha256",
            ),
        ],
    )?;
    require_fields(
        &current,
        action,
        &[
            ("fullMemberRootSha256", "fullMemberRootSha256"),
            ("lifecycleLedgerRootSha256", "lifecycleHeadSha256"),
            ("reportSha256", "policyReportSha256"),
            ("sourceCommit", "expectedSourceCommit"),
        ],
    )?;
    Ok(current)
}

// retain bounded canonical host or store object parsing before any authority comparison
fn canonical_object(
    bytes: &[u8],
    codec: &mut impl MaintenanceAnchorStatusJson,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    // reject a noncanonical or incomplete original authority document
    if !(2..=MAXIMUM_ANCHOR_BYTES).contains(&bytes.len()) {
        return Err(Slot);
    }
    let value = codec.parse_canonical_object(bytes).ok_or(Slot)?;
    if !matches!(value, JsonValue::Object(_))
        || codec.encode_canonical_json(&value).as_deref() != Some(bytes)
    {
        return Err(Slot);
    }
    Ok(value)
}

// independently read and validate the current original store exactly once for this comparison
fn read_current(
    kind: FamilyActionAnchorKind,
    files: &mut impl FamilyActionAuthorityFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    let bytes = files.read_validated_current(kind)?.ok_or(Slot)?;
    let value = canonical_object(&bytes, codec)?;
    // both custody-only stores also produce an exact current anchor hash in the original read
    let custody_kind = match kind {
        FamilyActionAnchorKind::Development => Some(CustodyAnchorKind::Development),
        FamilyActionAnchorKind::RainControl => Some(CustodyAnchorKind::RainControl),
        FamilyActionAnchorKind::Maintenance => None,
    };
    if let Some(custody) = custody_kind {
        validate_custody_anchor(custody, &value, codec, time).ok_or(Slot)?;
        let checksum = codec.sha256(&bytes).ok_or(Codec)?;
        if !valid_hash(checksum.as_str()) {
            return Err(Codec);
        }
    } else if text(&value, "contractVersion").as_deref() == Some(FUTURE_ANCHOR_VERSION) {
        validate_v3_anchor(&value, codec, time).ok_or(Slot)?;
    } else {
        validate_v2_anchor(&value, codec, time).ok_or(Slot)?;
    }
    Ok(value)
}

// compare the original strict scalar equality rather than structurally equating separate JSON objects
fn require_value(
    current: &JsonValue,
    key: &str,
    expected: Option<&JsonValue>,
) -> Result<(), MaintenanceAnchorStatusError> {
    let observed = field(current, key);
    // independently parsed arrays and objects are never strictly equal in JavaScript
    if expected.is_some_and(|value| matches!(value, JsonValue::Array(_) | JsonValue::Object(_)))
        || observed != expected
    {
        return Err(Slot);
    }
    Ok(())
}

// match the fixed mapping from proposed action or first host slot to the independent current slot
fn require_fields(
    current: &JsonValue,
    input: &JsonValue,
    mappings: &[(&str, &str)],
) -> Result<(), MaintenanceAnchorStatusError> {
    // preserve the original exact equality for each selected field
    for (anchor_key, input_key) in mappings {
        require_value(current, anchor_key, field(input, input_key))?;
    }
    Ok(())
}

// admit only a lowercase SHA-256 operand before the original constrained store call
fn valid_action_hash(value: &JsonValue) -> bool {
    matches!(value, JsonValue::String(units) if String::from_utf16(units).ok().as_deref().is_some_and(valid_hash))
}

// bind one exact inactive rain control-reference action without granting model authority
fn authorize_rain(
    action: &JsonValue,
    action_hash: &JsonValue,
    files: &mut impl FamilyActionAuthorityFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    // retain the original action input grammar before reading its selected store
    if text(action, "actionKind").as_deref() != Some("control_reference")
        || !regex_text(action, "dueMonth", codec)
            .as_deref()
            .is_some_and(valid_rain_due_month)
        || !regex_text(action, "expectedSourceCommit", codec)
            .as_deref()
            .is_some_and(valid_source_commit)
        || uint64_field(action, "fencingToken").is_none()
        || !valid_action_hash(action_hash)
        || ![
            "controlStateSha256",
            "graphManifestSha256",
            "ordinalArtifactSha256",
            "sourceMemberRootSha256",
            "sourceReceiptRootSha256",
        ]
        .iter()
        .all(|key| hash_field(action, key))
    {
        return Err(Slot);
    }
    let current = read_current(FamilyActionAnchorKind::RainControl, files, codec, time)?;
    require_value(&current, "actionSha256", Some(action_hash))?;
    require_fields(
        &current,
        action,
        &[
            ("actionKind", "actionKind"),
            ("controlStateSha256", "controlStateSha256"),
            ("dueMonth", "dueMonth"),
            ("fencingToken", "fencingToken"),
            ("graphManifestSha256", "graphManifestSha256"),
            ("ordinalArtifactSha256", "ordinalArtifactSha256"),
            ("sourceCommit", "expectedSourceCommit"),
            ("sourceMemberRootSha256", "sourceMemberRootSha256"),
            ("sourceReceiptRootSha256", "sourceReceiptRootSha256"),
        ],
    )?;
    Ok(current)
}

// bind one exact inactive shadow action using the two original development anchor reads
fn authorize_development(
    action: &JsonValue,
    action_hash: &JsonValue,
    first: &JsonValue,
    files: &mut impl FamilyActionAuthorityFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    // retain distinct development authority and its action-carried content identities
    if !matches!(
        text(action, "family").as_deref(),
        Some("rain" | "temperature" | "wind")
    ) || !regex_text(action, "expectedSourceCommit", codec)
        .as_deref()
        .is_some_and(valid_source_commit)
        || !valid_action_hash(action_hash)
        || ![
            "candidateGraphSha256",
            "candidateSha256",
            "lifecycleHeadSha256",
            "policyReportSha256",
        ]
        .iter()
        .all(|key| hash_field(action, key))
        || !["artifactSha256", "registrationSha256"]
            .iter()
            .all(|key| hash_field(first, key))
    {
        return Err(Slot);
    }
    let current = read_current(FamilyActionAnchorKind::Development, files, codec, time)?;
    require_value(&current, "actionSha256", Some(action_hash))?;
    require_fields(
        &current,
        first,
        &[
            ("artifactSha256", "artifactSha256"),
            ("registrationSha256", "registrationSha256"),
        ],
    )?;
    require_fields(
        &current,
        action,
        &[
            ("actionKind", "actionKind"),
            ("candidateGraphSha256", "candidateGraphSha256"),
            ("candidateSha256", "candidateSha256"),
            ("family", "family"),
            ("lifecycleLedgerRootSha256", "lifecycleHeadSha256"),
            ("policyReportSha256", "policyReportSha256"),
            ("sourceCommit", "expectedSourceCommit"),
        ],
    )?;
    Ok(current)
}

// retain the original finalized future-only maintenance authority and no legacy aliases
fn authorize_future(
    action: &JsonValue,
    action_hash: &JsonValue,
    files: &mut impl FamilyActionAuthorityFiles,
    codec: &mut impl MaintenanceAnchorStatusJson,
    time: &dyn CurrentTimeResolver,
) -> Result<JsonValue, MaintenanceAnchorStatusError> {
    // require the original closed hash and strict source-commit operands before accessing current
    if !valid_action_hash(action_hash)
        || ![
            "fullMemberRootSha256",
            "lifecycleHeadSha256",
            "policyReportSha256",
        ]
        .iter()
        .all(|key| hash_field(action, key))
        || !text(action, "expectedSourceCommit")
            .as_deref()
            .is_some_and(valid_source_commit)
    {
        return Err(Slot);
    }
    let current = read_current(FamilyActionAnchorKind::Maintenance, files, codec, time)?;
    // only the exact finalized v3 root grants this future-only authority
    if text(&current, "contractVersion").as_deref() != Some(FUTURE_ANCHOR_VERSION)
        || text(&current, "ctfState").as_deref() != Some("finalized")
    {
        return Err(Slot);
    }
    require_value(&current, "actionSha256", Some(action_hash))?;
    require_fields(
        &current,
        action,
        &[
            ("fullMemberRootSha256", "fullMemberRootSha256"),
            ("lifecycleLedgerRootSha256", "lifecycleHeadSha256"),
            ("policyReportSha256", "policyReportSha256"),
            ("sourceCommit", "expectedSourceCommit"),
        ],
    )?;
    Ok(current)
}
