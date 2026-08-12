<script lang="ts">
  import { logs, logText, type LogEntry } from "../stores/logs";

  let scrollContainer: HTMLDivElement;
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

  // Force WebKitGTK (Linux) to repaint after log entries change.
  // WebKitGTK has a bug where content in scrolled containers doesn't
  // repaint after DOM changes — the content is in the DOM but invisible
  // until an unrelated event (hover, scroll, resize) triggers a repaint.
  // Toggling opacity in a requestAnimationFrame forces the compositor
  // to redraw the area.
  $effect(() => {
    $logs.length; // dependency
    if (scrollContainer) {
      requestAnimationFrame(() => {
        if (!scrollContainer) return;
        scrollContainer.style.opacity = "0.999";
        requestAnimationFrame(() => {
          if (!scrollContainer) return;
          scrollContainer.style.opacity = "";
        });
      });
    }
  });

  // Convert ANSI escape codes to inline HTML <span> tags.
  // Tracks open/close state to ensure balanced HTML.
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

  function levelColor(level: string): string {
    return level === 'error'
      ? 'var(--color-error-500)'
      : level === 'warn'
        ? 'var(--color-warning-500)'
        : 'var(--color-success-500)';
  }

  // Render a single log entry as HTML — scoped @html per entry (not the
  // entire log as one string) so Svelte creates individual DOM nodes via
  // {#each}, which WebKitGTK repaints more reliably than bulk innerHTML
  // replacement.
  function renderEntry(entry: LogEntry): string {
    const color = levelColor(entry.level);
    const prefix = `<span style="color:${color}">[${entry.level.toUpperCase()}]</span>`;
    const body = ansiToHtml(entry.message);
    return `${prefix} ${body}`;
  }
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
  <!-- Using {#each} with per-entry divs instead of a single @html string.
       Svelte creates individual DOM nodes for each log entry, which WebKitGTK
       (Linux) repaints correctly. The previous approach (replacing the entire
       <pre> innerHTML via @html) caused content to become invisible after
       horizontal scrolling because WebKitGTK didn't repaint the scrolled region. -->
  <div
    bind:this={scrollContainer}
    onscroll={onScroll}
    class="flex-1 overflow-auto p-2 select-text m-0"
    style="font-family:'Cascadia Code','Consolas','Courier New',monospace;font-size:13px;line-height:1.4;"
  >
    {#each $logs as entry, i (i)}
      <div style="white-space:pre;">{@html renderEntry(entry)}</div>
    {/each}
  </div>
</div>
