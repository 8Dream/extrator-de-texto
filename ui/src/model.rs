//! Formatos das respostas e eventos do motor (tolerantes: campo ausente vira padrao).

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Modo {
    pub key: String,
    pub label: String,
    pub description: String,
    pub accepts: String,
    pub pages: bool,
    pub format: bool,
    pub combine: bool,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Extensoes {
    pub pdf: Vec<String>,
    pub direct: Vec<String>,
    pub images: Vec<String>,
    pub media: Vec<String>,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct InfoModelo {
    pub tamanho_mb: Option<f64>,
    pub precisao: String,
    pub velocidade: String,
    pub descricao: String,
    pub instalado: bool,
}

#[derive(Default, Clone, Debug)]
pub struct EstadoTranscricao {
    pub modelo_padrao: String,
    pub instalados: Vec<String>,
    /// Na ordem em que o motor enviou (tiny, base, small...).
    pub modelos: Vec<(String, InfoModelo)>,
    pub idiomas: Vec<String>,
    pub formatos: Vec<String>,
}

impl EstadoTranscricao {
    pub fn de_json(v: &Value) -> Self {
        let lista = |k: &str| -> Vec<String> {
            v.get(k)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        let modelos = v
            .get("models")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(k, i)| {
                        (
                            k.clone(),
                            serde_json::from_value(i.clone()).unwrap_or_default(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        EstadoTranscricao {
            modelo_padrao: v
                .get("default_model")
                .and_then(Value::as_str)
                .unwrap_or("small")
                .to_string(),
            instalados: lista("installed"),
            modelos,
            idiomas: lista("languages"),
            formatos: lista("formats"),
        }
    }

    pub fn info(&self, nome: &str) -> Option<&InfoModelo> {
        self.modelos.iter().find(|(n, _)| n == nome).map(|(_, i)| i)
    }

    /// Modelos que podem ser escolhidos na fila (os instalados; se nenhum, o padrao).
    pub fn selecionaveis(&self) -> Vec<String> {
        if self.instalados.is_empty() {
            vec![self.modelo_padrao.clone()]
        } else {
            self.instalados.clone()
        }
    }

    pub fn nao_instalados(&self) -> Vec<&(String, InfoModelo)> {
        self.modelos
            .iter()
            .filter(|(n, _)| !self.instalados.contains(n))
            .collect()
    }
}

/// Configuracoes salvas pelo programa. O que a interface nao conhece fica em `extra` e volta intacto.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub language: String,
    pub dpi: i64,
    pub psm: i64,
    pub native_text_threshold: i64,
    pub cpu_mode: String,
    pub pdf_output_type: String,
    pub verbosity: String,
    pub rotate_pages: bool,
    pub deskew: bool,
    pub resume: bool,
    pub auto_open_output: bool,
    pub motor: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            language: "por+eng".into(),
            dpi: 220,
            psm: 3,
            native_text_threshold: 80,
            cpu_mode: "normal".into(),
            pdf_output_type: "pdf".into(),
            verbosity: "detalhado".into(),
            rotate_pages: true,
            deskew: true,
            resume: true,
            auto_open_output: false,
            motor: "local".into(),
            extra: Map::new(),
        }
    }
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Ocupado {
    pub ocr: bool,
    pub transcricao: bool,
}

#[derive(Default, Clone, Debug)]
pub struct Boot {
    pub modos: Vec<Modo>,
    pub modos_lote: Vec<Modo>,
    pub config: Config,
    pub ext: Extensoes,
    pub data_dir: String,
    pub log_file: String,
    pub transcricao: EstadoTranscricao,
    pub ocupado: Ocupado,
}

impl Boot {
    pub fn de_json(v: &Value) -> Self {
        let get = |k: &str| v.get(k).cloned().unwrap_or(Value::Null);
        let texto = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Boot {
            modos: serde_json::from_value(get("convert_modes")).unwrap_or_default(),
            modos_lote: serde_json::from_value(get("batch_modes")).unwrap_or_default(),
            config: serde_json::from_value(get("config")).unwrap_or_default(),
            ext: serde_json::from_value(get("extensions")).unwrap_or_default(),
            data_dir: texto("data_dir"),
            log_file: texto("log_file"),
            transcricao: EstadoTranscricao::de_json(&get("transcricao")),
            ocupado: serde_json::from_value(get("busy")).unwrap_or_default(),
        }
    }
}

/// Recursos que o motor declara (`capacidades`). Tudo falso ate o motor dizer o contrario.
#[derive(Deserialize, Default, Clone, Copy, Debug, PartialEq)]
#[serde(default)]
pub struct Capacidades {
    pub interlocutores: bool,
    pub motor_spark: bool,
    pub nativo: bool,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct Execucao {
    pub id: String,
    pub ferramenta: String,
    pub ferramenta_nome: String,
    pub rotulo: String,
    pub entrada: String,
    pub variante: String,
    pub estado: String,
    pub estado_texto: String,
    pub etapa: String,
    pub determinada: bool,
    pub progresso: Option<f64>,
    pub progresso_texto: String,
    pub decorrido_s: Option<f64>,
    pub criada_em: Option<f64>,
    pub inicio_em: Option<f64>,
    pub fim_em: Option<f64>,
    pub mensagem: String,
    pub erro: String,
    pub cancelavel: bool,
    pub cancelando: bool,
    pub repetivel: bool,
    pub log_arquivo: String,
    pub log_ini: Option<i64>,
    pub log_fim: Option<i64>,
    pub eta_s: Option<f64>,
    pub eta_min_s: Option<f64>,
    pub eta_max_s: Option<f64>,
    pub eta_fonte: String,
    pub eta_texto: String,
    /// Futuro: "Spark" para execucoes remotas.
    pub origem: String,
    /// Pasta ou arquivo de saida, quando o motor informar (hoje nao informa).
    pub saida: String,
}

impl Execucao {
    pub fn ativa(&self) -> bool {
        self.estado == "rodando" || self.estado == "fila"
    }

    /// Texto da etapa/resultado mostrado sob o titulo do cartao.
    pub fn linha_etapa(&self) -> String {
        match self.estado.as_str() {
            "fila" => "Aguardando a vez na fila.".to_string(),
            "rodando" => {
                if self.etapa.is_empty() {
                    "Em andamento…".to_string()
                } else {
                    self.etapa.clone()
                }
            }
            _ => [&self.erro, &self.mensagem, &self.etapa]
                .iter()
                .find(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or_default(),
        }
    }

    /// Estimativa de tempo restante a exibir (so rodando; texto do motor ou calculado aqui).
    pub fn eta_exibido(&self) -> Option<String> {
        if self.estado != "rodando" {
            return None;
        }
        if !self.eta_texto.is_empty() {
            return Some(self.eta_texto.clone());
        }
        self.eta_s.map(|s| crate::fmt::eta_texto(Some(s)))
    }

    pub fn passa(&self, estado: &str, ferramenta: &str, busca: &str) -> bool {
        let ok_estado = match estado {
            "todas" => true,
            "ativas" => self.ativa(),
            e => self.estado == e,
        };
        if !ok_estado || (!ferramenta.is_empty() && self.ferramenta != ferramenta) {
            return false;
        }
        if busca.is_empty() {
            return true;
        }
        [
            &self.rotulo,
            &self.entrada,
            &self.etapa,
            &self.ferramenta_nome,
            &self.mensagem,
            &self.erro,
        ]
        .iter()
        .any(|s| s.to_lowercase().contains(busca))
    }
}

#[derive(Default, Clone, Debug)]
pub struct ListaExecucoes {
    pub rodando: usize,
    pub na_fila: usize,
    pub total: usize,
    pub ferramentas: Vec<(String, String)>,
    pub execucoes: Vec<Execucao>,
}

impl ListaExecucoes {
    pub fn de_json(v: &Value) -> Self {
        let n = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0) as usize;
        ListaExecucoes {
            rodando: n("rodando"),
            na_fila: n("na_fila"),
            total: n("total"),
            ferramentas: v
                .get("ferramentas")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .map(|(k, x)| (k.clone(), x.as_str().unwrap_or("").to_string()))
                        .collect()
                })
                .unwrap_or_default(),
            execucoes: v
                .get("execucoes")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|e| serde_json::from_value(e.clone()).ok())
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct LinhaExec {
    pub t: String,
    pub n: String,
    pub x: String,
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct LinhaLog {
    pub o: i64,
    pub t: String,
    pub nv: String,
    pub x: String,
}

impl LinhaLog {
    pub fn peso(&self) -> u8 {
        match self.nv.as_str() {
            "DEBUG" => 10,
            "WARNING" => 30,
            "ERROR" => 40,
            "CRITICAL" => 50,
            _ => 20,
        }
    }
}

/// Filtro de nivel da tela Log: "tudo", "avisos", "erros".
pub fn peso_minimo(nivel: &str) -> u8 {
    match nivel {
        "avisos" => 30,
        "erros" => 40,
        _ => 0,
    }
}

pub fn rotulo_nivel(nv: &str) -> &str {
    match nv {
        "ERROR" => "ERRO",
        "WARNING" => "AVISO",
        "CRITICAL" => "CRÍTICO",
        "DEBUG" => "DEPURAÇÃO",
        o => o,
    }
}

#[derive(Deserialize, Default, Clone, Debug)]
#[serde(default)]
pub struct ArquivoLog {
    pub nome: String,
    pub tamanho: u64,
    pub data: String,
    pub hora: String,
    pub atual: bool,
    pub tipo: String,
}

#[derive(Default, Clone, Debug)]
pub struct LeituraLog {
    pub nome: String,
    pub tamanho: u64,
    pub ini: i64,
    pub fim: i64,
    pub ao_vivo: bool,
    pub mais: bool,
    pub reiniciado: bool,
    pub linhas: Vec<LinhaLog>,
    pub total_achadas: usize,
    pub cortada: bool,
}

impl LeituraLog {
    pub fn de_json(v: &Value) -> Self {
        let i = |k: &str| v.get(k).and_then(Value::as_i64).unwrap_or(0);
        let b = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
        LeituraLog {
            nome: v
                .get("nome")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            tamanho: i("tamanho").max(0) as u64,
            ini: i("ini"),
            fim: i("fim"),
            ao_vivo: b("ao_vivo"),
            mais: b("mais"),
            reiniciado: b("reiniciado"),
            linhas: serde_json::from_value(v.get("linhas").cloned().unwrap_or(Value::Null))
                .unwrap_or_default(),
            total_achadas: i("total_achadas").max(0) as usize,
            cortada: b("cortada"),
        }
    }
}

/// Estado de um componente na verificacao da instalacao.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstadoComp {
    Ok,
    Aviso,
    Falha,
}

#[derive(Clone, Debug)]
pub struct LinhaInstalacao {
    pub nome: String,
    pub estado: EstadoComp,
    pub detalhe: String,
}

impl LinhaInstalacao {
    /// `estado` ("ok"/"aviso"/"falha") tem prioridade; sem ele, usa o booleano `ok`.
    pub fn de_json(v: &Value) -> Self {
        let estado = match v.get("estado").and_then(Value::as_str) {
            Some("ok") => EstadoComp::Ok,
            Some("aviso") => EstadoComp::Aviso,
            Some(_) => EstadoComp::Falha,
            None => {
                if v.get("ok").and_then(Value::as_bool).unwrap_or(false) {
                    EstadoComp::Ok
                } else {
                    EstadoComp::Falha
                }
            }
        };
        LinhaInstalacao {
            nome: v
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            estado,
            detalhe: v
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct ResultadoInstalacao {
    pub linhas: Vec<LinhaInstalacao>,
    pub faltam_pip: Vec<String>,
    pub falta_modelo: bool,
}

impl ResultadoInstalacao {
    pub fn de_json(v: &Value) -> Self {
        ResultadoInstalacao {
            linhas: v
                .get("rows")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(LinhaInstalacao::de_json).collect())
                .unwrap_or_default(),
            faltam_pip: v
                .get("missing_pip")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            falta_modelo: v
                .get("missing_model")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }
    }

    pub fn faltantes(&self) -> Vec<String> {
        let mut f = self.faltam_pip.clone();
        if self.falta_modelo {
            f.push("modelo de transcrição".to_string());
        }
        f
    }

    /// Componentes que nao estao OK (aviso ou falha).
    pub fn problemas(&self) -> Vec<&LinhaInstalacao> {
        self.linhas
            .iter()
            .filter(|l| l.estado != EstadoComp::Ok)
            .collect()
    }

    pub fn resumo(&self) -> String {
        let ruins = self.problemas();
        if ruins.is_empty() {
            format!(
                "Verificação concluída: todos os {} componentes estão OK.",
                self.linhas.len()
            )
        } else {
            let nomes: Vec<&str> = ruins.iter().map(|l| l.nome.as_str()).collect();
            format!(
                "Verificação concluída: {} de {} componentes OK. Atenção: {}.",
                self.linhas.len() - ruins.len(),
                self.linhas.len(),
                nomes.join(", ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn boot_le_modos_config_e_modelos_na_ordem() {
        let v = json!({
            "version": "0.1.1",
            "convert_modes": [{"key": "auto", "label": "OCR automático", "description": "d", "accepts": "pdf", "pages": true, "format": true, "combine": false}],
            "batch_modes": [{"key": "auto_md", "label": "L", "description": "d"}],
            "config": {"language": "por", "dpi": 300, "motor": "spark", "novo_campo": 7},
            "extensions": {"pdf": [".pdf"], "media": [".mp3"]},
            "transcricao": {"default_model": "small", "installed": ["small"],
                "models": {"tiny": {"tamanho_mb": 76}, "base": {"tamanho_mb": 142.5}, "small": {"instalado": true}},
                "languages": ["pt"], "formats": ["txt"]},
            "busy": {"ocr": true, "transcricao": false}
        });
        let b = Boot::de_json(&v);
        assert_eq!(b.modos.len(), 1);
        assert!(b.modos[0].pages && b.modos[0].format && !b.modos[0].combine);
        assert_eq!(b.modos_lote[0].key, "auto_md");
        assert_eq!(b.config.dpi, 300);
        assert_eq!(b.config.psm, 3, "campo ausente usa o padrao");
        assert_eq!(
            b.config.extra["novo_campo"], 7,
            "campo desconhecido e preservado"
        );
        let nomes: Vec<&str> = b
            .transcricao
            .modelos
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        assert_eq!(nomes, ["tiny", "base", "small"]);
        assert_eq!(
            b.transcricao.info("base").and_then(|i| i.tamanho_mb),
            Some(142.5)
        );
        assert_eq!(b.transcricao.nao_instalados().len(), 2);
        assert!(b.ocupado.ocr);
        // o campo desconhecido volta ao salvar
        let volta = serde_json::to_value(&b.config).expect("json");
        assert_eq!(volta["novo_campo"], 7);
    }

    #[test]
    fn selecionaveis_cai_no_padrao_sem_instalados() {
        let e = EstadoTranscricao {
            modelo_padrao: "small".into(),
            ..Default::default()
        };
        assert_eq!(e.selecionaveis(), vec!["small".to_string()]);
    }

    #[test]
    fn execucao_texto_de_etapa_por_estado() {
        let mut e = Execucao {
            estado: "fila".into(),
            ..Default::default()
        };
        assert_eq!(e.linha_etapa(), "Aguardando a vez na fila.");
        e.estado = "rodando".into();
        assert_eq!(e.linha_etapa(), "Em andamento…");
        e.etapa = "Página 3 de 42".into();
        assert_eq!(e.linha_etapa(), "Página 3 de 42");
        e.estado = "falhou".into();
        e.erro = "PDF protegido".into();
        assert_eq!(e.linha_etapa(), "PDF protegido");
    }

    #[test]
    fn eta_usa_texto_do_motor_ou_calcula() {
        let mut e = Execucao {
            estado: "rodando".into(),
            eta_s: Some(125.0),
            ..Default::default()
        };
        assert_eq!(e.eta_exibido().as_deref(), Some("cerca de 2 min"));
        e.eta_texto = "estimando…".into();
        assert_eq!(e.eta_exibido().as_deref(), Some("estimando…"));
        e.estado = "concluida".into();
        assert_eq!(e.eta_exibido(), None);
    }

    #[test]
    fn filtro_de_execucoes() {
        let e = Execucao {
            estado: "rodando".into(),
            ferramenta: "converter".into(),
            rotulo: "OCR automático".into(),
            entrada: "Petição.pdf".into(),
            ..Default::default()
        };
        assert!(e.passa("todas", "", ""));
        assert!(e.passa("ativas", "", ""));
        assert!(!e.passa("concluida", "", ""));
        assert!(!e.passa("todas", "lote", ""));
        assert!(e.passa("todas", "converter", "petição"));
        assert!(!e.passa("todas", "", "contrato"));
    }

    #[test]
    fn lista_de_execucoes_e_ferramentas() {
        let v = json!({"rodando": 2, "na_fila": 1, "total": 3,
            "ferramentas": {"converter": "Converter", "lote": "Lote"},
            "execucoes": [{"id": "a", "estado": "rodando", "progresso": 12.5, "determinada": true}, {"id": "b"}]});
        let l = ListaExecucoes::de_json(&v);
        assert_eq!((l.rodando, l.na_fila, l.total), (2, 1, 3));
        assert_eq!(l.ferramentas[1], ("lote".to_string(), "Lote".to_string()));
        assert_eq!(l.execucoes.len(), 2);
        assert_eq!(l.execucoes[0].progresso, Some(12.5));
        assert_eq!(l.execucoes[1].progresso, None);
    }

    #[test]
    fn instalacao_usa_estado_ou_booleano() {
        let v = json!({"rows": [
            {"name": "Python", "ok": true, "detail": "3.14"},
            {"name": "Ghostscript", "ok": false, "estado": "aviso", "detail": "opcional"},
            {"name": "Tesseract", "ok": false, "estado": "falha", "detail": "x"},
            {"name": "Rich", "ok": false, "detail": "y"}],
            "missing_pip": ["Rich"], "missing_model": true});
        let r = ResultadoInstalacao::de_json(&v);
        let estados: Vec<_> = r.linhas.iter().map(|l| l.estado).collect();
        assert_eq!(
            estados,
            [
                EstadoComp::Ok,
                EstadoComp::Aviso,
                EstadoComp::Falha,
                EstadoComp::Falha
            ]
        );
        assert_eq!(
            r.faltantes(),
            vec!["Rich".to_string(), "modelo de transcrição".to_string()]
        );
        assert!(r.resumo().contains("1 de 4"));
        assert_eq!(r.problemas().len(), 3);
    }

    #[test]
    fn log_pesos_e_filtro() {
        let l = LinhaLog {
            nv: "WARNING".into(),
            ..Default::default()
        };
        assert!(l.peso() >= peso_minimo("avisos"));
        assert!(l.peso() < peso_minimo("erros"));
        assert_eq!(rotulo_nivel("ERROR"), "ERRO");
    }

    #[test]
    fn capacidades_padrao_tudo_desligado() {
        let c: Capacidades = serde_json::from_value(json!({"nativo": true})).unwrap_or_default();
        assert!(c.nativo && !c.interlocutores && !c.motor_spark);
    }
}
