const GATES = new Set(["rt-u04", "conditional-seam", "rt-u16", "all"]);
const SCOPES = new Set([
  "current-source",
  "implemented-source",
  "physical-baseline",
]);
const USAGE =
  "usage: node scripts/check-backend-reference-time-source.mjs --gate <rt-u04|conditional-seam|rt-u16|all> --scope <current-source|implemented-source|physical-baseline>\n";

// create one sanitized pre-proof failure
function unsupportedGateError() {
  const error = new Error("internal.failure");
  error.code = "internal.failure";
  error.stage = "internal";
  return error;
}

// recursively freeze one detached document
function freezeRecursively(value) {
  // stop at scalars and frozen objects
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) {
    return value;
  }

  // freeze every child before its parent
  for (const child of Object.values(value)) {
    freezeRecursively(child);
  }
  return Object.freeze(value);
}

// encode one canonical json value
function canonicalJson(value) {
  // encode supported scalar values
  if (value === null || typeof value !== "object") {
    return JSON.stringify(value);
  }
  // preserve semantic array order
  if (Array.isArray(value)) {
    const members = [];

    // encode every ordered member
    for (const member of value) {
      members.push(canonicalJson(member));
    }
    return `[${members.join(",")}]`;
  }
  const keys = Object.keys(value).sort();
  const members = [];

  // encode every sorted record field
  for (const key of keys) {
    members.push(`${JSON.stringify(key)}:${canonicalJson(value[key])}`);
  }
  return `{${members.join(",")}}`;
}

// build one exact sanitized early-error envelope
function earlyErrorEnvelope(error) {
  const allowedStages = new Set([
    "reader",
    "policy",
    "frontend-bootstrap",
    "internal",
  ]);
  // retain only a closed public stage
  const stage = error !== null &&
      typeof error === "object" &&
      allowedStages.has(error.stage)
    ? error.stage
    : "internal";
  // select one closed stage error code
  const code = stage === "frontend-bootstrap"
    ? "frontend.invalid-output"
    : stage === "reader"
      ? "reader.unsafe-input"
      : stage === "policy"
        ? "policy.invalid"
        : "internal.failure";
  // select one closed diagnostic dimension
  const dimension = stage === "frontend-bootstrap"
    ? "frontend"
    : stage === "reader"
      ? "reader"
      : stage === "policy"
        ? "policy"
        : "internal";
  return freezeRecursively({
    contractVersion: "weather-backend-reference-time-source-error/v1",
    diagnostics: [{
      code,
      dimension,
      ids: [],
      messageId: code,
      severity: "error",
      sourceSiteId: null,
    }],
    stage,
    verdict: "FAIL",
  });
}

// validate the fixed reader boundary without fabricating a proof
export async function buildBackendReferenceTimeSourceGate(...argumentsList) {
  // require exactly the gate and scope arguments
  if (argumentsList.length !== 2) {
    throw new TypeError("source gate accepts gate and scope");
  }
  const [gate, scope] = argumentsList;

  // require the exact closed gate
  if (typeof gate !== "string" || !GATES.has(gate)) {
    throw new TypeError("source gate is invalid");
  }
  // require the exact closed scope
  if (typeof scope !== "string" || !SCOPES.has(scope)) {
    throw new TypeError("source scope is invalid");
  }

  throw unsupportedGateError();
}

// run the fixed command-line boundary
async function main() {
  const argumentsList = process.argv.slice(2);

  // reject every noncanonical argument sequence
  if (
    argumentsList.length !== 4 ||
    argumentsList[0] !== "--gate" ||
    argumentsList[2] !== "--scope" ||
    !GATES.has(argumentsList[1]) ||
    !SCOPES.has(argumentsList[3])
  ) {
    process.stderr.write(USAGE);
    process.exitCode = 2;
    return;
  }

  // convert only pre-proof failures at the cli boundary
  try {
    const document = await buildBackendReferenceTimeSourceGate(
      argumentsList[1],
      argumentsList[3],
    );
    process.stdout.write(`${canonicalJson(document)}\n`);
    // map the closed document verdict
    process.exitCode = document.verdict === "PASS"
      ? 0
      : document.verdict === "INCOMPLETE"
        ? 3
        : 1;
  } catch (error) {
    const envelope = earlyErrorEnvelope(error);
    process.stdout.write(`${canonicalJson(envelope)}\n`);
    process.exitCode = 1;
  }
}

// run only for the direct cli
if (import.meta.main) {
  await main();
}
