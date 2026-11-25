import assert from "node:assert/strict";
import { Readable } from "node:stream";

import {
  HomeNetworkMatcher,
  readTraceIpv4,
} from "../../../../deploy/scripts/home-network.mjs";

const homeAddress = "203.0.113.20";
const tunnelAddress = "172.20.0.2";

// construct one ordinary cloudflare-shaped request
function request(visitor = homeAddress, peer = tunnelAddress) {
  return {
    headers: { "cf-connecting-ip": visitor },
    socket: { remoteAddress: peer },
  };
}

let now = 0;
let address = homeAddress;
let fail = false;
let discoveries = 0;
const warnings = [];
const matcher = new HomeNetworkMatcher({
  discover: async () => {
    discoveries += 1;
    // expose one ordinary temporary discovery outage
    if (fail) {
      throw new Error("offline");
    }
    return address;
  },
  now: () => now,
  peers: async () => [tunnelAddress],
  warn: (message) => warnings.push(message),
});

assert.equal(await matcher.matches(request()), true);
assert.equal(await matcher.matches(request(homeAddress, `::ffff:${tunnelAddress}`)), true);
assert.equal(await matcher.matches(request("198.51.100.80")), false);
assert.equal(discoveries, 1);

now = 60_000;
address = "203.0.113.21";
assert.equal(await matcher.matches(request()), false);
assert.equal(await matcher.matches(request(address)), true);
assert.equal(discoveries, 2);

now = 120_000;
fail = true;
assert.equal(await matcher.matches(request(address)), false);
assert.equal(await matcher.matches(request(address)), false);
assert.equal(discoveries, 3);
assert.deepEqual(warnings, ["home network discovery unavailable"]);

let concurrentDiscoveries = 0;
const concurrent = new HomeNetworkMatcher({
  discover: async () => {
    concurrentDiscoveries += 1;
    await Promise.resolve();
    return homeAddress;
  },
  now: () => 0,
  peers: async () => [tunnelAddress],
  warn: () => {},
});
assert.deepEqual(await Promise.all([
  concurrent.matches(request()),
  concurrent.matches(request("198.51.100.80")),
  concurrent.matches(request()),
]), [true, false, true]);
assert.equal(concurrentDiscoveries, 1);

const prefix = `ip=${homeAddress}\r\n`;
const exactTrace = prefix + "x".repeat(4_096 - Buffer.byteLength(prefix));
const response = Readable.from([Buffer.from(exactTrace.slice(0, 2_000)), Buffer.from(exactTrace.slice(2_000))]);
response.statusCode = 200;
assert.equal(await readTraceIpv4(response), homeAddress);

process.stdout.write([
  "direct=true",
  "mapped=true",
  "away=false",
  "refresh_calls=3",
  "stale_after_failure=false",
  "warnings=home network discovery unavailable",
  "coalesced_calls=1",
  "trace_bytes=4096",
].join("\n") + "\n");
