<script lang="ts">
  import { onMount } from "svelte";
  import { _, locale } from "svelte-i18n";
  import { Eraser, HardDrive, GitBranch, AlertTriangle, CheckCircle, RefreshCw, Sparkles } from "lucide-svelte";
  import { api } from "../lib/api";
  import Notice from "../components/Notice.svelte";
  import { progress } from "../lib/progress";
  import { activeRepo, repos } from "../lib/repoStore";
  import { formatSize } from "../lib/packages";
  import Modal from "../components/Modal.svelte";
  import Select from "../components/Select.svelte";

  interface CleanRepoStats {
    repo: string;
    poolSize: number;
    gitSize: number;
    totalSize: number;
    commitCount: number;
  }

  let selectedRepo = $activeRepo || "";
  let stats: CleanRepoStats | null = null;
  let loading = false;
  let busy = false;
  let errorMessage: string | null = null;
  let notice: string | null = null;
  let confirmNormalOpen = false;
  let confirmDeepOpen = false;

  $: size = (bytes: number) => formatSize(bytes, $locale ?? "en");

  $: repoOptions = $repos.map((r) => ({ value: r.name, label: `${r.name} (${r.title || r.name})` }));

  $: if ($activeRepo && !selectedRepo) {
    selectedRepo = $activeRepo;
  }

  let lastLoadedRepo = "";
  $: if (selectedRepo && selectedRepo !== lastLoadedRepo) {
    lastLoadedRepo = selectedRepo;
    loadStats(selectedRepo);
  }

  async function loadStats(name: string) {
    if (!name) return;
    loading = true;
    errorMessage = null;
    try {
      stats = await api.invoke<CleanRepoStats>("get_cleanup_stats", { name });
    } catch (err) {
      stats = null;
      errorMessage = String(err);
    } finally {
      loading = false;
    }
  }

  async function runNormalCleanup() {
    confirmNormalOpen = false;
    if (!selectedRepo) return;
    busy = true;
    notice = null;
    errorMessage = null;
    progress.set("busy");
    try {
      await api.invoke("clean_repo_normal", { name: selectedRepo });
      notice = $_("cleanup.success");
      await loadStats(selectedRepo);
    } catch (err) {
      errorMessage = String(err);
    } finally {
      busy = false;
      progress.set(null);
    }
  }

  async function runDeepCleanup() {
    confirmDeepOpen = false;
    if (!selectedRepo) return;
    busy = true;
    notice = null;
    errorMessage = null;
    progress.set("busy");
    try {
      await api.invoke("clean_repo_history", { name: selectedRepo });
      notice = $_("cleanup.success");
      await loadStats(selectedRepo);
    } catch (err) {
      errorMessage = String(err);
    } finally {
      busy = false;
      progress.set(null);
    }
  }

  onMount(() => {
    if (selectedRepo) loadStats(selectedRepo);
  });
</script>

<div class="space-y-6">
  <p class="text-[0.85rem] text-text-muted leading-relaxed">
    {$_("cleanup.intro")}
  </p>

  <Notice bind:message={notice} />

  {#if errorMessage}
    <div class="flex items-center gap-2 p-3 rounded bg-danger/10 border border-danger/30 text-danger text-[0.85rem]">
      <AlertTriangle class="w-4 h-4 shrink-0" />
      <span>{errorMessage}</span>
    </div>
  {/if}

  <div class="bg-surface-1 border border-border rounded-lg p-5 space-y-4">
    <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
      <div>
        <label for="repo-select" class="block text-xs font-semibold uppercase tracking-wider text-text-muted mb-1.5">
          {$_("cleanup.selectRepo")}
        </label>
        <div class="w-72">
          <Select
            options={repoOptions}
            bind:value={selectedRepo}
            on:change={(e) => {
              selectedRepo = e.detail;
              loadStats(selectedRepo);
            }}
          />
        </div>
      </div>

      <button
        type="button"
        class="self-start sm:self-end flex items-center gap-1.5 px-3 py-2 rounded border border-border text-text hover:bg-surface-2 text-[0.8rem] disabled:opacity-40 transition-colors"
        disabled={loading || busy || !selectedRepo}
        on:click={() => loadStats(selectedRepo)}
      >
        <RefreshCw class="w-3.5 h-3.5 {loading ? 'animate-spin' : ''}" />
        <span>{$_("cleanup.refresh")}</span>
      </button>
    </div>

    {#if loading}
      <p class="text-[0.85rem] text-text-muted py-4">{$_("cleanup.readingUsage")}</p>
    {:else if stats}
      <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3 pt-2">
        <div class="bg-surface-2/60 border border-border/60 rounded p-3">
          <div class="flex items-center gap-2 text-text-muted text-xs mb-1">
            <HardDrive class="w-3.5 h-3.5" />
            <span>{$_("cleanup.poolSize")}</span>
          </div>
          <p class="text-base font-semibold text-text-bright">{size(stats.poolSize)}</p>
        </div>

        <div class="bg-surface-2/60 border border-border/60 rounded p-3">
          <div class="flex items-center gap-2 text-text-muted text-xs mb-1">
            <GitBranch class="w-3.5 h-3.5" />
            <span>{$_("cleanup.gitSize")}</span>
          </div>
          <p class="text-base font-semibold text-text-bright">{size(stats.gitSize)}</p>
        </div>

        <div class="bg-surface-2/60 border border-border/60 rounded p-3">
          <div class="flex items-center gap-2 text-text-muted text-xs mb-1">
            <HardDrive class="w-3.5 h-3.5" />
            <span>{$_("cleanup.totalSize")}</span>
          </div>
          <p class="text-base font-semibold text-text-bright">{size(stats.totalSize)}</p>
        </div>

        <div class="bg-surface-2/60 border border-border/60 rounded p-3">
          <div class="flex items-center gap-2 text-text-muted text-xs mb-1">
            <GitBranch class="w-3.5 h-3.5" />
            <span>{$_("cleanup.commits")}</span>
          </div>
          <p class="text-base font-semibold text-text-bright">{stats.commitCount}</p>
        </div>
      </div>
    {/if}
  </div>

  {#if busy}
    <div class="flex items-center gap-2 text-[0.85rem] text-primary p-3 rounded bg-primary/10 border border-primary/20">
      <RefreshCw class="w-4 h-4 animate-spin shrink-0" />
      <span>{$_("cleanup.cleaning")}</span>
    </div>
  {/if}

  <!-- Opción 1: Limpieza normal (optimización sin perder historial) -->
  <div class="bg-surface-1 border border-border rounded-lg p-5 space-y-4">
    <div class="flex items-start gap-3">
      <div class="p-2 rounded-md bg-info/10 text-info shrink-0 mt-0.5">
        <Sparkles class="w-5 h-5" />
      </div>
      <div class="space-y-1">
        <h3 class="text-sm font-semibold text-text-bright">{$_("cleanup.normalClean")}</h3>
        <p class="text-[0.82rem] text-text-muted leading-relaxed">
          {$_("cleanup.normalCleanDesc")}
        </p>
      </div>
    </div>

    <div class="pt-2 flex justify-end">
      <button
        type="button"
        class="flex items-center gap-2 px-4 py-2 rounded border border-border bg-surface-2 hover:bg-surface-3 text-text-bright text-[0.85rem] font-medium disabled:opacity-40 transition-colors shadow-sm"
        disabled={busy || loading || !selectedRepo}
        on:click={() => (confirmNormalOpen = true)}
      >
        <Sparkles class="w-4 h-4" />
        <span>{$_("cleanup.startNormalClean")}</span>
      </button>
    </div>
  </div>

  <!-- Opción 2: Limpieza profunda del historial (orphan squashing) -->
  <div class="bg-surface-1 border border-border rounded-lg p-5 space-y-4">
    <div class="flex items-start gap-3">
      <div class="p-2 rounded-md bg-warning/10 text-warning shrink-0 mt-0.5">
        <Eraser class="w-5 h-5" />
      </div>
      <div class="space-y-1">
        <h3 class="text-sm font-semibold text-text-bright">{$_("cleanup.deepClean")}</h3>
        <p class="text-[0.82rem] text-text-muted leading-relaxed">
          {$_("cleanup.deepCleanDesc")}
        </p>
      </div>
    </div>

    <div class="pt-2 flex justify-end">
      <button
        type="button"
        class="flex items-center gap-2 px-4 py-2 rounded bg-primary text-primary-foreground hover:bg-primary/90 text-[0.85rem] font-medium disabled:opacity-40 transition-colors shadow-sm"
        disabled={busy || loading || !selectedRepo}
        on:click={() => (confirmDeepOpen = true)}
      >
        <Eraser class="w-4 h-4" />
        <span>{$_("cleanup.startClean")}</span>
      </button>
    </div>
  </div>
</div>

{#if confirmNormalOpen}
  <Modal
    title={$_("cleanup.confirmNormalTitle", { values: { repo: selectedRepo } })}
    onClose={() => (confirmNormalOpen = false)}
  >
    <div class="space-y-4">
      <p class="text-[0.85rem] text-text leading-relaxed">
        {$_("cleanup.confirmNormalText")}
      </p>

      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          class="px-3 py-1.5 rounded border border-border text-text hover:bg-surface-2 text-[0.85rem]"
          on:click={() => (confirmNormalOpen = false)}
        >
          {$_("common.cancel")}
        </button>
        <button
          type="button"
          class="px-4 py-1.5 rounded bg-primary text-primary-foreground hover:bg-primary/90 text-[0.85rem] font-medium"
          on:click={runNormalCleanup}
        >
          {$_("cleanup.startNormalClean")}
        </button>
      </div>
    </div>
  </Modal>
{/if}

{#if confirmDeepOpen}
  <Modal
    title={$_("cleanup.confirmTitle", { values: { repo: selectedRepo } })}
    onClose={() => (confirmDeepOpen = false)}
  >
    <div class="space-y-4">
      <p class="text-[0.85rem] text-text leading-relaxed">
        {$_("cleanup.confirmText")}
      </p>

      <div class="flex justify-end gap-2 pt-2">
        <button
          type="button"
          class="px-3 py-1.5 rounded border border-border text-text hover:bg-surface-2 text-[0.85rem]"
          on:click={() => (confirmDeepOpen = false)}
        >
          {$_("common.cancel")}
        </button>
        <button
          type="button"
          class="px-4 py-1.5 rounded bg-danger text-white hover:bg-danger/90 text-[0.85rem] font-medium"
          on:click={runDeepCleanup}
        >
          {$_("cleanup.startClean")}
        </button>
      </div>
    </div>
  </Modal>
{/if}
