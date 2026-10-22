<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { withBase } from 'vitepress'
import { markSrc } from '../mark'

type ThemeName = 'dark' | 'light' | 'auto'

const names: ThemeName[] = ['dark', 'light', 'auto']
const current = ref<ThemeName>('auto')
const darkQuery = ref<MediaQueryList | null>(null)

function isDark(theme: ThemeName): boolean {
  return theme === 'dark' || (theme === 'auto' && darkQuery.value?.matches === true)
}

function paint(theme: ThemeName): void {
  const dark = isDark(theme)
  document.documentElement.setAttribute('data-theme', theme)
  document.documentElement.style.colorScheme = dark ? 'dark' : 'light'
  const meta = document.querySelector('meta[name="theme-color"]')
  if (meta) meta.setAttribute('content', dark ? '#000000' : '#ffffff')
  const square = dark ? '/brand/so-light-text.svg' : '/brand/so-dark-text.svg'
  markSrc.value = withBase(square)
  const icon = document.querySelector('link[rel="icon"][type="image/svg+xml"]')
  if (icon) icon.setAttribute('href', withBase(theme === 'auto' ? '/favicon.svg' : square))
  try {
    localStorage.setItem('socli-theme', theme)
  } catch {
    /* private mode */
  }
  current.value = theme
}

function onDarkChange(): void {
  if (current.value === 'auto') paint('auto')
}

onMounted(() => {
  darkQuery.value = window.matchMedia('(prefers-color-scheme: dark)')
  const attribute = document.documentElement.getAttribute('data-theme')
  current.value = attribute === 'dark' || attribute === 'light' || attribute === 'auto' ? attribute : 'auto'
  paint(current.value)
  darkQuery.value.addEventListener('change', onDarkChange)
})

onBeforeUnmount(() => {
  darkQuery.value?.removeEventListener('change', onDarkChange)
})
</script>

<template>
  <div class="themes" role="radiogroup" aria-label="Theme">
    <button
      v-for="name in names"
      :key="name"
      type="button"
      role="radio"
      :data-theme="name"
      :aria-label="name === 'auto' ? 'Auto, follow the browser' : name"
      :aria-checked="current === name ? 'true' : 'false'"
      @click="paint(name)"
    >
      {{ name }}
    </button>
  </div>
</template>
