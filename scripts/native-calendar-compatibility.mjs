#!/usr/bin/env -S node --harmony-temporal

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";

const CONTRACT_VERSION = "native-calendar-compatibility/v1";
const EXPECTED_NODE_VERSION = "24.16.0";
const EXPECTED_ICU_VERSION = "78.3";
const EXPECTED_TZDATA_VERSION = "2026b";
const TIMEZONE = "America/Los_Angeles";
const SOURCE_START = "0000-01-01T00:00:00Z";
const SOURCE_END = "9999-12-31T23:59:59.999Z";
const TRANSITION_START = SOURCE_START;
const TRANSITION_END = SOURCE_END;
const SPOT_INSTANTS = [
  SOURCE_START,
  "0001-01-01T00:00:00Z",
  "0099-01-01T00:00:00Z",
  "0100-01-01T00:00:00Z",
  "0999-01-01T00:00:00Z",
  "1000-01-01T00:00:00Z",
  "1883-11-18T19:59:59.999Z",
  "1883-11-18T20:00:00Z",
  "2026-03-08T09:59:59.999Z",
  "2026-03-08T10:00:00Z",
  "2026-11-01T08:59:59.999Z",
  "2026-11-01T09:00:00Z",
  SOURCE_END,
];

/// Refuse to produce authority bytes from any runtime other than the reviewed predecessor.
function assertRuntimeFingerprint() {
  // bind table generation to the exact runtime fingerprint admitted by native startup
  if (process.versions.node !== EXPECTED_NODE_VERSION ||
      process.versions.icu !== EXPECTED_ICU_VERSION ||
      process.versions.tz !== EXPECTED_TZDATA_VERSION) {
    throw new Error("native calendar generation requires pinned Node 24.16.0 / ICU 78.3 / tz 2026b");
  }
}

/// Return the exact integral offset seconds observed by the pinned Temporal owner.
function offsetSeconds(zone, instant) {
  const nanoseconds = zone.getOffsetNanosecondsFor(instant);
  // reject a subsecond offset that the native TZif owner cannot represent
  if (!Number.isSafeInteger(nanoseconds) || nanoseconds % 1_000_000_000 !== 0) {
    throw new RangeError("Temporal returned a non-integral offset second");
  }
  return nanoseconds / 1_000_000_000;
}

/// Preserve the predecessor Intl fields exactly, including its unpadded early years.
function calendarSpot(formatter, instantText) {
  const instant = Temporal.Instant.from(instantText);
  return calendarSpotMilliseconds(formatter, Number(instant.epochMilliseconds), instant.toString());
}

/// Read the exact predecessor Intl fields at one already bounded epoch millisecond.
function calendarSpotMilliseconds(formatter, milliseconds, instant = null) {
  const parts = new Map(
    formatter
      .formatToParts(new Date(milliseconds))
      .map((part) => [part.type, part.value]),
  );
  const year = parts.get("year");
  const month = parts.get("month");
  const day = parts.get("day");
  const hour = parts.get("hour");
  // require the exact four fields used by localCalendarFeaturesFor
  if (year === undefined || month === undefined || day === undefined || hour === undefined) {
    throw new Error("Intl did not return complete calendar fields");
  }
  return {
    hour: Number(hour),
    instant,
    localDate: `${year}-${month}-${day}`,
    month: Number(month),
  };
}

/// Derive the source Intl year spelling from exact offset and Gregorian arithmetic.
function calendarFieldsForOffset(milliseconds, offsetSeconds) {
  const local = new Date(milliseconds + offsetSeconds * 1_000);
  const astronomicalYear = local.getUTCFullYear();
  const displayYear = astronomicalYear <= 0 ? 1 - astronomicalYear : astronomicalYear;
  return {
    hour: local.getUTCHours(),
    localDate: `${displayYear}-${String(local.getUTCMonth() + 1).padStart(2, "0")}-${String(local.getUTCDate()).padStart(2, "0")}`,
    month: local.getUTCMonth() + 1,
  };
}

/// Require the generated offset table to reproduce actual Intl at each checked instant.
function verifyCalendarInstant(formatter, milliseconds, offsetSeconds) {
  const actual = calendarSpotMilliseconds(formatter, milliseconds);
  const expected = calendarFieldsForOffset(milliseconds, offsetSeconds);
  // reject any offset or Gregorian representation difference before writing the table
  if (actual.localDate !== expected.localDate || actual.hour !== expected.hour ||
      actual.month !== expected.month) {
    throw new Error(`calendar table differs from Intl at ${milliseconds}`);
  }
}

/// Verify both sides of every transition plus one interior point of every constant-offset span.
function verifyTransitionCalendar(formatter, start, end, initialOffsetSeconds, transitions) {
  let previousMilliseconds = Number(start.epochMilliseconds);
  let previousOffsetSeconds = initialOffsetSeconds;
  let verifiedInstants = 0;
  // cover every generated boundary and its preceding constant-offset interval
  transitions.forEach((transition) => {
    const midpoint = previousMilliseconds +
      Math.floor((transition.milliseconds - previousMilliseconds) / 2);
    verifyCalendarInstant(formatter, midpoint, previousOffsetSeconds);
    verifyCalendarInstant(formatter, transition.milliseconds - 1, previousOffsetSeconds);
    verifyCalendarInstant(formatter, transition.milliseconds, transition.offsetSeconds);
    verifiedInstants += 3;
    previousMilliseconds = transition.milliseconds;
    previousOffsetSeconds = transition.offsetSeconds;
  });
  const endMilliseconds = Number(end.epochMilliseconds);
  const finalMidpoint = previousMilliseconds +
    Math.floor((endMilliseconds - previousMilliseconds) / 2);
  verifyCalendarInstant(formatter, finalMidpoint, previousOffsetSeconds);
  verifyCalendarInstant(formatter, endMilliseconds, previousOffsetSeconds);
  return verifiedInstants + 2;
}

/// Generate one auditable Node witness without invoking repository model code.
function generateWitness() {
  const zone = Temporal.TimeZone.from(TIMEZONE);
  const start = Temporal.Instant.from(TRANSITION_START);
  const end = Temporal.Instant.from(TRANSITION_END);
  const initialOffsetSeconds = offsetSeconds(zone, start);
  const lines = [`start\t${start.epochMilliseconds}\t${initialOffsetSeconds}\n`];
  let cursor = start;
  let transitionCount = 0;
  const transitions = [];
  // enumerate every strict transition in the complete four-digit source domain
  while (true) {
    const next = zone.getNextTransition(cursor);
    // stop only when Temporal exhausts the zone or crosses the inclusive source domain
    if (next === null || Temporal.Instant.compare(next, end) > 0) {
      break;
    }
    // reject a non-advancing provider result rather than looping or duplicating a boundary
    if (Temporal.Instant.compare(next, cursor) <= 0) {
      throw new Error("Temporal returned a non-advancing timezone transition");
    }
    const offset = offsetSeconds(zone, next);
    lines.push(`transition\t${next.epochMilliseconds}\t${offset}\n`);
    transitions.push({ milliseconds: Number(next.epochMilliseconds), offsetSeconds: offset });
    transitionCount += 1;
    cursor = next;
  }
  lines.push(`end\t${end.epochMilliseconds}\t${offsetSeconds(zone, end)}\n`);
  const transitionSha256 = createHash("sha256").update(lines.join("")).digest("hex");
  const formatter = new Intl.DateTimeFormat("en-US", {
    calendar: "gregory",
    day: "2-digit",
    hour: "2-digit",
    hourCycle: "h23",
    month: "2-digit",
    numberingSystem: "latn",
    timeZone: TIMEZONE,
    year: "numeric",
  });
  const intlVerifiedInstants = verifyTransitionCalendar(
    formatter,
    Temporal.Instant.from(SOURCE_START),
    Temporal.Instant.from(SOURCE_END),
    initialOffsetSeconds,
    transitions,
  );
  return {
    contractVersion: CONTRACT_VERSION,
    icuVersion: process.versions.icu,
    initialOffsetSeconds,
    intlVerifiedInstants,
    intlCalendarSpots: SPOT_INSTANTS.map((instant) => calendarSpot(formatter, instant)),
    nodeVersion: process.versions.node,
    sourceDomainEndInclusive: Temporal.Instant.from(SOURCE_END).toString(),
    sourceDomainStartInclusive: Temporal.Instant.from(SOURCE_START).toString(),
    timezone: TIMEZONE,
    transitionEndInclusive: end.toString(),
    transitionCount,
    transitionSha256,
    transitionStartInclusive: start.toString(),
    tzdataVersion: process.versions.tz,
    transitions,
  };
}

/// Encode one fixed-width table consumed directly by the native model calendar.
function encodeTransitionTable(witness) {
  const headerBytes = 32;
  const entryBytes = 12;
  const output = Buffer.alloc(headerBytes + witness.transitions.length * entryBytes);
  output.write("NCALV1\0\0", 0, "ascii");
  output.writeBigInt64LE(BigInt(Date.parse(witness.transitionStartInclusive)), 8);
  output.writeBigInt64LE(BigInt(Date.parse(witness.transitionEndInclusive)), 16);
  output.writeInt32LE(witness.initialOffsetSeconds, 24);
  output.writeUInt32LE(witness.transitions.length, 28);
  // retain strict transition order in one allocation with no JSON runtime parsing
  witness.transitions.forEach((transition, index) => {
    const offset = headerBytes + index * entryBytes;
    output.writeBigInt64LE(BigInt(transition.milliseconds), offset);
    output.writeInt32LE(transition.offsetSeconds, offset + 8);
  });
  return output;
}

assertRuntimeFingerprint();
const witness = generateWitness();
const binaryIndex = process.argv.indexOf("--binary");
// write the optional generated artifact only when an explicit destination is supplied
if (binaryIndex !== -1) {
  const destination = process.argv[binaryIndex + 1];
  // reject missing output paths instead of writing an accidental undefined filename
  if (destination === undefined || destination.length === 0) {
    throw new RangeError("--binary requires an output path");
  }
  writeFileSync(destination, encodeTransitionTable(witness));
}
const checkIndex = process.argv.indexOf("--check");
// compare generated authority bytes without changing the worktree
if (checkIndex !== -1) {
  const destination = process.argv[checkIndex + 1];
  if (destination === undefined || destination.length === 0) {
    throw new RangeError("--check requires an artifact path");
  }
  if (!readFileSync(destination).equals(encodeTransitionTable(witness))) {
    throw new Error("native calendar artifact differs from pinned predecessor output");
  }
}
const { transitions: _transitions, ...report } = witness;
process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
