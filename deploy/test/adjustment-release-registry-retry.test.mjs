import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";
import { gzipSync } from "node:zlib";
import {
  cacheReleaseLayerMeasurement,
  measureReleaseLayer,
  retryWeatherRegistryObject,
} from "../scripts/adjustment-evaluation-package.mjs";

const digest = `sha256:${"a".repeat(64)}`;

// create one transport timeout without relying on a live timer
function timeout() {
  return new DOMException("The operation was aborted due to timeout", "TimeoutError");
}

// encode one minimal regular-file tar layer
function layerBytes() {
  const header = Buffer.alloc(512);
  header.write("file", 0, "ascii");
  header.write("00000000001\0", 124, "ascii");
  header.fill(0x20, 148, 156);
  header[156] = "0".charCodeAt(0);
  header.write("ustar\0", 257, "binary");
  header.write("00", 263, "ascii");
  // sum the canonical header bytes
  const checksum = [...header].reduce((total, byte) => total + byte, 0);
  header.write(`${checksum.toString(8).padStart(6, "0")}\0 `, 148, "binary");
  return Buffer.concat([header, Buffer.from("x"), Buffer.alloc(511), Buffer.alloc(1024)]);
}

// describe one complete compressed layer
function layerFixture() {
  const unpacked = layerBytes();
  const compressed = gzipSync(unpacked);
  return {
    compressed,
    descriptor: {
      digest: `sha256:${createHash("sha256").update(compressed).digest("hex")}`,
      size: compressed.length,
    },
    diffId: `sha256:${createHash("sha256").update(unpacked).digest("hex")}`,
  };
}

// retry one complete registry-object read after a body timeout
test("registry object timeout retries one complete read", async () => {
  let attempts = 0;
  const value = await retryWeatherRegistryObject(
    "weather-server",
    "blobs",
    digest,
    async () => {
      attempts += 1;
      let firstChunk = true;
      const response = attempts === 1 ? new Response(new ReadableStream({
        // deliver one partial chunk before the body deadline
        pull(controller) {
          // abort only after partial bytes were consumed
          if (firstChunk) {
            firstChunk = false;
            controller.enqueue(Buffer.from("partial"));
          } else {
            controller.error(timeout());
          }
        },
      })) : new Response("complete");
      const bytes = Buffer.from(await response.arrayBuffer());
      return { digest, bytes: bytes.toString("utf8") };
    },
  );

  assert.deepEqual(value, { digest, bytes: "complete" });
  assert.equal(attempts, 2);
});

// retain only closed identity context after the bounded retry is exhausted
test("registry object second timeout fails with sanitized identity", async () => {
  let attempts = 0;
  await assert.rejects(
    retryWeatherRegistryObject("weather-web", "manifests", digest, async () => {
      attempts += 1;
      throw timeout();
    }),
    (error) => {
      assert.equal(error.name, "TimeoutError");
      assert.match(error.message, /weather-web manifests sha256:[a-f0-9]{64}.*attempt 2/u);
      assert.doesNotMatch(error.message, /https:|authorization|token|sig=/iu);
      return true;
    },
  );
  assert.equal(attempts, 2);
});

// reject real layer-integrity failures without retrying
for (const scenario of [
  {
    name: "digest mismatch",
    expected: /release layer digest or size differs/u,
    // corrupt only the compressed descriptor digest
    mutate(fixture) {
      fixture.descriptor.digest = digest;
    },
  },
  {
    name: "size mismatch",
    expected: /release layer digest or size differs/u,
    // corrupt only the compressed descriptor size
    mutate(fixture) {
      fixture.descriptor.size += 1;
    },
  },
  {
    name: "tar framing failure",
    expected: /release layer tar framing is incomplete/u,
    // replace the layer with an empty archive
    mutate(fixture) {
      const unpacked = Buffer.alloc(1024);
      fixture.compressed = gzipSync(unpacked);
      fixture.descriptor = {
        digest: `sha256:${createHash("sha256").update(fixture.compressed).digest("hex")}`,
        size: fixture.compressed.length,
      };
      fixture.diffId = `sha256:${createHash("sha256").update(unpacked).digest("hex")}`;
    },
  },
]) {
  test(`registry object does not retry ${scenario.name}`, async () => {
    const fixture = layerFixture();
    scenario.mutate(fixture);
    let attempts = 0;
    await assert.rejects(
      retryWeatherRegistryObject("weather-server", "blobs", digest, async () => {
        attempts += 1;
        return measureReleaseLayer(
          new Response(fixture.compressed),
          fixture.descriptor,
          fixture.diffId,
        );
      }),
      scenario.expected,
    );
    assert.equal(attempts, 1);
  });
}

// reject one non-timeout transport refusal without retrying
test("registry object does not retry a non-timeout refusal", async () => {
  let attempts = 0;
  await assert.rejects(
    retryWeatherRegistryObject("weather-server", "blobs", digest, async () => {
      attempts += 1;
      throw new Error("registry blobs request failed");
    }),
    /registry blobs request failed/u,
  );
  assert.equal(attempts, 1);
});

// publish no partial measurement after an exhausted body retry
test("registry layer cache remains empty after timeout failure", async () => {
  const measurements = new Map();
  let attempts = 0;
  await assert.rejects(
    cacheReleaseLayerMeasurement(measurements, `${digest}/${digest}`, async () =>
      retryWeatherRegistryObject("weather-server", "blobs", digest, async () => {
        attempts += 1;
        throw timeout();
      })),
    /attempt 2/u,
  );
  assert.equal(attempts, 2);
  assert.equal(measurements.size, 0);
});

// reuse only one completed immutable measurement
test("registry layer cache retains only a completed measurement", async () => {
  const measurements = new Map();
  let loads = 0;
  const load = async () => {
    loads += 1;
    return { allocatedBytes: 4096, blobDigest: digest, entryInodes: 1 };
  };
  const first = await cacheReleaseLayerMeasurement(measurements, `${digest}/${digest}`, load);
  const second = await cacheReleaseLayerMeasurement(measurements, `${digest}/${digest}`, load);
  assert.equal(first, second);
  assert.equal(loads, 1);
  assert.equal(measurements.size, 1);
});
