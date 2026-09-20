<script lang="ts">
  import { goto } from '$app/navigation'
  import { onMount } from 'svelte'
  import { deleteArtwork, listArtworks, type ArtworkRecord } from '$lib/artworks'

  type GalleryArtwork = ArtworkRecord & { previewUrl: string }

  let artworks = $state<GalleryArtwork[]>([])
  let loading = $state(true)
  let error = $state('')

  onMount(() => {
    let disposed = false

    void listArtworks()
      .then((records) => {
        if (disposed) return
        artworks = records.map((record) => ({
          ...record,
          previewUrl: URL.createObjectURL(
            new Blob([record.document.layers[0].png.slice().buffer], { type: 'image/png' }),
          ),
        }))
      })
      .catch((cause) => {
        error = cause instanceof Error ? cause.message : String(cause)
      })
      .finally(() => {
        loading = false
      })

    return () => {
      disposed = true
      for (const artwork of artworks) URL.revokeObjectURL(artwork.previewUrl)
    }
  })

  function newArtwork() {
    void goto(`/artwork/${crypto.randomUUID()}`)
  }

  async function removeArtwork(artwork: GalleryArtwork) {
    if (!confirm(`Delete “${artwork.title}”?`)) return
    try {
      await deleteArtwork(artwork.id)
      URL.revokeObjectURL(artwork.previewUrl)
      artworks = artworks.filter((candidate) => candidate.id !== artwork.id)
    } catch (cause) {
      error = cause instanceof Error ? cause.message : String(cause)
    }
  }
</script>

<svelte:head>
  <title>Your artwork — Chromazen Web</title>
  <meta name="robots" content="noindex" />
  <meta
    name="description"
    content="Create and reopen artwork stored privately in your browser."
  />
</svelte:head>

<main class="mx-auto block w-full max-w-6xl px-8 py-12 max-[35rem]:px-4 max-[35rem]:py-8">
  <header class="flex items-center justify-between gap-60">
    <h1
      class="m-0 flex items-center gap-2 font-elms text-[clamp(1.5rem,4vw,2rem)] leading-none font-light"
    >
      Chromazen
      <span
        class="inline-flex translate-y-0.5 items-center rounded-lg bg-foreground px-1.5 py-1 text-[0.5em] leading-none font-bold tracking-[0.01em] text-background"
        >Web</span
      >
    </h1>
    <button
      class="grid size-8 cursor-pointer place-items-center rounded-lg border-0 bg-foreground text-background"
      type="button"
      aria-label="New artwork"
      onclick={newArtwork}
    >
      <svg class="size-6" viewBox="0 0 24 24" aria-hidden="true">
        <path d="M12 5v14M5 12h14" fill="none" stroke="currentColor" stroke-width="2" />
      </svg>
    </button>
  </header>

  {#if error}
    <p class="mt-20 text-center text-[#ffb4ab]">{error}</p>
  {:else if loading}
    <p class="mt-20 text-center">Loading artwork…</p>
  {:else if artworks.length === 0}
    <section class="mt-20 text-center">
      <h2 class="m-0 font-elms font-light">Start painting</h2>
      <p class="mt-2 mb-0 text-muted">Your artwork will appear here and save automatically.</p>
    </section>
  {:else}
    <section
      class="mt-12 grid grid-cols-[repeat(auto-fill,minmax(14rem,1fr))] gap-6"
      aria-label="Your artwork"
    >
      {#each artworks as artwork (artwork.id)}
        <article class="overflow-hidden rounded-xl border border-[rgb(128_128_128/0.25)]">
          <a
            class="grid aspect-4/3 place-items-center overflow-hidden bg-white"
            href={`/artwork/${artwork.id}`}
            aria-label={`Open ${artwork.title}`}
          >
            <img class="size-full object-contain" src={artwork.previewUrl} alt="" />
          </a>
          <div class="flex items-center justify-between gap-4 p-4">
            <div class="min-w-0">
              <a
                class="block overflow-hidden font-semibold text-ellipsis whitespace-nowrap no-underline"
                href={`/artwork/${artwork.id}`}>{artwork.title}</a
              >
              <small class="mt-1 block text-muted"
                >{new Date(artwork.updatedAt).toLocaleString()}</small
              >
            </div>
            <button
              class="cursor-pointer border-0 bg-transparent p-0 text-xs font-normal text-muted underline"
              type="button"
              aria-label={`Delete ${artwork.title}`}
              onclick={() => removeArtwork(artwork)}>Delete</button
            >
          </div>
        </article>
      {/each}
    </section>
  {/if}
</main>
