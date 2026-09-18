//! `--gui`: a small form-based front end for `files`/`git` mode, so the
//! command doesn't have to be assembled by hand on the command line.
//!
//! This is a thin layer over `main.rs`: every field here maps directly to
//! one CLI flag, `build_files_args`/`build_git_args` turn the form into
//! exactly the same `FilesArgs`/`GitArgs` clap would have parsed, and
//! "Generate" calls `run_files`/`run_git` -- the very same functions the
//! CLI itself calls -- on a background thread (so the window stays
//! responsive while Typst compiles/lays out/exports to PDF).

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use eframe::egui;
use typst::visualize::Color;
use typst_layout::PagedDocument;

use crate::{build_files_worlds, build_git_worlds, diff_and_layout, run_files, run_git, CommonArgs, FilesArgs, GitArgs};

pub fn run() -> Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1180.0, 760.0]),
        // eframe's default renderer (`wgpu`) fails outright ("Failed to
        // create surface for any enabled backend") in some environments
        // that otherwise have a perfectly usable display -- confirmed on
        // a real WSLg desktop, not just a sandboxed dev container. `glow`
        // (OpenGL, via glutin) is the standard fix -- see the `eframe`
        // dependency's own comment in Cargo.toml, and the README's
        // "Known limitations" if this still doesn't work on yours.
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "typst-diff",
        options,
        Box::new(|_cc| Ok(Box::new(App::default()))),
    )
    .map_err(|err| anyhow::anyhow!("GUI error: {err}"))
}

#[derive(PartialEq, Clone, Copy)]
enum Mode {
    Files,
    Git,
}

/// What the background job (see `App::job`) is doing, for the status
/// panel at the bottom of the window.
enum Status {
    Idle,
    Running,
    Success(PathBuf),
    Error(String),
}

/// What the background preview job (see `App::preview_job`) is doing, for
/// the preview panel on the right of the window.
enum Preview {
    Idle,
    Running,
    Ready(Vec<egui::TextureHandle>),
    Error(String),
}

struct App {
    mode: Mode,

    // `files` mode
    files_old: String,
    files_new: String,
    files_output: String,
    /// Left empty by default (unlike `git_old_root`): `run_files` then
    /// defaults it to `files_old`'s own parent directory, a real
    /// filesystem path that's usually right for a project laid out with
    /// its entry point at (or right under) its own root. `.` would mean
    /// something different and far less often correct here -- the
    /// process's current working directory, unrelated to `files_old`'s
    /// location.
    files_old_root: String,
    files_new_root: String,

    // `git` mode
    git_repo: String,
    git_file: String,
    git_output: String,
    git_old_rev: String,
    git_new_rev: String,
    git_old_root: String,
    git_new_root: String,
    /// Branch/tag names found in `git_repo`, cached for the "Choose..."
    /// menu next to `--old-rev`/`--new-rev` -- refreshed whenever
    /// `git_repo` changes (see `Self::refresh_git_refs`), not on every
    /// frame, since it has to open the repository to list them.
    git_refs: Vec<String>,
    git_refs_for: String,

    // Shared by both modes
    hide_deletions: bool,
    hide_additions: bool,
    deletion_color: egui::Color32,
    addition_color: egui::Color32,
    font_paths: Vec<String>,
    package_path: String,

    status: Status,
    /// `Some` while a `run_files`/`run_git` call is in flight on another
    /// thread -- polled (non-blockingly) from `update()`.
    job: Option<mpsc::Receiver<Result<PathBuf, String>>>,

    preview: Preview,
    /// `Some` while a preview render is in flight on another thread --
    /// polled (non-blockingly) from `update()`. Carries rendered pages as
    /// plain pixel buffers, not yet `App::preview`'s `TextureHandle`s:
    /// uploading a texture to the GPU needs the `egui::Context`, which
    /// only the UI thread has.
    preview_job: Option<mpsc::Receiver<Result<Vec<egui::ColorImage>, String>>>,
    /// The `egui::Scene` pan+zoom canvas's own view state for
    /// `App::preview_panel` -- which part of the (page-pixel-sized)
    /// scene is currently visible. `Rect::ZERO` (its starting value)
    /// means "not yet initialized," which `Scene::show` reads as "fit
    /// the content" -- see `Self::preview_panel`'s "Reset view" button.
    /// Deliberately not part of `FormSnapshot`: panning/zooming doesn't
    /// change what the diff *is*, so it has no business re-triggering a
    /// preview render.
    scene_rect: egui::Rect,

    /// Whether to re-run `Self::start_preview` on its own, a short while
    /// after any form field changes -- see `Self::maybe_auto_preview`.
    auto_preview: bool,
    /// The form's own state, as of the last frame -- compared against
    /// the current one each frame to notice a change at all (see
    /// `Self::snapshot`/`Self::maybe_auto_preview`). Always `Some` after
    /// the first frame; only `None` so `App::default()` doesn't need to
    /// duplicate every field's default just to build one.
    last_snapshot: Option<FormSnapshot>,
    /// Set the moment the snapshot last changed, cleared once
    /// `Self::start_preview` actually fires for it -- `maybe_auto_preview`
    /// waits for `AUTO_PREVIEW_DEBOUNCE` of quiet after the *last* change
    /// before firing, so typing several characters in a row triggers one
    /// re-render, not one per keystroke.
    dirty_since: Option<Instant>,
}

/// The part of `App`'s state that actually affects what a diff renders
/// as -- everything `Self::build_files_args`/`Self::build_git_args`
/// read, other than the output path (which affects where "Generate"
/// would save, not what the preview shows) and `git_refs`/`git_refs_for`
/// (derived, not user input). Compared frame to frame by
/// `Self::maybe_auto_preview` to notice a change worth re-previewing.
#[derive(Clone, PartialEq)]
struct FormSnapshot {
    mode: Mode,
    files_old: String,
    files_new: String,
    files_old_root: String,
    files_new_root: String,
    git_repo: String,
    git_file: String,
    git_old_rev: String,
    git_new_rev: String,
    git_old_root: String,
    git_new_root: String,
    hide_deletions: bool,
    hide_additions: bool,
    deletion_color: egui::Color32,
    addition_color: egui::Color32,
    font_paths: Vec<String>,
    package_path: String,
}

impl Default for App {
    fn default() -> Self {
        Self {
            mode: Mode::Files,
            files_old: String::new(),
            files_new: String::new(),
            files_output: "diff.pdf".to_string(),
            files_old_root: String::new(),
            files_new_root: String::new(),
            git_repo: String::new(),
            git_file: String::new(),
            git_output: "diff.pdf".to_string(),
            git_old_rev: String::new(),
            git_new_rev: String::new(),
            // Defaults to the repository root rather than empty (unlike
            // `files` mode's own root fields -- see `App::files_old_root`'s
            // doc comment): the far more common case for `git` mode is a
            // project whose entry point isn't at the repository root
            // (e.g. `src/main.typ`) but whose absolute imports still mean
            // the repository root, not `FILE`'s own directory -- leaving
            // this empty produces a working-but-wrong default that fails
            // confusingly on any absolute import (`git_root_field`'s
            // "Choose…" still lists every other directory, for the
            // uncommon case where a different one is actually needed).
            git_old_root: ".".to_string(),
            git_new_root: ".".to_string(),
            git_refs: Vec::new(),
            git_refs_for: String::new(),
            hide_deletions: false,
            hide_additions: false,
            deletion_color: egui::Color32::from_rgb(0xff, 0x00, 0x00),
            addition_color: egui::Color32::from_rgb(0x00, 0x00, 0xff),
            font_paths: Vec::new(),
            package_path: String::new(),
            status: Status::Idle,
            job: None,
            preview: Preview::Idle,
            preview_job: None,
            scene_rect: egui::Rect::ZERO,
            auto_preview: true,
            last_snapshot: None,
            dirty_since: None,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_job(ui.ctx());
        self.poll_preview(ui.ctx());

        if self.mode == Mode::Git && self.git_repo != self.git_refs_for {
            self.refresh_git_refs();
        }

        // `Self::ui`'s own `ui` has no margin/background (see its doc
        // comment) -- nested panels give both. The form lives in a
        // resizable side panel so the preview on the right (see
        // `Self::preview_panel`) gets to keep most of the window.
        egui::Panel::left("form")
            .resizable(true)
            .default_size(380.0)
            .size_range(280.0..=600.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("typst-diff");
                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.mode, Mode::Files, "Files");
                        ui.selectable_value(&mut self.mode, Mode::Git, "Git revisions");
                    });
                    ui.add_space(8.0);

                    match self.mode {
                        Mode::Files => self.files_form(ui),
                        Mode::Git => self.git_form(ui),
                    }

                    ui.separator();
                    self.common_form(ui);

                    ui.separator();
                    self.status_and_generate(ui);
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            self.preview_panel(ui);
        });

        // Checked after the form above, so this frame's own edits (a
        // keystroke, a picked path, a toggled checkbox...) are already
        // reflected in `self`.
        self.maybe_auto_preview(ui.ctx());
    }
}

impl App {
    fn files_form(&mut self, ui: &mut egui::Ui) {
        file_field(ui, "Old file:", &mut self.files_old, false);
        file_field(ui, "New file:", &mut self.files_new, false);
        file_field(ui, "Output PDF:", &mut self.files_output, true);
        folder_field(ui, "Old root (optional):", &mut self.files_old_root);
        folder_field(ui, "New root (optional):", &mut self.files_new_root);
    }

    fn git_form(&mut self, ui: &mut egui::Ui) {
        folder_field(ui, "Repository:", &mut self.git_repo);

        // FILE is the same path in both revisions, so its own picker
        // just needs *a* tree to list -- old-rev's if set, new-rev's
        // otherwise (see `git_file_field`'s doc comment).
        let file_rev = if !self.git_old_rev.is_empty() {
            self.git_old_rev.as_str()
        } else {
            self.git_new_rev.as_str()
        };
        git_file_field(ui, "File (relative to repo):", &self.git_repo, file_rev, &mut self.git_file);
        file_field(ui, "Output PDF:", &mut self.git_output, true);

        rev_field(ui, "Old revision:", &mut self.git_old_rev, &self.git_refs);
        rev_field(ui, "New revision:", &mut self.git_new_rev, &self.git_refs);

        git_root_field(ui, "Old root (optional, '.' = repo root):", &self.git_repo, &self.git_old_rev, &mut self.git_old_root);
        git_root_field(ui, "New root (optional, '.' = repo root):", &self.git_repo, &self.git_new_rev, &mut self.git_new_root);
    }

    fn common_form(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.hide_deletions, "Hide deletions");
            ui.checkbox(&mut self.hide_additions, "Hide additions");
            ui.label("Deletion color:");
            ui.color_edit_button_srgba(&mut self.deletion_color);
            ui.label("Addition color:");
            ui.color_edit_button_srgba(&mut self.addition_color);
        });

        ui.horizontal(|ui| {
            ui.label("Package path (optional):");
            ui.text_edit_singleline(&mut self.package_path);
            if ui.button("Browse…").clicked() {
                let mut dialog = rfd::FileDialog::new();
                if !self.package_path.is_empty() {
                    dialog = dialog.set_directory(&self.package_path);
                }
                if let Some(path) = dialog.pick_folder() {
                    self.package_path = path.display().to_string();
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("Font paths:");
            if ui.button("Add folder…").clicked() {
                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                    self.font_paths.push(path.display().to_string());
                }
            }
        });
        let mut remove = None;
        for (i, path) in self.font_paths.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(path);
                if ui.small_button("✕").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.font_paths.remove(i);
        }
    }

    fn status_and_generate(&mut self, ui: &mut egui::Ui) {
        let running = matches!(self.status, Status::Running);
        let previewing = matches!(self.preview, Preview::Running);
        ui.horizontal(|ui| {
            if ui.add_enabled(!previewing, egui::Button::new("Preview")).clicked() {
                self.start_preview();
            }
            ui.checkbox(&mut self.auto_preview, "Auto preview")
                .on_hover_text("Re-render the preview automatically, shortly after any field changes.");
            if ui.add_enabled(!running, egui::Button::new("Generate")).clicked() {
                self.start_job();
            }
        });
        ui.horizontal(|ui| {
            match &self.status {
                Status::Idle => {}
                Status::Running => {
                    ui.spinner();
                    ui.label("Generating…");
                }
                Status::Success(path) => {
                    ui.colored_label(egui::Color32::from_rgb(0x2e, 0xa0, 0x4f), "Done.");
                    ui.label(path.display().to_string());
                    if ui.button("Open").clicked() {
                        let _ = open::that(path);
                    }
                }
                Status::Error(_) => {
                    ui.colored_label(egui::Color32::from_rgb(0xd6, 0x33, 0x33), "Failed.");
                }
            }
        });
        if let Status::Error(err) = &self.status {
            egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                ui.colored_label(egui::Color32::from_rgb(0xd6, 0x33, 0x33), err);
            });
        }
    }

    /// Builds the `FilesArgs`/`GitArgs` the current form describes and
    /// spawns `run_files`/`run_git` on a background thread -- the exact
    /// entry points the CLI uses, so the GUI can't behave differently.
    fn start_job(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        self.status = Status::Running;

        match self.mode {
            Mode::Files => {
                let args = self.build_files_args();
                std::thread::spawn(move || {
                    let output = args.output.clone();
                    let result = run_files(args)
                        .map(|()| output)
                        .map_err(|err| format!("{err:?}"));
                    let _ = tx.send(result);
                });
            }
            Mode::Git => {
                let args = self.build_git_args();
                std::thread::spawn(move || {
                    let output = args.output.clone();
                    let result = run_git(args)
                        .map(|()| output)
                        .map_err(|err| format!("{err:?}"));
                    let _ = tx.send(result);
                });
            }
        }
    }

    /// Non-blockingly checks whether the background job (if any) has
    /// finished, and updates `self.status` accordingly. Keeps repainting
    /// while a job is running so its completion shows up promptly even
    /// without further user input.
    fn poll_job(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.job else { return };
        match rx.try_recv() {
            Ok(Ok(path)) => {
                self.status = Status::Success(path);
                self.job = None;
            }
            Ok(Err(err)) => {
                self.status = Status::Error(err);
                self.job = None;
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = Status::Error("the worker thread panicked".to_string());
                self.job = None;
            }
        }
    }

    /// Renders the current `self.preview` state: an idle hint, a spinner,
    /// the error text, or every rendered page stacked vertically inside an
    /// `egui::Scene` -- a pan+zoom canvas (think a diagram editor, or any
    /// image viewer), not a plain `ScrollArea`: drag (any mouse button) or
    /// scroll to pan in any direction, Ctrl+scroll or a touchpad/touch
    /// pinch to zoom, all handled by `Scene` itself. An initial attempt
    /// built this out of `ScrollArea` plus manually watching
    /// `egui::InputState::zoom_delta()` for Ctrl+scroll, but a plain
    /// `ScrollArea` only ever scrolls vertically from the wheel unless the
    /// content overflows horizontally *and* Shift is held for that axis
    /// too -- workable for reading a page top to bottom, useless for
    /// panning around once zoomed in past the panel's width. `Scene`
    /// doesn't have that limitation (nor any scrollbars to speak of).
    fn preview_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Reset view").clicked() {
                // `Scene::show` treats a zero-sized `scene_rect` as
                // "uninitialized" and recomputes it to fit the content --
                // see its own doc comment -- so this is also what
                // `self.scene_rect` starts as, before the user has ever
                // panned/zoomed anything.
                self.scene_rect = egui::Rect::ZERO;
            }
            ui.weak("(scroll or drag to pan, Ctrl+scroll or pinch to zoom)");
        });
        ui.separator();

        match &self.preview {
            Preview::Idle => {
                ui.centered_and_justified(|ui| {
                    ui.weak("Click \"Preview\" to render the diff here.");
                });
            }
            Preview::Running => {
                ui.centered_and_justified(|ui| {
                    ui.spinner();
                });
            }
            Preview::Error(err) => {
                egui::ScrollArea::both().show(ui, |ui| {
                    ui.colored_label(egui::Color32::from_rgb(0xd6, 0x33, 0x33), err);
                });
            }
            Preview::Ready(pages) => {
                egui::Scene::new().zoom_range(0.05..=8.0).show(ui, &mut self.scene_rect, |ui| {
                    ui.vertical(|ui| {
                        for (i, page) in pages.iter().enumerate() {
                            if i > 0 {
                                ui.add_space(8.0);
                            }
                            ui.image((page.id(), page.size_vec2()));
                        }
                    });
                });
            }
        }
    }

    /// Builds the current form's `FilesArgs`/`GitArgs` and spawns a
    /// background thread that diffs, lays out, and rasterizes every page
    /// (via [`diff_and_layout`] + [`render_pages`]) -- everything
    /// "Generate" does up to producing a PDF, minus actually producing
    /// one: no file is read back afterward, and nothing is written to
    /// disk. Pixels come back as plain `ColorImage`s (see
    /// `App::preview_job`'s doc comment for why), turned into
    /// `TextureHandle`s once they arrive, in [`Self::poll_preview`].
    fn start_preview(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.preview_job = Some(rx);
        self.preview = Preview::Running;

        match self.mode {
            Mode::Files => {
                let args = self.build_files_args();
                std::thread::spawn(move || {
                    let result = (|| -> Result<Vec<egui::ColorImage>> {
                        let (world_old, world_new) = build_files_worlds(&args)?;
                        let document = diff_and_layout(&world_old, &world_new, &args.common)?;
                        Ok(render_pages(&document))
                    })()
                    .map_err(|err| format!("{err:?}"));
                    let _ = tx.send(result);
                });
            }
            Mode::Git => {
                let args = self.build_git_args();
                std::thread::spawn(move || {
                    let result = (|| -> Result<Vec<egui::ColorImage>> {
                        let (world_old, world_new) = build_git_worlds(&args)?;
                        let document = diff_and_layout(&world_old, &world_new, &args.common)?;
                        Ok(render_pages(&document))
                    })()
                    .map_err(|err| format!("{err:?}"));
                    let _ = tx.send(result);
                });
            }
        }
    }

    /// Non-blockingly checks whether `start_preview`'s background thread
    /// has produced pages yet, and, if so, uploads them as textures (only
    /// possible here, on the UI thread -- see `App::preview_job`'s doc
    /// comment) and updates `self.preview`.
    fn poll_preview(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.preview_job else { return };
        match rx.try_recv() {
            Ok(Ok(images)) => {
                let textures = images
                    .into_iter()
                    .enumerate()
                    .map(|(i, image)| {
                        ctx.load_texture(format!("preview-page-{i}"), image, egui::TextureOptions::LINEAR)
                    })
                    .collect();
                self.preview = Preview::Ready(textures);
                self.preview_job = None;
            }
            Ok(Err(err)) => {
                self.preview = Preview::Error(err);
                self.preview_job = None;
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.preview = Preview::Error("the worker thread panicked".to_string());
                self.preview_job = None;
            }
        }
    }

    fn snapshot(&self) -> FormSnapshot {
        FormSnapshot {
            mode: self.mode,
            files_old: self.files_old.clone(),
            files_new: self.files_new.clone(),
            files_old_root: self.files_old_root.clone(),
            files_new_root: self.files_new_root.clone(),
            git_repo: self.git_repo.clone(),
            git_file: self.git_file.clone(),
            git_old_rev: self.git_old_rev.clone(),
            git_new_rev: self.git_new_rev.clone(),
            git_old_root: self.git_old_root.clone(),
            git_new_root: self.git_new_root.clone(),
            hide_deletions: self.hide_deletions,
            hide_additions: self.hide_additions,
            deletion_color: self.deletion_color,
            addition_color: self.addition_color,
            font_paths: self.font_paths.clone(),
            package_path: self.package_path.clone(),
        }
    }

    /// How long to wait, after the *last* change, before actually
    /// re-rendering -- long enough that typing several characters in a
    /// row (a path, a revision...) fires one re-render, not one per
    /// keystroke.
    const AUTO_PREVIEW_DEBOUNCE: Duration = Duration::from_millis(500);

    /// Notices whether anything `Self::snapshot` covers changed this
    /// frame and, if `self.auto_preview` is on, (re-)starts the
    /// `AUTO_PREVIEW_DEBOUNCE` countdown for it; once that countdown
    /// elapses with no further change, fires `Self::start_preview`. Keeps
    /// requesting repaints while the countdown is running so it's
    /// actually checked again even without further input (mouse-move,
    /// keystroke, ...) to trigger one on its own.
    fn maybe_auto_preview(&mut self, ctx: &egui::Context) {
        let snapshot = self.snapshot();
        // `None` only on the very first frame -- nothing to compare
        // against yet, and nothing worth previewing this early either
        // (every field is still at its starting value).
        let changed = self.last_snapshot.as_ref().is_some_and(|prev| *prev != snapshot);
        self.last_snapshot = Some(snapshot);
        if changed && self.auto_preview {
            self.dirty_since = Some(Instant::now());
        }

        if let Some(since) = self.dirty_since {
            let elapsed = since.elapsed();
            if elapsed >= Self::AUTO_PREVIEW_DEBOUNCE {
                self.dirty_since = None;
                self.start_preview();
            } else {
                ctx.request_repaint_after(Self::AUTO_PREVIEW_DEBOUNCE - elapsed);
            }
        }
    }

    fn build_files_args(&self) -> FilesArgs {
        FilesArgs {
            old: PathBuf::from(&self.files_old),
            new: PathBuf::from(&self.files_new),
            output: output_path(&self.files_output),
            old_root: non_empty_path(&self.files_old_root),
            new_root: non_empty_path(&self.files_new_root),
            common: self.build_common(),
        }
    }

    fn build_git_args(&self) -> GitArgs {
        GitArgs {
            repo: PathBuf::from(&self.git_repo),
            file: PathBuf::from(&self.git_file),
            output: output_path(&self.git_output),
            old_rev: self.git_old_rev.clone(),
            new_rev: self.git_new_rev.clone(),
            old_root: non_empty_path(&self.git_old_root),
            new_root: non_empty_path(&self.git_new_root),
            common: self.build_common(),
        }
    }

    fn build_common(&self) -> CommonArgs {
        CommonArgs {
            hide_deletions: self.hide_deletions,
            hide_additions: self.hide_additions,
            deletion_color: color32_to_typst(self.deletion_color),
            addition_color: color32_to_typst(self.addition_color),
            font_paths: self.font_paths.iter().map(PathBuf::from).collect(),
            package_path: non_empty_path(&self.package_path),
        }
    }

    /// Re-lists `self.git_repo`'s branches and tags for the "Choose..."
    /// menu next to `--old-rev`/`--new-rev`. Silently empties the list on
    /// any failure (not yet a valid repository, mid-typing, etc.) rather
    /// than surfacing an error -- this is a convenience popup, not
    /// validation; an actually invalid repo path still surfaces its real
    /// error from `run_git` once "Generate" is clicked.
    fn refresh_git_refs(&mut self) {
        self.git_refs_for = self.git_repo.clone();
        self.git_refs = (|| -> Option<Vec<String>> {
            let repo = gix::open(&self.git_repo).ok()?;
            let mut names: Vec<String> = repo
                .references()
                .ok()?
                .all()
                .ok()?
                .filter_map(Result::ok)
                .map(|r| r.name().shorten().to_string())
                .collect();
            names.sort();
            names.dedup();
            Some(names)
        })()
        .unwrap_or_default();
    }
}

/// `Color32::r()/g()/b()/a()` return the *premultiplied* bytes `Color32`
/// stores internally, not the plain sRGBA the color picker shows -- fine
/// at full opacity (premultiplied == unmultiplied there), wrong at any
/// other alpha, so this goes through `to_srgba_unmultiplied()` instead.
fn color32_to_typst(c: egui::Color32) -> Color {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Color::from_u8(r, g, b, a)
}

/// Rasterizes every page of `document` (the `typst-render` crate --
/// separate from `typst-pdf`, which `write_pdf` in `main.rs` uses for the
/// real, on-disk output) into an `egui::ColorImage`, ready to be uploaded
/// as a texture (see `App::poll_preview`) once back on the UI thread.
///
/// `tiny_skia::Pixmap` (what `typst_render::render` returns) stores
/// premultiplied-alpha RGBA8 pixels, the same representation
/// `egui::Color32`/`ColorImage` use internally -- `pixmap.data()`'s bytes
/// can be handed to `from_rgba_premultiplied` as-is, no conversion.
fn render_pages(document: &PagedDocument) -> Vec<egui::ColorImage> {
    let options = typst_render::RenderOptions::default();
    document
        .pages()
        .iter()
        .map(|page| {
            let pixmap = typst_render::render(page, &options);
            egui::ColorImage::from_rgba_premultiplied(
                [pixmap.width() as usize, pixmap.height() as usize],
                pixmap.data(),
            )
        })
        .collect()
}

fn non_empty_path(s: &str) -> Option<PathBuf> {
    if s.trim().is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

fn output_path(s: &str) -> PathBuf {
    if s.trim().is_empty() {
        PathBuf::from("diff.pdf")
    } else {
        PathBuf::from(s)
    }
}

/// A labeled single-line text field for a real filesystem file, plus a
/// "Browse…" button (a save dialog when `save`, an open dialog otherwise).
fn file_field(ui: &mut egui::Ui, label: &str, value: &mut String, save: bool) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(value);
        if ui.button("Browse…").clicked() {
            let mut dialog = rfd::FileDialog::new().add_filter("Typst/PDF", &["typ", "pdf"]);
            if let Some(dir) = existing_parent(value) {
                dialog = dialog.set_directory(dir);
            }
            let picked = if save { dialog.save_file() } else { dialog.pick_file() };
            if let Some(path) = picked {
                *value = path.display().to_string();
            }
        }
    });
}

/// A labeled single-line text field for a real filesystem directory, plus
/// a "Browse…" folder picker button.
fn folder_field(ui: &mut egui::Ui, label: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(value);
        if ui.button("Browse…").clicked() {
            let mut dialog = rfd::FileDialog::new();
            if !value.is_empty() {
                dialog = dialog.set_directory(&value);
            }
            if let Some(path) = dialog.pick_folder() {
                *value = path.display().to_string();
            }
        }
    });
}

/// `git` mode's `FILE` field: a plain text field (a path relative to the
/// repository, the same across both revisions), plus a "Choose…" menu
/// listing every `.typ` file actually found in `rev`'s tree. Unlike
/// [`file_field`], this can't be a real filesystem dialog: `FILE` is read
/// directly out of git's object database at a specific revision (see the
/// module doc comment), which may have no matching checkout on disk at
/// all -- listing `rev`'s tree (via [`list_git_paths`]) is the only way
/// to show what's actually there. `rev` is picked by the caller (`FILE`
/// has no revision of its own) -- see `App::git_form`.
fn git_file_field(ui: &mut egui::Ui, label: &str, repo: &str, rev: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(value);
        ui.menu_button("Choose…", |ui| {
            if repo.is_empty() || rev.is_empty() {
                ui.label("(set the repository and a revision first)");
                return;
            }
            let files = list_git_paths(repo, rev).files;
            if files.is_empty() {
                ui.label(format!("(no .typ file found at {rev:?})"));
            }
            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                for path in &files {
                    if ui.button(path).clicked() {
                        *value = path.clone();
                        ui.close();
                    }
                }
            });
        });
    });
}

/// Like [`git_file_field`], but for a directory (`--old-root`/
/// `--new-root` in `git` mode) -- `rev` is that same field's own
/// `--old-rev`/`--new-rev` here (unlike `FILE`, each root has one
/// obvious revision to list from). The repository root itself is offered
/// as `.`, matching `--old-root`/`--new-root`'s shorthand for it.
fn git_root_field(ui: &mut egui::Ui, label: &str, repo: &str, rev: &str, value: &mut String) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(egui::TextEdit::singleline(value).hint_text("defaults to FILE's parent"));
        ui.menu_button("Choose…", |ui| {
            if repo.is_empty() || rev.is_empty() {
                ui.label("(set the repository and this revision first)");
                return;
            }
            let dirs = list_git_paths(repo, rev).dirs;
            egui::ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                for dir in &dirs {
                    let shown = if dir.is_empty() { "." } else { dir.as_str() };
                    if ui.button(shown).clicked() {
                        *value = shown.to_string();
                        ui.close();
                    }
                }
            });
        });
    });
}

/// A revision text field (`--old-rev`/`--new-rev`), plus a "Choose…" menu
/// listing `refs` (branches/tags already found in the repository -- see
/// `App::refresh_git_refs`) to fill it in without having to remember/type
/// an exact name. Typing an arbitrary commit (not necessarily in `refs`)
/// still works, since this is a plain text field underneath.
fn rev_field(ui: &mut egui::Ui, label: &str, value: &mut String, refs: &[String]) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.text_edit_singleline(value);
        ui.menu_button("Choose…", |ui| {
            if refs.is_empty() {
                ui.label("(no branches/tags found)");
            }
            for name in refs {
                if ui.button(name).clicked() {
                    *value = name.clone();
                    ui.close();
                }
            }
        });
    });
}

/// Paths found by walking a git revision's tree (see [`list_git_paths`]):
/// `.typ` files (for [`git_file_field`]) and directories, including the
/// repository root itself as `""` (for [`git_root_field`]) -- both
/// relative to the repository root, sorted.
#[derive(Default)]
struct GitPaths {
    files: Vec<String>,
    dirs: Vec<String>,
}

/// Opens `repo_path` and recursively walks the tree `rev` resolves to
/// (the same way `Backend::Git`/`SimpleWorld::from_git` in `world.rs`
/// resolve `--old-rev`/`--new-rev`), collecting every path in it. Empty
/// on any failure (repo not found, rev doesn't resolve, ...): like
/// `App::refresh_git_refs`, this is a convenience picker, not
/// validation -- an actually invalid repo/rev still surfaces its real
/// error from `run_git` once "Generate" is clicked.
fn list_git_paths(repo_path: &str, rev: &str) -> GitPaths {
    (|| -> Option<GitPaths> {
        let repo = gix::open(repo_path).ok()?;
        let tree = repo
            .rev_parse_single(rev)
            .ok()?
            .object()
            .ok()?
            .into_commit()
            .tree()
            .ok()?;
        let mut paths = GitPaths { files: Vec::new(), dirs: vec![String::new()] };
        walk_git_tree(&tree, "", &mut paths);
        paths.files.sort();
        paths.dirs.sort();
        Some(paths)
    })()
    .unwrap_or_default()
}

/// Recursion for [`list_git_paths`]: `prefix` is `tree`'s own path
/// relative to the repository root (`""` for the root tree itself).
/// Submodules (a `commit`-mode entry) are neither listed nor descended
/// into -- they're a separate repository, with no tree of their own to
/// read from this one.
fn walk_git_tree(tree: &gix::Tree<'_>, prefix: &str, out: &mut GitPaths) {
    for entry in tree.iter().filter_map(Result::ok) {
        let name = entry.filename().to_string();
        let path = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let mode = entry.mode();
        if mode.is_tree() {
            out.dirs.push(path.clone());
            if let Ok(object) = entry.object() {
                walk_git_tree(&object.into_tree(), &path, out);
            }
        } else if mode.is_blob() && path.ends_with(".typ") {
            out.files.push(path);
        }
    }
}

/// The existing parent directory of `value` (a path the user may still be
/// typing), if any -- used to start a file dialog somewhere sensible
/// instead of always at the process's current directory. A bare filename
/// with no directory component (`"Cargo.toml"`) gives `None`, even if it
/// exists right there in the current directory: its `parent()` is `""`,
/// which doesn't stat as a directory -- an accepted quirk, not something
/// worth resolving against the current directory just for this hint.
fn existing_parent(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    let parent = path.parent()?;
    parent.is_dir().then(|| parent.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_git_tree_finds_typ_files_and_directories() {
        let repo = gix::open("examples/git-project").expect("test repo present");
        let tree = repo
            .rev_parse_single("example-old")
            .expect("example-old branch present")
            .object()
            .unwrap()
            .into_commit()
            .tree()
            .unwrap();
        let mut paths = GitPaths { files: Vec::new(), dirs: vec![String::new()] };
        walk_git_tree(&tree, "", &mut paths);
        paths.files.sort();
        paths.dirs.sort();

        assert!(paths.files.contains(&"src/main.typ".to_string()));
        assert!(paths.files.contains(&"lib/report.typ".to_string()));
        assert!(paths.dirs.contains(&String::new()), "repo root should be listed as \"\"");
        assert!(paths.dirs.contains(&"src".to_string()));
        assert!(paths.dirs.contains(&"lib".to_string()));
        // data.json/items.json aren't .typ files -- shouldn't show up as
        // choosable entry points.
        assert!(!paths.files.iter().any(|f| f.ends_with(".json")));
    }

    #[test]
    fn list_git_paths_is_empty_for_an_unresolvable_repo_or_revision() {
        assert!(list_git_paths("/definitely/not/a/repo", "main").files.is_empty());
        assert!(list_git_paths("examples/git-project", "no-such-rev").files.is_empty());
    }

    #[test]
    fn non_empty_path_treats_blank_as_none() {
        assert_eq!(non_empty_path(""), None);
        assert_eq!(non_empty_path("   "), None);
        assert_eq!(non_empty_path("examples/old"), Some(PathBuf::from("examples/old")));
    }

    #[test]
    fn output_path_defaults_to_diff_pdf() {
        assert_eq!(output_path(""), PathBuf::from("diff.pdf"));
        assert_eq!(output_path("custom.pdf"), PathBuf::from("custom.pdf"));
    }

    #[test]
    fn color32_to_typst_preserves_channels() {
        // Exact for full opacity: no premultiply/unmultiply rounding to
        // lose a channel's low bit over.
        let opaque = color32_to_typst(egui::Color32::from_rgba_unmultiplied(0x12, 0x34, 0x56, 0xff));
        assert_eq!(opaque.to_vec4_u8(), [0x12, 0x34, 0x56, 0xff]);

        // At partial opacity, `Color32`'s premultiplied storage makes an
        // exact round trip impossible -- only that it stays close (off by
        // at most 1 per channel, from integer rounding both ways) is
        // `color32_to_typst`'s to guarantee.
        let translucent =
            color32_to_typst(egui::Color32::from_rgba_unmultiplied(0x12, 0x34, 0x56, 0x78));
        let [r, g, b, a] = translucent.to_vec4_u8();
        assert!(r.abs_diff(0x12) <= 1 && g.abs_diff(0x34) <= 1 && b.abs_diff(0x56) <= 1);
        assert_eq!(a, 0x78);
    }

    #[test]
    fn existing_parent_requires_a_real_directory() {
        assert_eq!(existing_parent(""), None);
        assert_eq!(existing_parent("/definitely/not/a/real/path/file.typ"), None);
        // A bare filename's parent is "" (not "."), and "" doesn't stat as
        // a directory even when the file itself exists in the current
        // one -- an accepted quirk (see this fn's doc comment), not
        // something `existing_parent` tries to special-case.
        assert_eq!(existing_parent("Cargo.toml"), None);
        assert_eq!(existing_parent("src/main.rs"), Some(PathBuf::from("src")));
    }
}
