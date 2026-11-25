use std::path::Path;
use std::process::Command;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde_json::Value;
use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, JsonValue,
};

use super::*;
use crate::{
    NativeMaintenanceColdCatalog, NativeMaintenanceColdCatalogKindCounts,
    NativeMaintenanceHistoricalArchiveIndex, NativeMaintenanceHistoricalFamily,
    NativeMaintenanceHistoricalOccurrence, NativeWirePrimitives,
    VerifiedNativeMaintenanceCaptureEpochWitness,
    build_native_adjustment_historical_fit_projection,
};

/// Match the predecessor's authentic temperature join and exact fit-input identity.
#[test]
fn monthly_temperature_fit_matches_predecessor_join_and_hash() {
    let fixture = predecessor_fixture();
    let witness = witness(&fixture);
    let historical = build_native_adjustment_historical_fit_projection(
        &witness,
        NativeMaintenanceHistoricalFamily::Temperature,
        "2026-12",
        &history(&fixture),
    )
    .unwrap();
    let outcome = build_native_adjustment_monthly_fit_assembly(
        "2026-12",
        &witness,
        NativeMaintenanceHistoricalFamily::Temperature,
        &historical,
        Some(&NativeMaintenanceTemperatureIncumbent {
            artifact_bytes: b"compiled".to_vec(),
            artifact_identity_sha256: "a".repeat(64),
            authority_kind: "maintenance_qualified".to_owned(),
            family: NativeMaintenanceHistoricalFamily::Temperature,
            model: object(vec![(
                "contractVersion",
                string("temperature-permanent-model/v1"),
            )]),
            state: "active".to_owned(),
        }),
    )
    .unwrap();
    let NativeMaintenanceMonthlyFitOutcome::Ready(assembly) = outcome else {
        panic!("authentic fixture must be ready");
    };
    let fit_bytes = NativeWirePrimitives
        .encode_sorted_json(&assembly.fit_input)
        .unwrap();
    assert_eq!(
        fit_bytes,
        STANDARD
            .decode(string_field(&fixture, "fitInputBase64"))
            .unwrap()
    );
    assert_eq!(
        assembly.input_manifest_sha256,
        string_field(&fixture, "inputManifestSha256")
    );
    let Some(JsonValue::Array(development_rows)) = field(&assembly.fit_input, "developmentRows")
    else {
        panic!("temperature development rows");
    };
    assert_eq!(development_rows.len(), 12);
}

/// Preserve the genuine immutable-cutoff no-candidate reason before incumbent loading.
#[test]
fn monthly_fit_returns_population_gap_for_missing_required_class() {
    let fixture = predecessor_fixture();
    let witness = witness(&fixture);
    let mut historical = build_native_adjustment_historical_fit_projection(
        &witness,
        NativeMaintenanceHistoricalFamily::Temperature,
        "2026-12",
        &history(&fixture),
    )
    .unwrap();
    historical
        .projection_members
        .retain(|member| member.projection_kind == "actual_best_match");
    let outcome = build_native_adjustment_monthly_fit_assembly(
        "2026-12",
        &witness,
        NativeMaintenanceHistoricalFamily::Temperature,
        &historical,
        None,
    )
    .unwrap();
    assert_eq!(
        outcome,
        NativeMaintenanceMonthlyFitOutcome::NoCandidate {
            historical_member_root_sha256: historical.member_root_sha256,
            reason: NativeMaintenanceMonthlyNoCandidateReason::IncompleteDevelopmentPopulation,
        }
    );
}

/// Ask the installed predecessor assemblers for authentic occurrence and fit bytes.
fn predecessor_fixture() -> Value {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.ancestors().nth(3).expect("workspace root");
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", NODE_ORACLE])
        .current_dir(root)
        .output()
        .expect("run monthly fit predecessor fixture");
    assert!(
        output.status.success(),
        "monthly fit predecessor failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("decode monthly fit fixture")
}

/// Build the typed verified history boundary from predecessor occurrences.
fn history(fixture: &Value) -> NativeMaintenanceHistoricalArchiveIndex {
    let occurrences = fixture["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .map(|occurrence| NativeMaintenanceHistoricalOccurrence {
            graph_manifest_sha256: string_field(occurrence, "graphManifestSha256").to_owned(),
            page_sha256: "9".repeat(64),
            payload_bytes: STANDARD
                .decode(string_field(occurrence, "payloadBase64"))
                .unwrap(),
            payload_identity_sha256: string_field(occurrence, "payloadIdentitySha256").to_owned(),
            payload_kind: string_field(occurrence, "payloadKind").to_owned(),
            publication_disposition: "published".to_owned(),
            receipts: vec![receipt(&occurrence["receipt"])],
        })
        .collect();
    NativeMaintenanceHistoricalArchiveIndex {
        catalog: NativeMaintenanceColdCatalog {
            archive_commit_ordinal: "5".to_owned(),
            catalog_root_sha256: "1".repeat(64),
            frontier_count: 5,
            frontier_root_sha256: "2".repeat(64),
            kind_counts: NativeMaintenanceColdCatalogKindCounts {
                actual_best_match: 1,
                native_source: 1,
                rain_gate_input: 0,
                target_revision: 3,
            },
            receipt_root_sha256: "3".repeat(64),
            serving_snapshot_sha256: "4".repeat(64),
        },
        contract_version: "adjustment-revision-historical-archive-index/v1",
        history_root_sha256: string_field(fixture, "historyRootSha256").to_owned(),
        occurrences,
        pages: Vec::new(),
        receipt_count: 5,
    }
}

/// Build one exact generated receipt from the oracle's source fields.
fn receipt(value: &Value) -> AdjustmentRevisionCommitReceipt {
    AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: string_field(value, "archiveCommitOrdinal").to_owned(),
        archiveCommittedAt: string_field(value, "archiveCommittedAt").to_owned(),
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: string_field(value, "frontierSha256").to_owned(),
        predecessorFrontierSha256: string_field(value, "predecessorFrontierSha256").to_owned(),
        projectionIdentitySha256: string_field(value, "projectionIdentitySha256").to_owned(),
        projectionKind: match string_field(value, "projectionKind") {
            "actual_best_match" => {
                AdjustmentRevisionCommitReceiptProjectionKindEnum::ActualBestMatch
            }
            "native_source" => AdjustmentRevisionCommitReceiptProjectionKindEnum::NativeSource,
            "target_revision" => AdjustmentRevisionCommitReceiptProjectionKindEnum::TargetRevision,
            _ => panic!("fixture projection kind"),
        },
        projectionSha256: string_field(value, "projectionSha256").to_owned(),
        receiptSha256: string_field(value, "receiptSha256").to_owned(),
        stageReceiptSha256: string_field(value, "stageReceiptSha256").to_owned(),
    }
}

/// Build the already-verified witness fields consumed by both projectors.
fn witness(fixture: &Value) -> VerifiedNativeMaintenanceCaptureEpochWitness {
    VerifiedNativeMaintenanceCaptureEpochWitness {
        catalog_frontier_sha256: "1".repeat(64),
        control_plane_sha256: "2".repeat(64),
        control_plane_version: "14".to_owned(),
        database_migration_history_sha256: "3".repeat(64),
        epoch_at: "2026-10-10T08:00:00.000Z".to_owned(),
        serving_snapshot_sha256: "4".repeat(64),
        source_commit: "5".repeat(40),
        source_release: "2026.10.10-1".to_owned(),
        source_server_image_digest: format!("sha256:{}", "6".repeat(64)),
        witness_sha256: string_field(fixture, "witnessSha256").to_owned(),
    }
}

/// Borrow one required JSON fixture string.
fn string_field<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name].as_str().expect("fixture string")
}

/// Borrow one generated JSON object field.
fn field<'a>(value: &'a JsonValue, name: &str) -> Option<&'a JsonValue> {
    crate::maintenance_canonical_json::field(value, name)
}

/// Build one generated JSON object.
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(name, value)| (name.encode_utf16().collect(), value))
            .collect(),
    )
}

/// Build one generated JSON string.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

const NODE_ORACLE: &str = r#"
import { createHash } from "node:crypto";
import { FORECAST_OBSERVATION_SOURCE_LINEAGES } from
  "./scripts/research/adjustment-maintenance-runtime/node_modules/@weather/domain/dist/forecast-adjustment.js";
import { encodeAdjustmentRevisionProjection, encodeAdjustmentTemperatureNativeSourceProjection }
  from "./scripts/research/adjustment-maintenance-runtime/forecast/maintenance-revision-projection.js";
import { encodeMaintenanceBinary64 } from
  "./scripts/research/adjustment-maintenance-runtime/forecast/maintenance-shadow-values.js";
import { buildAdjustmentHistoricalFitProjection } from
  "./scripts/research/adjustment_historical_fit_assembler.mjs";
import { buildAdjustmentMonthlyFitAssembly } from
  "./scripts/research/adjustment_monthly_projection.mjs";
import { adjustmentSha256, canonicalJsonBytes } from
  "./scripts/research/adjustment_plaintext_archive.mjs";

const A="a".repeat(64), B="b".repeat(64), epochAt="2026-10-10T08:00:00.000Z";
const frontier=adjustmentSha256(Buffer.from("adjustment-revision-frontier/v1\n0\n"));
const unsigned={activationKind:"inert_v14_pre_activation",archiveCommitOrdinal:"0",
  catalogFrontierSha256:frontier,contractVersion:"adjustment-revision-capture-epoch-witness/v1",
  controlPlaneSha256:A,controlPlaneVersion:"14",databaseMigrationHistorySha256:
  "6de5c8c7efaa448aeb12bf1a9debe6fe7d4d4d1003ee0e21ab619ffa624c3424",epochAt,
  servingSnapshotSha256:adjustmentSha256(Buffer.from(["adjustment-revision-serving-snapshot/v1",
  epochAt,"0",frontier,""].join("\n"))),sourceCommit:"1".repeat(40),sourceRelease:"2026.10.10-1",
  sourceServerImageDigest:`sha256:${A}`,sourceWebImageDigest:`sha256:${B}`};
const epochWitness={...unsigned,witnessSha256:adjustmentSha256(canonicalJsonBytes(unsigned))};
const lineages=[...new Map(FORECAST_OBSERVATION_SOURCE_LINEAGES.filter(x=>x.acceptedEndExclusive===null)
  .map(x=>[x.physicalStationKey,x])).values()].slice(0,3);
const run="2026-10-12T08:00:00.000Z";
const row=(validAt,content)=>({apparentTemperatureC64:encodeMaintenanceBinary64(9),blackGlobeTemperatureC64:null,
 cloudCoverPercent64:encodeMaintenanceBinary64(50),contentSha256:content,pm25MicrogramsPerCubicMeter64:null,
 precipitationMm64:encodeMaintenanceBinary64(0),precipitationRateMmPerHour64:null,
 pressureHpa64:encodeMaintenanceBinary64(1010),relativeHumidityPercent64:encodeMaintenanceBinary64(80),
 soilElectricalConductivityMicrosiemensPerCm64:null,soilMoisturePercent64:null,solarRadiationWm264:null,
 temperatureC64:encodeMaintenanceBinary64(10),uvIndex64:null,validAt,waterLevelM64:null,
 wetBulbGlobeTemperatureC64:null,windDirectionDegrees64:encodeMaintenanceBinary64(180),
 windGustMps64:encodeMaintenanceBinary64(8),windSpeedMps64:encodeMaintenanceBinary64(5)});
const forecast={adapterVersion:"open-meteo-forecast-v4",contractEpoch:"forecast-v4",dataset:"best_match",
 providerKey:"open-meteo",sourceConfigFingerprint:A,sourceId:"2",sourceKey:"open-meteo-forecast-v4",
 sourceKind:"forecast",upstreamModel:"best_match"};
const state={b24C:null,b72C:null,cohort:"ecmwf_single_run_hindcast",localDates:0,mad72C:null,
 maximumSourceRunInitializedAt:null,maximumSourceValidAt:null,n24:0,n72:0,sourceKeys:[],supported:false,
 targetRunInitializedAt:run,windowEndValidAt:"2026-10-12T01:00:00.000Z"};
const documents=[], nativeRows=[];
for(let lead=1;lead<=18;lead+=1){
 const validAt=new Date(Date.parse(run)+lead*3600000).toISOString(), eligible=lead>=7;
 const bestContent=adjustmentSha256(Buffer.from(`best-${lead}`));
 nativeRows.push({bestMatchContentSha256:eligible?bestContent:null,
  bestMatchProductRunAt:eligible?"2026-10-12T12:00:00.000Z":null,bestMatchSourceId:eligible?"2":null,
  bestMatchTemperatureC64:eligible?encodeMaintenanceBinary64(10):null,
  contentSha256:adjustmentSha256(Buffer.from(`native-${lead}`)),modelCycle:"50r1",modelLeadHours:lead,
  rawRelativeHumidityPercent64:encodeMaintenanceBinary64(80),rawTemperatureC64:encodeMaintenanceBinary64(11),
  rawWindSpeedMps64:encodeMaintenanceBinary64(5),validAt});
 if(!eligible)continue;
 documents.push({contractVersion:"adjustment-revision-projection/v1",family:"shared",
  logicalKey:{productRunAt:"2026-10-12T12:00:00.000Z",sourceId:"2",sourceKind:"forecast",validAt},
  logicalReceivedAt:"2026-10-12T13:00:00.000Z",projectionKind:"actual_best_match",rows:[row(validAt,bestContent)],
  source:forecast,storedContentSha256:bestContent});
 for(const [i,lineage] of lineages.entries()) { const content=adjustmentSha256(Buffer.from(`target-${lead}-${i}`));
  documents.push({contractVersion:"adjustment-revision-projection/v1",family:"shared",
  logicalKey:{productRunAt:null,sourceId:String(i+10),sourceKind:"physical_sensor",validAt},
  logicalReceivedAt:new Date(Date.parse(validAt)+600000).toISOString(),projectionKind:"target_revision",
  rows:[row(validAt,content)],source:{adapterVersion:lineage.adapterContract,
  contractEpoch:"physical-station-hourly/v1",dataset:"physical_observation",providerKey:"fixture",
  sourceConfigFingerprint:lineage.checkedFingerprint,sourceId:String(i+10),sourceKey:lineage.sourceKey,
  sourceKind:"physical_sensor",upstreamModel:null},storedContentSha256:content}); }
}
documents.push({contractVersion:"adjustment-temperature-native-source-projection/v2",family:"temperature",
 logicalKey:{contentSha256:B,leadHours:null,providerResponseSha256:A,runInitializedAt:run,siteId:"1",
 sourceId:null,sourceType:"ecmwf_temperature_run",validAt:null},logicalReceivedAt:"2026-10-12T14:00:00.000Z",
 projectionKind:"native_source",recentErrorState:state,
 recentErrorStateSha256:createHash("sha256").update(JSON.stringify(state)).digest("hex"),rows:nativeRows,
 source:{...forecast,adapterVersion:"open-meteo-ecmwf-single-run/v1",contractEpoch:"ecmwf-temperature-canary/v1",
 dataset:"single_run",sourceId:"site:1",sourceKey:"open-meteo-ecmwf-single-run:ballydidean",upstreamModel:"ecmwf_ifs"},
 storedContentSha256:B});
const occurrences=documents.map((document,index)=>{const payloadBytes=document.contractVersion.includes("temperature-native")?
 encodeAdjustmentTemperatureNativeSourceProjection(document):encodeAdjustmentRevisionProjection(document);
 const id=adjustmentSha256(payloadBytes),ordinal=String(index+1),kind=document.projectionKind;
 const receipt={archiveCommitOrdinal:ordinal,archiveCommittedAt:kind==="actual_best_match"?
 "2026-10-12T13:30:00.000Z":kind==="native_source"?"2026-10-12T14:05:00.000Z":
 "2026-10-12T15:30:00.000Z",contractVersion:"adjustment-revision-commit-receipt/v1",frontierSha256:A,
 predecessorFrontierSha256:B,projectionIdentitySha256:id,projectionKind:kind,projectionSha256:id,
 receiptSha256:adjustmentSha256(Buffer.from(`receipt-${ordinal}`)),
 stageReceiptSha256:adjustmentSha256(Buffer.from(`stage-${ordinal}`))};
 return {graphManifestSha256:adjustmentSha256(Buffer.from(`graph-${ordinal}`)),payloadBytes,
 payloadIdentitySha256:id,payloadKind:document.contractVersion,publicationDisposition:"published",receipts:[receipt]};});
const historyRootSha256=adjustmentSha256(Buffer.from("history"));
const historicalProjection=buildAdjustmentHistoricalFitProjection({epochWitness,family:"temperature",fitMonth:"2026-12",
 history:{catalog:{},contractVersion:"adjustment-revision-historical-archive-index/v1",historyRootSha256,
 occurrences,pages:[],receiptCount:occurrences.length}});
const assembly=buildAdjustmentMonthlyFitAssembly({dueMonth:"2026-12",epochWitness,family:"temperature",
 historicalProjection,incumbent:{artifactBytes:Buffer.from("compiled"),artifactIdentitySha256:A,
 authorityKind:"maintenance_qualified",candidate:null,comparatorAuthorityBytes:Buffer.from("authority"),
 family:"temperature",model:{contractVersion:"temperature-permanent-model/v1"},reasonCode:null,state:"active"}});
process.stdout.write(JSON.stringify({fitInputBase64:canonicalJsonBytes(assembly.fitInput).toString("base64"),
 historyRootSha256,inputManifestSha256:assembly.inputManifestSha256,witnessSha256:epochWitness.witnessSha256,
 occurrences:occurrences.map(o=>({graphManifestSha256:o.graphManifestSha256,
 payloadBase64:o.payloadBytes.toString("base64"),payloadIdentitySha256:o.payloadIdentitySha256,
 payloadKind:o.payloadKind,receipt:o.receipts[0]}))}));
"#;
