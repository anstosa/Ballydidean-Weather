import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { chmod, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  AdjustmentShadowMetadataCustodyProofStore,
  runAdjustmentArchiveCommand,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";
import {
  buildAdjustmentShadowMetadataFinalizationSql,
  buildAdjustmentShadowMetadataPreparationSql,
} from "../../../../deploy/scripts/adjustment-evaluation-package.mjs";

const clocks = [
  "2026-10-08T00:35:00.000Z",
  "2026-10-08T00:36:00.000Z",
  "2026-10-08T00:37:00.000Z",
  "2026-10-08T00:38:00.000Z",
  "2026-10-08T00:39:00.000Z",
  "2026-10-08T00:40:00.000Z",
  "2026-10-08T00:41:00.000Z",
  "2026-10-08T06:35:00.000Z",
];
const hashPreimages = [];

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
  return sha256(Buffer.from(`shadow-metadata-custody-mutation-oracle/${label}`, "utf8"));
}

// bind one document to its production unsigned canonical bytes
function selfAddress(unsigned, field) {
  const preimage = canonicalBytes(unsigned);
  const digest = sha256(preimage);
  hashPreimages.push([preimage, digest]);
  return { ...unsigned, [field]: digest };
}

// build one exact proof entry from the compact archive fields
function proofEntry(suffix) {
  return {
    archiveCommitOrdinal: "1",
    archiveCommittedAt: clocks[2],
    bodyByteCount: 128,
    capsuleSha256: identity(`${suffix}/capsule`),
    dueKey: "capture/2026-10-08T00:35:00.000Z",
    frontierSha256: identity(`${suffix}/frontier`),
    inputSha256: identity(`${suffix}/input`),
    issuedAt: clocks[0],
    maxValidAt: clocks[7],
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

// build one valid self-addressed custody proof with canonically ordered entries
function proof(suffixes) {
  const entries = suffixes.map(proofEntry).sort((left, right) =>
    left.registrationSha256.localeCompare(right.registrationSha256) ||
      left.predictionSha256.localeCompare(right.predictionSha256));
  const suffix = suffixes.join("-");
  return selfAddress({
    acknowledgementSha256: identity(`${suffix}/acknowledgement`),
    archiveCommitOrdinal: "1",
    authority: "shadow_metadata_custody_only",
    contractVersion: "adjustment-shadow-metadata-custody-proof/v1",
    custodyCheckpointSha256: identity(`${suffix}/custody-checkpoint`),
    entries,
    frontierSha256: identity(`${suffix}/frontier`),
    memberRootSha256: identity(`${suffix}/member-root`),
    pageSha256: identity(`${suffix}/page`),
    preparedAt: clocks[3],
  }, "proofSha256");
}

// derive one exact owner-side finalization from a proof registration
function finalization(proofValue, registrationSha256, suffix, overrides = {}) {
  const entries = proofValue.entries.filter(
    // select the complete committed range for one registration
    (entry) => entry.registrationSha256 === registrationSha256,
  );
  const clocksForRegistration = entries.map((entry) => entry.predictionCommittedAt).sort();
  const value = {
    coldCommitSha256: proofValue.custodyCheckpointSha256,
    expectedPreviousMetadataRootSha256: identity(`${suffix}/previous-root`),
    finalDisposition: "retain",
    fromCommittedAt: clocksForRegistration[0],
    generation: 1,
    maintenanceAnchorSha256: proofValue.proofSha256,
    metadataManifestSha256: identity(`${suffix}/manifest`),
    registrationSha256,
    rowCount: entries.length,
    throughCommittedAt: clocksForRegistration.at(-1),
    ...overrides,
  };
  const preimage = Buffer.from([
    value.expectedPreviousMetadataRootSha256,
    value.metadataManifestSha256,
    String(value.generation),
    String(value.rowCount),
    value.throughCommittedAt,
    value.coldCommitSha256,
    value.maintenanceAnchorSha256,
  ].join("\n"), "utf8");
  hashPreimages.push([preimage, sha256(preimage)]);
  return { ...value, newMetadataRootSha256: sha256(preimage) };
}

// construct the exact durable preparation returned by the original command
function preparation(proofValue, finalizations, preparedAt) {
  const ordered = [...finalizations].sort(
    // retain the original registration ordering
    (left, right) => left.registrationSha256.localeCompare(right.registrationSha256),
  );
  return selfAddress({
    authority: "shadow_metadata_transfer",
    contractVersion: "adjustment-shadow-metadata-custody-preparation/v1",
    finalizations: ordered,
    preparedAt,
    proofSha256: proofValue.proofSha256,
  }, "preparationSha256");
}

// build one exact owner result bound to a prepared finalization
function result(finalizationValue, status = "finalized") {
  return {
    generation: finalizationValue.generation,
    metadataManifestSha256: finalizationValue.metadataManifestSha256,
    newMetadataRootSha256: finalizationValue.newMetadataRootSha256,
    registrationSha256: finalizationValue.registrationSha256,
    status,
  };
}

const proofOne = proof(["one"]);
const proofTwo = proof(["alpha", "beta"]);
const finalizationOne = finalization(
  proofOne,
  proofOne.entries[0].registrationSha256,
  "one",
);
const finalizationOneAlternate = finalization(
  proofOne,
  proofOne.entries[0].registrationSha256,
  "one-alternate",
);
const finalizationOneBadRange = finalization(
  proofOne,
  proofOne.entries[0].registrationSha256,
  "one-bad-range",
  { throughCommittedAt: clocks[2] },
);
const finalizationsTwo = proofTwo.entries.map((entry, index) =>
  finalization(proofTwo, entry.registrationSha256, `two-${index}`));
const preparationOne = preparation(proofOne, [finalizationOne], clocks[4]);
const preparationTwo = preparation(proofTwo, finalizationsTwo, clocks[6]);
const consumptionOne = {
  consumedAt: clocks[5],
  contractVersion: "adjustment-shadow-metadata-custody-consumption/v1",
  preparationSha256: preparationOne.preparationSha256,
  proofSha256: proofOne.proofSha256,
  state: "consumed",
};
const prepareInputOne = {
  finalizations: [finalizationOne],
  proofSha256: proofOne.proofSha256,
};
const prepareInputExtra = {
  ...prepareInputOne,
  unexpected: true,
};
const prepareInputAlternate = {
  finalizations: [finalizationOneAlternate],
  proofSha256: proofOne.proofSha256,
};
const prepareInputBadRange = {
  finalizations: [finalizationOneBadRange],
  proofSha256: proofOne.proofSha256,
};
const prepareInputTwoReversed = {
  finalizations: [...finalizationsTwo].reverse(),
  proofSha256: proofTwo.proofSha256,
};
const resultOne = result(finalizationOne);
const resultOneMismatch = {
  ...resultOne,
  metadataManifestSha256: identity("one/result-mismatch-manifest"),
};
const consumeInputOne = {
  preparationSha256: preparationOne.preparationSha256,
  proofSha256: proofOne.proofSha256,
  results: [resultOne],
};
const consumeInputMismatch = {
  ...consumeInputOne,
  results: [resultOneMismatch],
};
const consumeInputTwoReversed = {
  preparationSha256: preparationTwo.preparationSha256,
  proofSha256: proofTwo.proofSha256,
  results: [...preparationTwo.finalizations].map((value) => result(value)).reverse(),
};
const documents = {
  consumeInputMismatch,
  consumeInputOne,
  consumeInputTwoReversed,
  consumptionOne,
  preparationOne,
  preparationTwo,
  prepareInputAlternate,
  prepareInputBadRange,
  prepareInputExtra,
  prepareInputOne,
  prepareInputTwoReversed,
  proofOne,
  proofTwo,
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

// write one owner-private canonical slot
async function writeSlot(directory, slot, documentName) {
  // leave intentional absence untouched
  if (documentName === "-") return;
  const path = join(directory, `${slot}.json`);
  await writeFile(path, canonicalBytes(documents[documentName]), { mode: 0o600 });
  await chmod(path, 0o600);
}

// identify the exact fixed post-command slot population
async function requireSlots(directory, expectedNames) {
  const slotNames = ["current", "preparation", "previous", "previous-preparation",
    "previous-consumption"];
  const expectedFiles = slotNames.filter((_, index) => expectedNames[index] !== "-")
    .map((slot) => `${slot}.json`).sort();
  assert.deepEqual((await readdir(directory)).sort(), expectedFiles);
  // compare every retained or published slot byte-for-byte
  for (const [index, slot] of slotNames.entries()) {
    const documentName = expectedNames[index];
    if (documentName !== "-") {
      assert.deepEqual(await readFile(join(directory, `${slot}.json`)),
        canonicalBytes(documents[documentName]));
    }
  }
}

// run one actual original mutation against its own disposable exact slot population
async function observe(root, fixture) {
  const evidence = join(root, fixture.name);
  const directory = join(evidence, "shadow-metadata-custody-proofs");
  await mkdir(evidence, { mode: 0o700 });
  await chmod(evidence, 0o700);
  await mkdir(directory, { mode: 0o700 });
  await chmod(directory, 0o700);
  const before = [fixture.current, fixture.preparation, fixture.previous,
    fixture.previousPreparation, fixture.consumption];
  const slots = ["current", "preparation", "previous", "previous-preparation",
    "previous-consumption"];
  // install each selected exact predecessor slot
  for (const [index, slot] of slots.entries()) await writeSlot(directory, slot, before[index]);
  const chunks = [];
  let failure = null;
  try {
    await runAdjustmentArchiveCommand([
      `shadow-metadata-custody-${fixture.action}-v1`,
      fixture.argument,
    ], {
      now: () => new Date(fixture.now),
      root: evidence,
      stdinBytes: canonicalBytes(documents[fixture.input]),
      stdout: { write: (chunk) => chunks.push(Buffer.from(chunk)) },
    });
  } catch (error) {
    failure = error;
  }
  // distinguish an expected source rejection from an assertion about successful bytes
  if (fixture.expected === "error") {
    assert.notEqual(failure, null);
    assert.equal(chunks.length, 0);
  } else {
    assert.equal(failure, null);
    assert.deepEqual(Buffer.concat(chunks), canonicalBytes(documents[fixture.expected]));
  }
  const after = [fixture.afterCurrent, fixture.afterPreparation, fixture.afterPrevious,
    fixture.afterPreviousPreparation, fixture.afterConsumption];
  await requireSlots(directory, after);
  console.log(["case", fixture.name, fixture.action, fixture.now, fixture.argument,
    fixture.input, ...before, fixture.expected, ...after].join("\t"));
}

// run one original owner-query branch against disposable exact status files
async function observeOwnerQuery(root, fixture) {
  const evidence = join(root, fixture.name);
  const directory = join(evidence, "shadow-metadata-custody-proofs");
  await mkdir(evidence, { mode: 0o700 });
  await chmod(evidence, 0o700);
  // create the fixed proof root only when this status has a selected document
  if (fixture.current !== "-" || fixture.preparation !== "-") {
    await mkdir(directory, { mode: 0o700 });
    await chmod(directory, 0o700);
    await writeSlot(directory, "current", fixture.current);
    await writeSlot(directory, "preparation", fixture.preparation);
  }
  let output;
  let failure = null;
  try {
    const status = await new AdjustmentShadowMetadataCustodyProofStore({
      root: evidence,
    }).readStatus();
    // require the current proof before every original owner query
    if (status.proof === null) {
      throw new Error("shadow metadata custody proof is unavailable");
    }
    // preserve the three exact owner-query dispatch branches
    if (fixture.action === "shadow-metadata-finalization-sql-v1") {
      // bind finalization sql to the selected durable preparation
      if (status.preparation?.preparationSha256 !== fixture.argument) {
        throw new Error("shadow metadata custody preparation differs");
      }
      output = buildAdjustmentShadowMetadataFinalizationSql(status.preparation);
    } else {
      // bind both proof queries to the selected current proof
      if (status.proof.proofSha256 !== fixture.argument) {
        throw new Error("shadow metadata custody proof differs");
      }
      // preserve the identity query's exact one-line response
      if (fixture.action === "shadow-metadata-preparation-identity-v1") {
        output = `${status.preparation?.preparationSha256 ?? "unprepared"}\n`;
      } else if (fixture.action === "shadow-metadata-preparation-sql-v1") {
        // refuse a second owner preparation before emitting sql
        if (status.preparation !== null) {
          throw new Error("shadow metadata preparation already exists");
        }
        output = buildAdjustmentShadowMetadataPreparationSql(status.proof);
      } else {
        throw new Error("unexpected owner-query fixture action");
      }
    }
  } catch (error) {
    failure = error;
  }
  // expose exact successful bytes while retaining rejection as a closed outcome
  const expected = fixture.expected === "error"
    ? "error"
    : Buffer.from(output, "utf8").toString("hex");
  assert.equal(failure === null, fixture.expected !== "error");
  // owner queries must retain their complete selected file state unchanged
  if (fixture.current === "-" && fixture.preparation === "-") {
    await assert.rejects(readdir(directory));
  } else {
    await requireSlots(directory, [fixture.current, fixture.preparation, "-", "-", "-"]);
  }
  console.log(["owner-query", fixture.name, fixture.action, fixture.argument,
    fixture.current, fixture.preparation, expected].join("\t"));
}

// exercise the original owner bridge's JSON.parse boundary without its live-root CLI default
async function observeOwnerMutation(root, fixture, inputBytes) {
  const evidence = join(root, fixture.name);
  const directory = join(evidence, "shadow-metadata-custody-proofs");
  await mkdir(evidence, { mode: 0o700 });
  await chmod(evidence, 0o700);
  await mkdir(directory, { mode: 0o700 });
  await chmod(directory, 0o700);
  const before = [fixture.current, fixture.preparation, fixture.previous,
    fixture.previousPreparation, fixture.consumption];
  const slots = ["current", "preparation", "previous", "previous-preparation",
    "previous-consumption"];
  // install the exact predecessor state before directly calling the original store
  for (const [index, slot] of slots.entries()) await writeSlot(directory, slot, before[index]);
  const input = JSON.parse(inputBytes.toString("utf8"));
  const store = new AdjustmentShadowMetadataCustodyProofStore({
    now: () => new Date(fixture.now),
    root: evidence,
  });
  const resultValue = fixture.action === "prepare-shadow-metadata-custody-v1"
    ? await store.prepareFinalizations(input)
    : await store.consume(input);
  assert.deepEqual(canonicalBytes(resultValue), canonicalBytes(documents[fixture.expected]));
  const after = [fixture.afterCurrent, fixture.afterPreparation, fixture.afterPrevious,
    fixture.afterPreviousPreparation, fixture.afterConsumption];
  await requireSlots(directory, after);
  console.log(["owner-mutation", fixture.name, fixture.action, fixture.now, fixture.argument,
    fixture.input, ...before, fixture.expected, ...after].join("\t"));
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

const rawInputs = {
  ownerPrepareInputOne: [
    "prepareInputOne",
    Buffer.from(JSON.stringify(prepareInputOne, null, 2), "utf8"),
  ],
  ownerConsumeInputOne: [
    "consumeInputOne",
    Buffer.from(JSON.stringify(consumeInputOne, null, 2), "utf8"),
  ],
  ownerPrepareInputExtra: ["prepareInputExtra",
    Buffer.from(JSON.stringify(prepareInputExtra, null, 2), "utf8")],
};
Object.entries(rawInputs).forEach(
  // retain precise noncanonical owner bytes and their equivalent typed fixture
  ([name, [typedDocument, bytes]]) => console.log(
    ["raw", name, typedDocument, bytes.toString("hex")].join("\t"),
  ),
);

const cases = [
  { name: "prepare_success", action: "prepare", now: clocks[4],
    argument: proofOne.proofSha256, input: "prepareInputOne", current: "proofOne",
    preparation: "-", previous: "-", previousPreparation: "-", consumption: "-",
    expected: "preparationOne", afterCurrent: "proofOne", afterPreparation: "preparationOne",
    afterPrevious: "-", afterPreviousPreparation: "-", afterConsumption: "-" },
  { name: "prepare_retry", action: "prepare", now: clocks[6],
    argument: proofOne.proofSha256, input: "prepareInputOne", current: "proofOne",
    preparation: "preparationOne", previous: "-", previousPreparation: "-", consumption: "-",
    expected: "preparationOne", afterCurrent: "proofOne", afterPreparation: "preparationOne",
    afterPrevious: "-", afterPreviousPreparation: "-", afterConsumption: "-" },
  { name: "prepare_collision", action: "prepare", now: clocks[6],
    argument: proofOne.proofSha256, input: "prepareInputAlternate", current: "proofOne",
    preparation: "preparationOne", previous: "-", previousPreparation: "-", consumption: "-",
    expected: "error", afterCurrent: "proofOne", afterPreparation: "preparationOne",
    afterPrevious: "-", afterPreviousPreparation: "-", afterConsumption: "-" },
  { name: "prepare_range_mismatch", action: "prepare", now: clocks[6],
    argument: proofOne.proofSha256, input: "prepareInputBadRange", current: "proofOne",
    preparation: "-", previous: "-", previousPreparation: "-", consumption: "-",
    expected: "error", afterCurrent: "proofOne", afterPreparation: "-",
    afterPrevious: "-", afterPreviousPreparation: "-", afterConsumption: "-" },
  { name: "prepare_two_sorted", action: "prepare", now: clocks[6],
    argument: proofTwo.proofSha256, input: "prepareInputTwoReversed", current: "proofTwo",
    preparation: "-", previous: "-", previousPreparation: "-", consumption: "-",
    expected: "preparationTwo", afterCurrent: "proofTwo", afterPreparation: "preparationTwo",
    afterPrevious: "-", afterPreviousPreparation: "-", afterConsumption: "-" },
  { name: "consume_success", action: "consume", now: clocks[5],
    argument: preparationOne.preparationSha256, input: "consumeInputOne", current: "proofOne",
    preparation: "preparationOne", previous: "-", previousPreparation: "-", consumption: "-",
    expected: "consumptionOne", afterCurrent: "-", afterPreparation: "-",
    afterPrevious: "proofOne", afterPreviousPreparation: "preparationOne",
    afterConsumption: "consumptionOne" },
  { name: "consume_retry", action: "consume", now: clocks[6],
    argument: preparationOne.preparationSha256, input: "consumeInputOne", current: "-",
    preparation: "-", previous: "proofOne", previousPreparation: "preparationOne",
    consumption: "consumptionOne", expected: "consumptionOne", afterCurrent: "-",
    afterPreparation: "-", afterPrevious: "proofOne",
    afterPreviousPreparation: "preparationOne", afterConsumption: "consumptionOne" },
  { name: "consume_interrupted_retry", action: "consume", now: clocks[6],
    argument: preparationOne.preparationSha256, input: "consumeInputOne", current: "proofOne",
    preparation: "preparationOne", previous: "proofOne",
    previousPreparation: "preparationOne", consumption: "consumptionOne",
    expected: "consumptionOne", afterCurrent: "-", afterPreparation: "-",
    afterPrevious: "proofOne", afterPreviousPreparation: "preparationOne",
    afterConsumption: "consumptionOne" },
  { name: "consume_result_mismatch", action: "consume", now: clocks[5],
    argument: preparationOne.preparationSha256, input: "consumeInputMismatch", current: "proofOne",
    preparation: "preparationOne", previous: "-", previousPreparation: "-", consumption: "-",
    expected: "error", afterCurrent: "proofOne", afterPreparation: "preparationOne",
    afterPrevious: "-", afterPreviousPreparation: "-", afterConsumption: "-" },
  { name: "consume_two_reversed", action: "consume", now: clocks[6],
    argument: preparationTwo.preparationSha256, input: "consumeInputTwoReversed", current: "proofTwo",
    preparation: "preparationTwo", previous: "-", previousPreparation: "-", consumption: "-",
    expected: "error", afterCurrent: "proofTwo", afterPreparation: "preparationTwo",
    afterPrevious: "-", afterPreviousPreparation: "-", afterConsumption: "-" },
  { name: "prepare_after_interrupted_consumption", action: "prepare", now: clocks[6],
    argument: proofOne.proofSha256, input: "prepareInputOne", current: "proofOne",
    preparation: "preparationOne", previous: "proofOne",
    previousPreparation: "preparationOne", consumption: "consumptionOne",
    expected: "error", afterCurrent: "-", afterPreparation: "-",
    afterPrevious: "proofOne", afterPreviousPreparation: "preparationOne",
    afterConsumption: "consumptionOne" },
];
const ownerQueries = [
  { name: "owner_identity_unprepared", action: "shadow-metadata-preparation-identity-v1",
    argument: proofOne.proofSha256, current: "proofOne", preparation: "-", expected: "success" },
  { name: "owner_identity_prepared", action: "shadow-metadata-preparation-identity-v1",
    argument: proofOne.proofSha256, current: "proofOne", preparation: "preparationOne",
    expected: "success" },
  { name: "owner_preparation_sql", action: "shadow-metadata-preparation-sql-v1",
    argument: proofOne.proofSha256, current: "proofOne", preparation: "-", expected: "success" },
  { name: "owner_preparation_sql_two", action: "shadow-metadata-preparation-sql-v1",
    argument: proofTwo.proofSha256, current: "proofTwo", preparation: "-", expected: "success" },
  { name: "owner_preparation_sql_already_prepared", action: "shadow-metadata-preparation-sql-v1",
    argument: proofOne.proofSha256, current: "proofOne", preparation: "preparationOne",
    expected: "error" },
  { name: "owner_finalization_sql", action: "shadow-metadata-finalization-sql-v1",
    argument: preparationOne.preparationSha256, current: "proofOne",
    preparation: "preparationOne", expected: "success" },
  { name: "owner_finalization_sql_two", action: "shadow-metadata-finalization-sql-v1",
    argument: preparationTwo.preparationSha256, current: "proofTwo",
    preparation: "preparationTwo", expected: "success" },
  { name: "owner_proof_argument_mismatch", action: "shadow-metadata-preparation-identity-v1",
    argument: proofTwo.proofSha256, current: "proofOne", preparation: "-", expected: "error" },
  { name: "owner_preparation_argument_mismatch", action: "shadow-metadata-finalization-sql-v1",
    argument: preparationTwo.preparationSha256, current: "proofOne",
    preparation: "preparationOne", expected: "error" },
  { name: "owner_proof_absent", action: "shadow-metadata-preparation-identity-v1",
    argument: proofOne.proofSha256, current: "-", preparation: "-", expected: "error" },
];
const ownerMutations = [
  { name: "owner_prepare_noncanonical", action: "prepare-shadow-metadata-custody-v1",
    now: clocks[4], argument: proofOne.proofSha256, input: "ownerPrepareInputOne",
    current: "proofOne", preparation: "-", previous: "-", previousPreparation: "-",
    consumption: "-", expected: "preparationOne", afterCurrent: "proofOne",
    afterPreparation: "preparationOne", afterPrevious: "-", afterPreviousPreparation: "-",
    afterConsumption: "-" },
  { name: "owner_consume_noncanonical", action: "consume-shadow-metadata-custody-v1",
    now: clocks[5], argument: preparationOne.preparationSha256, input: "ownerConsumeInputOne",
    current: "proofOne", preparation: "preparationOne", previous: "-",
    previousPreparation: "-", consumption: "-", expected: "consumptionOne",
    afterCurrent: "-", afterPreparation: "-", afterPrevious: "proofOne",
    afterPreviousPreparation: "preparationOne", afterConsumption: "consumptionOne" },
  { name: "owner_extra_after_recovery", action: "prepare-shadow-metadata-custody-v1",
    now: clocks[6], argument: proofOne.proofSha256, input: "ownerPrepareInputExtra",
    current: "proofOne", preparation: "preparationOne", previous: "proofOne",
    previousPreparation: "preparationOne", consumption: "consumptionOne", expected: "error",
    afterCurrent: "-", afterPreparation: "-", afterPrevious: "proofOne",
    afterPreviousPreparation: "preparationOne", afterConsumption: "consumptionOne" },
];
const root = await mkdtemp(join(tmpdir(), "weather-shadow-custody-mutation-oracle-"));
try {
  // isolate every command invocation in its own current-user fixture root
  for (const fixture of cases) await observe(root, fixture);
  for (const fixture of ownerQueries) await observeOwnerQuery(root, fixture);
  for (const fixture of ownerMutations) {
    const [, bytes] = rawInputs[fixture.input];
    // assert the post-recovery refusal separately because the store throws before output
    if (fixture.expected === "error") {
      await assert.rejects(observeOwnerMutation(root, fixture, bytes));
      await requireSlots(join(root, fixture.name, "shadow-metadata-custody-proofs"), [
        fixture.afterCurrent, fixture.afterPreparation, fixture.afterPrevious,
        fixture.afterPreviousPreparation, fixture.afterConsumption,
      ]);
      console.log(["owner-mutation", fixture.name, fixture.action, fixture.now, fixture.argument,
        fixture.input, fixture.current, fixture.preparation, fixture.previous,
        fixture.previousPreparation, fixture.consumption, fixture.expected, fixture.afterCurrent,
        fixture.afterPreparation, fixture.afterPrevious, fixture.afterPreviousPreparation,
        fixture.afterConsumption].join("\t"));
    } else {
      await observeOwnerMutation(root, fixture, bytes);
    }
  }
} finally {
  await rm(root, { recursive: true, force: true });
}
