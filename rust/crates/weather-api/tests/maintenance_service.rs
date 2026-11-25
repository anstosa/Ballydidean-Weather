use std::cell::Cell;
use std::convert::Infallible;

use weather_api::{
    AdjustmentMaintenanceAdmissionBoundary, AdjustmentMaintenanceBodyError,
    AdjustmentMaintenanceBodyRead, AdjustmentMaintenanceCaptureCallback,
    AdjustmentMaintenanceCaptureCycle, AdjustmentMaintenanceErrorCode,
    AdjustmentMaintenanceGapCallback, AdjustmentMaintenanceListenerOwner,
    AdjustmentMaintenanceOperationError, AdjustmentMaintenanceResponseBody,
    AdjustmentMaintenanceScheduleRepository, AdjustmentMaintenanceServicePorts,
    AdjustmentMaintenanceWireEncoder, AdmittedAdjustmentMaintenanceAuthority,
    AdmittedAdjustmentMaintenanceCapture, AdmittedAdjustmentMaintenanceGap,
    AdmittedAdjustmentMaintenanceStage, AdmittedAdjustmentRevisionGap,
    AdmittedAdjustmentRevisionStage, AdmittedRainFixedGaugeTargetGap,
    AdmittedRainFixedGaugeTargetGapStatus, AdmittedRainMaintenanceControlStateGap,
    AdmittedRainMaintenanceControlStateStage, CanonicalInstant,
    DecodedAdjustmentMaintenancePublish, DecodedAdjustmentMaintenanceRequest,
    DecodedAdjustmentMaintenanceStage, DecodedAdjustmentRevisionPublish,
    DecodedAdjustmentRevisionStage, DecodedRainMaintenanceControlStateStage,
    MAINTENANCE_CAPTURE_PATH, MAINTENANCE_GAP_PATH, MAINTENANCE_SCHEDULE_SLOTS_PATH,
    MAX_ADJUSTMENT_MAINTENANCE_REQUEST_BYTES, PreparedAdjustmentMaintenancePublish,
    PreparedAdjustmentRevisionPublish, serve_adjustment_maintenance_request,
    serve_adjustment_maintenance_wire_request,
};
use weather_contracts::{
    AdjustmentMaintenanceCaptureRequest, AdjustmentMaintenanceGapInput,
    AdjustmentMaintenanceGapInputFamilyEnum, AdjustmentRevisionGapInput, AdjustmentShadowFamily,
    AdjustmentShadowRegistrationSlot, AdjustmentShadowRegistrationSlotContractVersionLiteral,
    AdjustmentShadowRegistrationSlotStateEnum, MaintenanceShadowGapReason, Nullable,
    RainFixedGaugeTargetGapInput, RainFixedGaugeTargetGapReceipt, RainFixedGaugeTargetGapStatus,
    RainFixedGaugeTargetGapStatusInput, RainMaintenanceControlStateGapInput,
};
use weather_database::{
    ADJUSTMENT_SHADOW_REGISTRATION_SLOT_SQL, AdjustmentShadowRegistrationSlotBind,
    AdjustmentShadowRegistrationSlotQueryPlan,
};

#[derive(Default)]
struct FixtureBody {
    capture_reads: usize,
    encoded_len: usize,
    gap_reads: usize,
}

impl AdjustmentMaintenanceBodyRead for FixtureBody {
    /// Report the bounded encoded fixture body size.
    fn encoded_len(&self) -> usize {
        self.encoded_len
    }

    /// Return one typed capture body and record the lazy read.
    fn capture(
        &mut self,
    ) -> Result<AdjustmentMaintenanceCaptureRequest, AdjustmentMaintenanceBodyError> {
        self.capture_reads += 1;
        Ok(AdjustmentMaintenanceCaptureRequest {
            dueKey: "capture/2028-08-26T06:35:00.000Z".to_owned(),
            issuedAt: "2028-08-26T06:35:00.000Z".to_owned(),
        })
    }

    /// Return one typed categorical gap and record the lazy read.
    fn gap(&mut self) -> Result<AdjustmentMaintenanceGapInput, AdjustmentMaintenanceBodyError> {
        self.gap_reads += 1;
        Ok(AdjustmentMaintenanceGapInput {
            dueKey: "capture/2028-08-26T06:35:00.000Z".to_owned(),
            family: AdjustmentMaintenanceGapInputFamilyEnum::Wind,
            reason: MaintenanceShadowGapReason::CandidateUnavailable,
            registrationSha256: "a".repeat(64),
        })
    }

    /// Keep publication body variants outside this focused fixture.
    fn shadow_stage(
        &mut self,
    ) -> Result<DecodedAdjustmentMaintenanceStage, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep publication body variants outside this focused fixture.
    fn shadow_publish(
        &mut self,
    ) -> Result<DecodedAdjustmentMaintenancePublish, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep publication body variants outside this focused fixture.
    fn revision_stage(
        &mut self,
    ) -> Result<DecodedAdjustmentRevisionStage, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep publication body variants outside this focused fixture.
    fn revision_publish(
        &mut self,
    ) -> Result<DecodedAdjustmentRevisionPublish, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep revision gaps outside this focused fixture.
    fn revision_gap(
        &mut self,
    ) -> Result<AdjustmentRevisionGapInput, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep rain control-state staging outside this focused fixture.
    fn rain_control_state_stage(
        &mut self,
    ) -> Result<DecodedRainMaintenanceControlStateStage, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep rain control-state gaps outside this focused fixture.
    fn rain_control_state_gap(
        &mut self,
    ) -> Result<RainMaintenanceControlStateGapInput, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep fixed-gauge gaps outside this focused fixture.
    fn rain_fixed_gauge_target_gap(
        &mut self,
    ) -> Result<RainFixedGaugeTargetGapInput, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep fixed-gauge status reads outside this focused fixture.
    fn rain_fixed_gauge_target_gap_status(
        &mut self,
    ) -> Result<RainFixedGaugeTargetGapStatusInput, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }
}

struct FixtureAdmission {
    capture_epoch_ms: i64,
    capture_admissions: Cell<usize>,
    gap_admissions: Cell<usize>,
}

impl AdjustmentMaintenanceAdmissionBoundary for FixtureAdmission {
    /// Keep revision receipt verification outside the capture and schedule fixture.
    fn verify_revision_publish_receipt(
        &self,
        _receipt: &weather_api::AdjustmentRevisionPublishResult,
    ) -> Result<(), AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Attach the source-derived scheduler cycle and canonical issue instant.
    fn capture(
        &self,
        request: AdjustmentMaintenanceCaptureRequest,
    ) -> Result<AdmittedAdjustmentMaintenanceCapture, AdjustmentMaintenanceBodyError> {
        self.capture_admissions
            .set(self.capture_admissions.get() + 1);
        Ok(AdmittedAdjustmentMaintenanceCapture {
            request,
            issued_at: instant(
                self.capture_epoch_ms,
                if self.capture_epoch_ms == 1_850_884_500_000 {
                    "2028-08-26T06:35:00.000Z"
                } else {
                    "2028-08-25T06:35:00.000Z"
                },
            ),
            cycle: AdjustmentMaintenanceCaptureCycle::Hour06,
        })
    }

    /// Preserve the already validated closed gap DTO.
    fn gap(
        &self,
        request: AdjustmentMaintenanceGapInput,
    ) -> Result<AdmittedAdjustmentMaintenanceGap, AdjustmentMaintenanceBodyError> {
        self.gap_admissions.set(self.gap_admissions.get() + 1);
        Ok(AdmittedAdjustmentMaintenanceGap { request })
    }

    /// Keep publication admission outside this focused fixture.
    fn shadow_stage(
        &self,
        _capture_epoch: &CanonicalInstant,
        _request: DecodedAdjustmentMaintenanceStage,
    ) -> Result<AdmittedAdjustmentMaintenanceStage, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep stage receipt admission outside this focused fixture.
    fn shadow_stage_receipt(
        &self,
        _input: &AdmittedAdjustmentMaintenanceStage,
        _receipt: weather_contracts::AdjustmentMaintenanceStageReceipt,
    ) -> Result<weather_contracts::AdjustmentMaintenanceStageReceipt, AdjustmentMaintenanceBodyError>
    {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep publication admission outside this focused fixture.
    fn shadow_publish(
        &self,
        _capture_epoch: &CanonicalInstant,
        _request: DecodedAdjustmentMaintenancePublish,
    ) -> Result<PreparedAdjustmentMaintenancePublish, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep publish receipt admission outside this focused fixture.
    fn shadow_publish_receipt(
        &self,
        _input: &weather_api::AdmittedAdjustmentMaintenancePublish,
        _receipt: weather_contracts::AdjustmentMaintenancePublishReceipt,
    ) -> Result<
        weather_contracts::AdjustmentMaintenancePublishReceipt,
        AdjustmentMaintenanceBodyError,
    > {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep publication admission outside this focused fixture.
    fn revision_stage(
        &self,
        _capture_epoch: &CanonicalInstant,
        _request: DecodedAdjustmentRevisionStage,
    ) -> Result<AdmittedAdjustmentRevisionStage, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep revision stage receipt admission outside this focused fixture.
    fn revision_stage_receipt(
        &self,
        _input: &AdmittedAdjustmentRevisionStage,
        _receipt: weather_contracts::AdjustmentRevisionStageReceipt,
    ) -> Result<weather_contracts::AdjustmentRevisionStageReceipt, AdjustmentMaintenanceBodyError>
    {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep publication admission outside this focused fixture.
    fn revision_publish(
        &self,
        _capture_epoch: &CanonicalInstant,
        _request: DecodedAdjustmentRevisionPublish,
    ) -> Result<PreparedAdjustmentRevisionPublish, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep revision-gap admission outside this focused fixture.
    fn revision_gap(
        &self,
        _request: AdjustmentRevisionGapInput,
    ) -> Result<AdmittedAdjustmentRevisionGap, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep rain control-state staging outside this focused fixture.
    fn rain_control_state_stage(
        &self,
        _capture_epoch: &CanonicalInstant,
        _request: DecodedRainMaintenanceControlStateStage,
    ) -> Result<AdmittedRainMaintenanceControlStateStage, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep rain control-state gaps outside this focused fixture.
    fn rain_control_state_gap(
        &self,
        _request: RainMaintenanceControlStateGapInput,
    ) -> Result<AdmittedRainMaintenanceControlStateGap, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep fixed-gauge gaps outside this focused fixture.
    fn rain_fixed_gauge_target_gap(
        &self,
        _request: RainFixedGaugeTargetGapInput,
    ) -> Result<AdmittedRainFixedGaugeTargetGap, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep fixed-gauge status reads outside this focused fixture.
    fn rain_fixed_gauge_target_gap_status(
        &self,
        _request: RainFixedGaugeTargetGapStatusInput,
    ) -> Result<AdmittedRainFixedGaugeTargetGapStatus, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep fixed-gauge receipt admission outside this focused fixture.
    fn rain_fixed_gauge_target_gap_receipt(
        &self,
        _request: &AdmittedRainFixedGaugeTargetGap,
        _receipt: RainFixedGaugeTargetGapReceipt,
    ) -> Result<RainFixedGaugeTargetGapReceipt, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }

    /// Keep fixed-gauge status outcome admission outside this focused fixture.
    fn rain_fixed_gauge_target_gap_status_outcome(
        &self,
        _request: &AdmittedRainFixedGaugeTargetGapStatus,
        _status: RainFixedGaugeTargetGapStatus,
    ) -> Result<RainFixedGaugeTargetGapStatus, AdjustmentMaintenanceBodyError> {
        Err(AdjustmentMaintenanceBodyError)
    }
}

#[derive(Default)]
struct FixtureRepository {
    reads: Vec<AdjustmentShadowRegistrationSlotQueryPlan>,
}

impl AdjustmentMaintenanceScheduleRepository for FixtureRepository {
    /// Execute one exact schedule read and return its family-bound slot.
    fn schedule_slot(
        &mut self,
        plan: AdjustmentShadowRegistrationSlotQueryPlan,
    ) -> Result<AdjustmentShadowRegistrationSlot, AdjustmentMaintenanceOperationError> {
        assert_eq!(plan.sql, ADJUSTMENT_SHADOW_REGISTRATION_SLOT_SQL);
        let AdjustmentShadowRegistrationSlotBind::Family(family) = plan.binds[0];
        self.reads.push(plan);
        Ok(slot(family))
    }
}

#[derive(Default)]
struct FixtureCapture {
    requests: Vec<AdmittedAdjustmentMaintenanceCapture>,
}

impl AdjustmentMaintenanceCaptureCallback for FixtureCapture {
    /// Record one admitted capture callback invocation.
    fn capture(
        &mut self,
        request: AdmittedAdjustmentMaintenanceCapture,
    ) -> Result<(), AdjustmentMaintenanceOperationError> {
        self.requests.push(request);
        Ok(())
    }
}

#[derive(Default)]
struct FixtureGap {
    requests: Vec<AdmittedAdjustmentMaintenanceGap>,
}

impl AdjustmentMaintenanceGapCallback for FixtureGap {
    /// Record one admitted categorical gap callback invocation.
    fn record_gap(
        &mut self,
        request: AdmittedAdjustmentMaintenanceGap,
    ) -> Result<(), AdjustmentMaintenanceOperationError> {
        self.requests.push(request);
        Ok(())
    }
}

/// Build one canonical instant from the fixture's admitted time pair.
fn instant(epoch_milliseconds: i64, utc: &str) -> CanonicalInstant {
    CanonicalInstant {
        epoch_milliseconds,
        utc: utc.to_owned(),
    }
}

/// Build one family-bound free schedule slot.
fn slot(family: AdjustmentShadowFamily) -> AdjustmentShadowRegistrationSlot {
    AdjustmentShadowRegistrationSlot {
        contractVersion: AdjustmentShadowRegistrationSlotContractVersionLiteral,
        epochWitnessSha256: "d".repeat(64),
        family,
        horizonEndAt: "2028-08-26T07:00:00.000Z".to_owned(),
        registrationSha256: Nullable::Null,
        scheduleContractSha256: "e".repeat(64),
        state: AdjustmentShadowRegistrationSlotStateEnum::Free,
        terminalAt: Nullable::Null,
    }
}

/// Invoke one private maintenance request through admitted listener ownership.
fn request(
    repository: &mut FixtureRepository,
    body: &mut FixtureBody,
    admission: &FixtureAdmission,
    capture: Option<&mut dyn AdjustmentMaintenanceCaptureCallback>,
    gap: &mut FixtureGap,
    method: &str,
    pathname: &str,
) -> weather_api::AdjustmentMaintenanceResponse {
    let capture_epoch = instant(1_850_880_900_000, "2028-08-26T05:35:00.000Z");
    serve_adjustment_maintenance_request(
        AdmittedAdjustmentMaintenanceAuthority::admitted(
            AdjustmentMaintenanceListenerOwner::AdjustmentMaintenanceApplication,
        ),
        repository,
        body,
        AdjustmentMaintenanceServicePorts {
            capture_epoch,
            admission,
            capture,
            gap,
            publish_repository: None,
            publication_relay: None,
        },
        DecodedAdjustmentMaintenanceRequest { method, pathname },
    )
}

#[test]
// preserve fixed schedule order without reading an irrelevant body
fn serves_private_schedule_slots_from_exact_database_plans() {
    let mut repository = FixtureRepository::default();
    let mut body = FixtureBody::default();
    let admission = FixtureAdmission {
        capture_epoch_ms: 1_850_884_500_000,
        capture_admissions: Cell::new(0),
        gap_admissions: Cell::new(0),
    };
    let mut gap = FixtureGap::default();
    let response = request(
        &mut repository,
        &mut body,
        &admission,
        None,
        &mut gap,
        "GET",
        MAINTENANCE_SCHEDULE_SLOTS_PATH,
    );

    assert_eq!(response.status, 200);
    let AdjustmentMaintenanceResponseBody::ScheduleSlots(slots) = response.body else {
        panic!("schedule response must contain the fixed slots")
    };
    assert_eq!(
        slots.iter().map(|slot| slot.family).collect::<Vec<_>>(),
        [
            AdjustmentShadowFamily::Temperature,
            AdjustmentShadowFamily::Wind,
            AdjustmentShadowFamily::Rain,
        ]
    );
    assert_eq!(repository.reads.len(), 3);
    assert_eq!(body.capture_reads, 0);
    assert_eq!(body.gap_reads, 0);
}

struct FixedCaptureWireEncoder;

impl AdjustmentMaintenanceWireEncoder for FixedCaptureWireEncoder {
    type Error = Infallible;

    /// The predefined accepted capture needs no dynamic encoder or allocation.
    fn encode<'a>(
        &'a mut self,
        _: &AdjustmentMaintenanceResponseBody,
    ) -> Result<&'a [u8], Infallible> {
        panic!("fixed capture result invoked the dynamic maintenance encoder");
    }
}

/// Execute admitted capture and gap callbacks with exact response statuses.
#[test]
fn serves_admitted_capture_and_gap_callbacks() {
    let mut repository = FixtureRepository::default();
    let mut body = FixtureBody::default();
    let admission = FixtureAdmission {
        capture_epoch_ms: 1_850_884_500_000,
        capture_admissions: Cell::new(0),
        gap_admissions: Cell::new(0),
    };
    let mut capture = FixtureCapture::default();
    let mut gap = FixtureGap::default();
    let mut encoder = FixedCaptureWireEncoder;
    let capture_response = serve_adjustment_maintenance_wire_request(
        AdmittedAdjustmentMaintenanceAuthority::admitted(
            AdjustmentMaintenanceListenerOwner::AdjustmentMaintenanceApplication,
        ),
        &mut repository,
        &mut body,
        AdjustmentMaintenanceServicePorts {
            capture_epoch: instant(1_850_880_900_000, "2028-08-26T05:35:00.000Z"),
            admission: &admission,
            capture: Some(&mut capture),
            gap: &mut gap,
            publish_repository: None,
            publication_relay: None,
        },
        DecodedAdjustmentMaintenanceRequest {
            method: "POST",
            pathname: MAINTENANCE_CAPTURE_PATH,
        },
        &mut encoder,
    );

    assert_eq!(capture_response.status_code, 202);
    assert_eq!(capture_response.headers.content_type, "application/json");
    assert_eq!(
        capture_response.headers.content_length,
        b"{\"accepted\":true}".len()
    );
    assert_eq!(capture_response.body.as_bytes(), b"{\"accepted\":true}");
    assert_eq!(capture.requests.len(), 1);
    assert_eq!(
        capture.requests[0].request.issuedAt,
        "2028-08-26T06:35:00.000Z"
    );
    assert_eq!(
        capture.requests[0].cycle,
        AdjustmentMaintenanceCaptureCycle::Hour06
    );

    let gap_response = request(
        &mut repository,
        &mut body,
        &admission,
        Some(&mut capture),
        &mut gap,
        "POST",
        MAINTENANCE_GAP_PATH,
    );
    assert_eq!(gap_response.status, 200);
    assert_eq!(
        gap_response.body,
        AdjustmentMaintenanceResponseBody::Recorded
    );
    assert_eq!(gap.requests.len(), 1);
    assert_eq!(body.capture_reads, 1);
    assert_eq!(body.gap_reads, 1);
}

#[test]
// preserve route, inactive-callback, and capture-epoch rejection order
fn preserves_private_maintenance_error_order() {
    let mut repository = FixtureRepository::default();
    let mut body = FixtureBody::default();
    let active_admission = FixtureAdmission {
        capture_epoch_ms: 1_850_884_500_000,
        capture_admissions: Cell::new(0),
        gap_admissions: Cell::new(0),
    };
    let mut gap = FixtureGap::default();
    let missing_callback = request(
        &mut repository,
        &mut body,
        &active_admission,
        None,
        &mut gap,
        "POST",
        MAINTENANCE_CAPTURE_PATH,
    );
    assert_eq!(missing_callback.status, 409);
    assert_eq!(body.capture_reads, 1);
    assert_eq!(active_admission.capture_admissions.get(), 0);

    let stale_admission = FixtureAdmission {
        capture_epoch_ms: 1_850_798_100_000,
        capture_admissions: Cell::new(0),
        gap_admissions: Cell::new(0),
    };
    let mut capture = FixtureCapture::default();
    let stale = request(
        &mut repository,
        &mut body,
        &stale_admission,
        Some(&mut capture),
        &mut gap,
        "POST",
        MAINTENANCE_CAPTURE_PATH,
    );
    assert_eq!(stale.status, 409);
    assert!(capture.requests.is_empty());

    let not_found = request(
        &mut repository,
        &mut body,
        &active_admission,
        Some(&mut capture),
        &mut gap,
        "GET",
        MAINTENANCE_CAPTURE_PATH,
    );
    assert_eq!(not_found.status, 404);
    assert_eq!(
        not_found.body,
        AdjustmentMaintenanceResponseBody::Error(AdjustmentMaintenanceErrorCode::NotFound)
    );
    assert_eq!(body.capture_reads, 2);

    let mut oversized_body = FixtureBody {
        encoded_len: MAX_ADJUSTMENT_MAINTENANCE_REQUEST_BYTES + 1,
        ..FixtureBody::default()
    };
    let oversized = request(
        &mut repository,
        &mut oversized_body,
        &active_admission,
        Some(&mut capture),
        &mut gap,
        "POST",
        MAINTENANCE_CAPTURE_PATH,
    );
    assert_eq!(oversized.status, 409);
    assert_eq!(oversized_body.capture_reads, 0);
}
