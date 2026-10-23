import { ReaderClient, mountReaderView } from '../../website/.vitepress/theme/wasm-reader.ts'

export const wasmUrl = new URL('./socli.wasm', import.meta.url)
export const workerUrl = new URL('./reader.worker.js', import.meta.url)
export const styleUrl = new URL('./reader.css', import.meta.url)

const fontAssets = [
  ['Geist Mono', new URL('./fonts/GeistMono-Regular.otf', import.meta.url), { weight: '400' }],
  ['Geist Mono', new URL('./fonts/GeistMono-Bold.otf', import.meta.url), { weight: '700' }],
  ['Symbols Nerd Font Mono', new URL('./fonts/SymbolsNerdFontMono-Regular.ttf', import.meta.url), { weight: '400' }],
]
let fontsReady

export async function instantiate(imports, url = wasmUrl) {
  const response = await fetch(url)
  if (!response.ok) throw new Error(`could not load socli wasm: HTTP ${response.status}`)
  return WebAssembly.instantiate(await response.arrayBuffer(), imports)
}

async function clientOptions(options) {
  const config = options.configStore ? await options.configStore.load() : options.config ?? ''
  if (typeof config !== 'string') throw new Error('soclib configuration must be a TOML string')
  const page = new URL(options.pageUrl ?? location.href)
  page.search = ''
  page.hash = ''
  return {
    worker: options.workerUrl
      ? new Worker(options.workerUrl, { type: 'module' })
      : new Worker(new URL('./reader.worker.js', import.meta.url), { type: 'module' }),
    config,
    page: page.href,
    oauth: options.oauth ?? null,
    wasmUrl: new URL(options.wasmUrl ?? wasmUrl, location.href).href,
    relayUrl: options.relayUrl == null ? null : new URL(options.relayUrl, location.href).href,
    onConfig: (text) => options.configStore?.save(text),
    onError: options.onError,
  }
}

function dimensions(cols, rows) {
  if (!Number.isInteger(cols) || !Number.isInteger(rows) || cols < 1 || rows < 1 || cols > 1000 || rows > 1000) {
    throw new Error('soclib dimensions must be integers between 1 and 1000')
  }
}

export async function createReader(options = {}) {
  const cols = options.cols ?? 80
  const rows = options.rows ?? 24
  dimensions(cols, rows)
  const client = new ReaderClient(await clientOptions(options))
  let frame
  const update = async (job) => {
    frame = await job()
    options.onFrame?.(frame)
    return frame
  }
  try {
    await update(() => client.boot(cols, rows, options.dark ?? true, 0.5))
  } catch (error) {
    client.stop()
    throw error
  }
  return {
    get frame() { return frame },
    get text() { return frame.text },
    write: (bytes) => update(() => client.key(bytes)),
    paste: (text) => update(() => client.paste(text)),
    pointer: (kind, col, row, button = 0) => update(() => client.pointer(kind, col, row, button)),
    resize: (cols, rows, ratio = 0.5) => {
      dimensions(cols, rows)
      return update(() => client.resize(cols, rows, ratio))
    },
    tick: () => update(() => client.tick()),
    flushConfig: () => client.flushConfig(),
    dispose: () => client.stop(),
  }
}

async function loadFonts() {
  if (!fontsReady) {
    fontsReady = Promise.all(fontAssets.map(async ([name, url, descriptors]) => {
      const font = await new FontFace(name, `url("${url.href}")`, descriptors).load()
      document.fonts.add(font)
    })).catch((error) => {
      fontsReady = undefined
      throw error
    })
  }
  await fontsReady
}

export async function mountReader(container, options = {}) {
  if (!(container instanceof HTMLElement)) throw new Error('soclib needs an HTML container')
  const themeValue = (value) => {
    if (!['auto', 'dark', 'light'].includes(value)) throw new Error('soclib theme must be auto, dark or light')
    return value
  }
  let theme = themeValue(options.theme ?? 'auto')
  const response = await fetch(options.styleUrl ?? styleUrl)
  if (!response.ok) throw new Error(`could not load soclib styles: HTTP ${response.status}`)
  const css = await response.text()
  if (options.fonts !== false) await loadFonts()
  const host = document.createElement('div')
  host.style.cssText = 'display:block;height:100%;min-height:240px'
  const shadow = host.attachShadow({ mode: 'open' })
  const style = document.createElement('style')
  style.textContent = css
  const root = document.createElement('div')
  root.className = 'soclib-reader'
  root.innerHTML = '<div id="term" class="term"><div id="screen" class="screen" tabindex="0" role="application" aria-label="socli reader"><div id="grid" class="wasm-grid"></div></div></div><p id="live" class="soclib-live" aria-live="polite"></p>'
  shadow.append(style, root)
  container.append(host)
  const dark = () => {
    const value = theme === 'dark' || (theme === 'auto' && !matchMedia('(prefers-color-scheme: light)').matches)
    root.dataset.theme = value ? 'dark' : 'light'
    return value
  }
  dark()
  try {
    const reader = await mountReaderView(root, { ...options, ...await clientOptions(options), embedded: true, dark, followSystemTheme: () => theme === 'auto' })
    if (options.autofocus) reader.focus()
    return {
      get text() { return reader.text },
      key: reader.key,
      write: reader.write,
      paste: reader.paste,
      flushConfig: reader.flushConfig,
      resize: reader.resize,
      focus: reader.focus,
      setTheme: (value) => {
        theme = themeValue(value)
        return reader.refreshTheme()
      },
      dispose: () => {
        reader.dispose()
        host.remove()
      },
    }
  } catch (error) {
    host.remove()
    throw error
  }
}
