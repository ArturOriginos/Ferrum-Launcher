use crate::avatar;
use crate::config::*;
use crate::custom;
use crate::export;
use crate::import;
use crate::skin;
use crate::launch;
use crate::minecraft::{self, McVersion};
use crate::mods::{self, InstalledMod, ModHit};
use crate::net::{self, Progress, ProgressState, Slot};
use crate::terms;
use crate::theme;
use crate::util::{fmt_num, fmt_playtime, open_path, total_ram_mb};
use eframe::egui::{
    self, vec2, Align, Align2, Color32, FontId, Frame, Layout, Margin, RichText, Sense, Stroke,
    TextEdit,
};
use reqwest::blocking::Client;
use std::path::Path;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Instances,
    Mods,
    Accounts,
    Settings,
}

#[derive(Clone, Copy, PartialEq)]
enum CustomKind {
    Background,
    Icon,
    Skin,
}

struct ImportState {
    path: String,
    info: Option<Result<import::PackInfo, String>>,
    busy: bool,
    done: Option<(bool, String, Vec<String>)>,
}

struct ExportState {
    inst: Instance,
    name: String,
    version: String,
    embed_unknown: bool,
    include_configs: bool,
    busy: bool,
    done: Option<(bool, String, Vec<String>, Option<std::path::PathBuf>)>,
}

#[derive(Clone)]
struct InstanceForm {
    editing: Option<String>,
    name: String,
    mc: String,
    loader: Loader,
    loader_ver: String,
    custom_ram: bool,
    ram_mb: u32,
    jvm: String,
    snapshots: bool,
    auto_perf: bool,
}

pub struct App {
    cfg: Config,
    http: Client,
    tab: Tab,
    dirty: bool,
    msg: String,
    sys_ram_mb: u32,
    applied: (Color32, usize, u32),
    applied_zoom: f32,

    // Minecraft versions
    versions: Vec<McVersion>,
    versions_slot: Slot<Result<Vec<McVersion>, String>>,
    versions_loading: bool,

    // launch
    progress: Option<Progress>,
    prev_running: bool,

    // instances
    form: Option<InstanceForm>,
    confirm_delete: Option<String>,

    // mods
    mod_query: String,
    mod_results: Vec<ModHit>,
    mod_search_slot: Slot<Result<Vec<ModHit>, String>>,
    mod_msg_slot: Slot<String>,
    mod_busy: bool,
    mod_msg: String,
    installed: Vec<InstalledMod>,
    installed_for: Option<String>,

    // accounts
    nick: String,

    // icon and terms
    icon: egui::TextureHandle,
    terms_checked: bool,

    // custom images
    bg_tex: Option<egui::TextureHandle>,
    bg_path_input: String,
    icon_path_input: String,
    custom_msg: Option<(bool, String)>,
    pending_drop: Option<std::path::PathBuf>,

    // modpack import
    import: Option<ImportState>,
    import_progress: Option<Progress>,
    import_slot: Slot<Result<(Instance, Vec<String>), String>>,

    // modpack export
    export: Option<ExportState>,
    export_progress: Option<Progress>,
    export_slot: Slot<Result<export::ExportResult, String>>,

    // play time
    running_inst: Option<String>,
    counted_secs: u64,

    // skins
    faces: std::collections::HashMap<String, egui::TextureHandle>,
    avatar_tex: std::collections::HashMap<&'static str, egui::TextureHandle>,
    avatar_edit: Option<String>,
    skin_edit: Option<String>,
    skin_path_input: String,
    skin_msg: Option<(bool, String)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut cfg = Config::load();
        cfg.purge_expired();
        cfg.save();
        let http = net::http_client();
        theme::set_palette(cfg.settings.palette);
        theme::set_radius(cfg.settings.radius);
        let accent0 = accent_of(&cfg.settings);
        theme::apply(&cc.egui_ctx, accent0);
        cc.egui_ctx.set_zoom_factor(cfg.settings.ui_scale);

        let versions_slot: Slot<Result<Vec<McVersion>, String>> = Slot::new();
        {
            let (h, s) = (http.clone(), versions_slot.clone());
            let s2 = s.clone();
            net::spawn_guarded(
                move |m| s2.put(Err(m)),
                move || s.put(minecraft::fetch_versions(&h).map_err(|e| format!("{e:#}"))),
            );
        }
        let applied = (accent0, cfg.settings.palette, cfg.settings.radius);
        let applied_zoom = cfg.settings.ui_scale;
        let icon = cc.egui_ctx.load_texture(
            "app-icon",
            egui::ColorImage::from_rgba_unmultiplied(
                [128, 128],
                include_bytes!("../assets/icon_128.rgba"),
            ),
            egui::TextureOptions::LINEAR,
        );
        let mut app = App {
            cfg,
            http,
            tab: Tab::Instances,
            dirty: false,
            msg: String::new(),
            sys_ram_mb: total_ram_mb(),
            applied,
            applied_zoom,
            versions: vec![],
            versions_slot,
            versions_loading: true,
            progress: None,
            prev_running: false,
            form: None,
            confirm_delete: None,
            mod_query: String::new(),
            mod_results: vec![],
            mod_search_slot: Slot::new(),
            mod_msg_slot: Slot::new(),
            mod_busy: false,
            mod_msg: String::new(),
            installed: vec![],
            installed_for: None,
            nick: String::new(),
            icon,
            terms_checked: false,
            bg_tex: None,
            bg_path_input: String::new(),
            icon_path_input: String::new(),
            custom_msg: None,
            pending_drop: None,
            import: None,
            import_progress: None,
            import_slot: Slot::new(),
            export: None,
            export_progress: None,
            export_slot: Slot::new(),
            faces: std::collections::HashMap::new(),
            avatar_tex: avatar::PRESETS
                .iter()
                .map(|p| {
                    let t = cc.egui_ctx.load_texture(
                        format!("avatar-{}", p.id),
                        p.image(),
                        egui::TextureOptions::NEAREST,
                    );
                    (p.id, t)
                })
                .collect(),
            avatar_edit: None,
            skin_edit: None,
            skin_path_input: String::new(),
            skin_msg: None,
            running_inst: None,
            counted_secs: 0,
        };
        let uuids: Vec<String> = app.cfg.accounts.iter().map(|a| a.uuid.clone()).collect();
        for u in uuids {
            app.reload_face(&cc.egui_ctx, &u);
        }
        if app.cfg.settings.has_icon {
            app.reload_icon(&cc.egui_ctx);
        }
        if app.cfg.settings.has_bg {
            app.reload_bg(&cc.egui_ctx);
        }
        app
    }

    // ---------- custom images ----------

    fn reload_icon(&mut self, ctx: &egui::Context) {
        let custom_on = self.cfg.settings.has_icon;
        let img = (if custom_on { custom::load_icon(128) } else { None }).unwrap_or_else(|| {
            egui::ColorImage::from_rgba_unmultiplied([128, 128], include_bytes!("../assets/icon_128.rgba"))
        });
        self.icon = ctx.load_texture("app-icon", img, egui::TextureOptions::LINEAR);
        let data = (if custom_on { custom::icon_data() } else { None }).unwrap_or_else(|| egui::IconData {
            rgba: include_bytes!("../assets/icon_256.rgba").to_vec(),
            width: 256,
            height: 256,
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Icon(Some(std::sync::Arc::new(data))));
    }

    fn reload_bg(&mut self, ctx: &egui::Context) {
        self.bg_tex = if self.cfg.settings.has_bg {
            custom::load_bg().map(|img| ctx.load_texture("app-bg", img, egui::TextureOptions::LINEAR))
        } else {
            None
        };
    }

    fn set_custom(&mut self, ctx: &egui::Context, kind: CustomKind, path: &str) {
        let p = Path::new(path.trim().trim_matches('"'));
        match kind {
            CustomKind::Background => match custom::set_background(p) {
                Ok(()) => {
                    self.cfg.settings.has_bg = true;
                    self.reload_bg(ctx);
                    self.custom_msg = Some((true, "Background applied".to_string()));
                    self.dirty = true;
                }
                Err(e) => self.custom_msg = Some((false, format!("{e:#}"))),
            },
            CustomKind::Icon => match custom::set_icon(p) {
                Ok(()) => {
                    self.cfg.settings.has_icon = true;
                    self.reload_icon(ctx);
                    self.custom_msg = Some((true, "Icon applied".to_string()));
                    self.dirty = true;
                }
                Err(e) => self.custom_msg = Some((false, format!("{e:#}"))),
            },
            CustomKind::Skin => {
                let Some(uuid) = self.cfg.selected_account.clone() else {
                    self.custom_msg = Some((false, "Create and select an account first".to_string()));
                    return;
                };
                self.skin_path_input = path.to_string();
                self.apply_skin(ctx, &uuid, false);
                self.skin_edit = Some(uuid);
                self.tab = Tab::Accounts;
            }
        }
    }

    fn clear_custom(&mut self, ctx: &egui::Context, kind: CustomKind) {
        match kind {
            CustomKind::Background => {
                custom::remove_background();
                self.cfg.settings.has_bg = false;
                self.reload_bg(ctx);
                self.custom_msg = Some((true, "Background removed".to_string()));
            }
            CustomKind::Icon => {
                custom::remove_icon();
                self.cfg.settings.has_icon = false;
                self.reload_icon(ctx);
                self.custom_msg = Some((true, "Default icon restored".to_string()));
            }
            CustomKind::Skin => {}
        }
        self.dirty = true;
    }

    fn drop_window(&mut self, ctx: &egui::Context) {
        let Some(path) = self.pending_drop.clone() else {
            return;
        };
        let accent = self.accent();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut choice: Option<Option<CustomKind>> = None;
        egui::Window::new("What should be done with the image?")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(RichText::new(name).size(16.0));
                ui.label(muted("Background: up to 10 MB, from 800×450 px. Icon: up to 5 MB, strictly square, from 128×128 px."));
                ui.horizontal(|ui| {
                    if accent_btn(ui, "Use as background", accent).clicked() {
                        choice = Some(Some(CustomKind::Background));
                    }
                    if ui.button("Use as icon").clicked() {
                        choice = Some(Some(CustomKind::Icon));
                    }
                    if ui.button("Use as skin").clicked() {
                        choice = Some(Some(CustomKind::Skin));
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(None);
                    }
                });
            });
        if let Some(c) = choice {
            self.pending_drop = None;
            if let Some(kind) = c {
                self.set_custom(ctx, kind, &path.to_string_lossy());
            }
        }
    }

    // ---------- modpack export ----------

    fn open_export(&mut self, inst: Instance) {
        self.export = Some(ExportState {
            name: inst.name.clone(),
            version: "1.0.0".to_string(),
            inst,
            embed_unknown: false,
            include_configs: true,
            busy: false,
            done: None,
        });
    }

    fn start_export(&mut self) {
        let Some(st) = &mut self.export else {
            return;
        };
        st.busy = true;
        st.done = None;
        let inst = st.inst.clone();
        let opts = export::ExportOpts {
            name: st.name.trim().to_string(),
            version: st.version.trim().to_string(),
            embed_unknown: st.embed_unknown,
            include_configs: st.include_configs,
        };
        let progress = Progress::new();
        self.export_progress = Some(progress.clone());
        let http = self.http.clone();
        let slot = self.export_slot.clone();
        let (slot2, progress2) = (slot.clone(), progress.clone());
        net::spawn_guarded(
            move |m| {
                progress2.finish("");
                slot2.put(Err(m));
            },
            move || {
                let r = export::run(&http, &inst, &opts, &progress).map_err(|e| format!("{e:#}"));
                progress.finish("");
                slot.put(r);
            },
        );
    }

    fn export_window(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.export.take() else {
            return;
        };
        let accent = self.accent();
        let snap = self.export_progress.as_ref().map(|p| p.snapshot());
        let (mut start, mut close, mut open) = (false, false, false);
        egui::Window::new("Export modpack")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.label(muted(
                    "Creates a .mrpack file. You can send it to a friend or import it into another launcher.",
                ));
                egui::Grid::new("export_grid")
                    .num_columns(2)
                    .spacing([14.0, 10.0])
                    .show(ui, |ui| {
                        ui.label("Name");
                        ui.add(TextEdit::singleline(&mut st.name).desired_width(320.0));
                        ui.end_row();
                        ui.label("Modpack version");
                        ui.add(TextEdit::singleline(&mut st.version).desired_width(120.0));
                        ui.end_row();
                    });
                ui.checkbox(
                    &mut st.include_configs,
                    "Include settings: config, resource packs, shaders, options.txt",
                );
                ui.checkbox(&mut st.embed_unknown, "Put mods that are not on Modrinth into the archive");
                ui.label(muted(
                    "Worlds, logs and screenshots are not included. Mods from the last option are distributed together with the file: make sure their license allows it.",
                ));
                if st.busy {
                    match &snap {
                        Some(s) => {
                            ui.label(&s.label);
                            if s.total > 0 {
                                ui.add(
                                    egui::ProgressBar::new(s.done as f32 / s.total as f32)
                                        .desired_width(420.0)
                                        .fill(accent)
                                        .show_percentage(),
                                );
                            }
                        }
                        None => {
                            ui.label("Preparing…");
                        }
                    }
                }
                if let Some((ok, msg, warns, path)) = &st.done {
                    ui.label(RichText::new(msg).color(if *ok { accent } else { theme::DANGER }));
                    if let Some(p) = path {
                        ui.label(muted(p.display().to_string()));
                    }
                    if !warns.is_empty() {
                        egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                            for w in warns {
                                ui.label(muted(w.as_str()));
                            }
                        });
                    }
                }
                let has_file = st.done.as_ref().map_or(false, |d| d.3.is_some());
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!st.busy, |ui| {
                        if accent_btn(ui, "Export", accent).clicked() {
                            start = true;
                        }
                    });
                    if has_file && ui.button("Open folder").clicked() {
                        open = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        if open {
            open_path(&export::exports_dir());
        }
        if close {
            return;
        }
        self.export = Some(st);
        if start {
            self.start_export();
        }
    }

    // ---------- skins and avatars ----------

    /// The picture shown for an account: the chosen preset avatar, else the face of its skin.
    fn face_of(&self, a: &Account) -> Option<egui::TextureId> {
        a.avatar
            .as_deref()
            .and_then(|id| self.avatar_tex.get(id))
            .or_else(|| self.faces.get(&a.uuid))
            .map(|t| t.id())
    }

    fn reload_face(&mut self, ctx: &egui::Context, uuid: &str) {
        match skin::load_face(uuid) {
            Some(img) => {
                let t = ctx.load_texture(format!("face-{uuid}"), img, egui::TextureOptions::NEAREST);
                self.faces.insert(uuid.to_string(), t);
            }
            None => {
                self.faces.remove(uuid);
            }
        }
    }

    fn apply_skin(&mut self, ctx: &egui::Context, uuid: &str, remove: bool) {
        if remove {
            skin::remove(uuid);
            self.reload_face(ctx, uuid);
            self.skin_msg = Some((true, "Skin removed".to_string()));
            return;
        }
        let p = self.skin_path_input.clone();
        match skin::set(uuid, Path::new(p.trim().trim_matches('"'))) {
            Ok(()) => {
                self.reload_face(ctx, uuid);
                self.skin_msg = Some((
                    true,
                    "Skin applied. It will appear in the game the next time you launch an instance with a mod loader".to_string(),
                ));
            }
            Err(e) => self.skin_msg = Some((false, format!("{e:#}"))),
        }
    }

    // ---------- modpack import ----------

    fn open_import(&mut self, path: String) {
        let info = if path.trim().is_empty() {
            None
        } else {
            Some(import::inspect(Path::new(path.trim().trim_matches('"'))).map_err(|e| format!("{e:#}")))
        };
        self.import = Some(ImportState { path, info, busy: false, done: None });
        self.tab = Tab::Instances;
    }

    fn start_import(&mut self) {
        let Some(st) = &mut self.import else {
            return;
        };
        st.busy = true;
        st.done = None;
        let path = std::path::PathBuf::from(st.path.trim().trim_matches('"'));
        let progress = Progress::new();
        self.import_progress = Some(progress.clone());
        let http = self.http.clone();
        let key = mods::curseforge_key(&self.cfg.settings.curseforge_key);
        let slot = self.import_slot.clone();
        let (slot2, progress2) = (slot.clone(), progress.clone());
        net::spawn_guarded(
            move |m| {
                progress2.finish("");
                slot2.put(Err(m));
            },
            move || {
                let r = import::run(&http, &path, &key, &progress).map_err(|e| format!("{e:#}"));
                progress.finish("");
                slot.put(r);
            },
        );
    }

    fn import_window(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.import.take() else {
            return;
        };
        let accent = self.accent();
        let snap = self.import_progress.as_ref().map(|p| p.snapshot());
        let cf_missing = mods::curseforge_key(&self.cfg.settings.curseforge_key).is_empty();
        let (mut check, mut start, mut close) = (false, false, false);
        egui::Window::new("Import modpack")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(580.0)
            .show(ctx, |ui| {
                ui.label(muted(
                    "Modrinth (.mrpack) and CurseForge (.zip) modpacks. You can drag the file right into the launcher window.",
                ));
                ui.horizontal(|ui| {
                    let te = ui.add(
                        TextEdit::singleline(&mut st.path)
                            .hint_text("Path to the modpack file")
                            .desired_width(400.0),
                    );
                    if ui.button("Check").clicked()
                        || (te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    {
                        check = true;
                    }
                });
                let mut can_start = false;
                match &st.info {
                    Some(Ok(i)) => {
                        card_frame(false, accent).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(&i.name).size(17.0).color(theme::text()));
                            let l = if i.loader == Loader::Vanilla {
                                "Vanilla".to_string()
                            } else if i.loader_version.is_empty() {
                                i.loader.label().to_string()
                            } else {
                                format!("{} {}", i.loader.label(), i.loader_version)
                            };
                            ui.label(muted(format!("Minecraft {}  •  {}", i.mc_version, l)));
                            ui.label(muted(format!(
                                "Files to download: {}  •  format: {}",
                                i.files,
                                if i.kind == import::PackKind::Mrpack { "Modrinth" } else { "CurseForge" }
                            )));
                        });
                        let blocked = i.kind == import::PackKind::CurseForge && cf_missing;
                        if blocked {
                            ui.label(
                                RichText::new("CurseForge modpacks need an API key (Settings → Mods).")
                                    .color(theme::WARN),
                            );
                        }
                        can_start = !blocked && !st.busy && st.done.as_ref().map_or(true, |d| !d.0);
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).color(theme::DANGER));
                    }
                    None => {}
                }
                if st.busy {
                    match &snap {
                        Some(s) => {
                            ui.label(&s.label);
                            if s.total > 0 {
                                ui.add(
                                    egui::ProgressBar::new(s.done as f32 / s.total as f32)
                                        .desired_width(420.0)
                                        .fill(accent)
                                        .show_percentage(),
                                );
                            }
                        }
                        None => {
                            ui.label("Preparing…");
                        }
                    }
                }
                if let Some((ok, msg, warns)) = &st.done {
                    ui.label(RichText::new(msg).color(if *ok { accent } else { theme::DANGER }));
                    if !warns.is_empty() {
                        egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                            for w in warns {
                                ui.label(muted(w.as_str()));
                            }
                        });
                    }
                }
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(can_start, |ui| {
                        if accent_btn(ui, "Import", accent).clicked() {
                            start = true;
                        }
                    });
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        if check {
            st.info = Some(
                import::inspect(Path::new(st.path.trim().trim_matches('"'))).map_err(|e| format!("{e:#}")),
            );
            st.done = None;
        }
        if close {
            return;
        }
        self.import = Some(st);
        if start {
            self.start_import();
        }
    }

    // ---------- play time ----------

    fn add_playtime(&mut self, id: &str, secs: u64) {
        if secs == 0 {
            return;
        }
        if let Some(i) = self.cfg.instances.iter_mut().find(|i| i.id == id) {
            i.playtime_secs = i.playtime_secs.saturating_add(secs);
            self.dirty = true;
        }
    }

    /// The instance is being launched or the game is running in it.
    fn is_busy_with(&self, id: &str) -> bool {
        let busy = self.progress.as_ref().map_or(false, |p| {
            let s = p.snapshot();
            s.busy || s.running
        });
        busy && self.running_inst.as_deref() == Some(id)
    }

    fn accent(&self) -> Color32 {
        accent_of(&self.cfg.settings)
    }

    fn selected_instance(&self) -> Option<&Instance> {
        let id = self.cfg.selected_instance.as_ref()?;
        self.cfg.instances.iter().find(|i| &i.id == id)
    }

    fn selected_account(&self) -> Option<&Account> {
        let id = self.cfg.selected_account.as_ref()?;
        self.cfg.accounts.iter().find(|a| &a.uuid == id)
    }

    // ---------- background results ----------

    fn poll(&mut self) {
        if let Some(r) = self.versions_slot.take() {
            self.versions_loading = false;
            match r {
                Ok(v) => self.versions = v,
                Err(e) => self.msg = format!("Could not load the version list: {e}"),
            }
        }
        if let Some(r) = self.mod_search_slot.take() {
            self.mod_busy = false;
            match r {
                Ok(v) => {
                    self.mod_msg = if v.is_empty() {
                        "Nothing found".to_string()
                    } else {
                        format!("Found: {}", v.len())
                    };
                    self.mod_results = v;
                }
                Err(e) => self.mod_msg = format!("Error: {e}"),
            }
        }
        if let Some(r) = self.import_slot.take() {
            self.import_progress = None;
            let done = match r {
                Ok((inst, warns)) => {
                    let mut m = format!("Modpack '{}' was imported and selected.", inst.name);
                    if !warns.is_empty() {
                        m.push_str(&format!(" Files that could not be fetched: {}.", warns.len()));
                    }
                    self.cfg.selected_instance = Some(inst.id.clone());
                    self.cfg.instances.push(inst);
                    self.dirty = true;
                    (true, m, warns)
                }
                Err(e) => (false, e, vec![]),
            };
            if let Some(st) = &mut self.import {
                st.busy = false;
                st.done = Some(done);
            } else {
                self.msg = done.1;
            }
        }
        if let Some(r) = self.export_slot.take() {
            self.export_progress = None;
            if let Some(st) = &mut self.export {
                st.busy = false;
                st.done = Some(match r {
                    Ok(res) => {
                        let mut m = format!("Done. Mods linked via Modrinth: {}", res.from_modrinth);
                        if res.embedded > 0 {
                            m.push_str(&format!(", inside the archive: {}", res.embedded));
                        }
                        m.push('.');
                        (true, m, res.warnings, Some(res.path))
                    }
                    Err(e) => (false, e, vec![], None),
                });
            }
        }
        if let Some(m) = self.mod_msg_slot.take() {
            self.mod_busy = false;
            self.mod_msg = m;
            self.installed_for = None;
        }
    }

    // ---------- actions ----------

    fn play(&mut self) {
        let Some(inst) = self.selected_instance().cloned() else {
            self.msg = "Select an instance".into();
            return;
        };
        let Some(acc) = self.selected_account().cloned() else {
            self.msg = "Create an account in the \"Accounts\" tab".into();
            self.tab = Tab::Accounts;
            return;
        };
        self.msg.clear();
        self.running_inst = Some(inst.id.clone());
        self.counted_secs = 0;
        let progress = Progress::new();
        self.progress = Some(progress.clone());
        if let Some(i) = self.cfg.instances.iter_mut().find(|i| i.id == inst.id) {
            i.last_played = now();
            i.launches = i.launches.saturating_add(1);
        }
        self.cfg.save();
        let http = self.http.clone();
        let st = self.cfg.settings.clone();
        let progress2 = progress.clone();
        net::spawn_guarded(
            move |m| progress2.fail(m),
            move || {
                if let Err(e) = launch::run(&http, &st, &inst, &acc, &progress) {
                    progress.fail(format!("{e:#}"));
                }
            },
        );
    }

    fn new_form(&self) -> InstanceForm {
        let mc = self
            .versions
            .iter()
            .find(|v| v.kind == "release")
            .map(|v| v.id.clone())
            .unwrap_or_else(|| "1.21.1".to_string());
        InstanceForm {
            editing: None,
            name: String::new(),
            mc,
            loader: Loader::Fabric,
            loader_ver: String::new(),
            custom_ram: false,
            ram_mb: self.cfg.settings.ram_mb,
            jvm: String::new(),
            snapshots: false,
            auto_perf: true,
        }
    }

    fn edit_form(&self, i: &Instance) -> InstanceForm {
        InstanceForm {
            editing: Some(i.id.clone()),
            name: i.name.clone(),
            mc: i.mc_version.clone(),
            loader: i.loader,
            loader_ver: i.loader_version.clone(),
            custom_ram: i.ram_mb > 0,
            ram_mb: if i.ram_mb > 0 { i.ram_mb } else { self.cfg.settings.ram_mb },
            jvm: i.jvm_args.clone(),
            snapshots: false,
            auto_perf: i.auto_perf,
        }
    }

    fn save_form(&mut self, f: &InstanceForm) {
        let name = if f.name.trim().is_empty() {
            "New instance".to_string()
        } else {
            f.name.trim().to_string()
        };
        let ram = if f.custom_ram { f.ram_mb } else { 0 };
        match &f.editing {
            Some(id) => {
                if let Some(i) = self.cfg.instances.iter_mut().find(|i| &i.id == id) {
                    i.name = name;
                    i.mc_version = f.mc.trim().to_string();
                    i.loader = f.loader;
                    i.loader_version = f.loader_ver.trim().to_string();
                    i.ram_mb = ram;
                    i.jvm_args = f.jvm.trim().to_string();
                    i.auto_perf = f.auto_perf;
                    let _ = std::fs::remove_file(i.dir().join(".ferrum_perf"));
                    // the loader version may have changed — we'll resolve it again at launch
                    let _ = std::fs::remove_file(i.dir().join(".ferrum_version"));
                }
            }
            None => {
                let inst = Instance {
                    id: unique_instance_id(&name, &self.cfg.instances),
                    name,
                    mc_version: f.mc.trim().to_string(),
                    loader: f.loader,
                    loader_version: f.loader_ver.trim().to_string(),
                    ram_mb: ram,
                    jvm_args: f.jvm.trim().to_string(),
                    last_played: 0,
                    auto_perf: f.auto_perf,
                    playtime_secs: 0,
                    launches: 0,
                    extra: Default::default(),
                };
                let _ = std::fs::create_dir_all(inst.mods_dir());
                self.cfg.selected_instance = Some(inst.id.clone());
                self.cfg.instances.push(inst);
            }
        }
        self.dirty = true;
    }

    fn add_account(&mut self, temporary: bool) {
        let name = if temporary {
            random_nick()
        } else {
            self.nick.trim().to_string()
        };
        let valid = (3..=16).contains(&name.chars().count())
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            self.msg = "Nickname: 3–16 characters, only Latin letters, digits and \"_\"".into();
            return;
        }
        if self.cfg.accounts.iter().any(|a| a.name.eq_ignore_ascii_case(&name)) {
            self.msg = "An account with this nickname already exists".into();
            return;
        }
        let acc = Account::offline(&name, if temporary { Some(24 * 3600) } else { None });
        self.cfg.selected_account = Some(acc.uuid.clone());
        self.cfg.accounts.push(acc);
        self.nick.clear();
        self.msg.clear();
        self.dirty = true;
    }

    fn start_search(&mut self, inst: &Instance) {
        self.mod_busy = true;
        self.mod_msg = "Searching…".into();
        let http = self.http.clone();
        let slot = self.mod_search_slot.clone();
        let source = self.cfg.settings.mod_source;
        let key = mods::curseforge_key(&self.cfg.settings.curseforge_key);
        let q = self.mod_query.trim().to_string();
        let (mc, loader) = (inst.mc_version.clone(), inst.loader);
        let slot2 = slot.clone();
        net::spawn_guarded(
            move |m| slot2.put(Err(m)),
            move || {
                let r = mods::search(&http, source, &key, &q, &mc, loader).map_err(|e| format!("{e:#}"));
                slot.put(r);
            },
        );
    }

    fn start_install(&mut self, inst: &Instance, hit: ModHit) {
        self.mod_busy = true;
        self.mod_msg = format!("Installing '{}'…", hit.title);
        let http = self.http.clone();
        let slot = self.mod_msg_slot.clone();
        let key = mods::curseforge_key(&self.cfg.settings.curseforge_key);
        let (mc, loader, dir) = (inst.mc_version.clone(), inst.loader, inst.mods_dir());
        let slot2 = slot.clone();
        net::spawn_guarded(
            move |m| slot2.put(m),
            move || {
                let r = mods::install(&http, hit.source, &key, &hit.id, &mc, loader, &dir);
                slot.put(match r {
                    Ok(log) => format!("Installed: {}", log.join(", ")),
                    Err(e) => format!("Error: {e:#}"),
                });
            },
        );
    }

    // ---------- sidebar ----------

    fn sidebar(&mut self, ctx: &egui::Context) {
        let accent = self.accent();
        let icon_id = self.icon.id();
        let face = self.selected_account().and_then(|a| self.face_of(a));
        let acc = self
            .selected_account()
            .map(|a| (a.name.clone(), a.expires_at.is_some()));
        egui::SidePanel::left("side")
            .exact_width(230.0)
            .resizable(false)
            .frame(Frame::none().fill(theme::sidebar()).inner_margin(Margin::same(16.0)))
            .show(ctx, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Image::new((icon_id, vec2(46.0, 46.0))));
                    ui.vertical(|ui| {
                        ui.add_space(1.0);
                        ui.label(RichText::new("Ferrum").size(26.0).strong().color(accent));
                        ui.label(RichText::new("Minecraft launcher").small().color(theme::muted()));
                    });
                });
                ui.add_space(22.0);
                for (tab, icon, label) in [
                    (Tab::Instances, Icon::Grid, "Instances"),
                    (Tab::Mods, Icon::Download, "Mods"),
                    (Tab::Accounts, Icon::User, "Accounts"),
                    (Tab::Settings, Icon::Sliders, "Settings"),
                ] {
                    if nav_item(ui, icon, label, self.tab == tab, accent).clicked() {
                        self.tab = tab;
                    }
                }
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.label(RichText::new(format!("v{VERSION}")).small().color(theme::muted()));
                    if let Some((name, temp)) = &acc {
                        let r = Frame::none()
                            .fill(theme::card())
                            .rounding(theme::round(14.0))
                            .inner_margin(Margin::same(10.0))
                            .stroke(Stroke::new(1.0, theme::border()))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    avatar(ui, name, accent, 36.0, face);
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new(name).strong());
                                        ui.label(
                                            RichText::new(if *temp { "temporary account" } else { "offline account" })
                                                .small()
                                                .color(theme::muted()),
                                        );
                                    });
                                });
                            })
                            .response;
                        let click = ui.interact(r.rect, ui.id().with("acc_chip"), Sense::click());
                        if click.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if click.clicked() {
                            self.tab = Tab::Accounts;
                        }
                    }
                });
            });
    }

    // ---------- bottom bar ----------

    fn bottom_bar(&mut self, ctx: &egui::Context, snap: Option<&ProgressState>) {
        let accent = self.accent();
        let busy = snap.map_or(false, |s| s.busy);
        let running = snap.map_or(false, |s| s.running);
        let info = self.selected_instance().map(|i| {
            (
                format!("{}  •  {} {}", i.name, i.mc_version, i.loader.label()),
                i.effective_ram(&self.cfg.settings),
            )
        });
        let mut play = false;
        egui::TopBottomPanel::bottom("play")
            .exact_height(96.0)
            .frame(
                Frame::none()
                    .fill(theme::sidebar())
                    .inner_margin(Margin::symmetric(24.0, 14.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| match snap {
                        Some(s) if s.error.is_some() => {
                            ui.label(
                                RichText::new("Launch failed — see the error window for details")
                                    .color(theme::DANGER),
                            );
                        }
                        Some(s) if s.busy || s.running => {
                            ui.label(RichText::new(&s.label).strong().size(16.0));
                            if s.busy && s.total > 0 {
                                ui.add(
                                    egui::ProgressBar::new(s.done as f32 / s.total as f32)
                                        .desired_width(380.0)
                                        .fill(accent)
                                        .show_percentage(),
                                );
                            }
                        }
                        _ => {
                            match &info {
                                Some((t, ram)) => {
                                    ui.label(RichText::new(t).size(17.0).strong());
                                    ui.label(muted(format!(
                                        "Memory: {:.1} GB",
                                        *ram as f32 / 1024.0
                                    )));
                                }
                                None => {
                                    ui.label(muted("Create or select an instance"));
                                }
                            }
                            if !self.msg.is_empty() {
                                ui.label(RichText::new(&self.msg).color(theme::WARN));
                            }
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (text, enabled) = if running {
                            ("Game running", false)
                        } else if busy {
                            ("Preparing…", false)
                        } else {
                            ("Play", true)
                        };
                        if play_button(ui, text, enabled, accent).clicked() {
                            play = true;
                        }
                    });
                });
            });
        if play {
            self.play();
        }
    }

    fn central(&mut self, ctx: &egui::Context) {
        let accent = self.accent();
        let bg = self.bg_tex.as_ref().map(|t| (t.id(), t.size()));
        let dim = self.cfg.settings.bg_dim.min(95);
        let glow = self.cfg.settings.glow;
        egui::CentralPanel::default()
            .frame(Frame::none().fill(theme::bg()).inner_margin(Margin::same(28.0)))
            .show(ctx, |ui| {
                // custom background: the image over the whole area + dimming
                if let Some((tid, [tw, th])) = bg {
                    let full = ui.max_rect().expand(28.0);
                    let ia = tw as f32 / th.max(1) as f32;
                    let ra = full.width() / full.height().max(1.0);
                    let (mut u0, mut u1, mut v0, mut v1) = (0.0_f32, 1.0_f32, 0.0_f32, 1.0_f32);
                    if ia > ra {
                        let f = ra / ia;
                        u0 = (1.0 - f) / 2.0;
                        u1 = 1.0 - u0;
                    } else {
                        let f = ia / ra;
                        v0 = (1.0 - f) / 2.0;
                        v1 = 1.0 - v0;
                    }
                    let painter = ui.painter().with_clip_rect(full);
                    painter.image(
                        tid,
                        full,
                        egui::Rect::from_min_max(egui::pos2(u0, v0), egui::pos2(u1, v1)),
                        Color32::WHITE,
                    );
                    let b = theme::bg();
                    painter.rect_filled(
                        full,
                        0.0,
                        Color32::from_rgba_unmultiplied(b.r(), b.g(), b.b(), (dim as f32 * 2.55) as u8),
                    );
                }
                // soft glow of the accent color at the top of the page
                let rect = ui.max_rect().expand(28.0);
                let alpha = if !glow { 0 } else if theme::is_light() { 34 } else { 40 };
                let c1 = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), alpha);
                let c0 = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 0);
                let (l, r, t) = (rect.left(), rect.right(), rect.top());
                let mut mesh = egui::Mesh::default();
                mesh.colored_vertex(egui::pos2(l, t), c1);
                mesh.colored_vertex(egui::pos2(r, t), c1);
                mesh.colored_vertex(egui::pos2(r, t + 240.0), c0);
                mesh.colored_vertex(egui::pos2(l, t + 240.0), c0);
                mesh.add_triangle(0, 1, 2);
                mesh.add_triangle(0, 2, 3);
                ui.painter().with_clip_rect(rect).add(egui::Shape::mesh(mesh));

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.tab {
                        Tab::Instances => self.page_instances(ui),
                        Tab::Mods => self.page_mods(ui),
                        Tab::Accounts => self.page_accounts(ui),
                        Tab::Settings => self.page_settings(ui),
                    });
            });
    }

    // ---------- “Instances” page ----------

    fn page_instances(&mut self, ui: &mut egui::Ui) {
        let accent = self.accent();
        let mut new_clicked = false;
        let mut import_clicked = false;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Instances").size(32.0).color(theme::text()));
                ui.label(muted(if self.cfg.instances.is_empty() {
                    "Create your first instance: game version, mod loader, memory".to_string()
                } else {
                    {
                        let total: u64 = self.cfg.instances.iter().map(|i| i.playtime_secs).sum();
                        let mut t = format!("Total: {}. Select an instance and press \"Play\"", self.cfg.instances.len());
                        if total > 0 {
                            t.push_str(&format!("  •  played {}", fmt_playtime(total)));
                        }
                        t
                    }
                }));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if accent_btn(ui, "+  New instance", accent).clicked() {
                    new_clicked = true;
                }
                if ui.button("Import").clicked() {
                    import_clicked = true;
                }
            });
        });
        ui.add_space(4.0);

        let mut edit: Option<Instance> = None;
        let mut export_req: Option<Instance> = None;
        if let Some(inst) = self.selected_instance().cloned() {
            let busy_this = self.is_busy_with(&inst.id);
            ui.horizontal(|ui| {
                ui.label(RichText::new(&inst.name).size(16.0).color(theme::text()));
                ui.label(muted(format!(
                    "played {}  •  launches {}",
                    fmt_playtime(inst.playtime_secs),
                    inst.launches
                )));
                if ui.button("Edit").clicked() {
                    edit = Some(inst.clone());
                }
                if ui.button("Open folder").clicked() {
                    open_path(&inst.dir());
                }
                if ui.button("Export").clicked() {
                    export_req = Some(inst.clone());
                }
                if ui
                    .add_enabled(!busy_this, egui::Button::new("Delete"))
                    .on_disabled_hover_text("The instance is being launched or the game is running in it")
                    .clicked()
                {
                    self.confirm_delete = Some(inst.id.clone());
                }
            });
        }
        if let Some(i) = edit {
            self.form = Some(self.edit_form(&i));
        }
        if let Some(i) = export_req {
            self.open_export(i);
        }

        let insts = self.cfg.instances.clone();
        let rams: Vec<u32> = insts
            .iter()
            .map(|i| i.effective_ram(&self.cfg.settings))
            .collect();
        let mut select: Option<String> = None;
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            for (inst, ram) in insts.iter().zip(rams.iter()) {
                let sel = self.cfg.selected_instance.as_deref() == Some(inst.id.as_str());
                if instance_card(ui, inst, *ram, sel, accent).clicked() {
                    select = Some(inst.id.clone());
                }
            }
            if add_card(ui, accent).clicked() {
                new_clicked = true;
            }
        });
        if let Some(id) = select {
            self.cfg.selected_instance = Some(id);
            self.installed_for = None;
            self.dirty = true;
        }
        if new_clicked {
            self.form = Some(self.new_form());
        }
        if import_clicked {
            self.open_import(String::new());
        }
    }

    // ---------- create / edit instance window ----------

    fn form_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.form.take() else {
            return;
        };
        let accent = self.accent();
        let max_ram = self.sys_ram_mb.max(4096);
        let default_ram = self.cfg.settings.ram_mb;
        let mut keep = true;
        let mut save = false;
        let title = if form.editing.is_some() {
            "Edit instance"
        } else {
            "New instance"
        };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                egui::Grid::new("form_grid")
                    .num_columns(2)
                    .spacing([16.0, 12.0])
                    .show(ui, |ui| {
                        ui.label("Name");
                        ui.add(
                            TextEdit::singleline(&mut form.name)
                                .hint_text("For example, \"Survival with tech mods\"")
                                .desired_width(320.0),
                        );
                        ui.end_row();

                        ui.label("Game version");
                        ui.vertical(|ui| {
                            if self.versions.is_empty() {
                                ui.add(
                                    TextEdit::singleline(&mut form.mc)
                                        .hint_text("1.21.1")
                                        .desired_width(140.0),
                                );
                                ui.label(muted(if self.versions_loading {
                                    "Loading the version list…"
                                } else {
                                    "Version list unavailable — enter the number manually"
                                }));
                            } else {
                                let snaps = form.snapshots;
                                egui::ComboBox::from_label("")
                                    .selected_text(form.mc.clone())
                                    .width(180.0)
                                    .show_ui(ui, |ui| {
                                        for v in self
                                            .versions
                                            .iter()
                                            .filter(|v| snaps || v.kind == "release")
                                        {
                                            ui.selectable_value(&mut form.mc, v.id.clone(), v.id.as_str());
                                        }
                                    });
                                ui.checkbox(&mut form.snapshots, "Show snapshots");
                            }
                        });
                        ui.end_row();

                        ui.label("Mod loader");
                        ui.horizontal(|ui| {
                            for l in Loader::ALL {
                                ui.selectable_value(&mut form.loader, l, l.label());
                            }
                        });
                        ui.end_row();

                        ui.label("Loader version");
                        ui.add(
                            TextEdit::singleline(&mut form.loader_ver)
                                .hint_text("empty — pick the latest")
                                .desired_width(320.0),
                        );
                        ui.end_row();

                        ui.label("Memory (RAM)");
                        ui.vertical(|ui| {
                            ui.checkbox(
                                &mut form.custom_ram,
                                format!(
                                    "Custom for this instance (default {:.1} GB)",
                                    default_ram as f32 / 1024.0
                                ),
                            );
                            if form.custom_ram {
                                ui.add(
                                    egui::Slider::new(&mut form.ram_mb, 1024..=max_ram)
                                        .step_by(256.0)
                                        .suffix(" MB"),
                                );
                            }
                        });
                        ui.end_row();

                        ui.label("JVM arguments");
                        ui.add(
                            TextEdit::singleline(&mut form.jvm)
                                .hint_text("in addition to the global ones")
                                .desired_width(320.0),
                        );
                        ui.end_row();

                        ui.label("Optimization");
                        ui.checkbox(
                            &mut form.auto_perf,
                            "Install automatically: Sodium (Fabric, Quilt, NeoForge) or OptiFine (Forge)",
                        );
                        ui.end_row();
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if accent_btn(ui, "Save", accent).clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        keep = false;
                    }
                });
            });
        if save {
            self.save_form(&form);
            keep = false;
        }
        if keep {
            self.form = Some(form);
        }
    }

    fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_delete.clone() else {
            return;
        };
        let Some(inst) = self.cfg.instances.iter().find(|i| i.id == id).cloned() else {
            self.confirm_delete = None;
            return;
        };
        let (mut yes, mut no) = (false, false);
        egui::Window::new("Delete instance?")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(format!(
                    "'{}' will be deleted together with its mods, worlds and settings.",
                    inst.name
                ));
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("Delete").color(Color32::WHITE))
                                .fill(theme::DANGER),
                        )
                        .clicked()
                    {
                        yes = true;
                    }
                    if ui.button("Cancel").clicked() {
                        no = true;
                    }
                });
            });
        if yes {
            let _ = std::fs::remove_dir_all(inst.dir());
            self.cfg.instances.retain(|i| i.id != id);
            if self.cfg.selected_instance.as_deref() == Some(id.as_str()) {
                self.cfg.selected_instance = self.cfg.instances.first().map(|i| i.id.clone());
            }
            self.confirm_delete = None;
            self.dirty = true;
        } else if no {
            self.confirm_delete = None;
        }
    }

    fn error_window(&mut self, ctx: &egui::Context, snap: Option<&ProgressState>) {
        let Some(err) = snap.and_then(|s| s.error.clone()) else {
            return;
        };
        let mut close = false;
        egui::Window::new("Launch error")
            .collapsible(false)
            .resizable(true)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(580.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    let mut text: &str = &err;
                    ui.add(TextEdit::multiline(&mut text).desired_width(f32::INFINITY));
                });
                ui.horizontal(|ui| {
                    if ui.button("Open logs folder").clicked() {
                        open_path(&logs_dir());
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        if close {
            self.progress = None;
        }
    }

    // ---------- terms of use ----------

    fn terms_screen(&mut self, ctx: &egui::Context) {
        let accent = self.accent();
        let icon_id = self.icon.id();
        let max_h = (ctx.screen_rect().height() - 360.0).max(160.0);
        let (mut accept, mut decline) = (false, false);
        egui::CentralPanel::default()
            .frame(Frame::none().fill(theme::bg()).inner_margin(Margin::same(24.0)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::new((icon_id, vec2(80.0, 80.0))));
                    ui.heading("Welcome to Ferrum Launcher");
                    ui.label(muted("Read and accept the terms of use before you start."));
                    ui.add_space(6.0);
                    card_frame(false, accent).show(ui, |ui| {
                        ui.set_width(740.0);
                        egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
                            ui.label(terms::TERMS_TEXT);
                        });
                    });
                    ui.checkbox(&mut self.terms_checked, "I have read the agreement and accept its terms");
                    ui.add_enabled_ui(self.terms_checked, |ui| {
                        if accent_btn(ui, "Accept and continue", accent).clicked() {
                            accept = true;
                        }
                    });
                    if ui.button("Decline and exit").clicked() {
                        decline = true;
                    }
                });
            });
        if accept {
            self.cfg.settings.accepted_terms = terms::TERMS_VERSION.to_string();
            self.cfg.save();
        }
        if decline {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    // ---------- “Mods” page ----------

    fn page_mods(&mut self, ui: &mut egui::Ui) {
        let accent = self.accent();
        ui.heading("Mods");
        let Some(inst) = self.selected_instance().cloned() else {
            ui.label(muted("First select an instance on the \"Instances\" tab."));
            return;
        };
        if inst.loader == Loader::Vanilla {
            ui.label(muted(format!(
                "Instance '{}' has no mod loader. Open \"Instances\" → \"Edit\" and choose Fabric, Quilt, Forge or NeoForge.",
                inst.name
            )));
            return;
        }
        ui.label(muted(format!(
            "Instance '{}': Minecraft {}, {}",
            inst.name,
            inst.mc_version,
            inst.loader.label()
        )));

        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Source:");
            for s in [ModSource::Modrinth, ModSource::CurseForge] {
                if ui
                    .selectable_value(&mut self.cfg.settings.mod_source, s, s.label())
                    .changed()
                {
                    changed = true;
                }
            }
        });
        if changed {
            self.dirty = true;
            self.mod_results.clear();
            self.mod_msg.clear();
        }

        let cf_missing = self.cfg.settings.mod_source == ModSource::CurseForge
            && mods::curseforge_key(&self.cfg.settings.curseforge_key).is_empty();
        if cf_missing {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("CurseForge needs an API key (free, console.curseforge.com).")
                        .color(theme::WARN),
                );
                if ui.button("Open settings").clicked() {
                    self.tab = Tab::Settings;
                }
            });
        }

        let mut go = false;
        ui.horizontal(|ui| {
            let te = ui.add(
                TextEdit::singleline(&mut self.mod_query)
                    .hint_text("Mod name: sodium, jei, create…")
                    .desired_width(380.0),
            );
            if accent_btn(ui, "Search", accent).clicked() {
                go = true;
            }
            if te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = true;
            }
        });
        if go && !self.mod_busy && !cf_missing {
            self.start_search(&inst);
        }
        if !self.mod_msg.is_empty() {
            ui.label(muted(self.mod_msg.clone()));
        }

        // search results
        let hits = self.mod_results.clone();
        for hit in hits {
            card_frame(false, accent).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&hit.title).size(16.0).strong());
                    ui.label(muted(format!(
                        "{}  •  {} downloads",
                        hit.author,
                        fmt_num(hit.downloads)
                    )));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let b = egui::Button::new(
                            RichText::new("Install").color(theme::on_accent(accent)),
                        )
                        .fill(accent);
                        if ui.add_enabled(!self.mod_busy, b).clicked() {
                            self.start_install(&inst, hit.clone());
                        }
                    });
                });
                let d: String = hit.description.chars().take(180).collect();
                ui.label(muted(d));
            });
        }

        // installed mods
        if self.installed_for.as_deref() != Some(inst.id.as_str()) {
            self.installed = mods::list_installed(&inst.mods_dir());
            self.installed_for = Some(inst.id.clone());
        }
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Installed mods ({})", self.installed.len()))
                    .size(18.0)
                    .strong(),
            );
            if ui.button("Mods folder").clicked() {
                open_path(&inst.mods_dir());
            }
            if ui.button("Refresh").clicked() {
                self.installed_for = None;
            }
        });
        let list = self.installed.clone();
        let mut action: Option<(InstalledMod, bool)> = None; // (mod, true = remove)
        for m in &list {
            ui.horizontal(|ui| {
                let color = if m.enabled { theme::text() } else { theme::muted() };
                ui.label(RichText::new(m.file.trim_end_matches(".disabled")).color(color));
                if !m.enabled {
                    ui.label(muted("(disabled)"));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Delete").clicked() {
                        action = Some((m.clone(), true));
                    }
                    if ui.button(if m.enabled { "Disable" } else { "Enable" }).clicked() {
                        action = Some((m.clone(), false));
                    }
                });
            });
        }
        if let Some((m, delete)) = action {
            if delete {
                mods::remove(&inst.mods_dir(), &m);
            } else {
                mods::toggle(&inst.mods_dir(), &m);
            }
            self.installed_for = None;
        }
        if list.is_empty() {
            ui.label(muted("No mods yet. Find one with the search above."));
        }
    }

    // ---------- “Accounts” page ----------

    fn page_accounts(&mut self, ui: &mut egui::Ui) {
        let accent = self.accent();
        let ctx = ui.ctx().clone();
        ui.heading("Accounts");
        ui.label(muted(
            "Offline accounts work in singleplayer and on servers with online-mode=false. Licensed servers don't let them in.",
        ));
        let (mut add, mut add_temp) = (false, false);
        ui.horizontal(|ui| {
            let te = ui.add(
                TextEdit::singleline(&mut self.nick)
                    .hint_text("Nickname: Latin letters, digits, _")
                    .desired_width(260.0),
            );
            if accent_btn(ui, "Add", accent).clicked()
                || (te.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                add = true;
            }
            if ui.button("Temporary (24 hours)").clicked() {
                add_temp = true;
            }
        });
        if add {
            self.add_account(false);
        }
        if add_temp {
            self.add_account(true);
        }
        if !self.msg.is_empty() && self.tab == Tab::Accounts {
            ui.label(RichText::new(self.msg.clone()).color(theme::WARN));
        }
        ui.add_space(6.0);

        let accounts = self.cfg.accounts.clone();
        let (mut sel, mut del, mut toggle_skin): (Option<String>, Option<String>, Option<String>) =
            (None, None, None);
        let mut toggle_avatar: Option<String> = None;
        let mut avatar_pick: Option<(String, Option<String>)> = None; // (account, preset id; None = auto)
        let mut skin_action: Option<(String, bool)> = None; // (account, true = remove)
        for a in &accounts {
            let selected = self.cfg.selected_account.as_deref() == Some(a.uuid.as_str());
            let face = self.face_of(a);
            card_frame(selected, accent).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    avatar(ui, &a.name, accent, 40.0, face);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&a.name).size(17.0).color(theme::text()));
                        let kind = match a.expires_at {
                            Some(e) => {
                                let s = e.saturating_sub(now());
                                format!("temporary, {}h {}m left", s / 3600, (s % 3600) / 60)
                            }
                            None => "offline account".to_string(),
                        };
                        ui.label(muted(kind));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Delete").clicked() {
                            del = Some(a.uuid.clone());
                        }
                        if ui.button("Skin").clicked() {
                            toggle_skin = Some(a.uuid.clone());
                        }
                        if ui.button("Avatar").clicked() {
                            toggle_avatar = Some(a.uuid.clone());
                        }
                        if !selected && ui.button("Select").clicked() {
                            sel = Some(a.uuid.clone());
                        }
                    });
                });
            });
            if self.avatar_edit.as_deref() == Some(a.uuid.as_str()) {
                card_frame(false, accent).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Account avatar").size(16.0).color(theme::text()));
                    ui.label(muted(
                        "Pick a head for this account. It is shown in the launcher only. \"Auto\" uses the face of the account's skin, or the first letter of the nickname.",
                    ));
                    ui.add_space(4.0);
                    // an id that is not in the list (e.g. from another version) counts as "Auto"
                    let current = a
                        .avatar
                        .as_deref()
                        .filter(|id| avatar::PRESETS.iter().any(|p| p.id == *id));
                    ui.horizontal_wrapped(|ui| {
                        let auto_face = self.faces.get(&a.uuid).map(|t| t.id());
                        if avatar_tile(ui, auto_face, &a.name, "Auto", current.is_none(), accent).clicked() {
                            avatar_pick = Some((a.uuid.clone(), None));
                        }
                        for p in avatar::PRESETS {
                            let tex = self.avatar_tex.get(p.id).map(|t| t.id());
                            let chosen = current == Some(p.id);
                            if avatar_tile(ui, tex, p.name, p.name, chosen, accent).clicked() {
                                avatar_pick = Some((a.uuid.clone(), Some(p.id.to_string())));
                            }
                        }
                    });
                });
            }
            if self.skin_edit.as_deref() == Some(a.uuid.as_str()) {
                card_frame(false, accent).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Account skin").size(16.0).color(theme::text()));
                    ui.label(muted(skin::SKIN_RULES));
                    ui.label(muted(
                        "In the game the skin is shown by the CustomSkinLoader mod; the launcher installs it itself. Requires an instance with Fabric, Quilt, Forge or NeoForge.",
                    ));
                    ui.horizontal(|ui| {
                        ui.add(
                            TextEdit::singleline(&mut self.skin_path_input)
                                .hint_text("Path to a PNG or drag the file into the window")
                                .desired_width(420.0),
                        );
                        if ui.button("Apply").clicked() {
                            skin_action = Some((a.uuid.clone(), false));
                        }
                        if skin::has(&a.uuid) && ui.button("Remove skin").clicked() {
                            skin_action = Some((a.uuid.clone(), true));
                        }
                    });
                    if let Some((ok, m)) = &self.skin_msg {
                        ui.label(RichText::new(m).color(if *ok { accent } else { theme::DANGER }));
                    }
                });
            }
        }
        if let Some(u) = sel {
            self.cfg.selected_account = Some(u);
            self.dirty = true;
        }
        if let Some(u) = toggle_avatar {
            self.avatar_edit = if self.avatar_edit.as_deref() == Some(u.as_str()) { None } else { Some(u) };
        }
        if let Some((u, id)) = avatar_pick {
            if let Some(acc) = self.cfg.accounts.iter_mut().find(|a| a.uuid == u) {
                acc.avatar = id;
                self.dirty = true;
            }
        }
        if let Some(u) = toggle_skin {
            self.skin_edit = if self.skin_edit.as_deref() == Some(u.as_str()) { None } else { Some(u) };
            self.skin_msg = None;
        }
        if let Some((u, remove)) = skin_action {
            self.apply_skin(&ctx, &u, remove);
        }
        if let Some(u) = del {
            skin::remove(&u);
            self.faces.remove(&u);
            self.cfg.accounts.retain(|a| a.uuid != u);
            if self.cfg.selected_account.as_deref() == Some(u.as_str()) {
                self.cfg.selected_account = self.cfg.accounts.first().map(|a| a.uuid.clone());
            }
            self.dirty = true;
        }
        if accounts.is_empty() {
            ui.label(muted("No accounts yet. Enter a nickname or create a temporary one with one click."));
        }
    }

    // ---------- “Settings” page ----------

    fn page_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        let max_ram = self.sys_ram_mb.max(4096);
        let sys = self.sys_ram_mb;
        let mut ch = false;
        let accent_now = self.accent();
        let ctx = ui.ctx().clone();

        section(ui, "Game", |ui| {
            let s = &mut self.cfg.settings;
            ui.label("Default memory");
            ch |= ui
                .add(egui::Slider::new(&mut s.ram_mb, 1024..=max_ram).step_by(256.0).suffix(" MB"))
                .changed();
            ui.label(muted(format!("System: {:.1} GB", sys as f32 / 1024.0)));
            if s.ram_mb as f32 > sys as f32 * 0.75 {
                ui.label(
                    RichText::new("That is more than 75% of the computer's memory — the system may run short.")
                        .color(theme::WARN),
                );
            }
            ui.label("JVM arguments");
            ch |= ui
                .add(TextEdit::singleline(&mut s.jvm_args).desired_width(f32::INFINITY))
                .changed();
            ui.horizontal(|ui| {
                ui.label("Game window");
                ch |= ui
                    .add(egui::Slider::new(&mut s.win_w, 640..=3840).text("width"))
                    .changed();
            });
            ui.horizontal(|ui| {
                ui.add_space(70.0);
                ch |= ui
                    .add(egui::Slider::new(&mut s.win_h, 480..=2160).text("height"))
                    .changed();
            });
            ch |= ui.checkbox(&mut s.fullscreen, "Launch in fullscreen mode").changed();
            ch |= ui
                .checkbox(&mut s.hide_launcher, "Minimize the launcher while the game is running")
                .changed();
        });

        section(ui, "Java", |ui| {
            let s = &mut self.cfg.settings;
            ch |= ui
                .checkbox(&mut s.auto_java, "Pick and download Java automatically")
                .changed();
            ui.label(muted(
                "For each game version the launcher will use the right Java (8, 17 or 21) from the system or download it itself.",
            ));
            ui.add_enabled_ui(!s.auto_java, |ui| {
                ch |= ui
                    .add(
                        TextEdit::singleline(&mut s.java_path)
                            .hint_text("Path to java / javaw, for example C:\\Java\\bin\\java.exe")
                            .desired_width(f32::INFINITY),
                    )
                    .changed();
            });
        });

        section(ui, "Mods", |ui| {
            let s = &mut self.cfg.settings;
            ui.horizontal(|ui| {
                ui.label("Default source");
                for src in [ModSource::Modrinth, ModSource::CurseForge] {
                    ch |= ui
                        .selectable_value(&mut s.mod_source, src, src.label())
                        .changed();
                }
            });
            ui.label("CurseForge API key");
            ch |= ui
                .add(
                    TextEdit::singleline(&mut s.curseforge_key)
                        .password(true)
                        .hint_text("Free key: console.curseforge.com")
                        .desired_width(f32::INFINITY),
                )
                .changed();
            ui.label(muted("Modrinth works without a key."));
        });

        section(ui, "Appearance", |ui| {
            let s = &mut self.cfg.settings;
            ui.label("Interface color");
            ui.horizontal_wrapped(|ui| {
                for (i, (name, col)) in theme::ACCENTS.iter().enumerate() {
                    let sel = !s.use_custom_accent && s.accent == i;
                    if swatch(ui, *col, sel, name).clicked() {
                        s.accent = i;
                        s.use_custom_accent = false;
                        ch = true;
                    }
                }
                ui.add_space(8.0);
                ui.label("Custom color:");
                let mut rgb = s.custom_accent;
                if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                    s.custom_accent = rgb;
                    s.use_custom_accent = true;
                    ch = true;
                }
                ui.label(muted(format!(
                    "#{:02X}{:02X}{:02X}",
                    s.custom_accent[0], s.custom_accent[1], s.custom_accent[2]
                )));
            });

            ui.add_space(4.0);
            ui.label("Background");
            ui.horizontal_wrapped(|ui| {
                for (i, p) in theme::PALETTES.iter().enumerate() {
                    if palette_card(ui, p, s.palette == i, accent_now).clicked() {
                        s.palette = i;
                        ch = true;
                    }
                }
            });

            ui.add_space(4.0);
            ui.label("Corner style");
            ui.horizontal(|ui| {
                for (label, v) in [("Sharp", 0u32), ("Soft", 1), ("Round", 2)] {
                    if ui.selectable_label(s.radius == v, label).clicked() {
                        s.radius = v;
                        ch = true;
                    }
                }
            });
            ch |= ui
                .checkbox(&mut s.glow, "Accent color glow at the top of the page")
                .changed();

            ui.add_space(4.0);
            ui.label("Interface scale");
            ui.horizontal(|ui| {
                for (label, v) in [("90%", 0.9_f32), ("100%", 1.0), ("110%", 1.1), ("125%", 1.25), ("150%", 1.5)] {
                    if ui.selectable_label((s.ui_scale - v).abs() < 0.01, label).clicked() {
                        s.ui_scale = v;
                        ch = true;
                    }
                }
            });
            if ui.button("Reset appearance").clicked() {
                s.use_custom_accent = false;
                s.accent = 0;
                s.palette = 0;
                s.ui_scale = 1.0;
                s.radius = 1;
                s.glow = true;
                ch = true;
            }
        });

        let mut act: Option<(CustomKind, bool)> = None; // (what, true = remove)
        let (has_bg, has_icon) = (self.cfg.settings.has_bg, self.cfg.settings.has_icon);
        section(ui, "Custom images", |ui| {
            ui.label("Interface background");
            ui.label(muted(custom::BG_RULES));
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.bg_path_input)
                        .hint_text("Path to a file or drag an image into the window")
                        .desired_width(420.0),
                );
                if ui.button("Apply").clicked() {
                    act = Some((CustomKind::Background, false));
                }
                if has_bg && ui.button("Remove background").clicked() {
                    act = Some((CustomKind::Background, true));
                }
            });
            if has_bg {
                ch |= ui
                    .add(egui::Slider::new(&mut self.cfg.settings.bg_dim, 0..=95).text("dimming").suffix("%"))
                    .changed();
            }
            ui.separator();
            ui.label("Launcher icon");
            ui.label(muted(custom::ICON_RULES));
            ui.horizontal(|ui| {
                ui.add(
                    TextEdit::singleline(&mut self.icon_path_input)
                        .hint_text("Path to a file or drag an image into the window")
                        .desired_width(420.0),
                );
                if ui.button("Apply").clicked() {
                    act = Some((CustomKind::Icon, false));
                }
                if has_icon && ui.button("Restore default").clicked() {
                    act = Some((CustomKind::Icon, true));
                }
            });
            if let Some((ok, msg)) = &self.custom_msg {
                ui.label(RichText::new(msg).color(if *ok { accent_now } else { theme::DANGER }));
            }
        });
        if let Some((kind, remove)) = act {
            if remove {
                self.clear_custom(&ctx, kind);
            } else {
                let path = match kind {
                    CustomKind::Background => self.bg_path_input.clone(),
                    CustomKind::Icon => self.icon_path_input.clone(),
                    CustomKind::Skin => String::new(),
                };
                self.set_custom(&ctx, kind, &path);
            }
        }

        section(ui, "Data", |ui| {
            ui.label(muted(data_dir().display().to_string()));
            ui.horizontal(|ui| {
                if ui.button("Open data folder").clicked() {
                    open_path(&data_dir());
                }
                if ui.button("Open logs folder").clicked() {
                    open_path(&logs_dir());
                }
            });
            ui.label(muted("Launch logs: logs/launches. When the game crashes, everything needed is copied to logs/crashes."));
        });

        ui.label(muted(format!("Ferrum Launcher v{VERSION}  •  MIT license")));
        if ch {
            self.dirty = true;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let key = (self.accent(), self.cfg.settings.palette, self.cfg.settings.radius);
        if key != self.applied {
            theme::set_palette(self.cfg.settings.palette);
            theme::set_radius(self.cfg.settings.radius);
            theme::apply(ctx, key.0);
            self.applied = key;
        }
        if (self.cfg.settings.ui_scale - self.applied_zoom).abs() > 0.001 {
            ctx.set_zoom_factor(self.cfg.settings.ui_scale);
            self.applied_zoom = self.cfg.settings.ui_scale;
        }
        if self.cfg.settings.accepted_terms != terms::TERMS_VERSION {
            self.terms_screen(ctx);
            return;
        }
        // dragging files into the window
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if let Some(p) = dropped.into_iter().next() {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            if ext == "mrpack" || ext == "zip" {
                self.open_import(p.to_string_lossy().to_string());
            } else {
                self.pending_drop = Some(p);
                self.tab = Tab::Settings;
            }
        }
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop_overlay"),
            ));
            let r = ctx.screen_rect();
            painter.rect_filled(r, 0.0, Color32::from_black_alpha(170));
            painter.rect_stroke(r.shrink(14.0), 18.0, Stroke::new(3.0, self.accent()));
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                "Drop the file: an image (background, icon, skin) or a modpack (.mrpack, .zip)",
                FontId::proportional(22.0),
                Color32::WHITE,
            );
        }
        self.poll();

        let snap = self.progress.as_ref().map(|p| p.snapshot());
        if let Some(s) = &snap {
            if s.running && !self.prev_running && self.cfg.settings.hide_launcher {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            self.prev_running = s.running;
        }

        // play time: while the game runs we add it every 30 s, on exit we reconcile the exact value
        if let Some(sn) = &snap {
            if let (Some(start), Some(id)) = (sn.play_started, self.running_inst.clone()) {
                if sn.running {
                    let elapsed = now().saturating_sub(start);
                    if elapsed >= self.counted_secs + 30 {
                        self.add_playtime(&id, elapsed - self.counted_secs);
                        self.counted_secs = elapsed;
                    }
                } else {
                    self.add_playtime(&id, sn.played_secs.saturating_sub(self.counted_secs));
                    self.running_inst = None;
                    self.counted_secs = 0;
                }
            }
        }

        self.sidebar(ctx);
        self.bottom_bar(ctx, snap.as_ref());
        self.central(ctx);
        self.form_window(ctx);
        self.confirm_window(ctx);
        self.error_window(ctx, snap.as_ref());
        self.drop_window(ctx);
        self.import_window(ctx);
        self.export_window(ctx);

        if self.dirty {
            self.cfg.save();
            self.dirty = false;
        }
        let busy = snap.as_ref().map_or(false, |s| s.busy || s.running)
            || self.mod_busy
            || self.import_progress.is_some()
            || self.export_progress.is_some()
            || self.versions_loading;
        if busy {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
    }
}

// ---------- small UI helpers ----------

fn accent_btn(ui: &mut egui::Ui, text: &str, accent: Color32) -> egui::Response {
    ui.add(
        egui::Button::new(
            RichText::new(text)
                .strong()
                .color(theme::on_accent(accent)),
        )
        .fill(accent)
        .rounding(theme::round(10.0))
        .min_size(vec2(0.0, 34.0)),
    )
}

fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(theme::muted())
}

fn card_frame(selected: bool, accent: Color32) -> Frame {
    Frame::none()
        .fill(theme::card())
        .rounding(theme::round(14.0))
        .inner_margin(Margin::same(16.0))
        .stroke(Stroke::new(
            if selected { 2.0 } else { 1.0 },
            if selected { accent } else { theme::border() },
        ))
}

fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    Frame::none()
        .fill(theme::card())
        .rounding(theme::round(14.0))
        .inner_margin(Margin::same(18.0))
        .stroke(Stroke::new(1.0, theme::border()))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).size(18.0).strong());
            ui.add_space(2.0);
            add(ui);
        });
    ui.add_space(4.0);
}

fn avatar(ui: &mut egui::Ui, name: &str, color: Color32, size: f32, face: Option<egui::TextureId>) {
    let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    paint_avatar(ui.painter(), rect, name, color, face);
}

/// A clickable tile of the avatar picker: the picture, a name under it and a frame when chosen.
fn avatar_tile(
    ui: &mut egui::Ui,
    face: Option<egui::TextureId>,
    name: &str,
    label: &str,
    chosen: bool,
    accent: Color32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(76.0, 92.0), Sense::click());
    let painter = ui.painter();
    if chosen {
        painter.rect_filled(rect, theme::round(10.0), accent.gamma_multiply(0.2));
    } else if resp.hovered() {
        painter.rect_filled(rect, theme::round(10.0), theme::hover());
    }
    painter.rect_stroke(
        rect,
        theme::round(10.0),
        Stroke::new(if chosen { 2.0 } else { 1.0 }, if chosen { accent } else { theme::border() }),
    );
    let pic = egui::Rect::from_min_size(rect.min + vec2(10.0, 8.0), vec2(56.0, 56.0));
    paint_avatar(painter, pic, name, accent, face);
    painter.text(
        egui::pos2(rect.center().x, rect.max.y - 14.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(13.0),
        theme::text(),
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn paint_avatar(
    painter: &egui::Painter,
    rect: egui::Rect,
    name: &str,
    color: Color32,
    face: Option<egui::TextureId>,
) {
    let size = rect.width();
    if let Some(tid) = face {
        painter.image(
            tid,
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::border()));
        return;
    }
    painter.circle_filled(rect.center(), size / 2.0, color);
    let letter = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_string());
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        letter,
        FontId::proportional(size * 0.5),
        theme::on_accent(color),
    );
}


// ---------- custom-drawn UI elements ----------

fn accent_of(s: &Settings) -> Color32 {
    if s.use_custom_accent {
        Color32::from_rgb(s.custom_accent[0], s.custom_accent[1], s.custom_accent[2])
    } else {
        theme::accent(s.accent)
    }
}

#[derive(Clone, Copy)]
enum Icon {
    Grid,
    Download,
    User,
    Sliders,
}

fn paint_icon(p: &egui::Painter, icon: Icon, c: egui::Pos2, col: Color32) {
    let st = Stroke::new(1.8, col);
    match icon {
        Icon::Grid => {
            for (dx, dy) in [(-6.0, -6.0), (1.0, -6.0), (-6.0, 1.0), (1.0, 1.0)] {
                p.rect_filled(egui::Rect::from_min_size(c + vec2(dx, dy), vec2(5.5, 5.5)), 1.6, col);
            }
        }
        Icon::Download => {
            p.line_segment([c + vec2(0.0, -7.0), c + vec2(0.0, 3.0)], st);
            p.line_segment([c + vec2(-4.5, -1.5), c + vec2(0.0, 3.0)], st);
            p.line_segment([c + vec2(4.5, -1.5), c + vec2(0.0, 3.0)], st);
            p.line_segment([c + vec2(-7.0, 7.0), c + vec2(7.0, 7.0)], st);
        }
        Icon::User => {
            p.circle_filled(c + vec2(0.0, -4.0), 4.0, col);
            p.rect_filled(
                egui::Rect::from_min_size(c + vec2(-7.0, 2.0), vec2(14.0, 6.0)),
                egui::Rounding { nw: 7.0, ne: 7.0, sw: 1.5, se: 1.5 },
                col,
            );
        }
        Icon::Sliders => {
            for (y, x) in [(-5.0, -3.0), (0.0, 3.0), (5.0, -1.0)] {
                p.line_segment([c + vec2(-7.0, y), c + vec2(7.0, y)], st);
                p.circle_filled(c + vec2(x, y), 2.6, col);
            }
        }
    }
}

fn nav_item(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool, accent: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 44.0), Sense::click());
    let t = ui.ctx().animate_bool(resp.id, resp.hovered());
    let fg = if selected { theme::on_accent(accent) } else { theme::text() };
    {
        let p = ui.painter();
        if selected {
            p.rect_filled(rect.expand(2.0), theme::round(14.0), accent.linear_multiply(0.18));
            p.rect_filled(rect, theme::round(12.0), accent);
        } else if t > 0.01 {
            p.rect_filled(rect, theme::round(12.0), theme::hover().linear_multiply(t));
        }
        paint_icon(p, icon, egui::pos2(rect.left() + 26.0, rect.center().y), fg);
        p.text(
            egui::pos2(rect.left() + 50.0, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(16.0),
            fg,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn loader_color(l: Loader) -> Color32 {
    match l {
        Loader::Vanilla => Color32::from_rgb(96, 190, 110),
        Loader::Fabric => Color32::from_rgb(214, 186, 134),
        Loader::Quilt => Color32::from_rgb(167, 123, 216),
        Loader::Forge => Color32::from_rgb(120, 150, 205),
        Loader::NeoForge => Color32::from_rgb(240, 150, 60),
    }
}

fn paint_chip(p: &egui::Painter, pos: egui::Pos2, text: &str, fg: Color32, bg: Color32) -> f32 {
    let g = p.layout_no_wrap(text.to_string(), FontId::proportional(12.5), fg);
    let size = g.size() + vec2(16.0, 8.0);
    p.rect_filled(egui::Rect::from_min_size(pos, size), theme::round(8.0), bg);
    p.galley(pos + vec2(8.0, 4.0), g, fg);
    size.x
}

fn ago(ts: u64) -> String {
    if ts == 0 {
        return "never launched".to_string();
    }
    let d = now().saturating_sub(ts);
    if d < 60 {
        "just now".to_string()
    } else if d < 3600 {
        format!("{} min ago", d / 60)
    } else if d < 86400 {
        format!("{}h ago", d / 3600)
    } else {
        format!("{}d ago", d / 86400)
    }
}

fn instance_card(
    ui: &mut egui::Ui,
    inst: &Instance,
    ram_mb: u32,
    selected: bool,
    accent: Color32,
) -> egui::Response {
    let (rect0, resp) = ui.allocate_exact_size(vec2(268.0, 142.0), Sense::click());
    let t = ui.ctx().animate_bool(resp.id, resp.hovered());
    let st = ui.ctx().animate_bool(resp.id.with("sel"), selected);
    let rect = rect0.translate(vec2(0.0, -2.0 * t));
    {
        let p = ui.painter();
        p.rect_filled(
            rect.translate(vec2(0.0, 4.0)),
            theme::round(16.0),
            Color32::from_black_alpha((34.0 + 36.0 * t) as u8),
        );
        p.rect_filled(rect, theme::round(16.0), theme::card().lerp_to_gamma(theme::hover(), t * 0.6));
        let border = theme::border().lerp_to_gamma(accent, st.max(t * 0.5));
        p.rect_stroke(rect, theme::round(16.0), Stroke::new(1.0 + st, border));

        // emblem with the first letter
        let em = egui::Rect::from_min_size(rect.min + vec2(16.0, 16.0), vec2(48.0, 48.0));
        p.rect_filled(em, theme::round(13.0), accent.linear_multiply(0.22));
        let letter = inst
            .name
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_else(|| "?".to_string());
        p.text(em.center(), Align2::CENTER_CENTER, letter, FontId::proportional(24.0), accent);

        let mut name: String = inst.name.chars().take(20).collect();
        if inst.name.chars().count() > 20 {
            name.push('…');
        }
        p.text(
            rect.min + vec2(76.0, 16.0),
            Align2::LEFT_TOP,
            name,
            FontId::proportional(17.0),
            theme::text(),
        );
        // on light palettes the loader tags are darker, otherwise they can't be read
        let lc = {
            let c = loader_color(inst.loader);
            if theme::is_light() {
                Color32::from_rgb((c.r() as f32 * 0.62) as u8, (c.g() as f32 * 0.62) as u8, (c.b() as f32 * 0.62) as u8)
            } else {
                c
            }
        };
        let w = paint_chip(
            p,
            rect.min + vec2(76.0, 42.0),
            &inst.mc_version,
            theme::muted(),
            theme::surface(),
        );
        paint_chip(
            p,
            rect.min + vec2(76.0 + w + 6.0, 42.0),
            inst.loader.label(),
            lc,
            Color32::from_rgba_unmultiplied(lc.r(), lc.g(), lc.b(), 46),
        );
        p.text(
            rect.min + vec2(16.0, 88.0),
            Align2::LEFT_TOP,
            format!("Played: {}", fmt_playtime(inst.playtime_secs)),
            FontId::proportional(13.0),
            theme::text(),
        );
        p.text(
            rect.min + vec2(16.0, 110.0),
            Align2::LEFT_TOP,
            format!("Memory {:.1} GB  •  {}", ram_mb as f32 / 1024.0, ago(inst.last_played)),
            FontId::proportional(12.5),
            theme::muted(),
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn add_card(ui: &mut egui::Ui, accent: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(268.0, 142.0), Sense::click());
    let t = ui.ctx().animate_bool(resp.id, resp.hovered());
    {
        let p = ui.painter();
        p.rect_filled(rect, theme::round(16.0), accent.linear_multiply(0.05 + 0.07 * t));
        p.rect_stroke(rect, theme::round(16.0), Stroke::new(1.5, theme::border().lerp_to_gamma(accent, t)));
        let c = rect.center() - vec2(0.0, 10.0);
        let st = Stroke::new(2.5, accent);
        p.line_segment([c + vec2(-10.0, 0.0), c + vec2(10.0, 0.0)], st);
        p.line_segment([c + vec2(0.0, -10.0), c + vec2(0.0, 10.0)], st);
        p.text(
            rect.center() + vec2(0.0, 28.0),
            Align2::CENTER_CENTER,
            "New instance",
            FontId::proportional(15.0),
            theme::muted().lerp_to_gamma(accent, t),
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn play_button(ui: &mut egui::Ui, text: &str, enabled: bool, accent: Color32) -> egui::Response {
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(vec2(224.0, 58.0), sense);
    let t = ui.ctx().animate_bool(resp.id, enabled && resp.hovered());
    {
        let p = ui.painter();
        let fill = if enabled {
            accent.lerp_to_gamma(Color32::WHITE, t * 0.14)
        } else {
            theme::surface()
        };
        let fg = if enabled { theme::on_accent(accent) } else { theme::muted() };
        if enabled {
            p.rect_filled(rect.expand(3.0 + 3.0 * t), theme::round(19.0), accent.linear_multiply(0.16 + 0.12 * t));
        }
        p.rect_filled(rect, theme::round(15.0), fill);
        let mut cx = rect.center().x;
        if enabled {
            let c = egui::pos2(rect.left() + 54.0, rect.center().y);
            p.add(egui::Shape::convex_polygon(
                vec![c + vec2(-6.0, -10.0), c + vec2(-6.0, 10.0), c + vec2(10.0, 0.0)],
                fg,
                Stroke::NONE,
            ));
            cx += 12.0;
        }
        p.text(
            egui::pos2(cx, rect.center().y),
            Align2::CENTER_CENTER,
            text,
            FontId::proportional(20.0),
            fg,
        );
    }
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn swatch(ui: &mut egui::Ui, col: Color32, selected: bool, name: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
    {
        let p = ui.painter();
        p.circle_filled(rect.center(), 13.0, col);
        if selected {
            p.circle_stroke(rect.center(), 16.5, Stroke::new(2.0, theme::text()));
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(name)
}

fn palette_card(ui: &mut egui::Ui, pal: &theme::Palette, selected: bool, accent: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(116.0, 98.0), Sense::click());
    {
        let p = ui.painter();
        let prev = egui::Rect::from_min_size(rect.min, vec2(116.0, 74.0));
        p.rect_filled(prev, 12.0, pal.bg);
        p.rect_filled(
            egui::Rect::from_min_size(prev.min, vec2(30.0, 74.0)),
            egui::Rounding { nw: 12.0, sw: 12.0, ne: 0.0, se: 0.0 },
            pal.sidebar,
        );
        p.rect_filled(egui::Rect::from_min_size(prev.min + vec2(40.0, 12.0), vec2(64.0, 24.0)), 7.0, pal.card);
        p.rect_filled(egui::Rect::from_min_size(prev.min + vec2(40.0, 46.0), vec2(38.0, 10.0)), 5.0, accent);
        p.rect_stroke(
            prev,
            12.0,
            Stroke::new(if selected { 2.5 } else { 1.0 }, if selected { accent } else { theme::border() }),
        );
        p.text(
            egui::pos2(rect.center().x, rect.bottom() - 10.0),
            Align2::CENTER_CENTER,
            pal.name,
            FontId::proportional(13.5),
            if selected { theme::text() } else { theme::muted() },
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}
