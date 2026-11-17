//! Telas Converter e Lote.

use crate::app::*;
use crate::bus::{Bus, Escolha, Msg, Tag};
use crate::fmt;
use crate::widgets::*;
use eframe::egui::{self, vec2, Color32, RichText, ScrollArea, Ui};
use serde_json::{json, Value};

const FORMATOS: [(&str, &str); 3] = [("md", "Markdown"), ("txt", "TXT"), ("both", "Ambos")];

fn texto_aceita(aceita: &str) -> &'static str {
    match aceita {
        "pdf" => "Aceita arquivos PDF.",
        "direct" => "Aceita PDF, Word, PowerPoint, Excel, HTML, TXT, CSV, JSON e XML.",
        "images" => "Aceita PNG, JPG, TIFF, BMP e WEBP.",
        _ => "",
    }
}

/// Filtros do seletor nativo de arquivos por tipo de aceite.
pub fn filtro_arquivos(aceita: &str, midia: &[String]) -> (String, Vec<String>) {
    let sem_ponto = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    match aceita {
        "images" => (
            "Imagens".into(),
            sem_ponto(&["png", "jpg", "jpeg", "tif", "tiff", "bmp", "webp"]),
        ),
        "direct" => (
            "Documentos".into(),
            sem_ponto(&[
                "pdf", "docx", "pptx", "xlsx", "html", "htm", "txt", "csv", "json", "xml",
            ]),
        ),
        "media" => (
            "Áudio e vídeo".into(),
            midia
                .iter()
                .map(|e| e.trim_start_matches('.').to_string())
                .collect(),
        ),
        "docs" => (
            "Documentos e imagens".into(),
            sem_ponto(&[
                "pdf", "docx", "pptx", "xlsx", "html", "htm", "txt", "csv", "json", "xml", "png",
                "jpg", "jpeg", "tif", "tiff", "bmp", "webp",
            ]),
        ),
        "tudo" => {
            let mut e = sem_ponto(&[
                "pdf", "docx", "pptx", "xlsx", "html", "htm", "txt", "csv", "json", "xml", "png",
                "jpg", "jpeg", "tif", "tiff", "bmp", "webp",
            ]);
            e.extend(midia.iter().map(|x| x.trim_start_matches('.').to_string()));
            ("Documentos, imagens, áudio e vídeo".into(), e)
        }
        "diagnose" => (
            "PDF e imagens".into(),
            sem_ponto(&["pdf", "png", "jpg", "jpeg", "tif", "tiff", "bmp", "webp"]),
        ),
        _ => ("PDF".into(), sem_ponto(&["pdf"])),
    }
}

impl App {
    /// Seletor nativo de arquivos em thread de fundo (so abre quando o usuario clica).
    pub fn escolher_arquivos(&mut self, e: Escolha, aceita: &str, multiplos: bool) {
        if !self.ocupado.insert("escolher") {
            return;
        }
        let midia = self
            .boot
            .as_ref()
            .map(|b| b.ext.media.clone())
            .unwrap_or_default();
        let (nome, exts) = filtro_arquivos(aceita, &midia);
        let bus: Bus = self.bus.clone();
        let r = std::thread::Builder::new()
            .name("escolher".into())
            .spawn(move || {
                let refs: Vec<&str> = exts.iter().map(String::as_str).collect();
                let d = rfd::FileDialog::new()
                    .add_filter(&nome, &refs)
                    .add_filter("Todos os arquivos", &["*"]);
                let escolhidos = if multiplos {
                    d.pick_files().unwrap_or_default()
                } else {
                    d.pick_file().into_iter().collect()
                };
                bus.enviar(Msg::Arquivos(e, escolhidos));
            });
        if r.is_err() {
            self.ocupado.remove("escolher");
        }
    }

    pub fn escolher_pasta(&mut self, e: Escolha) {
        if !self.ocupado.insert("escolher") {
            return;
        }
        let bus = self.bus.clone();
        let r = std::thread::Builder::new()
            .name("escolher".into())
            .spawn(move || {
                bus.enviar(Msg::Pasta(e, rfd::FileDialog::new().pick_folder()));
            });
        if r.is_err() {
            self.ocupado.remove("escolher");
        }
    }

    pub fn modo_conv(&self) -> Option<crate::model::Modo> {
        self.boot
            .as_ref()?
            .modos
            .iter()
            .find(|m| m.key == self.conv.modo)
            .cloned()
    }

    pub fn extensoes_do_modo(&self, aceita: &str) -> Vec<String> {
        let Some(b) = &self.boot else {
            return Vec::new();
        };
        match aceita {
            "direct" => b.ext.direct.clone(),
            "images" => b.ext.images.clone(),
            _ => b.ext.pdf.clone(),
        }
    }

    pub fn adicionar_conv(&mut self, caminhos: Vec<String>) {
        let Some(modo) = self.modo_conv() else { return };
        let exts = self.extensoes_do_modo(&modo.accepts);
        let mut ignorados = 0;
        for p in caminhos {
            if !exts.contains(&fmt::ext_of(&p)) {
                ignorados += 1;
                continue;
            }
            if !self.conv.arquivos.contains(&p) {
                self.conv.arquivos.push(p);
            }
        }
        if ignorados > 0 {
            self.aviso(
                &format!(
                    "{} (formato incompatível com este modo).",
                    fmt::plural(ignorados, "arquivo ignorado", "arquivos ignorados")
                ),
                TipoAviso::Aviso,
            );
        }
    }

    pub fn iniciar_conversao(&mut self, confirmado: bool) {
        if self.job_rodando || self.ocupado.contains("converter") {
            return;
        }
        let Some(modo) = self.modo_conv() else { return };
        if self.conv.arquivos.is_empty() {
            self.notificar("Selecione ao menos um arquivo.", TipoAviso::Erro);
            return;
        }
        let params = json!({
            "mode": modo.key,
            "files": self.conv.arquivos,
            "same_folder": self.conv.mesma_pasta,
            "output_folder": self.conv.pasta_saida,
            "pages": if modo.pages { self.conv.paginas.clone() } else { String::new() },
            "format": FORMATOS[self.conv.formato.min(2)].0,
            "combine_images": self.conv.juntar,
            "confirm_signed": confirmado,
        });
        self.iniciar_acao("converter", "Iniciando a conversão…");
        self.chamar(
            Tag::IniciarConversao(confirmado),
            "start_convert",
            vec![params],
        );
    }

    pub fn ui_converter(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let Some(modo) = self.modo_conv() else { return };
        let modos: Vec<(String, String, String)> = self
            .boot
            .as_ref()
            .map(|b| {
                b.modos
                    .iter()
                    .map(|m| (m.key.clone(), m.label.clone(), m.description.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let exts = self.extensoes_do_modo(&modo.accepts);
        let n = self.conv.arquivos.len();
        let ruins = self
            .conv
            .arquivos
            .iter()
            .filter(|p| !exts.contains(&fmt::ext_of(p)))
            .count();
        let resumo = if n == 0 {
            "Selecione ao menos um arquivo.".to_string()
        } else if ruins > 0 {
            format!(
                "{} com “{}”. Remova-os ou troque o modo.",
                fmt::plural(ruins, "arquivo incompatível", "arquivos incompatíveis"),
                modo.label
            )
        } else {
            format!("{} · {}", fmt::plural(n, "arquivo", "arquivos"), modo.label)
        };
        let sobre = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let rodando = self.job_rodando;
        let iniciando = self.ocupado.contains("converter");

        self.pagina_com_barra(
            ui,
            "converter",
            |app, ui| {
                cabecalho_pagina(
                    ui,
                    &pal,
                    "Converter",
                    "Transforme PDFs, imagens e documentos em texto pesquisável, Markdown ou TXT.",
                );
                cartao(ui, &pal, |ui| {
                    titulo2(ui, "1. O que você quer fazer?");
                    ui.add_space(6.0);
                    if let Some(k) = grade_modos(ui, &pal, &modos, &app.conv.modo) {
                        app.conv.modo = k;
                    }
                });
                cartao(ui, &pal, |ui| {
                    ui.horizontal(|ui| {
                        titulo2(ui, "2. Arquivos");
                        a_direita(ui, |ui| {
                            if n > 0 {
                                mudo(ui, &pal, &fmt::plural(n, "arquivo", "arquivos"));
                            }
                        });
                    });
                    ui.add_space(6.0);
                    let r = area_soltar(
                        ui,
                        &pal,
                        sobre,
                        false,
                        "Clique para selecionar",
                        "ou arraste os arquivos para cá",
                        texto_aceita(&modo.accepts),
                    );
                    if r.clicked() && !app.ocupado.contains("escolher") {
                        app.escolher_arquivos(Escolha::ArquivosConverter, &modo.accepts, true);
                    }
                    if n > 0 {
                        ui.add_space(6.0);
                        let mut remover: Option<usize> = None;
                        ScrollArea::vertical()
                            .id_salt("conv_arqs")
                            .max_height(240.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                for (i, p) in app.conv.arquivos.iter().enumerate() {
                                    let ruim = !exts.contains(&fmt::ext_of(p));
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(fmt::base_name(p)).color(if ruim {
                                                    pal.danger
                                                } else {
                                                    pal.text
                                                }),
                                            )
                                            .truncate(),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(fmt::dir_name(p))
                                                    .size(12.5)
                                                    .color(pal.muted),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(p);
                                        if ruim {
                                            chip(ui, &pal, "incompatível", Cor::Erro);
                                        }
                                        a_direita(ui, |ui| {
                                            if btn("×")
                                                .link()
                                                .pequeno()
                                                .mostrar(ui, &pal)
                                                .on_hover_text("Remover")
                                                .clicked()
                                            {
                                                remover = Some(i);
                                            }
                                        });
                                    });
                                }
                            });
                        if let Some(i) = remover {
                            app.conv.arquivos.remove(i);
                        }
                        ui.horizontal(|ui| {
                            a_direita(ui, |ui| {
                                if btn("Limpar lista").link().mostrar(ui, &pal).clicked() {
                                    app.conv.arquivos.clear();
                                }
                            });
                        });
                    }
                });
                cartao(ui, &pal, |ui| {
                    titulo2(ui, "3. Saída");
                    ui.add_space(4.0);
                    interruptor(
                        ui,
                        &pal,
                        &mut app.conv.mesma_pasta,
                        "Salvar na mesma pasta de cada arquivo de origem",
                    );
                    if !app.conv.mesma_pasta {
                        ui.add_space(4.0);
                        let escolhendo = app.ocupado.contains("escolher");
                        let mut p = app.conv.pasta_saida.clone();
                        if campo_caminho(
                            ui,
                            &pal,
                            &mut p,
                            "Pasta de saída",
                            "Escolher pasta",
                            false,
                            !escolhendo,
                        ) {
                            app.escolher_pasta(Escolha::PastaSaida);
                        }
                    }
                    if modo.pages || modo.format || modo.combine {
                        ui.add_space(8.0);
                        ui.horizontal_top(|ui| {
                            if modo.pages {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        rotulo(ui, "Páginas");
                                        mudo(ui, &pal, "(vazio = todas)");
                                    });
                                    entrada_simples(
                                        ui,
                                        &pal,
                                        &mut app.conv.paginas,
                                        "Ex.: 1-10, 15, 20-25",
                                        260.0,
                                    );
                                });
                                ui.add_space(14.0);
                            }
                            if modo.format {
                                ui.vertical(|ui| {
                                    rotulo(ui, "Formato de saída");
                                    let nomes: Vec<&str> = FORMATOS.iter().map(|f| f.1).collect();
                                    if let Some(i) = segmentado(ui, &pal, app.conv.formato, &nomes)
                                    {
                                        app.conv.formato = i;
                                    }
                                });
                            }
                        });
                        if modo.combine {
                            ui.add_space(6.0);
                            ui.checkbox(
                                &mut app.conv.juntar,
                                "Juntar todas as imagens em um único arquivo",
                            );
                        }
                    }
                });
                ui.horizontal(|ui| {
                    mudo_peq(ui, &pal, "Não sabe qual modo usar?");
                    if btn("Diagnosticar um arquivo")
                        .link()
                        .pequeno()
                        .mostrar(ui, &pal)
                        .clicked()
                    {
                        app.mostrar(Pagina::Diagnostico);
                    }
                });
            },
            |app, ui| {
                mudo(ui, &pal, &resumo);
                a_direita(ui, |ui| {
                    if btn(if iniciando {
                        "Iniciando…"
                    } else {
                        "Iniciar conversão"
                    })
                    .primario()
                    .ativo(!rodando)
                    .ocupado(iniciando)
                    .largura(150.0)
                    .mostrar(ui, &pal)
                    .clicked()
                    {
                        app.iniciar_conversao(false);
                    }
                    if rodando && btn("Cancelar").perigo().mostrar(ui, &pal).clicked() {
                        app.chamar(Tag::CancelarTrabalho, "cancel_job", vec![]);
                        app.definir_status("Cancelando…", Tom::Ocupado);
                    }
                });
            },
        );
    }

    // ------------------------------------------------------------------ lote

    fn lote_params(&self, confirmado: bool) -> Value {
        json!({
            "mode": self.lote.modo,
            "input_folder": self.lote.entrada,
            "output_folder": self.lote.saida,
            "recursive": self.lote.recursivo,
            "confirm_signed": confirmado,
        })
    }

    /// Passo 1: analisa a pasta (quantos arquivos, assinaturas); depois pede a confirmacao.
    pub fn iniciar_lote(&mut self) {
        if self.job_rodando || self.ocupado.contains("lote") {
            return;
        }
        if self.lote.entrada.trim().is_empty() {
            self.notificar("Escolha uma pasta de entrada válida.", TipoAviso::Erro);
            return;
        }
        self.iniciar_acao("lote", "Analisando a pasta…");
        let p = self.lote_params(false);
        self.chamar(Tag::AnalisarLote, "scan_batch", vec![p]);
    }

    pub fn depois_da_varredura(&mut self, v: Value) {
        let n = v.get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
        if n == 0 {
            self.notificar(
                "Nada para processar: nenhum arquivo compatível foi encontrado nessa pasta.",
                TipoAviso::Erro,
            );
            return;
        }
        let saida = v
            .get("output_folder")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        self.lote.varredura = Some(v);
        self.confirma = Some(Confirma {
            titulo: "Iniciar o lote?".into(),
            corpo: format!(
                "{}.\nSaída: {saida}",
                fmt::plural(n, "arquivo encontrado", "arquivos encontrados")
            ),
            ok: "Iniciar lote".into(),
            perigo: false,
            acao: AcaoConfirma::IniciarLote,
        });
    }

    pub fn depois_de_confirmar_lote(&mut self) {
        let assinados: Vec<String> = self
            .lote
            .varredura
            .as_ref()
            .and_then(|v| v.get("signed"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if assinados.is_empty() {
            self.iniciar_lote_real(false);
            return;
        }
        let mut corpo = assinados
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        if assinados.len() > 12 {
            corpo.push_str(&format!("\n… e mais {}", assinados.len() - 12));
        }
        self.confirma = Some(Confirma {
            titulo: "Assinatura digital detectada".into(),
            corpo: format!("{corpo}\nO OCR invalida a assinatura nesses arquivos. O original nunca é sobrescrito. Continuar?"),
            ok: "Continuar".into(),
            perigo: false,
            acao: AcaoConfirma::LoteAssinados,
        });
    }

    pub fn iniciar_lote_real(&mut self, confirmado: bool) {
        self.iniciar_acao("lote", "Iniciando o lote…");
        let p = self.lote_params(confirmado);
        self.chamar(Tag::IniciarLote, "start_batch", vec![p]);
    }

    pub fn ui_lote(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let modos: Vec<(String, String, String)> = self
            .boot
            .as_ref()
            .map(|b| {
                b.modos_lote
                    .iter()
                    .map(|m| (m.key.clone(), m.label.clone(), m.description.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let rodando = self.job_rodando;
        let ocupado = self.ocupado.contains("lote");
        let escolhendo = self.ocupado.contains("escolher");
        let resumo = self.lote.resumo.clone();
        self.pagina_com_barra(
            ui,
            "lote",
            |app, ui| {
                cabecalho_pagina(
                    ui,
                    &pal,
                    "Lote",
                    "Processe todos os arquivos de uma pasta de uma só vez.",
                );
                cartao(ui, &pal, |ui| {
                    titulo2(ui, "Pastas");
                    ui.add_space(6.0);
                    rotulo(ui, "Pasta de entrada");
                    let mut e = app.lote.entrada.clone();
                    if campo_caminho(
                        ui,
                        &pal,
                        &mut e,
                        "Nenhuma pasta escolhida",
                        "Escolher pasta",
                        false,
                        !escolhendo,
                    ) {
                        app.escolher_pasta(Escolha::PastaLoteEntrada);
                    }
                    ui.add_space(4.0);
                    interruptor(ui, &pal, &mut app.lote.recursivo, "Incluir subpastas");
                    ui.add_space(6.0);
                    rotulo(ui, "Pasta de saída");
                    let mut s = app.lote.saida.clone();
                    if campo_caminho(
                        ui,
                        &pal,
                        &mut s,
                        "Por padrão: Extrator_saida dentro da pasta de entrada",
                        "Escolher pasta",
                        true,
                        !escolhendo,
                    ) {
                        app.escolher_pasta(Escolha::PastaLoteSaida);
                    }
                    app.lote.saida = s;
                });
                cartao(ui, &pal, |ui| {
                    titulo2(ui, "O que fazer com os arquivos");
                    ui.add_space(6.0);
                    if let Some(k) = grade_modos(ui, &pal, &modos, &app.lote.modo) {
                        app.lote.modo = k;
                    }
                });
            },
            |app, ui| {
                mudo(ui, &pal, &resumo);
                a_direita(ui, |ui| {
                    if btn(if ocupado {
                        "Aguarde…"
                    } else {
                        "Iniciar lote"
                    })
                    .primario()
                    .ativo(!rodando)
                    .ocupado(ocupado)
                    .largura(130.0)
                    .mostrar(ui, &pal)
                    .clicked()
                    {
                        app.iniciar_lote();
                    }
                    if rodando && btn("Cancelar").perigo().mostrar(ui, &pal).clicked() {
                        app.chamar(Tag::CancelarTrabalho, "cancel_job", vec![]);
                        app.definir_status("Cancelando…", Tom::Ocupado);
                    }
                });
            },
        );
        let _ = (vec2(0.0, 0.0), Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtros_por_tipo() {
        let (n, e) = filtro_arquivos("pdf", &[]);
        assert_eq!((n.as_str(), e), ("PDF", vec!["pdf".to_string()]));
        let (_, e) = filtro_arquivos("media", &[".mp3".to_string(), ".mp4".to_string()]);
        assert_eq!(e, ["mp3", "mp4"]);
        assert!(filtro_arquivos("images", &[])
            .1
            .contains(&"webp".to_string()));
        assert!(filtro_arquivos("direct", &[])
            .1
            .contains(&"docx".to_string()));
    }
}
