//! Original cold-transfer cursor and bounded two-payload wire page.

use crate::revision_archive_canonical::{RevisionArchiveCanonicalCodec, lower_hex};
use crate::revision_gap_transfer::RevisionGapSha256;

const VERSION: &str = "adjustment-revision-cold-page/v1";
const GENESIS: &[u8] = b"adjustment-revision-frontier/v1\n0\n";
/// Maximum encoded page, including its newline and content identity.
pub const REVISION_COLD_PAGE_MAXIMUM_BYTES: usize = 4_832 * 1_024;

/// One exact ordinal/frontier coordinate supplied by the workstation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionColdCursor {
    pub ordinal: u64,
    pub frontier: RevisionGapSha256,
}

/// The original six ordered page operands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionColdPageRequest {
    pub watermark: RevisionColdCursor,
    pub start: RevisionGapSha256,
    pub after: RevisionColdCursor,
    pub previous: RevisionGapSha256,
}

/// Failure classes in the page command owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionColdPageError {
    Arguments,
    Ordinal,
    Digest,
    Cursor,
    Storage,
    Successor,
    Payload,
    PageLimit,
}

/// Parse the exact retained command, leaving other CLI verbs to their owners.
pub fn parse_revision_cold_page_command(
    action: &str,
    operands: &[&str],
) -> Result<Option<RevisionColdPageRequest>, RevisionColdPageError> {
    use RevisionColdPageError as Error;
    // leave operands untouched for commands owned by other modules
    if action != "revision-cold-page-v1" {
        return Ok(None);
    }
    let [
        watermark,
        watermark_frontier,
        start,
        after,
        after_frontier,
        previous,
    ] = operands
    else {
        return Err(Error::Arguments);
    };
    let digest = |value: &str| RevisionGapSha256::from_lower_hex(value).ok_or(Error::Digest);
    Ok(Some(RevisionColdPageRequest {
        watermark: RevisionColdCursor {
            ordinal: parse_ordinal(watermark)?,
            frontier: digest(watermark_frontier)?,
        },
        start: digest(start)?,
        after: RevisionColdCursor {
            ordinal: parse_ordinal(after)?,
            frontier: digest(after_frontier)?,
        },
        previous: digest(previous)?,
    }))
}

/// Admit only the existing canonical uint64 spelling.
fn parse_ordinal(value: &str) -> Result<u64, RevisionColdPageError> {
    let number: u64 = value.parse().map_err(|_| RevisionColdPageError::Ordinal)?;
    // reject noncanonical decimal aliases
    if number.to_string() != value {
        return Err(RevisionColdPageError::Ordinal);
    }
    Ok(number)
}

/// One independently admitted full atomic entry with all direct receipt coordinates.
pub(crate) struct RevisionColdPageEntry {
    pub chain: Vec<(RevisionGapSha256, RevisionColdCursor)>,
    pub canonical: Vec<u8>,
}

/// Select at most two complete entries from their immutable watermark chain.
pub(crate) fn compose_revision_cold_page(
    request: RevisionColdPageRequest,
    codec: &impl RevisionArchiveCanonicalCodec,
    require_directories: impl FnOnce() -> Result<(), RevisionColdPageError>,
    mut read_entry: impl FnMut(
        RevisionColdCursor,
        RevisionColdCursor,
    ) -> Result<RevisionColdPageEntry, RevisionColdPageError>,
) -> Result<Vec<u8>, RevisionColdPageError> {
    use RevisionColdPageError as Error;
    // retain the original argument-before-directory ordering
    if request.after.ordinal > request.watermark.ordinal {
        return Err(Error::Cursor);
    }
    require_directories()?;
    // enforce the reserved genesis and completed cursor identities
    if (request.after.ordinal == 0
        && (request.after.frontier != RevisionGapSha256::admitted(codec.sha256(GENESIS))
            || request.previous != request.start))
        || (request.after.ordinal == request.watermark.ordinal
            && request.after != request.watermark)
    {
        return Err(Error::Cursor);
    }

    let mut next = request.after;
    let mut entries = Vec::with_capacity(2);
    // defer only a complete second entry when its page exceeds the original ceiling
    while entries.len() < 2 && next.ordinal < request.watermark.ordinal {
        let entry = read_entry(next, request.watermark)?;
        let mut candidate_next = next;
        // verify every receipt coordinate, rather than splittable payload count
        for (predecessor, cursor) in &entry.chain {
            if *predecessor != candidate_next.frontier
                || cursor.ordinal <= candidate_next.ordinal
                || cursor.ordinal > request.watermark.ordinal
            {
                return Err(Error::Successor);
            }
            candidate_next = *cursor;
        }
        // a payload must represent at least one receipt and a complete canonical object
        if entry.chain.is_empty()
            || !entry.canonical.starts_with(b"{")
            || !entry.canonical.ends_with(b"}\n")
        {
            return Err(Error::Payload);
        }
        entries.push(entry.canonical);
        if page_length(request, candidate_next, &entries) > REVISION_COLD_PAGE_MAXIMUM_BYTES {
            entries.pop();
            if entries.is_empty() {
                return Err(Error::PageLimit);
            }
            break;
        }
        next = candidate_next;
    }
    // only exact equality with both watermark coordinates can report EOF
    if (next.ordinal >= request.watermark.ordinal || entries.is_empty())
        && next != request.watermark
    {
        return Err(Error::Successor);
    }
    let page = encode_page(request, next, &entries, codec);
    // bound the final representation independently of selected entries
    if page.len() > REVISION_COLD_PAGE_MAXIMUM_BYTES {
        return Err(Error::PageLimit);
    }
    Ok(page)
}

/// Render the fixed envelope portions without copying or hashing payload bytes.
fn page_parts(
    request: RevisionColdPageRequest,
    next: RevisionColdCursor,
) -> (String, String, String) {
    let hash = |value: RevisionGapSha256| lower_hex(*value.as_bytes());
    let prefix = format!(
        "{{\"afterArchiveCommitOrdinal\":\"{}\",\"afterFrontierSha256\":\"{}\",\"contractVersion\":\"{VERSION}\",\"entries\":[",
        request.after.ordinal,
        hash(request.after.frontier)
    );
    let middle = format!(
        "],\"eof\":{},\"nextArchiveCommitOrdinal\":\"{}\",\"nextFrontierSha256\":\"{}\"",
        next == request.watermark,
        next.ordinal,
        hash(next.frontier)
    );
    let suffix = format!(
        ",\"previousPageSha256\":\"{}\",\"startSha256\":\"{}\",\"watermarkArchiveCommitOrdinal\":\"{}\",\"watermarkFrontierSha256\":\"{}\"}}\n",
        hash(request.previous),
        hash(request.start),
        request.watermark.ordinal,
        hash(request.watermark.frontier)
    );
    (prefix, middle, suffix)
}

const PAGE_IDENTITY_BYTES: usize = b",\"pageSha256\":\"".len() + 64 + 1;

/// Calculate the exact canonical page length before making a large wire buffer.
fn page_length(
    request: RevisionColdPageRequest,
    next: RevisionColdCursor,
    entries: &[Vec<u8>],
) -> usize {
    let (prefix, middle, suffix) = page_parts(request, next);
    prefix.len()
        + middle.len()
        + suffix.len()
        + PAGE_IDENTITY_BYTES
        + entries.iter().map(|entry| entry.len() - 1).sum::<usize>()
        + entries.len().saturating_sub(1)
}

/// Serialize the fixed envelope in canonical key order around admitted entry bytes.
fn encode_page(
    request: RevisionColdPageRequest,
    next: RevisionColdCursor,
    entries: &[Vec<u8>],
    codec: &impl RevisionArchiveCanonicalCodec,
) -> Vec<u8> {
    let (prefix, middle, suffix) = page_parts(request, next);
    let mut bytes = Vec::with_capacity(
        prefix.len()
            + middle.len()
            + suffix.len()
            + PAGE_IDENTITY_BYTES
            + entries.iter().map(|entry| entry.len() - 1).sum::<usize>()
            + entries.len().saturating_sub(1),
    );
    bytes.extend_from_slice(prefix.as_bytes());
    // reuse the original entry bytes without reencoding large base64 members
    for (index, entry) in entries.iter().enumerate() {
        if index != 0 {
            bytes.push(b',');
        }
        bytes.extend_from_slice(&entry[..entry.len() - 1]);
    }
    bytes.extend_from_slice(middle.as_bytes());
    let identity_position = bytes.len();
    bytes.extend_from_slice(suffix.as_bytes());
    let identity = format!(",\"pageSha256\":\"{}\"", lower_hex(codec.sha256(&bytes)));
    // add the hash at its lexicographic canonical position
    bytes.splice(identity_position..identity_position, identity.bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::process::{Command, Stdio};

    struct HashCodec;
    impl RevisionArchiveCanonicalCodec for HashCodec {
        // time is not part of the page envelope
        fn canonical_utc_milliseconds(&self, _: &str) -> Option<i64> {
            None
        }

        // use the existing host hash command only in the fixture
        fn sha256(&self, bytes: &[u8]) -> [u8; 32] {
            let mut child = Command::new("sha256sum")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .expect("fixture hash");
            child.stdin.take().unwrap().write_all(bytes).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            let hash = std::str::from_utf8(&output.stdout[..64]).unwrap();
            *RevisionGapSha256::from_lower_hex(hash).unwrap().as_bytes()
        }
    }

    // construct an explicit known cursor chain
    fn cursor(ordinal: u64, tag: u8) -> RevisionColdCursor {
        RevisionColdCursor {
            ordinal,
            frontier: RevisionGapSha256::admitted([tag; 32]),
        }
    }

    // keep a complete bounded synthetic entry for selection-only tests
    fn entry(
        from: RevisionColdCursor,
        to: RevisionColdCursor,
        payload_bytes: usize,
    ) -> RevisionColdPageEntry {
        let mut canonical = b"{\"payload\":\"".to_vec();
        canonical.resize(canonical.len() + payload_bytes, b'a');
        canonical.extend_from_slice(b"\"}\n");
        RevisionColdPageEntry {
            chain: vec![(from.frontier, to)],
            canonical,
        }
    }

    #[test]
    // compare exact canonical bytes to the original empty-page command
    fn empty_watermark_matches_node_reader() {
        let start = RevisionGapSha256::admitted([4; 32]);
        let genesis = RevisionColdCursor {
            ordinal: 0,
            frontier: RevisionGapSha256::admitted(HashCodec.sha256(GENESIS)),
        };
        let request = RevisionColdPageRequest {
            watermark: genesis,
            after: genesis,
            start,
            previous: start,
        };
        let original = Command::new("node")
            .current_dir(format!("{}/../../..", env!("CARGO_MANIFEST_DIR")))
            .args(["--input-type=module", "--eval", r#"
import {mkdtemp, mkdir, rm, chmod} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {createHash} from 'node:crypto';
import {readAdjustmentRevisionColdPage} from './deploy/scripts/adjustment-evidence-store.mjs';
const root=await mkdtemp(join(tmpdir(),'weather-cold-empty-'));
try {
  await chmod(root,0o700);
  for (const dir of ['revision-commit-receipts','revision-frontier-successors','revision-projections','shadow-revision-capsules','rain-control-states','rain-control-state-stage-receipts']) await mkdir(join(root,dir),{mode:0o700});
  const frontier=createHash('sha256').update('adjustment-revision-frontier/v1\n0\n').digest('hex');
  const start='04'.repeat(32);
  const page=await readAdjustmentRevisionColdPage({root,afterArchiveCommitOrdinal:'0',afterFrontierSha256:frontier,previousPageSha256:start,startSha256:start,watermarkArchiveCommitOrdinal:'0',watermarkFrontierSha256:frontier});
  process.stdout.write(JSON.stringify(Object.fromEntries(Object.entries(page).sort(([a],[b])=>a<b?-1:a>b?1:0)))+'\n');
} finally {await rm(root,{recursive:true,force:true});}
"#])
            .output()
            .expect("original cold page");
        assert!(
            original.status.success(),
            "{}",
            String::from_utf8_lossy(&original.stderr)
        );
        let actual = compose_revision_cold_page(
            request,
            &HashCodec,
            || Ok(()),
            |_, _| panic!("empty watermark reads no entries"),
        )
        .unwrap();
        assert_eq!(actual, original.stdout);
    }

    #[test]
    // preserve the source's atomic group and second-payload deferral
    fn second_payload_defers_without_advancing_cursor() {
        let after = cursor(1, 1);
        let middle = cursor(7, 7);
        let watermark = cursor(9, 9);
        let request = RevisionColdPageRequest {
            after,
            watermark,
            start: after.frontier,
            previous: after.frontier,
        };
        let mut calls = Vec::new();
        let result = compose_revision_cold_page(
            request,
            &HashCodec,
            || Ok(()),
            |current, _| {
                calls.push(current.ordinal);
                Ok(entry(
                    current,
                    if current == after { middle } else { watermark },
                    3 * 1024 * 1024,
                ))
            },
        )
        .unwrap();
        assert_eq!(calls, vec![1, 7]);
        assert!(result.len() < REVISION_COLD_PAGE_MAXIMUM_BYTES);
        let text = String::from_utf8(result).unwrap();
        assert!(text.contains("\"nextArchiveCommitOrdinal\":\"7\""));
        assert!(text.contains("\"eof\":false"));
    }

    #[test]
    // retain an entire group only when its watermark is complete
    fn grouped_receipts_do_not_split_at_watermark() {
        let after = cursor(1, 1);
        let second = cursor(2, 2);
        let watermark = cursor(3, 3);
        let request = RevisionColdPageRequest {
            after,
            watermark,
            start: after.frontier,
            previous: after.frontier,
        };
        let actual = compose_revision_cold_page(
            request,
            &HashCodec,
            || Ok(()),
            |_, _| {
                let mut group = entry(after, second, 8);
                group.chain.push((second.frontier, watermark));
                Ok(group)
            },
        )
        .unwrap();
        let text = String::from_utf8(actual).unwrap();
        assert!(text.contains("\"nextArchiveCommitOrdinal\":\"3\""));
        assert!(text.contains("\"eof\":true"));
    }

    #[test]
    // accept exactly the source ceiling and refuse a one-byte larger first payload
    fn exact_page_byte_ceiling() {
        let after = cursor(1, 1);
        let watermark = cursor(2, 2);
        let request = RevisionColdPageRequest {
            after,
            watermark,
            start: after.frontier,
            previous: after.frontier,
        };
        let empty_len = page_length(request, watermark, &[entry(after, watermark, 0).canonical]);
        let payload_len = REVISION_COLD_PAGE_MAXIMUM_BYTES - empty_len;
        let exact = compose_revision_cold_page(
            request,
            &HashCodec,
            || Ok(()),
            |_, _| Ok(entry(after, watermark, payload_len)),
        )
        .unwrap();
        assert_eq!(exact.len(), REVISION_COLD_PAGE_MAXIMUM_BYTES);
        assert_eq!(
            compose_revision_cold_page(
                request,
                &HashCodec,
                || Ok(()),
                |_, _| { Ok(entry(after, watermark, payload_len + 1)) }
            ),
            Err(RevisionColdPageError::PageLimit)
        );
    }

    #[test]
    // interpret canonical decimal and digest operands through the original CLI order
    fn command_uses_exact_operand_order() {
        let start = "ab".repeat(32);
        let after = "cd".repeat(32);
        let request = parse_revision_cold_page_command(
            "revision-cold-page-v1",
            &["18446744073709551615", &start, &start, "7", &after, &after],
        )
        .unwrap()
        .unwrap();
        assert_eq!(request.watermark.ordinal, u64::MAX);
        assert_eq!(request.after.ordinal, 7);
        assert_eq!(
            request.after.frontier,
            RevisionGapSha256::admitted([0xcd; 32])
        );
        assert_eq!(parse_revision_cold_page_command("other", &[]), Ok(None));
    }
}
