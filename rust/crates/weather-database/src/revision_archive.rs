use std::error::Error;
use std::fmt::{Display, Formatter};

use weather_contracts::{NormalizedWeatherRecord, Nullable, SourceKind};

use crate::{PreparedWeatherRecord, RetainedSourceSession, SerializedJson, Sha256Digest};

/// the source-owned lookup for prior serving revision identities.
pub const SELECT_EXISTING_WEATHER_REVISIONS_SQL: &str = r#"
    SELECT wr.content_hash AS "contentHash", wr.product_run_at AS "productRunAt",
      wr.source_kind AS "sourceKind", wr.valid_at AS "validAt"
    FROM weather_records wr
    JOIN jsonb_to_recordset($2::jsonb) AS requested(
      "productRunAt" timestamptz, "sourceKind" text, "validAt" timestamptz
    ) ON requested."sourceKind" = wr.source_kind
      AND requested."validAt" = wr.valid_at
      AND requested."productRunAt" IS NOT DISTINCT FROM wr.product_run_at
    WHERE wr.source_id = $1
  "#;

/// the source-owned lineage lookup for one changed revision group.
pub const WEATHER_REVISION_SOURCE_CONTEXT_SQL: &str = r#"
    SELECT run.adapter_version AS "adapterVersion", run.started_at AS "logicalReceivedAt",
      provider.provider_key AS "providerKey",
      run.source_config_fingerprint::text AS "sourceConfigFingerprint",
      source.source_key AS "sourceKey"
    FROM ingestion_runs run
    JOIN sources source ON source.id = run.source_id
    JOIN providers provider ON provider.id = source.provider_id
    WHERE run.id = $1 AND run.source_id = $2
  "#;

/// the exact archive savepoint creation command.
pub const SAVEPOINT_WEATHER_REVISION_ARCHIVE_SQL: &str = "SAVEPOINT weather_revision_archive";
/// the exact archive savepoint rollback command.
pub const ROLLBACK_WEATHER_REVISION_ARCHIVE_SQL: &str =
    "ROLLBACK TO SAVEPOINT weather_revision_archive";
/// the exact archive savepoint release command.
pub const RELEASE_WEATHER_REVISION_ARCHIVE_SQL: &str = "RELEASE SAVEPOINT weather_revision_archive";

/// one canonical UTC instant admitted by the shared time boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedRevisionUtcInstant(String);

impl AdmittedRevisionUtcInstant {
    /// admit one canonical instant without implementing another time codec.
    pub fn admitted(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// borrow the canonical instant.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// whether the caller supplied the optional archive integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherRevisionArchiveSelection {
    Disabled,
    Enabled,
}

/// one hash and identity serialized for the prior-row lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherRevisionLookupRequest {
    pub content_hash: Sha256Digest,
    pub product_run_at: Option<AdmittedRevisionUtcInstant>,
    pub source_kind: SourceKind,
    pub valid_at: AdmittedRevisionUtcInstant,
}

/// typed values for the prior-revision lookup in placeholder order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeatherRevisionSelectionBind {
    Text(String),
    Json(SerializedJson),
}

/// the exact prior-revision lookup and encoded request set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherRevisionSelectionQuery {
    pub sql: &'static str,
    pub binds: [WeatherRevisionSelectionBind; 2],
}

/// one existing serving identity returned by the prior-row lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExistingWeatherRevisionIdentity {
    pub content_hash: Sha256Digest,
    pub product_run_at: Option<AdmittedRevisionUtcInstant>,
    pub source_kind: SourceKind,
    pub valid_at: AdmittedRevisionUtcInstant,
}

/// one admitted revision whose content is new or changed.
#[derive(Debug, Clone, PartialEq)]
pub struct ChangedWeatherRevisionInput {
    pub content_hash: Sha256Digest,
    pub record: NormalizedWeatherRecord,
}

/// typed values for the source-lineage lookup in placeholder order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeatherRevisionContextBind {
    Text(String),
}

/// the exact source-lineage lookup for one ingestion run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherRevisionContextQuery {
    pub sql: &'static str,
    pub binds: [WeatherRevisionContextBind; 2],
}

/// source lineage decoded and normalized by the owning database adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherRevisionSourceContext {
    pub adapter_version: String,
    pub logical_received_at: AdmittedRevisionUtcInstant,
    pub provider_key: String,
    pub source_config_fingerprint: String,
    pub source_key: String,
}

/// one complete changed revision passed to the cold archive adapter.
#[derive(Debug, Clone, PartialEq)]
pub struct PersistedWeatherAdjustmentRevision {
    pub adapter_version: String,
    pub content_hash: Sha256Digest,
    pub logical_received_at: AdmittedRevisionUtcInstant,
    pub provider_key: String,
    pub record: NormalizedWeatherRecord,
    pub source_config_fingerprint: String,
    pub source_key: String,
}

/// the three commands that must execute on the retained transaction port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherRevisionSavepointCommand {
    Savepoint,
    RollbackTo,
    Release,
}

impl WeatherRevisionSavepointCommand {
    /// return the exact PostgreSQL command owned by the TypeScript repository.
    pub const fn sql(self) -> &'static str {
        // preserve the source savepoint command order and spelling
        match self {
            Self::Savepoint => SAVEPOINT_WEATHER_REVISION_ARCHIVE_SQL,
            Self::RollbackTo => ROLLBACK_WEATHER_REVISION_ARCHIVE_SQL,
            Self::Release => RELEASE_WEATHER_REVISION_ARCHIVE_SQL,
        }
    }
}

/// the database, encoding, stage, and publication boundary for one retained port.
pub trait WeatherRevisionArchivePort: RetainedSourceSession {
    type Error;
    type Publication;

    /// encode lookup requests with the existing JSON adapter.
    fn serialize_revision_requests(
        &mut self,
        requests: &[WeatherRevisionLookupRequest],
    ) -> Result<SerializedJson, Self::Error>;

    /// execute the exact prior-revision lookup.
    fn select_existing_weather_revisions(
        &mut self,
        query: &WeatherRevisionSelectionQuery,
    ) -> Result<Vec<ExistingWeatherRevisionIdentity>, Self::Error>;

    /// load the locked ingestion-run lineage.
    fn load_weather_revision_source_context(
        &mut self,
        query: &WeatherRevisionContextQuery,
    ) -> Result<Option<WeatherRevisionSourceContext>, Self::Error>;

    /// execute one savepoint command on this same retained port.
    fn execute_weather_revision_savepoint(
        &mut self,
        command: WeatherRevisionSavepointCommand,
    ) -> Result<(), Self::Error>;

    /// stage archive bytes and bindings before the caller commits.
    fn stage_weather_revisions(
        &mut self,
        revisions: &[PersistedWeatherAdjustmentRevision],
    ) -> Result<Option<Self::Publication>, Self::Error>;

    /// publish one staged revision group after the caller commits.
    fn publish_weather_revisions(
        &mut self,
        publication: Self::Publication,
    ) -> Result<(), Self::Error>;
}

/// one helper failure that remains observable to the owning transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WeatherRevisionArchiveError<CommandError> {
    MissingSourceContext,
    Command(CommandError),
}

impl<CommandError: Display> Display for WeatherRevisionArchiveError<CommandError> {
    /// render the exact missing-row error or the ordinary adapter error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        // retain ordinary command errors without relabeling them
        match self {
            Self::MissingSourceContext => {
                formatter.write_str("weather revision source context returned no row")
            }
            Self::Command(error) => Display::fmt(error, formatter),
        }
    }
}

impl<CommandError> Error for WeatherRevisionArchiveError<CommandError> where
    CommandError: Error + 'static
{
}

/// publications staged inside the still-open serving transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedWeatherRevisionPublications<Publication> {
    publications: Vec<Publication>,
}

impl<Publication> StagedWeatherRevisionPublications<Publication> {
    /// report whether the stage produced any cold publication work.
    pub fn is_empty(&self) -> bool {
        self.publications.is_empty()
    }

    /// admit publications only when the caller's serving commit succeeded.
    pub fn after_caller_commit<CommitError>(
        self,
        commit_result: Result<(), CommitError>,
    ) -> Result<CommittedWeatherRevisionPublications<Publication>, CommitError> {
        commit_result.map(|()| CommittedWeatherRevisionPublications {
            publications: self.publications,
        })
    }
}

/// staged publications admitted only after a successful caller commit result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedWeatherRevisionPublications<Publication> {
    publications: Vec<Publication>,
}

/// select only revisions introduced or content-changed by this transaction.
pub fn select_changed_weather_revision_inputs<Port: WeatherRevisionArchivePort>(
    selection: WeatherRevisionArchiveSelection,
    records: &[PreparedWeatherRecord],
    port: &mut Port,
) -> Result<Vec<ChangedWeatherRevisionInput>, WeatherRevisionArchiveError<Port::Error>> {
    // omit all archive lookup work when the optional archiver is absent
    if selection == WeatherRevisionArchiveSelection::Disabled {
        return Ok(Vec::new());
    }

    // avoid the empty jsonb recordset inference query
    if records.is_empty() {
        return Ok(Vec::new());
    }

    let requests = records
        .iter()
        .map(weather_revision_lookup_request)
        .collect::<Vec<_>>();
    let requested_json = port
        .serialize_revision_requests(&requests)
        .map_err(WeatherRevisionArchiveError::Command)?;
    let query = weather_revision_selection_query(port.source_id(), requested_json);
    let existing = port
        .select_existing_weather_revisions(&query)
        .map_err(WeatherRevisionArchiveError::Command)?;
    let mut prior = Vec::<(WeatherRevisionIdentity, Sha256Digest)>::new();

    // reproduce JavaScript Map replacement for duplicate database identities
    for row in existing {
        let identity = WeatherRevisionIdentity::from_existing(&row);

        // retain the final hash for one duplicated identity
        if let Some((_, content_hash)) = prior
            .iter_mut()
            .find(|(candidate, _)| *candidate == identity)
        {
            *content_hash = row.content_hash;
        } else {
            prior.push((identity, row.content_hash));
        }
    }

    let mut changed = Vec::new();

    // preserve caller order while omitting exact prior content matches
    for (prepared, request) in records.iter().zip(&requests) {
        let identity = WeatherRevisionIdentity::from_request(request);
        let previous = prior
            .iter()
            .find(|(candidate, _)| *candidate == identity)
            .map(|(_, content_hash)| content_hash);

        // never backfill an unchanged legacy null archive pointer
        if previous == Some(&request.content_hash) {
            continue;
        }

        changed.push(ChangedWeatherRevisionInput {
            content_hash: prepared.storage.content_hash.clone(),
            record: prepared.record.clone(),
        });
    }

    Ok(changed)
}

/// stage one bounded eligible normalized group before the caller commits.
pub fn stage_changed_weather_revisions<Port: WeatherRevisionArchivePort>(
    run_id: &str,
    changed: &[ChangedWeatherRevisionInput],
    port: &mut Port,
) -> Result<
    StagedWeatherRevisionPublications<Port::Publication>,
    WeatherRevisionArchiveError<Port::Error>,
> {
    // skip source context and savepoint work when no serving value changed
    if changed.is_empty() {
        return Ok(StagedWeatherRevisionPublications {
            publications: Vec::new(),
        });
    }

    let query = weather_revision_context_query(run_id, port.source_id());
    let context = port
        .load_weather_revision_source_context(&query)
        .map_err(WeatherRevisionArchiveError::Command)?
        .ok_or(WeatherRevisionArchiveError::MissingSourceContext)?;
    let revisions = changed
        .iter()
        .map(|revision| persisted_weather_revision(&context, revision))
        .collect::<Vec<_>>();

    port.execute_weather_revision_savepoint(WeatherRevisionSavepointCommand::Savepoint)
        .map_err(WeatherRevisionArchiveError::Command)?;

    let staged = port.stage_weather_revisions(&revisions);

    // recover stage failure and intentionally omit the archive publication
    let publication = match staged {
        Ok(publication) => publication,
        Err(_) => {
            recover_weather_revision_savepoint(port)?;
            return Ok(StagedWeatherRevisionPublications {
                publications: Vec::new(),
            });
        }
    };

    // treat release failure like any other archive failure after cleanup
    if port
        .execute_weather_revision_savepoint(WeatherRevisionSavepointCommand::Release)
        .is_err()
    {
        recover_weather_revision_savepoint(port)?;
        return Ok(StagedWeatherRevisionPublications {
            publications: Vec::new(),
        });
    }

    Ok(StagedWeatherRevisionPublications {
        publications: publication.into_iter().collect(),
    })
}

/// publish committed archive work while preserving ordinary ingestion success.
pub fn publish_committed_weather_revisions<Port: WeatherRevisionArchivePort>(
    committed: CommittedWeatherRevisionPublications<Port::Publication>,
    port: &mut Port,
) {
    // preserve every staged publication attempt after commit
    for publication in committed.publications {
        // ignore permanent cold publication gaps like the TypeScript owner
        let _publication_result = port.publish_weather_revisions(publication);
    }
}

/// build the exact prior-revision lookup.
pub fn weather_revision_selection_query(
    source_id: &str,
    requested_json: SerializedJson,
) -> WeatherRevisionSelectionQuery {
    WeatherRevisionSelectionQuery {
        sql: SELECT_EXISTING_WEATHER_REVISIONS_SQL,
        binds: [
            WeatherRevisionSelectionBind::Text(source_id.to_owned()),
            WeatherRevisionSelectionBind::Json(requested_json),
        ],
    }
}

/// build the exact ingestion-run lineage lookup.
pub fn weather_revision_context_query(
    run_id: &str,
    source_id: &str,
) -> WeatherRevisionContextQuery {
    WeatherRevisionContextQuery {
        sql: WEATHER_REVISION_SOURCE_CONTEXT_SQL,
        binds: [
            WeatherRevisionContextBind::Text(run_id.to_owned()),
            WeatherRevisionContextBind::Text(source_id.to_owned()),
        ],
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WeatherRevisionIdentity {
    product_run_at: Option<AdmittedRevisionUtcInstant>,
    source_kind: SourceKind,
    valid_at: AdmittedRevisionUtcInstant,
}

impl WeatherRevisionIdentity {
    /// project one lookup request into its nullable serving identity.
    fn from_request(request: &WeatherRevisionLookupRequest) -> Self {
        Self {
            product_run_at: request.product_run_at.clone(),
            source_kind: request.source_kind,
            valid_at: request.valid_at.clone(),
        }
    }

    /// project one decoded prior row into its nullable serving identity.
    fn from_existing(row: &ExistingWeatherRevisionIdentity) -> Self {
        Self {
            product_run_at: row.product_run_at.clone(),
            source_kind: row.source_kind,
            valid_at: row.valid_at.clone(),
        }
    }
}

/// derive one adapter-serialized lookup request from prepared storage evidence.
fn weather_revision_lookup_request(
    prepared: &PreparedWeatherRecord,
) -> WeatherRevisionLookupRequest {
    WeatherRevisionLookupRequest {
        content_hash: prepared.storage.content_hash.clone(),
        product_run_at: nullable_instant(&prepared.record.productRunAt),
        source_kind: prepared.record.sourceKind,
        valid_at: AdmittedRevisionUtcInstant::admitted(prepared.record.validAt.clone()),
    }
}

/// convert the generated nullable instant without adding a time codec.
fn nullable_instant(value: &Nullable<String>) -> Option<AdmittedRevisionUtcInstant> {
    // preserve the canonical null or value distinction
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(AdmittedRevisionUtcInstant::admitted(value.clone())),
    }
}

/// attach one locked source lineage context to a changed record.
fn persisted_weather_revision(
    context: &WeatherRevisionSourceContext,
    revision: &ChangedWeatherRevisionInput,
) -> PersistedWeatherAdjustmentRevision {
    PersistedWeatherAdjustmentRevision {
        adapter_version: context.adapter_version.clone(),
        content_hash: revision.content_hash.clone(),
        logical_received_at: context.logical_received_at.clone(),
        provider_key: context.provider_key.clone(),
        record: revision.record.clone(),
        source_config_fingerprint: context.source_config_fingerprint.clone(),
        source_key: context.source_key.clone(),
    }
}

/// recover an aborted archive operation on the same retained port.
fn recover_weather_revision_savepoint<Port: WeatherRevisionArchivePort>(
    port: &mut Port,
) -> Result<(), WeatherRevisionArchiveError<Port::Error>> {
    port.execute_weather_revision_savepoint(WeatherRevisionSavepointCommand::RollbackTo)
        .map_err(WeatherRevisionArchiveError::Command)?;
    port.execute_weather_revision_savepoint(WeatherRevisionSavepointCommand::Release)
        .map_err(WeatherRevisionArchiveError::Command)
}
