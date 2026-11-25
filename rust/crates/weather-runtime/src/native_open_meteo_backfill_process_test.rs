use weather_contracts::{
    BackfillChunkIdentity, BackfillChunkReport, BackfillChunkReportStatusEnum, BackfillReport,
    BackfillReportExitCodeEnum, Nullable,
};
use weather_worker::{
    ForecastAnchorBackfillDate, ForecastAnchorBackfillTimeAdapter, PublicStationBackfillTimeAdapter,
};

use super::{NativeOpenMeteoPreviousRunsTime, serialize_backfill_report};

/// The native output owner preserves the predecessor's report order and final line feed.
#[test]
fn serializes_exact_pretty_backfill_report() {
    let report = BackfillReport {
        chunks: vec![BackfillChunkReport {
            errorCode: Nullable::Null,
            identity: BackfillChunkIdentity {
                adapterVersion: "open-meteo-archive/v1".to_owned(),
                chunkPlanVersion: "open-meteo-archive-chunk-plan/v1".to_owned(),
                intervalEndExclusive: "2026-11-02T08:00:00.000Z".to_owned(),
                intervalStart: "2026-11-01T07:00:00.000Z".to_owned(),
                requestedFromDate: "2026-11-01".to_owned(),
                requestedToDate: "2026-11-01".to_owned(),
                sourceConfigFingerprint: "a".repeat(64),
                sourceId: "17".to_owned(),
            },
            secondaryError: Nullable::Value("release_failed".to_owned()),
            status: BackfillChunkReportStatusEnum::Completed,
        }],
        dryRun: false,
        exitCode: BackfillReportExitCodeEnum::Value0,
        from: "2026-11-01".to_owned(),
        resume: true,
        site: "ballydidean".to_owned(),
        source: "open-meteo-reanalysis-v1".to_owned(),
        to: "2026-11-01".to_owned(),
    };
    let bytes = serialize_backfill_report(&report).expect("serialize report");
    let text = String::from_utf8(bytes).expect("UTF-8 report");
    assert_eq!(
        text,
        concat!(
            "{\n",
            "  \"chunks\": [\n",
            "    {\n",
            "      \"errorCode\": null,\n",
            "      \"identity\": {\n",
            "        \"adapterVersion\": \"open-meteo-archive/v1\",\n",
            "        \"chunkPlanVersion\": \"open-meteo-archive-chunk-plan/v1\",\n",
            "        \"intervalEndExclusive\": \"2026-11-02T08:00:00.000Z\",\n",
            "        \"intervalStart\": \"2026-11-01T07:00:00.000Z\",\n",
            "        \"requestedFromDate\": \"2026-11-01\",\n",
            "        \"requestedToDate\": \"2026-11-01\",\n",
            "        \"sourceConfigFingerprint\": \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\n",
            "        \"sourceId\": \"17\"\n",
            "      },\n",
            "      \"secondaryError\": \"release_failed\",\n",
            "      \"status\": \"completed\"\n",
            "    }\n",
            "  ],\n",
            "  \"dryRun\": false,\n",
            "  \"exitCode\": 0,\n",
            "  \"from\": \"2026-11-01\",\n",
            "  \"resume\": true,\n",
            "  \"site\": \"ballydidean\",\n",
            "  \"source\": \"open-meteo-reanalysis-v1\",\n",
            "  \"to\": \"2026-11-01\"\n",
            "}\n",
        )
    );
}

/// Previous Runs delegates its UTC arithmetic to the already retained native calendar.
#[test]
fn previous_runs_time_reuses_public_backfill_calendar() {
    let date = ForecastAnchorBackfillDate::admitted("2024-02-29", 1_709_164_800_000);
    let mut previous = NativeOpenMeteoPreviousRunsTime::default();
    let next = ForecastAnchorBackfillTimeAdapter::add_utc_days(&mut previous, &date, 1)
        .expect("add leap-day date");
    assert_eq!(
        (next.label(), next.start_ms()),
        ("2024-03-01", 1_709_251_200_000)
    );

    let mut shared = crate::NativePublicBackfillTime::default();
    let shared_next = PublicStationBackfillTimeAdapter::add_utc_days(&mut shared, &date, 1)
        .expect("shared calendar date");
    assert_eq!(next, shared_next);
}
