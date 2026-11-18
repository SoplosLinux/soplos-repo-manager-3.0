<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import {
    CloudUpload,
    GitBranch,
    Plus,
    Pencil,
    Trash2,
    Copy,
    Check,
    AlertCircle,
  } from "lucide-svelte";
  import { api } from "../lib/api";
  import { targets, loadTargets, type Target } from "../lib/targetStore";
  import Modal from "../components/Modal.svelte";

  let loading = true;
  let copiedId: string | null = null;
  let modalOpen = false;
  let isEditing = false;
  let busy = false;
  let errorMessage: string | null = null;

  // Form fields
  let targetId = "";
  let targetName = "";
  let targetDesc = "";
  let targetUrl = "";
  let targetBranch = "main";

  async function load() {
    loading = true;
    errorMessage = null;
    try {
      await loadTargets();
    } catch (err) {
      errorMessage = String(err);
    } finally {
      loading = false;
    }
  }

  onMount(load);

  async function copyUrl(target: Target) {
    try {
      await navigator.clipboard.writeText(target.gitUrl);
      copiedId = target.id;
      setTimeout(() => (copiedId = null), 2000);
    } catch (err) {
      console.error(err);
    }
  }

  function openCreateModal() {
    isEditing = false;
    targetId = "";
    targetName = "";
    targetDesc = "";
    targetUrl = "";
    targetBranch = "main";
    modalOpen = true;
  }

  function openEditModal(target: Target) {
    isEditing = true;
    targetId = target.id;
    targetName = target.name;
    targetDesc = target.description;
    targetUrl = target.gitUrl;
    targetBranch = target.branch || "main";
    modalOpen = true;
  }

  async function saveTarget() {
    if (!targetId.trim() || !targetUrl.trim()) return;
    busy = true;
    errorMessage = null;

    try {
      await api.invoke("save_target", {
        target: {
          id: targetId.trim(),
          name: targetName.trim() || targetId.trim(),
          description: targetDesc.trim(),
          gitUrl: targetUrl.trim(),
          branch: targetBranch.trim() || "main",
        },
      });
      modalOpen = false;
      await loadTargets();
    } catch (err) {
      errorMessage = String(err);
    } finally {
      busy = false;
    }
  }

  async function removeTarget(id: string) {
    if (!confirm($_("targets.deleteConfirm"))) return;
    try {
      await api.invoke("remove_target", { id });
      await loadTargets();
    } catch (err) {
      errorMessage = String(err);
    }
  }
</script>

<div class="flex flex-col gap-5 max-w-[860px]">
  <div class="flex items-start justify-between gap-4">
    <div>
      <h2 class="text-[0.95rem] font-bold text-text-bright mb-1">{$_("targets.title")}</h2>
      <p class="text-[0.8rem] text-text-muted">{$_("targets.subtitle")}</p>
    </div>
    <button
      class="flex items-center gap-1.5 px-3 py-1.5 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.8rem] font-semibold shrink-0 transition-colors shadow-sm cursor-pointer"
      on:click={openCreateModal}
    >
      <Plus size={14} />
      {$_("targets.addCustom")}
    </button>
  </div>

  {#if errorMessage}
    <div class="rounded-md border border-danger/30 bg-danger/10 p-3.5 flex items-start gap-2.5 text-danger text-[0.82rem]">
      <AlertCircle size={16} class="shrink-0 mt-0.5" />
      <span class="flex-1">{errorMessage}</span>
    </div>
  {/if}

  {#if loading}
    <div class="py-12 text-center text-text-muted text-[0.82rem]">
      {$_("common.loading")}...
    </div>
  {:else if $targets.length === 0}
    <div class="flex flex-col items-center justify-center gap-3 text-center py-12 border-2 border-dashed border-border rounded-xl bg-surface/40">
      <CloudUpload size={42} class="text-text-muted" />
      <div class="flex flex-col gap-1 max-w-[420px]">
        <span class="text-[0.9rem] font-semibold text-text-bright">{$_("targets.emptyTitle")}</span>
        <p class="text-[0.78rem] text-text-muted leading-relaxed">
          {$_("targets.emptyDesc")}
        </p>
      </div>
    </div>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 gap-3.5">
      {#each $targets as target (target.id)}
        <div class="rounded-lg border border-border bg-surface p-4 flex flex-col justify-between gap-3 hover:border-border/80 transition-colors">
          <div class="flex flex-col gap-1.5">
            <div class="flex items-center justify-between gap-2">
              <span class="font-bold text-[0.88rem] text-text-bright">{target.name}</span>
              <div class="flex items-center gap-1">
                <button
                  class="text-text-muted hover:text-text-bright p-1 rounded transition-colors cursor-pointer"
                  title={$_("targets.edit")}
                  on:click={() => openEditModal(target)}
                >
                  <Pencil size={13} />
                </button>
                <button
                  class="text-text-muted hover:text-danger p-1 rounded transition-colors cursor-pointer"
                  title={$_("targets.delete")}
                  on:click={() => removeTarget(target.id)}
                >
                  <Trash2 size={13} />
                </button>
              </div>
            </div>
            {#if target.description}
              <p class="text-[0.76rem] text-text-muted leading-relaxed min-h-[30px]">
                {target.description}
              </p>
            {/if}
            <div class="flex items-center gap-1.5 text-[0.72rem] font-mono text-text-muted bg-surface-2 px-2 py-1.5 rounded border border-border/50 min-w-0">
              <span class="truncate flex-1" title={target.gitUrl}>{target.gitUrl}</span>
              <button
                type="button"
                class="text-text-muted hover:text-text-bright p-0.5 rounded shrink-0 cursor-pointer"
                title={$_("repos.copySources")}
                on:click={() => copyUrl(target)}
              >
                {#if copiedId === target.id}
                  <Check size={12} class="text-success" />
                {:else}
                  <Copy size={12} />
                {/if}
              </button>
            </div>
          </div>

          <div class="flex items-center justify-between gap-2 pt-2 border-t border-border/50 text-[0.72rem] text-text-muted">
            <span class="flex items-center gap-1">
              <GitBranch size={12} /> {target.branch}
            </span>
            <span class="font-mono bg-surface-2 px-1.5 py-0.5 rounded border border-border/40">
              {target.id}
            </span>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<!-- Modal: Add / Edit Target -->
{#if modalOpen}
  <Modal
    variant="confirm"
    showIcon={false}
    title={isEditing ? $_("targets.editCustomTitle") : $_("targets.addCustomTitle")}
    confirmLabel={$_("common.save")}
    cancelLabel={$_("common.cancel")}
    confirmDisabled={busy || !targetId.trim() || !targetUrl.trim()}
    onClose={() => (modalOpen = false)}
    onConfirm={saveTarget}
  >
    <div class="flex flex-col gap-3.5">
      <div>
        <label for="target-id" class="block text-[0.76rem] font-semibold text-text-bright mb-1">
          {$_("targets.fieldId")} *
        </label>
        <input
          id="target-id"
          class="w-full bg-surface-2 border border-border rounded px-3 py-1.5 text-[0.82rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60 font-mono disabled:opacity-60"
          placeholder={$_("targets.idPlaceholder")}
          bind:value={targetId}
          disabled={isEditing}
        />
      </div>

      <div>
        <label for="target-name" class="block text-[0.76rem] font-semibold text-text-bright mb-1">
          {$_("targets.fieldName")}
        </label>
        <input
          id="target-name"
          class="w-full bg-surface-2 border border-border rounded px-3 py-1.5 text-[0.82rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60"
          placeholder={$_("targets.namePlaceholder")}
          bind:value={targetName}
        />
      </div>

      <div>
        <label for="target-url" class="block text-[0.76rem] font-semibold text-text-bright mb-1">
          {$_("targets.fieldUrl")} *
        </label>
        <input
          id="target-url"
          class="w-full bg-surface-2 border border-border rounded px-3 py-1.5 text-[0.82rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60 font-mono"
          placeholder={$_("targets.urlPlaceholder")}
          bind:value={targetUrl}
        />
      </div>

      <div class="grid grid-cols-2 gap-3">
        <div>
          <label for="target-branch" class="block text-[0.76rem] font-semibold text-text-bright mb-1">
            {$_("targets.fieldBranch")}
          </label>
          <input
            id="target-branch"
            class="w-full bg-surface-2 border border-border rounded px-3 py-1.5 text-[0.82rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60 font-mono"
            placeholder="main"
            bind:value={targetBranch}
          />
        </div>
        <div>
          <label for="target-desc" class="block text-[0.76rem] font-semibold text-text-bright mb-1">
            {$_("targets.fieldDesc")}
          </label>
          <input
            id="target-desc"
            class="w-full bg-surface-2 border border-border rounded px-3 py-1.5 text-[0.82rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60"
            placeholder={$_("targets.descPlaceholder")}
            bind:value={targetDesc}
          />
        </div>
      </div>
    </div>
  </Modal>
{/if}
