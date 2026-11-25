use std::collections::BTreeSet;

use crate::{
    CanonicalWeatherMetrics, FixedGaugeTargetResult, RAIN_COLLECTION_STATIONS, TaskFailure,
};

pub const RAIN_FIXED_GAUGE_TARGET_CATALOG_CONTRACT_VERSION: &str =
    "adjustment-rain-fixed-gauge-target-source-catalog/v1";
pub const RAIN_FIXED_GAUGE_TARGET_CATALOG_SHA256: &str =
    "ae9af10659350a555b48e135a61a04f3febb231b52aa5852b64c000016cee55d";
pub const RAIN_FIXED_GAUGE_TARGET_RUN_CONTRACT_SHA256: &str =
    "3d07bcb9a56470110197ff549e5cf3d07efe369e07998718b14ca5e2a8d12542";

/// One immutable dedicated target source admitted from the database catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainFixedGaugeSource {
    pub location_id: u64,
    pub source_config_fingerprint: String,
    pub source_id: String,
    pub source_key: String,
}

/// The complete ordered fixed-gauge source catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainFixedGaugeSourceCatalog {
    pub catalog_sha256: String,
    pub contract_version: String,
    pub run_contract_sha256: String,
    pub sources: Vec<RainFixedGaugeSource>,
}

/// One exact raw station response selected for a pending target hour.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainFixedGaugeCapture {
    pub body: Vec<u8>,
    pub body_sha256: String,
    pub claim_id: String,
    pub completed_at: String,
    pub station_id: u64,
    pub window_end_exclusive: String,
    pub window_start: String,
}

/// One retained raw claim address used by a normalized target row.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainFixedGaugeCaptureMember {
    pub body_sha256: String,
    pub claim_id: String,
    pub completed_at: String,
}

/// One normalized fixed-gauge physical-sensor record.
#[derive(Clone, Debug, PartialEq)]
pub struct RainFixedGaugeNormalizedRecord {
    pub metrics: CanonicalWeatherMetrics,
    pub received_at: String,
    pub source_id: String,
    pub valid_at: String,
}

/// One canonical projection row bound to a dedicated station source.
#[derive(Clone, Debug, PartialEq)]
pub struct RainFixedGaugeProjectionRow {
    pub capture_members: Vec<RainFixedGaugeCaptureMember>,
    pub normalized_record: RainFixedGaugeNormalizedRecord,
    pub station_id: u64,
    pub stored_content_sha256: String,
}

/// One typed canonical projection prepared by the admitted codec boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct RainFixedGaugeProjection {
    pub logical_received_at: String,
    pub rows: Vec<RainFixedGaugeProjectionRow>,
    pub valid_at: String,
}

/// Result from the admitted replay and canonical encoding boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum RainFixedGaugeBuildResult {
    Complete {
        bytes: Vec<u8>,
        projection: RainFixedGaugeProjection,
    },
    Gap {
        logical_hour_at: String,
    },
}

/// Permanent gap categories emitted by the fixed-gauge producer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RainFixedGaugeGapReason {
    TargetSourceOversized,
    ArchiveStageFailed,
    DatabaseBindFailed,
    ArchivePublishFailed,
    DatabaseAdmissionFailed,
}

impl RainFixedGaugeGapReason {
    /// Return the exact persisted Node reason code.
    pub const fn code(self) -> &'static str {
        // map every typed category to its durable wire identity
        match self {
            Self::TargetSourceOversized => "target_source_oversized",
            Self::ArchiveStageFailed => "archive_stage_failed",
            Self::DatabaseBindFailed => "database_bind_failed",
            Self::ArchivePublishFailed => "archive_publish_failed",
            Self::DatabaseAdmissionFailed => "database_admission_failed",
        }
    }
}

/// One authenticated durable pre-build oversized gap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainFixedGaugeDurableGap {
    pub gap_at: String,
    pub gap_sha256: String,
    pub logical_hour_at: String,
    pub logical_key_sha256: String,
    pub reason: RainFixedGaugeGapReason,
}

/// Existing durable oversized-gap status returned before body inflation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RainFixedGaugeGapStatus {
    Absent,
    Present(RainFixedGaugeDurableGap),
}

/// One durable group gap emitted by atomic database custody.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainFixedGaugeRevisionGap {
    pub logical_key_sha256: String,
    pub projection_identity_sha256: String,
    pub projection_sha256: String,
    pub reason: RainFixedGaugeGapReason,
}

/// One durable body stage receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainFixedGaugeStageReceipt {
    pub durable: bool,
    pub durable_at: String,
    pub projection_identity_sha256: String,
    pub projection_sha256: String,
    pub stage_receipt_sha256: String,
}

/// One server-issued target revision commit receipt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RainFixedGaugeCommitReceipt {
    pub archive_commit_ordinal: u64,
    pub archive_committed_at: String,
    pub frontier_sha256: String,
    pub predecessor_frontier_sha256: String,
    pub projection_identity_sha256: String,
    pub projection_sha256: String,
    pub receipt_sha256: String,
    pub stage_receipt_sha256: String,
}

/// One row persisted and bound inside the atomic revision group.
#[derive(Clone, Debug, PartialEq)]
pub struct RainFixedGaugeRevisionRow {
    pub normalized_record: RainFixedGaugeNormalizedRecord,
    pub station_id: u64,
    pub stored_content_sha256: String,
}

/// One complete target group passed to the repository transaction.
#[derive(Clone, Debug, PartialEq)]
pub struct RainFixedGaugeRevisionGroup {
    pub logical_key_sha256: String,
    pub projection_identity_sha256: String,
    pub projection_sha256: String,
    pub rows: Vec<RainFixedGaugeRevisionRow>,
    pub stage_receipt_sha256: Option<String>,
    pub valid_at: String,
}

/// Atomic database result for one complete target group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RainFixedGaugeRevisionResult {
    Committed(Vec<RainFixedGaugeCommitReceipt>),
    Gap(RainFixedGaugeRevisionGap),
}

/// Database effects required by the fixed-gauge target producer.
pub trait RainFixedGaugeRepository {
    /// Read the exact dedicated source catalog.
    fn read_source_catalog(&mut self) -> Result<RainFixedGaugeSourceCatalog, TaskFailure>;

    /// Select one latest complete post-epoch closed gauge hour.
    fn read_pending_hour(&mut self, capture_epoch_at: &str) -> Result<Option<String>, TaskFailure>;

    /// Inflate the exact ordered capture set only after gap admission.
    fn read_captures_for_hour(
        &mut self,
        capture_epoch_at: &str,
        valid_at: &str,
    ) -> Result<Vec<RainFixedGaugeCapture>, TaskFailure>;

    /// Atomically persist and bind one complete target group.
    fn commit_revision_group(
        &mut self,
        group: &RainFixedGaugeRevisionGroup,
    ) -> Result<RainFixedGaugeRevisionResult, TaskFailure>;

    /// Mark one committed group after publication or admission failure.
    fn mark_revision_group_gap(
        &mut self,
        group: &RainFixedGaugeRevisionGroup,
        reason: RainFixedGaugeGapReason,
    ) -> Result<RainFixedGaugeRevisionGap, TaskFailure>;
}

/// Replay and canonical encoding boundary admitted by the existing TypeScript codec.
pub trait RainFixedGaugeProjectionBuilder {
    /// Build normalized rows and canonical body bytes from admitted raw captures.
    fn build_projection(
        &mut self,
        captures: &[RainFixedGaugeCapture],
        sources: &[RainFixedGaugeSource],
        target_cutoff_at: &str,
        valid_at: &str,
    ) -> Result<RainFixedGaugeBuildResult, TaskFailure>;
}

/// Crypto and canonical-receipt verification delegated to admitted runtime code.
pub trait RainFixedGaugeIdentityAdapter {
    /// Derive the logical source-vector identity.
    fn logical_key_sha256(
        &mut self,
        source_ids: &[String],
        valid_at: &str,
    ) -> Result<String, TaskFailure>;

    /// Verify one raw body against its durable address.
    fn capture_body_matches(&mut self, body: &[u8], expected_sha256: &str) -> bool;

    /// Derive the canonical encoded projection identity.
    fn projection_identity_sha256(&mut self, bytes: &[u8]) -> Result<String, TaskFailure>;

    /// Derive one normalized weather-record content identity.
    fn weather_record_sha256(
        &mut self,
        station_id: u64,
        record: &RainFixedGaugeNormalizedRecord,
    ) -> Result<String, TaskFailure>;

    /// Verify the authenticated hash of one durable oversized-gap receipt.
    fn durable_gap_receipt_is_authentic(&mut self, gap: &RainFixedGaugeDurableGap) -> bool;

    /// Verify the authenticated hash of one durable body stage receipt.
    fn stage_receipt_is_authentic(&mut self, receipt: &RainFixedGaugeStageReceipt) -> bool;
}

/// Archive and gap-recording effects required after repository admission.
pub trait RainFixedGaugeClient {
    /// Read one already durable oversized-gap status.
    fn read_oversized_gap(
        &mut self,
        logical_hour_at: &str,
        logical_key_sha256: &str,
    ) -> Result<RainFixedGaugeGapStatus, TaskFailure>;

    /// Record one permanent pre-stage oversized gap.
    fn record_oversized_gap(
        &mut self,
        logical_hour_at: &str,
        logical_key_sha256: &str,
    ) -> Result<RainFixedGaugeDurableGap, TaskFailure>;

    /// Stage canonical body bytes before assigning database ordinals.
    fn stage_revision(&mut self, bytes: &[u8]) -> Result<RainFixedGaugeStageReceipt, TaskFailure>;

    /// Best-effort archive one already persisted database gap.
    fn record_revision_gap(&mut self, gap: &RainFixedGaugeRevisionGap) -> Result<(), TaskFailure>;

    /// Publish only one fully committed exact receipt vector.
    fn publish_revision_batch(
        &mut self,
        bytes: &[u8],
        stage_receipt: &RainFixedGaugeStageReceipt,
        commit_receipts: &[RainFixedGaugeCommitReceipt],
    ) -> Result<(), TaskFailure>;
}

/// One cohesive fixed-gauge target execution with typed effect ports.
pub struct RainFixedGaugeTargetExecution<'a, Repository, Builder, Identity, Client> {
    pub builder: &'a mut Builder,
    pub capture_epoch_at: &'a str,
    pub client: &'a mut Client,
    pub identity: &'a mut Identity,
    pub repository: &'a mut Repository,
}

// build one bounded target failure
fn target_failure(code: &str) -> TaskFailure {
    TaskFailure {
        attempts: 0,
        code: code.to_owned(),
    }
}

// require one lowercase hexadecimal sha-256 identity
fn sha256_is_valid(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

// require one positive decimal source identity
fn source_id_is_valid(value: &str) -> bool {
    let bytes = value.as_bytes();

    // reject empty, oversized, zero-prefixed, or non-decimal identities
    if bytes.is_empty() || bytes.len() > 128 || !matches!(bytes[0], b'1'..=b'9') {
        return false;
    }
    bytes[1..].iter().all(u8::is_ascii_digit)
}

// validate the immutable catalog before pending-hour or body work
fn validate_catalog(catalog: &RainFixedGaugeSourceCatalog) -> Result<(), TaskFailure> {
    // require the exact generated catalog and run identities
    if catalog.contract_version != RAIN_FIXED_GAUGE_TARGET_CATALOG_CONTRACT_VERSION
        || catalog.catalog_sha256 != RAIN_FIXED_GAUGE_TARGET_CATALOG_SHA256
        || catalog.run_contract_sha256 != RAIN_FIXED_GAUGE_TARGET_RUN_CONTRACT_SHA256
        || catalog.sources.len() != RAIN_COLLECTION_STATIONS.len()
    {
        return Err(target_failure(
            "rain fixed-gauge target source catalog differs",
        ));
    }
    let mut source_ids = BTreeSet::new();

    // retain fixed station order and distinct dedicated source identities
    for (source, station) in catalog.sources.iter().zip(RAIN_COLLECTION_STATIONS) {
        let expected_key = format!("rain-target-tempest-{}", station.location_id);

        // reject aliases and material source drift before raw-body reads
        if source.location_id != station.location_id
            || source.source_key != expected_key
            || !source_id_is_valid(&source.source_id)
            || !sha256_is_valid(&source.source_config_fingerprint)
            || !source_ids.insert(source.source_id.as_str())
        {
            return Err(target_failure(
                "rain fixed-gauge target source catalog differs",
            ));
        }
    }
    Ok(())
}

// validate existing durable oversized-gap custody
fn validate_durable_gap<I: RainFixedGaugeIdentityAdapter>(
    gap: &RainFixedGaugeDurableGap,
    logical_hour_at: &str,
    logical_key_sha256: &str,
    identity: &mut I,
) -> Result<(), TaskFailure> {
    // bind the permanent value-free gap to this exact logical target
    if gap.logical_hour_at != logical_hour_at
        || gap.logical_key_sha256 != logical_key_sha256
        || gap.reason != RainFixedGaugeGapReason::TargetSourceOversized
        || gap.gap_at.is_empty()
        || !sha256_is_valid(&gap.gap_sha256)
        || !identity.durable_gap_receipt_is_authentic(gap)
    {
        return Err(target_failure(
            "rain fixed-gauge target durable gap differs",
        ));
    }
    Ok(())
}

// validate ordered raw capture identities before replay
fn validate_captures<I: RainFixedGaugeIdentityAdapter>(
    captures: &[RainFixedGaugeCapture],
    identity: &mut I,
) -> Result<String, TaskFailure> {
    // require exactly one response in fixed station order
    if captures.len() != RAIN_COLLECTION_STATIONS.len() {
        return Err(target_failure(
            "rain fixed-gauge target capture set is incomplete",
        ));
    }
    let mut latest = None;

    // verify station order and whole-byte custody through the admitted hash boundary
    for (capture, station) in captures.iter().zip(RAIN_COLLECTION_STATIONS) {
        if capture.station_id != station.location_id
            || capture.claim_id.is_empty()
            || capture.completed_at.is_empty()
            || capture.window_start.is_empty()
            || capture.window_end_exclusive.is_empty()
            || !sha256_is_valid(&capture.body_sha256)
            || !identity.capture_body_matches(&capture.body, &capture.body_sha256)
        {
            return Err(target_failure(
                "rain fixed-gauge target capture order differs",
            ));
        }
        latest = Some(match latest {
            Some(previous) if previous > capture.completed_at.as_str() => previous,
            _ => capture.completed_at.as_str(),
        });
    }
    latest
        .map(str::to_owned)
        .ok_or_else(|| target_failure("rain fixed-gauge target capture set is empty"))
}

// require only the target precipitation and endpoint temperature metrics
fn metrics_are_target_only(metrics: CanonicalWeatherMetrics) -> bool {
    let precipitation_is_valid = metrics
        .precipitation_mm
        .is_none_or(|value| value.is_finite() && value >= 0.0);
    let temperature_is_valid = metrics.temperature_c.is_none_or(f64::is_finite);
    precipitation_is_valid
        && temperature_is_valid
        && metrics.apparent_temperature_c.is_none()
        && metrics.black_globe_temperature_c.is_none()
        && metrics.cloud_cover_percent.is_none()
        && metrics.pm25_micrograms_per_cubic_meter.is_none()
        && metrics.precipitation_rate_mm_per_hour == metrics.precipitation_mm
        && metrics.pressure_hpa.is_none()
        && metrics.relative_humidity_percent.is_none()
        && metrics
            .soil_electrical_conductivity_microsiemens_per_cm
            .is_none()
        && metrics.soil_moisture_percent.is_none()
        && metrics.solar_radiation_wm2.is_none()
        && metrics.uv_index.is_none()
        && metrics.water_level_m.is_none()
        && metrics.wind_direction_degrees.is_none()
        && metrics.wind_gust_mps.is_none()
        && metrics.wind_speed_mps.is_none()
        && metrics.wet_bulb_globe_temperature_c.is_none()
}

// validate normalized rows and their independent content identities
fn validate_projection<I: RainFixedGaugeIdentityAdapter>(
    projection: &RainFixedGaugeProjection,
    catalog: &RainFixedGaugeSourceCatalog,
    captures: &[RainFixedGaugeCapture],
    valid_at: &str,
    identity: &mut I,
) -> Result<(), TaskFailure> {
    // require one exact row for every frozen gauge
    if projection.valid_at != valid_at || projection.rows.len() != catalog.sources.len() {
        return Err(target_failure("rain fixed-gauge target projection differs"));
    }
    let mut logical_received_at = None;

    // retain catalog order and normalized target-only record semantics
    for ((row, source), station) in projection
        .rows
        .iter()
        .zip(&catalog.sources)
        .zip(RAIN_COLLECTION_STATIONS)
    {
        // require at least one immutable raw member per normalized target row
        if row.capture_members.is_empty() {
            return Err(target_failure("rain fixed-gauge target projection differs"));
        }

        // bind every normalized row member to one selected immutable raw capture
        if row.capture_members.iter().any(|member| {
            !captures.iter().any(|capture| {
                capture.station_id == row.station_id
                    && capture.claim_id == member.claim_id
                    && capture.body_sha256 == member.body_sha256
                    && capture.completed_at == member.completed_at
            })
        }) {
            return Err(target_failure("rain fixed-gauge target projection differs"));
        }
        let row_received_at = row
            .capture_members
            .iter()
            .map(|member| member.completed_at.as_str())
            .max()
            .ok_or_else(|| target_failure("rain fixed-gauge target projection differs"))?;
        let expected_content_sha256 =
            identity.weather_record_sha256(row.station_id, &row.normalized_record)?;

        // bind the parsed canonical record to source, station, hour, and content hash
        if row.station_id != station.location_id
            || source.location_id != station.location_id
            || row.normalized_record.source_id != source.source_id
            || row.normalized_record.valid_at != valid_at
            || row.normalized_record.received_at != row_received_at
            || !metrics_are_target_only(row.normalized_record.metrics)
            || row.stored_content_sha256 != expected_content_sha256
        {
            return Err(target_failure("rain fixed-gauge target projection differs"));
        }
        logical_received_at = Some(match logical_received_at {
            Some(previous) if previous > row_received_at => previous,
            _ => row_received_at,
        });
    }

    // bind the group logical clock to the latest actual raw member
    if logical_received_at != Some(projection.logical_received_at.as_str()) {
        return Err(target_failure("rain fixed-gauge target projection differs"));
    }
    Ok(())
}

// validate one stage receipt against the encoded projection identity
fn stage_receipt_is_valid<I: RainFixedGaugeIdentityAdapter>(
    receipt: &RainFixedGaugeStageReceipt,
    projection_identity_sha256: &str,
    identity: &mut I,
) -> bool {
    receipt.durable
        && !receipt.durable_at.is_empty()
        && receipt.projection_identity_sha256 == projection_identity_sha256
        && receipt.projection_sha256 == projection_identity_sha256
        && sha256_is_valid(&receipt.stage_receipt_sha256)
        && identity.stage_receipt_is_authentic(receipt)
}

// require one exact persisted group gap
fn require_revision_gap(
    result: RainFixedGaugeRevisionResult,
    expected_reason: RainFixedGaugeGapReason,
    group: &RainFixedGaugeRevisionGroup,
) -> Result<RainFixedGaugeRevisionGap, TaskFailure> {
    // reject committed receipts or mismatched durable gap identity
    let RainFixedGaugeRevisionResult::Gap(gap) = result else {
        return Err(target_failure("rain fixed-gauge target gap result differs"));
    };
    if gap.reason != expected_reason
        || gap.logical_key_sha256 != group.logical_key_sha256
        || gap.projection_identity_sha256 != group.projection_identity_sha256
        || gap.projection_sha256 != group.projection_sha256
    {
        return Err(target_failure("rain fixed-gauge target gap result differs"));
    }
    Ok(gap)
}

// validate one complete contiguous database receipt vector
fn validate_commit_receipts(
    receipts: &[RainFixedGaugeCommitReceipt],
    group: &RainFixedGaugeRevisionGroup,
    stage_receipt_sha256: &str,
) -> Result<(), TaskFailure> {
    // require every frozen gauge to have one server-issued receipt
    if receipts.len() != RAIN_COLLECTION_STATIONS.len() {
        return Err(target_failure(
            "rain fixed-gauge target receipt count differs",
        ));
    }
    let mut previous: Option<&RainFixedGaugeCommitReceipt> = None;

    // validate exact body identities and successor linkage in station order
    for receipt in receipts {
        if receipt.projection_identity_sha256 != group.projection_identity_sha256
            || receipt.projection_sha256 != group.projection_sha256
            || receipt.stage_receipt_sha256 != stage_receipt_sha256
            || receipt.archive_committed_at.is_empty()
            || !sha256_is_valid(&receipt.frontier_sha256)
            || !sha256_is_valid(&receipt.predecessor_frontier_sha256)
            || !sha256_is_valid(&receipt.receipt_sha256)
        {
            return Err(target_failure(
                "rain fixed-gauge target receipt vector differs",
            ));
        }

        // require contiguous ordinals and predecessor frontier linkage after the first receipt
        if let Some(previous_receipt) = previous
            && (receipt.archive_commit_ordinal != previous_receipt.archive_commit_ordinal + 1
                || receipt.predecessor_frontier_sha256 != previous_receipt.frontier_sha256)
        {
            return Err(target_failure(
                "rain fixed-gauge target receipt vector differs",
            ));
        }
        previous = Some(receipt);
    }
    Ok(())
}

// best-effort one already persisted database gap
fn record_revision_gap_best_effort<C: RainFixedGaugeClient>(
    client: &mut C,
    gap: &RainFixedGaugeRevisionGap,
) {
    let _ = client.record_revision_gap(gap);
}

/// Publish one complete closed-hour fixed-gauge target group.
pub fn publish_rain_fixed_gauge_target<Repository, Builder, Identity, Client>(
    execution: &mut RainFixedGaugeTargetExecution<'_, Repository, Builder, Identity, Client>,
) -> Result<FixedGaugeTargetResult, TaskFailure>
where
    Builder: RainFixedGaugeProjectionBuilder,
    Client: RainFixedGaugeClient,
    Identity: RainFixedGaugeIdentityAdapter,
    Repository: RainFixedGaugeRepository,
{
    let catalog = execution.repository.read_source_catalog()?;
    validate_catalog(&catalog)?;
    let pending_hour = execution
        .repository
        .read_pending_hour(execution.capture_epoch_at)?;

    // preserve no-work without archive, raw-body, or serving mutations
    let Some(valid_at) = pending_hour else {
        return Ok(FixedGaugeTargetResult::Idle);
    };
    let source_ids = catalog
        .sources
        .iter()
        .map(|source| source.source_id.clone())
        .collect::<Vec<_>>();
    let logical_key_sha256 = execution
        .identity
        .logical_key_sha256(&source_ids, &valid_at)?;
    let gap_status = execution
        .client
        .read_oversized_gap(&valid_at, &logical_key_sha256)?;

    // avoid reinflating raw bodies after one durable permanent gap
    if let RainFixedGaugeGapStatus::Present(gap) = gap_status {
        validate_durable_gap(&gap, &valid_at, &logical_key_sha256, execution.identity)?;
        return Ok(FixedGaugeTargetResult::Gap {
            reason: gap.reason.code().to_owned(),
        });
    }

    let captures = execution
        .repository
        .read_captures_for_hour(execution.capture_epoch_at, &valid_at)?;
    let target_cutoff_at = validate_captures(&captures, execution.identity)?;
    let built = execution.builder.build_projection(
        &captures,
        &catalog.sources,
        &target_cutoff_at,
        &valid_at,
    )?;
    let (bytes, projection) = match built {
        RainFixedGaugeBuildResult::Complete { bytes, projection } => (bytes, projection),
        RainFixedGaugeBuildResult::Gap { logical_hour_at } => {
            // bind the reviewed oversize refusal to the selected logical hour
            if logical_hour_at != valid_at {
                return Err(target_failure(
                    "rain fixed-gauge target oversize gap differs",
                ));
            }
            let gap = execution
                .client
                .record_oversized_gap(&valid_at, &logical_key_sha256)?;
            validate_durable_gap(&gap, &valid_at, &logical_key_sha256, execution.identity)?;
            return Ok(FixedGaugeTargetResult::Gap {
                reason: RainFixedGaugeGapReason::TargetSourceOversized
                    .code()
                    .to_owned(),
            });
        }
    };
    validate_projection(
        &projection,
        &catalog,
        &captures,
        &valid_at,
        execution.identity,
    )?;
    let projection_identity_sha256 = execution.identity.projection_identity_sha256(&bytes)?;
    let base_group = RainFixedGaugeRevisionGroup {
        logical_key_sha256,
        projection_identity_sha256: projection_identity_sha256.clone(),
        projection_sha256: projection_identity_sha256.clone(),
        rows: projection
            .rows
            .iter()
            .map(|row| RainFixedGaugeRevisionRow {
                normalized_record: row.normalized_record.clone(),
                station_id: row.station_id,
                stored_content_sha256: row.stored_content_sha256.clone(),
            })
            .collect(),
        stage_receipt_sha256: None,
        valid_at,
    };
    let stage_receipt = execution.client.stage_revision(&bytes);
    let stage_receipt = match stage_receipt {
        Ok(receipt)
            if stage_receipt_is_valid(
                &receipt,
                &projection_identity_sha256,
                execution.identity,
            ) =>
        {
            receipt
        }
        _ => {
            let result = execution.repository.commit_revision_group(&base_group)?;
            let gap = require_revision_gap(
                result,
                RainFixedGaugeGapReason::ArchiveStageFailed,
                &base_group,
            )?;
            record_revision_gap_best_effort(execution.client, &gap);
            return Ok(FixedGaugeTargetResult::Gap {
                reason: gap.reason.code().to_owned(),
            });
        }
    };
    let mut group = base_group;
    group.stage_receipt_sha256 = Some(stage_receipt.stage_receipt_sha256.clone());
    let committed = execution.repository.commit_revision_group(&group)?;
    let receipts = match committed {
        RainFixedGaugeRevisionResult::Committed(receipts) => receipts,
        gap @ RainFixedGaugeRevisionResult::Gap(_) => {
            let gap =
                require_revision_gap(gap, RainFixedGaugeGapReason::DatabaseBindFailed, &group)?;
            record_revision_gap_best_effort(execution.client, &gap);
            return Ok(FixedGaugeTargetResult::Gap {
                reason: gap.reason.code().to_owned(),
            });
        }
    };
    validate_commit_receipts(&receipts, &group, &stage_receipt.stage_receipt_sha256)?;

    // publish only after exact durable body and complete database receipt custody
    if execution
        .client
        .publish_revision_batch(&bytes, &stage_receipt, &receipts)
        .is_err()
    {
        let gap = execution
            .repository
            .mark_revision_group_gap(&group, RainFixedGaugeGapReason::ArchivePublishFailed);

        // archive only the exact permanent category persisted by this producer
        if let Ok(gap) = gap
            && gap.reason == RainFixedGaugeGapReason::ArchivePublishFailed
            && gap.logical_key_sha256 == group.logical_key_sha256
            && gap.projection_identity_sha256 == group.projection_identity_sha256
            && gap.projection_sha256 == group.projection_sha256
        {
            record_revision_gap_best_effort(execution.client, &gap);
            return Ok(FixedGaugeTargetResult::Gap {
                reason: gap.reason.code().to_owned(),
            });
        }
        return Ok(FixedGaugeTargetResult::Gap {
            reason: RainFixedGaugeGapReason::DatabaseAdmissionFailed
                .code()
                .to_owned(),
        });
    }

    Ok(FixedGaugeTargetResult::Published {
        revision_count: receipts.len(),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::{
        RAIN_FIXED_GAUGE_TARGET_CATALOG_CONTRACT_VERSION, RAIN_FIXED_GAUGE_TARGET_CATALOG_SHA256,
        RAIN_FIXED_GAUGE_TARGET_RUN_CONTRACT_SHA256, RainFixedGaugeBuildResult,
        RainFixedGaugeCapture, RainFixedGaugeCaptureMember, RainFixedGaugeClient,
        RainFixedGaugeCommitReceipt, RainFixedGaugeDurableGap, RainFixedGaugeGapReason,
        RainFixedGaugeGapStatus, RainFixedGaugeIdentityAdapter, RainFixedGaugeNormalizedRecord,
        RainFixedGaugeProjection, RainFixedGaugeProjectionBuilder, RainFixedGaugeProjectionRow,
        RainFixedGaugeRepository, RainFixedGaugeRevisionGap, RainFixedGaugeRevisionGroup,
        RainFixedGaugeRevisionResult, RainFixedGaugeSource, RainFixedGaugeSourceCatalog,
        RainFixedGaugeStageReceipt, RainFixedGaugeTargetExecution, publish_rain_fixed_gauge_target,
    };
    use crate::{
        CanonicalWeatherMetrics, FixedGaugeTargetResult, RAIN_COLLECTION_STATIONS, TaskFailure,
    };

    const VALID_AT: &str = "2026-09-14T07:00:00.000Z";
    const COMPLETED_AT: &str = "2026-09-14T07:12:00.000Z";

    // build one hexadecimal fixture identity
    fn hash(value: u64) -> String {
        format!("{value:064x}")
    }

    // build one bounded fixture failure
    fn failure(code: &str) -> TaskFailure {
        TaskFailure {
            attempts: 0,
            code: code.to_owned(),
        }
    }

    // build the exact ordered target source catalog
    fn source_catalog() -> RainFixedGaugeSourceCatalog {
        RainFixedGaugeSourceCatalog {
            catalog_sha256: RAIN_FIXED_GAUGE_TARGET_CATALOG_SHA256.to_owned(),
            contract_version: RAIN_FIXED_GAUGE_TARGET_CATALOG_CONTRACT_VERSION.to_owned(),
            run_contract_sha256: RAIN_FIXED_GAUGE_TARGET_RUN_CONTRACT_SHA256.to_owned(),
            sources: RAIN_COLLECTION_STATIONS
                .iter()
                .enumerate()
                .map(|(index, station)| RainFixedGaugeSource {
                    location_id: station.location_id,
                    source_config_fingerprint: hash(100 + index as u64),
                    source_id: (index + 1).to_string(),
                    source_key: format!("rain-target-tempest-{}", station.location_id),
                })
                .collect(),
        }
    }

    // build the exact ordered raw capture set
    fn captures() -> Vec<RainFixedGaugeCapture> {
        RAIN_COLLECTION_STATIONS
            .iter()
            .enumerate()
            .map(|(index, station)| RainFixedGaugeCapture {
                body: vec![index as u8],
                body_sha256: hash(200 + index as u64),
                claim_id: format!("claim-{}", index + 1),
                completed_at: COMPLETED_AT.to_owned(),
                station_id: station.location_id,
                window_end_exclusive: "2026-09-14T07:00:01.000Z".to_owned(),
                window_start: "2026-09-14T05:00:01.000Z".to_owned(),
            })
            .collect()
    }

    // retain only canonical target metrics
    fn target_metrics(value: f64) -> CanonicalWeatherMetrics {
        CanonicalWeatherMetrics {
            apparent_temperature_c: None,
            black_globe_temperature_c: None,
            cloud_cover_percent: None,
            pm25_micrograms_per_cubic_meter: None,
            precipitation_mm: Some(value),
            precipitation_rate_mm_per_hour: Some(value),
            pressure_hpa: None,
            relative_humidity_percent: None,
            soil_electrical_conductivity_microsiemens_per_cm: None,
            soil_moisture_percent: None,
            solar_radiation_wm2: None,
            temperature_c: Some(12.0),
            uv_index: None,
            water_level_m: None,
            wind_direction_degrees: None,
            wind_gust_mps: None,
            wind_speed_mps: None,
            wet_bulb_globe_temperature_c: None,
        }
    }

    #[derive(Clone, Copy)]
    enum BuildOutcome {
        Complete,
        Oversized,
    }

    struct FixtureBuilder {
        outcome: BuildOutcome,
        trace: Rc<RefCell<Vec<String>>>,
    }

    impl RainFixedGaugeProjectionBuilder for FixtureBuilder {
        // build typed normalized rows from the admitted replay fixture
        fn build_projection(
            &mut self,
            captures: &[RainFixedGaugeCapture],
            sources: &[RainFixedGaugeSource],
            target_cutoff_at: &str,
            valid_at: &str,
        ) -> Result<RainFixedGaugeBuildResult, TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("build:{}:{target_cutoff_at}", captures.len()));

            // return the reviewed pre-stage size refusal
            if matches!(self.outcome, BuildOutcome::Oversized) {
                return Ok(RainFixedGaugeBuildResult::Gap {
                    logical_hour_at: valid_at.to_owned(),
                });
            }
            let rows = captures
                .iter()
                .zip(sources)
                .enumerate()
                .map(|(index, (capture, source))| RainFixedGaugeProjectionRow {
                    capture_members: vec![RainFixedGaugeCaptureMember {
                        body_sha256: capture.body_sha256.clone(),
                        claim_id: capture.claim_id.clone(),
                        completed_at: capture.completed_at.clone(),
                    }],
                    normalized_record: RainFixedGaugeNormalizedRecord {
                        metrics: target_metrics(index as f64),
                        received_at: capture.completed_at.clone(),
                        source_id: source.source_id.clone(),
                        valid_at: valid_at.to_owned(),
                    },
                    station_id: capture.station_id,
                    stored_content_sha256: hash(400),
                })
                .collect();
            Ok(RainFixedGaugeBuildResult::Complete {
                bytes: b"canonical-target".to_vec(),
                projection: RainFixedGaugeProjection {
                    logical_received_at: COMPLETED_AT.to_owned(),
                    rows,
                    valid_at: valid_at.to_owned(),
                },
            })
        }
    }

    struct FixtureIdentity {
        trace: Rc<RefCell<Vec<String>>>,
    }

    impl RainFixedGaugeIdentityAdapter for FixtureIdentity {
        // derive one deterministic logical source-vector identity
        fn logical_key_sha256(
            &mut self,
            source_ids: &[String],
            valid_at: &str,
        ) -> Result<String, TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("logical-key:{}:{valid_at}", source_ids.len()));
            Ok(hash(300))
        }

        // verify fixture bodies against their station-order identity
        fn capture_body_matches(&mut self, body: &[u8], expected_sha256: &str) -> bool {
            body.first()
                .is_some_and(|value| expected_sha256 == hash(200 + u64::from(*value)))
        }

        // derive one deterministic canonical projection identity
        fn projection_identity_sha256(&mut self, bytes: &[u8]) -> Result<String, TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("projection-identity:{}", bytes.len()));
            Ok(hash(301))
        }

        // derive one deterministic normalized record content identity
        fn weather_record_sha256(
            &mut self,
            _station_id: u64,
            _record: &RainFixedGaugeNormalizedRecord,
        ) -> Result<String, TaskFailure> {
            Ok(hash(400))
        }

        // admit the fixture durable gap receipt hash
        fn durable_gap_receipt_is_authentic(&mut self, gap: &RainFixedGaugeDurableGap) -> bool {
            gap.gap_sha256 == hash(302)
        }

        // admit the fixture durable stage receipt hash
        fn stage_receipt_is_authentic(&mut self, receipt: &RainFixedGaugeStageReceipt) -> bool {
            receipt.stage_receipt_sha256 == hash(303)
        }
    }

    struct FixtureRepository {
        commit_result: Option<RainFixedGaugeRevisionResult>,
        mark_failure: bool,
        pending: Option<String>,
        trace: Rc<RefCell<Vec<String>>>,
    }

    impl RainFixedGaugeRepository for FixtureRepository {
        // return the exact ordered source catalog
        fn read_source_catalog(&mut self) -> Result<RainFixedGaugeSourceCatalog, TaskFailure> {
            self.trace.borrow_mut().push("catalog".to_owned());
            Ok(source_catalog())
        }

        // return one injected pending-hour selection
        fn read_pending_hour(
            &mut self,
            capture_epoch_at: &str,
        ) -> Result<Option<String>, TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("pending:{capture_epoch_at}"));
            Ok(self.pending.clone())
        }

        // inflate the exact ordered raw capture set
        fn read_captures_for_hour(
            &mut self,
            capture_epoch_at: &str,
            valid_at: &str,
        ) -> Result<Vec<RainFixedGaugeCapture>, TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("captures:{capture_epoch_at}:{valid_at}"));
            Ok(captures())
        }

        // retain one atomic commit effect and injected result
        fn commit_revision_group(
            &mut self,
            group: &RainFixedGaugeRevisionGroup,
        ) -> Result<RainFixedGaugeRevisionResult, TaskFailure> {
            self.trace.borrow_mut().push(format!(
                "commit:{}",
                group.stage_receipt_sha256.as_deref().unwrap_or("null")
            ));
            self.commit_result
                .take()
                .ok_or_else(|| failure("missing commit result"))
        }

        // retain one post-commit gap effect
        fn mark_revision_group_gap(
            &mut self,
            group: &RainFixedGaugeRevisionGroup,
            reason: RainFixedGaugeGapReason,
        ) -> Result<RainFixedGaugeRevisionGap, TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("mark-gap:{}", reason.code()));

            // inject one database admission failure
            if self.mark_failure {
                return Err(failure("mark failed"));
            }
            Ok(RainFixedGaugeRevisionGap {
                logical_key_sha256: group.logical_key_sha256.clone(),
                projection_identity_sha256: group.projection_identity_sha256.clone(),
                projection_sha256: group.projection_sha256.clone(),
                reason,
            })
        }
    }

    struct FixtureClient {
        gap_status: RainFixedGaugeGapStatus,
        publish_failure: bool,
        stage_failure: bool,
        trace: Rc<RefCell<Vec<String>>>,
    }

    impl RainFixedGaugeClient for FixtureClient {
        // return one injected durable oversized-gap status
        fn read_oversized_gap(
            &mut self,
            logical_hour_at: &str,
            logical_key_sha256: &str,
        ) -> Result<RainFixedGaugeGapStatus, TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("gap-status:{logical_hour_at}:{logical_key_sha256}"));
            Ok(self.gap_status.clone())
        }

        // persist one authenticated oversized-gap receipt
        fn record_oversized_gap(
            &mut self,
            logical_hour_at: &str,
            logical_key_sha256: &str,
        ) -> Result<RainFixedGaugeDurableGap, TaskFailure> {
            self.trace.borrow_mut().push("oversize-gap".to_owned());
            Ok(RainFixedGaugeDurableGap {
                gap_at: COMPLETED_AT.to_owned(),
                gap_sha256: hash(302),
                logical_hour_at: logical_hour_at.to_owned(),
                logical_key_sha256: logical_key_sha256.to_owned(),
                reason: RainFixedGaugeGapReason::TargetSourceOversized,
            })
        }

        // stage the canonical target body or inject archive failure
        fn stage_revision(
            &mut self,
            bytes: &[u8],
        ) -> Result<RainFixedGaugeStageReceipt, TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("stage:{}", bytes.len()));

            // inject archive stage failure before database ordinals
            if self.stage_failure {
                return Err(failure("archive unavailable"));
            }
            Ok(RainFixedGaugeStageReceipt {
                durable: true,
                durable_at: COMPLETED_AT.to_owned(),
                projection_identity_sha256: hash(301),
                projection_sha256: hash(301),
                stage_receipt_sha256: hash(303),
            })
        }

        // retain one best-effort already persisted group gap
        fn record_revision_gap(
            &mut self,
            gap: &RainFixedGaugeRevisionGap,
        ) -> Result<(), TaskFailure> {
            self.trace
                .borrow_mut()
                .push(format!("record-gap:{}", gap.reason.code()));
            Ok(())
        }

        // publish only the complete committed receipt vector
        fn publish_revision_batch(
            &mut self,
            bytes: &[u8],
            _stage_receipt: &RainFixedGaugeStageReceipt,
            commit_receipts: &[RainFixedGaugeCommitReceipt],
        ) -> Result<(), TaskFailure> {
            self.trace.borrow_mut().push(format!(
                "publish:{}:{}",
                bytes.len(),
                commit_receipts.len()
            ));

            // inject post-commit archive publication failure
            if self.publish_failure {
                return Err(failure("publish failed"));
            }
            Ok(())
        }
    }

    // build one exact contiguous committed receipt vector
    fn commit_receipts() -> Vec<RainFixedGaugeCommitReceipt> {
        (0..RAIN_COLLECTION_STATIONS.len())
            .map(|index| RainFixedGaugeCommitReceipt {
                archive_commit_ordinal: 31 + index as u64,
                archive_committed_at: COMPLETED_AT.to_owned(),
                frontier_sha256: hash(500 + index as u64),
                predecessor_frontier_sha256: if index == 0 {
                    hash(499)
                } else {
                    hash(499 + index as u64)
                },
                projection_identity_sha256: hash(301),
                projection_sha256: hash(301),
                receipt_sha256: hash(600 + index as u64),
                stage_receipt_sha256: hash(303),
            })
            .collect()
    }

    // execute one fixture producer configuration
    fn execute(
        repository: &mut FixtureRepository,
        builder: &mut FixtureBuilder,
        identity: &mut FixtureIdentity,
        client: &mut FixtureClient,
    ) -> Result<FixedGaugeTargetResult, TaskFailure> {
        publish_rain_fixed_gauge_target(&mut RainFixedGaugeTargetExecution {
            builder,
            capture_epoch_at: "2000-01-01T00:00:00.000Z",
            client,
            identity,
            repository,
        })
    }

    // publish only after catalog, pending, gap, body, stage, and atomic commit custody
    #[test]
    fn fixed_gauge_target_publishes_complete_normalized_group_in_order() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut repository = FixtureRepository {
            commit_result: Some(RainFixedGaugeRevisionResult::Committed(commit_receipts())),
            mark_failure: false,
            pending: Some(VALID_AT.to_owned()),
            trace: Rc::clone(&trace),
        };
        let mut builder = FixtureBuilder {
            outcome: BuildOutcome::Complete,
            trace: Rc::clone(&trace),
        };
        let mut identity = FixtureIdentity {
            trace: Rc::clone(&trace),
        };
        let mut client = FixtureClient {
            gap_status: RainFixedGaugeGapStatus::Absent,
            publish_failure: false,
            stage_failure: false,
            trace: Rc::clone(&trace),
        };
        let result = execute(&mut repository, &mut builder, &mut identity, &mut client)
            .expect("complete target should publish");

        assert_eq!(
            result,
            FixedGaugeTargetResult::Published { revision_count: 12 }
        );
        assert_eq!(
            trace.borrow().as_slice(),
            [
                "catalog",
                "pending:2000-01-01T00:00:00.000Z",
                &format!("logical-key:12:{VALID_AT}"),
                &format!("gap-status:{VALID_AT}:{}", hash(300)),
                &format!("captures:2000-01-01T00:00:00.000Z:{VALID_AT}"),
                &format!("build:12:{COMPLETED_AT}"),
                "projection-identity:16",
                "stage:16",
                &format!("commit:{}", hash(303)),
                "publish:16:12",
            ]
        );
    }

    // preserve idle and pre-existing permanent gap without inflating raw bodies
    #[test]
    fn fixed_gauge_target_preserves_idle_and_existing_gap_boundaries() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut repository = FixtureRepository {
            commit_result: None,
            mark_failure: false,
            pending: None,
            trace: Rc::clone(&trace),
        };
        let mut builder = FixtureBuilder {
            outcome: BuildOutcome::Complete,
            trace: Rc::clone(&trace),
        };
        let mut identity = FixtureIdentity {
            trace: Rc::clone(&trace),
        };
        let mut client = FixtureClient {
            gap_status: RainFixedGaugeGapStatus::Absent,
            publish_failure: false,
            stage_failure: false,
            trace: Rc::clone(&trace),
        };
        assert_eq!(
            execute(&mut repository, &mut builder, &mut identity, &mut client,)
                .expect("no pending target should stay idle"),
            FixedGaugeTargetResult::Idle
        );
        assert_eq!(trace.borrow().len(), 2);

        trace.borrow_mut().clear();
        repository.pending = Some(VALID_AT.to_owned());
        client.gap_status = RainFixedGaugeGapStatus::Present(RainFixedGaugeDurableGap {
            gap_at: COMPLETED_AT.to_owned(),
            gap_sha256: hash(302),
            logical_hour_at: VALID_AT.to_owned(),
            logical_key_sha256: hash(300),
            reason: RainFixedGaugeGapReason::TargetSourceOversized,
        });
        assert_eq!(
            execute(&mut repository, &mut builder, &mut identity, &mut client,)
                .expect("durable oversize gap should skip raw body work"),
            FixedGaugeTargetResult::Gap {
                reason: "target_source_oversized".to_owned(),
            }
        );
        assert!(
            !trace
                .borrow()
                .iter()
                .any(|event| event.starts_with("captures:"))
        );
    }

    // persist one value-free group gap when durable body staging fails
    #[test]
    fn fixed_gauge_target_commits_stage_failure_without_ordinals() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut repository = FixtureRepository {
            commit_result: Some(RainFixedGaugeRevisionResult::Gap(
                RainFixedGaugeRevisionGap {
                    logical_key_sha256: hash(300),
                    projection_identity_sha256: hash(301),
                    projection_sha256: hash(301),
                    reason: RainFixedGaugeGapReason::ArchiveStageFailed,
                },
            )),
            mark_failure: false,
            pending: Some(VALID_AT.to_owned()),
            trace: Rc::clone(&trace),
        };
        let mut builder = FixtureBuilder {
            outcome: BuildOutcome::Complete,
            trace: Rc::clone(&trace),
        };
        let mut identity = FixtureIdentity {
            trace: Rc::clone(&trace),
        };
        let mut client = FixtureClient {
            gap_status: RainFixedGaugeGapStatus::Absent,
            publish_failure: false,
            stage_failure: true,
            trace: Rc::clone(&trace),
        };
        let result = execute(&mut repository, &mut builder, &mut identity, &mut client)
            .expect("stage failure should persist one group gap");

        assert_eq!(
            result,
            FixedGaugeTargetResult::Gap {
                reason: "archive_stage_failed".to_owned(),
            }
        );
        assert!(trace.borrow().iter().any(|event| event == "commit:null"));
        assert!(
            trace
                .borrow()
                .iter()
                .any(|event| event == "record-gap:archive_stage_failed")
        );
        assert!(
            !trace
                .borrow()
                .iter()
                .any(|event| event.starts_with("publish:"))
        );
    }

    // retain the repository's complete-group database bind gap without publication
    #[test]
    fn fixed_gauge_target_preserves_database_bind_gap() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut repository = FixtureRepository {
            commit_result: Some(RainFixedGaugeRevisionResult::Gap(
                RainFixedGaugeRevisionGap {
                    logical_key_sha256: hash(300),
                    projection_identity_sha256: hash(301),
                    projection_sha256: hash(301),
                    reason: RainFixedGaugeGapReason::DatabaseBindFailed,
                },
            )),
            mark_failure: false,
            pending: Some(VALID_AT.to_owned()),
            trace: Rc::clone(&trace),
        };
        let mut builder = FixtureBuilder {
            outcome: BuildOutcome::Complete,
            trace: Rc::clone(&trace),
        };
        let mut identity = FixtureIdentity {
            trace: Rc::clone(&trace),
        };
        let mut client = FixtureClient {
            gap_status: RainFixedGaugeGapStatus::Absent,
            publish_failure: false,
            stage_failure: false,
            trace: Rc::clone(&trace),
        };
        let result = execute(&mut repository, &mut builder, &mut identity, &mut client)
            .expect("database bind gap should remain a completed sidecar result");

        assert_eq!(
            result,
            FixedGaugeTargetResult::Gap {
                reason: "database_bind_failed".to_owned(),
            }
        );
        assert!(
            trace
                .borrow()
                .iter()
                .any(|event| event == "record-gap:database_bind_failed")
        );
        assert!(
            !trace
                .borrow()
                .iter()
                .any(|event| event.starts_with("publish:"))
        );
    }

    // persist and archive one post-commit publication gap
    #[test]
    fn fixed_gauge_target_marks_archive_publish_failure() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut repository = FixtureRepository {
            commit_result: Some(RainFixedGaugeRevisionResult::Committed(commit_receipts())),
            mark_failure: false,
            pending: Some(VALID_AT.to_owned()),
            trace: Rc::clone(&trace),
        };
        let mut builder = FixtureBuilder {
            outcome: BuildOutcome::Complete,
            trace: Rc::clone(&trace),
        };
        let mut identity = FixtureIdentity {
            trace: Rc::clone(&trace),
        };
        let mut client = FixtureClient {
            gap_status: RainFixedGaugeGapStatus::Absent,
            publish_failure: true,
            stage_failure: false,
            trace: Rc::clone(&trace),
        };
        let result = execute(&mut repository, &mut builder, &mut identity, &mut client)
            .expect("publication failure should retain one permanent group gap");

        assert_eq!(
            result,
            FixedGaugeTargetResult::Gap {
                reason: "archive_publish_failed".to_owned(),
            }
        );
        assert!(
            trace
                .borrow()
                .iter()
                .any(|event| event == "mark-gap:archive_publish_failed")
        );
        assert!(
            trace
                .borrow()
                .iter()
                .any(|event| event == "record-gap:archive_publish_failed")
        );
    }

    // record the reviewed oversize result without stage or database mutation
    #[test]
    fn fixed_gauge_target_records_oversize_before_stage() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut repository = FixtureRepository {
            commit_result: None,
            mark_failure: false,
            pending: Some(VALID_AT.to_owned()),
            trace: Rc::clone(&trace),
        };
        let mut builder = FixtureBuilder {
            outcome: BuildOutcome::Oversized,
            trace: Rc::clone(&trace),
        };
        let mut identity = FixtureIdentity {
            trace: Rc::clone(&trace),
        };
        let mut client = FixtureClient {
            gap_status: RainFixedGaugeGapStatus::Absent,
            publish_failure: false,
            stage_failure: false,
            trace: Rc::clone(&trace),
        };
        let result = execute(&mut repository, &mut builder, &mut identity, &mut client)
            .expect("oversize target should become one durable gap");

        assert_eq!(
            result,
            FixedGaugeTargetResult::Gap {
                reason: "target_source_oversized".to_owned(),
            }
        );
        assert!(trace.borrow().iter().any(|event| event == "oversize-gap"));
        assert!(
            !trace
                .borrow()
                .iter()
                .any(|event| event.starts_with("stage:"))
        );
        assert!(
            !trace
                .borrow()
                .iter()
                .any(|event| event.starts_with("commit:"))
        );
    }
}
