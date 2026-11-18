<script lang="ts">
  import { _ } from "svelte-i18n";
  import { X, CheckCircle, AlertTriangle, Ban } from "lucide-svelte";
  import { jobs, cancelJob, jobText } from "../lib/jobs";

  let expanded = false;
  let dismissedId: number | null = null;

  $: closedAt = dismissedId === null ? -1 : $jobs.findIndex((j) => j.id === dismissedId);
  $: session = $jobs.slice(closedAt + 1);
  $: job = [...session].reverse().find((j) => j.status === "running") ?? session[session.length - 1];
  $: visible = !!job;
  $: tr = (raw: string) => jobText(raw, $_);
  $: lastLine = tr(job?.lines[job.lines.length - 1] ?? "");
  $: log = session.map((j) => `── ${tr(j.title)} ──\n${j.lines.map(tr).join("\n")}`).join("\n\n");

  function stickToBottom(node: HTMLElement, _length: number) {
    const scroll = () => (node.scrollTop = node.scrollHeight);
    scroll();
    return { update: scroll };
  }

  const STATUS_CLASS = {
    running: "text-primary",
    done: "text-success",
    failed: "text-danger",
    cancelled: "text-text-muted",
  };
</script>

{#if visible && job}
  <div class="border-t border-border bg-surface-1 shrink-0 z-30 transition-all">
    <div class="flex items-center gap-3 px-4 py-2 text-[0.78rem]">
      <span class="font-semibold text-text-bright shrink-0">{tr(job.title)}</span>
      <span class="shrink-0 font-medium {STATUS_CLASS[job.status]} flex items-center gap-1">
        {#if job.status === "done"}
          <CheckCircle size={13} class="text-success" />
        {:else if job.status === "failed"}
          <AlertTriangle size={13} class="text-danger" />
        {:else if job.status === "cancelled"}
          <Ban size={13} class="text-text-muted" />
        {/if}
        {job.status === "running" ? "" : $_(`jobs.${job.status}`)}
      </span>
      <span class="flex-1 min-w-0 truncate text-text-muted font-mono text-[0.72rem]">{lastLine}</span>
      
      {#if job.status === "running"}
        <button
          type="button"
          class="px-2.5 py-1 rounded border border-danger/40 bg-danger/10 hover:bg-danger/20 text-danger text-xs font-medium shrink-0 cursor-pointer transition-colors"
          on:click={cancelJob}
        >
          {$_("common.cancel")}
        </button>
      {/if}

      <button
        type="button"
        class="text-primary hover:underline shrink-0 text-xs font-medium cursor-pointer"
        on:click={() => (expanded = !expanded)}
      >
        {expanded ? $_("jobs.hideLog") : $_("jobs.showLog")}
      </button>

      {#if job.status !== "running"}
        <button
          type="button"
          class="p-1 rounded text-text-muted hover:bg-surface-2 hover:text-text-bright shrink-0 cursor-pointer"
          title={$_("jobs.close")}
          on:click={() => {
            dismissedId = session[session.length - 1].id;
            expanded = false;
          }}
        >
          <X size={14} />
        </button>
      {/if}
    </div>
    {#if expanded}
      <pre
        use:stickToBottom={log.length}
        class="content-scroll max-h-[220px] overflow-auto mx-4 mb-2.5 p-3 rounded bg-dark border border-border text-[0.72rem] text-text font-mono whitespace-pre-wrap select-text leading-relaxed shadow-inner">{log}</pre>
    {/if}
  </div>
{/if}
