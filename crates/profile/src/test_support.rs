//! The snapshot tool: a folder tree as one text, for `insta` snapshots of what a render wrote.
//! R22's render snapshots use it too.

use std::path::Path;

/// Every file under `root`, sorted by its `/`-separated path, as `== <path> ==` then its
/// content (lossy UTF-8) and a newline. Folders show only through their files. An unreadable
/// entry shows as `<unreadable>`.
pub fn tree_text(root: &Path) -> String {
    let mut files = Vec::new();
    collect(root, root, &mut files);
    files.sort();
    let mut out = String::new();
    for (path, content) in files {
        out.push_str("== ");
        out.push_str(&path);
        out.push_str(" ==\n");
        out.push_str(&content);
        if !content.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, files);
            continue;
        }
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let content = std::fs::read(&path).map_or_else(
            |_| "<unreadable>".to_owned(),
            |bytes| String::from_utf8_lossy(&bytes).into_owned(),
        );
        files.push((name, content));
    }
}
