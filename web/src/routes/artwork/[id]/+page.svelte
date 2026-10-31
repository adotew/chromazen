<script lang="ts">
  import Brush from '@lucide/svelte/icons/brush'
  import Eraser from '@lucide/svelte/icons/eraser'
  import Redo2 from '@lucide/svelte/icons/redo-2'
  import Undo2 from '@lucide/svelte/icons/undo-2'
  import X from '@lucide/svelte/icons/x'
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
    setWorkspaceDarkMode(darkMode: boolean): void
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
  let strokeUsesPressure = false
  type Tool = 'brush' | 'eraser' | 'smudge'
  const toolIds: Record<Tool, number> = { brush: 0, eraser: 1, smudge: 2 }
  type BrushInfo = { id: string; name: string; sizeMin: number; sizeMax: number; sizeDefault: number }
  const brushStorageKey = 'chromazen.brush'
  const pressureHintStorageKey = 'chromazen.safariPressureHintDismissed'

  let loading = $state(true)
  let error = $state('')
  let isLinux = $state(false)
  let isMacSafari = false
  let pressureHint = $state(false)
  let persistenceError = $state('')
  let saveState = $state<SaveState>('idle')
  let manualSaving = $state(false)
  let manualSaved = $state(false)
  let tool = $state<Tool>('brush')
  let brushes = $state<BrushInfo[]>([])
  let brushId = $state('')
  let brushSize = $state(300)
  let activeBrush = $derived(brushes.find((brush) => brush.id === brushId))
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

  async function manualSave() {
    manualSaving = true
    try {
      await saver.manualSave()
    } finally {
      manualSaving = false
    }
  }

  onMount(() => {
    let disposed = false
    const darkMode = window.matchMedia('(prefers-color-scheme: dark)')
    function themeChanged() {
      renderer?.setWorkspaceDarkMode(darkMode.matches)
      requestFrame()
    }
    darkMode.addEventListener('change', themeChanged)
    mounted = true
    isLinux = /Linux/.test(navigator.userAgent) && !/Android/.test(navigator.userAgent)
    // iPadOS Safari also reports "Macintosh" but has touch points and real Apple Pencil pressure.
    isMacSafari =
      /Macintosh/.test(navigator.userAgent) &&
      /Version\/[\d.]+.*Safari/.test(navigator.userAgent) &&
      navigator.maxTouchPoints === 0

    async function start() {
      try {
        const wasm = await import('$lib/wasm/chromazen_web')
        await wasm.default()
        if (disposed) return

        const { width, height, scale } = canvasSize()
        const created = await wasm.WebCanvas.create(canvasElement, width, height, scale, darkMode.matches)
        if (disposed) {
          created.free()
          return
        }
        renderer = created as Renderer
        renderer.setWorkspaceDarkMode(darkMode.matches)
        brushes = wasm.WebCanvas.brushes() as BrushInfo[]
        selectBrush(localStorage.getItem(brushStorageKey) ?? brushes[0]?.id ?? '')
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
      darkMode.removeEventListener('change', themeChanged)
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
    if (!strokeUsesPressure) lastPressure = 1
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
      // Some macOS browsers report tablet pens as mice; plain mice only report 0.5 while pressed.
      strokeUsesPressure = event.pointerType !== 'mouse' || (event.pressure > 0 && event.pressure !== 0.5)
      lastPressure = strokeUsesPressure ? event.pressure || 0.5 : 1
      if (isMacSafari && !strokeUsesPressure && !localStorage.getItem(pressureHintStorageKey)) {
        pressureHint = true
      }
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
      void manualSave()
      return
    }
    if (event.code !== 'Space' || activePointer !== undefined) return
    event.preventDefault()
    // Space always pans; drop focus so it can't open or activate a toolbar control.
    if (document.activeElement instanceof HTMLElement) document.activeElement.blur()
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

  function dismissPressureHint() {
    pressureHint = false
    localStorage.setItem(pressureHintStorageKey, '1')
  }

  function selectTool(next: Tool) {
    tool = next
    renderer?.setTool(toolIds[next])
    requestFrame()
  }

  function selectBrush(id: string) {
    const brush = brushes.find((candidate) => candidate.id === id) ?? brushes[0]
    if (!renderer || !brush) return
    renderer.setBrush(brush.id)
    brushId = brush.id
    brushSize = brush.sizeDefault
    localStorage.setItem(brushStorageKey, brush.id)
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
  <header class="pointer-events-none fixed top-3 left-3 z-2">
    <Menu onSave={manualSave} onReturnToGallery={saver.returnToGallery} />
  </header>

  <div
    class="toolbar fixed inset-x-0 z-2 mx-auto flex h-[42px] w-fit max-w-[calc(100%-1rem)] items-center gap-1 rounded-full bg-[rgb(18_18_16/0.72)] px-[0.4rem] backdrop-blur-[18px] backdrop-saturate-120"
    role="toolbar"
    aria-label="Painting tools"
  >
    {#each [['brush', 'Brush', Brush], ['eraser', 'Eraser', Eraser], ['smudge', 'Smudge', WavesHorizontal]] as const as [id, label, Icon] (id)}
      <button
        class={[
          'grid size-8 shrink-0 cursor-pointer place-items-center rounded-full border-0 bg-transparent p-0 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-foreground',
          tool === id ? 'text-[#0a84ff]' : 'text-[#c7c4bc] hover:text-white',
        ]}
        type="button"
        aria-label={label}
        aria-pressed={tool === id}
        title={label}
        onclick={() => selectTool(id)}
      >
        <Icon size={20} aria-hidden="true" />
      </button>
    {/each}

    <select
      class="h-8 min-w-0 shrink cursor-pointer rounded-full border-0 bg-transparent px-1 text-xs text-[#c7c4bc] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-foreground"
      aria-label="Brush preset"
      value={brushId}
      onchange={(event) => {
        selectBrush(event.currentTarget.value)
        event.currentTarget.blur()
      }}
    >
      {#each brushes as brush (brush.id)}
        <option value={brush.id}>{brush.name}</option>
      {/each}
    </select>

    <label
      class="grid size-8 shrink-0 place-items-center overflow-hidden rounded-full border border-white/[0.18]"
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

  <aside
    class="side-controls fixed top-1/2 right-3 z-2 flex w-[42px] -translate-y-1/2 flex-col items-center gap-3 rounded-full bg-[rgb(18_18_16/0.72)] py-4 backdrop-blur-[18px] backdrop-saturate-120"
    aria-label="Canvas controls"
  >
    <label class="size-control flex items-center gap-2 text-xs text-muted">
      <span class="sr-only">Size</span>
      <input
        class="accent-foreground"
        type="range"
        min={activeBrush?.sizeMin ?? 1}
        max={activeBrush?.sizeMax ?? 2000}
        step="1"
        bind:value={brushSize}
        oninput={resizeBrush}
      />
      <output class="text-foreground tabular-nums">{brushSize}</output>
    </label>
    <div class="flex flex-col items-center gap-1">
      {#each [['undo', 'Undo', Undo2], ['redo', 'Redo', Redo2]] as const as [action, label, Icon] (action)}
        <button
          class="grid size-8 shrink-0 cursor-pointer place-items-center rounded-full border-0 bg-transparent p-0 text-[#c7c4bc] hover:text-white focus-visible:bg-white/[0.08] focus-visible:text-white focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-foreground"
          type="button"
          aria-label={label}
          title={label}
          onclick={() => command(action)}
        >
          <Icon size={20} aria-hidden="true" />
        </button>
      {/each}
    </div>
  </aside>

  {#if persistenceError}
    <div
      class="fixed top-3 right-3 z-3 rounded-full bg-[rgb(18_18_16/0.72)] px-3 py-[0.35rem] text-xs text-[#ffb4ab] backdrop-blur-[18px]"
      title={persistenceError}>Not saved</div
    >
  {:else if manualSaving && saveState === 'saving'}
    <div
      class="fixed top-3 right-3 z-3 rounded-full bg-[rgb(18_18_16/0.72)] px-3 py-[0.35rem] text-xs text-muted backdrop-blur-[18px]"
      >Saving…</div
    >
  {:else if manualSaved}
    <div
      class="fixed top-3 right-3 z-3 rounded-full bg-[rgb(18_18_16/0.72)] px-3 py-[0.35rem] text-xs text-muted backdrop-blur-[18px]"
      >Saved</div
    >
  {/if}

  {#if pressureHint}
    <div
      class="fixed top-17 left-3 z-3 flex max-w-[22rem] items-start gap-2 rounded-[0.8rem] bg-[rgb(18_18_16/0.72)] py-[0.35rem] pr-1 pl-[0.6rem] text-xs leading-relaxed text-muted backdrop-blur-[18px]"
      role="status"
    >
      <span>Pen pressure isn't available in Safari on macOS. Use a different browser for pressure-sensitive drawing.</span>
      <button
        class="grid size-5 shrink-0 cursor-pointer place-items-center rounded-[0.3rem] border-0 bg-transparent p-0 text-muted hover:text-white focus-visible:outline-2 focus-visible:outline-foreground"
        aria-label="Dismiss"
        title="Dismiss"
        onclick={dismissPressureHint}
      >
        <X size={14} aria-hidden="true" />
      </button>
    </div>
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

  .toolbar {
    bottom: calc(12px + env(safe-area-inset-bottom));
  }

  .side-controls .size-control {
    flex-direction: column;
  }

  .side-controls .size-control input {
    width: 1.5rem;
    height: 8rem;
    direction: rtl;
    writing-mode: vertical-lr;
  }
</style>
