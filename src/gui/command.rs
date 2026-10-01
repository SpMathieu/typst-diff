//! Clipboard export for the exact settings used by a completed preview.
//! This module never renders a document, spawns a shell, or writes a PDF.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{ensure, Context as _, Result};
use eframe::egui;

use super::{App, FormSnapshot, Mode, Preview};
use crate::{CommonArgs, FilesArgs, GitArgs};

#[derive(Default)]
pub(super) struct CommandState {
    /// Captured BEFORE the preview worker starts, not when it finishes.
    preview_inputs: Option<FormSnapshot>,
    feedback: Option<(Instant, Result<(), String>)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Shell {
    Posix,
    PowerShell,
}

impl Shell {
    fn native() -> Self {
        if cfg!(windows) { Self::PowerShell } else { Self::Posix }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Posix => "sh / bash / zsh",
            Self::PowerShell => "PowerShell 7.3+ (not cmd.exe)",
        }
    }
}

impl App {
    pub(super) fn remember_preview_inputs(&mut self) {
        self.command_state.preview_inputs = Some(self.snapshot());
        self.command_state.feedback = None;
    }

    fn preview_matches_form(&self) -> bool {
        matches!(self.preview, Preview::Ready(_))
            && self.command_state.preview_inputs.as_ref()
                .is_some_and(|inputs| *inputs == self.snapshot())
    }

    pub(super) fn copy_command_button(&mut self, ui: &mut egui::Ui) {
        let current = self.preview_matches_form();
        let response = ui
            .add_enabled(current, egui::Button::new("Copy command"))
            .on_hover_text(format!(
                "Copy the CLI command for the displayed preview, using the current Output PDF. Paste into {}.",
                Shell::native().name(),
            ))
            .on_disabled_hover_text(
                "Render a successful preview with the current settings first. The output path can be changed without re-rendering.",
            );
        if response.clicked() {
            let result = self.command_text().map(|text| ui.ctx().copy_text(text))
                .map_err(|err| format!("Cannot copy command: {err:#}"));
            self.command_state.feedback = Some((Instant::now(), result));
        }
    }

    pub(super) fn copy_command_feedback(&mut self, ui: &mut egui::Ui) {
        if matches!(self.preview, Preview::Ready(_)) && !self.preview_matches_form() {
            ui.weak("Settings changed: refresh Preview before copying its command.");
        }
        if let Some((since, result)) = &self.command_state.feedback {
            let lifetime = if result.is_ok() {
                Duration::from_secs(4)
            } else {
                Duration::from_secs(12)
            };
            if since.elapsed() >= lifetime {
                self.command_state.feedback = None;
            } else {
                match result {
                    Ok(()) => { ui.label(format!("Command copied for {}.", Shell::native().name())); }
                    Err(error) => { ui.colored_label(ui.visuals().error_fg_color, error); }
                }
                ui.ctx().request_repaint_after(lifetime.saturating_sub(since.elapsed()));
            }
        }
    }

    fn command_text(&self) -> Result<String> {
        ensure!(self.preview_matches_form(), "refresh Preview with the current settings first");
        let cwd = std::env::current_dir().context("cannot read the GUI working directory")?;
        let executable = std::env::current_exe().context("cannot find the running typst-diff executable")?;
        // These are the same builders that Preview and Generate call. No
        // separate interpretation of blank roots, output defaults, or colors.
        let args = match self.mode {
            Mode::Files => files_tokens(&self.build_files_args(), &cwd)?,
            Mode::Git => git_tokens(&self.build_git_args(), &cwd)?,
        };
        shell_command(&executable, &args, Shell::native())
    }
}

/// Do not use `..` in these destructurings: a new CLI field must force the
/// serializer to be updated, rather than silently disappearing on export.
fn files_tokens(args: &FilesArgs, cwd: &Path) -> Result<Vec<String>> {
    let FilesArgs { old, new, output, old_root, new_root, common } = args;
    let mut tokens = vec!["files".to_owned()];
    if let Some(root) = old_root { option(&mut tokens, "old-root", &disk_path(root, cwd)?); }
    if let Some(root) = new_root { option(&mut tokens, "new-root", &disk_path(root, cwd)?); }
    common_tokens(&mut tokens, common, cwd)?;
    // Options precede `--`, so even a positional path starting with '-' is data.
    tokens.push("--".to_owned());
    tokens.push(disk_path(old, cwd)?);
    tokens.push(disk_path(new, cwd)?);
    tokens.push(disk_path(output, cwd)?);
    Ok(tokens)
}

fn git_tokens(args: &GitArgs, cwd: &Path) -> Result<Vec<String>> {
    let GitArgs { repo, file, output, old_rev, new_rev, old_root, new_root, common } = args;
    let mut tokens = vec!["git".to_owned()];
    option(&mut tokens, "old-rev", old_rev);
    option(&mut tokens, "new-rev", new_rev);
    // These paths belong to a Git tree, NOT to the real filesystem.
    if let Some(root) = old_root { option(&mut tokens, "old-root", path_text(root)?); }
    if let Some(root) = new_root { option(&mut tokens, "new-root", path_text(root)?); }
    common_tokens(&mut tokens, common, cwd)?;
    tokens.push("--".to_owned());
    tokens.push(disk_path(repo, cwd)?);
    tokens.push(path_text(file)?.to_owned());
    tokens.push(disk_path(output, cwd)?);
    Ok(tokens)
}

fn common_tokens(tokens: &mut Vec<String>, common: &CommonArgs, cwd: &Path) -> Result<()> {
    let CommonArgs {
        hide_deletions, hide_additions, deletion_color, addition_color,
        font_paths, package_path,
    } = common;
    if *hide_deletions { tokens.push("--hide-deletions".to_owned()); }
    if *hide_additions { tokens.push("--hide-additions".to_owned()); }
    // The GUI builder already converts premultiplied egui channels to plain
    // RGBA. Serialize that exact Typst color, including alpha, not Color32::r().
    option(tokens, "deletion-color", deletion_color.to_hex().as_str());
    option(tokens, "addition-color", addition_color.to_hex().as_str());
    for path in font_paths { option(tokens, "font-path", &disk_path(path, cwd)?); }
    if let Some(path) = package_path { option(tokens, "package-path", &disk_path(path, cwd)?); }
    Ok(())
}

fn option(tokens: &mut Vec<String>, name: &str, value: &str) {
    // The equals form also preserves option values starting with '-'.
    tokens.push(format!("--{name}={value}"));
}

fn path_text(path: &Path) -> Result<&str> {
    path.to_str().context("a path is not valid Unicode; a text clipboard cannot preserve it")
}

fn disk_path(path: &Path, cwd: &Path) -> Result<String> {
    ensure!(!path.as_os_str().is_empty(), "a required filesystem path is empty");
    ensure!(cwd.is_absolute(), "the GUI working directory must be absolute");
    // Do not canonicalize: the output need not exist, and collapsing '..'
    // lexically would change the meaning of paths that traverse symlinks.
    let absolute = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
    ensure!(absolute.is_absolute(), "use a fully qualified path instead of a drive-relative path: {}", path.display());
    Ok(path_text(&absolute)?.to_owned())
}

fn shell_command(executable: &Path, args: &[String], shell: Shell) -> Result<String> {
    ensure!(executable.is_absolute(), "the executable path must be absolute");
    let words: Vec<&str> = std::iter::once(path_text(executable)?)
        .chain(args.iter().map(String::as_str)).collect();
    for word in &words {
        ensure!(!word.chars().any(|c| matches!(c, '\0' | '\r' | '\n')),
            "a command argument contains NUL or a line break; cannot copy a safe single-line command");
    }
    Ok(match shell {
        Shell::Posix => words.iter().map(|s| quote_posix(s)).collect::<Vec<_>>().join(" "),
        // The script-block scope does not change the caller's preference.
        // Standard native argument passing requires PowerShell 7.3+ and keeps
        // embedded double quotes intact when invoking a native Windows exe.
        Shell::PowerShell => format!(
            "& {{ $PSNativeCommandArgumentPassing = 'Standard'; & {} }}",
            words.iter().map(|s| quote_powershell(s)).collect::<Vec<_>>().join(" "),
        ),
    })
}

fn quote_posix(value: &str) -> String {
    if !value.is_empty() && value.bytes().all(|c| {
        c.is_ascii_alphanumeric() || b"_@%+=:,./-".contains(&c)
    }) {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\"'\"'"))
    }
}

fn quote_powershell(value: &str) -> String {
    // PowerShell also recognizes typographic single quotation marks.
    let mut result = String::from("'");
    for c in value.chars() {
        result.push(c);
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}') { result.push(c); }
    }
    result.push('\'');
    result
}

#[cfg(test)]
mod tests;
