<script lang="ts">
  import { isRunning, currentOperation, progress, progressPercent } from "../stores/operations";
</script>

{#if $isRunning && $currentOperation}
  <div class="card preset-filled-surface-100-900 border border-surface-200-800 p-3">
    <div class="flex items-center justify-between mb-2">
      <span class="text-sm font-semibold capitalize">{$currentOperation}</span>
      <span class="text-sm opacity-60">{$progressPercent}%</span>
    </div>
    <div class="h-2 bg-surface-200-800 rounded-full overflow-hidden">
      <div
        class="h-full bg-primary-500 transition-all duration-100 ease-out"
        style="width: {$progressPercent}%"
      ></div>
    </div>
    {#if $progress}
      <div class="text-xs opacity-60 mt-1 text-right">
        {$progress.done.toLocaleString()} / {$progress.total.toLocaleString()} bytes
      </div>
    {/if}
  </div>
{/if}
