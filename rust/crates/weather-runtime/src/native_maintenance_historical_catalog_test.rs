use std::path::Path;
use std::process::Command;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use weather_contracts::JsonValue;

use super::{
    NativeMaintenanceHistoricalCatalogInput, NativeMaintenanceHistoricalCatalogMapping,
    NativeMaintenanceHistoricalCatalogOutcome, NativeMaintenanceHistoricalRetainedMember,
    NativeMaintenanceHistoricalRetainedPage, NativeMaintenanceHistoricalSelection,
    assemble_native_maintenance_historical_catalog, digest, object, string,
};
use crate::native_maintenance_cold_remote_documents::verify_native_revision_cold_start_document;
use crate::{
    MaintenanceCanonicalJson, NativeRevisionColdCursor, NativeWirePrimitives,
    VerifiedNativeRevisionColdStart,
};

const CUTOFF: &str = "2026-10-10T09:00:00.000Z";
const GENESIS_LABEL: &[u8] = b"adjustment-revision-frontier/v1\n0\n";

/// Preserve the source early block before any retained-page inspection.
#[test]
fn blocks_when_durable_cursor_does_not_reach_current_snapshot() {
    let fixture = ready_fixture();
    let outcome =
        assemble_native_maintenance_historical_catalog(NativeMaintenanceHistoricalCatalogInput {
            cursor: NativeRevisionColdCursor {
                archive_commit_ordinal: "3".to_owned(),
                frontier_sha256: digest(b"wrong-frontier"),
            },
            pages: Vec::new(),
            selection: fixture.selection,
            start: fixture.start,
        })
        .expect("blocked outcome");

    assert_eq!(outcome.state(), "blocked");
    assert_eq!(outcome.reason(), Some("semantic_input_blocked"));
}

/// Assemble the complete two-page frontier and select every current semantic pointer.
#[test]
fn assembles_complete_current_catalog_from_verified_retained_pages() {
    let fixture = ready_fixture();
    let expected_payloads = fixture.payloads.clone();
    let expected_receipts = fixture.receipt_sha256s.clone();
    let outcome =
        assemble_native_maintenance_historical_catalog(NativeMaintenanceHistoricalCatalogInput {
            cursor: fixture.cursor,
            pages: fixture.pages,
            selection: fixture.selection,
            start: fixture.start,
        })
        .expect("ready catalog");
    let NativeMaintenanceHistoricalCatalogOutcome::Ready(ready) = outcome else {
        panic!("fixture must produce a ready catalog");
    };

    assert_eq!(ready.history.receipt_count, 4);
    assert_eq!(ready.catalog.frontier_count, 4);
    assert_eq!(ready.catalog.kind_counts.actual_best_match, 1);
    assert_eq!(ready.catalog.kind_counts.native_source, 1);
    assert_eq!(ready.catalog.kind_counts.rain_gate_input, 1);
    assert_eq!(ready.catalog.kind_counts.target_revision, 1);
    assert_eq!(ready.members.len(), 4);
    assert_eq!(
        ready
            .members
            .iter()
            .map(|member| member.payload_bytes.clone())
            .collect::<Vec<_>>(),
        expected_payloads
    );
    assert_eq!(
        ready
            .members
            .iter()
            .map(|member| member.receipt.receiptSha256.clone())
            .collect::<Vec<_>>(),
        expected_receipts
    );
    assert_eq!(
        ready.input_manifest_sha256,
        ready.history.history_root_sha256
    );
}

struct ReadyFixture {
    cursor: NativeRevisionColdCursor,
    pages: Vec<NativeMaintenanceHistoricalRetainedPage>,
    payloads: Vec<Vec<u8>>,
    receipt_sha256s: Vec<String>,
    selection: NativeMaintenanceHistoricalSelection,
    start: VerifiedNativeRevisionColdStart,
}

/// Build source-encoded projections and exact receipt, snapshot, start, and checkpoint identities.
fn ready_fixture() -> ReadyFixture {
    let payloads = predecessor_payloads();
    let projection_kinds = [
        "actual_best_match",
        "native_source",
        "rain_gate_input",
        "target_revision",
    ];
    let payload_kinds = [
        "adjustment-revision-projection/v1",
        "adjustment-revision-projection/v1",
        "adjustment-rain-gate-feature-projection/v2",
        "adjustment-revision-projection/v1",
    ];
    let mut receipts = Vec::new();
    let mut receipt_bytes = Vec::new();
    let mut frontier = digest(GENESIS_LABEL);
    // build one exact direct database receipt for every semantic producer class
    for (index, projection_kind) in projection_kinds.iter().enumerate() {
        let payload_identity = digest(&payloads[index]);
        let (receipt, bytes) = revision_receipt(
            index as u64 + 1,
            projection_kind,
            &payload_identity,
            &frontier,
        );
        frontier = text_field(&receipt, "frontierSha256");
        receipts.push(receipt);
        receipt_bytes.push(bytes);
    }
    let snapshot = serving_snapshot(&receipts, &projection_kinds, &frontier);
    let start = cold_start(snapshot, &frontier);
    let entries = (0..4)
        .map(|index| {
            checkpoint_entry(
                &payloads[index],
                payload_kinds[index],
                &receipts[index],
                &receipt_bytes[index],
                index,
            )
        })
        .collect::<Vec<_>>();
    let first_page_sha256 = digest(b"historical-page-one");
    let second_page_sha256 = digest(b"historical-page-two");
    let start_member_sha256 = digest(&start.canonical_bytes);
    let first_checkpoint = checkpoint(
        "0",
        &digest(GENESIS_LABEL),
        &entries[..2],
        false,
        "2",
        &text_field(&receipts[1], "frontierSha256"),
        &first_page_sha256,
        &start.start_sha256,
        Some(&start_member_sha256),
        &start,
    );
    let second_checkpoint = checkpoint(
        "2",
        &text_field(&receipts[1], "frontierSha256"),
        &entries[2..],
        true,
        "4",
        &frontier,
        &second_page_sha256,
        &first_page_sha256,
        None,
        &start,
    );
    let graph_one = digest(b"historical-graph-one");
    let graph_two = digest(b"historical-graph-two");
    let first_mapping = NativeMaintenanceHistoricalCatalogMapping {
        graph_manifest_sha256: graph_one,
        page_sha256: first_page_sha256.clone(),
        start_sha256: start.start_sha256.clone(),
    };
    let second_mapping = NativeMaintenanceHistoricalCatalogMapping {
        graph_manifest_sha256: graph_two,
        page_sha256: second_page_sha256.clone(),
        start_sha256: start.start_sha256.clone(),
    };
    let mut first_members = vec![
        retained(
            &first_page_sha256,
            "adjustment-revision-cold-page-checkpoint/v1",
            first_checkpoint,
        ),
        retained(
            &start.start_sha256,
            "adjustment-revision-cold-transfer-start/v1",
            start.canonical_bytes.clone(),
        ),
    ];
    let mut second_members = vec![retained(
        &second_page_sha256,
        "adjustment-revision-cold-page-checkpoint/v1",
        second_checkpoint,
    )];
    // retain receipt and payload members beside the page that introduced them
    for index in 0..4 {
        let target = if index < 2 {
            &mut first_members
        } else {
            &mut second_members
        };
        target.push(retained(
            &text_field(&receipts[index], "receiptSha256"),
            "adjustment-revision-commit-receipt/v1",
            receipt_bytes[index].clone(),
        ));
        target.push(retained(
            &digest(&payloads[index]),
            payload_kinds[index],
            payloads[index].clone(),
        ));
    }
    let receipt_sha256s = receipts
        .iter()
        .map(|receipt| text_field(receipt, "receiptSha256"))
        .collect::<Vec<_>>();
    ReadyFixture {
        cursor: NativeRevisionColdCursor {
            archive_commit_ordinal: "4".to_owned(),
            frontier_sha256: frontier,
        },
        pages: vec![
            NativeMaintenanceHistoricalRetainedPage {
                mapping: first_mapping,
                members: first_members,
            },
            NativeMaintenanceHistoricalRetainedPage {
                mapping: second_mapping,
                members: second_members,
            },
        ],
        payloads,
        selection: NativeMaintenanceHistoricalSelection {
            family: None,
            from_at: None,
            receipt_sha256s: receipt_sha256s.clone(),
            to_at: None,
        },
        receipt_sha256s,
        start,
    }
}

/// Ask the installed predecessor encoders for four genuine projection bodies.
fn predecessor_payloads() -> Vec<Vec<u8>> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.ancestors().nth(3).expect("workspace root");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_PAYLOADS])
        .current_dir(root)
        .output()
        .expect("run predecessor projection fixture");
    assert!(
        output.status.success(),
        "predecessor payload fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice::<Vec<String>>(&output.stdout)
        .expect("decode payload list")
        .into_iter()
        .map(|encoded| STANDARD.decode(encoded).expect("decode payload base64"))
        .collect()
}

/// Build one exact identity-bearing database receipt and its canonical bytes.
fn revision_receipt(
    ordinal: u64,
    projection_kind: &str,
    projection_identity: &str,
    predecessor_frontier: &str,
) -> (JsonValue, Vec<u8>) {
    let committed_at = format!("2026-10-10T08:0{ordinal}:00.000Z");
    let stage = digest(format!("stage-{ordinal}").as_bytes());
    let receipt_sha256 = digest(
        [
            "adjustment-revision-commit-receipt/v1",
            &ordinal.to_string(),
            &committed_at,
            projection_kind,
            projection_identity,
            projection_identity,
            &stage,
            predecessor_frontier,
        ]
        .join("\n")
        .as_bytes(),
    );
    let frontier_sha256 = digest(
        format!(
            "adjustment-revision-frontier/v1\n{predecessor_frontier}\n{ordinal}\n{receipt_sha256}"
        )
        .as_bytes(),
    );
    let value = object(vec![
        ("archiveCommitOrdinal", string(&ordinal.to_string())),
        ("archiveCommittedAt", string(&committed_at)),
        (
            "contractVersion",
            string("adjustment-revision-commit-receipt/v1"),
        ),
        ("frontierSha256", string(&frontier_sha256)),
        ("predecessorFrontierSha256", string(predecessor_frontier)),
        ("projectionIdentitySha256", string(projection_identity)),
        ("projectionKind", string(projection_kind)),
        ("projectionSha256", string(projection_identity)),
        ("receiptSha256", string(&receipt_sha256)),
        ("stageReceiptSha256", string(&stage)),
    ]);
    let bytes = canonical(&value);
    (value, bytes)
}

/// Build the current serving snapshot over the exact four ordered receipts.
fn serving_snapshot(receipts: &[JsonValue], kinds: &[&str], frontier: &str) -> JsonValue {
    let entries = receipts
        .iter()
        .zip(kinds)
        .enumerate()
        .map(|(index, (receipt, kind))| {
            let relation = match *kind {
                "native_source" => "forecast_anchor_records",
                "rain_gate_input" => "rain_adjustment_runs",
                _ => "weather_records",
            };
            object(vec![
                (
                    "logicalReceivedAt",
                    string(&format!("2026-10-10T08:1{index}:00.000Z")),
                ),
                ("receipt", receipt.clone()),
                ("relation", string(relation)),
            ])
        })
        .collect::<Vec<_>>();
    let identities = receipts
        .iter()
        .map(|receipt| text_field(receipt, "receiptSha256"))
        .collect::<Vec<_>>();
    let snapshot_sha256 = digest(
        format!(
            "adjustment-revision-serving-snapshot/v1\n{CUTOFF}\n4\n{frontier}\n{}",
            identities.join("\n")
        )
        .as_bytes(),
    );
    object(vec![
        ("archiveCommitOrdinal", string("4")),
        (
            "contractVersion",
            string("adjustment-revision-serving-snapshot/v1"),
        ),
        ("cutoffAt", string(CUTOFF)),
        ("entries", JsonValue::Array(entries)),
        ("entryCount", JsonValue::Number(4.0)),
        ("frontierSha256", string(frontier)),
        ("snapshotSha256", string(&snapshot_sha256)),
    ])
}

/// Build and reverify one exact cutoff-bound transfer start.
fn cold_start(snapshot: JsonValue, frontier: &str) -> VerifiedNativeRevisionColdStart {
    let unsigned = object(vec![
        (
            "contractVersion",
            string("adjustment-revision-cold-transfer-start/v1"),
        ),
        ("servingSnapshot", snapshot),
        ("watermarkArchiveCommitOrdinal", string("4")),
        ("watermarkFrontierSha256", string(frontier)),
    ]);
    let start_sha256 = digest(&canonical(&unsigned));
    let JsonValue::Object(mut fields) = unsigned else {
        unreachable!("fixture object")
    };
    fields.push((
        "startSha256".encode_utf16().collect(),
        string(&start_sha256),
    ));
    let value = JsonValue::Object(fields);
    let bytes = canonical(&value);
    verify_native_revision_cold_start_document(
        &bytes,
        &mut NativeWirePrimitives::new(),
        &NativeWirePrimitives::new(),
    )
    .expect("verify fixture start")
}

/// Build one compact singular checkpoint entry for an actual retained payload.
fn checkpoint_entry(
    payload: &[u8],
    payload_kind: &str,
    receipt: &JsonValue,
    receipt_bytes: &[u8],
    index: usize,
) -> JsonValue {
    let receipt_sha256 = text_field(receipt, "receiptSha256");
    let successor = digest(format!("successor-{index}").as_bytes());
    object(vec![
        ("payloadIdentitySha256", string(&digest(payload))),
        ("payloadKind", string(payload_kind)),
        ("publicationDisposition", string("published")),
        (
            "publicationIdentitySha256",
            string(&digest(format!("publication-{index}").as_bytes())),
        ),
        (
            "publicationMemberSha256",
            string(&digest(format!("publication-member-{index}").as_bytes())),
        ),
        ("receiptMemberSha256", string(&digest(receipt_bytes))),
        ("receiptSha256", string(&receipt_sha256)),
        (
            "stageReceiptMemberSha256",
            string(&digest(format!("stage-member-{index}").as_bytes())),
        ),
        (
            "stageReceiptSha256",
            string(&text_field(receipt, "stageReceiptSha256")),
        ),
        ("successorMemberSha256", string(&successor)),
        ("successorSha256", string(&successor)),
    ])
}

/// Build one source-valid compact checkpoint over at most two entries.
#[allow(clippy::too_many_arguments)]
fn checkpoint(
    after_ordinal: &str,
    after_frontier: &str,
    entries: &[JsonValue],
    eof: bool,
    next_ordinal: &str,
    next_frontier: &str,
    page_sha256: &str,
    previous_page_sha256: &str,
    start_member_sha256: Option<&str>,
    start: &VerifiedNativeRevisionColdStart,
) -> Vec<u8> {
    let root_entries = entries
        .iter()
        .map(|entry| {
            object(vec![
                (
                    "payloadMemberSha256",
                    string(&text_field(entry, "payloadIdentitySha256")),
                ),
                (
                    "publicationDisposition",
                    string(&text_field(entry, "publicationDisposition")),
                ),
                (
                    "publicationIdentitySha256",
                    string(&text_field(entry, "publicationIdentitySha256")),
                ),
                (
                    "publicationMemberSha256",
                    string(&text_field(entry, "publicationMemberSha256")),
                ),
                (
                    "receiptMemberSha256",
                    string(&text_field(entry, "receiptMemberSha256")),
                ),
                (
                    "stageReceiptMemberSha256",
                    string(&text_field(entry, "stageReceiptMemberSha256")),
                ),
                (
                    "successorMemberSha256",
                    string(&text_field(entry, "successorMemberSha256")),
                ),
            ])
        })
        .collect::<Vec<_>>();
    let member_root_sha256 = digest(&canonical(&object(vec![
        ("entries", JsonValue::Array(root_entries)),
        (
            "startMemberSha256",
            start_member_sha256.map_or(JsonValue::Null, string),
        ),
    ])));
    canonical(&object(vec![
        ("afterArchiveCommitOrdinal", string(after_ordinal)),
        ("afterFrontierSha256", string(after_frontier)),
        (
            "contractVersion",
            string("adjustment-revision-cold-page-checkpoint/v1"),
        ),
        ("entries", JsonValue::Array(entries.to_vec())),
        ("eof", JsonValue::Boolean(eof)),
        ("memberRootSha256", string(&member_root_sha256)),
        ("nextArchiveCommitOrdinal", string(next_ordinal)),
        ("nextFrontierSha256", string(next_frontier)),
        ("pageSha256", string(page_sha256)),
        ("previousPageSha256", string(previous_page_sha256)),
        (
            "startMemberSha256",
            start_member_sha256.map_or(JsonValue::Null, string),
        ),
        ("startSha256", string(&start.start_sha256)),
        ("watermarkArchiveCommitOrdinal", string("4")),
        (
            "watermarkFrontierSha256",
            string(&start.watermark_frontier_sha256),
        ),
    ]))
}

/// Construct one retained member fixture.
fn retained(
    identity_sha256: &str,
    kind: &str,
    payload: Vec<u8>,
) -> NativeMaintenanceHistoricalRetainedMember {
    NativeMaintenanceHistoricalRetainedMember {
        identity_sha256: identity_sha256.to_owned(),
        kind: kind.to_owned(),
        payload,
    }
}

/// Canonically encode one fixture through the production owner.
fn canonical(value: &JsonValue) -> Vec<u8> {
    NativeWirePrimitives
        .encode_canonical_json(value)
        .expect("encode fixture")
}

/// Read one known string field from a fixture object.
fn text_field(value: &JsonValue, name: &str) -> String {
    crate::maintenance_canonical_json::text(value, name).expect("fixture text")
}

const NODE_PAYLOADS: &str = r#"
import {
  encodeAdjustmentRainGateFeatureProjection,
  encodeAdjustmentRevisionProjection,
  encodeMaintenanceBinary64,
} from "./packages/forecast-adjustment/dist/index.js";

const HASH = "a".repeat(64);
const SECOND = "b".repeat(64);
const actualSource = {
  adapterVersion: "open-meteo/v4",
  contractEpoch: "forecast/v4",
  dataset: "best_match",
  providerKey: "open-meteo",
  sourceConfigFingerprint: "open-meteo-forecast/v4",
  sourceId: "7",
  sourceKey: "open-meteo-forecast",
  sourceKind: "forecast",
  upstreamModel: "best_match",
};
const row = {
  apparentTemperatureC64: encodeMaintenanceBinary64(10),
  blackGlobeTemperatureC64: null,
  cloudCoverPercent64: encodeMaintenanceBinary64(50),
  contentSha256: HASH,
  pm25MicrogramsPerCubicMeter64: null,
  precipitationMm64: encodeMaintenanceBinary64(0),
  precipitationRateMmPerHour64: null,
  pressureHpa64: encodeMaintenanceBinary64(1010),
  relativeHumidityPercent64: encodeMaintenanceBinary64(80),
  soilElectricalConductivityMicrosiemensPerCm64: null,
  soilMoisturePercent64: null,
  solarRadiationWm264: null,
  temperatureC64: encodeMaintenanceBinary64(11),
  uvIndex64: null,
  validAt: "2026-10-10T08:30:00.000Z",
  waterLevelM64: null,
  wetBulbGlobeTemperatureC64: null,
  windDirectionDegrees64: encodeMaintenanceBinary64(180),
  windGustMps64: encodeMaintenanceBinary64(8),
  windSpeedMps64: encodeMaintenanceBinary64(5),
};
const actual = {
  contractVersion: "adjustment-revision-projection/v1",
  family: "shared",
  logicalKey: {
    productRunAt: "2026-10-10T08:00:00.000Z",
    sourceId: "7",
    sourceKind: "forecast",
    validAt: "2026-10-10T08:30:00.000Z",
  },
  logicalReceivedAt: "2026-10-10T08:05:00.000Z",
  projectionKind: "actual_best_match",
  rows: [row],
  source: actualSource,
  storedContentSha256: HASH,
};
const anchor = {
  contractVersion: "adjustment-revision-projection/v1",
  family: "wind",
  logicalKey: {
    contentSha256: null,
    leadHours: 1,
    providerResponseSha256: null,
    runInitializedAt: null,
    siteId: null,
    sourceId: "7",
    sourceType: "forecast_anchor",
    validAt: "2026-10-10T08:30:00.000Z",
  },
  logicalReceivedAt: "2026-10-10T08:05:00.000Z",
  projectionKind: "native_source",
  rows: [{
    apparentTemperatureC64: encodeMaintenanceBinary64(10),
    cloudCoverPercent64: encodeMaintenanceBinary64(50),
    contentSha256: HASH,
    leadHours: 1,
    precipitationMm64: encodeMaintenanceBinary64(0),
    pressureHpa64: encodeMaintenanceBinary64(1010),
    relativeHumidityPercent64: encodeMaintenanceBinary64(80),
    temperatureC64: encodeMaintenanceBinary64(11),
    validAt: "2026-10-10T08:30:00.000Z",
    windDirectionDegrees64: encodeMaintenanceBinary64(180),
    windGustMps64: encodeMaintenanceBinary64(8),
    windSpeedMps64: encodeMaintenanceBinary64(5),
  }],
  source: actualSource,
  storedContentSha256: HASH,
};
const target = {
  ...actual,
  logicalKey: {
    productRunAt: null,
    sourceId: "7",
    sourceKind: "physical_sensor",
    validAt: "2026-10-10T08:30:00.000Z",
  },
  projectionKind: "target_revision",
  source: { ...actualSource, upstreamModel: null },
};
const rain = {
  contractVersion: "adjustment-rain-gate-feature-projection/v2",
  family: "rain",
  logicalKey: {
    inputSha256: HASH,
    modelSha256: SECOND,
    runInitializedAt: "2026-10-10T08:00:00.000Z",
  },
  logicalReceivedAt: "2026-10-10T08:05:00.000Z",
  projectionKind: "rain_gate_input",
  rows: Array.from({ length: 23 }, (_unused, index) => ({
    features64: Array.from({ length: 107 }, () => null),
    modelLeadHours: index + 9,
    rawPrecipitationMm64: encodeMaintenanceBinary64(0),
    rawTargetHourTemperatureC64: encodeMaintenanceBinary64(10),
    validAt: new Date(Date.parse("2026-10-10T08:00:00.000Z") +
      (index + 9) * 3600000).toISOString(),
  })),
  source: {
    adapterVersion: "rain-hurdle-wind-features/v1",
    contractEpoch: "rain-prospective-capture/v1",
    dataset: "ecmwf_ifs",
    providerKey: "open-meteo-single-runs",
    sourceConfigFingerprint:
      "96e4365f74a0fb8694944172b73a65c29d89a68ddf86b1b0a50db38bb39d52aa",
    sourceId: "claim-1",
    sourceKey: "rain-prospective-forecast",
    sourceKind: "forecast",
    upstreamModel: "ecmwf_ifs",
  },
  storedContentSha256: HASH,
};
const encoded = [
  encodeAdjustmentRevisionProjection(actual),
  encodeAdjustmentRevisionProjection(anchor),
  encodeAdjustmentRainGateFeatureProjection(rain),
  encodeAdjustmentRevisionProjection(target),
].map((bytes) => bytes.toString("base64"));
process.stdout.write(JSON.stringify(encoded));
"#;
