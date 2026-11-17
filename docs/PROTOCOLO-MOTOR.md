# Protocolo do motor (`src\motor.py`)

O motor é o processo Python que a interface nativa (Rust) inicia como filho oculto. Ele não tem janela:
expõe a classe `Api` (`src\extrator_api.py`) por **stdin/stdout, uma linha JSON por mensagem**.
Os exemplos abaixo foram gerados rodando o motor de verdade (Python 3.14, versão 0.1.0).

## 1. Iniciar e encerrar

```
<instalação>\venv\Scripts\python.exe -X utf8 -u src\motor.py
```

* Diretório de trabalho = pasta de instalação (em desenvolvimento, a pasta do repositório). Sem console: o
  pai usa `CREATE_NO_WINDOW`. O motor já aplica o patch `semjanela` (nenhum subprocesso abre console).
* **O motor não manda nada sozinho.** Não há mensagem de "pronto": envie `ping` e espere a resposta (o
  carregamento das bibliotecas leva de 1 a 3 s na primeira chamada; requisições enviadas antes ficam na fila
  do pipe e são atendidas em seguida).
* **stdout é só do protocolo.** Ao iniciar, o motor separa o stdout original para o protocolo e aponta os
  descritores 1 e 2 (inclusive os dos processos-filho: tesseract, ffmpeg, OCR) para
  `dados\logs\motor-saida.out`. Nada de `print` de biblioteca chega ao pipe. **O stderr do pai não recebe
  nada** (só erros anteriores ao início, como falha de `import` do próprio Python): não é preciso drená-lo.
  Falha ao iniciar vira um quadro `{"id": null, "ok": false, "e": "Falha ao iniciar o motor: ..."}` e saída
  com código 1.
* **Encerrar:** `encerrar` (cancela tudo, responde `ok` e sai em seguida, código 0) ou **fechar o stdin**
  (o motor cancela tudo e sai em poucos segundos; o teto interno é 3 s, mas sai na hora em que não houver
  execução ativa). Nunca fica órfão: no Windows o motor entra num Job Object que mata os processos-filho
  (tesseract, ffmpeg, workers do OCR) quando ele sai, mesmo que seja morto à força.
* **Atenção ao `pid`:** o `python.exe` do `venv` é um redirecionador no Windows. O processo que o pai cria
  (`Child::id()`) é o redirecionador; o motor de verdade é o filho dele, e `ping` devolve o pid real. Para
  matar tudo à força, use um Job Object do lado do pai (ou `taskkill /T`), não só o pid do filho direto.

## 2. Quadros

Uma linha JSON UTF-8 (sem BOM, terminada em `\n`, sem `\r`, `ensure_ascii=False`, sem quebras dentro do JSON).

**Requisição** (pai → motor):

```json
{"id": 12, "m": "start_convert", "a": [ {"mode": "direct", "files": ["C:\\docs\\a.html"], "same_folder": true} ]}
```

* `id`: qualquer valor JSON (número ou texto) que o pai escolhe; volta igual na resposta.
* `m`: nome do método. `a`: lista de argumentos posicionais (opcional; ausente = `[]`).

**Resposta** (motor → pai), sempre exatamente uma por requisição:

```json
{"id": 12, "ok": true,  "r": <resultado da Api>}
{"id": 12, "ok": false, "e": "mensagem em português"}
```

* `ok: true`: `r` é o JSON devolvido pelo método (ver seção 4). **Atenção:** os métodos da Api têm o seu
  próprio `ok` dentro de `r`: recusas de negócio vêm como `{"ok": true, "r": {"ok": false, "error": "..."}}`
  (por exemplo, "Já existe uma operação em andamento."). Confira os dois níveis.
* `ok: false`: erro de protocolo ou exceção inesperada; `e` é a mensagem. Casos: JSON inválido, método
  inexistente, argumentos errados, método que depende de janela, exceção não tratada.
* As respostas **não chegam necessariamente na ordem das requisições**: cada requisição roda em thread
  própria (uma verificação longa não bloqueia um `ping`). Case sempre pelo `id`.
* Linha que não é JSON válido, ou JSON que não é objeto, responde com `"id": null`.
* Linhas em branco são ignoradas (sem resposta).

Exemplos reais:

```
-> {"id":1,"m":"ping"}
<- {"id":1,"ok":true,"r":{"versao":"0.1.0","python":"3.14.7","pid":42736}}

-> {"id":2,"m":"capacidades"}
<- {"id":2,"ok":true,"r":{"interlocutores":false,"motor_spark":false,"nativo":true}}

-> {"id":3,"m":"nao_existe"}
<- {"id":3,"ok":false,"e":"Método inexistente: nao_existe"}

-> {isso nao e json
<- {"id":null,"ok":false,"e":"JSON inválido: Expecting property name enclosed in double quotes: line 1 column 2 (char 1)"}

-> {"id":4,"m":"exec_detalhe","a":[]}
<- {"id":4,"ok":false,"e":"Argumentos inválidos para exec_detalhe: missing a required argument: 'id'"}

-> {"id":5,"m":"pick_files","a":["pdf"]}
<- {"id":5,"ok":false,"e":"use o seletor nativo da interface"}

-> {"id":221,"m":"encerrar"}
<- {"id":221,"ok":true,"r":{"ok":true}}          (o processo sai logo depois, código 0)
```

## 3. Eventos: não há push

A interface chama **`drain_events` em intervalos curtos** (sugestão: a cada 100 ms) e
`exec_listar` para a Central de execuções. `drain_events` devolve (e esvazia) a fila de eventos gerados
desde a última chamada, na ordem; `r` é uma lista, vazia quando não há nada.

| `type` | Campos | Quando |
|---|---|---|
| `line` | `text` | linha para o log da tela |
| `status` | `text` | etapa atual ("Convertendo a.html", "Cancelando...") |
| `file_start` | `index`, `total`, `name` | começou o arquivo `index` de `total` (conversão e lote) |
| `file_error` | `name`, `text` | um arquivo falhou (a fila continua) |
| `done` | — | conversão/lote terminou |
| `cancelled` | — | conversão/lote cancelado |
| `error` | `text` | erro que interrompeu a conversão/lote |
| `start` | `file`, `total` | OCR: começou um PDF com `total` páginas |
| `page` | `current`, `total`, `page`, `source` | OCR: página processada (`source`: "texto nativo" ou a descrição do OCR aplicado) |
| `image` | `current`, `total`, `name` | OCR de imagens |
| `transc_item_start` | `id`, `index`, `total`, `name` | transcrição: começou o arquivo |
| `transc_progress` | `id`, `index`, `total`, `pct`, `processado_s`, `total_s`, `restante_s`, `elapsed_s`, `eta_s` | transcrição: andamento |
| `transc_item_done` | `id`, `output_path` | arquivo transcrito |
| `transc_item_cancelled` | `id` | item removido/cancelado |
| `transc_item_error` | `id`, `text` | item falhou |
| `transc_queue_done` | — | a fila terminou |
| `model_download` | `name`, `status` (`done`/`error`), `text` (só no erro) | download do modelo terminou |
| `check_start` | — | `start_check_installation` começou |
| `check_row` | `index`, `name`, `ok`, `estado`, `detail` | um componente verificado |
| `check_done` | o mesmo objeto de `check_installation` (seção 4.7) | verificação terminou |
| `check_error` | `text` | a verificação quebrou |
| `install_fix_done` | — | `install_missing` terminou |

Os tipos `files_dropped` e `close_blocked` só existiam na janela antiga e **não ocorrem no motor**.
O progresso fino e o histórico de cada tarefa estão na Central de execuções (seção 4.5), que é a fonte
principal para barras de progresso e estados.

Exemplo real (uma conversão direta de um `.html`, drenada durante a execução):

```json
{"id":9,"ok":true,"r":[
  {"type":"line","text":"Iniciando: Conversão direta (1 arquivo(s))"},
  {"type":"file_start","index":1,"total":1,"name":"exemplo.html"},
  {"type":"status","text":"Convertendo exemplo.html"},
  {"type":"done"}]}
```

## 4. Métodos

Todos aceitam os mesmos argumentos da `Api` atual. "→" indica o `r` de sucesso.

### 4.1 Do motor

| Método | Retorno |
|---|---|
| `ping` | `{"versao": "0.1.0", "python": "3.14.7", "pid": 42736}` (versão lida do `VERSION.txt`) |
| `capacidades` | `{"interlocutores": false, "motor_spark": false, "nativo": true}` (outros agentes ligam as flags em `motor.CAPACIDADES`) |
| `encerrar` | `{"ok": true}`; o processo sai em seguida |

### 4.2 Início e estado

| Método | Argumentos | Retorno |
|---|---|---|
| `bootstrap` | — | `{ok, app_name, version, convert_modes[], batch_modes[], config{}, defaults{}, extensions{pdf[], direct[], images[], media[]}, data_dir, log_file, log_dir, transcricao{...}, busy{}}` |
| `drain_events` | — | lista de eventos (seção 3) |
| `is_busy` | — | `{"ocr": false, "transcricao": false}` |
| `request_stop_all` | — | `null` (cancela conversão e transcrição) |
| `open_path` | `path` | `{ok: true}`: abre a pasta do arquivo no Explorer (`os.startfile`) |
| `open_data_folder` | — | `{ok: true}` |
| `log_abrir_pasta` | — | `{ok: true}` |

`convert_modes[]`: `{key, label, description, accepts ("pdf"|"direct"|"images"), pages, format, combine}`.
`batch_modes[]`: `{key, label, description}`.

### 4.3 Conversão e lote

| Método | Argumentos | Retorno |
|---|---|---|
| `start_convert` | `params`: `mode` (chave de `convert_modes`), `files[]`, `pages` ("1-3,5"), `format` ("md"/"txt"), `combine_images`, `same_folder`, `output_folder`, `confirm_signed` | `{ok:true,status:"started"}`; `{ok:true,status:"needs_confirm",signed:[nomes]}` (PDFs assinados: reenvie com `confirm_signed:true`); `{ok:false,error}` |
| `cancel_job` | — | `{ok: true}` |
| `scan_batch` | `params`: `mode`, `input_folder`, `output_folder`, `recursive` | `{ok,count,signed[],output_folder,sample[]}` |
| `start_batch` | `params` (mesmos de `scan_batch` + `confirm_signed`) | `{ok:true,status:"started",count}` ou `{ok:false,error}` |

### 4.4 Transcrição, diagnóstico, configurações

| Método | Argumentos | Retorno |
|---|---|---|
| `transcricao_state` | — | `{ok, default_model, installed[], models{nome:{tamanho_mb,precisao,velocidade,descricao,instalado}}, languages[], formats[]}` |
| `transcricao_deps` | — | `{ok, deps:[{name, ok}]}` |
| `start_transcricao` | `params`: `items:[{id, path}]`, `model`, `language`, `format`, `beams`, `word_level` | `{ok:true,status:"started"}` / `{ok:false,error}` |
| `toggle_pause_transcricao` | — | `{ok, paused}` |
| `cancel_transcricao` | — | `{ok:true}` |
| `download_model` | `name` | `{ok:true,status:"started"}` / `{ok:false,error}` |
| `diagnose` | `path` | `{ok, rows:[{item, result}]}` |
| `save_settings` | `values` (`language`, `dpi`, `psm`, `native_text_threshold`, `verbosity`, `cpu_mode`, `pdf_output_type`, `rotate_pages`, `deskew`, `resume`, `auto_open_output`) | `{ok, config}` |
| `restore_settings` | — | `{ok, config}` |
| `clear_data` | — | `{ok, logs, jobs}` |

Exemplo real de `transcricao_state`:

```json
{"ok":true,"default_model":"small","installed":[],
 "models":{"tiny":{"tamanho_mb":76,"precisao":"Baixa","velocidade":"Muito rapida","descricao":"Para experimentacao rapida","instalado":false},
           "small":{"tamanho_mb":460,"precisao":"Boa","velocidade":"Moderada","descricao":"Melhor equilibrio (padrao)","instalado":false}},
 "languages":["pt","en","es","fr","de","ja","ko","auto"],"formats":["txt","md","srt","json"]}
```
(a lista real tem 5 modelos: `tiny`, `base`, `small`, `medium`, `large-v3`.)

### 4.5 Central de execuções

Toda tarefa (conversão, lote, transcrição, verificação, download de modelo/ffmpeg, instalação, limpeza...) é
uma execução. Uma execução em andamento é consultada por `exec_listar` (visão geral) e `exec_detalhe` (log).

| Método | Argumentos | Retorno |
|---|---|---|
| `exec_listar` | — | `{ok, versao, rodando, na_fila, total, execucoes[], ferramentas{}, estados{}}` |
| `exec_detalhe` | `id`, `desde=0` | `{ok, execucao{}, proximo, ativo, linhas:[{t,n,x}]}` |
| `exec_cancelar` | `id` | `{ok:true}` ou `{ok:false,error}` |
| `exec_repetir` | `id` | resultado da ação repetida + `acao` |
| `exec_resumo` | `id` | `{ok, texto}` (texto para copiar) |
| `exec_limpar_concluidas` | — | `{ok, removidas}` |

`exec_listar`: `versao` sobe a cada mudança (use para pular redesenhos); `execucoes` vem da mais nova para a
mais antiga; `ferramentas` mapeia chave → nome ("converter": "Converter", "instalacao": "Instalação", ...);
`estados` mapeia chave → texto.

Cada execução (também em `exec_detalhe.execucao`), exemplo real:

```json
{"id":"00fc531d4bcb","ferramenta":"converter","ferramenta_nome":"Converter","rotulo":"Conversão direta",
 "entrada":"exemplo.html","variante":"direct","estado":"concluida","estado_texto":"Concluída",
 "etapa":"Convertendo exemplo.html","determinada":false,"progresso":null,"progresso_texto":"",
 "decorrido_s":7.5,"criada_em":1794925867.67,"inicio_em":1794925867.67,"fim_em":1794925875.18,
 "mensagem":"1 arquivo(s) processado(s).","erro":"","avisos":0,"cancelavel":false,"cancelando":false,
 "repetivel":true,"log_arquivo":"extrator_20261117_113055.log","log_ini":1371,"log_fim":2282,"linhas":2,
 "eta_s":null,"eta_min_s":null,"eta_max_s":null,"eta_texto":"","eta_fonte":""}
```

* `estado`: `fila`, `rodando`, `concluida`, `falhou`, `cancelada`, `interrompida` (execução que o programa
  deixou aberta ao fechar). Ativas = `fila` e `rodando`.
* `determinada` + `progresso` (0 a 100): só há porcentagem quando o total é conhecido; senão `null` (barra
  indeterminada, mostre `etapa`). `progresso_texto`: "15 de 15 componentes", "00:01:20 de 00:05:00" (áudio),
  "12,5 MB de 72,0 MB" (download).
* `eta_*`: estimativa (`eta_texto` já formatado, `eta_fonte`: de onde veio). Vazios quando não há base.
* `log_arquivo`, `log_ini`, `log_fim`: trecho do log da sessão (bytes) que pertence a esta execução; use com
  `log_ler` (`modo: "intervalo"`).
* `exec_detalhe(id, desde)`: `linhas` traz só o que passou de `desde`; guarde `proximo` e passe-o na próxima
  chamada. `linhas[]`: `{"t": "11:31:07", "n": "info"|"aviso"|"erro", "x": "texto"}`.
* **Download do ffmpeg** (~72 MB, só na primeira verificação ou transcrição): aparece como execução
  `ferramenta: "instalacao"`, `rotulo: "Baixando ffmpeg"`, `variante: "ffmpeg"`, etapa "Baixando ffmpeg" →
  "Extraindo o ffmpeg", progresso em bytes (`progresso_texto` "12,5 MB de 72,0 MB") quando o servidor informa o
  tamanho (senão, só a etapa); é cancelável. Em caso de falha (sem internet) a execução fica `falhou`; uma
  nova tentativa só acontece depois de 30 s.

### 4.6 Logs

| Método | Argumentos | Retorno |
|---|---|---|
| `log_arquivos` | — | `{ok, atual, pasta, arquivos:[{nome, tamanho, modificado, data, hora, atual, tipo}]}` |
| `log_ler` | `params`: `nome`, `modo` ("cauda"\|"desde"\|"antes"\|"intervalo"\|"ao_redor"), `max_bytes`, `offset`, `nv_ant`, `ini`, `fim` | `{ok, nome, tamanho, ini, fim, linhas[], ao_vivo}` |
| `log_buscar` | `params`: `nome`, `nivel` ("tudo"\|"avisos"\|"erros"), `texto`, `limite` | `{ok, nome, ini, fim, linhas[], truncado_inicio, total_achadas, cortada, ao_vivo}` |
| `log_registrar` | `nivel` ("ERROR"\|"WARNING"\|"INFO"), `texto` | `{ok, nome, offset}` (grava no log da sessão; "Ver no log") |

`log_ler` real (cauda de 600 bytes); cada linha é `{o: deslocamento em bytes, t: "AAAA-MM-DD HH:MM:SS",
nv: "DEBUG"|"INFO"|"WARNING"|"ERROR", x: mensagem}`:

```json
{"ok":true,"nome":"extrator_20261117_113055.log","tamanho":2282,"ini":1371,"fim":2282,
 "linhas":[{"o":1371,"t":"2026-11-17 11:31:07","nv":"INFO","x":"[exec 00fc531d4bcb] Conversão direta iniciada: exemplo.html"},
           {"o":1466,"t":"2026-11-17 11:31:07","nv":"INFO","x":"[exec 00fc531d4bcb] etapa: Arquivo 1 de 1: exemplo.html"}],
 "ao_vivo":true}
```

`log_arquivos` só lista `*.log` e `*.txt`; a saída de diagnóstico do motor (`motor-saida.out`) não aparece.

### 4.7 Instalação: `check_installation` e `start_check_installation`

`check_installation` é síncrono (devolve o resultado inteiro); `start_check_installation` roda em segundo plano
e devolve `{ok:true,status:"started"}` na hora: cada componente vira um evento `check_row` e o fim vira
`check_done` (ou `check_error`). Os dois criam uma execução "Verificar instalação" com barra determinada
(componentes verificados / total).

Resultado (também é o corpo do evento `check_done`), exemplo real:

```json
{"ok":true,
 "rows":[
  {"name":"Python","ok":true,"estado":"ok","detail":"3.14.7"},
  {"name":"MarkItDown","ok":true,"estado":"ok","detail":"0.1.6"},
  {"name":"Idiomas OCR","ok":false,"estado":"falha","detail":"eng, osd"},
  {"name":"Modelo de transcricao","ok":true,"estado":"aviso","detail":"será baixado no 1º uso"}],
 "total":15,"n_ok":12,"n_aviso":2,"n_falha":1,
 "resumo":"12 de 15 OK, 2 avisos, 1 falha",
 "missing_pip":[],"missing_model":true}
```

* `estado` de cada linha: `"ok"`, `"aviso"` ou `"falha"`. `ok` continua booleano e **aviso conta como `ok: true`**
  (só `falha` é `ok: false`).
* **Aviso** = ausente mas não é defeito: o **modelo de transcrição** (baixado no 1º uso: "será baixado no 1º
  uso"). Nada disso é gravado no log como ERRO. O ffmpeg, as
  bibliotecas, o Tesseract e os idiomas OCR ausentes são `falha`.
* `n_ok` não inclui os avisos: `n_ok + n_aviso + n_falha = total`. `resumo` é o texto pronto
  ("14 de 15 OK, 1 aviso"; com falhas: "13 de 15 OK, 1 aviso, 1 falha"). A mensagem da execução na Central
  acrescenta ". Faltam: <nomes das falhas>." quando há falhas.
* `missing_pip`: nomes de pacotes pip em **falha** (o que `install_missing` instala). `missing_model`:
  `true` quando não há nenhum modelo baixado (`install_missing` baixa o padrão).
* No `check_row`, `index` vai de 1 a `total`; os mesmos campos `name`, `ok`, `estado`, `detail`.

`install_missing` → `{ok:true,status:"started"}`, `{ok:true,status:"nothing"}` ou `{ok:false,error}`; termina com
o evento `install_fix_done` e uma execução `ferramenta: "instalacao"`.

## 5. Métodos que não existem no motor

`pick_files`, `pick_folder`, `attach_window` e `notify_close_blocked` eram da janela antiga (WebView, já removida). A
interface nativa usa diálogos próprios; se forem chamados, a resposta é

```json
{"id": 5, "ok": false, "e": "use o seletor nativo da interface"}
```

Os caminhos escolhidos entram em `start_convert.files`, `start_batch.input_folder` etc. como texto normal.

## 6. Receita para o pai (Rust)

1. Criar o processo com `CREATE_NO_WINDOW`, stdin e stdout em pipe, stderr nulo; colocá-lo num Job Object.
2. Uma thread lê stdout linha a linha e entrega cada resposta pelo `id`; outra escreve as requisições.
3. `ping` até responder; depois `bootstrap`.
4. Timer de ~100 ms: `drain_events`; timer de ~500 ms (ou ao receber eventos): `exec_listar`.
5. Ao fechar: `encerrar` (e/ou fechar o stdin) e esperar o processo sair; só então matar à força.
