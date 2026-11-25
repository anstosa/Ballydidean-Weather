import { createHash } from "node:crypto";

import ts from "typescript";

export const BACKEND_CLOCK_OCCURRENCES_VERSION =
  "weather-backend-clock-occurrences/v1";

const EXPECTED_TYPESCRIPT_VERSION = "6.0.3";
const SOURCE_BYTE_LIMIT = 1_048_576;
const LANGUAGE_BY_EXTENSION = new Map([
  [".cjs", { language: "javascript", scriptKind: ts.ScriptKind.JS }],
  [".cts", { language: "typescript", scriptKind: ts.ScriptKind.TS }],
  [".js", { language: "javascript", scriptKind: ts.ScriptKind.JS }],
  [".jsx", { language: "javascript", scriptKind: ts.ScriptKind.JSX }],
  [".mjs", { language: "javascript", scriptKind: ts.ScriptKind.JS }],
  [".mts", { language: "typescript", scriptKind: ts.ScriptKind.TS }],
  [".sql", { language: "sql", scriptKind: null }],
  [".ts", { language: "typescript", scriptKind: ts.ScriptKind.TS }],
  [".tsx", { language: "typescript", scriptKind: ts.ScriptKind.TSX }],
]);
const TIMER_SURFACES = new Map([
  ["setTimeout", "node.set-timeout"],
  ["setInterval", "node.set-interval"],
  ["setImmediate", "node.set-immediate"],
]);
const WRAPPER_SURFACES = new Map([
  ["now", "ecmascript.wrapper-now"],
  ["currentDate", "ecmascript.wrapper-current-date"],
  ["defaultNow", "ecmascript.wrapper-default-now"],
]);
const CALL_SURFACES = new Map([
  ["Date.now", ["ecmascript.date-now", "current-time-read"]],
  ["Date.parse", ["ecmascript.date-parse", "parse-only"]],
  ["Date.UTC", ["ecmascript.date-utc", "parse-only"]],
  ["performance.now", ["node.performance-now", "relative-clock-read"]],
  ["process.hrtime", ["node.process-hrtime", "relative-clock-read"]],
  ["process.hrtime.bigint", ["node.process-hrtime-bigint", "relative-clock-read"]],
  ["process.uptime", ["node.process-uptime", "relative-clock-read"]],
  ["os.uptime", ["node.os-uptime", "relative-clock-read"]],
  ["AbortSignal.timeout", ["node.abort-signal-timeout", "timer-schedule"]],
  ["process.cpuUsage", ["node.process-cpu-usage", "resource-sample"]],
  ["process.resourceUsage", ["node.process-resource-usage", "resource-sample"]],
  ["process.memoryUsage", ["node.process-memory-usage", "resource-sample"]],
  ["process.memoryUsage.rss", ["node.process-memory-usage-rss", "resource-sample"]],
]);
const PROPERTY_SURFACES = new Map([
  ["performance.timeOrigin", ["node.performance-time-origin", "relative-clock-read"]],
]);
const SQL_KEYWORD_SURFACES = new Map([
  ["current_date", "postgres.current-date"],
  ["current_time", "postgres.current-time"],
  ["current_timestamp", "postgres.current-timestamp"],
  ["localtime", "postgres.localtime"],
  ["localtimestamp", "postgres.localtimestamp"],
]);
const SQL_PRECISION_KEYWORDS = new Set([
  "current_time",
  "current_timestamp",
  "localtime",
  "localtimestamp",
]);
const SQL_FUNCTION_SURFACES = new Map([
  ["transaction_timestamp", "postgres.transaction-timestamp"],
  ["statement_timestamp", "postgres.statement-timestamp"],
  ["clock_timestamp", "postgres.clock-timestamp"],
  ["now", "postgres.now"],
  ["timeofday", "postgres.timeofday"],
]);
const SQL_FIXED_CALLEES = new Set(["query", "execute", "prepare", "unsafe"]);
const SQL_ANCHOR_PATTERN = /(?:\b(?:CURRENT_DATE|CURRENT_TIME|CURRENT_TIMESTAMP|LOCALTIME|LOCALTIMESTAMP)\b|\b(?:transaction_timestamp|statement_timestamp|clock_timestamp|now|timeofday)\s*\(|\bTIMESTAMP\s*'now'|'now'\s*::\s*(?:date|time|timestamp)\b|\$[A-Za-z_][A-Za-z0-9_]*\$|\$\$)/iu;
const DIRECT_IMPORT_SURFACES = new Map([
  ["node:timers:setTimeout", ["node.set-timeout", "timer-schedule"]],
  ["node:timers:setInterval", ["node.set-interval", "timer-schedule"]],
  ["node:timers:setImmediate", ["node.set-immediate", "timer-schedule"]],
  ["node:timers/promises:setTimeout", ["node.set-timeout", "timer-schedule"]],
  ["node:timers/promises:setInterval", ["node.set-interval", "timer-schedule"]],
  ["node:timers/promises:setImmediate", ["node.set-immediate", "timer-schedule"]],
  ["node:process:hrtime", ["node.process-hrtime", "relative-clock-read"]],
  ["node:process:uptime", ["node.process-uptime", "relative-clock-read"]],
  ["node:process:cpuUsage", ["node.process-cpu-usage", "resource-sample"]],
  ["node:process:resourceUsage", ["node.process-resource-usage", "resource-sample"]],
  ["node:process:memoryUsage", ["node.process-memory-usage", "resource-sample"]],
  ["node:os:uptime", ["node.os-uptime", "relative-clock-read"]],
]);
const ASSIGNMENT_OPERATORS = new Set([
  ts.SyntaxKind.EqualsToken,
  ts.SyntaxKind.PlusEqualsToken,
  ts.SyntaxKind.MinusEqualsToken,
  ts.SyntaxKind.AsteriskEqualsToken,
  ts.SyntaxKind.AsteriskAsteriskEqualsToken,
  ts.SyntaxKind.SlashEqualsToken,
  ts.SyntaxKind.PercentEqualsToken,
  ts.SyntaxKind.LessThanLessThanEqualsToken,
  ts.SyntaxKind.GreaterThanGreaterThanEqualsToken,
  ts.SyntaxKind.GreaterThanGreaterThanGreaterThanEqualsToken,
  ts.SyntaxKind.AmpersandEqualsToken,
  ts.SyntaxKind.BarEqualsToken,
  ts.SyntaxKind.CaretEqualsToken,
  ts.SyntaxKind.BarBarEqualsToken,
  ts.SyntaxKind.AmpersandAmpersandEqualsToken,
  ts.SyntaxKind.QuestionQuestionEqualsToken,
]);

// hash one domain-separated value
function framedSha256(domain, value) {
  return createHash("sha256")
    .update(domain, "utf8")
    .update(Buffer.from([0]))
    .update(value, "utf8")
    .digest("hex");
}

// freeze one detached result tree
function freezeRecursively(value) {
  // stop at primitives and frozen values
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) {
    return value;
  }

  // freeze every retained child first
  for (const child of Object.values(value)) {
    freezeRecursively(child);
  }
  return Object.freeze(value);
}

// validate the closed input record
function validateInput(argumentsList) {
  // require exactly one argument
  if (argumentsList.length !== 1) {
    throw new TypeError("clock occurrence input must be one object");
  }
  const input = argumentsList[0];

  // require one plain record
  if (
    input === null ||
    typeof input !== "object" ||
    (Object.getPrototypeOf(input) !== Object.prototype &&
      Object.getPrototypeOf(input) !== null)
  ) {
    throw new TypeError("clock occurrence input must be plain");
  }
  const keys = Reflect.ownKeys(input);

  // require the exact two own keys
  if (
    keys.length !== 2 ||
    !keys.includes("sourcePath") ||
    !keys.includes("sourceText")
  ) {
    throw new TypeError("clock occurrence input keys are invalid");
  }
  const sourcePathDescriptor = Object.getOwnPropertyDescriptor(input, "sourcePath");
  const sourceTextDescriptor = Object.getOwnPropertyDescriptor(input, "sourceText");

  // reject accessors and nonstring values
  if (
    sourcePathDescriptor === undefined ||
    sourceTextDescriptor === undefined ||
    !("value" in sourcePathDescriptor) ||
    !("value" in sourceTextDescriptor) ||
    typeof sourcePathDescriptor.value !== "string" ||
    typeof sourceTextDescriptor.value !== "string"
  ) {
    throw new TypeError("clock occurrence input values are invalid");
  }
  const sourcePath = sourcePathDescriptor.value;
  const sourceText = sourceTextDescriptor.value;

  // reject unsafe path forms
  if (
    sourcePath.length === 0 ||
    sourcePath.startsWith("/") ||
    sourcePath.includes("\\") ||
    sourcePath.includes("\0")
  ) {
    throw new TypeError("clock occurrence source path is invalid");
  }
  const pathSegments = sourcePath.split("/");

  // reject empty and traversal segments
  for (const segment of pathSegments) {
    // reject one unsafe segment
    if (segment.length === 0 || segment === "." || segment === "..") {
      throw new TypeError("clock occurrence source path is invalid");
    }
  }
  const lastSegment = pathSegments.at(-1);
  const extensionIndex = lastSegment.lastIndexOf(".");
  const extension = extensionIndex < 0 ? "" : lastSegment.slice(extensionIndex);
  const sourceMode = LANGUAGE_BY_EXTENSION.get(extension);

  // require one closed source extension
  if (sourceMode === undefined) {
    throw new TypeError("clock occurrence source extension is unsupported");
  }

  // require the pinned parser version
  if (ts.version !== EXPECTED_TYPESCRIPT_VERSION) {
    throw new TypeError("clock occurrence parser version is unsupported");
  }
  return {
    sourcePath,
    sourceText,
    language: sourceMode.language,
    scriptKind: sourceMode.scriptKind,
  };
}

// collect utf16 line starts
function buildLineStarts(sourceText) {
  const starts = [0];

  // inspect every line terminator
  for (let index = 0; index < sourceText.length; index += 1) {
    const code = sourceText.charCodeAt(index);

    // consume one crlf pair
    if (code === 13 && sourceText.charCodeAt(index + 1) === 10) {
      index += 1;
      starts.push(index + 1);
    } else if (code === 13 || code === 10) {
      starts.push(index + 1);
    }
  }
  return starts;
}

// locate one utf16 coordinate
function coordinateAt(lineStarts, offset) {
  let low = 0;
  let high = lineStarts.length;

  // find the containing line
  while (low + 1 < high) {
    const middle = Math.floor((low + high) / 2);

    // narrow around the offset
    if (lineStarts[middle] <= offset) {
      low = middle;
    } else {
      high = middle;
    }
  }
  return { line: low + 1, column: offset - lineStarts[low] + 1 };
}

// build one half-open host span
function physicalSpan(lineStarts, startOffsetUtf16, endOffsetUtf16) {
  const start = coordinateAt(lineStarts, startOffsetUtf16);
  const end = coordinateAt(lineStarts, endOffsetUtf16);
  return {
    startOffsetUtf16,
    endOffsetUtf16,
    startLine: start.line,
    startColumn: start.column,
    endLine: end.line,
    endColumn: end.column,
  };
}

// add one fixed diagnostic draft
function addDiagnostic(state, code, severity, span = null) {
  state.diagnostics.push({
    code,
    severity,
    sourcePath: state.sourcePath,
    physicalSpan: span,
    relatedOccurrenceIds: [],
  });
}

// traverse every syntax sibling
function walkAst(node, visit) {
  visit(node);
  ts.forEachChild(
    node,
    // visit one child without short-circuiting
    (child) => {
      walkAst(child, visit);
      return undefined;
    },
  );
  return undefined;
}

// return one identifier member path
function expressionPath(node) {
  // retain one root identifier
  if (ts.isIdentifier(node)) {
    return [node.text];
  }

  // append one terminal property
  if (ts.isPropertyAccessExpression(node)) {
    const base = expressionPath(node.expression);

    // reject a nonidentifier base
    if (base === null) {
      return null;
    }
    return [...base, node.name.text];
  }
  return null;
}

// find the containing function scope
function functionScope(node, sourceFile) {
  let current = node.parent;

  // walk to one function or source boundary
  while (current !== undefined && current !== sourceFile) {
    // retain one function boundary
    if (ts.isFunctionLike(current)) {
      return current;
    }
    current = current.parent;
  }
  return sourceFile;
}

// find the containing lexical scope
function lexicalScope(node, sourceFile) {
  let current = node.parent;

  // walk to one block or source boundary
  while (current !== undefined && current !== sourceFile) {
    // retain one lexical block
    if (
      ts.isBlock(current) ||
      ts.isCaseBlock(current) ||
      ts.isModuleBlock(current) ||
      ts.isCatchClause(current)
    ) {
      return current;
    }
    current = current.parent;
  }
  return sourceFile;
}

// find one variable declaration scope
function variableDeclarationScope(node, sourceFile) {
  // bind catch parameters only inside the catch clause
  if (ts.isCatchClause(node.parent)) {
    return node.parent;
  }
  const declarationList = node.parent;
  const isBlockScoped =
    (ts.getCombinedNodeFlags(declarationList) & ts.NodeFlags.BlockScoped) !== 0;

  // bind var declarations across their function
  if (!isBlockScoped) {
    return functionScope(node, sourceFile);
  }
  const declarationOwner = declarationList.parent;

  // bind loop declarations only across the loop
  if (
    ts.isForStatement(declarationOwner) ||
    ts.isForInStatement(declarationOwner) ||
    ts.isForOfStatement(declarationOwner)
  ) {
    return declarationOwner;
  }
  return lexicalScope(node, sourceFile);
}

// collect identifiers from one binding name
function bindingIdentifiers(name, identifiers) {
  // retain one identifier binding
  if (ts.isIdentifier(name)) {
    identifiers.push(name.text);
    return;
  }

  // inspect every destructuring element
  for (const element of name.elements) {
    // skip omitted array elements
    if (ts.isOmittedExpression(element)) {
      continue;
    }
    bindingIdentifiers(element.name, identifiers);
  }
}

// register one scoped binding
function registerBinding(analysis, name, scope, kind) {
  analysis.bindings.push({
    name,
    kind,
    start: scope.getStart(analysis.sourceFile),
    end: scope.getEnd(),
  });
}

// register every declaration binding
function collectBindings(node, analysis) {
  const names = [];

  // bind one variable declaration
  if (ts.isVariableDeclaration(node)) {
    bindingIdentifiers(node.name, names);
    const isCatchBinding = ts.isCatchClause(node.parent);
    const isBlockScoped = isCatchBinding ||
      (ts.getCombinedNodeFlags(node.parent) & ts.NodeFlags.BlockScoped) !== 0;
    const scope = variableDeclarationScope(node, analysis.sourceFile);

    // register every variable binding
    for (const name of names) {
      registerBinding(
        analysis,
        name,
        scope,
        isCatchBinding ? "catch" : isBlockScoped ? "lexical" : "var",
      );
    }
    return;
  }

  // bind one parameter across its function
  if (ts.isParameter(node)) {
    bindingIdentifiers(node.name, names);
    const scope = node.parent;

    // register every parameter binding
    for (const name of names) {
      registerBinding(analysis, name, scope, "parameter");
    }
    return;
  }

  // bind one function declaration by hoisting
  if (ts.isFunctionDeclaration(node) && node.name !== undefined) {
    registerBinding(
      analysis,
      node.name.text,
      functionScope(node, analysis.sourceFile),
      "function",
    );
    return;
  }

  // bind one lexical class declaration
  if (ts.isClassDeclaration(node) && node.name !== undefined) {
    registerBinding(
      analysis,
      node.name.text,
      lexicalScope(node, analysis.sourceFile),
      "class",
    );
    return;
  }

  // bind one named function expression internally
  if (ts.isFunctionExpression(node) && node.name !== undefined) {
    registerBinding(analysis, node.name.text, node, "function-expression");
    return;
  }

  // bind one named class expression internally
  if (ts.isClassExpression(node) && node.name !== undefined) {
    registerBinding(analysis, node.name.text, node, "class-expression");
    return;
  }

  // bind one import clause
  if (ts.isImportClause(node)) {
    // retain one default import
    if (node.name !== undefined) {
      registerBinding(analysis, node.name.text, analysis.sourceFile, "import");
    }
    return;
  }

  // bind one named or namespace import
  if (ts.isImportSpecifier(node) || ts.isNamespaceImport(node)) {
    registerBinding(analysis, node.name.text, analysis.sourceFile, "import");
  }
}

// return one exact assignment member path
function assignmentMemberPath(node) {
  // retain one identifier root
  if (ts.isIdentifier(node)) {
    return [node.text];
  }

  // unwrap one transparent target
  if (
    ts.isParenthesizedExpression(node) ||
    ts.isAsExpression(node) ||
    ts.isTypeAssertionExpression(node) ||
    ts.isNonNullExpression(node) ||
    ts.isSatisfiesExpression(node)
  ) {
    return assignmentMemberPath(node.expression);
  }

  // retain one property segment
  if (ts.isPropertyAccessExpression(node)) {
    const base = assignmentMemberPath(node.expression);

    // reject one unresolved property base
    if (base === null) {
      return null;
    }
    return [...base, node.name.text];
  }

  // reject dynamic element access
  if (
    !ts.isElementAccessExpression(node) ||
    !ts.isStringLiteralLike(node.argumentExpression)
  ) {
    return null;
  }
  const base = assignmentMemberPath(node.expression);

  // reject one nonidentifier element base
  if (base === null) {
    return null;
  }
  return [...base, node.argumentExpression.text];
}

// encode one collision-free write path
function writePathKey(path) {
  return JSON.stringify(path);
}

// collect one assignment target key
function assignmentTargetKeys(node, keys) {
  // retain one identifier target
  if (ts.isIdentifier(node)) {
    const path = [node.text];
    keys.set(writePathKey(path), path);
    return;
  }
  const path = assignmentMemberPath(node);

  // retain one exact member target
  if (path !== null) {
    keys.set(writePathKey(path), path);
    return;
  }

  // unwrap one parenthesized target
  if (ts.isParenthesizedExpression(node)) {
    assignmentTargetKeys(node.expression, keys);
    return;
  }

  // unwrap one defaulted target
  if (
    ts.isBinaryExpression(node) &&
    node.operatorToken.kind === ts.SyntaxKind.EqualsToken
  ) {
    assignmentTargetKeys(node.left, keys);
    return;
  }

  // inspect one object assignment pattern
  if (ts.isObjectLiteralExpression(node)) {
    // collect every assignment property
    for (const property of node.properties) {
      // unwrap one object rest target
      if (ts.isSpreadAssignment(property)) {
        assignmentTargetKeys(property.expression, keys);
      } else if (ts.isPropertyAssignment(property)) {
        assignmentTargetKeys(property.initializer, keys);
      } else if (ts.isShorthandPropertyAssignment(property)) {
        const path = [property.name.text];
        keys.set(writePathKey(path), path);
      }
    }
    return;
  }

  // inspect one array assignment pattern
  if (ts.isArrayLiteralExpression(node)) {
    // collect every assignment element
    for (const element of node.elements) {
      // unwrap one array rest target
      if (ts.isSpreadElement(element)) {
        assignmentTargetKeys(element.expression, keys);
      } else if (!ts.isOmittedExpression(element)) {
        assignmentTargetKeys(element, keys);
      }
    }
  }
}

// collect writes that downgrade clock confidence
function collectWrites(node, analysis) {
  const keys = new Map();

  // retain one assignment expression
  if (ts.isBinaryExpression(node) && ASSIGNMENT_OPERATORS.has(node.operatorToken.kind)) {
    assignmentTargetKeys(node.left, keys);
  } else if (
    (ts.isPrefixUnaryExpression(node) || ts.isPostfixUnaryExpression(node)) &&
    (node.operator === ts.SyntaxKind.PlusPlusToken ||
      node.operator === ts.SyntaxKind.MinusMinusToken)
  ) {
    // retain one update expression
    assignmentTargetKeys(node.operand, keys);
  } else if (
    (ts.isForInStatement(node) || ts.isForOfStatement(node)) &&
    !ts.isVariableDeclarationList(node.initializer)
  ) {
    // retain one loop assignment target
    assignmentTargetKeys(node.initializer, keys);
  }

  // stop when this node has no writes
  if (keys.size === 0) {
    return;
  }
  const position = node.getStart(analysis.sourceFile);
  const functionBoundary = functionScope(node, analysis.sourceFile);
  const functionStart = functionBoundary.getStart(analysis.sourceFile);
  const functionEnd = functionBoundary.getEnd();

  // register every scoped write
  for (const [key, path] of keys) {
    const [root] = path;
    const binding = activeBinding(analysis, root, position);
    const useBindingScope = binding !== null &&
      binding.end - binding.start < functionEnd - functionStart;
    analysis.writes.push({
      key,
      start: useBindingScope ? binding.start : functionStart,
      end: useBindingScope ? binding.end : functionEnd,
    });
  }
}

// map one direct reference to a clock surface
function directReference(node) {
  // admit one timer identifier
  if (ts.isIdentifier(node) && TIMER_SURFACES.has(node.text)) {
    return [TIMER_SURFACES.get(node.text), "timer-schedule", node.text];
  }
  const path = expressionPath(node);

  // reject one nonproperty reference
  if (path === null) {
    return null;
  }
  const key = path.join(".");
  const declared = CALL_SURFACES.get(key);

  // retain one direct callable member
  if (declared !== undefined) {
    return [declared[0], declared[1], path[0]];
  }
  return null;
}

// add one declaration-level gap
function addNodeDiagnostic(analysis, node, code) {
  addDiagnostic(
    analysis,
    code,
    "gap",
    physicalSpan(
      analysis.lineStarts,
      node.getStart(analysis.sourceFile),
      node.getEnd(),
    ),
  );
}

// register one scoped alias
function registerAlias(analysis, name, node, alias) {
  const scope = ts.isImportSpecifier(node)
    ? analysis.sourceFile
    : ts.isVariableDeclaration(node)
      ? variableDeclarationScope(node, analysis.sourceFile)
      : lexicalScope(node, analysis.sourceFile);
  analysis.aliases.push({
    name,
    start: scope.getStart(analysis.sourceFile),
    end: scope.getEnd(),
    ...alias,
  });
}

// resolve the innermost active alias
function resolveAlias(analysis, name, position) {
  let selected = null;

  // inspect every matching alias scope
  for (const alias of analysis.aliases) {
    // retain one narrower active alias
    if (
      alias.name === name &&
      alias.start <= position &&
      position < alias.end &&
      (selected === null || alias.end - alias.start < selected.end - selected.start)
    ) {
      selected = alias;
    }
  }
  const binding = activeBinding(analysis, name, position);

  // reject an alias hidden by a narrower binding
  if (
    selected !== null &&
    binding !== null &&
    binding.end - binding.start < selected.end - selected.start
  ) {
    return null;
  }
  return selected;
}

// test scoped writes along one path
function hasWriteAtOrAbove(analysis, path, position) {
  // inspect every root-to-target prefix
  for (let length = 1; length <= path.length; length += 1) {
    const key = writePathKey(path.slice(0, length));

    // inspect every applicable write scope
    for (const write of analysis.writes) {
      // retain one active prefix write
      if (write.key === key && write.start <= position && position < write.end) {
        return true;
      }
    }
  }
  return false;
}

// collect one-hop aliases and rejected alias shapes
function collectAlias(node, analysis) {
  // inspect one named import alias
  if (ts.isImportSpecifier(node)) {
    const declaration = node.parent.parent.parent;
    const moduleName = declaration.moduleSpecifier;

    // require one literal module name
    if (!ts.isStringLiteral(moduleName)) {
      return;
    }
    const imported = (node.propertyName ?? node.name).text;
    const local = node.name.text;

    // admit the performance root import
    if (moduleName.text === "node:perf_hooks" && imported === "performance") {
      registerAlias(analysis, local, node, {
        mode: "root",
        root: "performance",
        step: "named-import-alias",
      });
      return;
    }
    const declared = DIRECT_IMPORT_SURFACES.get(`${moduleName.text}:${imported}`);

    // admit one named callable import
    if (declared !== undefined) {
      registerAlias(analysis, local, node, {
        mode: "call",
        surface: declared[0],
        kind: declared[1],
        step: "named-import-alias",
      });
    }
    return;
  }

  // inspect one immutable variable alias
  if (!ts.isVariableDeclaration(node)) {
    return;
  }
  const declarationList = node.parent;
  const isConst = (ts.getCombinedNodeFlags(declarationList) & ts.NodeFlags.Const) !== 0;

  // reject mutable or missing initializers
  if (!isConst || node.initializer === undefined) {
    return;
  }

  // inspect one identifier alias
  if (ts.isIdentifier(node.name)) {
    const local = node.name.text;
    const target = directReference(node.initializer);

    // admit one exact direct target
    if (target !== null) {
      const position = node.getStart(analysis.sourceFile);
      const targetPath = expressionPath(node.initializer) ?? [target[2]];
      const shadowed = activeBinding(analysis, target[2], position) !== null;
      const reassigned = hasWriteAtOrAbove(analysis, targetPath, position);

      // reject one shadowed alias target
      if (shadowed) {
        addNodeDiagnostic(analysis, node, "javascript.intrinsic-shadowed");
      }

      // reject one reassigned alias target
      if (reassigned) {
        addNodeDiagnostic(analysis, node, "javascript.clock-binding-reassigned");
      }

      // retain only one stable direct target
      if (shadowed || reassigned) {
        return;
      }
      registerAlias(analysis, local, node, {
        mode: "call",
        surface: target[0],
        kind: target[1],
        step: "const-identifier-alias",
      });
      return;
    }

    // reject one alias chain
    if (
      ts.isIdentifier(node.initializer) &&
      resolveAlias(
        analysis,
        node.initializer.text,
        node.getStart(analysis.sourceFile),
      ) !== null
    ) {
      addNodeDiagnostic(analysis, node, "javascript.alias-second-hop-unresolved");
      return;
    }

    // reject one arbitrary property alias
    if (
      ts.isPropertyAccessExpression(node.initializer) &&
      WRAPPER_SURFACES.has(node.initializer.name.text)
    ) {
      addNodeDiagnostic(analysis, node, "javascript.alias-property-unresolved");
      return;
    }

    // reject one computed clock member alias
    if (
      ts.isElementAccessExpression(node.initializer) &&
      ts.isStringLiteralLike(node.initializer.argumentExpression) &&
      WRAPPER_SURFACES.has(node.initializer.argumentExpression.text)
    ) {
      addNodeDiagnostic(analysis, node, "javascript.computed-clock-member-unresolved");
    }
    return;
  }

  // inspect one exact destructuring alias
  if (!ts.isObjectBindingPattern(node.name) || !ts.isIdentifier(node.initializer)) {
    return;
  }
  const destructuringPosition = node.getStart(analysis.sourceFile);

  // reject one shadowed destructuring root
  if (activeBinding(analysis, node.initializer.text, destructuringPosition) !== null) {
    addNodeDiagnostic(analysis, node, "javascript.intrinsic-shadowed");
    return;
  }

  // reject one reassigned destructuring root
  if (hasWriteAtOrAbove(analysis, [node.initializer.text], destructuringPosition)) {
    addNodeDiagnostic(analysis, node, "javascript.clock-binding-reassigned");
    return;
  }
  let validPattern = node.name.elements.length > 0;
  const pending = [];

  // validate every destructuring element
  for (const element of node.name.elements) {
    // reject defaults rests and nonidentifiers
    if (
      element.dotDotDotToken !== undefined ||
      element.initializer !== undefined ||
      !ts.isIdentifier(element.name)
    ) {
      validPattern = false;
      break;
    }
    const property = element.propertyName ?? element.name;

    // reject computed or nonidentifier properties
    if (!ts.isIdentifier(property)) {
      validPattern = false;
      break;
    }
    const target = directReference(
      ts.factory.createPropertyAccessExpression(
        ts.factory.createIdentifier(node.initializer.text),
        property.text,
      ),
    );

    // reject unsupported target properties
    if (target === null) {
      validPattern = false;
      break;
    }
    pending.push({
      local: element.name.text,
      target,
      targetPath: [node.initializer.text, property.text],
    });
  }

  // retain only one exact pattern
  if (validPattern) {
    // register every destructured alias
    for (const alias of pending) {
      // reject one reassigned selected member
      if (hasWriteAtOrAbove(analysis, alias.targetPath, destructuringPosition)) {
        addNodeDiagnostic(
          analysis,
          node,
          "javascript.clock-binding-reassigned",
        );
        continue;
      }
      registerAlias(analysis, alias.local, node, {
        mode: "call",
        surface: alias.target[0],
        kind: alias.target[1],
        step: "const-destructuring-alias",
      });
    }
  }
}

// build lexical bindings writes and aliases
function analyzeJavascript(sourceFile, state) {
  const analysis = {
    sourceFile,
    sourcePath: state.sourcePath,
    lineStarts: state.lineStarts,
    diagnostics: state.diagnostics,
    bindings: [],
    writes: [],
    aliases: [],
  };
  walkAst(
    sourceFile,
    // collect the complete scoped binding table first
    (node) => {
      collectBindings(node, analysis);
      return undefined;
    },
  );
  walkAst(
    sourceFile,
    // collect writes against complete bindings second
    (node) => {
      collectWrites(node, analysis);
      return undefined;
    },
  );
  walkAst(
    sourceFile,
    // collect aliases against the complete scope table
    (node) => {
      collectAlias(node, analysis);
      return undefined;
    },
  );
  return analysis;
}

// test whether one root is lexically bound
function activeBinding(analysis, name, position) {
  let selected = null;

  // inspect every matching scope
  for (const binding of analysis.bindings) {
    // retain one narrower active binding
    if (
      binding.name === name &&
      binding.start <= position &&
      position < binding.end &&
      (selected === null || binding.end - binding.start < selected.end - selected.start)
    ) {
      selected = binding;
    }
  }
  return selected;
}

// render one privacy-safe owner name
function memberOwnerName(name) {
  // redact one private owner
  if (ts.isPrivateIdentifier(name)) {
    return "<private>";
  }

  // retain one public identifier owner
  if (ts.isIdentifier(name)) {
    return name.text;
  }

  // redact one computed owner
  if (ts.isComputedPropertyName(name)) {
    return "<computed>";
  }
  return "<literal-member>";
}

// find one nearest stable lexical owner
function enclosingSymbol(node) {
  let current = node.parent;

  // walk to one named owner
  while (current !== undefined) {
    // retain one class member owner
    if (
      ts.isMethodDeclaration(current) ||
      ts.isGetAccessorDeclaration(current) ||
      ts.isSetAccessorDeclaration(current) ||
      ts.isPropertyDeclaration(current)
    ) {
      return memberOwnerName(current.name);
    }

    // retain one function declaration owner
    if (ts.isFunctionDeclaration(current)) {
      return current.name?.text ?? "<anonymous>";
    }

    // retain one function expression owner
    if (ts.isFunctionExpression(current) || ts.isArrowFunction(current)) {
      // retain one explicit function name
      if (ts.isFunctionExpression(current) && current.name !== undefined) {
        return current.name.text;
      }
      const parent = current.parent;

      // retain one initialized variable name
      if (ts.isVariableDeclaration(parent) && ts.isIdentifier(parent.name)) {
        return parent.name.text;
      }
      return "<anonymous>";
    }
    current = current.parent;
  }
  return "<module>";
}

// redact one expression through the pinned printer
function normalizedJavascriptExpression(node, sourceFile) {
  const factory = ts.factory;

  // create one literal-redaction transformer
  function transformer(context) {
    // redact one syntax node
    function visit(current) {
      // redact one string literal
      if (ts.isStringLiteral(current)) {
        return factory.createStringLiteral("<string>");
      }

      // redact one numeric literal
      if (ts.isNumericLiteral(current)) {
        return factory.createNumericLiteral("0");
      }

      // redact one bigint literal
      if (ts.isBigIntLiteral(current)) {
        return factory.createBigIntLiteral("0n");
      }

      // redact one regular expression
      if (ts.isRegularExpressionLiteral(current)) {
        return factory.createRegularExpressionLiteral("/(?:)/");
      }

      // redact one private identifier
      if (ts.isPrivateIdentifier(current)) {
        return factory.createPrivateIdentifier("#private");
      }

      // redact one element access argument
      if (ts.isElementAccessExpression(current)) {
        return factory.updateElementAccessExpression(
          current,
          ts.visitNode(current.expression, visit),
          factory.createStringLiteral("<element>"),
        );
      }

      // redact one computed key
      if (ts.isComputedPropertyName(current)) {
        return factory.createComputedPropertyName(factory.createStringLiteral("<computed>"));
      }

      // redact one object property key
      if (ts.isPropertyAssignment(current)) {
        return factory.updatePropertyAssignment(
          current,
          factory.createIdentifier("property"),
          ts.visitNode(current.initializer, visit),
        );
      }

      // redact one shorthand property key
      if (ts.isShorthandPropertyAssignment(current)) {
        return factory.createPropertyAssignment(
          factory.createIdentifier("property"),
          ts.visitNode(current.name, visit),
        );
      }

      // redact one object method key
      if (
        ts.isMethodDeclaration(current) &&
        ts.isObjectLiteralExpression(current.parent)
      ) {
        const visited = ts.visitEachChild(current, visit, context);
        return factory.updateMethodDeclaration(
          visited,
          visited.modifiers,
          visited.asteriskToken,
          factory.createIdentifier("property"),
          visited.questionToken,
          visited.typeParameters,
          visited.parameters,
          visited.type,
          visited.body,
        );
      }

      // redact one object getter key
      if (
        ts.isGetAccessorDeclaration(current) &&
        ts.isObjectLiteralExpression(current.parent)
      ) {
        const visited = ts.visitEachChild(current, visit, context);
        return factory.updateGetAccessorDeclaration(
          visited,
          visited.modifiers,
          factory.createIdentifier("property"),
          visited.parameters,
          visited.type,
          visited.body,
        );
      }

      // redact one object setter key
      if (
        ts.isSetAccessorDeclaration(current) &&
        ts.isObjectLiteralExpression(current.parent)
      ) {
        const visited = ts.visitEachChild(current, visit, context);
        return factory.updateSetAccessorDeclaration(
          visited,
          visited.modifiers,
          factory.createIdentifier("property"),
          visited.parameters,
          visited.body,
        );
      }

      // redact one static template
      if (ts.isNoSubstitutionTemplateLiteral(current)) {
        return factory.createNoSubstitutionTemplateLiteral("<template>");
      }

      // redact template text while retaining expressions
      if (ts.isTemplateExpression(current)) {
        const spans = [];

        // transform every retained interpolation
        for (const span of current.templateSpans) {
          const literal = ts.isTemplateTail(span.literal)
            ? factory.createTemplateTail("<template>")
            : factory.createTemplateMiddle("<template>");
          spans.push(
            factory.createTemplateSpan(
              ts.visitNode(span.expression, visit),
              literal,
            ),
          );
        }
        return factory.createTemplateExpression(
          factory.createTemplateHead("<template>"),
          spans,
        );
      }
      return ts.visitEachChild(current, visit, context);
    }

    // transform one expression root
    return function transformRoot(root) {
      return ts.visitNode(root, visit);
    };
  }
  const transformed = ts.transform(node, [transformer]);

  try {
    const printer = ts.createPrinter({
      removeComments: true,
      newLine: ts.NewLineKind.LineFeed,
    });
    return printer.printNode(
      ts.EmitHint.Expression,
      transformed.transformed[0],
      sourceFile,
    );
  } finally {
    transformed.dispose();
  }
}

// create one javascript occurrence draft
function addJavascriptOccurrence(state, sourceFile, node, descriptor) {
  const start = node.getStart(sourceFile);
  const end = node.getEnd();
  const rendered = normalizedJavascriptExpression(node, sourceFile);
  state.occurrenceDrafts.push({
    sourcePath: state.sourcePath,
    language: state.language,
    surface: descriptor.surface,
    kind: descriptor.kind,
    syntaxKind: descriptor.syntaxKind,
    enclosingSymbol: enclosingSymbol(node),
    confidence: descriptor.confidence,
    aliasTrace: descriptor.aliasTrace,
    normalizedExpressionSha256: framedSha256(
      "weather-clock-expression/v1",
      rendered,
    ),
    physicalSpan: physicalSpan(state.lineStarts, start, end),
    embeddedSql: null,
    countsAsClockCandidate: descriptor.kind !== "parse-only",
  });
}

// build one direct trace entry
function directTrace(step, binding, targetSurface) {
  return [{ step, binding, targetSurface }];
}

// downgrade one intrinsic when scope evidence requires it
function applyIntrinsicConfidence(analysis, node, descriptor, root, path) {
  const position = node.getStart(analysis.sourceFile);
  const binding = activeBinding(analysis, root, position);
  let confidence = descriptor.confidence;

  // downgrade one shadowed root
  if (binding !== null) {
    confidence = "name-only-candidate";
    addNodeDiagnostic(analysis, node, "javascript.intrinsic-shadowed");
  }

  // downgrade one reassigned root or member
  if (hasWriteAtOrAbove(analysis, path, position)) {
    confidence = "name-only-candidate";
    addNodeDiagnostic(analysis, node, "javascript.clock-binding-reassigned");
  }
  return { ...descriptor, confidence };
}

// downgrade one alias when its binding or member is written
function applyAliasConfidence(analysis, node, descriptor, path) {
  const position = node.getStart(analysis.sourceFile);

  // retain one stable one-hop alias
  if (!hasWriteAtOrAbove(analysis, path, position)) {
    return descriptor;
  }
  addNodeDiagnostic(analysis, node, "javascript.clock-binding-reassigned");
  return { ...descriptor, confidence: "name-only-candidate" };
}

// describe one admitted call expression
function describeCall(analysis, node) {
  const expression = node.expression;

  // detect one direct identifier call
  if (ts.isIdentifier(expression)) {
    const position = node.getStart(analysis.sourceFile);
    const alias = resolveAlias(analysis, expression.text, position);

    // retain one direct callable alias
    if (alias?.mode === "call") {
      return applyAliasConfidence(
        analysis,
        node,
        {
          surface: alias.surface,
          kind: alias.kind,
          syntaxKind: "aliased-call",
          confidence: "one-hop-candidate",
          aliasTrace: directTrace(alias.step, expression.text, alias.surface),
        },
        [expression.text],
      );
    }

    // retain ordinary Date calls
    if (expression.text === "Date") {
      return applyIntrinsicConfidence(
        analysis,
        node,
        {
          surface: "ecmascript.date-call",
          kind: "current-time-read",
          syntaxKind: "identifier-call",
          confidence: "syntactic-direct",
          aliasTrace: directTrace("direct-global", "Date", "ecmascript.date-call"),
        },
        "Date",
        ["Date"],
      );
    }
    const timerSurface = TIMER_SURFACES.get(expression.text);

    // retain one global timer
    if (timerSurface !== undefined) {
      return applyIntrinsicConfidence(
        analysis,
        node,
        {
          surface: timerSurface,
          kind: "timer-schedule",
          syntaxKind: "identifier-call",
          confidence: "syntactic-direct",
          aliasTrace: directTrace("direct-global", expression.text, timerSurface),
        },
        expression.text,
        [expression.text],
      );
    }
    const wrapperSurface = WRAPPER_SURFACES.get(expression.text);

    // retain one unresolved wrapper name
    if (wrapperSurface !== undefined) {
      const binding = activeBinding(
        analysis,
        expression.text,
        node.getStart(analysis.sourceFile),
      );
      addNodeDiagnostic(analysis, node, "javascript.wrapper-name-unresolved");
      return {
        surface: wrapperSurface,
        kind: "wrapper-candidate",
        syntaxKind: "identifier-call",
        confidence: "name-only-candidate",
        aliasTrace: directTrace(
          binding?.kind === "parameter" ? "parameter-name" : "direct-global",
          expression.text,
          wrapperSurface,
        ),
      };
    }

    // retain dynamic javascript execution gaps
    if (expression.text === "eval" || expression.text === "Function") {
      addNodeDiagnostic(analysis, node, "javascript.dynamic-code-unresolved");
    }
    return null;
  }
  const pathParts = expressionPath(expression);

  // detect one direct property call
  if (pathParts !== null) {
    const path = pathParts.join(".");
    const rootAlias = resolveAlias(
      analysis,
      pathParts[0],
      node.getStart(analysis.sourceFile),
    );

    // retain one imported performance member
    if (rootAlias?.mode === "root" && rootAlias.root === "performance") {
      const memberPath = [rootAlias.root, ...pathParts.slice(1)].join(".");
      const declared = CALL_SURFACES.get(memberPath);

      // retain one supported performance call
      if (declared !== undefined) {
        return applyAliasConfidence(
          analysis,
          node,
          {
            surface: declared[0],
            kind: declared[1],
            syntaxKind: "aliased-call",
            confidence: "one-hop-candidate",
            aliasTrace: directTrace(rootAlias.step, pathParts[0], declared[0]),
          },
          pathParts,
        );
      }
    }
    const declared = CALL_SURFACES.get(path);

    // retain one direct member surface
    if (declared !== undefined) {
      return applyIntrinsicConfidence(
        analysis,
        node,
        {
          surface: declared[0],
          kind: declared[1],
          syntaxKind: "property-call",
          confidence: "syntactic-direct",
          aliasTrace: directTrace(
            "terminal-property",
            pathParts.at(-1),
            declared[0],
          ),
        },
        pathParts[0],
        pathParts,
      );
    }
    const terminal = pathParts.at(-1);
    const wrapperSurface = WRAPPER_SURFACES.get(terminal);

    // retain one unresolved terminal wrapper
    if (wrapperSurface !== undefined) {
      addNodeDiagnostic(analysis, node, "javascript.wrapper-name-unresolved");
      return {
        surface: wrapperSurface,
        kind: "wrapper-candidate",
        syntaxKind: "property-call",
        confidence: "name-only-candidate",
        aliasTrace: directTrace("terminal-property", terminal, wrapperSurface),
      };
    }

    // retain dynamic vm execution gaps
    if (pathParts[0] === "vm" && /^run|^compile/u.test(terminal)) {
      addNodeDiagnostic(analysis, node, "javascript.dynamic-code-unresolved");
    }
    return null;
  }

  // retain one wrapper on a nonidentifier property base
  if (
    ts.isPropertyAccessExpression(expression) &&
    WRAPPER_SURFACES.has(expression.name.text)
  ) {
    const wrapperSurface = WRAPPER_SURFACES.get(expression.name.text);
    addNodeDiagnostic(analysis, node, "javascript.wrapper-name-unresolved");
    return {
      surface: wrapperSurface,
      kind: "wrapper-candidate",
      syntaxKind: "property-call",
      confidence: "name-only-candidate",
      aliasTrace: directTrace(
        "terminal-property",
        expression.name.text,
        wrapperSurface,
      ),
    };
  }

  // retain one computed clock-member gap
  if (
    ts.isElementAccessExpression(expression) &&
    ts.isStringLiteralLike(expression.argumentExpression)
  ) {
    const member = expression.argumentExpression.text;
    const base = expressionPath(expression.expression);
    const direct = base === null
      ? undefined
      : CALL_SURFACES.get([...base, member].join("."));
    const wrapperSurface = WRAPPER_SURFACES.get(member);

    // retain one computed direct or wrapper candidate
    if (direct !== undefined || wrapperSurface !== undefined) {
      const surface = direct?.[0] ?? wrapperSurface;
      addNodeDiagnostic(analysis, node, "javascript.computed-clock-member-unresolved");
      return {
        surface,
        kind: direct?.[1] ?? "wrapper-candidate",
        syntaxKind: "property-call",
        confidence: "name-only-candidate",
        aliasTrace: directTrace("terminal-property", "<computed>", surface),
      };
    }
  }
  return null;
}

// describe one new expression
function describeNewExpression(analysis, node) {
  // require the direct Date constructor
  if (!ts.isIdentifier(node.expression) || node.expression.text !== "Date") {
    return null;
  }
  const hasArguments = (node.arguments?.length ?? 0) > 0;
  const surface = hasArguments
    ? "ecmascript.date-construct-supplied"
    : "ecmascript.date-construct-current";
  return applyIntrinsicConfidence(
    analysis,
    node,
    {
      surface,
      kind: hasArguments ? "parse-only" : "current-time-read",
      syntaxKind: "new-expression",
      confidence: "syntactic-direct",
      aliasTrace: directTrace("direct-global", "Date", surface),
    },
    "Date",
    ["Date"],
  );
}

// describe one admitted property read
function describePropertyRead(analysis, node) {
  const pathParts = expressionPath(node);

  // reject one nonproperty expression
  if (pathParts === null) {
    return null;
  }
  const rootAlias = resolveAlias(
    analysis,
    pathParts[0],
    node.getStart(analysis.sourceFile),
  );

  // retain one imported performance property
  if (rootAlias?.mode === "root" && rootAlias.root === "performance") {
    const declared = PROPERTY_SURFACES.get(
      [rootAlias.root, ...pathParts.slice(1)].join("."),
    );

    // retain one supported alias property
    if (declared !== undefined) {
      return applyAliasConfidence(
        analysis,
        node,
        {
          surface: declared[0],
          kind: declared[1],
          syntaxKind: "property-read",
          confidence: "one-hop-candidate",
          aliasTrace: directTrace(rootAlias.step, pathParts[0], declared[0]),
        },
        pathParts,
      );
    }
  }
  const path = pathParts.join(".");
  const declared = PROPERTY_SURFACES.get(path);

  // reject one unrelated property
  if (declared === undefined) {
    return null;
  }
  return applyIntrinsicConfidence(
    analysis,
    node,
    {
      surface: declared[0],
      kind: declared[1],
      syntaxKind: "property-read",
      confidence: "syntactic-direct",
      aliasTrace: directTrace("terminal-property", pathParts.at(-1), declared[0]),
    },
    pathParts[0],
    pathParts,
  );
}

// derive one template segment boundary
function templateSegmentBoundary(node, sourceFile) {
  const start = node.getStart(sourceFile);
  const end = node.getEnd();

  // retain one template head
  if (ts.isTemplateHead(node)) {
    return { start, end, payloadStart: start + 1, payloadEnd: end - 2, segmentKind: "head" };
  }

  // retain one template middle
  if (ts.isTemplateMiddle(node)) {
    return { start, end, payloadStart: start + 1, payloadEnd: end - 2, segmentKind: "middle" };
  }
  return { start, end, payloadStart: start + 1, payloadEnd: end - 1, segmentKind: "tail" };
}

// derive one literal container contract
function literalContainer(node, state, sourceFile, interpolationPresent) {
  let containerKind;
  let segmentKind = null;
  let start;
  let end;
  let payloadStart;
  let payloadEnd;

  // describe one string literal
  if (ts.isStringLiteral(node)) {
    containerKind = "string-literal";
    start = node.getStart(sourceFile);
    end = node.getEnd();
    payloadStart = start + 1;
    payloadEnd = end - 1;
  } else if (ts.isNoSubstitutionTemplateLiteral(node)) {
    containerKind = "no-substitution-template";
    start = node.getStart(sourceFile);
    end = node.getEnd();
    payloadStart = start + 1;
    payloadEnd = end - 1;
  } else {
    containerKind = "template-segment";
    const boundary = templateSegmentBoundary(node, sourceFile);
    ({ start, end, payloadStart, payloadEnd, segmentKind } = boundary);
  }
  const decodedText = node.text;
  const rawPayload = state.sourceText.slice(payloadStart, payloadEnd);
  return {
    decodedText,
    exactMapping: rawPayload === decodedText,
    containerKind,
    segmentKind,
    containerPhysicalSpan: physicalSpan(state.lineStarts, start, end),
    payloadPhysicalSpan: physicalSpan(state.lineStarts, payloadStart, payloadEnd),
    payloadStart,
    interpolationPresent,
  };
}

// map one decoded sql range to its host contract
function embeddedSqlContract(container, decodedStart, decodedEnd, state) {
  return {
    embeddedSql: {
      containerKind: container.containerKind,
      segmentKind: container.segmentKind,
      containerPhysicalSpan: container.containerPhysicalSpan,
      payloadPhysicalSpan: container.payloadPhysicalSpan,
      decodedRange: {
        startOffsetUtf16: decodedStart,
        endOffsetUtf16: decodedEnd,
      },
      decodedDomain: container.containerKind === "template-segment"
        ? "decoded-template-segment"
        : "decoded-literal-text",
      hostMapping: container.exactMapping
        ? "exact-provider-span"
        : "container-only-escaped",
      interpolationPresent: container.interpolationPresent,
    },
    physicalSpan: container.exactMapping
      ? physicalSpan(
        state.lineStarts,
        container.payloadStart + decodedStart,
        container.payloadStart + decodedEnd,
      )
      : container.containerPhysicalSpan,
  };
}

// tokenize one bounded postgresql text
function tokenizeSql(text) {
  const tokens = [];
  const dollarBodies = [];
  let index = 0;

  // consume the complete sql text
  while (index < text.length) {
    const character = text[index];

    // skip whitespace
    if (/\s/u.test(character)) {
      index += 1;
      continue;
    }

    // skip one line comment
    if (text.startsWith("--", index)) {
      const newline = text.indexOf("\n", index + 2);
      index = newline < 0 ? text.length : newline + 1;
      continue;
    }

    // skip one nested block comment
    if (text.startsWith("/*", index)) {
      const start = index;
      let depth = 1;
      index += 2;

      // consume nested comment levels
      while (index < text.length && depth > 0) {
        // enter one nested comment
        if (text.startsWith("/*", index)) {
          depth += 1;
          index += 2;
        } else if (text.startsWith("*/", index)) {
          depth -= 1;
          index += 2;
        } else {
          index += 1;
        }
      }

      // fail one unterminated block comment
      if (depth > 0) {
        return { tokens, dollarBodies, error: { start, end: text.length } };
      }
      continue;
    }

    // consume one single-quoted value
    if (character === "'") {
      const start = index;
      const prefix = tokens.at(-1);
      const escapeString =
        prefix?.type === "identifier" &&
        !prefix.quoted &&
        prefix.end === start &&
        prefix.value.toLowerCase() === "e";
      let value = "";
      index += 1;
      let closed = false;
      let escapeObserved = false;

      // consume string contents
      while (index < text.length) {
        // consume one escape-string pair
        if (escapeString && text[index] === "\\" && index + 1 < text.length) {
          escapeObserved = true;
          value += text[index + 1];
          index += 2;
        } else if (text[index] === "'" && text[index + 1] === "'") {
          // decode one doubled quote
          value += "'";
          index += 2;
        } else if (text[index] === "'") {
          index += 1;
          closed = true;
          break;
        } else {
          value += text[index];
          index += 1;
        }
      }

      // fail one unterminated string
      if (!closed) {
        return { tokens, dollarBodies, error: { start, end: text.length } };
      }
      tokens.push({
        type: "string",
        value,
        start,
        end: index,
        quoted: true,
        escapeString,
        escapeObserved,
      });
      continue;
    }

    // consume one quoted identifier
    if (character === '"') {
      const start = index;
      let value = "";
      index += 1;
      let closed = false;

      // consume identifier contents
      while (index < text.length) {
        // decode one doubled quote
        if (text[index] === '"' && text[index + 1] === '"') {
          value += '"';
          index += 2;
        } else if (text[index] === '"') {
          index += 1;
          closed = true;
          break;
        } else {
          value += text[index];
          index += 1;
        }
      }

      // fail one unterminated identifier
      if (!closed) {
        return { tokens, dollarBodies, error: { start, end: text.length } };
      }
      tokens.push({ type: "identifier", value, start, end: index, quoted: true });
      continue;
    }

    // consume one dollar-quoted body
    if (character === "$") {
      const delimiterMatch = /^(?:\$\$|\$[A-Za-z_][A-Za-z0-9_]*\$)/u.exec(text.slice(index));

      // retain one valid dollar delimiter
      if (delimiterMatch !== null) {
        const start = index;
        const delimiter = delimiterMatch[0];
        const closing = text.indexOf(delimiter, index + delimiter.length);

        // fail one unterminated dollar body
        if (closing < 0) {
          return { tokens, dollarBodies, error: { start, end: text.length } };
        }
        index = closing + delimiter.length;
        dollarBodies.push({ start, end: index });
        continue;
      }
    }

    // consume one unquoted identifier
    if (/[A-Za-z_]/u.test(character)) {
      const start = index;
      index += 1;

      // consume identifier continuation
      while (index < text.length && /[A-Za-z0-9_$]/u.test(text[index])) {
        index += 1;
      }
      tokens.push({
        type: "identifier",
        value: text.slice(start, index),
        start,
        end: index,
        quoted: false,
      });
      continue;
    }

    // consume one number
    if (/[0-9]/u.test(character)) {
      const start = index;
      index += 1;

      // consume numeric continuation
      while (index < text.length && /[0-9]/u.test(text[index])) {
        index += 1;
      }
      tokens.push({ type: "number", value: text.slice(start, index), start, end: index });
      continue;
    }

    // consume one cast operator
    if (text.startsWith("::", index)) {
      tokens.push({ type: "symbol", value: "::", start: index, end: index + 2 });
      index += 2;
      continue;
    }
    tokens.push({ type: "symbol", value: character, start: index, end: index + 1 });
    index += 1;
  }
  return { tokens, dollarBodies, error: null };
}

// normalize one sql identifier token
function sqlIdentifier(token) {
  // reject one nonidentifier token
  if (token?.type !== "identifier") {
    return null;
  }

  // preserve quoted case
  if (token.quoted) {
    return token.value === token.value.toLowerCase() ? token.value : null;
  }
  return token.value.toLowerCase();
}

// add one sql occurrence draft
function addSqlOccurrence(
  state,
  context,
  surface,
  kind,
  syntaxKind,
  confidence,
  normalization,
  start,
  end,
) {
  const mapped = context === null
    ? {
      physicalSpan: physicalSpan(state.lineStarts, start, end),
      embeddedSql: null,
    }
    : embeddedSqlContract(context, start, end, state);
  const normalized = JSON.stringify({
    surface,
    kind,
    syntaxKind,
    provider: normalization,
  });
  state.occurrenceDrafts.push({
    sourcePath: state.sourcePath,
    language: state.language,
    surface,
    kind,
    syntaxKind,
    enclosingSymbol: context?.enclosingSymbol ?? "<module>",
    confidence,
    aliasTrace: [],
    normalizedExpressionSha256: framedSha256(
      "weather-clock-expression/v1",
      normalized,
    ),
    physicalSpan: mapped.physicalSpan,
    embeddedSql: mapped.embeddedSql,
    countsAsClockCandidate: kind !== "parse-only",
  });
}

// map one sql diagnostic span
function sqlDiagnosticSpan(state, context, start, end) {
  // retain one raw sql span
  if (context === null) {
    return physicalSpan(state.lineStarts, start, end);
  }
  return embeddedSqlContract(context, start, end, state).physicalSpan;
}

// detect providers in one sql payload
function detectSql(state, text, context, confidence) {
  const lexed = tokenizeSql(text);

  // retain one lexical failure
  if (lexed.error !== null) {
    addDiagnostic(
      state,
      "sql.lex-error",
      "error",
      sqlDiagnosticSpan(state, context, lexed.error.start, lexed.error.end),
    );
    return false;
  }

  // retain every nested dollar body
  for (const body of lexed.dollarBodies) {
    addSqlOccurrence(
      state,
      context,
      "postgres.dollar-quoted-body",
      "nested-language-candidate",
      "sql-dollar-quoted-body",
      "nested-language-unresolved",
      { form: "dollar-quoted-body" },
      body.start,
      body.end,
    );
    addDiagnostic(
      state,
      "sql.dollar-body-unresolved",
      "gap",
      sqlDiagnosticSpan(state, context, body.start, body.end),
    );
  }
  const tokens = lexed.tokens;

  // inspect every sql token
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    const identifier = sqlIdentifier(token);

    // retain one dynamic sql execution gap
    if (identifier === "execute" && !token.quoted) {
      addDiagnostic(
        state,
        "sql.dynamic-execution-unresolved",
        "gap",
        sqlDiagnosticSpan(state, context, token.start, token.end),
      );
    }

    // detect one keyword surface
    if (identifier !== null && !token.quoted && SQL_KEYWORD_SURFACES.has(identifier)) {
      let end = token.end;
      let precisionPresent = false;

      // include one optional precision
      if (
        SQL_PRECISION_KEYWORDS.has(identifier) &&
        tokens[index + 1]?.value === "(" &&
        tokens[index + 2]?.type === "number" &&
        tokens[index + 3]?.value === ")"
      ) {
        end = tokens[index + 3].end;
        precisionPresent = true;
      }
      addSqlOccurrence(
        state,
        context,
        SQL_KEYWORD_SURFACES.get(identifier),
        "current-time-read",
        "sql-keyword",
        confidence,
        {
          form: "keyword",
          precision: precisionPresent ? "present" : "absent",
        },
        token.start,
        end,
      );
      continue;
    }

    // detect one direct timestamp now literal
    if (
      identifier === "timestamp" &&
      !token.quoted &&
      tokens[index + 1]?.type === "string" &&
      !tokens[index + 1].escapeString &&
      tokens[index + 1].value.toLowerCase() === "now"
    ) {
      addSqlOccurrence(
        state,
        context,
        "postgres.timestamp-now-literal",
        "parse-time-current-value",
        "sql-special-literal",
        confidence,
        { form: "typed-current-literal", literal: "<string>" },
        token.start,
        tokens[index + 1].end,
      );
      continue;
    }

    // retain one contextual now cast gap
    if (
      token.type === "string" &&
      !token.escapeObserved &&
      token.value.toLowerCase() === "now" &&
      tokens[index + 1]?.value === "::" &&
      ["date", "time", "timestamp"].includes(sqlIdentifier(tokens[index + 2]))
    ) {
      addDiagnostic(
        state,
        "sql.typed-current-context-unresolved",
        "gap",
        sqlDiagnosticSpan(state, context, token.start, tokens[index + 2].end),
      );
      continue;
    }

    const precededByQualifier =
      index >= 2 && tokens[index - 1]?.value === "." && tokens[index - 2]?.type === "identifier";

    // detect one qualified function call
    if (
      !precededByQualifier &&
      token.type === "identifier" &&
      tokens[index + 1]?.value === "." &&
      tokens[index + 2]?.type === "identifier" &&
      tokens[index + 3]?.value === "(" &&
      tokens[index + 4]?.value === ")"
    ) {
      const terminal = sqlIdentifier(tokens[index + 2]);
      const surface = SQL_FUNCTION_SURFACES.get(terminal);

      // retain one bounded qualified function
      if (surface !== undefined) {
        addSqlOccurrence(
          state,
          context,
          surface,
          "current-time-read",
          "sql-function-call",
          confidence,
          {
            form: "function-call",
            qualifier: "present",
            qualifierQuoted: token.quoted,
            terminalQuoted: tokens[index + 2].quoted,
          },
          token.start,
          tokens[index + 4].end,
        );
        index += 4;
        continue;
      }
    }

    // skip one nonidentifier token
    if (identifier === null) {
      continue;
    }
    const surface = SQL_FUNCTION_SURFACES.get(identifier);

    // detect one unqualified function call
    if (
      !precededByQualifier &&
      surface !== undefined &&
      tokens[index + 1]?.value === "(" &&
      tokens[index + 2]?.value === ")"
    ) {
      addSqlOccurrence(
        state,
        context,
        surface,
        "current-time-read",
        "sql-function-call",
        confidence,
        {
          form: "function-call",
          qualifier: "absent",
          qualifierQuoted: false,
          terminalQuoted: token.quoted,
        },
        token.start,
        tokens[index + 2].end,
      );
      index += 2;
    }
  }
  return true;
}

// process one admitted embedded sql container
function processSqlContainer(state, sourceFile, node, mode, interpolationPresent) {
  // avoid duplicate fixed and anchored reads
  if (state.processedSqlNodes.has(node)) {
    return;
  }
  state.processedSqlNodes.add(node);
  const container = literalContainer(node, state, sourceFile, interpolationPresent);

  // require one fixed context or coarse anchor
  if (mode !== "fixed" && !SQL_ANCHOR_PATTERN.test(container.decodedText)) {
    return;
  }
  container.enclosingSymbol = enclosingSymbol(node);
  const confidence = mode === "fixed" ? "syntactic-direct" : "name-only-candidate";
  const completed = detectSql(state, container.decodedText, container, confidence);

  // retain one ambiguous anchor gap
  if (mode !== "fixed") {
    addDiagnostic(
      state,
      "sql.context-ambiguous",
      "gap",
      container.containerPhysicalSpan,
    );
  }

  // retain one embedded lexical failure
  if (!completed) {
    state.sqlLexingCompleted = false;
  }
}

// process one interpolated template
function processTemplateExpression(state, sourceFile, node, mode) {
  // avoid duplicate fixed and anchored templates
  if (state.processedTemplateExpressions.has(node)) {
    return;
  }
  const literals = [node.head];

  // collect every static segment
  for (const span of node.templateSpans) {
    literals.push(span.literal);
  }

  // require one fixed context or anchored segment
  if (
    mode !== "fixed" &&
    !literals.some(
      // test one decoded template segment
      (literal) => SQL_ANCHOR_PATTERN.test(literal.text),
    )
  ) {
    return;
  }
  state.processedTemplateExpressions.add(node);
  processSqlContainer(state, sourceFile, node.head, mode, true);

  // process every middle and tail segment
  for (const span of node.templateSpans) {
    processSqlContainer(state, sourceFile, span.literal, mode, true);
  }
  addDiagnostic(
    state,
    "sql.dynamic-fragment-unresolved",
    "gap",
    physicalSpan(state.lineStarts, node.getStart(sourceFile), node.getEnd()),
  );
}

// inspect one fixed-context sql call
function processSqlCall(state, sourceFile, node) {
  const path = expressionPath(node.expression);
  const terminal = path?.at(-1);

  // reject one unrelated call
  if (terminal === undefined || !SQL_FIXED_CALLEES.has(terminal)) {
    return;
  }

  // inspect every direct argument
  for (const argument of node.arguments) {
    // process one static literal
    if (ts.isStringLiteral(argument) || ts.isNoSubstitutionTemplateLiteral(argument)) {
      processSqlContainer(state, sourceFile, argument, "fixed", false);
    } else if (ts.isTemplateExpression(argument)) {
      processTemplateExpression(state, sourceFile, argument, "fixed");
    } else if (ts.isBinaryExpression(argument)) {
      addDiagnostic(
        state,
        "sql.dynamic-fragment-unresolved",
        "gap",
        physicalSpan(state.lineStarts, argument.getStart(sourceFile), argument.getEnd()),
      );
    } else {
      addDiagnostic(
        state,
        "sql.dynamic-execution-unresolved",
        "gap",
        physicalSpan(state.lineStarts, argument.getStart(sourceFile), argument.getEnd()),
      );
    }
  }
}

// inspect one tagged sql template
function processSqlTag(state, sourceFile, node) {
  const path = expressionPath(node.tag);

  // require the exact terminal sql tag
  if (path?.at(-1) !== "sql") {
    return;
  }

  // process one static tagged template
  if (ts.isNoSubstitutionTemplateLiteral(node.template)) {
    processSqlContainer(state, sourceFile, node.template, "fixed", false);
  } else {
    processTemplateExpression(state, sourceFile, node.template, "fixed");
  }
}

// detect javascript and embedded sql candidates
function detectJavascript(state, sourceFile, analysis) {
  walkAst(
    sourceFile,
    // inspect one syntax node
    (node) => {
      // detect one call expression
      if (ts.isCallExpression(node)) {
        const descriptor = describeCall(analysis, node);

        // retain one javascript call
        if (descriptor !== null) {
          addJavascriptOccurrence(state, sourceFile, node, descriptor);
        }
        processSqlCall(state, sourceFile, node);
      } else if (ts.isNewExpression(node)) {
        const descriptor = describeNewExpression(analysis, node);

        // retain one Date constructor
        if (descriptor !== null) {
          addJavascriptOccurrence(state, sourceFile, node, descriptor);
        }

        // retain one dynamic constructor gap
        if (
          descriptor === null &&
          ts.isIdentifier(node.expression) &&
          node.expression.text === "Function"
        ) {
          addNodeDiagnostic(analysis, node, "javascript.dynamic-code-unresolved");
        }
      } else if (ts.isPropertyAccessExpression(node)) {
        // avoid one property used as a call callee
        if (!(ts.isCallExpression(node.parent) && node.parent.expression === node)) {
          const descriptor = describePropertyRead(analysis, node);

          // retain one property read
          if (descriptor !== null) {
            addJavascriptOccurrence(state, sourceFile, node, descriptor);
          }
        }
      } else if (ts.isTaggedTemplateExpression(node)) {
        processSqlTag(state, sourceFile, node);
      } else if (ts.isTemplateExpression(node)) {
        processTemplateExpression(state, sourceFile, node, "anchor");
      }

      // inspect one anchor-only string
      if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node)) {
        processSqlContainer(state, sourceFile, node, "anchor", false);
      }
      return undefined;
    },
  );
}

// compare two occurrence drafts deterministically
function compareOccurrenceDrafts(left, right) {
  const leftDecoded = left.embeddedSql?.decodedRange ?? {
    startOffsetUtf16: -1,
    endOffsetUtf16: -1,
  };
  const rightDecoded = right.embeddedSql?.decodedRange ?? {
    startOffsetUtf16: -1,
    endOffsetUtf16: -1,
  };
  return (left.sourcePath < right.sourcePath ? -1 : left.sourcePath > right.sourcePath ? 1 : 0) ||
    left.physicalSpan.startOffsetUtf16 - right.physicalSpan.startOffsetUtf16 ||
    left.physicalSpan.endOffsetUtf16 - right.physicalSpan.endOffsetUtf16 ||
    leftDecoded.startOffsetUtf16 - rightDecoded.startOffsetUtf16 ||
    leftDecoded.endOffsetUtf16 - rightDecoded.endOffsetUtf16 ||
    (left.surface < right.surface ? -1 : left.surface > right.surface ? 1 : 0) ||
    (left.kind < right.kind ? -1 : left.kind > right.kind ? 1 : 0) ||
    (left.normalizedExpressionSha256 < right.normalizedExpressionSha256 ? -1 :
      left.normalizedExpressionSha256 > right.normalizedExpressionSha256 ? 1 : 0);
}

// finalize deterministic occurrence ids and ordinals
function finalizeOccurrences(drafts) {
  drafts.sort(compareOccurrenceDrafts);
  const ordinals = new Map();
  const occurrences = [];

  // finalize every sorted draft
  for (const draft of drafts) {
    const groupKey = JSON.stringify([
      draft.sourcePath,
      draft.surface,
      draft.kind,
      draft.enclosingSymbol,
      draft.normalizedExpressionSha256,
    ]);
    const duplicateOrdinal = (ordinals.get(groupKey) ?? 0) + 1;
    ordinals.set(groupKey, duplicateOrdinal);
    const payload = {
      sourcePath: draft.sourcePath,
      language: draft.language,
      surface: draft.surface,
      kind: draft.kind,
      syntaxKind: draft.syntaxKind,
      enclosingSymbol: draft.enclosingSymbol,
      confidence: draft.confidence,
      aliasTrace: draft.aliasTrace,
      normalizedExpressionSha256: draft.normalizedExpressionSha256,
      physicalSpan: draft.physicalSpan,
      embeddedSql: draft.embeddedSql,
      duplicateOrdinal,
      countsAsClockCandidate: draft.countsAsClockCandidate,
    };
    occurrences.push({
      id: `clock-occurrence-v1:${framedSha256(
        "weather-clock-occurrence/v1",
        JSON.stringify(payload),
      )}`,
      ...payload,
    });
  }
  return occurrences;
}

// compare two diagnostics deterministically
function compareDiagnostics(left, right) {
  const leftStart = left.physicalSpan?.startOffsetUtf16 ?? -1;
  const rightStart = right.physicalSpan?.startOffsetUtf16 ?? -1;
  const leftEnd = left.physicalSpan?.endOffsetUtf16 ?? -1;
  const rightEnd = right.physicalSpan?.endOffsetUtf16 ?? -1;
  return (left.sourcePath < right.sourcePath ? -1 : left.sourcePath > right.sourcePath ? 1 : 0) ||
    leftStart - rightStart ||
    leftEnd - rightEnd ||
    (left.code < right.code ? -1 : left.code > right.code ? 1 : 0);
}

// build one exact result envelope
function buildResult(state, coverage) {
  const occurrences = finalizeOccurrences(state.occurrenceDrafts);
  state.diagnostics.sort(compareDiagnostics);
  return freezeRecursively({
    contractVersion: BACKEND_CLOCK_OCCURRENCES_VERSION,
    source: {
      path: state.sourcePath,
      language: state.language,
      byteLength: state.sourceBytes.length,
      sha256: createHash("sha256").update(state.sourceBytes).digest("hex"),
    },
    coverage,
    occurrences,
    diagnostics: state.diagnostics,
    canonicalEligible: false,
    referenceFlowProven: false,
    physicalCoverageProven: false,
    operationReachabilityProven: false,
    readyForMetrics: false,
  });
}

// extract one frozen syntactic clock report
export function extractBackendClockOccurrences(...argumentsList) {
  const input = validateInput(argumentsList);
  const sourceBytes = Buffer.from(input.sourceText, "utf8");
  const state = {
    ...input,
    sourceBytes,
    lineStarts: buildLineStarts(input.sourceText),
    occurrenceDrafts: [],
    diagnostics: [],
    processedSqlNodes: new Set(),
    processedTemplateExpressions: new Set(),
    sqlLexingCompleted: true,
  };

  // fail closed above the byte ceiling
  if (sourceBytes.length > SOURCE_BYTE_LIMIT) {
    addDiagnostic(state, "source.size-limit", "error", null);
    return buildResult(state, {
      sourceParseCompleted: false,
      sqlLexingCompleted: false,
      candidateExtractionCompleted: false,
    });
  }

  // lex one raw sql source
  if (input.language === "sql") {
    const completed = detectSql(state, input.sourceText, null, "syntactic-direct");
    return buildResult(state, {
      sourceParseCompleted: completed,
      sqlLexingCompleted: completed,
      candidateExtractionCompleted: completed,
    });
  }
  const sourceFile = ts.createSourceFile(
    input.sourcePath,
    input.sourceText,
    ts.ScriptTarget.Latest,
    true,
    input.scriptKind,
  );
  const parseDiagnostics = sourceFile.parseDiagnostics;

  // fail closed on parser recovery
  if (!Array.isArray(parseDiagnostics) || parseDiagnostics.length > 0) {
    const first = Array.isArray(parseDiagnostics) ? parseDiagnostics[0] : null;
    const start = typeof first?.start === "number" ? first.start : 0;
    const length = typeof first?.length === "number" ? first.length : 0;
    addDiagnostic(
      state,
      "source.parse-error",
      "error",
      physicalSpan(state.lineStarts, start, start + length),
    );
    return buildResult(state, {
      sourceParseCompleted: false,
      sqlLexingCompleted: false,
      candidateExtractionCompleted: false,
    });
  }
  const analysis = analyzeJavascript(sourceFile, state);
  detectJavascript(state, sourceFile, analysis);
  return buildResult(state, {
    sourceParseCompleted: true,
    sqlLexingCompleted: state.sqlLexingCompleted,
    candidateExtractionCompleted: state.sqlLexingCompleted,
  });
}
