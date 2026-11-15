<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  AlertCircle,
  ChevronUp,
  Heart,
  ListMusic,
  Loader2,
  MessageSquareQuote,
  Pause,
  Play,
  Radio,
  Repeat,
  Repeat1,
  Shuffle,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
  X,
} from 'lucide-vue-next'
import { formatTime, getPastelGradient, usePlayerStore } from '../stores/player'

const props = defineProps<{
  lyricsOpen?: boolean
}>()

const emit = defineEmits<{
  (e: 'open-lyrics-view'): void
}>()

const player = usePlayerStore()
const previousVolume = ref(0.8)
const isSeeking = ref(false)
const seekPreviewSecs = ref(0)

const currentGradient = computed(() =>
  getPastelGradient((player.videoId || '') + (player.currentTrackTitle || ''))
)

const isCurrentLiked = computed(() => player.isTrackLiked(player.videoId))

const displayedPositionSecs = computed(() =>
  isSeeking.value ? seekPreviewSecs.value : player.positionSecs
)

const progressPercent = computed(() => {
  const dur = player.durationSecs || 1
  return Math.min(100, Math.max(0, (displayedPositionSecs.value / dur) * 100))
})

const repeatLabel = computed(() => {
  if (player.repeatMode === 'one') return 'Repetir 1 Música'
  if (player.repeatMode === 'all') return 'Repetir Fila Inteira'
  return 'Repetição Desativada'
})

let volumeDebounceTimer: ReturnType<typeof setTimeout> | null = null
function handleVolumeInput(event: Event) {
  const target = event.target as HTMLInputElement
  const val = parseFloat(target.value)
  player.volume = val
  if (volumeDebounceTimer) clearTimeout(volumeDebounceTimer)
  volumeDebounceTimer = setTimeout(() => {
    player.setVolume(val)
  }, 40)
}

function handleSeekStart() {
  isSeeking.value = true
  seekPreviewSecs.value = player.positionSecs
}

function handleSeekInput(event: Event) {
  const target = event.target as HTMLInputElement
  isSeeking.value = true
  seekPreviewSecs.value = parseFloat(target.value)
}

function handleSeekCommit(event: Event) {
  const target = event.target as HTMLInputElement
  const val = parseFloat(target.value)
  isSeeking.value = false
  player.seekTo(val)
}

function toggleMute() {
  if (player.volume > 0) {
    previousVolume.value = player.volume
    player.setVolume(0)
  } else {
    player.setVolume(previousVolume.value || 0.8)
  }
}
</script>

<template>
  <div
    v-if="!player.isFullscreenPlayer"
    class="fixed bottom-3.5 left-1/2 -translate-x-1/2 w-[calc(100%-1.5rem)] sm:w-[calc(100%-3rem)] max-w-4xl z-30 pointer-events-none flex flex-col items-center gap-2"
  >
    <!-- Discreet Error Toast when player.errorMessage exists -->
    <div
      v-if="player.errorMessage"
      class="pointer-events-auto flex items-center justify-between gap-3 px-4 py-2 rounded-2xl bg-rose-950/95 border border-rose-500/40 text-rose-100 text-xs shadow-xl backdrop-blur-md max-w-xl w-full"
    >
      <div class="flex items-center gap-2 min-w-0">
        <AlertCircle class="w-4 h-4 text-rose-400 shrink-0" />
        <span class="truncate">{{ player.errorMessage }}</span>
      </div>
      <button
        type="button"
        @click="player.dismissError()"
        class="p-1 rounded-full hover:bg-white/10 text-rose-200 transition-colors shrink-0"
      >
        <X class="w-3.5 h-3.5" />
      </button>
    </div>

    <!-- Floating Dark Pill Player ("Player do C") -->
    <footer
      class="pointer-events-auto relative w-full rounded-[28px] bg-[#231E36]/95 dark:bg-[#1D182E]/95 backdrop-blur-xl border border-white/10 shadow-[0_14px_40px_rgba(18,16,27,0.45)] px-4 sm:px-5 pt-2.5 pb-3.5 text-white overflow-hidden"
    >
      <div class="flex items-center justify-between gap-3">
        <!-- Left: Cover + Track Info + Time -->
        <div
          @click="player.toggleFullscreenPlayer()"
          class="flex items-center gap-3 w-1/3 min-w-0 cursor-pointer group"
          title="Clique para abrir Tocando agora (tela cheia)"
        >
          <div
            :class="[
              'w-11 h-11 rounded-2xl bg-gradient-to-br flex items-center justify-center shrink-0 shadow-md overflow-hidden relative transition-transform group-hover:scale-105',
              currentGradient,
            ]"
          >
            <img
              v-if="player.currentThumbnailUrl && !player.lowRamNoImages"
              :src="player.currentThumbnailUrl"
              :alt="player.currentTrackTitle"
              loading="lazy"
              decoding="async"
              class="w-full h-full object-cover"
            />
            <div
              v-if="player.isLoading"
              class="absolute inset-0 bg-black/55 flex items-center justify-center"
            >
              <Loader2 class="w-4 h-4 text-white animate-spin" />
            </div>
          </div>

          <div class="min-w-0">
            <p class="text-xs sm:text-sm font-bold text-white truncate group-hover:text-[#D9C2FF] transition-colors">
              {{ player.currentTrackTitle }}
            </p>
            <p class="text-[11px] text-[#A89FC4] truncate tabular-nums">
              {{ player.currentTrackArtist }} • {{ formatTime(displayedPositionSecs) }} /
              {{ formatTime(player.durationSecs) }}
            </p>
          </div>
        </div>

        <!-- Center: Playback Controls -->
        <div class="flex items-center justify-center gap-3 sm:gap-4 shrink-0">
          <!-- Shuffle -->
          <button
            type="button"
            @click="player.shuffleQueue()"
            title="Embaralhar fila"
            class="p-1.5 text-[#A89FC4] hover:text-white transition-colors rounded-full hover:bg-white/10"
          >
            <Shuffle class="w-3.5 h-3.5" />
          </button>

          <!-- Prev -->
          <button
            type="button"
            @click="player.prevTrack()"
            title="Música anterior"
            class="p-1.5 text-[#E2DCF5] hover:text-white transition-colors rounded-full hover:bg-white/10"
          >
            <SkipBack class="w-4 h-4 fill-current" />
          </button>

          <!-- Play / Pause Circular Lavender Highlight -->
          <button
            type="button"
            @click="player.togglePlay()"
            :title="player.isPlaying ? 'Pausar' : 'Reproduzir'"
            class="w-10 h-10 rounded-full bg-[#B8A2FF] hover:bg-[#CBB9FF] active:scale-95 text-[#1A162B] flex items-center justify-center transition-all shadow-md hover:scale-105"
          >
            <Loader2 v-if="player.isLoading" class="w-4 h-4 animate-spin text-[#1A162B]" />
            <Pause v-else-if="player.isPlaying" class="w-4 h-4 fill-current" />
            <Play v-else class="w-4 h-4 fill-current ml-0.5" />
          </button>

          <!-- Next -->
          <button
            type="button"
            @click="player.nextTrack()"
            title="Próxima música"
            class="p-1.5 text-[#E2DCF5] hover:text-white transition-colors rounded-full hover:bg-white/10"
          >
            <SkipForward class="w-4 h-4 fill-current" />
          </button>

          <!-- Repeat -->
          <button
            type="button"
            @click="player.cycleRepeatMode()"
            :title="repeatLabel"
            :class="[
              'p-1.5 transition-colors rounded-full',
              player.repeatMode !== 'off'
                ? 'text-[#C4B5FD] bg-[#8B6EFF]/25'
                : 'text-[#A89FC4] hover:text-white hover:bg-white/10',
            ]"
          >
            <Repeat1 v-if="player.repeatMode === 'one'" class="w-3.5 h-3.5" />
            <Repeat v-else class="w-3.5 h-3.5" />
          </button>
        </div>

        <!-- Right: Like, Lyrics, Radio Badge, Queue, Volume & Expand -->
        <div class="flex items-center justify-end gap-1.5 sm:gap-2 w-1/3">
          <!-- Heart (Like) -->
          <button
            type="button"
            @click="player.toggleLikeTrack()"
            :title="isCurrentLiked ? 'Remover das curtidas' : 'Curtir faixa atual'"
            :class="[
              'p-1.5 rounded-full transition-colors',
              isCurrentLiked
                ? 'text-[#F472B6] hover:text-[#EC4899]'
                : 'text-[#A89FC4] hover:text-white hover:bg-white/10',
            ]"
          >
            <Heart :class="['w-3.5 h-3.5', isCurrentLiked ? 'fill-current' : '']" />
          </button>

          <!-- Lyrics Button -->
          <button
            type="button"
            @click="emit('open-lyrics-view')"
            title="Letra sincronizada"
            :class="[
              'p-1.5 rounded-full transition-colors',
              props.lyricsOpen || (player.rightPanelOpen && player.rightPanelTab === 'lyrics')
                ? 'bg-[#8B6EFF]/30 text-[#DDD6FE]'
                : 'text-[#A89FC4] hover:text-white hover:bg-white/10',
            ]"
          >
            <MessageSquareQuote class="w-3.5 h-3.5" />
          </button>

          <!-- Autoplay Radio Green Badge ((•)) -->
          <button
            type="button"
            @click="player.toggleAutoplayRadio()"
            :title="
              player.autoplayRadio
                ? 'Rádio Autoplay ativa (Clique para desativar)'
                : 'Rádio Autoplay desativada (Clique para ativar)'
            "
            :class="[
              'w-7 h-7 rounded-full flex items-center justify-center border transition-all',
              player.autoplayRadio
                ? 'bg-[#22C55E]/20 border-[#22C55E]/40 text-[#4ADE80]'
                : 'bg-white/5 border-white/10 text-[#A89FC4] hover:text-white',
            ]"
          >
            <Radio class="w-3.5 h-3.5" />
          </button>

          <!-- Queue Drawer Toggle -->
          <button
            type="button"
            @click="player.openRightPanel('queue')"
            title="Abrir / fechar fila A seguir"
            :class="[
              'p-1.5 rounded-full transition-colors',
              player.rightPanelOpen && player.rightPanelTab === 'queue'
                ? 'bg-[#8B6EFF]/30 text-[#DDD6FE]'
                : 'text-[#A89FC4] hover:text-white hover:bg-white/10',
            ]"
          >
            <ListMusic class="w-3.5 h-3.5" />
          </button>

          <!-- Mute + Compact Volume Slider -->
          <div class="hidden md:flex items-center gap-1.5 ml-1">
            <button
              type="button"
              @click="toggleMute"
              class="text-[#A89FC4] hover:text-white transition-colors"
              :title="player.volume === 0 ? 'Restaurar volume' : 'Mudo'"
            >
              <VolumeX v-if="player.volume === 0" class="w-3.5 h-3.5 text-rose-400" />
              <Volume2 v-else class="w-3.5 h-3.5" />
            </button>
            <input
              type="range"
              min="0"
              max="1"
              step="0.01"
              :value="player.volume"
              @input="handleVolumeInput"
              class="w-14 h-1 bg-white/20 rounded-full appearance-none cursor-pointer accent-[#B8A2FF]"
            />
          </div>

          <!-- Fullscreen Expand Button -->
          <button
            type="button"
            @click="player.toggleFullscreenPlayer()"
            title="Expandir Tocando agora (tela cheia)"
            class="p-1.5 rounded-full text-[#A89FC4] hover:text-white hover:bg-white/10 transition-colors"
          >
            <ChevronUp class="w-4 h-4" />
          </button>
        </div>
      </div>

      <!-- Integrated Bottom Progress Bar inside the Pill -->
      <div class="mt-2 relative w-full h-1 rounded-full bg-white/12 overflow-hidden flex items-center">
        <div
          class="h-full bg-gradient-to-r from-[#8B6EFF] to-[#D9C2FF] rounded-full transition-all duration-150"
          :style="{ width: `${progressPercent}%` }"
        ></div>
        <input
          type="range"
          :min="0"
          :max="player.durationSecs || 1"
          step="0.5"
          :value="displayedPositionSecs"
          @pointerdown="handleSeekStart"
          @touchstart="handleSeekStart"
          @input="handleSeekInput"
          @change="handleSeekCommit"
          aria-label="Progresso da música"
          class="absolute inset-0 w-full h-full opacity-0 cursor-pointer"
        />
      </div>
    </footer>
  </div>
</template>
