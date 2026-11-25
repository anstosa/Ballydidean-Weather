//! Verify the retained bounded revision catalog page at the workstation boundary.

use crate::maintenance_canonical_json::{exact_keys, field, text, valid_hash};
use crate::native_maintenance_revision_cold_drain::{
    NativeRevisionColdPageRequest, NativeRevisionColdPublicationDisposition,
    VerifiedNativeRevisionColdEntry, VerifiedNativeRevisionColdPage,
};
use crate::{NativeWirePrimitives, native_maintenance_retirement_files_v2::format_digest};
use weather_contracts::{AdjustmentRevisionCommitReceipt, JsonValue};
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, PermanentArchiveGapCodec, PrivateRevisionReceiptCodec,
    PrivateShadowReceiptCodec, RainControlStateWireCodec, RevisionArchiveCanonicalCodec,
    shadow_stage_receipt_value,
};

const MAXIMUM: usize = 4_832 * 1024;
const PAGE_KEYS: &[&str] = &[
    "afterArchiveCommitOrdinal",
    "afterFrontierSha256",
    "contractVersion",
    "entries",
    "eof",
    "nextArchiveCommitOrdinal",
    "nextFrontierSha256",
    "pageSha256",
    "previousPageSha256",
    "startSha256",
    "watermarkArchiveCommitOrdinal",
    "watermarkFrontierSha256",
];

/// Validate the exact one-line remote response and every direct successor.
pub fn verify_native_maintenance_cold_remote_page(
    bytes: &[u8],
    request: &NativeRevisionColdPageRequest,
) -> Option<VerifiedNativeRevisionColdPage> {
    let wire = NativeWirePrimitives;
    let value = wire.decode_json(bytes)?;
    let canonical_bytes = wire.encode_sorted_json(&value)?;
    // retain the original transport framing and canonical payload budget
    if bytes != wire.encode_ordered_json(&value)?.as_slice()
        || canonical_bytes.len() > MAXIMUM
        || !exact_keys(&value, PAGE_KEYS)
        || text(&value, "contractVersion")? != "adjustment-revision-cold-page/v1"
        || text(&value, "afterArchiveCommitOrdinal")? != request.after_archive_commit_ordinal
        || text(&value, "afterFrontierSha256")? != request.after_frontier_sha256
        || text(&value, "watermarkArchiveCommitOrdinal")?
            != request.watermark_archive_commit_ordinal
        || text(&value, "watermarkFrontierSha256")? != request.watermark_frontier_sha256
        || text(&value, "startSha256")? != request.start_sha256
        || text(&value, "previousPageSha256")? != request.previous_page_sha256
    {
        return None;
    }
    // require all six immutable cursor fields
    let mut ordinal = ordinal_field(&value, "afterArchiveCommitOrdinal")?;
    let watermark = ordinal_field(&value, "watermarkArchiveCommitOrdinal")?;
    let mut frontier = hash_field(&value, "afterFrontierSha256")?;
    hash_field(&value, "watermarkFrontierSha256")?;
    hash_field(&value, "startSha256")?;
    hash_field(&value, "previousPageSha256")?;
    let page_sha256 = hash_field(&value, "pageSha256")?;
    let JsonValue::Object(mut unsigned) = value.clone() else {
        return None;
    };
    unsigned.retain(|(name, _)| !name.iter().copied().eq("pageSha256".encode_utf16()));
    if format_digest(wire.sha256(&wire.encode_sorted_json(&JsonValue::Object(unsigned))?))
        != page_sha256
    {
        return None;
    }
    let entries = array(&value, "entries")?;
    if entries.len() > 2 {
        return None;
    }
    let JsonValue::Boolean(eof) = field(&value, "eof")? else {
        return None;
    };
    let codec = ArchiveWireCodec::new(wire);
    let mut verified = Vec::with_capacity(entries.len());
    // validate all receipt metadata and preserve the full source entry
    for entry in entries {
        let (receipts, disposition) = verify_entry(entry, &codec)?;
        for receipt in receipts {
            let next = receipt.archiveCommitOrdinal.parse::<u64>().ok()?;
            if receipt.predecessorFrontierSha256 != frontier || next <= ordinal || next > watermark
            {
                return None;
            }
            ordinal = next;
            frontier = receipt.frontierSha256;
        }
        verified.push(VerifiedNativeRevisionColdEntry {
            publication_disposition: disposition,
            value: entry.clone(),
        });
    }
    let next_archive_commit_ordinal = text(&value, "nextArchiveCommitOrdinal")?;
    let next_frontier_sha256 = hash_field(&value, "nextFrontierSha256")?;
    let at_watermark = ordinal == watermark && frontier == request.watermark_frontier_sha256;
    // forbid an unproductive nonterminal page and require the exact resulting cursor
    if ordinal.to_string() != next_archive_commit_ordinal
        || frontier != next_frontier_sha256
        || *eof != at_watermark
        || (!at_watermark && ordinal >= watermark)
        || (verified.is_empty() && !*eof)
    {
        return None;
    }
    Some(VerifiedNativeRevisionColdPage {
        request: request.clone(),
        canonical_bytes,
        entries: verified,
        eof: *eof,
        next_archive_commit_ordinal,
        next_frontier_sha256,
        page_sha256,
        value,
    })
}

// admit all item-level metadata with the shared canonical archive codecs
fn verify_entry(
    entry: &JsonValue,
    codec: &ArchiveWireCodec<NativeWirePrimitives>,
) -> Option<(
    Vec<AdjustmentRevisionCommitReceipt>,
    NativeRevisionColdPublicationDisposition,
)> {
    let wire = NativeWirePrimitives;
    let payload = field(entry, "payload")?;
    if !exact_keys(payload, &["bytesBase64", "identitySha256", "kind"]) {
        return None;
    }
    let payload_kind = text(payload, "kind")?;
    let payload_hash = hash_field(payload, "identitySha256")?;
    let payload_bytes = decode_body(payload)?;
    if format_digest(wire.sha256(&payload_bytes)) != payload_hash {
        return None;
    }
    let batch = field(entry, "receipts").is_some();
    let rain = payload_kind == "adjustment-rain-gate-control-projection/v3";
    // normalize the three exact source entry forms
    let (receipts, successors): (Vec<&JsonValue>, Vec<&JsonValue>) = if batch {
        if !exact_keys(
            entry,
            &[
                "payload",
                "publication",
                "receipts",
                "stageReceipt",
                "successors",
            ],
        ) {
            return None;
        }
        (
            array(entry, "receipts")?.iter().collect(),
            array(entry, "successors")?.iter().collect(),
        )
    } else {
        let mut keys = vec![
            "payload",
            "publication",
            "receipt",
            "stageReceipt",
            "successor",
        ];
        if rain {
            keys.extend(["rainControlState", "rainControlStateStageReceipt"]);
        }
        if !exact_keys(entry, &keys) {
            return None;
        }
        (
            vec![field(entry, "receipt")?],
            vec![field(entry, "successor")?],
        )
    };
    if receipts.is_empty() || receipts.len() > 168 || receipts.len() != successors.len() {
        return None;
    }
    let decoded = receipts
        .iter()
        .map(|receipt| codec.decode_revision_commit_value(receipt))
        .collect::<Option<Vec<_>>>()?;
    let first = decoded.first()?;
    let kind = text(receipts[0], "projectionKind")?;
    // validate both receipt identities and the exact successor document
    for (receipt, successor) in decoded.iter().zip(successors) {
        if !codec.validate_revision_commit_receipt_identity(receipt)
            || wire.encode_sorted_json(successor)? != codec.revision_successor_bytes(receipt)?
            || receipt.projectionIdentitySha256 != first.projectionIdentitySha256
            || receipt.projectionKind != first.projectionKind
            || receipt.projectionSha256 != first.projectionSha256
            || receipt.stageReceiptSha256 != first.stageReceiptSha256
        {
            return None;
        }
    }
    let publication = field(entry, "publication")?;
    if !exact_keys(publication, &["disposition", "value"]) {
        return None;
    }
    let publication_value = field(publication, "value")?;
    let publication_bytes = wire.encode_sorted_json(publication_value)?;
    let disposition = match text(publication, "disposition")?.as_str() {
        "published" => NativeRevisionColdPublicationDisposition::Published,
        "committed_unpublished_gap" => {
            NativeRevisionColdPublicationDisposition::CommittedUnpublishedGap
        }
        _ => return None,
    };
    let stage = field(entry, "stageReceipt")?;
    if kind == "shadow_prediction" {
        // bind the page to the complete retained capsule
        let capsule = codec.decode_capsule_document(&payload_bytes)?;
        if batch
            || capsule.revision_receipt != *first
            || text(&wire.decode_json(&payload_bytes)?, "contractVersion")? != payload_kind
            || wire.encode_sorted_json(stage)?
                != wire.encode_sorted_json(&shadow_stage_receipt_value(&capsule.stage_receipt))?
        {
            return None;
        }
        let metadata = &capsule.stage_input.metadata;
        if disposition == NativeRevisionColdPublicationDisposition::Published {
            let receipt =
                PrivateShadowReceiptCodec::decode_publish_receipt(codec, &publication_bytes)?;
            if receipt.predictionSha256 != metadata.predictionSha256
                || receipt.comparatorSha256 != capsule.stage_receipt.comparatorSha256
            {
                return None;
            }
        } else {
            codec.accepted_shadow_gap(&publication_bytes)?;
            let input = field(publication_value, "input")?;
            if text(input, "reason")? != "archive_publish_failed"
                || text(input, "dueKey")? != metadata.dueKey
                || text(input, "registrationSha256")? != metadata.registrationSha256
            {
                return None;
            }
        }
    } else {
        // match the original six body discriminators without inventing a new producer grammar
        let projection = wire.decode_json(&payload_bytes)?;
        if !matches!(
            payload_kind.as_str(),
            "adjustment-revision-projection/v1"
                | "adjustment-revision-batch-projection/v2"
                | "adjustment-rain-gate-feature-projection/v2"
                | "adjustment-rain-gate-control-projection/v3"
                | "adjustment-rain-fixed-gauge-target-projection/v1"
                | "adjustment-temperature-native-source-projection/v2"
        ) || text(&projection, "contractVersion")? != payload_kind
            || first.projectionIdentitySha256 != payload_hash
            || first.projectionSha256 != payload_hash
        {
            return None;
        }
        if batch
            && (!matches!(
                payload_kind.as_str(),
                "adjustment-revision-batch-projection/v2"
                    | "adjustment-rain-fixed-gauge-target-projection/v1"
            ) || !matches!(kind.as_str(), "actual_best_match" | "target_revision")
                || array(&projection, "rows")?.len() != decoded.len())
        {
            return None;
        }
        codec.decode_revision_stage_value(stage)?;
        if text(stage, "projectionIdentitySha256")? != first.projectionIdentitySha256
            || text(stage, "projectionSha256")? != first.projectionSha256
            || text(stage, "projectionKind")? != kind
        {
            return None;
        }
        if disposition == NativeRevisionColdPublicationDisposition::Published {
            if batch {
                let receipt = codec.decode_revision_batch_publish_value(publication_value)?;
                if receipt
                    .revisionReceiptSha256s
                    .iter()
                    .map(String::as_str)
                    .ne(decoded.iter().map(|r| r.receiptSha256.as_str()))
                {
                    return None;
                }
            } else {
                let receipt = codec.decode_revision_publish_value(publication_value)?;
                if receipt.revisionReceiptSha256 != first.receiptSha256 {
                    return None;
                }
            }
            if text(publication_value, "projectionIdentitySha256")? != payload_hash
                || text(publication_value, "projectionSha256")? != payload_hash
                || text(publication_value, "projectionKind")? != kind
            {
                return None;
            }
        } else {
            codec.accepted_revision_gap(&publication_bytes)?;
            let input = field(publication_value, "input")?;
            if text(input, "reason")? != "archive_publish_failed"
                || text(input, "projectionIdentitySha256")? != payload_hash
                || text(input, "projectionSha256")? != payload_hash
                || text(input, "projectionKind")? != kind
            {
                return None;
            }
        }
        if rain {
            verify_rain_state(entry, &projection, codec)?;
        }
    }
    Some((decoded, disposition))
}

// validate the source rain control state and its exact stage receipt binding
fn verify_rain_state(
    entry: &JsonValue,
    projection: &JsonValue,
    codec: &ArchiveWireCodec<NativeWirePrimitives>,
) -> Option<()> {
    let wire = NativeWirePrimitives;
    let state_member = field(entry, "rainControlState")?;
    if !exact_keys(
        state_member,
        &["bytesBase64", "identitySha256", "kind", "stateSha256"],
    ) {
        return None;
    }
    let bytes = decode_body(state_member)?;
    let state = wire.decode_json(&bytes)?;
    let identity = format_digest(wire.sha256(&bytes));
    let stage_value = field(entry, "rainControlStateStageReceipt")?;
    RainControlStateWireCodec::decode_stage_receipt(codec, &wire.encode_sorted_json(stage_value)?)?;
    if wire.encode_sorted_json(&state)? != bytes
        || text(state_member, "kind")? != "rain-maintenance-control-state/v1"
        || text(&state, "contractVersion")? != text(state_member, "kind")?
        || text(state_member, "identitySha256")? != identity
        || text(state_member, "stateSha256")? != identity
        || text(projection, "stateSha256")? != identity
        || text(stage_value, "stateSha256")? != identity
        || text(&state, "ordinalArtifactSha256")? != text(projection, "ordinalArtifactSha256")?
        || text(stage_value, "stageReceiptSha256")? != text(projection, "stateStageReceiptSha256")?
    {
        return None;
    }
    Some(())
}

// decode only the canonical standard base64 spelling
fn decode_body(value: &JsonValue) -> Option<Vec<u8>> {
    let wire = NativeWirePrimitives;
    let encoded = text(value, "bytesBase64")?;
    let bytes = wire.decode_standard_base64(&encoded)?;
    (wire.encode_standard_base64(&bytes)? == encoded).then_some(bytes)
}

// borrow one exact array
fn array<'a>(value: &'a JsonValue, key: &str) -> Option<&'a [JsonValue]> {
    match field(value, key)? {
        JsonValue::Array(values) => Some(values),
        _ => None,
    }
}

// admit a lowercase digest
fn hash_field(value: &JsonValue, key: &str) -> Option<String> {
    let digest = text(value, key)?;
    valid_hash(&digest).then_some(digest)
}

// admit a canonical unsigned ordinal
fn ordinal_field(value: &JsonValue, key: &str) -> Option<u64> {
    let spelling = text(value, key)?;
    let ordinal = spelling.parse::<u64>().ok()?;
    (ordinal.to_string() == spelling).then_some(ordinal)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path, process::Command};

    /// Admit real grouped and shadow Node pages with exact returned cursor and bytes.
    #[test]
    fn native_maintenance_cold_remote_page_matches_node() {
        let fixture =
            std::env::temp_dir().join(format!("weather-native-cold-remote-{}", std::process::id()));
        let oracle =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native-cold-custody-node-oracle.mjs");
        // independently materialize each producer response with original Node code
        for mode in ["mixed", "shadow"] {
            let root = fixture.join(mode);
            fs::create_dir_all(&root).expect("temporary source fixture");
            let setup = Command::new("node")
                .arg(&oracle)
                .arg("fixtures")
                .arg(&root)
                .arg(mode)
                .output()
                .expect("node fixture");
            assert!(
                setup.status.success(),
                "{}",
                String::from_utf8_lossy(&setup.stderr)
            );
            let operands = fs::read_to_string(root.join("operands.txt")).expect("operands");
            let args: Vec<_> = operands.lines().collect();
            let request = NativeRevisionColdPageRequest {
                watermark_archive_commit_ordinal: args[0].to_owned(),
                watermark_frontier_sha256: args[1].to_owned(),
                start_sha256: args[2].to_owned(),
                after_archive_commit_ordinal: args[3].to_owned(),
                after_frontier_sha256: args[4].to_owned(),
                previous_page_sha256: args[5].to_owned(),
            };
            let page_bytes = fs::read(root.join("page.json")).expect("source page");
            let page = verify_native_maintenance_cold_remote_page(&page_bytes, &request)
                .expect("verified source page");
            assert_eq!(page.canonical_bytes, page_bytes);
            assert_eq!(page.request, request);
            assert_eq!(page.page_sha256, args[6]);
            assert!(!page.entries.is_empty());
        }
        fs::remove_dir_all(fixture).expect("clean isolated test fixture");
    }
}
