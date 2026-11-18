import { writable } from "svelte/store";
import { api } from "./api";
import type { Repo, RepoList } from "./repos";

/**
 * The repositories and the one in use, shared by the selector of the sidebar
 * and the pages: every page that adds, publishes or cleans works on `active`.
 */
export const repos = writable<Repo[]>([]);
export const activeRepo = writable<string | null>(null);

export async function loadRepos(): Promise<void> {
  const list = await api.invoke<RepoList>("list_repos");
  repos.set(list.repos);
  activeRepo.set(list.active);
}

export async function chooseRepo(name: string): Promise<void> {
  await api.invoke("set_active_repo", { name });
  activeRepo.set(name);
}
