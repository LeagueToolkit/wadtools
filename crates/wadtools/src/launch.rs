//! Executable launch ergonomics: drag-and-drop onto the binary and keeping the
//! console window readable when wadtools is started outside a terminal.
//!
//! When a user drags WAD files (or a folder) onto `wadtools.exe`, Explorer invokes the
//! binary with bare positional paths and no subcommand. [`preprocess_args`] detects that
//! shape and rewrites the arguments into an equivalent `extract` invocation so the normal
//! CLI machinery handles it. Because such a launch spawns a throwaway console that closes
//! on exit, the rewrite also injects `--pause on-error` so a failure stays visible.

use std::io::{Read, Write};

use clap::ValueEnum;

/// Subcommand names and aliases recognised by the CLI. If the first positional argument
/// matches one of these, the user is running a normal command and we must not treat it as
/// a dragged file. Keep in sync with the `Commands` enum and its aliases in `main.rs`.
const KNOWN_SUBCOMMANDS: &[&str] = &[
    "extract",
    "e",
    "diff",
    "list",
    "ls",
    "hashtable-dir",
    "hd",
    "download-hashes",
    "dl",
    "shell",
    "help",
];

/// Controls whether the process waits for the user before exiting, so a console spawned
/// by a double-click / drag-and-drop does not vanish before its output can be read.
#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
pub enum PauseMode {
    /// Never wait (normal terminal usage).
    Never,
    /// Wait only when the command failed, so the error can be read.
    OnError,
    /// Always wait for a keypress before exiting.
    Always,
}

/// Rewrites raw process arguments to support drag-and-drop onto the executable.
///
/// `raw` is the full argument vector including the program name at index 0. When the first
/// argument is a bare, existing filesystem path (not a flag and not a known subcommand),
/// every positional argument is treated as an extract input and the vector is rewritten to
/// `[<prog>, --pause, on-error, extract, -i, <paths...>]`. Otherwise `raw` is returned
/// unchanged so normal parsing (and normal error messages for typos) is preserved.
pub fn preprocess_args(raw: Vec<String>) -> Vec<String> {
    let Some(first) = raw.get(1) else {
        return raw; // no arguments: let clap show help
    };

    if first.starts_with('-') || KNOWN_SUBCOMMANDS.contains(&first.as_str()) {
        return raw;
    }

    // check if first argument is a real path
    if !std::path::Path::new(first).exists() {
        return raw;
    }

    let prog = raw[0].clone();
    let inputs = &raw[1..];

    let mut rewritten = Vec::with_capacity(inputs.len() + 5);
    rewritten.push(prog);
    rewritten.push("--pause".to_string());
    rewritten.push("on-error".to_string());
    rewritten.push("extract".to_string());
    rewritten.push("-i".to_string());
    rewritten.extend(inputs.iter().cloned());
    rewritten
}

/// Waits for the user before exiting when the pause mode calls for it.
///
/// `failed` reflects whether the command returned an error. `Always` waits unconditionally,
/// `OnError` waits only on failure, and `Never` returns immediately.
pub fn maybe_pause(mode: PauseMode, failed: bool) {
    let should_pause = match mode {
        PauseMode::Never => false,
        PauseMode::OnError => failed,
        PauseMode::Always => true,
    };
    if !should_pause {
        return;
    }

    print!("\nPress Enter to exit...");
    let _ = std::io::stdout().flush();
    let mut buf = [0u8; 1];
    let mut stdin = std::io::stdin();
    while let Ok(1) = stdin.read(&mut buf) {
        if buf[0] == b'\n' {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn passthrough_when_no_args() {
        assert_eq!(preprocess_args(v(&["wadtools"])), v(&["wadtools"]));
    }

    #[test]
    fn passthrough_for_flags() {
        assert_eq!(
            preprocess_args(v(&["wadtools", "--help"])),
            v(&["wadtools", "--help"])
        );
    }

    #[test]
    fn passthrough_for_known_subcommand() {
        let args = v(&["wadtools", "extract", "-i", "a.wad"]);
        assert_eq!(preprocess_args(args.clone()), args);
    }

    #[test]
    fn passthrough_for_nonexistent_first_arg() {
        // Typo'd subcommand that isn't a real path: leave it for clap to reject.
        let args = v(&["wadtools", "exract"]);
        assert_eq!(preprocess_args(args.clone()), args);
    }

    /// Creates a real, existing temp file and returns its path so drag-drop detection
    /// (which checks the filesystem) triggers regardless of the test working directory.
    fn temp_file(tag: &str) -> String {
        let path = std::env::temp_dir().join(format!(
            "wadtools_launch_test_{}_{tag}.wad",
            std::process::id()
        ));
        std::fs::write(&path, b"x").unwrap();
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn rewrites_dragged_file() {
        let file = temp_file("single");
        let out = preprocess_args(v(&["wadtools", &file]));
        assert_eq!(
            out,
            v(&["wadtools", "--pause", "on-error", "extract", "-i", &file])
        );
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn rewrites_multiple_dragged_files() {
        let a = temp_file("a");
        let b = temp_file("b");
        let out = preprocess_args(v(&["wadtools", &a, &b]));
        assert_eq!(
            out,
            v(&["wadtools", "--pause", "on-error", "extract", "-i", &a, &b])
        );
        let _ = std::fs::remove_file(&a);
        let _ = std::fs::remove_file(&b);
    }

    #[test]
    fn pause_mode_logic() {
        // Never/OnError(no failure) should not block; only assert the non-blocking paths.
        maybe_pause(PauseMode::Never, true);
        maybe_pause(PauseMode::OnError, false);
    }
}
