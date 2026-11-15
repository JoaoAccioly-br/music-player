<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import {
  ChevronDown,
  ChevronRight,
  Heart,
  Loader2,
  MessageSquareQuote,
  Moon,
  MoreHorizontal,
  Pause,
  Play,
  Plus,
  Radio,
  RefreshCw,
  Repeat,
  Repeat1,
  Shuffle,
  SkipBack,
  SkipForward,
  Sun,
  Volume2,
  VolumeX,
  X,
} from 'lucide-vue-next'
import HeaderBar from './components/HeaderBar.vue'
import TrackCard from './components/TrackCard.vue'
import QueueDrawer from './components/QueueDrawer.vue'
import PlayerBar from './components/PlayerBar.vue'
import AuthModal from './components/AuthModal.vue'
import {
  computeActiveLyricIndex,
  formatTime,
  getLyricLineSeekSecs,
  getPastelGradient,
  usePlayerStore,
  type PlaylistItem,
  type SearchFilter,
  type TrackItem,
} from './stores/player'

const player = usePlayerStore()

const activeTab = ref<'home' | 'explore' | 'library' | 'liked'>('home')
const searchQuery = ref('')
const isSearchActive = ref(false)
const showLyricsMainView = ref(false)
const showAuthModal = ref(false)
const showCustomStreamModal = ref(false)
const youtubeInputUrl = ref('')

// Home & Library filters
const activeHomeMood = ref<string>('Tudo')
const activeExploreCategory = ref<string>('Novos lançamentos')
const activeLibraryFilter = ref<'playlists' | 'albums' | 'artists' | 'songs'>('playlists')

const homeMoods = [
  {
    label: 'Tudo',
    query: '',
    pillClass:
      'bg-[#EBE5FF] text-[#5B3FD9] dark:bg-[#2A2248] dark:text-[#C4B5FD]',
  },
  {
    label: 'Relaxar',
    query: 'Chill Lofi Relax',
    pillClass:
      'bg-[#EBE5FF] text-[#5B3FD9] dark:bg-[#2A2248] dark:text-[#C4B5FD]',
  },
  {
    label: 'Foco',
    query: 'Deep Focus Ambient Study',
    pillClass:
      'bg-[#DCF7EC] text-[#147A56] dark:bg-[#1B3530] dark:text-[#86EFAC]',
  },
  {
    label: 'Energia',
    query: 'Upbeat Electronic Workout',
    pillClass:
      'bg-[#FDE8D7] text-[#B85418] dark:bg-[#3A2722] dark:text-[#FDBA74]',
  },
  {
    label: 'Gaming',
    query: 'Synthwave Gaming Mix',
    pillClass:
      'bg-[#DFEEFF] text-[#1D5FBF] dark:bg-[#1E2D48] dark:text-[#93C5FD]',
  },
]

const exploreMoodPills = [
  {
    label: 'Relaxar',
    bg: 'bg-[#EBE5FF] text-[#5B3FD9] dark:bg-[#2A2248] dark:text-[#C4B5FD]',
  },
  {
    label: 'Foco',
    bg: 'bg-[#DCF7EC] text-[#147A56] dark:bg-[#1B3530] dark:text-[#86EFAC]',
  },
  {
    label: 'Energia',
    bg: 'bg-[#FDE8D7] text-[#B85418] dark:bg-[#3A2722] dark:text-[#FDBA74]',
  },
  {
    label: 'Gaming',
    bg: 'bg-[#DFEEFF] text-[#1D5FBF] dark:bg-[#1E2D48] dark:text-[#93C5FD]',
  },
  {
    label: 'Treino',
    bg: 'bg-[#FFE4E6] text-[#BE123C] dark:bg-[#3B1D2A] dark:text-[#FDA4AF]',
  },
  {
    label: 'Dormir',
    bg: 'bg-[#E5E7EB] text-[#374151] dark:bg-[#262936] dark:text-[#D1D5DB]',
  },
  {
    label: 'Festa',
    bg: 'bg-[#FCE7F3] text-[#9D174D] dark:bg-[#381E34] dark:text-[#F9A8D4]',
  },
  {
    label: 'Viagem',
    bg: 'bg-[#FEF3C7] text-[#92400E] dark:bg-[#362C1E] dark:text-[#FCD34D]',
  },
  {
    label: 'Eletrônica',
    bg: 'bg-[#E0E7FF] text-[#3730A3] dark:bg-[#222647] dark:text-[#A5B4FC]',
  },
  {
    label: 'Rock',
    bg: 'bg-[#FEE2E2] text-[#991B1B] dark:bg-[#382024] dark:text-[#FCA5A5]',
  },
  {
    label: 'MPB',
    bg: 'bg-[#D1FAE5] text-[#065F46] dark:bg-[#19352D] dark:text-[#6EE7B7]',
  },
  {
    label: 'Hip-Hop',
    bg: 'bg-[#EDE9FE] text-[#5B21B6] dark:bg-[#2B2146] dark:text-[#C4B5FD]',
  },
]

const searchFilterOptions: Array<{ id: SearchFilter; label: string }> = [
  { id: 'all', label: 'Tudo' },
  { id: 'songs', label: 'Músicas' },
  { id: 'videos', label: 'Vídeos' },
  { id: 'albums', label: 'Álbuns' },
  { id: 'artists', label: 'Artistas' },
  { id: 'playlists', label: 'Playlists' },
]

const greetingText = computed(() => {
  const hour = new Date().getHours()
  const salutation = hour < 12 ? 'Bom dia' : hour < 18 ? 'Boa tarde' : 'Boa noite'
  const rawHint = (player.authStatus.account_hint || '').trim()
  const userName =
    rawHint && rawHint !== 'Não conectado' && !rawHint.includes('SAPISID')
      ? rawHint.split('@')[0]
      : 'Guilherme'
  return `${salutation}, ${userName}`
})

const listenAgainTracks = computed<TrackItem[]>(() => {
  const fromSections = player.homeSections.flatMap((s) => s.items)
  const source = fromSections.length > 0 ? fromSections : player.queue
  return source.slice(0, 5)
})

const exploreTracks = computed<TrackItem[]>(() => {
  if (player.searchResults.length > 0) {
    return player.searchResults.slice(0, 10)
  }
  const all = player.homeSections.flatMap((s) => s.items)
  return all.length > 0 ? all.slice(0, 10) : player.queue
})

const displayedSearchResults = computed<TrackItem[]>(() => {
  const base =
    player.searchResults.length > 0
      ? player.searchResults
      : player.homeSections.flatMap((s) => s.items)
  return base
})

const topSearchResult = computed<TrackItem | null>(() => {
  return displayedSearchResults.value[0] ?? player.queue[0] ?? null
})

const searchArtists = computed<Array<{ name: string; gradient: string }>>(() => {
  const defaults = ['Kavinsky', 'Carpenter Brut', 'The Midnight', 'Gunship', 'Perturbator']
  const seen = new Set<string>()
  const names: string[] = []

  for (const item of displayedSearchResults.value) {
    const clean = (item.artist || '').trim()
    if (clean && !seen.has(clean)) {
      seen.add(clean)
      names.push(clean)
    }
  }
  for (const fallbackName of defaults) {
    if (names.length >= 5) break
    if (!seen.has(fallbackName)) {
      seen.add(fallbackName)
      names.push(fallbackName)
    }
  }

  return names.slice(0, 5).map((name, idx) => ({
    name,
    gradient: getPastelGradient(name + idx),
  }))
})

const activeLyricIndex = computed(() =>
  computeActiveLyricIndex(player.lyrics, player.positionSecs, player.durationSecs)
)

const lyricLineEls = new Map<number, HTMLElement>()
function setLyricLineRef(el: unknown, idx: number) {
  if (el instanceof HTMLElement) {
    lyricLineEls.set(idx, el)
  } else {
    lyricLineEls.delete(idx)
  }
}

async function scrollActiveLyricIntoView(idx: number) {
  if (idx < 0 || !showLyricsMainView.value) return
  await nextTick()
  lyricLineEls.get(idx)?.scrollIntoView({ behavior: 'smooth', block: 'center' })
}

watch(activeLyricIndex, (idx) => {
  scrollActiveLyricIntoView(idx)
})

watch(showLyricsMainView, (visible) => {
  if (visible && activeLyricIndex.value >= 0) {
    scrollActiveLyricIntoView(activeLyricIndex.value)
  }
})

function seekToLyricLine(lineIdx: number) {
  const timed = player.lyrics?.timedLines ?? []
  if (!timed.length) return
  const targetSec = getLyricLineSeekSecs(player.lyrics, lineIdx, player.durationSecs)
  player.seekTo(targetSec)
}

async function handleSearchSubmit(query: string) {
  const q = query.trim()
  if (!q) {
    isSearchActive.value = false
    return
  }
  searchQuery.value = q
  isSearchActive.value = true
  showLyricsMainView.value = false
  await player.searchMusic(q)
}

function handleClearSearch() {
  searchQuery.value = ''
  isSearchActive.value = false
}

async function selectHomeMood(mood: { label: string; query: string }) {
  activeHomeMood.value = mood.label
  if (!mood.query) {
    await player.loadHomeFeed()
  } else {
    await player.searchMusic(mood.query)
    if (player.searchResults.length > 0) {
      player.homeSections = [
        {
          title: `Ouvir de novo • ${mood.label}`,
          items: player.searchResults.slice(0, 10),
        },
        ...player.homeSections.slice(1),
      ]
    }
  }
}

async function playMixCard(query: string) {
  await player.searchMusic(query)
  const tracks =
    player.searchResults.length > 0
      ? player.searchResults
      : player.homeSections.flatMap((s) => s.items)
  if (tracks.length > 0) {
    await player.playTrackList(tracks, 0)
  }
}

async function selectExploreCategory(title: string, query: string) {
  activeExploreCategory.value = title
  await player.searchMusic(query)
}

async function handleMoodPillSearch(label: string) {
  searchQuery.value = label
  isSearchActive.value = true
  showLyricsMainView.value = false
  await player.searchMusic(`${label} Music Mix`)
}

async function switchTab(tab: 'home' | 'explore' | 'library' | 'liked') {
  activeTab.value = tab
  isSearchActive.value = false
  showLyricsMainView.value = false

  if (tab === 'home' && player.homeSections.length === 0) {
    await player.loadHomeFeed()
  } else if (tab === 'library') {
    await player.loadUserPlaylists()
  } else if (tab === 'liked') {
    await player.loadLikedMusic()
  } else if (tab === 'explore' && player.searchResults.length === 0) {
    await player.searchMusic('Novos lançamentos música')
  }
}

function handlePlayFromList(tracks: TrackItem[], index: number) {
  player.playTrackList(tracks, index)
}

function handleSelectPlaylist(pl: PlaylistItem) {
  player.loadPlaylistTracks(pl)
}

async function handlePlayPlaylistImmediate(pl: PlaylistItem) {
  await player.loadPlaylistTracks(pl)
  if (player.selectedPlaylistTracks.length > 0) {
    player.playTrackList(player.selectedPlaylistTracks, 0)
  }
}

function handleOpenLyricsView() {
  showLyricsMainView.value = !showLyricsMainView.value
  if (showLyricsMainView.value) {
    player.setRightPanelTab('lyrics')
  }
}

function handlePlayCustomYoutube() {
  const trimmed = youtubeInputUrl.value.trim()
  if (!trimmed) return
  player.playYoutubeVideo(trimmed)
  youtubeInputUrl.value = ''
  showCustomStreamModal.value = false
}

function handleFullscreenSeek(event: Event) {
  const target = event.target as HTMLInputElement
  player.seekTo(parseFloat(target.value))
}

function handleFullscreenVolume(event: Event) {
  const target = event.target as HTMLInputElement
  player.setVolume(parseFloat(target.value))
}

onMounted(() => {
  player.initStore()
})

onUnmounted(() => {
  player.cleanupStore()
})
</script>

<template>
  <div
    class="relative flex flex-col h-screen w-screen overflow-hidden bg-[#F7F6FA] dark:bg-[#12101B] text-[#1A162B] dark:text-[#F4F2FA] transition-colors duration-250"
  >
    <!-- Soft Ambient Pastel Radial Glows at Top -->
    <div
      class="pointer-events-none fixed inset-x-0 top-0 h-72 bg-[radial-gradient(ellipse_at_top_left,_rgba(217,194,255,0.35),_transparent_60%),radial-gradient(ellipse_at_top_right,_rgba(250,208,196,0.28),_transparent_60%)] dark:bg-[radial-gradient(ellipse_at_top_left,_rgba(139,110,255,0.12),_transparent_60%),radial-gradient(ellipse_at_top_right,_rgba(244,162,184,0.08),_transparent_60%)] z-0"
    ></div>

    <!-- Top Unified Header Bar ("Navegação do C") -->
    <HeaderBar
      :active-tab="activeTab"
      :search-query="searchQuery"
      :is-search-active="isSearchActive"
      @change-tab="switchTab"
      @search="handleSearchSubmit"
      @clear-search="handleClearSearch"
      @open-auth="showAuthModal = true"
    />

    <!-- Main Workspace: Content Area + Optional Right Panel (A seguir / Letra / Relacionados) -->
    <div class="relative z-10 flex flex-1 overflow-hidden">
      <!-- Scrollable Main View -->
      <main class="flex-1 overflow-y-auto px-4 sm:px-8 pt-6 pb-28">
        <div class="max-w-6xl mx-auto space-y-8">
          <!-- ==================== DEDICATED SPLIT LYRICS VIEW ("Letra (escuro)") ==================== -->
          <template v-if="showLyricsMainView">
            <section class="pt-2">
              <div class="grid grid-cols-1 lg:grid-cols-12 gap-8 items-start">
                <!-- Left Column: Large Cover + Track Info + Like & Playlist Buttons -->
                <div class="lg:col-span-5 flex flex-col items-center lg:items-start">
                  <div
                    :class="[
                      'w-64 h-64 sm:w-72 sm:h-72 rounded-3xl overflow-hidden shadow-xl bg-gradient-to-br border border-[rgba(120,100,180,0.14)] dark:border-white/10',
                      getPastelGradient(player.videoId + player.currentTrackTitle),
                    ]"
                  >
                    <img
                      v-if="player.currentThumbnailUrl && !player.lowRamNoImages"
                      :src="player.currentThumbnailUrl"
                      :alt="player.currentTrackTitle"
                      class="w-full h-full object-cover"
                    />
                  </div>

                  <div class="mt-5 text-center lg:text-left">
                    <h1 class="text-2xl sm:text-3xl font-bold tracking-tight">
                      {{ player.currentTrackTitle }}
                    </h1>
                    <p class="text-xs sm:text-sm text-[#6E6885] dark:text-[#9B94B2] mt-1">
                      {{ player.currentTrackArtist }} • OutRun
                    </p>
                  </div>

                  <div class="mt-4 flex items-center gap-2.5 flex-wrap">
                    <button
                      type="button"
                      @click="player.toggleLikeTrack()"
                      :class="[
                        'px-4 py-2 rounded-full text-xs font-semibold flex items-center gap-1.5 border transition-colors',
                        player.isTrackLiked(player.videoId)
                          ? 'bg-[#EC4899]/15 border-[#EC4899]/35 text-[#EC4899] dark:text-[#F472B6]'
                          : 'bg-white dark:bg-[#1E1A2D] border-[rgba(120,100,180,0.14)] dark:border-white/10 text-[#1A162B] dark:text-[#F4F2FA]',
                      ]"
                    >
                      <Heart
                        :class="[
                          'w-3.5 h-3.5',
                          player.isTrackLiked(player.videoId) ? 'fill-current' : '',
                        ]"
                      />
                      <span>{{ player.isTrackLiked(player.videoId) ? 'Curtida' : 'Curtir' }}</span>
                    </button>

                    <button
                      type="button"
                      @click="showLyricsMainView = false"
                      class="px-4 py-2 rounded-full text-xs font-semibold bg-white dark:bg-[#1E1A2D] border border-[rgba(120,100,180,0.14)] dark:border-white/10 text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white transition-colors"
                    >
                      Voltar à navegação
                    </button>
                  </div>
                </div>

                <!-- Right Column: Tab Switcher + Synchronized Large Lyrics -->
                <div class="lg:col-span-7 space-y-5">
                  <div class="flex items-center justify-between flex-wrap gap-3">
                    <div
                      class="inline-flex items-center p-1 rounded-full bg-[#EDE9F5] dark:bg-[#1E1A2D] border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07]"
                    >
                      <button
                        type="button"
                        @click="
                          () => {
                            showLyricsMainView = false
                            player.openRightPanel('queue')
                          }
                        "
                        class="px-3.5 py-1.5 rounded-full text-xs font-semibold text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white"
                      >
                        A seguir
                      </button>
                      <button
                        type="button"
                        class="px-3.5 py-1.5 rounded-full text-xs font-semibold bg-white dark:bg-[#2C2640] text-[#1A162B] dark:text-[#F4F2FA] shadow-sm"
                      >
                        Letra
                      </button>
                      <button
                        type="button"
                        @click="
                          () => {
                            showLyricsMainView = false
                            player.openRightPanel('related')
                          }
                        "
                        class="px-3.5 py-1.5 rounded-full text-xs font-semibold text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white"
                      >
                        Relacionados
                      </button>
                    </div>

                    <span
                      :class="[
                        'inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-[11px] font-medium',
                        player.lyrics?.isSynced
                          ? 'bg-[#22C55E]/15 text-[#15803D] dark:text-[#4ADE80]'
                          : 'bg-[#EDE9F5] dark:bg-[#1E1A2D] text-[#6E6885] dark:text-[#9B94B2]',
                      ]"
                    >
                      {{ player.lyrics?.sourceLabel || 'Buscando letra...' }}
                    </span>
                  </div>

                  <div
                    v-if="player.isLoadingLyrics"
                    class="py-16 flex items-center gap-3 text-[#6E6885] dark:text-[#9B94B2]"
                  >
                    <Loader2 class="w-5 h-5 animate-spin text-[#7C5CFC]" />
                    <span class="text-sm">Carregando letra sincronizada...</span>
                  </div>

                  <div
                    v-else-if="!(player.lyrics?.timedLines?.length)"
                    class="py-16 text-sm text-[#6E6885] dark:text-[#9B94B2]"
                  >
                    {{ player.lyrics?.sourceLabel || 'Letra indisponível para esta faixa.' }}
                  </div>

                  <div v-else class="space-y-4 py-2 max-h-[60vh] overflow-y-auto pr-2">
                    <p
                      v-for="(line, idx) in player.lyrics?.timedLines || []"
                      :key="idx"
                      :ref="(el) => setLyricLineRef(el, idx)"
                      @click="seekToLyricLine(idx)"
                      :class="[
                        'group flex items-baseline gap-3 cursor-pointer transition-all duration-300 leading-snug',
                        idx === activeLyricIndex
                          ? 'text-xl sm:text-2xl font-bold text-[#1A162B] dark:text-[#F4F2FA]'
                          : idx === activeLyricIndex + 1
                            ? 'text-base sm:text-lg font-medium text-[#6E6885] dark:text-[#9B94B2]'
                            : idx < activeLyricIndex
                              ? 'text-sm sm:text-base font-medium text-[#6E6885]/50 dark:text-[#9B94B2]/40 hover:text-[#1A162B] dark:hover:text-white'
                              : 'text-sm sm:text-base font-medium text-[#6E6885]/65 dark:text-[#9B94B2]/55 hover:text-[#1A162B] dark:hover:text-white',
                      ]"
                    >
                      <span
                        v-if="line.timeSecs !== null"
                        class="text-[11px] font-normal tabular-nums text-[#6E6885]/60 dark:text-[#9B94B2]/50 opacity-0 group-hover:opacity-100 transition-opacity shrink-0"
                      >
                        {{ formatTime(line.timeSecs) }}
                      </span>
                      <span>{{ line.text }}</span>
                    </p>
                  </div>
                </div>
              </div>
            </section>
          </template>

          <!-- ==================== TELA 3: RESULTADOS DA BUSCA ==================== -->
          <template v-else-if="isSearchActive">
            <!-- Search Filter Pills -->
            <div class="flex items-center gap-2 flex-wrap">
              <button
                v-for="filter in searchFilterOptions"
                :key="filter.id"
                type="button"
                @click="player.searchFilter = filter.id"
                :class="[
                  'px-4 py-1.5 rounded-full text-xs font-semibold transition-all',
                  player.searchFilter === filter.id
                    ? 'bg-[#1A162B] text-white dark:bg-[#F4F2FA] dark:text-[#12101B] shadow-sm'
                    : 'bg-white dark:bg-[#1E1A2D] text-[#6E6885] dark:text-[#9B94B2] border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07] hover:text-[#1A162B] dark:hover:text-white',
                ]"
              >
                {{ filter.label }}
              </button>
            </div>

            <div
              v-if="player.isFetchingContent"
              class="py-16 flex items-center justify-center gap-3 text-[#6E6885] dark:text-[#9B94B2]"
            >
              <Loader2 class="w-5 h-5 animate-spin text-[#7C5CFC]" />
              <span class="text-sm">Buscando "{{ searchQuery }}" no YouTube Music...</span>
            </div>

            <template v-else>
              <!-- 2-Column Top Layout: Melhor resultado (Left) + Músicas (Right) -->
              <div class="grid grid-cols-1 lg:grid-cols-12 gap-6 items-start">
                <!-- Left Column: Melhor resultado -->
                <div v-if="topSearchResult" class="lg:col-span-5 space-y-3">
                  <h2 class="text-sm font-bold tracking-tight">Melhor resultado</h2>
                  <div
                    class="p-5 rounded-3xl bg-white dark:bg-[#1E1A2D] border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07] shadow-sm space-y-4"
                  >
                    <div
                      :class="[
                        'w-24 h-24 rounded-2xl overflow-hidden shadow-sm bg-gradient-to-br',
                        getPastelGradient(topSearchResult.video_id + topSearchResult.title),
                      ]"
                    >
                      <img
                        v-if="topSearchResult.thumbnail_url && !player.lowRamNoImages"
                        :src="topSearchResult.thumbnail_url"
                        :alt="topSearchResult.title"
                        class="w-full h-full object-cover"
                      />
                    </div>

                    <div>
                      <h3 class="text-xl font-bold tracking-tight">
                        {{ searchQuery || topSearchResult.title }}
                      </h3>
                      <p class="text-xs text-[#6E6885] dark:text-[#9B94B2] mt-1">
                        Rádio • {{ topSearchResult.artist }} • {{ topSearchResult.title }}
                      </p>
                    </div>

                    <div class="flex items-center gap-2.5 pt-1">
                      <button
                        type="button"
                        @click="handlePlayFromList(displayedSearchResults, 0)"
                        class="px-4 py-2 rounded-full bg-[#7C5CFC] hover:bg-[#6D4AFF] text-white text-xs font-semibold flex items-center gap-1.5 shadow-sm transition-colors"
                      >
                        <Play class="w-3.5 h-3.5 fill-current" />
                        <span>Tocar</span>
                      </button>

                      <button
                        type="button"
                        @click="player.startRadioFromTrack(topSearchResult)"
                        class="px-4 py-2 rounded-full bg-[#F2EFF9] dark:bg-[#252038] hover:bg-[#E6E0F5] dark:hover:bg-[#2F2946] text-[#1A162B] dark:text-[#F4F2FA] text-xs font-semibold transition-colors"
                      >
                        Iniciar rádio
                      </button>
                    </div>
                  </div>
                </div>

                <!-- Right Column: Músicas -->
                <div class="lg:col-span-7 space-y-3">
                  <h2 class="text-sm font-bold tracking-tight">Músicas</h2>
                  <div
                    class="rounded-3xl bg-white/80 dark:bg-[#1E1A2D]/80 border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07] p-2 divide-y divide-[rgba(120,100,180,0.08)] dark:divide-white/[0.05]"
                  >
                    <TrackCard
                      v-for="(track, idx) in displayedSearchResults.slice(0, 4)"
                      :key="track.video_id + '-top-' + idx"
                      :track="track"
                      mode="row"
                      @play="handlePlayFromList(displayedSearchResults, idx)"
                    />
                  </div>
                </div>
              </div>

              <!-- Artists Circular Avatars Row -->
              <section class="space-y-3">
                <h2 class="text-sm font-bold tracking-tight">Artistas</h2>
                <div class="grid grid-cols-2 sm:grid-cols-5 gap-4">
                  <button
                    v-for="artist in searchArtists"
                    :key="artist.name"
                    type="button"
                    @click="handleSearchSubmit(artist.name)"
                    class="group flex flex-col items-center gap-2.5 p-2 rounded-2xl hover:bg-white/60 dark:hover:bg-[#1E1A2D]/60 transition-colors"
                  >
                    <div
                      :class="[
                        'w-24 h-24 sm:w-28 sm:h-28 rounded-full bg-gradient-to-br shadow-sm group-hover:scale-105 transition-transform border border-[rgba(120,100,180,0.12)] dark:border-white/[0.08]',
                        artist.gradient,
                      ]"
                    ></div>
                    <span class="text-xs font-semibold text-center truncate w-full">
                      {{ artist.name }}
                    </span>
                  </button>
                </div>
              </section>

              <!-- Remaining Search Results -->
              <section v-if="displayedSearchResults.length > 4" class="space-y-3">
                <h2 class="text-sm font-bold tracking-tight">Mais resultados</h2>
                <div
                  class="rounded-3xl bg-white/80 dark:bg-[#1E1A2D]/80 border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07] p-2 divide-y divide-[rgba(120,100,180,0.08)] dark:divide-white/[0.05]"
                >
                  <TrackCard
                    v-for="(track, idx) in displayedSearchResults.slice(4)"
                    :key="track.video_id + '-more-' + idx"
                    :track="track"
                    mode="row"
                    :show-album="true"
                    @play="handlePlayFromList(displayedSearchResults, idx + 4)"
                  />
                </div>
              </section>
            </template>
          </template>

          <!-- ==================== TELA 1: INÍCIO (HOME) ==================== -->
          <template v-else-if="activeTab === 'home'">
            <!-- Greeting & Mood Pills -->
            <section class="space-y-4">
              <h1 class="text-2xl sm:text-3xl font-bold tracking-tight">
                {{ greetingText }}
              </h1>

              <div class="flex items-center gap-2 flex-wrap">
                <button
                  v-for="mood in homeMoods"
                  :key="mood.label"
                  type="button"
                  @click="selectHomeMood(mood)"
                  :class="[
                    'px-4 py-1.5 rounded-full text-xs font-semibold transition-all',
                    activeHomeMood === mood.label
                      ? 'bg-[#1A162B] text-white dark:bg-[#F4F2FA] dark:text-[#12101B] shadow-sm'
                      : mood.pillClass + ' hover:opacity-90',
                  ]"
                >
                  {{ mood.label }}
                </button>
              </div>
            </section>

            <!-- Section: Ouvir de novo (5-column square pastel cards) -->
            <section class="space-y-3.5">
              <div class="flex items-center justify-between">
                <h2 class="text-base font-bold tracking-tight">Ouvir de novo</h2>
                <button
                  type="button"
                  @click="handlePlayFromList(listenAgainTracks, 0)"
                  class="text-xs font-medium text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white transition-colors"
                >
                  Ver tudo
                </button>
              </div>

              <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-5 gap-4">
                <TrackCard
                  v-for="(track, idx) in listenAgainTracks"
                  :key="track.video_id + '-listen-' + idx"
                  :track="track"
                  mode="card"
                  @play="handlePlayFromList(listenAgainTracks, idx)"
                />
              </div>
            </section>

            <!-- Section: Mixes e rádios -->
            <section class="space-y-3.5">
              <h2 class="text-base font-bold tracking-tight">Mixes e rádios</h2>

              <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <!-- Card 1: RÁDIO • Synthwave Gaming -->
                <div
                  @click="playMixCard('Synthwave Gaming')"
                  class="group rounded-2xl p-5 h-28 flex flex-col justify-between cursor-pointer transition-all hover:-translate-y-0.5 hover:shadow-md bg-gradient-to-br from-[#E2D4FF] via-[#D8DCFF] to-[#C8D9FF] dark:from-[#2C2152] dark:via-[#26254A] dark:to-[#202944] border border-[rgba(120,100,180,0.14)] dark:border-white/[0.08]"
                >
                  <span
                    class="text-[10px] font-bold uppercase tracking-widest text-[#6D4AFF] dark:text-[#B497FF]"
                  >
                    RÁDIO
                  </span>
                  <div class="flex items-center justify-between">
                    <h3 class="text-base sm:text-lg font-bold text-[#1A162B] dark:text-[#F4F2FA]">
                      Synthwave Gaming
                    </h3>
                    <div
                      class="w-8 h-8 rounded-full bg-white/80 dark:bg-white/10 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity"
                    >
                      <Play class="w-3.5 h-3.5 fill-current ml-0.5" />
                    </div>
                  </div>
                </div>

                <!-- Card 2: MIX • Foco profundo -->
                <div
                  @click="playMixCard('Lofi Deep Focus')"
                  class="group rounded-2xl p-5 h-28 flex flex-col justify-between cursor-pointer transition-all hover:-translate-y-0.5 hover:shadow-md bg-gradient-to-br from-[#CEF5E6] via-[#D9F3E8] to-[#E2F5DC] dark:from-[#1B3A34] dark:via-[#1E352F] dark:to-[#223429] border border-[rgba(120,100,180,0.14)] dark:border-white/[0.08]"
                >
                  <span
                    class="text-[10px] font-bold uppercase tracking-widest text-[#147A56] dark:text-[#6EE7B7]"
                  >
                    MIX
                  </span>
                  <div class="flex items-center justify-between">
                    <h3 class="text-base sm:text-lg font-bold text-[#1A162B] dark:text-[#F4F2FA]">
                      Foco profundo
                    </h3>
                    <div
                      class="w-8 h-8 rounded-full bg-white/80 dark:bg-white/10 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity"
                    >
                      <Play class="w-3.5 h-3.5 fill-current ml-0.5" />
                    </div>
                  </div>
                </div>

                <!-- Card 3: RÁDIO • Baseada na música/artista atual -->
                <div
                  @click="player.startRadioFromTrack()"
                  class="group rounded-2xl p-5 h-28 flex flex-col justify-between cursor-pointer transition-all hover:-translate-y-0.5 hover:shadow-md bg-gradient-to-br from-[#FDE2CE] via-[#FCE8D8] to-[#F9EED8] dark:from-[#3C2824] dark:via-[#362724] dark:to-[#332A22] border border-[rgba(120,100,180,0.14)] dark:border-white/[0.08]"
                >
                  <span
                    class="text-[10px] font-bold uppercase tracking-widest text-[#B85418] dark:text-[#FDBA74]"
                  >
                    RÁDIO
                  </span>
                  <div class="flex items-center justify-between">
                    <h3
                      class="text-base sm:text-lg font-bold text-[#1A162B] dark:text-[#F4F2FA] truncate"
                    >
                      Baseada em {{ player.currentTrackArtist || 'Kavinsky' }}
                    </h3>
                    <div
                      class="w-8 h-8 rounded-full bg-white/80 dark:bg-white/10 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity shrink-0"
                    >
                      <Play class="w-3.5 h-3.5 fill-current ml-0.5" />
                    </div>
                  </div>
                </div>
              </div>
            </section>

            <!-- Additional Innertube Home Shelves -->
            <section
              v-for="(section, sIdx) in player.homeSections.slice(1)"
              :key="'home-sec-' + sIdx"
              class="space-y-3.5"
            >
              <div class="flex items-center justify-between">
                <h2 class="text-base font-bold tracking-tight">{{ section.title }}</h2>
                <button
                  type="button"
                  @click="handlePlayFromList(section.items, 0)"
                  class="text-xs font-medium text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white transition-colors"
                >
                  Tocar tudo
                </button>
              </div>

              <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-5 gap-4">
                <TrackCard
                  v-for="(track, tIdx) in section.items.slice(0, 5)"
                  :key="track.video_id + '-sec-' + sIdx + '-' + tIdx"
                  :track="track"
                  mode="card"
                  @play="handlePlayFromList(section.items, tIdx)"
                />
              </div>
            </section>
          </template>

          <!-- ==================== TELA 2: EXPLORAR ==================== -->
          <template v-else-if="activeTab === 'explore'">
            <section class="space-y-5">
              <h1 class="text-2xl sm:text-3xl font-bold tracking-tight">Explorar</h1>

              <!-- 3 Top Highlight Cards -->
              <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
                <button
                  type="button"
                  @click="selectExploreCategory('Novos lançamentos', 'New Music Releases')"
                  class="group p-5 rounded-2xl text-left flex items-center justify-between bg-[#EAE2FF] dark:bg-[#272042] border border-[rgba(120,100,180,0.14)] dark:border-white/[0.08] hover:-translate-y-0.5 transition-all"
                >
                  <div>
                    <h3 class="text-sm sm:text-base font-bold text-[#1A162B] dark:text-[#F4F2FA]">
                      Novos lançamentos
                    </h3>
                    <p class="text-xs text-[#6E6885] dark:text-[#9B94B2] mt-0.5">
                      Álbuns e singles da semana
                    </p>
                  </div>
                  <ChevronRight
                    class="w-4 h-4 text-[#6E6885] dark:text-[#9B94B2] group-hover:translate-x-0.5 transition-transform"
                  />
                </button>

                <button
                  type="button"
                  @click="selectExploreCategory('Paradas', 'Top Hits Global Charts')"
                  class="group p-5 rounded-2xl text-left flex items-center justify-between bg-[#FCE8D8] dark:bg-[#362628] border border-[rgba(120,100,180,0.14)] dark:border-white/[0.08] hover:-translate-y-0.5 transition-all"
                >
                  <div>
                    <h3 class="text-sm sm:text-base font-bold text-[#1A162B] dark:text-[#F4F2FA]">
                      Paradas
                    </h3>
                    <p class="text-xs text-[#6E6885] dark:text-[#9B94B2] mt-0.5">
                      O que está bombando
                    </p>
                  </div>
                  <ChevronRight
                    class="w-4 h-4 text-[#6E6885] dark:text-[#9B94B2] group-hover:translate-x-0.5 transition-transform"
                  />
                </button>

                <button
                  type="button"
                  @click="selectExploreCategory('Moods e gêneros', 'Synthwave Electronic Indie')"
                  class="group p-5 rounded-2xl text-left flex items-center justify-between bg-[#DCF5EC] dark:bg-[#1D3331] border border-[rgba(120,100,180,0.14)] dark:border-white/[0.08] hover:-translate-y-0.5 transition-all"
                >
                  <div>
                    <h3 class="text-sm sm:text-base font-bold text-[#1A162B] dark:text-[#F4F2FA]">
                      Moods e gêneros
                    </h3>
                    <p class="text-xs text-[#6E6885] dark:text-[#9B94B2] mt-0.5">
                      Para cada momento
                    </p>
                  </div>
                  <ChevronRight
                    class="w-4 h-4 text-[#6E6885] dark:text-[#9B94B2] group-hover:translate-x-0.5 transition-transform"
                  />
                </button>
              </div>
            </section>

            <!-- Moods e gêneros Colored Pills -->
            <section class="space-y-3.5">
              <h2 class="text-base font-bold tracking-tight">Moods e gêneros</h2>
              <div class="flex items-center gap-2.5 flex-wrap">
                <button
                  v-for="pill in exploreMoodPills"
                  :key="pill.label"
                  type="button"
                  @click="handleMoodPillSearch(pill.label)"
                  :class="[
                    'px-4 py-2 rounded-full text-xs font-semibold transition-transform hover:scale-105',
                    pill.bg,
                  ]"
                >
                  {{ pill.label }}
                </button>
              </div>
            </section>

            <!-- Novos lançamentos / Active Explore Grid -->
            <section class="space-y-3.5">
              <div class="flex items-center justify-between">
                <h2 class="text-base font-bold tracking-tight">{{ activeExploreCategory }}</h2>
                <button
                  type="button"
                  @click="handlePlayFromList(exploreTracks, 0)"
                  class="text-xs font-medium text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white transition-colors"
                >
                  Ver tudo
                </button>
              </div>

              <div
                v-if="player.isFetchingContent"
                class="py-12 flex items-center justify-center gap-2.5 text-[#6E6885] dark:text-[#9B94B2]"
              >
                <Loader2 class="w-5 h-5 animate-spin text-[#7C5CFC]" />
                <span class="text-xs">Carregando lançamentos...</span>
              </div>

              <div v-else class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-5 gap-4">
                <TrackCard
                  v-for="(track, idx) in exploreTracks.slice(0, 5)"
                  :key="track.video_id + '-exp-' + idx"
                  :track="track"
                  mode="card"
                  @play="handlePlayFromList(exploreTracks, idx)"
                />
              </div>
            </section>
          </template>

          <!-- ==================== TELA 4: BIBLIOTECA ==================== -->
          <template v-else-if="activeTab === 'library'">
            <section class="space-y-4">
              <div class="flex items-center justify-between">
                <h1 class="text-2xl sm:text-3xl font-bold tracking-tight">Biblioteca</h1>

                <button
                  type="button"
                  @click="player.loadUserPlaylists()"
                  class="px-3.5 py-1.5 rounded-full bg-white dark:bg-[#1E1A2D] border border-[rgba(120,100,180,0.14)] dark:border-white/[0.08] text-xs font-medium text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white flex items-center gap-1.5 shadow-sm transition-colors"
                >
                  <span>Recentes</span>
                  <RefreshCw
                    :class="['w-3 h-3', player.isFetchingContent ? 'animate-spin' : '']"
                  />
                </button>
              </div>

              <!-- Library Filter Pills -->
              <div class="flex items-center gap-2 flex-wrap">
                <button
                  v-for="tabItem in [
                    { id: 'playlists', label: 'Playlists' },
                    { id: 'albums', label: 'Álbuns' },
                    { id: 'artists', label: 'Artistas' },
                    { id: 'songs', label: 'Músicas' },
                  ]"
                  :key="tabItem.id"
                  type="button"
                  @click="
                    activeLibraryFilter = tabItem.id as
                      | 'playlists'
                      | 'albums'
                      | 'artists'
                      | 'songs'
                  "
                  :class="[
                    'px-4 py-1.5 rounded-full text-xs font-semibold transition-all',
                    activeLibraryFilter === tabItem.id
                      ? 'bg-[#1A162B] text-white dark:bg-[#F4F2FA] dark:text-[#12101B]'
                      : 'bg-white dark:bg-[#1E1A2D] text-[#6E6885] dark:text-[#9B94B2] border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07]',
                  ]"
                >
                  {{ tabItem.label }}
                </button>
              </div>
            </section>

            <!-- 5-Column Library Grid -->
            <section class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-5 gap-4">
              <!-- Card 1: Dashed "+ Nova playlist" / Tocar Stream Direto -->
              <div
                @click="showCustomStreamModal = true"
                class="group cursor-pointer flex flex-col"
              >
                <div
                  class="w-full aspect-square rounded-2xl mb-2.5 border-2 border-dashed border-[#C7BFE0] dark:border-white/15 bg-white/50 dark:bg-[#1E1A2D]/40 hover:border-[#7C5CFC] dark:hover:border-[#8B6EFF] flex items-center justify-center transition-colors"
                >
                  <Plus
                    class="w-7 h-7 text-[#6E6885] dark:text-[#9B94B2] group-hover:text-[#7C5CFC] dark:group-hover:text-[#B497FF] transition-colors"
                  />
                </div>
                <h3 class="font-semibold text-xs sm:text-[13px] truncate">Nova playlist</h3>
                <p class="text-[11px] text-[#6E6885] dark:text-[#9B94B2] truncate mt-0.5">
                  Tocar URL / ID direto
                </p>
              </div>

              <!-- Card 2: Músicas curtidas (Playlist automática) -->
              <div
                @click="switchTab('liked')"
                class="group cursor-pointer flex flex-col"
              >
                <div
                  class="w-full aspect-square rounded-2xl mb-2.5 relative overflow-hidden bg-gradient-to-br from-[#D9C2FF] via-[#F2C6DE] to-[#FAD0C4] shadow-sm group-hover:shadow-md group-hover:-translate-y-0.5 transition-all border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07]"
                >
                  <button
                    type="button"
                    @click.stop="handlePlayFromList(player.likedTracks, 0)"
                    title="Tocar Músicas curtidas"
                    class="absolute bottom-2.5 right-2.5 w-8 h-8 rounded-full bg-white/90 dark:bg-[#1E1A2D]/90 text-[#1A162B] dark:text-[#F4F2FA] flex items-center justify-center shadow-md group-hover:scale-110 transition-transform"
                  >
                    <Play class="w-3.5 h-3.5 fill-current ml-0.5" />
                  </button>
                </div>
                <h3 class="font-semibold text-xs sm:text-[13px] truncate">Músicas curtidas</h3>
                <p class="text-[11px] text-[#6E6885] dark:text-[#9B94B2] truncate mt-0.5">
                  Playlist automática
                </p>
              </div>

              <!-- User Playlists Cards -->
              <div
                v-for="(pl, pIdx) in player.userPlaylists"
                :key="pl.playlist_id + '-' + pIdx"
                @click="handleSelectPlaylist(pl)"
                class="group cursor-pointer flex flex-col"
              >
                <div
                  :class="[
                    'w-full aspect-square rounded-2xl mb-2.5 relative overflow-hidden bg-gradient-to-br shadow-sm group-hover:shadow-md group-hover:-translate-y-0.5 transition-all border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07]',
                    getPastelGradient(pl.playlist_id + pl.title + pIdx),
                  ]"
                >
                  <img
                    v-if="pl.thumbnail_url && !player.lowRamNoImages"
                    :src="pl.thumbnail_url"
                    :alt="pl.title"
                    loading="lazy"
                    decoding="async"
                    class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-500"
                  />
                  <button
                    type="button"
                    @click.stop="handlePlayPlaylistImmediate(pl)"
                    :title="`Tocar ${pl.title}`"
                    class="absolute bottom-2.5 right-2.5 w-8 h-8 rounded-full bg-white/90 dark:bg-[#1E1A2D]/90 text-[#1A162B] dark:text-[#F4F2FA] flex items-center justify-center shadow-md group-hover:scale-110 transition-transform"
                  >
                    <Play class="w-3.5 h-3.5 fill-current ml-0.5" />
                  </button>
                </div>
                <h3 class="font-semibold text-xs sm:text-[13px] truncate">{{ pl.title }}</h3>
                <p class="text-[11px] text-[#6E6885] dark:text-[#9B94B2] truncate mt-0.5">
                  {{ pl.track_count_text || pl.subtitle }}
                </p>
              </div>
            </section>

            <!-- Selected Playlist Tracks Drawer/Section -->
            <section v-if="player.selectedPlaylistTracks.length > 0" class="space-y-3 pt-2">
              <div class="flex items-center justify-between">
                <h2 class="text-base font-bold">
                  Faixas de {{ player.selectedPlaylistTitle }}
                </h2>
                <button
                  type="button"
                  @click="handlePlayFromList(player.selectedPlaylistTracks, 0)"
                  class="px-4 py-2 rounded-full bg-[#7C5CFC] hover:bg-[#6D4AFF] text-white text-xs font-semibold flex items-center gap-1.5 shadow-sm transition-colors"
                >
                  <Play class="w-3.5 h-3.5 fill-current" />
                  <span>Tocar tudo</span>
                </button>
              </div>

              <div
                class="rounded-3xl bg-white/80 dark:bg-[#1E1A2D]/80 border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07] p-2 divide-y divide-[rgba(120,100,180,0.08)] dark:divide-white/[0.05]"
              >
                <TrackCard
                  v-for="(track, idx) in player.selectedPlaylistTracks"
                  :key="track.video_id + '-pl-' + idx"
                  :track="track"
                  mode="row"
                  :index="idx"
                  :show-album="true"
                  @play="handlePlayFromList(player.selectedPlaylistTracks, idx)"
                />
              </div>
            </section>
          </template>

          <!-- ==================== TELA 5: MÚSICAS CURTIDAS ==================== -->
          <template v-else-if="activeTab === 'liked'">
            <!-- Hero Banner in Pink / Lavender Pastel Gradient -->
            <section
              class="relative overflow-hidden rounded-3xl p-6 sm:p-8 bg-gradient-to-r from-[#EFE6FF] via-[#FCE7F3] to-[#FFE8DF] dark:from-[#261E3F] dark:via-[#311F38] dark:to-[#362332] border border-[rgba(120,100,180,0.14)] dark:border-white/[0.08] flex items-center justify-between gap-6"
            >
              <div class="space-y-2 z-10">
                <span
                  class="text-[10px] font-bold uppercase tracking-widest text-[#7C5CFC] dark:text-[#C4B5FD]"
                >
                  SUA BIBLIOTECA
                </span>
                <h1 class="text-2xl sm:text-4xl font-bold tracking-tight">
                  Músicas curtidas
                </h1>
                <p class="text-xs text-[#6E6885] dark:text-[#9B94B2]">
                  Sincronizado com sua conta • {{ player.likedTracks.length }} músicas
                </p>

                <div class="pt-3 flex items-center gap-3">
                  <button
                    type="button"
                    @click="handlePlayFromList(player.likedTracks, 0)"
                    class="px-5 py-2.5 rounded-full bg-[#7C5CFC] hover:bg-[#6D4AFF] text-white text-xs font-semibold flex items-center gap-2 shadow-md shadow-[#7C5CFC]/20 transition-colors"
                  >
                    <Play class="w-3.5 h-3.5 fill-current" />
                    <span>Tocar tudo</span>
                  </button>

                  <button
                    type="button"
                    @click="
                      () => {
                        handlePlayFromList(player.likedTracks, 0)
                        player.shuffleQueue()
                      }
                    "
                    class="px-5 py-2.5 rounded-full bg-white/90 dark:bg-[#1E1A2D]/90 hover:bg-white dark:hover:bg-[#28223B] text-[#1A162B] dark:text-[#F4F2FA] border border-[rgba(120,100,180,0.14)] dark:border-white/10 text-xs font-semibold transition-colors"
                  >
                    Aleatório
                  </button>
                </div>
              </div>

              <!-- Decorative Stacked Pastel Cards Artwork on the Right (matches mockup) -->
              <div class="hidden sm:flex items-center pr-6 select-none pointer-events-none">
                <div
                  class="w-20 h-20 rounded-2xl bg-gradient-to-br from-[#B9F3E4] to-[#C2D9FF] shadow-md -rotate-12 translate-x-4"
                ></div>
                <div
                  class="w-20 h-20 rounded-2xl bg-gradient-to-br from-[#D9C2FF] to-[#B8C0FF] shadow-md rotate-3 z-10"
                ></div>
                <div
                  class="w-20 h-20 rounded-2xl bg-gradient-to-br from-[#FAD0C4] to-[#FDEBC8] shadow-md rotate-12 -translate-x-4 z-20"
                ></div>
              </div>
            </section>

            <!-- Numbered List of Liked Tracks -->
            <section
              class="rounded-3xl bg-white/80 dark:bg-[#1E1A2D]/80 border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07] p-2 divide-y divide-[rgba(120,100,180,0.08)] dark:divide-white/[0.05]"
            >
              <TrackCard
                v-for="(track, idx) in player.likedTracks"
                :key="track.video_id + '-liked-' + idx"
                :track="track"
                mode="row"
                :index="idx"
                :show-album="true"
                @play="handlePlayFromList(player.likedTracks, idx)"
              />
            </section>
          </template>
        </div>
      </main>

      <!-- Right Slide-over Panel (A seguir / Letra / Relacionados) -->
      <QueueDrawer />
    </div>

    <!-- ==================== TELA 6: TOCANDO AGORA (TELA CHEIA) ==================== -->
    <div
      v-if="player.isFullscreenPlayer"
      class="fixed inset-0 z-40 flex flex-col bg-[#F7F6FA] dark:bg-[#12101B] text-[#1A162B] dark:text-[#F4F2FA] overflow-hidden transition-colors duration-250"
    >
      <!-- Ambient Pastel Background Glow -->
      <div
        class="pointer-events-none absolute inset-0 bg-[radial-gradient(circle_at_25%_35%,_rgba(217,194,255,0.45),_transparent_55%),radial-gradient(circle_at_75%_65%,_rgba(250,208,196,0.3),_transparent_55%)] dark:bg-[radial-gradient(circle_at_25%_35%,_rgba(139,110,255,0.18),_transparent_55%),radial-gradient(circle_at_75%_65%,_rgba(244,162,184,0.12),_transparent_55%)]"
      ></div>

      <!-- Fullscreen Top Bar -->
      <div class="relative z-10 h-16 px-6 sm:px-10 flex items-center justify-between shrink-0">
        <!-- Minimize Button -->
        <button
          type="button"
          @click="player.isFullscreenPlayer = false"
          title="Minimizar tela cheia"
          class="w-9 h-9 rounded-full bg-white/90 dark:bg-[#1E1A2D] border border-[rgba(120,100,180,0.14)] dark:border-white/10 flex items-center justify-center text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white shadow-sm transition-transform hover:scale-105"
        >
          <ChevronDown class="w-4 h-4" />
        </button>

        <!-- Center Playing Source Label -->
        <div class="text-center">
          <p class="text-[10px] font-bold uppercase tracking-widest text-[#6E6885] dark:text-[#9B94B2]">
            TOCANDO DE
          </p>
          <p class="text-xs font-bold mt-0.5">
            Rádio • {{ player.currentTrackArtist }}
          </p>
        </div>

        <!-- Right Theme Toggle & Options -->
        <div class="flex items-center gap-2">
          <button
            type="button"
            @click="player.toggleTheme()"
            :title="player.theme === 'dark' ? 'Tema Claro' : 'Tema Escuro'"
            class="w-9 h-9 rounded-full bg-white/90 dark:bg-[#1E1A2D] border border-[rgba(120,100,180,0.14)] dark:border-white/10 flex items-center justify-center text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white shadow-sm"
          >
            <Moon v-if="player.theme === 'light'" class="w-4 h-4" />
            <Sun v-else class="w-4 h-4 text-[#FDEBC8]" />
          </button>

          <button
            type="button"
            @click="showCustomStreamModal = true"
            title="Mais opções / Stream direto"
            class="w-9 h-9 rounded-full bg-white/90 dark:bg-[#1E1A2D] border border-[rgba(120,100,180,0.14)] dark:border-white/10 flex items-center justify-center text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white shadow-sm"
          >
            <MoreHorizontal class="w-4 h-4" />
          </button>
        </div>
      </div>

      <!-- Fullscreen 2-Column Body -->
      <div
        class="relative z-10 flex-1 max-w-6xl w-full mx-auto px-6 sm:px-10 pb-8 grid grid-cols-1 lg:grid-cols-12 gap-8 items-center overflow-hidden"
      >
        <!-- Left Column: Giant Cover + Track Info + Progress + Controls -->
        <div class="lg:col-span-7 flex flex-col items-center justify-center max-w-md mx-auto w-full">
          <div
            :class="[
              'w-60 h-60 sm:w-72 sm:h-72 rounded-3xl overflow-hidden shadow-2xl bg-gradient-to-br border border-white/40 dark:border-white/10',
              getPastelGradient(player.videoId + player.currentTrackTitle),
            ]"
          >
            <img
              v-if="player.currentThumbnailUrl && !player.lowRamNoImages"
              :src="player.currentThumbnailUrl"
              :alt="player.currentTrackTitle"
              class="w-full h-full object-cover"
            />
          </div>

          <!-- Title, Artist & Heart -->
          <div class="w-full flex items-center justify-between mt-6 gap-4">
            <div class="min-w-0">
              <h2 class="text-xl sm:text-2xl font-bold tracking-tight truncate">
                {{ player.currentTrackTitle }}
              </h2>
              <p class="text-xs text-[#6E6885] dark:text-[#9B94B2] truncate mt-0.5">
                {{ player.currentTrackArtist }} • OutRun
              </p>
            </div>

            <button
              type="button"
              @click="player.toggleLikeTrack()"
              :class="[
                'w-9 h-9 rounded-full flex items-center justify-center transition-colors shrink-0',
                player.isTrackLiked(player.videoId)
                  ? 'text-[#EC4899] bg-[#EC4899]/10'
                  : 'text-[#6E6885] dark:text-[#9B94B2] hover:text-[#EC4899]',
              ]"
            >
              <Heart
                :class="[
                  'w-4 h-4',
                  player.isTrackLiked(player.videoId) ? 'fill-current' : '',
                ]"
              />
            </button>
          </div>

          <!-- Full Seek Bar -->
          <div class="w-full mt-4 space-y-1.5">
            <input
              type="range"
              :min="0"
              :max="player.durationSecs || 1"
              step="0.5"
              :value="player.positionSecs"
              @input="handleFullscreenSeek"
              class="w-full h-1.5 bg-[#DFD9F0] dark:bg-[#2B2540] rounded-full appearance-none cursor-pointer accent-[#7C5CFC]"
            />
            <div class="flex items-center justify-between text-[11px] text-[#6E6885] dark:text-[#9B94B2] tabular-nums">
              <span>{{ formatTime(player.positionSecs) }}</span>
              <span>{{ formatTime(player.durationSecs) }}</span>
            </div>
          </div>

          <!-- Large Playback Controls -->
          <div class="flex items-center justify-center gap-6 mt-3">
            <button
              type="button"
              @click="player.shuffleQueue()"
              class="p-2 text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white transition-colors"
            >
              <Shuffle class="w-4 h-4" />
            </button>

            <button
              type="button"
              @click="player.prevTrack()"
              class="p-2 text-[#1A162B] dark:text-[#F4F2FA] hover:scale-110 transition-transform"
            >
              <SkipBack class="w-5 h-5 fill-current" />
            </button>

            <button
              type="button"
              @click="player.togglePlay()"
              class="w-14 h-14 rounded-full bg-[#B8A2FF] hover:bg-[#A68BFF] text-[#1A162B] flex items-center justify-center shadow-lg shadow-[#7C5CFC]/25 hover:scale-105 transition-transform"
            >
              <Loader2 v-if="player.isLoading" class="w-6 h-6 animate-spin" />
              <Pause v-else-if="player.isPlaying" class="w-6 h-6 fill-current" />
              <Play v-else class="w-6 h-6 fill-current ml-0.5" />
            </button>

            <button
              type="button"
              @click="player.nextTrack()"
              class="p-2 text-[#1A162B] dark:text-[#F4F2FA] hover:scale-110 transition-transform"
            >
              <SkipForward class="w-5 h-5 fill-current" />
            </button>

            <button
              type="button"
              @click="player.cycleRepeatMode()"
              :class="[
                'p-2 rounded-full transition-colors',
                player.repeatMode !== 'off'
                  ? 'text-[#7C5CFC] dark:text-[#B497FF] bg-[#7C5CFC]/15'
                  : 'text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white',
              ]"
            >
              <Repeat1 v-if="player.repeatMode === 'one'" class="w-4 h-4" />
              <Repeat v-else class="w-4 h-4" />
            </button>
          </div>

          <!-- Bottom Utility Pills: Letra + Rádio ligada + Volume -->
          <div class="flex items-center justify-center gap-3 mt-5 flex-wrap">
            <button
              type="button"
              @click="player.setRightPanelTab('lyrics')"
              :class="[
                'px-3.5 py-1.5 rounded-full text-xs font-semibold flex items-center gap-1.5 border transition-colors',
                player.rightPanelTab === 'lyrics'
                  ? 'bg-[#7C5CFC]/15 border-[#7C5CFC]/35 text-[#7C5CFC] dark:text-[#C4B5FD]'
                  : 'bg-white/80 dark:bg-[#1E1A2D] border-[rgba(120,100,180,0.14)] dark:border-white/10 text-[#6E6885] dark:text-[#9B94B2]',
              ]"
            >
              <MessageSquareQuote class="w-3.5 h-3.5" />
              <span>Letra</span>
            </button>

            <button
              type="button"
              @click="player.toggleAutoplayRadio()"
              :class="[
                'px-3.5 py-1.5 rounded-full text-xs font-semibold flex items-center gap-1.5 border transition-colors',
                player.autoplayRadio
                  ? 'bg-[#22C55E]/15 border-[#22C55E]/35 text-[#15803D] dark:text-[#4ADE80]'
                  : 'bg-white/80 dark:bg-[#1E1A2D] border-[rgba(120,100,180,0.14)] dark:border-white/10 text-[#6E6885] dark:text-[#9B94B2]',
              ]"
            >
              <Radio class="w-3.5 h-3.5" />
              <span>Rádio {{ player.autoplayRadio ? 'ligada' : 'desligada' }}</span>
            </button>

            <div
              class="flex items-center gap-2 px-3 py-1.5 rounded-full bg-white/80 dark:bg-[#1E1A2D] border border-[rgba(120,100,180,0.14)] dark:border-white/10"
            >
              <VolumeX v-if="player.volume === 0" class="w-3.5 h-3.5 text-rose-400" />
              <Volume2 v-else class="w-3.5 h-3.5 text-[#6E6885] dark:text-[#9B94B2]" />
              <input
                type="range"
                min="0"
                max="1"
                step="0.01"
                :value="player.volume"
                @input="handleFullscreenVolume"
                class="w-16 h-1 bg-[#DFD9F0] dark:bg-[#2B2540] rounded-full appearance-none cursor-pointer accent-[#7C5CFC]"
              />
            </div>
          </div>
        </div>

        <!-- Right Column: Embedded Queue / Lyrics / Related Card -->
        <div class="lg:col-span-5 h-full max-h-[520px] flex flex-col overflow-hidden">
          <QueueDrawer :embedded="true" />
        </div>
      </div>
    </div>

    <!-- Custom YouTube URL / Video ID Modal (triggered from "+ Nova playlist" in Library) -->
    <div
      v-if="showCustomStreamModal"
      class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm flex items-center justify-center p-4"
    >
      <div
        class="w-full max-w-md rounded-3xl bg-white dark:bg-[#1E1A2D] border border-[rgba(120,100,180,0.16)] dark:border-white/10 shadow-2xl p-6 space-y-4"
      >
        <div class="flex items-center justify-between">
          <h3 class="text-base font-bold">Tocar Stream ou URL do YouTube</h3>
          <button
            type="button"
            @click="showCustomStreamModal = false"
            class="p-1 rounded-full text-[#6E6885] hover:text-[#1A162B] dark:hover:text-white"
          >
            <X class="w-4 h-4" />
          </button>
        </div>
        <p class="text-xs text-[#6E6885] dark:text-[#9B94B2]">
          Cole uma URL do YouTube Music ou ID de vídeo de 11 caracteres para reproduzir diretamente
          no buffer RAM em Rust:
        </p>
        <form @submit.prevent="handlePlayCustomYoutube" class="space-y-3">
          <input
            v-model="youtubeInputUrl"
            type="text"
            placeholder="Ex: https://music.youtube.com/watch?v=MV_3Dpw-BRY"
            class="w-full px-4 py-2.5 rounded-2xl text-xs bg-[#F7F6FA] dark:bg-[#161321] border border-[rgba(120,100,180,0.16)] dark:border-white/10 focus:border-[#7C5CFC] focus:outline-none"
          />
          <div class="flex justify-end gap-2">
            <button
              type="button"
              @click="showCustomStreamModal = false"
              class="px-4 py-2 rounded-full text-xs font-semibold text-[#6E6885] hover:text-[#1A162B] dark:hover:text-white"
            >
              Cancelar
            </button>
            <button
              type="submit"
              class="px-5 py-2 rounded-full bg-[#7C5CFC] hover:bg-[#6D4AFF] text-white text-xs font-semibold shadow-sm"
            >
              Reproduzir agora
            </button>
          </div>
        </form>
      </div>
    </div>

    <!-- Google Auth Modal -->
    <AuthModal :open="showAuthModal" @close="showAuthModal = false" />

    <!-- Bottom Floating Dark Pill Player ("Player do C") -->
    <PlayerBar @open-lyrics-view="handleOpenLyricsView" />
  </div>
</template>
