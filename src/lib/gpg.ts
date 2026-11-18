// The shapes that src-tauri/src/commands/gpg.rs returns.

export interface SigningKey {
  fingerprint: string;
  keyId: string;
  name: string;
  email: string;
  /** Seconds since 1970. */
  created: number;
  expires: number | null;
  expired: boolean;
  revoked: boolean;
  algorithm: string;
  /** null when gpg-agent does not say. */
  protected: boolean | null;
  usedBy: string[];
}

export interface ImportResult {
  read: number;
  secretImported: number;
  secretUnchanged: number;
}

/** "0123456789ABCDEF..." -> "0123 4567 89AB CDEF ...", as gpg shows it. */
export function groupFingerprint(fpr: string): string {
  return fpr.replace(/(.{4})/g, "$1 ").trim();
}

const DAY = 24 * 60 * 60;

/** A key that expires within 30 days gets a warning. */
export function expiresSoon(key: SigningKey, now = Date.now() / 1000): boolean {
  return !key.expired && key.expires !== null && key.expires - now < 30 * DAY;
}
