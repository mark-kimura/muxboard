#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod tmux;

use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};
use tmux::{Session, Window};

const REFRESH_EVERY: Duration = Duration::from_millis(1000);
const RED: egui::Color32 = egui::Color32::from_rgb(220, 80, 80);

/// The one modal dialog that can be open at a time.
enum Dialog {
    RenameSession { old: String, text: String },
    RenameWindow { session: String, index: u32, text: String },
    NewSession { name: String, dir: String },
    NewWindow { session: String, name: String },
    KillSession(String),
    KillWindow { session: String, index: u32 },
}

#[derive(Default)]
struct App {
    sessions: Vec<Session>,
    windows: HashMap<String, Vec<Window>>,
    selected: Option<String>,
    selected_window: Option<u32>,
    collapsed: HashSet<String>,
    preview: String,
    last_refresh: Option<Instant>,
    dialog: Option<Dialog>,
    command: String,
    status: Option<(String, bool)>,
}

impl App {
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

        if let Some(sel) = &self.selected {
            if !self.sessions.iter().any(|s| &s.name == sel) {
                self.selected = None;
                self.selected_window = None;
            }
        }
        if self.selected.is_none() {
            self.selected = self.sessions.first().map(|s| s.name.clone());
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

    fn select(&mut self, session: &str, window: Option<u32>) {
        let changed = self.selected.as_deref() != Some(session) || self.selected_window != window;
        self.selected = Some(session.to_string());
        self.selected_window = window;
        if changed {
            self.refresh();
        }
    }

    // ---------- actions (all go through here so menus, buttons and keys behave the same) ----------

    fn open_terminal(&mut self, session: &str, window: Option<u32>) {
        if let Some(w) = window {
            let _ = tmux::select_window(&format!("{session}:{w}"));
        }
        let r = tmux::attach_in_terminal(session);
        self.apply(r, "Opened a terminal");
    }

    fn make_active(&mut self, session: &str, index: u32) {
        let r = tmux::select_window(&format!("{session}:{index}"));
        self.apply(r, &format!("Window {index} is now active"));
    }

    fn session_menu(&mut self, ui: &mut egui::Ui, name: &str) {
        let attached = self.session(name).map(|s| s.attached).unwrap_or(false);
        if ui.button("▶  Open in terminal").clicked() {
            self.open_terminal(name, None);
            ui.close_menu();
        }
        if ui.button("+  New window").clicked() {
            self.dialog = Some(Dialog::NewWindow { session: name.into(), name: String::new() });
            ui.close_menu();
        }
        if ui.button("✏  Rename…").clicked() {
            self.dialog = Some(Dialog::RenameSession { old: name.into(), text: name.into() });
            ui.close_menu();
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
        if ui.button("▶  Open in terminal here").clicked() {
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
            ui.heading("Sessions");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("+ New session").clicked() {
                    self.dialog = Some(Dialog::NewSession { name: String::new(), dir: String::new() });
                }
            });
        });
        ui.add_space(4.0);
        ui.separator();

        if self.sessions.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("No tmux sessions are running.").weak());
            ui.label(egui::RichText::new("Use “New session” to start one.").weak());
            return;
        }

        let sessions = self.sessions.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for s in &sessions {
                self.tree_session_row(ui, s);
                if !self.collapsed.contains(&s.name) {
                    let wins = self.windows_of(&s.name).to_vec();
                    for w in &wins {
                        self.tree_window_row(ui, &s.name, w);
                    }
                }
                ui.add_space(2.0);
            }
        });
    }

    fn tree_session_row(&mut self, ui: &mut egui::Ui, s: &Session) {
        let is_sel = self.selected.as_deref() == Some(&s.name) && self.selected_window.is_none();
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
            // Status dot: filled when a terminal is attached, hollow otherwise.
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 18.0), egui::Sense::hover());
            let c = rect.center();
            let color = ui.visuals().strong_text_color();
            if s.attached {
                ui.painter().circle_filled(c, 4.5, egui::Color32::from_rgb(90, 190, 110));
            } else {
                ui.painter().circle_stroke(c, 4.5, egui::Stroke::new(1.5, color));
            }
            let text = egui::RichText::new(&s.name).strong();
            let resp = ui
                .with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min).with_main_justify(true), |ui| {
                    ui.add(egui::SelectableLabel::new(is_sel, text))
                })
                .inner
                .on_hover_text(format!(
                    "{}\n{} window{}\nCreated {}\n\nDouble-click: open in terminal\nRight-click: more",
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
            resp.context_menu(|ui| self.session_menu(ui, &name));
        });

        if toggle {
            if open {
                self.collapsed.insert(s.name.clone());
            } else {
                self.collapsed.remove(&s.name);
            }
        }
        if clicked {
            self.select(&s.name, None);
        }
        if double {
            self.open_terminal(&s.name, None);
        }
    }

    fn tree_window_row(&mut self, ui: &mut egui::Ui, session: &str, w: &Window) {
        let is_sel = self.selected.as_deref() == Some(session) && self.selected_window == Some(w.index);
        let mut clicked = false;
        let mut double = false;
        ui.horizontal(|ui| {
            ui.add_space(26.0);
            let mark = if w.active { "▶" } else { "   " };
            let mut text = egui::RichText::new(format!("{mark} {}  {}", w.index, w.name));
            if !w.active {
                text = text.weak();
            }
            let resp = ui
                .with_layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min).with_main_justify(true), |ui| {
                    ui.add(egui::SelectableLabel::new(is_sel, text))
                })
                .inner
                .on_hover_text(format!(
                    "Window {}: {}\nRunning: {}{}\n\nDouble-click: open in terminal here\nRight-click: more",
                    w.index,
                    w.name,
                    w.command,
                    if w.active { "\nThis is the active window" } else { "" }
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
            self.select(session, Some(w.index));
        }
        if double {
            self.open_terminal(session, Some(w.index));
        }
    }

    // ---------- right: detail ----------

    fn detail(&mut self, ui: &mut egui::Ui) {
        let Some(name) = self.selected.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label(egui::RichText::new("Select a session on the left, or create one.").weak())
            });
            return;
        };
        let session = self.session(&name).cloned();
        let window = self
            .selected_window
            .and_then(|i| self.windows_of(&name).iter().find(|w| w.index == i).cloned());

        // Header: title + subtitle
        ui.add_space(4.0);
        match &window {
            Some(w) => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(&name).weak().size(18.0));
                    ui.label(egui::RichText::new("›").weak().size(18.0));
                    ui.heading(format!("{}: {}", w.index, w.name));
                });
                ui.label(
                    egui::RichText::new(format!(
                        "Running {}{}",
                        w.command,
                        if w.active { " · active window" } else { "" }
                    ))
                    .weak(),
                );
            }
            None => {
                ui.heading(&name);
                if let Some(s) = &session {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} · {} window{} · created {}",
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

        // Toolbar: one primary action, the rest in a menu
        ui.horizontal(|ui| {
            let primary = egui::Button::new(egui::RichText::new("▶  Open in terminal").strong());
            if ui.add(primary).clicked() {
                self.open_terminal(&name, window.as_ref().map(|w| w.index));
            }
            if let Some(w) = &window {
                if ui.add_enabled(!w.active, egui::Button::new("★  Make active")).clicked() {
                    self.make_active(&name, w.index);
                }
            }
            ui.menu_button("Actions ⏷", |ui| match &window {
                Some(w) => self.window_menu(ui, &name, w),
                None => self.session_menu(ui, &name),
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

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show_inside(ui, |ui| {
                ui.label(
                    egui::RichText::new(if window.is_some() {
                        "Live view of this window"
                    } else {
                        "Live view of the active window"
                    })
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
            Dialog::RenameSession { .. } => "Rename session",
            Dialog::RenameWindow { .. } => "Rename window",
            Dialog::NewSession { .. } => "New session",
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
                    Dialog::NewSession { name, dir } => {
                        egui::Grid::new("new_session").num_columns(2).show(ui, |ui| {
                            ui.label("Name");
                            ui.add(egui::TextEdit::singleline(name).desired_width(240.0)).request_focus();
                            ui.end_row();
                            ui.label("Start in");
                            ui.add(egui::TextEdit::singleline(dir).desired_width(240.0).hint_text("home folder"));
                            ui.end_row();
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
                    Dialog::NewWindow { name, .. } => {
                        ui.horizontal(|ui| {
                            ui.label("Name");
                            ui.add(egui::TextEdit::singleline(name).desired_width(240.0).hint_text("optional"))
                                .request_focus();
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
                            let b = egui::Button::new(egui::RichText::new("Kill session").color(egui::Color32::WHITE))
                                .fill(egui::Color32::from_rgb(190, 50, 50));
                            if ui.add(b).clicked() {
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
                            let b = egui::Button::new(egui::RichText::new("Kill window").color(egui::Color32::WHITE))
                                .fill(egui::Color32::from_rgb(190, 50, 50));
                            if ui.add(b).clicked() {
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
            Dialog::NewSession { name, dir } => {
                let name = name.trim();
                if name.is_empty() {
                    self.fail("A session name is required");
                    return;
                }
                let dir = dir.trim();
                let dir = if dir.is_empty() { None } else { Some(expand_home(dir)) };
                let r = tmux::new_session(name, dir.as_deref());
                if r.is_ok() {
                    self.selected = Some(name.to_string());
                    self.selected_window = None;
                }
                self.apply(r, &format!("Created “{name}”"));
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
    let candidates: &[(&str, &[&str], u32)] = &[
        ("dejavu_mono", &["/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf", "/usr/share/fonts/TTF/DejaVuSansMono.ttf"], 0),
        ("noto_symbols2", &["/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf"], 0),
        ("noto_symbols", &["/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf"], 0),
        ("noto_cjk_mono", &["/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"], 5),
    ];

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

    // Monospace: DejaVu Sans Mono first (if present), then egui's own, then the symbol/CJK fallbacks.
    let mono = defs.families.entry(FontFamily::Monospace).or_default();
    if loaded.iter().any(|n| n == "dejavu_mono") {
        mono.insert(0, "dejavu_mono".to_string());
    }
    for n in &loaded {
        if n != "dejavu_mono" {
            mono.push(n.clone());
        }
    }
    // Proportional: keep egui's font first, add the fallbacks after it.
    let prop = defs.families.entry(FontFamily::Proportional).or_default();
    for n in &loaded {
        if n != "dejavu_mono" {
            prop.push(n.clone());
        }
    }
    ctx.set_fonts(defs);
}

fn expand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix('~') {
        if let Some(home) = std::env::var_os("HOME") {
            return format!("{}{}", home.to_string_lossy(), rest);
        }
    }
    p.to_string()
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let due = self.last_refresh.map(|t| t.elapsed() >= REFRESH_EVERY).unwrap_or(true);
        if due {
            self.refresh();
        }
        ctx.request_repaint_after(REFRESH_EVERY);

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                match &self.status {
                    Some((msg, true)) => {
                        ui.colored_label(RED, format!("⚠ {msg}"));
                    }
                    Some((msg, false)) => {
                        ui.label(msg);
                    }
                    None => {
                        ui.label(egui::RichText::new("Ready").weak());
                    }
                }
            });
        });

        egui::SidePanel::left("tree")
            .default_width(250.0)
            .min_width(180.0)
            .show(ctx, |ui| self.tree(ui));

        egui::CentralPanel::default().show(ctx, |ui| self.detail(ui));

        self.dialogs(ctx);
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("tmux sessions")
            .with_inner_size([960.0, 600.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(
        "tmux-gui",
        options,
        Box::new(|cc| {
            install_fonts(&cc.egui_ctx);
            cc.egui_ctx.set_pixels_per_point(1.15);
            Ok(Box::new(App::default()))
        }),
    )
}
