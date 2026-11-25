//! Compose verified rain performance package pairs without reopening package trust boundaries.

use std::collections::BTreeMap;
use std::fmt::{self, Display};

use weather_contracts::JsonValue;
use weather_worker::{AdmittedRainEdgeEvidence, RainPerformanceReportPackageInputs};

use crate::{
    AdmittedRainArchiveBody, AdmittedRainArchiveClaim, AdmittedRainArchivePackage,
    AdmittedRainArchiveReceipt, AdmittedRainArchiveRun, JoinedRainPerformanceEdgeRow,
    MaintenanceCanonicalJson, RainPerformanceAdjustmentRowsError, RainPerformanceEdgeRowsError,
    project_joined_rain_performance_edge_row, project_normalized_rain_performance_adjustment_rows,
};

/// one interval-selected edge retained until the report source revision is resolved.
#[derive(Debug, Clone, PartialEq)]
pub enum RainPerformancePackageEdgeEvidence {
    Projected(Box<AdmittedRainEdgeEvidence>),
    Joined(Box<JoinedRainPerformanceEdgeRow>),
}

/// one admitted edge receipt row plus the scoring projection retained from it.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainPerformancePackageEdge {
    pub edge_receipt_identity_sha256: String,
    pub row_index: u64,
    pub record_valid_at: String,
    pub canonical_receipt_row: Vec<u8>,
    pub evidence: RainPerformancePackageEdgeEvidence,
}

/// one matched and independently verified forecast and adjustment package pair.
#[derive(Debug, Clone, PartialEq)]
pub struct AdmittedRainPerformancePackagePair {
    pub inputs: RainPerformanceReportPackageInputs,
    pub bodies: Vec<AdmittedRainArchiveBody>,
    pub claims: Vec<AdmittedRainArchiveClaim>,
    pub edge_rows: Vec<AdmittedRainPerformancePackageEdge>,
    pub normalized_adjustment_rows: Vec<JsonValue>,
    pub receipts: Vec<AdmittedRainArchiveReceipt>,
    pub runs: Vec<AdmittedRainArchiveRun>,
}

/// the report identities and archive material for one composed cohort.
#[derive(Debug, Clone, PartialEq)]
pub struct ComposedRainPerformancePackage {
    pub inputs: RainPerformanceReportPackageInputs,
    archive: AdmittedRainArchivePackage,
    edge_evidence: Vec<RainPerformancePackageEdgeEvidence>,
    normalized_adjustment_rows: Vec<JsonValue>,
}

impl ComposedRainPerformancePackage {
    /// move chronological input rows to a separate scoring family without rain projection
    pub(crate) fn into_shared_evidence(
        self,
    ) -> (
        RainPerformanceReportPackageInputs,
        Vec<JsonValue>,
        Vec<RainPerformancePackageEdgeEvidence>,
    ) {
        (
            self.inputs,
            self.normalized_adjustment_rows,
            self.edge_evidence,
        )
    }

    /// Project interval-selected generic rows only after source revision resolution.
    pub fn materialize_archive(
        mut self,
    ) -> Result<
        (
            AdmittedRainArchivePackage,
            RainPerformanceReportPackageInputs,
        ),
        RainPerformancePackageMaterializationError,
    > {
        let adjustment_rows =
            project_normalized_rain_performance_adjustment_rows(self.normalized_adjustment_rows)?;
        self.archive.claims.extend(adjustment_rows.claims);
        self.archive.receipts.extend(adjustment_rows.receipts);
        self.archive.runs.extend(adjustment_rows.runs);

        self.archive.edges.reserve(self.edge_evidence.len());
        // project only rows retained by interval composition
        for evidence in self.edge_evidence {
            let projected = match evidence {
                RainPerformancePackageEdgeEvidence::Projected(projected) => *projected,
                RainPerformancePackageEdgeEvidence::Joined(joined) => {
                    project_joined_rain_performance_edge_row(&joined)?
                }
            };
            self.archive.edges.push(projected);
        }

        Ok((self.archive, self.inputs))
    }
}

/// approved calendar, canonical JSON, and digest owners used after verification.
pub trait RainPerformancePackageCompositionValues: MaintenanceCanonicalJson {
    /// resolve the record timestamp through the original package calendar.
    fn rain_package_local_date(&mut self, record_valid_at: &str) -> Option<String>;
}

/// closed failures from verified package-pair composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RainPerformancePackageCompositionError {
    BodyIdentityCollision,
    Calendar,
    CanonicalIdentity,
    EdgeReceiptRowIdentityCollision,
    Empty,
    OverlappingIntervals,
}

/// closed failures from deferred rain-specific row projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RainPerformancePackageMaterializationError {
    AdjustmentRows(RainPerformanceAdjustmentRowsError),
    EdgeRows(RainPerformanceEdgeRowsError),
}

impl Display for RainPerformancePackageMaterializationError {
    /// Preserve the projection owner's stable diagnostic.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AdjustmentRows(error) => Display::fmt(error, formatter),
            Self::EdgeRows(error) => Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for RainPerformancePackageMaterializationError {}

impl From<RainPerformanceAdjustmentRowsError> for RainPerformancePackageMaterializationError {
    /// Retain the adjustment projection failure class.
    fn from(error: RainPerformanceAdjustmentRowsError) -> Self {
        Self::AdjustmentRows(error)
    }
}

impl From<RainPerformanceEdgeRowsError> for RainPerformancePackageMaterializationError {
    /// Retain the edge projection failure class.
    fn from(error: RainPerformanceEdgeRowsError) -> Self {
        Self::EdgeRows(error)
    }
}

/// compose disjoint verified pairs while moving large body buffers into the result.
pub fn compose_admitted_rain_performance_package_pairs(
    mut pairs: Vec<AdmittedRainPerformancePackagePair>,
    values: &mut impl RainPerformancePackageCompositionValues,
) -> Result<ComposedRainPerformancePackage, RainPerformancePackageCompositionError> {
    // reject empty composition
    if pairs.is_empty() {
        return Err(RainPerformancePackageCompositionError::Empty);
    }

    // preserve stable order among equal starts
    pairs.sort_by(|left, right| {
        left.inputs
            .local_date_from
            .cmp(&right.inputs.local_date_from)
    });

    // require disjoint chronological blocks
    for adjacent in pairs.windows(2) {
        let previous = &adjacent[0];
        let current = &adjacent[1];
        // reject touching or overlapping blocks
        if previous.inputs.local_date_to >= current.inputs.local_date_from {
            return Err(RainPerformancePackageCompositionError::OverlappingIntervals);
        }
    }

    // retain original identities for one pair
    if pairs.len() == 1 {
        return compose_single_pair(pairs.pop().expect("one checked pair"), values);
    }

    compose_multiple_pairs(pairs, values)
}

/// retain one verified pair while filtering edges by the record clock.
fn compose_single_pair(
    pair: AdmittedRainPerformancePackagePair,
    values: &mut impl RainPerformancePackageCompositionValues,
) -> Result<ComposedRainPerformancePackage, RainPerformancePackageCompositionError> {
    let AdmittedRainPerformancePackagePair {
        inputs,
        bodies,
        claims,
        edge_rows,
        normalized_adjustment_rows,
        receipts,
        runs,
    } = pair;
    let mut edge_evidence = Vec::with_capacity(edge_rows.len());
    // preserve admitted edge order
    for edge in edge_rows {
        let local_date = values
            .rain_package_local_date(&edge.record_valid_at)
            .ok_or(RainPerformancePackageCompositionError::Calendar)?;
        // retain only this package block
        if local_date >= inputs.local_date_from && local_date <= inputs.local_date_to {
            edge_evidence.push(edge.evidence);
        }
    }

    Ok(ComposedRainPerformancePackage {
        inputs,
        archive: AdmittedRainArchivePackage {
            bodies,
            claims,
            edges: Vec::new(),
            receipts,
            runs,
        },
        edge_evidence,
        normalized_adjustment_rows,
    })
}

/// merge multiple verified pairs and synthesize only their aggregate identities.
fn compose_multiple_pairs(
    pairs: Vec<AdmittedRainPerformancePackagePair>,
    values: &mut impl RainPerformancePackageCompositionValues,
) -> Result<ComposedRainPerformancePackage, RainPerformancePackageCompositionError> {
    let mut adjustment_manifest_identities = Vec::with_capacity(pairs.len());
    let mut adjustment_watermark_identities = Vec::with_capacity(pairs.len());
    let mut forecast_manifest_identities = Vec::with_capacity(pairs.len());
    let mut first_local_date = None;
    let mut last_local_date = None;
    let mut target_cutoff_at = None;
    let mut bodies = Vec::<AdmittedRainArchiveBody>::new();
    let mut body_indexes = BTreeMap::<String, usize>::new();
    let mut claims = Vec::new();
    let mut receipts = Vec::new();
    let mut runs = Vec::new();
    let mut normalized_adjustment_rows = Vec::new();
    let mut global_edge_rows = BTreeMap::<(String, u64), Vec<u8>>::new();
    let mut bounded_edges = BTreeMap::<(String, u64), RainPerformancePackageEdgeEvidence>::new();

    // merge in chronological package order
    for pair in pairs {
        let AdmittedRainPerformancePackagePair {
            inputs,
            bodies: pair_bodies,
            claims: pair_claims,
            edge_rows,
            normalized_adjustment_rows: pair_normalized_adjustment_rows,
            receipts: pair_receipts,
            runs: pair_runs,
        } = pair;
        let RainPerformanceReportPackageInputs {
            adjustment_evidence_manifest_sha256,
            adjustment_evidence_watermark_sha256,
            forecast_training_manifest_sha256,
            local_date_from,
            local_date_to,
            target_cutoff_at: pair_target_cutoff_at,
        } = inputs;

        adjustment_manifest_identities.push(adjustment_evidence_manifest_sha256);
        adjustment_watermark_identities.push(adjustment_evidence_watermark_sha256);
        forecast_manifest_identities.push(forecast_training_manifest_sha256);

        // retain the lexically latest canonical cutoff
        if target_cutoff_at
            .as_ref()
            .is_none_or(|current| current < &pair_target_cutoff_at)
        {
            target_cutoff_at = Some(pair_target_cutoff_at);
        }

        // retain one byte-identical body per member path
        for body in pair_bodies {
            // compare any previously retained path
            if let Some(index) = body_indexes.get(&body.member_path).copied() {
                // reject different bytes under one path
                if bodies[index].compressed != body.compressed {
                    return Err(RainPerformancePackageCompositionError::BodyIdentityCollision);
                }
                continue;
            }
            body_indexes.insert(body.member_path.clone(), bodies.len());
            bodies.push(body);
        }

        // validate every global edge before date filtering
        for edge in edge_rows {
            let AdmittedRainPerformancePackageEdge {
                edge_receipt_identity_sha256,
                row_index,
                record_valid_at,
                canonical_receipt_row,
                evidence,
            } = edge;
            let identity = (edge_receipt_identity_sha256, row_index);
            // require one exact canonical row per receipt position
            if let Some(existing) = global_edge_rows.get(&identity) {
                // reject scoring drift even outside the package block
                if existing != &canonical_receipt_row {
                    return Err(
                        RainPerformancePackageCompositionError::EdgeReceiptRowIdentityCollision,
                    );
                }
            } else {
                global_edge_rows.insert(identity.clone(), canonical_receipt_row);
            }

            let local_date = values
                .rain_package_local_date(&record_valid_at)
                .ok_or(RainPerformancePackageCompositionError::Calendar)?;
            // retain the first in-block row for this identity
            if local_date >= local_date_from && local_date <= local_date_to {
                bounded_edges.entry(identity).or_insert(evidence);
            }
        }

        // retain the earliest sorted lower bound
        if first_local_date.is_none() {
            first_local_date = Some(local_date_from);
        }
        last_local_date = Some(local_date_to);
        claims.extend(pair_claims);
        normalized_adjustment_rows.extend(pair_normalized_adjustment_rows);
        receipts.extend(pair_receipts);
        runs.extend(pair_runs);
    }

    let adjustment_evidence_manifest_sha256 =
        canonical_identity_vector_sha256(&adjustment_manifest_identities, values)?;
    let adjustment_evidence_watermark_sha256 =
        canonical_identity_vector_sha256(&adjustment_watermark_identities, values)?;
    let forecast_training_manifest_sha256 =
        canonical_identity_vector_sha256(&forecast_manifest_identities, values)?;

    Ok(ComposedRainPerformancePackage {
        inputs: RainPerformanceReportPackageInputs {
            adjustment_evidence_manifest_sha256,
            adjustment_evidence_watermark_sha256,
            forecast_training_manifest_sha256,
            local_date_from: first_local_date.expect("nonempty checked pairs"),
            local_date_to: last_local_date.expect("nonempty checked pairs"),
            target_cutoff_at: target_cutoff_at.expect("nonempty checked pairs"),
        },
        archive: AdmittedRainArchivePackage {
            bodies,
            claims,
            edges: Vec::new(),
            receipts,
            runs,
        },
        edge_evidence: bounded_edges.into_values().collect(),
        normalized_adjustment_rows,
    })
}

/// hash one chronological identity vector through the approved canonical owner.
fn canonical_identity_vector_sha256(
    identities: &[String],
    values: &mut impl RainPerformancePackageCompositionValues,
) -> Result<String, RainPerformancePackageCompositionError> {
    let value = JsonValue::Array(
        identities
            .iter()
            .map(|identity| JsonValue::String(identity.encode_utf16().collect()))
            .collect(),
    );
    let encoded = values
        .encode_canonical_json(&value)
        .ok_or(RainPerformancePackageCompositionError::CanonicalIdentity)?;
    values
        .sha256(&encoded)
        .map(|digest| digest.as_str().to_owned())
        .ok_or(RainPerformancePackageCompositionError::CanonicalIdentity)
}

#[cfg(test)]
mod tests {
    use weather_contracts::{JsonValue, Nullable};
    use weather_database::Sha256Digest;
    use weather_worker::{AdmittedRainEdgeEvidence, AdmittedRainEdgeSource};

    use super::*;

    #[derive(Default)]
    struct Values {
        encoded: Vec<JsonValue>,
    }

    impl MaintenanceCanonicalJson for Values {
        /// parsing is outside composition tests.
        fn parse_canonical_object(&mut self, _: &[u8]) -> Option<JsonValue> {
            None
        }

        /// record the delegated identity vector.
        fn encode_canonical_json(&mut self, value: &JsonValue) -> Option<Vec<u8>> {
            self.encoded.push(value.clone());
            Some(vec![self.encoded.len() as u8])
        }

        /// distinguish the three delegated hashes.
        fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
            Some(Sha256Digest::admitted(format!("digest-{}", bytes[0])))
        }
    }

    impl RainPerformancePackageCompositionValues for Values {
        /// use the date prefix as the admitted local date.
        fn rain_package_local_date(&mut self, value: &str) -> Option<String> {
            value.get(..10).map(str::to_owned)
        }
    }

    /// build one minimal admitted evidence row.
    fn evidence(label: &str, forecast_valid_at: &str) -> AdmittedRainEdgeEvidence {
        AdmittedRainEdgeEvidence {
            active_bundle: Nullable::Value(label.to_owned()),
            adjustment_bundle_sha256: Nullable::Null,
            captured_applied: false,
            corrected_precipitation_mm: Nullable::Null,
            first_edge_committed_at: "2026-01-01T01:00:00.000Z".to_owned(),
            identity_context: JsonValue::String(label.encode_utf16().collect()),
            provenance_complete: true,
            raw_best_match_precipitation_mm: Nullable::Null,
            reason_code: Nullable::Null,
            source_receipt_at: "2026-01-01T00:30:00.000Z".to_owned(),
            source_forecast: Some(AdmittedRainEdgeSource {
                first_received_at: "2026-01-01T00:00:00.000Z".to_owned(),
                model_lead_hours: 1.0,
                raw_precipitation_mm: 0.0,
                run_initialized_at: "2026-01-01T00:00:00.000Z".to_owned(),
                valid_at: forecast_valid_at.to_owned(),
            }),
        }
    }

    /// build one verified pair with minimal archive material.
    fn pair(
        from: &str,
        to: &str,
        cutoff: &str,
        identity: &str,
        bodies: Vec<AdmittedRainArchiveBody>,
        edge_rows: Vec<AdmittedRainPerformancePackageEdge>,
    ) -> AdmittedRainPerformancePackagePair {
        AdmittedRainPerformancePackagePair {
            inputs: RainPerformanceReportPackageInputs {
                adjustment_evidence_manifest_sha256: format!("a-{identity}"),
                adjustment_evidence_watermark_sha256: format!("w-{identity}"),
                forecast_training_manifest_sha256: format!("f-{identity}"),
                local_date_from: from.to_owned(),
                local_date_to: to.to_owned(),
                target_cutoff_at: cutoff.to_owned(),
            },
            bodies,
            claims: Vec::new(),
            edge_rows,
            normalized_adjustment_rows: Vec::new(),
            receipts: Vec::new(),
            runs: Vec::new(),
        }
    }

    /// build one admitted edge wrapper.
    fn edge(
        receipt: &str,
        row_index: u64,
        record_valid_at: &str,
        canonical: &[u8],
        label: &str,
        forecast_valid_at: &str,
    ) -> AdmittedRainPerformancePackageEdge {
        AdmittedRainPerformancePackageEdge {
            edge_receipt_identity_sha256: receipt.to_owned(),
            row_index,
            record_valid_at: record_valid_at.to_owned(),
            canonical_receipt_row: canonical.to_vec(),
            evidence: RainPerformancePackageEdgeEvidence::Projected(Box::new(evidence(
                label,
                forecast_valid_at,
            ))),
        }
    }

    /// build one generic joined edge that cannot be projected as rain evidence.
    fn malformed_joined_edge(
        receipt: &str,
        row_index: u64,
        record_valid_at: &str,
        canonical: &[u8],
    ) -> AdmittedRainPerformancePackageEdge {
        AdmittedRainPerformancePackageEdge {
            edge_receipt_identity_sha256: receipt.to_owned(),
            row_index,
            record_valid_at: record_valid_at.to_owned(),
            canonical_receipt_row: canonical.to_vec(),
            evidence: RainPerformancePackageEdgeEvidence::Joined(Box::new(
                JoinedRainPerformanceEdgeRow {
                    bundle_identities: JsonValue::Object(Vec::new()),
                    first_edge_committed_at: "2026-01-01T01:00:00.000Z".to_owned(),
                    row: JsonValue::Object(vec![(
                        "record".encode_utf16().collect(),
                        JsonValue::Object(vec![(
                            "validAt".encode_utf16().collect(),
                            JsonValue::String(record_valid_at.encode_utf16().collect()),
                        )]),
                    )]),
                    settings_sha256: "f".repeat(64),
                    source_receipt_at: "2026-01-01T00:30:00.000Z".to_owned(),
                },
            )),
        }
    }

    /// build one already-normalized claim row for deferred projection.
    fn normalized_claim_row(id: &str) -> JsonValue {
        JsonValue::Object(vec![
            (
                "record_kind".encode_utf16().collect(),
                JsonValue::String("rain_claim".encode_utf16().collect()),
            ),
            (
                "payload".encode_utf16().collect(),
                JsonValue::Object(vec![
                    (
                        "id".encode_utf16().collect(),
                        JsonValue::String(id.encode_utf16().collect()),
                    ),
                    (
                        "kind".encode_utf16().collect(),
                        JsonValue::String("forecast".encode_utf16().collect()),
                    ),
                    ("runInitializedAt".encode_utf16().collect(), JsonValue::Null),
                    ("stationId".encode_utf16().collect(), JsonValue::Null),
                    (
                        "windowEndExclusive".encode_utf16().collect(),
                        JsonValue::Null,
                    ),
                    ("windowStart".encode_utf16().collect(), JsonValue::Null),
                ]),
            ),
        ])
    }

    /// recover the test label from one retained evidence row.
    fn label(edge: &AdmittedRainEdgeEvidence) -> String {
        // require the fixture label
        let Nullable::Value(value) = &edge.active_bundle else {
            panic!("missing test label");
        };
        value.clone()
    }

    /// compose chronological material without copying the first body buffer.
    #[test]
    fn composes_reversed_pairs_with_record_clock_edges_and_moved_bodies() {
        let shared = AdmittedRainArchiveBody {
            member_path: "body/shared.gz".to_owned(),
            compressed: vec![1, 2, 3],
        };
        let shared_pointer = shared.compressed.as_ptr();
        let mut early = pair(
            "2026-01-01",
            "2026-01-02",
            "2026-01-03T00:00:00.000Z",
            "early",
            vec![shared],
            vec![
                edge(
                    "bbb",
                    10,
                    "2026-01-01T08:00:00.000Z",
                    b"row-b",
                    "b",
                    "2040-01-01T00:00:00.000Z",
                ),
                edge(
                    "aaa",
                    2,
                    "2025-12-31T08:00:00.000Z",
                    b"outside-same",
                    "outside",
                    "2026-01-01T08:00:00.000Z",
                ),
            ],
        );
        early.normalized_adjustment_rows = vec![normalized_claim_row("early-claim")];
        let mut late = pair(
            "2026-01-04",
            "2026-01-05",
            "2026-01-06T00:00:00.000Z",
            "late",
            vec![AdmittedRainArchiveBody {
                member_path: "body/shared.gz".to_owned(),
                compressed: vec![1, 2, 3],
            }],
            vec![
                edge(
                    "aaa",
                    2,
                    "2025-12-31T08:00:00.000Z",
                    b"outside-same",
                    "outside-copy",
                    "2040-01-01T00:00:00.000Z",
                ),
                edge(
                    "aaa",
                    11,
                    "2026-01-04T08:00:00.000Z",
                    b"row-a-11",
                    "a-11",
                    "2040-01-01T00:00:00.000Z",
                ),
                edge(
                    "aaa",
                    3,
                    "2026-01-04T09:00:00.000Z",
                    b"row-a-3",
                    "a-3",
                    "2040-01-01T00:00:00.000Z",
                ),
            ],
        );
        late.normalized_adjustment_rows = vec![normalized_claim_row("late-claim")];
        let mut values = Values::default();

        let composed =
            compose_admitted_rain_performance_package_pairs(vec![late, early], &mut values)
                .expect("composition should succeed");

        assert_eq!(composed.inputs.local_date_from, "2026-01-01");
        assert_eq!(composed.inputs.local_date_to, "2026-01-05");
        assert_eq!(composed.inputs.target_cutoff_at, "2026-01-06T00:00:00.000Z");
        assert_eq!(
            (
                composed.inputs.adjustment_evidence_manifest_sha256.as_str(),
                composed
                    .inputs
                    .adjustment_evidence_watermark_sha256
                    .as_str(),
                composed.inputs.forecast_training_manifest_sha256.as_str(),
            ),
            ("digest-1", "digest-2", "digest-3")
        );
        let (archive, inputs) = composed
            .materialize_archive()
            .expect("selected rows should materialize");
        assert_eq!(inputs.local_date_from, "2026-01-01");
        assert_eq!(archive.bodies.len(), 1);
        assert_eq!(
            archive
                .claims
                .iter()
                .map(|claim| claim.id.as_str())
                .collect::<Vec<_>>(),
            vec!["early-claim", "late-claim"]
        );
        assert_eq!(archive.bodies[0].compressed.as_ptr(), shared_pointer);
        assert_eq!(
            archive.edges.iter().map(label).collect::<Vec<_>>(),
            vec!["a-3".to_owned(), "a-11".to_owned(), "b".to_owned()]
        );
        assert_eq!(
            values.encoded,
            vec![
                identity_vector(&["a-early", "a-late"]),
                identity_vector(&["w-early", "w-late"]),
                identity_vector(&["f-early", "f-late"]),
            ]
        );
    }

    /// retain one package's identities and admitted edge ordering.
    #[test]
    fn single_pair_preserves_hashes_and_input_edge_order() {
        let input = pair(
            "2026-02-01",
            "2026-02-01",
            "2026-02-02T00:00:00.000Z",
            "single",
            Vec::new(),
            vec![
                edge(
                    "zzz",
                    9,
                    "2026-02-01T08:00:00.000Z",
                    b"z",
                    "first",
                    "2040-01-01T00:00:00.000Z",
                ),
                edge(
                    "aaa",
                    1,
                    "2026-02-01T09:00:00.000Z",
                    b"a",
                    "second",
                    "2040-01-01T00:00:00.000Z",
                ),
                edge(
                    "mmm",
                    1,
                    "2026-02-02T09:00:00.000Z",
                    b"m",
                    "outside",
                    "2026-02-01T09:00:00.000Z",
                ),
            ],
        );
        let expected_inputs = input.inputs.clone();
        let mut values = Values::default();

        let composed = compose_admitted_rain_performance_package_pairs(vec![input], &mut values)
            .expect("single composition should succeed");

        assert_eq!(composed.inputs, expected_inputs);
        let (archive, materialized_inputs) = composed
            .materialize_archive()
            .expect("selected rows should materialize");
        assert_eq!(materialized_inputs, expected_inputs);
        assert_eq!(
            archive.edges.iter().map(label).collect::<Vec<_>>(),
            vec!["first".to_owned(), "second".to_owned()]
        );
        assert!(values.encoded.is_empty());
    }

    /// collision-check malformed generic edges without projecting excluded rain rows.
    #[test]
    fn malformed_out_of_block_joined_edge_is_not_rain_projected() {
        let composed = compose_admitted_rain_performance_package_pairs(
            vec![
                pair(
                    "2026-03-01",
                    "2026-03-01",
                    "2026-03-02T00:00:00.000Z",
                    "one",
                    Vec::new(),
                    vec![malformed_joined_edge(
                        "receipt",
                        4,
                        "2026-01-01T00:00:00.000Z",
                        b"same",
                    )],
                ),
                pair(
                    "2026-03-03",
                    "2026-03-03",
                    "2026-03-04T00:00:00.000Z",
                    "two",
                    Vec::new(),
                    vec![malformed_joined_edge(
                        "receipt",
                        4,
                        "2026-01-01T00:00:00.000Z",
                        b"same",
                    )],
                ),
            ],
            &mut Values::default(),
        )
        .expect("byte-identical global rows should compose");

        let (archive, _) = composed
            .materialize_archive()
            .expect("excluded malformed rain rows should not be projected");
        assert!(archive.edges.is_empty());
    }

    /// reject cross-package drift before bounded scoring projection.
    #[test]
    fn rejects_out_of_block_global_edge_drift_and_body_collisions() {
        let out_of_block_drift = compose_admitted_rain_performance_package_pairs(
            vec![
                pair(
                    "2026-03-01",
                    "2026-03-01",
                    "2026-03-02T00:00:00.000Z",
                    "one",
                    Vec::new(),
                    vec![malformed_joined_edge(
                        "receipt",
                        4,
                        "2026-01-01T00:00:00.000Z",
                        b"original",
                    )],
                ),
                pair(
                    "2026-03-03",
                    "2026-03-03",
                    "2026-03-04T00:00:00.000Z",
                    "two",
                    Vec::new(),
                    vec![malformed_joined_edge(
                        "receipt",
                        4,
                        "2026-01-01T00:00:00.000Z",
                        b"drifted",
                    )],
                ),
            ],
            &mut Values::default(),
        );
        assert_eq!(
            out_of_block_drift,
            Err(RainPerformancePackageCompositionError::EdgeReceiptRowIdentityCollision)
        );

        let body_collision = compose_admitted_rain_performance_package_pairs(
            vec![
                pair(
                    "2026-04-01",
                    "2026-04-01",
                    "2026-04-02T00:00:00.000Z",
                    "one",
                    vec![AdmittedRainArchiveBody {
                        member_path: "same".to_owned(),
                        compressed: vec![1],
                    }],
                    Vec::new(),
                ),
                pair(
                    "2026-04-03",
                    "2026-04-03",
                    "2026-04-04T00:00:00.000Z",
                    "two",
                    vec![AdmittedRainArchiveBody {
                        member_path: "same".to_owned(),
                        compressed: vec![2],
                    }],
                    Vec::new(),
                ),
            ],
            &mut Values::default(),
        );
        assert_eq!(
            body_collision,
            Err(RainPerformancePackageCompositionError::BodyIdentityCollision)
        );
    }

    /// build one generated canonical string vector.
    fn identity_vector(values: &[&str]) -> JsonValue {
        JsonValue::Array(
            values
                .iter()
                .map(|value| JsonValue::String(value.encode_utf16().collect()))
                .collect(),
        )
    }
}
