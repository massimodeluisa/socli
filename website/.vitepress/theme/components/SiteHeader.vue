<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from 'vue'
import { useData, withBase } from 'vitepress'
import ThemeSwitch from './ThemeSwitch.vue'
import { write } from '../copy'
import { markSrc as sharedMark } from '../mark'
import type { SiteKind } from '../types'

const props = defineProps<{ kind: SiteKind }>()

const { theme } = useData()

const header = ref<HTMLElement | null>(null)
defineExpose({ header })

const markSrc = computed(() => sharedMark.value ?? withBase('/brand/mark.svg'))

const keysHref = computed(() =>
  props.kind === 'home' ? '#keys' : withBase('/docs/keys.html'),
)

const label = ref('install')
let timer: ReturnType<typeof setTimeout> | null = null

async function copyCommand(): Promise<void> {
  try {
    await write(theme.value.installCommand)
    label.value = 'copied'
  } catch {
    label.value = 'copy failed'
  }
  if (timer) clearTimeout(timer)
  timer = setTimeout(() => {
    label.value = 'install'
  }, 1600)
}

onBeforeUnmount(() => {
  if (timer) clearTimeout(timer)
})
</script>

<template>
  <header ref="header">
    <a class="brand" :href="withBase('/')" aria-label="SoCLI home">
      <img class="mark" :src="markSrc" width="22" height="22" alt="So_" />
      <span>SoCLI</span>
    </a>
    <nav aria-label="Site">
      <a
        :href="withBase('/docs/')"
        :aria-current="kind === 'docs' ? 'page' : undefined"
        >docs</a
      >
      <a
        :href="withBase('/design.html')"
        :aria-current="kind === 'book' ? 'page' : undefined"
        >brand</a
      >
      <a :href="keysHref">keys</a>
      <a :href="theme.repo">source</a>
    </nav>
    <ThemeSwitch />
    <button
      v-if="kind === 'docs'"
      type="button"
      class="head-install"
      :data-command="theme.installCommand"
      @click="copyCommand"
    >
      {{ label }}
    </button>
  </header>
</template>
