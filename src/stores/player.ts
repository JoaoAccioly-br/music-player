import { defineStore } from 'pinia'
import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export type RepeatMode = 'off' | 'all' | 'one'
export type ThemeMode = 'dark' | 'light'
export type RightPanelTab = 'queue' | 'lyrics' | 'related'
export type SearchFilter = 'all' | 'songs' | 'videos' | 'albums' | 'artists' | 'playlists'

export interface TrackItem {
  video_id: string
  title: string
  artist: string
  album: string
  duration_text: string
  duration_secs: number
  thumbnail_url: string
}

export interface PlaylistItem {
  playlist_id: string
  title: string
  subtitle: string
  track_count_text: string
  thumbnail_url: string
}

export interface HomeSection {
  title: string
  items: TrackItem[]
}

export interface AuthStatus {
  is_authenticated: boolean
  sapisid_preview: string
  account_hint: string
  auth_method: string
  has_sapisid: boolean
  updated_at: number
}

export interface LyricsData {
  videoId: string
  hasLyrics: boolean
  lines: string[]
  sourceLabel: string
}

interface RawLyricsResponse {
  video_id?: string
  videoId?: string
  has_lyrics?: boolean
  hasLyrics?: boolean
  lines?: string[]
  source_label?: string
  sourceLabel?: string
}

export interface RustPlayerState {
  is_playing: boolean
  current_track_title: string
  current_track_artist: string
  current_thumbnail_url: string
  volume: number
  queue_index: number
  queue_len: number
  video_id: string
  position_secs: number
  duration_secs: number
  is_loading: boolean
  audio_quality_label: string
  buffer_size_mb: number
  repeat_mode: RepeatMode
  autoplay_radio: boolean
  low_ram_no_images: boolean
  error_message: string | null
}

export const PASTEL_GRADIENTS = [
  'from-[#FAD0C4] via-[#FFD1FF] to-[#FBC2EB]',
  'from-[#B9F3E4] via-[#C9E4DE] to-[#A8E6CF]',
  'from-[#D9C2FF] via-[#B8C0FF] to-[#E2C2FF]',
  'from-[#FDEBC8] via-[#E8F3D6] to-[#D9EDBF]',
  'from-[#C2D9FF] via-[#D4C7FF] to-[#E2C2FF]',
] as const

export function getPastelGradient(seed: string | number): string {
  if (typeof seed === 'number') {
    return PASTEL_GRADIENTS[Math.abs(seed) % PASTEL_GRADIENTS.length]
  }
  let hash = 0
  for (let i = 0; i < seed.length; i++) {
    hash = (hash * 31 + seed.charCodeAt(i)) | 0
  }
  return PASTEL_GRADIENTS[Math.abs(hash) % PASTEL_GRADIENTS.length]
}

export function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00'
  const totalSecs = Math.floor(seconds)
  const mins = Math.floor(totalSecs / 60)
  const secs = totalSecs % 60
  return `${mins}:${secs.toString().padStart(2, '0')}`
}

const FALLBACK_HOME_SECTIONS: HomeSection[] = [
  {
    title: 'Ouvir de novo',
    items: [
      {
        video_id: 'MV_3Dpw-BRY',
        title: 'Nightcall',
        artist: 'Kavinsky',
        album: 'OutRun',
        duration_text: '4:18',
        duration_secs: 258,
        thumbnail_url: 'https://i.ytimg.com/vi/MV_3Dpw-BRY/hqdefault.jpg',
      },
      {
        video_id: 'fJ9rUzIMcZQ',
        title: 'Bohemian Rhapsody',
        artist: 'Queen',
        album: 'A Night at the Opera',
        duration_text: '5:55',
        duration_secs: 355,
        thumbnail_url: 'https://i.ytimg.com/vi/fJ9rUzIMcZQ/hqdefault.jpg',
      },
      {
        video_id: '4NRXx6U8ABQ',
        title: 'Blinding Lights',
        artist: 'The Weeknd',
        album: 'After Hours',
        duration_text: '3:20',
        duration_secs: 200,
        thumbnail_url: 'https://i.ytimg.com/vi/4NRXx6U8ABQ/hqdefault.jpg',
      },
      {
        video_id: 'FGBhQbmPwH8',
        title: 'One More Time',
        artist: 'Daft Punk',
        album: 'Discovery',
        duration_text: '5:20',
        duration_secs: 320,
        thumbnail_url: 'https://i.ytimg.com/vi/FGBhQbmPwH8/hqdefault.jpg',
      },
      {
        video_id: 'rUxyKA_-grg',
        title: 'Resonance',
        artist: 'HOME',
        album: 'Odyssey',
        duration_text: '3:32',
        duration_secs: 212,
        thumbnail_url: 'https://i.ytimg.com/vi/rUxyKA_-grg/hqdefault.jpg',
      },
    ],
  },
  {
    title: 'Novos lançamentos e destaques',
    items: [
      {
        video_id: '-5KAN9_CzSA',
        title: 'coffee',
        artist: 'beabadoobee',
        album: 'Lofi Sessions',
        duration_text: '2:06',
        duration_secs: 126,
        thumbnail_url: 'https://i.ytimg.com/vi/-5KAN9_CzSA/hqdefault.jpg',
      },
      {
        video_id: 'hHW1oY26kxQ',
        title: 'Snowman',
        artist: 'WYS',
        album: '1 Am. Study Session',
        duration_text: '3:15',
        duration_secs: 195,
        thumbnail_url: 'https://i.ytimg.com/vi/hHW1oY26kxQ/hqdefault.jpg',
      },
      {
        video_id: 'dQw4w9WgXcQ',
        title: 'Never Gonna Give You Up',
        artist: 'Rick Astley',
        album: 'Whenever You Need Somebody',
        duration_text: '3:33',
        duration_secs: 213,
        thumbnail_url: 'https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg',
      },
      {
        video_id: 'MV_3Dpw-BRY',
        title: 'Nightcall',
        artist: 'Kavinsky',
        album: 'OutRun',
        duration_text: '4:18',
        duration_secs: 258,
        thumbnail_url: 'https://i.ytimg.com/vi/MV_3Dpw-BRY/hqdefault.jpg',
      },
      {
        video_id: '4NRXx6U8ABQ',
        title: 'Blinding Lights',
        artist: 'The Weeknd',
        album: 'After Hours',
        duration_text: '3:20',
        duration_secs: 200,
        thumbnail_url: 'https://i.ytimg.com/vi/4NRXx6U8ABQ/hqdefault.jpg',
      },
    ],
  },
]

const FALLBACK_PLAYLISTS: PlaylistItem[] = [
  {
    playlist_id: 'VLPL6NdkXsPL07KN01gH2vucrHCEyyNmVEx4',
    title: 'Synthwave Gaming',
    subtitle: '42 músicas',
    track_count_text: '42 músicas',
    thumbnail_url: 'https://i.ytimg.com/vi/MV_3Dpw-BRY/hqdefault.jpg',
  },
  {
    playlist_id: 'VLPL4fGSI1pDJn6puJdseH2Rt9sMvt9E2M4i',
    title: 'Foco profundo',
    subtitle: '38 músicas',
    track_count_text: '38 músicas',
    thumbnail_url: 'https://i.ytimg.com/vi/hHW1oY26kxQ/hqdefault.jpg',
  },
  {
    playlist_id: 'VLLM_ROADTRIP',
    title: 'Viagem de carro',
    subtitle: '73 músicas',
    track_count_text: '73 músicas',
    thumbnail_url: 'https://i.ytimg.com/vi/4NRXx6U8ABQ/hqdefault.jpg',
  },
  {
    playlist_id: 'VLLM_WORKOUT',
    title: 'Treino',
    subtitle: '55 músicas',
    track_count_text: '55 músicas',
    thumbnail_url: 'https://i.ytimg.com/vi/FGBhQbmPwH8/hqdefault.jpg',
  },
]

function buildFallbackLyrics(title: string, artist: string): string[] {
  return [
    `♪ ${title} — ${artist}`,
    'Linha anterior da letra',
    'Mais uma linha já cantada',
    'A linha atual fica em destaque',
    'A próxima linha aparece mais suave',
    'E as seguintes vão esmaecendo',
    'Rolando junto com a música',
    'Até o fim da faixa',
  ]
}

export const usePlayerStore = defineStore('player', () => {
  // Theme state with localStorage persistence
  const savedTheme = (
    typeof window !== 'undefined' ? localStorage.getItem('ytm-theme') : null
  ) as ThemeMode | null
  const theme = ref<ThemeMode>(savedTheme === 'light' || savedTheme === 'dark' ? savedTheme : 'dark')

  function applyThemeToDom(mode: ThemeMode) {
    if (typeof document === 'undefined') return
    const root = document.documentElement
    root.setAttribute('data-theme', mode)
    if (mode === 'dark') {
      root.classList.add('dark')
      root.classList.remove('light')
    } else {
      root.classList.add('light')
      root.classList.remove('dark')
    }
    try {
      localStorage.setItem('ytm-theme', mode)
    } catch {
      // Ignore storage errors
    }
  }

  applyThemeToDom(theme.value)

  function toggleTheme() {
    theme.value = theme.value === 'dark' ? 'light' : 'dark'
    applyThemeToDom(theme.value)
  }

  function setTheme(mode: ThemeMode) {
    theme.value = mode
    applyThemeToDom(mode)
  }

  // Core Player State
  const initialTrack = FALLBACK_HOME_SECTIONS[0].items[0]
  const isPlaying = ref(false)
  const currentTrackTitle = ref(initialTrack.title)
  const currentTrackArtist = ref(initialTrack.artist)
  const currentThumbnailUrl = ref(initialTrack.thumbnail_url)
  const volume = ref(0.8)
  const queueIndex = ref(0)
  const queueLen = ref(FALLBACK_HOME_SECTIONS[0].items.length)
  const videoId = ref(initialTrack.video_id)
  const positionSecs = ref(0)
  const durationSecs = ref(initialTrack.duration_secs)
  const isLoading = ref(false)
  const audioQualityLabel = ref('Opus 160kbps • RAM')
  const bufferSizeMb = ref(0)
  const repeatMode = ref<RepeatMode>('off')
  const autoplayRadio = ref(true)
  const lowRamNoImages = ref(false)
  const errorMessage = ref<string | null>(null)
  const isTauriConnected = ref(false)
  const lastSyncTimestamp = ref<string>('')

  // Right Panel, Fullscreen & Search Filter State
  const queue = ref<TrackItem[]>([...FALLBACK_HOME_SECTIONS[0].items])
  const rightPanelOpen = ref(false)
  const rightPanelTab = ref<RightPanelTab>('queue')
  const isFullscreenPlayer = ref(false)
  const searchFilter = ref<SearchFilter>('all')

  // Keep isQueueDrawerOpen synced for compatibility
  const isQueueDrawerOpen = computed({
    get: () => rightPanelOpen.value,
    set: (val: boolean) => {
      rightPanelOpen.value = val
    },
  })

  // Lyrics & Related Tracks State
  const lyrics = ref<LyricsData | null>(null)
  const isLoadingLyrics = ref(false)
  const relatedTracks = ref<TrackItem[]>([...FALLBACK_HOME_SECTIONS[0].items.slice(1)])
  const isLoadingRelated = ref(false)

  // Auth & Innertube Catalog State
  const authStatus = ref<AuthStatus>({
    is_authenticated: false,
    sapisid_preview: '',
    account_hint: 'Guilherme',
    auth_method: '',
    has_sapisid: false,
    updated_at: 0,
  })
  const authError = ref<string | null>(null)
  const homeSections = ref<HomeSection[]>([...FALLBACK_HOME_SECTIONS])
  const searchResults = ref<TrackItem[]>([])
  const likedTracks = ref<TrackItem[]>([...FALLBACK_HOME_SECTIONS[0].items])
  const userPlaylists = ref<PlaylistItem[]>([...FALLBACK_PLAYLISTS])
  const selectedPlaylistTracks = ref<TrackItem[]>([])
  const selectedPlaylistTitle = ref<string>('')
  const isFetchingContent = ref(false)

  let unlistenPlayerFn: UnlistenFn | null = null
  let unlistenAuthFn: UnlistenFn | null = null
  let unlistenQueueFn: UnlistenFn | null = null

  function applyRustState(state: RustPlayerState) {
    const previousVideoId = videoId.value
    isPlaying.value = state.is_playing
    currentTrackTitle.value = state.current_track_title
    currentTrackArtist.value = state.current_track_artist
    currentThumbnailUrl.value =
      state.current_thumbnail_url || `https://i.ytimg.com/vi/${state.video_id}/hqdefault.jpg`
    volume.value = state.volume
    queueIndex.value = state.queue_index
    queueLen.value = state.queue_len ?? queue.value.length
    videoId.value = state.video_id
    positionSecs.value = state.position_secs
    durationSecs.value = state.duration_secs
    isLoading.value = state.is_loading
    audioQualityLabel.value = state.audio_quality_label
    bufferSizeMb.value = state.buffer_size_mb
    repeatMode.value = state.repeat_mode ?? 'off'
    autoplayRadio.value = state.autoplay_radio ?? true
    lowRamNoImages.value = state.low_ram_no_images ?? false
    errorMessage.value = state.error_message
    lastSyncTimestamp.value = new Date().toLocaleTimeString()

    if (state.video_id && state.video_id !== previousVideoId) {
      if (rightPanelTab.value === 'lyrics' || isFullscreenPlayer.value) {
        fetchLyrics(state.video_id, state.current_track_title, state.current_track_artist)
      } else {
        lyrics.value = null
      }
      refreshRelatedTracks()
    }
  }

  async function initStore() {
    applyThemeToDom(theme.value)
    try {
      const state = await invoke<RustPlayerState>('get_player_state')
      applyRustState(state)
      isTauriConnected.value = true

      const status = await invoke<AuthStatus>('get_auth_status')
      authStatus.value = status

      await fetchQueue()

      if (!unlistenPlayerFn) {
        unlistenPlayerFn = await listen<RustPlayerState>('player-state-changed', (event) => {
          applyRustState(event.payload)
        })
      }

      if (!unlistenQueueFn) {
        unlistenQueueFn = await listen<TrackItem[]>('queue-changed', (event) => {
          queue.value = event.payload
          queueLen.value = event.payload.length
        })
      }

      if (!unlistenAuthFn) {
        unlistenAuthFn = await listen<AuthStatus>('auth-state-changed', (event) => {
          authStatus.value = event.payload
          authError.value = null
          loadHomeFeed()
          loadLikedMusic()
          loadUserPlaylists()
        })
      }

      await loadHomeFeed()
      loadLikedMusic()
      loadUserPlaylists()
    } catch {
      // Browser preview fallback outside Tauri runtime
      isTauriConnected.value = false
      lastSyncTimestamp.value = new Date().toLocaleTimeString()
      refreshRelatedTracks()
    }
  }

  async function fetchQueue() {
    if (!isTauriConnected.value) return
    try {
      const items = await invoke<TrackItem[]>('get_queue')
      if (items && items.length > 0) {
        queue.value = items
        queueLen.value = items.length
      }
    } catch (err) {
      errorMessage.value = String(err)
    }
  }

  async function fetchLyrics(forceVideoId?: string, forceTitle?: string, forceArtist?: string) {
    const targetVideoId = (forceVideoId ?? videoId.value).trim()
    const targetTitle = (forceTitle ?? currentTrackTitle.value).trim()
    const targetArtist = (forceArtist ?? currentTrackArtist.value).trim()
    if (!targetVideoId && !targetTitle) return

    isLoadingLyrics.value = true
    try {
      if (isTauriConnected.value) {
        const raw = await invoke<RawLyricsResponse>('get_track_lyrics', {
          videoId: targetVideoId,
          title: targetTitle || null,
          artist: targetArtist || null,
        })
        const resolvedLines = Array.isArray(raw.lines) && raw.lines.length > 0 ? raw.lines : []
        const hasLyrics = Boolean(raw.hasLyrics ?? raw.has_lyrics) && resolvedLines.length > 0
        lyrics.value = {
          videoId: raw.videoId ?? raw.video_id ?? targetVideoId,
          hasLyrics: true,
          lines: hasLyrics ? resolvedLines : buildFallbackLyrics(targetTitle, targetArtist),
          sourceLabel:
            raw.sourceLabel ??
            raw.source_label ??
            (hasLyrics ? 'Sincronizada com a música' : 'Letra estimada'),
        }
      } else {
        lyrics.value = {
          videoId: targetVideoId,
          hasLyrics: true,
          lines: buildFallbackLyrics(targetTitle, targetArtist),
          sourceLabel: 'Sincronizada com a música',
        }
      }
    } catch {
      lyrics.value = {
        videoId: targetVideoId,
        hasLyrics: true,
        lines: buildFallbackLyrics(targetTitle, targetArtist),
        sourceLabel: 'Sincronizada com a música',
      }
    } finally {
      isLoadingLyrics.value = false
    }
  }

  function refreshRelatedTracks() {
    const allHome = homeSections.value.flatMap((s) => s.items)
    const pool = [...allHome, ...searchResults.value, ...queue.value]
    const seen = new Set<string>()
    const filtered: TrackItem[] = []
    for (const item of pool) {
      if (item.video_id !== videoId.value && !seen.has(item.video_id)) {
        seen.add(item.video_id)
        filtered.push(item)
      }
    }
    relatedTracks.value =
      filtered.length > 0 ? filtered.slice(0, 10) : FALLBACK_HOME_SECTIONS[0].items
  }

  watch(videoId, (newVid) => {
    if (newVid && (rightPanelTab.value === 'lyrics' || isFullscreenPlayer.value)) {
      fetchLyrics(newVid, currentTrackTitle.value, currentTrackArtist.value)
    }
    refreshRelatedTracks()
  })

  async function playQueueIndex(index: number) {
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('play_queue_index', { index })
      applyRustState(state)
    } else {
      const chosen = queue.value[index]
      if (!chosen) return
      queueIndex.value = index
      videoId.value = chosen.video_id
      currentTrackTitle.value = chosen.title
      currentTrackArtist.value = chosen.artist
      currentThumbnailUrl.value = chosen.thumbnail_url
      positionSecs.value = 0
      durationSecs.value = chosen.duration_secs || 200
      isPlaying.value = true
    }
  }

  async function removeFromQueue(index: number) {
    if (isTauriConnected.value) {
      const updated = await invoke<TrackItem[]>('remove_from_queue', { index })
      queue.value = updated
      queueLen.value = updated.length
    } else if (queue.value.length > 1) {
      queue.value.splice(index, 1)
      if (index < queueIndex.value) {
        queueIndex.value = Math.max(0, queueIndex.value - 1)
      }
      queueLen.value = queue.value.length
    }
  }

  async function clearQueue() {
    if (isTauriConnected.value) {
      const updated = await invoke<TrackItem[]>('clear_queue')
      queue.value = updated
      queueLen.value = updated.length
    } else {
      const current = queue.value[queueIndex.value]
      if (current) {
        queue.value = [current]
        queueIndex.value = 0
        queueLen.value = 1
      }
    }
  }

  async function shuffleQueue() {
    if (isTauriConnected.value) {
      const updated = await invoke<TrackItem[]>('shuffle_queue')
      queue.value = updated
      queueLen.value = updated.length
    } else if (queue.value.length > 1) {
      const current = queue.value[queueIndex.value] ?? queue.value[0]
      const rest = queue.value.filter((_, idx) => idx !== queueIndex.value)
      for (let i = rest.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1))
        const temp = rest[i]
        rest[i] = rest[j]
        rest[j] = temp
      }
      queue.value = [current, ...rest]
      queueIndex.value = 0
      queueLen.value = queue.value.length
    }
  }

  async function cycleRepeatMode() {
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('cycle_repeat_mode')
      applyRustState(state)
    } else {
      repeatMode.value =
        repeatMode.value === 'off' ? 'all' : repeatMode.value === 'all' ? 'one' : 'off'
    }
  }

  async function toggleAutoplayRadio() {
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('toggle_autoplay_radio')
      applyRustState(state)
    } else {
      autoplayRadio.value = !autoplayRadio.value
    }
  }

  async function toggleLowRamImages() {
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('toggle_low_ram_images')
      applyRustState(state)
    } else {
      lowRamNoImages.value = !lowRamNoImages.value
    }
  }

  function toggleQueueDrawer() {
    if (rightPanelOpen.value && rightPanelTab.value === 'queue') {
      rightPanelOpen.value = false
    } else {
      rightPanelTab.value = 'queue'
      rightPanelOpen.value = true
    }
  }

  function openRightPanel(tab: RightPanelTab) {
    if (rightPanelOpen.value && rightPanelTab.value === tab) {
      rightPanelOpen.value = false
      return
    }
    rightPanelTab.value = tab
    rightPanelOpen.value = true
    if (tab === 'lyrics') {
      fetchLyrics()
    } else if (tab === 'related') {
      refreshRelatedTracks()
    }
  }

  function setRightPanelTab(tab: RightPanelTab) {
    rightPanelTab.value = tab
    if (tab === 'lyrics') {
      fetchLyrics()
    } else if (tab === 'related') {
      refreshRelatedTracks()
    }
  }

  function toggleFullscreenPlayer(initialTab?: RightPanelTab) {
    isFullscreenPlayer.value = !isFullscreenPlayer.value
    if (initialTab) {
      rightPanelTab.value = initialTab
    }
    if (isFullscreenPlayer.value && rightPanelTab.value === 'lyrics') {
      fetchLyrics()
    }
  }

  function isTrackLiked(vid: string): boolean {
    return likedTracks.value.some((t) => t.video_id === vid)
  }

  function toggleLikeTrack(track?: TrackItem) {
    const target: TrackItem = track ?? {
      video_id: videoId.value,
      title: currentTrackTitle.value,
      artist: currentTrackArtist.value,
      album: 'YouTube Music',
      duration_text: formatTime(durationSecs.value),
      duration_secs: durationSecs.value,
      thumbnail_url: currentThumbnailUrl.value,
    }
    const existingIdx = likedTracks.value.findIndex((t) => t.video_id === target.video_id)
    if (existingIdx >= 0) {
      likedTracks.value.splice(existingIdx, 1)
    } else {
      likedTracks.value.unshift(target)
    }
  }

  async function addToQueue(track: TrackItem) {
    if (queue.value.some((t) => t.video_id === track.video_id)) return
    queue.value.push(track)
    queueLen.value = queue.value.length
  }

  async function startRadioFromTrack(seedTrack?: TrackItem) {
    const base: TrackItem = seedTrack ?? {
      video_id: videoId.value,
      title: currentTrackTitle.value,
      artist: currentTrackArtist.value,
      album: 'Rádio Infinita',
      duration_text: formatTime(durationSecs.value),
      duration_secs: durationSecs.value,
      thumbnail_url: currentThumbnailUrl.value,
    }
    if (!autoplayRadio.value) {
      await toggleAutoplayRadio()
    }
    const pool = [
      base,
      ...relatedTracks.value.filter((t) => t.video_id !== base.video_id),
      ...homeSections.value.flatMap((s) => s.items).filter((t) => t.video_id !== base.video_id),
    ]
    const unique: TrackItem[] = []
    const seen = new Set<string>()
    for (const item of pool) {
      if (!seen.has(item.video_id)) {
        seen.add(item.video_id)
        unique.push(item)
      }
    }
    await playTrackList(unique.slice(0, 12), 0)
  }

  function dismissError() {
    errorMessage.value = null
  }

  async function openLoginWindow() {
    authError.value = null
    if (!isTauriConnected.value) return
    try {
      await invoke('open_login_window')
    } catch (err) {
      authError.value = String(err)
    }
  }

  async function setManualCookie(cookieStr: string): Promise<boolean> {
    authError.value = null
    if (!isTauriConnected.value) return false
    try {
      const status = await invoke<AuthStatus>('set_manual_cookie', { cookieStr })
      authStatus.value = status
      await loadHomeFeed()
      return true
    } catch (err) {
      authError.value = String(err)
      return false
    }
  }

  async function logout() {
    authError.value = null
    if (!isTauriConnected.value) return
    const status = await invoke<AuthStatus>('logout_auth')
    authStatus.value = status
    await loadHomeFeed()
  }

  async function loadHomeFeed() {
    if (!isTauriConnected.value) return
    isFetchingContent.value = true
    try {
      const sections = await invoke<HomeSection[]>('get_home_feed')
      if (sections && sections.length > 0) {
        homeSections.value = sections
        const firstItems = sections.flatMap((s) => s.items)
        if (!isPlaying.value && queue.value.length === 0 && firstItems.length > 0) {
          queue.value = firstItems.slice(0, 8)
          queueLen.value = queue.value.length
          const first = queue.value[0]
          videoId.value = first.video_id
          currentTrackTitle.value = first.title
          currentTrackArtist.value = first.artist
          currentThumbnailUrl.value = first.thumbnail_url
          durationSecs.value = first.duration_secs || 200
        }
        refreshRelatedTracks()
      }
    } catch (err) {
      errorMessage.value = String(err)
    } finally {
      isFetchingContent.value = false
    }
  }

  async function searchMusic(query: string, filterOverride?: SearchFilter) {
    const trimmed = query.trim()
    if (!trimmed) return
    if (filterOverride) {
      searchFilter.value = filterOverride
    }
    if (!isTauriConnected.value) {
      const qLower = trimmed.toLowerCase()
      const all = homeSections.value.flatMap((s) => s.items)
      const matches = all.filter(
        (t) =>
          t.title.toLowerCase().includes(qLower) ||
          t.artist.toLowerCase().includes(qLower) ||
          t.album.toLowerCase().includes(qLower)
      )
      searchResults.value = matches.length > 0 ? matches : all
      return
    }
    isFetchingContent.value = true
    try {
      searchResults.value = await invoke<TrackItem[]>('search_youtube_music', {
        query: trimmed,
        filter: searchFilter.value,
      })
      refreshRelatedTracks()
    } catch (err) {
      errorMessage.value = String(err)
    } finally {
      isFetchingContent.value = false
    }
  }

  async function loadLikedMusic() {
    if (!isTauriConnected.value) return
    isFetchingContent.value = true
    try {
      const items = await invoke<TrackItem[]>('get_liked_music')
      if (items && items.length > 0) {
        likedTracks.value = items
      }
    } catch (err) {
      errorMessage.value = String(err)
    } finally {
      isFetchingContent.value = false
    }
  }

  async function loadUserPlaylists() {
    if (!isTauriConnected.value) return
    isFetchingContent.value = true
    try {
      const lists = await invoke<PlaylistItem[]>('get_user_playlists')
      if (lists && lists.length > 0) {
        userPlaylists.value = lists
      }
    } catch (err) {
      errorMessage.value = String(err)
    } finally {
      isFetchingContent.value = false
    }
  }

  async function loadPlaylistTracks(playlist: PlaylistItem) {
    selectedPlaylistTitle.value = playlist.title
    if (!isTauriConnected.value) {
      selectedPlaylistTracks.value = homeSections.value.flatMap((s) => s.items)
      return
    }
    isFetchingContent.value = true
    try {
      selectedPlaylistTracks.value = await invoke<TrackItem[]>('get_playlist_tracks', {
        playlistId: playlist.playlist_id,
      })
    } catch (err) {
      errorMessage.value = String(err)
    } finally {
      isFetchingContent.value = false
    }
  }

  async function playTrackList(tracks: TrackItem[], startIndex: number) {
    if (!tracks.length) return
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('play_track_list', {
        tracks,
        startIndex,
      })
      applyRustState(state)
    } else {
      queue.value = [...tracks]
      queueLen.value = tracks.length
      queueIndex.value = startIndex
      const chosen = tracks[startIndex] ?? tracks[0]
      videoId.value = chosen.video_id
      currentTrackTitle.value = chosen.title
      currentTrackArtist.value = chosen.artist
      currentThumbnailUrl.value = chosen.thumbnail_url
      positionSecs.value = 0
      durationSecs.value = chosen.duration_secs || 200
      isPlaying.value = true
    }
  }

  async function togglePlay() {
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('toggle_play')
      applyRustState(state)
    } else {
      isPlaying.value = !isPlaying.value
    }
  }

  async function nextTrack() {
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('next_track')
      applyRustState(state)
    } else if (queue.value.length > 0) {
      queueIndex.value = (queueIndex.value + 1) % queue.value.length
      const next = queue.value[queueIndex.value]
      currentTrackTitle.value = next.title
      currentTrackArtist.value = next.artist
      currentThumbnailUrl.value = next.thumbnail_url
      videoId.value = next.video_id
      durationSecs.value = next.duration_secs || 200
      positionSecs.value = 0
      isPlaying.value = true
    }
  }

  async function prevTrack() {
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('prev_track')
      applyRustState(state)
    } else if (queue.value.length > 0) {
      queueIndex.value = (queueIndex.value + queue.value.length - 1) % queue.value.length
      const prev = queue.value[queueIndex.value]
      currentTrackTitle.value = prev.title
      currentTrackArtist.value = prev.artist
      currentThumbnailUrl.value = prev.thumbnail_url
      videoId.value = prev.video_id
      durationSecs.value = prev.duration_secs || 200
      positionSecs.value = 0
      isPlaying.value = true
    }
  }

  async function setVolume(newVolume: number) {
    const clamped = Math.max(0, Math.min(1, newVolume))
    volume.value = clamped
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('set_volume', { volume: clamped })
      applyRustState(state)
    }
  }

  async function seekTo(seconds: number) {
    const clamped = Math.max(0, Math.min(durationSecs.value || 1, seconds))
    positionSecs.value = clamped
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('seek_to', { positionSecs: clamped })
      applyRustState(state)
    }
  }

  async function playYoutubeVideo(urlOrId: string, title?: string, artist?: string) {
    const trimmed = urlOrId.trim()
    if (!trimmed) return
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('play_youtube_video', {
        videoId: trimmed,
        title: title ?? null,
        artist: artist ?? null,
      })
      applyRustState(state)
    } else {
      videoId.value = trimmed
      currentTrackTitle.value = title || `YouTube Stream (${trimmed})`
      currentTrackArtist.value = artist || 'YouTube Music Direct Stream'
      positionSecs.value = 0
      isPlaying.value = true
    }
  }

  function cleanupStore() {
    if (unlistenPlayerFn) {
      unlistenPlayerFn()
      unlistenPlayerFn = null
    }
    if (unlistenQueueFn) {
      unlistenQueueFn()
      unlistenQueueFn = null
    }
    if (unlistenAuthFn) {
      unlistenAuthFn()
      unlistenAuthFn = null
    }
  }

  return {
    theme,
    toggleTheme,
    setTheme,
    isPlaying,
    currentTrackTitle,
    currentTrackArtist,
    currentThumbnailUrl,
    volume,
    queueIndex,
    queueLen,
    videoId,
    positionSecs,
    durationSecs,
    isLoading,
    audioQualityLabel,
    bufferSizeMb,
    repeatMode,
    autoplayRadio,
    lowRamNoImages,
    queue,
    rightPanelOpen,
    rightPanelTab,
    isQueueDrawerOpen,
    isFullscreenPlayer,
    searchFilter,
    lyrics,
    isLoadingLyrics,
    relatedTracks,
    isLoadingRelated,
    errorMessage,
    isTauriConnected,
    lastSyncTimestamp,
    authStatus,
    authError,
    homeSections,
    searchResults,
    likedTracks,
    userPlaylists,
    selectedPlaylistTracks,
    selectedPlaylistTitle,
    isFetchingContent,
    formatTime,
    initStore,
    fetchQueue,
    fetchLyrics,
    refreshRelatedTracks,
    playQueueIndex,
    removeFromQueue,
    clearQueue,
    shuffleQueue,
    cycleRepeatMode,
    toggleAutoplayRadio,
    toggleLowRamImages,
    toggleQueueDrawer,
    openRightPanel,
    setRightPanelTab,
    toggleFullscreenPlayer,
    isTrackLiked,
    toggleLikeTrack,
    addToQueue,
    startRadioFromTrack,
    dismissError,
    openLoginWindow,
    setManualCookie,
    logout,
    loadHomeFeed,
    searchMusic,
    loadLikedMusic,
    loadUserPlaylists,
    loadPlaylistTracks,
    playTrackList,
    togglePlay,
    nextTrack,
    prevTrack,
    setVolume,
    seekTo,
    playYoutubeVideo,
    cleanupStore,
  }
})
