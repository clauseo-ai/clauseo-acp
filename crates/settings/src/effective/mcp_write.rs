//! The lawyer's MCP servers (Settings › Tools): reading the list, and the writes of one server
//! the host makes for `HostCommand::Mcp`. Each write checks the lock and the rules
//! ([`crate::mcp`]), saves the user's `settings.json` through `files`, then sets memory, so the
//! file and memory never disagree; a refusal or a failed write changes neither. None touches the
//! keychain: the host writes a server's values there before it calls these, and puts them back
//! when one fails.

use std::collections::BTreeMap;

use super::Effective;
use crate::error::Error;
use crate::files::Files;
use crate::mcp::{self, McpRefusal};
use crate::schema::McpServerLaunch;
use crate::user;

/// The key policy sets to replace the lawyer's list.
const KEY: &str = "mcp.servers";

#[expect(
    clippy::multiple_inherent_impl,
    reason = "effective.rs is at its 600-line limit; the MCP server writes live in their own file"
)]
impl Effective {
    /// Every MCP server in the settings, on or off, by name. When policy sets `mcp.servers`
    /// this is the firm's list alone.
    pub fn mcp_servers(&self) -> &BTreeMap<String, McpServerLaunch> {
        &self.settings.mcp.servers
    }

    /// The MCP servers turned on, in name order: the only ones an agent's session gets.
    pub fn enabled_mcp_servers(&self) -> impl Iterator<Item = (&str, &McpServerLaunch)> {
        self.settings
            .mcp
            .servers
            .iter()
            .filter(|(_, server)| server.enabled)
            .map(|(name, server)| (name.as_str(), server))
    }

    /// Whether the firm decides the list: policy sets `mcp.servers`, or a policy source that
    /// cannot be read locked it empty. Every write below is then [`Error::Locked`].
    pub fn mcp_locked(&self) -> bool {
        self.is_locked(KEY)
    }

    /// The checks [`Self::save_mcp_server`] makes, without the write: the lock, then that
    /// `previous` (when given) is a server, then the rules ([`mcp::check_server`]), then that no
    /// other server has `name` in any case (Credential Manager's accounts are case-blind).
    pub fn check_mcp_server(
        &self,
        previous: Option<&str>,
        name: &str,
        server: &McpServerLaunch,
    ) -> Result<(), Error> {
        if self.mcp_locked() {
            return Err(locked());
        }
        if let Some(previous) = previous
            && !self.settings.mcp.servers.contains_key(previous)
        {
            return Err(refused(previous, McpRefusal::Unknown));
        }
        mcp::check_server(name, server).map_err(|refusal| refused(name, refusal))?;
        let taken = self
            .settings
            .mcp
            .servers
            .keys()
            .any(|held| Some(held.as_str()) != previous && held.eq_ignore_ascii_case(name));
        if taken {
            return Err(refused(name, McpRefusal::NameTaken));
        }
        Ok(())
    }

    /// Adds the server `name` (`previous` `None`), or replaces `previous` with it, renamed when
    /// the names differ, after [`Self::check_mcp_server`]'s checks.
    pub fn save_mcp_server(
        &mut self,
        files: &dyn Files,
        previous: Option<&str>,
        name: &str,
        server: &McpServerLaunch,
    ) -> Result<(), Error> {
        self.check_mcp_server(previous, name, server)?;
        let entry = self.mcp_entry(server)?;
        user::write_mcp_server(&self.locations, files, previous, name, Some(entry))?;
        if let Some(previous) = previous {
            self.settings.mcp.servers.remove(previous);
        }
        self.settings
            .mcp
            .servers
            .insert(name.to_owned(), server.clone());
        Ok(())
    }

    /// Removes the server `name`, answering what it was; `Ok(None)`, with nothing written, when
    /// there is none.
    pub fn remove_mcp_server(
        &mut self,
        files: &dyn Files,
        name: &str,
    ) -> Result<Option<McpServerLaunch>, Error> {
        if self.mcp_locked() {
            return Err(locked());
        }
        if !self.settings.mcp.servers.contains_key(name) {
            return Ok(None);
        }
        user::write_mcp_server(&self.locations, files, None, name, None)?;
        Ok(self.settings.mcp.servers.remove(name))
    }

    /// Turns the server `name` on or off, changing only its `enabled` in the file. A server that
    /// is not there is refused; one already as asked writes nothing.
    pub fn set_mcp_server_enabled(
        &mut self,
        files: &dyn Files,
        name: &str,
        enabled: bool,
    ) -> Result<(), Error> {
        if self.mcp_locked() {
            return Err(locked());
        }
        let Some(held) = self.settings.mcp.servers.get(name) else {
            return Err(refused(name, McpRefusal::Unknown));
        };
        if held.enabled == enabled {
            return Ok(());
        }
        let mut server = held.clone();
        server.enabled = enabled;
        // Only the flag changes in the file: the entry's other keys stay byte for byte. An entry
        // the file lacks (the list loaded, then the file changed) is written whole.
        if !user::write_mcp_enabled(&self.locations, files, name, enabled)? {
            let entry = self.mcp_entry(&server)?;
            user::write_mcp_server(&self.locations, files, None, name, Some(entry))?;
        }
        self.settings.mcp.servers.insert(name.to_owned(), server);
        Ok(())
    }

    /// `server` as its `settings.json` entry.
    fn mcp_entry(&self, server: &McpServerLaunch) -> Result<serde_json_lenient::Value, Error> {
        serde_json_lenient::to_value(server).map_err(|error| Error::Io {
            path: self.locations.settings_file().to_owned(),
            source: error.into(),
        })
    }
}

fn locked() -> Error {
    Error::Locked {
        key: KEY.to_owned(),
    }
}

fn refused(name: &str, refusal: McpRefusal) -> Error {
    Error::Mcp {
        key: format!("{KEY}.{name}"),
        refusal,
    }
}
