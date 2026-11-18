import { writable } from "svelte/store";
import { api } from "./api";

export interface Target {
  id: string;
  name: string;
  description: string;
  gitUrl: string;
  branch: string;
}

export const targets = writable<Target[]>([]);
export const selectedSendTarget = writable<string>("");

export async function loadTargets(): Promise<Target[]> {
  try {
    const list = await api.invoke<Target[]>("list_targets");
    targets.set(list);
    return list;
  } catch (err) {
    console.error("Failed to load targets", err);
    return [];
  }
}
