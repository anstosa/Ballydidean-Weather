import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import test from "node:test";
import {
  RUNTIME_IMAGE_MATERIAL_INSPECTION_CLASS,
  buildRustRuntimeImageMaterial,
} from "../deploy/scripts/rust-runtime-image-material.mjs";
import {
  BOOTSTRAP_APPROVAL_ARTIFACT_VERSION,
  NATIVE_PUBLICATION_REQUEST_VERSION,
  canonicalJsonBytes,
  prepareNativeSchemaPreservingPublication,
} from "./prepare-native-schema-preserving-publication.mjs";
import {
  DECLARED_TRANSITION_CONTRACT_VERSION,
  buildReviewedTransitionDeclaration,
  canonicalSha256sumManifest,
  nativeCanonicalSha256,
  sha256,
} from "./schema-preserving-release.mjs";

const repoRoot = resolve(import.meta.dirname, "..");
const shellSource = join(repoRoot, "scripts/schema-preserving-release.sh");
const moduleSource = join(repoRoot, "scripts/schema-preserving-release.mjs");
const transitionFixture = join(repoRoot, "scripts/predecessor-rust-transition.fixture.json");
const materialFixture = join(
  repoRoot,
  "deploy/scripts/rust-runtime-image-material.fixture.json",
);
const controlSha256 = "6ea5cf5b770904bdfa557b29af2dd8899701d8f1b4a15b632f55ab5121032cbb";

// encode one stable private fixture document
function writePrivate(path, value) {
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, { mode: 0o600 });
  chmodSync(path, 0o600);
  return path;
}

// derive the real two-file installed bundle identity
function operatorBundle(moduleBytes, shellBytes) {
  const members = [
    { path: "schema-preserving-release.mjs", sha256: sha256(moduleBytes) },
    { path: "schema-preserving-release.sh", sha256: sha256(shellBytes) },
  ];
  const sha256sum = canonicalSha256sumManifest(members);
  return {
    contractVersion: "weather-operator-bundle-manifest/v1",
    manifestSha256: sha256(Buffer.from(sha256sum, "utf8")),
    members,
    sha256sum,
  };
}

// build the exact finite transition over retained old and installed new bytes
function finiteTransition({ oldBundle, oldReceiptSha256, sourceSha256, targetBundle }) {
  const transition = JSON.parse(readFileSync(transitionFixture, "utf8"));
  transition.contractVersion = DECLARED_TRANSITION_CONTRACT_VERSION;
  transition.predecessor.bundleSha256 = oldBundle.manifestSha256;
  transition.predecessor.controlSha256 = controlSha256;
  transition.predecessor.installationReceiptSha256 = oldReceiptSha256;
  transition.predecessor.sourceEnvironmentSha256 = sourceSha256;
  const predecessorOperators = oldBundle.members.map((member) => ({
    path: `scripts/${member.path}`,
    sha256: member.sha256,
  }));
  const targetOperators = targetBundle.members.map((member) => ({
    path: `scripts/${member.path}`,
    sha256: member.sha256,
  }));
  transition.predecessorMembers.push(...predecessorOperators);
  transition.targetMembers.push(...targetOperators);
  transition.predecessorMembers.sort(
    // preserve canonical path order
    (left, right) => left.path < right.path ? -1 : 1,
  );
  transition.targetMembers.sort(
    // preserve canonical path order
    (left, right) => left.path < right.path ? -1 : 1,
  );
  transition.changedMembers = predecessorOperators.map((member, index) => ({
    path: member.path,
    predecessorSha256: member.sha256,
    targetSha256: targetOperators[index].sha256,
  }));
  transition.reviewedDeclaration = buildReviewedTransitionDeclaration({
    addedFiles: [],
    changedMembers: structuredClone(transition.changedMembers),
    predecessorCommit: transition.predecessor.commit,
    targetCommit: transition.target.commit,
  });
  return transition;
}

// bind the existing extracted-material grammar to the fixture target commit
function inspectedMaterial(targetCommit) {
  const input = JSON.parse(readFileSync(materialFixture, "utf8"));
  input.source.commit = targetCommit;
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

// prepare a genuine v3 publication from the deterministic finite approval
function nativePublication(transition, targetBundle) {
  const transitionSha256 = nativeCanonicalSha256(transition);
  const imageMaterial = inspectedMaterial(transition.target.commit);
  const releaseEvidence = {
    check: {
      conclusion: "success",
      databaseId: 301,
      event: "push",
      headSha: transition.target.commit,
      name: "Check",
      status: "completed",
      url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/301",
    },
    operatorSha256: targetBundle.members[1].sha256,
    publish: {
      conclusion: "success",
      databaseId: 302,
      event: "push",
      headBranch: transition.target.release,
      headSha: transition.target.commit,
      name: "Publish production images",
      status: "completed",
      url: "https://github.com/anstosa/Ballydidean-Weather/actions/runs/302",
    },
    serverImage: `ghcr.io/anstosa/weather-server@sha256:${"a".repeat(64)}`,
    serverImageId: `sha256:${"c".repeat(64)}`,
    targetCommit: transition.target.commit,
    targetRelease: transition.target.release,
    validatorSha256: targetBundle.members[0].sha256,
    webImage: `ghcr.io/anstosa/weather-web@sha256:${"b".repeat(64)}`,
    webImageId: `sha256:${"d".repeat(64)}`,
  };
  const imagePublicationProof = {
    contractVersion: "weather-native-image-publication-proof/v1",
    generation: "rust-v1",
    imageMaterial,
    imageMaterialSha256: sha256(canonicalJsonBytes(imageMaterial)),
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
    release: transition.target.release,
    sourceCommit: transition.target.commit,
  };
  return prepareNativeSchemaPreservingPublication({
    bootstrapArtifact: {
      approvalClass: "operator-reviewed-finite-transition",
      approvedTransitionSha256: transitionSha256,
      artifactVersion: BOOTSTRAP_APPROVAL_ARTIFACT_VERSION,
      targetOperatorBundleSha256: targetBundle.manifestSha256,
      transition,
    },
    imagePublicationProof,
    releaseEvidence,
    requestVersion: NATIVE_PUBLICATION_REQUEST_VERSION,
    sourceGeneration: "node-v1",
    targetGeneration: "rust-v1",
  });
}

// create one disposable installed-operator layout
function bootstrapFixture() {
  const root = mkdtempSync(join(tmpdir(), "weather-native-bootstrap-"));
  const state = join(root, "deploy/state");
  const releases = join(root, "deploy/releases");
  const data = join(root, "var/lib/weather/xweather");
  const spRoot = join(state, "schema-preserving-release-v1");
  // create the fixed retained state layout
  for (const directory of [
    state,
    releases,
    data,
    spRoot,
    join(state, "adjustment-release-state"),
    join(spRoot, "installations"),
  ]) {
    mkdirSync(directory, { mode: 0o700, recursive: true });
  }
  const rewrittenShell = readFileSync(shellSource, "utf8")
    .replace("/opt/weather/current/deploy/state/schema-preserving-release-v1", spRoot)
    .replaceAll("/opt/weather/current/deploy/state", state)
    .replaceAll("/opt/weather/current/deploy/releases", releases)
    .replace("/var/lib/weather/xweather/forecast-adjustment-settings.json", join(data, "forecast-adjustment-settings.json"));
  const shellBytes = Buffer.from(rewrittenShell, "utf8");
  const moduleBytes = readFileSync(moduleSource);
  const targetBundle = operatorBundle(moduleBytes, shellBytes);
  const targetPath = join(root, "operators", targetBundle.manifestSha256);
  mkdirSync(targetPath, { mode: 0o755, recursive: true });
  writeFileSync(join(targetPath, "schema-preserving-release.mjs"), moduleBytes, { mode: 0o644 });
  writeFileSync(join(targetPath, "schema-preserving-release.sh"), shellBytes, { mode: 0o755 });

  const oldModuleBytes = Buffer.from("// retained predecessor validator\n", "utf8");
  const oldShellBytes = Buffer.from("#!/usr/bin/env bash\n# retained predecessor operator\n", "utf8");
  const oldBundle = operatorBundle(oldModuleBytes, oldShellBytes);
  const oldPath = join(root, "operators", oldBundle.manifestSha256);
  mkdirSync(oldPath, { mode: 0o755, recursive: true });
  writeFileSync(join(oldPath, "schema-preserving-release.mjs"), oldModuleBytes, { mode: 0o644 });
  writeFileSync(join(oldPath, "schema-preserving-release.sh"), oldShellBytes, { mode: 0o755 });

  const sourcePath = join(releases, "2099.01.01-1.env");
  const sourceBytes = Buffer.from([
    "WEATHER_RELEASE=2099.01.01-1",
    "WEATHER_SERVER_IMAGE=old-server",
    "WEATHER_WEB_IMAGE=old-web",
    "POSTGRES_IMAGE=postgres-image",
    "CLOUDFLARED_IMAGE=cloudflare-image",
    "",
  ].join("\n"), "utf8");
  writeFileSync(sourcePath, sourceBytes, { mode: 0o600 });
  // retain each protected predecessor state member
  for (const path of [
    join(state, "schema-release"),
    join(state, "adjustment-candidate-catalog.json"),
    join(state, "adjustment-capture-epoch.json"),
    join(state, "adjustment-release-state/current.json"),
    join(data, "forecast-adjustment-settings.json"),
  ]) {
    writeFileSync(path, `retained:${path.split("/").at(-1)}\n`, { mode: 0o600 });
  }

  const oldReceipt = {
    bundleSha256: oldBundle.manifestSha256,
    controlPlaneSha256: controlSha256,
    dropInSha256: "8".repeat(64),
  };
  const oldReceiptPath = writePrivate(
    join(spRoot, "installations", `${oldBundle.manifestSha256}.json`),
    oldReceipt,
  );
  const targetReceiptPath = writePrivate(
    join(spRoot, "installations", `${targetBundle.manifestSha256}.json`),
    { bundleSha256: targetBundle.manifestSha256 },
  );
  const transition = finiteTransition({
    oldBundle,
    oldReceiptSha256: sha256(readFileSync(oldReceiptPath)),
    sourceSha256: sha256(sourceBytes),
    targetBundle,
  });
  const approval = {
    approvalClass: "operator-reviewed-finite-transition",
    approvedTransitionSha256: nativeCanonicalSha256(transition),
    artifactVersion: BOOTSTRAP_APPROVAL_ARTIFACT_VERSION,
    targetOperatorBundleSha256: targetBundle.manifestSha256,
    transition,
  };
  const approvalPath = writePrivate(join(root, "approval.json"), approval);
  const publication = nativePublication(transition, targetBundle);
  const publicationPath = writePrivate(join(root, "publication.json"), publication);
  return {
    approvalPath,
    oldModuleBytes,
    oldPath,
    oldShellBytes,
    module: join(targetPath, "schema-preserving-release.mjs"),
    operator: join(targetPath, "schema-preserving-release.sh"),
    publication,
    publicationPath,
    root,
    sourceBytes,
    sourcePath,
    spRoot,
    targetBundle,
    targetModuleBytes: moduleBytes,
    targetPath,
    targetReceiptPath,
    targetShellBytes: shellBytes,
  };
}

// source the real installed operator and run one fixture command
function runFixture(context, body, args) {
  return spawnSync("bash", [
    "-c",
    `source "$1"\n${body}`,
    "native-bootstrap-test",
    context.operator,
    ...args,
  ], { encoding: "utf8", env: process.env });
}

test("native bootstrap recovery admits staging before any image or stage mutation", () => {
  const context = bootstrapFixture();
  const log = join(context.root, "events.log");
  const body = String.raw`
log=$2
root=$3
source_env=$4
approval=$5
publication=$6
target_release=$7
source_commit=$8
target_commit=$9
# terminate one invalid fixture transition
die() { printf 'die:%s\n' "$*" >>"$log"; exit 70; }
# validate the source release grammar used by the real stage function
validate_release() { [[ "$1" =~ ^[0-9]{4}\.[0-9]{2}\.[0-9]{2}-[1-9][0-9]*$ ]] || die 'invalid release'; }
# admit fixture-private inputs without claiming root ownership
sp_private() {
  printf 'private:%s\n' "$1" >>"$log"
  [[ "$1" == /* && -f "$1" && ! -L "$1" && "$(stat -c '%a' "$1")" == 600 ]] || die 'unsafe fixture input'
}
# read one retained environment scalar
env_value() { sed -n "s/^$2=//p" "$1" | tail -n1; }
# resolve one disposable release environment
release_env() { printf '%s/deploy/releases/%s.env\n' "$root" "$1"; }
# retain the exact predecessor source selection
sp_source() { printf '%s\n' "$source_env"; }
# mock only the installed systemd recovery witness
sp_installation() { printf 'installation:%s\n' "$1" >>"$log"; }
# mock the external database ledger probe
sp_database() { printf 'database:%s\n' "$1" >>"$log"; }
# mock the external live health probe
sp_health() { printf 'health:%s\n' "$1" >>"$log"; }
# mock the host capacity probe
sp_capacity() { printf 'capacity\n' >>"$log"; }
sp_native_host_readiness() { printf 'native-host-readiness:%s\n' "$1" >>"$log"; }
# mock the immutable image identity probe
sp_image() { printf 'image:%s\n' "$1" >>"$log"; }
# record the external boot recovery effect
sp_system_recover() { printf 'system-recover\n' >>"$log"; }
# mock the read-only database readiness transaction
sp_readiness() { printf 'readiness:%s\n' "$1" >>"$log"; }
# mock native Compose validation after admission
sp_compose() { printf 'sp-compose:%s\n' "$*" >>"$log"; }
# supply stable retained infrastructure container identities
compose() {
  local service
  printf 'compose:%s\n' "$*" >>"$log"
  service=$(printf '%s\n' "$@" | tail -n1)
  [[ "$service" == postgres ]] && printf '%064d\n' 5 || printf '%064d\n' 6
}
# supply stable image and container inspection evidence
docker() {
  local subject
  printf 'docker:%s\n' "$*" >>"$log"
  subject=$(printf '%s\n' "$@" | tail -n1)
  # project one retained infrastructure inspection
  if [[ "$1" == inspect ]]; then
    printf '%s|fixture-image|fixture-name|0|0|0|0|0|0\n' "$subject"
    return
  fi
  # bind predecessor and target revisions to their fixture images
  if [[ "$1 $2" == 'image inspect' && "$*" == *org.opencontainers.image.revision* ]]; then
    [[ "$subject" == old-web ]] && printf '%s\n' "$source_commit" || printf '%s\n' "$target_commit"
  fi
}
# create only disposable fixture directories
install() {
  local destination
  destination=$(printf '%s\n' "$@" | tail -n1)
  mkdir -p -- "$destination"
  chmod 700 -- "$destination"
  printf 'mkdir:%s\n' "$destination" >>"$log"
}
# adapt root ownership metadata while retaining exact file modes
stat() {
  local path
  path=$(printf '%s\n' "$@" | tail -n1)
  # project retained bundle member metadata
  if [[ "$*" == *"%u:%g:%a:%h"* ]]; then
    [[ "$path" == *.sh ]] && printf '0:0:755:1\n' || printf '0:0:644:1\n'
    return
  fi
  # project retained bundle and proof-directory metadata
  if [[ "$*" == *"%u:%g:%a"* ]]; then
    [[ "$path" == "$sp_root"/native-bootstrap-* ]] && printf '0:0:700\n' || printf '0:0:755\n'
    return
  fi
  command stat "$@"
}
# derive the target environment without mutating the predecessor
write_release_env() {
  cp -- "$1" "$2"
  sed -i "s/^WEATHER_RELEASE=.*/WEATHER_RELEASE=$3/;s|^WEATHER_SERVER_IMAGE=.*|WEATHER_SERVER_IMAGE=$4|;s|^WEATHER_WEB_IMAGE=.*|WEATHER_WEB_IMAGE=$5|" "$2"
  chmod 600 "$2"
}
# record disposable durability boundaries
sync() { printf 'sync:%s\n' "$*" >>"$log"; }
eval "$(declare -f sp_snapshot | sed '1s/sp_snapshot/sp_snapshot_real/')"
# log then execute the real protected snapshot owner
sp_snapshot() { printf 'snapshot:%s\n' "$2" >>"$log"; sp_snapshot_real "$@"; }
eval "$(declare -f sp_unchanged | sed '1s/sp_unchanged/sp_unchanged_real/')"
# log then execute the real protected snapshot comparison
sp_unchanged() { printf 'unchanged:%s\n' "$1" >>"$log"; sp_unchanged_real "$@"; }
sp_prove_native_bootstrap "$approval" "$(sp_sha "$approval")"
transition=$(sp_field "$publication" transitionSha256)
snapshot="$sp_root/native-bootstrap-$transition/protected.json"
printf 'proof-complete\n' >>"$log"
printf 'snapshot-before:%s\n' "$(sp_sha "$snapshot")" >>"$log"
printf 'stage-begin\n' >>"$log"
sp_stage "$target_release" "$publication" "$(sp_sha "$publication")"
printf 'snapshot-after:%s\n' "$(sp_sha "$snapshot")" >>"$log"
printf 'stage-complete\n' >>"$log"
`;
  try {
    const result = runFixture(context, body, [
      log,
      context.root,
      context.sourcePath,
      context.approvalPath,
      context.publicationPath,
      context.publication.targetRelease,
      context.publication.transition.predecessor.commit,
      context.publication.targetCommit,
    ]);
    const events = readFileSync(log, "utf8");
    assert.equal(result.status, 0, `${result.stderr}\n${result.stdout}\n${events}`);
    const stageEvents = events.slice(events.indexOf("stage-begin\n"));
    const admissionUnchanged = stageEvents.indexOf(`unchanged:${context.sourcePath}\n`);
    const stageDirectory = join(context.spRoot, context.publication.targetRelease);
    assert.ok(admissionUnchanged >= 0, events);
    assert.ok(admissionUnchanged < stageEvents.indexOf(`mkdir:${stageDirectory}\n`), events);
    assert.ok(admissionUnchanged < stageEvents.indexOf("docker:pull "), events);
    assert.match(events, /system-recover[\s\S]*proof-complete[\s\S]*stage-begin/u);
    const snapshotDigests = [...events.matchAll(/snapshot-(?:before|after):([a-f0-9]{64})/gu)]
      .map((match) => match[1]);
    assert.deepEqual(snapshotDigests, [snapshotDigests[0], snapshotDigests[0]]);
    assert.deepEqual(readFileSync(context.sourcePath), context.sourceBytes);
    assert.deepEqual(
      readFileSync(join(context.oldPath, "schema-preserving-release.mjs")),
      context.oldModuleBytes,
    );
    assert.deepEqual(
      readFileSync(join(context.oldPath, "schema-preserving-release.sh")),
      context.oldShellBytes,
    );
    assert.deepEqual(readFileSync(context.module), context.targetModuleBytes);
    assert.deepEqual(readFileSync(context.operator), context.targetShellBytes);
    assert.equal(statSync(join(stageDirectory, "stage.json")).mode & 0o777, 0o600);

    const proof = join(
      context.spRoot,
      `native-bootstrap-${context.publication.transitionSha256}`,
      "proof.json",
    );
    const installedValidation = spawnSync(process.execPath, [
      context.module,
      "validate-native-bootstrap-proof",
      proof,
      context.publicationPath,
      sha256(readFileSync(context.targetReceiptPath)),
      context.targetBundle.manifestSha256,
    ], { encoding: "utf8" });
    assert.equal(installedValidation.status, 0, installedValidation.stderr);
  } finally {
    rmSync(context.root, { force: true, recursive: true });
  }
});
