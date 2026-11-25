use weather_api::{CanonicalInstant, CurrentProjectionError, CurrentTimeResolver};
use weather_contracts::JsDateOrString;

use crate::native_maintenance_capture_readiness::{
    NativeMaintenanceCaptureArchiveStatus, NativeMaintenanceCaptureFamily,
    NativeMaintenanceCaptureInstalledReleaseAuthority, NativeMaintenanceCaptureReadiness,
    NativeMaintenanceCaptureReadinessPorts, NativeMaintenanceCaptureReleaseGeneration,
    NativeMaintenanceCaptureSourceAuthority, VerifiedNativeMaintenanceCaptureEpochSnapshot,
    VerifiedNativeMaintenanceCaptureEpochWitness, VerifiedNativeMaintenanceDatabaseLedger,
    VerifiedNativeMaintenanceFamilyCurrent, VerifiedNativeMaintenanceFamilyLineage,
    VerifiedNativeMaintenanceInstalledRelease, VerifiedNativeMaintenanceLocalArchiveReadiness,
    inspect_native_maintenance_capture_readiness,
};

const SOURCE_COMMIT: &str = "1111111111111111111111111111111111111111";
const SUCCESSOR_COMMIT: &str = "2222222222222222222222222222222222222222";
const MANIFEST_SHA256: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const CONTROL_SHA256: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const WITNESS_SHA256: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const FRONTIER_SHA256: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SNAPSHOT_SHA256: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const LIVE_HISTORY_SHA256: &str =
    "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const CAPTURE_HISTORY_SHA256: &str =
    "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424";
const EPOCH_AT: &str = "2026-10-08T12:00:00.000Z";
const SNAPSHOT_AT: &str = "2026-10-08T12:00:01.000Z";
const SERVER_IMAGE: &str =
    "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Failure {
    None,
    Installed,
    Fit,
    Local,
    Database,
    Witness,
    Snapshot,
    Current,
    Lineage,
}

#[derive(Clone)]
struct Fixture {
    failure: Failure,
    generation: NativeMaintenanceCaptureReleaseGeneration,
    installed_commit: String,
    current_commit: String,
    current_release: String,
    rain_commit: Option<String>,
    snapshot_ordinal: String,
}

impl Fixture {
    /// Build one complete valid source observation.
    fn valid() -> Self {
        Self {
            failure: Failure::None,
            generation: NativeMaintenanceCaptureReleaseGeneration::NodePredecessor,
            installed_commit: SOURCE_COMMIT.to_owned(),
            current_commit: SOURCE_COMMIT.to_owned(),
            current_release: "2026.10.08-1".to_owned(),
            rain_commit: None,
            snapshot_ordinal: "0".to_owned(),
        }
    }
}

impl NativeMaintenanceCaptureInstalledReleaseAuthority for Fixture {
    type Error = ();

    /// Return the fixture closure or the selected read failure.
    fn read_verified_installed_release(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceInstalledRelease, Self::Error> {
        // isolate installed closure failure from later authority reads
        if self.failure == Failure::Installed {
            return Err(());
        }
        Ok(VerifiedNativeMaintenanceInstalledRelease {
            generation: self.generation,
            manifest_sha256: MANIFEST_SHA256.to_owned(),
            member_count: 2,
            source_commit: self.installed_commit.clone(),
        })
    }
}

impl NativeMaintenanceCaptureReadinessPorts for Fixture {
    type Error = ();

    /// Return only the selected fit-runtime failure.
    fn verify_fit_runtime(&mut self) -> Result<(), Self::Error> {
        (self.failure != Failure::Fit).then_some(()).ok_or(())
    }

    /// Return the exact ready local archive envelope.
    fn read_local_archive_readiness(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceLocalArchiveReadiness, Self::Error> {
        // isolate local archive failure from its ordinary ready status
        if self.failure == Failure::Local {
            return Err(());
        }
        Ok(VerifiedNativeMaintenanceLocalArchiveReadiness {
            contract_version: "adjustment-archive-local-readiness/v1".to_owned(),
            ready: true,
            reason: "ready".to_owned(),
            status: Some(NativeMaintenanceCaptureArchiveStatus {
                allocated_bytes: "0".to_owned(),
                file_count: 0,
                incoming_count: 0,
                object_count: 0,
                task_inodes: "0".to_owned(),
            }),
        })
    }

    /// Return one already verified live database ledger.
    fn read_verified_database_ledger(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceDatabaseLedger, Self::Error> {
        // isolate database failure from subsequent epoch reads
        if self.failure == Failure::Database {
            return Err(());
        }
        Ok(VerifiedNativeMaintenanceDatabaseLedger {
            contract_version: "adjustment-database-ledger/v3".to_owned(),
            migration_history_sha256: LIVE_HISTORY_SHA256.to_owned(),
            snapshot_at: SNAPSHOT_AT.to_owned(),
        })
    }

    /// Return the exact retained capture epoch witness.
    fn read_verified_capture_epoch_witness(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceCaptureEpochWitness, Self::Error> {
        // isolate witness failure from snapshot admission
        if self.failure == Failure::Witness {
            return Err(());
        }
        Ok(VerifiedNativeMaintenanceCaptureEpochWitness {
            catalog_frontier_sha256: FRONTIER_SHA256.to_owned(),
            control_plane_sha256: CONTROL_SHA256.to_owned(),
            control_plane_version: "14".to_owned(),
            database_migration_history_sha256: CAPTURE_HISTORY_SHA256.to_owned(),
            epoch_at: EPOCH_AT.to_owned(),
            serving_snapshot_sha256: SNAPSHOT_SHA256.to_owned(),
            source_commit: SOURCE_COMMIT.to_owned(),
            source_release: "2026.10.08-1".to_owned(),
            source_server_image_digest: SERVER_IMAGE.to_owned(),
            witness_sha256: WITNESS_SHA256.to_owned(),
        })
    }

    /// Return the exact retained empty capture snapshot.
    fn read_verified_capture_epoch_snapshot(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceCaptureEpochSnapshot, Self::Error> {
        // isolate snapshot failure from its semantic mismatch fixture
        if self.failure == Failure::Snapshot {
            return Err(());
        }
        Ok(VerifiedNativeMaintenanceCaptureEpochSnapshot {
            archive_commit_ordinal: self.snapshot_ordinal.clone(),
            cutoff_at: EPOCH_AT.to_owned(),
            entry_count: 0,
            entries_empty: true,
            frontier_sha256: FRONTIER_SHA256.to_owned(),
            snapshot_sha256: SNAPSHOT_SHA256.to_owned(),
        })
    }
}

impl NativeMaintenanceCaptureSourceAuthority for Fixture {
    type Error = ();

    /// Return one exact family current observation.
    fn read_verified_family_current(
        &mut self,
        family: NativeMaintenanceCaptureFamily,
    ) -> Result<VerifiedNativeMaintenanceFamilyCurrent, Self::Error> {
        // refuse the first family read when the source authority is unavailable
        if self.failure == Failure::Current {
            return Err(());
        }
        let commit = if family == NativeMaintenanceCaptureFamily::Rain {
            self.rain_commit
                .clone()
                .unwrap_or_else(|| self.current_commit.clone())
        } else {
            self.current_commit.clone()
        };
        Ok(VerifiedNativeMaintenanceFamilyCurrent {
            commit,
            family: family.wire().to_owned(),
            release: self.current_release.clone(),
            source_server_image_digest: SERVER_IMAGE.to_owned(),
        })
    }

    /// Return the exact current-to-epoch lineage projection.
    fn read_verified_family_lineage(
        &mut self,
    ) -> Result<VerifiedNativeMaintenanceFamilyLineage, Self::Error> {
        // isolate lineage failure after all three current observations
        if self.failure == Failure::Lineage {
            return Err(());
        }
        Ok(VerifiedNativeMaintenanceFamilyLineage {
            contract_version: "adjustment-family-release-lineage/v2".to_owned(),
            control_sha256: CONTROL_SHA256.to_owned(),
            control_version: "14".to_owned(),
            current_commit: self.current_commit.clone(),
            current_release: self.current_release.clone(),
            epoch_ancestor_commit: SOURCE_COMMIT.to_owned(),
            epoch_witness_sha256: WITNESS_SHA256.to_owned(),
            state: "verified_epoch_descendant".to_owned(),
            verified_at: SNAPSHOT_AT.to_owned(),
        })
    }
}

struct FixtureTime;

impl CurrentTimeResolver for FixtureTime {
    /// Resolve only the two canonical fixture instants.
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        let JsDateOrString::String(units) = value else {
            return Err(time_error());
        };
        let utc = String::from_utf16(units).map_err(|_| time_error())?;
        let epoch_milliseconds = match utc.as_str() {
            EPOCH_AT => 1_791_460_800_000,
            SNAPSHOT_AT => 1_791_460_801_000,
            _ => return Err(time_error()),
        };
        Ok(CanonicalInstant {
            epoch_milliseconds,
            utc,
        })
    }
}

/// Return one stable fixture-only time failure.
fn time_error() -> CurrentProjectionError {
    CurrentProjectionError {
        code: "fixture.time",
        detail: "invalid fixture time".to_owned(),
    }
}

/// Run the pure projection while satisfying its three disjoint authority borrows.
fn inspect(fixture: &Fixture) -> NativeMaintenanceCaptureReadiness {
    let mut installed = fixture.clone();
    let mut ports = fixture.clone();
    let mut source = fixture.clone();
    inspect_native_maintenance_capture_readiness(
        &mut installed,
        &mut ports,
        &mut source,
        &FixtureTime,
    )
}

/// Preserve every admitted source identity in the ready projection.
#[test]
fn capture_readiness_accepts_the_complete_verified_authority_set() {
    let result = inspect(&Fixture::valid());
    assert!(result.ready);
    assert_eq!(result.reason, "ready");
    assert_eq!(
        result.contract_version,
        "adjustment-maintenance-capture-readiness/v1"
    );
    assert_eq!(
        result.release_manifest_sha256.as_deref(),
        Some(MANIFEST_SHA256)
    );
    assert_eq!(
        result.database_migration_history_sha256.as_deref(),
        Some(LIVE_HISTORY_SHA256),
    );
    assert_eq!(result.source_commit.as_deref(), Some(SOURCE_COMMIT));
    assert_eq!(result.source_release.as_deref(), Some("2026.10.08-1"));
    assert_eq!(
        result
            .archive_status
            .expect("archive status")
            .allocated_bytes,
        "0"
    );
}

/// Admit an authenticated family-only successor while retaining the epoch runner closure.
#[test]
fn capture_readiness_accepts_a_verified_epoch_descendant_release() {
    let mut fixture = Fixture::valid();
    fixture.current_commit = SUCCESSOR_COMMIT.to_owned();
    fixture.current_release = "2026.10.09-1".to_owned();
    let result = inspect(&fixture);
    assert!(result.ready);
    assert_eq!(result.source_commit.as_deref(), Some(SOURCE_COMMIT));
    assert_eq!(result.source_release.as_deref(), Some("2026.10.09-1"));
}

/// Keep every source first-failure category stable and path-free.
#[test]
fn capture_readiness_preserves_source_refusal_categories() {
    for (failure, reason) in [
        (Failure::Installed, "installed_release_unavailable"),
        (Failure::Fit, "fit_runtime_unavailable"),
        (Failure::Local, "local_archive_unready"),
        (Failure::Database, "database_ledger_unavailable"),
        (Failure::Witness, "capture_epoch_unavailable"),
        (Failure::Snapshot, "capture_epoch_unavailable"),
        (Failure::Current, "current_release_unavailable"),
        (Failure::Lineage, "current_lineage_unavailable"),
    ] {
        let mut fixture = Fixture::valid();
        fixture.failure = failure;
        let result = inspect(&fixture);
        assert!(!result.ready, "{failure:?}");
        assert_eq!(result.reason, reason, "{failure:?}");
    }
}

/// Refuse native substitution, epoch drift, and inconsistent live family releases.
#[test]
fn capture_readiness_refuses_cross_generation_or_cross_authority_drift() {
    let mut native = Fixture::valid();
    native.generation = NativeMaintenanceCaptureReleaseGeneration::Native;
    assert_eq!(inspect(&native).reason, "installed_release_unavailable");

    let mut epoch = Fixture::valid();
    epoch.snapshot_ordinal = "1".to_owned();
    assert_eq!(inspect(&epoch).reason, "capture_epoch_mismatch");

    let mut current = Fixture::valid();
    current.rain_commit = Some(SUCCESSOR_COMMIT.to_owned());
    assert_eq!(inspect(&current).reason, "current_release_mismatch");
}

/// Admit a native release only through the explicitly verified native-generation owner.
#[test]
fn capture_readiness_accepts_independently_verified_native_generation() {
    let mut fixture = Fixture::valid();
    fixture.generation = NativeMaintenanceCaptureReleaseGeneration::Native;
    let mut installed = fixture.clone();
    let mut ports = fixture.clone();
    let mut source = fixture.clone();
    let result = crate::native_maintenance_capture_readiness::inspect_native_maintenance_capture_readiness_for_generation(
        &mut installed, &mut ports, &mut source, &FixtureTime,
        NativeMaintenanceCaptureReleaseGeneration::Native,
    );
    assert!(result.ready);
    assert_eq!(result.source_commit.as_deref(), Some(SOURCE_COMMIT));
}
