<script setup lang="ts">
import {
  ListMusic,
  Music2,
  Play,
  Radio,
  Shuffle,
  Trash2,
  Volume2,
  X,
} from 'lucide-vue-next'
import { usePlayerStore } from '../stores/player'

const player = usePlayerStore()
</script>

<template>
  <aside
    v-if="player.isQueueDrawerOpen"
    class="w-80 sm:w-96 bg-[#0a0a0a] border-l border-zinc-800/90 flex flex-col shrink-0 h-full z-20 shadow-2xl"
  >
    <!-- Drawer Header -->
    <div class="p-4 border-b border-zinc-800/80 space-y-3">
      <div class="flex items-center justify-between">
        <div class="flex items-center gap-2">
          <ListMusic class="w-4 h-4 text-red-500" />
          <h3 class="text-sm font-bold text-white">Fila de Reprodução (Up Next)</h3>
          <span
            class="px-2 py-0.5 rounded-full bg-zinc-800 text-[11px] text-zinc-300 font-medium tabular-nums"
          >
            {{ player.queue.length }} faixas
          </span>
        </div>
        <button
          @click="player.toggleQueueDrawer()"
          class="p-1.5 rounded-lg text-zinc-400 hover:text-white hover:bg-zinc-800 transition-colors"
          title="Fechar Fila"
        >
          <X class="w-4 h-4" />
        </button>
      </div>

      <!-- Queue Action Bar: Shuffle, Autoplay Radio, Clear -->
      <div class="flex items-center gap-2">
        <button
          @click="player.shuffleQueue()"
          class="flex-1 py-1.5 px-2.5 rounded-lg bg-zinc-900 hover:bg-zinc-800 border border-zinc-800 text-xs text-zinc-200 font-medium flex items-center justify-center gap-1.5 transition-colors"
          title="Embaralhar fila mantendo a música atual no topo"
        >
          <Shuffle class="w-3.5 h-3.5 text-red-400" />
          <span>Embaralhar</span>
        </button>

        <button
          @click="player.toggleAutoplayRadio()"
          :class="[
            'flex-1 py-1.5 px-2.5 rounded-lg border text-xs font-medium flex items-center justify-center gap-1.5 transition-colors',
            player.autoplayRadio
              ? 'bg-emerald-950/50 border-emerald-500/40 text-emerald-300'
              : 'bg-zinc-900 border-zinc-800 text-zinc-400 hover:text-zinc-200',
          ]"
          title="Busca automática via /youtubei/v1/next quando a fila chega ao fim"
        >
          <Radio class="w-3.5 h-3.5" />
          <span>Rádio {{ player.autoplayRadio ? 'ON' : 'OFF' }}</span>
        </button>

        <button
          @click="player.clearQueue()"
          class="py-1.5 px-2.5 rounded-lg bg-zinc-900 hover:bg-red-950/60 border border-zinc-800 hover:border-red-500/40 text-xs text-zinc-400 hover:text-red-300 font-medium flex items-center justify-center gap-1 transition-colors"
          title="Limpar próximas faixas da fila"
        >
          <Trash2 class="w-3.5 h-3.5" />
          <span>Limpar</span>
        </button>
      </div>
    </div>

    <!-- Scrollable Queue List -->
    <div class="flex-1 overflow-y-auto divide-y divide-zinc-800/50">
      <div
        v-for="(item, idx) in player.queue"
        :key="item.video_id + '-' + idx"
        @click="player.playQueueIndex(idx)"
        :class="[
          'group flex items-center justify-between px-4 py-2.5 cursor-pointer transition-colors',
          idx === player.queueIndex
            ? 'bg-red-950/35 border-l-2 border-l-red-500'
            : 'hover:bg-zinc-900/80',
        ]"
      >
        <div class="flex items-center gap-3 min-w-0">
          <!-- Track Index or Playing Indicator -->
          <div class="w-5 text-center shrink-0">
            <Volume2
              v-if="idx === player.queueIndex && player.isPlaying"
              class="w-4 h-4 text-red-400 mx-auto animate-pulse"
            />
            <Play
              v-else-if="idx === player.queueIndex"
              class="w-3.5 h-3.5 text-red-400 mx-auto fill-current"
            />
            <span v-else class="text-xs text-zinc-500 tabular-nums">{{ idx + 1 }}</span>
          </div>

          <!-- Thumbnail or Ultra-Low-RAM Placeholder -->
          <div
            class="w-9 h-9 rounded bg-zinc-800 overflow-hidden shrink-0 flex items-center justify-center"
          >
            <img
              v-if="item.thumbnail_url && !player.lowRamNoImages"
              :src="item.thumbnail_url"
              :alt="item.title"
              loading="lazy"
              decoding="async"
              class="w-full h-full object-cover"
            />
            <Music2 v-else class="w-4 h-4 text-zinc-400" />
          </div>

          <!-- Track Title & Artist -->
          <div class="min-w-0">
            <p
              :class="[
                'text-xs font-semibold truncate',
                idx === player.queueIndex ? 'text-red-400' : 'text-zinc-200',
              ]"
            >
              {{ item.title }}
            </p>
            <p class="text-[11px] text-zinc-400 truncate">{{ item.artist }}</p>
          </div>
        </div>

        <!-- Duration & Remove Button -->
        <div class="flex items-center gap-2 shrink-0 ml-2">
          <span class="text-[11px] text-zinc-500 tabular-nums">{{ item.duration_text }}</span>
          <button
            v-if="player.queue.length > 1"
            @click.stop="player.removeFromQueue(idx)"
            class="p-1 rounded text-zinc-500 hover:text-red-400 hover:bg-zinc-800 opacity-0 group-hover:opacity-100 transition-opacity"
            title="Remover da fila"
          >
            <X class="w-3.5 h-3.5" />
          </button>
        </div>
      </div>
    </div>

    <!-- Footer Info on Autoplay Radio -->
    <div class="p-3 bg-zinc-950 border-t border-zinc-800/80 text-[11px] text-zinc-400 leading-relaxed">
      <span class="text-emerald-400 font-semibold">Autoplay `/youtubei/v1/next`:</span>
      Quando a última música terminar, o Rust buscará recomendações de Rádio automaticamente.
    </div>
  </aside>
</template>
