# Chromazen Web

Minimal landing page for [Chromazen](https://github.com/adotew/chromazen), built with Svelte 5, TypeScript, and Vite.

## Development

Install Rust's `wasm32-unknown-unknown` target and `wasm-pack`, then run:

```bash
npm install
npm run dev
```

The dev and production builds compile the browser canvas from `crates/chromazen-web`.

## Checks

```bash
npm run check
npm run build
```

The production build is written to `dist/` and can be deployed directly to Vercel.
