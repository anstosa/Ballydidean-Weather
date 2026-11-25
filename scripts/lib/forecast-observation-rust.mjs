import ts from "typescript";

const METRICS = "FORECAST_ADJUSTMENT_METRICS";
const PROVIDER_FAMILIES = "FORECAST_OBSERVATION_PROVIDER_FAMILIES";
const STATION_KEYS = "FORECAST_OBSERVATION_STATION_KEYS";
const STATIONS = "FORECAST_OBSERVATION_STATIONS";
const STATION_FIELDS = [
  "acceptedSourceKeys",
  "distanceMeters",
  "eligibleMetrics",
  "key",
  "latitude",
  "longitude",
  "nearestRank",
  "providerFamily",
  "unnormalizedSpatialWeight",
];

// render the retained observation station table without evaluating TypeScript
export function renderRetainedForecastObservationStations(sourceFile, bindings) {
  requireBindings(bindings);
  requireParsedSource(sourceFile);
  const metrics = readConstStringTuple(sourceFile, METRICS, {
    satisfies: "MetricName",
  });
  const providerFamilies = readConstStringTuple(sourceFile, PROVIDER_FAMILIES);
  const stationKeys = readConstStringTuple(sourceFile, STATION_KEYS);
  const stations = readStationCatalog(sourceFile);

  // pin the complete generated vocabularies and current station population
  if (
    metrics.length !== 5 ||
    providerFamilies.length !== 4 ||
    stationKeys.length !== 11 ||
    stations.length !== 11 ||
    new Set(metrics).size !== metrics.length ||
    new Set(providerFamilies).size !== providerFamilies.length ||
    new Set(stationKeys).size !== stationKeys.length
  ) {
    throw new Error("forecast observation retained vocabularies are incomplete");
  }

  const rosterKeys = stations.map((station) => station.key);
  const ranks = stations.map((station) => station.nearestRank).sort((left, right) => left - right);

  // bind every object to the extracted literal vocabularies without reordering the roster
  if (
    new Set(rosterKeys).size !== rosterKeys.length ||
    [...rosterKeys].sort().join("\0") !== [...stationKeys].sort().join("\0") ||
    ranks.some((rank, index) => rank !== index + 1) ||
    stations.some(
      (station) =>
        !providerFamilies.includes(station.providerFamily) ||
        station.eligibleMetrics !== METRICS,
    )
  ) {
    throw new Error("forecast observation station roster is not closed");
  }

  const metricValues = metrics.map((wire) => enumValue(bindings, "ForecastAdjustmentMetric", wire));
  const stationRows = stations.map((station) => renderStation(station, bindings));
  return `pub mod retained_forecast_observation {
    use std::sync::OnceLock;

    use super::{
        ForecastAdjustmentMetric, ForecastObservationProviderFamily,
        ForecastObservationStation, ForecastObservationStationKey,
    };

    /// Borrowed source-exact station material retained without per-fit allocation.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct RetainedForecastObservationStation {
        pub accepted_source_keys: &'static [&'static str],
        pub distance_meters: f64,
        pub eligible_metrics: &'static [ForecastAdjustmentMetric],
        pub key: ForecastObservationStationKey,
        pub latitude: f64,
        pub longitude: f64,
        pub nearest_rank: u64,
        pub provider_family: ForecastObservationProviderFamily,
        pub unnormalized_spatial_weight: f64,
    }

    const ELIGIBLE_METRICS: &[ForecastAdjustmentMetric] = &[
${metricValues.map((value) => `        ${value},`).join("\n")}
    ];

    /// Source-order station roster with exact binary64 material.
    pub static STATIONS: &[RetainedForecastObservationStation] = &[
${stationRows.join("\n")}
    ];

    /// Materialize the original generated DTO catalog once for existing consumers.
    pub fn station_catalog() -> &'static [ForecastObservationStation] {
        static CATALOG: OnceLock<Box<[ForecastObservationStation]>> = OnceLock::new();
        CATALOG.get_or_init(|| {
            STATIONS
                .iter()
                .map(|station| ForecastObservationStation {
                    acceptedSourceKeys: station
                        .accepted_source_keys
                        .iter()
                        .map(|source| (*source).to_owned())
                        .collect(),
                    distanceMeters: station.distance_meters,
                    eligibleMetrics: station.eligible_metrics.to_vec(),
                    key: station.key,
                    latitude: station.latitude,
                    longitude: station.longitude,
                    nearestRank: station.nearest_rank,
                    providerFamily: station.provider_family,
                    unnormalizedSpatialWeight: station.unnormalized_spatial_weight,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice()
        })
    }
}
`;
}

// require the existing generator's only two rendering capabilities
function requireBindings(bindings) {
  if (
    bindings === null ||
    typeof bindings !== "object" ||
    typeof bindings.variant !== "function" ||
    typeof bindings.rustString !== "function"
  ) {
    throw new Error("forecast observation Rust bindings are incomplete");
  }
}

// require a parsed TypeScript source file with no syntax diagnostics
function requireParsedSource(sourceFile) {
  if (
    sourceFile === null ||
    typeof sourceFile !== "object" ||
    !Array.isArray(sourceFile.statements) ||
    sourceFile.parseDiagnostics?.length !== 0
  ) {
    throw new Error("forecast observation source must be a parsed TypeScript file");
  }
}

// read one direct exported const tuple and its exact assertion shape
function readConstStringTuple(sourceFile, name, options = {}) {
  const initializer = exportedConstInitializer(sourceFile, name);
  const asserted = options.satisfies === undefined
    ? requireConstAssertion(initializer, name)
    : requireSatisfiesConstArray(initializer, options.satisfies, name);

  // admit only direct string literals with no spread or expression syntax
  const values = asserted.elements.map((element) => {
    if (!ts.isStringLiteral(element)) {
      throw new Error(`${name} must contain only direct string literals`);
    }
    return element.text;
  });

  // reject empty or duplicated literal vocabularies at their source boundary
  if (values.length === 0 || new Set(values).size !== values.length) {
    throw new Error(`${name} must be a nonempty unique string tuple`);
  }
  return values;
}

// read the source-exact object roster under its satisfies assertion
function readStationCatalog(sourceFile) {
  const initializer = exportedConstInitializer(sourceFile, STATIONS);
  const array = requireSatisfiesConstArray(initializer, "ForecastObservationStation", STATIONS);
  return array.elements.map((element, index) => readStation(sourceFile, element, index));
}

// read one closed station object without evaluating any property expression
function readStation(sourceFile, element, index) {
  if (!ts.isObjectLiteralExpression(element)) {
    throw new Error(`${STATIONS}[${index}] must be a direct object literal`);
  }
  const properties = new Map();

  // retain only direct identifier-keyed property assignments
  for (const property of element.properties) {
    if (
      !ts.isPropertyAssignment(property) ||
      !ts.isIdentifier(property.name) ||
      properties.has(property.name.text)
    ) {
      throw new Error(`${STATIONS}[${index}] contains dynamic or duplicate syntax`);
    }
    properties.set(property.name.text, property.initializer);
  }

  // require every declared station field and no extension
  if (
    properties.size !== STATION_FIELDS.length ||
    STATION_FIELDS.some((field) => !properties.has(field))
  ) {
    throw new Error(`${STATIONS}[${index}] fields are not closed`);
  }

  const eligible = properties.get("eligibleMetrics");

  // retain the sole local metric tuple by direct identifier reference
  if (!ts.isIdentifier(eligible) || eligible.text !== METRICS) {
    throw new Error(`${STATIONS}[${index}].eligibleMetrics must use ${METRICS}`);
  }

  return {
    acceptedSourceKeys: stringArray(properties.get("acceptedSourceKeys"), `${index}.acceptedSourceKeys`),
    distanceMeters: finiteNumber(sourceFile, properties.get("distanceMeters"), `${index}.distanceMeters`),
    eligibleMetrics: eligible.text,
    key: stringLiteral(properties.get("key"), `${index}.key`),
    latitude: finiteNumber(sourceFile, properties.get("latitude"), `${index}.latitude`),
    longitude: finiteNumber(sourceFile, properties.get("longitude"), `${index}.longitude`),
    nearestRank: unsignedInteger(sourceFile, properties.get("nearestRank"), `${index}.nearestRank`),
    providerFamily: stringLiteral(properties.get("providerFamily"), `${index}.providerFamily`),
    unnormalizedSpatialWeight: finiteNumber(
      sourceFile,
      properties.get("unnormalizedSpatialWeight"),
      `${index}.unnormalizedSpatialWeight`,
    ),
  };
}

// locate exactly one direct exported const binding
function exportedConstInitializer(sourceFile, name) {
  const matches = [];

  // inspect top-level declarations only
  for (const statement of sourceFile.statements) {
    if (!ts.isVariableStatement(statement)) {
      continue;
    }

    // retain only declarations bearing the requested identifier
    for (const declaration of statement.declarationList.declarations) {
      if (ts.isIdentifier(declaration.name) && declaration.name.text === name) {
        matches.push({ declaration, statement });
      }
    }
  }

  const match = matches[0];

  // reject shadowed, non-exported, mutable, annotated or indirect bindings
  if (
    matches.length !== 1 ||
    match === undefined ||
    !match.statement.modifiers?.some((modifier) => modifier.kind === ts.SyntaxKind.ExportKeyword) ||
    match.statement.modifiers?.some((modifier) => modifier.kind === ts.SyntaxKind.DefaultKeyword) ||
    (match.statement.declarationList.flags & ts.NodeFlags.Const) === 0 ||
    match.statement.declarationList.declarations.length !== 1 ||
    match.declaration.type !== undefined ||
    match.declaration.exclamationToken !== undefined ||
    match.declaration.initializer === undefined
  ) {
    throw new Error(`${name} must be one direct exported const binding`);
  }
  return match.declaration.initializer;
}

// require direct `[... ] as const` syntax
function requireConstAssertion(initializer, name) {
  if (!ts.isAsExpression(initializer) || !isConstType(initializer.type)) {
    throw new Error(`${name} must be asserted directly as const`);
  }
  if (!ts.isArrayLiteralExpression(initializer.expression)) {
    throw new Error(`${name} must initialize one direct array`);
  }
  return initializer.expression;
}

// require `[... ] as const satisfies readonly NamedType[]` syntax
function requireSatisfiesConstArray(initializer, typeName, name) {
  if (!ts.isSatisfiesExpression(initializer)) {
    throw new Error(`${name} must use a direct satisfies assertion`);
  }
  const array = requireConstAssertion(initializer.expression, name);
  const type = initializer.type;

  // pin the exact readonly array type assertion
  if (
    !ts.isTypeOperatorNode(type) ||
    type.operator !== ts.SyntaxKind.ReadonlyKeyword ||
    !ts.isArrayTypeNode(type.type) ||
    !ts.isTypeReferenceNode(type.type.elementType) ||
    !ts.isIdentifier(type.type.elementType.typeName) ||
    type.type.elementType.typeName.text !== typeName ||
    type.type.elementType.typeArguments?.length
  ) {
    throw new Error(`${name} satisfies type must be readonly ${typeName}[]`);
  }
  return array;
}

// recognize the parser's exact `const` assertion type node
function isConstType(type) {
  return ts.isTypeReferenceNode(type) && ts.isIdentifier(type.typeName) && type.typeName.text === "const";
}

// decode one nonempty direct string-literal array
function stringArray(node, label) {
  if (!ts.isArrayLiteralExpression(node) || node.elements.length === 0) {
    throw new Error(`${label} must be one nonempty direct array`);
  }
  return node.elements.map((element) => stringLiteral(element, label));
}

// decode one direct string literal
function stringLiteral(node, label) {
  if (!ts.isStringLiteral(node) || !/^[a-z0-9-]+$/u.test(node.text)) {
    throw new Error(`${label} must be one lowercase ASCII literal`);
  }
  return node.text;
}

// decode one finite decimal literal, including a direct unary minus
function finiteNumber(sourceFile, node, label) {
  let text;

  // retain a direct negative decimal literal without evaluating an expression
  if (
    ts.isPrefixUnaryExpression(node) &&
    node.operator === ts.SyntaxKind.MinusToken &&
    ts.isNumericLiteral(node.operand)
  ) {
    text = `-${node.operand.getText(sourceFile)}`;
  } else if (ts.isNumericLiteral(node)) {
    text = node.getText(sourceFile);
  } else {
    throw new Error(`${label} must be one direct numeric literal`);
  }

  // reject separators, alternate radices and every non-decimal spelling
  if (!/^-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?$/u.test(text)) {
    throw new Error(`${label} numeric spelling is unsupported`);
  }
  const value = Number(text);

  // reject overflow before preserving the runtime binary64 value
  if (!Number.isFinite(value)) {
    throw new Error(`${label} must be finite`);
  }
  return value;
}

// decode one direct nonnegative safe integer literal
function unsignedInteger(sourceFile, node, label) {
  const value = finiteNumber(sourceFile, node, label);

  // keep Rust u64 values within the source JavaScript integer domain
  if (!Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${label} must be a safe unsigned integer`);
  }
  return value;
}

// render one station row with exact binary64 values and borrowed string slices
function renderStation(station, bindings) {
  const strings = station.acceptedSourceKeys.map((value) => bindings.rustString(value));
  return `        RetainedForecastObservationStation {
            accepted_source_keys: &[${strings.join(", ")}],
            distance_meters: ${rustF64(station.distanceMeters)},
            eligible_metrics: ELIGIBLE_METRICS,
            key: ${enumValue(bindings, "ForecastObservationStationKey", station.key)},
            latitude: ${rustF64(station.latitude)},
            longitude: ${rustF64(station.longitude)},
            nearest_rank: ${station.nearestRank}u64,
            provider_family: ${enumValue(bindings, "ForecastObservationProviderFamily", station.providerFamily)},
            unnormalized_spatial_weight: ${rustF64(station.unnormalizedSpatialWeight)},
        },`;
}

// render a generated enum variant from the caller's established name transform
function enumValue(bindings, typeName, wire) {
  const variant = bindings.variant(wire);

  // reject raw syntax injection from the callback boundary
  if (typeof variant !== "string" || !/^[A-Z][A-Za-z0-9]*$/u.test(variant)) {
    throw new Error(`invalid Rust variant for ${wire}`);
  }
  return `${typeName}::${variant}`;
}

// preserve one JavaScript number through its exact IEEE-754 bits
function rustF64(value) {
  const bytes = new ArrayBuffer(8);
  const view = new DataView(bytes);
  view.setFloat64(0, value, false);
  return `f64::from_bits(0x${view.getBigUint64(0, false).toString(16).padStart(16, "0")}u64)`;
}
