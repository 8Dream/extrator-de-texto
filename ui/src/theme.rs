//! Cores, fontes e estilo (mesmo visual da interface HTML: src\web\styles.css).

use crate::cores::{derivar, Rgb};
use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
};
use std::sync::Arc;

const fn rgb(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub bg: Color32,
    pub surface: Color32,
    pub surface2: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub accent_text: Color32,
    pub accent_hover: Color32,
    pub accent_press: Color32,
    pub ok: Color32,
    pub ok_soft: Color32,
    pub warn: Color32,
    pub warn_soft: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    pub dark: bool,
}

impl Palette {
    pub const CLARO: Palette = Palette {
        bg: rgb(0xf4f5f7),
        surface: rgb(0xffffff),
        surface2: rgb(0xf8f9fb),
        border: rgb(0xdfe3e8),
        border_strong: rgb(0xc8ced6),
        text: rgb(0x1a1f26),
        muted: rgb(0x5d6877),
        accent: rgb(0x1068d8),
        accent_soft: rgb(0xe7edfb),
        accent_text: rgb(0xffffff),
        accent_hover: rgb(0x1a5cc0),
        accent_press: rgb(0x144fa5),
        ok: rgb(0x17794a),
        ok_soft: rgb(0xe3f4ea),
        warn: rgb(0x9a5b00),
        warn_soft: rgb(0xfcf0d9),
        danger: rgb(0xb3261e),
        danger_soft: rgb(0xfbe7e5),
        dark: false,
    };

    pub const ESCURO: Palette = Palette {
        bg: rgb(0x0f1216),
        surface: rgb(0x171b21),
        surface2: rgb(0x1c2128),
        border: rgb(0x29303a),
        border_strong: rgb(0x38414d),
        text: rgb(0xe7eaee),
        muted: rgb(0x9aa5b3),
        accent: rgb(0x6e96ff),
        accent_soft: rgb(0x1e2a45),
        accent_text: rgb(0x0b1020),
        accent_hover: rgb(0x8bacff),
        accent_press: rgb(0xa3bdff),
        ok: rgb(0x4cc58a),
        ok_soft: rgb(0x14291f),
        warn: rgb(0xe3a948),
        warn_soft: rgb(0x2e2512),
        danger: rgb(0xf1857e),
        danger_soft: rgb(0x331a18),
        dark: true,
    };

    /// Paleta de um modo com a cor de destaque escolhida. Devolve tambem se o tom precisou ser
    /// ajustado para manter o contraste minimo (WCAG AA). Cores de sucesso, aviso e erro nao mudam.
    pub fn montar(escuro: bool, destaque: Rgb) -> (Palette, bool) {
        let base = if escuro { Self::ESCURO } else { Self::CLARO };
        let d = derivar(destaque, escuro, para_rgb(base.surface), para_rgb(base.bg));
        let c = |v: Rgb| Color32::from_rgb(v[0], v[1], v[2]);
        (
            Palette {
                accent: c(d.destaque),
                accent_soft: c(d.suave),
                accent_text: c(d.sobre),
                accent_hover: c(d.hover),
                accent_press: c(d.pressionado),
                ..base
            },
            d.ajustada,
        )
    }
}

fn para_rgb(c: Color32) -> Rgb {
    [c.r(), c.g(), c.b()]
}

pub const RAIO: u8 = 10;
pub const RAIO_PEQ: u8 = 7;

/// Familia da fonte em negrito (semibold).
pub fn negrito() -> FontFamily {
    FontFamily::Name("negrito".into())
}

fn ler_fonte(nome: &str) -> Option<Vec<u8>> {
    let windir = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
    std::fs::read(std::path::Path::new(&windir).join("Fonts").join(nome)).ok()
}

/// Fontes: Segoe UI do Windows quando existir (mesmo visual da interface antiga);
/// senao, as fontes embutidas do egui (Ubuntu-Light e Hack, licencas livres).
pub fn instalar_fontes(ctx: &egui::Context) {
    let mut fontes = FontDefinitions::default();
    let normal = ler_fonte("segoeui.ttf");
    let semi = ler_fonte("seguisb.ttf");
    let simbolos = ler_fonte("seguisym.ttf");
    if let Some(dados) = normal {
        fontes
            .font_data
            .insert("segoe".into(), Arc::new(FontData::from_owned(dados)));
        fontes
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe".into());
    }
    let negrito_nome = if let Some(d) = semi {
        fontes
            .font_data
            .insert("segoe-semibold".into(), Arc::new(FontData::from_owned(d)));
        "segoe-semibold".to_string()
    } else if fontes.font_data.contains_key("segoe") {
        "segoe".to_string()
    } else {
        "Ubuntu-Light".to_string()
    };
    if let Some(d) = simbolos {
        fontes
            .font_data
            .insert("segoe-sym".into(), Arc::new(FontData::from_owned(d)));
        for fam in [FontFamily::Proportional, FontFamily::Monospace] {
            fontes
                .families
                .entry(fam)
                .or_default()
                .push("segoe-sym".into());
        }
    }
    let mut fam_neg = vec![negrito_nome];
    if let Some(p) = fontes.families.get(&FontFamily::Proportional) {
        fam_neg.extend(p.iter().cloned());
    }
    fontes.families.insert(negrito(), fam_neg);
    ctx.set_fonts(fontes);
}

pub fn aplicar(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.extreme_bg_color = p.surface;
    v.faint_bg_color = p.surface2;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent_soft;
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.menu_corner_radius = CornerRadius::same(RAIO_PEQ);
    v.window_corner_radius = CornerRadius::same(RAIO);
    let raio = CornerRadius::same(RAIO_PEQ);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = raio;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    v.widgets.noninteractive.bg_fill = p.surface;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.inactive.bg_fill = p.surface;
    v.widgets.inactive.weak_bg_fill = p.surface;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, p.border_strong);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.hovered.bg_fill = p.surface2;
    v.widgets.hovered.weak_bg_fill = p.surface2;
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
    v.widgets.active.bg_fill = p.accent_soft;
    v.widgets.active.weak_bg_fill = p.accent_soft;
    v.widgets.active.bg_stroke = Stroke::new(1.0, p.accent);
    v.widgets.active.fg_stroke = Stroke::new(1.0, p.text);
    let tema = if p.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_theme(tema);
    ctx.set_visuals_of(tema, v);
    ctx.style_mut_of(tema, |s| {
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(15.0, 7.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.scroll.bar_width = 10.0;
        s.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(24.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(14.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(12.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.5, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}
