//! Concrete worker request, API relay, and local archive-owner wire composition.

use super::{Clock, FixtureRoot, NodePrimitives};
use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use weather_api::{
    AdjustmentMaintenanceBodyRead, AdjustmentMaintenanceGapCallback,
    AdjustmentMaintenanceOperationError, AdjustmentMaintenancePublicationRelay,
    AdjustmentMaintenanceResponse, AdjustmentMaintenanceResponseBody,
    AdmittedAdjustmentMaintenanceGap, AdmittedAdjustmentMaintenancePublish,
    AdmittedAdjustmentMaintenanceStage, AdmittedAdjustmentRevisionGap,
    AdmittedAdjustmentRevisionPublish, AdmittedAdjustmentRevisionStage,
    AdmittedMaintenanceArchiveOrigin, AdmittedRainFixedGaugeTargetGap,
    AdmittedRainFixedGaugeTargetGapStatus, AdmittedRainMaintenanceControlStateGap,
    AdmittedRainMaintenanceControlStateStage, DecodedAdjustmentRevisionPublish,
    materialize_adjustment_maintenance_wire_response,
};
use weather_contracts::{self as c, FieldPresence, JsonValue};
use weather_edge::{
    ArchiveWireCodec, ArchiveWirePrimitives, PrivateArchiveInput, PrivateArchiveOperation,
    PrivateArchiveStorage, PrivateArchiveTransportRequest, PrivateArchiveWireBody,
    PrivateShadowPublishInput, decode_private_archive_input, serve_private_archive_wire_request,
};
use weather_runtime::{
    ApiMaintenanceArchivePostTransport, ApiMaintenanceArchiveRawPost,
    ApiMaintenanceArchiveRawResponse, HttpApiMaintenanceArchiveRelay,
    NativeAdjustmentMaintenanceWireEncoder, NativeApiMaintenanceArchiveHttpJson,
    NativeApiMaintenanceRequestBodyCodec, RetainedApiMaintenanceRequestBody,
    bind_native_private_archive_stores,
};

type CapturedPosts = Rc<RefCell<Vec<(String, Vec<u8>)>>>;

/// A test-only in-process transport which dispatches raw bytes to the actual native edge owners.
struct LocalArchiveTransport<S> {
    stores: S,
    posts: CapturedPosts,
    decoder: ArchiveWireCodec<NodePrimitives>,
    encoder: ArchiveWireCodec<NodePrimitives>,
}

impl<S: PrivateArchiveStorage> ApiMaintenanceArchivePostTransport for LocalArchiveTransport<S> {
    type Error = ();

    /// Keep real HTTP transport unselected while exercising the actual local raw listener.
    fn post(
        &mut self,
        request: ApiMaintenanceArchiveRawPost<'_>,
    ) -> Result<ApiMaintenanceArchiveRawResponse, Self::Error> {
        assert_eq!(request.origin.canonical(), "http://web:8080/");
        assert_eq!(request.content_type, "application/json");
        self.posts
            .borrow_mut()
            .push((request.path.to_owned(), request.body.to_vec()));
        let wire = serve_private_archive_wire_request(
            PrivateArchiveTransportRequest {
                method: "POST",
                target: request.path,
                content_type: Some(request.content_type),
            },
            &mut PrivateArchiveWireBody::new(request.body, &self.decoder),
            &mut self.stores,
            &mut self.encoder,
        );
        let body = wire.body.as_bytes().to_vec();
        assert!(body.len() <= request.maximum_response_body_bytes);
        Ok(ApiMaintenanceArchiveRawResponse {
            status: wire.status_code,
            body,
        })
    }
}

/// Compose actual typed relay and native stores on a disposable isolated root.
fn relay(
    root: PathBuf,
    posts: CapturedPosts,
) -> impl AdjustmentMaintenancePublicationRelay + AdjustmentMaintenanceGapCallback {
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    let stores = bind_native_private_archive_stores(root, NodePrimitives, Clock).unwrap();
    HttpApiMaintenanceArchiveRelay::new(
        AdmittedMaintenanceArchiveOrigin::admitted("http://web:8080/".to_owned()),
        NativeApiMaintenanceArchiveHttpJson::new(
            NodePrimitives,
            LocalArchiveTransport {
                stores,
                posts,
                decoder: ArchiveWireCodec::new(NodePrimitives),
                encoder: ArchiveWireCodec::new(NodePrimitives),
            },
        ),
    )
}

/// Run the original actual Node archive oracle only on the bounded fixture root.
fn oracle(root: &Path, name: &str, arguments: &[&str]) {
    fs::create_dir(root).unwrap();
    let output = Command::new("node")
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join(name),
        )
        .arg(root)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Compare one concrete private API encoding to the original Node receipt value.
fn assert_receipt(body: AdjustmentMaintenanceResponseBody, expected: &[u8]) {
    let mut encoder = NativeAdjustmentMaintenanceWireEncoder::new(NodePrimitives);
    let response = materialize_adjustment_maintenance_wire_response(
        AdjustmentMaintenanceResponse { status: 200, body },
        false,
        &mut encoder,
    );
    assert_eq!(response.status_code, 200);
    assert_eq!(
        response.headers.content_length,
        response.body.as_bytes().len()
    );
    assert!(!response.body.as_bytes().ends_with(b"\n"));
    let actual = NodePrimitives
        .decode_json(response.body.as_bytes())
        .unwrap();
    let expected = NodePrimitives.decode_json(expected).unwrap();
    assert_eq!(
        NodePrimitives.encode_sorted_json(&actual),
        NodePrimitives.encode_sorted_json(&expected)
    );
}

/// Verify the raw outgoing JSON with the actual web archive handler's original request value.
fn assert_post(posts: &CapturedPosts, index: usize, target: &str, expected: &[u8]) {
    let posts = posts.borrow();
    let (path, raw) = &posts[index];
    assert_eq!(path, target);
    let actual = NodePrimitives.decode_json(raw).unwrap();
    let expected = NodePrimitives.decode_json(expected).unwrap();
    assert_eq!(
        NodePrimitives.encode_sorted_json(&actual),
        NodePrimitives.encode_sorted_json(&expected)
    );
}

/// Turn a retained web request into the worker's original API request envelope.
fn transform_request(bytes: &[u8], remove: &[&str], extra: Option<(&str, JsonValue)>) -> Vec<u8> {
    let JsonValue::Object(mut fields) = NodePrimitives.decode_json(bytes).unwrap() else {
        panic!("request object")
    };
    // preserve the original optional field omissions, not a normalized unknown object
    fields.retain(|(key, _)| {
        !remove
            .iter()
            .any(|name| key.iter().copied().eq(name.encode_utf16()))
    });
    if let Some((key, value)) = extra {
        fields.push((key.encode_utf16().collect(), value));
    }
    NodePrimitives
        .encode_ordered_json(&JsonValue::Object(fields))
        .unwrap()
}

/// Exercise the concrete worker request decoder, compact outgoing receipt relay, and private reply encoder.
#[test]
fn native_api_revision_relay_matches_original_node() {
    let fixture = FixtureRoot::new("api-revision-relay");
    let node = fixture.0.join("node");
    oracle(&node, "private-revision-publication-node-oracle.mjs", &[]);
    // exercise the distinct ordinary and grouped publish grammars against real native file owners
    for name in ["single", "grouped"] {
        let posts = Rc::new(RefCell::new(Vec::new()));
        let mut relay = relay(fixture.0.join(format!("rust-{name}")), Rc::clone(&posts));
        let stage_raw = fs::read(node.join(format!("{name}-stage-first.request.json"))).unwrap();
        let publish_raw =
            fs::read(node.join(format!("{name}-publish-first.request.json"))).unwrap();
        let mut codec = NativeApiMaintenanceRequestBodyCodec::new(NodePrimitives);
        let stage = RetainedApiMaintenanceRequestBody::new(&stage_raw, &mut codec)
            .revision_stage()
            .unwrap();
        let admitted = weather_edge::admit_archive_revision_projection(
            &ArchiveWireCodec::new(NodePrimitives),
            &stage.projection,
        )
        .unwrap();
        let stage_receipt = relay
            .stage_revision(&AdmittedAdjustmentRevisionStage {
                projection: stage.projection.clone(),
                projection_kind: admitted.projection_kind,
            })
            .unwrap();
        assert_receipt(
            AdjustmentMaintenanceResponseBody::RevisionStage(stage_receipt),
            &fs::read(node.join(format!("{name}-stage-first.response.json"))).unwrap(),
        );
        let encoded_projection = NodePrimitives
            .encode_standard_base64(&stage.projection)
            .unwrap();
        let worker_publish = transform_request(
            &publish_raw,
            &[],
            Some((
                "projectionBase64",
                JsonValue::String(encoded_projection.encode_utf16().collect()),
            )),
        );
        let decoded = RetainedApiMaintenanceRequestBody::new(&worker_publish, &mut codec)
            .revision_publish()
            .unwrap();
        let publish = match decoded {
            DecodedAdjustmentRevisionPublish::Single {
                projection,
                revision_receipt,
                stage_receipt,
            } => AdmittedAdjustmentRevisionPublish::Single {
                projection,
                revision_receipt,
                stage_receipt,
            },
            DecodedAdjustmentRevisionPublish::Batch {
                projection,
                revision_receipts,
                stage_receipt,
            } => AdmittedAdjustmentRevisionPublish::Batch {
                projection,
                revision_receipts,
                stage_receipt,
            },
        };
        let receipt = relay.publish_revision(&publish).unwrap();
        assert_receipt(
            AdjustmentMaintenanceResponseBody::RevisionPublish(receipt.clone()),
            &fs::read(node.join(format!("{name}-publish-first.response.json"))).unwrap(),
        );
        // prove the outgoing archive request omits the projection that the worker-to-API request required
        assert_post(
            &posts,
            0,
            weather_edge::PRIVATE_REVISION_STAGE_TARGET,
            &stage_raw,
        );
        assert_post(
            &posts,
            1,
            weather_edge::PRIVATE_REVISION_PUBLISH_TARGET,
            &publish_raw,
        );
        assert_eq!(relay.publish_revision(&publish).unwrap(), receipt);
    }
}

/// Exercise the actual shadow producer byte mapping and v2 publication through both concrete codecs.
#[test]
fn native_api_shadow_relay_matches_original_node() {
    let fixture = FixtureRoot::new("api-shadow-relay");
    let node = fixture.0.join("node");
    oracle(&node, "shadow-producer-node-oracle.mjs", &["publication"]);
    let posts = Rc::new(RefCell::new(Vec::new()));
    let mut relay = relay(fixture.0.join("rust"), Rc::clone(&posts));
    let stage_raw = fs::read(node.join("publication-wind-v2-stage.request.json")).unwrap();
    let publish_raw = fs::read(node.join("publication-wind-v2-publish.request.json")).unwrap();
    let wire = ArchiveWireCodec::new(NodePrimitives);
    let Some(PrivateArchiveInput::StageShadowRevision(input)) = decode_private_archive_input(
        &wire,
        PrivateArchiveOperation::StageShadowRevision,
        &stage_raw,
    ) else {
        panic!("original shadow stage")
    };
    let worker_stage = transform_request(&stage_raw, &["metadata", "sourceProjectionSha256"], None);
    let mut codec = NativeApiMaintenanceRequestBodyCodec::new(NodePrimitives);
    let decoded = RetainedApiMaintenanceRequestBody::new(&worker_stage, &mut codec)
        .shadow_stage()
        .unwrap();
    assert_eq!(decoded.body, input.body);
    assert_eq!(decoded.comparator, input.comparator);
    assert_eq!(decoded.source_projection, input.sourceProjection);
    let stage_receipt = relay
        .stage_shadow(&AdmittedAdjustmentMaintenanceStage {
            relay_input: *input,
        })
        .unwrap();
    assert_receipt(
        AdjustmentMaintenanceResponseBody::ShadowStage(stage_receipt),
        &fs::read(node.join("publication-wind-v2-stage.response.json")).unwrap(),
    );
    let Some(PrivateArchiveInput::PublishShadowRevision(input)) = decode_private_archive_input(
        &wire,
        PrivateArchiveOperation::PublishShadowRevision,
        &publish_raw,
    ) else {
        panic!("original shadow publish")
    };
    let PrivateShadowPublishInput::V2 {
        prediction_committed_at,
        revision_receipt,
        stage_input,
        stage_receipt,
    } = *input
    else {
        panic!("v2 stage")
    };
    let worker_publish =
        transform_request(&publish_raw, &["metadata", "sourceProjectionSha256"], None);
    let decoded = RetainedApiMaintenanceRequestBody::new(&worker_publish, &mut codec)
        .shadow_publish()
        .unwrap();
    assert_eq!(decoded.stage_receipt, stage_receipt);
    assert_eq!(decoded.revision_receipt, revision_receipt);
    let receipt = relay
        .publish_shadow(&AdmittedAdjustmentMaintenancePublish {
            relay_input: c::AdjustmentMaintenancePublishInput {
                body: stage_input.body,
                comparator: stage_input.comparator,
                metadata: stage_input.metadata,
                predictionCommittedAt: FieldPresence::Value(prediction_committed_at),
                revisionReceipt: revision_receipt,
                sourceProjection: stage_input.sourceProjection,
                sourceProjectionSha256: stage_input.sourceProjectionSha256,
                stageReceipt: stage_receipt,
            },
        })
        .unwrap();
    assert_receipt(
        AdjustmentMaintenanceResponseBody::ShadowPublish(receipt),
        &fs::read(node.join("publication-wind-v2-publish.response.json")).unwrap(),
    );
    assert_post(
        &posts,
        0,
        weather_edge::PRIVATE_ARCHIVE_STAGE_TARGET,
        &stage_raw,
    );
    assert_post(
        &posts,
        1,
        weather_edge::PRIVATE_ARCHIVE_PUBLISH_TARGET,
        &publish_raw,
    );
}

/// Retain the observed original v1 API relay request grammar and its ordinary archive refusal.
#[test]
fn native_api_shadow_v1_preserves_original_relay_shape() {
    let fixture = FixtureRoot::new("api-shadow-v1-relay");
    let node = fixture.0.join("node");
    oracle(&node, "shadow-producer-node-oracle.mjs", &["publication"]);
    let original = Command::new("node")
        .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/api-relay-v1-node-oracle.mjs"))
        .arg(&node)
        .output()
        .unwrap();
    assert!(
        original.status.success(),
        "{}",
        String::from_utf8_lossy(&original.stderr)
    );
    let posts = Rc::new(RefCell::new(Vec::new()));
    let mut relay = relay(fixture.0.join("rust"), Rc::clone(&posts));
    let stage_raw = fs::read(node.join("publication-temperature-v1-stage.request.json")).unwrap();
    let publish_raw =
        fs::read(node.join("publication-temperature-v1-publish.request.json")).unwrap();
    let wire = ArchiveWireCodec::new(NodePrimitives);
    let Some(PrivateArchiveInput::StageShadowRevision(input)) = decode_private_archive_input(
        &wire,
        PrivateArchiveOperation::StageShadowRevision,
        &stage_raw,
    ) else {
        panic!("original v1 stage")
    };
    let stage_receipt = relay
        .stage_shadow(&AdmittedAdjustmentMaintenanceStage {
            relay_input: (*input).clone(),
        })
        .unwrap();
    let Some(PrivateArchiveInput::PublishShadowRevision(publish)) = decode_private_archive_input(
        &wire,
        PrivateArchiveOperation::PublishShadowRevision,
        &publish_raw,
    ) else {
        panic!("original v1 compact publish")
    };
    let PrivateShadowPublishInput::V1 {
        revision_receipt, ..
    } = *publish
    else {
        panic!("v1 receipt")
    };
    let input = AdmittedAdjustmentMaintenancePublish {
        relay_input: c::AdjustmentMaintenancePublishInput {
            body: input.body,
            comparator: FieldPresence::Missing,
            metadata: input.metadata,
            predictionCommittedAt: FieldPresence::Missing,
            revisionReceipt: revision_receipt,
            sourceProjection: input.sourceProjection,
            sourceProjectionSha256: input.sourceProjectionSha256,
            stageReceipt: stage_receipt,
        },
    };
    assert_eq!(
        relay.publish_shadow(&input),
        Err(AdjustmentMaintenanceOperationError::Rejected)
    );
    assert_post(
        &posts,
        1,
        weather_edge::PRIVATE_ARCHIVE_PUBLISH_TARGET,
        &fs::read(node.join("api-relay-v1-forwarded.json")).unwrap(),
    );
    let posts = posts.borrow();
    let value = NodePrimitives.decode_json(&posts[1].1).unwrap();
    let JsonValue::Object(fields) = value else {
        panic!("v1 request object")
    };
    let keys = fields
        .iter()
        .map(|(key, _)| String::from_utf16(key).unwrap())
        .collect::<Vec<_>>();
    // preserve the original API relay's full v1 outgoing shape rather than changing the predecessor behavior
    assert_eq!(
        keys,
        [
            "bodyBase64",
            "metadata",
            "revisionReceipt",
            "sourceProjectionBase64",
            "sourceProjectionSha256",
            "stageReceipt"
        ]
    );
}

/// Exercise the other native archive relay operations and both fixed-gauge status variants.
#[test]
fn native_api_state_and_gap_relay_matches_original_node() {
    let fixture = FixtureRoot::new("api-gap-relay");
    let rain_node = fixture.0.join("rain-node");
    oracle(
        &rain_node,
        "rain-control-state-archive-node-oracle.mjs",
        &[],
    );
    let gaps_node = fixture.0.join("gaps-node");
    oracle(&gaps_node, "private-archive-listener-node-oracle.mjs", &[]);
    let posts = Rc::new(RefCell::new(Vec::new()));
    let mut relay = relay(fixture.0.join("rust"), Rc::clone(&posts));
    let mut codec = NativeApiMaintenanceRequestBodyCodec::new(NodePrimitives);
    let stage = fs::read(rain_node.join("rain-control-stage-first.request.json")).unwrap();
    let decoded = RetainedApiMaintenanceRequestBody::new(&stage, &mut codec)
        .rain_control_state_stage()
        .unwrap();
    let receipt = relay
        .stage_rain_control_state(AdmittedRainMaintenanceControlStateStage {
            state: decoded.state,
        })
        .unwrap();
    assert_receipt(
        AdjustmentMaintenanceResponseBody::RainControlStateStage(receipt),
        &fs::read(rain_node.join("rain-control-stage-first.response.json")).unwrap(),
    );
    assert_post(
        &posts,
        0,
        weather_edge::PRIVATE_RAIN_CONTROL_STATE_STAGE_TARGET,
        &stage,
    );
    let gap = fs::read(rain_node.join("rain-control-gap-first.request.json")).unwrap();
    let request = RetainedApiMaintenanceRequestBody::new(&gap, &mut codec)
        .rain_control_state_gap()
        .unwrap();
    relay
        .record_rain_control_state_gap(AdmittedRainMaintenanceControlStateGap { request })
        .unwrap();
    assert_post(
        &posts,
        1,
        weather_edge::PRIVATE_RAIN_CONTROL_STATE_GAP_TARGET,
        &gap,
    );
    let status = fs::read(gaps_node.join("gauge-absent.request.json")).unwrap();
    let request = RetainedApiMaintenanceRequestBody::new(&status, &mut codec)
        .rain_fixed_gauge_target_gap_status()
        .unwrap();
    let result = relay
        .read_rain_fixed_gauge_target_gap(&AdmittedRainFixedGaugeTargetGapStatus {
            request: request.clone(),
        })
        .unwrap();
    assert_receipt(
        AdjustmentMaintenanceResponseBody::RainFixedGaugeTargetGapStatus(result),
        &fs::read(gaps_node.join("gauge-absent.response.json")).unwrap(),
    );
    let gap = fs::read(gaps_node.join("gauge-first.request.json")).unwrap();
    let gap_request = RetainedApiMaintenanceRequestBody::new(&gap, &mut codec)
        .rain_fixed_gauge_target_gap()
        .unwrap();
    let receipt = relay
        .record_rain_fixed_gauge_target_gap(&AdmittedRainFixedGaugeTargetGap {
            request: gap_request,
        })
        .unwrap();
    assert_receipt(
        AdjustmentMaintenanceResponseBody::RainFixedGaugeTargetGap(receipt),
        &fs::read(gaps_node.join("gauge-first.response.json")).unwrap(),
    );
    let result = relay
        .read_rain_fixed_gauge_target_gap(&AdmittedRainFixedGaugeTargetGapStatus { request })
        .unwrap();
    assert_receipt(
        AdjustmentMaintenanceResponseBody::RainFixedGaugeTargetGapStatus(result),
        &fs::read(gaps_node.join("gauge-present.response.json")).unwrap(),
    );
    assert_post(
        &posts,
        2,
        weather_edge::PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_STATUS_TARGET,
        &status,
    );
    assert_post(
        &posts,
        3,
        weather_edge::PRIVATE_RAIN_FIXED_GAUGE_TARGET_GAP_TARGET,
        &gap,
    );
    let shadow_gap = fs::read(gaps_node.join("shadow-gap-first.request.json")).unwrap();
    let request = RetainedApiMaintenanceRequestBody::new(&shadow_gap, &mut codec)
        .gap()
        .unwrap();
    relay
        .record_gap(AdmittedAdjustmentMaintenanceGap { request })
        .unwrap();
    assert_post(
        &posts,
        5,
        weather_edge::PRIVATE_ARCHIVE_GAP_TARGET,
        &shadow_gap,
    );
    let revision_gap = fs::read(gaps_node.join("revision-gap-first.request.json")).unwrap();
    let request = RetainedApiMaintenanceRequestBody::new(&revision_gap, &mut codec)
        .revision_gap()
        .unwrap();
    relay
        .record_revision_gap(AdmittedAdjustmentRevisionGap { request })
        .unwrap();
    assert_post(
        &posts,
        6,
        weather_edge::PRIVATE_REVISION_GAP_TARGET,
        &revision_gap,
    );
}

/// Keep the exact original schedule array order and the database slot member order.
#[test]
fn native_api_maintenance_schedule_wire_matches_original_node() {
    let fixture = FixtureRoot::new("api-schedule-wire");
    let expected = fixture.0.join("schedule.json");
    let oracle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../weather-api/tests/maintenance-schedule-node-oracle.mjs");
    let output = Command::new("node")
        .arg(oracle)
        .arg(&expected)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let slots = [
        c::AdjustmentShadowFamily::Temperature,
        c::AdjustmentShadowFamily::Wind,
        c::AdjustmentShadowFamily::Rain,
    ]
    .into_iter()
    .map(|family| c::AdjustmentShadowRegistrationSlot {
        contractVersion: c::AdjustmentShadowRegistrationSlotContractVersionLiteral,
        epochWitnessSha256: "d".repeat(64),
        family,
        horizonEndAt: "2028-08-26T07:00:00.000Z".to_owned(),
        registrationSha256: c::Nullable::Null,
        scheduleContractSha256: "e".repeat(64),
        state: c::AdjustmentShadowRegistrationSlotStateEnum::Free,
        terminalAt: c::Nullable::Null,
    })
    .collect();
    let mut encoder = NativeAdjustmentMaintenanceWireEncoder::new(NodePrimitives);
    let response = materialize_adjustment_maintenance_wire_response(
        AdjustmentMaintenanceResponse {
            status: 200,
            body: AdjustmentMaintenanceResponseBody::ScheduleSlots(slots),
        },
        false,
        &mut encoder,
    );
    assert_eq!(response.status_code, 200);
    assert_eq!(response.body.as_bytes(), fs::read(expected).unwrap());
    assert_eq!(
        response.headers.content_length,
        response.body.as_bytes().len()
    );
}
