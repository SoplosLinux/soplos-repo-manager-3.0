<script lang="ts">
  import { onDestroy } from "svelte";
  import { _ } from "svelte-i18n";
  import { CheckCircle, X } from "lucide-svelte";

  // A green notice of something done. It goes away by itself after a few
  // seconds, or at once with its close button; a new message starts the count again.
  export let message: string | null;
  export let seconds = 5;

  let timer: ReturnType<typeof setTimeout> | undefined;
  $: {
    clearTimeout(timer);
    if (message) timer = setTimeout(() => (message = null), seconds * 1000);
  }
  onDestroy(() => clearTimeout(timer));
</script>

{#if message}
  <div class="flex items-center gap-2 px-4 py-3 rounded-lg bg-success/10 border border-success/30 text-success text-[0.82rem]">
    <CheckCircle size={16} class="shrink-0" />
    <span class="flex-1 min-w-0">{message}</span>
    <button
      type="button"
      class="shrink-0 grid place-items-center w-6 h-6 rounded text-success/80 hover:bg-success/15 hover:text-success"
      title={$_("jobs.close")}
      aria-label={$_("jobs.close")}
      on:click={() => (message = null)}
    >
      <X size={14} />
    </button>
  </div>
{/if}
