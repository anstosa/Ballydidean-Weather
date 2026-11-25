//! Project the retained terminal adjustment-family ancestry from injected authority material.

use std::collections::HashSet;

use weather_api::CurrentTimeResolver;
use weather_contracts::{AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum, JsonValue};

use crate::MaintenanceCanonicalJson;
use crate::capture_epoch_witness::{resolve_operator_capture_clock, verify_capture_epoch_witness};
use crate::family_release_identity::compute_family_release_image_identity;
use crate::family_target_catalog::{object, string};
use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};

const FAMILY_TRANSACTION_VERSION: &str = "adjustment-family-release-transaction/v1";
const FAMILY_OPERATOR_OFF_TRANSACTION_VERSION: &str =
    "adjustment-family-operator-off-transaction/v1";
const FAMILY_CURRENT_VERSION: &str = "adjustment-family-release-current/v1";
const FAMILY_BOOTSTRAP_CURRENT_VERSION: &str = "adjustment-family-release-bootstrap-current/v1";
const FAMILY_LINEAGE_VERSION: &str = "adjustment-family-release-lineage/v2";
const MODEL_ACTION_VERSION: &str = "forecast-adjustment-model-action/v1";
const MAXIMUM_TRANSACTIONS: usize = 384;

const BOOTSTRAP_CURRENT_KEYS: &[&str] = &[
    "bootstrappedAt",
    "catalogSha256",
    "commit",
    "contractVersion",
    "imageDigestsSha256",
    "release",
    "serverImage",
    "settingsSha256",
    "state",
    "webImage",
];
const CURRENT_KEYS: &[&str] = &[
    "acknowledgedAt",
    "actionSha256",
    "commit",
    "contractVersion",
    "family",
    "imageDigestsSha256",
    "release",
    "state",
];
const TRANSACTION_INPUT_KEYS: &[&str] = &[
    "actionSha256",
    "compensatingRelease",
    "expectedCurrentRelease",
    "expectedSettingsSha256",
    "expectedSourceRelease",
    "family",
    "fencingToken",
    "reportSha256",
    "targetRelease",
];
const COMPENSATION_KEYS: &[&str] = &["actionSha256", "release", "state", "verifiedAt"];
const PROOF_KEYS: &[&str] = &[
    "baselineCatalogBase64",
    "compensationActionSha256",
    "compensationCommit",
    "compensationFamilyIdentitySha256",
    "compensationImageDigestsSha256",
    "sourceSettingsBase64",
    "targetCatalogBase64",
    "targetCommit",
    "targetFamilyIdentitySha256",
    "targetImageDigestsSha256",
];
const RECORD_KEYS: &[&str] = &[
    "action",
    "createdAt",
    "failedAt",
    "input",
    "outcome",
    "sourceCommit",
    "sourceSettingsSha256",
    "state",
    "targetCommit",
    "verifiedAt",
];
const ACTION_KEYS: &[&str] = &[
    "actionKind",
    "candidateGraphSha256",
    "candidateSha256",
    "contractVersion",
    "createdAt",
    "expectedSettingsSha256",
    "expectedSourceCommit",
    "expectedInstalledReceiptSha256",
    "expectedSourceRelease",
    "family",
    "fencingToken",
    "fullMemberRootSha256",
    "lifecycleHeadSha256",
    "policyDecision",
    "policyReportSha256",
    "predecessorActionSha256",
    "reason",
    "reportCreatedAt",
    "siteKey",
    "validThrough",
];
const OPERATOR_OFF_RECORD_KEYS: &[&str] = &[
    "completedAt",
    "input",
    "sourceCommit",
    "sourceSettingsSha256",
    "state",
];

/// One already inventoried transaction and its filename-derived action identity.
#[derive(Debug, Clone, PartialEq)]
pub struct NativeFamilyReleaseLineageTransaction {
    pub action_sha256: String,
    pub document: JsonValue,
}

/// Exact terminal completion clock and no-mutation class from one validated transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFamilyReleaseValidatedTransaction {
    pub completed_at: String,
    pub operator_off_unapplied: bool,
}

/// One frozen lineage observation supplied by an authority-owning reader.
pub struct NativeFamilyReleaseLineageInput<'a> {
    pub control_sha256: &'a str,
    pub control_version: &'a str,
    pub current: &'a JsonValue,
    pub epoch_witness_bytes: &'a [u8],
    pub transactions: &'a [NativeFamilyReleaseLineageTransaction],
    pub verified_at: &'a str,
}

/// The exact terminal ancestry projection consumed by restart admission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeFamilyReleaseLineageProjection {
    pub contract_version: &'static str,
    pub control_sha256: String,
    pub control_version: String,
    pub current_action_sha256: Option<String>,
    pub current_commit: String,
    pub current_release: String,
    pub epoch_ancestor_commit: String,
    pub epoch_witness_sha256: String,
    pub state: &'static str,
    pub verified_at: String,
}

/// One contained projection failure without leaking private host material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeFamilyReleaseLineageError {
    Witness,
    Control,
    Current,
    Transaction,
    Action,
    Ancestry,
    Codec,
    SourceCommit,
}

#[derive(Clone)]
struct CurrentAuthority {
    action_sha256: Option<String>,
    commit: String,
    release: String,
    bootstrap_images: Option<(String, String)>,
}

#[derive(Clone)]
struct LineageEdge {
    action_sha256: JsonValue,
    child_commit: String,
    child_release: String,
    parent_commit: String,
    parent_release: String,
}

#[derive(Clone, Copy)]
struct ActionRule {
    policy: Option<&'static str>,
    reason: &'static str,
    candidate: bool,
    predecessor: bool,
}

/// Project one bounded transitive epoch ancestry proof from a frozen injected observation.
pub fn project_native_family_release_lineage(
    input: &NativeFamilyReleaseLineageInput<'_>,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<NativeFamilyReleaseLineageProjection, NativeFamilyReleaseLineageError> {
    let witness = verify_capture_epoch_witness(input.epoch_witness_bytes, codec, time)
        .map_err(|_| NativeFamilyReleaseLineageError::Witness)?;
    let current = validate_current(input.current, codec)?;

    // bind the injected observation to the admitted v14 capture witness and bounded journal
    if input.control_version != "14"
        || witness.witness.controlPlaneVersion
            != AdjustmentRevisionCaptureEpochWitnessControlPlaneVersionEnum::Value14
        || input.control_sha256 != witness.witness.controlPlaneSha256
        || !valid_hash(input.control_sha256)
        || !utc_millisecond_shape(input.verified_at)
        || input.transactions.len() > MAXIMUM_TRANSACTIONS
    {
        return Err(NativeFamilyReleaseLineageError::Control);
    }
    let edges = lineage_edges(input.transactions, codec, time)?;

    // distinguish the actionless bootstrap authority from a terminal action descendant
    if let Some((server_digest, web_digest)) = &current.bootstrap_images {
        if current.commit != witness.witness.sourceCommit
            || current.release != witness.witness.sourceRelease
            || server_digest != &witness.witness.sourceServerImageDigest
            || web_digest != &witness.witness.sourceWebImageDigest
        {
            return Err(NativeFamilyReleaseLineageError::Ancestry);
        }
    } else {
        walk_to_epoch(
            &current,
            &edges,
            &witness.witness.sourceCommit,
            &witness.witness.sourceRelease,
            true,
        )?;
    }

    Ok(NativeFamilyReleaseLineageProjection {
        contract_version: FAMILY_LINEAGE_VERSION,
        control_sha256: input.control_sha256.to_owned(),
        control_version: input.control_version.to_owned(),
        current_action_sha256: current.action_sha256,
        current_commit: current.commit,
        current_release: current.release,
        epoch_ancestor_commit: witness.witness.sourceCommit,
        epoch_witness_sha256: witness.witness.witnessSha256,
        state: "verified_epoch_descendant",
        verified_at: input.verified_at.to_owned(),
    })
}

/// Verify that one immutable commit occurs on the exact retained terminal ancestry.
pub fn verify_native_family_release_ancestor_projection(
    input: &NativeFamilyReleaseLineageInput<'_>,
    source_commit: &str,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<bool, NativeFamilyReleaseLineageError> {
    // close the operand before validating or traversing caller-provided journal material
    if !valid_commit(source_commit) {
        return Err(NativeFamilyReleaseLineageError::SourceCommit);
    }
    let lineage = project_native_family_release_lineage(input, codec, time)?;
    let edges = lineage_edges(input.transactions, codec, time)?;
    let mut commit = lineage.current_commit;
    let mut release = lineage.current_release;
    let mut matched = commit == source_commit;
    let mut visited = HashSet::new();
    let mut depth = 0;

    // walk only the unique terminal path already admitted by the full projection
    while commit != lineage.epoch_ancestor_commit {
        if !visited.insert(commit.clone()) || depth >= MAXIMUM_TRANSACTIONS {
            return Err(NativeFamilyReleaseLineageError::Ancestry);
        }
        let matches = matching_edges(&edges, &commit, &release);
        // preserve the same unique predecessor requirement as the lineage proof
        if matches.len() != 1 {
            return Err(NativeFamilyReleaseLineageError::Ancestry);
        }
        commit.clone_from(&matches[0].parent_commit);
        release.clone_from(&matches[0].parent_release);
        matched |= commit == source_commit;
        depth += 1;
    }
    Ok(matched)
}

/// Validate one bootstrap or terminal family current authority.
fn validate_current(
    value: &JsonValue,
    codec: &mut impl MaintenanceCanonicalJson,
) -> Result<CurrentAuthority, NativeFamilyReleaseLineageError> {
    let version = text(value, "contractVersion").ok_or(NativeFamilyReleaseLineageError::Current)?;
    // retain the actionless v14 bridge as a distinct authority shape
    if version == FAMILY_BOOTSTRAP_CURRENT_VERSION {
        if !exact_keys(value, BOOTSTRAP_CURRENT_KEYS)
            || text(value, "state").as_deref() != Some("inert_v14_bootstrap")
        {
            return Err(NativeFamilyReleaseLineageError::Current);
        }
        let commit = required_text(value, "commit", NativeFamilyReleaseLineageError::Current)?;
        let release = required_text(value, "release", NativeFamilyReleaseLineageError::Current)?;
        let bootstrapped_at = required_text(
            value,
            "bootstrappedAt",
            NativeFamilyReleaseLineageError::Current,
        )?;
        let catalog_sha256 = required_text(
            value,
            "catalogSha256",
            NativeFamilyReleaseLineageError::Current,
        )?;
        let image_sha256 = required_text(
            value,
            "imageDigestsSha256",
            NativeFamilyReleaseLineageError::Current,
        )?;
        let settings_sha256 = required_text(
            value,
            "settingsSha256",
            NativeFamilyReleaseLineageError::Current,
        )?;
        let server_image = required_text(
            value,
            "serverImage",
            NativeFamilyReleaseLineageError::Current,
        )?;
        let web_image = required_text(value, "webImage", NativeFamilyReleaseLineageError::Current)?;
        let server_digest = image_digest(&server_image, "server")
            .ok_or(NativeFamilyReleaseLineageError::Current)?;
        let web_digest =
            image_digest(&web_image, "web").ok_or(NativeFamilyReleaseLineageError::Current)?;
        // rederive the aggregate rather than trusting the detached bridge identity
        if !valid_commit(&commit)
            || !valid_release(&release, Some(3))
            || !utc_millisecond_shape(&bootstrapped_at)
            || ![&catalog_sha256, &image_sha256, &settings_sha256]
                .into_iter()
                .all(|value| valid_hash(value))
        {
            return Err(NativeFamilyReleaseLineageError::Current);
        }
        let environment = object(vec![
            ("WEATHER_SERVER_IMAGE", string(&server_image)),
            ("WEATHER_WEB_IMAGE", string(&web_image)),
        ]);
        let computed = compute_family_release_image_identity(&environment, codec)
            .map_err(|_| NativeFamilyReleaseLineageError::Codec)?;
        if computed.as_str() != image_sha256 {
            return Err(NativeFamilyReleaseLineageError::Current);
        }
        return Ok(CurrentAuthority {
            action_sha256: None,
            commit,
            release,
            bootstrap_images: Some((server_digest, web_digest)),
        });
    }

    // accept only a complete acknowledgement or verified compensation authority
    if !exact_keys(value, CURRENT_KEYS)
        || version != FAMILY_CURRENT_VERSION
        || !matches!(
            text(value, "state").as_deref(),
            Some("acknowledged" | "compensated")
        )
        || !valid_family(text(value, "family").as_deref())
    {
        return Err(NativeFamilyReleaseLineageError::Current);
    }
    let action_sha256 = required_text(
        value,
        "actionSha256",
        NativeFamilyReleaseLineageError::Current,
    )?;
    let commit = required_text(value, "commit", NativeFamilyReleaseLineageError::Current)?;
    let release = required_text(value, "release", NativeFamilyReleaseLineageError::Current)?;
    let acknowledged_at = required_text(
        value,
        "acknowledgedAt",
        NativeFamilyReleaseLineageError::Current,
    )?;
    let image_sha256 = required_text(
        value,
        "imageDigestsSha256",
        NativeFamilyReleaseLineageError::Current,
    )?;
    if !valid_hash(&action_sha256)
        || !valid_hash(&image_sha256)
        || !valid_commit(&commit)
        || !valid_release(&release, Some(3))
        || !utc_millisecond_shape(&acknowledged_at)
    {
        return Err(NativeFamilyReleaseLineageError::Current);
    }
    Ok(CurrentAuthority {
        action_sha256: Some(action_sha256),
        commit,
        release,
        bootstrap_images: None,
    })
}

/// Validate every inventoried transaction before retaining only terminal ancestry edges.
fn lineage_edges(
    transactions: &[NativeFamilyReleaseLineageTransaction],
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Vec<LineageEdge>, NativeFamilyReleaseLineageError> {
    let mut edges = Vec::new();
    // preserve transaction order while ignoring only validated nonterminal branches
    for transaction in transactions {
        if let Some(edge) = lineage_edge(transaction, codec, time)? {
            edges.push(edge);
        }
    }
    Ok(edges)
}

/// Derive one authorizing root-journal edge or ignore a closed unfinished branch.
fn lineage_edge(
    transaction: &NativeFamilyReleaseLineageTransaction,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<Option<LineageEdge>, NativeFamilyReleaseLineageError> {
    if !valid_hash(&transaction.action_sha256) {
        return Err(NativeFamilyReleaseLineageError::Transaction);
    }
    let document = &transaction.document;
    let version =
        text(document, "contractVersion").ok_or(NativeFamilyReleaseLineageError::Transaction)?;
    // validate but never authorize ancestry from a no-mutation operator-off branch
    if version == FAMILY_OPERATOR_OFF_TRANSACTION_VERSION {
        if !exact_keys(document, &["contractVersion", "record"])
            || !validate_operator_off(
                field(document, "record").ok_or(NativeFamilyReleaseLineageError::Transaction)?,
                &transaction.action_sha256,
                time,
            )
        {
            return Err(NativeFamilyReleaseLineageError::Transaction);
        }
        return Ok(None);
    }
    if !exact_keys(
        document,
        &["compensation", "contractVersion", "proof", "record"],
    ) || version != FAMILY_TRANSACTION_VERSION
    {
        return Err(NativeFamilyReleaseLineageError::Transaction);
    }
    let record = field(document, "record").ok_or(NativeFamilyReleaseLineageError::Transaction)?;
    let input = field(record, "input").ok_or(NativeFamilyReleaseLineageError::Transaction)?;
    let compensation =
        field(document, "compensation").ok_or(NativeFamilyReleaseLineageError::Transaction)?;
    let proof = field(document, "proof").ok_or(NativeFamilyReleaseLineageError::Transaction)?;
    if !exact_keys(input, TRANSACTION_INPUT_KEYS)
        || !exact_keys(compensation, COMPENSATION_KEYS)
        || !exact_keys(proof, PROOF_KEYS)
        || !allowed_keys(record, RECORD_KEYS)
    {
        return Err(NativeFamilyReleaseLineageError::Transaction);
    }
    let action = field(record, "action").ok_or(NativeFamilyReleaseLineageError::Transaction)?;
    let input_action = required_text(
        input,
        "actionSha256",
        NativeFamilyReleaseLineageError::Transaction,
    )?;
    let family = required_text(
        input,
        "family",
        NativeFamilyReleaseLineageError::Transaction,
    )?;
    let source_commit = required_text(
        record,
        "sourceCommit",
        NativeFamilyReleaseLineageError::Transaction,
    )?;
    let target_commit = required_text(
        record,
        "targetCommit",
        NativeFamilyReleaseLineageError::Transaction,
    )?;
    let compensation_commit = required_text(
        proof,
        "compensationCommit",
        NativeFamilyReleaseLineageError::Transaction,
    )?;
    let source_release = required_text(
        input,
        "expectedSourceRelease",
        NativeFamilyReleaseLineageError::Transaction,
    )?;
    let current_release = required_text(
        input,
        "expectedCurrentRelease",
        NativeFamilyReleaseLineageError::Transaction,
    )?;
    let target_release = required_text(
        input,
        "targetRelease",
        NativeFamilyReleaseLineageError::Transaction,
    )?;
    let compensation_release = required_text(
        input,
        "compensatingRelease",
        NativeFamilyReleaseLineageError::Transaction,
    )?;
    let report_sha256 = required_text(
        input,
        "reportSha256",
        NativeFamilyReleaseLineageError::Transaction,
    )?;
    // reject alternate record fields and cross-document binding aliases
    if input_action != transaction.action_sha256
        || text(action, "expectedSourceCommit").as_deref() != Some(source_commit.as_str())
        || text(action, "expectedSourceRelease").as_deref() != Some(source_release.as_str())
        || text(action, "family").as_deref() != Some(family.as_str())
        || current_release != source_release
        || text(record, "sourceSettingsSha256") != text(input, "expectedSettingsSha256")
        || text(proof, "targetCommit").as_deref() != Some(target_commit.as_str())
        || field(proof, "compensationActionSha256") != field(compensation, "actionSha256")
        || text(compensation, "release").as_deref() != Some(compensation_release.as_str())
    {
        return Err(NativeFamilyReleaseLineageError::Transaction);
    }
    validate_action(
        action,
        &transaction.action_sha256,
        &family,
        &report_sha256,
        &source_commit,
        &source_release,
        codec,
        time,
    )?;
    if ![&source_commit, &target_commit, &compensation_commit]
        .into_iter()
        .all(|value| valid_commit(value))
        || ![&source_release, &target_release, &compensation_release]
            .into_iter()
            .all(|value| valid_release(value, Some(3)))
    {
        return Err(NativeFamilyReleaseLineageError::Transaction);
    }
    let state = text(record, "state").ok_or(NativeFamilyReleaseLineageError::Transaction)?;
    let compensation_state =
        text(compensation, "state").ok_or(NativeFamilyReleaseLineageError::Transaction)?;
    // ignore a closed branch that never established a new current authority
    if matches!(state.as_str(), "prepared" | "applying") {
        if compensation_state != "absent"
            || !matches!(field(compensation, "verifiedAt"), Some(JsonValue::Null))
            || field(record, "failedAt").is_some()
            || field(record, "outcome").is_some()
            || field(record, "verifiedAt").is_some()
        {
            return Err(NativeFamilyReleaseLineageError::Transaction);
        }
        return Ok(None);
    }
    // select only a completed target acknowledgement
    if state == "acknowledged" {
        if compensation_state != "absent"
            || !text(record, "verifiedAt")
                .as_deref()
                .is_some_and(utc_millisecond_shape)
        {
            return Err(NativeFamilyReleaseLineageError::Transaction);
        }
        return Ok(Some(LineageEdge {
            action_sha256: string(&transaction.action_sha256),
            child_commit: target_commit,
            child_release: target_release,
            parent_commit: source_commit,
            parent_release: source_release,
        }));
    }
    // ignore a failed deployment until its compensation establishes authority
    if state == "compensation_required"
        && matches!(compensation_state.as_str(), "absent" | "failed")
    {
        if !text(record, "failedAt")
            .as_deref()
            .is_some_and(utc_millisecond_shape)
            || !matches!(field(compensation, "verifiedAt"), Some(JsonValue::Null))
            || field(record, "outcome").is_some()
            || field(record, "verifiedAt").is_some()
        {
            return Err(NativeFamilyReleaseLineageError::Transaction);
        }
        return Ok(None);
    }
    // select only a verified family-only compensation acknowledgement
    if !matches!(state.as_str(), "applying" | "compensation_required")
        || compensation_state != "verified"
        || !text(compensation, "verifiedAt")
            .as_deref()
            .is_some_and(utc_millisecond_shape)
    {
        return Err(NativeFamilyReleaseLineageError::Transaction);
    }
    Ok(Some(LineageEdge {
        action_sha256: field(compensation, "actionSha256")
            .cloned()
            .ok_or(NativeFamilyReleaseLineageError::Transaction)?,
        child_commit: compensation_commit,
        child_release: compensation_release,
        parent_commit: source_commit,
        parent_release: source_release,
    }))
}

/// Validate one retained transaction with the complete lineage grammar and expose its terminal clock.
pub fn validate_native_family_release_transaction(
    transaction: &NativeFamilyReleaseLineageTransaction,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<NativeFamilyReleaseValidatedTransaction, NativeFamilyReleaseLineageError> {
    let edge = lineage_edge(transaction, codec, time)?;
    let document = &transaction.document;
    let version =
        text(document, "contractVersion").ok_or(NativeFamilyReleaseLineageError::Transaction)?;
    let record = field(document, "record").ok_or(NativeFamilyReleaseLineageError::Transaction)?;
    // the operator-off grammar is terminal despite deliberately producing no lineage edge
    if version == FAMILY_OPERATOR_OFF_TRANSACTION_VERSION {
        return Ok(NativeFamilyReleaseValidatedTransaction {
            completed_at: text(record, "completedAt")
                .ok_or(NativeFamilyReleaseLineageError::Transaction)?,
            operator_off_unapplied: true,
        });
    }
    // require a completed target or verified compensation rather than an unfinished valid branch
    edge.ok_or(NativeFamilyReleaseLineageError::Transaction)?;
    let completed_at = if text(record, "state").as_deref() == Some("acknowledged") {
        text(record, "verifiedAt")
    } else {
        field(document, "compensation").and_then(|value| text(value, "verifiedAt"))
    }
    .ok_or(NativeFamilyReleaseLineageError::Transaction)?;
    Ok(NativeFamilyReleaseValidatedTransaction {
        completed_at,
        operator_off_unapplied: false,
    })
}

/// Validate the exact model action embedded in a retained transaction.
#[allow(clippy::too_many_arguments)]
fn validate_action(
    action: &JsonValue,
    expected_sha256: &str,
    expected_family: &str,
    expected_report_sha256: &str,
    expected_source_commit: &str,
    expected_source_release: &str,
    codec: &mut impl MaintenanceCanonicalJson,
    time: &dyn CurrentTimeResolver,
) -> Result<(), NativeFamilyReleaseLineageError> {
    let bytes = codec
        .encode_canonical_json(action)
        .ok_or(NativeFamilyReleaseLineageError::Codec)?;
    if bytes.is_empty() || bytes.len() > 16_384 || !exact_keys(action, ACTION_KEYS) {
        return Err(NativeFamilyReleaseLineageError::Action);
    }
    let action_kind = text(action, "actionKind").ok_or(NativeFamilyReleaseLineageError::Action)?;
    let rule = action_rule(&action_kind).ok_or(NativeFamilyReleaseLineageError::Action)?;
    let digest = codec
        .sha256(&bytes)
        .ok_or(NativeFamilyReleaseLineageError::Codec)?;
    // bind canonical action bytes to the command, source and policy report identities
    if digest.as_str() != expected_sha256
        || text(action, "contractVersion").as_deref() != Some(MODEL_ACTION_VERSION)
        || text(action, "siteKey").as_deref() != Some("ballydidean")
        || text(action, "family").as_deref() != Some(expected_family)
        || !valid_family(Some(expected_family))
        || text(action, "expectedSourceRelease").as_deref() != Some(expected_source_release)
        || text(action, "expectedSourceCommit").as_deref() != Some(expected_source_commit)
        || text(action, "policyReportSha256").as_deref() != Some(expected_report_sha256)
        || text(action, "reason").as_deref() != Some(rule.reason)
    {
        return Err(NativeFamilyReleaseLineageError::Action);
    }
    let policy = text(action, "policyDecision").ok_or(NativeFamilyReleaseLineageError::Action)?;
    // compensation actions admit either terminal policy outcome while ordinary actions are fixed
    if rule.policy.map_or_else(
        || !matches!(policy.as_str(), "qualified" | "regressed"),
        |expected| policy != expected,
    ) {
        return Err(NativeFamilyReleaseLineageError::Action);
    }
    if ![
        "expectedSettingsSha256",
        "lifecycleHeadSha256",
        "policyReportSha256",
    ]
    .into_iter()
    .all(|key| text(action, key).as_deref().is_some_and(valid_hash))
        || ![
            "candidateGraphSha256",
            "candidateSha256",
            "fullMemberRootSha256",
            "expectedInstalledReceiptSha256",
            "predecessorActionSha256",
        ]
        .into_iter()
        .all(|key| optional_hash(action, key))
    {
        return Err(NativeFamilyReleaseLineageError::Action);
    }
    let candidate_sha = field(action, "candidateSha256");
    let candidate_graph = field(action, "candidateGraphSha256");
    let full_member = field(action, "fullMemberRootSha256");
    let predecessor = field(action, "predecessorActionSha256");
    let fencing_token =
        text(action, "fencingToken").ok_or(NativeFamilyReleaseLineageError::Action)?;
    // preserve pair presence, member-root and predecessor bindings selected by the action kind
    if !valid_commit(expected_source_commit)
        || !valid_release(expected_source_release, None)
        || !valid_u64(&fencing_token, true)
        || rule.candidate != (is_non_null(candidate_sha) && is_non_null(candidate_graph))
        || is_null(candidate_sha) != is_null(candidate_graph)
        || matches!(action_kind.as_str(), "shadow" | "compensate_shadow") != is_null(full_member)
        || rule.predecessor != is_non_null(predecessor)
    {
        return Err(NativeFamilyReleaseLineageError::Action);
    }
    let created = required_text(action, "createdAt", NativeFamilyReleaseLineageError::Action)?;
    let report = required_text(
        action,
        "reportCreatedAt",
        NativeFamilyReleaseLineageError::Action,
    )?;
    let through = required_text(
        action,
        "validThrough",
        NativeFamilyReleaseLineageError::Action,
    )?;
    let created_at =
        canonical_instant(&created, time).ok_or(NativeFamilyReleaseLineageError::Action)?;
    let report_at =
        canonical_instant(&report, time).ok_or(NativeFamilyReleaseLineageError::Action)?;
    let through_at =
        canonical_instant(&through, time).ok_or(NativeFamilyReleaseLineageError::Action)?;
    // the transaction validates with now equal to the action creation clock
    if through_at - report_at != 7 * 86_400_000
        || created_at < report_at
        || (!rule.predecessor && created_at > through_at)
    {
        return Err(NativeFamilyReleaseLineageError::Action);
    }
    Ok(())
}

/// Validate one no-mutation operator-off transaction record.
fn validate_operator_off(
    record: &JsonValue,
    expected_action_sha256: &str,
    time: &dyn CurrentTimeResolver,
) -> bool {
    if !exact_keys(record, OPERATOR_OFF_RECORD_KEYS) {
        return false;
    }
    let Some(input) = field(record, "input") else {
        return false;
    };
    if !exact_keys(input, TRANSACTION_INPUT_KEYS) {
        return false;
    }
    let Some(family) = text(input, "family") else {
        return false;
    };
    let Some(source_commit) = text(record, "sourceCommit") else {
        return false;
    };
    let Some(fencing_token) = text(input, "fencingToken") else {
        return false;
    };
    let releases = [
        text(input, "compensatingRelease"),
        text(input, "expectedCurrentRelease"),
        text(input, "expectedSourceRelease"),
        text(input, "targetRelease"),
    ];
    // bind one exact source observation and command identity without serving authority
    if text(record, "state").as_deref() != Some("operator_off_unapplied")
        || text(input, "actionSha256").as_deref() != Some(expected_action_sha256)
        || !valid_family(Some(&family))
        || !valid_commit(&source_commit)
        || !valid_u64(&fencing_token, false)
        || !releases.iter().all(|value| {
            value
                .as_deref()
                .is_some_and(|value| valid_release(value, None))
        })
        || releases[1] != releases[2]
        || releases[3] == releases[1]
        || releases[3] == releases[0]
        || text(record, "sourceSettingsSha256") != text(input, "expectedSettingsSha256")
    {
        return false;
    }
    let hashes = [
        text(input, "actionSha256"),
        text(input, "expectedSettingsSha256"),
        text(input, "reportSha256"),
        text(record, "sourceSettingsSha256"),
    ];
    hashes
        .iter()
        .all(|value| value.as_deref().is_some_and(valid_hash))
        && text(record, "completedAt")
            .as_deref()
            .filter(|value| utc_millisecond_shape(value))
            .and_then(|value| canonical_instant(value, time))
            .is_some()
}

/// Walk one current authority through unique commit-and-release terminal edges.
fn walk_to_epoch(
    current: &CurrentAuthority,
    edges: &[LineageEdge],
    epoch_commit: &str,
    epoch_release: &str,
    bind_first_action: bool,
) -> Result<(), NativeFamilyReleaseLineageError> {
    let mut commit = current.commit.clone();
    let mut release = current.release.clone();
    let mut visited = HashSet::new();
    let mut depth = 0;
    // walk every acknowledged edge back to the immutable epoch source
    while commit != epoch_commit {
        if !visited.insert(commit.clone()) || depth >= MAXIMUM_TRANSACTIONS {
            return Err(NativeFamilyReleaseLineageError::Ancestry);
        }
        let matches = matching_edges(edges, &commit, &release);
        // require one predecessor and the current root action on the first edge
        if matches.len() != 1
            || (bind_first_action
                && depth == 0
                && current
                    .action_sha256
                    .as_ref()
                    .is_none_or(|action| matches[0].action_sha256 != string(action)))
        {
            return Err(NativeFamilyReleaseLineageError::Ancestry);
        }
        commit.clone_from(&matches[0].parent_commit);
        release.clone_from(&matches[0].parent_release);
        depth += 1;
    }
    if depth == 0 || release != epoch_release {
        return Err(NativeFamilyReleaseLineageError::Ancestry);
    }
    Ok(())
}

/// Select edges matching both child commit and child release.
fn matching_edges<'a>(
    edges: &'a [LineageEdge],
    commit: &str,
    release: &str,
) -> Vec<&'a LineageEdge> {
    edges
        .iter()
        .filter(|edge| edge.child_commit == commit && edge.child_release == release)
        .collect()
}

/// Return the action rule selected by one admitted action kind.
fn action_rule(value: &str) -> Option<ActionRule> {
    Some(match value {
        "shadow" => ActionRule {
            policy: Some("pending"),
            reason: "development_candidate",
            candidate: true,
            predecessor: false,
        },
        "promote" => ActionRule {
            policy: Some("qualified"),
            reason: "qualified_candidate",
            candidate: true,
            predecessor: false,
        },
        "rollback_prior" => ActionRule {
            policy: Some("regressed"),
            reason: "qualified_prior",
            candidate: true,
            predecessor: false,
        },
        "raw" => ActionRule {
            policy: Some("regressed"),
            reason: "policy_raw",
            candidate: false,
            predecessor: false,
        },
        "compensate_incumbent" => ActionRule {
            policy: None,
            reason: "failed_promotion",
            candidate: true,
            predecessor: true,
        },
        "compensate_raw" => ActionRule {
            policy: None,
            reason: "invalid_incumbent",
            candidate: false,
            predecessor: true,
        },
        "compensate_shadow" => ActionRule {
            policy: Some("pending"),
            reason: "development_shadow_failure",
            candidate: true,
            predecessor: true,
        },
        _ => return None,
    })
}

/// Decode one required string using the owning failure category.
fn required_text(
    value: &JsonValue,
    key: &str,
    error: NativeFamilyReleaseLineageError,
) -> Result<String, NativeFamilyReleaseLineageError> {
    text(value, key).ok_or(error)
}

/// Require an object to contain no fields outside its source whitelist.
fn allowed_keys(value: &JsonValue, keys: &[&str]) -> bool {
    let JsonValue::Object(fields) = value else {
        return false;
    };
    fields.iter().all(|(name, _)| {
        String::from_utf16(name)
            .ok()
            .is_some_and(|name| keys.contains(&name.as_str()))
    })
}

/// Recognize one exact adjustment family.
fn valid_family(value: Option<&str>) -> bool {
    matches!(value, Some("temperature" | "wind" | "rain"))
}

/// Recognize one lowercase hexadecimal Git commit.
fn valid_commit(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(lower_hex)
}

/// Recognize one immutable release and optionally bound its revision width.
fn valid_release(value: &str, maximum_revision_digits: Option<usize>) -> bool {
    let bytes = value.as_bytes();
    let revision = bytes.get(11..).unwrap_or_default();
    bytes.len() >= 12
        && bytes.get(4) == Some(&b'.')
        && bytes.get(7) == Some(&b'.')
        && bytes.get(10) == Some(&b'-')
        && bytes
            .get(..4)
            .is_some_and(|value| value.iter().all(u8::is_ascii_digit))
        && bytes
            .get(5..7)
            .is_some_and(|value| value.iter().all(u8::is_ascii_digit))
        && bytes
            .get(8..10)
            .is_some_and(|value| value.iter().all(u8::is_ascii_digit))
        && matches!(revision.first(), Some(b'1'..=b'9'))
        && revision.iter().all(u8::is_ascii_digit)
        && maximum_revision_digits.is_none_or(|maximum| revision.len() <= maximum)
}

/// Recognize one canonical unsigned 64-bit decimal token.
fn valid_u64(value: &str, allow_zero: bool) -> bool {
    let spelling = value == "0" && allow_zero
        || value.len() <= 20
            && matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
            && value.bytes().all(|value| value.is_ascii_digit());
    spelling && value.parse::<u64>().is_ok()
}

/// Recognize one optional lowercase SHA-256 field.
fn optional_hash(value: &JsonValue, key: &str) -> bool {
    match field(value, key) {
        Some(JsonValue::Null) => true,
        Some(JsonValue::String(units)) => String::from_utf16(units)
            .ok()
            .as_deref()
            .is_some_and(valid_hash),
        _ => false,
    }
}

/// Report whether one present generated value is canonical null.
fn is_null(value: Option<&JsonValue>) -> bool {
    matches!(value, Some(JsonValue::Null))
}

/// Report whether one present generated value is not null.
fn is_non_null(value: Option<&JsonValue>) -> bool {
    value.is_some_and(|value| !matches!(value, JsonValue::Null))
}

/// Resolve one strict canonical JavaScript timestamp to epoch milliseconds.
fn canonical_instant(value: &str, time: &dyn CurrentTimeResolver) -> Option<i64> {
    resolve_operator_capture_clock(value, time).map(|instant| instant.epoch_milliseconds)
}

/// Recognize the source's fixed four-digit UTC millisecond spelling.
fn utc_millisecond_shape(value: &str) -> bool {
    let shape = b"dddd-dd-ddTdd:dd:dd.dddZ";
    value.len() == shape.len()
        && shape
            .iter()
            .zip(value.bytes())
            .all(|(&expected, observed)| {
                // require decimal digits at every variable position
                if expected == b'd' {
                    observed.is_ascii_digit()
                } else {
                    observed == expected
                }
            })
}

/// Extract one exact runtime digest from its immutable GHCR reference.
fn image_digest(value: &str, runtime: &str) -> Option<String> {
    let prefix = format!("ghcr.io/anstosa/weather-{runtime}@sha256:");
    let digest = value.strip_prefix(&prefix)?;
    valid_hash(digest).then(|| format!("sha256:{digest}"))
}

/// Recognize one lowercase hexadecimal code unit.
fn lower_hex(value: u8) -> bool {
    value.is_ascii_digit() || (b'a'..=b'f').contains(&value)
}

#[cfg(test)]
#[path = "native_family_release_lineage_test.rs"]
mod tests;
