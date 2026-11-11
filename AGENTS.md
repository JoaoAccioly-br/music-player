# AGENTS.md — Documentação de Arquitetura e Orquestração (`youtube-music-rust`)

## 1. Visão Geral do Projeto

O **`youtube-music-rust`** é um cliente desktop de **YouTube Music** ultra-leve, projetado especificamente para rodar em segundo plano durante sessões de jogos sem consumir memória RAM ou ciclos de CPU/GPU desnecessários.

### Stack Tecnológica
- **Frontend (UI Modular sob demanda):**
  - **Vue 3** (Composition API com `<script setup lang="ts">` e componentes modulares em `src/components/`)
  - **TypeScript** (Tipagem estática ponta a ponta com os comandos do Rust)
  - **Tailwind CSS** (Estilização utilitária rápida em Dark Mode nativo `#030303`)
  - **Pinia** (Gerenciamento de estado reativo sincronizado com o backend Rust via `invoke` + `listen`)
  - **Lucide Vue Next** (Ícones vetoriais leves)
- **Backend (Core Persistente de Baixo Consumo):**
  - **Rust** (Segurança de memória, concorrência sem Garbage Collector e consumo mínimo de recursos)
  - **Tauri v2** (Framework de comunicação IPC entre o sistema operacional, System Tray e WebView)
  - **Rodio (Patched em `src-tauri/vendor/rodio-0.19.0`) + Symphonia** (Decodificação e reprodução de áudio 100% em Rust via ALSA/PulseAudio/PipeWire sem navegador, com correção de `ReadSeekSource::byte_len()` para suporte total a containers `isomp4` M4A/AAC em memória)
  - **`yt-dlp` Standalone Isolado (`~/.local/share/com.ytmusic.rust/bin/yt-dlp`)** (Extração 100% real de streams M4A/AAC `itag 140` direto para a RAM via `stdout`, resolvendo automaticamente assinaturas `n-challenge`, `signatureCipher` e `PoToken` do YouTube com auto-bootstrap em Rust)
  - **Reqwest + Tokio + SHA-1** (Cliente HTTP assíncrono com TLS em Rust puro para a API Innertube `WEB_REMIX` do YouTube Music, assinatura `SAPISIDHASH` e Rádio Infinita `/youtubei/v1/next`)

---

## 2. Arquitetura "Zero-WebView em Background", Engine de Áudio Nativa, Extração Real `yt-dlp`, Proteção de RAM/DBus & Rádio Infinita

Nos clientes tradicionais baseados em Electron (ou mesmo em apps Tauri convencionais que apenas ocultam a janela com `window.hide()`), o processo de renderização do navegador (Chromium / WebKitGTK / WebView2) permanece alocado na memória RAM mesmo quando o usuário fecha a janela para a bandeja do sistema (System Tray). Isso facilmente consome entre **300 MB e 800 MB de RAM** apenas mantendo o DOM, o motor JavaScript V8/JavaScriptCore e texturas de GPU vivos em segundo plano.

### Como nossa arquitetura resolve isso:
1. **Fonte Única da Verdade no Rust (`Mutex<PlayerState>`, `Mutex<Vec<TrackItem>>` e `Mutex<AuthSession>`):**
   - Todo o estado do reprodutor (música atual, artista, `video_id`, miniatura, fila de reprodução real da Innertube, `repeat_mode`, `autoplay_radio`, `low_ram_no_images`, estado `is_playing`, `position_secs`, `duration_secs`, `buffer_size_mb`, `audio_quality_label`, volume e sessão autenticada) vive **exclusivamente na memória gerenciada pelo Rust** através de `tauri::State<AppContainer>`.
   - O processo Rust em repouso ou reproduzindo áudio nativo consome apenas cerca de **15 MB a 30 MB de RAM** (incluindo o buffer de áudio da faixa atual em memória).
2. **Thread Dedicada de Áudio (`AudioEngine` com `rodio` + `symphonia` em `src-tauri/src/audio.rs` + Patch `ReadSeekSource`):**
   - Como o `rodio::OutputStream` conversa diretamente com o dispositivo de som do sistema operacional (ALSA/PipeWire no Linux) usando ponteiros nativos que não implementam `Send`/`Sync`, criamos uma **thread dedicada de áudio (`std::thread::spawn`)** no Rust.
   - Os comandos do Tauri e da System Tray enviam mensagens assíncronas via `std::sync::mpsc::Sender<AudioCommand>` (`LoadAndPlay`, `SetPlaying`, `SetVolume`, `Seek`, `Stop`) para essa thread.
   - O buffer de áudio da música atual vive em um `std::io::Cursor<Vec<u8>>` na RAM (~3 a 8 MB para uma faixa M4A/AAC de 128 kbps) sem gravar arquivos de música em disco/SSD. Assim que a música muda, o `Sink` e o `Vec<u8>` anteriores são descartados (`drop`), liberando a RAM imediatamente.
   - **Correção Crítica de Decodificação M4A (`src-tauri/vendor/rodio-0.19.0`):** No `rodio 0.19.0` original do crates.io, `ReadSeekSource::byte_len()` retornava `None` fixo, fazendo com que o demuxer `IsoMp4Reader` do `symphonia` falhasse com `SeekError(Unseekable)` ao abrir qualquer stream M4A (`itag 140`) do YouTube. Além disso, `SymphoniaDecoder::total_duration()` dividia `(1.0 / frac)` ao invés de multiplicar `frac * 1_000_000_000.0`. Corrigimos ambos em `src-tauri/vendor/rodio-0.19.0` via `[patch.crates-io]`, garantindo decodificação e `Seek` 100% nativos de M4A/AAC na RAM.
3. **Extração 100% Real de Áudio do YouTube via Binário `yt-dlp` Isolado (Zero Synth Fallback):**
   - **Auto-Bootstrap Isolado (`ensure_yt_dlp_binary`):** O aplicativo mantém um binário standalone oficial `yt-dlp_linux` isolado em `~/.local/share/com.ytmusic.rust/bin/yt-dlp` (com permissão `0755`). Caso o arquivo não exista no computador do usuário, o próprio Rust faz o download automático da release oficial do GitHub na primeira execução.
   - **Streaming Direto para a RAM (`-o -`):** Ao tocar qualquer faixa, o Rust invoca `yt-dlp --no-playlist -f "140/bestaudio[ext=m4a]/bestaudio" -o - --quiet --no-warnings --no-part --no-cache-dir` via `tokio::process::Command` e lê os bytes do container M4A/AAC diretamente do `stdout` para um `Vec<u8>` em RAM.
   - **Cancelamento Instantâneo de Downloads Concorrentes (`download_generation: Arc<AtomicU64>` + `kill_on_drop(true)`):** Se o usuário trocar de música rapidamente várias vezes seguidas, o contador atômico `download_generation` é incrementado e o processo `yt-dlp` da faixa anterior é imediatamente encerrado (`child.kill().await`), garantindo que nunca existam múltiplos processos `yt-dlp` concorrendo por CPU ou rede.
   - **Remoção Total do Sintetizador Falso:** O antigo gerador de acordes PCM (`generate_synth_wav_buffer`) foi completamente removido. O usuário ouve exclusivamente o stream real do YouTube decodificado por `rodio::Decoder` + `symphonia`; caso ocorra algum erro real de rede ou vídeo indisponível, o erro real é exibido em `PlayerState.error_message`.
4. **Otimização Anti-Spam do DBus / System Tray e Compilação de Baixo Consumo (`src-tauri/.cargo/config.toml`):**
   - **Deduplicação de Status da System Tray (`last_tray_text: Mutex<String>`):** No Linux (GNOME Shell / AppIndicator / Xwayland), chamar `status_item.set_text(...)` a cada 500ms inunda o barramento DBus e causa vazamento de memória no shell gráfico. Agora, `broadcast_state_change` compara o novo texto com `last_tray_text` e **só invoca o DBus quando o título ou estado (Tocando/Pausado/Carregando) realmente muda**. Além disso, nos ticks normais de 500ms (`start_progress_ticker`), se a janela `"main"` estiver fechada (Modo Zero-WebView), nenhum evento IPC ou chamada de Tray é disparado até que a música termine.
   - **Proteção de RAM e Temperatura na Compilação (`.cargo/config.toml` e `Cargo.toml`):** Configuramos `jobs = 2` em `src-tauri/.cargo/config.toml` e perfis `[profile.dev]` / `[profile.release]` enxutos (`debug = 0`, `strip = "debuginfo"`, `lto = false`, `codegen-units = 4`) para evitar picos de uso de memória pelo linker `rust-lld`/`cc` e superaquecimento em notebooks.
5. **Autenticação Segura Google (`SAPISIDHASH` + Persistência Unix `0600` em `src-tauri/src/auth.rs`):**
   - **Login Híbrido Resiliente:**
     1. **Janela Temporária de Login (`open_google_login_window`):** Instancia uma WebView secundária efêmera (`"google-login"`) com User-Agent desktop Firefox Linux para evitar bloqueios de navegador embutido. Uma task assíncrona monitora os cookies (`cookies_for_url`) e, assim que detecta o cookie `SAPISID` / `__Secure-3PAPISID` pós-login, salva a sessão e **destrói a janela de login imediatamente** para liberar 100% da RAM.
     2. **Fallback Manual de Cookie (`set_manual_cookie`):** Permite colar o cabeçalho `Cookie:` ou o valor do `SAPISID` diretamente caso o usuário prefira autenticar pelo navegador padrão.
   - **Assinatura Criptográfica `SAPISIDHASH` (`generate_sapisidhash`):** A cada requisição para `https://music.youtube.com/youtubei/v1/*`, o Rust gera dinamicamente o hash `SHA-1` de `"{timestamp} {sapisid} https://music.youtube.com"` e monta o cabeçalho `Authorization: SAPISIDHASH {ts}_{sha1} SAPISID1PHASH {ts}_{sha1} SAPISID3PHASH {ts}_{sha1}`.
   - **Persistência Segura (`0600`):** A sessão é salva em `~/.local/share/com.ytmusic.rust/auth.json` com permissão de diretório `0700` e arquivo `0600` (`PermissionsExt::from_mode(0o600)`), garantindo que apenas o usuário dono da conta no Linux possa ler o arquivo.
6. **Cliente Nativo YouTube Music Innertube & Rádio Infinita (`src-tauri/src/innertube.rs`):**
   - Comunica-se diretamente com a API interna `WEB_REMIX` (`clientId: "67"`) do YouTube Music em Rust puro:
     - `fetch_home` (`browseId: "FEmusic_home"`)
     - `search_music` (`/youtubei/v1/search`)
     - `fetch_liked_music` (`browseId: "VLLM"`)
     - `fetch_user_playlists` (`browseId: "FEmusic_liked_playlists"`)
     - `fetch_playlist_tracks` (`browseId: "VL..."`)
     - `fetch_radio_up_next` (`/youtubei/v1/next` — extrai `playlistPanelVideoRenderer` para alimentar a Rádio Infinita automaticamente quando a fila chega ao fim durante um jogo).
7. **Destruição Real da WebView ao Fechar (`X`), Recriação Instantânea & Modo Ultra-Economia (Sem Imagens):**
   - Quando o usuário clica no botão de fechar (`X`) da janela, a `WebviewWindow` é **completamente destruída**, encerrando o processo do motor de renderização HTML/CSS/JS e devolvendo 100% da memória RAM da interface para o sistema operacional e para o seu jogo.
   - Quando a janela está aberta, todas as capas usam `loading="lazy" decoding="async"`, e o usuário pode ativar o **Modo Ultra-Economia (`low_ram_no_images`)** para substituir imagens por gradientes CSS leves e evitar decodificação de texturas bitmap na WebView.
   - No loop principal do Tauri (`app.run`), interceptamos o evento `RunEvent::ExitRequested` e chamamos `api.prevent_exit()`, mantendo o binário Rust e a thread de áudio tocando música silenciosamente em background.

```
+-----------------------------------------------------------------------+
|                       PROCESSO PRINCIPAL (RUST)                       |
|                                                                       |
|  +-------------------------+       +-------------------------------+  |
|  |  AppContainer (Mutex)   | <---> |  System Tray (Menu Nativo OS) |  |
|  |  - PlayerState          |       |  - Deduplicado (Zero DBus Spam|  |
|  |  - Queue: Vec<TrackItem>|       |  - Play/Pause, Next, Prev     |  |
|  |  - download_generation  |       |  - Shuffle & Rádio Autoplay   |  |
|  |  - AuthSession (0600)   |       |  - Abrir Janela / Sair        |  |
|  +-------------------------+       +-------------------------------+  |
|        ^              |                           ^                   |
|        |              | mpsc::Sender              | Innertube + yt-dlp|
|        |              v                           v                   |
|        |     +-----------------------------------------------------+  |
|        |     |  THREAD DE ÁUDIO (rodio) + EXTRAÇÃO REAL (yt-dlp)   |  |
|        |     |  - ~/.local/share/com.ytmusic.rust/bin/yt-dlp (-o -)|  |
|        |     |  - Cursor<Vec<u8>> (M4A Real em RAM, 0 Synth Fake!) |  |
|        |     +-----------------------------------------------------+  |
|        |                                                              |
|        | IPC (invoke / emit apenas quando a janela "main" está aberta)|
|        v                                                              |
|  + - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - +  |
|  :  WEBVIEW MODULAR (Vue 3 + Pinia + Tailwind CSS)                 :  |
|  :  - HeaderBar, SidebarNav, TrackCard, QueueDrawer, PlayerBar     :  |
|  :  - DESTRUÍDA ao clicar no 'X' (0 MB de RAM de WebView em jogo!) :  |
|  + - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - - +  |
+-----------------------------------------------------------------------+
```

---

## 3. Regras de Orquestração do Projeto

Para garantir qualidade arquitetural, aprendizado contínuo e organização modular ao longo de todo o desenvolvimento, seguimos três diretrizes estritas:

1. **Agente Principal como Orquestrador e Professor:**
   - O Agente Principal atua exclusivamente no planejamento arquitetural, revisão detalhada do código escrito pelos sub-agentes e explicação didática (do zero, passo a passo) dos conceitos de **Vue 3 (Reatividade, Pinia, Composition API, Props/Emits)** e **Rust/Tauri (Ownership, Structs, Enums, Mutex, Channels, IPC, Ciclo de Vida)**.
2. **Implementação 100% via Sub-Agents:**
   - Todo código-fonte (seja no frontend Vue/TypeScript ou no backend Rust/Tauri), execução de comandos de compilação e criação de arquivos de configuração são delegados e implementados por **Sub-Agents** especializados.
3. **Alinhamento Contínuo e Documentação Viva:**
   - Antes de iniciar qualquer nova fase ou passo técnico, o Agente Principal sempre consulta o usuário com perguntas de alinhamento de design/arquitetura e mantém este arquivo `AGENTS.md` rigorosamente atualizado com o progresso e decisões tomadas.

---

## 4. Roadmap de Desenvolvimento (5 Fases + Hardening de Estabilidade e Áudio Real — 100% Concluído)

- **Fase 1: Setup Tauri v2 + Vue 3 + Tailwind CSS + Pinia + Ciclo de Vida da Janela (Destruir WebView no 'X') + System Tray Nativo** — `[CONCLUÍDO]`
  - Estruturação completa do projeto frontend e backend, `PlayerState` em Rust com `Mutex`, `TrayIconBuilder` dinâmico e ciclo Zero-WebView (`RunEvent::ExitRequested` + `api.prevent_exit()`).
- **Fase 2: Engine de Áudio 100% Nativa em Rust (`rodio` / `symphonia` + extração real de stream do YouTube sem vídeo)** — `[CONCLUÍDO]`
  - Thread dedicada de áudio (`src-tauri/src/audio.rs`) com `rodio` (patched em `src-tauri/vendor/rodio-0.19.0`) + `symphonia` (`isomp4`, `aac`, `mp3`, `vorbis`, `wav`) controlada via `mpsc::Sender<AudioCommand>`.
  - Buffer 100% em RAM (`Cursor<Vec<u8>>`) com descarte imediato ao trocar de faixa e medidor em tempo real (`buffer_size_mb`).
  - Extração de stream de áudio 100% real do YouTube (`audio/mp4` / `itag 140` ~128 kbps) via binário isolado `~/.local/share/com.ytmusic.rust/bin/yt-dlp` (com auto-bootstrap e cancelamento instantâneo de processos anteriores via `download_generation`).
  - Suporte completo a Play/Pause real, Volume real, Seek (`try_seek`) e telemetria de tempo (`position_secs` / `duration_secs`) a cada 500ms sem spam de DBus.
- **Fase 3: Autenticação Google (WebView temporária de login + extração de cookies `SAPISIDHASH`) & Cliente Innertube API em Rust (Home, Busca, Playlists, Liked Music)** — `[CONCLUÍDO]`
  - Módulo `src-tauri/src/auth.rs`: Janela secundária efêmera `"google-login"` com captura automática de cookies (`cookies_for_url`) e destruição imediata pós-login + Fallback manual de cookie `SAPISID` / `__Secure-3PAPISID`.
  - Geração dinâmica do cabeçalho `Authorization: SAPISIDHASH <ts>_<sha1>` usando `sha1` em Rust e persistência em `~/.local/share/com.ytmusic.rust/auth.json` com permissão Unix estrita `0600`.
  - Módulo `src-tauri/src/innertube.rs`: Cliente `WEB_REMIX` em Rust com parsers recursivos resilientes para Home (`FEmusic_home`), Busca (`/youtubei/v1/search`), Músicas Curtidas (`VLLM`) e Playlists (`FEmusic_liked_playlists` e `VL...`), integrado à fila nativa do `AppContainer` e à extração de stream em `audio.rs`.
- **Fase 4: Interface Completa e Modular em Vue 3 (Dark Mode estilo YouTube Music, Gaveta da Fila, Modo Ultra-Economia de Imagens)** — `[CONCLUÍDO]`
  - Modularização didática em `src/components/`: `HeaderBar.vue`, `SidebarNav.vue`, `TrackCard.vue`, `PlayerBar.vue`, `QueueDrawer.vue` e `AuthModal.vue`.
  - Gaveta lateral interativa **Fila de Reprodução (`QueueDrawer.vue`)** sincronizada via evento `"queue-changed"` do Rust, permitindo pular para qualquer faixa (`play_queue_index`), remover faixas (`remove_from_queue`) ou limpar a fila (`clear_queue`).
  - **Modo Ultra-Economia (`low_ram_no_images`)** persistido no Rust + `loading="lazy" decoding="async"` nas miniaturas para reduzir o consumo de texturas de vídeo na WebView quando aberta.
- **Fase 5: Integração Final, Autoplay Contínuo (`/youtubei/v1/next` Rádio Infinita), Modos de Repetição, Shuffle & Hardening de Baixo Consumo** — `[CONCLUÍDO]`
  - Implementação de `fetch_radio_up_next` em `src-tauri/src/innertube.rs` consultando o endpoint `/youtubei/v1/next` (`playlistPanelVideoRenderer`) e anexando automaticamente faixas recomendadas quando a fila termina (`spawn_radio_continuation`).
  - Enum `RepeatMode` (`Off`, `All`, `One`) e algoritmo **Fisher-Yates Shuffle (`shuffle_queue`)** em Rust (mantendo a música atual no índice `0` sem interromper a reprodução).
  - Hardening de compilação (`jobs = 2`, `debug = 0`, `strip = "debuginfo"`) e eliminação de chamadas redundantes ao DBus/AppIndicator no Linux (`last_tray_text`).
