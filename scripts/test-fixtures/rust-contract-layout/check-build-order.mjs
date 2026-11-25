import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  buildWorkspaces,
  discoverWorkspaces,
  orderWorkspaces,
} from "../../build-workspaces.mjs";

const CONTRACTS_NAME = "@weather/contracts";
const repositoryRoot = new URL("../../../", import.meta.url);
const config = JSON.parse(readFileSync(
  new URL("../../../config/rust-contract-roots.json", import.meta.url),
  "utf8",
));
const expectedWorkspaces = [
  {
    dependencies: [
      "@weather/database",
      "@weather/domain",
      "@weather/forecast-adjustment",
    ],
    name: "@weather/api",
    path: "apps/api",
  },
  {
    dependencies: [],
    name: "@weather/web",
    path: "apps/web",
  },
  {
    dependencies: [
      "@weather/database",
      "@weather/domain",
      "@weather/forecast-adjustment",
      "@weather/providers",
    ],
    name: "@weather/worker",
    path: "apps/worker",
  },
  {
    dependencies: ["@weather/domain"],
    name: "@weather/database",
    path: "packages/database",
  },
  {
    dependencies: [],
    name: "@weather/domain",
    path: "packages/domain",
  },
  {
    dependencies: ["@weather/domain"],
    name: "@weather/forecast-adjustment",
    path: "packages/forecast-adjustment",
  },
  {
    dependencies: ["@weather/domain"],
    name: "@weather/providers",
    path: "packages/providers",
  },
];
const expectedOrder = [
  "@weather/domain",
  "@weather/database",
  "@weather/forecast-adjustment",
  "@weather/api",
  "@weather/providers",
  "@weather/web",
  "@weather/worker",
];
const expectedRootOwners = {
  "apps/api": 48,
  "apps/web": 10,
  "apps/worker": 30,
  "packages/database": 87,
  "packages/domain": 75,
  "packages/forecast-adjustment": 118,
  "packages/providers": 18,
};

// resolve one configured source to its owning workspace path
function workspacePath(source) {
  const match = /^(?:apps|packages)\/[^/]+/u.exec(source);
  assert.ok(match, source);
  return match[0];
}

// clone one graph without sharing mutable dependency arrays
function cloneWorkspaces(workspaces) {
  return workspaces.map(
    // retain exact package identity while isolating graph experiments
    (workspace) => ({
      ...workspace,
      dependencies: [...workspace.dependencies],
    }),
  );
}

// list simulated workspace nodes missing one direct contracts edge
function missingSimulatedDirectEdges(workspaces, simulatedConsumers) {
  return workspaces.filter(
    // exclude other nodes and retain only simulated missing edges
    (workspace) =>
      simulatedConsumers.has(workspace.name) &&
      !workspace.dependencies.includes(CONTRACTS_NAME),
  ).map(
    // stabilize evidence independently of graph insertion order
    (workspace) => workspace.name,
  ).sort();
}

const discovered = await discoverWorkspaces(repositoryRoot);
const discoveredSnapshot = JSON.stringify(discovered);
assert.equal(discovered.length, 7);
assert.deepEqual(
  [...discovered].sort((left, right) => left.path.localeCompare(right.path)),
  expectedWorkspaces,
);
const actualOrder = orderWorkspaces(discovered).map(
  // report only stable workspace identities
  (workspace) => workspace.name,
);
assert.deepEqual(actualOrder, expectedOrder);

const rootOwnerCounts = Object.fromEntries(
  Object.keys(expectedRootOwners).map(
    // initialize every expected current owner before counting roots
    (owner) => [owner, 0],
  ),
);
assert.equal(config.roots.length, 386);

// bind every configured root to one of the seven current owner workspaces
for (const root of config.roots) {
  const owner = workspacePath(root.source);
  assert.ok(Object.hasOwn(rootOwnerCounts, owner), root.source);
  rootOwnerCounts[owner] += 1;
}
assert.deepEqual(rootOwnerCounts, expectedRootOwners);
assert.equal(config.typeImports.length, 54);
const typeImportWorkspaceCandidates = new Set();

// record import candidates without using them to define the simulated edge set
for (const edge of config.typeImports) {
  const sources = [
    edge.consumer,
    edge.entry,
    ...Object.values(edge.exports),
    ...Object.values(edge.proofExports ?? {}),
  ];

  // retain every current source workspace touched by the configured edge
  for (const source of sources) {
    typeImportWorkspaceCandidates.add(workspacePath(source));
  }
}
assert.deepEqual(
  [...typeImportWorkspaceCandidates].sort(),
  Object.keys(expectedRootOwners).sort(),
);

const simulatedConsumers = new Set(expectedWorkspaces.map(
  // select all seven owners for the deliberate virtual stress graph
  (workspace) => workspace.name,
));
const neutralWorkspace = {
  dependencies: [],
  name: CONTRACTS_NAME,
  path: "virtual/contracts",
};
const withContracts = [
  neutralWorkspace,
  ...cloneWorkspaces(discovered).map(
    // add one deliberate direct edge to every simulated owner node
    (workspace) => ({
      ...workspace,
      dependencies: [...workspace.dependencies, CONTRACTS_NAME],
    }),
  ),
];
assert.deepEqual(
  missingSimulatedDirectEdges(withContracts, simulatedConsumers),
  [],
);
const neutralOrder = orderWorkspaces(withContracts).map(
  // report one stable neutral graph order
  (workspace) => workspace.name,
);
assert.deepEqual(neutralOrder, [CONTRACTS_NAME, ...expectedOrder]);
assert.equal(
  neutralOrder.filter(
    // count only the one virtual contracts node
    (name) => name === CONTRACTS_NAME,
  ).length,
  1,
);
assert.deepEqual(
  neutralOrder.filter(
    // compare the unchanged ordinary graph independently of the virtual node
    (name) => name !== CONTRACTS_NAME,
  ),
  actualOrder,
);

const unconnectedGraph = [
  neutralWorkspace,
  ...cloneWorkspaces(discovered),
];
const unconnectedOrder = orderWorkspaces(unconnectedGraph).map(
  // report the ordinary sorter result without direct contracts edges
  (workspace) => workspace.name,
);
assert.deepEqual(unconnectedOrder, [
  "@weather/domain",
  "@weather/database",
  "@weather/forecast-adjustment",
  "@weather/api",
  CONTRACTS_NAME,
  "@weather/providers",
  "@weather/web",
  "@weather/worker",
]);
assert.ok(
  unconnectedOrder.indexOf("@weather/api") <
    unconnectedOrder.indexOf(CONTRACTS_NAME),
);
const unconnectedMissingSimulatedConsumers = missingSimulatedDirectEdges(
  unconnectedGraph,
  simulatedConsumers,
);
assert.deepEqual(
  unconnectedMissingSimulatedConsumers,
  [...simulatedConsumers].sort(),
);

const removedApiEdgeGraph = withContracts.map(
  // remove only the api's direct edge while preserving its transitive graph
  (workspace) => workspace.name === "@weather/api"
    ? {
      ...workspace,
      dependencies: workspace.dependencies.filter(
        // retain every existing dependency except the neutral direct edge
        (dependency) => dependency !== CONTRACTS_NAME,
      ),
    }
    : {
      ...workspace,
      dependencies: [...workspace.dependencies],
    },
);
const removedApiOrder = orderWorkspaces(removedApiEdgeGraph).map(
  // compare available order separately from the direct-edge condition
  (workspace) => workspace.name,
);
assert.ok(
  removedApiOrder.indexOf(CONTRACTS_NAME) <
    removedApiOrder.indexOf("@weather/api"),
);
assert.deepEqual(removedApiOrder, neutralOrder);
assert.deepEqual(
  missingSimulatedDirectEdges(removedApiEdgeGraph, simulatedConsumers),
  ["@weather/api"],
);
const removedApi = removedApiEdgeGraph.find(
  // select the sole deliberately disconnected simulated node
  (workspace) => workspace.name === "@weather/api",
);
assert.ok(removedApi);
assert.ok(removedApi.dependencies.includes("@weather/database"));
assert.ok(removedApi.dependencies.includes("@weather/forecast-adjustment"));

// prove both retained api dependencies still supply transitive ordering
for (const dependencyName of [
  "@weather/database",
  "@weather/forecast-adjustment",
]) {
  const dependency = removedApiEdgeGraph.find(
    // select one retained transitive dependency
    (workspace) => workspace.name === dependencyName,
  );
  assert.ok(dependency, dependencyName);
  assert.ok(dependency.dependencies.includes(CONTRACTS_NAME), dependencyName);
}

const reverseCycles = [];

// prove each owner would cycle if contracts depended back on its source types
for (const owner of expectedWorkspaces) {
  const cycleGraph = withContracts.map(
    // inject one hypothetical reverse dependency on this owner only
    (workspace) => workspace.name === CONTRACTS_NAME
      ? { ...workspace, dependencies: [owner.name] }
      : { ...workspace, dependencies: [...workspace.dependencies] },
  );
  let callbackCount = 0;
  await assert.rejects(
    buildWorkspaces(cycleGraph, async () => {
      callbackCount += 1;
    }),
    /workspace dependency cycle/u,
  );
  assert.equal(callbackCount, 0, owner.name);
  reverseCycles.push(owner.name);
}
assert.deepEqual(
  reverseCycles,
  expectedWorkspaces.map(
    // retain the exact seven owner identities in source-owner order
    (workspace) => workspace.name,
  ),
);

const failedCallbacks = [];
await assert.rejects(
  buildWorkspaces(withContracts, async (workspace) => {
    failedCallbacks.push(workspace.name);
    throw new Error("injected contracts compile failure");
  }),
  /injected contracts compile failure/u,
);
assert.deepEqual(failedCallbacks, [CONTRACTS_NAME]);
assert.equal(JSON.stringify(discovered), discoveredSnapshot);

process.stdout.write(`${JSON.stringify({
  actualOrder,
  neutralOrder,
  unconnectedOrder,
  noDirect: {
    apiBeforeContracts: true,
    missingSimulatedConsumers: unconnectedMissingSimulatedConsumers,
  },
  missingDirect: {
    simulatedConsumer: "@weather/api",
    directEdgePresent: false,
    contractsBeforeConsumer: true,
    orderAvailableTransitively: true,
  },
  callbackSmoke: {
    attempted: failedCallbacks,
    dependentCallbacks: failedCallbacks.length - 1,
  },
  reverseCycles,
})}\n`);
