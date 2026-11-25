//! Execute the worker's ECMWF revision stage, bind, and post-commit publication lifecycle.

use weather_api::{AdmittedAdjustmentRevisionCaptureWitness, CurrentTimeResolver};
use weather_contracts::{
    AdjustmentEcmwfTemperatureRevisionAdmissionKey, AdjustmentEcmwfTemperatureRevisionPointer,
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionGapInput, AdjustmentRevisionGapReason,
    AdjustmentRevisionProjectionKind, AdjustmentRevisionStageReceipt,
    AdjustmentTemperatureNativeSourceProjection, JsDateOrString, Nullable,
    PersistedEcmwfTemperatureRevision,
};
use weather_database::{
    AdjustmentRevisionAdmissionKey, AdjustmentRevisionGapMarker, AdmittedEcmwfCanaryBestMatchRow,
    AdmittedRevisionUtcInstant, EcmwfCanaryBestMatchQuery, EcmwfCanaryExistingHoursQuery,
    EcmwfCanaryExistingRunQuery, EcmwfCanaryHourInsertQuery, EcmwfCanaryLogicalReceiptQuery,
    EcmwfCanaryPersistencePort, EcmwfCanaryReceiptUpdateQuery, EcmwfCanaryRevisionArchivePort,
    EcmwfCanaryRevisionSavepointCommand, EcmwfCanaryRunInsertQuery, EcmwfCanarySiteQuery,
    EcmwfCanarySiteRow, EcmwfTemperatureCanaryRunInitializationsReadPlan,
    MaintenancePublicationPlan, StoredEcmwfCanaryHourIdentity, StoredEcmwfCanaryRun,
    WorkerRevisionArchiveWritePort, WorkerRevisionArchiveWriteValues, adjustment_revision_gap_plan,
    bind_ecmwf_temperature_revision,
};
use weather_worker::TaskFailure;

use crate::api_maintenance_receipts::verify_revision_stage_receipt_for_identity;
use crate::api_revision_projection::revision_logical_key_sha256;
use crate::maintenance_canonical_json::valid_hash;
use crate::worker_ecmwf_revision_material::{
    EcmwfRevisionMaterialDigest, prepare_ecmwf_temperature_revision_material,
};
use crate::worker_rain_maintenance_http::WorkerRevisionArchiveClient;
use crate::{
    ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar,
    TemperatureCanaryPersistenceDatabasePort,
};

const MAXIMUM_PROJECTION_BYTES: usize = 64 * 1_024;

/// Approved ordered projection encoder plus the existing time, JSON, and digest owners.
pub trait EcmwfRevisionArchiveValues:
    ApiMaintenanceReceiptCanonicalValues
    + WorkerRevisionArchiveWriteValues
    + EcmwfRevisionMaterialDigest
{
    /// Encode one generated native-source projection in the original named field order.
    fn encode_ecmwf_revision_projection(
        &self,
        projection: &AdjustmentTemperatureNativeSourceProjection,
    ) -> Option<Vec<u8>>;
}

/// Exact nested savepoint commands around the staged revision database bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcmwfRevisionBindSavepointCommand {
    Savepoint,
    RollbackTo,
    Release,
}

impl EcmwfRevisionBindSavepointCommand {
    /// Return the source-owned command spelling.
    pub const fn sql(self) -> &'static str {
        // preserve the original savepoint name and recovery order
        match self {
            Self::Savepoint => "SAVEPOINT ecmwf_temperature_revision_bind",
            Self::RollbackTo => "ROLLBACK TO SAVEPOINT ecmwf_temperature_revision_bind",
            Self::Release => "RELEASE SAVEPOINT ecmwf_temperature_revision_bind",
        }
    }
}

/// Narrow database boundary for one permanent ECMWF revision disposition.
pub trait EcmwfRevisionGapPort {
    type Error;

    /// Execute the shared permanent-gap plan and decode every returned marker.
    fn write_ecmwf_revision_gap(
        &mut self,
        plan: MaintenancePublicationPlan<'_, 3>,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error>;
}

/// Same retained transaction used by the serving write and revision bind.
pub trait EcmwfRevisionArchiveTransactionPort:
    WorkerRevisionArchiveWritePort + EcmwfRevisionGapPort
{
    /// Execute one exact nested bind savepoint command.
    fn execute_ecmwf_revision_bind_savepoint(
        &mut self,
        command: EcmwfRevisionBindSavepointCommand,
    ) -> Result<(), <Self as WorkerRevisionArchiveWritePort>::Error>;
}

/// Concrete database capabilities composed by the archive-enabled canary repository.
pub trait EcmwfRevisionArchiveDatabasePort:
    EcmwfCanaryPersistencePort + EcmwfRevisionArchiveTransactionPort
{
    /// Execute the worker's exact warmup inventory statement.
    fn select_ecmwf_temperature_run_initializations(
        &mut self,
        plan: &EcmwfTemperatureCanaryRunInitializationsReadPlan,
    ) -> Result<Vec<JsDateOrString>, <Self as EcmwfCanaryPersistencePort>::Error>;

    /// Read the database-owned causal receipt inside the retained transaction.
    fn select_ecmwf_revision_logical_receipt(
        &mut self,
        query: &EcmwfCanaryLogicalReceiptQuery,
    ) -> Result<Option<AdmittedRevisionUtcInstant>, <Self as EcmwfCanaryPersistencePort>::Error>;

    /// Execute the outer archive savepoint owned by the shared persistence engine.
    fn execute_ecmwf_revision_archive_savepoint(
        &mut self,
        command: EcmwfCanaryRevisionSavepointCommand,
    ) -> Result<(), <Self as EcmwfCanaryPersistencePort>::Error>;

    /// Read the complete Best Match product visible at the causal receipt.
    fn select_ecmwf_revision_best_match_rows(
        &mut self,
        query: &EcmwfCanaryBestMatchQuery,
    ) -> Result<Vec<AdmittedEcmwfCanaryBestMatchRow>, <Self as EcmwfCanaryPersistencePort>::Error>;
}

/// Archive-enabled database adapter consumed directly by `DatabaseTemperatureCanaryRepository`.
pub struct EcmwfRevisionArchiveDatabase<Database, Values, Time, Client, Pool, MapError> {
    database: Database,
    values: Values,
    time: Time,
    client: Client,
    pool: Pool,
    capture_witness: AdmittedAdjustmentRevisionCaptureWitness,
    map_error: MapError,
}

impl<Database, Values, Time, Client, Pool, MapError>
    EcmwfRevisionArchiveDatabase<Database, Values, Time, Client, Pool, MapError>
{
    /// Compose already admitted runtime owners without performing I/O.
    pub const fn new(
        database: Database,
        values: Values,
        time: Time,
        client: Client,
        pool: Pool,
        capture_witness: AdmittedAdjustmentRevisionCaptureWitness,
        map_error: MapError,
    ) -> Self {
        Self {
            database,
            values,
            time,
            client,
            pool,
            capture_witness,
            map_error,
        }
    }

    /// Borrow the concrete database owner for diagnostics after shutdown.
    pub const fn database(&self) -> &Database {
        &self.database
    }

    /// Consume the adapter and return all five runtime owners.
    pub fn into_parts(self) -> (Database, Values, Time, Client, Pool) {
        (
            self.database,
            self.values,
            self.time,
            self.client,
            self.pool,
        )
    }
}

impl<Database, Values, Time, Client, Pool, MapError> EcmwfCanaryPersistencePort
    for EcmwfRevisionArchiveDatabase<Database, Values, Time, Client, Pool, MapError>
where
    Database: EcmwfCanaryPersistencePort,
{
    type Error = Database::Error;

    /// Delegate transaction start to the retained database owner.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.database.begin_transaction()
    }

    /// Delegate active-site selection to the retained database owner.
    fn select_site(
        &mut self,
        query: &EcmwfCanarySiteQuery,
    ) -> Result<Option<EcmwfCanarySiteRow>, Self::Error> {
        self.database.select_site(query)
    }

    /// Delegate immutable run insertion to the retained database owner.
    fn insert_run(
        &mut self,
        query: &EcmwfCanaryRunInsertQuery,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        self.database.insert_run(query)
    }

    /// Delegate one exact hour insert to the retained database owner.
    fn insert_hour(&mut self, query: &EcmwfCanaryHourInsertQuery) -> Result<(), Self::Error> {
        self.database.insert_hour(query)
    }

    /// Delegate retry locking to the retained database owner.
    fn select_existing_run(
        &mut self,
        query: &EcmwfCanaryExistingRunQuery,
    ) -> Result<Option<StoredEcmwfCanaryRun>, Self::Error> {
        self.database.select_existing_run(query)
    }

    /// Delegate retry hour identity reads to the retained database owner.
    fn select_existing_hours(
        &mut self,
        query: &EcmwfCanaryExistingHoursQuery,
    ) -> Result<Vec<StoredEcmwfCanaryHourIdentity>, Self::Error> {
        self.database.select_existing_hours(query)
    }

    /// Delegate monotonic receipt updates to the retained database owner.
    fn update_last_received(
        &mut self,
        query: &EcmwfCanaryReceiptUpdateQuery,
    ) -> Result<(), Self::Error> {
        self.database.update_last_received(query)
    }

    /// Commit before the shared engine invokes archive publication.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.database.commit_transaction()
    }

    /// Roll back any serving or archive-stage failure together.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.database.rollback_transaction()
    }

    /// Release the retained client after the engine's post-commit callback.
    fn release_client(&mut self) {
        self.database.release_client();
    }
}

impl<Database, Values, Time, Client, Pool, MapError> EcmwfCanaryRevisionArchivePort
    for EcmwfRevisionArchiveDatabase<Database, Values, Time, Client, Pool, MapError>
where
    Database: EcmwfRevisionArchiveDatabasePort
        + WorkerRevisionArchiveWritePort<Error = <Database as EcmwfCanaryPersistencePort>::Error>
        + EcmwfRevisionGapPort<Error = <Database as EcmwfCanaryPersistencePort>::Error>,
    Values: EcmwfRevisionArchiveValues,
    Time: CurrentTimeResolver,
    Client: WorkerRevisionArchiveClient,
    Pool: EcmwfRevisionGapPort,
    MapError: FnMut(EcmwfRevisionArchiveError) -> <Database as EcmwfCanaryPersistencePort>::Error,
{
    type Publication = StagedEcmwfRevisionPublication;

    /// Delegate the causal logical receipt to the concrete database owner.
    fn select_logical_receipt(
        &mut self,
        query: &EcmwfCanaryLogicalReceiptQuery,
    ) -> Result<Option<AdmittedRevisionUtcInstant>, Self::Error> {
        self.database.select_ecmwf_revision_logical_receipt(query)
    }

    /// Delegate outer archive savepoint transitions to the retained transaction.
    fn execute_revision_savepoint(
        &mut self,
        command: EcmwfCanaryRevisionSavepointCommand,
    ) -> Result<(), Self::Error> {
        self.database
            .execute_ecmwf_revision_archive_savepoint(command)
    }

    /// Delegate the causal comparator read to the concrete database owner.
    fn select_best_match_rows(
        &mut self,
        query: &EcmwfCanaryBestMatchQuery,
    ) -> Result<Vec<AdmittedEcmwfCanaryBestMatchRow>, Self::Error> {
        self.database.select_ecmwf_revision_best_match_rows(query)
    }

    /// Execute the concrete stage, bind, and gap lifecycle on the retained transaction.
    fn stage_ecmwf_temperature_revision(
        &mut self,
        revision: &PersistedEcmwfTemperatureRevision,
    ) -> Result<Option<Self::Publication>, Self::Error> {
        stage_ecmwf_revision_archive(
            revision,
            &self.capture_witness,
            &self.time,
            &mut self.database,
            &mut self.values,
            &mut self.client,
        )
        .map(Some)
        .map_err(&mut self.map_error)
    }

    /// Execute publication through the distinct pool owner after serving commit.
    fn publish_ecmwf_temperature_revision(
        &mut self,
        publication: Self::Publication,
    ) -> Result<(), Self::Error> {
        // the shared engine preserves serving after any permanent cold publication gap
        let _published =
            publish_ecmwf_revision_archive(publication, &mut self.pool, &mut self.client);
        Ok(())
    }
}

impl<Database, Values, Time, Client, Pool, MapError> TemperatureCanaryPersistenceDatabasePort
    for EcmwfRevisionArchiveDatabase<Database, Values, Time, Client, Pool, MapError>
where
    Database: EcmwfRevisionArchiveDatabasePort
        + WorkerRevisionArchiveWritePort<Error = <Database as EcmwfCanaryPersistencePort>::Error>
        + EcmwfRevisionGapPort<Error = <Database as EcmwfCanaryPersistencePort>::Error>,
    Values: EcmwfRevisionArchiveValues,
    Time: CurrentTimeResolver,
    Client: WorkerRevisionArchiveClient,
    Pool: EcmwfRevisionGapPort,
    MapError: FnMut(EcmwfRevisionArchiveError) -> <Database as EcmwfCanaryPersistencePort>::Error,
{
    /// Preserve the admitted statement across the archive-enabled database path.
    fn read_temperature_canary_run_initializations(
        &mut self,
        plan: &EcmwfTemperatureCanaryRunInitializationsReadPlan,
    ) -> Result<Vec<JsDateOrString>, Self::Error> {
        self.database
            .select_ecmwf_temperature_run_initializations(plan)
    }
}

/// Opaque work returned to the existing transaction engine for post-commit execution.
#[derive(Debug, Clone, PartialEq)]
pub struct StagedEcmwfRevisionPublication(StagedEcmwfRevisionPublicationInner);

/// Keep the pre-commit bytes and receipts inaccessible to callers before publication.
#[derive(Debug, Clone, PartialEq)]
enum StagedEcmwfRevisionPublicationInner {
    /// A durable body and database receipt ready for private archive publication.
    Ready(Box<ReadyEcmwfRevisionPublication>),
    /// A permanent in-transaction disposition that must be relayed after commit.
    Gap(AdjustmentRevisionGapInput),
}

/// One successful pre-commit archive transaction retained behind the opaque handle.
#[derive(Debug, Clone, PartialEq)]
struct ReadyEcmwfRevisionPublication {
    bytes: Vec<u8>,
    commit: AdjustmentRevisionCommitReceipt,
    database_key: AdjustmentEcmwfTemperatureRevisionAdmissionKey,
    identity: String,
    logical_key_sha256: String,
    stage: AdjustmentRevisionStageReceipt,
}

/// Failures that must escape to the outer archive savepoint or post-commit observer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcmwfRevisionArchiveError {
    Identity,
    Marker,
    Port,
    Publish,
    Recovery,
}

/// Prepare, stage, bind, and classify one revision before the serving commit.
pub fn stage_ecmwf_revision_archive<Database, Values, Client>(
    revision: &PersistedEcmwfTemperatureRevision,
    capture_witness: &AdmittedAdjustmentRevisionCaptureWitness,
    time: &dyn CurrentTimeResolver,
    database: &mut Database,
    values: &mut Values,
    client: &mut Client,
) -> Result<StagedEcmwfRevisionPublication, EcmwfRevisionArchiveError>
where
    Database: EcmwfRevisionArchiveTransactionPort
        + EcmwfRevisionGapPort<Error = <Database as WorkerRevisionArchiveWritePort>::Error>,
    Values: EcmwfRevisionArchiveValues,
    Client: WorkerRevisionArchiveClient,
{
    let logical_key_sha256 = logical_key_sha256(values, revision)?;
    let database_key = database_key(revision);
    let material = prepare_ecmwf_temperature_revision_material(
        revision,
        &capture_witness.capture_epoch,
        time,
        values,
    );
    let projection = match material {
        Ok(projection) => projection,
        Err(_) => {
            return stage_gap(
                database,
                &database_key,
                revision_gap(
                    logical_key_sha256,
                    None,
                    AdjustmentRevisionGapReason::ArchiveStageFailed,
                ),
            );
        }
    };
    let bytes = values
        .encode_ecmwf_revision_projection(&projection)
        .filter(|bytes| (2..=MAXIMUM_PROJECTION_BYTES).contains(&bytes.len()));
    let Some(bytes) = bytes else {
        return stage_gap(
            database,
            &database_key,
            revision_gap(
                logical_key_sha256,
                None,
                AdjustmentRevisionGapReason::ArchiveStageFailed,
            ),
        );
    };
    let identity = ApiMaintenanceReceiptCanonicalValues::sha256(values, &bytes)
        .filter(|digest| valid_hash(digest.as_str()))
        .map(|digest| digest.as_str().to_owned());
    let Some(identity) = identity else {
        return stage_gap(
            database,
            &database_key,
            revision_gap(
                logical_key_sha256,
                None,
                AdjustmentRevisionGapReason::ArchiveStageFailed,
            ),
        );
    };
    let stage = client.stage_revision(&bytes);
    let stage = match stage {
        Ok(stage)
            if verify_revision_stage_receipt_for_identity(
                values,
                &identity,
                AdjustmentRevisionProjectionKind::NativeSource,
                &stage,
            )
            .is_ok() =>
        {
            stage
        }
        _ => {
            return stage_gap(
                database,
                &database_key,
                revision_gap(
                    logical_key_sha256,
                    Some(identity),
                    AdjustmentRevisionGapReason::ArchiveStageFailed,
                ),
            );
        }
    };
    let pointer = AdjustmentEcmwfTemperatureRevisionPointer {
        projectionIdentitySha256: identity.clone(),
        projectionSha256: identity.clone(),
        providerResponseSha256: revision.providerResponseSha256.clone(),
        runInitializedAt: revision.runInitializedAt.clone(),
        siteId: revision.siteId.clone(),
        stageReceiptSha256: stage.stageReceiptSha256.clone(),
        storedContentSha256: revision.contentHash.clone(),
    };
    let commit = match bind_revision(database, values, &pointer) {
        Ok(commit) => commit,
        Err(BindFailure::Recovered) => {
            return stage_gap(
                database,
                &database_key,
                revision_gap(
                    logical_key_sha256,
                    Some(identity),
                    AdjustmentRevisionGapReason::DatabaseBindFailed,
                ),
            );
        }
        Err(BindFailure::Recovery) => return Err(EcmwfRevisionArchiveError::Recovery),
    };
    Ok(StagedEcmwfRevisionPublication(
        StagedEcmwfRevisionPublicationInner::Ready(Box::new(ReadyEcmwfRevisionPublication {
            bytes,
            commit,
            database_key,
            identity,
            logical_key_sha256,
            stage,
        })),
    ))
}

/// Publish only after commit, then durably classify any publication refusal.
pub fn publish_ecmwf_revision_archive<Pool, Client>(
    publication: StagedEcmwfRevisionPublication,
    pool: &mut Pool,
    client: &mut Client,
) -> Result<(), EcmwfRevisionArchiveError>
where
    Pool: EcmwfRevisionGapPort,
    Client: WorkerRevisionArchiveClient,
{
    match publication.0 {
        StagedEcmwfRevisionPublicationInner::Gap(gap) => {
            // the database marker remains authoritative when the private archive is unavailable
            let _recorded = client.record_revision_gap(&gap);
            Ok(())
        }
        StagedEcmwfRevisionPublicationInner::Ready(ready) => {
            let ReadyEcmwfRevisionPublication {
                bytes,
                commit,
                database_key,
                identity,
                logical_key_sha256,
                stage,
            } = *ready;
            // publication is deliberately outside the serving transaction
            if client.publish_revision(&bytes, &stage, &commit).is_ok() {
                return Ok(());
            }
            let gap = revision_gap(
                logical_key_sha256,
                Some(identity),
                AdjustmentRevisionGapReason::ArchivePublishFailed,
            );
            mark_gap(pool, &database_key, &gap)?;
            // relay only the matching category this caller successfully persisted
            let _recorded = client.record_revision_gap(&gap);
            Err(EcmwfRevisionArchiveError::Publish)
        }
    }
}

/// Bind within the exact nested savepoint and recover an ordinary failure.
fn bind_revision<Database, Values>(
    database: &mut Database,
    values: &mut Values,
    pointer: &AdjustmentEcmwfTemperatureRevisionPointer,
) -> Result<AdjustmentRevisionCommitReceipt, BindFailure>
where
    Database: EcmwfRevisionArchiveTransactionPort,
    Values: WorkerRevisionArchiveWriteValues,
{
    database
        .execute_ecmwf_revision_bind_savepoint(EcmwfRevisionBindSavepointCommand::Savepoint)
        .map_err(|_| BindFailure::Recovery)?;
    let attempted = bind_ecmwf_temperature_revision(database, values, pointer).and_then(|commit| {
        database
            .execute_ecmwf_revision_bind_savepoint(EcmwfRevisionBindSavepointCommand::Release)
            .map_err(weather_database::WorkerRevisionArchiveWriteError::Port)?;
        Ok(commit)
    });
    if let Ok(commit) = attempted {
        return Ok(commit);
    }
    // restore the retained serving transaction before its permanent gap write
    database
        .execute_ecmwf_revision_bind_savepoint(EcmwfRevisionBindSavepointCommand::RollbackTo)
        .map_err(|_| BindFailure::Recovery)?;
    database
        .execute_ecmwf_revision_bind_savepoint(EcmwfRevisionBindSavepointCommand::Release)
        .map_err(|_| BindFailure::Recovery)?;
    Err(BindFailure::Recovered)
}

/// Distinguish a recovered ordinary bind refusal from an unusable retained transaction.
enum BindFailure {
    Recovered,
    Recovery,
}

/// Persist an ordinary pre-commit gap and return its post-commit relay work.
fn stage_gap<Database>(
    database: &mut Database,
    key: &AdjustmentEcmwfTemperatureRevisionAdmissionKey,
    gap: AdjustmentRevisionGapInput,
) -> Result<StagedEcmwfRevisionPublication, EcmwfRevisionArchiveError>
where
    Database: EcmwfRevisionGapPort,
{
    mark_gap(database, key, &gap)?;
    Ok(StagedEcmwfRevisionPublication(
        StagedEcmwfRevisionPublicationInner::Gap(gap),
    ))
}

/// Execute and validate one shared database marker response.
fn mark_gap<Port>(
    port: &mut Port,
    key: &AdjustmentEcmwfTemperatureRevisionAdmissionKey,
    gap: &AdjustmentRevisionGapInput,
) -> Result<(), EcmwfRevisionArchiveError>
where
    Port: EcmwfRevisionGapPort,
{
    let admission = AdjustmentRevisionAdmissionKey::EcmwfTemperature(key.clone());
    let markers = port
        .write_ecmwf_revision_gap(adjustment_revision_gap_plan(&admission, gap))
        .map_err(|_| EcmwfRevisionArchiveError::Port)?;
    let [marker]: [AdjustmentRevisionGapMarker; 1] = markers
        .try_into()
        .map_err(|_| EcmwfRevisionArchiveError::Marker)?;
    // require the returned marker to retain every supplied identity and category
    if marker.logical_key_sha256 != gap.logicalKeySha256
        || marker.projection_identity_sha256 != gap.projectionIdentitySha256
        || marker.projection_kind != gap.projectionKind
        || marker.projection_sha256 != gap.projectionSha256
        || marker.reason != gap.reason
        || marker.stored_content_sha256 != key.storedContentSha256
    {
        return Err(EcmwfRevisionArchiveError::Marker);
    }
    Ok(())
}

/// Hash the exact native-source logical key before reading causal body values.
fn logical_key_sha256(
    values: &impl ApiMaintenanceReceiptCanonicalValues,
    revision: &PersistedEcmwfTemperatureRevision,
) -> Result<String, EcmwfRevisionArchiveError> {
    revision_logical_key_sha256(
        values,
        &[
            (
                "contentSha256",
                MaintenanceReceiptScalar::Text(&revision.contentHash),
            ),
            ("leadHours", MaintenanceReceiptScalar::Null),
            (
                "providerResponseSha256",
                MaintenanceReceiptScalar::Text(&revision.providerResponseSha256),
            ),
            (
                "runInitializedAt",
                MaintenanceReceiptScalar::Text(&revision.runInitializedAt),
            ),
            ("siteId", MaintenanceReceiptScalar::Text(&revision.siteId)),
            ("sourceId", MaintenanceReceiptScalar::Null),
            (
                "sourceType",
                MaintenanceReceiptScalar::Text("ecmwf_temperature_run"),
            ),
            ("validAt", MaintenanceReceiptScalar::Null),
        ],
    )
    .map_err(|_| EcmwfRevisionArchiveError::Identity)
}

/// Build the exact database key used by both binding and permanent gaps.
fn database_key(
    revision: &PersistedEcmwfTemperatureRevision,
) -> AdjustmentEcmwfTemperatureRevisionAdmissionKey {
    AdjustmentEcmwfTemperatureRevisionAdmissionKey {
        providerResponseSha256: revision.providerResponseSha256.clone(),
        runInitializedAt: revision.runInitializedAt.clone(),
        siteId: revision.siteId.clone(),
        storedContentSha256: revision.contentHash.clone(),
    }
}

/// Create one closed value-free disposition.
fn revision_gap(
    logical_key_sha256: String,
    identity: Option<String>,
    reason: AdjustmentRevisionGapReason,
) -> AdjustmentRevisionGapInput {
    let identity = identity.map_or(Nullable::Null, Nullable::Value);
    AdjustmentRevisionGapInput {
        logicalKeySha256: logical_key_sha256,
        projectionIdentitySha256: identity.clone(),
        projectionKind: AdjustmentRevisionProjectionKind::NativeSource,
        projectionSha256: identity,
        reason,
    }
}

/// Convert a boundary failure to the worker's existing categorical task error.
pub fn ecmwf_revision_archive_failure() -> TaskFailure {
    TaskFailure {
        attempts: 1,
        code: "ecmwf_temperature_revision_archive_failed".to_owned(),
    }
}

#[cfg(test)]
#[path = "worker_ecmwf_revision_archive_test.rs"]
mod tests;
