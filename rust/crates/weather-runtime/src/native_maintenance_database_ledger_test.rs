use super::*;
use std::process::Command;

/// Compare each three admitted rolling ledger generations against the actual Node projector.
#[test]
fn native_rolling_ledgers_match_actual_predecessor_projection() {
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap();
    let script = r#"
import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
const root=process.argv[2], migrationRoot=join(root,"packages/database/migrations");
const {projectAdjustmentMaintenanceDatabaseLedgerV3}=await import(pathToFileURL(join(root,"deploy/scripts/adjustment-evaluation-package.mjs")));
const {canonicalJsonBytes}=await import(pathToFileURL(join(root,"scripts/research/adjustment_plaintext_archive.mjs")));
const hash=(value)=>createHash("sha256").update(value).digest("hex");
const names=readdirSync(migrationRoot).filter(name=>name.endsWith(".sql")).sort();
const checksums=names.map(name=>hash(readFileSync(join(migrationRoot,name))));
const transaction={created_at_utc:"2026-10-08T12:00:01.000Z",idle_in_transaction_session_timeout:"30s",isolation_level:"repeatable read",lock_timeout:"5s",read_only:"on",statement_timeout:"5min"};
for (const n of [21,22,23]) {
  const prefixNames=names.slice(0,n), prefixChecksums=checksums.slice(0,n);
  const databaseManifest={
    contract_version:"adjustment-evaluation-export-manifest/v1",
    migration_checksums:prefixChecksums,
    migration_history_sha256:hash(prefixNames.map((name,index)=>name+":"+prefixChecksums[index]).join("\n")),
    migration_names:prefixNames,
    query_contract_sha256:"c860039c72818a9b813ed9f9d93f5d8e115f4144e99d698734e9159ca3457bd4",
    query_contract_version:"adjustment-evaluation-export-query/v1",
    row_schema_sha256:"c21782034130d003e4af48fada4c8f6545f6f1c4dff550f92d0523c7d96d20e2",
    schema_migration:"0017_adjustment_evaluation_export.sql",site_key:"ballydidean",site_timezone:"America/Los_Angeles",
  };
  const envelope={databaseManifest,payload:null,transaction};
  process.stdout.write(JSON.stringify(envelope)+"\n");
  process.stdout.write(canonicalJsonBytes(projectAdjustmentMaintenanceDatabaseLedgerV3(envelope)));
}
"#;
    let output = Command::new("node")
        .args(["--input-type=module", "-e", script, "weather-ledger-oracle"])
        .arg(repository)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = output
        .stdout
        .split_inclusive(|byte| *byte == b'\n')
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), EXPECTED.len() * 2);
    // compare the exact predecessor version, history and snapshot clock for every supported generation
    for (pair, (_, expected)) in lines.chunks_exact(2).zip(EXPECTED) {
        let projected = project_native_maintenance_database_ledger_v3(pair[0]).unwrap();
        assert_eq!(projected, pair[1]);
        let receipt = read_native_maintenance_database_ledger_bytes(&projected).unwrap();
        assert_eq!(receipt.contract_version, LEDGER_VERSION);
        assert_eq!(receipt.migration_history_sha256, *expected);
        assert_eq!(receipt.snapshot_at, "2026-10-08T12:00:01.000Z");
    }
}
