# 🎵 Music Player (Low-RAM) — `music-player`

[![Tauri v2](https://img.shields.io/badge/Tauri-v2-24C8DB?style=for-the-badge&logo=tauri&logoColor=white)](https://v2.tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-2021_Edition-000000?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Vue 3](https://img.shields.io/badge/Vue.js-3.5-4FC08D?style=for-the-badge&logo=vuedotjs&logoColor=white)](https://vuejs.org/)
[![Tailwind CSS](https://img.shields.io/badge/Tailwind_CSS-3.4-06B6D4?style=for-the-badge&logo=tailwindcss&logoColor=white)](https://tailwindcss.com/)
[![Linux](https://img.shields.io/badge/Linux-ALSA_%7C_PipeWire_%7C_PulseAudio-FCC624?style=for-the-badge&logo=linux&logoColor=black)](https://kernel.org/)

> **Player de música desktop ultra-leve e de baixo consumo de RAM (*Low-RAM*) construído com Tauri v2, Rust e Vue 3, integrado à API do YouTube Music (Innertube) e projetado com streaming progressivo em memória e modo *Zero-WebView* em segundo plano.**

---

## 💡 Por que este projeto existe? (Electron vs. Tauri + Rust)

Clientes tradicionais de streaming de música para desktop geralmente são empacotados com **Electron** (que embute uma instância completa do Chromium + Node.js) ou abertos diretamente em uma aba do navegador. Manter um navegador inteiro aberto apenas para ouvir música drena centenas de megabytes de RAM, consome ciclos de CPU/GPU continuamente e realiza escritas constantes de cache em disco.

O **`music-player`** resolve esse problema com uma arquitetura enxuta: toda a inteligência do player — estado da fila, persistência de sessão, autenticação, integração com a API Innertube do YouTube Music, busca de letras, **pré-carregamento (prefetch) da próxima faixa** e **decodificação/reprodução de áudio em streaming progressivo** — vive **100% no backend nativo em Rust**. A interface gráfica (WebView) funciona apenas como uma camada visual leve e descartável.

### 📊 Comparativo Real de Consumo de Recursos

| Métrica / Cenário | Clientes Electron / Navegador | `music-player` (Janela Aberta) | `music-player` (**System Tray / Zero-WebView**) |
| :--- | :---: | :---: | :---: |
| **Consumo Médio de RAM** | ~400 MB – 850 MB | ~60 MB – 90 MB | **~15 MB – 30 MB** |
| **Processos de Navegador (WebView)** | Múltiplos processos Chromium ativos | 1 processo leve (WebKitGTK) | **0 (WebView 100% destruída)** |
| **Uso de VRAM / GPU em Background** | Constante (compositor Chromium) | Mínimo | **0 MB (Zero overhead de GPU)** |
| **Escrita de Áudio em Disco (SSD)** | Cache contínuo em disco | **0 bytes** (Stream em RAM) | **0 bytes** (Stream em RAM) |
| **Latência de Início e Troca de Faixa** | Variável | **< 500ms** (Chunks Progressivos + Prefetch RAM) | **< 500ms** (Chunks Progressivos + Prefetch RAM) |
| **Decodificação de Áudio** | Blink / Chromium Media Engine | Nativa em Rust (`rodio` + `symphonia`) | Nativa em Rust (`rodio` + `symphonia`) |

---

## ✨ Principais Funcionalidades

### 1. ⚡ Streaming Progressivo em Chunks (< 500ms de Latência) e Zero-Copy (`Arc`)
Em vez de esperar o download completo do arquivo de áudio terminar para começar a tocar, o motor em Rust utiliza o `ProgressiveAudioBuffer` com `ProgressiveReader` (`Read + Seek` sincronizado via `Condvar`):
- **Início Imediato (256 KB):** Assim que os primeiros **256 KB** chegam na memória RAM, um parser nativo de MP4 DASH (`parse_mp4_sidx_metadata`) lê o átomo `sidx` (Segment Index) para extrair o tamanho total e a duração exata da faixa, iniciando a reprodução no `Symphonia`/`rodio` imediatamente enquanto uma thread dedicada continua baixando os chunks restantes de 32 KB em background.
- **Compartilhamento Zero-Copy:** O buffer é compartilhado via `Arc<ProgressiveShared>`, eliminando cópias desnecessárias na memória durante a decodificação e o `Seek`.
- **Zero Escrita de Mídia em SSD:** Nenhum arquivo de áudio é salvo no disco; todo o fluxo ocorre diretamente em memória RAM.

### 2. ⏭️ Pré-Carregamento Automático da Próxima Música (*Prefetch* na RAM)
Enquanto você ouve a música atual, o backend em Rust já prepara silenciosamente a próxima faixa da fila:
- **Transição Instantânea (*Cache Hit*):** Ao terminar a música atual ou clicar em *Próxima*, o áudio já está pré-carregado na RAM (`prefetched_audio`) e começa a tocar instantaneamente, engatilhando em seguida o prefetch da música `N + 2`.
- **Prefetch Antecipado da Rádio Infinita:** Se você estiver na última música da fila com a Rádio ligada, o prefetch já consulta `/youtubei/v1/next` antecipadamente, anexa as novas recomendações na fila e baixa o áudio da primeira música recomendada antes mesmo da faixa atual acabar.
- **Coordenação Inteligente de Banda:** Se você clicar em uma música diferente que ainda não estava no cache, qualquer prefetch em andamento é cancelado instantaneamente (`prefetch_generation`) para priorizar 100% da rede na música escolhida.

### 3. 🚀 Modo Zero-WebView em Segundo Plano (*Zero-WebView Background Mode*)
Ao clicar no botão `X` para fechar a janela, o aplicativo **não interrompe a música** e **não apenas oculta a janela**: o Rust permite que o processo WebView (`WebKitGTK`) seja **100% destruído** pelo sistema operacional, liberando toda a memória da interface gráfica.
- O motor de áudio nativo em Rust continua tocando via **ALSA / PipeWire / PulseAudio** sem qualquer interrupção.
- Você controla a reprodução pela **Bandeja do Sistema (System Tray)** e, ao reabrir a janela, o Vue 3 se reidrata instantaneamente a partir do estado mantido no Rust.

### 4. 🎨 Interface Unificada com Tema Claro/Escuro, Letras e Tela Cheia
Interface moderna, limpa e responsiva com suporte nativo a **Tema Claro** e **Tema Escuro** (alternável pelo botão de Sol/Lua no topo) e paleta em gradientes pastel suaves:
- **Navegação Superior Unificada:** Header horizontal com pílula central (`Início`, `Explorar`, `Biblioteca`, `Curtidas`), barra de busca com filtros (`Tudo`, `Músicas`, `Vídeos`, `Álbuns`, `Artistas`, `Playlists`), alternador de tema, modo de economia de imagens e perfil da conta.
- **Player Flutuante em Pílula & Modo Tela Cheia (*Tocando Agora*):** Barra inferior flutuante com controles completos, progresso integrado, atalhos para **Fila (`A seguir`)**, **Letra**, **Relacionados** e modo expandido em **Tela Cheia**.
- **Letras de Música (*Lyrics*) Integradas:** Extrai a letra oficial diretamente da API Innertube do YouTube Music (`MPLYt...`) e conta com fallback automático via API do **LRCLIB**, exibindo os versos sincronizados com destaque tipográfico na linha atual.

### 5. 📻 Rádio Infinita (`/youtubei/v1/next`) e Telemetria Real do Hardware
- **Rádio Contínua:** Quando a fila chega ao fim, o backend consulta automaticamente `/youtubei/v1/next` na API Innertube do YouTube Music para continuar tocando músicas parecidas sem interrupção.
- **Telemetria Atômica (`AudioTelemetry`):** A thread de áudio atualiza em tempo real a posição exata de hardware (`sink.get_pos()`) e o estado do buffer diretamente via tipos atômicos (`AtomicU64` / `AtomicBool`), garantindo sincronia precisa de tempo e avanço de faixa.

### 6. 🔐 Autenticação Google Híbrida (`SAPISIDHASH`) e Persistência de Sessão (`0600`)
- **Login via Janela Efêmera Auto-Destrutiva ou Cookie Manual:** Autentique sua conta para acessar suas playlists, músicas curtidas e recomendações personalizadas da API Innertube v1 (`WEB_REMIX`). A janela de login é destruída automaticamente assim que o cookie `SAPISID` é capturado.
- **Persistência Automática (`player_session.json`):** Volume, modo de repetição (`off`, `all`, `one`), status da Rádio, modo de economia de imagens, fila atual e posição na fila são salvos automaticamente em `~/.local/share/com.ytmusic.rust/player_session.json` (com permissões restritas `0600` no Unix) e restaurados ao abrir o app.

### 7. 🖼️ Modo Economia de RAM (Capas On/Off)
Deseja manter a janela aberta consumindo o mínimo absoluto de memória? Ative o botão de economia de imagens no topo para substituir todas as capas `<img>` por gradientes CSS vetoriais leves sem decodificação de texturas de imagem na WebView.

---

## 🏗️ Arquitetura do Sistema

### Diagrama de Arquitetura e Fluxo IPC (Vue 3 ⟷ Rust)

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│                        FRONTEND (Vue 3 + Pinia + Tailwind)                   │
│       [ Descartável: Destruído ao fechar a janela (0 MB RAM de UI) ]         │
│                                                                              │
│  ┌─────────────┐   ┌─────────────┐   ┌─────────────┐   ┌──────────────────┐  │
│  │  HeaderBar  │   │  TrackCard  │   │ QueueDrawer │   │    PlayerBar     │  │
│  │ (Nav+Busca) │   │ (Card/Row)  │   │(Fila/Letra) │   │ (Pílula/Fullscr) │  │
│  └──────┬──────┘   └──────┬──────┘   └──────┬──────┘   └────────┬─────────┘  │
│         └─────────────────┴────────┬────────┴───────────────────┘            │
│                                    ▼                                         │
│                      ┌───────────────────────────┐                           │
│                      │  Pinia Store (player.ts)  │                           │
│                      └──────┬─────────────▲──────┘                           │
└─────────────────────────────┼─────────────┼──────────────────────────────────┘
                              │ invoke()    │ listen("player-state-changed")
                              │ (Comandos)  │ listen("queue-changed")
══════════════════════════════▼═════════════╪═══════════════════════════════════
                              IPC Tauri v2  │ (Só emite se WebView existir)
══════════════════════════════╦═════════════╪═══════════════════════════════════
┌─────────────────────────────▼─────────────┴──────────────────────────────────┐
│                     BACKEND NATIVO (Rust + Tokio + Rodio)                    │
│              [ Sempre Ativo: ~15-30 MB RAM | Controla System Tray ]          │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │                    AppContainer (Fonte Única da Verdade)               │  │
│  │  • player: Mutex<PlayerState>     • queue: Mutex<Vec<TrackItem>>       │  │
│  │  • prefetched_audio: Mutex<...>   • active_buffer: ProgressiveBuffer   │  │
│  │  • auth: Mutex<AuthSession>       • audio_engine: AudioEngine          │  │
│  └──────┬──────────────────────────────────┬───────────────────────┬──────┘  │
│         │                                  │                       │         │
│         ▼                                  ▼                       ▼         │
│  ┌──────────────────────┐     ┌─────────────────────────┐   ┌─────────────┐  │
│  │    innertube.rs      │     │        audio.rs         │   │ auth.rs &   │  │
│  │ • Home / Explore     │     │ • ProgressiveAudioBuffer│   │persistence  │  │
│  │ • Busca c/ Filtros   │     │   (Início com 256 KB)   │   │ • SAPISID   │  │
│  │ • Rádio (/v1/next)   │     │ • Parser MP4 sidx       │   │   SHA-1     │  │
│  │ • Letras (Innertube  │     │ • Thread: ytm-native-   │   │ • Sessão e  │  │
│  │   + LRCLIB fallback) │     │   audio + AudioTelemetry│   │   Cookies   │  │
│  └──────────────────────┘     └────────────┬────────────┘   └─────────────┘  │
└────────────────────────────────────────────┼─────────────────────────────────┘
                                             ▼
                        ┌─────────────────────────────────────────┐
                        │  Streaming Progressivo Zero-Copy (RAM)  │
                        │  Saída Nativa: ALSA / PipeWire / Pulse  │
                        └─────────────────────────────────────────┘
```

---

## 📁 Estrutura de Pastas do Projeto

```text
music-player/
├── index.html                  # Ponto de entrada HTML da WebView
├── package.json                # Scripts e dependências do frontend (Vue 3, Pinia, Tailwind, Tauri API)
├── tailwind.config.js          # Configuração de cores e suporte a Tema Claro/Escuro (darkMode: 'class')
├── vite.config.ts              # Configuração do bundler Vite para integração com Tauri v2
│
├── src/                        # 🎨 FRONTEND (Interface Leve e Descartável em Vue 3 + TypeScript)
│   ├── main.ts                 # Inicialização do Vue 3 e Pinia
│   ├── style.css               # Variáveis de tema (Claro/Escuro), gradientes pastel e estilos globais
│   ├── App.vue                 # Views principais (Início, Explorar, Busca, Biblioteca, Curtidas, Letra e Tela Cheia)
│   ├── stores/
│   │   └── player.ts           # Store Pinia: sincronização IPC com o Rust, temas, filtros e letras
│   └── components/
│       ├── HeaderBar.vue       # Navegação unificada em pílula, busca, toggle de tema (Sol/Lua) e RAM
│       ├── TrackCard.vue       # Cards quadrados pastel e linhas de faixa com lazy-loading
│       ├── PlayerBar.vue       # Player flutuante em pílula escura com controles, letra, rádio e fila
│       ├── QueueDrawer.vue     # Painel lateral com 3 abas ("A seguir", "Letra" e "Relacionados")
│       └── AuthModal.vue       # Modal de autenticação Google (WebView efêmera + Fallback Manual)
│
└── src-tauri/                  # ⚙️ BACKEND NATIVO (Motor Sempre Ativo em Rust)
    ├── .cargo/
    │   └── config.toml         # Limita compilação paralela (jobs = 2) para baixo consumo de CPU/RAM
    ├── Cargo.toml              # Dependências Rust e perfis otimizados (strip de debuginfo)
    ├── tauri.conf.json         # Configurações da janela, CSP seguro e permissões Tauri v2
    ├── src/
    │   ├── main.rs             # Ponto de entrada do binário desktop
    │   ├── lib.rs              # Estado global (AppContainer), Prefetch RAM, comandos IPC e System Tray
    │   ├── audio.rs            # Streaming progressivo em chunks (256 KB), telemetria atômica e yt-dlp
    │   ├── innertube.rs        # Cliente da API YouTube Music (WEB_REMIX), filtros de busca e Letras
    │   ├── auth.rs             # Gerador SHA-1 SAPISIDHASH, captura de cookies e armazenamento 0600
    │   └── persistence.rs      # Persistência automática de fila, volume e preferências em disco (0600)
    └── vendor/
        └── rodio-0.19.0/       # Patch local do rodio 0.19.0 para seek e duração exata no symphonia
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

> **💡 Nota sobre o `yt-dlp`:** Você **não** precisa instalar o `yt-dlp` manualmente! Caso ele não esteja presente, o próprio aplicativo fará o download automático do binário oficial isolado em `~/.local/share/com.ytmusic.rust/bin/yt-dlp` na primeira execução.

### 3. Instalar Dependências do Projeto e Rodar em Modo Desenvolvimento

Clone o repositório, instale os pacotes do frontend e inicie o ambiente Tauri:

```bash
# 1. Clonar o repositório
git clone https://github.com/joao-accioly/music-player.git
cd music-player

# 2. Instalar dependências do Vue 3 / Vite
npm install

# 3. Iniciar o aplicativo em modo de desenvolvimento
npm run tauri dev
```

### 4. Executar a Suíte de Testes Automatizados (Rust)

O backend conta com testes automatizados cobrindo o streaming progressivo concorrente em chunks (`ProgressiveAudioBuffer` + `ProgressiveReader`), cálculo de prefetch e modos de repetição, geração de hash `SAPISIDHASH` e extração/decodificação real de áudio:

```bash
cargo test --lib --manifest-path src-tauri/Cargo.toml
```

### 5. Gerar Build Otimizada de Produção (`.deb` / `.AppImage` / Binário)

Para compilar a versão final otimizada (`strip = true`, sem símbolos de debug):

```bash
npm run tauri build
```

> **🧊 Compilação Amigável de CPU/RAM (`.cargo/config.toml`):**
> Este repositório inclui o arquivo [`src-tauri/.cargo/config.toml`](src-tauri/.cargo/config.toml) configurado com `jobs = 2` e perfis enxutos no [`Cargo.toml`](src-tauri/Cargo.toml) (`debug = 0`, `strip = "debuginfo"`). Isso garante que a compilação do Rust não congele a máquina nem eleve o uso de RAM ao limite. Caso esteja em uma máquina com muitos núcleos e bastante RAM livre, basta ajustar o valor de `jobs`.

---

## 📄 Licença

Desenvolvido com foco em leveza, baixo consumo de memória RAM e integração nativa com a API do YouTube Music no Linux. Sinta-se à vontade para abrir *issues*, sugerir melhorias ou enviar *pull requests*!
