pub mod builder;
pub mod process;
mod report;
pub mod state;
pub mod theme;
mod update;
pub mod util;
pub mod widgets;

use std::io::Cursor;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use egui::{
    pos2, Align, Align2, Color32, Context, Frame as EguiFrame, Layout, Margin, Rect, RichText, ScrollArea,
    Sense, TextureHandle, TextureOptions, Vec2, ViewportBuilder,
};

use state::{AppCore, Injection, Level, Route, StartupInstance, View, APP_TITLE};
use theme::Palette;
use update::UpdateInfo;

const PANORAMA_IMAGES: [&[u8]; 9] = [
    include_bytes!("../../assets/panorama/0.jpg"),
    include_bytes!("../../assets/panorama/1.jpg"),
    include_bytes!("../../assets/panorama/2.jpg"),
    include_bytes!("../../assets/panorama/3.jpg"),
    include_bytes!("../../assets/panorama/4.jpg"),
    include_bytes!("../../assets/panorama/5.jpg"),
    include_bytes!("../../assets/panorama/6.jpg"),
    include_bytes!("../../assets/panorama/7.jpg"),
    include_bytes!("../../assets/panorama/8.jpg"),
];
const PANORAMA_FRAME_SECONDS: f64 = 12.0;
const PANORAMA_FADE_SECONDS: f64 = 1.6;
const PANORAMA_OVERLAY: f32 = 0.56;
const APP_ICON: &[u8] = include_bytes!("../../assets/images/app-icon.png");
const HERO_BANNER: &[u8] = include_bytes!("../../assets/images/hero.png");
const EASTER_EGG_GIF: &[u8] = include_bytes!("../../assets/images/easter-egg.gif");

struct PanoramaState {
    current: usize,
    changed_at: f64,
}

struct AnimatedImage {
    frames: Vec<(TextureHandle, f64)>,
    duration: f64,
}

impl Default for PanoramaState {
    fn default() -> Self {
        Self {
            current: 0,
            changed_at: 0.0,
        }
    }
}

impl PanoramaState {
    fn elapsed(&self, time: f64) -> f64 {
        (time - self.changed_at).max(0.0)
    }

    fn advance(&mut self, time: f64, count: usize) {
        if count == 0 {
            return;
        }
        let elapsed = self.elapsed(time);
        if elapsed >= PANORAMA_FRAME_SECONDS {
            let steps = (elapsed / PANORAMA_FRAME_SECONDS).floor() as usize;
            self.current = (self.current + steps) % count;
            self.changed_at += steps as f64 * PANORAMA_FRAME_SECONDS;
        }
    }
}

pub fn run() -> Result<(), eframe::Error> {
    let icon = image::load_from_memory(APP_ICON)
        .expect("the embedded application icon must be a valid image")
        .to_rgba8();
    let (width, height) = (icon.width(), icon.height());
    let options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size([520.0, 640.0])
            .with_min_inner_size([360.0, 420.0])
            .with_app_id("jar-to-dynamic-library-gui")
            .with_icon(egui::IconData {
                rgba: icon.into_raw(),
                width,
                height,
            }),
        ..Default::default()
    };
    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|creation| {
            theme::apply(&creation.egui_ctx);
            Ok(Box::new(JtdlGui::new(&creation.egui_ctx)))
        }),
    )
}

pub struct JtdlGui {
    core: AppCore,
    logo: TextureHandle,
    hero_banner: TextureHandle,
    github_logo: TextureHandle,
    discord_logo: TextureHandle,
    easter_egg: AnimatedImage,
    easter_egg_open: bool,
    easter_egg_started_at: f64,
    target_icons: Vec<TextureHandle>,
    panoramas: Vec<TextureHandle>,
    panorama: PanoramaState,
    hovered_process: Option<u32>,
    hovered_target: Option<usize>,
    action_hovered: bool,
    startup_instances: Vec<StartupInstance>,
    startup_picker_open: bool,
    update_receiver: Receiver<Result<Option<UpdateInfo>, String>>,
    update_checked: bool,
    available_update: Option<UpdateInfo>,
}

impl JtdlGui {
    fn new(ctx: &Context) -> Self {
        let logo = load_embedded_texture(ctx, "jtdl-logo", widgets::BRAND_ICON);
        let hero_banner = load_embedded_texture(ctx, "jtdl-hero-banner", HERO_BANNER);
        let github_logo = load_embedded_texture(ctx, "jtdl-github-logo", widgets::GITHUB_ICON);
        let discord_logo = load_embedded_texture(ctx, "jtdl-discord-logo", widgets::DISCORD_ICON);
        let target_icons = builder::TARGETS
            .iter()
            .enumerate()
            .map(|(index, target)| load_embedded_texture(ctx, &format!("os-icon-{index}"), target.icon))
            .collect();
        let (update_sender, update_receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = update_sender.send(update::check_for_update());
        });
        Self {
            core: AppCore::default(),
            logo,
            hero_banner,
            github_logo,
            discord_logo,
            easter_egg: load_animated_image(ctx, "jtdl-easter-egg", EASTER_EGG_GIF),
            easter_egg_open: false,
            easter_egg_started_at: 0.0,
            target_icons,
            panoramas: load_panorama_frames(ctx),
            panorama: PanoramaState::default(),
            hovered_process: None,
            hovered_target: None,
            action_hovered: false,
            startup_instances: Vec::new(),
            startup_picker_open: false,
            update_receiver,
            update_checked: false,
            available_update: None,
        }
    }
}

impl eframe::App for JtdlGui {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        self.core.poll();
        if !self.update_checked {
            match self.update_receiver.try_recv() {
                Ok(Ok(update)) => {
                    self.available_update = update;
                    self.update_checked = true;
                }
                Ok(Err(error)) => {
                    self.core
                        .log(Level::Warn, format!("Update check failed: {error}"));
                    self.update_checked = true;
                }
                Err(TryRecvError::Disconnected) => {
                    self.core.log(Level::Error, "Update check stopped unexpectedly.");
                    self.update_checked = true;
                }
                Err(TryRecvError::Empty) => {}
            }
        }
        let time = ctx.input(|input| input.time);
        if ctx.input(|input| {
            (input.modifiers.ctrl || input.modifiers.command)
                && input.modifiers.shift
                && input.key_pressed(egui::Key::G)
        }) {
            self.easter_egg_open = true;
            self.easter_egg_started_at = time;
        }

        egui::TopBottomPanel::top("header")
            .frame(theme::header_frame())
            .show(ctx, |ui| {
                if let Some(next) = widgets::header(
                    ui,
                    &self.logo,
                    &self.github_logo,
                    &self.discord_logo,
                    self.core.view,
                ) {
                    self.core.set_view(next);
                }
            });

        match self.core.view {
            View::Build => {
                egui::CentralPanel::default()
                    .frame(EguiFrame::none().fill(Palette::BACKGROUND))
                    .show(ctx, |ui| {
                        let time = ctx.input(|input| input.time);
                        self.panorama.advance(time, self.panoramas.len());
                        paint_panorama(ui, &self.panoramas, &self.panorama, time);
                        scrolling_content(ui, "build-content", |ui| self.build_view(ui));
                    });
            }
            View::Injector => self.injector_view(ctx),
            View::Changelog => {
                egui::CentralPanel::default()
                    .frame(EguiFrame::none().fill(Palette::BACKGROUND))
                    .show(ctx, |ui| {
                        let time = ctx.input(|input| input.time);
                        self.panorama.advance(time, self.panoramas.len());
                        paint_panorama(ui, &self.panoramas, &self.panorama, time);
                        scrolling_content(ui, "changelog-content", widgets::changelog_panel);
                    });
            }
        }

        if self.easter_egg_open
            && show_animated_image(ctx, &self.easter_egg, self.easter_egg_started_at, time)
        {
            self.easter_egg_open = false;
        }

        confirm_modal(ctx, &mut self.core);
        update_modal(ctx, &mut self.available_update);
        startup_instance_modal(
            ctx,
            &mut self.core,
            &self.startup_instances,
            &mut self.startup_picker_open,
        );

        ctx.request_repaint_after(Duration::from_millis(32));
        if self.core.building || self.core.injection.is_busy() {
            ctx.request_repaint();
        }
    }
}

impl JtdlGui {
    fn build_view(&mut self, ui: &mut egui::Ui) {
        let banner_width = ui.available_width();
        let banner_height = banner_width * self.hero_banner.size_vec2().y / self.hero_banner.size_vec2().x;
        ui.add(
            egui::Image::new((self.hero_banner.id(), Vec2::new(banner_width, banner_height)))
                .fit_to_exact_size(Vec2::new(banner_width, banner_height)),
        );
        ui.add_space(8.0);
        widgets::section_heading(ui, "SELECT MOD JAR");
        ui.horizontal(|ui| {
            if widgets::tool_button(ui, "Choose .jar", !self.core.building).clicked() {
                self.core.pick_jar();
            }
            ui.label(
                RichText::new(self.core.jar_label())
                    .size(12.5)
                    .color(Palette::TEXT_DIM),
            );
        });
        ui.add_space(10.0);

        widgets::section_heading(ui, "TARGET PLATFORMS");
        self.target_grid(ui);
        ui.label(
            RichText::new("The mod stays in its Java JAR; these libraries provide the JVM injector.")
                .size(11.0)
                .color(Palette::TEXT_DIM),
        );
        ui.add_space(12.0);

        let can_build =
            !self.core.building && self.core.jar.is_some() && !self.core.selected_targets.is_empty();
        let label = if self.core.building {
            "Building..."
        } else {
            "Build libraries"
        };
        let clicked = widgets::inject_button(ui, label, can_build, self.action_hovered).clicked();
        self.action_hovered = ui.rect_contains_pointer(ui.min_rect());
        if clicked {
            self.core.start_build();
        }
        ui.add_space(8.0);
        if self.core.building {
            ui.add(
                egui::ProgressBar::new(self.core.progress)
                    .text(RichText::new(format!("{} target(s)", self.core.selected_targets.len())).size(11.0))
                    .animate(true),
            );
            ui.add_space(8.0);
        }

        widgets::log_panel(ui, &self.core.log);
    }

    fn target_grid(&mut self, ui: &mut egui::Ui) {
        self.target_group(ui, "Linux / Windows", &[0, 1]);
        ui.add_space(8.0);
        self.target_group(ui, "macOS architectures", &[2, 3]);
    }

    fn target_group(&mut self, ui: &mut egui::Ui, heading: &str, targets: &[usize]) {
        widgets::section_heading(ui, heading);
        let available_width = ui.available_width();
        let two_columns = available_width >= 420.0;
        let gap = 10.0;
        let width = if two_columns {
            (available_width - gap) / 2.0
        } else {
            available_width
        };
        let mut clicked = None;
        let mut hovered = None;
        let mut add_target = |ui: &mut egui::Ui, index: usize| {
            let target = &builder::TARGETS[index];
            let response = widgets::target_card(
                ui,
                target.name,
                target.detail,
                self.core.target_selected(index),
                self.hovered_target == Some(index),
                width,
                &self.target_icons[index],
            );
            if response.clicked() {
                clicked = Some(index);
            }
            if response.hovered() {
                hovered = Some(index);
            }
        };
        ui.spacing_mut().item_spacing = Vec2::new(gap, 10.0);
        if two_columns {
            ui.horizontal(|ui| {
                for index in targets {
                    add_target(ui, *index);
                }
            });
        } else {
            for index in targets {
                add_target(ui, *index);
            }
        }
        if let Some(index) = clicked {
            self.core.toggle_target(index);
        }
        self.hovered_target = hovered;
    }

    fn injector_view(&mut self, ctx: &Context) {
        egui::TopBottomPanel::top("main_toolbar")
            .frame(theme::toolbar_frame())
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let busy = self.core.injection.is_busy() || self.core.building;
                    if widgets::tool_button(ui, "Scan", !busy).clicked() {
                        self.core.scan();
                    }
                    if widgets::tool_button(ui, "Diagnostics", !busy)
                        .on_hover_text(
                            "Prints what this machine knows about the pending attach: the JDK \
                                        used to attach, both artefacts, the mod's entrypoints and mixin \
                                        configurations, the selected process and the report file. It never \
                                        touches the game.",
                        )
                        .clicked()
                    {
                        self.core.run_diagnostics();
                    }
                    let ready = self.core.injector_artifact().is_some();
                    ui.label(
                        RichText::new(if ready {
                            "artefact ready"
                        } else {
                            "run Build libraries first"
                        })
                        .size(11.0)
                        .color(if ready { Palette::OK } else { Palette::WARN }),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} found", self.core.processes.len()))
                                .size(11.5)
                                .color(Palette::TEXT_DIM),
                        );
                    });
                });
            });

        egui::TopBottomPanel::bottom("footer")
            .frame(theme::footer_frame())
            .show(ctx, |ui| {
                widgets::status_banner(
                    ui,
                    self.core.injection,
                    self.core.processes.len(),
                    self.core.selected_pid,
                );
                ui.add_space(6.0);
                self.route_picker(ui);
                ui.add_space(6.0);
                let can_inject_startup =
                    !self.core.injection.is_busy() && !self.core.building && self.core.jar.is_some();
                let can_inject = !self.core.injection.is_busy()
                    && !self.core.building
                    && self.core.selected_pid.is_some()
                    && self.core.jar.is_some()
                    && self.core.injector_artifact().is_some();
                let label = match self.core.injection {
                    Injection::Injecting(_) | Injection::Reporting(_) | Injection::Scanning => "Working...",
                    _ => "Inject now (late)",
                };
                let mut startup_clicked = false;
                let mut late_clicked = false;
                ui.horizontal(|ui| {
                    let width = ((ui.available_width() - ui.spacing().item_spacing.x) / 2.0).max(0.0);
                    startup_clicked = ui
                        .add_enabled(
                            can_inject_startup,
                            egui::Button::new(RichText::new("Inject at startup").size(14.0).strong().color(
                                if can_inject_startup {
                                    Palette::TEXT
                                } else {
                                    Palette::TEXT_DIM
                                },
                            ))
                            .fill(if can_inject_startup {
                                Palette::BUTTON
                            } else {
                                Palette::PANEL
                            })
                            .stroke(egui::Stroke::new(
                                1.0_f32,
                                if can_inject_startup {
                                    Palette::ACCENT
                                } else {
                                    Palette::BORDER
                                },
                            ))
                            .rounding(egui::Rounding::same(11.0))
                            .min_size(Vec2::new(width, 40.0)),
                        )
                        .on_hover_text(
                            "Searches for game instances, then configures the selected instance to load \
                             the mod before Fabric/Forge/NeoForge discovery on its next launch.",
                        )
                        .clicked();
                    let late_response = ui.add_enabled(
                        can_inject,
                        egui::Button::new(RichText::new(label).size(14.0).strong().color(if can_inject {
                            Palette::TEXT
                        } else {
                            Palette::TEXT_DIM
                        }))
                        .fill(if !can_inject {
                            Palette::BUTTON
                        } else if self.action_hovered {
                            Palette::ACCENT_HOVER
                        } else {
                            Palette::ACCENT
                        })
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            if can_inject {
                                Palette::ACCENT_SOFT
                            } else {
                                Palette::BORDER
                            },
                        ))
                        .rounding(egui::Rounding::same(11.0))
                        .min_size(Vec2::new(width, 40.0)),
                    );
                    self.action_hovered = late_response.hovered();
                    late_clicked = late_response.clicked();
                });
                if startup_clicked {
                    self.startup_instances = state::discover_startup_instances();
                    self.startup_picker_open = true;
                }
                if late_clicked {
                    self.core.request_injection();
                }
            });

        egui::CentralPanel::default()
            .frame(EguiFrame::none().fill(Palette::BACKGROUND))
            .show(ctx, |ui| {
                let time = ctx.input(|input| input.time);
                self.panorama.advance(time, self.panoramas.len());
                paint_panorama(ui, &self.panoramas, &self.panorama, time);
                scrolling_content(ui, "injector-content", |ui| self.process_list(ui));
            });
    }

    fn route_picker(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(Layout::top_down(Align::Center), |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Attach route:").size(11.5).color(Palette::TEXT_DIM));
                let mut chosen = None;
                for route in Route::ALL {
                    let active = self.core.route == route;
                    let response = ui
                        .add(
                            egui::Button::new(RichText::new(route.label()).size(11.5).color(if active {
                                Palette::TEXT
                            } else {
                                Palette::TEXT_DIM
                            }))
                            .fill(if active {
                                Palette::CARD_SELECTED
                            } else {
                                Palette::BUTTON
                            })
                            .stroke(egui::Stroke::new(
                                1.0_f32,
                                if active { Palette::ACCENT } else { Palette::BORDER },
                            ))
                            .rounding(egui::Rounding::same(8.0)),
                        )
                        .on_hover_text(route_hint(route));
                    if response.clicked() {
                        chosen = Some(route);
                    }
                }
                if let Some(route) = chosen {
                    self.core.route = route;
                }
            });
        });
    }

    fn process_list(&mut self, ui: &mut egui::Ui) {
        widgets::section_heading(ui, "RUNNING JAVA PROCESSES · MINECRAFT FIRST");
        if self.core.processes.is_empty() {
            widgets::empty_state(
                ui,
                match self.core.injection {
                    Injection::Scanning => "Scanning...",
                    _ => "No Java process visible. Check that Minecraft and JAR to Dynamic Library share the same user/session; a sandbox may hide host processes.",
                },
            );
            return;
        }
        let selected = self.core.selected_pid;
        let mut clicked = None;
        let mut hovered = None;
        for item in &self.core.processes {
            let response = widgets::process_card(
                ui,
                item.pid,
                &item.short_description(),
                if item.likely_minecraft {
                    "Minecraft-like"
                } else {
                    "other JVM"
                },
                item.likely_minecraft,
                selected == Some(item.pid),
                self.hovered_process == Some(item.pid),
            );
            if response.clicked() {
                clicked = Some(item.pid);
            }
            if response.hovered() {
                hovered = Some(item.pid);
            }
            ui.add_space(6.0);
        }
        self.hovered_process = hovered;
        if let Some(pid) = clicked {
            self.core.select_process(pid);
        }
        ui.add_space(10.0);
        widgets::log_panel(ui, &self.core.log);
    }
}

fn update_modal(ctx: &Context, available_update: &mut Option<UpdateInfo>) {
    let Some(update) = available_update.as_ref() else {
        return;
    };

    let version = update.version.clone();
    let release_url = update.release_url.clone();
    let screen = ctx.screen_rect();
    let mut update_now = false;
    let mut later = false;

    egui::Area::new(egui::Id::new("jtdl-update-backdrop"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (_, response) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(screen, 0.0, Color32::from_black_alpha(160));
            later = response.clicked();
        });

    egui::Area::new(egui::Id::new("jtdl-update-dialog"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            EguiFrame::none()
                .fill(Palette::PANEL)
                .stroke(egui::Stroke::new(1.0_f32, Palette::ACCENT))
                .rounding(egui::Rounding::same(12.0))
                .inner_margin(Margin::same(16.0))
                .show(ui, |ui| {
                    ui.set_width((screen.width() - 48.0).clamp(260.0, 360.0));
                    ui.label(
                        RichText::new("Update available")
                            .size(16.0)
                            .strong()
                            .color(Palette::TEXT),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(format!("A newer version ({version}) is available on GitHub."))
                            .size(12.0)
                            .color(Palette::TEXT_DIM),
                    );
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if widgets::tool_button(ui, "Later", true).clicked() {
                            later = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Update now")
                                        .size(13.0)
                                        .strong()
                                        .color(Palette::TEXT),
                                )
                                .fill(Palette::ACCENT)
                                .stroke(egui::Stroke::new(1.0_f32, Palette::ACCENT_SOFT))
                                .rounding(egui::Rounding::same(9.0)),
                            )
                            .clicked()
                        {
                            update_now = true;
                        }
                    });
                });
        });

    if update_now {
        ctx.open_url(egui::OpenUrl::same_tab(release_url));
        *available_update = None;
    } else if later {
        *available_update = None;
    }
}

fn route_hint(route: Route) -> &'static str {
    match route {
        Route::JavaAgent => "VirtualMachine#loadAgent: the game JVM calls agentmain(String, Instrumentation), which allows a supported dynamic classpath entry and class re-transformation.",
        Route::NativeLibrary => {
            "VirtualMachine#loadAgentPath: the game JVM calls the library's exported Agent_OnAttach entrypoint."
        }
    }
}

fn scrolling_content(ui: &mut egui::Ui, id: &'static str, add: impl FnOnce(&mut egui::Ui)) {
    ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.with_layout(Layout::top_down(Align::Center), |ui| {
                ui.set_width(ui.available_width().min(760.0));
                EguiFrame::none()
                    .inner_margin(Margin::symmetric(12.0, 10.0))
                    .show(ui, add);
            });
        });
}

fn confirm_modal(ctx: &Context, core: &mut AppCore) {
    let Some(pid) = core.pending_confirm else {
        return;
    };
    let jar = core.jar_label();
    let artifact = core
        .injector_artifact()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "not built yet".to_string());
    let description = core
        .selected_process()
        .map(|item| item.short_description())
        .unwrap_or_default();
    let route = core.route.label();
    let mut approve = false;
    let mut reject = false;

    let modal_id = egui::Id::new("jtdl-confirm-modal");
    let screen = ctx.screen_rect();

    egui::Area::new(modal_id.with("backdrop"))
        .order(egui::Order::Middle)
        .default_pos(screen.min)
        .show(ctx, |ui| {
            let (_, response) = ui.allocate_at_least(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(screen, egui::Rounding::same(0.0), Color32::from_black_alpha(160));
            if response.clicked() {
                reject = true;
            }
        });

    egui::Area::new(modal_id)
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            EguiFrame::none()
                .fill(Palette::PANEL)
                .stroke(egui::Stroke::new(1.0_f32, Palette::ACCENT))
                .rounding(egui::Rounding::same(12.0))
                .inner_margin(Margin::same(16.0))
                .show(ui, |ui| {
                    ui.set_width((screen.width() - 48.0).clamp(240.0, 360.0));
                    ui.label(
                        RichText::new(format!("Late inject {jar} into PID {pid}?"))
                            .size(14.0)
                            .strong()
                            .color(Palette::TEXT),
                    );
                    ui.add_space(4.0);
                    for (key, value) in [
                        ("Process", description),
                        ("Attach route", route.to_string()),
                        ("Artefact", artifact),
                    ] {
                        ui.label(
                            RichText::new(format!("{key}:"))
                                .size(11.5)
                                .color(Palette::TEXT_DIM),
                        );
                        ui.label(RichText::new(value).size(11.5).color(Palette::TEXT));
                    }
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(
                            "Late injection happens after Fabric/Forge/NeoForge have discovered mods \
                             and completed startup. JAR to Dynamic Library cannot replay dependency checks, preLaunch, \
                             registries, access wideners, or loader lifecycle events. Mods that require \
                             those steps may fail or behave partially. Mixins may not apply to classes \
                             already loaded; native attach cannot retransform them. The game may crash.",
                        )
                        .size(11.0)
                        .color(Palette::WARN),
                    );
                    ui.add_space(10.0);
                    ui.horizontal_centered(|ui| {
                        if widgets::tool_button(ui, "Cancel", true).clicked() {
                            reject = true;
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Inject").size(13.0).strong().color(Palette::TEXT),
                                )
                                .fill(Palette::ACCENT)
                                .stroke(egui::Stroke::new(1.0_f32, Palette::ACCENT_SOFT))
                                .rounding(egui::Rounding::same(9.0)),
                            )
                            .clicked()
                        {
                            approve = true;
                        }
                    });
                });
        });

    if reject {
        core.cancel_injection();
    }
    if approve {
        core.confirm_injection();
    }
}

fn startup_instance_modal(ctx: &Context, core: &mut AppCore, instances: &[StartupInstance], open: &mut bool) {
    if !*open {
        return;
    }

    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new("jtdl-startup-instance-backdrop"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (_, response) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(screen, egui::Rounding::same(0.0), Color32::from_black_alpha(160));
            if response.clicked() {
                *open = false;
            }
        });

    let mut selected = None;
    let mut browse = false;
    let mut dismiss = false;
    egui::Window::new("Choose a game instance")
        .id(egui::Id::new("jtdl-startup-instance-picker"))
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .default_size(Vec2::new(420.0, 400.0))
        .show(ctx, |ui| {
            ui.set_width((screen.width() - 48.0).clamp(280.0, 420.0));
            ui.label(
                RichText::new(
                    "Detected Minecraft game folders and launcher instances. Select one to configure startup loading.",
                )
                .size(12.0)
                .color(Palette::TEXT_DIM),
            );
            ui.add_space(8.0);
            if instances.is_empty() {
                widgets::empty_state(
                    ui,
                    "No known instances found. Choose a game folder manually; this works with official, TLauncher, SKlauncher, Pine and other launchers.",
                );
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("startup-instance-list")
                    .max_height(280.0)
                    .show(ui, |ui| {
                        for (index, instance) in instances.iter().enumerate() {
                            let response = ui
                                .add_sized(
                                    [ui.available_width(), 48.0],
                                    egui::Button::new(
                                        egui::RichText::new(format!(
                                            "{}  ·  {}",
                                            instance.name, instance.launcher
                                        ))
                                        .size(12.0),
                                    )
                                    .fill(Palette::CARD)
                                    .stroke(egui::Stroke::new(1.0_f32, Palette::BORDER)),
                                )
                                .on_hover_text(instance.path.display().to_string());
                            if response.clicked() {
                                selected = Some(index);
                            }
                        }
                    });
            }
            ui.add_space(10.0);
            ui.horizontal_centered(|ui| {
                if widgets::tool_button(ui, "Browse folder...", true).clicked() {
                    browse = true;
                }
                if widgets::tool_button(ui, "Cancel", true).clicked() {
                    dismiss = true;
                }
            });
        });

    if let Some(index) = selected {
        core.configure_startup_injection(&instances[index].path);
        *open = false;
    } else if browse {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Select the Minecraft game folder")
            .pick_folder()
        {
            core.configure_startup_injection(&path);
            *open = false;
        }
    } else if dismiss {
        *open = false;
    }
}

fn load_embedded_texture(ctx: &Context, name: &str, bytes: &[u8]) -> TextureHandle {
    let image = image::load_from_memory(bytes)
        .map(|decoded| decoded.to_rgba8())
        .unwrap_or_else(|_| image::RgbaImage::from_pixel(1, 1, image::Rgba([124, 92, 255, 255])));
    let size = [image.width() as usize, image.height() as usize];
    ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(size, &image.into_raw()),
        TextureOptions::LINEAR,
    )
}

fn load_animated_image(ctx: &Context, name: &str, bytes: &[u8]) -> AnimatedImage {
    use image::AnimationDecoder;

    let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
        .expect("the embedded easter egg must be a valid GIF");
    let frames = decoder
        .into_frames()
        .collect_frames()
        .expect("the embedded easter egg frames must decode");
    let frames: Vec<_> = frames
        .into_iter()
        .enumerate()
        .map(|(index, frame)| {
            let delay = frame.delay().numer_denom_ms();
            let seconds = delay.0 as f64 / delay.1.max(1) as f64 / 1000.0;
            let buffer = frame.into_buffer();
            let size = [buffer.width() as usize, buffer.height() as usize];
            let texture = ctx.load_texture(
                format!("{name}-{index}"),
                egui::ColorImage::from_rgba_unmultiplied(size, &buffer.into_raw()),
                TextureOptions::LINEAR,
            );
            (texture, seconds.max(0.02))
        })
        .collect();
    let duration = frames.iter().map(|(_, seconds)| seconds).sum();
    AnimatedImage { frames, duration }
}

fn show_animated_image(ctx: &Context, animated: &AnimatedImage, started_at: f64, time: f64) -> bool {
    if animated.frames.is_empty() || animated.duration <= 0.0 {
        return false;
    }

    let elapsed = (time - started_at).max(0.0).rem_euclid(animated.duration);
    let mut frame_time = elapsed;
    let (frame, _) = animated
        .frames
        .iter()
        .find(|(_, seconds)| {
            if frame_time < *seconds {
                true
            } else {
                frame_time -= seconds;
                false
            }
        })
        .unwrap_or(&animated.frames[0]);
    let screen = ctx.screen_rect();
    let mut dismiss = ctx.input(|input| input.key_pressed(egui::Key::Escape));

    egui::Area::new(egui::Id::new("jtdl-easter-egg-backdrop"))
        .order(egui::Order::Middle)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (_, response) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter()
                .rect_filled(screen, 0.0, Color32::from_black_alpha(190));
            dismiss |= response.clicked();
        });

    egui::Area::new(egui::Id::new("jtdl-easter-egg"))
        .order(egui::Order::Foreground)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            EguiFrame::none()
                .fill(Palette::PANEL)
                .stroke(egui::Stroke::new(1.0_f32, Palette::BORDER))
                .rounding(egui::Rounding::same(14.0))
                .inner_margin(Margin::same(10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_space(12.0);
                        if ui.small_button("×").clicked() {
                            dismiss = true;
                        }
                    });
                    ui.image((frame.id(), frame.size_vec2()));
                });
        });

    dismiss
}

fn load_panorama_frames(ctx: &Context) -> Vec<TextureHandle> {
    PANORAMA_IMAGES
        .iter()
        .copied()
        .enumerate()
        .map(|(index, image)| {
            let name = format!("jtdl-panorama-{index}");
            let custom = util::read_optional_asset(&format!("panorama/{index}.png"));
            load_embedded_texture(ctx, &name, custom.as_deref().unwrap_or(image))
        })
        .collect()
}

fn paint_panorama(ui: &mut egui::Ui, frames: &[TextureHandle], state: &PanoramaState, time: f64) {
    let rect = ui.max_rect();
    if frames.is_empty() || rect.area() <= 0.0 {
        return;
    }

    let mut uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    let image_size = frames[state.current].size_vec2();
    let image_aspect = image_size.x / image_size.y;
    let viewport_aspect = rect.width() / rect.height();
    if viewport_aspect > image_aspect {
        let visible_height = image_aspect / viewport_aspect;
        uv.min.y = (1.0 - visible_height) * 0.5;
        uv.max.y = uv.min.y + visible_height;
    } else {
        let visible_width = viewport_aspect / image_aspect;
        uv.min.x = (1.0 - visible_width) * 0.5;
        uv.max.x = uv.min.x + visible_width;
    }

    let elapsed = state.elapsed(time);
    let progress = (elapsed / PANORAMA_FRAME_SECONDS).clamp(0.0, 1.0) as f32;
    let zoom = 1.02 + progress * 0.012;
    let center = uv.center();
    let half_size = uv.size() / (2.0 * zoom);
    let pan = (progress - 0.5) * uv.width() * 0.035;
    uv = Rect::from_center_size(pos2(center.x + pan, center.y), half_size * 2.0);
    uv.min.x = uv.min.x.clamp(0.0, 1.0 - uv.width());
    uv.min.y = uv.min.y.clamp(0.0, 1.0 - uv.height());
    uv.max = uv.min + uv.size();

    let frame_count = frames.len().max(1);
    let next = (state.current + 1) % frame_count;
    let fade_start = PANORAMA_FRAME_SECONDS - PANORAMA_FADE_SECONDS;
    let t = ((elapsed - fade_start) / PANORAMA_FADE_SECONDS).clamp(0.0, 1.0);
    let smooth = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let next_alpha = (smooth * 255.0).round() as u8;

    let painter = ui.painter();
    painter.image(frames[state.current].id(), rect, uv, Color32::WHITE);
    if next_alpha > 0 {
        painter.image(frames[next].id(), rect, uv, Color32::from_white_alpha(next_alpha));
    }

    let alpha = (PANORAMA_OVERLAY.clamp(0.0, 1.0) * 255.0).round() as u8;
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(alpha));
}

#[cfg(test)]
mod tests {
    use super::{Cursor, PanoramaState, EASTER_EGG_GIF};

    #[test]
    fn panorama_advances_automatically_and_wraps() {
        let mut state = PanoramaState::default();
        state.advance(12.0, 10);
        assert_eq!(state.current, 1);
        state.advance(120.0, 10);
        assert_eq!(state.current, 0);
    }

    #[test]
    fn panorama_advances_after_each_slide_interval() {
        let mut state = PanoramaState::default();
        state.advance(11.0, 9);
        assert_eq!(state.current, 0);
        assert_eq!(state.elapsed(11.0), 11.0);
        state.advance(12.0, 9);
        assert_eq!(state.current, 1);
        assert_eq!(state.elapsed(12.0), 0.0);
    }

    #[test]
    fn embedded_easter_egg_is_an_animated_gif() {
        use image::AnimationDecoder;

        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(EASTER_EGG_GIF))
            .expect("embedded GIF should decode");
        let frames = decoder
            .into_frames()
            .collect_frames()
            .expect("embedded GIF frames should decode");
        assert!(frames.len() > 1);
        assert!(frames.iter().all(|frame| frame.delay().numer_denom_ms().0 > 0));
    }
}
