<script setup lang="ts">
import { ref, watch } from 'vue'
import {
  Cpu,
  Disc3,
  HardDrive,
  ImageOff,
  Image as ImageIcon,
  KeyRound,
  Search,
  UserCheck,
} from 'lucide-vue-next'
import { usePlayerStore } from '../stores/player'

const props = defineProps<{
  searchQuery: string
}>()

const emit = defineEmits<{
  (e: 'search', query: string): void
  (e: 'open-auth'): void
}>()

const player = usePlayerStore()
const localQuery = ref(props.searchQuery)

watch(
  () => props.searchQuery,
  (newVal) => {
    localQuery.value = newVal
  }
)

function onSubmit() {
  const q = localQuery.value.trim()
  if (!q) return
  emit('search', q)
}
</script>

<template>
  <header
    class="h-16 px-6 flex items-center justify-between border-b border-zinc-800/80 bg-[#030303]/95 backdrop-blur-md shrink-0 z-10"
  >
    <!-- Brand Logo -->
    <div class="flex items-center gap-3">
      <div
        class="w-9 h-9 rounded-full bg-red-600 flex items-center justify-center shadow-lg shadow-red-600/30"
      >
        <Disc3
          class="w-5 h-5 text-white animate-spin"
          :style="{ animationDuration: player.isPlaying ? '4s' : '0s' }"
        />
      </div>
      <div>
        <div class="flex items-center gap-2">
          <span class="font-bold text-lg tracking-tight">Music</span>
          <span
            class="text-[10px] font-semibold uppercase tracking-wider px-2 py-0.5 rounded-full bg-red-600/20 text-red-400 border border-red-500/30"
          >
            Rust Edition
          </span>
        </div>
        <p class="text-[11px] text-zinc-400 leading-none">
          Fase 4 &amp; 5 • Rádio Infinita &amp; Modo Ultra-Economia
        </p>
      </div>
    </div>

    <!-- Search Input (Innertube Search) -->
    <form @submit.prevent="onSubmit" class="flex-1 max-w-md mx-6">
      <div class="relative flex items-center">
        <Search class="w-4 h-4 text-zinc-400 absolute left-3.5 pointer-events-none" />
        <input
          v-model="localQuery"
          type="text"
          placeholder="Pesquisar músicas, artistas no YouTube Music (Enter)..."
          class="w-full bg-zinc-900/90 hover:bg-zinc-800/90 focus:bg-zinc-900 text-sm text-white placeholder-zinc-500 pl-10 pr-20 py-2 rounded-full border border-zinc-800 focus:border-red-500/60 focus:outline-none transition-colors"
        />
        <button
          type="submit"
          class="absolute right-1.5 px-3 py-1 rounded-full bg-zinc-800 hover:bg-red-600 text-[11px] font-medium text-zinc-200 hover:text-white transition-colors"
        >
          Buscar
        </button>
      </div>
    </form>

    <!-- Ultra-Low-RAM Image Toggle, RAM Buffer, Gaming Mode & Google Auth Status Badges -->
    <div class="flex items-center gap-2.5">
      <!-- Modo Ultra-Economia (Sem Imagens) Toggle -->
      <button
        @click="player.toggleLowRamImages()"
        :title="
          player.lowRamNoImages
            ? 'Modo Ultra-Economia Ativo: Capas desativadas para poupar memória de texturas da WebView'
            : 'Clique para ativar o Modo Ultra-Economia (Sem Imagens) e poupar RAM da WebView'
        "
        :class="[
          'flex items-center gap-1.5 px-3 py-1.5 rounded-full border text-xs font-medium transition-all',
          player.lowRamNoImages
            ? 'bg-amber-950/60 border-amber-500/40 text-amber-300 hover:bg-amber-900/50'
            : 'bg-zinc-900 hover:bg-zinc-800 border-zinc-700/80 text-zinc-300',
        ]"
      >
        <ImageOff v-if="player.lowRamNoImages" class="w-3.5 h-3.5 text-amber-400 shrink-0" />
        <ImageIcon v-else class="w-3.5 h-3.5 text-zinc-400 shrink-0" />
        <span class="hidden md:inline">
          {{ player.lowRamNoImages ? 'Sem Imagens (Ultra-RAM)' : 'Capas Lazy' }}
        </span>
      </button>

      <!-- RAM Buffer Badge -->
      <div
        class="flex items-center gap-2 px-3 py-1.5 rounded-full bg-zinc-900 border border-zinc-700/80 text-zinc-200 text-xs font-medium"
        title="Tamanho do buffer de áudio atual alocado em Cursor<Vec<u8>> na RAM do Rust (0 escrita em disco)"
      >
        <HardDrive class="w-3.5 h-3.5 text-red-400 shrink-0" />
        <span class="tabular-nums">{{ player.bufferSizeMb.toFixed(2) }} MB RAM</span>
      </div>

      <!-- Zero-WebView Badge -->
      <div
        class="hidden xl:flex items-center gap-2 px-3 py-1.5 rounded-full bg-emerald-950/50 border border-emerald-500/30 text-emerald-300 text-xs font-medium"
      >
        <Cpu class="w-3.5 h-3.5 text-emerald-400 shrink-0" />
        <span>Zero-WebView</span>
      </div>

      <!-- Google Account Auth Button -->
      <button
        @click="emit('open-auth')"
        :class="[
          'flex items-center gap-2 px-3.5 py-1.5 rounded-full border text-xs font-semibold transition-all',
          player.authStatus.is_authenticated
            ? 'bg-emerald-950/70 border-emerald-500/40 text-emerald-300 hover:bg-emerald-900/60'
            : 'bg-red-600/90 hover:bg-red-500 border-red-500 text-white shadow-md shadow-red-600/20',
        ]"
      >
        <UserCheck
          v-if="player.authStatus.is_authenticated"
          class="w-3.5 h-3.5 text-emerald-400 shrink-0"
        />
        <KeyRound v-else class="w-3.5 h-3.5 shrink-0" />
        <span>
          {{
            player.authStatus.is_authenticated
              ? `Google Ativo (${player.authStatus.sapisid_preview})`
              : 'Conectar Conta Google'
          }}
        </span>
      </button>
    </div>
  </header>
</template>
