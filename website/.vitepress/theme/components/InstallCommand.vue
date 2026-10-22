<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { useData } from 'vitepress'
import { write } from '../copy'

const { theme } = useData()

const command = computed(() => theme.value.installCommand)
const copied = ref(false)
const live = ref('')
let timer: ReturnType<typeof setTimeout> | null = null

async function copyCommand(): Promise<void> {
  try {
    await write(command.value)
    copied.value = true
    live.value = 'Copied install command.'
  } catch {
    live.value = 'Could not copy. Select the command and copy it.'
  }
  if (timer) clearTimeout(timer)
  timer = setTimeout(() => {
    copied.value = false
  }, 1600)
}

onBeforeUnmount(() => {
  if (timer) clearTimeout(timer)
})
</script>

<template>
  <p class="install">
    <button
      type="button"
      id="copy"
      :data-command="command"
      :aria-label="copied ? 'Copied' : 'Copy install command'"
      @click="copyCommand"
    >
      <span aria-hidden="true"
        ><span class="prompt">$ </span><span class="cmd">{{
          copied ? 'copied' : command
        }}</span></span
      >
    </button>
  </p>
  <p class="live vh" aria-live="polite">{{ live }}</p>
</template>
