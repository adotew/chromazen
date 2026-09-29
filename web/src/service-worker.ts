/// <reference no-default-lib="true"/>
/// <reference lib="esnext" />
/// <reference lib="webworker" />
/// <reference types="@sveltejs/kit" />
import { build, files, prerendered, version } from "$service-worker";

// SAFETY: SvelteKit runs this module in a worker, but its ambient types include DOM globals.
const sw = self as unknown as ServiceWorkerGlobalScope;
const CACHE = `chromazen-${version}`;
const SKIPPED_FILES = new Set(["/robots.txt", "/sitemap.xml"]);
const ASSETS = new Set([
  ...build,
  ...files.filter((file) => !SKIPPED_FILES.has(file)),
  ...prerendered,
  "/fallback.html",
]);

sw.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll([...ASSETS]))
      .then(() => sw.skipWaiting()),
  );
});

sw.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((key) => key.startsWith("chromazen-") && key !== CACHE)
            .map((key) => caches.delete(key)),
        ),
      )
      .then(() => sw.clients.claim()),
  );
});

sw.addEventListener("fetch", (event) => {
  const { request } = event;
  const url = new URL(request.url);
  if (request.method !== "GET" || url.origin !== sw.location.origin) return;
  if (url.pathname.startsWith("/_vercel/")) return;

  if (request.mode === "navigate") {
    event.respondWith(navigate(request, url));
  } else if (ASSETS.has(url.pathname)) {
    event.respondWith(cached(url.pathname, request));
  }
});

async function cached(path: string, request: Request) {
  const cache = await caches.open(CACHE);
  return (await cache.match(path)) ?? fetch(request);
}

async function navigate(request: Request, url: URL) {
  try {
    return await fetch(request);
  } catch (cause) {
    const cache = await caches.open(CACHE);
    const fallback = url.pathname.startsWith("/artwork/")
      ? "/fallback.html"
      : "/gallery";
    const response =
      (await cache.match(url.pathname)) ?? (await cache.match(fallback));
    if (response) return response;
    throw cause;
  }
}
