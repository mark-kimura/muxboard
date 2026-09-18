#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod projects;
mod tmux;

use eframe::egui;
use projects::{Project, Settings, SortMode};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tmux::{Session, Window};

const REFRESH_EVERY: Duration = Duration::from_millis(1000);
const RED: egui::Color32 = egui::Color32::from_rgb(220, 80, 80);
const GREEN: egui::Color32 = egui::Color32::from_rgb(90, 190, 110);
const YELLOW: egui::Color32 = egui::Color32::from_rgb(230, 190, 60);
/// Window width when only the project list is shown, and the full size when the detail panel is open.
const DEFAULT_LIST_WIDTH: f32 = 300.0;
const MIN_LIST_WIDTH: f32 = 180.0;
const TAB_WIDTH: f32 = 22.0;
const MIN_DETAIL_WIDTH: f32 = 420.0;
const EXPANDED_SIZE: egui::Vec2 = egui::vec2(1000.0, 640.0);

/// The one modal dialog that can be open at a time.
enum Dialog {
    EditProject { path: PathBuf, name: String, folder: String, command: String },
    Settings(Settings),
    RemoveProject { path: PathBuf },
    RenameSession { old: String, text: String },
    RenameWindow { session: String, index: u32, text: String },
    NewWindow { session: String, name: String },
    KillSession(String),
    KillWindow { session: String, index: u32 },
}

#[derive(Default)]
struct App {
    projects: Vec<Project>,
    settings: Settings,
    sessions: Vec<Session>,
    windows: HashMap<String, Vec<Window>>,
    /// project root (normalized) -> session name, for projects that have a running session
    matched: HashMap<PathBuf, String>,

    selected_project: Option<PathBuf>,
    selected: Option<String>,
    selected_window: Option<u32>,
    collapsed: HashSet<String>,

    preview: String,
    command: String,
    last_refresh: Option<Instant>,
    dialog: Option<Dialog>,
    status: Option<(String, bool)>,

    /// Whether the right-hand detail panel is shown. Off by default: the window is just the project list.
    detail_open: bool,
    /// Window size to restore when the detail panel is shown again.
    expanded_size: egui::Vec2,
    /// Width of the project list. Changed by dragging the divider (expanded) or resizing the window (collapsed).
    list_width: f32,
    /// Window width requested by the last toggle, until the window actually reaches it.
    /// While pending, the list is drawn at its remembered width and that width is not updated.
    pending_width: Option<(f32, u32)>,
    /// Window width being dragged to via the edge tab (collapsed mode only).
    drag_target: Option<f32>,
}

impl App {
    fn new() -> Self {
        App {
            projects: projects::load(),
            settings: projects::load_settings(),
            expanded_size: EXPANDED_SIZE,
            list_width: DEFAULT_LIST_WIDTH,
            // MUXDOCK_EXPANDED=1 starts with the preview panel open.
            detail_open: std::env::var("MUXDOCK_EXPANDED").map(|v| v == "1").unwrap_or(false),
            ..Default::default()
        }
    }

    // ---------- data ----------

    fn note(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), false));
    }
    fn fail(&mut self, msg: impl Into<String>) {
        self.status = Some((msg.into(), true));
    }
    fn apply(&mut self, res: Result<(), String>, ok_msg: &str) {
        match res {
            Ok(()) => self.note(ok_msg),
            Err(e) => self.fail(e),
        }
        self.refresh();
    }

    fn save_projects(&mut self) {
        if let Err(e) = projects::save(&self.projects) {
            self.fail(e);
        }
    }

    fn refresh(&mut self) {
        self.last_refresh = Some(Instant::now());
        match tmux::list_sessions() {
            Ok(s) => self.sessions = s,
            Err(e) => {
                self.sessions.clear();
                self.fail(e);
            }
        }
        self.windows.clear();
        for (s, w) in tmux::list_all_windows().unwrap_or_default() {
            self.windows.entry(s).or_default().push(w);
        }

        // Match sessions to projects by start folder.
        self.matched.clear();
        let by_path: HashMap<PathBuf, String> = self
            .sessions
            .iter()
            .map(|s| (projects::normalize(std::path::Path::new(&s.path)), s.name.clone()))
            .collect();
        for p in &self.projects {
            let key = projects::normalize(&p.path);
            if let Some(name) = by_path.get(&key) {
                self.matched.insert(key, name.clone());
            }
        }

        // Keep the selection consistent with what exists now.
        if let Some(pp) = &self.selected_project {
            if self.projects.iter().any(|p| projects::normalize(&p.path) == *pp) {
                let now = self.matched.get(pp).cloned();
                if now != self.selected {
                    self.selected_window = None;
                }
                self.selected = now;
            } else {
                self.selected_project = None;
                self.selected = None;
                self.selected_window = None;
            }
        } else if let Some(sel) = &self.selected {
            if !self.sessions.iter().any(|s| &s.name == sel) {
                self.selected = None;
                self.selected_window = None;
            }
        }
        if self.selected_project.is_none() && self.selected.is_none() {
            if let Some(p) = self.projects.first() {
                let key = projects::normalize(&p.path);
                self.selected = self.matched.get(&key).cloned();
                self.selected_project = Some(key);
            } else {
                self.selected = self.sessions.first().map(|s| s.name.clone());
            }
        }
        if let (Some(s), Some(w)) = (&self.selected, self.selected_window) {
            if !self.windows_of(s).iter().any(|x| x.index == w) {
                self.selected_window = None;
            }
        }

        self.preview = match (&self.selected, self.selected_window) {
            (Some(s), Some(w)) => tmux::capture_pane(&format!("{s}:{w}")).unwrap_or_default(),
            (Some(s), None) => tmux::capture_pane(s).unwrap_or_default(),
            (None, _) => String::new(),
        };
    }

    fn windows_of(&self, session: &str) -> &[Window] {
        self.windows.get(session).map(|v| v.as_slice()).unwrap_or(&[])
    }

    fn session(&self, name: &str) -> Option<&Session> {
        self.sessions.iter().find(|s| s.name == name)
    }

    fn project_at(&self, key: &PathBuf) -> Option<&Project> {
        self.projects.iter().find(|p| projects::normalize(&p.path) == *key)
    }

    /// The command to type into a new session for this project.
    fn command_for(&self, p: &Project) -> String {
        let c = p.command.trim();
        if c.is_empty() { self.settings.start_command.trim().to_string() } else { c.to_string() }
    }

    /// Projects in display order, each with its index in the stored list.
    fn ordered_projects(&self) -> Vec<(usize, Project)> {
        let mut v: Vec<(usize, Project)> = self.projects.iter().cloned().enumerate().collect();
        match self.settings.sort {
            SortMode::Manual => {}
            SortMode::Alphabetical => v.sort_by_key(|(_, p)| p.name.to_lowercase()),
            SortMode::Status => v.sort_by_key(|(_, p)| {
                let key = projects::normalize(&p.path);
                match self.matched.get(&key).and_then(|n| self.session(n)) {
                    Some(s) if s.attached => 0,
                    Some(_) => 1,
                    None => 2,
                }
            }),
        }
        v
    }

    fn set_sort(&mut self, mode: SortMode) {
        if self.settings.sort != mode {
            self.settings.sort = mode;
            if let Err(e) = projects::save_settings(&self.settings) {
                self.fail(e);
            }
        }
    }

    fn sort_menu(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Order projects by").weak().small());
        let mut mode = self.settings.sort;
        ui.radio_value(&mut mode, SortMode::Status, "Status: terminal open, running, idle");
        ui.radio_value(&mut mode, SortMode::Alphabetical, "Name");
        ui.radio_value(&mut mode, SortMode::Manual, "Manual (drag rows to reorder)");
        if mode != self.settings.sort {
            self.set_sort(mode);
            ui.close_menu();
        }
    }

    /// Move the project at stored index `from` so that it lands at display position `to`
    /// (manual mode only, where display order equals stored order).
    fn move_project(&mut self, from: usize, mut to: usize) {
        if from >= self.projects.len() {
            return;
        }
        let p = self.projects.remove(from);
        if from < to {
            to -= 1;
        }
        let to = to.min(self.projects.len());
        self.projects.insert(to, p);
        self.save_projects();
    }

    fn other_sessions(&self) -> Vec<Session> {
        let taken: HashSet<&String> = self.matched.values().collect();
        self.sessions.iter().filter(|s| !taken.contains(&s.name)).cloned().collect()
    }

    fn select_project(&mut self, key: PathBuf, window: Option<u32>) {
        let session = self.matched.get(&key).cloned();
        let changed = self.selected_project.as_ref() != Some(&key) || self.selected != session || self.selected_window != window;
        self.selected_project = Some(key);
        self.selected = session;
        self.selected_window = window;
        if changed {
            self.refresh();
        }
    }

    fn select_session(&mut self, name: &str, window: Option<u32>) {
        let changed = self.selected_project.is_some() || self.selected.as_deref() != Some(name) || self.selected_window != window;
        self.selected_project = None;
        self.selected = Some(name.to_string());
        self.selected_window = window;
        if changed {
            self.refresh();
        }
    }

    // ---------- actions ----------

    /// Show the session (or one of its windows) in a terminal. If a terminal is already open on the
    /// session and a window is named, the open terminal is switched to that window instead of
    /// opening a second terminal; every terminal on a session shows the same active window.
    fn open_terminal(&mut self, session: &str, window: Option<u32>) {
        let attached = self.session(session).map(|s| s.attached).unwrap_or(false);
        if let Some(w) = window {
            let r = tmux::select_window(&format!("{session}:{w}"));
            if attached {
                self.apply(r, &format!("Switched the open terminal to window {w}"));
                return;
            }
            if let Err(e) = r {
                self.fail(e);
                return;
            }
        }
        let r = tmux::attach_in_terminal(session, Some(&self.settings.terminal));
        self.apply(r, "Opened a terminal");
    }

    fn make_active(&mut self, session: &str, index: u32) {
        let r = tmux::select_window(&format!("{session}:{index}"));
        self.apply(r, &format!("Window {index} is now active"));
    }

    /// Create the project's tmux session in its folder, start Claude Code in it, open a terminal.
    fn start_project(&mut self, key: &PathBuf) {
        let Some(p) = self.project_at(key).cloned() else { return };
        let mut name = p.session_name();
        let mut n = 2;
        while self.session(&name).is_some() {
            name = format!("{}-{n}", p.session_name());
            n += 1;
        }
        let path = p.path.to_string_lossy().into_owned();
        let command = self.command_for(&p);
        let terminal = self.settings.terminal.clone();
        let r = tmux::new_session(&name, Some(&path))
            .and_then(|_| if command.is_empty() { Ok(()) } else { tmux::send_line(&name, &command) })
            .and_then(|_| tmux::attach_in_terminal(&name, Some(&terminal)));
        if r.is_ok() {
            self.selected_project = Some(key.clone());
            self.selected = Some(name.clone());
            self.selected_window = None;
        }
        self.apply(r, &format!("Started “{}”", p.name));
    }

    fn add_project_dialog(&mut self) {
        let Some(folder) = rfd::FileDialog::new().set_title("Choose a project folder").pick_folder() else {
            return;
        };
        self.add_project(folder, None);
    }

    /// Register a folder as a project. `name` overrides the default (the folder name).
    fn add_project(&mut self, folder: PathBuf, name: Option<String>) {
        let key = projects::normalize(&folder);
        if self.projects.iter().any(|p| projects::normalize(&p.path) == key) {
            self.fail("That folder is already registered");
            return;
        }
        let mut p = Project::from_path(folder);
        if let Some(n) = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()) {
            p.name = n;
        }
        let name = p.name.clone();
        self.projects.push(p);
        self.save_projects();
        self.selected_project = Some(key);
        self.selected = None;
        self.selected_window = None;
        self.apply(Ok(()), &format!("Added project “{name}”"));
    }

    fn toggle_detail(&mut self, ctx: &egui::Context) {
        let current = ctx.input(|i| i.viewport().inner_rect.map(|r| r.size()));
        if self.detail_open {
            if let Some(sz) = current {
                self.expanded_size = sz;
            }
            self.detail_open = false;
            let h = current.map(|s| s.y).unwrap_or(EXPANDED_SIZE.y);
            let w = self.list_width + TAB_WIDTH;
            self.pending_width = Some((w, 0));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
        } else {
            self.detail_open = true;
            let mut size = self.expanded_size;
            size.x = size.x.max(self.list_width + TAB_WIDTH + MIN_DETAIL_WIDTH);
            self.pending_width = Some((size.x, 0));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }
    }

    /// Open a folder in the desktop's default file manager: Finder on macOS, xdg-open elsewhere.
    fn open_folder(&mut self, path: &std::path::Path) {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let r = std::process::Command::new(opener)
            .arg(path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("could not run {opener}: {e}"));
        match r {
            Ok(()) => self.note(format!("Opened {}", path.display())),
            Err(e) => self.fail(e),
        }
    }

    // ---------- menus ----------

    fn project_menu(&mut self, ui: &mut egui::Ui, key: &PathBuf) {
        let session = self.matched.get(key).cloned();
        match &session {
            Some(name) => {
                self.session_menu(ui, name, false);
            }
            None => {
                if ui.button("▶  Start session").clicked() {
                    self.start_project(key);
                    ui.close_menu();
                }
            }
        }
        if ui.button("📁  Open folder").clicked() {
            if let Some(path) = self.project_at(key).map(|p| p.path.clone()) {
                self.open_folder(&path);
            }
            ui.close_menu();
        }
        ui.separator();
        if ui.button("✏  Edit project…").clicked() {
            if let Some(p) = self.project_at(key).cloned() {
                self.dialog = Some(Dialog::EditProject {
                    path: key.clone(),
                    name: p.name,
                    folder: p.path.display().to_string(),
                    command: p.command,
                });
            }
            ui.close_menu();
        }
        if ui.button("－  Remove from list…").clicked() {
            self.dialog = Some(Dialog::RemoveProject { path: key.clone() });
            ui.close_menu();
        }
    }

    /// `standalone` is true for a session that belongs to no project; then the folder item is shown here.
    fn session_menu(&mut self, ui: &mut egui::Ui, name: &str, standalone: bool) {
        let attached = self.session(name).map(|s| s.attached).unwrap_or(false);
        if ui.button("▶  Open in terminal").clicked() {
            self.open_terminal(name, None);
            ui.close_menu();
        }
        if ui.button("+  New window").clicked() {
            self.dialog = Some(Dialog::NewWindow { session: name.into(), name: String::new() });
            ui.close_menu();
        }
        if self.selected_project.is_none() && ui.button("✏  Rename session…").clicked() {
            self.dialog = Some(Dialog::RenameSession { old: name.into(), text: name.into() });
            ui.close_menu();
        }
        if let Some(path) = self.session(name).map(|s| s.path.clone()).filter(|_| standalone) {
            if !path.is_empty() && ui.button("📁  Open folder").clicked() {
                self.open_folder(std::path::Path::new(&path));
                ui.close_menu();
            }
            if !path.is_empty()
                && ui
                    .button("+  Add as project")
                    .on_hover_text(format!("Register {path} as a project named “{name}”"))
                    .clicked()
            {
                self.add_project(PathBuf::from(&path), Some(name.to_string()));
                ui.close_menu();
            }
        }
        if attached && ui.button("⏏  Close terminals (keep running)").clicked() {
            let r = tmux::detach_clients(name);
            self.apply(r, "Terminals closed; the session keeps running");
            ui.close_menu();
        }
        ui.separator();
        if ui.button(egui::RichText::new("✖  Kill session…").color(RED)).clicked() {
            self.dialog = Some(Dialog::KillSession(name.into()));
            ui.close_menu();
        }
    }

    fn window_menu(&mut self, ui: &mut egui::Ui, session: &str, w: &Window) {
        if ui.button("▶  Show in terminal").clicked() {
            self.open_terminal(session, Some(w.index));
            ui.close_menu();
        }
        if ui.add_enabled(!w.active, egui::Button::new("★  Make active")).clicked() {
            self.make_active(session, w.index);
            ui.close_menu();
        }
        if ui.button("✏  Rename…").clicked() {
            self.dialog = Some(Dialog::RenameWindow { session: session.into(), index: w.index, text: w.name.clone() });
            ui.close_menu();
        }
        ui.separator();
        if ui.button(egui::RichText::new("✖  Kill window…").color(RED)).clicked() {
            self.dialog = Some(Dialog::KillWindow { session: session.into(), index: w.index });
            ui.close_menu();
        }
    }

    // ---------- left: tree ----------

    fn tree(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.heading("Projects");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("⚙").on_hover_text("Settings").clicked() {
                    self.dialog = Some(Dialog::Settings(self.settings.clone()));
                }
                if ui.button("+ Add project").clicked() {
                    self.add_project_dialog();
                }
            });
        });
        ui.add_space(4.0);
        ui.separator();

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            if self.projects.is_empty() {
                ui.add_space(8.0);
                ui.label(egui::RichText::new("No projects yet.").weak());
                ui.label(egui::RichText::new("Use “Add project” to register a folder.").weak());
            }
            let manual = self.settings.sort == SortMode::Manual;
            let ordered = self.ordered_projects();
            // Top edge of each project row, in display order, plus the bottom of the last one:
            // used to place the insertion line while dragging.
            let mut edges: Vec<f32> = Vec::new();
            let mut list_x = ui.max_rect().x_range();
            for (stored_idx, p) in &ordered {
                let key = projects::normalize(&p.path);
                let session = self.matched.get(&key).cloned();
                let top = ui.cursor().top();
                edges.push(top);
                if manual {
                    let id = egui::Id::new(("project_drag", &p.path));
                    let r = ui.dnd_drag_source(id, *stored_idx, |ui| {
                        self.tree_project_row(ui, p, &key, session.as_deref());
                    });
                    list_x = r.response.rect.x_range();
                } else {
                    self.tree_project_row(ui, p, &key, session.as_deref());
                }
                if let Some(name) = &session {
                    if !self.collapsed.contains(name) {
                        let wins = self.windows_of(name).to_vec();
                        for w in &wins {
                            self.tree_window_row(ui, name, w, Some(&key));
                        }
                    }
                }
                ui.add_space(2.0);
            }
            edges.push(ui.cursor().top());

            if manual && !ordered.is_empty() {
                let pointer_y = ui.ctx().pointer_interact_pos().map(|p| p.y);
                let slot = |y: f32| -> usize {
                    // Insert before the first row whose middle is below the pointer.
                    (0..ordered.len())
                        .find(|&i| y < (edges[i] + edges[i + 1]) / 2.0)
                        .unwrap_or(ordered.len())
                };
                let dragging = egui::DragAndDrop::has_payload_of_type::<usize>(ui.ctx());
                let released = ui.ctx().input(|i| i.pointer.any_released());
                if dragging {
                    if let Some(y) = pointer_y {
                        let i = slot(y);
                        ui.painter().hline(list_x, edges[i], egui::Stroke::new(2.0, ui.visuals().selection.bg_fill));
                    }
                    if released {
                        if let (Some(from), Some(y)) = (egui::DragAndDrop::take_payload::<usize>(ui.ctx()), pointer_y) {
                            self.move_project(*from, slot(y));
                        }
                    }
                }
            }

            let others = self.other_sessions();
            if !others.is_empty() {
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Unassigned tmux sessions").weak());
                ui.separator();
                for s in &others {
                    self.tree_session_row(ui, s);
                    if !self.collapsed.contains(&s.name) {
                        let wins = self.windows_of(&s.name).to_vec();
                        for w in &wins {
                            self.tree_window_row(ui, &s.name, w, None);
                        }
                    }
                    ui.add_space(2.0);
                }
            }

            // Whatever space is left below the list: right-click for the ordering menu.
            let leftover = egui::vec2(ui.available_width(), ui.available_height().max(60.0));
            let (_, resp) = ui.allocate_exact_size(leftover, egui::Sense::click());
            resp.context_menu(|ui| self.sort_menu(ui));
        });
    }

    fn status_dot(ui: &mut egui::Ui, session: Option<&Session>) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 18.0), egui::Sense::hover());
        let c = rect.center();
        match session {
            Some(s) if s.attached => {
                ui.painter().circle_filled(c, 4.5, GREEN);
            }
            Some(_) => {
                ui.painter().circle_filled(c, 4.5, YELLOW);
            }
            None => {
                ui.painter().circle_stroke(c, 4.5, egui::Stroke::new(1.0, ui.visuals().weak_text_color()));
            }
        }
    }

    fn row_layout() -> egui::Layout {
        egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min).with_main_justify(true)
    }

    fn tree_project_row(&mut self, ui: &mut egui::Ui, p: &Project, key: &PathBuf, session: Option<&str>) {
        let is_sel = self.selected_project.as_ref() == Some(key) && self.selected_window.is_none();
        let sess = session.and_then(|n| self.session(n)).cloned();
        let open = session.map(|n| !self.collapsed.contains(n)).unwrap_or(false);
        let mut toggle = false;
        let mut clicked = false;
        let mut double = false;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if session.is_some() {
                let arrow = if open { "⏷" } else { "⏵" };
                if ui.add(egui::Button::new(arrow).frame(false)).clicked() {
                    toggle = true;
                }
            } else {
                // Reserve exactly the arrow's width so rows line up whether or not they expand.
                ui.add_visible(false, egui::Button::new("⏷").frame(false));
            }
            Self::status_dot(ui, sess.as_ref());
            // Running projects in full strength; not-running ones dimmed.
            let mut text = egui::RichText::new(&p.name).strong();
            if session.is_none() {
                text = egui::RichText::new(&p.name).weak();
            }
            let hover = match &sess {
                Some(s) => format!(
                    "{}\n{}\n{} window{}\n\nDouble-click: open in terminal\nRight-click: more",
                    p.path.display(),
                    if s.attached { "Open in a terminal" } else { "No terminal open" },
                    s.windows,
                    if s.windows == 1 { "" } else { "s" },
                ),
                None => format!(
                    "{}\nNot running\n\nDouble-click: start a session here\nRight-click: more",
                    p.path.display()
                ),
            };
            let hover = if self.settings.sort == SortMode::Manual { format!("{hover}\nDrag: reorder") } else { hover };
            let resp = ui
                .with_layout(Self::row_layout(), |ui| ui.add(egui::SelectableLabel::new(is_sel, text)))
                .inner
                .on_hover_text(hover);
            if resp.clicked() {
                clicked = true;
            }
            if resp.double_clicked() {
                double = true;
            }
            resp.context_menu(|ui| self.project_menu(ui, key));
        });

        if toggle {
            if let Some(n) = session {
                if open {
                    self.collapsed.insert(n.to_string());
                } else {
                    self.collapsed.remove(n);
                }
            }
        }
        if clicked {
            self.select_project(key.clone(), None);
        }
        if double {
            match session {
                Some(n) => self.open_terminal(n, None),
                None => self.start_project(key),
            }
        }
    }

    fn tree_session_row(&mut self, ui: &mut egui::Ui, s: &Session) {
        let is_sel = self.selected_project.is_none() && self.selected.as_deref() == Some(&s.name) && self.selected_window.is_none();
        let open = !self.collapsed.contains(&s.name);
        let mut toggle = false;
        let mut clicked = false;
        let mut double = false;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let arrow = if open { "⏷" } else { "⏵" };
            if ui.add(egui::Button::new(arrow).frame(false)).clicked() {
                toggle = true;
            }
            Self::status_dot(ui, Some(s));
            let text = egui::RichText::new(&s.name).strong();
            let resp = ui
                .with_layout(Self::row_layout(), |ui| ui.add(egui::SelectableLabel::new(is_sel, text)))
                .inner
                .on_hover_text(format!(
                    "{}\n{}\n{} window{}\nCreated {}\n\nDouble-click: open in terminal\nRight-click: more",
                    s.path,
                    if s.attached { "Open in a terminal" } else { "No terminal open" },
                    s.windows,
                    if s.windows == 1 { "" } else { "s" },
                    s.created
                ));
            if resp.clicked() {
                clicked = true;
            }
            if resp.double_clicked() {
                double = true;
            }
            let name = s.name.clone();
            resp.context_menu(|ui| self.session_menu(ui, &name, true));
        });

        if toggle {
            if open {
                self.collapsed.insert(s.name.clone());
            } else {
                self.collapsed.remove(&s.name);
            }
        }
        if clicked {
            self.select_session(&s.name, None);
        }
        if double {
            self.open_terminal(&s.name, None);
        }
    }

    fn tree_window_row(&mut self, ui: &mut egui::Ui, session: &str, w: &Window, project: Option<&PathBuf>) {
        let is_sel = self.selected.as_deref() == Some(session) && self.selected_window == Some(w.index);
        let mut clicked = false;
        let mut double = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add_space(30.0);
            // Active window: small filled square in the selection colour. Others: faint outline.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 18.0), egui::Sense::hover());
            let sq = egui::Rect::from_center_size(rect.center(), egui::vec2(7.0, 7.0));
            if w.active {
                ui.painter().rect_filled(sq, 1.0, ui.visuals().selection.bg_fill);
            } else {
                ui.painter().rect_stroke(sq, 1.0, egui::Stroke::new(1.0, ui.visuals().weak_text_color()));
            }
            let mut text = egui::RichText::new(&w.name);
            if !w.active {
                text = text.weak();
            }
            let resp = ui
                .with_layout(Self::row_layout(), |ui| ui.add(egui::SelectableLabel::new(is_sel, text)))
                .inner
                .on_hover_text(format!(
                    "Window {}: {}\nRunning: {}{}\n\nDouble-click: show this window in the terminal\nRight-click: more",
                    w.index,
                    w.name,
                    w.command,
                    if w.active { "\nThis is the window the terminal shows" } else { "" }
                ));
            if resp.clicked() {
                clicked = true;
            }
            if resp.double_clicked() {
                double = true;
            }
            resp.context_menu(|ui| self.window_menu(ui, session, w));
        });
        if clicked {
            match project {
                Some(k) => self.select_project(k.clone(), Some(w.index)),
                None => self.select_session(session, Some(w.index)),
            }
        }
        if double {
            self.open_terminal(session, Some(w.index));
        }
    }

    // ---------- right: detail ----------

    fn detail(&mut self, ui: &mut egui::Ui) {
        let project = self.selected_project.clone().and_then(|k| self.project_at(&k).cloned().map(|p| (k, p)));
        let name = self.selected.clone();

        // A project with no running session
        if let (Some((key, p)), None) = (&project, &name) {
            ui.add_space(4.0);
            ui.heading(&p.name);
            ui.label(egui::RichText::new(p.path.display().to_string()).weak());
            ui.label(egui::RichText::new("Not running").weak());
            ui.add_space(8.0);
            let command = self.command_for(p);
            ui.horizontal(|ui| {
                if ui.add(egui::Button::new(egui::RichText::new("▶  Start session").strong())).clicked() {
                    self.start_project(key);
                }
                ui.menu_button("Actions ⏷", |ui| self.project_menu(ui, key));
            });
            ui.add_space(24.0);
            ui.label(
                egui::RichText::new(if command.is_empty() {
                    "No tmux session is running in this folder. “Start session” opens a terminal there with a shell.".to_string()
                } else {
                    format!("No tmux session is running in this folder. “Start session” opens a terminal there and runs “{command}”.")
                })
                .weak(),
            );
            return;
        }

        let Some(name) = name else {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("Add a project, or select one on the left.").weak())
            });
            return;
        };
        let session = self.session(&name).cloned();
        let window = self
            .selected_window
            .and_then(|i| self.windows_of(&name).iter().find(|w| w.index == i).cloned());
        let title = project.as_ref().map(|(_, p)| p.name.clone()).unwrap_or_else(|| name.clone());

        // Header
        ui.add_space(4.0);
        match &window {
            Some(w) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&title).weak().size(18.0));
                    ui.label(egui::RichText::new("›").weak().size(18.0));
                    ui.heading(format!("{}: {}", w.index, w.name));
                });
                ui.label(
                    egui::RichText::new(format!("Running {}{}", w.command, if w.active { " · active window" } else { "" }))
                        .weak(),
                );
            }
            None => {
                ui.heading(&title);
                if let Some(s) = &session {
                    let folder = project
                        .as_ref()
                        .map(|(_, p)| p.path.display().to_string())
                        .unwrap_or_else(|| s.path.clone());
                    ui.label(egui::RichText::new(folder).weak());
                    ui.label(
                        egui::RichText::new(format!(
                            "{} · {} window{} · started {}",
                            if s.attached { "Open in a terminal" } else { "No terminal open" },
                            s.windows,
                            if s.windows == 1 { "" } else { "s" },
                            s.created
                        ))
                        .weak(),
                    );
                }
            }
        }
        ui.add_space(8.0);

        // Toolbar
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(egui::RichText::new("▶  Open in terminal").strong())).clicked() {
                self.open_terminal(&name, window.as_ref().map(|w| w.index));
            }
            if let Some(w) = &window {
                if ui.add_enabled(!w.active, egui::Button::new("★  Make active")).clicked() {
                    self.make_active(&name, w.index);
                }
            }
            ui.menu_button("Actions ⏷", |ui| match (&window, &project) {
                (Some(w), _) => self.window_menu(ui, &name, w),
                (None, Some((key, _))) => self.project_menu(ui, key),
                (None, None) => self.session_menu(ui, &name, true),
            });
        });
        ui.add_space(8.0);

        // Preview with a command box underneath
        let target = match &window {
            Some(w) => format!("{name}:{}", w.index),
            None => name.clone(),
        };
        egui::TopBottomPanel::bottom("command_box")
            .frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(0.0, 6.0)))
            .show_inside(ui, |ui| {
                ui.label(
                    egui::RichText::new("Type a command line and press Enter. It is typed into the window above.")
                        .weak()
                        .small(),
                );
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.command)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace)
                        .hint_text("e.g.  ls -la"),
                );
                let submit = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if submit && !self.command.trim().is_empty() {
                    let line = self.command.clone();
                    self.command.clear();
                    let r = tmux::send_line(&target, &line);
                    self.apply(r, &format!("Sent: {line}"));
                    edit.request_focus();
                }
            });

        egui::CentralPanel::default().frame(egui::Frame::none()).show_inside(ui, |ui| {
            ui.label(
                egui::RichText::new(if window.is_some() { "Live view of this window" } else { "Live view of the active window" })
                    .weak()
                    .small(),
            );
            egui::Frame::dark_canvas(ui.style()).show(ui, |ui| {
                egui::ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&self.preview).monospace().size(12.0))
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
            });
        });
    }

    // ---------- dialogs ----------

    fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let mut keep = true;
        let mut open = true;

        let title = match &dialog {
            Dialog::EditProject { .. } => "Edit project",
            Dialog::Settings(_) => "Settings",
            Dialog::RemoveProject { .. } => "Remove project from list?",
            Dialog::RenameSession { .. } => "Rename session",
            Dialog::RenameWindow { .. } => "Rename window",
            Dialog::NewWindow { .. } => "New window",
            Dialog::KillSession(_) => "Kill session?",
            Dialog::KillWindow { .. } => "Kill window?",
        };

        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                let mut confirm = false;
                let mut cancel = esc;

                let danger = |ui: &mut egui::Ui, label: &str| -> bool {
                    let b = egui::Button::new(egui::RichText::new(label).color(egui::Color32::WHITE))
                        .fill(egui::Color32::from_rgb(190, 50, 50));
                    ui.add(b).clicked()
                };

                match &mut dialog {
                    Dialog::RenameSession { text, .. } | Dialog::RenameWindow { text, .. } => {
                        ui.horizontal(|ui| {
                            ui.label("New name");
                            ui.add(egui::TextEdit::singleline(text).desired_width(240.0)).request_focus();
                        });
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                            if ui.button("Rename").clicked() || enter {
                                confirm = true;
                            }
                        });
                    }
                    Dialog::EditProject { name, folder, command, .. } => {
                        egui::Grid::new("edit_project").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
                            ui.label("Name");
                            ui.add(egui::TextEdit::singleline(name).desired_width(320.0));
                            ui.end_row();
                            ui.label("Folder");
                            ui.horizontal(|ui| {
                                ui.add(egui::TextEdit::singleline(folder).desired_width(250.0));
                                if ui.button("Browse…").clicked() {
                                    if let Some(f) = rfd::FileDialog::new().set_title("Choose the project folder").pick_folder() {
                                        *folder = f.display().to_string();
                                    }
                                }
                            });
                            ui.end_row();
                            ui.label("Start command");
                            ui.add(
                                egui::TextEdit::singleline(command)
                                    .desired_width(320.0)
                                    .font(egui::TextStyle::Monospace)
                                    .hint_text(format!("default: {}", self.settings.start_command)),
                            );
                            ui.end_row();
                        });
                        ui.label(egui::RichText::new("The start command is typed into the new tmux session when the project is started. Leave it blank to use the default from Settings.").weak().small());
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                            if ui.button("Save").clicked() || enter {
                                confirm = true;
                            }
                        });
                    }
                    Dialog::Settings(st) => {
                        egui::Grid::new("settings").num_columns(2).spacing([8.0, 8.0]).show(ui, |ui| {
                            ui.label("Default start command");
                            ui.add(
                                egui::TextEdit::singleline(&mut st.start_command)
                                    .desired_width(320.0)
                                    .font(egui::TextStyle::Monospace)
                                    .hint_text("blank: just a shell"),
                            );
                            ui.end_row();
                            ui.label("Terminal program");
                            ui.add(
                                egui::TextEdit::singleline(&mut st.terminal)
                                    .desired_width(320.0)
                                    .font(egui::TextStyle::Monospace)
                                    .hint_text(if cfg!(target_os = "macos") { "blank: iTerm or Terminal" } else { "blank: $TERMINAL or the first one found" }),
                            );
                            ui.end_row();
                        });
                        ui.label(egui::RichText::new("The start command is typed into a new tmux session when a project is started, e.g. “claude --continue”, “vim”, or “htop”. Each project can override it in Edit project.").weak().small());
                        ui.label(egui::RichText::new(if cfg!(target_os = "macos") {
                            "Terminal program: Terminal, iTerm, kitty, alacritty, or wezterm. Blank: iTerm if installed, else Terminal."
                        } else {
                            "Terminal program: gnome-terminal, kitty, alacritty, wezterm, konsole, xfce4-terminal, tilix, foot, or xterm."
                        }).weak().small());
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                            if ui.button("Save").clicked() || enter {
                                confirm = true;
                            }
                        });
                    }
                    Dialog::RemoveProject { path } => {
                        let pname = self.project_at(path).map(|p| p.name.clone()).unwrap_or_default();
                        ui.label(format!("Remove “{pname}” from the project list?"));
                        ui.label(egui::RichText::new("The folder and any running tmux session are left as they are.").weak());
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                            if ui.button("Remove").clicked() || enter {
                                confirm = true;
                            }
                        });
                    }
                    Dialog::NewWindow { name, .. } => {
                        ui.horizontal(|ui| {
                            ui.label("Name");
                            ui.add(egui::TextEdit::singleline(name).desired_width(240.0).hint_text("optional")).request_focus();
                        });
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                            if ui.button("Create").clicked() || enter {
                                confirm = true;
                            }
                        });
                    }
                    Dialog::KillSession(name) => {
                        ui.label(format!("End the session “{name}”?"));
                        ui.label(egui::RichText::new("Every program running inside it will be terminated.").weak());
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                            if danger(ui, "Kill session") {
                                confirm = true;
                            }
                        });
                    }
                    Dialog::KillWindow { session, index } => {
                        let last = self.windows_of(session).len() <= 1;
                        if last {
                            ui.label(format!("Window {index} is the only window of “{session}”."));
                            ui.label(egui::RichText::new("Closing it ends the whole session.").color(RED));
                        } else {
                            ui.label(format!("Close window {index} of “{session}”?"));
                            ui.label(egui::RichText::new("Programs running in it will be terminated.").weak());
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                            if danger(ui, "Kill window") {
                                confirm = true;
                            }
                        });
                    }
                }

                if cancel {
                    keep = false;
                }
                if confirm {
                    keep = false;
                    self.run_dialog(&dialog);
                }
            });

        if keep && open {
            self.dialog = Some(dialog);
        }
    }

    fn run_dialog(&mut self, d: &Dialog) {
        match d {
            Dialog::EditProject { path, name, folder, command } => {
                let new_name = name.trim();
                let new_folder = folder.trim();
                if new_name.is_empty() || new_folder.is_empty() {
                    self.fail("Name and folder are required");
                    return;
                }
                let session = self.matched.get(path).cloned();
                let new_key = projects::normalize(std::path::Path::new(new_folder));
                if new_key != *path && self.projects.iter().any(|p| projects::normalize(&p.path) == new_key) {
                    self.fail("Another project already uses that folder");
                    return;
                }
                if let Some(p) = self.projects.iter_mut().find(|p| projects::normalize(&p.path) == *path) {
                    p.name = new_name.to_string();
                    p.path = PathBuf::from(new_folder);
                    p.command = command.trim().to_string();
                }
                self.save_projects();
                self.selected_project = Some(new_key.clone());
                // Keep the running session's name in step with the project name.
                let mut r = Ok(());
                if let (Some(old), true) = (session, new_key == *path) {
                    let wanted = self.project_at(&new_key).map(|p| p.session_name()).unwrap_or_default();
                    if old != wanted {
                        r = tmux::rename_session(&old, &wanted);
                        if r.is_ok() && self.collapsed.remove(&old) {
                            self.collapsed.insert(wanted.clone());
                        }
                    }
                }
                self.apply(r, "Project saved");
            }
            Dialog::Settings(st) => {
                self.settings = st.clone();
                let r = projects::save_settings(&self.settings);
                self.apply(r, "Settings saved");
            }
            Dialog::RemoveProject { path } => {
                let name = self.project_at(path).map(|p| p.name.clone()).unwrap_or_default();
                self.projects.retain(|p| projects::normalize(&p.path) != *path);
                self.save_projects();
                self.selected_project = None;
                self.selected = None;
                self.selected_window = None;
                self.apply(Ok(()), &format!("Removed “{name}” from the list"));
            }
            Dialog::RenameSession { old, text } => {
                let new = text.trim();
                if new.is_empty() || new == old {
                    return;
                }
                let r = tmux::rename_session(old, new);
                if r.is_ok() {
                    self.selected = Some(new.to_string());
                    if self.collapsed.remove(old) {
                        self.collapsed.insert(new.to_string());
                    }
                }
                self.apply(r, &format!("Renamed to “{new}”"));
            }
            Dialog::RenameWindow { session, index, text } => {
                let new = text.trim();
                if new.is_empty() {
                    return;
                }
                let r = tmux::rename_window(&format!("{session}:{index}"), new);
                self.apply(r, &format!("Window renamed to “{new}”"));
            }
            Dialog::NewWindow { session, name } => {
                let n = name.trim();
                let r = tmux::new_window(session, if n.is_empty() { None } else { Some(n) });
                if r.is_ok() {
                    self.collapsed.remove(session);
                }
                self.apply(r, "Window created");
            }
            Dialog::KillSession(name) => {
                let r = tmux::kill_session(name);
                self.apply(r, &format!("Killed “{name}”"));
            }
            Dialog::KillWindow { session, index } => {
                self.selected_window = None;
                let r = tmux::kill_window(&format!("{session}:{index}"));
                self.apply(r, &format!("Closed window {index}"));
            }
        }
    }
}

/// Add system fonts with wide Unicode coverage (box drawing, symbols, Japanese/Chinese/Korean)
/// as fallbacks, so the preview can show whatever a terminal prints. Missing files are skipped.
fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut defs = FontDefinitions::default();

    // (name, candidate paths, face index inside a .ttc collection)
    // Linux paths first, then macOS. Whatever is missing is skipped.
    let candidates: &[(&str, &[&str], u32)] = &[
        ("dejavu_mono", &["/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", "/usr/share/fonts/TTF/DejaVuSansMono.ttf"], 0),
        ("menlo", &["/System/Library/Fonts/Menlo.ttc"], 0),
        ("noto_symbols2", &["/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf"], 0),
        ("noto_symbols", &["/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf"], 0),
        ("noto_cjk_mono", &["/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"], 5),
        ("apple_symbols", &["/System/Library/Fonts/Apple Symbols.ttf"], 0),
        ("arial_unicode", &["/System/Library/Fonts/Supplemental/Arial Unicode.ttf"], 0),
    ];
    const MONO_PRIMARY: &[&str] = &["dejavu_mono", "menlo"];

    let mut loaded: Vec<String> = Vec::new();
    for (name, paths, index) in candidates {
        if let Some(bytes) = paths.iter().find_map(|p| std::fs::read(p).ok()) {
            let mut data = FontData::from_owned(bytes);
            data.index = *index;
            defs.font_data.insert((*name).to_string(), data);
            loaded.push((*name).to_string());
        }
    }
    if loaded.is_empty() {
        return;
    }

    let mono = defs.families.entry(FontFamily::Monospace).or_default();
    if let Some(primary) = loaded.iter().find(|n| MONO_PRIMARY.contains(&n.as_str())) {
        mono.insert(0, primary.clone());
    }
    for n in &loaded {
        if !MONO_PRIMARY.contains(&n.as_str()) {
            mono.push(n.clone());
        }
    }
    let prop = defs.families.entry(FontFamily::Proportional).or_default();
    for n in &loaded {
        if !MONO_PRIMARY.contains(&n.as_str()) {
            prop.push(n.clone());
        }
    }
    ctx.set_fonts(defs);
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let due = self.last_refresh.map(|t| t.elapsed() >= REFRESH_EVERY).unwrap_or(true);
        if due {
            self.refresh();
        }
        ctx.request_repaint_after(REFRESH_EVERY);

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| match &self.status {
                Some((msg, true)) => {
                    ui.colored_label(RED, format!("⚠ {msg}"));
                }
                Some((msg, false)) => {
                    ui.label(msg);
                }
                None => {
                    ui.label(egui::RichText::new("Ready").weak());
                }
            });
        });

        // Narrow tab on the right edge that shows or hides the detail panel.
        let mut toggle = false;
        egui::SidePanel::right("detail_tab")
            .exact_width(TAB_WIDTH)
            .resizable(false)
            .frame(egui::Frame::none().fill(ctx.style().visuals.faint_bg_color))
            .show(ctx, |ui| {
                let (label, hint) = if self.detail_open { ("⏴", "Hide the preview panel") } else { ("⏵", "Show the preview panel") };
                let size = egui::vec2(TAB_WIDTH, ui.available_height());
                let sense = if self.detail_open { egui::Sense::click() } else { egui::Sense::click_and_drag() };
                let resp = ui
                    .add_sized(size, egui::Button::new(label).frame(false).sense(sense))
                    .on_hover_text(if self.detail_open { hint } else { "Click: show the preview panel\nDrag: change the width" });
                if resp.clicked() {
                    toggle = true;
                }
                if !self.detail_open {
                    if resp.hovered() || resp.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if resp.drag_started() {
                        self.drag_target = Some(ui.ctx().screen_rect().width());
                    }
                    if resp.dragged() {
                        if let Some(t) = self.drag_target.as_mut() {
                            *t = (*t + resp.drag_delta().x).max(MIN_LIST_WIDTH + TAB_WIDTH);
                            let h = ui.ctx().screen_rect().height();
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(*t, h)));
                        }
                    }
                    if resp.drag_stopped() {
                        self.drag_target = None;
                    }
                }
            });
        if toggle {
            self.toggle_detail(ctx);
        }

        // The list is always a side panel, so its header stays put when the detail panel toggles.
        // Expanded: the divider is draggable. Collapsed: the list fills the window, so resizing
        // the window sets its width. Either way the width is remembered in `list_width`.
        let screen_w = ctx.screen_rect().width();
        // Has the window reached the size we asked for? Give up waiting after a while
        // in case the window manager refused the request.
        let settling = match self.pending_width {
            Some((w, frames)) if (screen_w - w).abs() > 2.0 && frames < 60 => {
                self.pending_width = Some((w, frames + 1));
                ctx.request_repaint();
                true
            }
            _ => {
                self.pending_width = None;
                false
            }
        };
        let panel = if settling {
            egui::SidePanel::left("tree").resizable(false).exact_width(self.list_width)
        } else if self.detail_open {
            egui::SidePanel::left("tree")
                .resizable(true)
                .default_width(self.list_width)
                .min_width(MIN_LIST_WIDTH)
                .max_width((screen_w - TAB_WIDTH - MIN_DETAIL_WIDTH).max(MIN_LIST_WIDTH))
        } else {
            // Collapsed: the list fills the window. Its width is changed by dragging the edge tab,
            // which resizes the window, or by resizing the window itself.
            egui::SidePanel::left("tree")
                .resizable(false)
                .exact_width((screen_w - TAB_WIDTH).max(MIN_LIST_WIDTH))
        };
        let resp = panel.show(ctx, |ui| self.tree(ui));
        if !settling {
            self.list_width = resp.response.rect.width();
        }
        if self.detail_open {
            egui::CentralPanel::default().show(ctx, |ui| self.detail(ui));
        }

        self.dialogs(ctx);
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Muxdock")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
                    .expect("the bundled icon is a valid PNG"),
            )
            .with_inner_size(if std::env::var("MUXDOCK_EXPANDED").map(|v| v == "1").unwrap_or(false) {
                [EXPANDED_SIZE.x, EXPANDED_SIZE.y]
            } else {
                [DEFAULT_LIST_WIDTH + TAB_WIDTH, EXPANDED_SIZE.y]
            })
            .with_min_inner_size([MIN_LIST_WIDTH + TAB_WIDTH, 300.0]),
        ..Default::default()
    };
    eframe::run_native(
        "muxdock",
        options,
        Box::new(|cc| {
            install_fonts(&cc.egui_ctx);
            cc.egui_ctx.set_pixels_per_point(1.15);
            Ok(Box::new(App::new()))
        }),
    )
}
