import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import {
  RUNTIME_IMAGE_MATERIAL_INSPECTION_CLASS,
  buildRustRuntimeImageMaterial,
} from "../deploy/scripts/rust-runtime-image-material.mjs";
import {
  NATIVE_DESCENDANT_PUBLICATION_REQUEST_VERSION,
  prepareNativeDescendantPublication,
} from "./prepare-native-descendant-publication.mjs";
import {
  NATIVE_DESCENDANT_PUBLICATION_CONTRACT_VERSION,
  nativeCanonicalSha256,
  validatePublication,
} from "./schema-preserving-release.mjs";

const SCRIPT = new URL("./prepare-native-descendant-publication.mjs", import.meta.url).pathname;
const MATERIAL_FIXTURE = new URL(
  "../deploy/scripts/rust-runtime-image-material.fixture.json",
  import.meta.url,
);
const SOURCE_COMMIT = "a".repeat(40);
const TARGET_COMMIT = "b".repeat(40);
const SOURCE_RELEASE = "2099.01.01-1";
const TARGET_RELEASE = "2099.01.01-2";

// build one inspected native image-material fixture
function inspectedMaterial() {
  const input = JSON.parse(readFileSync(MATERIAL_FIXTURE, "utf8"));
  input.source.commit = TARGET_COMMIT;
  const material = buildRustRuntimeImageMaterial(input);
  material.artifactClass = RUNTIME_IMAGE_MATERIAL_INSPECTION_CLASS;
  material.inspection = {
    extractedFileBytes: true,
    provesBuildIdentity: false,
    provesElfArchitecture: true,
    provesImageIdentity: false,
    provesPlatform: false,
    provesSourceIdentity: false,
  };
  return material;
}

// build the original closed protected-root membership in canonical order
function protectedTree() {
  return [
    "Dockerfile",
    "deploy/scripts/common.sh",
    "deploy/scripts/update.sh",
    "package-lock.json",
    "packages/database/migrations/0020_adjustment_revision_frontier.sql",
    "packages/database/migrations/0023_forecast_product_read_index.sql",
    "packages/forecast-adjustment/src/maintenance-capture-epoch.ts",
  ].map((path, index) => ({ path, sha256: String(index + 1).repeat(64) }));
}

// build one complete ordinary Rust descendant request
function requestFixture() {
  const imageMaterial = inspectedMaterial();
  const releaseEvidence = {
    check: {
      conclusion: "success",
      databaseId: 201,
      event: "push",
      headSha: TARGET_COMMIT,
      name: "Check",
      status: "completed",
      url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/201",
    },
    operatorSha256: "c".repeat(64),
    publish: {
      conclusion: "success",
      databaseId: 202,
      event: "push",
      headBranch: TARGET_RELEASE,
      headSha: TARGET_COMMIT,
      name: "Publish production images",
      status: "completed",
      url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/202",
    },
    serverImage: `ghcr.io/anstosa/weather-server@sha256:${"d".repeat(64)}`,
    serverImageId: `sha256:${"e".repeat(64)}`,
    targetCommit: TARGET_COMMIT,
    targetRelease: TARGET_RELEASE,
    validatorSha256: "f".repeat(64),
    webImage: `ghcr.io/anstosa/weather-web@sha256:${"1".repeat(64)}`,
    webImageId: `sha256:${"2".repeat(64)}`,
  };
  const imagePublicationProof = {
    contractVersion: "weather-native-image-publication-proof/v1",
    generation: "rust-v1",
    imageMaterial,
    imageMaterialSha256: nativeCanonicalSha256(imageMaterial),
    publishedImages: [
      {
        configId: releaseEvidence.serverImageId,
        materialManifestSha256: imageMaterial.images[0].materialManifestSha256,
        name: "server",
        reference: releaseEvidence.serverImage,
      },
      {
        configId: releaseEvidence.webImageId,
        materialManifestSha256: imageMaterial.images[1].materialManifestSha256,
        name: "web",
        reference: releaseEvidence.webImage,
      },
    ],
    release: TARGET_RELEASE,
    sourceCommit: TARGET_COMMIT,
  };
  const sourceTree = protectedTree();
  return {
    imagePublicationProof,
    releaseEvidence,
    requestVersion: NATIVE_DESCENDANT_PUBLICATION_REQUEST_VERSION,
    sourceCommit: SOURCE_COMMIT,
    sourcePublicationSha256: "3".repeat(64),
    sourceRelease: SOURCE_RELEASE,
    sourceTree,
    targetTree: structuredClone(sourceTree),
  };
}

// write one bounded request for the actual command interface
function privateJson(directory, value) {
  const path = join(directory, "request.json");
  writeFileSync(path, `${JSON.stringify(value)}\n`, { mode: 0o600 });
  chmodSync(path, 0o600);
  return path;
}

// prove the pure projection binds direct source identity and equal protected trees
test("prepares one exact ordinary native descendant", () => {
  const request = requestFixture();
  const publication = prepareNativeDescendantPublication(request);
  assert.equal(
    publication.contractVersion,
    NATIVE_DESCENDANT_PUBLICATION_CONTRACT_VERSION,
  );
  assert.equal(publication.sourceGeneration, "rust-v1");
  assert.equal(publication.targetGeneration, "rust-v1");
  assert.equal(publication.sourceCommit, SOURCE_COMMIT);
  assert.equal(publication.sourcePublicationSha256, request.sourcePublicationSha256);
  assert.deepEqual(publication.sourceTree, publication.targetTree);
  assert.equal(validatePublication(publication, TARGET_RELEASE), publication);
});

// prove the actual CLI emits the same admitted descendant document
test("CLI emits the same deterministic native descendant", () => {
  const request = requestFixture();
  const directory = mkdtempSync(join(tmpdir(), "native-descendant-publication-"));
  try {
    const result = spawnSync(process.execPath, [SCRIPT, privateJson(directory, request)], {
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), prepareNativeDescendantPublication(request));
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});

// prove the installed shared validator CLI admits the new closed contract
test("installed validator CLI accepts the native descendant", () => {
  const publication = prepareNativeDescendantPublication(requestFixture());
  const directory = mkdtempSync(join(tmpdir(), "native-descendant-validator-"));
  const installed = new URL("./schema-preserving-release.mjs", import.meta.url).pathname;
  try {
    const path = join(directory, "publication.json");
    writeFileSync(path, `${JSON.stringify(publication, null, 2)}\n`, { mode: 0o600 });
    chmodSync(path, 0o600);
    const result = spawnSync(process.execPath, [
      installed,
      "validate-publication",
      path,
      TARGET_RELEASE,
    ], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout), publication);
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});

// prove protected membership and bytes cannot drift between native releases
test("rejects every protected tree change", () => {
  const changed = requestFixture();
  changed.targetTree[0].sha256 = "9".repeat(64);
  assert.throws(
    () => prepareNativeDescendantPublication(changed),
    /protected tree drifted/u,
  );

  const removed = requestFixture();
  removed.targetTree.pop();
  assert.throws(
    () => prepareNativeDescendantPublication(removed),
    /missing required path|length drifted/u,
  );
});

// prove source, workflow and image identities cannot be self-rebound
test("rejects source and independent publication evidence drift", () => {
  const sameCommit = requestFixture();
  sameCommit.sourceCommit = TARGET_COMMIT;
  assert.throws(
    () => prepareNativeDescendantPublication(sameCommit),
    /identity differs/u,
  );

  const failedCheck = requestFixture();
  failedCheck.releaseEvidence.check.conclusion = "failure";
  assert.throws(
    () => prepareNativeDescendantPublication(failedCheck),
    /exact successful target push/u,
  );

  const changedImage = requestFixture();
  changedImage.imagePublicationProof.publishedImages[0].configId = `sha256:${"8".repeat(64)}`;
  assert.throws(
    () => prepareNativeDescendantPublication(changedImage),
    /publishedImages\[0\] differs/u,
  );
});
