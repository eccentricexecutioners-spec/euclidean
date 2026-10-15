//! Design tokens (Illustrator's four UI brightness levels), fonts and egui style.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Brightness {
    Dark,
    #[default]
    MediumDark,
    MediumLight,
    Light,
}

impl Brightness {
    pub const ALL: [Brightness; 4] = [Brightness::Dark, Brightness::MediumDark, Brightness::MediumLight, Brightness::Light];
    pub fn label(self) -> &'static str {
        match self {
            Brightness::Dark => "Dark",
            Brightness::MediumDark => "Medium Dark",
            Brightness::MediumLight => "Medium Light",
            Brightness::Light => "Light",
        }
    }
    pub fn id(self) -> &'static str {
        match self {
            Brightness::Dark => "dark",
            Brightness::MediumDark => "mediumDark",
            Brightness::MediumLight => "mediumLight",
            Brightness::Light => "light",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.id().eq_ignore_ascii_case(s) || b.label().eq_ignore_ascii_case(s))
    }
}

/// All colours and metrics the UI uses. Never hard-code a colour in a widget.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    /// Fusion workbench: the frame around the cards, the surface of rows, the card fill.
    pub frame: Color32,
    pub surface: Color32,
    pub card: Color32,
    /// Text and icons on a filled accent or tab colour.
    pub on_accent: Color32,
    /// Fusion tab colours (see [`Tokens::tab_color`]).
    pub tab_draw: Color32,
    pub tab_sketch: Color32,
    pub tab_type: Color32,
    pub tab_output: Color32,
    pub app_bar: Color32,
    pub panel: Color32,
    pub panel_darker: Color32,
    pub input: Color32,
    pub input_border: Color32,
    pub border: Color32,
    pub divider: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub icon: Color32,
    pub hover: Color32,
    pub tool_active: Color32,
    pub accent: Color32,
    pub accent_strong: Color32,
    pub row_selected: Color32,
    pub pasteboard: Color32,
    pub ruler: Color32,
    pub ruler_tick: Color32,
    pub smart_guide: Color32,
    pub guide: Color32,
    pub measure_bg: Color32,
    pub button: Color32,
    pub radius: u8,
    /// Field values, button labels, link labels.
    pub text_strong: Color32,
    /// Panel tab strips, doc-tab bar, dock header.
    pub tab_strip: Color32,
    /// Outline of secondary (outlined) buttons.
    pub button_border: Color32,
    /// Default Layer 1 selection colour and selected-anchor fill.
    pub selection: Color32,
    pub anchor_selected: Color32,
    /// Active item text in tool flyouts.
    pub flyout_active: Color32,
    /// Hint-bar / tab-bar heights etc. scale with this base font size.
    pub font_size: f32,
    /// Window Close button (custom title bar) when hovered, and its glyph there.
    pub caption_close: Color32,
    pub caption_close_text: Color32,
    /// What preview images mark (Flattener Preview highlights).
    pub highlight: Color32,
    /// The bleed outline around artboards (Document Setup → Bleed).
    pub bleed: Color32,
    /// Error marks (a missing linked file).
    pub error: Color32,
    /// Warning marks (a modified linked file).
    pub warning: Color32,
    /// Rotation gizmo axes, also labelled X, Y and Z.
    pub axis_x: Color32,
    pub axis_y: Color32,
    pub axis_z: Color32,
}

impl Tokens {
    pub fn for_brightness(b: Brightness) -> Self {
        let hex = |s: u32| Color32::from_rgb((s >> 16) as u8, (s >> 8) as u8, s as u8);
        let base = Tokens {
            dark: true,
            frame: hex(0x1a1d23),
            surface: hex(0x22262e),
            card: hex(0x2a2f38),
            on_accent: hex(0xffffff),
            tab_draw: hex(0x5b8cff),
            tab_sketch: hex(0xff9447),
            tab_type: hex(0xb07dff),
            tab_output: hex(0x2fc796),
            app_bar: hex(0x1a1d23),
            panel: hex(0x2a2f38),
            panel_darker: hex(0x22262e),
            input: hex(0x1d2128),
            input_border: hex(0x444b57),
            border: hex(0x363c47),
            divider: hex(0x363c47),
            text: hex(0xe7eaef),
            text_dim: hex(0x98a1b0),
            text_disabled: hex(0x5f6775),
            icon: hex(0xd5dae2),
            hover: hex(0x353b46),
            tool_active: hex(0x1d2128),
            accent: hex(0x378ef0),
            accent_strong: hex(0x1473e6),
            row_selected: hex(0x354364),
            pasteboard: hex(0x121418),
            ruler: hex(0x22262e),
            ruler_tick: hex(0x8a919e),
            smart_guide: hex(0xff3dfc),
            guide: hex(0x4affff),
            measure_bg: Color32::from_rgba_unmultiplied(92, 92, 92, 235),
            button: hex(0x323844),
            radius: 6,
            text_strong: hex(0xffffff),
            tab_strip: hex(0x1a1d23),
            button_border: hex(0x6a7383),
            selection: hex(0x4f80ff),
            anchor_selected: hex(0x3d82ff),
            flyout_active: hex(0x54b2f7),
            font_size: 13.0,
            caption_close: hex(0xc42b1c),
            caption_close_text: Color32::WHITE,
            highlight: hex(0xff3030),
            bleed: hex(0xf03030),
            error: hex(0xe34850),
            warning: hex(0xf0a330),
            axis_x: hex(0xef6058),
            axis_y: hex(0x45af63),
            axis_z: hex(0x378ef0),
        };
        match b {
            Brightness::Dark => base,
            Brightness::MediumDark => Tokens {
                frame: hex(0x2f333b),
                surface: hex(0x393e47),
                card: hex(0x424852),
                app_bar: hex(0x2f333b),
                panel: hex(0x424852),
                panel_darker: hex(0x393e47),
                tab_strip: hex(0x2f333b),
                input: hex(0x343941),
                input_border: hex(0x5d6573),
                border: hex(0x4f5664),
                divider: hex(0x4f5664),
                text: hex(0xe2e5ea),
                text_strong: hex(0xffffff),
                text_dim: hex(0xaab2bf),
                text_disabled: hex(0x747c8a),
                icon: hex(0xcfd4dc),
                hover: hex(0x4c5360),
                tool_active: hex(0x2b2f36),
                row_selected: hex(0x4a5a7e),
                pasteboard: hex(0x3a3f47),
                ruler: hex(0x343840),
                ruler_tick: hex(0x8a919e),
                button: hex(0x4a515c),
                button_border: hex(0x7c8494),
                on_accent: hex(0xffffff),
                tab_draw: hex(0x5b8cff),
                tab_sketch: hex(0xff9447),
                tab_type: hex(0xb07dff),
                tab_output: hex(0x2fc796),
                ..base
            },
            Brightness::MediumLight => Tokens {
                dark: false,
                frame: hex(0xbcc2cb),
                surface: hex(0xccd1d9),
                card: hex(0xd8dce2),
                app_bar: hex(0xbcc2cb),
                panel: hex(0xd8dce2),
                panel_darker: hex(0xccd1d9),
                tab_strip: hex(0xbcc2cb),
                input: hex(0xe6e9ee),
                input_border: hex(0x8f97a3),
                border: hex(0xaab1bc),
                divider: hex(0xaab1bc),
                text: hex(0x1c2029),
                text_strong: hex(0x000000),
                text_dim: hex(0x3f4654),
                text_disabled: hex(0x7c8592),
                icon: hex(0x262b34),
                hover: hex(0xc5cbd4),
                tool_active: hex(0xa9b0bb),
                row_selected: hex(0xb3c5ea),
                pasteboard: hex(0xa9afb8),
                ruler: hex(0xc9ced6),
                ruler_tick: hex(0x4a5260),
                button: hex(0xcdd2da),
                button_border: hex(0x7f8794),
                on_accent: hex(0xffffff),
                tab_draw: hex(0x2f6bff),
                tab_sketch: hex(0xff7a1a),
                tab_type: hex(0x9b4dff),
                tab_output: hex(0x12a37a),
                ..base
            },
            Brightness::Light => Tokens {
                dark: false,
                frame: hex(0xe4e7ec),
                surface: hex(0xf7f8fa),
                card: hex(0xffffff),
                app_bar: hex(0xe4e7ec),
                panel: hex(0xffffff),
                panel_darker: hex(0xf7f8fa),
                tab_strip: hex(0xe4e7ec),
                input: hex(0xf3f5f8),
                input_border: hex(0xc3c9d2),
                border: hex(0xdde1e7),
                divider: hex(0xdde1e7),
                text: hex(0x1c2029),
                text_strong: hex(0x0f1218),
                text_dim: hex(0x6a7383),
                text_disabled: hex(0xa3aab5),
                icon: hex(0x2b313c),
                hover: hex(0xeef1f5),
                tool_active: hex(0xdfe4ec),
                row_selected: hex(0xe2eaff),
                pasteboard: hex(0xd9dde3),
                ruler: hex(0xeef0f3),
                ruler_tick: hex(0x5a6270),
                button: hex(0xeef1f5),
                button_border: hex(0x9aa1ad),
                on_accent: hex(0xffffff),
                tab_draw: hex(0x2f6bff),
                tab_sketch: hex(0xff7a1a),
                tab_type: hex(0x9b4dff),
                tab_output: hex(0x12a37a),
                ..base
            },
        }
    }

    pub fn get(ctx: &egui::Context) -> Tokens {
        ctx.data(|d| d.get_temp::<Tokens>(egui::Id::NULL)).unwrap_or_else(|| Tokens::for_brightness(Brightness::MediumDark))
    }
    /// Names a layout file may give a tab's colour.
    pub const TAB_COLOR_NAMES: [&'static str; 4] = ["draw", "sketch", "type", "output"];

    pub fn is_tab_color(name: &str) -> bool {
        Self::TAB_COLOR_NAMES.contains(&name)
    }

    /// A Fusion tab's colour by name; an unknown name or none gives the accent.
    pub fn tab_color(&self, name: Option<&str>) -> Color32 {
        match name {
            Some("draw") => self.tab_draw,
            Some("sketch") => self.tab_sketch,
            Some("type") => self.tab_type,
            Some("output") => self.tab_output,
            _ => self.accent,
        }
    }

    /// `c` mixed `amount` (0..1) over the card colour.
    pub fn wash(&self, c: Color32, amount: f32) -> Color32 {
        let a = if amount.is_finite() { amount.clamp(0.0, 1.0) } else { 0.0 };
        let mix = |x: u8, y: u8| (f32::from(y) + (f32::from(x) - f32::from(y)) * a).round().clamp(0.0, 255.0) as u8;
        Color32::from_rgb(mix(c.r(), self.card.r()), mix(c.g(), self.card.g()), mix(c.b(), self.card.b()))
    }

    pub fn cr(&self) -> CornerRadius {
        CornerRadius::same(self.radius)
    }
}

/// Selected text in the text fields `ui` draws from here on is in the strong text colour. egui
/// paints it in the selection stroke's colour, the accent, which is hard to read on the selection's
/// accent tint (#1465). For fields that draw their own frame: egui's frame takes its focus ring from
/// the same stroke.
pub fn strong_selected_text(ui: &mut egui::Ui) {
    ui.visuals_mut().selection.stroke.color = Tokens::get(ui.ctx()).text_strong;
}

pub const FONT_UI: &str = "ui";
pub const FONT_UI_SEMIBOLD: &str = "ui-semibold";
pub const FONT_MONO: &str = "mono";

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FONT_UI_SEMIBOLD.into()))
}
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FONT_MONO.into()))
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let add = |f: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        f.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add(&mut fonts, "SourceSans3", include_bytes!("../../../assets/fonts/SourceSans3-Regular.ttf"));
    add(&mut fonts, "SourceSans3-Semibold", include_bytes!("../../../assets/fonts/SourceSans3-Semibold.ttf"));
    add(&mut fonts, "JetBrainsMono", include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf"));
    let fallback: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut prop = vec!["SourceSans3".to_string()];
    prop.extend(fallback.clone());
    fonts.families.insert(FontFamily::Proportional, prop);
    let mut semi = vec!["SourceSans3-Semibold".to_string()];
    semi.extend(fallback.clone());
    fonts.families.insert(FontFamily::Name(FONT_UI_SEMIBOLD.into()), semi);
    let mut ui = vec!["SourceSans3".to_string()];
    ui.extend(fallback.clone());
    fonts.families.insert(FontFamily::Name(FONT_UI.into()), ui);
    let mut mono = vec!["JetBrainsMono".to_string()];
    mono.extend(fallback);
    fonts.families.insert(FontFamily::Name(FONT_MONO.into()), mono);
    add_craft_fonts(&mut fonts);
    ctx.set_fonts(fonts);
}

/// The egui name of a craft-fonts face.
pub(crate) fn craft_font_name(f: &vectorcraft_text::CraftFont) -> String {
    format!("craft-fonts {} {}", f.family, f.style)
}

/// The Japanese craft-fonts faces (when built with `CRAFT_FONTS_DIR`) as fallbacks at the end of
/// every family, after the app's own fonts and before the installed fonts `ui_fonts` adds: BIZ
/// UDPGothic first (bold first in the semibold family), then the Mincho faces.
fn add_craft_fonts(fonts: &mut FontDefinitions) {
    for f in vectorcraft_text::CRAFT_FONTS.iter().filter(|f| f.is_japanese()) {
        fonts.font_data.insert(craft_font_name(f), Arc::new(FontData::from_static(f.bytes)));
    }
    for (family, stack) in fonts.families.iter_mut() {
        let bold = *family == FontFamily::Name(FONT_UI_SEMIBOLD.into());
        stack.extend(vectorcraft_text::craft_fonts::japanese_ui_fonts(bold).into_iter().map(craft_font_name));
    }
}

/// Apply tokens to egui's global style.
pub fn apply(ctx: &egui::Context, b: Brightness) {
    let t = Tokens::for_brightness(b);
    ctx.data_mut(|d| d.insert_temp(egui::Id::NULL, t));
    let mut v = if t.dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = t.panel;
    v.window_fill = t.panel;
    v.extreme_bg_color = t.input;
    v.faint_bg_color = t.panel_darker;
    v.window_stroke = Stroke::new(1.0, t.border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(10);
    v.selection.bg_fill = t.accent.gamma_multiply(0.55);
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.hyperlink_color = t.accent;
    v.override_text_color = Some(t.text);
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 14, spread: 0, color: Color32::from_black_alpha(if t.dark { 120 } else { 60 }) };
    v.window_shadow = egui::epaint::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(if t.dark { 140 } else { 70 }) };
    let w = &mut v.widgets;
    craft_ui::theme::WidgetPalette { noninteractive: t.panel, inactive: t.button, hovered: t.hover, active: t.tool_active, open: t.hover }.apply_to(
        w,
        t.cr(),
        Stroke::new(1.0, t.text),
    );
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.divider);
    w.inactive.bg_stroke = Stroke::NONE;
    w.hovered.bg_stroke = Stroke::new(1.0, t.input_border);
    w.active.bg_stroke = Stroke::new(1.0, t.accent);
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 5.0);
        s.spacing.button_padding = egui::vec2(6.0, 2.0);
        s.spacing.interact_size = egui::vec2(24.0, 24.0);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.slider_width = 120.0;
        s.text_styles = [
            (TextStyle::Small, FontId::proportional(11.0)),
            (TextStyle::Body, FontId::proportional(13.0)),
            (TextStyle::Button, FontId::proportional(13.0)),
            (TextStyle::Heading, FontId::new(15.0, FontFamily::Name(FONT_UI_SEMIBOLD.into()))),
            (TextStyle::Monospace, FontId::new(11.5, FontFamily::Name(FONT_MONO.into()))),
        ]
        .into();
        s.animation_time = 0.08;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_roundtrip() {
        for b in Brightness::ALL {
            assert_eq!(Brightness::parse(b.id()), Some(b));
            assert_eq!(Brightness::parse(b.label()), Some(b));
        }
        assert!(!Tokens::for_brightness(Brightness::Light).dark);
    }

    /// #1662: a slider's rail (`button_border`, see `widgets::slider`) stands out from the panel
    /// in every brightness.
    #[test]
    fn slider_rails_show_on_every_panel() {
        for b in [Brightness::Dark, Brightness::MediumDark, Brightness::MediumLight, Brightness::Light] {
            let t = Tokens::for_brightness(b);
            let gap = (i32::from(t.button_border.r()) - i32::from(t.panel.r())).abs();
            assert!(gap >= 24, "{b:?}: rail {:?} on panel {:?}", t.button_border, t.panel);
        }
    }
}
