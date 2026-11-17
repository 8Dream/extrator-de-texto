//! Apoio a desenvolvimento e a capturas de tela. So age quando as variaveis de ambiente existem:
//! `EXTRATOR_CAPTURA=arquivo.png` grava uma captura da propria janela e fecha o programa;
//! `EXTRATOR_TAMANHO=1000x700` define o tamanho da janela; `EXTRATOR_FORA_DA_TELA=1` abre a janela
//! fora da area visivel, sem foco e sem botao na barra de tarefas (para nao atrapalhar quem esta
//! usando outro programa, como um jogo em tela cheia).

use crate::app::App;
use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Teto (so para dar erro claro) para a captura ficar pronta.
const TETO: Duration = Duration::from_secs(60);
/// Sem historico de execucoes, a captura segue depois deste tempo.
const SEM_HISTORICO: Duration = Duration::from_secs(8);

pub struct Captura {
    caminho: PathBuf,
    inicio: Instant,
    pedida: bool,
    quadros_prontos: u32,
    /// 0 = esperando, 1 = gravada, 2 = falhou ao gravar.
    resultado: Arc<AtomicU8>,
}

impl Captura {
    pub fn de_ambiente() -> Option<Captura> {
        let c = std::env::var_os("EXTRATOR_CAPTURA").filter(|v| !v.is_empty())?;
        Some(Captura {
            caminho: PathBuf::from(c),
            inicio: Instant::now(),
            pedida: false,
            quadros_prontos: 0,
            resultado: Arc::new(AtomicU8::new(0)),
        })
    }
}

/// Tamanho pedido em EXTRATOR_TAMANHO ("1000x700"), em pontos.
pub fn tamanho_da_janela() -> Option<[f32; 2]> {
    let t = std::env::var("EXTRATOR_TAMANHO").ok()?;
    let (w, h) = t.split_once('x')?;
    Some([w.trim().parse().ok()?, h.trim().parse().ok()?])
}

pub fn fora_da_tela() -> bool {
    std::env::var_os("EXTRATOR_FORA_DA_TELA").is_some_and(|v| !v.is_empty())
}

fn gravar_png(caminho: &std::path::Path, img: &egui::ColorImage) -> Result<(), String> {
    let arquivo = std::fs::File::create(caminho).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(
        std::io::BufWriter::new(arquivo),
        img.size[0] as u32,
        img.size[1] as u32,
    );
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_array()).collect();
    w.write_image_data(&bytes).map_err(|e| e.to_string())
}

impl App {
    /// Um passo da captura: espera a tela ficar pronta, pede a imagem, grava e encerra.
    pub fn passo_captura(&mut self, ctx: &egui::Context) {
        let Some(c) = self.captura.as_mut() else {
            return;
        };
        ctx.request_repaint_after(Duration::from_millis(50));
        let estado = c.resultado.load(Ordering::SeqCst);
        if estado != 0 || c.inicio.elapsed() > TETO {
            if estado != 1 {
                crate::engine::registrar(
                    &self.motor.local().raiz,
                    "Captura de tela nao concluida (tempo esgotado ou falha ao gravar).",
                );
            }
            self.captura = None;
            self.comecar_encerramento();
            return;
        }
        if c.pedida {
            return;
        }
        let pronto = self.boot.is_some()
            && self.ajuda.resultado.is_some()
            && !self.ajuda.verificando
            && (self.exec.lista.total > 0 || c.inicio.elapsed() > SEM_HISTORICO);
        if !pronto {
            return;
        }
        c.quadros_prontos += 1;
        if c.quadros_prontos < 4 {
            return;
        }
        c.pedida = true;
        let caminho = c.caminho.clone();
        let resultado = c.resultado.clone();
        ctx.request_screenshot(move |img| {
            let ok = gravar_png(&caminho, &img).is_ok();
            resultado.store(if ok { 1 } else { 2 }, Ordering::SeqCst);
        });
    }
}
