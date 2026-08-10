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
  // Tracks open/close state to ensure balanced HTML — unbalanced tags cause
  // WebKitGTK (Linux) rendering bugs where content doesn't repaint on scroll.
  function ansiToHtml(text: string): string {
    let result = '';
    let spanOpen = false;
    let i = 0;
    while (i < text.length) {
      if (text[i] === '\x1b' && i + 1 < text.length && text[i + 1] === '[') {
        // Parse the escape sequence
        let j = i + 2;
        while (j < text.length && text[j] !== 'm' && j < i + 10) j++;
        if (j < text.length && text[j] === 'm') {
          const code = text.slice(i + 2, j);
          if (code === '0;91') {
            // Red — close any open span first, then open a new one
            if (spanOpen) result += '</span>';
            result += '<span style="color:#ef4444;">';
            spanOpen = true;
          } else if (code === '0') {
            // Reset — close any open span
            if (spanOpen) {
              result += '</span>';
              spanOpen = false;
            }
          }
          // Other ANSI codes: ignore
          i = j + 1;
          continue;
        }
      }
      result += text[i];
      i++;
    }
    // Close any dangling span at end of text
    if (spanOpen) result += '</span>';
    return result;
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
    <span class="text-sm font-semibold">Log</span>
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
    style="font-family:'Cascadia Code','Consolas','Courier New',monospace;font-size:13px;line-height:1.4;white-space:pre;transform:translateZ(0);"
  >{@html htmlContent}</pre>
</div>
