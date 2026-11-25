use std::collections::BTreeSet;

use super::{expected_rain_parts, hash_string_array};
use crate::NativeWirePrimitives;

#[test]
fn rain_expected_parts_match_the_predecessor_334_day_geometry() {
    let wire = NativeWirePrimitives::new();
    let parts = expected_rain_parts(
        "2026-10-12T07:00:00.000Z",
        "2027-09-11T07:00:00.000Z",
        &wire,
    )
    .expect("rain geometry");
    let logical = parts
        .iter()
        .map(|part| part.logical_chunk_index)
        .collect::<BTreeSet<_>>();
    let key_count = parts
        .iter()
        .map(|part| part.expected_keys.len())
        .sum::<usize>();
    let mut logical_roots = Vec::new();
    // reproduce the predecessor hierarchy from physical expected-key subsets
    for logical_index in 0_usize..24 {
        let part_roots = parts
            .iter()
            .filter(|part| part.logical_chunk_index == logical_index)
            .map(|part| hash_string_array(&wire, &part.expected_keys).expect("part root"))
            .collect::<Vec<_>>();
        logical_roots.push(hash_string_array(&wire, &part_roots).expect("logical root"));
    }
    let reserved = hash_string_array(&wire, &logical_roots).expect("reserved root");

    assert_eq!(logical, (0_usize..24).collect());
    assert_eq!(parts.len(), 382);
    assert_eq!(key_count, 30_728);
    assert_eq!(
        parts[0].expected_keys[0],
        "capture/2026-10-11T00:35:00.000Z/23/rain"
    );
    assert_eq!(
        parts
            .last()
            .expect("last part")
            .expected_keys
            .last()
            .map(String::as_str),
        Some("capture/2027-09-10T18:35:00.000Z/4/rain")
    );
    assert_eq!(
        reserved,
        "e6f63fd144bfcff5e11262a1b576a8d2e3b5cd9947cfcf2a6a025119d5e34528"
    );
}
