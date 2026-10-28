import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  buildFixtureAdmissionReports,
  parseRestoreRehearsalArguments,
  prepareRepository,
} from "./rehearse-weather-page-restore.mjs";
import {
  runCommand,
  validateIngestionReport,
  validateIndexManifest,
  validateRehearsalReport,
} from "./weather-page-indexes.mjs";

const manifest = validateIndexManifest(JSON.parse(await readFile(new URL("./weather-page-indexes.json", import.meta.url), "utf8")));
const peaks = {
  indexBytesHighWater: 1024,
  tempBytesHighWater: 0,
  walGeneratedBytes: 2048,
  walRetainedBytesHighWater: 0,
  inodesHighWater: 1,
};

// freeze the one root-only cli interface
test("restore rehearsal CLI accepts only one absolute new output", () => {
  assert.deepEqual(parseRestoreRehearsalArguments(["--output", "/tmp/private-restore"]), { output: "/tmp/private-restore" });
  assert.throws(() => parseRestoreRehearsalArguments(["--output", "relative"]), /absolute canonical/u);
  assert.throws(() => parseRestoreRehearsalArguments([]), /usage/u);
  assert.throws(() => parseRestoreRehearsalArguments(["--output", "/tmp/a", "--short", "1"]), /usage/u);
});

// prove the annotated fixture tag does not depend on ambient git identity
test("restore rehearsal creates its annotated tag with an empty Git home", async () => {
  const root = await mkdtemp(join(tmpdir(), "weather-restore-git-"));
  const source = join(root, "source");
  const fixture = join(root, "fixture");
  const home = join(root, "home");
  const xdg = join(root, "xdg");
  const isolatedEnv = {
    ...process.env,
    HOME: home,
    XDG_CONFIG_HOME: xdg,
    GIT_CONFIG_GLOBAL: "/dev/null",
    GIT_CONFIG_NOSYSTEM: "1",
  };
  delete isolatedEnv.GIT_AUTHOR_NAME;
  delete isolatedEnv.GIT_AUTHOR_EMAIL;
  delete isolatedEnv.GIT_COMMITTER_NAME;
  delete isolatedEnv.GIT_COMMITTER_EMAIL;

  try {
    await mkdir(source);
    await mkdir(fixture);
    await mkdir(home);
    await mkdir(xdg);
    assert.equal((await runCommand("git", ["init", "--quiet", source], { env: isolatedEnv })).code, 0);
    await writeFile(join(source, "fixture.txt"), "fixture\n");
    assert.equal((await runCommand("git", ["-C", source, "add", "fixture.txt"], { env: isolatedEnv })).code, 0);
    assert.equal((await runCommand("git", [
      "-c", "user.name=Source Fixture",
      "-c", "user.email=source-fixture@example.invalid",
      "-C", source,
      "commit", "--quiet", "--message", "fixture source",
    ], { env: isolatedEnv })).code, 0);
    const prepared = await prepareRepository(source, fixture, { env: isolatedEnv });
    const tagType = await runCommand("git", ["-C", prepared.repository, "cat-file", "-t", "2099.01.01-99"], { env: isolatedEnv });
    const tagger = await runCommand("git", ["-C", prepared.repository, "for-each-ref", "--format=%(taggername)|%(taggeremail)", "refs/tags/2099.01.01-99"], { env: isolatedEnv });
    assert.equal(tagType.stdout.trim(), "tag");
    assert.equal(tagger.stdout.trim(), "Weather Restore Rehearsal|<weather-restore-rehearsal@ballydidean.farm>");
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});

// keep fixture contracts explicit and unusable as qualification evidence
test("restore rehearsal fixture evidence is accepted only by the isolated validator", () => {
  const reports = buildFixtureAdmissionReports({
    manifest,
    resources: {
      targetValid: peaks,
      targetCancelled: { ...peaks, indexBytesHighWater: 0 },
      predecessorValid: peaks,
      predecessorCancelled: { ...peaks, indexBytesHighWater: 0 },
    },
    rows: 500000,
    heapBytes: 64 * 1024 * 1024,
    capturedAt: "2026-10-28T20:00:00.000Z",
  });
  assert.equal(validateRehearsalReport(reports.rehearsal, manifest, { allowFixtureOnly: true }), reports.rehearsal);
  assert.throws(() => validateRehearsalReport(reports.rehearsal, manifest), /restricted to isolated restore/u);
  assert.equal(validateIngestionReport(
    reports.ingestion,
    reports.ingestion.identities,
    reports.ingestion.filesystems.map((filesystem) => ({ id: filesystem.id })),
    reports.rehearsal,
    reports.rehearsalDigest,
    { allowFixtureOnly: true },
  ), reports.ingestion);
  assert.equal(reports.rehearsal.qualification, false);
  assert.equal(reports.ingestion.contractOnly, true);
  assert.equal(reports.ingestion.durationSeconds, 0);
  assert.equal(reports.ingestion.events.burstEnvelopeMeasured, false);
});
