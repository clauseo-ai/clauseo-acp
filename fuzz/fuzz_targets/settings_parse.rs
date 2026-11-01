//! settings: settings.json and policy.json parsing, and the ui save, through the public API.
//!
//! Drives `policy::file::parse` (a policy.json), `Effective::load` (the user's settings.json,
//! read through the leaf and json readers, plus the policy source), `Effective::settings`, and
//! `Effective::apply_ui_patch` (the one runtime write, which saves the patch to settings.json and
//! then sets it in memory). The files live in memory (`Files`), so nothing touches the disk.
//!
//! Input: user settings.json bytes, 0x00, policy.json bytes, 0x00, UiPatch JSON. Each part is
//! optional: an empty or missing user file is no file, a missing policy is no policy source, and
//! a missing or empty patch is an empty UiPatch. Input whose patch does not parse is skipped.
//!
//! Invariants:
//!   1. every key a parsed policy sets (`PolicyValues::keys`) is locked in the Effective,
//!   2. the effective font size is within `FontSize::MIN..=FontSize::MAX`,
//!   3. the Settings, serialised to JSON and read back as a settings.json, give the same
//!      Settings with no warnings,
//!   4. when `apply_ui_patch` succeeds, the saved file reloads to what memory holds afterwards;
//!      the settings outside `ui` are unchanged; and the `ui` fields are the old ones with the
//!      patch's fields written over them,
//!   5. when `apply_ui_patch` fails, the user file does not parse as a JSON object.

#![no_main]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_settings::policy::file;
use clauseo_settings::{
    Effective, FilePolicy, Files, FontSize, Locations, PolicySource, Settings, UiPatch,
};
use libfuzzer_sys::fuzz_target;

#[derive(Default, Clone)]
struct MemFiles(std::rc::Rc<RefCell<BTreeMap<PathBuf, Vec<u8>>>>);

impl Files for MemFiles {
    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        Ok(self.0.borrow().get(path).cloned())
    }
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.0.borrow_mut().insert(path.to_owned(), bytes.to_vec());
        Ok(())
    }
}

fn load(
    locations: &Locations,
    files: &MemFiles,
    policy: Option<&FilePolicy<MemFiles>>,
) -> Effective {
    match policy {
        Some(p) => Effective::load(locations, files, &[p as &dyn PolicySource]),
        None => Effective::load(locations, files, &[]),
    }
}

/// The settings as JSON, with the patch's `ui` fields written over the settings' own.
fn with_patch(settings: &Settings, patch: &UiPatch) -> serde_json::Value {
    let mut value = serde_json::to_value(settings).expect("Settings serialise");
    let ui = value["ui"].as_object_mut().expect("a ui object");
    let patch = serde_json::to_value(patch).expect("UiPatch serialises");
    for (key, field) in patch.as_object().expect("a patch object") {
        ui.insert(key.clone(), field.clone());
    }
    value
}

/// The settings with the `ui` section removed, as JSON.
fn without_ui(settings: &Settings) -> serde_json::Value {
    let mut value = serde_json::to_value(settings).expect("Settings serialise");
    value
        .as_object_mut()
        .expect("a settings object")
        .remove("ui");
    value
}

fuzz_target!(|data: &[u8]| {
    let mut parts = data.splitn(3, |&b| b == 0);
    let user = parts.next();
    let policy = parts.next();
    let patch = parts.next();

    let root = Path::new("/fuzz-root");
    let locations = Locations::under_root(root);
    let policy_path = PathBuf::from("/etc/clauseo/policy.json");
    let files = MemFiles::default();
    if let Some(user) = user.filter(|u| !u.is_empty()) {
        files.write_atomic(locations.settings_file(), user).unwrap();
    }
    let policy_files = MemFiles::default();
    let mut parsed_keys = Vec::new();
    if let Some(policy) = policy {
        policy_files.write_atomic(&policy_path, policy).unwrap();
        if let Ok(values) = file::parse(&policy_path, policy) {
            parsed_keys = values.keys().collect();
            let _ = values.warnings();
            let _ = values.sets_a_value();
        }
    }
    let source = policy.map(|_| FilePolicy::new(policy_path.clone(), policy_files.clone()));
    let effective = load(&locations, &files, source.as_ref());

    // 1. Policy keys are locked.
    for key in &parsed_keys {
        assert!(effective.is_locked(key), "policy key {key:?} is not locked");
    }
    // 2. Font size within its documented range.
    let size = effective.settings().ui.font_size.get();
    assert!(
        (FontSize::MIN..=FontSize::MAX).contains(&size),
        "font size {size}"
    );
    let _ = effective.warnings();
    let _ = effective.locked().count();

    // 3. The Settings' JSON form reads back as the same Settings.
    let json = serde_json::to_vec(effective.settings()).expect("Settings serialise");
    let echo = MemFiles::default();
    echo.write_atomic(locations.settings_file(), &json).unwrap();
    let reread = load(&locations, &echo, None);
    assert_eq!(
        reread.settings(),
        effective.settings(),
        "Settings JSON does not read back the same: {}",
        String::from_utf8_lossy(&json)
    );
    assert!(
        reread.warnings().is_empty(),
        "Settings JSON reads back with warnings: {:?}\n{}",
        reread.warnings(),
        String::from_utf8_lossy(&json)
    );

    // 4 and 5. The ui save.
    let patch: UiPatch = match patch {
        Some(p) => match serde_json::from_slice(p) {
            Ok(p) => p,
            Err(_) => return,
        },
        None => UiPatch::default(),
    };
    let before = load(&locations, &files, None);
    let parses = match user.filter(|u| !u.is_empty()) {
        None => true,
        Some(bytes) => {
            let text = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
            serde_json_lenient::from_slice::<serde_json_lenient::Value>(text)
                .map(|v| v.is_object())
                .unwrap_or(false)
        }
    };
    // No policy source here, so no key is locked and the save is not refused for that.
    let mut live = load(&locations, &files, None);
    match live.apply_ui_patch(&files, &patch) {
        Ok(()) => {
            let after = load(&locations, &files, None);
            let written = files
                .read(locations.settings_file())
                .unwrap()
                .unwrap_or_default();
            assert_eq!(
                after.settings(),
                live.settings(),
                "the saved settings.json reloads to other settings than memory holds: {}",
                String::from_utf8_lossy(&written)
            );
            assert_eq!(
                without_ui(after.settings()),
                without_ui(before.settings()),
                "the save changed settings outside ui: {}",
                String::from_utf8_lossy(&written)
            );
            assert_eq!(
                serde_json::to_value(after.settings()).unwrap(),
                with_patch(before.settings(), &patch),
                "the save did not write exactly the patch over the old ui: {}",
                String::from_utf8_lossy(&written)
            );
        }
        Err(e) => {
            assert!(
                !parses,
                "apply_ui_patch refused a settings.json that parses as an object: {e}"
            );
        }
    }
});
