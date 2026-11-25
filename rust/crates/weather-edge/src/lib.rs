mod admin_adjustment_settings;
mod admin_adjustment_settings_file;
mod admin_forecast_adjustment_scorecard;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod admin_forecast_adjustment_scorecard_file;
mod admin_property_sensor_layout;
mod admin_property_sensor_layout_file;
mod admin_session;
mod admin_session_application;
mod archive_auxiliary_wire;
mod archive_wire;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod cold_entry_wire;
mod edge_dispatch;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod edge_process_environment;
mod edge_response_lifecycle;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod edge_startup_admission;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod edge_startup_configuration;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod edge_startup_lifecycle;
mod edge_wire_admin_response;
mod edge_wire_json_responses;
mod edge_wire_response;
mod edge_xweather_credentials;
mod forecast_cycle_pages;
mod forecast_cycle_reconcile;
mod forecast_evidence_material;
mod forecast_evidence_rows;
mod forecast_recorded_loader;
mod forecast_recorded_products;
mod forecast_response_capture;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod forecast_scheduler_capture_driver;
mod forecast_scheduler_due;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod forecast_scheduler_poll_driver;
mod home_network;
mod native_public_assets;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_revision_spool;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
mod native_revision_spool_file;
mod permanent_archive_gap;
mod private_archive_ports;
mod private_archive_service;
mod private_archive_wire_request;
mod private_archive_wire_response;
mod private_file_state;
mod private_revision_archive;
mod private_shadow_archive;
mod producer_projection;
mod producer_projection_fixed_gauge;
mod producer_projection_legacy;
mod producer_projection_rain;
mod public_adjustment_projection;
mod public_api_proxy;
mod public_serving_application;
mod public_templates;
mod rain_control_state_archive;
mod rain_fixed_gauge_gap_archive;
mod remote_agents_preview;
mod revision_archive_canonical;
mod revision_cold_ack;
mod revision_cold_ack_codec;
mod revision_cold_ack_material;
mod revision_cold_page;
mod revision_gap_ack;
mod revision_gap_command;
mod revision_gap_transfer;
mod revision_online_spool;
mod revision_receipt_wire;
mod shadow_receipt_wire;
mod widget_forecast_application;
mod xweather_map_proxy;
mod xweather_tile_cache;
mod xweather_tile_transport;
mod xweather_usage_budget;

pub use admin_adjustment_settings::{
    AdmittedAdminMutation, AdmittedAdminOperator, DEFAULT_FORECAST_ADJUSTMENT_SETTINGS,
    DISABLED_FORECAST_ADJUSTMENT_SETTINGS, DecodedForecastAdjustmentSettings,
    ForecastAdjustmentSettings, ForecastAdjustmentSettingsAllow,
    ForecastAdjustmentSettingsDiagnostic, ForecastAdjustmentSettingsEnvelope,
    ForecastAdjustmentSettingsOutcome, ForecastAdjustmentSettingsRequest,
    ForecastAdjustmentSettingsStatus, ForecastAdjustmentSettingsStore,
    ForecastAdjustmentSettingsValidationError, InMemoryForecastAdjustmentSettingsStore,
    handle_forecast_adjustment_settings, validate_forecast_adjustment_settings,
};
pub use admin_adjustment_settings_file::{
    AdmittedForecastAdjustmentSettingsInitialization, AdmittedForecastAdjustmentSettingsJson,
    FileForecastAdjustmentSettingsError, FileForecastAdjustmentSettingsStore,
    ForecastAdjustmentSettingsInitializationEvidence,
    ForecastAdjustmentSettingsInitializationEvidenceReader, ForecastAdjustmentSettingsJsonAdapter,
    ForecastAdjustmentSettingsTemporaryPathSource,
};
pub use admin_forecast_adjustment_scorecard::{
    AdmittedForecastAdjustmentScorecardJson, AdmittedForecastAdjustmentScorecardNow,
    AdmittedForecastAdjustmentScorecardResponseJson, AdmittedForecastAdjustmentScorecardSnapshot,
    ForecastAdjustmentScorecardAuthority, ForecastAdjustmentScorecardBody,
    ForecastAdjustmentScorecardCandidates, ForecastAdjustmentScorecardClock,
    ForecastAdjustmentScorecardContractVersion, ForecastAdjustmentScorecardEncoder,
    ForecastAdjustmentScorecardHeaders, ForecastAdjustmentScorecardMethod,
    ForecastAdjustmentScorecardPublicationState, ForecastAdjustmentScorecardRequest,
    ForecastAdjustmentScorecardResponse, ForecastAdjustmentScorecardStore,
    handle_forecast_adjustment_scorecard,
};
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use admin_forecast_adjustment_scorecard_file::{
    FileForecastAdjustmentScorecardError, FileForecastAdjustmentScorecardStore,
    ForecastAdjustmentScorecardFileAdapter,
};
pub use admin_property_sensor_layout::{
    AdmittedPropertySensorUpdateInstant, DecodedPropertySensorLayoutUpdate,
    InMemoryPropertySensorLayoutStore, PropertySensorIcon, PropertySensorLayoutAllow,
    PropertySensorLayoutEntry, PropertySensorLayoutOutcome, PropertySensorLayoutRequest,
    PropertySensorLayoutStore, PropertySensorLayoutUpdateInstantSource,
    PropertySensorLayoutValidationError, PropertySensorMapCenter, handle_property_sensor_layout,
    validate_property_sensor_layout_update,
};
pub use admin_property_sensor_layout_file::{
    AdmittedPropertySensorLayoutJson, FilePropertySensorLayoutError, FilePropertySensorLayoutStore,
    PropertySensorLayoutJsonAdapter, PropertySensorLayoutTemporaryPathSource,
};
pub use admin_session::{
    ADMIN_SESSION_TTL_SECONDS, AdmittedAdminCredential, AdmittedAdminCredentialDigest,
    AdmittedAdminPasswordVerifier, AdmittedAdminPersistentAuthState, AdmittedAdminSessionDigest,
    AdmittedAdminSessionNow, AdmittedAdminSessionToken, MAXIMUM_ADMIN_SESSIONS,
    WeatherAdminAuthStatePort, WeatherAdminBootstrapResponse, WeatherAdminBootstrapStatus,
    WeatherAdminPortError, WeatherAdminSessionClock, WeatherAdminSessionMaterialPort,
    WeatherAdminSessionResponse, WeatherAdminStore, WeatherAdminStoreError,
};
pub use admin_session_application::{
    AdmittedAdminCookieMode, AdmittedAdminLoginPage, AdmittedAdminRouteInput,
    AdmittedViewerContext, WeatherAdminRouteAllow, WeatherAdminRouteOutcome,
    WeatherAdminSessionApplication, WeatherAdminSessionApplicationPort,
    WeatherViewerContextOutcome, handle_admin_bootstrap, handle_admin_login, handle_admin_logout,
};
pub use archive_auxiliary_wire::{
    fixed_gauge_gap_input_value, fixed_gauge_status_input_value, rain_control_gap_input_value,
    revision_gap_input_value, shadow_gap_input_value,
};
pub use archive_wire::{ArchiveWireCodec, ArchiveWirePrimitives};
pub use edge_dispatch::{
    AdmittedPropertySensorMutation, WeatherEdgeDispatchBoundary, WeatherEdgeDispatchOutcome,
    WeatherEdgeDispatchPortSet, WeatherEdgeDispatchPorts, WeatherEdgeDispatchRequest,
    dispatch_weather_edge_request,
};
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use edge_process_environment::*;
pub use edge_response_lifecycle::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use edge_startup_admission::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use edge_startup_configuration::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use edge_startup_lifecycle::*;
pub use edge_wire_admin_response::EdgeWireAdminCookie;
pub use edge_wire_json_responses::EdgeWireJsonEncoder;
pub use edge_wire_response::*;
pub use edge_xweather_credentials::*;
pub use forecast_cycle_pages::*;
pub use forecast_cycle_reconcile::*;
pub use forecast_evidence_material::*;
pub use forecast_recorded_loader::*;
pub use forecast_recorded_products::{
    AdmittedForecastObservationsJson, AdmittedForecastRecordedNow,
    AdmittedForecastRecordedProducts, AdmittedSolarCloudEvidenceJson, ForecastRecordedProductBody,
    ForecastRecordedProductHeaders, ForecastRecordedProductMethod, ForecastRecordedProductRequest,
    ForecastRecordedProductResponse, ForecastRecordedProductRoute, ForecastRecordedProductsCache,
    ForecastRecordedProductsCacheError, ForecastRecordedProductsClock,
    ForecastRecordedProductsLoader, ForecastRecordedProductsRead, handle_forecast_recorded_product,
};
pub use forecast_response_capture::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use forecast_scheduler_capture_driver::*;
pub use forecast_scheduler_due::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use forecast_scheduler_poll_driver::*;
pub use home_network::*;
pub use native_public_assets::*;
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_revision_spool::{
    BoundForecastArchiveLockClock, BoundForecastArchiveLockCodec, BoundForecastCyclePageCodec,
    DurableForecastCycleGap, FORECAST_CYCLE_GAP_CONTRACT_VERSION, FORECAST_CYCLE_GAP_DISPOSITION,
    FORECAST_CYCLE_TRANSFER_CHECKPOINT_CONTRACT_VERSION, ForecastArchiveLockClock,
    ForecastArchiveLockCodec, ForecastCycleDiskAckError, ForecastCycleDiskAckReadCodec,
    ForecastCycleDiskAckSlotPort, ForecastCycleDiskSlotCodec, ForecastCycleDiskSlotError,
    ForecastCycleDiskSlotMetadata, ForecastCycleDiskSlotPage, ForecastCycleGapCodec,
    ForecastCycleGapError, ForecastCycleGapOutcome, ForecastCycleGapReason, ForecastCycleGapRecord,
    ForecastCycleHashPair, ForecastCycleTransferCheckpoint, ForecastCycleTransferCheckpointCodec,
    ForecastCycleTransferCheckpointDurabilityProof, ForecastCycleTransferCheckpointError,
    NativeForecastArchiveLeaseError, NativeForecastCycleDiskAcknowledgements,
    NativeForecastCycleDiskSlots, NativeForecastCycleRecovery, NativeForecastCycleRecoveryError,
    NativeForecastCycleTransferCheckpoints, recover_native_forecast_cycle,
    with_native_forecast_archive_lease,
};
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_revision_spool::{
    BoundedForecastSchedulerSlotReader, FORECAST_SCHEDULER_CAPTURE_PATH,
    FORECAST_SCHEDULER_HISTORY_LIMIT, FORECAST_SCHEDULER_POLL_INTERVAL,
    FORECAST_SCHEDULER_STATE_CONTRACT_VERSION, FORECAST_SCHEDULER_STATE_MAXIMUM_BYTES,
    ForecastSchedulerAdmission, ForecastSchedulerArchivePayloadError, ForecastSchedulerAttempt,
    ForecastSchedulerCaptureRequest, ForecastSchedulerConfiguration, ForecastSchedulerError,
    ForecastSchedulerGap, ForecastSchedulerGapReason, ForecastSchedulerHorizon,
    ForecastSchedulerPorts, ForecastSchedulerSlotDecoder, ForecastSchedulerSlotsError,
    ForecastSchedulerSlotsPort, ForecastSchedulerState, ForecastSchedulerStateCodec,
    ForecastSchedulerStateStoreError, ForecastSchedulerTick, ForecastSchedulerTickError,
    ForecastSchedulerTriggerPort, NativeForecastScheduler, NativeForecastSchedulerStateStore,
    evaluate_forecast_scheduler_admission, read_committed_scheduler_payload,
    validate_forecast_scheduler_horizon,
};
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_revision_spool::{
    DecodedForecastChannelAssertion, DecodedForecastChannelBinding, DecodedForecastChannelReceipt,
    DecodedForecastObjectReceipt, EncodedForecastChannelAssertion, EncodedForecastChannelBinding,
    EncodedForecastObjectReceipt, FORECAST_ALLOCATION_FREE_INODE_FLOOR,
    FORECAST_ALLOCATION_MAXIMUM_BYTES, FORECAST_ALLOCATION_MAXIMUM_CHANNEL_ASSERTIONS,
    FORECAST_ALLOCATION_MAXIMUM_IDENTITIES, FORECAST_ALLOCATION_REQUIRED_FREE_BYTES,
    FORECAST_CHANNEL_ASSERTION_MAXIMUM_BYTES, FORECAST_CHANNEL_MAXIMUM_ASSERTIONS,
    FORECAST_EVIDENCE_SNAPSHOT_CONTRACT_VERSION, FORECAST_OBJECT_MAXIMUM_BYTES,
    FORECAST_RECEIPT_MAXIMUM_BYTES, FORECAST_RECEIPT_V2_MAXIMUM_BYTES, ForecastAllocationCensus,
    ForecastAllocationRefusal, ForecastAllocationStatfsPort, ForecastAllocationStatfsSample,
    ForecastChannelAllocationDecision, ForecastChannelAllocationPort,
    ForecastChannelAllocationRequest, ForecastChannelCodec, ForecastChannelCommitError,
    ForecastChannelCommitInput, ForecastChannelCommitOutcome, ForecastChannelCommitRefusal,
    ForecastChannelPublication, ForecastChannelPublicationKind, ForecastIssuanceChannel,
    ForecastObjectAllocationDecision, ForecastObjectAllocationPort,
    ForecastObjectAllocationRequest, ForecastObjectCommitCodec, ForecastObjectCommitError,
    ForecastObjectCommitInput, ForecastObjectCommitOutcome, ForecastObjectCommitRefusal,
    ForecastObjectPublication, ForecastObjectPublicationKind, ForecastObjectReceiptVersion,
    ForecastObjectSha256, ForecastSnapshotCodec, ForecastSnapshotWatermarkEntry,
    FrozenForecastEvidenceCatalog, FrozenForecastEvidenceEntry, NativeForecastCaptureStore,
    NativeForecastCaptureStoreError, NativeForecastChannelStore, NativeForecastEvidenceStatus,
    NativeForecastObjectAllocation, NativeForecastObjectStore, NativeForecastSnapshotCatalog,
    NativeForecastSnapshotError, NativeRevisionGapAckDecoder, NativeRevisionGapAckStorage,
    NativeRevisionGapCommandCodec, NativeRevisionGapCommandError, NativeRevisionGapCommandPorts,
    NativeRevisionGapMember, NativeRevisionGapPayloadKind, NativeRevisionGapProjection,
    NativeRevisionGapRainAuxiliary, NativeRevisionGapRainBinding, NativeRevisionGapShadowStage,
    NativeRevisionGapTransferCodec, NativeRevisionGapTransferError, NativeRevisionSpool,
    NativeRevisionSpoolArtifactCodec, RevisionSpoolLeaseCodec,
};
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_revision_spool::{
    FORECAST_ARCHIVE_OPERATOR_MAXIMUM_BYTES, ForecastArchiveOperatorApplication,
    ForecastArchiveOperatorArgumentError, ForecastArchiveOperatorCommand,
    ForecastArchiveOperatorError, ForecastCycleOnlinePageAckError, ForecastCycleOnlinePageAckPorts,
    ForecastCycleOnlinePageAckRequest, ForecastCycleOnlineSealError, ForecastCycleOnlineSealPorts,
    ForecastCycleOnlineSealRequest, ForecastCycleOnlineWireError, NativeForecastArchiveOperator,
    NativeForecastArchiveOperatorError, acknowledge_forecast_cycle_online_page,
    encode_forecast_cycle_final_acknowledgement, encode_forecast_cycle_online_next,
    encode_forecast_cycle_page_acknowledgement, parse_forecast_archive_operator_command,
    run_forecast_archive_operator_command,
};
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_revision_spool::{
    FORECAST_ARCHIVE_TRANSFER_CONTRACT_VERSION, FORECAST_CYCLE_FINAL_ACK_CONTRACT_VERSION,
    ForecastCycleCapsuleCodec, ForecastCycleCapsuleError, ForecastCycleCapsuleManifestLayout,
    ForecastCycleCapsulePageRange, ForecastCycleCurrentRecoveryError, ForecastCycleFinalAckCodec,
    ForecastCycleFinalAckDurabilityProof, ForecastCycleFinalAckError,
    ForecastCycleFinalAcknowledgement, ForecastCycleOnlineNext, ForecastCycleOnlineNextError,
    ForecastCyclePendingFinal, ForecastCyclePendingFinalProvider,
    NativeForecastCycleFinalAcknowledgements, NativeForecastCycleOnlineTransfer,
    NativeForecastCycleOnlineTransferError, NativeForecastOnlineNext,
    NativeForecastPendingFinalError, NativeForecastTransferStorageError,
    ValidatedForecastCycleCapsule, encode_forecast_cycle_capsule, next_forecast_cycle_online,
    next_native_forecast_final, recover_current_forecast_cycle_slot,
    validate_forecast_cycle_capsule,
};
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_revision_spool::{
    FORECAST_CYCLE_INPUT_SEAL_CONTRACT_VERSION, FinalizedForecastCycleState,
    ForecastCycleFinalManifest, ForecastCycleFinalManifestCodec, ForecastCycleFinalManifestError,
    ForecastCycleFinalizationError, ForecastCycleInputSeal, ForecastCycleInputSealCodec,
    ForecastCycleInputSealDurabilityProof, ForecastCycleInputSealError, ForecastCycleMetadataCodec,
    ForecastCycleRequiredInputRoot, ForecastCycleRequiredInputs, NativeForecastCycleFinalManifests,
    NativeForecastCycleFinalizationError, NativeForecastCycleFinalizationOutcome,
    NativeForecastCycleInputSeals, RetainedForecastCycleManifest, finalize_native_forecast_cycle,
    prepare_forecast_cycle_final_manifest, prepare_forecast_cycle_input_seal,
};
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_revision_spool::{
    ForecastCaptureCoordinatorInputError, ForecastCaptureCoordinatorOperationError,
    ForecastCaptureCoordinatorOutcome, ForecastCaptureCoordinatorPorts,
    ForecastCaptureCoordinatorStatus, ForecastCaptureCustodyError,
    ForecastCycleTransferAcknowledgement, NativeForecastSchedulerArchive,
    NativeForecastSchedulerArchiveError, capture_committed_scheduler_evidence,
};
#[cfg(all(
    target_os = "linux",
    any(target_arch = "x86_64", target_arch = "aarch64")
))]
pub use native_revision_spool::{
    NativeRevisionColdAckCodec, NativeRevisionColdBatchGroup, NativeRevisionColdEntryCodec,
    NativeRevisionColdEntryError, NativeRevisionColdEntryMaterial, NativeRevisionColdGroup,
    NativeRevisionColdGroupCodec, NativeRevisionColdGroupError, NativeRevisionColdPageCodec,
    NativeRevisionColdPayloadKind, NativeRevisionColdPublication, NativeRevisionColdRainControl,
    NativeRevisionColdReceipt, NativeRevisionColdStageReceipt, NativeRevisionColdSuccessor,
};
pub use permanent_archive_gap::{
    PermanentArchiveGapAddress, PermanentArchiveGapCodec, PermanentArchiveGapInput,
    PermanentArchiveGapKind, PermanentArchiveGapStorage, PermanentArchiveGapStore,
};
pub use private_archive_ports::PrivateArchiveStores;
pub use private_archive_service::*;
pub use private_archive_wire_request::*;
pub use private_archive_wire_response::*;
pub use private_revision_archive::*;
pub use private_shadow_archive::*;
pub use producer_projection::admit_archive_revision_projection;
pub use public_adjustment_projection::*;
pub use public_api_proxy::*;
pub use public_serving_application::{
    AdminLoginPageApplicationRequest, AdminLoginPageRenderRequest, AdminLoginPageRenderer,
    AdminPageApplicationRequest, AdmittedPublicApiUpstreamResponse, AdmittedPublicAssetBytes,
    PublicApiProjection, PublicApiProxyClient, PublicApiProxyClientRequest,
    PublicServingApplicationBody, PublicServingApplicationBoundary,
    PublicServingApplicationHeaders, PublicServingApplicationOutcome,
    PublicServingApplicationRequest, PublicServingApplicationResponse, PublicServingHeaderProfile,
    PublicStaticAssetRead, PublicStaticTemplateRenderRequest, PublicStaticTemplateRenderer,
    handle_admin_login_page_application, handle_admin_page_application,
    handle_public_static_and_api,
};
pub use public_templates::*;
pub use rain_control_state_archive::{
    RainControlArchiveMetadataKind, RainControlExclusiveWrite, RainControlProjectionBinding,
    RainControlStateArchive, RainControlStateArchiveCodec, RainControlStateArchiveStorage,
    RainControlStateWireCodec,
};
pub use rain_fixed_gauge_gap_archive::{
    RAIN_FIXED_GAUGE_GAP_DIRECTORY, RainFixedGaugeGapArchive, RainFixedGaugeGapArchiveError,
    RainFixedGaugeGapCodec, RainFixedGaugeGapFileName, RainFixedGaugeGapStorage,
};
pub use remote_agents_preview::{
    AdmittedRemoteAgentsPreviewTarget, RemoteAgentsPreviewMode, RemoteAgentsPreviewResponse,
    handle_remote_agents_preview,
};
pub use revision_archive_canonical::{
    RevisionArchiveCanonicalClock, RevisionArchiveCanonicalCodec,
};
pub use revision_cold_ack::*;
pub use revision_cold_ack_codec::{
    RevisionColdAckJsonCodec, decode_revision_cold_ack, encode_revision_cold_ack,
};
pub use revision_cold_ack_material::{RevisionColdAckMaterialCodec, derive_revision_cold_ack_page};
pub use revision_cold_page::{
    REVISION_COLD_PAGE_MAXIMUM_BYTES, RevisionColdCursor, RevisionColdPageError,
    RevisionColdPageRequest, parse_revision_cold_page_command,
};
pub use revision_gap_ack::*;
pub use revision_gap_command::*;
pub use revision_gap_transfer::*;
pub use revision_online_spool::{
    REVISION_ONLINE_PAYLOAD_MAXIMUM_BYTES, REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS,
    RevisionOnlinePayload, RevisionOnlinePayloadAddress, RevisionOnlinePayloadAdmission,
    RevisionOnlinePayloadKind, RevisionOnlineSpoolError, RevisionOnlineSpoolStorage,
    with_revision_online_payload,
};
pub use revision_receipt_wire::{commit_receipt_value, revision_stage_receipt_value};
pub use shadow_receipt_wire::{shadow_metadata_value, shadow_stage_receipt_value};
pub use widget_forecast_application::{
    AdmittedWidgetForecastReceivedAt, AdmittedWidgetForecastSnapshotJson,
    WidgetForecastApplicationBody, WidgetForecastApplicationHeaders,
    WidgetForecastApplicationMethod, WidgetForecastApplicationRequest,
    WidgetForecastApplicationResponse, WidgetForecastClient, WidgetForecastClientRequest,
    WidgetForecastReceivedAtSource, WidgetForecastSnapshotProjector,
    handle_widget_forecast_application,
};
pub use xweather_map_proxy::{
    AdmittedXweatherMapSite, AdmittedXweatherProvider, AdmittedXweatherValidTime,
    DecodedXweatherFrameRoute, DecodedXweatherMapRoute, XweatherMapProxyBody,
    XweatherMapProxyHeaders, XweatherMapProxyMethod, XweatherMapProxyRequest,
    XweatherMapProxyResponse, XweatherMapTileCache, XweatherMapTileCacheError,
    handle_xweather_map_proxy,
};
pub use xweather_tile_cache::{
    XweatherTileBody, XweatherTileBodyError, XweatherTileCacheConfigurationError,
    XweatherTileCacheGetError, XweatherTileCacheKey, XweatherTileCacheRequest,
    XweatherTileCacheResponse, XweatherTileCacheStats, XweatherTileCacheStatus, XweatherTileClock,
    XweatherTileFrame, XweatherTileLayer, XweatherTileLoad, XweatherTileLoader,
    XweatherTileMemoryCache, XweatherTilePhase,
};
pub use xweather_tile_transport::*;
pub use xweather_usage_budget::{
    AdmittedXweatherBudgetRetryAfter, AdmittedXweatherUsageCalendar, AdmittedXweatherUsageLedger,
    XweatherBudgetedTileFailureResponse, XweatherBudgetedTileLoadError, XweatherBudgetedTileLoader,
    XweatherMapUnits, XweatherMapUnitsError, XweatherUsageBudget,
    XweatherUsageBudgetConfigurationError, XweatherUsageBudgetCreateError,
    XweatherUsageBudgetError, XweatherUsageCalendarSource, XweatherUsageLedgerError,
    XweatherUsagePersistence, XweatherUsageSnapshot, xweather_budgeted_tile_failure_response,
};

use weather_contracts::{
    ApiForecastAdjustmentDecision, ApiForecastWeatherRecord, ApiRainAdjustmentDecisionStateEnum,
    ForecastAdjustmentDecision, ForecastAdjustmentMetric, ForecastAdjustmentWindCanaryMetric,
    ForecastTemperatureCanaryDecisionV1StateEnum, Nullable, SourceKind, WidgetForecastField,
    WidgetForecastHour, WidgetForecastMode, WidgetForecastReason,
    WidgetForecastSnapshotAttribution, WidgetForecastSnapshotAttributionLabelLiteral,
    WidgetForecastSnapshotAttributionLicenseUrlLiteral,
    WidgetForecastSnapshotAttributionProviderUrlLiteral, WidgetForecastSnapshotSite,
    WidgetForecastSnapshotSiteLatitudeLiteral, WidgetForecastSnapshotSiteLongitudeLiteral,
    WidgetForecastSnapshotSiteNameLiteral, WidgetForecastSnapshotSiteSlugLiteral,
    WidgetForecastSnapshotSiteTimezoneLiteral, WidgetForecastSource, WidgetForecastStatus,
    WidgetForecastV2Hour, WidgetForecastV3Snapshot, WidgetForecastV3SnapshotCalendar,
    WidgetForecastV3SnapshotSchemaVersionLiteral,
};

/// Public map center used by the forecast preload projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ForecastMapSite {
    pub latitude: f64,
    pub longitude: f64,
}

const TEN_MINUTES_MS: i64 = 10 * 60 * 1_000;
const HOUR_MS: i64 = 60 * 60 * 1_000;
const ADJUSTMENT_MAX_AGE_MS: i64 = 90 * 60 * 1_000;
const RAIN_DECISION_MAX_AGE_MS: i64 = 12 * HOUR_MS;

/// Canonical timestamp serialization supplied by an owning runtime adapter.
pub trait CanonicalTimeAdapter {
    /// Serialize one Unix millisecond frame for the Xweather route segment.
    fn xweather_valid_time(&self, epoch_ms: i64) -> String;

    /// Serialize one Unix millisecond instant as canonical UTC ISO-8601.
    fn utc_iso(&self, epoch_ms: i64) -> String;
}

/// Prevalidated source clocks paired with one canonical forecast record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetForecastSourceEpochs {
    pub generated_at_epoch_ms: i64,
    pub received_at_epoch_ms: i64,
    pub product_run_at_epoch_ms: Option<i64>,
}

/// Prevalidated clocks for one independently selected forecast source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetForecastIndependentSourceEpochs {
    pub received_at_epoch_ms: i64,
    pub product_run_at_epoch_ms: Option<i64>,
}

/// Complete typed clock boundary for one widget v1 hour projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetForecastV1Epochs {
    pub start_epoch_ms: i64,
    pub raw_source: WidgetForecastSourceEpochs,
    pub rain_source: Option<WidgetForecastIndependentSourceEpochs>,
    pub temperature_source: Option<WidgetForecastIndependentSourceEpochs>,
    pub rain_decision_at_epoch_ms: Option<i64>,
}

/// One canonical instant paired with its prevalidated epoch representation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetForecastCanonicalInstant {
    pub value: String,
    pub epoch_ms: i64,
}

/// Admitted calendar boundary for one complete widget v3 response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetForecastV3CalendarBoundary {
    pub date: String,
    pub day_start: WidgetForecastCanonicalInstant,
    pub day_end: WidgetForecastCanonicalInstant,
    pub cutoff: WidgetForecastCanonicalInstant,
    pub anchor_start_epoch_ms: i64,
    pub overnight_end: WidgetForecastCanonicalInstant,
    pub sunset: Option<WidgetForecastCanonicalInstant>,
}

/// Admitted response clocks and calendar for one widget v3 build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetForecastV3Boundary {
    pub generated_at: WidgetForecastCanonicalInstant,
    pub received_at: WidgetForecastCanonicalInstant,
    pub calendar: WidgetForecastV3CalendarBoundary,
}

/// One validated API row and its typed projection clocks.
#[derive(Debug, Clone, Copy)]
pub struct WidgetForecastV3Record<'a> {
    pub record: &'a ApiForecastWeatherRecord,
    pub epochs: WidgetForecastV1Epochs,
    pub generic_deadline: WidgetForecastAdjustmentDeadline,
    pub temperature_deadline: WidgetForecastAdjustmentDeadline,
}

/// Projection-only metric values retained after the API response has been filtered.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidgetForecastRecordMetrics {
    pub cloud_cover_percent: Option<f64>,
    pub precipitation_mm: Option<f64>,
    pub precipitation_rate_mm_per_hour: Option<f64>,
    pub temperature_c: Option<f64>,
    pub wind_speed_mps: Option<f64>,
}

/// One independently selected correction source retained by the widget algorithm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetForecastIndependentAdjustmentSource {
    pub run_initialized_at: String,
    pub first_received_at: String,
}

/// One independently selected correction after public settings filtering.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetForecastIndependentAdjustment {
    pub active: bool,
    pub corrected_value: Option<f64>,
    pub source: Option<WidgetForecastIndependentAdjustmentSource>,
}

/// One rain correction after public settings filtering, including its private decision clock.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetForecastRainAdjustment {
    pub active: bool,
    pub corrected_value: Option<f64>,
    pub decision_at: Option<String>,
    pub source: Option<WidgetForecastIndependentAdjustmentSource>,
}

/// The generic adjustment facts used by temperature and wind projection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WidgetForecastGenericAdjustment {
    pub temperature_applied: bool,
    pub adjusted_temperature_c: Option<f64>,
    pub wind_speed_applied: bool,
    pub adjusted_wind_speed_mps: Option<f64>,
}

/// Minimal truthful widget row independent of unrelated generated API metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct WidgetForecastRecordInput {
    pub metrics: WidgetForecastRecordMetrics,
    pub product_run_at: Option<String>,
    pub received_at: String,
    pub valid_at: String,
    pub is_forecast: bool,
    pub rain_adjustment: WidgetForecastRainAdjustment,
    pub generic_adjustment: WidgetForecastGenericAdjustment,
    pub temperature_adjustment: WidgetForecastIndependentAdjustment,
}

/// One minimal filtered row and its typed projection clocks for widget v3.
#[derive(Debug, Clone, Copy)]
pub struct WidgetForecastV3InputRecord<'a> {
    pub record: &'a WidgetForecastRecordInput,
    pub epochs: WidgetForecastV1Epochs,
    pub generic_deadline: WidgetForecastAdjustmentDeadline,
    pub temperature_deadline: WidgetForecastAdjustmentDeadline,
}

/// Parsed runtime deadline state supplied by the API adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetForecastAdjustmentDeadline {
    Unavailable,
    Permanent,
    AtEpochMilliseconds(i64),
}

/// Closed projection failures surfaced at the typed widget boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetForecastProjectionError {
    CanonicalTimestampMismatch,
    DuplicateOrForeignHour,
    GeneratedAfterReceived,
    GeneratedOutsideV3Window,
    HourTimestampMismatch,
    InvalidV3Calendar,
    InvalidV3HourCount,
    InvalidRain,
    InvalidTemperature,
    InvalidSelectedRain,
    InvalidSelectedTemperature,
    InvalidCloudCoverPercent,
    InvalidWindSpeedMps,
    InvalidSelectedWindSpeedMps,
    NonForecastHour,
    RainDecisionFromFuture,
    SelectedSourceIncomplete,
    SourceTimestampMismatch,
    SourceTimestampsOutOfOrder,
    TimestampOutOfRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SupplementalMetric {
    CloudCoverPercent,
    WindSpeedMps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WidgetForecastV1FieldKind {
    Rain,
    Temperature,
}

#[derive(Debug, Clone, PartialEq)]
struct WidgetForecastV1Selection {
    deadline_epoch_ms: Option<i64>,
    reason: WidgetForecastReason,
    source: WidgetForecastSource,
}

/// Request methods that affect ordinary public serving semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicRequestMethod {
    Get,
    Head,
    Other,
}

/// Explicit boundaries whose existing handler retains transport or authorization ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegatedServingBoundary {
    ViewerContext,
    AdminLogin,
    AdminLogout,
    AdminBootstrap,
    PropertySensorLayout,
    PublicAdjustmentSettings,
    AdminAdjustmentSettings,
    AdminAdjustmentScorecard,
    AdminPropertySensorUpdate,
    AdminPage,
    XweatherMap,
    RemoteAgentsPreview,
}

/// Closed public widget contract versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetForecastVersion {
    V1,
    V2,
    V3,
}

/// Fixed upstream forecast selection needed by one widget projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetForecastUpstream {
    Daily,
    Overnight,
}

/// One supported widget serving action without transport behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WidgetServingPlan {
    pub version: WidgetForecastVersion,
    pub upstream: WidgetForecastUpstream,
    pub omit_body: bool,
    pub cache_control: &'static str,
    pub content_type: &'static str,
}

/// One cached recorded-forecast projection owned by its existing handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordedForecastServingPlan {
    pub solar_cloud_evidence: bool,
    pub omit_body: bool,
    pub cache_control: &'static str,
    pub content_type: &'static str,
}

/// Upstream method selected by the read-only API proxy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiUpstreamMethod {
    Get,
    Head,
}

/// Content-type ownership retained by the API forwarding layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiContentTypePlan {
    Json,
    UpstreamOrJson,
}

/// Deterministic API forwarding metadata without transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApiProxyServingPlan {
    pub projection: PublicApiProjection,
    pub upstream_method: ApiUpstreamMethod,
    pub omit_body: bool,
    pub maximum_response_bytes: usize,
    pub timeout_ms: u64,
    pub upstream_success_cache_control: &'static str,
    pub upstream_non_success_cache_control: &'static str,
    pub upstream_success_content_type: ApiContentTypePlan,
    pub upstream_non_success_content_type: ApiContentTypePlan,
    pub vary_accept: bool,
    pub track_successful_forecast_response: bool,
}

/// Root selected after an exact static allowlist match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StaticAssetRoot {
    Public,
    Compiled,
}

/// Deterministic static response metadata for one allowlisted asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaticAssetServingPlan<'a> {
    pub root: StaticAssetRoot,
    pub relative_path: &'a str,
    pub template: bool,
    pub analytics_allowed: bool,
    pub omit_body: bool,
    pub cache_control: &'static str,
    pub content_type: &'static str,
}

/// Closed ordinary public rejection classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicServingRejection {
    NotFound,
    BadRequest,
    UnsupportedQuery,
    MethodNotAllowed,
}

impl PublicServingRejection {
    /// Return the exact HTTP status selected by the Node edge.
    pub const fn status_code(self) -> u16 {
        // map the closed rejection class without transport behavior
        match self {
            Self::NotFound => 404,
            Self::BadRequest | Self::UnsupportedQuery => 400,
            Self::MethodNotAllowed => 405,
        }
    }

    /// Return the stable bounded text body for this rejection.
    pub const fn body(self) -> &'static str {
        // preserve the route-specific public diagnostic
        match self {
            Self::NotFound => "not found\n",
            Self::BadRequest => "bad request\n",
            Self::UnsupportedQuery => "unsupported query\n",
            Self::MethodNotAllowed => "method not allowed\n",
        }
    }

    /// Return the shared text response cache policy.
    pub const fn cache_control(self) -> &'static str {
        "no-store"
    }

    /// Return the shared text response media type.
    pub const fn content_type(self) -> &'static str {
        "text/plain; charset=utf-8"
    }

    /// Return the optional read-only method advertisement.
    pub const fn allow(self) -> Option<&'static str> {
        // attach Allow only to method rejection plans
        match self {
            Self::MethodNotAllowed => Some("GET, HEAD"),
            Self::NotFound | Self::BadRequest | Self::UnsupportedQuery => None,
        }
    }
}

/// Typed action selected before any transport or authorization execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicServingPlan<'a> {
    Widget(WidgetServingPlan),
    RecordedForecast(RecordedForecastServingPlan),
    ApiProxy(ApiProxyServingPlan),
    StaticAsset(StaticAssetServingPlan<'a>),
    Delegated(DelegatedServingBoundary),
    Reject(PublicServingRejection),
}

/// Select the freshness-aware public API cache policy used by the edge.
pub fn api_cache_control(pathname: &str) -> &'static str {
    // retain stable site metadata at the edge
    if pathname == "/api/v1/sites" {
        return "public, max-age=300, s-maxage=3600, stale-while-revalidate=86400";
    }

    let Some(resource) = site_resource(pathname) else {
        return "no-store";
    };
    match resource {
        "current" => "public, max-age=15, s-maxage=30, stale-while-revalidate=30",
        "forecast" | "tides" => "public, max-age=60, s-maxage=300, stale-while-revalidate=300",
        "trends" => "public, max-age=300, s-maxage=1800, stale-while-revalidate=86400",
        _ => "no-store",
    }
}

/// Select the bounded upstream request timeout used by the edge.
pub fn api_request_timeout_ms(pathname: &str) -> u64 {
    // allow only the exact trends resource a longer aggregation window
    if site_resource(pathname) == Some("trends") {
        30_000
    } else {
        5_000
    }
}

/// Select one ordinary public serving action from method and decoded path.
pub fn plan_public_request<'a>(
    release: &str,
    method: &str,
    pathname: &'a str,
    query_present: bool,
) -> PublicServingPlan<'a> {
    let method = public_request_method(method);

    // preserve every existing custom or authorization-owned handler boundary
    if let Some(boundary) = early_delegated_boundary(pathname) {
        return PublicServingPlan::Delegated(boundary);
    }

    // isolate supported and rejected widget shapes before the general API proxy
    if is_widget_forecast_path(pathname) {
        let Some((version, upstream)) = supported_widget_forecast(pathname) else {
            return PublicServingPlan::Reject(PublicServingRejection::NotFound);
        };
        // reject every query delimiter before evaluating the read-only method
        if query_present {
            return PublicServingPlan::Reject(PublicServingRejection::BadRequest);
        }
        // retain the widget handler's GET and HEAD boundary
        if method == PublicRequestMethod::Other {
            return PublicServingPlan::Reject(PublicServingRejection::MethodNotAllowed);
        }
        return PublicServingPlan::Widget(WidgetServingPlan {
            version,
            upstream,
            omit_body: method == PublicRequestMethod::Head,
            cache_control: "no-store",
            content_type: "application/json; charset=utf-8",
        });
    }

    // keep recorded forecast evidence queryless and read-only
    if pathname == "/api/v1/sites/ballydidean/forecast-observations"
        || pathname == "/api/v1/sites/ballydidean/solar-cloud-evidence"
    {
        // preserve method rejection before query rejection
        if method == PublicRequestMethod::Other {
            return PublicServingPlan::Reject(PublicServingRejection::MethodNotAllowed);
        }
        // reject caller-controlled archive geometry
        if query_present {
            return PublicServingPlan::Reject(PublicServingRejection::UnsupportedQuery);
        }
        return PublicServingPlan::RecordedForecast(RecordedForecastServingPlan {
            solar_cloud_evidence: pathname.ends_with("/solar-cloud-evidence"),
            omit_body: method == PublicRequestMethod::Head,
            cache_control: "public, max-age=60, s-maxage=60",
            content_type: "application/json; charset=utf-8",
        });
    }

    // forward only the same-origin v1 namespace through the read-only proxy plan
    if pathname.starts_with("/api/v1/") {
        // keep the proxy read-only before selecting forwarding metadata
        if method == PublicRequestMethod::Other {
            return PublicServingPlan::Reject(PublicServingRejection::MethodNotAllowed);
        }
        return PublicServingPlan::ApiProxy(api_proxy_serving_plan(method, pathname));
    }

    // retain map transport and credential decisions in their existing owner
    if pathname.starts_with("/maps/xweather/") {
        return PublicServingPlan::Delegated(DelegatedServingBoundary::XweatherMap);
    }

    // reject non-read methods before static allowlist resolution
    if method == PublicRequestMethod::Other {
        return PublicServingPlan::Reject(PublicServingRejection::MethodNotAllowed);
    }

    // preserve the preview bridge and administrator representation handlers
    if pathname == "/__rac/browser-device" {
        return PublicServingPlan::Delegated(DelegatedServingBoundary::RemoteAgentsPreview);
    }
    // avoid substituting any administrator session decision
    if pathname == "/admin" || pathname == "/admin/" {
        return PublicServingPlan::Delegated(DelegatedServingBoundary::AdminPage);
    }

    let Some(asset) = static_asset_plan(release, pathname, method) else {
        return PublicServingPlan::Reject(PublicServingRejection::NotFound);
    };
    PublicServingPlan::StaticAsset(asset)
}

/// Normalize one method into the three serving-relevant classes.
fn public_request_method(method: &str) -> PublicRequestMethod {
    // retain exact case-sensitive Node method identities
    match method {
        "GET" => PublicRequestMethod::Get,
        "HEAD" => PublicRequestMethod::Head,
        _ => PublicRequestMethod::Other,
    }
}

/// Preserve custom handler ownership before ordinary public classification.
fn early_delegated_boundary(pathname: &str) -> Option<DelegatedServingBoundary> {
    // match exact routes in the same order as the Node request handler
    match pathname {
        "/api/v1/viewer-context" => Some(DelegatedServingBoundary::ViewerContext),
        "/admin/login" => Some(DelegatedServingBoundary::AdminLogin),
        "/admin/logout" => Some(DelegatedServingBoundary::AdminLogout),
        "/api/v1/admin/bootstrap" => Some(DelegatedServingBoundary::AdminBootstrap),
        "/api/v1/sites/ballydidean/property-sensor-layout" => {
            Some(DelegatedServingBoundary::PropertySensorLayout)
        }
        "/api/v1/sites/ballydidean/forecast-adjustment-settings" => {
            Some(DelegatedServingBoundary::PublicAdjustmentSettings)
        }
        "/api/v1/admin/sites/ballydidean/forecast-adjustment-settings" => {
            Some(DelegatedServingBoundary::AdminAdjustmentSettings)
        }
        "/api/v1/admin/sites/ballydidean/forecast-adjustment-scorecard" => {
            Some(DelegatedServingBoundary::AdminAdjustmentScorecard)
        }
        _ => {
            // retain the existing administrator update prefix as one opaque boundary
            if pathname.starts_with("/api/v1/admin/sites/ballydidean/property-sensor-layout/") {
                Some(DelegatedServingBoundary::AdminPropertySensorUpdate)
            } else {
                None
            }
        }
    }
}

/// Recognize every exact or rejected-near widget route shape.
fn is_widget_forecast_path(pathname: &str) -> bool {
    widget_path_for_version(pathname, "v1")
        || widget_path_for_version(pathname, "v2")
        || widget_path_for_version(pathname, "v3")
}

/// Recognize one versioned site widget route including rejected near matches.
fn widget_path_for_version(pathname: &str, version: &str) -> bool {
    let prefix = match version {
        "v1" => "/api/v1/sites/",
        "v2" => "/api/v2/sites/",
        "v3" => "/api/v3/sites/",
        _ => return false,
    };
    let Some(remainder) = pathname.strip_prefix(prefix) else {
        return false;
    };
    let Some((site, resource)) = remainder.split_once('/') else {
        return false;
    };
    !site.is_empty() && (resource == "widget-forecast" || resource.starts_with("widget-forecast/"))
}

/// Select one supported fixed-site widget version and its upstream window.
fn supported_widget_forecast(
    pathname: &str,
) -> Option<(WidgetForecastVersion, WidgetForecastUpstream)> {
    // admit only the three exact public contracts
    match pathname {
        "/api/v1/sites/ballydidean/widget-forecast" => {
            Some((WidgetForecastVersion::V1, WidgetForecastUpstream::Daily))
        }
        "/api/v2/sites/ballydidean/widget-forecast" => {
            Some((WidgetForecastVersion::V2, WidgetForecastUpstream::Daily))
        }
        "/api/v3/sites/ballydidean/widget-forecast" => {
            Some((WidgetForecastVersion::V3, WidgetForecastUpstream::Overnight))
        }
        _ => None,
    }
}

/// Project the read-only API proxy's cache, body, and upstream method plan.
fn api_proxy_serving_plan(method: PublicRequestMethod, pathname: &str) -> ApiProxyServingPlan {
    let is_forecast = pathname == "/api/v1/sites/ballydidean/forecast";
    let is_rain_status = pathname == "/api/v1/sites/ballydidean/rain-collection";
    let projects_adjustment_settings = is_forecast || is_rain_status;
    let trends = site_resource(pathname) == Some("trends");
    ApiProxyServingPlan {
        // retain the two admin-filtered public API representations
        projection: if is_forecast {
            PublicApiProjection::Forecast
        } else if is_rain_status {
            PublicApiProjection::RainCollection
        } else {
            PublicApiProjection::None
        },
        upstream_method: if method == PublicRequestMethod::Head && projects_adjustment_settings {
            ApiUpstreamMethod::Get
        } else if method == PublicRequestMethod::Head {
            ApiUpstreamMethod::Head
        } else {
            ApiUpstreamMethod::Get
        },
        omit_body: method == PublicRequestMethod::Head,
        maximum_response_bytes: if trends { 2 * 1024 * 1024 } else { 1024 * 1024 },
        timeout_ms: api_request_timeout_ms(pathname),
        upstream_success_cache_control: if projects_adjustment_settings {
            "no-store"
        } else {
            api_cache_control(pathname)
        },
        upstream_non_success_cache_control: "no-store",
        upstream_success_content_type: if projects_adjustment_settings {
            ApiContentTypePlan::Json
        } else {
            ApiContentTypePlan::UpstreamOrJson
        },
        upstream_non_success_content_type: ApiContentTypePlan::UpstreamOrJson,
        vary_accept: true,
        track_successful_forecast_response: is_forecast && method == PublicRequestMethod::Get,
    }
}

/// Resolve one exact fixed or active-release static asset.
fn static_asset_plan<'a>(
    release: &str,
    pathname: &'a str,
    method: PublicRequestMethod,
) -> Option<StaticAssetServingPlan<'a>> {
    let omit_body = method == PublicRequestMethod::Head;
    // isolate immutable compiled assets under the exact active release
    if let Some(remainder) = pathname.strip_prefix("/assets/") {
        let (path_release, name) = remainder.split_once('/')?;
        // reject stale releases and nested or unknown bundle names
        if path_release != release || name.contains('/') {
            return None;
        }
        return versioned_asset_plan(name, omit_body);
    }

    // resolve application documents and standalone public metadata
    match pathname {
        "/" | "/index.html" | "/logs" | "/logs/" | "/map" | "/map/" | "/forecast"
        | "/forecast/" | "/trends" | "/trends/" | "/settings" | "/settings/" => Some(public_asset(
            "index.html",
            true,
            true,
            omit_body,
            "no-cache",
            "text/html; charset=utf-8",
        )),
        "/privacy" | "/privacy/" => Some(public_asset(
            "privacy.html",
            true,
            false,
            omit_body,
            "no-cache",
            "text/html; charset=utf-8",
        )),
        "/manifest.webmanifest" => Some(public_asset(
            "manifest.webmanifest",
            false,
            true,
            omit_body,
            "no-cache",
            "application/manifest+json; charset=utf-8",
        )),
        "/service-worker.js" => Some(public_asset(
            "service-worker.js",
            true,
            true,
            omit_body,
            "no-store",
            "text/javascript; charset=utf-8",
        )),
        "/brand/ballydidean-wide.svg" | "/brand/favicon.svg" => Some(public_asset(
            &pathname[1..],
            false,
            true,
            omit_body,
            "public, max-age=86400",
            "image/svg+xml",
        )),
        "/brand/ballydidean-weather-icon-32.png"
        | "/brand/ballydidean-weather-icon-180.png"
        | "/brand/ballydidean-weather-icon-192.png"
        | "/brand/ballydidean-weather-icon-512.png"
        | "/brand/ballydidean-weather-icon-maskable-512.png" => Some(public_asset(
            &pathname[1..],
            false,
            true,
            omit_body,
            "no-cache",
            "image/png",
        )),
        "/weather-icons/01-sunny.svg"
        | "/weather-icons/02-sunny-wind.svg"
        | "/weather-icons/03-partly-cloudy.svg"
        | "/weather-icons/04-partly-cloudy-wind.svg"
        | "/weather-icons/05-cloudy.svg"
        | "/weather-icons/06-cloudy-wind.svg"
        | "/weather-icons/07-light-rain.svg"
        | "/weather-icons/08-light-rain-wind.svg"
        | "/weather-icons/09-heavy-rain.svg"
        | "/weather-icons/10-heavy-rain-wind.svg"
        | "/weather-icons/12-unavailable.svg"
        | "/weather-icons/13-clear-night.svg"
        | "/weather-icons/14-clear-night-wind.svg"
        | "/weather-icons/15-partly-cloudy-night.svg"
        | "/weather-icons/16-partly-cloudy-night-wind.svg" => Some(public_asset(
            &pathname[1..],
            false,
            true,
            omit_body,
            "no-cache",
            "image/svg+xml",
        )),
        "/fonts/google-sans-flex-latin.woff2" | "/fonts/material-symbols-rounded-v4.woff2" => {
            Some(public_asset(
                &pathname[1..],
                false,
                true,
                omit_body,
                "public, max-age=31536000, immutable",
                "font/woff2",
            ))
        }
        "/fonts/LICENSE-google-sans-flex.txt" | "/fonts/LICENSE-material-symbols.txt" => {
            Some(public_asset(
                &pathname[1..],
                false,
                true,
                omit_body,
                "public, max-age=86400",
                "text/plain; charset=utf-8",
            ))
        }
        _ => None,
    }
}

/// The exact insertion order of deployed shell asset digests and their serving names.
const SHELL_VERSIONED_ASSET_NAMES: [&str; 11] = [
    "styles.css",
    "client.js",
    "hosted-updates.js",
    "app-install.js",
    "app-version.js",
    "index.js",
    "solar-cloud.js",
    "solar-cloud-evidence.js",
    "forecast-correction-policy.js",
    "local-forecast-bias.js",
    "units.js",
];

/// Resolve one exact immutable release asset.
fn versioned_asset_plan(name: &str, omit_body: bool) -> Option<StaticAssetServingPlan<'_>> {
    // share the exact startup digest and request-serving inventory
    if !SHELL_VERSIONED_ASSET_NAMES.contains(&name) {
        return None;
    }

    // keep the stylesheet in the public root and scripts in the compiled root
    let (root, content_type) = if name == "styles.css" {
        (StaticAssetRoot::Public, "text/css; charset=utf-8")
    } else {
        (StaticAssetRoot::Compiled, "text/javascript; charset=utf-8")
    };
    Some(StaticAssetServingPlan {
        root,
        relative_path: name,
        template: false,
        analytics_allowed: true,
        omit_body,
        cache_control: "public, max-age=31536000, immutable",
        content_type,
    })
}

/// Construct one public-root asset plan after an exact allowlist match.
fn public_asset<'a>(
    relative_path: &'a str,
    template: bool,
    analytics_allowed: bool,
    omit_body: bool,
    cache_control: &'static str,
    content_type: &'static str,
) -> StaticAssetServingPlan<'a> {
    StaticAssetServingPlan {
        root: StaticAssetRoot::Public,
        relative_path,
        template,
        analytics_allowed,
        omit_body,
        cache_control,
        content_type,
    }
}

/// Project the first forecast-map preload link from a request path and wall clock.
pub fn forecast_map_preload_link<T: CanonicalTimeAdapter>(
    pathname: &str,
    site: Option<ForecastMapSite>,
    now_epoch_ms: i64,
    time: &T,
) -> String {
    // keep weather-map traffic off unrelated routes and absent site configurations
    if pathname != "/forecast" && pathname != "/forecast/" {
        return String::new();
    }
    let Some(site) = site else {
        return String::new();
    };

    let frame_ms = now_epoch_ms
        .div_euclid(TEN_MINUTES_MS)
        .saturating_mul(TEN_MINUTES_MS);
    let valid_time = time.xweather_valid_time(frame_ms);
    let latitude = format_forecast_map_coordinate(site.latitude);
    let longitude = format_forecast_map_coordinate(site.longitude);
    let href =
        format!("/maps/xweather/history/radar/{valid_time}/10/256x168/{latitude},{longitude}.png");
    format!(
        "<link rel=\"preload\" as=\"image\" type=\"image/png\" fetchpriority=\"high\" href=\"{href}\">"
    )
}

/// Format a bounded map coordinate with the producer's six-digit midpoint rule.
fn format_forecast_map_coordinate(value: f64) -> String {
    // javascript normalizes an exact negative zero before fixed decimal formatting
    if value == 0.0 {
        return "0.000000".to_owned();
    }
    let scale = 1_000_000.0;
    let magnitude = value.abs();
    let scaled = magnitude * scale;
    // distinguish exact midpoints from nearby numbers rounded onto them by multiplication
    if scaled.fract() == 0.5 && magnitude.mul_add(scale, -scaled) == 0.0 {
        let rounded = scaled.ceil() as u32;
        let sign = if value < 0.0 { "-" } else { "" };
        return format!("{sign}{}.{:06}", rounded / 1_000_000, rounded % 1_000_000);
    }
    format!("{value:.6}")
}

/// Adapt one generated API record losslessly into the facts consumed by widget projection.
fn widget_forecast_record_input(record: &ApiForecastWeatherRecord) -> WidgetForecastRecordInput {
    let rain_source = match &record.rainAdjustment.sourceForecast {
        Nullable::Null => None,
        Nullable::Value(source) => Some(WidgetForecastIndependentAdjustmentSource {
            run_initialized_at: source.runInitializedAt.clone(),
            first_received_at: source.firstReceivedAt.clone(),
        }),
    };
    let rain_decision_at = match &record.rainAdjustment.sourceForecast {
        Nullable::Null => None,
        Nullable::Value(source) => Some(source.decisionAt.clone()),
    };
    let temperature_source = match &record.temperatureAdjustment.sourceForecast {
        Nullable::Null => None,
        Nullable::Value(source) => Some(WidgetForecastIndependentAdjustmentSource {
            run_initialized_at: source.runInitializedAt.clone(),
            first_received_at: source.firstReceivedAt.clone(),
        }),
    };
    let (temperature_applied, adjusted_temperature_c, wind_speed_applied, adjusted_wind_speed_mps) =
        generic_adjustment_input(&record.adjustment);
    WidgetForecastRecordInput {
        metrics: WidgetForecastRecordMetrics {
            cloud_cover_percent: nullable_f64(&record.metrics.cloudCoverPercent),
            precipitation_mm: nullable_f64(&record.metrics.precipitationMm),
            precipitation_rate_mm_per_hour: nullable_f64(
                &record.metrics.precipitationRateMmPerHour,
            ),
            temperature_c: nullable_f64(&record.metrics.temperatureC),
            wind_speed_mps: nullable_f64(&record.metrics.windSpeedMps),
        },
        product_run_at: match &record.productRunAt {
            Nullable::Null => None,
            Nullable::Value(value) => Some(value.clone()),
        },
        received_at: record.receivedAt.clone(),
        valid_at: record.validAt.clone(),
        is_forecast: record.provenance.sourceKind == SourceKind::Forecast,
        rain_adjustment: WidgetForecastRainAdjustment {
            active: record.rainAdjustment.state == ApiRainAdjustmentDecisionStateEnum::Active,
            corrected_value: nullable_f64(&record.rainAdjustment.correctedPrecipitationMm),
            decision_at: rain_decision_at,
            source: rain_source,
        },
        generic_adjustment: WidgetForecastGenericAdjustment {
            temperature_applied,
            adjusted_temperature_c,
            wind_speed_applied,
            adjusted_wind_speed_mps,
        },
        temperature_adjustment: WidgetForecastIndependentAdjustment {
            active: record.temperatureAdjustment.state
                == ForecastTemperatureCanaryDecisionV1StateEnum::Active,
            corrected_value: nullable_f64(&record.temperatureAdjustment.correctedTemperatureC),
            source: temperature_source,
        },
    }
}

/// Flatten every generated active generic decision family without changing selected values.
fn generic_adjustment_input(
    adjustment: &ApiForecastAdjustmentDecision,
) -> (bool, Option<f64>, bool, Option<f64>) {
    match adjustment {
        ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::Active(decision)) => (
            decision
                .appliedMetrics
                .contains(&ForecastAdjustmentMetric::TemperatureC),
            decision
                .adjustedMetrics
                .0
                .iter()
                .find_map(|(metric, value)| {
                    (*metric == ForecastAdjustmentMetric::TemperatureC).then_some(*value)
                }),
            decision
                .appliedMetrics
                .contains(&ForecastAdjustmentMetric::WindSpeedMps),
            decision
                .adjustedMetrics
                .0
                .iter()
                .find_map(|(metric, value)| {
                    (*metric == ForecastAdjustmentMetric::WindSpeedMps).then_some(*value)
                }),
        ),
        ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::WindCanaryActive(
            decision,
        )) => (
            false,
            None,
            decision
                .appliedMetrics
                .contains(&ForecastAdjustmentWindCanaryMetric::WindSpeedMps),
            decision
                .adjustedMetrics
                .0
                .iter()
                .find_map(|(metric, value)| {
                    (*metric == ForecastAdjustmentWindCanaryMetric::WindSpeedMps).then_some(*value)
                }),
        ),
        ApiForecastAdjustmentDecision::MaintenanceQualified(decision) => (
            false,
            None,
            decision
                .appliedMetrics
                .contains(&ForecastAdjustmentWindCanaryMetric::WindSpeedMps),
            decision
                .adjustedMetrics
                .0
                .iter()
                .find_map(|(metric, value)| {
                    (*metric == ForecastAdjustmentMetric::WindSpeedMps).then_some(*value)
                }),
        ),
        ApiForecastAdjustmentDecision::Domain(ForecastAdjustmentDecision::FailRaw(_)) => {
            (false, None, false, None)
        }
    }
}

/// Project one canonical API forecast row into the complete widget v1 hour contract.
pub fn project_widget_forecast_v1_hour<T: CanonicalTimeAdapter>(
    record: Option<&ApiForecastWeatherRecord>,
    start: String,
    epochs: WidgetForecastV1Epochs,
    generic_deadline: WidgetForecastAdjustmentDeadline,
    temperature_deadline: WidgetForecastAdjustmentDeadline,
    time: &T,
) -> Result<WidgetForecastHour, WidgetForecastProjectionError> {
    let record = record.map(widget_forecast_record_input);
    project_widget_forecast_v1_input_hour(
        record.as_ref(),
        start,
        epochs,
        generic_deadline,
        temperature_deadline,
        time,
    )
}

/// Project one filtered minimal forecast row into the complete widget v1 hour contract.
pub fn project_widget_forecast_v1_input_hour<T: CanonicalTimeAdapter>(
    record: Option<&WidgetForecastRecordInput>,
    start: String,
    epochs: WidgetForecastV1Epochs,
    generic_deadline: WidgetForecastAdjustmentDeadline,
    temperature_deadline: WidgetForecastAdjustmentDeadline,
    time: &T,
) -> Result<WidgetForecastHour, WidgetForecastProjectionError> {
    let Some(end_epoch_ms) = epochs.start_epoch_ms.checked_add(HOUR_MS) else {
        return Err(WidgetForecastProjectionError::TimestampOutOfRange);
    };
    let end = time.utc_iso(end_epoch_ms);

    // preserve a missing grid row without inventing either field
    let Some(record) = record else {
        return Ok(WidgetForecastHour {
            end,
            rainMmPerHour: unavailable_widget_field(),
            start,
            temperatureC: unavailable_widget_field(),
        });
    };

    let rain_mm_per_hour = project_widget_forecast_v1_field(
        record,
        WidgetForecastV1FieldKind::Rain,
        epochs,
        generic_deadline,
        temperature_deadline,
        time,
    )?;
    let temperature_c = project_widget_forecast_v1_field(
        record,
        WidgetForecastV1FieldKind::Temperature,
        epochs,
        generic_deadline,
        temperature_deadline,
        time,
    )?;
    Ok(WidgetForecastHour {
        end,
        rainMmPerHour: rain_mm_per_hour,
        start,
        temperatureC: temperature_c,
    })
}

/// Build one complete canonical widget v3 response from admitted clocks and API rows.
pub fn build_widget_forecast_v3<T: CanonicalTimeAdapter>(
    boundary: WidgetForecastV3Boundary,
    records: &[WidgetForecastV3Record<'_>],
    time: &T,
) -> Result<WidgetForecastV3Snapshot, WidgetForecastProjectionError> {
    let inputs: Vec<_> = records
        .iter()
        .map(|projected| widget_forecast_record_input(projected.record))
        .collect();
    let projected: Vec<_> = records
        .iter()
        .zip(inputs.iter())
        .map(|(projected, record)| WidgetForecastV3InputRecord {
            record,
            epochs: projected.epochs,
            generic_deadline: projected.generic_deadline,
            temperature_deadline: projected.temperature_deadline,
        })
        .collect();
    build_widget_forecast_v3_input(boundary, &projected, time)
}

/// Build one complete canonical widget v3 response from filtered minimal rows.
pub fn build_widget_forecast_v3_input<T: CanonicalTimeAdapter>(
    boundary: WidgetForecastV3Boundary,
    records: &[WidgetForecastV3InputRecord<'_>],
    time: &T,
) -> Result<WidgetForecastV3Snapshot, WidgetForecastProjectionError> {
    validate_widget_v3_boundary(&boundary, time)?;
    let duration_ms = boundary
        .calendar
        .overnight_end
        .epoch_ms
        .checked_sub(boundary.calendar.day_start.epoch_ms)
        .ok_or(WidgetForecastProjectionError::InvalidV3Calendar)?;

    // require an exact real-hour grid within the reviewed DST capacities
    if duration_ms % HOUR_MS != 0 {
        return Err(WidgetForecastProjectionError::InvalidV3HourCount);
    }
    let hour_count = duration_ms / HOUR_MS;
    // keep the response inside the source v3 schema bounds
    if !(30..=32).contains(&hour_count) {
        return Err(WidgetForecastProjectionError::InvalidV3HourCount);
    }
    let hour_count = usize::try_from(hour_count)
        .map_err(|_| WidgetForecastProjectionError::InvalidV3HourCount)?;
    let mut row_by_hour = vec![None; hour_count];

    // index each supplied row once while allowing genuine missing hours
    for (record_index, projected) in records.iter().enumerate() {
        // reject nonforecast rows before admitting their hour identity
        if !projected.record.is_forecast {
            return Err(WidgetForecastProjectionError::NonForecastHour);
        }
        // bind every row to the response generation anchor
        if projected.epochs.raw_source.generated_at_epoch_ms != boundary.generated_at.epoch_ms {
            return Err(WidgetForecastProjectionError::SourceTimestampMismatch);
        }
        let offset = projected
            .epochs
            .start_epoch_ms
            .checked_sub(boundary.calendar.day_start.epoch_ms)
            .ok_or(WidgetForecastProjectionError::DuplicateOrForeignHour)?;
        // reject off-grid and negative row offsets
        if offset < 0 || offset % HOUR_MS != 0 {
            return Err(WidgetForecastProjectionError::DuplicateOrForeignHour);
        }
        let hour_index = usize::try_from(offset / HOUR_MS)
            .map_err(|_| WidgetForecastProjectionError::DuplicateOrForeignHour)?;
        let Some(slot) = row_by_hour.get_mut(hour_index) else {
            return Err(WidgetForecastProjectionError::DuplicateOrForeignHour);
        };
        // reject duplicate rows for the same canonical interval
        if slot.is_some() {
            return Err(WidgetForecastProjectionError::DuplicateOrForeignHour);
        }
        let canonical_start = time.utc_iso(projected.epochs.start_epoch_ms);
        // pair the source validAt string with the admitted start epoch
        if projected.record.valid_at != canonical_start {
            return Err(WidgetForecastProjectionError::HourTimestampMismatch);
        }
        *slot = Some(record_index);
    }

    let mut hours = Vec::with_capacity(hour_count);
    // project every expected interval in chronological order
    for (hour_index, record_index) in row_by_hour.into_iter().enumerate() {
        let offset = i64::try_from(hour_index)
            .ok()
            .and_then(|index| index.checked_mul(HOUR_MS))
            .ok_or(WidgetForecastProjectionError::TimestampOutOfRange)?;
        let start_epoch_ms = boundary
            .calendar
            .day_start
            .epoch_ms
            .checked_add(offset)
            .ok_or(WidgetForecastProjectionError::TimestampOutOfRange)?;
        let start = time.utc_iso(start_epoch_ms);
        let projected = record_index.map(|index| records[index]);
        let epochs = projected.map_or(
            WidgetForecastV1Epochs {
                start_epoch_ms,
                raw_source: WidgetForecastSourceEpochs {
                    generated_at_epoch_ms: boundary.generated_at.epoch_ms,
                    received_at_epoch_ms: boundary.generated_at.epoch_ms,
                    product_run_at_epoch_ms: None,
                },
                rain_source: None,
                temperature_source: None,
                rain_decision_at_epoch_ms: None,
            },
            |value| value.epochs,
        );
        let record = projected.map(|value| value.record);
        let generic_deadline = projected
            .map_or(WidgetForecastAdjustmentDeadline::Unavailable, |value| {
                value.generic_deadline
            });
        let temperature_deadline = projected
            .map_or(WidgetForecastAdjustmentDeadline::Unavailable, |value| {
                value.temperature_deadline
            });
        let base = project_widget_forecast_v1_input_hour(
            record,
            start,
            epochs,
            generic_deadline,
            temperature_deadline,
            time,
        )?;
        hours.push(project_widget_forecast_v2_input_hour(
            base,
            record,
            epochs.raw_source,
            generic_deadline,
            time,
        )?);
    }

    let status = summarize_widget_forecast_v2_status(&hours);
    Ok(WidgetForecastV3Snapshot {
        attribution: WidgetForecastSnapshotAttribution {
            label: WidgetForecastSnapshotAttributionLabelLiteral,
            licenseUrl: WidgetForecastSnapshotAttributionLicenseUrlLiteral,
            providerUrl: WidgetForecastSnapshotAttributionProviderUrlLiteral,
        },
        generatedAt: boundary.generated_at.value,
        receivedAt: boundary.received_at.value,
        site: WidgetForecastSnapshotSite {
            latitude: WidgetForecastSnapshotSiteLatitudeLiteral,
            longitude: WidgetForecastSnapshotSiteLongitudeLiteral,
            name: WidgetForecastSnapshotSiteNameLiteral,
            slug: WidgetForecastSnapshotSiteSlugLiteral,
            timezone: WidgetForecastSnapshotSiteTimezoneLiteral,
        },
        status,
        hours,
        calendar: WidgetForecastV3SnapshotCalendar {
            cutoff: boundary.calendar.cutoff.value,
            date: boundary.calendar.date,
            dayEnd: boundary.calendar.day_end.value,
            dayStart: boundary.calendar.day_start.value,
            sunset: boundary
                .calendar
                .sunset
                .map_or(Nullable::Null, |sunset| Nullable::Value(sunset.value)),
            overnightEnd: boundary.calendar.overnight_end.value,
        },
        schemaVersion: WidgetForecastV3SnapshotSchemaVersionLiteral,
    })
}

/// Extend one canonical widget hour with cloud and wind application fields.
pub fn project_widget_forecast_v2_hour<T: CanonicalTimeAdapter>(
    hour: WidgetForecastHour,
    record: Option<&ApiForecastWeatherRecord>,
    source_epochs: WidgetForecastSourceEpochs,
    deadline: WidgetForecastAdjustmentDeadline,
    time: &T,
) -> Result<WidgetForecastV2Hour, WidgetForecastProjectionError> {
    let record = record.map(widget_forecast_record_input);
    project_widget_forecast_v2_input_hour(hour, record.as_ref(), source_epochs, deadline, time)
}

/// Extend one canonical widget hour from a filtered minimal record.
pub fn project_widget_forecast_v2_input_hour<T: CanonicalTimeAdapter>(
    hour: WidgetForecastHour,
    record: Option<&WidgetForecastRecordInput>,
    source_epochs: WidgetForecastSourceEpochs,
    deadline: WidgetForecastAdjustmentDeadline,
    time: &T,
) -> Result<WidgetForecastV2Hour, WidgetForecastProjectionError> {
    let Some(record) = record else {
        return Ok(WidgetForecastV2Hour {
            end: hour.end,
            rainMmPerHour: hour.rainMmPerHour,
            start: hour.start,
            temperatureC: hour.temperatureC,
            cloudCoverPercent: unavailable_widget_field(),
            windSpeedMps: unavailable_widget_field(),
        });
    };

    let cloud_cover_percent = project_supplemental_field(
        record,
        SupplementalMetric::CloudCoverPercent,
        source_epochs,
        deadline,
        time,
    )?;
    let wind_speed_mps = project_supplemental_field(
        record,
        SupplementalMetric::WindSpeedMps,
        source_epochs,
        deadline,
        time,
    )?;
    Ok(WidgetForecastV2Hour {
        end: hour.end,
        rainMmPerHour: hour.rainMmPerHour,
        start: hour.start,
        temperatureC: hour.temperatureC,
        cloudCoverPercent: cloud_cover_percent,
        windSpeedMps: wind_speed_mps,
    })
}

/// Summarize all four widget v2 field families without allocating.
pub fn summarize_widget_forecast_v2_status(hours: &[WidgetForecastV2Hour]) -> WidgetForecastStatus {
    let mut modes = hours.iter().flat_map(|hour| {
        [
            hour.temperatureC.mode,
            hour.rainMmPerHour.mode,
            hour.cloudCoverPercent.mode,
            hour.windSpeedMps.mode,
        ]
    });
    let Some(first) = modes.next() else {
        return WidgetForecastStatus::Mixed;
    };

    // preserve one mode only when every projected field agrees
    if modes.all(|mode| mode == first) {
        return widget_status_for_mode(first);
    }
    WidgetForecastStatus::Mixed
}

/// Validate one admitted v3 calendar without implementing date serialization.
fn validate_widget_v3_boundary<T: CanonicalTimeAdapter>(
    boundary: &WidgetForecastV3Boundary,
    time: &T,
) -> Result<(), WidgetForecastProjectionError> {
    let instants = [
        &boundary.generated_at,
        &boundary.received_at,
        &boundary.calendar.day_start,
        &boundary.calendar.day_end,
        &boundary.calendar.cutoff,
        &boundary.calendar.overnight_end,
    ];
    // require every supplied canonical string to match its typed epoch
    for instant in instants {
        if time.utc_iso(instant.epoch_ms) != instant.value {
            return Err(WidgetForecastProjectionError::CanonicalTimestampMismatch);
        }
    }
    // retain exact optional sunset nullability and clock pairing
    if let Some(sunset) = &boundary.calendar.sunset
        && time.utc_iso(sunset.epoch_ms) != sunset.value
    {
        return Err(WidgetForecastProjectionError::CanonicalTimestampMismatch);
    }
    // reject an upstream generation anchor later than edge receipt
    if boundary.generated_at.epoch_ms > boundary.received_at.epoch_ms {
        return Err(WidgetForecastProjectionError::GeneratedAfterReceived);
    }
    // require one ordered local-day through next-morning calendar
    if !(boundary.calendar.day_start.epoch_ms < boundary.calendar.anchor_start_epoch_ms
        && boundary.calendar.anchor_start_epoch_ms <= boundary.calendar.cutoff.epoch_ms
        && boundary.calendar.cutoff.epoch_ms < boundary.calendar.day_end.epoch_ms
        && boundary.calendar.day_end.epoch_ms < boundary.calendar.overnight_end.epoch_ms)
    {
        return Err(WidgetForecastProjectionError::InvalidV3Calendar);
    }
    // require the real forecast clock inside exactly one active overnight window
    if boundary.generated_at.epoch_ms < boundary.calendar.anchor_start_epoch_ms
        || boundary.generated_at.epoch_ms >= boundary.calendar.overnight_end.epoch_ms
    {
        return Err(WidgetForecastProjectionError::GeneratedOutsideV3Window);
    }
    Ok(())
}

/// Project one rain or temperature field through the complete widget v1 rules.
fn project_widget_forecast_v1_field<T: CanonicalTimeAdapter>(
    record: &WidgetForecastRecordInput,
    kind: WidgetForecastV1FieldKind,
    epochs: WidgetForecastV1Epochs,
    generic_deadline: WidgetForecastAdjustmentDeadline,
    temperature_deadline: WidgetForecastAdjustmentDeadline,
    time: &T,
) -> Result<WidgetForecastField, WidgetForecastProjectionError> {
    // reject either supplied rain representation before applying its fallback order
    if kind == WidgetForecastV1FieldKind::Rain {
        for value in [
            record.metrics.precipitation_rate_mm_per_hour,
            record.metrics.precipitation_mm,
        ] {
            // reject every present physical violation
            if let Some(value) = value
                && !valid_widget_v1_value(kind, value)
            {
                return Err(WidgetForecastProjectionError::InvalidRain);
            }
        }
    }

    let raw = widget_v1_field_value(record, kind, false);
    let raw_source = widget_source(record, epochs.raw_source)?;

    // reject a present field-specific physical violation
    if raw.is_some_and(|value| !valid_widget_v1_value(kind, value)) {
        return Err(match kind {
            WidgetForecastV1FieldKind::Rain => WidgetForecastProjectionError::InvalidRain,
            WidgetForecastV1FieldKind::Temperature => {
                WidgetForecastProjectionError::InvalidTemperature
            }
        });
    }

    let selected = widget_v1_field_value(record, kind, true);
    let adjustment = widget_v1_adjustment_selection(
        record,
        kind,
        epochs,
        generic_deadline,
        temperature_deadline,
    )?;

    // retain a truthful raw selection when no active correction applies
    let Some(adjustment) = adjustment else {
        return Ok(match raw {
            Some(raw) => raw_widget_field(raw, raw_source, WidgetForecastReason::RawForecast),
            None => unavailable_widget_field(),
        });
    };

    // demote unbounded corrections to their captured raw counterpart
    let Some(selected_until_epoch_ms) = adjustment.deadline_epoch_ms else {
        return Ok(match raw {
            Some(raw) => {
                raw_widget_field(raw, raw_source, WidgetForecastReason::DeadlineUnavailable)
            }
            None => unavailable_widget_field(),
        });
    };

    // demote corrections expired at snapshot generation
    if selected_until_epoch_ms <= epochs.raw_source.generated_at_epoch_ms {
        return Ok(match raw {
            Some(raw) => raw_widget_field(raw, raw_source, WidgetForecastReason::DeadlineExpired),
            None => unavailable_widget_field(),
        });
    }

    // reject an absent or invalid correction at the public boundary
    let Some(selected) = selected.filter(|value| valid_widget_v1_value(kind, *value)) else {
        return Err(match kind {
            WidgetForecastV1FieldKind::Rain => WidgetForecastProjectionError::InvalidSelectedRain,
            WidgetForecastV1FieldKind::Temperature => {
                WidgetForecastProjectionError::InvalidSelectedTemperature
            }
        });
    };

    Ok(WidgetForecastField {
        mode: WidgetForecastMode::Adjusted,
        raw: raw.map_or(Nullable::Null, Nullable::Value),
        rawSource: raw.map_or(Nullable::Null, |_| Nullable::Value(raw_source)),
        reason: adjustment.reason,
        selected: Nullable::Value(selected),
        selectedSource: Nullable::Value(adjustment.source),
        selectedUntil: Nullable::Value(time.utc_iso(selected_until_epoch_ms)),
    })
}

/// Resolve one v1 raw or selected field value with rain-rate precedence.
fn widget_v1_field_value(
    record: &WidgetForecastRecordInput,
    kind: WidgetForecastV1FieldKind,
    use_adjustments: bool,
) -> Option<f64> {
    // select only the independent temperature branch before generic adjustment
    if kind == WidgetForecastV1FieldKind::Temperature {
        if use_adjustments && record.temperature_adjustment.active {
            return record.temperature_adjustment.corrected_value;
        }
        // retain the explicitly applied generic temperature correction
        if use_adjustments && let Some(selected) = adjusted_temperature(record) {
            return Some(selected);
        }
        return record.metrics.temperature_c;
    }

    // select the independently corrected hourly amount for both rain representations
    if use_adjustments && record.rain_adjustment.active {
        return record.rain_adjustment.corrected_value;
    }
    record
        .metrics
        .precipitation_rate_mm_per_hour
        .or(record.metrics.precipitation_mm)
}

/// Select one active v1 correction branch and its independent deadline.
fn widget_v1_adjustment_selection(
    record: &WidgetForecastRecordInput,
    kind: WidgetForecastV1FieldKind,
    epochs: WidgetForecastV1Epochs,
    generic_deadline: WidgetForecastAdjustmentDeadline,
    temperature_deadline: WidgetForecastAdjustmentDeadline,
) -> Result<Option<WidgetForecastV1Selection>, WidgetForecastProjectionError> {
    let Some(maximum) = epochs
        .raw_source
        .generated_at_epoch_ms
        .checked_add(ADJUSTMENT_MAX_AGE_MS)
    else {
        return Err(WidgetForecastProjectionError::TimestampOutOfRange);
    };

    // prefer the independently selected temperature correction
    if kind == WidgetForecastV1FieldKind::Temperature && record.temperature_adjustment.active {
        let deadline_epoch_ms = bounded_widget_deadline(maximum, temperature_deadline);
        let Some(source) = &record.temperature_adjustment.source else {
            return Err(WidgetForecastProjectionError::SelectedSourceIncomplete);
        };
        let selected_source = independent_widget_source(
            &source.run_initialized_at,
            &source.first_received_at,
            epochs.temperature_source,
            epochs.raw_source.generated_at_epoch_ms,
        )?;
        return Ok(Some(WidgetForecastV1Selection {
            deadline_epoch_ms,
            reason: WidgetForecastReason::IndependentAdjustment,
            source: selected_source,
        }));
    }

    // prefer the independently selected rain correction
    if kind == WidgetForecastV1FieldKind::Rain && record.rain_adjustment.active {
        let deadline_epoch_ms = rain_widget_deadline(
            maximum,
            epochs.rain_decision_at_epoch_ms,
            epochs.raw_source.generated_at_epoch_ms,
        )?;
        let Some(source) = &record.rain_adjustment.source else {
            return Err(WidgetForecastProjectionError::SelectedSourceIncomplete);
        };
        let selected_source = independent_widget_source(
            &source.run_initialized_at,
            &source.first_received_at,
            epochs.rain_source,
            epochs.raw_source.generated_at_epoch_ms,
        )?;
        return Ok(Some(WidgetForecastV1Selection {
            deadline_epoch_ms,
            reason: WidgetForecastReason::RainAdjustment,
            source: selected_source,
        }));
    }

    // retain only the generic correction branch the shared resolver can select
    if kind == WidgetForecastV1FieldKind::Temperature && generic_temperature_applied(record) {
        return Ok(Some(WidgetForecastV1Selection {
            deadline_epoch_ms: bounded_widget_deadline(maximum, generic_deadline),
            reason: WidgetForecastReason::GenericAdjustment,
            source: widget_source(record, epochs.raw_source)?,
        }));
    }

    Ok(None)
}

/// Cap one generic or temperature correction deadline at snapshot freshness.
fn bounded_widget_deadline(
    maximum: i64,
    deadline: WidgetForecastAdjustmentDeadline,
) -> Option<i64> {
    // preserve the runtime's absent/null/finite distinction
    match deadline {
        WidgetForecastAdjustmentDeadline::Unavailable => None,
        WidgetForecastAdjustmentDeadline::Permanent => Some(maximum),
        WidgetForecastAdjustmentDeadline::AtEpochMilliseconds(expiry) => Some(maximum.min(expiry)),
    }
}

/// Derive the rain decision deadline without serializing its private clock.
fn rain_widget_deadline(
    maximum: i64,
    decision_at_epoch_ms: Option<i64>,
    generated_at_epoch_ms: i64,
) -> Result<Option<i64>, WidgetForecastProjectionError> {
    let Some(decision_at_epoch_ms) = decision_at_epoch_ms else {
        return Ok(None);
    };

    // reject a decision that had not happened at snapshot generation
    if decision_at_epoch_ms > generated_at_epoch_ms {
        return Err(WidgetForecastProjectionError::RainDecisionFromFuture);
    }
    let Some(decision_deadline) = decision_at_epoch_ms.checked_add(RAIN_DECISION_MAX_AGE_MS) else {
        return Err(WidgetForecastProjectionError::TimestampOutOfRange);
    };
    Ok(Some(maximum.min(decision_deadline)))
}

/// Bind one independent source's canonical strings to its prevalidated clocks.
fn independent_widget_source(
    run_at: &str,
    received_at: &str,
    epochs: Option<WidgetForecastIndependentSourceEpochs>,
    generated_at_epoch_ms: i64,
) -> Result<WidgetForecastSource, WidgetForecastProjectionError> {
    let Some(epochs) = epochs else {
        return Err(WidgetForecastProjectionError::SourceTimestampMismatch);
    };
    let Some(run_at_epoch_ms) = epochs.product_run_at_epoch_ms else {
        return Err(WidgetForecastProjectionError::SourceTimestampMismatch);
    };

    // reject independent source clocks later than their containing clocks
    if epochs.received_at_epoch_ms > generated_at_epoch_ms
        || run_at_epoch_ms > epochs.received_at_epoch_ms
    {
        return Err(WidgetForecastProjectionError::SourceTimestampsOutOfOrder);
    }
    Ok(WidgetForecastSource {
        runAt: Nullable::Value(run_at.into()),
        receivedAt: received_at.into(),
    })
}

/// Read an explicitly applied generic temperature correction.
fn adjusted_temperature(record: &WidgetForecastRecordInput) -> Option<f64> {
    // require the explicit application marker before selecting the map value
    if !record.generic_adjustment.temperature_applied {
        return None;
    }
    record
        .generic_adjustment
        .adjusted_temperature_c
        .or(record.metrics.temperature_c)
}

/// Report whether the generic resolver selected temperature at all.
fn generic_temperature_applied(record: &WidgetForecastRecordInput) -> bool {
    record.generic_adjustment.temperature_applied
}

/// Convert one generated nullable number without a sentinel.
fn nullable_f64(value: &Nullable<f64>) -> Option<f64> {
    // retain exact null semantics
    match value {
        Nullable::Value(value) => Some(*value),
        Nullable::Null => None,
    }
}

/// Check the closed physical range for one v1 field.
fn valid_widget_v1_value(kind: WidgetForecastV1FieldKind, value: f64) -> bool {
    // preserve each source contract's independent physical ceiling
    match kind {
        WidgetForecastV1FieldKind::Rain => value.is_finite() && (0.0..=2_000.0).contains(&value),
        WidgetForecastV1FieldKind::Temperature => {
            value.is_finite() && (-100.0..=70.0).contains(&value)
        }
    }
}

/// Project one supplemental condition field from canonical API input.
fn project_supplemental_field<T: CanonicalTimeAdapter>(
    record: &WidgetForecastRecordInput,
    metric: SupplementalMetric,
    source_epochs: WidgetForecastSourceEpochs,
    deadline: WidgetForecastAdjustmentDeadline,
    time: &T,
) -> Result<WidgetForecastField, WidgetForecastProjectionError> {
    let raw = match metric {
        SupplementalMetric::CloudCoverPercent => record.metrics.cloud_cover_percent,
        SupplementalMetric::WindSpeedMps => record.metrics.wind_speed_mps,
    };
    let Some(raw) = raw else {
        return Ok(unavailable_widget_field());
    };

    // reject non-finite and physical range violations at the public boundary
    if !valid_supplemental_metric(metric, raw) {
        return Err(match metric {
            SupplementalMetric::CloudCoverPercent => {
                WidgetForecastProjectionError::InvalidCloudCoverPercent
            }
            SupplementalMetric::WindSpeedMps => WidgetForecastProjectionError::InvalidWindSpeedMps,
        });
    }

    let source = widget_source(record, source_epochs)?;
    let selected_wind = match metric {
        SupplementalMetric::CloudCoverPercent => None,
        SupplementalMetric::WindSpeedMps => adjusted_wind_speed(record, raw),
    };
    let Some(selected) = selected_wind else {
        return Ok(raw_widget_field(
            raw,
            source,
            WidgetForecastReason::RawForecast,
        ));
    };

    let Some(maximum) = source_epochs
        .generated_at_epoch_ms
        .checked_add(ADJUSTMENT_MAX_AGE_MS)
    else {
        return Err(WidgetForecastProjectionError::TimestampOutOfRange);
    };
    let selected_until_epoch_ms = match deadline {
        WidgetForecastAdjustmentDeadline::Unavailable => {
            return Ok(raw_widget_field(
                raw,
                source,
                WidgetForecastReason::DeadlineUnavailable,
            ));
        }
        WidgetForecastAdjustmentDeadline::Permanent => maximum,
        WidgetForecastAdjustmentDeadline::AtEpochMilliseconds(expiry) => maximum.min(expiry),
    };

    // demote a correction that has expired at snapshot generation
    if selected_until_epoch_ms <= source_epochs.generated_at_epoch_ms {
        return Ok(raw_widget_field(
            raw,
            source,
            WidgetForecastReason::DeadlineExpired,
        ));
    }
    // reject invalid selected wind instead of synthesizing a fallback
    if !valid_supplemental_metric(SupplementalMetric::WindSpeedMps, selected) {
        return Err(WidgetForecastProjectionError::InvalidSelectedWindSpeedMps);
    }

    Ok(WidgetForecastField {
        mode: WidgetForecastMode::Adjusted,
        raw: Nullable::Value(raw),
        rawSource: Nullable::Value(source.clone()),
        reason: WidgetForecastReason::GenericAdjustment,
        selected: Nullable::Value(selected),
        selectedSource: Nullable::Value(source),
        selectedUntil: Nullable::Value(time.utc_iso(selected_until_epoch_ms)),
    })
}

/// Read an active adjusted wind value, falling back to raw when its map omits the key.
fn adjusted_wind_speed(record: &WidgetForecastRecordInput, raw: f64) -> Option<f64> {
    // require the explicit application marker before selecting an adjusted value
    if !record.generic_adjustment.wind_speed_applied {
        return None;
    }
    Some(
        record
            .generic_adjustment
            .adjusted_wind_speed_mps
            .unwrap_or(raw),
    )
}

/// Reuse canonical source strings after checking their paired epoch ordering.
fn widget_source(
    record: &WidgetForecastRecordInput,
    source_epochs: WidgetForecastSourceEpochs,
) -> Result<WidgetForecastSource, WidgetForecastProjectionError> {
    // require the prevalidated run epoch to match source nullability
    if record.product_run_at.is_none() != source_epochs.product_run_at_epoch_ms.is_none() {
        return Err(WidgetForecastProjectionError::SourceTimestampMismatch);
    }
    // reject source timestamps later than their containing clocks
    if source_epochs.received_at_epoch_ms > source_epochs.generated_at_epoch_ms
        || source_epochs
            .product_run_at_epoch_ms
            .is_some_and(|run_at| run_at > source_epochs.received_at_epoch_ms)
    {
        return Err(WidgetForecastProjectionError::SourceTimestampsOutOfOrder);
    }
    Ok(WidgetForecastSource {
        runAt: record
            .product_run_at
            .clone()
            .map_or(Nullable::Null, Nullable::Value),
        receivedAt: record.received_at.clone(),
    })
}

/// Check the closed physical range for one supplemental field.
fn valid_supplemental_metric(metric: SupplementalMetric, value: f64) -> bool {
    let maximum = match metric {
        SupplementalMetric::CloudCoverPercent => 100.0,
        SupplementalMetric::WindSpeedMps => 150.0,
    };
    value.is_finite() && (0.0..=maximum).contains(&value)
}

/// Create one raw field with matching captured provenance.
fn raw_widget_field(
    raw: f64,
    source: WidgetForecastSource,
    reason: WidgetForecastReason,
) -> WidgetForecastField {
    WidgetForecastField {
        mode: WidgetForecastMode::Raw,
        raw: Nullable::Value(raw),
        rawSource: Nullable::Value(source),
        reason,
        selected: Nullable::Value(raw),
        selectedSource: Nullable::Null,
        selectedUntil: Nullable::Null,
    }
}

/// Create one explicitly unavailable field.
fn unavailable_widget_field() -> WidgetForecastField {
    WidgetForecastField {
        mode: WidgetForecastMode::Unavailable,
        raw: Nullable::Null,
        rawSource: Nullable::Null,
        reason: WidgetForecastReason::Missing,
        selected: Nullable::Null,
        selectedSource: Nullable::Null,
        selectedUntil: Nullable::Null,
    }
}

/// Map one homogeneous field mode to the snapshot status enum.
fn widget_status_for_mode(mode: WidgetForecastMode) -> WidgetForecastStatus {
    // retain the generated wire-equivalent variant
    match mode {
        WidgetForecastMode::Adjusted => WidgetForecastStatus::Adjusted,
        WidgetForecastMode::Raw => WidgetForecastStatus::Raw,
        WidgetForecastMode::Unavailable => WidgetForecastStatus::Unavailable,
    }
}

/// Recognize the exact one-segment site resource shape from the Node edge regexes.
fn site_resource(pathname: &str) -> Option<&str> {
    let remainder = pathname.strip_prefix("/api/v1/sites/")?;
    let (site, resource) = remainder.split_once('/')?;
    // require one non-empty site key and one terminal resource segment
    if site.is_empty() || resource.is_empty() || resource.contains('/') {
        return None;
    }
    Some(resource)
}
