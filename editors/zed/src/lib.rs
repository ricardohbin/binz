//! Starts `binz lsp` for every binZ file.
//!
//! The server is the `binz` binary itself, found on the `PATH` of the
//! worktree. To run a checkout that is not installed, point Zed at it in
//! `settings.json`:
//!
//! ```json
//! "lsp": { "binz": { "binary": { "path": "/path/to/binlanguage/target/debug/binz" } } }
//! ```

use zed_extension_api::{self as zed, settings::LspSettings, LanguageServerId, Result};

struct Binz;

impl zed::Extension for Binz {
    fn new() -> Self {
        Binz
    }

    fn language_server_command(
        &mut self,
        id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let configured = LspSettings::for_worktree(id.as_ref(), worktree)
            .ok()
            .and_then(|s| s.binary)
            .and_then(|b| b.path);
        let command = match configured.or_else(|| worktree.which("binz")) {
            Some(path) => path,
            None => {
                return Err(
                    "`binz` is not on the PATH: run `cargo install --path .` in the binZ \
                            checkout, or set `lsp.binz.binary.path` in Zed's settings"
                        .into(),
                )
            }
        };
        Ok(zed::Command {
            command,
            args: vec!["lsp".into()],
            env: Default::default(),
        })
    }
}

zed::register_extension!(Binz);
