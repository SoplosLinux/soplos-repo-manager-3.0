<script lang="ts">
  import { onMount } from "svelte";
  import { _ } from "svelte-i18n";
  import { GitPullRequest, Check, X, RefreshCw, ExternalLink, Package, User } from "lucide-svelte";
  import { api } from "../lib/api";
  import Notice from "../components/Notice.svelte";
  import { progress } from "../lib/progress";
  import { activeRepo, repos } from "../lib/repoStore";
  import Select from "../components/Select.svelte";
  import Modal from "../components/Modal.svelte";

  interface ProposalFile {
    filename: string;
    rawUrl: string;
    size: number;
  }

  interface Proposal {
    number: number;
    title: string;
    body: string;
    user: string;
    userAvatar: string;
    branch: string;
    createdAt: string;
    htmlUrl: string;
    files: ProposalFile[];
  }

  let selectedRepo = $activeRepo || "";
  let proposals: Proposal[] = [];
  let loading = false;
  let busy = false;
  let errorMessage: string | null = null;
  let notice: string | null = null;

  // Accept modal state
  let acceptTarget: Proposal | null = null;
  let cleanFirst = true;

  // Reject modal state
  let rejectTarget: Proposal | null = null;
  let rejectReason = "";

  $: repoOptions = $repos.map((r) => ({ value: r.name, label: `${r.name} (${r.title || r.name})` }));

  $: if ($activeRepo && !selectedRepo) {
    selectedRepo = $activeRepo;
  }

  let lastLoaded = "";
  $: if (selectedRepo && selectedRepo !== lastLoaded) {
    lastLoaded = selectedRepo;
    loadProposals(selectedRepo);
  }

  async function loadProposals(name: string) {
    if (!name) return;
    loading = true;
    errorMessage = null;
    try {
      proposals = await api.invoke<Proposal[]>("list_proposals", { name });
    } catch (err) {
      proposals = [];
      errorMessage = String(err);
    } finally {
      loading = false;
    }
  }

  function openAccept(pr: Proposal) {
    acceptTarget = pr;
    cleanFirst = true;
  }

  async function confirmAccept() {
    if (!acceptTarget || !selectedRepo) return;
    const pr = acceptTarget;
    acceptTarget = null;
    busy = true;
    errorMessage = null;
    notice = null;
    progress.set("busy");

    try {
      await api.invoke("accept_proposal", {
        name: selectedRepo,
        prNumber: pr.number,
        cleanFirst,
        comment: $_("proposals.acceptComment", { values: { repo: selectedRepo } }),
      });
      notice = $_("proposals.acceptedNotice", { values: { number: pr.number } });
      await loadProposals(selectedRepo);
    } catch (err) {
      errorMessage = String(err);
    } finally {
      busy = false;
      progress.set(null);
    }
  }

  function openReject(pr: Proposal) {
    rejectTarget = pr;
    rejectReason = "";
  }

  async function confirmReject() {
    if (!rejectTarget || !selectedRepo) return;
    const prNumber = rejectTarget.number;
    rejectTarget = null;
    busy = true;
    errorMessage = null;
    notice = null;
    progress.set("busy");

    try {
      await api.invoke("reject_proposal", {
        name: selectedRepo,
        prNumber,
        comment: rejectReason.trim()
          ? $_("proposals.rejectCommentReason", { values: { reason: rejectReason.trim() } })
          : $_("proposals.rejectComment"),
      });
      notice = $_("proposals.rejectedNotice", { values: { number: prNumber } });
      await loadProposals(selectedRepo);
    } catch (err) {
      errorMessage = String(err);
    } finally {
      busy = false;
      progress.set(null);
    }
  }

  function formatDate(iso: string): string {
    try {
      const d = new Date(iso);
      return d.toLocaleDateString(undefined, {
        year: "numeric",
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
    } catch {
      return iso;
    }
  }

  onMount(() => {
    if (selectedRepo) loadProposals(selectedRepo);
  });

  const button =
    "flex items-center gap-1.5 px-3 py-1.5 rounded border border-border text-text hover:bg-surface-2 text-[0.8rem] disabled:opacity-40 cursor-pointer";
</script>

<div class="relative flex flex-col gap-5">
  <!-- Sticky header -->
  <div class="sticky top-0 z-10 bg-dark pb-3 pt-1 -mx-7 px-7 border-b border-border flex items-center justify-between gap-4">
    <div class="w-72">
      <Select bind:value={selectedRepo} label={$_("proposals.selectRepo")} options={repoOptions} />
    </div>

    <button
      class={button}
      disabled={loading || busy}
      on:click={() => selectedRepo && loadProposals(selectedRepo)}
    >
      <RefreshCw size={13} class={loading ? "animate-spin" : ""} />
      {$_("proposals.refresh")}
    </button>
  </div>

  <Notice bind:message={notice} />

  {#if errorMessage}
    <div class="px-4 py-3 rounded-lg bg-danger/10 border border-danger/30 text-danger text-[0.82rem] break-all">
      {errorMessage}
    </div>
  {/if}

  {#if loading}
    <p class="text-text-muted text-[0.85rem]">{$_("proposals.loading")}</p>
  {:else if proposals.length === 0}
    <div class="flex flex-col items-center justify-center gap-2 text-center py-12 border-2 border-dashed border-border rounded-lg">
      <GitPullRequest size={44} class="text-text-muted" />
      <span class="text-[0.88rem] font-medium text-text-bright">{$_("proposals.emptyTitle")}</span>
      <p class="text-[0.78rem] text-text-muted max-w-[420px]">
        {$_("proposals.emptyText")}
      </p>
    </div>
  {:else}
    <div class="flex flex-col gap-3.5">
      {#each proposals as pr (pr.number)}
        <div class="rounded-lg border border-border bg-surface p-4 flex flex-col gap-3">
          <div class="flex items-start justify-between gap-3">
            <div class="flex items-center gap-2.5">
              {#if pr.userAvatar}
                <img src={pr.userAvatar} alt={pr.user} class="w-7 h-7 rounded-full border border-border" />
              {:else}
                <User size={18} class="text-text-muted" />
              {/if}
              <div>
                <div class="flex items-center gap-2">
                  <span class="text-text-bright font-semibold text-[0.9rem]">{pr.title}</span>
                  <span class="text-text-muted text-[0.75rem] font-mono">#{pr.number}</span>
                </div>
                <div class="text-[0.72rem] text-text-muted">
                  {$_("proposals.by")} <span class="font-medium text-text">{pr.user}</span> · {formatDate(pr.createdAt)}
                </div>
              </div>
            </div>

            <div class="flex items-center gap-2">
              <a
                href={pr.htmlUrl}
                target="_blank"
                rel="noreferrer"
                class="flex items-center gap-1 text-[0.75rem] text-text-muted hover:text-text-bright p-1"
                title={$_("proposals.viewOnGithub")}
              >
                <ExternalLink size={13} />
              </a>
              <button
                class="flex items-center gap-1 px-2.5 py-1.5 rounded border border-danger/40 bg-danger/10 hover:bg-danger/20 text-danger text-[0.76rem] font-medium cursor-pointer transition-colors"
                disabled={busy}
                on:click={() => openReject(pr)}
              >
                <X size={13} />
                {$_("proposals.reject")}
              </button>
              <button
                class="flex items-center gap-1.5 px-3 py-1.5 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.76rem] font-semibold cursor-pointer disabled:opacity-40 transition-colors shadow-sm"
                disabled={busy}
                on:click={() => openAccept(pr)}
              >
                <Check size={13} />
                {$_("proposals.accept")}
              </button>
            </div>
          </div>

          {#if pr.body}
            <p class="text-[0.78rem] text-text-muted bg-surface-2/60 p-2.5 rounded border border-border whitespace-pre-wrap leading-relaxed">
              {pr.body}
            </p>
          {/if}

          {#if pr.files.length > 0}
            <div class="flex flex-col gap-1.5 pt-1">
              <span class="text-[0.72rem] font-semibold text-text-muted uppercase tracking-wider">
                {$_("proposals.packagesIncluded")} ({pr.files.length})
              </span>
              <div class="flex flex-wrap gap-1.5">
                {#each pr.files as file}
                  <span class="inline-flex items-center gap-1 px-2 py-0.5 rounded bg-surface-2 border border-border font-mono text-[0.72rem] text-text">
                    <Package size={11} class="text-primary" />
                    {file.filename.split("/").pop()}
                  </span>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}
</div>

<!-- Accept Modal -->
{#if acceptTarget}
  <Modal
    variant="confirm"
    showIcon={false}
    title={$_("proposals.acceptTitle", { values: { number: acceptTarget.number } })}
    confirmLabel={$_("proposals.confirmAccept")}
    onClose={() => (acceptTarget = null)}
    onConfirm={confirmAccept}
  >
    <div class="flex flex-col gap-3.5 text-left">
      <p class="text-[0.8rem] text-text-muted">
        {$_("proposals.acceptExplanation", { values: { count: acceptTarget.files.length, repo: selectedRepo } })}
      </p>

      <label class="flex items-start gap-2.5 p-3 rounded bg-surface-2 border border-border cursor-pointer select-none">
        <input
          type="checkbox"
          class="mt-0.5 accent-primary cursor-pointer"
          bind:checked={cleanFirst}
        />
        <div class="flex flex-col gap-0.5">
          <span class="text-[0.82rem] font-semibold text-text-bright">
            {$_("proposals.cleanFirstLabel")}
          </span>
          <span class="text-[0.74rem] text-text-muted leading-relaxed">
            {$_("proposals.cleanFirstHint")}
          </span>
        </div>
      </label>
    </div>
  </Modal>
{/if}

<!-- Reject Modal -->
{#if rejectTarget}
  <Modal
    variant="danger"
    title={$_("proposals.rejectTitle", { values: { number: rejectTarget.number } })}
    confirmLabel={$_("proposals.confirmReject")}
    onClose={() => (rejectTarget = null)}
    onConfirm={confirmReject}
  >
    <div class="flex flex-col gap-3 text-left">
      <p class="text-[0.8rem] text-text-muted">
        {$_("proposals.rejectExplanation")}
      </p>
      <label class="flex flex-col gap-1">
        <span class="text-[0.78rem] text-text-muted">{$_("proposals.reasonLabel")}</span>
        <textarea
          class="w-full bg-surface-2 border border-border rounded px-3 py-2 text-[0.82rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-danger/60 min-h-[80px]"
          bind:value={rejectReason}
          placeholder={$_("proposals.reasonPlaceholder")}
        ></textarea>
      </label>
    </div>
  </Modal>
{/if}
