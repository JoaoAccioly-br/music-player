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
    class="fixed inset-0 z-50 bg-black/80 backdrop-blur-sm flex items-center justify-center p-4"
  >
    <div
      class="w-full max-w-lg rounded-2xl bg-zinc-900 border border-zinc-700/90 shadow-2xl p-6 space-y-5"
    >
      <div class="flex items-center justify-between border-b border-zinc-800 pb-3.5">
        <div class="flex items-center gap-2.5">
          <KeyRound class="w-5 h-5 text-red-500" />
          <h3 class="text-base font-bold text-white">
            Autenticação Google — YouTube Music (`SAPISIDHASH`)
          </h3>
        </div>
        <button
          @click="emit('close')"
          class="p-1.5 rounded-lg text-zinc-400 hover:text-white hover:bg-zinc-800"
        >
          <X class="w-4 h-4" />
        </button>
      </div>

      <!-- Current Auth Status Banner -->
      <div
        :class="[
          'p-3.5 rounded-xl border text-xs flex items-center justify-between',
          player.authStatus.is_authenticated
            ? 'bg-emerald-950/40 border-emerald-500/30 text-emerald-200'
            : 'bg-zinc-950/80 border-zinc-800 text-zinc-300',
        ]"
      >
        <div class="flex items-center gap-2.5">
          <ShieldCheck
            :class="[
              'w-4 h-4 shrink-0',
              player.authStatus.is_authenticated ? 'text-emerald-400' : 'text-zinc-500',
            ]"
          />
          <div>
            <p class="font-semibold">
              {{
                player.authStatus.is_authenticated
                  ? `Sessão Autenticada (${player.authStatus.account_hint})`
                  : 'Nenhuma conta conectada no momento'
              }}
            </p>
            <p class="text-[11px] text-zinc-400">
              Arquivo local: <code>~/.local/share/com.ytmusic.rust/auth.json</code> (Permissão Unix
              <code>0600</code>)
            </p>
          </div>
        </div>

        <button
          v-if="player.authStatus.is_authenticated"
          @click="player.logout()"
          class="px-3 py-1.5 rounded-lg bg-red-950/80 hover:bg-red-900 border border-red-500/40 text-red-200 text-xs font-medium flex items-center gap-1.5 shrink-0"
        >
          <LogOut class="w-3.5 h-3.5" />
          <span>Sair</span>
        </button>
      </div>

      <!-- Option 1: Temporary Login WebView -->
      <div class="p-4 rounded-xl bg-zinc-950/70 border border-zinc-800/90 space-y-2.5">
        <p class="text-xs font-semibold text-white">
          Opção 1: Login via Janela Temporária (Captura Automática + Zero-RAM)
        </p>
        <p class="text-xs text-zinc-400 leading-relaxed">
          Abre uma janela secundária efêmera no Google Login. Assim que o cookie
          <code>SAPISID</code> é detectado, o Rust salva a sessão em disco e
          <strong>destrói a janela de login automaticamente</strong>.
        </p>
        <button
          @click="player.openLoginWindow()"
          class="w-full py-2.5 px-4 rounded-xl bg-red-600 hover:bg-red-500 text-xs font-semibold text-white flex items-center justify-center gap-2 transition-colors shadow-lg shadow-red-600/20"
        >
          <ExternalLink class="w-4 h-4" />
          <span>Abrir Janela de Login do Google</span>
        </button>
      </div>

      <!-- Option 2: Manual Cookie Fallback -->
      <div class="p-4 rounded-xl bg-zinc-950/70 border border-zinc-800/90 space-y-2.5">
        <p class="text-xs font-semibold text-white">
          Opção 2: Fallback Manual (Colar Cookie ou Token `SAPISID` do Navegador)
        </p>
        <p class="text-[11px] text-zinc-400 leading-relaxed">
          Se o Google bloquear o WebKitGTK no Linux, abra <code>music.youtube.com</code> no seu
          navegador padrão, aperte <code>F12 → Network</code> e cole o cabeçalho
          <code>Cookie: ...</code> completo (ou o valor de <code>SAPISID</code>):
        </p>
        <form @submit.prevent="handleSaveManualCookie" class="space-y-2.5">
          <textarea
            v-model="manualCookieInput"
            rows="3"
            placeholder="Cole aqui: SAPISID=xxx/yyy; __Secure-3PAPISID=xxx/yyy; SID=... (ou apenas o valor do SAPISID)"
            class="w-full bg-zinc-900 text-xs text-white placeholder-zinc-500 p-3 rounded-xl border border-zinc-800 focus:border-red-500/60 focus:outline-none font-mono"
          ></textarea>
          <button
            type="submit"
            class="w-full py-2 px-4 rounded-xl bg-zinc-800 hover:bg-zinc-700 text-xs font-semibold text-white flex items-center justify-center gap-2 transition-colors"
          >
            <CheckCircle2 class="w-4 h-4 text-emerald-400" />
            <span>Validar `SAPISID` e Salvar em `auth.json` (0600)</span>
          </button>
        </form>

        <p v-if="manualCookieSuccess" class="text-xs text-emerald-400 font-medium">
          ✓ Sessão validada e gravada com permissão 0600!
        </p>
        <p v-if="player.authError" class="text-xs text-red-400">
          {{ player.authError }}
        </p>
      </div>
    </div>
  </div>
</template>
