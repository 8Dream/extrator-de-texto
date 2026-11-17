//! Informacoes de versao, icone e manifesto embutidos no .exe.
//! A versao vem de ..\VERSION.txt ("Versao X.Y.Z"), a unica fonte da verdade.
use std::fs;

fn main() {
    println!("cargo:rerun-if-changed=../VERSION.txt");
    println!("cargo:rerun-if-changed=extrator.manifest");
    println!("cargo:rerun-if-changed=../src/assets/icone.ico");

    let texto = fs::read_to_string("../VERSION.txt").unwrap_or_default();
    let versao = texto
        .lines()
        .find_map(|l| {
            let l = l.trim();
            l.strip_prefix("Versao")
                .or_else(|| l.strip_prefix("Versão"))
                .map(|r| r.trim().to_string())
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "0.0.0".to_string());
    println!("cargo:rustc-env=EXTRATOR_VERSION={versao}");

    let mut partes: Vec<u64> = versao
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .take(4)
        .filter_map(|s| s.parse().ok())
        .collect();
    while partes.len() < 4 {
        partes.push(0);
    }
    let packed = (partes[0] << 48) | (partes[1] << 32) | (partes[2] << 16) | partes[3];
    let numerica = format!("{}.{}.{}.{}", partes[0], partes[1], partes[2], partes[3]);

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../src/assets/icone.ico");
        res.set_manifest_file("extrator.manifest");
        res.set("ProductName", "Extrator de Texto");
        res.set("CompanyName", "Amancio & Favarato Freire Advogados");
        res.set("FileDescription", "Extrator de Texto");
        res.set("InternalName", "Extrator de Texto");
        res.set("OriginalFilename", "Extrator de Texto.exe");
        res.set("LegalCopyright", "Amancio & Favarato Freire Advogados");
        res.set("ProductVersion", &versao);
        res.set("FileVersion", &numerica);
        res.set_version_info(winresource::VersionInfo::FILEVERSION, packed);
        res.set_version_info(winresource::VersionInfo::PRODUCTVERSION, packed);
        if let Err(e) = res.compile() {
            panic!("falha ao embutir recursos do Windows (rc.exe): {e}");
        }
    }
}
