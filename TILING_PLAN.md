# Large-canvas tiled storage plan

Status: implementation started. See the progress log below. Existing document limits and save formats remain unchanged.

## 1. Goal and proposed scope

Support finite documents larger than a single GPU texture and larger than the GPU working set, without breaking existing paint behavior, undo, artwork revisions, or export.

This requires **sparse layer storage, bounded residency, and bounded I/O**, not just replacing one texture with a grid of textures. A fully populated 16,384 × 16,384 RGBA8 layer contains 1 GiB of raw pixels regardless of tile size.

Proposed first release:

- Large documents on the native application; the same tiled engine remains usable by the browser.
- Preserve existing browser dimensions/limits and version-1 browser documents initially. Browser out-of-core storage is a separate extension, not an implicit native-filesystem dependency in the engine.
- Preserve finite document boundaries, RGBA8 premultiplied pixels, brush/eraser coverage semantics, smudge sequencing, layer clipping/opacity, and atomic native saves.
- No infinite canvas, hardware sparse textures, atlas/bindless requirement, new color model, or brush-smoothing redesign.
- Keep current public size limits until the entire native workflow is bounded. Select a higher finite limit from measurements, not merely the integer range.

## 2. Current architecture and affected locations

Paths in the first table are relative to `crates/chromazen-canvas/src/`.

| Location | Current assumption / coupling | Required change |
| --- | --- | --- |
| `renderer.rs`: `Canvas`, `CanvasSizeConstraints`, initialization and reset | Document dimensions determine GPU allocations; dimensions are capped by `max_texture_dimension_2d`, 8192, and 32 × 1024 × 1024 pixels. | Separate logical document limits from physical tile/scratch limits. Initialize sparse documents without document-sized allocations. |
| `renderer/layers.rs`: `PaintLayer` | Each layer owns one texture, view, and display bind group. | Layer metadata plus sparse tile-version references. Keep a separate small thumbnail resource and stable thumbnail identity. |
| `renderer/resources.rs`: `RenderResources`, `create_paint_layer`, `resize_document` | Full-document smudge, clipping-group, committed-mask, and preview-mask textures; bind groups refer to whole layers. | Pooled tile-sized resources, bounded scratch surfaces, per-tile bindings, and explicit accounting. Keep backdrop/surface resources window-sized. |
| `renderer.rs`: `render_to_view_with_backdrop`, `ensure_clipped_layer_bind_groups` | Display samples whole layers; clipped groups are composed into a document-sized intermediate. | Cull in document coordinates, schedule visible tile work, composite clipped groups per region, and display cached results/LOD. |
| `renderer/stamps.rs`: `StampQueue`, `StampRaw`, `drain_raw`, `preview_raw` | One document-clipped bound per dab and one union dirty rectangle for the stroke; budget is 1024 original stamps per frame. | Generate strokes once in document space, route each dab to intersected tiles, keep tile-local dirty regions, and budget expanded tile work as well as original dabs. |
| `renderer.rs`: `begin_stroke`, `flush_stamps`, `flush_stroke_preview`, `end_stroke` | One global mask/preview; stroke end synchronously drains all stamps. | Lazy per-tile masks and before-images; preview cleanup per tile; incremental completion/commit without an unbounded pointer-up flush. |
| `renderer.rs`: `flush_smudge_stamps`; `shaders/smudge.wgsl` | A full-layer source snapshot; every dab samples the previous dab's result. | Bounded source gathering across tiles, preserving the same pre-dab source for every destination fragment of one dab. |
| `renderer/history.rs` | Full-document mirror, rectangular GPU history entries, and detached `PaintLayer` resources for structure history; 256 MiB history budget. | Versioned tile before/after references, sparse structural snapshots, no permanent full-layer mirror, and budgets accounting for retained versions. |
| `renderer.rs`: `read_selected_layer_content_bounds`, transform methods | Bounds synchronously read back the whole layer. Transform preview samples the history mirror and rewrites the layer; cancel restores it. | Maintain exact per-tile alpha bounds; use immutable transform sources, non-destructive preview, and incremental destination-tile commit. |
| `renderer.rs`: duplicate, clear, delete, merge, resize; `apply_history_structure_effect` | Full textures copied/retained/recreated, sometimes a complete second layer set. | Sparse map operations and bounded raster jobs. Preserve atomic command completion and undo behavior. |
| `renderer/sampling.rs` | Eyedropper reads one pixel from each full-layer texture and waits synchronously. | Resolve the relevant tile/version per layer; load asynchronously when necessary and sample full-resolution pixels, never display LOD. |
| `renderer/persistence.rs`: `LayerReadback` | Allocates readback buffers for every full layer and returns `Vec<RgbaImage>`. | Snapshot-scoped, bounded tile/region readback and upload APIs. Retain a size-capped whole-image adapter only for compatibility/tests. |
| `renderer.rs`: `DocumentVersions`, `mark_layer_changed`, thumbnail rendering | Layer-level versions and thumbnail invalidation. | Keep cheap coarse dirty tracking; add tile content identities and snapshot tile maps. Distinguish content changes from residency/thumbnail changes. |
| `renderer/view.rs` | f32 transforms and fixed minimum zoom of 0.01. | Visible-tile coverage under rotation/flip; audit precision and fit-to-screen for higher limits. Do not unnecessarily change workspace/reference coordinates. |
| `lib.rs` | Exposes the current whole-document APIs. | Export only the minimal tile/snapshot/request types needed by hosts; maintain compatibility adapters deliberately. |
| `shaders/stamp.wgsl`, `stroke_composite.wgsl`, `blit.wgsl`, `clipped_layer.wgsl`, `merge.wgsl`, `layer_preview.wgsl`, `transform.wgsl`, `smudge.wgsl` | Positions, UVs, dimensions, or texture loads assume one document texture. | Explicit document origin, tile-local position, valid edge extent, and cross-tile sampling where needed. Update Rust/WGSL layouts together. |

Native integration:

| Location | Required change |
| --- | --- |
| `src/artwork/format.rs` | Versioned tiled document/layer descriptors; parse legacy schemas before applying new strict validation. Current document schema is 3 and migration accepts 2. |
| `src/artwork/store.rs` | Tile write/reuse records, lazy load descriptors, revision leases, cleanup safe for active readers, tiled duplication, and atomic commits with bounded payload delivery. |
| `src/artwork.rs` | Convert document metadata separately from layer pixel storage; stop manufacturing a single `layers/{id}.png` path for every layer. |
| `src/artwork/raster.rs` | Region/row compositing with the same premultiplied-alpha and clipping rules, and streaming PNG support. |
| `src/app/autosave.rs` | Currently reads every layer even when only changed layers are PNG-encoded. Capture immutable snapshots, read only changed tile versions, reuse other tiles, and create thumbnails without whole layers. |
| `src/app/gallery.rs` | Currently decodes all layer PNGs before opening. Load validated descriptors, thumbnail/overview, and initial visible tiles; handle legacy conversion off the UI thread. |
| `src/app/export.rs` | Replace whole-layer readback + full composite + encoded byte vector with snapshot-based region compositing and streaming atomic output. |
| `src/app/navigation.rs`, `completions.rs`, `src/app.rs` | Own asynchronous native tile I/O through the existing controller/completion pattern; use session/request identities; cancel obsolete work and retain backing data until readers finish. |
| `src/app/frame.rs` | Pump bounded uploads/readbacks/jobs and redraw on tile/LOD completion. `has_pending_stamps()` alone no longer expresses pending render work. Remove blocking content-bounds readback from redraw. |
| `src/app/input.rs`, `commands.rs`, `command.rs` | Route delayed sample/transform/merge/resize/undo completion through the existing flow; preserve input order and busy-state semantics without synchronous full-operation draining. |
| `src/app/menu.rs`, `src/app/ui.rs`, `ui/editor.rs`, `ui/dialogs.rs`, `ui/canvas_crop.rs`, `ui/layer_transform.rs`, `ui/interaction_geometry.rs` | Audit busy/menu availability, progress/error reporting, size validation, crop arithmetic, transform-bound readiness, and preview registration. Most presentation code should remain unchanged. |
| `src/gpu.rs` | Keep ordinary device features; configure engine budgets from host policy. No request for huge textures or backend-specific sparse residency is required. |
| Root `Cargo.toml`, `Cargo.lock` | Likely add a direct `png` dependency for row-streaming decode/encode; the dependency already exists transitively, but must be explicit if used directly. |

Browser compatibility is part of the change:

- `crates/chromazen-web/src/lib.rs` uses `Canvas::new`, whole-image load, `LayerReadback::finish_async`, synchronous stroke-end/history APIs, and `has_pending_stamps()` for animation scheduling.
- Keep version-1 `StoredDocument`/layer PNGs initially via explicitly bounded adapters and a bounded in-memory host tile backing store. Validate size limits before decoding stored PNGs. Never introduce native waits, threads, or filesystem requirements into the shared engine.
- Adapt completion scheduling in `web/src/routes/artwork/[id]/+page.svelte` if jobs/readbacks become incremental; save promises must complete even when no normal animation frame is pending.
- `web/src/lib/artwork-saving.ts` must regard engine stroke/job completion as interaction completion if pointer-up no longer commits synchronously. A captured save version must correspond to committed pixels.
- If large browser documents are included later, `web/src/lib/artworks.ts` needs separate metadata and tile records, a database migration, transactional revision publication, and quota/error handling. Its current gallery `getAll()` loads all artwork pixel payloads too. Related consumers are `web/src/routes/gallery/+page.svelte` and the artwork page.
- Regenerate `web/src/lib/wasm/*` using the existing build script; do not hand-edit generated bindings.

Expected to stay unchanged: platform tablet integrations, brush assets/import/config parsing, the smoothing algorithm, window-surface/frost rendering, reference-image storage, packaging, and unrelated website pages. Exercise references/cursors/frost as integration regressions, but do not tile them as part of this work.

## 3. Proposed engine design

### 3.1 Logical pixels versus physical residency

Start with 512 × 512 logical tiles, benchmark against 256 and 1024 before freezing the format. At 512, one RGBA8 tile is 1 MiB and one R8 mask is 256 KiB, excluding GPU allocation overhead.

Use independent reusable 2D textures initially. They are easy to bind as render targets and do not depend on texture arrays, bindless features, or hardware sparse textures. Pool by size/format. Optimize into array pages or an atlas only if profiling justifies the additional lifetime and sampling complexity.

Proposed concepts (names are provisional):

- `TileCoord` and checked document/tile/region conversions.
- A sparse ordered or hashed map per layer: tile coordinate → immutable content-version handle.
- An absent map entry means transparent. A present but nonresident entry means data must be loaded; it must **not** silently become transparent.
- A version can have GPU, CPU, and/or backing-store representations. Residency is separate from logical identity. Mutation detaches from snapshots/history by copy-on-write.
- Edge tiles have an explicit valid extent; out-of-document pixels remain transparent and cannot reappear after resizing. Halo/padding is derived, not persisted document content.
- CPU decoded cache, GPU resources, queued transfer buffers, history retention, and temporary disk data have explicit budgets. Sparse storage saves empty space; it does not make dense content free.

Keep tile geometry/model/residency policy in the canvas crate. Put native paths, compression, and filesystem work behind an app-owned tile-I/O controller. The engine emits bounded requests and accepts completions with opaque backing identifiers; a small in-memory host can serve headless tests and existing browser documents.

### 3.2 Operations and scheduling

Have a minimal operation state machine for edits that may wait on tiles. A paint sample is accepted once, then processed in order as required data arrives. Never run smoothing or spacing separately per tile.

- Prioritize the brush working region and visible full-resolution tiles, then overview and thumbnail maintenance, then background work.
- Budget bytes uploaded/read back, expanded stamp work, command encoding, and temporary allocations per tick. A stamp-count limit alone does not bound a large brush's work.
- A long stroke cannot pin every touched GPU tile until pointer-up. Retain logical before-images and mask state with spillable backing; pin only the current execution footprint and in-flight resources.
- On pointer-up, finish as an incremental operation if needed. Autosave/navigation/undo wait for logical commit, not merely the absence of a pointer interaction.
- Delay a dab requiring nonresident pixels rather than painting over empty placeholders. Show loading/backpressure; do not drop samples silently. Bound pending input/jobs and define an explicit cancellation/error path if resources cannot be obtained.
- Use completion wakeups rather than continuous busy repainting when all work is waiting on I/O.
- Capture immutable versions for save/export/undo so edits can continue without mixing generations. Reusing a texture must wait for submitted GPU work and outstanding readbacks to release it.
- Use distinct per-tile uniform slices/dynamic offsets or immutable buffers within a submission. Repeated `queue.write_buffer` calls to one uniform location are not per-draw state snapshots.

### 3.3 Brush, eraser, smudge, and transforms

Brush/eraser:

- Allocate layer tiles only when pixels are actually needed; preview-only coverage must not become saved layer content.
- Preserve maximum-coverage mask accumulation across the entire stroke, then apply color/erasure once, as today.
- Record each touched tile's before-version once; track committed mask and predicted-preview regions separately.
- Bin only intersected tiles, including corners and partial edge tiles. Clearing/replacing a preview must clear old tile coverage too.

Smudge is the highest-risk raster path:

- Preserve the existing algorithm: all destination fragments of dab N sample the result after dab N−1, not partial updates from dab N.
- Gather target/source regions, including interpolation neighbors, from immutable pre-dab versions into bounded scratch patches. Do not publish an output tile into dab N's source set while processing its neighbors.
- Process all chunks of a dab before advancing logical source state. Stage/spill output versions when the footprint exceeds the GPU budget.
- Internal tile edges must sample neighbors. Only the real document edge uses the current clamping behavior. One-pixel gutters alone do not cover an arbitrary smudge source offset.

Transforms:

- Track per-tile alpha bounds with bounded reductions/readbacks and union them for the exact layer bounds; shrinking/erasing must update bounds too. Keep the transform pivot identical to current behavior.
- Preview from an immutable source and a transform matrix; do not continually rerasterize the complete layer on pointer movement.
- On commit, traverse affected destination tiles and fetch needed source regions; preserve current manual bilinear filtering and transparent out-of-document sampling.
- Subdivide destination regions when a scaled/rotated source footprint would exceed texture or scratch limits. A fixed 512-pixel destination tile can sample a far larger source rectangle at minimum scale.
- Commit or cancel as one logical action. Changed/deleted source tiles and newly created destination tiles must all be represented in history.

Clear/delete/duplicate/merge/resize:

- Clear/delete retain sparse map snapshots rather than pixel copies. Duplicate shares immutable tile versions until edited.
- Merge walks the union of affected tile coordinates and preserves current visibility/opacity/clipping semantics.
- Tile-aligned resize translations may reuse compatible versions; arbitrary origins need bounded cross-tile copies. Cropped pixels stay out of the live map but remain accessible to undo.
- Stage structural results, publish metadata/maps atomically, and retain the old result on failure/cancel.

### 3.4 Display, LOD, and derived caches

Visible-region culling alone is insufficient: fit-to-screen can expose every full-resolution tile.

- Maintain a tiled composited overview pyramid; choose resolution by zoom so display cost follows viewport resolution rather than full document area.
- Build/update parent levels incrementally and allow persisted, version-keyed derived data for fast reopening. Derived data may be discarded/rebuilt and must never be authoritative paint/export pixels.
- Invalidate affected composite tiles and ancestors for pixel edits; metadata changes to layer order, clipping, visibility, opacity, or background invalidate the relevant composite generation. Rebuild with a budget, not a frame-wide full-document pass.
- Compose at source resolution before downsampling where necessary for correct alpha/clipping. Independently downsampling layers and then compositing is not generally equivalent.
- Fetch interpolation neighbors/gutters across composite-tile boundaries, including at LOD transitions. Never clamp at an internal tile border.
- During loads/rebuilds, show an explicitly stale coarse preview or loading placeholder, not silently missing paint. Full-resolution sampling, editing, and export cannot use that fallback.
- Maintain layer thumbnails separately; preserve `LayerResourceId` semantics for egui registration instead of changing it whenever a paint tile is evicted.

## 4. Native persistence and snapshot lifetime

Proposed next document schema: version 4, with tile size/pixel representation and a sparse per-layer tile index. Start with immutable lossless tile PNG payloads inside each revision (e.g. `layers/{layer_id}/{x}_{y}.png`), using the existing hard-link/copy reuse pattern. Benchmark file counts and index parsing before committing to the exact layout; an indexed pack file can be a later optimization.

Requirements:

1. Parse existing version-2/version-3 layer-PNG manifests with legacy types. Merely changing `LayerManifest` would reject them before `.migrate()` because of strict field validation.
2. Convert legacy pixels in a worker with bounded row/band decoding; initially support the app's existing PNG output, and explicitly handle or reject unsupported encodings/interlacing without corrupting the original artwork.
3. Opening old artwork must not rewrite it. Publish version 4 only on a successful save; leave the last committed revision valid on failure.
4. Record dimensions, edge extents, unique tile coordinates, and lossless premultiplied RGBA8 byte semantics explicitly. Do not accidentally unpremultiply/re-premultiply legacy pixels. Validate paths, dimensions, counts, arithmetic, decoded sizes, and supported versions before allocations.
5. Distinguish intentionally absent transparent tiles from missing/corrupt referenced files. Report corruption rather than treating missing data as blank paint.
6. Save a frozen metadata/tile-version snapshot. Reuse unchanged payloads; stream changed tiles to a temporary revision with bounded channels, not a `Vec` of all PNG bytes. Tile removals must remove entries from the new index.
7. Produce the thumbnail from the same snapshot, not a newer live frame. Metadata-only saves must not force layer-pixel readback.
8. Keep the existing temporary-revision → rename → atomic project-pointer publication structure and rollback handling. Do not claim stronger crash durability without auditing file/directory sync behavior separately.
9. **Fix revision lifetime before lazy loading ships.** Currently `commit_revision` immediately deletes the previous revision, and `scan_catalog` invokes cleanup of noncurrent revisions and temporary writes. Introduce shared leases for live document versions, undo, load jobs, save/export snapshots, and active writes. Cleanup/deletion must honor them, or transfer retained tiles into a session-owned backing area first.
10. Native spill storage must cover dirty unsaved tiles, masks, history versions, and staged operations when RAM/GPU budgets fill. Spill completion is not an artwork save. Disk-full/I/O failure must retain the only valid copy and report backpressure/error, never mark the document saved or discard paint.
11. Duplication, deleting the active artwork, navigation, and startup cleanup must respect outstanding readers. Decide and document single-writer/session ownership rather than introducing unsafe cross-process cleanup.

For export, stream full-resolution composited rows or bounded horizontal bands into `png`'s streaming writer and an `AtomicWriteFile`. Do not allocate the whole composite or encoded PNG. A full-width band can itself be large, so budget band height and tile traversal explicitly. Finish the encoder successfully before publishing the destination; failure leaves any previous export intact.

## 5. Implementation sequence and acceptance gates

Each phase should be reviewed as a coherent series of small commits. Do not expose higher size limits during the intermediate phases.

### Phase 0 — Baselines and decisions

- Expand `crates/chromazen-canvas/tests/headless.rs` beyond its current 32/64-pixel examples. Capture repeatable small-document raster fixtures for brushes, eraser, smudge, clipping, transforms, merge, resize, and undo.
- Add instrumentation for owned GPU/CPU bytes, transfer bytes, passes, expanded dabs, and operation timings. GPU memory estimates must include scratch/history/in-flight resources, not just layer textures.
- Compare 256/512/1024 tile workloads in targeted experiments; measure sparse and dense documents, few/many layers, tiny/large brushes, and zoomed-out display.
- Confirm first-release native/browser scope, budget defaults, finite target dimensions, tile codec/index layout, and source-gather strategy. No silent feature removal.

Gate: behavior oracle and measurable budgets exist; difficult cross-tile algorithms have a concrete design.

### Phase 1 — Tile model and host boundary

- Add small `renderer/tiles.rs` and residency/snapshot helpers as needed; use existing modules for their current responsibilities rather than building a general virtual-texture framework.
- Implement checked geometry, sparse maps, immutable identities, copy-on-write, explicit absence/nonresidency, per-tile dirty/bounds metadata, and resource accounting.
- Define request/completion and snapshot ownership contracts. Test with an in-memory host and tiny cache budgets, including stale session completions.
- Add the native tile-I/O controller skeleton only where needed; keep native filesystem/types outside the engine.

Gate: empty large logical maps allocate no pixel grid; copy-on-write and backing lifetimes are correct without a GPU.

### Phase 2 — Vertical slice: brush, eraser, display, tile history

- Convert `PaintLayer`/resources, shaders/uniforms, stamp binning, masks/previews, and visible-region rendering together.
- Introduce tile before-images/history concurrently: undo and preview cannot continue relying on a monolithic mirror.
- Add bounded tile readback/upload, plus explicit small-image compatibility adapters for existing tests/browser consumers.
- Preserve per-layer thumbnail resources and clipped-group rendering.

Gate: multi-tile brush/eraser, previews, opacity/clipping, readback, undo/redo, and empty/edge tiles match baseline behavior. No full-document painting/mask/history allocation remains in this path.

### Phase 3 — Complete editor operations

- Convert smudge first, including source ordering and chunking tests.
- Convert bounds/eyedropper, transform preview/commit/cancel, duplicate/clear/delete/merge, and tile-aligned/nonaligned resize.
- Introduce incremental operation completion and wire native command/input/busy states. Update the browser adapter and save scheduling where necessary.
- Remove remaining monolithic mirror and operation fallbacks from production engine paths.

Gate: all existing editor operations work across tile boundaries, including after undo and with delayed tile loads. Long operation completion does not synchronously drain unlimited work on the interactive thread.

### Phase 4 — Versioned tiled saves and lazy open

- Implement schema 4, legacy readers/conversion, tiled revision writes/reuse, revision leases, lazy descriptors, and bounded transfer pipelines.
- Update artwork conversions, autosave snapshots, gallery loading, navigation, and completion handling.
- Retain native revision atomicity, title/reference behavior, and old-format compatibility. Keep browser version-1 save/load under its existing finite limits.

Gate: save/reopen/duplicate/migrate work without whole-layer allocations; metadata-only saves read no layer pixels; interrupted commits preserve the previous document; old revision cleanup cannot invalidate a live reader.

### Phase 5 — Bounded residency and disk spill

- Enable native GPU/CPU eviction and session spill through the controller, with byte budgets, prioritization, completion wakeups, and generation checks.
- Make history/masks/snapshots/transform sources evictable too. Test a single stroke and a single large operation exceeding the cache, not only idle panning.
- Add user-visible I/O/resource errors and define bounded input backpressure/cancellation behavior.

Gate: repeated painting/panning/undo/load/save under a deliberately tiny cache remains correct. Memory estimates and observed process use reach a bounded working set rather than growing with painted area. Disk-full cannot lose the sole copy of a dirty tile.

### Phase 6 — Overview pyramid, thumbnails, streaming export

- Implement incremental composited LOD and version-keyed overview persistence/rebuild; reuse bounded reduction infrastructure for layer/gallery thumbnails.
- Convert raster/export to bounded regions and streaming PNG. Add an explicit direct dependency if needed; do not rely on transitive imports.
- Ensure snapshot jobs make progress without an open/visible editor surface.

Gate: fit-to-screen of a dense document does not load all full-resolution tiles; seam tests pass at varied zoom/rotation; export matches full-resolution baseline pixels and does not allocate full images/PNG payloads.

### Phase 7 — Raise limits and release hardening

- Remove the document-to-`max_texture_dimension_2d` coupling while retaining physical tile/brush/surface checks.
- Set tested document/layer/tile-count/operation limits, update size/crop UI and zoom policy, and audit f32/i32/u32/usize conversions for the chosen range.
- Run native and browser workflows, benchmark the workload matrix, document format changes and bounded-memory limitations, and remove temporary migration scaffolding that is no longer needed.

Gate: creating, painting, saving/reopening, transforming, undoing, and exporting a document beyond the GPU texture dimension all succeed under the configured budgets. Existing small documents remain responsive.

Dependencies: 0 → 1 → 2 → 3; persistence work can proceed after snapshot contracts in 1, but 4 and 5 must be integrated before enabling eviction of saved content. LOD/export work can start after 2/4 interfaces settle. Phase 7 depends on all earlier gates, not just basic tiled rendering.

## 6. Regression and performance coverage

Unit tests, colocated with implementation:

- Exact tile-edge/corner intersection, partial edge tiles, negative crop origins, overflow rejection, and rotated/flipped viewport coverage.
- Absent versus nonresident/error states; tile deletion after erase; copy-on-write isolation; retained-history byte accounting; eviction and failed-spill recovery.
- Dirty tile/version tracking across undo/redo, clear, transform, resize, merge, layer metadata changes, and save-in-flight edits.
- Job ordering, bounded requests, stale session/version completions, cancel/retry, and revision lease cleanup.
- Manifest v2/v3 migration, v4 round trips, invalid coordinates/paths/counts/decoded sizes, missing tiles, atomic failure/retry, and hard-link/copy fallback using temporary directories.

Headless GPU tests (`crates/chromazen-canvas/tests/headless.rs`):

- Use test tile sizes small enough for boundary coverage in small fixtures, plus production-sized and >8192-dimension sparse cases.
- Brush/eraser centered on an edge/corner; dab larger than one tile; opacity max-coverage behavior; predicted preview appearing/disappearing across tiles.
- Smudge traversing each boundary and corner, large source displacement, output independent of destination-tile processing order, real-document-edge clamping preserved.
- Clipped groups, semitransparent layers, transform filtering/cancel, nonaligned resize, merge, duplicate isolation, clear, and undo/redo after eviction.
- Seam-free display at 1:1, fractional zoom, rotation, flips, and LOD transitions. Compare exact bytes where semantics are unchanged; document tolerances for filtering, not blanket image differences.
- Save/export snapshots stable while painting continues; full-resolution eyedropper/export independent of display LOD.

Manual native checks: gallery → editor, delayed loads, pan/zoom/rotate, pressure and large brushes, autosave/reopen, export, references/cursor/frost integration, all layer operations, native menus/shortcuts, navigation/exit while saving, disk-full/missing files, cancellation and recovery. Run the affected GPU paths on Linux, Windows, and macOS.

Browser checks: old IndexedDB documents load, brush/eraser/smudge and undo work, async save resolves without a rendering deadlock, navigation waits for pending commit/save, and errors do not overwrite existing artwork. Native host builds do not compile the body of the `#![cfg(target_arch = "wasm32")]` web crate, so a wasm build is required.

Verification commands during implementation:

```sh
cargo fmt --all -- --check
cargo test --workspace
cargo test -p chromazen-canvas -- --ignored
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo check -p chromazen-web --target wasm32-unknown-unknown
cd web && npm run wasm && npm run check && npm run build
```

Track sparse and dense document cases separately: resident bytes, CPU RSS, staging/spill bytes, frame/input latency, expanded stamp work, cold/warm tile-load latency, save bytes/time, and export peak memory. Sparse-only success is not evidence that dense large documents are supported. Choose numerical budgets/performance targets in Phase 0 based on supported hardware.

## 7. Decisions to confirm before implementation

1. Native large-canvas support first, browser compatibility now, browser out-of-core support later (recommended).
2. Initial tile size: 512, subject to the Phase 0 comparison.
3. Initial target dimensions and layer/workload expectations; suggest benchmarking 16K/32K documents before promising a larger limit.
4. GPU/CPU/history/spill budget defaults and whether to expose configuration immediately or start with internal policy.
5. Per-tile PNG files versus packed tile payloads at the expected layer/tile counts; retain immutable revision semantics either way.
6. Exact handling of resource exhaustion and extended operations: progress/cancel/retry behavior without dropping strokes or publishing partial documents.

The first implementation work should be baseline tests and tile/snapshot contracts, **not increasing `MAX_CANVAS_DIMENSION`**.

## 8. Implementation progress

### Phase 0 — Baselines and initial policy

Implemented:

- Deterministic 1031 × 773 headless raster oracles crossing the candidate 256/512/1024 tile boundaries, including partially covered document edges and padded readback rows.
- Brush maximum-coverage opacity, eraser, sequential smudge (both batched and flushed per dab), transform cancel/commit, nonaligned resize, duplicate isolation, clipped merge, exact undo/redo restoration, and readback snapshot isolation.
- `Canvas::memory_usage()` reports live layer/thumbnail/settings payloads, the history mirror and actually detached history resources, scratch/masks/backdrop, brush, buffers, CPU stamp capacity, and outstanding layer readbacks. It is explicitly an ownership estimate, not driver VRAM or an enforceable budget. Submitted-only GPU references, driver overhead, caller-owned images, and temporary CPU work are not measured.
- `Canvas::work_counters()` counts painting passes/dabs and pixel/stamp uploads and padded layer readbacks. Readback leases remain accounted until both caller and mapping callbacks release them. Timing stays in the native test harness, avoiding a native clock dependency in the browser engine.

Initial decisions: native large canvases first; preserve browser v1 and present limits. Use 512-pixel tiles provisionally, independent 2D textures, immutable versions, and per-tile PNG revision payloads. Initial residency targets for later integration are 256 MiB GPU tile/scratch payload, 128 MiB decoded CPU cache, and 16 MiB in-flight transfers with an 8 MiB per-tick transfer target; preserve the existing 256 MiB history retention target. These are engineering starting points, not new promises about supported document size. Keep 16K/32K as benchmark targets only. Disk exhaustion must stop publication and retain dirty data, not silently evict it.

Sizing experiment (raw RGBA8, excluding halos/history/masks):

| Tile side | One tile | Dense 16K layer | Dense 32K layer | Radius-16 dab at (512,512) | Radius-384 dab at (512,512) |
| --- | --- | --- | --- | --- | --- |
| 256 | 256 KiB | 4096 tiles / 1 GiB | 16384 tiles / 4 GiB | 4 tiles / 1 MiB | 16 tiles / 4 MiB |
| 512 | 1 MiB | 1024 tiles / 1 GiB | 4096 tiles / 4 GiB | 4 tiles / 4 MiB | 4 tiles / 4 MiB |
| 1024 | 4 MiB | 256 tiles / 1 GiB | 1024 tiles / 4 GiB | 1 tile / 4 MiB | 1 tile / 4 MiB |

Eight dense layers multiply these counts/bytes by eight. This is a footprint calculation, **not a tiled-renderer performance benchmark**. Actual tile upload/draw/zoomed-out timing comparisons move to phases 2/6, where those paths exist; do not freeze a disk schema based on the sizing table alone. Smudge uses a stable pre-dab source for every fragment; transforms gather bounded source regions and subdivide destinations with oversized inverse footprints, as specified above.

Verification on the available Linux wgpu adapter: workspace tests, headless GPU tests, workspace/all-feature clippy, formatting, and `cargo check -p chromazen-web --target wasm32-unknown-unknown` pass. The new raster baselines took about 7.3 seconds in the debug test harness (including repeated pipeline setup/readback; not an interactive performance result). The diagnostic fixture with two layers and one stroke reports 17,818,682 owned GPU payload bytes before readback. Other platforms and native UI flows have not been exercised.

### Phase 1 — Tile model and host request boundary

Implemented in `crates/chromazen-canvas/src/tiles.rs` and `tiles/{versions,requests}.rs`:

- Checked, lazy tile intersections with explicit document/local coordinates, half-open bounds, valid edge extents, negative-region clipping and overflow handling. Huge empty logical grids allocate no pixel array.
- Sparse `LayerTiles` indexes with shared metadata snapshots and immutable `TileVersion` identities. Editing detaches the populated map, not pixel data; duplicate/history/save snapshots keep the old versions. Dirty comparison includes removed coordinates. Transparent content is absent; unbacked/nonresident content remains distinct.
- Validated per-version alpha-bound metadata (`Unknown`, `Empty`, or exact bounds), logical-byte accounting with overflow detection, and one-time host backing publication. Backing is a generic host lease, not a native filesystem path; failed writes cannot turn a version into an evictable backed one.
- Deduplicated asynchronous read request/completion bookkeeping with byte/count limits and globally non-reused request identities. Cancellation keeps its reservations until old jobs acknowledge completion. Missing files and bad dimensions remain errors, and stale/foreign completions cannot publish into current layer maps.
- Tests using an in-memory host and 64-byte transfer budget, including backing lease lifetime through snapshots and cancelled work. These are request-budget tests, not GPU/CPU-cache eviction tests; residency and spill remain phase 5.

The module is separate from `renderer` because it contains no GPU/window/platform dependencies. It is deliberately **not yet wired into `Canvas`**: phase 2 must replace the interdependent layer, mask, display and history paths together. No unused native I/O controller skeleton was added; the first production host arrives when the renderer actually issues requests. Hosts must budget decoder scratch separately, validate decoded dimensions before allocation, reuse the request queue across document switches, and transfer completion payloads into a residency budget (or drop them) before scheduling more work.

Verification: 98 canvas unit tests (15 new tile-model/request tests), all workspace tests, the expanded headless GPU test, workspace/all-target/all-feature clippy, formatting, and the WASM web-crate compile check pass. No persistence schema, shader layout, existing canvas limits, or UI behavior changed in this phase.

### Phase 2 — In progress

The first production conversion removes the document-sized history mirror. History captures 512-pixel before-image tiles immediately before the first write and retains only touched tiles (not the bounding rectangle between distant dabs). Starting a stroke allocates no history pixels. Transform source pixels no longer depend on history: the mutually exclusive transform/smudge operations share the existing operation-source texture. Transform cancellation, resize undo, layer switching and redo truncation remain covered by the raster tests.

Before-images now use the phase-1 immutable version identities and a small GPU residency table (`renderer/tiles.rs`). The table holds weak version references, while history holds the strong references; dropping a redo branch or evicting history releases dead GPU entries. Undo captures the alternate pixels in a new version rather than mutating a published one. While live layers remain monolithic this still copies pixels and temporarily retains submitted resources; it is not yet the bounded residency cache from phase 5. Once live layers share these indexes, undo can exchange version references instead.

`Canvas::begin_layer_tile_readback` adds a cropped, snapshot-isolated single-tile transfer path with a 16 MiB staging budget. Budget reservation is atomic, includes row padding and stays live through mapping callbacks and caller ownership. Region copies reuse the existing native/async readback completion path. GPU tests cover all six regions of the partial-edge fixture, exact pixels, invalid requests, edit-after-capture isolation, budget saturation/retry, and abandoned callbacks. A concurrent reservation unit test covers the check/reserve race. This API freezes one tile; multiple calls are not yet a frozen document save snapshot.

Stamp projection now accepts a document-space target origin. Smudge has separate bounded source and pre-dab target bindings, source-region origin/extent, and actual document dimensions for edge clamping. Rust/WGSL `Paint` grew from 16 to 48 bytes; all creation/resize/brush-replacement bindings were updated together. A GPU regression uses independent immutable uniforms for draws encoded in one submission, distant source/target coordinates in a virtual 16K document, and padded source regions crossing both document edges. Actual textures are only 16 × 16. Existing production callers still use the full-document adapter with origin zero; public canvas limits are unchanged. Run **all** ignored canvas tests (`cargo test -p chromazen-canvas -- --ignored`) to include this new colocated GPU test as well as the integration baselines.

The production brush/eraser stamp drain now sorts the generated document-space dabs into tile-coordinate batches, clips each batch's mask pass and before-image capture to its tile, and limits **expanded** fragments (not just source dabs) to 1024 per frame. An oversized dab keeps its tile cursor across frames; unchanged maximum-coverage blending allows reordering between disjoint tiles. Sequential smudge retains its old ordered path. Preview generation no longer clones the pending committed stamp queue and limits predicted line generation to the preview capacity, retaining the latest endpoint even for a million-pixel predicted segment. A regression checks this with thousands of queued committed stamps. Stamp-queue capacity accounting includes the resumable fragment cursor. `CanvasWork::tile_fragments` exposes the extra expansion work. Unit tests cover partial edges, grouped dabs and resuming an oversized dab. A headless regression submits 300 overlapping dabs across four tile regions, verifies the 1024-fragment frame cap/resumption, exact maximum-coverage opacity and undo/redo; the other raster baselines still pass. This is preparatory binning only: the attachment and mask are still whole-document textures, and pointer-up still flushes pending work synchronously. The cap bounds fragment submission per render call, not all input-generation/commit work.

The existing layer-display pipeline now accepts a 16-byte document-origin/extent uniform and samples only that region; the full-document binding uses origin zero and its former dimensions (including after resize). A GPU regression renders a 7 × 13 source at document position [25, 30] and verifies every displayed pixel and transparent surroundings. This changes the **existing** pipeline, not a parallel renderer. Layers still own full-document textures; clipped groups, previews and thumbnails have not yet been converted to tile sampling.

`PaintViewSnapshot::visible_tile_regions` now supplies a lazy conservative viewport-to-tile iterator with a one-document-pixel sampling border. Four inverse-projected corners cover rotation/flips; off-document regions are clipped by the grid, and tests sample every viewport orientation against point locations and verify that a huge sparse document requires no dense grid walk. This is planning geometry only; display still samples the original whole-layer textures. `visible_document_region_rect` now also derives a conservative, surface-clipped physical-pixel scissor from all four document-space tile corners (including rotation and partial-edge tiles), sharing the full-document display scissor code rather than adding another display path.

Clipped-group **display scratch is now surface-sized**, not document-sized. The existing display path composites each group in physical window pixels with view-aware merge/clipped/brush-preview sampling, then blits that group to the presentation view; structural layer merging retains its original document-space shader entry point. Resizing the document no longer reallocates clipping scratch; resizing the surface does. GPU regressions compare clipped display against merged pixels under rotation, horizontal flip and pan, and check that scratch changes by exactly two surface allocations (backdrop + clipping group) when the surface is resized. A 1031 × 773 fixture with a 64 × 64 surface saves 3,187,852 estimated scratch bytes. This does **not** supply an overview LOD, and at fit-to-screen the group shaders still sample full-resolution whole-layer textures.

These are intermediate changes, **not the phase-2 acceptance gate**: live layers, masks, operation sources and legacy whole-image readback remain monolithic. Layer-version sharing and the coupled tiled display/mask conversion are still required. No second renderer or higher document limit has been introduced. A new distant-dab GPU regression checks the sparse history allocation across separate frame submissions, exact undo/redo, and replacement of the redo branch.

### Phase-6 prerequisite — Streaming native PNG encoding

Transform content-bounds selection also consumes mapped readback rows directly instead of allocating a full decoded CPU layer. A GPU regression checks alpha at the bottom/right partial tile, invalidation after clear and restoration after undo. This still stages a full-layer GPU buffer and scans synchronously; sparse per-tile bounds metadata and incremental transform readiness remain necessary in phase 3.

Native export now composites premultiplied RGBA8 one row at a time into a directly streamed PNG writer inside `AtomicWriteFile`, instead of allocating a second full-size composite or complete encoded PNG. `LayerReadback::for_each_row` now visits mapped padded GPU buffers without creating whole decoded CPU images; the native export worker borrows those frozen source rows, composites them, and streams output. It shares the blend/rounding implementation with the whole-image raster adapter. Tests cover odd-width multi-row clipped/opacity layers, row alignment/snapshot isolation from a real GPU readback, early callback errors/unmapping, short/out-of-order PNG rows and preservation of an existing export on error. `png = 0.18.1` is a direct root dependency. **Export is not yet bounded end-to-end**: `Canvas::begin_document_layer_readback` still stages every whole-layer GPU texture at once, and layer storage itself is monolithic. This must become a versioned tile/band snapshot before lifting document limits. Browser artwork export is unchanged.

### Phase-4 prerequisite — Native revision path leases

`ArtworkStore` now shares path leases across store instances for the same root in this process. A lease also holds the shared registry alive after all store handles are dropped; released revision entries are pruned from its weak-reference index. Loading an artwork pins its revision before reading the document; catalog summaries pin thumbnail paths; background thumbnail and reference decoders carry the pin until they finish. In-progress temporary and published-but-not-yet-pointed-to revisions are pinned across atomic publication. Saves/catalog cleanup skip pinned old revisions, then later scans remove them after the last reader releases its lease. Deletion refuses to invalidate live readers (and the gallery drops its own summary pin before trying). Tests cover held layer/thumbnail paths across saves and scans, shared store handles, deletion, pinned temporary writes, and the rename-to-pointer-publication window.

The existing native gallery reader now inspects each image header and rejects dimensions that disagree with the validated manifest **before** decoding the layer payload. A regression uses a 30K × 30K PNG header with no decodable pixel data to verify the mismatch is returned before the decoder tries to allocate pixels. This does not make v3 loading incremental: every matching layer is still decoded eagerly.

This is groundwork, **not a v4 tiled save or lazy reader**: existing PNGs are still whole layers, loaded eagerly, and the pointer/store remain single-writer by application convention rather than cross-process locking. Leases protect paths inside this process, not external modifications or another process opening the same root. Native schema 4, incremental tile writes, bounded autosave and explicit host ownership still remain. No artwork schema change was published.

Workspace tests, headless GPU tests, clippy and WASM compilation pass for these intermediate conversions. Phases 3–7 remain unimplemented. Continue phase 2 before claiming tiled painting or bounded residency.

