//! Typed PostgreSQL plans for API-owned maintenance registration and prediction appends.

use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptProjectionKindEnum,
    AdjustmentShadowFamily, AdjustmentShadowPrediction, AdjustmentShadowPredictionReceipt,
    AdjustmentShadowRegistrationReceipt,
};

use crate::SerializedJson;

const MAX_INPUT_BYTES: usize = 2_048;
const HOUR_MILLISECONDS: i64 = 3_600_000;

const REGISTER_V2_SQL: &str = "SELECT weather_register_adjustment_shadow_v2($1::jsonb) AS value";
const REGISTER_V3_SQL: &str = "SELECT weather_register_adjustment_shadow_v3($1::jsonb) AS value";
const APPEND_TEMPERATURE_SQL: &str =
    "SELECT weather_append_adjustment_temperature_shadow_v2($1::jsonb) AS value";
const APPEND_WIND_SQL: &str = "SELECT weather_append_adjustment_wind_shadow_v2($1::jsonb) AS value";
const APPEND_RAIN_SQL: &str = "SELECT weather_append_adjustment_rain_shadow_v2($1::jsonb) AS value";

const RAIN_SCHEMA_SHA256: &str = "c7ae2f750f13970137b1b2d885f7cce81b8ecbc2720635c6716f005bc04a5e25";
const TEMPERATURE_SCHEMA_SHA256: &str =
    "4255feacfd464adf2cbbf1139ecdf30d9d00b847775c556407367ad1449d9e63";
const WIND_SCHEMA_SHA256: &str = "3f573c3e49ed3b97636674b0630e49508cf80533ff8edc8f31f6f0411f70d902";

/// Closed database registration function selected by the admitted loader version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiMaintenanceRegistrationVersion {
    V2,
    V3,
}

/// Registration material already validated and serialized by the approved loader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmittedApiMaintenanceRegistration<'a> {
    pub family: AdjustmentShadowFamily,
    pub version: ApiMaintenanceRegistrationVersion,
    pub registration_sha256: &'a str,
    pub candidate_sha256: &'a str,
    pub source_sha256: &'a str,
    pub canonical_json: &'a str,
}

/// One exact canonical JSON bind for a closed PostgreSQL function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiMaintenanceJsonBind<'a> {
    CanonicalJson(&'a str),
}

/// Exact versioned registration call and its single JSON bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiMaintenanceRegistrationPlan<'a> {
    pub sql: &'static str,
    pub binds: [ApiMaintenanceJsonBind<'a>; 1],
}

/// Exact family-specific prediction append and its single JSON bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiMaintenancePredictionPlan<'a> {
    pub sql: &'static str,
    pub binds: [ApiMaintenanceJsonBind<'a>; 1],
}

/// PostgreSQL execution boundary with typed decoding for both compact writes.
pub trait ApiMaintenancePredictionWritePort {
    type Error;

    /// Execute one versioned registration and decode all returned receipt rows.
    fn register_api_shadow(
        &mut self,
        plan: ApiMaintenanceRegistrationPlan<'_>,
    ) -> Result<Vec<AdjustmentShadowRegistrationReceipt>, Self::Error>;

    /// Execute one family append and decode all returned prediction receipts.
    fn append_api_shadow(
        &mut self,
        plan: ApiMaintenancePredictionPlan<'_>,
    ) -> Result<Vec<AdjustmentShadowPredictionReceipt>, Self::Error>;
}

/// Ingest-role PostgreSQL boundary restricted to rain maintenance writes.
pub trait RainMaintenancePredictionWritePort {
    type Error;

    /// Execute one rain registration and decode all returned receipt rows.
    fn register_rain_shadow(
        &mut self,
        plan: ApiMaintenanceRegistrationPlan<'_>,
    ) -> Result<Vec<AdjustmentShadowRegistrationReceipt>, Self::Error>;

    /// Execute one rain prediction append and decode all returned receipt rows.
    fn append_rain_shadow(
        &mut self,
        plan: ApiMaintenancePredictionPlan<'_>,
    ) -> Result<Vec<AdjustmentShadowPredictionReceipt>, Self::Error>;
}

/// Approved exact UTC parser and canonical flat-JSON encoder.
pub trait ApiMaintenancePredictionValues {
    type Error;

    /// Parse one exact canonical UTC millisecond instant.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error>;

    /// Encode the exact sorted-key compact prediction object without a newline.
    fn canonical_prediction_json(
        &mut self,
        prediction: &AdjustmentShadowPrediction,
    ) -> Result<SerializedJson, Self::Error>;
}

/// Local admission failures before or after one typed database call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiMaintenanceWriteAdmissionError {
    UnsupportedFamily,
    CanonicalJsonTooLarge,
    InvalidPredictionIdentity,
    InvalidPredictionSchema,
    InvalidPredictionRowCount,
    InvalidPredictionBodyByteCount,
    InvalidDueKey,
    InvalidPredictionIssueWindow,
    InvalidPredictionValidityWindow,
    ReceiptCardinality,
    RegistrationReceiptMismatch,
    PredictionReceiptMismatch,
    RevisionReceiptMismatch,
}

/// Registration failure preserving the database driver error.
#[derive(Debug, PartialEq, Eq)]
pub enum ApiMaintenanceRegistrationWriteError<PortError> {
    Admission(ApiMaintenanceWriteAdmissionError),
    Port(PortError),
}

/// Prediction failure preserving both database and approved value-owner errors.
#[derive(Debug, PartialEq, Eq)]
pub enum ApiMaintenancePredictionWriteError<PortError, ValuesError> {
    Admission(ApiMaintenanceWriteAdmissionError),
    Port(PortError),
    Values(ValuesError),
}

#[derive(Debug, Clone, Copy)]
struct FamilyLimits {
    append_sql: &'static str,
    body_bytes: u64,
    rows: u64,
    schema_sha256: &'static str,
}

/// Register one already admitted API temperature or wind shadow package.
pub fn register_api_maintenance_shadow<Port>(
    port: &mut Port,
    registration: &AdmittedApiMaintenanceRegistration<'_>,
) -> Result<AdjustmentShadowRegistrationReceipt, ApiMaintenanceRegistrationWriteError<Port::Error>>
where
    Port: ApiMaintenancePredictionWritePort,
{
    // keep the API role restricted to its original temperature and wind families
    if family_limits(registration.family).is_none() {
        return Err(registration_admission(
            ApiMaintenanceWriteAdmissionError::UnsupportedFamily,
        ));
    }
    let plan = registration_plan(registration)?;
    let receipts = port
        .register_api_shadow(plan)
        .map_err(ApiMaintenanceRegistrationWriteError::Port)?;
    validate_registration_receipt(receipts, registration)
}

/// Register one admitted ingest-owned rain shadow package.
pub fn register_rain_maintenance_shadow<Port>(
    port: &mut Port,
    registration: &AdmittedApiMaintenanceRegistration<'_>,
) -> Result<AdjustmentShadowRegistrationReceipt, ApiMaintenanceRegistrationWriteError<Port::Error>>
where
    Port: RainMaintenancePredictionWritePort,
{
    // keep the ingest role restricted to the rain family
    if registration.family != AdjustmentShadowFamily::Rain {
        return Err(registration_admission(
            ApiMaintenanceWriteAdmissionError::UnsupportedFamily,
        ));
    }
    let plan = registration_plan(registration)?;
    let receipts = port
        .register_rain_shadow(plan)
        .map_err(ApiMaintenanceRegistrationWriteError::Port)?;
    validate_registration_receipt(receipts, registration)
}

/// Validate and append one compact API temperature or wind prediction receipt.
pub fn append_api_maintenance_shadow<Port, Values>(
    port: &mut Port,
    values: &mut Values,
    family: AdjustmentShadowFamily,
    prediction: &AdjustmentShadowPrediction,
) -> Result<
    AdjustmentShadowPredictionReceipt,
    ApiMaintenancePredictionWriteError<Port::Error, Values::Error>,
>
where
    Port: ApiMaintenancePredictionWritePort,
    Values: ApiMaintenancePredictionValues,
{
    let limits = family_limits(family).ok_or(prediction_admission(
        ApiMaintenanceWriteAdmissionError::UnsupportedFamily,
    ))?;
    let canonical = prepare_prediction::<Port::Error, _>(values, prediction, limits)?;
    let receipts = port
        .append_api_shadow(ApiMaintenancePredictionPlan {
            sql: limits.append_sql,
            binds: [ApiMaintenanceJsonBind::CanonicalJson(canonical.as_str())],
        })
        .map_err(ApiMaintenancePredictionWriteError::Port)?;
    validate_prediction_receipt(receipts, prediction, values)
}

/// Validate and append one compact ingest-owned rain prediction receipt.
pub fn append_rain_maintenance_shadow<Port, Values>(
    port: &mut Port,
    values: &mut Values,
    prediction: &AdjustmentShadowPrediction,
) -> Result<
    AdjustmentShadowPredictionReceipt,
    ApiMaintenancePredictionWriteError<Port::Error, Values::Error>,
>
where
    Port: RainMaintenancePredictionWritePort,
    Values: ApiMaintenancePredictionValues,
{
    let limits = rain_family_limits();
    let canonical = prepare_prediction::<Port::Error, _>(values, prediction, limits)?;
    let receipts = port
        .append_rain_shadow(ApiMaintenancePredictionPlan {
            sql: limits.append_sql,
            binds: [ApiMaintenanceJsonBind::CanonicalJson(canonical.as_str())],
        })
        .map_err(ApiMaintenancePredictionWriteError::Port)?;
    validate_prediction_receipt(receipts, prediction, values)
}

/// Validate and serialize one family-bounded compact prediction.
fn prepare_prediction<PortError, Values>(
    values: &mut Values,
    prediction: &AdjustmentShadowPrediction,
    limits: FamilyLimits,
) -> Result<SerializedJson, ApiMaintenancePredictionWriteError<PortError, Values::Error>>
where
    Values: ApiMaintenancePredictionValues,
{
    // validate every client-owned compact prediction identity before clock work
    if [
        prediction.candidateSha256.as_str(),
        prediction.inputSha256.as_str(),
        prediction.predictionBodySha256.as_str(),
        prediction.predictionSchemaSha256.as_str(),
        prediction.predictionSha256.as_str(),
        prediction.registrationSha256.as_str(),
        prediction.sourceReceiptSha256.as_str(),
        prediction.sourceSha256.as_str(),
        prediction.stageReceiptSha256.as_str(),
    ]
    .into_iter()
    .any(|identity| !valid_sha256(identity))
    {
        return Err(prediction_admission(
            ApiMaintenanceWriteAdmissionError::InvalidPredictionIdentity,
        ));
    }
    // require the exact public family schema before row and body bounds
    if prediction.predictionSchemaSha256 != limits.schema_sha256 {
        return Err(prediction_admission(
            ApiMaintenanceWriteAdmissionError::InvalidPredictionSchema,
        ));
    }
    // complete family bodies cannot qualify through shortened row sets
    if prediction.rowCount != limits.rows {
        return Err(prediction_admission(
            ApiMaintenanceWriteAdmissionError::InvalidPredictionRowCount,
        ));
    }
    // preserve the original nonempty family-specific body envelope
    if !(1..=limits.body_bytes).contains(&prediction.bodyByteCount) {
        return Err(prediction_admission(
            ApiMaintenanceWriteAdmissionError::InvalidPredictionBodyByteCount,
        ));
    }

    let due_wire = due_clock(&prediction.dueKey).ok_or(prediction_admission(
        ApiMaintenanceWriteAdmissionError::InvalidDueKey,
    ))?;
    let due_at = values
        .utc_epoch_milliseconds(due_wire)
        .map_err(ApiMaintenancePredictionWriteError::Values)?;
    let issued_at = values
        .utc_epoch_milliseconds(&prediction.issuedAt)
        .map_err(ApiMaintenancePredictionWriteError::Values)?;
    let min_valid_at = values
        .utc_epoch_milliseconds(&prediction.minValidAt)
        .map_err(ApiMaintenancePredictionWriteError::Values)?;
    let max_valid_at = values
        .utc_epoch_milliseconds(&prediction.maxValidAt)
        .map_err(ApiMaintenancePredictionWriteError::Values)?;
    let due_window_end = due_at
        .checked_add(12 * HOUR_MILLISECONDS)
        .ok_or(prediction_admission(
            ApiMaintenanceWriteAdmissionError::InvalidPredictionIssueWindow,
        ))?;
    // preserve issue-time jitter only inside the exact twelve-hour due window
    if issued_at < due_at || issued_at > due_window_end {
        return Err(prediction_admission(
            ApiMaintenanceWriteAdmissionError::InvalidPredictionIssueWindow,
        ));
    }
    // require source issue time before one ordered prediction validity range
    if issued_at >= min_valid_at || min_valid_at > max_valid_at {
        return Err(prediction_admission(
            ApiMaintenanceWriteAdmissionError::InvalidPredictionValidityWindow,
        ));
    }

    let canonical = values
        .canonical_prediction_json(prediction)
        .map_err(ApiMaintenancePredictionWriteError::Values)?;
    // retain the original bounded canonical database input envelope
    if canonical.as_str().len() > MAX_INPUT_BYTES {
        return Err(prediction_admission(
            ApiMaintenanceWriteAdmissionError::CanonicalJsonTooLarge,
        ));
    }
    Ok(canonical)
}

/// Validate one returned prediction receipt against the submitted identity.
fn validate_prediction_receipt<PortError, Values>(
    receipts: Vec<AdjustmentShadowPredictionReceipt>,
    prediction: &AdjustmentShadowPrediction,
    values: &mut Values,
) -> Result<
    AdjustmentShadowPredictionReceipt,
    ApiMaintenancePredictionWriteError<PortError, Values::Error>,
>
where
    Values: ApiMaintenancePredictionValues,
{
    let receipt = one_prediction_receipt(receipts).map_err(prediction_admission)?;
    values
        .utc_epoch_milliseconds(&receipt.committedAt)
        .map_err(ApiMaintenancePredictionWriteError::Values)?;
    validate_revision_receipt(&receipt.revisionReceipt, prediction, values)?;
    // require the outer database receipt to bind the supplied prediction identity
    if receipt.predictionSha256 != prediction.predictionSha256 {
        return Err(prediction_admission(
            ApiMaintenanceWriteAdmissionError::PredictionReceiptMismatch,
        ));
    }
    Ok(receipt)
}

/// Build the shared versioned registration plan after its bounded envelope check.
fn registration_plan<'a, PortError>(
    registration: &AdmittedApiMaintenanceRegistration<'a>,
) -> Result<ApiMaintenanceRegistrationPlan<'a>, ApiMaintenanceRegistrationWriteError<PortError>> {
    // retain the original two-kibibyte canonical registration envelope
    if registration.canonical_json.len() > MAX_INPUT_BYTES {
        return Err(registration_admission(
            ApiMaintenanceWriteAdmissionError::CanonicalJsonTooLarge,
        ));
    }
    let sql = match registration.version {
        ApiMaintenanceRegistrationVersion::V2 => REGISTER_V2_SQL,
        ApiMaintenanceRegistrationVersion::V3 => REGISTER_V3_SQL,
    };
    Ok(ApiMaintenanceRegistrationPlan {
        sql,
        binds: [ApiMaintenanceJsonBind::CanonicalJson(
            registration.canonical_json,
        )],
    })
}

/// Validate one returned registration receipt against the submitted identity.
fn validate_registration_receipt<PortError>(
    receipts: Vec<AdjustmentShadowRegistrationReceipt>,
    registration: &AdmittedApiMaintenanceRegistration<'_>,
) -> Result<AdjustmentShadowRegistrationReceipt, ApiMaintenanceRegistrationWriteError<PortError>> {
    let receipt = one_registration_receipt(receipts).map_err(registration_admission)?;
    // require the database receipt to bind the supplied registration identity
    if receipt.registrationSha256 != registration.registration_sha256 {
        return Err(registration_admission(
            ApiMaintenanceWriteAdmissionError::RegistrationReceiptMismatch,
        ));
    }
    Ok(receipt)
}

/// Validate one server-owned revision receipt and its staged shadow binding.
fn validate_revision_receipt<PortError, Values>(
    receipt: &AdjustmentRevisionCommitReceipt,
    prediction: &AdjustmentShadowPrediction,
    values: &mut Values,
) -> Result<(), ApiMaintenancePredictionWriteError<PortError, Values::Error>>
where
    Values: ApiMaintenancePredictionValues,
{
    // require one positive decimal database archive ordinal
    if !positive_decimal(&receipt.archiveCommitOrdinal) {
        return Err(prediction_admission(
            ApiMaintenanceWriteAdmissionError::RevisionReceiptMismatch,
        ));
    }
    values
        .utc_epoch_milliseconds(&receipt.archiveCommittedAt)
        .map_err(ApiMaintenancePredictionWriteError::Values)?;
    // validate every database-owned chain identity before the input binding
    if [
        receipt.frontierSha256.as_str(),
        receipt.predecessorFrontierSha256.as_str(),
        receipt.projectionIdentitySha256.as_str(),
        receipt.projectionSha256.as_str(),
        receipt.receiptSha256.as_str(),
        receipt.stageReceiptSha256.as_str(),
    ]
    .into_iter()
    .any(|identity| !valid_sha256(identity))
    {
        return Err(prediction_admission(
            ApiMaintenanceWriteAdmissionError::RevisionReceiptMismatch,
        ));
    }
    // bind the nested receipt to the distinct staged shadow projection grammar
    if receipt.projectionKind != AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction
        || receipt.projectionIdentitySha256 != prediction.sourceReceiptSha256
        || receipt.projectionSha256 != prediction.inputSha256
        || receipt.stageReceiptSha256 != prediction.stageReceiptSha256
    {
        return Err(prediction_admission(
            ApiMaintenanceWriteAdmissionError::RevisionReceiptMismatch,
        ));
    }
    Ok(())
}

/// Select the exact family schema, row count, body cap, and PostgreSQL function.
const fn family_limits(family: AdjustmentShadowFamily) -> Option<FamilyLimits> {
    match family {
        AdjustmentShadowFamily::Temperature => Some(FamilyLimits {
            append_sql: APPEND_TEMPERATURE_SQL,
            body_bytes: 8_192,
            rows: 12,
            schema_sha256: TEMPERATURE_SCHEMA_SHA256,
        }),
        AdjustmentShadowFamily::Wind => Some(FamilyLimits {
            append_sql: APPEND_WIND_SQL,
            body_bytes: 65_536,
            rows: 168,
            schema_sha256: WIND_SCHEMA_SHA256,
        }),
        AdjustmentShadowFamily::Rain => None,
    }
}

/// Return the fixed ingest-owned rain append limits.
const fn rain_family_limits() -> FamilyLimits {
    FamilyLimits {
        append_sql: APPEND_RAIN_SQL,
        body_bytes: 12_288,
        rows: 23,
        schema_sha256: RAIN_SCHEMA_SHA256,
    }
}

/// Extract one registration row without accepting zero or duplicate receipts.
fn one_registration_receipt(
    receipts: Vec<AdjustmentShadowRegistrationReceipt>,
) -> Result<AdjustmentShadowRegistrationReceipt, ApiMaintenanceWriteAdmissionError> {
    let [receipt]: [AdjustmentShadowRegistrationReceipt; 1] = receipts
        .try_into()
        .map_err(|_| ApiMaintenanceWriteAdmissionError::ReceiptCardinality)?;
    Ok(receipt)
}

/// Extract one prediction row without accepting zero or duplicate receipts.
fn one_prediction_receipt(
    receipts: Vec<AdjustmentShadowPredictionReceipt>,
) -> Result<AdjustmentShadowPredictionReceipt, ApiMaintenanceWriteAdmissionError> {
    let [receipt]: [AdjustmentShadowPredictionReceipt; 1] = receipts
        .try_into()
        .map_err(|_| ApiMaintenanceWriteAdmissionError::ReceiptCardinality)?;
    Ok(receipt)
}

/// Borrow the exact fixed-cycle clock from one admitted due-key shape.
fn due_clock(value: &str) -> Option<&str> {
    let clock = value.strip_prefix("capture/")?;
    let bytes = clock.as_bytes();
    // enforce the fixed millisecond UTC layout and four six-hour schedule slots
    if bytes.len() != 24
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || !matches!(&bytes[11..13], b"00" | b"06" | b"12" | b"18")
        || &bytes[13..] != b":35:00.000Z"
        || !bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'.',
            23 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        })
    {
        return None;
    }
    Some(clock)
}

/// Check one lowercase SHA-256 field without implementing a digest.
fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// Check one positive decimal PostgreSQL identity without numeric conversion.
fn positive_decimal(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'1'..=b'9')) && bytes.all(|byte| byte.is_ascii_digit())
}

/// Wrap one local registration admission failure.
const fn registration_admission<PortError>(
    error: ApiMaintenanceWriteAdmissionError,
) -> ApiMaintenanceRegistrationWriteError<PortError> {
    ApiMaintenanceRegistrationWriteError::Admission(error)
}

/// Wrap one local prediction admission failure.
const fn prediction_admission<PortError, ValuesError>(
    error: ApiMaintenanceWriteAdmissionError,
) -> ApiMaintenancePredictionWriteError<PortError, ValuesError> {
    ApiMaintenancePredictionWriteError::Admission(error)
}

#[cfg(test)]
mod tests {
    use weather_contracts::AdjustmentRevisionCommitReceiptContractVersionLiteral;

    use super::*;

    const DIGEST_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const DIGEST_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const DIGEST_C: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    const DIGEST_D: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    const DIGEST_E: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
    const DIGEST_F: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    const DIGEST_0: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const DIGEST_1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const DIGEST_2: &str = "2222222222222222222222222222222222222222222222222222222222222222";
    const DIGEST_3: &str = "3333333333333333333333333333333333333333333333333333333333333333";
    const DIGEST_4: &str = "4444444444444444444444444444444444444444444444444444444444444444";

    #[derive(Default)]
    struct FixturePort {
        registration_receipts: Vec<AdjustmentShadowRegistrationReceipt>,
        prediction_receipts: Vec<AdjustmentShadowPredictionReceipt>,
        sql: Option<&'static str>,
        json: Option<String>,
    }

    impl ApiMaintenancePredictionWritePort for FixturePort {
        type Error = &'static str;

        /// Capture one registration plan and return the configured decoded rows.
        fn register_api_shadow(
            &mut self,
            plan: ApiMaintenanceRegistrationPlan<'_>,
        ) -> Result<Vec<AdjustmentShadowRegistrationReceipt>, Self::Error> {
            let [ApiMaintenanceJsonBind::CanonicalJson(json)] = plan.binds;
            self.sql = Some(plan.sql);
            self.json = Some(json.to_owned());
            Ok(std::mem::take(&mut self.registration_receipts))
        }

        /// Capture one append plan and return the configured decoded rows.
        fn append_api_shadow(
            &mut self,
            plan: ApiMaintenancePredictionPlan<'_>,
        ) -> Result<Vec<AdjustmentShadowPredictionReceipt>, Self::Error> {
            let [ApiMaintenanceJsonBind::CanonicalJson(json)] = plan.binds;
            self.sql = Some(plan.sql);
            self.json = Some(json.to_owned());
            Ok(std::mem::take(&mut self.prediction_receipts))
        }
    }

    impl RainMaintenancePredictionWritePort for FixturePort {
        type Error = &'static str;

        /// Capture one rain registration plan and return the configured decoded rows.
        fn register_rain_shadow(
            &mut self,
            plan: ApiMaintenanceRegistrationPlan<'_>,
        ) -> Result<Vec<AdjustmentShadowRegistrationReceipt>, Self::Error> {
            let [ApiMaintenanceJsonBind::CanonicalJson(json)] = plan.binds;
            self.sql = Some(plan.sql);
            self.json = Some(json.to_owned());
            Ok(std::mem::take(&mut self.registration_receipts))
        }

        /// Capture one rain append plan and return the configured decoded rows.
        fn append_rain_shadow(
            &mut self,
            plan: ApiMaintenancePredictionPlan<'_>,
        ) -> Result<Vec<AdjustmentShadowPredictionReceipt>, Self::Error> {
            let [ApiMaintenanceJsonBind::CanonicalJson(json)] = plan.binds;
            self.sql = Some(plan.sql);
            self.json = Some(json.to_owned());
            Ok(std::mem::take(&mut self.prediction_receipts))
        }
    }

    struct FixtureValues;

    impl ApiMaintenancePredictionValues for FixtureValues {
        type Error = &'static str;

        /// Return the fixed chronological fixture clock values.
        fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error> {
            match value {
                "2026-01-01T00:35:00.000Z" => Ok(0),
                "2026-01-01T00:36:00.000Z" => Ok(1),
                "2026-01-01T01:00:00.000Z" => Ok(2),
                "2026-01-01T12:00:00.000Z" => Ok(3),
                "2026-01-01T00:37:00.000Z" => Ok(4),
                "2026-01-01T00:38:00.000Z" => Ok(5),
                _ => Err("unexpected clock"),
            }
        }

        /// Return one admitted compact fixture document.
        fn canonical_prediction_json(
            &mut self,
            _prediction: &AdjustmentShadowPrediction,
        ) -> Result<SerializedJson, Self::Error> {
            Ok(SerializedJson::admitted("{}"))
        }
    }

    #[test]
    fn registration_selects_version_and_binds_identity() {
        let mut port = FixturePort {
            registration_receipts: vec![AdjustmentShadowRegistrationReceipt {
                inserted: true,
                registrationSha256: DIGEST_A.to_owned(),
            }],
            ..FixturePort::default()
        };
        let registration = AdmittedApiMaintenanceRegistration {
            family: AdjustmentShadowFamily::Temperature,
            version: ApiMaintenanceRegistrationVersion::V3,
            registration_sha256: DIGEST_A,
            candidate_sha256: DIGEST_B,
            source_sha256: DIGEST_C,
            canonical_json: "{}",
        };

        let receipt = register_api_maintenance_shadow(&mut port, &registration)
            .expect("registration must succeed");

        assert!(receipt.inserted);
        assert_eq!(port.sql, Some(REGISTER_V3_SQL));
        assert_eq!(port.json.as_deref(), Some("{}"));
    }

    #[test]
    fn append_binds_wind_plan_and_nested_revision_receipt() {
        let prediction = prediction();
        let mut port = FixturePort {
            prediction_receipts: vec![prediction_receipt()],
            ..FixturePort::default()
        };

        let receipt = append_api_maintenance_shadow(
            &mut port,
            &mut FixtureValues,
            AdjustmentShadowFamily::Wind,
            &prediction,
        )
        .expect("append must succeed");

        assert!(receipt.inserted);
        assert_eq!(port.sql, Some(APPEND_WIND_SQL));
        assert_eq!(port.json.as_deref(), Some("{}"));
    }

    /// Verify the ingest role uses the shared registration and rain append binds.
    #[test]
    fn rain_ingest_role_binds_registration_and_append_plans() {
        let prediction = rain_prediction();
        let mut port = FixturePort {
            registration_receipts: vec![AdjustmentShadowRegistrationReceipt {
                inserted: true,
                registrationSha256: DIGEST_A.to_owned(),
            }],
            prediction_receipts: vec![prediction_receipt()],
            ..FixturePort::default()
        };
        let registration = rain_registration();

        register_rain_maintenance_shadow(&mut port, &registration)
            .expect("rain registration must succeed");
        assert_eq!(port.sql, Some(REGISTER_V2_SQL));
        assert_eq!(port.json.as_deref(), Some("{}"));

        append_rain_maintenance_shadow(&mut port, &mut FixtureValues, &prediction)
            .expect("rain append must succeed");
        assert_eq!(port.sql, Some(APPEND_RAIN_SQL));
        assert_eq!(port.json.as_deref(), Some("{}"));
    }

    /// Verify the API role refuses rain before either database method can run.
    #[test]
    fn api_role_rejects_rain_without_calling_its_port() {
        let prediction = rain_prediction();
        let mut port = FixturePort::default();

        assert_eq!(
            register_api_maintenance_shadow(&mut port, &rain_registration()),
            Err(ApiMaintenanceRegistrationWriteError::Admission(
                ApiMaintenanceWriteAdmissionError::UnsupportedFamily,
            ))
        );
        assert_eq!(
            append_api_maintenance_shadow(
                &mut port,
                &mut FixtureValues,
                AdjustmentShadowFamily::Rain,
                &prediction,
            ),
            Err(ApiMaintenancePredictionWriteError::Admission(
                ApiMaintenanceWriteAdmissionError::UnsupportedFamily,
            ))
        );
        assert_eq!(port.sql, None);
        assert_eq!(port.json, None);
    }

    /// Build one admitted rain registration fixture.
    fn rain_registration() -> AdmittedApiMaintenanceRegistration<'static> {
        AdmittedApiMaintenanceRegistration {
            family: AdjustmentShadowFamily::Rain,
            version: ApiMaintenanceRegistrationVersion::V2,
            registration_sha256: DIGEST_A,
            candidate_sha256: DIGEST_B,
            source_sha256: DIGEST_C,
            canonical_json: "{}",
        }
    }

    /// Build one fully admitted rain prediction fixture.
    fn rain_prediction() -> AdjustmentShadowPrediction {
        let mut prediction = prediction();
        prediction.bodyByteCount = 12_288;
        prediction.predictionSchemaSha256 = RAIN_SCHEMA_SHA256.to_owned();
        prediction.rowCount = 23;
        prediction
    }

    /// Build one fully admitted wind prediction fixture.
    fn prediction() -> AdjustmentShadowPrediction {
        AdjustmentShadowPrediction {
            bodyByteCount: 1_024,
            candidateSha256: DIGEST_A.to_owned(),
            dueKey: "capture/2026-01-01T00:35:00.000Z".to_owned(),
            inputSha256: DIGEST_B.to_owned(),
            issuedAt: "2026-01-01T00:36:00.000Z".to_owned(),
            maxValidAt: "2026-01-01T12:00:00.000Z".to_owned(),
            minValidAt: "2026-01-01T01:00:00.000Z".to_owned(),
            predictionBodySha256: DIGEST_C.to_owned(),
            predictionSchemaSha256: WIND_SCHEMA_SHA256.to_owned(),
            predictionSha256: DIGEST_D.to_owned(),
            registrationSha256: DIGEST_E.to_owned(),
            rowCount: 168,
            sourceReceiptSha256: DIGEST_F.to_owned(),
            sourceSha256: DIGEST_0.to_owned(),
            stageReceiptSha256: DIGEST_1.to_owned(),
        }
    }

    /// Build one exact outer and nested database receipt fixture.
    fn prediction_receipt() -> AdjustmentShadowPredictionReceipt {
        AdjustmentShadowPredictionReceipt {
            committedAt: "2026-01-01T00:37:00.000Z".to_owned(),
            inserted: true,
            predictionSha256: DIGEST_D.to_owned(),
            revisionReceipt: AdjustmentRevisionCommitReceipt {
                archiveCommitOrdinal: "1".to_owned(),
                archiveCommittedAt: "2026-01-01T00:38:00.000Z".to_owned(),
                contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
                frontierSha256: DIGEST_2.to_owned(),
                predecessorFrontierSha256: DIGEST_3.to_owned(),
                projectionIdentitySha256: DIGEST_F.to_owned(),
                projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::ShadowPrediction,
                projectionSha256: DIGEST_B.to_owned(),
                receiptSha256: DIGEST_4.to_owned(),
                stageReceiptSha256: DIGEST_1.to_owned(),
            },
        }
    }
}
