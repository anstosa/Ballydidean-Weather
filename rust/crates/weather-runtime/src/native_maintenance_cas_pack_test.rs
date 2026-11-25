use super::*;
use std::process::Command;

struct FixtureValidator;
impl NativeMaintenanceCasMemberValidator for FixtureValidator {
    /// Admit only the two explicit fixture payload schemas.
    fn validate(&mut self, kind: &str, payload: &[u8]) -> bool {
        matches!(
            (kind, payload),
            ("fixture/row", b"weather") | ("fixture/empty", b"")
        )
    }
}
struct FixtureStream {
    kind: String,
    bytes: Vec<u8>,
}
impl NativeMaintenanceCasStreamValidator for FixtureValidator {
    type Session = FixtureStream;
    /// Open only the bounded source fixture schemas.
    fn begin(&mut self, kind: &str, length: u64) -> Option<Self::Session> {
        (length <= 7 && matches!(kind, "fixture/row" | "fixture/empty")).then(|| FixtureStream {
            kind: kind.into(),
            bytes: Vec::new(),
        })
    }
}
impl NativeMaintenanceCasStreamSession for FixtureStream {
    /// Retain only one small fixture member.
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.bytes.extend_from_slice(bytes);
        self.bytes.len() <= 7
    }
    /// Validate the real fixture member once complete.
    fn finish(self) -> bool {
        FixtureValidator.validate(&self.kind, &self.bytes)
    }
}

/// Compare exact binary source packs and inspect the same retained object by both native readers.
#[test]
fn native_cas_pack_matches_source_and_streaming_inventory() {
    let native = encode_native_maintenance_cas_pack(
        vec![
            NativeMaintenanceCasMember {
                kind: "fixture/row",
                identity_sha256: [1; 32],
                payload: b"weather",
            },
            NativeMaintenanceCasMember {
                kind: "fixture/empty",
                identity_sha256: [2; 32],
                payload: b"",
            },
        ],
        &mut FixtureValidator,
    )
    .unwrap();
    let result=Command::new("node").args(["--input-type=module","-e",r#"
import {pathToFileURL} from 'node:url';
const {encodeCasPack,inspectCasPack}=await import(pathToFileURL(process.argv[2]));
const validators=new Map([["fixture/row",bytes=>{if(bytes.toString()!=="weather")throw new Error("row");}],["fixture/empty",bytes=>{if(bytes.length!==0)throw new Error("empty");}]]);
const pack=encodeCasPack([{kind:"fixture/row",identitySha256:"01".repeat(32),payload:Buffer.from("weather")},{kind:"fixture/empty",identitySha256:"02".repeat(32),payload:Buffer.alloc(0)}],{validators});
inspectCasPack(pack,{validators});
process.stdout.write(pack);
"#,"cas-oracle"]).arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../scripts/research/adjustment_plaintext_archive.mjs").canonicalize().unwrap()).output().unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(native, result.stdout);
    let inventory = inspect_native_maintenance_cas_pack(&native, &mut FixtureValidator).unwrap();
    let streamed = inspect_native_maintenance_cas_stream(
        native.as_slice(),
        native.len() as u64,
        &mut FixtureValidator,
    )
    .unwrap();
    assert_eq!(inventory, streamed);
    assert_eq!(inventory.members[1].member_length, 7);
    assert_eq!(inventory.members[1].member_offset + 7, native.len() as u64);
}
