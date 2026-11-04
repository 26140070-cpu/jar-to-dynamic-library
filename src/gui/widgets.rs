use std::collections::VecDeque;

use egui::{
    pos2, Align, Align2, Color32, FontId, Frame, Layout, Margin, Rect, Response, RichText, Rounding,
    ScrollArea, Sense, Stroke, Ui, Vec2,
};

use crate::gui::state::{Injection, Level, LogLine, View, APP_TITLE, DISCORD_URL, GITHUB_URL};
use crate::gui::theme::Palette;

pub const BRAND_ICON: &[u8] = include_bytes!("../../assets/images/jar.png");
pub const GITHUB_ICON: &[u8] = include_bytes!("../../assets/images/github.png");
pub const DISCORD_ICON: &[u8] = include_bytes!("../../assets/images/discord.png");

pub struct ChangelogEntry {
    pub version: &'static str,
    pub date: &'static str,
    pub tag: &'static str,
    pub items: &'static [&'static str],
}

pub const CHANGELOG: &[ChangelogEntry] = &[
    ChangelogEntry {
        version: "0.2.1",
        date: "November 3, 2026",
        tag: "LATEST",
        items: &[
            "Added an automatic GitHub release check with Update now and Later actions.",
            "Added a hero banner, social network icons, and an improved rotating landscape carousel.",
            "Updated the application name and refined interface colors, contrast, and controls.",
        ],
    },
    ChangelogEntry {
        version: "0.2.0",
        date: "November 2, 2026",
        tag: "PREVIOUS",
        items: &[
            "Rebuilt the injector around the official JDK Attach API: Java agent JAR or native Agent_OnAttach route.",
            "The target JVM reads the original mod JAR at runtime, including resources, Fabric/Quilt entrypoints, nested JARs, Forge/NeoForge metadata and Mixin configs.",
            "Added process discovery, explicit attach confirmation, diagnostics, structured reports and the three-tab desktop GUI.",
            "Added per-attach session IDs so stale reports cannot be mistaken for a new run; different JARs can be attached to the same JVM without reloading the same archive twice.",
            "Fixed retry behavior after JNI setup failures, cleanup of temporary JNI thread attachments and narrow-window header clipping.",
        ],
    },
    ChangelogEntry {
        version: "0.1.0",
        date: "October 23, 2026",
        tag: "BASELINE",
        items: &[
            "First desktop builder for selecting a Minecraft mod JAR and target operating systems.",
            "Packaged mod classes into native .dll, .so and .dylib libraries with the original JNI injector.",
            "Initial Forge, NeoForge and Fabric support, with the limitations documented for the first release.",
        ],
    },
];

fn tag_color(tag: &str) -> Color32 {
    match tag {
        "NEW" => Palette::OK,
        "FIX" => Palette::WARN,
        _ => Palette::ACCENT_SOFT,
    }
}

fn level_color(level: Level) -> Color32 {
    match level {
        Level::Info => Palette::TEXT,
        Level::Ok => Palette::OK,
        Level::Warn => Palette::WARN,
        Level::Error => Palette::ERROR,
        Level::Detail => Palette::TEXT_DIM,
    }
}

pub fn header(
    ui: &mut Ui,
    logo: &egui::TextureHandle,
    github: &egui::TextureHandle,
    discord: &egui::TextureHandle,
    view: View,
) -> Option<View> {
    let mut next = None;

    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
    ui.horizontal_wrapped(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
        ui.painter().image(
            logo.id(),
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 0.0);
            ui.label(RichText::new(APP_TITLE).size(17.0).strong().color(Palette::TEXT));
        });
        ui.add_space(18.0);
        for candidate in View::ALL {
            if navigation(ui, candidate, view) {
                next = Some(candidate);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if social_button(ui, "Discord", discord) {
                ui.ctx().open_url(egui::OpenUrl::same_tab(DISCORD_URL));
            }
            if social_button(ui, "GitHub", github) {
                ui.ctx().open_url(egui::OpenUrl::same_tab(GITHUB_URL));
            }
        });
    });
    next
}

fn navigation(ui: &mut Ui, candidate: View, current: View) -> bool {
    let active = candidate == current;
    ui.add(
        egui::Button::new(RichText::new(candidate.label()).size(13.5).color(if active {
            Palette::TEXT
        } else {
            Palette::TEXT_DIM
        }))
        .fill(if active {
            Palette::CARD_SELECTED
        } else {
            Color32::TRANSPARENT
        })
        .stroke(Stroke::new(
            if active { 1.0_f32 } else { 0.0_f32 },
            Palette::ACCENT,
        ))
        .rounding(Rounding::same(9.0))
        .min_size(Vec2::new(74.0, 26.0)),
    )
    .clicked()
}

fn social_button(ui: &mut Ui, label: &str, icon: &egui::TextureHandle) -> bool {
    ui.add(
        egui::Button::image(egui::Image::new((icon.id(), Vec2::splat(18.0))))
            .fill(Palette::BUTTON)
            .stroke(Stroke::new(1.0_f32, Palette::BORDER))
            .rounding(Rounding::same(9.0))
            .min_size(Vec2::splat(32.0)),
    )
    .on_hover_text(match label {
        "GitHub" => GITHUB_URL,
        _ => DISCORD_URL,
    })
    .clicked()
}

pub fn tool_button(ui: &mut Ui, text: &str, enabled: bool) -> Response {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(text).size(13.0)))
}

pub fn inject_button(ui: &mut Ui, label: &str, enabled: bool, hovered: bool) -> Response {
    let width = ui.available_width().max(0.0);
    let fill = if !enabled {
        Palette::BUTTON
    } else if hovered {
        Palette::ACCENT_HOVER
    } else {
        Palette::ACCENT
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(label).size(15.0).strong().color(if enabled {
            Palette::TEXT
        } else {
            Palette::TEXT_DIM
        }))
        .fill(fill)
        .stroke(Stroke::new(
            1.0_f32,
            if enabled {
                Palette::ACCENT_SOFT
            } else {
                Palette::BORDER
            },
        ))
        .rounding(Rounding::same(11.0))
        .min_size(Vec2::new(width, 40.0)),
    )
}

pub fn process_card(
    ui: &mut Ui,
    pid: u32,
    title: &str,
    description: &str,
    is_minecraft: bool,
    selected: bool,
    hovered: bool,
) -> Response {
    let fill = if selected {
        Palette::CARD_SELECTED
    } else if hovered {
        Palette::CARD_HOVERED
    } else {
        Palette::CARD
    };
    let stroke = Stroke::new(
        if selected { 1.5_f32 } else { 1.0_f32 },
        if selected {
            Palette::ACCENT
        } else {
            Palette::BORDER
        },
    );
    let content = Frame::none()
        .fill(fill)
        .stroke(stroke)
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::same(12.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 2.0);
                    ui.label(RichText::new(title).size(13.5).strong().color(Palette::TEXT));
                    ui.label(
                        RichText::new(format!("PID {pid}  ·  {description}"))
                            .size(11.0)
                            .color(Palette::TEXT_DIM),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(
                        RichText::new(if is_minecraft { "Minecraft" } else { "JVM" })
                            .size(11.0)
                            .color(if is_minecraft {
                                Palette::OK
                            } else {
                                Palette::TEXT_DIM
                            }),
                    );
                });
            });
        });
    ui.interact(
        content.response.rect,
        ui.id().with(("process-card", pid)),
        Sense::click(),
    )
}

pub fn target_card(
    ui: &mut Ui,
    name: &str,
    detail: &str,
    selected: bool,
    hovered: bool,
    width: f32,
    icon: &egui::TextureHandle,
) -> Response {
    let fill = if selected {
        Palette::CARD_SELECTED
    } else if hovered {
        Palette::CARD_HOVERED
    } else {
        Palette::CARD
    };
    let stroke = Stroke::new(
        if selected { 1.5_f32 } else { 1.0_f32 },
        if selected {
            Palette::ACCENT
        } else {
            Palette::BORDER
        },
    );
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 58.0), Sense::click());
    if ui.is_rect_visible(rect) {
        ui.painter().rect(rect, Rounding::same(12.0), fill, stroke);
        let inner = rect.shrink(10.0);
        let icon_rect = Rect::from_min_size(pos2(inner.left(), inner.center().y - 13.0), Vec2::splat(26.0));
        ui.painter().image(
            icon.id(),
            icon_rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        let text_x = icon_rect.right() + 8.0;
        let text_right = inner.right() - 13.0;
        let text_width = (text_right - text_x).max(0.0);
        let text_clip = Rect::from_min_max(pos2(text_x, inner.top()), pos2(text_right, inner.bottom()));
        let painter = ui.painter().with_clip_rect(text_clip);
        painter.text(
            pos2(text_x, inner.center().y - 10.0),
            Align2::LEFT_CENTER,
            name,
            FontId::proportional(if text_width < 125.0 { 11.5 } else { 13.0 }),
            Palette::TEXT,
        );
        painter.text(
            pos2(text_x, inner.center().y + 9.0),
            Align2::LEFT_CENTER,
            detail,
            FontId::proportional(10.5),
            Palette::TEXT_DIM,
        );

        let mark_center = pos2(inner.right() - 3.0, inner.center().y);
        if selected {
            ui.painter().line_segment(
                [
                    pos2(mark_center.x - 3.0, mark_center.y),
                    pos2(mark_center.x - 1.0, mark_center.y + 2.0),
                ],
                Stroke::new(1.6_f32, Palette::ACCENT_SOFT),
            );
            ui.painter().line_segment(
                [
                    pos2(mark_center.x - 1.0, mark_center.y + 2.0),
                    pos2(mark_center.x + 3.0, mark_center.y - 3.0),
                ],
                Stroke::new(1.6_f32, Palette::ACCENT_SOFT),
            );
        } else {
            ui.painter()
                .circle_stroke(mark_center, 3.0, Stroke::new(1.0_f32, Palette::TEXT_DIM));
        }
    }
    response
}

pub fn section_heading(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(FontId::proportional(12.0))
            .color(Palette::TEXT_DIM),
    );
    ui.add_space(2.0);
}

pub fn empty_state(ui: &mut Ui, text: &str) {
    Frame::none()
        .fill(Palette::CARD)
        .stroke(Stroke::new(1.0_f32, Palette::BORDER))
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::same(18.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(text).size(13.0).color(Palette::TEXT_DIM));
            });
        });
}

pub fn status_banner(ui: &mut Ui, injection: Injection, count: usize, selected: Option<u32>) {
    let (text, color) = match injection {
        Injection::Scanning => ("Scanning for Java processes...".to_string(), Palette::ACCENT_SOFT),
        Injection::Injecting(pid) => (format!("Attaching to PID {pid}..."), Palette::ACCENT_SOFT),
        Injection::Reporting(pid) => (
            format!("Reading the injector report for PID {pid}..."),
            Palette::ACCENT_SOFT,
        ),
        Injection::Done(pid) => (format!("Attached to PID {pid}. Check the log."), Palette::OK),
        Injection::Failed => (
            "The last attach failed. Check the log.".to_string(),
            Palette::ERROR,
        ),
        Injection::Idle if selected.is_none() && count > 0 => {
            ("Select a process to inject into.".to_string(), Palette::TEXT_DIM)
        }
        Injection::Idle if count == 0 => (
            "No Java process visible. Check that Minecraft and JAR to Dynamic Library share the same user/session; a sandbox may hide host processes.".to_string(),
            Palette::TEXT_DIM,
        ),
        Injection::Idle => ("Ready.".to_string(), Palette::TEXT_DIM),
    };
    Frame::none()
        .fill(Palette::CARD)
        .stroke(Stroke::new(1.0_f32, color))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
                ui.painter()
                    .circle(rect.center(), 3.5, color, Stroke::new(0.0_f32, color));
                ui.label(RichText::new(text).size(12.5).color(Palette::TEXT));
            });
        });
}

pub fn log_panel(ui: &mut Ui, log: &VecDeque<LogLine>) {
    Frame::none()
        .fill(Palette::BACKGROUND)
        .stroke(Stroke::new(1.0_f32, Palette::BORDER))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ScrollArea::vertical()
                .id_salt("log-panel")
                .auto_shrink([false, true])
                .stick_to_bottom(true)
                .max_height(190.0)
                .show(ui, |ui| {
                    for line in log.iter() {
                        ui.label(
                            RichText::new(format!("[{:5}] {}", line.level.tag(), line.text))
                                .font(FontId::monospace(11.0))
                                .color(level_color(line.level)),
                        );
                    }
                });
        });
}

pub fn changelog_panel(ui: &mut Ui) {
    ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
    ui.label(
        RichText::new("Version history")
            .size(17.0)
            .strong()
            .color(Palette::TEXT),
    );
    ui.add_space(2.0);
    ui.label(
        RichText::new("Release notes for JAR to Dynamic Library")
            .size(12.5)
            .color(Palette::TEXT_DIM),
    );
    ui.add_space(6.0);
    for entry in CHANGELOG {
        changelog_card(ui, entry);
        ui.add_space(5.0);
    }
}

fn changelog_card(ui: &mut Ui, entry: &ChangelogEntry) {
    let color = tag_color(entry.tag);
    Frame::none()
        .fill(Palette::CARD)
        .stroke(Stroke::new(1.0_f32, Palette::BORDER))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::same(10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("v{}", entry.version))
                        .size(13.0)
                        .strong()
                        .color(Palette::TEXT),
                );
                ui.label(RichText::new(entry.date).size(10.5).color(Palette::TEXT_DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(entry.tag).size(10.0).strong().color(color));
                });
            });
            ui.add_space(3.0);
            for item in entry.items {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("-").size(12.0).color(color));
                    ui.label(RichText::new(*item).size(11.0).color(Palette::TEXT));
                });
            }
        });
}
