<script lang="ts">
  import Brush from '@lucide/svelte/icons/brush'
  import Eraser from '@lucide/svelte/icons/eraser'
  import Redo2 from '@lucide/svelte/icons/redo-2'
  import Undo2 from '@lucide/svelte/icons/undo-2'
  import WavesHorizontal from '@lucide/svelte/icons/waves-horizontal'
  import { onMount } from 'svelte'
  import Menu from '$lib/components/Menu.svelte'
  import { getArtwork, type StoredDocument } from '$lib/artworks'
  import { ArtworkSaver, type SaveState } from '$lib/artwork-saving'
  import type { WebCanvas } from '$lib/wasm/chromazen_web'

  type Renderer = WebCanvas & {
    setTool(tool: number): void
    panBy(deltaX: number, deltaY: number): boolean
    zoomAt(factor: number, x: number, y: number): boolean
    saveDocument(): Promise<StoredDocument>
    loadDocument(document: StoredDocument): void
  }

  let { data }: { data: { id: string } } = $props()
  let canvasElement: HTMLCanvasElement
  let workspace: HTMLElement
  let renderer: Renderer | undefined
  let resizeObserver: ResizeObserver | undefined
  let frame = 0
  let activePointer: number | undefined
  let panning = $state(false)
  let spacePressed = $state(false)
  let lastPanPoint = [0, 0]
  let strokeStartedAt = 0
  let lastPressure = 1
  type Tool = 'brush' | 'eraser' | 'smudge'
  const toolIds: Record<Tool, number> = { brush: 0, eraser: 1, smudge: 2 }

  let loading = $state(true)
  let error = $state('')
  let isLinux = $state(false)
  let persistenceError = $state('')
  let saveState = $state<SaveState>('idle')
  let manualSaved = $state(false)
  let tool = $state<Tool>('brush')
  let brushSize = $state(500)
  let color = $state('#1d4ed8')
  let createdAt = Date.now()
  let mounted = false
  const saver = new ArtworkSaver({
    getId: () => data.id,
    getRenderer: () => renderer,
    getCreatedAt: () => createdAt,
    isInteractionActive: () => activePointer !== undefined,
    isMounted: () => mounted,
    onStateChange: (state) => (saveState = state),
    onError: (message) => (persistenceError = message),
    onManualSavedChange: (saved) => (manualSaved = saved),
  })

  onMount(() => {
    let disposed = false
    mounted = true
    isLinux = /Linux/.test(navigator.userAgent) && !/Android/.test(navigator.userAgent)

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
        let isNew = false
        try {
          const artwork = await getArtwork(data.id)
          if (disposed) return
          if (artwork) {
            renderer.loadDocument(artwork.document)
            createdAt = artwork.createdAt
            color = rgbToHex(artwork.document.brushColor)
          } else {
            applyColor()
            isNew = true
          }
          saver.setReady(!isNew)
        } catch (cause) {
          persistenceError = errorMessage(cause)
          applyColor()
        }
        resizeObserver = new ResizeObserver(resize)
        resizeObserver.observe(workspace)
        loading = false
        requestFrame()
        if (isNew) void saver.saveInitial()
      } catch (cause) {
        error = cause instanceof Error ? cause.message : String(cause)
        loading = false
      }
    }

    void start()
    return () => {
      disposed = true
      mounted = false
      saver.dispose()
      resizeObserver?.disconnect()
      if (frame) cancelAnimationFrame(frame)
      renderer?.free()
    }
  })

  function rgbToHex([red, green, blue]: [number, number, number, number]) {
    return `#${[red, green, blue].map((value) => value.toString(16).padStart(2, '0')).join('')}`
  }

  function errorMessage(cause: unknown) {
    return cause instanceof Error ? cause.message : String(cause)
  }

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
      const samplesChanged = renderer.pushStrokeSamples(samples(event))
      const strokeChanged = renderer.endStroke()
      if (samplesChanged || strokeChanged) saver.markDirty()
    }
    activePointer = undefined
    panning = false
    saver.interactionEnded()
    if (canvasElement.hasPointerCapture(event.pointerId)) {
      canvasElement.releasePointerCapture(event.pointerId)
    }
    requestFrame()
  }

  function pointerCancel(event: PointerEvent) {
    if (!renderer || event.pointerId !== activePointer) return
    if (!panning && renderer.endStroke()) saver.markDirty()
    activePointer = undefined
    panning = false
    saver.interactionEnded()
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
    if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 's') {
      event.preventDefault()
      void saver.manualSave()
      return
    }
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
    if (!panning && renderer?.endStroke()) saver.markDirty()
    if (canvasElement.hasPointerCapture(activePointer)) canvasElement.releasePointerCapture(activePointer)
    activePointer = undefined
    panning = false
    saver.interactionEnded()
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

  function applyColor(next = color, persist = false) {
    if (!renderer) return
    const value = Number.parseInt(next.slice(1), 16)
    renderer.setColor((value >> 16) & 255, (value >> 8) & 255, value & 255)
    if (persist) saver.markDirty()
  }

  function command(action: 'undo' | 'redo') {
    if (renderer?.[action]()) saver.markDirty()
    requestFrame()
  }
</script>

<svelte:window
  onkeydown={keyDown}
  onkeyup={keyUp}
  onblur={windowBlur}
  onbeforeunload={saver.beforeUnload}
/>

<svelte:head>
  <title>Web App — Chromazen</title>
  <meta name="robots" content="noindex" />
  <meta
    name="description"
    content="Try Chromazen's GPU painting canvas directly in your browser."
  />
</svelte:head>

<main class="flex min-h-svh bg-[#292926] p-0">
  <header
    class="pointer-events-none fixed top-0 left-0 z-2 flex min-h-17 w-full items-center justify-between gap-4 px-4 py-[0.65rem] max-[35rem]:min-h-12 max-[35rem]:p-2"
  >
    <Menu onSave={saver.manualSave} onReturnToGallery={saver.returnToGallery} />
    <div
      class="pointer-events-auto absolute top-0 left-1/2 flex h-12 -translate-x-1/2 items-center gap-[0.6rem] rounded-b-2xl bg-[rgb(18_18_16/0.72)] px-4 py-2 backdrop-blur-[18px] backdrop-saturate-120 max-[35rem]:gap-[0.4rem] max-[35rem]:px-[0.65rem]"
      aria-label="Painting tools"
    >
      <div class="flex items-center gap-[0.6rem]">
        <button
          class={[
            'grid min-h-8 w-8 cursor-pointer place-items-center rounded-[0.35rem] border-0 bg-transparent p-[0.35rem] text-xs text-[#c7c4bc] hover:bg-white/[0.08] hover:text-white focus-visible:bg-white/[0.08] focus-visible:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-foreground max-[35rem]:px-2',
            tool === 'brush' &&
              'bg-white/[0.14] text-white shadow-[inset_0_0_0_1px_rgb(255_255_255/0.12)]',
          ]}
          type="button"
          aria-label="Brush"
          title="Brush"
          onclick={() => selectTool('brush')}
        >
          <Brush size={20} aria-hidden="true" />
        </button>
        <button
          class={[
            'grid min-h-8 w-8 cursor-pointer place-items-center rounded-[0.35rem] border-0 bg-transparent p-[0.35rem] text-xs text-[#c7c4bc] hover:bg-white/[0.08] hover:text-white focus-visible:bg-white/[0.08] focus-visible:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-foreground max-[35rem]:px-2',
            tool === 'eraser' &&
              'bg-white/[0.14] text-white shadow-[inset_0_0_0_1px_rgb(255_255_255/0.12)]',
          ]}
          type="button"
          aria-label="Eraser"
          title="Eraser"
          onclick={() => selectTool('eraser')}
        >
          <Eraser size={20} aria-hidden="true" />
        </button>
        <button
          class={[
            'grid min-h-8 w-8 cursor-pointer place-items-center rounded-[0.35rem] border-0 bg-transparent p-[0.35rem] text-xs text-[#c7c4bc] hover:bg-white/[0.08] hover:text-white focus-visible:bg-white/[0.08] focus-visible:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-foreground max-[35rem]:px-2',
            tool === 'smudge' &&
              'bg-white/[0.14] text-white shadow-[inset_0_0_0_1px_rgb(255_255_255/0.12)]',
          ]}
          type="button"
          aria-label="Smudge"
          title="Smudge"
          onclick={() => selectTool('smudge')}
        >
          <WavesHorizontal size={20} aria-hidden="true" />
        </button>
      </div>

      <label
        class="grid size-8 place-items-center overflow-hidden rounded-full border border-white/[0.18]"
        aria-label="Brush color"
      >
        <input
          class="color-input size-8 cursor-pointer appearance-none rounded-full border-0 bg-transparent p-0 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-foreground"
          type="color"
          bind:value={color}
          oninput={(event) => applyColor(event.currentTarget.value, true)}
        />
      </label>
    </div>
  </header>

  <aside
    class="side-controls fixed top-1/2 right-0 z-2 flex w-12 -translate-y-1/2 flex-col items-center gap-3 rounded-l-[1.25rem] bg-[rgb(18_18_16/0.72)] px-2 py-4 backdrop-blur-[18px] backdrop-saturate-120"
    aria-label="Canvas controls"
  >
    <label class="size-control flex items-center gap-2 text-xs text-muted">
      <span class="max-[35rem]:hidden">Size</span>
      <input
        class="h-32 w-6 accent-foreground [direction:rtl] [writing-mode:vertical-lr]"
        type="range"
        min="2"
        max="2000"
        step="1"
        bind:value={brushSize}
        oninput={resizeBrush}
      />
      <output class="w-auto text-right text-foreground tabular-nums max-[35rem]:hidden"
        >{brushSize}</output
      >
    </label>

    <div class="actions flex items-center gap-[0.6rem]">
      <button
        class="grid min-h-8 w-8 cursor-pointer place-items-center rounded-[0.35rem] border-0 bg-transparent p-[0.35rem] text-xs text-[#c7c4bc] hover:bg-white/[0.08] hover:text-white focus-visible:bg-white/[0.08] focus-visible:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-foreground max-[35rem]:px-2"
        type="button"
        aria-label="Undo"
        title="Undo"
        onclick={() => command('undo')}
      >
        <Undo2 size={20} aria-hidden="true" />
      </button>
      <button
        class="grid min-h-8 w-8 cursor-pointer place-items-center rounded-[0.35rem] border-0 bg-transparent p-[0.35rem] text-xs text-[#c7c4bc] hover:bg-white/[0.08] hover:text-white focus-visible:bg-white/[0.08] focus-visible:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-foreground max-[35rem]:px-2"
        type="button"
        aria-label="Redo"
        title="Redo"
        onclick={() => command('redo')}
      >
        <Redo2 size={20} aria-hidden="true" />
      </button>
    </div>
  </aside>

  {#if persistenceError}
    <div
      class="fixed right-3 bottom-3 z-3 rounded-[0.4rem] bg-[rgb(18_18_16/0.72)] px-[0.6rem] py-[0.35rem] text-xs text-[#ffb4ab] backdrop-blur-[18px]"
      title={persistenceError}>Not saved</div
    >
  {:else if saveState === 'saving'}
    <div
      class="fixed right-3 bottom-3 z-3 rounded-[0.4rem] bg-[rgb(18_18_16/0.72)] px-[0.6rem] py-[0.35rem] text-xs text-muted backdrop-blur-[18px]"
      >Saving…</div
    >
  {:else if manualSaved}
    <div
      class="fixed right-3 bottom-3 z-3 rounded-[0.4rem] bg-[rgb(18_18_16/0.72)] px-[0.6rem] py-[0.35rem] text-xs text-muted backdrop-blur-[18px]"
      >Saved</div
    >
  {/if}

  <section class="fixed inset-0" bind:this={workspace} aria-label="Painting canvas">
    <canvas
      bind:this={canvasElement}
      class={[
        'block size-full touch-none',
        panning ? 'cursor-grabbing' : spacePressed ? 'cursor-grab' : 'cursor-crosshair',
      ]}
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
      <div class="absolute inset-0 grid place-content-center bg-[#292926] p-8 text-center text-muted">
        Starting canvas…
      </div>
    {:else if error}
      <div
        class="absolute inset-0 grid place-content-center gap-2 bg-[#292926] p-8 text-center text-muted"
      >
        <strong class="text-foreground">WebGPU could not start</strong>
        <span class="max-w-[30rem] text-[0.85rem]">{error}</span>
        {#if isLinux}
          <p class="mt-3 mb-0 max-w-[30rem] text-sm leading-relaxed">
            On Linux with Chrome or Chromium, open
            <code class="select-all rounded bg-black/25 px-1.5 py-0.5 text-foreground"
              >chrome://flags/#enable-vulkan</code
            >, enable Vulkan, and relaunch the browser.
          </p>
        {/if}
      </div>
    {/if}
  </section>
</main>

<style>
  :global(html),
  :global(body) {
    overflow: hidden;
  }

  .color-input::-webkit-color-swatch-wrapper {
    padding: 0;
  }

  .color-input::-webkit-color-swatch,
  .color-input::-moz-color-swatch {
    border: 0;
    border-radius: 50%;
  }

  .side-controls .size-control,
  .side-controls .actions {
    flex-direction: column;
  }

  @media (max-width: 52rem), (max-height: 32rem) {
    .side-controls {
      top: auto;
      right: 50%;
      bottom: 0;
      width: auto;
      flex-direction: row;
      gap: 1rem;
      padding: 0.5rem 0.85rem calc(0.5rem + env(safe-area-inset-bottom));
      border-radius: 1rem 1rem 0 0;
      transform: translateX(50%);
    }

    .side-controls .size-control,
    .side-controls .actions {
      flex-direction: row;
    }

    .size-control input {
      width: 6rem;
      height: 1.5rem;
      direction: ltr;
      writing-mode: horizontal-tb;
    }
  }
</style>
