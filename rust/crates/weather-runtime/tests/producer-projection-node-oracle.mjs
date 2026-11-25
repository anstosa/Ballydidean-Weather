import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
  parseAdjustmentRevisionProjectionDocument,
  parseAdjustmentRainFixedGaugeTargetProjection,
} from "../../../../apps/web/src/adjustment-maintenance-contract.mjs";
import { AdjustmentRevisionArchiveStore } from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const HASH = "a".repeat(64);
const SECOND = "b".repeat(64);
const RUN = "2026-10-08T00:00:00.000Z";
const VALID = "2026-10-08T01:00:00.000Z";
const ACK_TIME = "2026-10-08T03:00:00.000Z";

// use the exact retained sorted object spelling for the fixture input
function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])]));
  }
  return value;
}
function bytes(value) { return Buffer.from(JSON.stringify(canonical(value))+"\n"); }
function sha(value) { return createHash("sha256").update(value).digest("hex"); }
function f64(value) { const encoded=Buffer.alloc(8); encoded.writeDoubleBE(value); return encoded.toString("hex"); }
function hour(index) { return new Date(Date.parse(RUN)+index*3_600_000).toISOString(); }

// construct the original complete weather comparator lineage
function source() {
  return {
    adapterVersion:"open-meteo/v4",contractEpoch:"forecast/v4",dataset:"best_match",
    providerKey:"open-meteo",sourceConfigFingerprint:"open-meteo-forecast/v4",sourceId:"7",
    sourceKey:"open-meteo-forecast",sourceKind:"forecast",upstreamModel:"best_match",
  };
}

// represent all canonical weather metrics, including explicit nulls
function weatherRow(validAt=VALID) {
  return {
    apparentTemperatureC64:f64(10),blackGlobeTemperatureC64:null,cloudCoverPercent64:f64(50),
    contentSha256:HASH,pm25MicrogramsPerCubicMeter64:null,precipitationMm64:f64(0),
    precipitationRateMmPerHour64:null,pressureHpa64:f64(1010),relativeHumidityPercent64:f64(80),
    soilElectricalConductivityMicrosiemensPerCm64:null,soilMoisturePercent64:null,solarRadiationWm264:null,
    temperatureC64:f64(11),uvIndex64:null,validAt,waterLevelM64:null,wetBulbGlobeTemperatureC64:null,
    windDirectionDegrees64:f64(180),windGustMps64:f64(8),windSpeedMps64:f64(5),
  };
}

// reproduce all five legacy source classes and the two additive weather representations
function legacyFixtures() {
  const actual={
    contractVersion:"adjustment-revision-projection/v1",family:"shared",
    logicalKey:{productRunAt:RUN,sourceId:"7",sourceKind:"forecast",validAt:VALID},
    logicalReceivedAt:"2026-10-08T00:05:00.000Z",projectionKind:"actual_best_match",
    rows:[weatherRow()],source:source(),storedContentSha256:HASH,
  };
  const target=structuredClone(actual);
  Object.assign(target.logicalKey,{productRunAt:null,sourceKind:"physical_sensor"});
  target.projectionKind="target_revision";
  Object.assign(target.source,{sourceKind:"physical_sensor",upstreamModel:null});
  const anchor={
    ...actual,projectionKind:"native_source",
    logicalKey:{contentSha256:null,leadHours:24,providerResponseSha256:null,runInitializedAt:null,siteId:null,sourceId:"7",sourceType:"forecast_anchor",validAt:VALID},
    rows:[{apparentTemperatureC64:f64(10),cloudCoverPercent64:f64(50),contentSha256:HASH,leadHours:24,
      precipitationMm64:f64(0),pressureHpa64:f64(1010),relativeHumidityPercent64:f64(80),
      temperatureC64:f64(11),validAt:VALID,windDirectionDegrees64:f64(180),windGustMps64:f64(8),windSpeedMps64:f64(5)}],
  };
  const temperature={
    ...actual,family:"temperature",projectionKind:"native_source",
    logicalKey:{contentSha256:HASH,leadHours:null,providerResponseSha256:SECOND,runInitializedAt:RUN,siteId:"3",sourceId:null,sourceType:"ecmwf_temperature_run",validAt:null},
    rows:Array.from({length:18},(_,index)=>({
      bestMatchContentSha256:index<6?null:SECOND,bestMatchProductRunAt:index<6?null:RUN,
      bestMatchSourceId:index<6?null:"7",bestMatchTemperatureC64:index<6?null:f64(10),
      contentSha256:sha(`temperature-${index}`),modelCycle:"50r1",modelLeadHours:index+1,
      rawRelativeHumidityPercent64:f64(80),rawTemperatureC64:f64(11),rawWindSpeedMps64:f64(5),validAt:hour(index+1),
    })),
    source:{...source(),adapterVersion:"open-meteo-ecmwf-single-run/v1",contractEpoch:"temperature-canary/v1",dataset:"single_run",upstreamModel:"ecmwf_ifs"},
  };
  const rain={
    ...actual,family:"rain",projectionKind:"rain_gate_input",
    logicalKey:{inputSha256:HASH,modelSha256:SECOND,runInitializedAt:RUN},
    rows:Array.from({length:23},(_,index)=>({applied:index%2===0,correctedPrecipitationMm64:f64(1),
      modelLeadHours:index+1,rawPrecipitationMm64:f64(2),reasonCode:"candidate_applied",validAt:hour(index+1)})),
    source:Object.fromEntries(Object.keys(source()).map(key=>[key,null])),
  };
  const grouped={...actual,contractVersion:"adjustment-revision-batch-projection/v2",family:"wind",rows:[weatherRow(),weatherRow(hour(2))]};
  const groupedTarget={...target,contractVersion:"adjustment-revision-batch-projection/v2",family:"shared",
    rows:[weatherRow(),weatherRow("2026-10-08T01:05:00.000Z")]};
  const state={
    b24C:0.2,b72C:0.1,cohort:"ecmwf_single_run_hindcast",localDates:3,mad72C:0.5,
    maximumSourceRunInitializedAt:"2026-10-07T12:00:00.000Z",maximumSourceValidAt:"2026-10-07T17:00:00.000Z",
    n24:6,n72:24,sourceKeys:Array.from({length:24},(_,index)=>`2026-10-07T00:00:00.000Z/${index}`),
    supported:true,targetRunInitializedAt:RUN,windowEndValidAt:"2026-10-07T17:00:00.000Z",
  };
  const tempV2={...temperature,contractVersion:"adjustment-temperature-native-source-projection/v2",recentErrorState:state,recentErrorStateSha256:sha(JSON.stringify(canonical(state)))};
  return {actual,target,anchor,temperature,rain,grouped,groupedTarget,tempV2};
}

// use the original reviewed station order and distinct physical identities
const STATIONS=[
  [64255,175727,"ST-00054713"],[225947,1239187,"ST-00212830"],[38270,115866,"ST-00157152"],
  [168853,401592,"ST-00170845"],[126537,313016,"ST-00134621"],[201058,466938,"ST-00194085"],
  [203055,470937,"ST-00198967"],[66270,180230,"ST-00173167"],[34768,107388,"ST-00020495"],
  [88159,230560,"ST-00094734"],[126197,312302,"ST-00129187"],[27140,87271,"ST-00000360"],
];

// construct the original pre-fit rain source and causal control representations
function rainFixtures() {
  const logicalKey={inputSha256:HASH,modelSha256:SECOND,runInitializedAt:RUN};
  const featureRows=Array.from({length:23},(_,index)=>({
    features64:Array.from({length:107},(_,feature)=>feature===17?null:f64(feature===5?0.5:index)),
    modelLeadHours:index+9,rawPrecipitationMm64:f64(0.5),rawTargetHourTemperatureC64:f64(10),validAt:hour(index+9),
  }));
  const rainSource={
    adapterVersion:"rain-hurdle-wind-features/v1",contractEpoch:"rain-prospective-capture/v1",dataset:"ecmwf_ifs",
    providerKey:"open-meteo-single-runs",sourceConfigFingerprint:"96e4365f74a0fb8694944172b73a65c29d89a68ddf86b1b0a50db38bb39d52aa",
    sourceId:"forecast-claim-1",sourceKey:"rain-prospective-forecast",sourceKind:"forecast",upstreamModel:"ecmwf_ifs",
  };
  const feature={
    contractVersion:"adjustment-rain-gate-feature-projection/v2",family:"rain",logicalKey,
    logicalReceivedAt:"2026-10-08T08:05:00.000Z",projectionKind:"rain_gate_input",rows:featureRows,
    source:rainSource,storedContentSha256:sha("stored-rain-jsonb"),
  };
  const unsigned={
    contractVersion:"rain-maintenance-persistence-target/v1",decisionAt:"2026-10-08T08:00:00.000Z",
    prediction64:f64(0.75),reason:"causal_target",rowCount:12,
    rows:STATIONS.map(([stationId],index)=>({
      captureMembers:[{bodySha256:sha(`capture-${index}`),claimId:`station-${stationId}`,completedAt:"2026-10-08T07:05:00.000Z"}],
      precipitationMm64:f64(0.75),receivedAt:"2026-10-08T07:05:00.000Z",stationId,
    })),validAt:"2026-10-08T07:00:00.000Z",
  };
  const persistenceTarget={...unsigned,targetMemberSha256:sha(bytes(unsigned))};
  const controlRows=featureRows.map(row=>({
    ...row,incumbentArtifactIdentitySha256:HASH,incumbentPrediction64:f64(0.6),
    incumbentProbability:{atLeast0_1:f64(0.7),atLeast1_0:f64(0.2),atLeast2_5:f64(0.1)},
    incumbentReceiptMemberSha256:SECOND,nativeSourceProbability:{atLeast0_1:f64(1),atLeast1_0:f64(0),atLeast2_5:f64(0)},
    persistencePrediction64:f64(0.75),persistenceReason:"causal_target",persistenceTargetMemberSha256:persistenceTarget.targetMemberSha256,
    recentVolumeScalePrediction64:f64(0.8),sameWindowVolumeScalePrediction64:f64(0.7),
    sourceRowSha256:sha(Buffer.concat([Buffer.from("adjustment-rain-gate-feature-row/v2\n"),bytes(logicalKey),bytes(row)])),
    unchangedOrdinalPrediction64:f64(0.6),volumeScalePrediction64:f64(0.625),
  }));
  const control={
    ...feature,contractVersion:"adjustment-rain-gate-control-projection/v3",ordinalArtifactSha256:"c".repeat(64),
    persistenceTarget,rows:controlRows,stateSha256:"d".repeat(64),stateStageReceiptSha256:"e".repeat(64),
    storedContentSha256:"f".repeat(64),
  };
  // ask the exact installed parser which varied station value is the accepted weighted median
  const variedRows=unsigned.rows.map((row,index)=>({...row,precipitationMm64:f64((index+1)/10)}));
  let controlVaried=null;
  for (const candidate of variedRows) {
    const targetUnsigned={...unsigned,rows:variedRows,prediction64:candidate.precipitationMm64};
    const target={...targetUnsigned,targetMemberSha256:sha(bytes(targetUnsigned))};
    const varied={...control,persistenceTarget:target,rows:controlRows.map(row=>({
      ...row,persistencePrediction64:target.prediction64,persistenceTargetMemberSha256:target.targetMemberSha256,
    }))};
    try { parseProjection(bytes(varied)); controlVaried=varied; break; } catch {}
  }
  if (controlVaried===null) throw new Error("original varied station median absent");
  // retain the exact unavailable member and deterministic raw fallback
  const fallbackUnsigned={...unsigned,prediction64:null,reason:"raw_fallback_unavailable",
    rows:unsigned.rows.map(row=>({...row,captureMembers:[],precipitationMm64:null,receivedAt:null}))};
  const fallbackTarget={...fallbackUnsigned,targetMemberSha256:sha(bytes(fallbackUnsigned))};
  const controlFallback={...control,persistenceTarget:fallbackTarget,rows:controlRows.map(row=>({
    ...row,persistencePrediction64:row.rawPrecipitationMm64,persistenceReason:"raw_fallback_unavailable",persistenceTargetMemberSha256:null,
  }))};
  return {feature,control,controlVaried,controlFallback};
}

// construct one replayable fixed twelve-gauge target with complete and missing intervals
function fixedFixture() {
  const completedAt="2026-10-08T01:05:00.000Z";
  const body=Buffer.from('{"fixture":"original raw capture member"}');
  const bodySha256=sha(body);
  const claimIdFor=id=>`station-${id}`;
  const claims=STATIONS.map(([stationId])=>({
    claimId:claimIdFor(stationId),completedAt,stationId,windowStart:"2026-10-08T00:00:00.000Z",windowEndExclusive:"2026-10-08T02:00:00.000Z",
  })).sort((a,b)=>a.claimId<b.claimId?-1:a.claimId>b.claimId?1:0);
  const metricNames=["apparentTemperatureC","blackGlobeTemperatureC","cloudCoverPercent","pm25MicrogramsPerCubicMeter","precipitationMm","precipitationRateMmPerHour","pressureHpa","relativeHumidityPercent","soilElectricalConductivityMicrosiemensPerCm","soilMoisturePercent","solarRadiationWm2","temperatureC","uvIndex","waterLevelM","wetBulbGlobeTemperatureC","windDirectionDegrees","windGustMps","windSpeedMps"];
  const rows=STATIONS.map(([stationId,device_id,serial],index)=>{
    const complete=index===0;
    const claimId=claimIdFor(stationId);
    const captureMembers=[{bodySha256,claimId,completedAt}];
    const intervals=complete?Array.from({length:12},(_,i)=>({
      bodySha256,claimId,completedAt,durationMinutes:5,precipitationMm64:f64(0.5),temperatureC64:f64(10),
      validAt:new Date(Date.parse(RUN)+(i+1)*5*60_000).toISOString(),
    })):[];
    const metrics=Object.fromEntries(metricNames.map(name=>[name,null]));
    if (complete) Object.assign(metrics,{precipitationMm:6,precipitationRateMmPerHour:6,temperatureC:10});
    const metadata={
      device:{model:"Tempest",serial,vendor:"WeatherFlow"},model:null,
      provider:{dataset:"rain_fixed_gauge_hourly",device_id,location_id:stationId,report_interval_minutes:60},
      quality:{sampling:"backward_exact_interval_tiling",status:complete?"complete_interval_coverage":"incomplete_interval_coverage"},
      upstreamTimezone:"America/Los_Angeles",
    };
    const normalizedRecord={metadata,metrics,productRunAt:null,receivedAt:completedAt,sourceId:String(stationId),sourceKind:"physical_sensor",validAt:VALID};
    const {receivedAt,...content}=normalizedRecord;
    return {captureMembers,intervals,normalizedRecord,stationId,storedContentSha256:sha(JSON.stringify(canonical(content)))};
  });
  return {captureBodies:[{bodyBase64:body.toString("base64"),bodySha256,claims}],contractVersion:"adjustment-rain-fixed-gauge-target-projection/v1",
    family:"rain",logicalReceivedAt:completedAt,projectionKind:"target_revision",rows,validAt:VALID};
}

// route through the exact installed Node producer parser, not the relaxed archive fixture parser
function parseProjection(raw) {
  const version=JSON.parse(raw.toString("utf8")).contractVersion;
  return version==="adjustment-rain-fixed-gauge-target-projection/v1"
    ?parseAdjustmentRainFixedGaugeTargetProjection(raw):parseAdjustmentRevisionProjectionDocument(raw);
}

const parent=process.argv[2];
const fixed=fixedFixture();
const fixedOffset={...fixed,rows:fixed.rows.map(row=>({...row,normalizedRecord:{...row.normalizedRecord,
  validAt:"2026-10-07T17:00:00.000-08:00",receivedAt:"2026-10-07T17:05:00.000-08:00"}}))};
const valid={...legacyFixtures(),...rainFixtures(),fixed,fixedOffset};
const invalid={
  short_native:{...valid.temperature,rows:valid.temperature.rows.slice(0,17)},
  wrong_group_source:{...valid.grouped,source:{...valid.grouped.source,sourceId:"8"}},
  wrong_state_digest:{...valid.tempV2,recentErrorStateSha256:HASH},
  incomplete_features:{...valid.feature,rows:valid.feature.rows.slice(0,22)},
  wrong_control_amount:{...valid.control,rows:valid.control.rows.map((r,i)=>i===0?{...r,persistencePrediction64:f64(0.1)}:r)},
  wrong_fixed_amount:{...valid.fixed,rows:valid.fixed.rows.map((r,i)=>i===0?{...r,normalizedRecord:{...r.normalizedRecord,metrics:{...r.normalizedRecord.metrics,precipitationMm:7}}}:r)},
};
const entries=[];
for (const [name,value] of Object.entries({...valid,...invalid})) {
  const raw=bytes(value);
  let accepted=false;
  try {parseProjection(raw);accepted=true;} catch(error) {
    if (Object.hasOwn(valid,name)) throw new Error(`valid producer fixture ${name} rejected: ${error.message}`);
  }
  if (accepted!==Object.hasOwn(valid,name)) throw new Error(`invalid producer fixture ${name} admitted`);
  const file=`${name}.json`;
  await writeFile(join(parent,file),raw);
  entries.push([file,accepted?"yes":"no",value.projectionKind,
    ["adjustment-revision-batch-projection/v2","adjustment-rain-fixed-gauge-target-projection/v1"].includes(value.contractVersion)?String(value.rows.length):"single",
    value.contractVersion==="adjustment-rain-gate-control-projection/v3"?value.stateSha256:"none"].join("\t"));
}
await writeFile(join(parent,"cases.tsv"),entries.join("\n")+"\n");

// exercise actual native-stage counterparts against the original Node producer store
for (const [rootName,names] of [["ordinary",["actual","grouped"]],["gauge",["fixed"]]]) {
  const root=join(parent,`node-${rootName}`);
  await mkdir(root,{mode:0o700});
  const store=new AdjustmentRevisionArchiveStore({root,parseProjection,now:()=>new Date(ACK_TIME)});
  for (const name of names) {
    const receipt=await store.stageProjection(bytes(valid[name]));
    await writeFile(join(parent,`stage-${name}.json`),bytes(receipt));
  }
}
