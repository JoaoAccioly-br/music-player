import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'

export type RepeatMode = 'off' | 'all' | 'one'

export interface DemoTrack {
  index: number
  videoId: string
  title: string
  artist: string
  album: string
  duration: string
  coverGradient: string
}

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

export const DEMO_TRACKS: DemoTrack[] = [
  {
    index: 0,
    videoId: 'jfKfPfyJRdk',
    title: 'Cyberpunk Synthwave (Low-RAM Mix)',
    artist: 'Rust Audio Engine',
    album: 'Zero-WebView Sessions Vol. 1',
    duration: '1:00',
    coverGradient: 'from-red-600 via-rose-700 to-zinc-900',
  },
  {
    index: 1,
    videoId: '4xDzrJKXOOY',
    title: 'Night City Overdrive',
    artist: 'Tauri Core Collective',
    album: 'Native Threads EP',
    duration: '1:00',
    coverGradient: 'from-purple-600 via-indigo-800 to-zinc-900',
  },
  {
    index: 2,
    videoId: '5qap5aO4i9A',
    title: 'Memory Safe Beats (15MB RAM)',
    artist: 'Borrow Checker',
    album: 'Fearless Concurrency',
    duration: '1:00',
    coverGradient: 'from-emerald-600 via-teal-800 to-zinc-900',
  },
  {
    index: 3,
    videoId: 'DWcJFNfaw9c',
    title: 'Boss Fight Phonk',
    artist: 'High FPS Gaming',
    album: 'No Chromium Overhead',
    duration: '1:00',
    coverGradient: 'from-amber-600 via-orange-800 to-zinc-900',
  },
]

export function formatTime(seconds: number): string {
  if (!Number.isFinite(seconds) || seconds < 0) return '0:00'
  const totalSecs = Math.floor(seconds)
  const mins = Math.floor(totalSecs / 60)
  const secs = totalSecs % 60
  return `${mins}:${secs.toString().padStart(2, '0')}`
}

export const usePlayerStore = defineStore('player', () => {
  const isPlaying = ref(false)
  const currentTrackTitle = ref(DEMO_TRACKS[0].title)
  const currentTrackArtist = ref(DEMO_TRACKS[0].artist)
  const currentThumbnailUrl = ref(`https://i.ytimg.com/vi/${DEMO_TRACKS[0].videoId}/hqdefault.jpg`)
  const volume = ref(0.8)
  const queueIndex = ref(0)
  const queueLen = ref(DEMO_TRACKS.length)
  const videoId = ref(DEMO_TRACKS[0].videoId)
  const positionSecs = ref(0)
  const durationSecs = ref(60)
  const isLoading = ref(false)
  const audioQualityLabel = ref('Pronto (Clique em Play para carregar na RAM)')
  const bufferSizeMb = ref(0)
  const repeatMode = ref<RepeatMode>('off')
  const autoplayRadio = ref(true)
  const lowRamNoImages = ref(false)
  const errorMessage = ref<string | null>(null)
  const isTauriConnected = ref(false)
  const lastSyncTimestamp = ref<string>('')

  // Phase 4 & 5: Queue Drawer State
  const queue = ref<TrackItem[]>(
    DEMO_TRACKS.map((d) => ({
      video_id: d.videoId,
      title: d.title,
      artist: d.artist,
      album: d.album,
      duration_text: d.duration,
      duration_secs: 60,
      thumbnail_url: `https://i.ytimg.com/vi/${d.videoId}/hqdefault.jpg`,
    }))
  )
  const isQueueDrawerOpen = ref(false)

  // Phase 3: Auth & Innertube State
  const authStatus = ref<AuthStatus>({
    is_authenticated: false,
    sapisid_preview: '',
    account_hint: 'Não conectado',
    auth_method: '',
    has_sapisid: false,
    updated_at: 0,
  })
  const authError = ref<string | null>(null)
  const homeSections = ref<HomeSection[]>([])
  const searchResults = ref<TrackItem[]>([])
  const likedTracks = ref<TrackItem[]>([])
  const userPlaylists = ref<PlaylistItem[]>([])
  const selectedPlaylistTracks = ref<TrackItem[]>([])
  const selectedPlaylistTitle = ref<string>('')
  const isFetchingContent = ref(false)

  let unlistenPlayerFn: UnlistenFn | null = null
  let unlistenAuthFn: UnlistenFn | null = null
  let unlistenQueueFn: UnlistenFn | null = null

  function applyRustState(state: RustPlayerState) {
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
  }

  /**
   * Rehydrates the Vue UI state directly from the persistent Rust Mutex<PlayerState>
   * and subscribes to real-time changes triggered by either the UI, the audio thread, or the System Tray.
   */
  async function initStore() {
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
    } catch {
      // Fallback for running in browser preview outside Tauri runtime
      isTauriConnected.value = false
      lastSyncTimestamp.value = new Date().toLocaleTimeString()
    }
  }

  async function fetchQueue() {
    if (!isTauriConnected.value) return
    try {
      const items = await invoke<TrackItem[]>('get_queue')
      queue.value = items
      queueLen.value = items.length
    } catch (err) {
      errorMessage.value = String(err)
    }
  }

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
      durationSecs.value = chosen.duration_secs
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
    isQueueDrawerOpen.value = !isQueueDrawerOpen.value
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
      homeSections.value = await invoke<HomeSection[]>('get_home_feed')
    } catch (err) {
      errorMessage.value = String(err)
    } finally {
      isFetchingContent.value = false
    }
  }

  async function searchMusic(query: string) {
    const trimmed = query.trim()
    if (!trimmed || !isTauriConnected.value) return
    isFetchingContent.value = true
    try {
      searchResults.value = await invoke<TrackItem[]>('search_youtube_music', { query: trimmed })
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
      likedTracks.value = await invoke<TrackItem[]>('get_liked_music')
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
      userPlaylists.value = await invoke<PlaylistItem[]>('get_user_playlists')
    } catch (err) {
      errorMessage.value = String(err)
    } finally {
      isFetchingContent.value = false
    }
  }

  async function loadPlaylistTracks(playlist: PlaylistItem) {
    selectedPlaylistTitle.value = playlist.title
    if (!isTauriConnected.value) return
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
      durationSecs.value = chosen.duration_secs
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
    } else {
      queueIndex.value = (queueIndex.value + 1) % DEMO_TRACKS.length
      currentTrackTitle.value = DEMO_TRACKS[queueIndex.value].title
      currentTrackArtist.value = DEMO_TRACKS[queueIndex.value].artist
      videoId.value = DEMO_TRACKS[queueIndex.value].videoId
      positionSecs.value = 0
      isPlaying.value = true
    }
  }

  async function prevTrack() {
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('prev_track')
      applyRustState(state)
    } else {
      queueIndex.value = (queueIndex.value + DEMO_TRACKS.length - 1) % DEMO_TRACKS.length
      currentTrackTitle.value = DEMO_TRACKS[queueIndex.value].title
      currentTrackArtist.value = DEMO_TRACKS[queueIndex.value].artist
      videoId.value = DEMO_TRACKS[queueIndex.value].videoId
      positionSecs.value = 0
      isPlaying.value = true
    }
  }

  async function selectDemoTrack(index: number) {
    if (isTauriConnected.value) {
      const state = await invoke<RustPlayerState>('select_demo_track', { index })
      applyRustState(state)
    } else {
      queueIndex.value = index % DEMO_TRACKS.length
      currentTrackTitle.value = DEMO_TRACKS[queueIndex.value].title
      currentTrackArtist.value = DEMO_TRACKS[queueIndex.value].artist
      videoId.value = DEMO_TRACKS[queueIndex.value].videoId
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
    isQueueDrawerOpen,
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
    playQueueIndex,
    removeFromQueue,
    clearQueue,
    shuffleQueue,
    cycleRepeatMode,
    toggleAutoplayRadio,
    toggleLowRamImages,
    toggleQueueDrawer,
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
    selectDemoTrack,
    setVolume,
    seekTo,
    playYoutubeVideo,
    cleanupStore,
  }
})
