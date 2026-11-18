<script lang="ts">
  import { onMount } from "svelte";
  import { _, locale } from "svelte-i18n";
  import { KeyRound, Plus, Upload, FileOutput, Archive, Trash2, ChevronDown } from "lucide-svelte";
  import { api } from "../lib/api";
  import { progress } from "../lib/progress";
  import { type SigningKey, type ImportResult, groupFingerprint, expiresSoon } from "../lib/gpg";

  // The cards start folded: name, e-mail and badges. The arrow opens the details and the buttons.
  let open = new Set<string>();
  function toggle(fingerprint: string) {
    open.has(fingerprint) ? open.delete(fingerprint) : open.add(fingerprint);
    open = open;
  }
  import Modal from "../components/Modal.svelte";
  import Select from "../components/Select.svelte";

  let keys: SigningKey[] = [];
  let loading = true;
  let busy = false;
  let errorMessage: string | null = null;
  let notice: string | null = null;

  async function load() {
    loading = true;
    try {
      keys = await api.invoke<SigningKey[]>("list_signing_keys");
    } catch (error) {
      errorMessage = String(error);
    } finally {
      loading = false;
    }
  }

  onMount(load);

  $: dateFormat = new Intl.DateTimeFormat($locale ?? "en", { dateStyle: "medium" });
  function date(seconds: number): string {
    return dateFormat.format(new Date(seconds * 1000));
  }

  // Runs a long action with the bar at the bottom of the window sliding, and
  // every button of the page off until it ends.
  async function work<T>(action: () => Promise<T>): Promise<T | undefined> {
    busy = true;
    progress.set("busy");
    try {
      return await action();
    } catch (error) {
      errorMessage = String(error);
      return undefined;
    } finally {
      busy = false;
      progress.set(null);
    }
  }

  // New key
  let createOpen = false;
  let newName = "";
  let newEmail = "";
  let newExpiry = "never";
  let newPass = "";
  let newPass2 = "";
  let noPassphrase = false;

  function openCreate() {
    newName = "";
    newEmail = "";
    newExpiry = "never";
    newPass = "";
    newPass2 = "";
    noPassphrase = false;
    createOpen = true;
  }

  $: passOk = noPassphrase || (newPass.length >= 8 && newPass === newPass2);
  $: createOk = newName.trim() !== "" && /^[^\s<>@]+@[^\s<>@]+$/.test(newEmail.trim()) && passOk;

  async function create() {
    if (!createOk || busy) return;
    createOpen = false;
    const done = await work(() =>
      api.invoke<string>("create_signing_key", {
        name: newName,
        email: newEmail,
        expiry: newExpiry,
        passphrase: noPassphrase ? null : newPass,
      }),
    );
    newPass = "";
    newPass2 = "";
    if (done) {
      notice = $_("keys.created");
      await load();
    }
  }

  // Import
  async function importKey() {
    const path = await api.invoke<string | null>("pick_key_file");
    if (!path) return;
    const result = await work(() => api.invoke<ImportResult>("import_signing_key", { path }));
    if (!result) return;
    if (result.secretImported > 0) notice = $_("keys.imported", { values: { count: result.secretImported } });
    else if (result.secretUnchanged > 0) notice = $_("keys.alreadyThere");
    else notice = $_("keys.noSecret");
    await load();
  }

  // Export of the public key
  let exportKey: SigningKey | null = null;

  function fileBase(key: SigningKey): string {
    const base = (key.name || key.keyId).toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
    return base || key.keyId;
  }

  async function exportPublic(armor: boolean) {
    const key = exportKey;
    exportKey = null;
    if (!key) return;
    const path = await api.invoke<string | null>("pick_save_path", {
      suggestedName: `${fileBase(key)}.${armor ? "asc" : "gpg"}`,
    });
    if (!path) return;
    const ok = await work(() => api.invoke("export_public_key", { fingerprint: key.fingerprint, path, armor }).then(() => true));
    if (ok) notice = $_("keys.exported");
  }

  // Backup of the secret key
  let backupKey: SigningKey | null = null;

  async function backup() {
    const key = backupKey;
    backupKey = null;
    if (!key) return;
    const path = await api.invoke<string | null>("pick_save_path", { suggestedName: `${fileBase(key)}-secret.asc` });
    if (!path) return;
    const ok = await work(() => api.invoke("backup_secret_key", { fingerprint: key.fingerprint, path }).then(() => true));
    if (ok) notice = $_("keys.backedUp");
  }

  // Delete
  let deleteKey: SigningKey | null = null;

  async function remove() {
    const key = deleteKey;
    deleteKey = null;
    if (!key) return;
    const ok = await work(() => api.invoke("delete_signing_key", { fingerprint: key.fingerprint }).then(() => true));
    if (ok) await load();
  }

  const inputClass =
    "w-full bg-surface-2 border border-border rounded px-3 py-2 text-[0.85rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60";
  const smallButton =
    "flex items-center gap-1.5 px-2.5 py-1.5 rounded border border-border text-[0.78rem] text-text hover:bg-surface-2 hover:text-text-bright disabled:opacity-40 disabled:pointer-events-none";
</script>

<div class="sticky top-0 z-10 bg-dark pb-3 pt-1 -mx-7 px-7 border-b border-border flex items-start justify-between gap-4 mb-4">
  <p class="text-[0.82rem] text-text-muted max-w-[520px]">{$_("keys.intro")}</p>
  <div class="flex gap-2 shrink-0">
    <button
      class="flex items-center gap-1.5 px-3 py-2 rounded border border-border text-text hover:bg-surface-2 text-[0.8rem] disabled:opacity-40"
      disabled={busy}
      on:click={importKey}
    >
      <Upload size={14} />
      {$_("keys.import")}
    </button>
    <button
      class="flex items-center gap-1.5 px-3 py-2 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.8rem] font-semibold disabled:opacity-40"
      disabled={busy}
      on:click={openCreate}
    >
      <Plus size={14} />
      {$_("keys.create")}
    </button>
  </div>
</div>

{#if busy}
  <p class="mb-3 text-[0.8rem] text-text-muted">{$_("keys.working")}</p>
{/if}

{#if loading}
  <p class="text-text-muted text-[0.85rem]">{$_("keys.loading")}</p>
{:else if keys.length === 0}
  <div class="flex flex-col items-center justify-center gap-2 text-center py-10">
    <KeyRound size={56} class="text-text-muted" />
    <p class="text-[0.85rem] text-text-muted max-w-[420px]">{$_("keys.empty")}</p>
  </div>
{:else}
  <div class="flex flex-col gap-2.5">
    {#each keys as key (key.fingerprint)}
      <div class="rounded-lg border border-border bg-surface p-4 flex gap-4">
        <div class="w-10 h-10 rounded-lg grid place-items-center bg-primary/15 text-primary shrink-0">
          <KeyRound size={20} />
        </div>

        <div class="min-w-0 flex-1">
          <div class="flex items-start gap-2">
            <div class="min-w-0 flex flex-wrap items-baseline gap-x-2">
              <span class="text-text-bright font-semibold text-[0.92rem] truncate">{key.name || key.keyId}</span>
              {#if key.email}<span class="text-text-muted text-[0.8rem] truncate">{key.email}</span>{/if}
            </div>
          <button
            type="button"
            class="ml-auto self-start w-7 h-7 grid place-items-center rounded text-text-muted hover:bg-surface-2 hover:text-text-bright"
            title={open.has(key.fingerprint) ? $_("common.collapse") : $_("common.expand")}
            aria-label={open.has(key.fingerprint) ? $_("common.collapse") : $_("common.expand")}
            aria-expanded={open.has(key.fingerprint)}
            on:click={() => toggle(key.fingerprint)}
          >
            <ChevronDown size={16} class="transition-transform {open.has(key.fingerprint) ? 'rotate-180' : ''}" />
          </button>
          </div>

          <div class="flex flex-wrap gap-1.5 mt-1.5">
            {#each key.usedBy as repo}
              <span class="px-2 py-0.5 rounded-full bg-primary/15 text-primary text-[0.7rem] font-semibold">
                {$_("keys.signs", { values: { repo } })}
              </span>
            {/each}
            {#if key.revoked}
              <span class="px-2 py-0.5 rounded-full bg-danger/15 text-danger text-[0.7rem] font-semibold">{$_("keys.revoked")}</span>
            {:else if key.expired}
              <span class="px-2 py-0.5 rounded-full bg-danger/15 text-danger text-[0.7rem] font-semibold">{$_("keys.expired")}</span>
            {:else if expiresSoon(key)}
              <span class="px-2 py-0.5 rounded-full bg-warning/15 text-warning text-[0.7rem] font-semibold">{$_("keys.expiresSoon")}</span>
            {/if}
            {#if key.protected === false}
              <span class="px-2 py-0.5 rounded-full bg-warning/15 text-warning text-[0.7rem] font-semibold" title={$_("keys.noPassphraseHint")}>
                {$_("keys.noPassphrase")}
              </span>
            {/if}
          </div>

          {#if open.has(key.fingerprint)}
          <dl class="grid grid-cols-[auto_1fr] gap-x-4 gap-y-0.5 mt-2.5 text-[0.76rem]">
            <dt class="text-text-muted">{$_("keys.fingerprint")}</dt>
            <dd class="font-mono text-text break-all">{groupFingerprint(key.fingerprint)}</dd>
            <dt class="text-text-muted">{$_("keys.type")}</dt>
            <dd class="text-text">{key.algorithm}</dd>
            <dt class="text-text-muted">{$_("keys.createdOn")}</dt>
            <dd class="text-text">{date(key.created)}</dd>
            <dt class="text-text-muted">{$_("keys.expires")}</dt>
            <dd class="text-text">{key.expires ? date(key.expires) : $_("keys.never")}</dd>
          </dl>
          {/if}
        </div>

        {#if open.has(key.fingerprint)}
        <div class="flex flex-col gap-1.5 shrink-0">
          <button class={smallButton} disabled={busy} on:click={() => (exportKey = key)}>
            <FileOutput size={13} />
            {$_("keys.exportPublic")}
          </button>
          <button class={smallButton} disabled={busy} on:click={() => (backupKey = key)}>
            <Archive size={13} />
            {$_("keys.backup")}
          </button>
          <!-- A key that signs a repository cannot be deleted: the button is off and says why. -->
          <span title={key.usedBy.length ? $_("keys.inUse", { values: { repos: key.usedBy.join(", ") } }) : ""}>
            <button
              class="{smallButton} w-full hover:text-danger"
              disabled={busy || key.usedBy.length > 0}
              on:click={() => (deleteKey = key)}
            >
              <Trash2 size={13} />
              {$_("keys.delete")}
            </button>
          </span>
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
    title={$_("keys.createTitle")}
    confirmLabel={$_("keys.createConfirm")}
    confirmDisabled={!createOk}
    maxWidth={480}
    onClose={() => (createOpen = false)}
    onConfirm={create}
  >
    <div class="flex flex-col gap-3">
      <label class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("keys.name")}</span>
        <input class={inputClass} bind:value={newName} placeholder={$_("keys.namePlaceholder")} />
      </label>
      <label class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("keys.email")}</span>
        <input class={inputClass} type="email" bind:value={newEmail} placeholder="repo@example.org" />
      </label>
      <div class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("keys.expiry")}</span>
        <Select
          bind:value={newExpiry}
          label={$_("keys.expiry")}
          options={[
            { value: "never", label: $_("keys.never") },
            { value: "1y", label: $_("keys.oneYear") },
            { value: "2y", label: $_("keys.twoYears") },
          ]}
        />
      </div>
      <p class="text-[0.75rem] text-text-muted">{$_("keys.typeNote")}</p>

      <label class="flex items-center gap-2 text-[0.82rem] text-text">
        <input type="checkbox" bind:checked={noPassphrase} />
        {$_("keys.withoutPassphrase")}
      </label>
      {#if noPassphrase}
        <p class="px-3 py-2 rounded border border-warning/30 bg-warning/10 text-[0.78rem] text-text">{$_("keys.withoutPassphraseWarning")}</p>
      {:else}
        <label class="flex flex-col gap-1">
          <span class="text-[0.78rem] text-text-muted">{$_("keys.passphrase")}</span>
          <input class={inputClass} type="password" bind:value={newPass} autocomplete="new-password" />
        </label>
        <label class="flex flex-col gap-1">
          <span class="text-[0.78rem] text-text-muted">{$_("keys.passphraseAgain")}</span>
          <input class={inputClass} type="password" bind:value={newPass2} autocomplete="new-password" />
        </label>
        {#if newPass && newPass.length < 8}
          <p class="text-[0.75rem] text-warning">{$_("keys.passphraseShort")}</p>
        {:else if newPass2 && newPass !== newPass2}
          <p class="text-[0.75rem] text-warning">{$_("keys.passphraseMismatch")}</p>
        {/if}
      {/if}
    </div>
  </Modal>
{/if}

{#if exportKey}
  <Modal variant="info" showFooter={false} title={$_("keys.exportTitle")} onClose={() => (exportKey = null)}>
    <p class="mb-3">{$_("keys.exportText")}</p>
    <div class="flex flex-col gap-2">
      <button class="text-left px-3 py-2.5 rounded border border-border hover:bg-surface-2" on:click={() => exportPublic(true)}>
        <span class="block text-text-bright font-semibold text-[0.85rem]">{$_("keys.exportAsc")}</span>
        <span class="block text-text-muted text-[0.76rem]">{$_("keys.exportAscHint")}</span>
      </button>
      <button class="text-left px-3 py-2.5 rounded border border-border hover:bg-surface-2" on:click={() => exportPublic(false)}>
        <span class="block text-text-bright font-semibold text-[0.85rem]">{$_("keys.exportGpg")}</span>
        <span class="block text-text-muted text-[0.76rem]">{$_("keys.exportGpgHint")}</span>
      </button>
    </div>
  </Modal>
{/if}

{#if backupKey}
  <Modal
    variant="warning"
    title={$_("keys.backupTitle")}
    confirmLabel={$_("keys.backupConfirm")}
    onClose={() => (backupKey = null)}
    onConfirm={backup}
  >
    <p>{$_("keys.backupText")}</p>
  </Modal>
{/if}

{#if deleteKey}
  <Modal
    variant="danger"
    title={$_("keys.deleteTitle", { values: { name: deleteKey.name || deleteKey.keyId } })}
    confirmLabel={$_("keys.delete")}
    onClose={() => (deleteKey = null)}
    onConfirm={remove}
  >
    <p>{$_("keys.deleteText")}</p>
  </Modal>
{/if}

{#if notice}
  <Modal variant="info" title={notice} onClose={() => (notice = null)} />
{/if}

{#if errorMessage}
  <Modal variant="info" title={$_("common.error")} onClose={() => (errorMessage = null)}>
    <p class="whitespace-pre-wrap">{errorMessage}</p>
  </Modal>
{/if}
