//! Canal unico entre as threads de fundo e a interface.
//!
//! Toda chamada ao motor, selecao de arquivo etc. roda fora da thread da
//! interface e devolve o resultado como `Msg`; a interface so desenha estado.

use crate::proto::Resposta;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

#[derive(Clone, Debug, PartialEq)]
pub enum LogUso {
    Abrir,
    Acompanhar,
    Anteriores,
    Intervalo,
    Redor(i64),
}

/// Identifica a que pergunta uma resposta do motor pertence.
#[derive(Clone, Debug, PartialEq)]
pub enum Tag {
    Boot,
    Capacidades,
    Eventos,
    ExecLista,
    ExecDetalhe(String),
    ExecCancelar(String),
    ExecRepetir(String),
    ExecResumo(String),
    ExecLimpar,
    LogArquivos,
    LogLer(LogUso),
    LogBuscar,
    LogRegistrar(u64),
    LogPasta,
    TrEstado,
    TrDeps,
    IniciarConversao(bool),
    AnalisarLote,
    IniciarLote,
    IniciarTr,
    PausarTr,
    CancelarTr,
    CancelarTrabalho,
    Diagnosticar,
    SalvarConfig,
    RestaurarConfig,
    VerificarInstalacao,
    InstalarFaltantes,
    LimparDados,
    AbrirCaminho,
    AbrirDados,
    BaixarModelo(String),
    TestarSpark,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Escolha {
    ArquivosInicio,
    ArquivosConverter,
    ArquivosTranscricao,
    ArquivoDiagnostico,
    PastaSaida,
    PastaLoteEntrada,
    PastaLoteSaida,
}

/// Estado do processo do motor, como a interface o enxerga.
#[derive(Clone, Debug, PartialEq)]
pub enum EstadoMotor {
    Iniciando,
    Pronto,
    /// Caiu; `tentativa` e o numero da religacao automatica em curso.
    Caiu {
        motivo: String,
        tentativa: u32,
    },
    /// Nao sobe (ambiente ausente ou quedas repetidas).
    Falhou(String),
}

#[derive(Debug)]
pub enum Msg {
    Resposta(Tag, Resposta),
    Motor(EstadoMotor),
    Arquivos(Escolha, Vec<PathBuf>),
    Pasta(Escolha, Option<PathBuf>),
    /// O motor terminou: a janela pode fechar.
    PodeFechar,
}

#[derive(Clone)]
pub struct Bus {
    tx: Sender<Msg>,
    ctx: eframe::egui::Context,
}

impl Bus {
    pub fn novo(tx: Sender<Msg>, ctx: eframe::egui::Context) -> Self {
        Bus { tx, ctx }
    }

    pub fn enviar(&self, m: Msg) {
        // Se a interface ja saiu, ninguem mais escuta: descartar e normal.
        let _ = self.tx.send(m);
        self.ctx.request_repaint();
    }
}
