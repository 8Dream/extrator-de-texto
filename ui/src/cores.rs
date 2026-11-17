//! Cores: leitura de `#RRGGBB`, contraste (WCAG) e derivacao das cores de destaque.
//!
//! A cor de destaque escolhida pelo usuario vira um conjunto de cores (botao, selecao, links,
//! fundos suaves, hover/pressionado) que respeita contraste minimo de 4,5:1 nos dois modos.

pub type Rgb = [u8; 3];

/// Contraste minimo (WCAG AA para texto normal).
pub const ALVO: f64 = 4.5;
pub const BRANCO: Rgb = [255, 255, 255];
/// Texto escuro usado sobre destaques claros (modo escuro).
pub const TINTA: Rgb = [11, 16, 32];

/// Destaques prontos: (chave gravada, nome, cor base). O primeiro e o azul do icone.
pub const DESTAQUES: [(&str, &str, Rgb); 10] = [
    ("azul", "Azul", [0x0F, 0x63, 0xCF]),
    ("indigo", "Índigo", [0x4F, 0x46, 0xE5]),
    ("roxo", "Roxo", [0x7C, 0x3A, 0xED]),
    ("rosa", "Rosa", [0xDB, 0x27, 0x77]),
    ("vermelho", "Vermelho", [0xDC, 0x26, 0x26]),
    ("laranja", "Laranja", [0xEA, 0x58, 0x0C]),
    ("ambar", "Âmbar", [0xF5, 0x9E, 0x0B]),
    ("verde", "Verde", [0x16, 0xA3, 0x4A]),
    ("verde-agua", "Verde-água", [0x0D, 0x94, 0x88]),
    ("grafite", "Grafite", [0x47, 0x55, 0x69]),
];

/// Resultado da validacao do campo de cor digitado.
pub fn validar_hex(s: &str) -> Result<Rgb, &'static str> {
    let t = s.trim().trim_start_matches('#');
    if t.is_empty() {
        return Err("Digite a cor no formato #RRGGBB.");
    }
    if !t.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Use só os dígitos 0 a 9 e as letras A a F.");
    }
    let t: String = match t.len() {
        3 => t.chars().flat_map(|c| [c, c]).collect(),
        6 => t.to_string(),
        n if n < 6 => return Err("Faltam dígitos: a cor tem 6 caracteres, como #0F63CF."),
        _ => return Err("Há dígitos demais: a cor tem 6 caracteres, como #0F63CF."),
    };
    let v = u32::from_str_radix(&t, 16).map_err(|_| "Cor inválida.")?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn analisar_hex(s: &str) -> Option<Rgb> {
    validar_hex(s).ok()
}

/// `#RRGGBB` em maiusculas.
pub fn hex(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

/// Normaliza qualquer entrada valida (`abc`, `#aabbcc`, ` AABBCC `) para `#RRGGBB`.
pub fn normalizar_hex(s: &str) -> Option<String> {
    analisar_hex(s).map(hex)
}

fn linear(c: u8) -> f64 {
    let s = f64::from(c) / 255.0;
    if s <= 0.039_28 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Luminancia relativa (WCAG).
pub fn luminancia(c: Rgb) -> f64 {
    0.2126 * linear(c[0]) + 0.7152 * linear(c[1]) + 0.0722 * linear(c[2])
}

/// Razao de contraste (1 a 21).
pub fn contraste(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminancia(a), luminancia(b));
    let (alto, baixo) = if la > lb { (la, lb) } else { (lb, la) };
    (alto + 0.05) / (baixo + 0.05)
}

/// Mistura `b` sobre `a` na proporcao `t` (0 = a, 1 = b).
pub fn mistura(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let m = |x: u8, y: u8| (f64::from(x) * (1.0 - t) + f64::from(y) * t).round() as u8;
    [m(a[0], b[0]), m(a[1], b[1]), m(a[2], b[2])]
}

fn para_hsl(c: Rgb) -> (f64, f64, f64) {
    let (r, g, b) = (
        f64::from(c[0]) / 255.0,
        f64::from(c[1]) / 255.0,
        f64::from(c[2]) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    if d == 0.0 {
        return (0.0, 0.0, l);
    }
    let s = d / (1.0 - (2.0 * l - 1.0).abs());
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s.clamp(0.0, 1.0), l)
}

fn de_hsl(h: f64, s: f64, l: f64) -> Rgb {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r, g, b) = match (h.rem_euclid(360.0) / 60.0) as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let v = |u: f64| ((u + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [v(r), v(g), v(b)]
}

/// Branco ou tinta escura, o que der mais contraste sobre `fundo`.
pub fn texto_sobre(fundo: Rgb) -> Rgb {
    if contraste(BRANCO, fundo) >= contraste(TINTA, fundo) {
        BRANCO
    } else {
        TINTA
    }
}

/// Conjunto de cores derivado de um destaque para um modo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Derivado {
    /// Cor principal (botao, links, selecao): >= 4,5:1 sobre superficie, fundo e fundo suave.
    pub destaque: Rgb,
    /// Fundo suave (selecao do menu, cartoes escolhidos).
    pub suave: Rgb,
    /// Texto sobre o destaque (branco ou tinta escura).
    pub sobre: Rgb,
    pub hover: Rgb,
    pub pressionado: Rgb,
    /// O tom escolhido foi ajustado para manter a leitura.
    pub ajustada: bool,
}

fn proporcao_suave(escuro: bool) -> f64 {
    if escuro {
        0.22
    } else {
        0.12
    }
}

fn passa(c: Rgb, escuro: bool, superficie: Rgb, fundo: Rgb) -> bool {
    let suave = mistura(superficie, c, proporcao_suave(escuro));
    contraste(c, superficie) >= ALVO
        && contraste(c, fundo) >= ALVO
        && contraste(c, suave) >= ALVO
        && contraste(c, texto_sobre(c)) >= ALVO
}

/// Deriva o conjunto de cores. Se `base` nao tiver contraste suficiente no modo, escurece (modo
/// claro) ou clareia (modo escuro) em passos pequenos ate passar, mantendo o matiz.
pub fn derivar(base: Rgb, escuro: bool, superficie: Rgb, fundo: Rgb) -> Derivado {
    let (h, s, l0) = para_hsl(base);
    let mut cor = base;
    let mut l = l0;
    let mut ajustada = false;
    while !passa(cor, escuro, superficie, fundo) {
        ajustada = true;
        l = if escuro {
            (l + 0.01).min(1.0)
        } else {
            (l - 0.01).max(0.0)
        };
        cor = de_hsl(h, s, l);
        if (escuro && l >= 1.0) || (!escuro && l <= 0.0) {
            // Limite do passeio: branco/preto sempre passam; sai do laco com o extremo.
            break;
        }
    }
    let passo = |d: f64| de_hsl(h, s, (if escuro { l + d } else { l - d }).clamp(0.0, 1.0));
    Derivado {
        destaque: cor,
        suave: mistura(superficie, cor, proporcao_suave(escuro)),
        sobre: texto_sobre(cor),
        hover: passo(0.06),
        pressionado: passo(0.11),
        ajustada,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUP_CLARO: Rgb = [0xff, 0xff, 0xff];
    const BG_CLARO: Rgb = [0xf4, 0xf5, 0xf7];
    const SUP_ESCURO: Rgb = [0x17, 0x1b, 0x21];
    const BG_ESCURO: Rgb = [0x0f, 0x12, 0x16];

    fn confere(base: Rgb, escuro: bool) {
        let (sup, bg) = if escuro {
            (SUP_ESCURO, BG_ESCURO)
        } else {
            (SUP_CLARO, BG_CLARO)
        };
        let d = derivar(base, escuro, sup, bg);
        assert!(
            contraste(d.destaque, sup) >= ALVO,
            "{} escuro={escuro}: destaque x superficie {:.2}",
            hex(base),
            contraste(d.destaque, sup)
        );
        assert!(contraste(d.destaque, bg) >= ALVO, "{} x fundo", hex(base));
        assert!(
            contraste(d.destaque, d.suave) >= ALVO,
            "{} x suave {:.2}",
            hex(base),
            contraste(d.destaque, d.suave)
        );
        assert!(
            contraste(d.sobre, d.destaque) >= ALVO,
            "{} texto sobre destaque {:.2}",
            hex(base),
            contraste(d.sobre, d.destaque)
        );
        assert!(
            contraste(d.sobre, d.hover) >= ALVO,
            "{} texto sobre hover {:.2}",
            hex(base),
            contraste(d.sobre, d.hover)
        );
        assert!(
            contraste(d.sobre, d.pressionado) >= ALVO,
            "{} texto sobre pressionado {:.2}",
            hex(base),
            contraste(d.sobre, d.pressionado)
        );
    }

    #[test]
    fn os_dez_destaques_passam_nos_dois_modos() {
        for (_, _, c) in DESTAQUES {
            confere(c, false);
            confere(c, true);
        }
    }

    #[test]
    fn cores_aleatorias_passam_nos_dois_modos() {
        // Gerador congruencial simples: deterministico, sem dependencia.
        let mut x: u64 = 0x2545_F491_4F6C_DD1D;
        for _ in 0..3000 {
            x = x
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let c = [(x >> 40) as u8, (x >> 32) as u8, (x >> 24) as u8];
            confere(c, false);
            confere(c, true);
        }
    }

    #[test]
    fn extremos_passam() {
        for c in [[0, 0, 0], [255, 255, 255], [255, 255, 0], [128, 128, 128]] {
            confere(c, false);
            confere(c, true);
        }
    }

    #[test]
    fn o_azul_do_icone_nao_e_ajustado_no_claro() {
        let d = derivar(DESTAQUES[0].2, false, SUP_CLARO, BG_CLARO);
        assert!(!d.ajustada, "o azul padrao ja passa no modo claro");
        assert_eq!(d.sobre, BRANCO);
    }

    #[test]
    fn cor_clara_demais_e_ajustada_e_avisa() {
        let d = derivar([0xff, 0xf2, 0x00], false, SUP_CLARO, BG_CLARO);
        assert!(d.ajustada);
        assert!(luminancia(d.destaque) < luminancia([0xff, 0xf2, 0x00]));
    }

    #[test]
    fn texto_sobre_escolhe_o_melhor_contraste() {
        assert_eq!(texto_sobre([0x10, 0x30, 0x90]), BRANCO);
        assert_eq!(texto_sobre([0xf5, 0xc8, 0x42]), TINTA);
    }

    #[test]
    fn contraste_de_referencia() {
        assert!((contraste(BRANCO, [0, 0, 0]) - 21.0).abs() < 0.01);
        assert!((contraste(BRANCO, BRANCO) - 1.0).abs() < 1e-9);
        // Cinza #767676 sobre branco: o limite classico de 4,54:1.
        assert!((contraste([0x76, 0x76, 0x76], BRANCO) - 4.54).abs() < 0.02);
    }

    #[test]
    fn hex_valida_e_normaliza() {
        assert_eq!(validar_hex("#1068d8"), Ok([0x10, 0x68, 0xD8]));
        assert_eq!(validar_hex("  1068D8 "), Ok([0x10, 0x68, 0xD8]));
        assert_eq!(validar_hex("#abc"), Ok([0xAA, 0xBB, 0xCC]));
        assert_eq!(normalizar_hex("abc").as_deref(), Some("#AABBCC"));
        assert!(validar_hex("").is_err());
        assert!(validar_hex("#").is_err());
        assert!(validar_hex("#12").unwrap_err().contains("Faltam"));
        assert!(validar_hex("#1234567").unwrap_err().contains("demais"));
        assert!(validar_hex("#12345G").unwrap_err().contains("0 a 9"));
        assert!(validar_hex("ééé").is_err());
        assert_eq!(hex([1, 2, 255]), "#0102FF");
    }

    #[test]
    fn hsl_ida_e_volta() {
        for c in [[16, 104, 216], [220, 38, 38], [71, 85, 105], [0, 0, 0]] {
            let (h, s, l) = para_hsl(c);
            let v = de_hsl(h, s, l);
            for i in 0..3 {
                assert!(
                    (i32::from(v[i]) - i32::from(c[i])).abs() <= 1,
                    "{c:?} {v:?}"
                );
            }
        }
    }
}
