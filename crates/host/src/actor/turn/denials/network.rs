//! The network checks R18 3a and 3b name, now `crate::network_paths`' (#766): a path the agent
//! chose is decided from its text alone before anything touches it. Kept as a name 3b imports.

pub(crate) use crate::network_paths::{NetEnv, is_network_path};
