<script lang="ts">
  import { onMount, getContext } from "svelte";
  import { _ } from "svelte-i18n";
  import {
    CircleUser,
    KeyRound,
    CheckCircle,
    AlertCircle,
    ExternalLink,
    RefreshCw,
    Trash2,
    Eye,
    EyeOff,
    Check,
    Send,
    ShieldCheck,
    Plus,
    Star,
  } from "lucide-svelte";
  import { api } from "../lib/api";
  import { progress } from "../lib/progress";
  import Modal from "../components/Modal.svelte";

  const navigate = getContext<(id: string) => void>("navigate");

  interface AccountInfo {
    id: string;
    label: string;
    maskedToken: string;
    isActive: boolean;
  }

  interface GitHubUser {
    login: string;
    name: string | null;
    avatarUrl: string | null;
    htmlUrl: string | null;
    publicRepos: number | null;
  }

  let accounts: AccountInfo[] = [];
  let userCache: Record<string, GitHubUser | null> = {};
  let loading = true;
  let busy = false;
  let errorMessage: string | null = null;
  let successNotice: string | null = null;

  // Add account modal
  let showAddModal = false;
  let newLabel = "";
  let newToken = "";
  let showNewToken = false;
  let addBusy = false;
  let addError: string | null = null;
  // Tokens live only in the keyring of the system: with none, no account can be added.
  let keyring = true;

  // The backend answers NO_KEYRING when there is no Secret Service.
  const errorText = (err: unknown) => (String(err) === "NO_KEYRING" ? $_("accounts.noKeyringTitle") : String(err));

  async function loadAccounts() {
    loading = true;
    errorMessage = null;
    try {
      accounts = await api.invoke<AccountInfo[]>("list_accounts");
      // Load GitHub profiles in background
      for (const acc of accounts) {
        if (!(acc.id in userCache)) {
          userCache[acc.id] = null;
          api.invoke<GitHubUser>("get_account_user", { id: acc.id })
            .then((u) => { userCache[acc.id] = u; userCache = { ...userCache }; })
            .catch(() => { userCache[acc.id] = null; userCache = { ...userCache }; });
        }
      }
    } catch (err) {
      errorMessage = String(err);
    } finally {
      loading = false;
    }
  }

  onMount(async () => {
    keyring = await api.invoke<boolean>("has_keyring").catch(() => false);
    await loadAccounts();
  });

  function openAddModal() {
    newLabel = "";
    newToken = "";
    showNewToken = false;
    addError = null;
    showAddModal = true;
  }

  async function addAccount() {
    if (!newToken.trim()) return;
    addBusy = true;
    addError = null;
    try {
      await api.invoke("add_account", {
        label: newLabel.trim() || "GitHub",
        token: newToken.trim(),
      });
      showAddModal = false;
      await loadAccounts();
      successNotice = $_("accounts.addedSuccess");
      setTimeout(() => (successNotice = null), 4000);
    } catch (err) {
      addError = errorText(err);
    } finally {
      addBusy = false;
    }
  }

  async function setActive(id: string) {
    if (accounts.find(a => a.id === id)?.isActive) return;
    busy = true;
    errorMessage = null;
    try {
      await api.invoke("set_active_account", { id });
      await loadAccounts();
    } catch (err) {
      errorMessage = String(err);
    } finally {
      busy = false;
    }
  }

  async function removeAccount(id: string) {
    if (!confirm($_("accounts.deleteConfirm"))) return;
    busy = true;
    errorMessage = null;
    try {
      await api.invoke("remove_account", { id });
      delete userCache[id];
      userCache = { ...userCache };
      await loadAccounts();
    } catch (err) {
      errorMessage = String(err);
    } finally {
      busy = false;
    }
  }

  // The result of Test connection, per account: green when GitHub answers
  // with the user, red with the reason when it does not.
  let tests: Record<string, { state: "testing" | "ok" | "error"; text: string }> = {};

  async function refreshUser(id: string) {
    tests = { ...tests, [id]: { state: "testing", text: $_("accounts.testing") } };
    try {
      const u = await api.invoke<GitHubUser>("get_account_user", { id });
      userCache = { ...userCache, [id]: u };
      tests = { ...tests, [id]: { state: "ok", text: $_("accounts.connectionOk", { values: { login: u.login } }) } };
    } catch (err) {
      tests = { ...tests, [id]: { state: "error", text: $_("accounts.connectionFailed", { values: { error: errorText(err) } }) } };
    }
  }

  function openGitHubTokenGenerator() {
    window.open("https://github.com/settings/tokens/new?scopes=repo", "_blank");
  }
</script>

<div class="flex flex-col gap-5 max-w-[760px]">
  <div class="flex items-start justify-between gap-4">
    <div>
      <h2 class="text-[0.95rem] font-bold text-text-bright mb-1">{$_("accounts.title")}</h2>
      <p class="text-[0.8rem] text-text-muted">{$_("accounts.subtitle")}</p>
    </div>
    <div class="flex items-center gap-2 shrink-0">
      {#if accounts.length > 0}
        <button
          class="flex items-center gap-1.5 px-3 py-1.5 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.8rem] font-semibold transition-colors"
          on:click={() => navigate?.("send")}
        >
          <Send size={14} />
          {$_("accounts.goToSend")}
        </button>
      {/if}
      <button
        class="flex items-center gap-1.5 px-3 py-1.5 rounded border border-border bg-surface-2 hover:bg-surface-3 text-text-bright text-[0.8rem] font-semibold transition-colors disabled:opacity-40 disabled:pointer-events-none"
        disabled={!keyring}
        on:click={openAddModal}
      >
        <Plus size={14} />
        {$_("accounts.addAccount")}
      </button>
    </div>
  </div>

  {#if !keyring}
    <div class="rounded-md border border-warning/40 bg-warning/10 p-3.5 flex items-start gap-2.5 text-[0.82rem]">
      <AlertCircle size={16} class="shrink-0 mt-0.5 text-warning" />
      <div class="flex flex-col gap-0.5">
        <span class="font-semibold text-text-bright">{$_("accounts.noKeyringTitle")}</span>
        <span class="text-text-muted">{$_("accounts.noKeyringDesc")}</span>
      </div>
    </div>
  {/if}

  {#if successNotice}
    <div class="rounded-md border border-success/30 bg-success/10 p-3.5 flex items-center gap-2.5 text-success text-[0.82rem]">
      <CheckCircle size={16} class="shrink-0" />
      <span>{successNotice}</span>
    </div>
  {/if}

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
  {:else if accounts.length === 0}
    <!-- Empty state -->
    <div class="rounded-lg border border-border bg-surface p-5 flex flex-col gap-4">
      <div class="flex items-center gap-2.5">
        <CircleUser size={18} class="text-primary shrink-0" />
        <span class="text-[0.86rem] font-semibold text-text-bright">GitHub</span>
      </div>
      <p class="text-[0.8rem] text-text-muted leading-relaxed">
        {$_("accounts.tokenExplanation")}
      </p>
      <div class="rounded-md bg-surface-2 p-3.5 border border-border/60 flex flex-col gap-2">
        <span class="text-[0.76rem] font-semibold text-text-bright">{$_("accounts.howToGetTokenTitle")}</span>
        <ol class="list-decimal list-inside text-[0.74rem] text-text-muted space-y-1">
          <li>{$_("accounts.step1")}</li>
          <li>{$_("accounts.step2")}</li>
          <li>{$_("accounts.step3")}</li>
        </ol>
        <div class="pt-1">
          <button
            class="inline-flex items-center gap-1.5 text-[0.75rem] font-semibold text-primary hover:underline"
            on:click={openGitHubTokenGenerator}
          >
            {$_("accounts.openTokenGenerator")}
            <ExternalLink size={12} />
          </button>
        </div>
      </div>
      <button
        class="self-start flex items-center gap-1.5 px-3.5 py-1.5 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.8rem] font-semibold transition-colors disabled:opacity-40 disabled:pointer-events-none"
        disabled={!keyring}
        on:click={openAddModal}
      >
        <Plus size={14} />
        {$_("accounts.addAccount")}
      </button>
    </div>
  {:else}
    <!-- Accounts list -->
    <div class="flex flex-col gap-3">
      {#each accounts as acc (acc.id)}
        {@const user = userCache[acc.id]}
        <div class="rounded-lg border {acc.isActive ? 'border-primary/50' : 'border-border'} bg-surface p-4 flex flex-col gap-3 transition-colors">
          <!-- Header row -->
          <div class="flex items-center justify-between gap-3">
            <div class="flex items-center gap-2">
              <CircleUser size={16} class="text-primary shrink-0" />
              <span class="text-[0.84rem] font-semibold text-text-bright">{acc.label}</span>
              {#if acc.isActive}
                <span class="px-2 py-0.5 rounded-full text-[0.68rem] font-semibold bg-primary/15 text-primary flex items-center gap-1">
                  <Star size={10} class="fill-current" />
                  {$_("accounts.activeAccount")}
                </span>
              {/if}
            </div>
            <div class="flex items-center gap-1.5">
              {#if !acc.isActive}
                <button
                  class="px-2.5 py-1 rounded border border-border bg-surface-2 hover:bg-surface-3 text-text-bright text-[0.74rem] flex items-center gap-1 transition-colors disabled:opacity-50"
                  disabled={busy}
                  on:click={() => setActive(acc.id)}
                >
                  <Star size={11} />
                  {$_("accounts.setActive")}
                </button>
              {/if}
              <button
                class="px-2.5 py-1 rounded border border-border bg-surface-2 hover:bg-surface-3 text-text-bright text-[0.74rem] flex items-center gap-1.5 transition-colors disabled:opacity-50"
                disabled={busy || tests[acc.id]?.state === "testing"}
                on:click={() => refreshUser(acc.id)}
              >
                <RefreshCw size={12} />
                {$_("accounts.testConnection")}
              </button>
              <button
                class="px-2.5 py-1 rounded border border-danger/30 hover:bg-danger/10 text-danger text-[0.74rem] flex items-center gap-1 transition-colors disabled:opacity-50"
                disabled={busy}
                on:click={() => removeAccount(acc.id)}
              >
                <Trash2 size={12} />
                {$_("accounts.disconnect")}
              </button>
            </div>
          </div>

          {#if tests[acc.id]}
            {@const test = tests[acc.id]}
            <div
              class="rounded-md border px-3 py-2 flex items-center gap-2 text-[0.78rem] {test.state === 'ok'
                ? 'border-success/30 bg-success/10 text-success'
                : test.state === 'error'
                  ? 'border-danger/30 bg-danger/10 text-danger'
                  : 'border-border bg-surface-2 text-text-muted'}"
            >
              {#if test.state === "ok"}<CheckCircle size={14} class="shrink-0" />{:else if test.state === "error"}<AlertCircle size={14} class="shrink-0" />{:else}<RefreshCw size={14} class="shrink-0" />{/if}
              <span class="min-w-0 break-words">{test.text}</span>
            </div>
          {/if}

          <!-- GitHub profile -->
          {#if user}
            <div class="flex items-center gap-3 p-3 rounded-lg bg-surface-2 border border-border/60">
              {#if user.avatarUrl}
                <img
                  src={user.avatarUrl}
                  alt={user.login}
                  class="rounded-full border border-border shrink-0 object-cover"
                  style="width:48px;height:48px;min-width:48px;max-width:48px;"
                />
              {:else}
                <div
                  class="rounded-full bg-surface-3 flex items-center justify-center text-text-muted shrink-0"
                  style="width:48px;height:48px;"
                >
                  <CircleUser size={24} />
                </div>
              {/if}
              <div class="flex flex-col gap-0.5 min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="font-bold text-text-bright text-[0.88rem] truncate">{user.name || user.login}</span>
                  <span class="text-[0.72rem] font-mono text-text-muted">@{user.login}</span>
                </div>
                <div class="flex items-center gap-3 text-[0.72rem] text-text-muted">
                  {#if user.publicRepos !== null && user.publicRepos !== undefined}
                    <span>{user.publicRepos} {$_("accounts.publicRepos")}</span>
                  {/if}
                  {#if user.htmlUrl}
                    <a href={user.htmlUrl} target="_blank" rel="noreferrer" class="text-primary hover:underline flex items-center gap-1">
                      GitHub Profile <ExternalLink size={10} />
                    </a>
                  {/if}
                </div>
                <div class="text-[0.70rem] font-mono text-text-muted/80 mt-0.5">
                  Token: <span class="bg-surface-3 px-1.5 py-0.5 rounded">{acc.maskedToken}</span>
                </div>
              </div>
            </div>
          {:else}
            <div class="flex items-center gap-2 px-3 py-2 rounded bg-surface-2 border border-border/40 text-[0.74rem] text-text-muted">
              <KeyRound size={13} class="shrink-0" />
              <span class="font-mono">{acc.maskedToken}</span>
            </div>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <!-- Developer Guidelines Card -->
  <div class="rounded-lg border border-border/80 bg-surface/60 p-4 flex items-start gap-3">
    <ShieldCheck size={20} class="text-primary shrink-0 mt-0.5" />
    <div class="text-[0.76rem] text-text-muted leading-relaxed">
      <strong class="text-text-bright block mb-0.5">{$_("accounts.developerInfoTitle")}</strong>
      {$_("accounts.developerInfoBody")}
    </div>
  </div>
</div>

<!-- Add Account Modal -->
{#if showAddModal}
  <Modal
    variant="confirm"
    showIcon={false}
    title={$_("accounts.addAccount")}
    confirmLabel={$_("accounts.saveAndConnect")}
    cancelLabel={$_("common.cancel")}
    confirmDisabled={addBusy || !newToken.trim()}
    onClose={() => (showAddModal = false)}
    onConfirm={addAccount}
  >
    <div class="flex flex-col gap-3.5">
      {#if addError}
        <div class="rounded border border-danger/30 bg-danger/10 p-2.5 text-danger text-[0.78rem]">
          {addError}
        </div>
      {/if}

      <div>
        <label for="acc-label" class="block text-[0.76rem] font-semibold text-text-bright mb-1">
          {$_("accounts.accountLabel")}
        </label>
        <input
          id="acc-label"
          class="w-full bg-surface-2 border border-border rounded px-3 py-1.5 text-[0.82rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60"
          placeholder={$_("accounts.accountPlaceholder")}
          bind:value={newLabel}
          disabled={addBusy}
        />
      </div>

      <div>
        <label for="acc-token" class="block text-[0.76rem] font-semibold text-text-bright mb-1">
          {$_("accounts.personalAccessToken")} *
        </label>
        <div class="relative">
          {#if showNewToken}
            <input
              id="acc-token"
              class="w-full bg-surface-2 border border-border rounded px-3 py-2 text-[0.82rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60 font-mono pr-9"
              type="text"
              bind:value={newToken}
              placeholder="ghp_xxxxxxxxxxxxxxxxxxxx"
              disabled={addBusy}
            />
          {:else}
            <input
              id="acc-token"
              class="w-full bg-surface-2 border border-border rounded px-3 py-2 text-[0.82rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60 font-mono pr-9"
              type="password"
              bind:value={newToken}
              placeholder="ghp_xxxxxxxxxxxxxxxxxxxx"
              disabled={addBusy}
            />
          {/if}
          <button
            type="button"
            class="absolute right-2.5 top-1/2 -translate-y-1/2 text-text-muted hover:text-text-bright p-0.5 rounded"
            on:click={() => (showNewToken = !showNewToken)}
          >
            {#if showNewToken}
              <EyeOff size={14} />
            {:else}
              <Eye size={14} />
            {/if}
          </button>
        </div>
      </div>

      <div class="rounded-md bg-surface-2 p-3 border border-border/60 flex flex-col gap-1.5">
        <span class="text-[0.74rem] font-semibold text-text-bright">{$_("accounts.howToGetTokenTitle")}</span>
        <ol class="list-decimal list-inside text-[0.72rem] text-text-muted space-y-0.5">
          <li>{$_("accounts.step1")}</li>
          <li>{$_("accounts.step2")}</li>
          <li>{$_("accounts.step3")}</li>
        </ol>
        <button
          class="self-start inline-flex items-center gap-1 text-[0.72rem] text-primary hover:underline mt-0.5"
          on:click={openGitHubTokenGenerator}
        >
          {$_("accounts.openTokenGenerator")} <ExternalLink size={11} />
        </button>
      </div>
    </div>
  </Modal>
{/if}
