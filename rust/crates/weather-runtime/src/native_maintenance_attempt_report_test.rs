use super::*;
use std::{path::Path, process::Command};

/// Compare all source reason classes and attempt/lease addresses with the retained Node owner.
#[test]
fn native_maintenance_attempt_report_matches_original_reason_matrix() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output=Command::new("node").current_dir(repo).args(["--input-type=module","--eval",r#"
import {validateAdjustmentMaintenanceAttempt} from "./scripts/research/adjustment_maintenance_controller.mjs";
import {canonicalJsonBytes,adjustmentSha256} from "./scripts/research/adjustment_plaintext_archive.mjs";
const reasons=[
["development_candidate_archived","completed",true,true,true,"monthly"],
["control_reference_archived","completed",false,false,true,"monthly"],
["candidate_archive_unavailable","blocked",false,true,true,"monthly"],
["fit_failed","failed",false,false,true,"monthly"],
["candidate_parity_failed","failed",false,true,true,"monthly"],
["monthly_no_candidate","completed",false,false,true,"monthly"],
["daily_candidate_rejected","completed",false,false,true,"daily"],
["daily_no_registered_candidate","completed",false,false,true,"daily"],
["daily_support_failed","completed",false,false,true,"daily"],
["daily_candidate_promoted","completed",false,false,true,"daily"],
["daily_history_pending","blocked",false,false,true,"daily"],
["daily_promotion_pending","blocked",false,false,true,"daily"],
["daily_evaluation_failed","failed",false,false,true,"daily"],
["family_slot_busy","blocked",false,false,true,"monthly"],
["registration_schedule_unavailable","blocked",false,false,true,"monthly"],
["dependency_failed","blocked",false,false,false,"daily"],
["history_unavailable","blocked",false,false,false,"daily"],
["semantic_catalog_unavailable","blocked",false,false,false,"monthly"],
["semantic_input_blocked","blocked",false,false,false,"monthly"],
];
const inputHeadSha256="1".repeat(64),lifecycleHeadSha256="2".repeat(64);
const originalCutoffAt="2026-10-08T07:00:00.000Z";
for(const [reason,state,candidate,fit,semantic,mode] of reasons){
 const dueKey=reason==="control_reference_archived"?"control-reference/rain/2026-11":mode==="daily"?"daily/2026-10-08":"monthly/rain/2026-10";
 const attemptIdentitySha256=adjustmentSha256(canonicalJsonBytes({contractVersion:"forecast-adjustment-maintenance-attempt-identity/v1",dueKey,inputHeadSha256,originalCutoffAt}));
 const report={actionEligible:reason==="daily_candidate_promoted",attemptIdentitySha256,candidateGraphSha256:candidate?"3".repeat(64):null,candidateSha256:candidate?"4".repeat(64):null,contractVersion:"forecast-adjustment-maintenance-attempt/v2",dueKey,family:mode==="monthly"?"rain":null,fitReceiptSha256:fit?"5".repeat(64):null,inputHeadSha256,lifecycleHeadSha256,mode,originalCutoffAt,reason,reservedConfirmationExposed:false,semanticInputSha256:semantic?"6".repeat(64):null,servingChanged:reason==="daily_candidate_promoted",state};
 const bytes=canonicalJsonBytes(validateAdjustmentMaintenanceAttempt(report));
 const runId="maintenance-"+adjustmentSha256(`${dueKey}\n${lifecycleHeadSha256}\n`).slice(0,32);
 process.stdout.write(JSON.stringify({bytes:bytes.toString("base64"),reportSha256:adjustmentSha256(bytes),runId})+"\n");
}
"#]).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = String::from_utf8(output.stdout).unwrap();
    assert_eq!(lines.lines().count(), 19);
    // check the exact canonical archive bytes and both source retry identities
    for line in lines.lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        let bytes = NativeWirePrimitives
            .decode_standard_base64(value["bytes"].as_str().unwrap())
            .unwrap();
        let report = verify_native_maintenance_attempt_report(&bytes).unwrap();
        assert_eq!(
            report.report_sha256,
            value["reportSha256"].as_str().unwrap()
        );
        let due = text(&report.value, "dueKey").unwrap();
        assert_eq!(
            native_maintenance_attempt_identity(
                &due,
                &text(&report.value, "inputHeadSha256").unwrap(),
                &text(&report.value, "originalCutoffAt").unwrap()
            )
            .unwrap(),
            text(&report.value, "attemptIdentitySha256").unwrap()
        );
        assert_eq!(
            native_maintenance_controller_run_id(
                &due,
                &text(&report.value, "lifecycleHeadSha256").unwrap()
            )
            .unwrap(),
            value["runId"].as_str().unwrap()
        );
        let segment = native_maintenance_attempt_archive_segment(&report).unwrap();
        assert_eq!(segment.members.len(), 1);
        assert_eq!(segment.members[0].payload, bytes);
    }
}
