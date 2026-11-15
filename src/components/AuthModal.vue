<script setup lang="ts">
import { ref } from 'vue'
import { CheckCircle2, ExternalLink, KeyRound, LogOut, ShieldCheck, X } from 'lucide-vue-next'
import { usePlayerStore } from '../stores/player'

defineProps<{
  open: boolean
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

const player = usePlayerStore()
const manualCookieInput = ref('')
const manualCookieSuccess = ref(false)

async function handleSaveManualCookie() {
  if (!manualCookieInput.value.trim()) return
  manualCookieSuccess.value = false
  const ok = await player.setManualCookie(manualCookieInput.value)
  if (ok) {
    manualCookieInput.value = ''
    manualCookieSuccess.value = true
    setTimeout(() => {
      manualCookieSuccess.value = false
      emit('close')
    }, 900)
  }
}
</script>

<template>
  <div
    v-if="open"
    class="fixed inset-0 z-50 bg-black/60 backdrop-blur-md flex items-center justify-center p-4"
  >
    <div
      class="w-full max-w-lg rounded-3xl bg-white dark:bg-[#1E1A2D] border border-[rgba(120,100,180,0.16)] dark:border-white/10 shadow-2xl p-6 space-y-5 text-[#1A162B] dark:text-[#F4F2FA]"
    >
      <div
        class="flex items-center justify-between border-b border-[rgba(120,100,180,0.12)] dark:border-white/[0.08] pb-3.5"
      >
        <div class="flex items-center gap-2.5">
          <div
            class="w-8 h-8 rounded-full bg-gradient-to-br from-[#8B6EFF] to-[#F4A2B8] flex items-center justify-center text-white"
          >
            <KeyRound class="w-4 h-4" />
          </div>
          <div>
            <h3 class="text-sm sm:text-base font-bold">
              Conta Google — YouTube Music
            </h3>
            <p class="text-[11px] text-[#6E6885] dark:text-[#9B94B2]">
              Sincronização de biblioteca e músicas curtidas (`SAPISIDHASH`)
            </p>
          </div>
        </div>
        <button
          type="button"
          @click="emit('close')"
          class="p-1.5 rounded-full text-[#6E6885] dark:text-[#9B94B2] hover:text-[#1A162B] dark:hover:text-white hover:bg-black/5 dark:hover:bg-white/10 transition-colors"
        >
          <X class="w-4 h-4" />
        </button>
      </div>

      <!-- Current Auth Status Banner -->
      <div
        :class="[
          'p-3.5 rounded-2xl border text-xs flex items-center justify-between gap-3',
          player.authStatus.is_authenticated
            ? 'bg-[#22C55E]/10 border-[#22C55E]/30 text-[#15803D] dark:text-[#86EFAC]'
            : 'bg-[#F2EFF9] dark:bg-[#161321] border-[rgba(120,100,180,0.12)] dark:border-white/[0.07] text-[#1A162B] dark:text-[#F4F2FA]',
        ]"
      >
        <div class="flex items-center gap-2.5 min-w-0">
          <ShieldCheck
            :class="[
              'w-4 h-4 shrink-0',
              player.authStatus.is_authenticated ? 'text-[#22C55E]' : 'text-[#7C5CFC]',
            ]"
          />
          <div class="min-w-0">
            <p class="font-semibold truncate">
              {{
                player.authStatus.is_authenticated
                  ? `Conectado (${player.authStatus.account_hint})`
                  : 'Modo visitante ativo'
              }}
            </p>
            <p class="text-[11px] text-[#6E6885] dark:text-[#9B94B2] truncate">
              Sessão local segura: <code>~/.local/share/com.ytmusic.rust/auth.json</code>
            </p>
          </div>
        </div>

        <button
          v-if="player.authStatus.is_authenticated"
          type="button"
          @click="player.logout()"
          class="px-3 py-1.5 rounded-full bg-rose-500/15 hover:bg-rose-500/25 border border-rose-500/30 text-rose-600 dark:text-rose-300 text-xs font-semibold flex items-center gap-1.5 shrink-0 transition-colors"
        >
          <LogOut class="w-3.5 h-3.5" />
          <span>Sair</span>
        </button>
      </div>

      <!-- Option 1: Temporary Login WebView -->
      <div
        class="p-4 rounded-2xl bg-[#F7F6FA] dark:bg-[#161321] border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07] space-y-2.5"
      >
        <p class="text-xs font-bold">
          Opção 1: Login via Janela Temporária (Captura Automática)
        </p>
        <p class="text-xs text-[#6E6885] dark:text-[#9B94B2] leading-relaxed">
          Abre uma janela efêmera no Google Login. Assim que o cookie <code>SAPISID</code> é
          detectado, a sessão é salva e a janela é fechada automaticamente para poupar RAM.
        </p>
        <button
          type="button"
          @click="player.openLoginWindow()"
          class="w-full py-2.5 px-4 rounded-full bg-[#7C5CFC] hover:bg-[#6D4AFF] text-xs font-semibold text-white flex items-center justify-center gap-2 transition-colors shadow-md shadow-[#7C5CFC]/25"
        >
          <ExternalLink class="w-4 h-4" />
          <span>Abrir Janela de Login do Google</span>
        </button>
      </div>

      <!-- Option 2: Manual Cookie Fallback -->
      <div
        class="p-4 rounded-2xl bg-[#F7F6FA] dark:bg-[#161321] border border-[rgba(120,100,180,0.12)] dark:border-white/[0.07] space-y-2.5"
      >
        <p class="text-xs font-bold">
          Opção 2: Colar Cookie ou Token `SAPISID` do Navegador
        </p>
        <p class="text-[11px] text-[#6E6885] dark:text-[#9B94B2] leading-relaxed">
          Abra <code>music.youtube.com</code> no navegador, aperte <code>F12 → Network</code> e cole
          o cabeçalho <code>Cookie: ...</code> completo:
        </p>
        <form @submit.prevent="handleSaveManualCookie" class="space-y-2.5">
          <textarea
            v-model="manualCookieInput"
            rows="3"
            placeholder="Cole aqui: SAPISID=xxx/yyy; __Secure-3PAPISID=xxx/yyy; SID=..."
            class="w-full bg-white dark:bg-[#1E1A2D] text-xs text-[#1A162B] dark:text-[#F4F2FA] placeholder-[#9B94B2] p-3 rounded-2xl border border-[rgba(120,100,180,0.16)] dark:border-white/10 focus:border-[#7C5CFC] focus:outline-none font-mono"
          ></textarea>
          <button
            type="submit"
            class="w-full py-2.5 px-4 rounded-full bg-[#EDE9F5] dark:bg-[#252038] hover:bg-[#E2DCF2] dark:hover:bg-[#2F2946] text-xs font-semibold text-[#1A162B] dark:text-[#F4F2FA] flex items-center justify-center gap-2 transition-colors"
          >
            <CheckCircle2 class="w-4 h-4 text-[#22C55E]" />
            <span>Validar e Salvar Sessão</span>
          </button>
        </form>

        <p v-if="manualCookieSuccess" class="text-xs text-[#16A34A] dark:text-[#4ADE80] font-medium">
          ✓ Sessão validada e sincronizada com sucesso!
        </p>
        <p v-if="player.authError" class="text-xs text-rose-500">
          {{ player.authError }}
        </p>
      </div>
    </div>
  </div>
</template>
