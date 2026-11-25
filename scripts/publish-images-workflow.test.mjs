import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

const workflow = readFileSync(
  new URL("../.github/workflows/publish-images.yml", import.meta.url),
  "utf8",
);

// retain Node as the absent declaration default while closing the explicit Rust selection
test("release publication selects one immutable runtime generation", () => {
  assert.match(workflow, /generation=node-v1/u);
  assert.match(workflow, /generation_file=\.github\/runtime-generation/u);
  assert.match(workflow, /git cat-file -e "\$\{tag_sha\}:\$\{generation_file\}"/u);
  assert.match(workflow, /node-v1\|rust-v1/u);
  assert.match(workflow, /node-v1\)[\s\S]*server_target=server[\s\S]*web_target=web/u);
  assert.match(
    workflow,
    /rust-v1\)[\s\S]*server_target=native-server[\s\S]*web_target=native-web/u,
  );
  assert.equal(
    (workflow.match(/ref: \$\{\{ needs\.validate-release\.outputs\.sha \}\}/gu) ?? []).length,
    2,
  );
  assert.match(workflow, /CI_SHA: \$\{\{ steps\.release\.outputs\.sha \}\}/u);
});

// require extracted native material and local configuration inspection before either image push
test("native publication proves selected images before publishing", () => {
  const extraction = workflow.indexOf("name: Extract independent native runtime material");
  const inspection = workflow.indexOf("name: Inspect native ARM64 image configuration and material");
  const publication = workflow.indexOf("name: Publish validated selected ARM64 images");
  assert.ok(extraction > 0 && inspection > extraction && publication > inspection);
  assert.match(workflow, /--target native-runtime-material-build/u);
  assert.match(workflow, /rust-runtime-image-material\.mjs --inspect/u);
  assert.match(workflow, /docker image inspect weather-server:inspect/u);
  assert.match(workflow, /docker image inspect weather-web:inspect/u);
  assert.match(workflow, /for target in server web/u);
  assert.match(workflow, /--platform linux\/arm64/u);
  assert.match(workflow, /--provenance=true --push/u);
});

// bind the immutable published configurations to canonical extracted material and its source commit
test("native publication artifact closes config material and source identities", () => {
  assert.match(workflow, /remote_config.*local_config/su);
  assert.match(workflow, /published \$\{target\} image differs from inspected candidate/u);
  assert.match(workflow, /weather-native-image-publication-proof\/v1/u);
  assert.match(workflow, /canonicalJsonBytes\(material\)/u);
  assert.match(workflow, /imageMaterialSha256/u);
  assert.match(workflow, /materialManifestSha256: image\.materialManifestSha256/u);
  assert.match(workflow, /configId/u);
  assert.match(workflow, /sourceCommit/u);
  assert.match(workflow, /name: native-runtime-publication-proof/u);
});

// keep the predecessor publication and inspection boundary when no native declaration exists
test("Node fallback retains its production image boundary", () => {
  assert.match(
    workflow,
    /name: Verify exact production image boundary\n\s+if: needs\.validate-release\.outputs\.generation == 'node-v1'/u,
  );
  assert.match(
    workflow,
    /run: node --test deploy\/test\/forecast-adjustment-image\.integration\.test\.mjs/u,
  );
  assert.match(workflow, /--target image-check-files/u);
  assert.match(workflow, /name: server-image-digest/u);
  assert.match(workflow, /name: web-image-digest/u);
});
