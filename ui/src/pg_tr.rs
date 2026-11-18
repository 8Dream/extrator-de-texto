//! Tela Transcricao: fila, opcoes, progresso, modelos.

use crate::app::*;
use crate::bus::{Escolha, Tag};
use crate::fmt;
use crate::widgets::*;
use eframe::egui::{
    self, vec2, CornerRadius, Frame, Margin, RichText, ScrollArea, Sense, Stroke, Ui,
};
use serde_json::{json, Value};

/// Lista suspensa simples: devolve true se mudou.
pub fn combo(
    ui: &mut Ui,
    id: &str,
    atual: &mut String,
    opcoes: &[(String, String)],
    largura: f32,
) -> bool {
    let rotulo = opcoes
        .iter()
        .find(|(v, _)| v == atual)
        .map(|(_, l)| l.clone())
        .unwrap_or_else(|| atual.clone());
    let mut mudou = false;
    egui::ComboBox::from_id_salt(id)
        .width(largura)
        .selected_text(rotulo)
        .show_ui(ui, |ui| {
            for (v, l) in opcoes {
                if ui.selectable_label(atual == v, l).clicked() && atual != v {
                    *atual = v.clone();
                    mudou = true;
                }
            }
        });
    mudou
}

fn cor_status(s: StatusItem) -> Cor {
    match s {
        StatusItem::Pendente => Cor::Neutro,
        StatusItem::EmAndamento => Cor::Rodando,
        StatusItem::Concluido => Cor::Ok,
        StatusItem::Erro | StatusItem::Cancelado => Cor::Erro,
    }
}

impl App {
    pub fn adicionar_tr(&mut self, caminhos: Vec<String>) {
        let Some(b) = &self.boot else { return };
        let ok = b.ext.media.clone();
        let mut ignorados = 0;
        for p in caminhos {
            if !ok.contains(&fmt::ext_of(&p)) {
                ignorados += 1;
                continue;
            }
            if self.tr.itens.iter().any(|i| i.caminho == p) {
                continue;
            }
            let id = format!("t{}", self.tr.prox_id);
            self.tr.prox_id += 1;
            self.tr.itens.push(ItemTr {
                id,
                caminho: p,
                status: StatusItem::Pendente,
                interlocutores: None,
            });
        }
        if ignorados > 0 {
            self.aviso(
                &format!(
                    "{} (não é áudio/vídeo).",
                    fmt::plural(ignorados, "arquivo ignorado", "arquivos ignorados")
                ),
                TipoAviso::Aviso,
            );
        }
        self.atualizar_resumo_tr();
    }

    fn mover_tr(&mut self, d: i32) {
        let Some(sel) = self.tr.selecionado.clone() else {
            return;
        };
        let Some(i) = self.tr.itens.iter().position(|x| x.id == sel) else {
            return;
        };
        let j = i as i32 + d;
        if j < 0 || j as usize >= self.tr.itens.len() {
            return;
        }
        self.tr.itens.swap(i, j as usize);
    }

    fn iniciar_tr(&mut self) {
        let pendentes: Vec<Value> = self
            .tr
            .itens
            .iter()
            .filter(|i| i.status == StatusItem::Pendente)
            .map(|i| json!({"id": i.id, "path": i.caminho}))
            .collect();
        if pendentes.is_empty() {
            self.notificar(
                "Fila vazia: adicione ao menos um arquivo pendente.",
                TipoAviso::Erro,
            );
            return;
        }
        let mut p = json!({
            "items": pendentes,
            "model": self.tr.modelo,
            "language": self.tr.idioma,
            "format": self.tr.formato,
            "beams": self.tr.beams.trim().parse::<i64>().ok().filter(|b| *b > 0).unwrap_or(5),
            "word_level": self.tr.palavras,
        });
        if self.caps.interlocutores {
            let n = |s: &str| s.trim().parse::<i64>().ok().filter(|v| *v > 0);
            p["interlocutores"] = json!({
                "ativo": self.tr.interloc_ativo,
                "min": n(&self.tr.interloc_min),
                "max": n(&self.tr.interloc_max),
            });
        }
        if self.caps.motor_spark {
            p["motor"] = json!(if self.cfg.motor == "spark" {
                "spark"
            } else {
                "local"
            });
        }
        self.iniciar_acao("tr_iniciar", "Iniciando a fila de transcrição…");
        self.chamar(Tag::IniciarTr, "start_transcricao", vec![p]);
    }

    fn texto_modelo(&self) -> String {
        let Some(b) = &self.boot else {
            return String::new();
        };
        let t = &b.transcricao;
        let info = t.info(&self.tr.modelo).cloned().unwrap_or_default();
        let onde = if t.instalados.contains(&self.tr.modelo) {
            "instalado neste computador"
        } else {
            "será baixado na primeira vez (exige internet)"
        };
        let tam = info
            .tamanho_mb
            .map(|m| format!("{m:.0}"))
            .unwrap_or_else(|| "?".into());
        format!(
            "{} MB · precisão {} · velocidade {} · {}",
            tam,
            if info.precisao.is_empty() {
                "?".into()
            } else {
                info.precisao.to_lowercase()
            },
            if info.velocidade.is_empty() {
                "?".into()
            } else {
                info.velocidade.to_lowercase()
            },
            onde
        )
    }

    pub fn ui_transcricao(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let rodando = self.tr.rodando;
        let iniciando = self.ocupado.contains("tr_iniciar");
        let pausando = self.ocupado.contains("tr_pausar");
        let pendentes = self
            .tr
            .itens
            .iter()
            .filter(|i| i.status == StatusItem::Pendente)
            .count();
        let resumo = if self.tr.itens.is_empty() {
            String::new()
        } else {
            format!(
                "{} de {}",
                fmt::plural(pendentes, "pendente", "pendentes"),
                self.tr.itens.len()
            )
        };
        let sobre = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let escolhendo = self.ocupado.contains("escolher");
        let modelos = self
            .boot
            .as_ref()
            .map(|b| b.transcricao.selecionaveis())
            .unwrap_or_default();
        let (idiomas, formatos) = self
            .boot
            .as_ref()
            .map(|b| {
                (
                    b.transcricao.idiomas.clone(),
                    b.transcricao.formatos.clone(),
                )
            })
            .unwrap_or_default();
        let info_modelo = self.texto_modelo();
        let faltam_modelos = self
            .boot
            .as_ref()
            .map(|b| !b.transcricao.nao_instalados().is_empty())
            .unwrap_or(false);

        self.pagina_com_barra(
            ui,
            "transcricao",
            |app, ui| {
                cabecalho_pagina(
                    ui,
                    &pal,
                    "Transcrição",
                    "Transforme áudios e vídeos em texto, com o Whisper rodando neste computador.",
                );
                // ---- fila
                cartao(ui, &pal, |ui| {
                    ui.horizontal(|ui| {
                        titulo2(ui, "Fila");
                        a_direita(ui, |ui| {
                            let livre = !rodando;
                            if btn("Limpar")
                                .pequeno()
                                .ativo(livre)
                                .mostrar(ui, &pal)
                                .clicked()
                            {
                                app.tr.itens.clear();
                                app.tr.selecionado = None;
                                app.atualizar_resumo_tr();
                            }
                            if btn("Remover")
                                .pequeno()
                                .ativo(livre && app.tr.selecionado.is_some())
                                .mostrar(ui, &pal)
                                .clicked()
                            {
                                let sel = app.tr.selecionado.take();
                                app.tr.itens.retain(|i| Some(&i.id) != sel.as_ref());
                                app.atualizar_resumo_tr();
                            }
                            if btn("↓")
                                .pequeno()
                                .ativo(livre)
                                .mostrar(ui, &pal)
                                .on_hover_text("Mover para baixo")
                                .clicked()
                            {
                                app.mover_tr(1);
                            }
                            if btn("↑")
                                .pequeno()
                                .ativo(livre)
                                .mostrar(ui, &pal)
                                .on_hover_text("Mover para cima")
                                .clicked()
                            {
                                app.mover_tr(-1);
                            }
                            if btn("Adicionar arquivos")
                                .pequeno()
                                .ativo(!escolhendo)
                                .mostrar(ui, &pal)
                                .clicked()
                            {
                                app.escolher_arquivos(Escolha::ArquivosTranscricao, "media", true);
                            }
                        });
                    });
                    ui.add_space(6.0);
                    let r = area_soltar(
                        ui,
                        &pal,
                        sobre,
                        true,
                        "Adicionar áudios ou vídeos",
                        "mp3, wav, m4a, ogg, flac, aac, wma, mp4, mkv, mov, avi, webm…",
                        "",
                    );
                    if r.clicked() && !escolhendo {
                        app.escolher_arquivos(Escolha::ArquivosTranscricao, "media", true);
                    }
                    if !app.tr.itens.is_empty() {
                        ui.add_space(6.0);
                        let mut clicado: Option<String> = None;
                        ScrollArea::vertical()
                            .id_salt("tr_fila")
                            .max_height(230.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for (i, it) in app.tr.itens.iter().enumerate() {
                                    let sel = app.tr.selecionado.as_ref() == Some(&it.id);
                                    let r = Frame::new()
                                        .fill(if sel { pal.accent_soft } else { pal.surface2 })
                                        .stroke(Stroke::new(
                                            1.0,
                                            if sel { pal.accent } else { pal.border },
                                        ))
                                        .corner_radius(CornerRadius::same(7))
                                        .inner_margin(Margin::symmetric(12, 7))
                                        .show(ui, |ui| {
                                            ui.set_width(ui.available_width());
                                            ui.horizontal(|ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new((i + 1).to_string())
                                                            .color(pal.muted),
                                                    )
                                                    .selectable(false),
                                                );
                                                ui.add(
                                                    egui::Label::new(fmt::base_name(&it.caminho))
                                                        .truncate()
                                                        .selectable(false),
                                                )
                                                .on_hover_text(&it.caminho);
                                                a_direita(ui, |ui| {
                                                    chip(
                                                        ui,
                                                        &pal,
                                                        it.status.texto(),
                                                        cor_status(it.status),
                                                    )
                                                });
                                            });
                                        })
                                        .response;
                                    if ui
                                        .interact(r.rect, r.id.with("sel"), Sense::click())
                                        .clicked()
                                    {
                                        clicado = Some(it.id.clone());
                                    }
                                }
                            });
                        if clicado.is_some() {
                            app.tr.selecionado = clicado;
                        }
                    }
                });
                // ---- opcoes
                cartao(ui, &pal, |ui| {
                    ui.horizontal(|ui| {
                        titulo2(ui, "Opções");
                        mudo(ui, &pal, "(valem para toda a fila)");
                    });
                    ui.add_space(6.0);
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            rotulo(ui, "Modelo");
                            let op: Vec<(String, String)> =
                                modelos.iter().map(|m| (m.clone(), m.clone())).collect();
                            let mut m = app.tr.modelo.clone();
                            if combo(ui, "tr_modelo", &mut m, &op, 150.0) {
                                app.tr.modelo = m;
                            }
                        });
                        ui.vertical(|ui| {
                            rotulo(ui, "Idioma");
                            let op: Vec<(String, String)> = idiomas
                                .iter()
                                .map(|l| {
                                    (
                                        l.clone(),
                                        if l == "auto" {
                                            "automático".to_string()
                                        } else {
                                            l.clone()
                                        },
                                    )
                                })
                                .collect();
                            let mut m = app.tr.idioma.clone();
                            if combo(ui, "tr_idioma", &mut m, &op, 130.0) {
                                app.tr.idioma = m;
                            }
                        });
                        ui.vertical(|ui| {
                            rotulo(ui, "Formato");
                            let op: Vec<(String, String)> =
                                formatos.iter().map(|m| (m.clone(), m.clone())).collect();
                            let mut m = app.tr.formato.clone();
                            if combo(ui, "tr_formato", &mut m, &op, 110.0) {
                                app.tr.formato = m;
                            }
                        });
                        ui.vertical(|ui| {
                            rotulo(ui, "Precisão da busca (feixes)");
                            entrada_simples(ui, &pal, &mut app.tr.beams, "5", 150.0);
                        });
                    });
                    ui.add_space(4.0);
                    ui.checkbox(
                        &mut app.tr.palavras,
                        "Marcar o tempo de cada palavra (srt e json)",
                    );
                    if app.caps.interlocutores {
                        ui.add_space(4.0);
                        ui.checkbox(&mut app.tr.interloc_ativo, "Identificar interlocutores");
                        if app.tr.interloc_ativo {
                            ui.horizontal(|ui| {
                                mudo(ui, &pal, "Quantidade automática. Limites opcionais:");
                                ui.label("mín.");
                                entrada_simples(ui, &pal, &mut app.tr.interloc_min, "", 60.0);
                                ui.label("máx.");
                                entrada_simples(ui, &pal, &mut app.tr.interloc_max, "", 60.0);
                            });
                        }
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        mudo(ui, &pal, &info_modelo);
                        if faltam_modelos
                            && btn("Baixar outro modelo…")
                                .link()
                                .mostrar(ui, &pal)
                                .clicked()
                        {
                            app.tr.baixar_aberto = !app.tr.baixar_aberto;
                        }
                    });
                    if app.tr.baixar_aberto {
                        ui.add_space(4.0);
                        app.ui_baixar_modelo(ui);
                    }
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        for (nome, ok) in &app.tr.deps {
                            chip(
                                ui,
                                &pal,
                                &format!("{} {}", if *ok { "✓" } else { "✕" }, nome),
                                if *ok { Cor::Ok } else { Cor::Erro },
                            );
                        }
                    });
                });
                // ---- progresso
                cartao(ui, &pal, |ui| {
                    ui.horizontal(|ui| {
                        titulo2(ui, "Progresso");
                        a_direita(ui, |ui| mudo(ui, &pal, &app.tr.status));
                    });
                    ui.add_space(8.0);
                    let largura = ui.available_width();
                    let det = app.tr.rodando && app.tr.pct > 0.0 || app.tr.pct > 0.0;
                    barra(
                        ui,
                        &pal,
                        if det || !app.tr.rodando {
                            Some(app.tr.pct)
                        } else {
                            None
                        },
                        largura,
                        14.0,
                        None,
                    );
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let w = (ui.available_width() - 3.0 * ui.spacing().item_spacing.x) / 4.0;
                        for (t, v) in [
                            ("Processado", app.tr.feito.as_str()),
                            ("Restante", app.tr.restante.as_str()),
                            ("Decorrido", app.tr.decorrido.as_str()),
                            ("Estimativa", app.tr.estimativa.as_str()),
                        ] {
                            ui.vertical(|ui| {
                                ui.set_width(w);
                                mudo_peq(ui, &pal, t);
                                ui.add(egui::Label::new(
                                    RichText::new(v).family(crate::theme::negrito()).size(15.0),
                                ));
                            });
                        }
                    });
                });
                app.ui_interlocutores(ui);
            },
            |app, ui| {
                mudo(ui, &pal, &resumo);
                a_direita(ui, |ui| {
                    if btn(if iniciando {
                        "Iniciando…"
                    } else {
                        "Iniciar fila"
                    })
                    .primario()
                    .ativo(!rodando)
                    .ocupado(iniciando)
                    .largura(120.0)
                    .mostrar(ui, &pal)
                    .clicked()
                    {
                        app.iniciar_tr();
                    }
                    if btn(if app.tr.pausado {
                        "Continuar"
                    } else {
                        "Pausar"
                    })
                    .ativo(rodando)
                    .ocupado(pausando)
                    .mostrar(ui, &pal)
                    .clicked()
                    {
                        app.iniciar_acao(
                            "tr_pausar",
                            if app.tr.pausado {
                                "Retomando…"
                            } else {
                                "Pausando…"
                            },
                        );
                        app.chamar(Tag::PausarTr, "toggle_pause_transcricao", vec![]);
                    }
                    if btn("Cancelar")
                        .perigo()
                        .ativo(rodando)
                        .mostrar(ui, &pal)
                        .clicked()
                    {
                        app.chamar(Tag::CancelarTr, "cancel_transcricao", vec![]);
                        app.tr.status = "Cancelando...".into();
                        app.definir_status("Cancelando…", Tom::Ocupado);
                    }
                    if btn("Abrir pasta de saída")
                        .ativo(app.tr.ultima_saida.is_some())
                        .mostrar(ui, &pal)
                        .clicked()
                    {
                        if let Some(p) = app.tr.ultima_saida.clone() {
                            app.chamar(Tag::AbrirCaminho, "open_path", vec![json!(p)]);
                        }
                    }
                });
            },
        );
        let _ = vec2(0.0, 0.0);
    }

    /// Painel dentro da tela (sem janela): escolher e baixar outro modelo.
    fn ui_baixar_modelo(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let Some(b) = &self.boot else { return };
        let candidatos: Vec<(String, String)> = b
            .transcricao
            .nao_instalados()
            .iter()
            .map(|(n, i)| {
                (
                    n.clone(),
                    format!(
                        "{} · {:.0} MB · {}",
                        n,
                        i.tamanho_mb.unwrap_or(0.0),
                        i.descricao
                    ),
                )
            })
            .collect();
        faixa(ui, pal.surface2, pal.border, |ui| {
            if candidatos.is_empty() {
                mudo(
                    ui,
                    &pal,
                    "Todos os modelos já estão instalados neste computador.",
                );
                return;
            }
            rotulo(ui, "Modelo a baixar (exige internet)");
            ui.horizontal(|ui| {
                let mut esc = self.tr.baixar_escolha.clone();
                if combo(ui, "tr_baixar", &mut esc, &candidatos, 380.0) {
                    self.tr.baixar_escolha = esc;
                }
                let ocupado = self.tr.baixando;
                if btn(if ocupado { "Baixando…" } else { "Baixar" })
                    .primario()
                    .ocupado(ocupado)
                    .largura(100.0)
                    .mostrar(ui, &pal)
                    .clicked()
                {
                    self.tr.baixando = true;
                    self.tr.baixar_msg = "Baixando… pode demorar bastante.".into();
                    let n = self.tr.baixar_escolha.clone();
                    self.chamar(
                        Tag::BaixarModelo(n.clone()),
                        "download_model",
                        vec![json!(n)],
                    );
                }
                if btn("Fechar").mostrar(ui, &pal).clicked() {
                    self.tr.baixar_aberto = false;
                }
            });
            if !self.tr.baixar_msg.is_empty() {
                mudo_peq(ui, &pal, &self.tr.baixar_msg);
            }
        });
    }

    /// Resumo dos interlocutores do item selecionado (so com a capacidade ligada).
    fn ui_interlocutores(&mut self, ui: &mut Ui) {
        if !self.caps.interlocutores {
            return;
        }
        let pal = self.pal;
        let Some(sel) = self.tr.selecionado.clone() else {
            return;
        };
        let Some(item) = self.tr.itens.iter_mut().find(|i| i.id == sel) else {
            return;
        };
        let nome = fmt::base_name(&item.caminho).to_string();
        let Some(r) = item.interlocutores.as_mut() else {
            return;
        };
        let n = r.rotulos.len();
        let total: f64 = r.tempo_s.iter().sum();
        let trocas = r.trocas;
        cartao(ui, &pal, |ui| {
            titulo2(ui, &format!("Interlocutores de {nome}"));
            mudo(
                ui,
                &pal,
                &format!(
                    "{} · tempo de fala {}{}",
                    fmt::plural(n, "interlocutor", "interlocutores"),
                    fmt::dur(Some(total)),
                    trocas
                        .map(|t| format!(" · {} trocas", t))
                        .unwrap_or_default()
                ),
            );
            ui.add_space(4.0);
            for (i, rot) in r.rotulos.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    entrada_simples(ui, &pal, rot, &format!("Interlocutor {}", i + 1), 240.0);
                    if let Some(t) = r.tempo_s.get(i) {
                        mudo(ui, &pal, &fmt::dur(Some(*t)));
                    }
                });
            }
        });
    }
}
