//! Extrator de Texto: interface nativa (egui). O motor de conversao, OCR e transcricao
//! continua em Python e conversa com esta interface por JSON, linha a linha (ver `proto`).
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod app;
mod app_ui;
mod bus;
mod cores;
mod dev;
mod engine;
mod fmt;
mod icons;
mod model;
mod pg_aparencia;
mod pg_conv;
mod pg_exec;
mod pg_inicio;
mod pg_log;
mod pg_misc;
mod pg_tr;
mod prefs;
mod proto;
mod reparar;
mod rotas;
mod theme;
mod widgets;
mod winutil;

use eframe::egui;
use std::sync::mpsc;

/// Versao lida de VERSION.txt na compilacao (build.rs).
pub const VERSAO: &str = env!("EXTRATOR_VERSION");
const PREFIXO_TITULO: &str = "Extrator de Texto v";

fn main() {
    let reparo = std::env::args().skip(1).any(|a| a == "--reparar");
    let local = engine::Local::descobrir();

    let raiz_log = local.raiz.clone();
    std::panic::set_hook(Box::new(move |info| {
        engine::registrar(&raiz_log, &format!("Falha inesperada: {info}"));
    }));

    // Desenvolvimento: EXTRATOR_TITULO da um titulo proprio a janela (e uma instancia propria),
    // para nao ser confundida com a do programa instalado.
    let titulo_dev = titulo_de_desenvolvimento();
    let sufixo: String = titulo_dev
        .as_deref()
        .map(|t| t.chars().filter(char::is_ascii_alphanumeric).collect())
        .unwrap_or_default();
    let nome = if reparo {
        format!("Local\\ExtratorDeTexto.Reparar{sufixo}")
    } else {
        format!("Local\\ExtratorDeTexto.Instancia{sufixo}")
    };
    let Some(_unica) = winutil::instancia_unica(&nome) else {
        if !reparo {
            winutil::focar_janela(titulo_dev.as_deref().unwrap_or(PREFIXO_TITULO));
        }
        return;
    };

    let desenvolvimento = std::env::var_os("EXTRATOR_MOTOR").is_some();
    if reparo {
        reparar::executar(local);
        return;
    }
    if !desenvolvimento && local.precisa_reparo() {
        engine::registrar(
            &local.raiz,
            &format!(
                "Ambiente incompleto ({}). Abrindo o reparo.",
                local.python.display()
            ),
        );
        reparar::executar(local);
        return;
    }
    executar(local);
}

/// Titulo definido por EXTRATOR_TITULO (so para desenvolvimento e capturas de tela).
fn titulo_de_desenvolvimento() -> Option<String> {
    std::env::var("EXTRATOR_TITULO")
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

fn executar(local: engine::Local) {
    let titulo = titulo_de_desenvolvimento().unwrap_or_else(|| format!("{PREFIXO_TITULO}{VERSAO}"));
    let mut janela = egui::ViewportBuilder::default()
        .with_title(titulo.clone())
        .with_inner_size(dev::tamanho_da_janela().unwrap_or([1220.0, 820.0]))
        .with_min_inner_size([1000.0, 660.0])
        .with_icon(winutil::icone_janela().unwrap_or_default())
        .with_drag_and_drop(true);
    if dev::fora_da_tela() {
        janela = janela
            .with_position([-6000.0, -6000.0])
            .with_active(false)
            .with_taskbar(false);
    }
    let opcoes = eframe::NativeOptions {
        viewport: janela,
        ..Default::default()
    };
    let raiz = local.raiz.clone();
    let r = eframe::run_native(
        &titulo,
        opcoes,
        Box::new(move |cc| {
            let (tx, rx) = mpsc::channel();
            let bus = bus::Bus::novo(tx, cc.egui_ctx.clone());
            let motor = engine::Motor::novo(bus.clone(), local);
            motor.iniciar();
            Ok(Box::new(app::App::novo(
                cc,
                rx,
                bus,
                motor,
                VERSAO.to_string(),
            )))
        }),
    );
    if let Err(e) = r {
        engine::registrar(&raiz, &format!("A janela não pôde ser aberta: {e}"));
    }
}
