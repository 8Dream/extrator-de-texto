; Instalador leve do Extrator de Texto.
; Copia o codigo para a pasta do usuario e executa install.ps1, que baixa
; Python, bibliotecas e Tesseract. Compile com installer\build.ps1.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

#define AppName "Extrator de Texto"
#define Root ".."

[Setup]
AppId={{3F3FEB2E-D62F-4C47-BD3A-84E1247889AE}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Amancio & Favarato Freire Advogados
; Pasta do usuario e gravavel: venv\ e dados\ ficam dentro dela, entao nao pode
; ser Program Files. Sem privilegios de administrador.
DefaultDirName={localappdata}\Programs\ExtratorDeTexto
DefaultGroupName=Extrator de Texto
PrivilegesRequired=lowest
DisableProgramGroupPage=yes
OutputDir={#Root}\dist
OutputBaseFilename=Extrator-de-Texto-Setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName={#AppName}
CloseApplications=yes
SetupIconFile={#Root}\src\assets\icone.ico
UninstallDisplayIcon={app}\src\assets\icone.ico

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "Criar atalho na area de trabalho"; Flags: unchecked

[Files]
Source: "{#Root}\src\*.py"; DestDir: "{app}\src"; Flags: ignoreversion
Source: "{#Root}\src\web\*"; DestDir: "{app}\src\web"; Excludes: "dev\*"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#Root}\src\assets\*"; DestDir: "{app}\src\assets"; Flags: ignoreversion
Source: "{#Root}\scripts\*.bat"; DestDir: "{app}\scripts"; Flags: ignoreversion
Source: "{#Root}\scripts\install.ps1"; DestDir: "{app}\scripts"; Flags: ignoreversion
Source: "{#Root}\requirements.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\VERSION.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Root}\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Extrator de Texto"; Filename: "{app}\venv\Scripts\pythonw.exe"; Parameters: """{app}\src\extrator.py"""; WorkingDir: "{app}"; IconFilename: "{app}\src\assets\icone.ico"
Name: "{autoprograms}\Extrator de Texto - Reparar instalacao"; Filename: "{app}\scripts\Instalar.bat"; WorkingDir: "{app}"; IconFilename: "{app}\src\assets\icone.ico"
Name: "{autodesktop}\Extrator de Texto"; Filename: "{app}\venv\Scripts\pythonw.exe"; Parameters: """{app}\src\extrator.py"""; WorkingDir: "{app}"; IconFilename: "{app}\src\assets\icone.ico"; Tasks: desktopicon

[UninstallDelete]
Type: filesandordirs; Name: "{app}\venv"
Type: filesandordirs; Name: "{app}\dados"
Type: filesandordirs; Name: "{app}\__pycache__"

[Code]
// As dependencias (Python, bibliotecas, Tesseract) sao instaladas pelo
// install.ps1 com a janela do PowerShell OCULTA. A saida aparece dentro do
// proprio instalador: a linha de estado mostra o passo atual e o botao
// "Mostrar detalhes" abre o log completo. Nenhuma caixa de mensagem: se algo
// falhar, o aviso fica na pagina final, com o log aberto.
type
  TMsg = record
    hwnd: Longint;
    message: Cardinal;
    wParam: Longint;
    lParam: Longint;
    time: Cardinal;
    ptX: Longint;
    ptY: Longint;
  end;

function PeekMessage(var lpMsg: TMsg; hWnd: Longint; wMsgFilterMin, wMsgFilterMax, wRemoveMsg: Cardinal): Longint;
  external 'PeekMessageW@user32.dll stdcall';
function GetTickCount: Cardinal;
  external 'GetTickCount@kernel32.dll stdcall';
function TranslateMessage(var lpMsg: TMsg): Longint;
  external 'TranslateMessage@user32.dll stdcall';
function DispatchMessage(var lpMsg: TMsg): Longint;
  external 'DispatchMessageW@user32.dll stdcall';

// Atende as mensagens do Windows (cliques, repintura) enquanto o instalador
// espera o install.ps1; sem isso o botao "Mostrar detalhes" nao responde.
procedure AppProcessMessage;
var
  Msg: TMsg;
begin
  while PeekMessage(Msg, 0, 0, 0, 1) <> 0 do
  begin
    TranslateMessage(Msg);
    DispatchMessage(Msg);
  end;
end;
var
  LogMemo: TNewMemo;
  DetailsButton: TNewButton;
  DepsFailed: Boolean;
  DepsExitCode: Integer;
  DepsLogPath: String;

// O Setup e 32 bits; num Windows 64 bits o powershell.exe comum seria o de
// 32 bits, em que ProgramFiles aponta para 'Program Files (x86)' e o
// Tesseract ja instalado nao era encontrado. Usa o PowerShell nativo.
function PowerShellPath: String;
begin
  if IsWin64 then
    Result := ExpandConstant('{sysnative}\WindowsPowerShell\v1.0\powershell.exe')
  else
    Result := ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe');
end;

procedure ToggleDetails(Sender: TObject);
begin
  LogMemo.Visible := not LogMemo.Visible;
  if LogMemo.Visible then
    DetailsButton.Caption := 'Ocultar detalhes'
  else
    DetailsButton.Caption := 'Mostrar detalhes';
end;

procedure PlaceLogControls(Page: TWinControl; Top, Left, Width: Integer);
begin
  DetailsButton.Parent := Page;
  DetailsButton.Left := Left;
  DetailsButton.Top := Top;
  LogMemo.Parent := Page;
  LogMemo.Left := Left;
  LogMemo.Top := DetailsButton.Top + DetailsButton.Height + ScaleY(6);
  LogMemo.Width := Width;
  LogMemo.Height := Page.ClientHeight - LogMemo.Top - ScaleY(4);
end;

procedure InitializeWizard;
begin
  DetailsButton := TNewButton.Create(WizardForm);
  DetailsButton.Width := ScaleX(140);
  DetailsButton.Height := ScaleY(24);
  DetailsButton.Caption := 'Mostrar detalhes';
  DetailsButton.OnClick := @ToggleDetails;
  DetailsButton.Visible := False;

  LogMemo := TNewMemo.Create(WizardForm);
  LogMemo.ReadOnly := True;
  LogMemo.ScrollBars := ssVertical;
  LogMemo.WordWrap := False;
  LogMemo.Font.Name := 'Consolas';
  LogMemo.Font.Size := 8;
  LogMemo.Visible := False;

  PlaceLogControls(WizardForm.InstallingPage,
    WizardForm.ProgressGauge.Top + WizardForm.ProgressGauge.Height + ScaleY(10),
    WizardForm.ProgressGauge.Left, WizardForm.ProgressGauge.Width);
  // Clicar na linha de estado tambem abre/fecha o log.
  WizardForm.FilenameLabel.OnClick := @ToggleDetails;
  WizardForm.StatusLabel.OnClick := @ToggleDetails;
end;

var
  LinesRead: Integer;

procedure AddInstallLine(const S: String);
var
  Line: String;
begin
  Line := TrimRight(S);
  Log('install.ps1: ' + Line);
  LogMemo.Lines.Add(Line);
  // Rola o log para a ultima linha (EM_LINESCROLL).
  SendMessage(LogMemo.Handle, $00B6, 0, LogMemo.Lines.Count);
  if Copy(Trim(Line), 1, 4) = '==> ' then
    WizardForm.StatusLabel.Caption := Copy(Trim(Line), 5, Length(Trim(Line)))
  else if Trim(Line) <> '' then
    WizardForm.FilenameLabel.Caption := Trim(Line);
end;

// Le do arquivo de log so as linhas ainda nao mostradas.
procedure ReadNewLines;
var
  Lines: TArrayOfString;
  I: Integer;
begin
  try
    if FileExists(DepsLogPath) and LoadStringsFromFile(DepsLogPath, Lines) then
    begin
      for I := LinesRead to GetArrayLength(Lines) - 1 do
        AddInstallLine(Lines[I]);
      if GetArrayLength(Lines) > LinesRead then
        LinesRead := GetArrayLength(Lines);
    end;
  except
    // arquivo em uso nesta volta: tenta na proxima
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  DonePath: String;
  Ok: Boolean;
  LastTick, Start: Cardinal;
  Codes: TArrayOfString;
  ResultCode: Integer;
begin
  // /SKIPDEPS=1 pula a instalacao das dependencias (uso em testes do instalador).
  if (CurStep = ssPostInstall) and (ExpandConstant('{param:SKIPDEPS|0}') <> '1') then
  begin
    DepsLogPath := ExpandConstant('{app}\dados\logs\instalacao.log');
    DonePath := ExpandConstant('{tmp}\extrator-done.txt');
    ForceDirectories(ExtractFileDir(DepsLogPath));
    DeleteFile(DepsLogPath);
    DeleteFile(DonePath);
    LinesRead := 0;
    WizardForm.StatusLabel.Caption := 'Instalando Python, bibliotecas e Tesseract (pode demorar)...';
    WizardForm.FilenameLabel.Caption := '';
    WizardForm.ProgressGauge.Style := npbstMarquee;
    DetailsButton.Visible := True;
    DepsExitCode := -1;
    try
      Ok := Exec(PowerShellPath,
        '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + ExpandConstant('{app}\scripts\install.ps1') +
        '" -LogFile "' + DepsLogPath + '" -DoneFile "' + DonePath + '"',
        ExpandConstant('{app}'), SW_HIDE, ewNoWait, ResultCode);
    except
      Ok := False;
      AddInstallLine('ERRO ao executar o install.ps1: ' + GetExceptionMessage);
    end;
    if Ok then
    begin
      Start := GetTickCount;
      LastTick := 0;
      // Espera o evento real (DoneFile); o teto de 60 min so existe para dar erro claro.
      while not FileExists(DonePath) do
      begin
        AppProcessMessage;
        if (GetTickCount - LastTick) >= 250 then
        begin
          LastTick := GetTickCount;
          ReadNewLines;
        end;
        if (GetTickCount - Start) > 3600000 then
        begin
          AddInstallLine('ERRO: o install.ps1 nao terminou em 60 minutos.');
          Ok := False;
          Break;
        end;
        Sleep(20);
      end;
      ReadNewLines;
      if Ok then
      begin
        if LoadStringsFromFile(DonePath, Codes) and (GetArrayLength(Codes) > 0) then
          DepsExitCode := StrToIntDef(Trim(Codes[0]), 1)
        else
          DepsExitCode := 1;
      end;
    end;
    WizardForm.ProgressGauge.Style := npbstNormal;
    DepsFailed := (not Ok) or (DepsExitCode <> 0);
  end;
end;
procedure CurPageChanged(CurPageID: Integer);
begin
  if (CurPageID = wpFinished) and DepsFailed then
  begin
    WizardForm.FinishedHeadingLabel.Caption := 'Instalacao incompleta';
    WizardForm.FinishedLabel.Caption :=
      'Os arquivos foram copiados, mas a instalacao das dependencias falhou (codigo ' +
      IntToStr(DepsExitCode) + '). Veja o motivo no log abaixo (salvo em dados\logs\instalacao.log).' + #13#10#13#10 +
      'Para tentar de novo, use "Extrator de Texto - Reparar instalacao" no Menu Iniciar.';
    WizardForm.FinishedLabel.AdjustHeight;
    PlaceLogControls(WizardForm.FinishedPage,
      WizardForm.FinishedLabel.Top + WizardForm.FinishedLabel.Height + ScaleY(10),
      WizardForm.FinishedLabel.Left, WizardForm.FinishedLabel.Width);
    DetailsButton.Visible := True;
    LogMemo.Visible := True;
    DetailsButton.Caption := 'Ocultar detalhes';
  end;
end;