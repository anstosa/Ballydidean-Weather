import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { access, chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { runAdjustmentArchiveCommand } from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const clocks = [
  "2026-10-08T00:35:00.000Z",
  "2026-10-08T00:36:00.000Z",
  "2026-10-08T00:37:00.000Z",
  "2026-10-08T00:38:00.000Z",
  "2026-10-08T00:39:00.000Z",
  "2026-10-08T00:40:00.000Z",
  "2026-10-08T06:35:00.000Z",
];

// recursively retain the production canonical object order
function canonical(value) {
  // preserve array order while canonicalizing every member
  if (Array.isArray(value)) return value.map(canonical);
  // sort every object by its exact UTF-16 fixture keys
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.keys(value).sort().map((key) => [key, canonical(value[key])]));
  }
  return value;
}

// encode one production-compatible canonical document
function canonicalText(value) {
  return JSON.stringify(canonical(value));
}

// retain the production one-lf file framing
function canonicalBytes(value) {
  return Buffer.from(`${canonicalText(value)}\n`, "utf8");
}

// compute one exact lowercase sha256 identity
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// create stable distinct fixture identities without aliases
function identity(label) {
  return sha256(Buffer.from(`shadow-metadata-custody-oracle/${label}`, "utf8"));
}

// bind one document to its production unsigned canonical bytes
function selfAddress(unsigned, field) {
  const preimage = canonicalBytes(unsigned);
  const digest = sha256(preimage);
  hashPreimages.push([preimage, digest]);
  return { ...unsigned, [field]: digest };
}

// build one exact proof entry from the compact archive fields
function proofEntry(suffix = "one") {
  return {
    archiveCommitOrdinal: "1",
    archiveCommittedAt: clocks[2],
    bodyByteCount: 128,
    capsuleSha256: identity(`${suffix}/capsule`),
    dueKey: clocks[0].replace("2026-10-08T00:35:00.000Z", "capture/2026-10-08T00:35:00.000Z"),
    frontierSha256: identity(`${suffix}/frontier`),
    inputSha256: identity(`${suffix}/input`),
    issuedAt: clocks[0],
    maxValidAt: "2026-10-08T06:35:00.000Z",
    minValidAt: clocks[0],
    predictionBodySha256: identity(`${suffix}/prediction-body`),
    predictionCommittedAt: clocks[1],
    predictionSchemaSha256: identity(`${suffix}/prediction-schema`),
    predictionSha256: identity(`${suffix}/prediction`),
    predecessorFrontierSha256: identity(`${suffix}/predecessor-frontier`),
    receiptSha256: identity(`${suffix}/receipt`),
    registrationSha256: identity(`${suffix}/registration`),
    rowCount: 24,
    sourceReceiptSha256: identity(`${suffix}/source-receipt`),
    stageReceiptSha256: identity(`${suffix}/stage-receipt`),
  };
}

// build one valid self-addressed custody proof
function proof(suffix = "one") {
  return selfAddress({
    acknowledgementSha256: identity(`${suffix}/acknowledgement`),
    archiveCommitOrdinal: "1",
    authority: "shadow_metadata_custody_only",
    contractVersion: "adjustment-shadow-metadata-custody-proof/v1",
    custodyCheckpointSha256: identity(`${suffix}/custody-checkpoint`),
    entries: [proofEntry(suffix)],
    frontierSha256: identity(`${suffix}/frontier`),
    memberRootSha256: identity(`${suffix}/member-root`),
    pageSha256: identity(`${suffix}/page`),
    preparedAt: clocks[3],
  }, "proofSha256");
}

const hashPreimages = [];
const currentProof = proof();
const laterProof = proof("later");
const successorPreimage = Buffer.from([
  identity("metadata/previous-root"),
  identity("metadata/manifest"),
  "1",
  "1",
  clocks[1],
  currentProof.custodyCheckpointSha256,
  currentProof.proofSha256,
].join("\n"), "utf8");
hashPreimages.push([successorPreimage, sha256(successorPreimage)]);
const finalization = {
  coldCommitSha256: currentProof.custodyCheckpointSha256,
  expectedPreviousMetadataRootSha256: identity("metadata/previous-root"),
  finalDisposition: "retain",
  fromCommittedAt: clocks[1],
  generation: 1,
  maintenanceAnchorSha256: currentProof.proofSha256,
  metadataManifestSha256: identity("metadata/manifest"),
  newMetadataRootSha256: sha256(successorPreimage),
  registrationSha256: currentProof.entries[0].registrationSha256,
  rowCount: 1,
  throughCommittedAt: clocks[1],
};
const preparation = selfAddress({
  authority: "shadow_metadata_transfer",
  contractVersion: "adjustment-shadow-metadata-custody-preparation/v1",
  finalizations: [finalization],
  preparedAt: clocks[4],
  proofSha256: currentProof.proofSha256,
}, "preparationSha256");
const consumption = {
  consumedAt: clocks[5],
  contractVersion: "adjustment-shadow-metadata-custody-consumption/v1",
  preparationSha256: preparation.preparationSha256,
  proofSha256: currentProof.proofSha256,
  state: "consumed",
};
const emptyStatus = {
  contractVersion: "adjustment-shadow-metadata-custody-status/v1",
  preparation: null,
  proof: null,
};
const activeStatus = {
  contractVersion: "adjustment-shadow-metadata-custody-status/v1",
  preparation,
  proof: currentProof,
};
const laterStatus = {
  contractVersion: "adjustment-shadow-metadata-custody-status/v1",
  preparation: null,
  proof: laterProof,
};
const documents = {
  activeStatus,
  consumption,
  emptyStatus,
  laterProof,
  laterStatus,
  preparation,
  proof: currentProof,
};

// emit every typed node for the independent Rust decoder
function emitNode(name, path, value) {
  // recursively expose arrays before their indexed members
  if (Array.isArray(value)) {
    console.log(["node", name, path, "array", "-"].join("\t"));
    value.forEach(
      // retain each original array index in its path
      (item, index) => emitNode(name, `${path}/${index}`, item),
    );
    return;
  }
  // recursively expose objects in production canonical key order
  if (value !== null && typeof value === "object") {
    console.log(["node", name, path, "object", "-"].join("\t"));
    Object.entries(canonical(value)).forEach(
      // retain each exact object key in its path
      ([key, item]) => emitNode(name, `${path}/${key}`, item),
    );
    return;
  }
  const type = value === null ? "null" : typeof value;
  console.log(["node", name, path, type, value === null ? "null" : String(value)].join("\t"));
}

// emit one canonical document and its complete typed tree
function emitDocument(name, value) {
  console.log(["doc", name, canonicalText(value)].join("\t"));
  emitNode(name, ".", canonical(value));
}

// distinguish exact fixture presence without following paths
async function exists(path) {
  try {
    await access(path);
    return true;
  } catch {
    return false;
  }
}

// write one owner-private canonical slot
async function writeSlot(directory, slot, documentName) {
  // leave intentional absence untouched
  if (documentName === "-") return;
  const path = join(directory, `${slot}.json`);
  await writeFile(path, canonicalBytes(documents[documentName]), { mode: 0o600 });
  await chmod(path, 0o600);
}

// run one command against its own disposable exact slot population
async function observe(root, fixture) {
  const evidence = join(root, fixture.name);
  const directory = join(evidence, "shadow-metadata-custody-proofs");
  await mkdir(evidence, { mode: 0o700 });
  const slotNames = ["current", "preparation", "previous", "previous-preparation",
    "previous-consumption"];
  const documentNames = [fixture.current, fixture.preparation, fixture.previous,
    fixture.previousPreparation, fixture.consumption];
  // create the fixed proof root only when this case contains durable state
  if (documentNames.some((name) => name !== "-")) {
    await mkdir(directory, { mode: 0o700 });
    await chmod(directory, 0o700);
    // install each selected canonical private slot
    for (const [index, slot] of slotNames.entries()) {
      await writeSlot(directory, slot, documentNames[index]);
    }
  }
  const chunks = [];
  let result;
  try {
    const argv = fixture.action === "status"
      ? ["shadow-metadata-custody-status-v1"]
      : ["shadow-metadata-custody-consumed-v1", fixture.argument];
    await runAdjustmentArchiveCommand(argv, {
      root: evidence,
      stdout: { write: (chunk) => chunks.push(Buffer.from(chunk)) },
    });
    result = Buffer.concat(chunks).toString("utf8").trimEnd();
  } catch {
    result = "error";
  }
  assert.equal(result, fixture.expected);
  assert.equal(await exists(join(directory, "current.json")), fixture.afterCurrent);
  assert.equal(await exists(join(directory, "preparation.json")), fixture.afterPreparation);
  // compare every current proof that the command must preserve
  if (fixture.afterCurrent) {
    assert.deepEqual(await readFile(join(directory, "current.json")),
      canonicalBytes(documents[fixture.current]));
  }
  // compare every preparation that the command must preserve
  if (fixture.afterPreparation) {
    assert.deepEqual(await readFile(join(directory, "preparation.json")),
      canonicalBytes(documents[fixture.preparation]));
  }
  // require every redundant recovery marker to survive the command byte-for-byte
  for (const [slot, documentName] of [
    ["previous", fixture.previous],
    ["previous-preparation", fixture.previousPreparation],
    ["previous-consumption", fixture.consumption],
  ]) {
    // compare only markers that existed in the fixture
    if (documentName !== "-") {
      assert.deepEqual(await readFile(join(directory, `${slot}.json`)),
        canonicalBytes(documents[documentName]));
    }
  }
  console.log(["case", fixture.name, fixture.action, fixture.argument, fixture.current,
    fixture.preparation, fixture.previous, fixture.previousPreparation, fixture.consumption,
    result, Number(fixture.afterCurrent), Number(fixture.afterPreparation)].join("\t"));
}

Object.entries(documents).forEach(
  // expose every reusable canonical document before the cases
  ([name, value]) => emitDocument(name, value),
);
hashPreimages.forEach(
  // expose every exact unsigned or successor preimage
  ([preimage, digest]) => console.log(["hash", preimage.toString("hex"), digest].join("\t")),
);
clocks.forEach(
  // expose every canonical UTC clock and its JavaScript epoch
  (clock) => console.log(["clock", clock, Date.parse(clock)].join("\t")),
);

const cases = [
  { name: "absent", action: "status", argument: "-", current: "-", preparation: "-",
    previous: "-", previousPreparation: "-", consumption: "-",
    expected: canonicalText(emptyStatus), afterCurrent: false, afterPreparation: false },
  { name: "consumed_absent", action: "consumed", argument: currentProof.proofSha256,
    current: "-", preparation: "-", previous: "-", previousPreparation: "-",
    consumption: "-", expected: canonicalText(null),
    afterCurrent: false, afterPreparation: false },
  { name: "active", action: "status", argument: "-", current: "proof",
    preparation: "preparation", previous: "-", previousPreparation: "-", consumption: "-",
    expected: canonicalText(activeStatus), afterCurrent: true, afterPreparation: true },
  { name: "interrupted_recovery", action: "status", argument: "-", current: "proof",
    preparation: "preparation", previous: "proof", previousPreparation: "preparation",
    consumption: "consumption", expected: canonicalText(emptyStatus),
    afterCurrent: false, afterPreparation: false },
  { name: "receipt_lookup", action: "consumed", argument: currentProof.proofSha256,
    current: "-", preparation: "-", previous: "proof", previousPreparation: "preparation",
    consumption: "consumption", expected: canonicalText(consumption),
    afterCurrent: false, afterPreparation: false },
  { name: "receipt_lookup_different_proof", action: "consumed",
    argument: laterProof.proofSha256, current: "-", preparation: "-", previous: "proof",
    previousPreparation: "preparation", consumption: "consumption", expected: "error",
    afterCurrent: false, afterPreparation: false },
  { name: "recovery_after_current_unlink", action: "status", argument: "-", current: "-",
    preparation: "preparation", previous: "proof", previousPreparation: "preparation",
    consumption: "consumption", expected: canonicalText(emptyStatus),
    afterCurrent: false, afterPreparation: false },
  { name: "later_proof_preserved", action: "status", argument: "-", current: "laterProof",
    preparation: "-", previous: "proof", previousPreparation: "preparation",
    consumption: "consumption", expected: canonicalText(laterStatus),
    afterCurrent: true, afterPreparation: false },
  { name: "matching_receipt_missing_previous", action: "status", argument: "-",
    current: "proof", preparation: "preparation", previous: "-",
    previousPreparation: "preparation", consumption: "consumption", expected: "error",
    afterCurrent: true, afterPreparation: true },
];
const root = await mkdtemp(join(tmpdir(), "weather-shadow-custody-oracle-"));
try {
  // isolate every command invocation in its own current-user fixture root
  for (const fixture of cases) await observe(root, fixture);
} finally {
  await rm(root, { recursive: true, force: true });
}
