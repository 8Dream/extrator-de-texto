//! Moldura da janela: menu lateral, rodape de execucao, barra de confirmacao, avisos rapidos.

use crate::app::*;
use crate::bus::Tag;
use crate::icons::{self, Icone};
use crate::theme::negrito;
use crate::widgets::*;
use eframe::egui::{
    self, vec2, Align2, CornerRadius, FontId, Frame, Margin, Panel, Pos2, Rect, RichText,
    ScrollArea, Sense, Stroke, Ui,
};

/// O Diagnostico nao tem item proprio: e atalho dentro de Inicio, Converter e Ajuda.
const ITENS_MENU: [(Pagina, Icone, &str); 8] = [
    (Pagina::Inicio, Icone::Casa, "Início"),
    (Pagina::Converter, Icone::Converter, "Converter"),
    (Pagina::Transcricao, Icone::Microfone, "Transcrição"),
    (Pagina::Lote, Icone::Lote, "Lote"),
    (Pagina::Execucoes, Icone::Execucoes, "Execuções"),
    (Pagina::Log, Icone::Log, "Log"),
    (Pagina::Configuracoes, Icone::Ajustes, "Configurações"),
    (Pagina::Ajuda, Icone::Ajuda, "Ajuda e instalação"),
];

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Arquivos soltos chegam no primeiro passe do quadro (antes do desenho).
        self.tratar_soltos(ctx);
    }
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.tick(&ctx);
        let pal = self.pal;
        Panel::left("menu")
            .exact_size(236.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(pal.surface)
                    .stroke(Stroke::new(1.0, pal.border))
                    .inner_margin(Margin {
                        left: 12,
                        right: 12,
                        top: 18,
                        bottom: 14,
                    }),
            )
            .show(ui, |ui| self.ui_menu(ui));
        Panel::bottom("rodape")
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(pal.surface)
                    .stroke(Stroke::new(1.0, pal.border))
                    .inner_margin(Margin::ZERO),
            )
            .show(ui, |ui| self.ui_rodape(ui));
        egui::CentralPanel::default()
            .frame(Frame::new().fill(pal.bg))
            .show(ui, |ui| {
                self.ui_conteudo(ui);
            });
        self.ui_avisos(&ctx);
        self.passo_captura(&ctx);
    }
}

impl App {
    fn ui_menu(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
            if let Some(t) = &self.logo {
                ui.painter().image(
                    t.id(),
                    r,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            ui.add_space(3.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add(
                    egui::Label::new(
                        RichText::new("Extrator de Texto")
                            .size(15.0)
                            .family(negrito()),
                    )
                    .selectable(false),
                );
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("v{}", self.versao))
                            .size(12.0)
                            .color(pal.muted),
                    )
                    .selectable(false),
                );
            });
        });
        ui.add_space(16.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        for (i, (pg, ic, rot)) in ITENS_MENU.iter().enumerate() {
            if i == 5 {
                ui.add_space(6.0);
                let (r, _) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                ui.painter().rect_filled(r, 0.0, pal.border);
                ui.add_space(6.0);
            }
            let (r, resp) =
                ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
            // O Diagnostico e filho do Converter: o item do Converter continua marcado.
            let ativo = self.pagina == *pg
                || (*pg == Pagina::Converter && self.pagina == Pagina::Diagnostico);
            if ativo {
                ui.painter()
                    .rect_filled(r, CornerRadius::same(7), pal.accent_soft);
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(r, CornerRadius::same(7), pal.surface2);
            }
            let cor = if ativo {
                pal.accent
            } else if resp.hovered() {
                pal.text
            } else {
                pal.muted
            };
            icons::desenhar(
                ui.painter(),
                Rect::from_center_size(Pos2::new(r.left() + 20.0, r.center().y), vec2(18.0, 18.0)),
                *ic,
                cor,
            );
            ui.painter().text(
                Pos2::new(r.left() + 40.0, r.center().y),
                Align2::LEFT_CENTER,
                *rot,
                FontId::new(14.0, negrito()),
                cor,
            );
            // selos: contador de execucoes, ponto de transcricao e alerta da ajuda
            let direita = Pos2::new(r.right() - 12.0, r.center().y);
            match pg {
                Pagina::Execucoes => {
                    let n = self.exec.lista.rodando + self.exec.lista.na_fila;
                    if n > 0 {
                        let t = n.to_string();
                        let w = (t.len() as f32 * 8.0 + 14.0).max(20.0);
                        let pill = Rect::from_center_size(
                            Pos2::new(r.right() - 12.0 - w / 2.0, r.center().y),
                            vec2(w, 20.0),
                        );
                        ui.painter()
                            .rect_filled(pill, CornerRadius::same(10), pal.accent);
                        ui.painter().text(
                            pill.center(),
                            Align2::CENTER_CENTER,
                            t,
                            FontId::new(11.5, negrito()),
                            pal.accent_text,
                        );
                    }
                }
                Pagina::Transcricao if self.tr.rodando => {
                    ui.painter().circle_filled(direita, 4.0, pal.accent);
                }
                Pagina::Ajuda if self.selo_ajuda() => {
                    ui.painter().circle_filled(direita, 4.0, pal.warn);
                }
                _ => {}
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.mostrar(*pg);
            }
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.add(egui::Label::new(
                RichText::new("Tudo é processado neste computador. Nada é enviado para fora.")
                    .size(12.0)
                    .color(pal.muted),
            ));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let largura = ui.available_width();
                // alterna claro/escuro
                let (r, resp) = ui.allocate_exact_size(vec2(largura - 40.0, 36.0), Sense::click());
                if resp.hovered() {
                    ui.painter()
                        .rect_filled(r, CornerRadius::same(7), pal.surface2);
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let ic = if self.escuro { Icone::Sol } else { Icone::Lua };
                icons::desenhar(
                    ui.painter(),
                    Rect::from_center_size(
                        Pos2::new(r.left() + 20.0, r.center().y),
                        vec2(18.0, 18.0),
                    ),
                    ic,
                    pal.muted,
                );
                ui.painter().text(
                    Pos2::new(r.left() + 40.0, r.center().y),
                    Align2::LEFT_CENTER,
                    if self.escuro {
                        "Tema claro"
                    } else {
                        "Tema escuro"
                    },
                    FontId::new(14.0, egui::FontFamily::Proportional),
                    pal.muted,
                );
                if resp
                    .on_hover_text("Alternar entre tema claro e escuro")
                    .clicked()
                {
                    self.alternar_tema();
                }
                // paleta: leva a Configuracoes > Aparencia
                let (r, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
                let ativo = self.pagina == Pagina::Configuracoes;
                if resp.hovered() || ativo {
                    ui.painter().rect_filled(
                        r,
                        CornerRadius::same(7),
                        if ativo { pal.accent_soft } else { pal.surface2 },
                    );
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                icons::desenhar(
                    ui.painter(),
                    Rect::from_center_size(r.center(), vec2(18.0, 18.0)),
                    Icone::Paleta,
                    if ativo { pal.accent } else { pal.muted },
                );
                if resp.on_hover_text("Cores e aparência").clicked() {
                    self.abrir_aparencia();
                }
            });
            ui.add_space(4.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, pal.border);
        });
    }

    /// Rodape: barra de confirmacao inline, status, progresso, contador "N rodando" e o registro da sessao.
    fn ui_rodape(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        if let Some(c) = self.confirma.clone() {
            self.ui_confirma(ui, &c);
        }
        Frame::new()
            .inner_margin(Margin::symmetric(16, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                    let cor = match self.tom {
                        Tom::Ocupado => pal.accent,
                        Tom::Erro => pal.danger,
                        Tom::Aviso => pal.warn,
                        Tom::Normal => pal.ok,
                    };
                    ui.painter().circle_filled(r.center(), 4.5, cor);
                    if self.tom == Tom::Ocupado {
                        ui.ctx().request_repaint();
                    }
                    let reservado = 330.0;
                    let largura = (ui.available_width() - reservado).max(120.0);
                    ui.allocate_ui_with_layout(
                        vec2(largura, 20.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(RichText::new(&self.status).color(
                                    if self.tom == Tom::Erro {
                                        pal.danger
                                    } else {
                                        pal.text
                                    },
                                ))
                                .truncate(),
                            );
                        },
                    );
                    a_direita(ui, |ui| {
                        let rotulo = if self.dock_aberto {
                            "Ocultar log"
                        } else {
                            "Ver log"
                        };
                        if btn(rotulo).link().pequeno().mostrar(ui, &pal).clicked() {
                            self.dock_aberto = !self.dock_aberto;
                        }
                        let n = self.exec.lista.rodando;
                        let f = self.exec.lista.na_fila;
                        if n + f > 0 {
                            let t = format!(
                                "{n} rodando{}",
                                if f > 0 {
                                    format!(" · {f} na fila")
                                } else {
                                    String::new()
                                }
                            );
                            if btn(&t)
                                .link()
                                .pequeno()
                                .mostrar(ui, &pal)
                                .on_hover_text("Abrir a tela Execuções")
                                .clicked()
                            {
                                self.mostrar(Pagina::Execucoes);
                            }
                        }
                        match self.prog {
                            Prog::Oculto => {}
                            Prog::Indeterminado => barra(ui, &pal, None, 150.0, 8.0, None),
                            Prog::Fracao(f) => {
                                ui.add(egui::Label::new(
                                    RichText::new(format!("{}%", (f * 100.0).round() as i32))
                                        .size(12.5)
                                        .color(pal.muted),
                                ));
                                barra(ui, &pal, Some(f), 150.0, 8.0, None);
                            }
                        }
                    });
                });
            });
        if self.dock_aberto {
            ui.add_space(0.0);
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().rect_filled(r, 0.0, pal.border);
            Frame::new()
                .inner_margin(Margin::symmetric(16, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        mudo_peq(ui, &pal, "Registro da sessão");
                        a_direita(ui, |ui| {
                            if btn("Limpar").link().pequeno().mostrar(ui, &pal).clicked() {
                                self.dock_log.clear();
                            }
                        });
                    });
                    Frame::new()
                        .fill(pal.surface2)
                        .stroke(Stroke::new(1.0, pal.border))
                        .corner_radius(CornerRadius::same(7))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.set_height(150.0);
                            let h = 16.0;
                            let n = self.dock_log.len();
                            ScrollArea::both()
                                .id_salt("dock_log")
                                .auto_shrink([false, false])
                                .stick_to_bottom(true)
                                .show_rows(ui, h, n, |ui, faixa| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    for i in faixa {
                                        if let Some(l) = self.dock_log.get(i) {
                                            let erro = l.contains("ERRO:") || l.contains("FALHA");
                                            ui.add(
                                                egui::Label::new(mono(
                                                    l.as_str(),
                                                    if erro { pal.danger } else { pal.text },
                                                ))
                                                .wrap_mode(egui::TextWrapMode::Extend),
                                            );
                                        }
                                    }
                                });
                        });
                });
        }
    }

    fn ui_confirma(&mut self, ui: &mut Ui, c: &Confirma) {
        let pal = self.pal;
        let (fundo, borda) = if c.perigo {
            (pal.danger_soft, pal.danger)
        } else {
            (pal.accent_soft, pal.accent)
        };
        let mut decisao: Option<bool> = None;
        Frame::new()
            .fill(fundo)
            .stroke(Stroke::new(1.0, borda))
            .inner_margin(Margin::symmetric(16, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let total = ui.available_width();
                    ui.vertical(|ui| {
                        ui.set_width((total - 340.0).max(200.0));
                        ui.add(
                            egui::Label::new(RichText::new(&c.titulo).family(negrito()))
                                .selectable(false),
                        );
                        ui.add(egui::Label::new(RichText::new(&c.corpo).size(13.0)));
                    });
                    if btn("Cancelar").mostrar(ui, &pal).clicked() {
                        decisao = Some(false);
                    }
                    let mut b = btn(&c.ok);
                    b = if c.perigo { b.perigo() } else { b.primario() };
                    if b.mostrar(ui, &pal).clicked() {
                        decisao = Some(true);
                    }
                });
            });
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            decisao = Some(false);
        }
        match decisao {
            Some(true) => {
                self.confirma = None;
                self.executar_confirmacao(c.acao.clone());
            }
            Some(false) => {
                self.confirma = None;
            }
            None => {}
        }
    }

    fn executar_confirmacao(&mut self, a: AcaoConfirma) {
        match a {
            AcaoConfirma::ConverterAssinados => self.iniciar_conversao(true),
            AcaoConfirma::IniciarLote => self.depois_de_confirmar_lote(),
            AcaoConfirma::LoteAssinados => self.iniciar_lote_real(true),
            AcaoConfirma::LimparExecucoes => {
                self.iniciar_acao("exec_limpar", "Limpando…");
                self.chamar(Tag::ExecLimpar, "exec_limpar_concluidas", vec![]);
            }
            AcaoConfirma::RestaurarPadroes => {
                self.iniciar_acao("cfg_restaurar", "Restaurando…");
                self.chamar(Tag::RestaurarConfig, "restore_settings", vec![]);
            }
            AcaoConfirma::LimparDados => {
                self.iniciar_acao("limpar", "Limpando…");
                self.chamar(Tag::LimparDados, "clear_data", vec![]);
            }
            AcaoConfirma::InstalarFaltantes => {
                self.iniciar_acao("instalar", "Instalando…");
                self.chamar(Tag::InstalarFaltantes, "install_missing", vec![]);
            }
            AcaoConfirma::FecharComTarefas => self.comecar_encerramento(),
        }
    }

    /// Coluna centralizada (largura maxima 940) com rolagem.
    pub fn coluna(&mut self, ui: &mut Ui, id: &str, f: impl FnOnce(&mut App, &mut Ui)) {
        let mut area = ScrollArea::vertical()
            .id_salt(id)
            .auto_shrink([false, false]);
        if self.rolar_topo && id == "configuracoes" {
            self.rolar_topo = false;
            area = area.vertical_scroll_offset(0.0);
        }
        area.show(ui, |ui| {
            let largura = ui.available_width();
            let lado = ((largura - 940.0) / 2.0).max(32.0).floor() as i8;
            Frame::new()
                .inner_margin(Margin {
                    left: lado,
                    right: lado,
                    top: 28,
                    bottom: 24,
                })
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    f(self, ui);
                });
        });
    }

    /// Pagina com barra de acoes fixa embaixo (resumo a esquerda, botoes a direita).
    pub fn pagina_com_barra(
        &mut self,
        ui: &mut Ui,
        id: &str,
        corpo: impl FnOnce(&mut App, &mut Ui),
        barra_acoes: impl FnOnce(&mut App, &mut Ui),
    ) {
        let pal = self.pal;
        Panel::bottom(format!("barra_{id}"))
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(pal.surface)
                    .stroke(Stroke::new(1.0, pal.border))
                    .inner_margin(Margin::symmetric(32, 12)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| barra_acoes(self, ui));
            });
        self.coluna(ui, id, corpo);
    }

    fn ui_conteudo(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        if let Some(m) = self.faixa_motor.clone() {
            let falha = matches!(self.estado_motor, crate::bus::EstadoMotor::Falhou(_));
            Frame::new()
                .inner_margin(Margin::symmetric(32, 10))
                .show(ui, |ui| {
                    faixa(ui, pal.danger_soft, pal.danger, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if !falha {
                                let (r, _) =
                                    ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                                girar(ui, r, pal.danger);
                            }
                            ui.add(egui::Label::new(RichText::new(&m).color(pal.danger)));
                            if falha && btn("Tentar de novo").pequeno().mostrar(ui, &pal).clicked()
                            {
                                self.motor.reiniciar_do_zero();
                            }
                        });
                    });
                });
        }
        if self.boot.is_none() {
            self.ui_sem_boot(ui);
            return;
        }
        match self.pagina {
            Pagina::Inicio => self.ui_inicio(ui),
            Pagina::Converter => self.ui_converter(ui),
            Pagina::Lote => self.ui_lote(ui),
            Pagina::Transcricao => self.ui_transcricao(ui),
            Pagina::Diagnostico => self.ui_diagnostico(ui),
            Pagina::Execucoes => self.ui_execucoes(ui),
            Pagina::Log => self.ui_log(ui),
            Pagina::Configuracoes => self.ui_configuracoes(ui),
            Pagina::Ajuda => self.ui_ajuda(ui),
        }
    }

    fn ui_sem_boot(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let falha = self.falha_boot.clone();
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            match falha {
                Some(m) => {
                    ui.add(egui::Label::new(
                        RichText::new("Não foi possível iniciar")
                            .size(22.0)
                            .family(negrito()),
                    ));
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(RichText::new(m).color(pal.muted)));
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.add_space((ui.available_width() / 2.0 - 140.0).max(0.0));
                        if btn("Tentar de novo").primario().mostrar(ui, &pal).clicked() {
                            self.motor.reiniciar_do_zero();
                        }
                        if btn("Reparar instalação").mostrar(ui, &pal).clicked() {
                            crate::reparar::abrir_reparo();
                        }
                    });
                }
                None => {
                    let (r, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                    girar(ui, r, pal.accent);
                    ui.add_space(8.0);
                    ui.add(egui::Label::new(
                        RichText::new("Iniciando o programa…").color(pal.muted),
                    ));
                }
            }
        });
    }

    fn ui_avisos(&mut self, ctx: &egui::Context) {
        if self.avisos.is_empty() {
            return;
        }
        let pal = self.pal;
        let mut ver: Option<(String, i64)> = None;
        #[allow(deprecated)]
        let id_avisos = egui::Id::new("avisos");
        egui::Area::new(id_avisos)
            .anchor(Align2::RIGHT_BOTTOM, vec2(-18.0, -124.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_max_width(400.0);
                for a in self.avisos.iter().rev() {
                    let borda = match a.tipo {
                        TipoAviso::Erro => pal.danger,
                        TipoAviso::Aviso => pal.warn,
                        TipoAviso::Info => pal.border_strong,
                    };
                    Frame::new()
                        .fill(pal.surface)
                        .stroke(Stroke::new(1.0, borda))
                        .corner_radius(CornerRadius::same(8))
                        .inner_margin(Margin::symmetric(14, 10))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 3],
                            blur: 12,
                            spread: 0,
                            color: egui::Color32::from_black_alpha(60),
                        })
                        .show(ui, |ui| {
                            ui.set_max_width(372.0);
                            ui.add(egui::Label::new(RichText::new(&a.texto).color(
                                if a.tipo == TipoAviso::Erro {
                                    pal.danger
                                } else {
                                    pal.text
                                },
                            )));
                            if let Some(l) = &a.link_log {
                                if btn("Ver no log")
                                    .link()
                                    .pequeno()
                                    .mostrar(ui, &pal)
                                    .clicked()
                                {
                                    ver = Some(l.clone());
                                }
                            }
                        });
                    ui.add_space(6.0);
                }
            });
        if let Some((nome, off)) = ver {
            self.avisos.clear();
            self.ir_para_linha_log(nome, off);
        }
    }
}
