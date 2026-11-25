use std::cell::Cell;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use weather_adjustment::{
    AdmittedTemperatureMosCalendar, AdmittedWindRowCalendar, AdmittedWindRowSourceIdentity,
    BorrowedTemperatureMosServingModel, WindCanaryAuthorizationWindow,
};
use weather_api::{
    AdmittedHistoryCursor, AdmittedPrivateRouteAuthority, AdmittedRainAdjustmentHour,
    AdmittedRainAdjustmentRun, AdmittedRainForecastRuntime, AdmittedTemperatureCanaryAuthorization,
    AdmittedTemperatureCanaryEvidence, AdmittedTemperatureCanaryRuntime,
    AdmittedTemperatureCanarySidecar, AdmittedTemperatureForecastServiceRuntime,
    AdmittedTemperatureMosModelOrigin, AdmittedTemperatureMosModelOwner,
    AdmittedWindCanaryEvidence, AdmittedWindCanaryRuntime, AdmittedWindForecastServiceRuntime,
    AdmittedWindModelOwner, AdmittedWindModelRow, CanonicalInstant, CurrentProjectionError,
    CurrentTimeResolver, DailyPrecipitationServiceBoundary, DatabaseStatus, DecodedServiceRequest,
    FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES, ForecastMaintenanceWindModelAdmissionBoundary,
    ForecastObservationGeometryBoundary, ForecastObservationInputResponseContractVersionLiteral,
    ForecastRainAdjustmentRead, ForecastSelection, ForecastServiceBoundary,
    ForecastTemperatureAdjustmentBoundary, ForecastTemperatureMosAdmissionBoundary,
    ForecastTemperatureSidecarRead, ForecastTemperatureStatusRead, ForecastWindAdjustmentBoundary,
    ForecastWindModelAdmissionBoundary, HealthSnapshot, HistoryBoundaryAdapter,
    HistoryBoundaryError, HistoryQueryTime, InactiveForecastServiceRuntime,
    LegacyTemperatureForecastStartup, LegacyWindForecastStartup,
    MaintenanceTemperatureForecastStartup, MaintenanceWindForecastStartup,
    MaintenanceWindModelCalendar, MigrationStatus, PrivateServiceListenerOwner, QueryParameter,
    RainCollectionTimestampBoundary, RepositoryWindModelAdmission,
    ResolvedDailyPrecipitationWindow, ResolvedForecastObservationGeometry,
    ResolvedForecastPressureContextWindow, ResolvedForecastReadWindow,
    ResolvedServiceForecastWindow, ServiceClockError, ServiceRepositoryError,
    StandardForecastAdjustmentStartup, TideBoundaryAdapter, TideBoundaryError, TideQueryTime,
    TrendTimeBoundary, WeatherApiStartupSnapshots, WeatherServiceBody, WeatherServiceBoundaries,
    WeatherServiceClock, WeatherServiceRepository, WeatherServiceWireEncoder,
    WindForecastRowAdmission, serve_decoded_request, serve_private_decoded_request,
    serve_private_wire_decoded_request, serve_wire_decoded_request,
    serve_wire_decoded_request_with_rain_diagnostics,
};
use weather_contracts::{
    ApiForecastAdjustmentDecision, ApiForecastAdjustmentRuntimeActivationModeEnum,
    ApiForecastAdjustmentRuntimeReasonCodeEnum, ApiForecastAdjustmentRuntimeStateEnum,
    ApiForecastTemperatureAdjustmentRuntimeActivationModeEnum,
    ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum,
    ApiForecastTemperatureAdjustmentRuntimeStateEnum, ApiHealthResponseDataDatabaseEnum,
    ApiHealthResponseDataMigrationStatusEnum, ApiHealthResponseDataWorkerFreshnessEnum,
    ApiRainAdjustmentDecisionStateEnum, ApiRainAdjustmentRuntimeReasonCodeEnum,
    ApiRainAdjustmentRuntimeStateEnum, ApplyTemperatureCanaryInputV1, CurrentWeatherRecordRow,
    DailyPrecipitationRow, EcmwfTemperatureCanaryHour, EcmwfTemperatureCanaryRecentErrorState,
    EcmwfTemperatureCanaryRecentErrorStateCohortLiteral, EcmwfTemperatureCanaryRun,
    EcmwfTemperatureCanaryRunAdapterVersionLiteral, EcmwfTemperatureCanaryRunModelCycleEnum,
    EcmwfTemperatureCanaryRunStateStatusEnum, EcmwfTemperatureCanaryRunUpstreamModelLiteral,
    EcmwfTemperatureCanarySidecar, EcmwfTemperatureCanaryStatus,
    EcmwfTemperatureCanaryStatusStateStatusEnum, ForecastAdjustmentCoefficient,
    ForecastAdjustmentCoefficientDaypartEnum, ForecastAdjustmentCoefficientLevelEnum,
    ForecastAdjustmentCoefficientSeasonEnum, ForecastAdjustmentDecision,
    ForecastAdjustmentFailRawDecisionStateEnum, ForecastAdjustmentForecastIdentity,
    ForecastAdjustmentForecastIdentityCohortLiteral,
    ForecastAdjustmentForecastIdentityReferenceKindLiteral, ForecastAdjustmentMetric,
    ForecastAdjustmentMetricBand, ForecastAdjustmentRawForecastProvenance,
    ForecastAdjustmentRawForecastProvenanceCohortLiteral,
    ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral, ForecastAdjustmentReasonCode,
    ForecastAdjustmentStartupSnapshot, ForecastAdjustmentTrainingEnvelope,
    ForecastAdjustmentTrainingEnvelopeMetricEnum, ForecastAdjustmentTrainingProvenance,
    ForecastAdjustmentWindCanaryAuthorizationV1,
    ForecastAdjustmentWindCanaryAuthorizationV1ArtifactKindLiteral,
    ForecastAdjustmentWindCanaryAuthorizationV1AuthorizedLiteral,
    ForecastAdjustmentWindCanaryAuthorizationV1ContractVersionLiteral,
    ForecastAdjustmentWindCanaryAuthorizationV2,
    ForecastAdjustmentWindCanaryAuthorizationV2ArtifactKindLiteral,
    ForecastAdjustmentWindCanaryAuthorizationV2AuthorizedLiteral,
    ForecastAdjustmentWindCanaryAuthorizationV2ContractVersionLiteral,
    ForecastAdjustmentWindCanaryAuthorizationV2ExpiresAtNull,
    ForecastAdjustmentWindCanaryAuthorizationV2PermanentLiteral,
    ForecastAdjustmentWindCanaryCandidateV1,
    ForecastAdjustmentWindCanaryCandidateV1AlgorithmContractVersionLiteral,
    ForecastAdjustmentWindCanaryCandidateV1ArtifactKindLiteral,
    ForecastAdjustmentWindCanaryCandidateV1ContractVersionLiteral,
    ForecastAdjustmentWindCanaryCandidateV1RuntimeFingerprint,
    ForecastAdjustmentWindCanaryCandidateV1SiteKeyLiteral,
    ForecastAdjustmentWindCanaryCandidateV1TimezoneLiteral, ForecastAdjustmentWindCanaryMetric,
    ForecastAdjustmentWindCanaryRuntimeBundle, ForecastAdjustmentWindCanaryRuntimeBundleV1,
    ForecastAdjustmentWindCanaryRuntimeBundleV1ArtifactKindLiteral,
    ForecastAdjustmentWindCanaryRuntimeBundleV1ContractVersionLiteral,
    ForecastAdjustmentWindCanaryRuntimeBundleV1SiteKeyLiteral,
    ForecastAdjustmentWindCanaryRuntimeBundleV1TimezoneLiteral,
    ForecastAdjustmentWindCanaryRuntimeBundleV2,
    ForecastAdjustmentWindCanaryRuntimeBundleV2ContractVersionLiteral,
    ForecastAdjustmentWindCanaryTrainingIdentity,
    ForecastAdjustmentWindCanaryTrainingIdentityCohortLiteral,
    ForecastAdjustmentWindCanaryTrainingIdentityDatasetLiteral,
    ForecastAdjustmentWindCanaryTrainingIdentityReferenceKindLiteral,
    ForecastAdjustmentWindCanaryTrainingIdentityUpstreamModelLiteral,
    ForecastAdjustmentWindCanaryTransferReportV1,
    ForecastAdjustmentWindCanaryTransferReportV1ArtifactKindLiteral,
    ForecastAdjustmentWindCanaryTransferReportV1ContractVersionLiteral,
    ForecastAdjustmentWindCanaryTransferReportV1PassedLiteral, ForecastDays, ForecastLeadBandKey,
    ForecastObservationInput, ForecastObservationInputRow,
    ForecastObservationInputRowSourceKindEnum, ForecastObservationInputSiteSlugLiteral,
    ForecastObservationInputTimezoneLiteral, ForecastRainAdjustmentStartupSnapshot,
    ForecastTemperatureAdjustmentStartupSnapshot, ForecastTemperatureCanaryDecisionV1,
    ForecastTemperatureCanaryDecisionV1BranchEnum,
    ForecastTemperatureCanaryDecisionV1ContractVersionLiteral,
    ForecastTemperatureCanaryDecisionV1SourceForecast,
    ForecastTemperatureCanaryDecisionV1StateEnum, HistoryQueryCursor, JsDate, JsDateOrString,
    JsonValue, LoadedApiForecastAdjustmentRuntime, LoadedForecastAdjustmentRainRuntimeRegistryV1,
    LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveArtifactSha256Literal,
    LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledArtifactSha256Null,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledStateLiteral,
    LoadedForecastAdjustmentRuntimeV1, LoadedForecastAdjustmentRuntimeV1Disabled,
    LoadedForecastAdjustmentRuntimeV1DisabledBundleNull,
    LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentRuntimeV1DisabledStateLiteral,
    LoadedForecastAdjustmentTemperatureCanaryRuntime,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Disabled,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledBundleNull,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum,
    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledStateLiteral,
    LoadedForecastAdjustmentWindCanaryRuntime, LoadedForecastAdjustmentWindCanaryRuntimeV1,
    LoadedForecastAdjustmentWindCanaryRuntimeV1Active,
    LoadedForecastAdjustmentWindCanaryRuntimeV1ActiveReasonCodeNull,
    LoadedForecastAdjustmentWindCanaryRuntimeV1ActiveStateLiteral, Nullable, RainCollectionStatus,
    RainCollectionStatusContractVersionLiteral, RainCollectionStatusModelEnabledLiteral,
    RainCollectionStatusQualificationEnabledLiteral, SourceKind, StationKind,
    TemperatureCanaryDecisionReasonValue, TemperatureCanarySourceForecast,
    TemperatureMosRuntimeModel, TemperatureMosRuntimeModelContractVersionLiteral,
    TemperatureMosRuntimeModelMaterialCohortLiteral,
    TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral,
    TemperatureMosRuntimeModelMaterialScopeEnum, TemperatureMosRuntimeModelMaterialStrengthBands,
    TemperatureMosRuntimeStrengthBand, TemperatureMosServingModel, TideRecordRow,
    TideRecordRowSourceKindEnum, TrendPointRow, WeatherRecordRow,
};
use weather_database::{
    ActiveSiteRow, CurrentWeatherBind, CurrentWeatherQueryPlan, DailyPrecipitationBind,
    DailyPrecipitationReadPlan, ForecastObservationInputBind, ForecastObservationInputReadPlan,
    ForecastPressureContextBind, ForecastPressureContextPlan, ForecastReadBind, ForecastReadPlan,
    HistoryReadBind, HistoryReadPlan, ListActiveSitesQuery, TideReadBind, TideReadPlan,
    TrendReadBind, TrendReadPlan,
};

struct FixtureBoundary;

impl CurrentTimeResolver for FixtureBoundary {
    /// resolve the admitted service fixture instants
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        // retain only source-derived timestamp pairs used by integrated projections
        for (source, epoch) in [
            ("2026-07-25T07:00:00.000Z", 1_784_962_800_000),
            ("2026-08-01T07:00:00.000Z", 1_785_567_600_000),
            ("2026-08-20T05:00:00.000Z", 1_787_202_000_000),
            ("2026-08-21T04:00:00.000Z", 1_787_284_800_000),
            ("2026-08-21T07:00:00.000Z", 1_787_295_600_000),
            ("2026-08-21T11:00:00.000Z", 1_787_310_000_000),
            ("2026-08-21T18:00:00.000Z", 1_787_335_200_000),
            ("2026-08-22T00:05:00.000Z", 1_787_357_100_000),
            ("2026-08-22T04:00:00.000Z", 1_787_371_200_000),
            ("2026-08-22T04:59:00.000Z", 1_787_374_740_000),
            ("2026-08-22T05:00:00.000Z", 1_787_374_800_000),
            ("2026-08-22T06:00:00.000Z", 1_787_378_400_000),
            ("2026-08-22T11:27:00.000Z", 1_787_398_020_000),
            ("2026-08-22T13:06:00.000Z", 1_787_403_960_000),
            ("2026-08-24T05:00:00.000Z", 1_787_547_600_000),
        ] {
            // compare the generated UTF-16 storage representation exactly
            if value == &JsDateOrString::String(text(source))
                || value == &JsDateOrString::DateEpochMilliseconds(epoch)
            {
                return Ok(instant(epoch, source));
            }
        }

        Err(CurrentProjectionError {
            code: "service.time",
            detail: "fixture time boundary rejected the repository timestamp".to_owned(),
        })
    }
}

struct RepositoryWindAdmissionTime;

impl CurrentTimeResolver for RepositoryWindAdmissionTime {
    /// normalize the exact original offset timestamp and bounded lead fixtures
    fn resolve(&self, value: &JsDateOrString) -> Result<CanonicalInstant, CurrentProjectionError> {
        let JsDateOrString::String(value) = value else {
            return Err(CurrentProjectionError {
                code: "fixture.repository_wind_time",
                detail: "repository wind fixture requires a string timestamp".to_owned(),
            });
        };
        let value = String::from_utf16(value).map_err(|_| CurrentProjectionError {
            code: "fixture.repository_wind_time",
            detail: "repository wind fixture timestamp is invalid UTF-16".to_owned(),
        })?;
        let resolved = match value.as_str() {
            "2026-09-08T10:45:00-07:00" => instant(1_788_889_500_000, "2026-09-08T17:45:00.000Z"),
            "2026-09-08T17:44:59.999Z" => instant(1_788_889_499_999, "2026-09-08T17:44:59.999Z"),
            "2026-09-09T06:00:00.000Z" => instant(1_788_933_600_000, "2026-09-09T06:00:00.000Z"),
            "2026-09-15T17:45:00.000Z" => instant(1_789_494_300_000, "2026-09-15T17:45:00.000Z"),
            "2026-09-15T18:45:00.000Z" => instant(1_789_497_900_000, "2026-09-15T18:45:00.000Z"),
            _ => {
                return Err(CurrentProjectionError {
                    code: "fixture.repository_wind_time",
                    detail: "repository wind fixture rejected an unknown timestamp".to_owned(),
                });
            }
        };
        Ok(resolved)
    }
}

struct RecordingWindCalendar {
    calendar_calls: AtomicUsize,
    fingerprint_calls: AtomicUsize,
    valid_at: Mutex<Vec<String>>,
}

impl RecordingWindCalendar {
    /// create one empty recording adapter for repository-row admission tests
    fn new() -> Self {
        Self {
            calendar_calls: AtomicUsize::new(0),
            fingerprint_calls: AtomicUsize::new(0),
            valid_at: Mutex::new(Vec::new()),
        }
    }
}

impl MaintenanceWindModelCalendar for RecordingWindCalendar {
    /// return the source calendar while retaining the exact normalized valid instant
    fn calendar(&self, valid_at: &str) -> Result<AdmittedWindRowCalendar, CurrentProjectionError> {
        self.calendar_calls.fetch_add(1, Ordering::Relaxed);
        self.valid_at
            .lock()
            .expect("recording wind calendar lock")
            .push(valid_at.to_owned());
        Ok(AdmittedWindRowCalendar {
            daypart: ForecastAdjustmentCoefficientDaypartEnum::Night,
            month: 9,
            season: ForecastAdjustmentCoefficientSeasonEnum::Summer,
        })
    }

    /// accept only the fitted runtime fingerprint used by both wind candidates
    fn fingerprint_matches(&self, icu_version: &str, tzdata_version: &str) -> bool {
        self.fingerprint_calls.fetch_add(1, Ordering::Relaxed);
        icu_version == "78.3" && tzdata_version == "2026b"
    }
}

impl ForecastServiceBoundary for FixtureBoundary {
    /// resolve the source-derived one-day service windows
    fn resolve_window(
        &self,
        selection: ForecastSelection,
        data_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<ResolvedServiceForecastWindow, CurrentProjectionError> {
        assert_eq!(selection, ForecastSelection::Daily { days: 1 });
        assert_eq!(data_at.utc, "2026-08-22T05:00:00.000Z");
        assert_eq!(timezone, "America/Los_Angeles");
        Ok(ResolvedServiceForecastWindow {
            public_start: instant(1_787_295_600_000, "2026-08-21T07:00:00.000Z"),
            public_end: instant(1_787_382_000_000, "2026-08-22T07:00:00.000Z"),
            repository: ResolvedForecastReadWindow {
                as_of: instant(1_787_252_400_000, "2026-08-20T19:00:00.000Z"),
                end_exclusive: instant(1_787_385_600_000, "2026-08-22T08:00:00.000Z"),
                hours: 37,
            },
            pressure_context: ResolvedForecastPressureContextWindow {
                as_of: instant(1_787_295_600_000, "2026-08-21T07:00:00.000Z"),
                context_start: instant(1_787_284_800_000, "2026-08-21T04:00:00.000Z"),
                context_end: instant(1_787_306_400_000, "2026-08-21T10:00:00.000Z"),
                oldest_run: instant(1_787_122_800_000, "2026-08-19T07:00:00.000Z"),
            },
        })
    }
}

impl ForecastObservationGeometryBoundary for FixtureBoundary {
    /// resolve the source-derived fixed private projection geometry
    fn resolve_expected_geometry(
        &self,
        data_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<ResolvedForecastObservationGeometry, CurrentProjectionError> {
        assert_eq!(data_at.utc, "2026-08-22T05:00:00.000Z");
        assert_eq!(timezone, "America/Los_Angeles");
        Ok(ResolvedForecastObservationGeometry {
            projection_from: instant(1_787_295_600_000, "2026-08-21T07:00:00.000Z"),
            query_from: instant(1_787_284_800_000, "2026-08-21T04:00:00.000Z"),
        })
    }
}

impl DailyPrecipitationServiceBoundary for FixtureBoundary {
    /// resolve the exact source-derived local-day window
    fn resolve_window(
        &self,
        data_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<ResolvedDailyPrecipitationWindow, CurrentProjectionError> {
        assert_eq!(data_at.utc, "2026-08-22T05:00:00.000Z");
        assert_eq!(timezone, "America/Los_Angeles");
        Ok(ResolvedDailyPrecipitationWindow {
            from: instant(1_787_295_600_000, "2026-08-21T07:00:00.000Z"),
            to: data_at.clone(),
        })
    }
}

impl RainCollectionTimestampBoundary for FixtureBoundary {
    /// validate only the source-derived canonical collection timestamps
    fn validate_canonical_utc(&self, value: &str) -> Result<(), CurrentProjectionError> {
        // preserve the exact retained receipt timestamp fixtures
        if matches!(
            value,
            "2026-09-13T01:00:00.000Z"
                | "2026-09-13T01:01:00.000Z"
                | "2026-09-13T01:02:00.000Z"
                | "2026-09-13T01:03:00.000Z"
                | "2026-09-14T00:00:00.000Z"
        ) {
            return Ok(());
        }

        Err(CurrentProjectionError {
            code: "service.rain_collection_time",
            detail: "fixture rejected the collection timestamp".to_owned(),
        })
    }
}

impl TrendTimeBoundary for FixtureBoundary {
    /// resolve the fixed local-calendar trend start
    fn calendar_start(
        &self,
        generated_at: &CanonicalInstant,
        timezone: &str,
    ) -> Result<CanonicalInstant, CurrentProjectionError> {
        assert_eq!(generated_at.utc, "2026-08-22T05:00:00.000Z");
        assert_eq!(timezone, "America/Los_Angeles");
        Ok(instant(1_546_329_600_000, "2019-01-01T08:00:00.000Z"))
    }

    /// resolve one typed trend row instant
    fn resolve_row_time(&self, value: &JsDate) -> Result<CanonicalInstant, CurrentProjectionError> {
        // admit only the source-derived fixture bucket
        match value {
            JsDate::EpochMilliseconds(1_787_295_600_000) => {
                Ok(instant(1_787_295_600_000, "2026-08-21T07:00:00.000Z"))
            }
            _ => Err(CurrentProjectionError {
                code: "service.trends_time",
                detail: "fixture trend boundary rejected the repository timestamp".to_owned(),
            }),
        }
    }
}

impl HistoryBoundaryAdapter for FixtureBoundary {
    /// reject unneeded submitted history times in this integration fixture
    fn resolve_query_time(
        &self,
        _value: &str,
        _field: HistoryQueryTime,
    ) -> Result<CanonicalInstant, HistoryBoundaryError> {
        Err(HistoryBoundaryError)
    }

    /// reject unneeded opaque cursors in this integration fixture
    fn decode_cursor(&self, _value: &str) -> Result<AdmittedHistoryCursor, HistoryBoundaryError> {
        Err(HistoryBoundaryError)
    }

    /// reject an unexpected continuation cursor for an empty page
    fn encode_cursor(
        &self,
        _cursor: &HistoryQueryCursor,
    ) -> Result<String, CurrentProjectionError> {
        Err(CurrentProjectionError {
            code: "service.cursor",
            detail: "empty fixture must not encode a continuation".to_owned(),
        })
    }
}

impl TideBoundaryAdapter for FixtureBoundary {
    /// reject unneeded submitted tide bounds in this integration fixture
    fn resolve_query_time(
        &self,
        _value: &str,
        _field: TideQueryTime,
    ) -> Result<CanonicalInstant, TideBoundaryError> {
        Err(TideBoundaryError)
    }

    /// resolve the exact Node default tide offsets
    fn offset_data_time(
        &self,
        data_at: &CanonicalInstant,
        offset_milliseconds: i64,
        field: TideQueryTime,
    ) -> Result<CanonicalInstant, TideBoundaryError> {
        assert_eq!(data_at.utc, "2026-08-22T05:00:00.000Z");

        // preserve the one-day history and ten-day forecast defaults
        match (offset_milliseconds, field) {
            (-86_400_000, TideQueryTime::From) => {
                Ok(instant(1_787_288_400_000, "2026-08-21T05:00:00.000Z"))
            }
            (864_000_000, TideQueryTime::To) => {
                Ok(instant(1_788_238_800_000, "2026-09-01T05:00:00.000Z"))
            }
            _ => Err(TideBoundaryError),
        }
    }
}

struct FixtureClock {
    reads: usize,
}

impl WeatherServiceClock for FixtureClock {
    /// return the one canonical request instant and count reads
    fn request_time(&mut self) -> Result<CanonicalInstant, ServiceClockError> {
        self.reads += 1;
        Ok(instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z"))
    }
}

struct PrivateFixtureClock {
    reads: usize,
}

impl WeatherServiceClock for PrivateFixtureClock {
    /// return a safety time distinct from the nominal private data time
    fn request_time(&mut self) -> Result<CanonicalInstant, ServiceClockError> {
        self.reads += 1;
        Ok(instant(1_787_461_200_000, "2026-08-23T05:00:00.000Z"))
    }
}

struct StaticFixtureClock {
    now: CanonicalInstant,
    reads: usize,
}

impl WeatherServiceClock for StaticFixtureClock {
    /// return one explicitly selected request safety instant
    fn request_time(&mut self) -> Result<CanonicalInstant, ServiceClockError> {
        self.reads += 1;
        Ok(self.now.clone())
    }
}

struct FixtureWindAdmission {
    calls: Cell<usize>,
    fail_public: bool,
    identity_mismatch: bool,
}

impl ForecastWindModelAdmissionBoundary for FixtureWindAdmission {
    /// admit the Node-derived calendar, provenance, and source guard result
    fn admit(
        &self,
        candidate: &ForecastAdjustmentWindCanaryCandidateV1,
        row: &WeatherRecordRow,
    ) -> Result<WindForecastRowAdmission, CurrentProjectionError> {
        assert_eq!(
            candidate.candidateArtifactSha256,
            active_wind_runtime().evidence.candidate_artifact_sha256
        );
        // count every model admission after raw row projection has completed
        self.calls.set(self.calls.get() + 1);

        // isolate one admitted row-material failure without losing sibling rows
        if self.fail_public && row.id == "forecast-public" {
            return Err(CurrentProjectionError {
                code: "fixture.wind",
                detail: "fixture wind admission failed".to_owned(),
            });
        }

        let valid_at = match &row.validAt {
            JsDateOrString::String(value) => String::from_utf16(value).expect("fixture UTF-16"),
            JsDateOrString::DateEpochMilliseconds(_) | JsDateOrString::InvalidDate => {
                panic!("fixture row must use a string")
            }
        };
        Ok(WindForecastRowAdmission::Admitted(AdmittedWindModelRow {
            calendar: AdmittedWindRowCalendar {
                daypart: ForecastAdjustmentCoefficientDaypartEnum::Night,
                month: 8,
                season: ForecastAdjustmentCoefficientSeasonEnum::Summer,
            },
            calendar_fingerprint_matches: true,
            raw_forecast_provenance: ForecastAdjustmentRawForecastProvenance {
                adapterVersion: "open-meteo-forecast-daily/v4".to_owned(),
                cohort: ForecastAdjustmentRawForecastProvenanceCohortLiteral,
                contractEpoch: "v4".to_owned(),
                dataset: "forecast".to_owned(),
                referenceAt: "2026-08-21T07:00:00.000Z".to_owned(),
                referenceKind: ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral,
                sourceConfigFingerprint: "fingerprint".to_owned(),
                sourceKey: row.sourceKey.clone(),
                targetLeadHours: 21,
                upstreamModel: "best_match".to_owned(),
                validAt: valid_at,
            },
            // expose the exact admitted whole-row source guard result
            source_identity: if self.identity_mismatch {
                AdmittedWindRowSourceIdentity::IdentityMismatch
            } else {
                AdmittedWindRowSourceIdentity::Match
            },
        }))
    }
}

impl ForecastMaintenanceWindModelAdmissionBoundary for FixtureWindAdmission {
    /// admit the same original row facts for the maintenance-qualified V2 model
    fn admit(
        &self,
        candidate: &weather_contracts::ForecastAdjustmentCandidateV2,
        row: &WeatherRecordRow,
    ) -> Result<WindForecastRowAdmission, CurrentProjectionError> {
        assert_eq!(candidate.candidateArtifactSha256, "b".repeat(64));
        // count every model admission after raw row projection has completed
        self.calls.set(self.calls.get() + 1);

        // isolate one admitted row-material failure without losing sibling rows
        if self.fail_public && row.id == "forecast-public" {
            return Err(CurrentProjectionError {
                code: "fixture.maintenance_wind",
                detail: "fixture maintenance wind admission failed".to_owned(),
            });
        }

        let valid_at = match &row.validAt {
            JsDateOrString::String(value) => String::from_utf16(value).expect("fixture UTF-16"),
            JsDateOrString::DateEpochMilliseconds(_) | JsDateOrString::InvalidDate => {
                panic!("fixture row must use a string")
            }
        };
        Ok(WindForecastRowAdmission::Admitted(AdmittedWindModelRow {
            calendar: AdmittedWindRowCalendar {
                daypart: ForecastAdjustmentCoefficientDaypartEnum::Night,
                month: 8,
                season: ForecastAdjustmentCoefficientSeasonEnum::Summer,
            },
            calendar_fingerprint_matches: true,
            raw_forecast_provenance: ForecastAdjustmentRawForecastProvenance {
                adapterVersion: "open-meteo-forecast-daily/v4".to_owned(),
                cohort: ForecastAdjustmentRawForecastProvenanceCohortLiteral,
                contractEpoch: "v4".to_owned(),
                dataset: "forecast".to_owned(),
                referenceAt: "2026-08-21T07:00:00.000Z".to_owned(),
                referenceKind: ForecastAdjustmentRawForecastProvenanceReferenceKindLiteral,
                sourceConfigFingerprint: "fingerprint".to_owned(),
                sourceKey: row.sourceKey.clone(),
                targetLeadHours: 21,
                upstreamModel: "best_match".to_owned(),
                validAt: valid_at,
            },
            // expose the exact admitted whole-row source guard result
            source_identity: if self.identity_mismatch {
                AdmittedWindRowSourceIdentity::IdentityMismatch
            } else {
                AdmittedWindRowSourceIdentity::Match
            },
        }))
    }
}

#[derive(Debug, Clone, Copy)]
enum FixtureTemperatureMode {
    Active,
    ApplicationFailure,
}

struct FixtureTemperatureAdjustment {
    mode: FixtureTemperatureMode,
}

struct FixtureTemperatureMosAdmission;

impl ForecastTemperatureMosAdmissionBoundary for FixtureTemperatureMosAdmission {
    /// admit the exact Node-derived source cycle and pre-resolved calendar fixture
    fn admit_calendar(
        &self,
        source: &TemperatureCanarySourceForecast,
        run_initialized_at: &CanonicalInstant,
        valid_at: &CanonicalInstant,
    ) -> Result<AdmittedTemperatureMosCalendar, CurrentProjectionError> {
        assert_eq!(
            source.modelCycle,
            weather_contracts::TemperatureCanarySourceForecastModelCycleEnum::Value50r1
        );
        assert_eq!(run_initialized_at.utc, "2026-08-21T18:00:00.000Z");
        assert_eq!(valid_at.utc, "2026-08-22T06:00:00.000Z");
        Ok(AdmittedTemperatureMosCalendar {
            annual_basis: [0.0, 1.0, 0.0, 1.0],
            cycle_basis: [0.0, 1.0],
            daily_basis: [0.0, 1.0, 0.0, 1.0],
        })
    }

    /// Preserve the source-derived local forecast month and locked embargo boundary.
    fn local_date_hour(
        &self,
        utc: &CanonicalInstant,
    ) -> Result<(String, u8), CurrentProjectionError> {
        match utc.utc.as_str() {
            "2026-08-01T07:00:00.000Z" => Ok(("2026-08-01".to_owned(), 0)),
            "2026-08-22T06:00:00.000Z" => Ok(("2026-08-21".to_owned(), 23)),
            _ => Err(CurrentProjectionError {
                code: "service.temperature_calendar",
                detail: "fixture local model clock was not admitted".to_owned(),
            }),
        }
    }
}

impl ForecastTemperatureAdjustmentBoundary for FixtureTemperatureAdjustment {
    /// apply one deterministic fixture decision through the generated input contract
    fn apply(
        &self,
        runtime: &AdmittedTemperatureCanaryRuntime,
        input: &ApplyTemperatureCanaryInputV1,
        recent_error_state_sha256: Option<&str>,
    ) -> Result<ForecastTemperatureCanaryDecisionV1, CurrentProjectionError> {
        assert_eq!(input.evaluatedAt, "2026-08-22T05:00:00.000Z");
        assert_eq!(runtime.maximum_receipt_age_hours, 12);
        assert_eq!(runtime.source_delay_hours, 6);

        // isolate the selected row's optional inference exception
        if matches!(self.mode, FixtureTemperatureMode::ApplicationFailure)
            && input.validAt == "2026-08-22T06:00:00.000Z"
        {
            return Err(CurrentProjectionError {
                code: "fixture.temperature",
                detail: "fixture temperature inference failed".to_owned(),
            });
        }

        let Nullable::Value(source) = &input.sourceForecast else {
            return Ok(temperature_raw_fallback(
                input,
                TemperatureCanaryDecisionReasonValue::MissingSourceForecast,
                Nullable::Value(runtime.evidence.active_bundle_sha256.clone()),
            ));
        };
        assert_eq!(
            recent_error_state_sha256,
            Some(temperature_recent_state_sha256())
        );
        assert_eq!(source.firstReceivedAt, "2026-08-22T00:05:00.000Z");
        assert_eq!(source.runInitializedAt, "2026-08-21T18:00:00.000Z");
        assert_eq!(source.modelLeadHours, 12);
        assert_eq!(source.rawTemperatureC, 11.0);

        Ok(ForecastTemperatureCanaryDecisionV1 {
            branch: Nullable::Value(ForecastTemperatureCanaryDecisionV1BranchEnum::Direct),
            bundleSha256: Nullable::Value(runtime.evidence.active_bundle_sha256.clone()),
            contractVersion: ForecastTemperatureCanaryDecisionV1ContractVersionLiteral,
            correctedTemperatureC: Nullable::Value(12.0),
            rawBestMatchTemperatureC: input.rawBestMatchTemperatureC.clone(),
            reasonCode: Nullable::Null,
            recentErrorStateSha256: Nullable::Value("e".repeat(64)),
            sourceForecast: Nullable::Value(temperature_decision_source(source, runtime)),
            state: ForecastTemperatureCanaryDecisionV1StateEnum::Active,
        })
    }
}

/// build one exact raw-fallback temperature decision
fn temperature_raw_fallback(
    input: &ApplyTemperatureCanaryInputV1,
    reason: TemperatureCanaryDecisionReasonValue,
    bundle_sha256: Nullable<String>,
) -> ForecastTemperatureCanaryDecisionV1 {
    ForecastTemperatureCanaryDecisionV1 {
        branch: Nullable::Null,
        bundleSha256: bundle_sha256,
        contractVersion: ForecastTemperatureCanaryDecisionV1ContractVersionLiteral,
        correctedTemperatureC: Nullable::Null,
        rawBestMatchTemperatureC: input.rawBestMatchTemperatureC.clone(),
        reasonCode: Nullable::Value(reason),
        recentErrorStateSha256: Nullable::Null,
        sourceForecast: Nullable::Null,
        state: ForecastTemperatureCanaryDecisionV1StateEnum::RawFallback,
    }
}

/// copy one admitted input source into the response and append its operational horizon
fn temperature_decision_source(
    source: &TemperatureCanarySourceForecast,
    runtime: &AdmittedTemperatureCanaryRuntime,
) -> ForecastTemperatureCanaryDecisionV1SourceForecast {
    ForecastTemperatureCanaryDecisionV1SourceForecast {
        adapterVersion: source.adapterVersion.clone(),
        dataset: source.dataset,
        firstReceivedAt: source.firstReceivedAt.clone(),
        modelCycle: source.modelCycle,
        modelLeadHours: source.modelLeadHours,
        providerKey: source.providerKey,
        providerResponseSha256: source.providerResponseSha256.clone(),
        rawRelativeHumidityPercent: source.rawRelativeHumidityPercent.clone(),
        rawTemperatureC: source.rawTemperatureC,
        rawWindSpeedMps: source.rawWindSpeedMps.clone(),
        runInitializedAt: source.runInitializedAt.clone(),
        upstreamModel: source.upstreamModel,
        validAt: source.validAt.clone(),
        operationalHorizonHours: source.modelLeadHours - runtime.source_delay_hours,
    }
}

#[derive(Default)]
struct FixtureRepository {
    site_reads: usize,
    current_plans: Vec<CurrentWeatherQueryPlan>,
    history_plans: Vec<HistoryReadPlan>,
    tide_plans: Vec<TideReadPlan>,
    forecast_plans: Vec<ForecastReadPlan>,
    pressure_plans: Vec<ForecastPressureContextPlan>,
    temperature_sidecar_reads: Vec<ForecastTemperatureSidecarRead>,
    temperature_status_reads: Vec<ForecastTemperatureStatusRead>,
    rain_adjustment_reads: Vec<ForecastRainAdjustmentRead>,
    optional_read_order: Vec<&'static str>,
    trend_plans: Vec<TrendReadPlan>,
    daily_plans: Vec<DailyPrecipitationReadPlan>,
    observation_plans: Vec<ForecastObservationInputReadPlan>,
    health_reads: usize,
    rain_collection_reads: usize,
    fail_history: bool,
    fail_pressure: bool,
    pressure_available: bool,
    maintenance_wind_fixture: bool,
    malformed_forecast_source: bool,
    temperature_fixture: bool,
    rain_fixture: bool,
    fail_temperature_sidecar: bool,
    fail_temperature_status: bool,
    rain_adjustment_available: bool,
    fail_rain_adjustment: bool,
    invalid_rain_adjustment: bool,
    wrong_model_rain_adjustment: bool,
    rain_raw_missing: bool,
    rain_non_best_match: bool,
    fail_health: bool,
    rain_collection_available: bool,
    observation_available: bool,
    fail_observation: bool,
}

impl WeatherServiceRepository for FixtureRepository {
    /// execute exact active-site discovery
    fn active_sites(
        &mut self,
        plan: ListActiveSitesQuery,
    ) -> Result<Vec<ActiveSiteRow>, ServiceRepositoryError> {
        assert!(plan.binds.is_empty());
        assert!(plan.sql.contains("FROM sites si"));
        self.site_reads += 1;
        Ok(site_rows())
    }

    /// capture one exact current plan and return an empty row set
    fn current(
        &mut self,
        plan: CurrentWeatherQueryPlan,
    ) -> Result<Vec<CurrentWeatherRecordRow>, ServiceRepositoryError> {
        assert!(plan.sql.contains("pressureChange3hHpa"));
        self.current_plans.push(plan);
        Ok(Vec::new())
    }

    /// capture one exact history plan and optionally fail
    fn history(
        &mut self,
        plan: HistoryReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, ServiceRepositoryError> {
        assert!(plan.sql.contains("ORDER BY candidate.valid_at DESC"));
        self.history_plans.push(plan);

        // expose only the final ordinary repository failure
        if self.fail_history {
            return Err(ServiceRepositoryError);
        }
        Ok(Vec::new())
    }

    /// capture one exact tide plan and return one ordered row
    fn tides(&mut self, plan: TideReadPlan) -> Result<Vec<TideRecordRow>, ServiceRepositoryError> {
        assert!(plan.sql.contains("ORDER BY wr.valid_at ASC, wr.id ASC"));
        self.tide_plans.push(plan);
        Ok(vec![tide_row()])
    }

    /// capture one expanded forecast plan and return public plus context rows
    fn forecast(
        &mut self,
        plan: ForecastReadPlan,
    ) -> Result<Vec<WeatherRecordRow>, ServiceRepositoryError> {
        assert!(plan.sql.contains("ORDER BY wr.valid_at ASC, wr.id ASC"));
        self.forecast_plans.push(plan);
        // expose the Node-derived public hour for temperature or rain integration fixtures
        if self.temperature_fixture || self.rain_fixture {
            let mut rows = vec![
                forecast_row("2026-08-21T04:00:00.000Z", "forecast-context"),
                forecast_row("2026-08-22T06:00:00.000Z", "forecast-public"),
            ];
            // expose the ordinary raw-value rain guard independently
            if self.rain_raw_missing {
                rows[1].precipitationMm = Nullable::Null;
            }
            // expose the ordinary source-row identity rain guard independently
            if self.rain_non_best_match {
                rows[1].upstreamModel = Nullable::Value("ecmwf_ifs".to_owned());
            }
            return Ok(rows);
        }
        let mut rows = vec![
            forecast_row("2026-08-21T04:00:00.000Z", "forecast-context"),
            forecast_row("2026-08-22T04:00:00.000Z", "forecast-public"),
        ];
        // use the original qualified-maintenance oracle values only for its route fixture
        if self.maintenance_wind_fixture {
            // update both repository rows before adjustment projection
            for row in &mut rows {
                row.windGustMps = Nullable::Value(8.0);
                row.windSpeedMps = Nullable::Value(5.0);
                row.providerMetadata =
                    Nullable::Value(vec![(text("dataset"), JsonValue::String(text("forecast")))]);
            }
        }
        // expose one raw-projection failure before any model application
        if self.malformed_forecast_source {
            rows[1].sourceId = "missing-source".to_owned();
        }
        Ok(rows)
    }

    /// capture one exact optional pressure plan and expose bounded failure
    fn forecast_pressure_context(
        &mut self,
        plan: ForecastPressureContextPlan,
    ) -> Result<Vec<WeatherRecordRow>, ServiceRepositoryError> {
        assert!(plan.sql.contains("LIMIT 6"));
        self.pressure_plans.push(plan);

        // preserve forecast availability when optional pressure context fails
        if self.fail_pressure {
            return Err(ServiceRepositoryError);
        }
        // expose one optional context row only for the active-wind integration case
        if self.pressure_available {
            return Ok(vec![forecast_row(
                "2026-08-21T04:00:00.000Z",
                "pressure-context",
            )]);
        }
        Ok(Vec::new())
    }

    /// capture the exact data-time sidecar read and its optional failure
    fn temperature_canary_sidecar(
        &mut self,
        read: ForecastTemperatureSidecarRead,
    ) -> Result<Option<AdmittedTemperatureCanarySidecar>, ServiceRepositoryError> {
        self.optional_read_order.push("temperature_sidecar");
        self.temperature_sidecar_reads.push(read);

        // contain the optional sidecar read independently from forecast serving
        if self.fail_temperature_sidecar {
            return Err(ServiceRepositoryError);
        }
        Ok(Some(AdmittedTemperatureCanarySidecar {
            sidecar: temperature_sidecar(),
            recent_error_state_sha256: temperature_recent_state_sha256().to_owned(),
        }))
    }

    /// capture the exact safety-time status read and its optional failure
    fn temperature_canary_status(
        &mut self,
        read: ForecastTemperatureStatusRead,
    ) -> Result<Option<EcmwfTemperatureCanaryStatus>, ServiceRepositoryError> {
        self.optional_read_order.push("temperature_status");
        self.temperature_status_reads.push(read);

        // contain optional monitoring failure independently from row application
        if self.fail_temperature_status {
            return Err(ServiceRepositoryError);
        }
        Ok(Some(temperature_status()))
    }

    /// capture one nominal-time rain read and expose bounded missing or failure states
    fn rain_adjustment_run(
        &mut self,
        read: ForecastRainAdjustmentRead,
    ) -> Result<Option<AdmittedRainAdjustmentRun>, ServiceRepositoryError> {
        self.optional_read_order.push("rain_adjustment");
        self.rain_adjustment_reads.push(read);

        // contain optional retained-output failure independently from forecast serving
        if self.fail_rain_adjustment {
            return Err(ServiceRepositoryError);
        }
        // preserve an ordinary absent retained run distinctly from read failure
        if !self.rain_adjustment_available {
            return Ok(None);
        }
        let mut run = rain_adjustment_run();
        // expose one malformed retained lifecycle without changing its decoded type
        if self.invalid_rain_adjustment {
            run.generated_at = instant(1_787_396_400_000, "2026-08-22T11:00:00.000Z");
        }
        // expose a decoded run that belongs to a different model artifact
        if self.wrong_model_rain_adjustment {
            run.model_sha256 = "wrong-model-sha256".to_owned();
        }
        Ok(Some(run))
    }

    /// capture one exact trend plan and return one ordered bucket
    fn trends(
        &mut self,
        plan: TrendReadPlan,
    ) -> Result<Vec<TrendPointRow>, ServiceRepositoryError> {
        assert!(plan.sql.contains("ORDER BY preferred_days.valid_at ASC"));
        self.trend_plans.push(plan);
        Ok(vec![trend_row()])
    }

    /// capture one exact daily plan and return the nearest physical gauge
    fn daily_precipitation(
        &mut self,
        plan: DailyPrecipitationReadPlan,
    ) -> Result<Option<DailyPrecipitationRow>, ServiceRepositoryError> {
        assert!(plan.sql.contains("WITH nearest_source AS"));
        self.daily_plans.push(plan);
        Ok(Some(DailyPrecipitationRow {
            accumulationMm: 2.54,
            sourceId: "13".to_owned(),
            stationSlug: "ballydidean-ecowitt".to_owned(),
            validThrough: JsDateOrString::String(text("2026-08-22T04:59:00.000Z")),
        }))
    }

    /// expose one ready snapshot or an ordinary repository failure
    fn health(&mut self) -> Result<HealthSnapshot, ServiceRepositoryError> {
        self.health_reads += 1;

        // preserve the handler's unavailable fallback on repository failure
        if self.fail_health {
            return Err(ServiceRepositoryError);
        }
        Ok(HealthSnapshot {
            database: DatabaseStatus::Ready,
            migration_status: MigrationStatus::Current,
            migration_version: Some("2026082201".to_owned()),
            worker_last_loop_epoch_ms: Some(1_787_374_200_000),
        })
    }

    /// expose the optional fixed-site sanitized collection status port
    fn rain_collection_status(
        &mut self,
    ) -> Result<Option<RainCollectionStatus>, ServiceRepositoryError> {
        self.rain_collection_reads += 1;

        // preserve the Node route's absent-store not-found behavior
        if !self.rain_collection_available {
            return Ok(None);
        }
        Ok(Some(rain_collection_status()))
    }

    /// execute the fixed private observation plan through the typed port
    fn forecast_observation_input(
        &mut self,
        plan: ForecastObservationInputReadPlan,
    ) -> Result<Option<ForecastObservationInput>, ServiceRepositoryError> {
        assert!(plan.sql.contains("WITH target_site AS"));
        self.observation_plans.push(plan);

        // preserve ordinary repository failure and missing-input categories
        if self.fail_observation {
            return Err(ServiceRepositoryError);
        }
        if !self.observation_available {
            return Ok(None);
        }
        Ok(Some(forecast_observation_input()))
    }
}

/// encode one generated UTF-16 string
fn text(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// build one admitted canonical instant
fn instant(epoch_milliseconds: i64, utc: &str) -> CanonicalInstant {
    CanonicalInstant {
        epoch_milliseconds,
        utc: utc.to_owned(),
    }
}

/// build one active source row
fn source_row(
    source_id: &str,
    source_key: &str,
    source_kind: SourceKind,
    station_name: &str,
    station_slug: &str,
    provider_key: &str,
) -> ActiveSiteRow {
    ActiveSiteRow {
        attribution_label: if source_kind == SourceKind::TidePrediction {
            "NOAA Tides & Currents".to_owned()
        } else {
            "Weather data by Open-Meteo".to_owned()
        },
        attribution_url: if source_kind == SourceKind::TidePrediction {
            "https://tidesandcurrents.noaa.gov/".to_owned()
        } else {
            "https://open-meteo.com/".to_owned()
        },
        latitude: 47.95,
        longitude: -122.42,
        provider_key: provider_key.to_owned(),
        provider_name: provider_key.to_owned(),
        site_name: "Ballydidean".to_owned(),
        site_slug: "ballydidean".to_owned(),
        source_id: source_id.to_owned(),
        source_key: source_key.to_owned(),
        source_kind,
        station_kind: StationKind::Virtual,
        station_latitude: 47.95,
        station_longitude: -122.42,
        station_name: station_name.to_owned(),
        station_slug: station_slug.to_owned(),
        timezone: "America/Los_Angeles".to_owned(),
    }
}

/// build active current and tide discovery rows
fn site_rows() -> Vec<ActiveSiteRow> {
    vec![
        source_row(
            "11",
            "open-meteo-current",
            SourceKind::ModelCurrent,
            "Open-Meteo Virtual",
            "open-meteo-virtual",
            "open-meteo",
        ),
        source_row(
            "12",
            "open-meteo-forecast-v4",
            SourceKind::Forecast,
            "Open-Meteo Forecast",
            "open-meteo-forecast",
            "open-meteo",
        ),
        source_row(
            "13",
            "ecowitt-rain",
            SourceKind::PhysicalSensor,
            "Ballydidean Ecowitt",
            "ballydidean-ecowitt",
            "ecowitt-local",
        ),
        source_row(
            "42",
            "glendale-tide-predictions",
            SourceKind::TidePrediction,
            "Glendale Tide Predictions",
            "glendale-tide-predictions",
            "noaa-co-ops",
        ),
    ]
}

/// build one representative forecast row with inactive model decisions
fn forecast_row(valid_at: &str, id: &str) -> WeatherRecordRow {
    WeatherRecordRow {
        adapterVersion: Nullable::Value("open-meteo-forecast-daily/v4".to_owned()),
        apparentTemperatureC: Nullable::Value(11.7),
        blackGlobeTemperatureC: Nullable::Null,
        cloudCoverPercent: Nullable::Value(72.0),
        contentHash: format!("sha256-{id}"),
        contractEpoch: Nullable::Value("v4".to_owned()),
        deviceModel: Nullable::Null,
        deviceSerial: Nullable::Null,
        deviceVendor: Nullable::Null,
        firstReceivedAt: JsDateOrString::String(text("2026-08-21T07:00:00.000Z")),
        id: id.to_owned(),
        lastReceivedAt: JsDateOrString::String(text("2026-08-21T07:00:00.000Z")),
        pm25MicrogramsPerCubicMeter: Nullable::Value(5.0),
        precipitationMm: Nullable::Value(0.1),
        precipitationRateMmPerHour: Nullable::Value(0.1),
        pressureHpa: Nullable::Value(1018.1),
        productRunAt: Nullable::Value(JsDateOrString::String(text("2026-08-21T07:00:00.000Z"))),
        providerKey: "open-meteo".to_owned(),
        providerMetadata: Nullable::Null,
        qualityMetadata: Nullable::Null,
        relativeHumidityPercent: Nullable::Value(83.0),
        revisionCount: 0,
        siteSlug: "ballydidean".to_owned(),
        sourceId: "12".to_owned(),
        sourceConfigFingerprint: Nullable::Value("fingerprint".to_owned()),
        sourceKey: "open-meteo-forecast-v4".to_owned(),
        sourceKind: SourceKind::Forecast,
        stationSlug: "open-meteo-forecast".to_owned(),
        soilElectricalConductivityMicrosiemensPerCm: Nullable::Null,
        soilMoisturePercent: Nullable::Null,
        solarRadiationWm2: Nullable::Null,
        temperatureC: Nullable::Value(12.1),
        upstreamModel: Nullable::Value("best_match".to_owned()),
        upstreamTimezone: "GMT".to_owned(),
        uvIndex: Nullable::Value(0.2),
        validAt: JsDateOrString::String(text(valid_at)),
        waterLevelM: Nullable::Null,
        windDirectionDegrees: Nullable::Value(180.0),
        windGustMps: Nullable::Value(3.1),
        windSpeedMps: Nullable::Value(1.8),
        wetBulbGlobeTemperatureC: Nullable::Null,
    }
}

/// build one canonical repository row with an offset-form retrieval instant
fn repository_wind_row() -> WeatherRecordRow {
    let mut row = forecast_row("2026-09-09T06:00:00.000Z", "repository-wind-admission");
    row.productRunAt = Nullable::Value(JsDateOrString::String(text("2026-09-08T10:45:00-07:00")));
    row.providerMetadata =
        Nullable::Value(vec![(text("dataset"), JsonValue::String(text("forecast")))]);
    row
}

/// assert the exact normalized row identity shared by both admitted wind grammars
fn assert_repository_wind_row_admitted(result: WindForecastRowAdmission) {
    let WindForecastRowAdmission::Admitted(admitted) = result else {
        panic!("repository wind row should be admitted");
    };
    assert_eq!(
        admitted.source_identity,
        AdmittedWindRowSourceIdentity::Match
    );
    assert!(admitted.calendar_fingerprint_matches);
    assert_eq!(admitted.calendar.month, 9);
    assert_eq!(
        admitted.calendar.daypart,
        ForecastAdjustmentCoefficientDaypartEnum::Night
    );
    assert_eq!(
        admitted.calendar.season,
        ForecastAdjustmentCoefficientSeasonEnum::Summer
    );
    assert_eq!(
        admitted.raw_forecast_provenance.adapterVersion,
        "open-meteo-forecast-daily/v4"
    );
    assert_eq!(admitted.raw_forecast_provenance.contractEpoch, "v4");
    assert_eq!(admitted.raw_forecast_provenance.dataset, "forecast");
    assert_eq!(
        admitted.raw_forecast_provenance.referenceAt,
        "2026-09-08T17:45:00.000Z"
    );
    assert_eq!(admitted.raw_forecast_provenance.targetLeadHours, 13);
    assert_eq!(
        admitted.raw_forecast_provenance.validAt,
        "2026-09-09T06:00:00.000Z"
    );
}

/// build one representative trend bucket
fn trend_row() -> TrendPointRow {
    TrendPointRow {
        apparentTemperatureC: Nullable::Value(10.1),
        pm25MicrogramsPerCubicMeter: Nullable::Value(12.3),
        precipitationMm: Nullable::Value(1.2),
        pressureHpa: Nullable::Value(1012.4),
        relativeHumidityPercent: Nullable::Value(82.0),
        temperatureC: Nullable::Value(11.5),
        temperatureMaximumC: Nullable::Value(19.4),
        temperatureMinimumC: Nullable::Value(8.2),
        validAt: JsDate::EpochMilliseconds(1_787_295_600_000),
        windDirectionDegrees: Nullable::Value(-135.0),
        windGustMps: Nullable::Value(7.2),
        windSpeedMps: Nullable::Value(4.1),
    }
}

/// build one typed sanitized rain collection status
fn rain_collection_status() -> RainCollectionStatus {
    RainCollectionStatus {
        contractVersion: RainCollectionStatusContractVersionLiteral,
        modelEnabled: RainCollectionStatusModelEnabledLiteral,
        qualificationEnabled: RainCollectionStatusQualificationEnabledLiteral,
        claims: 12,
        receipts: 11,
        validForecasts: 3,
        timelyForecasts: 2,
        validStationWindows: 7,
        stationsSeen: 4,
        failedRequests: 1,
        pendingRequests: 2,
        unknownRequests: 1,
        lastClaimAt: Nullable::Value("2026-09-13T01:00:00.000Z".to_owned()),
        lastReceiptAt: Nullable::Value("2026-09-13T01:01:00.000Z".to_owned()),
        lastForecastReceiptAt: Nullable::Value("2026-09-13T01:02:00.000Z".to_owned()),
        lastStationReceiptAt: Nullable::Value("2026-09-13T01:03:00.000Z".to_owned()),
        pausedUntil: Nullable::Value("2026-09-14T00:00:00.000Z".to_owned()),
        compressedBytes: 4096,
    }
}

/// build one typed fixed private observation input
fn forecast_observation_input() -> ForecastObservationInput {
    ForecastObservationInput {
        projectionFrom: JsDateOrString::String(text("2026-08-21T07:00:00.000Z")),
        queryFrom: JsDateOrString::String(text("2026-08-21T04:00:00.000Z")),
        records: vec![ForecastObservationInputRow {
            apparentTemperatureC: Nullable::Value(9.0),
            cloudCoverPercent: Nullable::Value(20.0),
            id: "100".to_owned(),
            lastReceivedAt: JsDateOrString::String(text("2026-08-22T04:00:00.000Z")),
            pm25MicrogramsPerCubicMeter: Nullable::Null,
            precipitationRateMmPerHour: Nullable::Value(0.0),
            pressureHpa: Nullable::Value(1010.0),
            providerKey: "ecowitt-local".to_owned(),
            relativeHumidityPercent: Nullable::Value(80.0),
            solarRadiationWm2: Nullable::Value(0.0),
            sourceId: "1".to_owned(),
            sourceKey: "ecowitt-local-live-v1".to_owned(),
            sourceKind: ForecastObservationInputRowSourceKindEnum::PhysicalSensor,
            stationSlug: "ballydidean-ecowitt".to_owned(),
            temperatureC: Nullable::Value(10.0),
            uvIndex: Nullable::Value(0.0),
            validAt: JsDateOrString::String(text("2026-08-22T04:00:00.000Z")),
            windDirectionDegrees: Nullable::Value(90.0),
            windGustMps: Nullable::Value(3.0),
            windSpeedMps: Nullable::Value(2.0),
        }],
        siteSlug: ForecastObservationInputSiteSlugLiteral,
        timezone: ForecastObservationInputTimezoneLiteral,
        through: JsDateOrString::String(text("2026-08-22T05:00:00.000Z")),
    }
}

/// build one admitted ECMWF sidecar matching the public forecast hour
fn temperature_sidecar() -> EcmwfTemperatureCanarySidecar {
    EcmwfTemperatureCanarySidecar {
        hours: vec![
            EcmwfTemperatureCanaryHour {
                contentHash: "d".repeat(64),
                modelLeadHours: 12,
                rawRelativeHumidityPercent: Nullable::Value(70.0),
                rawTemperatureC: 99.0,
                rawWindSpeedMps: Nullable::Value(1.0),
                validAt: "2026-08-22T06:00:00.000Z".to_owned(),
            },
            EcmwfTemperatureCanaryHour {
                contentHash: "c".repeat(64),
                modelLeadHours: 12,
                rawRelativeHumidityPercent: Nullable::Value(80.0),
                rawTemperatureC: 11.0,
                rawWindSpeedMps: Nullable::Value(2.0),
                validAt: "2026-08-22T06:00:00.000Z".to_owned(),
            },
        ],
        run: EcmwfTemperatureCanaryRun {
            adapterVersion: EcmwfTemperatureCanaryRunAdapterVersionLiteral,
            firstReceivedAt: "2026-08-22T00:05:00.000Z".to_owned(),
            id: "temperature-run".to_owned(),
            modelCycle: EcmwfTemperatureCanaryRunModelCycleEnum::Value50r1,
            providerResponseSha256: "5".repeat(64),
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
                sourceKeys: Vec::new(),
                supported: false,
                targetRunInitializedAt: "2026-08-21T18:00:00.000Z".to_owned(),
                windowEndValidAt: "2026-08-21T11:00:00.000Z".to_owned(),
            },
            runInitializedAt: "2026-08-21T18:00:00.000Z".to_owned(),
            stateReason: "cold_start".to_owned(),
            stateStatus: EcmwfTemperatureCanaryRunStateStatusEnum::Cold,
            upstreamModel: EcmwfTemperatureCanaryRunUpstreamModelLiteral,
        },
    }
}

/// build one bounded collector status from the sidecar fixture
fn temperature_status() -> EcmwfTemperatureCanaryStatus {
    EcmwfTemperatureCanaryStatus {
        firstReceivedAt: "2026-08-22T00:05:00.000Z".to_owned(),
        hourCount: 2,
        latestRunInitializedAt: "2026-08-21T18:00:00.000Z".to_owned(),
        stateReason: "cold_start".to_owned(),
        stateStatus: EcmwfTemperatureCanaryStatusStateStatusEnum::Cold,
    }
}

/// return the compiled Node digest for the fixture's admitted recent-error state
fn temperature_recent_state_sha256() -> &'static str {
    "70239b1e0bcc1171e36838ed4e1c4e508efa0a8c30f70393dfcad4df1a5115bf"
}

/// build one valid retained rain run with one applied public forecast hour
fn rain_adjustment_run() -> AdmittedRainAdjustmentRun {
    AdmittedRainAdjustmentRun {
        run_initialized_at: instant(1_787_335_200_000, "2026-08-21T18:00:00.000Z"),
        model_sha256:
            LoadedForecastAdjustmentRainRuntimeRegistryV1ActiveArtifactSha256Literal::WIRE_VALUE
                .to_owned(),
        first_received_at: instant(1_787_357_100_000, "2026-08-22T00:05:00.000Z"),
        decision_at: instant(1_787_364_000_000, "2026-08-22T02:00:00.000Z"),
        generated_at: instant(1_787_364_300_000, "2026-08-22T02:05:00.000Z"),
        hours: vec![AdmittedRainAdjustmentHour {
            valid_at: instant(1_787_378_400_000, "2026-08-22T06:00:00.000Z"),
            model_lead_hours: 12,
            raw_precipitation_mm: 0.1,
            corrected_precipitation_mm: 0.05,
            applied: true,
            reason_code: None,
        }],
    }
}

/// build one ordered tide projection row
fn tide_row() -> TideRecordRow {
    TideRecordRow {
        attributionLabel: "NOAA Tides & Currents".to_owned(),
        attributionUrl: "https://tidesandcurrents.noaa.gov/".to_owned(),
        predictionType: Nullable::Value("H".to_owned()),
        providerKey: "noaa-co-ops".to_owned(),
        sourceId: "42".to_owned(),
        sourceKind: TideRecordRowSourceKindEnum::TidePrediction,
        stationName: "Glendale Tide Predictions".to_owned(),
        stationSlug: "glendale-tide-predictions".to_owned(),
        validAt: JsDateOrString::String(text("2026-08-22T11:27:00.000Z")),
        waterLevelM: 3.271,
    }
}

/// execute one service fixture request
fn request(
    repository: &mut FixtureRepository,
    clock: &mut FixtureClock,
    boundary: &FixtureBoundary,
    method: &str,
    pathname: &str,
    query: &[QueryParameter<'_>],
) -> weather_api::WeatherServiceResponse {
    let forecast_runtime = inactive_forecast_runtime();
    serve_decoded_request(
        repository,
        clock,
        &WeatherServiceBoundaries {
            nominal_data_at: None,
            time_resolver: boundary,
            history: boundary,
            tides: boundary,
            forecast: boundary,
            forecast_runtime: &forecast_runtime,
            forecast_wind_runtime: None,
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: boundary,
            daily_precipitation: boundary,
            rain_collection: boundary,
            forecast_observation: boundary,
            version: None,
        },
        DecodedServiceRequest {
            method,
            pathname,
            query,
        },
    )
}

const METHOD_ERROR_WIRE: &[u8] =
    b"{\"error\":{\"code\":\"method_not_allowed\",\"message\":\"Only GET and HEAD are supported\"}}";

#[derive(Default)]
struct MethodWireEncoder {
    count: usize,
    refuse_sites: bool,
}

impl WeatherServiceWireEncoder for MethodWireEncoder {
    type Error = &'static str;

    /// Exercise the same serializer before head suppression and after method dispatch.
    fn encode<'a>(&'a mut self, body: &WeatherServiceBody) -> Result<&'a [u8], Self::Error> {
        self.count += 1;
        match body {
            WeatherServiceBody::Sites(_) if self.refuse_sites => Err("fixture encode refusal"),
            WeatherServiceBody::Sites(_) => Ok(b"{\"data\":[]}"),
            WeatherServiceBody::Error(response) => {
                assert_eq!(response.error.code, "method_not_allowed");
                assert_eq!(response.error.message, "Only GET and HEAD are supported");
                Ok(METHOD_ERROR_WIRE)
            }
            _ => panic!("unexpected service representation"),
        }
    }
}

/// exact private observation representation fixture
struct ObservationWireEncoder<'bytes> {
    bytes: &'bytes [u8],
    count: usize,
}

impl WeatherServiceWireEncoder for ObservationWireEncoder<'_> {
    type Error = &'static str;

    /// return one configured observation representation and count the encoding
    fn encode<'a>(&'a mut self, body: &WeatherServiceBody) -> Result<&'a [u8], Self::Error> {
        assert!(matches!(
            body,
            WeatherServiceBody::ForecastObservationInput(_)
        ));
        self.count += 1;
        Ok(self.bytes)
    }
}

const RAIN_COLLECTION_WIRE: &[u8] = b"{\"data\":{}}";
const INTERNAL_ERROR_WIRE: &[u8] =
    b"{\"error\":{\"code\":\"internal_error\",\"message\":\"Unexpected server error\"}}";

#[derive(Default)]
struct RainDiagnosticWireEncoder {
    count: usize,
    fail: bool,
}

impl WeatherServiceWireEncoder for RainDiagnosticWireEncoder {
    type Error = &'static str;

    /// return one rain representation or its selected encoding failure
    fn encode<'a>(&'a mut self, body: &WeatherServiceBody) -> Result<&'a [u8], Self::Error> {
        assert!(matches!(body, WeatherServiceBody::RainCollection(_)));
        self.count += 1;
        // preserve the selected final encoding outcome after recording the call
        if self.fail {
            Err("fixture encode refusal")
        } else {
            Ok(RAIN_COLLECTION_WIRE)
        }
    }
}

/// Transfer a real matched API method response and its read-only HEAD representation.
#[test]
fn public_wire_runs_the_service_before_exposing_its_representation() {
    let boundary = FixtureBoundary;
    let mut repository = FixtureRepository::default();
    let mut clock = FixtureClock { reads: 0 };
    let forecast_runtime = inactive_forecast_runtime();
    let boundaries = WeatherServiceBoundaries {
        nominal_data_at: None,
        time_resolver: &boundary,
        history: &boundary,
        tides: &boundary,
        forecast: &boundary,
        forecast_runtime: &forecast_runtime,
        forecast_wind_runtime: None,
        forecast_temperature_runtime: None,
        forecast_rain_runtime: None,
        trends: &boundary,
        daily_precipitation: &boundary,
        rain_collection: &boundary,
        forecast_observation: &boundary,
        version: None,
    };
    let mut encoder = MethodWireEncoder::default();
    // both requests reach the same existing typed service with no alternate path
    for (method, refuse_sites, status, allow, body) in [
        ("HEAD", false, 200, None, &[] as &[u8]),
        ("HEAD", true, 500, None, &[] as &[u8]),
        ("POST", false, 405, Some("GET, HEAD"), METHOD_ERROR_WIRE),
    ] {
        encoder.refuse_sites = refuse_sites;
        let response = serve_wire_decoded_request(
            &mut repository,
            &mut clock,
            &boundaries,
            DecodedServiceRequest {
                method,
                pathname: "/api/v1/sites",
                query: &[],
            },
            &mut encoder,
        );
        assert_eq!(response.status, status);
        assert_eq!(response.headers.cache_control, Some("no-store"));
        assert_eq!(
            response.headers.content_type,
            "application/json; charset=utf-8"
        );
        assert_eq!(response.headers.allow, allow);
        assert_eq!(response.body, body);
    }
    assert_eq!(repository.site_reads, 2);
    assert_eq!(encoder.count, 3);
}

/// execute one separately owned private-listener fixture request
fn private_request(
    repository: &mut FixtureRepository,
    clock: &mut PrivateFixtureClock,
    boundary: &FixtureBoundary,
    method: &str,
    query: &[QueryParameter<'_>],
) -> weather_api::WeatherServiceResponse {
    let nominal_data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let forecast_runtime = inactive_forecast_runtime();
    serve_private_decoded_request(
        AdmittedPrivateRouteAuthority::admitted(
            PrivateServiceListenerOwner::ForecastObservationProjection,
        ),
        repository,
        clock,
        &WeatherServiceBoundaries {
            nominal_data_at: Some(&nominal_data_at),
            time_resolver: boundary,
            history: boundary,
            tides: boundary,
            forecast: boundary,
            forecast_runtime: &forecast_runtime,
            forecast_wind_runtime: None,
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: boundary,
            daily_precipitation: boundary,
            rain_collection: boundary,
            forecast_observation: boundary,
            version: None,
        },
        DecodedServiceRequest {
            method,
            pathname: "/internal/v1/forecast-observation-input",
            query,
        },
    )
}

/// execute one private observation request through the wire boundary
fn private_wire_request<'a, Encoder: WeatherServiceWireEncoder>(
    repository: &mut FixtureRepository,
    clock: &mut PrivateFixtureClock,
    boundary: &FixtureBoundary,
    method: &str,
    encoder: &'a mut Encoder,
) -> weather_api::WeatherServiceWireResponse<'a> {
    let nominal_data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let forecast_runtime = inactive_forecast_runtime();
    serve_private_wire_decoded_request(
        AdmittedPrivateRouteAuthority::admitted(
            PrivateServiceListenerOwner::ForecastObservationProjection,
        ),
        repository,
        clock,
        &WeatherServiceBoundaries {
            nominal_data_at: Some(&nominal_data_at),
            time_resolver: boundary,
            history: boundary,
            tides: boundary,
            forecast: boundary,
            forecast_runtime: &forecast_runtime,
            forecast_wind_runtime: None,
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: boundary,
            daily_precipitation: boundary,
            rain_collection: boundary,
            forecast_observation: boundary,
            version: None,
        },
        DecodedServiceRequest {
            method,
            pathname: "/internal/v1/forecast-observation-input",
            query: &[],
        },
        encoder,
    )
}

/// build construction-snapshotted inactive forecast lifecycle instants
fn inactive_forecast_runtime() -> InactiveForecastServiceRuntime {
    InactiveForecastServiceRuntime::unconfigured(
        "2026-08-20T05:00:00.000Z",
        "2026-08-20T06:00:00.000Z",
    )
}

/// build distinct typed startup refusals for integrated service projection
fn refused_forecast_startup_snapshots() -> WeatherApiStartupSnapshots {
    WeatherApiStartupSnapshots {
        adjustment: ForecastAdjustmentStartupSnapshot {
            loadedAt: "2026-08-20T04:57:00.000Z".to_owned(),
            runtime: LoadedApiForecastAdjustmentRuntime::Standard(
                LoadedForecastAdjustmentRuntimeV1::Disabled(
                    LoadedForecastAdjustmentRuntimeV1Disabled {
                        bundle: LoadedForecastAdjustmentRuntimeV1DisabledBundleNull,
                        reasonCode:
                            LoadedForecastAdjustmentRuntimeV1DisabledReasonCodeEnum::RegistryInactive,
                        state: LoadedForecastAdjustmentRuntimeV1DisabledStateLiteral,
                    },
                ),
            ),
        },
        temperature_adjustment: ForecastTemperatureAdjustmentStartupSnapshot {
            loadedAt: "2026-08-20T04:58:00.000Z".to_owned(),
            runtime: LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy {
                value: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Disabled(
                    LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Disabled {
                        bundle:
                            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledBundleNull,
                        reasonCode:
                            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledReasonCodeEnum::BundleInvalid,
                        state:
                            LoadedForecastAdjustmentTemperatureCanaryRuntimeV1DisabledStateLiteral,
                    },
                ),
                comparatorAuthority: weather_contracts::FieldPresence::Missing,
            },
        },
        rain_adjustment: ForecastRainAdjustmentStartupSnapshot {
            loadedAt: "2026-08-20T04:59:00.000Z".to_owned(),
            runtime: LoadedForecastAdjustmentRainRuntimeRegistryV1::Disabled(
                LoadedForecastAdjustmentRainRuntimeRegistryV1Disabled {
                    artifactSha256:
                        LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledArtifactSha256Null,
                    comparatorAuthority: weather_contracts::FieldPresence::Missing,
                    reasonCode:
                        LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledReasonCodeEnum::PolicyRaw,
                    state: LoadedForecastAdjustmentRainRuntimeRegistryV1DisabledStateLiteral,
                },
            ),
        },
    }
}

/// build one admitted active wind snapshot around the divergent fixture clocks
fn active_wind_runtime() -> AdmittedWindCanaryRuntime {
    AdmittedWindCanaryRuntime {
        environment_kill_switch: None,
        authorization: WindCanaryAuthorizationWindow::TimeLimited {
            activated_at_epoch_ms: 1_787_202_000_000,
            expires_at_epoch_ms: 1_787_547_600_000,
        },
        expires_at: Some(instant(1_787_547_600_000, "2026-08-24T05:00:00.000Z")),
        enabled_metric_bands: vec![
            ForecastAdjustmentMetricBand {
                leadBand: ForecastLeadBandKey::Value001024,
                metric: ForecastAdjustmentMetric::WindGustMps,
            },
            ForecastAdjustmentMetricBand {
                leadBand: ForecastLeadBandKey::Value001024,
                metric: ForecastAdjustmentMetric::WindSpeedMps,
            },
        ],
        evaluated_at_epoch_ms: 0,
        evidence: AdmittedWindCanaryEvidence {
            active_bundle_sha256:
                "5e8b2e3932111621af6785a1b16dfd22edc0a2d26059c6e396654c70119abbe1".to_owned(),
            authorization_sha256:
                "fc4895cb45ffb59a725ead664ae20924399585a9d2fa5f5d068f40589b523537".to_owned(),
            candidate_artifact_sha256:
                "454e7b91a34407a6ace79150dfc5799c37ced2adbdab915b710f18180c3cd488".to_owned(),
            transfer_report_sha256:
                "2c68ba8be5d5343a8d358cee7e21bb157f60c54e6c0bf2bc4b0978f50ec50582".to_owned(),
        },
    }
}

/// Build the admitted typed wind candidate used by the concrete service owner.
fn active_wind_candidate() -> ForecastAdjustmentWindCanaryCandidateV1 {
    let metric_bands = vec![
        ForecastAdjustmentMetricBand {
            leadBand: ForecastLeadBandKey::Value001024,
            metric: ForecastAdjustmentMetric::WindGustMps,
        },
        ForecastAdjustmentMetricBand {
            leadBand: ForecastLeadBandKey::Value001024,
            metric: ForecastAdjustmentMetric::WindSpeedMps,
        },
    ];
    ForecastAdjustmentWindCanaryCandidateV1 {
        algorithmContractVersion:
            ForecastAdjustmentWindCanaryCandidateV1AlgorithmContractVersionLiteral,
        artifactKind: ForecastAdjustmentWindCanaryCandidateV1ArtifactKindLiteral,
        candidateArtifactSha256: "454e7b91a34407a6ace79150dfc5799c37ced2adbdab915b710f18180c3cd488"
            .to_owned(),
        coefficientPayloadSha256: "c".repeat(64),
        coefficients: vec![
            wind_root_coefficient(ForecastAdjustmentMetric::WindGustMps, -3.1),
            wind_root_coefficient(ForecastAdjustmentMetric::WindSpeedMps, -1.8),
        ],
        contractVersion: ForecastAdjustmentWindCanaryCandidateV1ContractVersionLiteral,
        enabledMetricBands: metric_bands,
        exportManifestSha256: "e".repeat(64),
        finalTrainingCutoff: "2026-07-25T07:00:00.000Z".to_owned(),
        runtimeFingerprint: ForecastAdjustmentWindCanaryCandidateV1RuntimeFingerprint {
            icuVersion: "78.3".to_owned(),
            tzdataVersion: "2026b".to_owned(),
        },
        servedForecastIdentity: ForecastAdjustmentForecastIdentity {
            adapterVersion: "open-meteo-forecast-daily/v4".to_owned(),
            cohort: ForecastAdjustmentForecastIdentityCohortLiteral,
            contractEpoch: "v4".to_owned(),
            dataset: "forecast".to_owned(),
            referenceKind: ForecastAdjustmentForecastIdentityReferenceKindLiteral,
            sourceConfigFingerprint: "fingerprint".to_owned(),
            sourceKey: "open-meteo-forecast-v4".to_owned(),
            upstreamModel: "best_match".to_owned(),
        },
        siteKey: ForecastAdjustmentWindCanaryCandidateV1SiteKeyLiteral,
        timezone: ForecastAdjustmentWindCanaryCandidateV1TimezoneLiteral,
        trainingEnvelopes: vec![
            ForecastAdjustmentTrainingEnvelope {
                leadBand: ForecastLeadBandKey::Value001024,
                maximum: 30.0,
                metric: ForecastAdjustmentTrainingEnvelopeMetricEnum::WindGustMps,
                minimum: 0.0,
            },
            ForecastAdjustmentTrainingEnvelope {
                leadBand: ForecastLeadBandKey::Value001024,
                maximum: 30.0,
                metric: ForecastAdjustmentTrainingEnvelopeMetricEnum::WindSpeedMps,
                minimum: 0.0,
            },
        ],
        trainingForecastIdentity: ForecastAdjustmentWindCanaryTrainingIdentity {
            adapterVersion: "open-meteo-previous-runs/v1".to_owned(),
            cohort: ForecastAdjustmentWindCanaryTrainingIdentityCohortLiteral,
            contractEpoch: "open-meteo-previous-runs-best-match/2026-08".to_owned(),
            dataset: ForecastAdjustmentWindCanaryTrainingIdentityDatasetLiteral,
            referenceKind: ForecastAdjustmentWindCanaryTrainingIdentityReferenceKindLiteral,
            sourceConfigFingerprint: "f".repeat(64),
            sourceKey: "open-meteo-previous-runs-v1".to_owned(),
            upstreamModel: ForecastAdjustmentWindCanaryTrainingIdentityUpstreamModelLiteral,
        },
        trainingProvenance: ForecastAdjustmentTrainingProvenance {
            aggregationContractSha256: "a".repeat(64),
            coordinateManifestSha256: "b".repeat(64),
            metricEligibilitySha256: "c".repeat(64),
            observationSourceLineageSha256: "d".repeat(64),
            observationStationManifestSha256: "e".repeat(64),
            spatialWeightSha256: "f".repeat(64),
        },
    }
}

/// build the admitted qualified-maintenance candidate used by service dispatch
fn maintenance_wind_candidate() -> weather_contracts::ForecastAdjustmentCandidateV2 {
    let enabled_metric_bands = vec![
        ForecastAdjustmentMetricBand {
            leadBand: ForecastLeadBandKey::Value001024,
            metric: ForecastAdjustmentMetric::WindGustMps,
        },
        ForecastAdjustmentMetricBand {
            leadBand: ForecastLeadBandKey::Value001024,
            metric: ForecastAdjustmentMetric::WindSpeedMps,
        },
    ];
    weather_contracts::ForecastAdjustmentCandidateV2 {
        algorithmContractVersion:
            weather_contracts::ForecastAdjustmentCandidateV2AlgorithmContractVersionLiteral,
        candidateArtifactSha256: "b".repeat(64),
        coefficientPayloadSha256: "c".repeat(64),
        coefficients: vec![
            wind_root_coefficient(ForecastAdjustmentMetric::WindGustMps, 0.25),
            wind_root_coefficient(ForecastAdjustmentMetric::WindSpeedMps, 0.25),
        ],
        contractVersion: weather_contracts::ForecastAdjustmentCandidateV2ContractVersionLiteral,
        developmentReportSha256: "d".repeat(64),
        enabledMetricBands: enabled_metric_bands.clone(),
        evaluationEpochId: "maintenance-2026-10".to_owned(),
        exportManifestSha256: "e".repeat(64),
        finalTrainingCutoff: "2026-07-25T07:00:00.000Z".to_owned(),
        forecastIdentity: ForecastAdjustmentForecastIdentity {
            adapterVersion: "open-meteo-forecast-daily/v4".to_owned(),
            cohort: ForecastAdjustmentForecastIdentityCohortLiteral,
            contractEpoch: "v4".to_owned(),
            dataset: "forecast".to_owned(),
            referenceKind: ForecastAdjustmentForecastIdentityReferenceKindLiteral,
            sourceConfigFingerprint: "fingerprint".to_owned(),
            sourceKey: "open-meteo-forecast-v4".to_owned(),
            upstreamModel: "best_match".to_owned(),
        },
        metricPolicies: Vec::new(),
        runtimeFingerprint: weather_contracts::ForecastAdjustmentCandidateV2RuntimeFingerprint {
            icuVersion: "78.3".to_owned(),
            tzdataVersion: "2026b".to_owned(),
        },
        siteKey: weather_contracts::ForecastAdjustmentCandidateV2SiteKeyLiteral,
        timezone: weather_contracts::ForecastAdjustmentCandidateV2TimezoneLiteral,
        trainingEnvelopes: enabled_metric_bands
            .iter()
            .map(|pair| ForecastAdjustmentTrainingEnvelope {
                leadBand: pair.leadBand,
                maximum: 30.0,
                metric: match pair.metric {
                    ForecastAdjustmentMetric::WindGustMps => {
                        ForecastAdjustmentTrainingEnvelopeMetricEnum::WindGustMps
                    }
                    ForecastAdjustmentMetric::WindSpeedMps => {
                        ForecastAdjustmentTrainingEnvelopeMetricEnum::WindSpeedMps
                    }
                    _ => unreachable!("fixture contains only wind metrics"),
                },
                minimum: 0.0,
            })
            .collect(),
        trainingProvenance: ForecastAdjustmentTrainingProvenance {
            aggregationContractSha256: "1".repeat(64),
            coordinateManifestSha256: "2".repeat(64),
            metricEligibilitySha256: "3".repeat(64),
            observationSourceLineageSha256: "4".repeat(64),
            observationStationManifestSha256: "5".repeat(64),
            spatialWeightSha256: "6".repeat(64),
        },
    }
}

/// build one exact maintenance-qualified wind startup snapshot
fn maintenance_qualified_wind_startup_snapshot() -> ForecastAdjustmentStartupSnapshot {
    let bundle_sha256 = "a".repeat(64);
    ForecastAdjustmentStartupSnapshot {
        loadedAt: "2026-08-20T05:00:00.000Z".to_owned(),
        runtime: LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::MaintenanceActive(
                weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActive {
                    bundle:
                        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActiveBundle {
                            bundleSha256: bundle_sha256.clone(),
                            candidate: maintenance_wind_candidate(),
                            maintenanceAuthority:
                                weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActiveBundleMaintenanceAuthority {
                                    actionSha256: "7".repeat(64),
                                    fullMemberRootSha256: "8".repeat(64),
                                    policyReportSha256: "9".repeat(64),
                                },
                            maintenanceBundleSha256: bundle_sha256.clone(),
                        },
                    comparatorAuthority: weather_contracts::MaintenanceShadowServingAuthority {
                        artifactBase64: Nullable::Value("Zml4dHVyZS1hcnRpZmFjdA==".to_owned()),
                        artifactIdentitySha256: Nullable::Value(bundle_sha256),
                        artifactMemberSha256: Nullable::Value("f".repeat(64)),
                        authorityKind:
                            weather_contracts::MaintenanceShadowComparatorAuthorityKind::MaintenanceQualified,
                        receiptBase64: "Zml4dHVyZS1yZWNlaXB0".to_owned(),
                        receiptMemberSha256: "0".repeat(64),
                    },
                    reasonCode:
                        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActiveReasonCodeNull,
                    state:
                        weather_contracts::LoadedForecastAdjustmentWindCanaryRuntimeMaintenanceActiveStateLiteral,
                },
            ),
        ),
    }
}

/// build one exact standard-qualified startup snapshot
fn standard_qualified_adjustment_startup_snapshot() -> ForecastAdjustmentStartupSnapshot {
    let mut candidate = maintenance_wind_candidate();
    let enabled_metric_bands = [
        ForecastAdjustmentMetric::RelativeHumidityPercent,
        ForecastAdjustmentMetric::TemperatureC,
        ForecastAdjustmentMetric::WindDirectionDegrees,
        ForecastAdjustmentMetric::WindGustMps,
        ForecastAdjustmentMetric::WindSpeedMps,
    ]
    .map(|metric| ForecastAdjustmentMetricBand {
        leadBand: ForecastLeadBandKey::Value001024,
        metric,
    })
    .to_vec();
    candidate.candidateArtifactSha256 = "b".repeat(64);
    candidate.coefficients = enabled_metric_bands
        .iter()
        .map(|pair| wind_root_coefficient(pair.metric, 0.25))
        .collect();
    candidate.enabledMetricBands = enabled_metric_bands.clone();
    candidate.trainingEnvelopes = enabled_metric_bands
        .iter()
        .filter_map(|pair| {
            let (minimum, maximum, metric) = match pair.metric {
                ForecastAdjustmentMetric::RelativeHumidityPercent => (
                    0.0,
                    100.0,
                    ForecastAdjustmentTrainingEnvelopeMetricEnum::RelativeHumidityPercent,
                ),
                ForecastAdjustmentMetric::TemperatureC => (
                    -50.0,
                    50.0,
                    ForecastAdjustmentTrainingEnvelopeMetricEnum::TemperatureC,
                ),
                // circular direction uses its dedicated calm guard instead of a scalar envelope
                ForecastAdjustmentMetric::WindDirectionDegrees => return None,
                ForecastAdjustmentMetric::WindGustMps => (
                    0.0,
                    30.0,
                    ForecastAdjustmentTrainingEnvelopeMetricEnum::WindGustMps,
                ),
                ForecastAdjustmentMetric::WindSpeedMps => (
                    0.0,
                    30.0,
                    ForecastAdjustmentTrainingEnvelopeMetricEnum::WindSpeedMps,
                ),
            };
            Some(ForecastAdjustmentTrainingEnvelope {
                leadBand: pair.leadBand,
                maximum,
                metric,
                minimum,
            })
        })
        .collect();

    let report_sha256 = "d".repeat(64);
    let receipt_sha256 = "e".repeat(64);
    let bundle = weather_contracts::ForecastAdjustmentRuntimeBundleV2 {
        evaluationReport: weather_contracts::ForecastAdjustmentEvaluationReportV2 {
            candidateArtifactSha256: candidate.candidateArtifactSha256.clone(),
            contractVersion:
                weather_contracts::ForecastAdjustmentEvaluationReportV2ContractVersionLiteral,
            enabledMetricBands: enabled_metric_bands.clone(),
            evaluationEpochId: candidate.evaluationEpochId.clone(),
            evaluationReportSha256: report_sha256.clone(),
            holdoutAccessMarkerSha256: "1".repeat(64),
            holdoutEndExclusive: "2026-08-01T00:00:00.000Z".to_owned(),
            holdoutEndLocalDate: "2026-07-31".to_owned(),
            holdoutStartInclusive: "2026-07-01T00:00:00.000Z".to_owned(),
            holdoutStartLocalDate: "2026-07-01".to_owned(),
            metricBandEvaluations: Vec::new(),
            preregistrationSha256: "2".repeat(64),
            trainingProvenance: candidate.trainingProvenance.clone(),
        },
        qualificationReceipt: weather_contracts::ForecastAdjustmentQualificationReceiptV2 {
            candidateArtifactSha256: candidate.candidateArtifactSha256.clone(),
            contractVersion:
                weather_contracts::ForecastAdjustmentQualificationReceiptV2ContractVersionLiteral,
            enabledMetricBands: enabled_metric_bands,
            evaluationEpochId: candidate.evaluationEpochId.clone(),
            evaluationReportSha256: report_sha256,
            evidenceRedundancy: weather_contracts::ForecastAdjustmentEvidenceRedundancy {
                attestationSha256: "3".repeat(64),
                status:
                    weather_contracts::ForecastAdjustmentEvidenceRedundancyStatusEnum::IndependentContentAddressedCopy,
                verified: true,
            },
            gates: Vec::new(),
            holdoutAccessMarkerSha256: "1".repeat(64),
            lifecycleState:
                weather_contracts::ForecastAdjustmentQualificationReceiptV2LifecycleStateEnum::Qualified,
            passed: true,
            preregistrationSha256: "2".repeat(64),
            qualificationReceiptSha256: receipt_sha256,
            trainingProvenance: candidate.trainingProvenance.clone(),
        },
        bundleSha256: "a".repeat(64),
        candidate,
        contractVersion: weather_contracts::ForecastAdjustmentRuntimeBundleV2ContractVersionLiteral,
        siteKey: weather_contracts::ForecastAdjustmentRuntimeBundleV2SiteKeyLiteral,
        timezone: weather_contracts::ForecastAdjustmentRuntimeBundleV2TimezoneLiteral,
    };

    ForecastAdjustmentStartupSnapshot {
        loadedAt: "2026-08-20T05:00:00.000Z".to_owned(),
        runtime: LoadedApiForecastAdjustmentRuntime::Standard(
            LoadedForecastAdjustmentRuntimeV1::Active(
                weather_contracts::LoadedForecastAdjustmentRuntimeV1Active {
                    bundle,
                    comparatorAuthority: weather_contracts::FieldPresence::Missing,
                    reasonCode:
                        weather_contracts::LoadedForecastAdjustmentRuntimeV1ActiveReasonCodeNull,
                    state: weather_contracts::LoadedForecastAdjustmentRuntimeV1ActiveStateLiteral,
                },
            ),
        ),
    }
}

/// build one finite generated startup snapshot around the active wind fixture
fn active_legacy_wind_startup_snapshot() -> ForecastAdjustmentStartupSnapshot {
    let candidate = active_wind_candidate();
    let enabled_metric_bands = candidate.enabledMetricBands.clone();
    let served_forecast_identity = candidate.servedForecastIdentity.clone();
    let training_forecast_identity = candidate.trainingForecastIdentity.clone();
    ForecastAdjustmentStartupSnapshot {
        loadedAt: "2026-08-20T05:00:00.000Z".to_owned(),
        runtime: LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
                value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Active(
                    LoadedForecastAdjustmentWindCanaryRuntimeV1Active {
                        bundle: ForecastAdjustmentWindCanaryRuntimeBundle::TimeLimited(
                            ForecastAdjustmentWindCanaryRuntimeBundleV1 {
                                artifactKind:
                                    ForecastAdjustmentWindCanaryRuntimeBundleV1ArtifactKindLiteral,
                                authorization: ForecastAdjustmentWindCanaryAuthorizationV1 {
                                    activatedAt: "2026-08-20T05:00:00.000Z".to_owned(),
                                    artifactKind:
                                        ForecastAdjustmentWindCanaryAuthorizationV1ArtifactKindLiteral,
                                    authorizationReason: "fixture authorization".to_owned(),
                                    authorizationSha256:
                                        "fc4895cb45ffb59a725ead664ae20924399585a9d2fa5f5d068f40589b523537"
                                            .to_owned(),
                                    authorized:
                                        ForecastAdjustmentWindCanaryAuthorizationV1AuthorizedLiteral,
                                    authorizedAt: "2026-08-20T05:00:00.000Z".to_owned(),
                                    authorizedBy: "fixture".to_owned(),
                                    candidateArtifactSha256:
                                        candidate.candidateArtifactSha256.clone(),
                                    contractVersion:
                                        ForecastAdjustmentWindCanaryAuthorizationV1ContractVersionLiteral,
                                    enabledMetricBands: enabled_metric_bands.clone(),
                                    expiresAt: "2026-08-24T05:00:00.000Z".to_owned(),
                                    transferReportSha256:
                                        "2c68ba8be5d5343a8d358cee7e21bb157f60c54e6c0bf2bc4b0978f50ec50582"
                                            .to_owned(),
                                },
                                bundleSha256:
                                    "5e8b2e3932111621af6785a1b16dfd22edc0a2d26059c6e396654c70119abbe1"
                                        .to_owned(),
                                candidate,
                                contractVersion:
                                    ForecastAdjustmentWindCanaryRuntimeBundleV1ContractVersionLiteral,
                                siteKey:
                                    ForecastAdjustmentWindCanaryRuntimeBundleV1SiteKeyLiteral,
                                timezone:
                                    ForecastAdjustmentWindCanaryRuntimeBundleV1TimezoneLiteral,
                                transferReport: ForecastAdjustmentWindCanaryTransferReportV1 {
                                    artifactKind:
                                        ForecastAdjustmentWindCanaryTransferReportV1ArtifactKindLiteral,
                                    bridgeEndExclusive: "2026-08-21T07:00:00.000Z".to_owned(),
                                    bridgeEvaluations: Vec::new(),
                                    bridgeStartInclusive: "2026-08-21T04:00:00.000Z".to_owned(),
                                    candidateArtifactSha256:
                                        "454e7b91a34407a6ace79150dfc5799c37ced2adbdab915b710f18180c3cd488"
                                            .to_owned(),
                                    contractVersion:
                                        ForecastAdjustmentWindCanaryTransferReportV1ContractVersionLiteral,
                                    enabledMetricBands: enabled_metric_bands,
                                    passed:
                                        ForecastAdjustmentWindCanaryTransferReportV1PassedLiteral,
                                    servedForecastIdentity: served_forecast_identity,
                                    trainingForecastIdentity: training_forecast_identity,
                                    transferReportSha256:
                                        "2c68ba8be5d5343a8d358cee7e21bb157f60c54e6c0bf2bc4b0978f50ec50582"
                                            .to_owned(),
                                },
                            },
                        ),
                        reasonCode: LoadedForecastAdjustmentWindCanaryRuntimeV1ActiveReasonCodeNull,
                        state: LoadedForecastAdjustmentWindCanaryRuntimeV1ActiveStateLiteral,
                    },
                ),
                comparatorAuthority: weather_contracts::FieldPresence::Missing,
            },
        ),
    }
}

/// move the finite fixture material into the generated permanent bundle shape
fn permanent_legacy_wind_startup_snapshot() -> ForecastAdjustmentStartupSnapshot {
    let finite = active_legacy_wind_startup_snapshot();
    // consume only the known active legacy fixture branch
    let LoadedApiForecastAdjustmentRuntime::Wind(
        LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
            value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Active(active),
            comparatorAuthority,
        },
    ) = finite.runtime
    else {
        panic!("finite fixture must remain an active legacy wind snapshot");
    };
    // preserve all owned candidate and report allocations while changing authorization kind
    let ForecastAdjustmentWindCanaryRuntimeBundle::TimeLimited(bundle) = active.bundle else {
        panic!("finite fixture must remain time limited");
    };
    let ForecastAdjustmentWindCanaryRuntimeBundleV1 {
        artifactKind,
        authorization,
        bundleSha256,
        candidate,
        siteKey,
        timezone,
        transferReport,
        ..
    } = bundle;
    let ForecastAdjustmentWindCanaryAuthorizationV1 {
        activatedAt,
        authorizationReason,
        authorizationSha256,
        authorizedAt,
        authorizedBy,
        candidateArtifactSha256,
        enabledMetricBands,
        transferReportSha256,
        ..
    } = authorization;

    ForecastAdjustmentStartupSnapshot {
        loadedAt: finite.loadedAt,
        runtime: LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
                value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Active(
                    LoadedForecastAdjustmentWindCanaryRuntimeV1Active {
                        bundle: ForecastAdjustmentWindCanaryRuntimeBundle::Permanent(
                            ForecastAdjustmentWindCanaryRuntimeBundleV2 {
                                artifactKind,
                                bundleSha256,
                                candidate,
                                siteKey,
                                timezone,
                                transferReport,
                                authorization: ForecastAdjustmentWindCanaryAuthorizationV2 {
                                    activatedAt,
                                    artifactKind:
                                        ForecastAdjustmentWindCanaryAuthorizationV2ArtifactKindLiteral,
                                    authorizationReason,
                                    authorizationSha256,
                                    authorized:
                                        ForecastAdjustmentWindCanaryAuthorizationV2AuthorizedLiteral,
                                    authorizedAt,
                                    authorizedBy,
                                    candidateArtifactSha256,
                                    contractVersion:
                                        ForecastAdjustmentWindCanaryAuthorizationV2ContractVersionLiteral,
                                    enabledMetricBands,
                                    expiresAt:
                                        ForecastAdjustmentWindCanaryAuthorizationV2ExpiresAtNull,
                                    permanent:
                                        ForecastAdjustmentWindCanaryAuthorizationV2PermanentLiteral,
                                    transferReportSha256,
                                },
                                contractVersion:
                                    ForecastAdjustmentWindCanaryRuntimeBundleV2ContractVersionLiteral,
                            },
                        ),
                        reasonCode: active.reasonCode,
                        state: active.state,
                    },
                ),
                comparatorAuthority,
            },
        ),
    }
}

/// Build one root wind coefficient for the service-level model fixture.
fn wind_root_coefficient(
    metric: ForecastAdjustmentMetric,
    coefficient: f64,
) -> ForecastAdjustmentCoefficient {
    ForecastAdjustmentCoefficient {
        coefficient,
        daypart: Nullable::Null,
        effectiveEventCount: 100.0,
        leadBand: ForecastLeadBandKey::Value001024,
        level: ForecastAdjustmentCoefficientLevelEnum::Value1,
        metric,
        month: Nullable::Null,
        season: Nullable::Null,
    }
}

/// build one admitted active temperature runtime matching the Node fixture
fn active_temperature_runtime() -> AdmittedTemperatureCanaryRuntime {
    AdmittedTemperatureCanaryRuntime {
        authorization: AdmittedTemperatureCanaryAuthorization::TimeLimited {
            activated_at_epoch_ms: 1_787_184_000_000,
            authorization_sha256: "b".repeat(64),
            expires_at: instant(1_788_048_000_000, "2026-08-30T00:00:00.000Z"),
        },
        evidence: AdmittedTemperatureCanaryEvidence {
            active_bundle_sha256: "a".repeat(64),
        },
        maximum_receipt_age_hours: 12,
        source_delay_hours: 6,
    }
}

/// build the admitted intercept-only delayed model used by the compiled Node fixture
fn temperature_mos_model() -> TemperatureMosServingModel {
    let mut direct_coefficients = vec![0.0; 35];
    direct_coefficients[0] = 1.0;
    TemperatureMosServingModel::Monthly(TemperatureMosRuntimeModel {
        adaptiveCoefficients: Nullable::Value(vec![0.0; 49]),
        cohort: TemperatureMosRuntimeModelMaterialCohortLiteral,
        directCoefficients: Nullable::Value(direct_coefficients),
        learnedStrengthContractVersion:
            TemperatureMosRuntimeModelMaterialLearnedStrengthContractVersionLiteral,
        scope: TemperatureMosRuntimeModelMaterialScopeEnum::AssumedDelay6Next12,
        strengthBands: TemperatureMosRuntimeModelMaterialStrengthBands {
            hours1To6: temperature_mos_strength_band(),
            hours7To12: temperature_mos_strength_band(),
        },
        supported: true,
        trainingCutoffUtc: "2026-07-25T07:00:00.000Z".to_owned(),
        contractVersion: TemperatureMosRuntimeModelContractVersionLiteral,
        month: "2026-08".to_owned(),
    })
}

/// build one finite generated startup snapshot around the monthly temperature fixture
fn active_legacy_temperature_startup_snapshot() -> ForecastTemperatureAdjustmentStartupSnapshot {
    // move the existing monthly fixture into the generated finite bundle
    let TemperatureMosServingModel::Monthly(model) = temperature_mos_model() else {
        panic!("temperature startup fixture must remain monthly")
    };
    ForecastTemperatureAdjustmentStartupSnapshot {
        loadedAt: "2026-08-22T04:59:00.000Z".to_owned(),
        runtime: LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy {
            value: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Active(
                weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeV1Active {
                    bundle:
                        weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundle::TimeLimited(
                            weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1 {
                                artifactKind:
                                    weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ArtifactKindLiteral,
                                authorization:
                                    weather_contracts::ForecastAdjustmentTemperatureCanaryAuthorizationV1 {
                                        activatedAt: "2026-08-20T05:00:00.000Z".to_owned(),
                                        authorizationReason: "fixture authorization".to_owned(),
                                        authorizationSha256: "b".repeat(64),
                                        authorized:
                                            weather_contracts::ForecastAdjustmentTemperatureCanaryAuthorizationV1AuthorizedLiteral,
                                        authorizedAt: "2026-08-20T05:00:00.000Z".to_owned(),
                                        authorizedBy: "fixture".to_owned(),
                                        expiresAt: "2026-08-24T05:00:00.000Z".to_owned(),
                                    },
                                bundleSha256: "a".repeat(64),
                                contractVersion:
                                    weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ContractVersionLiteral,
                                evidence:
                                    weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1Evidence {
                                        modelSourceSha256: "c".repeat(64),
                                        researchSummarySha256: "d".repeat(64),
                                        retentionManifestSha256: "e".repeat(64),
                                        strengthSourceSha256: "f".repeat(64),
                                    },
                                model,
                                runtimeFingerprint:
                                    weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1RuntimeFingerprint {
                                        icuVersion: "78.3".to_owned(),
                                        tzdataVersion: "2026b".to_owned(),
                                    },
                                servedForecastIdentity:
                                    weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentity {
                                        adapterVersion:
                                            weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityAdapterVersionLiteral,
                                        dataset:
                                            weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityDatasetLiteral,
                                        maximumReceiptAgeHours: 12,
                                        providerKey:
                                            weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityProviderKeyLiteral,
                                        sourceDelayHours:
                                            weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentitySourceDelayHoursLiteral,
                                        upstreamModel:
                                            weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1ServedForecastIdentityUpstreamModelLiteral,
                                    },
                                siteKey:
                                    weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1SiteKeyLiteral,
                                timezone:
                                    weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TimezoneLiteral,
                                trainingForecastIdentity:
                                    weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentity {
                                        cohort:
                                            weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentityCohortLiteral,
                                        scope:
                                            weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundleV1TrainingForecastIdentityScopeLiteral,
                                    },
                            },
                        ),
                    reasonCode:
                        weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeV1ActiveReasonCodeNull,
                    state:
                        weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeV1ActiveStateLiteral,
                },
            ),
            comparatorAuthority: weather_contracts::FieldPresence::Missing,
        },
    }
}

/// adapt the existing monthly numerical fixture into one causally valid permanent model
fn permanent_temperature_mos_model() -> weather_contracts::TemperatureMosPermanentRuntimeModel {
    // move the existing coefficients and strength material without duplicating it
    let TemperatureMosServingModel::Monthly(mut model) = temperature_mos_model() else {
        panic!("temperature maintenance fixture must start from the monthly model")
    };
    model.trainingCutoffUtc = "2026-08-01T07:00:00.000Z".to_owned();
    model.strengthBands.hours1To6.trainingCutoffUtc = model.trainingCutoffUtc.clone();
    model.strengthBands.hours7To12.trainingCutoffUtc = model.trainingCutoffUtc.clone();
    weather_contracts::TemperatureMosPermanentRuntimeModel {
        adaptiveCoefficients: model.adaptiveCoefficients,
        cohort: model.cohort,
        directCoefficients: model.directCoefficients,
        learnedStrengthContractVersion: model.learnedStrengthContractVersion,
        scope: model.scope,
        strengthBands: model.strengthBands,
        supported: model.supported,
        trainingCutoffUtc: model.trainingCutoffUtc,
        contractVersion:
            weather_contracts::TemperatureMosPermanentRuntimeModelContractVersionLiteral,
        effectiveFrom: "2026-08-20T05:00:00.000Z".to_owned(),
        latestTrainingValidAt: "2026-07-25T07:00:00.000Z".to_owned(),
    }
}

/// build one generated maintenance-qualified snapshot around the permanent fixture
fn maintenance_qualified_temperature_startup_snapshot()
-> ForecastTemperatureAdjustmentStartupSnapshot {
    let bundle_sha256 = "6".repeat(64);
    ForecastTemperatureAdjustmentStartupSnapshot {
        loadedAt: "2026-08-22T04:59:00.000Z".to_owned(),
        runtime: LoadedForecastAdjustmentTemperatureCanaryRuntime::MaintenanceActive(
            weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActive {
                bundle:
                    weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundle {
                        bundleSha256: bundle_sha256.clone(),
                        maintenanceAuthority:
                            weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleMaintenanceAuthority {
                                actionSha256: "7".repeat(64),
                                fullMemberRootSha256: "8".repeat(64),
                                policyReportSha256: "9".repeat(64),
                            },
                        maintenancePackage:
                            weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleMaintenancePackageLiteral,
                        model: permanent_temperature_mos_model(),
                        servedForecastIdentity:
                            weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentity {
                                adapterVersion:
                                    weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityAdapterVersionLiteral,
                                dataset:
                                    weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityDatasetLiteral,
                                maximumReceiptAgeHours:
                                    weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityMaximumReceiptAgeHoursLiteral,
                                providerKey:
                                    weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityProviderKeyLiteral,
                                sourceDelayHours:
                                    weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentitySourceDelayHoursLiteral,
                                upstreamModel:
                                    weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleServedForecastIdentityUpstreamModelLiteral,
                            },
                        trainingForecastIdentity:
                            weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleTrainingForecastIdentity {
                                cohort:
                                    weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleTrainingForecastIdentityCohortLiteral,
                                scope:
                                    weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveBundleTrainingForecastIdentityScopeLiteral,
                            },
                    },
                comparatorAuthority: weather_contracts::MaintenanceShadowServingAuthority {
                    artifactBase64: Nullable::Value("Zml4dHVyZS1hcnRpZmFjdA==".to_owned()),
                    artifactIdentitySha256: Nullable::Value(bundle_sha256),
                    artifactMemberSha256: Nullable::Value("c".repeat(64)),
                    authorityKind:
                        weather_contracts::MaintenanceShadowComparatorAuthorityKind::MaintenanceQualified,
                    receiptBase64: "Zml4dHVyZS1yZWNlaXB0".to_owned(),
                    receiptMemberSha256: "d".repeat(64),
                },
                reasonCode:
                    weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveReasonCodeNull,
                state:
                    weather_contracts::LoadedForecastAdjustmentTemperatureCanaryRuntimeMaintenanceActiveStateLiteral,
            },
        ),
    }
}

/// build one admitted fully supported learned-strength band
fn temperature_mos_strength_band() -> TemperatureMosRuntimeStrengthBand {
    TemperatureMosRuntimeStrengthBand {
        alpha: 1.0,
        supported: true,
        trainingCutoffUtc: "2026-07-25T07:00:00.000Z".to_owned(),
    }
}

/// Keep the original source provenance while causally out-of-scope MOS stays raw.
#[test]
fn temperature_mos_does_not_apply_with_same_run_cutoff_or_detached_causal_state() {
    let boundary = FixtureBoundary;
    let admission = FixtureTemperatureMosAdmission;
    let runtime = active_temperature_runtime();
    let mut row = forecast_row("2026-08-22T06:00:00.000Z", "forecast-public");
    row.upstreamModel = Nullable::Value("best_match".to_owned());
    row.temperatureC = Nullable::Value(12.1);
    let mut input = weather_api::temperature_canary_inputs(
        &[row],
        Some(&temperature_sidecar()),
        &instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z"),
        &boundary,
    )
    .expect("typed source input")
    .pop()
    .expect("one modeled hour");
    let mut future_model = temperature_mos_model();
    let TemperatureMosServingModel::Monthly(model) = &mut future_model else {
        panic!("monthly model fixture");
    };
    model.trainingCutoffUtc = "2026-08-21T18:00:00.000Z".to_owned();
    let stale_owner = AdmittedTemperatureMosModelOwner {
        model: &future_model,
        origin: AdmittedTemperatureMosModelOrigin::admitted(&runtime.evidence.active_bundle_sha256),
        served_adapter_version: "open-meteo-ecmwf-single-run/v1",
        time_resolver: &boundary,
        admission: &admission,
    };
    let same_run = stale_owner
        .apply(&runtime, &input, Some("fixture-state"))
        .expect("decision");
    assert_eq!(
        same_run.reasonCode,
        Nullable::Value(TemperatureCanaryDecisionReasonValue::ModelNotYetAvailable)
    );
    assert_eq!(same_run.correctedTemperatureC, Nullable::Null);

    let model = temperature_mos_model();
    let owner = AdmittedTemperatureMosModelOwner {
        model: &model,
        ..stale_owner
    };
    let Nullable::Value(state) = &mut input.recentErrorState else {
        panic!("rolling state fixture");
    };
    state.windowEndValidAt = "2026-08-21T07:00:00.000Z".to_owned();
    let detached = owner
        .apply(&runtime, &input, Some("fixture-state"))
        .expect("decision");
    assert_eq!(
        detached.reasonCode,
        Nullable::Value(TemperatureCanaryDecisionReasonValue::RecentErrorStateAsOfMismatch)
    );
    assert_eq!(detached.rawBestMatchTemperatureC, Nullable::Value(12.1));
    assert_eq!(detached.correctedTemperatureC, Nullable::Null);
    assert!(matches!(detached.sourceForecast, Nullable::Value(_)));
    assert_eq!(
        detached.recentErrorStateSha256,
        Nullable::Value("fixture-state".to_owned())
    );
}

/// execute one forecast with an admitted constructor-snapshotted wind runtime
fn active_wind_request(
    repository: &mut FixtureRepository,
    clock: &mut dyn WeatherServiceClock,
    boundary: &FixtureBoundary,
    adjustment: &dyn ForecastWindAdjustmentBoundary,
    runtime: &AdmittedWindCanaryRuntime,
) -> weather_api::WeatherServiceResponse {
    let nominal_data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let loaded_at = instant(1_787_202_000_000, "2026-08-20T05:00:00.000Z");
    let inactive_runtime = inactive_forecast_runtime();
    serve_decoded_request(
        repository,
        clock,
        &WeatherServiceBoundaries {
            nominal_data_at: Some(&nominal_data_at),
            time_resolver: boundary,
            history: boundary,
            tides: boundary,
            forecast: boundary,
            forecast_runtime: &inactive_runtime,
            forecast_wind_runtime: Some(AdmittedWindForecastServiceRuntime::Legacy {
                loaded_at: &loaded_at,
                runtime,
                adjustment,
            }),
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: boundary,
            daily_precipitation: boundary,
            rain_collection: boundary,
            forecast_observation: boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/sites/ballydidean/forecast",
            query: &[],
        },
    )
}

/// execute one health read with an admitted constructor-snapshotted wind runtime
fn admitted_wind_health_request(
    repository: &mut FixtureRepository,
    clock: &mut dyn WeatherServiceClock,
    boundary: &FixtureBoundary,
    adjustment: &dyn ForecastWindAdjustmentBoundary,
    runtime: &AdmittedWindCanaryRuntime,
) -> weather_api::WeatherServiceResponse {
    let loaded_at = instant(1_787_202_000_000, "2026-08-20T05:00:00.000Z");
    let inactive_runtime = inactive_forecast_runtime();
    serve_decoded_request(
        repository,
        clock,
        &WeatherServiceBoundaries {
            nominal_data_at: None,
            time_resolver: boundary,
            history: boundary,
            tides: boundary,
            forecast: boundary,
            forecast_runtime: &inactive_runtime,
            forecast_wind_runtime: Some(AdmittedWindForecastServiceRuntime::Legacy {
                loaded_at: &loaded_at,
                runtime,
                adjustment,
            }),
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: boundary,
            daily_precipitation: boundary,
            rain_collection: boundary,
            forecast_observation: boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/health",
            query: &[],
        },
    )
}

/// execute one forecast with an admitted constructor-snapshotted temperature runtime
fn active_temperature_request(
    repository: &mut FixtureRepository,
    clock: &mut dyn WeatherServiceClock,
    boundary: &FixtureBoundary,
    adjustment: &dyn ForecastTemperatureAdjustmentBoundary,
    runtime: &AdmittedTemperatureCanaryRuntime,
) -> weather_api::WeatherServiceResponse {
    let nominal_data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let loaded_at = instant(1_787_374_740_000, "2026-08-22T04:59:00.000Z");
    let inactive_runtime = inactive_forecast_runtime();
    serve_decoded_request(
        repository,
        clock,
        &WeatherServiceBoundaries {
            nominal_data_at: Some(&nominal_data_at),
            time_resolver: boundary,
            history: boundary,
            tides: boundary,
            forecast: boundary,
            forecast_runtime: &inactive_runtime,
            forecast_wind_runtime: None,
            forecast_temperature_runtime: Some(AdmittedTemperatureForecastServiceRuntime {
                loaded_at: &loaded_at,
                runtime,
                adjustment,
            }),
            forecast_rain_runtime: None,
            trends: boundary,
            daily_precipitation: boundary,
            rain_collection: boundary,
            forecast_observation: boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/sites/ballydidean/forecast",
            query: &[],
        },
    )
}

/// execute one health read with an admitted constructor-snapshotted temperature runtime
fn admitted_temperature_health_request(
    repository: &mut FixtureRepository,
    clock: &mut dyn WeatherServiceClock,
    boundary: &FixtureBoundary,
    adjustment: &dyn ForecastTemperatureAdjustmentBoundary,
    runtime: &AdmittedTemperatureCanaryRuntime,
) -> weather_api::WeatherServiceResponse {
    let loaded_at = instant(1_787_374_740_000, "2026-08-22T04:59:00.000Z");
    let inactive_runtime = inactive_forecast_runtime();
    serve_decoded_request(
        repository,
        clock,
        &WeatherServiceBoundaries {
            nominal_data_at: None,
            time_resolver: boundary,
            history: boundary,
            tides: boundary,
            forecast: boundary,
            forecast_runtime: &inactive_runtime,
            forecast_wind_runtime: None,
            forecast_temperature_runtime: Some(AdmittedTemperatureForecastServiceRuntime {
                loaded_at: &loaded_at,
                runtime,
                adjustment,
            }),
            forecast_rain_runtime: None,
            trends: boundary,
            daily_precipitation: boundary,
            rain_collection: boundary,
            forecast_observation: boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/health",
            query: &[],
        },
    )
}

/// execute one forecast with an admitted constructor-snapshotted rain runtime
fn admitted_rain_request(
    repository: &mut FixtureRepository,
    clock: &mut dyn WeatherServiceClock,
    boundary: &FixtureBoundary,
    method: &str,
    runtime: &AdmittedRainForecastRuntime,
) -> weather_api::WeatherServiceResponse {
    let nominal_data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let inactive_runtime = inactive_forecast_runtime();
    serve_decoded_request(
        repository,
        clock,
        &WeatherServiceBoundaries {
            nominal_data_at: Some(&nominal_data_at),
            time_resolver: boundary,
            history: boundary,
            tides: boundary,
            forecast: boundary,
            forecast_runtime: &inactive_runtime,
            forecast_wind_runtime: None,
            forecast_temperature_runtime: None,
            forecast_rain_runtime: Some(runtime),
            trends: boundary,
            daily_precipitation: boundary,
            rain_collection: boundary,
            forecast_observation: boundary,
            version: None,
        },
        DecodedServiceRequest {
            method,
            pathname: "/api/v1/sites/ballydidean/forecast",
            query: &[],
        },
    )
}

/// read one public error code from a typed service response
fn error_code(response: &weather_api::WeatherServiceResponse) -> &str {
    let Some(WeatherServiceBody::Error(body)) = &response.body else {
        panic!("service response must contain an error body");
    };
    &body.error.code
}

// verifies compiled Node ordering and the integrated typed plan execution path
#[test]
fn matches_compiled_node_service_dispatch() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/service-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run service Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    assert!(stdout.contains("statuses=200|200|200|200|405|404|404|400|500"));

    let boundary = FixtureBoundary;
    let mut repository = FixtureRepository::default();
    let mut clock = FixtureClock { reads: 0 };
    let sites = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/sites",
        &[],
    );
    let current = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/sites/ballydidean/current",
        &[QueryParameter {
            name: "source",
            value: "11",
        }],
    );
    let tides = request(
        &mut repository,
        &mut clock,
        &boundary,
        "HEAD",
        "/api/v1/sites/ballydidean/tides",
        &[],
    );
    let history = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/sites/ballydidean/history",
        &[QueryParameter {
            name: "limit",
            value: "1",
        }],
    );
    let method = request(
        &mut repository,
        &mut clock,
        &boundary,
        "POST",
        "/api/v1/sites/ballydidean/current",
        &[],
    );
    let missing_route = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/missing",
        &[],
    );
    let missing_site = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/sites/elsewhere/current",
        &[],
    );
    let invalid_query = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/sites/ballydidean/current",
        &[QueryParameter {
            name: "unsupported",
            value: "true",
        }],
    );
    repository.fail_history = true;
    let database_failure = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/sites/ballydidean/history",
        &[QueryParameter {
            name: "limit",
            value: "1",
        }],
    );

    assert_eq!(
        [
            sites.status,
            current.status,
            tides.status,
            history.status,
            method.status,
            missing_route.status,
            missing_site.status,
            invalid_query.status,
            database_failure.status,
        ],
        [200, 200, 200, 200, 405, 404, 404, 400, 500]
    );
    assert_eq!(clock.reads, 8);
    assert_eq!(repository.site_reads, 7);
    assert_eq!(tides.body, None);
    assert_eq!(error_code(&method), "method_not_allowed");
    assert_eq!(error_code(&missing_route), "not_found");
    assert_eq!(error_code(&missing_site), "site_not_found");
    assert_eq!(error_code(&invalid_query), "invalid_query");
    assert_eq!(error_code(&database_failure), "internal_error");

    let Some(WeatherServiceBody::Sites(body)) = sites.body else {
        panic!("sites response body is missing");
    };
    assert_eq!(body.data[0].stations[0].sources[0].id, "11");
    let Some(WeatherServiceBody::Current(body)) = current.body else {
        panic!("current response body is missing");
    };
    assert_eq!(body.generatedAt, "2026-08-22T05:00:00.000Z");
    let Some(WeatherServiceBody::History(body)) = history.body else {
        panic!("history response body is missing");
    };
    assert_eq!(body.page.limit, 1);
    assert_eq!(body.page.nextCursor, Nullable::Null);

    assert_eq!(
        repository.current_plans[0].binds,
        [
            CurrentWeatherBind::Text("ballydidean".to_owned()),
            CurrentWeatherBind::NullableText(None),
            CurrentWeatherBind::NullableText(Some("11".to_owned())),
            CurrentWeatherBind::UtcInstant("2026-08-22T05:00:00.000Z".to_owned()),
        ]
    );
    assert_eq!(
        repository.history_plans[0].binds.last(),
        Some(&HistoryReadBind::Limit(2))
    );
    assert_eq!(
        repository.tide_plans[0].binds,
        [
            TideReadBind::Text("ballydidean".to_owned()),
            TideReadBind::UtcInstant("2026-08-21T05:00:00.000Z".to_owned()),
            TideReadBind::UtcInstant("2026-09-01T05:00:00.000Z".to_owned()),
            TideReadBind::Limit(10_000),
        ]
    );
}

// verifies the private route stays closed to public dispatch and HEAD errors omit bodies
#[test]
fn exposes_incomplete_routes_and_head_omission() {
    let boundary = FixtureBoundary;
    let mut repository = FixtureRepository::default();
    let mut clock = FixtureClock { reads: 0 };
    let unsupported = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/internal/v1/forecast-observation-input",
        &[],
    );
    assert_eq!(unsupported.status, 404);
    assert_eq!(error_code(&unsupported), "not_found");
    assert_eq!(repository.site_reads, 0);
    assert_eq!(clock.reads, 0);

    let missing = request(
        &mut repository,
        &mut clock,
        &boundary,
        "HEAD",
        "/api/v1/sites/elsewhere/current",
        &[],
    );
    assert_eq!(missing.status, 404);
    assert_eq!(missing.body, None);
}

// verifies a nominal construction snapshot overrides data time but not the safety clock read
#[test]
fn preserves_divergent_nominal_data_and_request_safety_times() {
    let boundary = FixtureBoundary;
    let nominal_data_at = instant(1_787_202_000_000, "2026-08-20T05:00:00.000Z");
    let mut repository = FixtureRepository::default();
    let mut clock = FixtureClock { reads: 0 };
    let forecast_runtime = inactive_forecast_runtime();
    let response = serve_decoded_request(
        &mut repository,
        &mut clock,
        &WeatherServiceBoundaries {
            nominal_data_at: Some(&nominal_data_at),
            time_resolver: &boundary,
            history: &boundary,
            tides: &boundary,
            forecast: &boundary,
            forecast_runtime: &forecast_runtime,
            forecast_wind_runtime: None,
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: &boundary,
            daily_precipitation: &boundary,
            rain_collection: &boundary,
            forecast_observation: &boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/sites/ballydidean/current",
            query: &[QueryParameter {
                name: "source",
                value: "11",
            }],
        },
    );

    assert_eq!(clock.reads, 1);
    assert_eq!(response.status, 200);
    assert_eq!(
        repository.current_plans[0].binds[3],
        CurrentWeatherBind::UtcInstant("2026-08-20T05:00:00.000Z".to_owned())
    );
    let Some(WeatherServiceBody::Current(body)) = response.body else {
        panic!("divergent current response body is missing");
    };
    assert_eq!(body.generatedAt, "2026-08-20T05:00:00.000Z");
}

// verifies forecast and trends execute exact plans, projections, and optional fail-raw context
#[test]
fn serves_forecast_and_trends_with_distinct_lifecycle_clocks() {
    let boundary = FixtureBoundary;
    let mut repository = FixtureRepository {
        fail_pressure: true,
        ..FixtureRepository::default()
    };
    let mut clock = FixtureClock { reads: 0 };
    let forecast = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/sites/ballydidean/forecast",
        &[],
    );
    let trends = request(
        &mut repository,
        &mut clock,
        &boundary,
        "HEAD",
        "/api/v1/sites/ballydidean/trends",
        &[QueryParameter {
            name: "range",
            value: "7d",
        }],
    );

    assert_eq!([forecast.status, trends.status], [200, 200]);
    assert_eq!(trends.body, None);
    assert_eq!(clock.reads, 2);
    assert_eq!(repository.site_reads, 2);
    assert_eq!(repository.forecast_plans.len(), 1);
    assert_eq!(repository.pressure_plans.len(), 1);
    assert_eq!(repository.trend_plans.len(), 1);
    assert_eq!(
        repository.forecast_plans[0].binds,
        [
            ForecastReadBind::Text("ballydidean".to_owned()),
            ForecastReadBind::UtcInstant("2026-08-20T19:00:00.000Z".to_owned()),
            ForecastReadBind::UtcInstant("2026-08-22T08:00:00.000Z".to_owned()),
        ]
    );
    assert_eq!(
        repository.pressure_plans[0].binds,
        [
            ForecastPressureContextBind::Text("ballydidean".to_owned()),
            ForecastPressureContextBind::UtcInstant("2026-08-21T04:00:00.000Z".to_owned()),
            ForecastPressureContextBind::UtcInstant("2026-08-21T10:00:00.000Z".to_owned()),
            ForecastPressureContextBind::UtcInstant("2026-08-19T07:00:00.000Z".to_owned()),
            ForecastPressureContextBind::UtcInstant("2026-08-21T07:00:00.000Z".to_owned()),
        ]
    );
    assert_eq!(
        repository.trend_plans[0].binds,
        [
            TrendReadBind::Text("ballydidean".to_owned()),
            TrendReadBind::UtcInstant("2019-01-01T08:00:00.000Z".to_owned()),
            TrendReadBind::UtcInstant("2026-08-22T05:00:00.000Z".to_owned()),
        ]
    );

    let Some(WeatherServiceBody::Forecast(body)) = forecast.body else {
        panic!("forecast response body is missing");
    };
    assert_eq!(body.days, ForecastDays::Value1);
    assert_eq!(body.generatedAt, "2026-08-22T05:00:00.000Z");
    assert_eq!(body.data.len(), 1);
    assert_eq!(body.data[0].id, "forecast-public");
    assert_eq!(body.airQualityContext.len(), 1);
    assert_eq!(body.airQualityContext[0].id, "forecast-context");
    assert!(body.pressureContext.is_empty());
    assert_eq!(body.adjustmentRuntime.loadedAt, "2026-08-20T05:00:00.000Z");
    assert_eq!(
        body.temperatureAdjustmentRuntime.loadedAt,
        "2026-08-20T06:00:00.000Z"
    );
    assert_eq!(
        body.rainAdjustmentRuntime.loadedAt,
        "2026-08-22T05:00:00.000Z"
    );
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(decision)) =
        &body.data[0].adjustment
    else {
        panic!("forecast row must preserve the inactive fail-raw decision");
    };
    assert_eq!(
        decision.state,
        ForecastAdjustmentFailRawDecisionStateEnum::Disabled
    );
}

// verifies typed startup refusals override supplied active models in health and forecast
#[test]
fn serves_cached_startup_refusals_without_optional_model_reads() {
    let boundary = FixtureBoundary;
    let startup = refused_forecast_startup_snapshots();
    let inactive_runtime = InactiveForecastServiceRuntime::from_startup(&startup);
    let wind_runtime = active_wind_runtime();
    let wind_candidate = active_wind_candidate();
    let wind_admission = FixtureWindAdmission {
        calls: Cell::new(0),
        fail_public: false,
        identity_mismatch: false,
    };
    let wind_adjustment = AdmittedWindModelOwner {
        candidate: &wind_candidate,
        evidence: &wind_runtime.evidence,
        admission: &wind_admission,
    };
    let temperature_runtime = active_temperature_runtime();
    let temperature_model = temperature_mos_model();
    let temperature_admission = FixtureTemperatureMosAdmission;
    let temperature_adjustment = AdmittedTemperatureMosModelOwner {
        model: &temperature_model,
        origin: AdmittedTemperatureMosModelOrigin::admitted(
            &temperature_runtime.evidence.active_bundle_sha256,
        ),
        served_adapter_version: "open-meteo-ecmwf-single-run/v1",
        time_resolver: &boundary,
        admission: &temperature_admission,
    };
    let nominal_data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let wind_loaded_at = instant(1_787_202_000_000, "2026-08-20T05:00:00.000Z");
    let temperature_loaded_at = instant(1_787_374_740_000, "2026-08-22T04:59:00.000Z");
    let boundaries = || WeatherServiceBoundaries {
        nominal_data_at: Some(&nominal_data_at),
        time_resolver: &boundary,
        history: &boundary,
        tides: &boundary,
        forecast: &boundary,
        forecast_runtime: &inactive_runtime,
        forecast_wind_runtime: Some(AdmittedWindForecastServiceRuntime::Legacy {
            loaded_at: &wind_loaded_at,
            runtime: &wind_runtime,
            adjustment: &wind_adjustment,
        }),
        forecast_temperature_runtime: Some(AdmittedTemperatureForecastServiceRuntime {
            loaded_at: &temperature_loaded_at,
            runtime: &temperature_runtime,
            adjustment: &temperature_adjustment,
        }),
        forecast_rain_runtime: None,
        trends: &boundary,
        daily_precipitation: &boundary,
        rain_collection: &boundary,
        forecast_observation: &boundary,
        version: None,
    };

    let mut health_repository = FixtureRepository::default();
    let mut health_clock = FixtureClock { reads: 0 };
    let health = serve_decoded_request(
        &mut health_repository,
        &mut health_clock,
        &boundaries(),
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/health",
            query: &[],
        },
    );
    let Some(WeatherServiceBody::Health(health)) = health.body else {
        panic!("startup-refused health response body is missing");
    };
    assert_eq!(
        health.data.adjustmentRuntime.loadedAt,
        "2026-08-20T04:57:00.000Z"
    );
    assert_eq!(
        health.data.adjustmentRuntime.reasonCode,
        Nullable::Value(ApiForecastAdjustmentRuntimeReasonCodeEnum::RegistryInactive)
    );
    assert_eq!(
        health.data.adjustmentRuntime.state,
        ApiForecastAdjustmentRuntimeStateEnum::Disabled
    );
    assert_eq!(health.data.adjustmentRuntime.activeBundle, Nullable::Null);
    assert_eq!(
        health.data.temperatureAdjustmentRuntime.loadedAt,
        "2026-08-20T04:58:00.000Z"
    );
    assert_eq!(
        health.data.temperatureAdjustmentRuntime.reasonCode,
        Nullable::Value(ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::BundleInvalid)
    );
    assert_eq!(
        health.data.temperatureAdjustmentRuntime.state,
        ApiForecastTemperatureAdjustmentRuntimeStateEnum::Disabled
    );
    assert_eq!(
        health.data.temperatureAdjustmentRuntime.activeBundle,
        Nullable::Null
    );
    assert_eq!(
        health.data.temperatureAdjustmentRuntime.source,
        Nullable::Null
    );
    assert!(health_repository.temperature_status_reads.is_empty());

    let mut forecast_repository = FixtureRepository {
        temperature_fixture: true,
        ..FixtureRepository::default()
    };
    let mut forecast_clock = FixtureClock { reads: 0 };
    let forecast = serve_decoded_request(
        &mut forecast_repository,
        &mut forecast_clock,
        &boundaries(),
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/sites/ballydidean/forecast",
            query: &[],
        },
    );
    let Some(WeatherServiceBody::Forecast(forecast)) = forecast.body else {
        panic!("startup-refused forecast response body is missing");
    };
    assert_eq!(
        forecast.adjustmentRuntime.loadedAt,
        "2026-08-20T04:57:00.000Z"
    );
    assert_eq!(
        forecast.adjustmentRuntime.reasonCode,
        Nullable::Value(ApiForecastAdjustmentRuntimeReasonCodeEnum::RegistryInactive)
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.loadedAt,
        "2026-08-20T04:58:00.000Z"
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.reasonCode,
        Nullable::Value(ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::BundleInvalid)
    );
    assert_eq!(forecast.temperatureAdjustmentRuntime.source, Nullable::Null);
    // preserve startup refusal on public and context rows
    for row in forecast.data.iter().chain(&forecast.airQualityContext) {
        let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(
            wind_decision,
        )) = &row.adjustment
        else {
            panic!("startup-refused wind row must remain fail raw");
        };
        assert_eq!(
            wind_decision.reasonCode,
            ForecastAdjustmentReasonCode::RegistryInactive
        );
        assert_eq!(
            wind_decision.state,
            ForecastAdjustmentFailRawDecisionStateEnum::Disabled
        );
        assert_eq!(
            row.temperatureAdjustment.reasonCode,
            Nullable::Value(TemperatureCanaryDecisionReasonValue::BundleInvalid)
        );
        assert_eq!(
            row.temperatureAdjustment.state,
            ForecastTemperatureCanaryDecisionV1StateEnum::Disabled
        );
    }
    assert_eq!(
        forecast.data[0]
            .temperatureAdjustment
            .rawBestMatchTemperatureC,
        Nullable::Value(12.1)
    );
    assert_eq!(
        forecast.data[0].temperatureAdjustment.correctedTemperatureC,
        Nullable::Null
    );
    assert!(forecast_repository.temperature_sidecar_reads.is_empty());
    assert!(forecast_repository.temperature_status_reads.is_empty());
}

// verifies generated v2 authorization stays open ended in the native selector
#[test]
fn selects_permanent_legacy_wind_startup_without_an_expiry() {
    let snapshot = permanent_legacy_wind_startup_snapshot();
    let startup = LegacyWindForecastStartup::from_snapshot(&snapshot, &FixtureBoundary)
        .expect("valid permanent legacy wind startup timestamps")
        .expect("active permanent legacy wind startup");

    assert!(matches!(
        startup.runtime().authorization,
        WindCanaryAuthorizationWindow::Permanent {
            activated_at_epoch_ms: 1_787_202_000_000
        }
    ));
    assert_eq!(startup.runtime().expires_at, None);
}

// verifies generated legacy startup material drives the real native forecast path
#[test]
fn serves_active_legacy_wind_snapshot_without_copying_its_candidate() {
    let boundary = FixtureBoundary;
    let snapshot = active_legacy_wind_startup_snapshot();
    let startup = LegacyWindForecastStartup::from_snapshot(&snapshot, &boundary)
        .expect("valid legacy wind startup timestamps")
        .expect("active legacy wind startup");
    let admission = FixtureWindAdmission {
        calls: Cell::new(0),
        fail_public: false,
        identity_mismatch: false,
    };
    let owner = startup
        .model_owner(&snapshot, &admission)
        .expect("the exact startup snapshot must still own the candidate");
    // recover the candidate address solely to prove the mapper retained snapshot ownership
    let expected_candidate = match &snapshot.runtime {
        LoadedApiForecastAdjustmentRuntime::Wind(
            LoadedForecastAdjustmentWindCanaryRuntime::Legacy {
                value: LoadedForecastAdjustmentWindCanaryRuntimeV1::Active(active),
                ..
            },
        ) => match &active.bundle {
            ForecastAdjustmentWindCanaryRuntimeBundle::TimeLimited(bundle) => &bundle.candidate,
            ForecastAdjustmentWindCanaryRuntimeBundle::Permanent(_) => {
                panic!("fixture must remain finite")
            }
        },
        _ => panic!("fixture must remain an active legacy wind snapshot"),
    };
    assert!(std::ptr::eq(owner.candidate, expected_candidate));
    assert_eq!(startup.loaded_at().utc, snapshot.loadedAt);
    assert_eq!(
        startup.runtime().evaluated_at_epoch_ms,
        startup.loaded_at().epoch_milliseconds
    );

    let nominal_data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let inactive_runtime = inactive_forecast_runtime();
    let mut repository = FixtureRepository::default();
    let mut clock = FixtureClock { reads: 0 };
    let response = serve_decoded_request(
        &mut repository,
        &mut clock,
        &WeatherServiceBoundaries {
            nominal_data_at: Some(&nominal_data_at),
            time_resolver: &boundary,
            history: &boundary,
            tides: &boundary,
            forecast: &boundary,
            forecast_runtime: &inactive_runtime,
            forecast_wind_runtime: Some(AdmittedWindForecastServiceRuntime::Legacy {
                loaded_at: startup.loaded_at(),
                runtime: startup.runtime(),
                adjustment: &owner,
            }),
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: &boundary,
            daily_precipitation: &boundary,
            rain_collection: &boundary,
            forecast_observation: &boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/sites/ballydidean/forecast",
            query: &[],
        },
    );

    assert_eq!(response.status, 200);
    assert_eq!(clock.reads, 1);
    let Some(WeatherServiceBody::Forecast(forecast)) = response.body else {
        panic!("active legacy wind forecast response body is missing");
    };
    assert_eq!(forecast.adjustmentRuntime.loadedAt, snapshot.loadedAt);
    assert_eq!(
        forecast.adjustmentRuntime.activeBundle,
        Nullable::Value(
            "5e8b2e3932111621af6785a1b16dfd22edc0a2d26059c6e396654c70119abbe1".to_owned()
        )
    );
    assert_eq!(
        forecast.adjustmentRuntime.authorizationSha256,
        Nullable::Value(
            "fc4895cb45ffb59a725ead664ae20924399585a9d2fa5f5d068f40589b523537".to_owned()
        )
    );
    assert_eq!(
        forecast.adjustmentRuntime.candidateArtifactSha256,
        Nullable::Value(owner.candidate.candidateArtifactSha256.clone())
    );
    assert_eq!(
        forecast.adjustmentRuntime.transferReportSha256,
        Nullable::Value(
            "2c68ba8be5d5343a8d358cee7e21bb157f60c54e6c0bf2bc4b0978f50ec50582".to_owned()
        )
    );
    assert_eq!(
        forecast.adjustmentRuntime.expiresAt,
        Nullable::Value("2026-08-24T05:00:00.000Z".to_owned())
    );
    assert_eq!(
        forecast.adjustmentRuntime.enabledMetrics,
        [
            ForecastAdjustmentMetric::WindGustMps,
            ForecastAdjustmentMetric::WindSpeedMps,
        ]
    );
    assert_eq!(
        forecast.adjustmentRuntime.state,
        ApiForecastAdjustmentRuntimeStateEnum::Active
    );
    assert_eq!(forecast.data[0].metrics.windGustMps, Nullable::Value(3.1));
    assert_eq!(forecast.data[0].metrics.windSpeedMps, Nullable::Value(1.8));
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::WindCanaryActive(
        decision,
    )) = &forecast.data[0].adjustment
    else {
        panic!("active legacy snapshot must dispatch the native wind kernel");
    };
    assert_eq!(
        decision.adjustedMetrics.0,
        [
            (ForecastAdjustmentWindCanaryMetric::WindGustMps, 0.0),
            (ForecastAdjustmentWindCanaryMetric::WindSpeedMps, 0.0),
        ]
    );
    assert_eq!(
        decision.candidateArtifactSha256,
        owner.candidate.candidateArtifactSha256
    );
}

// verifies generated legacy temperature material stays borrowed through native forecast dispatch
#[test]
fn serves_active_legacy_temperature_snapshot_without_copying_its_model() {
    let boundary = FixtureBoundary;
    let snapshot = active_legacy_temperature_startup_snapshot();
    let startup = LegacyTemperatureForecastStartup::from_snapshot(&snapshot, &boundary)
        .expect("valid legacy temperature startup timestamps")
        .expect("active legacy temperature startup");
    let admission = FixtureTemperatureMosAdmission;
    let owner = startup
        .model_owner(&snapshot, &boundary, &admission)
        .expect("the exact startup snapshot must still own the model");
    // recover the model address solely to prove startup retained snapshot ownership
    let expected_model = match &snapshot.runtime {
        LoadedForecastAdjustmentTemperatureCanaryRuntime::Legacy {
            value: LoadedForecastAdjustmentTemperatureCanaryRuntimeV1::Active(active),
            ..
        } => match &active.bundle {
            weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundle::TimeLimited(
                bundle,
            ) => &bundle.model,
            weather_contracts::ForecastAdjustmentTemperatureCanaryRuntimeBundle::Permanent(_) => {
                panic!("fixture must remain finite")
            }
        },
        _ => panic!("fixture must remain an active legacy temperature snapshot"),
    };
    let BorrowedTemperatureMosServingModel::Monthly(selected_model) = owner.model else {
        panic!("finite fixture must retain its monthly model")
    };
    assert!(std::ptr::eq(selected_model, expected_model));
    assert_eq!(startup.loaded_at().utc, snapshot.loadedAt);
    assert_eq!(
        startup.runtime().authorization,
        AdmittedTemperatureCanaryAuthorization::TimeLimited {
            activated_at_epoch_ms: 1_787_202_000_000,
            authorization_sha256: "b".repeat(64),
            expires_at: instant(1_787_547_600_000, "2026-08-24T05:00:00.000Z"),
        }
    );
    assert_eq!(startup.runtime().maximum_receipt_age_hours, 12);
    assert_eq!(startup.runtime().source_delay_hours, 6);

    let mut repository = FixtureRepository {
        temperature_fixture: true,
        ..FixtureRepository::default()
    };
    let mut clock = FixtureClock { reads: 0 };
    let response = active_temperature_request(
        &mut repository,
        &mut clock,
        &boundary,
        &owner,
        startup.runtime(),
    );

    assert_eq!(response.status, 200);
    assert_eq!(clock.reads, 1);
    let Some(WeatherServiceBody::Forecast(forecast)) = response.body else {
        panic!("active legacy temperature forecast response body is missing");
    };
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.loadedAt,
        snapshot.loadedAt
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.activeBundle,
        Nullable::Value("a".repeat(64))
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.authorizationSha256,
        Nullable::Value("b".repeat(64))
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.expiresAt,
        Nullable::Value("2026-08-24T05:00:00.000Z".to_owned())
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.state,
        ApiForecastTemperatureAdjustmentRuntimeStateEnum::Active
    );
    assert_eq!(forecast.data[0].metrics.temperatureC, Nullable::Value(12.1));
    assert_eq!(
        forecast.data[0]
            .temperatureAdjustment
            .rawBestMatchTemperatureC,
        Nullable::Value(12.1)
    );
    assert_eq!(
        forecast.data[0].temperatureAdjustment.correctedTemperatureC,
        Nullable::Value(12.0)
    );
}

// verifies qualified maintenance metadata and its permanent model drive native dispatch
#[test]
fn serves_maintenance_qualified_temperature_snapshot_without_legacy_authorization() {
    let boundary = FixtureBoundary;
    let snapshot = maintenance_qualified_temperature_startup_snapshot();
    let startup = MaintenanceTemperatureForecastStartup::from_snapshot(&snapshot, &boundary)
        .expect("valid maintenance temperature startup timestamp")
        .expect("active maintenance-qualified temperature startup");
    let admission = FixtureTemperatureMosAdmission;
    let owner = startup
        .model_owner(&snapshot, &boundary, &admission)
        .expect("the exact maintenance snapshot must still own the permanent model");
    // recover the model address solely to prove startup retained snapshot ownership
    let expected_model = match &snapshot.runtime {
        LoadedForecastAdjustmentTemperatureCanaryRuntime::MaintenanceActive(active) => {
            &active.bundle.model
        }
        _ => panic!("fixture must remain maintenance qualified"),
    };
    let BorrowedTemperatureMosServingModel::Permanent(selected_model) = owner.model else {
        panic!("maintenance fixture must retain its permanent model")
    };
    assert!(std::ptr::eq(selected_model, expected_model));
    assert_eq!(startup.loaded_at().utc, snapshot.loadedAt);
    assert_eq!(
        startup.runtime().authorization,
        AdmittedTemperatureCanaryAuthorization::MaintenanceQualified {
            action_sha256: "7".repeat(64),
            full_member_root_sha256: "8".repeat(64),
            policy_report_sha256: "9".repeat(64),
        }
    );
    assert_eq!(startup.runtime().maximum_receipt_age_hours, 12);
    assert_eq!(startup.runtime().source_delay_hours, 6);

    let mut repository = FixtureRepository {
        temperature_fixture: true,
        ..FixtureRepository::default()
    };
    let mut clock = FixtureClock { reads: 0 };
    let response = active_temperature_request(
        &mut repository,
        &mut clock,
        &boundary,
        &owner,
        startup.runtime(),
    );

    assert_eq!(response.status, 200);
    assert_eq!(clock.reads, 1);
    let Some(WeatherServiceBody::Forecast(forecast)) = response.body else {
        panic!("maintenance-qualified temperature forecast response body is missing");
    };
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.activationMode,
        Nullable::Value(
            ApiForecastTemperatureAdjustmentRuntimeActivationModeEnum::MaintenanceQualified
        )
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.activeBundle,
        Nullable::Value("6".repeat(64))
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.actionSha256,
        Nullable::Value("7".repeat(64))
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.fullMemberRootSha256,
        Nullable::Value("8".repeat(64))
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.policyReportSha256,
        Nullable::Value("9".repeat(64))
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.loadedAt,
        snapshot.loadedAt
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.authorizationSha256,
        Nullable::Null
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.expiresAt,
        Nullable::Null
    );
    assert_eq!(
        forecast.temperatureAdjustmentRuntime.state,
        ApiForecastTemperatureAdjustmentRuntimeStateEnum::Active
    );
    assert_eq!(forecast.data[0].metrics.temperatureC, Nullable::Value(12.1));
    assert_eq!(
        forecast.data[0]
            .temperatureAdjustment
            .rawBestMatchTemperatureC,
        Nullable::Value(12.1)
    );
    assert_eq!(
        forecast.data[0].temperatureAdjustment.correctedTemperatureC,
        Nullable::Value(12.0)
    );
}

// verifies both installed wind grammars derive the same normalized repository identity
#[test]
fn admits_repository_wind_row_for_legacy_and_maintenance_candidates() {
    let calendar = Arc::new(RecordingWindCalendar::new());
    let admission =
        RepositoryWindModelAdmission::new(Arc::new(RepositoryWindAdmissionTime), calendar.clone());
    let row = repository_wind_row();

    let legacy =
        ForecastWindModelAdmissionBoundary::admit(&admission, &active_wind_candidate(), &row)
            .expect("legacy repository row admission");
    assert_repository_wind_row_admitted(legacy);

    let maintenance = ForecastMaintenanceWindModelAdmissionBoundary::admit(
        &admission,
        &maintenance_wind_candidate(),
        &row,
    )
    .expect("maintenance repository row admission");
    assert_repository_wind_row_admitted(maintenance);

    assert_eq!(calendar.calendar_calls.load(Ordering::Relaxed), 2);
    assert_eq!(calendar.fingerprint_calls.load(Ordering::Relaxed), 2);
    assert_eq!(
        *calendar.valid_at.lock().expect("recorded valid instants"),
        [
            "2026-09-09T06:00:00.000Z".to_owned(),
            "2026-09-09T06:00:00.000Z".to_owned(),
        ]
    );
}

// verifies the last supported lead remains eligible before the 169-hour refusal
#[test]
fn admits_exact_168_hour_repository_wind_lead() {
    let calendar = Arc::new(RecordingWindCalendar::new());
    let admission =
        RepositoryWindModelAdmission::new(Arc::new(RepositoryWindAdmissionTime), calendar.clone());
    let mut row = repository_wind_row();
    row.validAt = JsDateOrString::String(text("2026-09-15T17:45:00.000Z"));

    let result = ForecastMaintenanceWindModelAdmissionBoundary::admit(
        &admission,
        &maintenance_wind_candidate(),
        &row,
    )
    .expect("exact upper lead admission");
    let WindForecastRowAdmission::Admitted(admitted) = result else {
        panic!("exact 168-hour lead should remain admitted");
    };
    assert_eq!(admitted.raw_forecast_provenance.targetLeadHours, 168);
    assert_eq!(calendar.calendar_calls.load(Ordering::Relaxed), 1);
    assert_eq!(calendar.fingerprint_calls.load(Ordering::Relaxed), 1);
}

// verifies unsupported lead geometry is fail-raw before calendar infrastructure
#[test]
fn rejects_unsupported_repository_wind_leads_before_calendar_work() {
    let calendar = Arc::new(RecordingWindCalendar::new());
    let admission =
        RepositoryWindModelAdmission::new(Arc::new(RepositoryWindAdmissionTime), calendar.clone());
    let candidate = maintenance_wind_candidate();
    let mut missing_reference = repository_wind_row();
    missing_reference.productRunAt = Nullable::Null;
    let mut too_long = repository_wind_row();
    too_long.validAt = JsDateOrString::String(text("2026-09-15T18:45:00.000Z"));
    let mut negative = repository_wind_row();
    negative.validAt = JsDateOrString::String(text("2026-09-08T17:44:59.999Z"));

    // preserve the same unsupported-lead response for each invalid geometry
    for row in [missing_reference, too_long, negative] {
        let result =
            ForecastMaintenanceWindModelAdmissionBoundary::admit(&admission, &candidate, &row)
                .expect("unsupported row admission remains a typed result");
        assert_eq!(
            result,
            WindForecastRowAdmission::Rejected(ForecastAdjustmentReasonCode::UnsupportedLead)
        );
    }
    assert_eq!(calendar.calendar_calls.load(Ordering::Relaxed), 0);
    assert_eq!(calendar.fingerprint_calls.load(Ordering::Relaxed), 0);
}

// verifies every source identity mismatch stays fail-raw before calendar work
#[test]
fn rejects_repository_wind_identity_and_dataset_mismatches_before_calendar_work() {
    let calendar = Arc::new(RecordingWindCalendar::new());
    let admission =
        RepositoryWindModelAdmission::new(Arc::new(RepositoryWindAdmissionTime), calendar.clone());
    let candidate = maintenance_wind_candidate();
    let mut wrong_adapter = repository_wind_row();
    wrong_adapter.adapterVersion = Nullable::Value("other-adapter".to_owned());
    let mut missing_dataset = repository_wind_row();
    missing_dataset.providerMetadata = Nullable::Null;
    let mut non_string_dataset = repository_wind_row();
    non_string_dataset.providerMetadata =
        Nullable::Value(vec![(text("dataset"), JsonValue::Number(1.0))]);

    // reject mismatched and non-string identity values without calendar side effects
    for row in [wrong_adapter, missing_dataset, non_string_dataset] {
        let result =
            ForecastMaintenanceWindModelAdmissionBoundary::admit(&admission, &candidate, &row)
                .expect("identity mismatch remains a typed result");
        assert_eq!(
            result,
            WindForecastRowAdmission::Rejected(ForecastAdjustmentReasonCode::IdentityMismatch)
        );
    }
    assert_eq!(calendar.calendar_calls.load(Ordering::Relaxed), 0);
    assert_eq!(calendar.fingerprint_calls.load(Ordering::Relaxed), 0);
}

// verifies qualified maintenance uses one admitted owner in forecast and health
#[test]
fn serves_maintenance_qualified_wind_and_prefers_explicit_startup_refusal() {
    let boundary = FixtureBoundary;
    let snapshot = maintenance_qualified_wind_startup_snapshot();
    let startup = MaintenanceWindForecastStartup::from_snapshot(&snapshot, &boundary)
        .expect("valid maintenance wind startup timestamp")
        .expect("active maintenance-qualified wind startup");
    let calendar = Arc::new(RecordingWindCalendar::new());
    let admission = RepositoryWindModelAdmission::new(Arc::new(FixtureBoundary), calendar.clone());
    let owner = startup
        .model_owner(&snapshot, &admission)
        .expect("the exact maintenance snapshot must still own its candidate");
    let nominal_data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let inactive_runtime = inactive_forecast_runtime();
    let mut forecast_repository = FixtureRepository {
        maintenance_wind_fixture: true,
        ..FixtureRepository::default()
    };
    let mut forecast_clock = FixtureClock { reads: 0 };
    let forecast = serve_decoded_request(
        &mut forecast_repository,
        &mut forecast_clock,
        &WeatherServiceBoundaries {
            nominal_data_at: Some(&nominal_data_at),
            time_resolver: &boundary,
            history: &boundary,
            tides: &boundary,
            forecast: &boundary,
            forecast_runtime: &inactive_runtime,
            forecast_wind_runtime: Some(AdmittedWindForecastServiceRuntime::Maintenance {
                loaded_at: startup.loaded_at(),
                adjustment: &owner,
            }),
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: &boundary,
            daily_precipitation: &boundary,
            rain_collection: &boundary,
            forecast_observation: &boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/sites/ballydidean/forecast",
            query: &[],
        },
    );

    assert_eq!(forecast.status, 200);
    assert_eq!(forecast_clock.reads, 1);
    let Some(WeatherServiceBody::Forecast(forecast)) = forecast.body else {
        panic!("maintenance-qualified wind forecast response body is missing");
    };
    assert_eq!(
        forecast.adjustmentRuntime.activationMode,
        Nullable::Value(ApiForecastAdjustmentRuntimeActivationModeEnum::MaintenanceQualified)
    );
    assert_eq!(
        forecast.adjustmentRuntime.activeBundle,
        Nullable::Value("a".repeat(64))
    );
    assert_eq!(
        forecast.adjustmentRuntime.enabledMetrics,
        [
            ForecastAdjustmentMetric::WindGustMps,
            ForecastAdjustmentMetric::WindSpeedMps,
        ]
    );
    assert_eq!(
        forecast.adjustmentRuntime.actionSha256,
        weather_contracts::FieldPresence::Value("7".repeat(64))
    );
    assert_eq!(
        forecast.adjustmentRuntime.fullMemberRootSha256,
        weather_contracts::FieldPresence::Value("8".repeat(64))
    );
    assert_eq!(
        forecast.adjustmentRuntime.policyReportSha256,
        weather_contracts::FieldPresence::Value("9".repeat(64))
    );
    assert_eq!(
        forecast.adjustmentRuntime.authorizationSha256,
        Nullable::Null
    );
    assert_eq!(forecast.adjustmentRuntime.expiresAt, Nullable::Null);
    assert_eq!(forecast.data[0].metrics.windGustMps, Nullable::Value(8.0));
    assert_eq!(forecast.data[0].metrics.windSpeedMps, Nullable::Value(5.0));
    let ApiForecastAdjustmentDecision::MaintenanceQualified(decision) =
        &forecast.data[0].adjustment
    else {
        panic!("public row must use the qualified maintenance decision");
    };
    assert_eq!(
        decision.adjustedMetrics.0,
        [
            (ForecastAdjustmentMetric::WindGustMps, 8.25),
            (ForecastAdjustmentMetric::WindSpeedMps, 5.25),
        ]
    );
    assert_eq!(
        decision.appliedMetrics,
        [
            ForecastAdjustmentWindCanaryMetric::WindGustMps,
            ForecastAdjustmentWindCanaryMetric::WindSpeedMps,
        ]
    );
    assert_eq!(calendar.calendar_calls.load(Ordering::Relaxed), 1);
    assert_eq!(calendar.fingerprint_calls.load(Ordering::Relaxed), 1);

    let mut malformed_repository = FixtureRepository {
        maintenance_wind_fixture: true,
        malformed_forecast_source: true,
        ..FixtureRepository::default()
    };
    let mut malformed_clock = FixtureClock { reads: 0 };
    let malformed = serve_decoded_request(
        &mut malformed_repository,
        &mut malformed_clock,
        &WeatherServiceBoundaries {
            nominal_data_at: Some(&nominal_data_at),
            time_resolver: &boundary,
            history: &boundary,
            tides: &boundary,
            forecast: &boundary,
            forecast_runtime: &inactive_runtime,
            forecast_wind_runtime: Some(AdmittedWindForecastServiceRuntime::Maintenance {
                loaded_at: startup.loaded_at(),
                adjustment: &owner,
            }),
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: &boundary,
            daily_precipitation: &boundary,
            rain_collection: &boundary,
            forecast_observation: &boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/sites/ballydidean/forecast",
            query: &[],
        },
    );
    assert_eq!(malformed.status, 500);
    assert_eq!(calendar.calendar_calls.load(Ordering::Relaxed), 1);

    let mut health_repository = FixtureRepository::default();
    let mut health_clock = FixtureClock { reads: 0 };
    let health = serve_decoded_request(
        &mut health_repository,
        &mut health_clock,
        &WeatherServiceBoundaries {
            nominal_data_at: None,
            time_resolver: &boundary,
            history: &boundary,
            tides: &boundary,
            forecast: &boundary,
            forecast_runtime: &inactive_runtime,
            forecast_wind_runtime: Some(AdmittedWindForecastServiceRuntime::Maintenance {
                loaded_at: startup.loaded_at(),
                adjustment: &owner,
            }),
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: &boundary,
            daily_precipitation: &boundary,
            rain_collection: &boundary,
            forecast_observation: &boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/health",
            query: &[],
        },
    );
    let Some(WeatherServiceBody::Health(health)) = health.body else {
        panic!("maintenance-qualified wind health response body is missing");
    };
    assert_eq!(health_clock.reads, 1);
    assert_eq!(health_repository.site_reads, 0);
    assert_eq!(health_repository.health_reads, 1);
    assert_eq!(
        health.data.adjustmentRuntime.activationMode,
        Nullable::Value(ApiForecastAdjustmentRuntimeActivationModeEnum::MaintenanceQualified)
    );
    assert_eq!(
        health.data.adjustmentRuntime.loadedAt,
        "2026-08-20T05:00:00.000Z"
    );

    let refused_runtime =
        InactiveForecastServiceRuntime::from_startup(&refused_forecast_startup_snapshots());
    let mut refused_repository = FixtureRepository {
        maintenance_wind_fixture: true,
        ..FixtureRepository::default()
    };
    let mut refused_clock = FixtureClock { reads: 0 };
    let refused = serve_decoded_request(
        &mut refused_repository,
        &mut refused_clock,
        &WeatherServiceBoundaries {
            nominal_data_at: Some(&nominal_data_at),
            time_resolver: &boundary,
            history: &boundary,
            tides: &boundary,
            forecast: &boundary,
            forecast_runtime: &refused_runtime,
            forecast_wind_runtime: Some(AdmittedWindForecastServiceRuntime::Maintenance {
                loaded_at: startup.loaded_at(),
                adjustment: &owner,
            }),
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: &boundary,
            daily_precipitation: &boundary,
            rain_collection: &boundary,
            forecast_observation: &boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/sites/ballydidean/forecast",
            query: &[],
        },
    );
    let Some(WeatherServiceBody::Forecast(refused)) = refused.body else {
        panic!("startup-refused maintenance wind forecast body is missing");
    };
    assert_eq!(
        refused.adjustmentRuntime.state,
        ApiForecastAdjustmentRuntimeStateEnum::Disabled
    );
    assert_eq!(
        refused.adjustmentRuntime.reasonCode,
        Nullable::Value(ApiForecastAdjustmentRuntimeReasonCodeEnum::RegistryInactive)
    );
    assert_eq!(refused.data[0].metrics.windGustMps, Nullable::Value(8.0));
    assert_eq!(refused.data[0].metrics.windSpeedMps, Nullable::Value(5.0));
    assert!(matches!(
        refused.data[0].adjustment,
        ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(_))
    ));
    assert_eq!(calendar.calendar_calls.load(Ordering::Relaxed), 1);
}

// verifies the loader-selected standard fallback preserves all five qualified metrics
#[test]
fn serves_standard_qualified_adjustment_in_forecast_and_health() {
    let boundary = FixtureBoundary;
    let snapshot = standard_qualified_adjustment_startup_snapshot();
    let startup = StandardForecastAdjustmentStartup::from_snapshot(&snapshot, &boundary)
        .expect("valid standard adjustment startup timestamp")
        .expect("active standard-qualified startup");
    let calendar = Arc::new(RecordingWindCalendar::new());
    let admission = RepositoryWindModelAdmission::new(Arc::new(FixtureBoundary), calendar.clone());
    let owner = startup
        .model_owner(&snapshot, &admission)
        .expect("the exact standard snapshot must still own its candidate");
    let nominal_data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
    let inactive_runtime = inactive_forecast_runtime();
    let mut forecast_repository = FixtureRepository {
        maintenance_wind_fixture: true,
        ..FixtureRepository::default()
    };
    let mut forecast_clock = FixtureClock { reads: 0 };
    let forecast = serve_decoded_request(
        &mut forecast_repository,
        &mut forecast_clock,
        &WeatherServiceBoundaries {
            nominal_data_at: Some(&nominal_data_at),
            time_resolver: &boundary,
            history: &boundary,
            tides: &boundary,
            forecast: &boundary,
            forecast_runtime: &inactive_runtime,
            forecast_wind_runtime: Some(AdmittedWindForecastServiceRuntime::Standard {
                loaded_at: startup.loaded_at(),
                adjustment: &owner,
            }),
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: &boundary,
            daily_precipitation: &boundary,
            rain_collection: &boundary,
            forecast_observation: &boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/sites/ballydidean/forecast",
            query: &[],
        },
    );

    assert_eq!(forecast.status, 200);
    assert_eq!(forecast_clock.reads, 1);
    let Some(WeatherServiceBody::Forecast(forecast)) = forecast.body else {
        panic!("standard-qualified forecast response body is missing");
    };
    assert_eq!(
        forecast.adjustmentRuntime.activationMode,
        Nullable::Value(ApiForecastAdjustmentRuntimeActivationModeEnum::Qualified)
    );
    assert_eq!(
        forecast.adjustmentRuntime.activeBundle,
        Nullable::Value("a".repeat(64))
    );
    assert_eq!(
        forecast.adjustmentRuntime.candidateArtifactSha256,
        Nullable::Value("b".repeat(64))
    );
    assert_eq!(
        forecast.adjustmentRuntime.enabledMetrics,
        [
            ForecastAdjustmentMetric::RelativeHumidityPercent,
            ForecastAdjustmentMetric::TemperatureC,
            ForecastAdjustmentMetric::WindDirectionDegrees,
            ForecastAdjustmentMetric::WindGustMps,
            ForecastAdjustmentMetric::WindSpeedMps,
        ]
    );
    assert_eq!(
        forecast.adjustmentRuntime.evaluationReportSha256,
        Nullable::Value("d".repeat(64))
    );
    assert_eq!(
        forecast.adjustmentRuntime.qualificationReceiptSha256,
        Nullable::Value("e".repeat(64))
    );
    assert_eq!(
        forecast.adjustmentRuntime.authorizationSha256,
        Nullable::Null
    );
    assert_eq!(forecast.adjustmentRuntime.expiresAt, Nullable::Null);
    assert_eq!(
        forecast.data[0].metrics.relativeHumidityPercent,
        Nullable::Value(83.0)
    );
    assert_eq!(forecast.data[0].metrics.temperatureC, Nullable::Value(12.1));
    assert_eq!(
        forecast.data[0].metrics.windDirectionDegrees,
        Nullable::Value(180.0)
    );
    assert_eq!(forecast.data[0].metrics.windGustMps, Nullable::Value(8.0));
    assert_eq!(forecast.data[0].metrics.windSpeedMps, Nullable::Value(5.0));
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::Active(decision)) =
        &forecast.data[0].adjustment
    else {
        panic!("public row must use the standard qualified decision");
    };
    assert_eq!(
        decision.adjustedMetrics.0,
        [
            (ForecastAdjustmentMetric::RelativeHumidityPercent, 83.25),
            (ForecastAdjustmentMetric::TemperatureC, 12.35),
            (ForecastAdjustmentMetric::WindDirectionDegrees, 180.25),
            (ForecastAdjustmentMetric::WindGustMps, 8.25),
            (ForecastAdjustmentMetric::WindSpeedMps, 5.25),
        ]
    );
    assert_eq!(
        decision.appliedMetrics,
        forecast.adjustmentRuntime.enabledMetrics
    );
    assert_eq!(decision.candidateArtifactSha256, "b".repeat(64));
    assert_eq!(decision.evaluationReportSha256, "d".repeat(64));
    assert_eq!(decision.qualificationReceiptSha256, "e".repeat(64));
    assert_eq!(calendar.calendar_calls.load(Ordering::Relaxed), 1);
    assert_eq!(calendar.fingerprint_calls.load(Ordering::Relaxed), 1);

    let mut health_repository = FixtureRepository::default();
    let mut health_clock = FixtureClock { reads: 0 };
    let health = serve_decoded_request(
        &mut health_repository,
        &mut health_clock,
        &WeatherServiceBoundaries {
            nominal_data_at: None,
            time_resolver: &boundary,
            history: &boundary,
            tides: &boundary,
            forecast: &boundary,
            forecast_runtime: &inactive_runtime,
            forecast_wind_runtime: Some(AdmittedWindForecastServiceRuntime::Standard {
                loaded_at: startup.loaded_at(),
                adjustment: &owner,
            }),
            forecast_temperature_runtime: None,
            forecast_rain_runtime: None,
            trends: &boundary,
            daily_precipitation: &boundary,
            rain_collection: &boundary,
            forecast_observation: &boundary,
            version: None,
        },
        DecodedServiceRequest {
            method: "GET",
            pathname: "/api/v1/health",
            query: &[],
        },
    );
    let Some(WeatherServiceBody::Health(health)) = health.body else {
        panic!("standard-qualified health response body is missing");
    };
    assert_eq!(health_clock.reads, 1);
    assert_eq!(health_repository.site_reads, 0);
    assert_eq!(health_repository.health_reads, 1);
    assert_eq!(
        health.data.adjustmentRuntime.activationMode,
        Nullable::Value(ApiForecastAdjustmentRuntimeActivationModeEnum::Qualified)
    );
    assert_eq!(
        health.data.adjustmentRuntime.loadedAt,
        "2026-08-20T05:00:00.000Z"
    );
    assert_eq!(calendar.calendar_calls.load(Ordering::Relaxed), 1);
}

// verifies the service consumes the admitted wind snapshot without changing data-time semantics
#[test]
fn serves_active_and_fail_raw_wind_with_safety_clock_and_optional_pressure() {
    let boundary = FixtureBoundary;
    let runtime = active_wind_runtime();
    let candidate = active_wind_candidate();
    let active_admission = FixtureWindAdmission {
        calls: Cell::new(0),
        fail_public: false,
        identity_mismatch: false,
    };
    let active_adjustment = AdmittedWindModelOwner {
        candidate: &candidate,
        evidence: &runtime.evidence,
        admission: &active_admission,
    };
    let mut active_repository = FixtureRepository {
        fail_pressure: true,
        ..FixtureRepository::default()
    };
    let mut active_clock = PrivateFixtureClock { reads: 0 };
    let active = active_wind_request(
        &mut active_repository,
        &mut active_clock,
        &boundary,
        &active_adjustment,
        &runtime,
    );

    assert_eq!(active.status, 200);
    assert_eq!(active_clock.reads, 1);
    let Some(WeatherServiceBody::Forecast(active)) = active.body else {
        panic!("active wind forecast response body is missing");
    };
    assert_eq!(active.generatedAt, "2026-08-22T05:00:00.000Z");
    assert!(active.pressureContext.is_empty());
    assert_eq!(
        active.adjustmentRuntime.activationMode,
        Nullable::Value(ApiForecastAdjustmentRuntimeActivationModeEnum::WindCanary)
    );
    assert_eq!(
        active.adjustmentRuntime.state,
        ApiForecastAdjustmentRuntimeStateEnum::Active
    );
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::WindCanaryActive(
        active_decision,
    )) = &active.data[0].adjustment
    else {
        panic!("configured forecast row must use the active wind decision");
    };
    assert_eq!(
        active_decision.appliedMetrics,
        [
            ForecastAdjustmentWindCanaryMetric::WindGustMps,
            ForecastAdjustmentWindCanaryMetric::WindSpeedMps,
        ]
    );
    assert_eq!(
        active_decision.adjustedMetrics.0,
        [
            (ForecastAdjustmentWindCanaryMetric::WindGustMps, 0.0),
            (ForecastAdjustmentWindCanaryMetric::WindSpeedMps, 0.0),
        ]
    );
    assert_eq!(
        active_decision.authorizationSha256,
        runtime.evidence.authorization_sha256
    );
    assert_eq!(
        active_decision.candidateArtifactSha256,
        runtime.evidence.candidate_artifact_sha256
    );
    assert_eq!(
        active_decision.transferReportSha256,
        runtime.evidence.transfer_report_sha256
    );

    let fail_raw_admission = FixtureWindAdmission {
        calls: Cell::new(0),
        fail_public: false,
        identity_mismatch: true,
    };
    let fail_raw_adjustment = AdmittedWindModelOwner {
        candidate: &candidate,
        evidence: &runtime.evidence,
        admission: &fail_raw_admission,
    };
    let mut fail_raw_repository = FixtureRepository {
        pressure_available: true,
        ..FixtureRepository::default()
    };
    let mut fail_raw_clock = PrivateFixtureClock { reads: 0 };
    let fail_raw = active_wind_request(
        &mut fail_raw_repository,
        &mut fail_raw_clock,
        &boundary,
        &fail_raw_adjustment,
        &runtime,
    );
    let Some(WeatherServiceBody::Forecast(fail_raw)) = fail_raw.body else {
        panic!("fail-raw wind forecast response body is missing");
    };
    assert_eq!(fail_raw.pressureContext.len(), 1);
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(decision)) =
        &fail_raw.data[0].adjustment
    else {
        panic!("source mismatch must remain an explicit fail-raw decision");
    };
    assert_eq!(
        decision.reasonCode,
        ForecastAdjustmentReasonCode::IdentityMismatch
    );
    assert_eq!(
        decision.state,
        ForecastAdjustmentFailRawDecisionStateEnum::NotApplicable
    );

    let expired_admission = FixtureWindAdmission {
        calls: Cell::new(0),
        fail_public: false,
        identity_mismatch: false,
    };
    let expired_adjustment = AdmittedWindModelOwner {
        candidate: &candidate,
        evidence: &runtime.evidence,
        admission: &expired_admission,
    };
    let mut expired_repository = FixtureRepository::default();
    let mut expired_clock = StaticFixtureClock {
        now: instant(1_787_547_600_000, "2026-08-24T05:00:00.000Z"),
        reads: 0,
    };
    let expired = active_wind_request(
        &mut expired_repository,
        &mut expired_clock,
        &boundary,
        &expired_adjustment,
        &runtime,
    );
    let Some(WeatherServiceBody::Forecast(expired)) = expired.body else {
        panic!("expired wind forecast response body is missing");
    };
    assert_eq!(
        expired.adjustmentRuntime.reasonCode,
        Nullable::Value(ApiForecastAdjustmentRuntimeReasonCodeEnum::CanaryExpired)
    );
    assert_eq!(
        expired.adjustmentRuntime.state,
        ApiForecastAdjustmentRuntimeStateEnum::Disabled
    );
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(decision)) =
        &expired.data[0].adjustment
    else {
        panic!("expired runtime must override row fail-raw reason");
    };
    assert_eq!(
        decision.reasonCode,
        ForecastAdjustmentReasonCode::CanaryExpired
    );
    assert_eq!(
        decision.state,
        ForecastAdjustmentFailRawDecisionStateEnum::Disabled
    );
    assert_eq!(expired_admission.calls.get(), 0);

    let mut killed_runtime = runtime.clone();
    killed_runtime.environment_kill_switch = Some("1".to_owned());
    let killed_admission = FixtureWindAdmission {
        calls: Cell::new(0),
        fail_public: false,
        identity_mismatch: false,
    };
    let killed_adjustment = AdmittedWindModelOwner {
        candidate: &candidate,
        evidence: &killed_runtime.evidence,
        admission: &killed_admission,
    };
    let mut killed_repository = FixtureRepository::default();
    let mut killed_clock = PrivateFixtureClock { reads: 0 };
    let killed = active_wind_request(
        &mut killed_repository,
        &mut killed_clock,
        &boundary,
        &killed_adjustment,
        &killed_runtime,
    );
    let Some(WeatherServiceBody::Forecast(killed)) = killed.body else {
        panic!("killed wind forecast response body is missing");
    };
    assert_eq!(
        killed.adjustmentRuntime.reasonCode,
        Nullable::Value(ApiForecastAdjustmentRuntimeReasonCodeEnum::CanaryKilled)
    );
    assert_eq!(
        killed.adjustmentRuntime.state,
        ApiForecastAdjustmentRuntimeStateEnum::Disabled
    );
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(decision)) =
        &killed.data[0].adjustment
    else {
        panic!("killed runtime must override row model evaluation");
    };
    assert_eq!(
        decision.reasonCode,
        ForecastAdjustmentReasonCode::CanaryKilled
    );
    assert_eq!(
        decision.state,
        ForecastAdjustmentFailRawDecisionStateEnum::Disabled
    );
    assert_eq!(killed_admission.calls.get(), 0);

    let failing_admission = FixtureWindAdmission {
        calls: Cell::new(0),
        fail_public: true,
        identity_mismatch: false,
    };
    let failing_adjustment = AdmittedWindModelOwner {
        candidate: &candidate,
        evidence: &runtime.evidence,
        admission: &failing_admission,
    };
    let mut failing_repository = FixtureRepository::default();
    let mut failing_clock = PrivateFixtureClock { reads: 0 };
    let failure = active_wind_request(
        &mut failing_repository,
        &mut failing_clock,
        &boundary,
        &failing_adjustment,
        &runtime,
    );
    assert_eq!(failure.status, 200);
    let Some(WeatherServiceBody::Forecast(failure)) = failure.body else {
        panic!("optional wind failure response body is missing");
    };
    assert_eq!(
        failure.adjustmentRuntime.state,
        ApiForecastAdjustmentRuntimeStateEnum::Active
    );
    assert_eq!(failure.data[0].metrics.windGustMps, Nullable::Value(3.1));
    assert_eq!(failure.data[0].metrics.windSpeedMps, Nullable::Value(1.8));
    let ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(decision)) =
        &failure.data[0].adjustment
    else {
        panic!("optional application failure must become row-level fail raw");
    };
    assert_eq!(
        decision.reasonCode,
        ForecastAdjustmentReasonCode::AdjustmentError
    );
    assert_eq!(
        decision.state,
        ForecastAdjustmentFailRawDecisionStateEnum::NotApplicable
    );
    assert!(matches!(
        failure.airQualityContext[0].adjustment,
        ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::WindCanaryActive(_))
    ));
}

// verifies active temperature sidecar, raw fallback, and inference isolation in real dispatch
#[test]
fn serves_active_temperature_with_data_safety_reads_and_mixed_row_fallbacks() {
    let boundary = FixtureBoundary;
    let runtime = active_temperature_runtime();
    let model = temperature_mos_model();
    let admission = FixtureTemperatureMosAdmission;
    let active_adjustment = AdmittedTemperatureMosModelOwner {
        model: &model,
        origin: AdmittedTemperatureMosModelOrigin::admitted(&runtime.evidence.active_bundle_sha256),
        served_adapter_version: "open-meteo-ecmwf-single-run/v1",
        time_resolver: &boundary,
        admission: &admission,
    };
    let mut repository = FixtureRepository {
        temperature_fixture: true,
        ..FixtureRepository::default()
    };
    let mut clock = FixtureClock { reads: 0 };
    let response = active_temperature_request(
        &mut repository,
        &mut clock,
        &boundary,
        &active_adjustment,
        &runtime,
    );

    assert_eq!(response.status, 200);
    assert_eq!(clock.reads, 1);
    assert_eq!(
        repository.temperature_sidecar_reads,
        [ForecastTemperatureSidecarRead {
            site_slug: "ballydidean".to_owned(),
            as_of: "2026-08-22T05:00:00.000Z".to_owned(),
            from: "2026-08-21T07:00:00.000Z".to_owned(),
            to: "2026-08-22T07:00:00.000Z".to_owned(),
        }]
    );
    assert_eq!(
        repository.temperature_status_reads,
        [ForecastTemperatureStatusRead {
            site_slug: "ballydidean".to_owned(),
            as_of: "2026-08-22T05:00:00.000Z".to_owned(),
        }]
    );
    let Some(WeatherServiceBody::Forecast(response)) = response.body else {
        panic!("active temperature forecast response body is missing");
    };
    assert_eq!(
        response.temperatureAdjustmentRuntime.state,
        ApiForecastTemperatureAdjustmentRuntimeStateEnum::Active
    );
    let Nullable::Value(source) = &response.temperatureAdjustmentRuntime.source else {
        panic!("temperature collector status must be projected");
    };
    assert!(!source.adaptiveReady);
    assert_eq!(source.stateReason, Nullable::Value("cold_start".to_owned()));
    assert_eq!(response.data[0].metrics.temperatureC, Nullable::Value(12.1));
    assert_eq!(
        response.data[0].temperatureAdjustment.state,
        ForecastTemperatureCanaryDecisionV1StateEnum::Active
    );
    assert_eq!(
        response.data[0].temperatureAdjustment.correctedTemperatureC,
        Nullable::Value(12.0)
    );
    assert_eq!(
        response.data[0].temperatureAdjustment.branch,
        Nullable::Value(ForecastTemperatureCanaryDecisionV1BranchEnum::Direct)
    );
    assert_eq!(
        response.data[0]
            .temperatureAdjustment
            .recentErrorStateSha256,
        Nullable::Value(temperature_recent_state_sha256().to_owned())
    );
    let Nullable::Value(source_forecast) = &response.data[0].temperatureAdjustment.sourceForecast
    else {
        panic!("active temperature decision source is missing");
    };
    assert_eq!(source_forecast.operationalHorizonHours, 6);
    assert_eq!(
        response.airQualityContext[0]
            .temperatureAdjustment
            .reasonCode,
        Nullable::Value(TemperatureCanaryDecisionReasonValue::MissingSourceForecast)
    );

    let mismatched_bundle = "f".repeat(64);
    let mismatched_adjustment = AdmittedTemperatureMosModelOwner {
        model: &model,
        origin: AdmittedTemperatureMosModelOrigin::admitted(&mismatched_bundle),
        served_adapter_version: "open-meteo-ecmwf-single-run/v1",
        time_resolver: &boundary,
        admission: &admission,
    };
    let mut mismatched_repository = FixtureRepository {
        temperature_fixture: true,
        ..FixtureRepository::default()
    };
    let mut mismatched_clock = FixtureClock { reads: 0 };
    let mismatched = active_temperature_request(
        &mut mismatched_repository,
        &mut mismatched_clock,
        &boundary,
        &mismatched_adjustment,
        &runtime,
    );
    let Some(WeatherServiceBody::Forecast(mismatched)) = mismatched.body else {
        panic!("mismatched temperature model response body is missing");
    };
    assert_eq!(
        mismatched.temperatureAdjustmentRuntime.state,
        ApiForecastTemperatureAdjustmentRuntimeStateEnum::Active
    );
    assert_eq!(
        mismatched.data[0].temperatureAdjustment.state,
        ForecastTemperatureCanaryDecisionV1StateEnum::Disabled
    );
    assert_eq!(
        mismatched.data[0].temperatureAdjustment.reasonCode,
        Nullable::Value(TemperatureCanaryDecisionReasonValue::BundleInvalid)
    );
    assert_eq!(
        mismatched.data[0].temperatureAdjustment.bundleSha256,
        Nullable::Null
    );
    assert_eq!(
        mismatched.data[0]
            .temperatureAdjustment
            .correctedTemperatureC,
        Nullable::Null
    );

    let mut stale_repository = FixtureRepository {
        temperature_fixture: true,
        ..FixtureRepository::default()
    };
    let mut stale_clock = StaticFixtureClock {
        now: instant(1_787_403_960_000, "2026-08-22T13:06:00.000Z"),
        reads: 0,
    };
    let stale = active_temperature_request(
        &mut stale_repository,
        &mut stale_clock,
        &boundary,
        &active_adjustment,
        &runtime,
    );
    let Some(WeatherServiceBody::Forecast(stale)) = stale.body else {
        panic!("stale temperature forecast response body is missing");
    };
    assert_eq!(stale.data[0].metrics.temperatureC, Nullable::Value(12.1));
    assert_eq!(
        stale.data[0].temperatureAdjustment.state,
        ForecastTemperatureCanaryDecisionV1StateEnum::RawFallback
    );
    assert_eq!(
        stale.data[0].temperatureAdjustment.reasonCode,
        Nullable::Value(TemperatureCanaryDecisionReasonValue::SourceStale)
    );
    assert_eq!(
        stale.data[0].temperatureAdjustment.correctedTemperatureC,
        Nullable::Null
    );
    assert_eq!(
        stale.data[0].temperatureAdjustment.bundleSha256,
        Nullable::Value(runtime.evidence.active_bundle_sha256.clone())
    );
    assert_eq!(
        stale.data[0].temperatureAdjustment.recentErrorStateSha256,
        Nullable::Null
    );
    assert_eq!(
        stale.data[0].temperatureAdjustment.sourceForecast,
        Nullable::Null
    );

    let mut failed_reads_repository = FixtureRepository {
        temperature_fixture: true,
        fail_temperature_sidecar: true,
        fail_temperature_status: true,
        ..FixtureRepository::default()
    };
    let mut failed_reads_clock = FixtureClock { reads: 0 };
    let failed_reads = active_temperature_request(
        &mut failed_reads_repository,
        &mut failed_reads_clock,
        &boundary,
        &active_adjustment,
        &runtime,
    );
    let Some(WeatherServiceBody::Forecast(failed_reads)) = failed_reads.body else {
        panic!("temperature read-fallback response body is missing");
    };
    assert_eq!(
        failed_reads.temperatureAdjustmentRuntime.source,
        Nullable::Null
    );
    assert_eq!(
        failed_reads.data[0].temperatureAdjustment.reasonCode,
        Nullable::Value(TemperatureCanaryDecisionReasonValue::MissingSourceForecast)
    );
    assert_eq!(
        failed_reads.data[0].temperatureAdjustment.state,
        ForecastTemperatureCanaryDecisionV1StateEnum::RawFallback
    );

    let failing_adjustment = FixtureTemperatureAdjustment {
        mode: FixtureTemperatureMode::ApplicationFailure,
    };
    let mut inference_repository = FixtureRepository {
        temperature_fixture: true,
        ..FixtureRepository::default()
    };
    let mut inference_clock = FixtureClock { reads: 0 };
    let inference = active_temperature_request(
        &mut inference_repository,
        &mut inference_clock,
        &boundary,
        &failing_adjustment,
        &runtime,
    );
    assert_eq!(inference.status, 200);
    let Some(WeatherServiceBody::Forecast(inference)) = inference.body else {
        panic!("temperature inference-fallback response body is missing");
    };
    assert_eq!(
        inference.data[0].metrics.temperatureC,
        Nullable::Value(12.1)
    );
    assert_eq!(
        inference.data[0].temperatureAdjustment.reasonCode,
        Nullable::Value(TemperatureCanaryDecisionReasonValue::BundleInvalid)
    );
    assert_eq!(
        inference.data[0].temperatureAdjustment.state,
        ForecastTemperatureCanaryDecisionV1StateEnum::Disabled
    );
}

// verifies health reprojects the wind lifecycle at the same request safety instant
#[test]
fn serves_active_and_expired_wind_runtime_in_health() {
    let boundary = FixtureBoundary;
    let runtime = active_wind_runtime();
    let candidate = active_wind_candidate();
    let admission = FixtureWindAdmission {
        calls: Cell::new(0),
        fail_public: false,
        identity_mismatch: false,
    };
    let adjustment = AdmittedWindModelOwner {
        candidate: &candidate,
        evidence: &runtime.evidence,
        admission: &admission,
    };
    let mut active_repository = FixtureRepository::default();
    let mut active_clock = FixtureClock { reads: 0 };
    let active = admitted_wind_health_request(
        &mut active_repository,
        &mut active_clock,
        &boundary,
        &adjustment,
        &runtime,
    );

    assert_eq!(active.status, 200);
    assert_eq!(active_clock.reads, 1);
    assert_eq!(active_repository.site_reads, 0);
    assert_eq!(active_repository.health_reads, 1);
    // inspect the active request-time lifecycle projection
    let Some(WeatherServiceBody::Health(active)) = active.body else {
        panic!("active wind health body is missing");
    };
    assert_eq!(
        active.data.adjustmentRuntime.activationMode,
        Nullable::Value(ApiForecastAdjustmentRuntimeActivationModeEnum::WindCanary)
    );
    assert_eq!(
        active.data.adjustmentRuntime.state,
        ApiForecastAdjustmentRuntimeStateEnum::Active
    );
    assert_eq!(
        active.data.adjustmentRuntime.activeBundle,
        Nullable::Value(runtime.evidence.active_bundle_sha256.clone())
    );

    let mut expired_repository = FixtureRepository::default();
    let mut expired_clock = StaticFixtureClock {
        now: instant(1_787_547_600_000, "2026-08-24T05:00:00.000Z"),
        reads: 0,
    };
    let expired = admitted_wind_health_request(
        &mut expired_repository,
        &mut expired_clock,
        &boundary,
        &adjustment,
        &runtime,
    );
    // inspect the boundary-disabled request-time lifecycle projection
    let Some(WeatherServiceBody::Health(expired)) = expired.body else {
        panic!("expired wind health body is missing");
    };
    assert_eq!(expired_clock.reads, 1);
    assert_eq!(expired_repository.health_reads, 1);
    assert_eq!(
        expired.data.adjustmentRuntime.state,
        ApiForecastAdjustmentRuntimeStateEnum::Disabled
    );
    assert_eq!(
        expired.data.adjustmentRuntime.reasonCode,
        Nullable::Value(ApiForecastAdjustmentRuntimeReasonCodeEnum::CanaryExpired)
    );
    assert_eq!(expired.data.adjustmentRuntime.activeBundle, Nullable::Null);
}

// verifies health reprojects temperature lifecycle at safety time and isolates optional status
#[test]
fn serves_temperature_health_status_only_while_request_runtime_is_active() {
    let boundary = FixtureBoundary;
    let runtime = active_temperature_runtime();
    let adjustment = FixtureTemperatureAdjustment {
        mode: FixtureTemperatureMode::Active,
    };
    let mut active_repository = FixtureRepository::default();
    let mut active_clock = FixtureClock { reads: 0 };
    let active = admitted_temperature_health_request(
        &mut active_repository,
        &mut active_clock,
        &boundary,
        &adjustment,
        &runtime,
    );

    assert_eq!(active.status, 200);
    assert_eq!(active_clock.reads, 1);
    assert_eq!(active_repository.site_reads, 0);
    assert_eq!(active_repository.health_reads, 1);
    assert_eq!(
        active_repository.temperature_status_reads,
        [ForecastTemperatureStatusRead {
            site_slug: "ballydidean".to_owned(),
            as_of: "2026-08-22T05:00:00.000Z".to_owned(),
        }]
    );
    let Some(WeatherServiceBody::Health(active)) = active.body else {
        panic!("active temperature health body is missing");
    };
    assert_eq!(
        active.data.temperatureAdjustmentRuntime.state,
        ApiForecastTemperatureAdjustmentRuntimeStateEnum::Active
    );
    assert!(matches!(
        active.data.temperatureAdjustmentRuntime.source,
        Nullable::Value(_)
    ));

    let mut failed_status_repository = FixtureRepository {
        fail_temperature_status: true,
        ..FixtureRepository::default()
    };
    let mut failed_status_clock = FixtureClock { reads: 0 };
    let failed_status = admitted_temperature_health_request(
        &mut failed_status_repository,
        &mut failed_status_clock,
        &boundary,
        &adjustment,
        &runtime,
    );
    assert_eq!(failed_status.status, 200);
    let Some(WeatherServiceBody::Health(failed_status)) = failed_status.body else {
        panic!("status-failure temperature health body is missing");
    };
    assert_eq!(
        failed_status.data.temperatureAdjustmentRuntime.state,
        ApiForecastTemperatureAdjustmentRuntimeStateEnum::Active
    );
    assert_eq!(
        failed_status.data.temperatureAdjustmentRuntime.source,
        Nullable::Null
    );

    let mut expired_repository = FixtureRepository::default();
    let mut expired_clock = StaticFixtureClock {
        now: instant(1_788_048_000_000, "2026-08-30T00:00:00.000Z"),
        reads: 0,
    };
    let expired = admitted_temperature_health_request(
        &mut expired_repository,
        &mut expired_clock,
        &boundary,
        &adjustment,
        &runtime,
    );
    assert!(expired_repository.temperature_status_reads.is_empty());
    let Some(WeatherServiceBody::Health(expired)) = expired.body else {
        panic!("expired temperature health body is missing");
    };
    assert_eq!(
        expired.data.temperatureAdjustmentRuntime.state,
        ApiForecastTemperatureAdjustmentRuntimeStateEnum::Disabled
    );
    assert_eq!(
        expired.data.temperatureAdjustmentRuntime.reasonCode,
        Nullable::Value(ApiForecastTemperatureAdjustmentRuntimeReasonCodeEnum::CanaryExpired)
    );
    assert_eq!(
        expired.data.temperatureAdjustmentRuntime.activeBundle,
        Nullable::Null
    );
    assert_eq!(
        expired.data.temperatureAdjustmentRuntime.source,
        Nullable::Null
    );
}

// verifies forecast optional effects preserve temperature sidecar and status ordering before rain
#[test]
fn reads_temperature_optional_sources_before_rain_adjustment() {
    let boundary = FixtureBoundary;
    let runtime = active_temperature_runtime();
    let model = temperature_mos_model();
    let admission = FixtureTemperatureMosAdmission;
    let adjustment = AdmittedTemperatureMosModelOwner {
        model: &model,
        origin: AdmittedTemperatureMosModelOrigin::admitted(&runtime.evidence.active_bundle_sha256),
        served_adapter_version: "open-meteo-ecmwf-single-run/v1",
        time_resolver: &boundary,
        admission: &admission,
    };
    let mut repository = FixtureRepository {
        temperature_fixture: true,
        rain_adjustment_available: true,
        ..FixtureRepository::default()
    };
    let mut clock = FixtureClock { reads: 0 };

    let response = active_temperature_request(
        &mut repository,
        &mut clock,
        &boundary,
        &adjustment,
        &runtime,
    );

    assert_eq!(response.status, 200);
    assert_eq!(
        repository.optional_read_order,
        [
            "temperature_sidecar",
            "temperature_status",
            "rain_adjustment"
        ]
    );
}

// verifies retained rain output, independent fallbacks, guards, policy, and HEAD dispatch
#[test]
fn serves_admitted_rain_adjustments_with_node_fallback_semantics() {
    let boundary = FixtureBoundary;
    let active_runtime = AdmittedRainForecastRuntime::Active {
        loaded_at: "2026-08-22T04:59:00.000Z".to_owned(),
    };
    let mut repository = FixtureRepository {
        rain_fixture: true,
        rain_adjustment_available: true,
        ..FixtureRepository::default()
    };
    let mut clock = FixtureClock { reads: 0 };
    let active = admitted_rain_request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        &active_runtime,
    );

    assert_eq!(active.status, 200);
    assert_eq!(clock.reads, 1);
    assert_eq!(
        repository.rain_adjustment_reads,
        [ForecastRainAdjustmentRead {
            site_slug: "ballydidean".to_owned(),
            as_of: "2026-08-22T05:00:00.000Z".to_owned(),
        }]
    );
    let Some(WeatherServiceBody::Forecast(active)) = active.body else {
        panic!("active rain forecast response body is missing");
    };
    assert_eq!(active.data[0].metrics.precipitationMm, Nullable::Value(0.1));
    assert_eq!(
        active.data[0].rainAdjustment.state,
        ApiRainAdjustmentDecisionStateEnum::Active
    );
    assert_eq!(
        active.data[0].rainAdjustment.correctedPrecipitationMm,
        Nullable::Value(0.05)
    );
    let Nullable::Value(source) = &active.data[0].rainAdjustment.sourceForecast else {
        panic!("active rain source provenance is missing");
    };
    assert_eq!(source.modelLeadHours, 12);
    assert_eq!(source.rawPrecipitationMm, 0.1);
    assert_eq!(
        active.airQualityContext[0].rainAdjustment.state,
        ApiRainAdjustmentDecisionStateEnum::RawFallback
    );
    assert_eq!(
        active.airQualityContext[0].rainAdjustment.reasonCode,
        Nullable::Value("outside_operational_window".to_owned())
    );
    assert_eq!(
        active.rainAdjustmentRuntime.state,
        ApiRainAdjustmentRuntimeStateEnum::Active
    );
    assert_eq!(
        active.rainAdjustmentRuntime.loadedAt,
        "2026-08-22T04:59:00.000Z"
    );
    let Nullable::Value(runtime_source) = &active.rainAdjustmentRuntime.source else {
        panic!("active rain runtime source is missing");
    };
    assert_eq!(runtime_source.hourCount, 1);

    // contain missing, failed, and malformed optional reads without affecting forecast availability
    for mut fallback_repository in [
        FixtureRepository {
            rain_fixture: true,
            ..FixtureRepository::default()
        },
        FixtureRepository {
            rain_fixture: true,
            fail_rain_adjustment: true,
            ..FixtureRepository::default()
        },
        FixtureRepository {
            rain_fixture: true,
            rain_adjustment_available: true,
            invalid_rain_adjustment: true,
            ..FixtureRepository::default()
        },
    ] {
        let mut fallback_clock = FixtureClock { reads: 0 };
        let fallback = admitted_rain_request(
            &mut fallback_repository,
            &mut fallback_clock,
            &boundary,
            "GET",
            &active_runtime,
        );
        assert_eq!(fallback.status, 200);
        let Some(WeatherServiceBody::Forecast(fallback)) = fallback.body else {
            panic!("rain read-fallback response body is missing");
        };
        assert_eq!(
            fallback.data[0].rainAdjustment.state,
            ApiRainAdjustmentDecisionStateEnum::Disabled
        );
        assert_eq!(
            fallback.data[0].rainAdjustment.reasonCode,
            Nullable::Value("model_unavailable".to_owned())
        );
        assert_eq!(
            fallback.rainAdjustmentRuntime.reasonCode,
            Nullable::Value(ApiRainAdjustmentRuntimeReasonCodeEnum::ModelUnavailable)
        );
    }

    // retain valid-run lifecycle while guarding raw and source-row identity independently
    for mut guarded_repository in [
        FixtureRepository {
            rain_fixture: true,
            rain_adjustment_available: true,
            rain_raw_missing: true,
            ..FixtureRepository::default()
        },
        FixtureRepository {
            rain_fixture: true,
            rain_adjustment_available: true,
            rain_non_best_match: true,
            ..FixtureRepository::default()
        },
    ] {
        let mut guarded_clock = FixtureClock { reads: 0 };
        let guarded = admitted_rain_request(
            &mut guarded_repository,
            &mut guarded_clock,
            &boundary,
            "GET",
            &active_runtime,
        );
        let Some(WeatherServiceBody::Forecast(guarded)) = guarded.body else {
            panic!("guarded rain response body is missing");
        };
        assert_ne!(
            guarded.data[0].rainAdjustment.state,
            ApiRainAdjustmentDecisionStateEnum::Active
        );
        assert_eq!(
            guarded.rainAdjustmentRuntime.state,
            ApiRainAdjustmentRuntimeStateEnum::Active
        );
    }

    let mut unconfigured_repository = FixtureRepository {
        rain_fixture: true,
        rain_adjustment_available: true,
        ..FixtureRepository::default()
    };
    let mut unconfigured_clock = FixtureClock { reads: 0 };
    let unconfigured = request(
        &mut unconfigured_repository,
        &mut unconfigured_clock,
        &boundary,
        "GET",
        "/api/v1/sites/ballydidean/forecast",
        &[],
    );
    assert_eq!(unconfigured_repository.rain_adjustment_reads.len(), 1);
    let Some(WeatherServiceBody::Forecast(unconfigured)) = unconfigured.body else {
        panic!("unconfigured rain response body is missing");
    };
    assert_eq!(
        unconfigured.data[0].rainAdjustment.state,
        ApiRainAdjustmentDecisionStateEnum::Active
    );
    assert_eq!(
        unconfigured.rainAdjustmentRuntime.state,
        ApiRainAdjustmentRuntimeStateEnum::Active
    );
    assert_eq!(
        unconfigured.rainAdjustmentRuntime.loadedAt,
        "2026-08-22T05:00:00.000Z"
    );

    let disabled_runtime = AdmittedRainForecastRuntime::Disabled {
        loaded_at: "2026-08-22T04:58:00.000Z".to_owned(),
        reason_code: ApiRainAdjustmentRuntimeReasonCodeEnum::PolicyRaw,
    };
    let mut disabled_repository = FixtureRepository {
        rain_fixture: true,
        rain_adjustment_available: true,
        ..FixtureRepository::default()
    };
    let mut disabled_clock = FixtureClock { reads: 0 };
    let disabled = admitted_rain_request(
        &mut disabled_repository,
        &mut disabled_clock,
        &boundary,
        "GET",
        &disabled_runtime,
    );
    assert!(disabled_repository.rain_adjustment_reads.is_empty());
    let Some(WeatherServiceBody::Forecast(disabled)) = disabled.body else {
        panic!("policy-disabled rain response body is missing");
    };
    assert_eq!(
        disabled.data[0].rainAdjustment.reasonCode,
        Nullable::Value("policy_raw".to_owned())
    );
    assert_eq!(
        disabled.rainAdjustmentRuntime.loadedAt,
        "2026-08-22T04:58:00.000Z"
    );

    let mut head_repository = FixtureRepository {
        rain_fixture: true,
        rain_adjustment_available: true,
        ..FixtureRepository::default()
    };
    let mut head_clock = FixtureClock { reads: 0 };
    let head = admitted_rain_request(
        &mut head_repository,
        &mut head_clock,
        &boundary,
        "HEAD",
        &active_runtime,
    );
    assert_eq!(head.status, 200);
    assert!(head.body.is_none());
    assert_eq!(head_repository.rain_adjustment_reads.len(), 1);
}

// verifies daily site lookup and health no-site readiness behavior through exact DTOs
#[test]
fn serves_daily_precipitation_and_health_with_node_ordering() {
    let boundary = FixtureBoundary;
    let mut repository = FixtureRepository::default();
    let mut clock = FixtureClock { reads: 0 };
    let daily = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/sites/ballydidean/daily-precipitation",
        &[],
    );
    let ready = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/health",
        &[],
    );
    repository.fail_health = true;
    let unavailable = request(
        &mut repository,
        &mut clock,
        &boundary,
        "HEAD",
        "/api/v1/health",
        &[],
    );
    let invalid = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/health",
        &[QueryParameter {
            name: "unexpected",
            value: "true",
        }],
    );

    assert_eq!(
        [
            daily.status,
            ready.status,
            unavailable.status,
            invalid.status
        ],
        [200, 200, 503, 400]
    );
    assert_eq!(clock.reads, 4);
    assert_eq!(repository.site_reads, 1);
    assert_eq!(repository.health_reads, 2);
    assert!(repository.temperature_status_reads.is_empty());
    assert_eq!(repository.daily_plans.len(), 1);
    assert_eq!(
        repository.daily_plans[0].binds,
        [
            DailyPrecipitationBind::Text("ballydidean".to_owned()),
            DailyPrecipitationBind::UtcInstant("2026-08-21T07:00:00.000Z".to_owned()),
            DailyPrecipitationBind::UtcInstant("2026-08-22T05:00:00.000Z".to_owned()),
        ]
    );
    assert_eq!(unavailable.body, None);
    assert_eq!(error_code(&invalid), "invalid_query");

    let Some(WeatherServiceBody::DailyPrecipitation(body)) = daily.body else {
        panic!("daily precipitation response body is missing");
    };
    let Nullable::Value(data) = body.data else {
        panic!("daily precipitation row must be present");
    };
    assert_eq!(body.generatedAt, "2026-08-22T05:00:00.000Z");
    assert_eq!(data.accumulationMm, 2.54);
    assert_eq!(data.source.sourceId, "13");
    assert_eq!(data.validThrough, "2026-08-22T04:59:00.000Z");

    let Some(WeatherServiceBody::Health(body)) = ready.body else {
        panic!("ready health response body is missing");
    };
    assert_eq!(body.data.database, ApiHealthResponseDataDatabaseEnum::Ready);
    assert_eq!(
        body.data.migration.status,
        ApiHealthResponseDataMigrationStatusEnum::Current
    );
    assert_eq!(
        body.data.migration.version,
        Nullable::Value("2026082201".to_owned())
    );
    assert_eq!(
        body.data.worker.freshness,
        ApiHealthResponseDataWorkerFreshnessEnum::Fresh
    );
    assert!(body.data.ready);
    assert_eq!(body.data.version, "development");
}

// verifies the fixed-site rain collection route and typed optional store boundary
#[test]
fn serves_sanitized_rain_collection_status() {
    let boundary = FixtureBoundary;
    let mut repository = FixtureRepository {
        rain_collection_available: true,
        ..FixtureRepository::default()
    };
    let mut clock = FixtureClock { reads: 0 };
    let response = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/sites/ballydidean/rain-collection",
        &[],
    );
    let head = request(
        &mut repository,
        &mut clock,
        &boundary,
        "HEAD",
        "/api/v1/sites/ballydidean/rain-collection",
        &[],
    );
    let invalid = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/sites/ballydidean/rain-collection",
        &[QueryParameter {
            name: "unexpected",
            value: "true",
        }],
    );
    repository.rain_collection_available = false;
    let unavailable = request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        "/api/v1/sites/ballydidean/rain-collection",
        &[],
    );

    assert_eq!(
        [
            response.status,
            head.status,
            invalid.status,
            unavailable.status
        ],
        [200, 200, 400, 404]
    );
    assert_eq!(clock.reads, 4);
    assert_eq!(repository.site_reads, 4);
    assert_eq!(repository.rain_collection_reads, 3);
    assert_eq!(repository.rain_adjustment_reads.len(), 3);
    assert!(repository.rain_adjustment_reads.iter().all(|read| {
        read.site_slug == "ballydidean" && read.as_of == "2026-08-22T05:00:00.000Z"
    }));
    assert_eq!(head.body, None);
    assert_eq!(error_code(&invalid), "invalid_query");
    assert_eq!(error_code(&unavailable), "not_found");

    let Some(WeatherServiceBody::RainCollection(body)) = response.body else {
        panic!("rain collection response body is missing");
    };
    assert_eq!(body.generatedAt, "2026-08-22T05:00:00.000Z");
    assert_eq!(body.version, "development");
    assert_eq!(body.data.expectedStations, 12);
    assert!(body.data.stationAccessAuthorized);
    assert!(!body.data.modelEnabled);
    assert_eq!(body.data.claims, 12);
    assert_eq!(body.data.compressedBytes, 4096);
    assert_eq!(
        body.data.lastStationReceiptAt,
        Nullable::Value("2026-09-13T01:03:00.000Z".to_owned())
    );
}

// verifies collection model availability uses the same retained-run gate as forecast serving
#[test]
fn projects_rain_collection_model_availability_from_retained_output() {
    let boundary = FixtureBoundary;
    let active_runtime = AdmittedRainForecastRuntime::Active {
        loaded_at: "2026-08-22T04:59:00.000Z".to_owned(),
    };
    let disabled_runtime = AdmittedRainForecastRuntime::Disabled {
        loaded_at: "2026-08-22T04:58:00.000Z".to_owned(),
        reason_code: ApiRainAdjustmentRuntimeReasonCodeEnum::RegistryInvalid,
    };

    let cases = [
        (
            FixtureRepository {
                rain_collection_available: true,
                rain_adjustment_available: true,
                ..FixtureRepository::default()
            },
            Some(&active_runtime),
            true,
            1,
        ),
        (
            FixtureRepository {
                rain_collection_available: true,
                rain_adjustment_available: true,
                invalid_rain_adjustment: true,
                ..FixtureRepository::default()
            },
            Some(&active_runtime),
            false,
            1,
        ),
        (
            FixtureRepository {
                rain_collection_available: true,
                ..FixtureRepository::default()
            },
            Some(&active_runtime),
            false,
            1,
        ),
        (
            FixtureRepository {
                rain_collection_available: true,
                rain_adjustment_available: true,
                ..FixtureRepository::default()
            },
            Some(&disabled_runtime),
            false,
            0,
        ),
    ];

    // exercise active, stale, missing, and startup-disabled model states independently
    for (mut repository, runtime, expected_enabled, expected_reads) in cases {
        let mut clock = FixtureClock { reads: 0 };
        let nominal_data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
        let inactive_runtime = inactive_forecast_runtime();
        let response = serve_decoded_request(
            &mut repository,
            &mut clock,
            &WeatherServiceBoundaries {
                nominal_data_at: Some(&nominal_data_at),
                time_resolver: &boundary,
                history: &boundary,
                tides: &boundary,
                forecast: &boundary,
                forecast_runtime: &inactive_runtime,
                forecast_wind_runtime: None,
                forecast_temperature_runtime: None,
                forecast_rain_runtime: runtime,
                trends: &boundary,
                daily_precipitation: &boundary,
                rain_collection: &boundary,
                forecast_observation: &boundary,
                version: None,
            },
            DecodedServiceRequest {
                method: "GET",
                pathname: "/api/v1/sites/ballydidean/rain-collection",
                query: &[],
            },
        );
        let Some(WeatherServiceBody::RainCollection(body)) = response.body else {
            panic!("rain collection availability body is missing");
        };
        assert_eq!(body.data.modelEnabled, expected_enabled);
        // preserve the startup-disabled zero-effect branch independently
        let expected_rain_reads = if expected_reads == 0 {
            Vec::new()
        } else {
            vec![ForecastRainAdjustmentRead {
                site_slug: "ballydidean".to_owned(),
                as_of: "2026-08-22T05:00:00.000Z".to_owned(),
            }]
        };
        assert_eq!(repository.rain_adjustment_reads, expected_rain_reads);
        assert_eq!(clock.reads, 1);
    }
}

// verifies public rain fallback diagnostics retain the source route's exact observable semantics
#[test]
fn reports_public_rain_fallbacks_after_the_final_wire_status() {
    #[derive(Clone, Copy)]
    enum RetainedRunFixture {
        ReadError,
        WrongModel,
        InvalidLifecycle,
        Missing,
        Available,
    }

    struct Case<'runtime> {
        name: &'static str,
        run: RetainedRunFixture,
        method: &'static str,
        runtime: Option<&'runtime AdmittedRainForecastRuntime>,
        sink_fails: bool,
        encoder_fails: bool,
        expected_diagnostic: bool,
        expected_reads: usize,
        expected_status: u16,
        expected_body: &'static [u8],
    }

    let boundary = FixtureBoundary;
    let active_runtime = AdmittedRainForecastRuntime::Active {
        loaded_at: "2026-08-22T04:59:00.000Z".to_owned(),
    };
    let disabled_runtime = AdmittedRainForecastRuntime::Disabled {
        loaded_at: "2026-08-22T04:58:00.000Z".to_owned(),
        reason_code: ApiRainAdjustmentRuntimeReasonCodeEnum::PolicyRaw,
    };
    let cases = [
        Case {
            name: "read error",
            run: RetainedRunFixture::ReadError,
            method: "GET",
            runtime: Some(&active_runtime),
            sink_fails: false,
            encoder_fails: false,
            expected_diagnostic: true,
            expected_reads: 1,
            expected_status: 200,
            expected_body: RAIN_COLLECTION_WIRE,
        },
        Case {
            name: "wrong model",
            run: RetainedRunFixture::WrongModel,
            method: "GET",
            runtime: Some(&active_runtime),
            sink_fails: false,
            encoder_fails: false,
            expected_diagnostic: true,
            expected_reads: 1,
            expected_status: 200,
            expected_body: RAIN_COLLECTION_WIRE,
        },
        Case {
            name: "invalid lifecycle head",
            run: RetainedRunFixture::InvalidLifecycle,
            method: "HEAD",
            runtime: Some(&active_runtime),
            sink_fails: false,
            encoder_fails: false,
            expected_diagnostic: true,
            expected_reads: 1,
            expected_status: 200,
            expected_body: &[],
        },
        Case {
            name: "ordinary missing",
            run: RetainedRunFixture::Missing,
            method: "GET",
            runtime: Some(&active_runtime),
            sink_fails: false,
            encoder_fails: false,
            expected_diagnostic: false,
            expected_reads: 1,
            expected_status: 200,
            expected_body: RAIN_COLLECTION_WIRE,
        },
        Case {
            name: "policy disabled",
            run: RetainedRunFixture::Available,
            method: "GET",
            runtime: Some(&disabled_runtime),
            sink_fails: false,
            encoder_fails: false,
            expected_diagnostic: false,
            expected_reads: 0,
            expected_status: 200,
            expected_body: RAIN_COLLECTION_WIRE,
        },
        Case {
            name: "throwing sink",
            run: RetainedRunFixture::ReadError,
            method: "GET",
            runtime: Some(&active_runtime),
            sink_fails: true,
            encoder_fails: false,
            expected_diagnostic: true,
            expected_reads: 1,
            expected_status: 200,
            expected_body: RAIN_COLLECTION_WIRE,
        },
        Case {
            name: "final encoding failure",
            run: RetainedRunFixture::InvalidLifecycle,
            method: "GET",
            runtime: Some(&active_runtime),
            sink_fails: false,
            encoder_fails: true,
            expected_diagnostic: true,
            expected_reads: 1,
            expected_status: 500,
            expected_body: INTERNAL_ERROR_WIRE,
        },
    ];

    // exercise each source behavior through one shared public wire boundary
    for case in cases {
        let mut repository = FixtureRepository {
            rain_collection_available: true,
            ..FixtureRepository::default()
        };
        // select only the retained-run condition owned by this case
        match case.run {
            RetainedRunFixture::ReadError => {
                repository.rain_adjustment_available = true;
                repository.fail_rain_adjustment = true;
            }
            RetainedRunFixture::WrongModel => {
                repository.rain_adjustment_available = true;
                repository.wrong_model_rain_adjustment = true;
            }
            RetainedRunFixture::InvalidLifecycle => {
                repository.rain_adjustment_available = true;
                repository.invalid_rain_adjustment = true;
            }
            RetainedRunFixture::Missing => {}
            RetainedRunFixture::Available => {
                repository.rain_adjustment_available = true;
            }
        }
        let mut clock = FixtureClock { reads: 0 };
        let nominal_data_at = instant(1_787_374_800_000, "2026-08-22T05:00:00.000Z");
        let inactive_runtime = inactive_forecast_runtime();
        let boundaries = WeatherServiceBoundaries {
            nominal_data_at: Some(&nominal_data_at),
            time_resolver: &boundary,
            history: &boundary,
            tides: &boundary,
            forecast: &boundary,
            forecast_runtime: &inactive_runtime,
            forecast_wind_runtime: None,
            forecast_temperature_runtime: None,
            forecast_rain_runtime: case.runtime,
            trends: &boundary,
            daily_precipitation: &boundary,
            rain_collection: &boundary,
            forecast_observation: &boundary,
            version: None,
        };
        let mut encoder = RainDiagnosticWireEncoder {
            count: 0,
            fail: case.encoder_fails,
        };
        let mut diagnostics = Vec::new();
        let response = {
            let mut reporter = |diagnostic| {
                diagnostics.push(diagnostic);
                // contain a reporting sink failure without changing the public reply
                if case.sink_fails { Err(()) } else { Ok(()) }
            };
            serve_wire_decoded_request_with_rain_diagnostics(
                &mut repository,
                &mut clock,
                &boundaries,
                DecodedServiceRequest {
                    method: case.method,
                    pathname: "/api/v1/sites/ballydidean/rain-collection",
                    query: &[],
                },
                &mut encoder,
                &mut reporter,
            )
        };

        assert_eq!(response.status, case.expected_status, "{}", case.name);
        assert_eq!(response.body, case.expected_body, "{}", case.name);
        assert_eq!(encoder.count, 1, "{}", case.name);
        assert_eq!(clock.reads, 1, "{}", case.name);
        assert_eq!(repository.site_reads, 1, "{}", case.name);
        assert_eq!(repository.rain_collection_reads, 1, "{}", case.name);
        assert_eq!(
            repository.rain_adjustment_reads.len(),
            case.expected_reads,
            "{}",
            case.name
        );
        assert_eq!(
            diagnostics.len(),
            usize::from(case.expected_diagnostic),
            "{}",
            case.name
        );
        // validate every emitted field only for the cases that require one event
        if let Some(diagnostic) = diagnostics.first() {
            assert_eq!(diagnostic.error_code, "rain_sidecar_read_failed");
            assert_eq!(diagnostic.error_name, "UnknownError");
            assert_eq!(diagnostic.event, "rain_adjustment_fallback");
            assert_eq!(diagnostic.method, case.method);
            assert_eq!(diagnostic.operation, "sidecar_read");
            assert_eq!(diagnostic.status, case.expected_status);
        }
    }
}

// verifies the private listener owner executes the compact input without public site discovery
#[test]
fn serves_forecast_observation_input_only_through_private_authority() {
    let boundary = FixtureBoundary;
    let mut repository = FixtureRepository {
        observation_available: true,
        ..FixtureRepository::default()
    };
    let mut clock = PrivateFixtureClock { reads: 0 };
    let response = private_request(&mut repository, &mut clock, &boundary, "GET", &[]);
    let head = private_request(&mut repository, &mut clock, &boundary, "HEAD", &[]);
    let invalid = private_request(
        &mut repository,
        &mut clock,
        &boundary,
        "GET",
        &[QueryParameter {
            name: "from",
            value: "x",
        }],
    );
    let method = private_request(&mut repository, &mut clock, &boundary, "POST", &[]);
    repository.observation_available = false;
    let missing = private_request(&mut repository, &mut clock, &boundary, "GET", &[]);
    repository.fail_observation = true;
    let failure = private_request(&mut repository, &mut clock, &boundary, "GET", &[]);

    assert_eq!(
        [
            response.status,
            head.status,
            invalid.status,
            method.status,
            missing.status,
            failure.status,
        ],
        [200, 200, 400, 405, 404, 500]
    );
    assert_eq!(clock.reads, 6);
    assert_eq!(repository.site_reads, 0);
    assert_eq!(repository.observation_plans.len(), 4);
    assert_eq!(head.body, None);
    assert_eq!(error_code(&invalid), "invalid_query");
    assert_eq!(error_code(&method), "method_not_allowed");
    assert_eq!(error_code(&missing), "not_found");
    assert_eq!(error_code(&failure), "internal_error");
    assert_eq!(
        repository.observation_plans[0].binds,
        [ForecastObservationInputBind::UtcInstant(
            "2026-08-22T05:00:00.000Z".to_owned()
        )]
    );
    assert_eq!(FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES, 4 * 1024 * 1024);

    let Some(WeatherServiceBody::ForecastObservationInput(body)) = response.body else {
        panic!("private forecast observation response body is missing");
    };
    assert_eq!(
        ForecastObservationInputResponseContractVersionLiteral::WIRE_VALUE,
        "forecast-observation-input/v1"
    );
    assert_eq!(body.generatedAt, "2026-08-22T05:00:00.000Z");
    assert_eq!(body.projectionFrom, "2026-08-21T07:00:00.000Z");
    assert_eq!(body.queryFrom, "2026-08-21T04:00:00.000Z");
    assert_eq!(body.through, "2026-08-22T05:00:00.000Z");
    assert_eq!(body.records.len(), 1);
    assert_eq!(body.records[0].id, "100");
    assert_eq!(body.records[0].provenance.providerKey, "ecowitt-local");
    assert_eq!(body.records[0].metrics.temperatureC, Nullable::Value(10.0));
}

/// enforce the private encoded body limit through real route dispatch
#[test]
fn private_observation_wire_enforces_exact_body_limit_before_head_suppression() {
    const INTERNAL_ERROR_WIRE: &[u8] =
        b"{\"error\":{\"code\":\"internal_error\",\"message\":\"Unexpected server error\"}}";

    let boundary = FixtureBoundary;
    let mut repository = FixtureRepository {
        observation_available: true,
        ..FixtureRepository::default()
    };
    let mut clock = PrivateFixtureClock { reads: 0 };
    let exact_bytes = vec![b'x'; FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES];
    let oversized_bytes = vec![b'x'; FORECAST_OBSERVATION_INPUT_MAX_BODY_BYTES + 1];
    let cases = [
        ("GET", exact_bytes.as_slice(), 200, exact_bytes.as_slice()),
        ("HEAD", exact_bytes.as_slice(), 200, &[] as &[u8]),
        ("GET", oversized_bytes.as_slice(), 500, INTERNAL_ERROR_WIRE),
        ("HEAD", oversized_bytes.as_slice(), 500, &[] as &[u8]),
    ];

    // exercise both methods on each side of the exact byte boundary
    for (index, (method, bytes, expected_status, expected_body)) in cases.into_iter().enumerate() {
        let mut encoder = ObservationWireEncoder { bytes, count: 0 };
        let response =
            private_wire_request(&mut repository, &mut clock, &boundary, method, &mut encoder);
        assert_eq!(response.status, expected_status);
        assert_eq!(response.headers.cache_control, Some("no-store"));
        assert_eq!(response.body, expected_body);
        assert_eq!(repository.observation_plans.len(), index + 1);
        assert_eq!(encoder.count, 1);
    }
    assert_eq!(clock.reads, 4);
    assert_eq!(repository.site_reads, 0);
}
