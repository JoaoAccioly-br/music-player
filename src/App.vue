<script setup lang="ts">
import { onMounted, onUnmounted, ref } from 'vue'
import {
  Activity,
  Disc3,
  ListMusic,
  Loader2,
  Pause,
  Play,
  Radio,
  RefreshCw,
  Shuffle,
  Sparkles,
  Youtube,
} from 'lucide-vue-next'
import HeaderBar from './components/HeaderBar.vue'
import SidebarNav from './components/SidebarNav.vue'
import TrackCard from './components/TrackCard.vue'
import QueueDrawer from './components/QueueDrawer.vue'
import PlayerBar from './components/PlayerBar.vue'
import AuthModal from './components/AuthModal.vue'
import {
  DEMO_TRACKS,
  usePlayerStore,
  type PlaylistItem,
  type TrackItem,
} from './stores/player'

const player = usePlayerStore()
const activeTab = ref<'home' | 'explore' | 'library' | 'liked'>('home')
const searchQuery = ref('')
const youtubeInputUrl = ref('')
const showAuthModal = ref(false)

function handlePlayCustomYoutube() {
  if (!youtubeInputUrl.value.trim()) return
  player.playYoutubeVideo(youtubeInputUrl.value)
}

async function handleSearchSubmit(query: string) {
  const q = query.trim()
  if (!q) return
  searchQuery.value = q
  activeTab.value = 'explore'
  await player.searchMusic(q)
}

async function runQuickSearch(term: string) {
  searchQuery.value = term
  activeTab.value = 'explore'
  await player.searchMusic(term)
}

async function switchTab(tab: 'home' | 'explore' | 'library' | 'liked') {
  activeTab.value = tab
  if (tab === 'home' && player.homeSections.length === 0) {
    await player.loadHomeFeed()
  } else if (tab === 'library') {
    await player.loadUserPlaylists()
  } else if (tab === 'liked') {
    await player.loadLikedMusic()
  } else if (tab === 'explore' && player.searchResults.length === 0 && !searchQuery.value) {
    await runQuickSearch('Synthwave Gaming Mix')
  }
}

function handlePlayFromList(tracks: TrackItem[], index: number) {
  player.playTrackList(tracks, index)
}

function handleSelectPlaylist(pl: PlaylistItem) {
  player.loadPlaylistTracks(pl)
}

onMounted(() => {
  player.initStore()
})

onUnmounted(() => {
  player.cleanupStore()
})
</script>

<template>
  <div class="flex flex-col h-screen w-screen bg-[#030303] text-white overflow-hidden">
    <!-- Top Modular Header Bar -->
    <HeaderBar
      :search-query="searchQuery"
      @search="handleSearchSubmit"
      @open-auth="showAuthModal = true"
    />

    <!-- Middle Layout: Sidebar + Main View + Slide-Over Queue Drawer -->
    <div class="flex flex-1 overflow-hidden">
      <!-- Left Navigation & Telemetry Sidebar -->
      <SidebarNav :active-tab="activeTab" @change-tab="switchTab" />

      <!-- Main Scrollable Content Area -->
      <main
        class="flex-1 overflow-y-auto p-6 space-y-6 bg-gradient-to-b from-zinc-900/30 via-[#030303] to-[#030303]"
      >
        <!-- Direct YouTube URL / Video ID Stream Tester -->
        <section class="rounded-2xl border border-zinc-800/90 bg-zinc-900/60 p-5 space-y-3">
          <div class="flex items-center justify-between flex-wrap gap-2">
            <div class="flex items-center gap-2 text-sm font-semibold text-white">
              <Youtube class="w-4 h-4 text-red-500" />
              <span>Testar Stream Direto do YouTube (URL ou Video ID)</span>
            </div>
            <div class="flex items-center gap-2 text-xs text-zinc-400">
              <Activity class="w-3.5 h-3.5 text-emerald-400" />
              <span>{{ player.audioQualityLabel }}</span>
            </div>
          </div>

          <form @submit.prevent="handlePlayCustomYoutube" class="flex items-center gap-3">
            <input
              v-model="youtubeInputUrl"
              type="text"
              placeholder="Cole uma URL do YouTube / YouTube Music ou ID de 11 caracteres (ex: fJ9rUzIMcZQ)..."
              class="flex-1 bg-zinc-950/90 hover:bg-zinc-900 focus:bg-zinc-950 text-sm text-white placeholder-zinc-500 px-4 py-2.5 rounded-xl border border-zinc-800 focus:border-red-500/60 focus:outline-none transition-colors"
            />
            <button
              type="submit"
              :disabled="player.isLoading"
              class="px-5 py-2.5 rounded-xl bg-red-600 hover:bg-red-500 disabled:opacity-50 text-sm font-semibold text-white flex items-center gap-2 transition-colors shadow-lg shadow-red-600/20 shrink-0"
            >
              <Loader2 v-if="player.isLoading" class="w-4 h-4 animate-spin" />
              <Play v-else class="w-4 h-4 fill-current" />
              <span>Tocar Stream na RAM</span>
            </button>
          </form>
        </section>

        <!-- ==================== TAB: HOME ==================== -->
        <template v-if="activeTab === 'home'">
          <!-- Hero Architecture Banner -->
          <section
            class="relative overflow-hidden rounded-2xl border border-zinc-800/90 bg-gradient-to-r from-zinc-900 via-zinc-900/90 to-red-950/30 p-6"
          >
            <div class="flex items-start justify-between flex-wrap gap-4">
              <div class="max-w-2xl space-y-2.5">
                <div
                  class="inline-flex items-center gap-2 px-3 py-1 rounded-full bg-red-500/10 border border-red-500/20 text-red-400 text-xs font-medium"
                >
                  <Sparkles class="w-3.5 h-3.5" />
                  <span>
                    Fase 4 &amp; 5 • UI Modular Vue 3, Rádio Infinita (`/youtubei/v1/next`) &amp;
                    Ultra-Economia RAM
                  </span>
                </div>
                <h1 class="text-2xl font-bold tracking-tight text-white">
                  YouTube Music Desktop para Gamers (Zero-WebView em Background)
                </h1>
                <p class="text-sm text-zinc-300 leading-relaxed">
                  Quando a fila chega ao fim durante uma partida, o Rust aciona automaticamente o
                  endpoint <code>/youtubei/v1/next</code> para manter a
                  <strong>Rádio Infinita</strong> tocando sem interromper o jogo. Feche no
                  <code>X</code> a qualquer momento para destruir a WebView e liberar 100% da RAM
                  de interface!
                </p>
              </div>

              <div class="flex items-center gap-2">
                <button
                  @click="player.toggleQueueDrawer()"
                  class="px-3.5 py-2 rounded-xl bg-zinc-800/90 hover:bg-zinc-700 border border-zinc-700 text-xs font-semibold text-white flex items-center gap-2 transition-colors"
                >
                  <ListMusic class="w-4 h-4 text-red-400" />
                  <span>Abrir Fila ({{ player.queue.length }})</span>
                </button>
              </div>
            </div>
          </section>

          <!-- Innertube Home Sections using reusable TrackCard -->
          <section
            v-for="(section, sIdx) in player.homeSections"
            :key="sIdx"
            class="space-y-3"
          >
            <div class="flex items-center justify-between">
              <h2 class="text-lg font-bold tracking-tight text-white">{{ section.title }}</h2>
              <div class="flex items-center gap-3">
                <button
                  @click="handlePlayFromList(section.items, 0)"
                  class="text-xs text-red-400 hover:text-red-300 font-medium flex items-center gap-1 transition-colors"
                >
                  <Play class="w-3.5 h-3.5 fill-current" />
                  <span>Tocar Seção</span>
                </button>
                <button
                  @click="player.loadHomeFeed()"
                  class="text-xs text-zinc-400 hover:text-white flex items-center gap-1.5 transition-colors"
                >
                  <RefreshCw
                    :class="['w-3.5 h-3.5', player.isFetchingContent ? 'animate-spin' : '']"
                  />
                  <span>Atualizar</span>
                </button>
              </div>
            </div>

            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
              <TrackCard
                v-for="(track, tIdx) in section.items.slice(0, 8)"
                :key="track.video_id + '-' + tIdx"
                :track="track"
                mode="card"
                @play="handlePlayFromList(section.items, tIdx)"
              />
            </div>
          </section>

          <!-- Interactive Demo Tracks Grid (Low-RAM Synth & Stream Test) -->
          <section class="space-y-4">
            <div class="flex items-center justify-between">
              <div>
                <h2 class="text-lg font-bold tracking-tight">
                  Faixas de Demonstração (Teste Rápido de Buffer RAM)
                </h2>
                <p class="text-xs text-zinc-400">
                  Clique em qualquer faixa para testar o `rodio::Sink` e o modo Zero-WebView
                </p>
              </div>
              <div class="flex items-center gap-2 text-xs text-zinc-400">
                <Radio class="w-4 h-4 text-red-500" />
                <span>Fila #{{ player.queueIndex + 1 }} / {{ player.queue.length }}</span>
              </div>
            </div>

            <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
              <div
                v-for="track in DEMO_TRACKS"
                :key="track.index"
                @click="player.selectDemoTrack(track.index)"
                :class="[
                  'group relative rounded-xl p-4 border transition-all cursor-pointer',
                  player.videoId === track.videoId
                    ? 'bg-zinc-800/90 border-red-500/50 shadow-lg shadow-red-950/20'
                    : 'bg-zinc-900/50 border-zinc-800/80 hover:bg-zinc-800/60 hover:border-zinc-700',
                ]"
              >
                <div
                  :class="[
                    'w-full aspect-square rounded-lg mb-3.5 bg-gradient-to-br flex items-center justify-center relative overflow-hidden shadow-md',
                    track.coverGradient,
                  ]"
                >
                  <Disc3
                    :class="[
                      'w-12 h-12 text-white/80 transition-transform duration-500',
                      player.videoId === track.videoId && player.isPlaying
                        ? 'animate-spin'
                        : 'group-hover:scale-110',
                    ]"
                    :style="{ animationDuration: '5s' }"
                  />
                  <div
                    :class="[
                      'absolute bottom-2.5 right-2.5 w-10 h-10 rounded-full bg-red-600 text-white flex items-center justify-center shadow-lg transition-all',
                      player.videoId === track.videoId
                        ? 'opacity-100 translate-y-0'
                        : 'opacity-0 translate-y-2 group-hover:opacity-100 group-hover:translate-y-0',
                    ]"
                  >
                    <Loader2
                      v-if="player.videoId === track.videoId && player.isLoading"
                      class="w-4 h-4 animate-spin"
                    />
                    <Pause
                      v-else-if="player.videoId === track.videoId && player.isPlaying"
                      class="w-4 h-4 fill-current"
                    />
                    <Play v-else class="w-4 h-4 fill-current ml-0.5" />
                  </div>
                </div>

                <h3 class="font-semibold text-sm text-white truncate">{{ track.title }}</h3>
                <p class="text-xs text-zinc-400 truncate mt-0.5">{{ track.artist }}</p>
                <div
                  class="flex items-center justify-between mt-2 pt-2 border-t border-zinc-800/80 text-[11px] text-zinc-500"
                >
                  <span class="truncate">ID: {{ track.videoId }}</span>
                  <span>{{ track.duration }}</span>
                </div>
              </div>
            </div>
          </section>
        </template>

        <!-- ==================== TAB: EXPLORE / SEARCH ==================== -->
        <template v-else-if="activeTab === 'explore'">
          <section class="space-y-4">
            <div class="flex items-center justify-between flex-wrap gap-3">
              <div>
                <h2 class="text-xl font-bold tracking-tight">
                  Busca no YouTube Music (Innertube `WEB_REMIX`)
                </h2>
                <p class="text-xs text-zinc-400">
                  Resultados extraídos diretamente via `youtubei/v1/search` em Rust
                </p>
              </div>
              <div class="flex items-center gap-2 flex-wrap">
                <button
                  v-for="tag in [
                    'Synthwave Gaming',
                    'Daft Punk',
                    'The Weeknd',
                    'Linkin Park',
                    'Lofi Girl',
                  ]"
                  :key="tag"
                  @click="runQuickSearch(tag)"
                  class="px-3 py-1.5 rounded-full bg-zinc-900 hover:bg-zinc-800 border border-zinc-700/80 text-xs text-zinc-300 transition-colors"
                >
                  {{ tag }}
                </button>
              </div>
            </div>

            <div
              v-if="player.isFetchingContent"
              class="flex items-center justify-center py-12 text-zinc-400 gap-2.5"
            >
              <Loader2 class="w-5 h-5 animate-spin text-red-500" />
              <span class="text-sm">Consultando Innertube API no Rust...</span>
            </div>

            <div
              v-else
              class="divide-y divide-zinc-800/70 rounded-2xl border border-zinc-800/90 bg-zinc-900/40 overflow-hidden"
            >
              <TrackCard
                v-for="(track, idx) in player.searchResults"
                :key="track.video_id + '-' + idx"
                :track="track"
                mode="row"
                @play="handlePlayFromList(player.searchResults, idx)"
              />
            </div>
          </section>
        </template>

        <!-- ==================== TAB: LIBRARY / PLAYLISTS ==================== -->
        <template v-else-if="activeTab === 'library'">
          <section class="space-y-4">
            <div class="flex items-center justify-between">
              <div>
                <h2 class="text-xl font-bold tracking-tight">
                  Suas Playlists (`FEmusic_liked_playlists`)
                </h2>
                <p class="text-xs text-zinc-400">
                  Clique em uma playlist para carregar suas faixas diretamente na fila do Rust
                </p>
              </div>
              <button
                @click="player.loadUserPlaylists()"
                class="px-3 py-1.5 rounded-lg bg-zinc-900 hover:bg-zinc-800 border border-zinc-800 text-xs text-zinc-300 flex items-center gap-1.5"
              >
                <RefreshCw
                  :class="['w-3.5 h-3.5', player.isFetchingContent ? 'animate-spin' : '']"
                />
                <span>Recarregar</span>
              </button>
            </div>

            <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
              <div
                v-for="pl in player.userPlaylists"
                :key="pl.playlist_id"
                @click="handleSelectPlaylist(pl)"
                class="group rounded-xl p-4 bg-zinc-900/60 hover:bg-zinc-800/70 border border-zinc-800/90 hover:border-red-500/40 cursor-pointer transition-all flex items-center gap-3.5"
              >
                <div
                  class="w-16 h-16 rounded-lg bg-zinc-800 overflow-hidden shrink-0 flex items-center justify-center"
                >
                  <img
                    v-if="pl.thumbnail_url && !player.lowRamNoImages"
                    :src="pl.thumbnail_url"
                    :alt="pl.title"
                    loading="lazy"
                    decoding="async"
                    class="w-full h-full object-cover"
                  />
                  <ListMusic v-else class="w-7 h-7 text-red-400" />
                </div>
                <div class="min-w-0">
                  <h3 class="font-semibold text-sm text-white truncate">{{ pl.title }}</h3>
                  <p class="text-xs text-zinc-400 truncate mt-0.5">{{ pl.subtitle }}</p>
                  <span class="inline-block mt-1.5 text-[10px] text-red-400 font-medium">
                    Ver faixas →
                  </span>
                </div>
              </div>
            </div>

            <!-- Selected Playlist Tracks -->
            <div v-if="player.selectedPlaylistTracks.length > 0" class="space-y-3 pt-2">
              <div class="flex items-center justify-between">
                <h3 class="text-base font-bold text-white">
                  Faixas de: <span class="text-red-400">{{ player.selectedPlaylistTitle }}</span>
                </h3>
                <button
                  @click="handlePlayFromList(player.selectedPlaylistTracks, 0)"
                  class="px-4 py-1.5 rounded-full bg-red-600 hover:bg-red-500 text-xs font-semibold text-white flex items-center gap-1.5"
                >
                  <Play class="w-3.5 h-3.5 fill-current" />
                  <span>Tocar Playlist Inteira</span>
                </button>
              </div>

              <div
                class="divide-y divide-zinc-800/70 rounded-2xl border border-zinc-800/90 bg-zinc-900/40 overflow-hidden"
              >
                <TrackCard
                  v-for="(track, idx) in player.selectedPlaylistTracks"
                  :key="track.video_id + '-' + idx"
                  :track="track"
                  mode="row"
                  @play="handlePlayFromList(player.selectedPlaylistTracks, idx)"
                />
              </div>
            </div>
          </section>
        </template>

        <!-- ==================== TAB: LIKED MUSIC ==================== -->
        <template v-else-if="activeTab === 'liked'">
          <section class="space-y-4">
            <div class="flex items-center justify-between flex-wrap gap-3">
              <div>
                <h2 class="text-xl font-bold tracking-tight">Músicas Curtidas (`VLLM`)</h2>
                <p class="text-xs text-zinc-400">
                  Sincronizado via `youtubei/v1/browse` (`browseId: "VLLM"`) com `SAPISIDHASH`
                </p>
              </div>
              <div class="flex items-center gap-2">
                <button
                  v-if="player.likedTracks.length > 0"
                  @click="
                    () => {
                      handlePlayFromList(player.likedTracks, 0)
                      player.shuffleQueue()
                    }
                  "
                  class="px-4 py-2 rounded-full bg-zinc-800 hover:bg-zinc-700 text-xs font-semibold text-white flex items-center gap-2"
                >
                  <Shuffle class="w-3.5 h-3.5 text-red-400" />
                  <span>Tocar no Aleatório</span>
                </button>
                <button
                  v-if="player.likedTracks.length > 0"
                  @click="handlePlayFromList(player.likedTracks, 0)"
                  class="px-4 py-2 rounded-full bg-red-600 hover:bg-red-500 text-xs font-semibold text-white flex items-center gap-2 shadow-lg shadow-red-600/20"
                >
                  <Play class="w-3.5 h-3.5 fill-current" />
                  <span>Tocar Músicas Curtidas</span>
                </button>
              </div>
            </div>

            <div
              class="divide-y divide-zinc-800/70 rounded-2xl border border-zinc-800/90 bg-zinc-900/40 overflow-hidden"
            >
              <TrackCard
                v-for="(track, idx) in player.likedTracks"
                :key="track.video_id + '-' + idx"
                :track="track"
                mode="row"
                @play="handlePlayFromList(player.likedTracks, idx)"
              />
            </div>
          </section>
        </template>
      </main>

      <!-- Right Slide-over Queue Drawer -->
      <QueueDrawer />
    </div>

    <!-- Google Auth Modal -->
    <AuthModal :open="showAuthModal" @close="showAuthModal = false" />

    <!-- Bottom Fixed Player Bar -->
    <PlayerBar />
  </div>
</template>
