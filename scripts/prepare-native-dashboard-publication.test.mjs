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
  NATIVE_DASHBOARD_PUBLICATION_CONTRACT_VERSION,
  NATIVE_DASHBOARD_PUBLICATION_REQUEST_VERSION,
  prepareNativeDashboardPublication,
  validateNativeDashboardPublication,
} from "./prepare-native-dashboard-publication.mjs";
import { nativeCanonicalSha256 } from "./schema-preserving-release.mjs";

const SCRIPT = new URL("./prepare-native-dashboard-publication.mjs", import.meta.url).pathname;
const INSTALLED_VALIDATOR = new URL("./schema-preserving-release.mjs", import.meta.url).pathname;
const MATERIAL_FIXTURE = new URL(
  "../deploy/scripts/rust-runtime-image-material.fixture.json",
  import.meta.url,
);
const CORE_COMMIT = "a".repeat(40);
const TARGET_COMMIT = "b".repeat(40);
const CORE_RELEASE = "2099.01.01-1";
const TARGET_RELEASE = "2099.01.01-2";

// build one independently inspected native image-material fixture
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

// retain the predecessor protected membership in canonical order
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

// build one complete web-only native publication request
function requestFixture() {
  const imageMaterial = inspectedMaterial();
  const releaseEvidence = {
    check: {
      conclusion: "success",
      databaseId: 301,
      event: "push",
      headSha: TARGET_COMMIT,
      name: "Check",
      status: "completed",
      url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/301",
    },
    operatorSha256: "c".repeat(64),
    publish: {
      conclusion: "success",
      databaseId: 302,
      event: "push",
      headBranch: TARGET_RELEASE,
      headSha: TARGET_COMMIT,
      name: "Publish production images",
      status: "completed",
      url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/302",
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
  const coreTree = protectedTree();
  return {
    coreCommit: CORE_COMMIT,
    corePublicationSha256: "3".repeat(64),
    coreRelease: CORE_RELEASE,
    coreTree,
    imagePublicationProof,
    releaseEvidence,
    requestVersion: NATIVE_DASHBOARD_PUBLICATION_REQUEST_VERSION,
    sourceCommit: CORE_COMMIT,
    sourcePublicationSha256: "3".repeat(64),
    sourceRelease: CORE_RELEASE,
    sourceWebImage: `ghcr.io/anstosa/weather-web@sha256:${"4".repeat(64)}`,
    sourceWebImageId: `sha256:${"5".repeat(64)}`,
    targetTree: structuredClone(coreTree),
  };
}

// write one private bounded request for the command interface
function privateJson(directory, value) {
  const path = join(directory, "request.json");
  writeFileSync(path, `${JSON.stringify(value)}\n`, { mode: 0o600 });
  chmodSync(path, 0o600);
  return path;
}

// prove the publication binds web identity while retaining the exact core tree
test("prepares one exact native dashboard publication", () => {
  const request = requestFixture();
  const publication = prepareNativeDashboardPublication(request);
  assert.equal(publication.contractVersion, NATIVE_DASHBOARD_PUBLICATION_CONTRACT_VERSION);
  assert.equal(publication.coreRelease, CORE_RELEASE);
  assert.equal(publication.sourceRelease, CORE_RELEASE);
  assert.equal(publication.targetRelease, TARGET_RELEASE);
  assert.deepEqual(publication.coreTree, publication.targetTree);
  assert.equal(
    validateNativeDashboardPublication(
      publication,
      TARGET_RELEASE,
      CORE_RELEASE,
      CORE_RELEASE,
    ),
    publication,
  );
});

// prove the actual CLI emits and reads the admitted publication deterministically
test("CLI prepares validates and projects one dashboard publication", () => {
  const request = requestFixture();
  const directory = mkdtempSync(join(tmpdir(), "native-dashboard-publication-"));
  try {
    const requestPath = privateJson(directory, request);
    const prepared = spawnSync(process.execPath, [SCRIPT, requestPath], { encoding: "utf8" });
    assert.equal(prepared.status, 0, prepared.stderr);
    const publication = JSON.parse(prepared.stdout);
    const publicationPath = join(directory, "publication.json");
    writeFileSync(publicationPath, `${JSON.stringify(publication)}\n`, { mode: 0o600 });
    const validated = spawnSync(process.execPath, [
      INSTALLED_VALIDATOR,
      "validate-dashboard-publication",
      publicationPath,
      TARGET_RELEASE,
      CORE_RELEASE,
      CORE_RELEASE,
    ], { encoding: "utf8" });
    assert.equal(validated.status, 0, validated.stderr);
    const field = spawnSync(process.execPath, [
      INSTALLED_VALIDATOR,
      "field",
      publicationPath,
      "targetWebImage",
    ], { encoding: "utf8" });
    assert.equal(field.status, 0, field.stderr);
    assert.equal(field.stdout.trim(), request.releaseEvidence.webImage);
  } finally {
    rmSync(directory, { force: true, recursive: true });
  }
});

// prove a dashboard publication cannot mutate core-protected source bytes
test("rejects protected tree and independent image drift", () => {
  const treeDrift = requestFixture();
  treeDrift.targetTree[0].sha256 = "9".repeat(64);
  assert.throws(
    () => prepareNativeDashboardPublication(treeDrift),
    /protected tree drifted/u,
  );

  const imageDrift = requestFixture();
  imageDrift.imagePublicationProof.publishedImages[1].configId = `sha256:${"8".repeat(64)}`;
  assert.throws(
    () => prepareNativeDashboardPublication(imageDrift),
    /publishedImages\[1\] differs/u,
  );
});
