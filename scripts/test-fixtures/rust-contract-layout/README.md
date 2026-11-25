# Contract compiler fixtures

These opt-in fixtures compare selected structural constraints in the original
TypeScript declarations and generated Rust declarations. They cover air-quality
field presence, a required-but-nullable channel, exact forecast days, bootstrap
replicates, the rain threshold and null baseline score, rain-control byte
storage, date-or-string database rows, recursive JSON values, and both installed
receipt versions. Both compilers must accept the positive fixture and reject
each of the eleven independently labeled mismatches at its marked source line.

The TypeScript check uses the pinned compiler, original declarations, and
existing PostgreSQL driver row marker in a closed in-memory program. It does not
emit JavaScript. The Rust check consumes metadata from the unchanged generated
declarations and emits only metadata for the positive fixture. Neither fixture
is evaluated, linked to an executable, or run. The date and recursive JSON
fixtures preserve separate storage alternatives and UTF-16 code units, including
an isolated surrogate. These checks do not establish timestamp normalization,
receipt validation, numeric bounds outside the chosen literals, canonical
bytes, or wire parity.

This directory is not wired into project builds or CI. Under the repository's
bounded offline resource rules, run `node scripts/test-fixtures/rust-contract-layout/check-typescript.mjs`.
For Rust, compile `rust/crates/weather-contracts/src/generated.rs` with the
approved `rustc`, edition 2024, crate name `weather_contracts_generated`, crate
type `lib`, and `--emit=metadata` to
`libweather_contracts_generated.rmeta` in a disposable directory. Pass the
absolute compiler path and that directory to
`node scripts/test-fixtures/rust-contract-layout/check-rust.mjs`. Delete the
temporary directory afterward; no crate, dependency, serializer, or Cargo
workspace is needed.

## Workspace ordering preflight

`node scripts/test-fixtures/rust-contract-layout/check-build-order.mjs` reads
the current workspace manifests and calls the existing sorter with simulated
type-only contracts edges. It does not invoke a compiler, create the contracts
workspace, or write package manifests. The simulation checks that direct
dependency edges order contracts before their consumers, and that an indirect
ordering success does not replace the required direct dependency declaration.
The seven source-owning workspaces are simulated consumers for this stress
case, not a decision about which workspaces will import the future package.

Simply re-exporting types back from a workspace that imports the contracts
package would create a dependency cycle. Integration must keep each canonical
declaration in an acyclic location without duplicating it or changing the
producer contract. The simulation proves ordering for the tested graph, not
that the future workspace, type-only lint, or build integration already exists.

## Type-only package edge preflight

`node scripts/test-fixtures/rust-contract-layout/check-contract-package-edge.mjs`
checks examples of the planned contracts import rule using the pinned
TypeScript parser and in-memory JavaScript output. Whole `import type` and
`export type` declarations disappear; an inline type specifier can leave
`import {}` or `export {} from` and still load the module. The fixture checks
the listed literal import forms only. It does not resolve computed module
names or package exports, create a package, or wire the rule into lint or CI.
