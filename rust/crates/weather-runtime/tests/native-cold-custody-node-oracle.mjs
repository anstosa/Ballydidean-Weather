import { createHash } from "node:crypto";
import { readFileSync, readSync, writeSync } from "node:fs";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";
import {
  AdjustmentRevisionArchiveStore,
  acknowledgeAdjustmentRevisionColdCustodyCheckpoint,
  buildAdjustmentRevisionColdGraphSegment,
  createAdjustmentRevisionArchiveHandler,
  readAdjustmentRevisionColdPage,
  readAdjustmentRevisionColdTransferStart,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

// retain the original sorted value transformation
function canonical(value) {
  if (Array.isArray(value)) return value.map(canonical);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])]));
  }
  return value;
}

// serialize through the actual Node JSON primitive
function bytes(value) {
  return Buffer.from(JSON.stringify(canonical(value)) + "\n");
}

// preserve exact original content identities
function sha(value) {
  return createHash("sha256").update(value).digest("hex");
}

// emit UTF-16 code units in a test-only typed-value transport
function units(value) {
  let result = "";
  for (let index = 0; index < value.length; index += 1) {
    result += value.charCodeAt(index).toString(16).padStart(4,"0");
  }
  return result;
}

// bridge Node parsed values to generated Rust JsonValue, without a Rust JSON parser
function tree(value, lines) {
  if (value === null) lines.push("N");
  else if (typeof value === "boolean") lines.push(value ? "B1" : "B0");
  else if (typeof value === "number") lines.push("D" + String(value));
  else if (typeof value === "string") lines.push("S" + units(value));
  else if (Array.isArray(value)) {
    lines.push("A" + value.length);
    for (const item of value) tree(item, lines);
  } else {
    const keys = Object.keys(value);
    lines.push("O" + keys.length);
    for (const key of keys) {
      lines.push("K" + units(key));
      tree(value[key], lines);
    }
  }
}

// recover one fixture-only transported string
function fromUnits(encoded) {
  let result = "";
  for (let index = 0; index < encoded.length; index += 4) {
    result += String.fromCharCode(Number.parseInt(encoded.slice(index,index+4),16));
  }
  return result;
}

// decode the generated value tree for Node's actual writer
function fromTree(lines) {
  let cursor = 0;
  const read = () => {
    const line = lines[cursor++];
    const tag = line[0], value = line.slice(1);
    switch (tag) {
      case "N": return null;
      case "B": return value === "1";
      case "D": return Number(value);
      case "S": return fromUnits(value);
      case "A": return Array.from({length:Number(value)}, read);
      case "O": return Object.fromEntries(Array.from({length:Number(value)}, () => {
        const key = lines[cursor++];
        if (!key.startsWith("K")) throw new Error("fixture key");
        return [fromUnits(key.slice(1)), read()];
      }));
      default: throw new Error("fixture tag");
    }
  };
  const result = read();
  if (cursor !== lines.length) throw new Error("fixture excess tree");
  return result;
}

// provide only actual standard Node primitives for this disposable parity fixture
function primitiveBytes(operation,input) {
  const text=input.toString("utf8");
  switch (operation) {
    case "decode": {
      const lines=[];
      tree(JSON.parse(text),lines);
      return Buffer.from(lines.join("\n")+"\n");
    }
    // use the exact original server Request and URLSearchParams boundary
    case "api-target": {
      const [method, host, target] = fromTree(text.trimEnd().split("\n"));
      const request = new Request(`http://${host}${target}`, { method });
      const url = new URL(request.url);
      const lines = [];
      tree([request.method, url.pathname, [...url.searchParams]], lines);
      return Buffer.from(lines.join("\n") + "\n");
    }
    // preserve the original web edge's fixed-base URL and decoded ordered parameters
    case "edge-target": {
      const target = fromTree(text.trimEnd().split("\n"));
      const url = new URL(target, "http://weather.invalid");
      const lines = [];
      tree([url.pathname, url.search, [...url.searchParams]], lines);
      return Buffer.from(lines.join("\n") + "\n");
    }
    case "sorted": return bytes(fromTree(text.trimEnd().split("\n")));
    case "ordered": return Buffer.from(JSON.stringify(fromTree(text.trimEnd().split("\n")))+"\n");
    case "sha": return createHash("sha256").update(input).digest();
    case "base64-decode": return Buffer.from(text,"base64");
    case "base64-encode": return Buffer.from(input.toString("base64"));
    case "date-format": return Buffer.from(new Date(Number(text)).toISOString());
    case "date-parse-explicit": {
      const millis=Date.parse(text);
      if (!Number.isFinite(millis)) throw new Error("invalid date");
      return Buffer.from(String(millis));
    }
    case "date-parse": {
      const millis=Date.parse(text);
      if (!Number.isFinite(millis)||new Date(millis).toISOString()!==text) throw new Error("invalid canonical date");
      return Buffer.from(String(millis));
    }
    case "string": {
      const lines=[];
      tree(String(fromTree(text.trimEnd().split("\n"))),lines);
      return Buffer.from(lines.join("\n")+"\n");
    }
    default: throw new Error("unknown fixture primitive");
  }
}

// read one small fixture request header without buffering beyond its end
function bridgeHeader() {
  const buffer=Buffer.alloc(1);
  let line="";
  for (;;) {
    if (readSync(0,buffer,0,1,null)===0) {
      if (line.length!==0) throw new Error("partial fixture header");
      return null;
    }
    if (buffer[0]===10) return line;
    if (line.length>=100) throw new Error("fixture header too long");
    line+=String.fromCharCode(buffer[0]);
  }
}

// transfer only the announced disposable primitive fixture bytes
function bridgeRead(length) {
  const buffer=Buffer.alloc(length);
  let offset=0;
  while (offset<length) {
    const count=readSync(0,buffer,offset,length-offset,null);
    if (count===0) throw new Error("partial fixture body");
    offset+=count;
  }
  return buffer;
}

// write complete fixture output through the retained standard streams
function bridgeWrite(buffer) {
  let offset=0;
  while (offset<buffer.length) offset+=writeSync(1,buffer,offset,buffer.length-offset);
}

// keep one test-only primitive owner alive per Rust test instead of spawning per field
function bridge() {
  for (;;) {
    const header=bridgeHeader();
    if (header===null) return;
    const [operation,lengthText]=header.split(" ");
    const length=Number(lengthText);
    if (!Number.isSafeInteger(length)||length<0||length>32*1024*1024) throw new Error("fixture request bound");
    const input=bridgeRead(length);
    try {
      const output=primitiveBytes(operation,input);
      bridgeWrite(Buffer.from(`ok ${output.length}\n`));
      bridgeWrite(output);
    } catch {
      bridgeWrite(Buffer.from("error 0\n"));
    }
  }
}

// use actual database receipt identity preimages from the original source contract
function receiptFor(stage, ordinal, predecessor) {
  const receipt = {
    archiveCommitOrdinal: String(ordinal),
    archiveCommittedAt: new Date(Date.parse("2026-10-08T01:00:00.000Z") + ordinal*60_000).toISOString(),
    contractVersion: "adjustment-revision-commit-receipt/v1",
    frontierSha256: "",
    predecessorFrontierSha256: predecessor,
    projectionIdentitySha256: stage.projectionIdentitySha256,
    projectionKind: stage.projectionKind,
    projectionSha256: stage.projectionSha256,
    receiptSha256: "",
    stageReceiptSha256: stage.stageReceiptSha256,
  };
  receipt.receiptSha256 = sha([
    receipt.contractVersion,receipt.archiveCommitOrdinal,receipt.archiveCommittedAt,
    receipt.projectionKind,receipt.projectionIdentitySha256,receipt.projectionSha256,
    receipt.stageReceiptSha256,receipt.predecessorFrontierSha256,
  ].join("\n"));
  receipt.frontierSha256 = sha([
    "adjustment-revision-frontier/v1",predecessor,receipt.archiveCommitOrdinal,receipt.receiptSha256,
  ].join("\n"));
  return receipt;
}

// retain one ordinary and one one-row grouped body through the real Node archive store
async function seed(root) {
  await mkdir(root,{mode:0o700});
  const store = new AdjustmentRevisionArchiveStore({
    root, now: () => new Date("2026-10-08T00:30:00.000Z"),
    parseProjection: buffer => JSON.parse(buffer.toString("utf8")),
  });
  const genesis = sha("adjustment-revision-frontier/v1\n0\n");
  let predecessor = genesis;
  const receipts = [];
  for (let index=0; index<2; index+=1) {
    const grouped = index===1;
    const projection = {
      contractVersion: grouped ? "adjustment-revision-batch-projection/v2" : "adjustment-revision-projection/v1",
      family:"shared", logicalKey:{index},
      logicalReceivedAt:new Date(Date.parse("2026-10-08T00:00:00.000Z")+index*60_000).toISOString(),
      projectionKind: grouped ? "target_revision" : "actual_best_match",
      rows:[{index}], source:{index:String(index)}, storedContentSha256:String(index+1).repeat(64),
    };
    const stage = await store.stageProjection(bytes(projection));
    const receipt = receiptFor(stage,index+1,predecessor);
    if (grouped) await store.publishRevision({revisionReceipts:[receipt],stageReceipt:stage});
    else await store.publishRevision({revisionReceipt:receipt,stageReceipt:stage});
    receipts.push(receipt);
    predecessor=receipt.frontierSha256;
  }
  const snapshot = {
    archiveCommitOrdinal:"2", contractVersion:"adjustment-revision-serving-snapshot/v1",
    cutoffAt:"2026-10-08T02:00:00.000Z", entryCount:2, frontierSha256:predecessor,
    entries:receipts.map((receipt,index)=>({
      logicalReceivedAt:new Date(Date.parse("2026-10-08T00:00:00.000Z")+index*60_000).toISOString(),
      receipt, relation:"weather_records",
    })), snapshotSha256:"",
  };
  snapshot.snapshotSha256=sha([
    snapshot.contractVersion,snapshot.cutoffAt,snapshot.archiveCommitOrdinal,snapshot.frontierSha256,
    receipts.map(receipt=>receipt.receiptSha256).join("\n"),
  ].join("\n"));
  const start=await readAdjustmentRevisionColdTransferStart({root,servingSnapshot:snapshot});
  const request={
    root, afterArchiveCommitOrdinal:"0",afterFrontierSha256:genesis,
    previousPageSha256:start.startSha256,startSha256:start.startSha256,
    watermarkArchiveCommitOrdinal:"2",watermarkFrontierSha256:predecessor,
  };
  const page=await readAdjustmentRevisionColdPage(request);
  return {start,page,request};
}

// create a real v2 shadow capsule with its separate comparator and compact producer metadata
async function seedShadow(root) {
  await mkdir(root,{mode:0o700});
  const body=Buffer.from('{"body":true}\n');
  const comparator=Buffer.from('{"comparator":true}\n');
  const source=Buffer.from('{"source":true}\n');
  const metadata={
    bodyByteCount:body.length,candidateSha256:"1".repeat(64),
    dueKey:"capture/2026-10-08T00:35:00.000Z",inputSha256:"2".repeat(64),
    issuedAt:"2026-10-08T00:35:01.000Z",maxValidAt:"2026-10-08T12:00:00.000Z",
    minValidAt:"2026-10-08T01:00:00.000Z",predictionBodySha256:sha(body),
    predictionSchemaSha256:"3".repeat(64),predictionSha256:"4".repeat(64),
    registrationSha256:"5".repeat(64),rowCount:12,sourceReceiptSha256:"6".repeat(64),
    sourceSha256:"7".repeat(64),
  };
  const store=new AdjustmentRevisionArchiveStore({
    root,now:()=>new Date("2026-10-08T00:36:00.000Z"),
    parseProjection:buffer=>JSON.parse(buffer.toString("utf8")),
    // retain the same producer fixture checks as the original archive test
    validateShadowStage:input=> {
      if (!input.body.equals(body)||!input.comparator.equals(comparator)||!input.sourceProjection.equals(source)) throw new Error("fixture stage binding");
    },
    validateShadowCapsule:capsule=> {
      if (!Buffer.from(capsule.comparatorBase64,"base64").equals(comparator)) throw new Error("fixture capsule binding");
    },
  });
  const handler=createAdjustmentRevisionArchiveHandler({store});
  const stageInput={
    bodyBase64:body.toString("base64"),comparatorBase64:comparator.toString("base64"),
    metadata,sourceProjectionBase64:source.toString("base64"),sourceProjectionSha256:sha(source),
  };
  const stage=await handler("/internal/adjustment-maintenance/archive/stage",stageInput);
  const genesis=sha("adjustment-revision-frontier/v1\n0\n");
  const receipt=receiptFor({...stage, projectionKind:"shadow_prediction",
    projectionIdentitySha256:metadata.sourceReceiptSha256,projectionSha256:metadata.inputSha256},1,genesis);
  await handler("/internal/adjustment-maintenance/archive/publish",{
    ...stageInput,predictionCommittedAt:"2026-10-08T00:35:30.000Z",revisionReceipt:receipt,stageReceipt:stage,
  });
  const snapshot={
    archiveCommitOrdinal:"1",contractVersion:"adjustment-revision-serving-snapshot/v1",
    cutoffAt:"2026-10-08T02:00:00.000Z",entryCount:0,entries:[],
    frontierSha256:receipt.frontierSha256,snapshotSha256:"",
  };
  snapshot.snapshotSha256=sha([
    snapshot.contractVersion,snapshot.cutoffAt,snapshot.archiveCommitOrdinal,snapshot.frontierSha256,"",
  ].join("\n"));
  const start=await readAdjustmentRevisionColdTransferStart({root,servingSnapshot:snapshot});
  const request={
    root,afterArchiveCommitOrdinal:"0",afterFrontierSha256:genesis,
    previousPageSha256:start.startSha256,startSha256:start.startSha256,
    watermarkArchiveCommitOrdinal:"1",watermarkFrontierSha256:receipt.frontierSha256,
  };
  const page=await readAdjustmentRevisionColdPage(request);
  return {start,page,request};
}

// build the original Node page and complete packed custody acknowledgement on a second fixture
async function fixtures(parent, mode) {
  const seedFixture = mode==="shadow" ? seedShadow : seed;
  const {page:rustPage}=await seedFixture(join(parent,"rust"));
  const root=join(parent,"oracle");
  const {start,page,request}=await seedFixture(root);
  if (!bytes(page).equals(bytes(rustPage))) throw new Error("independent source fixtures differ");
  const graph=buildAdjustmentRevisionColdGraphSegment({page,start});
  const checkpoint=JSON.parse(graph.members[0].payload.toString("utf8"));
  const input={
    ...request,pageSha256:page.pageSha256,custodyCheckpointSha256:"c".repeat(64),
    memberRootSha256:checkpoint.memberRootSha256,startMemberSha256:checkpoint.startMemberSha256,
  };
  const acknowledgement=await acknowledgeAdjustmentRevisionColdCustodyCheckpoint(input,{
    now:()=>new Date("2026-10-08T03:00:00.000Z"),
  });
  await writeFile(join(parent,"start.json"),bytes(start));
  await writeFile(join(parent,"page.json"),bytes(page));
  await writeFile(join(parent,"ack.json"),bytes(acknowledgement));
  if (mode==="shadow") await writeFile(join(parent,"proof.json"),await readFile(join(root,"shadow-metadata-custody-proofs/current.json")));
  await writeFile(join(parent,"operands.txt"),[
    input.watermarkArchiveCommitOrdinal,input.watermarkFrontierSha256,input.startSha256,
    input.afterArchiveCommitOrdinal,input.afterFrontierSha256,input.previousPageSha256,
    input.pageSha256,input.custodyCheckpointSha256,input.memberRootSha256,input.startMemberSha256,
  ].join("\n")+"\n");
}

if (process.argv[2]==="primitive") process.stdout.write(primitiveBytes(process.argv[3],readFileSync(0)));
else if (process.argv[2]==="bridge") bridge();
else if (process.argv[2]==="fixtures") await fixtures(process.argv[3],process.argv[4]);
else throw new Error("unknown fixture command");
