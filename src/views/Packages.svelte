<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { _, locale } from "svelte-i18n";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import {
    Package,
    FolderPlus,
    FilePlus,
    Trash2,
    Download,
    ChevronUp,
    ChevronDown,
    CloudUpload,
    X,
  } from "lucide-svelte";
  import { api } from "../lib/api";
  import { progress } from "../lib/progress";
  import { cancelJob } from "../lib/jobs";
  import { activeRepo, repos } from "../lib/repoStore";
  import {
    type PackageList,
    type RepoPackage,
    type Candidate,
    type RepoResult,
    type AddProgress,
    type PublishState,
    addable,
    formatSize,
    queuedPaths,
  } from "../lib/packages";
  import Modal from "../components/Modal.svelte";
  import SearchInput from "../components/SearchInput.svelte";

  let list: PackageList | null = null;
  let loading = true;
  let busy = false;
  let errorMessage: string | null = null;
  let filter = "";

  async function load() {
    loading = true;
    try {
      list = await api.invoke<PackageList>("list_packages");
    } catch (error) {
      list = null;
      errorMessage = String(error);
    } finally {
      loading = false;
    }
  }

  // The page follows the repository chosen in the sidebar.
  $: if ($activeRepo !== undefined) load();

  $: size = (bytes: number) => formatSize(bytes, $locale ?? "en");

  // The list is sorted by one column; versions compare their numbers as numbers.
  type SortColumn = "name" | "version" | "architecture" | "size";
  const sortColumns: { id: SortColumn }[] = [
    { id: "name" },
    { id: "version" },
    { id: "architecture" },
    { id: "size" },
  ];
  let sortBy: SortColumn = "name";
  let ascending = true;

  function sortOn(column: SortColumn) {
    if (sortBy === column) ascending = !ascending;
    else {
      sortBy = column;
      ascending = column !== "size";
    }
  }

  const textOrder = new Intl.Collator(undefined, {
    numeric: true,
    sensitivity: "base",
  });
  $: sorted = [...shown].sort((a, b) => {
    const order =
      sortBy === "size"
        ? a.size - b.size
        : textOrder.compare(String(a[sortBy]), String(b[sortBy])) ||
          textOrder.compare(a.name, b.name);
    return ascending ? order : -order;
  });
  const columns =
    "grid grid-cols-[16px_minmax(0,1fr)_minmax(90px,150px)_70px_70px_32px] items-center gap-3";
  $: shown = (list?.packages ?? []).filter((p) => {
    const f = filter.trim().toLowerCase();
    return (
      !f ||
      p.name.toLowerCase().includes(f) ||
      p.summary.toLowerCase().includes(f)
    );
  });

  // What is dropped on the window, or chosen, is looked at before anything is added.
  let candidates: Candidate[] | null = null;
  let picked = new Set<string>();

  async function review(paths: string[]) {
    if (busy || paths.length === 0) return;
    busy = true;
    progress.set("busy");
    try {
      const found = await api.invoke<Candidate[]>("scan_packages", { paths });
      targets = $activeRepo ? [$activeRepo] : [];
      await loadStates();
      candidates = found;
      picked = new Set(found.filter(addable).map((c) => c.path));
    } catch (error) {
      errorMessage = String(error);
    } finally {
      busy = false;
      progress.set(null);
    }
  }

  // Packages sent from Check open the review as soon as the list is read.
  $: if ($queuedPaths.length && list && !busy) {
    const paths = $queuedPaths;
    queuedPaths.set([]);
    review(paths);
  }

  async function chooseFolder() {
    const path = await api.invoke<string | null>("pick_folder");
    if (path) await review([path]);
  }

  async function chooseFiles() {
    const paths = await api.invoke<string[]>("pick_deb_files");
    await review(paths);
  }

  function toggle(c: Candidate, on: boolean) {
    if (on) picked.add(c.path);
    else picked.delete(c.path);
    picked = picked;
  }

  $: groups = (candidates ?? []).reduce<Record<string, Candidate[]>>(
    (all, c) => {
      (all[c.group] ??= []).push(c);
      return all;
    },
    {},
  );
  $: chosen = (candidates ?? []).filter((c) => picked.has(c.path));
  $: chosenSize = chosen.reduce((sum, c) => sum + c.size, 0);
  $: skipped = (candidates ?? []).filter((c) => !addable(c)).length;

  function toggleGroup(items: Candidate[], on: boolean) {
    for (const c of items.filter(addable)) {
      if (on) picked.add(c.path);
      else picked.delete(c.path);
    }
    picked = picked;
  }

  // The repositories the packages go to: the one in use, and any other one chosen
  // (an application of Soplos goes to boro, tyron and tyson at once).
  let targets: string[] = [];
  let states: PublishState[] = [];

  async function loadStates() {
    try {
      states = await api.invoke<PublishState[]>("publish_states", {
        names: $repos.map((r) => r.name),
      });
    } catch {
      states = [];
    }
  }

  function toggleTarget(name: string, on: boolean) {
    targets = on ? [...targets, name] : targets.filter((t) => t !== name);
  }

  $: notConfigured = states
    .filter((s) => !s.configured && targets.includes(s.name))
    .map((s) => s.name);
  $: canAdd = chosen.length > 0 && targets.length > 0;

  // Adding: the bar fills as each package goes in, then slides while each
  // repository is prepared for the software centers, signed and published.
  let step: AddProgress | null = null;
  let results: RepoResult[] | null = null;

  function follow(e: { payload: AddProgress }) {
    step = e.payload;
    const p = e.payload;
    progress.set(
      p.stage === "adding" && p.total > 0 ? p.done / p.total : "busy",
    );
  }

  async function add() {
    const paths = chosen.map((c) => c.path);
    const to = [...targets];
    candidates = null;
    if (paths.length === 0 || to.length === 0) return;
    busy = true;
    progress.set(0);
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<AddProgress>("packages-progress", follow);
      results = await api.invoke<RepoResult[]>("add_packages", {
        paths,
        repos: to,
      });
    } catch (error) {
      errorMessage = String(error);
    } finally {
      unlisten?.();
      busy = false;
      step = null;
      progress.set(null);
      await load();
    }
  }

  // Publish again without adding anything: the data for the software centers,
  // the signature and the push.
  let showRepublishConfirm = false;

  function askRepublish() {
    if (!list) return;
    showRepublishConfirm = true;
  }

  async function confirmRepublish() {
    showRepublishConfirm = false;
    if (!list) return;
    await republish(list.repo);
  }

  async function republish(name: string) {
    busy = true;
    progress.set("busy");
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<AddProgress>("packages-progress", follow);
      results = [await api.invoke<RepoResult>("republish", { name })];
    } catch (error) {
      errorMessage = String(error);
    } finally {
      unlisten?.();
      busy = false;
      step = null;
      progress.set(null);
      await load();
    }
  }

  // Remove
  let removeTarget: RepoPackage | null = null;

  async function remove() {
    const target = removeTarget;
    removeTarget = null;
    if (!target) return;
    busy = true;
    progress.set("busy");
    let unlisten: UnlistenFn | undefined;
    try {
      unlisten = await listen<AddProgress>("packages-progress", follow);
      results = [
        await api.invoke<RepoResult>("remove_package", { name: target.name }),
      ];
    } catch (error) {
      errorMessage = String(error);
    } finally {
      unlisten?.();
      busy = false;
      step = null;
      progress.set(null);
      await load();
    }
  }

  // Files and folders dragged from the file manager onto the window.
  let dragging = false;
  let unlistenDrop: UnlistenFn | undefined;

  onMount(async () => {
    unlistenDrop = await getCurrentWebview().onDragDropEvent((event) => {
      const kind = event.payload.type;
      if (kind === "enter" || kind === "over") dragging = true;
      else if (kind === "leave") dragging = false;
      else if (kind === "drop") {
        dragging = false;
        review(event.payload.paths);
      }
    });
  });

  onDestroy(() => unlistenDrop?.());

  const statusTint: Record<string, string> = {
    new: "bg-success/15 text-success",
    upgrade: "bg-primary/15 text-primary",
    same: "bg-surface-2 text-text-muted",
    older: "bg-warning/15 text-warning",
    duplicate: "bg-surface-2 text-text-muted",
    wrongArch: "bg-danger/15 text-danger",
    noSection: "bg-danger/15 text-danger",
    invalid: "bg-danger/15 text-danger",
  };
  const button =
    "flex items-center gap-1.5 px-3 py-2 rounded border border-border text-text hover:bg-surface-2 text-[0.8rem] disabled:opacity-40";
</script>

<div class="relative">
  <!-- The search keeps room for its whole text in every language; on a narrow window the buttons go to a second row. -->
  <div class="sticky top-0 z-10 bg-dark pb-3 pt-1 -mx-7 px-7 border-b border-border flex flex-wrap items-center gap-2 mb-4">
    <div class="flex-1 min-w-[15.5rem]">
      <SearchInput bind:value={filter} placeholder={$_("packages.search")} />
    </div>
    <button
      class={button}
      disabled={busy || !list}
      on:click={askRepublish}
      title={$_("packages.republishHint")}
    >
      <CloudUpload size={14} />
      {$_("packages.republish")}
    </button>
    <button class={button} disabled={busy || !list} on:click={chooseFiles}>
      <FilePlus size={14} />
      {$_("packages.addFiles")}
    </button>
    <button
      class="flex items-center gap-1.5 px-3 py-2 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.8rem] font-semibold disabled:opacity-40"
      disabled={busy || !list}
      on:click={chooseFolder}
    >
      <FolderPlus size={14} />
      {$_("packages.addFolder")}
    </button>
    {#if busy}
      <button
        class="flex items-center gap-1.5 px-3 py-2 rounded bg-danger hover:bg-danger/85 text-text-bright text-[0.8rem] font-semibold cursor-pointer transition-colors shadow-sm"
        on:click={cancelJob}
      >
        <X size={14} />
        {$_("common.cancel")}
      </button>
    {/if}
  </div>

  {#if step}
    <p class="mb-3 text-[0.8rem] text-text-muted">
      {$_(`packages.stage.${step.stage}`, {
        values: {
          repo: step.repo,
          file: step.file,
          n: step.done + 1,
          total: step.total,
        },
      })}
    </p>
  {/if}

  {#if loading}
    <p class="text-text-muted text-[0.85rem]">{$_("packages.loading")}</p>
  {:else if !list}
    <p class="text-text-muted text-[0.85rem]">{$_("packages.noRepo")}</p>
  {:else}
    <p class="mb-2 text-[0.78rem] text-text-muted">
      {$_("packages.summary", {
        values: { count: list.packages.length, repo: list.repo },
      })}
    </p>
    {#if list.packages.length === 0}
      <div
        class="flex flex-col items-center justify-center gap-2 text-center py-10 border-2 border-dashed border-border rounded-lg"
      >
        <Download size={48} class="text-text-muted" />
        <p class="text-[0.85rem] text-text-muted max-w-[420px]">
          {$_("packages.empty")}
        </p>
      </div>
    {:else}
      <div class="rounded-lg border border-border bg-surface">
        <!-- A click on a heading sorts by that column; a second click turns the order around. -->
        <div
          class="{columns} px-3.5 py-2 border-b border-border text-[0.72rem] font-semibold text-text-muted"
        >
          <span></span>
          {#each sortColumns as column}
            <button
              class="flex items-center gap-1 text-left hover:text-text-bright {sortBy ===
              column.id
                ? 'text-primary'
                : ''} {column.id === 'size' ? 'justify-end' : ''}"
              on:click={() => sortOn(column.id)}
            >
              {$_(`packages.columns.${column.id}`)}
              {#if sortBy === column.id}
                <svelte:component
                  this={ascending ? ChevronUp : ChevronDown}
                  size={12}
                />
              {/if}
            </button>
          {/each}
          <span></span>
        </div>
        <div class="divide-y divide-border">
          {#each sorted as p (p.name + p.architecture)}
            <div class="{columns} px-3.5 py-2">
              <Package size={16} class="text-primary" />
              <div class="min-w-0">
                <span
                  class="block text-text-bright font-semibold text-[0.84rem] truncate"
                  >{p.name}</span
                >
                {#if p.summary}<span
                    class="block text-text-muted text-[0.74rem] truncate"
                    >{p.summary}</span
                  >{/if}
              </div>
              <span class="font-mono text-text text-[0.76rem] truncate"
                >{p.version}</span
              >
              <span class="text-text-muted text-[0.74rem]"
                >{p.architecture}</span
              >
              <span class="text-text-muted text-[0.74rem] text-right"
                >{size(p.size)}</span
              >
              <button
                class="p-1.5 rounded text-text-muted hover:text-danger hover:bg-surface-2 disabled:opacity-40 justify-self-end"
                title={$_("packages.remove")}
                aria-label={$_("packages.remove")}
                disabled={busy}
                on:click={() => (removeTarget = p)}
              >
                <Trash2 size={14} />
              </button>
            </div>
          {/each}
        </div>
      </div>
      <p class="mt-2 text-[0.74rem] text-text-muted">
        {$_("packages.dropHint")}
      </p>
    {/if}
  {/if}
</div>

{#if dragging && list}
  <div
    class="fixed inset-0 z-40 grid place-items-center bg-black/50 pointer-events-none"
  >
    <div
      class="px-10 py-8 rounded-xl border-2 border-dashed border-primary bg-surface text-center"
    >
      <FolderPlus size={40} class="text-primary mx-auto mb-2" />
      <p class="text-text-bright font-semibold text-[0.95rem]">
        {$_("packages.dropHere", { values: { repo: list.repo } })}
      </p>
    </div>
  </div>
{/if}

{#if showRepublishConfirm && list}
  <Modal
    variant="confirm"
    title={$_("packages.republish")}
    confirmLabel={$_("packages.republish")}
    onClose={() => (showRepublishConfirm = false)}
    onConfirm={confirmRepublish}
  >
    <p class="text-[0.82rem] text-text-muted leading-relaxed">
      {$_("packages.republishConfirmText", { values: { repo: list.repo } })}
    </p>
  </Modal>
{/if}

{#if candidates}
  <Modal
    variant="confirm"
    showIcon={false}
    title={$_("packages.reviewTitle", { values: { repo: list?.repo ?? "" } })}
    confirmLabel={$_("packages.addPublish", {
      values: { count: chosen.length },
    })}
    confirmDisabled={!canAdd}
    maxWidth={760}
    onClose={() => (candidates = null)}
    onConfirm={add}
  >
    {#if candidates.length === 0}
      <p>{$_("packages.noneFound")}</p>
    {:else}
      <p class="mb-3 text-[0.8rem] text-text-muted">
        {$_("packages.reviewSummary", {
          values: { count: chosen.length, size: size(chosenSize), skipped },
        })}
      </p>
      <div
        class="max-h-[52vh] overflow-y-auto content-scroll flex flex-col gap-3 pr-1"
      >
        {#each Object.entries(groups) as [group, items] (group)}
          {@const choosable = items.filter(addable)}
          <div class="rounded-lg border border-border">
            <div
              class="flex items-center justify-between gap-2 px-3 py-2 bg-surface-2/60 rounded-t-lg"
            >
              <span class="font-mono text-[0.78rem] text-text-bright"
                >{group}</span
              >
              {#if choosable.length}
                <label
                  class="flex items-center gap-1.5 text-[0.74rem] text-text-muted"
                >
                  <input
                    type="checkbox"
                    checked={choosable.every((c) => picked.has(c.path))}
                    on:change={(e) =>
                      toggleGroup(items, e.currentTarget.checked)}
                  />
                  {$_("packages.all", { values: { count: items.length } })}
                </label>
              {:else}
                <span class="text-[0.74rem] text-text-muted"
                  >{$_("packages.all", {
                    values: { count: items.length },
                  })}</span
                >
              {/if}
            </div>
            {#each items as c (c.path)}
              <label
                class="flex items-center gap-2.5 px-3 py-1.5 border-t border-border {addable(
                  c,
                )
                  ? 'cursor-pointer'
                  : 'opacity-70'}"
              >
                <input
                  type="checkbox"
                  disabled={!addable(c)}
                  checked={picked.has(c.path)}
                  on:change={(e) => toggle(c, e.currentTarget.checked)}
                />
                <span class="min-w-0 flex-1">
                  <span class="block text-[0.8rem] text-text-bright truncate"
                    >{c.name || c.file}</span
                  >
                  <span
                    class="block font-mono text-[0.72rem] text-text-muted truncate"
                  >
                    {#if c.status === "upgrade" || c.status === "older" || c.status === "same"}
                      {c.current} → {c.version}
                    {:else}
                      {c.version}
                    {/if}
                    {#if c.architecture}
                      · {c.architecture}{/if}
                  </span>
                </span>
                <span class="text-[0.72rem] text-text-muted shrink-0"
                  >{size(c.size)}</span
                >
                <span
                  class="px-2 py-0.5 rounded-full text-[0.68rem] font-semibold shrink-0 {statusTint[
                    c.status
                  ]}"
                >
                  {$_(`packages.status.${c.status}`)}
                </span>
              </label>
            {/each}
          </div>
        {/each}
      </div>

      <!-- Where they go: the repository in use, and any other. -->
      <div class="mt-4 pt-3 border-t border-border flex flex-col gap-2">
        <span class="text-[0.78rem] text-text-muted"
          >{$_("packages.publishTo")}</span
        >
        <div class="flex flex-wrap gap-x-4 gap-y-1.5">
          {#each $repos as r (r.name)}
            <label class="flex items-center gap-1.5 text-[0.82rem] text-text">
              <input
                type="checkbox"
                checked={targets.includes(r.name)}
                on:change={(e) => toggleTarget(r.name, e.currentTarget.checked)}
              />
              {r.name}
            </label>
          {/each}
        </div>
        {#if targets.length > 1}
          <p class="text-[0.74rem] text-text-muted">
            {$_("packages.otherRepos", { values: { repo: list?.repo ?? "" } })}
          </p>
        {/if}
        {#if notConfigured.length}
          <p class="text-[0.74rem] text-warning">
            {$_("packages.noGit", {
              values: { repos: notConfigured.join(", ") },
            })}
          </p>
        {/if}
      </div>
    {/if}
  </Modal>
{/if}

{#if results}
  <Modal
    variant="info"
    title={$_("packages.done")}
    maxWidth={560}
    onClose={() => (results = null)}
  >
    <div
      class="max-h-[50vh] overflow-y-auto content-scroll flex flex-col gap-3"
    >
      {#each results as r (r.repo)}
        <div class="flex flex-col gap-1">
          <span class="text-text-bright font-semibold text-[0.85rem]"
            >{r.repo}</span
          >
          {#if r.error}
            <p class="text-danger text-[0.78rem] whitespace-pre-wrap">
              {r.error}
            </p>
          {:else}
            <p class="text-[0.78rem]">
              {$_("packages.resultLine", {
                values: { added: r.added, skipped: r.skipped },
              })}
            </p>
            {#if r.published}
              <p class="text-success text-[0.78rem]">
                {$_("packages.published")}
              </p>
            {:else if r.publishError === "no-git"}
              <p class="text-warning text-[0.78rem]">
                {$_("packages.notPublishedNoGit")}
              </p>
            {:else if r.publishError}
              <p class="text-danger text-[0.78rem] whitespace-pre-wrap">
                {$_("packages.publishFailed")}
                {r.publishError}
              </p>
            {/if}
          {/if}
          {#each r.failed as f}
            <div class="text-[0.76rem]">
              <span class="font-mono text-text-bright">{f.file}</span>
              <p class="text-text-muted whitespace-pre-wrap">{f.error}</p>
            </div>
          {/each}
        </div>
      {/each}
    </div>
  </Modal>
{/if}

{#if removeTarget}
  <Modal
    variant="danger"
    title={$_("packages.removeTitle", { values: { name: removeTarget.name } })}
    confirmLabel={$_("packages.remove")}
    onClose={() => (removeTarget = null)}
    onConfirm={remove}
  >
    <p>{$_("packages.removeText", { values: { repo: list?.repo ?? "" } })}</p>
  </Modal>
{/if}

{#if errorMessage}
  <Modal
    variant="info"
    title={$_("common.error")}
    onClose={() => (errorMessage = null)}
  >
    <p class="whitespace-pre-wrap">{errorMessage}</p>
  </Modal>
{/if}
