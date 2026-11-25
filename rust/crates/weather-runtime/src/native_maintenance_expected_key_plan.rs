//! Compact rolling registration expected-key identities derived from every scheduled source cycle.

use std::collections::{BTreeMap, HashMap};

use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};

use crate::{
    NativeMaintenanceCaptureFamily, NativePredecessorModelCalendar, NativeWirePrimitives,
    native_maintenance_policy_comparison::{object, string},
    native_maintenance_retirement_files_v2::format_digest,
};

const HOUR: i64 = 3_600_000;
const DAY: i64 = 24 * HOUR;
const CYCLE: i64 = 6 * HOUR;

/// Exact compact registration expected-key plan and independently recomputed root.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeMaintenanceExpectedKeyPlan {
    pub descriptor: JsonValue,
    pub bytes: Vec<u8>,
    pub expected_plan_sha256: String,
    pub reserved_key_sha256: String,
}

/// Enumerate original fixed provider cycles and target cells before any value-bearing observations.
pub fn build_native_maintenance_expected_key_plan(
    family: NativeMaintenanceCaptureFamily,
    interval_start_at: &str,
    interval_end_at: &str,
) -> Option<NativeMaintenanceExpectedKeyPlan> {
    let wire = NativeWirePrimitives;
    let calendar = NativePredecessorModelCalendar::new()?;
    let start = wire.canonical_utc_milliseconds(interval_start_at)?;
    let end = wire.canonical_utc_milliseconds(interval_end_at)?;
    let start_date = calendar.features(interval_start_at).ok()?.local_date;
    let end_date = calendar.features(interval_end_at).ok()?.local_date;
    let start_day = date_day(&start_date)?;
    let end_day = date_day(&end_date)?;
    let (chunk_count, interval_days, first_lead, row_count) = match family {
        NativeMaintenanceCaptureFamily::Temperature => (27_usize, 366, 1, 12),
        NativeMaintenanceCaptureFamily::Wind => (27, 366, 1, 168),
        NativeMaintenanceCaptureFamily::Rain => (24, 334, 9, 23),
    };
    // require the exact frozen local interval geometry including DST transitions
    if end <= start || end_day - start_day != interval_days {
        return None;
    }
    let max_lead = first_lead + row_count - 1;
    let first_cycle = (start.checked_sub(max_lead * HOUR)?).div_euclid(CYCLE) * CYCLE;
    let mut groups = BTreeMap::<(usize, String), Vec<String>>::new();
    let mut day_cache = HashMap::<i64, i64>::new();
    let mut cycle = first_cycle;
    // each cell belongs to exactly one target window and source capture date
    while cycle < end {
        let due_at = wire.format_utc_milliseconds(cycle.checked_add(35 * 60_000)?)?;
        let due_key = format!("capture/{due_at}");
        let capture_date = calendar.features(&due_at).ok()?.local_date;
        for row in 0..row_count {
            let model_lead = first_lead + row;
            let valid = cycle.checked_add(model_lead * HOUR)?;
            if valid < start || valid >= end {
                continue;
            }
            let day = if let Some(day) = day_cache.get(&valid) {
                *day
            } else {
                let timestamp = wire.format_utc_milliseconds(valid)?;
                let day = date_day(&calendar.features(&timestamp).ok()?.local_date)?;
                day_cache.insert(valid, day);
                day
            };
            let index = usize::try_from((day - start_day).div_euclid(14)).ok()?;
            if index >= chunk_count {
                return None;
            }
            let keys = groups.entry((index, capture_date.clone())).or_default();
            let lead = row + 1;
            if family == NativeMaintenanceCaptureFamily::Wind {
                keys.push(format!("{due_key}/{lead}/windSpeedMps"));
                // the original unsupported 49-72 gust band contributes no key
                if !(49..=72).contains(&model_lead) {
                    keys.push(format!("{due_key}/{lead}/windGustMps"));
                }
            } else {
                keys.push(format!("{due_key}/{lead}/{}", family.wire()));
            }
        }
        cycle = cycle.checked_add(CYCLE)?;
    }
    let mut chunks = Vec::with_capacity(chunk_count);
    let mut logical_roots = Vec::with_capacity(chunk_count);
    let mut groups = groups.into_iter().peekable();
    // hash each physical part and then each logical window in exact source order
    for index in 0..chunk_count {
        let from = format_day(start_day.checked_add(i64::try_from(index).ok()? * 14)?)?;
        let to = format_day((start_day + i64::try_from(index + 1).ok()? * 14).min(end_day))?;
        let mut part_data = Vec::new();
        while groups
            .peek()
            .is_some_and(|((group_index, _), _)| *group_index == index)
        {
            let ((_, capture_date), mut keys) = groups.next()?;
            keys.sort_unstable();
            if keys.is_empty()
                || keys.len() > 8192
                || keys.windows(2).any(|pair| pair[0] == pair[1])
            {
                return None;
            }
            let root = hash_strings(keys.iter().map(String::as_str))?;
            part_data.push((capture_date, keys.len(), root));
        }
        if !(1..=24).contains(&part_data.len()) {
            return None;
        }
        let count = part_data.iter().map(|(_, count, _)| *count).sum::<usize>();
        let root = hash_strings(part_data.iter().map(|(_, _, root)| root.as_str()))?;
        let part_count = part_data.len();
        let parts = part_data
            .into_iter()
            .enumerate()
            .map(|(part_index, (capture_date, count, root))| {
                object(vec![
                    ("captureLocalDate", string(&capture_date)),
                    ("expectedKeyCount", number(count)),
                    ("expectedKeySubsetSha256", string(&root)),
                    ("partCount", number(part_count)),
                    ("partIndex", number(part_index)),
                ])
            })
            .collect();
        chunks.push(object(vec![
            ("expectedKeyCount", number(count)),
            ("expectedKeySubsetSha256", string(&root)),
            ("fromLocalDate", string(&from)),
            ("logicalChunkIndex", number(index)),
            ("partCount", number(part_count)),
            ("parts", JsonValue::Array(parts)),
            ("toLocalDateExclusive", string(&to)),
        ]));
        logical_roots.push(root);
    }
    let reserved_key_sha256 = hash_strings(logical_roots.iter().map(String::as_str))?;
    let descriptor = object(vec![
        (
            "contractVersion",
            string("adjustment-registration-expected-key-plan/v3"),
        ),
        ("family", string(family.wire())),
        ("intervalEndAt", string(interval_end_at)),
        ("intervalStartAt", string(interval_start_at)),
        ("logicalChunkCount", number(chunk_count)),
        ("logicalChunks", JsonValue::Array(chunks)),
        ("reservedKeySha256", string(&reserved_key_sha256)),
    ]);
    let bytes = wire.encode_sorted_json(&descriptor)?;
    let expected_plan_sha256 = format_digest(wire.sha256(&bytes));
    Some(NativeMaintenanceExpectedKeyPlan {
        descriptor,
        bytes,
        expected_plan_sha256,
        reserved_key_sha256,
    })
}

/// Convert a source local calendar string to its timezone-independent civil-day index.
fn date_day(date: &str) -> Option<i64> {
    NativeWirePrimitives
        .canonical_utc_milliseconds(&format!("{date}T00:00:00.000Z"))
        .map(|ms| ms.div_euclid(DAY))
}

/// Format a civil-day index through the original UTC wire codec.
fn format_day(day: i64) -> Option<String> {
    let canonical = NativeWirePrimitives.format_utc_milliseconds(day.checked_mul(DAY)?)?;
    Some(canonical.get(..10)?.to_owned())
}

/// Hash exact canonical sorted source key arrays with the archive final LF.
fn hash_strings<'a>(values: impl Iterator<Item = &'a str>) -> Option<String> {
    let array = JsonValue::Array(values.map(string).collect());
    let wire = NativeWirePrimitives;
    Some(format_digest(
        wire.sha256(&wire.encode_sorted_json(&array)?),
    ))
}

/// Project the bounded source unsigned counts.
fn number(value: usize) -> JsonValue {
    JsonValue::Number(value as f64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Compare a complete rolling temperature plan with the actual predecessor scheduler.
    #[test]
    fn native_maintenance_expected_key_plan_matches_source() {
        let start = "2026-11-01T07:00:00.000Z";
        let end = "2027-11-02T07:00:00.000Z";
        let compiled = build_native_maintenance_expected_key_plan(
            NativeMaintenanceCaptureFamily::Temperature,
            start,
            end,
        )
        .expect("native expected plan");
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let output = Command::new("node")
            .args(["--input-type=module", "-e", "import {buildAdjustmentRegistrationExpectedKeyPlan as build} from './scripts/research/adjustment_maintenance_controller.mjs';process.stdout.write(build({family:'temperature',intervalStartAt:process.argv[1],intervalEndAt:process.argv[2]}).bytes)", start, end])
            .current_dir(repository).output().expect("predecessor scheduler");
        assert!(
            output.status.success(),
            "source scheduler: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(compiled.bytes, output.stdout);
    }
}
