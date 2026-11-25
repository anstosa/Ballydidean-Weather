//! Focused native bind tests for the historical write repositories.

use super::*;
use weather_database::Sha256Digest;

/// Preserve bigint run/source identity and smallint lead across a complete forecast anchor.
#[test]
fn native_postgres_backfill_anchor_write_uses_exact_column_types() {
    use ForecastAnchorRecordBind::*;
    let forecast_hash = Sha256Digest::admitted("a".repeat(64));
    let content_hash = Sha256Digest::admitted("b".repeat(64));
    let mut binds = vec![
        Text("9007199254740993".to_owned()),
        SourceKind(ForecastAnchorSourceKindBind::Forecast),
        Fingerprint(forecast_hash),
        UtcInstant("2026-11-23T12:00:00.000Z".to_owned()),
        LeadHours(24),
        Text("open-meteo".to_owned()),
        Text("ecmwf_ifs".to_owned()),
        Text("previous-runs".to_owned()),
        Text("adapter/v1".to_owned()),
        Text("9007199254740994".to_owned()),
        UtcInstant("2026-11-24T12:00:00.000Z".to_owned()),
        Text("raw".to_owned()),
        NullableJson(None),
        NullableJson(None),
    ];
    // append all nine original nullable measurement fields in source order
    for index in 0..9 {
        binds.push(NullableNumber((index == 0).then_some(10.0)));
    }
    binds.push(ContentHash(content_hash));
    let parameters = forecast_record_parameters(&binds).unwrap();
    assert_eq!(parameters.len(), 24);
    assert_eq!(parameters[0], Parameter::I64(9_007_199_254_740_993));
    assert_eq!(parameters[4], Parameter::I16(24));
    assert_eq!(parameters[9], Parameter::I64(9_007_199_254_740_994));
    assert_eq!(parameters[14], Parameter::F64(10.0));
    assert_eq!(
        parameters[15],
        Parameter::Null(crate::NativePostgresType::F64)
    );
    assert_eq!(parameters[23], Parameter::Text("b".repeat(64)));
}

/// Preserve backfill transaction plan timestamps, immutable source identity and JSON null.
#[test]
fn native_postgres_backfill_tide_start_uses_bigint_run_source() {
    use TideBackfillRunStartBind::*;
    let parameters = tide_run_start_parameters(&[
        Text("41".to_owned()),
        BackfillMode,
        UtcInstant("2026-11-01T00:00:00.000Z".to_owned()),
        UtcInstant("2026-11-02T00:00:00.000Z".to_owned()),
        Fingerprint(Sha256Digest::admitted("a".repeat(64))),
        Text("noaa/v1".to_owned()),
        Text("tide-chunk/v1".to_owned()),
        UtcInstant("2026-11-24T00:00:00.000Z".to_owned()),
        Count(0),
        NullableJson(None),
    ])
    .unwrap();
    assert_eq!(parameters[0], Parameter::I64(41));
    assert_eq!(parameters[1], Parameter::Text("backfill".to_owned()));
    assert_eq!(
        parameters[2],
        Parameter::Timestamp("2026-11-01T00:00:00.000Z".to_owned())
    );
    assert_eq!(parameters[8], Parameter::I32(0));
    assert_eq!(
        parameters[9],
        Parameter::Null(crate::NativePostgresType::Json)
    );
}

/// Bind Tide and alias-backed Ambient chunk dates to PostgreSQL DATE, not text.
#[test]
fn native_postgres_tide_and_ambient_outcomes_use_typed_dates() {
    use TideBackfillOutcomeBind::*;
    let parameters = tide_outcome_parameters(&[
        Text("41".to_owned()),
        UtcInstant("2026-11-01T00:00:00.000Z".to_owned()),
        UtcInstant("2026-11-02T00:00:00.000Z".to_owned()),
        Fingerprint(Sha256Digest::admitted("a".repeat(64))),
        Text("ambient-weather/v1".to_owned()),
        Text("public-station-chunk/v1".to_owned()),
        Text("2026-11-01".to_owned()),
        Text("2026-11-01".to_owned()),
        Text("43".to_owned()),
        Outcome(TideBackfillOutcome::Succeeded),
        NullableText(None),
    ])
    .expect("admit shared Tide/Ambient outcome binds");

    assert_eq!(parameters[6], Parameter::Date("2026-11-01".to_owned()));
    assert_eq!(parameters[7], Parameter::Date("2026-11-01".to_owned()));
    assert_eq!(parameters[8], Parameter::I64(43));
}

/// Bind forecast-anchor requested range dates through the same typed DATE adapter.
#[test]
fn native_postgres_forecast_outcome_uses_typed_dates() {
    use ForecastAnchorOutcomeBind::*;
    let parameters = forecast_outcome_parameters(&[
        Text("41".to_owned()),
        UtcInstant("2026-11-01T00:00:00.000Z".to_owned()),
        UtcInstant("2026-11-02T00:00:00.000Z".to_owned()),
        Fingerprint(Sha256Digest::admitted("a".repeat(64))),
        Text("open-meteo/v1".to_owned()),
        Text("forecast-anchor-chunk/v1".to_owned()),
        Text("2026-11-01".to_owned()),
        Text("2026-11-01".to_owned()),
        Text("44".to_owned()),
        Outcome(ForecastAnchorOutcome::Succeeded),
        NullableText(None),
    ])
    .expect("admit forecast outcome binds");

    assert_eq!(parameters[6], Parameter::Date("2026-11-01".to_owned()));
    assert_eq!(parameters[7], Parameter::Date("2026-11-01".to_owned()));
    assert_eq!(parameters[8], Parameter::I64(44));
}
