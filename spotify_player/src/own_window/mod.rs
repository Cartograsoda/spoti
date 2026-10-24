//! Windows only: running the TUI in its own window, with its own taskbar button.
//!
//! Every Store-installed Windows Terminal window shares one taskbar button, and Windows ignores
//! attempts to regroup them from another process. A portable Terminal is a different program to
//! the taskbar, so `setup-window` installs one, and from then on starting the TUI inside Windows
//! Terminal reopens it there.

use anyhow::{Context, Result};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const SETUP_SCRIPT: &str = include_str!("setup.ps1");

fn install_dir() -> Result<PathBuf> {
    let local = dirs_next::data_local_dir().context("find the local app data folder")?;
    Ok(local.join("spoti"))
}

fn portable_terminal() -> Option<PathBuf> {
    let terminal = install_dir()
        .ok()?
        .join("terminal")
        .join("WindowsTerminal.exe");
    terminal.exists().then_some(terminal)
}

fn exe_name() -> String {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| env!("CARGO_PKG_NAME").to_string())
}

/// The `setup-window` subcommand: installs the portable Terminal and a Start menu shortcut for
/// pinning, or with `remove`, deletes both.
pub fn setup(remove: bool) -> Result<()> {
    let dir = install_dir()?;
    std::fs::create_dir_all(&dir).context("create the install folder")?;
    let script = dir.join("setup-window.ps1");
    std::fs::write(&script, SETUP_SCRIPT).context("write the setup script")?;
    let exe = std::env::current_exe().context("find the app's own path")?;

    let mut powershell = Command::new("powershell.exe");
    powershell
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg("-InstallDir")
        .arg(&dir)
        .arg("-Exe")
        .arg(&exe);
    if remove {
        powershell.arg("-Remove");
    }
    let status = powershell.status().context("run the setup script")?;
    if !status.success() {
        anyhow::bail!("setup-window failed, see the error above");
    }

    if !remove {
        // so the pinned shortcut has the user's colours before the first relaunch syncs them
        if let Some(terminal) = portable_terminal() {
            sync_terminal_settings(&terminal);
        }
        let _ = std::fs::remove_file(&script);
    }
    Ok(())
}

/// Reopens the TUI in the portable Terminal if [`setup`] installed one and the app was started
/// inside Windows Terminal. Returns whether it did, in which case this process should exit.
pub fn relaunch() -> Result<bool> {
    if !in_windows_terminal() {
        return Ok(false);
    }
    let Some(terminal) = portable_terminal() else {
        return Ok(false);
    };
    sync_terminal_settings(&terminal);

    let mut parts = vec!["-w new new-tab --title Spoti --suppressApplicationTitle".to_string()];
    // the profile of the tab the app was started from, so the new window looks the same. Tabs
    // handed over by Windows don't say, so use the default one: without `-p`, Terminal only
    // picks a profile whose command line matches, and none runs this app
    let profile = std::env::var("WT_PROFILE_ID")
        .ok()
        .or_else(|| default_profile(&terminal));
    if let Some(profile) = profile {
        parts.push(format!("-p {}", quote(&profile)));
    }
    let exe = std::env::current_exe().context("find the app's own path")?;
    parts.push(quote(&exe.to_string_lossy()));
    parts.push("--in-place".to_string());
    parts.extend(
        std::env::args_os()
            .skip(1)
            .map(|arg| quote(&arg.to_string_lossy())),
    );

    Command::new(terminal)
        .raw_arg(parts.join(" "))
        .spawn()
        .context("start the portable Windows Terminal (run with --in-place to skip it)")?;
    Ok(true)
}

/// Once, after the TUI quits, tells Windows Terminal users that `setup-window` exists.
pub fn print_setup_hint_once(cache_folder: &Path) {
    if !in_windows_terminal() || portable_terminal().is_some() {
        return;
    }
    let marker = cache_folder.join("setup-window-hint-shown");
    if marker.exists() || std::fs::write(&marker, "").is_err() {
        return;
    }
    println!(
        "Tip: `{} setup-window` gives this app its own window and taskbar button, \
         so it stops hiding among your other terminal windows.",
        exe_name()
    );
}

/// Tabs that Windows Terminal opens itself have `WT_SESSION` set. Consoles that Windows starts
/// and then hands over to it (cmd from the Start menu, when Terminal is the default) don't, but
/// their hidden console window is owned by the Terminal's window.
pub(crate) fn in_windows_terminal() -> bool {
    std::env::var_os("WT_SESSION").is_some()
        || console_owner_exe().is_some_and(|exe| {
            exe.file_name()
                .is_some_and(|name| name.eq_ignore_ascii_case("WindowsTerminal.exe"))
        })
}

#[allow(unsafe_code)] // Win32 calls; the process handle is closed before returning
fn console_owner_exe() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Console::GetConsoleWindow,
        System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::WindowsAndMessaging::{GetWindow, GetWindowThreadProcessId, GW_OWNER},
    };

    unsafe {
        let owner = GetWindow(GetConsoleWindow(), GW_OWNER);
        if owner.is_null() {
            return None;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(owner, &raw mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let mut path = [0u16; 1024];
        let mut len = path.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            path.as_mut_ptr(),
            &raw mut len,
        );
        CloseHandle(process);
        (ok != 0).then(|| std::ffi::OsString::from_wide(&path[..len as usize]).into())
    }
}

/// Copies the Store Terminal's settings over the portable one's, so the window keeps the user's
/// colours and fonts. Best effort: stale settings only cost looks.
fn sync_terminal_settings(terminal: &Path) {
    let Some(local) = dirs_next::data_local_dir() else {
        return;
    };
    let source = [
        "Microsoft.WindowsTerminal_8wekyb3d8bbwe",
        "Microsoft.WindowsTerminalPreview_8wekyb3d8bbwe",
    ]
    .iter()
    .map(|package| {
        local
            .join("Packages")
            .join(package)
            .join("LocalState")
            .join("settings.json")
    })
    .find(|path| path.exists());
    if let Some(source) = source {
        let _ = std::fs::copy(
            source,
            terminal.with_file_name("settings").join("settings.json"),
        );
    }
}

/// The `defaultProfile` of the synced settings. Searched for rather than parsed, since the file
/// may have comments.
fn default_profile(terminal: &Path) -> Option<String> {
    let settings =
        std::fs::read_to_string(terminal.with_file_name("settings").join("settings.json")).ok()?;
    let value = settings
        .split_once("\"defaultProfile\"")?
        .1
        .trim_start()
        .strip_prefix(':')?
        .trim_start()
        .strip_prefix('"')?;
    Some(value.split_once('"')?.0.to_string())
}

/// Quotes an argument so `CommandLineToArgvW` reads it back unchanged. Windows Terminal also
/// splits its command line on `;`, even inside quotes, unless it's escaped.
fn quote(arg: &str) -> String {
    let arg = arg.replace(';', r"\;");
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg;
    }
    let mut quoted = String::from('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            backslashes += 1;
            continue;
        }
        // backslashes only need escaping right before a quote
        let escaped = if c == '"' {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        quoted.push_str(&"\\".repeat(escaped));
        quoted.push(c);
        backslashes = 0;
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn quotes_only_when_needed() {
        assert_eq!(quote("-t"), "-t");
        assert_eq!(quote(""), r#""""#);
        assert_eq!(
            quote(r"C:\Users\Some One\spoti.exe"),
            r#""C:\Users\Some One\spoti.exe""#
        );
    }

    #[test]
    fn escapes_quotes_backslashes_and_semicolons() {
        assert_eq!(quote(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote(r"C:\some dir\"), r#""C:\some dir\\""#);
        assert_eq!(quote("a;b"), r"a\;b");
    }
}
