use std::path::Path;
use std::process::Command;

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use crate::maintenance_canonical_json::{field, text};
use crate::native_maintenance_policy_comparison::{number, object, string};
use crate::{NativeWirePrimitives, run_native_temperature_fit_document};

const PYTHON: &str = "/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/bin/python3";

/// Preserve the exact no-support report while still fitting both frozen arms.
#[test]
fn native_temperature_fit_returns_closed_no_candidate() {
    let output = run_native_temperature_fit_document(object(vec![
        (
            "contractVersion",
            string("temperature-maintenance-fit-input/v2"),
        ),
        ("month", string("2026-10")),
        ("trainingRows", JsonValue::Array(Vec::new())),
        ("developmentRows", JsonValue::Array(Vec::new())),
        ("incumbentModel", incumbent()),
    ]))
    .expect("closed empty support remains a valid monthly attempt");
    let value = NativeWirePrimitives
        .decode_json(&output)
        .expect("native candidate bytes");
    assert_eq!(
        text(&value, "contractVersion").as_deref(),
        Some("temperature-maintenance-fit/v2")
    );
    assert_eq!(text(&value, "state").as_deref(), Some("no_candidate"));
    assert_eq!(field(&value, "selectedArm"), Some(&JsonValue::Null));
    assert_eq!(field(&value, "model"), Some(&JsonValue::Null));
    assert_eq!(field(&value, "developmentRawMae"), Some(&JsonValue::Null));
    let arms = field(&value, "arms").expect("fixed arms");
    // preserve both preregistered arm identities even without support
    for arm in ["month_start_expanding", "month_start_trailing365"] {
        let receipt = field(field(arms, arm).expect("arm"), "fitReceipt").expect("receipt");
        assert_eq!(
            field(receipt, "trainingRows"),
            Some(&JsonValue::Number(0.0))
        );
        assert_eq!(
            field(receipt, "trainingDates"),
            Some(&JsonValue::Number(0.0))
        );
    }
}

/// Compare one fully supported native fit against the pinned original numerical source.
#[test]
#[ignore = "requires the pinned predecessor NumPy runtime and compiled inference oracle"]
fn native_temperature_fit_matches_supported_python_oracle() {
    // explicit ignored oracle execution must never degrade into a skipped parity claim
    assert!(
        Path::new(PYTHON).is_file(),
        "pinned supported temperature oracle is absent"
    );
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let output = Command::new(PYTHON)
        .current_dir(&repository)
        .arg("-c")
        .arg(SUPPORTED_ORACLE)
        .env(
            "PYTHONPATH",
            repository.join("scripts/research").as_os_str(),
        )
        .env("BLIS_NUM_THREADS", "1")
        .env("MKL_NUM_THREADS", "1")
        .env("NUMEXPR_NUM_THREADS", "1")
        .env("OMP_NUM_THREADS", "1")
        .env("OPENBLAS_NUM_THREADS", "1")
        .env("VECLIB_MAXIMUM_THREADS", "1")
        .output()
        .expect("launch pinned supported temperature oracle");
    // expose bounded source diagnostics rather than treating an empty oracle as parity
    if !output.status.success() {
        panic!(
            "supported temperature oracle failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let oracle = NativeWirePrimitives
        .decode_json(&output.stdout)
        .expect("supported oracle document");
    let input = field(&oracle, "input").expect("oracle input").clone();
    let expected = field(&oracle, "output").expect("oracle output");
    let actual_bytes =
        run_native_temperature_fit_document(input).expect("native supported temperature fit");
    let actual = NativeWirePrimitives
        .decode_json(&actual_bytes)
        .expect("native supported candidate");
    compare_supported_output(&actual, expected);
    let mut expected_bytes = text(&oracle, "outputCanonical")
        .expect("source canonical candidate")
        .into_bytes();
    expected_bytes.push(b'\n');
    let byte_exact = actual_bytes == expected_bytes;
    let expected_hash = arm_model_hash(expected, "month_start_expanding");
    let actual_hash = arm_model_hash(&actual, "month_start_expanding");
    let maximum_drift = maximum_model_coefficient_drift(&actual, expected);
    let mismatch = first_byte_mismatch(&actual_bytes, &expected_bytes);
    eprintln!(
        "supported temperature differential: byte_exact={byte_exact} model_hash_exact={} maximum_coefficient_drift={maximum_drift:.17e} mismatch={mismatch}",
        actual_hash == expected_hash
    );
    eprintln!(
        "metrics native=({:?},{:?},{:?}) oracle=({:?},{:?},{:?})",
        number(&actual, "developmentRawMae"),
        number(&actual, "developmentIncumbentMae"),
        number(
            field(field(&actual, "arms").unwrap(), "month_start_expanding").unwrap(),
            "developmentMae"
        ),
        number(expected, "developmentRawMae"),
        number(expected, "developmentIncumbentMae"),
        number(
            field(field(expected, "arms").unwrap(), "month_start_expanding").unwrap(),
            "developmentMae"
        )
    );
    assert_eq!(actual_hash, expected_hash, "candidate model hash parity");
    assert!(byte_exact, "candidate byte parity differs: {mismatch}");
}

/// Describe the first source-byte mismatch without retaining private fixture rows.
fn first_byte_mismatch(actual: &[u8], expected: &[u8]) -> String {
    let index = actual
        .iter()
        .zip(expected)
        .position(|(actual, expected)| actual != expected)
        .unwrap_or(actual.len().min(expected.len()));
    let start = index.saturating_sub(48);
    let actual_end = (index + 96).min(actual.len());
    let expected_end = (index + 96).min(expected.len());
    format!(
        "index={index} actual={} expected={}",
        String::from_utf8_lossy(&actual[start..actual_end]),
        String::from_utf8_lossy(&expected[start..expected_end])
    )
}

/// Compare every stable report field and tolerate only last-bit dense-kernel coefficient drift.
fn compare_supported_output(actual: &JsonValue, expected: &JsonValue) {
    for key in [
        "contractVersion",
        "dueMonth",
        "state",
        "selectedArm",
        "confirmationOpened",
        "servingChanged",
    ] {
        assert_eq!(field(actual, key), field(expected, key), "field {key}");
    }
    for key in ["developmentRawMae", "developmentIncumbentMae"] {
        assert_close(number(actual, key), number(expected, key), 1e-10, key);
    }
    let actual_arms = field(actual, "arms").expect("native arms");
    let expected_arms = field(expected, "arms").expect("oracle arms");
    // require both preregistered arms and every non-floating receipt identity
    for arm in ["month_start_expanding", "month_start_trailing365"] {
        let actual_arm = field(actual_arms, arm).expect("native arm");
        let expected_arm = field(expected_arms, arm).expect("oracle arm");
        for key in [
            "developmentDates",
            "developmentRows",
            "fallbackCount",
            "eligible",
        ] {
            assert_eq!(
                field(actual_arm, key),
                field(expected_arm, key),
                "arm {arm} field {key}"
            );
        }
        assert_close(
            number(actual_arm, "developmentMae"),
            number(expected_arm, "developmentMae"),
            1e-10,
            "developmentMae",
        );
        let actual_receipt = field(actual_arm, "fitReceipt").expect("native receipt");
        let expected_receipt = field(expected_arm, "fitReceipt").expect("oracle receipt");
        for key in [
            "arm",
            "month",
            "trainingRows",
            "trainingDates",
            "trainingCutoffUtc",
            "latestTrainingValidAt",
            "trainingKeySha256",
            "modelSha256",
            "exclusions",
            "strengthPolicy",
            "originalStrengthBandReceipts",
            "strengthRefitted",
            "servingChanged",
        ] {
            assert_eq!(
                field(actual_receipt, key),
                field(expected_receipt, key),
                "arm {arm} receipt {key}"
            );
        }
        assert_eq!(
            text(actual_receipt, "modelSha256").map(|hash| hash.len()),
            Some(64),
            "native model hash shape"
        );
    }
    compare_selected_model(
        field(actual, "model").expect("native model"),
        field(expected, "model").expect("oracle model"),
    );
}

/// Compare the selected permanent model and both fitted coefficient vectors.
fn compare_selected_model(actual: &JsonValue, expected: &JsonValue) {
    for key in [
        "contractVersion",
        "cohort",
        "scope",
        "supported",
        "trainingCutoffUtc",
        "learnedStrengthContractVersion",
        "strengthBands",
        "effectiveFrom",
        "latestTrainingValidAt",
    ] {
        assert_eq!(field(actual, key), field(expected, key), "model {key}");
    }
    for key in ["directCoefficients", "adaptiveCoefficients"] {
        let JsonValue::Array(actual_values) = field(actual, key).expect("native coefficients")
        else {
            panic!("native coefficient vector is absent");
        };
        let JsonValue::Array(expected_values) = field(expected, key).expect("oracle coefficients")
        else {
            panic!("oracle coefficient vector is absent");
        };
        assert_eq!(
            actual_values.len(),
            expected_values.len(),
            "model {key} width"
        );
        // retain source semantics while allowing only dense-kernel rounding differences
        for (index, (actual, expected)) in actual_values.iter().zip(expected_values).enumerate() {
            let (JsonValue::Number(actual), JsonValue::Number(expected)) = (actual, expected)
            else {
                panic!("non-numeric model coefficient");
            };
            assert!(
                (actual - expected).abs() <= 1e-9,
                "model {key}[{index}] differs: native={actual:.17e} oracle={expected:.17e}"
            );
        }
    }
}

/// Return one arm's candidate model identity for explicit byte-parity diagnostics.
fn arm_model_hash(value: &JsonValue, arm: &str) -> String {
    field(value, "arms")
        .and_then(|arms| field(arms, arm))
        .and_then(|arm| field(arm, "fitReceipt"))
        .and_then(|receipt| text(receipt, "modelSha256"))
        .expect("arm model hash")
}

/// Find the largest absolute selected-model coefficient difference.
fn maximum_model_coefficient_drift(actual: &JsonValue, expected: &JsonValue) -> f64 {
    let actual = field(actual, "model").expect("native selected model");
    let expected = field(expected, "model").expect("oracle selected model");
    ["directCoefficients", "adaptiveCoefficients"]
        .into_iter()
        .flat_map(|key| {
            let JsonValue::Array(actual) = field(actual, key).expect("native coefficients") else {
                panic!("native coefficients absent");
            };
            let JsonValue::Array(expected) = field(expected, key).expect("oracle coefficients")
            else {
                panic!("oracle coefficients absent");
            };
            actual.iter().zip(expected).map(|(actual, expected)| {
                let (JsonValue::Number(actual), JsonValue::Number(expected)) = (actual, expected)
                else {
                    panic!("non-numeric coefficient");
                };
                (actual - expected).abs()
            })
        })
        .fold(0.0, f64::max)
}

/// Compare one optional source metric under an explicit absolute tolerance.
fn assert_close(actual: Option<f64>, expected: Option<f64>, tolerance: f64, label: &str) {
    match (actual, expected) {
        (Some(actual), Some(expected)) => assert!(
            (actual - expected).abs() <= tolerance,
            "{label} differs: native={actual:.17e} oracle={expected:.17e}"
        ),
        (None, None) => {}
        _ => panic!("{label} support differs"),
    }
}

/// Build the minimum genuine incumbent strength material required by every fit receipt.
fn incumbent() -> JsonValue {
    let band = || {
        object(vec![
            ("alpha", JsonValue::Number(1.0)),
            ("supported", JsonValue::Boolean(true)),
            ("trainingCutoffUtc", string("2026-08-25T07:00:00.000Z")),
        ])
    };
    object(vec![
        (
            "learnedStrengthContractVersion",
            string("temperature-winner-extensions-research/v1"),
        ),
        (
            "strengthBands",
            object(vec![("1-6", band()), ("7-12", band())]),
        ),
    ])
}

const SUPPORTED_ORACLE: &str = r#"
import datetime as dt
import json
import math
import temperature_refresh as refresh

incumbent=json.loads(refresh.INCUMBENT_PATH.read_text())['model']

def rows(start,days):
    output=[]
    for day in range(days):
        initialized=start+dt.timedelta(days=day)
        for lead in range(7,19):
            valid=initialized+dt.timedelta(hours=lead)
            stamp=refresh.format_instant
            row={'key':stamp(initialized)+':'+str(lead),
                 'cohort':'ecmwf_single_run_hindcast',
                 'runInitializedAt':stamp(initialized),'validAt':stamp(valid),
                 'modelCycle':'50r1' if initialized>=dt.datetime(2026,5,12,6,tzinfo=dt.timezone.utc) else '49r1',
                 'modelLeadHours':lead,'operationalHorizonHours':lead-6,
                 'rawTemperatureC':10+math.sin(day/30),
                 'actualTemperatureC':9+math.sin(day/30),
                 'rawRelativeHumidityPercent':80,'rawWindSpeedMps':3,
                 'evidenceClass':'development',
                 'sourceReceiptAt':stamp(initialized+dt.timedelta(hours=6)),
                 'targetMaxReceiptAt':stamp(valid+dt.timedelta(hours=7))}
            row['state']={'cohort':'ecmwf_single_run_hindcast','supported':False,
                          'n24':0,'n72':0,'localDates':0,'b24C':None,'b72C':None,
                          'mad72C':None,'sourceKeys':[],
                          'maximumSourceRunInitializedAt':None,'maximumSourceValidAt':None,
                          'targetRunInitializedAt':stamp(initialized),
                          'windowEndValidAt':stamp(initialized-dt.timedelta(hours=7))}
            output.append(row)
    return output

payload={'contractVersion':'temperature-maintenance-fit-input/v2','month':'2026-10',
         'trainingRows':rows(dt.datetime(2025,11,1,tzinfo=dt.timezone.utc),120),
         'developmentRows':rows(dt.datetime(2026,4,1,tzinfo=dt.timezone.utc),90),
         'incumbentModel':incumbent}
result=refresh.fit_monthly_temperature(payload)
print(json.dumps({'input':payload,'output':result,
                  'outputCanonical':refresh.canonical(result).decode()},
                 sort_keys=True,separators=(',',':'),allow_nan=False))
"#;
