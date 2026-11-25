import {
  parseBackfillArguments,
} from "../../../../apps/worker/dist/backfill-cli.js";

const today = new Date("2026-08-22T00:00:00.000Z");
const rows = [];
let cursor = 2;

// decode each Rust-generated argv fixture and retain one exact parser outcome
while (cursor < process.argv.length) {
  const name = process.argv[cursor];
  const count = Number(process.argv[cursor + 1]);
  const arguments_ = process.argv.slice(cursor + 2, cursor + 2 + count);
  cursor += 2 + count;
  try {
    const parsed = parseBackfillArguments(arguments_, today);
    rows.push([
      name,
      "ok",
      String(parsed.chunkDays),
      String(parsed.dryRun),
      parsed.from,
      parsed.reportPath === null ? "<null>" : parsed.reportPath,
      String(parsed.resume),
      parsed.site,
      parsed.source === null ? "<null>" : parsed.source,
      parsed.to,
    ]);
  } catch (error) {
    rows.push([name, "err", error.name, error.message]);
  }
}

process.stdout.write(rows.map((row) => row.join("\t")).join("\n"));
