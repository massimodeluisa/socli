import { expect, test } from 'bun:test'
import { resolve } from 'node:path'

const path = resolve(import.meta.dir, '../target/wasm32-unknown-unknown/release/socli.wasm')
const module = new WebAssembly.Module(await Bun.file(path).arrayBuffer())

function readerModule() {
  let httpCalls = 0
  const instance = new WebAssembly.Instance(module, {
    env: {
      socli_millis: () => Date.UTC(2026, 9, 28, 12),
      socli_mono: () => performance.now(),
      socli_http: () => { httpCalls++; throw new Error('offline fixtures must not make HTTP requests') },
      socli_page: () => 0,
      socli_config_read: () => 0,
      socli_config_write: () => { throw new Error('demo must not save configuration') },
      socli_random: () => { throw new Error('demo must not start authentication') },
    },
  })
  const wasm = instance.exports as {
    memory: WebAssembly.Memory
    socli_new(cols: number, rows: number, dark: number, millis: number): number
    socli_free(ptr: number): void
    socli_alloc(length: number): number
    socli_free_bytes(ptr: number, length: number): void
    socli_text(ptr: number, length: number): number
    socli_key(ptr: number, bytes: number, length: number): void
    socli_theme(ptr: number, dark: number): void
    socli_resize(ptr: number, cols: number, rows: number, ratio: number): void
  }
  const length = wasm.socli_alloc(4)
  return {
    wasm,
    text(ptr: number) {
      const bytes = wasm.socli_text(ptr, length)
      const size = new DataView(wasm.memory.buffer).getUint32(length, true)
      return new TextDecoder().decode(new Uint8Array(wasm.memory.buffer, bytes, size))
    },
    key(ptr: number, byte: number) {
      const input = wasm.socli_alloc(1)
      try {
        new Uint8Array(wasm.memory.buffer)[input] = byte
        wasm.socli_key(ptr, input, 1)
      } finally {
        wasm.socli_free_bytes(input, 1)
      }
    },
    close() {
      wasm.socli_free_bytes(length, 4)
      expect(httpCalls).toBe(0)
    },
  }
}

test('compiled WASM keeps readers isolated through demo input, theme and resize', () => {
  const host = readerModule()
  const first = host.wasm.socli_new(104, 40, 1, Date.now())
  const second = host.wasm.socli_new(104, 40, 0, Date.now())
  expect(first).not.toBe(0)
  expect(second).not.toBe(0)
  try {
    expect(host.text(first)).toContain('SoCLI > login')
    expect(host.text(second)).toContain('SoCLI > login')
    host.key(first, 0x80)
    host.key(first, 0x0d)
    expect(host.text(first)).toContain('ok devs, assemble.')
    expect(host.text(first)).not.toContain('SoCLI > login')
    expect(host.text(second)).toContain('SoCLI > login')
    host.wasm.socli_theme(first, 0)
    expect(host.text(first)).toContain('ok devs, assemble.')
    host.wasm.socli_resize(first, 40, 16, 0.5)
    expect(host.text(first).split('\n').length).toBeLessThanOrEqual(17)
    expect(host.text(second)).toContain('SoCLI > login')
  } finally {
    host.wasm.socli_free(first)
    host.wasm.socli_free(second)
    host.close()
  }
})
