<script lang="ts">
  let {
    onSave,
    onReturnToGallery,
  }: {
    onSave: () => void
    onReturnToGallery: () => void
  } = $props()

  let open = $state(false)
  let root: HTMLElement
  let trigger: HTMLButtonElement

  function closeFromOutside(event: PointerEvent) {
    if (open && event.target instanceof Node && !root.contains(event.target)) open = false
  }

  function closeFromKeyboard(event: KeyboardEvent) {
    if (event.key !== 'Escape' || !open) return
    event.preventDefault()
    open = false
    trigger.focus()
  }

  function select(action: () => void) {
    open = false
    action()
  }
</script>

<svelte:window onkeydown={closeFromKeyboard} onpointerdown={closeFromOutside} />

<div class="pointer-events-auto relative" bind:this={root}>
  <button
    bind:this={trigger}
    class="grid size-9 cursor-pointer place-items-center border-0 bg-transparent p-0 text-[#c7c4bc] hover:text-white focus-visible:rounded-sm focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-foreground"
    type="button"
    aria-label="Application menu"
    title="Application menu"
    aria-haspopup="menu"
    aria-expanded={open}
    onclick={() => (open = !open)}
  >
    <svg
      class="size-5"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="M4 8h16" />
      <path d="M4 16h16" />
    </svg>
  </button>
  {#if open}
    <div
      class="absolute top-full left-0 mt-1 min-w-44 rounded-xl bg-[rgb(18_18_16/0.72)] p-1.5 shadow-xl backdrop-blur-[18px] backdrop-saturate-120"
      role="menu"
      aria-label="Application menu"
    >
      <button
        class="block w-full cursor-pointer rounded-lg border-0 bg-transparent px-3 py-2 text-left text-sm text-foreground hover:bg-white/10 focus-visible:bg-white/10 focus-visible:outline-none"
        type="button"
        role="menuitem"
        onclick={() => select(onSave)}
      >Save</button>
      <button
        class="block w-full cursor-pointer rounded-lg border-0 bg-transparent px-3 py-2 text-left text-sm text-foreground hover:bg-white/10 focus-visible:bg-white/10 focus-visible:outline-none"
        type="button"
        role="menuitem"
        onclick={() => select(onReturnToGallery)}
      >Return to Gallery</button>
    </div>
  {/if}
</div>
