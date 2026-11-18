//! Tela Inicio: o que importa em primeiro lugar (converter, transcrever, soltar arquivos,
//! o que foi feito recentemente e a saude do computador), com roteamento por extensao.

use crate::app::*;
use crate::bus::{Escolha, Tag};
use crate::fmt;
use crate::icons::{self, Icone};
use crate::model::{EstadoComp, Execucao, ResultadoInstalacao};
use crate::rotas::{self, Roteamento};
use crate::theme::{negrito, Palette, RAIO};
use crate::widgets::*;
use eframe::egui::{
    self, vec2, CornerRadius, Frame, Margin, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui,
};
use serde_json::json;

/// Quantas execucoes aparecem em "Recentes".
pub const LIMITE_RECENTES: usize = 5;

/// Ferramentas internas que nao interessam a quem so quer converter ou transcrever.
const FERRAMENTAS_INTERNAS: [&str; 4] = ["verificacao", "instalacao", "modelo", "limpeza"];

/// Mensagem inline da tela Inicio (nunca janela): texto, tom e atalhos para outras telas.
pub struct InfoInicio {
    pub texto: String,
    pub tom: Tom,
    pub links: Vec<(&'static str, Pagina)>,
}

/// Estado da tela Inicio.
#[derive(Default)]
pub struct Inicio {
    /// Mistura de documentos e audio/video aguardando a escolha do usuario.
    pub pergunta: Option<Roteamento>,
    pub info: Option<InfoInicio>,
}

// ----------------------------------------------------------------- logica pura

/// As ultimas execucoes de uso (conversoes, lotes, transcricoes e diagnosticos), da mais nova
/// para a mais antiga. Sem horario, vale a ordem em que o motor enviou (do mais novo ao mais velho).
pub fn recentes(lista: &[Execucao], n: usize) -> Vec<&Execucao> {
    let mut v: Vec<&Execucao> = lista
        .iter()
        .filter(|e| !FERRAMENTAS_INTERNAS.contains(&e.ferramenta.as_str()))
        .collect();
    let quando = |e: &Execucao| e.criada_em.or(e.inicio_em);
    v.sort_by(|a, b| match (quando(a), quando(b)) {
        (Some(x), Some(y)) => y.partial_cmp(&x).unwrap_or(std::cmp::Ordering::Equal),
        _ => std::cmp::Ordering::Equal,
    });
    v.truncate(n);
    v
}

/// Nome do arquivo para mostrar: o ultimo trecho do caminho; sem entrada, o rotulo da execucao.
pub fn nome_exibicao(e: &Execucao) -> String {
    let base = fmt::base_name(&e.entrada).trim();
    if !base.is_empty() {
        base.to_string()
    } else if !e.rotulo.is_empty() {
        e.rotulo.clone()
    } else {
        "Execução".to_string()
    }
}

/// Linha de estado do computador na tela Inicio.
#[derive(Clone, Debug, PartialEq)]
pub struct EstadoPc {
    pub texto: String,
    pub tom: Tom,
    /// Ha o que ver na tela Ajuda e instalacao.
    pub detalhes: bool,
}

/// Itens que o programa baixa sozinho no primeiro uso: nao sao problema.
fn baixa_no_primeiro_uso(nome: &str) -> bool {
    let n = nome.to_lowercase();
    n.contains("modelo") || n.contains("ffmpeg")
}

pub fn estado_computador(r: Option<&ResultadoInstalacao>, verificando: bool) -> EstadoPc {
    let Some(r) = r else {
        return EstadoPc {
            texto: if verificando {
                "Verificando o computador…".into()
            } else {
                "Estado do computador ainda não verificado.".into()
            },
            tom: Tom::Ocupado,
            detalhes: !verificando,
        };
    };
    let mut avisos = 0;
    let mut falhas = 0;
    let mut a_baixar = false;
    for l in r.linhas.iter().filter(|l| l.estado != EstadoComp::Ok) {
        if baixa_no_primeiro_uso(&l.nome) {
            a_baixar = true;
        } else if l.estado == EstadoComp::Falha {
            falhas += 1;
        } else {
            avisos += 1;
        }
    }
    if falhas > 0 {
        let n = falhas + avisos;
        EstadoPc {
            texto: if n == 1 {
                "1 item precisa de atenção: ver detalhes".to_string()
            } else {
                format!("{n} itens precisam de atenção: ver detalhes")
            },
            tom: Tom::Aviso,
            detalhes: true,
        }
    } else if avisos > 0 {
        EstadoPc {
            texto: format!("{}: ver detalhes", fmt::plural(avisos, "aviso", "avisos")),
            tom: Tom::Aviso,
            detalhes: true,
        }
    } else if a_baixar {
        EstadoPc {
            texto: "Instalação OK. O modelo de transcrição é baixado no primeiro uso.".into(),
            tom: Tom::Normal,
            detalhes: false,
        }
    } else {
        EstadoPc {
            texto: "Instalação OK. Tudo pronto para usar.".into(),
            tom: Tom::Normal,
            detalhes: false,
        }
    }
}

// ----------------------------------------------------------------- componentes

/// Textos de um cartao de acao grande.
struct TextosAcao<'a> {
    icone: Icone,
    titulo: &'a str,
    descricao: &'a str,
    botao: &'a str,
    atalho: &'a str,
}

/// Cartao de acao grande: icone, titulo, uma linha de descricao, botao principal e atalho.
/// Devolve (botao principal clicado, atalho clicado).
fn cartao_acao(ui: &mut Ui, pal: &Palette, largura: f32, t: &TextosAcao) -> (bool, bool) {
    let mut cliques = (false, false);
    Frame::new()
        .fill(pal.surface)
        .stroke(Stroke::new(1.0, pal.border))
        .corner_radius(CornerRadius::same(RAIO))
        .inner_margin(Margin::symmetric(20, 16))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.set_width(largura - 42.0);
                ui.set_min_height(102.0);
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
                    ui.painter()
                        .rect_filled(r, CornerRadius::same(13), pal.accent_soft);
                    icons::desenhar(
                        ui.painter(),
                        Rect::from_center_size(r.center(), vec2(26.0, 26.0)),
                        t.icone,
                        pal.accent,
                    );
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.add(
                            egui::Label::new(RichText::new(t.titulo).size(17.0).family(negrito()))
                                .selectable(false),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(t.descricao).size(13.0).color(pal.muted),
                            )
                            .selectable(false),
                        );
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    cliques.0 = btn(t.botao).primario().mostrar(ui, pal).clicked();
                    cliques.1 = btn(t.atalho).link().pequeno().mostrar(ui, pal).clicked();
                });
            });
        });
    cliques
}

/// Area grande para soltar arquivos (borda tracejada); tambem aceita clique.
fn area_soltar_grande(
    ui: &mut Ui,
    pal: &Palette,
    sobre: bool,
    titulo: &str,
    sub: &str,
) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 104.0), Sense::click());
    let destaque = sobre || resp.hovered();
    let (borda, fundo) = if destaque {
        (pal.accent, pal.accent_soft)
    } else {
        (pal.border_strong, pal.surface2)
    };
    ui.painter().rect_filled(r, CornerRadius::same(RAIO), fundo);
    let cantos = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    ui.painter().extend(Shape::dashed_line(
        &cantos,
        Stroke::new(1.6, borda),
        7.0,
        5.0,
    ));
    let cx = r.center().x;
    icons::desenhar(
        ui.painter(),
        Rect::from_center_size(Pos2::new(cx, r.top() + 24.0), vec2(30.0, 30.0)),
        Icone::Enviar,
        if destaque { pal.accent } else { pal.muted },
    );
    ui.painter().text(
        Pos2::new(cx, r.top() + 56.0),
        egui::Align2::CENTER_CENTER,
        titulo,
        egui::FontId::new(17.0, negrito()),
        pal.text,
    );
    ui.painter().text(
        Pos2::new(cx, r.top() + 79.0),
        egui::Align2::CENTER_CENTER,
        sub,
        egui::FontId::new(13.0, egui::FontFamily::Proportional),
        pal.muted,
    );
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn bolinha(ui: &mut Ui, cor: egui::Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 4.5, cor);
}

enum AcaoLinha {
    VerLog(Box<Execucao>),
    Abrir(String),
}

// -------------------------------------------------------------------------- app

impl App {
    /// Recebe arquivos soltos ou escolhidos na tela Inicio e decide o destino pela extensao.
    pub fn rotear_arquivos(&mut self, caminhos: Vec<String>) {
        let Some(b) = &self.boot else { return };
        let r = rotas::classificar(&caminhos, &b.ext);
        self.inicio.pergunta = None;
        self.inicio.info = None;
        if r.sem_aproveitaveis() {
            self.inicio.info = Some(InfoInicio {
                texto: "Nenhum desses arquivos pode ser processado. Use PDF, Word, Excel, PowerPoint, imagens, áudio ou vídeo.".into(),
                tom: Tom::Aviso,
                links: Vec::new(),
            });
            return;
        }
        if r.mistura() {
            self.inicio.pergunta = Some(r);
            return;
        }
        if !r.ignorados.is_empty() {
            self.aviso(
                &format!(
                    "{} (formato não compatível).",
                    fmt::plural(r.ignorados.len(), "arquivo ignorado", "arquivos ignorados")
                ),
                TipoAviso::Aviso,
            );
        }
        if r.documentos() > 0 {
            self.enviar_documentos(&r, true);
        } else {
            self.enviar_midia(&r, true);
        }
    }

    /// Coloca os documentos no Converter (modo padrao do tipo). Devolve quantos entraram.
    fn enviar_documentos(&mut self, r: &Roteamento, abrir: bool) -> usize {
        let Some((grupo, arquivos, sobra)) = r.documentos_do_grupo_principal() else {
            return 0;
        };
        let modos = self
            .boot
            .as_ref()
            .map(|b| b.modos.clone())
            .unwrap_or_default();
        let Some(chave) = rotas::modo_para(grupo, &modos) else {
            self.aviso(
                "Este programa não tem um modo de conversão para esse tipo de arquivo.",
                TipoAviso::Aviso,
            );
            return 0;
        };
        self.conv.modo = chave;
        // Arquivos antigos que o novo modo nao aceita sairiam marcados como incompativeis.
        let aceitos = self
            .modo_conv()
            .map(|m| self.extensoes_do_modo(&m.accepts))
            .unwrap_or_default();
        self.conv
            .arquivos
            .retain(|p| aceitos.contains(&fmt::ext_of(p)));
        let antes = self.conv.arquivos.len();
        self.adicionar_conv(arquivos);
        let entraram = self.conv.arquivos.len().saturating_sub(antes);
        if sobra > 0 {
            self.aviso(
                &format!(
                    "{} de outro tipo ficou de fora: o Converter usa um modo por vez. Converta-o depois, trocando o modo.",
                    fmt::plural(sobra, "arquivo", "arquivos")
                ),
                TipoAviso::Aviso,
            );
        }
        if abrir {
            self.mostrar(Pagina::Converter);
        }
        entraram
    }

    fn enviar_midia(&mut self, r: &Roteamento, abrir: bool) -> usize {
        let antes = self.tr.itens.len();
        self.adicionar_tr(r.midia.clone());
        if abrir {
            self.mostrar(Pagina::Transcricao);
        }
        self.tr.itens.len().saturating_sub(antes)
    }

    fn separar_automaticamente(&mut self, r: &Roteamento) {
        let docs = self.enviar_documentos(r, false);
        let midia = self.enviar_midia(r, false);
        self.inicio.pergunta = None;
        self.inicio.info = Some(InfoInicio {
            texto: format!(
                "Separamos os arquivos: {} no Converter e {} na Transcrição. Nada começou ainda.",
                fmt::plural(docs, "documento", "documentos"),
                fmt::plural(midia, "áudio ou vídeo", "áudios e vídeos"),
            ),
            tom: Tom::Normal,
            links: vec![
                ("Abrir Converter", Pagina::Converter),
                ("Abrir Transcrição", Pagina::Transcricao),
            ],
        });
    }

    pub fn ui_inicio(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        let sobre = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        let escolhendo = self.ocupado.contains("escolher");
        let recentes: Vec<Execucao> = recentes(&self.exec.lista.execucoes, LIMITE_RECENTES)
            .into_iter()
            .cloned()
            .collect();
        let pc = estado_computador(self.ajuda.resultado.as_ref(), self.ajuda.verificando);
        self.coluna(ui, "inicio", |app, ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    cabecalho_pagina(
                        ui,
                        &pal,
                        "O que você quer fazer?",
                        "Tudo é processado neste computador.",
                    );
                });
                a_direita(ui, |ui| {
                    if pc.detalhes {
                        if btn(&pc.texto).link().pequeno().mostrar(ui, &pal).clicked() {
                            app.mostrar(Pagina::Ajuda);
                        }
                    } else {
                        ui.add(egui::Label::new(
                            RichText::new(&pc.texto).size(12.5).color(pal.muted),
                        ));
                    }
                    let cor = match pc.tom {
                        Tom::Normal => pal.ok,
                        Tom::Aviso => pal.warn,
                        Tom::Erro => pal.danger,
                        Tom::Ocupado => pal.muted,
                    };
                    bolinha(ui, cor);
                });
            });

            // 1) as duas acoes principais
            let gap = 14.0;
            let w = ((ui.available_width() - gap) / 2.0).floor();
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                let (a, b) = cartao_acao(ui, &pal, w, &TextosAcao { icone: Icone::Converter, titulo: "Converter documentos", descricao: "PDF, Word, Excel, PowerPoint e imagens", botao: "Escolher documentos", atalho: "Abrir o Converter" });
                if a && !escolhendo {
                    app.escolher_arquivos(Escolha::ArquivosInicio, "docs", true);
                }
                if b {
                    app.mostrar(Pagina::Converter);
                }
                let (a, b) = cartao_acao(ui, &pal, w, &TextosAcao { icone: Icone::Microfone, titulo: "Transcrever áudio e vídeo", descricao: "MP3, WAV, MP4 e outros formatos", botao: "Escolher áudio ou vídeo", atalho: "Abrir a Transcrição" });
                if a && !escolhendo {
                    app.escolher_arquivos(Escolha::ArquivosInicio, "media", true);
                }
                if b {
                    app.mostrar(Pagina::Transcricao);
                }
            });
            ui.add_space(8.0);

            // 2) soltar ou escolher, com roteamento por extensao
            let r = area_soltar_grande(
                ui,
                &pal,
                sobre,
                "Solte arquivos aqui ou clique para escolher",
                "Documentos e imagens vão para Converter; áudio e vídeo vão para Transcrição.",
            );
            if r.clicked() && !escolhendo {
                app.escolher_arquivos(Escolha::ArquivosInicio, "tudo", true);
            }
            app.ui_faixas_inicio(ui);
            ui.add_space(14.0);

            // 3) recentes
            ui.horizontal(|ui| {
                titulo2(ui, "Recentes");
                a_direita(ui, |ui| {
                    if btn("Ver todas as execuções")
                        .link()
                        .pequeno()
                        .mostrar(ui, &pal)
                        .clicked()
                    {
                        app.mostrar(Pagina::Execucoes);
                    }
                });
            });
            ui.add_space(4.0);
            if recentes.is_empty() {
                cartao(ui, &pal, |ui| {
                    ui.add(egui::Label::new(
                        RichText::new("Nenhuma execução ainda").family(negrito()),
                    ));
                    mudo(
                        ui,
                        &pal,
                        "Solte um arquivo acima ou use um dos botões. O que você fizer aparece aqui.",
                    );
                });
            } else {
                let mut acao: Option<AcaoLinha> = None;
                let agora = fmt::agora_unix();
                cartao_rente(ui, &pal, |ui| {
                    for (i, ex) in recentes.iter().enumerate() {
                        if let Some(a) = app.linha_recente(ui, ex, i % 2 == 1, agora) {
                            acao = Some(a);
                        }
                    }
                });
                match acao {
                    Some(AcaoLinha::VerLog(ex)) => app.abrir_log_da_execucao(&ex),
                    Some(AcaoLinha::Abrir(p)) => {
                        app.chamar(Tag::AbrirCaminho, "open_path", vec![json!(p)]);
                    }
                    None => {}
                }
            }
            ui.add_space(6.0);

            // 4) atalhos secundarios
            ui.horizontal(|ui| {
                mudo_peq(ui, &pal, "Atalhos:");
                for (rot, pg) in [
                    ("Lote", Pagina::Lote),
                    ("Diagnóstico", Pagina::Diagnostico),
                    ("Registro", Pagina::Log),
                ] {
                    if btn(rot).link().pequeno().mostrar(ui, &pal).clicked() {
                        app.mostrar(pg);
                    }
                }
            });
        });
    }

    /// Faixas inline sob a area de soltar: pergunta (mistura) e informacao.
    fn ui_faixas_inicio(&mut self, ui: &mut Ui) {
        let pal = self.pal;
        if let Some(r) = self.inicio.pergunta.clone() {
            ui.add_space(8.0);
            let mut decisao: Option<u8> = None;
            faixa(ui, pal.accent_soft, pal.accent, |ui| {
                ui.add(egui::Label::new(
                    RichText::new(format!(
                        "Você trouxe {} e {}. Eles seguem por telas diferentes.",
                        fmt::plural(r.documentos(), "documento", "documentos"),
                        fmt::plural(
                            r.midia.len(),
                            "arquivo de áudio ou vídeo",
                            "arquivos de áudio ou vídeo"
                        ),
                    ))
                    .family(negrito()),
                ));
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if btn("Separar automaticamente")
                        .primario()
                        .pequeno()
                        .mostrar(ui, &pal)
                        .clicked()
                    {
                        decisao = Some(0);
                    }
                    if btn("Só os documentos")
                        .pequeno()
                        .mostrar(ui, &pal)
                        .clicked()
                    {
                        decisao = Some(1);
                    }
                    if btn("Só áudio e vídeo")
                        .pequeno()
                        .mostrar(ui, &pal)
                        .clicked()
                    {
                        decisao = Some(2);
                    }
                    if btn("Cancelar").link().pequeno().mostrar(ui, &pal).clicked() {
                        decisao = Some(3);
                    }
                });
            });
            match decisao {
                Some(0) => self.separar_automaticamente(&r),
                Some(1) => {
                    self.inicio.pergunta = None;
                    self.enviar_documentos(&r, true);
                }
                Some(2) => {
                    self.inicio.pergunta = None;
                    self.enviar_midia(&r, true);
                }
                Some(_) => self.inicio.pergunta = None,
                None => {}
            }
        }
        let mut fechar = false;
        let mut ir: Option<Pagina> = None;
        if let Some(i) = &self.inicio.info {
            ui.add_space(8.0);
            let (fundo, borda, cor) = match i.tom {
                Tom::Aviso | Tom::Erro => (pal.warn_soft, pal.warn, pal.warn),
                _ => (pal.ok_soft, pal.ok, pal.ok),
            };
            faixa(ui, fundo, borda, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::Label::new(RichText::new(&i.texto).color(cor)));
                    for (rot, pg) in &i.links {
                        if btn(rot).link().pequeno().mostrar(ui, &pal).clicked() {
                            ir = Some(*pg);
                        }
                    }
                    if btn("Fechar").link().pequeno().mostrar(ui, &pal).clicked() {
                        fechar = true;
                    }
                });
            });
        }
        if fechar {
            self.inicio.info = None;
        }
        if let Some(pg) = ir {
            self.inicio.info = None;
            self.mostrar(pg);
        }
    }

    /// Uma linha de "Recentes": o que foi feito, como esta e o que da para fazer a seguir.
    fn linha_recente(
        &mut self,
        ui: &mut Ui,
        ex: &Execucao,
        listrada: bool,
        agora: f64,
    ) -> Option<AcaoLinha> {
        let pal = self.pal;
        let mut acao = None;
        let mut central = false;
        Frame::new()
            .fill(if listrada { pal.surface2 } else { pal.surface })
            .inner_margin(Margin::symmetric(18, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let total = ui.available_width();
                let (lw, mw) = ((total * 0.36).max(150.0), (total * 0.28).max(130.0));
                ui.horizontal(|ui| {
                    // nome e quando
                    ui.allocate_ui_with_layout(
                        vec2(lw, 38.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_size(vec2(lw, 38.0));
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.add(
                                egui::Label::new(
                                    RichText::new(nome_exibicao(ex)).family(negrito()),
                                )
                                .truncate(),
                            );
                            let quando = ex
                                .fim_em
                                .or(ex.inicio_em)
                                .or(ex.criada_em)
                                .map(|t| format!(" · {}", fmt::ha_tempo(t, agora)))
                                .unwrap_or_default();
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("{}{quando}", ex.ferramenta_nome))
                                        .size(12.5)
                                        .color(pal.muted),
                                )
                                .truncate(),
                            );
                        },
                    );
                    // estado
                    ui.allocate_ui_with_layout(
                        vec2(mw, 38.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_min_size(vec2(mw, 38.0));
                            ui.spacing_mut().item_spacing.y = 2.0;
                            match ex.estado.as_str() {
                                "rodando" => {
                                    ui.horizontal(|ui| {
                                        let f = ex
                                            .determinada
                                            .then(|| fmt::frac(ex.progresso.unwrap_or(0.0)));
                                        barra(ui, &pal, f, (mw - 48.0).max(50.0), 8.0, None);
                                        if ex.determinada {
                                            mudo_peq(
                                                ui,
                                                &pal,
                                                &format!(
                                                    "{}%",
                                                    ex.progresso.unwrap_or(0.0).round() as i64
                                                ),
                                            );
                                        }
                                    });
                                    let t = ex
                                        .eta_exibido()
                                        .map(|e| format!("Restante: {e}"))
                                        .unwrap_or_else(|| ex.linha_etapa());
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(t).size(12.5).color(pal.muted),
                                        )
                                        .truncate(),
                                    );
                                }
                                "fila" => chip(ui, &pal, "Na fila", Cor::Neutro),
                                "concluida" => {
                                    chip(ui, &pal, "Concluída", Cor::Ok);
                                    if let Some(d) = ex.decorrido_s {
                                        mudo_peq(ui, &pal, &format!("em {}", fmt::dur(Some(d))));
                                    }
                                }
                                "falhou" => {
                                    chip(ui, &pal, "Falhou", Cor::Erro);
                                    let m = if ex.erro.is_empty() {
                                        ex.mensagem.as_str()
                                    } else {
                                        ex.erro.as_str()
                                    };
                                    if !m.is_empty() {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(m).size(12.5).color(pal.muted),
                                            )
                                            .truncate(),
                                        );
                                    }
                                }
                                outro => {
                                    let c = if outro == "interrompida" {
                                        Cor::Aviso
                                    } else {
                                        Cor::Neutro
                                    };
                                    chip(ui, &pal, &ex.estado_texto, c);
                                }
                            }
                        },
                    );
                    // acoes
                    a_direita(ui, |ui| {
                        if ex.saida.is_empty() {
                            if btn("Ver na Central").pequeno().mostrar(ui, &pal).clicked() {
                                central = true;
                            }
                        } else if btn("Abrir resultado").pequeno().mostrar(ui, &pal).clicked() {
                            acao = Some(AcaoLinha::Abrir(ex.saida.clone()));
                        }
                        if !ex.log_arquivo.is_empty()
                            && ex.log_ini.is_some()
                            && btn("Ver registro").link().pequeno().mostrar(ui, &pal).clicked()
                        {
                            acao = Some(AcaoLinha::VerLog(Box::new(ex.clone())));
                        }
                    });
                });
            });
        if central {
            self.mostrar(Pagina::Execucoes);
        }
        acao
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LinhaInstalacao;

    fn ex(id: &str, ferr: &str, criada: Option<f64>) -> Execucao {
        Execucao {
            id: id.into(),
            ferramenta: ferr.into(),
            criada_em: criada,
            ..Default::default()
        }
    }

    #[test]
    fn recentes_ordena_pela_mais_nova_e_limita_em_cinco() {
        let lista: Vec<Execucao> = (0..9)
            .map(|i| ex(&format!("e{i}"), "converter", Some(1000.0 + f64::from(i))))
            .collect();
        let r = recentes(&lista, LIMITE_RECENTES);
        assert_eq!(r.len(), 5);
        let ids: Vec<&str> = r.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["e8", "e7", "e6", "e5", "e4"]);
    }

    #[test]
    fn recentes_ignora_ferramentas_internas() {
        let lista = vec![
            ex("v", "verificacao", Some(9.0)),
            ex("m", "modelo", Some(8.0)),
            ex("i", "instalacao", Some(7.0)),
            ex("t", "transcricao", Some(5.0)),
            ex("c", "converter", Some(4.0)),
            ex("d", "diagnostico", Some(3.0)),
            ex("l", "lote", Some(2.0)),
        ];
        let ids: Vec<&str> = recentes(&lista, 5).iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["t", "c", "d", "l"]);
    }

    #[test]
    fn recentes_sem_horario_mantem_a_ordem_do_motor() {
        let lista = vec![
            ex("a", "converter", None),
            ex("b", "converter", None),
            ex("c", "lote", None),
        ];
        let ids: Vec<&str> = recentes(&lista, 2).iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["a", "b"]);
        assert!(recentes(&[], 5).is_empty());
    }

    #[test]
    fn nome_do_arquivo_vem_do_caminho() {
        let mut e = ex("a", "transcricao", None);
        e.entrada = r"C:\Aulas\Aula 3.mp3".into();
        assert_eq!(nome_exibicao(&e), "Aula 3.mp3");
        e.entrada = "3 arquivos".into();
        assert_eq!(nome_exibicao(&e), "3 arquivos");
        e.entrada.clear();
        e.rotulo = "Conversão direta".into();
        assert_eq!(nome_exibicao(&e), "Conversão direta");
        e.rotulo.clear();
        assert_eq!(nome_exibicao(&e), "Execução");
    }

    fn resultado(linhas: &[(&str, EstadoComp)]) -> ResultadoInstalacao {
        ResultadoInstalacao {
            linhas: linhas
                .iter()
                .map(|(n, e)| LinhaInstalacao {
                    nome: (*n).into(),
                    estado: *e,
                    detalhe: String::new(),
                })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn estado_do_computador_ok_aviso_e_neutro() {
        let ok = estado_computador(Some(&resultado(&[("Python", EstadoComp::Ok)])), false);
        assert_eq!(ok.tom, Tom::Normal);
        assert!(!ok.detalhes && ok.texto.contains("OK"));

        let modelo = estado_computador(
            Some(&resultado(&[
                ("Python", EstadoComp::Ok),
                ("Modelo de transcricao", EstadoComp::Falha),
                ("ffmpeg (transcricao)", EstadoComp::Falha),
            ])),
            false,
        );
        assert_eq!(modelo.tom, Tom::Normal, "o modelo a baixar e neutro");
        assert!(modelo.texto.contains("primeiro uso"));

        let avisos = estado_computador(
            Some(&resultado(&[
                ("Ghostscript", EstadoComp::Aviso),
                ("Outro", EstadoComp::Aviso),
            ])),
            false,
        );
        assert_eq!(avisos.texto, "2 avisos: ver detalhes");
        assert!(avisos.detalhes);

        let falha = estado_computador(
            Some(&resultado(&[
                ("Tesseract", EstadoComp::Falha),
                ("Ghostscript", EstadoComp::Aviso),
            ])),
            false,
        );
        assert_eq!(falha.tom, Tom::Aviso);
        assert!(falha.texto.starts_with("2 itens precisam"));
    }

    #[test]
    fn estado_do_computador_sem_resultado() {
        let v = estado_computador(None, true);
        assert_eq!(v.tom, Tom::Ocupado);
        assert!(v.texto.contains("Verificando") && !v.detalhes);
        let n = estado_computador(None, false);
        assert!(n.detalhes);
    }
}
