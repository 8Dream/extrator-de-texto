//! Estado da aplicacao, laco principal, tratamento das respostas e eventos do motor.

use crate::bus::{Bus, Escolha, EstadoMotor, Msg, Tag};
use crate::cores::Rgb;
use crate::engine::Motor;
use crate::fmt;
use crate::model::*;
use crate::pg_inicio::Inicio;
use crate::prefs::{Aparencia, TamanhoTexto};
use crate::theme::{self, Palette};
use eframe::egui;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pagina {
    Inicio,
    Converter,
    Lote,
    Transcricao,
    Diagnostico,
    Execucoes,
    Log,
    Configuracoes,
    Ajuda,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Prog {
    Oculto,
    Indeterminado,
    Fracao(f32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tom {
    Normal,
    Ocupado,
    Aviso,
    Erro,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TipoAviso {
    Info,
    Aviso,
    Erro,
}

pub struct Aviso {
    pub id: u64,
    pub texto: String,
    pub tipo: TipoAviso,
    pub nasceu: Instant,
    pub link_log: Option<(String, i64)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AcaoConfirma {
    ConverterAssinados,
    IniciarLote,
    LoteAssinados,
    LimparExecucoes,
    RestaurarPadroes,
    LimparDados,
    InstalarFaltantes,
    FecharComTarefas,
}

#[derive(Clone, Debug)]
pub struct Confirma {
    pub titulo: String,
    pub corpo: String,
    pub ok: String,
    pub perigo: bool,
    pub acao: AcaoConfirma,
}

// ---- estado por tela ------------------------------------------------------

pub struct Conv {
    pub modo: String,
    pub formato: usize,
    pub arquivos: Vec<String>,
    pub mesma_pasta: bool,
    pub pasta_saida: String,
    pub paginas: String,
    pub juntar: bool,
}

pub struct Lote {
    pub modo: String,
    pub entrada: String,
    pub saida: String,
    pub recursivo: bool,
    pub resumo: String,
    pub varredura: Option<Value>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusItem {
    Pendente,
    EmAndamento,
    Concluido,
    Erro,
    Cancelado,
}

impl StatusItem {
    pub fn texto(self) -> &'static str {
        match self {
            StatusItem::Pendente => "Pendente",
            StatusItem::EmAndamento => "Em andamento",
            StatusItem::Concluido => "Concluído",
            StatusItem::Erro => "Erro",
            StatusItem::Cancelado => "Cancelado",
        }
    }
}

pub struct ItemTr {
    pub id: String,
    pub caminho: String,
    pub status: StatusItem,
    /// Rotulos de interlocutores e resumo (so com a capacidade `interlocutores`).
    pub interlocutores: Option<ResumoInterlocutores>,
}

#[derive(Clone, Debug, Default)]
pub struct ResumoInterlocutores {
    pub rotulos: Vec<String>,
    pub tempo_s: Vec<f64>,
    pub trocas: Option<u64>,
}

pub struct Tr {
    pub itens: Vec<ItemTr>,
    pub selecionado: Option<String>,
    pub rodando: bool,
    pub pausado: bool,
    pub ultima_saida: Option<String>,
    pub prox_id: u32,
    pub modelo: String,
    pub idioma: String,
    pub formato: String,
    pub beams: String,
    pub palavras: bool,
    pub deps: Vec<(String, bool)>,
    pub status: String,
    pub pct: f32,
    pub feito: String,
    pub restante: String,
    pub decorrido: String,
    pub estimativa: String,
    pub baixar_aberto: bool,
    pub baixar_escolha: String,
    pub baixar_msg: String,
    pub baixando: bool,
    pub interloc_ativo: bool,
    pub interloc_min: String,
    pub interloc_max: String,
}

pub struct Diag {
    pub caminho: String,
    pub linhas: Option<Vec<(String, String)>>,
}

pub struct Cfg {
    pub idioma: String,
    pub dpi: String,
    pub psm: String,
    pub limite: String,
    pub cpu: String,
    pub pdf: String,
    pub verbosidade: String,
    pub girar: bool,
    pub inclinadas: bool,
    pub retomar: bool,
    pub abrir: bool,
    pub motor: String,
    pub msg: String,
    pub spark_msg: String,
    pub base: Config,
}

pub struct Ajuda {
    pub verificando: bool,
    pub silenciosa: bool,
    pub contagem: usize,
    pub parciais: Vec<LinhaInstalacao>,
    pub resultado: Option<ResultadoInstalacao>,
    pub linha: Option<(String, Tom)>,
    pub instalando: bool,
}

pub struct Exec {
    pub lista: ListaExecucoes,
    pub abertos: HashSet<String>,
    pub linhas: HashMap<String, Vec<LinhaExec>>,
    pub proximo: HashMap<String, i64>,
    pub detalhe_em_voo: HashSet<String>,
    pub estado: usize,
    pub ferramenta: String,
    pub busca: String,
    pub cancelando: HashSet<String>,
    pub resumo_copiado: Option<String>,
}

pub struct LogSt {
    pub iniciado: bool,
    pub nome: String,
    pub atual: String,
    pub arquivos: Vec<ArquivoLog>,
    pub linhas: Vec<LinhaLog>,
    pub ini: i64,
    pub fim: i64,
    pub tamanho: u64,
    pub nv_ant: String,
    pub pausado: bool,
    pub ao_vivo: bool,
    pub foco: Option<String>,
    pub tag_foco: Option<String>,
    pub marca: Option<i64>,
    pub rolar_para_marca: bool,
    pub modo_busca: bool,
    pub nivel: usize,
    pub busca: String,
    pub auto: bool,
    pub info: String,
    pub em_voo: bool,
    pub ultimo: Instant,
    pub pendente: Option<crate::pg_log::LogPedido>,
    pub ao_vivo_proximo: Option<bool>,
    pub marca_proxima: Option<i64>,
    pub rolar_proximo: bool,
}

pub const NIVEIS_LOG: [(&str, &str); 3] = [
    ("tudo", "Tudo"),
    ("avisos", "Avisos e erros"),
    ("erros", "Só erros"),
];

pub struct App {
    pub rx: Receiver<Msg>,
    pub bus: Bus,
    pub motor: Motor,
    pub pal: Palette,
    pub escuro: bool,
    /// Preferencias de aparencia (tema, destaque, tamanho do texto), gravadas em interface.json.
    pub ap: Aparencia,
    /// Ultima combinacao aplicada ao egui: (escuro, destaque, tamanho do texto).
    pub aplicado: Option<(bool, Rgb, TamanhoTexto)>,
    pub sistema_escuro: bool,
    pub ultimo_sistema: Instant,
    /// O destaque escolhido precisou de ajuste de tom para manter o contraste.
    pub destaque_ajustado: bool,
    pub hex_texto: String,
    pub seletor_aberto: bool,
    pub rolar_topo: bool,
    pub inicio: Inicio,
    /// Captura de tela de desenvolvimento (EXTRATOR_CAPTURA); `None` no uso normal.
    pub captura: Option<crate::dev::Captura>,
    pub pagina: Pagina,
    pub estado_motor: EstadoMotor,
    pub faixa_motor: Option<String>,
    pub falha_boot: Option<String>,
    pub boot: Option<Boot>,
    pub caps: Capacidades,
    pub versao: String,
    pub logo: Option<egui::TextureHandle>,

    pub status: String,
    pub tom: Tom,
    pub prog: Prog,
    pub dock_aberto: bool,
    pub dock_log: VecDeque<String>,
    pub avisos: Vec<Aviso>,
    pub prox_aviso: u64,
    pub confirma: Option<Confirma>,
    pub ocupado: HashSet<&'static str>,
    pub job_rodando: bool,
    pub job_erros: bool,

    pub conv: Conv,
    pub lote: Lote,
    pub tr: Tr,
    pub diag: Diag,
    pub cfg: Cfg,
    pub ajuda: Ajuda,
    pub exec: Exec,
    pub log: LogSt,

    pub eventos_em_voo: bool,
    pub ultimo_eventos: Instant,
    pub exec_em_voo: bool,
    pub ultimo_exec: Instant,
    pub encerrando: bool,
    pub pode_fechar: bool,
    pub dados_dir: std::path::PathBuf,
}

/// Consulta de eventos com trabalho em andamento / ocioso (menos acordadas = menos CPU).
const INTERVALO_EVENTOS: Duration = Duration::from_millis(250);
const INTERVALO_OCIOSO: Duration = Duration::from_millis(700);

/// Para desenvolvimento e capturas de tela: EXTRATOR_PAGINA=exec abre direto nessa tela.
fn pagina_inicial() -> Pagina {
    match std::env::var("EXTRATOR_PAGINA")
        .unwrap_or_default()
        .as_str()
    {
        "converter" => Pagina::Converter,
        "lote" => Pagina::Lote,
        "transcricao" => Pagina::Transcricao,
        "diagnostico" => Pagina::Diagnostico,
        "exec" => Pagina::Execucoes,
        "log" => Pagina::Log,
        "config" | "aparencia" => Pagina::Configuracoes,
        "ajuda" => Pagina::Ajuda,
        _ => Pagina::Inicio,
    }
}

fn longe() -> Instant {
    Instant::now()
        .checked_sub(Duration::from_secs(3600))
        .unwrap_or_else(Instant::now)
}

impl App {
    pub fn novo(
        cc: &eframe::CreationContext<'_>,
        rx: Receiver<Msg>,
        bus: Bus,
        motor: Motor,
        versao: String,
    ) -> App {
        theme::instalar_fontes(&cc.egui_ctx);
        let logo = crate::winutil::logotipo(96).map(|(rgba, w, h)| {
            cc.egui_ctx.load_texture(
                "logo",
                egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba),
                egui::TextureOptions::LINEAR,
            )
        });
        let dados_dir = motor.local().raiz.join("dados");
        let ap = crate::prefs::ler(&dados_dir);
        let sistema_escuro = crate::winutil::tema_escuro_do_windows().unwrap_or_else(|| {
            cc.egui_ctx
                .system_theme()
                .map(|t| t == egui::Theme::Dark)
                .unwrap_or(false)
        });
        let escuro = ap.escuro(sistema_escuro);
        let (pal, destaque_ajustado) = Palette::montar(escuro, ap.destaque.rgb());
        theme::aplicar(&cc.egui_ctx, &pal);
        cc.egui_ctx.set_zoom_factor(ap.texto.escala());
        let hex_texto = crate::cores::hex(ap.destaque.rgb());
        App {
            rx,
            bus,
            motor,
            pal,
            escuro,
            aplicado: Some((escuro, ap.destaque.rgb(), ap.texto)),
            ap,
            sistema_escuro,
            ultimo_sistema: Instant::now(),
            destaque_ajustado,
            hex_texto,
            seletor_aberto: false,
            rolar_topo: false,
            inicio: Inicio::default(),
            captura: crate::dev::Captura::de_ambiente(),
            pagina: pagina_inicial(),
            estado_motor: EstadoMotor::Iniciando,
            faixa_motor: None,
            falha_boot: None,
            boot: None,
            caps: Capacidades::default(),
            versao,
            logo,
            status: "Pronto.".into(),
            tom: Tom::Normal,
            prog: Prog::Oculto,
            dock_aberto: false,
            dock_log: VecDeque::new(),
            avisos: Vec::new(),
            prox_aviso: 1,
            confirma: None,
            ocupado: HashSet::new(),
            job_rodando: false,
            job_erros: false,
            conv: Conv {
                modo: "auto".into(),
                formato: 0,
                arquivos: Vec::new(),
                mesma_pasta: true,
                pasta_saida: String::new(),
                paginas: String::new(),
                juntar: true,
            },
            lote: Lote {
                modo: "auto_md".into(),
                entrada: String::new(),
                saida: String::new(),
                recursivo: false,
                resumo: "Escolha a pasta de entrada.".into(),
                varredura: None,
            },
            tr: Tr {
                itens: Vec::new(),
                selecionado: None,
                rodando: false,
                pausado: false,
                ultima_saida: None,
                prox_id: 1,
                modelo: String::new(),
                idioma: "pt".into(),
                formato: "txt".into(),
                beams: "5".into(),
                palavras: false,
                deps: Vec::new(),
                status: "Fila vazia.".into(),
                pct: 0.0,
                feito: "00:00:00 / 00:00:00".into(),
                restante: "00:00:00".into(),
                decorrido: "00:00:00".into(),
                estimativa: "--:--:--".into(),
                baixar_aberto: false,
                baixar_escolha: String::new(),
                baixar_msg: String::new(),
                baixando: false,
                interloc_ativo: false,
                interloc_min: String::new(),
                interloc_max: String::new(),
            },
            diag: Diag {
                caminho: String::new(),
                linhas: None,
            },
            cfg: Cfg::de(&Config::default()),
            ajuda: Ajuda {
                verificando: false,
                silenciosa: false,
                contagem: 0,
                parciais: Vec::new(),
                resultado: None,
                linha: None,
                instalando: false,
            },
            exec: Exec {
                lista: ListaExecucoes::default(),
                abertos: HashSet::new(),
                linhas: HashMap::new(),
                proximo: HashMap::new(),
                detalhe_em_voo: HashSet::new(),
                estado: 0,
                ferramenta: String::new(),
                busca: String::new(),
                cancelando: HashSet::new(),
                resumo_copiado: None,
            },
            log: LogSt {
                iniciado: false,
                nome: String::new(),
                atual: String::new(),
                arquivos: Vec::new(),
                linhas: Vec::new(),
                ini: 0,
                fim: 0,
                tamanho: 0,
                nv_ant: "INFO".into(),
                pausado: false,
                ao_vivo: true,
                foco: None,
                tag_foco: None,
                marca: None,
                rolar_para_marca: false,
                modo_busca: false,
                nivel: 0,
                busca: String::new(),
                auto: true,
                info: "Abra a tela para carregar o log.".into(),
                em_voo: false,
                ultimo: longe(),
                pendente: None,
                ao_vivo_proximo: None,
                marca_proxima: None,
                rolar_proximo: true,
            },
            eventos_em_voo: false,
            ultimo_eventos: longe(),
            exec_em_voo: false,
            ultimo_exec: longe(),
            encerrando: false,
            pode_fechar: false,
            dados_dir,
        }
    }

    // ------------------------------------------------------------ utilitarios

    pub fn chamar(&self, tag: Tag, metodo: &str, args: Vec<Value>) {
        self.motor.chamar(tag, metodo, args);
    }

    pub fn definir_status(&mut self, texto: impl Into<String>, tom: Tom) {
        self.status = texto.into();
        self.tom = tom;
    }

    pub fn log_dock(&mut self, texto: &str) {
        let linha = format!("[{}] {}", fmt::hora_local(fmt::agora_unix()), texto);
        self.dock_log.push_back(linha);
        while self.dock_log.len() > 2500 {
            self.dock_log.pop_front();
        }
    }

    pub fn tarefa_ativa(&self) -> bool {
        self.job_rodando || self.tr.rodando
    }

    pub fn esconder_progresso_se_livre(&mut self) {
        if !self.tarefa_ativa() && self.ocupado.is_empty() {
            self.prog = Prog::Oculto;
        }
    }

    /// Aviso rapido. Avisos e erros tambem gravam uma linha no log (para o "Ver no log").
    pub fn aviso(&mut self, texto: &str, tipo: TipoAviso) {
        let id = self.prox_aviso;
        self.prox_aviso += 1;
        self.avisos.push(Aviso {
            id,
            texto: texto.to_string(),
            tipo,
            nasceu: Instant::now(),
            link_log: None,
        });
        if tipo != TipoAviso::Info {
            let nivel = if tipo == TipoAviso::Erro {
                "ERROR"
            } else {
                "WARNING"
            };
            self.chamar(
                Tag::LogRegistrar(id),
                "log_registrar",
                vec![json!(nivel), json!(texto)],
            );
        }
    }

    /// Aviso DENTRO da janela: toast, status no rodape e linha no registro da sessao.
    pub fn notificar(&mut self, texto: &str, tipo: TipoAviso) {
        self.aviso(texto, tipo);
        self.definir_status(
            texto,
            if tipo == TipoAviso::Erro {
                Tom::Erro
            } else {
                Tom::Normal
            },
        );
        let prefixo = if tipo == TipoAviso::Erro {
            "ERRO: "
        } else {
            ""
        };
        self.log_dock(&format!("{prefixo}{texto}"));
        if tipo == TipoAviso::Erro {
            self.dock_aberto = true;
        }
    }

    /// Inicia uma acao com retorno visivel: botao ocupado, status e barra indeterminada.
    pub fn iniciar_acao(&mut self, chave: &'static str, texto: &str) {
        self.ocupado.insert(chave);
        self.definir_status(texto, Tom::Ocupado);
        self.prog = Prog::Indeterminado;
        self.log_dock(texto);
    }

    pub fn terminar_acao(&mut self, chave: &'static str) {
        self.ocupado.remove(chave);
        self.esconder_progresso_se_livre();
    }

    pub fn cutucar_exec(&mut self) {
        self.ultimo_exec = longe();
    }

    pub fn mostrar(&mut self, p: Pagina) {
        self.pagina = p;
        if p == Pagina::Execucoes || p == Pagina::Inicio {
            self.cutucar_exec();
        }
        if p == Pagina::Log {
            self.abrir_pagina_log();
        }
    }

    pub fn definir_job(&mut self, rodando: bool) {
        self.job_rodando = rodando;
        self.job_erros = false;
        if rodando {
            self.definir_status("Processando...", Tom::Ocupado);
            self.prog = Prog::Indeterminado;
        } else {
            self.esconder_progresso_se_livre();
        }
    }

    pub fn definir_tr(&mut self, rodando: bool) {
        self.tr.rodando = rodando;
        self.tr.pausado = false;
        if rodando {
            self.prog = Prog::Indeterminado;
        } else {
            self.esconder_progresso_se_livre();
        }
        self.atualizar_resumo_tr();
    }

    // -------------------------------------------------------------- laco

    /// Chamado a cada quadro: recebe mensagens, agenda consultas ao motor, trata fechamento.
    pub fn tick(&mut self, ctx: &egui::Context) {
        while let Ok(m) = self.rx.try_recv() {
            self.tratar(ctx, m);
        }
        self.motor.varrer_limites();
        self.atualizar_aparencia(ctx);
        self.avisos.retain(|a| {
            let dur = if a.tipo == TipoAviso::Info { 4.2 } else { 12.0 };
            a.nasceu.elapsed().as_secs_f32() < dur
        });
        self.tratar_fechamento(ctx);

        if self.estado_motor == EstadoMotor::Pronto && self.boot.is_some() {
            let agora = Instant::now();
            let ocupado = self.tarefa_ativa()
                || !self.ocupado.is_empty()
                || self.exec.lista.rodando + self.exec.lista.na_fila > 0;
            let intervalo_eventos = if ocupado {
                INTERVALO_EVENTOS
            } else {
                INTERVALO_OCIOSO
            };
            if !self.eventos_em_voo
                && agora.duration_since(self.ultimo_eventos) >= intervalo_eventos
            {
                self.eventos_em_voo = true;
                self.ultimo_eventos = agora;
                self.chamar(Tag::Eventos, "drain_events", vec![]);
            }
            let rapido = self.exec.lista.rodando > 0
                || self.exec.lista.na_fila > 0
                || self.pagina == Pagina::Execucoes;
            let intervalo = if rapido {
                Duration::from_secs(1)
            } else {
                Duration::from_secs(4)
            };
            if !self.exec_em_voo && agora.duration_since(self.ultimo_exec) >= intervalo {
                self.exec_em_voo = true;
                self.ultimo_exec = agora;
                self.chamar(Tag::ExecLista, "exec_listar", vec![]);
                let abertos: Vec<String> = self.exec.abertos.iter().cloned().collect();
                for id in abertos {
                    if self.exec.detalhe_em_voo.insert(id.clone()) {
                        let desde = self.exec.proximo.get(&id).copied().unwrap_or(0);
                        self.chamar(
                            Tag::ExecDetalhe(id.clone()),
                            "exec_detalhe",
                            vec![json!(id), json!(desde)],
                        );
                    }
                }
            }
            self.acompanhar_log();
        }
        let ocupado = self.tarefa_ativa() || !self.ocupado.is_empty();
        ctx.request_repaint_after(Duration::from_millis(if ocupado { 150 } else { 350 }));
    }

    /// Aplica na hora tema, destaque e tamanho do texto; no modo automatico acompanha o Windows.
    fn atualizar_aparencia(&mut self, ctx: &egui::Context) {
        if self.ap.modo == crate::prefs::ModoTema::Auto
            && self.ultimo_sistema.elapsed() >= Duration::from_secs(2)
        {
            self.ultimo_sistema = Instant::now();
            if let Some(e) = crate::winutil::tema_escuro_do_windows() {
                self.sistema_escuro = e;
            }
        }
        let escuro = self.ap.escuro(self.sistema_escuro);
        let chave = (escuro, self.ap.destaque.rgb(), self.ap.texto);
        if self.aplicado == Some(chave) {
            return;
        }
        let (pal, ajustada) = Palette::montar(escuro, chave.1);
        self.pal = pal;
        self.escuro = escuro;
        self.destaque_ajustado = ajustada;
        theme::aplicar(ctx, &self.pal);
        if self.aplicado.map(|a| a.2) != Some(chave.2) {
            ctx.set_zoom_factor(chave.2.escala());
        }
        self.aplicado = Some(chave);
    }

    pub fn tratar_soltos(&mut self, ctx: &egui::Context) {
        let soltos: Vec<String> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        });
        if soltos.is_empty() || self.boot.is_none() {
            return;
        }
        self.log_dock(&format!("Arquivos soltos na janela: {}", soltos.len()));
        match self.pagina {
            Pagina::Inicio => self.rotear_arquivos(soltos),
            Pagina::Converter => self.adicionar_conv(soltos),
            Pagina::Transcricao => self.adicionar_tr(soltos),
            Pagina::Diagnostico => {
                if let Some(p) = soltos.into_iter().next() {
                    self.diag.caminho = p;
                }
            }
            _ => {}
        }
    }

    fn tratar_fechamento(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.pode_fechar {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        if self.encerrando {
            return;
        }
        let ativo = self.tarefa_ativa() || self.exec.lista.rodando + self.exec.lista.na_fila > 0;
        let ja_perguntou =
            matches!(&self.confirma, Some(c) if c.acao == AcaoConfirma::FecharComTarefas);
        if ativo && !ja_perguntou {
            self.confirma = Some(Confirma {
                titulo: "Fechar o programa?".into(),
                corpo: "Há tarefas em andamento: fechar mesmo assim? Elas serão canceladas.".into(),
                ok: "Fechar mesmo assim".into(),
                perigo: true,
                acao: AcaoConfirma::FecharComTarefas,
            });
            return;
        }
        self.comecar_encerramento();
    }

    pub fn comecar_encerramento(&mut self) {
        if self.encerrando {
            return;
        }
        self.encerrando = true;
        self.confirma = None;
        self.definir_status("Encerrando o motor…", Tom::Ocupado);
        self.prog = Prog::Indeterminado;
        let motor = self.motor.clone();
        let bus = self.bus.clone();
        let r = std::thread::Builder::new()
            .name("encerrar".into())
            .spawn(move || {
                motor.encerrar();
                bus.enviar(Msg::PodeFechar);
            });
        if r.is_err() {
            self.pode_fechar = true;
        }
    }

    // --------------------------------------------------------- mensagens

    pub fn tratar(&mut self, ctx: &egui::Context, m: Msg) {
        match m {
            Msg::PodeFechar => {
                self.pode_fechar = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Msg::Motor(e) => self.tratar_motor(e),
            Msg::Arquivos(e, caminhos) => {
                self.ocupado.remove("escolher");
                let c: Vec<String> = caminhos
                    .iter()
                    .map(|p| p.to_string_lossy().to_string())
                    .collect();
                match e {
                    Escolha::ArquivosInicio => {
                        if !c.is_empty() {
                            self.rotear_arquivos(c);
                        }
                    }
                    Escolha::ArquivosConverter => self.adicionar_conv(c),
                    Escolha::ArquivosTranscricao => self.adicionar_tr(c),
                    Escolha::ArquivoDiagnostico => {
                        if let Some(p) = c.into_iter().next() {
                            self.diag.caminho = p;
                        }
                    }
                    _ => {}
                }
            }
            Msg::Pasta(e, p) => {
                self.ocupado.remove("escolher");
                let Some(p) = p else { return };
                let p = p.to_string_lossy().to_string();
                match e {
                    Escolha::PastaSaida => self.conv.pasta_saida = p,
                    Escolha::PastaLoteEntrada => {
                        if self.lote.saida.is_empty() {
                            self.lote.saida = format!("{p}\\Extrator_saida");
                        }
                        self.lote.entrada = p;
                        self.lote.resumo = "Pasta escolhida. Selecione o modo e inicie.".into();
                    }
                    Escolha::PastaLoteSaida => self.lote.saida = p,
                    _ => {}
                }
            }
            Msg::Resposta(tag, r) => self.tratar_resposta(tag, r),
        }
    }

    fn tratar_motor(&mut self, e: EstadoMotor) {
        match &e {
            EstadoMotor::Iniciando => {}
            EstadoMotor::Pronto => {
                if self.pagina == Pagina::Log && !self.log.iniciado {
                    self.abrir_pagina_log();
                }
                self.faixa_motor = None;
                self.falha_boot = None;
                self.eventos_em_voo = false;
                self.exec_em_voo = false;
                self.chamar(Tag::Boot, "bootstrap", vec![]);
                self.chamar(Tag::Capacidades, "capacidades", vec![]);
            }
            EstadoMotor::Caiu { motivo, tentativa } => {
                let msg = format!("{motivo} Religando o motor (tentativa {tentativa})…");
                self.faixa_motor = Some(msg);
                self.interromper_tudo();
            }
            EstadoMotor::Falhou(m) => {
                self.faixa_motor = Some(m.clone());
                self.falha_boot = Some(m.clone());
                self.interromper_tudo();
            }
        }
        self.estado_motor = e;
    }

    /// O motor caiu: o que estava rodando nele acabou.
    fn interromper_tudo(&mut self) {
        let tinha = self.tarefa_ativa() || !self.ocupado.is_empty();
        self.job_rodando = false;
        self.tr.rodando = false;
        self.tr.pausado = false;
        for it in &mut self.tr.itens {
            if it.status == StatusItem::EmAndamento {
                it.status = StatusItem::Erro;
            }
        }
        self.ocupado.clear();
        self.ajuda.verificando = false;
        self.ajuda.instalando = false;
        self.tr.baixando = false;
        self.eventos_em_voo = false;
        self.exec_em_voo = false;
        self.exec.detalhe_em_voo.clear();
        self.log.em_voo = false;
        self.prog = Prog::Oculto;
        if tinha {
            self.definir_status(
                "O motor foi interrompido; as tarefas em andamento pararam.",
                Tom::Erro,
            );
            self.log_dock("O motor foi interrompido; as tarefas em andamento pararam.");
        }
    }

    pub fn tratar_resposta(&mut self, tag: Tag, r: crate::proto::Resposta) {
        match tag {
            Tag::Boot => match r {
                Ok(v) => self.aplicar_boot(Boot::de_json(&v)),
                Err(e) => {
                    self.falha_boot = Some(e.clone());
                    self.registrar_launcher(&format!("bootstrap falhou: {e}"));
                }
            },
            Tag::Capacidades => {
                self.caps = r
                    .ok()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();
            }
            Tag::Eventos => {
                self.eventos_em_voo = false;
                if let Ok(Value::Array(evs)) = r {
                    for ev in evs {
                        self.tratar_evento(&ev);
                    }
                }
            }
            Tag::ExecLista => {
                self.exec_em_voo = false;
                if let Ok(v) = r {
                    self.aplicar_exec(ListaExecucoes::de_json(&v));
                }
            }
            Tag::ExecDetalhe(id) => {
                self.exec.detalhe_em_voo.remove(&id);
                if let Ok(v) = r {
                    let novas: Vec<LinhaExec> =
                        serde_json::from_value(v.get("linhas").cloned().unwrap_or(Value::Null))
                            .unwrap_or_default();
                    self.exec
                        .linhas
                        .entry(id.clone())
                        .or_default()
                        .extend(novas);
                    if let Some(p) = v.get("proximo").and_then(Value::as_i64) {
                        self.exec.proximo.insert(id, p);
                    }
                }
            }
            Tag::ExecCancelar(id) => {
                self.exec.cancelando.remove(&id);
                self.terminar_acao("exec_cancelar");
                if let Err(e) = r {
                    self.notificar(&e, TipoAviso::Erro);
                }
                self.cutucar_exec();
            }
            Tag::ExecRepetir(_) => {
                self.terminar_acao("exec_repetir");
                match r {
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                    Ok(v) => {
                        if v.get("status").and_then(Value::as_str) == Some("needs_confirm") {
                            self.notificar(
                                "Há PDFs com assinatura digital nesta execução. Repita pela tela Converter, onde a confirmação é pedida.",
                                TipoAviso::Erro,
                            );
                        } else {
                            match v.get("acao").and_then(Value::as_str) {
                                Some("start_convert") | Some("start_batch") => {
                                    self.definir_job(true)
                                }
                                Some("start_transcricao") => self.definir_tr(true),
                                Some("diagnose") => {
                                    if let Some(rows) = v.get("rows") {
                                        self.diag.linhas = Some(linhas_diag(rows));
                                    }
                                }
                                _ => {}
                            }
                            self.aviso(
                                "Execução repetida: acompanhe aqui na lista.",
                                TipoAviso::Info,
                            );
                        }
                    }
                }
                self.cutucar_exec();
            }
            Tag::ExecResumo(_) => match r {
                Ok(v) => {
                    self.exec.resumo_copiado =
                        v.get("texto").and_then(Value::as_str).map(str::to_string);
                }
                Err(e) => self.notificar(&e, TipoAviso::Erro),
            },
            Tag::ExecLimpar => {
                self.terminar_acao("exec_limpar");
                match r {
                    Ok(v) => {
                        let n = v.get("removidas").and_then(Value::as_u64).unwrap_or(0) as usize;
                        self.notificar(
                            &format!(
                                "{} da lista.",
                                fmt::plural(n, "execução removida", "execuções removidas")
                            ),
                            TipoAviso::Info,
                        );
                    }
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                }
                self.cutucar_exec();
            }
            Tag::LogArquivos => {
                if let Ok(v) = r {
                    self.aplicar_lista_logs(&v);
                }
            }
            Tag::LogLer(uso) => self.resposta_log(uso, r),
            Tag::LogBuscar => {
                self.terminar_acao("log_buscar");
                match r {
                    Ok(v) => self.aplicar_busca_log(LeituraLog::de_json(&v)),
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                }
            }
            Tag::LogRegistrar(id) => {
                if let Ok(v) = r {
                    if let (Some(nome), Some(off)) = (
                        v.get("nome").and_then(Value::as_str),
                        v.get("offset").and_then(Value::as_i64),
                    ) {
                        if let Some(a) = self.avisos.iter_mut().find(|a| a.id == id) {
                            a.link_log = Some((nome.to_string(), off));
                        }
                    }
                }
            }
            Tag::LogPasta => {
                self.terminar_acao("log_pasta");
                match r {
                    Ok(_) => self.notificar(
                        "Pasta dos logs aberta no Explorador de Arquivos.",
                        TipoAviso::Info,
                    ),
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                }
            }
            Tag::TrEstado => {
                if let Ok(v) = r {
                    if let Some(b) = self.boot.as_mut() {
                        b.transcricao = EstadoTranscricao::de_json(&v);
                    }
                    self.ajustar_opcoes_tr();
                }
            }
            Tag::TrDeps => {
                if let Ok(v) = r {
                    self.tr.deps = v
                        .get("deps")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .map(|d| {
                                    (
                                        d.get("name")
                                            .and_then(Value::as_str)
                                            .unwrap_or("")
                                            .to_string(),
                                        d.get("ok").and_then(Value::as_bool).unwrap_or(false),
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                }
            }
            Tag::IniciarConversao(confirmado) => {
                self.terminar_acao("converter");
                match r {
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                    Ok(v) => {
                        if v.get("status").and_then(Value::as_str) == Some("needs_confirm")
                            && !confirmado
                        {
                            let nomes: Vec<String> = v
                                .get("signed")
                                .and_then(Value::as_array)
                                .map(|a| {
                                    a.iter()
                                        .filter_map(|x| x.as_str().map(str::to_string))
                                        .collect()
                                })
                                .unwrap_or_default();
                            self.confirma = Some(Confirma {
                                titulo: "Assinatura digital detectada".into(),
                                corpo: format!(
                                    "{}\nO OCR invalida a assinatura nesses arquivos. O original nunca é sobrescrito. Continuar?",
                                    resumo_nomes(&nomes, 12)
                                ),
                                ok: "Continuar".into(),
                                perigo: false,
                                acao: AcaoConfirma::ConverterAssinados,
                            });
                        } else {
                            self.definir_job(true);
                        }
                    }
                }
                self.cutucar_exec();
            }
            Tag::AnalisarLote => {
                self.terminar_acao("lote");
                match r {
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                    Ok(v) => self.depois_da_varredura(v),
                }
            }
            Tag::IniciarLote => {
                self.terminar_acao("lote");
                match r {
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                    Ok(_) => self.definir_job(true),
                }
                self.cutucar_exec();
            }
            Tag::IniciarTr => {
                self.terminar_acao("tr_iniciar");
                match r {
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                    Ok(_) => self.definir_tr(true),
                }
                self.cutucar_exec();
            }
            Tag::PausarTr => {
                self.terminar_acao("tr_pausar");
                match r {
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                    Ok(v) => {
                        let p = v.get("paused").and_then(Value::as_bool).unwrap_or(false);
                        self.tr.pausado = p;
                        self.definir_status(
                            if p {
                                "Transcrição pausada."
                            } else {
                                "Transcrevendo..."
                            },
                            if p { Tom::Normal } else { Tom::Ocupado },
                        );
                    }
                }
            }
            Tag::CancelarTr | Tag::CancelarTrabalho => {
                if let Err(e) = r {
                    self.notificar(&e, TipoAviso::Erro);
                }
            }
            Tag::Diagnosticar => {
                self.terminar_acao("diag");
                match r {
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                    Ok(v) => {
                        self.diag.linhas = Some(linhas_diag(v.get("rows").unwrap_or(&Value::Null)));
                        self.notificar("Diagnóstico concluído.", TipoAviso::Info);
                    }
                }
                self.cutucar_exec();
            }
            Tag::SalvarConfig | Tag::RestaurarConfig => {
                let restaurar = tag == Tag::RestaurarConfig;
                self.terminar_acao(if restaurar {
                    "cfg_restaurar"
                } else {
                    "cfg_salvar"
                });
                match r {
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                    Ok(v) => {
                        let cfg: Config =
                            serde_json::from_value(v.get("config").cloned().unwrap_or(Value::Null))
                                .unwrap_or_default();
                        self.cfg = Cfg::de(&cfg);
                        let msg = if restaurar {
                            "Padrões restaurados."
                        } else {
                            "Configurações salvas."
                        };
                        self.cfg.msg = msg.into();
                        if let Some(b) = self.boot.as_mut() {
                            b.config = cfg;
                        }
                        self.notificar(msg, TipoAviso::Info);
                    }
                }
                self.cutucar_exec();
            }
            Tag::VerificarInstalacao => {
                if let Err(e) = r {
                    self.erro_verificacao(&e);
                }
                self.cutucar_exec();
            }
            Tag::InstalarFaltantes => {
                match r {
                    Err(e) => {
                        self.terminar_acao("instalar");
                        self.notificar(&e, TipoAviso::Erro);
                    }
                    Ok(v) => {
                        if v.get("status").and_then(Value::as_str) == Some("started") {
                            self.ajuda.instalando = true;
                            self.definir_status("Instalando o que falta…", Tom::Ocupado);
                            self.prog = Prog::Indeterminado;
                            self.dock_aberto = true;
                            self.log_dock("Instalando o que falta…");
                        } else {
                            self.terminar_acao("instalar");
                            self.notificar(
                                "Nada a instalar: tudo já está presente.",
                                TipoAviso::Info,
                            );
                        }
                    }
                }
                self.cutucar_exec();
            }
            Tag::LimparDados => {
                self.terminar_acao("limpar");
                match r {
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                    Ok(v) => {
                        let logs = v.get("logs").and_then(Value::as_u64).unwrap_or(0);
                        let jobs = v.get("jobs").and_then(Value::as_u64).unwrap_or(0);
                        self.notificar(
                            &format!(
                                "Cache limpo: removidos {logs} log(s) e {jobs} pasta(s) de cache."
                            ),
                            TipoAviso::Info,
                        );
                    }
                }
                self.cutucar_exec();
            }
            Tag::AbrirCaminho => {
                if let Err(e) = r {
                    self.notificar(&e, TipoAviso::Erro);
                }
            }
            Tag::AbrirDados => {
                self.terminar_acao("abrir_dados");
                match r {
                    Ok(_) => self.notificar(
                        "Pasta de dados aberta no Explorador de Arquivos.",
                        TipoAviso::Info,
                    ),
                    Err(e) => self.notificar(&e, TipoAviso::Erro),
                }
            }
            Tag::BaixarModelo(_) => {
                if let Err(e) = r {
                    self.tr.baixando = false;
                    self.tr.baixar_msg = e;
                }
                self.cutucar_exec();
            }
            Tag::TestarSpark => {
                self.terminar_acao("spark_testar");
                self.cfg.spark_msg = match r {
                    Ok(v) => {
                        let ms = v.get("latencia_ms").and_then(Value::as_f64);
                        let srv = v
                            .get("servico")
                            .and_then(Value::as_str)
                            .unwrap_or("serviço de transcrição");
                        match ms {
                            Some(ms) => {
                                format!("Conectado ao Spark ({srv}), resposta em {ms:.0} ms.")
                            }
                            None => format!("Conectado ao Spark ({srv})."),
                        }
                    }
                    Err(e) => format!("Sem conexão com o Spark: {e}"),
                };
            }
        }
    }

    fn registrar_launcher(&self, msg: &str) {
        crate::engine::registrar(&self.motor.local().raiz, msg);
    }

    fn aplicar_boot(&mut self, b: Boot) {
        // A versao vem de uma fonte so: VERSION.txt, embutida no .exe pelo build.rs.
        if !b.modos.iter().any(|m| m.key == self.conv.modo) {
            if let Some(m) = b.modos.first() {
                self.conv.modo = m.key.clone();
            }
        }
        if !b.modos_lote.iter().any(|m| m.key == self.lote.modo) {
            if let Some(m) = b.modos_lote.first() {
                self.lote.modo = m.key.clone();
            }
        }
        let primeira = self.boot.is_none();
        if primeira {
            // Depois de uma religacao do motor, mantem o que o usuario ja editou na tela.
            self.cfg = Cfg::de(&b.config);
        }
        self.boot = Some(b);
        self.ajustar_opcoes_tr();
        self.chamar(Tag::TrDeps, "transcricao_deps", vec![]);
        self.cutucar_exec();
        if let Some(b) = &self.boot {
            if b.ocupado.ocr || b.ocupado.transcricao {
                self.definir_status("Operação em andamento...", Tom::Ocupado);
            }
        }
        if primeira || self.ajuda.resultado.is_none() {
            self.verificar_instalacao(true);
        }
        if primeira {
            // Desenvolvimento: EXTRATOR_PRELOAD=a.pdf;b.mp3 ja coloca arquivos nas listas.
            if let Ok(lista) = std::env::var("EXTRATOR_PRELOAD") {
                let v: Vec<String> = lista
                    .split(';')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                self.adicionar_conv(v.clone());
                self.adicionar_tr(v);
            }
            // Desenvolvimento: EXTRATOR_SOLTAR=a.pdf;b.mp3 simula soltar arquivos na tela Inicio.
            if let Ok(lista) = std::env::var("EXTRATOR_SOLTAR") {
                let v: Vec<String> = lista
                    .split(';')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                self.rotear_arquivos(v);
            }
        }
    }

    pub fn ajustar_opcoes_tr(&mut self) {
        let Some(b) = &self.boot else { return };
        let t = &b.transcricao;
        let sel = t.selecionaveis();
        if !sel.contains(&self.tr.modelo) {
            self.tr.modelo = if sel.contains(&t.modelo_padrao) {
                t.modelo_padrao.clone()
            } else {
                sel[0].clone()
            };
        }
        if !t.idiomas.contains(&self.tr.idioma) {
            self.tr.idioma = if t.idiomas.contains(&"pt".to_string()) {
                "pt".into()
            } else {
                t.idiomas.first().cloned().unwrap_or_default()
            };
        }
        if !t.formatos.contains(&self.tr.formato) {
            self.tr.formato = if t.formatos.contains(&"txt".to_string()) {
                "txt".into()
            } else {
                t.formatos.first().cloned().unwrap_or_default()
            };
        }
        let nao: Vec<String> = t.nao_instalados().iter().map(|(n, _)| n.clone()).collect();
        if !nao.contains(&self.tr.baixar_escolha) {
            self.tr.baixar_escolha = nao.first().cloned().unwrap_or_default();
        }
    }

    // ------------------------------------------------------------ eventos

    fn tratar_evento(&mut self, ev: &Value) {
        let tipo = ev.get("type").and_then(Value::as_str).unwrap_or("");
        let txt = |k: &str| ev.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let num = |k: &str| ev.get(k).and_then(Value::as_f64);
        let inteiro = |k: &str| ev.get(k).and_then(Value::as_i64).unwrap_or(0);
        match tipo {
            "line" => self.log_dock(&txt("text")),
            "status" => self.definir_status(txt("text"), Tom::Ocupado),
            "file_start" => {
                let (i, n) = (inteiro("index"), inteiro("total"));
                self.log_dock(&format!("[{i}/{n}] {}", txt("name")));
                self.definir_status(format!("Arquivo {i}/{n}: {}", txt("name")), Tom::Ocupado);
                self.prog = if n > 1 {
                    Prog::Fracao(((i - 1) as f32 / n as f32).clamp(0.0, 1.0))
                } else {
                    Prog::Indeterminado
                };
            }
            "start" => self.log_dock(&format!(
                "Iniciando OCR: {} ({} página(s))",
                txt("file"),
                inteiro("total")
            )),
            "page" => {
                let (c, t) = (inteiro("current"), inteiro("total"));
                let fonte = txt("source");
                let fonte = if fonte.is_empty() {
                    String::new()
                } else {
                    format!(" ({fonte})")
                };
                self.definir_status(format!("Página {c}/{t}{fonte}"), Tom::Ocupado);
                self.prog = Prog::Fracao((c as f32 / t.max(1) as f32).clamp(0.0, 1.0));
            }
            "image" => {
                let (c, t) = (inteiro("current"), inteiro("total"));
                self.definir_status(format!("Imagem {c}/{t}: {}", txt("name")), Tom::Ocupado);
                self.prog = Prog::Fracao((c as f32 / t.max(1) as f32).clamp(0.0, 1.0));
            }
            "file_error" => {
                self.log_dock(&format!("FALHA em {}: {}", txt("name"), txt("text")));
                self.job_erros = true;
            }
            "done" => {
                let erros = self.job_erros;
                self.definir_job(false);
                self.definir_status(
                    if erros {
                        "Concluído com falhas (veja o log)."
                    } else {
                        "Concluído."
                    },
                    if erros { Tom::Erro } else { Tom::Normal },
                );
                self.log_dock("Operação concluída.");
                self.aviso(
                    if erros {
                        "Concluído com falhas. Veja o log."
                    } else {
                        "Concluído."
                    },
                    if erros {
                        TipoAviso::Erro
                    } else {
                        TipoAviso::Info
                    },
                );
                if erros {
                    self.dock_aberto = true;
                }
                self.cutucar_exec();
            }
            "cancelled" => {
                self.definir_job(false);
                self.definir_status("Cancelado.", Tom::Normal);
                self.log_dock("Operação cancelada pelo usuário.");
                self.cutucar_exec();
            }
            "error" => {
                self.definir_job(false);
                let t = txt("text");
                self.notificar(
                    if t.is_empty() {
                        "Erro desconhecido."
                    } else {
                        &t
                    },
                    TipoAviso::Erro,
                );
                self.cutucar_exec();
            }
            "check_start" => {
                self.ajuda.parciais.clear();
            }
            "check_row" => {
                let l = LinhaInstalacao::de_json(&json!({
                    "name": ev.get("name"), "ok": ev.get("ok"), "estado": ev.get("estado"), "detail": ev.get("detail")
                }));
                self.ajuda.contagem = inteiro("index") as usize;
                let nome = l.nome.clone();
                self.ajuda.parciais.push(l);
                self.ajuda.linha = Some((
                    format!(
                        "Verificando… {} componente(s) verificado(s) (último: {nome})",
                        self.ajuda.contagem
                    ),
                    Tom::Ocupado,
                ));
                if !self.ajuda.silenciosa {
                    self.definir_status(format!("Verificando instalação… {nome}"), Tom::Ocupado);
                }
            }
            "check_done" => self.fim_verificacao(ResultadoInstalacao::de_json(ev)),
            "check_error" => {
                let t = txt("text");
                self.erro_verificacao(&t);
            }
            "transc_item_start" => {
                let id = txt("id");
                if let Some(it) = self.tr.itens.iter_mut().find(|i| i.id == id) {
                    it.status = StatusItem::EmAndamento;
                }
                self.tr.selecionado = None;
                let (i, n) = (inteiro("index"), inteiro("total"));
                self.log_dock(&format!("[{i}/{n}] Transcrevendo: {}", txt("name")));
                self.tr.status = format!("Arquivo {i}/{n}: {}", txt("name"));
                self.definir_status(
                    format!("Transcrevendo {i}/{n}: {}", txt("name")),
                    Tom::Ocupado,
                );
                self.tr.pct = 0.0;
                self.tr.feito = "00:00:00 / 00:00:00".into();
                self.tr.restante = "00:00:00".into();
                self.tr.decorrido = "00:00:00".into();
                self.tr.estimativa = "--:--:--".into();
                self.atualizar_resumo_tr();
            }
            "transc_progress" => {
                let pct = num("pct").unwrap_or(0.0);
                self.tr.pct = fmt::frac(pct);
                self.tr.feito = format!(
                    "{} / {} ({}%)",
                    fmt::hms(num("processado_s")),
                    fmt::hms(num("total_s")),
                    pct.round() as i64
                );
                self.tr.restante = fmt::hms(num("restante_s"));
                self.tr.decorrido = fmt::hms(num("elapsed_s"));
                self.tr.estimativa = fmt::hms(num("eta_s"));
                self.prog = Prog::Fracao(fmt::frac(pct));
            }
            "transc_item_done" => {
                let id = txt("id");
                let saida = txt("output_path");
                let resumo = ev.get("interlocutores").map(ler_interlocutores);
                if let Some(it) = self.tr.itens.iter_mut().find(|i| i.id == id) {
                    it.status = StatusItem::Concluido;
                    it.interlocutores = resumo;
                }
                self.log_dock(&format!("Transcrição concluída: {saida}"));
                self.tr.ultima_saida = Some(saida);
                self.atualizar_resumo_tr();
            }
            "transc_item_cancelled" => {
                let id = txt("id");
                if let Some(it) = self.tr.itens.iter_mut().find(|i| i.id == id) {
                    it.status = StatusItem::Cancelado;
                }
                self.log_dock("Item de transcrição cancelado.");
                self.atualizar_resumo_tr();
            }
            "transc_item_error" => {
                let id = txt("id");
                if let Some(it) = self.tr.itens.iter_mut().find(|i| i.id == id) {
                    it.status = StatusItem::Erro;
                }
                self.log_dock(&format!("FALHA na transcrição: {}", txt("text")));
                self.atualizar_resumo_tr();
            }
            "transc_queue_done" => {
                self.definir_tr(false);
                self.tr.status = "Fila concluída.".into();
                self.definir_status("Fila de transcrição concluída.", Tom::Normal);
                self.log_dock("Fila de transcrição encerrada.");
                self.aviso("Fila de transcrição concluída.", TipoAviso::Info);
                self.cutucar_exec();
            }
            "model_download" => {
                let nome = txt("name");
                self.tr.baixando = false;
                if txt("status") == "done" {
                    self.tr.baixar_msg = format!("Modelo '{nome}' pronto.");
                    self.aviso(&format!("Modelo '{nome}' pronto."), TipoAviso::Info);
                    self.chamar(Tag::TrEstado, "transcricao_state", vec![]);
                } else {
                    self.tr.baixar_msg = format!("Falha: {}", txt("text"));
                }
                self.cutucar_exec();
            }
            "install_fix_done" => {
                self.log_dock("Instalação automática concluída. Verificando novamente...");
                self.ajuda.instalando = false;
                self.terminar_acao("instalar");
                self.esconder_progresso_se_livre();
                self.verificar_instalacao(true);
                self.chamar(Tag::TrEstado, "transcricao_state", vec![]);
                self.chamar(Tag::TrDeps, "transcricao_deps", vec![]);
                self.cutucar_exec();
            }
            _ => {}
        }
    }

    // --------------------------------------------------- instalacao (Ajuda)

    pub fn verificar_instalacao(&mut self, silenciosa: bool) {
        if self.ajuda.verificando {
            if !silenciosa {
                self.ajuda.silenciosa = false;
                self.ajuda.linha = Some((
                    format!(
                        "Verificando… {} componente(s) verificado(s)",
                        self.ajuda.contagem
                    ),
                    Tom::Ocupado,
                ));
            }
            return;
        }
        self.ajuda.verificando = true;
        self.ajuda.silenciosa = silenciosa;
        self.ajuda.contagem = 0;
        self.ajuda.parciais.clear();
        self.ajuda.linha = Some((
            "Verificando… isso pode levar alguns segundos.".into(),
            Tom::Ocupado,
        ));
        if !silenciosa {
            self.definir_status("Verificando instalação…", Tom::Ocupado);
            self.prog = Prog::Indeterminado;
            self.log_dock("Verificando instalação…");
        }
        self.chamar(Tag::VerificarInstalacao, "start_check_installation", vec![]);
    }

    fn fim_verificacao(&mut self, r: ResultadoInstalacao) {
        self.ajuda.verificando = false;
        self.esconder_progresso_se_livre();
        let resumo = r.resumo();
        let falhas =
            r.linhas.iter().any(|l| l.estado == EstadoComp::Falha) || !r.faltantes().is_empty();
        let tom = if falhas {
            Tom::Erro
        } else if r.problemas().is_empty() {
            Tom::Normal
        } else {
            Tom::Aviso
        };
        self.ajuda.linha = Some((resumo.clone(), tom));
        let silenciosa = self.ajuda.silenciosa;
        self.ajuda.resultado = Some(r);
        if !silenciosa {
            self.definir_status(resumo.clone(), tom);
            self.log_dock(&resumo);
            // So falha de verdade vira erro (e linha no log); aviso e informativo.
            self.aviso(
                &resumo,
                if falhas {
                    TipoAviso::Erro
                } else {
                    TipoAviso::Info
                },
            );
        }
    }

    fn erro_verificacao(&mut self, texto: &str) {
        self.ajuda.verificando = false;
        self.esconder_progresso_se_livre();
        let msg = format!(
            "Não foi possível verificar a instalação: {}",
            if texto.is_empty() {
                "erro desconhecido"
            } else {
                texto
            }
        );
        self.ajuda.linha = Some((msg.clone(), Tom::Erro));
        self.ajuda.parciais.clear();
        self.notificar(&msg, TipoAviso::Erro);
    }

    /// Selo de aviso no menu: ha componente com falha ou algo para instalar.
    pub fn selo_ajuda(&self) -> bool {
        self.ajuda
            .resultado
            .as_ref()
            .map(|r| {
                r.linhas.iter().any(|l| l.estado == EstadoComp::Falha) || !r.faltantes().is_empty()
            })
            .unwrap_or(false)
    }

    // ---------------------------------------------------------- execucoes

    fn aplicar_exec(&mut self, l: ListaExecucoes) {
        let ids: HashSet<&str> = l.execucoes.iter().map(|e| e.id.as_str()).collect();
        self.exec.abertos.retain(|i| ids.contains(i.as_str()));
        self.exec.linhas.retain(|i, _| ids.contains(i.as_str()));
        self.exec.proximo.retain(|i, _| ids.contains(i.as_str()));
        self.exec.cancelando.retain(|i| ids.contains(i.as_str()));
        self.exec.lista = l;
    }

    pub fn atualizar_resumo_tr(&mut self) {
        let pend = self
            .tr
            .itens
            .iter()
            .filter(|i| i.status == StatusItem::Pendente)
            .count();
        if !self.tr.rodando {
            self.tr.status = if self.tr.itens.is_empty() {
                "Fila vazia.".into()
            } else if self.tr.status.starts_with("Fila conclu")
                || self.tr.status.starts_with("Arquivo ")
            {
                self.tr.status.clone()
            } else {
                "Pronto para iniciar.".into()
            };
        }
        let _ = pend;
    }
}

impl Cfg {
    pub fn de(c: &Config) -> Cfg {
        Cfg {
            idioma: c.language.clone(),
            dpi: c.dpi.to_string(),
            psm: c.psm.to_string(),
            limite: c.native_text_threshold.to_string(),
            cpu: c.cpu_mode.clone(),
            pdf: c.pdf_output_type.clone(),
            verbosidade: c.verbosity.clone(),
            girar: c.rotate_pages,
            inclinadas: c.deskew,
            retomar: c.resume,
            abrir: c.auto_open_output,
            motor: c.motor.clone(),
            msg: String::new(),
            spark_msg: String::new(),
            base: c.clone(),
        }
    }

    /// Valores enviados a `save_settings` (o que a tela desconhece volta como estava).
    pub fn para_json(&self) -> Value {
        let mut m = self.base.extra.clone();
        let inteiro = |s: &str| -> Value {
            s.trim()
                .parse::<i64>()
                .map(Value::from)
                .unwrap_or_else(|_| Value::from(s.to_string()))
        };
        m.insert("language".into(), json!(self.idioma));
        m.insert("dpi".into(), inteiro(&self.dpi));
        m.insert("psm".into(), inteiro(&self.psm));
        m.insert("native_text_threshold".into(), inteiro(&self.limite));
        m.insert("cpu_mode".into(), json!(self.cpu));
        m.insert("pdf_output_type".into(), json!(self.pdf));
        m.insert("verbosity".into(), json!(self.verbosidade));
        m.insert("rotate_pages".into(), json!(self.girar));
        m.insert("deskew".into(), json!(self.inclinadas));
        m.insert("resume".into(), json!(self.retomar));
        m.insert("auto_open_output".into(), json!(self.abrir));
        m.insert("motor".into(), json!(self.motor));
        Value::Object(m)
    }
}

fn linhas_diag(rows: &Value) -> Vec<(String, String)> {
    rows.as_array()
        .map(|a| {
            a.iter()
                .map(|r| {
                    (
                        r.get("item")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        r.get("result")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn resumo_nomes(nomes: &[String], max: usize) -> String {
    let mut s = nomes
        .iter()
        .take(max)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    if nomes.len() > max {
        s.push_str(&format!("\n… e mais {}", nomes.len() - max));
    }
    s
}

fn ler_interlocutores(v: &Value) -> ResumoInterlocutores {
    let lista = |k: &str| -> Vec<Value> {
        v.get(k)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let mut rotulos: Vec<String> = lista("rotulos")
        .iter()
        .filter_map(|x| x.as_str().map(str::to_string))
        .collect();
    let n = v
        .get("n")
        .and_then(Value::as_u64)
        .unwrap_or(rotulos.len() as u64) as usize;
    while rotulos.len() < n {
        rotulos.push(format!("Interlocutor {}", rotulos.len() + 1));
    }
    ResumoInterlocutores {
        rotulos,
        tempo_s: lista("tempo_s").iter().filter_map(Value::as_f64).collect(),
        trocas: v.get("trocas").and_then(Value::as_u64),
    }
}

impl LogSt {
    pub fn nivel_chave(&self) -> &'static str {
        NIVEIS_LOG[self.nivel.min(2)].0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_volta_com_campos_desconhecidos_e_numeros_inteiros() {
        let mut c = Config::default();
        c.extra.insert("futuro".into(), json!("x"));
        let mut cfg = Cfg::de(&c);
        cfg.dpi = "300".into();
        cfg.psm = "abc".into();
        let v = cfg.para_json();
        assert_eq!(v["dpi"], 300);
        assert_eq!(
            v["psm"], "abc",
            "valor invalido segue como texto para o motor recusar com mensagem"
        );
        assert_eq!(v["futuro"], "x");
        assert_eq!(v["motor"], "local");
    }

    #[test]
    fn nomes_assinados_limitados() {
        let nomes: Vec<String> = (1..=15).map(|i| format!("a{i}.pdf")).collect();
        let s = resumo_nomes(&nomes, 12);
        assert!(s.ends_with("… e mais 3"));
        assert_eq!(s.lines().count(), 13);
    }

    #[test]
    fn interlocutores_completam_rotulos() {
        let r = ler_interlocutores(
            &json!({"n": 3, "rotulos": ["Ana"], "tempo_s": [10.0, 5.0], "trocas": 4}),
        );
        assert_eq!(r.rotulos, ["Ana", "Interlocutor 2", "Interlocutor 3"]);
        assert_eq!(r.tempo_s.len(), 2);
        assert_eq!(r.trocas, Some(4));
    }

    #[test]
    fn diagnostico_em_linhas() {
        let l = linhas_diag(&json!([{"item": "Páginas", "result": "42"}]));
        assert_eq!(l, vec![("Páginas".to_string(), "42".to_string())]);
    }
}
