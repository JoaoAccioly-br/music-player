<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Heart, Loader2, MoreHorizontal, Pause, Play } from 'lucide-vue-next'
import { getPastelGradient, usePlayerStore, type TrackItem } from '../stores/player'

const props = withDefaults(
  defineProps<{
    track: TrackItem
    mode?: 'card' | 'row'
    index?: number
    showAlbum?: boolean
  }>(),
  {
    mode: 'card',
    showAlbum: false,
  }
)

const emit = defineEmits<{
  (e: 'play'): void
}>()

const player = usePlayerStore()
const imageFailed = ref(false)

watch(
  () => props.track.thumbnail_url,
  () => {
    imageFailed.value = false
  }
)

const isCurrentTrack = computed(() => player.videoId === props.track.video_id)
const isLiked = computed(() => player.isTrackLiked(props.track.video_id))

const gradientClass = computed(() =>
  getPastelGradient((props.track.video_id || '') + (props.track.title || ''))
)

const showThumbnail = computed(
  () => Boolean(props.track.thumbnail_url) && !player.lowRamNoImages && !imageFailed.value
)
</script>

<template>
  <!-- SQUARE CARD LAYOUT (Home "Ouvir de novo", Explore "Novos lançamentos", etc.) -->
  <div
    v-if="mode === 'card'"
    @click="emit('play')"
    class="group cursor-pointer select-none flex flex-col"
  >
    <div
      :class="[
        'w-full aspect-square rounded-2xl mb-2.5 relative overflow-hidden bg-gradient-to-br transition-all duration-300 shadow-sm group-hover:shadow-md group-hover:-translate-y-0.5 border',
        gradientClass,
        isCurrentTrack
          ? 'ring-2 ring-[#7C5CFC] dark:ring-[#8B6EFF] border-transparent'
          : 'border-[rgba(120,100,180,0.12)] dark:border-white/[0.07]',
      ]"
    >
      <img
        v-if="showThumbnail"
        :src="track.thumbnail_url"
        :alt="track.title"
        loading="lazy"
        decoding="async"
        @error="imageFailed = true"
        class="w-full h-full object-cover group-hover:scale-105 transition-transform duration-500"
      />

      <!-- Subtle bottom-right circular play button matching the mockup -->
      <button
        type="button"
        @click.stop="emit('play')"
        :title="isCurrentTrack && player.isPlaying ? 'Pausar' : `Tocar ${track.title}`"
        :class="[
          'absolute bottom-2.5 right-2.5 w-8 h-8 rounded-full flex items-center justify-center shadow-md transition-all duration-200',
          isCurrentTrack
            ? 'bg-[#7C5CFC] text-white scale-100 opacity-100'
            : 'bg-white/90 dark:bg-[#1E1A2D]/90 text-[#1A162B] dark:text-[#F4F2FA] opacity-90 group-hover:opacity-100 group-hover:scale-110',
        ]"
      >
        <Loader2 v-if="isCurrentTrack && player.isLoading" class="w-3.5 h-3.5 animate-spin" />
        <Pause v-else-if="isCurrentTrack && player.isPlaying" class="w-3.5 h-3.5 fill-current" />
        <Play v-else class="w-3.5 h-3.5 fill-current ml-0.5" />
      </button>
    </div>

    <h3
      :class="[
        'font-semibold text-xs sm:text-[13px] truncate leading-snug',
        isCurrentTrack
          ? 'text-[#7C5CFC] dark:text-[#B497FF]'
          : 'text-[#1A162B] dark:text-[#F4F2FA]',
      ]"
      :title="track.title"
    >
      {{ track.title }}
    </h3>
    <p class="text-[11px] text-[#6E6885] dark:text-[#9B94B2] truncate mt-0.5">
      {{ track.artist }}
    </p>
  </div>

  <!-- ROW LAYOUT (Search Results, Liked Music, Playlists) -->
  <div
    v-else
    @click="emit('play')"
    :class="[
      'group flex items-center justify-between gap-3 px-3.5 py-2.5 rounded-2xl cursor-pointer transition-all',
      isCurrentTrack
        ? 'bg-[#7C5CFC]/10 dark:bg-[#8B6EFF]/15 border border-[#7C5CFC]/25 dark:border-[#8B6EFF]/30'
        : 'hover:bg-white/80 dark:hover:bg-[#1E1A2D]/80 border border-transparent',
    ]"
  >
    <div class="flex items-center gap-3 min-w-0 flex-1">
      <!-- Optional Index Number -->
      <div
        v-if="typeof index === 'number'"
        class="w-5 text-center text-xs font-medium text-[#6E6885] dark:text-[#9B94B2] tabular-nums shrink-0"
      >
        <Loader2
          v-if="isCurrentTrack && player.isLoading"
          class="w-3.5 h-3.5 animate-spin text-[#7C5CFC] dark:text-[#B497FF] mx-auto"
        />
        <Pause
          v-else-if="isCurrentTrack && player.isPlaying"
          class="w-3.5 h-3.5 text-[#7C5CFC] dark:text-[#B497FF] fill-current mx-auto"
        />
        <span v-else>{{ index + 1 }}</span>
      </div>

      <!-- Rounded Thumbnail / Pastel Gradient -->
      <div
        :class="[
          'w-10 h-10 rounded-xl overflow-hidden shrink-0 flex items-center justify-center relative bg-gradient-to-br shadow-sm',
          gradientClass,
        ]"
      >
        <img
          v-if="showThumbnail"
          :src="track.thumbnail_url"
          :alt="track.title"
          loading="lazy"
          decoding="async"
          @error="imageFailed = true"
          class="w-full h-full object-cover"
        />
      </div>

      <!-- Title & Artist -->
      <div class="min-w-0 flex-1">
        <p
          :class="[
            'text-xs sm:text-sm font-semibold truncate',
            isCurrentTrack
              ? 'text-[#7C5CFC] dark:text-[#B497FF]'
              : 'text-[#1A162B] dark:text-[#F4F2FA]',
          ]"
        >
          {{ track.title }}
        </p>
        <p class="text-[11px] text-[#6E6885] dark:text-[#9B94B2] truncate">
          {{ track.artist }}
        </p>
      </div>
    </div>

    <!-- Optional Album Column (for Liked Tracks & Playlist rows) -->
    <div
      v-if="showAlbum"
      class="hidden md:block w-1/4 min-w-0 text-xs text-[#6E6885] dark:text-[#9B94B2] truncate px-2"
    >
      {{ track.album || 'Single' }}
    </div>

    <!-- Right Actions: Heart, Duration, Add to Queue / More -->
    <div class="flex items-center gap-3 shrink-0">
      <button
        type="button"
        @click.stop="player.toggleLikeTrack(track)"
        :title="isLiked ? 'Remover das curtidas' : 'Adicionar às curtidas'"
        :class="[
          'p-1.5 rounded-full transition-colors',
          isLiked
            ? 'text-[#EC4899] hover:text-[#DB2777]'
            : 'text-[#9B94B2] opacity-0 group-hover:opacity-100 hover:text-[#EC4899]',
        ]"
      >
        <Heart :class="['w-3.5 h-3.5', isLiked ? 'fill-current' : '']" />
      </button>

      <span class="text-xs text-[#6E6885] dark:text-[#9B94B2] tabular-nums w-9 text-right">
        {{ track.duration_text || '3:30' }}
      </span>

      <button
        type="button"
        @click.stop="player.addToQueue(track)"
        title="Adicionar à fila A seguir"
        class="p-1.5 rounded-full text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white hover:bg-black/5 dark:hover:bg-white/10 transition-colors"
      >
        <MoreHorizontal class="w-4 h-4" />
      </button>
    </div>
  </div>
</template>
