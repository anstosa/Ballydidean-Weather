use weather_contracts::JsonValue;

use super::{producer_population, valid_page_acknowledgement};
use crate::NativeWirePrimitives;
use crate::family_target_catalog::{object, string};

/// Preserve the six required producer closure and four-class revision count.
#[test]
fn derives_complete_required_producer_population() {
    let required = object(vec![
        ("actual_best_match", population(2)),
        ("native_source", population(3)),
        ("rain_gate_input", population(5)),
        ("serving_evidence", population(0)),
        ("shadow_body", population(7)),
        ("shadow_metadata", population(11)),
        ("target_revision", population(13)),
    ]);

    assert_eq!(producer_population(&required).unwrap(), (true, 23));
}

/// Admit one exact canonical page acknowledgement and reject an overflow generation.
#[test]
fn validates_page_acknowledgement_bounds() {
    let mut acknowledgement = object(vec![
        ("acknowledgedAt", string("2026-10-08T16:00:00.000Z")),
        ("contractVersion", string("adjustment-cycle-page-ack/v1")),
        ("dueKey", string("capture/2026-10-08T12:35:00.000Z")),
        ("generation", string("1")),
        ("pageIndex", JsonValue::Number(0.0)),
        ("pageSha256", string(&"a".repeat(64))),
    ]);
    assert!(valid_page_acknowledgement(
        &acknowledgement,
        &NativeWirePrimitives,
    ));
    let JsonValue::Object(fields) = &mut acknowledgement else {
        unreachable!();
    };
    let generation = fields
        .iter_mut()
        .find(|(name, _)| name.iter().copied().eq("generation".encode_utf16()))
        .unwrap();
    generation.1 = string("18446744073709551616");
    assert!(!valid_page_acknowledgement(
        &acknowledgement,
        &NativeWirePrimitives,
    ));
}

/// Construct one admitted input population fixture.
fn population(count: u32) -> JsonValue {
    object(vec![
        ("count", JsonValue::Number(f64::from(count))),
        ("rootSha256", string(&"b".repeat(64))),
    ])
}
