use super::*;
use crate::{
    NativeMaintenanceCasMember, NativeMaintenanceCasMemberValidator,
    NativeMaintenanceCasStreamSession, build_native_maintenance_graph_manifest,
    encode_native_maintenance_cas_pack, inspect_native_maintenance_cas_pack,
};
use std::io::Cursor;

struct Objects(HashMap<[u8; 32], Vec<u8>>);
impl NativeMaintenanceFullGraphObjects for Objects {
    type Reader = Cursor<Vec<u8>>;
    /// Read one actual retained content-addressed fixture object.
    fn open_object(&mut self, digest: [u8; 32]) -> Option<(Self::Reader, u64)> {
        let bytes = self.0.get(&digest)?.clone();
        let size = bytes.len() as u64;
        Some((Cursor::new(bytes), size))
    }
}

struct Validator;
impl NativeMaintenanceCasMemberValidator for Validator {
    /// Admit the exact single fixture's original domain kind and payload.
    fn validate(&mut self, kind: &str, payload: &[u8]) -> bool {
        kind == "weather/fixture" && payload == b"rain"
    }
}
impl NativeMaintenanceCasStreamValidator for Validator {
    type Session = Session;
    /// Open only the fixture's bounded registered domain.
    fn begin(&mut self, kind: &str, length: u64) -> Option<Self::Session> {
        (kind == "weather/fixture" && length == 4).then_some(Session(Vec::new()))
    }
}
struct Session(Vec<u8>);
impl NativeMaintenanceCasStreamSession for Session {
    /// Collect the single four-byte fixture payload.
    fn write(&mut self, bytes: &[u8]) -> bool {
        self.0.extend_from_slice(bytes);
        self.0.len() <= 4
    }
    /// Require the real source domain fixture content.
    fn finish(self) -> bool {
        self.0 == b"rain"
    }
}

/// Verify a real two-graph chain with actual packed member inventory and predecessor counts.
#[test]
fn native_full_graph_verifies_real_content_and_predecessor_chain() {
    let mut store = Objects(HashMap::new());
    let payload = encode_native_maintenance_cas_pack(
        vec![NativeMaintenanceCasMember {
            kind: "weather/fixture",
            identity_sha256: [1; 32],
            payload: b"rain",
        }],
        &mut Validator,
    )
    .unwrap();
    let inventory = inspect_native_maintenance_cas_pack(&payload, &mut Validator).unwrap();
    let member = &inventory.members[0];
    let entry = NativeMaintenanceGraphEntry {
        kind: member.kind.clone(),
        identity_sha256: format_digest(member.identity_sha256),
        object_sha256: format_digest(inventory.object_sha256),
        member_offset: member.member_offset,
        member_length: member.member_length,
        member_sha256: format_digest(member.member_sha256),
    };
    store.0.insert(inventory.object_sha256, payload);
    let mut predecessor = None;
    let mut head = [0; 32];
    // build genesis then successor with the original single-member graph object format
    for _ in 0..2 {
        let graph =
            build_native_maintenance_graph_manifest(predecessor, vec![entry.clone()], vec![])
                .unwrap();
        let bytes = graph.canonical_bytes().unwrap();
        let pack = encode_native_maintenance_cas_pack(
            vec![NativeMaintenanceCasMember {
                kind: GRAPH_KIND,
                identity_sha256: weather_edge::RevisionArchiveCanonicalCodec::sha256(
                    &crate::NativeWirePrimitives,
                    &bytes,
                ),
                payload: &bytes,
            }],
            &mut Validator,
        )
        .unwrap();
        head = weather_edge::RevisionArchiveCanonicalCodec::sha256(
            &crate::NativeWirePrimitives,
            &pack,
        );
        predecessor = Some(format_digest(head));
        store.0.insert(head, pack);
    }
    let verified =
        verify_native_maintenance_full_graph(&mut store, head, &mut Validator, true).unwrap();
    assert_eq!(verified.predecessor_count, 1);
    assert_eq!(verified.member_count, 1);
    assert_eq!(verified.object_count, 1);
    let current_only =
        verify_native_maintenance_full_graph(&mut store, head, &mut Validator, false).unwrap();
    assert_eq!(current_only.predecessor_count, 0);
}
