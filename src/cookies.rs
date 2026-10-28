// SPDX-License-Identifier: GPL-3.0-or-later

//! the Roblox session jar, kept in the login keyring.
//!
//! the client only understands its own TSV file, so Roseline restores the
//! jar from libsecret before a launch and puts it back when the client exits.
//! the plaintext file is removed once it is safely in the keyring.

use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::Paths;

/// the schema name shown by keyring browsers.
const SCHEMA: &str = "net.buwryy.Roseline.Cookies";
/// the client's own file, named after the branded bundle identifier.
const BRANDED_COOKIE: &str = "net.buwryy.Roseline.cookies.tsv";
/// the pre-branding file, migrated from old installs.
const LEGACY_COOKIE: &str = "com.roblox.RobloxPlayer.cookies.tsv";
const SESSION_COOKIE: &str = ".ROBLOSECURITY";

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
        // rather than making anyone sign in again.
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

/// the guest user's jar, split across the two bundle identifiers.
struct Files {
    branded: PathBuf,
    legacy: PathBuf,
}

impl Files {
    fn for_paths(paths: &Paths) -> Option<Files> {
        let root = paths.guest_user()?.join("Library/Cookies");
        Some(Files {
            branded: root.join(BRANDED_COOKIE),
            legacy: root.join(LEGACY_COOKIE),
        })
    }

    fn paths(&self) -> [&Path; 2] {
        [&self.branded, &self.legacy]
    }

    /// the live jar: the branded copy when it has anything in it, then the
    /// pre-branding one for installations that predate it.
    fn read(&self) -> Result<Option<String>> {
        for path in self.paths() {
            match std::fs::read_to_string(path) {
                Ok(text) if !text.trim().is_empty() => return Ok(Some(text)),
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
        for path in self.paths() {
            write_private(path, text)?;
        }
        Ok(())
    }

    fn remove(&self) -> Result<()> {
        for path in self.paths() {
            match std::fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| format!("removing {}", path.display()));
                }
            }
        }
        Ok(())
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
    let key = key(paths);
    match files.read()? {
        Some(text) if has_session(&text) => {
            // a file on disk is the live jar, including the first launch
            // after an upgrade; it wins over an older keyring copy.
            store.set(&key, &text)?;
        }
        Some(_) => {
            // logged out: do not restore a stale session later.
            store.clear(&key)?;
            files.remove()?;
        }
        None => match store.get(&key)? {
            Some(text) if has_session(&text) => files.write(&text)?,
            Some(_) => store.clear(&key)?,
            None => {}
        },
    }
    Ok(())
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
    let key = key(paths);
    match files.read()? {
        Some(text) if has_session(&text) => {
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

fn has_session(text: &str) -> bool {
    text.lines().any(|line| {
        let mut fields = line.split('\t');
        fields.next() == Some(SESSION_COOKIE)
            && fields.next().is_some_and(|value| !value.trim().is_empty())
    })
}

fn write_private(path: &Path, text: &str) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temp = parent.join(format!(".{name}.tmp"));
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
        let store = Memory::default();

        prepare_with(&store, &paths).unwrap();

        assert_eq!(store.value.borrow().as_deref(), Some(JAR));
        assert!(files.branded.exists());
        let _ = std::fs::remove_dir_all(&paths.data);
    }

    #[test]
    fn a_stored_jar_is_restored_to_both_names() {
        let paths = temp_paths("restore");
        let files = files(&paths);
        let store = Memory::default();
        *store.value.borrow_mut() = Some(JAR.to_string());

        prepare_with(&store, &paths).unwrap();

        assert_eq!(std::fs::read_to_string(&files.branded).unwrap(), JAR);
        assert_eq!(std::fs::read_to_string(&files.legacy).unwrap(), JAR);
        assert_eq!(mode(&files.branded), 0o600);
        assert_eq!(mode(&files.legacy), 0o600);
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
