//! Telas Diagnostico, Configuracoes e Ajuda.

use crate::app::*;
use crate::bus::{Escolha, Tag};
use crate::model::EstadoComp;
use crate::pg_tr::combo;
use crate::theme::negrito;
use crate::widgets::*;
use eframe::egui::{self, Frame, Margin, RichText, Ui};
use serde_json::json;

fn tabela_linha<R>(
    ui: &mut Ui,
    pal: &crate::theme::Palette,
    listrada: bool,
    f: impl FnOnce(&mut Ui) -> R,
) -> R {
    Frame::new()
        .fill(if listrada { pal.surface2 } else { pal.surface })
        .inner_margin(Margin::symmetric(20, 7))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            f(ui)
        })
        .inner
}

impl App {
    pub fn ui_diagnostico(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let ocupado = self.ocupado.contains("diag");
        let escolhendo = self.ocupado.contains("escolher");
        self.coluna(ui, "diagnostico", |app, ui| {
            cabecalho_pagina(
                ui,
                &pal,
                "Diagnóstico",
                "Analise um PDF ou imagem antes de decidir qual modo usar.",
            );
            cartao(ui, &pal, |ui| {
                ui.horizontal(|ui| {
                    let w =
                        (ui.available_width() - 118.0 - 130.0 - 2.0 * ui.spacing().item_spacing.x)
                            .max(120.0);
                    campo_texto(
                        ui,
                        &pal,
                        &mut app.diag.caminho,
                        "Nenhum arquivo selecionado",
                        w,
                        false,
                    );
                    if btn("Escolher arquivo")
                        .ativo(!escolhendo)
                        .largura(118.0)
                        .mostrar(ui, &pal)
                        .clicked()
                    {
                        app.escolher_arquivos(Escolha::ArquivoDiagnostico, "diagnose", false);
                    }
                    if btn(if ocupado {
                        "Analisando…"
                    } else {
                        "Diagnosticar"
                    })
                    .primario()
                    .ocupado(ocupado)
                    .largura(130.0)
                    .mostrar(ui, &pal)
                    .clicked()
                    {
                        if app.diag.caminho.is_empty() {
                            app.notificar(
                                "Diagnóstico: escolha um arquivo primeiro.",
                                TipoAviso::Erro,
                            );
                        } else {
                            app.iniciar_acao("diag", "Analisando…");
                            let p = app.diag.caminho.clone();
                            app.chamar(Tag::Diagnosticar, "diagnose", vec![json!(p)]);
                        }
                    }
                });
            });
            if let Some(linhas) = &app.diag.linhas {
                cartao_rente(ui, &pal, |ui| {
                    for (i, (item, res)) in linhas.iter().enumerate() {
                        tabela_linha(ui, &pal, i % 2 == 1, |ui| {
                            ui.horizontal(|ui| {
                                texto_fixo(ui, 220.0, 18.0, RichText::new(item).family(negrito()));
                                ui.add(egui::Label::new(res));
                            });
                        });
                    }
                });
            }
        });
    }

    pub fn ui_configuracoes(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let salvando = self.ocupado.contains("cfg_salvar");
        let restaurando = self.ocupado.contains("cfg_restaurar");
        let msg = self.cfg.msg.clone();
        let spark = self.caps.motor_spark;
        let testando = self.ocupado.contains("spark_testar");
        self.pagina_com_barra(
            ui,
            "configuracoes",
            |app, ui| {
                cabecalho_pagina(
                    ui,
                    &pal,
                    "Configurações",
                    "Aparência e ajustes do OCR. A pasta de saída é escolhida a cada operação, nas telas Converter e Lote.",
                );
                app.card_aparencia(ui);
                cartao(ui, &pal, |ui| {
                    titulo2(ui, "Reconhecimento de texto");
                    ui.add_space(6.0);
                    let c = &mut app.cfg;
                    egui::Grid::new("cfg_grade").num_columns(3).spacing([16.0, 10.0]).show(ui, |ui| {
                        let campo = |ui: &mut Ui, rot: &str, v: &mut String| {
                            ui.vertical(|ui| {
                                rotulo(ui, rot);
                                entrada_simples(ui, &pal, v, "", 230.0);
                            });
                        };
                        let lista = |ui: &mut Ui, id: &str, rot: &str, v: &mut String, op: &[(&str, &str)]| {
                            ui.vertical(|ui| {
                                rotulo(ui, rot);
                                let o: Vec<(String, String)> = op.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
                                combo(ui, id, v, &o, 220.0);
                            });
                        };
                        campo(ui, "Idioma do OCR", &mut c.idioma);
                        campo(ui, "Resolução (DPI)", &mut c.dpi);
                        campo(ui, "PSM do Tesseract", &mut c.psm);
                        ui.end_row();
                        campo(ui, "Limite de texto nativo", &mut c.limite);
                        lista(ui, "cfg_cpu", "Uso de CPU", &mut c.cpu, &[("economico", "Econômico"), ("normal", "Normal"), ("maximo", "Máximo")]);
                        lista(ui, "cfg_pdf", "Tipo de PDF de saída", &mut c.pdf, &[("pdf", "PDF"), ("pdfa", "PDF/A (exige Ghostscript)")]);
                        ui.end_row();
                        lista(
                            ui,
                            "cfg_verb",
                            "Detalhe do log",
                            &mut c.verbosidade,
                            &[("normal", "Normal"), ("detalhado", "Detalhado"), ("depuracao", "Depuração")],
                        );
                        ui.end_row();
                    });
                });
                if spark {
                    cartao(ui, &pal, |ui| {
                        titulo2(ui, "Motor de transcrição");
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut app.cfg.motor, "local".to_string(), "Este computador");
                            ui.radio_value(&mut app.cfg.motor, "spark".to_string(), "Spark");
                            if app.cfg.motor == "spark"
                                && btn("Testar conexão").pequeno().ocupado(testando).mostrar(ui, &pal).clicked()
                            {
                                app.iniciar_acao("spark_testar", "Testando a conexão com o Spark…");
                                app.chamar(Tag::TestarSpark, "spark_testar", vec![]);
                            }
                        });
                        if !app.cfg.spark_msg.is_empty() {
                            mudo_peq(ui, &pal, &app.cfg.spark_msg);
                        }
                    });
                }
                cartao(ui, &pal, |ui| {
                    titulo2(ui, "Comportamento");
                    ui.add_space(4.0);
                    interruptor(ui, &pal, &mut app.cfg.girar, "Girar páginas automaticamente");
                    interruptor(ui, &pal, &mut app.cfg.inclinadas, "Corrigir páginas inclinadas");
                    interruptor(ui, &pal, &mut app.cfg.retomar, "Retomar OCR interrompido");
                    interruptor(ui, &pal, &mut app.cfg.abrir, "Abrir a pasta de saída ao concluir");
                });
            },
            |app, ui| {
                let aviso = if msg.is_empty() {
                    "Os ajustes do OCR só valem depois de Salvar. A aparência é aplicada na hora."
                } else {
                    msg.as_str()
                };
                mudo(ui, &pal, aviso);
                a_direita(ui, |ui| {
                    if btn(if salvando { "Salvando…" } else { "Salvar" })
                        .primario()
                        .ocupado(salvando)
                        .largura(110.0)
                        .mostrar(ui, &pal)
                        .clicked()
                    {
                        app.iniciar_acao("cfg_salvar", "Salvando…");
                        let v = app.cfg.para_json();
                        app.chamar(Tag::SalvarConfig, "save_settings", vec![v]);
                    }
                    if btn(if restaurando { "Restaurando…" } else { "Restaurar padrões" })
                        .ocupado(restaurando)
                        .mostrar(ui, &pal)
                        .clicked()
                    {
                        app.confirma = Some(Confirma {
                            titulo: "Restaurar padrões".into(),
                            corpo: "Voltar os ajustes do OCR ao padrão? A aparência (cores e tema) não muda.".into(),
                            ok: "Restaurar".into(),
                            perigo: false,
                            acao: AcaoConfirma::RestaurarPadroes,
                        });
                    }
                });
            },
        );
    }

    pub fn ui_ajuda(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let verificando = self.ajuda.verificando;
        let limpando = self.ocupado.contains("limpar");
        let abrindo = self.ocupado.contains("abrir_dados");
        let instalando = self.ajuda.instalando || self.ocupado.contains("instalar");
        let dados = self
            .boot
            .as_ref()
            .map(|b| b.data_dir.clone())
            .unwrap_or_default();
        let log_file = self
            .boot
            .as_ref()
            .map(|b| b.log_file.clone())
            .unwrap_or_default();
        self.coluna(ui, "ajuda", |app, ui| {
            cabecalho_pagina(ui, &pal, "Ajuda e instalação", "Verifique se tudo está instalado e consulte como cada modo funciona.");
            cartao_rente(ui, &pal, |ui| {
                Frame::new().inner_margin(Margin { left: 20, right: 20, top: 16, bottom: 12 }).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        titulo2(ui, "Estado da instalação");
                        a_direita(ui, |ui| {
                            if btn(if abrindo { "Abrindo…" } else { "Abrir pasta de dados" }).pequeno().ocupado(abrindo).mostrar(ui, &pal).clicked() {
                                app.iniciar_acao("abrir_dados", "Abrindo a pasta de dados…");
                                app.chamar(Tag::AbrirDados, "open_data_folder", vec![]);
                            }
                            if btn(if limpando { "Limpando…" } else { "Limpar cache e logs" }).pequeno().ocupado(limpando).mostrar(ui, &pal).clicked() {
                                app.confirma = Some(Confirma {
                                    titulo: "Limpar cache e logs".into(),
                                    corpo: format!("Remove todos os logs e o cache de retomada do OCR em:\n{dados}\nA configuração é mantida."),
                                    ok: "Limpar".into(),
                                    perigo: true,
                                    acao: AcaoConfirma::LimparDados,
                                });
                            }
                            if btn(if verificando { "Verificando…" } else { "Verificar instalação" })
                                .pequeno()
                                .primario()
                                .ocupado(verificando)
                                .mostrar(ui, &pal)
                                .clicked()
                            {
                                app.verificar_instalacao(false);
                            }
                        });
                    });
                });
                if let Some((texto, tom)) = app.ajuda.linha.clone() {
                    let (fundo, cor) = match tom {
                        Tom::Erro => (pal.danger_soft, pal.danger),
                        Tom::Aviso => (pal.warn_soft, pal.warn),
                        Tom::Ocupado => (pal.accent_soft, pal.accent),
                        Tom::Normal => (pal.ok_soft, pal.ok),
                    };
                    Frame::new().fill(fundo).inner_margin(Margin::symmetric(20, 7)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if tom == Tom::Ocupado {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::hover());
                                girar(ui, r, cor);
                            }
                            ui.add(egui::Label::new(RichText::new(texto).color(cor)));
                        });
                    });
                }
                let faltantes = app.ajuda.resultado.as_ref().map(|r| r.faltantes()).unwrap_or_default();
                if !faltantes.is_empty() && !verificando {
                    Frame::new().fill(pal.warn_soft).inner_margin(Margin::symmetric(20, 8)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.add(egui::Label::new(RichText::new(format!("Faltando: {}.", faltantes.join(", "))).color(pal.warn)));
                            a_direita(ui, |ui| {
                                if btn(if instalando { "Instalando…" } else { "Instalar o que falta" })
                                    .pequeno()
                                    .primario()
                                    .ocupado(instalando)
                                    .mostrar(ui, &pal)
                                    .clicked()
                                {
                                    app.confirma = Some(Confirma {
                                        titulo: "Instalar o que falta".into(),
                                        corpo: "As bibliotecas ausentes serão instaladas via pip e o modelo de transcrição baixado (exige internet). Continuar?".into(),
                                        ok: "Instalar".into(),
                                        perigo: false,
                                        acao: AcaoConfirma::InstalarFaltantes,
                                    });
                                }
                            });
                        });
                    });
                }
                // tabela de componentes
                let linhas: Vec<crate::model::LinhaInstalacao> = if verificando {
                    app.ajuda.parciais.clone()
                } else {
                    app.ajuda.resultado.as_ref().map(|r| r.linhas.clone()).unwrap_or_default()
                };
                if linhas.is_empty() {
                    tabela_linha(ui, &pal, false, |ui| {
                        let t = if verificando { "Verificando componentes…" } else { "Clique em “Verificar instalação”." };
                        mudo(ui, &pal, t);
                    });
                }
                for (i, l) in linhas.iter().enumerate() {
                    tabela_linha(ui, &pal, i % 2 == 1, |ui| {
                        ui.horizontal(|ui| {
                            texto_fixo(ui, 200.0, 20.0, RichText::new(&l.nome).family(negrito()));
                            let (t, c) = match l.estado {
                                EstadoComp::Ok => ("OK", Cor::Ok),
                                EstadoComp::Aviso => ("Aviso", Cor::Aviso),
                                EstadoComp::Falha => ("Falta", Cor::Erro),
                            };
                            ui.allocate_ui(egui::vec2(70.0, 20.0), |ui| {
                                ui.set_min_width(70.0);
                                chip(ui, &pal, t, c)
                            });
                            ui.add(egui::Label::new(RichText::new(&l.detalhe).color(pal.muted)).truncate());
                        });
                    });
                }
            });
            cartao(ui, &pal, |ui| {
                titulo2(ui, "Diagnóstico de arquivo");
                ui.add_space(4.0);
                mudo(
                    ui,
                    &pal,
                    "Analise um PDF ou imagem antes de decidir qual modo usar.",
                );
                ui.add_space(6.0);
                if btn("Abrir o Diagnóstico").mostrar(ui, &pal).clicked() {
                    app.mostrar(Pagina::Diagnostico);
                }
            });
            cartao(ui, &pal, |ui| {
                titulo2(ui, "Como cada modo funciona");
                ui.add_space(6.0);
                for (t, d) in [
                    ("Conversão direta", "Usa o MarkItDown e não faz OCR."),
                    ("OCR automático", "Mantém o texto nativo e aplica OCR só nas páginas sem texto suficiente."),
                    ("OCR forçado", "Reconhece todas as páginas. Mais lento."),
                    ("PDF pesquisável", "Usa o OCRmyPDF: mantém a aparência do PDF e adiciona uma camada de texto."),
                    ("Segurança", "O arquivo original nunca é sobrescrito. Documentos e áudios não saem deste computador."),
                ] {
                    ui.add(egui::Label::new(RichText::new(t).family(negrito())));
                    ui.add(egui::Label::new(RichText::new(d).color(pal.muted)));
                    ui.add_space(4.0);
                }
                mudo_peq(ui, &pal, &format!("Pasta de dados: {dados}  ·  Log desta sessão: {log_file}"));
            });
        });
    }
}
