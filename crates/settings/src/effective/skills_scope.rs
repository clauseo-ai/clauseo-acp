//! A skill pack's scope (`skills.scope`, decision record 0040): reading it, and its one write,
//! which the host makes for `HostCommand::SetAddonScope` and `SetAddonOn`. It checks the lock
//! and the id, saves the user's `settings.json` through `files`, then sets memory, so the file
//! and memory never disagree.

use super::Effective;
use crate::error::Error;
use crate::files::Files;
use crate::schema::ItemScope;
use crate::{skills, user};

/// The key a policy sets to decide every pack's scope.
const KEY: &str = "skills.scope";

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the skills' scopes live in their own file, as the MCP writes do (mcp_write.rs)"
)]
impl Effective {
    /// The pack `id`'s scope: `all` unless `skills.scope` says `matter`.
    pub fn skill_scope(&self, id: &str) -> ItemScope {
        self.settings
            .skills
            .scope
            .get(id)
            .copied()
            .unwrap_or_default()
    }

    /// Sets the pack `id`'s scope: `matter` is written as the entry `skills.scope.<id>`, `all`
    /// removes it. Refused with [`Error::Locked`] while policy sets `skills.scope`, and with
    /// [`Error::Refused`] for an id that is no pack id; nothing is written then, and one already
    /// as asked writes nothing.
    pub fn set_skill_scope(
        &mut self,
        files: &dyn Files,
        id: &str,
        scope: ItemScope,
    ) -> Result<(), Error> {
        if self.is_locked(KEY) {
            return Err(Error::Locked {
                key: KEY.to_owned(),
            });
        }
        if !skills::is_pack_id(id) {
            return Err(Error::Refused {
                key: KEY.to_owned(),
                reason: skills::BAD_ID,
            });
        }
        if self.skill_scope(id) == scope {
            return Ok(());
        }
        let matter = scope == ItemScope::Matter;
        user::write_skill_scope(&self.locations, files, id, matter)?;
        if matter {
            self.settings.skills.scope.insert(id.to_owned(), scope);
        } else {
            self.settings.skills.scope.remove(id);
        }
        Ok(())
    }
}
