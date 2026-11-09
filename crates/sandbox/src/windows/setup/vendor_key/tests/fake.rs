//! [`FakeTree`] as the walk's [`Tree`]: keys by path, a rename moving a subtree, and the
//! faults and crashes the tests inject.

use super::super::{Miss, Tree, Value};
use super::FakeTree;
use crate::windows::setup::record_key::VENDOR_KEY;
use crate::windows::setup::vendor_key::VENDOR_SDDL;

impl Tree for FakeTree {
    type Key = String;

    fn open_root(&mut self) -> Result<Option<String>, Miss> {
        Ok(self.has(VENDOR_KEY).then(|| VENDOR_KEY.to_owned()))
    }

    fn create_root(&mut self) -> Result<(), Miss> {
        self.insert(VENDOR_KEY.to_owned(), VENDOR_SDDL, false);
        self.created.push(VENDOR_KEY.to_owned());
        Ok(())
    }

    fn take_over_root(&mut self, sddl: &str) -> Result<String, Miss> {
        self.take(VENDOR_KEY, sddl).map(|(key, _)| key)
    }

    fn root_leads_elsewhere(&mut self, key: &String) -> Result<bool, Miss> {
        Ok(self.node(key)?.link)
    }

    fn take_over_child(
        &mut self,
        parent: &String,
        name: &str,
        sddl: &str,
    ) -> Result<(String, String), Miss> {
        self.node(parent)?;
        self.take(&format!("{parent}\\{name}"), sddl)
    }

    fn create_child(&mut self, parent: &String, name: &str, sddl: &str) -> Result<String, Miss> {
        self.node(parent)?;
        let path = format!("{parent}\\{name}");
        if self.has(&path) {
            return Err(Miss::Failed(format!("{name} was there already")));
        }
        self.insert(path.clone(), sddl, false);
        self.created.push(path.clone());
        Ok(path)
    }

    fn read_values(&mut self, key: &String) -> Result<Vec<Value>, Miss> {
        let node = self.node(key)?;
        Ok(node
            .values
            .iter()
            .map(|name| {
                let (kind, data) = node.data.get(name).cloned().unwrap_or((1, Vec::new()));
                Value {
                    name: name.clone(),
                    kind,
                    data,
                }
            })
            .collect())
    }

    fn write_values(&mut self, key: &String, values: &[Value]) -> Result<(), Miss> {
        let node = self.node(key)?;
        for value in values {
            node.values.push(value.name.clone());
            node.data
                .insert(value.name.clone(), (value.kind, value.data.clone()));
        }
        Ok(())
    }

    fn flush(&mut self, key: &String) -> Result<(), Miss> {
        self.node(key)?;
        if self.fail_flush {
            return Err(Miss::Failed(
                "RegFlushKey failed (the test's crash)".to_owned(),
            ));
        }
        Ok(())
    }

    fn rename(&mut self, key: &String, name: &str) -> Result<(), Miss> {
        self.node(key)?;
        if self.fail_rename {
            return Err(Miss::Failed(
                "RegRenameKey failed (the test's crash)".to_owned(),
            ));
        }
        let (parent, _) = key.rsplit_once('\\').unwrap_or_default();
        let to = format!("{parent}\\{name}");
        if self.has(&to) {
            return Err(Miss::Failed(
                "RegRenameKey failed: the name is taken".to_owned(),
            ));
        }
        let below = format!("{key}\\");
        let moved: Vec<String> = self
            .keys
            .keys()
            .filter(|path| *path == key || path.starts_with(&below))
            .cloned()
            .collect();
        for path in moved {
            if let Some(node) = self.keys.remove(&path) {
                let rest = path.strip_prefix(key.as_str()).unwrap_or_default();
                self.keys.insert(format!("{to}{rest}"), node);
            }
        }
        self.renamed.push((key.clone(), to));
        Ok(())
    }

    fn retake(&mut self, key: &String, sddl: &str) -> Result<(), Miss> {
        self.set_sddl(key, sddl)?;
        if let Some((path, undone)) = self.retake_undone.clone()
            && &path == key
        {
            self.node(key)?.sddl = undone;
        }
        Ok(())
    }

    fn has_link_value(&mut self, key: &String) -> Result<bool, Miss> {
        Ok(self.node(key)?.link_value)
    }

    fn delete_link_value(&mut self, key: &String) -> Result<(), Miss> {
        self.node(key)?.link_value = false;
        Ok(())
    }

    fn sddl(&mut self, key: &String) -> Result<String, Miss> {
        Ok(self.node(key)?.sddl.clone())
    }

    fn set_sddl(&mut self, key: &String, sddl: &str) -> Result<(), Miss> {
        self.node(key)?.sddl = sddl.to_owned();
        self.set.push((key.clone(), sddl.to_owned()));
        Ok(())
    }

    fn subkeys(&mut self, key: &String) -> Result<Vec<String>, Miss> {
        self.node(key)?;
        let prefix = format!("{key}\\");
        Ok(self
            .keys
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix))
            .filter(|rest| !rest.contains('\\'))
            .map(str::to_owned)
            .collect())
    }

    fn delete(&mut self, key: String) -> Result<(), Miss> {
        if self.fail_delete.as_deref() == Some(key.as_str()) {
            return Err(Miss::Failed("NtDeleteKey failed (0xc0000022)".to_owned()));
        }
        if !self.subkeys(&key)?.is_empty() {
            return Err(Miss::Failed(
                "NtDeleteKey failed: the key has subkeys".to_owned(),
            ));
        }
        self.keys.remove(&key);
        self.deleted.push(key);
        Ok(())
    }

    fn reset_install_dir(&mut self, key: &String) -> Result<Option<String>, Miss> {
        let location = self.install_location.clone();
        let node = self.node(key)?;
        node.values.retain(|value| value != "(default)");
        if location.is_some() {
            node.values.push("(default)".to_owned());
        }
        self.install_resets.push(key.clone());
        Ok(location)
    }

    fn local_domain(&mut self) -> Result<[u32; 3], String> {
        Ok([1, 2, 3])
    }

    fn in_administrators(&mut self, sid: &str) -> Result<bool, String> {
        self.admins.get(sid).cloned().unwrap_or(Ok(false))
    }

    fn spell_out(&mut self, alias: &str) -> Result<String, String> {
        match alias {
            "LA" => Ok("S-1-5-21-1-2-3-500".to_owned()),
            "LG" => Ok("S-1-5-21-1-2-3-501".to_owned()),
            _ => Err("no such alias".to_owned()),
        }
    }
}
