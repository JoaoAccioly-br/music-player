# 🎵 YouTube Music (Low-RAM) — `youtube-music-rust`

[![Tauri v2](https://img.shields.io/badge/Tauri-v2-24C8DB?style=for-the-badge&logo=tauri&logoColor=white)](https://v2.tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-2021_Edition-000000?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Vue 3](https://img.shields.io/badge/Vue.js-3.5-4FC08D?style=for-the-badge&logo=vuedotjs&logoColor=white)](https://vuejs.org/)
[![Tailwind CSS](https://img.shields.io/badge/Tailwind_CSS-3.4-06B6D4?style=for-the-badge&logo=tailwindcss&logoColor=white)](https://tailwindcss.com/)
[![Linux](https://img.shields.io/badge/Linux-ALSA_%7C_PipeWire_%7C_PulseAudio-FCC624?style=for-the-badge&logo=linux&logoColor=black)](https://kernel.org/)

> **Cliente desktop ultra-leve do YouTube Music construído com Tauri v2, Rust e Vue 3, projetado especificamente para rodar em segundo plano com 0 MB de consumo de navegador (*Zero-WebView Background Mode*).**

---

## 🎮 Por que este projeto existe? (Electron vs. Tauri + Rust)

Clientes tradicionais do YouTube Music para desktop são empacotados com **Electron** (que embute uma instância completa do Chromium + Node.js) ou abertos diretamente em uma aba do navegador. Durante sessões de jogos competitivos ou títulos pesados, manter um navegador inteiro aberto apenas para tocar música drena centenas de megabytes de RAM, consome ciclos de GPU e causa *micro-stuttering*.

O **`youtube-music-rust`** resolve esse problema de forma radical: toda a inteligência do player — estado da fila, autenticação, busca na API Innertube e **decodificação/reprodução de áudio** — vive **100% no backend nativo em Rust**. A interface gráfica (WebView) é tratada apenas como um "controle remoto visual descartável".

### 📊 Comparativo Real de Consumo de Recursos

| Métrica / Cenário | Clientes Electron / Navegador | `youtube-music-rust` (Janela Aberta) | `youtube-music-rust` (**Modo Jogo / System Tray**) |
| :--- | :---: | :---: | :---: |
| **Consumo Médio de RAM** | ~400 MB – 850 MB | ~60 MB – 90 MB | **~15 MB – 30 MB** |
| **Processos de Navegador (WebView)** | Múltiplos processos Chromium ativos | 1 processo leve (WebKitGTK) | **0 (WebView 100% destruída)** |
| **Uso de VRAM / GPU em Background** | Constante (compositor Chromium) | Mínimo | **0 MB (Zero overhead de GPU)** |
| **Escrita Temporária em Disco (SSD)** | Cache contínuo em disco | **0 bytes** (Stream em RAM) | **0 bytes** (Stream em RAM) |
| **Decodificação de Áudio** | Blink / Chromium Media Engine | Nativa em Rust (`rodio` + `symphonia`) | Nativa em Rust (`rodio` + `symphonia`) |

---

## ✨ Principais Funcionalidades

### 1. 🚀 Modo Zero-WebView em Segundo Plano (*Zero-WebView Background Mode*)
Ao clicar no botão `X` para fechar a janela, o aplicativo **não encerra a música** e **não apenas oculta a janela**: o Rust intercepta o evento `WindowEvent::CloseRequested` e permite que o processo WebView (`WebKitGTK`) seja **100% destruído** pelo sistema operacional.
- O motor de áudio nativo em Rust continua tocando na placa de som sem qualquer interrupção.
- Você controla tudo pela **Bandeja do Sistema (System Tray)** (Play/Pause, Próxima, Anterior, Reabrir Interface ou Sair) e, ao reabrir a janela, o Vue 3 se reidrata instantaneamente a partir da fonte única da verdade no Rust (`get_full_state`).

### 2. 🔊 Engine de Áudio 100% Nativa em RAM (`rodio` + `symphonia`)
Esqueça tags `<audio>` do HTML5. A reprodução de áudio acontece diretamente no sistema de som do sistema operacional (**ALSA / PipeWire / PulseAudio**) através do crate `rodio` com decodificador `symphonia` (suporte nativo a containers MP4/ISOMP4, AAC, Vorbis e Opus):
- **Zero Escrita em SSD:** Os bytes do áudio são baixados diretamente para um buffer em memória (`Vec<u8>`) e entregues ao decodificador via `std::io::Cursor<Vec<u8>>`.
- **Descarte Imediato de Memória:** Assim que uma faixa termina ou você pula de música, o buffer anterior sofre `drop()` imediato, mantendo o uso de RAM estrito e previsível.
- **Seek Nativo Instantâneo:** Avance ou retroceda para qualquer segundo da música diretamente no `Sink` nativo (patch dedicado aplicado ao `symphonia_decoder` para liberar seek em streams AAC/M4A).

### 3. ⚡ Extração Oficial M4A (`itag 140`) com Auto-Bootstrap do `yt-dlp`
O YouTube protege as URLs de áudio com assinaturas dinâmicas (`signatureCipher`) e tokens anti-throttling (`n-param`). Para garantir velocidade máxima e zero falha de reprodução:
- **Auto-Bootstrap Transparente:** Na primeira execução, se o `yt-dlp` não estiver instalado no sistema, o aplicativo baixa automaticamente o binário oficial standalone para `~/.local/share/com.ytmusic.rust/bin/yt-dlp` e aplica permissão `0755`.
- **Stream Direto `itag 140` (M4A/AAC ~128kbps):** Extrai a URL assinada do stream de áudio puro mais leve e eficiente do YouTube Music, baixando com headers de navegador pareados (`User-Agent` idêntico na extração e no download) para evitar bloqueios HTTP `403 Forbidden`.
- **Cancelamento Instantâneo (`kill_on_drop` + `AtomicU64`):** Clicar rapidamente em várias músicas seguidas cancela na hora qualquer processo `yt-dlp` ou download HTTP anterior através de um contador atômico de geração (`PLAY_GENERATION`), evitando filas travadas ou músicas tocando sobrepostas.

### 4. 📻 Rádio Infinita / Autoplay Contínuo (`/youtubei/v1/next`)
Estava no meio de uma partida ranqueada e a sua fila de músicas acabou? O motor em Rust detecta automaticamente o fim da fila e consulta o endpoint `/youtubei/v1/next` da API Innertube do YouTube Music usando o `videoId` da última música tocada, enfileirando recomendações relacionadas de forma 100% autônoma — mesmo com a janela fechada!

### 5. 🔐 Autenticação Google Híbrida (`SAPISIDHASH` + Permissão `0600`)
Acesse suas playlists privadas, músicas curtidas e recomendações personalizadas da sua conta Google com segurança:
- **Login via Janela Temporária Auto-Destrutiva:** Abre uma WebView isolada em `music.youtube.com`, monitora os cookies autenticados (`SAPISID` / `__Secure-3PAPISID`), salva a sessão e **destrói a janela de login automaticamente** para não gastar RAM.
- **Fallback Manual de Cookie:** Caso prefira não abrir a janela de login, basta colar sua string de Cookie ou o valor `SAPISID` diretamente no modal.
- **Criptografia de Cabeçalho `SAPISIDHASH`:** Calcula dinamicamente o hash `SHA-1("{timestamp} {sapisid} https://music.youtube.com")` exigido pela API Innertube v1.
- **Armazenamento Seguro Local:** Credenciais salvas em `~/.local/share/com.ytmusic.rust/auth.json` com permissões estritas de proprietário no Linux (`chmod 0600`).

### 6. 🖼️ Modo Ultra-Economia (Sem Imagens)
Quer usar a janela aberta em um segundo monitor consumindo o mínimo absoluto de memória? Ative o botão **"Sem Imagens (Ultra RAM)"** na barra superior. Todas as capas de álbuns e miniaturas `<img>` são desmontadas do DOM e substituídas por placeholders vetoriais leves.

### 7. 🔀 Fila Completa (*Up Next*), Shuffle (Fisher-Yates) e Loop
- **Gaveta Lateral da Fila (*Up Next*):** Adicione músicas à fila, clique para pular diretamente para qualquer item, remova faixas individuais ou limpe a fila inteira.
- **Shuffle Real (Fisher-Yates):** Embaralhamento matemático imparcial da fila mantendo a música atual no topo, com restauração da ordem original ao desativar.
- **3 Modos de Repetição (Loop):** `Off` (aciona Rádio Infinita ao terminar a fila), `All` (reinicia a fila do começo) e `One` (repete a mesma faixa continuamente).

---

## 🏗️ Arquitetura do Sistema

### Diagrama de Arquitetura e Fluxo IPC (Vue 3 ⟷ Rust)

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│                        FRONTEND (Vue 3 + Pinia + Tailwind)                   │
│             [ Descartável: Destruído ao fechar a janela (0 MB RAM) ]         │
│                                                                              │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐   ┌──────────────────┐  │
│  │  HeaderBar  │   │  TrackCard  │   │ QueueDrawer │   │    PlayerBar     │  │
│  └──────┬──────┘   └──────┬──────┘   └──────┬──────┘   └────────┬─────────┘  │
│         └─────────────────┴────────┬────────┴───────────────────┘            │
│                                    ▼                                         │
│                      ┌───────────────────────────┐                           │
│                      │  Pinia Store (player.ts)  │                           │
│                      └──────┬─────────────▲──────┘                           │
└─────────────────────────────┼─────────────┼──────────────────────────────────┘
                              │ invoke()    │ listen("player-state-changed")
                              │ (Comandos)  │ listen("playback-progress")
══════════════════════════════▼═════════════╪═══════════════════════════════════
                              IPC Tauri v2  │ (Só emite se WebView existir)
══════════════════════════════╦═════════════╪═══════════════════════════════════
┌─────────────────────────────▼─────────────┴──────────────────────────────────┐
│                     BACKEND NATIVO (Rust + Tokio + Rodio)                    │
│              [ Sempre Ativo: ~15-30 MB RAM | Controla System Tray ]          │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │                    AppContainer (Fonte Única da Verdade)               │  │
│  │  • state: Mutex<PlayerState>        • queue: Mutex<Vec<TrackItem>>     │  │
│  │  • auth:  Mutex<AuthSession>        • audio: AudioController (mpsc)    │  │
│  └──────┬──────────────────────────────────┬───────────────────────┬──────┘  │
│         │                                  │                       │         │
│         ▼                                  ▼                       ▼         │
│  ┌──────────────────────┐     ┌─────────────────────────┐   ┌─────────────┐  │
│  │    innertube.rs      │     │        audio.rs         │   │   auth.rs   │  │
│  │ • Busca / Home / Lib │     │ • Thread: ytm-native-   │   │ • SAPISID   │  │
│  │ • Rádio (/v1/next)   │     │   audio (rodio Sink)    │   │   SHA-1     │  │
│  │ • Extrator yt-dlp    │     │ • Thread: ytm-progress- │   │ • auth.json │  │
│  │   (itag 140 M4A)     │     │   ticker (250ms poll)   │   │   (0600)    │  │
│  └──────────────────────┘     └────────────┬────────────┘   └─────────────┘  │
└────────────────────────────────────────────┼─────────────────────────────────┘
                                             ▼
                        ┌─────────────────────────────────────────┐
                        │  Buffer em RAM: Cursor<Vec<u8>>         │
                        │  Saída Nativa: ALSA / PipeWire / Pulse  │
                        └─────────────────────────────────────────┘
```

### Como o Estado e as Threads Funcionam no Rust?

1. **`AppContainer` (Fonte Única da Verdade):**
   Diferente de apps web comuns onde o estado mora no JavaScript, aqui toda a sessão vive na struct [`AppContainer`](src-tauri/src/lib.rs) gerenciada pelo Tauri:
   - `Mutex<PlayerState>`: Guarda a música atual, estado de reprodução (`is_playing`, `is_loading`), posição em segundos, volume, modo de loop (`Off`, `All`, `One`), status do shuffle, aba ativa e modo de economia de imagens.
   - `Mutex<Vec<TrackItem>>`: Fila de reprodução principal e backup da fila original (`original_queue`) para restaurar a ordem após o Shuffle.
   - `Mutex<AuthSession>`: Sessão autenticada do Google carregada do disco na inicialização.
   - `AtomicU64` (`PLAY_GENERATION`): Contador monotônico incrementado a cada troca de música para invalidar downloads/extrações antigas em voo.

2. **Thread Dedicada de Áudio (`ytm-native-audio`):**
   Como o stream de áudio do sistema (`rodio::OutputStream`) não é `Send`/`Sync` entre threads arbitrárias, o módulo [`audio.rs`](src-tauri/src/audio.rs) cria uma thread exclusiva do sistema operacional (`ytm-native-audio`). Ela mantém a conexão com o ALSA/PipeWire aberta e recebe comandos instantâneos (`PlayBytes`, `TogglePause`, `Seek`, `SetVolume`, `Stop`) através de um canal `std::sync::mpsc::channel` sem bloquear o runtime assíncrono.

3. **Thread de Progresso e Auto-Avanço (`ytm-progress-ticker`):**
   Uma segunda thread leve acorda a cada `250ms` para:
   - Atualizar o tempo decorrido da faixa no estado compartilhado e emitir o evento `playback-progress` para a barra de progresso do Vue (se a janela estiver aberta).
   - Detectar o exato instante em que o `Sink` nativo termina de tocar a música (`sink.empty()`) e disparar automaticamente a função `advance_track_Automatically()`, que avança para a próxima música da fila ou aciona a **Rádio Infinita** na API Innertube.

---

## 📁 Estrutura de Pastas do Projeto

```text
youtube-music-rust/
├── index.html                  # Ponto de entrada HTML da WebView
├── package.json                # Scripts e dependências do frontend (Vue 3, Pinia, Tailwind, Tauri API)
├── tailwind.config.js          # Configuração de cores e tema escuro estilo YouTube Music
├── vite.config.ts              # Configuração do bundler Vite para integração com Tauri v2
│
├── src/                        # 🎨 FRONTEND (Interface Descartável em Vue 3 + TypeScript)
│   ├── main.ts                 # Inicialização do Vue 3 e Pinia
│   ├── style.css               # Estilos globais Tailwind CSS e barras de rolagem customizadas
│   ├── App.vue                 # Layout principal, grid de músicas/playlists e atalhos de teclado
│   ├── stores/
│   │   └── player.ts           # Store Pinia: ponte IPC com o Rust (invoke + listen de eventos)
│   └── components/
│       ├── HeaderBar.vue       # Barra de busca, toggle "Sem Imagens (Ultra RAM)" e botão "Modo Jogo"
│       ├── SidebarNav.vue      # Navegação lateral (Início, Explorar, Biblioteca, Login e Status RAM)
│       ├── TrackCard.vue       # Card de música/playlist com botões de Play imediato e Adicionar à Fila
│       ├── PlayerBar.vue       # Controles inferiores de mídia, barra de seek, shuffle, loop e volume
│       ├── QueueDrawer.vue     # Gaveta lateral retrátil da fila de reprodução ("Up Next")
│       └── AuthModal.vue       # Modal de autenticação Google (Janela WebView + Fallback Manual)
│
└── src-tauri/                  # ⚙️ BACKEND NATIVO (Motor Sempre Ativo em Rust)
    ├── .cargo/
    │   └── config.toml         # Limita compilação paralela (jobs = 2) para não travar CPU/RAM
    ├── Cargo.toml              # Dependências Rust e perfis otimizados (strip de debuginfo)
    ├── tauri.conf.json         # Configurações da janela, identificadores e permissões Tauri v2
    ├── src/
    │   ├── main.rs             # Binário de entrada do aplicativo desktop
    │   ├── lib.rs              # Estado global (AppContainer), comandos IPC, System Tray e ciclo de janela
    │   ├── audio.rs            # Motor de áudio em RAM (rodio + symphonia), seek e auto-avanço
    │   ├── innertube.rs        # Cliente HTTP da API Innertube v1 + extrator yt-dlp c/ auto-bootstrap
    │   └── auth.rs             # Gerador SHA-1 SAPISIDHASH, captura de cookies e persistência 0600
    └── vendor/
        └── rodio-0.19.0/       # Patch local do rodio 0.19.0 habilitando seek nativo no symphonia_decoder
```

---

## 🛠️ Pré-requisitos e Como Rodar Localmente (Linux)

As instruções abaixo cobrem distribuições baseadas em **Ubuntu / Debian / Zorin OS / Linux Mint / Pop!_OS**.

### 1. Instalar Dependências de Sistema (Tauri v2 + ALSA)

No Linux, o Tauri v2 utiliza as bibliotecas do `WebKitGTK` para renderizar a janela (quando aberta) e o `libasound2-dev` (ALSA) para compilar a saída de áudio nativa do `rodio`:

```bash
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libasound2-dev \
  pkg-config
```

### 2. Instalar Rust (`rustup`) e Node.js

Caso ainda não possua o compilador Rust ou o Node.js (v18+) instalados:

```bash
# Instalar Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# Verificar versões
rustc --version
cargo --version
node --version
npm --version
```

> **💡 Nota sobre o `yt-dlp`:** Você **não** precisa instalar o `yt-dlp` manualmente! Caso ele não esteja no seu `PATH`, o próprio aplicativo fará o download automático do binário oficial isolado em `~/.local/share/com.ytmusic.rust/bin/yt-dlp` na primeira vez que você der play em uma música.

### 3. Instalar Dependências do Projeto e Rodar em Modo Desenvolvimento

Clone o repositório, instale os pacotes do frontend e inicie o ambiente Tauri:

```bash
# 1. Instalar dependências do Vue 3 / Vite
npm install

# 2. Iniciar o aplicativo em modo de desenvolvimento
npm run tauri dev
```

### 4. Executar a Suíte de Testes Automatizados (Rust)

O backend conta com testes unitários cobrindo geração de hash `SAPISIDHASH`, parsing de respostas da API Innertube (busca, biblioteca, rádio e playlists), gerenciamento de permissões `0600` e decodificação/seek de áudio em memória:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

### 5. Gerar Build Otimizada de Produção (`.deb` / `.AppImage` / Binário)

Para compilar a versão final super otimizada (`strip = true`, sem símbolos de debug):

```bash
npm run tauri build
```

Os pacotes instaláveis e o binário final serão gerados em:
- **Binário executável:** `src-tauri/target/release/youtube-music-rust`
- **Pacote Debian/Ubuntu (`.deb`):** `src-tauri/target/release/bundle/deb/`

> **🧊 Compilação Amigável de CPU/RAM (`.cargo/config.toml`):**
> Este repositório inclui o arquivo [`src-tauri/.cargo/config.toml`](src-tauri/.cargo/config.toml) configurado com `jobs = 2` e perfis enxutos no [`Cargo.toml`](src-tauri/Cargo.toml) (`debug = 0`, `strip = "debuginfo"`). Isso garante que a compilação do Rust nunca congele o seu notebook nem dispare a temperatura da CPU ao máximo. Caso esteja em uma workstation com muitos núcleos e muita RAM livre, você pode aumentar o valor de `jobs` conforme desejar.

---

## 🕹️ Dica Rápida de Uso para Gamers

1. Abra o aplicativo, pesquise sua música, álbum ou playlist favorita e dê **Play** (ou ative o **Shuffle**).
2. Clique no botão verde **"Modo Jogo"** no canto superior direito (ou simplesmente feche a janela no `X`).
3. A janela desaparecerá destruindo a WebView (`0 MB` de navegador), e a música continuará tocando perfeitamente pelo motor nativo em Rust!
4. Quando a fila terminar, a **Rádio Infinita** buscará novas músicas relacionadas sozinha. Para pausar, pular de faixa ou reabrir a interface, basta clicar no ícone do **YouTube Music** na bandeja do sistema (*System Tray*).

---

## 📄 Licença

Desenvolvido para a comunidade gamer e entusiastas de performance em Linux. Sinta-se à vontade para abrir *issues*, sugerir melhorias ou enviar *pull requests*!
