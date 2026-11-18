//! Tela Log: leitura ao vivo e historica dos logs do motor.

use crate::app::*;
use crate::bus::{LogUso, Tag};
use crate::fmt;
use crate::model::{peso_minimo, rotulo_nivel, ArquivoLog, Execucao, LeituraLog, LinhaLog};
use crate::pg_tr::combo;
use crate::theme::{negrito, Palette};
use crate::widgets::*;
use eframe::egui::{self, CornerRadius, Frame, Margin, RichText, ScrollArea, Stroke, Ui};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

pub const LOG_MAX_BUFFER: usize = 20_000;

/// O que carregar assim que a lista de arquivos de log chegar.
#[derive(Clone, Debug, PartialEq)]
pub enum LogPedido {
    Cauda(String),
    Intervalo {
        nome: String,
        ini: i64,
        fim: Option<i64>,
        tag: String,
        titulo: String,
    },
    Redor {
        nome: String,
        offset: i64,
    },
}

/// Linha passa pelo filtro de nivel e de texto?
pub fn passa(l: &LinhaLog, nivel: &str, busca: &str) -> bool {
    if l.peso() < peso_minimo(nivel) {
        return false;
    }
    let b = busca.trim().to_lowercase();
    b.is_empty() || format!("{} {}", l.t, l.x).to_lowercase().contains(&b)
}

pub fn rotulo_arquivo(a: &ArquivoLog) -> String {
    let data = a.data.split('-').rev().collect::<Vec<_>>().join("/");
    format!(
        "{} {} · {}{}{} · {}",
        data,
        a.hora,
        a.nome,
        if a.atual { " (sessão atual)" } else { "" },
        if a.tipo == "instalacao" {
            " (instalador)"
        } else {
            ""
        },
        fmt::bytes(a.tamanho)
    )
}

impl App {
    pub fn abrir_pagina_log(&mut self) {
        if !self.log.iniciado {
            self.log.iniciado = true;
            self.chamar(Tag::LogArquivos, "log_arquivos", vec![]);
        } else {
            self.chamar(Tag::LogArquivos, "log_arquivos", vec![]);
            if self.log.ao_vivo && !self.log.pausado {
                self.log.ultimo = Instant::now()
                    .checked_sub(Duration::from_secs(10))
                    .unwrap_or_else(Instant::now);
            }
        }
    }

    fn ler_log(&mut self, nome: &str, mut params: Value, uso: LogUso) {
        self.log.em_voo = true;
        params["nome"] = json!(nome);
        self.chamar(Tag::LogLer(uso), "log_ler", vec![params]);
    }

    pub fn ir_para_linha_log(&mut self, nome: String, offset: i64) {
        self.log.iniciado = true;
        self.log.pendente = Some(LogPedido::Redor { nome, offset });
        self.pagina = Pagina::Log;
        self.chamar(Tag::LogArquivos, "log_arquivos", vec![]);
    }

    pub fn abrir_log_da_execucao(&mut self, ex: &Execucao) {
        self.log.iniciado = true;
        self.log.pendente = Some(LogPedido::Intervalo {
            nome: ex.log_arquivo.clone(),
            ini: ex.log_ini.unwrap_or(0),
            fim: ex.log_fim,
            tag: ex.id.clone(),
            titulo: ex.rotulo.clone(),
        });
        self.pagina = Pagina::Log;
        self.chamar(Tag::LogArquivos, "log_arquivos", vec![]);
    }

    pub fn aplicar_lista_logs(&mut self, v: &Value) {
        self.log.atual = v
            .get("atual")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        self.log.arquivos =
            serde_json::from_value(v.get("arquivos").cloned().unwrap_or(Value::Null))
                .unwrap_or_default();
        let pedido = self.log.pendente.take().or_else(|| {
            if self.log.nome.is_empty() {
                Some(LogPedido::Cauda(self.log.atual.clone()))
            } else {
                None
            }
        });
        let Some(p) = pedido else { return };
        self.log.nivel = 0;
        self.log.busca.clear();
        match p {
            LogPedido::Cauda(nome) => {
                self.log.foco = None;
                self.log.tag_foco = None;
                self.log.marca_proxima = None;
                self.log.ao_vivo_proximo = None;
                self.log.rolar_proximo = true;
                self.ler_log(&nome, json!({"modo": "cauda"}), LogUso::Abrir);
            }
            LogPedido::Intervalo {
                nome,
                ini,
                fim,
                tag,
                titulo,
            } => {
                self.log.pausado = false;
                self.log.tag_foco = Some(tag);
                self.log.foco = Some(format!(
                    "Mostrando o período da execução “{titulo}”. As linhas com contorno foram gravadas por ela; as outras são de tarefas que rodaram ao mesmo tempo."
                ));
                self.log.marca_proxima = None;
                self.log.ao_vivo_proximo = Some(fim.is_none());
                self.log.rolar_proximo = false;
                self.ler_log(
                    &nome,
                    json!({"modo": "intervalo", "ini": ini, "fim": fim}),
                    LogUso::Intervalo,
                );
            }
            LogPedido::Redor { nome, offset } => {
                self.log.foco = None;
                self.log.tag_foco = None;
                self.log.auto = false;
                self.log.marca_proxima = Some(offset);
                self.log.ao_vivo_proximo = Some(nome == self.log.atual);
                self.log.rolar_proximo = false;
                self.ler_log(
                    &nome,
                    json!({"modo": "ao_redor", "offset": offset}),
                    LogUso::Redor(offset),
                );
            }
        }
    }

    pub fn resposta_log(&mut self, uso: LogUso, r: crate::proto::Resposta) {
        self.log.em_voo = false;
        self.log.ultimo = Instant::now();
        let v = match r {
            Ok(v) => v,
            Err(e) => {
                if uso != LogUso::Acompanhar {
                    self.log.linhas.clear();
                    self.log.fim = 0;
                    self.log.ini = 0;
                    self.log.ao_vivo = false;
                    self.log.marca = None;
                    self.log.info = if e.is_empty() {
                        "Não foi possível ler o registro.".into()
                    } else {
                        e
                    };
                }
                return;
            }
        };
        let l = LeituraLog::de_json(&v);
        match uso {
            LogUso::Acompanhar => {
                self.log.tamanho = l.tamanho;
                let ultimo_nv = l.linhas.last().map(|u| u.nv.clone());
                if l.reiniciado {
                    self.log.linhas = l.linhas;
                    self.log.ini = l.ini;
                    self.log.fim = l.fim;
                } else {
                    self.log.fim = l.fim;
                    self.log.linhas.extend(l.linhas.iter().cloned());
                    if self.log.linhas.len() > LOG_MAX_BUFFER {
                        let corte = self.log.linhas.len() - LOG_MAX_BUFFER;
                        self.log.linhas.drain(..corte);
                    }
                }
                if let Some(nv) = ultimo_nv {
                    self.log.nv_ant = nv;
                }
                if l.mais {
                    self.log.ultimo = Instant::now()
                        .checked_sub(Duration::from_secs(10))
                        .unwrap_or_else(Instant::now);
                }
            }
            LogUso::Anteriores => {
                let mut novas = l.linhas;
                novas.append(&mut self.log.linhas);
                self.log.linhas = novas;
                self.log.ini = l.ini;
                self.terminar_acao("log_antigas");
            }
            LogUso::Abrir | LogUso::Intervalo | LogUso::Redor(_) => {
                self.log.nome = l.nome.clone();
                self.log.tamanho = l.tamanho;
                self.log.ini = l.ini;
                self.log.fim = l.fim;
                self.log.nv_ant = l
                    .linhas
                    .last()
                    .map(|x| x.nv.clone())
                    .unwrap_or_else(|| "INFO".into());
                self.log.ao_vivo = self.log.ao_vivo_proximo.take().unwrap_or(l.ao_vivo);
                self.log.modo_busca = false;
                self.log.marca = None;
                if let LogUso::Redor(off) = uso {
                    let alvo = l.linhas.iter().find(|x| x.o >= off).or(l.linhas.last());
                    self.log.marca = alvo.map(|x| x.o);
                }
                self.log.rolar_para_marca = self.log.marca.is_some() || !self.log.rolar_proximo;
                self.log.linhas = l.linhas;
            }
        }
    }

    pub fn aplicar_busca_log(&mut self, l: LeituraLog) {
        self.log.foco = Some(format!(
            "Busca no arquivo inteiro: {}{}. O filtro de nível e o texto acima foram aplicados ao arquivo todo.",
            fmt::plural(l.total_achadas, "linha encontrada", "linhas encontradas"),
            if l.cortada { format!(" (mostrando as últimas {})", l.linhas.len()) } else { String::new() }
        ));
        self.log.tag_foco = None;
        self.log.marca = None;
        self.log.linhas = l.linhas;
        self.log.ini = 0;
        self.log.fim = l.fim;
        self.log.tamanho = l.tamanho;
        self.log.modo_busca = true;
        self.log.ao_vivo = false;
        self.log.rolar_para_marca = false;
    }

    /// Consulta periodica das linhas novas (so com a tela aberta, ao vivo e sem pausa).
    pub fn acompanhar_log(&mut self) {
        if self.pagina != Pagina::Log
            || self.log.pausado
            || !self.log.ao_vivo
            || self.log.em_voo
            || self.log.nome.is_empty()
        {
            return;
        }
        if self.log.ultimo.elapsed() < Duration::from_secs(1) {
            return;
        }
        let (nome, fim, nv) = (self.log.nome.clone(), self.log.fim, self.log.nv_ant.clone());
        self.ler_log(
            &nome,
            json!({"modo": "desde", "offset": fim, "nv_ant": nv}),
            LogUso::Acompanhar,
        );
    }

    fn texto_info(&self, visiveis: usize) -> String {
        let l = &self.log;
        let mut p = vec![if l.nome.is_empty() {
            "—".to_string()
        } else {
            l.nome.clone()
        }];
        p.push(if l.modo_busca {
            "resultado de busca".into()
        } else if l.pausado {
            "pausado".into()
        } else if l.ao_vivo {
            "ao vivo".into()
        } else {
            "arquivo fechado".into()
        });
        p.push(fmt::plural(visiveis, "linha exibida", "linhas exibidas"));
        p.push(fmt::plural(
            l.linhas.len(),
            "linha carregada",
            "linhas carregadas",
        ));
        if l.tamanho > 0 {
            p.push(fmt::bytes(l.tamanho));
        }
        if l.ini > 0 && !l.modo_busca {
            p.push("há linhas anteriores ainda não carregadas".into());
        }
        p.join(" · ")
    }

    pub fn ui_log(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let nivel = self.log.nivel_chave();
        let busca = self.log.busca.clone();
        let indices: Vec<usize> = self
            .log
            .linhas
            .iter()
            .enumerate()
            .filter(|(_, l)| passa(l, nivel, &busca))
            .map(|(i, _)| i)
            .collect();
        let info = if self.log.iniciado && !self.log.nome.is_empty() {
            self.texto_info(indices.len())
        } else {
            self.log.info.clone()
        };
        let antigas_ocupado = self.ocupado.contains("log_antigas");
        let buscando = self.ocupado.contains("log_buscar");
        let pasta_ocupado = self.ocupado.contains("log_pasta");
        self.coluna(ui, "log", |app, ui| {
            cabecalho_pagina(
                ui,
                &pal,
                "Registro",
                "Acompanhe o registro do programa ao vivo ou abra um registro antigo. Cada execução também aponta para as suas linhas aqui.",
            );
            cartao(ui, &pal, |ui| {
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        rotulo(ui, "Arquivo");
                        let op: Vec<(String, String)> = app.log.arquivos.iter().map(|a| (a.nome.clone(), rotulo_arquivo(a))).collect();
                        let mut n = app.log.nome.clone();
                        if combo(ui, "log_arq", &mut n, &op, 430.0) {
                            app.log.pendente = Some(LogPedido::Cauda(n));
                            app.chamar(Tag::LogArquivos, "log_arquivos", vec![]);
                        }
                    });
                    ui.vertical(|ui| {
                        rotulo(ui, "Nível");
                        let op: Vec<(String, String)> = NIVEIS_LOG.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect();
                        let mut n = app.log.nivel_chave().to_string();
                        if combo(ui, "log_nivel", &mut n, &op, 150.0) {
                            app.log.nivel = NIVEIS_LOG.iter().position(|(k, _)| *k == n).unwrap_or(0);
                        }
                    });
                    ui.vertical(|ui| {
                        rotulo(ui, "Buscar");
                        entrada_simples(ui, &pal, &mut app.log.busca, "Texto na linha", 190.0);
                    });
                });
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if btn(if app.log.pausado { "Continuar" } else { "Pausar" }).pequeno().mostrar(ui, &pal).clicked() {
                        app.log.pausado = !app.log.pausado;
                        if !app.log.pausado {
                            app.log.ultimo = Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now);
                        }
                    }
                    ui.checkbox(&mut app.log.auto, "Rolar automaticamente");
                    if btn(if antigas_ocupado { "Carregando…" } else { "Carregar linhas anteriores" })
                        .pequeno()
                        .ativo(app.log.ini > 0 && !app.log.modo_busca)
                        .ocupado(antigas_ocupado)
                        .mostrar(ui, &pal)
                        .clicked()
                    {
                        app.iniciar_acao("log_antigas", "Carregando linhas anteriores…");
                        let (nome, ini) = (app.log.nome.clone(), app.log.ini);
                        app.ler_log(&nome, json!({"modo": "antes", "offset": ini}), LogUso::Anteriores);
                    }
                    if btn(if buscando { "Buscando…" } else { "Buscar no arquivo inteiro" }).pequeno().ocupado(buscando).mostrar(ui, &pal).clicked() {
                        app.iniciar_acao("log_buscar", "Buscando no arquivo…");
                        let p = json!({"nome": app.log.nome, "nivel": app.log.nivel_chave(), "texto": app.log.busca.trim(), "limite": 2000});
                        app.chamar(Tag::LogBuscar, "log_buscar", vec![p]);
                    }
                    a_direita(ui, |ui| {
                        if btn(if pasta_ocupado { "Abrindo…" } else { "Abrir pasta do registro" }).pequeno().ocupado(pasta_ocupado).mostrar(ui, &pal).clicked() {
                            app.iniciar_acao("log_pasta", "Abrindo a pasta dos registros…");
                            app.chamar(Tag::LogPasta, "log_abrir_pasta", vec![]);
                        }
                        if btn("Copiar").pequeno().mostrar(ui, &pal).clicked() {
                            if indices.is_empty() {
                                app.notificar("Não há linhas visíveis para copiar.", TipoAviso::Info);
                            } else {
                                let t = indices
                                    .iter()
                                    .map(|i| {
                                        let l = &app.log.linhas[*i];
                                        if l.t.is_empty() { l.x.clone() } else { format!("{} | {} | {}", l.t, l.nv, l.x) }
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                ui.ctx().copy_text(t);
                                app.notificar(&format!("Registro copiado: {}.", fmt::plural(indices.len(), "linha", "linhas")), TipoAviso::Info);
                            }
                        }
                    });
                });
                if let Some(f) = app.log.foco.clone() {
                    ui.add_space(4.0);
                    faixa(ui, pal.accent_soft, pal.accent, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.add(egui::Label::new(RichText::new(f).size(13.0)));
                            if btn("Mostrar o registro inteiro").pequeno().mostrar(ui, &pal).clicked() {
                                let n = app.log.nome.clone();
                                app.log.pendente = Some(LogPedido::Cauda(n));
                                app.chamar(Tag::LogArquivos, "log_arquivos", vec![]);
                            }
                        });
                    });
                }
                ui.add_space(6.0);
                app.desenhar_linhas_log(ui, &pal, &indices);
                ui.add_space(4.0);
                mudo_peq(ui, &pal, &info);
            });
        });
    }

    fn desenhar_linhas_log(&mut self, ui: &mut Ui, pal: &Palette, indices: &[usize]) {
        let h = 17.0;
        Frame::new()
            .fill(pal.surface2)
            .stroke(Stroke::new(1.0, pal.border))
            .corner_radius(CornerRadius::same(7))
            .inner_margin(Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height((ui.ctx().content_rect().height() - 420.0).clamp(220.0, 700.0));
                if indices.is_empty() {
                    mudo(ui, pal, "Nenhuma linha corresponde aos filtros.");
                    return;
                }
                let mut area = ScrollArea::both()
                    .id_salt("log_linhas")
                    .auto_shrink([false, false]);
                if self.log.rolar_para_marca {
                    if let Some(m) = self.log.marca {
                        if let Some(pos) = indices.iter().position(|i| self.log.linhas[*i].o == m) {
                            area = area.vertical_scroll_offset((pos as f32 * h - 100.0).max(0.0));
                        }
                    } else {
                        // Sem linha marcada: vai para o fim neste quadro (sem deslocamento gigante, que estoura o egui).
                        area = area.stick_to_bottom(true);
                    }
                    self.log.rolar_para_marca = false;
                } else {
                    area = area.stick_to_bottom(self.log.auto);
                }
                let tag = self.log.tag_foco.clone().map(|t| format!("[exec {t}]"));
                let marca = self.log.marca;
                area.show_rows(ui, h, indices.len(), |ui, faixa| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for k in faixa {
                        let Some(l) = indices.get(k).and_then(|i| self.log.linhas.get(*i)) else {
                            continue;
                        };
                        let destaque =
                            tag.as_ref().is_some_and(|t| l.x.contains(t)) || marca == Some(l.o);
                        let cor = match l.nv.as_str() {
                            "ERROR" | "CRITICAL" => pal.danger,
                            "WARNING" => pal.warn,
                            "DEBUG" => pal.muted,
                            _ => pal.text,
                        };
                        let r = ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if l.t.is_empty() {
                                ui.add_space(110.0);
                            } else {
                                ui.add(
                                    egui::Label::new(mono(
                                        l.t.get(11..).unwrap_or(&l.t),
                                        pal.muted,
                                    ))
                                    .selectable(false),
                                );
                                texto_fixo(
                                    ui,
                                    52.0,
                                    h,
                                    RichText::new(rotulo_nivel(&l.nv))
                                        .monospace()
                                        .size(12.0)
                                        .family(negrito())
                                        .color(cor),
                                );
                            }
                            ui.add(
                                egui::Label::new(mono(l.x.as_str(), cor))
                                    .wrap_mode(egui::TextWrapMode::Extend),
                            );
                        });
                        if destaque {
                            let mut cx = r.response.rect;
                            cx.max.x = cx.max.x.max(ui.max_rect().right());
                            ui.painter().rect_stroke(
                                cx.expand(1.0),
                                CornerRadius::same(3),
                                Stroke::new(1.0, pal.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                    }
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linha(nv: &str, t: &str, x: &str) -> LinhaLog {
        LinhaLog {
            o: 0,
            t: t.into(),
            nv: nv.into(),
            x: x.into(),
        }
    }

    #[test]
    fn filtro_de_nivel_e_texto() {
        let aviso = linha(
            "WARNING",
            "2026-11-16 10:00:05",
            "Tesseract sem o idioma por",
        );
        let info = linha("INFO", "2026-11-16 10:00:00", "Inicio");
        assert!(passa(&info, "tudo", ""));
        assert!(!passa(&info, "avisos", ""));
        assert!(passa(&aviso, "avisos", ""));
        assert!(!passa(&aviso, "erros", ""));
        assert!(passa(&aviso, "tudo", "TESSERACT"));
        assert!(!passa(&aviso, "tudo", "outra coisa"));
        assert!(
            passa(&aviso, "tudo", "10:00:05"),
            "a busca tambem olha a hora"
        );
    }

    #[test]
    fn rotulo_do_arquivo_de_log() {
        let a = ArquivoLog {
            nome: "extrator_20261116_100000.log".into(),
            tamanho: 2048,
            data: "2026-11-16".into(),
            hora: "10:00".into(),
            atual: true,
            tipo: "sessao".into(),
        };
        assert_eq!(
            rotulo_arquivo(&a),
            "16/11/2026 10:00 · extrator_20261116_100000.log (sessão atual) · 2.0 KB"
        );
    }
}
