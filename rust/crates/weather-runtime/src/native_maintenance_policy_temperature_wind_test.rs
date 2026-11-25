use super::*;
use std::{path::Path, process::Command};

/// Compare actual predecessor row hashes, date-balanced bootstrap reports and policy gates.
#[test]
fn native_temperature_wind_maintenance_policy_matches_source_reports() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let result=Command::new("node").current_dir(repo).args(["--input-type=module","--eval",r#"
import {createMaintenanceEvaluationRow,evaluateTemperatureMaintenancePromotion,evaluateTemperatureMaintenanceRegression,evaluateWindMaintenancePromotion,evaluateWindMaintenanceRegression,WIND_MAINTENANCE_PAIR_KEYS} from "./scripts/research/adjustment-maintenance-runtime/forecast/maintenance-policy.js";
import {canonicalJsonBytes} from "./scripts/research/adjustment-maintenance-runtime/forecast/candidate.js";
import {localCalendarFeaturesFor} from "./scripts/research/adjustment-maintenance-runtime/forecast/calendar.js";
const epoch={registeredAt:"2025-11-01T12:00:00.000Z",epochStartInclusive:"2025-12-01T12:00:00.000Z",epochEndExclusive:"2026-03-01T12:00:00.000Z",evaluatedAt:"2026-03-10T12:00:00.000Z"};
for(const family of ["temperature","wind"]){
 const pairs=family==="wind"?WIND_MAINTENANCE_PAIR_KEYS:[null,null,null];
 const rows=[];
 // preserve a nontrivial order and unequal per-date row losses
 for(const day of [4,1,7,2,8,5,3,6,9]){
  for(const [index,pair] of pairs.entries()){
   const validAt=new Date(Date.UTC(2026,0,day,18,0,0)).toISOString();
   const target=2+(day%4)+index/8;
   const row=createMaintenanceEvaluationRow({
    actualBestMatchPrediction:target+(index%2?0.8:-0.7),applied:index%3===0,
    candidatePrediction:target+(day%2?0.35:-0.45)+(index%3)/16,
    evidenceClass:"prospective_receipt",family,farmTarget:index%2===0?target+0.2:null,
    firstEdgeCommittedAt:new Date(Date.parse(validAt)-1800000).toISOString(),
    horizonHours:family==="temperature"?1+index*4:1+index*8,
    incumbentPrediction:target+(day%3?1.1:-1.3),key:`${family}/${day}/${index}`,
    localDate:localCalendarFeaturesFor(validAt).localDate,nativeSourcePrediction:target-(index%2?0.7:1.8),
    nearestThree:index%2===0,pairKey:pair,provenanceComplete:true,providerFamily:index%2===0?"tempest":"ecowitt",
    sourceReceiptAt:new Date(Date.parse(validAt)-3600000).toISOString(),sourceRowSha256:"a".repeat(64),
    stationKey:`station-${index%3}`,target,targetRowSha256:"b".repeat(64),validAt
   });
   rows.push(row);
  }
 }
 const policies=family==="temperature"?[evaluateTemperatureMaintenancePromotion(rows),evaluateTemperatureMaintenanceRegression(rows,epoch)]:[evaluateWindMaintenancePromotion(rows),evaluateWindMaintenanceRegression(rows,epoch)];
 process.stdout.write(JSON.stringify({family,rows,epoch,reports:policies.map(report=>Buffer.from(canonicalJsonBytes(report)).toString("base64"))})+"\n");
}
"#]).output().expect("source fixture process");
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let lines = String::from_utf8(result.stdout).unwrap();
    assert_eq!(lines.lines().count(), 2);
    // compare full exact canonical report bytes for both model families and both policy modes
    for line in lines.lines() {
        let fixture = NativeWirePrimitives.decode_json(line.as_bytes()).unwrap();
        let family = match text(&fixture, "family").as_deref() {
            Some("temperature") => NativeMaintenanceCaptureFamily::Temperature,
            Some("wind") => NativeMaintenanceCaptureFamily::Wind,
            _ => panic!("fixture family"),
        };
        let JsonValue::Array(rows) =
            crate::maintenance_canonical_json::field(&fixture, "rows").unwrap()
        else {
            panic!("fixture rows")
        };
        let epoch = crate::maintenance_canonical_json::field(&fixture, "epoch").unwrap();
        let JsonValue::Array(reports) =
            crate::maintenance_canonical_json::field(&fixture, "reports").unwrap()
        else {
            panic!("fixture reports")
        };
        for (index, kind) in [
            NativeMaintenancePolicyKind::Promotion,
            NativeMaintenancePolicyKind::Regression,
        ]
        .into_iter()
        .enumerate()
        {
            let actual = evaluate_native_temperature_wind_maintenance_policy(
                family,
                kind,
                rows,
                (index == 1).then_some(epoch),
            )
            .expect("native policy");
            let JsonValue::String(expected) = &reports[index] else {
                panic!("fixture report")
            };
            let expected = NativeWirePrimitives
                .decode_standard_base64(&String::from_utf16(expected).unwrap())
                .unwrap();
            assert_eq!(
                NativeWirePrimitives.encode_sorted_json(&actual).unwrap(),
                expected,
                "{family:?} {kind:?}"
            );
        }
    }
}
