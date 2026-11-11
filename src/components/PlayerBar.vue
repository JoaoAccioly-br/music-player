<script setup lang="ts">
import { computed, ref } from 'vue'
import {
  Disc3,
  ListMusic,
  Loader2,
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
} from 'lucide-vue-next'
import { DEMO_TRACKS, formatTime, usePlayerStore } from '../stores/player'

const player = usePlayerStore()
const previousVolume = ref(0.8)

const currentDemoTrack = computed(
  () => DEMO_TRACKS[player.queueIndex % DEMO_TRACKS.length] ?? DEMO_TRACKS[0]
)

const volumePercent = computed(() => Math.round(player.volume * 100))

const repeatLabel = computed(() => {
  if (player.repeatMode === 'one') return 'Repetir 1 Música (Loop Track)'
  if (player.repeatMode === 'all') return 'Repetir Fila Inteira (Loop Queue)'
  return 'Repetição Desativada'
})

function handleVolumeInput(event: Event) {
  const target = event.target as HTMLInputElement
  player.setVolume(parseFloat(target.value))
}

function handleSeekInput(event: Event) {
  const target = event.target as HTMLInputElement
  player.seekTo(parseFloat(target.value))
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
  <footer
    class="h-24 px-6 bg-[#121212] border-t border-zinc-800 flex items-center justify-between shrink-0 z-20"
  >
    <!-- Left: Currently Playing Track Info -->
    <div class="flex items-center gap-3.5 w-1/3 min-w-0">
      <div
        :class="[
          'w-12 h-12 rounded-lg bg-gradient-to-br flex items-center justify-center shrink-0 shadow overflow-hidden relative',
          currentDemoTrack.coverGradient,
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
        <Disc3
          v-else
          :class="[
            'w-6 h-6 text-white/85',
            player.isPlaying ? 'animate-spin' : '',
          ]"
          :style="{ animationDuration: '4s' }"
        />
        <div
          v-if="player.isLoading"
          class="absolute inset-0 bg-black/60 flex items-center justify-center"
        >
          <Loader2 class="w-5 h-5 text-white animate-spin" />
        </div>
      </div>

      <div class="min-w-0">
        <p class="text-sm font-semibold text-white truncate">{{ player.currentTrackTitle }}</p>
        <p class="text-xs text-zinc-400 truncate">{{ player.currentTrackArtist }}</p>
        <p class="text-[10px] text-zinc-500 truncate mt-0.5">{{ player.audioQualityLabel }}</p>
      </div>
    </div>

    <!-- Center: Shuffle, Prev, Play/Pause, Next, Repeat + Interactive Seek Progress Bar -->
    <div class="flex flex-col items-center gap-2 w-1/3 max-w-md">
      <div class="flex items-center gap-4">
        <!-- Shuffle Queue Button -->
        <button
          @click="player.shuffleQueue()"
          title="Embaralhar Fila em Rust (Mantém a música atual tocando no índice 0)"
          class="p-2 text-zinc-400 hover:text-white transition-colors rounded-full hover:bg-zinc-800"
        >
          <Shuffle class="w-4 h-4" />
        </button>

        <!-- Prev Track -->
        <button
          @click="player.prevTrack()"
          title="Música Anterior"
          class="p-2 text-zinc-300 hover:text-white transition-colors rounded-full hover:bg-zinc-800"
        >
          <SkipBack class="w-5 h-5 fill-current" />
        </button>

        <!-- Play / Pause -->
        <button
          @click="player.togglePlay()"
          :title="player.isPlaying ? 'Pausar' : 'Reproduzir'"
          class="w-10 h-10 rounded-full bg-white text-black hover:scale-105 active:scale-95 flex items-center justify-center transition-transform shadow-md"
        >
          <Loader2 v-if="player.isLoading" class="w-5 h-5 animate-spin text-red-600" />
          <Pause v-else-if="player.isPlaying" class="w-5 h-5 fill-current" />
          <Play v-else class="w-5 h-5 fill-current ml-0.5" />
        </button>

        <!-- Next Track -->
        <button
          @click="player.nextTrack()"
          title="Próxima Música"
          class="p-2 text-zinc-300 hover:text-white transition-colors rounded-full hover:bg-zinc-800"
        >
          <SkipForward class="w-5 h-5 fill-current" />
        </button>

        <!-- Repeat Mode Button (Off -> All -> One) -->
        <button
          @click="player.cycleRepeatMode()"
          :title="repeatLabel"
          :class="[
            'p-2 transition-colors rounded-full relative',
            player.repeatMode !== 'off'
              ? 'text-red-400 bg-red-950/40 hover:bg-red-950/60'
              : 'text-zinc-400 hover:text-white hover:bg-zinc-800',
          ]"
        >
          <Repeat1 v-if="player.repeatMode === 'one'" class="w-4 h-4" />
          <Repeat v-else class="w-4 h-4" />
        </button>
      </div>

      <!-- Seek Slider and Time Counters -->
      <div class="w-full flex items-center gap-2.5">
        <span class="text-[11px] text-zinc-400 tabular-nums w-9 text-right">
          {{ formatTime(player.positionSecs) }}
        </span>
        <input
          type="range"
          :min="0"
          :max="player.durationSecs || 1"
          step="0.5"
          :value="player.positionSecs"
          @input="handleSeekInput"
          class="flex-1 h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-red-600"
        />
        <span class="text-[11px] text-zinc-400 tabular-nums w-9">
          {{ formatTime(player.durationSecs) }}
        </span>
      </div>
    </div>

    <!-- Right: Autoplay Radio Badge, Queue Drawer Toggle & Volume Slider -->
    <div class="flex items-center justify-end gap-2.5 w-1/3">
      <!-- Autoplay Radio Indicator Button -->
      <button
        @click="player.toggleAutoplayRadio()"
        :title="
          player.autoplayRadio
            ? 'Rádio Infinita Autoplay Ativa (/youtubei/v1/next)'
            : 'Clique para ativar a Rádio Infinita Autoplay'
        "
        :class="[
          'hidden lg:flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg border text-xs font-medium transition-colors',
          player.autoplayRadio
            ? 'bg-emerald-950/50 border-emerald-500/40 text-emerald-300'
            : 'bg-zinc-900 border-zinc-800 text-zinc-500 hover:text-zinc-300',
        ]"
      >
        <Radio class="w-3.5 h-3.5" />
        <span>Rádio</span>
      </button>

      <!-- Queue Drawer Toggle Button -->
      <button
        @click="player.toggleQueueDrawer()"
        title="Abrir / Fechar Gaveta da Fila de Reprodução (Up Next)"
        :class="[
          'flex items-center gap-1.5 px-3 py-1.5 rounded-lg border text-xs font-medium transition-colors',
          player.isQueueDrawerOpen
            ? 'bg-red-600 border-red-500 text-white'
            : 'bg-zinc-900 hover:bg-zinc-800 border-zinc-700/80 text-zinc-200',
        ]"
      >
        <ListMusic class="w-3.5 h-3.5" />
        <span class="hidden sm:inline">Fila</span>
        <span class="px-1.5 py-0.2 rounded bg-black/30 text-[10px] tabular-nums">
          {{ player.queueIndex + 1 }}/{{ player.queue.length }}
        </span>
      </button>

      <!-- Mute + Volume Slider -->
      <button
        @click="toggleMute"
        class="p-1.5 text-zinc-400 hover:text-white transition-colors"
        :title="player.volume === 0 ? 'Restaurar Volume' : 'Mudo'"
      >
        <VolumeX v-if="player.volume === 0" class="w-4 h-4 text-red-400" />
        <Volume2 v-else class="w-4 h-4" />
      </button>

      <input
        type="range"
        min="0"
        max="1"
        step="0.01"
        :value="player.volume"
        @input="handleVolumeInput"
        class="w-24 h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-red-600"
      />

      <span class="text-xs text-zinc-400 w-8 text-right tabular-nums">
        {{ volumePercent }}%
      </span>
    </div>
  </footer>
</template>
