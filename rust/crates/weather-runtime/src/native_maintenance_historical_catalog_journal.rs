//! Read-only historical cold-catalog projection over the verified native maintenance journal.

use std::collections::HashSet;
use std::fmt::{self, Display, Formatter};

use weather_api::CurrentTimeResolver;
use weather_contracts::JsonValue;

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceHistoricalCatalogError,
    NativeMaintenanceHistoricalCatalogMapping, NativeMaintenanceHistoricalCatalogOutcome,
    NativeMaintenanceHistoricalSelection, NativeMaintenanceJournalRecord,
    NativeMaintenanceJournalStore, NativeMaintenanceJournalStoreError,
    NativeMaintenancePlaintextObjectStore, NativeRevisionColdCursor,
    VerifiedNativeRevisionColdStart, restore_native_maintenance_historical_catalog,
};

const GENESIS_KIND: &str = "lifecycle_genesis";
const CURSOR_KIND: &str = "revision_cursor_advanced";
const PACK_KIND: &str = "revision_custody_pack_sealed";
const LEGACY_CURSOR_FIELDS: &[&str] = &[
    "afterArchiveCommitOrdinal",
    "afterFrontierSha256",
    "graphManifestSha256",
    "nextArchiveCommitOrdinal",
    "nextFrontierSha256",
    "pageSha256",
    "startSha256",
];
const PACKED_CURSOR_FIELDS: &[&str] = &[
    "afterArchiveCommitOrdinal",
    "afterFrontierSha256",
    "custodyCheckpointSha256",
    "nextArchiveCommitOrdinal",
    "nextFrontierSha256",
    "pageSha256",
    "startSha256",
];
const PACK_FIELDS: &[&str] = &["checkpointSha256", "graphManifestSha256", "pages"];
const PACK_PAGE_FIELDS: &[&str] = &[
    "memberRootSha256",
    "pageSha256",
    "startMemberSha256",
    "startSha256",
];
const MAXIMUM_PACK_PAGES: usize = 19;

/// One coherent source projection from a single verified journal read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceHistoricalCatalogJournalState {
    pub cursor: Option<NativeRevisionColdCursor>,
    pub mappings: Vec<NativeMaintenanceHistoricalCatalogMapping>,
}

/// Read-only journal-store or closed semantic replay failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceHistoricalCatalogJournalError {
    Store(NativeMaintenanceJournalStoreError),
    State,
}

impl Display for NativeMaintenanceHistoricalCatalogJournalError {
    /// Render one stable source-owned failure category.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // retain the concrete verified-store cause when available
        match self {
            Self::Store(error) => Display::fmt(error, formatter),
            Self::State => formatter.write_str("historical catalog journal state is invalid"),
        }
    }
}

impl std::error::Error for NativeMaintenanceHistoricalCatalogJournalError {
    /// Preserve only a concrete verified-store source.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // semantic projection failures have no lower-level source
        match self {
            Self::Store(error) => Some(error),
            Self::State => None,
        }
    }
}

impl From<NativeMaintenanceJournalStoreError> for NativeMaintenanceHistoricalCatalogJournalError {
    /// Retain the actual journal-store read failure.
    fn from(error: NativeMaintenanceJournalStoreError) -> Self {
        Self::Store(error)
    }
}

/// Combined verified-journal and historical graph restoration failure.
#[derive(Debug)]
pub enum NativeMaintenanceInstalledHistoricalCatalogError {
    Journal(NativeMaintenanceHistoricalCatalogJournalError),
    Catalog(NativeMaintenanceHistoricalCatalogError),
}

impl Display for NativeMaintenanceInstalledHistoricalCatalogError {
    /// Render the exact owning operation failure.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        // delegate without exposing fixed filesystem paths
        match self {
            Self::Journal(error) => Display::fmt(error, formatter),
            Self::Catalog(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for NativeMaintenanceInstalledHistoricalCatalogError {
    /// Preserve the exact nested journal or catalog cause.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // both variants retain a concrete typed source
        match self {
            Self::Journal(error) => Some(error),
            Self::Catalog(error) => Some(error),
        }
    }
}

/// Concrete verified journal reader used by installed historical assembly.
pub struct NativeMaintenanceHistoricalCatalogJournal<'a> {
    store: &'a mut NativeMaintenanceJournalStore,
}

impl<'a> NativeMaintenanceHistoricalCatalogJournal<'a> {
    /// Borrow the already initialized native journal store.
    pub const fn new(store: &'a mut NativeMaintenanceJournalStore) -> Self {
        Self { store }
    }

    /// Read one coherent verified snapshot and derive cursor plus catalog mappings.
    pub fn read_state(
        &mut self,
        codec: &mut impl MaintenanceCanonicalJson,
        time: &dyn CurrentTimeResolver,
    ) -> Result<
        NativeMaintenanceHistoricalCatalogJournalState,
        NativeMaintenanceHistoricalCatalogJournalError,
    > {
        let records = self.store.read_verified(codec, time)?;
        project_native_maintenance_historical_catalog_journal(&records)
    }

    /// Read the source current revision cursor from one verified store snapshot.
    pub fn read_revision_cursor(
        &mut self,
        codec: &mut impl MaintenanceCanonicalJson,
        time: &dyn CurrentTimeResolver,
    ) -> Result<Option<NativeRevisionColdCursor>, NativeMaintenanceHistoricalCatalogJournalError>
    {
        Ok(self.read_state(codec, time)?.cursor)
    }

    /// List legacy and sealed-pack page mappings in exact source order.
    pub fn list_revision_catalog_pages(
        &mut self,
        codec: &mut impl MaintenanceCanonicalJson,
        time: &dyn CurrentTimeResolver,
    ) -> Result<
        Vec<NativeMaintenanceHistoricalCatalogMapping>,
        NativeMaintenanceHistoricalCatalogJournalError,
    > {
        Ok(self.read_state(codec, time)?.mappings)
    }

    /// Read one coherent journal state and restore it through the real plaintext object store.
    pub fn restore_catalog(
        &mut self,
        archive: &mut NativeMaintenancePlaintextObjectStore,
        start: VerifiedNativeRevisionColdStart,
        selection: NativeMaintenanceHistoricalSelection,
        codec: &mut impl MaintenanceCanonicalJson,
        time: &dyn CurrentTimeResolver,
    ) -> Result<
        NativeMaintenanceHistoricalCatalogOutcome,
        NativeMaintenanceInstalledHistoricalCatalogError,
    > {
        let state = self
            .read_state(codec, time)
            .map_err(NativeMaintenanceInstalledHistoricalCatalogError::Journal)?;
        let cursor = state
            .cursor
            .ok_or(NativeMaintenanceHistoricalCatalogJournalError::State)
            .map_err(NativeMaintenanceInstalledHistoricalCatalogError::Journal)?;
        restore_native_maintenance_historical_catalog(
            archive,
            cursor,
            start,
            state.mappings,
            selection,
        )
        .map_err(NativeMaintenanceInstalledHistoricalCatalogError::Catalog)
    }
}

/// Derive source cursor and mappings from an already verified append-only record population.
pub fn project_native_maintenance_historical_catalog_journal(
    records: &[NativeMaintenanceJournalRecord],
) -> Result<
    NativeMaintenanceHistoricalCatalogJournalState,
    NativeMaintenanceHistoricalCatalogJournalError,
> {
    use NativeMaintenanceHistoricalCatalogJournalError::State;

    let cursor = derive_cursor(records)?;
    let mut mappings = Vec::new();
    let mut pages = HashSet::new();
    // source ordering places all legacy individual mappings before sealed packs
    for record in records.iter().filter(|record| is_kind(record, CURSOR_KIND)) {
        // packed cursor advances remain pending until their later immutable seal
        if field(&record.payload, "graphManifestSha256").is_none() {
            validate_cursor_payload(&record.payload)?;
            continue;
        }
        let mapping = legacy_mapping(&record.payload)?;
        // refuse page reuse rather than expose an impossible source journal state
        if !pages.insert(mapping.page_sha256.clone()) {
            return Err(State);
        }
        mappings.push(mapping);
    }
    // append every sealed pack in journal order and every page in pack order
    for record in records.iter().filter(|record| is_kind(record, PACK_KIND)) {
        let pack = sealed_pack_mappings(&record.payload)?;
        // retain every pack member beside its shared immutable graph
        for mapping in pack {
            // preserve the mutation owner's global page uniqueness invariant
            if !pages.insert(mapping.page_sha256.clone()) {
                return Err(State);
            }
            mappings.push(mapping);
        }
    }
    Ok(NativeMaintenanceHistoricalCatalogJournalState { cursor, mappings })
}

/// Derive the latest append-only cursor or authenticated future-only genesis.
fn derive_cursor(
    records: &[NativeMaintenanceJournalRecord],
) -> Result<Option<NativeRevisionColdCursor>, NativeMaintenanceHistoricalCatalogJournalError> {
    use NativeMaintenanceHistoricalCatalogJournalError::State;

    let advanced = records
        .iter()
        .rev()
        .find(|record| is_kind(record, CURSOR_KIND));
    // prefer the most recent cursor even when its packed graph is not sealed yet
    if let Some(record) = advanced {
        validate_cursor_payload(&record.payload)?;
        return Ok(Some(NativeRevisionColdCursor {
            archive_commit_ordinal: required_ordinal(&record.payload, "nextArchiveCommitOrdinal")?,
            frontier_sha256: required_hash(&record.payload, "nextFrontierSha256")?,
        }));
    }
    let genesis = records.iter().find(|record| {
        is_kind(record, GENESIS_KIND)
            && text(&record.payload, "sourceKind").as_deref()
                == Some("future_only_verified_capture_epoch")
    });
    let Some(genesis) = genesis else {
        return Ok(None);
    };
    let witness = field(&genesis.payload, "witness").ok_or(State)?;
    Ok(Some(NativeRevisionColdCursor {
        archive_commit_ordinal: required_ordinal(witness, "archiveCommitOrdinal")?,
        frontier_sha256: required_hash(witness, "catalogFrontierSha256")?,
    }))
}

/// Validate either exact legacy or packed cursor transition payload.
fn validate_cursor_payload(
    value: &JsonValue,
) -> Result<(), NativeMaintenanceHistoricalCatalogJournalError> {
    use NativeMaintenanceHistoricalCatalogJournalError::State;

    let legacy = field(value, "graphManifestSha256").is_some();
    // select the exact closed payload shape from its proof discriminator
    let fields = if legacy {
        LEGACY_CURSOR_FIELDS
    } else {
        PACKED_CURSOR_FIELDS
    };
    // reject extension fields and require a strict monotonic source transition
    if !exact_keys(value, fields) {
        return Err(State);
    }
    let after = ordinal_value(value, "afterArchiveCommitOrdinal")?;
    let next = ordinal_value(value, "nextArchiveCommitOrdinal")?;
    // select the only proof field admitted by that closed shape
    let proof = if legacy {
        "graphManifestSha256"
    } else {
        "custodyCheckpointSha256"
    };
    // require every addressed cursor coordinate before projection
    if next <= after
        || [
            "afterFrontierSha256",
            proof,
            "nextFrontierSha256",
            "pageSha256",
            "startSha256",
        ]
        .into_iter()
        .any(|name| required_hash(value, name).is_err())
    {
        return Err(State);
    }
    Ok(())
}

/// Project one exact legacy individually archived cursor mapping.
fn legacy_mapping(
    value: &JsonValue,
) -> Result<NativeMaintenanceHistoricalCatalogMapping, NativeMaintenanceHistoricalCatalogJournalError>
{
    validate_cursor_payload(value)?;
    Ok(NativeMaintenanceHistoricalCatalogMapping {
        graph_manifest_sha256: required_hash(value, "graphManifestSha256")?,
        page_sha256: required_hash(value, "pageSha256")?,
        start_sha256: required_hash(value, "startSha256")?,
    })
}

/// Validate and project one immutable sealed pack in source page order.
fn sealed_pack_mappings(
    value: &JsonValue,
) -> Result<
    Vec<NativeMaintenanceHistoricalCatalogMapping>,
    NativeMaintenanceHistoricalCatalogJournalError,
> {
    use NativeMaintenanceHistoricalCatalogJournalError::State;

    let JsonValue::Array(pages) = field(value, "pages").ok_or(State)? else {
        return Err(State);
    };
    // retain the source fixed pack population and exact envelope
    if !exact_keys(value, PACK_FIELDS) || !(1..=MAXIMUM_PACK_PAGES).contains(&pages.len()) {
        return Err(State);
    }
    required_hash(value, "checkpointSha256")?;
    let graph_manifest_sha256 = required_hash(value, "graphManifestSha256")?;
    let mut seen = HashSet::with_capacity(pages.len());
    let mut mappings = Vec::with_capacity(pages.len());
    // preserve the open pack's exact page order under one graph identity
    for page in pages {
        // reject extension fields before reading any page identity
        if !exact_keys(page, PACK_PAGE_FIELDS) {
            return Err(State);
        }
        required_hash(page, "memberRootSha256")?;
        optional_hash(page, "startMemberSha256")?;
        let page_sha256 = required_hash(page, "pageSha256")?;
        let start_sha256 = required_hash(page, "startSha256")?;
        // reject duplicate page addresses inside one claimed immutable pack
        if !seen.insert(page_sha256.clone()) {
            return Err(State);
        }
        mappings.push(NativeMaintenanceHistoricalCatalogMapping {
            graph_manifest_sha256: graph_manifest_sha256.clone(),
            page_sha256,
            start_sha256,
        });
    }
    Ok(mappings)
}

/// Compare one verified journal event discriminator.
fn is_kind(record: &NativeMaintenanceJournalRecord, expected: &str) -> bool {
    matches!(&record.kind, JsonValue::String(units) if units.iter().copied().eq(expected.encode_utf16()))
}

/// Read one canonical uint64 text field while retaining its source spelling.
fn required_ordinal(
    value: &JsonValue,
    name: &str,
) -> Result<String, NativeMaintenanceHistoricalCatalogJournalError> {
    let value = text(value, name).ok_or(NativeMaintenanceHistoricalCatalogJournalError::State)?;
    canonical_ordinal(&value)
        .is_some()
        .then_some(value)
        .ok_or(NativeMaintenanceHistoricalCatalogJournalError::State)
}

/// Read one canonical uint64 field for monotonic comparison.
fn ordinal_value(
    value: &JsonValue,
    name: &str,
) -> Result<u64, NativeMaintenanceHistoricalCatalogJournalError> {
    text(value, name)
        .and_then(|value| canonical_ordinal(&value))
        .ok_or(NativeMaintenanceHistoricalCatalogJournalError::State)
}

/// Read one required lowercase content address.
fn required_hash(
    value: &JsonValue,
    name: &str,
) -> Result<String, NativeMaintenanceHistoricalCatalogJournalError> {
    let value = text(value, name).ok_or(NativeMaintenanceHistoricalCatalogJournalError::State)?;
    valid_hash(&value)
        .then_some(value)
        .ok_or(NativeMaintenanceHistoricalCatalogJournalError::State)
}

/// Read one nullable lowercase content address.
fn optional_hash(
    value: &JsonValue,
    name: &str,
) -> Result<Option<String>, NativeMaintenanceHistoricalCatalogJournalError> {
    match field(value, name) {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(units)) => {
            let value = String::from_utf16(units)
                .map_err(|_| NativeMaintenanceHistoricalCatalogJournalError::State)?;
            valid_hash(&value)
                .then_some(Some(value))
                .ok_or(NativeMaintenanceHistoricalCatalogJournalError::State)
        }
        _ => Err(NativeMaintenanceHistoricalCatalogJournalError::State),
    }
}

/// Parse one canonical uint64 decimal spelling.
fn canonical_ordinal(value: &str) -> Option<u64> {
    let parsed = value.parse::<u64>().ok()?;
    // preserve the predecessor's signed database bigint ceiling
    (parsed <= i64::MAX as u64 && parsed.to_string() == value).then_some(parsed)
}

#[cfg(test)]
#[path = "native_maintenance_historical_catalog_journal_test.rs"]
mod tests;
