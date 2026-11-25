//! Bounded native open cold-custody pack checkpoint and sealed graph projection.

use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io};
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeMaintenanceArchiveSegment, NativeWirePrimitives,
    maintenance_canonical_json::{exact_keys, field, text, valid_hash},
    native_maintenance_cold_archive_segment::build_native_maintenance_cold_archive_segment,
    native_maintenance_cold_remote_documents::verify_native_revision_cold_start_document,
    native_maintenance_cold_remote_page::verify_native_maintenance_cold_remote_page,
    native_maintenance_open_pack_store::{
        NATIVE_COLD_PACK_MAXIMUM_BYTES, NATIVE_COLD_PACK_METADATA_MAXIMUM_BYTES,
        NativeMaintenanceOpenPackStore,
    },
    native_maintenance_retirement_files_v2::format_digest,
    native_maintenance_revision_cold_drain::{
        NativeRevisionColdPageRequest, VerifiedNativeRevisionColdPage,
        VerifiedNativeRevisionColdStart,
    },
};

const VERSION: &str = "adjustment-revision-custody-open-pack/v1";
const CHECKPOINT: &str = "adjustment-revision-custody-open-pack-checkpoint/v1";
const MAXIMUM_PAGES: usize = 19;

/// Exact immutable per-page open pack metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeMaintenanceOpenPackPageRecord {
    pub offset: usize,
    pub start_length: usize,
    pub page_length: usize,
    pub member_root_sha256: String,
    pub start_member_sha256: Option<String>,
    pub page_sha256: String,
    pub start_sha256: String,
    pub transfer_sha256: String,
}

/// Restored source page and its exact checkpoint range.
pub struct NativeMaintenanceOpenPackPage {
    pub record: NativeMaintenanceOpenPackPageRecord,
    pub page: VerifiedNativeRevisionColdPage,
    pub start: Option<VerifiedNativeRevisionColdStart>,
}

/// Complete durable current open pack after independent replay.
#[derive(Default)]
pub struct NativeMaintenanceOpenPackStatus {
    pub byte_length: usize,
    pub metadata_byte_length: usize,
    pub checkpoint_sha256: Option<String>,
    pub pages: Vec<NativeMaintenanceOpenPackPage>,
}

/// Exact verified archive graph and mapping consumed by the seal owner.
pub struct NativeMaintenanceSealedOpenPack {
    pub checkpoint_sha256: String,
    pub records: Vec<NativeMaintenanceOpenPackPageRecord>,
    pub segment: NativeMaintenanceArchiveSegment,
}

/// Distinguish a full pack that must seal from all other failures.
#[derive(Debug)]
pub enum NativeMaintenanceOpenPackError {
    Storage(io::Error),
    Invalid,
    Full,
}
impl From<io::Error> for NativeMaintenanceOpenPackError {
    /// Preserve the owning filesystem failure.
    fn from(error: io::Error) -> Self {
        Self::Storage(error)
    }
}

type Result<T> = std::result::Result<T, NativeMaintenanceOpenPackError>;

/// Complete fixed open-pack protocol over the actual durable byte store.
pub struct NativeMaintenanceOpenPack<Store> {
    pub store: Store,
}

impl<Store: NativeMaintenanceOpenPackStore> NativeMaintenanceOpenPack<Store> {
    /// Reopen every retained range and recompute all metadata and graph identities.
    pub fn inspect(&mut self) -> Result<NativeMaintenanceOpenPackStatus> {
        let metadata = self.store.read_metadata()?;
        let (records, payload_bytes) =
            parse_metadata(&metadata).ok_or(NativeMaintenanceOpenPackError::Invalid)?;
        let size = self.store.payload_size()?;
        // discard only the uncheckpointed raw suffix
        if size > payload_bytes {
            self.store.truncate_payload(payload_bytes)?;
        } else if size != payload_bytes {
            return Err(NativeMaintenanceOpenPackError::Invalid);
        }
        let mut pages = Vec::with_capacity(records.len());
        let mut raw_hash = Sha256::new();
        // replay every individual range before exposing a checkpoint
        for record in records {
            let bytes = self
                .store
                .read_payload(record.offset, record.start_length + record.page_length)?;
            if bytes.len() != record.start_length + record.page_length
                || digest(&bytes) != record.transfer_sha256
            {
                return Err(NativeMaintenanceOpenPackError::Invalid);
            }
            raw_hash.update(&bytes);
            let (start_bytes, page_bytes) = bytes.split_at(record.start_length);
            let start = if start_bytes.is_empty() {
                None
            } else {
                let start = verify_native_revision_cold_start_document(
                    start_bytes,
                    &mut NativeWirePrimitives,
                    &NativeWirePrimitives,
                )
                .map_err(|_| NativeMaintenanceOpenPackError::Invalid)?;
                if start.canonical_bytes != start_bytes {
                    return Err(NativeMaintenanceOpenPackError::Invalid);
                }
                Some(start)
            };
            let request =
                request_from_page(page_bytes).ok_or(NativeMaintenanceOpenPackError::Invalid)?;
            let page = verify_native_maintenance_cold_remote_page(page_bytes, &request)
                .ok_or(NativeMaintenanceOpenPackError::Invalid)?;
            if page.canonical_bytes != page_bytes {
                return Err(NativeMaintenanceOpenPackError::Invalid);
            }
            let graph = build_native_maintenance_cold_archive_segment(&page, start.as_ref())
                .map_err(|_| NativeMaintenanceOpenPackError::Invalid)?;
            // bind every source coordinate to the atomic range metadata
            if page.page_sha256 != record.page_sha256
                || page.request.start_sha256 != record.start_sha256
                || graph.member_root_sha256 != record.member_root_sha256
                || graph.start_member_sha256 != record.start_member_sha256
            {
                return Err(NativeMaintenanceOpenPackError::Invalid);
            }
            pages.push(NativeMaintenanceOpenPackPage {
                record,
                page,
                start,
            });
        }
        let checkpoint_sha256 = (!pages.is_empty()).then(|| {
            digest(
                format!(
                    "{CHECKPOINT}\n{}\n{}\n",
                    digest(&metadata),
                    format_digest(raw_hash.finalize().into())
                )
                .as_bytes(),
            )
        });
        Ok(NativeMaintenanceOpenPackStatus {
            byte_length: payload_bytes,
            checkpoint_sha256,
            metadata_byte_length: metadata.len(),
            pages,
        })
    }

    /// Commit one exact transfer before returning its verified custody checkpoint.
    pub fn append(
        &mut self,
        page: &VerifiedNativeRevisionColdPage,
        start: Option<&VerifiedNativeRevisionColdStart>,
    ) -> Result<NativeMaintenanceOpenPackStatus> {
        if page.entries.is_empty() {
            return Err(NativeMaintenanceOpenPackError::Invalid);
        }
        let graph = build_native_maintenance_cold_archive_segment(page, start)
            .map_err(|_| NativeMaintenanceOpenPackError::Invalid)?;
        let current = self.inspect()?;
        // converge only the byte-identical most recent page retry
        if let Some(last) = current.pages.last() {
            if last.page.page_sha256 == page.page_sha256 {
                if last.page.canonical_bytes != page.canonical_bytes
                    || last.start.as_ref().map(|s| s.canonical_bytes.as_slice())
                        != start.map(|s| s.canonical_bytes.as_slice())
                {
                    return Err(NativeMaintenanceOpenPackError::Invalid);
                }
                return Ok(current);
            }
            if page.request.after_archive_commit_ordinal != last.page.next_archive_commit_ordinal
                || page.request.after_frontier_sha256 != last.page.next_frontier_sha256
                || if page.request.start_sha256 == last.page.request.start_sha256 {
                    start.is_some() || page.request.previous_page_sha256 != last.page.page_sha256
                } else {
                    !last.page.eof || start.is_none()
                }
            {
                return Err(NativeMaintenanceOpenPackError::Invalid);
            }
        }
        if current
            .pages
            .iter()
            .any(|r| r.page.page_sha256 == page.page_sha256)
        {
            return Err(NativeMaintenanceOpenPackError::Invalid);
        }
        let start_bytes = start.map_or(&[][..], |s| s.canonical_bytes.as_slice());
        let page_bytes = page.canonical_bytes.as_slice();
        if current.pages.len() >= MAXIMUM_PAGES
            || current.byte_length + start_bytes.len() + page_bytes.len()
                > NATIVE_COLD_PACK_MAXIMUM_BYTES
        {
            return Err(NativeMaintenanceOpenPackError::Full);
        }
        // commit raw ranges before the metadata checkpoint is atomically replaced
        if !start_bytes.is_empty() {
            self.store.append_payload(start_bytes)?;
        }
        self.store.append_payload(page_bytes)?;
        let mut raw = Sha256::new();
        raw.update(start_bytes);
        raw.update(page_bytes);
        let record = NativeMaintenanceOpenPackPageRecord {
            offset: current.byte_length,
            start_length: start_bytes.len(),
            page_length: page_bytes.len(),
            member_root_sha256: graph.member_root_sha256,
            start_member_sha256: graph.start_member_sha256,
            page_sha256: page.page_sha256.clone(),
            start_sha256: page.request.start_sha256.clone(),
            transfer_sha256: format_digest(raw.finalize().into()),
        };
        let mut records = current
            .pages
            .into_iter()
            .map(|p| p.record)
            .collect::<Vec<_>>();
        records.push(record.clone());
        self.store.write_metadata(
            &encode_metadata(&records).ok_or(NativeMaintenanceOpenPackError::Invalid)?,
        )?;
        let verified = self.inspect()?;
        // return only the reopened durable matching source page
        if verified.pages.last().is_none_or(|p| p.record != record) {
            return Err(NativeMaintenanceOpenPackError::Invalid);
        }
        Ok(verified)
    }

    /// Merge the independently verified page segments without repeated members or relations.
    pub fn build_sealed_segment(&mut self) -> Result<NativeMaintenanceSealedOpenPack> {
        let status = self.inspect()?;
        let checkpoint_sha256 = status
            .checkpoint_sha256
            .ok_or(NativeMaintenanceOpenPackError::Invalid)?;
        let mut segment = NativeMaintenanceArchiveSegment {
            members: Vec::new(),
            cross_links: Vec::new(),
        };
        let mut identities = BTreeSet::new();
        let mut relations = BTreeSet::new();
        let mut records = Vec::with_capacity(status.pages.len());
        // preserve source page order and unique graph identities
        for retained in status.pages {
            let graph = build_native_maintenance_cold_archive_segment(
                &retained.page,
                retained.start.as_ref(),
            )
            .map_err(|_| NativeMaintenanceOpenPackError::Invalid)?;
            for member in graph.segment.members {
                if !identities.insert(member.identity_sha256) {
                    return Err(NativeMaintenanceOpenPackError::Invalid);
                }
                segment.members.push(member);
            }
            for link in graph.segment.cross_links {
                if !relations.insert((
                    link.from_identity_sha256.clone(),
                    link.relation.clone(),
                    link.to_identity_sha256.clone(),
                )) {
                    return Err(NativeMaintenanceOpenPackError::Invalid);
                }
                segment.cross_links.push(link);
            }
            records.push(retained.record);
        }
        Ok(NativeMaintenanceSealedOpenPack {
            checkpoint_sha256,
            records,
            segment,
        })
    }

    /// Remove only the exact pack already sealed and durably mapped by the caller.
    pub fn remove_sealed(&mut self, checkpoint: &str) -> Result<()> {
        if self.inspect()?.checkpoint_sha256.as_deref() != Some(checkpoint) {
            return Err(NativeMaintenanceOpenPackError::Invalid);
        }
        self.store.remove()?;
        let empty = self.inspect()?;
        if empty.byte_length != 0 || empty.metadata_byte_length != 0 {
            return Err(NativeMaintenanceOpenPackError::Invalid);
        }
        Ok(())
    }
}

// parse exact bounded canonical metadata and contiguous source ranges
fn parse_metadata(bytes: &[u8]) -> Option<(Vec<NativeMaintenanceOpenPackPageRecord>, usize)> {
    if bytes.is_empty() {
        return Some((Vec::new(), 0));
    }
    if bytes.len() > NATIVE_COLD_PACK_METADATA_MAXIMUM_BYTES {
        return None;
    }
    let wire = NativeWirePrimitives;
    let value = wire.decode_json(bytes)?;
    if wire.encode_sorted_json(&value)? != bytes
        || !exact_keys(&value, &["contractVersion", "pages", "payloadBytes"])
        || text(&value, "contractVersion")? != VERSION
    {
        return None;
    }
    let JsonValue::Array(pages) = field(&value, "pages")? else {
        return None;
    };
    let payload = integer(&value, "payloadBytes")?;
    if pages.is_empty()
        || pages.len() > MAXIMUM_PAGES
        || payload == 0
        || payload > NATIVE_COLD_PACK_MAXIMUM_BYTES
    {
        return None;
    }
    let mut records = Vec::with_capacity(pages.len());
    let mut offset = 0usize;
    // each raw page range must be contiguous and bound to the retained start member
    for page in pages {
        if !exact_keys(
            page,
            &[
                "memberRootSha256",
                "offset",
                "pageLength",
                "pageSha256",
                "startLength",
                "startMemberSha256",
                "startSha256",
                "transferSha256",
            ],
        ) {
            return None;
        }
        let record = NativeMaintenanceOpenPackPageRecord {
            offset: integer(page, "offset")?,
            page_length: integer(page, "pageLength")?,
            start_length: integer(page, "startLength")?,
            start_member_sha256: match field(page, "startMemberSha256")? {
                JsonValue::Null => None,
                _ => Some(hash(page, "startMemberSha256")?),
            },
            member_root_sha256: hash(page, "memberRootSha256")?,
            page_sha256: hash(page, "pageSha256")?,
            start_sha256: hash(page, "startSha256")?,
            transfer_sha256: hash(page, "transferSha256")?,
        };
        if record.offset != offset
            || record.page_length < 2
            || (record.start_length == 0) != record.start_member_sha256.is_none()
        {
            return None;
        }
        offset = offset
            .checked_add(record.start_length)?
            .checked_add(record.page_length)?;
        records.push(record);
    }
    (offset == payload).then_some((records, payload))
}

// encode exactly the source metadata preimage
fn encode_metadata(records: &[NativeMaintenanceOpenPackPageRecord]) -> Option<Vec<u8>> {
    let pages = records
        .iter()
        .map(|r| {
            object(vec![
                ("offset", JsonValue::Number(r.offset as f64)),
                ("pageLength", JsonValue::Number(r.page_length as f64)),
                ("startLength", JsonValue::Number(r.start_length as f64)),
                ("memberRootSha256", string(&r.member_root_sha256)),
                ("pageSha256", string(&r.page_sha256)),
                ("startSha256", string(&r.start_sha256)),
                ("transferSha256", string(&r.transfer_sha256)),
                (
                    "startMemberSha256",
                    r.start_member_sha256
                        .as_deref()
                        .map(string)
                        .unwrap_or(JsonValue::Null),
                ),
            ])
        })
        .collect();
    let payload: usize = records.iter().map(|r| r.start_length + r.page_length).sum();
    NativeWirePrimitives.encode_sorted_json(&object(vec![
        ("contractVersion", string(VERSION)),
        ("pages", JsonValue::Array(pages)),
        ("payloadBytes", JsonValue::Number(payload as f64)),
    ]))
}

// recover the exact typed request from an independently authenticated retained page
fn request_from_page(bytes: &[u8]) -> Option<NativeRevisionColdPageRequest> {
    let page = NativeWirePrimitives.decode_json(bytes)?;
    Some(NativeRevisionColdPageRequest {
        after_archive_commit_ordinal: text(&page, "afterArchiveCommitOrdinal")?,
        after_frontier_sha256: text(&page, "afterFrontierSha256")?,
        previous_page_sha256: text(&page, "previousPageSha256")?,
        start_sha256: text(&page, "startSha256")?,
        watermark_archive_commit_ordinal: text(&page, "watermarkArchiveCommitOrdinal")?,
        watermark_frontier_sha256: text(&page, "watermarkFrontierSha256")?,
    })
}

// make a bounded canonical numeric offset
fn integer(value: &JsonValue, key: &str) -> Option<usize> {
    let JsonValue::Number(n) = field(value, key)? else {
        return None;
    };
    (n.fract() == 0.0 && *n >= 0.0 && *n <= NATIVE_COLD_PACK_MAXIMUM_BYTES as f64)
        .then_some(*n as usize)
}

// admit a canonical lowercase content identity
fn hash(value: &JsonValue, key: &str) -> Option<String> {
    let v = text(value, key)?;
    valid_hash(&v).then_some(v)
}

// hash exact canonical bytes
fn digest(bytes: &[u8]) -> String {
    format_digest(NativeWirePrimitives.sha256(bytes))
}

// construct source canonical JSON values
fn object(fields: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(k, v)| (k.encode_utf16().collect(), v))
            .collect(),
    )
}

// encode the generated UTF-16 contract string
fn string(v: &str) -> JsonValue {
    JsonValue::String(v.encode_utf16().collect())
}

#[cfg(test)]
#[path = "native_maintenance_open_pack_test.rs"]
mod tests;
