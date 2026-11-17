//! Cliente do protocolo do motor: JSON por linha em stdin/stdout.
//!
//! Requisicao: `{"id": 12, "m": "start_convert", "a": [ {...} ]}`
//! Resposta:   `{"id": 12, "ok": true, "r": <json>}` ou `{"id": 12, "ok": false, "e": "mensagem"}`
//!
//! Este modulo nao conhece a interface nem processos: recebe um leitor e um
//! escritor, o que permite testar framing, ids, timeouts e queda do motor.

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub type Resposta = Result<Value, String>;
type Callback = Box<dyn FnOnce(Resposta) + Send>;

struct Pendente {
    cb: Callback,
    limite: Instant,
    metodo: String,
}

pub struct Cliente {
    escritor: Mutex<Box<dyn Write + Send>>,
    pendentes: Arc<Mutex<HashMap<u64, Pendente>>>,
    proximo: AtomicU64,
    vivo: Arc<AtomicBool>,
    /// Ultimo quadro de erro sem `id` (ex.: "Falha ao iniciar o motor: ...").
    erro_sem_id: Arc<Mutex<Option<String>>>,
}

fn trava<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // Um panic em outra thread nao deve derrubar a interface inteira.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Converte a resposta do protocolo no formato usado pela interface.
///
/// - `ok:false` vira `Err(e)`.
/// - `ok:true` com `r` do tipo `{"ok": false, "error": "..."}` (erro da API Python) vira `Err(error)`.
pub fn achatar(resp: &Value) -> Resposta {
    if resp.get("ok").and_then(Value::as_bool) == Some(false) {
        let e = resp
            .get("e")
            .and_then(Value::as_str)
            .unwrap_or("Erro desconhecido no motor.");
        return Err(e.to_string());
    }
    let r = resp.get("r").cloned().unwrap_or(Value::Null);
    if r.get("ok").and_then(Value::as_bool) == Some(false) {
        let e = r
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("Não foi possível concluir.");
        return Err(e.to_string());
    }
    Ok(r)
}

impl Cliente {
    /// Cria o cliente e inicia a thread leitora. `ao_fechar` roda uma vez, quando o
    /// fluxo de leitura termina (motor encerrado ou morto).
    pub fn novo(
        leitor: impl Read + Send + 'static,
        escritor: impl Write + Send + 'static,
        ao_fechar: impl FnOnce() + Send + 'static,
    ) -> Arc<Self> {
        let c = Arc::new(Cliente {
            escritor: Mutex::new(Box::new(escritor)),
            pendentes: Arc::new(Mutex::new(HashMap::new())),
            proximo: AtomicU64::new(1),
            vivo: Arc::new(AtomicBool::new(true)),
            erro_sem_id: Arc::new(Mutex::new(None)),
        });
        let pendentes = Arc::clone(&c.pendentes);
        let vivo = Arc::clone(&c.vivo);
        let sem_id = Arc::clone(&c.erro_sem_id);
        std::thread::Builder::new()
            .name("motor-leitor".into())
            .spawn(move || {
                let mut r = BufReader::new(leitor);
                let mut linha = Vec::new();
                loop {
                    linha.clear();
                    match r.read_until(b'\n', &mut linha) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {
                            let texto = String::from_utf8_lossy(&linha);
                            Self::tratar_linha(&pendentes, &sem_id, texto.trim());
                        }
                    }
                }
                vivo.store(false, Ordering::SeqCst);
                let todos: Vec<Pendente> = trava(&pendentes).drain().map(|(_, p)| p).collect();
                for p in todos {
                    (p.cb)(Err("O motor foi encerrado.".to_string()));
                }
                ao_fechar();
            })
            .ok();
        c
    }

    fn tratar_linha(
        pendentes: &Mutex<HashMap<u64, Pendente>>,
        sem_id: &Mutex<Option<String>>,
        texto: &str,
    ) {
        if texto.is_empty() {
            return;
        }
        // Linhas que nao sao JSON (um print perdido do Python) sao ignoradas.
        let Ok(v) = serde_json::from_str::<Value>(texto) else {
            return;
        };
        let Some(id) = v.get("id").and_then(Value::as_u64) else {
            // Quadro de erro sem id (falha ao iniciar, JSON invalido): guarda para explicar uma queda.
            if v.get("ok").and_then(Value::as_bool) == Some(false) {
                if let Some(e) = v.get("e").and_then(Value::as_str) {
                    *trava(sem_id) = Some(e.to_string());
                }
            }
            return;
        };
        let p = trava(pendentes).remove(&id);
        if let Some(p) = p {
            (p.cb)(achatar(&v));
        }
    }

    /// Fecha a entrada do motor (o motor ve fim de arquivo e deve encerrar sozinho).
    pub fn fechar_escrita(&self) {
        *trava(&self.escritor) = Box::new(std::io::sink());
    }

    /// Mensagem do ultimo quadro de erro que o motor mandou sem `id`.
    pub fn erro_sem_id(&self) -> Option<String> {
        trava(&self.erro_sem_id).clone()
    }

    pub fn vivo(&self) -> bool {
        self.vivo.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub fn pendentes(&self) -> usize {
        trava(&self.pendentes).len()
    }

    /// Envia uma chamada; `cb` recebe a resposta, o erro, o tempo esgotado ou a queda do motor.
    pub fn chamar(
        &self,
        metodo: &str,
        args: Vec<Value>,
        limite: Duration,
        cb: impl FnOnce(Resposta) + Send + 'static,
    ) {
        if !self.vivo() {
            cb(Err("O motor não está em execução.".to_string()));
            return;
        }
        let id = self.proximo.fetch_add(1, Ordering::SeqCst);
        let linha = json!({"id": id, "m": metodo, "a": args}).to_string();
        trava(&self.pendentes).insert(
            id,
            Pendente {
                cb: Box::new(cb),
                limite: Instant::now() + limite,
                metodo: metodo.to_string(),
            },
        );
        let res = {
            let mut w = trava(&self.escritor);
            w.write_all(linha.as_bytes())
                .and_then(|_| w.write_all(b"\n"))
                .and_then(|_| w.flush())
        };
        if let Err(e) = res {
            self.vivo.store(false, Ordering::SeqCst);
            if let Some(p) = trava(&self.pendentes).remove(&id) {
                (p.cb)(Err(format!("Falha ao falar com o motor: {e}")));
            }
        }
    }

    /// Falha as chamadas que passaram do limite. Retorna quantas expiraram.
    pub fn varrer_limites(&self, agora: Instant) -> usize {
        let vencidas: Vec<Pendente> = {
            let mut m = trava(&self.pendentes);
            let ids: Vec<u64> = m
                .iter()
                .filter(|(_, p)| p.limite <= agora)
                .map(|(i, _)| *i)
                .collect();
            ids.into_iter().filter_map(|i| m.remove(&i)).collect()
        };
        let n = vencidas.len();
        for p in vencidas {
            let msg = format!("Tempo esgotado esperando o motor ({}).", p.metodo);
            (p.cb)(Err(msg));
        }
        n
    }
}

/// Limite de espera por metodo: chamadas sincronas longas ganham mais tempo.
pub fn limite_de(metodo: &str) -> Duration {
    match metodo {
        "ping" | "drain_events" | "exec_listar" | "exec_detalhe" | "log_ler" | "is_busy" => {
            Duration::from_secs(30)
        }
        "diagnose" | "scan_batch" | "check_installation" | "clear_data" | "transcricao_deps" => {
            Duration::from_secs(900)
        }
        _ => Duration::from_secs(120),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{pipe, PipeReader, PipeWriter};
    use std::sync::mpsc;

    /// Par de canais: (lado do motor falso, lado do cliente).
    struct Par {
        motor_le: BufReader<PipeReader>,
        motor_escreve: PipeWriter,
        cliente: Arc<Cliente>,
        fechou: mpsc::Receiver<()>,
    }

    fn par() -> Par {
        let (req_r, req_w) = pipe().expect("pipe");
        let (resp_r, resp_w) = pipe().expect("pipe");
        let (tx, rx) = mpsc::channel();
        let cliente = Cliente::novo(resp_r, req_w, move || {
            let _ = tx.send(());
        });
        Par {
            motor_le: BufReader::new(req_r),
            motor_escreve: resp_w,
            cliente,
            fechou: rx,
        }
    }

    fn ler_requisicao(p: &mut Par) -> Value {
        let mut s = String::new();
        p.motor_le.read_line(&mut s).expect("ler");
        serde_json::from_str(&s).expect("json")
    }

    fn esperar<T>(rx: &mpsc::Receiver<T>) -> T {
        rx.recv_timeout(Duration::from_secs(5))
            .expect("resposta a tempo")
    }

    #[test]
    fn requisicao_tem_id_metodo_e_args() {
        let mut p = par();
        let (tx, rx) = mpsc::channel();
        p.cliente.chamar(
            "start_convert",
            vec![json!({"mode": "auto"})],
            Duration::from_secs(5),
            move |r| {
                let _ = tx.send(r);
            },
        );
        let req = ler_requisicao(&mut p);
        assert_eq!(req["id"], 1);
        assert_eq!(req["m"], "start_convert");
        assert_eq!(req["a"][0]["mode"], "auto");
        writeln!(
            p.motor_escreve,
            r#"{{"id": 1, "ok": true, "r": {{"ok": true, "status": "started"}}}}"#
        )
        .expect("w");
        let r = esperar(&rx).expect("ok");
        assert_eq!(r["status"], "started");
    }

    #[test]
    fn ids_distintos_e_respostas_fora_de_ordem() {
        let mut p = par();
        let (tx, rx) = mpsc::channel();
        for nome in ["a", "b"] {
            let tx = tx.clone();
            p.cliente
                .chamar(nome, vec![], Duration::from_secs(5), move |r| {
                    let _ = tx.send((nome, r));
                });
        }
        let r1 = ler_requisicao(&mut p);
        let r2 = ler_requisicao(&mut p);
        assert_ne!(r1["id"], r2["id"]);
        // responde a segunda antes da primeira
        writeln!(
            p.motor_escreve,
            r#"{{"id": {}, "ok": true, "r": "segunda"}}"#,
            r2["id"]
        )
        .expect("w");
        writeln!(
            p.motor_escreve,
            r#"{{"id": {}, "ok": true, "r": "primeira"}}"#,
            r1["id"]
        )
        .expect("w");
        let (n1, v1) = esperar(&rx);
        let (n2, v2) = esperar(&rx);
        assert_eq!((n1, v1.expect("ok")), ("b", json!("segunda")));
        assert_eq!((n2, v2.expect("ok")), ("a", json!("primeira")));
    }

    #[test]
    fn erro_do_motor_e_erro_da_api_viram_err() {
        assert_eq!(
            achatar(&json!({"id": 1, "ok": false, "e": "falhou"})),
            Err("falhou".to_string())
        );
        assert_eq!(
            achatar(&json!({"id": 1, "ok": true, "r": {"ok": false, "error": "PDF protegido"}})),
            Err("PDF protegido".to_string())
        );
        assert!(achatar(&json!({"id": 1, "ok": true, "r": {"ok": true, "x": 1}})).is_ok());
        assert_eq!(achatar(&json!({"id": 1, "ok": true})), Ok(Value::Null));
    }

    #[test]
    fn erro_sem_id_fica_guardado_para_explicar_a_queda() {
        let mut p = par();
        writeln!(
            p.motor_escreve,
            r#"{{"id": null, "ok": false, "e": "Falha ao iniciar o motor: import"}}"#
        )
        .expect("w");
        let Par {
            motor_escreve,
            fechou,
            cliente,
            motor_le,
        } = p;
        drop(motor_escreve);
        esperar(&fechou);
        assert_eq!(
            cliente.erro_sem_id().as_deref(),
            Some("Falha ao iniciar o motor: import")
        );
        drop(motor_le);
    }

    #[test]
    fn linhas_que_nao_sao_json_sao_ignoradas() {
        let mut p = par();
        let (tx, rx) = mpsc::channel();
        p.cliente
            .chamar("ping", vec![], Duration::from_secs(5), move |r| {
                let _ = tx.send(r);
            });
        let req = ler_requisicao(&mut p);
        writeln!(p.motor_escreve, "isto e um print perdido").expect("w");
        writeln!(p.motor_escreve).expect("w");
        writeln!(p.motor_escreve, r#"{{"sem_id": true}}"#).expect("w");
        writeln!(
            p.motor_escreve,
            r#"{{"id": {}, "ok": true, "r": "pong"}}"#,
            req["id"]
        )
        .expect("w");
        assert_eq!(esperar(&rx).expect("ok"), json!("pong"));
    }

    #[test]
    fn tempo_esgotado_falha_so_a_chamada_vencida() {
        let mut p = par();
        let (tx, rx) = mpsc::channel();
        let tx2 = tx.clone();
        p.cliente
            .chamar("lenta", vec![], Duration::from_millis(1), move |r| {
                let _ = tx.send(("lenta", r));
            });
        p.cliente
            .chamar("longa", vec![], Duration::from_secs(60), move |r| {
                let _ = tx2.send(("longa", r));
            });
        let _ = ler_requisicao(&mut p);
        let _ = ler_requisicao(&mut p);
        let n = p
            .cliente
            .varrer_limites(Instant::now() + Duration::from_millis(50));
        assert_eq!(n, 1);
        let (nome, r) = esperar(&rx);
        assert_eq!(nome, "lenta");
        assert!(r.expect_err("tempo").contains("Tempo esgotado"));
        assert_eq!(p.cliente.pendentes(), 1);
    }

    #[test]
    fn motor_que_morre_falha_pendentes_e_avisa() {
        let p = par();
        let (tx, rx) = mpsc::channel();
        p.cliente
            .chamar("diagnose", vec![], Duration::from_secs(60), move |r| {
                let _ = tx.send(r);
            });
        let Par {
            motor_escreve,
            fechou,
            cliente,
            motor_le,
        } = p;
        drop(motor_escreve); // o motor "morre": fim do fluxo
        let err = esperar(&rx).expect_err("encerrado");
        assert!(err.contains("encerrado"));
        esperar(&fechou);
        assert!(!cliente.vivo());
        // chamadas depois da queda falham na hora
        let (tx, rx) = mpsc::channel();
        cliente.chamar("ping", vec![], Duration::from_secs(1), move |r| {
            let _ = tx.send(r);
        });
        assert!(esperar(&rx).is_err());
        drop(motor_le);
    }

    #[test]
    fn escrita_quebrada_marca_o_motor_como_morto() {
        let (resp_r, _resp_w) = pipe().expect("pipe");
        let (req_r, req_w) = pipe().expect("pipe");
        drop(req_r); // ninguem le as requisicoes: a escrita falha
        let c = Cliente::novo(resp_r, req_w, || {});
        let (tx, rx) = mpsc::channel();
        c.chamar("ping", vec![], Duration::from_secs(5), move |r| {
            let _ = tx.send(r);
        });
        assert!(esperar(&rx).is_err());
        assert!(!c.vivo());
    }

    #[test]
    fn limites_por_metodo() {
        assert!(limite_de("drain_events") < limite_de("start_convert"));
        assert!(limite_de("start_convert") < limite_de("diagnose"));
    }
}
