//! Preferencias de aparencia da interface em `dados\interface.json`.
//!
//! Formato (todos os campos opcionais; arquivo antigo so com `tema` continua valendo):
//! `{"tema": "claro" | "escuro" | "auto", "destaque": "azul" | ... | "personalizada",
//!   "destaque_cor": "#RRGGBB", "texto": "pequeno" | "normal" | "grande"}`.
//! Campos que esta versao nao conhece sao preservados ao gravar.

use crate::cores::{self, Rgb, DESTAQUES};
use serde_json::{Map, Value};
use std::path::Path;

fn arquivo(dados: &Path) -> std::path::PathBuf {
    dados.join("interface.json")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModoTema {
    Claro,
    Escuro,
    /// Segue o tema do Windows.
    Auto,
}

impl ModoTema {
    pub const TODOS: [ModoTema; 3] = [ModoTema::Claro, ModoTema::Escuro, ModoTema::Auto];

    pub fn chave(self) -> &'static str {
        match self {
            ModoTema::Claro => "claro",
            ModoTema::Escuro => "escuro",
            ModoTema::Auto => "auto",
        }
    }

    pub fn rotulo(self) -> &'static str {
        match self {
            ModoTema::Claro => "Claro",
            ModoTema::Escuro => "Escuro",
            ModoTema::Auto => "Automático",
        }
    }

    fn de_chave(s: &str) -> Option<ModoTema> {
        Self::TODOS.into_iter().find(|m| m.chave() == s)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Destaque {
    /// Indice em `cores::DESTAQUES`.
    Pronto(usize),
    Personalizado(Rgb),
}

impl Destaque {
    pub fn rgb(self) -> Rgb {
        match self {
            Destaque::Pronto(i) => DESTAQUES[i.min(DESTAQUES.len() - 1)].2,
            Destaque::Personalizado(c) => c,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TamanhoTexto {
    Pequeno,
    Normal,
    Grande,
}

impl TamanhoTexto {
    pub const TODOS: [TamanhoTexto; 3] = [
        TamanhoTexto::Pequeno,
        TamanhoTexto::Normal,
        TamanhoTexto::Grande,
    ];

    /// Fator de escala da interface.
    pub fn escala(self) -> f32 {
        match self {
            TamanhoTexto::Pequeno => 0.9,
            TamanhoTexto::Normal => 1.0,
            TamanhoTexto::Grande => 1.15,
        }
    }

    pub fn chave(self) -> &'static str {
        match self {
            TamanhoTexto::Pequeno => "pequeno",
            TamanhoTexto::Normal => "normal",
            TamanhoTexto::Grande => "grande",
        }
    }

    pub fn rotulo(self) -> &'static str {
        match self {
            TamanhoTexto::Pequeno => "Pequeno",
            TamanhoTexto::Normal => "Normal",
            TamanhoTexto::Grande => "Grande",
        }
    }

    fn de_chave(s: &str) -> Option<TamanhoTexto> {
        Self::TODOS.into_iter().find(|t| t.chave() == s)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Aparencia {
    pub modo: ModoTema,
    pub destaque: Destaque,
    pub texto: TamanhoTexto,
}

impl Default for Aparencia {
    fn default() -> Self {
        Aparencia {
            // Sem preferencia salva, segue o tema do Windows (como a versao anterior fazia).
            modo: ModoTema::Auto,
            destaque: Destaque::Pronto(0),
            texto: TamanhoTexto::Normal,
        }
    }
}

impl Aparencia {
    /// Modo escuro efetivo; `sistema_escuro` so vale no modo automatico.
    pub fn escuro(&self, sistema_escuro: bool) -> bool {
        match self.modo {
            ModoTema::Claro => false,
            ModoTema::Escuro => true,
            ModoTema::Auto => sistema_escuro,
        }
    }
}

fn ler_mapa(dados: &Path) -> Map<String, Value> {
    std::fs::read_to_string(arquivo(dados))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| match v {
            Value::Object(m) => Some(m),
            _ => None,
        })
        .unwrap_or_default()
}

/// Le a aparencia; campo ausente ou invalido usa o padrao daquele campo.
pub fn ler(dados: &Path) -> Aparencia {
    let m = ler_mapa(dados);
    let texto = |k: &str| m.get(k).and_then(Value::as_str);
    let mut a = Aparencia::default();
    if let Some(modo) = texto("tema").and_then(ModoTema::de_chave) {
        a.modo = modo;
    }
    if let Some(d) = texto("destaque") {
        if d == "personalizada" {
            if let Some(c) = texto("destaque_cor").and_then(cores::analisar_hex) {
                a.destaque = Destaque::Personalizado(c);
            }
        } else if let Some(i) = DESTAQUES.iter().position(|(k, _, _)| *k == d) {
            a.destaque = Destaque::Pronto(i);
        }
    }
    if let Some(t) = texto("texto").and_then(TamanhoTexto::de_chave) {
        a.texto = t;
    }
    a
}

/// Grava a aparencia sem perder campos desconhecidos do arquivo.
pub fn gravar(dados: &Path, a: &Aparencia) {
    let _ = std::fs::create_dir_all(dados);
    let mut m = ler_mapa(dados);
    m.insert("tema".into(), Value::from(a.modo.chave()));
    match a.destaque {
        Destaque::Pronto(i) => {
            m.insert(
                "destaque".into(),
                Value::from(DESTAQUES[i.min(DESTAQUES.len() - 1)].0),
            );
            m.remove("destaque_cor");
        }
        Destaque::Personalizado(c) => {
            m.insert("destaque".into(), Value::from("personalizada"));
            m.insert("destaque_cor".into(), Value::from(cores::hex(c)));
        }
    }
    m.insert("texto".into(), Value::from(a.texto.chave()));
    let _ = std::fs::write(arquivo(dados), Value::Object(m).to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pasta(nome: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("extrator-prefs-{nome}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn sem_arquivo_segue_o_windows_com_o_azul_do_icone() {
        let d = pasta("vazio");
        let a = ler(&d);
        assert_eq!(a, Aparencia::default());
        assert!(a.escuro(true) && !a.escuro(false));
        assert_eq!(a.destaque.rgb(), DESTAQUES[0].2);
        assert_eq!(a.texto.escala(), 1.0);
    }

    #[test]
    fn arquivo_antigo_so_com_tema_continua_valendo() {
        let d = pasta("antigo");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("interface.json"), r#"{"tema":"escuro"}"#).unwrap();
        let a = ler(&d);
        assert_eq!(a.modo, ModoTema::Escuro);
        assert!(a.escuro(false), "o tema salvo e respeitado");
        assert_eq!(a.destaque, Destaque::Pronto(0));
        assert_eq!(a.texto, TamanhoTexto::Normal);
        std::fs::write(d.join("interface.json"), r#"{"tema":"claro"}"#).unwrap();
        assert_eq!(ler(&d).modo, ModoTema::Claro);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn grava_e_le_tudo() {
        let d = pasta("tudo");
        for destaque in [
            Destaque::Pronto(7),
            Destaque::Personalizado([0x12, 0xAB, 0xEF]),
        ] {
            for modo in ModoTema::TODOS {
                for texto in TamanhoTexto::TODOS {
                    let a = Aparencia {
                        modo,
                        destaque,
                        texto,
                    };
                    gravar(&d, &a);
                    assert_eq!(ler(&d), a);
                }
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn campos_desconhecidos_sobrevivem_e_cor_antiga_sai() {
        let d = pasta("extra");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("interface.json"),
            r##"{"tema":"claro","futuro":{"x":1},"destaque":"personalizada","destaque_cor":"#112233"}"##,
        )
        .unwrap();
        assert_eq!(
            ler(&d).destaque,
            Destaque::Personalizado([0x11, 0x22, 0x33])
        );
        let mut a = ler(&d);
        a.destaque = Destaque::Pronto(3);
        gravar(&d, &a);
        let v: Value =
            serde_json::from_str(&std::fs::read_to_string(d.join("interface.json")).unwrap())
                .unwrap();
        assert_eq!(v["futuro"]["x"], 1);
        assert_eq!(v["destaque"], "rosa");
        assert!(v.get("destaque_cor").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn valores_invalidos_caem_no_padrao_de_cada_campo() {
        let d = pasta("ruim");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(
            d.join("interface.json"),
            r#"{"tema":"roxo","destaque":"personalizada","destaque_cor":"zzz","texto":"gigante"}"#,
        )
        .unwrap();
        assert_eq!(ler(&d), Aparencia::default());
        std::fs::write(d.join("interface.json"), "isto nao e json").unwrap();
        assert_eq!(ler(&d), Aparencia::default());
        std::fs::write(d.join("interface.json"), "[1,2]").unwrap();
        assert_eq!(ler(&d), Aparencia::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn escala_do_texto() {
        assert_eq!(TamanhoTexto::Pequeno.escala(), 0.9);
        assert_eq!(TamanhoTexto::Normal.escala(), 1.0);
        assert_eq!(TamanhoTexto::Grande.escala(), 1.15);
    }

    #[test]
    fn modo_automatico_segue_o_sistema() {
        let mut a = Aparencia {
            modo: ModoTema::Auto,
            ..Default::default()
        };
        assert!(a.escuro(true) && !a.escuro(false));
        a.modo = ModoTema::Claro;
        assert!(!a.escuro(true));
        a.modo = ModoTema::Escuro;
        assert!(a.escuro(false));
    }
}
