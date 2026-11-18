<script lang="ts">
  import { onMount, onDestroy, getContext } from "svelte";
  import { _, locale } from "svelte-i18n";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import {
    FolderPlus,
    FilePlus,
    CheckCircle,
    AlertTriangle,
    XCircle,
    ChevronDown,
    PackagePlus,
    Eraser,
    ShieldCheck,
    Info,
    X,
  } from "lucide-svelte";
  import { api } from "../lib/api";
  import { progress } from "../lib/progress";
  import { cancelJob } from "../lib/jobs";
  import { activeRepo } from "../lib/repoStore";
  import { type DebCheck, type CheckLevel, formatSize, queuedPaths } from "../lib/packages";

  const navigate = getContext<(id: string) => void>("navigate");

  // Check only reads the packages: nothing is added or changed until a
  // package is sent to Packages, where its own review decides.
  let results: DebCheck[] = [];
  let busy = false;
  let errorMessage: string | null = null;
  let open = new Set<string>();
  let lintianOpen = new Set<string>();
  // lintian takes minutes on a large package and is only a second opinion: off unless asked for.
  let withLintian = false;

  $: size = (bytes: number) => formatSize(bytes, $locale ?? "en");
  $: counts = {
    ok: results.filter((r) => r.level === "ok").length,
    warning: results.filter((r) => r.level === "warning").length,
    error: results.filter((r) => r.level === "error").length,
  };
  $: sendable = results.filter((r) => r.level !== "error");

  async function check(paths: string[]) {
    if (busy || paths.length === 0) return;
    busy = true;
    errorMessage = null;
    progress.set("busy");
    try {
      const found = await api.invoke<DebCheck[]>("check_packages", { paths, lintian: withLintian });
      // A package checked again replaces its earlier result.
      const again = new Set(found.map((r) => r.path));
      results = [...results.filter((r) => !again.has(r.path)), ...found];
      // The packages with a problem open by themselves.
      for (const r of found) if (r.level !== "ok") open.add(r.path);
      open = open;
      if (found.length === 0) errorMessage = $_("check.noneFound");
    } catch (error) {
      errorMessage = String(error);
    } finally {
      busy = false;
      progress.set(null);
    }
  }

  async function chooseFiles() {
    await check(await api.invoke<string[]>("pick_deb_files"));
  }

  async function chooseFolder() {
    const path = await api.invoke<string | null>("pick_folder");
    if (path) await check([path]);
  }

  function toggle(set: Set<string>, path: string) {
    set.has(path) ? set.delete(path) : set.add(path);
    open = open;
    lintianOpen = lintianOpen;
  }

  // The packages without errors go to Packages, which shows its own review
  // (new, upgrade, same version...) before anything is added.
  function sendToRepository() {
    queuedPaths.set(sendable.map((r) => r.path));
    navigate?.("packages");
  }

  let dragging = false;
  let unlistenDrop: UnlistenFn | undefined;
  onMount(async () => {
    unlistenDrop = await getCurrentWebview().onDragDropEvent((event) => {
      const kind = event.payload.type;
      if (kind === "enter" || kind === "over") dragging = true;
      else if (kind === "leave") dragging = false;
      else if (kind === "drop") {
        dragging = false;
        check(event.payload.paths);
      }
    });
  });
  onDestroy(() => unlistenDrop?.());

  const tint: Record<CheckLevel, string> = {
    ok: "bg-success/15 text-success",
    warning: "bg-warning/15 text-warning",
    error: "bg-danger/15 text-danger",
  };
  const icon = { ok: CheckCircle, warning: AlertTriangle, error: XCircle, info: Info };
  const findingTint = { ok: "text-success", warning: "text-warning", error: "text-danger", info: "text-text-muted" };
  const button =
    "flex items-center gap-1.5 px-3 py-2 rounded border border-border text-text hover:bg-surface-2 text-[0.8rem] disabled:opacity-40";
</script>

<div class="relative">
  <div class="sticky top-0 z-10 bg-dark pb-3 pt-1 -mx-7 px-7 border-b border-border flex flex-wrap items-center gap-2 mb-4">
    <p class="flex-1 min-w-[15rem] text-[0.78rem] text-text-muted">{$_("check.intro")}</p>
    {#if results.length}
      <button class={button} disabled={busy} on:click={() => { results = []; open = new Set(); lintianOpen = new Set(); }}>
        <Eraser size={14} />
        {$_("check.clear")}
      </button>
    {/if}
    <button class={button} disabled={busy} on:click={chooseFiles}>
      <FilePlus size={14} />
      {$_("packages.addFiles")}
    </button>
    <button
      class="flex items-center gap-1.5 px-3 py-2 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.8rem] font-semibold disabled:opacity-40"
      disabled={busy}
      on:click={chooseFolder}
    >
      <FolderPlus size={14} />
      {$_("packages.addFolder")}
    </button>
    {#if busy}
      <button
        class="flex items-center gap-1.5 px-3 py-2 rounded bg-danger hover:bg-danger/85 text-text-bright text-[0.8rem] font-semibold"
        on:click={cancelJob}
      >
        <X size={14} />
        {$_("common.cancel")}
      </button>
    {/if}
    <label class="basis-full flex items-center gap-2 text-[0.76rem] text-text-muted cursor-pointer select-none">
      <input type="checkbox" class="w-4 h-4 accent-primary" bind:checked={withLintian} disabled={busy} />
      {$_("check.withLintian")}
    </label>
  </div>

  {#if errorMessage}
    <p class="mb-3 rounded-md border border-danger/30 bg-danger/10 p-3 text-[0.8rem] text-danger">{errorMessage}</p>
  {/if}

  {#if busy}
    <p class="mb-3 text-[0.8rem] text-text-muted">{$_("check.working")}</p>
  {/if}

  {#if results.length === 0 && !busy}
    <div class="flex flex-col items-center justify-center gap-3 text-center py-14 border-2 border-dashed rounded-xl {dragging ? 'border-primary bg-primary/5' : 'border-border bg-surface/40'}">
      <ShieldCheck size={44} class="text-text-muted" />
      <span class="text-[0.9rem] font-semibold text-text-bright">{$_("check.emptyTitle")}</span>
      <p class="text-[0.78rem] text-text-muted max-w-[440px]">{$_("check.emptyDesc")}</p>
    </div>
  {:else if results.length}
    <div class="flex flex-wrap items-center gap-2 mb-3 text-[0.78rem]">
      <span class="text-text-muted">{$_("check.summary", { values: { count: results.length } })}</span>
      <span class="px-2 py-0.5 rounded-full font-semibold {tint.ok}">{$_("check.levels.ok")}: {counts.ok}</span>
      <span class="px-2 py-0.5 rounded-full font-semibold {tint.warning}">{$_("check.levels.warning")}: {counts.warning}</span>
      <span class="px-2 py-0.5 rounded-full font-semibold {tint.error}">{$_("check.levels.error")}: {counts.error}</span>
      <button
        class="ml-auto flex items-center gap-1.5 px-3 py-1.5 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.78rem] font-semibold disabled:opacity-40 disabled:pointer-events-none"
        disabled={busy || sendable.length === 0 || !$activeRepo}
        title={$activeRepo ? "" : $_("check.noRepo")}
        on:click={sendToRepository}
      >
        <PackagePlus size={14} />
        {$_("check.sendToRepo", { values: { count: sendable.length, repo: $activeRepo ?? "" } })}
      </button>
    </div>

    <div class="flex flex-col gap-2.5">
      {#each results as r (r.path)}
        <div class="rounded-lg border bg-surface p-4 flex flex-col gap-2.5 {r.level === 'error' ? 'border-danger/40' : r.level === 'warning' ? 'border-warning/40' : 'border-border'}">
          <div class="flex items-start gap-3">
            <svelte:component this={icon[r.level]} size={20} class="shrink-0 mt-0.5 {r.level === 'ok' ? 'text-success' : r.level === 'warning' ? 'text-warning' : 'text-danger'}" />
            <div class="min-w-0 flex-1">
              <div class="flex flex-wrap items-baseline gap-x-2">
                <span class="text-text-bright font-semibold text-[0.9rem] break-all">{r.name || r.file}</span>
                {#if r.version}<span class="font-mono text-text text-[0.78rem]">{r.version}</span>{/if}
                {#if r.architecture}<span class="text-text-muted text-[0.76rem]">{r.architecture}</span>{/if}
                <span class="text-text-muted text-[0.76rem]">{size(r.size)}</span>
              </div>
              <p class="text-[0.72rem] text-text-muted break-all">{r.group ? `${r.group} · ` : ""}{r.file}</p>
            </div>
            <span class="shrink-0 px-2 py-0.5 rounded-full text-[0.7rem] font-semibold {tint[r.level]}">{$_(`check.levels.${r.level}`)}</span>
            <button
              type="button"
              class="shrink-0 w-7 h-7 grid place-items-center rounded text-text-muted hover:bg-surface-2 hover:text-text-bright"
              title={open.has(r.path) ? $_("common.collapse") : $_("common.expand")}
              aria-label={open.has(r.path) ? $_("common.collapse") : $_("common.expand")}
              aria-expanded={open.has(r.path)}
              on:click={() => toggle(open, r.path)}
            >
              <ChevronDown size={16} class="transition-transform {open.has(r.path) ? 'rotate-180' : ''}" />
            </button>
          </div>

          {#if open.has(r.path)}
            <ul class="flex flex-col gap-1 pl-8">
              {#each r.findings as f}
                <li class="flex items-start gap-2 text-[0.78rem]">
                  <svelte:component this={icon[f.level]} size={14} class="shrink-0 mt-0.5 {findingTint[f.level]}" />
                  <span class="text-text">{$_(`check.items.${f.code}`, { values: f.values })}</span>
                </li>
              {/each}
            </ul>
            {#if r.depends}
              <p class="pl-8 text-[0.74rem] text-text-muted break-words"><span class="text-text">{$_("check.depends")}:</span> {r.depends}</p>
            {/if}
            {#if r.lintian && r.lintian.length}
              <div class="pl-8">
                <button class="text-[0.75rem] font-semibold text-primary hover:underline" on:click={() => toggle(lintianOpen, r.path)}>
                  {lintianOpen.has(r.path) ? $_("check.hideLintian") : $_("check.showLintian", { values: { count: r.lintian.length } })}
                </button>
                {#if lintianOpen.has(r.path)}
                  <pre class="mt-1.5 max-h-64 overflow-auto rounded bg-surface-2 border border-border p-2.5 text-[0.72rem] text-text font-mono whitespace-pre-wrap select-text">{r.lintian.join("\n")}</pre>
                {/if}
              </div>
            {/if}
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>
