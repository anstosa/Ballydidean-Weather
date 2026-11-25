use std::convert::Infallible;
use std::path::PathBuf;
use std::process::Command;

use weather_contracts::{
    AdjustmentMaintenanceGapInput, AdjustmentMaintenanceGapInputFamilyEnum,
    AdjustmentMaintenancePublishReceipt, AdjustmentMaintenanceStageInput,
    AdjustmentMaintenanceStageReceipt, AdjustmentRevisionGapInput, AdjustmentRevisionGapReason,
    AdjustmentRevisionProjectionKind, AdjustmentRevisionStageReceipt, MaintenanceShadowGapReason,
    Nullable, RainFixedGaugeTargetGapInput, RainFixedGaugeTargetGapReceipt,
    RainFixedGaugeTargetGapStatus, RainFixedGaugeTargetGapStatusAbsent,
    RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
    RainFixedGaugeTargetGapStatusAbsentStateLiteral, RainFixedGaugeTargetGapStatusInput,
    RainMaintenanceControlStateGapInput, RainMaintenanceControlStateStageReceipt,
};
use weather_edge::{
    AdjustmentMaintenanceGapReceipt, AdjustmentRevisionGapReceipt, AdmittedPrivateArchiveRequest,
    EdgeForecastTerminalObserver, EdgeWireAdminCookie, EdgeWireBody, EdgeWireHeaders,
    EdgeWireReply, EdgeWireWriter, PRIVATE_ARCHIVE_HANDLER_TARGETS,
    PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES, PRIVATE_ARCHIVE_STAGE_TARGET,
    PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET, PrivateArchiveBodyReader,
    PrivateArchiveErrorBody, PrivateArchiveFixedGaugeStore, PrivateArchiveInput,
    PrivateArchiveOperation, PrivateArchivePermanentGapStore, PrivateArchiveRainControlStore,
    PrivateArchiveReceipt, PrivateArchiveResponseBody, PrivateArchiveRevisionStore,
    PrivateArchiveShadowStore, PrivateArchiveStatus, PrivateArchiveStorage,
    PrivateArchiveStorageError, PrivateArchiveTransportRequest, PrivateArchiveWireReceiptEncoder,
    PrivateRevisionPublishInput, PrivateRevisionPublishReceipt, PrivateShadowPublishInput,
    PublicServingHeaderProfile, RainMaintenanceControlStateGapReceipt,
    SUPPORTED_PRIVATE_ARCHIVE_TARGETS, dispatch_private_archive_request, emit_edge_wire_response,
    serve_private_archive_request, serve_private_archive_wire_request,
};

const GAP_STATUS_WIRE: &[u8] =
    b"{\"contractVersion\":\"adjustment-rain-fixed-gauge-target-gap-status/v1\",\"state\":\"absent\"}\n";

#[derive(Default)]
struct ReceiptEncoder(usize);

impl PrivateArchiveWireReceiptEncoder for ReceiptEncoder {
    type Error = Infallible;

    /// Return the fixture's canonical receipt after observing the selected typed result.
    fn encode_receipt(&mut self, receipt: &PrivateArchiveReceipt) -> Result<Vec<u8>, Infallible> {
        assert!(matches!(
            receipt,
            PrivateArchiveReceipt::RainFixedGaugeTargetGapStatus(
                RainFixedGaugeTargetGapStatus::Absent(_)
            )
        ));
        self.0 += 1;
        Ok(GAP_STATUS_WIRE.to_vec())
    }
}

#[derive(Default)]
struct ArchiveWireWriter {
    head: Option<(u16, usize)>,
    body: Option<EdgeWireBody>,
}

impl EdgeWireWriter<()> for ArchiveWireWriter {
    type Error = Infallible;

    /// Record the private archive status and ordinary representation length.
    fn write_head(
        &mut self,
        status_code: u16,
        _: PublicServingHeaderProfile,
        headers: &EdgeWireHeaders,
        _: Option<&EdgeWireAdminCookie>,
    ) -> Result<(), Infallible> {
        assert_eq!(headers.cache_control, "no-store");
        assert_eq!(
            headers.content_type.as_deref(),
            Some("application/json; charset=utf-8")
        );
        self.head = Some((status_code, headers.content_length));
        Ok(())
    }

    /// Archive responses never register a public forecast observer.
    fn install_forecast_terminal(
        &mut self,
        _: EdgeForecastTerminalObserver<()>,
    ) -> Result<(), Infallible> {
        panic!("private wire writer cannot own public forecast observation")
    }

    /// Retain the response only after its headers are available.
    fn send_body(&mut self, body: EdgeWireBody) -> Result<(), Infallible> {
        assert!(self.head.is_some());
        self.body = Some(body);
        Ok(())
    }
}

/// Route a real private application read and ordinary spool refusal to the shared writer.
#[test]
fn private_archive_wire_preserves_receipt_and_spool_refusal() {
    // use the same supported post route for both ordinary storage outcomes
    for (storage_error, status, payload, encode_calls) in [
        (None, 200, GAP_STATUS_WIRE, 1),
        (
            Some(PrivateArchiveStorageError::RevisionSpoolRefused),
            429,
            b"{\"error\":\"adjustment_revision_spool_full\"}\n".as_slice(),
            0,
        ),
    ] {
        let mut body = FixtureBodyReader::fixed_status();
        let mut storage = FixtureStorage {
            calls: 0,
            fixed_status_error: storage_error,
        };
        let mut encoder = ReceiptEncoder::default();
        let response = serve_private_archive_wire_request(
            PrivateArchiveTransportRequest {
                method: "POST",
                target: PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
                content_type: Some("application/json"),
            },
            &mut body,
            &mut storage,
            &mut encoder,
        );
        assert_eq!((body.calls, storage.calls, encoder.0), (1, 1, encode_calls));
        let mut writer = ArchiveWireWriter::default();
        emit_edge_wire_response(EdgeWireReply::without_forecast(response), &mut writer).unwrap();
        assert_eq!(writer.head, Some((status, payload.len())));
        assert_eq!(writer.body.unwrap().as_bytes(), payload);
    }
}

#[derive(Debug, Default)]
struct FixtureStorage {
    calls: usize,
    fixed_status_error: Option<PrivateArchiveStorageError>,
}

impl PrivateArchiveRainControlStore for FixtureStorage {
    /// reject unused rain control staging in this focused fixture
    fn stage_rain_control_state(
        &mut self,
        _bytes: Vec<u8>,
    ) -> Result<RainMaintenanceControlStateStageReceipt, PrivateArchiveStorageError> {
        unreachable!("rain control stage is not selected")
    }

    /// reject unused rain control gaps in this focused fixture
    fn record_rain_control_state_gap(
        &mut self,
        _input: RainMaintenanceControlStateGapInput,
    ) -> Result<RainMaintenanceControlStateGapReceipt, PrivateArchiveStorageError> {
        unreachable!("rain control gap is not selected")
    }
}

impl PrivateArchiveFixedGaugeStore for FixtureStorage {
    /// reject unused fixed-gauge writes in this focused fixture
    fn record_rain_fixed_gauge_target_gap(
        &mut self,
        _input: RainFixedGaugeTargetGapInput,
    ) -> Result<RainFixedGaugeTargetGapReceipt, PrivateArchiveStorageError> {
        unreachable!("fixed-gauge gap write is not selected")
    }

    /// return one injected fixed-gauge status outcome
    fn read_rain_fixed_gauge_target_gap(
        &mut self,
        _input: RainFixedGaugeTargetGapStatusInput,
    ) -> Result<RainFixedGaugeTargetGapStatus, PrivateArchiveStorageError> {
        self.calls += 1;
        // surface the requested operational outcome
        if let Some(error) = self.fixed_status_error.take() {
            return Err(error);
        }
        Ok(RainFixedGaugeTargetGapStatus::Absent(
            RainFixedGaugeTargetGapStatusAbsent {
                contractVersion: RainFixedGaugeTargetGapStatusAbsentContractVersionLiteral,
                state: RainFixedGaugeTargetGapStatusAbsentStateLiteral,
            },
        ))
    }
}

impl PrivateArchiveStorage for FixtureStorage {
    /// bind the separately tested rain control fixture
    fn rain_control_store(&mut self) -> &mut dyn PrivateArchiveRainControlStore {
        self
    }

    /// bind the separately tested fixed-gauge fixture
    fn fixed_gauge_gap_store(&mut self) -> &mut dyn PrivateArchiveFixedGaugeStore {
        self
    }

    /// Bind the separate shadow fixture.
    fn shadow_store(&mut self) -> &mut dyn PrivateArchiveShadowStore {
        self
    }

    /// Bind the separate projection fixture.
    fn revision_store(&mut self) -> &mut dyn PrivateArchiveRevisionStore {
        self
    }

    /// Bind the separate disposition fixture.
    fn permanent_gap_store(&mut self) -> &mut dyn PrivateArchivePermanentGapStore {
        self
    }
}

impl PrivateArchiveShadowStore for FixtureStorage {
    /// reject unused shadow staging in this focused fixture
    fn stage_shadow_revision(
        &mut self,
        _input: AdjustmentMaintenanceStageInput,
    ) -> Result<AdjustmentMaintenanceStageReceipt, PrivateArchiveStorageError> {
        unreachable!("shadow stage is not selected")
    }

    /// reject unused shadow publication in this focused fixture
    fn publish_shadow_revision(
        &mut self,
        _input: PrivateShadowPublishInput,
    ) -> Result<AdjustmentMaintenancePublishReceipt, PrivateArchiveStorageError> {
        unreachable!("shadow publish is not selected")
    }
}

impl PrivateArchiveRevisionStore for FixtureStorage {
    /// reject unused revision staging in this focused fixture
    fn stage_revision(
        &mut self,
        _bytes: Vec<u8>,
    ) -> Result<AdjustmentRevisionStageReceipt, PrivateArchiveStorageError> {
        unreachable!("revision stage is not selected")
    }

    /// reject unused revision publication in this focused fixture
    fn publish_revision(
        &mut self,
        _input: PrivateRevisionPublishInput,
    ) -> Result<PrivateRevisionPublishReceipt, PrivateArchiveStorageError> {
        unreachable!("revision publish is not selected")
    }
}

impl PrivateArchivePermanentGapStore for FixtureStorage {
    /// reject unused shadow gaps in this focused fixture
    fn record_shadow_gap(
        &mut self,
        _input: AdjustmentMaintenanceGapInput,
    ) -> Result<AdjustmentMaintenanceGapReceipt, PrivateArchiveStorageError> {
        unreachable!("shadow gap is not selected")
    }

    /// reject unused revision gaps in this focused fixture
    fn record_revision_gap(
        &mut self,
        _input: AdjustmentRevisionGapInput,
    ) -> Result<AdjustmentRevisionGapReceipt, PrivateArchiveStorageError> {
        unreachable!("revision gap is not selected")
    }
}

#[derive(Debug)]
struct FixtureBodyReader {
    calls: usize,
    fail: bool,
    input: Option<PrivateArchiveInput>,
    maximum_bytes: Option<usize>,
    operation: Option<PrivateArchiveOperation>,
}

impl FixtureBodyReader {
    /// create one unread fixed-gauge status body
    fn fixed_status() -> Self {
        Self {
            calls: 0,
            fail: false,
            input: Some(PrivateArchiveInput::ReadRainFixedGaugeTargetGap(
                RainFixedGaugeTargetGapStatusInput {
                    logicalHourAt: "2026-11-23T12:00:00.000Z".to_owned(),
                    logicalKeySha256: "a".repeat(64),
                },
            )),
            maximum_bytes: None,
            operation: None,
        }
    }
}

impl PrivateArchiveBodyReader for FixtureBodyReader {
    type Error = ();

    /// record one lazy bounded decode request
    fn read_decoded(
        &mut self,
        operation: PrivateArchiveOperation,
        maximum_bytes: usize,
    ) -> Result<PrivateArchiveInput, Self::Error> {
        self.calls += 1;
        self.maximum_bytes = Some(maximum_bytes);
        self.operation = Some(operation);
        // inject one ordinary decoder failure
        if self.fail {
            return Err(());
        }
        self.input.take().ok_or(())
    }
}

/// build one admitted fixed-gauge status request
fn fixed_status_request<'a>(
    method: &'a str,
    target: &'a str,
    content_type: Option<&'a str>,
    encoded_body_bytes: usize,
) -> AdmittedPrivateArchiveRequest<'a> {
    AdmittedPrivateArchiveRequest {
        method,
        target,
        content_type,
        encoded_body_bytes,
        input: PrivateArchiveInput::ReadRainFixedGaugeTargetGap(
            RainFixedGaugeTargetGapStatusInput {
                logicalHourAt: "2026-11-23T12:00:00.000Z".to_owned(),
                logicalKeySha256: "a".repeat(64),
            },
        ),
    }
}

/// extract one listener error body
fn error_body(response: &weather_edge::PrivateArchiveResponse) -> PrivateArchiveErrorBody {
    // require one error response
    match &response.body {
        PrivateArchiveResponseBody::Error(error) => *error,
        PrivateArchiveResponseBody::Receipt(_) => panic!("expected listener error"),
    }
}

// verifies exact source route order and ordinary status parity
#[test]
fn matches_actual_node_private_archive_listener_and_handler() {
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/private-archive-service-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .output()
        .expect("run private archive Node oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let node = String::from_utf8(output.stdout).expect("Node oracle UTF-8");
    let expected = format!(
        concat!(
            "listener-targets|{}\n",
            "handler-targets|{}\n",
            "handler-calls|stageRainControlState,recordRainControlStateGap,",
            "recordRainFixedGaugeTargetGap,readRainFixedGaugeTargetGap,",
            "stageShadowRevision,publishShadowRevision,recordShadowGap,stageProjection,",
            "publishRevision,publishRevision,recordGap\n",
            "gap-inputs|dueKey,family,reason,registrationSha256|",
            "logicalKeySha256,projectionIdentitySha256,projectionKind,projectionSha256,reason\n",
            "method|404|not_found|\n",
            "target|404|not_found|\n",
            "media|415|unsupported_media_type|\n",
            "ok|200|stage|read:{},dispatch:{}\n",
            "body|409|revision_archive_rejected|read:{}\n",
            "spool|429|adjustment_revision_spool_full|read:{},dispatch:{}\n",
            "rejected|409|revision_archive_rejected|read:{},dispatch:{}\n",
        ),
        SUPPORTED_PRIVATE_ARCHIVE_TARGETS.join(","),
        PRIVATE_ARCHIVE_HANDLER_TARGETS.join(","),
        PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES,
        PRIVATE_ARCHIVE_STAGE_TARGET,
        PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES,
        PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES,
        PRIVATE_ARCHIVE_STAGE_TARGET,
        PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES,
        PRIVATE_ARCHIVE_STAGE_TARGET,
    );
    assert_eq!(node, expected);
}

// verifies the transport boundary stays lazy through route and media admission
#[test]
fn admits_transport_before_requesting_any_body_bytes() {
    let rejected = [
        PrivateArchiveTransportRequest {
            method: "GET",
            target: PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
            content_type: Some("application/json"),
        },
        PrivateArchiveTransportRequest {
            method: "POST",
            target: "/internal/adjustment-maintenance/archive/unknown",
            content_type: Some("application/json"),
        },
        PrivateArchiveTransportRequest {
            method: "POST",
            target: PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
            content_type: Some("text/plain"),
        },
    ];

    // prove every pre-body rejection leaves the reader untouched
    for request in rejected {
        let mut body = FixtureBodyReader::fixed_status();
        let mut storage = FixtureStorage::default();
        let response = serve_private_archive_request(request, &mut body, &mut storage);
        assert_ne!(response.status, PrivateArchiveStatus::Ok);
        assert_eq!(body.calls, 0);
        assert_eq!(storage.calls, 0);
    }

    let mut body = FixtureBodyReader::fixed_status();
    let mut storage = FixtureStorage::default();
    let response = serve_private_archive_request(
        PrivateArchiveTransportRequest {
            method: "POST",
            target: PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
            content_type: Some("application/json; charset=utf-8"),
        },
        &mut body,
        &mut storage,
    );
    assert_eq!(response.status, PrivateArchiveStatus::Ok);
    assert_eq!(body.calls, 1);
    assert_eq!(body.maximum_bytes, Some(PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES));
    assert_eq!(
        body.operation,
        Some(PrivateArchiveOperation::ReadRainFixedGaugeTargetGap)
    );
    assert_eq!(storage.calls, 1);

    let mut body = FixtureBodyReader::fixed_status();
    body.fail = true;
    let mut storage = FixtureStorage::default();
    let response = serve_private_archive_request(
        PrivateArchiveTransportRequest {
            method: "POST",
            target: PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
            content_type: Some("application/json"),
        },
        &mut body,
        &mut storage,
    );
    assert_eq!(response.status, PrivateArchiveStatus::Conflict);
    assert_eq!(
        error_body(&response),
        PrivateArchiveErrorBody::RevisionArchiveRejected
    );
    assert_eq!(body.calls, 1);
    assert_eq!(storage.calls, 0);
}

// verifies route, media and body gates precede storage dispatch
#[test]
fn rejects_before_storage_dispatch_in_listener_order() {
    let fixtures = [
        (
            fixed_status_request(
                "GET",
                PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
                Some("application/json"),
                2,
            ),
            PrivateArchiveStatus::NotFound,
            PrivateArchiveErrorBody::NotFound,
        ),
        (
            fixed_status_request(
                "POST",
                "/internal/adjustment-maintenance/archive/unknown",
                Some("application/json"),
                2,
            ),
            PrivateArchiveStatus::NotFound,
            PrivateArchiveErrorBody::NotFound,
        ),
        (
            fixed_status_request(
                "POST",
                PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
                Some("text/plain"),
                2,
            ),
            PrivateArchiveStatus::UnsupportedMediaType,
            PrivateArchiveErrorBody::UnsupportedMediaType,
        ),
        (
            fixed_status_request(
                "POST",
                PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
                Some("application/json"),
                PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES + 1,
            ),
            PrivateArchiveStatus::Conflict,
            PrivateArchiveErrorBody::RevisionArchiveRejected,
        ),
    ];

    // verify every pre-dispatch rejection independently
    for (request, expected_status, expected_error) in fixtures {
        let mut storage = FixtureStorage::default();
        let response = dispatch_private_archive_request(request, &mut storage);
        assert_eq!(response.status, expected_status);
        assert_eq!(error_body(&response), expected_error);
        assert_eq!(storage.calls, 0);
    }
}

// verifies the selected storage operation and closed failure mapping
#[test]
fn dispatches_selected_operation_and_maps_storage_outcomes() {
    let mut storage = FixtureStorage::default();
    let response = dispatch_private_archive_request(
        fixed_status_request(
            "POST",
            PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
            Some(" Application/JSON ; charset=utf-8"),
            PRIVATE_ARCHIVE_MAXIMUM_BODY_BYTES,
        ),
        &mut storage,
    );
    assert_eq!(response.status.code(), 200);
    assert_eq!(storage.calls, 1);
    assert!(matches!(
        response.body,
        PrivateArchiveResponseBody::Receipt(PrivateArchiveReceipt::RainFixedGaugeTargetGapStatus(
            RainFixedGaugeTargetGapStatus::Absent(_)
        ))
    ));

    storage.fixed_status_error = Some(PrivateArchiveStorageError::RevisionSpoolRefused);
    let response = dispatch_private_archive_request(
        fixed_status_request(
            "POST",
            PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
            Some("application/json"),
            2,
        ),
        &mut storage,
    );
    assert_eq!(response.status.code(), 429);
    assert_eq!(
        error_body(&response).code(),
        "adjustment_revision_spool_full"
    );

    storage.fixed_status_error = Some(PrivateArchiveStorageError::Rejected);
    let response = dispatch_private_archive_request(
        fixed_status_request(
            "POST",
            PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
            Some("application/json"),
            2,
        ),
        &mut storage,
    );
    assert_eq!(response.status.code(), 409);
    assert_eq!(error_body(&response).code(), "revision_archive_rejected");
    assert_eq!(
        PrivateArchiveStorageError::RevisionSpoolRefused.code(),
        Some("adjustment_revision_spool_refused")
    );
}

// verifies the route-neutral handler mapping remains complete
#[test]
fn maps_every_handler_target_to_its_exact_operation() {
    let expected = [
        PrivateArchiveOperation::StageRainControlState,
        PrivateArchiveOperation::RecordRainControlStateGap,
        PrivateArchiveOperation::RecordRainFixedGaugeTargetGap,
        PrivateArchiveOperation::ReadRainFixedGaugeTargetGap,
        PrivateArchiveOperation::StageShadowRevision,
        PrivateArchiveOperation::PublishShadowRevision,
        PrivateArchiveOperation::RecordShadowGap,
        PrivateArchiveOperation::StageRevision,
        PrivateArchiveOperation::PublishRevision,
        PrivateArchiveOperation::RecordRevisionGap,
    ];

    // bind each source-ordered target to one operation
    for (target, expected_operation) in PRIVATE_ARCHIVE_HANDLER_TARGETS.into_iter().zip(expected) {
        assert_eq!(
            PrivateArchiveOperation::from_target(target),
            Some(expected_operation)
        );
    }
    assert_eq!(PrivateArchiveOperation::from_target("/unknown"), None);
}

// verifies decoded-input mismatch stays an ordinary closed rejection
#[test]
fn rejects_decoded_input_that_does_not_match_the_selected_route() {
    let mut storage = FixtureStorage::default();
    let response = dispatch_private_archive_request(
        AdmittedPrivateArchiveRequest {
            method: "POST",
            target: PRIVATE_ARCHIVE_STAGE_TARGET,
            content_type: Some("application/json"),
            encoded_body_bytes: 2,
            input: PrivateArchiveInput::StageRevision(vec![b'{', b'}']),
        },
        &mut storage,
    );
    assert_eq!(response.status, PrivateArchiveStatus::Conflict);
    assert_eq!(
        error_body(&response),
        PrivateArchiveErrorBody::RevisionArchiveRejected
    );
    assert_eq!(storage.calls, 0);
}

// verifies generated gap DTO fields match the actual store key oracle
#[test]
fn generated_shadow_and_revision_gap_inputs_keep_store_field_shapes() {
    let shadow = AdjustmentMaintenanceGapInput {
        dueKey: "capture/2026-11-23T12:35:00.000Z".to_owned(),
        family: AdjustmentMaintenanceGapInputFamilyEnum::Rain,
        reason: MaintenanceShadowGapReason::ArchiveStageFailed,
        registrationSha256: "a".repeat(64),
    };
    let AdjustmentMaintenanceGapInput {
        dueKey,
        family,
        reason,
        registrationSha256,
    } = shadow;
    assert_eq!(dueKey, "capture/2026-11-23T12:35:00.000Z");
    assert_eq!(family, AdjustmentMaintenanceGapInputFamilyEnum::Rain);
    assert_eq!(reason, MaintenanceShadowGapReason::ArchiveStageFailed);
    assert_eq!(registrationSha256.len(), 64);

    let revision = AdjustmentRevisionGapInput {
        logicalKeySha256: "b".repeat(64),
        projectionIdentitySha256: Nullable::Null,
        projectionKind: AdjustmentRevisionProjectionKind::TargetRevision,
        projectionSha256: Nullable::Null,
        reason: AdjustmentRevisionGapReason::ArchiveStageFailed,
    };
    let AdjustmentRevisionGapInput {
        logicalKeySha256,
        projectionIdentitySha256,
        projectionKind,
        projectionSha256,
        reason,
    } = revision;
    assert_eq!(logicalKeySha256.len(), 64);
    assert_eq!(projectionIdentitySha256, Nullable::Null);
    assert_eq!(
        projectionKind,
        AdjustmentRevisionProjectionKind::TargetRevision
    );
    assert_eq!(projectionSha256, Nullable::Null);
    assert_eq!(reason, AdjustmentRevisionGapReason::ArchiveStageFailed);
}
