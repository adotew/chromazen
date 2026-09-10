<script lang="ts">
  import { onMount } from 'svelte'
  import '../app.css'

  let { children } = $props()

  onMount(async () => {
    const [{ inject }, { injectSpeedInsights }] = await Promise.all([
      import('@vercel/analytics'),
      import('@vercel/speed-insights'),
    ])

    inject()
    injectSpeedInsights()
  })
</script>

{@render children()}
