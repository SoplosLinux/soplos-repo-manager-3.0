// The shapes that src-tauri/src/commands/packages.rs returns.

export interface RepoPackage {
  name: string;
  version: string;
  architecture: string;
  size: number;
  summary: string;
}

export interface PackageList {
  repo: string;
  architectures: string[];
  packages: RepoPackage[];
}

export type CandidateStatus = "new" | "upgrade" | "same" | "older" | "duplicate" | "wrongArch" | "noSection" | "invalid";

export interface Candidate {
  path: string;
  file: string;
  group: string;
  name: string;
  version: string;
  architecture: string;
  size: number;
  status: CandidateStatus;
  current: string | null;
}

/** What happened in one repository: added, published, or why not. */
export interface RepoResult {
  repo: string;
  added: number;
  skipped: number;
  failed: { file: string; error: string }[];
  published: boolean;
  /** "no-git" when the repository has no Git address, else the error of the push. */
  publishError: string | null;
  error: string | null;
}

export interface AddProgress {
  repo: string;
  stage: "adding" | "store" | "signing" | "publishing";
  done: number;
  total: number;
  file: string;
}

export interface PublishState {
  name: string;
  configured: boolean;
  hasHistory: boolean;
}

/** Only these are added; the rest are shown with the reason they are left out. */
export function addable(c: Candidate): boolean {
  return c.status === "new" || c.status === "upgrade";
}

export function formatSize(bytes: number, locale: string): string {
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit >= 2 ? 1 : 0;
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: digits }).format(value)} ${units[unit]}`;
}

// Packages handed from Check to Packages: the review of Packages opens with them.
import { writable } from "svelte/store";
export const queuedPaths = writable<string[]>([]);

// Mirrors src-tauri/src/commands/check.rs.
export type CheckLevel = "ok" | "warning" | "error";
/** "info" never decides the result of a package (lintian). */
export type FindingLevel = CheckLevel | "info";

export interface Finding {
  code: string;
  level: FindingLevel;
  values: Record<string, string>;
}

export interface DebCheck {
  path: string;
  file: string;
  group: string;
  name: string;
  version: string;
  architecture: string;
  size: number;
  depends: string;
  level: CheckLevel;
  findings: Finding[];
  /** null when lintian is not installed. */
  lintian: string[] | null;
}
