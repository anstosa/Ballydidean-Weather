//! Candidate custody graph assembled from the actual fit, package parity and rolling registration.

use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec, RevisionGapSha256};

use crate::{
    MaintenanceCanonicalJson, NativeMaintenanceArchiveJournal, NativeMaintenanceArchiveSegment,
    NativeMaintenanceArchiveSegmentArchive, NativeMaintenanceArchiveSegmentClock,
    NativeMaintenanceArchiveSegmentMember, NativeMaintenanceCandidateParityMaterial,
    NativeMaintenanceCaptureFamily, NativeMaintenanceFitResult, NativeMaintenanceGraphCrossLink,
    NativeMaintenanceJournalStore, NativeMaintenancePersistedCandidate,
    NativeMaintenancePostFitRegistration, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness,
    capture_epoch_witness::verify_operator_capture_epoch_witness,
    maintenance_canonical_json::{text, valid_hash},
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
    publish_native_maintenance_archive_segment,
};

/// Borrowed complete candidate graph source inputs, retaining original fit and witness bytes.
pub struct NativeMaintenanceCandidateArchiveInput<'a> {
    pub family: NativeMaintenanceCaptureFamily,
    pub fit: &'a NativeMaintenanceFitResult,
    pub parity: &'a NativeMaintenanceCandidateParityMaterial,
    pub registration: &'a NativeMaintenancePostFitRegistration,
    pub witness: &'a VerifiedNativeMaintenanceCaptureEpochWitness,
    pub witness_bytes: &'a [u8],
}

/// Source-exact closed graph material and its independent registration identities.
pub struct NativeMaintenanceCandidateArchiveSegment {
    pub due_key: String,
    pub segment: NativeMaintenanceArchiveSegment,
    pub artifact_sha256: String,
    pub parity_sha256: String,
    pub registration_sha256: String,
    pub source_sha256: String,
}

/// Validate cross-bound identities and build the original nine-member candidate custody segment.
pub fn build_native_maintenance_candidate_archive_segment(
    input: NativeMaintenanceCandidateArchiveInput<'_>,
) -> Option<NativeMaintenanceCandidateArchiveSegment> {
    let wire = NativeWirePrimitives;
    let registration = &input.registration;
    let portable = &input.parity.portable;
    let parity = &input.parity.receipt;
    let candidate = &input.fit.candidate_sha256;
    let artifact = &portable.artifact_sha256;
    let source = &registration.source_sha256;
    let shadow = &registration.shadow.registration_sha256;
    let policy = &registration.policy.policy_sha256;
    let cohort = &registration.cohort.cohort_sha256;
    let expected_plan = &registration.expected_plan.expected_plan_sha256;
    let epoch = &input.witness.witness_sha256;
    // independently require immutable identities before assembling a value-bearing candidate graph
    if input.fit.family != input.family
        || candidate != &portable.candidate_sha256
        || ![
            candidate,
            artifact,
            source,
            shadow,
            policy,
            cohort,
            expected_plan,
            epoch,
            &parity.parity_sha256,
        ]
        .into_iter()
        .all(|value| valid_hash(value))
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &wire,
            &input.fit.candidate_json,
        )) != *candidate
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &wire,
            &registration.source_bytes,
        )) != *source
        || format_digest(RevisionArchiveCanonicalCodec::sha256(&wire, &parity.bytes))
            != parity.parity_sha256
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &wire,
            &registration.policy.bytes,
        )) != *policy
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &wire,
            &registration.cohort.bytes,
        )) != *cohort
        || format_digest(RevisionArchiveCanonicalCodec::sha256(
            &wire,
            &registration.expected_plan.bytes,
        )) != *expected_plan
    {
        return None;
    }
    let verified_witness = verify_operator_capture_epoch_witness(
        input.witness_bytes,
        &mut NativeWirePrimitives,
        &NativeWirePrimitives,
    )
    .ok()?;
    let witness = NativeWirePrimitives.parse_canonical_object(input.witness_bytes)?;
    if text(&witness, "witnessSha256")? != *epoch
        || verified_witness.witness.witnessSha256 != *epoch
    {
        return None;
    }
    let shadow_registration = &registration.shadow.registration;
    for (key, identity) in [
        ("candidateSha256", candidate),
        ("artifactSha256", artifact),
        ("policySha256", policy),
        ("cohortSha256", cohort),
        (
            "reservedKeySha256",
            &registration.expected_plan.reserved_key_sha256,
        ),
        ("sourceSha256", source),
        ("epochWitnessSha256", epoch),
        ("registrationSha256", shadow),
    ] {
        if text(shadow_registration, key)? != *identity {
            return None;
        }
    }
    if text(&registration.source_descriptor, "sourceIdentitySha256")?
        != portable.source_identity_sha256
        || text(&registration.source_descriptor, "epochWitnessSha256")? != *epoch
        || text(&registration.source_descriptor, "family")? != input.family.wire()
        || text(&parity.fixture, "candidateSha256")? != *candidate
    {
        return None;
    }
    let artifact_document =
        NativeWirePrimitives.parse_canonical_object(&portable.artifact_bytes)?;
    let artifact_kind = text(&artifact_document, "contractVersion")?;
    // retain exact raw candidate and portable artifact bytes rather than reserializing the package
    let candidate_kind = format!(
        "forecast-adjustment-{}-development-candidate/v1",
        input.family.wire()
    );
    let shadow_bytes = wire.encode_sorted_json(shadow_registration)?;
    let members = vec![
        member(&candidate_kind, candidate, &input.fit.candidate_json)?,
        member(&artifact_kind, artifact, &portable.artifact_bytes)?,
        member(
            &text(&parity.fixture, "contractVersion")?,
            &parity.parity_sha256,
            &parity.bytes,
        )?,
        member(
            "forecast-adjustment-future-only-source-lineage/v1",
            source,
            &registration.source_bytes,
        )?,
        member(
            "adjustment-registration-policy/v3",
            policy,
            &registration.policy.bytes,
        )?,
        member(
            "adjustment-registration-cohort/v3",
            cohort,
            &registration.cohort.bytes,
        )?,
        member(
            "adjustment-registration-expected-key-plan/v3",
            expected_plan,
            &registration.expected_plan.bytes,
        )?,
        member("adjustment-shadow-registration/v3", shadow, &shadow_bytes)?,
        member(
            "adjustment-revision-capture-epoch-witness/v1",
            epoch,
            input.witness_bytes,
        )?,
    ];
    let links = vec![
        link(candidate, "binds_portable_artifact", artifact),
        link(candidate, "binds_runtime_parity", &parity.parity_sha256),
        link(artifact, "binds_future_only_source", source),
        link(candidate, "binds_shadow_registration", shadow),
        link(shadow, "binds_registration_policy", policy),
        link(shadow, "binds_registration_cohort", cohort),
        link(shadow, "binds_reserved_key_plan", expected_plan),
        link(shadow, "binds_future_only_source", source),
        link(source, "binds_capture_epoch", epoch),
    ];
    Some(NativeMaintenanceCandidateArchiveSegment {
        due_key: format!(
            "archive/development-candidate/{}/{candidate}",
            input.family.wire()
        ),
        segment: NativeMaintenanceArchiveSegment {
            members,
            cross_links: links,
        },
        artifact_sha256: artifact.clone(),
        parity_sha256: parity.parity_sha256.clone(),
        registration_sha256: shadow.clone(),
        source_sha256: source.clone(),
    })
}

/// Publish the actual source candidate graph with durable archive due completion before preregistration.
pub fn persist_native_maintenance_candidate<Archive, Clock>(
    input: NativeMaintenanceCandidateArchiveInput<'_>,
    archive: &mut Archive,
    journal: &mut NativeMaintenanceJournalStore,
    clock: &mut Clock,
) -> Option<NativeMaintenancePersistedCandidate>
where
    Archive: NativeMaintenanceArchiveSegmentArchive,
    Clock: NativeMaintenanceArchiveSegmentClock,
{
    let prepared = build_native_maintenance_candidate_archive_segment(input)?;
    let publication = publish_native_maintenance_archive_segment(
        archive,
        &mut NativeMaintenanceArchiveJournal::new(journal),
        clock,
        &prepared.due_key,
        &prepared.segment,
    )
    .ok()?;
    let candidate_graph_sha256 = format_digest(publication.manifest_object_sha256);
    let value = object(vec![
        ("artifactSha256", string(&prepared.artifact_sha256)),
        ("candidateGraphSha256", string(&candidate_graph_sha256)),
        ("paritySha256", string(&prepared.parity_sha256)),
        ("registrationSha256", string(&prepared.registration_sha256)),
        ("sourceSha256", string(&prepared.source_sha256)),
    ]);
    Some(NativeMaintenancePersistedCandidate {
        candidate_graph_sha256,
        value,
    })
}

/// Construct one content-addressed member while retaining the original producer bytes.
fn member(
    kind: &str,
    identity: &str,
    bytes: &[u8],
) -> Option<NativeMaintenanceArchiveSegmentMember> {
    Some(NativeMaintenanceArchiveSegmentMember {
        kind: kind.to_owned(),
        identity_sha256: *RevisionGapSha256::from_lower_hex(identity)?.as_bytes(),
        payload: bytes.to_vec(),
    })
}

/// Build one exact semantic cross-link in the graph identity set.
fn link(from: &str, relation: &str, to: &str) -> NativeMaintenanceGraphCrossLink {
    NativeMaintenanceGraphCrossLink {
        from_identity_sha256: from.to_owned(),
        relation: relation.to_owned(),
        to_identity_sha256: to.to_owned(),
    }
}
