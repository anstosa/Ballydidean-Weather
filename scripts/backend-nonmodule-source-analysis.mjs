import { spawn } from "node:child_process";
import { createHash } from "node:crypto";

import { extractBackendClockOccurrences } from "./backend-clock-occurrences.mjs";

const PRIVATE_SOURCE_KEYS = Object.freeze([
  "containerSpan",
  "derivedId",
  "language",
  "origins",
  "parentPath",
  "parentRawSha256",
  "path",
  "rawSha256",
  "resolutionBase",
  "sourceKind",
  "sourceText",
]);
const ORIGINS = new Set([
  "prerequisite",
  "initial-seed",
  "local-module",
  "runtime-opened-code",
  "raw-sql",
  "database-definition",
  "shell-source",
  "embedded-javascript",
  "runtime-data",
]);
const SUPPORTED_LANGUAGES = Object.freeze(["python", "shell", "sql"]);
const SHA256_PATTERN = /^[0-9a-f]{64}$/u;
const PYTHON_VERSION_PATTERN = /^3\.(10|11|12|13|14)\.[0-9]+$/u;
const PER_FILE_BYTE_CAP = 1_048_576;
const BATCH_BYTE_CAP = 4_194_304;
const PYTHON_STDOUT_CAP = 33_554_432;
const PYTHON_NODE_CAP = 250_000;
const DERIVED_SOURCE_BYTE_CAP = 262_144;
const DERIVED_TOTAL_BYTE_CAP = 2_097_152;
const DERIVED_SOURCE_COUNT_CAP = 128;
const STRING_LITERAL_BYTE_CAP = 4_096;
const DIAGNOSTIC_COUNT_CAP = 10_000;
const PYTHON_HELPER_SOURCE = String.raw`
import ast
import hashlib
import json
import math
import sys

# require the reviewed interpreter family
if sys.implementation.name != "cpython":
    raise RuntimeError("implementation")

version = sys.version_info

# require the reviewed version range
if version < (3, 10, 0) or version >= (3, 15, 0):
    raise RuntimeError("version")

sample = ast.parse("\u03c0 = 1\nformatted = f'{\u03c0}'\n")
name = sample.body[0].targets[0]
formatted = sample.body[1].value

# verify native utf-8 byte columns and ends
if name.col_offset != 0 or name.end_col_offset != 2:
    raise RuntimeError("span")

# verify the required common ast features
if not isinstance(formatted, ast.JoinedStr) or not any(isinstance(value, ast.FormattedValue) for value in formatted.values):
    raise RuntimeError("features")

features = ["end-positions", "joined-string", "formatted-value"]

# append optional template features together
if hasattr(ast, "TemplateStr") or hasattr(ast, "Interpolation"):
    if not hasattr(ast, "TemplateStr") or not hasattr(ast, "Interpolation"):
        raise RuntimeError("template-features")
    features.extend(["template-string", "interpolation"])

profile = {
    "astFeatures": features,
    "implementation": "CPython",
    "spanEncoding": "utf8-byte-columns-exclusive-end",
    "version": f"{version.major}.{version.minor}.{version.micro}",
}

kind_by_type = {
    ast.Module: "module", ast.FunctionDef: "function", ast.AsyncFunctionDef: "async-function",
    ast.Lambda: "lambda", ast.arg: "parameter", ast.Import: "import", ast.ImportFrom: "import-from",
    ast.alias: "alias", ast.Assign: "assign", ast.AnnAssign: "annotated-assign",
    ast.AugAssign: "augmented-assign", ast.NamedExpr: "named-expression", ast.Name: "name",
    ast.Attribute: "attribute", ast.Call: "call", ast.keyword: "keyword", ast.Constant: "constant",
    ast.If: "if", ast.IfExp: "conditional-expression", ast.For: "for", ast.AsyncFor: "async-for",
    ast.While: "while", ast.Try: "try", ast.ExceptHandler: "except", ast.With: "with",
    ast.AsyncWith: "async-with", ast.Return: "return", ast.Raise: "raise", ast.BoolOp: "bool-operator",
    ast.BinOp: "binary-operator", ast.UnaryOp: "unary-operator", ast.Compare: "compare",
    ast.Subscript: "subscript", ast.Slice: "slice", ast.List: "list", ast.Tuple: "tuple",
    ast.Set: "set", ast.Dict: "dict", ast.Starred: "starred", ast.ListComp: "list-comprehension",
    ast.SetComp: "set-comprehension", ast.DictComp: "dict-comprehension",
    ast.GeneratorExp: "generator-expression", ast.Await: "await", ast.Yield: "yield",
    ast.YieldFrom: "yield-from", ast.JoinedStr: "joined-string", ast.FormattedValue: "formatted-value",
    ast.Expr: "expression-statement",
    ast.Pass: "pass", ast.Break: "break", ast.Continue: "continue",
}
# register optional template nodes together
if hasattr(ast, "TemplateStr"):
    kind_by_type[ast.TemplateStr] = "template-string"
# register optional interpolation nodes together
if hasattr(ast, "Interpolation"):
    kind_by_type[ast.Interpolation] = "interpolation"

token_by_type = {
    ast.Load: "context.load", ast.Store: "context.store", ast.Del: "context.delete",
    ast.And: "operator.and", ast.Or: "operator.or", ast.Add: "operator.add",
    ast.Sub: "operator.subtract", ast.Mult: "operator.multiply", ast.MatMult: "operator.matrix-multiply",
    ast.Div: "operator.true-divide", ast.FloorDiv: "operator.floor-divide", ast.Mod: "operator.modulo",
    ast.Pow: "operator.power", ast.LShift: "operator.left-shift", ast.RShift: "operator.right-shift",
    ast.BitOr: "operator.bitwise-or", ast.BitXor: "operator.bitwise-xor", ast.BitAnd: "operator.bitwise-and",
    ast.Invert: "operator.invert", ast.Not: "operator.not", ast.UAdd: "operator.unary-plus",
    ast.USub: "operator.unary-minus", ast.Eq: "operator.equal", ast.NotEq: "operator.not-equal",
    ast.Lt: "operator.less-than", ast.LtE: "operator.less-than-or-equal",
    ast.Gt: "operator.greater-than", ast.GtE: "operator.greater-than-or-equal", ast.Is: "operator.is",
    ast.IsNot: "operator.is-not", ast.In: "operator.in", ast.NotIn: "operator.not-in",
}
conversion_by_value = {-1: "conversion.none", 115: "conversion.string", 114: "conversion.repr", 97: "conversion.ascii"}
emitted_node_count = 0

# encode one exact float
def float_value(value):
    # retain nonfinite spellings
    if math.isnan(value):
        return "nan"
    # retain positive infinity
    if value == math.inf:
        return "inf"
    # retain negative infinity
    if value == -math.inf:
        return "-inf"
    return value.hex()

# encode one exact constant
def literal_value(value):
    # retain none before numerics
    if value is None:
        return "none", "null"
    # retain booleans before integers
    if isinstance(value, bool):
        return "boolean", "true" if value else "false"
    # retain arbitrary integers
    if isinstance(value, int):
        return "integer", str(value)
    # retain binary floats
    if isinstance(value, float):
        return "float", float_value(value)
    # retain exact complex components
    if isinstance(value, complex):
        return "complex", f"({float_value(value.real)},{float_value(value.imag)})"
    # retain decoded strings privately
    if isinstance(value, str):
        return "string", value
    # retain bytes as hexadecimal
    if isinstance(value, bytes):
        return "bytes", value.hex()
    # retain ellipsis
    if value is Ellipsis:
        return "ellipsis", "..."
    raise RuntimeError("constant")

# resolve one dotted callee
def dotted_name(node):
    # resolve one name leaf
    if isinstance(node, ast.Name):
        return node.id
    # resolve one attribute chain
    if isinstance(node, ast.Attribute):
        base = dotted_name(node.value)
        return None if base is None else f"{base}.{node.attr}"
    return None

# identify bounded role-required literals
def proof_target_names(target):
    names = []
    stack = [target]
    # flatten one binder without recursion
    while stack:
        current = stack.pop()
        # retain one direct target
        if isinstance(current, ast.Name):
            names.append(current.id)
        # descend through one destructuring target
        elif isinstance(current, (ast.List, ast.Tuple)):
            stack.extend(reversed(current.elts))
        # descend through one starred target
        elif isinstance(current, ast.Starred):
            stack.append(current.value)
    return names

# compare one proof cell exactly
def proof_same_cell(left, right):
    # reject one different cell kind
    if left[0] != right[0]:
        return False
    # preserve literal node identity
    if left[0] == "literal":
        return left[1] is right[1]
    return left[1] == right[1]

# retain closed proof surface ids
PROOF_NONE = 0
PROOF_SUBPROCESS = 1
PROOF_RUN = 2
PROOF_CALL = 3
PROOF_CHECK_CALL = 4
PROOF_CHECK_OUTPUT = 5
PROOF_POPEN = 6
PROOF_PIPE = 7
PROOF_SUBPROCESS_MEMBER = 8
PROOF_IMPORTLIB = 9
PROOF_IMPORT_MODULE = 10
PROOF_APIS = {PROOF_RUN, PROOF_CALL, PROOF_CHECK_CALL, PROOF_CHECK_OUTPUT, PROOF_POPEN}

# resolve one exact import surface id
def proof_import_surface(module, member=None):
    # resolve one direct module import
    if member is None:
        if module == "subprocess":
            return PROOF_SUBPROCESS
        if module == "importlib":
            return PROOF_IMPORTLIB
        return PROOF_NONE
    # resolve one subprocess member import
    if module == "subprocess":
        return {
            "run": PROOF_RUN,
            "call": PROOF_CALL,
            "check_call": PROOF_CHECK_CALL,
            "check_output": PROOF_CHECK_OUTPUT,
            "Popen": PROOF_POPEN,
            "PIPE": PROOF_PIPE,
        }.get(member, PROOF_NONE)
    # resolve one importlib member import
    if module == "importlib" and member == "import_module":
        return PROOF_IMPORT_MODULE
    return PROOF_NONE

# extend one finite surface path
def proof_extend_surface(surface, depth, first_attribute):
    # preserve one root surface
    if depth == 0:
        return surface
    # extend one subprocess module reference
    if surface == PROOF_SUBPROCESS:
        if depth > 1:
            return PROOF_SUBPROCESS_MEMBER
        return {
            "run": PROOF_RUN,
            "call": PROOF_CALL,
            "check_call": PROOF_CHECK_CALL,
            "check_output": PROOF_CHECK_OUTPUT,
            "Popen": PROOF_POPEN,
            "PIPE": PROOF_PIPE,
        }.get(first_attribute, PROOF_SUBPROCESS_MEMBER)
    # extend one importlib module reference
    if surface == PROOF_IMPORTLIB:
        if depth == 1 and first_attribute == "import_module":
            return PROOF_IMPORT_MODULE
        return PROOF_NONE
    # retain extended subprocess members conservatively
    if surface in PROOF_APIS or surface in (PROOF_PIPE, PROOF_SUBPROCESS_MEMBER):
        return PROOF_SUBPROCESS_MEMBER
    return PROOF_NONE

# classify one canonical subprocess identity
def proof_canonical_surface(surface):
    return surface == PROOF_SUBPROCESS or surface in PROOF_APIS or surface == PROOF_SUBPROCESS_MEMBER

# identify one exact literal role map
def literal_roles(tree):
    roles = {}
    names = []
    name_ids = {}
    # intern one proof name
    def intern_name(name):
        name_id = name_ids.get(name)
        # retain one existing name id
        if name_id is not None:
            return name_id
        name_id = len(names)
        names.append(name)
        name_ids[name] = name_id
        return name_id
    scopes = [{
        "children": [], "events": [], "locals": set(), "parent": None,
        "relevant": set(), "type": "module",
    }]
    records = []
    owners = {}
    scope_by_node = {id(tree): 0}
    conditional_by_node = {id(tree): False}
    body_scope_by_node = {}
    stack = [(tree, 0, False, None, "root")]

    # assign every syntax node to one proof scope
    while stack:
        node, scope_id, conditional, owner, relation = stack.pop()
        node_id = id(node)
        scope_by_node[node_id] = scope_id
        conditional_by_node[node_id] = conditional
        owners[node_id] = (owner, relation)
        records.append(node)
        # split one callable body from enclosing expressions
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            child_scope = len(scopes)
            scopes.append({
                "children": [], "events": [], "locals": set(),
                "parent": scope_id, "relevant": set(), "type": "callable",
            })
            scopes[scope_id]["children"].append(child_scope)
            body_scope_by_node[node_id] = child_scope
            enclosing = list(node.decorator_list) + list(node.args.defaults)
            enclosing.extend(value for value in node.args.kw_defaults if value is not None)
            arguments = list(node.args.posonlyargs) + list(node.args.args) + list(node.args.kwonlyargs)
            enclosing.extend(argument.annotation for argument in arguments if argument.annotation is not None)
            # retain optional callable annotations
            if node.args.vararg is not None and node.args.vararg.annotation is not None:
                enclosing.append(node.args.vararg.annotation)
            # retain optional keyword annotations
            if node.args.kwarg is not None and node.args.kwarg.annotation is not None:
                enclosing.append(node.args.kwarg.annotation)
            # retain one return annotation
            if node.returns is not None:
                enclosing.append(node.returns)
            # push enclosing expressions in reverse order
            for child in reversed(enclosing):
                stack.append((child, scope_id, conditional, node, "terminal"))
            # push callable statements in reverse order
            for child in reversed(node.body):
                stack.append((child, child_scope, conditional, node, "terminal"))
            # predeclare ordinary parameters
            for argument in arguments:
                scopes[child_scope]["locals"].add(intern_name(argument.arg))
            # predeclare one variadic parameter
            if node.args.vararg is not None:
                scopes[child_scope]["locals"].add(intern_name(node.args.vararg.arg))
            # predeclare one keyword variadic parameter
            if node.args.kwarg is not None:
                scopes[child_scope]["locals"].add(intern_name(node.args.kwarg.arg))
            continue
        # isolate one lambda body
        if isinstance(node, ast.Lambda):
            child_scope = len(scopes)
            scopes.append({
                "children": [], "events": [], "locals": set(),
                "parent": scope_id, "relevant": set(), "type": "callable",
            })
            scopes[scope_id]["children"].append(child_scope)
            body_scope_by_node[node_id] = child_scope
            arguments = list(node.args.posonlyargs) + list(node.args.args) + list(node.args.kwonlyargs)
            # predeclare lambda parameters
            for argument in arguments:
                scopes[child_scope]["locals"].add(intern_name(argument.arg))
            # retain optional variadic parameters
            if node.args.vararg is not None:
                scopes[child_scope]["locals"].add(intern_name(node.args.vararg.arg))
            if node.args.kwarg is not None:
                scopes[child_scope]["locals"].add(intern_name(node.args.kwarg.arg))
            enclosing = list(node.args.defaults)
            enclosing.extend(value for value in node.args.kw_defaults if value is not None)
            enclosing.extend(argument.annotation for argument in arguments if argument.annotation is not None)
            # push lambda defaults and annotations
            for child in reversed(enclosing):
                stack.append((child, scope_id, conditional, node, "terminal"))
            stack.append((node.body, child_scope, conditional, node, "terminal"))
            continue
        # isolate one comprehension scope
        if isinstance(node, (ast.ListComp, ast.SetComp, ast.DictComp, ast.GeneratorExp)):
            child_scope = len(scopes)
            scopes.append({
                "children": [], "events": [], "locals": set(),
                "parent": scope_id, "relevant": set(), "type": "comprehension",
            })
            scopes[scope_id]["children"].append(child_scope)
            body_scope_by_node[node_id] = child_scope
            generators = list(node.generators)
            implicit = []
            # retain each generator clause after the outer iterator
            for index, generator in enumerate(generators):
                if index > 0:
                    implicit.append(generator.iter)
                implicit.append(generator.target)
                implicit.extend(generator.ifs)
            # retain one produced comprehension value
            if isinstance(node, ast.DictComp):
                implicit.extend((node.key, node.value))
            else:
                implicit.append(node.elt)
            # assign implicit children once to the child scope
            for child in reversed(implicit):
                stack.append((child, child_scope, True, node, "terminal"))
            # evaluate the first iterator in the enclosing scope
            if generators:
                stack.append((generators[0].iter, scope_id, conditional, node, "terminal"))
            continue
        branch = conditional or isinstance(node, (
            ast.If, ast.For, ast.AsyncFor, ast.While, ast.Try,
            ast.ExceptHandler,
        ))
        children = []
        # assign every owned child iteratively
        for field, value in ast.iter_fields(node):
            relation = "attribute-receiver" if isinstance(node, ast.Attribute) and field == "value" else "call-func" if isinstance(node, ast.Call) and field == "func" else "terminal"
            # retain one scalar child
            if isinstance(value, ast.AST):
                children.append((value, relation))
            # retain every list child
            elif isinstance(value, list):
                for child in value:
                    if isinstance(child, ast.AST):
                        children.append((child, relation))
        # push owned children in reverse order
        for child, relation in reversed(children):
            stack.append((child, scope_id, branch, node, relation))

    # compute finite path descriptors bottom-up
    shapes = {}
    for node in reversed(records):
        node_id = id(node)
        # retain one name root
        if isinstance(node, ast.Name):
            root_id = intern_name(node.id)
            shapes[node_id] = (root_id, 0, None)
            scopes[scope_by_node[node_id]]["relevant"].add(root_id)
        # extend one finite attribute path
        elif isinstance(node, ast.Attribute) and id(node.value) in shapes:
            root_id, depth, first_attribute = shapes[id(node.value)]
            shapes[node_id] = (
                root_id,
                1 if depth == 0 else 2,
                node.attr if depth == 0 else first_attribute,
            )

    # predeclare every compile-time local name
    for node in records:
        scope_id = scope_by_node[id(node)]
        scope = scopes[scope_id]
        # bind one callable name in its enclosing scope
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            scope["locals"].add(intern_name(node.name))
        # bind every assignment target
        if isinstance(node, ast.Assign):
            for target in node.targets:
                scope["locals"].update(intern_name(name) for name in proof_target_names(target))
        # bind one annotated or named target
        elif isinstance(node, (ast.AnnAssign, ast.NamedExpr, ast.AugAssign)):
            scope["locals"].update(intern_name(name) for name in proof_target_names(node.target))
        # bind one loop target
        elif isinstance(node, (ast.For, ast.AsyncFor)):
            scope["locals"].update(intern_name(name) for name in proof_target_names(node.target))
        # bind every context alias
        elif isinstance(node, (ast.With, ast.AsyncWith)):
            for item in node.items:
                # retain one present context alias
                if item.optional_vars is not None:
                    scope["locals"].update(intern_name(name) for name in proof_target_names(item.optional_vars))
        # bind one exception alias
        elif isinstance(node, ast.ExceptHandler) and node.name is not None:
            scope["locals"].add(intern_name(node.name))
        # bind every import alias
        elif isinstance(node, ast.Import):
            for alias in node.names:
                scope["locals"].add(intern_name(alias.asname or alias.name.split(".")[0]))
        # bind every explicit from-import alias
        elif isinstance(node, ast.ImportFrom):
            for alias in node.names:
                # skip one wildcard binder
                if alias.name != "*":
                    scope["locals"].add(intern_name(alias.asname or alias.name))
        # retain one backstop local target
        if isinstance(node, ast.Name) and isinstance(node.ctx, (ast.Store, ast.Del)):
            scope["locals"].add(intern_name(node.id))
    next_event_id = 1
    # append one ordered binding event
    def add_event(scope_id, node, name, cell, conditional):
        nonlocal next_event_id
        position = (
            getattr(node, "end_lineno", getattr(node, "lineno", 0)),
            getattr(node, "end_col_offset", getattr(node, "col_offset", 0)),
        )
        event_id = next_event_id
        next_event_id += 1
        scopes[scope_id]["events"].append((position, name, cell, conditional, event_id))
        return event_id

    # build every exact or poisoning binding event
    for node in records:
        scope_id = scope_by_node[id(node)]
        conditional = conditional_by_node[id(node)]
        # bind one callable declaration after header evaluation
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            add_event(scope_id, node, intern_name(node.name), ("poison", None), conditional)
        # bind one exact module import
        elif isinstance(node, ast.Import):
            for alias in node.names:
                local = alias.asname or alias.name.split(".")[0]
                module = alias.name if alias.asname is not None else alias.name.split(".")[0]
                surface = proof_import_surface(module)
                cell = ("surface", surface) if surface != PROOF_NONE and not conditional else ("poison", None)
                add_event(scope_id, node, intern_name(local), cell, conditional)
        # bind one exact from-import
        elif isinstance(node, ast.ImportFrom):
            for alias in node.names:
                # retain one constant wildcard barrier
                if alias.name == "*":
                    add_event(scope_id, node, None, ("wildcard", None), True)
                    continue
                local = alias.asname or alias.name
                surface = PROOF_NONE if node.level != 0 or node.module is None else proof_import_surface(node.module, alias.name)
                cell = ("surface", surface) if surface != PROOF_NONE and not conditional else ("poison", None)
                add_event(scope_id, node, intern_name(local), cell, conditional)
        # bind one exact simple assignment
        elif isinstance(node, ast.Assign):
            simple = len(node.targets) == 1 and isinstance(node.targets[0], ast.Name)
            literal = isinstance(node.value, ast.Constant) and isinstance(node.value.value, str)
            for target in node.targets:
                for name in proof_target_names(target):
                    cell = ("literal", node.value) if simple and literal and not conditional else ("poison", None)
                    add_event(scope_id, node, intern_name(name), cell, conditional)
        # bind one exact annotated assignment
        elif isinstance(node, ast.AnnAssign):
            literal = isinstance(node.value, ast.Constant) and isinstance(node.value.value, str)
            for name in proof_target_names(node.target):
                cell = ("literal", node.value) if literal and not conditional else ("poison", None)
                add_event(scope_id, node, intern_name(name), cell, conditional)
        # poison remaining direct target families
        elif isinstance(node, (ast.NamedExpr, ast.AugAssign)):
            for name in proof_target_names(node.target):
                add_event(scope_id, node, intern_name(name), ("poison", None), conditional)
        # poison one loop target
        elif isinstance(node, (ast.For, ast.AsyncFor)):
            for name in proof_target_names(node.target):
                add_event(scope_id, node, intern_name(name), ("poison", None), True)
        # poison every context alias
        elif isinstance(node, (ast.With, ast.AsyncWith)):
            for item in node.items:
                # retain one present alias
                if item.optional_vars is not None:
                    for name in proof_target_names(item.optional_vars):
                        add_event(scope_id, node, intern_name(name), ("poison", None), conditional)
        # poison one exception alias
        elif isinstance(node, ast.ExceptHandler) and node.name is not None:
            add_event(scope_id, node, intern_name(node.name), ("poison", None), True)

    # derive one stable module-cell summary
    module_stable = {}
    module_seen = set()
    module_poison_all = False
    for _, name, cell, conditional, _ in scopes[0]["events"]:
        # retain one permanent wildcard instability
        if name is None:
            module_poison_all = True
            continue
        # poison one conditional or competing definition
        if conditional or cell[0] == "poison":
            module_stable[name] = ("poison", None)
            module_seen.add(name)
            continue
        # retain one first exact definition
        if name not in module_seen:
            module_stable[name] = cell
            module_seen.add(name)
        # poison one distinct later definition
        elif not proof_same_cell(module_stable[name], cell):
            module_stable[name] = ("poison", None)

    free_allowed = [set() for _ in scopes]
    blockers = {}
    scope_stack = [(0, False)]
    # project module-free names through the scope tree
    while scope_stack:
        scope_id, exiting = scope_stack.pop()
        scope = scopes[scope_id]
        # undo one completed scope
        if exiting:
            if scope_id != 0:
                for name_id in scope["locals"]:
                    blockers[name_id] -= 1
                    if blockers[name_id] == 0:
                        del blockers[name_id]
            continue
        # retain every unblocked free name
        for name_id in scope["relevant"]:
            if name_id not in scope["locals"] and blockers.get(name_id, 0) == 0:
                free_allowed[scope_id].add(name_id)
        scope_stack.append((scope_id, True))
        # block this scope's locals for descendants
        if scope_id != 0:
            for name_id in scope["locals"]:
                blockers[name_id] = blockers.get(name_id, 0) + 1
        # visit child scopes in stable order
        for child_scope in reversed(scope["children"]):
            scope_stack.append((child_scope, False))

    resolved = {}
    # resolve one root cell without ancestor walks
    def resolve_root(scope_id, root_id, active, epochs):
        scope = scopes[scope_id]
        # use one current compile-local cell
        if root_id in scope["locals"]:
            entry = active[scope_id].get(root_id)
            if entry is not None and entry[0] == epochs[scope_id]:
                return entry[2], entry[1]
            return ("poison", None), None
        # use one stable module-free cell
        if root_id in free_allowed[scope_id] and not module_poison_all:
            return module_stable.get(root_id, ("poison", None)), None
        return ("poison", None), None

    actions_by_line = {}
    maximum_line = 0
    maximum_column_by_line = {}
    current_event_by_node = {}
    current_epoch_by_node = {}
    # reject one direct name binder read
    def direct_name_binder(node):
        owner = owners.get(id(node), (None, None))[0]
        return (
            isinstance(node, ast.Name) and
            isinstance(node.ctx, (ast.Store, ast.Del)) and
            not isinstance(owner, ast.AugAssign)
        )
    # retain every binding event in source-coordinate buckets
    for scope_id, scope in enumerate(scopes):
        for position, name, cell, _, event_id in scope["events"]:
            line, column = position
            line_actions = actions_by_line.setdefault(line, {})
            line_actions.setdefault(column, []).append(("event", scope_id, name, (cell, event_id)))
            maximum_line = max(maximum_line, line)
            maximum_column_by_line[line] = max(
                maximum_column_by_line.get(line, 0), column,
            )
    # retain every path and name demand in source-coordinate buckets
    for node in records:
        # retain path-shaped expression demands only
        if id(node) not in shapes or direct_name_binder(node):
            continue
        line = getattr(node, "lineno", 0)
        column = getattr(node, "col_offset", 0)
        line_actions = actions_by_line.setdefault(line, {})
        line_actions.setdefault(column, []).append((
            "demand", scope_by_node[id(node)], node, None,
        ))
        maximum_line = max(maximum_line, line)
        maximum_column_by_line[line] = max(
            maximum_column_by_line.get(line, 0), column,
        )
    active_by_scope = [
        {}
        for scope in scopes
    ]
    epochs = [0 for _ in scopes]
    # process source coordinates in bounded linear order
    for line in range(maximum_line + 1):
        line_actions = actions_by_line.get(line)
        # skip one empty physical line
        if line_actions is None:
            continue
        for column in range(maximum_column_by_line[line] + 1):
            actions = line_actions.get(column, ())
            # resolve demands before same-position writes
            for action, scope_id, primary, _ in actions:
                # skip one binding action in the read phase
                if action != "demand":
                    continue
                root_id, depth, first_attribute = shapes[id(primary)]
                root_cell, event_id = resolve_root(
                    scope_id, root_id, active_by_scope, epochs,
                )
                current_event_by_node[id(primary)] = event_id
                current_epoch_by_node[id(primary)] = epochs[scope_id]
                # retain one exact provider surface
                if root_cell[0] == "surface":
                    surface = proof_extend_surface(root_cell[1], depth, first_attribute)
                    if surface != PROOF_NONE:
                        resolved[id(primary)] = (("surface", surface), event_id)
                # retain one exact literal name
                elif root_cell[0] == "literal" and depth == 0:
                    resolved[id(primary)] = (root_cell, event_id)
            # install writes after same-position reads
            for action, scope_id, primary, cell in actions:
                # skip one demand action in the write phase
                if action != "event":
                    continue
                value, event_id = cell
                # advance one wildcard epoch without scanning locals
                if primary is None:
                    epochs[scope_id] += 1
                    continue
                active_by_scope[scope_id][primary] = (
                    epochs[scope_id], event_id, value,
                )

    surface_tainted = False
    # classify every terminal canonical use once
    for node in records:
        resolution = resolved.get(id(node))
        # skip one unresolved or literal expression
        if (
            direct_name_binder(node) or resolution is None or
            resolution[0][0] != "surface"
        ):
            continue
        surface = resolution[0][1]
        owner, relation = owners[id(node)]
        # propagate one attribute receiver
        if relation == "attribute-receiver":
            continue
        # discharge one exact reviewed API call target
        if relation == "call-func" and surface in PROOF_APIS:
            continue
        # discharge every exact PIPE load
        if surface == PROOF_PIPE and isinstance(getattr(node, "ctx", None), ast.Load):
            continue
        # taint every remaining canonical terminal use
        if proof_canonical_surface(surface) or surface == PROOF_PIPE:
            surface_tainted = True
    has_subprocess_binding = any(
        cell[0] == "surface" and (
            cell[1] == PROOF_SUBPROCESS or cell[1] in PROOF_APIS
        )
        for scope in scopes for _, _, cell, _, _ in scope["events"]
    )
    # inspect exact bare builtins without recursive path recovery
    for node in records:
        # skip non-call nodes and nonbare functions
        if not isinstance(node, ast.Call) or not isinstance(node.func, ast.Name):
            continue
        scope_id = scope_by_node[id(node.func)]
        name_id = intern_name(node.func.id)
        # use current module state but stable nested free state
        if scope_id == 0:
            bare = (
                current_event_by_node.get(id(node.func)) is None and
                current_epoch_by_node.get(id(node.func), 0) == 0
            )
        else:
            bare = (
                name_id not in scopes[scope_id]["locals"] and
                name_id not in scopes[0]["locals"] and
                name_id in free_allowed[scope_id] and
                not module_poison_all
            )
        # taint one unbounded namespace builtin
        if bare and node.func.id in ("exec", "eval", "globals", "locals") and has_subprocess_binding:
            surface_tainted = True
        # taint one exact setattr or delattr receiver
        if bare and node.func.id in ("setattr", "delattr") and node.args:
            receiver = resolved.get(id(node.args[0]))
            if receiver is not None and receiver[0][0] == "surface" and (
                proof_canonical_surface(receiver[0][1]) or receiver[0][1] == PROOF_PIPE
            ):
                surface_tainted = True

    apis = {
        PROOF_RUN: "run", PROOF_CALL: "call_check_call",
        PROOF_CHECK_CALL: "call_check_call",
        PROOF_CHECK_OUTPUT: "check_output", PROOF_POPEN: "Popen",
    }
    allowed_keywords = {
        "run": {"check", "input", "capture_output", "cwd", "text", "stdout", "stderr", "shell"},
        "call_check_call": {"cwd", "text", "stdout", "stderr", "shell"},
        "check_output": {"input", "cwd", "text", "stderr", "shell"},
        "Popen": {"cwd", "text", "stdout", "stderr", "shell"},
    }

    # validate one closed subprocess keyword envelope
    def valid_envelope(call, api):
        # require one ordinary positional vector only
        if len(call.args) != 1 or isinstance(call.args[0], ast.Starred):
            return False
        seen = set()
        values = {}
        # validate every explicit keyword
        for keyword in call.keywords:
            # reject expansion duplicate or unknown names
            if keyword.arg is None or keyword.arg in seen or keyword.arg not in allowed_keywords[api]:
                return False
            seen.add(keyword.arg)
            values[keyword.arg] = keyword.value
        # require shell false when present
        if "shell" in values and not (
            isinstance(values["shell"], ast.Constant) and values["shell"].value is False
        ):
            return False
        # require exact boolean keyword values
        for name in ("check", "capture_output", "text"):
            if name in values and not (
                isinstance(values[name], ast.Constant) and isinstance(values[name].value, bool)
            ):
                return False
        # require inert scalar or name values
        for name in ("input", "cwd"):
            if name in values and not isinstance(values[name], (ast.Constant, ast.Name)):
                return False
        # require canonical PIPE reads
        for name in ("stdout", "stderr"):
            if name in values:
                cell = resolved.get(id(values[name]))
                if cell is None or cell[0] != ("surface", PROOF_PIPE):
                    return False
        # reject conflicting output ownership
        if (
            "capture_output" in values and
            isinstance(values["capture_output"], ast.Constant) and
            values["capture_output"].value is True and
            ("stdout" in values or "stderr" in values)
        ):
            return False
        return True

    # inspect every reviewed call after whole-source taint is final
    for node in records:
        # skip non-call nodes
        if not isinstance(node, ast.Call):
            continue
        callee_cell = resolved.get(id(node.func))
        callee = callee_cell[0][1] if callee_cell is not None and callee_cell[0][0] == "surface" else PROOF_NONE
        # mark one exact importlib specifier
        if callee == PROOF_IMPORT_MODULE and node.args:
            argument = node.args[0]
            if isinstance(argument, ast.Constant) and isinstance(argument.value, str):
                roles[id(argument)] = "import-specifier"
        # require one exact subprocess API authority
        if callee not in apis or surface_tainted or not valid_envelope(node, apis.get(callee)):
            continue
        vector = node.args[0]
        # require one literal closed argv vector
        if not isinstance(vector, (ast.List, ast.Tuple)):
            continue
        elements = vector.elts
        direct_shape = len(elements) == 3
        module_shape = len(elements) == 4
        # require one exact reviewed argv spelling
        if direct_shape:
            shape_valid = (
                isinstance(elements[0], ast.Constant) and elements[0].value == "node" and
                isinstance(elements[1], ast.Constant) and elements[1].value == "-e"
            )
            payload = elements[2]
        elif module_shape:
            shape_valid = (
                isinstance(elements[0], ast.Constant) and elements[0].value == "node" and
                isinstance(elements[1], ast.Constant) and elements[1].value == "--input-type=module" and
                isinstance(elements[2], ast.Constant) and elements[2].value == "-e"
            )
            payload = elements[3]
        else:
            shape_valid = False
            payload = None
        # reject one malformed active argv
        if not shape_valid or payload is None:
            continue
        literal = payload if isinstance(payload, ast.Constant) else None
        # resolve one exact bound string payload
        if isinstance(payload, ast.Name):
            payload_cell = resolved.get(id(payload))
            literal = payload_cell[0][1] if payload_cell is not None and payload_cell[0][0] == "literal" else None
        # mark one proven string payload only
        if isinstance(literal, ast.Constant) and isinstance(literal.value, str):
            roles[id(literal)] = "embedded-javascript"
    return roles

# signal one reviewed helper bound
class LiteralBound(Exception):
    pass

# encode one source tree
def encode_tree(tree):
    roles = literal_roles(tree)
    nodes = []
    unsupported = False
    literal_bound = False

    # emit one compact positioned node
    def emit(node, depth=0):
        global emitted_node_count
        nonlocal literal_bound, unsupported
        # enforce the nesting bound
        if depth > 256:
            raise LiteralBound()
        kind = kind_by_type.get(type(node))
        # reject an unlisted reached kind
        if kind is None:
            unsupported = True
            return None
        emitted_node_count += 1
        # enforce the running normalized-node bound
        if emitted_node_count > 250000:
            raise LiteralBound()
        ordinal = len(nodes)
        nodes.append(None)
        # retain null module positions only
        if isinstance(node, ast.Module):
            positions = [None, None, None, None]
            if node.type_ignores:
                unsupported = True
        else:
            positions = [getattr(node, "lineno", None), getattr(node, "col_offset", None), getattr(node, "end_lineno", None), getattr(node, "end_col_offset", None)]
            # reject absent ends
            if any(value is None for value in positions):
                unsupported = True
        identifier = None
        import_level = None
        literal_kind = None
        literal_role = "none"
        encoded_literal = None
        operators = []
        associations = []
        children = []

        # encode closed scalar fields
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            identifier = node.name
            if getattr(node, "type_comment", None) is not None or getattr(node, "type_params", []):
                unsupported = True
        elif isinstance(node, ast.arg):
            identifier = node.arg
            if node.type_comment is not None:
                unsupported = True
        elif isinstance(node, ast.ImportFrom):
            identifier = node.module
            import_level = node.level
        elif isinstance(node, ast.alias):
            identifier = json.dumps([node.name, node.asname], ensure_ascii=False, separators=(",", ":"))
        elif isinstance(node, ast.Assign):
            if node.type_comment is not None:
                unsupported = True
        elif isinstance(node, ast.AnnAssign):
            identifier = "assignment.simple" if node.simple == 1 else "assignment.qualified"
        elif isinstance(node, (ast.Name, ast.Attribute)):
            identifier = node.id if isinstance(node, ast.Name) else node.attr
            operators = [token_by_type[type(node.ctx)]]
        elif isinstance(node, ast.keyword):
            identifier = node.arg
        elif isinstance(node, ast.Constant):
            if node.kind not in (None, "u"):
                unsupported = True
            identifier = "literal-prefix.unicode" if node.kind == "u" else None
            literal_kind, encoded_literal = literal_value(node.value)
            literal_role = roles.get(id(node), "semantic-constant")
        elif isinstance(node, (ast.For, ast.AsyncFor, ast.With, ast.AsyncWith)):
            if getattr(node, "type_comment", None) is not None:
                unsupported = True
        elif isinstance(node, ast.ExceptHandler):
            identifier = node.name
        elif isinstance(node, (ast.BoolOp, ast.BinOp, ast.AugAssign, ast.UnaryOp)):
            operators = [token_by_type[type(node.op)]]
        elif isinstance(node, ast.Compare):
            operators = [token_by_type[type(operator)] for operator in node.ops]
        elif isinstance(node, (ast.Subscript, ast.List, ast.Tuple, ast.Starred)):
            operators = [token_by_type[type(node.ctx)]]
        elif isinstance(node, ast.FormattedValue):
            identifier = conversion_by_value.get(node.conversion)
            if identifier is None:
                unsupported = True
        elif hasattr(ast, "Interpolation") and isinstance(node, ast.Interpolation):
            identifier = conversion_by_value.get(node.conversion)
            if identifier is None:
                unsupported = True
            if node.str is not None:
                literal_kind = "string"
                literal_role = "semantic-constant"
                encoded_literal = node.str

        # measure one emitted literal carrier once
        if encoded_literal is not None:
            limit = 262144 if literal_role == "embedded-javascript" else 4096
            # retain one closed literal bound failure
            if len(encoded_literal.encode("utf-8")) > limit:
                literal_bound = True

        # emit one ordinary child
        def child(field, index, child_node):
            # skip absent optional children
            if child_node is None:
                return
            child_ordinal = emit(child_node, depth + 1)
            # retain one successful reference
            if child_ordinal is not None:
                children.append([field, index, child_ordinal])

        # flatten one arguments record
        def arguments(value):
            positional = list(value.posonlyargs) + list(value.args)
            default_start = len(positional) - len(value.defaults)
            # retain positional-only parameters
            for index, parameter in enumerate(value.posonlyargs):
                primary = emit(parameter, depth + 1)
                default = value.defaults[index - default_start] if index >= default_start else None
                secondary = emit(default, depth + 1) if default is not None else None
                associations.append(["parameter", index, None, "positional-only", primary, secondary, None])
            # retain positional parameters
            for index, parameter in enumerate(value.args):
                primary = emit(parameter, depth + 1)
                combined = len(value.posonlyargs) + index
                default = value.defaults[combined - default_start] if combined >= default_start else None
                secondary = emit(default, depth + 1) if default is not None else None
                associations.append(["parameter", index, None, "positional", primary, secondary, None])
            primary = emit(value.vararg, depth + 1) if value.vararg is not None else None
            associations.append(["parameter", 0, None, "vararg", primary, None, None])
            # retain keyword-only parameters
            for index, parameter in enumerate(value.kwonlyargs):
                primary = emit(parameter, depth + 1)
                default = value.kw_defaults[index]
                secondary = emit(default, depth + 1) if default is not None else None
                associations.append(["parameter", index, None, "keyword-only", primary, secondary, None])
            primary = emit(value.kwarg, depth + 1) if value.kwarg is not None else None
            associations.append(["parameter", 0, None, "kwarg", primary, None, None])

        # flatten callable arguments first
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.Lambda)):
            arguments(node.args)
        # flatten with items
        if isinstance(node, (ast.With, ast.AsyncWith)):
            for index, item in enumerate(node.items):
                primary = emit(item.context_expr, depth + 1)
                secondary = emit(item.optional_vars, depth + 1) if item.optional_vars is not None else None
                associations.append(["with-item", index, None, "context", primary, secondary, None])
        # flatten comprehension clauses
        if isinstance(node, (ast.ListComp, ast.SetComp, ast.DictComp, ast.GeneratorExp)):
            for index, generator in enumerate(node.generators):
                primary = emit(generator.target, depth + 1)
                secondary = emit(generator.iter, depth + 1)
                associations.append(["generator", index, None, "target-iterator", primary, secondary, generator.is_async])
                for subindex, predicate in enumerate(generator.ifs):
                    associations.append(["generator-filter", index, subindex, "predicate", emit(predicate, depth + 1), None, None])

        fields = []
        # project ordinary ast children explicitly
        if isinstance(node, ast.Module): fields = [("body", node.body)]
        elif isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)): fields = [("body", node.body), ("decorator_list", node.decorator_list), ("returns", node.returns)]
        elif isinstance(node, ast.Lambda): fields = [("body", node.body)]
        elif isinstance(node, ast.arg): fields = [("type", node.annotation)]
        elif isinstance(node, (ast.Import, ast.ImportFrom)): fields = [("names", node.names)]
        elif isinstance(node, ast.Assign): fields = [("targets", node.targets), ("value", node.value)]
        elif isinstance(node, ast.AnnAssign): fields = [("target", node.target), ("type", node.annotation), ("value", node.value)]
        elif isinstance(node, (ast.AugAssign, ast.NamedExpr)): fields = [("target", node.target), ("value", node.value)]
        elif isinstance(node, ast.Attribute): fields = [("value", node.value)]
        elif isinstance(node, ast.Call): fields = [("func", node.func), ("args", node.args), ("keywords", node.keywords)]
        elif isinstance(node, ast.keyword): fields = [("value", node.value)]
        elif isinstance(node, (ast.If, ast.IfExp)): fields = [("test", node.test), ("body", node.body), ("orelse", node.orelse)]
        elif isinstance(node, (ast.For, ast.AsyncFor)): fields = [("target", node.target), ("iter", node.iter), ("body", node.body), ("orelse", node.orelse)]
        elif isinstance(node, ast.While): fields = [("test", node.test), ("body", node.body), ("orelse", node.orelse)]
        elif isinstance(node, ast.Try): fields = [("body", node.body), ("handlers", node.handlers), ("orelse", node.orelse), ("finalbody", node.finalbody)]
        elif isinstance(node, ast.ExceptHandler): fields = [("type", node.type), ("body", node.body)]
        elif isinstance(node, (ast.With, ast.AsyncWith)): fields = [("body", node.body)]
        elif isinstance(node, ast.Return): fields = [("value", node.value)]
        elif isinstance(node, ast.Raise): fields = [("exc", node.exc), ("cause", node.cause)]
        elif isinstance(node, ast.BoolOp): fields = [("values", node.values)]
        elif isinstance(node, ast.BinOp): fields = [("left", node.left), ("right", node.right)]
        elif isinstance(node, ast.UnaryOp): fields = [("operand", node.operand)]
        elif isinstance(node, ast.Compare): fields = [("left", node.left), ("comparators", node.comparators)]
        elif isinstance(node, ast.Subscript): fields = [("value", node.value), ("slice", node.slice)]
        elif isinstance(node, ast.Slice): fields = [("lower", node.lower), ("upper", node.upper), ("step", node.step)]
        elif isinstance(node, (ast.List, ast.Tuple, ast.Set)): fields = [("elts", node.elts)]
        elif isinstance(node, ast.Dict): fields = [("keys", node.keys), ("values", node.values)]
        elif isinstance(node, ast.Starred): fields = [("value", node.value)]
        elif isinstance(node, (ast.ListComp, ast.SetComp, ast.GeneratorExp)): fields = [("elt", node.elt)]
        elif isinstance(node, ast.DictComp): fields = [("key", node.key), ("value", node.value)]
        elif isinstance(node, (ast.Await, ast.Yield, ast.YieldFrom, ast.Expr)): fields = [("value", node.value)]
        elif isinstance(node, (ast.JoinedStr,)) or (hasattr(ast, "TemplateStr") and isinstance(node, ast.TemplateStr)): fields = [("values", node.values)]
        elif isinstance(node, ast.FormattedValue) or (hasattr(ast, "Interpolation") and isinstance(node, ast.Interpolation)): fields = [("value", node.value), ("format_spec", node.format_spec)]
        elif not isinstance(node, (ast.alias, ast.Name, ast.Constant, ast.Pass, ast.Break, ast.Continue)):
            unsupported = True
        # emit ordinary fields in order
        for field, value in fields:
            # emit one list field
            if isinstance(value, list):
                for index, child_node in enumerate(value):
                    child(field, index, child_node)
            else:
                child(field, 0, value)
        nodes[ordinal] = [ordinal, kind, *positions, identifier, import_level, literal_kind, literal_role, encoded_literal, operators, associations, children]
        return ordinal

    emit(tree)
    # preserve unsupported syntax ahead of literal bounds
    if unsupported:
        return []
    # stop before encoding one oversized response literal
    if literal_bound:
        raise LiteralBound()
    return nodes

# validate the exact private request
request = json.load(sys.stdin)
if type(request) is not dict or list(request.keys()) != ["files", "requestVersion"] or request["requestVersion"] != "weather-backend-python-helper-request/v1" or type(request["files"]) is not list:
    raise RuntimeError("request")
response_files = []
# parse every explicit source without execution
for file_record in request["files"]:
    if type(file_record) is not dict or list(file_record.keys()) != ["path", "rawSha256", "sourceText"]:
        raise RuntimeError("file")
    source_text = file_record["sourceText"]
    if hashlib.sha256(source_text.encode("utf-8")).hexdigest() != file_record["rawSha256"]:
        raise RuntimeError("hash")
    try:
        tree = ast.parse(source_text, filename=file_record["path"], mode="exec", type_comments=False, feature_version=None)
        nodes = encode_tree(tree)
    except LiteralBound:
        raise SystemExit(86)
    except (SyntaxError, ValueError):
        raise SystemExit(1) from None
    response_files.append({"nodes": nodes, "path": file_record["path"], "rawSha256": file_record["rawSha256"]})
response = {"analysisProfile": profile, "files": response_files, "responseVersion": "weather-backend-python-helper-response/v1"}
encoder = json.JSONEncoder(ensure_ascii=False, separators=(",", ":"), sort_keys=True)
output_bytes = 0
# preflight canonical chunks under the exact output budget
try:
    for chunk in encoder.iterencode(response):
        encoded_length = len(chunk.encode("utf-8"))
        # fail before writing an oversized response
        if encoded_length > 33554432 - output_bytes:
            raise LiteralBound()
        output_bytes += encoded_length
except LiteralBound:
    raise SystemExit(86)
# write one independently encoded bounded response
for chunk in encoder.iterencode(response):
    sys.stdout.write(chunk)
`;

const HELPER_KINDS = new Set([
  "module", "function", "async-function", "lambda", "parameter", "import",
  "import-from", "alias", "assign", "annotated-assign", "augmented-assign",
  "named-expression", "name", "attribute", "call", "keyword", "constant",
  "if", "conditional-expression", "for", "async-for", "while", "try", "except",
  "with", "async-with", "return", "raise", "bool-operator", "binary-operator",
  "unary-operator", "compare", "subscript", "slice", "list", "tuple", "set",
  "dict", "starred", "list-comprehension", "set-comprehension",
  "dict-comprehension", "generator-expression", "await", "yield", "yield-from",
  "joined-string", "formatted-value", "template-string", "interpolation",
  "expression-statement", "pass", "break", "continue",
]);
const OPERATOR_TOKENS = new Set([
  "context.load", "context.store", "context.delete", "operator.and", "operator.or",
  "operator.add", "operator.subtract", "operator.multiply", "operator.matrix-multiply",
  "operator.true-divide", "operator.floor-divide", "operator.modulo", "operator.power",
  "operator.left-shift", "operator.right-shift", "operator.bitwise-or",
  "operator.bitwise-xor", "operator.bitwise-and", "operator.invert", "operator.not",
  "operator.unary-plus", "operator.unary-minus", "operator.equal", "operator.not-equal",
  "operator.less-than", "operator.less-than-or-equal", "operator.greater-than",
  "operator.greater-than-or-equal", "operator.is", "operator.is-not", "operator.in",
  "operator.not-in",
]);
const CHILD_ROLES = new Set([
  "args", "body", "cause", "comparators", "decorator_list", "elt", "elts",
  "exc", "finalbody", "format_spec", "func", "handlers", "iter", "key", "keys",
  "keywords", "left", "lower", "names", "operand", "orelse", "returns", "right",
  "slice", "step", "target", "targets", "test", "type", "type_params", "upper",
  "value", "values",
]);
const FACT_KIND_BY_HELPER_KIND = new Map([
  ["function", "function"], ["async-function", "async-function"], ["lambda", "lambda"],
  ["parameter", "parameter"], ["import", "import"], ["import-from", "import-from"],
  ["alias", "alias"], ["assign", "binding"], ["annotated-assign", "binding"],
  ["augmented-assign", "binding"], ["named-expression", "binding"], ["name", "name"],
  ["attribute", "attribute"], ["call", "call"], ["keyword", "keyword"],
  ["constant", "constant"], ["if", "control"], ["conditional-expression", "control"],
  ["for", "control"], ["async-for", "control"], ["while", "control"],
  ["try", "control"], ["except", "control"], ["with", "control"],
  ["async-with", "control"], ["pass", "control"], ["break", "control"],
  ["continue", "control"], ["return", "return"], ["raise", "control"],
  ["bool-operator", "operator"], ["binary-operator", "operator"],
  ["unary-operator", "operator"], ["compare", "operator"],
  ["subscript", "expression"], ["slice", "expression"], ["list", "expression"],
  ["tuple", "expression"], ["set", "expression"], ["dict", "expression"],
  ["starred", "expression"], ["list-comprehension", "expression"],
  ["set-comprehension", "expression"], ["dict-comprehension", "expression"],
  ["generator-expression", "expression"], ["await", "expression"],
  ["yield", "expression"], ["yield-from", "expression"],
  ["joined-string", "expression"], ["formatted-value", "expression"],
  ["template-string", "expression"], ["interpolation", "expression"],
  ["expression-statement", "control"],
]);
const NULL_IDENTIFIER_KINDS = new Set([
  "module", "lambda", "import", "assign", "augmented-assign",
  "named-expression", "call", "if", "conditional-expression", "for",
  "async-for", "while", "try", "with", "async-with", "return", "raise",
  "bool-operator", "binary-operator", "unary-operator", "compare",
  "subscript", "slice", "list", "tuple", "set", "dict", "starred",
  "list-comprehension", "set-comprehension", "dict-comprehension",
  "generator-expression", "await", "yield", "yield-from", "joined-string",
  "template-string", "expression-statement", "pass", "break", "continue",
]);
const REQUIRED_IDENTIFIER_KINDS = new Set([
  "function", "async-function", "parameter", "name", "attribute",
]);
const CONTEXT_KINDS = new Set([
  "name", "attribute", "subscript", "list", "tuple", "starred",
]);
const SINGLE_OPERATOR_KINDS = new Set([
  "augmented-assign", "bool-operator", "binary-operator", "unary-operator",
]);
const COMPARISON_OPERATORS = new Set([
  "operator.equal", "operator.not-equal", "operator.less-than",
  "operator.less-than-or-equal", "operator.greater-than",
  "operator.greater-than-or-equal", "operator.is", "operator.is-not",
  "operator.in", "operator.not-in",
]);
const CHILD_ROLES_BY_KIND = new Map([
  ["module", ["body"]],
  ["function", ["body", "decorator_list", "returns"]],
  ["async-function", ["body", "decorator_list", "returns"]],
  ["lambda", ["body"]],
  ["parameter", ["type"]],
  ["import", ["names"]],
  ["import-from", ["names"]],
  ["assign", ["targets", "value"]],
  ["annotated-assign", ["target", "type", "value"]],
  ["augmented-assign", ["target", "value"]],
  ["named-expression", ["target", "value"]],
  ["attribute", ["value"]],
  ["call", ["func", "args", "keywords"]],
  ["keyword", ["value"]],
  ["if", ["test", "body", "orelse"]],
  ["conditional-expression", ["test", "body", "orelse"]],
  ["for", ["target", "iter", "body", "orelse"]],
  ["async-for", ["target", "iter", "body", "orelse"]],
  ["while", ["test", "body", "orelse"]],
  ["try", ["body", "handlers", "orelse", "finalbody"]],
  ["except", ["type", "body"]],
  ["with", ["body"]],
  ["async-with", ["body"]],
  ["return", ["value"]],
  ["raise", ["exc", "cause"]],
  ["bool-operator", ["values"]],
  ["binary-operator", ["left", "right"]],
  ["unary-operator", ["operand"]],
  ["compare", ["left", "comparators"]],
  ["subscript", ["value", "slice"]],
  ["slice", ["lower", "upper", "step"]],
  ["list", ["elts"]],
  ["tuple", ["elts"]],
  ["set", ["elts"]],
  ["dict", ["keys", "values"]],
  ["starred", ["value"]],
  ["list-comprehension", ["elt"]],
  ["set-comprehension", ["elt"]],
  ["generator-expression", ["elt"]],
  ["dict-comprehension", ["key", "value"]],
  ["await", ["value"]],
  ["yield", ["value"]],
  ["yield-from", ["value"]],
  ["expression-statement", ["value"]],
  ["joined-string", ["values"]],
  ["template-string", ["values"]],
  ["formatted-value", ["value", "format_spec"]],
  ["interpolation", ["value", "format_spec"]],
]);
const CHILD_CARDINALITY_BY_KIND = new Map([
  ["module", { body: [0, Infinity, "list"] }],
  ["function", {
    body: [1, Infinity, "list"],
    decorator_list: [0, Infinity, "list"],
    returns: [0, 1, "scalar"],
  }],
  ["async-function", {
    body: [1, Infinity, "list"],
    decorator_list: [0, Infinity, "list"],
    returns: [0, 1, "scalar"],
  }],
  ["lambda", { body: [1, 1, "scalar"] }],
  ["parameter", { type: [0, 1, "scalar"] }],
  ["import", { names: [1, Infinity, "list"] }],
  ["import-from", { names: [1, Infinity, "list"] }],
  ["assign", {
    targets: [1, Infinity, "list"],
    value: [1, 1, "scalar"],
  }],
  ["annotated-assign", {
    target: [1, 1, "scalar"],
    type: [1, 1, "scalar"],
    value: [0, 1, "scalar"],
  }],
  ["augmented-assign", {
    target: [1, 1, "scalar"],
    value: [1, 1, "scalar"],
  }],
  ["named-expression", {
    target: [1, 1, "scalar"],
    value: [1, 1, "scalar"],
  }],
  ["attribute", { value: [1, 1, "scalar"] }],
  ["call", {
    func: [1, 1, "scalar"],
    args: [0, Infinity, "list"],
    keywords: [0, Infinity, "list"],
  }],
  ["keyword", { value: [1, 1, "scalar"] }],
  ["if", {
    test: [1, 1, "scalar"],
    body: [1, Infinity, "list"],
    orelse: [0, Infinity, "list"],
  }],
  ["conditional-expression", {
    test: [1, 1, "scalar"],
    body: [1, 1, "scalar"],
    orelse: [1, 1, "scalar"],
  }],
  ["for", {
    target: [1, 1, "scalar"],
    iter: [1, 1, "scalar"],
    body: [1, Infinity, "list"],
    orelse: [0, Infinity, "list"],
  }],
  ["async-for", {
    target: [1, 1, "scalar"],
    iter: [1, 1, "scalar"],
    body: [1, Infinity, "list"],
    orelse: [0, Infinity, "list"],
  }],
  ["while", {
    test: [1, 1, "scalar"],
    body: [1, Infinity, "list"],
    orelse: [0, Infinity, "list"],
  }],
  ["try", {
    body: [1, Infinity, "list"],
    handlers: [0, Infinity, "list"],
    orelse: [0, Infinity, "list"],
    finalbody: [0, Infinity, "list"],
  }],
  ["except", {
    type: [0, 1, "scalar"],
    body: [1, Infinity, "list"],
  }],
  ["with", { body: [1, Infinity, "list"] }],
  ["async-with", { body: [1, Infinity, "list"] }],
  ["return", { value: [0, 1, "scalar"] }],
  ["raise", {
    exc: [0, 1, "scalar"],
    cause: [0, 1, "scalar"],
  }],
  ["bool-operator", { values: [2, Infinity, "list"] }],
  ["binary-operator", {
    left: [1, 1, "scalar"],
    right: [1, 1, "scalar"],
  }],
  ["unary-operator", { operand: [1, 1, "scalar"] }],
  ["compare", {
    left: [1, 1, "scalar"],
    comparators: [1, Infinity, "list"],
  }],
  ["subscript", {
    value: [1, 1, "scalar"],
    slice: [1, 1, "scalar"],
  }],
  ["slice", {
    lower: [0, 1, "scalar"],
    upper: [0, 1, "scalar"],
    step: [0, 1, "scalar"],
  }],
  ["list", { elts: [0, Infinity, "list"] }],
  ["tuple", { elts: [0, Infinity, "list"] }],
  ["set", { elts: [1, Infinity, "list"] }],
  ["dict", {
    keys: [0, Infinity, "nullable-list"],
    values: [0, Infinity, "list"],
  }],
  ["starred", { value: [1, 1, "scalar"] }],
  ["list-comprehension", { elt: [1, 1, "scalar"] }],
  ["set-comprehension", { elt: [1, 1, "scalar"] }],
  ["generator-expression", { elt: [1, 1, "scalar"] }],
  ["dict-comprehension", {
    key: [1, 1, "scalar"],
    value: [1, 1, "scalar"],
  }],
  ["await", { value: [1, 1, "scalar"] }],
  ["yield", { value: [0, 1, "scalar"] }],
  ["yield-from", { value: [1, 1, "scalar"] }],
  ["expression-statement", { value: [1, 1, "scalar"] }],
  ["joined-string", { values: [0, Infinity, "list"] }],
  ["template-string", { values: [0, Infinity, "list"] }],
  ["formatted-value", {
    value: [1, 1, "scalar"],
    format_spec: [0, 1, "scalar"],
  }],
  ["interpolation", {
    value: [1, 1, "scalar"],
    format_spec: [0, 1, "scalar"],
  }],
]);

// hash one exact byte sequence
function sha256(value) {
  return createHash("sha256").update(value).digest("hex");
}

// emit canonical json chunks iteratively
function canonicalJsonChunks(value) {
  const chunks = [];
  const active = new Set();
  const stack = [{ kind: "value", value }];

  // consume one explicit serialization stack
  while (stack.length > 0) {
    const frameValue = stack.pop();

    // append one fixed token
    if (frameValue.kind === "token") {
      chunks.push(frameValue.value);
      continue;
    }
    // release one completed container
    if (frameValue.kind === "leave") {
      active.delete(frameValue.value);
      continue;
    }
    const current = frameValue.value;

    // preserve scalar spelling
    if (current === null || typeof current !== "object") {
      const encoded = JSON.stringify(current);

      // reject values outside canonical json
      if (typeof encoded !== "string") {
        throw new TypeError("canonical json value is invalid");
      }
      chunks.push(encoded);
      continue;
    }
    // reject cyclic internal values
    if (active.has(current)) {
      throw new TypeError("canonical json cycle is invalid");
    }
    active.add(current);
    stack.push({ kind: "leave", value: current });

    // preserve semantic array order
    if (Array.isArray(current)) {
      stack.push({ kind: "token", value: "]" });

      // push array members in reverse source order
      for (let index = current.length - 1; index >= 0; index -= 1) {
        stack.push({ kind: "value", value: current[index] });

        // separate adjacent members
        if (index > 0) {
          stack.push({ kind: "token", value: "," });
        }
      }
      stack.push({ kind: "token", value: "[" });
      continue;
    }
    const keys = Object.keys(current).sort(compareCanonicalKey);
    stack.push({ kind: "token", value: "}" });

    // push object members in reverse key order
    for (let index = keys.length - 1; index >= 0; index -= 1) {
      const key = keys[index];
      stack.push({ kind: "value", value: current[key] });
      stack.push({ kind: "token", value: ":" });
      stack.push({ kind: "token", value: JSON.stringify(key) });

      // separate adjacent members
      if (index > 0) {
        stack.push({ kind: "token", value: "," });
      }
    }
    stack.push({ kind: "token", value: "{" });
  }
  return chunks;
}

// canonicalize one json value
function canonicalJson(value) {
  return canonicalJsonChunks(value).join("");
}

// frame one canonical identity
function frame(domain, payload) {
  const hash = createHash("sha256");
  hash.update(domain, "utf8");
  hash.update(Buffer.from([0]));

  // hash canonical chunks without a second aggregate buffer
  for (const chunk of canonicalJsonChunks(payload)) {
    hash.update(chunk, "utf8");
  }
  return hash.digest("hex");
}

// compare two text values by code point
function compareText(left, right) {
  const leftPoints = [...left];
  const rightPoints = [...right];
  const length = Math.min(leftPoints.length, rightPoints.length);

  // compare every scalar value in order
  for (let index = 0; index < length; index += 1) {
    const leftPoint = leftPoints[index].codePointAt(0);
    const rightPoint = rightPoints[index].codePointAt(0);

    // order one unequal scalar pair
    if (leftPoint !== rightPoint) {
      return leftPoint < rightPoint ? -1 : 1;
    }
  }
  return leftPoints.length - rightPoints.length;
}

// compare canonical object keys by utf-16 units
function compareCanonicalKey(left, right) {
  // order unequal canonical keys
  if (left < right) {
    return -1;
  }
  // order unequal canonical keys
  if (left > right) {
    return 1;
  }
  return 0;
}

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

// read one dense ordinary array
function denseArray(value, label) {
  // require the built-in array shape
  if (!Array.isArray(value) || Object.getPrototypeOf(value) !== Array.prototype) {
    throw new TypeError(`${label} must be an array`);
  }
  const ownKeys = Reflect.ownKeys(value);

  // reject extra properties and holes
  if (ownKeys.length !== value.length + 1 || !ownKeys.includes("length")) {
    throw new TypeError(`${label} array shape is invalid`);
  }
  const result = [];

  // read every explicit data element
  for (let index = 0; index < value.length; index += 1) {
    const descriptor = Object.getOwnPropertyDescriptor(value, String(index));

    // reject holes and accessors
    if (
      descriptor === undefined ||
      descriptor.enumerable !== true ||
      !("value" in descriptor)
    ) {
      throw new TypeError(`${label} array shape is invalid`);
    }
    result.push(descriptor.value);
  }
  return result;
}

// locate one physical repository directory
function repositoryDirectory(path) {
  const separator = path.lastIndexOf("/");
  return separator === -1 ? "" : path.slice(0, separator);
}

// recognize one contained repository path
function isRepositoryPath(value) {
  // reject unsafe outer path forms
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.startsWith("/") ||
    value.endsWith("/") ||
    value.includes("\\") ||
    value.includes("\0") ||
    value.includes("%") ||
    /^[A-Za-z][A-Za-z0-9+.-]*:/u.test(value)
  ) {
    return false;
  }
  const segments = value.split("/");

  // reject aliases and empty segments
  for (const segment of segments) {
    // reject one unsafe segment
    if (segment.length === 0 || segment === "." || segment === "..") {
      return false;
    }
  }
  return true;
}

// validate one source origin list
function validateOrigins(value) {
  const origins = denseArray(value, "source origins");

  // require at least one origin
  if (origins.length === 0) {
    throw new TypeError("source origins must not be empty");
  }
  let previous = null;

  // validate sorted unique origin values
  for (const origin of origins) {
    // reject unknown or repeated origins
    if (
      typeof origin !== "string" ||
      !ORIGINS.has(origin) ||
      (previous !== null && compareText(previous, origin) >= 0)
    ) {
      throw new TypeError("source origins are invalid");
    }
    previous = origin;
  }
  return origins;
}

// validate one private source input
function validatePrivateSource(value, allowDiscoveredLanguage = false) {
  const source = exactDataRecord(value, PRIVATE_SOURCE_KEYS, "private source");
  const origins = validateOrigins(source.origins);

  // require a supported non-module language
  if (
    !SUPPORTED_LANGUAGES.includes(source.language) &&
    !(allowDiscoveredLanguage && source.language === "javascript")
  ) {
    throw new TypeError("private source language is invalid");
  }
  // require exact textual source custody
  if (
    typeof source.sourceText !== "string" ||
    !validUnicode(source.sourceText) ||
    source.sourceText.includes("\0") ||
    typeof source.rawSha256 !== "string" ||
    !SHA256_PATTERN.test(source.rawSha256)
  ) {
    throw new TypeError("private source bytes are invalid");
  }
  const sourceBytes = Buffer.from(source.sourceText, "utf8");

  // require the supplied exact-byte hash
  if (sha256(sourceBytes) !== source.rawSha256) {
    throw new TypeError("private source hash is invalid");
  }
  // enforce the applicable per-source bound
  if (
    sourceBytes.length > PER_FILE_BYTE_CAP ||
    (source.sourceKind === "derived" && sourceBytes.length > DERIVED_SOURCE_BYTE_CAP)
  ) {
    throw new TypeError("private source byte bound exceeded");
  }
  // require a contained resolution base
  if (
    typeof source.resolutionBase !== "string" ||
    (source.resolutionBase !== "" && !isRepositoryPath(source.resolutionBase))
  ) {
    throw new TypeError("private source resolution base is invalid");
  }

  let containerSpan = null;

  // validate one physical source variant
  if (source.sourceKind === "physical") {
    // require physical path and null derived custody
    if (
      !isRepositoryPath(source.path) ||
      (source.language === "sql" && !source.path.endsWith(".sql")) ||
      source.resolutionBase !== repositoryDirectory(source.path) ||
      source.containerSpan !== null ||
      source.derivedId !== null ||
      source.parentPath !== null ||
      source.parentRawSha256 !== null
    ) {
      throw new TypeError("physical source custody is invalid");
    }
  } else if (source.sourceKind === "derived") {
    const span = exactDataRecord(
      source.containerSpan,
      ["endUtf16", "startUtf16"],
      "derived container span",
    );

    // require valid derived custody fields
    if (
      typeof source.derivedId !== "string" ||
      !SHA256_PATTERN.test(source.derivedId) ||
      source.path !== `derived:${source.derivedId}` ||
      typeof source.parentPath !== "string" ||
      (!isRepositoryPath(source.parentPath) &&
        !/^derived:[0-9a-f]{64}$/u.test(source.parentPath)) ||
      typeof source.parentRawSha256 !== "string" ||
      !SHA256_PATTERN.test(source.parentRawSha256) ||
      !Number.isSafeInteger(span.startUtf16) ||
      !Number.isSafeInteger(span.endUtf16) ||
      span.startUtf16 < 0 ||
      span.endUtf16 < span.startUtf16
    ) {
      throw new TypeError("derived source custody is invalid");
    }
    containerSpan = {
      endUtf16: span.endUtf16,
      startUtf16: span.startUtf16,
    };
  } else {
    throw new TypeError("private source kind is invalid");
  }

  return {
    byteLength: sourceBytes.length,
    containerSpan,
    derivedId: source.derivedId,
    language: source.language,
    origins,
    parentPath: source.parentPath,
    parentRawSha256: source.parentRawSha256,
    path: source.path,
    rawSha256: source.rawSha256,
    resolutionBase: source.resolutionBase,
    sourceKind: source.sourceKind,
    sourceText: source.sourceText,
  };
}

// identify one complete validated source record
function completeSourceKey(source) {
  return canonicalJson({
    containerSpan: source.containerSpan,
    derivedId: source.derivedId,
    language: source.language,
    origins: source.origins,
    parentPath: source.parentPath,
    parentRawSha256: source.parentRawSha256,
    path: source.path,
    rawSha256: source.rawSha256,
    resolutionBase: source.resolutionBase,
    sourceKind: source.sourceKind,
    sourceText: source.sourceText,
  });
}

// deduplicate one fully validated input sequence
function deduplicateInputSources(sources) {
  const unique = [];
  const byLocator = new Map();

  // validate sorted order before removing exact duplicates
  for (let index = 0; index < sources.length; index += 1) {
    const source = sources[index];

    // reject one caller ordering reversal
    if (index > 0 && compareSources(sources[index - 1], source) > 0) {
      throw new TypeError("frontend sources must be sorted");
    }
    const completeKey = completeSourceKey(source);
    const existing = byLocator.get(source.path);

    // reject one locator with conflicting complete custody
    if (existing !== undefined && existing.completeKey !== completeKey) {
      throw new Error("binding.source-conflict");
    }
    // omit one exact complete duplicate
    if (existing !== undefined) {
      continue;
    }
    byLocator.set(source.path, { completeKey, source });
    unique.push(source);
  }
  return unique;
}

// validate all cross-source derived custody
function validateDerivedCustody(sources) {
  const byPath = new Map();
  const derived = sources.filter(
    // retain explicit derived inputs
    (source) => source.sourceKind === "derived",
  );
  // enforce explicit aggregate derived caps
  if (
    derived.length > DERIVED_SOURCE_COUNT_CAP ||
    derived.reduce(
      // count one derived payload
      (total, source) => total + source.byteLength,
      0,
    ) > DERIVED_TOTAL_BYTE_CAP
  ) {
    throw bootstrapError("frontend.bound-exceeded");
  }
  // index every exact source locator
  for (const source of sources) {
    // reject one conflicting locator defensively
    if (byPath.has(source.path)) {
      throw new Error("binding.source-conflict");
    }
    byPath.set(source.path, source);
  }
  const intervalsByParent = new Map();
  // validate every explicit lineage
  for (const source of derived) {
    const expectedId = frame("weather-backend-derived-source/v1", {
      containerSpan: source.containerSpan,
      language: source.language,
      parentPath: source.parentPath,
      parentRawSha256: source.parentRawSha256,
      rawSha256: source.rawSha256,
      resolutionBase: source.resolutionBase,
    });
    // require one exact derived identity
    if (source.derivedId !== expectedId) {
      throw new Error("binding.source-conflict");
    }
    const parent = byPath.get(source.parentPath);
    // require one exact in-batch parent
    if (
      parent === undefined ||
      parent.rawSha256 !== source.parentRawSha256 ||
      source.resolutionBase !== parent.resolutionBase
    ) {
      throw new Error("binding.source-conflict");
    }
    // require scalar-aligned contained offsets
    if (
      source.containerSpan.endUtf16 > parent.sourceText.length ||
      /[\uD800-\uDBFF]/u.test(parent.sourceText[source.containerSpan.startUtf16 - 1] ?? "") ||
      /[\uD800-\uDBFF]/u.test(parent.sourceText[source.containerSpan.endUtf16 - 1] ?? "")
    ) {
      throw new Error("binding.source-conflict");
    }
    const intervals = intervalsByParent.get(parent) ?? [];
    intervals.push(source);
    intervalsByParent.set(parent, intervals);
    let depth = 1;
    let ancestor = parent;
    const seen = new Set([source.path]);
    // follow one bounded derived lineage
    while (ancestor.sourceKind === "derived") {
      // reject one cycle or excess depth
      if (seen.has(ancestor.path) || depth >= 4) {
        throw new Error("binding.source-conflict");
      }
      seen.add(ancestor.path);
      depth += 1;
      ancestor = byPath.get(ancestor.parentPath);
      // require one complete ancestor chain
      if (ancestor === undefined) {
        throw new Error("binding.source-conflict");
      }
    }
  }
  // sweep each exact parent interval group once
  for (const intervals of intervalsByParent.values()) {
    intervals.sort(
      // order one interval by its exact numeric coordinates
      (left, right) =>
        left.containerSpan.startUtf16 - right.containerSpan.startUtf16 ||
        left.containerSpan.endUtf16 - right.containerSpan.endUtf16,
    );
    let priorMaximumEnd = -1;
    let cursor = 0;

    // consume equal-start groups without rescanning prior claims
    while (cursor < intervals.length) {
      const start = intervals[cursor].containerSpan.startUtf16;
      let groupEnd = cursor;
      let groupMaximumEnd = start;
      let nonemptyCount = 0;
      let previousEnd = -1;

      // validate one complete equal-start group
      while (
        groupEnd < intervals.length &&
        intervals[groupEnd].containerSpan.startUtf16 === start
      ) {
        const end = intervals[groupEnd].containerSpan.endUtf16;

        // reject one repeated exact interval
        if (groupEnd > cursor && end === previousEnd) {
          throw new Error("binding.source-conflict");
        }
        // count one nonempty interval
        if (end > start) {
          nonemptyCount += 1;
        }
        // reject same-start nonempty overlap
        if (nonemptyCount > 1) {
          throw new Error("binding.source-conflict");
        }
        groupMaximumEnd = Math.max(groupMaximumEnd, end);
        previousEnd = end;
        groupEnd += 1;
      }
      // reject overlap with any strictly earlier start
      if (priorMaximumEnd > start) {
        throw new Error("binding.source-conflict");
      }
      priorMaximumEnd = Math.max(priorMaximumEnd, groupMaximumEnd);
      cursor = groupEnd;
    }
  }
  return { byPath, intervalsByParent };
}

// admit newly discovered derived descendants
function admitDiscoveredSources(inputSources, discoveredValues) {
  const admitted = [...inputSources];
  const byPath = new Map(inputSources.map(
    // retain one exact existing locator
    (source) => [source.path, {
      completeKey: completeSourceKey(source),
      source,
    }],
  ));
  const discovered = [];

  // validate every newly discovered private record
  for (const value of discoveredValues) {
    const source = validatePrivateSource(value, true);

    // require descendant-only discovery
    if (source.sourceKind !== "derived") {
      throw new Error("binding.source-conflict");
    }
    const completeKey = completeSourceKey(source);
    const existing = byPath.get(source.path);

    // omit one already admitted exact descendant
    if (existing !== undefined && existing.completeKey === completeKey) {
      continue;
    }
    // reject one reused locator
    if (existing !== undefined) {
      throw new Error("binding.source-conflict");
    }
    byPath.set(source.path, { completeKey, source });
    admitted.push(source);
    discovered.push(source);
  }
  validateDerivedCustody(admitted);
  discovered.sort(compareSources);
  return discovered.map(
    // remove internal byte accounting from the private dto
    (source) => ({
      containerSpan: source.containerSpan,
      derivedId: source.derivedId,
      language: source.language,
      origins: [...source.origins],
      parentPath: source.parentPath,
      parentRawSha256: source.parentRawSha256,
      path: source.path,
      rawSha256: source.rawSha256,
      resolutionBase: source.resolutionBase,
      sourceKind: source.sourceKind,
      sourceText: source.sourceText,
    }),
  );
}

// compare two validated batch inputs
function compareSources(left, right) {
  return compareText(
    `${left.language}\0${left.path}\0${left.rawSha256}`,
    `${right.language}\0${right.path}\0${right.rawSha256}`,
  );
}

// recursively freeze one detached result
function freezeRecursively(value) {
  // stop at scalars and frozen objects
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) {
    return value;
  }
  const visited = new Set();
  const stack = [{ expanded: false, value }];

  // freeze every child before its parent iteratively
  while (stack.length > 0) {
    const entry = stack.pop();

    // freeze one fully expanded object
    if (entry.expanded) {
      Object.freeze(entry.value);
      continue;
    }
    // skip one repeated or already frozen object
    if (visited.has(entry.value) || Object.isFrozen(entry.value)) {
      continue;
    }
    visited.add(entry.value);
    stack.push({ expanded: true, value: entry.value });

    // schedule every mutable child first
    for (const child of Object.values(entry.value)) {
      // retain object children only
      if (child !== null && typeof child === "object" && !Object.isFrozen(child)) {
        stack.push({ expanded: false, value: child });
      }
    }
  }
  return value;
}

// create one sanitized frontend gap
function unsupportedDiagnostic() {
  return {
    code: "frontend.unsupported",
    dimension: "frontend",
    ids: [],
    messageId: "frontend.unsupported",
    severity: "gap",
    sourceSiteId: null,
  };
}

// append values without argument expansion
function appendValues(target, values, start = 0) {
  // append every retained value in order
  for (let index = start; index < values.length; index += 1) {
    target.push(values[index]);
  }
}

// reduce one numeric sequence without argument expansion
function minimumValue(values, fallback) {
  let minimum = fallback;
  // retain exact pairwise numeric minimum semantics
  for (const value of values) {
    minimum = Math.min(minimum, value);
  }
  return minimum;
}

// append diagnostics under the frozen aggregate cap
function appendDiagnostics(target, values) {
  // fail before one aggregate overflow
  if (values.length > DIAGNOSTIC_COUNT_CAP - target.length) {
    throw bootstrapError("frontend.bound-exceeded");
  }
  appendValues(target, values);
}

// create one sanitized bootstrap error
function bootstrapError(code = "frontend.invalid-output") {
  const error = new Error(code);
  error.code = code;
  error.stage = "frontend-bootstrap";
  return error;
}

// reject unpaired unicode surrogate text
function validUnicode(value) {
  return typeof value === "string" &&
    !/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(^|[^\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(value);
}

// validate one fixed capability record
function validateRuntime(value) {
  const capability = exactDataRecord(
    value,
    ["astFeatures", "implementation", "spanEncoding", "version"],
    "python capability",
  );
  const astFeatures = denseArray(capability.astFeatures, "python ast features");
  const baseFeatures = ["end-positions", "joined-string", "formatted-value"];
  const fullFeatures = [...baseFeatures, "template-string", "interpolation"];
  const expectedFeatures = astFeatures.length === 3 ? baseFeatures : fullFeatures;
  // require exact capability values and order
  if (
    astFeatures.length !== expectedFeatures.length ||
    astFeatures.some(
      // detect one reordered capability
      (feature, index) => feature !== expectedFeatures[index],
    ) ||
    capability.implementation !== "CPython" ||
    capability.spanEncoding !== "utf8-byte-columns-exclusive-end" ||
    typeof capability.version !== "string" ||
    !PYTHON_VERSION_PATTERN.test(capability.version)
  ) {
    throw new TypeError("python capability is invalid");
  }
  return {
    astFeatures: [...astFeatures],
    implementation: capability.implementation,
    spanEncoding: capability.spanEncoding,
    version: capability.version,
  };
}

// validate one helper identifier
function validateHelperIdentifier(kind, value) {
  // validate one canonical alias pair
  if (kind === "alias") {
    // reject nonstring alias payloads
    if (!validUnicode(value)) {
      throw new TypeError("alias identifier is invalid");
    }
    const pair = denseArray(JSON.parse(value), "alias pair");
    // require exact nested canonical bytes
    if (
      pair.length !== 2 ||
      !validUnicode(pair[0]) ||
      (pair[1] !== null && !validUnicode(pair[1])) ||
      JSON.stringify(pair) !== value
    ) {
      throw new TypeError("alias identifier is invalid");
    }
    return value;
  }
  // validate one formatted conversion
  if (kind === "formatted-value" || kind === "interpolation") {
    // require one exact conversion string
    if (![
      "conversion.none",
      "conversion.string",
      "conversion.repr",
      "conversion.ascii",
    ].includes(value)) {
      throw new TypeError("conversion identifier is invalid");
    }
    return value;
  }
  // validate one annotated assignment scalar
  if (kind === "annotated-assign") {
    // require the closed assignment vocabulary
    if (value !== "assignment.simple" && value !== "assignment.qualified") {
      throw new TypeError("assignment identifier is invalid");
    }
    return value;
  }
  // validate one constant prefix scalar
  if (kind === "constant") {
    // admit only the exact unicode prefix marker
    if (value !== null && value !== "literal-prefix.unicode") {
      throw new TypeError("constant identifier is invalid");
    }
    return value;
  }
  // require null-only identifier kinds
  if (NULL_IDENTIFIER_KINDS.has(kind)) {
    // reject one invented scalar
    if (value !== null) {
      throw new TypeError("helper identifier is invalid");
    }
    return null;
  }
  // require named identifier kinds
  if (REQUIRED_IDENTIFIER_KINDS.has(kind)) {
    // reject one absent or invalid name
    if (!validUnicode(value) || value.length === 0) {
      throw new TypeError("helper identifier is invalid");
    }
    return value;
  }
  // admit exact nullable dynamic identifiers
  if (["import-from", "keyword", "except"].includes(kind)) {
    // reject one invalid dynamic name
    if (value !== null && (!validUnicode(value) || value.length === 0)) {
      throw new TypeError("helper identifier is invalid");
    }
    return value;
  }
  // admit reviewed nullable identifiers
  if (value === null) {
    return null;
  }
  // require unicode scalar identifier text
  if (!validUnicode(value)) {
    throw new TypeError("helper identifier is invalid");
  }
  return value;
}

// validate one exact encoded literal scalar
function validateLiteralEncoding(kind, value) {
  // validate each closed literal family
  if (kind === "none") {
    return value === "null";
  }
  // validate one boolean spelling
  if (kind === "boolean") {
    return value === "true" || value === "false";
  }
  // validate one minimal integer spelling
  if (kind === "integer") {
    return /^(?:0|-[1-9][0-9]*|[1-9][0-9]*)$/u.test(value);
  }
  // validate one exact binary float spelling
  if (kind === "float") {
    return /^(?:nan|inf|-inf|-?0x[0-9a-f]+\.[0-9a-f]+p[+-][0-9]+)$/u.test(value);
  }
  // validate one exact complex pair
  if (kind === "complex") {
    const component = "(?:nan|inf|-inf|-?0x[0-9a-f]+\\.[0-9a-f]+p[+-][0-9]+)";
    return new RegExp(`^\\(${component},${component}\\)$`, "u").test(value);
  }
  // validate one exact decoded string
  if (kind === "string") {
    return validUnicode(value);
  }
  // validate one lowercase byte sequence
  if (kind === "bytes") {
    return /^(?:[0-9a-f]{2})*$/u.test(value);
  }
  // validate one ellipsis spelling
  if (kind === "ellipsis") {
    return value === "...";
  }
  return false;
}

// validate one kind-specific operator vector
function validateHelperOperators(kind, operators) {
  // require one exact context token
  if (CONTEXT_KINDS.has(kind)) {
    return operators.length === 1 && [
      "context.load", "context.store", "context.delete",
    ].includes(operators[0]);
  }
  // require one exact operator token
  if (SINGLE_OPERATOR_KINDS.has(kind)) {
    if (operators.length !== 1) {
      return false;
    }
    // bind boolean operators to their closed family
    if (kind === "bool-operator") {
      return operators[0] === "operator.and" || operators[0] === "operator.or";
    }
    // bind unary operators to their closed family
    if (kind === "unary-operator") {
      return [
        "operator.invert", "operator.not", "operator.unary-plus",
        "operator.unary-minus",
      ].includes(operators[0]);
    }
    return operators[0].startsWith("operator.") &&
      !COMPARISON_OPERATORS.has(operators[0]) &&
      !["operator.and", "operator.or", "operator.invert", "operator.not",
        "operator.unary-plus", "operator.unary-minus"].includes(operators[0]);
  }
  // require comparison tokens in source order
  if (kind === "compare") {
    return operators.length > 0 && operators.every(
      // accept one comparison member
      (operator) => COMPARISON_OPERATORS.has(operator),
    );
  }
  return operators.length === 0;
}

// validate one kind-specific child vector
function validateHelperChildren(kind, children, operators) {
  const allowed = CHILD_ROLES_BY_KIND.get(kind) ?? [];
  const cardinalities = CHILD_CARDINALITY_BY_KIND.get(kind) ?? {};
  const grouped = new Map(allowed.map(
    // create one role bucket
    (role) => [role, []],
  ));
  let previousRoleIndex = -1;
  let previousIndex = -1;
  // validate ordered fields and source-list indexes
  for (const child of children) {
    const roleIndex = allowed.indexOf(child[0]);
    // reject one impossible or reordered field
    if (roleIndex < previousRoleIndex || roleIndex < 0) {
      return false;
    }
    // require strictly increasing indexes within one field
    if (
      (roleIndex === previousRoleIndex && child[1] <= previousIndex) ||
      (roleIndex !== previousRoleIndex && child[1] < 0)
    ) {
      return false;
    }
    grouped.get(child[0]).push(child);
    previousRoleIndex = roleIndex;
    previousIndex = child[1];
  }
  // validate every role cardinality and index form
  for (const role of allowed) {
    const entries = grouped.get(role);
    const [minimum, maximum, indexMode] = cardinalities[role];

    // enforce one exact role count
    if (entries.length < minimum || entries.length > maximum) {
      return false;
    }
    // require scalar index zero
    if (indexMode === "scalar" && entries.some(
      // locate one nonzero scalar index
      (entry) => entry[1] !== 0,
    )) {
      return false;
    }
    // require contiguous ordinary list indexes
    if (indexMode === "list" && entries.some(
      // locate one missing or shifted list index
      (entry, index) => entry[1] !== index,
    )) {
      return false;
    }
  }
  const counts = Object.fromEntries([...grouped].map(
    // retain one role count
    ([role, entries]) => [role, entries.length],
  ));

  // align comparison operators and operands
  if (kind === "compare" && operators.length !== counts.comparators) {
    return false;
  }
  // require one exception path for try
  if (
    kind === "try" &&
    ((counts.handlers === 0 && counts.finalbody === 0) ||
      (counts.orelse > 0 && counts.handlers === 0))
  ) {
    return false;
  }
  // require a raised exception before a cause
  if (kind === "raise" && counts.cause > 0 && counts.exc === 0) {
    return false;
  }
  // align nullable dict keys to exact value slots
  if (kind === "dict") {
    const valueIndexes = new Set(grouped.get("values").map(
      // retain one value slot
      (entry) => entry[1],
    ));

    // reject a key without one aligned value
    if (grouped.get("keys").some(
      // locate one impossible nullable key slot
      (entry) => !valueIndexes.has(entry[1]),
    )) {
      return false;
    }
  }
  return true;
}

// validate one compact helper tuple
function validateHelperNode(value) {
  const tuple = denseArray(value, "helper node");
  // require the exact tuple width
  if (tuple.length !== 14) {
    throw new TypeError("helper tuple width is invalid");
  }
  const [
    ordinal,
    kind,
    startLine,
    startByteColumn,
    endLine,
    endByteColumn,
    rawIdentifier,
    importLevel,
    literalKind,
    literalRole,
    literalValue,
    rawOperators,
    rawAssociations,
    rawChildren,
  ] = tuple;
  // require safe nonnegative ordinals and closed kinds
  if (!Number.isSafeInteger(ordinal) || ordinal < 0 || !HELPER_KINDS.has(kind)) {
    throw new TypeError("helper node identity is invalid");
  }
  // require null module positions only
  if (kind === "module") {
    // reject positioned modules
    if ([startLine, startByteColumn, endLine, endByteColumn].some(
      // detect one nonnull module position
      (position) => position !== null,
    )) {
      throw new TypeError("module span is invalid");
    }
  } else if (
    !Number.isSafeInteger(startLine) ||
    !Number.isSafeInteger(startByteColumn) ||
    !Number.isSafeInteger(endLine) ||
    !Number.isSafeInteger(endByteColumn) ||
    startLine < 1 ||
    endLine < 1 ||
    startByteColumn < 0 ||
    endByteColumn < 0
  ) {
    throw new TypeError("helper span is invalid");
  }
  const identifier = validateHelperIdentifier(kind, rawIdentifier);
  // require import levels only on import-from
  if (
    (kind === "import-from" && (!Number.isSafeInteger(importLevel) || importLevel < 0)) ||
    (kind !== "import-from" && importLevel !== null)
  ) {
    throw new TypeError("helper import level is invalid");
  }
  const literalKinds = ["none", "boolean", "integer", "float", "complex", "string", "bytes", "ellipsis"];
  const carryingRoles = ["semantic-constant", "import-specifier", "embedded-javascript"];
  // require closed literal custody
  if (
    (literalKind !== null && !literalKinds.includes(literalKind)) ||
    !["none", ...carryingRoles].includes(literalRole) ||
    (literalValue !== null && !validUnicode(literalValue)) ||
    (literalRole === "none" && (literalKind !== null || literalValue !== null)) ||
    (carryingRoles.includes(literalRole) && (literalKind === null || literalValue === null)) ||
    (kind === "constant" && !carryingRoles.includes(literalRole)) ||
    (kind !== "constant" && kind !== "interpolation" && literalRole !== "none") ||
    (literalValue !== null && !validateLiteralEncoding(literalKind, literalValue)) ||
    (kind === "interpolation" && literalRole !== "none" &&
      (literalKind !== "string" || literalRole !== "semantic-constant")) ||
    (identifier === "literal-prefix.unicode" && literalKind !== "string")
  ) {
    throw new TypeError("helper literal custody is invalid");
  }
  // enforce private decoded literal bounds
  if (literalValue !== null) {
    const limit = literalRole === "embedded-javascript"
      ? DERIVED_SOURCE_BYTE_CAP
      : STRING_LITERAL_BYTE_CAP;
    // fail one oversized literal without leakage
    if (Buffer.byteLength(literalValue, "utf8") > limit) {
      throw bootstrapError("frontend.bound-exceeded");
    }
  }
  const operators = denseArray(rawOperators, "helper operators");
  // validate every closed operator token
  for (const operator of operators) {
    // reject one unknown operator token
    if (!OPERATOR_TOKENS.has(operator)) {
      throw new TypeError("helper operator is invalid");
    }
  }
  // require the exact per-kind operator vector
  if (!validateHelperOperators(kind, operators)) {
    throw new TypeError("helper operator shape is invalid");
  }
  const associations = denseArray(rawAssociations, "helper associations").map(
    // detach one structural association
    (associationValue) => {
      const association = denseArray(associationValue, "helper association");
      // require one exact tuple width and nonnegative index
      if (
        association.length !== 7 ||
        !Number.isSafeInteger(association[1]) ||
        association[1] < 0
      ) {
        throw new TypeError("helper association is invalid");
      }
      const [associationKind, index, subindex, role, primary, secondary, isAsync] = association;
      // require one exact parameter tuple
      if (associationKind === "parameter") {
        const nullablePrimary = role === "vararg" || role === "kwarg";
        // reject one invalid parameter slot
        if (
          !["positional-only", "positional", "vararg", "keyword-only", "kwarg"].includes(role) ||
          subindex !== null ||
          isAsync !== null ||
          (nullablePrimary
            ? primary !== null && (!Number.isSafeInteger(primary) || primary < 0)
            : !Number.isSafeInteger(primary) || primary < 0) ||
          (secondary !== null && (!Number.isSafeInteger(secondary) || secondary < 0))
        ) {
          throw new TypeError("helper association is invalid");
        }
      } else if (associationKind === "with-item") {
        // require one exact with-item tuple
        if (
          subindex !== null || role !== "context" ||
          !Number.isSafeInteger(primary) || primary < 0 ||
          (secondary !== null && (!Number.isSafeInteger(secondary) || secondary < 0)) ||
          isAsync !== null
        ) {
          throw new TypeError("helper association is invalid");
        }
      } else if (associationKind === "generator") {
        // require one exact generator tuple
        if (
          subindex !== null || role !== "target-iterator" ||
          !Number.isSafeInteger(primary) || primary < 0 ||
          !Number.isSafeInteger(secondary) || secondary < 0 ||
          (isAsync !== 0 && isAsync !== 1)
        ) {
          throw new TypeError("helper association is invalid");
        }
      } else if (associationKind === "generator-filter") {
        // require one exact generator-filter tuple
        if (
          !Number.isSafeInteger(subindex) || subindex < 0 || role !== "predicate" ||
          !Number.isSafeInteger(primary) || primary < 0 ||
          secondary !== null || isAsync !== null
        ) {
          throw new TypeError("helper association is invalid");
        }
      } else {
        throw new TypeError("helper association is invalid");
      }
      return [...association];
    },
  );
  const children = denseArray(rawChildren, "helper children").map(
    // detach one ordinary child edge
    (childValue) => {
      const child = denseArray(childValue, "helper child");
      // require exact child width and values
      if (
        child.length !== 3 ||
        !CHILD_ROLES.has(child[0]) ||
        !Number.isSafeInteger(child[1]) ||
        !Number.isSafeInteger(child[2]) ||
        child[1] < 0 ||
        child[2] < 0
      ) {
        throw new TypeError("helper child is invalid");
      }
      return [...child];
    },
  );
  // require the exact per-kind child sequence
  if (!validateHelperChildren(kind, children, operators)) {
    throw new TypeError("helper child shape is invalid");
  }
  return {
    associations,
    children,
    endByteColumn,
    endLine,
    identifier,
    importLevel,
    kind,
    literalKind,
    literalRole,
    literalValue,
    operators: [...operators],
    ordinal,
    startByteColumn,
    startLine,
  };
}

// validate one complete rooted helper tree
function validateHelperTree(nodes) {
  // admit one unsupported empty projection
  if (nodes.length === 0) {
    return;
  }
  // require one module root only
  if (nodes[0].kind !== "module" || nodes.some(
    // locate one extra module
    (node, index) => index > 0 && node.kind === "module",
  )) {
    throw new TypeError("helper module root is invalid");
  }
  const incoming = new Array(nodes.length).fill(0);
  const edges = nodes.map(() => []);
  // validate every parent-specific relation
  for (const node of nodes) {
    // validate one function association sequence
    if (["function", "async-function", "lambda"].includes(node.kind)) {
      const ranks = new Map([
        ["positional-only", 0], ["positional", 1], ["vararg", 2],
        ["keyword-only", 3], ["kwarg", 4],
      ]);
      let previousRank = -1;
      let previousIndex = -1;
      let varargCount = 0;
      let kwargCount = 0;
      let positionalDefaultSeen = false;
      // validate every parameter slot
      for (const association of node.associations) {
        const rank = ranks.get(association[3]);
        // reject one wrong association kind or order
        if (
          association[0] !== "parameter" || rank === undefined ||
          rank < previousRank ||
          (rank === previousRank && association[1] !== previousIndex + 1) ||
          (rank !== previousRank && association[1] !== 0)
        ) {
          throw new TypeError("helper parameter order is invalid");
        }
        // require parameter primary nodes
        if (association[4] !== null && nodes[association[4]]?.kind !== "parameter") {
          throw new TypeError("helper parameter target is invalid");
        }
        // require nullable singleton slots
        if (association[3] === "vararg") {
          varargCount += 1;
        }
        // require nullable singleton slots
        if (association[3] === "kwarg") {
          kwargCount += 1;
        }
        // require positional defaults on the right edge
        if (["positional-only", "positional"].includes(association[3])) {
          // reject one missing default after defaults begin
          if (positionalDefaultSeen && association[5] === null) {
            throw new TypeError("helper parameter defaults are invalid");
          }
          // retain the first positional default
          if (association[5] !== null) {
            positionalDefaultSeen = true;
          }
        }
        previousRank = rank;
        previousIndex = association[1];
      }
      // require both explicit nullable singleton slots
      if (varargCount !== 1 || kwargCount !== 1) {
        throw new TypeError("helper parameter slots are invalid");
      }
    } else if (["with", "async-with"].includes(node.kind)) {
      // require ordered with-item relations only
      if (node.associations.length === 0 || node.associations.some(
        // reject one wrong with relation
        (association, index) => association[0] !== "with-item" || association[1] !== index,
      )) {
        throw new TypeError("helper with-item order is invalid");
      }
    } else if ([
      "list-comprehension", "set-comprehension", "dict-comprehension",
      "generator-expression",
    ].includes(node.kind)) {
      let generatorIndex = -1;
      let filterIndex = -1;
      // require generator clauses and filters in native order
      for (const association of node.associations) {
        // advance one generator clause
        if (association[0] === "generator") {
          if (association[1] !== generatorIndex + 1) {
            throw new TypeError("helper generator order is invalid");
          }
          generatorIndex = association[1];
          filterIndex = -1;
        } else if (
          association[0] !== "generator-filter" ||
          association[1] !== generatorIndex ||
          association[2] !== filterIndex + 1
        ) {
          throw new TypeError("helper generator filter order is invalid");
        } else {
          filterIndex = association[2];
        }
      }
      // require one generator clause
      if (generatorIndex < 0) {
        throw new TypeError("helper generator order is invalid");
      }
    } else if (node.associations.length !== 0) {
      throw new TypeError("helper association parent is invalid");
    }

    const targets = [];

    // preserve helper association emission order first
    for (const association of node.associations) {
      // retain one primary association target
      if (association[4] !== null) {
        targets.push(association[4]);
      }
      // retain one secondary association target
      if (association[5] !== null) {
        targets.push(association[5]);
      }
    }
    // preserve ordinary field emission order second
    for (const child of node.children) {
      targets.push(child[2]);
    }
    // bind every relation into one ownership tree
    for (const target of targets) {
      // reject one missing target
      if (target >= nodes.length || target <= node.ordinal) {
        throw new TypeError("helper relation target is invalid");
      }
      incoming[target] += 1;
      edges[node.ordinal].push(target);
    }
  }
  // require exactly one incoming relation outside the root
  if (incoming[0] !== 0 || incoming.slice(1).some(
    // reject one shared or unreachable node
    (count) => count !== 1,
  )) {
    throw new TypeError("helper tree ownership is invalid");
  }
  const visited = new Set();
  const stack = [{ depth: 0, ordinal: 0 }];
  let expectedOrdinal = 0;

  // walk the rooted encoder preorder iteratively
  while (stack.length > 0) {
    const entry = stack.pop();

    // enforce the reviewed helper nesting bound
    if (entry.depth > 256) {
      throw bootstrapError("frontend.bound-exceeded");
    }
    // reject shared or nonpreorder traversal
    if (visited.has(entry.ordinal) || entry.ordinal !== expectedOrdinal) {
      throw new TypeError("helper tree preorder is invalid");
    }
    visited.add(entry.ordinal);
    expectedOrdinal += 1;

    // push owned children in reverse emission order
    for (let index = edges[entry.ordinal].length - 1; index >= 0; index -= 1) {
      stack.push({
        depth: entry.depth + 1,
        ordinal: edges[entry.ordinal][index],
      });
    }
  }
  // require complete root reachability
  if (visited.size !== nodes.length) {
    throw new TypeError("helper tree reachability is invalid");
  }
}

// validate one complete helper response
function validatePythonResponse(stdout, sources) {
  // reject bytes above the fixed stdout cap
  if (Buffer.byteLength(stdout, "utf8") > PYTHON_STDOUT_CAP) {
    throw bootstrapError("frontend.bound-exceeded");
  }
  let parsed;
  // parse only the fixed response
  try {
    parsed = JSON.parse(stdout);
  } catch {
    throw bootstrapError();
  }
  try {
    const response = exactDataRecord(
      parsed,
      ["analysisProfile", "files", "responseVersion"],
      "python response",
    );
    // require the fixed protocol version
    if (response.responseVersion !== "weather-backend-python-helper-response/v1") {
      throw new TypeError("python response version is invalid");
    }
    const runtime = validateRuntime(response.analysisProfile);
    const files = denseArray(response.files, "python response files");
    // require one response file per input
    if (files.length !== sources.length) {
      throw new TypeError("python response file count is invalid");
    }
    const normalizedFiles = [];
    let totalNodes = 0;
    // validate every sorted response file
    for (let index = 0; index < files.length; index += 1) {
      const record = exactDataRecord(
        files[index],
        ["nodes", "path", "rawSha256"],
        "python response file",
      );
      const source = sources[index];
      // require exact source custody
      if (record.path !== source.path || record.rawSha256 !== source.rawSha256) {
        throw new TypeError("python response source custody is invalid");
      }
      const rawNodes = denseArray(record.nodes, "python response nodes");
      totalNodes += rawNodes.length;
      // enforce the normalized-node cap
      if (totalNodes > PYTHON_NODE_CAP) {
        throw bootstrapError("frontend.bound-exceeded");
      }
      const nodes = rawNodes.map(
        // validate one compact tuple
        (rawNode) => validateHelperNode(rawNode),
      );
      // require deterministic preorder ordinals
      for (let ordinal = 0; ordinal < nodes.length; ordinal += 1) {
        // reject one reordered or duplicate ordinal
        if (nodes[ordinal].ordinal !== ordinal) {
          throw new TypeError("helper ordinals are invalid");
        }
      }
      validateHelperTree(nodes);
      normalizedFiles.push({ nodes, source });
    }
    return { files: normalizedFiles, runtime };
  } catch (error) {
    // preserve one closed bound failure
    if (error?.code === "frontend.bound-exceeded") {
      throw error;
    }
    throw bootstrapError();
  }
}

// run one fixed python helper batch
async function analyzePythonSources(sources) {
  let requestBuffer = Buffer.from(canonicalJson({
    files: sources.map(
      // project one private helper file
      (source) => ({
        path: source.path,
        rawSha256: source.rawSha256,
        sourceText: source.sourceText,
      }),
    ),
    requestVersion: "weather-backend-python-helper-request/v1",
  }), "utf8");
  return new Promise(
    // own one complete helper lifecycle
    (resolve, reject) => {
      const causes = new Set();
      const stdoutChunks = [];
      let stdoutBytes = 0;
      let acceptStdout = true;
      let child = null;
      let phase = "prepared";
      let killRequested = false;
      let spawnObserved = false;
      let childCloseObservable = false;
      let childClosed = false;
      let literalBoundCandidate = false;
      let stdinFinished = false;
      let stdinClosed = false;
      let stdoutEnded = false;
      let stdoutClosed = false;
      let stderrEnded = false;
      let stderrClosed = false;
      let deadlineTimer = null;

      // request one terminal signal at most once
      function requestKill() {
        // stop after one request or terminal closure
        if (
          killRequested ||
          child === null ||
          childCloseObservable && childClosed
        ) {
          return;
        }
        // stop without one signal surface
        if (typeof child?.kill !== "function") {
          return;
        }
        killRequested = true;
        // contain signal failures privately
        try {
          // retain a failed control request as a lower-ranked cause
          if (child.kill("SIGKILL") === false) {
            causes.add("child-control-failure");
          }
        } catch {
          causes.add("child-control-failure");
        }
      }

      // record one closed cause token
      function addCause(cause, shouldKill = true) {
        causes.add(cause);
        phase = phase === "settled" ? phase : "closing";
        acceptStdout = false;
        stdoutChunks.length = 0;
        // signal one live child for active failures
        if (shouldKill) {
          requestKill();
        }
      }

      // test the complete child and stream close gate
      function closeGateReached() {
        return childClosed && stdinClosed && stdoutClosed && stderrClosed;
      }

      // select one deterministic observed cause
      function selectedCause() {
        const precedence = [
          "launch-failure",
          "stdout-overflow",
          "stderr-byte",
          "deadline",
          "stdin-failure",
          "stdout-failure",
          "stderr-failure",
          "child-control-failure",
          "literal-bound",
          "exit-signal",
          "exit-nonzero",
          "exit-status-missing",
        ];
        // choose the first observed ranked cause
        for (const cause of precedence) {
          // retain one present cause
          if (causes.has(cause)) {
            return cause;
          }
        }
        return null;
      }

      // settle once after every existing pipe closes
      function settle() {
        // wait for the exact close gate
        if (phase === "settled" || !closeGateReached()) {
          return;
        }
        // classify one deferred exact-86 status at the complete gate
        if (literalBoundCandidate) {
          const exactSilentBound = stdinFinished && stdoutBytes === 0 && causes.size === 0;
          addCause(exactSilentBound ? "literal-bound" : "exit-nonzero", false);
        }
        phase = "settled";
        clearTimeout(deadlineTimer);
        requestBuffer = null;
        const cause = selectedCause();
        // reject one observed process failure
        if (cause !== null) {
          stdoutChunks.length = 0;
          reject(bootstrapError(
            cause === "stdout-overflow" || cause === "literal-bound" || cause === "deadline"
              ? "frontend.bound-exceeded"
              : "frontend.invalid-output",
          ));
          return;
        }
        let stdout;
        // decode one bounded successful response strictly
        try {
          stdout = new TextDecoder("utf-8", { fatal: true }).decode(
            Buffer.concat(stdoutChunks, stdoutBytes),
          );
        } catch {
          stdoutChunks.length = 0;
          stdout = null;
          reject(bootstrapError());
          return;
        }
        stdoutChunks.length = 0;
        // validate one decoded helper response
        try {
          const response = validatePythonResponse(stdout, sources);
          stdout = null;
          resolve(response);
        } catch (error) {
          stdout = null;
          reject(bootstrapError(
            error?.code === "frontend.bound-exceeded"
              ? "frontend.bound-exceeded"
              : "frontend.invalid-output",
          ));
        }
      }

      // arm the fixed deadline immediately before launch
      deadlineTimer = setTimeout(
        // stop one overlong helper
        () => addCause("deadline"),
        10_000,
      );
      // launch the one fixed helper
      try {
        child = spawn(
          "/usr/bin/python3",
          ["-I", "-B", "-S", "-c", PYTHON_HELPER_SOURCE],
          {
            cwd: "/",
            env: { LANG: "C" },
            shell: false,
            stdio: ["pipe", "pipe", "pipe"],
          },
        );
      } catch {
        clearTimeout(deadlineTimer);
        phase = "settled";
        requestBuffer = null;
        reject(bootstrapError());
        return;
      }
      phase = "running";

      childCloseObservable = typeof child?.on === "function";
      childClosed = !childCloseObservable;
      stdinClosed = typeof child?.stdin?.on !== "function";
      stdoutClosed = typeof child?.stdout?.on !== "function";
      stderrClosed = typeof child?.stderr?.on !== "function";
      const invalidChildShape =
        child === null ||
        typeof child !== "object" ||
        child.stdin === null ||
        typeof child.stdin?.on !== "function" ||
        typeof child.stdin?.end !== "function" ||
        child.stdout === null ||
        typeof child.stdout?.on !== "function" ||
        child.stderr === null ||
        typeof child.stderr?.on !== "function" ||
        typeof child.on !== "function" ||
        typeof child.kill !== "function";

      // observe child lifecycle when available
      if (typeof child?.on === "function") {
        child.on("spawn",
          // record one successful launch
          () => {
            spawnObserved = true;
          });
        child.on("error",
          // classify one private child failure
          () => addCause(spawnObserved ? "child-control-failure" : "launch-failure"));
        child.on("close",
          // retain one terminal status and close gate
          (code, signal) => {
            // ignore duplicate close attempts
            if (childClosed) {
              return;
            }
            childClosed = true;
            // require one observed spawn
            if (!spawnObserved) {
              addCause("launch-failure", false);
            } else if (signal !== null) {
              addCause("exit-signal", false);
            } else if (code === 86) {
              // defer one possible silent helper literal bound
              literalBoundCandidate = true;
            } else if (typeof code === "number" && code !== 0) {
              addCause("exit-nonzero", false);
            } else if (typeof code !== "number") {
              addCause("exit-status-missing", false);
            }
            settle();
          });
      }

      // observe one writable request pipe
      if (child?.stdin !== null && typeof child?.stdin?.on === "function") {
        child.stdin.on("finish",
          // record one accepted request
          () => {
            stdinFinished = true;
          });
        child.stdin.on("error",
          // contain one private writable failure
          () => addCause("stdin-failure"));
        child.stdin.on("close",
          // record one writable terminal event
          () => {
            stdinClosed = true;
            // reject closure before acceptance
            if (!stdinFinished) {
              addCause("stdin-failure");
            }
            settle();
          });
      }

      // observe one bounded stdout pipe
      if (child?.stdout !== null && typeof child?.stdout?.on === "function") {
        child.stdout.on("data",
          // retain one bounded buffer chunk
          (chunk) => {
            // reject nonbuffer output
            if (!Buffer.isBuffer(chunk)) {
              addCause("stdout-failure");
              return;
            }
            // detect the first byte beyond the cap
            if (chunk.length > PYTHON_STDOUT_CAP - stdoutBytes) {
              stdoutBytes = PYTHON_STDOUT_CAP;
              addCause("stdout-overflow");
              return;
            }
            stdoutBytes += chunk.length;
            // retain only while the invocation is clean
            if (acceptStdout && chunk.length > 0) {
              stdoutChunks.push(chunk);
            }
          });
        child.stdout.on("end",
          // record one readable end
          () => {
            stdoutEnded = true;
          });
        child.stdout.on("error",
          // contain one private readable failure
          () => addCause("stdout-failure"));
        child.stdout.on("close",
          // record one readable terminal event
          () => {
            stdoutClosed = true;
            // reject closure before end
            if (!stdoutEnded) {
              addCause("stdout-failure");
            }
            settle();
          });
      }

      // observe and discard every stderr byte
      if (child?.stderr !== null && typeof child?.stderr?.on === "function") {
        child.stderr.on("data",
          // reject one stderr observation without retention
          (chunk) => {
            // classify one invalid stream value
            if (!Buffer.isBuffer(chunk)) {
              addCause("stderr-failure");
              return;
            }
            // fail on the first nonempty byte
            if (chunk.length > 0) {
              addCause("stderr-byte");
            }
          });
        child.stderr.on("end",
          // record one readable end
          () => {
            stderrEnded = true;
          });
        child.stderr.on("error",
          // contain one private readable failure
          () => addCause("stderr-failure"));
        child.stderr.on("close",
          // record one readable terminal event
          () => {
            stderrClosed = true;
            // reject closure before end without a byte witness
            if (!stderrEnded && !causes.has("stderr-byte")) {
              addCause("stderr-failure");
            }
            settle();
          });
      }

      // fail only after every observable terminal listener is attached
      if (invalidChildShape) {
        addCause("launch-failure");
      }

      // stop before writing through an invalid shape
      if (causes.has("launch-failure")) {
        settle();
        return;
      }
      // write the canonical request exactly once
      try {
        child.stdin.end(requestBuffer,
          // contain one private completion error
          (error) => {
            // reject one failed write callback
            if (error !== null && error !== undefined) {
              addCause("stdin-failure");
            }
          });
      } catch {
        addCause("stdin-failure");
      }
    },
  );
}

// build utf-8 byte to utf-16 line boundaries
function buildLineMaps(sourceText) {
  const maps = [];
  let absoluteUtf16 = 0;
  // map every physical source line
  for (const line of sourceText.split("\n")) {
    const boundaries = new Map([[0, absoluteUtf16]]);
    let byteColumn = 0;
    let utf16Column = 0;
    // advance by unicode code point
    for (const character of line) {
      byteColumn += Buffer.byteLength(character, "utf8");
      utf16Column += character.length;
      boundaries.set(byteColumn, absoluteUtf16 + utf16Column);
    }
    maps.push(boundaries);
    absoluteUtf16 += line.length + 1;
  }
  return maps;
}

// convert one native helper span to utf-16
function convertHelperSpan(node, source, lineMaps) {
  const startUtf16 = lineMaps[node.startLine - 1]?.get(node.startByteColumn);
  const endUtf16 = lineMaps[node.endLine - 1]?.get(node.endByteColumn);
  // reject absent or nonboundary positions
  if (
    !Number.isSafeInteger(startUtf16) ||
    !Number.isSafeInteger(endUtf16) ||
    endUtf16 < startUtf16 ||
    endUtf16 > source.sourceText.length
  ) {
    throw bootstrapError();
  }
  return { endUtf16, startUtf16 };
}

// build one immutable source line index
function buildOccurrenceSpanIndex(sourceText) {
  const lineStarts = [0];
  // retain every line start
  for (let index = 0; index < sourceText.length; index += 1) {
    // retain one newline boundary
    if (sourceText[index] === "\n") {
      lineStarts.push(index + 1);
    }
  }
  return Object.freeze(lineStarts);
}

// build one six-field occurrence span
function occurrenceSpan(lineStarts, startOffsetUtf16, endOffsetUtf16) {
  // locate one offset coordinate
  function locate(offset) {
    let low = 0;
    let high = lineStarts.length;
    // binary search the containing line
    while (low + 1 < high) {
      const middle = Math.floor((low + high) / 2);
      // retain one preceding line start
      if (lineStarts[middle] <= offset) {
        low = middle;
      } else {
        high = middle;
      }
    }
    return { column: offset - lineStarts[low] + 1, line: low + 1 };
  }
  const start = locate(startOffsetUtf16);
  const end = locate(endOffsetUtf16);
  return {
    endColumn: end.column,
    endLine: end.line,
    endOffsetUtf16,
    startColumn: start.column,
    startLine: start.line,
    startOffsetUtf16,
  };
}

// hash one literal-redacted expression envelope
function expressionDigest(language, normalization, value) {
  return frame("weather-clock-expression/v2", { language, normalization, value });
}

// prepare one shared python projection representation
function createPythonProjectionState(nodes, parentByOrdinal) {
  const encodedScalars = new Map();

  // intern one canonical scalar byte sequence
  function encodeScalar(value) {
    const existing = encodedScalars.get(value);

    // reuse one previously encoded scalar
    if (existing !== undefined) {
      return existing;
    }
    const encoded = JSON.stringify(value);

    // reject one impossible validated scalar
    if (typeof encoded !== "string") {
      throw bootstrapError();
    }
    const bytes = Buffer.from(encoded, "utf8");
    encodedScalars.set(value, bytes);
    return bytes;
  }

  const descriptors = new Array(nodes.length);
  const ownerStates = new Array(nodes.length);
  const descendantOwnerStates = new Array(nodes.length);
  const structuralDepths = new Uint16Array(nodes.length);
  const moduleOwner = {
    byteLength: 0,
    kind: "module",
    parent: null,
    segmentCount: 0,
    valid: true,
    value: "<module>",
  };
  const anonymousOwner = {
    byteLength: 11,
    kind: "anonymous",
    parent: null,
    segmentCount: 0,
    valid: true,
    value: "<anonymous>",
  };
  let maximumDepth = 1;

  // build exact descriptors and structural owner states once
  for (const node of nodes) {
    const operatorTokens = new Array(node.operators.length);

    // retain each ordered operator token
    for (let index = 0; index < node.operators.length; index += 1) {
      operatorTokens[index] = encodeScalar(node.operators[index]);
    }
    const childEdges = new Array(node.children.length);

    // retain each ordered child reference
    for (let index = 0; index < node.children.length; index += 1) {
      const [role, childIndex, childOrdinal] = node.children[index];
      childEdges[index] = {
        indexToken: encodeScalar(childIndex),
        ordinal: childOrdinal,
        roleToken: encodeScalar(role),
      };
    }
    descriptors[node.ordinal] = {
      children: childEdges,
      identifierToken: encodeScalar(node.identifier),
      importLevelToken: encodeScalar(node.importLevel),
      kindToken: encodeScalar(node.kind),
      literalKindToken: encodeScalar(node.literalKind),
      operators: operatorTokens,
    };
    const parentOrdinal = parentByOrdinal.get(node.ordinal);
    const inheritedOwner = parentOrdinal === undefined
      ? moduleOwner
      : descendantOwnerStates[parentOrdinal];
    ownerStates[node.ordinal] = inheritedOwner;
    structuralDepths[node.ordinal] = parentOrdinal === undefined
      ? 1
      : structuralDepths[parentOrdinal] + 1;
    maximumDepth = Math.max(maximumDepth, structuralDepths[node.ordinal]);
    let descendantOwner = inheritedOwner;

    // preserve any anonymous structural ancestor
    if (inheritedOwner.kind === "anonymous" || node.kind === "lambda") {
      descendantOwner = anonymousOwner;
    } else if (node.kind === "function" || node.kind === "async-function") {
      const separatorBytes = inheritedOwner.kind === "named" ? 1 : 0;
      const nameBytes = typeof node.identifier === "string"
        ? Buffer.byteLength(node.identifier, "utf8")
        : 193;
      const byteLength = Math.min(
        193,
        inheritedOwner.byteLength + separatorBytes + nameBytes,
      );
      descendantOwner = {
        byteLength,
        kind: "named",
        name: node.identifier,
        parent: inheritedOwner,
        segmentCount: inheritedOwner.segmentCount + 1,
        valid: inheritedOwner.valid &&
          typeof node.identifier === "string" &&
          /^[A-Za-z_$][A-Za-z0-9_$]*$/u.test(node.identifier) &&
          byteLength <= 192,
        value: null,
      };
    }
    descendantOwnerStates[node.ordinal] = descendantOwner;
  }

  const openArray = Buffer.from("[", "utf8");
  const closeArray = Buffer.from("]", "utf8");
  const closeNode = Buffer.from("]]", "utf8");
  const comma = Buffer.from(",", "utf8");
  const openOperators = Buffer.from(",[", "utf8");
  const openChildren = Buffer.from("],[", "utf8");
  const domain = Buffer.from("weather-clock-expression/v2", "utf8");
  const nul = Buffer.from([0]);
  const envelopePrefix = Buffer.from(
    '{"language":"python","normalization":"python-helper-tree/v1","value":',
    "utf8",
  );
  const envelopeClose = Buffer.from("}", "utf8");
  const ordinalStack = new Uint32Array(maximumDepth);
  const childCursorStack = new Uint32Array(maximumDepth);
  const ownerScratch = new Array(maximumDepth);

  // emit one descriptor prefix and its operators
  function emitPrefix(hash, descriptor) {
    hash.update(openArray);
    hash.update(descriptor.kindToken);
    hash.update(comma);
    hash.update(descriptor.identifierToken);
    hash.update(comma);
    hash.update(descriptor.importLevelToken);
    hash.update(comma);
    hash.update(descriptor.literalKindToken);
    hash.update(openOperators);

    // emit each exact operator in order
    for (let index = 0; index < descriptor.operators.length; index += 1) {
      // separate adjacent operator tokens
      if (index > 0) {
        hash.update(comma);
      }
      hash.update(descriptor.operators[index]);
    }
    hash.update(openChildren);
  }

  // materialize one valid used structural owner
  function owner(ordinal) {
    const state = ownerStates[ordinal];

    // suppress one invalid emitted owner
    if (state === undefined || !state.valid) {
      return null;
    }
    // reuse one fixed or memoized owner
    if (state.value !== null) {
      return state.value;
    }
    ownerScratch.length = state.segmentCount;
    let cursor = state;
    let index = state.segmentCount;

    // collect one linked owner chain in output order
    while (cursor.kind === "named") {
      index -= 1;
      ownerScratch[index] = cursor.name;
      cursor = cursor.parent;
    }
    const value = ownerScratch.join(".");

    // release reusable scratch references
    for (let clear = 0; clear < state.segmentCount; clear += 1) {
      ownerScratch[clear] = undefined;
    }
    state.value = value;
    return value;
  }

  // hash one full recursive frozen tree without rebuilding it
  function digest(rootOrdinal) {
    const root = descriptors[rootOrdinal];

    // reject one impossible validated root
    if (root === undefined) {
      throw bootstrapError();
    }
    const hash = createHash("sha256");
    hash.update(domain);
    hash.update(nul);
    hash.update(envelopePrefix);
    let depth = 0;
    ordinalStack[0] = rootOrdinal;
    childCursorStack[0] = 0;
    emitPrefix(hash, root);
    // traverse one complete child tree with reusable primitive frames
    while (depth >= 0) {
      const descriptor = descriptors[ordinalStack[depth]];
      const childCursor = childCursorStack[depth];

      // descend through one next child edge
      if (childCursor < descriptor.children.length) {
        const edge = descriptor.children[childCursor];

        // separate adjacent child edges
        if (childCursor > 0) {
          hash.update(comma);
        }
        hash.update(openArray);
        hash.update(edge.roleToken);
        hash.update(comma);
        hash.update(edge.indexToken);
        hash.update(comma);
        childCursorStack[depth] = childCursor + 1;
        depth += 1;
        ordinalStack[depth] = edge.ordinal;
        childCursorStack[depth] = 0;
        emitPrefix(hash, descriptors[edge.ordinal]);
        continue;
      }
      hash.update(closeNode);

      // close one owning child tuple
      if (depth > 0) {
        hash.update(closeArray);
      }
      depth -= 1;
    }
    hash.update(envelopeClose);
    return hash.digest("hex");
  }

  return { digest, owner };
}

// create one private semantic fact preimage
function semanticFactPreimage(node, source, lineMaps, factIds) {
  const span = convertHelperSpan(node, source, lineMaps);
  const childEdges = node.children.map(
    // replace one child ordinal with its fact id
    ([role, index, ordinal]) => ({
      childFactId: factIds.get(ordinal),
      index,
      role,
    }),
  );
  const associations = node.associations.map(
    // replace structural ordinals with fact ids
    ([kind, index, subindex, role, primary, secondary, isAsync]) => [
      kind,
      index,
      subindex,
      role,
      primary === null ? null : factIds.get(primary),
      secondary === null ? null : factIds.get(secondary),
      isAsync,
    ],
  );
  // reject unresolved references
  if (
    childEdges.some(
      // locate one missing child id
      (edge) => typeof edge.childFactId !== "string",
    ) ||
    associations.some(
      // locate one missing association id
      (association) =>
        (association[4] !== null && typeof association[4] !== "string") ||
        (association[5] !== null && typeof association[5] !== "string"),
    )
  ) {
    throw bootstrapError();
  }
  return {
    associations,
    attributes: {
      identifier: node.identifier,
      importLevel: node.importLevel,
      literalKind: node.literalKind,
      literalRole: node.literalRole,
      literalValue: node.literalValue,
      operators: [...node.operators],
    },
    childEdges,
    factKind: FACT_KIND_BY_HELPER_KIND.get(node.kind),
    language: "python",
    site: {
      endUtf16: span.endUtf16,
      path: source.path,
      rawSha256: source.rawSha256,
      startUtf16: span.startUtf16,
    },
  };
}

// resolve one dotted helper expression path
function helperPath(ordinal, nodesByOrdinal) {
  const node = nodesByOrdinal.get(ordinal);
  // resolve one name leaf
  if (node?.kind === "name") {
    return node.identifier;
  }
  // resolve one attribute chain
  if (node?.kind === "attribute") {
    const receiver = node.children.find(
      // locate one scalar receiver
      ([role, index]) => role === "value" && index === 0,
    );
    // reject one missing receiver
    if (receiver === undefined) {
      return null;
    }
    const base = helperPath(receiver[2], nodesByOrdinal);
    return base === null ? null : `${base}.${node.identifier}`;
  }
  return null;
}

// identify one python clock surface
function pythonSurface(path) {
  const descriptors = [
    ["datetime.datetime.now", "python.datetime-now", "current-time-read"],
    ["datetime.datetime.today", "python.datetime-today", "current-time-read"],
    ["datetime.date.today", "python.date-today", "current-time-read"],
    ["time.time", "python.time-time", "current-time-read"],
    ["time.monotonic", "python.time-monotonic", "relative-clock-read"],
    ["time.perf_counter", "python.time-perf-counter", "relative-clock-read"],
    ["time.process_time", "python.time-process-time", "relative-clock-read"],
  ];
  // match one exact resolved provider
  for (const [resolvedPath, surface, kind] of descriptors) {
    // retain one exact semantic binding
    if (path === resolvedPath) {
      return { kind, surface };
    }
  }
  return null;
}

// decode one validated alias identifier
function pythonAlias(node) {
  // reject one nonalias node
  if (node?.kind !== "alias") {
    return null;
  }
  const [name, asname] = JSON.parse(node.identifier);
  return { asname, name };
}

// collect one complete helper subtree
function collectPythonSubtree(root, nodesByOrdinal, result) {
  // stop one repeated structural visit
  if (result.has(root)) {
    return;
  }
  result.add(root);
  const node = nodesByOrdinal.get(root);
  // stop one absent target
  if (node === undefined) {
    return;
  }
  // visit ordinary children
  for (const child of node.children) {
    collectPythonSubtree(child[2], nodesByOrdinal, result);
  }
  // visit structural children
  for (const association of node.associations) {
    // visit one primary association
    if (association[4] !== null) {
      collectPythonSubtree(association[4], nodesByOrdinal, result);
    }
    // visit one secondary association
    if (association[5] !== null) {
      collectPythonSubtree(association[5], nodesByOrdinal, result);
    }
  }
}

// derive lexical python bindings and literal roles
function resolvePythonSemantics(nodes, nodesByOrdinal, parentByOrdinal, source, lineMaps) {
  const SURFACE_NONE = 0;
  const SURFACE_SUBPROCESS = 1;
  const SURFACE_RUN = 2;
  const SURFACE_CALL = 3;
  const SURFACE_CHECK_CALL = 4;
  const SURFACE_CHECK_OUTPUT = 5;
  const SURFACE_POPEN = 6;
  const SURFACE_PIPE = 7;
  const SURFACE_MEMBER = 8;
  const SURFACE_APIS = new Set([
    SURFACE_RUN, SURFACE_CALL, SURFACE_CHECK_CALL,
    SURFACE_CHECK_OUTPUT, SURFACE_POPEN,
  ]);
  const surfaceNameIds = new Map();
  const surfaceNames = [];
  const pathSegmentIds = new Map();
  const pathSegments = [null];
  const pathParentIds = [0];
  const pathSegmentById = [0];
  const pathDepths = [0];
  const pathChildrenByParent = [new Map()];
  const combinedTailIds = new Map();
  let nextBindingEventId = 1;
  let nextBindingValueId = 2;
  const UNKNOWN_BINDING = Object.freeze({ kind: "unknown", valueId: 1 });
  const pathBindingValues = new Map();
  const literalBindingValues = new Map();
  const bindingEventByTargetOrdinal = new Map();
  const surfaceEvents = [];
  const cells = [];
  const pendingMemberWrites = [];
  const callDemands = [];
  let canonicalBindingPresent = false;
  let moduleSurfaceWildcard = false;

  // intern one rejection-only surface name
  function internSurfaceName(name) {
    const existing = surfaceNameIds.get(name);

    // retain one existing name id
    if (existing !== undefined) {
      return existing;
    }
    const nameId = surfaceNames.length;
    surfaceNames.push(name);
    surfaceNameIds.set(name, nameId);
    return nameId;
  }

  // intern one exact path segment
  function internPathSegment(segment) {
    const existing = pathSegmentIds.get(segment);

    // retain one existing segment id
    if (existing !== undefined) {
      return existing;
    }
    const segmentId = pathSegments.length;
    pathSegments.push(segment);
    pathSegmentIds.set(segment, segmentId);
    return segmentId;
  }

  // append one exact persistent path segment
  function appendPathSegmentId(parentId, segmentId) {
    const children = pathChildrenByParent[parentId];
    const existing = children.get(segmentId);

    // retain one existing persistent path
    if (existing !== undefined) {
      return existing;
    }
    const pathId = pathParentIds.length;
    pathParentIds.push(parentId);
    pathSegmentById.push(segmentId);
    pathDepths.push(pathDepths[parentId] + 1);
    pathChildrenByParent.push(new Map());
    children.set(segmentId, pathId);
    return pathId;
  }

  // append one exact persistent path segment
  function appendPathSegment(parentId, segment) {
    return appendPathSegmentId(parentId, internPathSegment(segment));
  }

  // append dotted components while preserving empty segments
  function appendDottedPath(parentId, path) {
    let resultId = parentId;
    let start = 0;

    // consume every exact dotted component
    for (let index = 0; index <= path.length; index += 1) {
      // wait for one component boundary
      if (index < path.length && path[index] !== ".") {
        continue;
      }
      resultId = appendPathSegment(resultId, path.slice(start, index));
      start = index + 1;
    }
    return resultId;
  }

  // intern one dotted provider path
  function internDottedPath(path) {
    return appendDottedPath(0, path);
  }

  // read one first dotted component
  function firstDottedSegment(path) {
    const boundary = path.indexOf(".");
    return boundary < 0 ? path : path.slice(0, boundary);
  }

  // append one raw path tail to one resolved root
  function appendRawTail(basePathId, rawPathId) {
    let byRaw = combinedTailIds.get(basePathId);

    // allocate one sparse base cache
    if (byRaw === undefined) {
      byRaw = new Map();
      combinedTailIds.set(basePathId, byRaw);
    }
    const cached = byRaw.get(rawPathId);

    // retain one existing combined path
    if (cached !== undefined) {
      return cached;
    }
    const tail = [];
    let currentId = rawPathId;

    // exclude the raw lexical root
    while (pathParentIds[currentId] !== 0) {
      tail.push(pathSegmentById[currentId]);
      currentId = pathParentIds[currentId];
    }
    let resultId = basePathId;

    // append the tail in source order
    for (let index = tail.length - 1; index >= 0; index -= 1) {
      resultId = appendPathSegmentId(resultId, tail[index]);
    }
    byRaw.set(rawPathId, resultId);
    return resultId;
  }

  const trackedProviderPaths = [
    "datetime.datetime.now", "datetime.datetime.today", "datetime.date.today",
    "time.time", "time.monotonic", "time.perf_counter", "time.process_time",
    "importlib.import_module", "subprocess.run", "subprocess.call",
    "subprocess.check_call", "subprocess.check_output", "subprocess.Popen",
  ];
  const trackedEndpointPathIds = new Set();
  const trackedPrefixPathIds = new Set();
  // intern the fixed tracked vocabulary
  for (const path of trackedProviderPaths) {
    const endpointId = internDottedPath(path);
    trackedEndpointPathIds.add(endpointId);
    let currentId = endpointId;

    // retain every exact member prefix
    while (currentId !== 0) {
      trackedPrefixPathIds.add(currentId);
      currentId = pathParentIds[currentId];
    }
  }
  const providerStatePathIds = [...trackedPrefixPathIds];
  const providerStateIndexByPathId = new Map();
  // assign one fixed state to every tracked prefix
  for (let index = 0; index < providerStatePathIds.length; index += 1) {
    providerStateIndexByPathId.set(providerStatePathIds[index], index);
  }
  // reject one impossible fixed-vocabulary overflow
  if (providerStatePathIds.length >= 31) {
    throw bootstrapError();
  }
  const rawRequirementTransitions = [new Map()];
  const rawRequirementHistoricalMasks = [0];
  const rawRequirementWildcardMasks = [0];

  // add one exact suffix to the finite requirement trie
  function addRawRequirement(
    segments,
    startIndex,
    historicalMask,
    wildcardMask,
  ) {
    let state = 0;
    // consume every exact suffix component
    for (let index = startIndex; index < segments.length; index += 1) {
      const segmentId = internPathSegment(segments[index]);
      let nextState = rawRequirementTransitions[state].get(segmentId);

      // create one missing finite transition
      if (nextState === undefined) {
        nextState = rawRequirementTransitions.length;
        rawRequirementTransitions[state].set(segmentId, nextState);
        rawRequirementTransitions.push(new Map());
        rawRequirementHistoricalMasks.push(0);
        rawRequirementWildcardMasks.push(0);
      }
      state = nextState;
    }
    rawRequirementHistoricalMasks[state] = (
      rawRequirementHistoricalMasks[state] | historicalMask
    ) >>> 0;
    rawRequirementWildcardMasks[state] |= wildcardMask;
  }

  // index every exact endpoint and canonical-prefix suffix once
  for (let endpointIndex = 0;
    endpointIndex < trackedProviderPaths.length;
    endpointIndex += 1) {
    const segments = trackedProviderPaths[endpointIndex].split(".");
    let prefixId = 0;
    // retain every canonical provider-prefix requirement
    for (let depth = 0; depth < segments.length; depth += 1) {
      prefixId = appendPathSegment(prefixId, segments[depth]);
      const stateIndex = providerStateIndexByPathId.get(prefixId);

      // reject one fixed endpoint without its canonical state
      if (stateIndex === undefined) {
        throw bootstrapError();
      }
      addRawRequirement(segments, depth + 1, 1 << stateIndex, 0);
    }
    const wildcardMask = segments[0] === "datetime" && endpointIndex < 7
      ? 1
      : segments[0] === "time" && endpointIndex < 7
        ? 2
        : 0;

    // retain only reviewed clock wildcard expansions
    if (wildcardMask !== 0) {
      addRawRequirement(segments, 1, 0, wildcardMask);
    }
  }

  const clockDescriptorByPathId = new Map([
    [internDottedPath("datetime.datetime.now"), {
      kind: "current-time-read",
      surface: "python.datetime-now",
    }],
    [internDottedPath("datetime.datetime.today"), {
      kind: "current-time-read",
      surface: "python.datetime-today",
    }],
    [internDottedPath("datetime.date.today"), {
      kind: "current-time-read",
      surface: "python.date-today",
    }],
    [internDottedPath("time.time"), {
      kind: "current-time-read",
      surface: "python.time-time",
    }],
    [internDottedPath("time.monotonic"), {
      kind: "relative-clock-read",
      surface: "python.time-monotonic",
    }],
    [internDottedPath("time.perf_counter"), {
      kind: "relative-clock-read",
      surface: "python.time-perf-counter",
    }],
    [internDottedPath("time.process_time"), {
      kind: "relative-clock-read",
      surface: "python.time-process-time",
    }],
  ]);
  const importlibImportModulePathId = internDottedPath(
    "importlib.import_module",
  );

  // intern one exact path binding value
  function pathBinding(pathId) {
    const existing = pathBindingValues.get(pathId);

    // retain one existing exact value
    if (existing !== undefined) {
      return existing;
    }
    const value = { kind: "path", pathId, valueId: nextBindingValueId };
    nextBindingValueId += 1;
    pathBindingValues.set(pathId, value);
    return value;
  }

  // intern one literal custody value
  function literalBinding(node) {
    const existing = literalBindingValues.get(node.ordinal);

    // retain one existing literal identity
    if (existing !== undefined) {
      return existing;
    }
    const value = {
      kind: "literal",
      ordinal: node.ordinal,
      value: node.literalValue,
      valueId: nextBindingValueId,
    };
    nextBindingValueId += 1;
    literalBindingValues.set(node.ordinal, value);
    return value;
  }

  const startByOrdinal = new Int32Array(nodes.length);
  const endByOrdinal = new Int32Array(nodes.length);
  // cache every positioned coordinate once
  for (const node of nodes) {
    // skip the synthetic module span
    if (node.kind === "module") {
      continue;
    }
    const converted = convertHelperSpan(node, source, lineMaps);
    startByOrdinal[node.ordinal] = converted.startUtf16;
    endByOrdinal[node.ordinal] = converted.endUtf16;
  }

  const scopes = new Map([[0, {
    bindingDestination: 0,
    cellByNameId: new Map(),
    definitionPosition: 0,
    hasWildcard: false,
    localNames: new Set(),
    parent: null,
    surfaceBindings: new Map(),
    type: "module",
    wildcardImports: [],
    wildcardPositions: [],
  }]]);
  const scopeByOrdinal = new Uint32Array(nodes.length);
  const controlAncestorsByOrdinal = new Uint32Array(nodes.length);
  const assignedOrdinals = new Uint8Array(nodes.length);
  assignedOrdinals[0] = 1;
  const defaultOrdinals = new Set();
  const callableKinds = new Set(["function", "async-function", "lambda"]);
  const comprehensionKinds = new Set([
    "list-comprehension", "set-comprehension", "dict-comprehension",
    "generator-expression",
  ]);
  const conditionalKinds = new Set([
    "if", "conditional-expression", "for", "async-for", "while",
    "try", "except",
  ]);
  let unsupported = false;

  // create one explicit lexical scope
  function createScope(ordinal, parent, type) {
    // retain one already created scope
    if (scopes.has(ordinal)) {
      return;
    }
    const parentScope = scopes.get(parent);
    scopes.set(ordinal, {
      bindingDestination: type === "comprehension"
        ? parentScope.bindingDestination
        : ordinal,
      cellByNameId: new Map(),
      definitionPosition: endByOrdinal[ordinal],
      hasWildcard: false,
      localNames: new Set(),
      parent,
      surfaceBindings: new Map(),
      type,
      wildcardImports: [],
      wildcardPositions: [],
    });
  }

  const assignmentStack = [];
  // seed every module child in reverse order
  for (let index = nodes[0].children.length - 1; index >= 0; index -= 1) {
    assignmentStack.push({
      controlAncestors: 0,
      defaultDescendant: false,
      kind: "node",
      ordinal: nodes[0].children[index][2],
      scopeOrdinal: 0,
    });
  }
  // assign every owned edge once
  while (assignmentStack.length > 0) {
    const task = assignmentStack.pop();
    const node = nodesByOrdinal.get(task.ordinal);

    // stop one absent structural target
    if (node === undefined) {
      continue;
    }
    // retain every descendant of one default root
    if (task.defaultDescendant) {
      defaultOrdinals.add(task.ordinal);
    }
    // stop one repeated ownership edge
    if (assignedOrdinals[task.ordinal] !== 0) {
      continue;
    }
    assignedOrdinals[task.ordinal] = 1;
    scopeByOrdinal[task.ordinal] = task.scopeOrdinal;
    controlAncestorsByOrdinal[task.ordinal] = task.controlAncestors;
    const childControls = task.controlAncestors + (
      conditionalKinds.has(node.kind) ? 1 : 0
    );

    // split one parameter annotation into its enclosing scope
    if (task.kind === "parameter") {
      for (let index = node.children.length - 1; index >= 0; index -= 1) {
        assignmentStack.push({
          controlAncestors: childControls,
          defaultDescendant: task.defaultDescendant,
          kind: "node",
          ordinal: node.children[index][2],
          scopeOrdinal: task.enclosingScope,
        });
      }
      continue;
    }
    // split callable declaration evaluation from its body
    if (callableKinds.has(node.kind)) {
      createScope(node.ordinal, task.scopeOrdinal, "callable");
      const nextTasks = [];
      // retain parameter and default evaluation order
      for (const association of node.associations) {
        // retain one present parameter
        if (association[4] !== null) {
          nextTasks.push({
            controlAncestors: childControls,
            defaultDescendant: task.defaultDescendant,
            enclosingScope: task.scopeOrdinal,
            kind: "parameter",
            ordinal: association[4],
            scopeOrdinal: node.ordinal,
          });
        }
        // retain one complete default descendant tree
        if (association[5] !== null) {
          nextTasks.push({
            controlAncestors: childControls,
            defaultDescendant: true,
            kind: "node",
            ordinal: association[5],
            scopeOrdinal: task.scopeOrdinal,
          });
        }
      }
      // retain callable body and enclosing expression scopes
      for (const child of node.children) {
        nextTasks.push({
          controlAncestors: childControls,
          defaultDescendant: task.defaultDescendant,
          kind: "node",
          ordinal: child[2],
          scopeOrdinal: child[0] === "body" ? node.ordinal : task.scopeOrdinal,
        });
      }
      // preserve recursive ownership order
      for (let index = nextTasks.length - 1; index >= 0; index -= 1) {
        assignmentStack.push(nextTasks[index]);
      }
      continue;
    }
    // isolate one comprehension scope
    if (comprehensionKinds.has(node.kind)) {
      createScope(node.ordinal, task.scopeOrdinal, "comprehension");
      const nextTasks = [];
      // retain generator evaluation order
      for (const association of node.associations) {
        // split one generator target and iterator
        if (association[0] === "generator") {
          nextTasks.push({
            controlAncestors: childControls,
            defaultDescendant: task.defaultDescendant,
            kind: "node",
            ordinal: association[4],
            scopeOrdinal: node.ordinal,
          });
          nextTasks.push({
            controlAncestors: childControls,
            defaultDescendant: task.defaultDescendant,
            kind: "node",
            ordinal: association[5],
            scopeOrdinal: association[1] === 0
              ? task.scopeOrdinal
              : node.ordinal,
          });
        } else if (association[4] !== null) {
          // retain one comprehension-local association
          nextTasks.push({
            controlAncestors: childControls,
            defaultDescendant: task.defaultDescendant,
            kind: "node",
            ordinal: association[4],
            scopeOrdinal: node.ordinal,
          });
        }
      }
      // retain every produced value
      for (const child of node.children) {
        nextTasks.push({
          controlAncestors: childControls,
          defaultDescendant: task.defaultDescendant,
          kind: "node",
          ordinal: child[2],
          scopeOrdinal: node.ordinal,
        });
      }
      // preserve recursive ownership order
      for (let index = nextTasks.length - 1; index >= 0; index -= 1) {
        assignmentStack.push(nextTasks[index]);
      }
      continue;
    }
    const nextTasks = [];
    // retain every structural association
    for (const association of node.associations) {
      // retain one primary relation
      if (association[4] !== null) {
        nextTasks.push({
          controlAncestors: childControls,
          defaultDescendant: task.defaultDescendant,
          kind: "node",
          ordinal: association[4],
          scopeOrdinal: task.scopeOrdinal,
        });
      }
      // retain one secondary relation
      if (association[5] !== null) {
        nextTasks.push({
          controlAncestors: childControls,
          defaultDescendant: task.defaultDescendant,
          kind: "node",
          ordinal: association[5],
          scopeOrdinal: task.scopeOrdinal,
        });
      }
    }
    // retain every ordinary child
    for (const child of node.children) {
      nextTasks.push({
        controlAncestors: childControls,
        defaultDescendant: task.defaultDescendant,
        kind: "node",
        ordinal: child[2],
        scopeOrdinal: task.scopeOrdinal,
      });
    }
    // preserve recursive ownership order
    for (let index = nextTasks.length - 1; index >= 0; index -= 1) {
      assignmentStack.push(nextTasks[index]);
    }
  }

  const bindingScopeByOrdinal = new Uint32Array(nodes.length);
  const conditionalByOrdinal = new Uint8Array(nodes.length);
  // cache every binding destination and control state
  for (const node of nodes) {
    const evaluationScope = scopeByOrdinal[node.ordinal];
    const bindingScope = node.kind === "named-expression"
      ? scopes.get(evaluationScope).bindingDestination
      : evaluationScope;
    bindingScopeByOrdinal[node.ordinal] = bindingScope;
    conditionalByOrdinal[node.ordinal] = Number(
      controlAncestorsByOrdinal[node.ordinal] >
        controlAncestorsByOrdinal[bindingScope],
    );
  }

  const aliasesByOrdinal = new Array(nodes.length).fill(null);
  // decode every validated alias once
  for (const node of nodes) {
    // retain alias records only
    if (node.kind === "alias") {
      aliasesByOrdinal[node.ordinal] = pythonAlias(node);
    }
  }

  // declare every recursively bound target name
  function declareTarget(rootOrdinal, scopeOrdinal) {
    const targets = [rootOrdinal];

    // consume every destructuring edge once
    while (targets.length > 0) {
      const ordinal = targets.pop();
      const target = nodesByOrdinal.get(ordinal);

      // declare one direct name target
      if (target?.kind === "name") {
        scopes.get(scopeOrdinal).localNames.add(target.identifier);
        continue;
      }
      // retain nested destructuring targets
      if (["list", "tuple", "starred"].includes(target?.kind)) {
        for (let index = target.children.length - 1; index >= 0; index -= 1) {
          targets.push(target.children[index][2]);
        }
      }
    }
  }

  // predeclare all compile-time local binders
  for (const node of nodes) {
    const scopeOrdinal = bindingScopeByOrdinal[node.ordinal];

    // declare callable parameters in their body scope
    if (callableKinds.has(node.kind)) {
      for (const association of node.associations) {
        // declare one present parameter name
        if (association[4] !== null) {
          declareTarget(association[4], node.ordinal);
        }
      }
      // declare the definition name in its enclosing scope
      if (node.kind !== "lambda") {
        scopes.get(scopeByOrdinal[node.ordinal]).localNames.add(node.identifier);
      }
    }
    // declare assignment target families
    if (["assign", "annotated-assign", "augmented-assign", "named-expression"].includes(node.kind)) {
      for (const child of node.children) {
        // declare one assignment target
        if (child[0] === "target" || child[0] === "targets") {
          declareTarget(child[2], scopeOrdinal);
        }
      }
    }
    // declare loop target families
    if (["for", "async-for"].includes(node.kind)) {
      const target = node.children.find(
        // locate one loop target
        (child) => child[0] === "target",
      );

      // declare one present loop target
      if (target !== undefined) {
        declareTarget(target[2], scopeOrdinal);
      }
    }
    // declare context-manager aliases
    if (["with", "async-with"].includes(node.kind)) {
      for (const association of node.associations) {
        // declare one present context alias
        if (association[5] !== null) {
          declareTarget(association[5], scopeOrdinal);
        }
      }
    }
    // declare one exception alias
    if (node.kind === "except" && node.identifier !== null) {
      scopes.get(scopeOrdinal).localNames.add(node.identifier);
    }
    // declare one comprehension target
    if (comprehensionKinds.has(node.kind)) {
      for (const association of node.associations) {
        // declare generator targets only
        if (association[0] === "generator") {
          declareTarget(association[4], node.ordinal);
        }
      }
    }
    // declare import-bound names
    if (node.kind === "import" || node.kind === "import-from") {
      for (const child of node.children) {
        const alias = aliasesByOrdinal[child[2]];

        // skip wildcard import barriers
        if (alias.name === "*") {
          continue;
        }
        const localName = alias.asname ?? (
          node.kind === "import" ? firstDottedSegment(alias.name) : alias.name
        );
        scopes.get(scopeOrdinal).localNames.add(localName);
      }
    }
  }

  // compare two positioned node ordinals
  function compareStartOrdinals(leftOrdinal, rightOrdinal) {
    const left = nodesByOrdinal.get(leftOrdinal);
    const right = nodesByOrdinal.get(rightOrdinal);
    return left.startLine - right.startLine ||
      left.startByteColumn - right.startByteColumn ||
      left.ordinal - right.ordinal;
  }

  const positionedCount = nodes.reduce(
    // count every positioned node
    (count, node) => count + Number(node.kind !== "module"),
    0,
  );
  let orderedNodeOrdinals = new Uint32Array(positionedCount);
  let orderedNodeScratch = new Uint32Array(positionedCount);
  let orderedIndex = 0;
  // seed one stable node identifier stream
  for (const node of nodes) {
    // omit the synthetic module node
    if (node.kind !== "module") {
      orderedNodeOrdinals[orderedIndex] = node.ordinal;
      orderedIndex += 1;
    }
  }
  // merge one exact start-coordinate order
  for (let width = 1; width < positionedCount; width *= 2) {
    // merge every adjacent run pair
    for (let start = 0; start < positionedCount; start += width * 2) {
      const middle = Math.min(start + width, positionedCount);
      const end = Math.min(start + width * 2, positionedCount);
      let left = start;
      let right = middle;
      let output = start;

      // consume both stable runs
      while (left < middle || right < end) {
        const takeLeft = right >= end || (
          left < middle && compareStartOrdinals(
            orderedNodeOrdinals[left],
            orderedNodeOrdinals[right],
          ) <= 0
        );
        orderedNodeScratch[output] = takeLeft
          ? orderedNodeOrdinals[left++]
          : orderedNodeOrdinals[right++];
        output += 1;
      }
    }
    const previous = orderedNodeOrdinals;
    orderedNodeOrdinals = orderedNodeScratch;
    orderedNodeScratch = previous;
  }
  const orderedNodes = [];
  // expose one stable node reference stream
  for (const ordinal of orderedNodeOrdinals) {
    orderedNodes.push(nodesByOrdinal.get(ordinal));
  }

  const rawPathByOrdinal = new Array(nodes.length).fill(null);
  // cache every persistent syntactic path bottom-up
  for (let index = nodes.length - 1; index >= 0; index -= 1) {
    const node = nodes[index];

    // retain one lexical name root
    if (node.kind === "name") {
      const pathId = internDottedPath(node.identifier);
      rawPathByOrdinal[node.ordinal] = {
        depth: pathDepths[pathId],
        firstAttribute: null,
        pathId,
        rootNameId: internSurfaceName(firstDottedSegment(node.identifier)),
        surfaceDepth: 0,
        surfaceRootNameId: internSurfaceName(node.identifier),
      };
      continue;
    }
    // extend one exact attribute receiver
    if (node.kind === "attribute") {
      const receiver = node.children.find(
        // locate one scalar receiver
        (child) => child[0] === "value" && child[1] === 0,
      );
      const base = receiver === undefined
        ? null
        : rawPathByOrdinal[receiver[2]];

      // retain one complete attribute path
      if (base !== null) {
        const pathId = appendDottedPath(base.pathId, node.identifier);
        rawPathByOrdinal[node.ordinal] = {
          depth: pathDepths[pathId],
          firstAttribute: base.surfaceDepth === 0
            ? node.identifier
            : base.firstAttribute,
          pathId,
          rootNameId: base.rootNameId,
          surfaceDepth: base.surfaceDepth === 0 ? 1 : 2,
          surfaceRootNameId: base.surfaceRootNameId,
        };
      }
    }
  }

  const rawPathTrieNodeCount = pathParentIds.length;
  const rawWholeRequirementStates = new Int32Array(rawPathTrieNodeCount);
  const rawTailRequirementStates = new Int32Array(rawPathTrieNodeCount);
  rawWholeRequirementStates.fill(-1);
  rawTailRequirementStates.fill(-1);
  rawWholeRequirementStates[0] = 0;
  rawTailRequirementStates[0] = 0;

  // advance one exact finite requirement state
  function advanceRawRequirement(state, segmentId) {
    // retain one permanently dead continuation
    if (state < 0) {
      return -1;
    }
    return rawRequirementTransitions[state].get(segmentId) ?? -1;
  }

  // derive whole and root-skipping states for every frozen raw path node
  for (let pathId = 1; pathId < rawPathTrieNodeCount; pathId += 1) {
    const parentId = pathParentIds[pathId];
    const segmentId = pathSegmentById[pathId];
    rawWholeRequirementStates[pathId] = advanceRawRequirement(
      rawWholeRequirementStates[parentId],
      segmentId,
    );
    rawTailRequirementStates[pathId] = parentId === 0
      ? 0
      : advanceRawRequirement(
        rawTailRequirementStates[parentId],
        segmentId,
      );
  }

  // assign every syntactic path from its complete finite states
  for (const raw of rawPathByOrdinal) {
    // skip one nonpath expression
    if (raw !== null) {
      const historicalState = rawTailRequirementStates[raw.pathId];
      const wildcardState = rawWholeRequirementStates[raw.pathId];
      raw.historicalRequiredMask = historicalState < 0
        ? 0
        : rawRequirementHistoricalMasks[historicalState];
      raw.wildcardRequiredMask = wildcardState < 0
        ? 0
        : rawRequirementWildcardMasks[wildcardState];
    }
  }

  const surfacePathIds = new Map([
    [internDottedPath("subprocess"), SURFACE_SUBPROCESS],
    [internDottedPath("subprocess.run"), SURFACE_RUN],
    [internDottedPath("subprocess.call"), SURFACE_CALL],
    [internDottedPath("subprocess.check_call"), SURFACE_CHECK_CALL],
    [internDottedPath("subprocess.check_output"), SURFACE_CHECK_OUTPUT],
    [internDottedPath("subprocess.Popen"), SURFACE_POPEN],
    [internDottedPath("subprocess.PIPE"), SURFACE_PIPE],
  ]);

  // map one exact lexical path to one finite surface id
  function surfaceIdForPath(pathId) {
    return surfacePathIds.get(pathId) ?? SURFACE_NONE;
  }

  // create one dense lexical cell
  function lexicalCell(scopeOrdinal, nameId) {
    const scope = scopes.get(scopeOrdinal);
    const existing = scope.cellByNameId.get(nameId);

    // retain one existing scope-name cell
    if (existing !== undefined) {
      return existing;
    }
    const cellId = cells.length;
    cells.push({
      events: [],
      historicalMask: 0,
      nameId,
      scopeOrdinal,
      suffixValueIds: null,
    });
    scope.cellByNameId.set(nameId, cellId);
    return cellId;
  }

  // bind one ordered lexical value
  function addBinding(scopeOrdinal, name, position, value, conditional = false) {
    const scope = scopes.get(scopeOrdinal);
    const nameId = internSurfaceName(name);
    const cellId = lexicalCell(scopeOrdinal, nameId);
    const binding = {
      conditional,
      eventId: nextBindingEventId,
      position,
      value,
    };
    nextBindingEventId += 1;
    scope.localNames.add(name);
    const stable = scope.surfaceBindings.get(nameId) ?? null;
    const surfaceId = value.kind === "path"
      ? surfaceIdForPath(value.pathId)
      : SURFACE_NONE;
    const proofSurfaceId = conditional ? SURFACE_NONE : surfaceId;
    surfaceEvents.push({
      binding,
      cellId,
      eventId: binding.eventId,
      nameId,
      position,
      scopeOrdinal,
      surfaceId: proofSurfaceId,
      wildcard: null,
    });
    // retain one actual canonical binding event
    if (canonicalSurfaceId(proofSurfaceId)) {
      canonicalBindingPresent = true;
    }
    // poison one conditional nonpath or competing surface binding
    if (
      conditional || surfaceId === SURFACE_NONE ||
      stable === SURFACE_NONE ||
      (stable !== null && stable !== surfaceId)
    ) {
      scope.surfaceBindings.set(nameId, SURFACE_NONE);
    } else {
      // retain one exact stable surface path
      scope.surfaceBindings.set(nameId, surfaceId);
    }
    return binding;
  }

  // bind one target tree and retain member work
  function bindTarget(
    rootTargetOrdinal,
    rootValueOrdinal,
    scopeOrdinal,
    position,
    conditional,
  ) {
    const targets = [{
      targetOrdinal: rootTargetOrdinal,
      valueOrdinal: rootValueOrdinal,
    }];

    // consume every destructuring edge in order
    while (targets.length > 0) {
      const current = targets.pop();
      const target = nodesByOrdinal.get(current.targetOrdinal);
      const value = current.valueOrdinal === null
        ? null
        : nodesByOrdinal.get(current.valueOrdinal);

      // bind one direct name
      if (target?.kind === "name") {
        const binding = value?.kind === "constant" &&
          value.literalKind === "string"
          ? literalBinding(value)
          : UNKNOWN_BINDING;
        const event = addBinding(
          scopeOrdinal,
          target.identifier,
          position,
          binding,
          conditional,
        );
        bindingEventByTargetOrdinal.set(target.ordinal, event.eventId);
        continue;
      }
      // align exact tuple and list destructuring
      if (["list", "tuple"].includes(target?.kind)) {
        const targetChildren = target.children.filter(
          // retain target elements only
          (child) => child[0] === "elts",
        );
        const valueChildren = ["list", "tuple"].includes(value?.kind)
          ? value.children.filter(
            // retain value elements only
            (child) => child[0] === "elts",
          )
          : [];
        const exact = valueChildren.length === targetChildren.length &&
          targetChildren.every(
            // reject starred alignment
            (child) => nodesByOrdinal.get(child[2])?.kind !== "starred",
          );

        // preserve recursive left-to-right target order
        for (let index = targetChildren.length - 1; index >= 0; index -= 1) {
          targets.push({
            targetOrdinal: targetChildren[index][2],
            valueOrdinal: exact ? valueChildren[index][2] : null,
          });
        }
        continue;
      }
      // unwrap one starred target conservatively
      if (target?.kind === "starred") {
        const nested = target.children.find(
          // locate one starred value target
          (child) => child[0] === "value",
        );

        // retain one present nested target
        if (nested !== undefined) {
          targets.push({ targetOrdinal: nested[2], valueOrdinal: null });
        }
        continue;
      }
      // retain member and subscript invalidation work
      if (target?.kind === "attribute" || target?.kind === "subscript") {
        const base = target.kind === "attribute"
          ? target.ordinal
          : target.children.find(
            // locate one subscript base
            (child) => child[0] === "value",
          )?.[2] ?? null;
        pendingMemberWrites.push({
          conditional,
          position,
          queryScopeOrdinal: base === null ? scopeOrdinal : scopeByOrdinal[base],
          raw: base === null ? null : rawPathByOrdinal[base],
          scopeOrdinal,
          targetOrdinal: target.ordinal,
        });
      }
    }
  }

  // reset every helper literal to its derived semantic role
  for (const node of nodes) {
    // normalize one literal role before lexical selection
    if (node.kind === "constant") {
      node.literalRole = "semantic-constant";
    }
  }

  // emit every binding wildcard member and query event once
  for (const node of orderedNodes) {
    const scopeOrdinal = bindingScopeByOrdinal[node.ordinal];
    const position = endByOrdinal[node.ordinal];
    const conditional = conditionalByOrdinal[node.ordinal] !== 0;

    // retain one call path demand
    if (node.kind === "call") {
      const func = node.children.find(
        // locate one call target
        (child) => child[0] === "func" && child[1] === 0,
      );
      callDemands.push({
        callOrdinal: node.ordinal,
        position: startByOrdinal[node.ordinal],
        raw: func === undefined ? null : rawPathByOrdinal[func[2]],
        targetOrdinal: func?.[2] ?? null,
      });
    }
    // bind one callable declaration after enclosing evaluation
    if (["function", "async-function"].includes(node.kind)) {
      addBinding(
        scopeByOrdinal[node.ordinal],
        node.identifier,
        position,
        UNKNOWN_BINDING,
        conditional,
      );
    }
    // bind one import statement
    if (node.kind === "import") {
      for (const child of node.children) {
        const alias = aliasesByOrdinal[child[2]];
        const localName = alias.asname ?? firstDottedSegment(alias.name);
        const pathId = internDottedPath(
          alias.asname === null ? firstDottedSegment(alias.name) : alias.name,
        );
        addBinding(
          scopeOrdinal,
          localName,
          position,
          pathBinding(pathId),
          conditional,
        );
      }
    }
    // bind one from-import statement
    if (node.kind === "import-from") {
      for (const child of node.children) {
        const alias = aliasesByOrdinal[child[2]];

        // retain one wildcard barrier without inventing names
        if (alias.name === "*") {
          const scope = scopes.get(scopeOrdinal);
          const wildcard = {
            module: node.importLevel === 0 ? node.identifier : null,
            moduleMask: node.importLevel === 0 && node.identifier === "datetime"
              ? 1
              : node.importLevel === 0 && node.identifier === "time"
                ? 2
                : 0,
            position,
          };
          scope.hasWildcard = true;
          surfaceEvents.push({
            binding: null,
            cellId: null,
            eventId: 0,
            nameId: null,
            position,
            scopeOrdinal,
            surfaceId: SURFACE_NONE,
            wildcard,
          });
          // retain one permanent free-module wildcard barrier
          if (scopeOrdinal === 0) {
            moduleSurfaceWildcard = true;
          }
          continue;
        }
        const value = node.importLevel === 0 && node.identifier !== null
          ? pathBinding(appendDottedPath(
            internDottedPath(node.identifier),
            alias.name,
          ))
          : UNKNOWN_BINDING;
        addBinding(
          scopeOrdinal,
          alias.asname ?? alias.name,
          position,
          value,
          conditional,
        );
      }
    }
    // bind assignment target families after value evaluation
    if (["assign", "annotated-assign", "named-expression"].includes(node.kind)) {
      const valueEdge = node.children.find(
        // locate one assignment value
        (child) => child[0] === "value" && child[1] === 0,
      );
      for (const child of node.children) {
        // bind one assignment target tree
        if (child[0] === "target" || child[0] === "targets") {
          bindTarget(
            child[2],
            valueEdge?.[2] ?? null,
            scopeOrdinal,
            position,
            conditional,
          );
        }
      }
    }
    // invalidate one augmented target
    if (node.kind === "augmented-assign") {
      const targetEdge = node.children.find(
        // locate one augmented target
        (child) => child[0] === "target" && child[1] === 0,
      );

      // bind one present target as unknown
      if (targetEdge !== undefined) {
        bindTarget(
          targetEdge[2],
          null,
          scopeOrdinal,
          position,
          conditional,
        );
      }
    }
    // bind one regular loop target after iterator evaluation
    if (["for", "async-for"].includes(node.kind)) {
      const target = node.children.find(
        // locate one loop target
        (child) => child[0] === "target",
      );
      const iterator = node.children.find(
        // locate one loop iterator
        (child) => child[0] === "iter",
      );

      // bind one present loop target
      if (target !== undefined) {
        bindTarget(
          target[2],
          null,
          scopeOrdinal,
          iterator === undefined ? position : endByOrdinal[iterator[2]],
          true,
        );
      }
    }
    // bind context-manager aliases left to right
    if (["with", "async-with"].includes(node.kind)) {
      for (const association of node.associations) {
        // bind one present context alias
        if (association[5] !== null) {
          bindTarget(
            association[5],
            null,
            scopeOrdinal,
            endByOrdinal[association[4]],
            conditional,
          );
        }
      }
    }
    // bind one exception alias within its handler
    if (node.kind === "except" && node.identifier !== null) {
      addBinding(
        scopeOrdinal,
        node.identifier,
        startByOrdinal[node.ordinal],
        UNKNOWN_BINDING,
        true,
      );
    }
    // prebind comprehension targets in their implicit scope
    if (comprehensionKinds.has(node.kind)) {
      for (const association of node.associations) {
        // bind generator targets only
        if (association[0] === "generator") {
          bindTarget(association[4], null, node.ordinal, -1, false);
        }
      }
    }
    // prebind callable parameters
    if (callableKinds.has(node.kind)) {
      for (const association of node.associations) {
        const parameter = association[4] === null
          ? null
          : nodesByOrdinal.get(association[4]);

        // bind one present parameter as unknown
        if (parameter !== null) {
          addBinding(
            node.ordinal,
            parameter.identifier,
            -1,
            UNKNOWN_BINDING,
            false,
          );
        }
      }
    }
  }

  const subprocessApis = new Set([
    SURFACE_RUN, SURFACE_CALL, SURFACE_CHECK_CALL,
    SURFACE_CHECK_OUTPUT, SURFACE_POPEN,
  ]);

  // extend one finite rejection-only surface id
  function extendSurfaceId(surfaceId, depth, firstAttribute) {
    // preserve one root identity
    if (depth === 0) {
      return surfaceId;
    }
    // extend one subprocess module reference
    if (surfaceId === SURFACE_SUBPROCESS) {
      if (depth > 1) {
        return SURFACE_MEMBER;
      }
      return new Map([
        ["run", SURFACE_RUN],
        ["call", SURFACE_CALL],
        ["check_call", SURFACE_CHECK_CALL],
        ["check_output", SURFACE_CHECK_OUTPUT],
        ["Popen", SURFACE_POPEN],
        ["PIPE", SURFACE_PIPE],
      ]).get(firstAttribute) ?? SURFACE_MEMBER;
    }
    // retain one extended subprocess member
    if (
      SURFACE_APIS.has(surfaceId) ||
      surfaceId === SURFACE_PIPE || surfaceId === SURFACE_MEMBER
    ) {
      return SURFACE_MEMBER;
    }
    return SURFACE_NONE;
  }

  // classify one canonical subprocess identity
  function canonicalSurfaceId(surfaceId) {
    return surfaceId === SURFACE_SUBPROCESS ||
      SURFACE_APIS.has(surfaceId) || surfaceId === SURFACE_MEMBER;
  }

  // reject one direct name binder read
  function directNameBinder(node) {
    const write = node?.kind === "name" &&
      node.operators.some(
        // detect one direct store or delete context
        (operator) => operator === "context.store" || operator === "context.delete",
      );
    const parent = write
      ? nodesByOrdinal.get(parentByOrdinal.get(node.ordinal))
      : null;
    return write && parent?.kind !== "augmented-assign";
  }

  const surfaceShapeByOrdinal = new Map();
  const relevantNamesByScope = new Map();
  // project every finite surface demand from the shared path cache
  for (const node of nodes) {
    const raw = rawPathByOrdinal[node.ordinal];

    // omit nonpaths and direct binders
    if (raw === null || directNameBinder(node)) {
      continue;
    }
    const shape = {
      depth: raw.surfaceDepth,
      firstAttribute: raw.firstAttribute,
      rootNameId: raw.surfaceRootNameId,
    };
    surfaceShapeByOrdinal.set(node.ordinal, shape);
    const scopeOrdinal = scopeByOrdinal[node.ordinal];
    const relevant = relevantNamesByScope.get(scopeOrdinal) ?? new Set();
    relevant.add(shape.rootNameId);
    relevantNamesByScope.set(scopeOrdinal, relevant);
  }

  const localNameIdsByScope = new Map();
  const scopeChildren = new Map();
  // intern every actual scope local and edge once
  for (const [scopeOrdinal, scope] of scopes) {
    const localIds = new Set();
    // intern each declared local name
    for (const name of scope.localNames) {
      localIds.add(internSurfaceName(name));
    }
    localNameIdsByScope.set(scopeOrdinal, localIds);
    scopeChildren.set(scopeOrdinal, []);
  }
  // link every child scope to its direct parent
  for (const [scopeOrdinal, scope] of scopes) {
    // skip the module root
    if (scope.parent !== null) {
      scopeChildren.get(scope.parent).push(scopeOrdinal);
    }
  }

  const freeModuleNamesByScope = new Map();
  const blockerCounts = new Map();
  const scopeStack = [{ exiting: false, scopeOrdinal: 0 }];
  // project module-free names with blocker undo
  while (scopeStack.length > 0) {
    const entry = scopeStack.pop();
    const localIds = localNameIdsByScope.get(entry.scopeOrdinal);

    // undo one completed lexical scope
    if (entry.exiting) {
      if (entry.scopeOrdinal !== 0) {
        // remove each local blocker once
        for (const nameId of localIds) {
          const remaining = blockerCounts.get(nameId) - 1;
          // remove one exhausted blocker
          if (remaining === 0) {
            blockerCounts.delete(nameId);
          } else {
            blockerCounts.set(nameId, remaining);
          }
        }
      }
      continue;
    }
    const allowed = new Set();
    const relevant = relevantNamesByScope.get(entry.scopeOrdinal) ?? new Set();
    // retain every unblocked free name
    for (const nameId of relevant) {
      if (!localIds.has(nameId) && !blockerCounts.has(nameId)) {
        allowed.add(nameId);
      }
    }
    freeModuleNamesByScope.set(entry.scopeOrdinal, allowed);
    scopeStack.push({ exiting: true, scopeOrdinal: entry.scopeOrdinal });
    // block this scope's locals for descendants
    if (entry.scopeOrdinal !== 0) {
      for (const nameId of localIds) {
        blockerCounts.set(nameId, (blockerCounts.get(nameId) ?? 0) + 1);
      }
    }
    const children = scopeChildren.get(entry.scopeOrdinal);
    // visit child scopes in stable order
    for (let index = children.length - 1; index >= 0; index -= 1) {
      scopeStack.push({ exiting: false, scopeOrdinal: children[index] });
    }
  }

  const stableModuleSurfaces = scopes.get(0).surfaceBindings;
  const surfaceByOrdinal = new Uint8Array(nodes.length);
  const surfaceEventByOrdinal = new Uint32Array(nodes.length);
  const surfaceEpochAtOrdinal = new Uint32Array(nodes.length);
  const surfaceActions = new Array(source.sourceText.length + 2);
  const activeSurfacesByScope = new Map();
  const surfaceEpochByScope = new Map();
  // initialize one finite active table per actual scope
  for (const scopeOrdinal of scopes.keys()) {
    activeSurfacesByScope.set(scopeOrdinal, new Map());
    surfaceEpochByScope.set(scopeOrdinal, 0);
  }

  // retain one sparse source-coordinate bucket
  function surfaceBucket(position) {
    const index = position + 1;
    let bucket = surfaceActions[index];

    // allocate one actual coordinate bucket
    if (bucket === undefined) {
      bucket = { demands: [], memberWrites: [], writes: [] };
      surfaceActions[index] = bucket;
    }
    return bucket;
  }

  // bucket every actual finite path demand once
  for (const ordinal of surfaceShapeByOrdinal.keys()) {
    surfaceBucket(startByOrdinal[ordinal]).demands.push(ordinal);
  }
  // bucket every existing binding or wildcard event once
  for (const event of surfaceEvents) {
    surfaceBucket(event.position).writes.push(event);
  }
  // bucket every member write in original emission order
  for (const write of pendingMemberWrites) {
    surfaceBucket(write.position).memberWrites.push(write);
  }

  const orderedMemberWrites = [];
  // sweep reads before same-position writes
  for (const bucket of surfaceActions) {
    // skip one empty source coordinate
    if (bucket === undefined) {
      continue;
    }
    // freeze every demand against current state
    for (const ordinal of bucket.demands) {
      const shape = surfaceShapeByOrdinal.get(ordinal);
      const scopeOrdinal = scopeByOrdinal[ordinal];
      const localIds = localNameIdsByScope.get(scopeOrdinal);
      const epoch = surfaceEpochByScope.get(scopeOrdinal);
      let rootId = SURFACE_NONE;
      let eventId = 0;

      // use one current module or compile-local cell
      if (scopeOrdinal === 0 || localIds.has(shape.rootNameId)) {
        const entry = activeSurfacesByScope.get(scopeOrdinal).get(shape.rootNameId);

        // retain one cell stamped after the latest wildcard
        if (entry !== undefined && entry.epoch === epoch) {
          rootId = entry.surfaceId;
          eventId = entry.eventId;
        }
      } else if (
        !moduleSurfaceWildcard &&
        freeModuleNamesByScope.get(scopeOrdinal)?.has(shape.rootNameId)
      ) {
        // use one unblocked whole-module stable cell
        rootId = stableModuleSurfaces.get(shape.rootNameId) ??
          SURFACE_NONE;
      }
      surfaceByOrdinal[ordinal] = extendSurfaceId(
        rootId,
        shape.depth,
        shape.firstAttribute,
      );
      surfaceEventByOrdinal[ordinal] = eventId;
      surfaceEpochAtOrdinal[ordinal] = epoch;
    }
    // install every same-position write in insertion order
    for (const event of bucket.writes) {
      // advance one wildcard epoch without scanning cells
      if (event.nameId === null) {
        const scope = scopes.get(event.scopeOrdinal);
        scope.wildcardImports.push(event.wildcard);
        scope.wildcardPositions.push(event.position);
        surfaceEpochByScope.set(
          event.scopeOrdinal,
          surfaceEpochByScope.get(event.scopeOrdinal) + 1,
        );
        continue;
      }
      cells[event.cellId].events.push(event.binding);
      activeSurfacesByScope.get(event.scopeOrdinal).set(event.nameId, {
        epoch: surfaceEpochByScope.get(event.scopeOrdinal),
        eventId: event.eventId,
        surfaceId: event.surfaceId,
      });
    }
    // retain one already ordered member-write stream
    for (const write of bucket.memberWrites) {
      orderedMemberWrites.push(write);
    }
  }

  // read one frozen rejection-only surface identity
  function surfaceIdentity(ordinal) {
    return surfaceByOrdinal[ordinal] ?? SURFACE_NONE;
  }

  const parentUseByOrdinal = new Map();
  // retain every incoming owned use once
  for (const node of nodes) {
    // index ordinary child uses
    for (const child of node.children) {
      const relation = node.kind === "attribute" && child[0] === "value"
        ? "attribute-receiver"
        : node.kind === "call" && child[0] === "func"
          ? "call-func"
          : "terminal";
      parentUseByOrdinal.set(child[2], relation);
    }
    // index structural association uses
    for (const association of node.associations) {
      // retain one primary terminal use
      if (association[4] !== null) {
        parentUseByOrdinal.set(association[4], "terminal");
      }
      // retain one secondary terminal use
      if (association[5] !== null) {
        parentUseByOrdinal.set(association[5], "terminal");
      }
    }
  }

  let subprocessSurfaceTainted = false;
  // classify every terminal canonical use once
  for (const node of nodes) {
    const surfaceId = surfaceIdentity(node.ordinal);
    const relation = parentUseByOrdinal.get(node.ordinal) ?? "terminal";

    // propagate one attribute receiver
    if (relation === "attribute-receiver") {
      continue;
    }
    // discharge one exact reviewed API call target
    if (relation === "call-func" && SURFACE_APIS.has(surfaceId)) {
      continue;
    }
    // discharge every exact PIPE load
    if (
      surfaceId === SURFACE_PIPE && node.operators.includes("context.load")
    ) {
      continue;
    }
    // taint every remaining canonical terminal use
    if (canonicalSurfaceId(surfaceId) || surfaceId === SURFACE_PIPE) {
      subprocessSurfaceTainted = true;
    }
  }

  const capturedApiBindingEvents = new Set();
  // retain exact API capture binding events for rejection only
  for (const node of orderedNodes) {
    // inspect assignment target families only
    if (!["assign", "annotated-assign", "named-expression"].includes(node.kind)) {
      continue;
    }
    const value = node.children.find(
      // locate one assignment value
      (child) => child[0] === "value" && child[1] === 0,
    );
    const sourceId = value === undefined
      ? SURFACE_NONE
      : surfaceIdentity(value[2]);

    // skip non-API copies
    if (!SURFACE_APIS.has(sourceId)) {
      continue;
    }
    // retain each direct target's exact binding event
    for (const child of node.children) {
      if (child[0] !== "target" && child[0] !== "targets") {
        continue;
      }
      const target = nodesByOrdinal.get(child[2]);
      const eventId = target?.kind === "name"
        ? bindingEventByTargetOrdinal.get(target.ordinal) ?? null
        : null;

      // retain one current event identity
      if (eventId !== null) {
        capturedApiBindingEvents.add(eventId);
      }
    }
  }

  // classify bare namespace mutators without scope walks
  for (const node of orderedNodes) {
    // skip noncall or nonbare targets
    if (node.kind !== "call") {
      continue;
    }
    const func = node.children.find(
      // locate one call target
      (child) => child[0] === "func" && child[1] === 0,
    );
    const target = func === undefined ? null : nodesByOrdinal.get(func[2]);

    // skip nonbare function names
    if (target?.kind !== "name") {
      continue;
    }
    const scopeOrdinal = scopeByOrdinal[target.ordinal];
    const nameId = internSurfaceName(target.identifier);
    const localIds = localNameIdsByScope.get(scopeOrdinal);
    let bare = false;

    // use sequential module state only at module scope
    if (scopeOrdinal === 0) {
      bare = surfaceEventByOrdinal[target.ordinal] === 0 &&
        surfaceEpochAtOrdinal[target.ordinal] === 0;
    } else {
      // preserve compile-local and stable free-module shadows
      bare = !localIds.has(nameId) &&
        !localNameIdsByScope.get(0).has(nameId) &&
        !moduleSurfaceWildcard &&
        freeModuleNamesByScope.get(scopeOrdinal)?.has(nameId);
    }

    // taint one unbounded namespace builtin
    if (
      bare && canonicalBindingPresent &&
      ["exec", "eval", "globals", "locals"].includes(target.identifier)
    ) {
      subprocessSurfaceTainted = true;
    }
    // taint one exact setattr or delattr receiver
    if (
      bare && ["setattr", "delattr"].includes(target.identifier)
    ) {
      const argument = node.children.find(
        // locate one first positional argument
        (child) => child[0] === "args" && child[1] === 0,
      );
      const receiverId = argument === undefined
        ? SURFACE_NONE
        : surfaceIdentity(argument[2]);

      // retain one canonical or PIPE mutation
      if (canonicalSurfaceId(receiverId) || receiverId === SURFACE_PIPE) {
        subprocessSurfaceTainted = true;
      }
    }
  }

  const scopeOrdinals = [...scopes.keys()];
  const scopeIdByOrdinal = new Map();
  const scopeById = [];
  // assign one dense identifier to every actual scope
  for (let index = 0; index < scopeOrdinals.length; index += 1) {
    const scopeOrdinal = scopeOrdinals[index];
    const scope = scopes.get(scopeOrdinal);
    scope.id = index;
    scope.ordinal = scopeOrdinal;
    scopeIdByOrdinal.set(scopeOrdinal, index);
    scopeById.push(scope);
  }
  // link every dense parent identifier
  for (const scope of scopeById) {
    scope.parentId = scope.parent === null
      ? null
      : scopeIdByOrdinal.get(scope.parent);
  }

  let nextEuler = 1;
  const eulerStack = [{ depth: 0, exiting: false, scopeOrdinal: 0 }];
  // index the lexical scope tree once
  while (eulerStack.length > 0) {
    const entry = eulerStack.pop();
    const scope = scopes.get(entry.scopeOrdinal);

    // close one lexical subtree
    if (entry.exiting) {
      scope.tout = nextEuler - 1;
      continue;
    }
    scope.depth = entry.depth;
    scope.tin = nextEuler;
    nextEuler += 1;
    eulerStack.push({
      depth: entry.depth,
      exiting: true,
      scopeOrdinal: entry.scopeOrdinal,
    });
    const children = scopeChildren.get(entry.scopeOrdinal);
    // preserve stable lexical child order
    for (let index = children.length - 1; index >= 0; index -= 1) {
      eulerStack.push({
        depth: entry.depth + 1,
        exiting: false,
        scopeOrdinal: children[index],
      });
    }
  }

  const SUFFIX_EMPTY = 0;
  const SUFFIX_UNSTABLE = -1;
  // build every cell suffix and historical state once
  for (const cell of cells) {
    let historicalMask = 0;
    const suffix = new Float64Array(cell.events.length + 1);
    suffix[cell.events.length] = SUFFIX_EMPTY;

    // fold one already ordered history in reverse
    for (let index = cell.events.length - 1; index >= 0; index -= 1) {
      const event = cell.events[index];
      const stateIndex = event.value.kind === "path"
        ? providerStateIndexByPathId.get(event.value.pathId)
        : undefined;

      // retain one exact provider-prefix state
      if (stateIndex !== undefined) {
        historicalMask |= 1 << stateIndex;
      }
      const following = suffix[index + 1];
      suffix[index] = event.conditional
        ? SUFFIX_UNSTABLE
        : following === SUFFIX_EMPTY || following === event.value.valueId
          ? event.value.valueId
          : SUFFIX_UNSTABLE;
    }
    cell.historicalMask = historicalMask >>> 0;
    cell.suffixValueIds = suffix;
  }
  // build every wildcard provider prefix once
  for (const scope of scopeById) {
    const prefixMasks = new Uint8Array(scope.wildcardImports.length + 1);
    // fold one already ordered wildcard stream
    for (let index = 0; index < scope.wildcardImports.length; index += 1) {
      prefixMasks[index + 1] = prefixMasks[index] |
        scope.wildcardImports[index].moduleMask;
    }
    scope.wildcardPrefixMasks = prefixMasks;
  }

  const routeNameIdsByScope = new Map();
  // allocate one sparse demand-name set per scope
  for (const scopeOrdinal of scopeOrdinals) {
    routeNameIdsByScope.set(scopeOrdinal, new Set());
  }
  // retain every syntactic path root demand
  for (let ordinal = 0; ordinal < rawPathByOrdinal.length; ordinal += 1) {
    const raw = rawPathByOrdinal[ordinal];

    // retain one actual path root only
    if (raw !== null) {
      routeNameIdsByScope.get(scopeByOrdinal[ordinal]).add(raw.rootNameId);
    }
  }
  // retain every exact identifier demand for payload lookup
  for (const node of nodes) {
    // retain name nodes only
    if (node.kind === "name") {
      routeNameIdsByScope.get(scopeByOrdinal[node.ordinal]).add(
        internSurfaceName(node.identifier),
      );
    }
  }

  let minimumTreeSize = 1;
  // cover every possible lexical depth
  while (minimumTreeSize < scopeById.length + 1) {
    minimumTreeSize *= 2;
  }
  const callableMinimumTree = new Float64Array(minimumTreeSize * 2);
  callableMinimumTree.fill(Number.POSITIVE_INFINITY);

  // update one active callable depth
  function updateCallableMinimum(depth, value) {
    let index = minimumTreeSize + depth;
    callableMinimumTree[index] = value;

    // refresh every affected minimum
    while (index > 1) {
      index = Math.floor(index / 2);
      callableMinimumTree[index] = Math.min(
        callableMinimumTree[index * 2],
        callableMinimumTree[index * 2 + 1],
      );
    }
  }

  // query one inclusive callable-depth minimum
  function queryCallableMinimum(leftDepth, rightDepth) {
    // return one empty lexical range
    if (leftDepth > rightDepth) {
      return Number.POSITIVE_INFINITY;
    }
    let left = minimumTreeSize + leftDepth;
    let right = minimumTreeSize + rightDepth;
    let result = Number.POSITIVE_INFINITY;

    // consume one flat segment-tree range
    while (left <= right) {
      // consume one left fragment
      if ((left & 1) === 1) {
        result = Math.min(result, callableMinimumTree[left]);
        left += 1;
      }
      // consume one right fragment
      if ((right & 1) === 0) {
        result = Math.min(result, callableMinimumTree[right]);
        right -= 1;
      }
      left = Math.floor(left / 2);
      right = Math.floor(right / 2);
    }
    return result;
  }

  const routesByScope = new Map();
  const activeOwners = new Map();
  const activeHistoricalMasks = new Map();
  const activeCallableScopeIds = [];
  let activeWildcardScope = null;
  const routeStack = [{ exiting: false, scopeId: 0 }];
  // build every sparse lexical route with undo
  while (routeStack.length > 0) {
    const entry = routeStack.pop();
    const scope = scopeById[entry.scopeId];

    // undo one completed lexical scope
    if (entry.exiting) {
      // restore every exact name owner
      for (const [nameId, previous] of entry.ownerUndo) {
        // remove one newly introduced owner
        if (previous === undefined) {
          activeOwners.delete(nameId);
        } else {
          activeOwners.set(nameId, previous);
        }
      }
      // restore every historical provider union
      for (const [nameId, previous] of entry.historicalUndo) {
        // remove one newly introduced union
        if (previous === undefined) {
          activeHistoricalMasks.delete(nameId);
        } else {
          activeHistoricalMasks.set(nameId, previous);
        }
      }
      activeWildcardScope = entry.previousWildcard;
      // close one callable depth
      if (scope.type === "callable") {
        activeCallableScopeIds.pop();
      }
      updateCallableMinimum(scope.depth, Number.POSITIVE_INFINITY);
      continue;
    }
    updateCallableMinimum(
      scope.depth,
      scope.type === "callable"
        ? scope.definitionPosition
        : Number.POSITIVE_INFINITY,
    );
    // open one callable depth
    if (scope.type === "callable") {
      activeCallableScopeIds.push(scope.id);
    }
    const ownerUndo = [];
    const localIds = localNameIdsByScope.get(scope.ordinal);
    // install every compile-time local owner
    for (const nameId of localIds) {
      const previous = activeOwners.get(nameId);
      ownerUndo.push([nameId, previous]);
      activeOwners.set(nameId, {
        cellId: scope.cellByNameId.get(nameId) ?? null,
        scopeId: scope.id,
      });
    }
    const historicalUndo = [];
    // install every exact historical provider state
    for (const cellId of scope.cellByNameId.values()) {
      const cell = cells[cellId];

      // skip one empty provider state
      if (cell.historicalMask === 0) {
        continue;
      }
      const previous = activeHistoricalMasks.get(cell.nameId);
      historicalUndo.push([cell.nameId, previous]);
      activeHistoricalMasks.set(
        cell.nameId,
        ((previous ?? 0) | cell.historicalMask) >>> 0,
      );
    }
    const previousWildcard = activeWildcardScope;
    // install one nearest wildcard stop
    if (scope.hasWildcard) {
      activeWildcardScope = scope;
    }
    const routes = new Map();
    const demandedNames = routeNameIdsByScope.get(scope.ordinal);
    // freeze every demanded route at this scope
    for (const nameId of demandedNames) {
      const owner = activeOwners.get(nameId) ?? null;
      const wildcard = activeWildcardScope;
      const useOwner = owner !== null && (
        wildcard === null || scopeById[owner.scopeId].depth >= wildcard.depth
      );
      const stopScope = useOwner
        ? scopeById[owner.scopeId]
        : wildcard ?? scopeById[0];
      const minimumDefinition = queryCallableMinimum(
        stopScope.depth + 1,
        scope.depth,
      );
      const nearestCallableId = activeCallableScopeIds.length === 0
        ? null
        : activeCallableScopeIds[activeCallableScopeIds.length - 1];
      const firstCallableId = nearestCallableId !== null &&
        scopeById[nearestCallableId].depth > stopScope.depth
        ? nearestCallableId
        : null;
      routes.set(nameId, {
        cellId: useOwner ? owner.cellId : null,
        firstCallableId,
        historicalMask: activeHistoricalMasks.get(nameId) ?? 0,
        minimumDefinition,
        queryScopeId: scope.id,
        stable: Number.isFinite(minimumDefinition),
        stopScopeId: stopScope.id,
      });
    }
    routesByScope.set(scope.ordinal, routes);
    routeStack.push({
      exiting: true,
      historicalUndo,
      ownerUndo,
      previousWildcard,
      scopeId: scope.id,
    });
    const children = scopeChildren.get(scope.ordinal);
    // preserve stable lexical child order
    for (let index = children.length - 1; index >= 0; index -= 1) {
      routeStack.push({
        exiting: false,
        scopeId: scopeIdByOrdinal.get(children[index]),
      });
    }
  }

  // read one frozen sparse lexical route
  function lexicalRoute(scopeOrdinal, nameId) {
    return routesByScope.get(scopeOrdinal).get(nameId);
  }

  // locate one first event after a numeric boundary
  function upperBoundEvents(events, boundary) {
    let left = 0;
    let right = events.length;

    // narrow one allocation-free predecessor range
    while (left < right) {
      const middle = Math.floor((left + right) / 2);

      // consume every event at or before the boundary
      if (events[middle].position <= boundary) {
        left = middle + 1;
      } else {
        right = middle;
      }
    }
    return left;
  }

  // locate one first wildcard after a numeric boundary
  function upperBoundPositions(positions, boundary) {
    let left = 0;
    let right = positions.length;

    // narrow one allocation-free predecessor range
    while (left < right) {
      const middle = Math.floor((left + right) / 2);

      // consume every position at or before the boundary
      if (positions[middle] <= boundary) {
        left = middle + 1;
      } else {
        right = middle;
      }
    }
    return left;
  }

  // select one exact indexed cell event
  function selectCellEvent(route, position) {
    // stop one route without a lexical cell
    if (route.cellId === null) {
      return null;
    }
    const cell = cells[route.cellId];
    const scope = scopeById[route.stopScopeId];
    const boundary = route.stable
      ? Math.min(position, route.minimumDefinition)
      : position;
    const count = upperBoundEvents(cell.events, boundary);

    // reject one cell without a preceding event
    if (count === 0) {
      return null;
    }
    const selected = cell.events[count - 1];
    const wildcardCount = upperBoundPositions(scope.wildcardPositions, boundary);
    const priorWildcard = wildcardCount === 0
      ? null
      : scope.wildcardPositions[wildcardCount - 1];

    // reject one wildcard after the selected event
    if (priorWildcard !== null && priorWildcard > selected.position) {
      return null;
    }
    // preserve one sequential unconditional event including unknown
    if (!route.stable) {
      return selected.conditional ? null : selected;
    }
    // reject one unstable selected value
    if (selected.conditional || selected.value.kind === "unknown") {
      return null;
    }
    const suffix = cell.suffixValueIds[count];

    // reject one later divergent or conditional event
    if (suffix !== SUFFIX_EMPTY && suffix !== selected.value.valueId) {
      return null;
    }
    // reject every later wildcard barrier
    if (wildcardCount < scope.wildcardPositions.length) {
      return null;
    }
    return selected;
  }

  // resolve one indexed identifier binding
  function resolveIndexedName(scopeOrdinal, nameId, position) {
    const route = lexicalRoute(scopeOrdinal, nameId);
    const selected = selectCellEvent(route, position);

    // retain one selected lexical event
    if (selected !== null) {
      return {
        eventId: selected.eventId,
        route,
        value: selected.value,
      };
    }
    return { eventId: null, route, value: UNKNOWN_BINDING };
  }

  // resolve one indexed syntactic path root
  function resolveBindingPath(raw, scopeOrdinal, position) {
    // reject one nonpath expression
    if (raw === null) {
      return { pathId: null, resolution: null };
    }
    const resolution = resolveIndexedName(
      scopeOrdinal,
      raw.rootNameId,
      position,
    );

    // require one exact import path
    if (resolution.value.kind !== "path") {
      return { pathId: null, resolution };
    }
    return {
      pathId: appendRawTail(resolution.value.pathId, raw.pathId),
      resolution,
    };
  }

  const memberWriteCount = orderedMemberWrites.length;
  const memberCallCount = callDemands.length;
  const memberWorkCount = memberWriteCount + memberCallCount;
  const memberWriteTraces = new Array(memberWriteCount);
  const memberWriteScopeIds = new Uint32Array(memberWriteCount);
  // materialize every member-write base before freezing the path trie
  for (let index = 0; index < memberWriteCount; index += 1) {
    const write = orderedMemberWrites[index];
    memberWriteTraces[index] = resolveBindingPath(
      write.raw,
      write.queryScopeOrdinal,
      write.position,
    );
    memberWriteScopeIds[index] = scopeIdByOrdinal.get(write.scopeOrdinal);
  }

  const memberCallTraces = new Array(memberCallCount);
  // materialize every member-sensitive call before freezing the path trie
  for (let index = 0; index < memberCallCount; index += 1) {
    const demand = callDemands[index];
    const queryScopeOrdinal = demand.targetOrdinal === null
      ? scopeByOrdinal[demand.callOrdinal]
      : scopeByOrdinal[demand.targetOrdinal];
    memberCallTraces[index] = resolveBindingPath(
      demand.raw,
      queryScopeOrdinal,
      demand.position,
    );
  }

  const frozenPathCount = pathParentIds.length;
  const pathFirstChild = new Int32Array(frozenPathCount);
  const pathNextSibling = new Int32Array(frozenPathCount);
  pathFirstChild.fill(-1);
  pathNextSibling.fill(-1);
  // link every frozen path node without copying child lists
  for (let pathId = frozenPathCount - 1; pathId > 0; pathId -= 1) {
    const parentId = pathParentIds[pathId];
    pathNextSibling[pathId] = pathFirstChild[parentId];
    pathFirstChild[parentId] = pathId;
  }
  const pathTin = new Int32Array(frozenPathCount);
  const pathTout = new Int32Array(frozenPathCount);
  const pathEulerNodes = new Int32Array(frozenPathCount);
  const pathEulerChildren = new Int32Array(frozenPathCount);
  let pathEulerDepth = 1;
  let nextPathEuler = 1;
  pathEulerNodes[0] = 0;
  pathEulerChildren[0] = pathFirstChild[0];
  pathTin[0] = nextPathEuler;
  nextPathEuler += 1;
  // index the complete exact-path trie iteratively
  while (pathEulerDepth > 0) {
    const stackIndex = pathEulerDepth - 1;
    const childId = pathEulerChildren[stackIndex];

    // close one exhausted path subtree
    if (childId < 0) {
      pathTout[pathEulerNodes[stackIndex]] = nextPathEuler - 1;
      pathEulerDepth -= 1;
      continue;
    }
    pathEulerChildren[stackIndex] = pathNextSibling[childId];
    pathEulerNodes[pathEulerDepth] = childId;
    pathEulerChildren[pathEulerDepth] = pathFirstChild[childId];
    pathTin[childId] = nextPathEuler;
    nextPathEuler += 1;
    pathEulerDepth += 1;
  }

  const memberEventCapacity = Math.max(1, memberWorkCount * 3 + 1);
  const memberEventX = new Int32Array(memberEventCapacity);
  const memberEventKind = new Uint8Array(memberEventCapacity);
  const memberEventFirst = new Int32Array(memberEventCapacity);
  const memberEventSecond = new Int32Array(memberEventCapacity);
  const memberEventDelta = new Int32Array(memberEventCapacity);
  const memberEventTarget = new Int32Array(memberEventCapacity);
  const memberEventOrder = new Uint32Array(memberEventCapacity);
  const memberEventSortScratch = new Uint32Array(memberEventCapacity);
  const memberScopeFenwick = new Int32Array(scopeById.length + 2);
  let memberEventCount = 0;

  // update one reusable scope difference index
  function addMemberScopePoint(index, delta) {
    let cursor = index;

    // update every containing Fenwick range
    while (cursor < memberScopeFenwick.length) {
      memberScopeFenwick[cursor] += delta;
      cursor += cursor & -cursor;
    }
  }

  // read one reusable scope point
  function readMemberScopePoint(index) {
    let cursor = index;
    let result = 0;

    // consume every contributing Fenwick range
    while (cursor > 0) {
      result += memberScopeFenwick[cursor];
      cursor -= cursor & -cursor;
    }
    return result;
  }

  // apply one lexical-scope subtree range
  function addMemberScopeRange(start, end, delta) {
    addMemberScopePoint(start, delta);
    addMemberScopePoint(end + 1, -delta);
  }

  // append one closed path and scope rectangle
  function appendMemberRectangle(pathId, scopeId) {
    // reject one impossible shared-pool overflow
    if (memberEventCount + 2 > memberEventCapacity) {
      throw bootstrapError();
    }
    const scope = scopeById[scopeId];
    memberEventX[memberEventCount] = pathTin[pathId];
    memberEventKind[memberEventCount] = 0;
    memberEventFirst[memberEventCount] = scope.tin;
    memberEventSecond[memberEventCount] = scope.tout;
    memberEventDelta[memberEventCount] = 1;
    memberEventCount += 1;
    memberEventX[memberEventCount] = pathTout[pathId] + 1;
    memberEventKind[memberEventCount] = 0;
    memberEventFirst[memberEventCount] = scope.tin;
    memberEventSecond[memberEventCount] = scope.tout;
    memberEventDelta[memberEventCount] = -1;
    memberEventCount += 1;
  }

  // append one signed lexical-route point query
  function appendMemberQuery(pathId, startScopeId, endScopeId, targetIndex) {
    // reject one impossible shared-pool overflow
    if (memberEventCount >= memberEventCapacity) {
      throw bootstrapError();
    }
    const startScope = scopeById[startScopeId];
    memberEventX[memberEventCount] = pathTin[pathId];
    memberEventKind[memberEventCount] = 1;
    memberEventFirst[memberEventCount] = scopeById[endScopeId].tin;
    memberEventSecond[memberEventCount] = startScope.parentId === null
      ? 0
      : scopeById[startScope.parentId].tin;
    memberEventTarget[memberEventCount] = targetIndex;
    memberEventCount += 1;
  }

  // order one shared path-coordinate event pool stably
  function sortMemberEvents() {
    // initialize one numeric permutation
    for (let index = 0; index < memberEventCount; index += 1) {
      memberEventOrder[index] = index;
    }
    let sourceOrder = memberEventOrder;
    let targetOrder = memberEventSortScratch;
    // merge every numeric run without callback allocation
    for (let width = 1; width < memberEventCount; width *= 2) {
      // merge every adjacent run pair
      for (let start = 0; start < memberEventCount; start += width * 2) {
        const middle = Math.min(start + width, memberEventCount);
        const end = Math.min(start + width * 2, memberEventCount);
        let left = start;
        let right = middle;
        let output = start;

        // consume both ordered numeric runs
        while (left < middle || right < end) {
          let takeLeft = right >= end;

          // compare one present left event to one right event
          if (!takeLeft && left < middle) {
            const leftId = sourceOrder[left];
            const rightId = sourceOrder[right];
            const leftX = memberEventX[leftId];
            const rightX = memberEventX[rightId];
            takeLeft = leftX < rightX || (
              leftX === rightX && (
                memberEventKind[leftId] < memberEventKind[rightId] ||
                (
                  memberEventKind[leftId] === memberEventKind[rightId] &&
                  leftId < rightId
                )
              )
            );
          }
          targetOrder[output] = takeLeft
            ? sourceOrder[left]
            : sourceOrder[right];
          left += takeLeft ? 1 : 0;
          right += takeLeft ? 0 : 1;
          output += 1;
        }
      }
      const previousSource = sourceOrder;
      sourceOrder = targetOrder;
      targetOrder = previousSource;
    }
    // restore the canonical order buffer after an odd merge count
    if (sourceOrder !== memberEventOrder) {
      // copy every sorted numeric index once
      for (let index = 0; index < memberEventCount; index += 1) {
        memberEventOrder[index] = sourceOrder[index];
      }
    }
  }

  // execute and roll back one path-coordinate sweep
  function runMemberSweep(resultCounts) {
    sortMemberEvents();
    // apply updates before equal-coordinate queries
    for (let index = 0; index < memberEventCount; index += 1) {
      const eventId = memberEventOrder[index];

      // apply one scope-subtree update
      if (memberEventKind[eventId] === 0) {
        addMemberScopeRange(
          memberEventFirst[eventId],
          memberEventSecond[eventId],
          memberEventDelta[eventId],
        );
        continue;
      }
      resultCounts[memberEventTarget[eventId]] +=
        readMemberScopePoint(memberEventFirst[eventId]) -
        readMemberScopePoint(memberEventSecond[eventId]);
    }
    // roll back only the updates applied by this sweep
    for (let index = 0; index < memberEventCount; index += 1) {
      const eventId = memberEventOrder[index];

      // reverse one scope-subtree update only
      if (memberEventKind[eventId] === 0) {
        addMemberScopeRange(
          memberEventFirst[eventId],
          memberEventSecond[eventId],
          -memberEventDelta[eventId],
        );
      }
    }
    memberEventCount = 0;
  }

  const memberFrameCapacity = Math.max(4, memberWorkCount * 2 + 4);
  const memberFrameLeft = new Int32Array(memberFrameCapacity);
  const memberFrameRight = new Int32Array(memberFrameCapacity);
  const memberFramePhase = new Uint8Array(memberFrameCapacity);
  const memberWriteBlockers = new Int32Array(memberWriteCount);
  const admittedMemberWrites = new Uint8Array(memberWriteCount);
  let memberFrameCount = 0;
  // seed one temporal admission frame
  if (memberWriteCount > 0) {
    memberFrameLeft[0] = 0;
    memberFrameRight[0] = memberWriteCount;
    memberFrameCount = 1;
  }
  // admit writes through left-complete temporal divide and conquer
  while (memberFrameCount > 0) {
    const frameIndex = memberFrameCount - 1;
    const left = memberFrameLeft[frameIndex];
    const right = memberFrameRight[frameIndex];

    // decide one leaf only after every prior contribution
    if (right - left === 1) {
      admittedMemberWrites[left] = memberWriteTraces[left].pathId !== null &&
        memberWriteBlockers[left] === 0
        ? 1
        : 0;
      memberFrameCount -= 1;
      continue;
    }
    const middle = Math.floor((left + right) / 2);

    // solve the complete left half before its sweep
    if (memberFramePhase[frameIndex] === 0) {
      memberFramePhase[frameIndex] = 1;
      memberFrameLeft[memberFrameCount] = left;
      memberFrameRight[memberFrameCount] = middle;
      memberFramePhase[memberFrameCount] = 0;
      memberFrameCount += 1;
      continue;
    }
    // load only admitted-left rectangles
    for (let index = left; index < middle; index += 1) {
      // omit one rejected or pathless member write
      if (admittedMemberWrites[index] === 0) {
        continue;
      }
      appendMemberRectangle(
        memberWriteTraces[index].pathId,
        memberWriteScopeIds[index],
      );
    }
    // load every path-bearing right candidate query
    for (let index = middle; index < right; index += 1) {
      const traced = memberWriteTraces[index];

      // omit one pathless member candidate
      if (traced.pathId === null) {
        continue;
      }
      appendMemberQuery(
        traced.pathId,
        traced.resolution.route.stopScopeId,
        traced.resolution.route.queryScopeId,
        index,
      );
    }
    runMemberSweep(memberWriteBlockers);
    memberFrameCount -= 1;
    memberFrameLeft[memberFrameCount] = middle;
    memberFrameRight[memberFrameCount] = right;
    memberFramePhase[memberFrameCount] = 0;
    memberFrameCount += 1;
  }

  const memberTimelineKind = new Uint8Array(memberWorkCount);
  const memberTimelineIndex = new Uint32Array(memberWorkCount);
  let timelineWriteIndex = 0;
  let timelineCallIndex = 0;
  let timelineCount = 0;
  // merge writes before calls at every equal source position
  while (
    timelineWriteIndex < memberWriteCount ||
    timelineCallIndex < memberCallCount
  ) {
    const useWrite = timelineWriteIndex < memberWriteCount && (
      timelineCallIndex >= memberCallCount ||
      orderedMemberWrites[timelineWriteIndex].position <=
        callDemands[timelineCallIndex].position
    );

    // append one admitted-or-rejected write position
    if (useWrite) {
      memberTimelineKind[timelineCount] = 0;
      memberTimelineIndex[timelineCount] = timelineWriteIndex;
      timelineWriteIndex += 1;
    } else {
      // append one call position
      memberTimelineKind[timelineCount] = 1;
      memberTimelineIndex[timelineCount] = timelineCallIndex;
      timelineCallIndex += 1;
    }
    timelineCount += 1;
  }

  const sequentialMemberCounts = new Int32Array(memberCallCount);
  memberFrameCount = 0;
  // seed one chronological call-query frame
  if (timelineCount > 1) {
    memberFrameLeft[0] = 0;
    memberFrameRight[0] = timelineCount;
    memberFrameCount = 1;
  }
  // count every preceding admitted write exactly once
  while (memberFrameCount > 0) {
    memberFrameCount -= 1;
    const left = memberFrameLeft[memberFrameCount];
    const right = memberFrameRight[memberFrameCount];

    // stop one indivisible temporal range
    if (right - left <= 1) {
      continue;
    }
    const middle = Math.floor((left + right) / 2);
    // load every admitted write in the left temporal half
    for (let index = left; index < middle; index += 1) {
      // omit one call position
      if (memberTimelineKind[index] !== 0) {
        continue;
      }
      const writeIndex = memberTimelineIndex[index];

      // omit one rejected member write
      if (admittedMemberWrites[writeIndex] === 0) {
        continue;
      }
      appendMemberRectangle(
        memberWriteTraces[writeIndex].pathId,
        memberWriteScopeIds[writeIndex],
      );
    }
    // load every exact call path in the right temporal half
    for (let index = middle; index < right; index += 1) {
      // omit one write position
      if (memberTimelineKind[index] !== 1) {
        continue;
      }
      const callIndex = memberTimelineIndex[index];
      const traced = memberCallTraces[callIndex];

      // omit one pathless call
      if (traced.pathId === null) {
        continue;
      }
      const route = traced.resolution.route;
      appendMemberQuery(
        traced.pathId,
        route.firstCallableId ?? route.stopScopeId,
        route.queryScopeId,
        callIndex,
      );
    }
    runMemberSweep(sequentialMemberCounts);
    memberFrameLeft[memberFrameCount] = middle;
    memberFrameRight[memberFrameCount] = right;
    memberFrameCount += 1;
    memberFrameLeft[memberFrameCount] = left;
    memberFrameRight[memberFrameCount] = middle;
    memberFrameCount += 1;
  }

  const stableMemberCounts = new Int32Array(memberCallCount);
  // load every admitted write into one all-time stable sweep
  for (let index = 0; index < memberWriteCount; index += 1) {
    // omit one rejected member write
    if (admittedMemberWrites[index] === 0) {
      continue;
    }
    appendMemberRectangle(
      memberWriteTraces[index].pathId,
      memberWriteScopeIds[index],
    );
  }
  // load every crossed-callable stable outer segment
  for (let index = 0; index < memberCallCount; index += 1) {
    const traced = memberCallTraces[index];

    // omit one pathless or wholly sequential call
    if (
      traced.pathId === null ||
      traced.resolution.route.firstCallableId === null
    ) {
      continue;
    }
    const route = traced.resolution.route;
    const callableParentId = scopeById[route.firstCallableId].parentId;

    // reject one impossible callable without a lexical parent
    if (callableParentId === null) {
      throw bootstrapError();
    }
    appendMemberQuery(
      traced.pathId,
      route.stopScopeId,
      callableParentId,
      index,
    );
  }
  runMemberSweep(stableMemberCounts);

  const callResolutionByOrdinal = new Array(nodes.length).fill(null);
  // apply exact sequential and stable member invalidations
  for (let index = 0; index < memberCallCount; index += 1) {
    const traced = memberCallTraces[index];

    // null one exact path only after complete route counting
    if (
      traced.pathId !== null &&
      sequentialMemberCounts[index] + stableMemberCounts[index] > 0
    ) {
      traced.pathId = null;
    }
    callResolutionByOrdinal[callDemands[index].callOrdinal] = traced;
  }

  // detect one provider-shaped unresolved lexical root
  function potentiallyTrackedPath(raw, route) {
    return trackedEndpointPathIds.has(raw.pathId) ||
      (route.historicalMask & raw.historicalRequiredMask) !== 0;
  }

  // detect one applicable wildcard-introduced clock candidate
  function wildcardClockCandidate(raw, route, position) {
    const scope = scopeById[route.stopScopeId];
    const count = route.stable
      ? scope.wildcardImports.length
      : upperBoundPositions(scope.wildcardPositions, position);
    const moduleMask = scope.wildcardPrefixMasks[count];
    return (moduleMask & raw.wildcardRequiredMask) !== 0;
  }


  const NODE_ROLE_IMPORT = 1;
  const NODE_ROLE_EMBEDDED = 2;
  const selectedLiteralRoles = new Uint8Array(nodes.length);

  // classify one closed node argv vector
  function nodeVectorClaim(call) {
    const positional = call.children.filter(
      // retain positional arguments only
      (child) => child[0] === "args",
    );
    let vector = positional.length === 0
      ? null
      : nodesByOrdinal.get(positional[0][2]);
    // unwrap one exact starred-only rejection witness
    if (vector?.kind === "starred") {
      const value = vector.children.find(
        // locate one starred value
        (child) => child[0] === "value" && child[1] === 0,
      );
      const outer = value === undefined ? null : nodesByOrdinal.get(value[2]);
      const entries = outer?.kind === "list"
        ? outer.children.filter(
          // retain outer list elements only
          (child) => child[0] === "elts",
        )
        : [];

      // retain one exact nested argv vector for rejection only
      if (entries.length === 1) {
        vector = nodesByOrdinal.get(entries[0][2]) ?? vector;
      }
    }
    const elements = vector?.children.filter(
      // retain vector elements only
      (child) => child[0] === "elts",
    ) ?? [];
    const first = elements.length === 0
      ? null
      : nodesByOrdinal.get(elements[0][2]);

    // ignore calls not proven to target node
    if (first?.kind !== "constant" || first.literalValue !== "node") {
      return { kind: "ordinary", payload: null };
    }
    const second = elements.length < 2
      ? null
      : nodesByOrdinal.get(elements[1][2]);

    // retain one node invocation without an eval option
    if (second === null) {
      return { kind: "ordinary", payload: null };
    }
    // retain one explicit end-options boundary
    if (second?.kind === "constant" && second.literalValue === "--") {
      return { kind: "ordinary", payload: null };
    }
    // retain one literal script operand
    if (
      second?.kind === "constant" &&
      typeof second.literalValue === "string" &&
      !second.literalValue.startsWith("-")
    ) {
      return { kind: "ordinary", payload: null };
    }
    const direct = elements.length === 3 &&
      second?.kind === "constant" && second.literalValue === "-e";
    const moduleFlag = elements.length < 3
      ? null
      : nodesByOrdinal.get(elements[2][2]);
    const module = elements.length === 4 &&
      second?.kind === "constant" &&
      second.literalValue === "--input-type=module" &&
      moduleFlag?.kind === "constant" && moduleFlag.literalValue === "-e";

    // retain one exact payload slot
    if (direct || module) {
      return {
        kind: "exact",
        payload: nodesByOrdinal.get(elements[direct ? 2 : 3][2]) ?? null,
      };
    }
    return { kind: "active-unsupported", payload: null };
  }

  // validate one exact subprocess call envelope
  function validNodeEnvelope(call, resolvedSurfaceId) {
    const positional = call.children.filter(
      // retain positional arguments only
      (child) => child[0] === "args",
    );

    // require one nonstarred positional vector
    if (
      positional.length !== 1 ||
      nodesByOrdinal.get(positional[0][2])?.kind === "starred"
    ) {
      return false;
    }
    const api = resolvedSurfaceId === SURFACE_RUN
      ? "run"
      : resolvedSurfaceId === SURFACE_CALL ||
          resolvedSurfaceId === SURFACE_CHECK_CALL
        ? "call-check-call"
        : resolvedSurfaceId === SURFACE_CHECK_OUTPUT
          ? "check-output"
          : "popen";
    const allowed = new Map([
      ["run", new Set([
        "check", "input", "capture_output", "cwd", "text", "stdout",
        "stderr", "shell",
      ])],
      ["call-check-call", new Set([
        "cwd", "text", "stdout", "stderr", "shell",
      ])],
      ["check-output", new Set([
        "input", "cwd", "text", "stderr", "shell",
      ])],
      ["popen", new Set([
        "cwd", "text", "stdout", "stderr", "shell",
      ])],
    ]).get(api);
    const values = new Map();
    const keywords = call.children.filter(
      // retain keyword arguments only
      (child) => child[0] === "keywords",
    );
    // validate every explicit keyword
    for (const child of keywords) {
      const keyword = nodesByOrdinal.get(child[2]);
      const value = keyword?.children.find(
        // locate one keyword value
        (edge) => edge[0] === "value" && edge[1] === 0,
      );

      // reject expansion duplicate unknown or missing values
      if (
        keyword?.kind !== "keyword" || keyword.identifier === null ||
        !allowed.has(keyword.identifier) || values.has(keyword.identifier) ||
        value === undefined
      ) {
        return false;
      }
      values.set(keyword.identifier, nodesByOrdinal.get(value[2]));
    }
    // require shell false when present
    if (
      values.has("shell") &&
      !(
        values.get("shell")?.kind === "constant" &&
        values.get("shell")?.literalKind === "boolean" &&
        values.get("shell")?.literalValue === "false"
      )
    ) {
      return false;
    }
    // validate exact boolean values
    for (const name of ["check", "capture_output", "text"]) {
      const value = values.get(name);

      // reject one dynamic boolean
      if (
        value !== undefined &&
        !(value.kind === "constant" && value.literalKind === "boolean")
      ) {
        return false;
      }
    }
    // validate inert scalar or name values
    for (const name of ["input", "cwd"]) {
      const value = values.get(name);

      // reject one effectful keyword value
      if (value !== undefined && !["constant", "name"].includes(value.kind)) {
        return false;
      }
    }
    // validate canonical PIPE reads
    for (const name of ["stdout", "stderr"]) {
      const value = values.get(name);

      // reject one noncanonical stream expression
      if (value !== undefined && surfaceIdentity(value.ordinal) !== SURFACE_PIPE) {
        return false;
      }
    }
    const capture = values.get("capture_output");
    // reject conflicting output ownership
    if (
      capture?.kind === "constant" && capture.literalValue === "true" &&
      (values.has("stdout") || values.has("stderr"))
    ) {
      return false;
    }
    return true;
  }

  const callDescriptors = new Map();
  const derivedLiterals = new Set();
  // resolve calls against the completed lexical history
  for (const node of orderedNodes) {
    // inspect call nodes only
    if (node.kind !== "call") {
      continue;
    }
    const position = startByOrdinal[node.ordinal];
    const func = node.children.find(
      // locate one call target
      (child) => child[0] === "func" && child[1] === 0,
    );
    const rawPath = func === undefined ? null : rawPathByOrdinal[func[2]];
    const traced = callResolutionByOrdinal[node.ordinal] ?? {
      pathId: null,
      resolution: null,
    };
    const descriptor = traced.pathId === null
      ? null
      : clockDescriptorByPathId.get(traced.pathId) ?? null;
    const resolvedSurfaceId = traced.pathId === null
      ? SURFACE_NONE
      : surfaceIdForPath(traced.pathId);

    // retain one gap for an opaque numeric receiver call
    if (func !== undefined && rawPath === null) {
      const target = nodesByOrdinal.get(func[2]);
      const receiverEdge = target?.kind === "attribute"
        ? target.children.find(
          // locate one attribute receiver
          (child) => child[0] === "value" && child[1] === 0,
        )
        : undefined;
      const receiver = receiverEdge === undefined
        ? null
        : nodesByOrdinal.get(receiverEdge[2]);

      // reject one numeric literal receiver without class inference
      if (
        receiver?.kind === "constant" &&
        ["integer", "float", "complex"].includes(receiver.literalKind)
      ) {
        unsupported = true;
      }
    }
    // retain one exact clock provider
    if (descriptor !== null) {
      callDescriptors.set(node.ordinal, descriptor);
    }
    // retain one sanitized gap for ambiguous tracked surfaces
    if (
      descriptor === null &&
      traced.pathId === null &&
      rawPath !== null &&
      (
        potentiallyTrackedPath(rawPath, traced.resolution.route) ||
        wildcardClockCandidate(rawPath, traced.resolution.route, position)
      )
    ) {
      unsupported = true;
    }
    // mark one exact importlib string argument
    if (traced.pathId === importlibImportModulePathId) {
      const argument = node.children.find(
        // locate the first positional argument
        (child) => child[0] === "args" && child[1] === 0,
      );
      const literal = argument === undefined ? null : nodesByOrdinal.get(argument[2]);
      // retain one exact import specifier
      if (literal?.kind === "constant" && literal.literalKind === "string") {
        selectedLiteralRoles[literal.ordinal] |= NODE_ROLE_IMPORT;
      }
    }
    const claim = nodeVectorClaim(node);
    const capturedApi = traced.resolution?.eventId !== null &&
      traced.resolution?.eventId !== undefined &&
      capturedApiBindingEvents.has(traced.resolution.eventId);
    // reject one tainted captured API active-e demand
    if (
      traced.pathId === null && capturedApi && subprocessSurfaceTainted &&
      claim.kind !== "ordinary"
    ) {
      unsupported = true;
    }
    // skip calls without exact subprocess API authority
    if (!subprocessApis.has(resolvedSurfaceId)) {
      continue;
    }
    // retain proven ordinary node invocations without a gap
    if (claim.kind === "ordinary") {
      continue;
    }
    // reject malformed tainted or open active-e demands
    if (
      claim.kind !== "exact" || subprocessSurfaceTainted ||
      !validNodeEnvelope(node, resolvedSurfaceId)
    ) {
      unsupported = true;
      continue;
    }
    let payload = claim.payload;
    // resolve the existing single payload name lookup
    if (payload?.kind === "name") {
      const resolution = resolveIndexedName(
        scopeByOrdinal[node.ordinal],
        internSurfaceName(payload.identifier),
        position,
      );
      payload = resolution.value.kind === "literal"
        ? nodesByOrdinal.get(resolution.value.ordinal)
        : null;
    }
    // reject one unproved active-e payload
    if (payload?.kind !== "constant" || payload.literalKind !== "string") {
      unsupported = true;
      continue;
    }
    selectedLiteralRoles[payload.ordinal] |= NODE_ROLE_EMBEDDED;
  }
  // reconcile final literal roles and caps before facts
  for (const node of nodes) {
    // skip nonconstant nodes
    if (node.kind !== "constant") {
      continue;
    }
    const selected = selectedLiteralRoles[node.ordinal];
    // reject one overlapping or incompatible selected role
    if (
      selected === (NODE_ROLE_IMPORT | NODE_ROLE_EMBEDDED) ||
      (selected !== 0 && node.literalKind !== "string")
    ) {
      throw bootstrapError();
    }
    node.literalRole = selected === NODE_ROLE_IMPORT
      ? "import-specifier"
      : selected === NODE_ROLE_EMBEDDED
        ? "embedded-javascript"
        : "semantic-constant";
    const limit = selected === NODE_ROLE_EMBEDDED
      ? DERIVED_SOURCE_BYTE_CAP
      : STRING_LITERAL_BYTE_CAP;
    // enforce the reconciled private literal cap
    if (Buffer.byteLength(node.literalValue, "utf8") > limit) {
      throw bootstrapError("frontend.bound-exceeded");
    }
    // retain one final embedded payload
    if (selected === NODE_ROLE_EMBEDDED) {
      derivedLiterals.add(node.ordinal);
    }
  }
  return { callDescriptors, defaultOrdinals, derivedLiterals, unsupported };
}

// create one native occurrence draft
function nativeDraft(fields) {
  return {
    confidence: fields.confidence,
    container: fields.container ?? null,
    countsAsClockCandidate: fields.countsAsClockCandidate,
    extractorOrigin: fields.extractorOrigin ?? null,
    kind: fields.kind,
    language: fields.language,
    normalizedExpressionSha256: fields.normalizedExpressionSha256,
    ownerSymbol: fields.ownerSymbol,
    parentOccurrenceId: fields.parentOccurrenceId ?? null,
    sourceLocator: fields.sourceLocator,
    span: fields.span,
    surface: fields.surface,
    syntaxKind: fields.syntaxKind,
  };
}

// finalize native duplicate ordinals and ids
function finalizeNative(drafts) {
  const groups = new Map();
  // group native drafts by semantic identity
  for (const draft of drafts) {
    const key = canonicalJson([
      draft.sourceLocator,
      draft.surface,
      draft.kind,
      draft.ownerSymbol,
      draft.normalizedExpressionSha256,
      draft.parentOccurrenceId,
    ]);
    const group = groups.get(key) ?? [];
    group.push(draft);
    groups.set(key, group);
  }
  const occurrences = [];
  // finalize every duplicate group
  for (const group of groups.values()) {
    group.sort(
      // order one group by exact coordinates
      (left, right) =>
        left.span.startOffsetUtf16 - right.span.startOffsetUtf16 ||
        left.span.endOffsetUtf16 - right.span.endOffsetUtf16 ||
        (left.container?.decodedSpan.startOffsetUtf16 ?? -1) -
          (right.container?.decodedSpan.startOffsetUtf16 ?? -1) ||
        (left.container?.decodedSpan.endOffsetUtf16 ?? -1) -
          (right.container?.decodedSpan.endOffsetUtf16 ?? -1),
    );
    // reject one exact semantic-coordinate tie
    for (let index = 1; index < group.length; index += 1) {
      const left = group[index - 1];
      const right = group[index];
      // fail one indistinguishable duplicate draft
      if (
        left.span.startOffsetUtf16 === right.span.startOffsetUtf16 &&
        left.span.endOffsetUtf16 === right.span.endOffsetUtf16 &&
        (left.container?.decodedSpan.startOffsetUtf16 ?? -1) ===
          (right.container?.decodedSpan.startOffsetUtf16 ?? -1) &&
        (left.container?.decodedSpan.endOffsetUtf16 ?? -1) ===
          (right.container?.decodedSpan.endOffsetUtf16 ?? -1)
      ) {
        throw new Error("binding.source-conflict");
      }
    }
    // assign one-based ordinals
    for (let index = 0; index < group.length; index += 1) {
      const draft = group[index];
      const preimage = {
        confidence: draft.confidence,
        container: draft.container,
        countsAsClockCandidate: draft.countsAsClockCandidate,
        duplicateOrdinal: index + 1,
        extractorOrigin: draft.extractorOrigin,
        identityVersion: "union-v2",
        kind: draft.kind,
        language: draft.language,
        normalizedExpressionSha256: draft.normalizedExpressionSha256,
        ownerSymbol: draft.ownerSymbol,
        parentOccurrenceId: draft.parentOccurrenceId,
        sourceLocator: draft.sourceLocator,
        span: draft.span,
        surface: draft.surface,
        syntaxKind: draft.syntaxKind,
      };
      occurrences.push({
        confidence: preimage.confidence,
        container: preimage.container,
        countsAsClockCandidate: preimage.countsAsClockCandidate,
        duplicateOrdinal: preimage.duplicateOrdinal,
        extractorOrigin: preimage.extractorOrigin,
        id: `clock-occurrence-v2:${frame("weather-clock-occurrence/v2", preimage)}`,
        identityVersion: preimage.identityVersion,
        kind: preimage.kind,
        language: preimage.language,
        normalizedExpressionSha256: preimage.normalizedExpressionSha256,
        ownerSymbol: preimage.ownerSymbol,
        parentOccurrenceId: preimage.parentOccurrenceId,
        sourceLocator: preimage.sourceLocator,
        span: preimage.span,
        surface: preimage.surface,
        syntaxKind: preimage.syntaxKind,
      });
    }
  }
  return occurrences;
}

// compare complete union occurrences
function compareOccurrences(left, right) {
  return compareText(left.sourceLocator, right.sourceLocator) ||
    left.span.startOffsetUtf16 - right.span.startOffsetUtf16 ||
    left.span.endOffsetUtf16 - right.span.endOffsetUtf16 ||
    (left.container?.decodedSpan.startOffsetUtf16 ?? -1) -
      (right.container?.decodedSpan.startOffsetUtf16 ?? -1) ||
    (left.container?.decodedSpan.endOffsetUtf16 ?? -1) -
      (right.container?.decodedSpan.endOffsetUtf16 ?? -1) ||
    compareText(left.parentOccurrenceId ?? "", right.parentOccurrenceId ?? "") ||
    compareText(left.surface, right.surface) ||
    compareText(left.kind, right.kind) ||
    compareText(left.normalizedExpressionSha256, right.normalizedExpressionSha256) ||
    left.duplicateOrdinal - right.duplicateOrdinal ||
    compareText(left.id, right.id);
}

// project one validated python file
function projectPythonFile(file, spanIndex) {
  const { nodes, source } = file;
  // represent unsupported syntax as one gap
  if (nodes.length === 0) {
    return {
      derivedSources: [],
      diagnostics: [unsupportedDiagnostic()],
      drafts: [],
      semanticFacts: [],
    };
  }
  const nodesByOrdinal = new Map();
  const parentByOrdinal = new Map();
  // index every helper node
  for (const node of nodes) {
    nodesByOrdinal.set(node.ordinal, node);
    // index every ordinary parent edge
    for (const child of node.children) {
      parentByOrdinal.set(child[2], node.ordinal);
    }
    // index every structural parent edge
    for (const association of node.associations) {
      // retain one primary parent
      if (association[4] !== null) {
        parentByOrdinal.set(association[4], node.ordinal);
      }
      // retain one secondary parent
      if (association[5] !== null) {
        parentByOrdinal.set(association[5], node.ordinal);
      }
    }
  }
  const lineMaps = buildLineMaps(source.sourceText);
  const semantics = resolvePythonSemantics(
    nodes,
    nodesByOrdinal,
    parentByOrdinal,
    source,
    lineMaps,
  );
  const projectionState = createPythonProjectionState(nodes, parentByOrdinal);
  const factIds = new Map();
  const factsByOrdinal = new Map();
  // build one dependency-ordered private fact
  function buildFact(ordinal) {
    // reuse one finished fact
    if (factsByOrdinal.has(ordinal)) {
      return factsByOrdinal.get(ordinal);
    }
    const node = nodesByOrdinal.get(ordinal);
    // skip the module transport node
    if (node === undefined || node.kind === "module") {
      return null;
    }
    // build ordinary child facts first
    for (const child of node.children) {
      buildFact(child[2]);
    }
    // build association facts first
    for (const association of node.associations) {
      // build one primary fact
      if (association[4] !== null) {
        buildFact(association[4]);
      }
      // build one secondary fact
      if (association[5] !== null) {
        buildFact(association[5]);
      }
    }
    const preimage = semanticFactPreimage(node, source, lineMaps, factIds);
    // reject one unmapped fact kind
    if (typeof preimage.factKind !== "string") {
      throw bootstrapError();
    }
    const factId = frame("weather-backend-semantic-fact/v1", preimage);
      const fact = {
        associations: preimage.associations,
        attributes: preimage.attributes,
        childEdges: preimage.childEdges,
        factId,
        factKind: preimage.factKind,
        language: preimage.language,
        site: preimage.site,
      };
    factIds.set(ordinal, factId);
    factsByOrdinal.set(ordinal, fact);
    return fact;
  }
  // build all positioned helper facts
  for (const node of nodes) {
    // skip one module transport node
    if (node.kind !== "module") {
      buildFact(node.ordinal);
    }
  }
  const semanticFacts = nodes.filter(
    // retain positioned nodes only
    (node) => node.kind !== "module",
  ).map(
    // preserve helper preorder
    (node) => factsByOrdinal.get(node.ordinal),
  );
  const drafts = [];
  // emit every native python provider
  for (const node of nodes) {
    // inspect call nodes only
    if (node.kind !== "call") {
      continue;
    }
    const descriptor = semantics.callDescriptors.get(node.ordinal) ?? null;
    // skip nonprovider calls
    if (descriptor === null) {
      continue;
    }
    const ownerSymbol = projectionState.owner(node.ordinal);
    // reject one invalid emitted owner before hashing
    if (ownerSymbol === null) {
      semantics.unsupported = true;
      continue;
    }
    const converted = convertHelperSpan(node, source, lineMaps);
    drafts.push(nativeDraft({
      confidence: "expanded-direct",
      countsAsClockCandidate: true,
      kind: descriptor.kind,
      language: "python",
      normalizedExpressionSha256: projectionState.digest(node.ordinal),
      ownerSymbol,
      sourceLocator: source.path,
      span: occurrenceSpan(spanIndex, converted.startUtf16, converted.endUtf16),
      surface: descriptor.surface,
      syntaxKind: semantics.defaultOrdinals.has(node.ordinal)
        ? "python-default-argument"
        : "python-call",
    }));
  }
  const derivedSources = [];
  // create every private literal node-e source
  for (const node of nodes) {
    // retain embedded-javascript literals only
    if (!semantics.derivedLiterals.has(node.ordinal)) {
      continue;
    }
    const ownerSymbol = projectionState.owner(node.ordinal);

    // suppress invalid public custody before hashing or derivation
    if (ownerSymbol === null) {
      semantics.unsupported = true;
      continue;
    }
    const converted = convertHelperSpan(node, source, lineMaps);
    const containerSpan = {
      endUtf16: converted.endUtf16,
      startUtf16: converted.startUtf16,
    };
    const rawSha256 = sha256(Buffer.from(node.literalValue, "utf8"));
    const derivedId = frame("weather-backend-derived-source/v1", {
      containerSpan,
      language: "javascript",
      parentPath: source.path,
      parentRawSha256: source.rawSha256,
      rawSha256,
      resolutionBase: source.resolutionBase,
    });
    derivedSources.push({
      containerSpan,
      derivedId,
      language: "javascript",
      origins: ["embedded-javascript"],
      parentPath: source.path,
      parentRawSha256: source.rawSha256,
      path: `derived:${derivedId}`,
      rawSha256,
      resolutionBase: source.resolutionBase,
      sourceKind: "derived",
      sourceText: node.literalValue,
    });
    drafts.push(nativeDraft({
      confidence: "non-provider-container",
      countsAsClockCandidate: false,
      kind: "non-provider-container",
      language: "python",
      normalizedExpressionSha256: projectionState.digest(node.ordinal),
      ownerSymbol,
      sourceLocator: source.path,
      span: occurrenceSpan(spanIndex, converted.startUtf16, converted.endUtf16),
      surface: "container.python-node-e",
      syntaxKind: "expanded-container",
    }));
  }
  return {
    derivedSources,
    diagnostics: semantics.unsupported ? [unsupportedDiagnostic()] : [],
    drafts,
    semanticFacts,
  };
}

// create one private synthetic fact
function syntheticFact({
  factKind,
  language,
  source,
  span,
  attributes,
  childEdges = [],
  associations = [],
}) {
  const detachedEdges = denseArray(childEdges, "synthetic fact child edges").map(
    // validate one exact private child edge
    (edgeValue) => {
      const edge = exactDataRecord(
        edgeValue,
        ["childFactId", "index", "role"],
        "synthetic fact child edge",
      );

      // require one closed child relation
      if (
        typeof edge.childFactId !== "string" ||
        !SHA256_PATTERN.test(edge.childFactId) ||
        !Number.isSafeInteger(edge.index) ||
        edge.index < 0 ||
        typeof edge.role !== "string" ||
        !CHILD_ROLES.has(edge.role)
      ) {
        throw bootstrapError();
      }
      return {
        childFactId: edge.childFactId,
        index: edge.index,
        role: edge.role,
      };
    },
  );
  const detachedAssociations = denseArray(
    associations,
    "synthetic fact associations",
  ).map(
    // validate one exact private association
    (associationValue) => {
      const association = denseArray(
        associationValue,
        "synthetic fact association",
      );

      // require the existing seven-slot relation schema
      if (
        association.length !== 7 ||
        typeof association[0] !== "string" ||
        !Number.isSafeInteger(association[1]) ||
        association[1] < 0 ||
        (association[2] !== null &&
          (!Number.isSafeInteger(association[2]) || association[2] < 0)) ||
        typeof association[3] !== "string" ||
        (association[4] !== null &&
          (typeof association[4] !== "string" ||
            !SHA256_PATTERN.test(association[4]))) ||
        (association[5] !== null &&
          (typeof association[5] !== "string" ||
            !SHA256_PATTERN.test(association[5]))) ||
        ![null, 0, 1].includes(association[6])
      ) {
        throw bootstrapError();
      }
      return [...association];
    },
  );
  const preimage = {
    associations: detachedAssociations,
    attributes: {
      identifier: attributes.identifier ?? null,
      importLevel: attributes.importLevel ?? null,
      literalKind: attributes.literalKind ?? null,
      literalRole: attributes.literalRole ?? "none",
      literalValue: attributes.literalValue ?? null,
      operators: attributes.operators ?? [],
    },
    childEdges: detachedEdges,
    factKind,
    language,
    site: {
      endUtf16: span.endOffsetUtf16,
      path: source.path,
      rawSha256: source.rawSha256,
      startUtf16: span.startOffsetUtf16,
    },
  };
  return {
    associations: preimage.associations,
    attributes: preimage.attributes,
    childEdges: preimage.childEdges,
    factId: frame("weather-backend-semantic-fact/v1", preimage),
    factKind: preimage.factKind,
    language: preimage.language,
    site: preimage.site,
  };
}

// tokenize one quote-aware shell source and its substitutions
function shellWords(text, baseOffset = 0) {
  const lexicon = shellCreateLexicon(text, baseOffset);
  const root = shellLexRange(lexicon, 0, text.length);
  let planCursor = 0;
  // tokenize every source-owned substitution body once
  while (planCursor < lexicon.plans.length) {
    const plan = lexicon.plans[planCursor];
    planCursor += 1;
    // retain one malformed plan without body interpretation
    if (!plan.valid) {
      plan.tokens = Object.freeze([]);
      plan.linePlans = Object.freeze([]);
      continue;
    }
    const nested = shellLexRange(lexicon, plan.bodyStart, plan.bodyEnd);
    plan.tokens = Object.freeze(nested.tokens);
    plan.unsupported ||= nested.unsupported;
  }
  shellFinalizeArithmeticPlans(lexicon, root.tokens);
  // plan lines after arithmetic metadata is complete
  for (const plan of lexicon.plans) {
    // leave one invalid plan empty
    if (!plan.valid) {
      continue;
    }
    plan.pathDescriptor = shellPathDescriptor(plan);
    plan.linePlans = Object.freeze(shellPlanLines(plan.tokens));
  }
  return {
    lexicon,
    tokens: root.tokens,
    unsupported: root.unsupported || lexicon.unsupported,
  };
}

// finalize arithmetic plan metadata from direct child references
function shellFinalizeArithmeticPlans(lexicon, rootTokens) {
  // finish children before their unique lexical parents
  for (let index = lexicon.plans.length - 1; index >= 0; index -= 1) {
    const plan = lexicon.plans[index];
    // project direct arithmetic child metadata onto atomic parts
    for (const token of plan.tokens ?? []) {
      // skip one nonword token
      if (token.kind !== "word") {
        continue;
      }
      // annotate every direct arithmetic reference once
      for (const part of token.parts) {
        // skip one nonarithmetic reference
        if (part.kind !== "arithmetic") {
          continue;
        }
        const child = lexicon.plans[part.planId];
        part.groupBalance = child.groupBalance ?? 0;
        part.mutatesParent = child.mutatesParent === true;
      }
    }
    plan.groupBalance = shellArithmeticGroupBalance(plan.tokens ?? []);
    plan.mutatesParent = shellArithmeticMutates(plan.tokens ?? []);
  }
  // annotate root-level arithmetic references after every child
  for (const token of rootTokens) {
    // skip one nonword token
    if (token.kind !== "word") {
      continue;
    }
    // annotate each direct root arithmetic reference
    for (const part of token.parts) {
      // skip one nonarithmetic reference
      if (part.kind !== "arithmetic") {
        continue;
      }
      const child = lexicon.plans[part.planId];
      part.groupBalance = child.groupBalance ?? 0;
      part.mutatesParent = child.mutatesParent === true;
    }
  }
}

// create one source-owned substitution registry
function shellCreateLexicon(text, baseOffset) {
  return {
    baseOffset,
    nextCloseBrace: shellBuildNextCloseBrace(text),
    plans: [],
    plansByStart: new Map(),
    sourcePathId: 1,
    text,
    unsupported: false,
  };
}

// index every next closing brace once
function shellBuildNextCloseBrace(text) {
  const next = new Int32Array(text.length + 1);
  let close = -1;
  // fill one reverse suffix index
  for (let index = text.length; index >= 0; index -= 1) {
    next[index] = close;
    // retain one closing brace position
    if (index > 0 && text[index - 1] === "}") {
      close = index - 1;
    }
  }
  return next;
}

const SHELL_DIAGNOSTIC_CAP = 10_000;
const SHELL_LITERAL_BYTE_CAP = 4_096;
const SHELL_INSTALLED_DEPLOY_PREFIX = "/opt/weather/current/deploy/";
const SHELL_REVIEWED_COMMANDS = new Set([
  ":", "awk", "cat", "cd", "chmod", "command", "date", "df", "die",
  "docker", "env", "eval", "exec", "exit", "export", "find", "flock", "getconf",
  "getopts", "grep", "id", "install", "local", "mapfile", "mktemp",
  "node", "nproc", "printf", "pwd", "read", "readarray", "realpath",
  "require_command", "require_file", "return", "rm", "set", "setpriv",
  "sha256sum", "shift", "sleep", "sort", "source", "ssh", "ssh-add", "stat",
  "sudo", "sync", "tail", "timeout", "tr", "trap", "umask", "unset", "wc",
]);
const SHELL_PARENT_BINDING_NOOP_COMMANDS = new Set([
  ":", "awk", "cat", "chmod", "date", "df", "dirname", "docker", "env", "exec",
  "exit", "find", "flock", "getconf", "grep", "id", "install", "ln", "mkdir",
  "mktemp", "mv", "node", "nproc", "pwd", "realpath", "return", "rm", "rmdir",
  "setpriv", "sha256sum", "sleep", "sort", "ssh", "ssh-add", "stat", "sudo",
  "sync", "tail", "timeout", "tr", "true", "wc",
]);
const SHELL_BUILTIN_FALLBACK_COMMANDS = new Set([
  ".", "cd", "command", "eval", "export", "getopts", "mapfile", "printf", "read",
  "readarray", "readonly", "set", "shift", "source", "trap", "unset",
]);
const SHELL_STATEFUL_BUILTIN_COMMANDS = new Set([
  ...SHELL_BUILTIN_FALLBACK_COMMANDS,
  "shift",
]);
const SHELL_REDIRECT_OPERATORS = new Set([
  "<", ">", ">>", "<>", ">|", "<&", ">&", "<<", "<<-", "<<<",
]);

// append one bounded scalar fragment
function shellAppendScalar(current, fragment, limit = SHELL_LITERAL_BYTE_CAP) {
  const nextBytes = Buffer.byteLength(current, "utf8") + Buffer.byteLength(fragment, "utf8");
  // enforce the selected frozen literal bound
  if (nextBytes > limit) {
    throw bootstrapError("frontend.bound-exceeded");
  }
  return current + fragment;
}

// read one shell operator
function shellOperatorAt(text, index) {
  const operators = [
    ";;&", "<<<", "<<-", "&&", "||", ";;", ";&", "|&", ">>", "<<", "<&",
    ">&", "<>", ">|", "[[", "]]", "((", "))", ";", "|", "<", ">", "(",
    ")", "{", "}",
  ];
  // select the longest operator
  for (const operator of operators) {
    // retain one exact operator
    if (text.startsWith(operator, index)) {
      return operator;
    }
  }
  return null;
}

// append one word text part
function shellAppendTextPart(parts, value, quote, start, end) {
  // retain one completed text atom
  parts.push({ end, kind: "text", quote, start, value });
}

// create one source-owned substitution plan
function shellCreateSubstitutionPlan(lexicon, start, kind, openerWidth, parentId) {
  const existing = lexicon.plansByStart.get(start);
  // reuse one previously indexed opener
  if (existing !== undefined) {
    return lexicon.plans[existing];
  }
  const id = lexicon.plans.length;
  const plan = {
    bodyEnd: -1,
    bodyStart: start + openerWidth,
    childPlanIds: [],
    end: -1,
    id,
    kind,
    linePlans: null,
    parentId,
    start,
    tokens: null,
    unsupported: false,
    valid: false,
  };
  lexicon.plans.push(plan);
  lexicon.plansByStart.set(start, id);
  // retain one direct immutable ownership edge
  if (parentId !== null) {
    lexicon.plans[parentId].childPlanIds.push(id);
  }
  return plan;
}

// index one literal heredoc delimiter without interpreting it
function shellIndexHeredocDelimiter(text, start) {
  const stripTabs = text.startsWith("<<-", start);
  let cursor = start + (stripTabs ? 3 : 2);
  // consume horizontal delimiter spacing
  while (cursor < text.length && /[ \t]/u.test(text[cursor])) {
    cursor += 1;
  }
  const delimiterStart = cursor;
  let delimiter = "";
  let quote = null;
  // consume one literal delimiter word
  while (cursor < text.length) {
    const character = text[cursor];
    // close one delimiter quote
    if (quote !== null && character === quote) {
      quote = null;
      cursor += 1;
      continue;
    }
    // retain one quoted delimiter character
    if (quote !== null) {
      delimiter += character;
      cursor += 1;
      continue;
    }
    // open one delimiter quote
    if (character === "'" || character === "\"") {
      quote = character;
      cursor += 1;
      continue;
    }
    // finish one delimiter word
    if (/\s/u.test(character) || shellOperatorAt(text, cursor) !== null) {
      break;
    }
    // reject one expanded or escaped delimiter
    if (character === "$" || character === "`" || character === "\\") {
      return null;
    }
    delimiter += character;
    cursor += 1;
  }
  // reject one absent or unterminated literal delimiter
  if (delimiter.length === 0 || quote !== null || cursor === delimiterStart) {
    return null;
  }
  return { cursor, delimiter, quoted: delimiterStart !== cursor - delimiter.length, stripTabs };
}

// index one balanced substitution tree iteratively
function shellIndexSubstitution(lexicon, start, kind) {
  const existing = lexicon.plansByStart.get(start);
  // reuse one already indexed source opener
  if (existing !== undefined) {
    return lexicon.plans[existing];
  }
  const openerWidth = kind === "arithmetic" ? 3 : 1 + (kind === "command" ? 1 : 0);
  const root = shellCreateSubstitutionPlan(lexicon, start, kind, openerWidth, null);
  const stack = [{ ordinaryParenDepth: 0, pendingHeredocs: [], plan: root, quote: null }];
  let cursor = root.bodyStart;
  // consume each owned source unit under only its active frame
  while (cursor < lexicon.text.length && stack.length > 0) {
    const frame = stack.at(-1);
    const character = lexicon.text[cursor];
    // skip every frame-owned heredoc body before delimiter scanning resumes
    if (character === "\n" && frame.quote === null && frame.pendingHeredocs.length > 0) {
      let afterBodies = cursor + 1;
      // consume each declared heredoc body in source order
      for (const descriptor of frame.pendingHeredocs) {
        const scanned = shellScanHeredoc(
          lexicon.text,
          afterBodies,
          lexicon.text.length,
          descriptor,
          0,
        );
        // fail closed on one unterminated heredoc body
        if (scanned === null) {
          lexicon.unsupported = true;
          frame.plan.unsupported = true;
          afterBodies = lexicon.text.length;
          break;
        }
        afterBodies = scanned.end;
      }
      frame.pendingHeredocs.length = 0;
      cursor = afterBodies;
      continue;
    }
    // index one command heredoc delimiter before its body
    if (
      frame.quote === null &&
      ["command", "legacy-command"].includes(frame.plan.kind) &&
      !lexicon.text.startsWith("<<<", cursor) &&
      (lexicon.text.startsWith("<<-", cursor) ||
        lexicon.text.startsWith("<<", cursor))
    ) {
      const descriptor = shellIndexHeredocDelimiter(lexicon.text, cursor);
      // fail closed on one unindexed delimiter
      if (descriptor === null) {
        lexicon.unsupported = true;
        frame.plan.unsupported = true;
        cursor += 2;
      } else {
        frame.pendingHeredocs.push(descriptor);
        cursor = descriptor.cursor;
      }
      continue;
    }
    // consume one legacy substitution escape or close
    if (frame.plan.kind === "legacy-command") {
      // skip one escaped legacy character
      if (character === "\\" && cursor + 1 < lexicon.text.length) {
        cursor += 2;
        continue;
      }
      // close one legacy command body
      if (character === "`") {
        frame.plan.bodyEnd = cursor;
        frame.plan.end = cursor + 1;
        frame.plan.valid = true;
        stack.pop();
        cursor += 1;
        continue;
      }
    } else {
      // consume one single-quoted region without expansions
      if (frame.quote === "'") {
        // close one single quote
        if (character === "'") {
          frame.quote = null;
        }
        cursor += 1;
        continue;
      }
      // consume one escaped double-quote character
      if (frame.quote === "\"" && character === "\\" && cursor + 1 < lexicon.text.length) {
        cursor += 2;
        continue;
      }
      // close one double quote
      if (frame.quote === "\"" && character === "\"") {
        frame.quote = null;
        cursor += 1;
        continue;
      }
      // open one frame-local quote
      if (frame.quote === null && (character === "'" || character === "\"")) {
        frame.quote = character;
        cursor += 1;
        continue;
      }
      // consume one escaped character
      if (character === "\\" && cursor + 1 < lexicon.text.length) {
        cursor += 2;
        continue;
      }
      // close one outer arithmetic body
      if (
        frame.quote === null &&
        frame.plan.kind === "arithmetic" &&
        frame.ordinaryParenDepth === 0 &&
        lexicon.text.startsWith("))", cursor)
      ) {
        frame.plan.bodyEnd = cursor;
        frame.plan.end = cursor + 2;
        frame.plan.valid = true;
        stack.pop();
        cursor += 2;
        continue;
      }
      // enter one ordinary arithmetic group
      if (
        frame.quote === null &&
        frame.plan.kind === "arithmetic" &&
        character === "("
      ) {
        frame.ordinaryParenDepth += 1;
        cursor += 1;
        continue;
      }
      // leave one ordinary arithmetic group
      if (
        frame.quote === null &&
        frame.plan.kind === "arithmetic" &&
        character === ")" &&
        frame.ordinaryParenDepth > 0
      ) {
        frame.ordinaryParenDepth -= 1;
        cursor += 1;
        continue;
      }
      // reject one unmatched arithmetic close
      if (
        frame.quote === null &&
        frame.plan.kind === "arithmetic" &&
        character === ")"
      ) {
        lexicon.unsupported = true;
        frame.plan.unsupported = true;
        cursor += 1;
        continue;
      }
      // close one command body
      if (
        frame.quote === null &&
        frame.plan.kind === "command" &&
        character === ")"
      ) {
        frame.plan.bodyEnd = cursor;
        frame.plan.end = cursor + 1;
        frame.plan.valid = true;
        stack.pop();
        cursor += 1;
        continue;
      }
    }
    // open one nested arithmetic body
    if (lexicon.text.startsWith("$((", cursor)) {
      const child = shellCreateSubstitutionPlan(
        lexicon,
        cursor,
        "arithmetic",
        3,
        frame.plan.id,
      );
      stack.push({ ordinaryParenDepth: 0, pendingHeredocs: [], plan: child, quote: null });
      cursor += 3;
      continue;
    }
    // open one nested command body
    if (lexicon.text.startsWith("$(", cursor)) {
      const child = shellCreateSubstitutionPlan(
        lexicon,
        cursor,
        "command",
        2,
        frame.plan.id,
      );
      stack.push({ ordinaryParenDepth: 0, pendingHeredocs: [], plan: child, quote: null });
      cursor += 2;
      continue;
    }
    // open one nested legacy body
    if (character === "`") {
      const child = shellCreateSubstitutionPlan(
        lexicon,
        cursor,
        "legacy-command",
        1,
        frame.plan.id,
      );
      stack.push({ ordinaryParenDepth: 0, pendingHeredocs: [], plan: child, quote: null });
      cursor += 1;
      continue;
    }
    cursor += 1;
  }
  // mark every unclosed frame as unsupported once
  if (stack.length > 0) {
    lexicon.unsupported = true;
    // close every incomplete plan at the source boundary
    for (const frame of stack) {
      frame.plan.bodyEnd = lexicon.text.length;
      frame.plan.end = lexicon.text.length;
      frame.plan.unsupported = true;
    }
  }
  return root;
}

// retain every failed recognition state
function shellRememberFailedWordStates(failedWordStates, wordStatePath) {
  // publish only states visited by one failed candidate
  for (const stateKey of wordStatePath) {
    failedWordStates.add(stateKey);
  }
}

// recognize one word without decoded allocations
function shellRecognizeWord(
  lexicon,
  start,
  end,
  failedWordStates,
  wordStatePath,
) {
  const { text } = lexicon;
  let cursor = start;
  let quote = null;
  let sawSyntax = false;
  wordStatePath.length = 0;
  // consume one finite recognition path
  while (cursor < end) {
    const quoteCode = quote === "single" ? 1 : quote === "double" ? 2 : 0;
    const stateKey = cursor * 3 + quoteCode;
    // stop on one known failed suffix state
    if (failedWordStates.has(stateKey)) {
      shellRememberFailedWordStates(failedWordStates, wordStatePath);
      return null;
    }
    wordStatePath.push(stateKey);
    const character = text[cursor];
    // consume one single-quoted unit
    if (quote === "single") {
      // close one single quote
      if (character === "'") {
        quote = null;
        sawSyntax = true;
      }
      cursor += 1;
      continue;
    }
    // consume one double-quoted unit
    if (quote === "double") {
      // close one double quote
      if (character === "\"") {
        quote = null;
        sawSyntax = true;
        cursor += 1;
        continue;
      }
      // consume one admitted double-quote escape atomically
      if (character === "\\" && cursor + 1 < end) {
        const escaped = text[cursor + 1];
        // skip one decoded or continued escape
        if (escaped === "\n" || ["$", "`", "\"", "\\"].includes(escaped)) {
          cursor += 2;
          continue;
        }
      }
    } else {
      // finish at one unquoted separator
      if (/\s/u.test(character) || shellOperatorAt(text, cursor) !== null) {
        break;
      }
      // open one single quote
      if (character === "'") {
        quote = "single";
        sawSyntax = true;
        cursor += 1;
        continue;
      }
      // open one double quote
      if (character === "\"") {
        quote = "double";
        sawSyntax = true;
        cursor += 1;
        continue;
      }
      // consume one unquoted escape atomically
      if (character === "\\" && cursor + 1 < end) {
        sawSyntax = true;
        cursor += 2;
        continue;
      }
    }
    // consume one parameter or substitution atomically
    if (character === "$") {
      // consume one arithmetic substitution
      if (text.startsWith("$((", cursor)) {
        const scanned = shellIndexSubstitution(lexicon, cursor, "arithmetic");
        // reject one unterminated arithmetic form
        if (!scanned.valid || scanned.end > end) {
          shellRememberFailedWordStates(failedWordStates, wordStatePath);
          return null;
        }
        cursor = scanned.end;
        sawSyntax = true;
        continue;
      }
      // consume one command substitution
      if (text.startsWith("$(", cursor)) {
        const scanned = shellIndexSubstitution(lexicon, cursor, "command");
        // reject one unterminated command form
        if (!scanned.valid || scanned.end > end) {
          shellRememberFailedWordStates(failedWordStates, wordStatePath);
          return null;
        }
        cursor = scanned.end;
        sawSyntax = true;
        continue;
      }
      // consume one indexed braced parameter
      if (text[cursor + 1] === "{") {
        const close = lexicon.nextCloseBrace[cursor + 2] ?? -1;
        // reject one unterminated parameter
        if (close === -1 || close >= end) {
          shellRememberFailedWordStates(failedWordStates, wordStatePath);
          return null;
        }
        cursor = close + 1;
        sawSyntax = true;
        continue;
      }
      let nameEnd = cursor + 1;
      const firstCode = nameEnd < end ? text.charCodeAt(nameEnd) : -1;
      // consume one ascii identifier
      if (
        nameEnd < end &&
        (text[nameEnd] === "_" || firstCode >= 65 && firstCode <= 90 ||
          firstCode >= 97 && firstCode <= 122)
      ) {
        nameEnd += 1;
        // consume one identifier tail
        while (nameEnd < end) {
          const code = text.charCodeAt(nameEnd);
          // stop before one nonidentifier byte
          if (!(text[nameEnd] === "_" || code >= 48 && code <= 57 ||
            code >= 65 && code <= 90 || code >= 97 && code <= 122)) {
            break;
          }
          nameEnd += 1;
        }
        cursor = nameEnd;
      } else if (nameEnd < end && /[0-9@*#?$!-]/u.test(text[nameEnd])) {
        // consume one special parameter
        cursor += 2;
      } else {
        // consume one unknown dollar atom
        cursor += 1;
      }
      sawSyntax = true;
      continue;
    }
    // consume one legacy command substitution
    if (character === "`") {
      const scanned = shellIndexSubstitution(lexicon, cursor, "legacy-command");
      // reject one unterminated legacy form
      if (!scanned.valid || scanned.end > end) {
        shellRememberFailedWordStates(failedWordStates, wordStatePath);
        return null;
      }
      cursor = scanned.end;
      sawSyntax = true;
      continue;
    }
    cursor += 1;
  }
  // reject one unterminated quote
  if (quote !== null) {
    shellRememberFailedWordStates(failedWordStates, wordStatePath);
    return null;
  }
  return { end: cursor, sawSyntax };
}

// materialize one already-recognized word
function shellMaterializeWord(lexicon, start, end, sawSyntax) {
  const { baseOffset, text } = lexicon;
  const parts = [];
  let cursor = start;
  let quote = null;
  // consume one successful word into maximal source runs
  while (cursor < end) {
    const character = text[cursor];
    // consume one quote boundary
    if (quote === "single" && character === "'") {
      quote = null;
      cursor += 1;
      continue;
    }
    // consume one double quote boundary
    if (quote === "double" && character === "\"") {
      quote = null;
      cursor += 1;
      continue;
    }
    // open one unquoted quote boundary
    if (quote === null && (character === "'" || character === "\"")) {
      quote = character === "'" ? "single" : "double";
      cursor += 1;
      continue;
    }
    // retain one maximal single-quoted literal run
    if (quote === "single") {
      const runStart = cursor;
      // stop only at the closing apostrophe
      while (cursor < end && text[cursor] !== "'") {
        cursor += 1;
      }
      shellAppendTextPart(
        parts,
        text.slice(runStart, cursor),
        "single",
        baseOffset + runStart,
        baseOffset + cursor,
      );
      continue;
    }
    // consume one admitted escape
    if (character === "\\" && cursor + 1 < end) {
      const escaped = text[cursor + 1];
      const admitted = quote === null || escaped === "\n" ||
        ["$", "`", "\"", "\\"].includes(escaped);
      // retain one decoded escape only when admitted
      if (admitted) {
        // omit one logical continuation
        if (escaped !== "\n") {
          shellAppendTextPart(
            parts,
            escaped,
            quote ?? "unquoted",
            baseOffset + cursor,
            baseOffset + cursor + 2,
          );
        }
        cursor += 2;
        continue;
      }
    }
    // consume one parameter or substitution
    if (character === "$") {
      const partStart = cursor;
      const partQuote = quote ?? "unquoted";
      // retain one arithmetic substitution
      if (text.startsWith("$((", cursor)) {
        const scanned = shellIndexSubstitution(lexicon, cursor, "arithmetic");
        parts.push({
          end: baseOffset + scanned.end,
          kind: "arithmetic",
          planId: scanned.id,
          quote: partQuote,
          start: baseOffset + partStart,
        });
        cursor = scanned.end;
        continue;
      }
      // retain one command substitution
      if (text.startsWith("$(", cursor)) {
        const scanned = shellIndexSubstitution(lexicon, cursor, "command");
        parts.push({
          end: baseOffset + scanned.end,
          kind: "command",
          planId: scanned.id,
          quote: partQuote,
          start: baseOffset + partStart,
        });
        cursor = scanned.end;
        continue;
      }
      // retain one braced parameter
      if (text[cursor + 1] === "{") {
        const close = lexicon.nextCloseBrace[cursor + 2];
        parts.push({
          end: baseOffset + close + 1,
          kind: "parameter",
          name: text.slice(cursor + 2, close),
          quote: partQuote,
          start: baseOffset + partStart,
        });
        cursor = close + 1;
        continue;
      }
      let nameEnd = cursor + 1;
      const firstCode = nameEnd < end ? text.charCodeAt(nameEnd) : -1;
      // retain one named parameter
      if (
        nameEnd < end &&
        (text[nameEnd] === "_" || firstCode >= 65 && firstCode <= 90 ||
          firstCode >= 97 && firstCode <= 122)
      ) {
        nameEnd += 1;
        // consume one identifier tail
        while (nameEnd < end) {
          const code = text.charCodeAt(nameEnd);
          // stop before one nonidentifier byte
          if (!(text[nameEnd] === "_" || code >= 48 && code <= 57 ||
            code >= 65 && code <= 90 || code >= 97 && code <= 122)) {
            break;
          }
          nameEnd += 1;
        }
        parts.push({
          end: baseOffset + nameEnd,
          kind: "parameter",
          name: text.slice(cursor + 1, nameEnd),
          quote: partQuote,
          start: baseOffset + partStart,
        });
        cursor = nameEnd;
        continue;
      }
      const special = cursor + 1 < end ? text[cursor + 1] : undefined;
      // retain one special parameter
      if (special !== undefined && /[0-9@*#?$!-]/u.test(special)) {
        parts.push({
          end: baseOffset + cursor + 2,
          kind: "parameter",
          name: special,
          quote: partQuote,
          start: baseOffset + partStart,
        });
        cursor += 2;
        continue;
      }
      parts.push({
        end: baseOffset + cursor + 1,
        kind: "unknown",
        quote: partQuote,
        start: baseOffset + cursor,
      });
      cursor += 1;
      continue;
    }
    // retain one legacy substitution
    if (character === "`") {
      const scanned = shellIndexSubstitution(lexicon, cursor, "legacy-command");
      parts.push({
        end: baseOffset + scanned.end,
        kind: "command",
        planId: scanned.id,
        quote: quote ?? "unquoted",
        start: baseOffset + cursor,
      });
      cursor = scanned.end;
      continue;
    }
    const runStart = cursor;
    // extend one raw text run
    while (cursor < end) {
      const next = text[cursor];
      // stop before one syntax boundary
      if (next === "$" || next === "`" || next === "\\" ||
        quote === null && (next === "'" || next === "\"") ||
        quote === "single" && next === "'" ||
        quote === "double" && next === "\"") {
        break;
      }
      cursor += 1;
    }
    // guarantee one raw unit progresses
    if (cursor === runStart) {
      cursor += 1;
    }
    shellAppendTextPart(
      parts,
      text.slice(runStart, cursor),
      quote ?? "unquoted",
      baseOffset + runStart,
      baseOffset + cursor,
    );
  }
  // retain one empty quoted word
  if (parts.length === 0 && sawSyntax) {
    parts.push({
      end: baseOffset + end,
      kind: "text",
      quote: "quoted-empty",
      start: baseOffset + start,
      value: "",
    });
  }
  const materializedParts = [];
  let pendingText = null;
  // coalesce completed adjacent text atoms without prefix concatenation
  for (const part of parts) {
    // extend one same-mode raw-adjacent text part
    if (
      part.kind === "text" && pendingText !== null &&
      pendingText.quote === part.quote && pendingText.end === part.start
    ) {
      pendingText.end = part.end;
      pendingText.values.push(part.value);
      continue;
    }
    // flush one completed text part before a boundary
    if (pendingText !== null) {
      materializedParts.push({
        end: pendingText.end,
        kind: "text",
        quote: pendingText.quote,
        start: pendingText.start,
        value: pendingText.values.join(""),
      });
      pendingText = null;
    }
    // start one completed text part
    if (part.kind === "text") {
      pendingText = {
        end: part.end,
        quote: part.quote,
        start: part.start,
        values: [part.value],
      };
    } else {
      // retain one semantic word part boundary
      materializedParts.push(part);
    }
  }
  // flush one final completed text part
  if (pendingText !== null) {
    materializedParts.push({
      end: pendingText.end,
      kind: "text",
      quote: pendingText.quote,
      start: pendingText.start,
      value: pendingText.values.join(""),
    });
  }
  return {
    end: baseOffset + end,
    kind: "word",
    parts: materializedParts,
    start: baseOffset + start,
  };
}

// scan one shell word with finite failed-state ownership
function shellScanWord(
  lexicon,
  start,
  end,
  failedWordStates = new Set(),
  wordStatePath = [],
) {
  const recognized = shellRecognizeWord(
    lexicon,
    start,
    end,
    failedWordStates,
    wordStatePath,
  );
  // stop after one failed recognition
  if (recognized === null) {
    return null;
  }
  return shellMaterializeWord(lexicon, start, recognized.end, recognized.sawSyntax);
}

// decode one literal-only word
function shellDirectTextExceeds(word, limit = SHELL_LITERAL_BYTE_CAP) {
  let bytes = 0;
  // count direct text without materializing it
  for (const part of word.parts) {
    // ignore expansions so their amplification remains a hard resolver error
    if (part.kind !== "text") {
      continue;
    }
    bytes += Buffer.byteLength(part.value, "utf8");
    // stop once one ordinary word exceeds its fact payload bound
    if (bytes > limit) {
      return true;
    }
  }
  return false;
}

// resolve one literal only when it fits an ordinary fact payload
function shellOptionalLiteralWord(word, limit = SHELL_LITERAL_BYTE_CAP) {
  // leave oversized direct text opaque
  if (shellDirectTextExceeds(word, limit)) {
    return null;
  }
  return shellLiteralWord(word, limit);
}

// resolve one parameter-free literal word
function shellLiteralWord(word, limit = SHELL_LITERAL_BYTE_CAP) {
  let value = "";
  // collect literal parts only
  for (const part of word.parts) {
    // reject every expansion
    if (part.kind !== "text") {
      return null;
    }
    value = shellAppendScalar(value, part.value, limit);
  }
  return value;
}

// scan one heredoc body without interpretation
function shellScanHeredoc(text, start, end, descriptor, baseOffset) {
  let cursor = start;
  const bodyStart = cursor;
  // inspect every physical heredoc line
  while (cursor <= end) {
    const newline = text.indexOf("\n", cursor);
    const lineEnd = newline === -1 || newline > end ? end : newline;
    const line = text.slice(cursor, lineEnd);
    const compared = descriptor.stripTabs ? line.replace(/^\t+/u, "") : line;
    // retain one exact terminator
    if (compared === descriptor.delimiter) {
      return {
        bodyEnd: baseOffset + cursor,
        bodyStart: baseOffset + bodyStart,
        end: baseOffset + (newline === -1 || newline >= end ? lineEnd : lineEnd + 1),
      };
    }
    // stop after one final unterminated line
    if (newline === -1 || newline >= end) {
      break;
    }
    cursor = newline + 1;
  }
  return null;
}

// tokenize one bounded logical shell plan range
function shellLexRange(lexicon, rangeStart, rangeEnd) {
  const { baseOffset, text } = lexicon;
  const tokens = [];
  const pendingHeredocs = [];
  let expectedHeredoc = null;
  let cursor = rangeStart;
  let unsupported = false;
  const failedWordStates = new Set();
  const wordStatePath = [];
  // scan source left to right
  while (cursor < rangeEnd) {
    const character = text[cursor];
    // consume horizontal whitespace
    if (character !== "\n" && /\s/u.test(character)) {
      cursor += 1;
      continue;
    }
    // consume one logical newline and pending bodies
    if (character === "\n") {
      let afterBodies = cursor + 1;
      const heredocTokens = [];
      // consume every declared heredoc body
      for (const descriptor of pendingHeredocs) {
        const scanned = shellScanHeredoc(
          text,
          afterBodies,
          rangeEnd,
          descriptor,
          baseOffset,
        );
        // mark one malformed heredoc
        if (scanned === null) {
          unsupported = true;
          afterBodies = rangeEnd;
          heredocTokens.push({
            bodyEnd: baseOffset + rangeEnd,
            bodyStart: baseOffset + afterBodies,
            end: baseOffset + rangeEnd,
            kind: "heredoc",
            start: descriptor.start,
          });
          break;
        }
        heredocTokens.push({
          ...scanned,
          delimiter: descriptor.delimiter,
          kind: "heredoc",
          quoted: descriptor.quoted,
          start: descriptor.start,
          stripTabs: descriptor.stripTabs,
        });
        afterBodies = scanned.end - baseOffset;
      }
      appendValues(tokens, heredocTokens);
      tokens.push({
        end: baseOffset + afterBodies,
        kind: "newline",
        start: baseOffset + cursor,
      });
      pendingHeredocs.length = 0;
      expectedHeredoc = null;
      cursor = afterBodies;
      continue;
    }
    // remove one standalone logical continuation
    if (character === "\\" && text[cursor + 1] === "\n") {
      cursor += 2;
      continue;
    }
    // discard one unquoted comment
    if (character === "#") {
      const newline = text.indexOf("\n", cursor);
      cursor = newline === -1 || newline > rangeEnd ? rangeEnd : newline;
      continue;
    }
    const operator = shellOperatorAt(text, cursor);
    // retain one operator
    if (operator !== null) {
      const token = {
        end: baseOffset + cursor + operator.length,
        kind: "operator",
        start: baseOffset + cursor,
        value: operator,
      };
      tokens.push(token);
      // await one heredoc delimiter
      if (operator === "<<" || operator === "<<-") {
        expectedHeredoc = {
          start: token.start,
          stripTabs: operator === "<<-",
        };
      }
      cursor += operator.length;
      continue;
    }
    const word = shellScanWord(
      lexicon,
      cursor,
      rangeEnd,
      failedWordStates,
      wordStatePath,
    );
    // reject one malformed word
    if (word === null || word.end === baseOffset + cursor) {
      unsupported = true;
      cursor += 1;
      continue;
    }
    tokens.push(word);
    // bind one pending heredoc delimiter
    if (expectedHeredoc !== null) {
      const delimiter = shellLiteralWord(word);
      // retain one exact delimiter
      if (delimiter !== null && delimiter.length > 0) {
        pendingHeredocs.push({
          ...expectedHeredoc,
          delimiter,
          quoted: word.parts.some(
            // locate one quoted delimiter part
            (part) => part.quote !== "unquoted",
          ),
        });
      } else {
        unsupported = true;
      }
      expectedHeredoc = null;
    }
    cursor = word.end - baseOffset;
  }
  // retain one final logical boundary
  if (tokens.at(-1)?.kind !== "newline") {
    tokens.push({
      end: baseOffset + rangeEnd,
      kind: "newline",
      start: baseOffset + rangeEnd,
    });
  }
  // reject one dangling heredoc declaration
  if (pendingHeredocs.length > 0 || expectedHeredoc !== null) {
    unsupported = true;
  }
  return { tokens, unsupported };
}

// normalize one contained posix path
function shellNormalizePath(value) {
  const absolute = value.startsWith("/");
  const segments = [];
  // reduce every path segment
  for (const segment of value.split("/")) {
    // skip empty and current segments
    if (segment.length === 0 || segment === ".") {
      continue;
    }
    // consume one parent segment
    if (segment === "..") {
      // reject one escaping path
      if (segments.length === 0) {
        return null;
      }
      segments.pop();
      continue;
    }
    segments.push(segment);
  }
  return `${absolute ? "/" : ""}${segments.join("/")}`;
}

// read one scalar environment value
function shellEnvironmentScalar(environment, name, source) {
  // expose the current physical source path
  if (name === "BASH_SOURCE[0]") {
    return source.path;
  }
  const value = shellEnvironmentGet(environment, name);
  return typeof value === "string" ? value : null;
}

// create one pass-local reviewed path memo
function shellCreatePathResolution(lexicon) {
  return {
    lexicon,
    states: new Uint8Array(lexicon.plans.length),
    tokens: Array(lexicon.plans.length).fill(null),
    values: Array(lexicon.plans.length).fill(null),
  };
}

// capture one constant effective environment token
function shellPathResolutionToken(resolution, environment) {
  return {
    barrierId: environment.barrierId,
    barrierTop: environment.barrierTop,
    overrideRoot: environment.overrideRoot,
    registryRef: environment.registry,
    root: environment.root,
    sourcePathId: resolution.lexicon.sourcePathId,
    topRoot: environment.topRoot,
  };
}

// compare one constant path-resolution token
function shellPathTokensEqual(left, right) {
  return left !== null && right !== null &&
    left.barrierId === right.barrierId &&
    left.barrierTop === right.barrierTop &&
    left.overrideRoot === right.overrideRoot &&
    left.registryRef === right.registryRef &&
    left.root === right.root &&
    left.sourcePathId === right.sourcePathId &&
    left.topRoot === right.topRoot;
}

// classify one direct reviewed path command
function shellPathDescriptor(plan) {
  const words = [];
  const operators = [];
  // inspect direct immutable tokens only
  for (const token of plan.tokens ?? []) {
    // retain one direct word
    if (token.kind === "word") {
      words.push(token);
      continue;
    }
    // retain one direct command operator
    if (token.kind === "operator") {
      operators.push(token.value);
    }
  }
  // retain one exact dirname descriptor
  if (
    words.length === 2 &&
    operators.length === 0 &&
    shellOptionalLiteralWord(words[0]) === "dirname"
  ) {
    return Object.freeze({
      argument: words[1],
      childPlanIds: Object.freeze(words[1].parts.filter(
        // retain one direct path child
        (part) => part.kind === "command",
      ).map(
        // project one direct path child id
        (part) => part.planId,
      )),
      kind: "dirname",
    });
  }
  // retain one exact cd-and-pwd descriptor
  if (
    words.length === 3 &&
    operators.length === 1 &&
    operators[0] === "&&" &&
    shellOptionalLiteralWord(words[0]) === "cd" &&
    shellOptionalLiteralWord(words[2]) === "pwd"
  ) {
    return Object.freeze({
      argument: words[1],
      childPlanIds: Object.freeze(words[1].parts.filter(
        // retain one direct path child
        (part) => part.kind === "command",
      ).map(
        // project one direct path child id
        (part) => part.planId,
      )),
      kind: "cd-pwd",
    });
  }
  return null;
}

// resolve one word from completed direct child path plans
function shellResolvePathWord(word, source, environment, resolution, token) {
  let result = "";
  // consume every immutable word part once
  for (const part of word.parts) {
    // retain one literal fragment
    if (part.kind === "text") {
      // reject one unquoted expansion boundary
      if (part.quote === "unquoted" && /[*?[]/u.test(part.value)) {
        return null;
      }
      result = shellAppendScalar(result, part.value);
      continue;
    }
    // retain one exact scalar parameter
    if (part.kind === "parameter") {
      const replacement = shellEnvironmentScalar(environment, part.name, source);
      // reject one unresolved or splitting parameter
      if (
        replacement === null ||
        (part.quote === "unquoted" && /[\s*?[]/u.test(replacement))
      ) {
        return null;
      }
      result = shellAppendScalar(result, replacement);
      continue;
    }
    // retain one already completed direct child plan
    if (part.kind === "command") {
      const childToken = resolution.tokens[part.planId];
      const replacement = resolution.states[part.planId] === 2 &&
        shellPathTokensEqual(childToken, token)
        ? resolution.values[part.planId]
        : null;
      // reject one unresolved or splitting child result
      if (
        replacement === null ||
        (part.quote === "unquoted" && /[\s*?[]/u.test(replacement))
      ) {
        return null;
      }
      result = shellAppendScalar(result, replacement);
      continue;
    }
    return null;
  }
  return result;
}

// resolve one reviewed path substitution iteratively
function shellResolvePathSubstitution(part, source, environment, resolution) {
  // reject one unavailable plan registry
  if (resolution === null || part.planId === undefined) {
    return null;
  }
  const token = shellPathResolutionToken(resolution, environment);
  const priorToken = resolution.tokens[part.planId];
  // reject one cross-environment memo use before child access
  if (priorToken !== null && !shellPathTokensEqual(priorToken, token)) {
    return null;
  }
  const stack = [{ entered: false, planId: part.planId }];
  // evaluate every direct dependency edge once
  while (stack.length > 0) {
    const frame = stack.at(-1);
    const planId = frame.planId;
    // finish one already completed frame
    if (resolution.states[planId] === 2 || resolution.states[planId] === 3) {
      stack.pop();
      continue;
    }
    const plan = resolution.lexicon.plans[planId];
    // enter one reviewed direct descriptor
    if (!frame.entered) {
      frame.entered = true;
      // reject one malformed or token-mismatched plan
      if (
        plan === undefined ||
        !plan.valid ||
        plan.unsupported ||
        (resolution.tokens[planId] !== null &&
          !shellPathTokensEqual(resolution.tokens[planId], token))
      ) {
        resolution.states[planId] = 3;
        resolution.tokens[planId] = token;
        continue;
      }
      resolution.states[planId] = 1;
      resolution.tokens[planId] = token;
      frame.descriptor = plan.pathDescriptor;
      // reject one other command grammar without descending
      if (frame.descriptor === null) {
        resolution.states[planId] = 3;
        continue;
      }
      frame.children = frame.descriptor.childPlanIds;
      frame.childIndex = 0;
    }
    // evaluate one unseen direct child first
    if (frame.childIndex < frame.children.length) {
      const childId = frame.children[frame.childIndex];
      frame.childIndex += 1;
      const childToken = resolution.tokens[childId];
      // fail one environment mismatch or cycle transactionally
      if (
        (childToken !== null && !shellPathTokensEqual(childToken, token)) ||
        resolution.states[childId] === 1 ||
        resolution.states[childId] === 3
      ) {
        resolution.states[planId] = 3;
        continue;
      }
      // retain one completed direct child
      if (resolution.states[childId] === 2) {
        continue;
      }
      stack.push({ entered: false, planId: childId });
      continue;
    }
    const argument = shellResolvePathWord(
      frame.descriptor.argument,
      source,
      environment,
      resolution,
      token,
    );
    let value = null;
    // retain one exact current-source dirname
    if (frame.descriptor.kind === "dirname" && argument === source.path) {
      const slash = source.path.lastIndexOf("/");
      value = slash === -1 ? "." : source.path.slice(0, slash);
    }
    // retain one exact normalized cd-and-pwd result
    if (frame.descriptor.kind === "cd-pwd" && argument !== null) {
      value = shellNormalizePath(argument);
    }
    resolution.values[planId] = value;
    resolution.states[planId] = value === null ? 3 : 2;
  }
  return resolution.states[part.planId] === 2
    ? resolution.values[part.planId]
    : null;
}

// resolve one parameter-free word
function shellResolveWord(
  word,
  source,
  environment,
  limit = SHELL_LITERAL_BYTE_CAP,
  resolution = null,
) {
  let result = "";
  // resolve every word part
  for (const part of word.parts) {
    // retain one literal part
    if (part.kind === "text") {
      // reject unquoted globbing
      if (part.quote === "unquoted" && /[*?[]/u.test(part.value)) {
        return null;
      }
      result = shellAppendScalar(result, part.value, limit);
      continue;
    }
    // resolve one exact scalar parameter
    if (part.kind === "parameter") {
      const replacement = shellEnvironmentScalar(environment, part.name, source);
      // reject one unresolved parameter
      if (replacement === null) {
        return null;
      }
      // reject unquoted splitting and globbing
      if (part.quote === "unquoted" && /[\s*?[]/u.test(replacement)) {
        return null;
      }
      result = shellAppendScalar(result, replacement, limit);
      continue;
    }
    // resolve one reviewed path substitution
    if (part.kind === "command") {
      const replacement = shellResolvePathSubstitution(
        part,
        source,
        environment,
        resolution,
      );
      // reject one other command result
      if (replacement === null) {
        return null;
      }
      // reject unquoted splitting and globbing
      if (part.quote === "unquoted" && /[\s*?[]/u.test(replacement)) {
        return null;
      }
      result = shellAppendScalar(result, replacement, limit);
      continue;
    }
    return null;
  }
  return result;
}

// resolve one scalar or exact array expansion
function shellResolveWordSequence(
  word,
  source,
  environment,
  limit = SHELL_LITERAL_BYTE_CAP,
  resolution = null,
) {
  // retain one exact quoted or unquoted array-at reference
  if (word.parts.length === 1 && word.parts[0].kind === "parameter") {
    const match = /^([A-Za-z_][A-Za-z0-9_]*)\[@\]$/u.exec(word.parts[0].name);
    // retain one exact immutable array value
    if (match !== null) {
      const value = shellEnvironmentGet(environment, match[1]);
      return shellIsArrayValue(value) ? value : null;
    }
  }
  return shellResolveWord(word, source, environment, limit, resolution);
}

// identify one private immutable array node
function shellIsArrayValue(value) {
  return value !== null && typeof value === "object" && value.kind === "array";
}

// create one private immutable array node
function shellArrayValue(pieces) {
  return Object.freeze({
    kind: "array",
    pieces: Object.freeze(pieces.slice()),
  });
}

// materialize only a uniquely reachable syntax-linear sequence
function shellAppendResolvedSequence(output, resolved) {
  // append one scalar sequence member
  if (typeof resolved === "string") {
    output.push(resolved);
    return true;
  }
  // reject one dynamic sequence member
  if (!shellIsArrayValue(resolved)) {
    return false;
  }
  const seen = new Set();
  const stack = [resolved];
  // traverse each immutable array node at most once
  while (stack.length > 0) {
    const current = stack.pop();
    // append one scalar leaf
    if (typeof current === "string") {
      output.push(current);
      continue;
    }
    // reject one malformed or multiply reached array node
    if (!shellIsArrayValue(current) || seen.has(current)) {
      return false;
    }
    seen.add(current);
    // retain source element order without recursive descent
    for (let index = current.pieces.length - 1; index >= 0; index -= 1) {
      stack.push(current.pieces[index]);
    }
  }
  return true;
}

// resolve one ordinary word without retaining oversized direct text
function shellResolveOptionalWord(
  word,
  source,
  environment,
  limit = SHELL_LITERAL_BYTE_CAP,
  resolution = null,
) {
  // leave oversized direct text opaque before the hard scalar resolver
  if (shellDirectTextExceeds(word, limit)) {
    return null;
  }
  return shellResolveWord(word, source, environment, limit, resolution);
}

// resolve one ordinary sequence without retaining oversized direct text
function shellResolveOptionalWordSequence(
  word,
  source,
  environment,
  limit = SHELL_LITERAL_BYTE_CAP,
  resolution = null,
) {
  // leave oversized direct text out of command normalization
  if (shellDirectTextExceeds(word, limit)) {
    return null;
  }
  return shellResolveWordSequence(word, source, environment, limit, resolution);
}

// retain private binding markers
const SHELL_ENVIRONMENT_DELETED = Symbol("shell-environment-deleted");
const SHELL_VARIABLE_UNSET = Object.freeze({ kind: "shell-variable-unset" });
const SHELL_EFFECT_VAR_WRITE = 1;
const SHELL_EFFECT_FN_WRITE = 2;
const SHELL_EFFECT_MAY_RO_VAR = 4;
const SHELL_EFFECT_MAY_RO_FN = 8;
const SHELL_EFFECT_MAY_ERASE_LOCAL = 16;
const SHELL_EFFECT_OPEN = 32;
const SHELL_EFFECT_ALL_BINDINGS = SHELL_EFFECT_VAR_WRITE |
  SHELL_EFFECT_FN_WRITE |
  SHELL_EFFECT_MAY_RO_VAR |
  SHELL_EFFECT_MAY_RO_FN |
  SHELL_EFFECT_MAY_ERASE_LOCAL;
const SHELL_AUTHORITY_NONE = "none";
const SHELL_AUTHORITY_PENDING = "pending-exact";
const SHELL_AUTHORITY_SOURCE = "intrinsic-source";
const SHELL_AUTHORITY_LOCAL = "local-possible";
const SHELL_FUNCTION_ORIGIN_NONE = 0;
const SHELL_FUNCTION_ORIGIN_LOCAL = 1;
const SHELL_FUNCTION_ORIGIN_ABSENT = 2;
const SHELL_FUNCTION_ORIGIN_POSSIBLE = 3;
const SHELL_FUNCTION_LOCAL_PREFIX = "local:";
const SHELL_FUNCTION_UNKNOWN = "unknown";
const SHELL_RADIX_DEPTH = 8;
const SHELL_RADIX_WIDTH = 16;

// encode one closed local function binding
function shellFunctionLocalValue(
  effectMask = 0,
  open = false,
  functionCause = SHELL_AUTHORITY_NONE,
) {
  return `${SHELL_FUNCTION_LOCAL_PREFIX}${effectMask}:${open ? 1 : 0}:${functionCause}`;
}

// decode one local function binding
function shellFunctionLocalEffect(value) {
  // reject one nonlocal binding value
  if (typeof value !== "string" || !value.startsWith(SHELL_FUNCTION_LOCAL_PREFIX)) {
    return null;
  }
  const [maskText, openText, functionCause = SHELL_AUTHORITY_NONE] = value
    .slice(SHELL_FUNCTION_LOCAL_PREFIX.length)
    .split(":");
  const effectMask = Number(maskText);
  // reject one malformed private binding
  if (
    !Number.isSafeInteger(effectMask) ||
    !["0", "1"].includes(openText) ||
    ![
      SHELL_AUTHORITY_NONE,
      SHELL_AUTHORITY_PENDING,
      SHELL_AUTHORITY_SOURCE,
      SHELL_AUTHORITY_LOCAL,
    ].includes(functionCause)
  ) {
    return null;
  }
  return { effectMask, functionCause, open: openText === "1" };
}

// compare one compact shell value
function shellValuesEqual(left, right) {
  // retain one identical private or scalar value
  if (left === right) {
    return true;
  }
  // compare scalar values
  if (typeof left === "string" || typeof right === "string") {
    return typeof left === "string" && left === right;
  }
  // immutable arrays are equal only when they share one proven value
  return shellIsArrayValue(left) && left === right;
}

// create one canonical radix registry
function shellCreateRadixStore() {
  return {
    interned: new Map(),
    nodes: [null],
  };
}

// create one invocation-local binding registry
function shellCreateBindingRegistry() {
  return {
    cells: [null],
    cellIds: new Map(),
    eventRadix: shellCreateRadixStore(),
    events: [null],
    localNameRadix: shellCreateRadixStore(),
    names: new Map(),
    namesById: [null],
    nextBarrierId: 1,
    nextEpoch: 1,
    originRadix: shellCreateRadixStore(),
    recoveryRadix: shellCreateRadixStore(),
    readonlyRadix: shellCreateRadixStore(),
    records: [null],
    recordIds: new Map(),
    semanticRadix: shellCreateRadixStore(),
    topRadix: shellCreateRadixStore(),
    valueIds: new Map(),
    values: [null],
    weakValueIds: new WeakMap(),
  };
}

// intern one fixed radix node
function shellInternRadixNode(store, depth, children, value = 0) {
  const key = depth === SHELL_RADIX_DEPTH
    ? `l:${value}`
    : `${depth}:${children.join(",")}`;
  const existing = store.interned.get(key);
  // reuse one canonical node
  if (existing !== undefined) {
    return existing;
  }
  const id = store.nodes.length;
  store.nodes.push(depth === SHELL_RADIX_DEPTH
    ? { depth, value }
    : { children: Object.freeze(children.slice()), depth });
  store.interned.set(key, id);
  return id;
}

// read one fixed-depth radix leaf
function shellRadixGet(store, root, keyId) {
  let nodeId = root;
  // follow exactly eight identifier nibbles
  for (let depth = 0; depth < SHELL_RADIX_DEPTH; depth += 1) {
    // stop at one absent subtree
    if (nodeId === 0) {
      return 0;
    }
    const shift = (SHELL_RADIX_DEPTH - depth - 1) * 4;
    nodeId = store.nodes[nodeId].children[(keyId >>> shift) & 0x0f];
  }
  return nodeId === 0 ? 0 : store.nodes[nodeId].value;
}

// update one fixed-depth radix leaf
function shellRadixSet(store, root, keyId, value) {
  const path = [];
  let nodeId = root;
  // capture one existing path
  for (let depth = 0; depth < SHELL_RADIX_DEPTH; depth += 1) {
    const shift = (SHELL_RADIX_DEPTH - depth - 1) * 4;
    const nibble = (keyId >>> shift) & 0x0f;
    path.push({ depth, nibble, nodeId });
    nodeId = nodeId === 0 ? 0 : store.nodes[nodeId].children[nibble];
  }
  let child = value === 0
    ? 0
    : shellInternRadixNode(store, SHELL_RADIX_DEPTH, [], value);
  // rebuild exactly eight canonical ancestors
  for (let index = path.length - 1; index >= 0; index -= 1) {
    const entry = path[index];
    const children = entry.nodeId === 0
      ? Array(SHELL_RADIX_WIDTH).fill(0)
      : store.nodes[entry.nodeId].children.slice();
    children[entry.nibble] = child;
    const nonempty = children.some(
      // retain one populated child
      (candidate) => candidate !== 0,
    );
    child = nonempty
      ? shellInternRadixNode(store, entry.depth, children)
      : 0;
  }
  return child;
}

// intern one collision-free identifier id
function shellNameId(registry, name) {
  const existing = registry.names.get(name);
  // reuse one prior identifier
  if (existing !== undefined) {
    return existing;
  }
  const id = registry.names.size + 1;
  registry.names.set(name, id);
  registry.namesById[id] = name;
  return id;
}

// read one persistent function-local name
function shellLocalNameHas(state, name) {
  const registry = state.environment.registry;
  const nameId = registry.names.get(name);
  return nameId !== undefined && shellRadixGet(
    registry.localNameRadix,
    state.localNameRoot,
    nameId,
  ) === 1;
}

// add one persistent function-local name
function shellLocalNameAdd(state, name) {
  const registry = state.environment.registry;
  state.localNameRoot = shellRadixSet(
    registry.localNameRadix,
    state.localNameRoot,
    shellNameId(registry, name),
    1,
  );
}

// intern one compact binding value
function shellBindingValueId(registry, value) {
  // retain primitive identity by value
  if (typeof value === "string") {
    const key = `${typeof value}:${value}`;
    const existing = registry.valueIds.get(key);
    // reuse one primitive value
    if (existing !== undefined) {
      return existing;
    }
    const id = registry.values.length;
    registry.values.push(value);
    registry.valueIds.set(key, id);
    return id;
  }
  // retain object identity for immutable arrays
  if (value !== null && typeof value === "object") {
    const existing = registry.weakValueIds.get(value);
    // reuse one immutable object
    if (existing !== undefined) {
      return existing;
    }
    const id = registry.values.length;
    registry.values.push(value);
    registry.weakValueIds.set(value, id);
    return id;
  }
  throw bootstrapError();
}

// intern one normalized product cell
function shellInternCell(registry, namespace, kind, value = null, mutable = true) {
  const valueId = kind === "exact" ? shellBindingValueId(registry, value) : 0;
  const key = `${namespace}:${kind}:${valueId}:${mutable ? 1 : 0}`;
  const existing = registry.cellIds.get(key);
  // reuse one normalized cell
  if (existing !== undefined) {
    return existing;
  }
  const id = registry.cells.length;
  registry.cells.push({ kind, mutable, namespace, value, valueId });
  registry.cellIds.set(key, id);
  return id;
}

// intern one epoch-stamped semantic record
function shellInternRecord(registry, cellId, epoch) {
  const key = `${cellId}:${epoch}`;
  const existing = registry.recordIds.get(key);
  // reuse one exact stamped record
  if (existing !== undefined) {
    return existing;
  }
  const id = registry.records.length;
  registry.records.push({ cellId, epoch });
  registry.recordIds.set(key, id);
  return id;
}

// create one compact shell environment
function shellCreateEnvironment(registry = shellCreateBindingRegistry(), namespace = "variable") {
  return {
    authorityCause: SHELL_AUTHORITY_NONE,
    barrierTop: false,
    barrierId: 0,
    cwdBase: null,
    epoch: 0,
    eventRoot: 0,
    events: null,
    historicalLocalRoot: 0,
    latentMask: 0,
    namespace,
    namespaceMayReadonly: false,
    overrideRoot: 0,
    possibleReadonlyRoot: 0,
    recovery: null,
    registry,
    root: 0,
    topRoot: 0,
  };
}

// clone one compact shell environment
function shellCloneEnvironment(environment, emptyEvents = false) {
  return {
    ...environment,
    events: emptyEvents ? null : environment.events,
  };
}

// read one independent readonly-risk state
function shellEnvironmentReadonlyRisk(environment, name) {
  const registry = environment.registry;
  const nameId = shellNameId(registry, name);
  const cell = registry.cells[shellEnvironmentCellId(environment, name)];
  // retain one definitely readonly binding
  if ((cell.kind === "exact" || cell.kind === "absent") && !cell.mutable) {
    return "known";
  }
  // retain one namespace-wide or positioned risk
  if (
    environment.namespaceMayReadonly ||
    shellRadixGet(registry.readonlyRadix, environment.possibleReadonlyRoot, nameId) !== 0
  ) {
    return "possible";
  }
  return "none";
}

// mark one source-proven readonly name monotonically
function shellEnvironmentMarkReadonly(environment, name) {
  const registry = environment.registry;
  const nameId = shellNameId(registry, name);
  environment.possibleReadonlyRoot = shellRadixSet(
    registry.readonlyRadix,
    environment.possibleReadonlyRoot,
    nameId,
    1,
  );
}

// mark one unresolved readonly-capable namespace effect
function shellEnvironmentMarkReadonlyUnknown(environment) {
  environment.namespaceMayReadonly = true;
}

// join one monotone readonly-risk proof without replay
function shellJoinReadonlyRisk(entry, branches, output) {
  const changedRoots = new Set();
  let namespaceMayReadonly = entry.namespaceMayReadonly;
  // inspect every continuing branch once
  for (const branch of branches) {
    // skip one unreachable branch
    if (branch === null) {
      continue;
    }
    namespaceMayReadonly ||= branch.namespaceMayReadonly;
    // retain only one root change from the common entry
    if (branch.possibleReadonlyRoot !== entry.possibleReadonlyRoot) {
      changedRoots.add(branch.possibleReadonlyRoot);
    }
  }
  output.namespaceMayReadonly = namespaceMayReadonly;
  // retain the common entry risk without one branch change
  if (changedRoots.size === 0) {
    output.possibleReadonlyRoot = entry.possibleReadonlyRoot;
    return;
  }
  // adopt one monotone changed point set directly
  if (changedRoots.size === 1) {
    output.possibleReadonlyRoot = changedRoots.values().next().value;
    return;
  }
  // widen only the risk namespace for incompatible point sets
  output.possibleReadonlyRoot = entry.possibleReadonlyRoot;
  output.namespaceMayReadonly = true;
}

// concatenate one exact event rope
function shellConcatEvents(left, right) {
  // retain one empty left rope
  if (left === null) {
    return right;
  }
  // retain one empty right rope
  if (right === null) {
    return left;
  }
  return { kind: "concat", left, right };
}

// append one exact event leaf
function shellAppendEvent(rope, eventId) {
  return shellConcatEvents(rope, { eventId, kind: "event" });
}

// read one raw semantic record id
function shellEnvironmentRecord(environment, nameId) {
  const registry = environment.registry;
  // prefer one exact override after a namespace barrier
  if (environment.barrierTop) {
    const override = shellRadixGet(registry.semanticRadix, environment.overrideRoot, nameId);
    // retain one current-epoch override
    if (override !== 0) {
      return override;
    }
  }
  return shellRadixGet(registry.semanticRadix, environment.root, nameId);
}

// read one effective product cell id
function shellEnvironmentCellId(environment, name) {
  const registry = environment.registry;
  const nameId = shellNameId(registry, name);
  // retain one explicit per-key top
  if (shellRadixGet(registry.topRadix, environment.topRoot, nameId) !== 0) {
    return shellInternCell(registry, environment.namespace, "top");
  }
  const recordId = shellEnvironmentRecord(environment, nameId);
  // apply one namespace top to an unstamped key
  if (environment.barrierTop && shellRadixGet(
    registry.semanticRadix,
    environment.overrideRoot,
    nameId,
  ) === 0) {
    return shellInternCell(registry, environment.namespace, "top");
  }
  // retain one definite absence
  if (recordId === 0) {
    return shellInternCell(registry, environment.namespace, "absent");
  }
  return registry.records[recordId].cellId;
}

// read one binding or private marker
function shellEnvironmentValue(environment, name) {
  const cell = environment.registry.cells[shellEnvironmentCellId(environment, name)];
  // retain one exact binding
  if (cell.kind === "exact") {
    return cell.value;
  }
  // retain one possible local function
  if (cell.kind === "top" && environment.namespace === "function") {
    return SHELL_FUNCTION_UNKNOWN;
  }
  return SHELL_ENVIRONMENT_DELETED;
}

// read one exact environment binding
function shellEnvironmentGet(environment, name) {
  const value = shellEnvironmentValue(environment, name);
  return value === SHELL_ENVIRONMENT_DELETED ? undefined : value;
}

// test one exact environment binding
function shellEnvironmentHas(environment, name) {
  const cell = environment.registry.cells[shellEnvironmentCellId(environment, name)];
  return cell.kind === "exact";
}

// compare two present or deleted binding values
function shellEnvironmentValuesEqual(left, right) {
  // compare one deleted binding
  if (left === SHELL_ENVIRONMENT_DELETED || right === SHELL_ENVIRONMENT_DELETED) {
    return left === right;
  }
  return shellValuesEqual(left, right);
}

// rank one private function-authority cause
function shellAuthorityCauseRank(cause) {
  // retain one strongest local-shadow cause
  if (cause === SHELL_AUTHORITY_LOCAL) {
    return 3;
  }
  // retain one intrinsic source gap
  if (cause === SHELL_AUTHORITY_SOURCE) {
    return 2;
  }
  // retain one pending exact dependency
  if (cause === SHELL_AUTHORITY_PENDING) {
    return 1;
  }
  return 0;
}

// merge one constant authority header
function shellMergeAuthorityCause(left, right) {
  return shellAuthorityCauseRank(left) >= shellAuthorityCauseRank(right)
    ? left
    : right;
}

// write one current function-origin marker
function shellWriteFunctionOrigin(environment, nameId, origin) {
  // ignore variable namespace writes
  if (environment.namespace !== "function") {
    return;
  }
  const stampedOrigin = origin === SHELL_FUNCTION_ORIGIN_NONE
    ? 0
    : environment.barrierId * 4 + origin;
  environment.historicalLocalRoot = shellRadixSet(
    environment.registry.originRadix,
    environment.historicalLocalRoot,
    nameId,
    stampedOrigin,
  );
}

// classify one exact function cell origin
function shellFunctionCellOrigin(cell) {
  // retain one definite local function
  if (cell.kind === "exact" && shellFunctionLocalEffect(cell.value) !== null) {
    return SHELL_FUNCTION_ORIGIN_LOCAL;
  }
  // retain one possible local function
  if (cell.kind === "top") {
    return SHELL_FUNCTION_ORIGIN_POSSIBLE;
  }
  return SHELL_FUNCTION_ORIGIN_ABSENT;
}

// create one complete semantic entry token
function shellEnvironmentToken(environment) {
  const exactRoot = environment.barrierTop ? environment.overrideRoot : environment.root;
  return `${environment.namespace}:${exactRoot}:${environment.topRoot}:${environment.barrierTop ? 1 : 0}:${environment.authorityCause}:${environment.latentMask}:${environment.barrierId}:${environment.historicalLocalRoot}`;
}

// update one recovery override path
function shellUpdateRecovery(environment, nameId, cellId, baseToken) {
  const registry = environment.registry;
  const compatible = environment.recovery?.baseToken === baseToken;
  const root = compatible ? environment.recovery.root : 0;
  environment.recovery = {
    baseToken,
    root: shellRadixSet(registry.recoveryRadix, root, nameId, cellId),
  };
}

// clear one recovery override path
function shellClearRecovery(environment, nameId) {
  // stop without one recovery block
  if (environment.recovery === null) {
    return;
  }
  const registry = environment.registry;
  const root = shellRadixSet(registry.recoveryRadix, environment.recovery.root, nameId, 0);
  environment.recovery = root === 0
    ? null
    : { ...environment.recovery, root };
}

// write one full exact binding cell
function shellEnvironmentWriteExact(environment, name, cellId, recoversTop = false) {
  const registry = environment.registry;
  const nameId = shellNameId(registry, name);
  const previousCell = registry.cells[shellEnvironmentCellId(environment, name)];
  const previousEventId = shellRadixGet(registry.eventRadix, environment.eventRoot, nameId);
  const previousEvent = previousEventId === 0 ? null : registry.events[previousEventId];
  const stickyRecovery = recoversTop || previousCell.kind === "top" || previousEvent?.recoversTop === true;
  const recordId = shellInternRecord(registry, cellId, environment.epoch);
  shellWriteFunctionOrigin(
    environment,
    nameId,
    shellFunctionCellOrigin(registry.cells[cellId]),
  );
  environment.root = shellRadixSet(registry.semanticRadix, environment.root, nameId, recordId);
  // retain one exact barrier override
  if (environment.barrierTop) {
    environment.overrideRoot = shellRadixSet(
      registry.semanticRadix,
      environment.overrideRoot,
      nameId,
      recordId,
    );
  }
  environment.topRoot = shellRadixSet(registry.topRadix, environment.topRoot, nameId, 0);
  const eventId = registry.events.length;
  registry.events.push({ cellId, nameId, recoversTop: stickyRecovery });
  environment.eventRoot = shellRadixSet(
    registry.eventRadix,
    environment.eventRoot,
    nameId,
    eventId,
  );
  environment.events = shellAppendEvent(environment.events, eventId);
  // preserve one compatible recovery block
  if (stickyRecovery && environment.recovery !== null) {
    shellUpdateRecovery(environment, nameId, cellId, environment.recovery.baseToken);
  } else if (!stickyRecovery) {
    shellClearRecovery(environment, nameId);
  }
}

// write one exact mutable or readonly binding
function shellEnvironmentSet(environment, name, value, mutable = true) {
  const cellId = shellInternCell(
    environment.registry,
    environment.namespace,
    "exact",
    value,
    mutable,
  );
  shellEnvironmentWriteExact(environment, name, cellId);
}

// write one exact absent binding
function shellEnvironmentUnset(environment, name, mutable = true) {
  const cellId = shellInternCell(
    environment.registry,
    environment.namespace,
    "absent",
    null,
    mutable,
  );
  shellEnvironmentWriteExact(environment, name, cellId);
}

// write one per-key full product top
function shellEnvironmentTop(environment, name) {
  const registry = environment.registry;
  const nameId = shellNameId(registry, name);
  const cellId = shellInternCell(registry, environment.namespace, "top");
  shellWriteFunctionOrigin(environment, nameId, SHELL_FUNCTION_ORIGIN_POSSIBLE);
  const recordId = shellInternRecord(registry, cellId, environment.epoch);
  environment.root = shellRadixSet(registry.semanticRadix, environment.root, nameId, recordId);
  environment.overrideRoot = shellRadixSet(
    registry.semanticRadix,
    environment.overrideRoot,
    nameId,
    0,
  );
  environment.topRoot = shellRadixSet(registry.topRadix, environment.topRoot, nameId, 1);
  environment.eventRoot = shellRadixSet(registry.eventRadix, environment.eventRoot, nameId, 0);
  shellClearRecovery(environment, nameId);
}

// promote one affected namespace to full top
function shellEnvironmentPromoteTop(
  environment,
  cause = SHELL_AUTHORITY_LOCAL,
  latentMask = 0,
) {
  // merge one function-authority header
  if (environment.namespace === "function") {
    environment.authorityCause = shellMergeAuthorityCause(
      environment.authorityCause,
      cause,
    );
    environment.latentMask |= latentMask;
    environment.barrierId = environment.registry.nextBarrierId;
    environment.registry.nextBarrierId += 1;
  }
  environment.barrierTop = true;
  environment.epoch = environment.registry.nextEpoch;
  environment.registry.nextEpoch += 1;
  environment.eventRoot = 0;
  environment.overrideRoot = 0;
  environment.recovery = null;
  environment.topRoot = 0;
}

// read one effective cell by interned name
function shellEnvironmentCellIdByNameId(environment, nameId) {
  const registry = environment.registry;
  // retain one explicit per-key top
  if (shellRadixGet(registry.topRadix, environment.topRoot, nameId) !== 0) {
    return shellInternCell(registry, environment.namespace, "top");
  }
  // apply one namespace barrier outside exact overrides
  if (
    environment.barrierTop &&
    shellRadixGet(registry.semanticRadix, environment.overrideRoot, nameId) === 0
  ) {
    return shellInternCell(registry, environment.namespace, "top");
  }
  const recordId = shellEnvironmentRecord(environment, nameId);
  // retain one exact absence
  if (recordId === 0) {
    return shellInternCell(registry, environment.namespace, "absent");
  }
  return registry.records[recordId].cellId;
}

// classify one canonical semantic top state
function shellEnvironmentTopState(environment) {
  return `${environment.topRoot}:${environment.barrierTop ? 1 : 0}:${environment.authorityCause}:${environment.latentMask}:${environment.historicalLocalRoot}`;
}

// classify one flat recovery block
function shellEnvironmentRecoveryState(environment) {
  return environment.recovery === null
    ? "none"
    : `${environment.recovery.baseToken}:${environment.recovery.root}`;
}

// visit one exact event rope iteratively
function shellVisitEvents(rope, callback) {
  const stack = rope === null ? [] : [rope];
  // consume every rope node once
  while (stack.length > 0) {
    const current = stack.pop();
    // retain one event leaf
    if (current.kind === "event") {
      callback(current.eventId);
      continue;
    }
    stack.push(current.right, current.left);
  }
}

// normalize one branch-owned exact event rope
function shellNormalizeEnvironmentEvents(environment, entry) {
  const registry = environment.registry;
  const normalized = new Map();
  shellVisitEvents(environment.events,
    // retain only one live final event per key
    (eventId) => {
      const event = registry.events[eventId];
      const live = shellRadixGet(registry.eventRadix, environment.eventRoot, event.nameId);
      // skip one overwritten event
      if (live !== eventId) {
        return;
      }
      const currentCellId = shellEnvironmentCellIdByNameId(environment, event.nameId);
      const currentCell = registry.cells[currentCellId];
      const entryCellId = shellEnvironmentCellIdByNameId(entry, event.nameId);
      // retain only full exact values or absences
      if (currentCell.kind === "top") {
        return;
      }
      // drop one entry-equal nonrecovery event
      if (currentCellId === entryCellId && !event.recoversTop) {
        return;
      }
      normalized.set(event.nameId, {
        cellId: currentCellId,
        recoversTop: event.recoversTop,
      });
    });
  return normalized;
}

// read one canonical missing special-group value
function shellSpecialDefaultCell(environment, entry, nameId) {
  const registry = environment.registry;
  // apply one namespace-wide top
  if (environment.barrierTop) {
    return shellInternCell(registry, environment.namespace, "top");
  }
  // apply one per-key top
  if (shellRadixGet(registry.topRadix, environment.topRoot, nameId) !== 0) {
    return shellInternCell(registry, environment.namespace, "top");
  }
  return shellEnvironmentCellIdByNameId(entry, nameId);
}

// write one aggregate exact cell without losing recovery
function shellWriteAggregateCell(environment, entry, nameId, cellId, recoversTop) {
  const registry = environment.registry;
  const name = registry.namesById[nameId];
  // reject one missing identifier spelling
  if (name === undefined) {
    throw bootstrapError();
  }
  shellEnvironmentWriteExact(environment, name, cellId, recoversTop);
  // retain one entry-equal recovery override
  if (recoversTop && cellId === shellEnvironmentCellIdByNameId(entry, nameId)) {
    shellUpdateRecovery(environment, nameId, cellId, shellEnvironmentToken(entry));
  }
}

// fold one local function summary into a join accumulator
function shellAccumulateFunctionCell(accumulator, registry, cellId) {
  const cell = registry.cells[cellId];
  const effect = cell.kind === "exact"
    ? shellFunctionLocalEffect(cell.value)
    : null;
  // reject one nonlocal continuation
  if (effect === null) {
    accumulator.allLocal = false;
    return;
  }
  accumulator.localEffectMask |= effect.effectMask;
  accumulator.localFunctionCause = shellMergeAuthorityCause(
    accumulator.localFunctionCause,
    effect.functionCause,
  );
  accumulator.localOpen ||= effect.open;
  accumulator.localMutable &&= cell.mutable;
}

// join one namespace across control alternatives
function shellJoinEnvironments(state, entry, branches, implicitEntry, prefix) {
  const continuing = branches.filter(
    // retain reachable alternatives only
    (branch) => branch !== null,
  );
  const total = continuing.length + (implicitEntry ? 1 : 0);
  // retain one unreachable join
  if (total === 0) {
    const unreachable = shellCloneEnvironment(entry, true);
    unreachable.events = prefix;
    return unreachable;
  }
  // adopt one sole continuation exactly
  if (total === 1 && !implicitEntry) {
    const adopted = shellCloneEnvironment(continuing[0]);
    adopted.events = shellConcatEvents(prefix, adopted.events);
    return adopted;
  }
  let joinedCause = implicitEntry ? entry.authorityCause : SHELL_AUTHORITY_NONE;
  let joinedLatentMask = implicitEntry ? entry.latentMask : 0;
  // merge every continuing private authority header once
  for (const branch of continuing) {
    joinedCause = shellMergeAuthorityCause(joinedCause, branch.authorityCause);
    joinedLatentMask |= branch.latentMask;
  }
  const entryTopState = shellEnvironmentTopState(entry);
  const changedTopStates = new Map();
  // classify every changed semantic top state
  for (const branch of continuing) {
    const topState = shellEnvironmentTopState(branch);
    // retain only state changes from the common entry
    if (topState !== entryTopState) {
      const group = changedTopStates.get(topState) ?? { branches: [], environment: branch };
      group.branches.push(branch);
      changedTopStates.set(topState, group);
    }
  }
  const entryRecovery = shellEnvironmentRecoveryState(entry);
  const changedRecoveryStates = new Map();
  // classify every changed recovery block
  for (const branch of continuing) {
    const recoveryState = shellEnvironmentRecoveryState(branch);
    // retain one changed block identity
    if (recoveryState !== entryRecovery) {
      changedRecoveryStates.set(
        recoveryState,
        (changedRecoveryStates.get(recoveryState) ?? 0) + 1,
      );
    }
  }
  const changedRecoveryCount = changedRecoveryStates.size === 1
    ? changedRecoveryStates.values().next().value
    : 0;
  const promoteNamespace = changedTopStates.size > 1 ||
    changedRecoveryStates.size > 1 ||
    (changedRecoveryStates.size === 1 && changedRecoveryCount < total);
  const specialGroup = changedTopStates.size === 1
    ? changedTopStates.values().next().value
    : null;
  const base = specialGroup?.environment ?? entry;
  const output = shellCloneEnvironment(base, true);
  shellJoinReadonlyRisk(entry, continuing, output);
  output.epoch = output.registry.nextEpoch;
  output.registry.nextEpoch += 1;
  output.eventRoot = entry.eventRoot;
  output.events = null;
  // promote only one affected namespace on incompatible states
  if (promoteNamespace) {
    const promotionCause = output.namespace === "function" && joinedCause === SHELL_AUTHORITY_NONE
      ? SHELL_AUTHORITY_LOCAL
      : joinedCause;
    shellEnvironmentPromoteTop(output, promotionCause, joinedLatentMask);
  } else if (output.namespace === "function") {
    // retain one compatible authority header
    output.authorityCause = joinedCause;
    output.latentMask = joinedLatentMask;
  }
  const accumulators = new Map();
  const specialState = specialGroup === null
    ? null
    : shellEnvironmentTopState(specialGroup.environment);
  let specialCount = 0;
  // fold every branch-owned event once
  for (const branch of continuing) {
    const special = specialState !== null && shellEnvironmentTopState(branch) === specialState;
    // count one special-state continuation
    if (special) {
      specialCount += 1;
    }
    const normalized = shellNormalizeEnvironmentEvents(branch, entry);
    // fold every exact event key once
    for (const [nameId, event] of normalized) {
      const accumulator = accumulators.get(nameId) ?? {
        allLocal: output.namespace === "function",
        eventCount: 0,
        firstCellId: 0,
        identical: true,
        localEffectMask: 0,
        localFunctionCause: SHELL_AUTHORITY_NONE,
        localMutable: true,
        localOpen: false,
        ordinaryEventCount: 0,
        recoversTop: false,
        specialEventCount: 0,
      };
      accumulator.eventCount += 1;
      accumulator.recoversTop ||= event.recoversTop;
      // count one event group
      if (special) {
        accumulator.specialEventCount += 1;
      } else {
        accumulator.ordinaryEventCount += 1;
      }
      // seed one exact candidate
      if (accumulator.firstCellId === 0) {
        accumulator.firstCellId = event.cellId;
      } else {
        accumulator.identical &&= accumulator.firstCellId === event.cellId;
      }
      // retain every all-path local summary for effect union
      if (accumulator.allLocal) {
        shellAccumulateFunctionCell(accumulator, output.registry, event.cellId);
      }
      accumulators.set(nameId, accumulator);
    }
  }
  const ordinaryCount = total - specialCount;
  // resolve every sparse changed key once
  for (const [nameId, accumulator] of accumulators) {
    let firstCellId = accumulator.firstCellId;
    let identical = accumulator.identical;
    // fold one canonical missing special-group value
    if (specialGroup !== null && accumulator.specialEventCount < specialCount) {
      const missingCellId = shellSpecialDefaultCell(specialGroup.environment, entry, nameId);
      identical &&= firstCellId === missingCellId;
      firstCellId ||= missingCellId;
      // include one special-state default function summary
      if (accumulator.allLocal) {
        shellAccumulateFunctionCell(accumulator, output.registry, missingCellId);
      }
    }
    // fold one common-entry value for ordinary missing continuations
    if (accumulator.ordinaryEventCount < ordinaryCount) {
      const entryCellId = shellEnvironmentCellIdByNameId(entry, nameId);
      identical &&= firstCellId === entryCellId;
      firstCellId ||= entryCellId;
      // include one ordinary entry function summary
      if (accumulator.allLocal) {
        shellAccumulateFunctionCell(accumulator, output.registry, entryCellId);
      }
    }
    // union differing all-local function effects without losing authority
    if (!identical && accumulator.allLocal) {
      firstCellId = shellInternCell(
        output.registry,
        "function",
        "exact",
        shellFunctionLocalValue(
          accumulator.localEffectMask,
          accumulator.localOpen,
          accumulator.localFunctionCause,
        ),
        accumulator.localMutable,
      );
      identical = true;
    }
    const cell = output.registry.cells[firstCellId];
    const exact = identical && cell.kind !== "top";
    // retain one exact all-continuation recovery
    if (exact) {
      const defaultCellId = promoteNamespace
        ? shellInternCell(output.registry, output.namespace, "top")
        : shellEnvironmentCellIdByNameId(output, nameId);
      const recoversTop = accumulator.recoversTop || defaultCellId !== firstCellId;
      shellWriteAggregateCell(output, entry, nameId, firstCellId, recoversTop);
    } else {
      const name = output.registry.namesById[nameId];
      // retain one full product top
      if (name !== undefined) {
        shellEnvironmentTop(output, name);
      }
    }
  }
  const cwdStates = new Set(continuing.map(
    // retain one continuing directory state
    (branch) => branch.cwdBase,
  ));
  // include one implicit entry directory state
  if (implicitEntry) {
    cwdStates.add(entry.cwdBase);
  }
  output.cwdBase = cwdStates.size === 1
    ? cwdStates.values().next().value
    : null;
  output.events = shellConcatEvents(prefix, output.events);
  return output;
}

// commit one logical path into its entry
function shellCommitEnvironmentPath(entry, path) {
  // retain one unreachable continuation
  if (path === null) {
    return null;
  }
  return shellCloneEnvironment(path.environment);
}

// create one flat logical-list status path
function shellCreateEnvironmentPath(entry, functionEntry) {
  const root = shellCloneEnvironment(entry);
  const functionRoot = shellCloneEnvironment(functionEntry);
  return {
    changed: new Set(),
    environment: root,
    functionChanged: new Set(),
    functionEnvironment: functionRoot,
    functionRoot,
    root,
  };
}

// adopt one uniquely owned status journal into its list root
function shellOwnEnvironmentPath(path) {
  return path;
}

// fork one status with an empty write journal
function shellForkEnvironmentPath(path) {
  return {
    changed: path.changed,
    environment: shellCloneEnvironment(path.environment),
    functionChanged: path.functionChanged,
    functionEnvironment: shellCloneEnvironment(path.functionEnvironment),
    functionRoot: path.functionRoot,
    root: path.root,
  };
}

// join namespace-wide logical state without key replay
function shellJoinLogicalEnvironmentHeaders(target, continuing) {
  const barrierStates = new Set();
  const cwdStates = new Set();
  let authorityCause = SHELL_AUTHORITY_NONE;
  let latentMask = 0;
  // classify every continuing semantic header once
  for (const path of continuing) {
    barrierStates.add(path.environment.barrierTop);
    cwdStates.add(path.environment.cwdBase);
    authorityCause = shellMergeAuthorityCause(
      authorityCause,
      path.environment.authorityCause,
    );
    latentMask |= path.environment.latentMask;
  }
  // widen one incompatible namespace header
  if (barrierStates.size > 1) {
    shellEnvironmentPromoteTop(target.environment, authorityCause, latentMask);
  } else if (target.environment.namespace === "function") {
    // retain one compatible private authority header
    target.environment.authorityCause = authorityCause;
    target.environment.latentMask = latentMask;
  }
  // retain only one identical current directory
  if (cwdStates.size > 1) {
    target.environment.cwdBase = null;
  }
}

// join only newly divergent keys into one bounded alternative
function shellJoinEnvironmentAlternatives(paths, changed, functionChanged = new Set()) {
  const continuing = paths.filter(
    // retain reachable alternatives only
    (path) => path !== null,
  );
  // retain one unreachable join
  if (continuing.length === 0) {
    return null;
  }
  const target = continuing[0];
  // retain one already shared status path
  if (continuing.every(
    // compare path identity only
    (path) => path === target,
  )) {
    return shellOwnEnvironmentPath(target);
  }
  shellJoinReadonlyRisk(
    target.root,
    continuing.map(
      // project one alternative environment
      (path) => path.environment,
    ),
    target.environment,
  );
  shellJoinLogicalEnvironmentHeaders(target, continuing);
  // merge only keys changed since the prior status join
  for (const name of changed) {
    const first = shellEnvironmentValue(target.environment, name);
    const identical = continuing.every(
      // compare one alternative binding
      (path) => shellEnvironmentValuesEqual(
        first,
        shellEnvironmentValue(path.environment, name),
      ),
    );
    // retain one identical binding
    if (identical && first !== SHELL_ENVIRONMENT_DELETED) {
      shellEnvironmentSet(target.environment, name, first);
    } else {
      shellEnvironmentTop(target.environment, name);
    }
    target.changed.add(name);
  }
  const functionPaths = continuing.map(
    // project one function status path
    (path) => ({
      environment: path.functionEnvironment,
      root: path.functionRoot,
    }),
  );
  const functionTarget = functionPaths[0];
  shellJoinReadonlyRisk(
    functionTarget.root,
    functionPaths.map(
      // project one function environment
      (path) => path.environment,
    ),
    functionTarget.environment,
  );
  shellJoinLogicalEnvironmentHeaders(functionTarget, functionPaths);
  // merge only function keys changed since the prior join
  for (const name of functionChanged) {
    const first = shellEnvironmentValue(functionTarget.environment, name);
    const identical = functionPaths.every(
      // compare one alternative function binding
      (path) => shellEnvironmentValuesEqual(
        first,
        shellEnvironmentValue(path.environment, name),
      ),
    );
    // retain one identical function binding
    if (identical && first !== SHELL_ENVIRONMENT_DELETED) {
      shellEnvironmentSet(functionTarget.environment, name, first);
    } else {
      shellEnvironmentTop(functionTarget.environment, name);
    }
    target.functionChanged.add(name);
  }
  target.functionEnvironment = functionTarget.environment;
  return target;
}

// define one local shell function
function shellDefineFunction(state, name) {
  // reject one known or possible readonly function mutation
  if (shellGuardSourceMutation(state, state.functionEnvironment, name)) {
    return;
  }
  const effect = state.functionEffects.get(name) ?? { effectMask: 0, open: true };
  shellEnvironmentSet(
    state.functionEnvironment,
    name,
    shellFunctionLocalValue(
      effect.effectMask,
      effect.open,
      effect.functionCause ?? SHELL_AUTHORITY_NONE,
    ),
  );
  // retain one logical function-key mutation
  shellRecordLogicalFunctionChange(state, name);
}

// resolve one command name against supported function bindings
function shellCommandAuthority(state, name, position) {
  const declarations = state.functionInventory.get(name);
  // preserve module source order during deferred analysis
  if (
    state.ownerSymbol === "<module>" &&
    declarations !== undefined &&
    position < declarations[0] &&
    !state.functionEnvironment.barrierTop
  ) {
    return "external";
  }
  const environment = state.functionEnvironment;
  const registry = environment.registry;
  const nameId = shellNameId(registry, name);
  // prefer one exact post-barrier override
  if (environment.barrierTop) {
    const stampedOrigin = shellRadixGet(
      registry.originRadix,
      environment.historicalLocalRoot,
      nameId,
    );
    const originBarrierId = Math.floor(stampedOrigin / 4);
    const historicalOrigin = stampedOrigin % 4;
    // retain one exact post-barrier definition
    if (
      originBarrierId === environment.barrierId &&
      historicalOrigin === SHELL_FUNCTION_ORIGIN_LOCAL
    ) {
      return "local";
    }
    // retain one exact successful post-barrier unset
    if (
      originBarrierId === environment.barrierId &&
      historicalOrigin === SHELL_FUNCTION_ORIGIN_ABSENT
    ) {
      return "external";
    }
    const imported = shellImportedFunctionEffect(state, name);
    // retain direct local-shadow evidence through one namespace barrier
    if (
      historicalOrigin === SHELL_FUNCTION_ORIGIN_LOCAL ||
      historicalOrigin === SHELL_FUNCTION_ORIGIN_POSSIBLE ||
      (imported !== null && position >= state.functionProfile.activationPosition) ||
      environment.authorityCause === SHELL_AUTHORITY_LOCAL
    ) {
      return "unknown-local";
    }
    // retain one pending exact dependency authority
    if (environment.authorityCause === SHELL_AUTHORITY_PENDING) {
      return "unknown-pending";
    }
    // retain one intrinsic source authority
    if (environment.authorityCause === SHELL_AUTHORITY_SOURCE) {
      return "unknown-source";
    }
    return "unknown-local";
  }
  const value = shellEnvironmentValue(state.functionEnvironment, name);
  // retain one definite local binding
  if (shellFunctionLocalEffect(value) !== null) {
    return "local";
  }
  // retain one conditional local binding
  if (value === SHELL_FUNCTION_UNKNOWN) {
    return "unknown-local";
  }
  const imported = shellImportedFunctionEffect(state, name);
  // retain one dominating imported authority view
  if (
    imported !== null &&
    position >= state.functionProfile.activationPosition &&
    position < state.functionProfile.openPosition
  ) {
    return "local";
  }
  // treat function-body free names as call-time unknown
  if (
    state.ownerSymbol !== "<module>" &&
    declarations !== undefined
  ) {
    return "unknown";
  }
  return "external";
}

// push one bounded sanitized diagnostic
function shellPushDiagnostic(state) {
  // fail instead of truncating diagnostics
  if (state.diagnostics.length >= SHELL_DIAGNOSTIC_CAP) {
    throw bootstrapError("frontend.bound-exceeded");
  }
  state.diagnostics.push(unsupportedDiagnostic());
}

// count one node's initial child roles
function shellChildRoleCounts(children) {
  const counts = new Map();
  // count every existing child once
  for (const child of children) {
    counts.set(child.role, (counts.get(child.role) ?? 0) + 1);
  }
  return counts;
}

// append one indexed shell child edge
function shellAppendChildEdge(parent, node, role, index = null) {
  const count = parent.childRoleCounts.get(role) ?? 0;
  parent.children.push({ index: index ?? count, node, role });
  parent.childRoleCounts.set(role, count + 1);
}

// attach one syntax node to the active parent
function shellAttachNode(state, node, role = "body", index = null) {
  const context = state.contexts.at(-1);
  // stop at module scope
  if (context === undefined) {
    return;
  }
  const parent = context.type === "case" && context.currentArm !== null
    ? context.currentArm.node
    : context.node;
  // stop without one positioned parent
  if (parent === null) {
    return;
  }
  shellAppendChildEdge(parent, node, role, index);
}

// create one positioned shell syntax node
function shellCreateNode(state, {
  attributes = {},
  children = [],
  end,
  factKind,
  start,
  attach = true,
  role = "body",
}) {
  // reject one fabricated or empty span
  if (
    !Number.isSafeInteger(start) ||
    !Number.isSafeInteger(end) ||
    start < 0 ||
    end <= start ||
    end > state.source.sourceText.length
  ) {
    shellPushDiagnostic(state);
    return null;
  }
  const node = {
    attributes: {
      identifier: attributes.identifier ?? null,
      importLevel: attributes.importLevel ?? null,
      literalKind: attributes.literalKind ?? null,
      literalRole: attributes.literalRole ?? "none",
      literalValue: attributes.literalValue ?? null,
      operators: attributes.operators ?? [],
    },
    childRoleCounts: shellChildRoleCounts(children),
    children: [...children],
    end,
    factKind,
    ordinal: state.nodes.length,
    start,
  };
  state.nodes.push(node);
  // attach one ordinary statement node
  if (attach) {
    shellAttachNode(state, node, role);
  }
  return node;
}

// add one ordered child relationship
function shellAddChild(parent, child, role, index = null) {
  // skip one absent child
  if (parent === null || child === null) {
    return;
  }
  shellAppendChildEdge(parent, child, role, index);
}

// create one shell word fact
function shellWordNode(
  state,
  word,
  environment,
  purpose = "value",
  limit = SHELL_LITERAL_BYTE_CAP,
  literalRole = "semantic-constant",
  required = false,
) {
  const exact = required
    ? shellResolveWord(word, state.source, environment, limit, state.pathResolution)
    : shellResolveOptionalWord(
        word,
        state.source,
        environment,
        limit,
        state.pathResolution,
      );
  let node;
  // retain one exact callee name
  if (purpose === "func" && exact !== null) {
    node = shellCreateNode(state, {
      attributes: { identifier: exact },
      attach: false,
      end: word.end,
      factKind: "name",
      start: word.start,
    });
    shellQueueWordSubstitutions(state, word, node, state.ownerSymbol);
    return node;
  }
  // retain one exact private literal
  if (exact !== null) {
    node = shellCreateNode(state, {
      attributes: {
        literalKind: "string",
        literalRole,
        literalValue: exact,
      },
      attach: false,
      end: word.end,
      factKind: "constant",
      start: word.start,
    });
    shellQueueWordSubstitutions(state, word, node, state.ownerSymbol);
    return node;
  }
  const expression = shellCreateNode(state, {
    attributes: { identifier: "shell-word", literalRole: "opaque" },
    attach: false,
    end: word.end,
    factKind: "expression",
    start: word.start,
  });
  let parameterIndex = 0;
  // retain every direct parameter reference
  for (const part of word.parts) {
    // skip nonparameter parts
    if (part.kind !== "parameter") {
      continue;
    }
    const parameter = shellCreateNode(state, {
      attributes: { identifier: part.name },
      attach: false,
      end: part.end,
      factKind: "name",
      start: part.start,
    });
    shellAddChild(expression, parameter, "value", parameterIndex);
    parameterIndex += 1;
  }
  shellQueueWordSubstitutions(state, word, expression, state.ownerSymbol);
  return expression;
}

// create one opaque shell word fact without literal retention
function shellOpaqueWordNode(state, word, environment) {
  const expression = shellCreateNode(state, {
    attributes: { identifier: "shell-word", literalRole: "opaque" },
    attach: false,
    end: word.end,
    factKind: "expression",
    start: word.start,
  });
  let parameterIndex = 0;
  // retain every direct parameter reference
  for (const part of word.parts) {
    // skip nonparameter parts
    if (part.kind !== "parameter") {
      continue;
    }
    const parameter = shellCreateNode(state, {
      attributes: { identifier: part.name },
      attach: false,
      end: part.end,
      factKind: "name",
      start: part.start,
    });
    shellAddChild(expression, parameter, "value", parameterIndex);
    parameterIndex += 1;
  }
  shellQueueWordSubstitutions(state, word, expression, state.ownerSymbol);
  return expression;
}


// record one current logical-segment change
function shellRecordLogicalChange(state, name) {
  // retain one current logical-segment delta
  if (state.logicalChanged !== null) {
    state.logicalChanged.add(name);
  }
}

// record one current logical function change
function shellRecordLogicalFunctionChange(state, name) {
  // retain one current logical function delta
  if (state.logicalFunctionChanged !== null) {
    state.logicalFunctionChanged.add(name);
  }
}

// guard one source-level mutation against readonly risk
function shellGuardSourceMutation(state, environment, name) {
  const risk = shellEnvironmentReadonlyRisk(environment, name);
  // preserve one definitely readonly binding
  if (risk === "known") {
    shellPushDiagnostic(state);
    return true;
  }
  // suppress one possibly readonly confident replacement
  if (risk === "possible") {
    shellEnvironmentTop(environment, name);
    // retain one mutation in its selected namespace
    if (environment.namespace === "function") {
      shellRecordLogicalFunctionChange(state, name);
    } else {
      shellRecordLogicalChange(state, name);
    }
    shellPushDiagnostic(state);
    return true;
  }
  return false;
}

// apply one exact readonly assignment atomically
function shellAssignReadonlyBinding(state, name, value) {
  const risk = shellEnvironmentReadonlyRisk(state.environment, name);
  // reject one repeated assignment to a readonly name
  if (risk === "known") {
    shellPushDiagnostic(state);
    return false;
  }
  shellEnvironmentMarkReadonly(state.environment, name);
  // suppress one assignment under possible readonly risk
  if (risk === "possible") {
    shellEnvironmentTop(state.environment, name);
    shellRecordLogicalChange(state, name);
    shellPushDiagnostic(state);
    return false;
  }
  shellEnvironmentSet(state.environment, name, value, false);
  shellRecordLogicalChange(state, name);
  return true;
}

// preserve one declaration-only readonly binding
function shellDeclareReadonlyBinding(state, name) {
  const environment = state.environment;
  const registry = environment.registry;
  const cell = registry.cells[shellEnvironmentCellId(environment, name)];
  const risk = shellEnvironmentReadonlyRisk(environment, name);
  shellEnvironmentMarkReadonly(environment, name);
  // retain one idempotent readonly declaration
  if ((cell.kind === "exact" || cell.kind === "absent") && !cell.mutable) {
    return true;
  }
  // preserve one exact value or invisible absence
  if ((cell.kind === "exact" || cell.kind === "absent") && risk === "none") {
    const cellId = shellInternCell(
      registry,
      environment.namespace,
      cell.kind,
      cell.value,
      false,
    );
    shellEnvironmentWriteExact(environment, name, cellId);
    shellRecordLogicalChange(state, name);
    return true;
  }
  // retain uncertainty without inventing one value
  shellEnvironmentTop(environment, name);
  shellRecordLogicalChange(state, name);
  shellPushDiagnostic(state);
  return false;
}

// retain one unresolved readonly declaration conservatively
function shellDeclareReadonlyUnknown(state, name) {
  shellEnvironmentMarkReadonly(state.environment, name);
  shellEnvironmentTop(state.environment, name);
  shellRecordLogicalChange(state, name);
  shellPushDiagnostic(state);
}

// update one semantic binding and both proof channels
function shellUpdateBinding(state, name, value, mutable = true) {
  // apply one readonly assignment through its attribute owner
  if (!mutable) {
    shellAssignReadonlyBinding(state, name, value);
    return;
  }
  // reject one readonly source mutation
  if (shellGuardSourceMutation(state, state.environment, name)) {
    shellRecordLogicalChange(state, name);
    return;
  }
  shellEnvironmentSet(state.environment, name, value, mutable);
  shellRecordLogicalChange(state, name);
}

// invalidate one semantic binding and both proof channels
function shellInvalidateBinding(state, name) {
  // reject one readonly source mutation
  if (shellGuardSourceMutation(state, state.environment, name)) {
    shellRecordLogicalChange(state, name);
    return;
  }
  shellEnvironmentTop(state.environment, name);
  shellRecordLogicalChange(state, name);
}

// split one assignment spelling
function shellAssignment(word) {
  const first = word.parts[0];
  // reject one nonliteral prefix
  if (first?.kind !== "text") {
    return null;
  }
  const match = /^([A-Za-z_][A-Za-z0-9_]*)(\+?=)(.*)$/u.exec(first.value);
  // reject one ordinary word
  if (match === null) {
    return null;
  }
  const valueParts = [];
  // retain one suffix in the assignment prefix
  if (match[3].length > 0) {
    valueParts.push({
      ...first,
      start: first.end - match[3].length,
      value: match[3],
    });
  }
  appendValues(valueParts, word.parts, 1);
  return {
    append: match[2] === "+=",
    name: match[1],
    valueWord: {
      end: word.end,
      kind: "word",
      parts: valueParts,
      start: Math.min(
        word.end - 1,
        first.start + match[1].length + match[2].length,
      ),
    },
  };
}

// capture one command plan at first syntax discovery
function shellQueueCommandPlan(state, planId, parent, ownerSymbol) {
  const existingIndex = state.queuedPlanIds[planId];
  // preserve one immutable first capture
  if (existingIndex !== 0) {
    return;
  }
  const substitution = {
    environment: shellCloneEnvironment(state.environment, true),
    functionEnvironment: shellCloneEnvironment(state.functionEnvironment, true),
    localNameRoot: state.localNameRoot,
    ownerSymbol,
    parent,
    planId,
  };
  state.substitutions.push(substitution);
  state.queuedPlanIds[planId] = state.substitutions.length;
}

// queue nested command substitutions without rescanning source
function shellQueueWordSubstitutions(state, word, parent, ownerSymbol) {
  const parts = [...word.parts];
  // consume every direct plan reference once
  while (parts.length > 0) {
    const part = parts.pop();
    // queue one direct command body
    if (part.kind === "command") {
      shellQueueCommandPlan(state, part.planId, parent, ownerSymbol);
      continue;
    }
    // skip one nonarithmetic part
    if (part.kind !== "arithmetic") {
      continue;
    }
    // avoid revisiting one arithmetic plan from duplicate fact emission
    if (state.queuedArithmeticPlanIds[part.planId] !== 0) {
      continue;
    }
    state.queuedArithmeticPlanIds[part.planId] = 1;
    const plan = state.lexicon.plans[part.planId];
    // inspect only direct pretokenized child references
    for (const token of plan?.tokens ?? []) {
      // retain one nested word reference list
      if (token.kind === "word") {
        appendValues(parts, token.parts);
      }
    }
  }
}

// assign one scalar or array binding
function shellBindAssignment(
  state,
  assignment,
  word,
  declarationStart,
  arrayWords = null,
  mutable = true,
  readonlyScope = true,
) {
  let value = null;
  let valueNode = null;
  // resolve one explicit array value
  if (arrayWords !== null) {
    const pieces = [];
    const arrayNode = shellCreateNode(state, {
      attributes: { identifier: "array" },
      attach: false,
      end: arrayWords.end,
      factKind: "expression",
      start: arrayWords.start,
    });
    let elementIndex = 0;
    let complete = true;
    // resolve every syntax-level array element
    for (const element of arrayWords.words) {
      const resolved = shellResolveWordSequence(
        element,
        state.source,
        state.environment,
        SHELL_LITERAL_BYTE_CAP,
        state.pathResolution,
      );
      const elementNode = shellWordNode(state, element, state.environment);
      shellAddChild(arrayNode, elementNode, "elts", elementIndex);
      elementIndex += 1;
      // invalidate one dynamic array without expanding prior arrays
      if (resolved === null) {
        complete = false;
        break;
      }
      pieces.push(resolved);
    }
    // retain one immutable syntax-linear array DAG node
    if (complete) {
      value = shellArrayValue(pieces);
    }
    valueNode = arrayNode;
  } else {
    value = shellResolveWord(
      assignment.valueWord,
      state.source,
      state.environment,
      SHELL_LITERAL_BYTE_CAP,
      state.pathResolution,
    );
    valueNode = shellWordNode(state, assignment.valueWord, state.environment);
  }
  const targetStart = word.start;
  const target = shellCreateNode(state, {
    attributes: { identifier: assignment.name },
    attach: false,
    end: Math.min(word.end, targetStart + assignment.name.length),
    factKind: "name",
    start: targetStart,
  });
  const bindingEnd = arrayWords?.end ?? word.end;
  const binding = shellCreateNode(state, {
    attributes: { identifier: assignment.name },
    attach: true,
    end: bindingEnd,
    factKind: "binding",
    start: declarationStart ?? word.start,
  });
  shellAddChild(binding, target, "target", 0);
  shellAddChild(binding, valueNode, "value", 0);
  // fail closed for one unproved readonly declaration scope
  if (!mutable && !readonlyScope) {
    shellDeclareReadonlyUnknown(state, assignment.name);
    return binding;
  }
  // apply one exact append
  if (assignment.append && value !== null) {
    const previous = shellEnvironmentGet(state.environment, assignment.name);
    // append compatible compact scalars only
    if (typeof value === "string" && typeof previous === "string") {
      shellUpdateBinding(state, assignment.name, shellAppendScalar(previous, value), mutable);
      return binding;
    }
    // append compatible immutable arrays by reference
    if (shellIsArrayValue(value) && shellIsArrayValue(previous)) {
      shellUpdateBinding(state, assignment.name, shellArrayValue([previous, value]), mutable);
      return binding;
    }
    // retain one unresolved readonly append as readonly risk
    if (!mutable) {
      shellDeclareReadonlyUnknown(state, assignment.name);
    } else {
      shellInvalidateBinding(state, assignment.name);
    }
    return binding;
  }
  // retain one resolved assignment
  if (value !== null) {
    shellUpdateBinding(state, assignment.name, value, mutable);
  } else if (!mutable) {
    shellDeclareReadonlyUnknown(state, assignment.name);
  } else {
    shellInvalidateBinding(state, assignment.name);
  }
  return binding;
}

// parse one adjacent typed process substitution
function shellProcessSubstitutionAt(tokens, index, end = tokens.length, rangeIndex = null) {
  const direction = tokens[index];
  const open = tokens[index + 1];
  // require one adjacent direction and opener
  if (
    direction?.kind !== "operator" ||
    !["<", ">"].includes(direction.value) ||
    open?.kind !== "operator" ||
    open.value !== "(" ||
    direction.end !== open.start
  ) {
    return null;
  }
  let depth = 1;
  let closeIndex = index + 2;
  const indexedClose = rangeIndex?.pairClose[index + 1] ?? -1;
  // reuse one indexed process close
  if (indexedClose >= index + 1 && indexedClose < end) {
    closeIndex = indexedClose + 1;
    depth = 0;
  }
  // locate one exact typed close
  for (; closeIndex < end && depth > 0; closeIndex += 1) {
    const token = tokens[closeIndex];
    // open one nested parenthesis
    if (token.kind === "operator" && token.value === "(") {
      depth += 1;
    } else if (token.kind === "operator" && token.value === ")") {
      // close one nested parenthesis
      depth -= 1;
    }
  }
  // reject one incomplete process substitution
  if (depth !== 0) {
    return null;
  }
  const close = tokens[closeIndex - 1];
  return {
    body: {
      end: closeIndex - 1,
      plan: rangeIndex,
      start: index + 2,
      tokens,
    },
    direction: direction.value,
    endIndex: closeIndex - 1,
    word: {
      end: close.end,
      kind: "word",
      parts: [{
        end: close.end,
        kind: "unknown",
        quote: "unquoted",
        start: direction.start,
      }],
      start: direction.start,
    },
  };
}

// validate and remove simple-command redirections
function shellCommandShape(tokens, start = 0, end = tokens.length, rangeIndex = null) {
  const words = [];
  const wordTokenIndexes = [];
  const substitutions = [];
  let arrayDepth = 0;
  // inspect every simple-command token
  for (let index = start; index < end; index += 1) {
    const token = tokens[index];
    const processSubstitution = arrayDepth === 0
      ? shellProcessSubstitutionAt(tokens, index, end, rangeIndex)
      : null;
    // retain one ordinary process-substitution argument position
    if (processSubstitution !== null) {
      substitutions.push(processSubstitution.body);
      words.push(processSubstitution.word);
      wordTokenIndexes.push(-1);
      index = processSubstitution.endIndex;
      continue;
    }
    // consume one exact brace file descriptor
    if (
      arrayDepth === 0 &&
      token.kind === "operator" &&
      token.value === "{" &&
      tokens[index + 1]?.kind === "word" &&
      /^[A-Za-z_][A-Za-z0-9_]*$/u.test(
        shellOptionalLiteralWord(tokens[index + 1]) ?? "",
      ) &&
      tokens[index + 2]?.kind === "operator" &&
      tokens[index + 2].value === "}" &&
      tokens[index + 3]?.kind === "operator" &&
      SHELL_REDIRECT_OPERATORS.has(tokens[index + 3].value)
    ) {
      index += 2;
      continue;
    }
    // open one owned array assignment
    if (token.kind === "operator" && token.value === "(") {
      const prior = tokens[index - 1];
      const owned = arrayDepth > 0 || (
        prior?.kind === "word" && shellAssignment(prior) !== null
      );
      // reject one unowned group opener
      if (!owned) {
        return { ok: false, wordTokenIndexes: [], words: [] };
      }
      arrayDepth += 1;
      continue;
    }
    // close one owned array assignment
    if (token.kind === "operator" && token.value === ")") {
      // reject one unmatched close
      if (arrayDepth === 0) {
        return { ok: false, wordTokenIndexes: [], words: [] };
      }
      arrayDepth -= 1;
      continue;
    }
    // skip one redirect and its operand
    if (
      arrayDepth === 0 &&
      token.kind === "operator" &&
      SHELL_REDIRECT_OPERATORS.has(token.value)
    ) {
      const previous = words.at(-1);
      // remove one adjacent file descriptor word
      if (
        previous !== undefined &&
        previous.end === token.start &&
        /^[0-9]+$/u.test(shellOptionalLiteralWord(previous) ?? "")
      ) {
        words.pop();
        wordTokenIndexes.pop();
      }
      const operand = tokens[index + 1];
      const redirectSubstitution = shellProcessSubstitutionAt(
        tokens,
        index + 1,
        end,
        rangeIndex,
      );
      // consume one matching process-substitution redirect target
      if (redirectSubstitution !== null && redirectSubstitution.direction === token.value) {
        substitutions.push(redirectSubstitution.body);
        index = redirectSubstitution.endIndex;
        continue;
      }
      // reject one unsupported process-substitution redirect spelling
      if (operand?.kind === "operator" && ["<", ">", "("].includes(operand.value)) {
        return { ok: false, substitutions: [], wordTokenIndexes: [], words: [] };
      }
      // require one ordinary redirect operand word
      if (operand?.kind !== "word") {
        return { ok: false, wordTokenIndexes: [], words: [] };
      }
      index += 1;
      continue;
    }
    // reject every other unconsumed operator
    if (token.kind === "operator") {
      return { ok: false, wordTokenIndexes: [], words: [] };
    }
    // retain ordinary words only
    if (token.kind === "word") {
      words.push(token);
      wordTokenIndexes.push(index);
    }
  }
  // reject one unterminated array assignment
  if (arrayDepth !== 0) {
    return { ok: false, wordTokenIndexes: [], words: [] };
  }
  return { ok: true, substitutions, wordTokenIndexes, words };
}

// locate one array assignment extent
function shellArrayAssignment(tokens, wordIndex, assignment, end = tokens.length) {
  const open = tokens[wordIndex + 1];
  // reject one scalar assignment
  if (open?.kind !== "operator" || open.value !== "(") {
    return null;
  }
  const words = [];
  let depth = 1;
  // scan to the matching array close
  for (let index = wordIndex + 2; index < end; index += 1) {
    const token = tokens[index];
    // enter one nested group
    if (token.kind === "operator" && token.value === "(") {
      depth += 1;
      continue;
    }
    // leave one nested group
    if (token.kind === "operator" && token.value === ")") {
      depth -= 1;
      // return one complete array
      if (depth === 0) {
        return { end: token.end, nextIndex: index + 1, start: open.start, words };
      }
      continue;
    }
    // retain top-level array elements
    if (depth === 1 && token.kind === "word") {
      words.push(token);
    }
  }
  return { assignment, malformed: true };
}

// classify one shell atom without payload leakage
function shellAtom(word, durationSlot = false) {
  const numeric = /^[+-]?(?:[0-9]+(?:\.[0-9]+)?|\.[0-9]+)$/u;
  const duration = /^[+-]?(?:[0-9]+(?:\.[0-9]+)?|\.[0-9]+)(?:[smhd])?$/u;
  // prioritize one required duration slot
  if (durationSlot && duration.test(word)) {
    return ["duration"];
  }
  // retain suffixed durations
  if (duration.test(word) && /[smhd]$/u.test(word)) {
    return ["duration"];
  }
  // retain unsuffixed numbers
  if (numeric.test(word)) {
    return ["number"];
  }
  return ["literal"];
}

// parse one timeout layer without descending
function parseTimeoutLayer(values, start) {
  const args = [];
  let cursor = start;
  let optionPhase = true;
  // consume admitted timeout options
  while (cursor < values.length && optionPhase) {
    const word = values[cursor];
    // stop at the required duration
    if (/^[+-]?(?:[0-9]+(?:\.[0-9]+)?|\.[0-9]+)(?:[smhd])?$/u.test(word)) {
      break;
    }
    // preserve one end marker
    if (word === "--") {
      args.push(["end-options"]);
      optionPhase = false;
      cursor += 1;
      break;
    }
    const attached = /^(--signal|--kill-after)=(.*)$/u.exec(word);
    // consume one attached value option
    if (attached !== null) {
      // reject one empty required value
      if (attached[2].length === 0) {
        return null;
      }
      const kind = attached[1] === "--kill-after"
        ? shellAtom(attached[2], true)[0]
        : "literal";
      // require a duration kill-after
      if (attached[1] === "--kill-after" && kind !== "duration") {
        return null;
      }
      args.push(["option", attached[1], kind]);
      cursor += 1;
      continue;
    }
    // consume one separate value option
    if (["-s", "--signal", "-k", "--kill-after"].includes(word)) {
      const value = values[cursor + 1];
      // reject one missing required value
      if (value === undefined || value.length === 0) {
        return null;
      }
      const durationValue = word === "-k" || word === "--kill-after";
      const kind = durationValue ? shellAtom(value, true)[0] : "literal";
      // require one duration option value
      if (durationValue && kind !== "duration") {
        return null;
      }
      args.push(["option", word, kind]);
      cursor += 2;
      continue;
    }
    // consume one admitted flag
    if (["-p", "--preserve-status", "-f", "--foreground", "-v", "--verbose"].includes(word)) {
      args.push(["option", word, null]);
      cursor += 1;
      continue;
    }
    return null;
  }
  // require one timeout duration
  if (values[cursor] === undefined || shellAtom(values[cursor], true)[0] !== "duration") {
    return null;
  }
  args.push(["duration"]);
  cursor += 1;
  // require one child command
  if (values[cursor] === undefined || values[cursor].length === 0) {
    return null;
  }
  return { args, childIndex: cursor };
}

// normalize one iterative timeout chain
function normalizeTimeoutChain(words, context) {
  const layers = [];
  let cursor = 0;
  let currentContext = context;
  // peel every timeout layer without recursion or repeated slices
  while (words[cursor] === "timeout") {
    const layer = parseTimeoutLayer(words, cursor + 1);
    // reject one invalid timeout layer
    if (layer === null) {
      return null;
    }
    layers.push({ args: layer.args, context: currentContext });
    cursor = layer.childIndex;
    currentContext = "simple";
  }
  const terminalWords = words.slice(cursor);
  let child;
  // normalize one admitted semantic child
  if (["date", "sleep", "timeout", "exec"].includes(terminalWords[0])) {
    child = normalizeShell(terminalWords, currentContext);
  } else {
    child = [currentContext, terminalWords[0], terminalWords.slice(1).map(
      // classify one generic child argument
      (word) => word === "--" ? ["end-options"] : shellAtom(word),
    )];
  }
  // reject one unsupported terminal child
  if (child === null) {
    return null;
  }
  // rebuild each timeout layer from inner to outer
  for (let index = layers.length - 1; index >= 0; index -= 1) {
    const layer = layers[index];
    child = [
      layer.context,
      "timeout",
      [...layer.args, ["child", child[1], child[2]]],
    ];
  }
  return child;
}

// normalize one closed shell command
function shellNormalizeTerminal(words, context) {
  // reject a missing command
  if (words === null || words.length === 0 || words[0].length === 0) {
    return null;
  }
  const command = words[0];
  const values = words.slice(1);
  const args = [];
  // normalize one date command
  if (command === "date") {
    let optionPhase = true;
    let formatSeen = false;
    // consume date arguments
    for (const word of values) {
      // preserve one end marker
      if (optionPhase && word === "--") {
        args.push(["end-options"]);
        optionPhase = false;
      } else if (optionPhase && (word === "-u" || word === "--utc")) {
        args.push(["option", word, null]);
      } else if (!formatSeen && word.startsWith("+")) {
        args.push(["format"]);
        formatSeen = true;
      } else {
        return null;
      }
    }
    return [context, command, args];
  }
  // normalize one sleep command
  if (command === "sleep") {
    let cursor = 0;
    // preserve one optional end marker
    if (values[cursor] === "--") {
      args.push(["end-options"]);
      cursor += 1;
    }
    // require at least one duration
    if (cursor >= values.length) {
      return null;
    }
    // consume every exact duration
    for (; cursor < values.length; cursor += 1) {
      const atom = shellAtom(values[cursor], true);
      // reject one nonduration value
      if (atom[0] !== "duration") {
        return null;
      }
      args.push(atom);
    }
    return [context, command, args];
  }
  // normalize one timeout command
  if (command === "timeout") {
    return normalizeTimeoutChain(words, context);
  }
  return null;
}

// normalize one closed command through the authoritative wrapper parse
function normalizeShell(words, context, wrapper = shellParseWrapperChain(words)) {
  // reject one invalid wrapper chain
  if (!wrapper.ok) {
    return null;
  }
  const terminalWords = words.slice(wrapper.terminalIndex);
  const terminal = shellNormalizeTerminal(
    terminalWords,
    wrapper.wrappers.length === 0 ? context : "simple",
  );
  // reject one unsupported terminal command
  if (terminal === null) {
    return null;
  }
  return shellWrapCommandTree(wrapper, terminal, context);
}

// rebuild one complete wrapper parent tree
function shellWrapCommandTree(wrapper, terminal, context) {
  let child = terminal;
  // rebuild each proven wrapper from inner to outer
  for (let index = wrapper.wrappers.length - 1; index >= 0; index -= 1) {
    const current = wrapper.wrappers[index];
    child = [
      index === 0 ? context : "simple",
      current.command,
      [...current.normalizedArgs, ["child", child[1], child[2]]],
    ];
  }
  return child;
}

// locate the normalized provider below fixed wrappers
function shellNormalizedProvider(tree) {
  let current = tree;
  // peel each fixed wrapper child iteratively
  while (["exec", "sudo", "setpriv"].includes(current[1])) {
    const child = current[2].find(
      // locate one canonical child atom
      (argument) => argument[0] === "child",
    );
    // reject one incomplete wrapper tree
    if (child === undefined) {
      return null;
    }
    current = ["simple", child[1], child[2]];
  }
  return ["date", "sleep", "timeout"].includes(current[1]) ? current[1] : null;
}

// resolve one repository source target
function shellSourceTarget(value, source) {
  // map the reviewed installed deployment root
  if (value.startsWith(SHELL_INSTALLED_DEPLOY_PREFIX)) {
    const mapped = `deploy/${value.slice(SHELL_INSTALLED_DEPLOY_PREFIX.length)}`;
    return isRepositoryPath(mapped) ? mapped : null;
  }
  // resolve one explicitly relative target
  if (value.startsWith("./") || !value.includes("/")) {
    const target = source.resolutionBase === ""
      ? value
      : `${source.resolutionBase}/${value}`;
    const joined = shellNormalizePath(target);
    return joined !== null && isRepositoryPath(joined) ? joined : null;
  }
  // retain one already repository-relative target
  if (isRepositoryPath(value)) {
    return value;
  }
  return null;
}

// resolve one source target against the current directory header
function shellRuntimeSourceTarget(value, state) {
  const relative = value.startsWith("./") || !value.includes("/");
  // reject one relative target after directory uncertainty
  if (relative && state.environment.cwdBase === null) {
    return null;
  }
  const source = relative
    ? { ...state.source, resolutionBase: state.environment.cwdBase }
    : state.source;
  return shellSourceTarget(value, source);
}

// identify one reviewed command under resolved authority
function shellReviewedCommand(command, authority) {
  // retain one supported local or ambiguous function call
  if (authority !== "external") {
    return true;
  }
  return SHELL_REVIEWED_COMMANDS.has(command) || command.includes("/");
}

// create one nested wrapper call chain
function shellCallTree(
  state,
  words,
  environment,
  commandStart,
  commandEnd,
  outerAuthority,
) {
  const exactWords = [];
  let previous = null;
  // defer each apparent eval payload until terminal custody is proven
  for (const word of words) {
    const resolved = ["-e", "--eval"].includes(previous)
      ? null
      : shellResolveOptionalWord(
          word,
          state.source,
          environment,
          SHELL_LITERAL_BYTE_CAP,
          state.pathResolution,
        );
    exactWords.push(resolved);
    previous = resolved;
  }
  const authoritativeWrapper = outerAuthority === "external"
    ? shellParseWrapperChain(exactWords)
    : shellFinalizeCommandParse(exactWords, true, 0, []);
  const terminalAuthority = authoritativeWrapper.wrappers.length > 0
    ? "external"
    : outerAuthority;
  const wrapper = terminalAuthority === "external"
    ? authoritativeWrapper
    : {
        ...authoritativeWrapper,
        nodeEval: "none",
        payloadIndex: -1,
      };
  const payloadIndex = wrapper.payloadIndex;
  // retain one actual terminal node-e payload under the derived-source bound
  if (payloadIndex !== -1 && terminalAuthority === "external") {
    exactWords[payloadIndex] = shellResolveWord(
      words[payloadIndex],
      state.source,
      environment,
      DERIVED_SOURCE_BYTE_CAP,
      state.pathResolution,
    );
  }
  // reject one unsupported node inline-eval prefix without custody
  if (wrapper.nodeEval === "invalid" && terminalAuthority === "external") {
    shellPushDiagnostic(state);
  }
  // classify every oversized nonpayload argument once
  for (let index = 0; index < words.length; index += 1) {
    // record one opaque ordinary argument without retaining its payload
    if (index !== payloadIndex && shellDirectTextExceeds(words[index])) {
      shellPushDiagnostic(state);
    }
  }
  // retain one closed wrapper gap without downstream child claims
  if (!wrapper.ok) {
    shellPushDiagnostic(state);
    const func = shellWordNode(state, words[0], environment, "func");
    const outer = shellCreateNode(state, {
      attributes: { identifier: exactWords[0] },
      attach: false,
      end: commandEnd,
      factKind: "call",
      start: commandStart,
    });
    shellAddChild(outer, func, "func", 0);
    // retain invalid wrapper arguments as opaque syntax only
    for (let index = 1; index < words.length; index += 1) {
      shellAddChild(
        outer,
        shellOpaqueWordNode(state, words[index], environment),
        "args",
        index - 1,
      );
    }
    shellAttachNode(state, outer);
    return {
      exactWords,
      node: outer,
      payloadIndex,
      terminalAuthority,
      terminalNode: null,
      wrapper,
    };
  }
  const terminalWord = words[wrapper.terminalIndex];
  const terminal = exactWords[wrapper.terminalIndex];
  const func = shellWordNode(state, terminalWord, environment, "func");
  let child = shellCreateNode(state, {
    attributes: { identifier: terminal },
    attach: false,
    end: commandEnd,
    factKind: "call",
    start: terminalWord.start,
  });
  const terminalNode = child;
  shellAddChild(child, func, "func", 0);
  let argumentIndex = 0;
  // retain every terminal argument
  for (let index = wrapper.terminalIndex + 1; index < words.length; index += 1) {
    const embedded = index === payloadIndex && terminalAuthority === "external";
    const argument = wrapper.nodeEval === "invalid" && terminalAuthority === "external"
      ? shellOpaqueWordNode(state, words[index], environment)
      : shellWordNode(
        state,
        words[index],
        environment,
        "value",
        embedded ? DERIVED_SOURCE_BYTE_CAP : SHELL_LITERAL_BYTE_CAP,
        embedded ? "embedded-javascript" : "semantic-constant",
        embedded,
      );
    shellAddChild(
      child,
      argument,
      "args",
      argumentIndex,
    );
    argumentIndex += 1;
  }
  // rebuild every proven wrapper from inner to outer
  for (let index = wrapper.wrappers.length - 1; index >= 0; index -= 1) {
    const parsed = wrapper.wrappers[index];
    const wrapperWord = words[parsed.commandIndex];
    const outer = shellCreateNode(state, {
      attributes: { identifier: parsed.command },
      attach: false,
      end: commandEnd,
      factKind: "call",
      start: wrapperWord.start,
    });
    shellAddChild(
      outer,
      shellWordNode(state, wrapperWord, environment, "func"),
      "func",
      0,
    );
    let wrapperArgumentIndex = 0;
    // retain every parsed wrapper argument
    for (const wordIndex of parsed.argumentIndices) {
      shellAddChild(
        outer,
        shellWordNode(state, words[wordIndex], environment),
        "args",
        wrapperArgumentIndex,
      );
      wrapperArgumentIndex += 1;
    }
    shellAddChild(outer, child, "args", wrapperArgumentIndex);
    child = outer;
  }
  shellAttachNode(state, child);
  return {
    exactWords,
    node: child,
    payloadIndex,
    terminalAuthority,
    terminalNode,
    wrapper,
  };
}

// finalize one authoritative command parse and closed node-e prefix
function shellFinalizeCommandParse(values, ok, terminalIndex, wrappers) {
  let nodeEval = "none";
  let payloadIndex = -1;
  // classify only one proven terminal node command
  if (ok && values[terminalIndex] === "node") {
    const firstArgument = terminalIndex + 1;
    const evalOption = values[firstArgument] === "-e" || values[firstArgument] === "--eval";
    const mentionsEval = values.slice(firstArgument).some(
      // locate one known inline-eval option
      (value) => value === "-e" || value === "--eval",
    );
    // admit exactly one eval option followed by exactly one payload
    if (evalOption && values.length === firstArgument + 2) {
      nodeEval = "valid";
      payloadIndex = firstArgument + 1;
    }
    // reject every other known eval option placement
    if (nodeEval === "none" && mentionsEval) {
      nodeEval = "invalid";
    }
  }
  return { nodeEval, ok, payloadIndex, terminalIndex, wrappers };
}

// parse one authoritative closed wrapper chain
function shellParseWrapperChain(values) {
  const wrappers = [];
  let index = 0;
  // retain one direct nonwrapper command
  if (values[index] !== "exec") {
    return shellFinalizeCommandParse(
      values,
      !["sudo", "setpriv"].includes(values[index]),
      0,
      wrappers,
    );
  }
  // reject one missing exec child
  if (values.length < 2) {
    return shellFinalizeCommandParse(values, false, -1, wrappers);
  }
  wrappers.push({
    argumentIndices: [],
    command: "exec",
    commandIndex: index,
    normalizedArgs: [],
  });
  index += 1;
  // parse one exact sudo wrapper signature
  if (values[index] === "sudo") {
    const argumentIndices = [];
    const normalizedArgs = [];
    const commandIndex = index;
    index += 1;
    // require the exact admitted noninteractive flag
    if (values[index] !== "-n") {
      return shellFinalizeCommandParse(values, false, -1, wrappers);
    }
    argumentIndices.push(index);
    normalizedArgs.push(["option", "-n", null]);
    index += 1;
    // retain one optional child boundary marker
    if (values[index] === "--") {
      argumentIndices.push(index);
      normalizedArgs.push(["end-options"]);
      index += 1;
    }
    wrappers.push({
      argumentIndices,
      command: "sudo",
      commandIndex,
      normalizedArgs,
    });
  } else if (values[index] === "setpriv") {
    const argumentIndices = [];
    const normalizedArgs = [];
    const commandIndex = index;
    index += 1;
    // consume the exact ordered identity options
    for (const option of ["--reuid", "--regid"]) {
      const value = values[index];
      const prefix = `${option}=`;
      // reject one missing, separate, empty, or reordered value
      if (typeof value !== "string" || !value.startsWith(prefix) || value.length === prefix.length) {
        return shellFinalizeCommandParse(values, false, -1, wrappers);
      }
      argumentIndices.push(index);
      normalizedArgs.push(["option", option, shellAtom(value.slice(prefix.length))[0]]);
      index += 1;
    }
    // require the exact group-mode flag once
    if (values[index] !== "--clear-groups") {
      return shellFinalizeCommandParse(values, false, -1, wrappers);
    }
    argumentIndices.push(index);
    normalizedArgs.push(["option", "--clear-groups", null]);
    index += 1;
    // retain one optional child boundary marker
    if (values[index] === "--") {
      argumentIndices.push(index);
      normalizedArgs.push(["end-options"]);
      index += 1;
    }
    wrappers.push({
      argumentIndices,
      command: "setpriv",
      commandIndex,
      normalizedArgs,
    });
  }
  const terminal = values[index];
  // reject one missing, dynamic, option-like, duplicate, or nested privileged child
  if (
    typeof terminal !== "string" ||
    terminal.length === 0 ||
    terminal.startsWith("-") ||
    ["exec", "sudo", "setpriv"].includes(terminal)
  ) {
    return shellFinalizeCommandParse(values, false, -1, wrappers);
  }
  return shellFinalizeCommandParse(values, true, index, wrappers);
}

// retain one literal node-e derived source
function shellNodeEvalSource(state, words, exactWords, callNode, wrapper) {
  const scriptIndex = wrapper.payloadIndex;
  // stop without one valid literal eval payload or terminal call
  if (wrapper.nodeEval !== "valid" || scriptIndex === -1 || callNode === null) {
    return;
  }
  const scriptWord = words[scriptIndex];
  const script = exactWords[scriptIndex];
  // stop at one dynamic eval payload
  if (script === null) {
    shellPushDiagnostic(state);
    return;
  }
  const containerSpan = {
    endUtf16: callNode.end,
    startUtf16: callNode.start,
  };
  const rawSha256 = sha256(Buffer.from(script, "utf8"));
  const derivedId = frame("weather-backend-derived-source/v1", {
    containerSpan,
    language: "javascript",
    parentPath: state.source.path,
    parentRawSha256: state.source.rawSha256,
    rawSha256,
    resolutionBase: state.source.resolutionBase,
  });
  state.derivedSources.push({
    containerSpan,
    derivedId,
    language: "javascript",
    origins: ["embedded-javascript"],
    parentPath: state.source.path,
    parentRawSha256: state.source.rawSha256,
    path: `derived:${derivedId}`,
    rawSha256,
    resolutionBase: state.source.resolutionBase,
    sourceKind: "derived",
    sourceText: script,
  });
  const embedded = shellCreateNode(state, {
    attributes: {
      identifier: "javascript",
      literalKind: "string",
      literalRole: "embedded-javascript",
      literalValue: script,
    },
    attach: false,
    end: scriptWord.end,
    factKind: "embedded-source",
    start: scriptWord.start,
  });
  const argumentIndex = scriptIndex - wrapper.terminalIndex - 1;
  const prior = callNode.children.find(
    // locate the original literal argument
    (edge) => edge.role === "args" && edge.index === argumentIndex,
  );
  // retain the original expression below the embedded source
  if (prior !== undefined) {
    shellAddChild(embedded, prior.node, "value", 0);
    prior.node = embedded;
  } else {
    // retain one missing argument edge conservatively
    shellAddChild(callNode, embedded, "args", argumentIndex);
  }
  const terminalTree = [
    "simple",
    "node",
    [["option", exactWords[scriptIndex - 1], "literal"]],
  ];
  const tree = shellWrapCommandTree(wrapper, terminalTree, "simple");
  state.drafts.push(nativeDraft({
    confidence: "non-provider-container",
    countsAsClockCandidate: false,
    kind: "non-provider-container",
    language: "shell",
    normalizedExpressionSha256: expressionDigest(
      "shell",
      "shell-command-tree/v1",
      tree,
    ),
    ownerSymbol: state.ownerSymbol,
    sourceLocator: state.source.path,
    span: occurrenceSpan(state.spanIndex, callNode.start, callNode.end),
    surface: "container.shell-node-e",
    syntaxKind: "expanded-container",
  }));
}

// apply one exact namespace unset
function shellUnsetExact(state, environment, name) {
  const registry = environment.registry;
  const cell = registry.cells[shellEnvironmentCellId(environment, name)];
  const risk = shellEnvironmentReadonlyRisk(environment, name);
  // preserve one definitely readonly binding or absence
  if (risk === "known") {
    shellPushDiagnostic(state);
    return false;
  }
  // suppress one possibly readonly mutation
  if (risk === "possible") {
    shellEnvironmentTop(environment, name);
    shellPushDiagnostic(state);
    return false;
  }
  // retain one already absent target
  if (cell.kind === "absent") {
    return true;
  }
  // fail one readonly or unknown target conservatively
  if (cell.kind !== "exact" || !cell.mutable) {
    shellPushDiagnostic(state);
    return false;
  }
  shellEnvironmentUnset(environment, name);
  // retain one logical function-key mutation
  if (environment.namespace === "function") {
    shellRecordLogicalFunctionChange(state, name);
  }
  return true;
}

// promote one dynamic namespace effect
function shellUnsetUnknown(state, variable, functions) {
  // widen one variable namespace
  if (variable) {
    shellEnvironmentPromoteTop(state.environment);
  }
  // widen one function namespace
  if (functions) {
    shellEnvironmentPromoteTop(state.functionEnvironment);
  }
  shellPushDiagnostic(state);
}

// apply one closed unset builtin invocation
function shellApplyUnset(state, exactWords) {
  let mode = "default";
  let cursor = 1;
  let optionPhase = true;
  let unsupported = false;
  // parse one exact option phase
  while (cursor < exactWords.length && optionPhase) {
    const word = exactWords[cursor];
    // reject one dynamic option or target boundary
    if (typeof word !== "string") {
      unsupported = true;
      break;
    }
    // retain one exact option terminator
    if (word === "--") {
      optionPhase = false;
      cursor += 1;
      break;
    }
    // select one exact function mode
    if (word === "-f") {
      unsupported ||= mode === "variable" || mode === "function";
      mode = "function";
      cursor += 1;
      continue;
    }
    // select one exact variable mode
    if (word === "-v") {
      unsupported ||= mode === "function" || mode === "variable";
      mode = "variable";
      cursor += 1;
      continue;
    }
    // stop at one target spelling
    if (!word.startsWith("-")) {
      break;
    }
    unsupported = true;
    break;
  }
  const targets = exactWords.slice(cursor);
  // reject one unknown option shape or target
  if (
    unsupported ||
    targets.length === 0 ||
    targets.some(
      // require one exact identifier target
      (name) => typeof name !== "string" || !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(name),
    )
  ) {
    shellUnsetUnknown(
      state,
      mode !== "function",
      mode !== "variable",
    );
    return;
  }
  // apply every exact target once
  for (const name of targets) {
    // remove one selected function
    if (mode === "function") {
      shellUnsetExact(state, state.functionEnvironment, name);
      continue;
    }
    // remove one selected variable
    if (mode === "variable") {
      // record one successful, rejected, or uncertain variable mutation
      shellUnsetExact(state, state.environment, name);
      shellRecordLogicalChange(state, name);
      continue;
    }
    const variableCell = state.environment.registry.cells[
      shellEnvironmentCellId(state.environment, name)
    ];
    // prefer one definitely present mutable variable
    if (variableCell.kind === "exact") {
      // stop on one readonly failure without function fallback
      if (!variableCell.mutable) {
        shellPushDiagnostic(state);
        continue;
      }
      // suppress one possible readonly default mutation
      if (shellEnvironmentReadonlyRisk(state.environment, name) === "possible") {
        shellEnvironmentTop(state.environment, name);
        shellRecordLogicalChange(state, name);
        shellPushDiagnostic(state);
        continue;
      }
      shellEnvironmentUnset(state.environment, name);
      shellRecordLogicalChange(state, name);
      continue;
    }
    // fall back only after definite variable absence
    if (variableCell.kind === "absent") {
      // preserve one invisible readonly variable without function fallback
      if (!variableCell.mutable) {
        shellPushDiagnostic(state);
        continue;
      }
      shellUnsetExact(state, state.functionEnvironment, name);
      continue;
    }
    shellUnsetUnknown(state, true, true);
  }
}

// invalidate one unproved read effect
function shellApplyUnknownRead(state) {
  shellEnvironmentPromoteTop(state.environment);
  shellPushDiagnostic(state);
}

// parse one closed read builtin target grammar
function shellReadTargets(exactWords) {
  const values = exactWords.slice(1);
  // reject every dynamic expanded argument
  if (values.some(
    // locate one unresolved argument
    (value) => typeof value !== "string",
  )) {
    return null;
  }
  let cursor = 0;
  // consume the sole admitted no-value option
  if (values[cursor] === "-r") {
    cursor += 1;
  }
  let targets;
  // consume one exact array target option
  if (values[cursor] === "-a") {
    const target = values[cursor + 1];
    // reject one missing or nonidentifier array target
    if (target === undefined || !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(target)) {
      return null;
    }
    targets = [target];
    cursor += 2;
    // reject nonname ignored operands under the closed grammar
    if (values.slice(cursor).some(
      // require one literal ignored name
      (name) => !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(name),
    )) {
      return null;
    }
  } else if (values[cursor] === "-d") {
    // require one exact delimiter argument
    if (values[cursor + 1] === undefined) {
      return null;
    }
    cursor += 2;
    targets = values.slice(cursor);
  } else {
    targets = values.slice(cursor);
  }
  // reject every unadmitted option or target spelling
  if (targets.some(
    // require one exact scalar target
    (name) => !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(name),
  )) {
    return null;
  }
  // select the default read target
  if (targets.length === 0) {
    targets = ["REPLY"];
  }
  return targets;
}

// apply one closed read builtin target grammar
function shellApplyRead(state, exactWords) {
  const targets = shellReadTargets(exactWords);
  // reject one unproved read option or target
  if (targets === null) {
    shellApplyUnknownRead(state);
    return;
  }
  // apply every sequential read target once
  for (const name of targets) {
    shellInvalidateBinding(state, name);
  }
}

// apply one arbitrary code-capable shell effect
function shellApplyAllCodeEffects(state) {
  shellApplyAuthorityHeaderEffect(
    state,
    SHELL_AUTHORITY_LOCAL,
    SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN,
  );
}

// classify one exact printf format mutation
function shellPrintfFormatEffect(format) {
  let cursor = 0;
  // inspect every format conversion once
  while (cursor < format.length) {
    const percent = format.indexOf("%", cursor);
    // retain one complete safe suffix
    if (percent === -1) {
      return "safe";
    }
    // consume one escaped percent
    if (format[percent + 1] === "%") {
      cursor = percent + 2;
      continue;
    }
    let conversion = percent + 1;
    // consume one parenthesized time format
    if (format[conversion] === "(") {
      const close = format.indexOf(")", conversion + 1);
      // reject one incomplete time conversion
      if (close === -1 || format[close + 1] !== "T") {
        return "unknown";
      }
      cursor = close + 2;
      continue;
    }
    // locate one conversion terminator
    while (conversion < format.length && !/[A-Za-z%]/u.test(format[conversion])) {
      conversion += 1;
    }
    // reject one incomplete conversion
    if (conversion >= format.length) {
      return "unknown";
    }
    // retain one name-writing conversion
    if (format[conversion] === "n") {
      return "writes";
    }
    cursor = conversion + 1;
  }
  return "safe";
}

// apply one closed printf mutation grammar
function shellApplyPrintf(state, exactWords) {
  const variableMode = exactWords[1] === "-v";
  const target = variableMode ? exactWords[2] : null;
  const format = exactWords[variableMode ? 3 : 1];
  // reject one dynamic or incomplete format
  if (typeof format !== "string") {
    shellEnvironmentPromoteTop(state.environment);
    shellPushDiagnostic(state);
    return;
  }
  const effect = shellPrintfFormatEffect(format);
  // reject one multi-target or unparsed format
  if (effect !== "safe") {
    shellEnvironmentPromoteTop(state.environment);
    shellPushDiagnostic(state);
    return;
  }
  // retain one ordinary state-pure printf
  if (!variableMode) {
    return;
  }
  // reject one dynamic printf target
  if (typeof target !== "string" || !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(target)) {
    shellEnvironmentPromoteTop(state.environment);
    shellPushDiagnostic(state);
    return;
  }
  shellInvalidateBinding(state, target);
}

// parse one closed mapfile target grammar
function shellMapfileTarget(exactWords) {
  let cursor = 1;
  let sawDelimiter = false;
  let sawTrim = false;
  // parse the admitted exact option phase
  while (cursor < exactWords.length) {
    const word = exactWords[cursor];
    // retain one line-trimming option
    if (word === "-t" && !sawTrim) {
      sawTrim = true;
      cursor += 1;
      continue;
    }
    // retain one empty delimiter option
    if (word === "-d" && !sawDelimiter && exactWords[cursor + 1] === "") {
      sawDelimiter = true;
      cursor += 2;
      continue;
    }
    break;
  }
  const remaining = exactWords.slice(cursor);
  // reject one dynamic, callback, or unadmitted mapfile shape
  if (
    remaining.length > 1 ||
    remaining.some(
      // locate one unresolved or nonidentifier target
      (name) => typeof name !== "string" || !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(name),
    ) ||
    exactWords.slice(1, cursor).some(
      // reject one unresolved consumed option value
      (value) => typeof value !== "string",
    )
  ) {
    return null;
  }
  return remaining[0] ?? "MAPFILE";
}

// apply one closed mapfile target grammar
function shellApplyMapfile(state, exactWords) {
  const target = shellMapfileTarget(exactWords);
  // reject one dynamic callback or unadmitted option
  if (target === null) {
    shellApplyAllCodeEffects(state);
    return;
  }
  shellInvalidateBinding(state, target);
}

// apply one closed export mutation grammar
function shellApplyExport(state, exactWords) {
  const mutations = shellExportMutations(exactWords);
  // fail closed for one unproved export shape
  if (mutations === null) {
    shellApplyAllCodeEffects(state);
    return;
  }
  // apply every exact export target once
  for (const mutation of mutations) {
    // write one exact assigned export value
    if (mutation.assigned) {
      shellUpdateBinding(state, mutation.name, mutation.value);
      continue;
    }
    // retain one existing dynamic binding unchanged
    if (shellEnvironmentHas(state.environment, mutation.name)) {
      continue;
    }
    // create one invisible unset variable binding
    shellUpdateBinding(state, mutation.name, SHELL_VARIABLE_UNSET);
  }
}

// apply one closed invocation effect descriptor
function shellApplyInvocationEffect(state, effect) {
  // widen one modeled variable namespace
  if ((effect.bits & SHELL_EFFECT_VAR_WRITE) !== 0) {
    shellEnvironmentPromoteTop(state.environment);
  }
  // widen one modeled function namespace
  if ((effect.bits & SHELL_EFFECT_FN_WRITE) !== 0) {
    shellEnvironmentPromoteTop(
      state.functionEnvironment,
      effect.functionCause ?? SHELL_AUTHORITY_LOCAL,
      effect.latentMask ?? 0,
    );
  }
  // retain one modeled variable-readonly risk
  if ((effect.bits & SHELL_EFFECT_MAY_RO_VAR) !== 0) {
    shellEnvironmentMarkReadonlyUnknown(state.environment);
  }
  // retain one modeled function-readonly risk
  if ((effect.bits & SHELL_EFFECT_MAY_RO_FN) !== 0) {
    shellEnvironmentMarkReadonlyUnknown(state.functionEnvironment);
  }
  // expose only one genuinely open effect
  if ((effect.bits & SHELL_EFFECT_OPEN) !== 0) {
    shellPushDiagnostic(state);
  }
}

// apply one unresolved authority header effect
function shellApplyAuthorityHeaderEffect(state, cause, mask) {
  shellApplyInvocationEffect(state, {
    bits: mask,
    functionCause: cause,
    kind: "authority-header",
    latentMask: mask,
  });
}

// install one latent sourced-code authority barrier
function shellApplySourceAuthorityBarrier(state, cause, mask) {
  shellEnvironmentPromoteTop(state.environment);
  shellEnvironmentPromoteTop(state.functionEnvironment, cause, mask);
  // expose only one intrinsic dynamic-source gap
  if ((mask & SHELL_EFFECT_OPEN) !== 0) {
    shellPushDiagnostic(state);
  }
}

// apply one authority-proven builtin effect
function shellApplyBuiltinEffect(state, command, exactWords) {
  const effect = shellInvocationEffect(command, exactWords);
  // retain effects owned by their exact command handlers
  if (command === "read" || command === "unset") {
    return;
  }
  // retain the sole admitted command query form
  if (command === "command") {
    // admit one raw query without executing its name operand
    if (exactWords.length === 3 && exactWords[1] === "-v") {
      return;
    }
    shellApplyAllCodeEffects(state);
    return;
  }
  // reject arbitrary evaluated shell code
  if (command === "eval") {
    shellApplyAllCodeEffects(state);
    return;
  }
  // apply one exact export mutation
  if (command === "export" && effect.kind === "export") {
    shellApplyExport(state, exactWords);
    return;
  }
  // invalidate directory-dependent state conservatively
  if (command === "cd" && effect.kind === "cd") {
    const pwdRisk = shellEnvironmentReadonlyRisk(state.environment, "PWD");
    const oldPwdRisk = shellEnvironmentReadonlyRisk(state.environment, "OLDPWD");
    const target = exactWords[1];
    // retain one exact directory only when shell metadata can update
    if (pwdRisk === "none" && oldPwdRisk === "none") {
      state.environment.cwdBase = target.startsWith("/")
        ? shellNormalizePath(target)
        : state.environment.cwdBase === null
          ? null
          : shellNormalizePath(`${state.environment.cwdBase}/${target}`);
    } else {
      // chdir may precede one readonly metadata failure
      state.environment.cwdBase = null;
    }
    shellInvalidateBinding(state, "PWD");
    shellInvalidateBinding(state, "OLDPWD");
    shellPushDiagnostic(state);
    return;
  }
  // apply one printf target effect
  if (command === "printf") {
    shellApplyPrintf(state, exactWords);
    return;
  }
  // apply one mapfile target effect
  if (command === "mapfile") {
    shellApplyMapfile(state, exactWords);
    return;
  }
  // retain one explicitly proven parent-binding noop
  if (effect.bits === 0) {
    return;
  }
  shellApplyInvocationEffect(state, effect);
}

// apply one constant local function binding effect
function shellApplyLocalFunctionEffect(state, command) {
  const value = shellEnvironmentGet(state.functionEnvironment, command);
  const effect = shellFunctionLocalEffect(value) ??
    shellImportedFunctionEffect(state, command);
  // reject one missing exact local summary
  if (effect === null) {
    shellApplyAllCodeEffects(state);
    return;
  }
  const baseBits = effect.effectMask | (effect.open ? SHELL_EFFECT_OPEN : 0);
  const latentMask = state.functionEnvironment.latentMask;
  const baseFunctionCause = effect.functionCause === SHELL_AUTHORITY_NONE ||
    effect.functionCause === undefined
    ? SHELL_AUTHORITY_LOCAL
    : effect.functionCause;
  const functionCause = (baseBits & SHELL_EFFECT_FN_WRITE) !== 0
    ? shellMergeAuthorityCause(
        baseFunctionCause,
        state.functionEnvironment.authorityCause,
      )
    : state.functionEnvironment.authorityCause;
  shellApplyInvocationEffect(state, {
    bits: baseBits | latentMask,
    functionCause,
    kind: "local",
    latentMask,
  });
}

// split one owned compound list into physical command groups
function shellCompoundCommandGroups(tokens) {
  const groups = [];
  let current = [];
  const stack = [];
  const closers = new Map([
    ["(", ")"],
    ["((", "))"],
    ["[[", "]]"],
    ["{", "}"],
  ]);
  // consume every compound token once
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    const activeClose = stack.at(-1);
    // retain opaque expression internals
    if (activeClose === "]]" || activeClose === "))") {
      // close one exact opaque expression
      if (shellOwnedExpressionClose(tokens, index, activeClose)) {
        stack.pop();
      }
      current.push(token);
      continue;
    }
    // enter one nested typed range
    if (token.kind === "operator" && closers.has(token.value)) {
      stack.push(closers.get(token.value));
      current.push(token);
      continue;
    }
    // leave one nested typed range
    if (
      token.kind === "operator" &&
      [")", "))", "]]", "}"].includes(token.value)
    ) {
      // reject one malformed nested range without reclassifying it
      if (stack.pop() !== token.value) {
        return null;
      }
      current.push(token);
      continue;
    }
    // finish one top-level physical command group
    if (
      stack.length === 0 &&
      token.kind === "operator" &&
      token.value === ";"
    ) {
      // retain one nonempty group
      if (current.length > 0) {
        groups.push(current);
      }
      current = [];
      continue;
    }
    current.push(token);
  }
  // reject one unclosed typed range
  if (stack.length > 0) {
    return null;
  }
  // retain one final physical group
  if (current.length > 0) {
    groups.push(current);
  }
  return groups;
}

// analyze one owned compound list without leaking structural markers
function shellProcessCompoundSegments(state, tokens, context, parent) {
  const groups = shellCompoundCommandGroups(tokens);
  // reject one malformed compound list transactionally
  if (groups === null) {
    shellPushDiagnostic(state);
    return;
  }
  const priorContexts = state.contexts.length;
  // attach compound facts to one explicit parent
  if (parent !== null) {
    state.contexts.push({ node: parent, type: "compound" });
  }
  // execute every owned physical group once
  for (const group of groups) {
    (0, shellProcessLine)(state, group, null, context);
  }
  // restore one explicit parent context
  if (parent !== null && state.contexts.length > priorContexts) {
    state.contexts.length = priorContexts;
  }
}

// analyze one complete process-local control boundary
function shellAnalyzeIsolatedSegments(state, tokens, context, parent) {
  const savedCase = state.currentCase;
  const savedEnvironment = state.environment;
  const savedFunctionEnvironment = state.functionEnvironment;
  const savedIf = state.currentIf;
  const savedIsolatedDepth = state.isolatedDepth;
  const savedLoop = state.currentLoop;
  const savedContinues = state.segmentContinues;
  state.currentCase = null;
  state.currentIf = null;
  state.isolatedDepth += 1;
  state.currentLoop = null;
  state.segmentContinues = true;
  state.environment = shellCloneEnvironment(savedEnvironment, true);
  state.functionEnvironment = shellCloneEnvironment(savedFunctionEnvironment, true);
  // retain one nonempty isolated body
  if (tokens.length > 0) {
    (0, shellProcessCompoundSegments)(state, tokens, context, parent);
  }
  state.segmentContinues = savedContinues;
  state.functionEnvironment = savedFunctionEnvironment;
  state.environment = savedEnvironment;
  state.isolatedDepth = savedIsolatedDepth;
  state.currentLoop = savedLoop;
  state.currentIf = savedIf;
  state.currentCase = savedCase;
}

// analyze owned process substitutions in isolated environments
function shellAnalyzeProcessSubstitutions(state, substitutions, context, parent) {
  // analyze every closed substitution body once
  for (const body of substitutions) {
    const tokens = [];
    // materialize one shallow legacy body only
    for (let index = body.start; index < body.end; index += 1) {
      tokens.push(body.tokens[index]);
    }
    const analyzeIsolated = shellAnalyzeIsolatedSegments;
    analyzeIsolated(state, tokens, context, parent);
  }
}

// analyze one resolved simple command
function shellProcessSimple(
  state,
  tokens,
  context = "simple",
  explicitParent = null,
  start = 0,
  end = tokens.length,
  suppliedShape = null,
) {
  const commandShape = suppliedShape ?? shellCommandShape(tokens, start, end);
  // reject one malformed redirection transactionally
  if (!commandShape.ok) {
    shellPushDiagnostic(state);
    return null;
  }
  const priorContextCount = state.contexts.length;
  // install one syntax owner before any command emission
  if (explicitParent !== null) {
    state.contexts.push({ node: explicitParent, type: "substitution" });
  }
  // restore one syntax owner across every early return
  try {
  // avoid replaying substitutions already owned by the frame driver
  if (state.semanticFrameSubstitutions !== true) {
    (0, shellAnalyzeProcessSubstitutions)(
      state,
      commandShape.substitutions ?? [],
      "process-substitution",
      explicitParent,
    );
  }
  const words = commandShape.words;
  // stop at one empty command
  if (words.length === 0) {
    return null;
  }
  let wordIndex = 0;
  let declarationStart = null;
  const declarationFlags = [];
  const firstLiteral = shellOptionalLiteralWord(words[0]);
  const firstAuthority = firstLiteral === null
    ? "unknown"
    : shellCommandAuthority(state, firstLiteral, words[0].start);
  const declarationBuiltin =
    ["local", "export", "readonly", "declare", "typeset"].includes(firstLiteral) &&
    firstAuthority === "external";
  // consume one authoritative declaration builtin
  if (declarationBuiltin) {
    declarationStart = words[0].start;
    wordIndex = 1;
    // skip declaration flags
    while (wordIndex < words.length && (shellOptionalLiteralWord(words[wordIndex]) ?? "").startsWith("-")) {
      declarationFlags.push(shellOptionalLiteralWord(words[wordIndex]));
      wordIndex += 1;
    }
  }
  const supportedDeclarationFlags = new Set(["--", "-a", "-A", "-g", "-i", "-l", "-n", "-r", "-t", "-u", "-x"]);
  const functionDeclarationFlag = declarationFlags.some(
    // identify one function-affecting option
    (flag) => flag === "-f" || flag === "-F",
  );
  const unsupportedDeclarationFlag = declarationFlags.some(
    // reject one unmodeled declaration option
    (flag) => !supportedDeclarationFlags.has(flag) && flag !== "-f" && flag !== "-F",
  );
  // fail closed before applying one unadmitted export grammar
  if (firstLiteral === "export") {
    const exactExportWords = words.map(
      // resolve every export operand once
      (word) => shellResolveOptionalWord(
        word,
        state.source,
        state.environment,
        SHELL_LITERAL_BYTE_CAP,
        state.pathResolution,
      ),
    );
    const exportEffect = shellInvocationEffect("export", exactExportWords);
    // reject one dynamic or option-bearing export shape
    if (exportEffect.kind === "all") {
      shellCallTree(
        state,
        words,
        state.environment,
        words[0].start,
        tokens[end - 1]?.end ?? words.at(-1).end,
        firstAuthority,
      );
      shellApplyInvocationEffect(state, exportEffect);
      return null;
    }
  }
  // widen one unmodeled declaration effect
  if (declarationBuiltin && (functionDeclarationFlag || unsupportedDeclarationFlag)) {
    const readonlyFunctionEffect = declarationFlags.some(
      // locate one function-readonly attribute cluster
      (flag) => typeof flag === "string" && flag.includes("f") && (
        firstLiteral === "readonly" || flag.includes("r")
      ),
    );
    const readonlyVariableEffect = firstLiteral === "readonly" &&
      unsupportedDeclarationFlag && !readonlyFunctionEffect;
    shellEnvironmentPromoteTop(state.functionEnvironment);
    // retain one unsupported function-readonly risk
    if (readonlyFunctionEffect) {
      shellEnvironmentMarkReadonlyUnknown(state.functionEnvironment);
    }
    // widen variables for one generally unmodeled option
    if (unsupportedDeclarationFlag) {
      shellEnvironmentPromoteTop(state.environment);
    }
    // retain one unsupported variable-readonly risk
    if (readonlyVariableEffect) {
      shellEnvironmentMarkReadonlyUnknown(state.environment);
    }
    shellPushDiagnostic(state);
    return null;
  }
  const declarationMutable = firstLiteral !== "readonly" &&
    !declarationFlags.includes("-r");
  const readonlyScope = declarationMutable || (
    state.ownerSymbol === "<module>" && firstLiteral !== "local"
  ) || words.slice(wordIndex).every(
    // retain exact function-local readonly targets
    (word) => {
      const assignment = shellAssignment(word);
      const value = shellOptionalLiteralWord(word);
      const name = assignment?.name ?? value;
      return typeof name === "string" && shellLocalNameHas(state, name);
    },
  );
  // consume leading assignments
  while (wordIndex < words.length) {
    const assignment = shellAssignment(words[wordIndex]);
    // stop at one nonassignment word
    if (assignment === null) {
      break;
    }
    const originalTokenIndex = commandShape.wordTokenIndexes[wordIndex];
    const array = shellArrayAssignment(tokens, originalTokenIndex, assignment, end);
    // reject one malformed array
    if (array?.malformed) {
      shellPushDiagnostic(state);
      // retain one malformed readonly declaration as risk
      if (declarationMutable) {
        shellInvalidateBinding(state, assignment.name);
      } else {
        shellDeclareReadonlyUnknown(state, assignment.name);
      }
      return null;
    }
    shellBindAssignment(
      state,
      assignment,
      words[wordIndex],
      declarationStart,
      array,
      declarationMutable,
      readonlyScope,
    );
    // retain one exact function-local declaration owner
    if (firstLiteral === "local") {
      shellLocalNameAdd(state, assignment.name);
    }
    // advance across one array spelling
    if (array !== null) {
      const closing = tokens[array.nextIndex - 1];
      // skip words contained by the array
      while (wordIndex + 1 < words.length && words[wordIndex + 1].end <= closing.end) {
        wordIndex += 1;
      }
    }
    wordIndex += 1;
  }
  // retain declaration-only bindings
  if (declarationStart !== null) {
    // bind every remaining declared name
    for (; wordIndex < words.length; wordIndex += 1) {
      const name = shellLiteralWord(words[wordIndex]);
      // reject one dynamic declaration target
      if (name === null || !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(name)) {
        // preserve one unresolved readonly-capable namespace effect
        if (!declarationMutable) {
          shellEnvironmentPromoteTop(state.environment);
          shellEnvironmentMarkReadonlyUnknown(state.environment);
        }
        shellPushDiagnostic(state);
        continue;
      }
      const target = shellCreateNode(state, {
        attributes: { identifier: name },
        attach: false,
        end: words[wordIndex].end,
        factKind: "name",
        start: words[wordIndex].start,
      });
      const binding = shellCreateNode(state, {
        attributes: { identifier: name },
        attach: true,
        end: words[wordIndex].end,
        factKind: "binding",
        start: declarationStart,
      });
      shellAddChild(binding, target, "target", 0);
      // create one invisible unset export binding
      if (
        firstLiteral === "export" &&
        !shellEnvironmentHas(state.environment, name)
      ) {
        shellUpdateBinding(state, name, SHELL_VARIABLE_UNSET);
      }
      // retain one declaration-only readonly attribute
      if (!declarationMutable && readonlyScope) {
        shellDeclareReadonlyBinding(state, name);
      } else if (!declarationMutable) {
        shellDeclareReadonlyUnknown(state, name);
      } else if (firstLiteral === "local") {
        // invalidate one local declaration
        shellInvalidateBinding(state, name);
        shellLocalNameAdd(state, name);
      }
    }
    return null;
  }
  // finish one assignment-only statement
  if (wordIndex >= words.length) {
    return null;
  }
  const commandWords = words.slice(wordIndex);
  const commandStart = commandWords[0].start;
  const commandEnd = tokens[end - 1]?.end ?? commandWords.at(-1).end;
  const command = shellResolveOptionalWord(
    commandWords[0],
    state.source,
    state.environment,
    SHELL_LITERAL_BYTE_CAP,
    state.pathResolution,
  );
  const outerAuthority = command === null
    ? "unknown-local"
    : shellCommandAuthority(state, command, commandStart);
  const call = shellCallTree(
    state,
    commandWords,
    state.environment,
    commandStart,
    commandEnd,
    outerAuthority,
  );
  const exactWords = call.exactWords;
  const terminalCommand = call.wrapper.ok
    ? exactWords[call.wrapper.terminalIndex]
    : null;
  const pendingCandidateAuthority =
    call.terminalAuthority === "unknown-pending" ||
    call.terminalAuthority === "unknown-source";
  const directCandidateAuthority = pendingCandidateAuthority &&
    call.wrapper.wrappers.length === 0;
  const providerAuthority = call.terminalAuthority === "external" ||
    directCandidateAuthority;
  const providerDemand = providerAuthority &&
    ["date", "sleep", "timeout"].includes(terminalCommand);
  let invocationGapOwned = false;
  const normalizedWords = [];
  let normalizedComplete = true;
  // expand only uniquely reachable constant arrays for command normalization
  for (let index = 0; index < commandWords.length; index += 1) {
    const word = commandWords[index];
    const value = index === call.payloadIndex
      ? exactWords[index]
      : shellResolveOptionalWordSequence(
          word,
          state.source,
          state.environment,
          SHELL_LITERAL_BYTE_CAP,
          state.pathResolution,
        );
    // invalidate one dynamic command sequence
    if (value === null) {
      normalizedComplete = false;
      break;
    }
    // reject one multiply reached array DAG without materializing it
    if (!shellAppendResolvedSequence(normalizedWords, value)) {
      normalizedComplete = false;
      shellPushDiagnostic(state);
      break;
    }
  }
  // apply one local or unresolved callee effect before consumers
  if (outerAuthority === "local" && command !== null) {
    shellApplyLocalFunctionEffect(state, command);
  } else if (outerAuthority === "unknown-local") {
    // retain lexer-owned arithmetic fragments without guessing a callee
    if (!state.arithmeticContinuation) {
      shellApplyAllCodeEffects(state);
      invocationGapOwned = true;
    }
  }
  // emit one exact source edge
  if (
    (command === "source" || command === ".") &&
    outerAuthority === "external"
  ) {
    const targetValue = commandWords.length === 2
      ? shellResolveWord(
          commandWords[1],
          state.source,
          state.environment,
          SHELL_LITERAL_BYTE_CAP,
          state.pathResolution,
        )
      : null;
    const target = targetValue === null ? null : shellRuntimeSourceTarget(targetValue, state);
    // retain one reviewed repository source
    if (target !== null) {
      const importFact = shellCreateNode(state, {
        attributes: {
          literalKind: "string",
          literalRole: "import-specifier",
          literalValue: target,
        },
        attach: false,
        end: commandEnd,
        factKind: "import",
        start: commandStart,
      });
      shellAddChild(call.node, importFact, "value", 0);
      const importEffect = shellCompleteImportEffect(
        state,
        target,
        commandStart,
      );
      // retain one present incomplete dependency as intrinsic source code
      if (shellIncompleteProvidedImport(state, target)) {
        shellApplySourceAuthorityBarrier(
          state,
          SHELL_AUTHORITY_SOURCE,
          SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN,
        );
      } else if (importEffect === null) {
        // retain one missing dependency as a latent authority barrier
        shellApplySourceAuthorityBarrier(
          state,
          SHELL_AUTHORITY_PENDING,
          SHELL_EFFECT_ALL_BINDINGS,
        );
      } else {
        const importBits = importEffect.effectMask |
          (importEffect.open ? SHELL_EFFECT_OPEN : 0);
        // apply one complete initializer effect without reopening authority
        if (importBits !== 0) {
          shellApplyInvocationEffect(state, {
            bits: importBits,
            functionCause: importEffect.functionCause,
            kind: "source-initializer",
          });
        }
      }
    } else {
      shellApplySourceAuthorityBarrier(
        state,
        SHELL_AUTHORITY_SOURCE,
        SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN,
      );
    }
    return call.node;
  }
  // invalidate explicit environment mutators
  if (command === "unset" && outerAuthority === "external") {
    shellApplyUnset(state, exactWords);
  }
  // apply one exact read target grammar
  if (command === "read" && outerAuthority === "external") {
    shellApplyRead(state, exactWords);
  }
  // apply every other reviewed builtin effect behind raw authority
  if (
    outerAuthority === "external" &&
    command !== null &&
    SHELL_STATEFUL_BUILTIN_COMMANDS.has(command)
  ) {
    shellApplyBuiltinEffect(state, command, exactWords);
  }
  // retain the sole admitted child-local umask form
  if (
    outerAuthority === "external" &&
    command === "umask" &&
    !(state.isolatedDepth > 0 && exactWords.length === 2 && exactWords[1] === "077")
  ) {
    shellApplyAllCodeEffects(state);
  }
  // retain only fully resolved provider words
  if (normalizedComplete && providerAuthority) {
    const tree = normalizeShell(normalizedWords, context, call.wrapper);
    const provider = tree === null ? null : shellNormalizedProvider(tree);
    // emit one closed clock command
    if (tree !== null && provider !== null) {
      state.drafts.push(nativeDraft({
        confidence: "expanded-direct",
        countsAsClockCandidate: true,
        kind: provider === "date" ? "current-time-read" : "timer-schedule",
        language: "shell",
        normalizedExpressionSha256: expressionDigest(
          "shell",
          "shell-command-tree/v1",
          tree,
        ),
        ownerSymbol: state.ownerSymbol,
        sourceLocator: state.source.path,
        span: occurrenceSpan(state.spanIndex, commandStart, commandEnd),
        surface: provider === "date" && call.wrapper.wrappers.length > 0
          ? "shell.wrapper-now"
          : `shell.${provider}`,
        syntaxKind: context === "command-substitution"
          ? "shell-command-substitution"
          : "shell-command",
      }));
    } else if (providerDemand) {
      shellPushDiagnostic(state);
    }
  } else if (providerDemand) {
    shellPushDiagnostic(state);
  }
  shellNodeEvalSource(
    state,
    commandWords,
    exactWords,
    call.terminalNode,
    call.wrapper,
  );
  // retain one ambiguous function target as a sanitized gap
  if (outerAuthority === "unknown-local" && !invocationGapOwned) {
    shellPushDiagnostic(state);
    invocationGapOwned = true;
  }
  // mark one unknown command target
  if (
    (command === null || !shellReviewedCommand(command, outerAuthority)) &&
    call.wrapper.ok &&
    !invocationGapOwned
  ) {
    shellPushDiagnostic(state);
  }
  // mark one noncontinuing command
  if (
    ["exec", "exit", "return"].includes(command) &&
    outerAuthority === "external"
  ) {
    state.segmentContinues = false;
    // isolate noncontinuation inside one pipeline component
    if (!state.trackAssignments) {
      return call.node;
    }
    // stop only the active conditional branch
    if (state.currentIf !== null) {
      state.currentIf.currentContinues = false;
    } else {
      // preserve zero-iteration loop continuation
      if (state.currentLoop === null) {
        // stop one direct case continuation
        if (state.currentCase !== null && state.currentCase.currentArm !== null) {
          state.currentCase.currentArm.continues = false;
        }
      }
    }
  }
  return call.node;
  } finally {
    // remove only the syntax owner installed by this command
    if (explicitParent !== null && state.contexts.length > priorContextCount) {
      state.contexts.length = priorContextCount;
    }
  }
}

// split top-level command operators
function shellSplitSegments(tokens) {
  const segments = [];
  let current = [];
  const stack = [];
  const closers = new Map([
    ["(", ")"],
    ["((", "))"],
    ["[[", "]]"],
    ["{", "}"],
  ]);
  // scan every logical-line token
  for (let tokenIndex = 0; tokenIndex < tokens.length; tokenIndex += 1) {
    const token = tokens[tokenIndex];
    const activeClose = stack.at(-1);
    // retain opaque test and arithmetic internals
    if (activeClose === "]]" || activeClose === "))") {
      // close one owned opaque expression
      if (shellOwnedExpressionClose(tokens, tokenIndex, activeClose)) {
        stack.pop();
      }
      current.push(token);
      continue;
    }
    // track one typed nested group
    if (token.kind === "operator" && closers.has(token.value)) {
      stack.push(closers.get(token.value));
      current.push(token);
      continue;
    }
    // close one exact typed group
    if (token.kind === "operator" && [")", "))", "]]", "}"].includes(token.value)) {
      // reject one unmatched or mismatched close
      if (stack.pop() !== token.value) {
        return { ok: false, segments: [] };
      }
      current.push(token);
      continue;
    }
    // split one top-level command relation
    if (
      stack.length === 0 &&
      token.kind === "operator" &&
      [";", "&&", "||", "|", "|&"].includes(token.value)
    ) {
      // reject one missing relation operand
      if (current.length === 0 && token.value !== ";") {
        return { ok: false, segments: [] };
      }
      // retain one nonempty segment
      if (current.length > 0) {
        segments.push({ operator: token.value, tokens: current });
      }
      current = [];
      continue;
    }
    current.push(token);
  }
  // reject one unterminated group or dangling relation
  if (
    stack.length > 0 ||
    (segments.length > 0 && current.length === 0 &&
      ["&&", "||", "|", "|&"].includes(segments.at(-1).operator))
  ) {
    return { ok: false, segments: [] };
  }
  // retain one final segment
  if (current.length > 0) {
    segments.push({ operator: null, tokens: current });
  }
  return { ok: true, segments };
}

// process one shell test expression segment
function shellProcessTestSegment(state, tokens, parent, start = 0, end = tokens.length) {
  const opener = tokens[start];
  // reject one ordinary command segment
  if (opener?.kind !== "operator" || !["[[", "(("].includes(opener.value)) {
    return false;
  }
  const expectedClose = opener.value === "[[" ? "]]" : "))";
  // reject one incomplete owned expression
  if (tokens[end - 1]?.kind !== "operator" || tokens[end - 1].value !== expectedClose) {
    return false;
  }
  const expression = shellCreateNode(state, {
    attributes: { identifier: opener.value === "[[" ? "shell-test" : "shell-arithmetic" },
    attach: false,
    end: tokens[end - 1].end,
    factKind: "operator",
    start: opener.start,
  });
  let valueIndex = 0;
  // retain every positioned test operand
  for (let index = start; index < end; index += 1) {
    const token = tokens[index];
    // retain words only
    if (token.kind !== "word") {
      continue;
    }
    const value = shellWordNode(state, token, state.environment);
    shellAddChild(expression, value, "value", valueIndex);
    valueIndex += 1;
  }
  // attach one explicit compound parent
  if (parent !== null) {
    shellAddChild(parent, expression, "test");
  } else {
    shellAttachNode(state, expression);
  }
  return true;
}

// process one isolated subshell segment
function shellProcessSubshellSegment(state, tokens, context, parent) {
  // reject one ordinary segment
  if (
    tokens[0]?.kind !== "operator" ||
    tokens[0].value !== "(" ||
    tokens.at(-1)?.kind !== "operator" ||
    tokens.at(-1).value !== ")"
  ) {
    return false;
  }
  const node = shellCreateNode(state, {
    attributes: { identifier: "subshell" },
    attach: parent === null,
    end: tokens.at(-1).end,
    factKind: "control",
    start: tokens[0].start,
  });
  // attach one explicit subshell parent
  if (parent !== null) {
    shellAddChild(parent, node, "body");
  }
  (0, shellAnalyzeIsolatedSegments)(state, tokens.slice(1, -1), context, node);
  return true;
}

// process one owned brace group
function shellProcessBraceSegment(state, tokens, context, parent) {
  // reject one ordinary segment
  if (
    tokens[0]?.kind !== "operator" ||
    tokens[0].value !== "{" ||
    tokens.at(-1)?.kind !== "operator" ||
    tokens.at(-1).value !== "}"
  ) {
    return false;
  }
  (0, shellProcessSegments)(state, tokens.slice(1, -1), context, parent);
  return true;
}

// process one typed command segment
function shellProcessOwnedSegment(state, tokens, context, parent) {
  let commandTokens = tokens;
  // consume one command-list negation prefix
  if (tokens[0]?.kind === "word" && shellOptionalLiteralWord(tokens[0]) === "!") {
    commandTokens = tokens.slice(1);
    // reject one missing negated command
    if (commandTokens.length === 0) {
      shellPushDiagnostic(state);
      return;
    }
  }
  // retain one typed compound or simple command
  if (
    !(0, shellProcessTestSegment)(state, commandTokens, parent) &&
    !(0, shellProcessSubshellSegment)(state, commandTokens, context, parent) &&
    !(0, shellProcessBraceSegment)(state, commandTokens, context, parent)
  ) {
    (0, shellProcessSimple)(state, commandTokens, context, parent);
  }
}

// process one ordinary logical command list
function shellProcessSegments(state, tokens, context = "simple", explicitParent = null) {
  const split = shellSplitSegments(tokens);
  // reject one malformed typed command list
  if (!split.ok) {
    // invalidate every malformed array target transactionally
    for (let index = 0; index + 1 < tokens.length; index += 1) {
      const assignment = tokens[index]?.kind === "word"
        ? shellAssignment(tokens[index])
        : null;
      // retain one apparent array assignment
      if (
        assignment !== null &&
        tokens[index + 1]?.kind === "operator" &&
        tokens[index + 1].value === "("
      ) {
        shellInvalidateBinding(state, assignment.name);
      }
    }
    shellPushDiagnostic(state);
    return;
  }
  const segments = split.segments;
  const groups = shellSegmentGroups(segments);
  // run one unconditional command without any environment fork or join
  if (groups.length === 1 && groups[0].segments.length === 1) {
    state.segmentContinues = true;
    (0, shellProcessOwnedSegment)(
      state,
      groups[0].segments[0].tokens,
      context,
      explicitParent,
    );
    // discard bindings after one noncontinuing unconditional command
    if (!state.segmentContinues) {
      state.environment = shellCreateEnvironment(state.environment.registry, "variable");
    }
    return;
  }
  let parent = explicitParent;
  // create one actual compound command parent
  if (segments.length > 1 && explicitParent === null) {
    parent = shellCreateNode(state, {
      attributes: { identifier: "command-list" },
      attach: true,
      end: tokens.at(-1).end,
      factKind: "control",
      start: tokens[0].start,
    });
  }
  const entry = state.environment;
  const functionEntry = state.functionEnvironment;
  const initial = shellCreateEnvironmentPath(entry, functionEntry);
  // retain changes since the last sequence boundary
  let alternativeChanged = new Set();
  let alternativeFunctionChanged = new Set();
  // retain finite success and failure alternatives
  let success = null;
  let failure = null;
  // analyze each command group once with a finite status lattice
  for (let index = 0; index < groups.length; index += 1) {
    const operator = index === 0 ? null : groups[index - 1].operator;
    let input;
    let forkInput = false;
    // seed one first command from the fixed logical-list entry
    if (index === 0) {
      input = initial;
    } else if (operator === "&&") {
      // continue only the success alternative
      input = success;
      forkInput = input !== null && input === failure;
    } else if (operator === "||") {
      // continue only the failure alternative
      input = failure;
      forkInput = input !== null && input === success;
    } else {
      // commit one completed sequence before continuing
      const joined = shellJoinEnvironmentAlternatives(
        [success, failure],
        alternativeChanged,
        alternativeFunctionChanged,
      );
      const committed = shellCommitEnvironmentPath(entry, joined);
      const committedFunction = joined === null
        ? null
        : shellCloneEnvironment(joined.functionEnvironment);
      input = committed === null
        ? null
        : shellCreateEnvironmentPath(committed, committedFunction);
      alternativeChanged = new Set();
      alternativeFunctionChanged = new Set();
    }
    const reachable = input !== null;
    const analysisPath = input ?? shellCreateEnvironmentPath(entry, functionEntry);
    const output = (0, shellProcessSegmentGroup)(
      state,
      groups[index],
      analysisPath,
      context,
      parent,
      forkInput,
    );
    const rightSuccess = reachable && output !== null ? output.path : null;
    const rightFailure = rightSuccess;
    // retain only reachable segment changes
    if (rightSuccess !== null) {
      for (const name of output.changed) {
        alternativeChanged.add(name);
      }
      // retain only reachable function changes
      for (const name of output.functionChanged) {
        alternativeFunctionChanged.add(name);
      }
    }
    // seed one first command status pair
    if (index === 0) {
      success = rightSuccess;
      failure = rightFailure;
      continue;
    }
    // join one conditional-and continuation
    if (operator === "&&") {
      // merge only the right command delta into accumulated failures
      if (rightFailure !== null) {
        failure = shellJoinEnvironmentAlternatives(
          [failure, rightFailure],
          output.changed,
          output.functionChanged,
        );
      }
      success = rightSuccess;
      continue;
    }
    // join one conditional-or continuation
    if (operator === "||") {
      // merge only the right command delta into accumulated successes
      if (rightSuccess !== null) {
        success = shellJoinEnvironmentAlternatives(
          [success, rightSuccess],
          output.changed,
          output.functionChanged,
        );
      }
      failure = rightFailure;
      continue;
    }
    success = rightSuccess;
    failure = rightFailure;
  }
  const joined = shellJoinEnvironmentAlternatives(
    [success, failure],
    alternativeChanged,
    alternativeFunctionChanged,
  );
  state.environment = shellCommitEnvironmentPath(entry, joined) ??
    shellCreateEnvironment(state.environment.registry, "variable");
  state.functionEnvironment = joined === null
    ? shellCreateEnvironment(state.functionEnvironment.registry, "function")
    : shellCloneEnvironment(joined.functionEnvironment);
}

// group pipeline-connected simple commands
function shellSegmentGroups(segments) {
  const groups = [];
  let current = [];
  // retain each maximal pipeline group
  for (const segment of segments) {
    current.push(segment);
    // continue one pipeline
    if (segment.operator === "|" || segment.operator === "|&") {
      continue;
    }
    groups.push({ operator: segment.operator, segments: current });
    current = [];
  }
  // retain one malformed trailing pipeline conservatively
  if (current.length > 0) {
    groups.push({ operator: null, segments: current });
  }
  return groups;
}

// execute one simple segment in an owned environment
function shellExecuteSegment(
  state,
  segment,
  environment,
  functionEnvironment,
  context,
  parent,
  trackAssignments,
  logicalChanged,
  logicalFunctionChanged,
) {
  state.environment = environment;
  state.functionEnvironment = functionEnvironment;
  state.segmentContinues = true;
  const savedChanged = state.logicalChanged;
  const savedFunctionChanged = state.logicalFunctionChanged;
  const savedTracking = state.trackAssignments;
  state.logicalChanged = logicalChanged;
  state.logicalFunctionChanged = logicalFunctionChanged;
  state.trackAssignments = trackAssignments;
  (0, shellProcessOwnedSegment)(state, segment.tokens, context, parent);
  state.logicalChanged = savedChanged;
  state.logicalFunctionChanged = savedFunctionChanged;
  state.trackAssignments = savedTracking;
  return state.segmentContinues;
}

// process one simple segment under one bounded status journal
function shellProcessDetachedSegment(
  state,
  segment,
  path,
  context,
  parent,
  forkPath,
) {
  const output = forkPath ? shellForkEnvironmentPath(path) : path;
  const changed = new Set();
  const functionChanged = new Set();
  const continues = (0, shellExecuteSegment)(
    state,
    segment,
    output.environment,
    output.functionEnvironment,
    context,
    parent,
    true,
    changed,
    functionChanged,
  );
  // retain only this segment's changed keys
  for (const name of changed) {
    output.changed.add(name);
  }
  // retain only this segment's changed function keys
  for (const name of functionChanged) {
    output.functionChanged.add(name);
  }
  return continues ? { changed, functionChanged, path: output } : null;
}

// process one simple command or isolated pipeline
function shellProcessSegmentGroup(
  state,
  group,
  path,
  context,
  parent,
  forkPath,
) {
  // process one ordinary nonpipeline segment
  if (group.segments.length === 1) {
    return (0, shellProcessDetachedSegment)(
      state,
      group.segments[0],
      path,
      context,
      parent,
      forkPath,
    );
  }
  // analyze each pipeline component in one disposable local journal
  for (const segment of group.segments) {
    (0, shellExecuteSegment)(
      state,
      segment,
      shellCloneEnvironment(path.environment),
      shellCloneEnvironment(path.functionEnvironment),
      context,
      parent,
      false,
      null,
      null,
    );
  }
  return { changed: new Set(), functionChanged: new Set(), path };
}

// finish one active case arm
function shellFinishCaseArm(state, context) {
  // stop without one active arm
  if (context.currentArm === null) {
    return;
  }
  context.branches.push({
    continues: context.currentArm.continues,
    environment: state.environment,
    functionEnvironment: state.functionEnvironment,
  });
  context.currentArm = null;
}

// open one finite case arm
function shellOpenCaseArm(state, context, tokens, closeIndex) {
  shellFinishCaseArm(state, context);
  const patternTokens = tokens.slice(0, closeIndex);
  const armEnd = tokens.at(-1)?.end ?? patternTokens.at(-1)?.end;
  const arm = shellCreateNode(state, {
    attributes: { identifier: "case-arm" },
    attach: false,
    end: armEnd,
    factKind: "control",
    start: patternTokens[0]?.start ?? tokens[0].start,
  });
  shellAddChild(context.node, arm, "body");
  let exhaustive = false;
  let patternIndex = 0;
  // retain each actual arm pattern
  for (const token of patternTokens) {
    // skip pattern separators
    if (token.kind !== "word") {
      continue;
    }
    const pattern = shellLiteralWord(token);
    // retain one fixed pattern fact
    if (pattern !== null) {
      const patternNode = shellCreateNode(state, {
        attributes: {
          literalKind: "string",
          literalRole: "semantic-constant",
          literalValue: pattern,
        },
        attach: false,
        end: token.end,
        factKind: "constant",
        start: token.start,
      });
      shellAddChild(arm, patternNode, "test", patternIndex);
      patternIndex += 1;
      exhaustive ||= pattern === "*";
    }
  }
  context.exhaustive ||= exhaustive;
  context.currentArm = {
    continues: true,
    node: arm,
  };
  state.environment = shellCloneEnvironment(context.entry, true);
  state.functionEnvironment = shellCloneEnvironment(context.functionEntry, true);
}

// close one finite case control
function shellCloseCase(state, context, end) {
  shellFinishCaseArm(state, context);
  context.node.end = Math.max(context.node.end, end);
  state.environment = shellJoinEnvironments(
    state,
    context.entry,
    context.branches.map(
      // project one variable continuation
      (branch) => branch.continues ? branch.environment : null,
    ),
    !context.exhaustive,
    context.variablePrefix,
  );
  state.functionEnvironment = shellJoinEnvironments(
    state,
    context.functionEntry,
    context.branches.map(
      // project one function continuation
      (branch) => branch.continues ? branch.functionEnvironment : null,
    ),
    !context.exhaustive,
    context.functionPrefix,
  );
  state.contexts.pop();
  state.currentCase = context.priorCase;
}

// process one case-body logical line
function shellProcessCaseLine(state, context, tokens, emissionContext) {
  const first = tokens.find(
    // locate one word token
    (token) => token.kind === "word",
  );
  const firstValue = first === undefined ? null : shellOptionalLiteralWord(first);
  // close one case statement
  if (firstValue === "esac") {
    shellCloseCase(state, context, tokens.at(-1).end);
    return true;
  }
  let closeIndex = -1;
  // locate one arm-pattern close
  for (let index = 0; index < tokens.length; index += 1) {
    // retain the first close before an active arm body
    if (tokens[index].kind === "operator" && tokens[index].value === ")") {
      closeIndex = index;
      break;
    }
  }
  // open one new arm
  if (closeIndex !== -1 && context.currentArm === null) {
    (0, shellOpenCaseArm)(state, context, tokens, closeIndex);
    tokens = tokens.slice(closeIndex + 1);
  }
  const armEnd = tokens.findIndex(
    // locate one arm terminator
    (token) => token.kind === "operator" && [";;", ";&", ";;&"].includes(token.value),
  );
  const body = armEnd === -1 ? tokens : tokens.slice(0, armEnd);
  // analyze one arm body
  if (body.length > 0) {
    (0, shellProcessSegments)(state, body, emissionContext);
  }
  // finish one explicit arm
  if (armEnd !== -1) {
    // reject fallthrough arms
    if (tokens[armEnd].value !== ";;") {
      shellPushDiagnostic(state);
      context.exhaustive = false;
    }
    shellFinishCaseArm(state, context);
  }
  return true;
}

// collect one function declaration signature
function shellFunctionSignature(tokens) {
  const words = tokens.filter(
    // retain words only
    (token) => token.kind === "word",
  );
  const operators = tokens.filter(
    // retain operators only
    (token) => token.kind === "operator",
  ).map(
    // project operator spelling
    (token) => token.value,
  );
  // recognize name-parentheses-brace form
  if (
    words.length === 1 &&
    operators.length === 3 &&
    operators[0] === "(" &&
    operators[1] === ")" &&
    operators[2] === "{"
  ) {
    const name = shellLiteralWord(words[0]);
    return name !== null && /^[A-Za-z_][A-Za-z0-9_]*$/u.test(name) ? name : null;
  }
  // recognize function-keyword form
  if (words.length === 2 && shellLiteralWord(words[0]) === "function" && operators.includes("{")) {
    const name = shellLiteralWord(words[1]);
    return name !== null && /^[A-Za-z_][A-Za-z0-9_]*$/u.test(name) ? name : null;
  }
  return null;
}

// build one typed delimiter index
function shellDelimiterIndex(tokens) {
  const matches = new Map();
  const stack = [];
  const closers = new Map([
    ["(", ")"],
    ["((", "))"],
    ["[[", "]]"],
    ["{", "}"],
  ]);
  let valid = true;
  // scan every token exactly once
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    const active = stack.at(-1);
    // retain opaque expression internals until one owned close
    if (active?.opaque) {
      // close one exact opaque expression
      if (shellOwnedExpressionClose(tokens, index, active.close)) {
        stack.pop();
        matches.set(active.index, index);
      }
      continue;
    }
    // retain one typed opener
    if (token.kind === "operator" && closers.has(token.value)) {
      stack.push({
        close: closers.get(token.value),
        index,
        opaque: token.value === "[[" || token.value === "((",
      });
      continue;
    }
    // retain one typed closer
    if (token.kind === "operator" && [")", "))", "]]", "}"].includes(token.value)) {
      const opener = stack.pop();
      // reject one mismatched delimiter
      if (opener?.close !== token.value) {
        // preserve one still-open typed owner
        if (opener !== undefined) {
          stack.push(opener);
        }
        valid = false;
        continue;
      }
      matches.set(opener.index, index);
    }
  }
  // reject one unterminated delimiter
  if (stack.length > 0) {
    valid = false;
  }
  return { matches, valid };
}

// locate one inline function declaration
function shellInlineFunctionAt(tokens, delimiters, index) {
  const first = tokens[index];
  // stop without one literal declaration start
  if (first?.kind !== "word") {
    return null;
  }
  const firstValue = shellOptionalLiteralWord(first);
  let nameToken = first;
  let openIndex = index + 3;
  // parse one function-keyword spelling
  if (firstValue === "function") {
    nameToken = tokens[index + 1];
    // reject one missing literal function name
    if (nameToken?.kind !== "word") {
      return null;
    }
    openIndex = tokens[index + 2]?.value === "(" && tokens[index + 3]?.value === ")"
      ? index + 4
      : index + 2;
  } else if (
    tokens[index + 1]?.value !== "(" ||
    tokens[index + 2]?.value !== ")"
  ) {
    return null;
  }
  const name = shellOptionalLiteralWord(nameToken);
  // require one exact identifier and brace body
  if (
    name === null ||
    !/^[A-Za-z_][A-Za-z0-9_]*$/u.test(name) ||
    tokens[openIndex]?.kind !== "operator" ||
    tokens[openIndex].value !== "{"
  ) {
    return null;
  }
  const closeIndex = delimiters.matches.get(openIndex);
  // require one complete same-line body
  if (closeIndex === undefined) {
    return null;
  }
  return {
    bodyEnd: closeIndex,
    bodyStart: openIndex + 1,
    end: closeIndex + 1,
    name,
    start: index,
  };
}

// build one immutable token-range ownership index
function shellCreateRangeIndex(grammarTokens) {
  const delimiters = shellDelimiterIndex(grammarTokens);
  const pairClose = new Int32Array(grammarTokens.length);
  const pairOpen = new Int32Array(grammarTokens.length);
  pairClose.fill(-1);
  pairOpen.fill(-1);
  const executablePairs = new Set();
  const processOpeners = new Set();
  const processByDirection = new Map();
  const processSubstitutions = [];
  // retain every typed pair endpoint once
  for (const [open, close] of delimiters.matches) {
    pairClose[open] = close;
    pairOpen[close] = open;
  }
  const invalidDelimiter = new Uint8Array(grammarTokens.length);
  const delimiterStack = [];
  const delimiterClosers = new Map([
    ["(", ")"],
    ["((", "))"],
    ["[[", "]]"],
    ["{", "}"],
  ]);
  // classify every malformed delimiter endpoint once
  for (let index = 0; index < grammarTokens.length; index += 1) {
    const token = grammarTokens[index];
    const active = delimiterStack.at(-1);
    // retain opaque expression internals
    if (active?.opaque) {
      // close one exact opaque expression
      if (shellOwnedExpressionClose(grammarTokens, index, active.close)) {
        delimiterStack.pop();
      }
      continue;
    }
    // retain one typed opener
    if (token.kind === "operator" && delimiterClosers.has(token.value)) {
      delimiterStack.push({
        close: delimiterClosers.get(token.value),
        index,
        opaque: token.value === "[[" || token.value === "((",
      });
      continue;
    }
    // classify one typed closer
    if (token.kind === "operator" && [")", "))", "]]", "}"].includes(token.value)) {
      const opener = delimiterStack.pop();
      // retain one mismatched closer without losing its open owner
      if (opener?.close !== token.value) {
        invalidDelimiter[index] = 1;
        // preserve one still-open typed owner
        if (opener !== undefined) {
          delimiterStack.push(opener);
        }
      }
    }
  }
  // retain every unclosed opener
  while (delimiterStack.length > 0) {
    invalidDelimiter[delimiterStack.pop().index] = 1;
  }
  const invalidDelimiterPrefix = new Uint32Array(grammarTokens.length + 1);
  // prefix every malformed endpoint for constant-time range checks
  for (let index = 0; index < grammarTokens.length; index += 1) {
    invalidDelimiterPrefix[index + 1] = invalidDelimiterPrefix[index] +
      invalidDelimiter[index];
  }
  // classify every process-substitution opener once
  for (let index = 0; index + 1 < grammarTokens.length; index += 1) {
    const direction = grammarTokens[index];
    const open = grammarTokens[index + 1];
    // retain one adjacent process substitution
    if (
      direction?.kind === "operator" && ["<", ">"].includes(direction.value) &&
      open?.kind === "operator" && open.value === "(" &&
      direction.end === open.start && pairClose[index + 1] >= 0
    ) {
      const close = pairClose[index + 1];
      processOpeners.add(index + 1);
      const process = {
        close,
        direction: direction.value,
        directionIndex: index,
        end: close + 1,
        open: index + 1,
        start: index + 2,
      };
      processSubstitutions.push(process);
      processByDirection.set(index, process);
    }
  }
  // classify executable structural pairs once
  for (const [open, close] of delimiters.matches) {
    const token = grammarTokens[open];
    // skip opaque or process-owned pairs
    if (token?.kind !== "operator" || !["(", "{"].includes(token.value) ||
      processOpeners.has(open)) {
      continue;
    }
    const prior = grammarTokens[open - 1];
    const arrayOwned = token.value === "(" && prior?.kind === "word" &&
      shellAssignment(prior) !== null;
    // retain only executable command groups
    if (!arrayOwned && close > open) {
      executablePairs.add(open);
    }
  }
  const declarations = [];
  const declarationChildren = new Map();
  const rootDeclarations = [];
  const work = [{ end: grammarTokens.length, parent: -1, start: 0 }];
  // discover every declaration without rescanning descendants
  while (work.length > 0) {
    const range = work.pop();
    let cursor = range.start;
    // scan one direct-owned declaration range
    while (cursor < range.end) {
      // skip one separator
      if (grammarTokens[cursor]?.kind === "operator" &&
        grammarTokens[cursor].value === ";") {
        cursor += 1;
        continue;
      }
      const declaration = shellInlineFunctionAt(grammarTokens, delimiters, cursor);
      // retain one exact declaration
      if (declaration !== null && declaration.end <= range.end) {
        const id = declarations.length;
        const retained = { ...declaration, id, parent: range.parent };
        declarations.push(retained);
        // retain one direct declaration child
        if (range.parent < 0) {
          rootDeclarations.push(retained);
        } else {
          const children = declarationChildren.get(range.parent) ?? [];
          children.push(retained);
          declarationChildren.set(range.parent, children);
        }
        work.push({
          end: declaration.bodyEnd,
          parent: id,
          start: declaration.bodyStart,
        });
        cursor = declaration.end;
        continue;
      }
      const ownedClose = pairClose[cursor];
      // skip one nondeclaration child range
      if (ownedClose >= cursor && ownedClose < range.end) {
        cursor = ownedClose + 1;
        continue;
      }
      cursor += 1;
    }
  }
  return {
    declarationChildren,
    declarations,
    delimiters,
    executablePairs,
    invalidDelimiterPrefix,
    pairClose,
    pairOpen,
    processOpeners,
    processByDirection,
    processSubstitutions,
    requiresFrames: executablePairs.size > 0 || processSubstitutions.length > 0,
    rootDeclarations,
  };
}

// update one bounded physical signature
function shellAdvancePhysicalSignature(signature, token) {
  // retain one direct word reference
  if (token.kind === "word") {
    signature.wordCount += 1;
    // retain only the first two word references
    if (signature.words.length < 2) {
      signature.words.push(token);
    }
    return;
  }
  // retain one direct operator spelling
  if (token.kind === "operator") {
    signature.operatorCount += 1;
    // retain only the first three operators
    if (signature.operators.length < 3) {
      signature.operators.push(token.value);
    }
    // retain one brace presence bit
    if (token.value === "{") {
      signature.hasBrace = true;
    }
  }
}

// read one completed physical function signature
function shellPhysicalSignatureName(signature) {
  // recognize one name-parentheses-brace form
  if (
    signature.wordCount === 1 && signature.operatorCount === 3 &&
    signature.operators[0] === "(" && signature.operators[1] === ")" &&
    signature.operators[2] === "{"
  ) {
    const name = shellLiteralWord(signature.words[0]);
    return name !== null && /^[A-Za-z_][A-Za-z0-9_]*$/u.test(name) ? name : null;
  }
  // recognize one function-keyword form
  if (
    signature.wordCount === 2 && shellLiteralWord(signature.words[0]) === "function" &&
    signature.hasBrace
  ) {
    const name = shellLiteralWord(signature.words[1]);
    return name !== null && /^[A-Za-z_][A-Za-z0-9_]*$/u.test(name) ? name : null;
  }
  return null;
}

// plan one logical line once for inventory and execution
function shellPlanLine(tokens) {
  const grammarTokens = tokens.filter(
    // omit heredoc bodies from shell grammar
    (token) => token.kind !== "heredoc",
  );
  const rangeIndex = shellCreateRangeIndex(grammarTokens);
  return {
    declarations: rangeIndex.rootDeclarations,
    delimiters: rangeIndex.delimiters,
    grammarTokens,
    rangeIndex,
    tokens,
  };
}

// trim only declaration-adjacent empty separators
function shellInlineStatementSlice(tokens, start, end) {
  let first = start;
  let last = end;
  // remove one leading declaration boundary
  if (tokens[first]?.kind === "operator" && tokens[first].value === ";") {
    first += 1;
  }
  // remove one trailing declaration boundary
  if (tokens[last - 1]?.kind === "operator" && tokens[last - 1].value === ";") {
    last -= 1;
  }
  return tokens.slice(first, last);
}

// distinguish one structural expression close from adjacent regex text
function shellOwnedExpressionClose(tokens, index, expected) {
  const token = tokens[index];
  // reject one different token
  if (token?.kind !== "operator" || token.value !== expected) {
    return false;
  }
  const next = tokens[index + 1];
  // admit one terminal or whitespace-separated close
  if (next === undefined || next.kind === "newline" || token.end < next.start) {
    return true;
  }
  return next.kind === "operator" &&
    [";", "&&", "||", "|", "|&", ")", "}"].includes(next.value);
}

// classify direct arithmetic parent-variable writes
function shellArithmeticMutates(tokens) {
  let expression = "";
  // inspect every arithmetic token once
  for (const token of tokens) {
    // skip structural arithmetic delimiters
    if (token.kind === "operator") {
      continue;
    }
    // preserve token boundaries without interpreting expansions
    if (token.kind !== "word") {
      expression += " 0 ";
      continue;
    }
    // inspect every direct word part once
    for (const part of token.parts) {
      // retain literal arithmetic syntax
      if (part.kind === "text") {
        expression += part.value;
      } else if (part.kind === "arithmetic") {
        // retain one already finalized nested mutation
        if (part.mutatesParent === true) {
          return true;
        }
        expression += " (0) ";
      } else {
        // isolate nonparent substitution effects
        expression += " 0 ";
      }
    }
    expression += " ";
  }
  return /\+\+|--|(?:^|[^=!<>])(?:<<=|>>=|\*\*=|[+*/%&|^-]?=)(?!=)/u.test(
    expression,
  );
}

// count one arithmetic plan's direct grouping balance
function shellArithmeticGroupBalance(tokens) {
  let balance = 0;
  // inspect every direct arithmetic token once
  for (const token of tokens) {
    // skip one nonword token
    if (token.kind !== "word") {
      continue;
    }
    // inspect every direct word part once
    for (const part of token.parts) {
      // include one finalized nested arithmetic balance
      if (part.kind === "arithmetic") {
        balance += part.groupBalance ?? 0;
        continue;
      }
      // ignore opaque command substitutions and parameters
      if (part.kind !== "text") {
        continue;
      }
      // count one direct literal grouping stream
      for (const character of part.value) {
        // enter one grouping expression
        if (character === "(") {
          balance += 1;
        } else if (character === ")") {
          // leave one grouping expression
          balance -= 1;
        }
      }
    }
  }
  return balance;
}

// detect one lexer-truncated arithmetic continuation
function shellStartsArithmeticContinuation(tokens) {
  // inspect every word part once
  for (const token of tokens) {
    // skip one nonword token
    if (token.kind !== "word") {
      continue;
    }
    // inspect every arithmetic body once
    for (const part of token.parts) {
      // skip one nonarithmetic part
      if (part.kind !== "arithmetic") {
        continue;
      }
      // retain one continuation with unmatched grouping opens
      if ((part.groupBalance ?? 0) > 0) {
        return true;
      }
    }
  }
  return false;
}

// finish one physical shell plan with private ownership
function shellFinishPhysicalPlan(tokens, arithmeticContinuation, arithmeticStart = false) {
  const plan = shellPlanLine(tokens);
  plan.arithmeticContinuation = arithmeticContinuation;
  plan.arithmeticStart = arithmeticStart;
  return plan;
}

// plan physical lines under owned multiline expressions
function shellPlanLines(tokens) {
  const plans = [];
  let line = [];
  let lastGrammarToken = null;
  let arithmeticContinuation = false;
  let signature = { hasBrace: false, operatorCount: 0, operators: [], wordCount: 0, words: [] };
  const stack = [];
  // consume every lexer token once
  for (let tokenIndex = 0; tokenIndex < tokens.length; tokenIndex += 1) {
    const token = tokens[tokenIndex];
    // update one retained grammar signature
    if (token.kind !== "newline" && token.kind !== "heredoc") {
      shellAdvancePhysicalSignature(signature, token);
    }
    const active = stack.at(-1);
    // close one opaque expression only at its exact terminator
    if (
      active?.opaque &&
      shellOwnedExpressionClose(tokens, tokenIndex, active.close)
    ) {
      stack.pop();
    } else if (!active?.opaque && token.kind === "operator") {
      // open one test or arithmetic expression
      if (["[[", "(("].includes(token.value)) {
        stack.push({ close: token.value === "[[" ? "]]" : "))", opaque: true });
      } else if (token.value === "(") {
        // open one structural group or array
        stack.push({ close: ")", opaque: false });
      } else if (token.value === "{") {
        const functionHeader = shellPhysicalSignatureName(signature);
        // open only command braces, not multiline function bodies
        if (functionHeader === null) {
          stack.push({ close: "}", opaque: false });
        }
      } else if (token.value === active?.close) {
        // close one structural group
        stack.pop();
      }
    }
    const trailingRelation = lastGrammarToken?.kind === "operator" &&
      ["&&", "||", "|", "|&"].includes(lastGrammarToken.value);
    // finish one complete physical command line
    if (token.kind === "newline" && stack.length === 0 && !trailingRelation) {
      const startsContinuation = shellStartsArithmeticContinuation(line);
      plans.push(shellFinishPhysicalPlan(
        line,
        arithmeticContinuation || startsContinuation,
        startsContinuation,
      ));
      // retain ownership until the lexer-visible arithmetic close
      if (startsContinuation) {
        arithmeticContinuation = true;
      }
      // close one owned continuation after its terminal marker
      if (
        arithmeticContinuation &&
        line.some(
          // locate one terminal arithmetic token
          (entry) => entry.kind === "operator" && entry.value === "))",
        )
      ) {
        arithmeticContinuation = false;
      }
      line = [];
      signature = { hasBrace: false, operatorCount: 0, operators: [], wordCount: 0, words: [] };
      lastGrammarToken = null;
      continue;
    }
    // retain one command boundary inside structural groups
    if (
      token.kind === "newline" &&
      stack.length > 0 &&
      !stack.at(-1).opaque &&
      !trailingRelation
    ) {
      line.push({
        end: token.end,
        kind: "operator",
        start: token.start,
        value: ";",
      });
      lastGrammarToken = line.at(-1);
      continue;
    }
    // omit one owned internal newline
    if (token.kind !== "newline") {
      line.push(token);
      // retain only grammar tokens for continuation ownership
      if (token.kind !== "heredoc") {
        lastGrammarToken = token;
      }
    }
  }
  // retain one final unterminated plan for diagnostics
  if (line.length > 0) {
    plans.push(shellFinishPhysicalPlan(line, arithmeticContinuation));
  }
  return plans;
}

// create one fixed-lattice effect graph node
function shellCreateEffectNode() {
  return {
    bits: 0,
    dependents: new Set(),
    functionCause: SHELL_AUTHORITY_NONE,
  };
}

// normalize one function binding effect mask
function shellNormalizeEffectBits(bits) {
  let normalized = bits;
  // local erasure can expose one caller variable
  if ((normalized & SHELL_EFFECT_MAY_ERASE_LOCAL) !== 0) {
    normalized |= SHELL_EFFECT_VAR_WRITE;
  }
  // readonly-variable risk implies one possible variable write
  if ((normalized & SHELL_EFFECT_MAY_RO_VAR) !== 0) {
    normalized |= SHELL_EFFECT_VAR_WRITE;
  }
  // readonly-function risk implies one possible function write
  if ((normalized & SHELL_EFFECT_MAY_RO_FN) !== 0) {
    normalized |= SHELL_EFFECT_FN_WRITE;
  }
  return normalized;
}

// read or create one literal callee-name node
function shellEffectNameNode(model, name) {
  const existing = model.nameNodes.get(name);
  // reuse one name universe node
  if (existing !== undefined) {
    return existing;
  }
  const node = shellCreateEffectNode();
  model.nameNodes.set(name, node);
  model.nodes.push(node);
  return node;
}

// register one owned function body definition
function shellRegisterEffectDefinition(model, name, start, unconditional) {
  const node = shellCreateEffectNode();
  const definition = {
    chunks: [],
    localNames: new Set(),
    localProofEpoch: 0,
    localProofEpochs: new Map(),
    name,
    node,
    start,
  };
  model.definitions.push(definition);
  model.nodes.push(node);
  const definitions = model.definitionsByName.get(name) ?? [];
  definitions.push(definition);
  model.definitionsByName.set(name, definitions);
  // retain one all-path module definition proof
  if (unconditional) {
    model.unconditionalNames.add(name);
  }
  const nameNode = shellEffectNameNode(model, name);
  node.dependents.add(nameNode);
  return definition;
}

// read one current all-path local proof
function shellFunctionLocalProven(definition, name) {
  return definition.localProofEpochs.get(name) === definition.localProofEpoch;
}

// expose one constant-time local proof view
function shellFunctionLocalView(definition) {
  return {
    // test one current local proof
    has(name) {
      return shellFunctionLocalProven(definition, name);
    },
  };
}

// record one exact all-path local declaration
function shellFunctionRecordLocal(definition, name) {
  definition.localNames.add(name);
  definition.localProofEpochs.set(name, definition.localProofEpoch);
}

// invalidate every dynamic local proof in constant time
function shellFunctionInvalidateLocals(definition) {
  definition.localProofEpoch += 1;
}

// append one direct body token chunk
function shellAppendEffectChunk(definition, tokens, depth) {
  // skip one empty owned chunk
  if (tokens.length === 0) {
    return;
  }
  definition.chunks.push({ depth, tokens });
}

// collect inline definitions without assigning nested bodies to parents
function shellCollectInlineEffectDefinitions(
  model,
  tokens,
  declarations,
  parent,
  depth,
  unconditional,
  rangeIndex = null,
) {
  const stack = [{
    cursor: 0,
    declarations,
    depth,
    index: 0,
    parent,
    rangeEnd: tokens.length,
    rangeIndex,
    rangeStart: 0,
    unconditional,
  }];
  // materialize one direct-owned chunk
  function appendRange(definition, start, end, chunkDepth) {
    // skip one empty range
    if (start >= end) {
      return;
    }
    const owned = [];
    // copy each direct-owned token once
    for (let index = start; index < end; index += 1) {
      owned.push(tokens[index]);
    }
    shellAppendEffectChunk(definition, owned, chunkDepth);
  }
  // consume every nested declaration frame iteratively
  while (stack.length > 0) {
    const frame = stack.at(-1);
    // finish one declaration range
    if (frame.index >= frame.declarations.length) {
      // retain one parent-owned suffix
      if (frame.parent !== null) {
        let suffixStart = frame.cursor;
        let suffixEnd = frame.rangeEnd;
        // trim one leading separator
        if (tokens[suffixStart]?.kind === "operator" && tokens[suffixStart].value === ";") {
          suffixStart += 1;
        }
        // trim one trailing separator
        if (tokens[suffixEnd - 1]?.kind === "operator" &&
          tokens[suffixEnd - 1].value === ";") {
          suffixEnd -= 1;
        }
        appendRange(frame.parent, suffixStart, suffixEnd, frame.depth);
      }
      stack.pop();
      continue;
    }
    const declaration = frame.declarations[frame.index];
    frame.index += 1;
    // retain one parent-owned prefix
    if (frame.parent !== null) {
      let prefixStart = frame.cursor;
      let prefixEnd = declaration.start;
      // trim one leading separator
      if (tokens[prefixStart]?.kind === "operator" && tokens[prefixStart].value === ";") {
        prefixStart += 1;
      }
      // trim one trailing separator
      if (tokens[prefixEnd - 1]?.kind === "operator" &&
        tokens[prefixEnd - 1].value === ";") {
        prefixEnd -= 1;
      }
      appendRange(frame.parent, prefixStart, prefixEnd, frame.depth);
      frame.parent.node.bits |= SHELL_EFFECT_FN_WRITE;
    }
    const definition = shellRegisterEffectDefinition(
      model,
      declaration.name,
      tokens[declaration.start].start,
      frame.unconditional,
    );
    frame.cursor = declaration.end;
    const children = frame.rangeIndex?.declarationChildren.get(declaration.id) ?? [];
    // enter one nested declaration body
    if (children.length > 0) {
      stack.push({
        cursor: declaration.bodyStart,
        declarations: children,
        depth: 0,
        index: 0,
        parent: definition,
        rangeEnd: declaration.bodyEnd,
        rangeIndex: frame.rangeIndex,
        rangeStart: declaration.bodyStart,
        unconditional: false,
      });
      continue;
    }
    appendRange(definition, declaration.bodyStart, declaration.bodyEnd, 0);
  }
}

// identify one control-depth closing marker
function shellEffectControlClose(first) {
  return first === "fi" || first === "done" || first === "esac";
}

// identify one control-depth opening marker
function shellEffectControlOpen(first) {
  return ["if", "case", "for", "while", "until", "select"].includes(first);
}

// collect every owned function body exactly once
function shellCreateFunctionEffectModel() {
  return {
    definitions: [],
    definitionsByName: new Map(),
    nameNodes: new Map(),
    nodes: [],
    unconditionalNames: new Set(),
  };
}

// collect every owned function body exactly once
function shellCollectFunctionEffectBodies(linePlans, model = shellCreateFunctionEffectModel()) {
  const stack = [];
  let moduleDepth = 0;
  // consume every shared logical-line plan once
  for (const plan of linePlans) {
    const tokens = plan.grammarTokens;
    const firstWord = tokens.find(
      // locate one first literal word
      (token) => token.kind === "word",
    );
    const first = firstWord === undefined ? null : shellOptionalLiteralWord(firstWord);
    const owner = stack.at(-1);
    // close one active multiline function body
    if (
      owner !== undefined &&
      first === null &&
      tokens.length === 1 &&
      tokens[0].kind === "operator" &&
      tokens[0].value === "}"
    ) {
      stack.pop();
      continue;
    }
    const multilineName = shellFunctionSignature(tokens);
    // open one multiline definition body
    if (multilineName !== null) {
      // record one nested definition effect on its parent
      if (owner !== undefined) {
        owner.definition.node.bits |= SHELL_EFFECT_FN_WRITE;
      }
      const definition = shellRegisterEffectDefinition(
        model,
        multilineName,
        tokens[0].start,
        owner === undefined && moduleDepth === 0,
      );
      stack.push({ controlDepth: 0, definition });
      continue;
    }
    // close one owned control depth before its terminator
    if (owner !== undefined && shellEffectControlClose(first)) {
      owner.controlDepth = Math.max(0, owner.controlDepth - 1);
    } else if (owner === undefined && shellEffectControlClose(first)) {
      moduleDepth = Math.max(0, moduleDepth - 1);
    }
    const current = stack.at(-1);
    // partition definitions inside one function body
    if (current !== undefined) {
      if (plan.declarations.length > 0) {
        shellCollectInlineEffectDefinitions(
          model,
          tokens,
          plan.declarations,
          current.definition,
          current.controlDepth,
          false,
          plan.rangeIndex,
        );
      } else {
        shellAppendEffectChunk(current.definition, tokens, current.controlDepth);
      }
      // enter one nested control body after its header
      if (shellEffectControlOpen(first)) {
        current.controlDepth += 1;
      }
      continue;
    }
    // collect top-level inline definitions
    if (plan.declarations.length > 0) {
      shellCollectInlineEffectDefinitions(
        model,
        tokens,
        plan.declarations,
        null,
        moduleDepth,
        moduleDepth === 0,
        plan.rangeIndex,
      );
    }
    // enter one module control body after its header
    if (shellEffectControlOpen(first)) {
      moduleDepth += 1;
    }
  }
  return model;
}

// parse one closed export mutation grammar
function shellExportMutations(exactWords) {
  const mutations = [];
  // parse every export operand once
  for (const value of exactWords.slice(1)) {
    // reject one option, dynamic value, or nonidentifier
    if (typeof value !== "string" || value.startsWith("-")) {
      return null;
    }
    const match = /^([A-Za-z_][A-Za-z0-9_]*)(?:=(.*))?$/u.exec(value);
    // reject one unowned export spelling
    if (match === null) {
      return null;
    }
    mutations.push({ assigned: match[2] !== undefined, name: match[1], value: match[2] });
  }
  return mutations.length === 0 ? null : mutations;
}

// classify one authority-proven invocation effect
function shellInvocationEffect(command, exactWords, localNames = null) {
  const locals = localNames ?? new Set();
  // retain one closed state-pure command query
  if (command === "command") {
    return exactWords.length === 3 && exactWords[1] === "-v"
      ? { bits: 0, kind: "noop" }
      : { bits: SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, kind: "all" };
  }
  // retain one exact pending source body without an intrinsic grammar gap
  if (command === "source" || command === ".") {
    const pendingExact = exactWords.length === 2 &&
      typeof exactWords[1] === "string" && exactWords[1].length > 0;
    return {
      bits: SHELL_EFFECT_ALL_BINDINGS | (pendingExact ? 0 : SHELL_EFFECT_OPEN),
      kind: pendingExact ? "pending-source" : "all",
    };
  }
  // reject arbitrary evaluated shell code
  if (command === "eval") {
    return { bits: SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, kind: "all" };
  }
  // retain one variable-writing read
  if (command === "read") {
    const targets = shellReadTargets(exactWords);
    // reject one unclosed read grammar
    if (targets === null) {
      return { bits: SHELL_EFFECT_VAR_WRITE | SHELL_EFFECT_OPEN, kind: "unknown-read" };
    }
    return {
      bits: targets.every((name) => locals.has(name)) ? 0 : SHELL_EFFECT_VAR_WRITE,
      kind: "read",
    };
  }
  // classify one admitted mapfile target
  if (command === "mapfile") {
    const target = shellMapfileTarget(exactWords);
    // reject one unclosed callback or option grammar
    if (target === null) {
      return { bits: SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, kind: "all" };
    }
    return {
      bits: locals.has(target) ? 0 : SHELL_EFFECT_VAR_WRITE,
      kind: "mapfile",
    };
  }
  // fail closed for unadmitted stateful builtin grammars
  if (command === "readarray" || command === "getopts") {
    return { bits: SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, kind: "all" };
  }
  // classify one export mutation
  if (command === "export") {
    const mutations = shellExportMutations(exactWords);
    // reject one unclosed export grammar
    if (mutations === null) {
      return { bits: SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, kind: "all" };
    }
    return {
      bits: mutations.every((mutation) => locals.has(mutation.name))
        ? 0
        : SHELL_EFFECT_VAR_WRITE,
      kind: "export",
    };
  }
  // classify one directory mutation conservatively
  if (command === "cd") {
    const closed = exactWords.length === 2 &&
      typeof exactWords[1] === "string" &&
      exactWords[1].length > 0 &&
      !exactWords[1].startsWith("-");
    return {
      bits: SHELL_EFFECT_VAR_WRITE | SHELL_EFFECT_OPEN,
      kind: closed ? "cd" : "all",
    };
  }
  // classify one trap registration boundary
  if (command === "trap") {
    const reset = exactWords.length >= 3 && exactWords[1] === "-" && exactWords.slice(2).every(
      // require one literal signal name
      (value) => typeof value === "string" && value.length > 0,
    );
    const deferredExit = exactWords.length === 3 &&
      typeof exactWords[1] === "string" && exactWords[2] === "EXIT";
    // retain only closed reset and deferred exit bindings
    if (reset || deferredExit) {
      return { bits: 0, kind: "trap-noop" };
    }
    return { bits: SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, kind: "all" };
  }
  // retain one closed option-only shell-mode mutation
  if (command === "set") {
    const closed = exactWords.slice(1).every(
      // require one literal option atom
      (value) => typeof value === "string" && (value.startsWith("-") || value === "pipefail"),
    );
    return closed
      ? { bits: 0, kind: "noop" }
      : { bits: SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, kind: "all" };
  }
  // retain one closed positional-parameter shift
  if (command === "shift") {
    const closed = exactWords.length <= 2 && (
      exactWords.length === 1 || /^[0-9]+$/u.test(exactWords[1] ?? "")
    );
    return closed
      ? { bits: 0, kind: "noop" }
      : { bits: SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, kind: "all" };
  }
  // classify one printf target or format-name effect
  if (command === "printf") {
    const variableMode = exactWords[1] === "-v";
    const format = exactWords[variableMode ? 3 : 1];
    const formatEffect = typeof format === "string"
      ? shellPrintfFormatEffect(format)
      : "unknown";
    const target = exactWords[2];
    let bits = 0;
    // retain one caller-visible direct target
    if (variableMode && !(typeof target === "string" && locals.has(target))) {
      bits |= SHELL_EFFECT_VAR_WRITE;
    }
    // retain one unknown format-name write
    if (formatEffect !== "safe") {
      bits |= SHELL_EFFECT_VAR_WRITE | SHELL_EFFECT_OPEN;
    }
    return { bits, kind: "printf" };
  }
  // classify one unset namespace selection
  if (command === "unset") {
    // retain one exact function-only unset
    if (exactWords[1] === "-f") {
      return { bits: SHELL_EFFECT_FN_WRITE, kind: "unset" };
    }
    const targets = exactWords[1] === "-v" ? exactWords.slice(2) : exactWords.slice(1);
    const closedTargets = targets.length > 0 && targets.every(
      // require one literal unset target
      (name) => typeof name === "string" && /^[A-Za-z_][A-Za-z0-9_]*$/u.test(name),
    );
    // fail closed for one unproved default namespace selection
    if (!closedTargets || (exactWords[1]?.startsWith("-") && exactWords[1] !== "-v")) {
      return { bits: SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, kind: "all" };
    }
    return {
      bits: SHELL_EFFECT_VAR_WRITE | SHELL_EFFECT_MAY_ERASE_LOCAL,
      kind: "unset",
    };
  }
  // classify one readonly attribute mutation
  if (command === "readonly") {
    const functionMode = exactWords.includes("-f");
    const targets = exactWords.slice(1).filter(
      // retain nonoption readonly targets
      (value) => typeof value === "string" && !value.startsWith("-"),
    );
    // retain exact function-local variable attributes
    if (!functionMode && targets.length > 0 && targets.every((name) => locals.has(name))) {
      return { bits: 0, kind: "readonly" };
    }
    return {
      bits: functionMode
        ? SHELL_EFFECT_FN_WRITE | SHELL_EFFECT_MAY_RO_FN
        : SHELL_EFFECT_VAR_WRITE | SHELL_EFFECT_MAY_RO_VAR,
      kind: "readonly",
    };
  }
  // retain one explicitly proven external parent-binding noop
  if (SHELL_PARENT_BINDING_NOOP_COMMANDS.has(command) || command.includes("/")) {
    return { bits: 0, kind: "noop" };
  }
  return { bits: SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, kind: "all" };
}

// classify one literal builtin's direct parent-binding effects
function shellDirectFunctionEffect(command, exactWords, localNames) {
  return shellInvocationEffect(command, exactWords, localNames).bits;
}

// classify one function-write provenance payload
function shellInvocationFunctionCause(command, effect) {
  // stop without one function namespace write
  if ((effect.bits & SHELL_EFFECT_FN_WRITE) === 0) {
    return SHELL_AUTHORITY_NONE;
  }
  // retain one exact pending source provenance
  if ((command === "source" || command === ".") && effect.kind === "pending-source") {
    return SHELL_AUTHORITY_PENDING;
  }
  // retain one intrinsic dynamic source provenance
  if (command === "source" || command === ".") {
    return SHELL_AUTHORITY_SOURCE;
  }
  return SHELL_AUTHORITY_LOCAL;
}

// add one phase-owned direct function effect
function shellAddFunctionEffect(definition, bits, phase) {
  // retain only local-erasure evidence during phase one
  if (phase === "erase") {
    definition.node.bits |= bits & SHELL_EFFECT_MAY_ERASE_LOCAL;
    return;
  }
  // reject one late erasure discovery after topology freeze
  if (
    (bits & SHELL_EFFECT_MAY_ERASE_LOCAL) !== 0 &&
    (definition.node.bits & SHELL_EFFECT_MAY_ERASE_LOCAL) === 0
  ) {
    throw bootstrapError();
  }
  definition.node.bits |= bits;
}

// record one literal callee edge and external fallback
function shellRecordFunctionEffectCall(
  model,
  definition,
  command,
  exactWords,
  phase,
) {
  const nameNode = shellEffectNameNode(model, command);
  // create every reverse edge during phase one only
  if (phase === "erase") {
    nameNode.dependents.add(definition.node);
  }
  const shadowedDeclaration =
    ["local", "declare", "typeset", "readonly"].includes(command) &&
    model.definitionsByName.has(command);
  // retain one possible declaration-function call without spelling authority
  if (shadowedDeclaration) {
    // retain the builtin readonly fallback independently of the function edge
    if (command === "readonly") {
      const readonlyEffect = shellInvocationEffect(
        command,
        exactWords,
        shellFunctionLocalView(definition),
      );
      shellAddFunctionEffect(definition, readonlyEffect.bits, phase);
    }
    return;
  }
  // retain one proven nonbuiltin local call without an external fallback
  if (
    model.unconditionalNames.has(command) &&
    !SHELL_BUILTIN_FALLBACK_COMMANDS.has(command)
  ) {
    return;
  }
  const effect = shellInvocationEffect(
    command,
    exactWords,
    shellFunctionLocalView(definition),
  );
  shellAddFunctionEffect(definition, effect.bits, phase);
  // defer provenance until the complete effect pass
  if (phase === "erase") {
    return;
  }
  definition.node.functionCause = shellMergeAuthorityCause(
    definition.node.functionCause,
    shellInvocationFunctionCause(command, effect),
  );
}

// isolate one case-arm command body for effect scanning
function shellFunctionEffectCaseBody(tokens, depth) {
  // retain ordinary commands outside nested controls
  if (depth === 0) {
    return tokens;
  }
  const terminator = tokens.at(-1);
  // retain one header-only case arm
  if (terminator?.kind === "operator" && terminator.value === ")") {
    return [];
  }
  // retain one nonarm line
  if (
    terminator?.kind !== "operator" ||
    ![";;", ";&", ";;&"].includes(terminator.value)
  ) {
    return tokens;
  }
  // retain one standalone arm terminator
  if (tokens.length === 1) {
    return [];
  }
  const closeIndex = tokens.findIndex(
    // locate the case pattern terminator
    (token) => token.kind === "operator" && token.value === ")",
  );
  // reject one unowned case-arm spelling
  if (closeIndex === -1) {
    return null;
  }
  return tokens.slice(closeIndex + 1, -1);
}

// compact one owned array assignment for effect scanning
function shellCompactFunctionEffectArrays(tokens) {
  const compact = [];
  // consume every source token once
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    const assignment = token.kind === "word" ? shellAssignment(token) : null;
    const array = assignment === null
      ? null
      : shellArrayAssignment(tokens, index, assignment);
    // retain one ordinary token
    if (array === null) {
      compact.push(token);
      continue;
    }
    // reject one unterminated assignment without hiding its syntax
    if (array.malformed) {
      return null;
    }
    compact.push(token);
    index = array.nextIndex - 1;
  }
  return compact;
}

// identify one complete process-local subshell
function shellFunctionEffectSubshell(tokens) {
  // reject one non-subshell chunk
  if (
    tokens[0]?.kind !== "operator" ||
    tokens[0].value !== "(" ||
    tokens.at(-1)?.kind !== "operator" ||
    tokens.at(-1).value !== ")"
  ) {
    return false;
  }
  let depth = 0;
  // prove the outer close is the final token
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    // enter one subshell group
    if (token.kind === "operator" && token.value === "(") {
      depth += 1;
    }
    // leave one subshell group
    if (token.kind === "operator" && token.value === ")") {
      depth -= 1;
      // reject one premature or unmatched close
      if (depth < 0 || (depth === 0 && index !== tokens.length - 1)) {
        return false;
      }
    }
  }
  return depth === 0;
}

// scan one direct function-owned command chunk
function shellScanFunctionEffectChunk(model, definition, chunk, phase) {
  const ownedTokens = shellFunctionEffectCaseBody(chunk.tokens, chunk.depth);
  // reject one malformed case-arm owner
  if (ownedTokens === null) {
    shellAddFunctionEffect(definition, SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, phase);
    return;
  }
  // isolate one complete process-local binding environment
  if (shellFunctionEffectSubshell(ownedTokens)) {
    return;
  }
  const compactTokens = shellCompactFunctionEffectArrays(ownedTokens);
  // reject one malformed array owner
  if (compactTokens === null) {
    shellAddFunctionEffect(definition, SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, phase);
    return;
  }
  const splitOwned = shellSplitSegments;
  const split = splitOwned(compactTokens);
  // reject one unowned command shape conservatively
  if (!split.ok) {
    shellAddFunctionEffect(definition, SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, phase);
    return;
  }
  const conditionalChunk = chunk.depth !== 0 || split.segments.length !== 1;
  // inspect every direct segment once
  for (const segment of split.segments) {
    let segmentTokens = segment.tokens;
    // isolate one pipeline-local subshell binding environment
    if (shellFunctionEffectSubshell(segmentTokens)) {
      continue;
    }
    const control = segmentTokens[0]?.kind === "word"
      ? shellOptionalLiteralWord(segmentTokens[0])
      : null;
    // project one executable control header before command shaping
    if (["if", "elif", "while", "until"].includes(control)) {
      segmentTokens = segmentTokens.slice(1);
      const marker = segmentTokens.at(-1);
      // omit one exact body marker
      if (marker?.kind === "word" && ["then", "do"].includes(shellOptionalLiteralWord(marker))) {
        segmentTokens = segmentTokens.slice(0, -1);
      }
    } else if (["then", "else", "fi", "do", "done", "case", "esac"].includes(control)) {
      continue;
    } else if (control === "for" || control === "select") {
      // defer value-only loop targets until the complete effect pass
      if (phase === "erase") {
        continue;
      }
      const target = segmentTokens[1]?.kind === "word"
        ? shellOptionalLiteralWord(segmentTokens[1])
        : null;
      // retain one caller-visible assigning loop target
      if (target === null || !shellFunctionLocalProven(definition, target)) {
        shellAddFunctionEffect(definition, SHELL_EFFECT_VAR_WRITE, phase);
      }
      continue;
    }
    // retain one empty control marker
    if (segmentTokens.length === 0) {
      continue;
    }
    // retain one state-pure test expression
    if (segmentTokens[0]?.kind === "operator" && segmentTokens[0].value === "[[") {
      continue;
    }
    // classify one direct arithmetic parent write
    if (segmentTokens[0]?.kind === "operator" && segmentTokens[0].value === "((") {
      // retain one modeled variable mutation in the complete pass
      if (phase !== "erase" && shellArithmeticMutates(segmentTokens)) {
        shellAddFunctionEffect(definition, SHELL_EFFECT_VAR_WRITE, phase);
      }
      continue;
    }
    const shape = shellCommandShape(segmentTokens);
    // reject one malformed direct command
    if (!shape.ok || shape.words.length === 0) {
      shellAddFunctionEffect(definition, SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, phase);
      continue;
    }
    const words = shape.words;
    const exactWords = words.map(
      // decode one literal call word
      (word) => shellOptionalLiteralWord(word),
    );
    let wordIndex = 0;
    const assignments = [];
    // consume every leading assignment once
    while (wordIndex < words.length) {
      const assignment = shellAssignment(words[wordIndex]);
      // stop at one command word
      if (assignment === null) {
        break;
      }
      assignments.push(assignment.name);
      wordIndex += 1;
    }
    const command = exactWords[wordIndex];
    // classify only topology and erasure during phase one
    if (phase === "erase") {
      // skip one assignment-only statement
      if (wordIndex >= words.length) {
        continue;
      }
      // retain declaration and attribute builtins as non-erasing
      if (
        ["local", "declare", "typeset", "readonly"].includes(command) &&
        !model.definitionsByName.has(command)
      ) {
        continue;
      }
      // retain one dynamic producer as possible local erasure
      if (typeof command !== "string") {
        shellAddFunctionEffect(definition, SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, phase);
        continue;
      }
      shellRecordFunctionEffectCall(
        model,
        definition,
        command,
        exactWords.slice(wordIndex),
        phase,
      );
      continue;
    }
    // retain one exact local declaration without caller writes
    if (command === "local" && !model.definitionsByName.has(command)) {
      const declarationWords = words.slice(wordIndex + 1);
      let unsupported = false;
      const localNames = [];
      // classify every declaration operand once
      for (const word of declarationWords) {
        const assignment = shellAssignment(word);
        const value = shellOptionalLiteralWord(word);
        // retain one exact assignment target with a dynamic value
        if (assignment !== null) {
          localNames.push(assignment.name);
          continue;
        }
        // reject one nameref or unresolved operand
        if (value === null || value === "-n") {
          unsupported = true;
          continue;
        }
        // retain one reviewed declaration option
        if (value.startsWith("-")) {
          continue;
        }
        localNames.push(value);
      }
      // prove only unconditional exact local names
      if (!unsupported && !conditionalChunk) {
        for (const name of localNames) {
          // retain one exact local identifier
          if (/^[A-Za-z_][A-Za-z0-9_]*$/u.test(name)) {
            shellFunctionRecordLocal(definition, name);
          }
        }
      } else if (unsupported) {
        shellAddFunctionEffect(definition, SHELL_EFFECT_OPEN, phase);
      }
      continue;
    }
    // classify one function-local declare or typeset
    if (
      (command === "declare" || command === "typeset") &&
      !model.definitionsByName.has(command)
    ) {
      const flags = exactWords.slice(wordIndex + 1).filter(
        // retain option spellings only
        (value) => typeof value === "string" && value.startsWith("-"),
      );
      const global = flags.includes("-g");
      const readonly = flags.includes("-r");
      // retain one explicit global declaration effect
      if (global) {
        shellAddFunctionEffect(
          definition,
          SHELL_EFFECT_VAR_WRITE | (readonly ? SHELL_EFFECT_MAY_RO_VAR : 0),
          phase,
        );
      } else if (chunk.depth === 0 && !flags.includes("-n") && !exactWords.includes(null)) {
        // prove exact unconditional local declaration names
        for (const word of words.slice(wordIndex + 1)) {
          const value = shellOptionalLiteralWord(word);
          const assignment = shellAssignment(word);
          const name = assignment?.name ?? value;
          // retain one nonoption local identifier
          if (typeof name === "string" && !name.startsWith("-") && /^[A-Za-z_][A-Za-z0-9_]*$/u.test(name)) {
            shellFunctionRecordLocal(definition, name);
          }
        }
      }
      continue;
    }
    // classify one unproved readonly scope inside a function
    if (command === "readonly" && !model.definitionsByName.has(command)) {
      const readonlyEffect = shellDirectFunctionEffect(
        command,
        exactWords.slice(wordIndex),
        shellFunctionLocalView(definition),
      );
      shellAddFunctionEffect(definition, readonlyEffect, phase);
      continue;
    }
    // retain assignment-only writes outside proven local names
    if (wordIndex >= words.length) {
      // inspect every assignment target once
      for (const name of assignments) {
        // record one caller-visible assignment
        if (!shellFunctionLocalProven(definition, name)) {
          shellAddFunctionEffect(definition, SHELL_EFFECT_VAR_WRITE, phase);
        }
      }
      continue;
    }
    // reject one dynamic callee and ambiguous prefix ownership
    if (typeof command !== "string") {
      shellAddFunctionEffect(definition, SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN, phase);
      shellFunctionInvalidateLocals(definition);
      continue;
    }
    // prefix assignments are temporary only for one closed invocation
    if (assignments.length > 0 && !model.definitionsByName.has(command) && !shellReviewedCommand(command, "external")) {
      shellAddFunctionEffect(definition, SHELL_EFFECT_VAR_WRITE | SHELL_EFFECT_OPEN, phase);
    }
    shellRecordFunctionEffectCall(
      model,
      definition,
      command,
      exactWords.slice(wordIndex),
      phase,
    );
    const directEffect = model.unconditionalNames.has(command) &&
      !SHELL_BUILTIN_FALLBACK_COMMANDS.has(command)
      ? 0
      : shellDirectFunctionEffect(
        command,
        exactWords.slice(wordIndex),
        shellFunctionLocalView(definition),
      );
    const calleeMayErase = (model.nameNodes.get(command)?.bits ?? 0) & SHELL_EFFECT_MAY_ERASE_LOCAL;
    // invalidate later local proofs only after one proven erasing call
    if ((directEffect & SHELL_EFFECT_MAY_ERASE_LOCAL) !== 0 || calleeMayErase !== 0) {
      shellFunctionInvalidateLocals(definition);
    }
  }
}

// propagate one fixed monotone effect projection
function shellPropagateEffectProjection(model, projection) {
  const queue = [];
  // seed every nonempty projected node once
  for (const node of model.nodes) {
    // retain one projected seed
    if (projection(node) !== null) {
      queue.push(node);
    }
  }
  let cursor = 0;
  // consume every changed node once per projected lattice increase
  while (cursor < queue.length) {
    const node = queue[cursor];
    cursor += 1;
    // visit every reverse dependency after one projected change
    for (const dependent of node.dependents) {
      // enqueue only one changed projected dependent
      if (projection(node, dependent)) {
        queue.push(dependent);
      }
    }
  }
}

// solve one fixed-height function effect graph
function shellSolveFunctionEffects(linePlanGroups) {
  const model = shellCreateFunctionEffectModel();
  // collect every source into one possible-definition universe
  for (const linePlans of linePlanGroups) {
    shellCollectFunctionEffectBodies(linePlans, model);
  }
  const structuralBits = new Map();
  // preserve every collection-owned structural effect
  for (const definition of model.definitions) {
    structuralBits.set(definition.node, definition.node.bits);
    definition.node.bits = 0;
  }
  // scan every owned body once for erasure topology
  for (const definition of model.definitions) {
    // inspect every direct chunk once
    for (const chunk of definition.chunks) {
      shellScanFunctionEffectChunk(model, definition, chunk, "erase");
    }
  }
  // close the single local-erasure bit over the frozen topology
  shellPropagateEffectProjection(
    model,
    (node, dependent = null) => {
      const erased = node.bits & SHELL_EFFECT_MAY_ERASE_LOCAL;
      // seed one already-erasing node
      if (dependent === null) {
        return erased === 0 ? null : true;
      }
      // skip one unchanged erasure projection
      if (erased === 0 || (dependent.bits & SHELL_EFFECT_MAY_ERASE_LOCAL) !== 0) {
        return false;
      }
      dependent.bits |= SHELL_EFFECT_MAY_ERASE_LOCAL;
      return true;
    },
  );
  // restore definition-local state before the complete body pass
  for (const definition of model.definitions) {
    definition.localProofEpoch = 0;
    definition.localProofEpochs = new Map();
    definition.localNames = new Set();
    definition.node.bits |= structuralBits.get(definition.node);
    definition.node.functionCause = SHELL_AUTHORITY_NONE;
  }
  // freeze every phase-one node with all implied product facts
  for (const node of model.nodes) {
    // retain one positioned function-write provenance invariant
    if (node.functionCause !== SHELL_AUTHORITY_NONE) {
      node.bits |= SHELL_EFFECT_FN_WRITE;
    }
    node.bits = shellNormalizeEffectBits(node.bits);
  }
  // scan every owned body once with final callee erasure
  for (const definition of model.definitions) {
    // inspect every direct chunk once
    for (const chunk of definition.chunks) {
      shellScanFunctionEffectChunk(model, definition, chunk, "effects");
    }
    definition.node.bits = shellNormalizeEffectBits(definition.node.bits);
  }
  // close four booleans and the bounded function-cause chain
  shellPropagateEffectProjection(
    model,
    (node, dependent = null) => {
      // seed one nonempty complete summary
      if (dependent === null) {
        return node.bits === 0 && node.functionCause === SHELL_AUTHORITY_NONE
          ? null
          : true;
      }
      const next = shellNormalizeEffectBits(dependent.bits | node.bits);
      const nextFunctionCause = shellMergeAuthorityCause(
        dependent.functionCause,
        node.functionCause,
      );
      // skip one unchanged complete summary
      if (next === dependent.bits && nextFunctionCause === dependent.functionCause) {
        return false;
      }
      dependent.bits = next;
      dependent.functionCause = nextFunctionCause;
      return true;
    },
  );
  const effects = new Map();
  // publish one constant summary per retained literal function name
  for (const [name, node] of model.nameNodes) {
    effects.set(name, {
      effectMask: node.bits & SHELL_EFFECT_ALL_BINDINGS,
      functionCause: node.functionCause,
      open: (node.bits & SHELL_EFFECT_OPEN) !== 0,
    });
  }
  return effects;
}

// create one private imported-function profile registry
function shellCreateFunctionProfileRegistry() {
  return {
    names: new Map(),
    namesById: [null],
    radix: shellCreateRadixStore(),
    valueIds: new Map(),
    values: [null],
  };
}

// intern one imported function summary
function shellProfileValueId(registry, effect) {
  const key = `${effect.effectMask}:${effect.open ? 1 : 0}:${effect.functionCause ?? SHELL_AUTHORITY_NONE}`;
  const existing = registry.valueIds.get(key);
  // reuse one canonical summary value
  if (existing !== undefined) {
    return existing;
  }
  const id = registry.values.length;
  registry.values.push(effect);
  registry.valueIds.set(key, id);
  return id;
}

// write one imported function summary into a persistent root
function shellProfileSet(registry, root, name, effect) {
  const nameId = shellNameId(registry, name);
  return shellRadixSet(
    registry.radix,
    root,
    nameId,
    shellProfileValueId(registry, effect),
  );
}

// read one imported function summary from a persistent root
function shellProfileGet(registry, root, name) {
  const nameId = registry.names.get(name);
  // stop without one retained imported name
  if (nameId === undefined) {
    return null;
  }
  const valueId = shellRadixGet(registry.radix, root, nameId);
  return valueId === 0 ? null : registry.values[valueId];
}

// classify one module command's function-authority effect
function shellStaticFunctionMutation(words) {
  let cursor = 0;
  // skip every leading assignment once
  while (cursor < words.length && shellAssignment(words[cursor]) !== null) {
    cursor += 1;
  }
  // retain one assignment-only statement
  if (cursor === words.length) {
    return false;
  }
  const command = shellOptionalLiteralWord(words[cursor]);
  const exactWords = words.slice(cursor).map(
    // decode one module command atom
    (word) => shellOptionalLiteralWord(word),
  );
  // fail closed for one dynamic command authority
  if (command === null) {
    return true;
  }
  const effect = shellInvocationEffect(command, exactWords);
  return (effect.bits & (SHELL_EFFECT_FN_WRITE | SHELL_EFFECT_OPEN)) !== 0;
}

// classify one owned control header's function-authority effect
function shellStaticHeaderMutation(tokens, first) {
  // retain assigning loop headers without executing a command name
  if (["for", "select", "case"].includes(first)) {
    return false;
  }
  const marker = first === "if" ? "then" : "do";
  const markerIndex = shellOwnedBodyMarkerIndex(tokens, marker, 1);
  const header = tokens.slice(1, markerIndex === -1 ? tokens.length : markerIndex);
  // retain one empty or typed expression header
  if (
    header.length === 0 ||
    (header[0]?.kind === "operator" && ["[[", "(("].includes(header[0].value))
  ) {
    return false;
  }
  const split = shellSplitSegments(header);
  // fail closed for one malformed executable header
  if (!split.ok) {
    return true;
  }
  // inspect each executable header segment once
  for (const segment of split.segments) {
    const shape = shellCommandShape(segment.tokens);
    // retain one function-mutating or malformed header
    if (!shape.ok || shellStaticFunctionMutation(shape.words)) {
      return true;
    }
  }
  return false;
}

// collect one source's exact top-level import edges
function shellStaticImports(source, linePlans, pathResolution) {
  const environment = shellCreateEnvironment();
  const imports = [];
  const controls = [];
  let cwdBase = source.resolutionBase;
  let functionDepth = 0;
  let dominating = true;
  // mark one active control function world unsafe
  function markControlUnsafe() {
    const control = controls.at(-1);
    // retain one top-level permanent loss
    if (control === undefined) {
      dominating = false;
      return;
    }
    control.functionUnsafe = true;
  }
  // inspect every logical line once
  for (const plan of linePlans) {
    const tokens = plan.grammarTokens;
    const firstWord = tokens.find(
      // locate one first literal word
      (token) => token.kind === "word",
    );
    const first = firstWord === undefined ? null : shellOptionalLiteralWord(firstWord);
    const functionName = shellFunctionSignature(tokens);
    // enter one multiline function body
    if (functionName !== null) {
      // conditional definitions do not dominate caller authority
      if (controls.length > 0) {
        markControlUnsafe();
      }
      functionDepth += 1;
      continue;
    }
    // leave one multiline function body
    if (functionDepth > 0 && tokens.length === 1 && tokens[0].kind === "operator" && tokens[0].value === "}") {
      functionDepth -= 1;
      continue;
    }
    // ignore every function body line
    if (functionDepth > 0) {
      continue;
    }
    // close one module control after its last branch
    if (shellEffectControlClose(first)) {
      const closed = controls.pop();
      // propagate only one actual function-world mutation
      if (closed?.functionUnsafe) {
        markControlUnsafe();
      }
      continue;
    }
    // enter one owned module control
    if (shellEffectControlOpen(first)) {
      controls.push({ functionUnsafe: false });
      // apply condition effects before branching
      if (shellStaticHeaderMutation(tokens, first)) {
        markControlUnsafe();
      }
      continue;
    }
    // retain one exact same-line definition set
    if (plan.declarations.length > 0) {
      // conditional inline definitions do not dominate all continuations
      if (controls.length > 0) {
        markControlUnsafe();
      }
      continue;
    }
    const shape = shellCommandShape(tokens);
    // fail closed for one malformed module statement
    if (!shape.ok) {
      markControlUnsafe();
      continue;
    }
    // retain one empty structural marker
    if (shape.words.length === 0) {
      continue;
    }
    const words = shape.words;
    let cursor = 0;
    const assignments = [];
    // consume every leading assignment once
    while (cursor < words.length) {
      const assignment = shellAssignment(words[cursor]);
      // stop at one command word
      if (assignment === null) {
        break;
      }
      assignments.push(assignment);
      cursor += 1;
    }
    // retain exact assignment-only constants
    if (cursor === words.length) {
      for (const assignment of assignments) {
        const value = assignment.append
          ? null
          : shellResolveWord(
              assignment.valueWord,
              source,
              environment,
              SHELL_LITERAL_BYTE_CAP,
              pathResolution,
            );
        // retain or invalidate one source-order constant
        if (value === null) {
          shellEnvironmentTop(environment, assignment.name);
        } else {
          shellEnvironmentSet(environment, assignment.name, value);
        }
      }
      continue;
    }
    const command = shellOptionalLiteralWord(words[cursor]);
    // invalidate one static source base after directory mutation
    if (command === "cd") {
      cwdBase = null;
    }
    // retain one direct source invocation
    if (command === "source" || command === ".") {
      const targetValue = words.length === cursor + 2
        ? shellResolveWord(
            words[cursor + 1],
            source,
            environment,
            SHELL_LITERAL_BYTE_CAP,
            pathResolution,
          )
        : null;
      const relative = targetValue !== null && (
        targetValue.startsWith("./") || !targetValue.includes("/")
      );
      const target = targetValue === null || (relative && cwdBase === null)
        ? null
        : shellSourceTarget(
          targetValue,
          relative ? { ...source, resolutionBase: cwdBase } : source,
        );
      const exactContinuation = dominating && controls.length === 0;
      imports.push({ dominating: exactContinuation, position: words[cursor].start, target });
      // unresolved or conditional source code opens the caller function world
      if (target === null || !exactContinuation) {
        markControlUnsafe();
      }
      continue;
    }
    // preserve function authority across proven child and option-only commands
    if (shellStaticFunctionMutation(words)) {
      markControlUnsafe();
    }
  }
  return imports;
}

// collect one source's definitely installed function names
function shellSourceFunctionExports(linePlans) {
  const names = [];
  let complete = true;
  let controlDepth = 0;
  let functionDepth = 0;
  let terminated = false;
  // inspect every source line once
  for (const plan of linePlans) {
    const tokens = plan.grammarTokens;
    const firstWord = tokens.find(
      // locate one first literal word
      (token) => token.kind === "word",
    );
    const first = firstWord === undefined ? null : shellOptionalLiteralWord(firstWord);
    const multilineName = shellFunctionSignature(tokens);
    // enter one multiline definition body
    if (multilineName !== null) {
      functionDepth += 1;
      // retain only one reachable unconditional definition
      if (controlDepth === 0 && !terminated) {
        names.push(multilineName);
      } else {
        complete = false;
      }
      continue;
    }
    // leave one multiline definition body
    if (
      functionDepth > 0 &&
      tokens.length === 1 &&
      tokens[0].kind === "operator" &&
      tokens[0].value === "}"
    ) {
      functionDepth -= 1;
      continue;
    }
    // ignore one function body line
    if (functionDepth > 0) {
      continue;
    }
    // close one module control before its terminator
    if (shellEffectControlClose(first)) {
      controlDepth = Math.max(0, controlDepth - 1);
      continue;
    }
    // retain one reachable inline declaration set
    if (plan.declarations.length > 0) {
      // reject one conditional or posttermination export
      if (controlDepth !== 0 || terminated) {
        complete = false;
      } else {
        for (const declaration of plan.declarations) {
          names.push(declaration.name);
        }
      }
    }
    // mark one top-level source termination boundary
    if (
      controlDepth === 0 &&
      ["exec", "exit", "return"].includes(first)
    ) {
      terminated = true;
    }
    // reject one caller-returning or uncertain conditional termination
    if (
      controlDepth > 0 &&
      ["exec", "return"].includes(first)
    ) {
      complete = false;
    }
    // enter one module control body
    if (shellEffectControlOpen(first)) {
      controlDepth += 1;
    }
  }
  return { complete, names };
}

// summarize one source initializer's modeled caller effects
function shellSourceInitializerEffect(linePlans, effects) {
  let bits = 0;
  let functionCause = SHELL_AUTHORITY_NONE;
  let functionDepth = 0;
  // inspect every module-owned line once
  for (const plan of linePlans) {
    const tokens = plan.grammarTokens;
    const functionName = shellFunctionSignature(tokens);
    // enter one multiline function body
    if (functionName !== null) {
      functionDepth += 1;
      continue;
    }
    // leave one multiline function body
    if (
      functionDepth > 0 &&
      tokens.length === 1 &&
      tokens[0].kind === "operator" &&
      tokens[0].value === "}"
    ) {
      functionDepth -= 1;
      continue;
    }
    // ignore every deferred function body line
    if (functionDepth > 0 || plan.declarations.length > 0) {
      continue;
    }
    const shape = shellCommandShape(tokens);
    // retain one malformed initializer as intrinsically open
    if (!shape.ok) {
      bits |= SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN;
      functionCause = shellMergeAuthorityCause(functionCause, SHELL_AUTHORITY_SOURCE);
      continue;
    }
    let cursor = 0;
    // consume every leading assignment once
    while (cursor < shape.words.length && shellAssignment(shape.words[cursor]) !== null) {
      cursor += 1;
    }
    // retain assignment initialization in the target's private value domain
    if (cursor === shape.words.length) {
      continue;
    }
    const command = shellOptionalLiteralWord(shape.words[cursor]);
    // retain one dynamic initializer as intrinsically open
    if (command === null) {
      bits |= SHELL_EFFECT_ALL_BINDINGS | SHELL_EFFECT_OPEN;
      functionCause = shellMergeAuthorityCause(functionCause, SHELL_AUTHORITY_SOURCE);
      continue;
    }
    // defer exact nested source authority to the import graph
    if (command === "source" || command === ".") {
      continue;
    }
    const localEffect = effects.get(command);
    // apply one exact local initializer call summary
    if (localEffect !== undefined) {
      bits |= localEffect.effectMask | (localEffect.open ? SHELL_EFFECT_OPEN : 0);
      functionCause = shellMergeAuthorityCause(
        functionCause,
        localEffect.functionCause ?? SHELL_AUTHORITY_LOCAL,
      );
      continue;
    }
    const exactWords = shape.words.slice(cursor).map(
      // decode one initializer call atom
      (word) => shellOptionalLiteralWord(word),
    );
    const direct = shellInvocationEffect(command, exactWords);
    // retain only modeled parent-binding effects
    bits |= direct.bits;
    functionCause = shellMergeAuthorityCause(
      functionCause,
      shellInvocationFunctionCause(command, direct),
    );
  }
  return {
    effectMask: bits & SHELL_EFFECT_ALL_BINDINGS,
    functionCause,
    open: (bits & SHELL_EFFECT_OPEN) !== 0,
  };
}

// build scoped imported-function profiles in linear graph passes
function shellBuildFunctionProfiles(shellSources) {
  const lexedByPath = new Map();
  const plansByPath = new Map();
  const importsByPath = new Map();
  const exportsByPath = new Map();
  const sourceByPath = new Map();
  // plan every source once
  for (const source of shellSources) {
    const lexed = shellWords(source.sourceText, 0);
    const plans = shellPlanLines(lexed.tokens);
    lexedByPath.set(source.path, lexed);
    plansByPath.set(source.path, plans);
    importsByPath.set(
      source.path,
      shellStaticImports(
        source,
        plans,
        shellCreatePathResolution(lexed.lexicon),
      ),
    );
    exportsByPath.set(source.path, shellSourceFunctionExports(plans));
    sourceByPath.set(source.path, source);
  }
  const neighbors = new Map();
  // seed every source component vertex
  for (const source of shellSources) {
    neighbors.set(source.path, new Set());
  }
  // connect every exact provided import once
  for (const source of shellSources) {
    for (const edge of importsByPath.get(source.path)) {
      // ignore one missing dependency for component closure
      if (!edge.dominating || !sourceByPath.has(edge.target)) {
        continue;
      }
      neighbors.get(source.path).add(edge.target);
      neighbors.get(edge.target).add(source.path);
    }
  }
  const componentByPath = new Map();
  const components = [];
  // discover every import-connected component iteratively
  for (const source of shellSources) {
    // skip one assigned component
    if (componentByPath.has(source.path)) {
      continue;
    }
    const component = [];
    const stack = [source.path];
    componentByPath.set(source.path, components.length);
    // consume every component vertex once
    while (stack.length > 0) {
      const path = stack.pop();
      component.push(path);
      // visit every exact import neighbor once
      for (const neighbor of neighbors.get(path)) {
        // retain one unassigned neighbor
        if (!componentByPath.has(neighbor)) {
          componentByPath.set(neighbor, components.length);
          stack.push(neighbor);
        }
      }
    }
    components.push(component);
  }
  const effectsByComponent = [];
  // solve every effect-only component independently
  for (const component of components) {
    effectsByComponent.push(shellSolveFunctionEffects(component.map(
      // project one source plan group
      (path) => plansByPath.get(path),
    )));
  }
  const registry = shellCreateFunctionProfileRegistry();
  const profiles = new Map();
  const profileInputs = new Map();
  const reverseDependencies = new Map();
  const pendingDependencies = new Map();
  const queue = [];
  // classify every caller authority dependency once
  for (const source of shellSources) {
    const provided = importsByPath.get(source.path).filter(
      // retain only exact provided imports
      (edge) => edge.dominating && sourceByPath.has(edge.target),
    );
    const missingPositions = importsByPath.get(source.path).filter(
      // retain every missing static dependency
      (edge) => !edge.dominating || !sourceByPath.has(edge.target),
    ).map(
      // project one unknown-producer position
      (edge) => edge.position,
    );
    const blocked = provided.length > 1;
    profileInputs.set(source.path, { blocked, missingPositions, provided });
    const pending = blocked ? 0 : provided.length;
    pendingDependencies.set(source.path, pending);
    // index one reverse single-import edge
    if (!blocked && provided.length === 1) {
      const callers = reverseDependencies.get(provided[0].target) ?? [];
      callers.push(source.path);
      reverseDependencies.set(provided[0].target, callers);
    }
    // seed one dependency-free or blocked profile
    if (pending === 0) {
      queue.push(source.path);
    }
  }
  let queueCursor = 0;
  // resolve every acyclic profile exactly once
  while (queueCursor < queue.length) {
    const path = queue[queueCursor];
    queueCursor += 1;
    const input = profileInputs.get(path);
    const componentEffects = effectsByComponent[componentByPath.get(path)];
    // retain one fail-closed multi-import authority view
    if (input.blocked) {
      profiles.set(path, {
        activationPosition: Number.POSITIVE_INFINITY,
        complete: false,
        completeTargets: new Set(),
        effects: componentEffects,
        exportRoot: 0,
        importEffects: new Map(),
        importRoot: 0,
        initializerEffect: shellSourceInitializerEffect(
          plansByPath.get(path),
          componentEffects,
        ),
        openPosition: minimumValue(input.missingPositions, Number.POSITIVE_INFINITY),
        ownNames: new Set(),
        providedTargets: new Set(input.provided.map(
          // retain every present direct dependency
          (edge) => edge.target,
        )),
        registry,
      });
    } else {
      const targetProfile = input.provided.length === 1
        ? profiles.get(input.provided[0].target)
        : null;
      const targetComplete = targetProfile?.complete === true;
      const importRoot = targetComplete ? targetProfile.exportRoot : 0;
      let exportRoot = importRoot;
      const ownNames = new Set();
      const sourceExports = exportsByPath.get(path);
      // point-write every definitely installed definition once
      for (const name of sourceExports.names) {
        const effect = componentEffects.get(name) ?? { effectMask: 0, open: true };
        exportRoot = shellProfileSet(registry, exportRoot, name, effect);
        ownNames.add(name);
      }
      const complete = sourceExports.complete && (
        input.provided.length === 0 || targetComplete
      );
      const importEffects = new Map();
      // retain one complete direct initializer summary
      if (targetComplete) {
        importEffects.set(
          input.provided[0].target,
          targetProfile.initializerEffect,
        );
      }
      const incompleteImportPosition = input.provided.length === 1 && !targetComplete
        ? input.provided[0].position
        : Number.POSITIVE_INFINITY;
      profiles.set(path, {
        activationPosition: input.provided[0]?.position ?? Number.POSITIVE_INFINITY,
        complete,
        completeTargets: new Set(input.provided.filter(
          // retain only complete direct dependencies
          () => targetComplete,
        ).map(
          // retain only direct complete imports
          (edge) => edge.target,
        )),
        effects: componentEffects,
        exportRoot,
        importEffects,
        importRoot,
        initializerEffect: shellSourceInitializerEffect(
          plansByPath.get(path),
          componentEffects,
        ),
        openPosition: minimumValue(input.missingPositions, incompleteImportPosition),
        ownNames,
        providedTargets: new Set(input.provided.map(
          // retain every present direct dependency
          (edge) => edge.target,
        )),
        registry,
      });
    }
    // release every direct dependent once
    for (const caller of reverseDependencies.get(path) ?? []) {
      const pending = pendingDependencies.get(caller) - 1;
      pendingDependencies.set(caller, pending);
      // enqueue one newly ready caller
      if (pending === 0) {
        queue.push(caller);
      }
    }
  }
  // fail closed for every cyclic authority view
  for (const source of shellSources) {
    // skip one resolved acyclic view
    if (profiles.has(source.path)) {
      continue;
    }
    profiles.set(source.path, {
      activationPosition: Number.POSITIVE_INFINITY,
      complete: false,
      completeTargets: new Set(),
      effects: effectsByComponent[componentByPath.get(source.path)],
      exportRoot: 0,
      importEffects: new Map(),
      importRoot: 0,
      initializerEffect: {
        effectMask: SHELL_EFFECT_ALL_BINDINGS,
        functionCause: SHELL_AUTHORITY_SOURCE,
        open: true,
      },
      openPosition: 0,
      ownNames: new Set(),
      providedTargets: new Set(),
      registry,
    });
  }
  return { lexedByPath, plansByPath, profiles };
}

// read one source-scoped imported function summary
function shellImportedFunctionEffect(state, name) {
  const profile = state.functionProfile;
  // stop without one imported authority view
  if (profile === null) {
    return null;
  }
  return shellProfileGet(profile.registry, profile.importRoot, name);
}

// prove one exact static import is complete in this caller view
function shellCompleteImport(state, target, position) {
  const profile = state.functionProfile;
  return profile !== null &&
    position >= profile.activationPosition &&
    position < profile.openPosition &&
    profile.completeTargets.has(target);
}

// read one complete imported initializer summary
function shellCompleteImportEffect(state, target, position) {
  // stop without one exact complete dependency
  if (!shellCompleteImport(state, target, position)) {
    return null;
  }
  return state.functionProfile.importEffects.get(target) ?? {
    effectMask: 0,
    functionCause: SHELL_AUTHORITY_NONE,
    open: false,
  };
}

// identify one present but incomplete dependency
function shellIncompleteProvidedImport(state, target) {
  return state.functionProfile?.providedTargets.has(target) === true &&
    !state.functionProfile.completeTargets.has(target);
}

// analyze one complete inline function body
function shellProcessInlineFunction(state, tokens, declaration, emissionContext) {
  shellDefineFunction(state, declaration.name);
  const node = shellCreateNode(state, {
    attributes: { identifier: declaration.name },
    attach: true,
    end: tokens[declaration.end - 1].end,
    factKind: "function",
    start: tokens[declaration.start].start,
  });
  const savedEnvironment = state.environment;
  const savedFunctionEnvironment = state.functionEnvironment;
  const savedLocalNameRoot = state.localNameRoot;
  const savedOwner = state.ownerSymbol;
  state.contexts.push({ node, type: "function" });
  state.environment = shellCreateEnvironment(savedEnvironment.registry, "variable");
  state.functionEnvironment = shellCloneEnvironment(savedFunctionEnvironment, true);
  state.localNameRoot = 0;
  state.ownerSymbol = declaration.name;
  const body = tokens.slice(declaration.bodyStart, declaration.bodyEnd);
  // analyze one nonempty inline body
  if (body.length > 0) {
    (0, shellProcessSegments)(state, body, emissionContext);
  }
  state.ownerSymbol = savedOwner;
  state.functionEnvironment = savedFunctionEnvironment;
  state.localNameRoot = savedLocalNameRoot;
  state.environment = savedEnvironment;
  state.contexts.pop();
}

// process one line containing inline declarations
function shellProcessInlineLine(state, plan, emissionContext) {
  let cursor = 0;
  // process every declaration and intervening statement once
  for (const declaration of plan.declarations) {
    const prefix = shellInlineStatementSlice(
      plan.grammarTokens,
      cursor,
      declaration.start,
    );
    // retain one ordinary prefix
    if (prefix.length > 0) {
      (0, shellProcessSegments)(state, prefix, emissionContext);
    }
    (0, shellProcessInlineFunction)(
      state,
      plan.grammarTokens,
      declaration,
      emissionContext,
    );
    cursor = declaration.end;
  }
  const suffix = shellInlineStatementSlice(
    plan.grammarTokens,
    cursor,
    plan.grammarTokens.length,
  );
  // retain one ordinary suffix
  if (suffix.length > 0) {
    (0, shellProcessSegments)(state, suffix, emissionContext);
  }
}

// open one function body
function shellOpenFunction(state, tokens, name) {
  shellDefineFunction(state, name);
  const node = shellCreateNode(state, {
    attributes: { identifier: name },
    attach: true,
    end: tokens.at(-1).end,
    factKind: "function",
    start: tokens[0].start,
  });
  state.contexts.push({
    savedFunctionEnvironment: state.functionEnvironment,
    savedLocalNameRoot: state.localNameRoot,
    node,
    savedEnvironment: state.environment,
    savedOwner: state.ownerSymbol,
    type: "function",
  });
  // free function bindings are call-time unknown during definition analysis
  state.environment = shellCreateEnvironment(state.environment.registry, "variable");
  state.functionEnvironment = shellCloneEnvironment(state.functionEnvironment, true);
  state.localNameRoot = 0;
  state.ownerSymbol = name;
}

// close one function body
function shellCloseFunction(state, context, end) {
  context.node.end = Math.max(context.node.end, end);
  state.environment = context.savedEnvironment;
  state.functionEnvironment = context.savedFunctionEnvironment;
  state.localNameRoot = context.savedLocalNameRoot;
  state.ownerSymbol = context.savedOwner;
  state.contexts.pop();
}

// locate one owned body marker at command boundary
function shellOwnedBodyMarkerIndex(tokens, marker, start = 0) {
  const delimiters = shellDelimiterIndex(tokens);
  // reject one malformed owner range
  if (!delimiters.valid) {
    return -1;
  }
  let found = -1;
  // inspect every possible marker once
  for (let index = start; index < tokens.length; index += 1) {
    const token = tokens[index];
    const ownedClose = delimiters.matches.get(index);
    // skip one nested owned range without rescanning it
    if (ownedClose !== undefined) {
      index = ownedClose;
      continue;
    }
    const part = token?.kind === "word" && token.parts.length === 1
      ? token.parts[0]
      : null;
    // retain only one unquoted standalone keyword
    if (
      part?.kind !== "text" ||
      part.quote !== "unquoted" ||
      part.value !== marker
    ) {
      continue;
    }
    const previous = tokens[index - 1];
    // require one command-list boundary
    if (
      index === start ||
      (previous?.kind === "operator" && previous.value === ";")
    ) {
      found = index;
    }
  }
  return found;
}

// open one if control
function shellOpenIf(state, tokens, emissionContext) {
  const node = shellCreateNode(state, {
    attributes: { identifier: "if" },
    attach: true,
    end: tokens.at(-1).end,
    factKind: "control",
    start: tokens[0].start,
  });
  const markerIndex = shellOwnedBodyMarkerIndex(tokens, "then", 1);
  const condition = tokens.slice(1, markerIndex === -1 ? tokens.length : markerIndex);
  // retain one positioned condition expression
  if (condition.length > 0) {
    const test = shellCreateNode(state, {
      attributes: { identifier: "shell-test", literalRole: "opaque" },
      attach: false,
      end: condition.at(-1).end,
      factKind: "expression",
      start: condition[0].start,
    });
    shellAddChild(node, test, "test", 0);
    (0, shellProcessSegments)(state, condition, emissionContext, node);
  }
  const context = {
    branches: [],
    currentContinues: true,
    entry: state.environment,
    exhaustive: false,
    falseEnvironment: state.environment,
    falseFunctionEnvironment: state.functionEnvironment,
    functionEntry: state.functionEnvironment,
    functionPrefix: state.functionEnvironment.events,
    node,
    priorIf: state.currentIf,
    type: "if",
    variablePrefix: state.environment.events,
  };
  state.contexts.push(context);
  state.currentIf = context;
  state.environment = shellCloneEnvironment(context.entry, true);
  state.functionEnvironment = shellCloneEnvironment(context.functionEntry, true);
}

// advance one if alternative
function shellNextIfBranch(state, context, exhaustive) {
  context.branches.push({
    continues: context.currentContinues,
    environment: state.environment,
    functionEnvironment: state.functionEnvironment,
  });
  context.currentContinues = true;
  context.exhaustive ||= exhaustive;
  state.environment = shellCloneEnvironment(context.falseEnvironment, true);
  state.functionEnvironment = shellCloneEnvironment(context.falseFunctionEnvironment, true);
}

// close one if control
function shellCloseIf(state, context, end) {
  shellNextIfBranch(state, context, false);
  // retain one explicit final false continuation
  if (!context.exhaustive) {
    context.branches.push({
      continues: true,
      environment: context.falseEnvironment,
      functionEnvironment: context.falseFunctionEnvironment,
    });
  }
  context.node.end = Math.max(context.node.end, end);
  const hasContinuation = context.branches.some(
    // locate one continuing explicit branch
    (branch) => branch.continues,
  );
  state.environment = shellJoinEnvironments(
    state,
    context.entry,
    context.branches.map(
      // project one variable continuation
      (branch) => branch.continues ? branch.environment : null,
    ),
    false,
    context.variablePrefix,
  );
  state.functionEnvironment = shellJoinEnvironments(
    state,
    context.functionEntry,
    context.branches.map(
      // project one function continuation
      (branch) => branch.continues ? branch.functionEnvironment : null,
    ),
    false,
    context.functionPrefix,
  );
  state.contexts.pop();
  state.currentIf = context.priorIf;
  // stop one enclosing case arm only when every if branch terminates
  if (!hasContinuation) {
    // mark one noncontinuing case arm
    if (state.currentCase !== null && state.currentCase.currentArm !== null) {
      state.currentCase.currentArm.continues = false;
    }
  }
}

// open one loop control
function shellOpenLoop(state, tokens, kind, emissionContext) {
  const node = shellCreateNode(state, {
    attributes: { identifier: kind },
    attach: true,
    end: tokens.at(-1).end,
    factKind: "control",
    start: tokens[0].start,
  });
  const header = tokens.slice(1);
  // retain one positioned loop header
  if (header.length > 0) {
    const test = shellCreateNode(state, {
      attributes: { identifier: "shell-loop-test", literalRole: "opaque" },
      attach: false,
      end: header.at(-1).end,
      factKind: "expression",
      start: header[0].start,
    });
    shellAddChild(node, test, "test", 0);
  }
  // execute one while or until header before the zero-body entry
  if (kind === "while" || kind === "until") {
    const doIndex = shellOwnedBodyMarkerIndex(header, "do", 0);
    const executableHeader = doIndex === -1 ? header : header.slice(0, doIndex);
    const opaqueHeader = executableHeader[0]?.kind === "operator" &&
      ["[[", "(("].includes(executableHeader[0].value);
    // execute one command-list header before its zero-body entry
    if (executableHeader.length > 0 && !opaqueHeader) {
      (0, shellProcessSegments)(state, executableHeader, emissionContext, node);
    }
    // retain one direct arithmetic header mutation before entry
    if (opaqueHeader && executableHeader[0].value === "((" && shellArithmeticMutates(executableHeader)) {
      shellEnvironmentPromoteTop(state.environment);
      shellPushDiagnostic(state);
    }
    // retain substitutions owned by one opaque test
    if (opaqueHeader) {
      for (const token of executableHeader) {
        // queue each nested substitution once
        if (token.kind === "word") {
          shellQueueWordSubstitutions(state, token, node, state.ownerSymbol);
        }
      }
    }
  }
  const context = {
    entry: state.environment,
    functionEntry: state.functionEnvironment,
    functionPrefix: state.functionEnvironment.events,
    node,
    priorLoop: state.currentLoop,
    type: "loop",
    variablePrefix: state.environment.events,
  };
  state.contexts.push(context);
  state.currentLoop = context;
  state.environment = shellCloneEnvironment(context.entry, true);
  state.functionEnvironment = shellCloneEnvironment(context.functionEntry, true);
  // invalidate one assigning loop target
  if ((kind === "for" || kind === "select") && tokens[1]?.kind === "word") {
    const name = shellLiteralWord(tokens[1]);
    // retain one exact loop target
    if (name !== null && /^[A-Za-z_][A-Za-z0-9_]*$/u.test(name)) {
      shellInvalidateBinding(state, name);
    }
  }
  // invalidate the implicit select reply target
  if (kind === "select") {
    shellInvalidateBinding(state, "REPLY");
  }
  // discover substitutions in nonexecutable assigning headers
  if (kind === "for" || kind === "select") {
    for (const token of tokens.slice(1)) {
      // queue one header substitution
      if (token.kind === "word") {
        shellQueueWordSubstitutions(state, token, node, state.ownerSymbol);
      }
    }
  }
}

// close one loop control
function shellCloseLoop(state, context, end) {
  context.node.end = Math.max(context.node.end, end);
  state.environment = shellJoinEnvironments(
    state,
    context.entry,
    [state.environment],
    true,
    context.variablePrefix,
  );
  state.functionEnvironment = shellJoinEnvironments(
    state,
    context.functionEntry,
    [state.functionEnvironment],
    true,
    context.functionPrefix,
  );
  state.contexts.pop();
  state.currentLoop = context.priorLoop;
}

// recognize one sql-target path word
function shellSqlPathWord(word) {
  // inspect literal path fragments only
  return word.parts.some(
    // locate one sql suffix
    (part) => part.kind === "text" && /\.sql(?:$|\/)/u.test(part.value),
  );
}

// retain private sql heredoc evidence only
function shellRecordSqlHeredocs(state, tokens, heredocs) {
  // avoid inspecting ordinary command words
  if (heredocs.length === 0) {
    return;
  }
  const split = shellSplitSegments(tokens);
  // stop before malformed list ownership
  if (!split.ok) {
    return;
  }
  const words = [];
  // validate every simple pipeline member before classification
  for (const segment of split.segments) {
    const shape = shellCommandShape(segment.tokens);
    // stop before one malformed member
    if (!shape.ok) {
      return;
    }
    appendValues(words, shape.words);
  }
  const allWords = tokens.filter(
    // retain redirect operands for typed path evidence
    (token) => token.kind === "word",
  );
  const literals = words.map(
    // decode one literal-only command word
    (word) => shellOptionalLiteralWord(word),
  );
  const sqlCommand = literals.includes("psql") || literals.some(
    // locate one reviewed nested psql spelling
    (value) => typeof value === "string" && /(?:^|[ ;])exec psql(?:[ ;]|$)/u.test(value),
  ) || (literals[0] === "cat" && allWords.some(
    // locate one sql output path
    (word) => shellSqlPathWord(word),
  ));
  // stop without a typed sql consumer
  if (!sqlCommand) {
    return;
  }
  // retain every positioned sql body
  for (const heredoc of heredocs) {
    // skip one empty or malformed body
    if (heredoc.bodyEnd <= heredoc.bodyStart) {
      continue;
    }
    const sourceText = state.source.sourceText.slice(heredoc.bodyStart, heredoc.bodyEnd);
    const retainLiteral = heredoc.quoted &&
      Buffer.byteLength(sourceText, "utf8") <= SHELL_LITERAL_BYTE_CAP;
    shellCreateNode(state, {
      attributes: retainLiteral
        ? {
            identifier: "sql",
            literalKind: "string",
            literalRole: "semantic-constant",
            literalValue: sourceText,
          }
        : { identifier: "sql", literalRole: "opaque" },
      attach: true,
      end: heredoc.bodyEnd,
      factKind: "embedded-source",
      start: heredoc.bodyStart,
    });
  }
}

// open one finite case control
function shellOpenCase(state, tokens) {
  const node = shellCreateNode(state, {
    attributes: { identifier: "case" },
    attach: true,
    end: tokens.at(-1).end,
    factKind: "control",
    start: tokens[0].start,
  });
  const testWord = tokens.find(
    // locate one case test word
    (token, index) => index > 0 && token.kind === "word" && shellOptionalLiteralWord(token) !== "in",
  );
  // retain one actual case test
  if (testWord !== undefined) {
    shellAddChild(node, shellWordNode(state, testWord, state.environment), "test", 0);
  }
  const context = {
    branches: [],
    currentArm: null,
    entry: state.environment,
    exhaustive: false,
    functionEntry: state.functionEnvironment,
    functionPrefix: state.functionEnvironment.events,
    node,
    priorCase: state.currentCase,
    type: "case",
    variablePrefix: state.environment.events,
  };
  state.contexts.push(context);
  state.currentCase = context;
  state.environment = shellCloneEnvironment(context.entry, true);
  state.functionEnvironment = shellCloneEnvironment(context.functionEntry, true);
}

// run one closed semantic frame stack
function shellRunSemanticFrames(state, initialFrame) {
  const frames = [initialFrame];
  // consume every semantic frame LIFO
  try {
    while (frames.length > 0) {
      const frame = frames.pop();
      // reduce one line frame
      if (frame.kind === "line") {
        shellStepLineFrame(state, frames, frame);
      } else if (frame.kind === "list" || frame.kind === "group") {
        // reduce one command-list or pipeline group frame
        shellStepListFrame(state, frames, frame);
      } else if (frame.kind === "command") {
        // reduce one command frame
        shellStepCommandFrame(state, frames, frame);
      } else if (frame.kind === "scope") {
        // enter one scoped child
        shellStepScopeFrame(state, frames, frame);
      } else if (frame.kind === "control") {
        // reduce one legacy control line
        shellStepControlFrame(state, frames, frame);
      } else if (frame.kind === "restore") {
        // restore one completed scope
        shellRestoreSemanticFrame(state, frame);
      }
    }
  } catch (error) {
    // restore every entered scope before rethrow
    while (frames.length > 0) {
      const frame = frames.pop();
      // restore only pending exits
      if (frame.kind === "restore") {
        shellRestoreSemanticFrame(state, frame);
      }
    }
    throw error;
  }
}

// reduce one logical line frame
function shellStepLineFrame(state, frames, frame) {
  const { plan } = frame;
  const rangeIndex = plan.rangeIndex;
  // schedule one same-line declaration sequence without native recursion
  if (plan.declarations.length > 0) {
    frames.push({
      context: frame.emissionContext ?? "simple",
      cursor: 0,
      declarationCursor: 0,
      kind: "control",
      mode: "inline-sequence",
      phase: "next",
      plan,
    });
    return;
  }
  const tokens = plan.grammarTokens;
  const firstWord = tokens.find(
    // locate one first direct word
    (token) => token.kind === "word",
  );
  const first = firstWord === undefined ? null : shellOptionalLiteralWord(firstWord);
  const top = state.contexts.at(-1);
  // preserve one ordinary or multiline function boundary
  if (
    rangeIndex === undefined || !rangeIndex.requiresFrames ||
    shellFunctionSignature(tokens) !== null ||
    (first === null && tokens.length === 1 && tokens[0].kind === "operator" &&
      tokens[0].value === "}")
  ) {
    frames.push({ ...frame, kind: "control" });
    return;
  }
  const caseTerminator = tokens.some(
    // locate one case-arm terminator
    (token) => token.kind === "operator" && [";;", ";&", ";;&"].includes(token.value),
  );
  // process one arm boundary before nested compounds
  if (
    top?.type === "case" &&
    (first === "esac" || top.currentArm === null || caseTerminator)
  ) {
    frames.push({
      context: frame.emissionContext ?? "simple",
      kind: "control",
      mode: "case-line",
      plan,
      top,
    });
    return;
  }
  // open one structural nested or module case header
  if (first === "case") {
    frames.push({
      kind: "control",
      mode: "case-open",
      plan,
    });
    return;
  }
  // schedule one structural if header
  if (first === "if") {
    frames.push({
      context: frame.emissionContext ?? "simple",
      kind: "control",
      mode: "if-open",
      plan,
    });
    return;
  }
  // schedule one structural elif header
  if (first === "elif" && top?.type === "if") {
    frames.push({
      context: frame.emissionContext ?? "simple",
      kind: "control",
      mode: "elif-open",
      plan,
      top,
    });
    return;
  }
  // preserve branch, close and inert control markers exactly
  if (
    (first === "else" && top?.type === "if") ||
    (first === "fi" && top?.type === "if") ||
    (first === "done" && top?.type === "loop") ||
    ["then", "do"].includes(first)
  ) {
    frames.push({ ...frame, kind: "control" });
    return;
  }
  // schedule one structural loop header
  if (["for", "while", "until", "select"].includes(first)) {
    frames.push({
      context: frame.emissionContext ?? "simple",
      kind: "control",
      loopKind: first,
      mode: "loop-open",
      plan,
    });
    return;
  }
  // process one ordinary active case-arm line after nested controls
  if (top?.type === "case") {
    frames.push({
      context: frame.emissionContext ?? "simple",
      kind: "control",
      mode: "case-line",
      plan,
      top,
    });
    return;
  }
  const savedArithmeticContinuation = state.arithmeticContinuation;
  state.arithmeticContinuation = plan.arithmeticContinuation === true;
  // invalidate one unclosed arithmetic effect
  if (plan.arithmeticStart === true) {
    shellEnvironmentPromoteTop(state.environment);
  }
  frames.push({
    kind: "restore",
    savedArithmeticContinuation,
    scope: "line",
  });
  frames.push({
    context: frame.emissionContext ?? "simple",
    end: tokens.length,
    kind: "list",
    parent: null,
    plan,
    start: 0,
  });
}

// reduce one direct command-list range
// reduce one direct command-list or pipeline range
function shellStepListFrame(state, frames, frame) {
  // reduce one pipeline or simple group continuation
  if (frame.kind === "group") {
    // finish one just-executed segment
    if (frame.phase === "after-segment") {
      // continue one disposable pipeline in source order
      if (frame.pipeline) {
        frame.segmentCursor += 1;
        // return one unchanged pipeline path
        if (frame.segmentCursor >= frame.group.segmentEnd) {
          frame.returnFrame.childResult = {
            changed: new Set(),
            functionChanged: new Set(),
            path: frame.path,
          };
          return;
        }
        frame.phase = "next-segment";
        frames.push(frame);
        return;
      }
      // retain only one ordinary segment's changed keys
      for (const name of frame.changed) {
        frame.output.changed.add(name);
      }
      // retain only one ordinary segment's changed function keys
      for (const name of frame.functionChanged) {
        frame.output.functionChanged.add(name);
      }
      frame.returnFrame.childResult = state.segmentContinues
        ? {
            changed: frame.changed,
            functionChanged: frame.functionChanged,
            path: frame.output,
          }
        : null;
      return;
    }
    // schedule one pipeline component or ordinary segment
    if (frame.phase === "next-segment") {
      const segment = frame.segments[frame.segmentCursor];
      const pipeline = frame.group.segmentEnd - frame.group.segmentStart > 1;
      frame.pipeline = pipeline;
      frame.phase = "after-segment";
      frames.push({
        child: {
          context: frame.context,
          end: segment.end,
          kind: "command",
          parent: frame.parent,
          phase: "enter",
          plan: frame.plan,
          start: segment.start,
        },
        environment: pipeline
          ? shellCloneEnvironment(frame.path.environment)
          : frame.output.environment,
        functionEnvironment: pipeline
          ? shellCloneEnvironment(frame.path.functionEnvironment)
          : frame.output.functionEnvironment,
        kind: "scope",
        logicalChanged: pipeline ? null : frame.changed,
        logicalFunctionChanged: pipeline ? null : frame.functionChanged,
        resume: frame,
        scope: "segment",
        trackAssignments: !pipeline,
      });
      return;
    }
    const pipeline = frame.group.segmentEnd - frame.group.segmentStart > 1;
    frame.segmentCursor = frame.group.segmentStart;
    frame.pipeline = pipeline;
    frame.output = pipeline
      ? frame.path
      : (frame.forkPath ? shellForkEnvironmentPath(frame.path) : frame.path);
    frame.changed = new Set();
    frame.functionChanged = new Set();
    frame.phase = "next-segment";
    frames.push(frame);
    return;
  }

  // finish one unconditional command
  if (frame.phase === "after-unconditional") {
    // discard bindings after one noncontinuing unconditional command
    if (!state.segmentContinues) {
      state.environment = shellCreateEnvironment(state.environment.registry, "variable");
    }
    return;
  }

  // merge one completed logical-list group
  if (frame.phase === "after-group") {
    const output = frame.childResult;
    const rightSuccess = frame.reachable && output !== null ? output.path : null;
    const rightFailure = rightSuccess;
    // retain only reachable segment changes
    if (rightSuccess !== null) {
      for (const name of output.changed) {
        frame.alternativeChanged.add(name);
      }
      // retain only reachable function changes
      for (const name of output.functionChanged) {
        frame.alternativeFunctionChanged.add(name);
      }
    }
    // seed one first command status pair
    if (frame.groupCursor === 0) {
      frame.success = rightSuccess;
      frame.failure = rightFailure;
    } else if (frame.operator === "&&") {
      // merge only the right command delta into accumulated failures
      if (rightFailure !== null) {
        frame.failure = shellJoinEnvironmentAlternatives(
          [frame.failure, rightFailure],
          output.changed,
          output.functionChanged,
        );
      }
      frame.success = rightSuccess;
    } else if (frame.operator === "||") {
      // merge only the right command delta into accumulated successes
      if (rightSuccess !== null) {
        frame.success = shellJoinEnvironmentAlternatives(
          [frame.success, rightSuccess],
          output.changed,
          output.functionChanged,
        );
      }
      frame.failure = rightFailure;
    } else {
      frame.success = rightSuccess;
      frame.failure = rightFailure;
    }
    frame.groupCursor += 1;
    frame.phase = "next-group";
    frames.push(frame);
    return;
  }

  // schedule one logical-list group from the exact status lattice
  if (frame.phase === "next-group") {
    // commit one completed command list
    if (frame.groupCursor >= frame.groups.length) {
      const joined = shellJoinEnvironmentAlternatives(
        [frame.success, frame.failure],
        frame.alternativeChanged,
        frame.alternativeFunctionChanged,
      );
      state.environment = shellCommitEnvironmentPath(frame.entry, joined) ??
        shellCreateEnvironment(state.environment.registry, "variable");
      state.functionEnvironment = joined === null
        ? shellCreateEnvironment(state.functionEnvironment.registry, "function")
        : shellCloneEnvironment(joined.functionEnvironment);
      return;
    }
    const operator = frame.groupCursor === 0
      ? null
      : frame.groups[frame.groupCursor - 1].operator;
    let input;
    let forkInput = false;
    // seed one first command from the fixed logical-list entry
    if (frame.groupCursor === 0) {
      input = frame.initial;
    } else if (operator === "&&") {
      // continue only the success alternative
      input = frame.success;
      forkInput = input !== null && input === frame.failure;
    } else if (operator === "||") {
      // continue only the failure alternative
      input = frame.failure;
      forkInput = input !== null && input === frame.success;
    } else {
      // commit one completed sequence before continuing
      const joined = shellJoinEnvironmentAlternatives(
        [frame.success, frame.failure],
        frame.alternativeChanged,
        frame.alternativeFunctionChanged,
      );
      const committed = shellCommitEnvironmentPath(frame.entry, joined);
      const committedFunction = joined === null
        ? null
        : shellCloneEnvironment(joined.functionEnvironment);
      input = committed === null
        ? null
        : shellCreateEnvironmentPath(committed, committedFunction);
      frame.alternativeChanged = new Set();
      frame.alternativeFunctionChanged = new Set();
    }
    frame.operator = operator;
    frame.reachable = input !== null;
    frame.childResult = null;
    frame.phase = "after-group";
    frames.push(frame);
    frames.push({
      context: frame.context,
      forkPath: forkInput,
      group: frame.groups[frame.groupCursor],
      kind: "group",
      parent: frame.parent,
      path: input ?? shellCreateEnvironmentPath(frame.entry, frame.functionEntry),
      phase: "enter",
      plan: frame.plan,
      returnFrame: frame,
      segments: frame.segments,
    });
    return;
  }

  const tokens = frame.plan.grammarTokens;
  const rangeIndex = frame.plan.rangeIndex;
  const segments = [];
  let start = frame.start;
  let cursor = frame.start;
  let malformed = rangeIndex.invalidDelimiterPrefix[frame.end] !==
    rangeIndex.invalidDelimiterPrefix[frame.start];
  // partition only direct-owned relations
  while (cursor < frame.end && !malformed) {
    const token = tokens[cursor];
    const close = rangeIndex.pairClose[cursor];
    // jump one complete indexed child range
    if (close >= cursor && close < frame.end) {
      cursor = close + 1;
      continue;
    }
    // reject one unterminated or range-external direct opener
    if (
      token?.kind === "operator" &&
      ["(", "((", "[[", "{"].includes(token.value) &&
      (close < cursor || close >= frame.end)
    ) {
      malformed = true;
      break;
    }
    // reject one unmatched or range-external closer
    if (
      token?.kind === "operator" &&
      [")", "))", "]]", "}"].includes(token.value) &&
      (rangeIndex.pairOpen[cursor] < frame.start || rangeIndex.pairOpen[cursor] < 0)
    ) {
      malformed = true;
      break;
    }
    // retain one direct command boundary
    if (token?.kind === "operator" && [";", "&&", "||", "|", "|&"].includes(token.value)) {
      // reject one missing nonsequence operand
      if (start === cursor && token.value !== ";") {
        malformed = true;
        break;
      }
      // retain one nonempty direct segment
      if (start < cursor) {
        segments.push({ end: cursor, operator: token.value, start });
      }
      start = cursor + 1;
    }
    cursor += 1;
  }
  // retain one final direct segment
  if (!malformed && start < frame.end) {
    segments.push({ end: frame.end, operator: null, start });
  }
  const finalOperator = segments.at(-1)?.operator ?? null;
  // reject one dangling logical or pipeline relation
  if (
    !malformed && start === frame.end &&
    ["&&", "||", "|", "|&"].includes(finalOperator)
  ) {
    malformed = true;
  }
  // reject one malformed typed command list
  if (malformed) {
    // invalidate every malformed array target transactionally
    for (let index = frame.start; index + 1 < frame.end; index += 1) {
      const assignment = tokens[index]?.kind === "word"
        ? shellAssignment(tokens[index])
        : null;
      // retain one apparent array assignment
      if (
        assignment !== null &&
        tokens[index + 1]?.kind === "operator" &&
        tokens[index + 1].value === "("
      ) {
        shellInvalidateBinding(state, assignment.name);
      }
    }
    shellPushDiagnostic(state);
    return;
  }
  const groups = [];
  let groupStart = 0;
  // group every maximal direct pipeline once
  for (let index = 0; index < segments.length; index += 1) {
    const segment = segments[index];
    // continue one direct pipeline
    if (segment.operator === "|" || segment.operator === "|&") {
      continue;
    }
    groups.push({
      operator: segment.operator,
      segmentEnd: index + 1,
      segmentStart: groupStart,
    });
    groupStart = index + 1;
  }
  // retain one malformed trailing pipeline conservatively
  if (groupStart < segments.length) {
    groups.push({
      operator: null,
      segmentEnd: segments.length,
      segmentStart: groupStart,
    });
  }
  // run one unconditional command without any environment fork or join
  if (groups.length === 1 && groups[0].segmentEnd - groups[0].segmentStart === 1) {
    const segment = segments[0];
    state.segmentContinues = true;
    frame.phase = "after-unconditional";
    frames.push(frame);
    frames.push({
      context: frame.context,
      end: segment.end,
      kind: "command",
      parent: frame.parent,
      phase: "enter",
      plan: frame.plan,
      start: segment.start,
    });
    return;
  }
  let parent = frame.parent;
  // create one actual compound command parent
  if (segments.length > 1 && parent === null) {
    parent = shellCreateNode(state, {
      attributes: { identifier: "command-list" },
      attach: true,
      end: tokens[frame.end - 1].end,
      factKind: "control",
      start: tokens[frame.start].start,
    });
  }
  frame.alternativeChanged = new Set();
  frame.alternativeFunctionChanged = new Set();
  frame.entry = state.environment;
  frame.failure = null;
  frame.functionEntry = state.functionEnvironment;
  frame.groupCursor = 0;
  frame.groups = groups;
  frame.initial = shellCreateEnvironmentPath(frame.entry, frame.functionEntry);
  frame.parent = parent;
  frame.segments = segments;
  frame.success = null;
  frame.phase = "next-group";
  frames.push(frame);
}

// reduce one typed command range
function shellStepCommandFrame(state, frames, frame) {
  const tokens = frame.plan.grammarTokens;
  const rangeIndex = frame.plan.rangeIndex;
  let start = frame.commandStart ?? frame.start;
  const end = frame.end;
  // consume one list negation prefix once
  if (frame.phase === "enter") {
    // advance beyond one exact negation word
    if (tokens[start]?.kind === "word" && shellOptionalLiteralWord(tokens[start]) === "!") {
      start += 1;
    }
    frame.commandStart = start;
  }
  // reject one empty command
  if (start >= end) {
    shellPushDiagnostic(state);
    return;
  }
  // resume after one process-substitution body
  if (frame.phase === "next-process") {
    const nextIndex = frame.processCursor + 1;
    // schedule one remaining process body
    if (nextIndex < frame.processes.length) {
      frame.processCursor = nextIndex;
      frames.push(frame);
      const process = frame.processes[nextIndex];
      frames.push({
        child: {
          context: "process-substitution",
          end: process.close,
          kind: "list",
          parent: null,
          plan: frame.plan,
          start: process.start,
        },
        kind: "scope",
        parent: frame.parent,
        scope: "isolated-compound",
      });
      return;
    }
    frame.phase = "emit";
  }
  // enter one simple-command syntax parent before owned substitutions
  if (frame.phase === "start-process") {
    frame.phase = "next-process";
    frames.push(frame);
    frames.push({
      child: {
        context: "process-substitution",
        end: frame.processes[0].close,
        kind: "list",
        parent: null,
        plan: frame.plan,
        start: frame.processes[0].start,
      },
      kind: "scope",
      parent: frame.parent,
      scope: "compound-parent",
    });
    return;
  }
  // emit one simple leaf after its owned substitutions
  if (frame.phase === "emit") {
    const shape = shellCommandShape(tokens, start, end, rangeIndex);
    const saved = state.semanticFrameSubstitutions;
    state.semanticFrameSubstitutions = true;
    try {
      const processSimple = shellProcessSimple;
      processSimple(
        state,
        tokens,
        frame.context,
        frame.parentScoped ? null : frame.parent,
        start,
        end,
        shape,
      );
    } finally {
      state.semanticFrameSubstitutions = saved;
    }
    return;
  }
  // retain one exact opaque test or arithmetic command
  if ((0, shellProcessTestSegment)(state, tokens, frame.parent, start, end)) {
    return;
  }
  const close = rangeIndex.pairClose[start];
  // enter one exact outer compound range
  if (close === end - 1 && rangeIndex.executablePairs.has(start)) {
    const opener = tokens[start];
    // enter one isolated subshell body
    if (opener.value === "(") {
      const node = shellCreateNode(state, {
        attributes: { identifier: "subshell" },
        attach: frame.parent === null,
        end: tokens[close].end,
        factKind: "control",
        start: opener.start,
      });
      // attach one explicit parent
      if (frame.parent !== null) {
        shellAddChild(frame.parent, node, "body");
      }
      frames.push({
        child: {
          context: frame.context,
          end: close,
          kind: "list",
          parent: null,
          plan: frame.plan,
          start: start + 1,
        },
        kind: "scope",
        parent: node,
        scope: "isolated-compound",
      });
      return;
    }
    // enter one shared brace body
    frames.push({
      context: frame.context,
      end: close,
      kind: "list",
      parent: frame.parent,
      plan: frame.plan,
      start: start + 1,
    });
    return;
  }
  const processes = [];
  let cursor = start;
  // collect direct process substitutions without descending
  while (cursor < end) {
    const process = rangeIndex.processByDirection.get(cursor);
    // retain one direct process body
    if (process !== undefined && process.end <= end) {
      processes.push(process);
      cursor = process.end;
      continue;
    }
    const childClose = rangeIndex.pairClose[cursor];
    // skip one nonprocess child range
    if (childClose >= cursor && childClose < end) {
      cursor = childClose + 1;
      continue;
    }
    cursor += 1;
  }
  // execute process bodies before one simple command
  if (processes.length > 0) {
    frame.phase = "start-process";
    frame.processes = processes;
    frame.processCursor = 0;
    // install one syntax parent across substitutions and emission
    if (frame.parent !== null) {
      frame.parentScoped = true;
      frames.push({
        child: frame,
        kind: "scope",
        parent: frame.parent,
        scope: "syntax-parent",
      });
    } else {
      frames.push(frame);
    }
    return;
  }
  frame.phase = "emit";
  frames.push(frame);
}

// enter one exact semantic scope
function shellStepScopeFrame(state, frames, frame) {
  // enter one isolated compound with exact nested custody
  if (frame.scope === "isolated-compound") {
    frames.push({
      child: {
        child: frame.child,
        kind: "scope",
        parent: frame.parent,
        scope: "compound-parent",
      },
      kind: "scope",
      scope: "isolation",
    });
    return;
  }
  // enter one process-local isolation scope
  if (frame.scope === "isolation") {
    const restore = {
      kind: "restore",
      savedCase: state.currentCase,
      savedContinues: state.segmentContinues,
      savedEnvironment: state.environment,
      savedFunctionEnvironment: state.functionEnvironment,
      savedIf: state.currentIf,
      savedIsolatedDepth: state.isolatedDepth,
      savedLoop: state.currentLoop,
      scope: "isolation",
    };
    frames.push(restore);
    state.currentCase = null;
    state.currentIf = null;
    state.isolatedDepth += 1;
    state.currentLoop = null;
    state.segmentContinues = true;
    state.environment = shellCloneEnvironment(restore.savedEnvironment, true);
    state.functionEnvironment = shellCloneEnvironment(
      restore.savedFunctionEnvironment,
      true,
    );
    frames.push(frame.child);
    return;
  }
  // install one compound parent across the whole owned body
  if (frame.scope === "compound-parent") {
    const restore = {
      kind: "restore",
      parent: frame.parent,
      priorContextCount: state.contexts.length,
      scope: "compound-parent",
    };
    frames.push(restore);
    // attach one explicit compound parent
    if (frame.parent !== null) {
      state.contexts.push({ node: frame.parent, type: "compound" });
    }
    frames.push(frame.child);
    return;
  }
  // install one simple-command parent before owned substitutions
  if (frame.scope === "syntax-parent") {
    const restore = {
      kind: "restore",
      parent: frame.parent,
      priorContextCount: state.contexts.length,
      scope: "syntax-parent",
    };
    frames.push(restore);
    // attach one explicit syntax parent
    if (frame.parent !== null) {
      state.contexts.push({ node: frame.parent, type: "substitution" });
    }
    frames.push(frame.child);
    return;
  }
  // enter one logical segment journal
  if (frame.scope === "segment") {
    const restore = {
      kind: "restore",
      savedChanged: state.logicalChanged,
      savedFunctionChanged: state.logicalFunctionChanged,
      savedTracking: state.trackAssignments,
      scope: "segment",
    };
    frames.push(frame.resume);
    frames.push(restore);
    state.environment = frame.environment;
    state.functionEnvironment = frame.functionEnvironment;
    state.segmentContinues = true;
    state.logicalChanged = frame.logicalChanged;
    state.logicalFunctionChanged = frame.logicalFunctionChanged;
    state.trackAssignments = frame.trackAssignments;
    frames.push(frame.child);
  }
}

// reduce one legacy control boundary
function shellStepControlFrame(state, frames, frame) {
  // open one finite case header
  if (frame.mode === "case-open") {
    const openCase = shellOpenCase;
    openCase(state, frame.plan.grammarTokens);
    return;
  }
  // finish one structural case arm body
  if (frame.mode === "case-after-body") {
    // finish one explicit arm terminator
    if (frame.armEnd !== -1) {
      // reject one fallthrough arm
      if (frame.plan.grammarTokens[frame.armEnd].value !== ";;") {
        shellPushDiagnostic(state);
        frame.top.exhaustive = false;
      }
      shellFinishCaseArm(state, frame.top);
    }
    return;
  }
  // process one case-body line without recursive segment dispatch
  if (frame.mode === "case-line") {
    const tokens = frame.plan.grammarTokens;
    const firstWord = tokens.find(
      // locate one first case-body word
      (token) => token.kind === "word",
    );
    const firstValue = firstWord === undefined ? null : shellOptionalLiteralWord(firstWord);
    // close one complete case statement
    if (firstValue === "esac") {
      shellCloseCase(state, frame.top, tokens.at(-1).end);
      return;
    }
    let bodyStart = 0;
    // locate one arm-pattern close
    if (frame.top.currentArm === null) {
      for (let index = 0; index < tokens.length; index += 1) {
        // open one first arm boundary
        if (tokens[index].kind === "operator" && tokens[index].value === ")") {
          const openCaseArm = shellOpenCaseArm;
          openCaseArm(state, frame.top, tokens, index);
          bodyStart = index + 1;
          break;
        }
      }
    }
    let armEnd = -1;
    // locate one direct arm terminator
    for (let index = bodyStart; index < tokens.length; index += 1) {
      // retain one exact arm terminator
      if (tokens[index].kind === "operator" && [";;", ";&", ";;&"].includes(tokens[index].value)) {
        armEnd = index;
        break;
      }
    }
    const bodyEnd = armEnd === -1 ? tokens.length : armEnd;
    frames.push({ ...frame, armEnd, mode: "case-after-body" });
    // execute one nonempty arm body
    if (bodyStart < bodyEnd) {
      frames.push({
        context: frame.context,
        end: bodyEnd,
        kind: "list",
        parent: null,
        plan: frame.plan,
        start: bodyStart,
      });
    }
    return;
  }
  // activate one completed if header
  if (frame.mode === "if-after-condition") {
    const context = {
      branches: [],
      currentContinues: true,
      entry: state.environment,
      exhaustive: false,
      falseEnvironment: state.environment,
      falseFunctionEnvironment: state.functionEnvironment,
      functionEntry: state.functionEnvironment,
      functionPrefix: state.functionEnvironment.events,
      node: frame.node,
      priorIf: state.currentIf,
      type: "if",
      variablePrefix: state.environment.events,
    };
    state.contexts.push(context);
    state.currentIf = context;
    state.environment = shellCloneEnvironment(context.entry, true);
    state.functionEnvironment = shellCloneEnvironment(context.functionEntry, true);
    return;
  }
  // open one if header before its branch context
  if (frame.mode === "if-open") {
    const tokens = frame.plan.grammarTokens;
    const node = shellCreateNode(state, {
      attributes: { identifier: "if" },
      attach: true,
      end: tokens.at(-1).end,
      factKind: "control",
      start: tokens[0].start,
    });
    const markerIndex = shellOwnedBodyMarkerIndex(tokens, "then", 1);
    const conditionEnd = markerIndex === -1 ? tokens.length : markerIndex;
    // retain one positioned condition expression
    if (conditionEnd > 1) {
      const test = shellCreateNode(state, {
        attributes: { identifier: "shell-test", literalRole: "opaque" },
        attach: false,
        end: tokens[conditionEnd - 1].end,
        factKind: "expression",
        start: tokens[1].start,
      });
      shellAddChild(node, test, "test", 0);
    }
    frames.push({ ...frame, mode: "if-after-condition", node });
    // execute one nonempty condition before branch entry
    if (conditionEnd > 1) {
      frames.push({
        context: frame.context,
        end: conditionEnd,
        kind: "list",
        parent: node,
        plan: frame.plan,
        start: 1,
      });
    }
    return;
  }
  // finish one structural elif condition
  if (frame.mode === "elif-after-condition") {
    frame.top.falseEnvironment = state.environment;
    frame.top.falseFunctionEnvironment = state.functionEnvironment;
    state.environment = shellCloneEnvironment(frame.top.falseEnvironment, true);
    state.functionEnvironment = shellCloneEnvironment(
      frame.top.falseFunctionEnvironment,
      true,
    );
    return;
  }
  // advance one elif branch before its condition
  if (frame.mode === "elif-open") {
    const tokens = frame.plan.grammarTokens;
    shellNextIfBranch(state, frame.top, false);
    const markerIndex = shellOwnedBodyMarkerIndex(tokens, "then", 1);
    const conditionEnd = markerIndex === -1 ? tokens.length : markerIndex;
    frames.push({ ...frame, mode: "elif-after-condition" });
    // execute one nonempty elif condition
    if (conditionEnd > 1) {
      frames.push({
        context: frame.context,
        end: conditionEnd,
        kind: "list",
        parent: frame.top.node,
        plan: frame.plan,
        start: 1,
      });
    }
    return;
  }
  // activate one loop after its executable header
  if (frame.mode === "loop-after-header") {
    const tokens = frame.plan.grammarTokens;
    const context = {
      entry: state.environment,
      functionEntry: state.functionEnvironment,
      functionPrefix: state.functionEnvironment.events,
      node: frame.node,
      priorLoop: state.currentLoop,
      type: "loop",
      variablePrefix: state.environment.events,
    };
    state.contexts.push(context);
    state.currentLoop = context;
    state.environment = shellCloneEnvironment(context.entry, true);
    state.functionEnvironment = shellCloneEnvironment(context.functionEntry, true);
    // invalidate one assigning loop target
    if (["for", "select"].includes(frame.loopKind) && tokens[1]?.kind === "word") {
      const name = shellLiteralWord(tokens[1]);
      // retain one exact loop target
      if (name !== null && /^[A-Za-z_][A-Za-z0-9_]*$/u.test(name)) {
        shellInvalidateBinding(state, name);
      }
    }
    // invalidate one implicit select reply target
    if (frame.loopKind === "select") {
      shellInvalidateBinding(state, "REPLY");
    }
    // discover substitutions in one nonexecuted assigning header
    if (["for", "select"].includes(frame.loopKind)) {
      for (let index = 1; index < tokens.length; index += 1) {
        // queue one positioned header word
        if (tokens[index].kind === "word") {
          shellQueueWordSubstitutions(state, tokens[index], node, state.ownerSymbol);
        }
      }
    }
    return;
  }
  // open one loop before its body context
  if (frame.mode === "loop-open") {
    const tokens = frame.plan.grammarTokens;
    const node = shellCreateNode(state, {
      attributes: { identifier: frame.loopKind },
      attach: true,
      end: tokens.at(-1).end,
      factKind: "control",
      start: tokens[0].start,
    });
    // retain one positioned loop header
    if (tokens.length > 1) {
      const test = shellCreateNode(state, {
        attributes: { identifier: "shell-loop-test", literalRole: "opaque" },
        attach: false,
        end: tokens.at(-1).end,
        factKind: "expression",
        start: tokens[1].start,
      });
      shellAddChild(node, test, "test", 0);
    }
    const markerIndex = shellOwnedBodyMarkerIndex(tokens, "do", 1);
    const headerEnd = markerIndex === -1 ? tokens.length : markerIndex;
    const opaqueHeader = tokens[1]?.kind === "operator" &&
      ["[[", "(("].includes(tokens[1].value);
    // retain one direct opaque arithmetic header mutation
    if (opaqueHeader && tokens[1].value === "((") {
      const chunks = [];
      let nestedMutation = false;
      // collect one bounded arithmetic spelling
      for (let index = 1; index < headerEnd; index += 1) {
        const token = tokens[index];
        // skip structural arithmetic delimiters
        if (token.kind === "operator") {
          continue;
        }
        // preserve token boundaries without interpreting expansions
        if (token.kind !== "word") {
          chunks.push(" 0 ");
          continue;
        }
        // inspect every direct word part once
        for (const part of token.parts) {
          // retain literal arithmetic syntax
          if (part.kind === "text") {
            chunks.push(part.value);
          } else if (part.kind === "arithmetic") {
            // retain one already finalized nested mutation
            if (part.mutatesParent === true) {
              nestedMutation = true;
            }
            chunks.push(" (0) ");
          } else {
            // isolate nonparent substitution effects
            chunks.push(" 0 ");
          }
        }
        chunks.push(" ");
      }
      const expression = chunks.join("");
      const directMutation = /\+\+|--|(?:^|[^=!<>])(?:<<=|>>=|\*\*=|[+*/%&|^-]?=)(?!=)/u.test(
        expression,
      );
      // widen one mutating opaque arithmetic header
      if (nestedMutation || directMutation) {
        shellEnvironmentPromoteTop(state.environment);
        shellPushDiagnostic(state);
      }
    }
    // retain substitutions owned by one opaque header
    if (opaqueHeader) {
      for (let index = 1; index < headerEnd; index += 1) {
        // queue every positioned opaque header word
        if (tokens[index].kind === "word") {
          shellQueueWordSubstitutions(state, tokens[index], node, state.ownerSymbol);
        }
      }
    }
    frames.push({ ...frame, mode: "loop-after-header", node });
    // execute one structural while or until header first
    if (
      ["while", "until"].includes(frame.loopKind) &&
      headerEnd > 1 &&
      !opaqueHeader
    ) {
      frames.push({
        context: frame.context,
        end: headerEnd,
        kind: "list",
        parent: node,
        plan: frame.plan,
        start: 1,
      });
    }
    return;
  }
  // process one legacy-unsupported inline body once
  if (frame.mode === "legacy-inline-body") {
    const processSegments = shellProcessSegments;
    processSegments(state, frame.tokens, frame.context);
    return;
  }
  // continue one same-line declaration sequence
  if (frame.mode === "inline-sequence") {
    const tokens = frame.plan.grammarTokens;
    const declaration = frame.plan.declarations[frame.declarationCursor];
    // finish one trailing statement suffix
    if (declaration === undefined) {
      let start = frame.cursor;
      let end = tokens.length;
      // trim one leading declaration boundary
      if (tokens[start]?.kind === "operator" && tokens[start].value === ";") {
        start += 1;
      }
      // trim one trailing declaration boundary
      if (tokens[end - 1]?.kind === "operator" && tokens[end - 1].value === ";") {
        end -= 1;
      }
      // schedule one nonempty suffix
      if (start < end) {
        frames.push({
          context: frame.context,
          end,
          kind: "list",
          parent: null,
          plan: frame.plan,
          start,
        });
      }
      return;
    }
    // schedule one ordinary prefix before the declaration
    if (frame.phase === "next") {
      let start = frame.cursor;
      let end = declaration.start;
      // trim one leading declaration boundary
      if (tokens[start]?.kind === "operator" && tokens[start].value === ";") {
        start += 1;
      }
      // trim one trailing declaration boundary
      if (tokens[end - 1]?.kind === "operator" && tokens[end - 1].value === ";") {
        end -= 1;
      }
      frame.phase = "declaration";
      frames.push(frame);
      // execute one nonempty prefix first
      if (start < end) {
        frames.push({
          context: frame.context,
          end,
          kind: "list",
          parent: null,
          plan: frame.plan,
          start,
        });
      }
      return;
    }
    shellDefineFunction(state, declaration.name);
    const node = shellCreateNode(state, {
      attributes: { identifier: declaration.name },
      attach: true,
      end: tokens[declaration.end - 1].end,
      factKind: "function",
      start: tokens[declaration.start].start,
    });
    const restore = {
      kind: "restore",
      priorContextCount: state.contexts.length,
      savedEnvironment: state.environment,
      savedFunctionEnvironment: state.functionEnvironment,
      savedLocalNameRoot: state.localNameRoot,
      savedOwner: state.ownerSymbol,
      scope: "inline",
    };
    // resume one declaration sequence after exact restoration
    const continuation = {
      ...frame,
      cursor: declaration.end,
      declarationCursor: frame.declarationCursor + 1,
      phase: "next",
    };
    frames.push(continuation);
    frames.push(restore);
    state.contexts.push({ node, type: "function" });
    state.environment = shellCreateEnvironment(restore.savedEnvironment.registry, "variable");
    state.functionEnvironment = shellCloneEnvironment(
      restore.savedFunctionEnvironment,
      true,
    );
    state.localNameRoot = 0;
    state.ownerSymbol = declaration.name;
    const children = frame.plan.rangeIndex.declarationChildren.get(declaration.id) ?? [];
    // preserve one legacy unsupported nested declaration body
    if (children.length > 0) {
      const body = [];
      // copy one unsupported body only once
      for (let index = declaration.bodyStart; index < declaration.bodyEnd; index += 1) {
        body.push(tokens[index]);
      }
      frames.push({
        context: frame.context,
        kind: "control",
        mode: "legacy-inline-body",
        tokens: body,
      });
    } else if (declaration.bodyStart < declaration.bodyEnd) {
      // schedule one admitted declaration body
      frames.push({
        context: frame.context,
        end: declaration.bodyEnd,
        kind: "list",
        parent: null,
        plan: frame.plan,
        start: declaration.bodyStart,
      });
    }
    return;
  }
  void frames;
  const processLine = shellProcessLine;
  processLine(
    state,
    frame.plan.tokens,
    frame.plan,
    frame.emissionContext ?? "simple",
  );
}

// restore one completed semantic scope
function shellRestoreSemanticFrame(state, frame) {
  // restore one line-local continuation bit
  if (frame.scope === "line") {
    state.arithmeticContinuation = frame.savedArithmeticContinuation;
    return;
  }
  // restore one segment journal without discarding its result state
  if (frame.scope === "segment") {
    state.logicalChanged = frame.savedChanged;
    state.logicalFunctionChanged = frame.savedFunctionChanged;
    state.trackAssignments = frame.savedTracking;
    return;
  }
  // restore one explicit syntax or compound parent
  if (frame.scope === "syntax-parent" || frame.scope === "compound-parent") {
    // truncate only one actually installed parent context
    if (frame.parent !== null && state.contexts.length > frame.priorContextCount) {
      state.contexts.length = frame.priorContextCount;
    }
    return;
  }
  // restore one isolation scope in exact reverse custody order
  if (frame.scope === "isolation") {
    state.segmentContinues = frame.savedContinues;
    state.functionEnvironment = frame.savedFunctionEnvironment;
    state.environment = frame.savedEnvironment;
    state.isolatedDepth = frame.savedIsolatedDepth;
    state.currentLoop = frame.savedLoop;
    state.currentIf = frame.savedIf;
    state.currentCase = frame.savedCase;
    return;
  }
  // restore one inline function scope
  if (frame.scope === "inline") {
    state.ownerSymbol = frame.savedOwner;
    state.functionEnvironment = frame.savedFunctionEnvironment;
    state.localNameRoot = frame.savedLocalNameRoot;
    state.environment = frame.savedEnvironment;
    state.contexts.length = frame.priorContextCount;
  }
}

// process one logical shell line
function shellProcessLine(
  state,
  lineTokens,
  suppliedPlan = null,
  emissionContext = "simple",
) {
  const heredocs = lineTokens.filter(
    // retain heredoc tokens only
    (token) => token.kind === "heredoc",
  );
  let tokens = lineTokens.filter(
    // remove heredoc bodies from shell grammar
    (token) => token.kind !== "heredoc",
  );
  shellRecordSqlHeredocs(state, tokens, heredocs);
  // retain each unreviewed heredoc as one gap
  for (const ignored of heredocs) {
    void ignored;
    shellPushDiagnostic(state);
  }
  // stop at one empty line
  if (tokens.length === 0) {
    return;
  }
  const planLine = shellPlanLine;
  const plan = suppliedPlan ?? planLine(tokens);
  // process same-line declarations through the shared plan
  if (plan.declarations.length > 0) {
    (0, shellProcessInlineLine)(state, plan, emissionContext);
    return;
  }
  const functionName = shellFunctionSignature(tokens);
  // open one declared function
  if (functionName !== null) {
    (0, shellOpenFunction)(state, tokens, functionName);
    return;
  }
  const firstWord = tokens.find(
    // locate one first word
    (token) => token.kind === "word",
  );
  const first = firstWord === undefined ? null : shellOptionalLiteralWord(firstWord);
  const top = state.contexts.at(-1);
  // close one function or group
  if (first === null && tokens.length === 1 && tokens[0].kind === "operator" && tokens[0].value === "}") {
    // close one active function
    if (top?.type === "function") {
      shellCloseFunction(state, top, tokens[0].end);
    } else {
      shellPushDiagnostic(state);
    }
    return;
  }
  const caseTerminator = tokens.some(
    // locate one case-arm terminator
    (token) => token.kind === "operator" && [";;", ";&", ";;&"].includes(token.value),
  );
  // process one arm boundary before nested compounds
  if (top?.type === "case" && (first === "esac" || top.currentArm === null || caseTerminator)) {
    (0, shellProcessCaseLine)(state, top, tokens, emissionContext);
    return;
  }
  // open one nested or module case control
  if (first === "case") {
    (0, shellOpenCase)(state, tokens);
    return;
  }
  // open one conditional
  if (first === "if") {
    (0, shellOpenIf)(state, tokens, emissionContext);
    return;
  }
  // advance one elif branch
  if (first === "elif" && top?.type === "if") {
    shellNextIfBranch(state, top, false);
    const markerIndex = shellOwnedBodyMarkerIndex(tokens, "then", 1);
    const condition = tokens.slice(1, markerIndex === -1 ? tokens.length : markerIndex);
    // retain actual elif calls and substitutions
    if (condition.length > 0) {
      (0, shellProcessSegments)(state, condition, emissionContext, top.node);
    }
    top.falseEnvironment = state.environment;
    top.falseFunctionEnvironment = state.functionEnvironment;
    state.environment = shellCloneEnvironment(top.falseEnvironment, true);
    state.functionEnvironment = shellCloneEnvironment(
      top.falseFunctionEnvironment,
      true,
    );
    return;
  }
  // advance one exhaustive else branch
  if (first === "else" && top?.type === "if") {
    shellNextIfBranch(state, top, true);
    return;
  }
  // close one conditional
  if (first === "fi" && top?.type === "if") {
    shellCloseIf(state, top, tokens.at(-1).end);
    return;
  }
  // open one loop
  if (["for", "while", "until", "select"].includes(first)) {
    (0, shellOpenLoop)(state, tokens, first, emissionContext);
    return;
  }
  // close one loop
  if (first === "done" && top?.type === "loop") {
    shellCloseLoop(state, top, tokens.at(-1).end);
    return;
  }
  // ignore standalone compound markers
  if (["then", "do"].includes(first)) {
    return;
  }
  const active = state.contexts.at(-1);
  // process one ordinary active case-arm line
  if (active?.type === "case") {
    (0, shellProcessCaseLine)(state, active, tokens, emissionContext);
    return;
  }
  const savedArithmeticContinuation = state.arithmeticContinuation;
  state.arithmeticContinuation = plan.arithmeticContinuation === true;
  // invalidate parent variables before one unclosed arithmetic effect
  if (plan.arithmeticStart === true) {
    shellEnvironmentPromoteTop(state.environment);
  }
  (0, shellProcessSegments)(state, tokens, emissionContext);
  state.arithmeticContinuation = savedArithmeticContinuation;
}

// analyze queued substitutions iteratively
function shellProcessSubstitutions(state) {
  let cursor = 0;
  // consume every discovered substitution exactly once
  while (cursor < state.substitutions.length) {
    const substitution = state.substitutions[cursor];
    cursor += 1;
    const plan = state.lexicon.plans[substitution.planId];
    // retain one malformed substitution gap
    if (plan === undefined || !plan.valid || plan.unsupported) {
      shellPushDiagnostic(state);
      continue;
    }
    const savedArithmeticContinuation = state.arithmeticContinuation;
    const savedCase = state.currentCase;
    const savedContexts = state.contexts;
    const savedEnvironment = state.environment;
    const savedFunctionEnvironment = state.functionEnvironment;
    const savedIf = state.currentIf;
    const savedIsolatedDepth = state.isolatedDepth;
    const savedLocalNameRoot = state.localNameRoot;
    const savedLogicalChanged = state.logicalChanged;
    const savedLogicalFunctionChanged = state.logicalFunctionChanged;
    const savedLoop = state.currentLoop;
    const savedOwner = state.ownerSymbol;
    const savedSegmentContinues = state.segmentContinues;
    const savedTrackAssignments = state.trackAssignments;
    // isolate every execution field while sharing outputs and child queue
    try {
      state.arithmeticContinuation = false;
      state.contexts = substitution.parent === null
        ? []
        : [{ node: substitution.parent, type: "substitution" }];
      state.currentCase = null;
      state.currentIf = null;
      state.currentLoop = null;
      state.environment = shellCloneEnvironment(substitution.environment, true);
      state.functionEnvironment = shellCloneEnvironment(
        substitution.functionEnvironment,
        true,
      );
      state.isolatedDepth = savedIsolatedDepth + 1;
      state.localNameRoot = substitution.localNameRoot;
      state.logicalChanged = null;
      state.logicalFunctionChanged = null;
      state.ownerSymbol = substitution.ownerSymbol;
      state.segmentContinues = true;
      state.trackAssignments = true;
      // execute every immutable logical plan once
      for (const linePlan of plan.linePlans) {
        shellRunSemanticFrames(state, {
          emissionContext: "command-substitution",
          kind: "line",
          plan: linePlan,
        });
      }
      // retain one unclosed isolated compound as a gap
      if (state.contexts.length > (substitution.parent === null ? 0 : 1)) {
        shellPushDiagnostic(state);
      }
    } finally {
      // restore every caller execution field exactly
      state.arithmeticContinuation = savedArithmeticContinuation;
      state.contexts = savedContexts;
      state.currentCase = savedCase;
      state.currentIf = savedIf;
      state.currentLoop = savedLoop;
      state.environment = savedEnvironment;
      state.functionEnvironment = savedFunctionEnvironment;
      state.isolatedDepth = savedIsolatedDepth;
      state.localNameRoot = savedLocalNameRoot;
      state.logicalChanged = savedLogicalChanged;
      state.logicalFunctionChanged = savedLogicalFunctionChanged;
      state.ownerSymbol = savedOwner;
      state.segmentContinues = savedSegmentContinues;
      state.trackAssignments = savedTrackAssignments;
    }
  }
}

// build dependency-ordered shell facts
function shellBuildFacts(state) {
  const factsByOrdinal = new Map();
  // build every fact without recursive descent
  for (const root of state.nodes) {
    // reuse one completed fact
    if (factsByOrdinal.has(root.ordinal)) {
      continue;
    }
    const stack = [{ expanded: false, node: root }];
    // walk one fact dependency graph
    while (stack.length > 0) {
      const entry = stack.pop();
      // finish one dependency-complete node
      if (entry.expanded) {
        const childEdges = [];
        // replace every child node with its private fact id
        for (const child of entry.node.children) {
          const fact = factsByOrdinal.get(child.node.ordinal);
          // reject one missing child fact
          if (fact === undefined) {
            throw bootstrapError();
          }
          childEdges.push({
            childFactId: fact.factId,
            index: child.index,
            role: child.role,
          });
        }
        const fact = syntheticFact({
          associations: [],
          attributes: entry.node.attributes,
          childEdges,
          factKind: entry.node.factKind,
          language: "shell",
          source: state.source,
          span: occurrenceSpan(
            state.spanIndex,
            entry.node.start,
            entry.node.end,
          ),
        });
        factsByOrdinal.set(entry.node.ordinal, fact);
        continue;
      }
      // revisit one parent after its children
      stack.push({ expanded: true, node: entry.node });
      // push missing children in reverse order
      for (let index = entry.node.children.length - 1; index >= 0; index -= 1) {
        const child = entry.node.children[index].node;
        // retain one missing child dependency
        if (!factsByOrdinal.has(child.ordinal)) {
          stack.push({ expanded: false, node: child });
        }
      }
    }
  }
  return state.nodes.map(
    // restore syntax source order
    (node) => factsByOrdinal.get(node.ordinal),
  );
}

// analyze one bounded shell source
function analyzeShell(
  source,
  spanIndex,
  functionProfile = null,
  suppliedPlans = null,
  suppliedLexed = null,
) {
  const lexed = suppliedLexed ?? shellWords(source.sourceText, 0);
  const bindingRegistry = shellCreateBindingRegistry();
  const state = {
    arithmeticContinuation: false,
    contexts: [],
    currentCase: null,
    currentIf: null,
    currentLoop: null,
    derivedSources: [],
    diagnostics: [],
    drafts: [],
    environment: shellCreateEnvironment(bindingRegistry, "variable"),
    functionEnvironment: shellCreateEnvironment(bindingRegistry, "function"),
    functionEffects: functionProfile?.effects ?? new Map(),
    functionProfile,
    functionInventory: new Map(),
    localNameRoot: 0,
    isolatedDepth: 0,
    lexicon: lexed.lexicon,
    logicalChanged: null,
    logicalFunctionChanged: null,
    nodes: [],
    ownerSymbol: "<module>",
    pathResolution: shellCreatePathResolution(lexed.lexicon),
    queuedArithmeticPlanIds: new Uint8Array(lexed.lexicon.plans.length),
    queuedPlanIds: new Uint32Array(lexed.lexicon.plans.length),
    source,
    spanIndex,
    substitutions: [],
    segmentContinues: true,
    trackAssignments: true,
  };
  state.environment.cwdBase = source.resolutionBase;
  // retain one lexer gap
  if (lexed.unsupported) {
    shellPushDiagnostic(state);
  }
  const linePlans = suppliedPlans ?? shellPlanLines(lexed.tokens);
  state.functionEffects = functionProfile?.effects ?? shellSolveFunctionEffects([linePlans]);
  // discover all declarations from shared plans
  for (const plan of linePlans) {
    const multilineName = shellFunctionSignature(plan.grammarTokens);
    const declarations = multilineName === null
      ? plan.declarations
      : [{ name: multilineName, start: 0 }];
    // retain every exact function name
    for (const declaration of declarations) {
      const name = declaration.name;
      // retain one exact declaration
      if (name !== null) {
        const declarations = state.functionInventory.get(name) ?? [];
        declarations.push(plan.grammarTokens[declaration.start]?.start ?? 0);
        state.functionInventory.set(name, declarations);
      }
    }
  }
  // execute every shared line plan through one closed frame driver
  for (const plan of linePlans) {
    shellRunSemanticFrames(state, {
      emissionContext: "simple",
      kind: "line",
      plan,
    });
  }
  // mark every unclosed compound construct
  if (state.contexts.length > 0) {
    shellPushDiagnostic(state);
  }
  (0, shellProcessSubstitutions)(state);
  const semanticFacts = shellBuildFacts(state);
  return {
    derivedSources: state.derivedSources,
    diagnostics: state.diagnostics,
    drafts: state.drafts,
    semanticFacts,
  };
}

// project one frozen physical-v1 occurrence
function projectPhysical(record) {
  const dollar = record.surface === "postgres.dollar-quoted-body";
  return {
    confidence: dollar ? "non-provider-container" : record.confidence,
    container: record.embeddedSql === null
      ? null
      : {
          containerOccurrenceId: null,
          containerSpan: record.embeddedSql.containerPhysicalSpan,
          decodedSpan: record.embeddedSql.decodedRange,
          parentSourceLocator: record.sourcePath,
        },
    countsAsClockCandidate: dollar ? false : record.countsAsClockCandidate,
    duplicateOrdinal: record.duplicateOrdinal,
    extractorOrigin: { record, syntheticSourcePath: null },
    id: record.id,
    identityVersion: "physical-v1",
    kind: dollar ? "non-provider-container" : record.kind,
    language: record.language,
    normalizedExpressionSha256: record.normalizedExpressionSha256,
    ownerSymbol: record.enclosingSymbol,
    parentOccurrenceId: null,
    sourceLocator: record.sourcePath,
    span: record.physicalSpan,
    surface: record.surface,
    syntaxKind: dollar ? "expanded-container" : record.syntaxKind,
  };
}

// locate one complete dollar delimiter
function sqlDollarDelimiterEnd(text, start) {
  // require one dollar opener
  if (text[start] !== "$") {
    return -1;
  }
  let cursor = start + 1;
  // retain one empty tag
  if (text[cursor] === "$") {
    return cursor + 1;
  }
  // require one identifier start
  const first = text.charCodeAt(cursor);
  if (!(text[cursor] === "_" || first >= 65 && first <= 90 || first >= 97 && first <= 122)) {
    return -1;
  }
  cursor += 1;
  // consume one identifier tag
  while (cursor < text.length) {
    const code = text.charCodeAt(cursor);
    // finish one exact tag
    if (text[cursor] === "$") {
      return cursor + 1;
    }
    // reject one nonidentifier tag byte
    if (!(text[cursor] === "_" || code >= 48 && code <= 57 || code >= 65 && code <= 90 || code >= 97 && code <= 122)) {
      return -1;
    }
    cursor += 1;
  }
  return -1;
}

// locate one exact numeric token end
function sqlNumberEnd(text, start) {
  let cursor = start;
  let digits = false;
  // consume one integer prefix
  while (cursor < text.length && text.charCodeAt(cursor) >= 48 && text.charCodeAt(cursor) <= 57) {
    digits = true;
    cursor += 1;
  }
  // consume one nonrange decimal point
  if (text[cursor] === "." && text[cursor + 1] !== ".") {
    cursor += 1;
    // consume every fractional digit
    while (cursor < text.length && text.charCodeAt(cursor) >= 48 && text.charCodeAt(cursor) <= 57) {
      digits = true;
      cursor += 1;
    }
  }
  // reject one absent numeric body
  if (!digits) {
    return -1;
  }
  // consume one complete exponent
  if (text[cursor] === "e" || text[cursor] === "E") {
    let exponent = cursor + 1;
    // consume one exponent sign
    if (text[exponent] === "+" || text[exponent] === "-") {
      exponent += 1;
    }
    const digitStart = exponent;
    // consume every exponent digit
    while (exponent < text.length && text.charCodeAt(exponent) >= 48 && text.charCodeAt(exponent) <= 57) {
      exponent += 1;
    }
    // retain an exponent only when complete
    if (exponent > digitStart) {
      cursor = exponent;
    }
  }
  return cursor;
}

// locate one unquoted identifier end
function sqlIdentifierEnd(text, start) {
  const first = text.charCodeAt(start);
  // require one ascii identifier start
  if (!(text[start] === "_" || first >= 65 && first <= 90 || first >= 97 && first <= 122)) {
    return -1;
  }
  let cursor = start + 1;
  // consume every ascii identifier continuation
  while (cursor < text.length) {
    const code = text.charCodeAt(cursor);
    // stop before one nonidentifier byte
    if (!(text[cursor] === "_" || text[cursor] === "$" || code >= 48 && code <= 57 || code >= 65 && code <= 90 || code >= 97 && code <= 122)) {
      break;
    }
    cursor += 1;
  }
  return cursor;
}

// scan one protected sql fragment for private grammar or selected identity
function scanSqlTokens(text, baseOffset = 0, selectedIdentity = false) {
  const tokens = [];
  const identityMultiSymbols = [
    "!~~*", "->>", "#>>", "!~*", "~~*", "::", ":=", "=>", "->", "#>",
    "<=", ">=", "<>", "!=", "||", "&&", "<<", ">>", "**", "..", "@>",
    "<@", "?&", "?|", "#-", "!~~", "!~", "~~", "~*",
  ];
  const privateMultiSymbols = ["@?", "@@", ...identityMultiSymbols];
  const multiSymbols = selectedIdentity ? identityMultiSymbols : privateMultiSymbols;
  let cursor = 0;

  // materialize one closed quoted atom stream
  function sqlMaterializeQuotedAtoms(atoms) {
    const values = [];
    // decode every retained source run or escape once
    for (const atom of atoms) {
      values.push(atom.value ?? text.slice(atom.start, atom.end));
    }
    return values.join("");
  }

  // scan one quoted sql string
  function sqlScanString(start, escape) {
    let index = start + 1;
    let runStart = index;
    const atoms = [];
    // consume through one closing apostrophe
    while (index < text.length) {
      // consume one escape pair
      if (escape && text[index] === "\\") {
        // reject one terminal escape
        if (index + 1 >= text.length) {
          return null;
        }
        // flush one preceding source run
        if (runStart < index) {
          atoms.push({ end: index, start: runStart, value: null });
        }
        atoms.push({ end: index + 2, start: index, value: text[index + 1] });
        index += 2;
        runStart = index;
        continue;
      }
      // consume one doubled apostrophe
      if (text[index] === "'" && text[index + 1] === "'") {
        // flush one preceding source run
        if (runStart < index) {
          atoms.push({ end: index, start: runStart, value: null });
        }
        atoms.push({ end: index + 2, start: index, value: "'" });
        index += 2;
        runStart = index;
        continue;
      }
      // close one quoted string
      if (text[index] === "'") {
        // flush one final source run
        if (runStart < index) {
          atoms.push({ end: index, start: runStart, value: null });
        }
        return { decoded: sqlMaterializeQuotedAtoms(atoms), end: index + 1 };
      }
      index += 1;
    }
    return null;
  }

  // consume the complete fragment
  while (cursor < text.length) {
    const character = text[cursor];
    // skip admitted whitespace
    if (/[ \t\r\n\f]/u.test(character)) {
      cursor += 1;
      continue;
    }
    // reject other control bytes
    if (character === "\0" || /[\u0001-\u0008\u000B\u000E-\u001F\u007F]/u.test(character)) {
      return null;
    }
    // skip one line comment
    if (text.startsWith("--", cursor)) {
      const newline = text.indexOf("\n", cursor + 2);
      cursor = newline === -1 ? text.length : newline + 1;
      continue;
    }
    // skip one nested block comment
    if (text.startsWith("/*", cursor)) {
      let depth = 1;
      cursor += 2;
      // consume every nested comment level
      while (cursor < text.length && depth > 0) {
        // open one nested comment
        if (text.startsWith("/*", cursor)) {
          depth += 1;
          cursor += 2;
        } else if (text.startsWith("*/", cursor)) {
          // close one nested comment
          depth -= 1;
          cursor += 2;
        } else {
          cursor += 1;
        }
      }
      // reject one unterminated block comment
      if (depth !== 0) {
        return null;
      }
      continue;
    }
    // reject unsupported prefixed literal openers
    if (
      (/[BbXxNn]/u.test(character) && text[cursor + 1] === "'") ||
      (/[Uu]/u.test(character) && text[cursor + 1] === "&" &&
        (text[cursor + 2] === "'" || text[cursor + 2] === "\""))
    ) {
      return null;
    }
    let stringStart = cursor;
    let escape = false;
    // recognize one escape string before identifiers
    if ((character === "E" || character === "e") && text[cursor + 1] === "'") {
      escape = true;
      stringStart = cursor + 1;
    }
    // scan one ordinary or escape string
    if (text[stringStart] === "'") {
      const scanned = sqlScanString(stringStart, escape);
      // reject one malformed string
      if (scanned === null) {
        return null;
      }
      tokens.push({
        end: baseOffset + scanned.end,
        start: baseOffset + cursor,
        type: escape ? "escape-string" : "string",
        value: scanned.decoded,
      });
      cursor = scanned.end;
      continue;
    }
    // scan one quoted identifier
    if (character === "\"") {
      let index = cursor + 1;
      let runStart = index;
      const atoms = [];
      // consume the quoted identifier
      while (index < text.length) {
        // consume one doubled quote
        if (text[index] === "\"" && text[index + 1] === "\"") {
          // flush one preceding identifier run
          if (runStart < index) {
            atoms.push({ end: index, start: runStart, value: null });
          }
          atoms.push({ end: index + 2, start: index, value: "\"" });
          index += 2;
          runStart = index;
          continue;
        }
        // close one quoted identifier
        if (text[index] === "\"") {
          break;
        }
        index += 1;
      }
      // materialize one closed identifier only
      if (index < text.length && runStart < index) {
        atoms.push({ end: index, start: runStart, value: null });
      }
      const decoded = index < text.length ? sqlMaterializeQuotedAtoms(atoms) : "";
      // reject one malformed quoted identifier
      if (index >= text.length || decoded.length === 0) {
        return null;
      }
      tokens.push({
        end: baseOffset + index + 1,
        start: baseOffset + cursor,
        type: "quoted-identifier",
        value: decoded,
      });
      cursor = index + 1;
      continue;
    }
    // scan one exact dollar body
    if (character === "$") {
      const delimiterEnd = sqlDollarDelimiterEnd(text, cursor);
      // reject one positional or malformed dollar token
      if (delimiterEnd < 0) {
        return null;
      }
      const delimiter = text.slice(cursor, delimiterEnd);
      const closing = text.indexOf(delimiter, cursor + delimiter.length);
      // reject one unterminated dollar body
      if (closing === -1) {
        return null;
      }
      tokens.push({
        bodyEnd: baseOffset + closing,
        bodyStart: baseOffset + cursor + delimiter.length,
        delimiter,
        end: baseOffset + closing + delimiter.length,
        start: baseOffset + cursor,
        type: "dollar-body",
        value: null,
      });
      cursor = closing + delimiter.length;
      continue;
    }
    const numberEnd = sqlNumberEnd(text, cursor);
    // scan one exact numeric literal
    if (numberEnd >= 0) {
      const next = text[numberEnd] ?? "";
      // reject an adjacent identifier or a second decimal point
      if (/[A-Za-z_$]/u.test(next) || next === "." && !text.startsWith("..", numberEnd)) {
        return null;
      }
      tokens.push({
        end: baseOffset + numberEnd,
        start: baseOffset + cursor,
        type: "number",
        value: null,
      });
      cursor = numberEnd;
      continue;
    }
    const identifierEnd = sqlIdentifierEnd(text, cursor);
    // scan one unquoted identifier
    if (identifierEnd >= 0) {
      const identifier = text.slice(cursor, identifierEnd);
      tokens.push({
        end: baseOffset + identifierEnd,
        start: baseOffset + cursor,
        type: "identifier",
        value: identifier.toLowerCase(),
      });
      cursor = identifierEnd;
      continue;
    }
    const symbol = multiSymbols.find(
      // locate one longest exact symbol
      (candidate) => text.startsWith(candidate, cursor),
    ) ?? (
      selectedIdentity
        ? (/^[()\[\],.;:+\-*/%^=<>~]/u.test(character) ? character : null)
        : (/^[()\[\],.;:+\-*/%^=<>~?]/u.test(character) ? character : null)
    );
    // reject one unknown lexical byte
    if (symbol === null) {
      return null;
    }
    // reject one named parameter opener
    if (symbol === ":" && /[A-Za-z_0-9]/u.test(text[cursor + 1] ?? "")) {
      return null;
    }
    tokens.push({
      end: baseOffset + cursor + symbol.length,
      start: baseOffset + cursor,
      type: "symbol",
      value: symbol,
    });
    cursor += symbol.length;
  }
  return tokens;
}

// tokenize one selected provider with the frozen identity vocabulary
function sqlSelectedTokens(text, baseOffset = 0) {
  return scanSqlTokens(text, baseOffset, true);
}

// test one unquoted keyword token
function sqlKeyword(token, value) {
  return token?.type === "identifier" && token.value === value;
}

// recognize one exact structural symbol
function sqlSymbol(token, value) {
  return token?.type === "symbol" && token.value === value;
}

// create one private sql semantic node
function sqlNode(factKind, start, end, identifier = null) {
  return {
    attributes: {
      identifier,
      importLevel: null,
      literalKind: null,
      literalRole: "none",
      literalValue: null,
      operators: [],
    },
    childIndexes: new Map(),
    children: [],
    end,
    factKind,
    start,
  };
}

// append one indexed semantic child
function sqlAddChild(parent, role, child) {
  const index = parent.childIndexes.get(role) ?? 0;
  parent.childIndexes.set(role, index + 1);
  parent.children.push({ child, index, role });
}

// normalize one provider token without literal payloads
function sqlNormalizedToken(token) {
  // preserve one unquoted identifier
  if (token.type === "identifier") {
    return ["identifier", token.value];
  }
  // preserve one quoted identifier
  if (token.type === "quoted-identifier") {
    return ["quoted-identifier", token.value];
  }
  // redact one literal family
  if (["string", "escape-string", "number", "dollar-body"].includes(token.type)) {
    return ["literal", token.type];
  }
  return ["symbol", token.value];
}
// build one immutable direct-clause owner index
function sqlBuildDirectClauseIndex(tokens, start, end, ownerByIndex, closeByOpen) {
  const positionsByOwner = new Map();
  // partition every token by its existing query owner
  for (let index = start; index < end; index += 1) {
    const owner = ownerByIndex.get(index) ?? null;
    const positions = positionsByOwner.get(owner) ?? [];
    positions.push(index);
    positionsByOwner.set(owner, positions);
  }
  const byOwner = new Map();
  // build one immutable CASE forest per query owner
  for (const [owner, positions] of positionsByOwner) {
    const rootByFamily = { body: [], conflict: [], returning: [] };
    const cases = [];
    const caseStack = [];
    let skipThrough = -1;
    // charge every direct owner token once
    for (const index of positions) {
      // skip one already-indexed delimiter body
      if (index <= skipThrough) {
        continue;
      }
      const close = closeByOpen.get(index);
      // skip one grouped or nested-query range
      if (close !== undefined) {
        skipThrough = close;
        continue;
      }
      const token = tokens[index];
      // open one lexical CASE node
      if (sqlKeyword(token, "case")) {
        const parent = caseStack.at(-1) ?? null;
        const node = {
          close: -1,
          id: cases.length,
          itemsByFamily: { body: [], conflict: [], returning: [] },
          open: index,
          parent,
        };
        cases.push(node);
        const targets = parent?.itemsByFamily ?? rootByFamily;
        // retain one placeholder per direct clause family
        for (const family of ["body", "conflict", "returning"]) {
          targets[family].push({ caseId: node.id, kind: "placeholder", position: index });
        }
        caseStack.push(node);
        continue;
      }
      // close one lexical CASE node
      if (sqlKeyword(token, "end") && caseStack.length > 0) {
        const node = caseStack.pop();
        node.close = index;
        continue;
      }
      const targets = caseStack.at(-1)?.itemsByFamily ?? rootByFamily;
      // retain one direct body marker
      if (token?.type === "identifier" &&
        ["values", "select", "with", "default"].includes(token.value)) {
        targets.body.push({ family: "body", kind: "row", position: index });
      }
      // retain one direct ON CONFLICT marker
      if (sqlKeyword(token, "on") && sqlKeyword(tokens[index + 1], "conflict")) {
        targets.conflict.push({ family: "conflict", kind: "row", position: index });
      }
      // retain one direct RETURNING marker
      if (sqlKeyword(token, "returning")) {
        targets.returning.push({ family: "returning", kind: "row", position: index });
      }
    }
    byOwner.set(owner, { cases, rootByFamily });
  }
  return { byOwner };
}

// intern one production range without serialized keys
function sqlInternProductionRange(attempt, start, end) {
  let byEnd = attempt.rangeIds.get(start);
  // create one start bucket
  if (byEnd === undefined) {
    byEnd = new Map();
    attempt.rangeIds.set(start, byEnd);
  }
  const existing = byEnd.get(end);
  // reuse one exact range id
  if (existing !== undefined) {
    return existing;
  }
  const rangeId = attempt.ranges.length;
  attempt.ranges.push({ end, start });
  byEnd.set(end, rangeId);
  return rangeId;
}

// create one private static validation attempt
function sqlCreateStaticAttempt(structure, procedural) {
  return {
    captures: [],
    insertSummaries: new Map(),
    insertWindowByOwner: new Map(),
    memo: new Map(),
    owner: null,
    procedural,
    rangeIds: new Map(),
    ranges: [],
    structure,
    validByOpen: new Map(),
  };
}

// index delimiter ownership and direct query owners once
function sqlParseIndex(tokens, start, end) {
  const closeByOpen = new Map();
  const nonCallOpens = new Set();
  const ownerByIndex = new Map();
  const queryByOpen = new Map();
  const queryRanges = [];
  const delimiterStack = [];
  const queryStack = [];
  const queryStarts = new Set(["select", "with", "insert", "update", "delete"]);
  // charge every token to one direct query owner
  for (let index = start; index < end; index += 1) {
    const token = tokens[index];
    const owner = queryStack.at(-1)?.open ?? null;
    ownerByIndex.set(index, owner);
    // open one owned delimiter
    if (sqlSymbol(token, "(") || sqlSymbol(token, "[")) {
      const next = index + 1 < end ? tokens[index + 1] : undefined;
      const startsQuery = sqlSymbol(token, "(") &&
        next?.type === "identifier" && queryStarts.has(next.value);
      const query = startsQuery
        ? { children: [], end: -1, open: index, parent: owner, start: index + 1 }
        : null;
      // link one nested query to its direct query owner
      if (query !== null) {
        const parent = queryStack.at(-1);
        // retain one direct query child
        if (parent !== undefined) {
          parent.children.push(query.open);
        }
        queryByOpen.set(index, query);
        queryStack.push(query);
      }
      delimiterStack.push({ index, query, value: token.value });
      continue;
    }
    // close one owned delimiter
    if (sqlSymbol(token, ")") || sqlSymbol(token, "]")) {
      const opener = delimiterStack.pop();
      // reject one mismatched delimiter
      if (
        opener === undefined ||
        opener.value === "(" && !sqlSymbol(token, ")") ||
        opener.value === "[" && !sqlSymbol(token, "]")
      ) {
        return null;
      }
      closeByOpen.set(opener.index, index);
      // finalize one query after every child range
      if (opener.query !== null) {
        opener.query.end = index;
        queryRanges.push(opener.query);
        queryStack.pop();
      }
    }
  }
  // reject one unclosed delimiter
  if (delimiterStack.length !== 0 || queryStack.length !== 0) {
    return null;
  }
  const directClauseIndex = sqlBuildDirectClauseIndex(
    tokens,
    start,
    end,
    ownerByIndex,
    closeByOpen,
  );
  return {
    closeByOpen,
    directClauseIndex,
    nonCallOpens,
    ownerByIndex,
    queryByOpen,
    queryRanges,
  };
}

// skip one directly owned child query range
function sqlOwnedJump(structure, owner, index) {
  const query = structure?.queryByOpen.get(index);
  return query !== undefined && query.parent === owner ? query.end : index;
}

// validate indexed query ranges from inner to outer
function sqlValidateQueryRanges(tokens, structure, attempt = null) {
  const activeAttempt = attempt ?? sqlCreateStaticAttempt(structure, false);
  const validByOpen = activeAttempt.validByOpen;
  let allValid = true;
  // validate each range once in close order
  for (const range of structure.queryRanges) {
    const childrenValid = range.children.every(
      // require one already-validated direct query child
      (open) => validByOpen.get(open) === true,
    );
    activeAttempt.owner = range.open;
    activeAttempt.procedural = false;
    const valid = childrenValid && sqlRunStaticValidationFrames(
      tokens,
      range.start,
      range.end,
      activeAttempt,
      "STATIC_RANGE",
      range.open,
      false,
    );
    validByOpen.set(range.open, valid);
    // classify one successfully validated query wrapper
    if (valid) {
      structure.nonCallOpens.add(range.open);
    }
    // retain one aggregate validation status
    if (!valid) {
      allValid = false;
    }
  }
  return { allValid, validByOpen };
}

// parse one exact cast type reference without recursion
function sqlCastTypeReference(tokens, start, end, structure) {
  const first = sqlBoundedToken(tokens, start, end);
  // require one leading type name
  if (!sqlNameToken(first)) {
    return null;
  }
  let cursor = start + 1;
  let qualified = false;
  const nonCallOpens = [];
  // consume one qualified type-name path
  while (sqlSymbol(sqlBoundedToken(tokens, cursor, end), ".")) {
    // require one exact name after every qualifier
    if (!sqlNameToken(sqlBoundedToken(tokens, cursor + 1, end))) {
      return null;
    }
    qualified = true;
    cursor += 2;
  }
  // consume one admitted multiword built-in type
  if (!qualified && first.type === "identifier") {
    const phrases = new Map([
      ["bit", ["varying"]],
      ["character", ["varying"]],
      ["double", ["precision"]],
    ]);
    const continuation = phrases.get(first.value);
    // consume one exact optional continuation
    if (
      continuation !== undefined &&
      continuation.includes(sqlBoundedToken(tokens, cursor, end)?.value)
    ) {
      cursor += 1;
    } else if (
      first.value === "national" &&
      sqlKeyword(sqlBoundedToken(tokens, cursor, end), "character")
    ) {
      cursor += 1;
      // consume one optional VARYING suffix
      if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "varying")) {
        cursor += 1;
      }
    }
  }
  // consume one numeric type-modifier list
  if (sqlSymbol(sqlBoundedToken(tokens, cursor, end), "(")) {
    const open = cursor;
    const close = structure.closeByOpen.get(open);
    // require one bounded nonempty modifier group
    if (close === undefined || close >= end || close === open + 1) {
      return null;
    }
    const ranges = sqlCommaRanges(tokens, open + 1, close, structure.closeByOpen);
    // require every exact signed or unsigned numeric modifier
    if (ranges.some(
      // reject one nonnumeric modifier slot
      ([oneStart, oneEnd]) => !(
        oneEnd === oneStart + 1 && tokens[oneStart]?.type === "number" ||
        oneEnd === oneStart + 2 &&
          (sqlSymbol(tokens[oneStart], "+") || sqlSymbol(tokens[oneStart], "-")) &&
          tokens[oneStart + 1]?.type === "number"
      ),
    )) {
      return null;
    }
    nonCallOpens.push(open);
    cursor = close + 1;
  }
  // consume one exact time-zone type suffix
  if (
    !qualified && first.type === "identifier" &&
    (first.value === "time" || first.value === "timestamp") &&
    (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "with") ||
      sqlKeyword(sqlBoundedToken(tokens, cursor, end), "without"))
  ) {
    // require one complete WITH/WITHOUT TIME ZONE phrase
    if (
      !sqlKeyword(sqlBoundedToken(tokens, cursor + 1, end), "time") ||
      !sqlKeyword(sqlBoundedToken(tokens, cursor + 2, end), "zone")
    ) {
      return null;
    }
    cursor += 3;
  }
  // consume every exact empty array suffix
  while (sqlSymbol(sqlBoundedToken(tokens, cursor, end), "[")) {
    const close = structure.closeByOpen.get(cursor);
    // require one empty bounded array suffix
    if (close !== cursor + 1 || close >= end) {
      return null;
    }
    cursor = close + 1;
  }
  return { end: cursor, nonCallOpens };
}

// recognize one unquoted single-part syntax introducer
function sqlSpecialName(base, word) {
  return base?.kind === "name" && base.parts === 1 &&
    base.unquoted === true && base.word === word;
}

// validate one complete private sql expression
function sqlStrictExpression(tokens, start, end, providedState = null) {
  const indexedState = providedState?.structure === undefined
    ? null
    : providedState;
  const structure = indexedState?.structure ?? providedState ?? sqlParseIndex(tokens, start, end);
  // reject malformed delimiter ownership
  if (structure === null || start >= end) {
    return false;
  }
  const nested = indexedState ?? sqlValidateQueryRanges(tokens, structure);
  // reject one malformed nested query before expression admission
  if (indexedState === null && structure.queryRanges.length > 0 && !nested.allValid) {
    return false;
  }
  const binaryKeywords = new Set([
    "and", "at", "between", "collate", "from", "ilike", "in", "is", "like",
    "or", "similar",
  ]);
  const typedBridges = new Set([
    "bit", "date", "double", "interval", "json", "jsonb", "national", "precision",
    "time", "timestamp", "timestamptz", "varbit",
  ]);
  const binarySymbols = new Set([
    "+", "-", "*", "/", "%", "^", "=", "<", ">", "~", "=>",
    "<=", ">=", "<>", "!=", "||", "&&", "<<", ">>", "**", "@>", "<@",
    "?", "?&", "?|", "->", "->>", "#>", "#>>", "#-", "!~", "!~*", "~~", "~~*", "!~~",
    "!~~*", "~*", "@?", "@@",
  ]);
  const comparisonSymbols = new Set(["=", "<", ">", "<=", ">=", "<>", "!="]);
  const frames = [];
  const caseStack = [];
  // stage syntax-owned groups until validation succeeds
  const nonCallOpens = new Set();
  // retain current base-expression ownership
  let base = null;
  let expectOperand = true;
  let expectMember = false;
  let keywordContinuation = null;
  // retain one pending dotted-member owner
  let memberBase = null;
  // retain one exact right-operand authority
  let operandContext = "ordinary";
  // retain one construct awaiting its opener
  let pendingConstruct = null;
  let previousToken = null;
  // validate every positioned expression token once
  for (let index = start; index < end; index += 1) {
    const token = tokens[index];
    const query = structure.queryByOpen.get(index);
    // consume one already-validated nested query as an atom
    if (query !== undefined) {
      const prefixQuery = expectOperand && base === null;
      const quantifierQuery = pendingConstruct === "quantifier";
      const arrayQuery = sqlSpecialName(base, "array");
      // require one valid indexed query range at an operand boundary
      if (
        nested.validByOpen.get(index) !== true || expectMember ||
        keywordContinuation !== null ||
        !(prefixQuery || quantifierQuery || arrayQuery)
      ) {
        return false;
      }
      // classify one validated subquery wrapper
      nonCallOpens.add(index);
      base = { kind: "value" };
      expectOperand = false;
      keywordContinuation = null;
      operandContext = "ordinary";
      pendingConstruct = null;
      previousToken = tokens[query.end];
      index = query.end;
      continue;
    }
    // open one sql case expression at an operand boundary
    if (sqlKeyword(token, "case")) {
      // reject one case after a complete atom
      if (!expectOperand || expectMember || keywordContinuation !== null) {
        return false;
      }
      caseStack.push({ phase: "selector", sawElse: false, sawWhen: false });
      base = null;
      expectOperand = true;
      operandContext = "ordinary";
      previousToken = token;
      continue;
    }
    // open one case arm after an optional selector or prior result
    if (sqlKeyword(token, "when") && caseStack.length > 0) {
      const frame = caseStack.at(-1);
      // require one complete prior arm and forbid arms after else
      if (expectMember || frame.sawElse || frame.sawWhen && (frame.phase !== "then" || expectOperand)) {
        return false;
      }
      frame.phase = "when";
      frame.sawWhen = true;
      base = null;
      expectOperand = true;
      operandContext = "ordinary";
      previousToken = token;
      continue;
    }
    // separate one case result
    if (sqlKeyword(token, "then") && caseStack.length > 0) {
      const frame = caseStack.at(-1);
      // require one complete current arm condition
      if (frame.phase !== "when" || expectOperand || expectMember) {
        return false;
      }
      frame.phase = "then";
      base = null;
      expectOperand = true;
      operandContext = "ordinary";
      previousToken = token;
      continue;
    }
    // separate one unique case fallback
    if (sqlKeyword(token, "else") && caseStack.length > 0) {
      const frame = caseStack.at(-1);
      // require one complete arm before one fallback
      if (frame.sawElse || frame.phase !== "then" || expectOperand || expectMember) {
        return false;
      }
      frame.phase = "else";
      frame.sawElse = true;
      base = null;
      expectOperand = true;
      operandContext = "ordinary";
      previousToken = token;
      continue;
    }
    // close one complete case expression as an atom
    if (sqlKeyword(token, "end") && caseStack.length > 0) {
      const frame = caseStack.pop();
      // require one complete current result
      if (!frame.sawWhen || expectOperand || expectMember || base === null || frame.phase !== "then" && frame.phase !== "else") {
        return false;
      }
      base = { kind: "value" };
      expectOperand = false;
      previousToken = token;
      continue;
    }
    // reject one orphan case delimiter keyword
    if (caseStack.length === 0 && (sqlKeyword(token, "when") || sqlKeyword(token, "then") || sqlKeyword(token, "else") || sqlKeyword(token, "end"))) {
      return false;
    }
    // open one ordinary expression or postfix group
    if (sqlSymbol(token, "(") || sqlSymbol(token, "[")) {
      // reject one group after a half-spelled keyword operator
      if (keywordContinuation !== null) {
        return false;
      }
      const postfix = !expectOperand && !expectMember;
      let kind = null;
      // resolve one exact opener owner
      if (sqlSymbol(token, "[")) {
        kind = sqlSpecialName(base, "array")
          ? "array"
          : postfix ? "subscript" : "group";
      } else if (pendingConstruct !== null) {
        kind = pendingConstruct;
      } else if (!postfix) {
        kind = "group";
      } else if (base?.kind === "provider") {
        // admit precision only for time-like keyword providers
        if (base.word === "current_date") {
          return false;
        }
        kind = "provider-precision";
      } else if (base?.kind === "name") {
        kind = sqlSpecialName(base, "cast")
          ? "cast"
          : sqlSpecialName(base, "row") ? "row" : "call";
      } else {
        return false;
      }
      // stage one ordinary prefix group immediately
      if (sqlSymbol(token, "(") && !postfix) {
        nonCallOpens.add(index);
      }
      frames.push({
        allowEmpty: ["array", "call", "row"].includes(kind),
        caseDepth: caseStack.length,
        hadValue: false,
        kind,
        open: index,
        opener: token.value,
        phase: kind === "filter" ? "where" : kind === "over" ? "start" : "value",
      });
      base = null;
      expectOperand = true;
      expectMember = false;
      operandContext = "ordinary";
      pendingConstruct = null;
      previousToken = token;
      continue;
    }
    // close one ordinary expression or postfix group
    if (sqlSymbol(token, ")") || sqlSymbol(token, "]")) {
      const frame = frames.pop();
      // require one matching complete or admitted empty group
      if (
        frame === undefined || frame.caseDepth !== caseStack.length ||
        frame.opener === "(" && !sqlSymbol(token, ")") ||
        frame.opener === "[" && !sqlSymbol(token, "]") || expectMember ||
        !expectOperand && base === null ||
        expectOperand && !(
          frame.kind === "over" && frame.phase === "start" ||
          frame.allowEmpty && !frame.hadValue
        ) ||
        frame.kind === "cast" && frame.phase !== "type" ||
        frame.kind === "filter" && frame.phase !== "predicate" ||
        frame.kind === "over" && !["start", "partition", "order"].includes(frame.phase) ||
        frame.kind === "provider-precision" && !(
          index === frame.open + 2 && tokens[frame.open + 1]?.type === "number"
        )
      ) {
        return false;
      }
      // commit one construct-specific syntax role after exact closure
      if (["cast", "filter", "over", "quantifier", "row"].includes(frame.kind)) {
        nonCallOpens.add(frame.open);
      }
      // restore one exact outer base descriptor
      base = frame.kind === "call"
        ? { kind: "call", phase: "plain" }
        : frame.kind === "filter"
          ? { kind: "call", phase: "filtered" }
          : frame.kind === "over"
            ? { kind: "call", phase: "windowed" }
            : { kind: "value" };
      expectOperand = false;
      expectMember = false;
      keywordContinuation = null;
      operandContext = "ordinary";
      pendingConstruct = null;
      previousToken = token;
      continue;
    }
    // separate one positioned group member
    if (sqlSymbol(token, ",")) {
      const frame = frames.at(-1);
      // reject one top-level or empty group member
      if (
        frame === undefined || expectOperand || expectMember || base === null ||
        ["cast", "filter", "provider-precision", "quantifier"].includes(frame.kind)
      ) {
        return false;
      }
      frame.hadValue = true;
      base = null;
      expectOperand = true;
      keywordContinuation = null;
      operandContext = "ordinary";
      pendingConstruct = null;
      previousToken = token;
      continue;
    }
    // consume one qualified member separator
    if (sqlSymbol(token, ".")) {
      // require one complete qualifier
      if (expectOperand || expectMember || base === null) {
        return false;
      }
      memberBase = base;
      base = null;
      expectOperand = true;
      expectMember = true;
      keywordContinuation = null;
      previousToken = token;
      continue;
    }
    // consume one symbolic operator
    if (token.type === "symbol") {
      // admit one qualified wildcard member
      if (sqlSymbol(token, "*") && expectMember) {
        base = { kind: "value" };
        expectOperand = false;
        expectMember = false;
        memberBase = null;
        keywordContinuation = null;
        previousToken = token;
        continue;
      }
      // admit one call wildcard atom only
      if (sqlSymbol(token, "*") && expectOperand && frames.length > 0 && sqlSymbol(tokens[index + 1], frames.at(-1).opener === "(" ? ")" : "]")) {
        base = { kind: "value" };
        expectOperand = false;
        keywordContinuation = null;
        previousToken = token;
        continue;
      }
      // admit one unary sign or complement
      if (expectOperand && ["+", "-", "~"].includes(token.value)) {
        base = null;
        operandContext = "ordinary";
        previousToken = token;
        continue;
      }
      // consume one exact postfix cast type
      if (sqlSymbol(token, "::")) {
        // require one complete value before the cast
        if (expectOperand || expectMember || base === null) {
          return false;
        }
        const type = sqlCastTypeReference(tokens, index + 1, end, structure);
        // require one complete type reference
        if (type === null) {
          return false;
        }
        // stage every validated type-modifier group
        for (const open of type.nonCallOpens) {
          nonCallOpens.add(open);
        }
        index = type.end - 1;
        base = { kind: "value" };
        expectOperand = false;
        keywordContinuation = null;
        previousToken = tokens[index];
        continue;
      }
      // require one binary operator after a complete atom
      if (expectOperand || expectMember || base === null || !binarySymbols.has(token.value)) {
        return false;
      }
      base = null;
      expectOperand = true;
      keywordContinuation = null;
      operandContext = comparisonSymbols.has(token.value) ? "comparison" : "ordinary";
      pendingConstruct = null;
      previousToken = token;
      continue;
    }
    // consume one exact member name
    if (expectMember) {
      // require one identifier member
      if (!["identifier", "quoted-identifier"].includes(token.type)) {
        return false;
      }
      base = memberBase?.kind === "name"
        ? {
          kind: "name",
          parts: memberBase.parts + 1,
          unquoted: memberBase.unquoted && token.type === "identifier",
          word: token.value,
        }
        : { kind: "value" };
      expectOperand = false;
      expectMember = false;
      memberBase = null;
      keywordContinuation = null;
      previousToken = token;
      continue;
    }
    // consume one typed literal suffix
    if (!expectOperand && ["string", "escape-string"].includes(token.type) && previousToken?.type === "identifier" && typedBridges.has(previousToken.value)) {
      base = { kind: "value" };
      previousToken = token;
      continue;
    }
    // continue one exact multiword keyword operator
    if (expectOperand && keywordContinuation !== null && token.type === "identifier") {
      let nextContinuation = undefined;
      // recognize one positioned continuation word
      if (keywordContinuation === "not" && ["between", "ilike", "in", "like"].includes(token.value)) {
        nextContinuation = null;
      } else if (keywordContinuation === "not" && token.value === "similar") {
        nextContinuation = "similar";
      } else if (keywordContinuation === "is" && token.value === "not") {
        nextContinuation = "is-not";
      } else if (
        (keywordContinuation === "is" || keywordContinuation === "is-not") &&
        token.value === "distinct"
      ) {
        nextContinuation = `${keywordContinuation}-distinct`;
      } else if (
        (keywordContinuation === "is-distinct" || keywordContinuation === "is-not-distinct") &&
        token.value === "from"
      ) {
        nextContinuation = null;
      } else if (keywordContinuation === "at" && token.value === "time") {
        nextContinuation = "at-time";
      } else if (keywordContinuation === "at-time" && token.value === "zone") {
        nextContinuation = null;
      } else if (keywordContinuation === "similar" && token.value === "to") {
        nextContinuation = null;
      } else if (
        (keywordContinuation === "order" || keywordContinuation === "partition") &&
        token.value === "by"
      ) {
        nextContinuation = null;
      }
      // retain one complete continuation transition
      if (nextContinuation !== undefined) {
        base = null;
        keywordContinuation = nextContinuation;
        previousToken = token;
        continue;
      }
    }
    // consume one unary NOT or EXISTS prefix
    if (
      token.type === "identifier" && expectOperand && keywordContinuation === null &&
      (token.value === "not" || token.value === "exists")
    ) {
      base = null;
      operandContext = "ordinary";
      previousToken = token;
      continue;
    }
    // consume one comparison-quantifier syntax atom
    if (
      token.type === "identifier" && expectOperand && keywordContinuation === null &&
      ["all", "any", "some"].includes(token.value)
    ) {
      // require one comparison operator to own the quantifier
      if (operandContext !== "comparison" || !sqlSymbol(tokens[index + 1], "(")) {
        return false;
      }
      base = { kind: "value" };
      expectOperand = false;
      pendingConstruct = "quantifier";
      previousToken = token;
      continue;
    }
    // consume one cast AS separator only inside CAST
    if (sqlKeyword(token, "as")) {
      const frame = frames.at(-1);
      // require one complete CAST operand
      if (
        frame?.kind !== "cast" || frame.phase !== "value" ||
        expectOperand || expectMember || base === null || keywordContinuation !== null
      ) {
        return false;
      }
      const close = structure.closeByOpen.get(frame.open);
      const type = close === undefined
        ? null
        : sqlCastTypeReference(tokens, index + 1, close, structure);
      // require the type to consume the complete CAST tail
      if (type === null || type.end !== close) {
        return false;
      }
      // stage every validated type-modifier group
      for (const open of type.nonCallOpens) {
        nonCallOpens.add(open);
      }
      frame.phase = "type";
      index = type.end - 1;
      base = { kind: "value" };
      expectOperand = false;
      keywordContinuation = null;
      previousToken = tokens[index];
      continue;
    }
    const activeFrame = frames.at(-1);
    // consume one required FILTER predicate introducer
    if (
      activeFrame?.kind === "filter" && activeFrame.phase === "where"
    ) {
      // require WHERE as the first direct FILTER token
      if (!sqlKeyword(token, "where") || !expectOperand || keywordContinuation !== null) {
        return false;
      }
      activeFrame.phase = "predicate";
      base = null;
      operandContext = "ordinary";
      previousToken = token;
      continue;
    }
    // consume one ordered OVER clause introducer
    if (
      activeFrame?.kind === "over" &&
      (sqlKeyword(token, "partition") || sqlKeyword(token, "order"))
    ) {
      const partition = sqlKeyword(token, "partition");
      // require ordered unique PARTITION then ORDER clauses
      if (
        !sqlKeyword(tokens[index + 1], "by") ||
        partition && activeFrame.phase !== "start" ||
        !partition && !(
          activeFrame.phase === "start" ||
          activeFrame.phase === "partition" && !expectOperand && !expectMember && base !== null
        )
      ) {
        return false;
      }
      activeFrame.phase = partition ? "partition" : "order";
      base = null;
      expectOperand = true;
      keywordContinuation = null;
      operandContext = "ordinary";
      index += 1;
      previousToken = tokens[index];
      continue;
    }
    // reject non-clause content before one admitted OVER clause
    if (activeFrame?.kind === "over" && activeFrame.phase === "start") {
      return false;
    }
    // consume one aggregate ORDER BY separator inside a group
    if (sqlKeyword(token, "order")) {
      // require one complete aggregate input
      if (frames.at(-1)?.kind !== "call" || expectOperand || base === null || keywordContinuation !== null) {
        return false;
      }
      base = null;
      expectOperand = true;
      keywordContinuation = "order";
      operandContext = "ordinary";
      previousToken = token;
      continue;
    }
    // consume one binary keyword operator
    if (token.type === "identifier" && binaryKeywords.has(token.value)) {
      // require one complete left operand
      if (expectOperand || expectMember || base === null || keywordContinuation !== null) {
        return false;
      }
      base = null;
      expectOperand = true;
      keywordContinuation = ["at", "is", "similar"].includes(token.value)
        ? token.value
        : null;
      operandContext = "ordinary";
      pendingConstruct = null;
      previousToken = token;
      continue;
    }
    // consume one positioned NOT composite-operator prefix
    if (sqlKeyword(token, "not") && !expectOperand && !expectMember && base !== null) {
      base = null;
      expectOperand = true;
      keywordContinuation = "not";
      operandContext = "ordinary";
      pendingConstruct = null;
      previousToken = token;
      continue;
    }
    // consume one postfix FILTER or OVER construct name
    if (token.type === "identifier" && !expectOperand && (token.value === "filter" || token.value === "over") && sqlSymbol(tokens[index + 1], "(")) {
      // require one ordered call-result postfix chain
      if (
        base?.kind !== "call" ||
        token.value === "filter" && base.phase !== "plain" ||
        token.value === "over" && !["plain", "filtered"].includes(base.phase)
      ) {
        return false;
      }
      pendingConstruct = token.value;
      previousToken = token;
      continue;
    }
    // require one atom only at an operand boundary
    if (expectOperand) {
      // admit direct IS/IS NOT test operands but no half-spelled operator phrase
      if (
        keywordContinuation !== null && keywordContinuation !== "is" &&
        keywordContinuation !== "is-not"
      ) {
        return false;
      }
      const provider = token.type === "identifier"
        ? sqlKeywordSurface(token.value)
        : null;
      const keywordValue = token.type === "identifier" &&
        ["false", "null", "true"].includes(token.value);
      base = provider !== null
        ? { kind: "provider", word: token.value }
        : !keywordValue && ["identifier", "quoted-identifier"].includes(token.type)
          ? {
            kind: "name",
            parts: 1,
            unquoted: token.type === "identifier",
            word: token.value,
          }
          : { kind: "value" };
      expectOperand = false;
      keywordContinuation = null;
      operandContext = "ordinary";
      pendingConstruct = null;
      previousToken = token;
      continue;
    }
    return false;
  }
  const valid = base !== null && !expectOperand && !expectMember &&
    memberBase === null && pendingConstruct === null &&
    frames.length === 0 && caseStack.length === 0;
  // commit grammar roles only after the complete expression validates
  if (valid) {
    for (const open of nonCallOpens) {
      structure.nonCallOpens.add(open);
    }
  }
  return valid;
}

// split one comma-delimited token range
function sqlCommaRanges(tokens, start, end, pairs) {
  const ranges = [];
  let rangeStart = start;
  // scan one top-level comma list
  for (let index = start; index < end; index += 1) {
    // skip one balanced group
    if (pairs.has(index)) {
      index = pairs.get(index);
      continue;
    }
    // split at one top-level comma
    if (sqlSymbol(tokens[index], ",")) {
      ranges.push([rangeStart, index]);
      rangeStart = index + 1;
    }
  }
  ranges.push([rangeStart, end]);
  return ranges;
}

// resolve one frozen sql keyword surface
function sqlKeywordSurface(value) {
  switch (value) {
    case "current_date": return "postgres.current-date";
    case "current_time": return "postgres.current-time";
    case "current_timestamp": return "postgres.current-timestamp";
    case "localtime": return "postgres.localtime";
    case "localtimestamp": return "postgres.localtimestamp";
    default: return null;
  }
}

// resolve one frozen sql function surface
function sqlFunctionSurface(value) {
  switch (value) {
    case "transaction_timestamp": return "postgres.transaction-timestamp";
    case "statement_timestamp": return "postgres.statement-timestamp";
    case "clock_timestamp": return "postgres.clock-timestamp";
    case "now": return "postgres.now";
    case "timeofday": return "postgres.timeofday";
    default: return null;
  }
}

// verify one executable provider boundary
function sqlProviderBoundary(tokens, index, start) {
  // admit one expression-root provider
  if (index === start) {
    return true;
  }
  const previous = tokens[index - 1];
  // admit one keyword expression boundary
  if (previous.type === "identifier") {
    switch (previous.value) {
      case "and": case "by": case "distinct": case "else": case "from":
      case "having": case "in": case "join": case "not": case "on": case "or":
      case "perform": case "return": case "select": case "set": case "then":
      case "values": case "when": case "where": return true;
      default: return false;
    }
  }
  return previous.type === "symbol" && ![")", "]", "."].includes(previous.value);
}

// describe one exact executable sql provider
function sqlProviderDescriptor(tokens, index, pairs, start) {
  const token = tokens[index];
  // reject one provider token outside an expression boundary
  if (!sqlProviderBoundary(tokens, index, start)) {
    return null;
  }
  // recognize one typed parse-time current literal
  if (
    sqlKeyword(token, "timestamp") && tokens[index + 1]?.type === "string" &&
    tokens[index + 1].value.toLowerCase() === "now"
  ) {
    return {
      endIndex: index + 2,
      kind: "parse-time-current-value",
      surface: "postgres.timestamp-now-literal",
    };
  }
  const keywordSurface = token?.type === "identifier" ? sqlKeywordSurface(token.value) : null;
  // recognize one exact keyword provider
  if (keywordSurface !== null) {
    // reject a qualified or invalid call spelling
    if (
      sqlSymbol(tokens[index - 1], ".") ||
      sqlSymbol(tokens[index + 1], ".")
    ) {
      return null;
    }
    // include one admitted precision
    if (
      token.value !== "current_date" && sqlSymbol(tokens[index + 1], "(") &&
      pairs.get(index + 1) === index + 3 && tokens[index + 2]?.type === "number"
    ) {
      return { endIndex: index + 4, kind: "current-time-read", surface: keywordSurface };
    }
    // reject other parenthesized keyword forms
    if (sqlSymbol(tokens[index + 1], "(")) {
      return null;
    }
    return { endIndex: index + 1, kind: "current-time-read", surface: keywordSurface };
  }
  // locate one direct qualified or unqualified call
  if (token?.type !== "identifier" && token?.type !== "quoted-identifier") {
    return null;
  }
  if (sqlSymbol(tokens[index - 1], ".")) {
    return null;
  }
  let terminal = index;
  // consume one dotted callee chain
  while (
    sqlSymbol(tokens[terminal + 1], ".") &&
    ["identifier", "quoted-identifier"].includes(tokens[terminal + 2]?.type)
  ) {
    terminal += 2;
  }
  const open = terminal + 1;
  const close = pairs.get(open);
  const surface = sqlFunctionSurface(tokens[terminal]?.value);
  // require one known exact zero-argument call
  if (surface === null || close !== open + 1) {
    return null;
  }
  return { endIndex: close + 1, kind: "current-time-read", surface };
}

// classify one direct syntactic call outside validated grammar roles
function sqlDirectCall(tokens, index, pairs, end, structure) {
  const first = tokens[index];
  // reject one nonname or qualified-name continuation
  if (
    !["identifier", "quoted-identifier"].includes(first?.type) ||
    sqlSymbol(tokens[index - 1], ".")
  ) {
    return null;
  }
  let terminal = index;
  const names = [first.value];
  // consume one dotted call name
  while (
    sqlSymbol(tokens[terminal + 1], ".") &&
    ["identifier", "quoted-identifier"].includes(tokens[terminal + 2]?.type)
  ) {
    names.push(tokens[terminal + 2].value);
    terminal += 2;
  }
  const open = terminal + 1;
  // require an actual parenthesized call opener
  if (!sqlSymbol(tokens[open], "(")) {
    return null;
  }
  const close = pairs.get(open);
  // reject one group already classified as grammar syntax
  if (structure.nonCallOpens.has(open)) {
    return null;
  }
  // require one bounded complete call
  if (close === undefined || close >= end) {
    return null;
  }
  return { close, names, open, terminal };
}

// parse one bounded expression into semantic calls and providers
function sqlExpression(tokens, start, end, context, validatedState = null) {
  // reject one empty expression
  if (start >= end) {
    return null;
  }
  const structure = validatedState?.structure ?? sqlParseIndex(tokens, start, end);
  const pairs = structure?.closeByOpen ?? null;
  // reject malformed groups or statement delimiters
  if (
    pairs === null || tokens.slice(start, end).some(
      // locate one forbidden expression symbol
      (token) => token.type === "symbol" &&
        (sqlSymbol(token, ";") || sqlSymbol(token, ":") || sqlSymbol(token, ":=")),
    )
  ) {
    return null;
  }
  const first = tokens[start];
  const last = tokens[end - 1];
  const trailingOperators = new Set([
    ",", ".", ":", "+", "-", "*", "/", "%", "^", "=", "<", ">", "~",
    "<=", ">=", "<>", "!=", "||", "&&", "->", "->>", "#>", "#>>", "@>",
    "<@", "?&", "?|",
  ]);
  // reject one obviously incomplete expression boundary
  if (
    first.type === "symbol" && [")", "]", ",", "."].includes(first.value) ||
    last.type === "symbol" && (
      trailingOperators.has(last.value) || sqlSymbol(last, "(") || sqlSymbol(last, "[")
    ) ||
    validatedState === null && !sqlStrictExpression(tokens, start, end, {
      owner: null,
      structure,
      validByOpen: sqlValidateQueryRanges(tokens, structure).validByOpen,
    })
  ) {
    return null;
  }
  const root = sqlNode("expression", first.start, last.end);
  const providerKeys = new Set();
  // inspect every positioned expression token
  for (let index = start; index < end; index += 1) {
    const token = tokens[index];
    const provider = sqlProviderDescriptor(tokens, index, pairs, start);
    // retain one exact provider node
    if (provider !== null && provider.endIndex <= end) {
      const providerEnd = tokens[provider.endIndex - 1].end;
      const selected = sqlSelectedTokens(
        context.sourceText.slice(token.start, providerEnd),
        token.start,
      );
      const selectedExact = selected !== null && selected.length > 0 &&
        selected[0].start === token.start && selected.at(-1).end === providerEnd;
      const key = `${index}:${provider.endIndex}`;
      // retain one provider range once after exact identity tokenization
      if (selectedExact && !providerKeys.has(key)) {
        providerKeys.add(key);
        const providerNode = sqlNode(
          "clock-expression",
          token.start,
          providerEnd,
          provider.surface,
        );
        sqlAddChild(root, "value", providerNode);
        context.providers.push({
          end: providerEnd,
          kind: provider.kind,
          node: providerNode,
          normalized: selected.map(sqlNormalizedToken),
          start: token.start,
          surface: provider.surface,
        });
        const providerRange = `${token.start}:${providerEnd}`;
        // preserve one first provider node per exact range
        if (!context.providerNodesByRange.has(providerRange)) {
          context.providerNodesByRange.set(providerRange, providerNode);
        }
      }
    }
    const directCall = sqlDirectCall(tokens, index, pairs, end, structure);
    // skip one noncall name or SQL grammar construct
    if (directCall === null) {
      continue;
    }
    const { close, names, open, terminal } = directCall;
    const call = sqlNode("call", token.start, tokens[close].end, names.join("."));
    const callee = sqlNode("name", token.start, tokens[terminal].end, names.join("."));
    sqlAddChild(call, "func", callee);
    const argumentRanges = sqlCommaRanges(tokens, open + 1, close, pairs);
    // retain every nonempty call argument
    for (const [argumentStart, argumentEnd] of argumentRanges) {
      // omit the empty zero-argument range
      if (argumentStart === argumentEnd) {
        continue;
      }
      const argument = sqlNode(
        "expression",
        tokens[argumentStart].start,
        tokens[argumentEnd - 1].end,
      );
      sqlAddChild(call, "args", argument);
    }
    const providerMatch = providerKeys.has(`${index}:${close + 1}`);
    // link a provider wrapper to its call fact
    if (providerMatch) {
      const providerNode = context.providerNodesByRange.get(
        `${token.start}:${tokens[close].end}`,
      );
      // retain one exact provider-call relationship
      if (providerNode !== undefined) {
        sqlAddChild(providerNode, "value", call);
      }
    } else {
      sqlAddChild(root, "value", call);
      // retain one unresolved routine-call gap for later graph binding
      context.unsupported = true;
    }
  }
  return root;
}

// find one direct top-level keyword in a bounded range
function sqlFindKeyword(tokens, start, end, values, providedStructure = null) {
  const structure = providedStructure ?? sqlParseIndex(tokens, start, end);
  // reject one malformed grouped range
  if (structure === null) {
    return -2;
  }
  let caseDepth = 0;
  // scan one bounded range while jumping owned groups
  for (let index = start; index < end; index += 1) {
    const close = structure.closeByOpen.get(index);
    // skip one complete parenthesized or bracketed member
    if (close !== undefined) {
      index = close;
      continue;
    }
    const token = tokens[index];
    // track one SQL CASE expression
    if (sqlKeyword(token, "case")) {
      caseDepth += 1;
      continue;
    }
    // close one SQL CASE expression
    if (sqlKeyword(token, "end") && caseDepth > 0) {
      caseDepth -= 1;
      continue;
    }
    // locate one requested top-level keyword
    if (caseDepth === 0 && token.type === "identifier" && values.has(token.value)) {
      return index;
    }
  }
  return caseDepth === 0 ? -1 : -2;
}

// find one top-level statement terminator
function sqlStatementEnd(tokens, start, end, providedStructure = null) {
  const structure = providedStructure ?? sqlParseIndex(tokens, start, end);
  // reject one malformed grouped statement
  if (structure === null) {
    return -2;
  }
  let caseDepth = 0;
  // scan until one direct top-level semicolon
  for (let index = start; index < end; index += 1) {
    const close = structure.closeByOpen.get(index);
    // skip one complete grouped member
    if (close !== undefined) {
      index = close;
      continue;
    }
    const token = tokens[index];
    // open one SQL CASE expression
    if (sqlKeyword(token, "case")) {
      caseDepth += 1;
      continue;
    }
    // close one SQL CASE expression
    if (sqlKeyword(token, "end") && caseDepth > 0) {
      caseDepth -= 1;
      continue;
    }
    // return one complete statement boundary
    if (caseDepth === 0 && sqlSymbol(token, ";")) {
      return index;
    }
  }
  return caseDepth === 0 ? -1 : -2;
}

// locate one earliest positioned clause
function sqlFirstClause(indexes, fallback) {
  const positioned = indexes.filter(
    // retain only located clauses
    (index) => index >= 0,
  );
  return positioned.length === 0
    ? fallback
    : minimumValue(positioned, Number.POSITIVE_INFINITY);
}

// locate every direct-owner clause keyword
function sqlClauseIndexes(tokens, start, end, values, state = null) {
  const indexes = [];
  let parenDepth = 0;
  let bracketDepth = 0;
  let caseDepth = 0;
  // scan one bounded owner range
  for (let index = start; index < end; index += 1) {
    const jumped = sqlOwnedJump(state?.structure, state?.owner ?? null, index);
    // skip one child query body atomically
    if (jumped !== index) {
      index = jumped;
      continue;
    }
    const token = tokens[index];
    // track one grouping boundary
    if (sqlSymbol(token, "(")) {
      parenDepth += 1;
      continue;
    }
    // close one parenthesis boundary
    if (sqlSymbol(token, ")")) {
      parenDepth -= 1;
      continue;
    }
    // track one bracket boundary
    if (sqlSymbol(token, "[")) {
      bracketDepth += 1;
      continue;
    }
    // close one bracket boundary
    if (sqlSymbol(token, "]")) {
      bracketDepth -= 1;
      continue;
    }
    // skip grouped tokens
    if (parenDepth !== 0 || bracketDepth !== 0) {
      continue;
    }
    // track one case expression
    if (sqlKeyword(token, "case")) {
      caseDepth += 1;
      continue;
    }
    // close one case expression
    if (sqlKeyword(token, "end") && caseDepth > 0) {
      caseDepth -= 1;
      continue;
    }
    const distinctFrom = sqlKeyword(token, "from") &&
      sqlKeyword(tokens[index - 1], "distinct") && (
        sqlKeyword(tokens[index - 2], "is") ||
        sqlKeyword(tokens[index - 2], "not") && sqlKeyword(tokens[index - 3], "is")
      );
    // retain one requested top-level clause keyword
    if (
      caseDepth === 0 && token.type === "identifier" && values.has(token.value) &&
      !distinctFrom
    ) {
      indexes.push(index);
    }
  }
  return parenDepth === 0 && bracketDepth === 0 && caseDepth === 0 ? indexes : null;
}

// locate one clause index after a boundary
function sqlClauseAfter(indexes, value, after = -1) {
  return indexes.find(
    // locate one matching later clause
    (entry) => entry.value === value && entry.index > after,
  )?.index ?? -1;
}

// locate the next clause boundary
function sqlNextClause(indexes, current, fallback) {
  return sqlFirstClause(indexes.filter(
    // retain later clause indexes
    (index) => index > current,
  ), fallback);
}



// recognize one identifier token
function sqlNameToken(token) {
  return token !== undefined && ["identifier", "quoted-identifier"].includes(token.type);
}

// read one token only inside a production range
function sqlBoundedToken(tokens, index, end) {
  return index >= 0 && index < end ? tokens[index] : undefined;
}

// recognize one qualified wildcard target
function sqlQualifiedWildcard(tokens, start, end) {
  // require one terminal wildcard symbol
  if (!sqlSymbol(tokens[end - 1], "*")) {
    return false;
  }
  // admit one unqualified wildcard
  if (end === start + 1) {
    return true;
  }
  let cursor = start;
  // consume one exact qualified-name chain
  while (cursor < end - 1) {
    // require one positioned qualifier name
    if (!sqlNameToken(tokens[cursor]) || !sqlSymbol(tokens[cursor + 1], ".")) {
      return false;
    }
    cursor += 2;
  }
  return cursor === end - 1;
}

// validate one strict expression-list member
function sqlExpressionMemberValid(tokens, start, end, state, options) {
  let expressionEnd = end;
  // consume one order null-placement suffix
  if (
    options.order === true && expressionEnd - start >= 2 &&
    sqlKeyword(tokens[expressionEnd - 2], "nulls") &&
    (sqlKeyword(tokens[expressionEnd - 1], "first") || sqlKeyword(tokens[expressionEnd - 1], "last"))
  ) {
    expressionEnd -= 2;
  }
  // consume one order direction suffix
  if (
    options.order === true && expressionEnd > start &&
    (sqlKeyword(tokens[expressionEnd - 1], "asc") || sqlKeyword(tokens[expressionEnd - 1], "desc"))
  ) {
    expressionEnd -= 1;
  }
  // locate an explicit top-level alias marker
  const aliases = options.alias === true
    ? sqlClauseIndexes(tokens, start, expressionEnd, new Set(["as"]), state)
    : [];
  // reject malformed or repeated aliases
  if (aliases === null || aliases.length > 1) {
    return false;
  }
  // consume one exact explicit alias
  if (aliases.length === 1) {
    const alias = aliases[0];
    // require one terminal alias name
    if (alias <= start || alias + 2 !== expressionEnd || !sqlNameToken(tokens[alias + 1])) {
      return false;
    }
    expressionEnd = alias;
  }
  // admit one exact wildcard production
  if (options.wildcard === true && sqlQualifiedWildcard(tokens, start, expressionEnd)) {
    return true;
  }
  // admit one complete expression before any implicit alias
  if (sqlStrictExpression(tokens, start, expressionEnd, state)) {
    return true;
  }
  // reject implicit aliases outside selected target grammar
  if (options.alias !== true || expressionEnd - start < 2 || !sqlNameToken(tokens[expressionEnd - 1])) {
    return false;
  }
  return sqlStrictExpression(tokens, start, expressionEnd - 1, state);
}

// validate one strict comma-delimited expression list
function sqlExpressionListValid(tokens, start, end, state, options = {}) {
  const structure = state?.structure ?? sqlParseIndex(tokens, start, end);
  const pairs = structure?.closeByOpen ?? null;
  // reject one malformed or empty list
  if (pairs === null || start >= end) {
    return false;
  }
  // validate every complete list member
  for (const [oneStart, oneEnd] of sqlCommaRanges(tokens, start, end, pairs)) {
    // reject one empty or malformed member
    if (oneStart >= oneEnd || !sqlExpressionMemberValid(tokens, oneStart, oneEnd, state, options)) {
      return false;
    }
  }
  return true;
}

// validate one selected target or returning list
function sqlTargetListValid(tokens, start, end, state) {
  return sqlExpressionListValid(
    tokens,
    start,
    end,
    state,
    { alias: true, wildcard: true },
  );
}

// locate one top-level relation boundary
function sqlRelationBoundary(tokens, start, end, pairs) {
  let caseDepth = 0;
  const joinWords = new Set(["cross", "full", "inner", "join", "left", "natural", "right"]);
  // scan one relation predicate once
  for (let index = start; index < end; index += 1) {
    // skip one owned group or nested query
    if (pairs.has(index)) {
      index = pairs.get(index);
      continue;
    }
    const token = tokens[index];
    // open one case expression
    if (sqlKeyword(token, "case")) {
      caseDepth += 1;
      continue;
    }
    // close one case expression
    if (sqlKeyword(token, "end") && caseDepth > 0) {
      caseDepth -= 1;
      continue;
    }
    // locate one comma or join boundary outside CASE
    if (
      caseDepth === 0 && (
        sqlSymbol(token, ",") ||
        token.type === "identifier" && joinWords.has(token.value)
      )
    ) {
      return index;
    }
  }
  return end;
}

// parse one exact relation primary and optional alias
function sqlRelationPrimaryEnd(tokens, start, end, state) {
  const pairs = state.structure.closeByOpen;
  let cursor = start;
  // consume one lateral or only modifier
  if (sqlKeyword(tokens[cursor], "lateral") || sqlKeyword(tokens[cursor], "only")) {
    cursor += 1;
  }
  const query = state.structure.queryByOpen.get(cursor);
  // consume one validated subquery relation
  if (query !== undefined) {
    // require one already-validated query body
    if (state.validByOpen.get(cursor) !== true) {
      return -1;
    }
    cursor = query.end + 1;
  } else {
    // require one relation or set-returning function name
    if (!sqlNameToken(tokens[cursor])) {
      return -1;
    }
    cursor += 1;
    // consume one qualified relation name
    while (
      sqlSymbol(tokens[cursor], ".") &&
      sqlNameToken(tokens[cursor + 1])
    ) {
      cursor += 2;
    }
    // consume one strict set-returning call
    if (sqlSymbol(sqlBoundedToken(tokens, cursor, end), "(")) {
      const open = cursor;
      const close = pairs.get(open);
      // require one owned call group
      if (close === undefined || close >= end) {
        return -1;
      }
      // validate every executable call argument
      if (
        open + 1 < close &&
        !sqlExpressionListValid(tokens, open + 1, close, state)
      ) {
        return -1;
      }
      cursor = close + 1;
    }
  }
  // consume one optional WITH ORDINALITY phrase
  if (
    sqlKeyword(sqlBoundedToken(tokens, cursor, end), "with") &&
    sqlKeyword(sqlBoundedToken(tokens, cursor + 1, end), "ordinality")
  ) {
    cursor += 2;
  }
  const reserved = new Set([
    "cross", "full", "inner", "join", "left", "natural", "on", "right", "using",
  ]);
  // consume one explicit alias
  if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "as")) {
    // require one alias name after AS
    if (!sqlNameToken(sqlBoundedToken(tokens, cursor + 1, end))) {
      return -1;
    }
    cursor += 2;
  } else if (
    sqlNameToken(sqlBoundedToken(tokens, cursor, end)) &&
    !(tokens[cursor].type === "identifier" && reserved.has(tokens[cursor].value))
  ) {
    // consume one implicit alias name
    cursor += 1;
  }
  // consume one optional alias-column group
  if (sqlSymbol(sqlBoundedToken(tokens, cursor, end), "(")) {
    const open = cursor;
    const close = pairs.get(open);
    // require one nonempty exact column-name list
    if (close === undefined || close >= end) {
      return -1;
    }
    const columns = sqlCommaRanges(tokens, cursor + 1, close, pairs);
    // reject one malformed alias-column slot
    if (columns.some(([oneStart, oneEnd]) => oneEnd !== oneStart + 1 || !sqlNameToken(tokens[oneStart]))) {
      return -1;
    }
    // classify one validated relation alias-column group
    state.structure.nonCallOpens.add(open);
    cursor = close + 1;
  }
  return cursor;
}

// validate one from or using relation production
function sqlFromRangeValid(tokens, start, end, providedState = null) {
  const structure = providedState?.structure ?? sqlParseIndex(tokens, start, end);
  // reject one malformed or empty relation range
  if (structure === null || start >= end) {
    return false;
  }
  const state = providedState ?? {
    structure,
    validByOpen: sqlValidateQueryRanges(tokens, structure).validByOpen,
  };
  let cursor = start;
  let needRelation = true;
  let requirePredicate = false;
  // parse every relation and join suffix once
  while (cursor < end) {
    // consume one required relation primary
    if (needRelation) {
      cursor = sqlRelationPrimaryEnd(tokens, cursor, end, state);
      // reject one missing or malformed relation
      if (cursor < 0) {
        return false;
      }
      needRelation = false;
      continue;
    }
    const token = tokens[cursor];
    // consume one comma-separated relation
    if (sqlSymbol(token, ",")) {
      cursor += 1;
      needRelation = true;
      requirePredicate = false;
      continue;
    }
    // consume one join production and its relation
    if (
      sqlKeyword(token, "join") || sqlKeyword(token, "left") || sqlKeyword(token, "right") ||
      sqlKeyword(token, "full") || sqlKeyword(token, "inner") || sqlKeyword(token, "cross") ||
      sqlKeyword(token, "natural")
    ) {
      const predicateOptional = sqlKeyword(token, "cross") || sqlKeyword(token, "natural");
      // consume join modifiers through one required JOIN keyword
      while (cursor < end && !sqlKeyword(tokens[cursor], "join")) {
        cursor += 1;
      }
      // reject one modifier sequence without JOIN
      if (cursor >= end) {
        return false;
      }
      cursor += 1;
      cursor = sqlRelationPrimaryEnd(tokens, cursor, end, state);
      // reject one missing joined relation
      if (cursor < 0) {
        return false;
      }
      requirePredicate = !predicateOptional;
      continue;
    }
    // consume one ON predicate through the next relation boundary
    if (sqlKeyword(token, "on")) {
      const predicateEnd = sqlRelationBoundary(tokens, cursor + 1, end, structure.closeByOpen);
      // require one strict join predicate
      if (!requirePredicate || !sqlStrictExpression(tokens, cursor + 1, predicateEnd, state)) {
        return false;
      }
      cursor = predicateEnd;
      requirePredicate = false;
      continue;
    }
    // consume one USING name group
    if (sqlKeyword(token, "using")) {
      const open = cursor + 1;
      const close = sqlSymbol(tokens[open], "(")
        ? structure.closeByOpen.get(open)
        : undefined;
      // require one strict nonempty using-name list
      if (close === undefined || close <= open + 1) {
        return false;
      }
      const names = sqlCommaRanges(tokens, open + 1, close, structure.closeByOpen);
      // reject one malformed using-name slot
      if (names.some(([oneStart, oneEnd]) => oneEnd !== oneStart + 1 || !sqlNameToken(tokens[oneStart]))) {
        return false;
      }
      // classify one validated join-using group
      state.structure.nonCallOpens.add(open);
      cursor = close + 1;
      requirePredicate = false;
      continue;
    }
    return false;
  }
  return !needRelation && !requirePredicate;
}



// validate one exact SELECT locking tail
function sqlLockingClauseValid(tokens, start, end) {
  let cursor = start;
  // consume one exact lock strength
  if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "update")) {
    cursor += 1;
  } else if (
    sqlKeyword(sqlBoundedToken(tokens, cursor, end), "no") &&
    sqlKeyword(sqlBoundedToken(tokens, cursor + 1, end), "key") &&
    sqlKeyword(sqlBoundedToken(tokens, cursor + 2, end), "update")
  ) {
    cursor += 3;
  } else if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "share")) {
    cursor += 1;
  } else if (
    sqlKeyword(sqlBoundedToken(tokens, cursor, end), "key") &&
    sqlKeyword(sqlBoundedToken(tokens, cursor + 1, end), "share")
  ) {
    cursor += 2;
  } else {
    return false;
  }
  // consume one optional OF relation-name list
  if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "of")) {
    cursor += 1;
    let needName = true;
    // parse every qualified lock target
    while (cursor < end) {
      const next = sqlNamePathEnd(tokens, cursor, end);
      // require one exact name at each position
      if (!needName || next < 0) {
        return false;
      }
      cursor = next;
      needName = false;
      // continue one comma-delimited target list
      if (sqlSymbol(sqlBoundedToken(tokens, cursor, end), ",")) {
        cursor += 1;
        needName = true;
        continue;
      }
      break;
    }
    // reject one trailing comma
    if (needName) {
      return false;
    }
  }
  // consume one optional wait policy
  if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "nowait")) {
    cursor += 1;
  } else if (
    sqlKeyword(sqlBoundedToken(tokens, cursor, end), "skip") &&
    sqlKeyword(sqlBoundedToken(tokens, cursor + 1, end), "locked")
  ) {
    cursor += 2;
  }
  return cursor === end;
}

// validate one closed select statement
function sqlSelectStatementValid(tokens, start, end, state) {
  const values = new Set([
    "into", "from", "where", "group", "having", "order", "limit", "offset", "for",
  ]);
  const rawIndexes = sqlClauseIndexes(tokens, start + 1, end, values, state);
  // reject malformed select grouping
  if (rawIndexes === null) {
    return false;
  }
  const indexes = rawIndexes.map(
    // pair one clause index with its keyword
    (index) => ({ index, value: tokens[index].value }),
  );
  const clauseRanks = new Map([
    ["into", 1], ["from", 2], ["where", 3], ["group", 4], ["having", 5],
    ["order", 6], ["limit", 7], ["offset", 8], ["for", 9],
  ]);
  const seenClauses = new Set();
  let clauseRank = 0;
  // enforce one ordered unique SELECT clause production
  for (const entry of indexes) {
    const rank = clauseRanks.get(entry.value);
    // reject one duplicate, unknown, or regressing clause
    if (rank === undefined || seenClauses.has(entry.value) || rank < clauseRank) {
      return false;
    }
    seenClauses.add(entry.value);
    clauseRank = rank;
  }
  const into = sqlClauseAfter(indexes, "into");
  const from = sqlClauseAfter(indexes, "from");
  const where = sqlClauseAfter(indexes, "where");
  const group = sqlClauseAfter(indexes, "group");
  const having = sqlClauseAfter(indexes, "having");
  const order = sqlClauseAfter(indexes, "order");
  const limit = sqlClauseAfter(indexes, "limit");
  const offset = sqlClauseAfter(indexes, "offset");
  const locking = sqlClauseAfter(indexes, "for");
  const clausePositions = indexes.map(
    // retain one positioned clause
    (entry) => entry.index,
  );
  let captureRange = null;
  let targetStart = start + 1;
  // consume one all modifier
  if (sqlKeyword(tokens[targetStart], "all")) {
    targetStart += 1;
  } else if (sqlKeyword(tokens[targetStart], "distinct")) {
    // consume one distinct modifier and optional on group
    targetStart += 1;
    if (sqlKeyword(tokens[targetStart], "on")) {
      const open = targetStart + 1;
      const close = sqlSymbol(tokens[open], "(")
        ? state.structure.closeByOpen.get(open)
        : undefined;
      // require one nonempty strict distinct-on list
      if (
        close === undefined || close <= open + 1 ||
        !sqlExpressionListValid(tokens, open + 1, close, state)
      ) {
        return false;
      }
      // classify one validated distinct-on group
      state.structure.nonCallOpens.add(open);
      targetStart = close + 1;
    }
  }
  const targetEnd = sqlFirstClause(clausePositions, end);
  // require one complete select target list
  if (!sqlTargetListValid(tokens, targetStart, targetEnd, state)) {
    return false;
  }
  // validate one direct procedural INTO target list
  if (into >= 0) {
    // reject capture syntax outside one direct PL statement
    if (state.procedural !== true) {
      return false;
    }
    const intoEnd = sqlNextClause(clausePositions, into, end);
    let intoStart = into + 1;
    // consume one strict modifier
    if (sqlKeyword(tokens[intoStart], "strict")) {
      intoStart += 1;
    }
    // require one exact positioned into-name list
    if (!sqlExpressionListValid(tokens, intoStart, intoEnd, state)) {
      return false;
    }
    captureRange = [intoStart, intoEnd];
  }
  // validate one from relation production
  if (from >= 0) {
    const fromEnd = sqlNextClause(clausePositions, from, end);
    // require one complete from relation
    if (!sqlFromRangeValid(tokens, from + 1, fromEnd, state)) {
      return false;
    }
  }
  // validate every predicate expression
  for (const clause of [where, having]) {
    // require one strict predicate when present
    if (
      clause >= 0 &&
      !sqlStrictExpression(tokens, clause + 1, sqlNextClause(clausePositions, clause, end), state)
    ) {
      return false;
    }
  }
  // validate one exact GROUP BY expression list
  if (group >= 0) {
    const listStart = sqlKeyword(tokens[group + 1], "by") ? group + 2 : -1;
    // require BY and every strict group member
    if (
      listStart < 0 ||
      !sqlExpressionListValid(tokens, listStart, sqlNextClause(clausePositions, group, end), state)
    ) {
      return false;
    }
  }
  // validate one exact ORDER BY expression list
  if (order >= 0) {
    const listStart = sqlKeyword(tokens[order + 1], "by") ? order + 2 : -1;
    // require BY and every strict ordering member
    if (
      listStart < 0 ||
      !sqlExpressionListValid(
        tokens,
        listStart,
        sqlNextClause(clausePositions, order, end),
        state,
        { order: true },
      )
    ) {
      return false;
    }
  }
  // validate one limit or offset expression
  for (const clause of [limit, offset]) {
    // require one strict bound expression when present
    if (
      clause >= 0 &&
      !sqlStrictExpression(tokens, clause + 1, sqlNextClause(clausePositions, clause, end), state)
    ) {
      return false;
    }
  }
  // validate one exact locking tail
  if (locking >= 0 && !sqlLockingClauseValid(tokens, locking + 1, end)) {
    return false;
  }
  // retain one grammar-owned root capture range
  if (captureRange !== null) {
    state.captures.push(captureRange);
  }
  return true;
}

// consume one qualified identifier path
function sqlNamePathEnd(tokens, start, end) {
  let cursor = start;
  // require one leading name
  if (!sqlNameToken(sqlBoundedToken(tokens, cursor, end))) {
    return -1;
  }
  cursor += 1;
  // consume every exact dot-name pair
  while (sqlSymbol(sqlBoundedToken(tokens, cursor, end), ".")) {
    // require one member after the separator
    if (!sqlNameToken(sqlBoundedToken(tokens, cursor + 1, end))) {
      return -1;
    }
    cursor += 2;
  }
  return cursor;
}

// validate one comma-delimited identifier list
function sqlNameListValid(tokens, start, end, state) {
  // reject one empty name list
  if (start >= end) {
    return false;
  }
  const pairs = state.structure.closeByOpen;
  // validate every exact name slot
  for (const [oneStart, oneEnd] of sqlCommaRanges(tokens, start, end, pairs)) {
    // require one unqualified positioned name
    if (oneEnd !== oneStart + 1 || !sqlNameToken(tokens[oneStart])) {
      return false;
    }
  }
  return true;
}

// partition one ordered unique clause production
function sqlOrderedClauses(tokens, start, end, state, ranks) {
  const indexes = sqlClauseIndexes(tokens, start, end, new Set(ranks.keys()), state);
  // reject malformed structural ownership
  if (indexes === null) {
    return null;
  }
  const clauses = [];
  const seen = new Set();
  let rank = -1;
  // validate every discovered clause in source order
  for (const index of indexes) {
    const value = tokens[index].value;
    const nextRank = ranks.get(value);
    // reject one duplicate or reordered clause
    if (nextRank === undefined || seen.has(value) || nextRank < rank) {
      return null;
    }
    seen.add(value);
    rank = nextRank;
    clauses.push({ index, value });
  }
  return clauses;
}

// locate one clause in an ordered production
function sqlOrderedClause(clauses, value) {
  return clauses.find(
    // locate one exact clause role
    (clause) => clause.value === value,
  )?.index ?? -1;
}

// locate one ordered clause end
function sqlOrderedClauseEnd(clauses, index, end) {
  return clauses.find(
    // locate the immediate following clause
    (clause) => clause.index > index,
  )?.index ?? end;
}

// validate one exact INSERT target and optional columns
function sqlInsertTargetValid(tokens, start, end, state) {
  let cursor = sqlNamePathEnd(tokens, start, end);
  // reject one missing relation name
  if (cursor < 0) {
    return false;
  }
  // consume one optional exact column-name group
  if (sqlSymbol(sqlBoundedToken(tokens, cursor, end), "(")) {
    const open = cursor;
    const close = state.structure.closeByOpen.get(open);
    // require one terminal nonempty column group
    if (
      close === undefined || close + 1 !== end ||
      !sqlNameListValid(tokens, cursor + 1, close, state)
    ) {
      return false;
    }
    // classify one validated INSERT column-name group
    state.structure.nonCallOpens.add(open);
    cursor = close + 1;
  }
  return cursor === end;
}

// validate one UPDATE relation and optional alias
function sqlUpdateTargetValid(tokens, start, end) {
  let cursor = start;
  // consume one optional ONLY modifier
  if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "only")) {
    cursor += 1;
  }
  cursor = sqlNamePathEnd(tokens, cursor, end);
  // reject one missing update relation
  if (cursor < 0) {
    return false;
  }
  // consume one explicit alias
  if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "as")) {
    cursor += 1;
    // require one exact alias name
    if (!sqlNameToken(sqlBoundedToken(tokens, cursor, end))) {
      return false;
    }
    cursor += 1;
  } else if (sqlNameToken(sqlBoundedToken(tokens, cursor, end))) {
    // consume one implicit alias name
    cursor += 1;
  }
  return cursor === end;
}

// validate one assignment target
function sqlAssignmentTargetValid(tokens, start, end, state) {
  let cursor = sqlNamePathEnd(tokens, start, end);
  // reject one missing assignment target
  if (cursor < 0) {
    return false;
  }
  // consume positioned subscripts
  while (sqlSymbol(sqlBoundedToken(tokens, cursor, end), "[")) {
    const close = state.structure.closeByOpen.get(cursor);
    // require one strict nonempty subscript
    if (
      close === undefined || close >= end ||
      !sqlStrictExpression(tokens, cursor + 1, close, state)
    ) {
      return false;
    }
    cursor = close + 1;
  }
  return cursor === end;
}

// validate one exact assignment list
function sqlAssignmentListValid(tokens, start, end, state) {
  const pairs = state.structure.closeByOpen;
  // reject one empty assignment list
  if (start >= end) {
    return false;
  }
  // validate every positioned assignment
  for (const [assignmentStart, assignmentEnd] of sqlCommaRanges(tokens, start, end, pairs)) {
    let equals = -1;
    // locate one top-level assignment split
    for (let index = assignmentStart; index < assignmentEnd; index += 1) {
      // skip one nested expression group
      if (pairs.has(index)) {
        index = pairs.get(index);
        continue;
      }
      // retain the first exact assignment symbol
      if (sqlSymbol(tokens[index], "=")) {
        equals = index;
        break;
      }
    }
    // require one exact target and strict value
    if (
      equals <= assignmentStart ||
      !sqlAssignmentTargetValid(tokens, assignmentStart, equals, state) ||
      !sqlStrictExpression(tokens, equals + 1, assignmentEnd, state)
    ) {
      return false;
    }
  }
  return true;
}

// validate one insert values row list
function sqlValuesValid(tokens, start, end, state) {
  const pairs = state.structure.closeByOpen;
  // reject one malformed values list
  if (start >= end) {
    return false;
  }
  // validate every positioned values row
  for (const [rowStart, rowEnd] of sqlCommaRanges(tokens, start, end, pairs)) {
    const close = sqlSymbol(tokens[rowStart], "(") ? pairs.get(rowStart) : undefined;
    // require one complete nonempty row group
    if (close !== rowEnd - 1 || rowStart + 1 >= close) {
      return false;
    }
    // require every strict row value
    if (!sqlExpressionListValid(tokens, rowStart + 1, close, state)) {
      return false;
    }
    // classify one validated values-row group
    state.structure.nonCallOpens.add(rowStart);
  }
  return true;
}

// validate one RETURNING list and optional procedural capture
function sqlReturningClauseValid(tokens, start, end, state) {
  const clauses = sqlOrderedClauses(
    tokens,
    start,
    end,
    state,
    new Map([["into", 0]]),
  );
  // reject malformed or repeated procedural capture suffixes
  if (clauses === null) {
    return false;
  }
  const into = sqlOrderedClause(clauses, "into");
  // reject capture syntax outside one direct PL statement
  if (into >= 0 && state.procedural !== true) {
    return false;
  }
  const targetEnd = into < 0 ? end : into;
  // require every returned expression
  if (!sqlTargetListValid(tokens, start, targetEnd, state)) {
    return false;
  }
  // finish one SQL-only RETURNING clause
  if (into < 0) {
    return true;
  }
  let captureStart = into + 1;
  // consume one procedural STRICT capture modifier
  if (sqlKeyword(sqlBoundedToken(tokens, captureStart, end), "strict")) {
    captureStart += 1;
  }
  // require one complete procedural capture target list
  if (!sqlExpressionListValid(tokens, captureStart, end, state)) {
    return false;
  }
  state.captures.push([captureStart, end]);
  return true;
}

// validate one exact ON CONFLICT production
function sqlConflictClauseValid(tokens, start, end, state) {
  let cursor = start;
  // require one exact ON CONFLICT prefix
  if (
    !sqlKeyword(sqlBoundedToken(tokens, cursor, end), "on") ||
    !sqlKeyword(sqlBoundedToken(tokens, cursor + 1, end), "conflict")
  ) {
    return false;
  }
  cursor += 2;
  // consume one optional inference target
  if (sqlSymbol(sqlBoundedToken(tokens, cursor, end), "(")) {
    const open = cursor;
    const close = state.structure.closeByOpen.get(cursor);
    // require one strict nonempty inference list
    if (
      close === undefined || close >= end ||
      !sqlExpressionListValid(tokens, cursor + 1, close, state)
    ) {
      return false;
    }
    // classify one validated conflict-inference group
    state.structure.nonCallOpens.add(open);
    cursor = close + 1;
  } else if (
    sqlKeyword(sqlBoundedToken(tokens, cursor, end), "on") &&
    sqlKeyword(sqlBoundedToken(tokens, cursor + 1, end), "constraint")
  ) {
    cursor += 2;
    // require one exact constraint name
    if (!sqlNameToken(sqlBoundedToken(tokens, cursor, end))) {
      return false;
    }
    cursor += 1;
  }
  const doIndexes = sqlClauseIndexes(tokens, cursor, end, new Set(["do"]), state);
  // require one unique conflict action marker
  if (doIndexes === null || doIndexes.length !== 1) {
    return false;
  }
  const action = doIndexes[0];
  // validate one optional inference predicate
  if (cursor < action) {
    // require one exact WHERE predicate prefix
    if (
      !sqlKeyword(tokens[cursor], "where") ||
      !sqlStrictExpression(tokens, cursor + 1, action, state)
    ) {
      return false;
    }
  }
  cursor = action + 1;
  // admit one exact DO NOTHING action
  if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "nothing")) {
    return cursor + 1 === end;
  }
  // require one DO UPDATE SET action
  if (
    !sqlKeyword(sqlBoundedToken(tokens, cursor, end), "update") ||
    !sqlKeyword(sqlBoundedToken(tokens, cursor + 1, end), "set")
  ) {
    return false;
  }
  cursor += 2;
  const whereIndexes = sqlClauseIndexes(tokens, cursor, end, new Set(["where"]), state);
  // reject repeated action predicates
  if (whereIndexes === null || whereIndexes.length > 1) {
    return false;
  }
  const where = whereIndexes[0] ?? -1;
  const assignmentsEnd = where < 0 ? end : where;
  // require every update assignment
  if (!sqlAssignmentListValid(tokens, cursor, assignmentsEnd, state)) {
    return false;
  }
  return where < 0 || sqlStrictExpression(tokens, where + 1, end, state);
}

// summarize one exact INSERT clause range
function sqlInsertClauseSummary(tokens, start, end, attempt, owner) {
  const rangeId = sqlInternProductionRange(attempt, start, end);
  const summaryKey = `${owner ?? -1}:${rangeId}`;
  const cached = attempt.insertSummaries.get(summaryKey);
  // reuse one exact scalar summary
  if (cached !== undefined) {
    return cached;
  }
  const ownerIndex = attempt.structure.directClauseIndex.byOwner.get(owner);
  // reject one absent direct owner index
  if (ownerIndex === undefined) {
    attempt.insertSummaries.set(summaryKey, null);
    return null;
  }
  let window = attempt.insertWindowByOwner.get(owner);
  // create one owner-local intrusive clause window
  if (window === undefined) {
    window = {
      activationCursor: 0,
      heads: { body: null, conflict: null, returning: null },
      lastEndOrdinal: end,
      lastStartOrdinal: start,
      placeholders: new Map(),
    };
    // materialize one immutable topology family
    function materialize(items, family) {
      const nodes = [];
      // allocate one scratch node per immutable row or placeholder
      for (const item of items) {
        const node = { ...item, next: null, previous: null };
        nodes.push(node);
        // retain one family placeholder address
        if (item.kind === "placeholder") {
          let placeholders = window.placeholders.get(item.caseId);
          // create one case placeholder family map
          if (placeholders === undefined) {
            placeholders = {};
            window.placeholders.set(item.caseId, placeholders);
          }
          placeholders[family] = node;
        }
      }
      // link one direct family sequence
      for (let index = 1; index < nodes.length; index += 1) {
        nodes[index - 1].next = nodes[index];
        nodes[index].previous = nodes[index - 1];
      }
      return nodes;
    }
    // seed every direct family head once
    for (const family of ["body", "conflict", "returning"]) {
      const nodes = materialize(ownerIndex.rootByFamily[family], family);
      window.heads[family] = nodes[0] ?? null;
    }
    attempt.insertWindowByOwner.set(owner, window);
  }
  const scanStart = start + 2;
  // activate every CASE opener excluded by the advancing range start
  while (
    window.activationCursor < ownerIndex.cases.length &&
    ownerIndex.cases[window.activationCursor].open < scanStart
  ) {
    const caseNode = ownerIndex.cases[window.activationCursor];
    window.activationCursor += 1;
    const placeholders = window.placeholders.get(caseNode.id) ?? {};
    // splice each direct family block exactly once
    for (const family of ["body", "conflict", "returning"]) {
      const placeholder = placeholders[family];
      // skip one unreachable nested placeholder
      if (placeholder === undefined) {
        continue;
      }
      const nodes = [];
      // allocate one scratch node per newly visible family item
      for (const item of caseNode.itemsByFamily[family]) {
        const node = { ...item, next: null, previous: null };
        nodes.push(node);
        // retain one nested placeholder address
        if (item.kind === "placeholder") {
          let nested = window.placeholders.get(item.caseId);
          // create one nested placeholder family map
          if (nested === undefined) {
            nested = {};
            window.placeholders.set(item.caseId, nested);
          }
          nested[family] = node;
        }
      }
      // link one newly visible family block
      for (let index = 1; index < nodes.length; index += 1) {
        nodes[index - 1].next = nodes[index];
        nodes[index].previous = nodes[index - 1];
      }
      const before = placeholder.previous;
      const after = placeholder.next;
      const first = nodes[0] ?? after;
      const last = nodes.at(-1) ?? before;
      // splice one child block between shared boundary cells
      if (before !== null) {
        before.next = first;
      }
      if (first !== null) {
        first.previous = before;
      }
      if (last !== null) {
        last.next = after;
      }
      if (after !== null) {
        after.previous = last;
      }
      let candidate = nodes[0] ?? null;
      // locate one newly visible eligible row
      while (candidate !== null &&
        (candidate.kind !== "row" || candidate.position < scanStart)) {
        candidate = candidate.next === after ? null : candidate.next;
      }
      const head = window.heads[family];
      // move one family head to an earlier newly visible row
      if (candidate !== null && (head === null || candidate.position < head.position)) {
        window.heads[family] = candidate;
      } else if (head === placeholder) {
        window.heads[family] = first;
      }
    }
  }
  window.lastStartOrdinal = Math.max(window.lastStartOrdinal, start);
  window.lastEndOrdinal = Math.min(window.lastEndOrdinal, end);
  // locate one first visible row in a family
  function firstRow(family) {
    let node = window.heads[family];
    // advance one monotone family head
    while (node !== null && (node.kind !== "row" || node.position < scanStart)) {
      node = node.next;
    }
    window.heads[family] = node;
    return node !== null && node.position < end ? node : null;
  }
  // locate at most two visible rows in one saturated family
  function firstTwo(family) {
    const first = firstRow(family);
    // stop without one visible row
    if (first === null) {
      return [];
    }
    let second = first.next;
    // skip inactive placeholders only
    while (second !== null && second.kind !== "row") {
      second = second.next;
    }
    return second !== null && second.position < end
      ? [first.position, second.position]
      : [first.position];
  }
  const bodyRow = firstRow("body");
  const summary = {
    body: bodyRow?.position,
    conflicts: firstTwo("conflict"),
    returnings: firstTwo("returning"),
  };
  attempt.insertSummaries.set(summaryKey, summary);
  return summary;
}

// run one closed static-production frame stack
function sqlRunStaticValidationFrames(
  tokens,
  start,
  end,
  attempt,
  code = "STATIC_RANGE",
  owner = attempt.owner,
  procedural = attempt.procedural,
) {
  const result = { value: false };
  const frames = [{ code, end, owner, phase: "enter", procedural, result, start }];
  // consume every production frame LIFO
  while (frames.length > 0) {
    const frame = frames.pop();
    sqlStepStaticValidationFrame(tokens, attempt, frames, frame);
  }
  return result.value === true;
}

// reduce one static-production frame
function sqlStepStaticValidationFrame(tokens, attempt, frames, frame) {
  attempt.owner = frame.owner;
  attempt.procedural = frame.procedural;
  const rangeId = sqlInternProductionRange(attempt, frame.start, frame.end);
  const memoKey = `${frame.code}:${rangeId}:${frame.owner ?? -1}:${frame.procedural ? 1 : 0}:0`;
  // finish one production result
  function finish(value) {
    const valid = value === true;
    attempt.memo.set(memoKey, valid);
    frame.result.value = valid;
  }
  // reuse one completed production
  if (frame.phase === "enter" && attempt.memo.has(memoKey)) {
    frame.result.value = attempt.memo.get(memoKey);
    return;
  }
  // forward one completed child result
  if (frame.phase === "after-child") {
    finish(frame.child.value);
    return;
  }
  // finish one INSERT after its embedded query body
  if (frame.phase === "after-insert-body") {
    if (!frame.child.value) {
      finish(false);
      return;
    }
    attempt.owner = frame.owner;
    attempt.procedural = frame.procedural;
    const conflict = frame.conflict;
    const returning = frame.returning;
    // validate one exact conflict action
    if (
      conflict >= 0 &&
      !sqlConflictClauseValid(tokens, conflict, returning >= 0 ? returning : frame.end, attempt)
    ) {
      finish(false);
      return;
    }
    finish(returning < 0 || sqlReturningClauseValid(tokens, returning + 1, frame.end, attempt));
    return;
  }
  // dispatch one range production
  if (frame.code === "STATIC_RANGE") {
    const child = { value: false };
    frame.child = child;
    frame.phase = "after-child";
    frames.push(frame);
    frames.push({
      code: sqlKeyword(tokens[frame.start], "with") ? "WITH" : "STATIC_CORE",
      end: frame.end,
      owner: frame.owner,
      phase: "enter",
      procedural: frame.procedural,
      result: child,
      start: frame.start,
    });
    return;
  }
  // dispatch one static core production
  if (frame.code === "STATIC_CORE") {
    const first = tokens[frame.start];
    // reject one empty or symbol-led statement
    if (first?.type !== "identifier" || frame.start + 1 >= frame.end) {
      finish(false);
      return;
    }
    // schedule one INSERT production
    if (sqlKeyword(first, "insert")) {
      const child = { value: false };
      frame.child = child;
      frame.phase = "after-child";
      frames.push(frame);
      frames.push({
        code: "INSERT",
        end: frame.end,
        owner: frame.owner,
        phase: "enter",
        procedural: frame.procedural,
        result: child,
        start: frame.start,
      });
      return;
    }
    // validate one existing synchronous leaf
    const valid = sqlKeyword(first, "select")
      ? sqlSelectQueryValid(tokens, frame.start, frame.end, attempt)
      : sqlKeyword(first, "update")
        ? sqlUpdateStatementValid(tokens, frame.start, frame.end, attempt)
        : sqlKeyword(first, "delete")
          ? sqlDeleteStatementValid(tokens, frame.start, frame.end, attempt)
          : false;
    finish(valid);
    return;
  }
  // parse one WITH prefix before its main core
  if (frame.code === "WITH") {
    let cursor = frame.start + 1;
    const names = new Set();
    // consume one optional recursive modifier
    if (sqlKeyword(sqlBoundedToken(tokens, cursor, frame.end), "recursive")) {
      cursor += 1;
    }
    // parse every positioned CTE item
    while (cursor < frame.end) {
      const name = sqlBoundedToken(tokens, cursor, frame.end);
      // require one unique CTE name
      if (![
        "identifier",
        "quoted-identifier",
      ].includes(name?.type) || names.has(`${name.type}:${name.value}`)) {
        finish(false);
        return;
      }
      names.add(`${name.type}:${name.value}`);
      cursor += 1;
      // consume one optional column-name group
      if (sqlSymbol(sqlBoundedToken(tokens, cursor, frame.end), "(")) {
        const open = cursor;
        const close = attempt.structure.closeByOpen.get(open);
        // require one nonempty complete column group
        if (close === undefined || close <= cursor + 1) {
          finish(false);
          return;
        }
        const ranges = sqlCommaRanges(tokens, cursor + 1, close, attempt.structure.closeByOpen);
        // require every exact column name
        if (ranges.some(
          // reject one nonname column slot
          ([oneStart, oneEnd]) => oneEnd !== oneStart + 1 ||
            !["identifier", "quoted-identifier"].includes(tokens[oneStart]?.type),
        )) {
          finish(false);
          return;
        }
        attempt.structure.nonCallOpens.add(open);
        cursor = close + 1;
      }
      // require one exact AS marker
      if (!sqlKeyword(sqlBoundedToken(tokens, cursor, frame.end), "as")) {
        finish(false);
        return;
      }
      cursor += 1;
      // consume one optional materialization phrase
      if (
        sqlKeyword(sqlBoundedToken(tokens, cursor, frame.end), "not") &&
        sqlKeyword(sqlBoundedToken(tokens, cursor + 1, frame.end), "materialized")
      ) {
        cursor += 2;
      } else if (sqlKeyword(sqlBoundedToken(tokens, cursor, frame.end), "materialized")) {
        cursor += 1;
      }
      const query = attempt.structure.queryByOpen.get(cursor);
      // require one indexed and valid CTE query
      if (query === undefined || attempt.validByOpen.get(cursor) !== true) {
        finish(false);
        return;
      }
      cursor = query.end + 1;
      // continue one CTE list
      if (sqlSymbol(sqlBoundedToken(tokens, cursor, frame.end), ",")) {
        cursor += 1;
        continue;
      }
      break;
    }
    const child = { value: false };
    frame.child = child;
    frame.phase = "after-child";
    frames.push(frame);
    frames.push({
      code: "STATIC_CORE",
      end: frame.end,
      owner: frame.owner,
      phase: "enter",
      procedural: frame.procedural,
      result: child,
      start: cursor,
    });
    return;
  }
  // validate one INSERT production around one optional child query
  if (frame.code === "INSERT") {
    // require one INTO prefix
    if (!sqlKeyword(sqlBoundedToken(tokens, frame.start + 1, frame.end), "into")) {
      finish(false);
      return;
    }
    const summary = sqlInsertClauseSummary(
      tokens,
      frame.start,
      frame.end,
      attempt,
      frame.owner,
    );
    // reject malformed statement ownership
    if (summary === null) {
      finish(false);
      return;
    }
    const { body, conflicts, returnings } = summary;
    // require one ordered body and tails
    if (
      body === undefined || conflicts.length > 1 || returnings.length > 1 ||
      conflicts.some((index) => index < body) || returnings.some((index) => index < body) ||
      conflicts.length === 1 && returnings.length === 1 && conflicts[0] > returnings[0]
    ) {
      finish(false);
      return;
    }
    // require one exact target before the body
    if (!sqlInsertTargetValid(tokens, frame.start + 2, body, attempt)) {
      finish(false);
      return;
    }
    const conflict = conflicts[0] ?? -1;
    const returning = returnings[0] ?? -1;
    const bodyEnd = minimumValue(
      [conflict, returning].filter((index) => index >= 0),
      frame.end,
    );
    // validate one VALUES body
    if (sqlKeyword(tokens[body], "values")) {
      if (!sqlValuesValid(tokens, body + 1, bodyEnd, attempt)) {
        finish(false);
        return;
      }
    } else if (sqlKeyword(tokens[body], "default")) {
      // require one exact DEFAULT VALUES body
      if (!sqlKeyword(tokens[body + 1], "values") || body + 2 !== bodyEnd) {
        finish(false);
        return;
      }
    } else {
      const child = { value: false };
      frame.child = child;
      frame.conflict = conflict;
      frame.returning = returning;
      frame.phase = "after-insert-body";
      frames.push(frame);
      frames.push({
        code: "STATIC_RANGE",
        end: bodyEnd,
        owner: frame.owner,
        phase: "enter",
        procedural: false,
        result: child,
        start: body,
      });
      return;
    }
    // validate one synchronous tail
    if (
      conflict >= 0 &&
      !sqlConflictClauseValid(tokens, conflict, returning >= 0 ? returning : frame.end, attempt)
    ) {
      finish(false);
      return;
    }
    finish(returning < 0 || sqlReturningClauseValid(tokens, returning + 1, frame.end, attempt));
    return;
  }
  finish(false);
}

// validate one insert statement
function sqlInsertStatementValid(tokens, start, end, state) {
  return sqlRunStaticValidationFrames(tokens, start, end, state, "INSERT");
}

// validate one update statement
function sqlUpdateStatementValid(tokens, start, end, state) {
  const clauses = sqlOrderedClauses(
    tokens,
    start + 1,
    end,
    state,
    new Map([["set", 0], ["from", 1], ["where", 2], ["returning", 3]]),
  );
  // require one ordered production beginning with SET
  if (clauses === null || clauses[0]?.value !== "set") {
    return false;
  }
  const set = clauses[0].index;
  const from = sqlOrderedClause(clauses, "from");
  const where = sqlOrderedClause(clauses, "where");
  const returning = sqlOrderedClause(clauses, "returning");
  // require one exact target relation
  if (!sqlUpdateTargetValid(tokens, start + 1, set)) {
    return false;
  }
  // require every exact assignment
  if (!sqlAssignmentListValid(tokens, set + 1, sqlOrderedClauseEnd(clauses, set, end), state)) {
    return false;
  }
  // validate one optional FROM production
  if (
    from >= 0 &&
    !sqlFromRangeValid(tokens, from + 1, sqlOrderedClauseEnd(clauses, from, end), state)
  ) {
    return false;
  }
  // validate one optional WHERE predicate
  if (
    where >= 0 &&
    !sqlStrictExpression(tokens, where + 1, sqlOrderedClauseEnd(clauses, where, end), state)
  ) {
    return false;
  }
  return returning < 0 || sqlReturningClauseValid(tokens, returning + 1, end, state);
}

// validate one exact DELETE target
function sqlDeleteTargetValid(tokens, start, end) {
  let cursor = start;
  // consume one optional ONLY modifier
  if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "only")) {
    cursor += 1;
  }
  cursor = sqlNamePathEnd(tokens, cursor, end);
  // reject one missing relation name
  if (cursor < 0) {
    return false;
  }
  // consume one optional inheritance marker
  if (sqlSymbol(sqlBoundedToken(tokens, cursor, end), "*")) {
    cursor += 1;
  }
  // consume one explicit alias
  if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "as")) {
    cursor += 1;
    // require one exact alias name
    if (!sqlNameToken(sqlBoundedToken(tokens, cursor, end))) {
      return false;
    }
    cursor += 1;
  } else if (sqlNameToken(sqlBoundedToken(tokens, cursor, end))) {
    // consume one implicit alias name
    cursor += 1;
  }
  return cursor === end;
}

// validate one delete statement
function sqlDeleteStatementValid(tokens, start, end, state) {
  // require one FROM prefix
  if (!sqlKeyword(sqlBoundedToken(tokens, start + 1, end), "from")) {
    return false;
  }
  const clauses = sqlOrderedClauses(
    tokens,
    start + 2,
    end,
    state,
    new Map([["using", 0], ["where", 1], ["returning", 2]]),
  );
  // reject duplicate or reordered clauses
  if (clauses === null) {
    return false;
  }
  const using = sqlOrderedClause(clauses, "using");
  const where = sqlOrderedClause(clauses, "where");
  const returning = sqlOrderedClause(clauses, "returning");
  const relationEnd = clauses[0]?.index ?? end;
  // require one exact deleted relation target
  if (!sqlDeleteTargetValid(tokens, start + 2, relationEnd)) {
    return false;
  }
  // validate one USING relation list
  if (
    using >= 0 &&
    !sqlFromRangeValid(tokens, using + 1, sqlOrderedClauseEnd(clauses, using, end), state)
  ) {
    return false;
  }
  // validate one WHERE predicate
  if (
    where >= 0 &&
    !sqlStrictExpression(tokens, where + 1, sqlOrderedClauseEnd(clauses, where, end), state)
  ) {
    return false;
  }
  return returning < 0 || sqlReturningClauseValid(tokens, returning + 1, end, state);
}

// validate one SELECT set-expression production
function sqlSelectQueryValid(tokens, start, end, state) {
  const operators = sqlClauseIndexes(
    tokens,
    start + 1,
    end,
    new Set(["union", "intersect", "except"]),
    state,
  );
  // reject malformed set-expression grouping
  if (operators === null) {
    return false;
  }
  let cursor = start;
  const coreState = operators.length === 0
    ? state
    : { ...state, procedural: false };
  // validate every complete SELECT core
  for (const operator of operators) {
    // require one SELECT core before the set operator
    if (!sqlKeyword(tokens[cursor], "select") || !sqlSelectStatementValid(tokens, cursor, operator, coreState)) {
      return false;
    }
    cursor = operator + 1;
    // consume one optional set quantifier
    if (sqlKeyword(tokens[cursor], "all") || sqlKeyword(tokens[cursor], "distinct")) {
      cursor += 1;
    }
  }
  return sqlKeyword(tokens[cursor], "select") &&
    sqlSelectStatementValid(tokens, cursor, end, coreState);
}

// validate one exact WITH prefix and its main statement
function sqlWithStatementValid(tokens, start, end, state) {
  return sqlRunStaticValidationFrames(tokens, start, end, state, "WITH");
}

// validate one indexed static statement core
function sqlStaticStatementCoreValid(tokens, start, end, state) {
  return sqlRunStaticValidationFrames(tokens, start, end, state, "STATIC_CORE");
}

// validate one indexed static statement range
function sqlStaticStatementRangeValid(tokens, start, end, state) {
  return sqlRunStaticValidationFrames(tokens, start, end, state, "STATIC_RANGE");
}

// consume one exact TRUNCATE target
function sqlTruncateTargetEnd(tokens, start, end) {
  let cursor = start;
  // consume one optional ONLY modifier
  if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "only")) {
    cursor += 1;
  }
  cursor = sqlNamePathEnd(tokens, cursor, end);
  // reject one missing relation name
  if (cursor < 0) {
    return -1;
  }
  // consume one optional inheritance marker
  if (sqlSymbol(sqlBoundedToken(tokens, cursor, end), "*")) {
    cursor += 1;
  }
  return cursor;
}

// validate one exact TRUNCATE statement
function sqlTruncateStatementValid(tokens, start, end) {
  let cursor = start + 1;
  let needTarget = true;
  let targetCount = 0;
  // consume one optional TABLE keyword
  if (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "table")) {
    cursor += 1;
  }
  // consume every comma-delimited relation target
  while (cursor < end) {
    const next = sqlTruncateTargetEnd(tokens, cursor, end);
    // reject one missing or malformed target
    if (next < 0) {
      return false;
    }
    targetCount += 1;
    needTarget = false;
    cursor = next;
    // continue one exact target list
    if (sqlSymbol(sqlBoundedToken(tokens, cursor, end), ",")) {
      cursor += 1;
      needTarget = true;
      continue;
    }
    break;
  }
  // require at least one relation target and no trailing separator
  if (targetCount === 0 || needTarget) {
    return false;
  }
  // consume one optional identity policy
  if (
    (sqlKeyword(sqlBoundedToken(tokens, cursor, end), "restart") ||
      sqlKeyword(sqlBoundedToken(tokens, cursor, end), "continue")) &&
    sqlKeyword(sqlBoundedToken(tokens, cursor + 1, end), "identity")
  ) {
    cursor += 2;
  }
  // consume one optional dependency policy
  if (
    sqlKeyword(sqlBoundedToken(tokens, cursor, end), "cascade") ||
    sqlKeyword(sqlBoundedToken(tokens, cursor, end), "restrict")
  ) {
    cursor += 1;
  }
  return cursor === end;
}

// parse one closed static sql statement shape
function sqlStaticStatementState(tokens, start, end, procedural = false) {
  const structure = sqlParseIndex(tokens, start, end);
  // reject malformed statement delimiter ownership
  if (structure === null) {
    return null;
  }
  const state = sqlCreateStaticAttempt(structure, procedural);
  const nested = sqlValidateQueryRanges(tokens, structure, state);
  // reject one malformed nested query before its parent
  if (!nested.allValid) {
    return null;
  }
  state.owner = null;
  state.procedural = procedural;
  // retain one separately closed truncate statement
  if (sqlKeyword(tokens[start], "truncate")) {
    return sqlTruncateStatementValid(tokens, start, end) ? state : null;
  }
  return sqlRunStaticValidationFrames(
    tokens,
    start,
    end,
    state,
    "STATIC_RANGE",
    null,
    procedural,
  ) ? state : null;
}

// validate one closed static sql statement shape
function sqlStaticStatementValid(tokens, start, end) {
  return sqlStaticStatementState(tokens, start, end, false) !== null;
}

// parse one simple plpgsql or sql statement
function sqlSimpleStatement(tokens, start, end, context, procedural = false) {
  const first = tokens[start];
  // reject one empty statement
  if (first === undefined || start >= end) {
    return { nodes: [], valid: false };
  }
  // isolate dynamic constructed sql
  if (sqlKeyword(first, "execute")) {
    context.unsupported = true;
    return { nodes: [], valid: true };
  }
  // parse one return statement
  if (sqlKeyword(first, "return")) {
    // reject unsupported return-query forms
    if (sqlKeyword(tokens[start + 1], "next") || sqlKeyword(tokens[start + 1], "query")) {
      context.unsupported = true;
      return { nodes: [], valid: true };
    }
    const returned = sqlNode("return", first.start, tokens[end - 1].end, "return");
    // retain one returned expression
    if (start + 1 < end) {
      const expression = sqlExpression(tokens, start + 1, end, context);
      // reject one malformed return expression
      if (expression === null) {
        return { nodes: [], valid: false };
      }
      sqlAddChild(returned, "value", expression);
    }
    return { nodes: [returned], valid: true };
  }
  // parse one raise statement without treating message text as code
  if (sqlKeyword(first, "raise")) {
    return {
      nodes: [sqlNode("control", first.start, tokens[end - 1].end, "raise")],
      valid: true,
    };
  }
  let assignmentIndex = -1;
  const assignmentStructure = sqlParseIndex(tokens, start, end);
  const assignmentPairs = assignmentStructure?.closeByOpen ?? null;
  // locate one top-level assignment symbol
  for (let index = start; index < end; index += 1) {
    // skip one nested expression group
    if (assignmentPairs?.has(index)) {
      index = assignmentPairs.get(index);
      continue;
    }
    // retain one procedural assignment
    if (sqlSymbol(tokens[index], ":=")) {
      assignmentIndex = index;
      break;
    }
  }
  // parse one procedural binding
  if (assignmentStructure !== null && assignmentIndex > start) {
    const assignmentNested = assignmentStructure === null
      ? null
      : sqlValidateQueryRanges(tokens, assignmentStructure);
    const assignmentState = assignmentNested === null || !assignmentNested.allValid
      ? null
      : {
        owner: null,
        structure: assignmentStructure,
        validByOpen: assignmentNested.validByOpen,
      };
    // reject one malformed target or value expression
    if (
      assignmentState === null ||
      !sqlAssignmentTargetValid(tokens, start, assignmentIndex, assignmentState) ||
      !sqlStrictExpression(tokens, assignmentIndex + 1, end, assignmentState)
    ) {
      return { nodes: [], valid: false };
    }
    const value = sqlExpression(
      tokens,
      assignmentIndex + 1,
      end,
      context,
      assignmentState,
    );
    // reject one malformed assigned expression
    if (value === null) {
      return { nodes: [], valid: false };
    }
    const targetTokens = tokens.slice(start, assignmentIndex);
    const identifier = targetTokens.filter(
      // retain identifier target parts
      (token) => ["identifier", "quoted-identifier"].includes(token.type),
    ).map(
      // retain one decoded target name
      (token) => token.value,
    ).join(".");
    const binding = sqlNode("binding", first.start, tokens[end - 1].end, identifier || null);
    const target = sqlNode("name", first.start, tokens[assignmentIndex - 1].end, identifier || null);
    sqlAddChild(binding, "target", target);
    sqlAddChild(binding, "value", value);
    return { nodes: [binding], valid: true };
  }
  const staticStarts = new Set(["select", "insert", "update", "delete", "truncate", "with"]);
  const staticStatement = first.type === "identifier" && staticStarts.has(first.value);
  // parse one perform, call, or static sql statement
  if (sqlKeyword(first, "perform") || sqlKeyword(first, "call") || staticStatement) {
    const expressionStart = sqlKeyword(first, "perform") || sqlKeyword(first, "call")
      ? start + 1
      : start;
    let expressionState = staticStatement
      ? sqlStaticStatementState(tokens, start, end, procedural)
      : null;
    // validate one PERFORM target list before semantic collection
    if (sqlKeyword(first, "perform")) {
      const structure = sqlParseIndex(tokens, expressionStart, end);
      const nested = structure === null ? null : sqlValidateQueryRanges(tokens, structure);
      expressionState = nested === null || !nested.allValid
        ? null
        : {
          owner: null,
          procedural: false,
          structure,
          validByOpen: nested.validByOpen,
        };
      // require every positioned PERFORM expression
      if (
        expressionState === null ||
        !sqlExpressionListValid(tokens, expressionStart, end, expressionState)
      ) {
        return { nodes: [], valid: false };
      }
    }
    // reject one malformed static statement before collecting providers
    if (staticStatement && expressionState === null) {
      return { nodes: [], valid: false };
    }
    const expression = sqlExpression(
      tokens,
      expressionStart,
      end,
      context,
      expressionState,
    );
    // reject one malformed executable statement
    if (expression === null) {
      return { nodes: [], valid: false };
    }
    const control = sqlNode("control", first.start, tokens[end - 1].end, first.value);
    sqlAddChild(control, "value", expression);
    const nodes = [control];
    // retain every grammar-owned root capture target binding
    for (const [captureStart, captureEnd] of expressionState?.captures ?? []) {
      // retain every positioned target name
      for (const [targetStart, oneEnd] of sqlCommaRanges(
        tokens,
        captureStart,
        captureEnd,
        expressionState.structure.closeByOpen,
      )) {
        // skip one empty target
        if (targetStart >= oneEnd) {
          continue;
        }
        const name = tokens.slice(targetStart, oneEnd).filter(
          // retain target identifiers
          (token) => ["identifier", "quoted-identifier"].includes(token.type),
        ).map(
          // decode one target identifier
          (token) => token.value,
        ).join(".");
        const binding = sqlNode(
          "binding",
          tokens[targetStart].start,
          tokens[oneEnd - 1].end,
          name || null,
        );
        sqlAddChild(
          binding,
          "target",
          sqlNode("name", tokens[targetStart].start, tokens[oneEnd - 1].end, name || null),
        );
        nodes.push(binding);
      }
    }
    return { nodes, valid: true };
  }
  // retain one null statement
  if (sqlKeyword(first, "null") && start + 1 === end) {
    return { nodes: [sqlNode("control", first.start, first.end, "null")], valid: true };
  }
  context.unsupported = true;
  return { nodes: [], valid: true };
}

// append one node to the current control frame
function sqlAppendFrame(frames, node) {
  const frame = frames.at(-1);
  // append to one active if branch
  if (frame.kind === "if") {
    sqlAddChild(frame.active, frame.role, node);
    return;
  }
  // append to one active case branch
  if (frame.kind === "case") {
    // append one else statement directly to the case
    if (frame.role === "orelse") {
      sqlAddChild(frame.node, "orelse", node);
    } else if (frame.active !== null) {
      // append one when-body statement
      sqlAddChild(frame.active, "body", node);
    }
    return;
  }
  sqlAddChild(frame.node, "body", node);
}

// parse one complete plpgsql block iteratively
function sqlPlpgsqlBody(tokens, context) {
  const bodyStructure = sqlParseIndex(tokens, 0, tokens.length);
  // reject one malformed block structure
  if (bodyStructure === null) {
    return null;
  }
  let cursor = 0;
  const declarations = [];
  // skip one optional block label
  if (sqlSymbol(tokens[cursor], "<<")) {
    // require one complete opening label
    if (
      !["identifier", "quoted-identifier"].includes(tokens[cursor + 1]?.type) ||
      !sqlSymbol(tokens[cursor + 2], ">>")
    ) {
      return null;
    }
    cursor += 3;
  }
  // parse one optional declaration section
  if (sqlKeyword(tokens[cursor], "declare")) {
    cursor += 1;
    // parse declarations through begin
    while (cursor < tokens.length && !sqlKeyword(tokens[cursor], "begin")) {
      // admit the repeated declare spelling used by retained migrations
      if (sqlKeyword(tokens[cursor], "declare")) {
        cursor += 1;
        continue;
      }
      const terminator = sqlStatementEnd(tokens, cursor, tokens.length, bodyStructure);
      // reject one unterminated declaration
      if (terminator < 0) {
        return null;
      }
      const nameToken = tokens[cursor];
      // require one positioned local name
      if (!["identifier", "quoted-identifier"].includes(nameToken?.type)) {
        return null;
      }
      let initializer = -1;
      // locate one declaration initializer
      for (let index = cursor + 1; index < terminator; index += 1) {
        // retain one default marker
        if (sqlSymbol(tokens[index], ":=") || sqlKeyword(tokens[index], "default")) {
          initializer = index;
          break;
        }
      }
      const binding = sqlNode("binding", nameToken.start, tokens[terminator - 1].end, nameToken.value);
      sqlAddChild(binding, "target", sqlNode("name", nameToken.start, nameToken.end, nameToken.value));
      const typeStart = sqlKeyword(tokens[cursor + 1], "constant") ? cursor + 2 : cursor + 1;
      const typeEnd = initializer === -1 ? terminator : initializer;
      // require one positioned declaration type
      if (typeStart >= typeEnd) {
        return null;
      }
      const type = sqlNode("expression", tokens[typeStart].start, tokens[typeEnd - 1].end);
      sqlAddChild(binding, "type", type);
      // parse one local initializer
      if (initializer !== -1) {
        const value = sqlExpression(tokens, initializer + 1, terminator, context);
        // reject one malformed local initializer
        if (value === null) {
          return null;
        }
        sqlAddChild(binding, "value", value);
      }
      declarations.push(binding);
      cursor = terminator + 1;
    }
  }
  // require one executable begin
  if (!sqlKeyword(tokens[cursor], "begin")) {
    return null;
  }
  const root = sqlNode("control", tokens[cursor].start, tokens[cursor].end, "block");
  // retain every local binding before statements
  for (const declaration of declarations) {
    sqlAddChild(root, "body", declaration);
  }
  cursor += 1;
  const frames = [{ kind: "block", node: root }];
  // parse statements through the root end
  while (cursor < tokens.length) {
    const token = tokens[cursor];
    const frame = frames.at(-1);
    // open one if statement
    if (sqlKeyword(token, "if")) {
      const thenIndex = sqlFindKeyword(tokens, cursor + 1, tokens.length, new Set(["then"]), bodyStructure);
      // require one bounded if condition
      if (thenIndex < 0) {
        return null;
      }
      const test = sqlExpression(tokens, cursor + 1, thenIndex, context);
      // reject one malformed if condition
      if (test === null) {
        return null;
      }
      const control = sqlNode("control", token.start, tokens[thenIndex].end, "if");
      sqlAddChild(control, "test", test);
      sqlAppendFrame(frames, control);
      frames.push({ active: control, chain: [control], kind: "if", role: "body" });
      cursor = thenIndex + 1;
      continue;
    }
    // continue one elsif chain
    if (sqlKeyword(token, "elsif")) {
      // require one active if frame
      if (frame.kind !== "if") {
        return null;
      }
      const thenIndex = sqlFindKeyword(tokens, cursor + 1, tokens.length, new Set(["then"]), bodyStructure);
      const test = thenIndex < 0 ? null : sqlExpression(tokens, cursor + 1, thenIndex, context);
      // reject one malformed elsif condition
      if (test === null) {
        return null;
      }
      const control = sqlNode("control", token.start, tokens[thenIndex].end, "elsif");
      sqlAddChild(control, "test", test);
      sqlAddChild(frame.active, "orelse", control);
      frame.active = control;
      frame.chain.push(control);
      frame.role = "body";
      cursor = thenIndex + 1;
      continue;
    }
    // switch one control frame to else
    if (sqlKeyword(token, "else")) {
      // require one active branch frame
      if (frame.kind !== "if" && frame.kind !== "case") {
        return null;
      }
      frame.role = "orelse";
      cursor += 1;
      continue;
    }
    // open one procedural case statement
    if (sqlKeyword(token, "case")) {
      const whenIndex = sqlFindKeyword(tokens, cursor + 1, tokens.length, new Set(["when"]), bodyStructure);
      // require at least one case arm
      if (whenIndex < 0) {
        return null;
      }
      const control = sqlNode("control", token.start, tokens[whenIndex].start, "case");
      // retain one optional case selector
      if (cursor + 1 < whenIndex) {
        const selector = sqlExpression(tokens, cursor + 1, whenIndex, context);
        // reject one malformed case selector
        if (selector === null) {
          return null;
        }
        sqlAddChild(control, "test", selector);
      }
      sqlAppendFrame(frames, control);
      frames.push({ active: null, kind: "case", node: control, role: "body" });
      cursor = whenIndex;
      continue;
    }
    // open one procedural case arm
    if (sqlKeyword(token, "when") && frame.kind === "case") {
      const thenIndex = sqlFindKeyword(tokens, cursor + 1, tokens.length, new Set(["then"]), bodyStructure);
      const test = thenIndex < 0 ? null : sqlExpression(tokens, cursor + 1, thenIndex, context);
      // reject one malformed case arm
      if (test === null) {
        return null;
      }
      const branch = sqlNode("control", token.start, tokens[thenIndex].end, "when");
      sqlAddChild(branch, "test", test);
      sqlAddChild(frame.node, "body", branch);
      frame.active = branch;
      frame.role = "body";
      cursor = thenIndex + 1;
      continue;
    }
    // open one for or while loop
    if (sqlKeyword(token, "for") || sqlKeyword(token, "while")) {
      const loopIndex = sqlFindKeyword(tokens, cursor + 1, tokens.length, new Set(["loop"]), bodyStructure);
      // require one complete loop header
      if (loopIndex < 0) {
        return null;
      }
      const control = sqlNode("control", token.start, tokens[loopIndex].end, token.value);
      // parse one for target and iterator
      if (token.value === "for") {
        const inIndex = sqlFindKeyword(tokens, cursor + 1, loopIndex, new Set(["in"]), bodyStructure);
        // require one exact for split
        if (inIndex <= cursor + 1 || inIndex + 1 >= loopIndex) {
          return null;
        }
        const targetName = tokens.slice(cursor + 1, inIndex).filter(
          // retain loop target identifiers
          (entry) => ["identifier", "quoted-identifier"].includes(entry.type),
        ).map(
          // decode one loop target
          (entry) => entry.value,
        ).join(".");
        const target = sqlNode("binding", tokens[cursor + 1].start, tokens[inIndex - 1].end, targetName || null);
        sqlAddChild(
          target,
          "target",
          sqlNode("name", tokens[cursor + 1].start, tokens[inIndex - 1].end, targetName || null),
        );
        const iteratorStatic = sqlKeyword(tokens[inIndex + 1], "select") ||
          sqlKeyword(tokens[inIndex + 1], "with");
        const iteratorState = iteratorStatic
          ? sqlStaticStatementState(tokens, inIndex + 1, loopIndex, false)
          : null;
        const iterator = iteratorStatic && iteratorState === null
          ? null
          : sqlExpression(
            tokens,
            inIndex + 1,
            loopIndex,
            context,
            iteratorState,
          );
        // reject one malformed loop iterator
        if (iterator === null) {
          return null;
        }
        sqlAddChild(control, "target", target);
        sqlAddChild(control, "iter", iterator);
      } else {
        const test = sqlExpression(tokens, cursor + 1, loopIndex, context);
        // reject one malformed while condition
        if (test === null) {
          return null;
        }
        sqlAddChild(control, "test", test);
      }
      sqlAppendFrame(frames, control);
      frames.push({ kind: "loop", node: control });
      cursor = loopIndex + 1;
      continue;
    }
    // open one unconditional loop
    if (sqlKeyword(token, "loop")) {
      const control = sqlNode("control", token.start, token.end, "loop");
      sqlAppendFrame(frames, control);
      frames.push({ kind: "loop", node: control });
      cursor += 1;
      continue;
    }
    // close one compound or root block
    if (sqlKeyword(token, "end")) {
      const next = tokens[cursor + 1];
      const compound = next?.type === "identifier" ? next.value : null;
      const semicolonIndex = compound === "if" || compound === "case" || compound === "loop"
        ? cursor + 2
        : cursor + 1;
      // close one labeled or unlabeled root block
      if (compound !== "if" && compound !== "case" && compound !== "loop") {
        // require the root frame before accepting its optional terminator
        if (frames.length !== 1 || frame.kind !== "block") {
          return null;
        }
        const labelPresent = ["identifier", "quoted-identifier"].includes(next?.type);
        let finalIndex = -1;
        // admit the reviewed final unlabeled end without an inner semicolon
        if (cursor + 1 === tokens.length) {
          finalIndex = cursor;
        } else if (sqlSymbol(next, ";") && cursor + 2 === tokens.length) {
          // admit one ordinary final root semicolon
          finalIndex = cursor + 1;
        } else if (
          labelPresent && sqlSymbol(tokens[cursor + 2], ";") &&
          cursor + 3 === tokens.length
        ) {
          // require a semicolon after one closing label
          finalIndex = cursor + 2;
        } else {
          return null;
        }
        root.end = tokens[finalIndex].end;
        return root;
      }
      // require one exact compound terminator
      if (!sqlSymbol(tokens[semicolonIndex], ";")) {
        return null;
      }
      // close one if chain
      if (compound === "if") {
        // require one matching if frame
        if (frame.kind !== "if") {
          return null;
        }
        // close every elsif node at the same compound boundary
        for (const control of frame.chain) {
          control.end = tokens[semicolonIndex].end;
        }
        frames.pop();
        cursor = semicolonIndex + 1;
        continue;
      }
      // close one case statement
      if (compound === "case") {
        // require one matching case frame
        if (frame.kind !== "case" || frame.active === null) {
          return null;
        }
        frame.node.end = tokens[semicolonIndex].end;
        frames.pop();
        cursor = semicolonIndex + 1;
        continue;
      }
      // close one loop statement
      if (compound === "loop") {
        // require one matching loop frame
        if (frame.kind !== "loop") {
          return null;
        }
        frame.node.end = tokens[semicolonIndex].end;
        frames.pop();
        cursor = semicolonIndex + 1;
        continue;
      }
      return null;
    }
    // reject branch keywords outside their owners
    if (sqlKeyword(token, "when") || sqlKeyword(token, "elsif")) {
      return null;
    }
    const terminator = sqlStatementEnd(tokens, cursor, tokens.length, bodyStructure);
    // require one complete simple statement
    if (terminator < 0) {
      return null;
    }
    const parsed = sqlSimpleStatement(tokens, cursor, terminator, context, true);
    // reject one malformed simple statement
    if (!parsed.valid) {
      return null;
    }
    // append every positioned simple node
    for (const node of parsed.nodes) {
      sqlAppendFrame(frames, node);
    }
    cursor = terminator + 1;
  }
  return null;
}

// parse one sql-language routine body
function sqlLanguageBody(tokens, context) {
  // reject one empty sql body
  if (tokens.length === 0) {
    return null;
  }
  const bodyStructure = sqlParseIndex(tokens, 0, tokens.length);
  // reject one malformed body structure
  if (bodyStructure === null) {
    return null;
  }
  const root = sqlNode("control", tokens[0].start, tokens.at(-1).end, "sql-body");
  let cursor = 0;
  // parse every static sql statement
  while (cursor < tokens.length) {
    let terminator = sqlStatementEnd(tokens, cursor, tokens.length, bodyStructure);
    // admit one final statement without a semicolon
    if (terminator === -1) {
      terminator = tokens.length;
    }
    // reject malformed groups
    if (terminator === -2 || terminator === cursor) {
      return null;
    }
    const parsed = sqlSimpleStatement(tokens, cursor, terminator, context, false);
    // require one supported static statement
    if (!parsed.valid || parsed.nodes.length === 0) {
      return null;
    }
    // retain every static statement node
    for (const node of parsed.nodes) {
      sqlAddChild(root, "body", node);
    }
    cursor = terminator === tokens.length ? terminator : terminator + 1;
  }
  return root;
}

// parse one routine parameter list
function sqlParameters(tokens, start, end, context) {
  const pairs = sqlParseIndex(tokens, start, end)?.closeByOpen ?? null;
  // reject malformed parameter delimiters
  if (pairs === null) {
    return null;
  }
  const parameters = [];
  const ranges = start === end ? [] : sqlCommaRanges(tokens, start, end, pairs);
  // parse every positioned parameter
  for (const [rangeStart, rangeEnd] of ranges) {
    // reject one empty parameter slot
    if (rangeStart >= rangeEnd) {
      return null;
    }
    let cursor = rangeStart;
    let mode = null;
    // consume one explicit mode
    if (
      tokens[cursor]?.type === "identifier" &&
      ["in", "out", "inout", "variadic"].includes(tokens[cursor].value)
    ) {
      mode = tokens[cursor];
      cursor += 1;
    }
    let defaultIndex = -1;
    // locate one default marker
    for (let index = cursor; index < rangeEnd; index += 1) {
      // retain one exact default marker
      if (sqlKeyword(tokens[index], "default") || sqlSymbol(tokens[index], "=")) {
        defaultIndex = index;
        break;
      }
    }
    const declarationEnd = defaultIndex === -1 ? rangeEnd : defaultIndex;
    // require at least one type token
    if (cursor >= declarationEnd) {
      return null;
    }
    const named = declarationEnd - cursor > 1 &&
      ["identifier", "quoted-identifier"].includes(tokens[cursor].type);
    const name = named ? tokens[cursor].value : null;
    const typeStart = named ? cursor + 1 : cursor;
    // require one positioned type
    if (typeStart >= declarationEnd) {
      return null;
    }
    const parameter = sqlNode("parameter", tokens[rangeStart].start, tokens[rangeEnd - 1].end, name);
    // retain one explicit mode keyword
    if (mode !== null) {
      sqlAddChild(parameter, "args", sqlNode("control", mode.start, mode.end, mode.value));
    }
    const type = sqlNode("expression", tokens[typeStart].start, tokens[declarationEnd - 1].end);
    sqlAddChild(parameter, "type", type);
    // parse one positioned parameter default
    if (defaultIndex !== -1) {
      const value = sqlExpression(tokens, defaultIndex + 1, rangeEnd, context);
      // reject one malformed parameter default
      if (value === null) {
        return null;
      }
      sqlAddChild(parameter, "value", value);
    }
    parameters.push(parameter);
  }
  return parameters;
}

// recognize one complete outer routine declaration
function sqlRoutineHeader(tokens, start, end) {
  let cursor = start;
  let routineKind = null;
  // parse one do declaration
  if (sqlKeyword(tokens[cursor], "do")) {
    routineKind = "do";
    cursor += 1;
  } else if (sqlKeyword(tokens[cursor], "create")) {
    // parse one optional or-replace clause
    cursor += 1;
    if (sqlKeyword(tokens[cursor], "or") && sqlKeyword(tokens[cursor + 1], "replace")) {
      cursor += 2;
    }
    // require one function or procedure kind
    if (sqlKeyword(tokens[cursor], "function") || sqlKeyword(tokens[cursor], "procedure")) {
      routineKind = tokens[cursor].value;
      cursor += 1;
    } else {
      return { status: "ignore" };
    }
  } else {
    return { status: "ignore" };
  }
  let owner = "<anonymous>";
  let parameterStart = cursor;
  let parameterEnd = cursor;
  // parse one declared routine name and parameter list
  if (routineKind !== "do") {
    const names = [];
    // consume one qualified routine name
    while (cursor < end) {
      const token = tokens[cursor];
      // retain one name segment
      if (["identifier", "quoted-identifier"].includes(token?.type)) {
        names.push(token.value);
        cursor += 1;
        // consume one qualification dot
        if (sqlSymbol(tokens[cursor], ".")) {
          cursor += 1;
          continue;
        }
        break;
      }
      return { status: "unsupported" };
    }
    // require one valid public owner and parameter opener
    if (
      names.length === 0 || !sqlSymbol(tokens[cursor], "(") ||
      names.some((name) => !/^[A-Za-z_$][A-Za-z0-9_$]*$/u.test(name)) ||
      Buffer.byteLength(names.join("."), "utf8") > 192
    ) {
      return { status: "unsupported" };
    }
    owner = names.join(".");
    parameterStart = cursor + 1;
    let depth = 1;
    cursor += 1;
    // locate one exact parameter close
    while (cursor < end && depth > 0) {
      // track nested type parentheses
      if (sqlSymbol(tokens[cursor], "(")) {
        depth += 1;
      } else if (sqlSymbol(tokens[cursor], ")")) {
        // close one type or parameter parenthesis
        depth -= 1;
      }
      cursor += 1;
    }
    // reject one unclosed parameter list
    if (depth !== 0) {
      return { status: "unsupported" };
    }
    parameterEnd = cursor - 1;
  }
  let bodyToken = null;
  let language = routineKind === "do" ? "plpgsql" : null;
  let languageDeclared = false;
  let returnsRange = null;
  let sawReturns = false;
  const optionStarts = new Set([
    "as", "language", "transform", "window", "immutable", "stable", "volatile",
    "leakproof", "called", "returns", "strict", "security", "parallel", "cost",
    "rows", "support", "set",
  ]);
  // parse every outer routine option
  while (cursor < end) {
    const token = tokens[cursor];
    // parse one do body without as
    if (routineKind === "do" && token.type === "dollar-body") {
      // reject duplicate bodies
      if (bodyToken !== null) {
        return { status: "unsupported" };
      }
      bodyToken = token;
      cursor += 1;
      continue;
    }
    // parse one as body option
    if (sqlKeyword(token, "as")) {
      // require one exact dollar body
      if (bodyToken !== null || tokens[cursor + 1]?.type !== "dollar-body") {
        return { status: "unsupported" };
      }
      bodyToken = tokens[cursor + 1];
      cursor += 2;
      continue;
    }
    // parse one language option
    if (sqlKeyword(token, "language")) {
      const name = tokens[cursor + 1];
      // require one exact language name
      if (
        languageDeclared || !name ||
        !["identifier", "quoted-identifier"].includes(name.type)
      ) {
        return { status: "unsupported" };
      }
      languageDeclared = true;
      language = name.value;
      cursor += 2;
      continue;
    }
    // consume returns null on null input as an option
    if (
      sqlKeyword(token, "returns") && sqlKeyword(tokens[cursor + 1], "null") &&
      sqlKeyword(tokens[cursor + 2], "on") && sqlKeyword(tokens[cursor + 3], "null") &&
      sqlKeyword(tokens[cursor + 4], "input")
    ) {
      cursor += 5;
      continue;
    }
    // parse one return type
    if (sqlKeyword(token, "returns")) {
      // reject procedure returns or duplicate return clauses
      if (routineKind !== "function" || sawReturns) {
        return { status: "unsupported" };
      }
      sawReturns = true;
      const typeStart = cursor + 1;
      cursor = typeStart;
      let depth = 0;
      // consume one bounded return type
      while (cursor < end) {
        const current = tokens[cursor];
        // track return-type parentheses
        if (sqlSymbol(current, "(")) {
          depth += 1;
        } else if (sqlSymbol(current, ")")) {
          // close one return-type parenthesis
          depth -= 1;
        }
        // stop before the next top-level option
        if (
          depth === 0 && cursor > typeStart && current.type === "identifier" &&
          optionStarts.has(current.value)
        ) {
          break;
        }
        cursor += 1;
      }
      // require one positioned return type
      if (cursor <= typeStart) {
        return { status: "unsupported" };
      }
      returnsRange = [typeStart, cursor];
      continue;
    }
    // consume one volatility or strictness flag
    if (
      token.type === "identifier" &&
      ["window", "immutable", "stable", "volatile", "leakproof", "strict"].includes(token.value)
    ) {
      cursor += 1;
      continue;
    }
    // consume one paired option
    if (
      sqlKeyword(token, "security") || sqlKeyword(token, "parallel") ||
      sqlKeyword(token, "cost") || sqlKeyword(token, "rows") || sqlKeyword(token, "support")
    ) {
      // require one option value
      if (cursor + 1 >= end) {
        return { status: "unsupported" };
      }
      cursor += 2;
      continue;
    }
    // consume one null-input phrase
    if (sqlKeyword(token, "called")) {
      // require called on null input
      if (
        !sqlKeyword(tokens[cursor + 1], "on") || !sqlKeyword(tokens[cursor + 2], "null") ||
        !sqlKeyword(tokens[cursor + 3], "input")
      ) {
        return { status: "unsupported" };
      }
      cursor += 4;
      continue;
    }
    // consume one set clause through the next option
    if (sqlKeyword(token, "set")) {
      cursor += 1;
      // consume the configuration clause
      while (
        cursor < end &&
        !(tokens[cursor].type === "identifier" && optionStarts.has(tokens[cursor].value))
      ) {
        cursor += 1;
      }
      continue;
    }
    return { status: "unsupported" };
  }
  // require the exact routine essentials
  if (
    bodyToken === null || language === null ||
    routineKind === "function" && !sawReturns ||
    routineKind === "procedure" && sawReturns
  ) {
    return { status: "unsupported" };
  }
  return {
    bodyToken,
    language,
    owner,
    parameterEnd,
    parameterStart,
    returnsRange,
    routineKind,
    status: "supported",
  };
}

// build framed semantic facts without recursive hashing
function sqlFacts(root, language, source, spanIndex) {
  const ordered = [];
  const facts = new Map();
  const discovered = new Set();
  const stack = [{ node: root, visited: false }];
  // build every child before its parent
  while (stack.length > 0) {
    const entry = stack.pop();
    // frame one finished node
    if (entry.visited) {
      const childEdges = entry.node.children.map(
        // replace one semantic child with its private fact id
        (edge) => ({
          childFactId: facts.get(edge.child).factId,
          index: edge.index,
          role: edge.role,
        }),
      );
      facts.set(entry.node, syntheticFact({
        associations: [],
        attributes: entry.node.attributes,
        childEdges,
        factKind: entry.node.factKind,
        language,
        source,
        span: occurrenceSpan(spanIndex, entry.node.start, entry.node.end),
      }));
      continue;
    }
    // skip one already scheduled node
    if (discovered.has(entry.node)) {
      continue;
    }
    discovered.add(entry.node);
    ordered.push(entry.node);
    stack.push({ node: entry.node, visited: true });
    // schedule children in reverse source order
    for (let index = entry.node.children.length - 1; index >= 0; index -= 1) {
      stack.push({ node: entry.node.children[index].child, visited: false });
    }
  }
  return ordered.map(
    // retain source-preorder facts
    (node) => facts.get(node),
  );
}

// split one outer sql token stream into complete statements
function sqlOuterStatements(tokens) {
  const ranges = [];
  let start = 0;
  let parenDepth = 0;
  let bracketDepth = 0;
  // scan every outer token
  for (let index = 0; index < tokens.length; index += 1) {
    const token = tokens[index];
    // track one outer parenthesis
    if (sqlSymbol(token, "(")) {
      parenDepth += 1;
    } else if (sqlSymbol(token, ")")) {
      // close one outer parenthesis
      parenDepth -= 1;
    } else if (sqlSymbol(token, "[")) {
      // track one outer bracket
      bracketDepth += 1;
    } else if (sqlSymbol(token, "]")) {
      // close one outer bracket
      bracketDepth -= 1;
    }
    // reject one malformed outer delimiter
    if (parenDepth < 0 || bracketDepth < 0) {
      return null;
    }
    // close one complete outer statement
    if (sqlSymbol(token, ";") && parenDepth === 0 && bracketDepth === 0) {
      // retain one nonempty statement range
      if (start < index) {
        ranges.push({ end: index, start, terminator: index });
      }
      start = index + 1;
    }
  }
  // reject one unclosed delimiter
  if (parenDepth !== 0 || bracketDepth !== 0) {
    return null;
  }
  // retain one final unterminated range for unsupported adjudication
  if (start < tokens.length) {
    ranges.push({ end: tokens.length, start, terminator: null });
  }
  return ranges;
}

// create one linked plpgsql occurrence draft
function sqlProviderDraft(provider, source, owner, container, bodyStart, spanIndex) {
  return nativeDraft({
    confidence: "expanded-direct",
    container: {
      containerOccurrenceId: container.id,
      containerSpan: container.span,
      decodedSpan: {
        endOffsetUtf16: provider.end - bodyStart,
        startOffsetUtf16: provider.start - bodyStart,
      },
      parentSourceLocator: source.path,
    },
    countsAsClockCandidate: provider.kind !== "parse-only",
    kind: provider.kind,
    language: "plpgsql",
    normalizedExpressionSha256: expressionDigest(
      "plpgsql",
      "sql-token-sequence/v1",
      provider.normalized,
    ),
    ownerSymbol: owner,
    parentOccurrenceId: container.id,
    sourceLocator: source.path,
    span: occurrenceSpan(spanIndex, provider.start, provider.end),
    surface: provider.surface,
    syntaxKind: "plpgsql-expression",
  });
}

// analyze one physical or derived sql source
function analyzeSql(source, spanIndex) {
  const syntheticPath = source.sourceKind === "derived"
    ? `__derived__/clock/${source.derivedId}.sql`
    : source.path;
  const report = extractBackendClockOccurrences({
    sourcePath: syntheticPath,
    sourceText: source.sourceText,
  });
  const physical = source.sourceKind === "physical"
    ? report.occurrences.map(
      // preserve one physical-v1 record
      (record) => projectPhysical(record),
    )
    : [];
  const physicalContainersBySpan = new Map();
  // index every physical dollar container once in source order
  for (const occurrence of physical) {
    // skip one ordinary physical provider
    if (occurrence.surface !== "postgres.dollar-quoted-body") {
      continue;
    }
    const key = `${occurrence.span.startOffsetUtf16}:${occurrence.span.endOffsetUtf16}`;
    const candidates = physicalContainersBySpan.get(key) ?? [];
    candidates.push(occurrence);
    physicalContainersBySpan.set(key, candidates);
  }
  const semanticFacts = [];
  const drafts = [];
  let unsupported = report.diagnostics.some(
    // retain extractor gaps other than adjudicable dollar bodies
    (diagnostic) => diagnostic.code !== "sql.dollar-body-unresolved",
  );
  const providerRecords = source.sourceKind === "physical"
    ? physical.filter(
      // retain ordinary physical providers only
      (occurrence) => occurrence.kind !== "non-provider-container",
    ).map(
      // retain one physical provider span
      (occurrence) => occurrence.span,
    )
    : report.occurrences.filter(
      // retain derived private providers without public identity projection
      (record) => record.surface !== "postgres.dollar-quoted-body",
    ).map(
      // retain one derived provider span
      (record) => record.physicalSpan,
    );
  // retain one private fact per ordinary provider
  for (const span of providerRecords) {
    semanticFacts.push(syntheticFact({
      associations: [],
      attributes: {},
      childEdges: [],
      factKind: "clock-expression",
      language: "sql",
      source,
      span,
    }));
  }
  const tokens = scanSqlTokens(source.sourceText);
  // reject one malformed outer token stream
  if (tokens === null) {
    return {
      diagnostics: [unsupportedDiagnostic()],
      drafts,
      physical,
      semanticFacts,
    };
  }
  const statements = sqlOuterStatements(tokens);
  // reject one malformed outer statement structure
  if (statements === null) {
    return {
      diagnostics: [unsupportedDiagnostic()],
      drafts,
      physical,
      semanticFacts,
    };
  }
  const adjudicatedBodies = new Set();
  // parse every possible outer routine statement
  for (const statement of statements) {
    const header = sqlRoutineHeader(tokens, statement.start, statement.end);
    // skip one ordinary sql statement
    if (header.status === "ignore") {
      continue;
    }
    // retain one unsupported declaration gap
    if (header.status !== "supported" || statement.terminator === null) {
      unsupported = true;
      continue;
    }
    const bodyToken = header.bodyToken;
    adjudicatedBodies.add(`${bodyToken.start}:${bodyToken.end}`);
    const headerContext = {
      providerNodesByRange: new Map(),
      providers: [],
      sourceText: source.sourceText,
      unsupported: false,
    };
    const parameters = sqlParameters(
      tokens,
      header.parameterStart,
      header.parameterEnd,
      headerContext,
    );
    // reject malformed parameters before body parsing
    if (parameters === null) {
      unsupported = true;
      continue;
    }
    const bodyText = source.sourceText.slice(bodyToken.bodyStart, bodyToken.bodyEnd);
    const bodyTokens = scanSqlTokens(bodyText, bodyToken.bodyStart);
    // reject one malformed body token stream
    if (bodyTokens === null) {
      unsupported = true;
      continue;
    }
    const context = {
      providerNodesByRange: new Map(),
      providers: [],
      sourceText: source.sourceText,
      unsupported: false,
    };
    let body = null;
    // parse one supported procedural body
    if (header.language === "plpgsql") {
      body = sqlPlpgsqlBody(bodyTokens, context);
    } else if (header.language === "sql" && header.routineKind !== "do") {
      // parse one known sql-language body privately
      body = sqlLanguageBody(bodyTokens, context);
    } else {
      // retain one unknown language gap
      unsupported = true;
      continue;
    }
    // reject one malformed routine transactionally
    if (body === null) {
      unsupported = true;
      continue;
    }
    const routine = sqlNode(
      "function",
      tokens[statement.start].start,
      tokens[statement.terminator].end,
      header.owner,
    );
    // retain every positioned parameter edge
    for (const parameter of parameters) {
      sqlAddChild(routine, "args", parameter);
    }
    // retain one positioned return type
    if (header.returnsRange !== null) {
      const [returnStart, returnEnd] = header.returnsRange;
      sqlAddChild(
        routine,
        "returns",
        sqlNode("expression", tokens[returnStart].start, tokens[returnEnd - 1].end),
      );
    }
    sqlAddChild(routine, "body", body);
    const language = header.language === "plpgsql" ? "plpgsql" : "sql";
    appendValues(semanticFacts, sqlFacts(routine, language, source, spanIndex));
    // retain one sanitized header or body partial-semantics gap
    if (headerContext.unsupported || context.unsupported) {
      unsupported = true;
    }
    // public expansion belongs only to physical plpgsql containers
    if (source.sourceKind !== "physical" || header.language !== "plpgsql") {
      continue;
    }
    const containers = physicalContainersBySpan.get(
      `${bodyToken.start}:${bodyToken.end}`,
    ) ?? [];
    // reject one conflicting physical container mapping
    if (containers.length > 1) {
      throw new Error("binding.source-conflict");
    }
    // retain one missing physical container as unsupported
    if (containers.length === 0) {
      unsupported = true;
      continue;
    }
    // emit every parsed executable body provider
    for (const provider of context.providers) {
      // reject one provider outside the decoded body range
      if (
        provider.start < bodyToken.bodyStart || provider.end > bodyToken.bodyEnd ||
        provider.end <= provider.start
      ) {
        unsupported = true;
        continue;
      }
      drafts.push(sqlProviderDraft(
        provider,
        source,
        header.owner,
        containers[0],
        bodyToken.bodyStart,
        spanIndex,
      ));
    }
  }
  // retain unresolved dollar containers as one sanitized gap
  if (tokens.some(
    // locate one unadjudicated dollar body
    (token) => token.type === "dollar-body" &&
      !adjudicatedBodies.has(`${token.start}:${token.end}`),
  )) {
    unsupported = true;
  }
  return {
    diagnostics: unsupported ? [unsupportedDiagnostic()] : [],
    drafts,
    physical,
    semanticFacts,
  };
}


// project one derived javascript source
function projectDerived(derived, container) {
  const syntheticSourcePath = `__derived__/clock/${derived.derivedId}.mjs`;
  const report = extractBackendClockOccurrences({
    sourcePath: syntheticSourcePath,
    sourceText: derived.sourceText,
  });
  const drafts = report.occurrences.map(
    // bridge one frozen v1 expression digest
    (record) => nativeDraft({
      confidence: record.confidence,
      container: {
        containerOccurrenceId: container.id,
        containerSpan: container.span,
        decodedSpan: {
          endOffsetUtf16: record.physicalSpan.endOffsetUtf16,
          startOffsetUtf16: record.physicalSpan.startOffsetUtf16,
        },
        parentSourceLocator: derived.parentPath,
      },
      countsAsClockCandidate: record.countsAsClockCandidate,
      extractorOrigin: { record, syntheticSourcePath },
      kind: record.kind,
      language: "javascript",
      normalizedExpressionSha256: expressionDigest(
        "javascript",
        "derived-javascript-v1-digest/v1",
        record.normalizedExpressionSha256,
      ),
      ownerSymbol: record.enclosingSymbol,
      parentOccurrenceId: container.id,
      sourceLocator: derived.path,
      span: record.physicalSpan,
      surface: record.surface,
      syntaxKind: record.syntaxKind,
    }),
  );
  return { drafts, unsupported: report.diagnostics.length > 0 };
}

// validate and analyze one explicit nonmodule batch
export async function analyzeBackendNonmoduleSourcesBatch(...argumentsList) {
  // require exactly one envelope
  if (argumentsList.length !== 1) {
    throw new TypeError("frontend accepts one input record");
  }
  const input = exactDataRecord(argumentsList[0], ["sources"], "frontend input");
  const sourceValues = denseArray(input.sources, "frontend sources");

  // require a nonempty explicit batch
  if (sourceValues.length === 0) {
    throw new TypeError("frontend sources must not be empty");
  }
  const validatedSources = [];
  let totalBytes = 0;

  // validate every explicit private source
  for (const value of sourceValues) {
    const source = validatePrivateSource(value);
    validatedSources.push(source);
    totalBytes += source.byteLength;
  }

  // enforce the explicit batch byte cap
  if (totalBytes > BATCH_BYTE_CAP) {
    throw new TypeError("frontend batch byte bound exceeded");
  }

  const sources = deduplicateInputSources(validatedSources);
  validateDerivedCustody(sources);
  const spanIndexes = new Map();

  // reuse one immutable line index per detached source
  function spanIndexFor(source) {
    let index = spanIndexes.get(source);
    // build one absent source index
    if (index === undefined) {
      index = buildOccurrenceSpanIndex(source.sourceText);
      spanIndexes.set(source, index);
    }
    return index;
  }

  const languages = [];

  // retain present languages in fixed order
  for (const language of SUPPORTED_LANGUAGES) {
    let present = false;

    // inspect each validated source language
    for (const source of sources) {
      // record one present language
      if (source.language === language) {
        present = true;
        break;
      }
    }
    // append one present language
    if (present) {
      languages.push(language);
    }
  }

  const batchIdentity = [];

  // retain only public batch identity fields
  for (const source of sources) {
    batchIdentity.push({
      language: source.language,
      path: source.path,
      rawSha256: source.rawSha256,
    });
  }
  const pythonSources = sources.filter(
    // retain one python input
    (source) => source.language === "python",
  );
  const python = pythonSources.length === 0
    ? { files: [], runtime: null }
    : await analyzePythonSources(pythonSources);
  const semanticFacts = [];
  const diagnostics = [];
  const nativeDrafts = [];
  const physicalOccurrences = [];
  const discoveredSources = [];
  const shellSources = sources.filter(
    // retain every shell source for one batch-wide definition universe
    (source) => source.language === "shell",
  );
  const shellProfiles = shellBuildFunctionProfiles(shellSources);
  // project every validated helper file
  for (const file of python.files) {
    const projection = projectPythonFile(file, spanIndexFor(file.source));
    appendValues(semanticFacts, projection.semanticFacts);
    appendDiagnostics(diagnostics, projection.diagnostics);
    appendValues(nativeDrafts, projection.drafts);
    appendValues(discoveredSources, projection.derivedSources);
  }
  // analyze every non-python input without execution
  for (const source of sources) {
    // analyze one shell source
    if (source.language === "shell") {
      const projection = analyzeShell(
        source,
        spanIndexFor(source),
        shellProfiles.profiles.get(source.path) ?? null,
        shellProfiles.plansByPath.get(source.path),
        shellProfiles.lexedByPath.get(source.path),
      );
      appendValues(semanticFacts, projection.semanticFacts);
      appendDiagnostics(diagnostics, projection.diagnostics);
      appendValues(nativeDrafts, projection.drafts);
      appendValues(discoveredSources, projection.derivedSources);
    }
    // analyze one sql source
    if (source.language === "sql") {
      const projection = analyzeSql(source, spanIndexFor(source));
      appendValues(semanticFacts, projection.semanticFacts);
      appendDiagnostics(diagnostics, projection.diagnostics);
      appendValues(nativeDrafts, projection.drafts);
      appendValues(physicalOccurrences, projection.physical);
    }
  }
  const derivedSources = admitDiscoveredSources(sources, discoveredSources);
  const nativeOccurrences = finalizeNative(nativeDrafts);
  const containersBySource = new Map();

  // index every finalized node-e container once
  for (const occurrence of nativeOccurrences) {
    // skip one ordinary native occurrence
    if (
      occurrence.surface !== "container.python-node-e" &&
      occurrence.surface !== "container.shell-node-e"
    ) {
      continue;
    }
    let byStart = containersBySource.get(occurrence.sourceLocator);

    // create one source coordinate index
    if (byStart === undefined) {
      byStart = new Map();
      containersBySource.set(occurrence.sourceLocator, byStart);
    }
    let byEnd = byStart.get(occurrence.span.startOffsetUtf16);

    // create one start coordinate index
    if (byEnd === undefined) {
      byEnd = new Map();
      byStart.set(occurrence.span.startOffsetUtf16, byEnd);
    }
    const existing = byEnd.get(occurrence.span.endOffsetUtf16);

    // count one repeated exact container tuple
    if (existing !== undefined) {
      existing.count += 1;
    } else {
      byEnd.set(occurrence.span.endOffsetUtf16, {
        count: 1,
        firstOccurrence: occurrence,
      });
    }
  }
  const derivedDrafts = [];
  // map every private derived source to its native container
  for (const derived of derivedSources) {
    const container = containersBySource.get(derived.parentPath)
      ?.get(derived.containerSpan.startUtf16)
      ?.get(derived.containerSpan.endUtf16);
    // require exactly one parent container
    if (container?.count !== 1) {
      throw new Error("binding.source-conflict");
    }
    const projection = projectDerived(derived, container.firstOccurrence);
    appendValues(derivedDrafts, projection.drafts);
    // retain one sanitized extractor gap
    if (projection.unsupported) {
      appendDiagnostics(diagnostics, [unsupportedDiagnostic()]);
    }
  }
  const occurrences = [
    ...physicalOccurrences,
    ...nativeOccurrences,
    ...finalizeNative(derivedDrafts),
  ];
  occurrences.sort(compareOccurrences);
  diagnostics.sort(
    // order sanitized diagnostics deterministically
    (left, right) => compareText(left.code, right.code),
  );
  const result = {
    analysisProfile: {
      batchInputSha256: sha256(canonicalJson(batchIdentity)),
      contractVersion: "weather-backend-nonmodule-analysis/v1",
      languages,
      // retain the actual helper call count
      pythonHelperCalls: pythonSources.length === 0 ? 0 : 1,
      pythonRuntime: python.runtime,
    },
    derivedSources,
    diagnostics,
    occurrences,
    semanticFacts,
  };
  return freezeRecursively(result);
}
