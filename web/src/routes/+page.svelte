<script lang="ts">
  import ArrowDownCircle from '@lucide/svelte/icons/arrow-down-circle'

  const repositoryUrl = 'https://github.com/adotew/chromazen'
  const pageTitle = 'Chromazen — Fast, Lightweight Painting App'
  const pageDescription =
    'Chromazen is a fast, distraction-free painting app for macOS, Windows, and Linux, with pressure-sensitive brushes, layers, autosave, and PNG export.'
  const canonicalUrl = 'https://www.chromazen.app/'
</script>

<svelte:head>
  <title>{pageTitle}</title>
  <link rel="canonical" href={canonicalUrl} />
  <meta name="description" content={pageDescription} />
  <meta property="og:type" content="website" />
  <meta property="og:site_name" content="Chromazen" />
  <meta property="og:title" content={pageTitle} />
  <meta property="og:description" content={pageDescription} />
  <meta property="og:url" content={canonicalUrl} />
  <meta name="twitter:card" content="summary" />
  <meta name="twitter:title" content={pageTitle} />
  <meta name="twitter:description" content={pageDescription} />
</svelte:head>

<main class="grid min-h-svh place-items-center p-8">
  <div class="relative z-1 max-w-xl text-center">
    <img
      class="mx-auto mb-4 size-40 [filter:drop-shadow(0_0_1px_rgb(23_23_20/0.7))_drop-shadow(0_3px_8px_rgb(23_23_20/0.2))]"
      src="/logo.png"
      alt=""
      width="512"
      height="512"
    />
    <h1 class="m-0 font-elms text-[clamp(2.5rem,7vw,4rem)] leading-none font-light tracking-[-0.02em]">
      Chromazen
    </h1>
    <p class="mx-auto my-6 max-w-[30rem] text-muted leading-[1.6]">
      A lightweight, distraction-free painting application focused on performance.
    </p>
    <div class="mt-10 flex flex-row items-center justify-center gap-5">
      <a
        class="inline-flex cursor-pointer items-center gap-[0.55rem] rounded-full bg-foreground px-6 py-[0.8rem] font-semibold text-background no-underline transition-[transform,box-shadow] duration-150 hover:-translate-y-px hover:shadow-[0_6px_20px_rgb(0_0_0/0.35)] active:translate-y-0 active:shadow-none focus-visible:rounded-sm focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-current"
        href="/download"
      >
        <ArrowDownCircle size={18} aria-hidden="true" />
        Download
      </a>
      <a
        class="cursor-pointer border-0 bg-transparent p-0 font-semibold text-foreground underline decoration-1 underline-offset-[0.3em] transition-[text-decoration-thickness] duration-150 hover:decoration-2 focus-visible:rounded-sm focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-current"
        href="/gallery">Web App</a
      >
    </div>
    <a
      class="fixed top-8 right-8 inline-flex cursor-pointer items-center gap-2 font-semibold no-underline hover:opacity-85 focus-visible:rounded-sm focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-current"
      href={repositoryUrl}
      target="_blank"
      rel="noreferrer"
    >
      <svg class="size-[1.6rem] fill-current" viewBox="0 0 24 24" aria-hidden="true">
        <path
          d="M12 2C6.48 2 2 6.58 2 12.23c0 4.52 2.87 8.35 6.84 9.71.5.1.68-.22.68-.49v-1.91c-2.78.62-3.37-1.21-3.37-1.21-.45-1.18-1.11-1.49-1.11-1.49-.91-.64.07-.63.07-.63 1 .08 1.53 1.06 1.53 1.06.9 1.56 2.35 1.11 2.92.85.09-.66.35-1.11.64-1.37-2.22-.26-4.56-1.14-4.56-5.06 0-1.12.39-2.03 1.03-2.75-.1-.26-.45-1.3.1-2.71 0 0 .84-.28 2.75 1.05A9.4 9.4 0 0 1 12 6.95a9.4 9.4 0 0 1 2.5.34c1.91-1.33 2.75-1.05 2.75-1.05.55 1.41.2 2.45.1 2.71.64.72 1.03 1.63 1.03 2.75 0 3.93-2.34 4.8-4.57 5.05.36.32.68.94.68 1.9v2.81c0 .27.18.59.69.49A10.24 10.24 0 0 0 22 12.23C22 6.58 17.52 2 12 2Z"
        />
      </svg>
      GitHub
    </a>
  </div>
</main>
