import { writable, get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface Job {
  id: number;
  title: string;
  lines: string[];
  status: "running" | "done" | "failed" | "cancelled";
}

const MAX_LINES = 500;

export const jobs = writable<Job[]>([]);
export const isJobRunning = writable(false);

let initialized = false;

export async function cancelJob() {
  try {
    await invoke("cancel_current_job");
  } catch (err) {
    console.error("Error cancelling job:", err);
  }
}

export async function initJobs() {
  if (initialized) return;
  initialized = true;

  await listen<{ id: number; title: string }>("job-start", ({ payload }) => {
    isJobRunning.set(true);
    jobs.update((list) => [
      ...list.slice(-20),
      { id: payload.id, title: payload.title, lines: [], status: "running" },
    ]);
  });

  await listen<{ id: number; line: string }>("job-line", ({ payload }) => {
    jobs.update((list) =>
      list.map((j) => {
        if (j.id !== payload.id) return j;
        return {
          ...j,
          lines: [...j.lines, payload.line].slice(-MAX_LINES),
        };
      })
    );
  });

  await listen<{ id: number; success: boolean; cancelled?: boolean }>("job-done", ({ payload }) => {
    jobs.update((list) =>
      list.map((j) => {
        if (j.id !== payload.id) return j;
        const status = payload.cancelled ? "cancelled" : payload.success ? "done" : "failed";
        return {
          ...j,
          status,
        };
      })
    );
    const hasRunning = get(jobs).some((j) => j.status === "running");
    isJobRunning.set(hasRunning);
  });
}

// A step the backend marks as translatable (STEP_MARK in commands/run.rs):
// the record separator, then {"k": key, "v": values}. Anything else is the
// raw output of a tool and is shown as it is.
const STEP_MARK = "\u001e";
type Translate = (key: string, options?: { values?: Record<string, string> }) => string;

export function jobText(raw: string, t: Translate): string {
  if (!raw.startsWith(STEP_MARK)) return raw;
  try {
    const step = JSON.parse(raw.slice(1)) as { k: string; v: Record<string, string> };
    return t(`jobs.steps.${step.k}`, { values: step.v });
  } catch {
    return raw.slice(1);
  }
}
