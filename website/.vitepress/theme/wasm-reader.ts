/**
The landing-page reader. A worker runs the socli wasm module. This file paints cells,
forwards keys, and reads the plain-text screen. If the module cannot load, the caller
keeps the scripted reader.
*/

const RECORD = 15
/** A grapheme also fills the next cell. The frame does not send that cell. */
const WIDE = 0x20
const SPLIT_AT = 124
/** Border, screen padding and 2px of slack, so `fit` stays on 124 columns. */
const CHROME = 12

const EXTENDED = 0xfe
const MOD_SHIFT = 0b0000_0001
const MOD_ALT = 0b0000_0010
const MOD_CTRL = 0b0000_0100
const MOD_META = 0b0001_0000

interface Picture {
  url: string
  col: number
  row: number
  cols: number
  rows: number
  avatar: boolean
  square: boolean
  png?: ArrayBuffer
}

export interface FrameReply {
  frame: Uint8Array
  text: string
  cursor: { col: number; row: number } | null
  clipboard: string
  open: string
  pictures: Picture[]
}

const CONFIG_KEY = 'socli.config'

export interface ReaderClientOptions {
  worker: Worker
  config?: string
  page?: string
  oauth?: { code: string; state: string } | null
  wasmUrl?: string
  relayUrl?: string | null
  onConfig?: (text: string) => void | Promise<void>
  onError?: (error: Error) => void
}

export class ReaderClient {
  private readonly worker: Worker
  private pending = 0
  private chain: Promise<void> = Promise.resolve()
  private stopped = false
  private failCurrent: ((err: Error) => void) | null = null
  private failure: Error | null = null
  private saves: Promise<void> = Promise.resolve()

  constructor(private readonly options: ReaderClientOptions) {
    this.worker = options.worker
    this.worker.addEventListener('error', (event) => {
      this.failure = new Error(event.message || 'reader failed')
      this.failCurrent?.(this.failure)
      this.worker.terminate()
      this.options.onError?.(this.failure)
    })
    this.worker.addEventListener('messageerror', () => {
      this.failure = new Error('reader message could not be decoded')
      this.failCurrent?.(this.failure)
      this.worker.terminate()
      this.options.onError?.(this.failure)
    })
  }

  get busy(): boolean {
    return this.pending > 0
  }

  boot(cols: number, rows: number, dark: boolean, ratio: number): Promise<FrameReply> {
    return this.call({
      type: 'boot',
      cols,
      rows,
      dark,
      ratio,
      config: this.options.config ?? storedConfig(),
      page: this.options.page ?? pageAddress(),
      oauth: this.options.oauth === undefined ? oauthFromLocation() : this.options.oauth,
      wasmUrl: this.options.wasmUrl,
      relayUrl: this.options.relayUrl,
    })
  }

  reopen(cols: number, rows: number, dark: boolean, ratio: number): Promise<FrameReply> {
    return this.call({ type: 'reopen', cols, rows, dark, ratio })
  }

  key(bytes: Uint8Array): Promise<FrameReply> {
    const copy = bytes.slice()
    return this.call({ type: 'key', bytes: copy.buffer }, [copy.buffer])
  }

  paste(text: string): Promise<FrameReply> {
    return this.call({ type: 'paste', text })
  }

  pointer(kind: number, col: number, row: number, button: number): Promise<FrameReply> {
    return this.call({ type: 'pointer', kind, col, row, button })
  }

  resize(cols: number, rows: number, ratio: number): Promise<FrameReply> {
    return this.call({ type: 'resize', cols, rows, ratio })
  }

  tick(): Promise<FrameReply> {
    return this.call({ type: 'tick' })
  }

  theme(dark: boolean): Promise<FrameReply> {
    return this.call({ type: 'theme', dark })
  }

  stop(): void {
    this.stopped = true
    this.failCurrent?.(new Error('reader stopped'))
    this.worker.terminate()
  }

  flushConfig(): Promise<void> {
    return this.saves
  }

  private call(message: object, transfer?: Transferable[]): Promise<FrameReply> {
    if (this.stopped) return Promise.reject(new Error('reader stopped'))
    if (this.failure) return Promise.reject(this.failure)
    if (this.pending >= 256) return Promise.reject(new Error('reader queue is full; wait for the current input'))
    this.pending += 1
    const job = this.chain.then(
      () =>
        new Promise<FrameReply>((resolve, reject) => {
          if (this.stopped || this.failure) {
            reject(this.failure ?? new Error('reader stopped'))
            return
          }
          const finish = (fn: () => void) => {
            this.worker.removeEventListener('message', onMessage)
            this.failCurrent = null
            fn()
          }
          const onMessage = (event: MessageEvent) => {
            const data = event.data as { type?: string; text?: string; message?: string }
            if (!data || typeof data.type !== 'string') return
            if (data.type === 'config') {
              if (typeof data.text === 'string') {
                const text = data.text
                this.saves = this.saves.catch(() => {}).then(() => this.options.onConfig ? this.options.onConfig(text) : persistConfig(text))
                void this.saves.catch((error) => {
                  this.options.onError?.(error instanceof Error ? error : new Error('could not save reader configuration'))
                })
              }
              return
            }
            if (data.type === 'frame') finish(() => resolve(asFrame(event.data)))
            else finish(() => reject(new Error(typeof data.message === 'string' ? data.message : 'reader failed')))
          }
          this.failCurrent = (err) => finish(() => reject(err))
          this.worker.addEventListener('message', onMessage)
          try {
            this.worker.postMessage(message, transfer ?? [])
          } catch (error) {
            finish(() => reject(error))
          }
        }),
    )
    this.chain = job.then(
      () => {},
      () => {},
    )
    return job.finally(() => {
      this.pending -= 1
    })
  }
}

function asFrame(data: {
  frame?: ArrayBuffer
  text?: string
  cursor?: { col?: number; row?: number } | null
  clipboard?: string
  open?: string
  pictures?: unknown
}): FrameReply {
  const cursor = data.cursor
  const point =
    cursor && typeof cursor.col === 'number' && typeof cursor.row === 'number'
      ? { col: cursor.col, row: cursor.row }
      : null
  return {
    frame: data.frame instanceof ArrayBuffer ? new Uint8Array(data.frame) : new Uint8Array(),
    text: typeof data.text === 'string' ? data.text : '',
    cursor: point,
    clipboard: typeof data.clipboard === 'string' ? data.clipboard : '',
    open: typeof data.open === 'string' ? data.open : '',
    pictures: asPictures(data.pictures),
  }
}

function asPictures(value: unknown): Picture[] {
  if (!Array.isArray(value)) return []
  const pictures: Picture[] = []
  for (const item of value) {
    if (!item || typeof item !== 'object') continue
    const pic = item as Record<string, unknown>
    if (typeof pic.url !== 'string') continue
    const col = num(pic.col)
    const row = num(pic.row)
    const cols = num(pic.cols)
    const rows = num(pic.rows)
    if (col === null || row === null || cols === null || rows === null) continue
    pictures.push({
      url: pic.url,
      col,
      row,
      cols,
      rows,
      avatar: pic.avatar === true,
      square: pic.square === true,
      png: pic.png instanceof ArrayBuffer ? pic.png : undefined,
    })
  }
  return pictures
}

function num(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

function storedConfig(): string {
  try {
    return localStorage.getItem(CONFIG_KEY) ?? ''
  } catch {
    return ''
  }
}

function persistConfig(text: string): void {
  try {
    localStorage.setItem(CONFIG_KEY, text)
  } catch {
    // The worker still holds the text for this visit.
  }
}

export function canonicalPageAddress(href: string, baseUrl?: string): string {
  const url = new URL(href)
  const path = url.pathname
  let base = !baseUrl || baseUrl === '/' ? '' : baseUrl
  if (base.endsWith('/')) base = base.slice(0, -1)
  if (base && !base.startsWith('/')) base = `/${base}`
  if (base === '/') base = ''
  let rest = path
  if (base) {
    if (path === base) rest = '/'
    else if (path.startsWith(`${base}/`)) rest = path.slice(base.length) || '/'
    else return url.origin + path
  }
  if (rest === '/' || rest === '/index.html') return `${url.origin}${base}/`
  if (rest === '/web' || rest === '/web/' || rest === '/web/index.html') return `${url.origin}${base}/web/`
  return url.origin + path
}

function pageAddress(): string {
  return canonicalPageAddress(location.href, import.meta.env.BASE_URL)
}

function oauthFromLocation(): { code: string; state: string } | null {
  const params = new URLSearchParams(location.search)
  const code = params.get('code')
  const state = params.get('state')
  if (!code || !state) return null
  return { code, state }
}

function stripOauthQuery(): void {
  const params = new URLSearchParams(location.search)
  if (!params.has('code') && !params.has('state')) return
  params.delete('code')
  params.delete('state')
  const rest = params.toString()
  history.replaceState(null, '', location.pathname + (rest ? `?${rest}` : '') + location.hash)
}

function isHttps(url: string): boolean {
  try {
    return new URL(url).protocol === 'https:'
  } catch {
    return false
  }
}

export function isAuthorize(url: string): boolean {
  try {
    const parsed = new URL(url)
    if (parsed.protocol !== 'https:') return false
    if (parsed.pathname === '/i/oauth2/authorize') {
      return parsed.hostname === 'x.com' || parsed.hostname === 'twitter.com'
    }
    return parsed.pathname === '/oauth/authorize'
  } catch {
    return false
  }
}

interface Cell {
  text: string
  fg: string
  bg: string
  mods: number
  width: number
}

class ScreenView {
  private cols = 0
  private rows = 0
  private cells: Cell[][] = []
  private rowEls: HTMLDivElement[] = []
  private cursor: { col: number; row: number } | null = null

  constructor(private readonly root: HTMLElement) {}

  reset(cols: number, rows: number): void {
    this.cols = cols
    this.rows = rows
    this.cells = Array.from({ length: rows }, () => Array.from({ length: cols }, blank))
    this.rowEls = []
    this.root.replaceChildren()
    for (let y = 0; y < rows; y++) {
      const row = document.createElement('div')
      row.className = 'wasm-row'
      this.root.append(row)
      this.rowEls.push(row)
    }
  }

  apply(frame: Uint8Array): void {
    if (frame.length === 0 || this.rows === 0) return
    const data = new DataView(frame.buffer, frame.byteOffset, frame.byteLength)
    const dirty = new Set<number>()
    let at = 0
    while (at + RECORD <= frame.length) {
      const col = data.getUint16(at, true)
      const row = data.getUint16(at + 2, true)
      const scalar = data.getUint32(at + 4, true)
      let fg = rgb(frame[at + 8], frame[at + 9], frame[at + 10])
      let bg = rgb(frame[at + 11], frame[at + 12], frame[at + 13])
      let mods = frame[at + 14]
      at += RECORD
      let text = scalar === 0 ? ' ' : String.fromCodePoint(scalar)
      if ((mods & 0x80) !== 0) {
        if (at >= frame.length) return
        const len = frame[at]
        at += 1
        if (at + len > frame.length) return
        text = new TextDecoder().decode(frame.subarray(at, at + len))
        at += len
        mods &= 0x7f
      }
      const wide = (mods & WIDE) !== 0
      mods &= ~WIDE
      if ((mods & 16) !== 0) {
        const swap = fg
        fg = bg
        bg = swap
        mods &= ~16
      }
      if (row >= this.rows || col >= this.cols) continue
      // The wide bit is the column count from Rust. displayWidth covers the same BMP emoji
      // when an older frame does not carry the bit.
      const width = wide || displayWidth(text) === 2 ? 2 : 1
      this.cells[row][col] = { text: text || ' ', fg, bg, mods, width }
      dirty.add(row)
    }
    for (const row of dirty) this.render(row)
  }

  setCursor(cursor: { col: number; row: number } | null): void {
    const prev = this.cursor
    this.cursor = cursor
    if (prev) this.render(prev.row)
    if (cursor && cursor.row !== prev?.row) this.render(cursor.row)
    else if (cursor && cursor.col !== prev?.col) this.render(cursor.row)
  }

  private render(y: number): void {
    const el = this.rowEls[y]
    const cells = this.cells[y]
    if (!el || !cells) return
    const frag = document.createDocumentFragment()
    let x = 0
    while (x < this.cols) {
      const cell = cells[x]
      let end = Math.min(this.cols, x + cell.width)
      const key = styleKey(cell)
      const cursorHere = this.cursor?.row === y && this.cursor.col === x
      const block = !cursorHere && cell.width === 1 ? blockKind(cell.text) : null
      const line = !cursorHere && !block && cell.width === 1 ? lineKind(cell.text) : null
      const icon = !block && !line && cell.width === 1 && isIcon(cell.text)
      if (!cursorHere && !block && !line && !icon) {
        while (end < this.cols) {
          if (this.cursor?.row === y && this.cursor.col === end) break
          const next = cells[end]
          if (styleKey(next) !== key || blockKind(next.text) || lineKind(next.text) || isIcon(next.text)) break
          end = Math.min(this.cols, end + next.width)
        }
      }
      if (block) {
        let run = x + 1
        // ▍ and ▎ are a fraction of one cell. A merged span would scale that fraction across the run.
        if (BLOCK_RUNS.has(block)) {
          while (run < this.cols) {
            if (this.cursor?.row === y && this.cursor.col === run) break
            const next = cells[run]
            if (next.width !== 1 || blockKind(next.text) !== block || styleKey(next) !== key) break
            run += 1
          }
        }
        frag.append(blockSpan(cell, block, run - x))
        x = run
        continue
      }
      if (line) {
        let run = x + 1
        // A run of ─ is one bar. Other marks stay one cell so a corner keeps its own arms.
        if (line === 'h') {
          while (run < this.cols) {
            if (this.cursor?.row === y && this.cursor.col === run) break
            const next = cells[run]
            if (next.width !== 1 || lineKind(next.text) !== 'h' || styleKey(next) !== key) break
            run += 1
          }
        }
        frag.append(lineSpan(cell, line, run - x))
        x = run
        continue
      }
      if (icon) {
        const span = document.createElement('span')
        span.className = cursorHere ? 'is-icon is-cursor' : 'is-icon'
        span.textContent = cell.text
        paintSpan(span, cell)
        frag.append(span)
        x += 1
        continue
      }
      const span = document.createElement('span')
      let text = ''
      for (let i = x; i < end; ) {
        text += cells[i].text
        i += cells[i].width || 1
      }
      span.textContent = text
      // The font's advance of an emoji is not one column. The width keeps the pane rule in one line.
      span.style.width = `${end - x}ch`
      paintSpan(span, cell)
      if (cursorHere) span.className = 'is-cursor'
      frag.append(span)
      x = end
    }
    el.replaceChildren(frag)
  }
}

export interface ReaderViewOptions extends ReaderClientOptions {
  embedded?: boolean
  dark?: () => boolean
  followSystemTheme?: () => boolean
  onOpenUrl?: (url: string) => void | Promise<void>
  onClipboard?: (text: string) => void | Promise<void>
  onFrame?: (frame: FrameReply) => void
}

export async function mountWasmReader(root: HTMLElement): Promise<() => void> {
  const base = import.meta.env.BASE_URL || '/'
  const prefix = base.endsWith('/') ? base : `${base}/`
  const reader = await mountReaderView(root, {
    worker: new Worker(new URL('./reader.worker.ts', import.meta.url), { type: 'module' }),
    relayUrl: `${prefix}api/relay/api.x.com`,
  })
  return () => reader.dispose()
}

export async function mountReaderView(root: HTMLElement, options: ReaderViewOptions) {
  const screen = root.querySelector<HTMLElement>('#screen')
  const grid = root.querySelector<HTMLElement>('#grid')
  const live = root.querySelector<HTMLElement>('#live')
  const term = root.querySelector<HTMLElement>('#term')
  if (!screen || !grid || !live || !term) {
    throw new Error('reader chrome is missing')
  }

  const client = new ReaderClient({ ...options, onError: fail })
  const dark = () => options.dark?.() ?? pageIsDark()
  const activeElement = () => (screen.getRootNode() as Document | ShadowRoot).activeElement
  const view = new ScreenView(grid)
  const pics = new PictureLayer(grid)
  let cols = 1
  let rows = 1
  let lastText = ''
  let disposed = false

  const stage = term.closest('.stage')
  let ch = 8
  let line = 17
  let ratio = 0.5

  function place(): { cols: number; rows: number } {
    const probe = metrics(term)
    ch = probe.ch
    line = probe.line
    const room = !options.embedded && splitRoom(term, ch)
    term.classList.toggle('can-split', room)
    if (stage instanceof HTMLElement) stage.classList.toggle('room', room)
    const fitted = fit(screen, term)
    ch = fitted.ch
    line = fitted.line
    return { cols: fitted.cols, rows: fitted.rows }
  }

  function apply(reply: FrameReply, announce: boolean): void {
    if (reply.frame.length > 0) view.apply(reply.frame)
    if (announce || reply.frame.length > 0) {
      if (reply.text !== lastText) {
        live.textContent = reply.text
        lastText = reply.text
      }
    }
    view.setCursor(reply.cursor)
    pics.apply(reply.pictures, ch, line)
    options.onFrame?.(reply)
    if (reply.clipboard) {
      if (options.onClipboard) void Promise.resolve(options.onClipboard(reply.clipboard)).catch(fail)
      else void navigator.clipboard?.writeText(reply.clipboard).catch(() => {})
    }
    if (reply.open) {
      const target = reply.open
      void client.flushConfig().then(() => {
        if (disposed || !isHttps(target)) return
        if (options.onOpenUrl) return options.onOpenUrl(target)
        if (isAuthorize(target)) location.assign(target)
        else window.open(target, '_blank', 'noopener,noreferrer')
      }).catch(fail)
    }
  }

  function fail(err: unknown): void {
    const message = err instanceof Error ? err.message : 'reader failed'
    if (message) live.textContent = message
    options.onError?.(err instanceof Error ? err : new Error(message))
  }

  function updateTheme(): void {
    void client.theme(dark()).then((reply) => {
      if (disposed) return
      apply(reply, true)
    }, fail)
  }

  function send(bytes: Uint8Array | null): void {
    if (!bytes || disposed) return
    void client.key(bytes).then((reply) => {
      if (!disposed) apply(reply, true)
    }, fail)
  }

  const returning = options.oauth === undefined ? oauthFromLocation() : options.oauth
  try {
    term.classList.add('is-wasm')
    const size = place()
    cols = size.cols
    rows = size.rows
    ratio = measuredRatio(ch, line)
    view.reset(cols, rows)
    apply(await client.boot(cols, rows, dark(), ratio), true)
    if (returning && !options.embedded) stripOauthQuery()
  } catch (err) {
    if (returning && !options.embedded) stripOauthQuery()
    client.stop()
    term.classList.remove('is-wasm', 'can-split')
    if (stage instanceof HTMLElement) stage.classList.remove('room')
    throw err
  }

  const controller = new AbortController()
  const { signal } = controller

  const onKey = (event: KeyboardEvent) => {
    if (event.isComposing) return
    if (browserOwns(event)) return
    const bytes = encodeKey(event)
    if (!bytes) return
    event.preventDefault()
    send(bytes)
  }
  screen.addEventListener('keydown', onKey, { signal })
  // The homepage card is not focused until a click, so an arrow would scroll the page.
  if (stage instanceof HTMLElement) {
    window.addEventListener(
      'keydown',
      (event) => {
        if (!term.matches(':hover')) return
        if (activeElement() instanceof Node && screen.contains(activeElement())) return
        if (keyGoesToPage(event)) return
        onKey(event)
        if (event.defaultPrevented) event.stopPropagation()
      },
      { signal, capture: true },
    )
  }

  screen.addEventListener(
    'paste',
    (event) => {
      const text = event.clipboardData?.getData('text/plain') ?? ''
      if (!text || disposed) return
      event.preventDefault()
      void client.paste(text).then((reply) => {
        if (!disposed) apply(reply, true)
      }, fail)
    },
    { signal },
  )

  const pointer = (kind: number, event: MouseEvent) => {
    if (disposed) return
    const at = cellAt(grid, event, ch, line)
    const col = Math.min(cols - 1, at.col)
    const row = Math.min(rows - 1, at.row)
    const button = event.button === 2 ? 1 : event.button === 1 ? 2 : 0
    void client.pointer(kind, col, row, button).then((reply) => {
      if (!disposed) apply(reply, true)
    }, fail)
  }
  let touch: { x: number; y: number; id: number } | null = null
  let swallowMouse = false
  let swallowTimer = 0
  const swallowFollowingClick = () => {
    swallowMouse = true
    window.clearTimeout(swallowTimer)
    swallowTimer = window.setTimeout(() => {
      swallowMouse = false
    }, 500)
  }
  grid.addEventListener('mousedown', (event) => {
    if (swallowMouse) {
      event.preventDefault()
      event.stopPropagation()
      return
    }
    screen.focus()
    pointer(0, event)
  }, { signal })
  window.addEventListener('mouseup', (event) => {
    if (swallowMouse) return
    if (activeElement() !== screen) return
    pointer(1, event)
  }, { signal })
  let hover = ''
  grid.addEventListener(
    'mousemove',
    (event) => {
      if (disposed || client.busy) return
      const at = cellAt(grid, event, ch, line)
      const col = Math.min(cols - 1, at.col)
      const row = Math.min(rows - 1, at.row)
      const id = `${col},${row}`
      if (id === hover) return
      hover = id
      void client.pointer(2, col, row, 0).then((reply) => {
        if (!disposed) apply(reply, false)
      }, () => {})
    },
    { signal },
  )
  grid.addEventListener('contextmenu', (event) => event.preventDefault(), { signal })
  // The wheel stays with the page. One finger swipe steps one post.
  screen.addEventListener(
    'touchstart',
    (event) => {
      if (event.touches.length !== 1) {
        touch = null
        return
      }
      const point = event.touches[0]
      touch = { x: point.clientX, y: point.clientY, id: point.identifier }
    },
    { signal, passive: true },
  )
  screen.addEventListener(
    'touchmove',
    (event) => {
      if (!touch || event.touches.length !== 1) return
      const point = event.touches[0]
      if (point.identifier !== touch.id) return
      const dx = point.clientX - touch.x
      const dy = point.clientY - touch.y
      if (Math.abs(dy) < 16 || Math.abs(dy) <= Math.abs(dx)) return
      event.preventDefault()
    },
    { signal, passive: false },
  )
  screen.addEventListener(
    'touchend',
    (event) => {
      const start = touch
      touch = null
      if (!start || disposed) return
      const point = [...event.changedTouches].find((item) => item.identifier === start.id)
      if (!point) return
      const dx = point.clientX - start.x
      const dy = point.clientY - start.y
      if (Math.abs(dy) < 48 || Math.abs(dy) <= Math.abs(dx)) return
      swallowFollowingClick()
      // A finger moving up is the next post. One gesture, one step.
      send(Uint8Array.of(dy < 0 ? 0x81 : 0x80))
    },
    { signal },
  )
  screen.addEventListener('touchcancel', () => {
    touch = null
  }, { signal })

  screen.addEventListener('focus', () => term.classList.add('is-live'), { signal })
  screen.addEventListener('blur', () => term.classList.remove('is-live'), { signal })

  root.querySelectorAll<HTMLButtonElement>('.touch button').forEach((button) => {
    button.addEventListener(
      'click',
      () => {
        screen.focus()
        send(encodeNamed(button.dataset.key ?? ''))
      },
      { signal },
    )
  })

  let resizeFrame = 0
  const refit = () => {
    cancelAnimationFrame(resizeFrame)
    resizeFrame = requestAnimationFrame(() => {
      if (disposed) return
      const next = place()
      const nextRatio = measuredRatio(ch, line)
      if (next.cols === cols && next.rows === rows && Math.abs(nextRatio - ratio) < 0.005) return
      cols = next.cols
      rows = next.rows
      ratio = nextRatio
      view.reset(cols, rows)
      void client.resize(cols, rows, ratio).then((reply) => {
        if (disposed) return
        apply(reply, true)
      }, fail)
    })
  }
  const observer = new ResizeObserver(refit)
  observer.observe(screen)
  window.addEventListener('resize', refit, { signal })

  const themeObserver = new MutationObserver(() => {
    if (disposed) return
    updateTheme()
  })
  if (!options.embedded) themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })
  const scheme = window.matchMedia('(prefers-color-scheme: light)')
  const onScheme = () => {
    if (options.embedded ? options.followSystemTheme?.() !== false : (document.documentElement.dataset.theme ?? 'auto') === 'auto') updateTheme()
  }
  scheme.addEventListener('change', onScheme)

  const timer = window.setInterval(() => {
    if (disposed || client.busy) return
    void client.tick().then((reply) => {
      if (!disposed) apply(reply, false)
    }, () => {})
  }, 33)
  void document.fonts?.ready.then(() => {
    if (disposed) return
    const next = place()
    const nextRatio = measuredRatio(ch, line)
    if (next.cols === cols && next.rows === rows && Math.abs(nextRatio - ratio) < 0.005) return
    cols = next.cols
    rows = next.rows
    ratio = nextRatio
    view.reset(cols, rows)
    void client.resize(cols, rows, ratio).then((reply) => {
      if (disposed) return
      apply(reply, true)
    }, fail)
  })

  const dispose = () => {
    if (disposed) return
    disposed = true
    controller.abort()
    observer.disconnect()
    themeObserver.disconnect()
    scheme.removeEventListener('change', onScheme)
    window.clearInterval(timer)
    window.clearTimeout(swallowTimer)
    cancelAnimationFrame(resizeFrame)
    pics.clear()
    client.stop()
    term.classList.remove('is-wasm', 'can-split', 'is-live')
    if (stage instanceof HTMLElement) stage.classList.remove('room')
  }

  const update = async (job: () => Promise<FrameReply>) => {
    if (disposed) throw new Error('reader stopped')
    const reply = await job()
    if (!disposed) apply(reply, true)
    return reply
  }

  return {
    get text() { return lastText },
    focus: () => screen.focus(),
    key: (name: string) => {
      const bytes = encodeKey({ key: name, shiftKey: false, altKey: false, ctrlKey: false, metaKey: false } as KeyboardEvent)
      if (!bytes) return Promise.reject(new Error(`unsupported reader key: ${name}`))
      return update(() => client.key(bytes))
    },
    write: (bytes: Uint8Array) => update(() => client.key(bytes)),
    paste: (text: string) => update(() => client.paste(text)),
    flushConfig: () => client.flushConfig(),
    resize: refit,
    refreshTheme: () => {
      if (disposed) return Promise.reject(new Error('reader stopped'))
      return update(() => client.theme(dark()))
    },
    dispose,
  }
}

function pageIsDark(): boolean {
  const theme = document.documentElement.dataset.theme ?? 'auto'
  if (theme === 'dark') return true
  if (theme === 'light') return false
  return !window.matchMedia('(prefers-color-scheme: light)').matches
}

function measuredRatio(ch: number, line: number): number {
  if (!(ch > 0) || !(line > 0)) return 0.5
  const ratio = ch / line
  return Number.isFinite(ratio) && ratio > 0.2 && ratio < 2 ? ratio : 0.5
}

function metrics(term: HTMLElement): { ch: number; line: number } {
  const style = getComputedStyle(term)
  const probe = document.createElement('span')
  probe.textContent = '0'.repeat(64)
  probe.style.position = 'absolute'
  probe.style.visibility = 'hidden'
  probe.style.whiteSpace = 'pre'
  probe.style.fontFamily = style.fontFamily
  probe.style.fontSize = style.fontSize
  probe.style.fontWeight = style.fontWeight
  term.append(probe)
  const ch = probe.getBoundingClientRect().width / 64
  probe.remove()
  const line = Number.parseFloat(style.lineHeight)
  return { ch: ch > 0 ? ch : 8, line: Number.isFinite(line) ? line : 17 }
}

function fit(screen: HTMLElement, term: HTMLElement): { cols: number; rows: number; ch: number; line: number } {
  const { ch, line } = metrics(term)
  const style = getComputedStyle(screen)
  const padX = Number.parseFloat(style.paddingLeft) + Number.parseFloat(style.paddingRight)
  const padY = Number.parseFloat(style.paddingTop) + Number.parseFloat(style.paddingBottom)
  const width = Math.max(0, screen.clientWidth - padX)
  const height = Math.max(0, screen.clientHeight - padY)
  return {
    cols: Math.max(1, Math.floor(width / ch + 0.02)),
    rows: Math.max(1, Math.floor(height / line + 0.02)),
    ch,
    line,
  }
}

function splitRoom(term: HTMLElement, ch: number): boolean {
  const want = ch * SPLIT_AT + CHROME
  const main = term.closest('main') ?? document.body
  const mainStyle = getComputedStyle(main)
  const pad = Number.parseFloat(mainStyle.paddingLeft) + Number.parseFloat(mainStyle.paddingRight)
  const viewport = document.documentElement.clientWidth - pad
  const stage = term.closest('.stage')
  if (!(stage instanceof HTMLElement)) return viewport >= want
  const copy = stage.querySelector('.stage-copy')
  const stacked = getComputedStyle(stage).display === 'block'
  if (stacked || !(copy instanceof HTMLElement)) return viewport >= want
  const gap = Number.parseFloat(getComputedStyle(stage).columnGap) || 0
  return viewport >= copy.getBoundingClientRect().width + gap + want
}

function cellAt(grid: HTMLElement, event: MouseEvent, ch: number, line: number): { col: number; row: number } {
  const rect = grid.getBoundingClientRect()
  const col = Math.floor((event.clientX - rect.left + grid.scrollLeft) / Math.max(1, ch))
  const row = Math.floor((event.clientY - rect.top + grid.scrollTop) / Math.max(1, line))
  return { col: Math.max(0, col), row: Math.max(0, row) }
}

function keyGoesToPage(event: KeyboardEvent): boolean {
  const target = event.target
  if (!(target instanceof HTMLElement)) return false
  if (target.isContentEditable || target.closest('input, textarea, select, [contenteditable]')) return true
  if ((event.key === 'Enter' || event.key === ' ') && target.closest('a, button')) return true
  return false
}

function browserOwns(event: KeyboardEvent): boolean {
  const key = event.key.toLowerCase()
  if ((event.metaKey || event.ctrlKey) && (key === 'c' || key === 'v' || key === 'x')) return true
  if (event.metaKey && key !== 'r') return true
  return false
}

function encodeNamed(name: string): Uint8Array | null {
  if (name === 'j' || name === 'k') return Uint8Array.of(name.charCodeAt(0))
  if (name === 'Enter') return Uint8Array.of(0x0d)
  if (name === 'Escape') return Uint8Array.of(0x1b)
  return null
}

function encodeKey(event: KeyboardEvent): Uint8Array | null {
  if (event.key === 'Shift' || event.key === 'Control' || event.key === 'Alt' || event.key === 'Meta') return null
  const nav: Record<string, number> = {
    ArrowUp: 0,
    ArrowDown: 1,
    ArrowLeft: 2,
    ArrowRight: 3,
    Home: 4,
    End: 5,
    PageUp: 6,
    PageDown: 7,
    Delete: 8,
    F6: 10,
  }
  const id = nav[event.key]
  if (id !== undefined) {
    if (event.shiftKey || (event.altKey && event.ctrlKey) || event.metaKey) return null
    if (event.altKey) return Uint8Array.of(0x90 + id)
    if (event.ctrlKey) return Uint8Array.of(0xa0 + id)
    return Uint8Array.of(0x80 + id)
  }
  if (event.key === 'Tab' && !event.ctrlKey && !event.altKey && !event.metaKey) {
    return Uint8Array.of(event.shiftKey ? 0x80 + 9 : 0x09)
  }
  if (event.key === 'Enter' && !event.ctrlKey && !event.altKey && !event.metaKey && !event.shiftKey) {
    return Uint8Array.of(0x0d)
  }
  if (event.key === 'Escape') return Uint8Array.of(0x1b)
  if (event.key === 'Backspace' && !event.ctrlKey && !event.altKey && !event.metaKey) return Uint8Array.of(0x7f)
  if (event.key.length !== 1 && event.key.codePointAt(0) === undefined) return null
  if ([...event.key].length !== 1) return null
  const ch = event.key
  const letter = /^[a-z]$/i.test(ch)
  const index = letter ? ch.toLowerCase().charCodeAt(0) - 97 : -1
  if (event.altKey && letter && !event.ctrlKey && !event.metaKey) return Uint8Array.of(0xb0 + index)
  if (event.metaKey && letter && !event.ctrlKey && !event.altKey) return Uint8Array.of(0xe4 + index)
  if (event.ctrlKey && letter && !event.altKey && !event.metaKey && !event.shiftKey) {
    return Uint8Array.of(ch.toLowerCase().charCodeAt(0) - 97 + 1)
  }
  if (!event.ctrlKey && !event.metaKey && !(event.altKey && letter)) {
    const code = ch.codePointAt(0) ?? 0
    if (code >= 0x20 && code <= 0x7e) return Uint8Array.of(code)
    return extended(event.shiftKey ? MOD_SHIFT : 0, ch)
  }
  let bits = 0
  if (event.shiftKey) bits |= MOD_SHIFT
  if (event.altKey) bits |= MOD_ALT
  if (event.ctrlKey) bits |= MOD_CTRL
  if (event.metaKey) bits |= MOD_META
  return extended(bits, ch)
}

function extended(bits: number, ch: string): Uint8Array {
  const body = new TextEncoder().encode(ch)
  const out = new Uint8Array(2 + body.length)
  out[0] = EXTENDED
  out[1] = bits
  out.set(body, 2)
  return out
}

const BLOCKS: Record<string, string> = {
  '\u2580': 'upper',
  '\u2584': 'lower',
  '\u2588': 'full',
  '\u258c': 'left',
  '\u258d': 'three',
  '\u258e': 'quarter',
  '\u2590': 'right',
}

/** Half-blocks tile into pictures. The selection bar and the minimal rail stay one cell wide. */
const BLOCK_RUNS = new Set(['upper', 'lower', 'full', 'left', 'right'])

function blockKind(text: string): string | null {
  return BLOCKS[text] ?? null
}

/**
 * Nerd Font icons shipped in the page font. Each glyph fills one em, and a column is narrower,
 * so the mark has to be its own cell or the next column covers the right side.
 */
const ICONS = new Set([
  0xf0065, 0xf009c, 0xf00c0, 0xf00c3, 0xf0128, 0xf012c, 0xf0156, 0xf0182, 0xf01d8, 0xf02d1, 0xf02d5,
  0xf0339, 0xf0456, 0xf0791, 0xf0976,
])

function isIcon(text: string): boolean {
  // These scalars sit in plane 15, so one icon is two UTF-16 units.
  const code = text.codePointAt(0) ?? 0
  if (text.length !== (code > 0xffff ? 2 : 1)) return false
  return ICONS.has(code) || (code >= 0xf0a9e && code <= 0xf0aa5)
}

/** Box-drawing marks. The font glyph stops short of the cell edge, so the avatar box shows a seam. */
const LINES: Record<string, string> = {
  '\u2500': 'h',
  '\u2501': 'h',
  '\u2502': 'v',
  '\u2503': 'v',
  '\u256d': 'tl',
  '\u256e': 'tr',
  '\u2570': 'bl',
  '\u256f': 'br',
  '\u250c': 'stl',
  '\u2510': 'str',
  '\u2514': 'sbl',
  '\u2518': 'sbr',
  '\u251c': 'vr',
  '\u2524': 'vl',
  '\u252c': 'hd',
  '\u2534': 'hu',
  '\u253c': 'cr',
  '\u254e': 'vdash',
  '\u2506': 'vdash',
  '\u254c': 'hdash',
  '\u2504': 'hdash',
}

const LINE_ARMS = new Set([
  'vr',
  'vl',
  'hd',
  'hu',
  'cr',
  'vdash',
  'hdash',
  'tl',
  'tr',
  'bl',
  'br',
  'stl',
  'str',
  'sbl',
  'sbr',
])
const LINE_ROUND = new Set(['tl', 'tr', 'bl', 'br'])

function lineKind(text: string): string | null {
  return LINES[text] ?? null
}

function paintSpan(span: HTMLElement, cell: Cell): void {
  if (cell.fg) span.style.color = cell.fg
  if (cell.bg) span.style.backgroundColor = cell.bg
  if ((cell.mods & 1) !== 0) span.style.fontWeight = '700'
  if ((cell.mods & 2) !== 0) span.style.opacity = '0.72'
  if ((cell.mods & 4) !== 0) span.style.fontStyle = 'italic'
  if ((cell.mods & 8) !== 0) span.style.textDecoration = 'underline'
}

/** Half-blocks and full blocks fill the cell. The font glyph leaves a hairline inside the line box. */
function blockSpan(cell: Cell, kind: string, cols: number): HTMLSpanElement {
  const span = document.createElement('span')
  span.className = `wasm-block ${kind}`
  span.style.width = `${cols}ch`
  paintSpan(span, cell)
  span.append(document.createElement('i'))
  return span
}

/**
 * Straight arms are one rectangle, overlapped into the next cell.
 * A rounded corner's curve sits under those arms so the join cannot step off the stroke.
 */
function lineSpan(cell: Cell, kind: string, cols: number): HTMLSpanElement {
  const span = document.createElement('span')
  span.className = `wasm-draw ${kind}`
  span.style.width = `${cols}ch`
  paintSpan(span, cell)
  if (LINE_ROUND.has(kind)) {
    const curve = document.createElement('span')
    curve.className = 'curve'
    span.append(curve)
  }
  span.append(document.createElement('i'))
  if (LINE_ARMS.has(kind)) span.append(document.createElement('b'))
  return span
}

class PictureLayer {
  private readonly src = new Map<string, string>()
  private readonly nodes = new Map<string, HTMLImageElement>()

  constructor(private readonly grid: HTMLElement) {}

  apply(list: Picture[], ch: number, line: number): void {
    const layer = this.layer()
    const keep = new Set<string>()
    for (const pic of list) {
      if (pic.png && pic.png.byteLength > 0) {
        const prev = this.src.get(pic.url)
        if (prev?.startsWith('blob:')) URL.revokeObjectURL(prev)
        this.src.set(pic.url, URL.createObjectURL(new Blob([pic.png], { type: 'image/png' })))
      } else if (!this.src.has(pic.url) && /^https?:\/\//i.test(pic.url)) {
        this.src.set(pic.url, pic.url)
      }
      const src = this.src.get(pic.url)
      if (!src) continue
      const key = `${pic.url}@${pic.col},${pic.row},${pic.cols},${pic.rows},${pic.avatar ? 1 : 0}`
      keep.add(key)
      let img = this.nodes.get(key)
      if (!img) {
        const created = document.createElement('img')
        created.alt = ''
        created.draggable = false
        created.referrerPolicy = 'no-referrer'
        created.addEventListener('error', () => {
          created.style.visibility = 'hidden'
        })
        this.nodes.set(key, created)
        img = created
      }
      if (img.getAttribute('src') !== src) {
        img.style.visibility = ''
        img.src = src
      }
      img.className = pic.avatar ? (pic.square ? 'is-square' : 'is-avatar') : ''
      const boxW = pic.cols * ch
      const boxH = pic.rows * line
      if (pic.avatar) {
        const side = Math.min(boxW, boxH)
        img.style.left = `${pic.col * ch + (boxW - side) / 2}px`
        img.style.top = `${pic.row * line + (boxH - side) / 2}px`
        img.style.width = `${side}px`
        img.style.height = `${side}px`
      } else {
        img.style.left = `${pic.col * ch}px`
        img.style.top = `${pic.row * line}px`
        img.style.width = `${boxW}px`
        img.style.height = `${boxH}px`
      }
      if (img.parentElement !== layer) layer.append(img)
    }
    for (const [key, img] of this.nodes) {
      if (keep.has(key)) continue
      img.remove()
      this.nodes.delete(key)
    }
  }

  clear(): void {
    for (const url of this.src.values()) URL.revokeObjectURL(url)
    this.src.clear()
    this.nodes.clear()
    this.grid.querySelector(':scope > .wasm-pics')?.remove()
  }

  private layer(): HTMLElement {
    let layer = this.grid.querySelector<HTMLElement>(':scope > .wasm-pics')
    if (!layer) {
      layer = document.createElement('div')
      layer.className = 'wasm-pics'
      this.grid.append(layer)
    }
    return layer
  }
}

function blank(): Cell {
  return { text: ' ', fg: '', bg: '', mods: 0, width: 1 }
}

function styleKey(cell: Cell): string {
  return `${cell.fg}|${cell.bg}|${cell.mods}`
}

function rgb(red: number, green: number, blue: number): string {
  return `rgb(${red} ${green} ${blue})`
}

function displayWidth(text: string): number {
  if (!text || text === ' ') return 1
  for (const ch of text) {
    const code = ch.codePointAt(0) ?? 0
    if (code >= 0x1100 && isWide(code)) return 2
  }
  return 1
}

function isWide(code: number): boolean {
  return (
    (code >= 0x1100 && code <= 0x115f) ||
    code === 0x2329 ||
    code === 0x232a ||
    (code >= 0x2e80 && code <= 0xa4cf && code !== 0x303f) ||
    (code >= 0xac00 && code <= 0xd7a3) ||
    (code >= 0xf900 && code <= 0xfaff) ||
    (code >= 0xfe10 && code <= 0xfe19) ||
    (code >= 0xfe30 && code <= 0xfe6f) ||
    (code >= 0xff00 && code <= 0xff60) ||
    (code >= 0xffe0 && code <= 0xffe6) ||
    (code >= 0x1f300 && code <= 0x1faff) ||
    (code >= 0x20000 && code <= 0x3fffd) ||
    emojiPresentation(code)
  )
}

/** BMP emoji that unicode-width treats as two columns and that sit outside the ranges above. */
function emojiPresentation(code: number): boolean {
  return (
    code === 0x231a ||
    code === 0x231b ||
    (code >= 0x23e9 && code <= 0x23ec) ||
    code === 0x23f0 ||
    code === 0x23f3 ||
    (code >= 0x25fd && code <= 0x25fe) ||
    (code >= 0x2614 && code <= 0x2615) ||
    (code >= 0x2648 && code <= 0x2653) ||
    code === 0x267f ||
    code === 0x2693 ||
    code === 0x26a1 ||
    (code >= 0x26aa && code <= 0x26ab) ||
    (code >= 0x26bd && code <= 0x26be) ||
    (code >= 0x26c4 && code <= 0x26c5) ||
    code === 0x26ce ||
    code === 0x26d4 ||
    code === 0x26ea ||
    (code >= 0x26f2 && code <= 0x26f3) ||
    code === 0x26f5 ||
    code === 0x26fa ||
    code === 0x26fd ||
    code === 0x2705 ||
    (code >= 0x270a && code <= 0x270b) ||
    code === 0x2728 ||
    code === 0x274c ||
    code === 0x274e ||
    (code >= 0x2753 && code <= 0x2755) ||
    code === 0x2757 ||
    (code >= 0x2795 && code <= 0x2797) ||
    code === 0x27b0 ||
    code === 0x27bf ||
    (code >= 0x2b1b && code <= 0x2b1c) ||
    code === 0x2b50 ||
    code === 0x2b55
  )
}
