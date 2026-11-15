<script setup lang="ts">
import { computed, nextTick, watch } from 'vue'
import {
  Loader2,
  Maximize2,
  Plus,
  Radio,
  Repeat,
  Repeat1,
  Shuffle,
  Trash2,
  X,
} from 'lucide-vue-next'
import {
  computeActiveLyricIndex,
  formatTime,
  getLyricLineSeekSecs,
  getPastelGradient,
  usePlayerStore,
  type RightPanelTab,
} from '../stores/player'

const props = withDefaults(
  defineProps<{
    embedded?: boolean
  }>(),
  {
    embedded: false,
  }
)

const player = usePlayerStore()

const tabs: Array<{ id: RightPanelTab; label: string }> = [
  { id: 'queue', label: 'A seguir' },
  { id: 'lyrics', label: 'Letra' },
  { id: 'related', label: 'Relacionados' },
]

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
  const isVisible = (props.embedded || player.rightPanelOpen) && player.rightPanelTab === 'lyrics'
  if (idx < 0 || !isVisible) return
  await nextTick()
  lyricLineEls.get(idx)?.scrollIntoView({ behavior: 'smooth', block: 'center' })
}

watch(activeLyricIndex, (idx) => {
  scrollActiveLyricIntoView(idx)
})

watch(
  () => [player.rightPanelTab, player.rightPanelOpen, props.embedded] as const,
  ([tab, open, embedded]) => {
    if ((embedded || open) && tab === 'lyrics' && activeLyricIndex.value >= 0) {
      scrollActiveLyricIntoView(activeLyricIndex.value)
    }
  }
)

function seekToLyricLine(lineIdx: number) {
  const timed = player.lyrics?.timedLines ?? []
  if (!timed.length) return
  const targetSec = getLyricLineSeekSecs(player.lyrics, lineIdx, player.durationSecs)
  player.seekTo(targetSec)
}
</script>

<template>
  <aside
    v-if="embedded || player.rightPanelOpen"
    :class="[
      'flex flex-col overflow-hidden transition-colors duration-200',
      embedded
        ? 'w-full h-full rounded-3xl bg-white/90 dark:bg-[#1E1A2D]/95 border border-[rgba(120,100,180,0.14)] dark:border-white/[0.08] shadow-xl'
        : 'w-80 sm:w-96 shrink-0 h-full border-l border-[rgba(120,100,180,0.12)] dark:border-white/[0.07] bg-[#FAF9FC]/95 dark:bg-[#161321]/95 backdrop-blur-xl z-20',
    ]"
  >
    <!-- Top 3-Tab Pill Switcher: A seguir | Letra | Relacionados -->
    <div class="p-4 pb-2 flex items-center justify-between gap-2">
      <div
        class="flex-1 grid grid-cols-3 p-1 rounded-full bg-[#EDE9F5] dark:bg-[#12101B] border border-[rgba(120,100,180,0.1)] dark:border-white/[0.06]"
      >
        <button
          v-for="t in tabs"
          :key="t.id"
          type="button"
          @click="player.setRightPanelTab(t.id)"
          :class="[
            'py-1.5 px-2 rounded-full text-xs font-semibold transition-all text-center',
            player.rightPanelTab === t.id
              ? 'bg-white dark:bg-[#2B2540] text-[#1A162B] dark:text-[#F4F2FA] shadow-sm'
              : 'text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white',
          ]"
        >
          {{ t.label }}
        </button>
      </div>

      <button
        v-if="!embedded"
        type="button"
        @click="player.rightPanelOpen = false"
        title="Fechar painel"
        class="w-7 h-7 rounded-full flex items-center justify-center text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white hover:bg-black/5 dark:hover:bg-white/10 transition-colors shrink-0"
      >
        <X class="w-4 h-4" />
      </button>
    </div>

    <!-- ==================== TAB 1: A SEGUIR (QUEUE) ==================== -->
    <template v-if="player.rightPanelTab === 'queue'">
      <!-- Queue Subheader -->
      <div class="px-4 py-2 flex items-center justify-between text-xs">
        <span class="text-[#6E6885] dark:text-[#9B94B2] font-medium">
          {{ player.queue.length }} faixas •
          <button
            type="button"
            @click="player.toggleAutoplayRadio()"
            :class="[
              'hover:underline font-medium',
              player.autoplayRadio
                ? 'text-[#16A34A] dark:text-[#4ADE80]'
                : 'text-[#6E6885] dark:text-[#9B94B2]',
            ]"
          >
            Rádio {{ player.autoplayRadio ? 'ligada' : 'desligada' }}
          </button>
        </span>

        <div class="flex items-center gap-1">
          <button
            type="button"
            @click="player.shuffleQueue()"
            title="Embaralhar fila"
            class="p-1.5 rounded-lg bg-[#EDE9F5]/80 dark:bg-[#252038] text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white transition-colors"
          >
            <Shuffle class="w-3.5 h-3.5" />
          </button>

          <button
            type="button"
            @click="player.cycleRepeatMode()"
            :title="
              player.repeatMode === 'one'
                ? 'Repetir 1 faixa'
                : player.repeatMode === 'all'
                  ? 'Repetir fila'
                  : 'Repetição desativada'
            "
            :class="[
              'p-1.5 rounded-lg transition-colors',
              player.repeatMode !== 'off'
                ? 'bg-[#7C5CFC]/20 text-[#7C5CFC] dark:text-[#B497FF]'
                : 'bg-[#EDE9F5]/80 dark:bg-[#252038] text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white',
            ]"
          >
            <Repeat1 v-if="player.repeatMode === 'one'" class="w-3.5 h-3.5" />
            <Repeat v-else class="w-3.5 h-3.5" />
          </button>

          <button
            v-if="player.queue.length > 1"
            type="button"
            @click="player.clearQueue()"
            title="Limpar próximas faixas"
            class="p-1.5 rounded-lg bg-[#EDE9F5]/80 dark:bg-[#252038] text-[#6E6885] dark:text-[#9B94B2] hover:text-rose-500 transition-colors"
          >
            <Trash2 class="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      <!-- Queue Tracks List -->
      <div class="flex-1 overflow-y-auto px-3 py-1 space-y-1">
        <div
          v-for="(item, idx) in player.queue"
          :key="item.video_id + '-' + idx"
          @click="player.playQueueIndex(idx)"
          :class="[
            'group flex items-center justify-between gap-2.5 p-2 rounded-2xl cursor-pointer transition-all',
            idx === player.queueIndex
              ? 'bg-[#EBE4FF] dark:bg-[#2B224B] border border-[#7C5CFC]/25 dark:border-[#8B6EFF]/35 shadow-sm'
              : 'hover:bg-white/70 dark:hover:bg-[#1E1A2D]/70 border border-transparent',
          ]"
        >
          <div class="flex items-center gap-2.5 min-w-0">
            <!-- Pastel Cover / Thumbnail -->
            <div
              :class="[
                'w-9 h-9 rounded-xl overflow-hidden shrink-0 flex items-center justify-center bg-gradient-to-br shadow-sm',
                getPastelGradient(item.video_id + item.title),
              ]"
            >
              <img
                v-if="item.thumbnail_url && !player.lowRamNoImages"
                :src="item.thumbnail_url"
                :alt="item.title"
                loading="lazy"
                decoding="async"
                class="w-full h-full object-cover"
              />
            </div>

            <div class="min-w-0">
              <p
                :class="[
                  'text-xs font-semibold truncate',
                  idx === player.queueIndex
                    ? 'text-[#6D4AFF] dark:text-[#C4B5FD]'
                    : 'text-[#1A162B] dark:text-[#F4F2FA]',
                ]"
              >
                {{ item.title }}
              </p>
              <p class="text-[11px] text-[#6E6885] dark:text-[#9B94B2] truncate">
                {{ item.artist }}
              </p>
            </div>
          </div>

          <div class="flex items-center gap-1.5 shrink-0">
            <span class="text-[11px] text-[#6E6885] dark:text-[#9B94B2] tabular-nums">
              {{ item.duration_text || '3:30' }}
            </span>
            <button
              v-if="player.queue.length > 1"
              type="button"
              @click.stop="player.removeFromQueue(idx)"
              title="Remover da fila"
              class="p-1 rounded-full text-[#6E6885] hover:text-rose-500 opacity-0 group-hover:opacity-100 transition-opacity"
            >
              <X class="w-3 h-3" />
            </button>
          </div>
        </div>
      </div>

      <!-- Radio Autoplay Footer Notice (matches green box in mockup) -->
      <div class="p-3.5 pt-2">
        <div
          class="p-3 rounded-2xl bg-[#22C55E]/10 dark:bg-[#22C55E]/12 border border-[#22C55E]/25 text-[#15803D] dark:text-[#86EFAC] text-[11px] leading-relaxed"
        >
          Quando a fila acabar, a Rádio continua com músicas parecidas.
        </div>
      </div>
    </template>

    <!-- ==================== TAB 2: LETRA (LYRICS) ==================== -->
    <template v-else-if="player.rightPanelTab === 'lyrics'">
      <div class="px-4 py-2 flex items-center justify-between gap-2 text-xs">
        <span
          :class="[
            'inline-flex items-center px-2 py-0.5 rounded-full text-[11px] font-medium truncate',
            player.lyrics?.isSynced
              ? 'bg-[#22C55E]/15 text-[#15803D] dark:text-[#4ADE80]'
              : 'text-[#6E6885] dark:text-[#9B94B2]',
          ]"
        >
          {{ player.lyrics?.sourceLabel || 'Buscando letra...' }}
        </span>
        <button
          v-if="!embedded"
          type="button"
          @click="player.toggleFullscreenPlayer('lyrics')"
          class="text-[#7C5CFC] dark:text-[#B497FF] hover:underline font-medium flex items-center gap-1 shrink-0"
        >
          <Maximize2 class="w-3 h-3" />
          <span>Expandir</span>
        </button>
      </div>

      <div
        v-if="player.isLoadingLyrics"
        class="flex-1 flex flex-col items-center justify-center gap-2 text-[#6E6885] dark:text-[#9B94B2] p-6"
      >
        <Loader2 class="w-5 h-5 animate-spin text-[#7C5CFC]" />
        <span class="text-xs">Buscando letra de {{ player.currentTrackTitle }}...</span>
      </div>

      <div
        v-else-if="!(player.lyrics?.timedLines?.length)"
        class="flex-1 flex items-center justify-center text-center text-xs text-[#6E6885] dark:text-[#9B94B2] p-6"
      >
        {{ player.lyrics?.sourceLabel || 'Letra indisponível para esta faixa.' }}
      </div>

      <div
        v-else
        class="flex-1 overflow-y-auto px-5 py-4 space-y-3.5"
      >
        <p
          v-for="(line, idx) in player.lyrics?.timedLines || []"
          :key="idx"
          :ref="(el) => setLyricLineRef(el, idx)"
          @click="seekToLyricLine(idx)"
          :class="[
            'group flex items-baseline gap-2 cursor-pointer transition-all duration-300 leading-relaxed',
            idx === activeLyricIndex
              ? 'text-base sm:text-lg font-bold text-[#1A162B] dark:text-white'
              : idx < activeLyricIndex
                ? 'text-xs sm:text-sm font-medium text-[#6E6885]/55 dark:text-[#9B94B2]/45 hover:text-[#1A162B] dark:hover:text-white'
                : 'text-xs sm:text-sm font-medium text-[#6E6885] dark:text-[#9B94B2]/80 hover:text-[#1A162B] dark:hover:text-white',
          ]"
        >
          <span
            v-if="line.timeSecs !== null"
            class="text-[10px] font-normal tabular-nums text-[#6E6885]/60 dark:text-[#9B94B2]/50 opacity-0 group-hover:opacity-100 transition-opacity shrink-0"
          >
            {{ formatTime(line.timeSecs) }}
          </span>
          <span>{{ line.text }}</span>
        </p>
      </div>
    </template>

    <!-- ==================== TAB 3: RELACIONADOS (RELATED) ==================== -->
    <template v-else>
      <div class="px-4 py-2 flex items-center justify-between text-xs">
        <span class="text-[#6E6885] dark:text-[#9B94B2] truncate">
          Similares a {{ player.currentTrackArtist }}
        </span>
        <button
          type="button"
          @click="player.startRadioFromTrack()"
          class="text-[#7C5CFC] dark:text-[#B497FF] hover:underline font-semibold flex items-center gap-1 shrink-0"
        >
          <Radio class="w-3 h-3" />
          <span>Iniciar rádio</span>
        </button>
      </div>

      <div class="flex-1 overflow-y-auto px-3 py-1 space-y-1">
        <div
          v-for="(item, idx) in player.relatedTracks"
          :key="item.video_id + '-rel-' + idx"
          @click="player.playTrackList(player.relatedTracks, idx)"
          class="group flex items-center justify-between gap-2.5 p-2 rounded-2xl hover:bg-white/70 dark:hover:bg-[#1E1A2D]/70 cursor-pointer transition-all"
        >
          <div class="flex items-center gap-2.5 min-w-0">
            <div
              :class="[
                'w-9 h-9 rounded-xl overflow-hidden shrink-0 flex items-center justify-center bg-gradient-to-br shadow-sm',
                getPastelGradient(item.video_id + item.title),
              ]"
            >
              <img
                v-if="item.thumbnail_url && !player.lowRamNoImages"
                :src="item.thumbnail_url"
                :alt="item.title"
                loading="lazy"
                decoding="async"
                class="w-full h-full object-cover"
              />
            </div>

            <div class="min-w-0">
              <p class="text-xs font-semibold text-[#1A162B] dark:text-[#F4F2FA] truncate">
                {{ item.title }}
              </p>
              <p class="text-[11px] text-[#6E6885] dark:text-[#9B94B2] truncate">
                {{ item.artist }}
              </p>
            </div>
          </div>

          <div class="flex items-center gap-1.5 shrink-0">
            <span class="text-[11px] text-[#6E6885] dark:text-[#9B94B2] tabular-nums">
              {{ item.duration_text }}
            </span>
            <button
              type="button"
              @click.stop="player.addToQueue(item)"
              title="Adicionar à fila"
              class="p-1 rounded-full text-[#6E6885] dark:text-[#9B94B2] hover:text-[#7C5CFC] dark:hover:text-[#B497FF] hover:bg-black/5 dark:hover:bg-white/10 transition-colors"
            >
              <Plus class="w-3.5 h-3.5" />
            </button>
          </div>
        </div>
      </div>
    </template>
  </aside>
</template>
