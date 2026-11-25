//! Node-golden coverage for fixed-station weighted median selection.

use super::fixed_gauge_weighted_median;

/// Preserve the ordinary three-near-station midpoint crossing in value order.
#[test]
fn returns_first_value_crossing_half_the_fixed_station_weight() {
    let mut values = [(0, 10.0), (1, 20.0), (2, 30.0)];

    assert_eq!(fixed_gauge_weighted_median(&mut values), Some(20.0));
}
