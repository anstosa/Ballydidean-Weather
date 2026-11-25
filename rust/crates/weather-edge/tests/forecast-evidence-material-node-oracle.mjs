import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { gunzipSync, gzipSync } from "node:zlib";

import {
  AdjustmentEvidenceStore,
  freezeAdjustmentEvidenceSnapshot,
  ADJUSTMENT_EVIDENCE_OBJECT_V2_CONTRACT_VERSION,
  ADJUSTMENT_EVIDENCE_RECEIPT_CONTRACT_VERSION,
  ADJUSTMENT_EVIDENCE_RECEIPT_V2_CONTRACT_VERSION,
  createAdjustmentEvidenceCapture,
  createAdjustmentEvidenceCaptureV2,
} from "../../../../deploy/scripts/adjustment-evidence-store.mjs";

const now = "2026-10-07T08:54:05.500Z";
const storeSource = readFileSync(new URL("../../../../deploy/scripts/adjustment-evidence-store.mjs", import.meta.url), "utf8");
const testSource = readFileSync(new URL("../../../../deploy/test/adjustment-evidence-store.test.mjs", import.meta.url), "utf8");

// use the original producer functions without opening a storage root or listener
function span(source, startMarker, endMarker) {
  const start = source.indexOf(startMarker);
  const end = source.indexOf(endMarker, start);
  assert.ok(start >= 0 && end > start, startMarker);
  return source.slice(start, end);
}

const { canonicalJson, createReceipt } = Function(
  "gzipSync",
  "ADJUSTMENT_EVIDENCE_OBJECT_V2_CONTRACT_VERSION",
  "ADJUSTMENT_EVIDENCE_RECEIPT_V2_CONTRACT_VERSION",
  "ADJUSTMENT_EVIDENCE_RECEIPT_CONTRACT_VERSION",
  `${span(storeSource, "function canonicalJson(", "// encode one canonical newline-framed document")}
    ${span(storeSource, "\nfunction canonicalValue(", "// calculate one lowercase SHA-256 digest")}
    ${span(storeSource, "\nfunction requirePlainObject(", "// require exact closed object keys")}
    ${span(storeSource, "function createReceipt(", "// retain only active response-level serving identities")}
    return { canonicalJson, createReceipt }; `,
)(gzipSync, ADJUSTMENT_EVIDENCE_OBJECT_V2_CONTRACT_VERSION,
  ADJUSTMENT_EVIDENCE_RECEIPT_V2_CONTRACT_VERSION, ADJUSTMENT_EVIDENCE_RECEIPT_CONTRACT_VERSION);

// emit an opaque fixture value as typed lines, not a hand-written JSON codec
function describe(value, lines, clocks) {
  // retain only canonical instants as their original Node-parsed milliseconds
  if (typeof value === "string") {
    const epoch = Date.parse(value);
    if (Number.isFinite(epoch) && new Date(epoch).toISOString() === value) {
      clocks.set(value, epoch);
    }
  }
  if (value === null) {
    lines.push("n");
  } else if (typeof value === "string") {
    lines.push(`s${value.split("").map((character) => character.charCodeAt(0)).join(",")}`);
  } else if (typeof value === "number") {
    lines.push(`d${value}`);
  } else if (typeof value === "boolean") {
    lines.push(value ? "t" : "f");
  } else if (Array.isArray(value)) {
    lines.push(`a${value.length}`);
    for (const item of value) {
      describe(item, lines, clocks);
    }
  } else {
    const fields = Object.entries(value);
    lines.push(`o${fields.length}`);
    for (const [key, field] of fields) {
      describe(key, lines, clocks);
      describe(field, lines, clocks);
    }
  }
}

// decode only the fixture frame passed by rust before using the original Node codec
function readTyped(lines) {
  const token = lines.next().value;
  assert.equal(typeof token, "string");
  const text = token.slice(1);
  switch (token[0]) {
    case "n": return null;
    case "t": return true;
    case "f": return false;
    case "d": return Number(text);
    case "s": return String.fromCharCode(...(text === "" ? [] : text.split(",").map(Number)));
    case "a": return Array.from({ length: Number(text) }, () => readTyped(lines));
    case "o": return Object.fromEntries(Array.from({ length: Number(text) }, () => [
      readTyped(lines), readTyped(lines),
    ]));
    default: throw new Error(`unknown typed fixture token: ${token[0]}`);
  }
}

// publish a real Node predecessor object, channel, and stable binding in a disposable spool
async function channelFixture(body, prepared, initiallyLegacy) {
  const root = await mkdtemp(join(tmpdir(), "weather-node-forecast-channel-"));
  try {
    const store = await new AdjustmentEvidenceStore({
      root,
      statfs: async () => ({ bsize: 4096n, bavail: 600000n, ffree: 65536n }),
      now: () => new Date(now),
    }).initialize();
    const first = initiallyLegacy
      ? store.prepare(body, "days=1")
      : store.prepareV2(body, "days=1", "public_get");
    assert.equal((await store.commit(first, now)).status, "created");
    const again = store.prepareV2(body, "days=1", "scheduler_request");
    assert.equal((await store.commit(again, "2026-10-07T08:55:00.000Z")).status, "duplicate");
    const identity = prepared.edgeReceiptIdentitySha256;
    const stem = `sha256-${identity}.json`;
    const binding = await readFile(join(root, "channel-bindings", stem));
    const assertionAddress = JSON.parse(binding).assertionSha256;
    const assertion = await readFile(join(root, "channels", `sha256-${assertionAddress}.json`));
    const snapshot = await freezeAdjustmentEvidenceSnapshot({ root, now: () => new Date(now) });
    assert.equal(snapshot.entries.length, 1);
    assert.equal(snapshot.entries[0].edgeReceiptIdentitySha256, identity);
    const status = store.status();
    return {
      assertion,
      assertion_address: assertionAddress,
      binding,
      watermark: snapshot.watermarkSha256,
      snapshot_at: snapshot.frozenAt,
      archive_objects: String(status.objects),
      archive_channels: String(status.channelAssertions),
      archive_finished: String(status.finishedSuccesses),
      archive_incomplete: String(status.provenanceIncomplete),
    };
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

const [operation, version] = process.argv.slice(2);
if (operation === "fixture") {
  const { forecastRow, forecastBody } = Function(
    `${span(testSource, "const hashes = {", "// prove a root-authorized initializer")}
      ${span(testSource, "function forecastRow(", "// provide measured production headroom")}
      return { forecastRow, forecastBody }; `,
  )();
  const extended = version === "extended";
  const rows = Array.from({ length: extended ? 241 : 2 }, (_, index) => forecastRow(index));
  // alter only a volatile response clock to prove the first receipt and object identities survive
  if (version === "clock") {
    rows[0].receivedAt = "2026-10-07T00:07:00.000Z";
  }
  const input = forecastBody(rows, { days: extended ? 10 : 1 });
  const captured = extended
    ? createAdjustmentEvidenceCaptureV2(input, "days=10")
    : createAdjustmentEvidenceCapture(input, "days=1");
  const lines = [];
  const later = "2026-10-07T08:55:00.000Z";
  const clocks = new Map([[now, Date.parse(now)], [later, Date.parse(later)]]);
  describe(JSON.parse(input), lines, clocks);
  const entries = {
    input,
    typed: lines.join("\n"),
    clocks: [...clocks].map(([key, epoch]) => `${key}=${epoch}`).join("\n"),
    canonical: canonicalJson(captured.object),
    gzip: captured.compressedObject,
    object: captured.objectSha256,
    identity: captured.edgeReceiptIdentitySha256,
    receipt: `${canonicalJson(createReceipt(captured, now))}\n`,
  };
  // keep ordinary fixture modes cheap and add exact filesystem evidence only for channel cases
  if (version === "channel-public" || version === "channel-legacy") {
    Object.assign(entries, await channelFixture(input, captured, version === "channel-legacy"));
  }
  for (const [label, value] of Object.entries(entries)) {
    process.stdout.write(`${label}=${Buffer.from(value).toString("hex")}\n`);
  }
} else {
  const input = readFileSync(0);
  switch (operation) {
    case "describe": {
      const lines = [];
      describe(JSON.parse(input.toString("utf8")), lines, new Map());
      process.stdout.write(lines.join("\n"));
      break;
    }
    case "gunzip":
      process.stdout.write(gunzipSync(input, { maxOutputLength: Number(version) }));
      break;
    case "base64-decode":
      process.stdout.write(Buffer.from(input.toString("utf8"), "base64"));
      break;
    case "canonical":
      process.stdout.write(canonicalJson(readTyped(input.toString("utf8").trimEnd().split("\n").values())));
      break;
    case "scheduler-clocks": {
      const clocks = new Map();
      const include = (epoch) => clocks.set(new Date(epoch).toISOString(), epoch);
      // supply only exact source UTC times for the three disposable lifecycle dates
      for (const date of ["2026-10-07", "2026-10-08", "2026-10-09"]) {
        include(Date.parse(`${date}T00:00:00.000Z`));
        for (const hour of ["00", "06", "12", "18"]) {
          include(Date.parse(`${date}T${hour}:40:00.000Z`));
          const base = `${date}T${hour}:35:00.000Z`;
          const epoch = Date.parse(base);
          const digest = createHash("sha256").update(`ballydidean|capture/${base}`).digest();
          include(epoch);
          include(epoch + (digest.readUInt32BE(0) % 300) * 1000);
        }
      }
      include(Date.parse("2028-10-08T07:00:00.000Z"));
      process.stdout.write([...clocks].map(([time, epoch]) => `${time}=${epoch}\n`).join(""));
      break;
    }
    case "hash":
      process.stdout.write(createHash("sha256").update(input).digest("hex"));
      break;
    case "gzip":
      process.stdout.write(gzipSync(input, { level: 9, mtime: 0 }));
      break;
    case "base64":
      process.stdout.write(input.toString("base64"));
      break;
    default:
      throw new Error(`unknown Node fixture operation: ${operation}`);
  }
}
