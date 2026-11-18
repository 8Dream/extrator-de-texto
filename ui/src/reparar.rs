//! Modo `--reparar`: janela propria que roda scripts\install.ps1 oculto e mostra o log ao vivo.

use crate::engine::{registrar, Local};
use crate::theme::{self, negrito, Palette};
use crate::widgets::{barra, btn, mono};
use eframe::egui::{self, CornerRadius, Frame, Margin, RichText, ScrollArea, Stroke};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Abre a janela de reparo como outro processo (usado pelo botao "Reparar instalacao").
pub fn abrir_reparo() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = Command::new(exe).arg("--reparar").spawn();
    }
}

/// Decodifica o log do PowerShell: UTF-8 quando valido; senao Windows-1252 (o `-Encoding Default` do PS 5.1).
pub fn decodificar(bytes: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    const ALTA: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    bytes
        .iter()
        .map(|&b| {
            if (0x80..0xA0).contains(&b) {
                ALTA[(b - 0x80) as usize]
            } else {
                b as char
            }
        })
        .collect()
}

/// Linhas completas do texto (ignora a ultima se ainda nao terminou em quebra de linha).
pub fn linhas_completas(texto: &str) -> Vec<String> {
    let t = texto.replace("\r\n", "\n");
    let mut v: Vec<&str> = t.split('\n').collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v.into_iter().map(|l| l.trim_end().to_string()).collect()
}

/// Passo atual: ultima linha "==> texto".
pub fn passo_atual(linhas: &[String]) -> Option<String> {
    linhas
        .iter()
        .rev()
        .find_map(|l| l.trim().strip_prefix("==> ").map(str::to_string))
}

/// Mensagem final conforme o codigo de saida.
pub fn mensagem_final(codigo: i32) -> String {
    if codigo == 0 {
        "Concluído. Já pode abrir o Extrator de Texto.".to_string()
    } else {
        format!("A instalação falhou (código {codigo}). Veja o motivo no registro abaixo.")
    }
}

fn caminho_powershell() -> PathBuf {
    let win = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:\\Windows"));
    win.join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

struct Reparo {
    local: Local,
    pal: Palette,
    log: PathBuf,
    pronto: PathBuf,
    filho: Option<Child>,
    linhas: Vec<String>,
    passo: String,
    codigo: Option<i32>,
    erro_inicio: Option<String>,
}

impl Reparo {
    fn novo(cc: &eframe::CreationContext<'_>, local: Local) -> Reparo {
        theme::instalar_fontes(&cc.egui_ctx);
        let ap = crate::prefs::ler(&local.raiz.join("dados"));
        let sistema_escuro = crate::winutil::tema_escuro_do_windows()
            .unwrap_or(cc.egui_ctx.system_theme() == Some(egui::Theme::Dark));
        let (pal, _) = Palette::montar(ap.escuro(sistema_escuro), ap.destaque.rgb());
        theme::aplicar(&cc.egui_ctx, &pal);
        cc.egui_ctx.set_zoom_factor(ap.texto.escala());
        let log = local.dados_logs().join("instalacao.log");
        let pronto =
            std::env::temp_dir().join(format!("extrator-reparo-{}.done", std::process::id()));
        let mut r = Reparo {
            local,
            pal,
            log,
            pronto,
            filho: None,
            linhas: Vec::new(),
            passo: "Verificando e instalando o que falta...".into(),
            codigo: None,
            erro_inicio: None,
        };
        r.iniciar();
        r
    }

    fn iniciar(&mut self) {
        let ps1 = self.local.raiz.join("scripts").join("install.ps1");
        if !ps1.is_file() {
            let msg = format!("install.ps1 não encontrado em {}", ps1.display());
            registrar(&self.local.raiz, &msg);
            self.erro_inicio = Some(msg);
            self.codigo = Some(1);
            return;
        }
        let _ = std::fs::create_dir_all(self.local.dados_logs());
        let _ = std::fs::remove_file(&self.pronto);
        let _ = std::fs::write(&self.log, "");
        let r = Command::new(caminho_powershell())
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(&ps1)
            .arg("-LogFile")
            .arg(&self.log)
            .arg("-DoneFile")
            .arg(&self.pronto)
            .current_dir(&self.local.raiz)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn();
        match r {
            Ok(f) => self.filho = Some(f),
            Err(e) => {
                let msg = format!("Não foi possível executar o PowerShell: {e}");
                registrar(&self.local.raiz, &msg);
                self.erro_inicio = Some(msg);
                self.codigo = Some(1);
            }
        }
    }

    fn atualizar(&mut self) {
        if let Ok(b) = std::fs::read(&self.log) {
            self.linhas = linhas_completas(&decodificar(&b));
            if let Some(p) = passo_atual(&self.linhas) {
                self.passo = p;
            }
        }
        if self.codigo.is_none() {
            let terminou = self
                .filho
                .as_mut()
                .map(|f| f.try_wait().ok().flatten().is_some())
                .unwrap_or(false);
            if self.pronto.exists() || terminou {
                let c = std::fs::read_to_string(&self.pronto)
                    .ok()
                    .and_then(|t| t.trim().parse::<i32>().ok())
                    .unwrap_or(1);
                self.codigo = Some(c);
                let _ = std::fs::remove_file(&self.pronto);
                if c != 0 {
                    registrar(
                        &self.local.raiz,
                        &format!("O reparo terminou com código {c}."),
                    );
                }
            }
        }
    }
}

impl eframe::App for Reparo {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.atualizar();
        // Sem fechar no meio da instalacao.
        if self.codigo.is_none() && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        let pal = self.pal;
        egui::CentralPanel::default()
            .frame(Frame::new().fill(pal.bg).inner_margin(Margin::same(16)))
            .show(ui, |ui| {
                let status = match (self.codigo, &self.erro_inicio) {
                    (Some(_), Some(e)) => e.clone(),
                    (Some(c), None) => mensagem_final(c),
                    (None, _) => self.passo.clone(),
                };
                let cor = match self.codigo {
                    Some(0) => pal.ok,
                    Some(_) => pal.danger,
                    None => pal.text,
                };
                ui.add(egui::Label::new(
                    RichText::new(status)
                        .family(negrito())
                        .size(15.0)
                        .color(cor),
                ));
                ui.add_space(4.0);
                let w = ui.available_width();
                match self.codigo {
                    None => barra(ui, &pal, None, w, 10.0, None),
                    Some(0) => barra(ui, &pal, Some(1.0), w, 10.0, Some(pal.ok)),
                    Some(_) => barra(ui, &pal, Some(0.0), w, 10.0, None),
                }
                ui.add_space(8.0);
                let altura = ui.available_height() - 46.0;
                Frame::new()
                    .fill(pal.surface)
                    .stroke(Stroke::new(1.0, pal.border))
                    .corner_radius(CornerRadius::same(7))
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_height(altura.max(100.0));
                        ScrollArea::both()
                            .auto_shrink([false, false])
                            .stick_to_bottom(true)
                            .show(ui, |ui| {
                                for l in &self.linhas {
                                    ui.add(
                                        egui::Label::new(mono(l.as_str(), pal.text))
                                            .wrap_mode(egui::TextWrapMode::Extend),
                                    );
                                }
                            });
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    crate::widgets::a_direita(ui, |ui| {
                        if btn("Fechar")
                            .ativo(self.codigo.is_some())
                            .primario()
                            .largura(100.0)
                            .mostrar(ui, &pal)
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if self.codigo == Some(0)
                            && btn("Abrir o Extrator de Texto")
                                .largura(190.0)
                                .mostrar(ui, &pal)
                                .clicked()
                        {
                            if let Ok(exe) = std::env::current_exe() {
                                let _ = Command::new(exe).spawn();
                            }
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
            });
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

/// Executa a janela de reparo (bloqueia ate fechar).
pub fn executar(local: Local) {
    let opcoes = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Extrator de Texto - Reparar instalação")
            .with_inner_size([660.0, 460.0])
            .with_min_inner_size([480.0, 320.0])
            .with_icon(crate::winutil::icone_janela().unwrap_or_default()),
        ..Default::default()
    };
    let l2 = local.clone();
    if let Err(e) = eframe::run_native(
        "Extrator de Texto - Reparar",
        opcoes,
        Box::new(move |cc| Ok(Box::new(Reparo::novo(cc, l2)))),
    ) {
        registrar(
            &local.raiz,
            &format!("Falha ao abrir a janela de reparo: {e}"),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodifica_utf8_e_cp1252() {
        assert_eq!(decodificar("instalação".as_bytes()), "instalação");
        // Windows-1252: ç = E7, ã = E3
        assert_eq!(decodificar(&[0x69, 0xE7, 0xE3]), "içã");
        assert_eq!(decodificar(&[0x93, 0x61, 0x94]), "“a”");
    }

    #[test]
    fn linhas_ignoram_a_ultima_incompleta_e_o_cr() {
        assert_eq!(linhas_completas("a\r\nb\r\n"), ["a", "b"]);
        assert_eq!(linhas_completas(""), Vec::<String>::new());
        assert_eq!(linhas_completas("a\nb"), ["a", "b"]);
    }

    #[test]
    fn passo_e_a_ultima_linha_com_seta() {
        let l = vec![
            "==> Python".to_string(),
            "baixando".to_string(),
            "  ==> Tesseract".to_string(),
            "ok".to_string(),
        ];
        assert_eq!(passo_atual(&l).as_deref(), Some("Tesseract"));
        assert_eq!(passo_atual(&["x".to_string()]), None);
    }

    #[test]
    fn mensagens_finais() {
        assert_eq!(
            mensagem_final(0),
            "Concluído. Já pode abrir o Extrator de Texto."
        );
        assert!(mensagem_final(2).contains("código 2"));
    }
}
