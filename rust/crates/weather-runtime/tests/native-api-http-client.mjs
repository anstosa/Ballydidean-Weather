import assert from "node:assert/strict";
import http from "node:http";

const [mode, port] = process.argv.slice(2);
const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
const sockets = new Set();
const trace = [];

// use the predecessor's regular HTTP client rather than a hand-written response parser
async function request(method, path) {
  return await new Promise((resolve, reject) => {
    const req = http.request({
      host: "127.0.0.1", port: Number(port), path, method, agent,
      // explicitly send an empty ordinary post rather than opening chunked input
      headers: method === "POST" ? { "content-length": "0" } : {},
    }, (res) => {
      trace.push({ method, path, connection: res.headers.connection, socketCount: sockets.size });
      const chunks = [];
      res.on("data", (chunk) => chunks.push(chunk));
      res.on("error", reject);
      res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
    });
    req.on("socket", (socket) => sockets.add(socket));
    req.on("error", reject);
    req.setTimeout(5000, () => req.destroy(new Error("API request deadline")));
    req.end();
  });
}

try {
  // preserve the real predecessor URL normalization and response behavior
  assert.equal(mode, "public");
  const sites = await request("GET", "/api/v1/../v1/sites");
  assert.equal(sites.status, 200);
  assert.equal(sites.headers["cache-control"], "no-store");
  assert.equal(sites.headers["content-type"], "application/json; charset=utf-8");
  assert.equal(JSON.parse(sites.body).data[0].slug, "ballydidean");
  const head = await request("HEAD", "/api/v1/sites");
  assert.equal(head.status, 200);
  assert.equal(head.body.length, 0);
  const badQuery = await request("GET", "/api/v1/sites?first+name=&second=ignored");
  assert.equal(badQuery.status, 400);
  assert.deepEqual(JSON.parse(badQuery.body), {
    error: { code: "invalid_query", message: "Unsupported query parameter: first name" },
  });
  const duplicates = await request("GET", "/api/v1/sites/ballydidean/forecast?days=%31&days=5");
  assert.equal(duplicates.status, 400);
  assert.deepEqual(JSON.parse(duplicates.body), {
    error: { code: "invalid_query", message: "days may be provided once" },
  });
  const refused = await request("POST", "/api/v1/sites");
  assert.equal(refused.status, 405);
  assert.equal(refused.headers.allow, "GET, HEAD");
  assert.deepEqual(JSON.parse(refused.body), {
    error: { code: "method_not_allowed", message: "Only GET and HEAD are supported" },
  });
  // the predecessor Node server plus this pinned agent retires its socket after HEAD
  assert.deepEqual(trace.map(({ socketCount }) => socketCount), [1, 1, 2, 2, 2], JSON.stringify(trace));
} finally {
  agent.destroy();
}
