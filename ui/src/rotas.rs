//! Roteamento inteligente da tela Inicio: separa os arquivos por extensao entre
//! Converter (documentos e imagens) e Transcricao (audio e video). Logica pura, sem interface.

use crate::fmt;
use crate::model::{Extensoes, Modo};

const MIDIA_PADRAO: [&str; 13] = [
    ".mp3", ".m4a", ".wav", ".ogg", ".opus", ".flac", ".aac", ".wma", ".mp4", ".mkv", ".mov",
    ".avi", ".webm",
];
const PDF_PADRAO: [&str; 1] = [".pdf"];
const DIRETOS_PADRAO: [&str; 10] = [
    ".pdf", ".docx", ".pptx", ".xlsx", ".html", ".htm", ".txt", ".csv", ".json", ".xml",
];
const IMAGENS_PADRAO: [&str; 7] = [".png", ".jpg", ".jpeg", ".tif", ".tiff", ".bmp", ".webp"];

/// Lista do motor; se ele ainda nao informou, usa a lista embutida.
fn efetiva(do_motor: &[String], padrao: &[&str]) -> Vec<String> {
    if do_motor.is_empty() {
        padrao.iter().map(|s| (*s).to_string()).collect()
    } else {
        do_motor.iter().map(|s| s.to_lowercase()).collect()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrupoDoc {
    Pdf,
    Direto,
    Imagens,
}

/// Arquivos separados por destino.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct Roteamento {
    pub pdfs: Vec<String>,
    /// Word, Excel, PowerPoint, HTML, TXT, CSV, JSON e XML.
    pub diretos: Vec<String>,
    pub imagens: Vec<String>,
    /// Audio e video.
    pub midia: Vec<String>,
    /// Sem extensao ou formato desconhecido.
    pub ignorados: Vec<String>,
}

/// Classifica por extensao (sem diferenciar maiusculas). Repetidos entram uma vez so.
pub fn classificar(caminhos: &[String], ext: &Extensoes) -> Roteamento {
    let midia = efetiva(&ext.media, &MIDIA_PADRAO);
    let pdf = efetiva(&ext.pdf, &PDF_PADRAO);
    let imagens = efetiva(&ext.images, &IMAGENS_PADRAO);
    let diretos = efetiva(&ext.direct, &DIRETOS_PADRAO);
    let mut r = Roteamento::default();
    let mut vistos: Vec<&str> = Vec::new();
    for c in caminhos {
        if c.trim().is_empty() || vistos.contains(&c.as_str()) {
            continue;
        }
        vistos.push(c);
        let e = fmt::ext_of(c);
        let destino = if e.is_empty() {
            &mut r.ignorados
        } else if midia.contains(&e) {
            &mut r.midia
        } else if pdf.contains(&e) {
            &mut r.pdfs
        } else if imagens.contains(&e) {
            &mut r.imagens
        } else if diretos.contains(&e) {
            &mut r.diretos
        } else {
            &mut r.ignorados
        };
        destino.push(c.clone());
    }
    r
}

impl Roteamento {
    pub fn documentos(&self) -> usize {
        self.pdfs.len() + self.diretos.len() + self.imagens.len()
    }

    /// Nada que o programa saiba processar.
    pub fn sem_aproveitaveis(&self) -> bool {
        self.documentos() == 0 && self.midia.is_empty()
    }

    /// Ha documentos e audio/video ao mesmo tempo.
    pub fn mistura(&self) -> bool {
        self.documentos() > 0 && !self.midia.is_empty()
    }

    /// O tipo de documento com mais arquivos (empate: PDF, depois direto, depois imagens).
    /// O Converter trabalha com um modo por vez; os de outro tipo ficam de fora (`sobra`).
    pub fn documentos_do_grupo_principal(&self) -> Option<(GrupoDoc, Vec<String>, usize)> {
        let grupos = [
            (GrupoDoc::Pdf, &self.pdfs),
            (GrupoDoc::Direto, &self.diretos),
            (GrupoDoc::Imagens, &self.imagens),
        ];
        let melhor = grupos.iter().filter(|(_, l)| !l.is_empty()).fold(
            None::<&(GrupoDoc, &Vec<String>)>,
            |m, g| match m {
                Some(a) if a.1.len() >= g.1.len() => Some(a),
                _ => Some(g),
            },
        )?;
        let total = self.documentos();
        Some((melhor.0, melhor.1.clone(), total - melhor.1.len()))
    }
}

/// Modo do Converter para o tipo de documento: OCR automatico para PDF; o primeiro modo que
/// aceita o tipo nos demais casos.
pub fn modo_para(grupo: GrupoDoc, modos: &[Modo]) -> Option<String> {
    let aceita = match grupo {
        GrupoDoc::Pdf => "pdf",
        GrupoDoc::Direto => "direct",
        GrupoDoc::Imagens => "images",
    };
    if grupo == GrupoDoc::Pdf {
        if let Some(m) = modos.iter().find(|m| m.key == "auto") {
            return Some(m.key.clone());
        }
    }
    modos
        .iter()
        .find(|m| m.accepts == aceita)
        .map(|m| m.key.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(l: &[&str]) -> Vec<String> {
        l.iter().map(|s| (*s).to_string()).collect()
    }

    fn ext_motor() -> Extensoes {
        Extensoes {
            pdf: v(&[".pdf"]),
            direct: v(&[".pdf", ".docx", ".xlsx"]),
            images: v(&[".png", ".jpg"]),
            media: v(&[".mp3", ".mp4", ".wav"]),
        }
    }

    #[test]
    fn separa_documentos_imagens_e_midia() {
        let r = classificar(
            &v(&[
                r"C:\a\Peça.pdf",
                r"C:\a\planilha.xlsx",
                r"C:\a\foto.png",
                r"C:\a\aula.mp3",
                r"C:\a\video.mp4",
                r"C:\a\leiame",
                r"C:\a\programa.exe",
            ]),
            &ext_motor(),
        );
        assert_eq!(r.pdfs, v(&[r"C:\a\Peça.pdf"]));
        assert_eq!(r.diretos, v(&[r"C:\a\planilha.xlsx"]));
        assert_eq!(r.imagens, v(&[r"C:\a\foto.png"]));
        assert_eq!(r.midia, v(&[r"C:\a\aula.mp3", r"C:\a\video.mp4"]));
        assert_eq!(r.ignorados, v(&[r"C:\a\leiame", r"C:\a\programa.exe"]));
        assert!(r.mistura());
    }

    #[test]
    fn maiusculas_nao_atrapalham() {
        let r = classificar(
            &v(&[r"C:\A\FOTO.JPG", r"C:\A\AULA.Mp3", r"C:\A\X.PDF"]),
            &ext_motor(),
        );
        assert_eq!(r.imagens.len(), 1);
        assert_eq!(r.midia.len(), 1);
        assert_eq!(r.pdfs.len(), 1);
        assert!(r.ignorados.is_empty());
    }

    #[test]
    fn sem_extensao_e_so_ponto_final_sao_ignorados() {
        let r = classificar(
            &v(&["semext", r"C:\pasta.com.ponto\arquivo", "termina."]),
            &ext_motor(),
        );
        assert_eq!(r.ignorados.len(), 3);
        assert!(r.sem_aproveitaveis());
        assert!(!r.mistura());
    }

    #[test]
    fn so_documentos_ou_so_midia_nao_e_mistura() {
        let docs = classificar(&v(&["a.pdf", "b.docx"]), &ext_motor());
        assert!(!docs.mistura() && docs.documentos() == 2);
        let audio = classificar(&v(&["a.mp3", "b.wav"]), &ext_motor());
        assert!(!audio.mistura() && audio.documentos() == 0 && !audio.sem_aproveitaveis());
    }

    #[test]
    fn repetidos_e_vazios_sao_descartados() {
        let r = classificar(&v(&["a.pdf", "a.pdf", "", "  "]), &ext_motor());
        assert_eq!(r.pdfs, v(&["a.pdf"]));
        assert!(r.ignorados.is_empty());
        assert!(classificar(&[], &ext_motor()).sem_aproveitaveis());
    }

    #[test]
    fn sem_lista_do_motor_usa_a_embutida() {
        let r = classificar(
            &v(&[
                "a.m4a", "b.opus", "c.flac", "d.aac", "e.wma", "f.mkv", "g.mov", "h.avi", "i.webm",
                "j.ogg", "k.TIFF", "l.html",
            ]),
            &Extensoes::default(),
        );
        assert_eq!(r.midia.len(), 10);
        assert_eq!(r.imagens, v(&["k.TIFF"]));
        assert_eq!(r.diretos, v(&["l.html"]));
    }

    #[test]
    fn pdf_vence_a_lista_de_conversao_direta() {
        let r = classificar(&v(&["a.pdf"]), &ext_motor());
        assert_eq!(r.pdfs.len(), 1);
        assert!(r.diretos.is_empty());
    }

    #[test]
    fn grupo_principal_conta_e_desempata() {
        let r = classificar(&v(&["a.pdf", "b.docx", "c.docx", "d.png"]), &ext_motor());
        let (g, arqs, sobra) = r.documentos_do_grupo_principal().expect("grupo");
        assert_eq!(g, GrupoDoc::Direto);
        assert_eq!(arqs.len(), 2);
        assert_eq!(sobra, 2);
        let r = classificar(&v(&["a.png", "b.pdf"]), &ext_motor());
        assert_eq!(
            r.documentos_do_grupo_principal().map(|x| x.0),
            Some(GrupoDoc::Pdf)
        );
        assert!(classificar(&v(&["a.mp3"]), &ext_motor())
            .documentos_do_grupo_principal()
            .is_none());
    }

    #[test]
    fn modo_certo_para_cada_tipo() {
        let m = |k: &str, a: &str| Modo {
            key: k.into(),
            accepts: a.into(),
            ..Default::default()
        };
        let modos = vec![
            m("force", "pdf"),
            m("auto", "pdf"),
            m("direct", "direct"),
            m("images", "images"),
        ];
        assert_eq!(modo_para(GrupoDoc::Pdf, &modos).as_deref(), Some("auto"));
        assert_eq!(
            modo_para(GrupoDoc::Direto, &modos).as_deref(),
            Some("direct")
        );
        assert_eq!(
            modo_para(GrupoDoc::Imagens, &modos).as_deref(),
            Some("images")
        );
        assert_eq!(
            modo_para(GrupoDoc::Pdf, &modos[..1]).as_deref(),
            Some("force")
        );
        assert_eq!(modo_para(GrupoDoc::Imagens, &modos[..2]), None);
    }
}
