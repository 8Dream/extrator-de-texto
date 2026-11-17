//! Componentes visuais reutilizaveis (botoes, chips, barras, cartoes...).

use crate::theme::{negrito, Palette, RAIO, RAIO_PEQ};
use eframe::egui::{
    self, vec2, Align, Align2, Button, Color32, CornerRadius, FontId, Frame, Layout, Margin, Pos2,
    Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, Ui,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tipo {
    Normal,
    Primario,
    Perigo,
    Link,
}

/// Botao com estados: normal, primario, perigo ou link; pequeno; ocupado (mostra o texto de
/// andamento com um indicador girando e nao aceita novo clique).
pub struct Btn<'a> {
    texto: &'a str,
    tipo: Tipo,
    pequeno: bool,
    ativo: bool,
    ocupado: bool,
    largura: Option<f32>,
}

pub fn btn(texto: &str) -> Btn<'_> {
    Btn {
        texto,
        tipo: Tipo::Normal,
        pequeno: false,
        ativo: true,
        ocupado: false,
        largura: None,
    }
}

impl<'a> Btn<'a> {
    pub fn primario(mut self) -> Self {
        self.tipo = Tipo::Primario;
        self
    }
    pub fn perigo(mut self) -> Self {
        self.tipo = Tipo::Perigo;
        self
    }
    pub fn link(mut self) -> Self {
        self.tipo = Tipo::Link;
        self
    }
    pub fn pequeno(mut self) -> Self {
        self.pequeno = true;
        self
    }
    pub fn ativo(mut self, v: bool) -> Self {
        self.ativo = v;
        self
    }
    pub fn ocupado(mut self, v: bool) -> Self {
        self.ocupado = v;
        self
    }
    pub fn largura(mut self, w: f32) -> Self {
        self.largura = Some(w);
        self
    }

    pub fn mostrar(self, ui: &mut Ui, pal: &Palette) -> Response {
        let tam = if self.pequeno { 13.0 } else { 14.0 };
        let cor_txt = match self.tipo {
            Tipo::Normal => pal.text,
            Tipo::Primario => pal.accent_text,
            Tipo::Perigo => pal.danger,
            Tipo::Link => pal.accent,
        };
        // (fundo, borda) em repouso, com o mouse em cima e pressionado.
        let nenhum = Color32::TRANSPARENT;
        let estados = match self.tipo {
            Tipo::Normal => [
                (pal.surface, pal.border_strong),
                (pal.surface2, pal.accent),
                (pal.accent_soft, pal.accent),
            ],
            Tipo::Primario => [
                (pal.accent, pal.accent),
                (pal.accent_hover, pal.accent_hover),
                (pal.accent_press, pal.accent_press),
            ],
            Tipo::Perigo => [
                (nenhum, pal.danger),
                (pal.danger_soft, pal.danger),
                (pal.danger_soft, pal.danger),
            ],
            Tipo::Link => [
                (nenhum, nenhum),
                (pal.accent_soft, nenhum),
                (pal.accent_soft, nenhum),
            ],
        };
        let mut texto = RichText::new(self.texto)
            .size(tam)
            .color(cor_txt)
            .family(negrito());
        if self.tipo == Tipo::Link {
            texto = texto.underline();
        }
        let espaco = if self.ocupado { "      " } else { "" };
        let rotulo = if self.ocupado {
            RichText::new(format!("{espaco}{}", self.texto))
                .size(tam)
                .color(cor_txt)
                .family(negrito())
        } else {
            texto
        };
        let habilitado = self.ativo && !self.ocupado;
        let mut b = Button::new(rotulo).corner_radius(CornerRadius::same(RAIO_PEQ));
        if let Some(w) = self.largura {
            b = b.min_size(vec2(w, 0.0));
        }
        let resp = ui
            .scope(|ui| {
                {
                    let v = ui.visuals_mut();
                    for (w, (fundo, borda)) in [
                        &mut v.widgets.inactive,
                        &mut v.widgets.hovered,
                        &mut v.widgets.active,
                    ]
                    .into_iter()
                    .zip(estados)
                    {
                        w.weak_bg_fill = fundo;
                        w.bg_fill = fundo;
                        w.bg_stroke = Stroke::new(1.0, borda);
                    }
                }
                if self.pequeno {
                    ui.spacing_mut().button_padding = vec2(11.0, 4.0);
                    ui.spacing_mut().interact_size.y = 26.0;
                }
                if self.tipo == Tipo::Link {
                    ui.spacing_mut().button_padding = vec2(6.0, 3.0);
                }
                // Ocupado: sem clique, mas sem o aspecto apagado de "desabilitado".
                ui.add_enabled(
                    self.ativo || self.ocupado,
                    b.sense(if habilitado {
                        Sense::click()
                    } else {
                        Sense::hover()
                    }),
                )
            })
            .inner;
        if self.ocupado {
            girar(
                ui,
                Rect::from_center_size(
                    Pos2::new(resp.rect.left() + 15.0, resp.rect.center().y),
                    vec2(13.0, 13.0),
                ),
                cor_txt,
            );
        }
        if resp.hovered() && habilitado {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    }
}

/// Indicador de carregamento girando.
pub fn girar(ui: &Ui, r: Rect, cor: Color32) {
    let t = ui.input(|i| i.time) as f32;
    let c = r.center();
    let raio = r.width().min(r.height()) / 2.0 - 1.0;
    let ini = t * 6.0;
    let pts: Vec<Pos2> = (0..=18)
        .map(|k| {
            let a = ini + k as f32 / 18.0 * 4.4;
            c + vec2(a.cos(), a.sin()) * raio
        })
        .collect();
    ui.painter().add(Shape::line(pts, Stroke::new(2.0, cor)));
    ui.ctx().request_repaint();
}

/// Etiqueta de estado (chip): "OK", "Falta", "Em andamento"...
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cor {
    Neutro,
    Rodando,
    Ok,
    Aviso,
    Erro,
}

pub fn chip(ui: &mut Ui, pal: &Palette, texto: &str, cor: Cor) {
    let (fg, bg) = match cor {
        Cor::Neutro => (pal.muted, pal.surface2),
        Cor::Rodando => (pal.accent, pal.accent_soft),
        Cor::Ok => (pal.ok, pal.ok_soft),
        Cor::Aviso => (pal.warn, pal.warn_soft),
        Cor::Erro => (pal.danger, pal.danger_soft),
    };
    Frame::new()
        .fill(bg)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(9, 2))
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(texto).size(12.0).color(fg).family(negrito()))
                    .selectable(false),
            );
        });
}

/// Cartao com borda e cantos arredondados.
pub fn cartao<R>(ui: &mut Ui, pal: &Palette, f: impl FnOnce(&mut Ui) -> R) -> R {
    let r = Frame::new()
        .fill(pal.surface)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(RAIO))
        .inner_margin(Margin::symmetric(20, 18))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            f(ui)
        })
        .inner;
    ui.add_space(6.0);
    r
}

/// Cartao sem margem interna (para tabelas que vao de ponta a ponta).
pub fn cartao_rente<R>(ui: &mut Ui, pal: &Palette, f: impl FnOnce(&mut Ui) -> R) -> R {
    let r = Frame::new()
        .fill(pal.surface)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(RAIO))
        .inner_margin(Margin::ZERO)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            f(ui)
        })
        .inner;
    ui.add_space(6.0);
    r
}

pub fn titulo2(ui: &mut Ui, texto: &str) {
    ui.add(egui::Label::new(RichText::new(texto).size(14.5).family(negrito())).selectable(false));
}

pub fn mudo(ui: &mut Ui, pal: &Palette, texto: &str) {
    ui.add(egui::Label::new(RichText::new(texto).color(pal.muted)));
}

pub fn mudo_peq(ui: &mut Ui, pal: &Palette, texto: &str) {
    ui.add(egui::Label::new(
        RichText::new(texto).size(12.5).color(pal.muted),
    ));
}

pub fn cabecalho_pagina(ui: &mut Ui, pal: &Palette, titulo: &str, sub: &str) {
    ui.add(egui::Label::new(RichText::new(titulo).size(24.0).family(negrito())).selectable(false));
    ui.add_space(2.0);
    ui.add(egui::Label::new(RichText::new(sub).color(pal.muted)));
    ui.add_space(14.0);
}

/// Rotulo de campo (acima de um input).
pub fn rotulo(ui: &mut Ui, texto: &str) {
    ui.add(egui::Label::new(RichText::new(texto).size(13.0).family(negrito())).selectable(false));
}

/// Barra de progresso. `frac = None` mostra a barra indeterminada (animada).
pub fn barra(
    ui: &mut Ui,
    pal: &Palette,
    frac: Option<f32>,
    largura: f32,
    altura: f32,
    cor: Option<Color32>,
) {
    let (r, _) = ui.allocate_exact_size(vec2(largura, altura), Sense::hover());
    let p = ui.painter();
    let raio = CornerRadius::same((altura / 2.0).ceil() as u8);
    p.rect_filled(r, raio, pal.border);
    let cor = cor.unwrap_or(pal.accent);
    match frac {
        Some(f) => {
            let w = (r.width() * f.clamp(0.0, 1.0)).max(if f > 0.0 { altura } else { 0.0 });
            if w > 0.0 {
                p.rect_filled(Rect::from_min_size(r.min, vec2(w, altura)), raio, cor);
            }
        }
        None => {
            let t = ui.input(|i| i.time) as f32;
            let seg = r.width() * 0.32;
            let x = ((t * 0.9) % 1.0) * (r.width() + seg) - seg;
            let a = (r.min.x + x).max(r.min.x);
            let b = (r.min.x + x + seg).min(r.max.x);
            if b > a {
                p.rect_filled(
                    Rect::from_min_max(Pos2::new(a, r.min.y), Pos2::new(b, r.max.y)),
                    raio,
                    cor,
                );
            }
            ui.ctx().request_repaint();
        }
    }
}

/// Interruptor com rotulo (como o `.switch-row` da interface HTML).
pub fn interruptor(ui: &mut Ui, pal: &Palette, valor: &mut bool, texto: &str) -> Response {
    let resp = ui
        .horizontal(|ui| {
            let (r, resp) = ui.allocate_exact_size(vec2(34.0, 20.0), Sense::click());
            let cor = if *valor {
                pal.accent
            } else {
                pal.border_strong
            };
            ui.painter().rect_filled(r, CornerRadius::same(10), cor);
            let cx = if *valor {
                r.right() - 10.0
            } else {
                r.left() + 10.0
            };
            ui.painter().circle_filled(
                Pos2::new(cx, r.center().y),
                7.0,
                if *valor {
                    pal.accent_text
                } else {
                    Color32::WHITE
                },
            );
            let t = ui.add(
                egui::Label::new(texto)
                    .selectable(false)
                    .sense(Sense::click()),
            );
            if resp.clicked() || t.clicked() {
                *valor = !*valor;
            }
            if resp.hovered() || t.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            resp
        })
        .inner;
    resp
}

/// Controle segmentado; devolve o indice clicado.
pub fn segmentado(ui: &mut Ui, pal: &Palette, atual: usize, itens: &[&str]) -> Option<usize> {
    let mut clicado = None;
    Frame::new()
        .fill(pal.surface2)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(RAIO_PEQ))
        .inner_margin(Margin::same(3))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.horizontal_wrapped(|ui| {
                for (i, t) in itens.iter().enumerate() {
                    let on = i == atual;
                    let b = Button::new(
                        RichText::new(*t)
                            .size(13.0)
                            .color(if on { pal.accent } else { pal.muted })
                            .family(negrito()),
                    )
                    .fill(if on {
                        pal.accent_soft
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius::same(5));
                    let r = ui.scope(|ui| {
                        ui.spacing_mut().button_padding = vec2(12.0, 4.0);
                        ui.spacing_mut().interact_size.y = 26.0;
                        ui.add(b)
                    });
                    if r.inner.clicked() {
                        clicado = Some(i);
                    }
                    if r.inner.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }
            });
        });
    clicado
}

/// Cartao de escolha (modo), com o circulo de selecao. Devolve true se clicado.
pub fn cartao_modo(
    ui: &mut Ui,
    pal: &Palette,
    largura: f32,
    ligado: bool,
    titulo: &str,
    descricao: &str,
) -> bool {
    let (borda, fundo) = if ligado {
        (pal.accent, pal.accent_soft)
    } else {
        (pal.border, pal.surface)
    };
    let resp = Frame::new()
        .fill(fundo)
        .stroke(Stroke::new(1.0, borda))
        .corner_radius(CornerRadius::same(RAIO_PEQ))
        .inner_margin(Margin::symmetric(13, 11))
        .show(ui, |ui| {
            ui.set_width(largura - 28.0);
            ui.set_min_height(48.0);
            ui.horizontal_top(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                let c = r.center() + vec2(0.0, 2.0);
                ui.painter().circle_stroke(
                    c,
                    7.0,
                    Stroke::new(
                        1.5,
                        if ligado {
                            pal.accent
                        } else {
                            pal.border_strong
                        },
                    ),
                );
                if ligado {
                    ui.painter().circle_filled(c, 3.6, pal.accent);
                }
                ui.vertical(|ui| {
                    ui.add(
                        egui::Label::new(RichText::new(titulo).family(negrito())).selectable(false),
                    );
                    ui.add(
                        egui::Label::new(RichText::new(descricao).size(12.5).color(pal.muted))
                            .selectable(false),
                    );
                });
            });
        })
        .response;
    let resp = ui.interact(resp.rect, resp.id.with("clique"), Sense::click());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Grade de cartoes de modo em duas colunas. Devolve a chave clicada.
pub fn grade_modos(
    ui: &mut Ui,
    pal: &Palette,
    itens: &[(String, String, String)],
    atual: &str,
) -> Option<String> {
    let gap = 10.0;
    let w = ((ui.available_width() - gap) / 2.0).floor();
    let mut clicado = None;
    for par in itens.chunks(2) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (k, t, d) in par {
                if cartao_modo(ui, pal, w, k == atual, t, d) {
                    clicado = Some(k.clone());
                }
            }
        });
    }
    clicado
}

/// Area de soltar arquivos (borda tracejada). `sobre` destaca durante o arrasto.
pub fn area_soltar(
    ui: &mut Ui,
    pal: &Palette,
    sobre: bool,
    esbelta: bool,
    titulo: &str,
    sub: &str,
    extra: &str,
) -> Response {
    let altura = if esbelta { 56.0 } else { 86.0 };
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), altura), Sense::click());
    let destaque = sobre || resp.hovered();
    let (borda, fundo) = if destaque {
        (pal.accent, pal.accent_soft)
    } else {
        (pal.border_strong, pal.surface2)
    };
    ui.painter()
        .rect_filled(r, CornerRadius::same(RAIO_PEQ), fundo);
    let cantos = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    ui.painter().extend(Shape::dashed_line(
        &cantos,
        Stroke::new(1.4, borda),
        6.0,
        4.0,
    ));
    let ic = Rect::from_center_size(Pos2::new(r.left() + 34.0, r.center().y), vec2(22.0, 22.0));
    crate::icons::desenhar(
        ui.painter(),
        ic,
        crate::icons::Icone::Enviar,
        if destaque { pal.accent } else { pal.muted },
    );
    let x = r.left() + 62.0;
    let y = r.center().y;
    let g1 =
        ui.painter()
            .layout_no_wrap(titulo.to_string(), FontId::new(14.0, negrito()), pal.text);
    let w1 = g1.size().x;
    let sub_g = ui.painter().layout_no_wrap(
        sub.to_string(),
        FontId::new(14.0, egui::FontFamily::Proportional),
        pal.muted,
    );
    let dy = if extra.is_empty() { 0.0 } else { 9.0 };
    ui.painter()
        .galley(Pos2::new(x, y - g1.size().y / 2.0 - dy), g1, pal.text);
    ui.painter().galley(
        Pos2::new(x + w1 + 6.0, y - sub_g.size().y / 2.0 - dy),
        sub_g,
        pal.muted,
    );
    if !extra.is_empty() {
        ui.painter().text(
            Pos2::new(x, y + 11.0),
            Align2::LEFT_CENTER,
            extra,
            FontId::new(12.5, egui::FontFamily::Proportional),
            pal.muted,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// Campo de texto de uma linha com borda propria (acende ao receber foco).
pub fn campo_texto(
    ui: &mut Ui,
    pal: &Palette,
    texto: &mut String,
    dica: &str,
    largura: f32,
    editavel: bool,
) -> Response {
    let id = ui.next_auto_id();
    let (r, _) = ui.allocate_exact_size(vec2(largura, 32.0), Sense::hover());
    let foco = ui.memory(|m| m.has_focus(id));
    ui.painter().rect_filled(
        r,
        CornerRadius::same(RAIO_PEQ),
        if editavel { pal.surface } else { pal.surface2 },
    );
    ui.painter().rect_stroke(
        r,
        CornerRadius::same(RAIO_PEQ),
        Stroke::new(
            if foco { 1.5 } else { 1.0 },
            if foco { pal.accent } else { pal.border_strong },
        ),
        StrokeKind::Inside,
    );
    let mut filho = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(r.shrink2(vec2(2.0, 0.0)))
            .layout(Layout::left_to_right(Align::Center)),
    );
    filho.add_sized(
        [r.width() - 4.0, 30.0],
        egui::TextEdit::singleline(texto)
            .id(id)
            .frame(egui::Frame::NONE)
            .hint_text(dica)
            .align(egui::Align2::LEFT_CENTER)
            .interactive(editavel)
            .margin(Margin::symmetric(8, 6)),
    )
}

/// Linha de campo com caminho (somente leitura ou editavel) e botao ao lado. Devolve true se o botao foi clicado.
pub fn campo_caminho(
    ui: &mut Ui,
    pal: &Palette,
    texto: &mut String,
    dica: &str,
    botao: &str,
    editavel: bool,
    ativo: bool,
) -> bool {
    let mut clicou = false;
    ui.horizontal(|ui| {
        let w_btn = 118.0;
        let w = (ui.available_width() - w_btn - ui.spacing().item_spacing.x).max(100.0);
        campo_texto(ui, pal, texto, dica, w, editavel);
        clicou = btn(botao)
            .ativo(ativo)
            .largura(w_btn)
            .mostrar(ui, pal)
            .clicked();
    });
    clicou
}

pub fn entrada_simples(
    ui: &mut Ui,
    pal: &Palette,
    texto: &mut String,
    dica: &str,
    largura: f32,
) -> Response {
    campo_texto(ui, pal, texto, dica, largura, true)
}
/// Linha de aviso (banner) com fundo suave.
pub fn faixa<R>(ui: &mut Ui, fundo: Color32, borda: Color32, f: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .fill(fundo)
        .stroke(Stroke::new(1.0, borda))
        .corner_radius(CornerRadius::same(RAIO_PEQ))
        .inner_margin(Margin::symmetric(14, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            f(ui)
        })
        .inner
}

/// Alinha o conteudo a direita numa linha.
pub fn a_direita<R>(ui: &mut Ui, f: impl FnOnce(&mut Ui) -> R) -> R {
    ui.with_layout(Layout::right_to_left(Align::Center), f)
        .inner
}

/// Texto monoespaco colorido (linhas de log).
pub fn mono(texto: impl Into<String>, cor: Color32) -> RichText {
    RichText::new(texto.into())
        .monospace()
        .size(12.5)
        .color(cor)
}

/// Texto alinhado a esquerda numa coluna de largura fixa (ao contrario de `add_sized`, que centraliza).
pub fn texto_fixo(ui: &mut Ui, largura: f32, altura: f32, texto: RichText) {
    ui.allocate_ui_with_layout(
        vec2(largura, altura),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_size(vec2(largura, altura));
            ui.add(egui::Label::new(texto).truncate());
        },
    );
}
