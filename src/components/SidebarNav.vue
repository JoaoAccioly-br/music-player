<script setup lang="ts">
defineProps<{
  activeTab: 'home' | 'explore' | 'library' | 'liked'
}>()

defineEmits<{
  (e: 'change-tab', tab: 'home' | 'explore' | 'library' | 'liked'): void
}>()
</script>

<template>
  <!-- Navigation is now unified inside HeaderBar ("Navegação do C") -->
  <div class="hidden"></div>
</template>
