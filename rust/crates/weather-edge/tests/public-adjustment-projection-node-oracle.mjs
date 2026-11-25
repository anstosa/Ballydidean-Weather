import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";

const [mode, projection, switches] = process.argv.slice(2);
const input = readFileSync(0, "utf8");

// decode the test-only typed value stream and let node own actual JSON formatting
function decodeWitness(tokens) {
  const scalar = tokens.shift();
  assert.equal(typeof scalar, "string");
  const kind = scalar[0];
  const payload = scalar.slice(1);
  switch (kind) {
    case "n": return null;
    case "t": return true;
    case "f": return false;
    case "d": return Number(payload);
    case "s": return String.fromCharCode(...(payload === "" ? [] : payload.split(",").map(Number)));
    case "a": return Array.from({ length: Number(payload) }, () => decodeWitness(tokens));
    case "o": return Object.fromEntries(Array.from({ length: Number(payload) }, () => [
      decodeWitness(tokens), decodeWitness(tokens),
    ]));
    default: throw new Error(`unknown witness token: ${kind}`);
  }
}

// use the independent node codec for complete rust-generated output trees
if (mode === "encode") {
  const tokens = input.trimEnd().split("\n");
  const value = decodeWitness(tokens);
  assert.equal(tokens.length, 0);
  process.stdout.write(JSON.stringify(value));
} else {
  assert.equal(mode, "project");
  const source = readFileSync(new URL("../../../../deploy/scripts/web-server.mjs", import.meta.url), "utf8");
  const from = source.indexOf("function filterForecastResponse(");
  const end = source.indexOf("// select one freshness-aware public API cache policy", from);
  assert.ok(from >= 0 && end > from);
  const context = vm.createContext({ Buffer });
  const execute = vm.runInContext(`
    ${source.slice(from, end)}
    (name, bytes, settings) => name === "forecast"
      ? filterForecastResponse(bytes, settings).body
      : filterRainCollectionStatus(bytes, settings);
  `, context);
  process.stdout.write(execute(projection, Buffer.from(input), {
    version: 1,
    temperature: switches[0] === "1",
    wind: switches[1] === "1",
    rain: switches[2] === "1",
  }));
}
