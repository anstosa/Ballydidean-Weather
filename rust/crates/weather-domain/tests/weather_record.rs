use weather_contracts::{ForecastLeadBandKey, SourceKind};
use weather_domain::{
    DomainError, MetricName, Unit, forecast_lead_band_for, is_physical_measurement,
    normalize_metric_value, parse_source_kind, source_kind_label, validate_coordinates,
};

/// Extract a required canonical value for compact parity assertions.
fn normalized(metric: MetricName, value: f64, unit: Unit) -> f64 {
    normalize_metric_value(metric, Some(value), unit)
        .expect("fixture must normalize")
        .expect("fixture must remain present")
}

/// Mirror the TypeScript U-DOM-03 unit conversion oracle.
#[test]
fn converts_canonical_units() {
    assert_eq!(
        normalized(MetricName::TemperatureC, 32.0, Unit::Fahrenheit),
        0.0
    );
    assert_eq!(
        normalized(MetricName::PrecipitationMm, 1.0, Unit::Inch),
        25.4
    );
    assert_eq!(
        normalized(MetricName::WindSpeedMps, 36.0, Unit::KilometerPerHour),
        10.0,
    );
    assert_eq!(
        normalized(MetricName::WindSpeedMps, 10.0, Unit::MilePerHour),
        4.4704,
    );
    assert_eq!(
        normalized(MetricName::PressureHpa, 101_300.0, Unit::Pascal),
        1013.0
    );
    assert!(
        (normalized(MetricName::PressureHpa, 29.92, Unit::InchOfMercury) - 1_013.207_489_067_664)
            .abs()
            < 1e-9,
    );
    assert_eq!(
        normalized(MetricName::RelativeHumidityPercent, 50.0, Unit::Percent),
        50.0,
    );
    assert_eq!(
        normalized(
            MetricName::SoilElectricalConductivityMicrosiemensPerCm,
            450.0,
            Unit::MicrosiemensPerCentimeter,
        ),
        450.0,
    );
    assert_eq!(
        normalized(
            MetricName::Pm25MicrogramsPerCubicMeter,
            12.0,
            Unit::MicrogramPerCubicMeter,
        ),
        12.0,
    );
    assert_eq!(
        normalized(
            MetricName::PrecipitationRateMmPerHour,
            3.5,
            Unit::MillimeterPerHour,
        ),
        3.5,
    );
    assert_eq!(
        normalized(
            MetricName::PrecipitationRateMmPerHour,
            1.0,
            Unit::InchPerHour,
        ),
        25.4,
    );
}

/// Mirror the TypeScript U-DOM-03 range and unit rejections.
#[test]
fn rejects_invalid_canonical_metrics() {
    let humidity = normalize_metric_value(
        MetricName::RelativeHumidityPercent,
        Some(101.0),
        Unit::Percent,
    )
    .expect_err("humidity outside the contract must fail");
    assert!(matches!(humidity, DomainError::MetricOutOfRange { .. }));
    assert_eq!(
        humidity.to_string(),
        "relativeHumidityPercent must be between 0 and 100",
    );

    let direction =
        normalize_metric_value(MetricName::WindDirectionDegrees, Some(360.0), Unit::Degree)
            .expect_err("the direction maximum is exclusive");
    assert_eq!(
        direction.to_string(),
        "windDirectionDegrees must be less than 360"
    );

    let unsupported = normalize_metric_value(MetricName::TemperatureC, Some(4.0), Unit::Millimeter)
        .expect_err("unrelated units must fail");
    assert!(matches!(unsupported, DomainError::UnsupportedUnit { .. }));
}

/// Mirror the TypeScript U-DOM-04 null-versus-zero contract.
#[test]
fn preserves_missing_values_separately_from_zero() {
    assert_eq!(
        normalize_metric_value(MetricName::PrecipitationMm, None, Unit::Millimeter),
        Ok(None),
    );
    assert_eq!(
        normalize_metric_value(MetricName::PrecipitationMm, Some(0.0), Unit::Millimeter,),
        Ok(Some(0.0)),
    );
}

/// Mirror the TypeScript provenance-kind catalog and presentation semantics.
#[test]
fn uses_generated_source_kind_contracts() {
    // require every generated source-kind spelling to round trip
    for (wire_value, expected) in SourceKind::WIRE_VALUES {
        assert_eq!(parse_source_kind(wire_value), Ok(*expected));
    }
    assert_eq!(
        parse_source_kind("observation"),
        Err(DomainError::UnsupportedSourceKind("observation".to_owned())),
    );
    assert!(is_physical_measurement(SourceKind::PhysicalSensor));
    assert!(!is_physical_measurement(SourceKind::ModelCurrent));
    assert!(source_kind_label(SourceKind::ModelCurrent).contains("model-derived"));
    assert!(source_kind_label(SourceKind::Reanalysis).contains("reanalysis"));
}

/// Mirror the TypeScript coordinate-domain checks.
#[test]
fn validates_coordinates() {
    assert_eq!(validate_coordinates(47.95, -122.42), Ok((47.95, -122.42)));
    assert_eq!(
        validate_coordinates(91.0, 0.0),
        Err(DomainError::InvalidLatitude)
    );
    assert_eq!(
        validate_coordinates(0.0, -181.0),
        Err(DomainError::InvalidLongitude)
    );
}

/// Mirror the TypeScript U-MOS-13 exhaustive lead-band oracle.
#[test]
fn maps_every_eligible_forecast_lead() {
    // require every integer lead to map to one generated band
    for lead in 1..=168 {
        let band = forecast_lead_band_for(f64::from(lead)).expect("eligible lead must map");
        assert!(
            ForecastLeadBandKey::WIRE_VALUES
                .iter()
                .any(|(_wire_value, expected)| *expected == band)
        );
    }

    assert_eq!(
        forecast_lead_band_for(1.0),
        Ok(ForecastLeadBandKey::Value001024),
    );
    assert_eq!(
        forecast_lead_band_for(24.0),
        Ok(ForecastLeadBandKey::Value001024),
    );
    assert_eq!(
        forecast_lead_band_for(25.0),
        Ok(ForecastLeadBandKey::Value025048),
    );
    assert_eq!(
        forecast_lead_band_for(168.0),
        Ok(ForecastLeadBandKey::Value145168),
    );
    assert_eq!(
        forecast_lead_band_for(0.0),
        Err(DomainError::ForecastLeadHoursOutOfRange),
    );
    assert_eq!(
        forecast_lead_band_for(169.0),
        Err(DomainError::ForecastLeadHoursOutOfRange),
    );
    assert_eq!(
        forecast_lead_band_for(1.5),
        Err(DomainError::NonIntegerForecastLeadHours),
    );
}
