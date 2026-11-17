//! Tela Execucoes (Central): filtros, cartoes ao vivo, detalhes e log por execucao.

use crate::app::*;
use crate::bus::Tag;
use crate::fmt;
use crate::model::Execucao;
use crate::pg_tr::combo;
use crate::theme::negrito;
use crate::widgets::*;
use eframe::egui::{self, CornerRadius, Frame, Margin, RichText, ScrollArea, Stroke, Ui};
use serde_json::json;

const ESTADOS: [(&str, &str); 6] = [
    ("todas", "Todas"),
    ("ativas", "Em andamento"),
    ("concluida", "Concluídas"),
    ("falhou", "Falhas"),
    ("cancelada", "Canceladas"),
    ("interrompida", "Interrompidas"),
];

fn cor_estado(e: &str) -> Cor {
    match e {
        "rodando" => Cor::Rodando,
        "concluida" => Cor::Ok,
        "falhou" => Cor::Erro,
        "interrompida" => Cor::Aviso,
        _ => Cor::Neutro,
    }
}

fn fonte_eta(f: &str) -> &'static str {
    match f {
        "andamento" => "pelo ritmo atual",
        "historico" => "pelo histórico de execuções anteriores",
        "misto" => "pelo ritmo atual e pelo histórico",
        _ => "estimativa",
    }
}

/// Resumo "N de M execuções · X rodando · Y na fila".
pub fn resumo_lista(visiveis: usize, total: usize, rodando: usize, fila: usize) -> String {
    if total == 0 {
        "Nenhuma execução registrada ainda.".to_string()
    } else {
        format!(
            "{} de {} · {} rodando · {} na fila",
            visiveis,
            fmt::plural(total, "execução", "execuções"),
            rodando,
            fila
        )
    }
}

impl App {
    pub fn ui_execucoes(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        if let Some(t) = self.exec.resumo_copiado.take() {
            ui.ctx().copy_text(t);
            self.notificar("Resumo da execução copiado.", TipoAviso::Info);
        }
        let busca = self.exec.busca.trim().to_lowercase();
        let estado = ESTADOS[self.exec.estado.min(5)].0;
        let visiveis: Vec<Execucao> = self
            .exec
            .lista
            .execucoes
            .iter()
            .filter(|e| e.passa(estado, &self.exec.ferramenta, &busca))
            .cloned()
            .collect();
        let filtrando = estado != "todas" || !self.exec.ferramenta.is_empty() || !busca.is_empty();
        let limpando = self.ocupado.contains("exec_limpar");
        self.coluna(ui, "execucoes", |app, ui| {
            cabecalho_pagina(
                ui,
                &pal,
                "Execuções",
                "Tudo o que o programa está fazendo e o que já fez, com etapa, tempo, estimativa e log de cada tarefa.",
            );
            cartao(ui, &pal, |ui| {
                let nomes: Vec<&str> = ESTADOS.iter().map(|e| e.1).collect();
                if let Some(i) = segmentado(ui, &pal, app.exec.estado, &nomes) {
                    app.exec.estado = i;
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let mut op = vec![(String::new(), "Todas as ferramentas".to_string())];
                    op.extend(app.exec.lista.ferramentas.iter().map(|(k, v)| (k.clone(), v.clone())));
                    let mut f = app.exec.ferramenta.clone();
                    if combo(ui, "exec_ferr", &mut f, &op, 190.0) {
                        app.exec.ferramenta = f;
                    }
                    let w = (ui.available_width() - 150.0 - ui.spacing().item_spacing.x).max(120.0);
                    entrada_simples(ui, &pal, &mut app.exec.busca, "Buscar por nome, arquivo, etapa ou resultado", w);
                    if btn(if limpando { "Limpando…" } else { "Limpar concluídas" }).pequeno().ocupado(limpando).mostrar(ui, &pal).clicked() {
                        app.confirma = Some(Confirma {
                            titulo: "Limpar execuções concluídas".into(),
                            corpo: "Remove da lista (e do histórico em disco) as execuções já terminadas. As que estão rodando ou na fila continuam. O histórico usado nas estimativas de tempo é mantido.".into(),
                            ok: "Limpar".into(),
                            perigo: false,
                            acao: AcaoConfirma::LimparExecucoes,
                        });
                    }
                });
                mudo_peq(
                    ui,
                    &pal,
                    &resumo_lista(visiveis.len(), app.exec.lista.total, app.exec.lista.rodando, app.exec.lista.na_fila),
                );
            });
            if visiveis.is_empty() {
                let t = if app.exec.lista.total == 0 {
                    "Nada foi executado ainda. Converta um arquivo, rode um lote ou verifique a instalação: cada tarefa aparece aqui, ao vivo."
                } else if filtrando {
                    "Nenhuma execução corresponde aos filtros."
                } else {
                    ""
                };
                mudo(ui, &pal, t);
            }
            for ex in &visiveis {
                app.cartao_execucao(ui, ex);
            }
        });
    }

    fn cartao_execucao(&mut self, ui: &mut Ui, ex: &Execucao) {
        let pal = self.pal;
        let aberto = self.exec.abertos.contains(&ex.id);
        let ativa = ex.ativa();
        Frame::new()
            .fill(pal.surface)
            .stroke(Stroke::new(1.0, pal.border))
            .corner_radius(CornerRadius::same(crate::theme::RAIO))
            .inner_margin(Margin::symmetric(18, 14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&ex.rotulo).family(negrito())).truncate(),
                    );
                    let sub = [ex.ferramenta_nome.as_str(), ex.entrada.as_str()]
                        .iter()
                        .filter(|s| !s.is_empty())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(" · ");
                    ui.add(
                        egui::Label::new(RichText::new(sub).size(12.5).color(pal.muted)).truncate(),
                    )
                    .on_hover_text(&ex.entrada);
                    a_direita(ui, |ui| {
                        if btn(if aberto { "Ocultar" } else { "Detalhes" })
                            .link()
                            .pequeno()
                            .mostrar(ui, &pal)
                            .clicked()
                        {
                            if aberto {
                                self.exec.abertos.remove(&ex.id);
                            } else {
                                self.exec.abertos.insert(ex.id.clone());
                                self.cutucar_exec();
                            }
                        }
                        chip(ui, &pal, &ex.estado_texto, cor_estado(&ex.estado));
                        if !ex.origem.is_empty() {
                            chip(ui, &pal, &ex.origem, Cor::Neutro);
                        }
                    });
                });
                let etapa = ex.linha_etapa();
                if !etapa.is_empty() {
                    ui.add(egui::Label::new(RichText::new(etapa).size(12.5)));
                }
                // Barra: determinada so com total conhecido; sem total, indeterminada (nunca porcentagem inventada).
                let mostra = ex.estado == "rodando" || (!ativa && ex.determinada);
                if mostra {
                    let det = ex.determinada;
                    let cor = match ex.estado.as_str() {
                        "concluida" => Some(pal.ok),
                        "falhou" => Some(pal.danger),
                        "cancelada" | "interrompida" => Some(pal.muted),
                        _ => None,
                    };
                    ui.horizontal(|ui| {
                        let w = (ui.available_width() - 50.0).max(60.0);
                        let frac = if det {
                            Some(fmt::frac(ex.progresso.unwrap_or(0.0)))
                        } else if ex.estado == "rodando" {
                            None
                        } else {
                            Some(0.0)
                        };
                        barra(ui, &pal, frac, w, 8.0, cor);
                        if det {
                            mudo_peq(
                                ui,
                                &pal,
                                &format!("{}%", ex.progresso.unwrap_or(0.0).round() as i64),
                            );
                        }
                    });
                }
                ui.horizontal_wrapped(|ui| {
                    if ex.decorrido_s.is_some() {
                        let rot = if ativa { "Decorrido" } else { "Duração" };
                        mudo_peq(ui, &pal, &format!("{rot}: {}", fmt::dur(ex.decorrido_s)));
                    }
                    if let Some(eta) = ex.eta_exibido() {
                        let r = mudo_resp(ui, &pal, &format!("Restante: {eta}"));
                        if ex.eta_s.is_some() {
                            r.on_hover_text(format!(
                                "Entre {} e {} ({})",
                                fmt::dur(ex.eta_min_s),
                                fmt::dur(ex.eta_max_s),
                                fonte_eta(&ex.eta_fonte)
                            ));
                        }
                    }
                    if !ex.progresso_texto.is_empty() {
                        mudo_peq(ui, &pal, &ex.progresso_texto);
                    }
                });
                ui.horizontal(|ui| {
                    if ex.cancelavel || ex.cancelando {
                        let cancelando = ex.cancelando || self.exec.cancelando.contains(&ex.id);
                        if btn(if cancelando {
                            "Cancelando…"
                        } else {
                            "Cancelar"
                        })
                        .perigo()
                        .pequeno()
                        .ocupado(cancelando)
                        .mostrar(ui, &pal)
                        .clicked()
                        {
                            self.exec.cancelando.insert(ex.id.clone());
                            self.iniciar_acao("exec_cancelar", "Cancelando…");
                            self.chamar(
                                Tag::ExecCancelar(ex.id.clone()),
                                "exec_cancelar",
                                vec![json!(ex.id)],
                            );
                        }
                    }
                    if ex.repetivel {
                        let rep = self.ocupado.contains("exec_repetir");
                        if btn("Repetir")
                            .pequeno()
                            .ocupado(rep)
                            .mostrar(ui, &pal)
                            .clicked()
                        {
                            if ex.ferramenta == "verificacao" && ex.variante.is_empty() {
                                self.verificar_instalacao(false);
                            } else {
                                self.iniciar_acao("exec_repetir", "Repetindo…");
                                self.chamar(
                                    Tag::ExecRepetir(ex.id.clone()),
                                    "exec_repetir",
                                    vec![json!(ex.id)],
                                );
                            }
                        }
                    }
                    if btn("Copiar resumo").pequeno().mostrar(ui, &pal).clicked() {
                        self.chamar(
                            Tag::ExecResumo(ex.id.clone()),
                            "exec_resumo",
                            vec![json!(ex.id)],
                        );
                    }
                    if !ex.log_arquivo.is_empty()
                        && ex.log_ini.is_some()
                        && btn("Ver no log").pequeno().mostrar(ui, &pal).clicked()
                    {
                        self.abrir_log_da_execucao(ex);
                    }
                });
                if aberto {
                    ui.add_space(4.0);
                    let sep = ui
                        .allocate_exact_size(
                            egui::vec2(ui.available_width(), 1.0),
                            egui::Sense::hover(),
                        )
                        .0;
                    ui.painter().rect_filled(sep, 0.0, pal.border);
                    ui.add_space(4.0);
                    let dl: [(&str, String); 11] = [
                        ("Ferramenta", ex.ferramenta_nome.clone()),
                        ("Entrada", ex.entrada.clone()),
                        ("Estado", ex.estado_texto.clone()),
                        ("Etapa", ex.etapa.clone()),
                        ("Progresso", ex.progresso_texto.clone()),
                        (
                            "Início",
                            ex.inicio_em.map(fmt::data_local).unwrap_or_default(),
                        ),
                        ("Fim", ex.fim_em.map(fmt::data_local).unwrap_or_default()),
                        ("Resultado", ex.mensagem.clone()),
                        ("Erro", ex.erro.clone()),
                        ("Log", ex.log_arquivo.clone()),
                        ("Identificador", ex.id.clone()),
                    ];
                    egui::Grid::new(format!("dl_{}", ex.id))
                        .num_columns(2)
                        .spacing([14.0, 3.0])
                        .show(ui, |ui| {
                            for (k, v) in dl.iter().filter(|(_, v)| !v.is_empty()) {
                                ui.add(egui::Label::new(
                                    RichText::new(*k).size(12.5).color(pal.muted),
                                ));
                                ui.add(egui::Label::new(RichText::new(v).size(12.5)));
                                ui.end_row();
                            }
                        });
                    ui.add_space(6.0);
                    mudo_peq(ui, &pal, "Log desta execução");
                    Frame::new()
                        .fill(pal.surface2)
                        .stroke(Stroke::new(1.0, pal.border))
                        .corner_radius(CornerRadius::same(7))
                        .inner_margin(Margin::symmetric(10, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ScrollArea::vertical()
                                .id_salt(format!("lg_{}", ex.id))
                                .max_height(180.0)
                                .auto_shrink([false, true])
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    let linhas = self.exec.linhas.get(&ex.id);
                                    match linhas {
                                        Some(l) if !l.is_empty() => {
                                            for x in l {
                                                let rot = match x.n.as_str() {
                                                    "erro" => "ERRO: ",
                                                    "aviso" => "AVISO: ",
                                                    _ => "",
                                                };
                                                let cor = match x.n.as_str() {
                                                    "erro" => pal.danger,
                                                    "aviso" => pal.warn,
                                                    _ => pal.text,
                                                };
                                                ui.add(egui::Label::new(mono(
                                                    format!("[{}] {}{}", x.t, rot, x.x),
                                                    cor,
                                                )));
                                            }
                                        }
                                        _ => {
                                            ui.add(egui::Label::new(mono(
                                                "Esta execução ainda não registrou linhas de log.",
                                                pal.muted,
                                            )));
                                        }
                                    }
                                });
                        });
                }
            });
        ui.add_space(8.0);
    }
}

fn mudo_resp(ui: &mut Ui, pal: &crate::theme::Palette, t: &str) -> egui::Response {
    ui.add(
        egui::Label::new(RichText::new(t).size(12.5).color(pal.muted)).sense(egui::Sense::hover()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumo_da_lista() {
        assert_eq!(
            resumo_lista(0, 0, 0, 0),
            "Nenhuma execução registrada ainda."
        );
        assert_eq!(
            resumo_lista(2, 4, 1, 1),
            "2 de 4 execuções · 1 rodando · 1 na fila"
        );
        assert_eq!(
            resumo_lista(1, 1, 0, 0),
            "1 de 1 execução · 0 rodando · 0 na fila"
        );
    }

    #[test]
    fn cores_por_estado() {
        assert!(cor_estado("rodando") == Cor::Rodando);
        assert!(cor_estado("falhou") == Cor::Erro);
        assert!(cor_estado("fila") == Cor::Neutro);
    }
}
