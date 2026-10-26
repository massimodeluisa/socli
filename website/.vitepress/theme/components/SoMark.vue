<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'

const FULL = 'SoCLI'
const KEEP = 'So'
const TAIL = FULL.slice(KEEP.length)

const shown = ref('')
const split = ref(false)
const gone = ref(false)
const blink = ref(false)
const hovering = ref(false)
const reduce = ref(false)

let timers: ReturnType<typeof setTimeout>[] = []

function clearTimers(): void {
  for (const timer of timers) clearTimeout(timer)
  timers = []
}

function later(ms: number, run: () => void): void {
  timers.push(setTimeout(run, ms))
}

function settleSo(): void {
  shown.value = KEEP
  split.value = true
  gone.value = true
  blink.value = !reduce.value && !hovering.value
}

function writeFrom(count: number): void {
  shown.value = FULL.slice(0, count)
  split.value = false
  blink.value = false
  if (!hovering.value) return
  if (count < FULL.length) {
    later(72, () => writeFrom(count + 1))
    return
  }
}

function eraseTo(count: number): void {
  if (hovering.value) return
  shown.value = FULL.slice(0, count)
  split.value = false
  blink.value = false
  if (count > KEEP.length) {
    later(48, () => eraseTo(count - 1))
    return
  }
  settleSo()
}

function compact(): void {
  if (hovering.value || reduce.value) return
  split.value = true
  gone.value = false
  later(32, () => {
    if (hovering.value) return
    gone.value = true
    later(620, () => {
      if (!hovering.value) blink.value = true
    })
  })
}

function type(count: number): void {
  shown.value = FULL.slice(0, count)
  if (count < FULL.length) {
    later(72, () => type(count + 1))
    return
  }
  later(460, () => compact())
}

function onEnter(): void {
  hovering.value = true
  clearTimers()
  blink.value = false
  if (reduce.value) {
    shown.value = FULL
    split.value = true
    gone.value = false
    return
  }
  const count = split.value && gone.value ? KEEP.length : shown.value.length
  writeFrom(Math.min(count, FULL.length))
}

function onLeave(): void {
  hovering.value = false
  clearTimers()
  if (reduce.value) {
    settleSo()
    blink.value = false
    return
  }
  const count = split.value && !gone.value ? FULL.length : shown.value.length
  eraseTo(Math.max(count, KEEP.length))
}

onMounted(() => {
  reduce.value = window.matchMedia('(prefers-reduced-motion: reduce)').matches
  if (reduce.value) {
    settleSo()
    blink.value = false
    return
  }
  type(0)
})

onBeforeUnmount(() => {
  clearTimers()
})
</script>

<template>
  <p
    class="so-mark"
    role="img"
    aria-label="SoCLI"
    @mouseenter="onEnter"
    @mouseleave="onLeave"
  >
    <span v-if="!split" class="so-run">{{ shown }}</span>
    <template v-else>
      <span class="so-keep">{{ KEEP }}</span>
      <span class="so-tail" :class="{ 'is-gone': gone }"
        ><span class="so-tail-text">{{ TAIL }}</span></span
      >
    </template>
    <span class="so-cursor" :class="{ 'is-blink': blink }" aria-hidden="true">_</span>
  </p>
</template>
