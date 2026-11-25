import {
  canonicalJsonBytes,
  canonicalSha256,
  createQualificationCalendarEpoch,
  fitWindMaintenanceDevelopment,
  localCalendarFeaturesFor,
  runtimeCalendarFingerprint,
} from "../../../../packages/forecast-adjustment/dist/index.js";

const TO_LOCAL_DATE = "2026-08-01";
const DUE_MONTH = "2026-10";
const VALID_AT = "2026-01-01T12:00:00.000Z";
const OPENED_AT = "2026-08-15T12:00:00.000Z";
const epoch = createQualificationCalendarEpoch(TO_LOCAL_DATE);

// encode one original canonical preimage as lowercase hexadecimal bytes
function hex(value) {
  return Buffer.from(value, "utf8").toString("hex");
}

// construct one exact archive member for every retained local date
function member(localDate, index) {
  const sha256 = (index + 1).toString(16).padStart(64, "0");
  return {
    localDate,
    maxValidAt: VALID_AT,
    minValidAt: VALID_AT,
    path: `archive-members/${localDate}/forecast/${sha256}`,
    plaintextBytes: 1,
    recordKind: "actual_best_match",
    rowCount: 1,
    sha256,
    sizeBytes: 1,
    stationKey: null,
  };
}

const members = epoch.localDates.map(member);
const manifest = {
  aggregationContractSha256: "1".repeat(64),
  contractVersion: "adjustment-wind-archive-fit-manifest/v2",
  coordinateManifestSha256: "2".repeat(64),
  epochWitnessSha256: "3".repeat(64),
  fromLocalDate: epoch.d0,
  historyRootSha256: "4".repeat(64),
  members,
  metricEligibilitySha256: "5".repeat(64),
  sourceLineageSha256: "6".repeat(64),
  spatialWeightsSha256: "7".repeat(64),
  stationManifestSha256: "8".repeat(64),
  toLocalDate: epoch.d401,
  totalRowCount: members.length,
};
const snapshotManifestSha256 = canonicalSha256(manifest);
const openedMembers = members.map((item) => ({
  memberSha256: item.sha256,
  maximumSourceReceiptAt: OPENED_AT,
  maximumTargetReceiptAt: OPENED_AT,
}));
const input = {
  dueMonth: DUE_MONTH,
  manifest,
  openedMembers,
  rows: [{
    receivedAt: null,
    recordKind: "fixed_lead_anchor",
    validAt: VALID_AT,
  }],
  snapshotManifestSha256,
};
const result = await fitWindMaintenanceDevelopment(input);
const publication = {
  ...result,
  contractVersion: "wind-maintenance-fit/v2",
  state: "no_candidate",
  dueMonth: DUE_MONTH,
  confirmationOpened: false,
  reason: "insufficient_development_support",
};
const unsignedResult = Object.fromEntries(
  Object.entries(result).filter(([key]) => key !== "reportSha256"),
);
const local = localCalendarFeaturesFor(VALID_AT);
const fingerprint = runtimeCalendarFingerprint();
const rows = [
  ...epoch.localDates.map((date, index) => ["DATE", String(index), date]),
  ...epoch.localDates.map((date) => {
    const utc = `${date}T12:00:00.000Z`;
    const features = localCalendarFeaturesFor(utc);
    return ["NOON", date, utc, String(Date.parse(utc)), features.localDate,
      String(features.month), features.season, features.daypart];
  }),
  ["INSTANT", VALID_AT, String(Date.parse(VALID_AT)), local.localDate,
    String(local.month), local.season, local.daypart],
  ["INSTANT", OPENED_AT, String(Date.parse(OPENED_AT)), "", "", "", ""],
  ["MIDNIGHT", `${DUE_MONTH}-01`, "false", String(Date.parse("2026-10-01T07:00:00.000Z")),
    "2026-10-01T07:00:00.000Z"],
  ["MIDNIGHT", epoch.d401, "true", String(Date.parse("2026-08-02T07:00:00.000Z")),
    "2026-08-02T07:00:00.000Z"],
  ["MIDNIGHT", epoch.localDates[364], "true",
    String(Date.parse("2026-06-26T07:00:00.000Z")), "2026-06-26T07:00:00.000Z"],
  ["UTC", String(Date.parse("2026-06-26T06:59:59.999Z")),
    "2026-06-26T06:59:59.999Z"],
  ["FINGERPRINT", fingerprint.icuVersion, fingerprint.tzdataVersion],
  ["PREIMAGE", "manifest", hex(canonicalJsonBytes(manifest)), snapshotManifestSha256],
  ["PREIMAGE", "insufficient", hex(canonicalJsonBytes(unsignedResult)), result.reportSha256],
  ["BYTES", "insufficient", hex(canonicalJsonBytes(result))],
  ["BYTES", "publication", hex(canonicalJsonBytes(publication))],
  ["RESULT", result.contractVersion, result.state, result.failedGates.join(","),
    result.reportSha256, result.snapshotManifestSha256, String(manifest.totalRowCount)],
];

process.stdout.write(`${rows.map((row) => row.join("\t")).join("\n")}\n`);
