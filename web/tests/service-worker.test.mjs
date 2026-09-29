import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";
import ts from "typescript";

test("service worker caches app assets, preserves unrelated caches, and routes offline requests", async () => {
  const handlers = {};
  const entries = new Map();
  const storedCaches = new Map([
    ["chromazen-current", entries],
    ["chromazen-old", new Map()],
    ["other-app", new Map()],
  ]);
  const offline = new Error("Offline");
  let networkResponse = "online";
  const context = {
    URL,
    exports: {},
    require: () => ({
      build: ["/app.js"],
      files: ["/icon-192.png", "/robots.txt", "/sitemap.xml"],
      prerendered: ["/", "/gallery", "/download", "/gallery"],
      version: "current",
    }),
    self: {
      location: { origin: "https://chromazen.test" },
      addEventListener: (name, handler) => (handlers[name] = handler),
      skipWaiting: async () => {},
      clients: { claim: async () => {} },
    },
    caches: {
      open: async (name) => {
        assert.equal(name, "chromazen-current");
        return {
          addAll: async (paths) => {
            assert.equal(paths.length, new Set(paths).size);
            for (const path of paths) entries.set(path, `cached:${path}`);
          },
          match: async (path) => entries.get(path),
        };
      },
      keys: async () => [...storedCaches.keys()],
      delete: async (name) => storedCaches.delete(name),
    },
    fetch: async () => {
      if (networkResponse === undefined) throw offline;
      return networkResponse;
    },
  };
  const source = readFileSync(
    new URL("../src/service-worker.ts", import.meta.url),
    "utf8",
  );
  runInNewContext(
    ts.transpileModule(source, {
      compilerOptions: { module: ts.ModuleKind.CommonJS },
    }).outputText,
    context,
  );

  let pending;
  handlers.install({ waitUntil: (promise) => (pending = promise) });
  await pending;
  assert.equal(entries.has("/robots.txt"), false);
  assert.equal(entries.has("/sitemap.xml"), false);
  handlers.activate({ waitUntil: (promise) => (pending = promise) });
  await pending;
  assert.deepEqual(
    [...storedCaches.keys()],
    ["chromazen-current", "other-app"],
  );

  function request(path, mode = "navigate", method = "GET") {
    let response;
    handlers.fetch({
      request: {
        url: new URL(path, context.self.location.origin).href,
        mode,
        method,
      },
      respondWith: (promise) => (response = promise),
    });
    return response;
  }

  assert.equal(await request("/gallery"), "online");
  assert.equal(await request("/app.js?v=1", "cors"), "cached:/app.js");
  assert.equal(request("/_vercel/insights"), undefined);
  assert.equal(request("https://other.test/app.js", "cors"), undefined);
  assert.equal(request("/gallery", "navigate", "POST"), undefined);
  assert.equal(request("/unknown.js", "cors"), undefined);

  networkResponse = undefined;
  for (const [path, fallback] of [
    ["/", "/"],
    ["/gallery", "/gallery"],
    ["/download", "/download"],
    ["/artwork/123", "/fallback.html"],
    ["/unknown", "/gallery"],
  ]) {
    assert.equal(await request(path), `cached:${fallback}`);
  }
  entries.clear();
  await assert.rejects(request("/artwork/123"), (error) => error === offline);
});
