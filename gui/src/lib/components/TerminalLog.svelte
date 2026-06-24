<script lang="ts">
  import { logs, logText } from "../stores/logs";

  let scrollContainer: HTMLPreElement;
  let wasAtBottom = true;

  function onScroll() {
    if (!scrollContainer) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollContainer;
    wasAtBottom = scrollHeight - scrollTop - clientHeight < 20;
  }

  $effect(() => {
    if ($logs.length > 0 && scrollContainer && wasAtBottom) {
      scrollContainer.scrollTop = scrollContainer.scrollHeight;
    }
  });

  // Convert ANSI escape codes to inline HTML <span> tags.
  function ansiToHtml(text: string): string {
    return text
      .replace(/\x1b\[0;91m/g, '<span style="color:#ef4444;">')
      .replace(/\x1b\[0m/g, '</span>')
      .replace(/\x1b\[[0-9;]*m/g, '');
  }

  // Build the entire terminal as a single HTML string with \n line breaks.
  // Using <pre> + \n avoids per-line <div> whitespace issues.
  function renderAll(entries: { level: string; message: string }[]): string {
    return entries
      .map((entry) => {
        const color =
          entry.level === 'error'
            ? 'var(--color-error-500)'
            : entry.level === 'warn'
              ? 'var(--color-warning-500)'
              : 'var(--color-success-500)';
        const prefix = `[${entry.level.toUpperCase()}]`;
        const body = ansiToHtml(entry.message);
        return `<span style="color:${color}">${prefix}</span> ${body}`;
      })
      .join('\n');
  }

  let htmlContent = $derived(renderAll($logs));
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
  <pre
    bind:this={scrollContainer}
    onscroll={onScroll}
    class="flex-1 overflow-auto p-2 select-text m-0"
    style="font-family:'Cascadia Code','Consolas','Courier New',monospace;font-size:13px;line-height:1.4;white-space:pre;"
  >{@html htmlContent}</pre>
</div>
