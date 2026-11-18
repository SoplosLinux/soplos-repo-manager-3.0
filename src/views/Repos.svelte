<script lang="ts">
  import { onMount, getContext } from "svelte";
  import { _, locale } from "svelte-i18n";
  import {
    Database,
    Plus,
    Pencil,
    Trash2,
    FolderOpen,
    Check,
    Terminal,
    Copy,
    ChevronDown,
  } from "lucide-svelte";
  import { api } from "../lib/api";
  import { progress } from "../lib/progress";
  import { type Repo, ARCHITECTURES, namesKey, sameKey } from "../lib/repos";
  import { repos as repoList, activeRepo, loadRepos, chooseRepo } from "../lib/repoStore";
  import type { SigningKey } from "../lib/gpg";
  import Modal from "../components/Modal.svelte";
  import Select from "../components/Select.svelte";

  const navigate = getContext<(id: string) => void>("navigate");

  // The list and the repository in use are the ones of the selector of the sidebar.
  $: repos = $repoList;
  $: active = $activeRepo;
  let keys: SigningKey[] = [];

  // The cards start folded: name, title and warnings. The arrow opens the details and the buttons.
  let open = new Set<string>();
  function toggle(name: string) {
    open.has(name) ? open.delete(name) : open.add(name);
    open = open;
  }
  let loading = true;
  let busy = false;
  let errorMessage: string | null = null;

  async function load() {
    loading = true;
    try {
      await loadRepos();
    } catch (error) {
      errorMessage = String(error);
    }
    try {
      keys = await api.invoke<SigningKey[]>("list_signing_keys");
    } catch {
      // Without the keys, the cards only show the ID of the key of each repository.
      keys = [];
    }
    loading = false;
  }

  onMount(load);

  async function select(repo: Repo) {
    if (busy || active === repo.name) return;
    try {
      await chooseRepo(repo.name);
    } catch (error) {
      errorMessage = String(error);
    }
  }

  $: keyOptions = keys.map((k) => ({ value: k.fingerprint, label: `${k.name} (${k.keyId})` }));

  function keyOf(repo: Repo): SigningKey | undefined {
    return keys.find((k) => namesKey(repo.gpgKey, k.fingerprint));
  }

  $: dateFormat = new Intl.DateTimeFormat($locale ?? "en", { dateStyle: "medium", timeStyle: "short" });
  function released(value: string): string {
    const d = new Date(value);
    return isNaN(d.getTime()) ? value : dateFormat.format(d);
  }

  async function work(action: () => Promise<unknown>): Promise<boolean> {
    busy = true;
    progress.set("busy");
    try {
      await action();
      return true;
    } catch (error) {
      errorMessage = String(error);
      return false;
    } finally {
      busy = false;
      progress.set(null);
    }
  }

  // New repository
  let createOpen = false;
  let form = { name: "", title: "", description: "", dir: "", gpgKey: "", architectures: ["amd64"] };

  function openCreate() {
    form = { name: "", title: "", description: "", dir: "", gpgKey: keys[0]?.fingerprint ?? "", architectures: ["amd64"] };
    createOpen = true;
  }

  async function chooseFolder() {
    const path = await api.invoke<string | null>("pick_folder");
    if (path) form.dir = path;
  }

  function toggleArch(arch: string, on: boolean) {
    form.architectures = on ? [...form.architectures, arch] : form.architectures.filter((a) => a !== arch);
  }

  $: nameOk = /^[a-z0-9][a-z0-9-]*$/.test(form.name) && !repos.some((r) => r.name === form.name);
  $: createOk = nameOk && form.title.trim() !== "" && form.dir.startsWith("/") && form.gpgKey !== "" && form.architectures.length > 0;

  async function create() {
    if (!createOk) return;
    createOpen = false;
    if (await work(() => api.invoke("create_repo", { repo: form }))) await load();
  }

  // Edit
  let editRepo: Repo | null = null;
  let edit = { title: "", description: "", gitUrl: "", branch: "", listFilename: "", gpgKey: "" };

  function openEdit(repo: Repo) {
    editRepo = repo;
    edit = {
      title: repo.title,
      description: repo.description,
      gitUrl: repo.gitUrl,
      branch: repo.branch,
      listFilename: repo.listFilename,
      gpgKey: keyOf(repo)?.fingerprint ?? repo.gpgKey,
    };
  }

  $: keyChanges = editRepo ? !sameKey(edit.gpgKey, editRepo.gpgKey) : false;

  async function saveEdit() {
    const repo = editRepo;
    if (!repo || edit.title.trim() === "") return;
    editRepo = null;
    if (await work(() => api.invoke("update_repo", { name: repo.name, changes: edit }))) await load();
  }

  // Remove
  let removeRepo: Repo | null = null;

  async function remove() {
    const repo = removeRepo;
    removeRepo = null;
    if (repo && (await work(() => api.invoke("remove_repo", { name: repo.name })))) await load();
  }

  const inputClass =
    "w-full bg-surface-2 border border-border rounded px-3 py-2 text-[0.85rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60";
  const smallButton =
    "flex items-center gap-1.5 px-2.5 py-1.5 rounded border border-border text-[0.78rem] text-text hover:bg-surface-2 hover:text-text-bright disabled:opacity-40 disabled:pointer-events-none";
  const badge = "px-2 py-0.5 rounded-full text-[0.7rem] font-semibold";

  // Install script & sources
  interface InstallScript {
    sourcesContent: string;
    sourcesFilename: string;
    bashScript: string;
    keyUrl: string;
    keyringPath: string;
  }

  let scriptModalRepo: Repo | null = null;
  let scriptData: InstallScript | null = null;
  let scriptLoading = false;
  let copiedScript = false;
  let copiedSources = false;

  async function openScript(repo: Repo) {
    scriptModalRepo = repo;
    scriptData = null;
    scriptLoading = true;
    copiedScript = false;
    copiedSources = false;
    try {
      scriptData = await api.invoke<InstallScript>("generate_install_script", { name: repo.name });
    } catch (err) {
      errorMessage = String(err);
      scriptModalRepo = null;
    } finally {
      scriptLoading = false;
    }
  }

  async function copyText(text: string, type: "script" | "sources") {
    try {
      await navigator.clipboard.writeText(text);
      if (type === "script") {
        copiedScript = true;
        setTimeout(() => (copiedScript = false), 2000);
      } else {
        copiedSources = true;
        setTimeout(() => (copiedSources = false), 2000);
      }
    } catch (err) {
      console.error(err);
    }
  }
</script>

<div class="flex items-start justify-between gap-4 mb-4">
  <p class="text-[0.82rem] text-text-muted max-w-[520px]">{$_("repos.intro")}</p>
  <button
    class="flex items-center gap-1.5 px-3 py-2 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.8rem] font-semibold disabled:opacity-40 shrink-0"
    disabled={busy}
    on:click={openCreate}
  >
    <Plus size={14} />
    {$_("repos.create")}
  </button>
</div>

{#if busy}
  <p class="mb-3 text-[0.8rem] text-text-muted">{$_("repos.working")}</p>
{/if}

{#if loading}
  <p class="text-text-muted text-[0.85rem]">{$_("repos.loading")}</p>
{:else if repos.length === 0}
  <div class="flex flex-col items-center justify-center gap-2 text-center py-10">
    <Database size={56} class="text-text-muted" />
    <p class="text-[0.85rem] text-text-muted max-w-[420px]">{$_("repos.empty")}</p>
  </div>
{:else}
  <div class="flex flex-col gap-2.5">
    {#each repos as repo (repo.name)}
      {@const key = keyOf(repo)}
      <!-- A click on the card makes it the repository in use, as the selector of the sidebar does. -->
      <div
        class="rounded-lg border bg-surface p-4 flex gap-4 cursor-pointer transition-colors
          {active === repo.name ? 'border-primary/50' : 'border-border hover:border-primary/30'}"
        role="button"
        tabindex="0"
        aria-pressed={active === repo.name}
        on:click={() => select(repo)}
        on:keydown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); select(repo); } }}
      >
        <div class="w-10 h-10 rounded-lg grid place-items-center bg-primary/15 text-primary shrink-0">
          <Database size={20} />
        </div>

        <div class="min-w-0 flex-1">
          <div class="flex items-start gap-2">
            <div class="min-w-0 flex flex-wrap items-baseline gap-x-2">
              <span class="text-text-bright font-semibold text-[0.92rem]">{repo.name}</span>
              <span class="text-text-muted text-[0.8rem] truncate">{repo.title}</span>
            </div>
          <button
            type="button"
            class="ml-auto self-start w-7 h-7 grid place-items-center rounded text-text-muted hover:bg-surface-2 hover:text-text-bright"
            title={open.has(repo.name) ? $_("common.collapse") : $_("common.expand")}
            aria-label={open.has(repo.name) ? $_("common.collapse") : $_("common.expand")}
            aria-expanded={open.has(repo.name)}
            on:click|stopPropagation={() => toggle(repo.name)}
          >
            <ChevronDown size={16} class="transition-transform {open.has(repo.name) ? 'rotate-180' : ''}" />
          </button>
          </div>

          <div class="flex flex-wrap gap-1.5 mt-1.5">
            {#if !repo.exists}
              <span class="{badge} bg-danger/15 text-danger">{$_("repos.missing")}</span>
            {:else if !repo.managed}
              <span class="{badge} bg-warning/15 text-warning">{$_("repos.notReprepro")}</span>
            {/if}
            {#if !key}
              <span class="{badge} bg-danger/15 text-danger">{$_("repos.keyMissing")}</span>
            {/if}
            {#if repo.managed && repo.signWith && !sameKey(repo.signWith, repo.gpgKey)}
              <span class="{badge} bg-warning/15 text-warning" title={$_("repos.keyMismatchHint", { values: { key: repo.signWith } })}>
                {$_("repos.keyMismatch")}
              </span>
            {/if}
          </div>

          {#if open.has(repo.name)}
          <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-0.5 mt-2.5 text-[0.76rem]">
            <dt class="text-text-muted">{$_("repos.folder")}</dt>
            <dd class="font-mono text-text break-all">{repo.dir}</dd>
            <dt class="text-text-muted">{$_("repos.key")}</dt>
            <dd class="text-text">{key ? `${key.name} (${key.keyId})` : repo.gpgKey || $_("repos.none")}</dd>
            <dt class="text-text-muted">{$_("repos.packages")}</dt>
            <dd class="text-text">{repo.packages}</dd>
            {#if repo.architectures.length}
              <dt class="text-text-muted">{$_("repos.architectures")}</dt>
              <dd class="text-text">{repo.architectures.join(", ")}</dd>
            {/if}
            <dt class="text-text-muted">{$_("repos.signed")}</dt>
            <dd class="text-text">{repo.released ? released(repo.released) : $_("repos.never")}</dd>
            <dt class="text-text-muted">{$_("repos.git")}</dt>
            <dd class="text-text break-all">{repo.gitUrl ? `${repo.gitUrl} · ${repo.branch}` : $_("repos.noGit")}</dd>
          </dl>
          {/if}
        </div>

        {#if open.has(repo.name)}
        <div class="flex flex-col gap-1.5 shrink-0">
          <button
            class={smallButton}
            disabled={busy || !repo.gitUrl}
            title={repo.gitUrl ? $_("repos.installScript") : $_("repos.noGit")}
            on:click|stopPropagation={() => openScript(repo)}
          >
            <Terminal size={13} />
            {$_("repos.installScript")}
          </button>
          <button class={smallButton} disabled={busy} on:click|stopPropagation={() => openEdit(repo)}>
            <Pencil size={13} />
            {$_("repos.edit")}
          </button>
          <button class="{smallButton} hover:text-danger" disabled={busy} on:click|stopPropagation={() => (removeRepo = repo)}>
            <Trash2 size={13} />
            {$_("repos.remove")}
          </button>
        </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

{#if createOpen}
  <Modal
    variant="confirm"
    showIcon={false}
    title={$_("repos.createTitle")}
    confirmLabel={$_("repos.createConfirm")}
    confirmDisabled={!createOk}
    maxWidth={520}
    onClose={() => (createOpen = false)}
    onConfirm={create}
  >
    <div class="flex flex-col gap-3">
      <label class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("repos.name")}</span>
        <input class={inputClass} bind:value={form.name} placeholder="dev-tools" />
        <span class="text-[0.72rem] {form.name && !nameOk ? 'text-warning' : 'text-text-muted'}">{$_("repos.nameHint")}</span>
      </label>
      <label class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("repos.title")}</span>
        <input class={inputClass} bind:value={form.title} />
        <span class="text-[0.72rem] text-text-muted">{$_("repos.titleHint")}</span>
      </label>
      <label class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("repos.description")}</span>
        <input class={inputClass} bind:value={form.description} />
      </label>
      <div class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("repos.folder")}</span>
        <div class="flex gap-2">
          <input class={inputClass} bind:value={form.dir} placeholder="/home/…/apt-repos/dev-tools" />
          <button class="{smallButton} shrink-0" on:click={chooseFolder}>
            <FolderOpen size={13} />
            {$_("repos.choose")}
          </button>
        </div>
        <span class="text-[0.72rem] text-text-muted">{$_("repos.folderHint")}</span>
      </div>
      <div class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("repos.key")}</span>
        {#if keys.length}
          <Select bind:value={form.gpgKey} label={$_("repos.key")} options={keyOptions} />
        {:else}
          <p class="text-[0.78rem] text-warning">
            {$_("repos.noKeys")}
            <button class="underline" on:click={() => { createOpen = false; navigate("keys"); }}>{$_("repos.goToKeys")}</button>
          </p>
        {/if}
      </div>
      <div class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("repos.architectures")}</span>
        <div class="flex flex-wrap gap-3">
          {#each ARCHITECTURES as arch}
            <label class="flex items-center gap-1.5 text-[0.82rem] text-text">
              <input type="checkbox" checked={form.architectures.includes(arch)} on:change={(e) => toggleArch(arch, e.currentTarget.checked)} />
              {arch}
            </label>
          {/each}
        </div>
      </div>
    </div>
  </Modal>
{/if}

{#if editRepo}
  <Modal
    variant="confirm"
    showIcon={false}
    title={$_("repos.editTitle", { values: { name: editRepo.name } })}
    confirmLabel={$_("repos.save")}
    confirmDisabled={edit.title.trim() === ""}
    maxWidth={520}
    onClose={() => (editRepo = null)}
    onConfirm={saveEdit}
  >
    <div class="flex flex-col gap-3">
      <label class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("repos.title")}</span>
        <input class={inputClass} bind:value={edit.title} />
      </label>
      <label class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("repos.description")}</span>
        <input class={inputClass} bind:value={edit.description} />
      </label>
      <div class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("repos.key")}</span>
        <!-- A key that is not in the keyring is kept as an option, so saving does not change it. -->
        <Select
          bind:value={edit.gpgKey}
          label={$_("repos.key")}
          options={keys.some((k) => k.fingerprint === edit.gpgKey)
            ? keyOptions
            : [{ value: edit.gpgKey, label: edit.gpgKey || $_("repos.none") }, ...keyOptions]}
        />
        {#if keyChanges}
          <span class="text-[0.72rem] text-warning">{$_("repos.keyChangeHint")}</span>
        {/if}
      </div>
      <label class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("repos.gitUrl")}</span>
        <input class={inputClass} bind:value={edit.gitUrl} placeholder="https://github.com/…/….git" />
      </label>
      <div class="grid grid-cols-2 gap-3">
        <label class="flex flex-col gap-1">
          <span class="text-[0.78rem] text-text-muted">{$_("repos.branch")}</span>
          <input class={inputClass} bind:value={edit.branch} />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-[0.78rem] text-text-muted">{$_("repos.listFilename")}</span>
          <input class={inputClass} bind:value={edit.listFilename} />
        </label>
      </div>
    </div>
  </Modal>
{/if}

{#if removeRepo}
  <Modal
    variant="danger"
    title={$_("repos.removeTitle", { values: { name: removeRepo.name } })}
    confirmLabel={$_("repos.remove")}
    onClose={() => (removeRepo = null)}
    onConfirm={remove}
  >
    <p>{$_("repos.removeText")}</p>
  </Modal>
{/if}

{#if scriptModalRepo}
  <Modal
    variant="info"
    showIcon={false}
    title={$_("repos.installScriptTitle", { values: { name: scriptModalRepo.title || scriptModalRepo.name } })}
    maxWidth={640}
    onClose={() => (scriptModalRepo = null)}
  >
    {#if scriptLoading}
      <p class="text-text-muted text-[0.85rem]">{$_("repos.loading")}</p>
    {:else if scriptData}
      <div class="flex flex-col gap-4 text-left">
        <!-- Sources file (DEB822) -->
        <div class="flex flex-col gap-1.5">
          <div class="flex items-center justify-between">
            <span class="text-[0.78rem] font-semibold text-text-bright">
              {$_("repos.sourcesFileTitle")}
              <span class="font-mono text-text-muted text-[0.72rem] ml-1">(/etc/apt/sources.list.d/{scriptData.sourcesFilename})</span>
            </span>
            <button
              type="button"
              class="flex items-center gap-1 text-xs text-primary hover:underline cursor-pointer"
              on:click={() => { if (scriptData) copyText(scriptData.sourcesContent, "sources"); }}
            >
              {#if copiedSources}
                <Check size={12} class="text-success" />
                <span class="text-success text-[0.72rem]">{$_("repos.copied")}</span>
              {:else}
                <Copy size={12} />
                <span class="text-[0.72rem]">{$_("repos.copy")}</span>
              {/if}
            </button>
          </div>
          <pre class="bg-dark p-3 rounded border border-border text-[0.75rem] font-mono text-text select-text overflow-x-auto leading-relaxed">{scriptData.sourcesContent}</pre>
        </div>

        <!-- Bash install script -->
        <div class="flex flex-col gap-1.5">
          <div class="flex items-center justify-between">
            <span class="text-[0.78rem] font-semibold text-text-bright">
              {$_("repos.bashScriptTitle")}
            </span>
            <button
              type="button"
              class="flex items-center gap-1 text-xs text-primary hover:underline cursor-pointer"
              on:click={() => { if (scriptData) copyText(scriptData.bashScript, "script"); }}
            >
              {#if copiedScript}
                <Check size={12} class="text-success" />
                <span class="text-success text-[0.72rem]">{$_("repos.copied")}</span>
              {:else}
                <Copy size={12} />
                <span class="text-[0.72rem]">{$_("repos.copy")}</span>
              {/if}
            </button>
          </div>
          <p class="text-[0.72rem] text-text-muted">{$_("repos.bashScriptHint")}</p>
          <pre class="bg-dark p-3 rounded border border-border text-[0.72rem] font-mono text-text select-text overflow-x-auto max-h-[180px] content-scroll leading-relaxed">{scriptData.bashScript}</pre>
        </div>
      </div>
    {/if}
  </Modal>
{/if}

{#if errorMessage}
  <Modal variant="info" title={$_("common.error")} onClose={() => (errorMessage = null)}>
    <p class="whitespace-pre-wrap">{errorMessage}</p>
  </Modal>
{/if}
