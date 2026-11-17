//! Formatacao de tempos, tamanhos e plurais (puro, testavel, sem interface).

/// "00:01:05"; `--:--:--` quando o valor nao existe.
pub fn hms(s: Option<f64>) -> String {
    match s {
        Some(v) if v.is_finite() => {
            let t = v.max(0.0).round() as u64;
            format!("{:02}:{:02}:{:02}", t / 3600, (t / 60) % 60, t % 60)
        }
        _ => "--:--:--".to_string(),
    }
}

/// "1 h 02 min 03 s", "4 min 05 s" ou "7 s"; "—" quando nao existe.
pub fn dur(s: Option<f64>) -> String {
    match s {
        Some(v) if v.is_finite() => {
            let t = v.max(0.0).round() as u64;
            let (h, m, sec) = (t / 3600, (t / 60) % 60, t % 60);
            if h > 0 {
                format!("{h} h {m:02} min {sec:02} s")
            } else if m > 0 {
                format!("{m} min {sec:02} s")
            } else {
                format!("{sec} s")
            }
        }
        _ => "—".to_string(),
    }
}

/// Estimativa de tempo restante em palavras: "cerca de 2 min", "menos de 1 min", "estimando…".
pub fn eta_texto(eta_s: Option<f64>) -> String {
    match eta_s {
        Some(v) if v.is_finite() => {
            let t = v.max(0.0);
            if t < 45.0 {
                "menos de 1 min".to_string()
            } else if t < 3600.0 {
                format!("cerca de {} min", (t / 60.0).round().max(1.0) as u64)
            } else {
                let min = (t / 60.0).round() as u64;
                format!("cerca de {} h {:02} min", min / 60, min % 60)
            }
        }
        _ => "estimando…".to_string(),
    }
}

/// "agora há pouco", "há 5 min", "há 2 h", "ontem", "há 4 dias" ou a data (dd/mm/aaaa).
pub fn ha_tempo(instante: f64, agora: f64) -> String {
    let d = agora - instante;
    if !d.is_finite() || d < 0.0 || d < 45.0 {
        return "agora há pouco".to_string();
    }
    if d < 3600.0 {
        return format!("há {} min", (d / 60.0).round().max(1.0) as u64);
    }
    if d < 86_400.0 {
        return format!("há {} h", (d / 3600.0).round() as u64);
    }
    if d < 172_800.0 {
        return "ontem".to_string();
    }
    if d < 30.0 * 86_400.0 {
        return format!("há {} dias", (d / 86_400.0).floor() as u64);
    }
    data_local(instante).chars().take(10).collect()
}

pub fn bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1_048_576 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / 1_048_576.0)
    }
}

pub fn plural(n: usize, um: &str, varios: &str) -> String {
    format!("{} {}", n, if n == 1 { um } else { varios })
}

/// Nome do arquivo (ultimo trecho depois de `\` ou `/`).
pub fn base_name(p: &str) -> &str {
    p.rsplit(['\\', '/']).next().unwrap_or(p)
}

/// Pasta que contem o arquivo.
pub fn dir_name(p: &str) -> &str {
    match p.rfind(['\\', '/']) {
        Some(i) => &p[..i],
        None => "",
    }
}

/// Extensao em minusculas, com ponto (".pdf"); vazio se nao houver.
pub fn ext_of(p: &str) -> String {
    let n = base_name(p);
    match n.rfind('.') {
        Some(i) => n[i..].to_lowercase(),
        None => String::new(),
    }
}

/// Fracao 0..=1 a partir de um percentual 0..=100.
pub fn frac(pct: f64) -> f32 {
    (pct / 100.0).clamp(0.0, 1.0) as f32
}

/// Segundos desde 1970 (UTC).
pub fn agora_unix() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Data e hora locais "dd/mm/aaaa HH:MM:SS" a partir de segundos Unix.
pub fn data_local(unix: f64) -> String {
    match local_parts(unix) {
        Some((a, mo, d, h, mi, s)) => format!("{d:02}/{mo:02}/{a:04} {h:02}:{mi:02}:{s:02}"),
        None => "—".to_string(),
    }
}

/// Hora local "HH:MM:SS".
pub fn hora_local(unix: f64) -> String {
    match local_parts(unix) {
        Some((_, _, _, h, mi, s)) => format!("{h:02}:{mi:02}:{s:02}"),
        None => "--:--:--".to_string(),
    }
}

#[cfg(windows)]
fn local_parts(unix: f64) -> Option<(u16, u16, u16, u16, u16, u16)> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    if !unix.is_finite() || unix < 0.0 {
        return None;
    }
    let ticks = ((unix + 11_644_473_600.0) * 10_000_000.0) as u64;
    let ft = FILETIME {
        dwLowDateTime: (ticks & 0xFFFF_FFFF) as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    // SAFETY: estruturas locais validas, ponteiros para a pilha desta funcao.
    unsafe {
        let mut utc: SYSTEMTIME = std::mem::zeroed();
        let mut loc: SYSTEMTIME = std::mem::zeroed();
        if FileTimeToSystemTime(&ft, &mut utc) == 0 {
            return None;
        }
        if SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut loc) == 0 {
            return None;
        }
        Some((
            loc.wYear,
            loc.wMonth,
            loc.wDay,
            loc.wHour,
            loc.wMinute,
            loc.wSecond,
        ))
    }
}

#[cfg(not(windows))]
fn local_parts(unix: f64) -> Option<(u16, u16, u16, u16, u16, u16)> {
    // Sem fuso: UTC (so para testes fora do Windows).
    let t = unix as i64;
    let s = t.rem_euclid(86_400);
    Some((
        1970,
        1,
        1,
        (s / 3600) as u16,
        ((s / 60) % 60) as u16,
        (s % 60) as u16,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hms_formata_e_trata_ausente() {
        assert_eq!(hms(Some(65.4)), "00:01:05");
        assert_eq!(hms(Some(3725.0)), "01:02:05");
        assert_eq!(hms(Some(-3.0)), "00:00:00");
        assert_eq!(hms(None), "--:--:--");
        assert_eq!(hms(Some(f64::NAN)), "--:--:--");
    }

    #[test]
    fn dur_escolhe_a_unidade() {
        assert_eq!(dur(Some(7.0)), "7 s");
        assert_eq!(dur(Some(245.0)), "4 min 05 s");
        assert_eq!(dur(Some(3723.0)), "1 h 02 min 03 s");
        assert_eq!(dur(None), "—");
    }

    #[test]
    fn eta_em_palavras() {
        assert_eq!(eta_texto(None), "estimando…");
        assert_eq!(eta_texto(Some(20.0)), "menos de 1 min");
        assert_eq!(eta_texto(Some(120.0)), "cerca de 2 min");
        assert_eq!(eta_texto(Some(5400.0)), "cerca de 1 h 30 min");
    }

    #[test]
    fn tempo_relativo() {
        let agora = 1_800_000_000.0;
        assert_eq!(ha_tempo(agora - 10.0, agora), "agora há pouco");
        assert_eq!(ha_tempo(agora + 50.0, agora), "agora há pouco");
        assert_eq!(ha_tempo(agora - 300.0, agora), "há 5 min");
        assert_eq!(ha_tempo(agora - 7200.0, agora), "há 2 h");
        assert_eq!(ha_tempo(agora - 100_000.0, agora), "ontem");
        assert_eq!(ha_tempo(agora - 4.5 * 86_400.0, agora), "há 4 dias");
        assert_eq!(ha_tempo(agora - 60.0 * 86_400.0, agora).len(), 10);
    }

    #[test]
    fn tamanhos_e_plurais() {
        assert_eq!(bytes(900), "900 B");
        assert_eq!(bytes(2048), "2.0 KB");
        assert_eq!(bytes(3 * 1_048_576), "3.0 MB");
        assert_eq!(plural(1, "arquivo", "arquivos"), "1 arquivo");
        assert_eq!(plural(0, "arquivo", "arquivos"), "0 arquivos");
    }

    #[test]
    fn caminhos() {
        assert_eq!(base_name(r"C:\Pasta\a b.PDF"), "a b.PDF");
        assert_eq!(dir_name(r"C:\Pasta\a.pdf"), r"C:\Pasta");
        assert_eq!(ext_of(r"C:\x\Foto.JPG"), ".jpg");
        assert_eq!(ext_of("semext"), "");
        assert_eq!(dir_name("a.pdf"), "");
    }

    #[test]
    fn fracao_limitada() {
        assert_eq!(frac(50.0), 0.5);
        assert_eq!(frac(150.0), 1.0);
        assert_eq!(frac(-5.0), 0.0);
    }
}
