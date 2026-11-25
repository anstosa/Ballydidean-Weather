import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import http from "node:http";
import { join } from "node:path";

const [root, port] = process.argv.slice(2);
const agent = new http.Agent({ keepAlive: true, maxSockets: 1 });
const sockets = new Set();
const cases = (await readFile(join(root, "listener-cases.tsv"), "utf8")).trimEnd().split("\n");
try {
  // use node's actual http client on the same connection for all archive operations
  for (const line of cases) {
    const [name, path] = line.split("\t");
    const body = await readFile(join(root, `${name}.request.json`));
    const expected = JSON.parse(await readFile(join(root, `${name}.response.json`)));
    const result = await new Promise((resolve, reject) => {
      const request = http.request({
        host: "127.0.0.1", port: Number(port), path, method: "POST", agent,
        headers: { "content-type": "application/json", "content-length": body.length },
      }, (response) => {
        const chunks = [];
        response.on("data", (chunk) => chunks.push(chunk));
        response.on("end", () => resolve({ status: response.statusCode, headers: response.headers, body: Buffer.concat(chunks) }));
        response.on("error", reject);
      });
      request.on("socket", (socket) => sockets.add(socket));
      request.on("error", reject);
      request.setTimeout(5000, () => request.destroy(new Error("archive request deadline")));
      request.end(body);
    });
    assert.equal(result.status, 200, name);
    assert.equal(result.headers["cache-control"], "no-store", name);
    assert.equal(result.headers["content-type"], "application/json; charset=utf-8", name);
    assert.equal(Number(result.headers["content-length"]), result.body.length, name);
    assert.deepEqual(JSON.parse(result.body), expected, name);
  }
  assert.equal(sockets.size, 1);
} finally {
  agent.destroy();
}
