// SPDX-License-Identifier: GPL-3.0-or-later

//! the Roblox session jar, kept in the login keyring.
//!
//! the client only understands its own TSV file, so Roseline restores the
//! jar from libsecret before a launch and puts it back when the client exits.
//! the plaintext file is removed once it is safely in the keyring.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};

use crate::config::Paths;

/// the schema name shown by keyring browsers. the first build used the
/// plain name without a bottle attribute; a separate schema lets that item
/// be cleared without touching a per-bottle one.
const SCHEMA: &str = "net.buwryy.Roseline.Cookies.V2";
const LEGACY_SCHEMA: &str = "net.buwryy.Roseline.Cookies";
/// the client's own file, named after the branded bundle identifier.
const BRANDED_COOKIE: &str = "net.buwryy.Roseline.cookies.tsv";
/// the pre-branding file, migrated from old installs.
const LEGACY_COOKIE: &str = "com.roblox.RobloxPlayer.cookies.tsv";
const SESSION_COOKIE: &str = ".ROBLOSECURITY";

static SESSION: Mutex<Option<String>> = Mutex::new(None);

pub fn session() -> Option<String> {
    SESSION.lock().ok().and_then(|guard| guard.clone())
}

/// the keyring operations the migration needs, kept behind a trait so the
/// flow is testable without a running secret service.
trait Store {
    fn get(&self, key: &Key) -> Result<Option<String>>;
    fn set(&self, key: &Key, value: &str) -> Result<()>;
    fn clear(&self, key: &Key) -> Result<()>;
}

/// what makes an item this bottle's own: two installs (a native package and
/// a Flatpak, say) must not overwrite each other's session.
struct Key {
    profile: String,
    bottle: String,
}

struct Keyring;

impl Store for Keyring {
    fn get(&self, key: &Key) -> Result<Option<String>> {
        let schema = schema();
        let value = libsecret::password_lookup_sync(
            Some(&schema),
            attributes(key),
            gtk::gio::Cancellable::NONE,
        )?;
        if value.is_some() {
            return Ok(value.map(|value| value.to_string()));
        }
        // the first build keyed items by user alone; move that item over
        // rather than making anyone sign in again. it lives under a
        // different schema, so clearing it cannot touch this one.
        let schema = legacy_schema();
        let legacy = libsecret::password_lookup_sync(
            Some(&schema),
            legacy_attributes(key),
            gtk::gio::Cancellable::NONE,
        )?;
        if let Some(text) = &legacy {
            self.set(key, text)?;
            libsecret::password_clear_sync(
                Some(&schema),
                legacy_attributes(key),
                gtk::gio::Cancellable::NONE,
            )?;
        }
        Ok(legacy.map(|value| value.to_string()))
    }

    fn set(&self, key: &Key, value: &str) -> Result<()> {
        let schema = schema();
        libsecret::password_store_sync(
            Some(&schema),
            attributes(key),
            Some(libsecret::COLLECTION_DEFAULT.as_str()),
            &label(&key.profile),
            value,
            gtk::gio::Cancellable::NONE,
        )?;
        Ok(())
    }

    fn clear(&self, key: &Key) -> Result<()> {
        let schema = schema();
        libsecret::password_clear_sync(
            Some(&schema),
            attributes(key),
            gtk::gio::Cancellable::NONE,
        )?;
        Ok(())
    }
}

fn schema() -> libsecret::Schema {
    libsecret::Schema::new(
        SCHEMA,
        libsecret::SchemaFlags::NONE,
        HashMap::from([
            ("app", libsecret::SchemaAttributeType::String),
            ("profile", libsecret::SchemaAttributeType::String),
            ("bottle", libsecret::SchemaAttributeType::String),
        ]),
    )
}

fn legacy_schema() -> libsecret::Schema {
    libsecret::Schema::new(
        LEGACY_SCHEMA,
        libsecret::SchemaFlags::NONE,
        HashMap::from([
            ("app", libsecret::SchemaAttributeType::String),
            ("profile", libsecret::SchemaAttributeType::String),
        ]),
    )
}

fn attributes(key: &Key) -> HashMap<&str, &str> {
    HashMap::from([
        ("app", "roseline"),
        ("profile", key.profile.as_str()),
        ("bottle", key.bottle.as_str()),
    ])
}

fn legacy_attributes(key: &Key) -> HashMap<&str, &str> {
    HashMap::from([("app", "roseline"), ("profile", key.profile.as_str())])
}

fn label(profile: &str) -> String {
    format!("Roséline Roblox cookies ({profile})")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Branded,
    Legacy,
}

/// the guest user's jar, split across the two bundle identifiers.
struct Files {
    dir: PathBuf,
    branded: PathBuf,
    legacy: PathBuf,
}

impl Files {
    fn for_paths(paths: &Paths) -> Option<Files> {
        let dir = paths.guest_user()?.join("Library/Cookies");
        Some(Files {
            dir: dir.clone(),
            branded: dir.join(BRANDED_COOKIE),
            legacy: dir.join(LEGACY_COOKIE),
        })
    }

    /// the client runs as this user, so only this user needs to look inside.
    fn ensure_private_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::set_permissions(&self.dir, std::fs::Permissions::from_mode(0o700))?;
        // an interrupted atomic write can leave the cookie in a temp file.
        for path in self.paths() {
            let _ = std::fs::remove_file(temp_path(path));
        }
        Ok(())
    }

    fn paths(&self) -> [&Path; 2] {
        [&self.branded, &self.legacy]
    }

    /// the live jar and which name it came from. the branded copy is the
    /// client's own; the legacy one is only a migration source.
    fn read(&self) -> Result<Option<(Source, String)>> {
        for (source, path) in [
            (Source::Branded, &self.branded),
            (Source::Legacy, &self.legacy),
        ] {
            match std::fs::read_to_string(path) {
                Ok(text) if !text.trim().is_empty() => return Ok(Some((source, text))),
                // an empty branded jar is the client saying it is signed
                // out; never fall through to a stale legacy session.
                Ok(_) if source == Source::Branded => {
                    return Ok(Some((Source::Branded, String::new())));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| format!("reading {}", path.display()));
                }
            }
        }
        Ok(None)
    }

    fn write(&self, text: &str) -> Result<()> {
        self.ensure_private_dir()?;
        for path in self.paths() {
            write_private(path, text)?;
        }
        Ok(())
    }

    fn remove(&self) -> Result<()> {
        for path in self.paths() {
            remove_quietly(path)?;
        }
        Ok(())
    }

    /// the legacy name is only a migration source; once it has been read it
    /// must not be able to bring an old session back.
    fn remove_legacy(&self) -> Result<()> {
        remove_quietly(&self.legacy)
    }
}

fn remove_quietly(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("removing {}", path.display())),
    }
}

/// before a launch: adopt a jar left on disk, or restore the keyring one.
/// a failure is logged and leaves the disk alone; it must not stop a launch.
pub fn prepare() {
    if let Err(error) = prepare_with(&Keyring, &Paths::get()) {
        eprintln!("roseline: could not use the keyring for the Roblox cookies: {error:#}");
    }
}

fn prepare_with(store: &dyn Store, paths: &Paths) -> Result<()> {
    let Some(files) = Files::for_paths(paths) else {
        return Ok(());
    };
    files.ensure_private_dir()?;
    let key = key(paths);
    let mut cached = None;
    let result = (|| -> Result<()> {
        match files.read()? {
            Some((Source::Branded, text)) if has_session(&text) => {
                // the client's own file is the live jar, including the first
                // launch after an upgrade; it wins over an older keyring copy.
                // rewrite it so a jar left world-readable is closed down before
                // the client starts.
                cached = session_value(&text).map(str::to_string);
                store.set(&key, &text)?;
                files.write(&text)?;
            }
            Some((Source::Branded, _)) => {
                // signed out: do not restore a stale session later.
                store.clear(&key)?;
                files.remove()?;
            }
            Some((Source::Legacy, text)) => match store.get(&key) {
                Ok(Some(stored)) if has_session(&stored) => {
                    // the keyring has already owned this bottle; a leftover
                    // legacy file must not sign anyone back in.
                    cached = session_value(&stored).map(str::to_string);
                    files.write(&stored)?;
                }
                Ok(Some(_)) => {
                    store.clear(&key)?;
                    files.remove()?;
                }
                Ok(None) if has_session(&text) => {
                    // first run after upgrading from the pre-keyring build.
                    cached = session_value(&text).map(str::to_string);
                    store.set(&key, &text)?;
                    files.write(&text)?;
                }
                Ok(None) => files.remove()?,
                Err(error) => {
                    // the client reads the branded name; leave it a working
                    // session while the keyring error is reported.
                    cached = session_value(&text).map(str::to_string);
                    files.write(&text)?;
                    return Err(error);
                }
            },
            None => match store.get(&key)? {
                Some(text) if has_session(&text) => {
                    cached = session_value(&text).map(str::to_string);
                    files.write(&text)?;
                }
                Some(_) => store.clear(&key)?,
                None => {}
            },
        }
        files.remove_legacy()?;
        Ok(())
    })();
    if let Ok(mut session) = SESSION.lock() {
        *session = cached.clone();
    }
    best_effort_purge(paths, cached.as_deref());
    result
}

/// after the client exits: save its jar, then remove the plaintext copies.
/// the caller keeps the application alive while this runs.
pub fn seal() {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = seal_with(&Keyring, &Paths::get()).map_err(|error| format!("{error:#}"));
        let _ = sender.send(result);
    });
    match receiver.recv_timeout(std::time::Duration::from_secs(10)) {
        Ok(Ok(())) => {}
        Ok(Err(error)) => eprintln!("roseline: could not store the Roblox cookies: {error}"),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => eprintln!(
            "roseline: storing the Roblox cookies timed out; the plaintext jar stays for the next launch"
        ),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            eprintln!("roseline: the Roblox cookie store stopped unexpectedly")
        }
    }
}

fn seal_with(store: &dyn Store, paths: &Paths) -> Result<()> {
    let Some(files) = Files::for_paths(paths) else {
        return Ok(());
    };
    files.ensure_private_dir()?;
    let key = key(paths);
    let mut cached = None;
    let result = (|| -> Result<()> {
        match files.read()? {
            Some((_, text)) if has_session(&text) => {
                cached = session_value(&text).map(str::to_string);
                store.set(&key, &text)?;
                files.remove()?;
            }
            Some(_) => {
                store.clear(&key)?;
                files.remove()?;
            }
            None => {
                // the client may have removed the jar when signing out.
                store.clear(&key)?;
            }
        }
        Ok(())
    })();
    best_effort_purge(paths, cached.as_deref());
    if result.is_ok()
        && let Ok(mut session) = SESSION.lock()
    {
        *session = None;
    }
    result
}

fn key(paths: &Paths) -> Key {
    let profile = paths
        .guest_user()
        .and_then(|user| {
            user.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "default".to_string());
    Key {
        profile,
        bottle: paths.bottle().display().to_string(),
    }
}

fn session_value(text: &str) -> Option<&str> {
    text.lines().find_map(|line| {
        let mut fields = line.split('\t');
        (fields.next() == Some(SESSION_COOKIE))
            .then(|| fields.next())
            .flatten()
            .filter(|value| !value.trim().is_empty())
    })
}

fn has_session(text: &str) -> bool {
    session_value(text).is_some()
}

/// cached responses can carry the cookie header; they are safe to drop and
/// must not outlive the plaintext jar.
fn purge_cached_copies(paths: &Paths, session: Option<&str>) -> usize {
    let Some(user) = paths.guest_user() else {
        return 0;
    };
    let mut needles: Vec<&[u8]> = vec![b".ROBLOSECURITY="];
    // a real session cookie is long; a short value would match half the cache.
    if let Some(session) = session.filter(|value| value.len() >= 16) {
        needles.push(session.as_bytes());
    }
    let mut removed = 0;
    for name in ["Library/Caches", "Library/WebKit", "Library/Logs"] {
        walk_and_purge(&user.join(name), &needles, &mut removed);
    }
    // Roseline's own launch log tails the client; it must not keep a copy.
    walk_and_purge(&paths.logs(), &needles, &mut removed);
    removed
}

/// one unreadable cache file must not stop the rest from being cleaned.
fn walk_and_purge(dir: &Path, needles: &[&[u8]], removed: &mut usize) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            eprintln!("roseline: could not scan {}: {error}", dir.display());
            return;
        }
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            walk_and_purge(&entry.path(), needles, removed);
        } else if file_type.is_file() {
            let contains = file_contains(&entry.path(), needles).unwrap_or_else(|error| {
                eprintln!(
                    "roseline: could not read {}: {error:#}",
                    entry.path().display()
                );
                false
            });
            if contains {
                match std::fs::remove_file(entry.path()) {
                    Ok(()) => *removed += 1,
                    Err(error) => eprintln!(
                        "roseline: could not remove {}: {error}",
                        entry.path().display()
                    ),
                }
            }
        }
    }
}

fn file_contains(path: &Path, needles: &[&[u8]]) -> Result<bool> {
    let mut file =
        std::fs::File::open(path).with_context(|| format!("reading {}", path.display()))?;
    let keep = needles.iter().map(|needle| needle.len()).max().unwrap_or(0);
    let keep = keep.saturating_sub(1);
    let mut tail = Vec::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("reading {}", path.display()))?;
        if read == 0 {
            return Ok(false);
        }
        tail.extend_from_slice(&buffer[..read]);
        if needles
            .iter()
            .any(|needle| tail.windows(needle.len()).any(|window| window == *needle))
        {
            return Ok(true);
        }
        if tail.len() > keep {
            tail.drain(..tail.len() - keep);
        }
    }
}

fn best_effort_purge(paths: &Paths, session: Option<&str>) {
    purge_cached_copies(paths, session);
}

fn temp_path(path: &Path) -> PathBuf {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!(".{name}.tmp"))
}

fn write_private(path: &Path, text: &str) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)?;
    let temp = temp_path(path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temp)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Memory {
        value: std::cell::RefCell<Option<String>>,
    }

    impl Store for Memory {
        fn get(&self, _key: &Key) -> Result<Option<String>> {
            Ok(self.value.borrow().clone())
        }

        fn set(&self, _key: &Key, value: &str) -> Result<()> {
            *self.value.borrow_mut() = Some(value.to_string());
            Ok(())
        }

        fn clear(&self, _key: &Key) -> Result<()> {
            *self.value.borrow_mut() = None;
            Ok(())
        }
    }

    struct Failing;

    impl Store for Failing {
        fn get(&self, _key: &Key) -> Result<Option<String>> {
            Ok(None)
        }

        fn set(&self, _key: &Key, _value: &str) -> Result<()> {
            anyhow::bail!("the secret service is unavailable")
        }

        fn clear(&self, _key: &Key) -> Result<()> {
            Ok(())
        }
    }

    fn temp_paths(name: &str) -> Paths {
        let root =
            std::env::temp_dir().join(format!("roseline-cookies-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths {
            data: root.clone(),
            config: root.clone(),
            cache: root,
        };
        std::fs::create_dir_all(paths.prefix().join("root/Users/miles/Library/Cookies")).unwrap();
        paths
    }

    fn files(paths: &Paths) -> Files {
        Files::for_paths(paths).unwrap()
    }

    fn mode(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    const JAR: &str = "RBXEventTrackerV2\tCreateDate=1&rbxid=2\troblox.com\t/\t0\t1\n\
                       .ROBLOSECURITY\tsecret-value\troblox.com\t/\t1\t2\n";

    #[test]
    fn only_a_non_empty_security_cookie_counts_as_a_session() {
        assert!(has_session(JAR));
        assert!(!has_session(".ROBLOSECURITY\t\troblox.com\t/\t1\t2\n"));
        assert!(!has_session("rbx-ip2\t1.2.3.4\troblox.com\t/\t0\t2\n"));
        assert!(!has_session(""));
    }

    #[test]
    fn the_keyring_item_is_keyed_by_the_guest_user_and_bottle() {
        let paths = temp_paths("key");
        let key = key(&paths);
        assert_eq!(key.profile, "miles");
        assert_eq!(key.bottle, paths.bottle().display().to_string());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn two_bottles_get_different_keyring_items() {
        let native = temp_paths("key-native");
        let flatpak = temp_paths("key-flatpak");
        assert_ne!(key(&native).bottle, key(&flatpak).bottle);
        assert_eq!(key(&native).profile, key(&flatpak).profile);
        let _ = std::fs::remove_dir_all(&native.data);
        let _ = std::fs::remove_dir_all(&flatpak.data);
    }

    #[test]
    fn an_existing_jar_is_adopted_before_the_first_launch() {
        let paths = temp_paths("adopt");
        let files = files(&paths);
        std::fs::write(&files.branded, JAR).unwrap();
        std::fs::write(temp_path(&files.branded), JAR).unwrap();
        std::fs::set_permissions(&files.branded, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::fs::set_permissions(&files.dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let store = Memory::default();

        prepare_with(&store, &paths).unwrap();

        assert_eq!(store.value.borrow().as_deref(), Some(JAR));
        assert_eq!(std::fs::read_to_string(&files.branded).unwrap(), JAR);
        assert!(!files.legacy.exists());
        assert_eq!(mode(&files.branded), 0o600);
        assert_eq!(mode(&files.dir), 0o700);
        assert!(!temp_path(&files.branded).exists());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn cached_cookie_headers_are_removed() {
        let paths = temp_paths("cache");
        let user = paths.guest_user().unwrap();
        let caches = user.join("Library/Caches/net.buwryy.Roseline");
        let webkit = user.join("Library/WebKit/net.buwryy.Roseline");
        let logs = user.join("Library/Logs/Roblox");
        for dir in [&caches, &webkit, &logs] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(
            caches.join("response"),
            "Cookie: .ROBLOSECURITY=old-value; rbx-ip2=1",
        )
        .unwrap();
        std::fs::write(
            webkit.join("state"),
            "token secret-value-that-is-long-enough here",
        )
        .unwrap();
        std::fs::write(
            logs.join("player.log"),
            ".ROBLOSECURITY=secret-value-that-is-long-enough",
        )
        .unwrap();
        std::fs::write(caches.join("unrelated"), "nothing to see").unwrap();
        std::fs::write(caches.join("locked"), "nothing here either").unwrap();
        std::fs::set_permissions(
            caches.join("locked"),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
        std::fs::create_dir_all(paths.logs()).unwrap();
        std::fs::write(
            paths.logs().join("launch-1.log"),
            "header .ROBLOSECURITY=secret-value",
        )
        .unwrap();

        let removed = purge_cached_copies(&paths, Some("secret-value-that-is-long-enough"));

        assert_eq!(removed, 4);
        assert!(!caches.join("response").exists());
        assert!(!webkit.join("state").exists());
        assert!(!logs.join("player.log").exists());
        assert!(!paths.logs().join("launch-1.log").exists());
        assert!(caches.join("unrelated").exists());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn cached_headers_are_removed_even_without_a_session() {
        let paths = temp_paths("cache-logout");
        let user = paths.guest_user().unwrap();
        let caches = user.join("Library/Caches/net.buwryy.Roseline");
        std::fs::create_dir_all(&caches).unwrap();
        std::fs::write(caches.join("response"), "Cookie: .ROBLOSECURITY=old-value").unwrap();
        std::fs::write(caches.join("unrelated"), "1").unwrap();

        // a one-byte session value must not be used as a needle.
        let removed = purge_cached_copies(&paths, Some("1"));

        assert_eq!(removed, 1);
        assert!(!caches.join("response").exists());
        assert!(caches.join("unrelated").exists());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn a_legacy_jar_is_migrated_when_the_keyring_is_empty() {
        let paths = temp_paths("legacy");
        let files = files(&paths);
        std::fs::write(&files.legacy, JAR).unwrap();
        let store = Memory::default();

        prepare_with(&store, &paths).unwrap();

        assert_eq!(store.value.borrow().as_deref(), Some(JAR));
        assert_eq!(std::fs::read_to_string(&files.branded).unwrap(), JAR);
        assert!(!files.legacy.exists());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn a_stale_legacy_jar_cannot_undo_a_stored_session() {
        let paths = temp_paths("stale");
        let files = files(&paths);
        let stale = "ROBLOSECURITY_OLD\told-value\troblox.com\t/\t1\t1\n\
                     .ROBLOSECURITY\told-value\troblox.com\t/\t1\t1\n";
        std::fs::write(&files.legacy, stale).unwrap();
        let store = Memory::default();
        *store.value.borrow_mut() = Some(JAR.to_string());

        prepare_with(&store, &paths).unwrap();

        assert_eq!(store.value.borrow().as_deref(), Some(JAR));
        assert_eq!(std::fs::read_to_string(&files.branded).unwrap(), JAR);
        assert!(!files.legacy.exists());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn an_empty_branded_jar_does_not_fall_back_to_legacy() {
        let paths = temp_paths("empty-branded");
        let files = files(&paths);
        std::fs::write(&files.branded, "").unwrap();
        std::fs::write(&files.legacy, JAR).unwrap();
        let store = Memory::default();
        *store.value.borrow_mut() = Some(JAR.to_string());

        prepare_with(&store, &paths).unwrap();

        assert!(store.value.borrow().is_none());
        assert!(!files.branded.exists());
        assert!(!files.legacy.exists());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn a_logged_out_branded_jar_clears_the_stored_session() {
        let paths = temp_paths("signed-out");
        let files = files(&paths);
        std::fs::write(&files.branded, "rbx-ip2\t1.2.3.4\troblox.com\t/\t0\t2\n").unwrap();
        let store = Memory::default();
        *store.value.borrow_mut() = Some(JAR.to_string());

        prepare_with(&store, &paths).unwrap();

        assert!(store.value.borrow().is_none());
        assert!(!files.branded.exists());
        assert!(!files.legacy.exists());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn a_stored_jar_is_restored_to_the_branded_name() {
        let paths = temp_paths("restore");
        let files = files(&paths);
        let store = Memory::default();
        *store.value.borrow_mut() = Some(JAR.to_string());

        prepare_with(&store, &paths).unwrap();

        assert_eq!(std::fs::read_to_string(&files.branded).unwrap(), JAR);
        assert!(!files.legacy.exists());
        assert_eq!(mode(&files.branded), 0o600);
        assert_eq!(mode(&files.dir), 0o700);
        assert!(!temp_path(&files.branded).exists());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn sealing_stores_the_jar_and_removes_both_names() {
        let paths = temp_paths("seal");
        let files = files(&paths);
        std::fs::write(&files.branded, JAR).unwrap();
        std::fs::write(&files.legacy, JAR).unwrap();
        let store = Memory::default();

        seal_with(&store, &paths).unwrap();

        assert_eq!(store.value.borrow().as_deref(), Some(JAR));
        assert!(!files.branded.exists());
        assert!(!files.legacy.exists());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn a_logged_out_jar_clears_the_stored_session() {
        let paths = temp_paths("logout");
        let files = files(&paths);
        std::fs::write(&files.branded, "rbx-ip2\t1.2.3.4\troblox.com\t/\t0\t2\n").unwrap();
        let store = Memory::default();
        *store.value.borrow_mut() = Some(JAR.to_string());

        seal_with(&store, &paths).unwrap();

        assert!(store.value.borrow().is_none());
        assert!(!files.branded.exists());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn a_failed_store_leaves_the_plaintext_jar_alone() {
        let paths = temp_paths("failure");
        let files = files(&paths);
        std::fs::write(&files.branded, JAR).unwrap();

        assert!(seal_with(&Failing, &paths).is_err());
        assert!(files.branded.exists());
        assert_eq!(std::fs::read_to_string(&files.branded).unwrap(), JAR);
        let _ = std::fs::remove_dir_all(&paths.data);
    }
}
