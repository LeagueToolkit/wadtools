//! Windows Explorer shell integration: register/unregister right-click context-menu
//! entries for WAD files and folders.
//!
//! Entries are written under `HKEY_CURRENT_USER\Software\Classes` so no administrator
//! rights are required. Compound extensions like `.wad.client` cannot be targeted by a
//! classic per-extension ProgID, so we attach the verbs to the catch-all `*` (all files)
//! shell key and constrain them with an `AppliesTo` query that matches `*.wad` / `*.wad.*`.
//!
//! All entries are grouped under a single cascading **wadtools** submenu that is forced to
//! the top of the context menu. This is done with a parent verb carrying `MUIVerb`,
//! `Position = "Top"`, and an empty `SubCommands` value; Explorer then enumerates the
//! sub-verbs from a nested `shell` subkey.

use eyre::Result;

/// Actions for the `shell` subcommand.
#[derive(clap::Subcommand, Debug)]
pub enum ShellAction {
    /// Register the wadtools Explorer context-menu entries (per-user, no admin required)
    Install,
    /// Remove the wadtools Explorer context-menu entries
    Uninstall,
    /// Show whether the context-menu entries are installed and where they point
    Status,
}

pub fn run(action: &ShellAction) -> Result<()> {
    #[cfg(windows)]
    {
        match action {
            ShellAction::Install => windows_impl::install(),
            ShellAction::Uninstall => windows_impl::uninstall(),
            ShellAction::Status => windows_impl::status(),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = action;
        eyre::bail!("shell integration is only supported on Windows");
    }
}

#[cfg(windows)]
mod windows_impl {
    use eyre::{Context, Result};
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    /// The registry class a menu is attached to.
    #[derive(Copy, Clone)]
    enum VerbRoot {
        /// All files (`*`), constrained by `AppliesTo`.
        AllFiles,
        /// Directories.
        Directory,
    }

    impl VerbRoot {
        fn class(self) -> &'static str {
            match self {
                VerbRoot::AllFiles => "*",
                VerbRoot::Directory => "Directory",
            }
        }
    }

    /// A single entry inside the cascading `wadtools` submenu. `command` uses `{exe}` as a
    /// placeholder for the executable path; Explorer substitutes `%1` with the clicked item.
    struct SubVerb {
        key: &'static str,
        label: &'static str,
        command: &'static str,
        /// Draw a separator line above this entry in the cascading menu.
        separator_before: bool,
    }

    const ECF_SEPARATORBEFORE: u32 = 0x20;

    /// A cascading `wadtools` submenu attached to one registry class. The parent verb is
    /// forced to the top of the context menu and holds the sub-verbs in a nested `shell` key.
    struct Menu {
        root: VerbRoot,
        applies_to: Option<&'static str>,
        subverbs: &'static [SubVerb],
    }

    /// Parent key name of the cascading submenu (under `<class>\shell`).
    const MENU_KEY: &str = "wadtools";
    /// Label shown for the cascading submenu itself.
    const MENU_LABEL: &str = "wadtools";

    /// Query that matches WAD archives, including target-suffixed variants (`*.wad.client`).
    const WAD_APPLIES_TO: &str = "System.FileName:\"*.wad\" OR System.FileName:\"*.wad.*\"";

    const MENUS: &[Menu] = &[
        Menu {
            root: VerbRoot::AllFiles,
            applies_to: Some(WAD_APPLIES_TO),
            subverbs: &[
                SubVerb {
                    key: "extract",
                    label: "Extract",
                    // Routes through the drag-and-drop path, which pauses only on error.
                    command: "\"{exe}\" \"%1\"",
                    separator_before: false,
                },
                SubVerb {
                    key: "ripcdragon",
                    label: "Get CDragon Hashtable (.txt)",
                    // Writes a sibling `<name>.paths.txt`; pause so the result can be read.
                    command: "\"{exe}\" --pause always paths -i \"%1\"",
                    separator_before: true,
                },
                SubVerb {
                    key: "ripmimir",
                    label: "Get Mimir Hashtable (.lhdb)",
                    // Writes a sibling `<name>.paths.lhdb` mimir hash table.
                    command: "\"{exe}\" --pause always paths -F lhdb -i \"%1\"",
                    separator_before: false,
                },
            ],
        },
        Menu {
            root: VerbRoot::Directory,
            applies_to: None,
            subverbs: &[SubVerb {
                key: "extractfolder",
                label: "Extract all WADs",
                command: "\"{exe}\" \"%1\"",
                separator_before: false,
            }],
        },
    ];

    /// Registry path (under HKCU) of a menu's parent `shell\wadtools` node.
    fn menu_path(menu: &Menu) -> String {
        format!(
            "Software\\Classes\\{}\\shell\\{MENU_KEY}",
            menu.root.class()
        )
    }

    /// Registry path (under HKCU) of a sub-verb's node inside the cascading submenu.
    fn subverb_path(menu: &Menu, sub: &SubVerb) -> String {
        format!("{}\\shell\\{}", menu_path(menu), sub.key)
    }

    fn current_exe_string() -> Result<String> {
        let exe =
            std::env::current_exe().wrap_err("failed to resolve the wadtools executable path")?;
        Ok(exe.to_string_lossy().into_owned())
    }

    pub fn install() -> Result<()> {
        let exe = current_exe_string()?;
        let icon = format!("\"{}\",0", exe);
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        for menu in MENUS {
            let path = menu_path(menu);
            // Clear any previous submenu subtree first so stale sub-verbs from an older
            // install (e.g. the former "List contents") don't linger alongside the new ones.
            match hkcu.delete_subkey_all(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(e).wrap_err_with(|| format!("failed to reset registry key {path}"))
                }
            }
            let (key, _) = hkcu
                .create_subkey(&path)
                .wrap_err_with(|| format!("failed to create registry key {path}"))?;
            // Parent cascading verb: label, icon, forced to the top of the menu.
            key.set_value("MUIVerb", &MENU_LABEL)?;
            key.set_value("Icon", &icon)?;
            key.set_value("Position", &"Top")?;
            // An (empty) SubCommands value tells Explorer to build a cascading menu from the
            // nested `shell` subkey rather than treating this as an invokable verb.
            key.set_value("SubCommands", &"")?;
            if let Some(applies_to) = menu.applies_to {
                key.set_value("AppliesTo", &applies_to)?;
            }

            for sub in menu.subverbs {
                let sub_path = subverb_path(menu, sub);
                let (sub_key, _) = hkcu
                    .create_subkey(&sub_path)
                    .wrap_err_with(|| format!("failed to create registry key {sub_path}"))?;
                sub_key.set_value("", &sub.label)?;
                sub_key.set_value("Icon", &icon)?;
                if sub.separator_before {
                    sub_key.set_value("CommandFlags", &ECF_SEPARATORBEFORE)?;
                }

                let (command_key, _) = hkcu
                    .create_subkey(format!("{sub_path}\\command"))
                    .wrap_err_with(|| {
                        format!("failed to create registry key {sub_path}\\command")
                    })?;
                command_key.set_value("", &sub.command.replace("{exe}", &exe))?;

                tracing::info!("registered '{}' -> {}", sub.label, sub_path);
            }
        }

        println!("wadtools Explorer integration installed.");
        println!("Right-click a .wad / .wad.client file or a folder and open the 'wadtools' menu.");
        println!("(pointing at {exe})");
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let mut removed = 0usize;

        for menu in MENUS {
            let path = menu_path(menu);
            match hkcu.delete_subkey_all(&path) {
                Ok(()) => {
                    removed += 1;
                    tracing::info!("removed wadtools menu ({path})");
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    return Err(e).wrap_err_with(|| format!("failed to delete registry key {path}"))
                }
            }
        }

        if removed == 0 {
            println!("wadtools Explorer integration was not installed; nothing to remove.");
        } else {
            println!(
                "wadtools Explorer integration removed ({removed} menu{}).",
                if removed == 1 { "" } else { "s" }
            );
        }
        Ok(())
    }

    pub fn status() -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let current = current_exe_string().ok();

        println!("wadtools Explorer integration status:");
        for menu in MENUS {
            for sub in menu.subverbs {
                let sub_path = subverb_path(menu, sub);
                match hkcu.open_subkey(format!("{sub_path}\\command")) {
                    Ok(command_key) => {
                        let command: String = command_key.get_value("").unwrap_or_default();
                        let stale =
                            matches!(&current, Some(exe) if !command.contains(exe.as_str()));
                        let note = if stale {
                            "  [points at a different executable]"
                        } else {
                            ""
                        };
                        println!("  [installed] wadtools > {} -> {command}{note}", sub.label);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        println!("  [ missing ] wadtools > {}", sub.label);
                    }
                    Err(e) => {
                        return Err(e).wrap_err_with(|| {
                            format!("failed to read registry key {sub_path}\\command")
                        })
                    }
                }
            }
        }
        if let Some(exe) = current {
            println!("current executable: {exe}");
        }
        Ok(())
    }
}
