//! Build the original maintenance-source graph before canonical private serialization.

use weather_contracts::{
    EcmwfTemperatureCanaryRecentErrorState, EcmwfTemperatureCanarySidecar, JsDateOrString,
    JsonValue, MaintenanceShadowSourceIdentity, Nullable, WeatherRecordRow,
};
use weather_database::Sha256Digest;

use crate::{
    CanonicalInstant, CurrentTimeResolver,
    maintenance_temperature_projection::project_temperature_maintenance_source_rows,
    maintenance_wind_projection::project_wind_maintenance_source_rows,
};

const HOUR_MS: i64 = 3_600_000;
const WIND_LIMIT_BYTES: usize = 768 * 1_024;
const TEMPERATURE_LIMIT_BYTES: usize = 64 * 1_024;

/// Original TypeScript source-projection discriminator.
pub const MAINTENANCE_SOURCE_PROJECTION_VERSION: &str = "adjustment-shadow-source-projection/v1";

/// One private source scalar staged for the admitted canonical JSON codec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaintenanceSourceValue {
    Text(String),
    Utf16(Vec<u16>),
    Number(u64),
    Null,
}

/// One ordered field in the immutable TypeScript family schema.
pub type MaintenanceSourceField = (&'static str, MaintenanceSourceValue);

/// Scope the original candidate, registration, and source evidence to one scheduled capture.
#[derive(Debug, Clone, Copy)]
pub struct MaintenanceSourceProjectionHeader<'a> {
    pub registration_sha256: &'a str,
    pub candidate_sha256: &'a str,
    pub source_sha256: &'a str,
    pub due_key: &'a str,
    pub issued_at: &'a CanonicalInstant,
}

/// The exact ordered family inputs, excluding the rain graph's separately owned path.
#[derive(Debug)]
pub enum MaintenanceSourceProjectionRows<'a> {
    Wind(Vec<[MaintenanceSourceField; 17]>),
    Temperature {
        recent_error_state: &'a EcmwfTemperatureCanaryRecentErrorState,
        rows: Vec<[MaintenanceSourceField; 20]>,
    },
}

/// Already checked source projection using generated TS rows and an admitted family shape.
#[derive(Debug)]
pub struct MaintenanceSourceProjection<'a> {
    header: MaintenanceSourceProjectionHeader<'a>,
    rows: MaintenanceSourceProjectionRows<'a>,
}

impl<'a> MaintenanceSourceProjection<'a> {
    /// Borrow the scheduled context without duplicating its identity strings.
    pub const fn header(&self) -> MaintenanceSourceProjectionHeader<'a> {
        self.header
    }

    /// Report the exact source family selected by the checked row variant.
    pub const fn family(&self) -> &'static str {
        match &self.rows {
            MaintenanceSourceProjectionRows::Wind(_) => "wind",
            MaintenanceSourceProjectionRows::Temperature { .. } => "temperature",
        }
    }

    /// Borrow the full typed family payload for the admitted codec and evaluator.
    pub const fn rows(&self) -> &MaintenanceSourceProjectionRows<'a> {
        &self.rows
    }

    /// Borrow one verified source row already in the original canonical order.
    pub fn row_fields(&self, index: usize) -> Option<&[MaintenanceSourceField]> {
        match &self.rows {
            MaintenanceSourceProjectionRows::Wind(rows) => {
                rows.get(index).map(|row| row.as_slice())
            }
            MaintenanceSourceProjectionRows::Temperature { rows, .. } => {
                rows.get(index).map(|row| row.as_slice())
            }
        }
    }

    /// Report the fixed checked lead count without consuming the model inputs.
    pub fn row_count(&self) -> usize {
        match &self.rows {
            MaintenanceSourceProjectionRows::Wind(rows) => rows.len(),
            MaintenanceSourceProjectionRows::Temperature { rows, .. } => rows.len(),
        }
    }
}

/// Bounded reasons an ordinary capture cannot become complete source evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenanceSourceProjectionError {
    Incomplete,
    Geometry,
    Clock,
    Identity,
    Lineage,
    Metric,
    CausalState,
    CaptureEpoch,
    Canonical,
}

type ProjectionError = MaintenanceSourceProjectionError;

/// Admit all 168 raw Best Match wind inputs before entering the model/shadow path.
pub fn build_wind_maintenance_source_projection<'a>(
    header: MaintenanceSourceProjectionHeader<'a>,
    rows: &[WeatherRecordRow],
    time: &dyn CurrentTimeResolver,
) -> Result<MaintenanceSourceProjection<'a>, MaintenanceSourceProjectionError> {
    admit_source_header(header, time)?;
    let rows =
        project_wind_maintenance_source_rows(rows, header.issued_at, header.source_sha256, time)?;
    Ok(MaintenanceSourceProjection {
        header,
        rows: MaintenanceSourceProjectionRows::Wind(rows),
    })
}

/// Admit all 12 paired ECMWF/Best Match inputs and their unchanged rolling error state.
pub fn build_temperature_maintenance_source_projection<'a>(
    header: MaintenanceSourceProjectionHeader<'a>,
    best_match: &[WeatherRecordRow],
    sidecar: &'a EcmwfTemperatureCanarySidecar,
    time: &dyn CurrentTimeResolver,
) -> Result<MaintenanceSourceProjection<'a>, MaintenanceSourceProjectionError> {
    admit_source_header(header, time)?;
    let rows = project_temperature_maintenance_source_rows(
        best_match,
        sidecar,
        header.issued_at,
        header.source_sha256,
        time,
    )?;
    Ok(MaintenanceSourceProjection {
        header,
        rows: MaintenanceSourceProjectionRows::Temperature {
            recent_error_state: &sidecar.run.recentErrorState,
            rows,
        },
    })
}

/// The approved runtime owns exact JSON serialization and hashing, not the model projection.
pub trait MaintenanceSourceCanonicalCodec {
    /// Encode the original source header and family payload in TS order with one trailing LF.
    fn encode_source(&mut self, projection: &MaintenanceSourceProjection<'_>) -> Option<Vec<u8>>;

    /// Encode a row's exact ordered keys and values without a trailing LF.
    fn encode_row(&mut self, fields: &[MaintenanceSourceField]) -> Option<Vec<u8>>;

    /// Digest the exact ordered byte parts as one input without delimiter insertion.
    fn sha256(&mut self, parts: &[&[u8]]) -> Option<Sha256Digest>;
}

/// Exact canonical source and source identities ready for candidate evaluation and durable staging.
pub struct PreparedMaintenanceSource<'a> {
    pub projection: MaintenanceSourceProjection<'a>,
    pub bytes: Vec<u8>,
    pub identity: MaintenanceShadowSourceIdentity,
}

/// Bind a built source to the loaded capture epoch before handing it to the model.
pub fn prepare_maintenance_source<'a>(
    projection: MaintenanceSourceProjection<'a>,
    capture_epoch: &CanonicalInstant,
    time: &dyn CurrentTimeResolver,
    codec: &mut impl MaintenanceSourceCanonicalCodec,
) -> Result<PreparedMaintenanceSource<'a>, ProjectionError> {
    require_maintenance_source_capture_epoch(&projection, capture_epoch, time)?;
    let (bytes, identity) = encode_and_identify_maintenance_source(&projection, codec)?;
    Ok(PreparedMaintenanceSource {
        projection,
        bytes,
        identity,
    })
}

/// Preserve the original future-only source frontier without rescanning decoded database rows.
fn require_maintenance_source_capture_epoch(
    projection: &MaintenanceSourceProjection<'_>,
    capture_epoch: &CanonicalInstant,
    time: &dyn CurrentTimeResolver,
) -> Result<(), ProjectionError> {
    let epoch = admit_source_wire_clock(time, &capture_epoch.utc)?;
    // reject an epoch whose admitted text and numerical instant differ
    if epoch.epoch_milliseconds != capture_epoch.epoch_milliseconds {
        return Err(ProjectionError::CaptureEpoch);
    }
    let temperature = matches!(
        projection.rows(),
        MaintenanceSourceProjectionRows::Temperature { .. }
    );
    // compare already validated four-digit UTC clocks directly without reparsing each forecast row
    for index in 0..projection.row_count() {
        let fields = projection
            .row_fields(index)
            .ok_or(ProjectionError::Incomplete)?;
        let clocks = ["referenceAt", "receivedAt", "bestMatchProductRunAt"];
        // temperature alone carries a separate Best Match initialization
        for clock in &clocks[..if temperature { 3 } else { 2 }] {
            let Some((_, MaintenanceSourceValue::Text(value))) =
                fields.iter().find(|(field, _)| field == clock)
            else {
                return Err(ProjectionError::CaptureEpoch);
            };
            // old backfill clocks cannot qualify as prospective evidence
            if value < &epoch.utc {
                return Err(ProjectionError::CaptureEpoch);
            }
        }
    }

    // temperature inference retains the entire rolling window and its populated causal maxima
    if let MaintenanceSourceProjectionRows::Temperature {
        recent_error_state, ..
    } = projection.rows()
    {
        let state = recent_error_state;
        let window_end = admit_source_wire_clock(time, &state.windowEndValidAt)?;
        let earliest = window_end
            .epoch_milliseconds
            .checked_sub(71 * HOUR_MS)
            .ok_or(ProjectionError::CaptureEpoch)?;
        // preserve the admitted epoch even for the opposite edge of an otherwise valid window
        if earliest < epoch.epoch_milliseconds || state.targetRunInitializedAt < epoch.utc {
            return Err(ProjectionError::CaptureEpoch);
        }
        // keep absent cold-state maxima distinct from actual earlier source clocks
        for value in [
            &state.maximumSourceRunInitializedAt,
            &state.maximumSourceValidAt,
        ] {
            // only populated causal maxima represent provenance to compare
            if matches!(value, Nullable::Value(clock) if clock < &epoch.utc) {
                return Err(ProjectionError::CaptureEpoch);
            }
        }
    }
    Ok(())
}

/// Produce the byte-bound original TS source identity and each ordered row identity.
pub fn encode_and_identify_maintenance_source(
    projection: &MaintenanceSourceProjection<'_>,
    codec: &mut impl MaintenanceSourceCanonicalCodec,
) -> Result<(Vec<u8>, MaintenanceShadowSourceIdentity), MaintenanceSourceProjectionError> {
    let bytes = codec
        .encode_source(projection)
        .ok_or(ProjectionError::Canonical)?;
    let limit = match projection.rows() {
        MaintenanceSourceProjectionRows::Wind(_) => WIND_LIMIT_BYTES,
        MaintenanceSourceProjectionRows::Temperature { .. } => TEMPERATURE_LIMIT_BYTES,
    };
    // preserve the original newline and bounded private relay byte count
    if bytes.len() > limit || !bytes.ends_with(b"}\n") {
        return Err(ProjectionError::Canonical);
    }
    let input_sha256 = source_sha256(codec, &[&bytes])?;
    let family = projection.family().as_bytes();
    let count = projection.row_count();
    let mut source_row_sha256 = Vec::with_capacity(count);
    // bind each canonical row to its own fixed family-specific preimage
    for index in 0..count {
        let fields = projection
            .row_fields(index)
            .ok_or(ProjectionError::Incomplete)?;
        let encoded = codec.encode_row(fields).ok_or(ProjectionError::Canonical)?;
        // do not permit a codec's extra newline to change a row identity
        if !encoded.ends_with(b"}") {
            return Err(ProjectionError::Canonical);
        }
        source_row_sha256.push(source_sha256(
            codec,
            &[
                b"adjustment-shadow-source-row/v1\n",
                family,
                b"\n",
                &encoded,
                b"\n",
            ],
        )?);
    }
    let header = projection.header();
    let first = source_row_valid_at(projection, 0)?;
    let last = source_row_valid_at(
        projection,
        count.checked_sub(1).ok_or(ProjectionError::Incomplete)?,
    )?;
    let count = count.to_string();
    // preserve the exact newline-dispatched source receipt with no trailing delimiter
    let receipt = source_sha256(
        codec,
        &[
            b"adjustment-shadow-source-receipt/v1\n",
            header.registration_sha256.as_bytes(),
            b"\n",
            header.candidate_sha256.as_bytes(),
            b"\n",
            header.source_sha256.as_bytes(),
            b"\n",
            header.due_key.as_bytes(),
            b"\n",
            header.issued_at.utc.as_bytes(),
            b"\n",
            first.as_bytes(),
            b"\n",
            last.as_bytes(),
            b"\n",
            count.as_bytes(),
            b"\n",
            input_sha256.as_bytes(),
        ],
    )?;
    Ok((
        bytes,
        MaintenanceShadowSourceIdentity {
            inputSha256: input_sha256,
            sourceReceiptSha256: receipt,
            sourceRowSha256: source_row_sha256,
        },
    ))
}

/// Borrow the original valid clock from the first field in each closed row schema.
fn source_row_valid_at<'a>(
    projection: &'a MaintenanceSourceProjection<'_>,
    index: usize,
) -> Result<&'a str, ProjectionError> {
    match projection.row_fields(index).and_then(<[_]>::first) {
        Some(("validAt", MaintenanceSourceValue::Text(value))) => Ok(value),
        _ => Err(ProjectionError::Clock),
    }
}

/// Return only an admitted lowercase digest from the canonical owner.
fn source_sha256(
    codec: &mut impl MaintenanceSourceCanonicalCodec,
    parts: &[&[u8]],
) -> Result<String, ProjectionError> {
    codec
        .sha256(parts)
        .filter(|digest| is_source_digest(digest.as_str()))
        .map(|digest| digest.as_str().to_owned())
        .ok_or(ProjectionError::Canonical)
}

/// Verify the source header and the original fixed six-hour capture key.
fn admit_source_header(
    header: MaintenanceSourceProjectionHeader<'_>,
    time: &dyn CurrentTimeResolver,
) -> Result<(), ProjectionError> {
    // refuse unbound registration, candidate, or source identities
    if ![
        header.registration_sha256,
        header.candidate_sha256,
        header.source_sha256,
    ]
    .into_iter()
    .all(is_source_digest)
    {
        return Err(ProjectionError::Identity);
    }
    let issued = admit_source_wire_clock(time, &header.issued_at.utc)?;
    // require epoch and textual clock to describe the same admitted input
    if issued.epoch_milliseconds != header.issued_at.epoch_milliseconds {
        return Err(ProjectionError::Clock);
    }
    let due = header
        .due_key
        .strip_prefix("capture/")
        .ok_or(ProjectionError::Clock)?;
    let due = admit_source_wire_clock(time, due)?;
    let at = due.utc.as_bytes();
    // the already parsed canonical UTC string has a fixed hour and millisecond layout
    if !matches!(&at[11..13], b"00" | b"06" | b"12" | b"18") || &at[13..] != b":35:00.000Z" {
        return Err(ProjectionError::Clock);
    }
    Ok(())
}

/// Resolve an original database Date or date-like string through the admitted UTC owner.
pub(crate) fn admit_source_clock(
    time: &dyn CurrentTimeResolver,
    value: &JsDateOrString,
) -> Result<CanonicalInstant, ProjectionError> {
    let resolved = time.resolve(value).map_err(|_| ProjectionError::Clock)?;
    // reject noncanonical shape even when an injected resolver returned a value
    if !exact_source_utc_shape(&resolved.utc) {
        return Err(ProjectionError::Clock);
    }
    Ok(resolved)
}

/// Admit an already canonical wire clock without tolerating text normalization.
pub(crate) fn admit_source_wire_clock(
    time: &dyn CurrentTimeResolver,
    value: &str,
) -> Result<CanonicalInstant, ProjectionError> {
    let resolved = admit_source_clock(
        time,
        &JsDateOrString::String(value.encode_utf16().collect()),
    )?;
    // require source evidence itself to use the exact approved wire representation
    if resolved.utc != value {
        return Err(ProjectionError::Clock);
    }
    Ok(resolved)
}

/// Match the fixed millisecond layout without implementing a calendar parser.
fn exact_source_utc_shape(value: &str) -> bool {
    let value = value.as_bytes();
    value.len() == 24
        && value.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'.',
            23 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        })
}

/// Preserve complete hourly prospective geometry and the original ceiling model lead.
pub(crate) fn admit_source_geometry(
    issued: &CanonicalInstant,
    valid: &CanonicalInstant,
    reference: &CanonicalInstant,
    received: &CanonicalInstant,
    previous_valid_ms: Option<i64>,
    declared_model_lead: Option<u64>,
) -> Result<u64, ProjectionError> {
    let valid = valid.epoch_milliseconds;
    let issued = issued.epoch_milliseconds;
    // require prospective, consecutive forecast rows with no late receipt
    if valid <= issued
        || received.epoch_milliseconds > issued
        || previous_valid_ms.is_some_and(|previous| previous.checked_add(HOUR_MS) != Some(valid))
    {
        return Err(ProjectionError::Geometry);
    }
    let span = valid
        .checked_sub(reference.epoch_milliseconds)
        .ok_or(ProjectionError::Geometry)?;
    let lead = span.div_euclid(HOUR_MS) + i64::from(span.rem_euclid(HOUR_MS) != 0);
    let lead = u64::try_from(lead).map_err(|_| ProjectionError::Geometry)?;
    // verify the exact duration-derived source lead even when supplied by the model row
    if lead > 384 || declared_model_lead.is_some_and(|declared| declared != lead) {
        return Err(ProjectionError::Geometry);
    }
    Ok(lead)
}

/// Keep raw finite values in exact big-endian IEEE binary64 hex, including negative zero.
pub(crate) fn source_binary64(
    value: &Nullable<f64>,
    minimum: f64,
    maximum: f64,
) -> Result<MaintenanceSourceValue, ProjectionError> {
    match value {
        Nullable::Null => Ok(MaintenanceSourceValue::Null),
        Nullable::Value(number) if (minimum..=maximum).contains(number) => Ok(
            MaintenanceSourceValue::Text(format!("{:016x}", number.to_bits())),
        ),
        Nullable::Value(_) => Err(ProjectionError::Metric),
    }
}

/// Check the original lowercase SHA-256 field shape.
pub(crate) fn is_source_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|digit| digit.is_ascii_digit() || matches!(digit, b'a'..=b'f'))
}

/// Admit bounded nonempty upstream text by its original UTF-16 wire length.
pub(crate) fn bounded_source_text(value: &str) -> bool {
    !value.is_empty()
        && value.encode_utf16().count() <= 128
        && !value.bytes().any(|byte| byte < 32 || byte == 127)
}

/// Preserve raw JavaScript code units when the upstream JSON metadata owns the text.
pub(crate) fn bounded_source_utf16(value: &[u16]) -> bool {
    (1..=128).contains(&value.len()) && !value.iter().any(|unit| *unit < 32 || *unit == 127)
}

/// Borrow the exact typed dataset in the database JSON metadata without conversion.
pub(crate) fn forecast_dataset(row: &WeatherRecordRow) -> Option<&[u16]> {
    const DATASET: [u16; 7] = [100, 97, 116, 97, 115, 101, 116];
    let Nullable::Value(metadata) = &row.providerMetadata else {
        return None;
    };
    metadata.iter().find_map(|(key, value)| {
        // treat a missing or nonstring dataset as incomplete source material
        match value {
            JsonValue::String(value) if key == &DATASET => Some(value.as_slice()),
            _ => None,
        }
    })
}
