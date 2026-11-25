import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";
import {
  DECLARED_TRANSITION_CONTRACT_VERSION,
  OPERATOR_BUNDLE_CONTRACT_VERSION,
  WP8B_TARGET_ADDED_FILES,
  WP8B_TARGET_ONLY_ROOTS,
  buildReviewedTransitionDeclaration,
  buildTwoFileBundleManifest,
  treeSha256,
  validatePredecessorTransition,
} from "./predecessor-rust-transition.mjs";
import {
  validatePredecessorTransition as validateInstalledPredecessorTransition,
} from "./schema-preserving-release.mjs";

const FIXTURE_PATH = new URL("./predecessor-rust-transition.fixture.json", import.meta.url);

// load a fresh fixture
function fixture() {
  return JSON.parse(readFileSync(FIXTURE_PATH, "utf8"));
}

// preserve every predecessor member and exact declared additions
test("checked transition fixture preserves predecessor equality and finite additions", () => {
  const transition = fixture();
  assert.equal(validatePredecessorTransition(transition), transition);
  assert.equal(validateInstalledPredecessorTransition(transition), transition);
  const predecessor = new Map(
    transition.predecessorMembers.map((member) => [member.path, member.sha256]),
  );
  const target = new Map(transition.targetMembers.map((member) => [member.path, member.sha256]));
  // compare every retained fixture member
  for (const [path, digest] of predecessor) {
    assert.equal(target.get(path), digest);
  }
  const declaredAdditions = [
    ...transition.addedFiles,
    ...transition.targetOnlyRoots.flatMap((targetRoot) => targetRoot.members),
  ].sort((left, right) => left.path < right.path ? -1 : left.path > right.path ? 1 : 0);
  assert.deepEqual(
    transition.targetMembers.filter((member) => !predecessor.has(member.path)),
    declaredAdditions,
  );
  assert.deepEqual(transition.addedFiles.map((member) => member.path), WP8B_TARGET_ADDED_FILES);
  assert.deepEqual(
    transition.targetOnlyRoots.map((targetRoot) => targetRoot.root),
    WP8B_TARGET_ONLY_ROOTS,
  );
});

// bind exact sorted bundle bytes and the final newline
test("two-file bundle manifest is deterministic and hashes its final newline", () => {
  const bundle = buildTwoFileBundleManifest([
    { bytes: Buffer.from("operator\n"), path: "scripts/schema-preserving-release.sh" },
    { bytes: Buffer.from("validator\n"), path: "scripts/schema-preserving-release.mjs" },
  ]);
  const expectedManifest = [
    `${createHash("sha256").update("validator\n").digest("hex")}  scripts/schema-preserving-release.mjs\n`,
    `${createHash("sha256").update("operator\n").digest("hex")}  scripts/schema-preserving-release.sh\n`,
  ].join("");
  assert.equal(bundle.contractVersion, OPERATOR_BUNDLE_CONTRACT_VERSION);
  assert.equal(bundle.sha256sum, expectedManifest);
  assert.equal(bundle.sha256sum.endsWith("\n"), true);
  assert.equal(
    bundle.manifestSha256,
    createHash("sha256").update(expectedManifest).digest("hex"),
  );
  assert.deepEqual(
    buildTwoFileBundleManifest([
      { bytes: Buffer.from("validator\n"), path: "scripts/schema-preserving-release.mjs" },
      { bytes: Buffer.from("operator\n"), path: "scripts/schema-preserving-release.sh" },
    ]),
    bundle,
  );
});

// reject unpinned retained-member drift
test("transition rejects undeclared predecessor byte changes", () => {
  const transition = fixture();
  transition.targetMembers[0].sha256 = "a".repeat(64);
  assert.throws(
    () => validatePredecessorTransition(transition),
    /changed without an exact declaration/u,
  );
});

// accept only both sides of a declared changed member
test("transition binds separately named changed members on both sides", () => {
  const transition = fixture();
  const predecessorSha256 = transition.predecessorMembers[0].sha256;
  const targetSha256 = "a".repeat(64);
  transition.targetMembers[0].sha256 = targetSha256;
  transition.changedMembers.push({
    path: transition.predecessorMembers[0].path,
    predecessorSha256,
    targetSha256,
  });
  assert.equal(validatePredecessorTransition(transition), transition);
  transition.changedMembers[0].predecessorSha256 = "b".repeat(64);
  assert.throws(
    () => validatePredecessorTransition(transition),
    /does not match predecessor and target bytes/u,
  );
});

// admit one extension only through its exact content-addressed finite declaration
test("declared transition binds extra additions and changes to both commit trees", () => {
  const transition = fixture();
  const added = {
    path: ".github/workflows/native-images.yml",
    sha256: "a".repeat(64),
  };
  const changed = {
    path: transition.predecessorMembers[0].path,
    predecessorSha256: transition.predecessorMembers[0].sha256,
    targetSha256: "b".repeat(64),
  };
  const declaration = buildReviewedTransitionDeclaration({
    addedFiles: [added],
    changedMembers: [changed],
    predecessorCommit: transition.predecessor.commit,
    targetCommit: transition.target.commit,
  });
  transition.contractVersion = DECLARED_TRANSITION_CONTRACT_VERSION;
  transition.reviewedDeclaration = declaration;
  transition.addedFiles.push(added);
  transition.addedFiles.sort((left, right) => left.path < right.path ? -1 : 1);
  transition.changedMembers.push(changed);
  transition.changedMembers.sort((left, right) => left.path < right.path ? -1 : 1);
  transition.targetMembers.push({ path: added.path, sha256: added.sha256 });
  transition.targetMembers.find((member) => member.path === changed.path).sha256 =
    changed.targetSha256;
  transition.targetMembers.sort((left, right) => left.path < right.path ? -1 : 1);
  assert.equal(validatePredecessorTransition(transition), transition);

  const declarationDrift = structuredClone(transition);
  declarationDrift.reviewedDeclaration.addedFiles[0].sha256 = "c".repeat(64);
  assert.throws(
    () => validatePredecessorTransition(declarationDrift),
    /declarationSha256 does not match/u,
  );

  const undeclaredChange = structuredClone(transition);
  undeclaredChange.reviewedDeclaration = buildReviewedTransitionDeclaration({
    addedFiles: [added],
    changedMembers: [],
    predecessorCommit: transition.predecessor.commit,
    targetCommit: transition.target.commit,
  });
  assert.throws(
    () => validatePredecessorTransition(undeclaredChange),
    /outside the content-addressed review declaration/u,
  );
});

// reject target members missing from finite additions
test("transition rejects extra and omitted target-only members", () => {
  const extra = fixture();
  extra.targetMembers.splice(2, 0, {
    path: "config/unreviewed.json",
    sha256: "a".repeat(64),
  });
  assert.throws(
    () => validatePredecessorTransition(extra),
    /absent from the exact transition manifest/u,
  );

  const omitted = fixture();
  omitted.targetMembers.splice(1, 1);
  assert.throws(
    () => validatePredecessorTransition(omitted),
    /declared target addition .* is absent/u,
  );
});

// reject declarations outside the shared immutable WP8b set
test("transition requires the exact shared WP8b file and root declarations", () => {
  const missingFileDeclaration = fixture();
  missingFileDeclaration.addedFiles.pop();
  assert.throws(
    () => validatePredecessorTransition(missingFileDeclaration),
    /addedFiles must exactly match the immutable WP8b declaration/u,
  );

  const missingRootDeclaration = fixture();
  missingRootDeclaration.targetOnlyRoots.pop();
  assert.throws(
    () => validatePredecessorTransition(missingRootDeclaration),
    /targetOnlyRoots must exactly match the immutable WP8b declaration/u,
  );
});

// reject permissive target roots and wildcard declarations
test("transition rejects broad roots wildcard paths and root escapes", () => {
  const broad = fixture();
  broad.targetOnlyRoots[0].root = "packages";
  assert.throws(
    () => validatePredecessorTransition(broad),
    /not broad prefixes/u,
  );

  const wildcard = fixture();
  wildcard.targetOnlyRoots[0].root = "packages/*";
  assert.throws(
    () => validatePredecessorTransition(wildcard),
    /without wildcards/u,
  );

  const escape = fixture();
  escape.targetOnlyRoots[0].root = "packages/contracts/deeper";
  assert.throws(
    () => validatePredecessorTransition(escape),
    /outside packages\/contracts\/deeper/u,
  );
});

// reject incomplete or drifted finite root expansion
test("transition binds root membership and its canonical tree digest", () => {
  const digestDrift = fixture();
  digestDrift.targetOnlyRoots[0].treeSha256 = "a".repeat(64);
  assert.throws(
    () => validatePredecessorTransition(digestDrift),
    /does not match its exact members/u,
  );

  const membershipDrift = fixture();
  membershipDrift.targetOnlyRoots[0].members.push({
    path: "packages/contracts/src/model.ts",
    sha256: "f".repeat(64),
  });
  membershipDrift.targetOnlyRoots[0].treeSha256 = treeSha256(
    membershipDrift.targetOnlyRoots[0].members,
  );
  assert.throws(
    () => validatePredecessorTransition(membershipDrift),
    /absent from targetMembers/u,
  );
});

// reject the historical ordinary publication lane
test("transition requires the versioned predecessor bootstrap lane", () => {
  const oldContract = fixture();
  oldContract.contractVersion = "weather-schema-preserving-publication/v2";
  assert.throws(
    () => validatePredecessorTransition(oldContract),
    /contractVersion is invalid/u,
  );

  const ordinaryLane = fixture();
  ordinaryLane.publicationLane = "ordinary-release";
  assert.throws(
    () => validatePredecessorTransition(ordinaryLane),
    /must use predecessor-control-bootstrap/u,
  );
});

// require one unique two-file operator bundle
test("bundle manifest rejects incomplete duplicate and wildcard membership", () => {
  assert.throws(
    () => buildTwoFileBundleManifest([
      { bytes: Buffer.from("operator\n"), path: "scripts/operator.sh" },
    ]),
    /exactly two files/u,
  );
  assert.throws(
    () => buildTwoFileBundleManifest([
      { bytes: Buffer.from("operator\n"), path: "scripts/operator.sh" },
      { bytes: Buffer.from("validator\n"), path: "scripts/operator.sh" },
    ]),
    /paths must be unique/u,
  );
  assert.throws(
    () => buildTwoFileBundleManifest([
      { bytes: Buffer.from("operator\n"), path: "scripts/*.sh" },
      { bytes: Buffer.from("validator\n"), path: "scripts/validator.mjs" },
    ]),
    /without wildcards/u,
  );
});
