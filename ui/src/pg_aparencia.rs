//! Secao "Aparencia" da tela Configuracoes: tema, cor de destaque, tamanho do texto.
//! Tudo e desenhado dentro da janela (sem pop-up) e aplicado na hora.

use crate::app::*;
use crate::cores::{self, Rgb, DESTAQUES};
use crate::prefs::{Aparencia, Destaque, ModoTema, TamanhoTexto};
use crate::theme::{Palette, RAIO_PEQ};
use crate::widgets::*;
use eframe::egui::{
    self, vec2, Color32, CornerRadius, Frame, Margin, Pos2, Response, RichText, Sense, Stroke, Ui,
};

fn c32(c: Rgb) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Bolinha de cor (clicavel) com aro quando marcada.
fn bolinha_cor(
    ui: &mut Ui,
    pal: &Palette,
    cor: Rgb,
    sobre: Rgb,
    marcada: bool,
    dica: &str,
) -> Response {
    let (r, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
    let c = r.center();
    if marcada {
        ui.painter()
            .circle_stroke(c, 16.0, Stroke::new(2.0, pal.text));
    } else if resp.hovered() {
        ui.painter()
            .circle_stroke(c, 16.0, Stroke::new(1.5, pal.border_strong));
    }
    ui.painter().circle_filled(c, 12.5, c32(cor));
    if marcada {
        let tr = Stroke::new(2.2, c32(sobre));
        ui.painter().line_segment(
            [Pos2::new(c.x - 5.0, c.y), Pos2::new(c.x - 1.5, c.y + 4.0)],
            tr,
        );
        ui.painter().line_segment(
            [
                Pos2::new(c.x - 1.5, c.y + 4.0),
                Pos2::new(c.x + 5.5, c.y - 4.0),
            ],
            tr,
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(dica)
}

fn rgb_da(c: Color32) -> Rgb {
    [c.r(), c.g(), c.b()]
}

impl App {
    pub fn salvar_aparencia(&mut self) {
        crate::prefs::gravar(&self.dados_dir, &self.ap);
    }

    /// Vai para Configuracoes, na secao Aparencia (no topo).
    pub fn abrir_aparencia(&mut self) {
        self.mostrar(Pagina::Configuracoes);
        self.rolar_topo = true;
    }

    /// Alterna entre claro e escuro (o atalho do menu); sai do modo automatico.
    pub fn alternar_tema(&mut self) {
        self.ap.modo = if self.escuro {
            ModoTema::Claro
        } else {
            ModoTema::Escuro
        };
        self.salvar_aparencia();
    }

    fn definir_destaque(&mut self, d: Destaque) {
        if self.ap.destaque != d {
            self.ap.destaque = d;
            self.salvar_aparencia();
        }
    }

    pub fn card_aparencia(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        cartao(ui, &pal, |ui| {
            titulo2(ui, "Aparência");
            mudo_peq(
                ui,
                &pal,
                "As mudanças valem na hora e ficam salvas neste computador.",
            );
            ui.add_space(10.0);

            // ---- tema
            rotulo(ui, "Tema");
            let nomes: Vec<&str> = ModoTema::TODOS.iter().map(|m| m.rotulo()).collect();
            let atual = ModoTema::TODOS
                .iter()
                .position(|m| *m == self.ap.modo)
                .unwrap_or(2);
            if let Some(i) = segmentado(ui, &pal, atual, &nomes) {
                self.ap.modo = ModoTema::TODOS[i];
                self.salvar_aparencia();
            }
            if self.ap.modo == ModoTema::Auto {
                mudo_peq(
                    ui,
                    &pal,
                    &format!(
                        "Segue o tema do Windows (agora está {}).",
                        if self.sistema_escuro {
                            "escuro"
                        } else {
                            "claro"
                        }
                    ),
                );
            }
            ui.add_space(12.0);

            // ---- cor de destaque
            rotulo(ui, "Cor de destaque");
            let (sup, bg) = (rgb_da(pal.surface), rgb_da(pal.bg));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for (i, (_, nome, base)) in DESTAQUES.iter().enumerate() {
                    let d = cores::derivar(*base, self.escuro, sup, bg);
                    let marcada = self.ap.destaque == Destaque::Pronto(i);
                    if bolinha_cor(ui, &pal, d.destaque, d.sobre, marcada, nome).clicked() {
                        self.definir_destaque(Destaque::Pronto(i));
                        self.hex_texto = cores::hex(*base);
                    }
                }
                // personalizada: mostra a cor digitada ou um "+" quando ainda nao ha
                let (r, resp) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::click());
                let c = r.center();
                let custom = matches!(self.ap.destaque, Destaque::Personalizado(_));
                if let Destaque::Personalizado(base) = self.ap.destaque {
                    let d = cores::derivar(base, self.escuro, sup, bg);
                    ui.painter()
                        .circle_stroke(c, 16.0, Stroke::new(2.0, pal.text));
                    ui.painter().circle_filled(c, 12.5, c32(d.destaque));
                } else {
                    let cor = if resp.hovered() { pal.text } else { pal.muted };
                    ui.painter().circle_stroke(c, 12.5, Stroke::new(1.5, cor));
                    ui.painter().line_segment(
                        [Pos2::new(c.x - 5.0, c.y), Pos2::new(c.x + 5.0, c.y)],
                        Stroke::new(1.6, cor),
                    );
                    ui.painter().line_segment(
                        [Pos2::new(c.x, c.y - 5.0), Pos2::new(c.x, c.y + 5.0)],
                        Stroke::new(1.6, cor),
                    );
                }
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.on_hover_text("Cor personalizada").clicked() && !custom {
                    let base = self.ap.destaque.rgb();
                    self.definir_destaque(Destaque::Personalizado(base));
                    self.hex_texto = cores::hex(base);
                }
            });
            ui.add_space(6.0);
            self.linha_cor_personalizada(ui);
            ui.add_space(12.0);

            // ---- tamanho do texto
            rotulo(ui, "Tamanho do texto");
            let nomes: Vec<&str> = TamanhoTexto::TODOS.iter().map(|t| t.rotulo()).collect();
            let atual = TamanhoTexto::TODOS
                .iter()
                .position(|t| *t == self.ap.texto)
                .unwrap_or(1);
            if let Some(i) = segmentado(ui, &pal, atual, &nomes) {
                self.ap.texto = TamanhoTexto::TODOS[i];
                self.salvar_aparencia();
            }
            mudo_peq(
                ui,
                &pal,
                "Amplia ou reduz o texto e os elementos da janela.",
            );
            ui.add_space(12.0);

            // ---- pre-visualizacao
            rotulo(ui, "Pré-visualização");
            Frame::new()
                .fill(pal.surface2)
                .stroke(Stroke::new(1.0, pal.border))
                .corner_radius(CornerRadius::same(RAIO_PEQ))
                .inner_margin(Margin::symmetric(14, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        btn("Iniciar conversão").primario().mostrar(ui, &pal);
                        btn("Cancelar").mostrar(ui, &pal);
                        chip(ui, &pal, "2 rodando", Cor::Rodando);
                        chip(ui, &pal, "Concluída", Cor::Ok);
                        barra(ui, &pal, Some(0.6), 120.0, 8.0, None);
                        btn("Ver detalhes").link().mostrar(ui, &pal);
                    });
                });
            ui.add_space(10.0);
            if btn("Restaurar padrão")
                .pequeno()
                .mostrar(ui, &pal)
                .clicked()
            {
                self.ap = Aparencia::default();
                self.hex_texto = cores::hex(self.ap.destaque.rgb());
                self.seletor_aberto = false;
                self.salvar_aparencia();
            }
        });
    }

    /// Amostra, campo `#RRGGBB` com validacao ao digitar e seletor de cor dentro da janela.
    fn linha_cor_personalizada(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let (sup, bg) = (rgb_da(pal.surface), rgb_da(pal.bg));
        let mut erro: Option<&'static str> = None;
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
            let d = cores::derivar(self.ap.destaque.rgb(), self.escuro, sup, bg);
            ui.painter()
                .rect_filled(r, CornerRadius::same(RAIO_PEQ), c32(d.destaque));
            ui.painter().rect_stroke(
                r,
                CornerRadius::same(RAIO_PEQ),
                Stroke::new(1.0, pal.border_strong),
                egui::StrokeKind::Inside,
            );
            let resp = entrada_simples(ui, &pal, &mut self.hex_texto, "#RRGGBB", 130.0);
            if resp.lost_focus() {
                if let Some(n) = cores::normalizar_hex(&self.hex_texto) {
                    self.hex_texto = n;
                }
            }
            if resp.changed() {
                match cores::validar_hex(&self.hex_texto) {
                    Ok(c) => self.definir_destaque(Destaque::Personalizado(c)),
                    Err(m) => erro = Some(m),
                }
            } else if !self.hex_texto.trim().is_empty() {
                // Mantem a mensagem enquanto o texto continuar invalido.
                if let Err(m) = cores::validar_hex(&self.hex_texto) {
                    erro = Some(m);
                }
            }
            let rot = if self.seletor_aberto {
                "Ocultar seletor"
            } else {
                "Escolher no seletor"
            };
            if btn(rot).pequeno().mostrar(ui, &pal).clicked() {
                self.seletor_aberto = !self.seletor_aberto;
            }
        });
        if let Some(m) = erro {
            ui.add(egui::Label::new(
                RichText::new(m).size(12.5).color(pal.danger),
            ));
        } else if self.destaque_ajustado && matches!(self.ap.destaque, Destaque::Personalizado(_)) {
            mudo_peq(
                ui,
                &pal,
                "Ajustamos um pouco o tom para o texto continuar legível (contraste mínimo de 4,5 para 1).",
            );
        } else {
            mudo_peq(
                ui,
                &pal,
                "Digite um código como #0F63CF ou escolha no seletor.",
            );
        }
        if self.seletor_aberto {
            ui.add_space(4.0);
            let base = self.ap.destaque.rgb();
            let mut cor = c32(base);
            if egui::color_picker::color_picker_color32(
                ui,
                &mut cor,
                egui::color_picker::Alpha::Opaque,
            ) {
                let novo = rgb_da(cor);
                self.hex_texto = cores::hex(novo);
                self.definir_destaque(Destaque::Personalizado(novo));
            }
        }
    }
}
