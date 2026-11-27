import {
  RESOURCE_INTERVAL_CONTRACT_VERSION,
} from "./backend-performance-resources.mjs";
import {
  receiptSha256,
  validateLiveStatusReceipt,
} from "./backend-performance-receipts.mjs";
import {
  reconcileBackendPerformanceDirectCoverage,
} from "./backend-performance-direct-coverage.mjs";
import {
  BACKEND_PERFORMANCE_DIRECT_APP_AGGREGATE_VERSION,
} from "./backend-performance-direct-app-aggregate.mjs";
import {
  BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_VERSION,
} from "./backend-performance-direct-host-accounting.mjs";

export const BACKEND_PERFORMANCE_DIRECT_NATIVE_READINESS_VERSION =
  "weather-backend-performance-direct-native-readiness/v1";

const REQUIRED_DIRECT_SCOPES = Object.freeze([
  Object.freeze({ accountingScope: "service", scopeId: "api" }),
  Object.freeze({ accountingScope: "service", scopeId: "web" }),
  Object.freeze({ accountingScope: "service", scopeId: "worker" }),
  Object.freeze({ accountingScope: "postgresql", scopeId: "postgres" }),
]);
const DIGEST_PATTERN = /^[a-f0-9]{64}$/u;

// identify one plain evidence record
function isRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    && (Object.getPrototypeOf(value) === Object.prototype
      || Object.getPrototypeOf(value) === null);
}

// compare one exact ordered string list
function sameList(actual, expected) {
  return Array.isArray(actual) && actual.length === expected.length && actual.every(
    // compare one list member
    (value, index) => value === expected[index],
  );
}

// append one stable readiness diagnostic
function addDiagnostic(diagnostics, code, path, message) {
  diagnostics.push({ code, message, path });
}

// validate the identity portion of one published resource interval
function inspectResourceInterval(value, index, sourceReceiptSha256, diagnostics) {
  const path = `$.resourceIntervals[${String(index)}]`;

  // reject any other interval contract
  if (!isRecord(value) || value.contractVersion !== RESOURCE_INTERVAL_CONTRACT_VERSION) {
    addDiagnostic(
      diagnostics,
      "native-readiness.resource-contract",
      path,
      "resource interval does not use the published direct cgroup contract",
    );
    return null;
  }
  const nativeAuthority = isRecord(value.authority)
    && value.authority.kind === "native-linux-cgroup-v2"
    && value.authority.liveEligible === true
    && isRecord(value.authority.nativeCgroup)
    && Object.keys(value.authority.nativeCgroup).length > 0
    && value.authority.scopeCoverage === "direct-process-cgroup-only"
    && sameList(value.authority.unsupportedScopes, ["app-aggregate", "host"]);
  const injectedAuthority = isRecord(value.authority)
    && (value.authority.kind === "injected-test-only"
      || value.authority.liveEligible === false);
  const identityComplete = isRecord(value.sourceBinding)
    && value.sourceBinding.kind === "integration-supplied-live-receipt-binding"
    && DIGEST_PATTERN.test(value.sourceBinding.sha256 ?? "")
    && isRecord(value.cgroupIdentity)
    && DIGEST_PATTERN.test(value.cgroupIdentity.sha256 ?? "")
    && isRecord(value.before)
    && isRecord(value.after)
    && value.before.cgroupId === value.cgroupIdentity.sha256
    && value.after.cgroupId === value.cgroupIdentity.sha256
    && value.before.releaseId === value.after.releaseId
    && isRecord(value.accounting)
    && value.accounting.aggregateMethod === "direct-single-cgroup"
    && value.accounting.coverage === "measured-process-cgroup-only"
    && value.accounting.sampledServiceSumUsed === false
    && sameList(value.accounting.unsupportedScopes, ["app-aggregate", "host"]);
  const sourceBound = sourceReceiptSha256 !== null
    && value.sourceBinding?.sha256 === sourceReceiptSha256;

  // require exact interval identity before it can cover one direct scope
  if (!identityComplete) {
    addDiagnostic(
      diagnostics,
      "native-readiness.resource-identity",
      path,
      "resource interval identity or direct-scope accounting descriptor is incomplete",
    );
  }
  // distinguish injected collection from native cgroup authority
  if (injectedAuthority) {
    addDiagnostic(
      diagnostics,
      "native-readiness.resource-injected",
      path,
      "resource interval was produced by an injected test collector",
    );
  } else if (!nativeAuthority) {
    addDiagnostic(
      diagnostics,
      "native-readiness.resource-authority",
      path,
      "resource interval lacks native Linux cgroup v2 authority",
    );
  }
  // bind every credited interval to the verified live receipt
  if (!sourceBound) {
    addDiagnostic(
      diagnostics,
      "native-readiness.resource-source",
      path,
      "resource interval is not bound to the verified live-status receipt",
    );
  }
  return {
    accountingScope: value.accountingScope,
    authorityClass: injectedAuthority
      ? "injected-test-only"
      : nativeAuthority ? "native-linux-cgroup-v2" : "unverified",
    cgroupSha256: value.cgroupIdentity?.sha256 ?? null,
    completedAt: value.completedAt ?? null,
    nativeEligible: nativeAuthority && identityComplete && sourceBound,
    releaseId: value.before?.releaseId ?? null,
    scopeId: value.scopeId,
    sourceBound,
    startedAt: value.startedAt ?? null,
  };
}

// inspect the fixed direct service and database accounting set
function inspectResourceCoverage(values, sourceReceiptSha256, diagnostics) {
  const observed = new Map();
  const expected = new Set(REQUIRED_DIRECT_SCOPES.map(
    // index the closed required direct scope set
    (scope) => `${scope.accountingScope}\u0000${scope.scopeId}`,
  ));
  const duplicates = new Set();
  let unexpected = false;

  // inspect every supplied interval once
  for (let index = 0; index < values.length; index += 1) {
    const interval = inspectResourceInterval(
      values[index],
      index,
      sourceReceiptSha256,
      diagnostics,
    );

    // reject malformed interval records from the exact batch
    if (interval === null) {
      unexpected = true;
      continue;
    }
    const key = `${String(interval.accountingScope)}\u0000${String(interval.scopeId)}`;
    const path = `$.resourceIntervals[${String(index)}]`;
    // do not admit caller-selected extra scopes
    if (!expected.has(key)) {
      unexpected = true;
      addDiagnostic(diagnostics, "native-readiness.resource-extra", path,
        "resource interval is outside the fixed direct scope set");
      continue;
    }
    // do not select an arbitrary observation for a repeated scope
    if (observed.has(key)) {
      duplicates.add(key);
      addDiagnostic(diagnostics, "native-readiness.resource-duplicate", path,
        "resource interval repeats a fixed direct scope");
      continue;
    }
    observed.set(key, interval);
  }
  const resources = REQUIRED_DIRECT_SCOPES.map(
    // close one fixed direct accounting requirement
    (required) => {
      const key = `${required.accountingScope}\u0000${required.scopeId}`;
      // withhold the identity and authority of an ambiguous direct scope
      if (duplicates.has(key)) {
        return {
          ...required,
          authorityClass: "ambiguous",
          cgroupSha256: null,
          completedAt: null,
          nativeEligible: false,
          present: true,
          releaseId: null,
          sourceBound: false,
          startedAt: null,
        };
      }
      const interval = observed.get(key) ?? null;

      // report each absent service or database interval independently
      if (interval === null) {
        addDiagnostic(
          diagnostics,
          "native-readiness.resource-missing",
          "$.resourceIntervals",
          `missing native ${required.accountingScope}/${required.scopeId} resource interval`,
        );
      // preserve one exact nonnative scope failure
      } else if (!interval.nativeEligible) {
        addDiagnostic(
          diagnostics,
          "native-readiness.resource-nonnative",
          "$.resourceIntervals",
          `${required.accountingScope}/${required.scopeId} is not verified native cgroup evidence`,
        );
      }
      return {
        ...required,
        authorityClass: interval?.authorityClass ?? null,
        cgroupSha256: interval?.cgroupSha256 ?? null,
        completedAt: interval?.completedAt ?? null,
        nativeEligible: interval?.nativeEligible ?? false,
        present: interval !== null,
        releaseId: interval?.releaseId ?? null,
        sourceBound: interval?.sourceBound ?? false,
        startedAt: interval?.startedAt ?? null,
      };
    },
  );
  return {
    resources,
    scopeSetExact: !unexpected && duplicates.size === 0 && observed.size === expected.size,
  };
}

// inspect one derived application aggregate without widening offline authority
function inspectApplicationAggregate(value, sourceReceiptSha256, directResources, diagnostics) {
  const path = "$.applicationAggregateReceipt";

  // preserve an explicit missing aggregate
  if (value === null || value === undefined) {
    addDiagnostic(
      diagnostics,
      "native-readiness.app-aggregate-missing",
      path,
      "one source-bound application aggregate receipt is required",
    );
    return {
      contractValid: false,
      nativeEligible: false,
      present: false,
      scopesAligned: false,
      sourceBound: false,
      status: "missing",
    };
  }

  // require the published aggregate result contract
  if (!isRecord(value)
    || value.contractVersion !== BACKEND_PERFORMANCE_DIRECT_APP_AGGREGATE_VERSION) {
    addDiagnostic(
      diagnostics,
      "native-readiness.app-aggregate-contract",
      path,
      "application aggregate does not use the published result contract",
    );
    return {
      contractValid: false,
      nativeEligible: false,
      present: true,
      scopesAligned: false,
      sourceBound: false,
      status: "invalid",
    };
  }
  const offlineAuthority = isRecord(value.authority)
    && value.authority.collectionAuthority === "unverified-caller-supplied-offline-intervals"
    && value.authority.nativeEligible === false
    && value.authority.trafficGenerated === false
    && value.metadataKind === "performance-direct-app-aggregate-offline-noncanonical"
    && isRecord(value.identity)
    && Array.isArray(value.services);

  // reject receipts that relabel the fixed offline result contract
  if (!offlineAuthority) {
    addDiagnostic(
      diagnostics,
      "native-readiness.app-aggregate-authority",
      path,
      "application aggregate authority or metadata differs from the published offline result",
    );
    return {
      contractValid: false,
      nativeEligible: false,
      present: true,
      scopesAligned: false,
      sourceBound: false,
      status: "invalid",
    };
  }
  const sourceBound = sourceReceiptSha256 !== null
    && value.identity?.sourceReceiptSha256 === sourceReceiptSha256;
  const serviceResources = directResources.filter(
    // select the fixed application service scopes
    (resource) => resource.accountingScope === "service",
  );
  const receiptServices = Array.isArray(value.services) ? value.services : [];
  const receiptByScope = new Map(receiptServices.map(
    // index one aggregate service identity
    (service) => [service?.scopeId, service],
  ));
  const exactScopeSet = receiptServices.length === serviceResources.length
    && serviceResources.every(
      // require every fixed direct service exactly once
      (resource) => receiptByScope.has(resource.scopeId),
    );
  const scopesAligned = exactScopeSet && serviceResources.every(
    // bind aggregate contributors to exact direct interval identities
    (resource) => {
      const service = receiptByScope.get(resource.scopeId);
      return isRecord(service) && isRecord(service.cgroupIdentity)
        && service.cgroupIdentity.sha256 === resource.cgroupSha256
        && service.nativeAuthority === true
        && value.identity?.releaseId === resource.releaseId
        && value.identity?.startedAt === resource.startedAt
        && value.identity?.completedAt === resource.completedAt;
    },
  );

  // require the current live-status source identity
  if (!sourceBound) {
    addDiagnostic(
      diagnostics,
      "native-readiness.app-aggregate-source",
      path,
      "application aggregate is not bound to the verified live-status receipt",
    );
  }
  // require the exact three application service intervals
  if (!scopesAligned) {
    addDiagnostic(
      diagnostics,
      "native-readiness.app-aggregate-alignment",
      path,
      "application aggregate does not align with the fixed direct service intervals",
    );
  }
  // retain the published evaluator's explicit noncanonical authority boundary
  addDiagnostic(
    diagnostics,
    "native-readiness.app-aggregate-offline-authority",
    path,
    "published application aggregation accepts caller-supplied intervals and cannot establish native collection custody",
  );
  return {
    contractValid: true,
    nativeEligible: false,
    present: true,
    scopesAligned,
    sourceBound,
    status: "offline-noncanonical",
  };
}

// inspect one derived host accounting result without inventing source custody
function inspectHostAccounting(value, diagnostics) {
  const path = "$.hostAccountingReceipt";

  // preserve an explicit missing host result
  if (value === null || value === undefined) {
    addDiagnostic(
      diagnostics,
      "native-readiness.host-accounting-missing",
      path,
      "one live-receipt-bound whole-host accounting receipt is required",
    );
    return {
      contractValid: false,
      nativeEligible: false,
      present: false,
      sourceBound: false,
      status: "missing",
    };
  }

  // require the published host-accounting result contract
  if (!isRecord(value)
    || value.contractVersion !== BACKEND_PERFORMANCE_DIRECT_HOST_ACCOUNTING_VERSION) {
    addDiagnostic(
      diagnostics,
      "native-readiness.host-accounting-contract",
      path,
      "host accounting does not use the published result contract",
    );
    return {
      contractValid: false,
      nativeEligible: false,
      present: true,
      sourceBound: false,
      status: "invalid",
    };
  }
  const offlineAuthority = isRecord(value.authority)
    && value.authority.collectionAuthority === "unverified-caller-supplied-offline-snapshots"
    && value.authority.nativeEligible === false
    && value.authority.trafficGenerated === false
    && value.metadataKind === "performance-direct-host-accounting-offline-noncanonical"
    && Array.isArray(value.snapshots);

  // reject receipts that relabel the fixed offline result contract
  if (!offlineAuthority) {
    addDiagnostic(
      diagnostics,
      "native-readiness.host-accounting-authority",
      path,
      "host accounting authority or metadata differs from the published offline result",
    );
    return {
      contractValid: false,
      nativeEligible: false,
      present: true,
      sourceBound: false,
      status: "invalid",
    };
  }

  // name the missing binding in the current published result schema
  addDiagnostic(
    diagnostics,
    "native-readiness.host-accounting-source-unavailable",
    path,
    "published host accounting has no live-status source binding or collection-custody proof",
  );
  // retain the published evaluator's explicit noncanonical authority boundary
  addDiagnostic(
    diagnostics,
    "native-readiness.host-accounting-offline-authority",
    path,
    "published host accounting accepts caller-supplied snapshots and cannot establish native collection custody",
  );
  return {
    contractValid: true,
    nativeEligible: false,
    present: true,
    sourceBound: false,
    status: "offline-noncanonical",
  };
}

// inspect one validator-admitted live receipt without widening its authority
function inspectLiveStatus(value, validator, validatorInjected, diagnostics) {
  // require one explicit live-status input
  if (!isRecord(value) || !isRecord(value.receipt) || !isRecord(value.validationOptions)) {
    addDiagnostic(
      diagnostics,
      "native-readiness.live-status-missing",
      "$.liveStatus",
      "one live-status receipt and its retained validation inputs are required",
    );
    return {
      architectureClass: "absent",
      arm64NativeHost: false,
      receiptSha256: null,
      validation: null,
    };
  }
  let validation;

  try {
    validation = validator(value.receipt, value.validationOptions);
  } catch (error) {
    addDiagnostic(
      diagnostics,
      "native-readiness.live-status-validator",
      "$.liveStatus.receipt",
      error instanceof Error ? error.message : String(error),
    );
    return {
      architectureClass: "unverified",
      arm64NativeHost: false,
      receiptSha256: null,
      validation: null,
    };
  }

  // reject every receipt that the existing validator did not admit
  if (!isRecord(validation) || validation.ok !== true
    || !DIGEST_PATTERN.test(validation.sha256 ?? "")
    || validation.sha256 !== receiptSha256(value.receipt)) {
    addDiagnostic(
      diagnostics,
      "native-readiness.live-status-invalid",
      "$.liveStatus.receipt",
      "live-status receipt failed the published validator or digest binding",
    );
    return {
      architectureClass: "unverified",
      arm64NativeHost: false,
      receiptSha256: null,
      validation: isRecord(validation) ? validation : null,
    };
  }
  const blueberry = Array.isArray(value.receipt.captures)
    ? value.receipt.captures.find(
      // select the fixed application host capture
      (capture) => capture?.profile === "blueberry-compose"
        && capture?.hostRole === "blueberry",
    )
    : undefined;
  const architecture = blueberry?.observation?.identity?.architecture;
  const physicalCapture = blueberry?.source?.kind === "raw-main-custody"
    && blueberry?.observation?.authority?.kind === "physical-host"
    && blueberry?.observation?.authority?.liveEligible === true;
  const canonicalCustody = value.receipt.authority?.origin === "main-fixed-custody"
    && value.receipt.authority?.canonicalEligible === true;
  const arm64NativeHost = !validatorInjected && architecture === "aarch64"
    && physicalCapture && canonicalCustody;

  // retain structural fixture validation without promoting it
  if (validatorInjected) {
    addDiagnostic(
      diagnostics,
      "native-readiness.live-status-validator-injected",
      "$.liveStatus.receipt",
      "injected receipt validation cannot establish physical readiness",
    );
  }
  // require canonical physical ARM64 host custody
  if (!arm64NativeHost) {
    addDiagnostic(
      diagnostics,
      architecture === "aarch64"
        ? "native-readiness.host-arm64-noncanonical"
        : "native-readiness.host-not-arm64",
      "$.liveStatus.receipt.captures",
      architecture === "aarch64"
        ? "Blueberry reports ARM64 but lacks canonical physical Main-custody validation"
        : "verified Blueberry receipt does not report an ARM64 host",
    );
  }
  return {
    architectureClass: architecture === "aarch64"
      ? arm64NativeHost ? "arm64-native" : "arm64-noncanonical-or-injected"
      : typeof architecture === "string" ? "non-arm64" : "unknown",
    arm64NativeHost,
    receiptSha256: validation.sha256,
    validation: {
      canonicalEligible: validation.canonicalEligible === true,
      diagnostics: Array.isArray(validation.diagnostics) ? validation.diagnostics : [],
      ok: true,
    },
  };
}

// assess one read-only future native batch without collecting host data
export function assessBackendPerformanceDirectNativeReadiness(input, adapters = {}) {
  const requiredKeys = ["liveStatus", "operationReceipts", "resourceIntervals"];
  const optionalKeys = ["applicationAggregateReceipt", "hostAccountingReceipt"];
  const inputKeys = isRecord(input) ? Object.keys(input) : [];

  // reject open caller-selected requirements
  if (!isRecord(input)
    || requiredKeys.some(
      // require every retained input batch
      (key) => !inputKeys.includes(key),
    )
    || inputKeys.some(
      // reject every unrecognized authority surface
      (key) => !requiredKeys.includes(key) && !optionalKeys.includes(key),
    )
    || !Array.isArray(input.operationReceipts) || !Array.isArray(input.resourceIntervals)
    || !isRecord(adapters)
    || Object.keys(adapters).some(
      // allow only the explicitly nonphysical test validator seam
      (key) => key !== "validateLiveStatusReceipt",
    )
    || (adapters.validateLiveStatusReceipt !== undefined
      && typeof adapters.validateLiveStatusReceipt !== "function")) {
    const error = new Error("native readiness input must contain only fixed receipt batches");
    error.code = "native-readiness.arguments";
    throw error;
  }
  const diagnostics = [];
  const validatorInjected = adapters.validateLiveStatusReceipt !== undefined;
  const liveStatus = inspectLiveStatus(
    input.liveStatus,
    adapters.validateLiveStatusReceipt ?? validateLiveStatusReceipt,
    validatorInjected,
    diagnostics,
  );
  const operations = reconcileBackendPerformanceDirectCoverage(input.operationReceipts);

  // summarize the complete source-derived native operation shortfall once
  if (!operations.nativeComplete) {
    addDiagnostic(
      diagnostics,
      "native-readiness.operation-coverage",
      "$.operationReceipts",
      `${String(operations.summary.missingNativeBuckets)} operation-stratum buckets and ${String(operations.summary.missingNativeAttempts)} planned attempts lack native evidence`,
    );
  }
  const directCoverage = inspectResourceCoverage(
    input.resourceIntervals,
    liveStatus.receiptSha256,
    diagnostics,
  );
  const directResources = directCoverage.resources;
  const applicationAggregate = inspectApplicationAggregate(
    input.applicationAggregateReceipt,
    liveStatus.receiptSha256,
    directResources,
    diagnostics,
  );
  const hostAccounting = inspectHostAccounting(
    input.hostAccountingReceipt,
    diagnostics,
  );
  diagnostics.sort(
    // retain deterministic batch diagnostics
    (left, right) => JSON.stringify(left).localeCompare(JSON.stringify(right)),
  );
  const ready = liveStatus.arm64NativeHost
    && operations.nativeComplete
    && directCoverage.scopeSetExact
    && directResources.every(
      // require every fixed service and database interval
      (resource) => resource.nativeEligible,
    )
    && applicationAggregate.nativeEligible
    && hostAccounting.nativeEligible;
  return Object.freeze({
    contractVersion: BACKEND_PERFORMANCE_DIRECT_NATIVE_READINESS_VERSION,
    diagnostics,
    host: {
      architectureClass: liveStatus.architectureClass,
      arm64NativeHost: liveStatus.arm64NativeHost,
      wholeHostAccounting: hostAccounting.status,
    },
    liveStatus: {
      receiptSha256: liveStatus.receiptSha256,
      validation: liveStatus.validation,
      validatorAuthority: validatorInjected ? "injected-test-only" : "published-validator",
    },
    operations,
    physicalNativeReady: ready,
    resources: {
      applicationAggregate,
      direct: directResources,
      directScopeSetExact: directCoverage.scopeSetExact,
      host: hostAccounting,
    },
    status: ready ? "ready" : "blocked",
  });
}
