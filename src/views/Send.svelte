<script lang="ts">
  import { onMount, onDestroy, getContext } from "svelte";
  import { _ } from "svelte-i18n";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import {
    Send,
    FilePlus,
    FolderPlus,
    Trash2,
    CheckCircle,
    AlertTriangle,
    Package,
    UploadCloud,
    KeyRound,
    CircleUser,
    ExternalLink,
    X,
  } from "lucide-svelte";
  import { api } from "../lib/api";
  import { progress } from "../lib/progress";
  import { cancelJob } from "../lib/jobs";
  import { repos } from "../lib/repoStore";
  import { targets, loadTargets, selectedSendTarget } from "../lib/targetStore";
  import Select from "../components/Select.svelte";
  import Modal from "../components/Modal.svelte";

  const navigate = getContext<(id: string) => void>("navigate");

  let selectedTarget = $selectedSendTarget || "";
  let title = "";
  let description = "";
  let paths: string[] = [];
  let busy = false;
  let hasToken = true;
  let notice: string | null = null;
  let errorMessage: string | null = null;
  let successPr: number | null = null;

  async function checkAuth() {
    try {
      hasToken = await api.invoke<boolean>("has_github_token");
    } catch {
      hasToken = false;
    }
  }

  onMount(async () => {
    await checkAuth();
    const list = await loadTargets();
    if ($selectedSendTarget) {
      selectedTarget = $selectedSendTarget;
    } else if (list.length > 0 && !selectedTarget) {
      selectedTarget = list[0].id;
    }
  });

  $: if ($selectedSendTarget && $selectedSendTarget !== selectedTarget) {
    selectedTarget = $selectedSendTarget;
  }

  $: targetOptions = [
    ...$targets.map((t) => ({
      value: t.id,
      label: `${t.name} (${t.id})`,
    })),
    ...$repos
      .filter((r) => !$targets.some((t) => t.id === r.name))
      .map((r) => ({
        value: r.name,
        label: `${r.name} (${r.title || r.name})`,
      })),
  ];

  async function chooseFiles() {
    try {
      const picked = await api.invoke<string[]>("pick_deb_files");
      if (picked && picked.length > 0) {
        paths = Array.from(new Set([...paths, ...picked]));
      }
    } catch (err) {
      errorMessage = String(err);
    }
  }

  async function chooseFolder() {
    try {
      const folder = await api.invoke<string | null>("pick_folder");
      if (folder) {
        const found = await api.invoke<{ path: string }[]>("scan_packages", { dir: folder });
        const debPaths = found.map((f) => f.path);
        paths = Array.from(new Set([...paths, ...debPaths]));
      }
    } catch (err) {
      errorMessage = String(err);
    }
  }

  function removePath(pathToRemove: string) {
    paths = paths.filter((p) => p !== pathToRemove);
  }

  function clearAll() {
    paths = [];
  }

  let dragging = false;
  let unlistenDrop: (() => void) | undefined;

  async function handleDroppedPaths(dropped: string[]) {
    for (const item of dropped) {
      if (item.endsWith(".deb")) {
        paths = Array.from(new Set([...paths, item]));
      } else {
        try {
          const found = await api.invoke<{ path: string }[]>("scan_packages", { dir: item });
          const debPaths = found.map((f) => f.path);
          paths = Array.from(new Set([...paths, ...debPaths]));
        } catch { /* ignore */ }
      }
    }
  }

  onMount(async () => {
    try {
      unlistenDrop = await getCurrentWebview().onDragDropEvent((event) => {
        const kind = event.payload.type;
        if (kind === "enter" || kind === "over") dragging = true;
        else if (kind === "leave") dragging = false;
        else if (kind === "drop") {
          dragging = false;
          handleDroppedPaths(event.payload.paths);
        }
      });
    } catch { /* ignore */ }
  });

  onDestroy(() => unlistenDrop?.());

  $: resolvedTarget = selectedTarget;
  $: canSend = resolvedTarget.length > 0 && title.trim().length > 0 && paths.length > 0 && !busy && hasToken;

  async function handleSend() {
    if (!canSend) return;
    busy = true;
    errorMessage = null;
    notice = null;
    successPr = null;
    progress.set("busy");

    try {
      const prNumber = await api.invoke<number>("send_proposal", {
        target: resolvedTarget,
        title: title.trim(),
        description: description.trim(),
        paths,
        footer: $_("send.prFooter"),
      });
      successPr = Number(prNumber);
      notice = $_("send.success", { values: { number: prNumber } });
      paths = [];
      title = "";
      description = "";
    } catch (err) {
      errorMessage = String(err);
    } finally {
      busy = false;
      progress.set(null);
    }
  }

  function resetForm() {
    paths = [];
    title = "";
    description = "";
    errorMessage = null;
    notice = null;
  }

  const inputClass =
    "w-full bg-surface-2 border border-border rounded px-3 py-2 text-[0.85rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60";
  const button =
    "flex items-center gap-1.5 px-3 py-2 rounded border border-border text-text hover:bg-surface-2 text-[0.8rem] disabled:opacity-40 cursor-pointer transition-colors whitespace-nowrap shrink-0";
</script>

<div class="relative flex flex-col gap-5">
  {#if dragging}
    <div class="fixed inset-0 z-50 bg-dark/85 backdrop-blur-sm flex flex-col items-center justify-center gap-3 pointer-events-none">
      <UploadCloud size={54} class="text-primary animate-bounce" />
      <span class="text-text-bright font-semibold text-base">{$_("packages.dropHere", { values: { repo: selectedTarget } })}</span>
      <span class="text-text-muted text-xs">{$_("packages.dropHint")}</span>
    </div>
  {/if}

  <!-- Missing GitHub Token Banner -->
  {#if !hasToken}
    <div class="rounded-lg border border-warning/40 bg-warning/10 p-4 flex flex-col gap-3">
      <div class="flex items-start justify-between gap-3">
        <div class="flex items-start gap-2.5">
          <KeyRound size={18} class="text-warning shrink-0 mt-0.5" />
          <div class="flex flex-col gap-0.5">
            <span class="font-bold text-text-bright text-[0.85rem]">{$_("send.authRequiredTitle")}</span>
            <p class="text-[0.78rem] text-text-muted">{$_("send.authRequiredDesc")}</p>
          </div>
        </div>
        <div class="flex items-center gap-2 shrink-0">
          <button
            class="px-3 py-1.5 rounded bg-warning text-dark font-semibold text-[0.78rem] hover:bg-warning/90 transition-colors"
            on:click={() => navigate?.("accounts")}
          >
            <CircleUser size={13} class="inline mr-1" />
            {$_("send.goToAccounts")}
          </button>
        </div>
      </div>
    </div>
  {/if}

  <!-- Sticky toolbar -->
  <div class="sticky top-0 z-10 bg-dark pb-3 pt-1 -mx-7 px-7 border-b border-border flex items-center justify-between gap-2.5">
    <div class="min-w-[170px] max-w-[220px] flex-1">
      <Select bind:value={selectedTarget} label={$_("send.selectTarget")} options={targetOptions} />
    </div>

    <div class="flex items-center gap-1.5 shrink-0">
      <button class={button} disabled={busy} on:click={chooseFiles}>
        <FilePlus size={14} />
        <span>{$_("packages.addFiles")}</span>
      </button>
      <button class={button} disabled={busy} on:click={chooseFolder}>
        <FolderPlus size={14} />
        <span>{$_("packages.addFolder")}</span>
      </button>

      {#if busy}
        <button
          class="flex items-center gap-1 px-3 py-2 rounded bg-danger hover:bg-danger/85 text-text-bright text-[0.8rem] font-semibold cursor-pointer transition-colors shadow-sm whitespace-nowrap shrink-0"
          on:click={cancelJob}
        >
          <X size={14} />
          <span>{$_("common.cancel")}</span>
        </button>
      {:else}
        {#if paths.length > 0 || title.trim() || description.trim()}
          <button
            class="flex items-center gap-1 px-2.5 py-2 rounded border border-border bg-surface-2 hover:bg-surface-3 text-text-muted hover:text-text-bright text-[0.8rem] cursor-pointer transition-colors whitespace-nowrap shrink-0"
            title={$_("common.cancel")}
            on:click={resetForm}
          >
            <X size={14} />
            <span class="hidden md:inline">{$_("common.cancel")}</span>
          </button>
        {/if}
        <button
          class="flex items-center gap-1.5 px-3.5 py-2 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.8rem] font-semibold disabled:opacity-40 cursor-pointer transition-colors shadow-sm whitespace-nowrap shrink-0"
          disabled={!canSend}
          on:click={handleSend}
        >
          <Send size={14} />
          <span>{$_("send.submit")}</span>
        </button>
      {/if}
    </div>
  </div>

  {#if notice}
    <div class="flex items-center gap-2 px-4 py-3 rounded-lg bg-success/10 border border-success/30 text-success text-[0.82rem]">
      <CheckCircle size={16} class="shrink-0" />
      <span>{notice}</span>
    </div>
  {/if}

  {#if errorMessage}
    <div class="flex items-center gap-2 px-4 py-3 rounded-lg bg-danger/10 border border-danger/30 text-danger text-[0.82rem]">
      <AlertTriangle size={16} class="shrink-0" />
      <span class="break-all">{errorMessage}</span>
    </div>
  {/if}

  <!-- Proposal Metadata -->
  {#if targetOptions.length === 0}
    <div class="flex flex-col items-center justify-center gap-3 text-center py-14 border-2 border-dashed border-border rounded-xl bg-surface/40">
      <UploadCloud size={42} class="text-text-muted" />
      <div class="flex flex-col gap-1 max-w-[380px]">
        <span class="text-[0.9rem] font-semibold text-text-bright">{$_("send.noTargetsTitle")}</span>
        <p class="text-[0.78rem] text-text-muted leading-relaxed">{$_("send.noTargetsDesc")}</p>
      </div>
      <button
        class="flex items-center gap-1.5 px-3.5 py-1.5 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.8rem] font-semibold transition-colors"
        on:click={() => navigate?.("targets")}
      >
        {$_("send.goToTargets")}
      </button>
    </div>
  {:else}
  <div class="rounded-lg border border-border bg-surface p-4 flex flex-col gap-3">
    <span class="text-[0.82rem] font-semibold text-text-bright">{$_("send.detailsTitle")}</span>

    <label class="flex flex-col gap-1">
      <span class="text-[0.78rem] text-text-muted">{$_("send.titleLabel")} *</span>
      <input
        class={inputClass}
        bind:value={title}
        placeholder={$_("send.titlePlaceholder")}
        disabled={busy}
      />
    </label>

    <label class="flex flex-col gap-1">
      <span class="text-[0.78rem] text-text-muted">{$_("send.descriptionLabel")}</span>
      <textarea
        class="{inputClass} min-h-[80px] resize-y"
        bind:value={description}
        placeholder={$_("send.descriptionPlaceholder")}
        disabled={busy}
      ></textarea>
    </label>
  </div>

  <!-- Packages List -->
  <div class="rounded-lg border border-border bg-surface p-4 flex flex-col gap-3">
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-2">
        <Package size={16} class="text-primary" />
        <span class="text-[0.82rem] font-semibold text-text-bright">
          {$_("send.packagesTitle")} ({paths.length})
        </span>
      </div>
      {#if paths.length > 0}
        <button
          type="button"
          class="text-xs text-text-muted hover:text-danger cursor-pointer"
          disabled={busy}
          on:click={clearAll}
        >
          {$_("common.clear")}
        </button>
      {/if}
    </div>

    {#if paths.length === 0}
      <div class="flex flex-col items-center justify-center gap-2 text-center py-8 border-2 border-dashed border-border rounded-lg">
        <Package size={36} class="text-text-muted" />
        <p class="text-[0.8rem] text-text-muted max-w-[420px]">
          {$_("send.emptyPackages")}
        </p>
      </div>
    {:else}
      <div class="max-h-[300px] overflow-y-auto content-scroll flex flex-col gap-1.5 pr-1">
        {#each paths as p (p)}
          <div class="flex items-center justify-between gap-3 px-3 py-2 rounded bg-surface-2 border border-border text-[0.78rem]">
            <span class="font-mono text-text truncate flex-1 min-w-0" title={p}>
              {p.split("/").pop()}
            </span>
            <button
              type="button"
              class="text-text-muted hover:text-danger p-1 rounded cursor-pointer"
              disabled={busy}
              on:click={() => removePath(p)}
            >
              <Trash2 size={13} />
            </button>
          </div>
        {/each}
      </div>
    {/if}
  </div>
  {/if}
</div>
