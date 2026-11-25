//! Native wire primitives against the retained Node primitive oracle.

#[path = "support/node_primitives.rs"]
mod node_primitives;

use node_primitives::{NodePrimitives, primitive, read_value, value_bytes};
use weather_contracts::JsonValue;
use weather_edge::{ArchiveWirePrimitives, RevisionArchiveCanonicalCodec};
use weather_runtime::{
    DecodedNativeApiTarget, DecodedNativeEdgeTarget, NativeApiRequestTarget,
    NativeApiRequestTargetOwner, NativeEdgeRequestTargetOwner, NativeWirePrimitives,
    WorkerHealthOutput, native_worker_health_output,
};
use weather_worker::WorkerHealth;

type DecodedTargetFields = (String, String, Vec<(String, String)>);

/// Build one generated string value without selecting either serializer.
fn json_string(value: &str) -> JsonValue {
    JsonValue::String(value.encode_utf16().collect())
}

/// Decode one transported predecessor string without normalizing its contents.
fn node_string(value: JsonValue) -> Option<String> {
    let JsonValue::String(units) = value else {
        return None;
    };
    String::from_utf16(&units).ok()
}

/// Decode one predecessor URL field tuple and its ordered query pairs.
fn node_target_fields(operation: &str, input: &JsonValue) -> Option<DecodedTargetFields> {
    let JsonValue::Array(mut fields) = read_value(&primitive(operation, &value_bytes(input))?)?
    else {
        return None;
    };
    let JsonValue::Array(query) = fields.pop()? else {
        return None;
    };
    let second = node_string(fields.pop()?)?;
    let first = node_string(fields.pop()?)?;
    let query = query
        .into_iter()
        .map(|value| {
            let JsonValue::Array(mut pair) = value else {
                return None;
            };
            let value = node_string(pair.pop()?)?;
            let name = node_string(pair.pop()?)?;
            pair.is_empty().then_some((name, value))
        })
        .collect::<Option<Vec<_>>>()?;
    fields.is_empty().then_some((first, second, query))
}

/// Ask the original Fetch and URLSearchParams owners for one API target.
fn node_api_target(target: NativeApiRequestTarget<'_>) -> Option<DecodedNativeApiTarget> {
    let input = JsonValue::Array(
        [target.method, target.host, target.raw_target]
            .into_iter()
            .map(json_string)
            .collect(),
    );
    let (method, pathname, query) = node_target_fields("api-target", &input)?;
    Some(DecodedNativeApiTarget {
        method,
        pathname,
        query,
    })
}

/// Ask the original fixed-base URL owner for one public-edge target.
fn node_edge_target(raw_target: &str) -> Option<DecodedNativeEdgeTarget> {
    let input = json_string(raw_target);
    let (pathname, search, query) = node_target_fields("edge-target", &input)?;
    Some(DecodedNativeEdgeTarget {
        pathname,
        search,
        query,
    })
}

/// Render one actual worker-health DTO through the selected shared wire owner.
fn worker_health_line<Primitives>(primitives: Primitives) -> Vec<u8>
where
    Primitives: ArchiveWirePrimitives + Clone,
{
    let mut output = native_worker_health_output(Vec::new(), primitives);
    output
        .write_health(WorkerHealth {
            last_loop_at_ms: Some(1_790_000_000_000),
            last_success_at_ms: Some(1_789_999_940_000),
            live: true,
            ready: true,
            stale: false,
        })
        .expect("representative worker health should encode");
    output.into_parts().0
}

/// Match Node for actual worker-health output and the underlying archive primitives.
#[test]
fn native_values_match_node_wire_primitives() {
    let native = NativeWirePrimitives::new();
    let node = NodePrimitives;
    let value = JsonValue::Object(vec![
        ("zeta".encode_utf16().collect(), json_string("snowman ☃")),
        (
            "alpha".encode_utf16().collect(),
            JsonValue::Object(vec![
                ("second".encode_utf16().collect(), JsonValue::Number(1.25)),
                ("first".encode_utf16().collect(), JsonValue::Boolean(true)),
            ]),
        ),
        (
            "array".encode_utf16().collect(),
            JsonValue::Array(vec![JsonValue::Null, json_string("line\nbreak")]),
        ),
    ]);

    assert_eq!(worker_health_line(native), worker_health_line(node));
    assert_eq!(
        native.encode_ordered_json(&value),
        node.encode_ordered_json(&value)
    );
    assert_eq!(
        native.encode_sorted_json(&value),
        node.encode_sorted_json(&value)
    );
    assert_ne!(
        native.encode_ordered_json(&value),
        native.encode_sorted_json(&value)
    );
    let encoded = node
        .encode_ordered_json(&value)
        .expect("Node should encode the fixture value");
    assert_eq!(native.decode_json(&encoded), node.decode_json(&encoded));

    let epoch = 1_790_000_000_000;
    let explicit = "2026-11-24T18:00:00-08:00";
    let canonical = "2026-11-25T02:00:00.000Z";
    assert_eq!(
        native.format_utc_milliseconds(epoch),
        node.format_utc_milliseconds(epoch)
    );
    assert_eq!(
        native.parse_explicit_utc_milliseconds(explicit),
        node.parse_explicit_utc_milliseconds(explicit)
    );
    assert_eq!(
        native.canonical_utc_milliseconds(canonical),
        node.canonical_utc_milliseconds(canonical)
    );

    let bytes = b"native wire\0\x01\xfb\xff";
    let encoded_base64 = node
        .encode_standard_base64(bytes)
        .expect("Node should encode the fixture bytes");
    assert_eq!(
        native.encode_standard_base64(bytes),
        Some(encoded_base64.clone())
    );
    assert_eq!(
        native.decode_standard_base64(&encoded_base64),
        node.decode_standard_base64(&encoded_base64)
    );
    assert_eq!(native.sha256(bytes), node.sha256(bytes));
}

/// Match Fetch and fixed-base URL normalization with ordered repeated parameters.
#[test]
fn native_request_targets_match_node_url_owners() {
    let native = NativeWirePrimitives::new();
    let api_target = NativeApiRequestTarget {
        method: "post",
        host: "weather.example:8080",
        raw_target: "/v1/weather/%73tations?station=A%20B&tag=one&tag=two&empty=&plus=a+b",
    };
    assert_eq!(
        native.decode_target(api_target),
        node_api_target(api_target)
    );

    let edge_target = "/forecast/%E2%98%83?station=A%20B&tag=one&tag=two&empty=&plus=a+b";
    assert_eq!(
        native.decode_edge_target(edge_target),
        node_edge_target(edge_target)
    );
}

/// Preserve exact Node UTF-16 strings, keys and duplicate-member behavior.
#[test]
fn native_json_matches_node_raw_utf16() {
    let native = NativeWirePrimitives::new();
    let node = NodePrimitives;
    let source = br#"{"z":"paired \ud83d\ude00","\ud800":"\udc00","single":"\ud800","marker":"\ue000d800\ue000","z":"last","10":10,"2":2}"#;
    let expected = node
        .decode_json(source)
        .expect("Node fixture parses UTF-16");
    assert_eq!(native.decode_json(source), Some(expected.clone()));
    assert_eq!(
        native.encode_ordered_json(&expected),
        node.encode_ordered_json(&expected)
    );
    assert_eq!(
        native.encode_sorted_json(&expected),
        node.encode_sorted_json(&expected)
    );
}
