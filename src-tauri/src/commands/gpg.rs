// The GPG keys that sign the repositories: the secret keys of the keyring of
// the user (~/.gnupg). Everything goes through the gpg program, with a time
// limit, so a gpg that hangs never freezes the window.

use super::repos;
use super::run::{run, Run};
use serde::Serialize;
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri_plugin_dialog::DialogExt;

const LIST_TIMEOUT: Duration = Duration::from_secs(10);
// Making an RSA 4096 key needs randomness, and can take a while.
const CREATE_TIMEOUT: Duration = Duration::from_secs(180);
// The backup of a secret key waits for the user to type the passphrase.
const PASSPHRASE_TIMEOUT: Duration = Duration::from_secs(300);

fn gpg(args: &[&str], input: Option<&str>, timeout: Duration) -> Result<Run, String> {
    run("gpg", args, None, input, timeout)
}

fn gpg_error(run: &Run) -> String {
    run.error("gpg")
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SigningKey {
    pub fingerprint: String,
    /// The last 16 hex digits of the fingerprint, as 2.x stored it.
    pub key_id: String,
    pub name: String,
    pub email: String,
    /// Seconds since 1970.
    pub created: i64,
    pub expires: Option<i64>,
    pub expired: bool,
    pub revoked: bool,
    /// "RSA 4096", "Ed25519"...
    pub algorithm: String,
    /// Whether the secret key asks for a passphrase; None when gpg-agent does not say.
    pub protected: Option<bool>,
    /// The names of the repositories that this key signs.
    pub used_by: Vec<String>,
    #[serde(skip)]
    keygrip: String,
}

/// gpg escapes some bytes of the colon listing as \xNN.
fn unescape(field: &str) -> String {
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1] == b'x' {
            if let Ok(b) = u8::from_str_radix(&field[i + 2..i + 4], 16) {
                out.push(b);
                i += 4;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// "Name (comment) <mail@example.org>" -> ("Name (comment)", "mail@example.org").
fn split_user_id(uid: &str) -> (String, String) {
    match (uid.rfind('<'), uid.rfind('>')) {
        (Some(open), Some(close)) if open < close => {
            (uid[..open].trim().to_string(), uid[open + 1..close].trim().to_string())
        }
        _ => (uid.trim().to_string(), String::new()),
    }
}

fn algorithm_name(algo: &str, bits: &str, curve: &str) -> String {
    match algo {
        "1" | "2" | "3" => format!("RSA {bits}"),
        "17" => format!("DSA {bits}"),
        "22" if curve.eq_ignore_ascii_case("ed25519") => "Ed25519".to_string(),
        "22" => format!("EdDSA {curve}"),
        "19" => format!("ECDSA {curve}"),
        other => format!("{other} {bits}"),
    }
}

/// Reads `gpg --list-secret-keys --with-colons --with-keygrip`: one key per
/// "sec" record, with the fingerprint, keygrip and first user ID that follow
/// it (and not the ones of its subkeys).
fn parse_secret_keys(listing: &str) -> Vec<SigningKey> {
    let mut keys: Vec<SigningKey> = Vec::new();
    // True while the records belong to the primary key, false after an "ssb".
    let mut in_primary = false;
    for line in listing.lines() {
        let f: Vec<&str> = line.split(':').collect();
        match f.first().copied() {
            Some("sec") => {
                let field = |i: usize| f.get(i).copied().unwrap_or_default();
                let expires = field(6).parse::<i64>().ok().filter(|&e| e > 0);
                keys.push(SigningKey {
                    key_id: field(4).to_string(),
                    created: field(5).parse().unwrap_or(0),
                    expires,
                    expired: field(1) == "e",
                    revoked: field(1) == "r",
                    algorithm: algorithm_name(field(3), field(2), field(16)),
                    ..Default::default()
                });
                in_primary = true;
            }
            Some("ssb") => in_primary = false,
            Some("fpr") if in_primary => {
                if let Some(k) = keys.last_mut() {
                    if k.fingerprint.is_empty() {
                        k.fingerprint = f.get(9).copied().unwrap_or_default().to_string();
                    }
                }
            }
            Some("grp") if in_primary => {
                if let Some(k) = keys.last_mut() {
                    if k.keygrip.is_empty() {
                        k.keygrip = f.get(9).copied().unwrap_or_default().to_string();
                    }
                }
            }
            Some("uid") => {
                if let Some(k) = keys.last_mut() {
                    if k.name.is_empty() && k.email.is_empty() {
                        let (name, email) = split_user_id(&unescape(f.get(9).copied().unwrap_or_default()));
                        k.name = name;
                        k.email = email;
                    }
                }
            }
            _ => {}
        }
    }
    keys
}

/// For each keygrip, whether gpg-agent keeps its secret key under a passphrase.
/// A line of `KEYINFO --list` is "S KEYINFO <grip> <type> <serial> <id>
/// <cached> <protection> ...", where the protection is P (passphrase) or C (none).
fn protections() -> std::collections::HashMap<String, bool> {
    let mut map = std::collections::HashMap::new();
    let Ok(out) = Command::new("gpg-connect-agent").args(["KEYINFO --list", "/bye"]).stdin(Stdio::null()).output() else {
        return map;
    };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() > 7 && f[0] == "S" && f[1] == "KEYINFO" {
            match f[7] {
                "P" => {
                    map.insert(f[2].to_string(), true);
                }
                "C" => {
                    map.insert(f[2].to_string(), false);
                }
                _ => {}
            }
        }
    }
    map
}

/// Whether `reference` (a long ID or a fingerprint, as a repository stores it)
/// names the key with this fingerprint.
fn names_key(reference: &str, fingerprint: &str) -> bool {
    let r = reference.trim().to_uppercase();
    !r.is_empty() && fingerprint.to_uppercase().ends_with(&r)
}

fn secret_keys() -> Result<Vec<SigningKey>, String> {
    let run = gpg(
        &["--batch", "--with-colons", "--fixed-list-mode", "--with-keygrip", "--list-secret-keys"],
        None,
        LIST_TIMEOUT,
    )?;
    // With no secret key at all, gpg answers with nothing and a success.
    if !run.ok && !run.stdout.trim().is_empty() {
        return Err(gpg_error(&run));
    }
    let mut keys = parse_secret_keys(&run.stdout);
    let protection = protections();
    let repos = repos::load();
    for k in keys.iter_mut() {
        k.protected = protection.get(&k.keygrip).copied();
        k.used_by = repos.iter().filter(|r| names_key(&r.gpg_key, &k.fingerprint)).map(|r| r.name.clone()).collect();
    }
    Ok(keys)
}

/// The secret key that a repository names, by long ID or fingerprint.
pub(crate) fn key_for(reference: &str) -> Result<SigningKey, String> {
    secret_keys()?
        .into_iter()
        .find(|k| names_key(reference, &k.fingerprint))
        .ok_or_else(|| "The key is not in the keyring".to_string())
}

/// The public key, as text, to place in a repository as public.key.
pub(crate) fn public_key_armored(fingerprint: &str) -> Result<String, String> {
    let run = gpg(&["--batch", "--armor", "--export", fingerprint], None, LIST_TIMEOUT)?;
    if run.ok && run.stdout.contains("BEGIN PGP PUBLIC KEY BLOCK") {
        Ok(run.stdout)
    } else {
        Err(gpg_error(&run))
    }
}

/// Only a full fingerprint of a key that is in the keyring is accepted, so a
/// command can never act on something else.
fn known_key(fingerprint: &str) -> Result<SigningKey, String> {
    let valid = fingerprint.len() >= 40 && fingerprint.chars().all(|c| c.is_ascii_hexdigit());
    if !valid {
        return Err("Not a fingerprint of a key".to_string());
    }
    secret_keys()?
        .into_iter()
        .find(|k| k.fingerprint.eq_ignore_ascii_case(fingerprint))
        .ok_or_else(|| "The key is not in the keyring".to_string())
}

#[tauri::command(async)]
pub fn list_signing_keys() -> Result<Vec<SigningKey>, String> {
    secret_keys()
}

/// Makes a new RSA 4096 key that signs and certifies, and returns its
/// fingerprint. `expiry` is "never", "1y" or "2y". With no passphrase the key
/// is kept without one.
#[tauri::command(async)]
pub fn create_signing_key(name: String, email: String, expiry: String, passphrase: Option<String>) -> Result<String, String> {
    let name = name.trim();
    let email = email.trim();
    if name.is_empty() || name.contains(['<', '>', '\n']) {
        return Err("The name is not valid".to_string());
    }
    if !email.contains('@') || email.contains(['<', '>', ' ', '\n']) {
        return Err("The email is not valid".to_string());
    }
    if !["never", "1y", "2y"].contains(&expiry.as_str()) {
        return Err("The expiry is not valid".to_string());
    }
    let passphrase = passphrase.unwrap_or_default();
    if passphrase.contains('\n') {
        return Err("The passphrase cannot have a line break".to_string());
    }
    let uid = format!("{name} <{email}>");
    // The passphrase goes through the standard input, never on the command line.
    let run = gpg(
        &[
            "--batch",
            "--status-fd",
            "1",
            "--pinentry-mode",
            "loopback",
            "--passphrase-fd",
            "0",
            "--quick-generate-key",
            &uid,
            "rsa4096",
            "sign,cert",
            &expiry,
        ],
        Some(&format!("{passphrase}\n")),
        CREATE_TIMEOUT,
    )?;
    let created = run
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("[GNUPG:] KEY_CREATED "))
        .and_then(|rest| rest.split_whitespace().nth(1))
        .map(str::to_string);
    match created {
        Some(fpr) if run.ok => Ok(fpr),
        _ => Err(gpg_error(&run)),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    /// Keys read from the file.
    pub read: u32,
    /// Secret keys that were added.
    pub secret_imported: u32,
    /// Secret keys that were already in the keyring.
    pub secret_unchanged: u32,
}

/// Imports the keys of a file. Only secret keys can sign, so the result says
/// how many of them came in.
#[tauri::command(async)]
pub fn import_signing_key(path: String) -> Result<ImportResult, String> {
    let run = gpg(&["--batch", "--status-fd", "1", "--import", &path], None, PASSPHRASE_TIMEOUT)?;
    // IMPORT_RES <count> <no_user_id> <imported> <imported_rsa> <unchanged> <n_uids>
    //   <n_subk> <n_sigs> <n_revoc> <sec_read> <sec_imported> <sec_dups> ...
    let res: Vec<u32> = run
        .stdout
        .lines()
        .find_map(|l| l.strip_prefix("[GNUPG:] IMPORT_RES "))
        .map(|rest| rest.split_whitespace().map(|n| n.parse().unwrap_or(0)).collect())
        .unwrap_or_default();
    if res.is_empty() {
        return Err(gpg_error(&run));
    }
    let at = |i: usize| res.get(i).copied().unwrap_or(0);
    Ok(ImportResult { read: at(0), secret_imported: at(10), secret_unchanged: at(11) })
}

/// Writes the public key to `path`, as text (.asc) or binary (.gpg), the form
/// that goes in /usr/share/keyrings of whoever uses the repository.
#[tauri::command(async)]
pub fn export_public_key(fingerprint: String, path: String, armor: bool) -> Result<(), String> {
    let key = known_key(&fingerprint)?;
    let mut args = vec!["--batch", "--yes"];
    if armor {
        args.push("--armor");
    }
    args.extend(["--output", &path, "--export", &key.fingerprint]);
    let run = gpg(&args, None, LIST_TIMEOUT)?;
    let written = std::fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false);
    if run.ok && written {
        Ok(())
    } else {
        Err(gpg_error(&run))
    }
}

/// Writes a backup of the secret key to `path`, readable only by the user.
/// gpg-agent asks for the passphrase in its own window when the key has one.
#[tauri::command(async)]
pub fn backup_secret_key(fingerprint: String, path: String) -> Result<(), String> {
    let key = known_key(&fingerprint)?;
    // No --batch: gpg-agent must be free to ask for the passphrase.
    let run = gpg(&["--yes", "--armor", "--output", &path, "--export-secret-keys", &key.fingerprint], None, PASSPHRASE_TIMEOUT)?;
    let written = std::fs::metadata(&path).map(|m| m.len() > 0).unwrap_or(false);
    if !(run.ok && written) {
        let _ = std::fs::remove_file(&path);
        return Err(gpg_error(&run));
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())
}

/// Deletes a key from the keyring. A key that signs a repository is refused.
#[tauri::command(async)]
pub fn delete_signing_key(fingerprint: String) -> Result<(), String> {
    let key = known_key(&fingerprint)?;
    if !key.used_by.is_empty() {
        return Err(format!("The key signs these repositories: {}", key.used_by.join(", ")));
    }
    let run = gpg(&["--batch", "--yes", "--delete-secret-and-public-key", &key.fingerprint], None, LIST_TIMEOUT)?;
    if run.ok {
        Ok(())
    } else {
        Err(gpg_error(&run))
    }
}

#[tauri::command]
pub async fn pick_key_file(app: tauri::AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("GPG keys", &["asc", "gpg", "pgp", "key"])
            .blocking_pick_file()
            .and_then(|fp| fp.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .unwrap_or(None)
}

#[tauri::command]
#[allow(non_snake_case)]
pub async fn pick_save_path(app: tauri::AppHandle, suggestedName: String) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_file_name(&suggestedName)
            .blocking_save_file()
            .and_then(|fp| fp.into_path().ok())
            .map(|p| p.to_string_lossy().to_string())
    })
    .await
    .unwrap_or(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LISTING: &str = "\
sec:u:4096:1:3B23F6613DAD1C29:1745300000:::u:::scESC:::+:::23::0:
fpr:::::::::0123456789ABCDEF0123456789AB3B23F6613DAD1C29:
grp:::::::::AAAA1111AAAA1111AAAA1111AAAA1111AAAA1111:
uid:u::::1745300000::HASH::SoplosLinux tyron Repository <tyron@soplos.org>::::::::::0:
ssb:u:4096:1:1111222233334444:1745300000::::::e:::+:::23:
fpr:::::::::FFFF222233334444FFFF222233334444FFFF2222:
grp:::::::::BBBB2222BBBB2222BBBB2222BBBB2222BBBB2222:
sec:e:255:22:5992F2A3B0B77EF0:1746000000:1777536000::u:::scESC:::+::ed25519:23::0:
fpr:::::::::99999999999999999999999999995992F2A3B0B77EF0:
grp:::::::::CCCC3333CCCC3333CCCC3333CCCC3333CCCC3333:
uid:u::::1746000000::HASH::Kernels \\x3a team <k@soplos.org>::::::::::0:
";

    #[test]
    fn the_listing_gives_one_key_per_secret_key_with_its_own_fingerprint() {
        let keys = parse_secret_keys(LISTING);
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].fingerprint, "0123456789ABCDEF0123456789AB3B23F6613DAD1C29");
        assert_eq!(keys[0].keygrip, "AAAA1111AAAA1111AAAA1111AAAA1111AAAA1111");
        assert_eq!(keys[0].name, "SoplosLinux tyron Repository");
        assert_eq!(keys[0].email, "tyron@soplos.org");
        assert_eq!(keys[0].algorithm, "RSA 4096");
        assert_eq!(keys[0].expires, None);
        assert!(!keys[0].expired);
        assert_eq!(keys[1].algorithm, "Ed25519");
        assert!(keys[1].expired);
        assert_eq!(keys[1].expires, Some(1777536000));
        assert_eq!(keys[1].name, "Kernels : team");
    }

    #[test]
    fn a_long_id_of_2x_names_its_key() {
        let fpr = "0123456789ABCDEF0123456789AB3B23F6613DAD1C29";
        assert!(names_key("3B23F6613DAD1C29", fpr));
        assert!(names_key("3b23f6613dad1c29", fpr));
        assert!(names_key(fpr, fpr));
        assert!(!names_key("", fpr));
        assert!(!names_key("5992F2A3B0B77EF0", fpr));
    }

    #[test]
    fn a_user_id_without_email_keeps_the_name() {
        assert_eq!(split_user_id("Only Name"), ("Only Name".to_string(), String::new()));
    }
}
