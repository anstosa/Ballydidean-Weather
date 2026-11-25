//! Connect grouped weather revision material to the retained scheduled-ingestion transaction.

use weather_api::AdmittedAdjustmentRevisionCaptureWitness;
use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionGapInput, AdjustmentRevisionGapReason,
    AdjustmentRevisionProjectionKind, AdjustmentRevisionStageReceipt,
    AdjustmentWeatherRevisionAdmissionKey, AdjustmentWeatherRevisionPointer,
    AdjustmentWeatherRevisionPointerSourceKindEnum, Nullable, SourceKind,
};
use weather_database::{
    AdjustmentMaintenanceDatabasePort, AdjustmentRevisionAdmissionKey, AdjustmentRevisionGapMarker,
    CompleteScheduledIngestion, ExistingWeatherRevisionIdentity,
    PendingPhysicalWeatherRevisionReadPort, PersistedWeatherAdjustmentRevision,
    RetainedSourceSession, ScheduledCheckpointQuery, ScheduledIngestionCommandPort,
    ScheduledIngestionExecutionError, ScheduledRunIdentityQuery, SerializedJson,
    WeatherRecordUpsertQuery, WeatherRevisionArchivePort, WeatherRevisionContextQuery,
    WeatherRevisionLookupRequest, WeatherRevisionSavepointCommand, WeatherRevisionSelectionQuery,
    WeatherRevisionSourceContext, WorkerRevisionArchiveWriteError, WorkerRevisionArchiveWritePort,
    WorkerRevisionArchiveWriteValues, adjustment_revision_gap_plan,
    bind_weather_adjustment_revisions, complete_scheduled_ingestion_with_revision_archive,
    list_pending_physical_weather_adjustment_revisions,
};
use weather_worker::TaskFailure;

use crate::api_maintenance_receipts::{
    ApiMaintenanceReceiptCanonicalValues, MaintenanceReceiptScalar,
    verify_revision_stage_receipt_for_identity,
};
use crate::worker_rain_maintenance_http::WorkerRevisionArchiveClient;
use crate::worker_weather_revision_material::{
    PreparedWeatherRevisionMaterial, SelectedWeatherRevisionGroup,
    WorkerWeatherRevisionMaterialValues, pending_physical_weather_revision_source,
    prepare_weather_revision_material, select_weather_revision_group,
};

/// Exact nested savepoint commands around the staged weather pointer bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerWeatherRevisionBindSavepointCommand {
    Savepoint,
    RollbackTo,
    Release,
}

impl WorkerWeatherRevisionBindSavepointCommand {
    /// Return the predecessor-owned command text.
    pub const fn sql(self) -> &'static str {
        // preserve the original nested savepoint spelling
        match self {
            Self::Savepoint => "SAVEPOINT weather_revision_bind",
            Self::RollbackTo => "ROLLBACK TO SAVEPOINT weather_revision_bind",
            Self::Release => "RELEASE SAVEPOINT weather_revision_bind",
        }
    }
}

/// Database operations not already represented by shared typed facades.
pub trait WorkerWeatherRevisionSessionPort: ScheduledIngestionCommandPort {
    /// Encode the prior-row lookup through the approved JSON owner.
    fn serialize_weather_revision_requests(
        &mut self,
        requests: &[WeatherRevisionLookupRequest],
    ) -> Result<SerializedJson, Self::Error>;

    /// Read prior serving identities before the normalized upsert.
    fn select_existing_weather_revision_rows(
        &mut self,
        query: &WeatherRevisionSelectionQuery,
    ) -> Result<Vec<ExistingWeatherRevisionIdentity>, Self::Error>;

    /// Read the ingestion-run lineage retained by the same source session.
    fn load_weather_revision_context(
        &mut self,
        query: &WeatherRevisionContextQuery,
    ) -> Result<Option<WeatherRevisionSourceContext>, Self::Error>;

    /// Execute the outer archive savepoint selected by the transaction owner.
    fn execute_weather_revision_archive_savepoint(
        &mut self,
        command: WeatherRevisionSavepointCommand,
    ) -> Result<(), Self::Error>;

    /// Execute the nested pointer-bind savepoint on the same retained session.
    fn execute_weather_revision_bind_savepoint(
        &mut self,
        command: WorkerWeatherRevisionBindSavepointCommand,
    ) -> Result<(), Self::Error>;

    /// Persist one publication-failure marker through the post-commit pool owner.
    fn write_post_commit_weather_revision_gap(
        &mut self,
        key: &AdjustmentRevisionAdmissionKey,
        gap: &AdjustmentRevisionGapInput,
    ) -> Result<Vec<AdjustmentRevisionGapMarker>, Self::Error>;
}

/// Shared canonical owners required by material, receipt, and database facades.
pub trait WorkerWeatherRevisionArchiveValues:
    WorkerWeatherRevisionMaterialValues
    + ApiMaintenanceReceiptCanonicalValues
    + WorkerRevisionArchiveWriteValues
{
}

impl<T> WorkerWeatherRevisionArchiveValues for T where
    T: WorkerWeatherRevisionMaterialValues
        + ApiMaintenanceReceiptCanonicalValues
        + WorkerRevisionArchiveWriteValues
{
}

/// One row-specific database marker and value-free archive gap.
#[derive(Debug, Clone, PartialEq)]
struct WorkerWeatherRevisionGap {
    key: AdjustmentRevisionAdmissionKey,
    input: AdjustmentRevisionGapInput,
}

/// Work retained across the serving commit without mutable adapters.
#[derive(Debug, Clone, PartialEq)]
enum WorkerWeatherRevisionPublication {
    Publish {
        bytes: Vec<u8>,
        gaps: Vec<WorkerWeatherRevisionGap>,
        receipts: Vec<AdjustmentRevisionCommitReceipt>,
        stage: AdjustmentRevisionStageReceipt,
    },
    Gaps(Vec<WorkerWeatherRevisionGap>),
}

/// Closed callback failure while the shared transaction decides recovery.
#[derive(Debug)]
pub enum WorkerWeatherRevisionArchiveError<DatabaseError, ValuesError> {
    Database(DatabaseError),
    Values(ValuesError),
    Material,
    Receipt,
    GapMarker,
    Client(TaskFailure),
}

/// Bind injected owners to one immutable capture epoch.
pub struct WorkerWeatherRevisionArchive<'a, Values, Client> {
    capture_witness: &'a AdmittedAdjustmentRevisionCaptureWitness,
    values: &'a mut Values,
    client: &'a mut Client,
}

impl<'a, Values, Client> WorkerWeatherRevisionArchive<'a, Values, Client> {
    /// Retain injected owners without performing database or archive work.
    pub const fn new(
        capture_witness: &'a AdmittedAdjustmentRevisionCaptureWitness,
        values: &'a mut Values,
        client: &'a mut Client,
    ) -> Self {
        Self {
            capture_witness,
            values,
            client,
        }
    }
}

/// Callback failure type projected from one concrete database and canonical values owner.
pub type WorkerWeatherRevisionArchiveFailure<Database, Values> = WorkerWeatherRevisionArchiveError<
    <Database as ScheduledIngestionCommandPort>::Error,
    <Values as WorkerRevisionArchiveWriteValues>::Error,
>;

/// Scheduled completion result with the archive callback's exact injected error owners.
pub type WorkerWeatherScheduledArchiveResult<Database, Values> = Result<
    (),
    ScheduledIngestionExecutionError<WorkerWeatherRevisionArchiveFailure<Database, Values>>,
>;

type ArchiveResult<Database, Values, Output> =
    Result<Output, WorkerWeatherRevisionArchiveFailure<Database, Values>>;

impl<Values, Client> WorkerWeatherRevisionArchive<'_, Values, Client>
where
    Values: WorkerWeatherRevisionArchiveValues,
    Client: WorkerRevisionArchiveClient,
{
    /// Select, stage, bind, and retain one publication inside the serving transaction.
    fn stage<Database>(
        &mut self,
        database: &mut Database,
        changed: &[PersistedWeatherAdjustmentRevision],
    ) -> ArchiveResult<Database, Values, Option<WorkerWeatherRevisionPublication>>
    where
        Database: ArchiveDatabase,
    {
        let pending = match pending_physical_weather_revision_source(changed) {
            Some(source_id) => list_pending_physical_weather_adjustment_revisions(
                database,
                self.values,
                source_id,
                &self.capture_witness.capture_epoch.utc,
            )
            .map_err(map_write_error)?,
            None => Vec::new(),
        };
        let Some(selected) = select_weather_revision_group(
            changed,
            &pending,
            &self.capture_witness.capture_epoch.utc,
            self.values,
        ) else {
            return Ok(None);
        };
        let prepared = match prepare_weather_revision_material(&selected, self.values) {
            Ok(prepared) => prepared,
            Err(_) => {
                let gaps = self.persist_gaps(
                    database,
                    &selected,
                    None,
                    AdjustmentRevisionGapReason::ArchiveStageFailed,
                )?;
                return Ok(Some(WorkerWeatherRevisionPublication::Gaps(gaps)));
            }
        };
        let kind = projection_kind(&selected);
        let stage = match self.client.stage_revision(&prepared.bytes) {
            Ok(stage)
                if verify_revision_stage_receipt_for_identity(
                    self.values,
                    prepared.identity.as_str(),
                    kind,
                    &stage,
                )
                .is_ok() =>
            {
                stage
            }
            _ => {
                let gaps = self.persist_gaps(
                    database,
                    &selected,
                    Some(prepared.identity.as_str()),
                    AdjustmentRevisionGapReason::ArchiveStageFailed,
                )?;
                return Ok(Some(WorkerWeatherRevisionPublication::Gaps(gaps)));
            }
        };
        self.bind_material(database, selected, prepared, stage)
            .map(Some)
    }

    /// Publish committed bytes or terminal gap dispositions outside the transaction.
    fn publish<Database>(
        &mut self,
        database: &mut Database,
        publication: WorkerWeatherRevisionPublication,
    ) -> ArchiveResult<Database, Values, ()>
    where
        Database: ArchiveDatabase,
    {
        match publication {
            WorkerWeatherRevisionPublication::Gaps(gaps) => {
                publish_recorded_gaps(self.client, &gaps)
            }
            WorkerWeatherRevisionPublication::Publish {
                bytes,
                gaps,
                receipts,
                stage,
            } => {
                if let Err(error) = self
                    .client
                    .publish_revision_batch(&bytes, &stage, &receipts)
                {
                    let publish_gaps = gaps
                        .into_iter()
                        .map(|gap| WorkerWeatherRevisionGap {
                            key: gap.key,
                            input: AdjustmentRevisionGapInput {
                                logicalKeySha256: gap.input.logicalKeySha256,
                                projectionIdentitySha256: gap.input.projectionIdentitySha256,
                                projectionKind: gap.input.projectionKind,
                                projectionSha256: gap.input.projectionSha256,
                                reason: AdjustmentRevisionGapReason::ArchivePublishFailed,
                            },
                        })
                        .collect::<Vec<_>>();
                    // use the post-commit database scope for every row marker
                    for gap in &publish_gaps {
                        let _marker_result = write_post_commit_gap_marker::<
                            Database,
                            <Values as WorkerRevisionArchiveWriteValues>::Error,
                        >(database, gap);
                    }
                    // retain one grouped archive disposition after row markers
                    if let Some(first) = publish_gaps.first() {
                        let _archive_result = self.client.record_revision_gap(&first.input);
                    }
                    return Err(WorkerWeatherRevisionArchiveError::Client(error));
                }
                Ok(())
            }
        }
    }

    /// Bind one stage receipt to every ordered serving row inside a nested savepoint.
    fn bind_material<Database>(
        &mut self,
        database: &mut Database,
        selected: SelectedWeatherRevisionGroup,
        prepared: PreparedWeatherRevisionMaterial,
        stage: AdjustmentRevisionStageReceipt,
    ) -> ArchiveResult<Database, Values, WorkerWeatherRevisionPublication>
    where
        Database: ArchiveDatabase,
    {
        let gaps = revision_gaps(
            self.values,
            &selected,
            Some(prepared.identity.as_str()),
            AdjustmentRevisionGapReason::DatabaseBindFailed,
        )?;
        let pointers = revision_pointers(&selected, prepared.identity.as_str(), &stage);
        database
            .execute_weather_revision_bind_savepoint(
                WorkerWeatherRevisionBindSavepointCommand::Savepoint,
            )
            .map_err(WorkerWeatherRevisionArchiveError::Database)?;
        let receipts = bind_weather_adjustment_revisions(database, self.values, &pointers);
        if let Ok(receipts) = receipts {
            // keep RELEASE inside the predecessor's bind try block
            if database
                .execute_weather_revision_bind_savepoint(
                    WorkerWeatherRevisionBindSavepointCommand::Release,
                )
                .is_ok()
            {
                Ok(WorkerWeatherRevisionPublication::Publish {
                    bytes: prepared.bytes,
                    gaps,
                    receipts,
                    stage,
                })
            } else {
                self.recover_bind_as_gaps(database, gaps)
            }
        } else {
            self.recover_bind_as_gaps(database, gaps)
        }
    }

    /// Recover the nested bind scope before retaining permanent row gaps.
    fn recover_bind_as_gaps<Database>(
        &mut self,
        database: &mut Database,
        gaps: Vec<WorkerWeatherRevisionGap>,
    ) -> ArchiveResult<Database, Values, WorkerWeatherRevisionPublication>
    where
        Database: ArchiveDatabase,
    {
        database
            .execute_weather_revision_bind_savepoint(
                WorkerWeatherRevisionBindSavepointCommand::RollbackTo,
            )
            .map_err(WorkerWeatherRevisionArchiveError::Database)?;
        database
            .execute_weather_revision_bind_savepoint(
                WorkerWeatherRevisionBindSavepointCommand::Release,
            )
            .map_err(WorkerWeatherRevisionArchiveError::Database)?;
        for gap in &gaps {
            write_gap_marker(database, gap)?;
        }
        Ok(WorkerWeatherRevisionPublication::Gaps(gaps))
    }

    /// Persist every row-specific failure marker before the serving commit.
    fn persist_gaps<Database>(
        &mut self,
        database: &mut Database,
        selected: &SelectedWeatherRevisionGroup,
        identity: Option<&str>,
        reason: AdjustmentRevisionGapReason,
    ) -> ArchiveResult<Database, Values, Vec<WorkerWeatherRevisionGap>>
    where
        Database: ArchiveDatabase,
    {
        let gaps = revision_gaps(self.values, selected, identity, reason)?;
        // retain every exact serving row marker inside the shared transaction
        for gap in &gaps {
            write_gap_marker(database, gap)?;
        }
        Ok(gaps)
    }
}

/// Database trait composition used by the concrete retained-session wrapper.
pub trait ArchiveDatabase:
    WorkerWeatherRevisionSessionPort
    + WorkerRevisionArchiveWritePort<Error = <Self as ScheduledIngestionCommandPort>::Error>
    + PendingPhysicalWeatherRevisionReadPort<Error = <Self as ScheduledIngestionCommandPort>::Error>
    + AdjustmentMaintenanceDatabasePort<Error = <Self as ScheduledIngestionCommandPort>::Error>
{
}

impl<T> ArchiveDatabase for T where
    T: WorkerWeatherRevisionSessionPort
        + WorkerRevisionArchiveWritePort<Error = <T as ScheduledIngestionCommandPort>::Error>
        + PendingPhysicalWeatherRevisionReadPort<Error = <T as ScheduledIngestionCommandPort>::Error>
        + AdjustmentMaintenanceDatabasePort<Error = <T as ScheduledIngestionCommandPort>::Error>
{
}

/// Adapter joining the callback owner to the database transaction engine.
struct WorkerWeatherRevisionSession<'a, Database, Values, Client> {
    database: &'a mut Database,
    archive: WorkerWeatherRevisionArchive<'a, Values, Client>,
}

/// Execute scheduled ingestion with weather revision callbacks enabled.
pub fn complete_scheduled_ingestion_with_weather_revision_archive<Database, Values, Client>(
    input: &CompleteScheduledIngestion,
    database: &mut Database,
    capture_witness: &AdmittedAdjustmentRevisionCaptureWitness,
    values: &mut Values,
    client: &mut Client,
) -> WorkerWeatherScheduledArchiveResult<Database, Values>
where
    Database: ArchiveDatabase,
    Values: WorkerWeatherRevisionArchiveValues,
    Client: WorkerRevisionArchiveClient,
{
    let archive = WorkerWeatherRevisionArchive::new(capture_witness, values, client);
    let mut session = WorkerWeatherRevisionSession { database, archive };
    complete_scheduled_ingestion_with_revision_archive(input, &mut session)
}

impl<Database, Values, Client> RetainedSourceSession
    for WorkerWeatherRevisionSession<'_, Database, Values, Client>
where
    Database: RetainedSourceSession,
{
    /// Delegate the immutable source identity to the retained database session.
    fn source_id(&self) -> &str {
        self.database.source_id()
    }
}

impl<Database, Values, Client> ScheduledIngestionCommandPort
    for WorkerWeatherRevisionSession<'_, Database, Values, Client>
where
    Database: ScheduledIngestionCommandPort,
    Values: WorkerRevisionArchiveWriteValues,
{
    type Error = WorkerWeatherRevisionArchiveError<
        Database::Error,
        <Values as WorkerRevisionArchiveWriteValues>::Error,
    >;

    /// Start the shared serving transaction.
    fn begin_transaction(&mut self) -> Result<(), Self::Error> {
        self.database
            .begin_transaction()
            .map_err(WorkerWeatherRevisionArchiveError::Database)
    }

    /// Assert the exact running scheduled identity.
    fn assert_scheduled_run(
        &mut self,
        query: &ScheduledRunIdentityQuery,
    ) -> Result<usize, Self::Error> {
        self.database
            .assert_scheduled_run(query)
            .map_err(WorkerWeatherRevisionArchiveError::Database)
    }

    /// Upsert one bounded normalized batch.
    fn upsert_weather_records(
        &mut self,
        query: &WeatherRecordUpsertQuery,
    ) -> Result<(), Self::Error> {
        self.database
            .upsert_weather_records(query)
            .map_err(WorkerWeatherRevisionArchiveError::Database)
    }

    /// Advance the scheduled checkpoint after archive staging.
    fn advance_scheduled_checkpoint(
        &mut self,
        query: &ScheduledCheckpointQuery,
    ) -> Result<usize, Self::Error> {
        self.database
            .advance_scheduled_checkpoint(query)
            .map_err(WorkerWeatherRevisionArchiveError::Database)
    }

    /// Finalize the running row after all pre-commit work.
    fn finalize_scheduled_run(
        &mut self,
        query: &weather_database::FinalizeScheduledRunQuery,
    ) -> Result<usize, Self::Error> {
        self.database
            .finalize_scheduled_run(query)
            .map_err(WorkerWeatherRevisionArchiveError::Database)
    }

    /// Commit serving data before any private archive publication.
    fn commit_transaction(&mut self) -> Result<(), Self::Error> {
        self.database
            .commit_transaction()
            .map_err(WorkerWeatherRevisionArchiveError::Database)
    }

    /// Roll back failed serving or pre-commit archive work.
    fn rollback_transaction(&mut self) -> Result<(), Self::Error> {
        self.database
            .rollback_transaction()
            .map_err(WorkerWeatherRevisionArchiveError::Database)
    }
}

impl<Database, Values, Client> WeatherRevisionArchivePort
    for WorkerWeatherRevisionSession<'_, Database, Values, Client>
where
    Database: ArchiveDatabase,
    Values: WorkerWeatherRevisionArchiveValues,
    Client: WorkerRevisionArchiveClient,
{
    type Error = WorkerWeatherRevisionArchiveError<
        <Database as ScheduledIngestionCommandPort>::Error,
        <Values as WorkerRevisionArchiveWriteValues>::Error,
    >;
    type Publication = WorkerWeatherRevisionPublication;

    /// Encode lookup requests through the retained session owner.
    fn serialize_revision_requests(
        &mut self,
        requests: &[WeatherRevisionLookupRequest],
    ) -> Result<SerializedJson, Self::Error> {
        self.database
            .serialize_weather_revision_requests(requests)
            .map_err(WorkerWeatherRevisionArchiveError::Database)
    }

    /// Select prior serving rows on the same transaction.
    fn select_existing_weather_revisions(
        &mut self,
        query: &WeatherRevisionSelectionQuery,
    ) -> Result<Vec<ExistingWeatherRevisionIdentity>, Self::Error> {
        self.database
            .select_existing_weather_revision_rows(query)
            .map_err(WorkerWeatherRevisionArchiveError::Database)
    }

    /// Load the locked ingestion-run lineage.
    fn load_weather_revision_source_context(
        &mut self,
        query: &WeatherRevisionContextQuery,
    ) -> Result<Option<WeatherRevisionSourceContext>, Self::Error> {
        self.database
            .load_weather_revision_context(query)
            .map_err(WorkerWeatherRevisionArchiveError::Database)
    }

    /// Execute the shared outer archive savepoint command.
    fn execute_weather_revision_savepoint(
        &mut self,
        command: WeatherRevisionSavepointCommand,
    ) -> Result<(), Self::Error> {
        self.database
            .execute_weather_revision_archive_savepoint(command)
            .map_err(WorkerWeatherRevisionArchiveError::Database)
    }

    /// Stage and bind one eligible group before the serving commit.
    fn stage_weather_revisions(
        &mut self,
        revisions: &[PersistedWeatherAdjustmentRevision],
    ) -> Result<Option<Self::Publication>, Self::Error> {
        self.archive.stage(self.database, revisions)
    }

    /// Publish only after the transaction engine admits the commit.
    fn publish_weather_revisions(
        &mut self,
        publication: Self::Publication,
    ) -> Result<(), Self::Error> {
        self.archive.publish(self.database, publication)
    }
}

/// Convert a typed database facade failure without erasing its owner.
fn map_write_error<DatabaseError, ValuesError>(
    error: WorkerRevisionArchiveWriteError<DatabaseError, ValuesError>,
) -> WorkerWeatherRevisionArchiveError<DatabaseError, ValuesError> {
    match error {
        WorkerRevisionArchiveWriteError::Port(error) => {
            WorkerWeatherRevisionArchiveError::Database(error)
        }
        WorkerRevisionArchiveWriteError::Values(error) => {
            WorkerWeatherRevisionArchiveError::Values(error)
        }
        WorkerRevisionArchiveWriteError::Admission(_) => {
            WorkerWeatherRevisionArchiveError::Material
        }
    }
}

/// Map the selected generated discriminator to the private API kind.
fn projection_kind(selected: &SelectedWeatherRevisionGroup) -> AdjustmentRevisionProjectionKind {
    match selected.projection_kind {
        weather_contracts::AdjustmentRevisionBatchProjectionProjectionKindEnum::ActualBestMatch => {
            AdjustmentRevisionProjectionKind::ActualBestMatch
        }
        weather_contracts::AdjustmentRevisionBatchProjectionProjectionKindEnum::TargetRevision => {
            AdjustmentRevisionProjectionKind::TargetRevision
        }
    }
}

/// Construct ordered database pointers for one durable stage receipt.
fn revision_pointers(
    selected: &SelectedWeatherRevisionGroup,
    identity: &str,
    stage: &AdjustmentRevisionStageReceipt,
) -> Vec<AdjustmentWeatherRevisionPointer> {
    selected
        .revisions
        .iter()
        .map(|revision| AdjustmentWeatherRevisionPointer {
            productRunAt: revision.record.productRunAt.clone(),
            projectionIdentitySha256: identity.to_owned(),
            projectionSha256: identity.to_owned(),
            sourceId: revision.record.sourceId.clone(),
            sourceKind: pointer_source_kind(revision.record.sourceKind),
            stageReceiptSha256: stage.stageReceiptSha256.clone(),
            storedContentSha256: revision.content_hash.as_str().to_owned(),
            validAt: revision.record.validAt.clone(),
        })
        .collect()
}

/// Construct every row key and gap using the original key field order.
fn revision_gaps<Values, DatabaseError>(
    values: &Values,
    selected: &SelectedWeatherRevisionGroup,
    identity: Option<&str>,
    reason: AdjustmentRevisionGapReason,
) -> Result<
    Vec<WorkerWeatherRevisionGap>,
    WorkerWeatherRevisionArchiveError<
        DatabaseError,
        <Values as WorkerRevisionArchiveWriteValues>::Error,
    >,
>
where
    Values: WorkerWeatherRevisionArchiveValues,
{
    let kind = projection_kind(selected);
    let mut gaps = Vec::with_capacity(selected.revisions.len());
    // bind each category to its exact serving logical key
    for revision in &selected.revisions {
        let product_run = match &revision.record.productRunAt {
            Nullable::Null => MaintenanceReceiptScalar::Null,
            Nullable::Value(value) => MaintenanceReceiptScalar::Text(value),
        };
        let fields = [
            ("productRunAt", product_run),
            (
                "sourceId",
                MaintenanceReceiptScalar::Text(&revision.record.sourceId),
            ),
            (
                "sourceKind",
                MaintenanceReceiptScalar::Text(source_kind_wire(revision.record.sourceKind)),
            ),
            (
                "validAt",
                MaintenanceReceiptScalar::Text(&revision.record.validAt),
            ),
        ];
        let logical = values
            .unsigned_receipt_sha256(&fields)
            .filter(|digest| valid_hash(digest.as_str()))
            .ok_or(WorkerWeatherRevisionArchiveError::Receipt)?;
        let optional_identity = identity.map_or(Nullable::Null, |identity| {
            Nullable::Value(identity.to_owned())
        });
        let key =
            AdjustmentRevisionAdmissionKey::WeatherRecord(AdjustmentWeatherRevisionAdmissionKey {
                productRunAt: revision.record.productRunAt.clone(),
                sourceId: revision.record.sourceId.clone(),
                sourceKind: pointer_source_kind(revision.record.sourceKind),
                storedContentSha256: revision.content_hash.as_str().to_owned(),
                validAt: revision.record.validAt.clone(),
            });
        gaps.push(WorkerWeatherRevisionGap {
            key,
            input: AdjustmentRevisionGapInput {
                logicalKeySha256: logical.as_str().to_owned(),
                projectionIdentitySha256: optional_identity.clone(),
                projectionKind: kind,
                projectionSha256: optional_identity,
                reason,
            },
        });
    }
    Ok(gaps)
}

/// Persist and validate one row-specific permanent database marker.
fn write_gap_marker<Database, ValuesError>(
    database: &mut Database,
    gap: &WorkerWeatherRevisionGap,
) -> Result<
    (),
    WorkerWeatherRevisionArchiveError<
        <Database as AdjustmentMaintenanceDatabasePort>::Error,
        ValuesError,
    >,
>
where
    Database: AdjustmentMaintenanceDatabasePort,
{
    let rows = database
        .write_revision_gap(adjustment_revision_gap_plan(&gap.key, &gap.input))
        .map_err(WorkerWeatherRevisionArchiveError::Database)?;
    let [marker] = rows.as_slice() else {
        return Err(WorkerWeatherRevisionArchiveError::GapMarker);
    };
    if !gap_marker_matches(marker, gap) {
        return Err(WorkerWeatherRevisionArchiveError::GapMarker);
    }
    Ok(())
}

/// Persist and validate one post-commit marker through the separate pool method.
fn write_post_commit_gap_marker<Database, ValuesError>(
    database: &mut Database,
    gap: &WorkerWeatherRevisionGap,
) -> Result<
    (),
    WorkerWeatherRevisionArchiveError<
        <Database as ScheduledIngestionCommandPort>::Error,
        ValuesError,
    >,
>
where
    Database: WorkerWeatherRevisionSessionPort,
{
    let rows = database
        .write_post_commit_weather_revision_gap(&gap.key, &gap.input)
        .map_err(WorkerWeatherRevisionArchiveError::Database)?;
    let [marker] = rows.as_slice() else {
        return Err(WorkerWeatherRevisionArchiveError::GapMarker);
    };
    if !gap_marker_matches(marker, gap) {
        return Err(WorkerWeatherRevisionArchiveError::GapMarker);
    }
    Ok(())
}

/// Require the marker to retain every supplied category and identity.
fn gap_marker_matches(
    marker: &AdjustmentRevisionGapMarker,
    gap: &WorkerWeatherRevisionGap,
) -> bool {
    let AdjustmentRevisionAdmissionKey::WeatherRecord(key) = &gap.key else {
        return false;
    };
    marker.logical_key_sha256 == gap.input.logicalKeySha256
        && marker.projection_identity_sha256 == gap.input.projectionIdentitySha256
        && marker.projection_kind == gap.input.projectionKind
        && marker.projection_sha256 == gap.input.projectionSha256
        && marker.reason == gap.input.reason
        && marker.stored_content_sha256 == key.storedContentSha256
}

/// Relay pre-commit gap categories after the serving transaction commits.
fn publish_recorded_gaps<Client: WorkerRevisionArchiveClient, DatabaseError, ValuesError>(
    client: &mut Client,
    gaps: &[WorkerWeatherRevisionGap],
) -> Result<(), WorkerWeatherRevisionArchiveError<DatabaseError, ValuesError>> {
    let projection_absent = gaps
        .first()
        .is_some_and(|gap| gap.input.projectionIdentitySha256 == Nullable::Null);
    let count = if projection_absent {
        gaps.len()
    } else {
        usize::from(!gaps.is_empty())
    };
    // preserve every no-body marker but one grouped disposition for shared bytes
    for gap in gaps.iter().take(count) {
        client
            .record_revision_gap(&gap.input)
            .map_err(WorkerWeatherRevisionArchiveError::Client)?;
    }
    Ok(())
}

/// Map the source class to the generated pointer discriminator.
fn pointer_source_kind(value: SourceKind) -> AdjustmentWeatherRevisionPointerSourceKindEnum {
    match value {
        SourceKind::Forecast => AdjustmentWeatherRevisionPointerSourceKindEnum::Forecast,
        SourceKind::PhysicalSensor => {
            AdjustmentWeatherRevisionPointerSourceKindEnum::PhysicalSensor
        }
        _ => unreachable!("material admission rejects unrelated source kinds"),
    }
}

/// Return the original lowercase source kind key spelling.
fn source_kind_wire(value: SourceKind) -> &'static str {
    match value {
        SourceKind::Forecast => "forecast",
        SourceKind::PhysicalSensor => "physical_sensor",
        _ => unreachable!("material admission rejects unrelated source kinds"),
    }
}

/// Recognize one immutable lowercase SHA-256 address.
fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "worker_weather_revision_archive_test.rs"]
mod tests;
