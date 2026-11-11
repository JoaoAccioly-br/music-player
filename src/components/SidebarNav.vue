<script setup lang="ts">
import { Compass, Heart, Home, Library, ListMusic, Radio } from 'lucide-vue-next'
import { usePlayerStore } from '../stores/player'

defineProps<{
  activeTab: 'home' | 'explore' | 'library' | 'liked'
}>()

const emit = defineEmits<{
  (e: 'change-tab', tab: 'home' | 'explore' | 'library' | 'liked'): void
}>()

const player = usePlayerStore()
</script>

<template>
  <aside
    class="w-60 bg-[#030303] border-r border-zinc-800/70 p-3 flex flex-col justify-between shrink-0"
  >
    <div class="space-y-4">
      <nav class="space-y-1">
        <button
          @click="emit('change-tab', 'home')"
          :class="[
            'w-full flex items-center gap-3.5 px-3.5 py-2.5 rounded-lg text-sm font-medium transition-colors',
            activeTab === 'home'
              ? 'bg-zinc-800/90 text-white'
              : 'text-zinc-400 hover:text-white hover:bg-zinc-900',
          ]"
        >
          <Home class="w-4 h-4" />
          <span>Início</span>
        </button>

        <button
          @click="emit('change-tab', 'explore')"
          :class="[
            'w-full flex items-center gap-3.5 px-3.5 py-2.5 rounded-lg text-sm font-medium transition-colors',
            activeTab === 'explore'
              ? 'bg-zinc-800/90 text-white'
              : 'text-zinc-400 hover:text-white hover:bg-zinc-900',
          ]"
        >
          <Compass class="w-4 h-4" />
          <span>Explorar &amp; Busca</span>
        </button>

        <button
          @click="emit('change-tab', 'library')"
          :class="[
            'w-full flex items-center gap-3.5 px-3.5 py-2.5 rounded-lg text-sm font-medium transition-colors',
            activeTab === 'library'
              ? 'bg-zinc-800/90 text-white'
              : 'text-zinc-400 hover:text-white hover:bg-zinc-900',
          ]"
        >
          <Library class="w-4 h-4" />
          <span>Biblioteca &amp; Playlists</span>
        </button>

        <button
          @click="emit('change-tab', 'liked')"
          :class="[
            'w-full flex items-center gap-3.5 px-3.5 py-2.5 rounded-lg text-sm font-medium transition-colors',
            activeTab === 'liked'
              ? 'bg-zinc-800/90 text-white'
              : 'text-zinc-400 hover:text-white hover:bg-zinc-900',
          ]"
        >
          <Heart class="w-4 h-4 text-red-500" />
          <span>Músicas Curtidas</span>
        </button>
      </nav>

      <!-- Quick Queue & Infinite Radio Autoplay Controls -->
      <div class="pt-3 border-t border-zinc-800/80 space-y-2">
        <button
          @click="player.toggleQueueDrawer()"
          :class="[
            'w-full flex items-center justify-between px-3.5 py-2 rounded-lg text-xs font-medium border transition-colors',
            player.isQueueDrawerOpen
              ? 'bg-red-950/40 border-red-500/40 text-red-300'
              : 'bg-zinc-900/60 hover:bg-zinc-800/80 border-zinc-800 text-zinc-300',
          ]"
        >
          <span class="flex items-center gap-2">
            <ListMusic class="w-3.5 h-3.5 text-red-400" />
            <span>Fila de Reprodução</span>
          </span>
          <span class="px-1.5 py-0.5 rounded bg-zinc-800 text-[10px] tabular-nums text-zinc-200">
            {{ player.queue.length }}
          </span>
        </button>

        <button
          @click="player.toggleAutoplayRadio()"
          :class="[
            'w-full flex items-center justify-between px-3.5 py-2 rounded-lg text-xs font-medium border transition-colors',
            player.autoplayRadio
              ? 'bg-emerald-950/40 border-emerald-500/30 text-emerald-300'
              : 'bg-zinc-900/60 border-zinc-800 text-zinc-400 hover:text-zinc-200',
          ]"
          title="Quando a fila chega ao fim, o Rust consulta /youtubei/v1/next automaticamente para manter a música tocando sem parar durante o jogo"
        >
          <span class="flex items-center gap-2">
            <Radio
              :class="[
                'w-3.5 h-3.5',
                player.autoplayRadio ? 'text-emerald-400 animate-pulse' : 'text-zinc-500',
              ]"
            />
            <span>Rádio Autoplay</span>
          </span>
          <span class="text-[10px] uppercase font-bold">
            {{ player.autoplayRadio ? 'ON' : 'OFF' }}
          </span>
        </button>
      </div>
    </div>

    <!-- Rust Audio & Auth Telemetry Card -->
    <div class="p-3.5 rounded-xl bg-zinc-900/70 border border-zinc-800/90 space-y-2">
      <div class="flex items-center justify-between">
        <span class="text-[11px] font-semibold uppercase tracking-wider text-zinc-400">
          Rodio + Symphonia
        </span>
        <span
          :class="[
            'w-2 h-2 rounded-full',
            player.isTauriConnected
              ? 'bg-emerald-400 shadow-sm shadow-emerald-400'
              : 'bg-amber-400',
          ]"
        ></span>
      </div>
      <p class="text-[11px] text-zinc-300 leading-relaxed truncate" :title="player.audioQualityLabel">
        Codec: <strong class="text-red-400">{{ player.audioQualityLabel }}</strong>
      </p>
      <p class="text-[10px] text-zinc-400 leading-relaxed">
        Auth:
        <strong :class="player.authStatus.is_authenticated ? 'text-emerald-400' : 'text-amber-400'">
          {{ player.authStatus.is_authenticated ? 'SAPISIDHASH (0600)' : 'Modo Público' }}
        </strong>
      </p>
      <p class="text-[10px] text-zinc-400 leading-relaxed">
        Video ID:
        <code class="text-zinc-200 bg-zinc-800 px-1 py-0.5 rounded">{{ player.videoId }}</code>
      </p>
    </div>
  </aside>
</template>
