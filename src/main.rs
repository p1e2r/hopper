#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod mc;
mod modrinth;
mod text_input;
mod ui;
mod update;

use gpui::{
    AnyElement, App, Application, Bounds, Context, Entity, ExternalPaths, FontWeight,
    PathPromptOptions, TitlebarOptions, Window, WindowBounds, WindowOptions, deferred, div,
    prelude::*, px, rgb, rgba, size,
};
use mc::{Entry, Kind, Profile};
use modrinth::{Hit, Identified, Target};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use text_input::{InputEvent, TextInput};
use ui::{BAR, BG, BLACK, GOLD, GREEN, GREEN_DARK, Icon, MUTED, RED, TEXT};

const REPO_URL: &str = "https://github.com/p1e2r/hopper";
const LOADERS: [(&str, &str); 4] = [
    ("fabric", "Fabric"),
    ("quilt", "Quilt"),
    ("forge", "Forge"),
    ("neoforge", "NeoForge"),
];

fn main() {
    Application::new().run(|cx: &mut App| {
        update::cleanup();
        ui::load_fonts(cx);
        TextInput::bind_keys(cx);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1060.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(820.), px(500.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Hopper".into()),
                    ..Default::default()
                }),
                app_id: Some("hopper".into()),
                ..Default::default()
            },
            |_, cx| cx.new(Hopper::new),
        )
        .expect("could not open window");
        cx.activate(true);
    });
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
struct Config {
    root: Option<PathBuf>,
    #[serde(default)]
    mc_version: String,
    #[serde(default)]
    loader: String,
    /// Look files up on Modrinth by their hash.
    #[serde(default = "yes")]
    identify: bool,
    /// Ask before moving something to the trash.
    #[serde(default = "yes")]
    confirm_remove: bool,
    /// Look for a new Hopper release at startup.
    #[serde(default = "yes")]
    check_updates: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            root: None,
            mc_version: String::new(),
            loader: String::new(),
            identify: true,
            confirm_remove: true,
            check_updates: true,
        }
    }
}

impl Config {
    fn path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("hopper").join("config.json"))
    }
    fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    fn save(&self) {
        if let Some(p) = Self::path() {
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap_or_default());
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Installed(Kind),
    Browse,
    Options,
}

#[derive(Clone, Copy, PartialEq)]
enum Menu {
    Version,
    Loader,
}

struct Hopper {
    cfg: Config,
    tab: Tab,
    profiles: Vec<Profile>,
    /// Every Minecraft release, from Modrinth (empty while offline).
    all_versions: Vec<String>,
    menu: Option<Menu>,
    entries: [Vec<Entry>; 3],
    filter: Entity<TextInput>,
    status: String,
    status_is_error: bool,
    confirm_delete: Option<PathBuf>,
    /// Files Modrinth recognised, keyed by path without the `.disabled` suffix.
    identified: HashMap<PathBuf, Identified>,
    /// Files already looked up (found or not), so each is only hashed once.
    checked: HashSet<PathBuf>,
    identifying: bool,
    /// The last lookup failed (usually offline).
    identify_failed: bool,
    updater: Updater,

    // Modrinth tab
    search: Entity<TextInput>,
    browse_kind: Kind,
    hits: Vec<Hit>,
    total_hits: usize,
    offset: usize,
    searching: bool,
    search_id: u64,
    searched_once: bool,
    installing: HashSet<String>,
    install_state: HashMap<String, bool>, // project id -> succeeded
}

impl Hopper {
    fn new(cx: &mut Context<Self>) -> Self {
        let filter = cx.new(|cx| TextInput::new("Filter...", cx));
        let search = cx.new(|cx| TextInput::new("Search mods...", cx));
        cx.subscribe(&filter, |_, _, _: &InputEvent, cx| cx.notify())
            .detach();
        cx.subscribe(&search, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Submit) {
                this.start_search(0, cx);
            }
        })
        .detach();

        let mut this = Hopper {
            cfg: Config::load(),
            tab: Tab::Installed(Kind::Mod),
            profiles: Vec::new(),
            all_versions: Vec::new(),
            menu: None,
            entries: Default::default(),
            filter,
            status: String::new(),
            status_is_error: false,
            confirm_delete: None,
            identified: HashMap::new(),
            checked: HashSet::new(),
            identifying: false,
            identify_failed: false,
            updater: Updater::Idle,
            search,
            browse_kind: Kind::Mod,
            hits: Vec::new(),
            total_hits: 0,
            offset: 0,
            searching: false,
            search_id: 0,
            searched_once: false,
            installing: HashSet::new(),
            install_state: HashMap::new(),
        };
        if !LOADERS.iter().any(|l| l.0 == this.cfg.loader) {
            this.cfg.loader = "fabric".into();
        }
        // Latch onto the saved folder, or the standard one on first run.
        match this
            .cfg
            .root
            .clone()
            .filter(|p| p.is_dir())
            .or_else(mc::default_dir)
        {
            Some(root) => this.attach(root, cx),
            None => this.cfg.root = None,
        }

        if this.cfg.check_updates {
            this.check_for_update(false, cx);
        }

        // Fill the version picker with every release in the background.
        cx.spawn(async move |this, cx| {
            let versions = cx
                .background_executor()
                .spawn(async { modrinth::game_versions() })
                .await;
            this.update(cx, |this, cx| {
                if let Ok(v) = versions {
                    if this.cfg.mc_version.is_empty() {
                        this.cfg.mc_version = v.first().cloned().unwrap_or_default();
                    }
                    this.all_versions = v;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        this
    }

    fn attach(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.profiles = mc::detect_profiles(&root);
        let changed = self.cfg.root.as_deref() != Some(root.as_path());
        if changed || self.cfg.mc_version.is_empty() {
            if let Some(p) = self.profiles.first() {
                self.cfg.mc_version = p.mc_version.clone();
                if !p.loader.is_empty() {
                    self.cfg.loader = p.loader.clone();
                }
            }
        }
        if changed {
            self.identified.clear();
            self.checked.clear();
        }
        self.cfg.root = Some(root);
        self.cfg.save();
        self.rescan(cx);
        self.searched_once = false;
    }

    fn rescan(&mut self, cx: &mut Context<Self>) {
        if let Some(root) = &self.cfg.root {
            for k in Kind::ALL {
                self.entries[k as usize] = mc::scan(root, k);
            }
        }
        self.identify_new(cx);
    }

    /// Ask Modrinth about any file we haven't looked up yet.
    fn identify_new(&mut self, cx: &mut Context<Self>) {
        if !self.cfg.identify {
            return;
        }
        // (stable key, current path on disk)
        let todo: Vec<(PathBuf, PathBuf)> = self
            .entries
            .iter()
            .flatten()
            .filter(|e| !e.is_dir)
            .map(|e| (e.path.with_file_name(&e.file_name), e.path.clone()))
            .filter(|(key, _)| !self.checked.contains(key))
            .collect();
        if todo.is_empty() {
            return;
        }
        self.checked.extend(todo.iter().map(|(key, _)| key.clone()));
        self.identifying = true;
        cx.spawn(async move |this, cx| {
            let paths: Vec<PathBuf> = todo.iter().map(|(_, path)| path.clone()).collect();
            let result = cx
                .background_executor()
                .spawn(async move { modrinth::identify(&paths) })
                .await;
            this.update(cx, |this, cx| {
                this.identifying = false;
                this.identify_failed = result.is_err();
                match result {
                    Ok(found) => {
                        let n = found.len();
                        for (key, path) in &todo {
                            if let Some(id) = found.get(path) {
                                this.identified.insert(key.clone(), id.clone());
                            }
                        }
                        if !this.status_is_error {
                            this.ok(format!(
                                "Recognised {n} of {} files on Modrinth",
                                todo.len()
                            ));
                        }
                    }
                    Err(e) => {
                        // Forget them so "Check again" retries once we're online.
                        for (key, _) in &todo {
                            this.checked.remove(key);
                        }
                        this.fail(format!(
                            "Couldn't check files against Modrinth: {}",
                            friendly(&e)
                        ));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn ok(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_is_error = false;
    }
    fn fail(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_is_error = true;
    }

    /// The version + loader everything is searched and downloaded for.
    fn target(&self) -> Target {
        Target {
            mc_version: self.cfg.mc_version.clone(),
            loader: self.cfg.loader.clone(),
        }
    }

    fn loader_name(&self) -> &'static str {
        LOADERS
            .iter()
            .find(|l| l.0 == self.cfg.loader)
            .map_or("Fabric", |l| l.1)
    }

    fn choose(&mut self, menu: Menu, value: String, cx: &mut Context<Self>) {
        self.menu = None;
        match menu {
            Menu::Version => {
                // If this version is installed with a different loader, follow it.
                let installed: Vec<&Profile> = self
                    .profiles
                    .iter()
                    .filter(|p| p.mc_version == value && !p.loader.is_empty())
                    .collect();
                if !installed.is_empty() && !installed.iter().any(|p| p.loader == self.cfg.loader) {
                    self.cfg.loader = installed[0].loader.clone();
                }
                self.cfg.mc_version = value;
            }
            Menu::Loader => self.cfg.loader = value,
        }
        self.cfg.save();
        self.ok(format!(
            "Now installing for Minecraft {} - {}",
            self.cfg.mc_version,
            self.loader_name()
        ));
        self.install_state.clear();
        self.searched_once = false;
        if self.tab == Tab::Browse {
            self.start_search(0, cx);
        }
        cx.notify();
    }

    fn pick_folder(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| {
                if mc::looks_like_minecraft(&dir) {
                    this.ok(format!("Attached to {}", dir.display()));
                    this.attach(dir, cx);
                } else {
                    this.fail(format!(
                        "{} doesn't look like a Minecraft folder",
                        dir.display()
                    ));
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn pick_files(&mut self, kind: Kind, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            this.update(cx, |this, cx| {
                this.import_files(paths, Some(kind), cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn import_files(&mut self, files: Vec<PathBuf>, forced: Option<Kind>, cx: &mut Context<Self>) {
        let Some(root) = self.cfg.root.clone() else {
            return;
        };
        let (mut added, mut skipped) = (Vec::new(), Vec::new());
        for f in files {
            let name = f
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            // A jar is always a mod; for zips trust the open tab if we can't tell.
            let kind = mc::guess_kind(&f).or(forced.filter(|k| {
                *k != Kind::Mod && f.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip"))
            }));
            match kind.map(|k| (k, mc::import(&root, k, &f))) {
                Some((k, Ok(_))) => added.push(format!("{name} -> {}", k.dir())),
                Some((_, Err(e))) => skipped.push(format!("{name}: {e}")),
                None => skipped.push(format!(
                    "{name}: not a mod jar, resource pack or shader pack"
                )),
            }
        }
        self.rescan(cx);
        if !skipped.is_empty() {
            self.fail(format!("Skipped {}", skipped.join("; ")));
        } else if !added.is_empty() {
            self.ok(format!("Added {}", added.join(", ")));
        }
    }

    fn remove(&mut self, path: PathBuf, name: String, cx: &mut Context<Self>) {
        match trash::delete(&path) {
            Ok(()) => self.ok(format!("Removed {name}")),
            Err(e) => self.fail(format!("Couldn't remove {name}: {e}")),
        }
        self.confirm_delete = None;
        self.rescan(cx);
        cx.notify();
    }

    fn start_search(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.search_id += 1;
        self.searching = true;
        self.searched_once = true;
        self.offset = offset;
        let (id, kind, target) = (self.search_id, self.browse_kind, self.target());
        let query = self.search.read(cx).text();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { modrinth::search(&query, kind, Some(&target), offset) })
                .await;
            this.update(cx, |this, cx| {
                if id != this.search_id {
                    return; // an older search finished late
                }
                this.searching = false;
                match result {
                    Ok(r) => {
                        this.hits = r.hits;
                        this.total_hits = r.total_hits;
                    }
                    Err(e) => this.fail(format!("Modrinth search failed: {}", friendly(&e))),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn start_install(&mut self, hit: &Hit, cx: &mut Context<Self>) {
        let Some(root) = self.cfg.root.clone() else {
            return;
        };
        self.installing.insert(hit.project_id.clone());
        let (project, title, kind, target) = (
            hit.project_id.clone(),
            hit.title.clone(),
            self.browse_kind,
            self.target(),
        );
        cx.spawn(async move |this, cx| {
            let p = project.clone();
            let result = cx
                .background_executor()
                .spawn(async move { modrinth::install(&root, &p, kind, Some(&target)) })
                .await;
            this.update(cx, |this, cx| {
                this.installing.remove(&project);
                this.install_state.insert(project, result.is_ok());
                match result {
                    Ok(files) if files.is_empty() => {
                        this.ok(format!("{title} is already installed"))
                    }
                    Ok(files) => this.ok(format!("Installed {title}: {}", files.join(", "))),
                    Err(e) => this.fail(format!("Couldn't install {title}: {}", friendly(&e))),
                }
                this.rescan(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn open_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.confirm_delete = None;
        self.filter.update(cx, |f, cx| f.clear(cx));
        if tab == Tab::Browse && !self.searched_once {
            self.start_search(0, cx);
        }
        cx.notify();
    }

    fn set_browse_kind(&mut self, kind: Kind, cx: &mut Context<Self>) {
        self.browse_kind = kind;
        self.install_state.clear();
        self.search.update(cx, |s, cx| {
            s.set_placeholder(format!("Search {}...", kind.label().to_lowercase()), cx)
        });
        self.start_search(0, cx);
    }

    // ---------------------------------------------------------------- top bar

    fn check_for_update(&mut self, manual: bool, cx: &mut Context<Self>) {
        if matches!(self.updater, Updater::Checking | Updater::Installing(_)) {
            return;
        }
        self.updater = Updater::Checking;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async { update::check() })
                .await;
            this.update(cx, |this, cx| {
                this.updater = match result {
                    Ok(Some(u)) => {
                        this.ok(format!("Hopper {} is available", u.tag));
                        Updater::Available(u)
                    }
                    Ok(None) => Updater::UpToDate,
                    // A failed automatic check stays quiet.
                    Err(e) if manual => Updater::Failed(friendly(&e)),
                    Err(_) => Updater::Idle,
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn install_update(&mut self, cx: &mut Context<Self>) {
        let Updater::Available(u) = &self.updater else {
            return;
        };
        let u = u.clone();
        self.ok(format!("Downloading Hopper {}...", u.tag));
        self.updater = Updater::Installing(u.clone());
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { update::install(&u) })
                .await;
            this.update(cx, |this, cx| match result {
                Ok(path) => {
                    cx.set_restart_path(path);
                    cx.restart();
                }
                Err(e) => {
                    this.fail(format!("Update failed: {}", friendly(&e)));
                    this.updater = Updater::Failed(friendly(&e));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn top_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        // Versions found in the folder first, then every other release.
        let mut items = Vec::new();
        let mut local: Vec<String> = Vec::new();
        for p in &self.profiles {
            if !local.contains(&p.mc_version) {
                local.push(p.mc_version.clone());
            }
        }
        if !local.is_empty() {
            items.push(MenuItem::Header("In this folder"));
            items.extend(local.iter().cloned().map(|v| MenuItem::Item {
                label: v.clone(),
                value: v,
            }));
        }
        if self.all_versions.is_empty() {
            items.push(MenuItem::Header("More need a connection"));
        } else if !local.is_empty() {
            items.push(MenuItem::Header("All versions"));
        }
        items.extend(
            self.all_versions
                .iter()
                .filter(|v| !local.contains(v))
                .cloned()
                .map(|v| MenuItem::Item {
                    label: v.clone(),
                    value: v,
                }),
        );
        let loaders = LOADERS
            .iter()
            .map(|(id, name)| MenuItem::Item {
                label: (*name).into(),
                value: (*id).into(),
            })
            .collect();
        let version_label = if self.cfg.mc_version.is_empty() {
            "Pick".to_owned()
        } else {
            self.cfg.mc_version.clone()
        };

        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .child(ui::grass_strip())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_4()
                    .py_2()
                    .bg(rgb(BAR))
                    .border_b_2()
                    .border_color(rgb(BLACK))
                    .child(ui::app_icon(30.))
                    .child(ui::title("HOPPER", 22.))
                    .child(div().flex_1())
                    .map(|bar| match &self.updater {
                        Updater::Available(u) => bar.child(
                            ui::gold_button("update", format!("Update to {}", u.tag))
                                .on_click(cx.listener(|this, _, _, cx| this.install_update(cx))),
                        ),
                        Updater::Installing(u) => bar.child(
                            div()
                                .text_color(rgb(GOLD))
                                .child(format!("Updating to {}...", u.tag)),
                        ),
                        _ => bar,
                    })
                    .child(div().text_color(rgb(MUTED)).child("Install for"))
                    .child(self.dropdown(
                        Menu::Version,
                        version_label,
                        items,
                        self.cfg.mc_version.clone(),
                        px(150.),
                        cx,
                    ))
                    .child(div().text_color(rgb(MUTED)).child("with"))
                    .child(self.dropdown(
                        Menu::Loader,
                        self.loader_name().into(),
                        loaders,
                        self.cfg.loader.clone(),
                        px(140.),
                        cx,
                    )),
            )
    }

    fn dropdown(
        &self,
        menu: Menu,
        label: String,
        items: Vec<MenuItem>,
        current: String,
        width: gpui::Pixels,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let open = self.menu == Some(menu);
        let id = menu as usize;
        let trigger = ui::raised(("dropdown", id), 0x3a3a40, 0x6a6a72, 0x1e1e22, 0x4a4a54)
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .text_color(rgb(if open { GOLD } else { TEXT }))
                            .child(label),
                    )
                    .child(
                        div()
                            .text_color(rgb(MUTED))
                            .child(if open { "^" } else { "v" }),
                    ),
            )
            .0
            .w(width)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.menu = if this.menu == Some(menu) {
                    None
                } else {
                    Some(menu)
                };
                cx.notify();
            }));
        div().relative().child(trigger).when(open, |d| {
            d.child(
                deferred(
                    div()
                        .id(("menu", id))
                        .occlude()
                        .absolute()
                        .top(px(36.))
                        .right_0()
                        .w(width)
                        .max_h(px(380.))
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .p(px(3.))
                        .bg(rgb(0x141416))
                        .border_2()
                        .border_color(rgb(0x5a3fa0))
                        .children(items.into_iter().enumerate().map(|(ix, item)| {
                            match item {
                                MenuItem::Header(text) => div()
                                    .flex_shrink_0()
                                    .px_2()
                                    .pt_2()
                                    .pb_1()
                                    .text_size(px(10.))
                                    .text_color(rgb(MUTED))
                                    .child(text)
                                    .into_any_element(),
                                MenuItem::Item { label, value } => {
                                    let selected = value == current;
                                    div()
                                        .id(("menu-item", id * 10_000 + ix))
                                        .flex_shrink_0()
                                        .px_2()
                                        .py_1()
                                        .cursor_pointer()
                                        .text_color(rgb(if selected { GOLD } else { TEXT }))
                                        .when(selected, |d| d.bg(rgb(0x2a2a30)))
                                        .hover(|s| s.bg(rgb(0x34343c)).text_color(rgb(0xffff9a)))
                                        .child(if selected {
                                            format!("> {label}")
                                        } else {
                                            label
                                        })
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.choose(menu, value.clone(), cx)
                                        }))
                                        .into_any_element()
                                }
                            }
                        })),
                )
                .with_priority(2),
            )
        })
    }

    // ---------------------------------------------------------------- sidebar

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab_button = |id: usize,
                          icon: Icon,
                          label: String,
                          count: Option<String>,
                          tab: Tab,
                          cx: &mut Context<Self>| {
            let active = self.tab == tab;
            div()
                .id(("tab", id))
                .h(px(36.))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .border_2()
                .when(active, |d| {
                    d.bg(rgb(GREEN_DARK)).border_color(rgb(0x9ad97c))
                })
                .when(!active, |d| {
                    d.border_color(gpui::transparent_black())
                        .hover(|s| s.bg(rgba(0xffffff14)).border_color(rgb(0x55555c)))
                })
                .child(ui::icon(icon, 2.))
                .child(div().flex_1().child(label))
                .children(count.map(|c| {
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(if active { 0xd8f5c8 } else { MUTED }))
                        .child(c)
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.open_tab(tab, cx)))
        };
        let icons = [Icon::Mods, Icon::Packs, Icon::Shaders];
        let mut items = div().relative().flex().flex_col().gap_1().p_2().size_full();
        for (k, icon) in Kind::ALL.into_iter().zip(icons) {
            let list = &self.entries[k as usize];
            let on = list.iter().filter(|e| e.enabled).count();
            items = items.child(tab_button(
                k as usize,
                icon,
                k.label().into(),
                Some(format!("{on}/{}", list.len())),
                Tab::Installed(k),
                cx,
            ));
        }
        items = items
            .child(div().h(px(2.)).my_2().bg(rgb(BLACK)))
            .child(tab_button(
                9,
                Icon::Download,
                "Modrinth".into(),
                None,
                Tab::Browse,
                cx,
            ))
            .child(div().flex_1())
            .child(
                div()
                    .p_2()
                    .bg(rgba(0x00000066))
                    .text_size(px(10.))
                    .text_color(rgb(MUTED))
                    .child("Drop .jar or .zip files on this window to add them."),
            )
            .child(tab_button(
                10,
                Icon::Options,
                "Options".into(),
                None,
                Tab::Options,
                cx,
            ));
        div()
            .relative()
            .w(px(210.))
            .flex_shrink_0()
            .overflow_hidden()
            .border_r_2()
            .border_color(rgb(BLACK))
            .child(ui::stone_backdrop())
            .child(items)
    }

    // -------------------------------------------------------------- installed

    fn installed(&self, kind: Kind, cx: &mut Context<Self>) -> impl IntoElement {
        let all = &self.entries[kind as usize];
        let any_off = all.iter().any(|e| !e.enabled);
        let needle = self.filter.read(cx).text().to_lowercase();
        let list: Vec<Entry> = all
            .iter()
            .filter(|e| {
                let title = self
                    .identified
                    .get(&e.path.with_file_name(&e.file_name))
                    .map(|k| k.title.to_lowercase());
                needle.is_empty()
                    || e.name.to_lowercase().contains(&needle)
                    || e.file_name.to_lowercase().contains(&needle)
                    || title.is_some_and(|t| t.contains(&needle))
            })
            .cloned()
            .collect();

        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .child(ui::heading(kind.label()))
            .child(div().flex_1())
            .child(div().w(px(200.)).flex().child(self.filter.clone()))
            .when(!all.is_empty(), |d| {
                d.child(
                    ui::button(
                        "toggle-all",
                        if any_off { "Enable all" } else { "Disable all" },
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let errors: Vec<String> = this.entries[kind as usize]
                            .iter()
                            .filter_map(|e| {
                                mc::set_enabled(&e.path, any_off)
                                    .err()
                                    .map(|e| e.to_string())
                            })
                            .collect();
                        this.rescan(cx);
                        if !errors.is_empty() {
                            this.fail(errors.join("; "));
                        }
                        cx.notify();
                    })),
                )
            })
            .child(
                ui::button("refresh", "Refresh").on_click(cx.listener(|this, _, _, cx| {
                    this.rescan(cx);
                    cx.notify();
                })),
            )
            .child(
                ui::primary_button("add-file", format!("+ .{}", kind.extension()))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_files(kind, cx))),
            );

        let body = if list.is_empty() {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(rgb(MUTED))
                .child(ui::icon(Icon::Logo, 6.))
                .child(
                    div()
                        .text_size(px(15.))
                        .text_color(rgb(TEXT))
                        .child(format!("Nothing in {} yet", kind.dir())),
                )
                .child("Add a file, drop one here, or get one from Modrinth.")
                .into_any_element()
        } else {
            div()
                .id(("installed-list", kind as usize))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2()
                .pr_1()
                .children(
                    list.into_iter()
                        .enumerate()
                        .map(|(ix, e)| self.entry_row(ix, e, cx)),
                )
                .into_any_element()
        };
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child(header)
            .child(body)
    }

    fn entry_row(&self, ix: usize, e: Entry, cx: &mut Context<Self>) -> AnyElement {
        let key = e.path.with_file_name(&e.file_name);
        let known = self.identified.get(&key).cloned();
        let display_name = known.as_ref().map_or(e.name.clone(), |k| k.title.clone());
        let version = match &known {
            Some(k) if !k.version_number.is_empty() => k.version_number.clone(),
            _ => e.version.clone(),
        };
        let mut bits = Vec::new();
        if !version.is_empty() {
            bits.push(version);
        }
        if !e.loader.is_empty() {
            bits.push(e.loader.clone());
        }
        bits.push(if e.is_dir {
            "folder".to_owned()
        } else {
            human_size(e.size)
        });
        bits.push(e.file_name.clone());

        let badge = match &known {
            Some(k) => {
                let url = format!("https://modrinth.com/{}/{}", k.project_type, k.slug);
                ui::tag("MODRINTH", 0x0e1a0a, GREEN)
                    .id(("badge", ix))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(0x8fe06a)))
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
                    .into_any_element()
            }
            None if e.is_dir || !self.cfg.identify || self.identify_failed => {
                div().into_any_element()
            }
            None if self.checked.contains(&key) && !self.identifying => {
                ui::tag("LOCAL", MUTED, 0x3a3a40).into_any_element()
            }
            None => ui::tag("CHECKING", MUTED, 0x3a3a40).into_any_element(),
        };

        let confirming = self.confirm_delete.as_ref() == Some(&e.path);
        let (path, name, enabled) = (e.path.clone(), display_name.clone(), e.enabled);

        let toggle = {
            let (path, name) = (path.clone(), name.clone());
            ui::switch(("switch", ix), enabled).on_click(cx.listener(move |this, _, _, cx| {
                match mc::set_enabled(&path, !enabled) {
                    Ok(_) => this.ok(format!(
                        "{name} {}",
                        if enabled { "disabled" } else { "enabled" }
                    )),
                    Err(e) => this.fail(format!("Couldn't switch {name}: {e}")),
                }
                this.rescan(cx);
                cx.notify();
            }))
        };

        let actions = if confirming {
            let (path, name) = (path.clone(), name.clone());
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().text_color(rgb(RED)).child("Trash it?"))
                .child(ui::button(("yes", ix), "Yes").on_click(
                    cx.listener(move |this, _, _, cx| this.remove(path.clone(), name.clone(), cx)),
                ))
                .child(
                    ui::button(("no", ix), "No").on_click(cx.listener(|this, _, _, cx| {
                        this.confirm_delete = None;
                        cx.notify();
                    })),
                )
        } else {
            let name = name.clone();
            div().child(ui::button(("remove", ix), "Remove").on_click(cx.listener(
                move |this, _, _, cx| {
                    if this.cfg.confirm_remove {
                        this.confirm_delete = Some(path.clone());
                        cx.notify();
                    } else {
                        this.remove(path.clone(), name.clone(), cx);
                    }
                },
            )))
        };

        ui::slot(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .child(toggle)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(if enabled { TEXT } else { MUTED }))
                                        .when(!enabled, |d| d.line_through())
                                        .child(display_name),
                                )
                                .child(badge),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(rgb(MUTED))
                                .child(bits.join("  |  ")),
                        ),
                )
                .child(actions),
        )
        .into_any_element()
    }

    // ----------------------------------------------------------------- browse

    fn browse(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut kinds = div().flex().items_center().gap_1();
        for k in Kind::ALL {
            let active = self.browse_kind == k;
            kinds = kinds.child(
                div()
                    .id(("browse-kind", k as usize))
                    .px_3()
                    .py_1()
                    .cursor_pointer()
                    .border_2()
                    .when(active, |d| {
                        d.bg(rgb(GREEN_DARK)).border_color(rgb(0x9ad97c))
                    })
                    .when(!active, |d| {
                        d.border_color(rgb(0x3a3a40)).hover(|s| s.bg(rgb(0x2a2a30)))
                    })
                    .child(k.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.browse_kind != k {
                            this.set_browse_kind(k, cx);
                        }
                    })),
            );
        }
        let scope = if self.browse_kind == Kind::Mod {
            format!("for {} {}", self.cfg.mc_version, self.loader_name())
        } else {
            format!("for {}", self.cfg.mc_version)
        };

        let header = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(ui::heading("Modrinth"))
                    .child(kinds)
                    .child(div().flex_1())
                    .child(ui::tag(scope.to_uppercase(), 0x0e1a0a, GREEN)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.search.clone())
                    .child(
                        ui::primary_button("search", "Search")
                            .on_click(cx.listener(|this, _, _, cx| this.start_search(0, cx))),
                    ),
            );

        let body = if self.hits.is_empty() {
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2()
                .text_color(rgb(MUTED))
                .child(ui::icon(Icon::Download, 6.))
                .child(if self.searching {
                    "Searching..."
                } else {
                    "No results for this version"
                })
                .into_any_element()
        } else {
            let last = (self.offset + self.hits.len()).min(self.total_hits);
            let (prev, next) = (
                self.offset.saturating_sub(modrinth::PAGE),
                self.offset + modrinth::PAGE,
            );
            div()
                .id("browse-list")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_2()
                .pr_1()
                .children(
                    self.hits
                        .iter()
                        .cloned()
                        .enumerate()
                        .map(|(ix, h)| self.hit_row(ix, h, cx)),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .gap_3()
                        .py_2()
                        .when(self.offset > 0, |d| {
                            d.child(ui::button("prev", "< Prev").on_click(
                                cx.listener(move |this, _, _, cx| this.start_search(prev, cx)),
                            ))
                        })
                        .child(div().text_color(rgb(MUTED)).child(format!(
                            "{}-{} of {}",
                            self.offset + 1,
                            last,
                            self.total_hits
                        )))
                        .when(last < self.total_hits, |d| {
                            d.child(ui::button("next", "Next >").on_click(
                                cx.listener(move |this, _, _, cx| this.start_search(next, cx)),
                            ))
                        })
                        .when(self.searching, |d| {
                            d.child(div().text_color(rgb(MUTED)).child("Loading..."))
                        }),
                )
                .into_any_element()
        };
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child(header)
            .child(body)
    }

    fn hit_row(&self, ix: usize, h: Hit, cx: &mut Context<Self>) -> AnyElement {
        let url = format!("https://modrinth.com/{}/{}", h.project_type, h.slug);
        let install = if self.installing.contains(&h.project_id) {
            div()
                .w(px(110.))
                .flex()
                .justify_center()
                .text_color(rgb(GOLD))
                .child("Installing")
                .into_any_element()
        } else {
            let have = self
                .identified
                .values()
                .any(|k| k.project_id == h.project_id);
            let hit = h.clone();
            let on_click = cx.listener(move |this, _, _, cx| this.start_install(&hit, cx));
            match self.install_state.get(&h.project_id) {
                Some(true) => ui::button(("install", ix), "Installed")
                    .w(px(110.))
                    .text_color(rgb(GREEN))
                    .on_click(on_click),
                None if have => ui::button(("install", ix), "Installed")
                    .w(px(110.))
                    .text_color(rgb(GREEN))
                    .on_click(on_click),
                Some(false) => ui::button(("install", ix), "Retry")
                    .w(px(110.))
                    .text_color(rgb(RED))
                    .on_click(on_click),
                None => ui::primary_button(("install", ix), "Install")
                    .w(px(110.))
                    .on_click(on_click),
            }
            .into_any_element()
        };
        ui::slot(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap_3()
                .px_3()
                .py_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::BOLD)
                                        .child(h.title),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(11.))
                                        .text_color(rgb(MUTED))
                                        .child(format!(
                                            "by {} | {} downloads",
                                            h.author,
                                            human_count(h.downloads)
                                        )),
                                ),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(rgb(0xb8b8c0))
                                .child(h.description),
                        ),
                )
                .child(
                    div()
                        .id(("page", ix))
                        .cursor_pointer()
                        .text_color(rgb(0x7ab6e8))
                        .hover(|s| s.underline())
                        .child("Page")
                        .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url))),
                )
                .child(install),
        )
        .into_any_element()
    }

    // ---------------------------------------------------------------- options

    fn options(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let root = self.cfg.root.clone().unwrap_or_default();
        let row = |title: &'static str, detail: String| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().font_weight(FontWeight::BOLD).child(title))
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(rgb(MUTED))
                        .child(detail),
                )
        };
        let section = |items: Vec<AnyElement>| {
            ui::slot(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_3()
                    .children(items),
            )
        };
        let recognised = self.identified.len();
        let files = self.entries.iter().flatten().filter(|e| !e.is_dir).count();

        div()
            .id("options")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .child(ui::heading("Options"))
            .child(
                section(vec![
                    row("Minecraft folder", root.display().to_string()).into_any_element(),
                    ui::button("open-folder", "Open").on_click(cx.listener(|this, _, _, cx| {
                        if let Some(r) = &this.cfg.root {
                            cx.open_with_system(r);
                        }
                    })).into_any_element(),
                    ui::primary_button("change-folder", "Change...").on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx))).into_any_element(),
                ]),
            )
            .child(
                section(vec![
                    ui::switch("identify", self.cfg.identify).on_click(cx.listener(|this, _, _, cx| {
                        this.cfg.identify = !this.cfg.identify;
                        this.cfg.save();
                        this.identify_new(cx);
                        cx.notify();
                    })).into_any_element(),
                    row(
                        "Recognise files on Modrinth",
                        if self.cfg.identify && self.identify_failed {
                            "Couldn't reach Modrinth. Press Check again when you're online.".into()
                        } else if self.cfg.identify {
                            format!("{recognised} of {files} files matched. Uses each file's hash; nothing else is sent.")
                        } else {
                            "Off: files are shown by their own names.".into()
                        },
                    ).into_any_element(),
                    ui::button("recheck", "Check again").on_click(cx.listener(|this, _, _, cx| {
                        this.identified.clear();
                        this.checked.clear();
                        this.rescan(cx);
                        cx.notify();
                    })).into_any_element(),
                ]),
            )
            .child(
                section(vec![
                    ui::switch("confirm", self.cfg.confirm_remove).on_click(cx.listener(|this, _, _, cx| {
                        this.cfg.confirm_remove = !this.cfg.confirm_remove;
                        this.cfg.save();
                        cx.notify();
                    })).into_any_element(),
                    row("Ask before removing", "Removed files always go to the trash, never straight to deletion.".into()).into_any_element(),
                ]),
            )
            .child(
                section(vec![
                    ui::switch("auto-update", self.cfg.check_updates)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.cfg.check_updates = !this.cfg.check_updates;
                            this.cfg.save();
                            cx.notify();
                        }))
                        .into_any_element(),
                    row("Check for updates when Hopper starts", self.update_line()).into_any_element(),
                    match &self.updater {
                        Updater::Available(u) => ui::gold_button("options-update", format!("Update to {}", u.tag))
                            .on_click(cx.listener(|this, _, _, cx| this.install_update(cx)))
                            .into_any_element(),
                        Updater::Checking | Updater::Installing(_) => {
                            div().text_color(rgb(MUTED)).child("Working...").into_any_element()
                        }
                        Updater::Failed(_) => ui::button("download-page", "Download page")
                            .on_click(cx.listener(|_, _, _, cx| cx.open_url(update::DOWNLOAD_PAGE)))
                            .into_any_element(),
                        _ => ui::button("check-update", "Check now")
                            .on_click(cx.listener(|this, _, _, cx| this.check_for_update(true, cx)))
                            .into_any_element(),
                    },
                ]),
            )
            .child(
                section(vec![
                    ui::app_icon(40.).into_any_element(),
                    row("Hopper", format!("Version {} | MIT license", Self::version_label())).into_any_element(),
                    ui::button("repo", "GitHub").on_click(cx.listener(|_, _, _, cx| cx.open_url(REPO_URL))).into_any_element(),
                ]),
            )
    }

    fn version_label() -> String {
        update::current_tag().map_or_else(
            || format!("{} (built from source)", env!("CARGO_PKG_VERSION")),
            str::to_owned,
        )
    }

    fn update_line(&self) -> String {
        if update::current_tag().is_none() {
            return "Only release builds can update themselves.".into();
        }
        match &self.updater {
            Updater::Idle => "One click to download, install and restart.".into(),
            Updater::Checking => "Checking GitHub for a new version...".into(),
            Updater::UpToDate => {
                format!("You're on the latest version ({}).", Self::version_label())
            }
            Updater::Available(u) => {
                format!("{} is out. Hopper will restart after updating.", u.tag)
            }
            Updater::Installing(u) => format!("Installing {}...", u.tag),
            Updater::Failed(e) => format!("Couldn't update: {e}"),
        }
    }

    fn welcome(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(ui::grass_strip())
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_4()
                    .child(ui::app_icon(96.))
                    .child(ui::title("HOPPER", 40.))
                    .child("Couldn't find a Minecraft folder in the usual place.")
                    .child(
                        ui::primary_button("choose", "Choose Minecraft folder...")
                            .on_click(cx.listener(|this, _, _, cx| this.pick_folder(cx))),
                    )
                    .when(!self.status.is_empty(), |d| {
                        d.child(div().text_color(rgb(RED)).child(self.status.clone()))
                    }),
            )
    }
}

impl Render for Hopper {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let base = div()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .text_size(px(13.))
            .font_family(ui::FONT);
        if self.cfg.root.is_none() {
            return base.child(self.welcome(cx));
        }
        let forced = match self.tab {
            Tab::Installed(k) => Some(k),
            _ => None,
        };
        let content = match self.tab {
            Tab::Installed(k) => self.installed(k, cx).into_any_element(),
            Tab::Browse => self.browse(cx).into_any_element(),
            Tab::Options => self.options(cx).into_any_element(),
        };
        base.relative()
            .flex()
            .flex_col()
            .drag_over::<ExternalPaths>(|s, _, _, _| s.bg(rgb(0x1e3a14)))
            .on_drop(cx.listener(move |this, paths: &ExternalPaths, _, cx| {
                this.import_files(paths.paths().to_vec(), forced, cx);
                cx.notify();
            }))
            .child(self.top_bar(cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.sidebar(cx))
                    .child(content),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px_3()
                    .py_1()
                    .bg(rgb(0x111113))
                    .border_t_2()
                    .border_color(rgb(BLACK))
                    .truncate()
                    .text_size(px(11.))
                    .text_color(rgb(if self.status_is_error { RED } else { MUTED }))
                    .child(if self.status.is_empty() {
                        "Ready".to_owned()
                    } else {
                        self.status.clone()
                    }),
            )
            // Clicking anywhere else closes an open dropdown.
            .when(self.menu.is_some(), |d| {
                d.child(
                    deferred(
                        div()
                            .id("backdrop")
                            .occlude()
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .bg(rgba(0x00000001))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.menu = None;
                                cx.notify();
                            })),
                    )
                    .with_priority(1),
                )
            })
    }
}

enum Updater {
    Idle,
    Checking,
    UpToDate,
    Available(update::Update),
    Installing(update::Update),
    Failed(String),
}

enum MenuItem {
    Header(&'static str),
    Item { label: String, value: String },
}

/// Turn low-level network errors into something readable.
fn friendly(e: &str) -> String {
    let lower = e.to_lowercase();
    if ["dns", "connection", "timed out", "network", "resolve"]
        .iter()
        .any(|w| lower.contains(w))
    {
        "can't reach Modrinth (are you offline?)".into()
    } else {
        e.to_owned()
    }
}

fn human_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.0} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

fn human_count(n: u64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1e6),
        n if n >= 1_000 => format!("{:.1}k", n as f64 / 1e3),
        n => n.to_string(),
    }
}
