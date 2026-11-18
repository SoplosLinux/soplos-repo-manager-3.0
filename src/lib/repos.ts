// The shapes that src-tauri/src/commands/repos.rs returns.

export interface Repo {
  name: string;
  title: string;
  description: string;
  dir: string;
  gitUrl: string;
  branch: string;
  /** Long ID or fingerprint of the key that signs the repository. */
  gpgKey: string;
  listFilename: string;
  exists: boolean;
  managed: boolean;
  packages: number;
  /** The key that conf/distributions of reprepro signs with. */
  signWith: string;
  architectures: string[];
  /** The Date line of the Release file, when there is one. */
  released: string | null;
}

export interface RepoList {
  active: string | null;
  repos: Repo[];
}

export const ARCHITECTURES = ["amd64", "arm64", "i386", "armhf"];

/** Whether `reference` (a long ID or a fingerprint) names the key with this fingerprint. */
export function namesKey(reference: string, fingerprint: string): boolean {
  const r = reference.trim().toUpperCase();
  return r !== "" && fingerprint.toUpperCase().endsWith(r);
}

/** Whether two references (IDs or fingerprints) can be the same key. */
export function sameKey(a: string, b: string): boolean {
  const x = a.trim().toUpperCase();
  const y = b.trim().toUpperCase();
  return x !== "" && y !== "" && (x.endsWith(y) || y.endsWith(x));
}
