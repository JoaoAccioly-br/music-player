<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import {
  Image as ImageIcon,
  ImageOff,
  Moon,
  Play,
  Search,
  Sun,
  X,
} from 'lucide-vue-next'
import { usePlayerStore } from '../stores/player'

const props = defineProps<{
  activeTab: 'home' | 'explore' | 'library' | 'liked'
  searchQuery: string
  isSearchActive?: boolean
}>()

const emit = defineEmits<{
  (e: 'change-tab', tab: 'home' | 'explore' | 'library' | 'liked'): void
  (e: 'search', query: string): void
  (e: 'clear-search'): void
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

const navItems: Array<{ id: 'home' | 'explore' | 'library' | 'liked'; label: string }> = [
  { id: 'home', label: 'Início' },
  { id: 'explore', label: 'Explorar' },
  { id: 'library', label: 'Biblioteca' },
  { id: 'liked', label: 'Curtidas' },
]

const accountInitial = computed(() => {
  if (!player.authStatus.is_authenticated) {
    return 'V'
  }
  const hint = (player.authStatus.account_hint || '').trim()
  if (hint && hint !== 'Não conectado') {
    return hint.charAt(0).toUpperCase()
  }
  return 'U'
})

function onSubmit() {
  const q = localQuery.value.trim()
  if (!q) {
    emit('clear-search')
    return
  }
  emit('search', q)
}

function handleClearSearch() {
  localQuery.value = ''
  emit('clear-search')
}

function handleLogoClick() {
  localQuery.value = ''
  emit('clear-search')
  emit('change-tab', 'home')
}
</script>

<template>
  <header
    class="h-16 px-4 sm:px-8 flex items-center justify-between gap-3 shrink-0 z-20 transition-colors duration-200 border-b border-[rgba(120,100,180,0.1)] dark:border-white/[0.06] bg-[#F7F6FA]/85 dark:bg-[#12101B]/85 backdrop-blur-xl"
  >
    <!-- Left: Unified Brand Logo -->
    <button
      type="button"
      @click="handleLogoClick"
      class="flex items-center gap-2.5 group focus:outline-none shrink-0"
    >
      <div
        class="w-8 h-8 rounded-full bg-gradient-to-br from-[#8B6EFF] via-[#B497FF] to-[#F4A2B8] flex items-center justify-center shadow-sm group-hover:scale-105 transition-transform"
      >
        <div class="w-4 h-4 rounded-full bg-white/90 flex items-center justify-center">
          <Play class="w-2.5 h-2.5 text-[#6D4AFF] fill-current ml-0.5" />
        </div>
      </div>
      <span class="font-bold text-base tracking-tight text-[#1A162B] dark:text-[#F4F2FA]">
        Music
      </span>
    </button>

    <!-- Center: Pill Navigation ("Navegação do C") -->
    <nav
      aria-label="Navegação Principal"
      class="flex items-center p-1 rounded-full bg-[#EDE9F5]/90 dark:bg-[#1E1A2D]/95 border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07] shadow-sm"
    >
      <button
        v-for="item in navItems"
        :key="item.id"
        type="button"
        @click="emit('change-tab', item.id)"
        :class="[
          'px-3.5 sm:px-4 py-1.5 rounded-full text-xs font-semibold transition-all duration-150',
          activeTab === item.id && !isSearchActive
            ? 'bg-white dark:bg-[#2C2640] text-[#1A162B] dark:text-[#F4F2FA] shadow-sm'
            : 'text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-[#F4F2FA]',
        ]"
      >
        {{ item.label }}
      </button>
    </nav>

    <!-- Right: Search Pill + Theme Toggle (Moon/Sun) + Low-RAM Toggle + Avatar -->
    <div class="flex items-center gap-2 sm:gap-2.5 shrink-0">
      <!-- Search Bar -->
      <form @submit.prevent="onSubmit" class="relative flex items-center">
        <Search
          class="w-3.5 h-3.5 text-[#6E6885] dark:text-[#9B94B2] absolute left-3.5 pointer-events-none"
        />
        <input
          v-model="localQuery"
          type="text"
          placeholder="Buscar"
          class="w-36 sm:w-52 md:w-60 focus:w-48 sm:focus:w-64 md:focus:w-72 h-9 pl-9 pr-8 rounded-full text-xs font-medium bg-white/90 dark:bg-[#1E1A2D] text-[#1A162B] dark:text-[#F4F2FA] placeholder-[#6E6885] dark:placeholder-[#9B94B2] border border-[rgba(120,100,180,0.14)] dark:border-white/[0.08] focus:border-[#7C5CFC] dark:focus:border-[#8B6EFF] focus:outline-none shadow-sm transition-all duration-200"
        />
        <button
          v-if="localQuery"
          type="button"
          @click="handleClearSearch"
          title="Limpar busca"
          class="absolute right-2.5 p-0.5 rounded-full text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white transition-colors"
        >
          <X class="w-3.5 h-3.5" />
        </button>
        <span
          v-else
          class="hidden sm:inline-block absolute right-3 text-[10px] font-medium text-[#9B94B2] dark:text-[#6E6885] pointer-events-none"
        >
          ↵
        </span>
      </form>

      <!-- Low-RAM Mode (Images On/Off) + RAM Buffer Tooltip -->
      <button
        type="button"
        @click="player.toggleLowRamImages()"
        :title="
          player.lowRamNoImages
            ? `Modo Economia de RAM Ativo (Sem capas) • Buffer: ${player.bufferSizeMb.toFixed(1)} MB`
            : `Capas ativas • Buffer RAM: ${player.bufferSizeMb.toFixed(1)} MB (Clique para Modo Ultra-RAM)`
        "
        :class="[
          'w-9 h-9 rounded-full flex items-center justify-center border transition-all',
          player.lowRamNoImages
            ? 'bg-[#7C5CFC]/15 border-[#7C5CFC]/40 text-[#7C5CFC] dark:text-[#B497FF]'
            : 'bg-white/90 dark:bg-[#1E1A2D] border-[rgba(120,100,180,0.14)] dark:border-white/[0.08] text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-[#F4F2FA]',
        ]"
      >
        <ImageOff v-if="player.lowRamNoImages" class="w-4 h-4" />
        <ImageIcon v-else class="w-4 h-4" />
      </button>

      <!-- Theme Toggle Button (Moon / Sun) -->
      <button
        type="button"
        @click="player.toggleTheme()"
        :title="
          player.theme === 'dark'
            ? 'Alternar para Tema Claro'
            : 'Alternar para Tema Escuro'
        "
        class="w-9 h-9 rounded-full flex items-center justify-center bg-white/90 dark:bg-[#1E1A2D] border border-[rgba(120,100,180,0.14)] dark:border-white/[0.08] text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-[#F4F2FA] shadow-sm transition-all hover:scale-105"
      >
        <Moon v-if="player.theme === 'light'" class="w-4 h-4" />
        <Sun v-else class="w-4 h-4 text-[#FDEBC8]" />
      </button>

      <!-- User Avatar / Google Auth Trigger -->
      <button
        type="button"
        @click="emit('open-auth')"
        :title="
          player.authStatus.is_authenticated
            ? `Conta Google conectada (${player.authStatus.account_hint})`
            : 'Conectar Conta Google (SAPISIDHASH)'
        "
        class="relative w-9 h-9 rounded-full bg-gradient-to-br from-[#F7C6D0] via-[#E5C6FF] to-[#C9D6FF] text-[#1A162B] font-bold text-xs flex items-center justify-center shadow-sm hover:scale-105 transition-transform border border-white/60 dark:border-white/15"
      >
        <span>{{ accountInitial }}</span>
        <span
          :class="[
            'absolute -bottom-0.5 -right-0.5 w-2.5 h-2.5 rounded-full ring-2 ring-[#F7F6FA] dark:ring-[#12101B]',
            player.authStatus.is_authenticated
              ? 'bg-[#22C55E]'
              : 'bg-[#9B94B2] dark:bg-[#6E6885]',
          ]"
        ></span>
      </button>
    </div>
  </header>
</template>
