use crate::forecast_scorecard_fields::{
    ScorecardContractError, member, require_array, require_count, require_equal, require_keys,
    require_nullable_finite, require_object,
};
use weather_contracts::JsonValue;

// validate aggregate rain diagnostics
pub(crate) fn validate_rain_diagnostics(
    value: &JsonValue,
    path: &str,
) -> Result<(), ScorecardContractError> {
    require_object(value, path)?;
    require_keys(
        value,
        &[
            "accumulations",
            "annualBalancedVolumeRatio",
            "heavyAdjustedMae",
            "heavyRawMae",
            "probabilityOrderViolationCount",
            "thresholds",
            "wetAdjustedMae",
            "wetRawMae",
            "winterBalancedVolumeRatio",
        ],
        path,
    )?;
    require_nullable_finite(
        member(value, "annualBalancedVolumeRatio"),
        &format!("{path}.annualBalancedVolumeRatio"),
    )?;
    require_nullable_finite(
        member(value, "winterBalancedVolumeRatio"),
        &format!("{path}.winterBalancedVolumeRatio"),
    )?;
    require_nullable_finite(member(value, "wetRawMae"), &format!("{path}.wetRawMae"))?;
    require_nullable_finite(
        member(value, "wetAdjustedMae"),
        &format!("{path}.wetAdjustedMae"),
    )?;
    require_nullable_finite(member(value, "heavyRawMae"), &format!("{path}.heavyRawMae"))?;
    require_nullable_finite(
        member(value, "heavyAdjustedMae"),
        &format!("{path}.heavyAdjustedMae"),
    )?;

    // reject negative volumes and errors
    for key in [
        "annualBalancedVolumeRatio",
        "winterBalancedVolumeRatio",
        "wetRawMae",
        "wetAdjustedMae",
        "heavyRawMae",
        "heavyAdjustedMae",
    ] {
        // retain nonnegative nullable values
        if matches!(member(value, key), JsonValue::Number(number) if *number < 0.0) {
            return Err(ScorecardContractError::new(format!(
                "{path}.{key} must be nonnegative"
            )));
        }
    }
    require_count(
        member(value, "probabilityOrderViolationCount"),
        &format!("{path}.probabilityOrderViolationCount"),
    )?;
    let thresholds = require_array(
        member(value, "thresholds"),
        3,
        &format!("{path}.thresholds"),
        Some(3),
    )?;
    let accumulations = require_array(
        member(value, "accumulations"),
        3,
        &format!("{path}.accumulations"),
        Some(3),
    )?;

    // keep threshold order fixed
    for (index, (threshold, expected_threshold)) in
        thresholds.iter().zip([0.1_f64, 1.0, 2.5]).enumerate()
    {
        validate_rain_threshold(
            threshold,
            expected_threshold,
            &format!("{path}.thresholds[{index}]"),
        )?;
    }

    // keep accumulation windows fixed
    for (index, (accumulation, expected_hours)) in
        accumulations.iter().zip([6.0_f64, 12.0, 23.0]).enumerate()
    {
        validate_rain_accumulation(
            accumulation,
            expected_hours,
            &format!("{path}.accumulations[{index}]"),
        )?;
    }

    Ok(())
}

// validate one fixed rain threshold
fn validate_rain_threshold(
    value: &JsonValue,
    expected_threshold: f64,
    path: &str,
) -> Result<(), ScorecardContractError> {
    require_object(value, path)?;
    require_keys(
        value,
        &[
            "adjustedBrier",
            "csi",
            "falseAlarms",
            "far",
            "hits",
            "misses",
            "pod",
            "rawBrier",
            "reliability",
            "thresholdMmPerHour",
        ],
        path,
    )?;
    require_equal(
        member(value, "thresholdMmPerHour"),
        &JsonValue::Number(expected_threshold),
        &format!("{path}.thresholdMmPerHour"),
    )?;
    require_nullable_finite(member(value, "rawBrier"), &format!("{path}.rawBrier"))?;
    require_nullable_finite(
        member(value, "adjustedBrier"),
        &format!("{path}.adjustedBrier"),
    )?;
    require_count(member(value, "hits"), &format!("{path}.hits"))?;
    require_count(member(value, "misses"), &format!("{path}.misses"))?;
    require_count(member(value, "falseAlarms"), &format!("{path}.falseAlarms"))?;
    require_nullable_finite(member(value, "pod"), &format!("{path}.pod"))?;
    require_nullable_finite(member(value, "far"), &format!("{path}.far"))?;
    require_nullable_finite(member(value, "csi"), &format!("{path}.csi"))?;

    // retain scores in their physical range
    for key in ["rawBrier", "adjustedBrier", "pod", "far", "csi"] {
        // reject scores outside the unit interval
        if matches!(member(value, key), JsonValue::Number(number) if *number < 0.0 || *number > 1.0)
        {
            return Err(ScorecardContractError::new(format!(
                "{path}.{key} is outside 0..1"
            )));
        }
    }
    let reliability = require_array(
        member(value, "reliability"),
        10,
        &format!("{path}.reliability"),
        Some(10),
    )?;

    // preserve empty reliability deciles
    for (index, bin) in reliability.iter().enumerate() {
        let bin_path = format!("{path}.reliability[{index}]");
        require_object(bin, &bin_path)?;
        require_keys(
            bin,
            &["count", "meanProbability", "observedFrequency"],
            &bin_path,
        )?;
        require_count(member(bin, "count"), &format!("{bin_path}.count"))?;
        require_nullable_finite(
            member(bin, "meanProbability"),
            &format!("{bin_path}.meanProbability"),
        )?;
        require_nullable_finite(
            member(bin, "observedFrequency"),
            &format!("{bin_path}.observedFrequency"),
        )?;

        // retain calibrated probabilities
        for key in ["meanProbability", "observedFrequency"] {
            // reject probabilities outside the unit interval
            if matches!(member(bin, key), JsonValue::Number(number) if *number < 0.0 || *number > 1.0)
            {
                return Err(ScorecardContractError::new(format!(
                    "{bin_path}.{key} is outside 0..1"
                )));
            }
        }
    }

    Ok(())
}

// validate one fixed accumulation window
fn validate_rain_accumulation(
    value: &JsonValue,
    expected_hours: f64,
    path: &str,
) -> Result<(), ScorecardContractError> {
    require_object(value, path)?;
    require_keys(
        value,
        &["adjustedMae", "completeWindows", "hours", "rawMae"],
        path,
    )?;
    require_equal(
        member(value, "hours"),
        &JsonValue::Number(expected_hours),
        &format!("{path}.hours"),
    )?;
    require_count(
        member(value, "completeWindows"),
        &format!("{path}.completeWindows"),
    )?;
    require_nullable_finite(member(value, "rawMae"), &format!("{path}.rawMae"))?;
    require_nullable_finite(member(value, "adjustedMae"), &format!("{path}.adjustedMae"))?;

    // reject negative accumulation errors
    if matches!(member(value, "rawMae"), JsonValue::Number(number) if *number < 0.0)
        || matches!(member(value, "adjustedMae"), JsonValue::Number(number) if *number < 0.0)
    {
        return Err(ScorecardContractError::new(format!(
            "{path} MAE must be nonnegative"
        )));
    }

    Ok(())
}
