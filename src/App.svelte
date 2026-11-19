<script lang="ts">
  import { setContext, onMount, type ComponentType } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getVersion } from "@tauri-apps/api/app";
  import { listen } from "@tauri-apps/api/event";
  import { _ } from "svelte-i18n";
  import { api } from "./lib/api";
  import { progress } from "./lib/progress";
  import { isJobRunning, initJobs } from "./lib/jobs";
  import Sidebar from "./components/Sidebar.svelte";
  import WindowControls from "./components/WindowControls.svelte";
  import AboutDialog from "./components/AboutDialog.svelte";
  import PageHero from "./components/PageHero.svelte";
  import JobBar from "./components/JobBar.svelte";
  import Check from "./views/Check.svelte";
  import Build from "./views/Build.svelte";
  import Keys from "./views/Keys.svelte";
  import Repos from "./views/Repos.svelte";
  import Packages from "./views/Packages.svelte";
  import Cleanup from "./views/Cleanup.svelte";
  import Proposals from "./views/Proposals.svelte";
  import SendView from "./views/Send.svelte";
  import Targets from "./views/Targets.svelte";
  import Accounts from "./views/Accounts.svelte";
  import {
    Database,
    Package,
    PackagePlus,
    ShieldCheck,
    GitPullRequest,
    Send,
    CloudUpload,
    CircleUser,
    KeyRound,
    Eraser,
  } from "lucide-svelte";

  // The pages follow the work on a repository: build a package, check it,
  // publish it. The team pages are where packages are sent for review and
  // where the administrator receives them.
  $: tabs = [
    {
      id: "repos",
      group: "repo",
      icon: Database,
      label: $_("app.tabs.repos.label"),
      title: $_("app.tabs.repos.title"),
    },
    {
      id: "packages",
      group: "repo",
      icon: Package,
      label: $_("app.tabs.packages.label"),
      title: $_("app.tabs.packages.title"),
    },
    {
      id: "build",
      group: "build",
      icon: PackagePlus,
      label: $_("app.tabs.build.label"),
      title: $_("app.tabs.build.title"),
    },
    {
      id: "check",
      group: "build",
      icon: ShieldCheck,
      label: $_("app.tabs.check.label"),
      title: $_("app.tabs.check.title"),
    },
    {
      id: "proposals",
      group: "team",
      icon: GitPullRequest,
      label: $_("app.tabs.proposals.label"),
      title: $_("app.tabs.proposals.title"),
    },
    {
      id: "send",
      group: "team",
      icon: Send,
      label: $_("app.tabs.send.label"),
      title: $_("app.tabs.send.title"),
    },
    {
      id: "targets",
      group: "publish",
      icon: CloudUpload,
      label: $_("app.tabs.targets.label"),
      title: $_("app.tabs.targets.title"),
    },
    {
      id: "accounts",
      group: "publish",
      icon: CircleUser,
      label: $_("app.tabs.accounts.label"),
      title: $_("app.tabs.accounts.title"),
    },
    {
      id: "keys",
      group: "tools",
      icon: KeyRound,
      label: $_("app.tabs.keys.label"),
      title: $_("app.tabs.keys.title"),
    },
    {
      id: "cleanup",
      group: "tools",
      icon: Eraser,
      label: $_("app.tabs.cleanup.label"),
      title: $_("app.tabs.cleanup.title"),
    },
  ];

  const views: Record<string, ComponentType> = {
    repos: Repos,
    packages: Packages,
    build: Build,
    check: Check,
    proposals: Proposals,
    send: SendView,
    targets: Targets,
    accounts: Accounts,
    keys: Keys,
    cleanup: Cleanup,
  };

  let active = "repos";
  // The open proposals of the team, next to the Proposals page.
  let pendingProposals = 0;
  $: badges = { proposals: pendingProposals };

  async function readPending() {
    try {
      pendingProposals = await api.invoke<number>("pending_proposals");
    } catch {
      pendingProposals = 0;
    }
  }
  let appVersion = "";
  let envInfo: {
    desktopEnvironment: string;
    displayProtocol: string;
    edition: string;
  } | null = null;

  // With `decorations` off in tauri.conf.json (and `transparent` on) the system
  // draws no frame: the window is one card with rounded corners and the buttons
  // of the window float at its top right. With `decorations` on, nothing of
  // this shows, so that setting is the only switch between the two.
  let frameless = false;
  let maximized = false;

  onMount(() => {
    // Initialize job event listeners as early as possible so no events are lost.
    initJobs();
    let unlisten: (() => void) | undefined;
    let unlistenView: (() => void) | undefined;
    // GitHub is asked every five minutes while the window is open.
    readPending();
    const pendingTimer = setInterval(readPending, 5 * 60 * 1000);
    (async () => {
      try {
        // The button of the notification opens the program on Proposals.
        if (await api.invoke<boolean>("take_start_on_proposals")) select("proposals");
        unlistenView = await listen<string>("show-view", (event) => {
          if (views[event.payload]) select(event.payload);
          readPending();
        });
      } catch {
        // Outside the program: nothing to listen to.
      }
      try {
        appVersion = await getVersion();
      } catch {
        appVersion = "";
      }
      try {
        envInfo = await api.invoke("get_environment_info");
      } catch {
        envInfo = null;
      }
      try {
        const win = getCurrentWindow();
        frameless = !(await win.isDecorated());
        if (frameless) {
          maximized = await win.isMaximized();
          unlisten = await win.onResized(async () => {
            maximized = await win.isMaximized();
          });
        }
      } catch {
        frameless = false;
      }
    })();
    return () => {
      unlisten?.();
      unlistenView?.();
      clearInterval(pendingTimer);
    };
  });

  function select(id: string) {
    active = id;
    document.getElementById("main-scroll")?.scrollTo({ top: 0 });
  }

  setContext("navigate", select);
  setContext("openAbout", () => (aboutOpen = true));
  setContext("environment", () => envInfo);

  $: ActiveView = views[active];
  $: activeTab = tabs.find((t) => t.id === active)!;

  // F1 -> About, Ctrl+1..9 -> pages, Ctrl+Tab -> next page, Ctrl+Q -> quit.
  let aboutOpen = false;

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "F1") {
      e.preventDefault();
      aboutOpen = true;
      return;
    }
    if (e.ctrlKey && (e.key === "q" || e.key === "Q")) {
      e.preventDefault();
      getCurrentWindow().close();
      return;
    }
    // Ctrl+Tab and Ctrl+Shift+Tab: the next and the previous page.
    if (e.ctrlKey && !e.altKey && e.key === "Tab") {
      e.preventDefault();
      const i = tabs.findIndex((t) => t.id === active);
      const next = (i + (e.shiftKey ? -1 : 1) + tabs.length) % tabs.length;
      select(tabs[next].id);
      return;
    }
    // Ctrl+1 to Ctrl+9: the pages, in the order of the sidebar.
    if (e.ctrlKey && !e.shiftKey && !e.altKey && /^[1-9]$/.test(e.key)) {
      const tab = tabs[Number(e.key) - 1];
      if (tab) {
        e.preventDefault();
        select(tab.id);
      }
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<div
  class="relative flex h-screen overflow-hidden bg-dark {frameless && !maximized
    ? 'window-card'
    : ''}"
>
  <Sidebar {tabs} {active} onSelect={select} {appVersion} {envInfo} {badges} />

  {#if frameless}
    <div class="absolute right-2 top-[5px] z-40">
      <WindowControls {maximized} />
    </div>
  {/if}

  <div class="flex-1 flex flex-col overflow-hidden min-w-0">
    <!-- No page has a top bar: the strip over the content holds the window
         buttons and moves the window. -->
    {#if frameless}
      <div class="h-7 shrink-0" data-tauri-drag-region></div>
    {/if}
    <!-- The banner of the page stays where it is while the page scrolls under it. -->
    <div class="shrink-0 px-7 {frameless ? '' : 'pt-7'}">
      <PageHero title={activeTab.title} icon={activeTab.icon} />
    </div>
    <div
      id="main-scroll"
      class="content-scroll flex-1 overflow-y-auto px-7 pb-7"
    >
      <svelte:component this={ActiveView} />
    </div>

    <JobBar />

    <div
      class="h-1.5 bg-surface-2 overflow-hidden shrink-0"
      class:invisible={$progress === null && !$isJobRunning}
    >
      {#if typeof $progress === "number"}
        <div
          class="h-full bg-primary transition-[width]"
          style="width: {$progress * 100}%"
        ></div>
      {:else if $progress === "busy" || $isJobRunning}
        <div class="h-full w-1/3 bg-primary progress-slide"></div>
      {/if}
    </div>
  </div>

  {#if aboutOpen}
    <AboutDialog onClose={() => (aboutOpen = false)} />
  {/if}
</div>
