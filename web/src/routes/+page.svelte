<script lang="ts">
  const repositoryUrl = 'https://github.com/adotew/chromazen'
</script>

<svelte:head>
  <title>Chromazen — Minimal Native Painting App</title>
  <link rel="icon" href="/favicon.png" />
  <meta
    name="description"
    content="Chromazen is a minimal native painting application focused on brush performance."
  />
</svelte:head>

<main>
  <div class="intro">
    <img class="app-icon" src="/favicon.png" alt="" width="512" height="512" />
    <h1>Chromazen</h1>
    <p>A lightweight, distraction-free painting application focused on performance.</p>
    <div class="download-buttons">
      <a class="download-button secondary" href="/demo">Web Demo</a>
      <a class="download-button" href="/download">Download</a>
    </div>
    <a class="github-button" href={repositoryUrl} target="_blank" rel="noreferrer">
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <path
          d="M12 2C6.48 2 2 6.58 2 12.23c0 4.52 2.87 8.35 6.84 9.71.5.1.68-.22.68-.49v-1.91c-2.78.62-3.37-1.21-3.37-1.21-.45-1.18-1.11-1.49-1.11-1.49-.91-.64.07-.63.07-.63 1 .08 1.53 1.06 1.53 1.06.9 1.56 2.35 1.11 2.92.85.09-.66.35-1.11.64-1.37-2.22-.26-4.56-1.14-4.56-5.06 0-1.12.39-2.03 1.03-2.75-.1-.26-.45-1.3.1-2.71 0 0 .84-.28 2.75 1.05A9.4 9.4 0 0 1 12 6.95a9.4 9.4 0 0 1 2.5.34c1.91-1.33 2.75-1.05 2.75-1.05.55 1.41.2 2.45.1 2.71.64.72 1.03 1.63 1.03 2.75 0 3.93-2.34 4.8-4.57 5.05.36.32.68.94.68 1.9v2.81c0 .27.18.59.69.49A10.24 10.24 0 0 0 22 12.23C22 6.58 17.52 2 12 2Z"
        />
      </svg>
      GitHub
    </a>
  </div>
</main>
