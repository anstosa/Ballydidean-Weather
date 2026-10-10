import {
  buildForecastObservationPoints,
  fromSiteWallClock,
  parseForecastRecordsResponse,
  toSiteWallClock,
} from "../../apps/web/dist/index.js";

// retain one shared compact day rather than repeating archive scans for every viewer
export class ForecastObservationCache {
  #cached = null;
  #pending = null;
  #fetcher;
  #origin;
  #now;
  #maxPages;
  #maxBytes;
  #timeoutMs;
  #retryMs;
  #retryAfter = 0;

  // bind fixed upstream geometry with injectable limits for deterministic refusal tests
  constructor({ origin, fetcher = fetch, now = () => new Date(), maxPages = 128, maxBytes = 24 * 1024 * 1024, timeoutMs = 30_000, retryMs = 30_000 }) {
    this.#origin = origin;
    this.#fetcher = fetcher;
    this.#now = now;
    this.#maxPages = maxPages;
    this.#maxBytes = maxBytes;
    this.#timeoutMs = timeoutMs;
    this.#retryMs = retryMs;
  }

  // share only a completed same-local-day projection for five minutes
  async get() {
    const now = this.#now();
    const localDay = toSiteWallClock(now.toISOString(), "America/Los_Angeles").slice(0, 10);
    // never carry yesterday's cache across the farm's local midnight
    if (this.#cached?.day === localDay && this.#cached.expires > now.getTime()) {
      return this.#cached.body;
    }
    // concurrent viewers share a fill without owning its cancellation signal
    if (this.#pending !== null) {
      await this.#pending;
      return this.get();
    }
    // repeated public callers cannot continuously rescan a failing core archive
    if (this.#retryAfter > now.getTime()) {
      throw new Error("Forecast observation retry cooldown is active");
    }
    const start = fromSiteWallClock(`${localDay}T00:00`, "America/Los_Angeles");
    const pending = this.#fill(start, now.toISOString()).then((body) => {
      this.#cached = { body, day: localDay, expires: now.getTime() + 5 * 60_000 };
      return body;
    });
    this.#pending = pending;
    try {
      return await pending;
    } catch (error) {
      this.#retryAfter = this.#now().getTime() + this.#retryMs;
      throw error;
    } finally {
      // failed paging never poisons later recovery or publishes an incomplete mean
      this.#pending = null;
    }
  }

  // fully page the two existing core read contracts without changing core or evidence capture
  async #fill(start, through) {
    const from = new Date(Date.parse(start) - 3 * 3_600_000).toISOString();
    const abort = new AbortController();
    const signal = abort.signal;
    const timer = setTimeout(() => abort.abort(), this.#timeoutMs);
    const deadline = performance.now() + this.#timeoutMs;
    let bytes = 0;
    let rows = 0;
    // preserve one immutable window and one sequential cursor chain per source kind
    const readKind = async (kind) => {
      const records = [];
      const cursors = new Set();
      let cursor;
      // refuse a growing archive rather than silently publishing a truncated day
      for (let page = 0; page < this.#maxPages; page += 1) {
        signal.throwIfAborted();
        // retain a monotonic deadline even when a test or transport ignores abort
        if (performance.now() >= deadline) {
          throw new Error("Forecast observation deadline exceeded");
        }
        const url = new URL("/api/v1/sites/ballydidean/history", this.#origin);
        url.search = new URLSearchParams({ from, to: through, sourceKind: kind, limit: "250", ...cursor === undefined ? {} : { cursor } }).toString();
        const response = await this.#fetcher(url, { headers: { accept: "application/json" }, redirect: "error", signal });
        // an incomplete class cannot produce a trustworthy station mean
        if (!response.ok || response.body === null) {
          throw new Error("Forecast observation history is unavailable");
        }
        let body = "";
        let pageBytes = 0;
        const decoder = new TextDecoder("utf-8", { fatal: true });
        // bound individual pages and the combined fill before decoding normalized records
        for await (const chunk of response.body) {
          bytes += chunk.byteLength;
          pageBytes += chunk.byteLength;
          body += decoder.decode(chunk, { stream: true });
          // refuse memory or wall-time growth without keeping partial results
          if (bytes > this.#maxBytes || pageBytes > 1024 * 1024 || performance.now() >= deadline) {
            throw new Error("Forecast observation history limit exceeded");
          }
          signal.throwIfAborted();
        }
        body += decoder.decode();
        const raw = JSON.parse(body);
        const parsed = parseForecastRecordsResponse(raw);
        const next = raw.page?.nextCursor;
        // bind every envelope and cursor to the one fixed farm product
        if (parsed.site.slug !== "ballydidean" || parsed.site.timezone !== "America/Los_Angeles" || parsed.data.length > 250 || raw.page?.limit !== 250 ||
          (next !== null && (typeof next !== "string" || next.length === 0 || next.length > 512 || cursors.has(next))) ||
          parsed.data.some((record) => record.provenance.sourceKind !== kind || Date.parse(record.validAt) < Date.parse(from) || Date.parse(record.validAt) >= Date.parse(through))) {
          throw new Error("Forecast observation history envelope is invalid");
        }
        rows += parsed.data.length;
        // cap expanded normalized objects within the web container memory budget
        if (rows > 20_000) {
          throw new Error("Forecast observation row limit exceeded");
        }
        records.push(...parsed.data);
        // finish only after the core confirms that this window has no remaining rows
        if (next === null) {
          return records;
        }
        cursors.add(next);
        cursor = next;
      }
      throw new Error("Forecast observation history page limit exceeded");
    };
    try {
      const [physical, regional] = await Promise.all([readKind("physical_sensor"), readKind("model_current")]);
      signal.throwIfAborted();
      return { generatedAt: through, data: buildForecastObservationPoints([...physical, ...regional], start, through) };
    } finally {
      clearTimeout(timer);
      abort.abort();
    }
  }
}
