<script lang="ts">
  import { logs, logText } from "../stores/logs";

  let scrollContainer: HTMLDivElement;
  let wasAtBottom = true;

  function onScroll() {
    if (!scrollContainer) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollContainer;
    wasAtBottom = scrollHeight - scrollTop - clientHeight < 20;
  }

  $: if ($logs.length > 0 && scrollContainer && wasAtBottom) {
    scrollContainer.scrollTop = scrollContainer.scrollHeight;
  }

</script>

<div class="card preset-filled-surface-100-900 border border-surface-200-800 flex flex-col h-full">
  <header class="flex items-center justify-between p-2 border-b border-surface-200-800">
    <span class="text-sm font-semibold">Terminal</span>
    <div class="flex items-center gap-1.5">
      <button
        class="btn preset-tonal text-xs px-2 py-1 flex items-center gap-1"
        onclick={async () => {
          try {
            await navigator.clipboard.writeText($logText);
          } catch {
            // Fallback for environments where clipboard API fails
          }
        }}
      >
        <svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2">
          <path stroke-linecap="round" stroke-linejoin="round" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
        </svg>
        Copy
      </button>
      <button
        class="btn preset-tonal-primary text-xs px-2 py-1"
        onclick={() => logs.clear()}
      >
        Clear
      </button>
    </div>
  </header>
  <div bind:this={scrollContainer} onscroll={onScroll} class="flex-1 overflow-auto p-2 terminal-log select-text">
    {#each $logs as entry}
      <div class="py-px">
        <span class="level-{entry.level}">[{entry.level.toUpperCase()}]</span>
        <span>{entry.message}</span>
      </div>
    {/each}
  </div>
</div>
