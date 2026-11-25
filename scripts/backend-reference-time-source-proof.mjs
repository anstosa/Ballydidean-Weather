const INPUT_KEYS = Object.freeze(["graph", "occurrences", "policy", "scope"]);
const SCOPES = new Set([
  "current-source",
  "implemented-source",
  "physical-baseline",
]);

// read one exact plain data record
function exactDataRecord(value, expectedKeys, label) {
  // require one ordinary record
  if (
    value === null ||
    typeof value !== "object" ||
    (Object.getPrototypeOf(value) !== Object.prototype &&
      Object.getPrototypeOf(value) !== null)
  ) {
    throw new TypeError(`${label} must be a plain record`);
  }
  const ownKeys = Reflect.ownKeys(value);

  // require the exact reviewed keys
  if (
    ownKeys.length !== expectedKeys.length ||
    expectedKeys.some(
      // locate one missing reviewed key
      (key) => !ownKeys.includes(key),
    )
  ) {
    throw new TypeError(`${label} keys are invalid`);
  }
  const result = Object.create(null);

  // read only enumerable data properties
  for (const key of expectedKeys) {
    const descriptor = Object.getOwnPropertyDescriptor(value, key);

    // reject accessors and hidden values
    if (
      descriptor === undefined ||
      descriptor.enumerable !== true ||
      !("value" in descriptor)
    ) {
      throw new TypeError(`${label} fields are invalid`);
    }
    result[key] = descriptor.value;
  }
  return result;
}

// recognize one plain checked document
function isPlainRecord(value) {
  return value !== null &&
    typeof value === "object" &&
    (Object.getPrototypeOf(value) === Object.prototype ||
      Object.getPrototypeOf(value) === null);
}

// create one sanitized pre-proof failure
function unsupportedProofError() {
  const error = new Error("internal.failure");
  error.code = "internal.failure";
  error.stage = "internal";
  return error;
}

// validate the proof boundary without fabricating child documents
export function buildBackendReferenceTimeSourceProof(...argumentsList) {
  // require exactly one proof envelope
  if (argumentsList.length !== 1) {
    throw new TypeError("proof accepts one input record");
  }
  const input = exactDataRecord(argumentsList[0], INPUT_KEYS, "proof input");

  // require checked graph and policy records
  if (!isPlainRecord(input.graph) || !isPlainRecord(input.policy)) {
    throw new TypeError("proof documents are invalid");
  }
  // require an explicit occurrence array
  if (!Array.isArray(input.occurrences)) {
    throw new TypeError("proof occurrences are invalid");
  }
  // require the exact closed scope
  if (!SCOPES.has(input.scope)) {
    throw new TypeError("proof scope is invalid");
  }

  throw unsupportedProofError();
}
