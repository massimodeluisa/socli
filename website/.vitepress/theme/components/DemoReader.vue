<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import { withBase } from 'vitepress'
import { mountReader } from '../reader'
import { mountWasmReader } from '../wasm-reader'

const root = ref<HTMLElement | null>(null)
const readerHref = withBase('/web/')
let stop: (() => void) | null = null

const sizes = ['mobile', 'tablet', 'desktop'] as const

function applySize(size: (typeof sizes)[number]): void {
  const term = root.value?.querySelector<HTMLElement>('#term')
  if (!term) return
  for (const name of sizes) term.classList.toggle(`is-${name}`, name === size)
  root.value?.querySelectorAll<HTMLButtonElement>('[data-size]').forEach((button) => {
    button.setAttribute('aria-pressed', button.dataset.size === size ? 'true' : 'false')
  })
}

onMounted(() => {
  const el = root.value
  if (!el) return
  el.querySelectorAll<HTMLButtonElement>('[data-size]').forEach((button) => {
    button.addEventListener('click', () => {
      const size = button.dataset.size
      if (size === 'mobile' || size === 'tablet' || size === 'desktop') applySize(size)
    })
  })
  let dead = false
  stop = () => {
    dead = true
  }
  mountWasmReader(el).then(
    (dispose) => {
      if (dead) dispose()
      else stop = dispose
    },
    () => {
      if (dead || !root.value) return
      stop = mountReader(el)
    },
  )
})

onBeforeUnmount(() => {
  if (stop) stop()
  stop = null
})
</script>

<template>
  <div ref="root">
    <p class="live vh" id="live" aria-live="polite"></p>

    <div class="term" id="term">
      <div class="term-bar">
        <span class="dots" aria-hidden="true"><i></i><i></i><i></i></span>
        <span class="term-title">socli</span>
        <span class="term-tools">
          <span class="term-sizes" role="group" aria-label="Window size">
            <button type="button" class="size-btn" data-size="mobile" aria-pressed="false" title="Mobile">
              <i></i><span class="vh">Mobile</span>
            </button>
            <button type="button" class="size-btn" data-size="tablet" aria-pressed="false" title="Tablet">
              <i></i><span class="vh">Tablet</span>
            </button>
            <button type="button" class="size-btn" data-size="desktop" aria-pressed="false" title="Desktop">
              <i></i><span class="vh">Desktop</span>
            </button>
            <a class="term-full" :href="readerHref" title="Full screen">Full screen</a>
          </span>
        </span>
      </div>
      <div class="screen" id="screen" tabindex="0" role="application" aria-label="socli reader">
        <div class="wasm-grid" id="grid"></div>
        <div class="chrome" id="chrome-top"></div>
        <div class="feed" id="feed"></div>
        <div class="chrome" id="chrome-bot"></div>
      </div>
    </div>
    <div class="touch">
      <button type="button" data-key="k">up</button>
      <button type="button" data-key="j">down</button>
      <button type="button" data-key="Enter">open</button>
      <button type="button" data-key="Escape">back</button>
    </div>
  </div>
</template>
