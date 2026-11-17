//! Ciclo de vida do motor Python: localizar, iniciar oculto, religar se cair, encerrar.

use crate::bus::{Bus, EstadoMotor, Msg, Tag};
use crate::proto::{limite_de, Cliente};
use serde_json::Value;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Quedas aceitas dentro da janela antes de desistir de religar.
const MAX_QUEDAS: usize = 3;
const JANELA_QUEDAS: Duration = Duration::from_secs(120);
/// Teto (so para dar erro claro) para o motor encerrar sozinho antes de ser morto.
const TETO_ENCERRAR: Duration = Duration::from_secs(8);

fn trava<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Onde estao a instalacao, o Python e o script do motor.
#[derive(Clone, Debug)]
pub struct Local {
    pub raiz: PathBuf,
    pub python: PathBuf,
    pub script: PathBuf,
}

impl Local {
    /// Localiza a partir do proprio .exe. Para desenvolvimento:
    /// `EXTRATOR_RAIZ` (pasta de instalacao), `EXTRATOR_PYTHON` e `EXTRATOR_MOTOR` (script do motor).
    pub fn descobrir() -> Local {
        let raiz = std::env::var_os("EXTRATOR_RAIZ")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|e| e.parent().map(Path::to_path_buf))
            })
            .unwrap_or_else(|| PathBuf::from("."));
        let python = std::env::var_os("EXTRATOR_PYTHON")
            .map(PathBuf::from)
            .unwrap_or_else(|| raiz.join("venv").join("Scripts").join("python.exe"));
        let script = std::env::var_os("EXTRATOR_MOTOR")
            .map(PathBuf::from)
            .unwrap_or_else(|| raiz.join("src").join("motor.py"));
        Local {
            raiz,
            python,
            script,
        }
    }

    pub fn dados_logs(&self) -> PathBuf {
        self.raiz.join("dados").join("logs")
    }

    /// Descricao do que falta para iniciar o motor, se algo faltar.
    pub fn problema(&self) -> Option<String> {
        if !self.python.is_file() {
            return Some(format!(
                "Python do programa não encontrado: {}",
                self.python.display()
            ));
        }
        if !self.script.is_file() {
            return Some(format!(
                "Arquivo do motor não encontrado: {}",
                self.script.display()
            ));
        }
        None
    }

    /// So o ambiente Python faltando indica instalacao quebrada (o reparo resolve).
    pub fn precisa_reparo(&self) -> bool {
        !self.python.is_file()
    }
}

/// Registro de erros do lancador em `dados\logs\launcher.log` (nunca em caixa de mensagem).
pub fn registrar(raiz: &Path, msg: &str) {
    let dir = raiz.join("dados").join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("launcher.log"))
    {
        let _ = writeln!(
            f,
            "{}  {}",
            crate::fmt::data_local(crate::fmt::agora_unix()),
            msg
        );
    }
}

struct Interno {
    bus: Bus,
    local: Local,
    cliente: Mutex<Option<Arc<Cliente>>>,
    filho: Mutex<Option<Child>>,
    geracao: AtomicU64,
    encerrando: AtomicBool,
    quedas: Mutex<Vec<Instant>>,
}

#[derive(Clone)]
pub struct Motor {
    i: Arc<Interno>,
}

impl Motor {
    pub fn novo(bus: Bus, local: Local) -> Motor {
        Motor {
            i: Arc::new(Interno {
                bus,
                local,
                cliente: Mutex::new(None),
                filho: Mutex::new(None),
                geracao: AtomicU64::new(0),
                encerrando: AtomicBool::new(false),
                quedas: Mutex::new(Vec::new()),
            }),
        }
    }

    pub fn local(&self) -> &Local {
        &self.i.local
    }

    /// Inicia (ou reinicia) o motor em segundo plano; o resultado chega como `Msg::Motor`.
    pub fn iniciar(&self) {
        let m = self.clone();
        let r = std::thread::Builder::new()
            .name("motor-iniciar".into())
            .spawn(move || m.subir());
        if r.is_err() {
            self.i.bus.enviar(Msg::Motor(EstadoMotor::Falhou(
                "Não foi possível criar a thread do motor.".into(),
            )));
        }
    }

    /// Zera o limite de quedas (botao "Tentar de novo") e inicia.
    pub fn reiniciar_do_zero(&self) {
        trava(&self.i.quedas).clear();
        self.iniciar();
    }

    fn subir(&self) {
        let i = &self.i;
        i.bus.enviar(Msg::Motor(EstadoMotor::Iniciando));
        if let Some(p) = i.local.problema() {
            registrar(&i.local.raiz, &p);
            i.bus.enviar(Msg::Motor(EstadoMotor::Falhou(p)));
            return;
        }
        // Se ainda ha um processo antigo, encerra antes de subir outro.
        self.matar_filho();
        let gen = i.geracao.fetch_add(1, Ordering::SeqCst) + 1;

        let mut cmd = Command::new(&i.local.python);
        cmd.args(["-X", "utf8", "-u"])
            .arg(&i.local.script)
            .current_dir(&i.local.raiz)
            .env("PYTHONIOENCODING", "utf-8")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);
        let erros = i.local.dados_logs();
        let _ = std::fs::create_dir_all(&erros);
        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(erros.join("motor-stderr.log"))
        {
            Ok(f) => cmd.stderr(Stdio::from(f)),
            Err(_) => cmd.stderr(Stdio::null()),
        };
        let mut filho = match cmd.spawn() {
            Ok(f) => f,
            Err(e) => {
                let msg = format!("Não foi possível iniciar o motor: {e}");
                registrar(&i.local.raiz, &msg);
                i.bus.enviar(Msg::Motor(EstadoMotor::Falhou(msg)));
                return;
            }
        };
        let (Some(stdin), Some(stdout)) = (filho.stdin.take(), filho.stdout.take()) else {
            let _ = filho.kill();
            i.bus.enviar(Msg::Motor(EstadoMotor::Falhou(
                "O motor não abriu os canais de comunicação.".into(),
            )));
            return;
        };
        *trava(&i.filho) = Some(filho);

        let fraco = Arc::downgrade(&self.i);
        let cliente = Cliente::novo(stdout, stdin, move || {
            if let Some(i) = fraco.upgrade() {
                Motor { i }.caiu(gen);
            }
        });
        *trava(&i.cliente) = Some(Arc::clone(&cliente));

        // Pronto = o motor respondeu ao ping (sem espera fixa).
        let m = self.clone();
        cliente.chamar("ping", vec![], Duration::from_secs(90), move |r| {
            if m.i.geracao.load(Ordering::SeqCst) != gen {
                return;
            }
            match r {
                Ok(_) => m.i.bus.enviar(Msg::Motor(EstadoMotor::Pronto)),
                Err(e) => {
                    registrar(
                        &m.i.local.raiz,
                        &format!("O motor não respondeu ao ping: {e}"),
                    );
                    // Se o fluxo caiu, `caiu` ja trata; aqui so o caso de travado/sem resposta.
                    if m.i
                        .cliente
                        .lock()
                        .map(|c| c.as_ref().is_some_and(|c| c.vivo()))
                        .unwrap_or(false)
                    {
                        m.i.bus.enviar(Msg::Motor(EstadoMotor::Falhou(format!(
                            "O motor não respondeu: {e}"
                        ))));
                    }
                }
            }
        });
    }

    fn caiu(&self, gen: u64) {
        let i = &self.i;
        if i.encerrando.load(Ordering::SeqCst) || i.geracao.load(Ordering::SeqCst) != gen {
            return;
        }
        let codigo = trava(&i.filho)
            .as_mut()
            .and_then(|f| f.try_wait().ok().flatten())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "sem código de saída".to_string());
        let mut motivo = format!("O motor foi encerrado inesperadamente ({codigo}).");
        if let Some(erro) = self.cliente().and_then(|c| c.erro_sem_id()) {
            motivo = format!("{motivo} {erro}");
        }
        registrar(&i.local.raiz, &motivo);
        let tentativa = {
            let mut q = trava(&i.quedas);
            let agora = Instant::now();
            q.retain(|t| agora.duration_since(*t) < JANELA_QUEDAS);
            q.push(agora);
            q.len()
        };
        if tentativa > MAX_QUEDAS {
            let msg =
                format!("{motivo} Ele caiu {tentativa} vezes seguidas; não será religado sozinho.");
            i.bus.enviar(Msg::Motor(EstadoMotor::Falhou(msg)));
            return;
        }
        i.bus.enviar(Msg::Motor(EstadoMotor::Caiu {
            motivo,
            tentativa: tentativa as u32,
        }));
        self.iniciar();
    }

    fn matar_filho(&self) {
        if let Some(mut f) = trava(&self.i.filho).take() {
            // O python.exe do venv e um redirecionador: encerra a arvore inteira (o motor de verdade e o neto).
            let _ = Command::new("taskkill")
                .args(["/PID", &f.id().to_string(), "/T", "/F"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(CREATE_NO_WINDOW)
                .status();
            let _ = f.kill();
            let _ = f.wait();
        }
    }

    pub fn cliente(&self) -> Option<Arc<Cliente>> {
        trava(&self.i.cliente).clone()
    }

    /// Chamada assincrona: a resposta chega como `Msg::Resposta(tag, ...)`.
    pub fn chamar(&self, tag: Tag, metodo: &str, args: Vec<Value>) {
        let bus = self.i.bus.clone();
        match self.cliente() {
            Some(c) => {
                let t = tag.clone();
                let b = bus.clone();
                c.chamar(metodo, args, limite_de(metodo), move |r| {
                    b.enviar(Msg::Resposta(t, r))
                });
            }
            None => bus.enviar(Msg::Resposta(
                tag,
                Err("O motor não está em execução.".into()),
            )),
        }
    }

    /// Falha as chamadas que passaram do limite (chamado a cada volta da interface).
    pub fn varrer_limites(&self) {
        if let Some(c) = self.cliente() {
            c.varrer_limites(Instant::now());
        }
    }

    /// Encerra o motor: pede `encerrar`, fecha a entrada e espera o processo terminar
    /// (laco curto que confere a condicao); so mata se estourar o teto. Bloqueia: rode em thread.
    pub fn encerrar(&self) {
        let i = &self.i;
        i.encerrando.store(true, Ordering::SeqCst);
        if let Some(c) = self.cliente() {
            if c.vivo() {
                c.chamar("encerrar", vec![], Duration::from_secs(3), |_| {});
            }
            c.fechar_escrita();
        }
        let inicio = Instant::now();
        loop {
            let terminou = match trava(&i.filho).as_mut() {
                Some(f) => f.try_wait().map(|s| s.is_some()).unwrap_or(true),
                None => true,
            };
            if terminou {
                break;
            }
            if inicio.elapsed() >= TETO_ENCERRAR {
                registrar(
                    &i.local.raiz,
                    "O motor não encerrou a tempo; finalizado à força.",
                );
                self.matar_filho();
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        trava(&i.filho).take();
    }
}
