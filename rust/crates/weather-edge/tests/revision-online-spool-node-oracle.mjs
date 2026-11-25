import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import vm from "node:vm";

const source = await readFile(
  new URL("../../../../deploy/scripts/adjustment-evidence-store.mjs", import.meta.url),
  "utf8",
);
const start = source.indexOf("  async #admitOnlinePayload(path, bytes, replacementPath)");
const end = source.indexOf("  // serialize all payload admissions", start);
assert.ok(start >= 0 && end > start);
const method = source.slice(start, end)
  .replace("async #admitOnlinePayload(", "async function admitOnlinePayload(")
  .replaceAll("this.#root", "this.root");
const directory = {
  capsule: "shadow-revision-capsules",
  projection: "revision-projections",
  stage: "shadow-revision-stages",
  state: "rain-control-states",
};
const root = "/fixture-revision-spool";
// derive a fixed disposable file identity from one fixture digest byte
function file(kind, digest) {
  return join(root, directory[kind], `sha256-${digest.repeat(64)}.json`);
}

const projectionBytes = 3_500 * 1024;
const matchingCapsuleBytes = 1_332 * 1024;
const cases = [
  ["first", [], ["state", "a", 4], null],
  ["retry", [
    ["projection", "b", projectionBytes],
    ["capsule", "c", matchingCapsuleBytes],
  ], ["capsule", "c", matchingCapsuleBytes], null],
  ["third", [
    ["projection", "b", projectionBytes],
    ["capsule", "c", matchingCapsuleBytes],
  ], ["state", "a", 4], null],
  ["exact-ceiling", [["projection", "b", projectionBytes]],
    ["capsule", "c", matchingCapsuleBytes], null],
  ["over-ceiling", [["projection", "b", projectionBytes]],
    ["capsule", "c", matchingCapsuleBytes + 1], null],
  ["stage-replacement", [["projection", "b", projectionBytes], ["stage", "c", 512]],
    ["capsule", "d", matchingCapsuleBytes], ["stage", "c"]],
  ["other-stage-still-charged", [
    ["projection", "b", projectionBytes], ["stage", "c", 512],
  ], ["capsule", "d", matchingCapsuleBytes], ["stage", "e"]],
];

for (const [label, files, incoming, replacement] of cases) {
  const trace = [];
  const members = new Map(files.map(
    // retain normal immutable payload byte counts in source directory order
    ([kind, digest, bytes]) => [file(kind, digest), bytes],
  ));
  const enabled = vm.runInNewContext(`${method}; admitOnlinePayload`, {
    ADJUSTMENT_REVISION_PROJECTION_MAXIMUM_BYTES: projectionBytes,
    ADJUSTMENT_RAIN_CONTROL_STATE_MAXIMUM_BYTES: 32 * 1024,
    ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_SLOTS: 2,
    ADJUSTMENT_REVISION_ONLINE_PAYLOAD_MAXIMUM_BYTES: 4_832 * 1024,
    join,
    // return actual bytes only for exact retry comparison
    async readOptionalPrivateFile(path) {
      trace.push("target");
      const length = members.get(path);
      return length === undefined ? null : Buffer.alloc(length, 1);
    },
    // list only this exact fixed-directory fixture
    async readdir(path) {
      trace.push(`list:${basename(path)}`);
      return [...members.keys()].filter(
        // select only the retained members of this directory
        (member) => dirname(member) === path,
      ).map(
        // preserve the fixed digest basename
        (member) => basename(member),
      );
    },
    // observe Node reading existing members only for their length
    async readRegularFile(path, maximum) {
      trace.push(`size:${basename(dirname(path))}`);
      const length = members.get(path);
      assert.ok(length <= maximum);
      return Buffer.alloc(length, 1);
    },
  });
  const [kind, digest, size] = incoming;
  let outcome = "admitted";
  try {
    await enabled.call({ root }, file(kind, digest), Buffer.alloc(size, 1),
      replacement === null ? null : file(...replacement));
  } catch (error) {
    assert.equal(error.code, "adjustment_revision_spool_refused");
    outcome = "capacity";
  }
  console.log(`${label}=${outcome}|${trace.join(",")}`);
}
