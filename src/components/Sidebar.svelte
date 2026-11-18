<script lang="ts">
  import type { ComponentType } from "svelte";
  import { getContext, onMount } from "svelte";
  import { theme, toggleTheme } from "../lib/theme";
  import { repos, activeRepo, loadRepos, chooseRepo } from "../lib/repoStore";
  import Select from "./Select.svelte";
  import { Sun, Moon, Globe, ChevronDown, Database } from "lucide-svelte";
  import { _, locale } from "svelte-i18n";
  import { SUPPORTED_LOCALES, LOCALE_NAMES, setLocale, type SupportedLocale } from "../i18n";

  // The sections that are open, kept between runs. All of them start folded.
  const FOLDED_KEY = "soplos-repo-manager-open-sections";
  function readFolded(): Record<string, boolean> {
    try {
      return JSON.parse(localStorage.getItem(FOLDED_KEY) ?? "{}") ?? {};
    } catch {
      return {};
    }
  }
  let opened: Record<string, boolean> = readFolded();
  function toggle(group: string) {
    opened = { ...opened, [group]: !opened[group] };
    try {
      localStorage.setItem(FOLDED_KEY, JSON.stringify(opened));
    } catch {
      // Blocked storage: the sections still fold for this session.
    }
  }

  onMount(() => {
    loadRepos().catch(() => {
      // The page of repositories shows the error when it is opened.
    });
  });

  async function onRepoChange(name: string) {
    if (!name) return;
    try {
      await chooseRepo(name);
    } catch {
      // Not saved: the list is read again, so the selector shows the repository still in use.
      await loadRepos().catch(() => {});
    }
  }

  function onLanguageChange(e: Event) {
    setLocale((e.currentTarget as HTMLSelectElement).value as SupportedLocale);
  }

  export let tabs: { id: string; group: string; icon: ComponentType; label: string }[];
  export let active: string;
  export let onSelect: (id: string) => void;
  export let appVersion: string = "";
  export let envInfo: { desktopEnvironment: string; displayProtocol: string } | null = null;

  const openAbout = getContext<() => void>("openAbout");

  const DE_LABELS: Record<string, string> = {
    gnome: "GNOME",
    kde: "Plasma",
    xfce: "Xfce",
    unknown: "",
  };
  const PROTO_LABELS: Record<string, string> = {
    wayland: "Wayland",
    x11: "X11",
    unknown: "",
  };

  $: deLabel = envInfo ? (DE_LABELS[envInfo.desktopEnvironment] ?? envInfo.desktopEnvironment) : "";
  $: protoLabel = envInfo ? (PROTO_LABELS[envInfo.displayProtocol] ?? "") : "";
  $: envLabel = [deLabel, protoLabel].filter(Boolean).join(" · ");
  // "3.0.0-1-beta" -> "3.0.0-beta": the Debian revision in the middle is not shown.
  $: cleanVersion = appVersion.replace(/-\d+-/, "-");
  $: footerLabel = [envLabel, cleanVersion].filter(Boolean).join(" · ") || "Soplos Repo Manager";
</script>

<aside class="w-[230px] shrink-0 flex flex-col pt-[18px] px-[10px] pb-2 bg-panel border-r border-border" data-tauri-drag-region>
  <!-- With no frame of the system, the top of the sidebar moves the window. -->
  <div class="flex items-center gap-2 px-3 pb-[22px] font-bold text-[0.98rem] text-text-bright" data-tauri-drag-region="deep">
    <img src="/assets/icons/soplos-logo.png" alt="Soplos" class="w-[28px] h-[28px] object-contain shrink-0" />
    <span class="flex flex-col leading-tight min-w-0">
      <span class="whitespace-nowrap">Soplos</span>
      <span class="whitespace-nowrap">Repo Manager</span>
    </span>
  </div>

  <!-- The repository in use: every page that adds, publishes or cleans works on it. -->
  <div class="flex flex-col gap-1 px-1 pb-4">
    <span class="flex items-center gap-1.5 px-2 text-[0.72rem] font-semibold tracking-[0.08em] uppercase text-text-muted/70">
      <Database size={13} class="shrink-0" />
      {$_("sidebar.repository")}
    </span>
    <Select
      value={$activeRepo ?? ""}
      label={$_("sidebar.repository")}
      options={$repos.length ? $repos.map((r) => r.name) : [{ value: "", label: $_("sidebar.noRepository") }]}
      on:change={(e) => onRepoChange(e.detail)}
    />
  </div>

  <!-- The pages are in groups, and the menu scrolls when the window is short. -->
  <!-- The scroll bar gets its own room, so it is not drawn over the buttons. -->
  <nav class="content-scroll flex flex-col gap-[2px] flex-1 min-h-0 overflow-y-auto pr-3 [scrollbar-gutter:stable]">
    {#each tabs as tab, i}
      {#if i === 0 || tabs[i - 1].group !== tab.group}
        <!-- A section folds and unfolds; what was chosen is remembered. -->
        <button
          class="flex items-center justify-between gap-2 px-3.5 pt-2.5 pb-1 text-left text-[0.72rem] font-semibold tracking-[0.08em] uppercase hover:text-text-bright
            {tabs.some((t) => t.group === tab.group && t.id === active) ? 'text-primary' : 'text-text-muted/70'}"
          aria-expanded={!!opened[tab.group]}
          on:click={() => toggle(tab.group)}
        >
          <span>{$_(`app.groups.${tab.group}`)}</span>
          <ChevronDown size={14} class="shrink-0 transition-transform {opened[tab.group] ? '' : '-rotate-90'}" />
        </button>
      {/if}
      <!-- A folded section shows nothing; the one with the open page has its name in color. -->
      {#if opened[tab.group]}
        <button
          title={i < 9 ? `${tab.label} (Ctrl+${i + 1})` : tab.label}
          class="flex items-center gap-3 px-3.5 py-2 rounded text-[0.82rem] text-left transition-colors
            {active === tab.id
            ? 'bg-primary/15 text-primary font-semibold'
            : 'text-text-muted hover:bg-surface hover:text-text-bright'}"
          on:click={() => onSelect(tab.id)}
        >
          <svelte:component this={tab.icon} size={16} class="shrink-0" />
          <span class="truncate">{tab.label}</span>
        </button>
      {/if}
    {/each}
  </nav>

  <div class="mt-3 flex flex-col gap-2.5">
    <div class="flex items-center gap-1.5 px-1">
      <Globe size={13} class="text-text-muted shrink-0" />
      <select
        class="appearance-none flex-1 min-w-0 bg-surface-2 border border-border rounded px-1.5 py-1 text-[0.68rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60"
        value={$locale}
        title={$_("sidebar.interfaceLanguage")}
        on:change={onLanguageChange}
      >
        {#each SUPPORTED_LOCALES as code}
          <option value={code}>{LOCALE_NAMES[code]}</option>
        {/each}
      </select>
    </div>
    <div class="pt-2.5 border-t border-border flex items-center gap-2 px-1">
      <button
        class="p-1 rounded text-text-muted hover:bg-surface hover:text-text-bright shrink-0"
        on:click={toggleTheme}
        title={$theme === "dark" ? $_("sidebar.switchToLight") : $_("sidebar.switchToDark")}
        aria-label={$theme === "dark" ? $_("sidebar.switchToLight") : $_("sidebar.switchToDark")}
      >
        <svelte:component this={$theme === "dark" ? Moon : Sun} size={14} />
      </button>
      <button
        class="text-[0.62rem] text-text-muted hover:text-text-bright hover:underline"
        on:click={openAbout}
        title={$_("sidebar.aboutTooltip")}
      >
        {footerLabel}
      </button>
    </div>
  </div>
</aside>
