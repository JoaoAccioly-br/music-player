<script setup lang="ts">
import { computed } from 'vue'
import { Disc3, Loader2, Music2, Pause, Play } from 'lucide-vue-next'
import { usePlayerStore, type TrackItem } from '../stores/player'

const props = withDefaults(
  defineProps<{
    track: TrackItem
    mode?: 'card' | 'row'
  }>(),
  {
    mode: 'card',
  }
)

const emit = defineEmits<{
  (e: 'play'): void
}>()

const player = usePlayerStore()

const isCurrentTrack = computed(() => player.videoId === props.track.video_id)

const fallbackGradients = [
  'from-red-700/80 via-rose-900/80 to-zinc-900',
  'from-indigo-700/80 via-purple-900/80 to-zinc-900',
  'from-emerald-700/80 via-teal-900/80 to-zinc-900',
  'from-amber-700/80 via-orange-900/80 to-zinc-900',
]

const gradientClass = computed(() => {
  const code = props.track.video_id.charCodeAt(0) || 0
  return fallbackGradients[code % fallbackGradients.length]
})
</script>

<template>
  <!-- CARD LAYOUT (Used on Home shelves) -->
  <div
    v-if="mode === 'card'"
    @click="emit('play')"
    :class="[
      'group relative rounded-xl p-3.5 border transition-all cursor-pointer flex flex-col justify-between',
      isCurrentTrack
        ? 'bg-zinc-800/90 border-red-500/50 shadow-lg shadow-red-950/20'
        : 'bg-zinc-900/50 border-zinc-800/80 hover:bg-zinc-800/60 hover:border-zinc-700',
    ]"
  >
    <div>
      <div
        :class="[
          'w-full aspect-video rounded-lg mb-3 relative overflow-hidden flex items-center justify-center bg-gradient-to-br',
          gradientClass,
        ]"
      >
        <img
          v-if="track.thumbnail_url && !player.lowRamNoImages"
          :src="track.thumbnail_url"
          :alt="track.title"
          loading="lazy"
          decoding="async"
          class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-300"
        />
        <div v-else class="flex flex-col items-center justify-center text-white/80 gap-1">
          <Disc3
            :class="[
              'w-8 h-8',
              isCurrentTrack && player.isPlaying ? 'animate-spin' : 'group-hover:scale-110 transition-transform',
            ]"
            :style="{ animationDuration: '4s' }"
          />
          <span class="text-[10px] font-medium uppercase tracking-wider text-white/60">
            Modo Ultra-RAM
          </span>
        </div>

        <div
          :class="[
            'absolute bottom-2 right-2 w-9 h-9 rounded-full bg-red-600 text-white flex items-center justify-center shadow-lg transition-all',
            isCurrentTrack
              ? 'opacity-100 translate-y-0'
              : 'opacity-0 translate-y-2 group-hover:opacity-100 group-hover:translate-y-0',
          ]"
        >
          <Loader2 v-if="isCurrentTrack && player.isLoading" class="w-4 h-4 animate-spin" />
          <Pause v-else-if="isCurrentTrack && player.isPlaying" class="w-4 h-4 fill-current" />
          <Play v-else class="w-4 h-4 fill-current ml-0.5" />
        </div>
      </div>

      <h3 class="font-semibold text-sm text-white truncate" :title="track.title">
        {{ track.title }}
      </h3>
      <p class="text-xs text-zinc-400 truncate mt-0.5">{{ track.artist }}</p>
    </div>

    <div
      class="flex items-center justify-between mt-2.5 pt-2 border-t border-zinc-800/80 text-[11px] text-zinc-500"
    >
      <span class="truncate">{{ track.album || track.video_id }}</span>
      <span class="tabular-nums shrink-0 ml-2">{{ track.duration_text }}</span>
    </div>
  </div>

  <!-- ROW LAYOUT (Used in Search, Playlists, and Liked Music) -->
  <div
    v-else
    @click="emit('play')"
    :class="[
      'flex items-center justify-between px-4 py-3 cursor-pointer transition-colors group',
      isCurrentTrack ? 'bg-red-950/30' : 'hover:bg-zinc-800/60',
    ]"
  >
    <div class="flex items-center gap-3.5 min-w-0">
      <div
        :class="[
          'w-11 h-11 rounded-lg overflow-hidden shrink-0 flex items-center justify-center relative bg-gradient-to-br',
          gradientClass,
        ]"
      >
        <img
          v-if="track.thumbnail_url && !player.lowRamNoImages"
          :src="track.thumbnail_url"
          :alt="track.title"
          loading="lazy"
          decoding="async"
          class="w-full h-full object-cover"
        />
        <Music2 v-else class="w-5 h-5 text-white/80" />
      </div>

      <div class="min-w-0">
        <p
          :class="[
            'text-sm font-semibold truncate',
            isCurrentTrack ? 'text-red-400' : 'text-white',
          ]"
        >
          {{ track.title }}
        </p>
        <p class="text-xs text-zinc-400 truncate">
          {{ track.artist }}
          <span v-if="track.album" class="text-zinc-500"> • {{ track.album }}</span>
        </p>
      </div>
    </div>

    <div class="flex items-center gap-4 shrink-0">
      <span class="text-xs text-zinc-400 tabular-nums">{{ track.duration_text }}</span>
      <div
        :class="[
          'w-8 h-8 rounded-full flex items-center justify-center text-white transition-colors',
          isCurrentTrack ? 'bg-red-600' : 'bg-zinc-800 group-hover:bg-red-600',
        ]"
      >
        <Loader2 v-if="isCurrentTrack && player.isLoading" class="w-3.5 h-3.5 animate-spin" />
        <Pause v-else-if="isCurrentTrack && player.isPlaying" class="w-3.5 h-3.5 fill-current" />
        <Play v-else class="w-3.5 h-3.5 fill-current ml-0.5" />
      </div>
    </div>
  </div>
</template>
