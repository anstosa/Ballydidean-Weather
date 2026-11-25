//! Build the original temperature native-source revision document before serialization.

use std::collections::BTreeMap;

use weather_api::{CanonicalInstant, CurrentTimeResolver};
use weather_contracts::{
    AdjustmentRevisionProjectionLogicalKeyValue, AdjustmentRevisionProjectionRowsValue,
    AdjustmentTemperatureNativeSourceProjection,
    AdjustmentTemperatureNativeSourceProjectionContractVersionLiteral,
    AdjustmentTemperatureNativeSourceProjectionFamilyLiteral,
    AdjustmentTemperatureNativeSourceProjectionProjectionKindLiteral,
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanaryRunAdapterVersionLiteral,
    EcmwfTemperatureCanaryRunModelCycleEnum, EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    JsDateOrString, JsonValue, MaintenanceShadowTemperatureRecentErrorState,
    MaintenanceShadowTemperatureRecentErrorStateCohortLiteral, Nullable,
    PersistedEcmwfTemperatureRevision, WeatherRecordRow,
};
use weather_database::Sha256Digest;

type LogicalValue = AdjustmentRevisionProjectionLogicalKeyValue;
type RowValue = AdjustmentRevisionProjectionRowsValue;

const HOUR_COUNT: usize = 18;
const COMPARATOR_COUNT: usize = 12;
const HOUR_MILLISECONDS: i64 = 3_600_000;

/// One bounded material error without database or private archive detail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EcmwfRevisionMaterialError {
    Clock,
    Digest,
    Geometry,
    Lineage,
    Number,
}

/// Approved decimal and digest owner used only for the source fingerprint.
pub trait EcmwfRevisionMaterialDigest {
    /// Render the source's existing JavaScript `toFixed(6)` coordinate spelling.
    fn fixed_six(&mut self, value: f64) -> Option<String>;

    /// Hash one exact predecessor-owned byte preimage.
    fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest>;
}

/// Build one typed, stage-ready native temperature projection.
///
/// The existing ordered revision codec remains responsible for serializing this generated type
/// and binding the recent-error-state digest before private staging.
pub fn prepare_ecmwf_temperature_revision_material(
    revision: &PersistedEcmwfTemperatureRevision,
    capture_epoch: &CanonicalInstant,
    time: &dyn CurrentTimeResolver,
    digest: &mut impl EcmwfRevisionMaterialDigest,
) -> Result<AdjustmentTemperatureNativeSourceProjection, EcmwfRevisionMaterialError> {
    // preserve the predecessor's complete source and comparator geometry
    if revision.hours.len() != HOUR_COUNT || revision.bestMatchRows.len() != COMPARATOR_COUNT {
        return Err(EcmwfRevisionMaterialError::Geometry);
    }
    let logical_received_at = wire_clock(&revision.logicalReceivedAt, time)?;
    let run_initialized_at = wire_clock(&revision.runInitializedAt, time)?;
    // bind both actual provider clocks to the verified capture epoch
    if logical_received_at.epoch_milliseconds < capture_epoch.epoch_milliseconds
        || run_initialized_at.epoch_milliseconds < capture_epoch.epoch_milliseconds
    {
        return Err(EcmwfRevisionMaterialError::Clock);
    }
    require_hour_geometry(revision, &run_initialized_at, time)?;
    let comparator_by_valid = index_comparators(&revision.bestMatchRows, time)?;
    require_one_comparator_lineage(&revision.bestMatchRows, time)?;
    // project every retained hour in its already validated source order
    let rows = revision
        .hours
        .iter()
        .map(|hour| {
            let valid_at = wire_clock(&hour.validAt, time)?;
            // leave the first six ineligible leads explicitly comparator-free
            let comparator = if hour.modelLeadHours < 7 {
                None
            } else {
                comparator_by_valid.get(&valid_at.utc).copied()
            };
            // require every eligible native hour to retain an exact best-match row
            if hour.modelLeadHours >= 7 && comparator.is_none() {
                return Err(EcmwfRevisionMaterialError::Geometry);
            }
            build_row(hour, revision.modelCycle, comparator, capture_epoch, time)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let fingerprint = source_fingerprint(revision, digest)?;
    Ok(AdjustmentTemperatureNativeSourceProjection {
        contractVersion: AdjustmentTemperatureNativeSourceProjectionContractVersionLiteral,
        family: AdjustmentTemperatureNativeSourceProjectionFamilyLiteral,
        logicalKey: logical_key(revision),
        logicalReceivedAt: logical_received_at.utc,
        projectionKind: AdjustmentTemperatureNativeSourceProjectionProjectionKindLiteral,
        recentErrorState: recent_error_state(&revision.recentErrorState),
        recentErrorStateSha256: revision.recentErrorStateSha256.clone(),
        rows,
        source: source(revision, fingerprint.as_str()),
        storedContentSha256: revision.contentHash.clone(),
    })
}

/// Require the persisted run's complete ordered one-based native horizon.
fn require_hour_geometry(
    revision: &PersistedEcmwfTemperatureRevision,
    initialized: &CanonicalInstant,
    time: &dyn CurrentTimeResolver,
) -> Result<(), EcmwfRevisionMaterialError> {
    // bind each retained row to run initialization plus its exact one-based lead
    for (index, hour) in revision.hours.iter().enumerate() {
        let lead = (index + 1) as u64;
        let valid = wire_clock(&hour.validAt, time)?;
        // reject a skipped, repeated, or time-shifted native lead
        if hour.modelLeadHours != lead
            || valid.epoch_milliseconds
                != initialized.epoch_milliseconds + lead as i64 * HOUR_MILLISECONDS
        {
            return Err(EcmwfRevisionMaterialError::Geometry);
        }
    }
    Ok(())
}

/// Build the exact source-fingerprint preimage for an approved decimal owner.
pub fn ecmwf_revision_source_fingerprint_preimage(
    revision: &PersistedEcmwfTemperatureRevision,
    digest: &mut impl EcmwfRevisionMaterialDigest,
) -> Result<Vec<u8>, EcmwfRevisionMaterialError> {
    let latitude = digest
        .fixed_six(revision.siteLatitude)
        .ok_or(EcmwfRevisionMaterialError::Number)?;
    let longitude = digest
        .fixed_six(revision.siteLongitude)
        .ok_or(EcmwfRevisionMaterialError::Number)?;
    let mut preimage = [
        EcmwfTemperatureCanaryRunAdapterVersionLiteral::WIRE_VALUE,
        revision.siteSlug.as_str(),
        latitude.as_str(),
        longitude.as_str(),
        EcmwfTemperatureCanaryRunUpstreamModelLiteral::WIRE_VALUE,
        "forecast_hours=19",
        "temperature_2m,relative_humidity_2m,wind_speed_10m",
    ]
    .join("\n")
    .into_bytes();
    preimage.push(b'\n');
    Ok(preimage)
}

/// Hash the exact source-fingerprint material through the injected owner.
fn source_fingerprint(
    revision: &PersistedEcmwfTemperatureRevision,
    digest: &mut impl EcmwfRevisionMaterialDigest,
) -> Result<Sha256Digest, EcmwfRevisionMaterialError> {
    let preimage = ecmwf_revision_source_fingerprint_preimage(revision, digest)?;
    let fingerprint = digest
        .sha256(&preimage)
        .ok_or(EcmwfRevisionMaterialError::Digest)?;
    // reject a digest adapter that returned a noncanonical address
    if !valid_digest(fingerprint.as_str()) {
        return Err(EcmwfRevisionMaterialError::Digest);
    }
    Ok(fingerprint)
}

/// Index comparators by canonical valid time with JavaScript `Map` replacement semantics.
fn index_comparators<'a>(
    rows: &'a [WeatherRecordRow],
    time: &dyn CurrentTimeResolver,
) -> Result<BTreeMap<String, &'a WeatherRecordRow>, EcmwfRevisionMaterialError> {
    let mut indexed = BTreeMap::new();
    // later duplicate valid clocks replace earlier rows
    for row in rows {
        indexed.insert(repository_clock(&row.validAt, time)?.utc, row);
    }
    Ok(indexed)
}

/// Require the one source and product-run lineage asserted before row projection.
fn require_one_comparator_lineage(
    rows: &[WeatherRecordRow],
    time: &dyn CurrentTimeResolver,
) -> Result<(), EcmwfRevisionMaterialError> {
    let first = rows.first().ok_or(EcmwfRevisionMaterialError::Geometry)?;
    let first_product = comparator_product_run(first, time)?;
    // retain one exact source and product-run identity for all twelve rows
    for row in &rows[1..] {
        // reject any source or initialization drift within the comparator horizon
        if row.sourceId != first.sourceId
            || comparator_product_run(row, time)?.utc != first_product.utc
        {
            return Err(EcmwfRevisionMaterialError::Geometry);
        }
    }
    Ok(())
}

/// Build one ordered original row without encoding JSON.
fn build_row(
    hour: &weather_contracts::EcmwfTemperatureCanaryHour,
    model_cycle: EcmwfTemperatureCanaryRunModelCycleEnum,
    comparator: Option<&WeatherRecordRow>,
    capture_epoch: &CanonicalInstant,
    time: &dyn CurrentTimeResolver,
) -> Result<Vec<(Vec<u16>, RowValue)>, EcmwfRevisionMaterialError> {
    let (content, product_run, source_id, temperature) = match comparator {
        Some(comparator) => {
            require_best_match(comparator)?;
            let product_run = comparator_product_run(comparator, time)?;
            // retain every source-decision product clock in capture-epoch admission
            if product_run.epoch_milliseconds < capture_epoch.epoch_milliseconds {
                return Err(EcmwfRevisionMaterialError::Clock);
            }
            (
                text_value(&comparator.contentHash),
                text_value(&product_run.utc),
                text_value(&comparator.sourceId),
                nullable_binary64(&comparator.temperatureC)?,
            )
        }
        None => (
            RowValue::Null,
            RowValue::Null,
            RowValue::Null,
            RowValue::Null,
        ),
    };
    Ok(vec![
        field("bestMatchContentSha256", content),
        field("bestMatchProductRunAt", product_run),
        field("bestMatchSourceId", source_id),
        field("bestMatchTemperatureC64", temperature),
        field("contentSha256", text_value(&hour.contentHash)),
        field(
            "modelCycle",
            text_value(model_cycle_wire_value(model_cycle)),
        ),
        field(
            "modelLeadHours",
            RowValue::Number(hour.modelLeadHours as f64),
        ),
        field(
            "rawRelativeHumidityPercent64",
            nullable_binary64(&hour.rawRelativeHumidityPercent)?,
        ),
        field("rawTemperatureC64", finite_binary64(hour.rawTemperatureC)?),
        field(
            "rawWindSpeedMps64",
            nullable_binary64(&hour.rawWindSpeedMps)?,
        ),
        field("validAt", text_value(&wire_clock(&hour.validAt, time)?.utc)),
    ])
}

/// Require the exact best-match source fields consumed by the predecessor.
fn require_best_match(row: &WeatherRecordRow) -> Result<(), EcmwfRevisionMaterialError> {
    let dataset = object_text(&row.providerMetadata, "dataset");
    let upstream = nullable_text(&row.upstreamModel);
    // prohibit a missing or differently identified serving comparator
    if dataset.as_deref() != Some("best_match") || upstream.as_deref() != Some("best_match") {
        return Err(EcmwfRevisionMaterialError::Lineage);
    }
    Ok(())
}

/// Build the frozen logical-key order used by the revision codec.
fn logical_key(revision: &PersistedEcmwfTemperatureRevision) -> Vec<(Vec<u16>, LogicalValue)> {
    vec![
        logical_field("contentSha256", logical_text(&revision.contentHash)),
        logical_field("leadHours", LogicalValue::Null),
        logical_field(
            "providerResponseSha256",
            logical_text(&revision.providerResponseSha256),
        ),
        logical_field("runInitializedAt", logical_text(&revision.runInitializedAt)),
        logical_field("siteId", logical_text(&revision.siteId)),
        logical_field("sourceId", LogicalValue::Null),
        logical_field("sourceType", logical_text("ecmwf_temperature_run")),
        logical_field("validAt", LogicalValue::Null),
    ]
}

/// Build the frozen source-lineage order used by the revision codec.
fn source(
    revision: &PersistedEcmwfTemperatureRevision,
    fingerprint: &str,
) -> Vec<(Vec<u16>, Nullable<Vec<u16>>)> {
    vec![
        source_field(
            "adapterVersion",
            EcmwfTemperatureCanaryRunAdapterVersionLiteral::WIRE_VALUE,
        ),
        source_field("contractEpoch", "ecmwf-temperature-canary/v1"),
        source_field("dataset", "single_run"),
        source_field("providerKey", "open-meteo"),
        source_field("sourceConfigFingerprint", fingerprint),
        source_field("sourceId", &format!("site:{}", revision.siteId)),
        source_field(
            "sourceKey",
            &format!("open-meteo-ecmwf-single-run:{}", revision.siteSlug),
        ),
        source_field("sourceKind", "forecast"),
        source_field(
            "upstreamModel",
            EcmwfTemperatureCanaryRunUpstreamModelLiteral::WIRE_VALUE,
        ),
    ]
}

/// Retype the original recent state into its revision-document projection.
fn recent_error_state(
    state: &EcmwfTemperatureCanaryRecentErrorState,
) -> MaintenanceShadowTemperatureRecentErrorState {
    MaintenanceShadowTemperatureRecentErrorState {
        b24C: state.b24C.clone(),
        b72C: state.b72C.clone(),
        cohort: MaintenanceShadowTemperatureRecentErrorStateCohortLiteral,
        localDates: state.localDates,
        mad72C: state.mad72C.clone(),
        maximumSourceRunInitializedAt: state.maximumSourceRunInitializedAt.clone(),
        maximumSourceValidAt: state.maximumSourceValidAt.clone(),
        n24: state.n24,
        n72: state.n72,
        sourceKeys: state.sourceKeys.clone(),
        supported: state.supported,
        targetRunInitializedAt: state.targetRunInitializedAt.clone(),
        windowEndValidAt: state.windowEndValidAt.clone(),
    }
}

/// Resolve one wire UTC string through the existing time owner.
fn wire_clock(
    value: &str,
    time: &dyn CurrentTimeResolver,
) -> Result<CanonicalInstant, EcmwfRevisionMaterialError> {
    time.resolve(&JsDateOrString::String(value.encode_utf16().collect()))
        .map_err(|_| EcmwfRevisionMaterialError::Clock)
}

/// Resolve one repository date-or-string through the existing time owner.
fn repository_clock(
    value: &JsDateOrString,
    time: &dyn CurrentTimeResolver,
) -> Result<CanonicalInstant, EcmwfRevisionMaterialError> {
    time.resolve(value)
        .map_err(|_| EcmwfRevisionMaterialError::Clock)
}

/// Resolve one required comparator product clock.
fn comparator_product_run(
    row: &WeatherRecordRow,
    time: &dyn CurrentTimeResolver,
) -> Result<CanonicalInstant, EcmwfRevisionMaterialError> {
    let Nullable::Value(value) = &row.productRunAt else {
        return Err(EcmwfRevisionMaterialError::Lineage);
    };
    repository_clock(value, time)
}

/// Encode one exact finite binary64 value as network-order hexadecimal.
fn finite_binary64(value: f64) -> Result<RowValue, EcmwfRevisionMaterialError> {
    value
        .is_finite()
        .then(|| RowValue::String(format!("{:016x}", value.to_bits()).encode_utf16().collect()))
        .ok_or(EcmwfRevisionMaterialError::Number)
}

/// Preserve null separately from every finite binary64 bit pattern.
fn nullable_binary64(value: &Nullable<f64>) -> Result<RowValue, EcmwfRevisionMaterialError> {
    match value {
        Nullable::Null => Ok(RowValue::Null),
        Nullable::Value(value) => finite_binary64(*value),
    }
}

/// Read one nullable UTF-16 generated string.
fn nullable_text(value: &Nullable<String>) -> Option<String> {
    match value {
        Nullable::Null => None,
        Nullable::Value(value) => Some(value.clone()),
    }
}

/// Read one text member from an ordered generated JSON object.
fn object_text(value: &Nullable<Vec<(Vec<u16>, JsonValue)>>, key: &str) -> Option<String> {
    let Nullable::Value(fields) = value else {
        return None;
    };
    fields.iter().find_map(|(name, value)| {
        // retain the exact UTF-16 key without normalization
        if !name.iter().copied().eq(key.encode_utf16()) {
            return None;
        }
        let JsonValue::String(value) = value else {
            return None;
        };
        String::from_utf16(value).ok()
    })
}

/// Return the model-cycle wire value without aliases.
fn model_cycle_wire_value(cycle: EcmwfTemperatureCanaryRunModelCycleEnum) -> &'static str {
    match cycle {
        EcmwfTemperatureCanaryRunModelCycleEnum::Value49r1 => "49r1",
        EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1 => "50r1",
    }
}

/// Recognize one canonical lowercase SHA-256 address.
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Build one UTF-16 row field.
fn field(name: &str, value: RowValue) -> (Vec<u16>, RowValue) {
    (name.encode_utf16().collect(), value)
}

/// Build one UTF-16 logical field.
fn logical_field(name: &str, value: LogicalValue) -> (Vec<u16>, LogicalValue) {
    (name.encode_utf16().collect(), value)
}

/// Build one UTF-16 source field.
fn source_field(name: &str, value: &str) -> (Vec<u16>, Nullable<Vec<u16>>) {
    (
        name.encode_utf16().collect(),
        Nullable::Value(value.encode_utf16().collect()),
    )
}

/// Build one string row value.
fn text_value(value: &str) -> RowValue {
    RowValue::String(value.encode_utf16().collect())
}

/// Build one string logical-key value.
fn logical_text(value: &str) -> LogicalValue {
    LogicalValue::String(value.encode_utf16().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use weather_api::CurrentProjectionError;
    use weather_contracts::{
        EcmwfTemperatureCanaryHour, EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
        JsDateOrString, SourceKind,
    };

    const BASE: i64 = 1_791_417_600_000;

    struct FixtureTime;

    impl CurrentTimeResolver for FixtureTime {
        /// Resolve only the fixed October fixture clocks.
        fn resolve(
            &self,
            value: &JsDateOrString,
        ) -> Result<CanonicalInstant, CurrentProjectionError> {
            let text = match value {
                JsDateOrString::String(value) => String::from_utf16(value).map_err(|_| error())?,
                JsDateOrString::DateEpochMilliseconds(epoch_milliseconds) => {
                    return Ok(CanonicalInstant {
                        epoch_milliseconds: *epoch_milliseconds,
                        utc: format!("fixture:{epoch_milliseconds}"),
                    });
                }
                JsDateOrString::InvalidDate => return Err(error()),
            };
            // admit only the current and immediately prior fixture dates
            let day_offset = if text.starts_with("2026-10-08T") {
                0
            } else if text.starts_with("2026-10-07T") {
                -86_400_000
            } else {
                return Err(error());
            };
            let hour = text
                .get(11..13)
                .and_then(|value| value.parse::<i64>().ok())
                .ok_or_else(error)?;
            let minute = text
                .get(14..16)
                .and_then(|value| value.parse::<i64>().ok())
                .ok_or_else(error)?;
            Ok(CanonicalInstant {
                epoch_milliseconds: BASE + day_offset + hour * 3_600_000 + minute * 60_000,
                utc: text,
            })
        }
    }

    #[derive(Default)]
    struct FixtureDigest {
        preimage: Vec<u8>,
    }

    impl EcmwfRevisionMaterialDigest for FixtureDigest {
        /// Render the fixture coordinates like the existing JavaScript owner.
        fn fixed_six(&mut self, value: f64) -> Option<String> {
            value.is_finite().then(|| format!("{value:.6}"))
        }

        /// Retain the exact requested preimage and return one admitted fixture address.
        fn sha256(&mut self, bytes: &[u8]) -> Option<Sha256Digest> {
            self.preimage = bytes.to_vec();
            Some(Sha256Digest::admitted("a".repeat(64)))
        }
    }

    #[test]
    fn builds_all_eighteen_rows_with_twelve_eligible_comparators() {
        let revision = revision();
        let mut digest = FixtureDigest::default();
        let projection = prepare_ecmwf_temperature_revision_material(
            &revision,
            &CanonicalInstant {
                epoch_milliseconds: BASE,
                utc: "2026-10-08T00:00:00.000Z".to_owned(),
            },
            &FixtureTime,
            &mut digest,
        )
        .expect("material");

        assert_eq!(projection.rows.len(), 18);
        assert_eq!(
            row_text(&projection.rows[5], "bestMatchContentSha256"),
            None
        );
        assert_eq!(
            row_text(&projection.rows[6], "bestMatchContentSha256").as_deref(),
            Some("0000000000000000000000000000000000000000000000000000000000000064")
        );
        assert_eq!(
            row_text(&projection.rows[6], "modelCycle").as_deref(),
            Some("50r1")
        );
        assert_eq!(
            row_text(&projection.rows[6], "bestMatchTemperatureC64").as_deref(),
            Some("4022000000000000")
        );
        assert_eq!(
            projection.source[4].1,
            Nullable::Value(text("a".repeat(64)))
        );
        assert_eq!(
            String::from_utf8(digest.preimage).expect("utf8"),
            concat!(
                "open-meteo-ecmwf-single-run/v1\n",
                "ballydidean\n",
                "47.950430\n",
                "-122.427970\n",
                "ecmwf_ifs\n",
                "forecast_hours=19\n",
                "temperature_2m,relative_humidity_2m,wind_speed_10m\n"
            )
        );
    }

    #[test]
    fn rejects_missing_or_pre_epoch_eligible_comparators() {
        let mut missing = revision();
        missing.bestMatchRows[11].validAt =
            JsDateOrString::String(text("2026-10-08T17:00:00.000Z"));
        let mut digest = FixtureDigest::default();
        assert_eq!(
            prepare_ecmwf_temperature_revision_material(
                &missing,
                &capture_epoch(),
                &FixtureTime,
                &mut digest,
            ),
            Err(EcmwfRevisionMaterialError::Geometry)
        );

        let mut old = revision();
        // move every source-decision product clock behind the epoch without changing its lineage
        for row in &mut old.bestMatchRows {
            row.productRunAt =
                Nullable::Value(JsDateOrString::String(text("2026-10-07T23:00:00.000Z")));
        }
        assert_eq!(
            prepare_ecmwf_temperature_revision_material(
                &old,
                &capture_epoch(),
                &FixtureTime,
                &mut digest,
            ),
            Err(EcmwfRevisionMaterialError::Clock)
        );
    }

    /// Build the complete original worker fixture shape.
    fn revision() -> PersistedEcmwfTemperatureRevision {
        PersistedEcmwfTemperatureRevision {
            adapterVersion: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
            bestMatchRows: (0..12).map(best_match_row).collect(),
            contentHash: "c".repeat(64),
            hours: (0..18).map(native_hour).collect(),
            logicalReceivedAt: "2026-10-08T00:05:00.000Z".to_owned(),
            modelCycle: EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1,
            providerResponseSha256: "d".repeat(64),
            recentErrorState: EcmwfTemperatureCanaryRecentErrorState {
                b24C: Nullable::Null,
                b72C: Nullable::Null,
                cohort: EcmwfTemperatureCanaryRecentErrorStateCohortLiteral,
                localDates: 0,
                mad72C: Nullable::Null,
                maximumSourceRunInitializedAt: Nullable::Null,
                maximumSourceValidAt: Nullable::Null,
                n24: 0,
                n72: 0,
                sourceKeys: vec![],
                supported: false,
                targetRunInitializedAt: "2026-10-08T00:00:00.000Z".to_owned(),
                windowEndValidAt: "2026-10-07T17:00:00.000Z".to_owned(),
            },
            recentErrorStateSha256: "e".repeat(64),
            runInitializedAt: "2026-10-08T00:00:00.000Z".to_owned(),
            siteId: "3".to_owned(),
            siteLatitude: 47.95043,
            siteLongitude: -122.42797,
            siteSlug: "ballydidean".to_owned(),
            upstreamModel: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
        }
    }

    /// Build one native hour in source order.
    fn native_hour(index: usize) -> EcmwfTemperatureCanaryHour {
        EcmwfTemperatureCanaryHour {
            contentHash: format!("{index:064x}"),
            modelLeadHours: (index + 1) as u64,
            rawRelativeHumidityPercent: Nullable::Value(80.0),
            rawTemperatureC: 10.0 + index as f64,
            rawWindSpeedMps: Nullable::Value(5.0),
            validAt: format!("2026-10-08T{:02}:00:00.000Z", index + 1),
        }
    }

    /// Build one best-match row aligned to native leads seven through eighteen.
    fn best_match_row(index: usize) -> WeatherRecordRow {
        let valid_at = format!("2026-10-08T{:02}:00:00.000Z", index + 7);
        WeatherRecordRow {
            adapterVersion: Nullable::Null,
            apparentTemperatureC: Nullable::Null,
            blackGlobeTemperatureC: Nullable::Null,
            cloudCoverPercent: Nullable::Null,
            contentHash: format!("{:064x}", index + 100),
            contractEpoch: Nullable::Null,
            deviceModel: Nullable::Null,
            deviceSerial: Nullable::Null,
            deviceVendor: Nullable::Null,
            firstReceivedAt: JsDateOrString::String(text("2026-10-08T00:05:00.000Z")),
            id: format!("row-{index}"),
            lastReceivedAt: JsDateOrString::String(text("2026-10-08T00:05:00.000Z")),
            pm25MicrogramsPerCubicMeter: Nullable::Null,
            precipitationMm: Nullable::Null,
            precipitationRateMmPerHour: Nullable::Null,
            pressureHpa: Nullable::Null,
            productRunAt: Nullable::Value(JsDateOrString::String(text("2026-10-08T00:00:00.000Z"))),
            providerKey: "open-meteo".to_owned(),
            providerMetadata: Nullable::Value(vec![(
                text("dataset"),
                JsonValue::String(text("best_match")),
            )]),
            qualityMetadata: Nullable::Null,
            relativeHumidityPercent: Nullable::Null,
            revisionCount: 1,
            siteSlug: "ballydidean".to_owned(),
            sourceId: "7".to_owned(),
            sourceConfigFingerprint: Nullable::Null,
            sourceKey: "open-meteo-forecast-v4".to_owned(),
            sourceKind: SourceKind::Forecast,
            stationSlug: "property".to_owned(),
            soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
            soilMoisturePercent: Nullable::Null,
            solarRadiationWm2: Nullable::Null,
            temperatureC: Nullable::Value(9.0 + index as f64),
            upstreamModel: Nullable::Value("best_match".to_owned()),
            upstreamTimezone: "UTC".to_owned(),
            uvIndex: Nullable::Null,
            validAt: JsDateOrString::String(text(valid_at)),
            waterLevelM: Nullable::Null,
            windDirectionDegrees: Nullable::Null,
            windGustMps: Nullable::Null,
            windSpeedMps: Nullable::Null,
            wetBulbGlobeTemperatureC: Nullable::Null,
        }
    }

    /// Return the fixed capture boundary.
    fn capture_epoch() -> CanonicalInstant {
        CanonicalInstant {
            epoch_milliseconds: BASE,
            utc: "2026-10-08T00:00:00.000Z".to_owned(),
        }
    }

    /// Decode one ordered row text member.
    fn row_text(row: &[(Vec<u16>, RowValue)], key: &str) -> Option<String> {
        row.iter().find_map(|(name, value)| {
            // select only the exact UTF-16 field name
            if !name.iter().copied().eq(key.encode_utf16()) {
                return None;
            }
            let RowValue::String(value) = value else {
                return None;
            };
            String::from_utf16(value).ok()
        })
    }

    /// Encode one fixture string as UTF-16.
    fn text(value: impl AsRef<str>) -> Vec<u16> {
        value.as_ref().encode_utf16().collect()
    }

    /// Build one deterministic resolver error.
    fn error() -> CurrentProjectionError {
        CurrentProjectionError {
            code: "fixture_clock",
            detail: "fixture clock differs".to_owned(),
        }
    }
}
