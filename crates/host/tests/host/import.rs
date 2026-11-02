//! Import (`CreateMatter` of an existing folder): the data folder is refused with everything
//! inside it, as the matters root is.

use clauseo_protocol::{ErrorCode, HostCommand, MatterSource, Title};

use crate::support::Dir;
use crate::write;

#[test]
fn import_refuses_a_folder_inside_the_data_folder() {
    let dir = Dir::new("import_refuses_inside_data", &[]);
    let host = dir.start(0);
    // An agent's home, as the host makes one: a folder inside the data folder.
    let agents = dir.data_dir().join("agents");
    write::write(&agents.join("config.toml"), b"model = \"a\"\n");

    let folder = host.folder_ref(&agents);
    host.refused(
        HostCommand::CreateMatter {
            name: Title::new("agents").unwrap(),
            source: MatterSource::ExistingFolder { folder },
            client: None,
            next_date: None,
        },
        ErrorCode::MatterFolderInMattersRoot,
    );
    assert!(
        !agents.join(".clauseo").exists(),
        "the refused folder was marked"
    );
    host.shutdown();
}
