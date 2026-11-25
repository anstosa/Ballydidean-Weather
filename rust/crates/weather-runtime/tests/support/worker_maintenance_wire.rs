//! Focused concrete worker-to-API maintenance wire adapter checks.

use super::NodePrimitives;
use std::collections::VecDeque;
use weather_contracts::{
    AdjustmentRevisionCommitReceipt, AdjustmentRevisionCommitReceiptContractVersionLiteral,
    AdjustmentRevisionCommitReceiptProjectionKindEnum, AdjustmentRevisionProjectionKind,
    AdjustmentRevisionStageReceipt, AdjustmentRevisionStageReceiptContractVersionLiteral,
    AdjustmentRevisionStageReceiptDurableLiteral, JsonValue,
};
use weather_edge::ArchiveWirePrimitives;
use weather_runtime::{
    HttpWorkerRainMaintenanceClient, NativeWorkerRainMaintenanceHttpJson,
    WorkerRainMaintenanceOriginOwner, WorkerRainMaintenancePostTransport,
    WorkerRainMaintenanceRawPost, WorkerRainMaintenanceRawResponse, WorkerRevisionArchiveClient,
};

const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// Retain raw outgoing bodies and provide deterministic response and wait effects.
#[derive(Default)]
struct CaptureTransport {
    posts: Vec<(String, Vec<u8>)>,
    responses: VecDeque<(u16, Vec<u8>)>,
    waits: Vec<u64>,
}

impl WorkerRainMaintenancePostTransport for CaptureTransport {
    type Error = ();

    /// Capture one already-bounded request before returning its queued raw response.
    fn post(
        &mut self,
        request: WorkerRainMaintenanceRawPost<'_>,
    ) -> Result<WorkerRainMaintenanceRawResponse, Self::Error> {
        assert_eq!(request.origin.as_str(), "http://api/");
        assert_eq!(request.content_type, "application/json");
        assert!(request.body.len() <= request.maximum_response_body_bytes);
        self.posts
            .push((request.path.to_owned(), request.body.to_vec()));
        let (status, body) = self.responses.pop_front().unwrap();
        Ok(WorkerRainMaintenanceRawResponse { status, body })
    }

    /// Record the exact bounded retry schedule without sleeping the fixture.
    fn wait_for_revision_archive_capacity(&mut self, milliseconds: u64) -> Result<(), Self::Error> {
        self.waits.push(milliseconds);
        Ok(())
    }
}

/// Admit only the fixture's exact private origin spelling.
struct Origin;

impl WorkerRainMaintenanceOriginOwner for Origin {
    type Error = ();

    /// Canonicalize the one fixture origin exactly as the process owner would.
    fn admit_worker_rain_maintenance_origin(&mut self, raw: &str) -> Result<String, Self::Error> {
        assert_eq!(raw, "http://api");
        Ok("http://api/".to_owned())
    }
}

/// Build one generated revision stage receipt for outgoing mapping.
fn stage_receipt() -> AdjustmentRevisionStageReceipt {
    AdjustmentRevisionStageReceipt {
        contractVersion: AdjustmentRevisionStageReceiptContractVersionLiteral,
        durable: AdjustmentRevisionStageReceiptDurableLiteral,
        durableAt: "2026-11-24T00:00:00.000Z".to_owned(),
        projectionIdentitySha256: HASH_A.to_owned(),
        projectionKind: AdjustmentRevisionProjectionKind::RainGateInput,
        projectionSha256: HASH_A.to_owned(),
        stageReceiptSha256: HASH_B.to_owned(),
    }
}

/// Build one generated database receipt for outgoing mapping.
fn commit_receipt() -> AdjustmentRevisionCommitReceipt {
    AdjustmentRevisionCommitReceipt {
        archiveCommitOrdinal: "1".to_owned(),
        archiveCommittedAt: "2026-11-24T00:01:00.000Z".to_owned(),
        contractVersion: AdjustmentRevisionCommitReceiptContractVersionLiteral,
        frontierSha256: HASH_A.to_owned(),
        predecessorFrontierSha256: HASH_B.to_owned(),
        projectionIdentitySha256: HASH_A.to_owned(),
        projectionKind: AdjustmentRevisionCommitReceiptProjectionKindEnum::RainGateInput,
        projectionSha256: HASH_A.to_owned(),
        receiptSha256: HASH_B.to_owned(),
        stageReceiptSha256: HASH_B.to_owned(),
    }
}

/// Read one named object field without normalizing the fixture value.
fn field<'a>(value: &'a JsonValue, name: &str) -> &'a JsonValue {
    let JsonValue::Object(fields) = value else {
        panic!("object")
    };
    fields
        .iter()
        .find_map(|(key, value)| key.iter().copied().eq(name.encode_utf16()).then_some(value))
        .unwrap()
}

/// Verify worker publication retains the full projection omitted by the API-to-web relay.
#[test]
fn native_worker_revision_publication_keeps_projection_body() {
    let transport = CaptureTransport {
        responses: VecDeque::from([(200, b"{}".to_vec())]),
        ..CaptureTransport::default()
    };
    let owner = NativeWorkerRainMaintenanceHttpJson::new(NodePrimitives, transport);
    let mut client = HttpWorkerRainMaintenanceClient::admit("http://api", &mut Origin, owner)
        .expect("admitted origin");
    WorkerRevisionArchiveClient::publish_revision(
        &mut client,
        b"projection",
        &stage_receipt(),
        &commit_receipt(),
    )
    .unwrap();
    let (_, transport) = client.into_owner().into_parts();
    assert_eq!(transport.posts.len(), 1);
    assert_eq!(
        transport.posts[0].0,
        "/internal/adjustment-maintenance/revision/publish"
    );
    let value = NodePrimitives.decode_json(&transport.posts[0].1).unwrap();
    assert_eq!(
        field(&value, "projectionBase64"),
        &JsonValue::String("cHJvamVjdGlvbg==".encode_utf16().collect())
    );
    let JsonValue::Object(fields) = value else {
        unreachable!("decoded object")
    };
    assert_eq!(fields.len(), 3);
    assert!(field(&JsonValue::Object(fields), "revisionReceipt") != &JsonValue::Null);
}

/// Verify invalid non-success bodies never enter JSON decoding and retain bounded retries.
#[test]
fn native_worker_capacity_status_precedes_json_and_retries_seven_times() {
    let transport = CaptureTransport {
        responses: VecDeque::from_iter((0..7).map(|_| (429, b"not-json".to_vec()))),
        ..CaptureTransport::default()
    };
    let owner = NativeWorkerRainMaintenanceHttpJson::new(NodePrimitives, transport);
    let mut client = HttpWorkerRainMaintenanceClient::admit("http://api", &mut Origin, owner)
        .expect("admitted origin");
    let error = WorkerRevisionArchiveClient::stage_revision(&mut client, b"projection")
        .expect_err("capacity refusal");
    assert_eq!(error.code, "adjustment_revision_spool_refused");
    let (_, transport) = client.into_owner().into_parts();
    assert_eq!(transport.posts.len(), 7);
    assert_eq!(transport.waits, vec![5_000; 6]);
    // each retry must retain the exact same encoded request body
    assert!(
        transport
            .posts
            .windows(2)
            .all(|posts| posts[0].1 == posts[1].1)
    );
}
