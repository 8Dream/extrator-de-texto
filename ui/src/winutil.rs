//! Pequenos recursos do Windows: instancia unica, trazer a janela existente a frente, icone da janela.

use eframe::egui::IconData;
use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, LPARAM, TRUE,
};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow,
    SW_RESTORE,
};

/// Mantem o mutex nomeado vivo enquanto o programa roda.
pub struct Instancia(HANDLE);

impl Drop for Instancia {
    fn drop(&mut self) {
        // SAFETY: o handle foi criado por CreateMutexW e so e fechado aqui.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn largo(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `Some` se esta e a primeira instancia; `None` se outra ja esta aberta.
pub fn instancia_unica(nome: &str) -> Option<Instancia> {
    let n = largo(nome);
    // SAFETY: `n` termina em zero e vive durante a chamada.
    unsafe {
        let h = CreateMutexW(std::ptr::null(), 1, n.as_ptr());
        if h.is_null() {
            // Sem mutex nao ha como saber: deixa abrir.
            return Some(Instancia(std::ptr::null_mut()));
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(h);
            return None;
        }
        Some(Instancia(h))
    }
}

struct Busca {
    prefixo: Vec<u16>,
    achada: HANDLE,
}

unsafe extern "system" fn ao_enumerar(h: HANDLE, l: LPARAM) -> i32 {
    // SAFETY: `l` aponta para o `Busca` vivo na pilha de `focar_janela`.
    let busca = &mut *(l as *mut Busca);
    if IsWindowVisible(h) == 0 {
        return TRUE;
    }
    let mut buf = [0u16; 256];
    let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
    if n > 0 && buf[..n as usize].starts_with(&busca.prefixo) {
        busca.achada = h;
        return 0;
    }
    TRUE
}

/// Traz ao primeiro plano a janela cujo titulo comeca com `prefixo`.
pub fn focar_janela(prefixo: &str) {
    let mut b = Busca {
        prefixo: prefixo.encode_utf16().collect(),
        achada: std::ptr::null_mut(),
    };
    // SAFETY: o ponteiro para `b` so e usado durante EnumWindows, que e sincrono.
    unsafe {
        EnumWindows(Some(ao_enumerar), &mut b as *mut Busca as LPARAM);
        if !b.achada.is_null() {
            if IsIconic(b.achada) != 0 {
                ShowWindow(b.achada, SW_RESTORE);
            }
            SetForegroundWindow(b.achada);
        }
    }
}

/// Tema de aplicativos do Windows (registro, sem janela): `Some(true)` = escuro.
/// `None` se o valor nao existir (Windows antigo) ou nao puder ser lido.
pub fn tema_escuro_do_windows() -> Option<bool> {
    let chave = largo(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let valor = largo("AppsUseLightTheme");
    let mut dado: u32 = 1;
    let mut tamanho = std::mem::size_of::<u32>() as u32;
    // SAFETY: nomes terminam em zero; `dado` e `tamanho` sao locais validos durante a chamada.
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            chave.as_ptr(),
            valor.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut dado as *mut u32 as *mut core::ffi::c_void,
            &mut tamanho,
        )
    };
    (r == 0).then_some(dado == 0)
}

/// Decodifica o PNG do icone (src\assets\icone.png) para RGBA.
fn decodificar_icone() -> Option<(Vec<u8>, u32, u32)> {
    let dados: &[u8] = include_bytes!("../../src/assets/icone.png");
    let dec = png::Decoder::new(std::io::Cursor::new(dados));
    let mut leitor = dec.read_info().ok()?;
    let mut buf = vec![0u8; leitor.output_buffer_size()?];
    let info = leitor.next_frame(&mut buf).ok()?;
    let buf = &buf[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf.to_vec(),
        png::ColorType::Rgb => buf
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        _ => return None,
    };
    Some((rgba, info.width, info.height))
}

/// Icone da janela e da barra de tarefas (256 px, reduzido do PNG).
pub fn icone_janela() -> Option<IconData> {
    let (rgba, w, h) = decodificar_icone()?;
    let (rgba, w, h) = reduzir(&rgba, w, h, 256);
    Some(IconData {
        rgba,
        width: w,
        height: h,
    })
}

/// Logotipo do menu lateral, reduzido para `lado` px.
pub fn logotipo(lado: u32) -> Option<(Vec<u8>, u32, u32)> {
    let (rgba, w, h) = decodificar_icone()?;
    Some(reduzir(&rgba, w, h, lado))
}

/// Reducao por media de caixa (alpha pre-multiplicado), boa para icones.
pub fn reduzir(rgba: &[u8], w: u32, h: u32, lado: u32) -> (Vec<u8>, u32, u32) {
    if w <= lado || h <= lado {
        return (rgba.to_vec(), w, h);
    }
    let mut saida = vec![0u8; (lado * lado * 4) as usize];
    for oy in 0..lado {
        let (y0, y1) = (oy * h / lado, ((oy + 1) * h / lado).max(oy * h / lado + 1));
        for ox in 0..lado {
            let (x0, x1) = (ox * w / lado, ((ox + 1) * w / lado).max(ox * w / lado + 1));
            let (mut r, mut g, mut b, mut a, mut n) = (0u64, 0u64, 0u64, 0u64, 0u64);
            for y in y0..y1.min(h) {
                for x in x0..x1.min(w) {
                    let p = ((y * w + x) * 4) as usize;
                    let al = rgba[p + 3] as u64;
                    r += rgba[p] as u64 * al;
                    g += rgba[p + 1] as u64 * al;
                    b += rgba[p + 2] as u64 * al;
                    a += al;
                    n += 1;
                }
            }
            let o = ((oy * lado + ox) * 4) as usize;
            if a > 0 && n > 0 {
                saida[o] = (r / a) as u8;
                saida[o + 1] = (g / a) as u8;
                saida[o + 2] = (b / a) as u8;
                saida[o + 3] = (a / n) as u8;
            }
        }
    }
    (saida, lado, lado)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icone_decodifica_e_reduz() {
        let (rgba, w, h) = logotipo(64).expect("png embutido");
        assert_eq!((w, h), (64, 64));
        assert_eq!(rgba.len(), 64 * 64 * 4);
        assert_eq!(rgba[3], 0, "canto transparente");
        assert!(icone_janela().is_some());
    }

    #[test]
    fn tema_do_windows_le_sem_falhar() {
        // Pode ser Some(true/false) ou None; o importante e nao travar nem entrar em panico.
        let _ = tema_escuro_do_windows();
    }

    #[test]
    fn reduzir_nao_amplia() {
        let (_, w, _) = reduzir(&[255; 16], 2, 2, 8);
        assert_eq!(w, 2);
    }
}
