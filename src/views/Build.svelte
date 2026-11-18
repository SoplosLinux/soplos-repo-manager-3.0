<script lang="ts">
  import { onMount, getContext } from "svelte";
  import { _ } from "svelte-i18n";
  import { FileCode, FolderOpen, FolderTree, ImagePlus, Hammer, PackagePlus, CheckCircle, AlertTriangle, XCircle, Info } from "lucide-svelte";
  import { api } from "../lib/api";
  import { progress } from "../lib/progress";
  import { activeRepo } from "../lib/repoStore";
  import { type DebCheck, queuedPaths } from "../lib/packages";
  import Select from "../components/Select.svelte";
  import Modal from "../components/Modal.svelte";

  const navigate = getContext<(id: string) => void>("navigate");

  // What the package is made from: one program, a program in a folder (to
  // /opt with a launcher), or a folder already laid out as the system.
  type Kind = "file" | "folder" | "tree";
  let kind: Kind = "file";
  let source = "";
  let entry = "";

  let name = "";
  let version = "";
  let architecture = "";
  let section = "utils";
  let priority = "optional";
  let maintainerName = "";
  let maintainerEmail = "";
  let homepage = "";
  let summary = "";
  let description = "";
  let depends = "";
  let recommends = "";
  let outputDir = "";

  // The menu entry, the icon and the metainfo, named by the same application id.
  let withDesktop = true;
  let appId = "";
  let displayName = "";
  let comment = "";
  let categories = new Set<string>(["Utility"]);
  let terminal = false;
  let icon = "";
  let license = "";

  let busy = false;
  let errorMessage: string | null = null;
  let result: { path: string; check: DebCheck } | null = null;
  let askOverwrite = false;

  let hostArch = "amd64";
  onMount(async () => {
    hostArch = await api.invoke<string>("host_architecture").catch(() => "amd64");
    architecture = hostArch;
    const m = await api.invoke<{ name: string; email: string }>("load_maintainer").catch(() => ({ name: "", email: "" }));
    maintainerName = m.name;
    maintainerEmail = m.email;
  });

  // Debian's sections and priorities, and the main categories of the menu specification.
  const SECTIONS = ["admin", "comm", "database", "devel", "doc", "editors", "education", "fonts", "games", "gnome", "graphics", "interpreters", "kde", "libs", "mail", "math", "metapackages", "misc", "net", "science", "shells", "sound", "text", "utils", "video", "web", "x11", "xfce"];
  const PRIORITIES = ["optional", "standard", "important", "required"];
  const CATEGORIES = ["AudioVideo", "Development", "Education", "Game", "Graphics", "Network", "Office", "Science", "Settings", "System", "Utility"];
  $: architectures = ["all", ...new Set([hostArch, "amd64", "arm64", "i386", "armhf", "riscv64", "ppc64el"])];

  /** A package name from a file name: "My Tool.sh" -> "my-tool". */
  const toPackageName = (text: string) =>
    text.replace(/\.[a-z0-9]+$/i, "").toLowerCase().replace(/[^a-z0-9+.-]+/g, "-").replace(/^-+|-+$/g, "");

  async function chooseSource() {
    const path = kind === "file" ? await api.invoke<string | null>("pick_file", { images: false, start: null }) : await api.invoke<string | null>("pick_folder");
    if (!path) return;
    source = path;
    entry = "";
    if (!name) name = toPackageName(path.split("/").pop() ?? "");
    if (!displayName) displayName = path.split("/").pop()?.replace(/\.[a-z0-9]+$/i, "") ?? "";
    if (!outputDir) outputDir = path.split("/").slice(0, -1).join("/");
  }

  async function chooseEntry() {
    const path = await api.invoke<string | null>("pick_file", { images: false, start: source });
    if (!path) return;
    // The entry is written relative to the folder of the program.
    entry = path.startsWith(source + "/") ? path.slice(source.length + 1) : path;
  }

  async function chooseIcon() {
    const path = await api.invoke<string | null>("pick_file", { images: true, start: null });
    if (path) icon = path;
  }

  async function chooseOutput() {
    const path = await api.invoke<string | null>("pick_folder");
    if (path) outputDir = path;
  }

  function toggleCategory(c: string) {
    categories.has(c) ? categories.delete(c) : categories.add(c);
    categories = categories;
  }

  const list = (text: string) => text.split(",").map((s) => s.trim()).filter(Boolean);

  $: ready =
    !!source &&
    (kind !== "folder" || !!entry) &&
    /^[a-z0-9][a-z0-9+.-]+$/.test(name) &&
    !!version.trim() &&
    !!summary.trim() &&
    !!maintainerName.trim() &&
    maintainerEmail.includes("@") &&
    !!outputDir &&
    (!withDesktop || (/^[A-Za-z][\w-]*(\.[A-Za-z][\w-]*)+$/.test(appId) && !!displayName.trim()));

  async function build(overwrite = false) {
    if (!ready || busy) return;
    busy = true;
    errorMessage = null;
    result = null;
    progress.set("busy");
    try {
      await api.invoke("save_maintainer", { name: maintainerName, email: maintainerEmail }).catch(() => {});
      result = await api.invoke<{ path: string; check: DebCheck }>("build_package", {
        spec: {
          kind,
          source,
          entry,
          name,
          version: version.trim(),
          architecture,
          section,
          priority,
          maintainerName,
          maintainerEmail,
          homepage,
          summary,
          description,
          depends: list(depends),
          recommends: list(recommends),
          desktop: withDesktop
            ? { appId, displayName, comment, categories: [...categories], terminal, icon, license }
            : null,
          outputDir,
          overwrite,
        },
      });
    } catch (error) {
      if (String(error) === "EXISTS") askOverwrite = true;
      else errorMessage = String(error);
    } finally {
      busy = false;
      progress.set(null);
    }
  }

  function sendToRepository() {
    if (!result) return;
    queuedPaths.set([result.path]);
    navigate?.("packages");
  }

  const KINDS: { value: Kind; icon: typeof FileCode }[] = [
    { value: "file", icon: FileCode },
    { value: "folder", icon: FolderOpen },
    { value: "tree", icon: FolderTree },
  ];
  const findingIcon = { ok: CheckCircle, warning: AlertTriangle, error: XCircle, info: Info };
  const findingTint = { ok: "text-success", warning: "text-warning", error: "text-danger", info: "text-text-muted" };
  const inputClass =
    "w-full bg-surface-2 border border-border rounded px-3 py-2 text-[0.82rem] text-text-bright outline-none focus-visible:ring-2 focus-visible:ring-primary/60";
  const fieldLabel = "text-[0.76rem] text-text-muted";
  const card = "rounded-lg border border-border bg-surface p-4 flex flex-col gap-3";
  const cardTitle = "text-[0.8rem] font-semibold uppercase tracking-wide text-text-muted";
  const smallButton =
    "flex items-center gap-1.5 px-3 py-2 rounded border border-border text-[0.8rem] text-text hover:bg-surface-2 hover:text-text-bright shrink-0 disabled:opacity-40";
</script>

<div class="flex flex-col gap-4 max-w-[820px]">
  <p class="text-[0.78rem] text-text-muted">{$_("build.intro")}</p>

  <!-- 1. The program -->
  <section class={card}>
    <h2 class={cardTitle}>{$_("build.sourceTitle")}</h2>
    <div class="grid gap-2 sm:grid-cols-3">
      {#each KINDS as { value, icon: Icon }}
        <button
          type="button"
          class="flex flex-col items-start gap-1 rounded-lg border p-3 text-left transition-colors {kind === value ? 'border-primary bg-primary/10' : 'border-border hover:border-primary/40'}"
          on:click={() => { kind = value; source = ""; entry = ""; }}
        >
          <span class="flex items-center gap-2 text-[0.84rem] font-semibold text-text-bright"><svelte:component this={Icon} size={15} class="text-primary" />{$_(`build.kinds.${value}`)}</span>
          <span class="text-[0.72rem] text-text-muted">{$_(`build.kinds.${value}Hint`)}</span>
        </button>
      {/each}
    </div>
    <div class="flex gap-2">
      <input class="{inputClass} font-mono" readonly value={source} placeholder={$_(kind === "file" ? "build.pickFile" : "build.pickFolder")} />
      <button class={smallButton} on:click={chooseSource}>{$_("build.choose")}</button>
    </div>
    {#if kind === "folder"}
      <div class="flex flex-col gap-1">
        <span class={fieldLabel}>{$_("build.entry")}</span>
        <div class="flex gap-2">
          <input class="{inputClass} font-mono" bind:value={entry} placeholder="bin/my-program" />
          <button class={smallButton} disabled={!source} on:click={chooseEntry}>{$_("build.choose")}</button>
        </div>
        <span class="text-[0.72rem] text-text-muted">{$_("build.entryHint", { values: { name: name || "…" } })}</span>
      </div>
    {/if}
  </section>

  <!-- 2. The package -->
  <section class={card}>
    <h2 class={cardTitle}>{$_("build.packageTitle")}</h2>
    <div class="grid gap-3 sm:grid-cols-2">
      <label class="flex flex-col gap-1">
        <span class={fieldLabel}>{$_("build.name")}</span>
        <input class="{inputClass} font-mono" bind:value={name} placeholder="my-program" />
      </label>
      <label class="flex flex-col gap-1">
        <span class={fieldLabel}>{$_("build.version")}</span>
        <input class="{inputClass} font-mono" bind:value={version} placeholder="1.0.0" />
      </label>
      <div class="flex flex-col gap-1">
        <span class={fieldLabel}>{$_("build.architecture")}</span>
        <Select bind:value={architecture} label={$_("build.architecture")} options={architectures} />
      </div>
      <div class="grid grid-cols-2 gap-3">
        <div class="flex flex-col gap-1">
          <span class={fieldLabel}>{$_("build.section")}</span>
          <Select bind:value={section} label={$_("build.section")} options={SECTIONS} />
        </div>
        <div class="flex flex-col gap-1">
          <span class={fieldLabel}>{$_("build.priority")}</span>
          <Select bind:value={priority} label={$_("build.priority")} options={PRIORITIES} />
        </div>
      </div>
      <label class="flex flex-col gap-1 sm:col-span-2">
        <span class={fieldLabel}>{$_("build.summary")}</span>
        <input class={inputClass} bind:value={summary} maxlength="80" placeholder={$_("build.summaryPlaceholder")} />
      </label>
      <label class="flex flex-col gap-1 sm:col-span-2">
        <span class={fieldLabel}>{$_("build.description")}</span>
        <textarea class="{inputClass} min-h-[90px]" bind:value={description} placeholder={$_("build.descriptionPlaceholder")}></textarea>
      </label>
      <label class="flex flex-col gap-1">
        <span class={fieldLabel}>{$_("build.depends")}</span>
        <input class="{inputClass} font-mono" bind:value={depends} placeholder="libgtk-3-0, curl (>= 7.0)" />
      </label>
      <label class="flex flex-col gap-1">
        <span class={fieldLabel}>{$_("build.recommends")}</span>
        <input class="{inputClass} font-mono" bind:value={recommends} />
      </label>
      <label class="flex flex-col gap-1 sm:col-span-2">
        <span class={fieldLabel}>{$_("build.homepage")}</span>
        <input class={inputClass} bind:value={homepage} placeholder="https://" />
      </label>
    </div>
  </section>

  <!-- 3. The maintainer -->
  <section class={card}>
    <h2 class={cardTitle}>{$_("build.maintainerTitle")}</h2>
    <div class="grid gap-3 sm:grid-cols-2">
      <label class="flex flex-col gap-1">
        <span class={fieldLabel}>{$_("build.maintainerName")}</span>
        <input class={inputClass} bind:value={maintainerName} />
      </label>
      <label class="flex flex-col gap-1">
        <span class={fieldLabel}>{$_("build.maintainerEmail")}</span>
        <input class={inputClass} type="email" bind:value={maintainerEmail} placeholder="name@example.org" />
      </label>
    </div>
    <span class="text-[0.72rem] text-text-muted">{$_("build.maintainerHint")}</span>
  </section>

  <!-- 4. The desktop -->
  <section class={card}>
    <label class="flex items-center gap-2 cursor-pointer select-none">
      <input type="checkbox" class="w-4 h-4 accent-primary" bind:checked={withDesktop} />
      <span class={cardTitle}>{$_("build.desktopTitle")}</span>
    </label>
    <span class="text-[0.74rem] text-text-muted">{$_("build.desktopHint")}</span>
    {#if withDesktop}
      <div class="grid gap-3 sm:grid-cols-2">
        <label class="flex flex-col gap-1">
          <span class={fieldLabel}>{$_("build.appId")}</span>
          <input class="{inputClass} font-mono" bind:value={appId} placeholder="org.example.MyProgram" />
        </label>
        <label class="flex flex-col gap-1">
          <span class={fieldLabel}>{$_("build.displayName")}</span>
          <input class={inputClass} bind:value={displayName} />
        </label>
        <label class="flex flex-col gap-1 sm:col-span-2">
          <span class={fieldLabel}>{$_("build.comment")}</span>
          <input class={inputClass} bind:value={comment} />
        </label>
        <div class="flex flex-col gap-1 sm:col-span-2">
          <span class={fieldLabel}>{$_("build.categories")}</span>
          <div class="flex flex-wrap gap-1.5">
            {#each CATEGORIES as c}
              <button
                type="button"
                class="px-2.5 py-1 rounded-full border text-[0.74rem] {categories.has(c) ? 'border-primary bg-primary/15 text-primary' : 'border-border text-text hover:border-primary/40'}"
                on:click={() => toggleCategory(c)}
              >
                {$_(`build.category.${c}`)}
              </button>
            {/each}
          </div>
        </div>
        <div class="flex flex-col gap-1">
          <span class={fieldLabel}>{$_("build.icon")}</span>
          <div class="flex gap-2">
            <input class="{inputClass} font-mono" readonly value={icon} placeholder="PNG, SVG" />
            <button class={smallButton} on:click={chooseIcon}><ImagePlus size={14} />{$_("build.choose")}</button>
          </div>
        </div>
        <label class="flex flex-col gap-1">
          <span class={fieldLabel}>{$_("build.license")}</span>
          <input class="{inputClass} font-mono" bind:value={license} placeholder="GPL-3.0-or-later" />
        </label>
        <label class="flex items-center gap-2 text-[0.8rem] text-text cursor-pointer select-none sm:col-span-2">
          <input type="checkbox" class="w-4 h-4 accent-primary" bind:checked={terminal} />
          {$_("build.terminal")}
        </label>
      </div>
    {/if}
  </section>

  <!-- 5. Where it goes -->
  <section class={card}>
    <h2 class={cardTitle}>{$_("build.outputTitle")}</h2>
    <div class="flex gap-2">
      <input class="{inputClass} font-mono" readonly value={outputDir} placeholder={$_("build.pickFolder")} />
      <button class={smallButton} on:click={chooseOutput}>{$_("build.choose")}</button>
    </div>
  </section>

  {#if errorMessage}
    <p class="rounded-md border border-danger/30 bg-danger/10 p-3 text-[0.8rem] text-danger">{errorMessage}</p>
  {/if}

  <div class="flex items-center gap-3">
    <button
      class="flex items-center gap-1.5 px-4 py-2 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.84rem] font-semibold disabled:opacity-40 disabled:pointer-events-none"
      disabled={!ready || busy}
      on:click={() => build(false)}
    >
      <Hammer size={15} />
      {busy ? $_("build.building") : $_("build.build")}
    </button>
    {#if !ready}<span class="text-[0.74rem] text-text-muted">{$_("build.missing")}</span>{/if}
  </div>

  {#if result}
    {@const check = result.check}
    <section class="rounded-lg border p-4 flex flex-col gap-2.5 {check.level === 'error' ? 'border-danger/40' : check.level === 'warning' ? 'border-warning/40' : 'border-success/40'} bg-surface">
      <div class="flex items-start gap-3">
        <svelte:component this={findingIcon[check.level]} size={20} class="shrink-0 mt-0.5 {findingTint[check.level]}" />
        <div class="min-w-0 flex-1">
          <span class="text-text-bright font-semibold text-[0.9rem]">{$_("build.done")}</span>
          <p class="font-mono text-[0.74rem] text-text break-all">{result.path}</p>
        </div>
      </div>
      <ul class="flex flex-col gap-1 pl-8">
        {#each check.findings as f}
          <li class="flex items-start gap-2 text-[0.78rem]">
            <svelte:component this={findingIcon[f.level]} size={14} class="shrink-0 mt-0.5 {findingTint[f.level]}" />
            <span class="text-text">{$_(`check.items.${f.code}`, { values: f.values })}</span>
          </li>
        {/each}
      </ul>
      <div class="flex flex-wrap gap-2 pl-8">
        <button
          class="flex items-center gap-1.5 px-3 py-1.5 rounded bg-primary hover:bg-primary-hover text-text-bright text-[0.78rem] font-semibold disabled:opacity-40 disabled:pointer-events-none"
          disabled={check.level === "error" || !$activeRepo}
          title={$activeRepo ? "" : $_("check.noRepo")}
          on:click={sendToRepository}
        >
          <PackagePlus size={14} />
          {$_("check.sendToRepo", { values: { count: 1, repo: $activeRepo ?? "" } })}
        </button>
        <button class={smallButton} on:click={() => api.invoke("open_folder", { path: outputDir }).catch((e) => (errorMessage = String(e)))}>
          <FolderOpen size={14} />
          {$_("build.openFolder")}
        </button>
      </div>
    </section>
  {/if}
</div>

{#if askOverwrite}
  <Modal
    variant="warning"
    title={$_("build.existsTitle")}
    confirmLabel={$_("build.replace")}
    onClose={() => (askOverwrite = false)}
    onConfirm={() => { askOverwrite = false; build(true); }}
  >
    <p class="text-[0.82rem] text-text">{$_("build.existsText", { values: { folder: outputDir } })}</p>
  </Modal>
{/if}
