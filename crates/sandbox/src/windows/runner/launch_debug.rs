//! [`LaunchMessage`]'s `Debug`, apart from `runner.rs` to keep it under 600 lines.

use super::LaunchMessage;

impl std::fmt::Debug for LaunchMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The environment's values (the proxy's token among them) are never printed.
        let names: Vec<&str> = self.env.iter().map(|(name, _)| name.as_str()).collect();
        f.debug_struct("LaunchMessage")
            .field("version", &self.version)
            .field("pool_user", &self.pool_user)
            .field("pool_name", &self.pool_name)
            .field("matter_sid", &self.matter_sid)
            .field("matter", &self.matter)
            .field("matter_root", &self.matter_root)
            .field("notes_root", &self.notes_root)
            .field("program", &self.program)
            .field("args", &self.args.len())
            .field("cwd", &self.cwd)
            .field("env", &names)
            .field("state", &self.state)
            .field("tmpdir", &self.tmpdir)
            .field("firm_ca", &self.firm_ca)
            .finish_non_exhaustive()
    }
}
