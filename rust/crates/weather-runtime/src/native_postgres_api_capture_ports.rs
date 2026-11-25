//! Native PostgreSQL cohort reads and private capture publication ownership.

use std::{
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use weather_api::{
    AdjustmentMaintenanceOperationError, AdmittedMaintenanceArchiveOrigin,
    DatabaseAdjustmentMaintenanceRepository,
};
use weather_contracts::{
    AdjustmentShadowPrediction, CaptureApiMaintenanceShadowResult, EcmwfTemperatureCanarySidecar,
    JsonValue, WeatherRecordRow,
};
use weather_database::{
    AdmittedEcmwfTemperatureCanarySidecarQuery, ApiMaintenancePredictionValues,
    EcmwfTemperatureCanaryReadError, ForecastReadPlan, SerializedJson,
    get_ecmwf_temperature_canary_sidecar,
};
use weather_edge::RevisionArchiveCanonicalCodec;

use crate::{
    ApiMaintenanceCaptureJob, ApiMaintenanceTemperatureReads, HttpApiMaintenanceArchiveRelay,
    NativeApiCaptureCodec, NativeApiMaintenanceArchiveHttpJson, NativeModelEvidenceValues,
    NativeOutboundHttp, NativePostgresError, NativePostgresPool, NativePostgresPrivateArchiveRelay,
    NativePrivatePostTransport, NativeWirePrimitives, PooledWeatherApiMaintenance,
    PooledWeatherApiReads, RetainedApiMaintenancePredictionPorts, WeatherApiMaintenanceCapturePool,
    native_json::NativeJson, read_api_forecast,
};

const MAXIMUM_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

/// Closed primitive failure while preparing one compact database prediction bind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeApiCaptureValueError {
    InvalidUtc,
    InvalidNumber,
    Encoding,
}

impl WeatherApiMaintenanceCapturePool for NativePostgresPool {
    /// Start both complete read sequences together on independent shared-pool handles.
    fn read_temperature_capture_cohort(
        &mut self,
        forecast: &ForecastReadPlan,
        sidecar: &AdmittedEcmwfTemperatureCanarySidecarQuery,
    ) -> Result<ApiMaintenanceTemperatureReads, Self::Error> {
        let forecast_pool = self.clone();
        let sidecar_pool = self.clone();
        let forecast = forecast.clone();
        let sidecar = sidecar.clone();
        let (forecast_rows, sidecar_read, first_error) = thread::scope(|scope| {
            let (sender, receiver) = mpsc::channel();
            let forecast_sender = sender.clone();
            let sidecar_sender = sender.clone();
            scope.spawn(move || {
                let mut reads = PooledWeatherApiReads::new(forecast_pool);
                let result = read_api_forecast(&mut reads, &forecast);
                let _ = forecast_sender.send(CohortResult::Forecast(result));
            });
            scope.spawn(move || {
                let mut reads = PooledWeatherApiReads::new(sidecar_pool);
                let result = get_ecmwf_temperature_canary_sidecar(&mut reads, &sidecar)
                    .map_err(sidecar_error);
                let _ = sidecar_sender.send(CohortResult::Sidecar(Box::new(result)));
            });
            drop(sender);

            let mut forecast_rows = None;
            let mut sidecar_read = None;
            let mut first_error = None;
            // drain both independently leased sequences while retaining the first reported error
            for _ in 0..2 {
                match receiver.recv() {
                    Ok(CohortResult::Forecast(Ok(rows))) => forecast_rows = Some(rows),
                    Ok(CohortResult::Sidecar(result)) => match *result {
                        Ok(sidecar) => sidecar_read = Some(sidecar),
                        Err(error) => {
                            // preserve the first reported failure while draining the other lane
                            if first_error.is_none() {
                                first_error = Some(error);
                            }
                        }
                    },
                    Ok(CohortResult::Forecast(Err(error))) => {
                        // retain the first completed failure without cancelling the other sequence
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                    Err(_) => {
                        // represent a disconnected scoped worker only when no earlier error won
                        if first_error.is_none() {
                            first_error = Some(NativePostgresError::RuntimeUnavailable);
                        }
                        break;
                    }
                }
            }
            (forecast_rows, sidecar_read, first_error)
        });
        // return only after both scoped query sequences have joined and released their leases
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(ApiMaintenanceTemperatureReads {
            best_match_rows: forecast_rows.ok_or_else(|| {
                NativePostgresError::decode("temperature forecast cohort", "result is missing")
            })?,
            sidecar: sidecar_read.ok_or_else(|| {
                NativePostgresError::decode("temperature sidecar cohort", "result is missing")
            })?,
        })
    }
}

/// One independently completed side of the temperature capture cohort.
enum CohortResult {
    Forecast(Result<Vec<WeatherRecordRow>, NativePostgresError>),
    Sidecar(Box<Result<Option<EcmwfTemperatureCanarySidecar>, NativePostgresError>>),
}

impl ApiMaintenancePredictionValues for NativeApiCaptureCodec {
    type Error = NativeApiCaptureValueError;

    /// Admit only the exact native canonical UTC millisecond grammar.
    fn utc_epoch_milliseconds(&mut self, value: &str) -> Result<i64, Self::Error> {
        NativeWirePrimitives
            .canonical_utc_milliseconds(value)
            .ok_or(NativeApiCaptureValueError::InvalidUtc)
    }

    /// Encode the predecessor's alphabetically sorted compact prediction object.
    fn canonical_prediction_json(
        &mut self,
        prediction: &AdjustmentShadowPrediction,
    ) -> Result<SerializedJson, Self::Error> {
        // preserve the generated safe integer domain before converting to JSON numbers
        if prediction.bodyByteCount > MAXIMUM_SAFE_INTEGER
            || prediction.rowCount > MAXIMUM_SAFE_INTEGER
        {
            return Err(NativeApiCaptureValueError::InvalidNumber);
        }
        let value = JsonValue::Object(vec![
            member("bodyByteCount", number(prediction.bodyByteCount)),
            member("candidateSha256", text(&prediction.candidateSha256)),
            member("dueKey", text(&prediction.dueKey)),
            member("inputSha256", text(&prediction.inputSha256)),
            member("issuedAt", text(&prediction.issuedAt)),
            member("maxValidAt", text(&prediction.maxValidAt)),
            member("minValidAt", text(&prediction.minValidAt)),
            member(
                "predictionBodySha256",
                text(&prediction.predictionBodySha256),
            ),
            member(
                "predictionSchemaSha256",
                text(&prediction.predictionSchemaSha256),
            ),
            member("predictionSha256", text(&prediction.predictionSha256)),
            member("registrationSha256", text(&prediction.registrationSha256)),
            member("rowCount", number(prediction.rowCount)),
            member("sourceReceiptSha256", text(&prediction.sourceReceiptSha256)),
            member("sourceSha256", text(&prediction.sourceSha256)),
            member("stageReceiptSha256", text(&prediction.stageReceiptSha256)),
        ]);
        let mut bytes =
            NativeJson::encode_ordered(&value).ok_or(NativeApiCaptureValueError::Encoding)?;
        // remove only the native archive encoder's required framing newline
        if bytes.pop() != Some(b'\n') {
            return Err(NativeApiCaptureValueError::Encoding);
        }
        String::from_utf8(bytes)
            .map(SerializedJson::admitted)
            .map_err(|_| NativeApiCaptureValueError::Encoding)
    }
}

/// Cloneable actual database and archive owner for one prepared capture job.
#[derive(Clone)]
pub struct NativeApiCapturePredictionOwner {
    pool: NativePostgresPool,
    archive_origin: AdmittedMaintenanceArchiveOrigin,
    client: Arc<NativeOutboundHttp>,
    archive_timeout: Duration,
}

impl NativeApiCapturePredictionOwner {
    /// Retain the existing process pool, shared HTTP owner, and admitted archive origin.
    pub const fn new(
        pool: NativePostgresPool,
        archive_origin: AdmittedMaintenanceArchiveOrigin,
        client: Arc<NativeOutboundHttp>,
        archive_timeout: Duration,
    ) -> Self {
        Self {
            pool,
            archive_origin,
            client,
            archive_timeout,
        }
    }

    /// Construct an independent relay over the shared native HTTP pool.
    pub fn relay(&self) -> NativePostgresPrivateArchiveRelay {
        let transport =
            NativePrivatePostTransport::new(Arc::clone(&self.client), self.archive_timeout);
        let archive =
            NativeApiMaintenanceArchiveHttpJson::new(NativeModelEvidenceValues::new(), transport);
        HttpApiMaintenanceArchiveRelay::new(self.archive_origin.clone(), archive)
    }

    /// Run one already-started job through actual database admission and archive effects.
    pub fn run(
        &self,
        job: ApiMaintenanceCaptureJob<'_>,
        codec: &mut NativeApiCaptureCodec,
    ) -> Result<CaptureApiMaintenanceShadowResult, AdjustmentMaintenanceOperationError> {
        let mut pool = self.pool.clone();
        let database = PooledWeatherApiMaintenance::from_shared_handle(&mut pool);
        let mut repository = DatabaseAdjustmentMaintenanceRepository::new(database);
        let mut values = NativeApiCaptureCodec;
        let mut relay = self.relay();
        let mut ports = RetainedApiMaintenancePredictionPorts {
            repository: &mut repository,
            values: &mut values,
            relay: &mut relay,
        };
        job.run(codec, &mut ports, &NativeModelEvidenceValues::new())
    }
}

/// Preserve the underlying query error and classify an impossible admitted-plan failure.
fn sidecar_error(
    error: EcmwfTemperatureCanaryReadError<NativePostgresError>,
) -> NativePostgresError {
    // retain actual driver failures without converting them to local decode diagnostics
    match error {
        EcmwfTemperatureCanaryReadError::Query(error) => error,
        EcmwfTemperatureCanaryReadError::Input(error) => {
            NativePostgresError::decode("temperature sidecar plan", error)
        }
    }
}

/// Build one ordered UTF-16 JSON object member.
fn member(name: &str, value: JsonValue) -> (Vec<u16>, JsonValue) {
    (name.encode_utf16().collect(), value)
}

/// Build one JSON string without changing its UTF-16 spelling.
fn text(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Build one already-admitted safe unsigned JSON number.
fn number(value: u64) -> JsonValue {
    JsonValue::Number(value as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the database prediction owner reproduces sorted compact JSON without framing LF.
    #[test]
    fn native_capture_prediction_json_is_sorted_and_compact() {
        let mut codec = NativeApiCaptureCodec;
        let prediction = AdjustmentShadowPrediction {
            bodyByteCount: 1,
            candidateSha256: "a".repeat(64),
            dueKey: "capture/2026-11-24T00:00:00.000Z".to_owned(),
            inputSha256: "b".repeat(64),
            issuedAt: "2026-11-24T00:35:00.000Z".to_owned(),
            maxValidAt: "2026-11-24T12:00:00.000Z".to_owned(),
            minValidAt: "2026-11-24T01:00:00.000Z".to_owned(),
            predictionBodySha256: "c".repeat(64),
            predictionSchemaSha256: "d".repeat(64),
            predictionSha256: "e".repeat(64),
            registrationSha256: "f".repeat(64),
            rowCount: 12,
            sourceReceiptSha256: "1".repeat(64),
            sourceSha256: "2".repeat(64),
            stageReceiptSha256: "3".repeat(64),
        };
        let encoded = codec
            .canonical_prediction_json(&prediction)
            .expect("prediction JSON");
        assert!(encoded.as_str().starts_with("{\"bodyByteCount\":1,"));
        assert!(
            encoded
                .as_str()
                .ends_with(&format!("\"stageReceiptSha256\":\"{}\"}}", "3".repeat(64)))
        );
        assert!(!encoded.as_str().ends_with('\n'));
    }
}
