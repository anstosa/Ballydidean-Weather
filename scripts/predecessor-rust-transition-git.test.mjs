import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import test from "node:test";
import {
  GIT_ADAPTER_VERSION,
  TARGET_ADDED_FILES,
  TARGET_ONLY_ROOTS,
  deriveDeclaredGitTransitionFixture,
  deriveGitTransitionFixture,
} from "./predecessor-rust-transition-git.mjs";
import {
  DECLARED_TRANSITION_CONTRACT_VERSION,
  TRANSITION_CONTRACT_VERSION,
  TRANSITION_PUBLICATION_LANE,
  WP8B_TARGET_ADDED_FILES,
  WP8B_TARGET_ONLY_ROOTS,
  buildReviewedTransitionDeclaration,
  validatePredecessorTransition,
} from "./predecessor-rust-transition.mjs";

const PREDECESSOR_COMMIT = "a".repeat(40);
const TARGET_COMMIT = "b".repeat(40);
const REQUIRED_PATHS = [
  "Dockerfile",
  "deploy/scripts/common.sh",
  "deploy/scripts/update.sh",
  "package-lock.json",
  "packages/database/migrations/0020_adjustment_revision_frontier.sql",
  "packages/database/migrations/0023_forecast_product_read_index.sql",
  "packages/forecast-adjustment/src/maintenance-capture-epoch.ts",
];

// hash one fixture blob
function digest(value) {
  return createHash("sha256").update(value).digest("hex");
}

// build one isolated Git object fixture
function gitFixture(options = {}) {
  const predecessorPaths = [...REQUIRED_PATHS];
  const targetPaths = [
    ...REQUIRED_PATHS,
    ...TARGET_ADDED_FILES,
    ...TARGET_ONLY_ROOTS.map((root) => `${root}/fixture.txt`),
  ];
  // contaminate the predecessor on request
  if (options.sourceAddition !== undefined) {
    predecessorPaths.push(options.sourceAddition);
  }
  // add undeclared target material on request
  if (options.targetExtra !== undefined) {
    targetPaths.push(options.targetExtra);
  }
  // retain one extra existing member on request
  if (options.retainedExtra !== undefined) {
    predecessorPaths.push(options.retainedExtra);
    targetPaths.push(options.retainedExtra);
  }
  // omit named target material on request
  if (options.targetOmission !== undefined) {
    const index = targetPaths.indexOf(options.targetOmission);
    // remove only the requested fixture member
    if (index !== -1) {
      targetPaths.splice(index, 1);
    }
  }
  predecessorPaths.sort();
  targetPaths.sort();
  const calls = [];

  // emulate fixed Git object reads
  function runGit(args) {
    calls.push(args);
    // resolve exact fixture commits
    if (args[0] === "rev-parse") {
      return Buffer.from(`${args[2].slice(0, 40)}\n`);
    }
    // prove fixture ancestry without repository mutation
    if (args[0] === "merge-base") {
      return Buffer.alloc(0);
    }
    // list one exact fixture tree
    if (args[0] === "ls-tree") {
      const paths = args[4] === PREDECESSOR_COMMIT ? predecessorPaths : targetPaths;
      return Buffer.from(`${paths.join("\0")}\0`);
    }
    // return one exact fixture blob
    if (args[0] === "cat-file") {
      const separator = args[2].indexOf(":");
      const commit = args[2].slice(0, separator);
      const path = args[2].slice(separator + 1);
      const drift = options.driftPath === path && commit === TARGET_COMMIT ? "drift:" : "";
      return Buffer.from(`${drift}fixture:${path}\n`);
    }
    throw new Error(`unexpected Git fixture command ${args[0]}`);
  }

  return { calls, predecessorPaths, runGit, targetPaths };
}

// assemble fixture-only metadata around derived members
function transitionDocument(derived) {
  return {
    addedFiles: derived.addedFiles,
    changedMembers: derived.changedMembers,
    contractVersion: TRANSITION_CONTRACT_VERSION,
    predecessor: {
      bundleSha256: "1".repeat(64),
      commit: derived.predecessorCommit,
      controlSha256: "2".repeat(64),
      installationReceiptSha256: "3".repeat(64),
      recoveryDropInSha256: "4".repeat(64),
      release: "2026.11.03-3",
      sourceEnvironmentSha256: "5".repeat(64),
    },
    predecessorMembers: derived.predecessorMembers,
    publicationLane: TRANSITION_PUBLICATION_LANE,
    target: {
      commit: derived.targetCommit,
      release: "2026.11.03-4",
    },
    targetMembers: derived.targetMembers,
    targetOnlyRoots: derived.targetOnlyRoots,
  };
}

// derive exact fixed surfaces from injected object bytes
test("Git adapter derives finite transition members from object bytes", () => {
  const fixture = gitFixture();
  const transition = deriveGitTransitionFixture(
    PREDECESSOR_COMMIT,
    TARGET_COMMIT,
    fixture.runGit,
  );
  assert.equal(transition.adapterVersion, GIT_ADAPTER_VERSION);
  assert.equal(transition.evidenceMode, "injected-test-fixture");
  assert.equal(TARGET_ADDED_FILES, WP8B_TARGET_ADDED_FILES);
  assert.equal(TARGET_ONLY_ROOTS, WP8B_TARGET_ONLY_ROOTS);
  assert.deepEqual(
    transition.predecessorMembers.map((member) => member.path),
    REQUIRED_PATHS,
  );
  assert.deepEqual(
    transition.addedFiles.map((member) => member.path),
    TARGET_ADDED_FILES,
  );
  assert.deepEqual(
    transition.targetOnlyRoots.map((targetRoot) => targetRoot.root),
    TARGET_ONLY_ROOTS,
  );
  assert.equal(
    transition.addedFiles[0].sha256,
    digest(`fixture:${TARGET_ADDED_FILES[0]}\n`),
  );
  // bind every root member and digest
  for (const targetRoot of transition.targetOnlyRoots) {
    assert.deepEqual(
      targetRoot.members.map((member) => member.path),
      [`${targetRoot.root}/fixture.txt`],
    );
    assert.match(targetRoot.treeSha256, /^[a-f0-9]{64}$/u);
  }
  assert.deepEqual(
    new Set(fixture.calls.map((args) => args[0])),
    new Set(["cat-file", "ls-tree", "merge-base", "rev-parse"]),
  );
  assert.equal(
    validatePredecessorTransition(transitionDocument(transition)).target.commit,
    TARGET_COMMIT,
  );
});

// reject protected changes outside fixed reviewed transitions
test("Git adapter rejects unreviewed existing protected drift", () => {
  const fixture = gitFixture({ driftPath: "Dockerfile" });
  assert.throws(
    () => deriveGitTransitionFixture(PREDECESSOR_COMMIT, TARGET_COMMIT, fixture.runGit),
    /outside a fixed reviewed transition/u,
  );
});

// derive only the extra paths and byte transitions named by one exact declaration
test("Git adapter admits finite declared additions and changed members", () => {
  const addedPath = ".github/workflows/native-images.yml";
  const changedPath = "Dockerfile";
  const fixture = gitFixture({ driftPath: changedPath, targetExtra: addedPath });
  const declaration = buildReviewedTransitionDeclaration({
    addedFiles: [{ path: addedPath, sha256: digest(`fixture:${addedPath}\n`) }],
    changedMembers: [{
      path: changedPath,
      predecessorSha256: digest(`fixture:${changedPath}\n`),
      targetSha256: digest(`drift:fixture:${changedPath}\n`),
    }],
    predecessorCommit: PREDECESSOR_COMMIT,
    targetCommit: TARGET_COMMIT,
  });
  const derived = deriveDeclaredGitTransitionFixture(
    PREDECESSOR_COMMIT,
    TARGET_COMMIT,
    declaration,
    fixture.runGit,
  );
  assert.deepEqual(derived.reviewedDeclaration, declaration);
  assert.equal(derived.addedFiles.find((member) => member.path === addedPath).sha256,
    declaration.addedFiles[0].sha256);
  assert.deepEqual(derived.changedMembers, declaration.changedMembers);
  const document = transitionDocument(derived);
  document.contractVersion = DECLARED_TRANSITION_CONTRACT_VERSION;
  document.reviewedDeclaration = declaration;
  assert.equal(validatePredecessorTransition(document), document);

  const wrongHash = buildReviewedTransitionDeclaration({
    addedFiles: [{ path: addedPath, sha256: "f".repeat(64) }],
    changedMembers: declaration.changedMembers,
    predecessorCommit: PREDECESSOR_COMMIT,
    targetCommit: TARGET_COMMIT,
  });
  assert.throws(
    () => deriveDeclaredGitTransitionFixture(
      PREDECESSOR_COMMIT,
      TARGET_COMMIT,
      wrongHash,
      fixture.runGit,
    ),
    /reviewed addition .* differs/u,
  );
});

// discover every fixed v2 source surface independently of the declaration
test("declared Git adapter rejects omitted additions across the native source surface", () => {
  const undeclaredPaths = [
    ".github/runtime-generation",
    "apps/api/src/native-new.ts",
    "apps/web/src/native-new.ts",
    "apps/worker/src/native-new.ts",
    "packages/repositories/src/native-new.ts",
    "rust/crates/weather-runtime/src/native_new.rs",
    "scripts/native-new.mjs",
  ];
  const declaration = buildReviewedTransitionDeclaration({
    addedFiles: [],
    changedMembers: [],
    predecessorCommit: PREDECESSOR_COMMIT,
    targetCommit: TARGET_COMMIT,
  });
  // require each fixed surface addition to be discovered before review matching
  for (const path of undeclaredPaths) {
    const fixture = gitFixture({ targetExtra: path });
    assert.throws(
      () => deriveDeclaredGitTransitionFixture(
        PREDECESSOR_COMMIT,
        TARGET_COMMIT,
        declaration,
        fixture.runGit,
      ),
      new RegExp(`added undeclared protected member ${path.replace(/[.*+?^${}()|[\]\\]/gu, "\\$&")}`, "u"),
    );
  }
});

// discover changed v2 members without relying on caller-provided membership
test("declared Git adapter rejects an omitted native source change", () => {
  const changedPath = "rust/crates/weather-runtime/src/existing.rs";
  const fixture = gitFixture({ driftPath: changedPath, retainedExtra: changedPath });
  const declaration = buildReviewedTransitionDeclaration({
    addedFiles: [],
    changedMembers: [],
    predecessorCommit: PREDECESSOR_COMMIT,
    targetCommit: TARGET_COMMIT,
  });
  assert.throws(
    () => deriveDeclaredGitTransitionFixture(
      PREDECESSOR_COMMIT,
      TARGET_COMMIT,
      declaration,
      fixture.runGit,
    ),
    /changed protected member .* outside a fixed reviewed transition/u,
  );
});

// preserve the legacy v1 evidence surface byte for byte
test("legacy Git adapter ignores declared-v2-only source additions", () => {
  const baseline = gitFixture();
  const extended = gitFixture({
    targetExtra: "rust/crates/weather-runtime/src/v2-only.rs",
  });
  assert.deepEqual(
    deriveGitTransitionFixture(PREDECESSOR_COMMIT, TARGET_COMMIT, extended.runGit),
    deriveGitTransitionFixture(PREDECESSOR_COMMIT, TARGET_COMMIT, baseline.runGit),
  );
});

// reject protected additions outside the fixed target manifest
test("Git adapter rejects undeclared protected additions", () => {
  const fixture = gitFixture({ targetExtra: "config/unreviewed.json" });
  assert.throws(
    () => deriveGitTransitionFixture(PREDECESSOR_COMMIT, TARGET_COMMIT, fixture.runGit),
    /added undeclared protected member config\/unreviewed\.json/u,
  );
});

// reject incomplete fixed target additions
test("Git adapter rejects missing named target files and roots", () => {
  const missingFile = gitFixture({ targetOmission: TARGET_ADDED_FILES[0] });
  assert.throws(
    () => deriveGitTransitionFixture(PREDECESSOR_COMMIT, TARGET_COMMIT, missingFile.runGit),
    /missing target-only file/u,
  );

  const missingRoot = gitFixture({ targetOmission: `${TARGET_ONLY_ROOTS[0]}/fixture.txt` });
  assert.throws(
    () => deriveGitTransitionFixture(PREDECESSOR_COMMIT, TARGET_COMMIT, missingRoot.runGit),
    /missing target-only root/u,
  );
});

// reject target-only material already present in the predecessor
test("Git adapter requires target additions to be absent from predecessor", () => {
  const fileFixture = gitFixture({ sourceAddition: TARGET_ADDED_FILES[0] });
  assert.throws(
    () => deriveGitTransitionFixture(PREDECESSOR_COMMIT, TARGET_COMMIT, fileFixture.runGit),
    /predecessor already contains target-only file/u,
  );

  const rootFixture = gitFixture({ sourceAddition: `${TARGET_ONLY_ROOTS[0]}/fixture.txt` });
  assert.throws(
    () => deriveGitTransitionFixture(PREDECESSOR_COMMIT, TARGET_COMMIT, rootFixture.runGit),
    /predecessor already contains target-only root/u,
  );
});

// reject refs and same-commit comparisons before object reads
test("Git adapter accepts only distinct exact commit ids", () => {
  const fixture = gitFixture();
  assert.throws(
    () => deriveGitTransitionFixture("HEAD", TARGET_COMMIT, fixture.runGit),
    /exact lowercase commit SHA/u,
  );
  assert.throws(
    () => deriveGitTransitionFixture(PREDECESSOR_COMMIT, PREDECESSOR_COMMIT, fixture.runGit),
    /must differ/u,
  );
});
