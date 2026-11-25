//! Ordered machine-report and process output ownership for historical backfill commands.

use crate::{HistoricalBackfillOutputPort, HistoricalBackfillReportFileMode, NativeWirePrimitives};
use serde::{Serialize, Serializer, ser::SerializeStruct};
use std::{fs::OpenOptions, io::Write, os::unix::fs::OpenOptionsExt};
use weather_edge::ArchiveWirePrimitives;
use weather_worker::{
    HistoricalBackfillReport, PublicStationBackfillSourceReport, TaskFailure,
    TempestBackfillChunkIdentity, TempestBackfillChunkReport, TempestBackfillChunkStatus,
    TempestStationBackfillReport, TideBackfillSourceReport,
};

/// Write the admitted historical report to the real requested file and stdout boundary.
pub struct NativeHistoricalBackfillOutput<W>(W);

impl<W: Write> NativeHistoricalBackfillOutput<W> {
    /// Retain the process output writer without opening any report files.
    pub const fn new(output: W) -> Self {
        Self(output)
    }

    /// Recover the output sink for direct process verification.
    pub fn into_inner(self) -> W {
        self.0
    }
}

impl<W: Write> HistoricalBackfillOutputPort for NativeHistoricalBackfillOutput<W> {
    /// Preserve the predecessor's two-space pretty report and terminating LF.
    fn serialize_report(
        &mut self,
        report: &HistoricalBackfillReport,
    ) -> Result<Vec<u8>, TaskFailure> {
        let mut bytes = serde_json::to_vec_pretty(&Report(report))
            .map_err(|_| failure("historical_backfill_report_encoding_failed"))?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Match the source-specific mode only on new report-file creation.
    fn write_report_file(
        &mut self,
        path: &str,
        contents: &[u8],
        mode: HistoricalBackfillReportFileMode,
    ) -> Result<(), TaskFailure> {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        // Tempest and public-station reports request owner-only creation mode
        if mode == HistoricalBackfillReportFileMode::OwnerReadWrite {
            options.mode(0o600);
        }
        options
            .open(path)
            .and_then(|mut file| file.write_all(contents))
            .map_err(|_| failure("historical_backfill_report_write_failed"))
    }

    /// Write the exact same serialized bytes after any requested report file.
    fn write_stdout(&mut self, contents: &[u8]) -> Result<(), TaskFailure> {
        self.0
            .write_all(contents)
            .map_err(|_| failure("historical_backfill_stdout_failed"))
    }
}

/// Retain a categorical output failure without embedding filesystem paths.
fn failure(code: &str) -> TaskFailure {
    TaskFailure {
        code: code.to_owned(),
        attempts: 0,
    }
}

struct Report<'a>(&'a HistoricalBackfillReport);

impl Serialize for Report<'_> {
    /// Emit the source's family-specific report fields in their original insertion order.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("HistoricalBackfillReport", 5)?;
        // dispatch only the source report's existing family
        match self.0 {
            HistoricalBackfillReport::Tempest(report) => {
                out.serialize_field("dryRun", &report.dry_run)?;
                out.serialize_field("exitCode", &report.exit_code)?;
                out.serialize_field("resume", &report.resume)?;
                out.serialize_field("site", &report.site)?;
                out.serialize_field(
                    "stations",
                    &report.stations.iter().map(Station).collect::<Vec<_>>(),
                )?;
            }
            HistoricalBackfillReport::Tide(report) => {
                out.serialize_field("dryRun", &report.dry_run)?;
                out.serialize_field("exitCode", &report.exit_code)?;
                out.serialize_field("resume", &report.resume)?;
                out.serialize_field("site", &report.site)?;
                out.serialize_field(
                    "sources",
                    &report.sources.iter().map(Source::from).collect::<Vec<_>>(),
                )?;
            }
            HistoricalBackfillReport::PublicStations(report) => {
                out.serialize_field("dryRun", &report.dry_run)?;
                out.serialize_field("exitCode", &report.exit_code)?;
                out.serialize_field("resume", &report.resume)?;
                out.serialize_field("site", &report.site)?;
                out.serialize_field(
                    "sources",
                    &report.sources.iter().map(Source::from).collect::<Vec<_>>(),
                )?;
            }
        }
        out.end()
    }
}

struct Station<'a>(&'a TempestStationBackfillReport);

impl Serialize for Station<'_> {
    /// Preserve the exact station summary field order.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let station = self.0;
        let mut out = serializer.serialize_struct("TempestStationBackfillReport", 6)?;
        out.serialize_field(
            "chunks",
            &station.chunks.iter().map(Chunk).collect::<Vec<_>>(),
        )?;
        out.serialize_field("exitCode", &station.exit_code)?;
        out.serialize_field("from", &station.from)?;
        out.serialize_field("locationId", &station.location_id)?;
        out.serialize_field("source", &station.source)?;
        out.serialize_field("to", &station.to)?;
        out.end()
    }
}

struct Chunk<'a>(&'a TempestBackfillChunkReport);

impl Serialize for Chunk<'_> {
    /// Preserve the report's exact identity and bounded secondary diagnostic field.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let chunk = self.0;
        let mut out = serializer.serialize_struct("TempestBackfillChunkReport", 5)?;
        out.serialize_field("errorCode", &chunk.error_code)?;
        out.serialize_field("identity", &Identity(&chunk.identity))?;
        out.serialize_field("recordCount", &chunk.record_count)?;
        out.serialize_field(
            "secondaryError",
            &secondary_error(&chunk.secondary_error_codes),
        )?;
        let state = match chunk.status {
            TempestBackfillChunkStatus::Abandoned => "abandoned",
            TempestBackfillChunkStatus::Completed => "completed",
            TempestBackfillChunkStatus::Failed => "failed",
            TempestBackfillChunkStatus::Planned => "planned",
            TempestBackfillChunkStatus::Skipped => "skipped",
        };
        out.serialize_field("status", state)?;
        out.end()
    }
}

/// Render the original secondary error envelope from the retained bounded error codes.
fn secondary_error(codes: &[String]) -> Option<String> {
    // retain null when no cleanup operation failed
    if codes.is_empty() {
        return None;
    }
    // the predecessor emits a lone finalization diagnostic without a category prefix
    if let [code] = codes
        && let Some(value) = code.strip_prefix("finalization:")
    {
        return Some(value.to_owned());
    }
    Some(
        codes
            .iter()
            .map(|code| {
                code.split_once(':').map_or_else(
                    || code.clone(),
                    |(category, value)| format!("{category}={value}"),
                )
            })
            .collect::<Vec<_>>()
            .join("; "),
    )
}

struct Identity<'a>(&'a TempestBackfillChunkIdentity);

impl Serialize for Identity<'_> {
    /// Emit canonical UTC half-open chunk bounds with their original member ordering.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::Error as _;
        let identity = self.0;
        let start = NativeWirePrimitives
            .format_utc_milliseconds(identity.interval_start.start_ms())
            .ok_or_else(|| S::Error::custom("invalid historical chunk start"))?;
        let end = NativeWirePrimitives
            .format_utc_milliseconds(identity.interval_end_exclusive.start_ms())
            .ok_or_else(|| S::Error::custom("invalid historical chunk end"))?;
        let mut out = serializer.serialize_struct("BackfillChunkIdentity", 8)?;
        out.serialize_field("adapterVersion", identity.adapter_version)?;
        out.serialize_field("chunkPlanVersion", identity.chunk_plan_version)?;
        out.serialize_field("intervalEndExclusive", &end)?;
        out.serialize_field("intervalStart", &start)?;
        out.serialize_field("requestedFromDate", &identity.requested_from_date)?;
        out.serialize_field("requestedToDate", &identity.requested_to_date)?;
        out.serialize_field(
            "sourceConfigFingerprint",
            &identity.source_config_fingerprint,
        )?;
        out.serialize_field("sourceId", &identity.source_id)?;
        out.end()
    }
}

struct Failed<'a>(&'a str, &'a str);

impl Serialize for Failed<'_> {
    /// Emit the source's failed chunk diagnostic.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("FailedChunk", 2)?;
        out.serialize_field("errorCode", self.0)?;
        out.serialize_field("startDate", self.1)?;
        out.end()
    }
}

struct Source<'a> {
    completed_chunks: usize,
    exit_code: u8,
    failed_chunk: Option<Failed<'a>>,
    from: &'a str,
    planned_chunks: usize,
    records: usize,
    skipped_chunks: usize,
    source: &'a str,
    to: &'a str,
}

impl<'a> From<&'a PublicStationBackfillSourceReport> for Source<'a> {
    /// Borrow one public-station report without cloning its source fields.
    fn from(report: &'a PublicStationBackfillSourceReport) -> Self {
        Self {
            completed_chunks: report.completed_chunks,
            exit_code: report.exit_code,
            failed_chunk: report
                .failed_chunk
                .as_ref()
                .map(|failed| Failed(&failed.error_code, &failed.start_date)),
            from: &report.from,
            planned_chunks: report.planned_chunks,
            records: report.records,
            skipped_chunks: report.skipped_chunks,
            source: &report.source,
            to: &report.to,
        }
    }
}

impl<'a> From<&'a TideBackfillSourceReport> for Source<'a> {
    /// Borrow one tide report without cloning its source fields.
    fn from(report: &'a TideBackfillSourceReport) -> Self {
        Self {
            completed_chunks: report.completed_chunks,
            exit_code: report.exit_code,
            failed_chunk: report
                .failed_chunk
                .as_ref()
                .map(|failed| Failed(&failed.error_code, &failed.start_date)),
            from: &report.from,
            planned_chunks: report.planned_chunks,
            records: report.records,
            skipped_chunks: report.skipped_chunks,
            source: &report.source,
            to: &report.to,
        }
    }
}

impl Serialize for Source<'_> {
    /// Preserve the common public-station and tide source summary.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = serializer.serialize_struct("SourceReport", 9)?;
        out.serialize_field("completedChunks", &self.completed_chunks)?;
        out.serialize_field("exitCode", &self.exit_code)?;
        out.serialize_field("failedChunk", &self.failed_chunk)?;
        out.serialize_field("from", self.from)?;
        out.serialize_field("plannedChunks", &self.planned_chunks)?;
        out.serialize_field("records", &self.records)?;
        out.serialize_field("skippedChunks", &self.skipped_chunks)?;
        out.serialize_field("source", self.source)?;
        out.serialize_field("to", self.to)?;
        out.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_worker::{TideBackfillReport, TideBackfillSourceReport};

    /// Exercise the real pretty report and stdout byte owner without external data.
    #[test]
    fn native_historical_backfill_output_preserves_source_order() {
        let report = HistoricalBackfillReport::Tide(TideBackfillReport {
            dry_run: true,
            exit_code: 0,
            resume: false,
            site: "fixture".to_owned(),
            sources: vec![TideBackfillSourceReport {
                completed_chunks: 0,
                exit_code: 0,
                failed_chunk: None,
                from: "2026-11-01".to_owned(),
                planned_chunks: 1,
                records: 0,
                skipped_chunks: 0,
                source: "fixture-tide".to_owned(),
                to: "2026-11-01".to_owned(),
            }],
        });
        let mut output = NativeHistoricalBackfillOutput::new(Vec::new());
        let bytes = output.serialize_report(&report).unwrap();
        let expected = concat!(
            "{\n  \"dryRun\": true,\n  \"exitCode\": 0,\n  \"resume\": false,\n  \"site\": \"fixture\",\n",
            "  \"sources\": [\n    {\n      \"completedChunks\": 0,\n      \"exitCode\": 0,\n      \"failedChunk\": null,\n",
            "      \"from\": \"2026-11-01\",\n      \"plannedChunks\": 1,\n      \"records\": 0,\n      \"skippedChunks\": 0,\n",
            "      \"source\": \"fixture-tide\",\n      \"to\": \"2026-11-01\"\n    }\n  ]\n}\n"
        );
        assert_eq!(bytes, expected.as_bytes());
        output.write_stdout(&bytes).unwrap();
        assert_eq!(output.into_inner(), bytes);
    }
}
