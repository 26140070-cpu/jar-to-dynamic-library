use egui::{Color32, Frame, Margin, Rounding, Stroke, TextStyle, Visuals};

pub const UI_FONT: &[u8] = include_bytes!("../../assets/fonts/fingerpaint.ttf");
pub const UI_FONT_NAME: &str = "fingerpaint";

#[derive(Clone, Copy, Debug)]
pub struct Palette;

impl Palette {
    pub const BACKGROUND: Color32 = Color32::from_rgb(9, 13, 24);
    pub const HEADER: Color32 = Color32::from_rgb(14, 18, 31);
    pub const PANEL: Color32 = Color32::from_rgb(18, 24, 39);
    pub const CARD: Color32 = Color32::from_rgb(22, 29, 47);
    pub const CARD_HOVERED: Color32 = Color32::from_rgb(32, 42, 65);
    pub const CARD_SELECTED: Color32 = Color32::from_rgb(45, 38, 73);
    pub const BORDER: Color32 = Color32::from_rgb(49, 55, 78);
    pub const ACCENT: Color32 = Color32::from_rgb(165, 132, 255);
    pub const ACCENT_SOFT: Color32 = Color32::from_rgb(202, 182, 255);
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(183, 149, 255);
    pub const BUTTON: Color32 = Color32::from_rgb(31, 38, 57);
    pub const TEXT: Color32 = Color32::from_rgb(242, 243, 250);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(174, 180, 199);
    pub const OK: Color32 = Color32::from_rgb(91, 218, 180);
    pub const WARN: Color32 = Color32::from_rgb(255, 195, 112);
    pub const ERROR: Color32 = Color32::from_rgb(255, 127, 144);

    pub fn card() -> Color32 {
        Self::CARD
    }
}

pub fn apply(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts
        .font_data
        .insert(UI_FONT_NAME.to_owned(), egui::FontData::from_static(UI_FONT));
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        if let Some(list) = fonts.families.get_mut(&family) {
            list.insert(0, UI_FONT_NAME.to_owned());
        }
    }
    ctx.set_fonts(fonts);

    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(Palette::TEXT);
    visuals.panel_fill = Palette::BACKGROUND;
    visuals.window_fill = Palette::PANEL;
    visuals.extreme_bg_color = Palette::BACKGROUND;
    visuals.faint_bg_color = Palette::CARD;
    visuals.hyperlink_color = Palette::ACCENT_SOFT;
    visuals.widgets.noninteractive.bg_fill = Palette::PANEL;
    visuals.widgets.noninteractive.weak_bg_fill = Palette::CARD;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, Palette::BORDER);
    visuals.widgets.inactive.bg_fill = Palette::BUTTON;
    visuals.widgets.inactive.weak_bg_fill = Palette::BUTTON;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, Palette::TEXT);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Palette::BORDER);
    visuals.widgets.hovered.bg_fill = Palette::CARD_HOVERED;
    visuals.widgets.hovered.weak_bg_fill = Palette::CARD_HOVERED;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Palette::ACCENT);
    visuals.widgets.active.bg_fill = Palette::CARD_SELECTED;
    visuals.widgets.active.weak_bg_fill = Palette::CARD_SELECTED;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, Palette::ACCENT);
    visuals.widgets.noninteractive.rounding = Rounding::same(10.0);
    visuals.widgets.inactive.rounding = Rounding::same(10.0);
    visuals.widgets.hovered.rounding = Rounding::same(10.0);
    visuals.widgets.active.rounding = Rounding::same(10.0);
    visuals.selection.bg_fill = Palette::CARD_SELECTED;
    visuals.selection.stroke = Stroke::new(1.0_f32, Palette::ACCENT_SOFT);
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.window_margin = Margin::same(14.0);
    style
        .text_styles
        .insert(TextStyle::Heading, egui::FontId::proportional(19.0));
    style
        .text_styles
        .insert(TextStyle::Body, egui::FontId::proportional(13.0));
    style
        .text_styles
        .insert(TextStyle::Small, egui::FontId::proportional(11.5));
    ctx.set_style(style);
}

fn frame(fill: Color32) -> Frame {
    Frame::none()
        .fill(fill)
        .inner_margin(Margin::symmetric(14.0, 10.0))
        .stroke(Stroke::new(1.0_f32, Palette::BORDER))
        .rounding(Rounding::same(0.0))
}

pub fn header_frame() -> Frame {
    let mut frame = frame(Palette::HEADER);
    frame.inner_margin = Margin::symmetric(16.0, 12.0);
    frame
}

pub fn toolbar_frame() -> Frame {
    frame(Palette::PANEL)
}

pub fn footer_frame() -> Frame {
    let mut frame = frame(Palette::HEADER);
    frame.inner_margin = Margin::symmetric(14.0, 12.0);
    frame
}
