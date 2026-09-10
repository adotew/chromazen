# Chromazen Web

Minimal landing page for [Chromazen](https://github.com/adotew/chromazen), built with Svelte 5, TypeScript, and Vite.

## Development

```bash
npm install
npm run dev
```

The generated WASM bindings are checked in so deployment does not require a Rust toolchain. After changing `crates/chromazen-web`, install Rust's `wasm32-unknown-unknown` target and `wasm-pack`, then regenerate them with `npm run wasm`.

## Checks

```bash
npm run check
npm run build
```

The production build is written to `dist/` and can be deployed directly to Vercel.
