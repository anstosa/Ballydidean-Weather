use std::path::Path;
use std::process::Command;

use weather_contracts::JsonValue;
use weather_edge::ArchiveWirePrimitives;

use super::{
    FEATURE_COUNT, NativeMaintenanceRainFitError, NativeWirePrimitives, OrdinalModel,
    RainFitRequest, XgBoost, digest, encode_python_sorted_json, field, fit_control_reference_at,
    fit_monthly_rain, month_masks, object, run_native_rain_fit_document, string, text,
};

const HOST_LIBRARY: &str = "/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/lib/python3.14/site-packages/xgboost/lib/libxgboost.so";
const EXPECTED_HASHES: [&str; 4] = [
    "4ef79283b2e6299881cb1839264cf109d4b59a347302ed32e4a71b8ef26f902f",
    "bc4e6aa2617ee22f9070169218cbc2d447aae1c0eb9154ecb920c969e50418c0",
    "a9c1d27d11a3cec0cc9a0e8db8bcf8aba7c4d550fd4ebf44eb5248a0d53ff21c",
    "8de757281f9def82aa80db1c8a6e7e858a6635a85ec49be7190b9279c1b19428",
];
const EXPECTED_PREDICTIONS: [[f32; 3]; 4] = [
    [0.75, 0.75, 0.75],
    [0.5, 0.5, 0.5],
    [0.25, 0.25, 0.25],
    [1.6333333, 1.6333333, 1.6333333],
];

#[test]
fn invalid_contract_fails_before_loading_the_numerical_runtime() {
    let input = object(vec![(
        "contractVersion".to_owned(),
        string("rain-maintenance-fit-input/unknown"),
    )]);
    assert_eq!(
        run_native_rain_fit_document(input),
        Err(NativeMaintenanceRainFitError::Contract)
    );
}

#[test]
fn monthly_masks_retain_the_ninety_day_window_and_both_embargos() {
    let masks = month_masks("2026-12").expect("build monthly masks");
    assert_eq!(masks.calibration_stop - masks.calibration_start, 90 * 24);
    assert_eq!(masks.calibration_start - masks.fit_stop, 7 * 24);
    assert_eq!(masks.decision_start - masks.calibration_stop, 7 * 24);
}

#[test]
#[ignore = "requires pinned workstation numerical runtime"]
fn direct_c_api_reproduces_the_pinned_python_four_head_fit() {
    assert!(Path::new(HOST_LIBRARY).is_file());
    let xgboost = XgBoost::load_at(HOST_LIBRARY).expect("load pinned XGBoost C API");
    let hours = (24 * 200..24 * 260).map(i64::from).collect::<Vec<_>>();
    let actual = [0.0, 0.4, 1.5, 3.0]
        .into_iter()
        .cycle()
        .take(hours.len())
        .collect::<Vec<_>>();
    let features = vec![1.0_f32; hours.len() * FEATURE_COUNT];
    let model =
        OrdinalModel::fit(&xgboost, &features, &actual, &hours).expect("fit four native heads");
    // bind exact model JSON bytes to the original pinned Python wrapper result.
    for (index, expected) in EXPECTED_HASHES.iter().enumerate() {
        let bytes = model.heads[index]
            .bytes
            .as_deref()
            .expect("supported head bytes");
        assert_eq!(digest(bytes), *expected);
    }
    let mut synthetic = vec![0.0_f32; FEATURE_COUNT];
    synthetic.extend(std::iter::repeat_n(1.0_f32, FEATURE_COUNT));
    synthetic.extend(std::iter::repeat_n(f32::NAN, FEATURE_COUNT));
    let (probabilities, amount) = model
        .predict(&xgboost, &synthetic, 3)
        .expect("predict pinned parity rows");
    // preserve exact native f32 output from every occurrence head.
    for row in 0..3 {
        for head in 0..3 {
            assert_eq!(
                probabilities[row][head] as f32,
                EXPECTED_PREDICTIONS[head][row]
            );
        }
        assert_eq!(amount[row] as f32, EXPECTED_PREDICTIONS[3][row]);
    }
}

#[test]
#[ignore = "requires pinned workstation numerical runtime"]
fn control_reference_report_matches_the_complete_python_source_oracle() {
    assert!(Path::new(HOST_LIBRARY).is_file());
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("resolve repository root");
    let python = "/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/bin/python3";
    assert!(Path::new(python).is_file());
    let source = r#"
import base64,datetime as dt,json,tempfile
from pathlib import Path
import rain_refresh as refresh
from rain_wind_features import FEATURE_NAMES
stamp=lambda value:value.isoformat(timespec='milliseconds').replace('+00:00','Z')
rows=[];start=dt.datetime(2026,2,1,tzinfo=dt.timezone.utc)
for day in range(295):
 for hour in range(0,24,3):
  valid=start+dt.timedelta(days=day,hours=hour);run=valid-dt.timedelta(hours=9)
  rows.append({'key':stamp(valid),'validAt':stamp(valid),'runInitializedAt':stamp(run),
   'sourceReceiptAt':stamp(valid-dt.timedelta(hours=1)),'targetMaxReceiptAt':stamp(valid+dt.timedelta(hours=1)),
   'evidenceClass':'development','features':[1.]*107,'rawTargetHourTemperatureC':10.,
   'raw':.5,'actual':[0.,.4,1.5,3.][hour//3%4],'modelLeadHours':9,
   'operationalHorizonHours':1,'persistencePrediction':None,'gaugeCount':3,
   'sourceRowSha256':'a'*64,'targetRowSha256':'b'*64})
payload={'contractVersion':'rain-control-reference-fit-input/v1','month':'2026-12',
 'requestedAt':'2026-11-25T00:00:00.000Z','featureNames':list(FEATURE_NAMES),'trainingRows':rows}
with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
 result=refresh.fit_rain_control_reference(payload,Path(directory),
  clock=lambda:dt.datetime(2026,11,25,1,tzinfo=dt.timezone.utc))
raw=json.dumps(result,sort_keys=True,separators=(',',':'),allow_nan=False).encode()+b'\n'
print(json.dumps({'input':payload,'expectedRaw':base64.b64encode(raw).decode()},sort_keys=True,separators=(',',':')))
"#;
    let output = Command::new(python)
        .args(["-c", source])
        .current_dir(&root)
        .env("PYTHONPATH", root.join("scripts/research"))
        .env("OMP_NUM_THREADS", "1")
        .env("OPENBLAS_NUM_THREADS", "1")
        .env("MKL_NUM_THREADS", "1")
        .output()
        .expect("run source control oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let wire = NativeWirePrimitives;
    let envelope = wire
        .decode_json(&output.stdout)
        .expect("decode source oracle");
    let input = field(&envelope, "input").expect("source input");
    let expected = text(&envelope, "expectedRaw").expect("source raw output");
    let xgboost = XgBoost::load_at(HOST_LIBRARY).expect("load pinned XGBoost C API");
    let generated = wire
        .parse_explicit_utc_milliseconds("2026-11-25T01:00:00.000Z")
        .expect("fixed generation clock");
    let actual = fit_control_reference_at(input, &xgboost, &wire, Some(generated))
        .expect("fit native control reference");
    let actual = encode_python_sorted_json(&actual).expect("encode native report");
    let expected = wire
        .decode_standard_base64(&expected)
        .expect("decode source raw report");
    assert_eq!(actual, expected);
}

#[test]
#[ignore = "requires pinned workstation numerical runtime"]
fn future_only_v3_report_matches_the_complete_python_source_oracle() {
    assert!(Path::new(HOST_LIBRARY).is_file());
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("resolve repository root");
    let python = "/home/ubuntu/.weather/research-runtimes/xgboost-cpu-3.4.1/bin/python3";
    assert!(Path::new(python).is_file());
    let source = r#"
import base64,datetime as dt,json,tempfile
from pathlib import Path
import rain_refresh as refresh
from rain_wind_features import FEATURE_NAMES
stamp=lambda value:value.isoformat(timespec='milliseconds').replace('+00:00','Z')
training=[];training_start=dt.datetime(2024,3,14,tzinfo=dt.timezone.utc)
development_start=dt.datetime(2025,11,24,tzinfo=dt.timezone.utc)
development_end=dt.datetime(2026,11,24,tzinfo=dt.timezone.utc)
cursor=training_start
while cursor<development_start:
 run=cursor-dt.timedelta(hours=9);actual=[0.,.4,1.5,3.][(cursor.hour//3)%4]
 training.append({'key':'training/'+stamp(cursor),'validAt':stamp(cursor),'runInitializedAt':stamp(run),
  'sourceReceiptAt':stamp(cursor-dt.timedelta(hours=1)),'targetMaxReceiptAt':stamp(cursor),
  'evidenceClass':'development','features':[1.]*107,'rawTargetHourTemperatureC':10.,
  'raw':.5,'actual':actual,'modelLeadHours':9,'operationalHorizonHours':1,
  'persistencePrediction':None,'gaugeCount':12,'sourceRowSha256':'d'*64,'targetRowSha256':'e'*64})
 cursor+=dt.timedelta(hours=3)
# retain the complete annual proof while sampling low, middle, and high eligible leads
first=int((development_start.timestamp()/3600-31)//6*6);members=[];development=[]
eligible_leads={9,20,31}
for run_hour in range(first,int(development_end.timestamp()//3600)-9+1,6):
 run=dt.datetime.fromtimestamp(run_hour*3600,dt.timezone.utc);issued=run+dt.timedelta(hours=8)
 for lead in range(9,32):
  valid=run+dt.timedelta(hours=lead)
  if not development_start<=valid<development_end: continue
  key=stamp(run)+'/'+stamp(valid);actual=[0.,.4,1.5,3.][(valid.hour//3)%4]
  phase_eligible=lead in eligible_leads
  members.append({'issuedAt':stamp(issued),'key':key,'modelLeadHours':lead,
   'operationalHorizonHours':lead-8,'phaseEligible':phase_eligible,'sourceMemberSha256':'a'*64,
   'sourceReceiptSha256':'b'*64,'targetAvailable':True,'validAt':stamp(valid)})
  # materialize labels only for the source phase-eligible annual subset
  if not phase_eligible: continue
  development.append({'key':key,'validAt':stamp(valid),'runInitializedAt':stamp(run),
   'sourceReceiptAt':stamp(issued),'targetMaxReceiptAt':stamp(valid),'evidenceClass':'development',
   'features':[1.]*107,'rawTargetHourTemperatureC':10.,'raw':.5,'actual':actual,
   'modelLeadHours':lead,'operationalHorizonHours':lead-8,'persistencePrediction':None,
   'gaugeCount':12,'sourceRowSha256':'a'*64,'targetRowSha256':'c'*64})
assert len(members)==33580 and len(development)==4380
proof={'contractVersion':'rain-maintenance-development-population/v3','cycleHours':[0,6,12,18],
 'developmentEndAt':stamp(development_end),'developmentStartAt':stamp(development_start),
 'eligibleRowCount':len(development),'excludedColdRowCount':len(members)-len(development),'expectedRowCount':len(members),
 'missingSourceRowCount':0,'missingTargetRowCount':0,'observedRowCount':len(members),
 'operationalHorizonHours':list(range(1,24)),
 'populationMemberRootSha256':refresh.canonical_hash(sorted([item['sourceMemberSha256'] for item in members])),
 'populationReceiptRootSha256':refresh.canonical_hash(['b'*64]),
 'populationSha256':refresh.canonical_hash(members),'sourceModelLeadHours':list(range(9,32)),
 'sourcePopulation':members}
payload={'contractVersion':'rain-maintenance-fit-input/v3','month':'2026-12',
 'featureNames':list(FEATURE_NAMES),'trainingRows':training,'developmentRows':development,
 'developmentPopulation':proof}
source_input=json.loads(json.dumps(payload))
with tempfile.TemporaryDirectory(dir='/dev/shm') as directory:
 result=refresh.fit_monthly_rain(payload,Path(directory))
assert result['fitState']['supported'] is True
assert len(result['gridReports'])==7 and set(result['gridReports'])==set(refresh.GRID)
raw=json.dumps(result,sort_keys=True,separators=(',',':'),allow_nan=False).encode()+b'\n'
print(json.dumps({'input':source_input,'expectedRaw':base64.b64encode(raw).decode()},sort_keys=True,separators=(',',':')))
"#;
    let output = Command::new(python)
        .args(["-c", source])
        .current_dir(&root)
        .env("PYTHONPATH", root.join("scripts/research"))
        .env("OMP_NUM_THREADS", "1")
        .env("OPENBLAS_NUM_THREADS", "1")
        .env("MKL_NUM_THREADS", "1")
        .output()
        .expect("run source V3 oracle");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let wire = NativeWirePrimitives;
    let envelope = wire
        .decode_json(&output.stdout)
        .expect("decode source V3 oracle");
    let input = field(&envelope, "input").expect("source V3 input");
    let expected = text(&envelope, "expectedRaw").expect("source V3 raw output");
    let request = RainFitRequest::parse(input, &wire).expect("admit native V3 input");
    let xgboost = XgBoost::load_at(HOST_LIBRARY).expect("load pinned XGBoost C API");
    let actual = fit_monthly_rain(&request, &xgboost).expect("fit native V3 report");
    assert!(matches!(
        field(
            field(&actual, "fitState").expect("native fit state"),
            "supported"
        ),
        Some(JsonValue::Boolean(true))
    ));
    // require the native run to traverse every real development arm.
    let Some(JsonValue::Object(grid_reports)) = field(&actual, "gridReports") else {
        panic!("native grid reports");
    };
    assert_eq!(grid_reports.len(), 7);
    let actual = encode_python_sorted_json(&actual).expect("encode native V3 report");
    let expected = wire
        .decode_standard_base64(&expected)
        .expect("decode source raw V3 report");
    assert_eq!(actual, expected);
}
