<script lang="ts">
  import { logs, logText } from "../stores/logs";

  let scrollContainer: HTMLDivElement;

  $: if ($logs.length > 0 && scrollContainer) {
    scrollContainer.scrollTop = scrollContainer.scrollHeight;
  }

  function formatTime(d: Date): string {
    return d.toLocaleTimeString("en-US", { hour12: false });
  }
</script>

<div class="card preset-filled-surface-100-900 border border-surface-200-800 flex flex-col h-full">
  <header class="flex items-center justify-between p-2 border-b border-surface-200-800">
    <span class="text-sm font-semibold">Terminal</span>
    <button
      class="btn preset-tonal-primary text-xs px-2 py-1"
      onclick={() => logs.clear()}
    >
      Clear
    </button>
  </header>
  <div bind:this={scrollContainer} class="flex-1 overflow-auto p-2 terminal-log">
    {#each $logs as entry}
      <div class="py-px">
        <span class="timestamp">[{formatTime(entry.timestamp)}]</span>
        <span class="level-{entry.level}">[{entry.level.toUpperCase()}]</span>
        <span>{entry.message}</span>
      </div>
    {/each}
  </div>
</div>
