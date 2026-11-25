import { createHash } from "node:crypto";

const recentErrorState = {
  b24C: null,
  b72C: null,
  cohort: "ecmwf_single_run_hindcast",
  localDates: 0,
  mad72C: null,
  maximumSourceRunInitializedAt: null,
  maximumSourceValidAt: null,
  n24: 0,
  n72: 0,
  sourceKeys: [],
  supported: false,
  targetRunInitializedAt: "2026-09-07T12:00:00.000Z",
  windowEndValidAt: "2026-09-07T05:00:00.000Z",
};

console.log(createHash("sha256").update(JSON.stringify(recentErrorState)).digest("hex"));
