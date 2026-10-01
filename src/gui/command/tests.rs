use super::*;
use clap::Parser;
use std::path::PathBuf;

fn cwd() -> PathBuf { std::env::current_dir().unwrap() }
fn files_app() -> App {
    let mut app = App::default();
    app.files_old = "old project/main.typ".to_owned();
    app.files_new = "new project/main.typ".to_owned();
    app
}
fn mark_ready(app: &mut App) {
    app.remember_preview_inputs();
    app.preview = Preview::Ready(Vec::new());
}
fn parse(tokens: Vec<String>) -> crate::Command {
    let cli = crate::Cli::try_parse_from(std::iter::once("typst-diff".to_owned()).chain(tokens))
        .unwrap_or_else(|error| panic!("exported arguments rejected by clap: {error}"));
    assert!(!cli.gui);
    cli.command.expect("explicit subcommand")
}

#[test]
fn no_copy_before_a_successful_preview() {
    let mut app = files_app();
    assert!(!app.preview_matches_form());
    app.remember_preview_inputs();
    app.preview = Preview::Running;
    assert!(!app.preview_matches_form());
    app.preview = Preview::Error("test error".to_owned());
    assert!(!app.preview_matches_form());
    assert!(app.command_text().is_err());
}

#[test]
fn copy_enabled_only_for_the_rendered_settings() {
    let mut app = files_app();
    mark_ready(&mut app);
    assert!(app.preview_matches_form());
    app.hide_deletions = true;
    assert!(!app.preview_matches_form());
    app.hide_deletions = false;
    assert!(app.preview_matches_form());
    app.addition_color = egui::Color32::GREEN;
    assert!(!app.preview_matches_form());
    mark_ready(&mut app);
    assert!(app.preview_matches_form());
    app.files_old.push('x');
    assert!(!app.preview_matches_form());
}

#[test]
fn edits_during_render_are_not_mistaken_for_rendered_inputs() {
    let mut app = files_app();
    app.remember_preview_inputs();
    app.preview = Preview::Running;
    app.hide_additions = true;
    app.preview = Preview::Ready(Vec::new());
    assert!(!app.preview_matches_form());
    mark_ready(&mut app);
    assert!(app.preview_matches_form());
}

#[test]
fn output_and_view_controls_do_not_stale_the_preview() {
    let mut app = files_app();
    mark_ready(&mut app);
    app.files_output = "my output/review.pdf".to_owned();
    app.auto_preview = false;
    app.fit_width_pending = false;
    app.scene_rect = egui::Rect::from_min_size(egui::pos2(20.0, 40.0), egui::vec2(200.0, 400.0));
    assert!(app.preview_matches_form());
    let crate::Command::Files(parsed) = parse(files_tokens(&app.build_files_args(), &cwd()).unwrap())
        else { panic!("expected files") };
    assert_eq!(parsed.output, cwd().join("my output/review.pdf"));
}

#[test]
fn mode_switch_stales_the_preview() {
    let mut app = files_app();
    mark_ready(&mut app);
    app.mode = Mode::Git;
    assert!(!app.preview_matches_form());
}

#[test]
fn files_roundtrip_uses_every_common_option_and_absolute_paths() {
    let mut app = files_app();
    app.files_output = "review output.pdf".to_owned();
    app.files_old_root = "old project".to_owned();
    app.files_new_root = "new project".to_owned();
    app.hide_deletions = true;
    app.hide_additions = true;
    app.deletion_color = egui::Color32::from_rgba_unmultiplied(200, 40, 100, 128);
    app.addition_color = egui::Color32::from_rgba_unmultiplied(40, 150, 200, 90);
    app.font_paths = vec!["fonts one".to_owned(), "fonts two".to_owned()];
    app.package_path = "cached packages".to_owned();
    let args = app.build_files_args();
    let crate::Command::Files(parsed) = parse(files_tokens(&args, &cwd()).unwrap())
        else { panic!("expected files") };
    assert_eq!(parsed.old, cwd().join(&args.old));
    assert_eq!(parsed.new, cwd().join(&args.new));
    assert_eq!(parsed.output, cwd().join(&args.output));
    assert_eq!(parsed.old_root, Some(cwd().join("old project")));
    assert_eq!(parsed.new_root, Some(cwd().join("new project")));
    assert!(parsed.common.hide_additions && parsed.common.hide_deletions);
    assert_eq!(parsed.common.deletion_color, args.common.deletion_color);
    assert_eq!(parsed.common.addition_color, args.common.addition_color);
    assert_eq!(parsed.common.font_paths, vec![cwd().join("fonts one"), cwd().join("fonts two")]);
    assert_eq!(parsed.common.package_path, Some(cwd().join("cached packages")));
}

#[test]
fn git_roundtrip_keeps_tree_paths_relative() {
    let mut app = App::default();
    app.mode = Mode::Git;
    app.git_repo = "my repository".to_owned();
    app.git_file = "src/main.typ".to_owned();
    app.git_old_rev = "HEAD~3".to_owned();
    app.git_new_rev = "release/v2".to_owned();
    app.git_old_root = ".".to_owned();
    app.git_new_root = "reports".to_owned();
    app.font_paths = vec!["fonts".to_owned()];
    app.package_path = "packages".to_owned();
    app.hide_deletions = true;
    let args = app.build_git_args();
    let crate::Command::Git(parsed) = parse(git_tokens(&args, &cwd()).unwrap())
        else { panic!("expected git") };
    assert_eq!(parsed.repo, cwd().join("my repository"));
    assert_eq!(parsed.file, PathBuf::from("src/main.typ"));
    assert_eq!(parsed.old_root, Some(PathBuf::from(".")));
    assert_eq!(parsed.new_root, Some(PathBuf::from("reports")));
    assert_eq!(parsed.old_rev, "HEAD~3");
    assert_eq!(parsed.new_rev, "release/v2");
    assert!(parsed.common.hide_deletions);
    assert_eq!(parsed.common.font_paths, vec![cwd().join("fonts")]);
    assert_eq!(parsed.common.package_path, Some(cwd().join("packages")));
}

#[test]
fn blank_optional_fields_and_output_keep_builder_defaults() {
    let mut app = files_app();
    app.files_output = "   ".to_owned();
    app.files_old_root = " \t ".to_owned();
    app.package_path = "  ".to_owned();
    let crate::Command::Files(parsed) = parse(files_tokens(&app.build_files_args(), &cwd()).unwrap())
        else { panic!("expected files") };
    assert_eq!(parsed.output, cwd().join("diff.pdf"));
    assert!(parsed.old_root.is_none() && parsed.new_root.is_none());
    assert!(parsed.common.package_path.is_none());
    assert!(parsed.common.font_paths.is_empty());
}

#[test]
fn both_hide_switches_roundtrip_in_all_four_combinations() {
    for hide_deletions in [false, true] {
        for hide_additions in [false, true] {
            let mut app = files_app();
            app.hide_deletions = hide_deletions;
            app.hide_additions = hide_additions;
            let crate::Command::Files(parsed) = parse(files_tokens(&app.build_files_args(), &cwd()).unwrap())
                else { panic!("expected files") };
            assert_eq!(parsed.common.hide_deletions, hide_deletions);
            assert_eq!(parsed.common.hide_additions, hide_additions);
        }
    }
}

#[test]
fn transparent_color_roundtrip_uses_the_preview_conversion() {
    let mut app = files_app();
    for alpha in [0, 1, 64, 128, 254, 255] {
        app.deletion_color = egui::Color32::from_rgba_unmultiplied(100, 150, 200, alpha);
        let args = app.build_files_args();
        let crate::Command::Files(parsed) = parse(files_tokens(&args, &cwd()).unwrap())
            else { panic!("expected files") };
        assert_eq!(parsed.common.deletion_color, args.common.deletion_color);
    }
}

#[test]
fn leading_hyphens_in_values_and_git_file_are_not_options() {
    let mut app = App::default();
    app.git_repo = "repo".to_owned();
    app.git_file = "-entry.typ".to_owned();
    app.git_old_root = "-root".to_owned();
    app.git_old_rev = "-a-value".to_owned();
    app.git_new_rev = "main".to_owned();
    let tokens = git_tokens(&app.build_git_args(), &cwd()).unwrap();
    assert!(tokens.iter().any(|token| token == "--"));
    let crate::Command::Git(parsed) = parse(tokens) else { panic!("expected git") };
    assert_eq!(parsed.file, PathBuf::from("-entry.typ"));
    assert_eq!(parsed.old_root, Some(PathBuf::from("-root")));
    assert_eq!(parsed.old_rev, "-a-value");
}

#[test]
fn posix_quotes_spaces_apostrophes_unicode_and_metacharacters() {
    assert_eq!(quote_posix("files"), "files");
    assert_eq!(quote_posix(""), "''");
    assert_eq!(quote_posix("a b"), "'a b'");
    assert_eq!(quote_posix("a'b"), "'a'\"'\"'b'");
    assert_eq!(quote_posix("#f008"), "'#f008'");
    assert_eq!(quote_posix("$(printf wrong);*"), "'$(printf wrong);*'");
    assert_eq!(quote_posix("caf\u{e9}.typ"), "'caf\u{e9}.typ'");
}

#[test]
#[cfg(unix)]
fn posix_shell_preserves_arguments_without_expanding_them() {
    let cases = ["", "a'b", "spaces and tabs\t", "#00aabb88", "$(printf wrong)",
        "`printf wrong`", "; echo wrong", "$HOME", "*?[x]", "a\\b", "a\"b", "caf\u{e9}", "--font-path=/fonts/one two"];
    let command = format!("printf '%s\\0' {}", cases.iter().map(|s| quote_posix(s)).collect::<Vec<_>>().join(" "));
    let output = std::process::Command::new("/bin/sh").args(["-c", &command]).output().unwrap();
    assert!(output.status.success());
    let expected = cases.iter().flat_map(|s| s.as_bytes().iter().copied().chain([0])).collect::<Vec<_>>();
    assert_eq!(output.stdout, expected);
    assert!(output.stderr.is_empty());
}

#[test]
fn powershell_quotes_apostrophes_and_smart_quotes() {
    assert_eq!(quote_powershell(""), "''");
    assert_eq!(quote_powershell("a'b"), "'a''b'");
    assert_eq!(quote_powershell("$HOME; a\"b"), "'$HOME; a\"b'");
    assert_eq!(quote_powershell("l\u{2019}equipe"), "'l\u{2019}\u{2019}equipe'");
    assert_eq!(quote_powershell("\u{2018}x\u{2019}"), "'\u{2018}\u{2018}x\u{2019}\u{2019}'");
}

#[test]
fn powershell_scopes_standard_native_argument_passing() {
    let exe = cwd().join("Program Files/typst-diff.exe");
    let command = shell_command(&exe, &["files".to_owned()], Shell::PowerShell).unwrap();
    assert_eq!(command, format!(
        "& {{ $PSNativeCommandArgumentPassing = 'Standard'; & {} 'files' }}",
        quote_powershell(exe.to_str().unwrap())
    ));
}

#[test]
fn exported_command_names_the_actual_executable() {
    let exe = cwd().join("tools with spaces/typst-diff");
    let args = files_tokens(&files_app().build_files_args(), &cwd()).unwrap();
    let command = shell_command(&exe, &args, Shell::Posix).unwrap();
    assert!(command.starts_with(&quote_posix(exe.to_str().unwrap())));
    assert!(!command.contains("cargo run"));
    assert!(!command.contains("--gui"));
}

#[test]
fn command_rejects_nul_and_line_breaks() {
    let exe = cwd().join("typst-diff");
    for bad in ["a\0b", "a\nb", "a\rb"] {
        for shell in [Shell::Posix, Shell::PowerShell] {
            assert!(shell_command(&exe, &[bad.to_owned()], shell).is_err());
        }
    }
}

#[test]
fn filesystem_paths_are_absolute_without_resolving_symlinks() {
    let base = cwd();
    assert_eq!(disk_path(Path::new("future output.pdf"), &base).unwrap(), base.join("future output.pdf").to_str().unwrap());
    assert_eq!(disk_path(Path::new("symlink/../main.typ"), &base).unwrap(), base.join("symlink/../main.typ").to_str().unwrap());
    assert!(disk_path(Path::new(""), &base).is_err());
    assert!(disk_path(Path::new("file.typ"), Path::new("relative")).is_err());
}

#[test]
#[cfg(unix)]
fn non_unicode_paths_are_rejected_instead_of_corrupted() {
    use std::os::unix::ffi::OsStringExt;
    let path = PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', 0xff]));
    assert!(path_text(&path).is_err());
    assert!(disk_path(&path, &cwd()).is_err());
}

#[test]
fn complete_command_is_available_without_creating_an_output_file() {
    let mut app = files_app();
    mark_ready(&mut app);
    // These paths need not exist: exporting only serializes the validated
    // preview settings. The actual GUI reaches Ready after real evaluation.
    let text = app.command_text().unwrap();
    assert!(text.contains("files"));
    assert!(text.contains("--deletion-color="));
    assert!(text.contains("--addition-color="));
}
