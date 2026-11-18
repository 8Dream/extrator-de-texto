"""
Extrator de Texto - tradutor central de mensagens.

Tudo o que o usuario ve deve estar em portugues. Bibliotecas e programas
externos (Python, Tesseract, OCRmyPDF, ffmpeg, faster-whisper, MarkItDown,
pip, Windows) devolvem erros em ingles; este modulo converte os conhecidos
por um dicionario de padroes e, para um erro desconhecido, devolve uma frase
em portugues seguida do texto original como "detalhe tecnico".

Funcoes publicas:
- erro_usuario(exc_ou_texto, acao): mensagem final de um erro, em portugues.
- traduzir_linha(linha): traduz uma linha de saida de programa externo (sem
  perder linhas desconhecidas, que voltam como estao).
- parece_ingles(texto): True quando o texto parece estar em ingles.
"""
from __future__ import annotations

import re

# --------------------------------------------------------------------------
# Padroes de erro (regex em ingles -> modelo em portugues; \1 \2 = grupos)
# --------------------------------------------------------------------------
_ERROS: list[tuple[str, str]] = [
    # Windows / Python: arquivos e pastas
    (r"\[WinError 2\] The system cannot find the file specified.*", "O sistema não encontrou o arquivo especificado."),
    (r"\[WinError 3\] The system cannot find the path specified.*", "O sistema não encontrou o caminho especificado."),
    (r"\[WinError 5\] Access is denied.*", "Acesso negado pelo Windows."),
    (
        r"\[WinError 32\] The process cannot access the file because it is being used by another process.*",
        "O arquivo está em uso por outro programa. Feche-o e tente de novo.",
    ),
    (r"\[WinError 206\] The filename or extension is too long.*", "O caminho do arquivo é longo demais."),
    (r"\[Errno 2\] No such file or directory: '?(.*?)'?$", r"Arquivo ou pasta não encontrado: \1"),
    (r"No such file or directory: '?(.*?)'?$", r"Arquivo ou pasta não encontrado: \1"),
    (r"No such file or directory.*", "Arquivo ou pasta não encontrado."),
    (r"\[Errno 13\] Permission denied: '?(.*?)'?$", r"Permissão negada: \1. Se o arquivo estiver aberto em outro programa, feche-o."),
    (r"Permission denied.*", "Permissão negada. Se o arquivo estiver aberto em outro programa, feche-o."),
    (r"\[Errno 28\] No space left on device.*", "Não há espaço livre no disco."),
    (r"No space left on device.*", "Não há espaço livre no disco."),
    (r"\[Errno 22\] Invalid argument.*", "Nome ou caminho de arquivo inválido."),
    (r"\[Errno 21\] Is a directory.*", "Era esperado um arquivo, mas o caminho é uma pasta."),
    (r"\[Errno 20\] Not a directory.*", "Era esperada uma pasta, mas o caminho é um arquivo."),
    (r"\[Errno 17\] File exists.*", "Já existe um arquivo com esse nome."),
    (r"\[Errno 30\] Read-only file system.*", "O destino é somente leitura."),
    (r"\[Errno 39\] Directory not empty.*", "A pasta não está vazia."),
    (r"File name too long.*", "O nome do arquivo é longo demais."),
    # Rede
    (
        r".*(getaddrinfo failed|Name or service not known|Temporary failure in name resolution|"
        r"nodename nor servname).*",
        "Sem conexão com a internet (não foi possível encontrar o servidor).",
    ),
    (r".*(Connection refused|Connection reset|Connection aborted|Max retries exceeded|Network is unreachable|Failed to establish a new connection).*",
     "Sem conexão com a internet ou o servidor recusou a conexão."),
    (r"(?!Command ).*(timed out|Timeout|TimeoutError).*","O tempo de espera esgotou. Verifique a conexão e tente de novo."),
    (r"HTTP Error (\d+): (.*)", r"O servidor respondeu com erro \1 (\2)."),
    (r"<urlopen error (.*)>", r"Não foi possível acessar o endereço: \1"),
    (r"SSL: CERTIFICATE_VERIFY_FAILED.*", "Falha ao verificar o certificado de segurança do servidor."),
    # Python
    (r"No module named '?([\w\.]+)'?", r"Falta o módulo '\1' do Python. Use \"Reparar instalação\" no Menu Iniciar."),
    (r"cannot import name '?(\w+)'?.*", r"Falha ao carregar a biblioteca '\1'. Use \"Reparar instalação\" no Menu Iniciar."),
    (r"list index out of range", "Posição inexistente em uma lista."),
    (r"invalid literal for int\(\) with base 10: '?(.*?)'?$", r"Valor numérico inválido: \1"),
    (r"could not convert string to float: '?(.*?)'?$", r"Valor numérico inválido: \1"),
    (r"'NoneType' object has no attribute '?(\w+)'?", "Um dado esperado não estava disponível."),
    (r"'(\w+)' object has no attribute '?(\w+)'?", "Um dado esperado não estava disponível."),
    (r"object is not subscriptable", "Um dado esperado não estava disponível."),
    (r"Command '(.*)' returned non-zero exit status (\d+)\.?", r"O comando terminou com falha (código \2)."),
    (r"Command '(.*)' timed out after ([\d\.]+) seconds?", r"O comando demorou mais de \2 segundos e foi interrompido."),
    (r"'utf-8' codec can't decode.*", "Não foi possível ler o texto do arquivo (codificação inesperada)."),
    (r"'charmap' codec can't (en|de)code.*", "O texto contém caracteres que não puderam ser gravados."),
    (r"MemoryError.*|Unable to allocate .*", "Memória insuficiente para esta operação."),
    (r"Interpreter change detected.*", "O Python foi trocado durante a execução; reinicie o programa."),
    # Tesseract / pytesseract
    (
        r".*tesseract is not installed or it's not in your PATH.*",
        "O Tesseract (programa de reconhecimento de texto) não está instalado ou não foi encontrado. "
        "Use \"Reparar instalação\" no Menu Iniciar.",
    ),
    (r".*Failed loading language '?([\w\+]+)'?.*", r"Não foi possível carregar o idioma '\1' do Tesseract."),
    (r".*Error opening data file (.*)", r"O Tesseract não conseguiu abrir o arquivo de idioma \1"),
    (r".*Could not initialize tesseract.*", "Não foi possível iniciar o Tesseract."),
    (r".*Image too large.*", "A imagem é grande demais para o reconhecimento de texto."),
    (r".*Estimating resolution as (\d+).*", r"Resolução estimada em \1 DPI."),
    (r".*Empty page!+.*", "Página vazia."),
    (r"Tesseract Error:?(.*)", r"Erro do Tesseract:\1"),
    # OCRmyPDF
    (r".*PriorOcrFoundError.*|.*page already has text.*", "Esta página já tem texto (há um OCR anterior)."),
    (r".*EncryptedPdfError.*|.*Input PDF is encrypted.*", "O PDF está protegido por senha."),
    (r".*DigitalSignatureError.*|.*digital signature.*", "O PDF tem assinatura digital; o OCR a invalidaria."),
    (r".*(InputFileError|not a valid PDF|Input file is not a PDF|is not a PDF).*", "O arquivo não é um PDF válido."),
    (r".*MissingDependencyError.*", "Falta um programa necessário (Tesseract ou Ghostscript). Use \"Reparar instalação\" no Menu Iniciar."),
    (r".*SubprocessOutputError.*", "Um programa auxiliar (Ghostscript ou Tesseract) terminou com falha."),
    (r".*TaggedPDFError.*", "O PDF é marcado (acessível) e não pode receber OCR sem perder a marcação."),
    (r".*PdfMergeFailedError.*|.*Failed to merge.*", "Falha ao juntar as páginas do PDF."),
    (r".*Ghostscript.*not (found|installed).*", "O Ghostscript não foi encontrado."),
    (r".*UnsupportedImageFormatError.*", "Formato de imagem não aceito."),
    (r".*DpiError.*", "Não foi possível determinar a resolução (DPI) da imagem."),
    # ffmpeg
    (r".*moov atom not found.*", "O arquivo de vídeo está incompleto ou corrompido."),
    (r".*Invalid data found when processing input.*", "O arquivo está corrompido ou não é de áudio/vídeo."),
    (r".*(does not contain any stream|Stream map '.*' matches no streams).*", "O arquivo não tem faixa de áudio."),
    (r".*Error opening input.*", "Não foi possível abrir o arquivo de entrada."),
    (r".*Unknown encoder.*", "O ffmpeg não tem o codificador necessário."),
    (r".*Output file #0 does not contain any stream.*", "O arquivo não tem faixa de áudio."),
    # faster-whisper / Hugging Face / CUDA
    (
        r".*(Cannot find an appropriate cached snapshot|cannot find the requested files in the local cache|"
        r"Entry Not Found|Repository Not Found|LocalEntryNotFoundError).*",
        "Não foi possível baixar o modelo e ele não está salvo neste computador. Conecte-se à internet e tente de novo.",
    ),
    (r".*Invalid model size '?([\w\.\-]+)'?.*", r"Tamanho de modelo inválido: '\1'."),
    (r".*CUDA out of memory.*|.*out of memory.*", "A placa de vídeo ficou sem memória."),
    (r".*(Could not locate cudnn|cublas.*not found|CUDA.*(failed|unavailable|driver)).*", "As bibliotecas da placa de vídeo (CUDA) não estão disponíveis; o programa usará o processador."),
    (r".*Library .*not found or could not be loaded.*", "Uma biblioteca necessária não foi encontrada ou não pôde ser carregada."),
    # PyMuPDF / Pillow / MarkItDown
    (r".*cannot open broken document.*|.*Failed to open file.*|.*FileDataError.*", "Não foi possível abrir o arquivo; ele pode estar corrompido."),
    (r".*document closed or encrypted.*|.*needs password.*|.*bad password.*", "O documento está protegido por senha."),
    (r".*Cannot open empty file.*|.*empty file.*", "O arquivo está vazio."),
    (r".*cannot identify image file.*", "Não foi possível reconhecer a imagem (arquivo corrompido ou formato não aceito)."),
    (r".*image file is truncated.*|.*broken data stream.*", "A imagem está incompleta ou corrompida."),
    (r".*decompression bomb.*", "A imagem é grande demais e foi recusada por segurança."),
    (r".*UnsupportedFormatException.*|.*are not supported.*", "O MarkItDown não aceita este formato de arquivo."),
    (r".*MissingDependencyException.*", "Falta uma biblioteca para converter este formato. Use \"Reparar instalação\" no Menu Iniciar."),
    (r".*FileConversionException.*|.*Could not convert.*", "O MarkItDown não conseguiu converter o arquivo."),
]

# Nomes de classes de excecao quando a excecao vem sem texto
_CLASSES: dict[str, str] = {
    "TimeoutError": "O tempo de espera esgotou.",
    "FileNotFoundError": "Arquivo ou pasta não encontrado.",
    "PermissionError": "Permissão negada.",
    "MemoryError": "Memória insuficiente para esta operação.",
    "KeyboardInterrupt": "Operação interrompida.",
    "ConnectionError": "Sem conexão com a internet ou o servidor recusou a conexão.",
    "OSError": "Erro ao acessar o disco ou o sistema.",
    "RuntimeError": "Falha inesperada.",
    "ValueError": "Valor inválido.",
    "KeyError": "Dado ausente.",
}

# --------------------------------------------------------------------------
# Linhas de saida de programas externos (OCRmyPDF, pip etc.)
# --------------------------------------------------------------------------
_LINHAS: list[tuple[str, str]] = [
    (r"^Scanning contents\.*", "Examinando o conteúdo"),
    (r"^Start processing (\d+) pages? concurrently.*", r"Processando \1 página(s) ao mesmo tempo"),
    (r"^Postprocessing\.*", "Pós-processamento"),
    (r"^Recompressing JBIG2 objects.*", "Recomprimindo imagens JBIG2"),
    (r"^Linearizing\.*", "Otimizando o PDF para leitura na internet"),
    (r"^Deflating JPEGs.*", "Comprimindo imagens JPEG"),
    (r"^Optimizing images?.*", "Otimizando imagens"),
    (r"^Image optimization ratio: ([\d\.]+) Total file size ratio: ([\d\.]+)",
     r"Taxa de otimização das imagens: \1 · Taxa do tamanho total do arquivo: \2"),
    (r"^Output file is a PDF/A-(\w+) \(as expected\)", r"O arquivo de saída é um PDF/A-\1, como esperado."),
    (r"^Output file is a (.*) \(not (.*)\)", r"O arquivo de saída é \1 (não \2)."),
    (r"^Output file is a (.*)", r"O arquivo de saída é \1."),
    (r"^Input file is a PDF/A-(\w+)(.*)", r"O arquivo de entrada é um PDF/A-\1."),
    (r"^Input file is not a PDF/A.*", "O arquivo de entrada não é um PDF/A."),
    (r"^(\d+) page(s)? already has text.*", "As páginas já têm texto."),
    (r"^Page already has text!? ?-? ?skipping.*", "A página já tem texto; foi ignorada."),
    (r"^Page already has text.*", "A página já tem texto."),
    (r"^Using Tesseract OCR (.*)", r"Usando o Tesseract OCR \1"),
    (r"^OCR\.\.\.?$", "Reconhecendo texto"),
    (r"^Using cached (.*)", r"Usando cache de \1"),
    (r"^Collecting (.*)", r"Coletando \1"),
    (r"^Downloading (.*)", r"Baixando \1"),
    (r"^Installing collected packages: (.*)", r"Instalando os pacotes: \1"),
    (r"^Successfully installed (.*)", r"Instalado com sucesso: \1"),
    (r"^Requirement already satisfied: (.*)", r"Já instalado: \1"),
    (r"^WARNING: (.*)", r"AVISO: \1"),
    (r"^ERROR: (.*)", r"ERRO: \1"),
    (r"^Building wheel for (.*)", r"Compilando \1"),
    (r"^Preparing metadata (.*)", r"Preparando \1"),
    (r"^Warning: (.*)", r"Aviso: \1"),
    (r"^Error: (.*)", r"Erro: \1"),
]

_COMPILADOS = [(re.compile(p, re.IGNORECASE | re.DOTALL), t) for p, t in _ERROS]
_LINHAS_COMP = [(re.compile(p, re.IGNORECASE), t) for p, t in _LINHAS]

# Palavras que denunciam ingles x portugues (para decidir se um texto
# desconhecido precisa de uma frase em portugues antes do detalhe tecnico).
_EN = re.compile(
    r"\b(the|is|are|was|were|not|cannot|can't|couldn't|unable|failed|failure|error|invalid|expected|"
    r"missing|found|file|files|no such|object|attribute|unexpected|must|permission|denied|exceeded|"
    r"connection|returned|exit|status|module|argument|arguments|got|with|from|while|during|could|"
    r"does|did|has|have|cannot|exist|exists|unknown|unsupported|empty|timed|out|of|to|for|in|on|at|"
    r"an|a|or|and|it|its|please|try|again|using|used|open|read|write|reading|writing|opening)\b",
    re.IGNORECASE,
)
_PT = re.compile(
    r"\b(não|nao|arquivo|arquivos|falha|erro|inválido|invalido|pasta|deve|ainda|está|esta|foi|para|com|"
    r"uma|um|do|da|dos|das|em|ou|ao|aos|ser|sem|há|já|este|esta|essa|esse|nenhum|nenhuma|tente|tentar|"
    r"escolha|selecione|informe|aguarde|modelo|página|páginas|conversão|transcrição|instalação)\b",
    re.IGNORECASE,
)
_ACENTO = re.compile(r"[áàâãéêíóôõúçÁÀÂÃÉÊÍÓÔÕÚÇ]")


def parece_ingles(texto: str) -> bool:
    """True se o texto parece estar em ingles (e nao em portugues)."""
    if not texto or _ACENTO.search(texto):
        return False
    palavras = re.findall(r"[A-Za-z']+", texto)
    if len(palavras) < 2:
        return bool(re.search(r"(Error|Exception|Failed|Warning)$", texto))
    ingles = len(_EN.findall(texto))
    portugues = len(_PT.findall(texto))
    return ingles > portugues


def _aplicar(texto: str) -> str | None:
    for padrao, modelo in _COMPILADOS:
        m = padrao.fullmatch(texto.strip()) or padrao.match(texto.strip())
        if m:
            try:
                return m.expand(modelo)
            except (re.error, IndexError):
                return modelo
    return None


def erro_usuario(erro, acao: str = "concluir a operação") -> str:
    """Mensagem de erro em portugues para exibir ao usuario.

    Aceita uma excecao ou um texto. Erro conhecido vira frase pronta; texto ja
    em portugues passa intacto; texto desconhecido em ingles vira
    "Algo deu errado ao <acao>. Detalhe técnico: <texto original>".
    """
    if isinstance(erro, BaseException):
        texto = str(erro).strip()
        if not texto:
            return _CLASSES.get(erro.__class__.__name__) or f"Algo deu errado ao {acao}. Detalhe técnico: {erro.__class__.__name__}"
    else:
        texto = str(erro or "").strip()
        if not texto:
            return f"Algo deu errado ao {acao}."
    # Frases multilinha (ex.: saida do ffmpeg): traduz pelo conjunto
    traduzido = _aplicar(texto)
    if traduzido is not None:
        return traduzido
    if not parece_ingles(texto):
        return texto
    return f"Algo deu errado ao {acao}. Detalhe técnico: {texto}"


def traduzir_linha(linha: str) -> str:
    """Traduz uma linha de saida de programa externo; desconhecida volta igual."""
    base = linha.strip()
    if not base:
        return linha
    for padrao, modelo in _LINHAS_COMP:
        m = padrao.match(base)
        if m:
            try:
                return m.expand(modelo)
            except (re.error, IndexError):
                return linha
    ptbr = _aplicar(base) if parece_ingles(base) else None
    return ptbr if ptbr is not None else linha
