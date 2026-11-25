//! Concrete owner-terminal tombstone publication and journal pointer retention.

use weather_contracts::JsonValue;
use weather_edge::{RevisionArchiveCanonicalCodec, RevisionGapSha256};

use crate::maintenance_canonical_json::{exact_keys, text, valid_hash};
use crate::native_maintenance_policy_comparison::{object, string};
use crate::native_maintenance_retirement_files_v2::format_digest;
use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceArchiveJournal,
    NativeMaintenanceArchiveJournalError, NativeMaintenanceArchiveLeaseRequest,
    NativeMaintenanceArchiveSegment, NativeMaintenanceArchiveSegmentArchive,
    NativeMaintenanceArchiveSegmentJournal, NativeMaintenanceArchiveSegmentMember,
    NativeMaintenanceCaptureFamily, NativeMaintenanceControllerArchiveValidators,
    NativeMaintenanceDueAttemptJournal, NativeMaintenanceDueTerminalRemotePort,
    NativeMaintenanceGraphEntry, NativeMaintenanceInstalledTerminalMaterial,
    NativeMaintenanceJournalRecord, NativeMaintenanceJournalStore,
    NativeMaintenanceJournalStoreError, NativeMaintenanceOwnedJournalAddition,
    NativeMaintenanceOwnerTerminalClass, NativeMaintenanceOwnerTerminalCompletion,
    NativeMaintenanceOwnerTerminalError, NativeMaintenanceOwnerTerminalInput,
    NativeMaintenanceOwnerTerminalMaterial, NativeMaintenanceOwnerTombstonePort,
    NativeMaintenancePlaintextArchive, NativeMaintenancePlaintextArchiveError,
    NativeMaintenanceRecordDueTerminalOutcome, NativeMaintenanceRecordDueTerminalRetirement,
    NativeWirePrimitives, build_native_maintenance_graph_manifest,
    build_native_maintenance_owner_terminal_material,
    reconcile_native_maintenance_due_terminal_retirement,
};

const TOMBSTONE_VERSION: &str = "adjustment-shadow-terminal-tombstone/v3";
const TOMBSTONE_JOURNAL_KIND: &str = "confirmation_owner_terminal_tombstone";

/// Exact concrete archive and journal failures for one terminal tombstone effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeMaintenanceTerminalTombstoneAdapterError {
    Input,
    Archive(NativeMaintenancePlaintextArchiveError),
    Journal(NativeMaintenanceArchiveJournalError),
    LeaseOccupied,
    ReconciliationRequired,
    CompletedMemberUnavailable,
    MissingJournalHead,
    Graph,
    HeadDiffers,
    JournalRecordCollision,
}

impl From<NativeMaintenanceJournalStoreError> for NativeMaintenanceTerminalTombstoneAdapterError {
    /// Preserve the actual verified journal storage failure.
    fn from(error: NativeMaintenanceJournalStoreError) -> Self {
        Self::Journal(NativeMaintenanceArchiveJournalError::Store(error))
    }
}

/// Actual installed terminal coordinates plus the owner-returned terminal authority.
pub struct NativeMaintenanceInstalledOwnerTerminalInput<'a> {
    pub installed: &'a NativeMaintenanceInstalledTerminalMaterial,
    pub authority_proof: &'a JsonValue,
    pub class: NativeMaintenanceOwnerTerminalClass,
    pub due_key: &'a str,
    pub now: &'a str,
    pub semantic_input_sha256: &'a str,
    pub terminal_record: &'a JsonValue,
}

/// Concrete fixed-root archive and the one verified controller journal.
pub struct NativeMaintenanceTerminalTombstoneAdapter<'a> {
    archive:
        &'a mut NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>,
    journal: &'a mut NativeMaintenanceJournalStore,
}

impl<'a> NativeMaintenanceTerminalTombstoneAdapter<'a> {
    /// Borrow the installed archive and journal without reopening either fixed owner.
    pub fn new(
        archive: &'a mut NativeMaintenancePlaintextArchive<
            NativeMaintenanceControllerArchiveValidators,
        >,
        journal: &'a mut NativeMaintenanceJournalStore,
    ) -> Self {
        Self { archive, journal }
    }
}

impl NativeMaintenanceOwnerTombstonePort for NativeMaintenanceTerminalTombstoneAdapter<'_> {
    type Error = NativeMaintenanceTerminalTombstoneAdapterError;

    /// Publish the exact single-member graph before retaining its family pointer.
    fn publish_and_record_tombstone(
        &mut self,
        family: NativeMaintenanceCaptureFamily,
        now: &str,
        tombstone_sha256: &str,
        tombstone_bytes: &[u8],
        tombstone: &JsonValue,
    ) -> Result<(), Self::Error> {
        let identity = admit_tombstone(now, tombstone_sha256, tombstone_bytes, tombstone)?;
        publish_terminal_tombstone_member(
            self.archive,
            self.journal,
            now,
            identity,
            tombstone_bytes,
        )?;
        record_terminal_tombstone_pointer(self.journal, family, now, tombstone)?;
        Ok(())
    }
}

/// Build terminal material only when it belongs to the actual installed family and registration.
pub fn build_native_maintenance_installed_owner_terminal_material(
    input: &NativeMaintenanceInstalledOwnerTerminalInput<'_>,
) -> Result<NativeMaintenanceOwnerTerminalMaterial, NativeMaintenanceOwnerTerminalError<()>> {
    use NativeMaintenanceOwnerTerminalError::Material;

    let (family, unsupported) = match &input.installed.assembly {
        crate::NativeMaintenanceDailyTerminalValues::Complete { family, .. } => (*family, false),
        crate::NativeMaintenanceDailyTerminalValues::Unsupported { family, .. } => (*family, true),
        crate::NativeMaintenanceDailyTerminalValues::Pending { .. } => return Err(Material),
    };
    let registration = &input
        .installed
        .access
        .shadow_registration
        .registration_sha256;
    // bind the owner result to the installed shadow slot rather than only its declared proof
    if (input.installed.access.unsupported && !unsupported)
        || (input.class == NativeMaintenanceOwnerTerminalClass::Unsupported) != unsupported
        || text(input.terminal_record, "family").as_deref() != Some(family.wire())
        || text(input.terminal_record, "registrationSha256").as_deref()
            != Some(registration.as_str())
        || text(
            &input.installed.access.shadow_registration.registration,
            "registrationSha256",
        )
        .as_deref()
            != Some(registration.as_str())
    {
        return Err(Material);
    }
    build_native_maintenance_owner_terminal_material(NativeMaintenanceOwnerTerminalInput {
        authority_proof: input.authority_proof,
        class: input.class,
        due_key: input.due_key,
        now: input.now,
        semantic_input_sha256: input.semantic_input_sha256,
        terminal_record: input.terminal_record,
    })
}

/// Run the concrete tombstone, due-retention, and forced-retirement sequence without aliasing the journal.
pub fn complete_native_maintenance_installed_owner_terminal(
    input: NativeMaintenanceInstalledOwnerTerminalInput<'_>,
    archive: &mut NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>,
    journal: &mut NativeMaintenanceJournalStore,
    remote: &mut impl NativeMaintenanceDueTerminalRemotePort,
) -> Result<
    NativeMaintenanceOwnerTerminalCompletion,
    NativeMaintenanceOwnerTerminalError<NativeMaintenanceTerminalTombstoneAdapterError>,
> {
    use NativeMaintenanceOwnerTerminalError as Error;

    let due_key = input.due_key.to_owned();
    let now = input.now.to_owned();
    let material = build_native_maintenance_installed_owner_terminal_material(&input)
        .map_err(|_| Error::Material)?;
    NativeMaintenanceTerminalTombstoneAdapter::new(archive, journal)
        .publish_and_record_tombstone(
            material.family,
            &now,
            &material.tombstone_sha256,
            &material.tombstone_bytes,
            &material.tombstone,
        )
        .map_err(Error::Archive)?;
    let mut due_journal = NativeMaintenanceDueAttemptJournal::new(journal);
    due_journal
        .record_due_terminal_outcome(NativeMaintenanceRecordDueTerminalOutcome {
            due_key: &due_key,
            now: &now,
            outcome: &material.outcome,
        })
        .map_err(Error::Journal)?;
    let retained = due_journal
        .record_due_terminal_retirement(NativeMaintenanceRecordDueTerminalRetirement {
            due_key: &due_key,
            kind: material.retirement_kind,
            now: &now,
            request: &material.retirement_request,
        })
        .map_err(Error::Journal)?;
    // preserve an already acknowledged retirement without resending its forced command
    let retirement = if retained.completed {
        retained
    } else {
        reconcile_native_maintenance_due_terminal_retirement(
            &mut due_journal,
            remote,
            &due_key,
            &now,
        )
        .map_err(Error::Remote)?
    };
    Ok(NativeMaintenanceOwnerTerminalCompletion {
        material,
        retirement,
    })
}

/// Validate the exact canonical tombstone and return its admitted binary identity.
fn admit_tombstone(
    now: &str,
    tombstone_sha256: &str,
    tombstone_bytes: &[u8],
    tombstone: &JsonValue,
) -> Result<[u8; 32], NativeMaintenanceTerminalTombstoneAdapterError> {
    use NativeMaintenanceTerminalTombstoneAdapterError::Input;

    const KEYS: &[&str] = &[
        "contractVersion",
        "reconciliationSha256",
        "registrationSha256",
        "terminalResultSha256",
    ];
    let mut wire = NativeWirePrimitives;
    // refuse any noncanonical body, caller hash drift, extension, or invalid pointer coordinate
    if wire.canonical_utc_milliseconds(now).is_none()
        || !exact_keys(tombstone, KEYS)
        || text(tombstone, "contractVersion").as_deref() != Some(TOMBSTONE_VERSION)
        || [
            "reconciliationSha256",
            "registrationSha256",
            "terminalResultSha256",
        ]
        .iter()
        .any(|key| text(tombstone, key).is_none_or(|value| !valid_hash(&value)))
        || wire.encode_canonical_json(tombstone).as_deref() != Some(tombstone_bytes)
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &wire,
            tombstone_bytes,
        )) != tombstone_sha256
    {
        return Err(Input);
    }
    RevisionGapSha256::from_lower_hex(tombstone_sha256)
        .map(|hash| *hash.as_bytes())
        .ok_or(Input)
}

/// Publish one exact predecessor-linked tombstone member using the source member lease identity.
fn publish_terminal_tombstone_member(
    archive: &mut NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>,
    store: &mut NativeMaintenanceJournalStore,
    now: &str,
    identity: [u8; 32],
    payload: &[u8],
) -> Result<[u8; 32], NativeMaintenanceTerminalTombstoneAdapterError> {
    use NativeMaintenanceTerminalTombstoneAdapterError as Error;

    let identity_hex = format_digest(identity);
    let due_key = format!("archive/member/{identity_hex}");
    let run_id = format!("archive-member-{identity_hex}");
    let segment = NativeMaintenanceArchiveSegment {
        members: vec![NativeMaintenanceArchiveSegmentMember {
            kind: TOMBSTONE_VERSION.to_owned(),
            identity_sha256: identity,
            payload: payload.to_vec(),
        }],
        cross_links: Vec::new(),
    };
    let mut journal = NativeMaintenanceArchiveJournal::new(store);
    let due = journal.due(&due_key).map_err(Error::Journal)?;
    let existing_head = archive.read_head().map_err(Error::Archive)?;
    // reuse a completed due only when its exact member remains reachable at its recorded graph
    if let Some(output) = due.output_sha256 {
        let existing = archive
            .find_member(existing_head, identity, TOMBSTONE_VERSION)
            .map_err(Error::Archive)?
            .filter(|head| *head == output)
            .ok_or(Error::CompletedMemberUnavailable)?;
        verify_existing_entry(archive, existing, identity, payload)?;
        return Ok(existing);
    }
    let status = journal.status().map_err(Error::Journal)?;
    // reconcile only this exact expired archive member lease against durable reachability
    if let Some(active) = status.active_lease {
        if active.due_key != due_key || active.run_id != run_id {
            return Err(Error::LeaseOccupied);
        }
        let expires = NativeWirePrimitives
            .canonical_utc_milliseconds(&active.expires_at)
            .ok_or(Error::Input)?;
        let instant = NativeWirePrimitives
            .canonical_utc_milliseconds(now)
            .ok_or(Error::Input)?;
        if expires <= instant {
            let existing = archive
                .find_member(existing_head, identity, TOMBSTONE_VERSION)
                .map_err(Error::Archive)?
                .ok_or(Error::ReconciliationRequired)?;
            verify_existing_entry(archive, existing, identity, payload)?;
            journal
                .reconcile_complete(&due_key, &run_id, now, existing, existing_head)
                .map_err(Error::Journal)?;
        }
    }
    let refreshed = journal.status().map_err(Error::Journal)?;
    let input_head = refreshed.head_sha256.ok_or(Error::MissingJournalHead)?;
    journal
        .acquire(NativeMaintenanceArchiveLeaseRequest {
            due_key: &due_key,
            run_id: &run_id,
            now,
            input_head_sha256: due.input_head_sha256.unwrap_or(input_head),
        })
        .map_err(Error::Journal)?;
    let result = publish_acquired_tombstone(
        archive,
        &mut journal,
        now,
        &due_key,
        &run_id,
        identity,
        payload,
        &segment,
    );
    // release only this same-process lease after a recoverable publication failure
    if result.is_err() {
        let _ = journal.release(&due_key, &run_id, now);
    }
    result
}

/// Publish the CAS member and rotate the fully verified graph head under an acquired lease.
#[allow(clippy::too_many_arguments)]
fn publish_acquired_tombstone(
    archive: &mut NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>,
    journal: &mut NativeMaintenanceArchiveJournal<'_>,
    now: &str,
    due_key: &str,
    run_id: &str,
    identity: [u8; 32],
    payload: &[u8],
    segment: &NativeMaintenanceArchiveSegment,
) -> Result<[u8; 32], NativeMaintenanceTerminalTombstoneAdapterError> {
    use NativeMaintenanceTerminalTombstoneAdapterError as Error;

    let published = NativeMaintenanceArchiveSegmentArchive::publish_cas_object(archive, segment)
        .map_err(Error::Archive)?;
    let [member] = published.members.as_slice() else {
        return Err(Error::Graph);
    };
    let expected = graph_entry(published.object_sha256, member);
    // require the measured retained member to be the requested canonical tombstone
    if expected.kind != TOMBSTONE_VERSION
        || expected.identity_sha256 != format_digest(identity)
        || expected.member_sha256 != format_digest(identity)
        || expected.member_length != payload.len() as u64
    {
        return Err(Error::Graph);
    }
    let current_head = archive.read_head().map_err(Error::Archive)?;
    let existing = archive
        .find_member(current_head, identity, TOMBSTONE_VERSION)
        .map_err(Error::Archive)?;
    let head = if let Some(existing) = existing {
        let actual = verify_existing_entry(archive, existing, identity, payload)?;
        // never reuse an identity whose retained physical member location differs
        if actual != expected {
            return Err(Error::Graph);
        }
        existing
    } else {
        let manifest = build_native_maintenance_graph_manifest(
            current_head.map(format_digest),
            vec![expected],
            Vec::new(),
        )
        .ok_or(Error::Graph)?;
        let head =
            NativeMaintenanceArchiveSegmentArchive::publish_graph_manifest(archive, &manifest)
                .map_err(Error::Archive)?;
        archive
            .verify_full_graph(head, true)
            .map_err(Error::Archive)?;
        archive.update_head(head).map_err(Error::Archive)?;
        // require pointer durability before completing the journal due
        if archive.read_head().map_err(Error::Archive)? != Some(head) {
            return Err(Error::HeadDiffers);
        }
        head
    };
    journal
        .complete(due_key, now, head)
        .map_err(Error::Journal)?;
    journal
        .release(due_key, run_id, now)
        .map_err(Error::Journal)?;
    Ok(head)
}

/// Rebind one reachable tombstone graph to the exact expected member metadata and bytes.
fn verify_existing_entry(
    archive: &mut NativeMaintenancePlaintextArchive<NativeMaintenanceControllerArchiveValidators>,
    graph: [u8; 32],
    identity: [u8; 32],
    payload: &[u8],
) -> Result<NativeMaintenanceGraphEntry, NativeMaintenanceTerminalTombstoneAdapterError> {
    use NativeMaintenanceTerminalTombstoneAdapterError as Error;

    let identity = format_digest(identity);
    let verified = archive
        .verify_full_graph(graph, false)
        .map_err(Error::Archive)?;
    let entry = verified
        .manifest
        .entries
        .iter()
        .find(|entry| entry.identity_sha256 == identity && entry.kind == TOMBSTONE_VERSION)
        .cloned()
        .ok_or(Error::CompletedMemberUnavailable)?;
    // the source identity and member checksum are the canonical tombstone bytes
    if entry.member_sha256 != identity
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &NativeWirePrimitives,
            payload,
        )) != identity
    {
        return Err(Error::Graph);
    }
    Ok(entry)
}

/// Project one source graph entry from the actual retained CAS inventory.
fn graph_entry(
    object_sha256: [u8; 32],
    member: &crate::NativeMaintenanceCasMemberInventory,
) -> NativeMaintenanceGraphEntry {
    NativeMaintenanceGraphEntry {
        kind: member.kind.clone(),
        identity_sha256: format_digest(member.identity_sha256),
        object_sha256: format_digest(object_sha256),
        member_offset: member.member_offset,
        member_length: member.member_length,
        member_sha256: format_digest(member.member_sha256),
    }
}

/// Append or exactly reuse the family-scoped owner tombstone pointer after archive durability.
fn record_terminal_tombstone_pointer(
    store: &mut NativeMaintenanceJournalStore,
    family: NativeMaintenanceCaptureFamily,
    now: &str,
    tombstone: &JsonValue,
) -> Result<(), NativeMaintenanceTerminalTombstoneAdapterError> {
    use NativeMaintenanceTerminalTombstoneAdapterError as Error;

    let registration = text(tombstone, "registrationSha256").ok_or(Error::Input)?;
    let due_key = format!("confirmation/{}/{registration}", family.wire());
    let record_key = format!(
        "confirmation-owner-terminal/{}/{}",
        family.wire(),
        registration
    );
    let payload = object(vec![
        ("family", string(family.wire())),
        ("tombstone", tombstone.clone()),
    ]);
    store.mutate_checked(
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
        |records| {
            let existing = records
                .iter()
                .find(|record| record.record_key == record_key);
            // preserve exact immutable retry semantics for the source logical record key
            if let Some(existing) = existing {
                if !is_kind(existing, TOMBSTONE_JOURNAL_KIND)
                    || existing.due_key.as_deref() != Some(due_key.as_str())
                    || existing.payload_sha256 != payload_identity(&payload)?
                {
                    return Err(Error::JournalRecordCollision);
                }
                return Ok(Vec::new());
            }
            Ok(vec![NativeMaintenanceOwnedJournalAddition {
                due_key: Some(due_key),
                kind: TOMBSTONE_JOURNAL_KIND.to_owned(),
                payload,
                record_key,
                recorded_at: now.to_owned(),
            }])
        },
    )
}

/// Compare one retained journal kind without accepting non-string aliases.
fn is_kind(record: &NativeMaintenanceJournalRecord, expected: &str) -> bool {
    matches!(&record.kind, JsonValue::String(units) if units.iter().copied().eq(expected.encode_utf16()))
}

/// Hash one exact canonical journal payload for collision-safe idempotence.
fn payload_identity(
    payload: &JsonValue,
) -> Result<String, NativeMaintenanceTerminalTombstoneAdapterError> {
    let mut wire = NativeWirePrimitives;
    let bytes = wire
        .encode_canonical_json(payload)
        .ok_or(NativeMaintenanceTerminalTombstoneAdapterError::Graph)?;
    MaintenanceCanonicalJson::sha256(&mut wire, &bytes)
        .map(|hash| hash.as_str().to_owned())
        .ok_or(NativeMaintenanceTerminalTombstoneAdapterError::Graph)
}
