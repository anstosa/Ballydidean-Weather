use std::fs::{self, DirBuilder};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use weather_edge::RevisionSpoolLeaseCodec;

static NEXT: AtomicU64 = AtomicU64::new(0);

/// One freshly allocated, bounded private integration-test directory.
pub struct TempSpool(PathBuf);

impl TempSpool {
    /// Allocate only the fixture's own disposable directory.
    pub fn new() -> Self {
        let base = fs::canonicalize(std::env::temp_dir()).expect("test temp root");
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("test time")
            .as_nanos();
        let path = base.join(format!(
            "weather-native-spool-{}-{timestamp}-{nonce}",
            std::process::id()
        ));
        DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .expect("private spool root");
        Self(path)
    }

    /// Borrow only the fixture-owned explicit archive root.
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempSpool {
    /// Clean up only the newly allocated fixture root.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Use Node's existing UTC/JSON behavior only as a local parity fixture.
pub struct NodeLeaseCodec;

impl RevisionSpoolLeaseCodec for NodeLeaseCodec {
    /// Encode the complete expiry and lease using Node's admitted JSON and UTC behavior.
    fn encode_canonical_lease(&self, milliseconds: i64, token: &str) -> Option<Vec<u8>> {
        eval(
            r#"const value={
contractVersion:"adjustment-revision-spool-lock/v1",
expiresAt:new Date(Number(process.argv[1])).toISOString(),
token:process.argv[2]};
process.stdout.write(JSON.stringify(value)+"\n");"#,
            &[&milliseconds.to_string(), token],
        )
        .map(String::into_bytes)
    }

    /// Confirm the complete canonical lease grammar before admitting its timestamp.
    fn validated_lease_expiry(&self, bytes: &[u8]) -> Option<i64> {
        let text = std::str::from_utf8(bytes).ok()?;
        eval(
            r#"const source=process.argv[1], value=JSON.parse(source);
const ordered=Object.fromEntries(Object.keys(value).sort().map(key=>[key,value[key]]));
if(JSON.stringify(ordered)+"\n"!==source ||
value.contractVersion!=="adjustment-revision-spool-lock/v1" ||
!(/^[a-f0-9-]{36}$/).test(value.token) || typeof value.expiresAt!=="string")process.exit(1);
process.stdout.write(String(Date.parse(value.expiresAt))); "#,
            &[text],
        )?
        .parse()
        .ok()
    }
}

/// Run one closed test-only Node codec function without touching a live service.
fn eval(script: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("node")
        .args(["--input-type=module", "--eval", script, "--"])
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(
            // return the exact machine-readable fixture output
            || String::from_utf8(output.stdout).ok(),
        )
        .flatten()
}
