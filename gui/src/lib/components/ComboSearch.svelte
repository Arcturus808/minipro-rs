<script>
  let {
    value = $bindable(''),
    placeholder = '',
    storageKey = '',
    onselect = () => {},
    onsubmit = () => {}
  } = $props();

  let isOpen = $state(false);
  let inputRef = $state(null);

  function loadEntries() {
    if (!storageKey) return [];
    try {
      const raw = localStorage.getItem(storageKey);
      return raw ? JSON.parse(raw) : [];
    } catch {
      return [];
    }
  }

  let entries = $state(loadEntries());

  $effect(() => {
    if (storageKey) {
      localStorage.setItem(storageKey, JSON.stringify(entries));
    }
  });

  // Sort favorites first, then by text
  let sortedEntries = $derived(
    [...entries].sort((a, b) => {
      if (a.isFavorite === b.isFavorite) {
        return a.text.localeCompare(b.text);
      }
      return a.isFavorite ? -1 : 1;
    })
  );

  let filteredEntries = $derived(
    value.trim()
      ? sortedEntries.filter(e =>
          e.text.toLowerCase().includes(value.toLowerCase())
        )
      : sortedEntries
  );

  function addEntry(text) {
    if (!entries.some(e => e.text === text)) {
      entries = [...entries, { id: crypto.randomUUID(), text, isFavorite: false }];
    }
  }

  function toggleFavorite(id) {
    entries = entries.map(e =>
      e.id === id ? { ...e, isFavorite: !e.isFavorite } : e
    );
  }

  function deleteEntry(id) {
    entries = entries.filter(e => e.id !== id);
  }

  function selectEntry(text) {
    value = text;
    isOpen = false;
    onselect(text);
  }

  function handleKeydown(e) {
    if (e.key === 'Enter') {
      const trimmed = value.trim();
      if (!trimmed) return;

      const match = sortedEntries.find(e => e.text.toLowerCase() === trimmed.toLowerCase());
      if (match) {
        selectEntry(match.text);
      } else {
        addEntry(trimmed);
        isOpen = false;
        onsubmit(trimmed);
      }
    } else if (e.key === 'Escape') {
      isOpen = false;
    }
  }

  function handleClickOutside(e) {
    if (inputRef && !inputRef.contains(e.target)) {
      isOpen = false;
    }
  }
</script>

<svelte:window on:click={handleClickOutside} />

<div class="relative w-full" bind:this={inputRef}>
  <input
    type="text"
    bind:value
    {placeholder}
    class="w-full rounded border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 text-surface-950-50 px-3 py-2 focus:outline-none focus:ring-2 focus:ring-blue-500"
    onfocus={() => (isOpen = true)}
    onkeydown={handleKeydown}
  />

  {#if isOpen}
    <div
      class="absolute z-10 mt-1 w-full max-h-48 overflow-y-auto rounded border border-gray-300 dark:border-slate-600 bg-white dark:bg-slate-800 shadow-lg"
    >
      {#if filteredEntries.length > 0}
        {#each filteredEntries as entry (entry.id)}
          <div
            role="button"
            tabindex="0"
            class="group flex cursor-pointer items-center justify-between px-3 py-2 hover:bg-gray-100 dark:hover:bg-slate-700"
            onclick={() => selectEntry(entry.text)}
            onkeydown={(e) => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); selectEntry(entry.text); } }}
          >
            <span class="truncate">{entry.text}</span>
            <div class="flex items-center gap-1">
              <button
                class="shrink-0 rounded p-1 hover:bg-gray-200 dark:hover:bg-slate-600"
                onclick={(e) => {
                  e.stopPropagation();
                  toggleFavorite(entry.id);
                }}
                aria-label={entry.isFavorite ? 'Unfavorite' : 'Favorite'}
              >
                <svg
                  class="h-4 w-4 transition-colors"
                  class:fill-yellow-400={entry.isFavorite}
                  class:text-yellow-400={entry.isFavorite}
                  class:fill-transparent={!entry.isFavorite}
                  class:text-gray-400={!entry.isFavorite}
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  stroke-width="2"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    d="M11.049 2.927c.3-.921 1.603-.921 1.902 0l1.519 4.674a1 1 0 00.95.69h4.915c.969 0 1.371 1.24.588 1.81l-3.976 2.888a1 1 0 00-.363 1.118l1.518 4.674c.3.922-.755 1.688-1.538 1.118l-3.976-2.888a1 1 0 00-1.176 0l-3.976 2.888c-.783.57-1.838-.197-1.538-1.118l1.518-4.674a1 1 0 00-.363-1.118l-3.976-2.888c-.784-.57-.38-1.81.588-1.81h4.914a1 1 0 00.951-.69l1.519-4.674z"
                  />
                </svg>
              </button>
              <button
                class="shrink-0 rounded p-1 opacity-0 group-hover:opacity-100 transition-all hover:bg-red-100 dark:hover:bg-red-900/30"
                onclick={(e) => {
                  e.stopPropagation();
                  deleteEntry(entry.id);
                }}
                aria-label="Delete entry"
                title="Delete"
              >
                <svg
                  class="h-4 w-4 text-gray-400 hover:text-red-500 transition-colors"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  stroke-width="2"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"
                  />
                </svg>
              </button>
            </div>
          </div>
        {/each}
      {:else}
        <div class="px-3 py-2 text-sm text-gray-500 dark:text-gray-400">No results</div>
      {/if}
    </div>
  {/if}
</div>
