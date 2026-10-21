//! The places outside `tauri.conf.json` and `build.rs` that hand Tauri another config: arguments
//! to `generate_context!` in the desktop crate's Rust files, and `TAURI_CONFIG` in the
//! cargo config's `[env]`.
//!
//! `generate_context!` (tauri-macros 2.7.3 `context.rs:36-56`) reads a string literal as its
//! first argument as the path of the config to read in place of `tauri.conf.json`, and then
//! takes a root path, `capabilities = [...]` (the capability files to read in place of the
//! checked folder), `assets = ...` and `test = ...` (`context.rs:58-118`). With no argument it
//! reads `tauri.conf.json` of the crate (`context.rs:121-127`). So any argument fails. A
//! `generate_context` imported under another name (`use ... as`) fails too, since its calls
//! would carry another name. Each file is lexed into tokens and walked, macro bodies included;
//! it is not parsed as a whole file, since `include!` can pull in a `.rs` file that holds one
//! expression. A file named other than `*.rs` and pulled in with `include!` is not read. A symlink
//! to a folder fails and is not followed: a `mod` can compile a file behind it, and following it
//! could loop the walk (`x -> ..`).
//!
//! Cargo passes `[env]` of `.cargo/config.toml` to build scripts and rustc invocations (cargo
//! 1.99 config reference, `[env]`), so `TAURI_CONFIG` there reaches both readers: tauri-build
//! 2.7.3 (`lib.rs:681`) and tauri-codegen 2.7.3 `get_config` (`lib.rs:85`), which merge its JSON
//! over `tauri.conf.json`. The name matches without regard to ASCII case, as Windows reads
//! environment names. A cargo config that is not TOML is core-io's failure, reported there.

use super::{ENV_CONFIG, Fix, line_of};
use crate::checks::toml_lines::Manifest;
use crate::checks::{self, violation};
use crate::violation::Violation;
use proc_macro2::{TokenStream, TokenTree};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use syn::ext::IdentExt;

/// The macro that builds the app's context from the Tauri config.
const MACRO: &str = "generate_context";

/// Every `*.rs` file under `dir`, at any depth, other than `skip`, sorted by path, and every
/// symlink to a folder there, sorted by path. A symlink to a file counts as a file, read through
/// the link; a symlink to a folder is not followed, so a link to a folder above (`x -> ..`)
/// cannot loop the walk. Fails when a folder cannot be listed.
pub(super) fn rust_files(dir: &Path, skip: &Path) -> io::Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut files = Vec::new();
    let mut linked_folders = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in fs::read_dir(&next)? {
            let entry = entry?;
            let path = entry.path();
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_symlink() && path.is_dir() {
                linked_folders.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") && path != skip {
                files.push(path);
            }
        }
    }
    files.sort();
    linked_folders.sort();
    Ok((files, linked_folders))
}

/// The violations in the Rust file at `path` (repo-relative) holding `text`: each
/// `generate_context!` call with arguments and each rename of `generate_context`. Text that does
/// not lex as Rust tokens fails at the error's line.
pub(super) fn context_violations(path: &str, text: &str) -> Vec<Violation> {
    let tokens: TokenStream = match text.parse() {
        Ok(tokens) => tokens,
        Err(error) => {
            let line = line_of(error.span());
            let message = format!("`{path}` is not valid Rust: {error}");
            return vec![violation(path, line, message, Fix::SourceSyntax)];
        }
    };
    let mut found = Vec::new();
    find_calls(path, tokens, &mut found);
    found.sort_by_key(|violation| violation.line);
    found
}

/// Pushes onto `found` a violation for each `generate_context` in `tokens`, inside every group,
/// that is followed by `!` and a group holding tokens, or by `as`.
fn find_calls(path: &str, tokens: TokenStream, found: &mut Vec<Violation>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        let ident = match tree {
            TokenTree::Group(group) => {
                find_calls(path, group.stream(), found);
                continue;
            }
            TokenTree::Ident(ident) if ident.unraw() == MACRO => ident,
            TokenTree::Ident(_) | TokenTree::Literal(_) | TokenTree::Punct(_) => continue,
        };
        let line = line_of(ident.span());
        match (trees.get(index + 1), trees.get(index + 2)) {
            (Some(TokenTree::Punct(bang)), Some(TokenTree::Group(args)))
                if bang.as_char() == '!' && !args.stream().is_empty() =>
            {
                let message = format!(
                    "`{MACRO}!` is called with arguments, which can name \
                     another config, capability files or assets than the checked ones"
                );
                found.push(violation(path, line, message, Fix::ContextArguments));
            }
            (Some(TokenTree::Ident(word)), _) if word == "as" => {
                let message = format!(
                    "`{MACRO}` is renamed with `as`, so this check cannot \
                     see the calls under the new name"
                );
                found.push(violation(path, line, message, Fix::ContextRenamed));
            }
            _ => {}
        }
    }
}

/// The violations of the cargo config `rel` holding `text`: one for each `[env]` entry named
/// `TAURI_CONFIG`, in any ASCII case, at its key's line.
pub(super) fn env_violations(rel: &str, text: &str) -> Vec<Violation> {
    Manifest::parse(text)
        .table_keys(&["env"])
        .into_iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case(ENV_CONFIG))
        .map(|(name, line)| {
            let message = format!(
                "`{rel}` sets `{name}` in [env], which tauri-build and \
                 `{MACRO}!` merge over tauri.conf.json"
            );
            violation(rel, line, message, Fix::ConfigInEnv)
        })
        .collect()
}

/// The violations in the desktop crate's Rust files under `desktop` (repo-relative), other than
/// `build`, and the number of files read. Each symlink to a folder there fails: a `mod` can
/// compile a file behind it, which the walk does not read.
pub(super) fn source_violations(
    root: &Path,
    desktop: &str,
    build: &str,
) -> (usize, Vec<Violation>) {
    let (files, linked_folders) = match rust_files(&root.join(desktop), &root.join(build)) {
        Ok(walk) => walk,
        Err(error) => return (0, vec![checks::unreadable(desktop, &error)]),
    };
    let mut read = 0;
    let mut found: Vec<Violation> = linked_folders
        .iter()
        .map(|link| {
            let path = checks::repo_path(root, link);
            let message = format!(
                "`{path}` is a symlink to a folder: the desktop crate can compile Rust files \
                 behind it, and this check does not follow it"
            );
            violation(&path, 1, message, Fix::LinkedFolder)
        })
        .collect();
    for file in &files {
        let path = checks::repo_path(root, file);
        match fs::read_to_string(file) {
            Ok(text) => {
                read += 1;
                found.extend(context_violations(&path, &text));
            }
            Err(error) => found.push(checks::unreadable(&path, &error)),
        }
    }
    (read, found)
}
