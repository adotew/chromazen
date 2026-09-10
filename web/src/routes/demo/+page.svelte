<script lang="ts">
  import Brush from '@lucide/svelte/icons/brush'
  import Eraser from '@lucide/svelte/icons/eraser'
  import Redo2 from '@lucide/svelte/icons/redo-2'
  import Undo2 from '@lucide/svelte/icons/undo-2'
  import Waves from '@lucide/svelte/icons/waves'
  import { onMount } from 'svelte'
  import type { WebCanvas } from '$lib/wasm/chromazen_web'

  type Renderer = WebCanvas & {
    setTool(tool: number): void
    panBy(deltaX: number, deltaY: number): boolean
    zoomAt(factor: number, x: number, y: number): boolean
  }

  let canvasElement: HTMLCanvasElement
  let workspace: HTMLElement
  let renderer: Renderer | undefined
  let resizeObserver: ResizeObserver | undefined
  let frame = 0
  let activePointer: number | undefined
  let panning = false
  let spacePressed = false
  let lastPanPoint = [0, 0]
  let strokeStartedAt = 0
  let lastPressure = 1
  type Tool = 'brush' | 'eraser' | 'smudge'
  const toolIds: Record<Tool, number> = { brush: 0, eraser: 1, smudge: 2 }

  let loading = true
  let error = ''
  let tool: Tool = 'brush'
  let brushSize = 48
  let color = '#151513'

  onMount(() => {
    let disposed = false

    async function start() {
      try {
        const wasm = await import('$lib/wasm/chromazen_web')
        await wasm.default()
        if (disposed) return

        const { width, height, scale } = canvasSize()
        const created = await wasm.WebCanvas.create(canvasElement, width, height, scale)
        if (disposed) {
          created.free()
          return
        }
        renderer = created as Renderer
        renderer.setBrushSize(brushSize)
        applyColor()
        resizeObserver = new ResizeObserver(resize)
        resizeObserver.observe(workspace)
        loading = false
        requestFrame()
      } catch (cause) {
        error = cause instanceof Error ? cause.message : String(cause)
        loading = false
      }
    }

    void start()
    return () => {
      disposed = true
      resizeObserver?.disconnect()
      if (frame) cancelAnimationFrame(frame)
      renderer?.free()
    }
  })

  function canvasSize() {
    const bounds = workspace.getBoundingClientRect()
    const scale = Math.min(window.devicePixelRatio || 1, 2)
    const width = Math.max(1, Math.round(bounds.width * scale))
    const height = Math.max(1, Math.round(bounds.height * scale))
    canvasElement.width = width
    canvasElement.height = height
    return { width, height, scale }
  }

  function resize() {
    if (!renderer) return
    const { width, height, scale } = canvasSize()
    renderer.resize(width, height, scale)
    requestFrame()
  }

  function requestFrame() {
    if (!renderer || frame) return
    frame = requestAnimationFrame(render)
  }

  function render() {
    frame = 0
    if (renderer?.render()) requestFrame()
  }

  function point(event: PointerEvent, bounds = canvasElement.getBoundingClientRect()) {
    if (event.pointerType === 'mouse') lastPressure = 1
    else if (event.pressure > 0) lastPressure = event.pressure
    return {
      x: event.clientX - bounds.left,
      y: event.clientY - bounds.top,
      pressure: lastPressure,
      time: Math.max(0, event.timeStamp - strokeStartedAt),
    }
  }

  function samples(event: PointerEvent) {
    const events = event.getCoalescedEvents?.() ?? []
    const points = events.length ? events : [event]
    const bounds = canvasElement.getBoundingClientRect()
    const packed = new Float32Array(points.length * 4)
    for (let index = 0; index < points.length; index++) {
      const value = point(points[index], bounds)
      const offset = index * 4
      packed[offset] = value.x
      packed[offset + 1] = value.y
      packed[offset + 2] = value.pressure
      packed[offset + 3] = value.time
    }
    return packed
  }

  function pointerDown(event: PointerEvent) {
    if (!renderer || activePointer !== undefined || !event.isPrimary) return
    const startsPan = event.button === 1 || event.button === 2 || (event.button === 0 && spacePressed)
    if (!startsPan && event.button !== 0) return

    event.preventDefault()
    activePointer = event.pointerId
    panning = startsPan
    canvasElement.setPointerCapture(event.pointerId)
    if (panning) {
      lastPanPoint = [event.clientX, event.clientY]
    } else {
      strokeStartedAt = event.timeStamp
      lastPressure = event.pointerType === 'mouse' ? 1 : event.pressure || 0.5
      const sample = point(event)
      renderer.beginStroke(sample.x, sample.y, sample.pressure, sample.time)
    }
    requestFrame()
  }

  function pointerMove(event: PointerEvent) {
    if (!renderer || event.pointerId !== activePointer) return
    event.preventDefault()
    if (panning) {
      const next = [event.clientX, event.clientY]
      if (renderer.panBy(next[0] - lastPanPoint[0], next[1] - lastPanPoint[1])) requestFrame()
      lastPanPoint = next
    } else {
      renderer.pushStrokeSamples(samples(event))
      requestFrame()
    }
  }

  function pointerUp(event: PointerEvent) {
    if (!renderer || event.pointerId !== activePointer) return
    event.preventDefault()
    if (!panning) {
      renderer.pushStrokeSamples(samples(event))
      renderer.endStroke()
    }
    activePointer = undefined
    panning = false
    if (canvasElement.hasPointerCapture(event.pointerId)) {
      canvasElement.releasePointerCapture(event.pointerId)
    }
    requestFrame()
  }

  function pointerCancel(event: PointerEvent) {
    if (!renderer || event.pointerId !== activePointer) return
    if (!panning) renderer.endStroke()
    activePointer = undefined
    panning = false
    requestFrame()
  }

  function wheel(event: WheelEvent) {
    if (!renderer) return
    event.preventDefault()
    const bounds = canvasElement.getBoundingClientRect()
    const unit = event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? bounds.height : 1
    const exponent = Math.max(-0.7, Math.min(0.7, -event.deltaY * unit * 0.0015))
    if (renderer.zoomAt(Math.exp(exponent), event.clientX - bounds.left, event.clientY - bounds.top)) {
      requestFrame()
    }
  }

  function keyDown(event: KeyboardEvent) {
    if (event.code !== 'Space' || activePointer !== undefined) return
    const target = event.target
    if (target instanceof Element && target.closest('button, input, a')) return
    event.preventDefault()
    spacePressed = true
  }

  function keyUp(event: KeyboardEvent) {
    if (event.code === 'Space') spacePressed = false
  }

  function windowBlur() {
    spacePressed = false
    if (activePointer === undefined) return
    if (!panning) renderer?.endStroke()
    if (canvasElement.hasPointerCapture(activePointer)) canvasElement.releasePointerCapture(activePointer)
    activePointer = undefined
    panning = false
    requestFrame()
  }

  function selectTool(next: Tool) {
    tool = next
    renderer?.setTool(toolIds[next])
    requestFrame()
  }

  function resizeBrush() {
    renderer?.setBrushSize(brushSize)
  }

  function applyColor() {
    if (!renderer) return
    const value = Number.parseInt(color.slice(1), 16)
    renderer.setColor((value >> 16) & 255, (value >> 8) & 255, value & 255)
  }

  function command(action: 'undo' | 'redo') {
    renderer?.[action]()
    requestFrame()
  }
</script>

<svelte:window onkeydown={keyDown} onkeyup={keyUp} onblur={windowBlur} />

<svelte:head>
  <title>Web Demo — Chromazen</title>
  <meta
    name="description"
    content="Try Chromazen's GPU painting canvas directly in your browser."
  />
</svelte:head>

<main class="demo">
  <header class="topbar">
    <a class="brand" href="/" aria-label="Back to Chromazen">
      <img src="/favicon.png" alt="" width="32" height="32" />
      <span>Chromazen</span>
    </a>

    <div class="paint-controls" aria-label="Painting tools">
      <div class="tool-group">
        <button
          class="icon-button"
          class:active={tool === 'brush'}
          type="button"
          aria-label="Brush"
          title="Brush"
          onclick={() => selectTool('brush')}
        >
          <Brush size={20} aria-hidden="true" />
        </button>
        <button
          class="icon-button"
          class:active={tool === 'eraser'}
          type="button"
          aria-label="Eraser"
          title="Eraser"
          onclick={() => selectTool('eraser')}
        >
          <Eraser size={20} aria-hidden="true" />
        </button>
        <button
          class="icon-button"
          class:active={tool === 'smudge'}
          type="button"
          aria-label="Smudge"
          title="Smudge"
          onclick={() => selectTool('smudge')}
        >
          <Waves size={20} aria-hidden="true" />
        </button>
      </div>

      <label class="color-control" aria-label="Brush color">
        <input type="color" bind:value={color} oninput={applyColor} disabled={tool !== 'brush'} />
      </label>
    </div>

    <a class="download-link" href="/download">Download App</a>
  </header>

  <aside class="side-controls" aria-label="Canvas controls">
    <label class="size-control">
      <span>Size</span>
      <input
        type="range"
        min="2"
        max="2000"
        step="1"
        bind:value={brushSize}
        oninput={resizeBrush}
      />
      <output>{brushSize}</output>
    </label>

    <div class="actions">
      <button
        class="icon-button"
        type="button"
        aria-label="Undo"
        title="Undo"
        onclick={() => command('undo')}
      >
        <Undo2 size={20} aria-hidden="true" />
      </button>
      <button
        class="icon-button"
        type="button"
        aria-label="Redo"
        title="Redo"
        onclick={() => command('redo')}
      >
        <Redo2 size={20} aria-hidden="true" />
      </button>
    </div>
  </aside>

  <section class="workspace" bind:this={workspace} aria-label="Painting canvas">
    <canvas
      bind:this={canvasElement}
      class:pan-ready={spacePressed && !panning}
      class:panning
      aria-label="Chromazen drawing canvas"
      onpointerdown={pointerDown}
      onpointermove={pointerMove}
      onpointerup={pointerUp}
      onpointercancel={pointerCancel}
      onlostpointercapture={pointerCancel}
      onwheel={wheel}
      oncontextmenu={(event) => event.preventDefault()}
    ></canvas>

    {#if loading}
      <div class="status">Starting canvas…</div>
    {:else if error}
      <div class="status error-state">
        <strong>WebGPU could not start</strong>
        <span>{error}</span>
      </div>
    {/if}
  </section>
</main>

<style>
  :global(html),
  :global(body) {
    overflow: hidden;
  }

  .demo {
    display: flex;
    min-height: 100svh;
    padding: 0;
    background: #292926;
  }

  .topbar {
    position: fixed;
    z-index: 2;
    top: 0;
    left: 0;
    display: flex;
    width: 100%;
    min-height: 4.25rem;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    padding: 0.65rem 1rem;
    border-bottom: 1px solid rgb(255 255 255 / 0.1);
    background: rgb(18 18 16 / 0.94);
    backdrop-filter: blur(14px);
  }

  .brand {
    display: inline-flex;
    align-items: center;
    gap: 0.55rem;
    flex: none;
    font-family: "Elms Sans", sans-serif;
    font-size: 1.45rem;
    font-weight: 300;
    text-decoration: none;
  }

  .brand img {
    width: 2.5rem;
    height: 2.5rem;
  }

  .paint-controls,
  .tool-group,
  .actions,
  .size-control {
    display: flex;
    align-items: center;
  }

  .paint-controls {
    position: absolute;
    left: 50%;
    gap: 0.75rem;
    transform: translateX(-50%);
  }

  .download-link {
    padding: 0.55rem 0.9rem;
    border-radius: 0.45rem;
    color: #11110f;
    background: #f1efe8;
    font-size: 0.85rem;
    font-weight: 700;
    text-decoration: none;
  }

  .download-link:hover,
  .download-link:focus-visible {
    color: #11110f;
    background: #fff;
  }

  .download-link:focus-visible {
    outline: 2px solid #f1efe8;
    outline-offset: 2px;
  }

  .side-controls {
    position: fixed;
    z-index: 2;
    top: 50%;
    right: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 0.75rem;
    padding: 1rem 0.5rem;
    border-radius: 1.25rem 0 0 1.25rem;
    background: rgb(18 18 16 / 0.9);
    backdrop-filter: blur(14px);
    transform: translateY(-50%);
  }

  .side-controls .size-control,
  .side-controls .actions {
    flex-direction: column;
  }

  .tool-group,
  .actions {
    gap: 0.2rem;
    padding: 0.2rem;
  }

  button {
    min-height: 2rem;
    padding: 0.35rem 0.65rem;
    border: 0;
    border-radius: 0.35rem;
    color: #c7c4bc;
    background: transparent;
    cursor: pointer;
    font-size: 0.8rem;
  }

  .icon-button {
    display: grid;
    width: 2rem;
    padding: 0.35rem;
    place-items: center;
  }

  button:hover,
  button:focus-visible,
  button.active {
    color: #11110f;
    background: #f1efe8;
  }

  button:focus-visible,
  input:focus-visible {
    outline: 2px solid #f1efe8;
    outline-offset: 2px;
  }

  .color-control {
    display: grid;
    width: 2rem;
    height: 2rem;
    overflow: hidden;
    border: 1px solid rgb(255 255 255 / 0.18);
    border-radius: 50%;
    place-items: center;
  }

  .color-control:has(input:disabled) {
    opacity: 0.35;
  }

  input[type='color'] {
    width: 2.8rem;
    height: 2.8rem;
    padding: 0;
    border: 0;
    background: none;
    cursor: pointer;
  }

  .size-control {
    gap: 0.5rem;
    color: #aaa79e;
    font-size: 0.75rem;
  }

  .size-control input {
    width: 1.5rem;
    height: 8rem;
    direction: rtl;
    accent-color: #f1efe8;
    writing-mode: vertical-lr;
  }

  .size-control output {
    width: auto;
    color: #f1efe8;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .workspace {
    position: fixed;
    inset: 4.25rem 0 0;
  }

  canvas {
    display: block;
    width: 100%;
    height: 100%;
    cursor: crosshair;
    touch-action: none;
  }

  canvas.pan-ready {
    cursor: grab;
  }

  canvas.panning {
    cursor: grabbing;
  }

  .status {
    position: absolute;
    inset: 0;
    display: grid;
    padding: 2rem;
    color: #aaa79e;
    background: #292926;
    place-content: center;
    text-align: center;
  }

  .error-state {
    gap: 0.5rem;
  }

  .error-state strong {
    color: #f1efe8;
  }

  .error-state span {
    max-width: 30rem;
    font-size: 0.85rem;
  }

  @media (max-width: 52rem) {
    .topbar {
      min-height: 7.5rem;
      align-items: flex-start;
    }

    .paint-controls {
      bottom: 0.6rem;
    }

    .workspace {
      inset-block-start: 7.5rem;
    }
  }

  @media (max-width: 35rem) {
    .size-control span,
    .size-control output {
      display: none;
    }

    .size-control input {
      height: 6rem;
    }

    button {
      padding-inline: 0.5rem;
    }
  }
</style>
