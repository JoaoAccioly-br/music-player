# 🎵 Music Player (Low-RAM) — `music-player`

[![Tauri v2](https://img.shields.io/badge/Tauri-v2-24C8DB?style=for-the-badge&logo=tauri&logoColor=white)](https://v2.tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-2021_Edition-000000?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Vue 3](https://img.shields.io/badge/Vue.js-3.5-4FC08D?style=for-the-badge&logo=vuedotjs&logoColor=white)](https://vuejs.org/)
[![Tailwind CSS](https://img.shields.io/badge/Tailwind_CSS-3.4-06B6D4?style=for-the-badge&logo=tailwindcss&logoColor=white)](https://tailwindcss.com/)
[![Multiplataforma](https://img.shields.io/badge/Desktop-Linux_%7C_Windows_%7C_macOS-FCC624?style=for-the-badge&logo=linux&logoColor=black)](https://github.com/JoaoAccioly-br/music-player/releases)

> **Player de música desktop ultra-leve e de baixo consumo de RAM (*Low-RAM*) construído com Tauri v2, Rust e Vue 3, integrado à API do YouTube Music (Innertube) e projetado com streaming progressivo em memória, letras sincronizadas (LRC) e modo *Zero-WebView* em segundo plano.**

---

## 📥 Download e Instalação (Linux, Windows e macOS)

Os instaladores prontos são compilados diretamente a partir do código-fonte público pelo GitHub Actions e publicados na aba **[Releases](https://github.com/JoaoAccioly-br/music-player/releases)**:

| Sistema | Formatos disponíveis | Como instalar / executar |
| :--- | :--- | :--- |
| **Linux** | `.AppImage`, `.deb`, `.rpm` | • **AppImage:** `chmod +x *.AppImage && ./YouTube*.AppImage`<br>• **Debian/Ubuntu/Mint/Pop!\_OS:** `sudo apt install ./arquivo.deb`<br>• **Fedora/openSUSE/RHEL:** `sudo dnf install ./arquivo.rpm` |
| **Windows** | `.exe` (NSIS), `.msi` | Execute o instalador normalmente (Windows 10/11). |
| **macOS** | `.dmg` (Apple Silicon) | Abra o `.dmg` e arraste o app para **Aplicativos**. |

> **⚠️ Avisos de Segurança do Sistema (SmartScreen / Gatekeeper):**
> Como este é um projeto open-source sem certificado pago de assinatura digital (*Code Signing*), o **Windows Defender SmartScreen** (*"O Windows protegeu o computador"*) e o **Gatekeeper do macOS** (*"desenvolvedor não verificado"*) podem exibir um alerta na primeira abertura:
> - **Windows:** clique em **Mais informações → Executar assim mesmo**.
> - **macOS:** vá em **Ajustes do Sistema → Privacidade e Segurança → Abrir Mesmo Assim** (ou `xattr -cr /Applications/YouTube\ Music\ \(Low-RAM\).app`).
> - Todos os binários oficiais são gerados de forma transparente via [`.github/workflows/release.yml`](.github/workflows/release.yml), diretamente do repositório público.

---

## 💡 Por que este projeto existe? (Electron vs. Tauri + Rust)

Clientes tradicionais de streaming de música para desktop geralmente são empacotados com **Electron** (que embute uma instância completa do Chromium + Node.js) ou abertos diretamente em uma aba do navegador. Manter um navegador inteiro aberto apenas para ouvir música drena centenas de megabytes de RAM, consome ciclos de CPU/GPU continuamente e realiza escritas constantes de cache em disco.

O **`music-player`** resolve esse problema com uma arquitetura enxuta: toda a inteligência do player — estado da fila, persistência de sessão, autenticação, integração com a API Innertube do YouTube Music, busca de letras, **pré-carregamento (prefetch) da próxima faixa** e **decodificação/reprodução de áudio em streaming progressivo** — vive **100% no backend nativo em Rust**. A interface gráfica (WebView) funciona apenas como uma camada visual leve e descartável.

### 📊 Comparativo Real de Consumo de Recursos

| Métrica / Cenário | Clientes Electron / Navegador | `music-player` (Janela Aberta) | `music-player` (**System Tray / Zero-WebView**) |
| :--- | :---: | :---: | :---: |
| **Consumo Médio de RAM** | ~400 MB – 850 MB | ~60 MB – 90 MB | **~15 MB – 30 MB** |
| **Processos de Navegador (WebView)** | Múltiplos processos Chromium ativos | 1 processo leve (WebKitGTK / WebView2 / WKWebView) | **0 (WebView 100% destruída)** |
| **Uso de VRAM / GPU em Background** | Constante (compositor Chromium) | Mínimo | **0 MB (Zero overhead de GPU)** |
| **Escrita de Áudio em Disco (SSD)** | Cache contínuo em disco | **0 bytes** (Stream em RAM) | **0 bytes** (Stream em RAM) |
| **Latência de Início e Troca de Faixa** | Variável | **< 500ms** (Chunks Progressivos + Prefetch RAM) | **< 500ms** (Chunks Progressivos + Prefetch RAM) |
| **Decodificação de Áudio** | Blink / Chromium Media Engine | Nativa em Rust (`rodio` + `symphonia`) | Nativa em Rust (`rodio` + `symphonia`) |

---

## ✨ Principais Funcionalidades

### 1. ⚡ Streaming Progressivo em Chunks (< 500ms de Latência) e Zero-Copy (`Arc`)
Em vez de esperar o download completo do arquivo de áudio terminar para começar a tocar, o motor em Rust utiliza o `ProgressiveAudioBuffer` com `ProgressiveReader` (`Read + Seek` sincronizado via `Condvar`):
- **Início Imediato (256 KB):** Assim que os primeiros **256 KB** chegam na memória RAM, um parser nativo de MP4 DASH (`parse_mp4_sidx_metadata`) lê o átomo `sidx` (Segment Index) apenas no cabeçalho inicial para extrair o tamanho total e a duração exata da faixa, iniciando a reprodução no `Symphonia`/`rodio` imediatamente enquanto uma task em Rust continua baixando os chunks restantes em background.
- **Compartilhamento Zero-Copy:** O buffer é compartilhado via `Arc<ProgressiveShared>`, eliminando cópias desnecessárias na memória durante a decodificação e o `Seek`.
- **Zero Escrita de Mídia em SSD:** Nenhum arquivo de áudio é salvo no disco; todo o fluxo ocorre diretamente em memória RAM (limitado a 25 MB por faixa).

### 2. ⏭️ Pré-Carregamento Automático da Próxima Música (*Prefetch* na RAM)
Enquanto você ouve a música atual, o backend em Rust já prepara silenciosamente a próxima faixa da fila:
- **Transição Instantânea (*Cache Hit*):** Ao terminar a música atual ou clicar em *Próxima*, o áudio já está pré-carregado na RAM (`prefetched_audio`) e começa a tocar instantaneamente, engatilhando em seguida o prefetch da música `N + 2`.
- **Prefetch Antecipado da Rádio Infinita:** Se você estiver na última música da fila com a Rádio ligada, o prefetch já consulta `/youtubei/v1/next` antecipadamente, anexa as novas recomendações na fila e baixa o áudio da primeira música recomendada antes mesmo da faixa atual acabar.
- **Coordenação Inteligente de Banda:** Se você clicar em uma música diferente que ainda não estava no cache, qualquer prefetch em andamento é cancelado instantaneamente (`prefetch_generation`) para priorizar 100% da rede na música escolhida.

### 3. 🚀 Modo Zero-WebView em Segundo Plano (*Zero-WebView Background Mode*)
Ao clicar no botão `X` para fechar a janela, o aplicativo **não interrompe a música** e **não apenas oculta a janela**: o Rust permite que o processo WebView seja **100% destruído** pelo sistema operacional, liberando toda a memória da interface gráfica.
- O motor de áudio nativo em Rust continua tocando via **ALSA / PipeWire / PulseAudio** (Linux), **WASAPI** (Windows) ou **CoreAudio** (macOS) sem qualquer interrupção.
- Você controla a reprodução pela **Bandeja do Sistema (System Tray)** (e no macOS o ícone do Dock é ocultado em modo acessório, reaparecendo ao reabrir a janela). Ao reabrir, o Vue 3 se reidrata instantaneamente a partir do estado mantido no Rust.

### 4. 🎨 Interface Unificada com Tema Claro/Escuro, Letras Sincronizadas e Tela Cheia
Interface moderna, limpa e responsiva com suporte nativo a **Tema Claro** e **Tema Escuro** (alternável pelo botão de Sol/Lua no topo) e paleta em gradientes pastel suaves:
- **Navegação Superior Unificada:** Header horizontal com pílula central (`Início`, `Explorar`, `Biblioteca`, `Curtidas`), barra de busca com filtros (`Tudo`, `Músicas`, `Vídeos`, `Álbuns`, `Artistas`, `Playlists`), alternador de tema, modo de economia de imagens e perfil da conta.
- **Player Flutuante em Pílula & Modo Tela Cheia (*Tocando Agora*):** Barra inferior flutuante com controles completos, progresso integrado, atalhos para **Fila (`A seguir`)**, **Letra**, **Relacionados** e modo expandido em **Tela Cheia**.
- **Letras Sincronizadas (LRCLIB + Fallback Innertube):** Prioriza letras com timestamps reais `.lrc` via API do **LRCLIB** (com suporte a `[offset:ms]`, múltiplas tags por linha e limpeza de sufixos como `(Official Video)`), acompanha o ritmo com **auto-scroll centralizado** e permite clicar em qualquer verso para pular para o segundo exato. Caso não haja letra sincronizada, faz fallback automático para o YouTube Music Innertube (`MPLYt...`) ou texto simples estimado.

### 5. 📻 Rádio Infinita (`/youtubei/v1/next`) e Telemetria Real do Hardware
- **Rádio Contínua:** Quando a fila chega ao fim, o backend consulta automaticamente `/youtubei/v1/next` na API Innertube do YouTube Music para continuar tocando músicas parecidas sem interrupção.
- **Telemetria Atômica (`AudioTelemetry`):** A thread de áudio atualiza em tempo real a posição exata de hardware (`sink.get_pos()`) e o estado do buffer diretamente via tipos atômicos (`AtomicU64` / `AtomicBool`), garantindo sincronia precisa de tempo (tick de 250ms) e avanço de faixa.

### 6. 🖼️ Modo Economia de RAM (Capas On/Off)
Deseja manter a janela aberta consumindo o mínimo absoluto de memória? Ative o botão de economia de imagens no topo para substituir todas as capas `<img>` por gradientes CSS vetoriais leves sem decodificação de texturas de imagem na WebView.

---

## 🔐 Segurança, Privacidade e Armazenamento Local

O aplicativo foi estruturado para reduzir a superfície de ataque em ambiente desktop e proteger os dados locais de cada usuário:

- **Armazenamento local isolado por sistema operacional:**
  - **Linux:** `~/.local/share/com.ytmusic.rust/` (ou `$XDG_DATA_HOME/com.ytmusic.rust/`)
  - **Windows:** `%LOCALAPPDATA%\com.ytmusic.rust\`
  - **macOS:** `~/Library/Application Support/com.ytmusic.rust/`
  - No Linux e macOS, o diretório é criado com permissão `0700` e os arquivos sensíveis (`auth.json`, `player_session.json` e cookies temporários) são gravados atomicamente já no modo `0600` (acessíveis apenas pelo próprio usuário).
- **Sobre cookies da conta Google (`auth.json`):**
  - Ao fazer login para acessar playlists e curtidas da sua conta, os cookies ficam salvos **apenas no seu computador** (nunca são enviados para terceiros além das requisições oficiais ao YouTube Music via `SAPISIDHASH`).
  - **Atenção:** atualmente `auth.json` é gravado em texto plano dentro da pasta de dados do usuário; nunca compartilhe esse arquivo.
  - A janela temporária de login (`google-login`) bloqueia navegação para fora da allowlist oficial (`accounts.google.com`, `*.google.com`, `music.youtube.com`, `*.youtube.com`).
- **Download e execução isolada do `yt-dlp`:**
  - Na primeira execução, se o binário não existir localmente, o app baixa a versão oficial do [`yt-dlp/yt-dlp`](https://github.com/yt-dlp/yt-dlp/releases) diretamente via HTTPS (seguindo redirecionamentos apenas para domínios oficiais do GitHub) e executa sempre com `--no-config` e `--no-exec`, validando o `video_id` antes da chamada.
  - *Nota no Windows:* alguns antivírus podem analisar na primeira vez o download automático do `yt-dlp.exe` para `%LOCALAPPDATA%\com.ytmusic.rust\bin\`.
- **Permissões mínimas no Tauri v2 e CSP:**
  - [`src-tauri/capabilities/default.json`](src-tauri/capabilities/default.json) expõe apenas `core:default`, `core:event:default` e `core:window:default` para a janela principal (sem APIs genéricas de filesystem/shell abertas ao JS).
  - [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json) aplica *Content Security Policy* (`script-src 'self'`, `connect-src` restrito a YouTube e LRCLIB) e valida URLs de stream/thumbnails (`https://`).

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
│  │ • Letras (LRCLIB     │     │ • Thread: ytm-native-   │   │ • Sessão e  │  │
│  │   + Innertube)       │     │   audio + AudioTelemetry│   │   Cookies   │  │
│  └──────────────────────┘     └────────────┬────────────┘   └─────────────┘  │
└────────────────────────────────────────────┼─────────────────────────────────┘
                                             ▼
                        ┌─────────────────────────────────────────┐
                        │  Streaming Progressivo Zero-Copy (RAM)  │
                        │  ALSA/PipeWire • WASAPI • CoreAudio     │
                        └─────────────────────────────────────────┘
```

---

## 📁 Estrutura de Pastas do Projeto

```text
music-player/
├── .github/workflows/
│   ├── tests.yml               # CI de testes (frontend + cargo test em Linux/Windows/macOS)
│   └── release.yml             # Build multiplataforma e publicação no GitHub Releases (tags v*)
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
│   │   └── player.ts           # Store Pinia: sincronização IPC com o Rust, temas, filtros e letras LRC
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
    ├── capabilities/
    │   └── default.json        # Permissões mínimas da janela principal no Tauri v2
    ├── Cargo.toml              # Dependências Rust e perfis otimizados (lto = "thin", strip = true)
    ├── tauri.conf.json         # Configurações da janela, CSP seguro e empacotamento multiplataforma
    ├── src/
    │   ├── main.rs             # Ponto de entrada do binário desktop
    │   ├── lib.rs              # Estado global (AppContainer), Prefetch RAM, comandos IPC e System Tray
    │   ├── audio.rs            # Streaming progressivo em chunks (256 KB), telemetria atômica e yt-dlp
    │   ├── innertube.rs        # Cliente da API YouTube Music (WEB_REMIX), filtros de busca e Letras LRC
    │   ├── auth.rs             # Gerador SHA-1 SAPISIDHASH, allowlist do login WebView e armazenamento 0600
    │   └── persistence.rs      # Resolução de pastas por SO e persistência de sessão em disco (0600)
    ├── tests/
    │   └── integration_tests.rs# Testes de integração offline via API pública da lib
    └── vendor/
        └── rodio-0.19.0/       # Patch local do rodio 0.19.0 para seek e duração exata no symphonia
```

---

## 🛠️ Como Compilar e Desenvolver Localmente

As instruções abaixo cobrem **Ubuntu / Debian / Zorin OS / Linux Mint / Pop!_OS** (no Windows e macOS, basta ter **Rust**, **Node.js 20+** e as ferramentas padrão do Tauri — WebView2/MSVC no Windows ou Xcode CLT no macOS).

### 1. Instalar Dependências de Sistema no Linux (Tauri v2 + ALSA)

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
*(No Ubuntu 22.04 ou mais antigo, substitua `libayatana-appindicator3-dev` por `libappindicator3-dev` se necessário.)*

### 2. Instalar Rust (`rustup`) e Node.js

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

> **💡 Nota sobre o `yt-dlp`:** Você **não** precisa instalar o `yt-dlp` manualmente. Na primeira execução, o app baixa automaticamente o binário oficial adequado ao seu sistema operacional na pasta local de dados do aplicativo.

### 3. Rodar em Modo Desenvolvimento

```bash
git clone https://github.com/JoaoAccioly-br/music-player.git
cd music-player
npm ci
npm run tauri dev
```

### 4. Executar Testes Automatizados e Verificação de Build

O backend possui **testes unitários** (`#[cfg(test)]` nos módulos) e **testes de integração offline** ([`src-tauri/tests/integration_tests.rs`](src-tauri/tests/integration_tests.rs)) cobrindo parsing LRC, extração Innertube com fixtures offline, streaming/seek no `ProgressiveAudioBuffer`, allowlist/cookies em `auth.rs` e persistência em disco:

```bash
# Testes unitários e de integração offline em Rust
cargo test --manifest-path src-tauri/Cargo.toml

# Typecheck + build de produção do frontend
npm run build
```

> O teste de ponta a ponta que acessa o YouTube ao vivo (`test_real_youtube_audio_extraction_and_decoding`) fica marcado com `#[ignore]` para não falhar em CI/offline. Para executá-lo manualmente com acesso à internet:
> ```bash
> cargo test --manifest-path src-tauri/Cargo.toml -- --ignored
> ```

### 5. Gerar Build Local ou Publicar Release via GitHub Actions

- **Build local:**
  ```bash
  npm run tauri build
  ```
- **Release pública automática (Linux, Windows e macOS via GitHub Actions):**
  1. Atualize a versão em [`package.json`](package.json), [`src-tauri/Cargo.toml`](src-tauri/Cargo.toml) e [`src-tauri/tauri.conf.json`](src-tauri/tauri.conf.json).
  2. Envie as alterações para a branch `main` (`git push origin main`).
  3. Crie e envie uma tag no formato `v*` (ex.: `git tag v0.1.0 && git push origin v0.1.0`).

> **🧊 Compilação Amigável de CPU/RAM (`.cargo/config.toml`):**
> Este repositório inclui o arquivo [`src-tauri/.cargo/config.toml`](src-tauri/.cargo/config.toml) configurado com `jobs = 2` e perfis enxutos no [`Cargo.toml`](src-tauri/Cargo.toml) (`debug = 0`, `strip = "debuginfo"`). Isso evita que a compilação do Rust sobrecarregue máquinas com pouca RAM.

---

## 📄 Licença

Desenvolvido com foco em leveza, baixo consumo de memória RAM e integração nativa com o YouTube Music. Sinta-se à vontade para abrir *issues*, sugerir melhorias ou enviar *pull requests*!
