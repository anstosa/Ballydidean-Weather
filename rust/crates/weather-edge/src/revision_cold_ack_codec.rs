//! Canonical JSON binding for durable cold-revision custody acknowledgements.

use weather_contracts::JsonValue;

use crate::rain_control_state_archive::RainControlStateWireCodec;
use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, instant, lower_hex};
use crate::revision_cold_ack::{
    REVISION_COLD_ACK_MAXIMUM_BYTES, RevisionColdAck, RevisionColdAckIdentity,
    RevisionColdAckRequest, RevisionColdRetirementEntry, RevisionColdRetirementKind,
};
use crate::revision_cold_page::{RevisionColdCursor, RevisionColdPageRequest};
use crate::revision_gap_transfer::RevisionGapSha256;

const ACK_V1: &str = "adjustment-revision-custody-acknowledgement/v1";
const ACK_V2: &str = "adjustment-revision-custody-acknowledgement/v2";
const AUTHORITY: &str = "cold_custody_only";
const MINIMUM_RETIREMENTS: usize = 4;
const MAXIMUM_RETIREMENTS: usize = 10;

/// Reuse one admitted JSON implementation rather than introducing a second parser or encoder.
pub trait RevisionColdAckJsonCodec: RevisionArchiveCanonicalCodec {
    /// Decode one complete JSON object without accepting duplicate or normalized keys.
    fn decode_revision_cold_ack_json(&self, bytes: &[u8]) -> Option<JsonValue>;

    /// Encode recursively sorted canonical JSON with exactly one terminating line feed.
    fn encode_revision_cold_ack_json(&self, value: &JsonValue) -> Option<Vec<u8>>;
}

impl<Codec: RainControlStateWireCodec> RevisionColdAckJsonCodec for Codec {
    /// Forward decoding to the already admitted archive JSON owner.
    fn decode_revision_cold_ack_json(&self, bytes: &[u8]) -> Option<JsonValue> {
        self.decode_control_json(bytes)
    }

    /// Forward canonical encoding to the already admitted archive JSON owner.
    fn encode_revision_cold_ack_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
        self.encode_control_sorted_json(value)
    }
}

/// Decode and fully validate one canonical V1 or V2 custody acknowledgement.
pub fn decode_revision_cold_ack(
    bytes: &[u8],
    codec: &impl RevisionColdAckJsonCodec,
) -> Option<RevisionColdAck> {
    // retain the original bounded canonical document boundary
    if !(2..=REVISION_COLD_ACK_MAXIMUM_BYTES).contains(&bytes.len()) {
        return None;
    }
    let value = codec.decode_revision_cold_ack_json(bytes)?;
    let version = text(&value, "contractVersion")?;
    let (identity_key, packed) = match version.as_str() {
        ACK_V1 => ("graphManifestSha256", false),
        ACK_V2 => ("custodyCheckpointSha256", true),
        _ => return None,
    };
    let keys = [
        "acknowledgedAt",
        "acknowledgementSha256",
        "afterArchiveCommitOrdinal",
        "afterFrontierSha256",
        "authority",
        "contractVersion",
        identity_key,
        "memberRootSha256",
        "nextArchiveCommitOrdinal",
        "nextFrontierSha256",
        "pageSha256",
        "previousAcknowledgementSha256",
        "previousPageSha256",
        "retirementEntries",
        "startMemberSha256",
        "startSha256",
        "watermarkArchiveCommitOrdinal",
        "watermarkFrontierSha256",
    ];
    // admit no extension or omitted member before interpreting authority
    if !exact_keys(&value, &keys) || text(&value, "authority").as_deref() != Some(AUTHORITY) {
        return None;
    }
    let acknowledged_at = text(&value, "acknowledgedAt")?;
    instant(codec, &acknowledged_at)?;
    let after = RevisionColdCursor {
        ordinal: uint64(&value, "afterArchiveCommitOrdinal")?,
        frontier: digest(&value, "afterFrontierSha256")?,
    };
    let next = RevisionColdCursor {
        ordinal: uint64(&value, "nextArchiveCommitOrdinal")?,
        frontier: digest(&value, "nextFrontierSha256")?,
    };
    // preserve the source's strict ordinal advance independently of the watermark
    if next.ordinal <= after.ordinal {
        return None;
    }
    let request = RevisionColdAckRequest {
        page: RevisionColdPageRequest {
            watermark: RevisionColdCursor {
                ordinal: uint64(&value, "watermarkArchiveCommitOrdinal")?,
                frontier: digest(&value, "watermarkFrontierSha256")?,
            },
            start: digest(&value, "startSha256")?,
            after,
            previous: digest(&value, "previousPageSha256")?,
        },
        page_sha256: digest(&value, "pageSha256")?,
        // select the version-specific identity only after exact keys are admitted
        identity: if packed {
            RevisionColdAckIdentity::CustodyCheckpoint(digest(&value, identity_key)?)
        } else {
            RevisionColdAckIdentity::GraphManifest(digest(&value, identity_key)?)
        },
        member_root_sha256: digest(&value, "memberRootSha256")?,
        start_member_sha256: nullable_digest(&value, "startMemberSha256")?,
    };
    let retirement_entries = retirement_entries(&value)?;
    let acknowledgement = RevisionColdAck {
        acknowledged_at,
        request,
        next,
        previous_acknowledgement_sha256: nullable_digest(&value, "previousAcknowledgementSha256")?,
        acknowledgement_sha256: digest(&value, "acknowledgementSha256")?,
        retirement_entries,
    };
    // bind the self-address and prohibit an alternate canonical byte representation
    let canonical = encode_revision_cold_ack(&acknowledgement, codec, true)?;
    (canonical == bytes).then_some(acknowledgement)
}

/// Encode either the unsigned identity preimage or the complete self-addressed acknowledgement.
pub fn encode_revision_cold_ack(
    acknowledgement: &RevisionColdAck,
    codec: &impl RevisionColdAckJsonCodec,
    include_digest: bool,
) -> Option<Vec<u8>> {
    // reject invalid typed values before materializing any durable bytes
    if instant(codec, &acknowledgement.acknowledged_at).is_none()
        || acknowledgement.next.ordinal <= acknowledgement.request.page.after.ordinal
        || !valid_retirements(&acknowledgement.retirement_entries)
    {
        return None;
    }
    let (contract, identity_key, identity) = match &acknowledgement.request.identity {
        RevisionColdAckIdentity::GraphManifest(identity) => {
            (ACK_V1, "graphManifestSha256", *identity)
        }
        RevisionColdAckIdentity::CustodyCheckpoint(identity) => {
            (ACK_V2, "custodyCheckpointSha256", *identity)
        }
    };
    let request = &acknowledgement.request;
    let mut fields = vec![
        entry("acknowledgedAt", string(&acknowledgement.acknowledged_at)),
        entry(
            "afterArchiveCommitOrdinal",
            string(&request.page.after.ordinal.to_string()),
        ),
        entry("afterFrontierSha256", hash(request.page.after.frontier)),
        entry("authority", string(AUTHORITY)),
        entry("contractVersion", string(contract)),
        entry(identity_key, hash(identity)),
        entry("memberRootSha256", hash(request.member_root_sha256)),
        entry(
            "nextArchiveCommitOrdinal",
            string(&acknowledgement.next.ordinal.to_string()),
        ),
        entry("nextFrontierSha256", hash(acknowledgement.next.frontier)),
        entry("pageSha256", hash(request.page_sha256)),
        entry(
            "previousAcknowledgementSha256",
            optional_hash(acknowledgement.previous_acknowledgement_sha256),
        ),
        entry("previousPageSha256", hash(request.page.previous)),
        entry(
            "retirementEntries",
            JsonValue::Array(
                acknowledgement
                    .retirement_entries
                    .iter()
                    // retain exact source order after validating it independently
                    .map(retirement_value)
                    .collect(),
            ),
        ),
        entry(
            "startMemberSha256",
            optional_hash(request.start_member_sha256),
        ),
        entry("startSha256", hash(request.page.start)),
        entry(
            "watermarkArchiveCommitOrdinal",
            string(&request.page.watermark.ordinal.to_string()),
        ),
        entry(
            "watermarkFrontierSha256",
            hash(request.page.watermark.frontier),
        ),
    ];
    // add only the self-address after the complete unsigned preimage is fixed
    if include_digest {
        let unsigned = encode_revision_cold_ack(acknowledgement, codec, false)?;
        // refuse a caller-supplied identity that does not address the exact preimage
        if RevisionGapSha256::admitted(codec.sha256(&unsigned))
            != acknowledgement.acknowledgement_sha256
        {
            return None;
        }
        fields.push(entry(
            "acknowledgementSha256",
            hash(acknowledgement.acknowledgement_sha256),
        ));
    }
    let bytes = codec.encode_revision_cold_ack_json(&JsonValue::Object(fields))?;
    // apply the predecessor's ceiling only to a complete durable acknowledgement
    if include_digest && bytes.len() > REVISION_COLD_ACK_MAXIMUM_BYTES {
        return None;
    }
    Some(bytes)
}

/// Decode and validate the fixed sorted retirement population.
fn retirement_entries(value: &JsonValue) -> Option<Vec<RevisionColdRetirementEntry>> {
    let JsonValue::Array(values) = field(value, "retirementEntries")? else {
        return None;
    };
    let mut entries = Vec::with_capacity(values.len());
    // preserve every exact file identity in source order
    for value in values {
        // reject an extended or incomplete retirement address
        if !exact_keys(value, &["fileSha256", "identitySha256", "kind"]) {
            return None;
        }
        entries.push(RevisionColdRetirementEntry {
            kind: RevisionColdRetirementKind::from_wire_value(&text(value, "kind")?)?,
            identity_sha256: digest(value, "identitySha256")?,
            file_sha256: digest(value, "fileSha256")?,
        });
    }
    valid_retirements(&entries).then_some(entries)
}

/// Require the original bounded stable kind-and-identity ordering.
fn valid_retirements(entries: &[RevisionColdRetirementEntry]) -> bool {
    (MINIMUM_RETIREMENTS..=MAXIMUM_RETIREMENTS).contains(&entries.len())
        && entries.windows(2).all(
            // permit stable equal addresses while refusing descending order
            |pair| {
                (pair[0].kind, pair[0].identity_sha256) <= (pair[1].kind, pair[1].identity_sha256)
            },
        )
}

/// Project one typed retirement entry into its exact closed wire object.
fn retirement_value(value: &RevisionColdRetirementEntry) -> JsonValue {
    JsonValue::Object(vec![
        entry("fileSha256", hash(value.file_sha256)),
        entry("identitySha256", hash(value.identity_sha256)),
        entry("kind", string(value.kind.wire_value())),
    ])
}

/// Return one exact object field without normalizing UTF-16 key units.
fn field<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    let JsonValue::Object(fields) = value else {
        return None;
    };
    fields
        .iter()
        .find_map(|(name, value)| name.iter().copied().eq(key.encode_utf16()).then_some(value))
}

/// Require the exact closed object key set.
fn exact_keys(value: &JsonValue, expected: &[&str]) -> bool {
    let JsonValue::Object(fields) = value else {
        return false;
    };
    fields.len() == expected.len() && expected.iter().all(|key| field(value, key).is_some())
}

/// Decode one exact UTF-16 JSON string.
fn text(value: &JsonValue, key: &str) -> Option<String> {
    let JsonValue::String(units) = field(value, key)? else {
        return None;
    };
    String::from_utf16(units).ok()
}

/// Decode only the canonical unsigned 64-bit decimal spelling.
fn uint64(value: &JsonValue, key: &str) -> Option<u64> {
    let text = text(value, key)?;
    let ordinal = text.parse::<u64>().ok()?;
    // reject leading zeroes and every alternate integer spelling
    (ordinal.to_string() == text).then_some(ordinal)
}

/// Decode one required lowercase hexadecimal SHA-256 identity.
fn digest(value: &JsonValue, key: &str) -> Option<RevisionGapSha256> {
    RevisionGapSha256::from_lower_hex(&text(value, key)?)
}

/// Decode one nullable SHA-256 identity without accepting a missing field.
fn nullable_digest(value: &JsonValue, key: &str) -> Option<Option<RevisionGapSha256>> {
    match field(value, key)? {
        JsonValue::Null => Some(None),
        JsonValue::String(units) => Some(Some(RevisionGapSha256::from_lower_hex(
            &String::from_utf16(units).ok()?,
        )?)),
        _ => None,
    }
}

/// Encode one fixed object key.
fn entry(key: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (key.encode_utf16().collect(), value)
}

/// Encode one exact Rust string as unnormalized UTF-16 units.
fn string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Encode one fixed lowercase hexadecimal SHA-256 identity.
fn hash(value: RevisionGapSha256) -> JsonValue {
    string(&lower_hex(*value.as_bytes()))
}

/// Encode one nullable fixed digest boundary.
fn optional_hash(value: Option<RevisionGapSha256>) -> JsonValue {
    match value {
        Some(value) => hash(value),
        None => JsonValue::Null,
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};

    use crate::RainMaintenanceControlStateGapReceipt;
    use weather_contracts::RainMaintenanceControlStateStageReceipt;

    use super::*;

    struct NodeMaterial {
        acknowledgement_sha256: RevisionGapSha256,
        unsigned: Vec<u8>,
        signed: Vec<u8>,
    }

    /// Canonical fixture adapter whose expected bytes come from the original Node algorithm.
    struct NodeCodec {
        unsigned_value: JsonValue,
        signed_value: JsonValue,
        unsigned: Vec<u8>,
        signed: Vec<u8>,
    }

    impl RevisionArchiveCanonicalCodec for NodeCodec {
        /// Admit only the fixture's exact canonical timestamp.
        fn canonical_utc_milliseconds(&self, text: &str) -> Option<i64> {
            (text == "2026-10-08T01:11:12.000Z").then_some(1_760_231_472_000)
        }

        /// Use the host's existing digest primitive only inside this test fixture.
        fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
            let mut child = Command::new("sha256sum")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("fixture hash");
            std::io::Write::write_all(child.stdin.as_mut().expect("hash stdin"), bytes)
                .expect("write hash bytes");
            let output = child.wait_with_output().expect("read hash");
            assert!(output.status.success());
            let text = std::str::from_utf8(&output.stdout[..64]).expect("hex hash");
            *RevisionGapSha256::from_lower_hex(text)
                .expect("fixture digest")
                .as_bytes()
        }
    }

    impl RainControlStateWireCodec for NodeCodec {
        /// Decode only the exact signed Node fixture.
        fn decode_control_json(&self, bytes: &[u8]) -> Option<JsonValue> {
            (bytes == self.signed.as_slice()).then(|| self.signed_value.clone())
        }

        /// This codec is not used for producer-order control state encoding.
        fn encode_control_ordered_json(&self, _: &JsonValue) -> Option<Vec<u8>> {
            None
        }

        /// Return only bytes independently emitted for the matching Node value.
        fn encode_control_sorted_json(&self, value: &JsonValue) -> Option<Vec<u8>> {
            // return the original unsigned identity preimage only for its exact value
            if value == &self.unsigned_value {
                return Some(self.unsigned.clone());
            }
            // return the complete original acknowledgement only for its exact value
            if value == &self.signed_value {
                return Some(self.signed.clone());
            }
            None
        }

        /// Stage receipt decoding is outside this acknowledgement fixture.
        fn decode_stage_receipt(
            &self,
            _: &[u8],
        ) -> Option<RainMaintenanceControlStateStageReceipt> {
            None
        }

        /// Gap receipt decoding is outside this acknowledgement fixture.
        fn decode_gap_receipt(&self, _: &[u8]) -> Option<RainMaintenanceControlStateGapReceipt> {
            None
        }
    }

    #[test]
    fn matches_original_node_v1_and_v2_bytes_and_decoding() {
        // exercise both version-specific identity fields against independently executed Node
        for identity in [
            RevisionColdAckIdentity::GraphManifest(digest_marker(7)),
            RevisionColdAckIdentity::CustodyCheckpoint(digest_marker(8)),
        ] {
            let material = node_material(identity);
            let acknowledgement =
                fixture_acknowledgement(identity, material.acknowledgement_sha256);
            let unsigned_value = expected_value(&acknowledgement, false);
            let signed_value = expected_value(&acknowledgement, true);
            let codec = NodeCodec {
                unsigned_value,
                signed_value,
                unsigned: material.unsigned.clone(),
                signed: material.signed.clone(),
            };

            assert_eq!(
                encode_revision_cold_ack(&acknowledgement, &codec, false),
                Some(material.unsigned)
            );
            assert_eq!(
                encode_revision_cold_ack(&acknowledgement, &codec, true),
                Some(material.signed.clone())
            );
            assert_eq!(
                decode_revision_cold_ack(&material.signed, &codec),
                Some(acknowledgement)
            );
        }
    }

    #[test]
    fn refuses_wrong_self_address_and_descending_retirement_order() {
        let identity = RevisionColdAckIdentity::GraphManifest(digest_marker(7));
        let material = node_material(identity);
        let mut acknowledgement =
            fixture_acknowledgement(identity, material.acknowledgement_sha256);
        let codec = NodeCodec {
            unsigned_value: expected_value(&acknowledgement, false),
            signed_value: expected_value(&acknowledgement, true),
            unsigned: material.unsigned,
            signed: material.signed,
        };

        acknowledgement.acknowledgement_sha256 = digest_marker(99);
        assert_eq!(
            encode_revision_cold_ack(&acknowledgement, &codec, true),
            None
        );
        // swap different kind entries to violate the fixed kind-and-identity order
        acknowledgement = fixture_acknowledgement(
            RevisionColdAckIdentity::GraphManifest(digest_marker(7)),
            material.acknowledgement_sha256,
        );
        acknowledgement.retirement_entries.swap(0, 3);
        assert_eq!(
            encode_revision_cold_ack(&acknowledgement, &codec, false),
            None
        );
    }

    /// Build one typed fixture shared with the original Node process.
    fn fixture_acknowledgement(
        identity: RevisionColdAckIdentity,
        acknowledgement_sha256: RevisionGapSha256,
    ) -> RevisionColdAck {
        RevisionColdAck {
            acknowledged_at: "2026-10-08T01:11:12.000Z".to_owned(),
            request: RevisionColdAckRequest {
                page: RevisionColdPageRequest {
                    watermark: RevisionColdCursor {
                        ordinal: 9,
                        frontier: digest_marker(9),
                    },
                    start: digest_marker(4),
                    after: RevisionColdCursor {
                        ordinal: 2,
                        frontier: digest_marker(2),
                    },
                    previous: digest_marker(3),
                },
                page_sha256: digest_marker(5),
                identity,
                member_root_sha256: digest_marker(6),
                start_member_sha256: Some(digest_marker(10)),
            },
            next: RevisionColdCursor {
                ordinal: 7,
                frontier: digest_marker(11),
            },
            previous_acknowledgement_sha256: Some(digest_marker(12)),
            acknowledgement_sha256,
            retirement_entries: vec![
                retirement(RevisionColdRetirementKind::RainControlState, 1, 21),
                retirement(RevisionColdRetirementKind::RevisionCommitReceipt, 2, 22),
                retirement(RevisionColdRetirementKind::RevisionProjection, 3, 23),
                retirement(RevisionColdRetirementKind::ShadowRevisionCapsule, 4, 24),
            ],
        }
    }

    /// Create one exact retirement address.
    fn retirement(
        kind: RevisionColdRetirementKind,
        identity: u8,
        file: u8,
    ) -> RevisionColdRetirementEntry {
        RevisionColdRetirementEntry {
            kind,
            identity_sha256: digest_marker(identity),
            file_sha256: digest_marker(file),
        }
    }

    /// Project the independently expected JSON value for the fixture adapter.
    fn expected_value(acknowledgement: &RevisionColdAck, include_digest: bool) -> JsonValue {
        let request = &acknowledgement.request;
        let (version, identity_key, identity) = match request.identity {
            RevisionColdAckIdentity::GraphManifest(value) => (ACK_V1, "graphManifestSha256", value),
            RevisionColdAckIdentity::CustodyCheckpoint(value) => {
                (ACK_V2, "custodyCheckpointSha256", value)
            }
        };
        let mut fields = vec![
            entry("acknowledgedAt", string(&acknowledgement.acknowledged_at)),
            entry(
                "afterArchiveCommitOrdinal",
                string(&request.page.after.ordinal.to_string()),
            ),
            entry("afterFrontierSha256", hash(request.page.after.frontier)),
            entry("authority", string(AUTHORITY)),
            entry("contractVersion", string(version)),
            entry(identity_key, hash(identity)),
            entry("memberRootSha256", hash(request.member_root_sha256)),
            entry(
                "nextArchiveCommitOrdinal",
                string(&acknowledgement.next.ordinal.to_string()),
            ),
            entry("nextFrontierSha256", hash(acknowledgement.next.frontier)),
            entry("pageSha256", hash(request.page_sha256)),
            entry(
                "previousAcknowledgementSha256",
                optional_hash(acknowledgement.previous_acknowledgement_sha256),
            ),
            entry("previousPageSha256", hash(request.page.previous)),
            entry(
                "retirementEntries",
                JsonValue::Array(
                    acknowledgement
                        .retirement_entries
                        .iter()
                        .map(retirement_value)
                        .collect(),
                ),
            ),
            entry(
                "startMemberSha256",
                optional_hash(request.start_member_sha256),
            ),
            entry("startSha256", hash(request.page.start)),
            entry(
                "watermarkArchiveCommitOrdinal",
                string(&request.page.watermark.ordinal.to_string()),
            ),
            entry(
                "watermarkFrontierSha256",
                hash(request.page.watermark.frontier),
            ),
        ];
        // include the identity only in the durable signed representation
        if include_digest {
            fields.push(entry(
                "acknowledgementSha256",
                hash(acknowledgement.acknowledgement_sha256),
            ));
        }
        JsonValue::Object(fields)
    }

    /// Ask the original Node canonicalization and hash functions for exact fixture bytes.
    fn node_material(identity: RevisionColdAckIdentity) -> NodeMaterial {
        let (version, identity_key, marker) = match identity {
            RevisionColdAckIdentity::GraphManifest(_) => (ACK_V1, "graphManifestSha256", 7),
            RevisionColdAckIdentity::CustodyCheckpoint(_) => (ACK_V2, "custodyCheckpointSha256", 8),
        };
        let marker_text = marker.to_string();
        let module = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../deploy/scripts/adjustment-evidence-store.mjs")
            .canonicalize()
            .expect("canonical predecessor module");
        let module_url = format!("file://{}", module.display());
        let output = Command::new("node")
            .args([
                "--input-type=module",
                "--eval",
                NODE_ORACLE,
                "--",
                &module_url,
                version,
                identity_key,
                &marker_text,
            ])
            .output()
            .expect("Node acknowledgement oracle");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let first = output
            .stdout
            .iter()
            .position(|byte| *byte == b'\n')
            .expect("digest line");
        let second_relative = output.stdout[first + 1..]
            .iter()
            .position(|byte| *byte == b'\n')
            .expect("length line");
        let second = first + 1 + second_relative;
        let digest = std::str::from_utf8(&output.stdout[..first]).expect("digest UTF-8");
        let unsigned_length = std::str::from_utf8(&output.stdout[first + 1..second])
            .expect("length UTF-8")
            .parse::<usize>()
            .expect("unsigned length");
        let unsigned_start = second + 1;
        let signed_start = unsigned_start + unsigned_length;
        NodeMaterial {
            acknowledgement_sha256: RevisionGapSha256::from_lower_hex(digest)
                .expect("acknowledgement digest"),
            unsigned: output.stdout[unsigned_start..signed_start].to_vec(),
            signed: output.stdout[signed_start..].to_vec(),
        }
    }

    /// Return one visible fixed digest marker.
    fn digest_marker(marker: u8) -> RevisionGapSha256 {
        RevisionGapSha256::admitted([marker; 32])
    }

    const NODE_ORACLE: &str = r#"
import {createHash} from 'node:crypto';
const [moduleUrl,version,identityKey,markerText]=process.argv.slice(1);
const hex=n=>Number(n).toString(16).padStart(2,'0').repeat(32);
const canonical=value=>Array.isArray(value)?value.map(canonical):
  value!==null&&typeof value==='object'?
    Object.fromEntries(Object.keys(value).sort().map(key=>[key,canonical(value[key])])):value;
const bytes=value=>Buffer.from(JSON.stringify(canonical(value))+'\n');
const unsigned={
  acknowledgedAt:'2026-10-08T01:11:12.000Z',afterArchiveCommitOrdinal:'2',
  afterFrontierSha256:hex(2),authority:'cold_custody_only',contractVersion:version,
  [identityKey]:hex(Number(markerText)),memberRootSha256:hex(6),
  nextArchiveCommitOrdinal:'7',nextFrontierSha256:hex(11),pageSha256:hex(5),
  previousAcknowledgementSha256:hex(12),previousPageSha256:hex(3),
  retirementEntries:[
    {fileSha256:hex(21),identitySha256:hex(1),kind:'rain_control_state'},
    {fileSha256:hex(22),identitySha256:hex(2),kind:'revision_commit_receipt'},
    {fileSha256:hex(23),identitySha256:hex(3),kind:'revision_projection'},
    {fileSha256:hex(24),identitySha256:hex(4),kind:'shadow_revision_capsule'},
  ],
  startMemberSha256:hex(10),startSha256:hex(4),
  watermarkArchiveCommitOrdinal:'9',watermarkFrontierSha256:hex(9),
};
const unsignedBytes=bytes(unsigned);
const digest=createHash('sha256').update(unsignedBytes).digest('hex');
const signedBytes=bytes({...unsigned,acknowledgementSha256:digest});
const predecessor=await import(moduleUrl);
const validate=version.endsWith('/v2')?
  predecessor.validateAdjustmentRevisionColdCustodyAcknowledgementV2:
  predecessor.validateAdjustmentRevisionColdCustodyAcknowledgement;
validate(JSON.parse(signedBytes.toString('utf8')));
process.stdout.write(digest+'\n'+unsignedBytes.length+'\n');
process.stdout.write(unsignedBytes);
process.stdout.write(signedBytes);
"#;
}
