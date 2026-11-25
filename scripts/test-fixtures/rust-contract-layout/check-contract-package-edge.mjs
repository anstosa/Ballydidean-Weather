import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

import ts from "typescript";

const PACKAGE_NAME = "@weather/contracts";
const TYPESCRIPT_VERSION = "6.0.3";
const SCOPE = {
  kind: "synthetic-literal-ast-preflight",
  literalFormsOnly: true,
  packageIntegration: false,
};
const CASES = [
  {
    id: "whole-type-import",
    source: `import type { Contract } from "@weather/contracts";
export type LocalContract = Contract;
`,
    diagnosticIds: [],
    astReferences: 1,
    emitRuntimeReferences: 0,
    outcome: "allowed-erased",
  },
  {
    id: "whole-type-export-subpath",
    source: `export type { Contract } from "@weather/contracts/models";
`,
    diagnosticIds: [],
    astReferences: 1,
    emitRuntimeReferences: 0,
    outcome: "allowed-erased",
  },
  {
    id: "unrelated-package-and-text",
    source: `import { runtimeValue } from "@weather/other";
const text = "@weather/contracts";
// import "@weather/contracts";
console.log("@weather/contracts");
void runtimeValue;
void text;
`,
    diagnosticIds: [],
    astReferences: 0,
    emitRuntimeReferences: 0,
    outcome: "allowed-unrelated",
  },
  {
    id: "value-import",
    source: `import { runtimeValue } from "@weather/contracts";
void runtimeValue;
`,
    diagnosticIds: ["value-import"],
    astReferences: 1,
    emitRuntimeReferences: 1,
    outcome: "rejected-runtime",
  },
  {
    id: "value-export",
    source: `export { runtimeValue } from "@weather/contracts";
`,
    diagnosticIds: ["value-export"],
    astReferences: 1,
    emitRuntimeReferences: 1,
    outcome: "rejected-runtime",
  },
  {
    id: "inline-type-import",
    source: `import { type Contract } from "@weather/contracts";
`,
    diagnosticIds: ["inline-type-import"],
    astReferences: 1,
    emitRuntimeReferences: 1,
    outcome: "rejected-runtime",
  },
  {
    id: "mixed-import",
    source: `import runtimeDefault, { type Contract } from "@weather/contracts";
void runtimeDefault;
`,
    diagnosticIds: ["mixed-import"],
    astReferences: 1,
    emitRuntimeReferences: 1,
    outcome: "rejected-runtime",
  },
  {
    id: "inline-type-export",
    source: `export { type Contract } from "@weather/contracts";
`,
    diagnosticIds: ["inline-type-export"],
    astReferences: 1,
    emitRuntimeReferences: 1,
    outcome: "rejected-runtime",
  },
  {
    id: "mixed-export",
    source: `export { type Contract, runtimeValue } from "@weather/contracts";
`,
    diagnosticIds: ["mixed-export"],
    astReferences: 1,
    emitRuntimeReferences: 1,
    outcome: "rejected-runtime",
  },
  {
    id: "bare-side-effect-import",
    source: `import "@weather/contracts";
`,
    diagnosticIds: ["side-effect-import"],
    astReferences: 1,
    emitRuntimeReferences: 1,
    outcome: "rejected-runtime",
  },
  {
    id: "literal-dynamic-import",
    source: `void import("@weather/contracts");
`,
    diagnosticIds: ["dynamic-import"],
    astReferences: 1,
    emitRuntimeReferences: 1,
    outcome: "rejected-runtime",
  },
  {
    id: "direct-literal-require",
    source: `declare function require(specifier: string): unknown;
void require("@weather/contracts");
`,
    diagnosticIds: ["require-call"],
    astReferences: 1,
    emitRuntimeReferences: 1,
    outcome: "rejected-runtime",
  },
  {
    id: "direct-import-equals",
    source: `import Contracts = require("@weather/contracts");
void Contracts;
`,
    diagnosticIds: ["import-equals"],
    astReferences: 1,
    emitRuntimeReferences: null,
    outcome: "rejected-source-only",
  },
  {
    id: "import-type-reference",
    source: `export type Contract = import("@weather/contracts").Contract;
`,
    diagnosticIds: ["import-type-reference"],
    astReferences: 1,
    emitRuntimeReferences: 0,
    outcome: "rejected-erased",
  },
];

// recognize only the exact literal package family used by this preflight
function isTargetSpecifier(node) {
  return (
    ts.isStringLiteral(node) &&
    (node.text === PACKAGE_NAME || node.text.startsWith(`${PACKAGE_NAME}/`))
  );
}

// read one literal from an import type node
function importTypeSpecifier(node) {
  return (
    ts.isLiteralTypeNode(node.argument) &&
    isTargetSpecifier(node.argument.literal)
  );
}

// classify one target import declaration
function importDiagnosticId(node) {
  const clause = node.importClause;

  // allow only whole-declaration import type syntax
  if (clause?.isTypeOnly === true) {
    return null;
  }

  // reject bare evaluation imports separately
  if (clause === undefined) {
    return "side-effect-import";
  }
  const bindings = clause.namedBindings;

  // reject default and namespace value bindings
  if (bindings === undefined || ts.isNamespaceImport(bindings)) {
    return "value-import";
  }
  const typeCount = bindings.elements.filter(
    // count only inline type specifiers
    (element) => element.isTypeOnly,
  ).length;

  // distinguish pure inline type syntax from a default value plus inline types
  if (typeCount === bindings.elements.length && typeCount > 0) {
    return clause.name === undefined ? "inline-type-import" : "mixed-import";
  }

  // preserve one reason for mixed type and value clauses
  if (typeCount > 0) {
    return "mixed-import";
  }
  return "value-import";
}

// classify one target export declaration
function exportDiagnosticId(node) {
  // allow only whole-declaration export type syntax
  if (node.isTypeOnly) {
    return null;
  }
  const clause = node.exportClause;

  // reject export star and other value export forms
  if (clause === undefined || ts.isNamespaceExport(clause)) {
    return "value-export";
  }
  const typeCount = clause.elements.filter(
    // count only inline type specifiers
    (element) => element.isTypeOnly,
  ).length;

  // distinguish pure inline type syntax from mixed clauses
  if (typeCount === clause.elements.length && typeCount > 0) {
    return "inline-type-export";
  }

  // preserve one reason for mixed type and value clauses
  if (typeCount > 0) {
    return "mixed-export";
  }
  return "value-export";
}

// inspect only exact literal source forms named by the bounded wording
function inspectSource(sourceFile) {
  const diagnosticIds = [];
  let astReferences = 0;

  // visit every syntax node without resolving modules or bindings
  function visit(node) {
    // classify one static import declaration
    if (ts.isImportDeclaration(node) && isTargetSpecifier(node.moduleSpecifier)) {
      astReferences += 1;
      const id = importDiagnosticId(node);

      // retain only rejected reason categories
      if (id !== null) {
        diagnosticIds.push(id);
      }
      return;
    }

    // classify one static export declaration
    if (
      ts.isExportDeclaration(node) &&
      node.moduleSpecifier !== undefined &&
      isTargetSpecifier(node.moduleSpecifier)
    ) {
      astReferences += 1;
      const id = exportDiagnosticId(node);

      // retain only rejected reason categories
      if (id !== null) {
        diagnosticIds.push(id);
      }
      return;
    }

    // reject one direct literal import-equals reference
    if (
      ts.isImportEqualsDeclaration(node) &&
      ts.isExternalModuleReference(node.moduleReference) &&
      node.moduleReference.expression !== undefined &&
      isTargetSpecifier(node.moduleReference.expression)
    ) {
      astReferences += 1;
      diagnosticIds.push("import-equals");
      return;
    }

    // reject one exact import type reference even though emit erases it
    if (ts.isImportTypeNode(node) && importTypeSpecifier(node)) {
      astReferences += 1;
      diagnosticIds.push("import-type-reference");
      return;
    }

    // reject direct literal dynamic imports and require calls
    if (ts.isCallExpression(node) && node.arguments.length > 0) {
      const [specifier] = node.arguments;

      // recognize the exact literal dynamic import form
      if (
        node.expression.kind === ts.SyntaxKind.ImportKeyword &&
        isTargetSpecifier(specifier)
      ) {
        astReferences += 1;
        diagnosticIds.push("dynamic-import");
        return;
      }

      // recognize the exact direct require identifier form
      if (
        ts.isIdentifier(node.expression) &&
        node.expression.text === "require" &&
        isTargetSpecifier(specifier)
      ) {
        astReferences += 1;
        diagnosticIds.push("require-call");
        return;
      }
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return { astReferences, diagnosticIds };
}

// count direct literal runtime references that survive javascript emit
function emittedRuntimeReferenceCount(sourceFile) {
  let count = 0;

  // visit emitted syntax without interpreting unrelated string literals
  function visit(node) {
    // count live static import and export declarations
    if (
      (ts.isImportDeclaration(node) || ts.isExportDeclaration(node)) &&
      node.moduleSpecifier !== undefined &&
      isTargetSpecifier(node.moduleSpecifier)
    ) {
      count += 1;
      return;
    }

    // count only direct dynamic import and require calls
    if (
      ts.isCallExpression(node) &&
      node.arguments.length > 0 &&
      (
        node.expression.kind === ts.SyntaxKind.ImportKeyword ||
        (
          ts.isIdentifier(node.expression) &&
          node.expression.text === "require"
        )
      ) &&
      isTargetSpecifier(node.arguments[0])
    ) {
      count += 1;
      return;
    }
    ts.forEachChild(node, visit);
  }

  visit(sourceFile);
  return count;
}

// parse one in-memory module and reject syntax diagnostics
function parseModule(source, fileName, scriptKind) {
  const sourceFile = ts.createSourceFile(
    fileName,
    source,
    ts.ScriptTarget.ES2023,
    true,
    scriptKind,
  );
  assert.equal(sourceFile.parseDiagnostics.length, 0, fileName);
  return sourceFile;
}

assert.equal(ts.version, TYPESCRIPT_VERSION);
const workspaceRoot = fileURLToPath(new URL("../../../", import.meta.url));
const baseConfig = JSON.parse(
  readFileSync(
    new URL("../../../tsconfig.base.json", import.meta.url),
    "utf8",
  ),
);
assert.equal(baseConfig.compilerOptions.module, "NodeNext");
assert.equal(baseConfig.compilerOptions.moduleResolution, "NodeNext");
assert.equal(baseConfig.compilerOptions.target, "ES2023");
assert.equal(baseConfig.compilerOptions.verbatimModuleSyntax, true);
const converted = ts.convertCompilerOptionsFromJson(
  baseConfig.compilerOptions,
  workspaceRoot,
);
assert.equal(converted.errors.length, 0);
assert.equal(converted.options.module, ts.ModuleKind.NodeNext);
assert.equal(converted.options.moduleResolution, ts.ModuleResolutionKind.NodeNext);
assert.equal(converted.options.target, ts.ScriptTarget.ES2023);
assert.equal(converted.options.verbatimModuleSyntax, true);

const results = [];

// validate every finite source and emitted ast case in declared order
for (const fixture of CASES) {
  const virtualFileName = resolve(
    workspaceRoot,
    "scripts/test-fixtures/rust-contract-layout",
    `${fixture.id}.virtual.mts`,
  );
  const sourceFile = parseModule(
    fixture.source,
    virtualFileName,
    ts.ScriptKind.TS,
  );
  const inspected = inspectSource(sourceFile);
  assert.deepEqual(inspected.diagnosticIds, fixture.diagnosticIds, fixture.id);
  assert.equal(inspected.astReferences, fixture.astReferences, fixture.id);
  let emitRuntimeReferences = null;

  // keep import-equals evidence source-only instead of inferring helper emit
  if (fixture.emitRuntimeReferences !== null) {
    const transpiled = ts.transpileModule(fixture.source, {
      compilerOptions: converted.options,
      fileName: virtualFileName,
      reportDiagnostics: true,
    });
    assert.equal(transpiled.diagnostics?.length ?? 0, 0, fixture.id);
    const emittedFile = parseModule(
      transpiled.outputText,
      virtualFileName.replace(/\.mts$/u, ".mjs"),
      ts.ScriptKind.JS,
    );
    emitRuntimeReferences = emittedRuntimeReferenceCount(emittedFile);
    assert.equal(
      emitRuntimeReferences,
      fixture.emitRuntimeReferences,
      fixture.id,
    );
  }
  results.push({
    id: fixture.id,
    astReferences: inspected.astReferences,
    diagnosticIds: inspected.diagnosticIds,
    emitRuntimeReferences,
    outcome: fixture.outcome,
  });
}

const representativeRuntimeIds = new Set([
  "value-import",
  "bare-side-effect-import",
  "mixed-import",
  "inline-type-import",
  "literal-dynamic-import",
  "direct-literal-require",
]);

// require representative live forms to retain one emitted package reference
for (const result of results) {
  // verify the selected live syntax categories independently
  if (representativeRuntimeIds.has(result.id)) {
    assert.equal(result.emitRuntimeReferences, 1, result.id);
  }
}

// keep whole type declarations erased from emitted runtime syntax
for (const result of results.filter(
  // select only the two allowed whole-declaration cases
  (entry) => entry.outcome === "allowed-erased",
)) {
  assert.equal(result.emitRuntimeReferences, 0, result.id);
}

process.stdout.write(`${JSON.stringify({
  typescriptVersion: ts.version,
  scope: SCOPE,
  cases: results,
})}\n`);
