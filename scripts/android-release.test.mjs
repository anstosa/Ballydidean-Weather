import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtemp, readFile, rm, stat, writeFile, mkdir, access } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

const preparation = new URL("../mobile/android/scripts/prepare-release.sh", import.meta.url).pathname;
const codeScript = new URL("../mobile/android/scripts/android-version-code.sh", import.meta.url).pathname;
const requiredSecrets = [
  "ANDROID_UPLOAD_KEYSTORE_BASE64",
  "ANDROID_UPLOAD_KEYSTORE_PASSWORD",
  "ANDROID_UPLOAD_KEY_ALIAS",
  "ANDROID_UPLOAD_KEY_PASSWORD",
  "PLAY_SERVICE_ACCOUNT_JSON",
];

// isolate release preparation with fake credentials and a fixed workflow counter
async function fixture(t, overrides = {}) {
  const root = await mkdtemp(join(tmpdir(), "weather-release-test-"));
  // remove only this test's private temporary directory
  t.after(async () => await rm(root, { recursive: true, force: true }));
  const env = {
    PATH: process.env.PATH,
    RUNNER_TEMP: root,
    GITHUB_ENV: join(root, "environment"),
    GITHUB_OUTPUT: join(root, "outputs"),
    GITHUB_REF_TYPE: "branch",
    GITHUB_RUN_NUMBER: "6",
    REQUESTED_VERSION_NAME: "0.1.0",
    ANDROID_UPLOAD_KEYSTORE_BASE64: Buffer.from("fixture-key-not-a-keystore").toString("base64"),
    ANDROID_UPLOAD_KEYSTORE_PASSWORD: "fixture-store-password",
    ANDROID_UPLOAD_KEY_ALIAS: "fixture-alias",
    ANDROID_UPLOAD_KEY_PASSWORD: "fixture-key-password",
    PLAY_SERVICE_ACCOUNT_JSON: JSON.stringify({
      type: "service_account",
      client_email: "fixture@example.invalid",
      private_key: "fixture-not-a-private-key",
    }),
    ...overrides,
  };
  return { root, env };
}

// run the real preparation script without contacting github or google
function prepare(env) {
  return spawnSync("bash", [preparation], { env, encoding: "utf8" });
}

// preserve explicit version names and confine the upload key to private storage
test("Android release preparation retains the requested version and private credentials", async (t) => {
  const { root, env } = await fixture(t, { REQUESTED_VERSION_NAME: "1.2.3-beta.1" });
  const result = prepare(env);
  assert.equal(result.status, 0, result.stderr);
  const key = join(root, "weather-android-signing", "upload.jks");
  assert.equal(await readFile(env.GITHUB_OUTPUT, "utf8"), "code=262850157\nname=1.2.3-beta.1\n");
  const account = join(root, "weather-android-signing", "play-service-account.json");
  assert.equal(await readFile(env.GITHUB_ENV, "utf8"), `ANDROID_UPLOAD_KEYSTORE_FILE=${key}\nPLAY_SERVICE_ACCOUNT_FILE=${account}\n`);
  assert.equal(await readFile(key, "utf8"), "fixture-key-not-a-keystore");
  assert.equal(await readFile(account, "utf8"), env.PLAY_SERVICE_ACCOUNT_JSON);
  assert.equal((await stat(key)).mode & 0o777, 0o600);
  assert.equal((await stat(account)).mode & 0o777, 0o600);
  assert.equal((await stat(join(root, "weather-android-signing"))).mode & 0o777, 0o700);
  assert.equal(result.stdout, "");
  assert.equal(result.stderr, "");
});

// derive names only from android tags and never override an explicit mismatch
test("Android tag releases preserve the exact requested version", async (t) => {
  const { env } = await fixture(t, {
    GITHUB_REF_TYPE: "tag",
    GITHUB_REF_NAME: "android-v2.4",
    REQUESTED_VERSION_NAME: "",
  });
  const result = prepare(env);
  assert.equal(result.status, 0, result.stderr);
  assert.match(await readFile(env.GITHUB_OUTPUT, "utf8"), /name=2\.4\n$/u);
  const mismatched = prepare({ ...env, REQUESTED_VERSION_NAME: "2.5" });
  assert.notEqual(mismatched.status, 0);
  assert.match(mismatched.stderr, /does not match/u);
  const unrelated = prepare({ ...env, GITHUB_REF_NAME: "2026.09.23-6" });
  assert.notEqual(unrelated.status, 0);
  assert.match(unrelated.stderr, /android-vVERSION/u);
});

// give repeated internal builds distinct tags without changing the app version
test("Android build tags retain the explicitly chosen version name", async (t) => {
  // preserve both ordinary and prerelease names under an immutable build identity
  for (const version of ["0.1.0", "0.1.0-beta.1"]) {
    const { env } = await fixture(t, {
      GITHUB_REF_TYPE: "tag",
      GITHUB_REF_NAME: `android-v${version}+build.20260924T173000Z`,
      REQUESTED_VERSION_NAME: version,
    });
    const result = prepare(env);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(await readFile(env.GITHUB_OUTPUT, "utf8"), `code=262850157\nname=${version}\n`);
  }
  const { env } = await fixture(t, {
    GITHUB_REF_TYPE: "tag",
    GITHUB_REF_NAME: "android-v0.1.0+build.20260924T173001Z",
    REQUESTED_VERSION_NAME: "",
  });
  assert.equal(prepare(env).status, 0);
  assert.match(await readFile(env.GITHUB_OUTPUT, "utf8"), /name=0\.1\.0\n$/u);
  const mismatched = prepare({ ...env, REQUESTED_VERSION_NAME: "0.1.1" });
  assert.notEqual(mismatched.status, 0);
  assert.match(mismatched.stderr, /does not match/u);
});

// reject malformed build identities before restoring any release credentials
test("Android build tags reject malformed or unsafe build identifiers", async (t) => {
  const { root, env } = await fixture(t, { GITHUB_REF_TYPE: "tag", REQUESTED_VERSION_NAME: "" });
  // keep the accepted metadata format narrow and single-line
  for (const suffix of [
    "+build.", "+build.1", "+build.20260924T173000", "+build.20260924T173000Z.extra",
    "+other.20260924T173000Z", "+build.20260924T173000Z\nname=bad",
    "+extra+build.20260924T173000Z",
  ]) {
    const result = prepare({ ...env, GITHUB_REF_NAME: `android-v0.1.0${suffix}` });
    assert.notEqual(result.status, 0, suffix);
    assert.match(result.stderr, /build tag/u);
  }
  await assert.rejects(access(join(root, "weather-android-signing")));
});

// reject input that could change workflow outputs or gradle command boundaries
test("Android release preparation rejects missing and unsafe version names", async (t) => {
  const { root, env } = await fixture(t);
  // exercise invalid manual versions without creating a signing directory
  for (const name of ["", "v1.2", "1", "1.2\ncode=999", "1.2 $(id)", "1.2/other", "1.2-"]) {
    const result = prepare({ ...env, REQUESTED_VERSION_NAME: name });
    assert.notEqual(result.status, 0, name);
    assert.match(result.stderr, /version must use/u);
  }
  await assert.rejects(access(join(root, "weather-android-signing")));
});

// fail before restoring credentials when any private release input is absent
test("Android release preparation requires every signing and Play secret", async (t) => {
  const { root, env } = await fixture(t);
  // check each required secret independently
  for (const name of requiredSecrets) {
    const result = prepare({ ...env, [name]: "" });
    assert.notEqual(result.status, 0, name);
    assert.match(result.stderr, new RegExp(`Missing GitHub Actions secret: ${name}`, "u"));
    assert.doesNotMatch(result.stderr, /fixture-store-password|fixture-key-password/u);
  }
  await assert.rejects(access(join(root, "weather-android-signing")));
});

// keep malformed private json out of logs and retain no partial key
test("Android release preparation fails closed on malformed credential payloads", async (t) => {
  const { root, env } = await fixture(t);
  // require a structured service-account key rather than arbitrary json
  for (const json of ["sensitive malformed value", "{}", '{"type":"authorized_user"}', "null"]) {
    const result = prepare({ ...env, PLAY_SERVICE_ACCOUNT_JSON: json });
    assert.notEqual(result.status, 0);
    assert.match(result.stderr, /must contain a service-account key/u);
    assert.ok(!result.stderr.includes(json));
  }
  const invalidKey = prepare({ ...env, ANDROID_UPLOAD_KEYSTORE_BASE64: "invalid!base64" });
  assert.notEqual(invalidKey.status, 0);
  await assert.rejects(access(join(root, "weather-android-signing")));
});

// never overwrite a preexisting credential directory
test("Android release preparation refuses existing key material", async (t) => {
  const { root, env } = await fixture(t);
  const directory = join(root, "weather-android-signing");
  await mkdir(directory);
  await writeFile(join(directory, "upload.jks"), "untouched");
  assert.notEqual(prepare(env).status, 0);
  assert.equal(await readFile(join(directory, "upload.jks"), "utf8"), "untouched");
});

// increase codes by one per new workflow run without timestamp jumps
test("Android version codes increment from the last published timestamp code", async (t) => {
  const { env } = await fixture(t);
  // preserve the migration baseline, adjacent increments and the play upper boundary
  for (const [run, code] of [["6", 262850157], ["7", 262850158], ["8", 262850159], ["1837149849", 2100000000]]) {
    const result = spawnSync("bash", [codeScript], {
      env: { ...env, GITHUB_RUN_NUMBER: run },
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, `${code}\n`);
  }
});

// retries retain artifact identity rather than silently allocating another release
test("Android version codes do not change when a workflow is retried", async (t) => {
  const { env } = await fixture(t);
  // retry counts cannot affect the release counter
  for (const attempt of ["1", "2", "100"]) {
    const result = spawnSync("bash", [codeScript], {
      env: { ...env, GITHUB_RUN_ATTEMPT: attempt },
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, "262850157\n");
  }
});

// reject counter resets, injection and overflow before printing workflow output
test("Android version codes fail closed on invalid or exhausted counters", async (t) => {
  const { env } = await fixture(t);
  // bound decimal parsing before shell arithmetic and refuse old workflow runs
  for (const run of ["", "0", "1", "5", "06", "-6", "6.0", "date-error", "1837149850", "9999999999", "999999999999999999999", "6\nname=bad", "6+1", "$(id)"]) {
    const result = spawnSync("bash", [codeScript], {
      env: { ...env, GITHUB_RUN_NUMBER: run },
      encoding: "utf8",
    });
    assert.notEqual(result.status, 0, run);
    assert.equal(result.stdout, "");
  }
});

// invalid release counters cannot leave private credentials or partial outputs
test("Android release preparation validates its counter before restoring credentials", async (t) => {
  const { root, env } = await fixture(t, { GITHUB_RUN_NUMBER: "" });
  const result = prepare(env);
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /GITHUB_RUN_NUMBER/u);
  await assert.rejects(access(join(root, "weather-android-signing")));
  await assert.rejects(access(env.GITHUB_OUTPUT));
  await assert.rejects(access(env.GITHUB_ENV));
});

// bind publishing to Weather's exact validated commit and bounded artifacts
test("Android workflow targets only Weather internal testing after artifact verification", async () => {
  const workflow = await readFile(new URL("../.github/workflows/publish-android.yml", import.meta.url), "utf8");
  assert.match(workflow, /workflow_dispatch:[\s\S]*version_name:/u);
  assert.match(workflow, /'android-v\*'/u);
  assert.match(workflow, /ref: \$\{\{ github\.sha \}\}/u);
  assert.match(workflow, /CI_CHECK_MODE: release[\s\S]*CI_SHA: \$\{\{ github\.sha \}\}/u);
  assert.match(workflow, /cancel-in-progress: false/u);
  assert.match(workflow, /java-version: '17'/u);
  assert.match(workflow, /'platforms;android-37\.0' 'build-tools;36\.0\.0'/u);
  assert.match(workflow, /--no-configuration-cache/u);
  assert.match(workflow, /-PweatherRequireReleaseSigning=true/u);
  assert.match(workflow, /packageName: farm\.ballydidean\.weather/u);
  assert.match(workflow, /track: internal/u);
  assert.match(workflow, /status: completed/u);
  assert.match(workflow, /serviceAccountJson: \$\{\{ env\.PLAY_SERVICE_ACCOUNT_FILE \}\}/u);
  assert.doesNotMatch(workflow, /serviceAccountJsonPlainText:/u);
  assert.match(workflow, /r0adkll\/upload-google-play@935ef9c68bb393a8e6116b1575626a7f5be3a7fb/u);
  assert.match(workflow, /if: always\(\)[\s\S]*rm -f .*weather-android-signing\/upload\.jks/u);
  assert.ok(workflow.indexOf("Require successful exact-commit Check") < workflow.indexOf("Prepare version"));
  assert.ok(workflow.indexOf("Verify signed release artifacts") < workflow.indexOf("Publish to Google Play"));
  assert.doesNotMatch(workflow, /fyi\.ferry|AUTH0|FIREBASE|yarn|cap sync|keystore\.properties|secrets: inherit/u);
  assert.doesNotMatch(workflow, /pull_request|track: production|continue-on-error|cache: gradle/u);
});
