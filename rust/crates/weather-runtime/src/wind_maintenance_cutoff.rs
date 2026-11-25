//! Cutoff and immutable-member admission before retained wind phase selection.

use std::collections::{HashMap, HashSet};

use weather_contracts::ForecastAdjustmentCoefficientSeasonEnum;
use weather_database::Sha256Digest;

const QUALIFICATION_EPOCH_DAYS: usize = 402;
const FINAL_TRAINING_DAYS: usize = 365;

/// Canonical UTC instant already resolved by the approved runtime calendar owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmittedWindMaintenanceInstant<'a> {
    utc: &'a str,
    epoch_milliseconds: i64,
}

impl<'a> AdmittedWindMaintenanceInstant<'a> {
    /// Retain one instant only after the shared canonical clock admitted it.
    pub(crate) const fn from_canonical(utc: &'a str, epoch_milliseconds: i64) -> Self {
        Self {
            utc,
            epoch_milliseconds,
        }
    }

    /// Borrow the canonical UTC representation used by later artifact identity.
    pub const fn utc(self) -> &'a str {
        self.utc
    }

    /// Read the admitted epoch value used only for cutoff ordering.
    pub const fn epoch_milliseconds(self) -> i64 {
        self.epoch_milliseconds
    }
}

/// One admitted Los Angeles calendar date in exact epoch order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmittedWindMaintenanceDate<'a> {
    pub label: &'a str,
    pub ordinal: i64,
    pub season: ForecastAdjustmentCoefficientSeasonEnum,
}

/// Exact D0 through D401 material produced by the approved calendar owner.
#[derive(Clone, Copy, Debug)]
pub struct AdmittedWindMaintenanceEpoch<'a> {
    pub local_dates: &'a [AdmittedWindMaintenanceDate<'a>],
    pub end_exclusive: AdmittedWindMaintenanceInstant<'a>,
    pub final_training_cutoff: AdmittedWindMaintenanceInstant<'a>,
}

/// One already-validated immutable member named by an admitted manifest.
#[derive(Clone, Copy, Debug)]
pub struct AdmittedWindMaintenanceManifestMember<'a> {
    pub local_date: &'a str,
    pub sha256: &'a Sha256Digest,
}

/// Exact manifest projection after its separate shape and hash admission.
#[derive(Clone, Copy, Debug)]
pub struct AdmittedWindMaintenanceManifest<'a> {
    pub identity: &'a Sha256Digest,
    pub from_local_date: &'a str,
    pub to_local_date: &'a str,
    pub members: &'a [AdmittedWindMaintenanceManifestMember<'a>],
}

/// Journal proof that both immutable member sides existed before the cutoff.
#[derive(Clone, Copy, Debug)]
pub struct AdmittedWindMaintenanceOpenedMember<'a> {
    pub member_sha256: &'a Sha256Digest,
    pub maximum_source_receipt_at: AdmittedWindMaintenanceInstant<'a>,
    pub maximum_target_receipt_at: AdmittedWindMaintenanceInstant<'a>,
}

/// One admitted export row plus its calendar and availability projection.
#[derive(Clone, Copy, Debug)]
pub struct AdmittedWindMaintenanceRow<'a, Material> {
    pub material: Material,
    pub local_date: &'a str,
    pub received_at: Option<AdmittedWindMaintenanceInstant<'a>>,
}

/// Complete typed inputs before any numerical training phase is selected.
#[derive(Clone, Copy, Debug)]
pub struct WindMaintenanceCutoffInput<'a, Material> {
    pub due_month: &'a str,
    pub cutoff: AdmittedWindMaintenanceInstant<'a>,
    pub epoch: AdmittedWindMaintenanceEpoch<'a>,
    pub manifest: AdmittedWindMaintenanceManifest<'a>,
    pub opened_members: &'a [AdmittedWindMaintenanceOpenedMember<'a>],
    pub rows: &'a [AdmittedWindMaintenanceRow<'a, Material>],
}

/// Pre-holdout material after every row has proven earlier availability.
#[derive(Debug)]
pub struct ReadyWindMaintenancePreholdout<'a, Material> {
    pub due_month: &'a str,
    pub d0: &'a str,
    pub d401: &'a str,
    pub final_training_cutoff: AdmittedWindMaintenanceInstant<'a>,
    pub snapshot_manifest_sha256: &'a Sha256Digest,
    pub expected_training_dates: Vec<(&'a str, ForecastAdjustmentCoefficientSeasonEnum)>,
    pub training_rows: Vec<Material>,
}

/// The original ordinary insufficiency returned instead of selecting a phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindMaintenanceCutoffRefusal<'a> {
    pub snapshot_manifest_sha256: &'a Sha256Digest,
}

impl WindMaintenanceCutoffRefusal<'_> {
    /// Preserve the literal retained evidence gate name.
    pub const fn failed_gate(&self) -> &'static str {
        "maintenance_earlier_receipt_provenance"
    }
}

/// Admitted pre-holdout material or the source's ordinary provenance refusal.
#[derive(Debug)]
pub enum WindMaintenanceCutoffOutcome<'a, Material> {
    Ready(ReadyWindMaintenancePreholdout<'a, Material>),
    Refused(WindMaintenanceCutoffRefusal<'a>),
}

/// Structural refusal corresponding to the original throwing boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindMaintenanceCutoffError {
    DuplicateOpenedMember,
    InvalidEpoch,
    InvalidManifestBoundary,
    MemberNotOpenedBeforeCutoff,
    MissingEpochCoverage,
}

/// Admit the complete earlier epoch before copying final-training rows downstream.
pub fn admit_wind_maintenance_cutoff<'a, Material: Copy>(
    input: WindMaintenanceCutoffInput<'a, Material>,
) -> Result<WindMaintenanceCutoffOutcome<'a, Material>, WindMaintenanceCutoffError> {
    let covered = admit_wind_maintenance_coverage(&input)?;
    Ok(select_wind_maintenance_phase(input, &covered))
}

/// Admit immutable epoch coverage before touching row contents.
pub(crate) fn admit_wind_maintenance_coverage<'a, Material>(
    input: &WindMaintenanceCutoffInput<'a, Material>,
) -> Result<HashSet<&'a str>, WindMaintenanceCutoffError> {
    let dates = input.epoch.local_dates;
    // require the literal D0 through D401 population before indexing boundaries
    if dates.len() != QUALIFICATION_EPOCH_DAYS {
        return Err(WindMaintenanceCutoffError::InvalidEpoch);
    }
    // retain strict label and admitted local-day chronology across the full epoch
    for pair in dates.windows(2) {
        // reject duplicate, reversed, or skipped admitted dates
        if pair[0].label >= pair[1].label || pair[0].ordinal.checked_add(1) != Some(pair[1].ordinal)
        {
            return Err(WindMaintenanceCutoffError::InvalidEpoch);
        }
    }
    // bind the manifest span and require D401 to end no later than the due cutoff
    if input.manifest.from_local_date != dates[0].label
        || input.manifest.to_local_date != dates[QUALIFICATION_EPOCH_DAYS - 1].label
        || input.epoch.end_exclusive.epoch_milliseconds() > input.cutoff.epoch_milliseconds()
        || input.epoch.final_training_cutoff.epoch_milliseconds()
            >= input.epoch.end_exclusive.epoch_milliseconds()
    {
        return Err(WindMaintenanceCutoffError::InvalidManifestBoundary);
    }

    let mut opened = HashMap::with_capacity(input.opened_members.len());
    // reject duplicate journal identities before joining any manifest member
    for receipt in input.opened_members {
        // preserve Map size equality from the source implementation
        if opened
            .insert(receipt.member_sha256.as_str(), receipt)
            .is_some()
        {
            return Err(WindMaintenanceCutoffError::DuplicateOpenedMember);
        }
    }
    // require source and target custody for every immutable manifest member
    for member in input.manifest.members {
        let Some(receipt) = opened.get(member.sha256.as_str()) else {
            return Err(WindMaintenanceCutoffError::MemberNotOpenedBeforeCutoff);
        };
        // both receipt maxima must remain strictly before the monthly cutoff
        if receipt.maximum_source_receipt_at.epoch_milliseconds()
            >= input.cutoff.epoch_milliseconds()
            || receipt.maximum_target_receipt_at.epoch_milliseconds()
                >= input.cutoff.epoch_milliseconds()
        {
            return Err(WindMaintenanceCutoffError::MemberNotOpenedBeforeCutoff);
        }
    }

    let covered: HashSet<&str> = input
        .manifest
        .members
        .iter()
        .map(|member| member.local_date)
        .collect();
    // require at least one immutable member for every exact epoch date
    if dates.iter().any(|date| !covered.contains(date.label)) {
        return Err(WindMaintenanceCutoffError::MissingEpochCoverage);
    }

    Ok(covered)
}

/// Select the final training phase from an already admitted epoch and its custody.
pub(crate) fn select_wind_maintenance_phase<'a, Material: Copy>(
    input: WindMaintenanceCutoffInput<'a, Material>,
    covered: &HashSet<&str>,
) -> WindMaintenanceCutoffOutcome<'a, Material> {
    let dates = input.epoch.local_dates;
    // validate every row's availability before selecting the final-training phase
    for row in input.rows {
        let Some(received_at) = row.received_at else {
            return refused(input.manifest.identity);
        };
        // refuse unknown dates and post-cutoff receipts as ordinary insufficiency
        if received_at.epoch_milliseconds() >= input.cutoff.epoch_milliseconds()
            || !covered.contains(row.local_date)
        {
            return refused(input.manifest.identity);
        }
    }

    let final_training_dates: HashSet<&str> = dates[..FINAL_TRAINING_DAYS]
        .iter()
        .map(|date| date.label)
        .collect();
    let expected_training_dates = dates[..FINAL_TRAINING_DAYS]
        .iter()
        .map(|date| (date.label, date.season))
        .collect();
    let training_rows = input
        .rows
        .iter()
        .filter(|row| final_training_dates.contains(row.local_date))
        .map(|row| row.material)
        .collect();
    WindMaintenanceCutoffOutcome::Ready(ReadyWindMaintenancePreholdout {
        due_month: input.due_month,
        d0: dates[0].label,
        d401: dates[QUALIFICATION_EPOCH_DAYS - 1].label,
        final_training_cutoff: input.epoch.final_training_cutoff,
        snapshot_manifest_sha256: input.manifest.identity,
        expected_training_dates,
        training_rows,
    })
}

/// Return the one ordinary cutoff failure without duplicating its identity binding.
fn refused<'a, Material>(
    snapshot_manifest_sha256: &'a Sha256Digest,
) -> WindMaintenanceCutoffOutcome<'a, Material> {
    WindMaintenanceCutoffOutcome::Refused(WindMaintenanceCutoffRefusal {
        snapshot_manifest_sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RECEIPT_UTC: &str = "2025-01-01T00:00:00.000Z";
    const ROW_UTC: &str = "2025-01-02T00:00:00.000Z";
    const FINAL_TRAINING_UTC: &str = "2025-01-31T23:59:59.999Z";
    const EPOCH_END_UTC: &str = "2025-02-07T00:00:00.000Z";
    const CUTOFF_UTC: &str = "2025-03-01T00:00:00.000Z";

    /// Borrowed views over test-owned labels and digest identities.
    struct FixtureViews<'a> {
        dates: Vec<AdmittedWindMaintenanceDate<'a>>,
        members: Vec<AdmittedWindMaintenanceManifestMember<'a>>,
        opened: Vec<AdmittedWindMaintenanceOpenedMember<'a>>,
    }

    /// Build test-only monotonically ordered labels with admitted day ordinals.
    fn fixture_backing() -> (Vec<String>, Vec<Sha256Digest>, Sha256Digest) {
        // labels intentionally test the typed ordinal boundary without reimplementing a calendar
        let labels = (0..QUALIFICATION_EPOCH_DAYS)
            .map(|index| format!("test-local-date-{index:03}"))
            .collect();
        let digests = (0..QUALIFICATION_EPOCH_DAYS)
            .map(|index| Sha256Digest::admitted(format!("{index:064x}")))
            .collect();
        let identity = Sha256Digest::admitted(format!("{:064x}", 10_000));
        (labels, digests, identity)
    }

    /// Project one complete admitted epoch and its matching journal receipts.
    fn fixture_views<'a>(labels: &'a [String], digests: &'a [Sha256Digest]) -> FixtureViews<'a> {
        let dates = labels
            .iter()
            .enumerate()
            .map(|(index, label)| AdmittedWindMaintenanceDate {
                label,
                ordinal: index as i64,
                season: ForecastAdjustmentCoefficientSeasonEnum::Winter,
            })
            .collect();
        let members = labels
            .iter()
            .zip(digests)
            .map(
                |(local_date, sha256)| AdmittedWindMaintenanceManifestMember { local_date, sha256 },
            )
            .collect();
        let opened = digests
            .iter()
            .map(|member_sha256| AdmittedWindMaintenanceOpenedMember {
                member_sha256,
                maximum_source_receipt_at: instant(RECEIPT_UTC, 1_000),
                maximum_target_receipt_at: instant(RECEIPT_UTC, 1_000),
            })
            .collect();
        FixtureViews {
            dates,
            members,
            opened,
        }
    }

    /// Build the cutoff input from one complete borrowed fixture.
    fn input<'a, 'data: 'a, Material>(
        identity: &'data Sha256Digest,
        views: &'a FixtureViews<'data>,
        rows: &'a [AdmittedWindMaintenanceRow<'data, Material>],
    ) -> WindMaintenanceCutoffInput<'a, Material> {
        WindMaintenanceCutoffInput {
            due_month: "2025-03",
            cutoff: instant(CUTOFF_UTC, 6_000),
            epoch: AdmittedWindMaintenanceEpoch {
                local_dates: &views.dates,
                end_exclusive: instant(EPOCH_END_UTC, 5_000),
                final_training_cutoff: instant(FINAL_TRAINING_UTC, 3_999),
            },
            manifest: AdmittedWindMaintenanceManifest {
                identity,
                from_local_date: views.dates[0].label,
                to_local_date: views.dates[QUALIFICATION_EPOCH_DAYS - 1].label,
                members: &views.members,
            },
            opened_members: &views.opened,
            rows,
        }
    }

    /// Construct an instant through the same private admitted boundary as production adapters.
    const fn instant(
        utc: &'static str,
        epoch_milliseconds: i64,
    ) -> AdmittedWindMaintenanceInstant<'static> {
        AdmittedWindMaintenanceInstant::from_canonical(utc, epoch_milliseconds)
    }

    #[test]
    fn selects_d0_through_d364_in_original_row_order() {
        let (labels, digests, identity) = fixture_backing();
        let views = fixture_views(&labels, &digests);
        let rows = (0..QUALIFICATION_EPOCH_DAYS)
            .rev()
            .map(|index| AdmittedWindMaintenanceRow {
                material: index,
                local_date: labels[index].as_str(),
                received_at: Some(instant(ROW_UTC, 2_000)),
            })
            .collect::<Vec<_>>();

        let outcome = admit_wind_maintenance_cutoff(input(&identity, &views, &rows))
            .expect("complete admitted epoch should pass cutoff admission");
        let WindMaintenanceCutoffOutcome::Ready(ready) = outcome else {
            panic!("complete admitted epoch should be ready");
        };

        assert_eq!(ready.d0, labels[0].as_str());
        assert_eq!(ready.d401, labels[401].as_str());
        assert_eq!(ready.expected_training_dates.len(), FINAL_TRAINING_DAYS);
        assert_eq!(ready.expected_training_dates[364].0, labels[364].as_str());
        assert_eq!(
            ready.training_rows,
            (0..FINAL_TRAINING_DAYS).rev().collect::<Vec<_>>()
        );
    }

    #[test]
    fn refuses_d401_row_received_exactly_at_cutoff() {
        let (labels, digests, identity) = fixture_backing();
        let views = fixture_views(&labels, &digests);
        let rows = [AdmittedWindMaintenanceRow {
            material: 401,
            local_date: labels[401].as_str(),
            received_at: Some(instant(CUTOFF_UTC, 6_000)),
        }];

        let outcome = admit_wind_maintenance_cutoff(input(&identity, &views, &rows))
            .expect("late row is an ordinary provenance refusal");
        let WindMaintenanceCutoffOutcome::Refused(refusal) = outcome else {
            panic!("cutoff equality must refuse before phase selection");
        };

        assert_eq!(
            refusal.failed_gate(),
            "maintenance_earlier_receipt_provenance"
        );
        assert_eq!(refusal.snapshot_manifest_sha256, &identity);
    }

    #[test]
    fn distinguishes_duplicate_receipts_and_missing_epoch_coverage() {
        let (labels, digests, identity) = fixture_backing();
        let mut views = fixture_views(&labels, &digests);
        views.opened.push(views.opened[0]);

        let duplicate = admit_wind_maintenance_cutoff(input::<usize>(&identity, &views, &[]))
            .expect_err("duplicate receipt identities must be structural");
        assert_eq!(duplicate, WindMaintenanceCutoffError::DuplicateOpenedMember);

        views.opened.pop();
        views.members.pop();
        let missing = admit_wind_maintenance_cutoff(input::<usize>(&identity, &views, &[]))
            .expect_err("an uncovered epoch date must be structural");
        assert_eq!(missing, WindMaintenanceCutoffError::MissingEpochCoverage);
    }
}
