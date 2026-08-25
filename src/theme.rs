use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    sync::{
        OnceLock,
        atomic::{AtomicU8, Ordering},
    },
};

use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle, Visuals,
};
use serde::Deserialize;

static THEME_MODE: AtomicU8 = AtomicU8::new(1);
static THEME_ACCENT: AtomicU8 = AtomicU8::new(1);
static TEXT_CONTRAST: AtomicU8 = AtomicU8::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeMode {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ThemeAccent {
    Green,
    Blue,
    Purple,
    Rose,
    Orange,
    SunRed,
    DeepSeaBlue,
    ForestGreen,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextContrast {
    Soft,
    #[default]
    Standard,
    Strong,
}

impl TextContrast {
    pub const ALL: [Self; 3] = [Self::Soft, Self::Standard, Self::Strong];
}

#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum FontWeight {
    #[default]
    Regular,
    Medium,
    Semibold,
    Bold,
}

impl FontWeight {
    pub const ALL: [Self; 4] = [Self::Regular, Self::Medium, Self::Semibold, Self::Bold];

    fn numeric(self) -> u16 {
        match self {
            Self::Regular => 400,
            Self::Medium => 500,
            Self::Semibold => 600,
            Self::Bold => 700,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontSelection {
    pub ui_family: Option<String>,
    pub ui_weight: FontWeight,
    pub code_family: Option<String>,
}

impl Default for FontSelection {
    fn default() -> Self {
        Self {
            ui_family: None,
            ui_weight: FontWeight::Regular,
            code_family: None,
        }
    }
}

#[derive(Clone, Debug)]
struct SystemFontFace {
    weight: FontWeight,
    path: PathBuf,
    index: u32,
}

#[derive(Clone, Debug)]
pub struct SystemFontFamily {
    pub name: String,
    pub monospaced: bool,
    faces: Vec<SystemFontFace>,
}

impl SystemFontFamily {
    pub fn weights(&self) -> Vec<FontWeight> {
        let mut weights = self
            .faces
            .iter()
            .map(|face| face.weight)
            .collect::<Vec<_>>();
        weights.sort_unstable();
        weights.dedup();
        weights
    }
}

pub fn font_family_names(catalog: &[SystemFontFamily], monospaced_only: bool) -> Vec<String> {
    catalog
        .iter()
        .filter(|family| !monospaced_only || family.monospaced)
        .map(|family| family.name.clone())
        .collect()
}

pub fn available_font_weights(
    catalog: &[SystemFontFamily],
    family: Option<&str>,
) -> Vec<FontWeight> {
    resolve_font_family(catalog, family, false)
        .map(SystemFontFamily::weights)
        .unwrap_or_else(|| vec![FontWeight::Regular])
}

pub struct LoadedFontSet(FontDefinitions);

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color32,
    pub chrome_bg: Color32,
    pub panel: Color32,
    pub panel_soft: Color32,
    pub panel_recessed: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_deep: Color32,
    pub accent_soft: Color32,
    pub accent_shadow: Color32,
    pub hover: Color32,
    pub scroll_track: Color32,
    pub inset_highlight: Color32,
    pub inset_shadow: Color32,
    pub info: Color32,
    pub warning: Color32,
    pub menu_disabled: Color32,
    pub diff_added_bg: Color32,
    pub diff_added_text: Color32,
    pub diff_removed_bg: Color32,
    pub diff_removed_text: Color32,
    pub diff_context_text: Color32,
    pub diff_gutter: Color32,
    pub diff_gutter_text: Color32,
    pub diff_gutter_border: Color32,
    pub diff_selected: Color32,
    pub diff_selected_gutter: Color32,
    pub diff_selected_gutter_text: Color32,
    pub diff_indent_guide: Color32,
    pub syntax_comment: Color32,
    pub syntax_string: Color32,
    pub syntax_number: Color32,
    pub syntax_keyword: Color32,
    pub syntax_type: Color32,
    pub syntax_function: Color32,
    pub syntax_constant: Color32,
    pub syntax_variable: Color32,
    pub syntax_tag: Color32,
    pub syntax_attribute: Color32,
    pub syntax_operator: Color32,
    pub syntax_punctuation: Color32,
    pub syntax_invalid: Color32,
}

const EMBEDDED_THEME_JSON: &str = include_str!("../theme.json");

#[derive(Clone, Debug, Default, Deserialize)]
struct ThemePool {
    #[serde(default)]
    hues: HashMap<String, String>,
    #[serde(default)]
    themes: HashMap<String, HashMap<String, String>>,
}

impl ThemePool {
    fn overlay(&mut self, overlay: Self) {
        self.hues.extend(overlay.hues);
        for (theme, colors) in overlay.themes {
            self.themes.entry(theme).or_default().extend(colors);
        }
    }
}

static THEME_POOL: OnceLock<ThemePool> = OnceLock::new();

fn theme_pool() -> &'static ThemePool {
    THEME_POOL.get_or_init(|| load_theme_pool(&theme_pool_paths()))
}

fn load_theme_pool(paths: &[PathBuf]) -> ThemePool {
    let mut pool: ThemePool = serde_json::from_str(EMBEDDED_THEME_JSON).unwrap_or_default();

    for path in paths {
        let Ok(source) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(base) = serde_json::from_str::<ThemePool>(&source) else {
            continue;
        };

        pool.overlay(base);
        if let Ok(source) = fs::read_to_string(local_theme_path(path)) {
            if let Ok(local) = serde_json::from_str::<ThemePool>(&source) {
                pool.overlay(local);
            }
        }
        break;
    }

    pool
}

fn local_theme_path(path: &Path) -> PathBuf {
    path.with_file_name("theme.local.json")
}

fn theme_pool_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("GIT_AGENT_THEME") {
        paths.push(PathBuf::from(path));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            paths.push(directory.join("theme.json"));
        }
    }
    paths.push(PathBuf::from("theme.json"));
    paths
}

fn mode_key(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::Dark => "dark",
        ThemeMode::Light => "light",
    }
}

fn accent_key(accent: ThemeAccent) -> &'static str {
    match accent {
        ThemeAccent::Green => "green",
        ThemeAccent::Blue => "blue",
        ThemeAccent::Purple => "purple",
        ThemeAccent::Rose => "rose",
        ThemeAccent::Orange => "orange",
        ThemeAccent::SunRed => "sun-red",
        ThemeAccent::DeepSeaBlue => "deep-sea-blue",
        ThemeAccent::ForestGreen => "forest-green",
    }
}

fn configured_color(mode: ThemeMode, accent: ThemeAccent, token: &str) -> Option<Color32> {
    let pool = theme_pool();
    let hue = pool.hues.get(accent_key(accent))?;
    let template = pool.themes.get(mode_key(mode))?.get(token)?;
    parse_hsl_color(&template.replace("${c}", hue))
}

fn parse_hsl_color(value: &str) -> Option<Color32> {
    let body = value.trim().strip_prefix("hsl(")?.strip_suffix(')')?;
    let (main, alpha) = body
        .split_once('/')
        .map_or((body, None), |(main, alpha)| (main, Some(alpha)));
    let mut parts = main.split_whitespace();
    let hue = parts
        .next()?
        .trim_end_matches("deg")
        .parse::<f32>()
        .ok()?
        .rem_euclid(360.0)
        / 360.0;
    let saturation = parts
        .next()?
        .strip_suffix('%')?
        .parse::<f32>()
        .ok()?
        .clamp(0.0, 100.0)
        / 100.0;
    let lightness = parts
        .next()?
        .strip_suffix('%')?
        .parse::<f32>()
        .ok()?
        .clamp(0.0, 100.0)
        / 100.0;
    if parts.next().is_some() {
        return None;
    }
    let color = hsl_to_rgb(hue, saturation, lightness);
    let alpha = alpha
        .and_then(|alpha| {
            let alpha = alpha.trim();
            alpha
                .strip_suffix('%')
                .and_then(|percent| percent.parse::<f32>().ok().map(|value| value * 2.55))
                .or_else(|| {
                    alpha
                        .parse::<f32>()
                        .ok()
                        .map(|value| if value <= 1.0 { value * 255.0 } else { value })
                })
        })
        .unwrap_or(255.0)
        .clamp(0.0, 255.0)
        .round() as u8;
    Some(Color32::from_rgba_unmultiplied(
        color.r(),
        color.g(),
        color.b(),
        alpha,
    ))
}

fn apply_theme_pool(palette: &mut Palette, mode: ThemeMode, accent: ThemeAccent) {
    macro_rules! set {
        ($field:ident, $token:literal) => {
            if let Some(color) = configured_color(mode, accent, $token) {
                palette.$field = color;
            }
        };
    }
    set!(bg, "--bg");
    set!(chrome_bg, "--chrome-bg");
    set!(panel, "--panel");
    set!(panel_soft, "--panel-soft");
    set!(panel_recessed, "--panel-recessed");
    set!(text, "--text");
    set!(muted, "--muted");
    set!(accent, "--accent");
    set!(accent_deep, "--accent-deep");
    set!(accent_soft, "--accent-soft");
    set!(accent_shadow, "--accent-shadow");
    set!(hover, "--hover");
    set!(scroll_track, "--scroll-track");
    set!(inset_highlight, "--inset-highlight");
    set!(inset_shadow, "--inset-shadow");
    set!(info, "--info");
    set!(warning, "--warning");
    set!(menu_disabled, "--menu-disabled");
    set!(diff_added_bg, "--diff-added-bg");
    set!(diff_added_text, "--diff-added-text");
    set!(diff_removed_bg, "--diff-removed-bg");
    set!(diff_removed_text, "--diff-removed-text");
    set!(diff_context_text, "--diff-context-text");
    set!(diff_gutter, "--diff-gutter");
    set!(diff_gutter_text, "--diff-gutter-text");
    set!(diff_gutter_border, "--diff-gutter-border");
    set!(diff_selected, "--diff-selected");
    set!(diff_selected_gutter, "--diff-selected-gutter");
    set!(diff_selected_gutter_text, "--diff-selected-gutter-text");
    set!(diff_indent_guide, "--diff-indent-guide");
    set!(syntax_comment, "--syntax-comment");
    set!(syntax_string, "--syntax-string");
    set!(syntax_number, "--syntax-number");
    set!(syntax_keyword, "--syntax-keyword");
    set!(syntax_type, "--syntax-type");
    set!(syntax_function, "--syntax-function");
    set!(syntax_constant, "--syntax-constant");
    set!(syntax_variable, "--syntax-variable");
    set!(syntax_tag, "--syntax-tag");
    set!(syntax_attribute, "--syntax-attribute");
    set!(syntax_operator, "--syntax-operator");
    set!(syntax_punctuation, "--syntax-punctuation");
    set!(syntax_invalid, "--syntax-invalid");
}

pub const BG: Color32 = Color32::from_rgb(16, 18, 24);
pub const PANEL: Color32 = Color32::from_rgb(24, 27, 36);
pub const PANEL_SOFT: Color32 = Color32::from_rgb(31, 35, 46);
pub const TEXT: Color32 = Color32::from_rgb(235, 239, 246);
pub const MUTED: Color32 = Color32::from_rgb(142, 151, 169);
pub const ACCENT: Color32 = Color32::from_rgb(85, 195, 176);
pub const WARNING: Color32 = Color32::from_rgb(242, 171, 90);

pub const LANES: [Color32; 8] = [
    Color32::from_rgb(85, 195, 176),
    Color32::from_rgb(244, 113, 116),
    Color32::from_rgb(120, 164, 255),
    Color32::from_rgb(232, 190, 95),
    Color32::from_rgb(177, 136, 255),
    Color32::from_rgb(104, 210, 121),
    Color32::from_rgb(247, 142, 214),
    Color32::from_rgb(107, 202, 231),
];

pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    apply(ctx, ThemeMode::Light, ThemeAccent::Blue);
}

pub fn apply(ctx: &egui::Context, mode: ThemeMode, accent: ThemeAccent) {
    THEME_MODE.store(
        match mode {
            ThemeMode::Dark => 0,
            ThemeMode::Light => 1,
        },
        Ordering::Relaxed,
    );
    THEME_ACCENT.store(accent_index(accent), Ordering::Relaxed);
    let palette = palette_with_text_contrast(mode, accent, current_text_contrast());
    let mut visuals = match mode {
        ThemeMode::Dark => Visuals::dark(),
        ThemeMode::Light => Visuals::light(),
    };
    visuals.panel_fill = palette.bg;
    visuals.window_fill = palette.panel;
    visuals.window_stroke = Stroke::NONE;
    let surface_shadow = eframe::epaint::Shadow {
        offset: [3, 4],
        blur: 12,
        spread: 0,
        color: palette.accent_shadow.gamma_multiply(0.92),
    };
    visuals.window_shadow = surface_shadow;
    visuals.popup_shadow = surface_shadow;
    visuals.extreme_bg_color = palette.panel_recessed;
    visuals.faint_bg_color = palette.panel_soft;
    visuals.widgets.noninteractive.bg_fill = palette.panel;
    visuals.widgets.noninteractive.bg_stroke = Stroke::NONE;
    visuals.widgets.noninteractive.fg_stroke.color = palette.text;
    visuals.widgets.inactive.bg_fill = palette.panel_soft;
    visuals.widgets.inactive.bg_stroke = Stroke::NONE;
    visuals.widgets.inactive.weak_bg_fill = palette.panel_soft;
    visuals.widgets.inactive.fg_stroke.color = palette.text;
    visuals.widgets.hovered.bg_fill = palette.hover;
    visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    visuals.widgets.hovered.weak_bg_fill = visuals.widgets.hovered.bg_fill;
    visuals.widgets.hovered.fg_stroke.color = palette.text;
    visuals.widgets.active.bg_fill = if mode == ThemeMode::Dark {
        palette.accent_deep
    } else {
        palette.accent_deep
    };
    visuals.widgets.active.bg_stroke = Stroke::NONE;
    visuals.widgets.active.weak_bg_fill = visuals.widgets.active.bg_fill;
    visuals.widgets.active.fg_stroke.color = Color32::WHITE;
    visuals.widgets.open.bg_fill = palette.hover;
    visuals.widgets.open.bg_stroke = Stroke::NONE;
    visuals.widgets.open.weak_bg_fill = palette.hover;
    visuals.widgets.open.fg_stroke.color = palette.text;
    visuals.selection.bg_fill = if mode == ThemeMode::Dark {
        palette.accent_deep
    } else {
        palette.accent_deep
    };
    apply_selected_item_visuals(&mut visuals);
    visuals.hyperlink_color = palette.accent;
    let mut style = (*ctx.style()).clone();
    style.visuals = visuals;
    style.spacing.scroll.foreground_color = false;
    style.spacing.scroll.active_background_opacity = 0.24;
    style.spacing.scroll.interact_background_opacity = 0.36;
    style.spacing.scroll.active_handle_opacity = 0.74;
    style.spacing.scroll.interact_handle_opacity = 1.0;
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 7.0);
    style.interaction.tooltip_delay = 0.12;
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(26.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
    ]
    .into();
    ctx.set_style(style);
}

pub fn apply_if_needed(ctx: &egui::Context, mode: ThemeMode, accent: ThemeAccent) -> bool {
    if current_mode() == mode && current_accent() == accent {
        return false;
    }
    apply(ctx, mode, accent);
    true
}

pub fn current_mode() -> ThemeMode {
    if THEME_MODE.load(Ordering::Relaxed) == 1 {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    }
}

pub fn current_accent() -> ThemeAccent {
    accent_from_index(THEME_ACCENT.load(Ordering::Relaxed))
}

pub fn palette(mode: ThemeMode) -> Palette {
    palette_with_text_contrast(mode, current_accent(), current_text_contrast())
}

pub fn current_text_contrast() -> TextContrast {
    match TEXT_CONTRAST.load(Ordering::Relaxed) {
        0 => TextContrast::Soft,
        2 => TextContrast::Strong,
        _ => TextContrast::Standard,
    }
}

pub fn set_text_contrast(ctx: &egui::Context, contrast: TextContrast) {
    let value = match contrast {
        TextContrast::Soft => 0,
        TextContrast::Standard => 1,
        TextContrast::Strong => 2,
    };
    if TEXT_CONTRAST.swap(value, Ordering::Relaxed) != value {
        apply(ctx, current_mode(), current_accent());
    }
}

fn palette_with_text_contrast(
    mode: ThemeMode,
    accent: ThemeAccent,
    contrast: TextContrast,
) -> Palette {
    let mut palette = palette_for(mode, accent);
    let contrast_target = match (mode, contrast) {
        (_, TextContrast::Standard) => return palette,
        (ThemeMode::Light, TextContrast::Soft) => palette.panel,
        (ThemeMode::Dark, TextContrast::Soft) => palette.panel,
        (ThemeMode::Light, TextContrast::Strong) => Color32::BLACK,
        (ThemeMode::Dark, TextContrast::Strong) => Color32::WHITE,
    };
    let (text_amount, muted_amount) = match contrast {
        TextContrast::Soft => (0.18, 0.12),
        TextContrast::Strong => (0.12, 0.08),
        TextContrast::Standard => unreachable!(),
    };
    palette.text = mix_color(palette.text, contrast_target, text_amount);
    palette.muted = mix_color(palette.muted, contrast_target, muted_amount);
    palette
}

fn mix_color(from: Color32, to: Color32, amount: f32) -> Color32 {
    let amount = amount.clamp(0.0, 1.0);
    let mix = |from: u8, to: u8| {
        (from as f32 + (to as f32 - from as f32) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color32::from_rgba_unmultiplied(
        mix(from.r(), to.r()),
        mix(from.g(), to.g()),
        mix(from.b(), to.b()),
        mix(from.a(), to.a()),
    )
}

pub fn palette_for(mode: ThemeMode, accent: ThemeAccent) -> Palette {
    let seed = accent_seed(accent);
    let hsl = rgb_to_hsl(seed);
    let neutral_s = (hsl.s * 0.10).clamp(0.02, 0.09);
    let muted_s = (hsl.s * 0.18).clamp(0.04, 0.16);
    let neutral = |lightness: f32| hsl_to_rgb(hsl.h, neutral_s, lightness);
    let muted_neutral = |lightness: f32| hsl_to_rgb(hsl.h, muted_s, lightness);
    let recessed_neutral =
        |lightness: f32| hsl_to_rgb(hsl.h, (hsl.s * 0.20).clamp(0.08, 0.18), lightness);
    let accent_color = hsl_to_rgb(hsl.h, hsl.s, hsl.l);
    let accent_deep = match mode {
        ThemeMode::Dark => hsl_to_rgb(hsl.h, (hsl.s * 0.72).clamp(0.0, 1.0), 0.28),
        ThemeMode::Light => hsl_to_rgb(hsl.h, (hsl.s * 0.78).clamp(0.0, 1.0), 0.34),
    };
    let accent_soft = match mode {
        ThemeMode::Dark => hsl_to_rgb(hsl.h, (hsl.s * 0.42).clamp(0.0, 1.0), 0.16),
        ThemeMode::Light => hsl_to_rgb(hsl.h, (hsl.s * 0.30).clamp(0.0, 1.0), 0.91),
    };
    let hover = match mode {
        ThemeMode::Dark => hsl_to_rgb(hsl.h, (hsl.s * 0.36).clamp(0.0, 1.0), 0.22),
        ThemeMode::Light => hsl_to_rgb(hsl.h, (hsl.s * 0.30).clamp(0.0, 1.0), 0.88),
    };
    let scroll_track = match mode {
        ThemeMode::Dark => hsl_to_rgb(hsl.h, (hsl.s * 0.48).clamp(0.0, 1.0), 0.11),
        ThemeMode::Light => hsl_to_rgb(hsl.h, (hsl.s * 0.24).clamp(0.0, 1.0), 0.84),
    };
    let shadow_base = muted_neutral(match mode {
        ThemeMode::Dark => 0.36,
        ThemeMode::Light => 0.28,
    });
    let accent_shadow_base = match mode {
        // Dark surfaces need depth, not a tinted halo. Keep the hue derived
        // from the active theme seed, but make shadows nearly neutral.
        ThemeMode::Dark => hsl_to_rgb(hsl.h, (hsl.s * 0.14).clamp(0.035, 0.12), 0.035),
        ThemeMode::Light => shadow_base,
    };
    let inset_shadow_base = match mode {
        ThemeMode::Dark => hsl_to_rgb(hsl.h, (hsl.s * 0.10).clamp(0.025, 0.09), 0.045),
        ThemeMode::Light => shadow_base,
    };
    let accent_shadow = match mode {
        ThemeMode::Dark => Color32::from_rgba_unmultiplied(
            accent_shadow_base.r(),
            accent_shadow_base.g(),
            accent_shadow_base.b(),
            92,
        ),
        ThemeMode::Light => {
            Color32::from_rgba_unmultiplied(shadow_base.r(), shadow_base.g(), shadow_base.b(), 58)
        }
    };
    let mut palette = match mode {
        ThemeMode::Dark => Palette {
            bg: neutral(0.085),
            chrome_bg: neutral(0.055),
            panel: neutral(0.115),
            panel_soft: neutral(0.155),
            panel_recessed: neutral(0.125),
            text: muted_neutral(0.97),
            muted: muted_neutral(0.78),
            accent: accent_color,
            accent_deep,
            accent_soft,
            accent_shadow,
            hover,
            scroll_track,
            inset_highlight: Color32::from_rgba_unmultiplied(255, 255, 255, 28),
            inset_shadow: Color32::from_rgba_unmultiplied(
                inset_shadow_base.r(),
                inset_shadow_base.g(),
                inset_shadow_base.b(),
                132,
            ),
            info: Color32::from_rgb(120, 164, 255),
            warning: WARNING,
            menu_disabled: muted_neutral(0.36),
            diff_added_bg: Color32::from_rgb(23, 67, 45),
            diff_added_text: Color32::from_rgb(153, 232, 180),
            diff_removed_bg: Color32::from_rgb(78, 35, 42),
            diff_removed_text: Color32::from_rgb(255, 178, 184),
            diff_context_text: muted_neutral(0.72),
            diff_gutter: neutral(0.115),
            diff_gutter_text: muted_neutral(0.52),
            diff_gutter_border: muted_neutral(0.24),
            diff_selected: hsl_to_rgb(hsl.h, (hsl.s * 0.55).clamp(0.0, 1.0), 0.33),
            diff_selected_gutter: hsl_to_rgb(hsl.h, (hsl.s * 0.42).clamp(0.0, 1.0), 0.24),
            diff_selected_gutter_text: muted_neutral(0.96),
            diff_indent_guide: muted_neutral(0.30),
            syntax_comment: Color32::from_rgb(140, 166, 134),
            syntax_string: Color32::from_rgb(224, 168, 112),
            syntax_number: Color32::from_rgb(196, 146, 232),
            syntax_keyword: Color32::from_rgb(126, 166, 255),
            syntax_type: Color32::from_rgb(91, 201, 191),
            syntax_function: Color32::from_rgb(232, 202, 126),
            syntax_constant: Color32::from_rgb(204, 157, 238),
            syntax_variable: muted_neutral(0.90),
            syntax_tag: Color32::from_rgb(116, 201, 145),
            syntax_attribute: Color32::from_rgb(236, 181, 116),
            syntax_operator: Color32::from_rgb(185, 196, 215),
            syntax_punctuation: muted_neutral(0.72),
            syntax_invalid: Color32::from_rgb(255, 116, 126),
        },
        ThemeMode::Light => Palette {
            bg: neutral(0.948),
            chrome_bg: neutral(0.91),
            panel: neutral(0.982),
            panel_soft: neutral(0.91),
            panel_recessed: recessed_neutral(0.985),
            text: muted_neutral(0.16),
            muted: muted_neutral(0.46),
            accent: accent_color,
            accent_deep,
            accent_soft,
            accent_shadow,
            hover,
            scroll_track,
            inset_highlight: Color32::from_rgba_unmultiplied(255, 255, 255, 190),
            inset_shadow: Color32::from_rgba_unmultiplied(
                shadow_base.r(),
                shadow_base.g(),
                shadow_base.b(),
                86,
            ),
            info: Color32::from_rgb(59, 107, 185),
            warning: Color32::from_rgb(181, 98, 28),
            menu_disabled: muted_neutral(0.66),
            diff_added_bg: Color32::from_rgb(214, 250, 221),
            diff_added_text: Color32::from_rgb(16, 92, 42),
            diff_removed_bg: Color32::from_rgb(255, 226, 226),
            diff_removed_text: Color32::from_rgb(142, 37, 37),
            diff_context_text: muted_neutral(0.34),
            diff_gutter: Color32::from_rgb(248, 250, 252),
            diff_gutter_text: muted_neutral(0.50),
            diff_gutter_border: muted_neutral(0.84),
            diff_selected: hsl_to_rgb(hsl.h, (hsl.s * 0.70).clamp(0.0, 1.0), 0.56),
            diff_selected_gutter: hsl_to_rgb(hsl.h, (hsl.s * 0.52).clamp(0.0, 1.0), 0.48),
            diff_selected_gutter_text: Color32::from_rgb(235, 247, 255),
            diff_indent_guide: muted_neutral(0.74),
            syntax_comment: Color32::from_rgb(88, 112, 82),
            syntax_string: Color32::from_rgb(157, 72, 13),
            syntax_number: Color32::from_rgb(117, 71, 164),
            syntax_keyword: Color32::from_rgb(25, 83, 156),
            syntax_type: Color32::from_rgb(0, 111, 111),
            syntax_function: Color32::from_rgb(112, 78, 9),
            syntax_constant: Color32::from_rgb(111, 66, 151),
            syntax_variable: muted_neutral(0.20),
            syntax_tag: Color32::from_rgb(24, 111, 65),
            syntax_attribute: Color32::from_rgb(126, 72, 10),
            syntax_operator: Color32::from_rgb(67, 78, 94),
            syntax_punctuation: muted_neutral(0.36),
            syntax_invalid: Color32::from_rgb(184, 24, 42),
        },
    };
    apply_theme_pool(&mut palette, mode, accent);
    palette
}

pub fn bg() -> Color32 {
    palette(current_mode()).bg
}

pub fn chrome_bg() -> Color32 {
    palette(current_mode()).chrome_bg
}

pub fn panel() -> Color32 {
    palette(current_mode()).panel
}

pub fn panel_soft() -> Color32 {
    palette(current_mode()).panel_soft
}

pub fn panel_recessed() -> Color32 {
    palette(current_mode()).panel_recessed
}

pub fn text() -> Color32 {
    palette(current_mode()).text
}

/// Applies the borderless, high-contrast foreground used by every selected
/// ComboBox, menu, and list item.
pub fn apply_selected_item_visuals(visuals: &mut Visuals) {
    // egui takes a selected widget's foreground from this stroke color.
    // Width zero retains the no-outline appearance.
    visuals.selection.stroke = Stroke::new(0.0, Color32::WHITE);
}

/// Applies the lower-contrast selection treatment for editable text.
pub fn apply_text_edit_selection_visuals(visuals: &mut Visuals) {
    visuals.selection.bg_fill = accent_soft();
    visuals.selection.stroke = Stroke::new(0.0, text());
}

pub fn muted() -> Color32 {
    palette(current_mode()).muted
}

pub fn accent() -> Color32 {
    palette(current_mode()).accent
}

pub fn accent_deep() -> Color32 {
    palette(current_mode()).accent_deep
}

pub fn accent_soft() -> Color32 {
    palette(current_mode()).accent_soft
}

pub fn accent_shadow() -> Color32 {
    palette(current_mode()).accent_shadow
}

pub fn hover() -> Color32 {
    palette(current_mode()).hover
}

pub fn inset_highlight() -> Color32 {
    palette(current_mode()).inset_highlight
}

pub fn inset_shadow() -> Color32 {
    palette(current_mode()).inset_shadow
}

pub fn info() -> Color32 {
    palette(current_mode()).info
}

pub fn warning() -> Color32 {
    palette(current_mode()).warning
}

pub fn menu_disabled() -> Color32 {
    palette(current_mode()).menu_disabled
}

pub fn diff_added_bg() -> Color32 {
    palette(current_mode()).diff_added_bg
}

pub fn diff_added_text() -> Color32 {
    palette(current_mode()).diff_added_text
}

pub fn diff_removed_bg() -> Color32 {
    palette(current_mode()).diff_removed_bg
}

pub fn diff_removed_text() -> Color32 {
    palette(current_mode()).diff_removed_text
}

pub fn diff_context_text() -> Color32 {
    palette(current_mode()).diff_context_text
}

pub fn diff_gutter() -> Color32 {
    palette(current_mode()).diff_gutter
}

pub fn diff_gutter_text() -> Color32 {
    palette(current_mode()).diff_gutter_text
}

pub fn diff_gutter_border() -> Color32 {
    palette(current_mode()).diff_gutter_border
}

pub fn diff_selected() -> Color32 {
    palette(current_mode()).diff_selected
}

pub fn diff_selected_gutter() -> Color32 {
    palette(current_mode()).diff_selected_gutter
}

pub fn diff_selected_gutter_text() -> Color32 {
    palette(current_mode()).diff_selected_gutter_text
}

pub fn diff_indent_guide() -> Color32 {
    palette(current_mode()).diff_indent_guide
}

pub fn syntax_color(role: crate::syntax::SyntaxRole) -> Color32 {
    let palette = palette(current_mode());
    match role {
        crate::syntax::SyntaxRole::Plain => palette.text,
        crate::syntax::SyntaxRole::Comment => palette.syntax_comment,
        crate::syntax::SyntaxRole::String => palette.syntax_string,
        crate::syntax::SyntaxRole::Number => palette.syntax_number,
        crate::syntax::SyntaxRole::Keyword => palette.syntax_keyword,
        crate::syntax::SyntaxRole::Type => palette.syntax_type,
        crate::syntax::SyntaxRole::Function => palette.syntax_function,
        crate::syntax::SyntaxRole::Constant => palette.syntax_constant,
        crate::syntax::SyntaxRole::Variable => palette.syntax_variable,
        crate::syntax::SyntaxRole::Tag => palette.syntax_tag,
        crate::syntax::SyntaxRole::Attribute => palette.syntax_attribute,
        crate::syntax::SyntaxRole::Operator => palette.syntax_operator,
        crate::syntax::SyntaxRole::Punctuation => palette.syntax_punctuation,
        crate::syntax::SyntaxRole::Invalid => palette.syntax_invalid,
    }
}

pub fn all_accents() -> [ThemeAccent; 8] {
    [
        ThemeAccent::Blue,
        ThemeAccent::Green,
        ThemeAccent::Purple,
        ThemeAccent::Rose,
        ThemeAccent::Orange,
        ThemeAccent::SunRed,
        ThemeAccent::DeepSeaBlue,
        ThemeAccent::ForestGreen,
    ]
}

pub fn accent_color(accent: ThemeAccent) -> Color32 {
    configured_color(current_mode(), accent, "--accent").unwrap_or_else(|| accent_seed(accent))
}

pub fn pattern_accent(mode: ThemeMode, accent: ThemeAccent) -> Color32 {
    let token = match accent {
        ThemeAccent::Green => "--pattern-accent-green",
        ThemeAccent::ForestGreen => "--pattern-accent-forest",
        ThemeAccent::SunRed => "--pattern-accent-red",
        _ => "--pattern-accent",
    };
    configured_color(mode, accent, token).unwrap_or_else(|| {
        let hue = rgb_to_hsl(accent_seed(accent)).h;
        let (saturation, lightness, alpha) = match (mode, accent) {
            (ThemeMode::Light, ThemeAccent::SunRed) => (0.68, 0.50, 36),
            (ThemeMode::Dark, ThemeAccent::SunRed) => (0.66, 0.68, 46),
            (ThemeMode::Light, ThemeAccent::Green) => (0.50, 0.32, 46),
            (ThemeMode::Dark, ThemeAccent::Green) => (0.50, 0.68, 51),
            (ThemeMode::Light, ThemeAccent::ForestGreen) => (0.42, 0.28, 46),
            (ThemeMode::Dark, ThemeAccent::ForestGreen) => (0.42, 0.65, 51),
            (ThemeMode::Light, _) => (0.78, 0.42, 56),
            (ThemeMode::Dark, _) => (0.60, 0.70, 56),
        };
        let color = hsl_to_rgb(hue, saturation, lightness);
        Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
    })
}

fn accent_index(accent: ThemeAccent) -> u8 {
    match accent {
        ThemeAccent::Green => 0,
        ThemeAccent::Blue => 1,
        ThemeAccent::Purple => 2,
        ThemeAccent::Rose => 3,
        ThemeAccent::Orange => 4,
        ThemeAccent::SunRed => 5,
        ThemeAccent::DeepSeaBlue => 6,
        ThemeAccent::ForestGreen => 7,
    }
}

fn accent_from_index(index: u8) -> ThemeAccent {
    match index {
        1 => ThemeAccent::Blue,
        2 => ThemeAccent::Purple,
        3 => ThemeAccent::Rose,
        4 => ThemeAccent::Orange,
        5 => ThemeAccent::SunRed,
        6 => ThemeAccent::DeepSeaBlue,
        7 => ThemeAccent::ForestGreen,
        _ => ThemeAccent::Green,
    }
}

fn accent_seed(accent: ThemeAccent) -> Color32 {
    match accent {
        ThemeAccent::Green => ACCENT,
        ThemeAccent::Blue => Color32::from_rgb(74, 137, 229),
        ThemeAccent::Purple => Color32::from_rgb(142, 105, 222),
        ThemeAccent::Rose => Color32::from_rgb(210, 88, 132),
        ThemeAccent::Orange => Color32::from_rgb(213, 126, 48),
        ThemeAccent::SunRed => Color32::from_rgb(220, 55, 55),
        ThemeAccent::DeepSeaBlue => Color32::from_rgb(47, 91, 194),
        ThemeAccent::ForestGreen => Color32::from_rgb(52, 159, 91),
    }
}

#[derive(Clone, Copy, Debug)]
struct Hsl {
    h: f32,
    s: f32,
    l: f32,
}

fn rgb_to_hsl(color: Color32) -> Hsl {
    let r = color.r() as f32 / 255.0;
    let g = color.g() as f32 / 255.0;
    let b = color.b() as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) * 0.5;
    if (max - min).abs() < f32::EPSILON {
        return Hsl { h: 0.0, s: 0.0, l };
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let mut h = if (max - r).abs() < f32::EPSILON {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if (max - g).abs() < f32::EPSILON {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h /= 6.0;
    Hsl { h, s, l }
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> Color32 {
    if s <= 0.0 {
        let v = (l.clamp(0.0, 1.0) * 255.0).round() as u8;
        return Color32::from_rgb(v, v, v);
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let r = hue_to_rgb(p, q, h + 1.0 / 3.0);
    let g = hue_to_rgb(p, q, h);
    let b = hue_to_rgb(p, q, h - 1.0 / 3.0);
    Color32::from_rgb(
        (r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (b.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        p + (q - p) * 6.0 * t
    } else if t < 1.0 / 2.0 {
        q
    } else if t < 2.0 / 3.0 {
        p + (q - p) * (2.0 / 3.0 - t) * 6.0
    } else {
        p
    }
}

#[cfg(test)]
mod theme_pool_tests {
    use super::*;

    #[test]
    fn local_theme_overrides_only_declared_values() {
        let mut pool: ThemePool = serde_json::from_str(EMBEDDED_THEME_JSON).unwrap();
        let original_green = pool.hues["green"].clone();
        let original_light_bg = pool.themes["light"]["--bg"].clone();
        let local: ThemePool = serde_json::from_str(
            r#"{
                "hues": {"blue": "211deg"},
                "themes": {"dark": {"--panel": "hsl(${c} 9% 12%)"}}
            }"#,
        )
        .unwrap();

        pool.overlay(local);

        assert_eq!(pool.hues["blue"], "211deg");
        assert_eq!(pool.hues["green"], original_green);
        assert_eq!(pool.themes["dark"]["--panel"], "hsl(${c} 9% 12%)");
        assert_eq!(pool.themes["light"]["--bg"], original_light_bg);
    }

    #[test]
    fn local_theme_is_sibling_of_selected_theme() {
        assert_eq!(
            local_theme_path(Path::new("x/custom-theme.json")),
            PathBuf::from("x/theme.local.json")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_theme_pool_resolves_hsl_templates() {
        let pool: ThemePool = serde_json::from_str(EMBEDDED_THEME_JSON)
            .expect("embedded theme pool should be valid JSON");
        let hue = pool.hues.get("blue").expect("blue hue should exist");
        let template = pool
            .themes
            .get("dark")
            .and_then(|theme| theme.get("--primary-test-color"))
            .expect("dark primary test color should exist");
        let color = parse_hsl_color(&template.replace("${c}", hue))
            .expect("HSL template should resolve to a color");

        assert_eq!(color.a(), 153);
        assert!(color.b() > color.r());
    }

    #[test]
    fn neutral_surfaces_are_derived_from_theme_accent_hsl() {
        let green = palette_for(ThemeMode::Light, ThemeAccent::Green);
        let blue = palette_for(ThemeMode::Light, ThemeAccent::Blue);
        let dark_green = palette_for(ThemeMode::Dark, ThemeAccent::Green);
        let dark_blue = palette_for(ThemeMode::Dark, ThemeAccent::Blue);

        assert_ne!(green.bg, blue.bg);
        assert_ne!(green.panel, blue.panel);
        assert_ne!(green.panel_soft, blue.panel_soft);
        // External HSL theme tokens may round very pale recessed surfaces to
        // the same RGB value. Accent-bound surfaces must still differ.
        assert_ne!(green.accent, blue.accent);
        assert_ne!(green.accent_shadow, blue.accent_shadow);
        assert!(green.panel_recessed.r() >= green.panel.r());
        assert!(green.panel_recessed.g() >= green.panel.g());
        assert!(green.panel_recessed.b() >= green.panel.b());
        assert_ne!(dark_green.panel, dark_blue.panel);
        assert_ne!(dark_green.panel_recessed, dark_blue.panel_recessed);
    }

    #[test]
    fn dark_theme_uses_visible_neutral_inset_highlight_and_shadow() {
        let palette = palette_for(ThemeMode::Dark, ThemeAccent::Blue);
        let shadow_hsl = rgb_to_hsl(Color32::from_rgb(
            palette.accent_shadow.r(),
            palette.accent_shadow.g(),
            palette.accent_shadow.b(),
        ));
        let inset_hsl = rgb_to_hsl(Color32::from_rgb(
            palette.inset_shadow.r(),
            palette.inset_shadow.g(),
            palette.inset_shadow.b(),
        ));
        let recessed_hsl = rgb_to_hsl(palette.panel_recessed);

        assert!(palette.inset_highlight.a() >= 20);
        assert!(palette.accent_shadow.a() >= 64);
        let shadow_spread = palette
            .accent_shadow
            .r()
            .max(palette.accent_shadow.g())
            .max(palette.accent_shadow.b())
            - palette
                .accent_shadow
                .r()
                .min(palette.accent_shadow.g())
                .min(palette.accent_shadow.b());
        let inset_spread = palette
            .inset_shadow
            .r()
            .max(palette.inset_shadow.g())
            .max(palette.inset_shadow.b())
            - palette
                .inset_shadow
                .r()
                .min(palette.inset_shadow.g())
                .min(palette.inset_shadow.b());
        assert!(
            shadow_spread <= 4,
            "dark accent shadow should stay near neutral"
        );
        assert!(shadow_hsl.l <= 0.06);
        assert!(palette.inset_shadow.a() >= 96);
        assert!(inset_hsl.l <= 0.10);
        assert!(
            inset_spread <= 3,
            "dark inset shadow should stay near neutral"
        );
        assert!(
            recessed_hsl.s <= 0.14,
            "dark text edit background should stay near neutral, got {recessed_hsl:?}"
        );
    }

    #[test]
    fn dark_theme_uses_tinted_near_white_text_for_readability() {
        let palette = palette_for(ThemeMode::Dark, ThemeAccent::Blue);
        let text_hsl = rgb_to_hsl(palette.text);
        let muted_hsl = rgb_to_hsl(palette.muted);

        assert!(text_hsl.l >= 0.95, "primary text should be near white");
        assert!(text_hsl.s >= 0.10, "primary text should retain theme hue");
        assert!(muted_hsl.l >= 0.74, "secondary text should remain readable");

        let ctx = egui::Context::default();
        apply(&ctx, ThemeMode::Dark, ThemeAccent::Blue);
        let visuals = &ctx.style().visuals;
        assert_eq!(visuals.widgets.noninteractive.fg_stroke.color, palette.text);
        assert_eq!(visuals.widgets.inactive.fg_stroke.color, palette.text);
    }

    #[test]
    fn dark_chrome_background_is_deeper_than_main_background() {
        let dark = palette_for(ThemeMode::Dark, ThemeAccent::Blue);
        let light = palette_for(ThemeMode::Light, ThemeAccent::Blue);
        let dark_bg_hsl = rgb_to_hsl(dark.bg);
        let dark_chrome_hsl = rgb_to_hsl(dark.chrome_bg);

        assert!(dark_chrome_hsl.l < dark_bg_hsl.l);
        assert!(dark_chrome_hsl.s <= 0.12);
        assert!((rgb_to_hsl(light.chrome_bg).l - rgb_to_hsl(light.panel_soft).l).abs() <= 0.01);
    }

    #[test]
    fn blue_theme_accent_is_first_option() {
        assert_eq!(all_accents()[0], ThemeAccent::Blue);
    }

    #[test]
    fn all_theme_accents_include_the_eight_named_hues() {
        assert_eq!(
            all_accents(),
            [
                ThemeAccent::Blue,
                ThemeAccent::Green,
                ThemeAccent::Purple,
                ThemeAccent::Rose,
                ThemeAccent::Orange,
                ThemeAccent::SunRed,
                ThemeAccent::DeepSeaBlue,
                ThemeAccent::ForestGreen,
            ]
        );
        assert_ne!(
            accent_color(ThemeAccent::SunRed),
            accent_color(ThemeAccent::ForestGreen)
        );
        assert_ne!(
            accent_color(ThemeAccent::DeepSeaBlue),
            accent_color(ThemeAccent::Blue)
        );
        let sky = palette_for(ThemeMode::Light, ThemeAccent::Blue).accent;
        let deep_sea = palette_for(ThemeMode::Light, ThemeAccent::DeepSeaBlue).accent;
        let luminance = |color: Color32| {
            0.2126 * color.r() as f32 + 0.7152 * color.g() as f32 + 0.0722 * color.b() as f32
        };
        assert!(
            luminance(deep_sea) < luminance(sky),
            "deep sea blue should read darker than sky blue"
        );
    }

    #[test]
    fn background_pattern_uses_separate_day_and_night_color_tuning() {
        let light = pattern_accent(ThemeMode::Light, ThemeAccent::Blue);
        let dark = pattern_accent(ThemeMode::Dark, ThemeAccent::Blue);
        let unpremultiplied_hsl = |color: Color32| {
            let [r, g, b, _] = color.to_srgba_unmultiplied();
            rgb_to_hsl(Color32::from_rgb(r, g, b))
        };
        let light_hsl = unpremultiplied_hsl(light);
        let dark_hsl = unpremultiplied_hsl(dark);
        let light_red = pattern_accent(ThemeMode::Light, ThemeAccent::SunRed);
        let light_green = pattern_accent(ThemeMode::Light, ThemeAccent::Green);
        let light_forest = pattern_accent(ThemeMode::Light, ThemeAccent::ForestGreen);
        let light_green_hsl = unpremultiplied_hsl(light_green);
        let light_forest_hsl = unpremultiplied_hsl(light_forest);

        assert!(light_hsl.s >= 0.74);
        assert!((0.38..=0.46).contains(&light_hsl.l));
        assert!(dark_hsl.s <= light_hsl.s);
        assert!(dark_hsl.l >= 0.66);
        assert!(dark_hsl.l > light_hsl.l);
        assert!(light.a() >= 52);
        assert!(dark.a() >= 52);
        assert!(light_red.a() < light.a());
        assert!(light_green_hsl.s < light_hsl.s);
        assert!(light_green_hsl.l < light_hsl.l);
        assert!(light_forest_hsl.s <= light_green_hsl.s);
        assert!(light_forest_hsl.l < light_green_hsl.l);
    }

    #[test]
    fn every_background_pattern_keeps_its_theme_accent_hue() {
        let unpremultiplied_hsl = |color: Color32| {
            let [r, g, b, _] = color.to_srgba_unmultiplied();
            rgb_to_hsl(Color32::from_rgb(r, g, b))
        };
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            for accent in all_accents() {
                let main = configured_color(mode, accent, "--accent").unwrap();
                let pattern = pattern_accent(mode, accent);
                let main_hue = unpremultiplied_hsl(main).h;
                let pattern_hue = unpremultiplied_hsl(pattern).h;
                let direct = (main_hue - pattern_hue).abs();
                let circular_difference = direct.min(1.0 - direct);
                assert!(
                    circular_difference <= 0.01,
                    "{mode:?}/{accent:?} pattern hue {pattern_hue} diverged from main hue {main_hue}"
                );
            }
        }
    }

    #[test]
    fn apply_if_needed_skips_unchanged_theme() {
        let ctx = egui::Context::default();

        apply(&ctx, ThemeMode::Light, ThemeAccent::Blue);
        assert!(!apply_if_needed(&ctx, ThemeMode::Light, ThemeAccent::Blue));
        assert!(apply_if_needed(&ctx, ThemeMode::Dark, ThemeAccent::Blue));
        assert!(!apply_if_needed(&ctx, ThemeMode::Dark, ThemeAccent::Blue));
        assert!(apply_if_needed(&ctx, ThemeMode::Dark, ThemeAccent::Orange));
    }

    #[test]
    fn supported_font_weights_only_expose_real_readable_faces() {
        assert_eq!(supported_font_weight(400), Some(FontWeight::Regular));
        assert_eq!(supported_font_weight(500), Some(FontWeight::Medium));
        assert_eq!(supported_font_weight(600), Some(FontWeight::Semibold));
        assert_eq!(supported_font_weight(700), Some(FontWeight::Bold));
        assert_eq!(supported_font_weight(100), None);
        assert_eq!(supported_font_weight(900), None);
    }

    #[test]
    fn code_font_detection_accepts_mono_names_without_trusting_fixed_pitch_only() {
        assert!(looks_like_monospace_font("Maple Mono NF CN"));
        assert!(looks_like_monospace_font("MapleMono-NF-CN-Regular"));
        assert!(looks_like_monospace_font("JetBrainsMono"));
        assert!(looks_like_monospace_font("Some Fixed Pitch Font"));
        assert!(!looks_like_monospace_font("Monotype Corsiva"));
        assert!(!looks_like_monospace_font("Noto Sans SC"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn installed_maple_mono_is_available_to_the_code_font_pool() {
        let Some(user_profile) = std::env::var_os("USERPROFILE") else {
            return;
        };
        let user_font_dir = PathBuf::from(user_profile)
            .join("AppData")
            .join("Local")
            .join("Microsoft")
            .join("Windows")
            .join("Fonts");
        let maple_is_installed = fs::read_dir(user_font_dir).ok().is_some_and(|entries| {
            entries.filter_map(Result::ok).any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains("maplemono")
            })
        });
        if !maple_is_installed {
            return;
        }

        let catalog = scan_system_fonts();
        assert!(catalog.iter().any(|family| {
            family.monospaced && family.name.to_lowercase().contains("maple mono")
        }));
    }

    #[test]
    fn automatic_font_resolution_prefers_ui_and_code_system_defaults() {
        let family = |name: &str, monospaced: bool| SystemFontFamily {
            name: name.to_owned(),
            monospaced,
            faces: vec![SystemFontFace {
                weight: FontWeight::Regular,
                path: PathBuf::from(format!("{name}.ttf")),
                index: 0,
            }],
        };
        let catalog = vec![
            family("Arial", false),
            family("Consolas", true),
            family("Microsoft YaHei UI", false),
            family("Noto Sans SC", false),
            family("Cascadia Mono", true),
        ];

        assert_eq!(
            resolve_font_family(&catalog, None, false).map(|font| font.name.as_str()),
            Some("Noto Sans SC")
        );
        assert_eq!(
            resolve_font_family(&catalog, None, true).map(|font| font.name.as_str()),
            Some("Cascadia Mono")
        );
        assert_eq!(
            resolve_font_family(&catalog, Some("Arial"), false).map(|font| font.name.as_str()),
            Some("Arial")
        );
        assert_eq!(
            resolve_font_family(&catalog, Some("Cascadia Mono"), false)
                .map(|font| font.name.as_str()),
            Some("Cascadia Mono")
        );
        assert!(font_family_names(&catalog, false).contains(&"Cascadia Mono".to_owned()));
        assert!(!font_family_names(&catalog, true).contains(&"Arial".to_owned()));
    }

    #[test]
    fn text_contrast_adjusts_text_without_changing_theme_tokens() {
        let luminance = |color: Color32| {
            0.2126 * color.r() as f32 + 0.7152 * color.g() as f32 + 0.0722 * color.b() as f32
        };
        let light_standard = palette_for(ThemeMode::Light, ThemeAccent::Blue);
        let light_soft =
            palette_with_text_contrast(ThemeMode::Light, ThemeAccent::Blue, TextContrast::Soft);
        let light_strong =
            palette_with_text_contrast(ThemeMode::Light, ThemeAccent::Blue, TextContrast::Strong);
        assert!(luminance(light_soft.text) > luminance(light_standard.text));
        assert!(luminance(light_strong.text) < luminance(light_standard.text));

        let dark_standard = palette_for(ThemeMode::Dark, ThemeAccent::Blue);
        let dark_soft =
            palette_with_text_contrast(ThemeMode::Dark, ThemeAccent::Blue, TextContrast::Soft);
        let dark_strong =
            palette_with_text_contrast(ThemeMode::Dark, ThemeAccent::Blue, TextContrast::Strong);
        assert!(luminance(dark_soft.text) < luminance(dark_standard.text));
        assert!(luminance(dark_strong.text) > luminance(dark_standard.text));
        assert_eq!(
            palette_for(ThemeMode::Light, ThemeAccent::Blue).text,
            light_standard.text
        );
    }
}

fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(default_font_definitions());
}

fn default_font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    if let Some(path) = system_cjk_font_path() {
        if let Ok(bytes) = std::fs::read(path) {
            let name = "system_cjk_fallback".to_owned();
            fonts
                .font_data
                .insert(name.clone(), FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, name.clone());
            fonts
                .families
                .entry(FontFamily::Monospace)
                .or_default()
                .push(name);
        }
    }
    fonts
}

fn system_cjk_font_path() -> Option<PathBuf> {
    [
        r"C:\Windows\Fonts\NotoSansSC-VF.ttf",
        r"C:\Windows\Fonts\msyh.ttc",
        r"C:\Windows\Fonts\simhei.ttf",
        r"C:\Windows\Fonts\simsun.ttc",
    ]
    .into_iter()
    .map(PathBuf::from)
    .find(|path| path.is_file())
}

pub fn scan_system_fonts() -> Vec<SystemFontFamily> {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    let mut families = BTreeMap::<(String, bool), SystemFontFamily>::new();

    for face in database.faces() {
        if face.style != fontdb::Style::Normal {
            continue;
        }
        let path = match &face.source {
            fontdb::Source::File(path) | fontdb::Source::SharedFile(path, _) => path.clone(),
            fontdb::Source::Binary(_) => continue,
        };
        let supports_basic_latin = database
            .with_face_data(face.id, |data, index| {
                ttf_parser::Face::parse(data, index)
                    .ok()
                    .is_some_and(|font| {
                        ['A', 'a', '0']
                            .into_iter()
                            .all(|c| font.glyph_index(c).is_some())
                    })
            })
            .unwrap_or(false);
        if !supports_basic_latin {
            continue;
        }
        let Some((family_name, _)) = face.families.first() else {
            continue;
        };
        let monospaced = face.monospaced
            || looks_like_monospace_font(family_name)
            || looks_like_monospace_font(&face.post_script_name)
            || path
                .file_stem()
                .is_some_and(|name| looks_like_monospace_font(&name.to_string_lossy()));
        let Some(weight) = supported_font_weight(face.weight.0) else {
            continue;
        };
        let key = (family_name.to_lowercase(), monospaced);
        let family = families.entry(key).or_insert_with(|| SystemFontFamily {
            name: family_name.clone(),
            monospaced,
            faces: Vec::new(),
        });
        if !family
            .faces
            .iter()
            .any(|candidate| candidate.weight == weight)
        {
            family.faces.push(SystemFontFace {
                weight,
                path,
                index: face.index,
            });
        }
    }

    let mut families = families.into_values().collect::<Vec<_>>();
    for family in &mut families {
        family.faces.sort_by_key(|face| face.weight);
    }
    families.sort_by_key(|family| family.name.to_lowercase());
    families
}

fn supported_font_weight(weight: u16) -> Option<FontWeight> {
    match weight {
        350..=450 => Some(FontWeight::Regular),
        451..=550 => Some(FontWeight::Medium),
        551..=650 => Some(FontWeight::Semibold),
        651..=750 => Some(FontWeight::Bold),
        _ => None,
    }
}

fn looks_like_monospace_font(name: &str) -> bool {
    let normalized = name.to_lowercase().replace(['_', '-'], " ");
    (normalized.contains("mono") && !normalized.contains("monotype"))
        || normalized.contains("fixed pitch")
        || normalized.contains("code nerd font")
}

pub fn load_font_set(
    selection: &FontSelection,
    catalog: &[SystemFontFamily],
) -> Result<LoadedFontSet, String> {
    let mut fonts = FontDefinitions::default();
    let mut registered = HashMap::<(PathBuf, u32), String>::new();

    let ui_family = resolve_font_family(catalog, selection.ui_family.as_deref(), false);
    if let Some(face) = ui_family.and_then(|family| closest_face(family, selection.ui_weight)) {
        let name = register_font_face(&mut fonts, &mut registered, face)?;
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, name);
    }

    let code_family = resolve_font_family(catalog, selection.code_family.as_deref(), true);
    if let Some(face) = code_family.and_then(|family| closest_face(family, FontWeight::Regular)) {
        let name = register_font_face(&mut fonts, &mut registered, face)?;
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, name);
    }

    if let Some(path) = system_cjk_font_path() {
        let fallback = SystemFontFace {
            weight: FontWeight::Regular,
            path,
            index: 0,
        };
        if let Ok(name) = register_font_face(&mut fonts, &mut registered, &fallback) {
            let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
            if ui_family.is_none() {
                proportional.insert(0, name.clone());
            } else if !proportional.contains(&name) {
                proportional.push(name.clone());
            }
            let monospace = fonts.families.entry(FontFamily::Monospace).or_default();
            if !monospace.contains(&name) {
                monospace.push(name);
            }
        }
    }

    Ok(LoadedFontSet(fonts))
}

pub fn apply_loaded_font_set(ctx: &egui::Context, fonts: LoadedFontSet) {
    ctx.set_fonts(fonts.0);
}

fn resolve_font_family<'a>(
    catalog: &'a [SystemFontFamily],
    requested: Option<&str>,
    monospaced_only: bool,
) -> Option<&'a SystemFontFamily> {
    if let Some(requested) = requested {
        return catalog.iter().find(|family| {
            (!monospaced_only || family.monospaced) && family.name.eq_ignore_ascii_case(requested)
        });
    }

    let priorities: &[&str] = if monospaced_only {
        &[
            "Cascadia Mono",
            "Cascadia Code",
            "JetBrains Mono",
            "Consolas",
        ]
    } else {
        &[
            "Noto Sans SC",
            "Noto Sans CJK SC",
            "Microsoft YaHei UI",
            "Microsoft YaHei",
            "Segoe UI",
        ]
    };
    priorities
        .iter()
        .find_map(|name| {
            catalog.iter().find(|family| {
                (!monospaced_only || family.monospaced) && family.name.eq_ignore_ascii_case(name)
            })
        })
        .or_else(|| {
            catalog
                .iter()
                .find(|family| !monospaced_only || family.monospaced)
        })
}

fn closest_face(family: &SystemFontFamily, weight: FontWeight) -> Option<&SystemFontFace> {
    family
        .faces
        .iter()
        .min_by_key(|face| face.weight.numeric().abs_diff(weight.numeric()))
}

fn register_font_face(
    fonts: &mut FontDefinitions,
    registered: &mut HashMap<(PathBuf, u32), String>,
    face: &SystemFontFace,
) -> Result<String, String> {
    let key = (face.path.clone(), face.index);
    if let Some(name) = registered.get(&key) {
        return Ok(name.clone());
    }
    let bytes = fs::read(&face.path)
        .map_err(|error| format!("Unable to load font {}: {error}", face.path.display()))?;
    let name = format!("system_font_{}", registered.len());
    let mut data = FontData::from_owned(bytes);
    data.index = face.index;
    fonts.font_data.insert(name.clone(), data.into());
    registered.insert(key, name.clone());
    Ok(name)
}
