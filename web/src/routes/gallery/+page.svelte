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

<main class="gallery-main">
  <header>
      <h1>Chromazen Web</h1>
    <button type="button" onclick={newArtwork}>New artwork</button>
  </header>

  {#if error}
    <p class="message error-message">{error}</p>
  {:else if loading}
    <p class="message">Loading artwork…</p>
  {:else if artworks.length === 0}
    <section class="empty">
      <h2>Start painting</h2>
      <p>Your artwork will appear here and save automatically.</p>
      <button type="button" onclick={newArtwork}>New artwork</button>
    </section>
  {:else}
    <section class="artwork-grid" aria-label="Your artwork">
      {#each artworks as artwork (artwork.id)}
        <article>
          <a class="preview" href={`/artwork/${artwork.id}`} aria-label={`Open ${artwork.title}`}>
            <img src={artwork.previewUrl} alt="" />
          </a>
          <div class="artwork-details">
            <div>
              <a href={`/artwork/${artwork.id}`}>{artwork.title}</a>
              <small>{new Date(artwork.updatedAt).toLocaleString()}</small>
            </div>
            <button
              class="delete-button"
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

<style>
  .gallery-main {
    display: block;
    width: min(72rem, 100%);
    margin: 0 auto;
    padding: 3rem 2rem;
  }

  header,
  .artwork-details {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }

  h1,
  h2 {
    margin: 0;
    font-family: 'Elms Sans', sans-serif;
    font-weight: 300;
  }

  h1 {
    font-size: clamp(2rem, 5vw, 3rem);
  }

  header p,
  .empty p {
    margin: 0.5rem 0 0;
  }

  button {
    padding: 0.75rem 1.2rem;
    border: 0;
    border-radius: 9999px;
    color: var(--color-background);
    background: var(--color-foreground);
    cursor: pointer;
    font: inherit;
    font-weight: 600;
  }

  .artwork-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(14rem, 1fr));
    gap: 1.5rem;
    margin-top: 3rem;
  }

  article {
    overflow: hidden;
    border: 1px solid rgb(128 128 128 / 0.25);
    border-radius: 0.75rem;
  }

  .preview {
    display: grid;
    aspect-ratio: 4 / 3;
    overflow: hidden;
    background: #fff;
    place-items: center;
  }

  .preview img {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  .artwork-details {
    padding: 1rem;
  }

  .artwork-details > div {
    min-width: 0;
  }

  .artwork-details a {
    display: block;
    overflow: hidden;
    font-weight: 600;
    text-decoration: none;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  small {
    display: block;
    margin-top: 0.25rem;
    color: var(--color-muted);
  }

  .delete-button {
    padding: 0;
    color: var(--color-muted);
    background: none;
    font-size: 0.8rem;
    font-weight: 400;
    text-decoration: underline;
  }

  .message,
  .empty {
    margin-top: 5rem;
    text-align: center;
  }

  .error-message {
    color: #ffb4ab;
  }

  @media (max-width: 35rem) {
    .gallery-main {
      padding: 2rem 1rem;
    }
  }
</style>
