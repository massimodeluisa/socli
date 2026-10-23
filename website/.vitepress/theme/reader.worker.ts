/**
The reader worker. It owns the wasm module so a network read can use a synchronous
XMLHttpRequest, which the main thread is not allowed to make. The page paints frames
and stores the config text. One wasm call runs at a time.
*/

const CONFIG_TOO_BIG = 0xffffffff
const MAX_RESPONSE = 8 * 1024 * 1024
const COOKIE_BLOCKED =
  'the browser blocked the Cookie header. X session cookies cannot be attached from this page. `socli login x` in a terminal still works.'

interface WasmExports {
  memory: WebAssembly.Memory
  socli_new(cols: number, rows: number, dark: number, millis: number): number
  socli_free(ptr: number): void
  socli_theme(ptr: number, dark: number): void
  socli_key(ptr: number, bytes: number, len: number): void
  socli_paste(ptr: number, bytes: number, len: number): void
  socli_pointer(ptr: number, kind: number, col: number, row: number, button: number): void
  socli_resize(ptr: number, cols: number, rows: number, ratio: number): void
  socli_tick(ptr: number): void
  socli_frame(ptr: number, outLen: number): number
  socli_text(ptr: number, outLen: number): number
  socli_take_clipboard(ptr: number, outLen: number): number
  socli_take_open(ptr: number, outLen: number): number
  socli_cursor(ptr: number): number
  socli_pictures(ptr: number, outLen: number): number
  socli_picture(ptr: number, url: number, len: number, outLen: number): number
  socli_alloc(len: number): number
  socli_free_bytes(ptr: number, len: number): void
  socli_finish_oauth(
    codePtr: number,
    codeLen: number,
    statePtr: number,
    stateLen: number,
    outPtr: number,
    outCap: number,
  ): number
  socli_notice(ptr: number, bytes: number, len: number): void
}

interface Stash {
  key: string
  bytes: Uint8Array
}

let wasm: WasmExports | null = null
let session: Session | null = null
let configText = ''
let pageUrl = ''
let moduleUrl: string | undefined
let relayBase: string | null | undefined
let stash: Stash | null = null
const sentPictures = new Set<string>()
const encoder = new TextEncoder()
const decoder = new TextDecoder()

class Session {
  private ptr: number

  constructor(exports: WasmExports, cols: number, rows: number, dark: boolean) {
    this.ptr = exports.socli_new(cols, rows, dark ? 1 : 0, Date.now())
    if (this.ptr === 0) throw new Error('socli_new returned null')
  }

  free(): void {
    if (!wasm || this.ptr === 0) return
    wasm.socli_free(this.ptr)
    this.ptr = 0
  }

  key(bytes: Uint8Array): void {
    this.withBytes(bytes, (ptr, len) => wasm?.socli_key(this.ptr, ptr, len))
  }

  paste(text: string): void {
    this.withBytes(encoder.encode(text), (ptr, len) => wasm?.socli_paste(this.ptr, ptr, len))
  }

  pointer(kind: number, col: number, row: number, button: number): void {
    wasm?.socli_pointer(this.ptr, kind, col, row, button)
  }

  resize(cols: number, rows: number, ratio: number): void {
    wasm?.socli_resize(this.ptr, cols, rows, ratio)
  }

  tick(): void {
    wasm?.socli_tick(this.ptr)
  }

  theme(dark: boolean): void {
    wasm?.socli_theme(this.ptr, dark ? 1 : 0)
  }

  frame(): Uint8Array {
    return this.read((out) => wasm?.socli_frame(this.ptr, out) ?? 0)
  }

  text(): string {
    return decoder.decode(this.read((out) => wasm?.socli_text(this.ptr, out) ?? 0))
  }

  cursor(): { col: number; row: number } | null {
    if (!wasm) return null
    const packed = wasm.socli_cursor(this.ptr) >>> 0
    if (packed === 0xffffffff) return null
    return { col: packed >>> 16, row: packed & 0xffff }
  }

  takeClipboard(): string {
    return decoder.decode(this.read((out) => wasm?.socli_take_clipboard(this.ptr, out) ?? 0))
  }

  takeOpen(): string {
    return decoder.decode(this.read((out) => wasm?.socli_take_open(this.ptr, out) ?? 0))
  }

  notice(text: string): void {
    if (!wasm || !text) return
    const bytes = encoder.encode(text)
    const ptr = wasm.socli_alloc(bytes.length)
    new Uint8Array(wasm.memory.buffer, ptr, bytes.length).set(bytes)
    try {
      wasm.socli_notice(this.ptr, ptr, bytes.length)
    } finally {
      wasm.socli_free_bytes(ptr, bytes.length)
    }
  }

  pictures(): Array<Record<string, unknown>> {
    const raw = decoder.decode(this.read((out) => wasm?.socli_pictures(this.ptr, out) ?? 0))
    if (!raw) return []
    let parsed: unknown
    try {
      parsed = JSON.parse(raw)
    } catch {
      return []
    }
    if (!Array.isArray(parsed)) return []
    const out: Array<Record<string, unknown>> = []
    for (const item of parsed) {
      if (!item || typeof item !== 'object') continue
      const place = item as Record<string, unknown>
      if (typeof place.url !== 'string') continue
      const row: Record<string, unknown> = { ...place }
      if (!sentPictures.has(place.url)) {
        // The page loads an http(s) picture from the URL. Only demo:// has PNG bytes.
        if (/^https?:\/\//i.test(place.url)) {
          sentPictures.add(place.url)
        } else {
          const png = this.picture(place.url)
          if (png.byteLength > 0) {
            sentPictures.add(place.url)
            row.png = png.buffer
          }
        }
      }
      out.push(row)
    }
    return out
  }

  private picture(url: string): Uint8Array {
    return this.withText(url, (ptr, len, out) => wasm?.socli_picture(this.ptr, ptr, len, out) ?? 0)
  }

  private withText(text: string, fn: (ptr: number, len: number, out: number) => number): Uint8Array {
    const data = encoder.encode(text)
    if (!wasm || data.length === 0) return new Uint8Array()
    const ptr = wasm.socli_alloc(data.length)
    new Uint8Array(wasm.memory.buffer, ptr, data.length).set(data)
    try {
      return this.read((out) => fn(ptr, data.length, out))
    } finally {
      wasm.socli_free_bytes(ptr, data.length)
    }
  }

  private withBytes(data: Uint8Array, fn: (ptr: number, len: number) => void): void {
    if (!wasm || data.length === 0) {
      fn(0, 0)
      return
    }
    const ptr = wasm.socli_alloc(data.length)
    new Uint8Array(wasm.memory.buffer, ptr, data.length).set(data)
    try {
      fn(ptr, data.length)
    } finally {
      wasm.socli_free_bytes(ptr, data.length)
    }
  }

  /** Copy bytes out before the next call reuses the session buffer. */
  private read(fn: (outPtr: number) => number): Uint8Array {
    if (!wasm) return new Uint8Array()
    const outPtr = wasm.socli_alloc(4)
    try {
      const ptr = fn(outPtr)
      const len = new DataView(wasm.memory.buffer).getUint32(outPtr, true)
      if (ptr === 0 || len === 0) return new Uint8Array()
      return new Uint8Array(wasm.memory.buffer, ptr, len).slice()
    } finally {
      wasm.socli_free_bytes(outPtr, 4)
    }
  }
}

self.onmessage = (event: MessageEvent) => {
  const data = event.data as { type?: string }
  if (data?.type === 'boot') {
    void boot(event.data)
    return
  }
  try {
    dispatch(event.data)
    reply()
  } catch (err) {
    fail(err)
  }
}

async function boot(data: {
  cols?: number
  rows?: number
  dark?: boolean
  ratio?: number
  config?: string
  page?: string
  wasmUrl?: string
  relayUrl?: string | null
  oauth?: { code?: string; state?: string }
}): Promise<void> {
  try {
    configText = typeof data.config === 'string' ? data.config : ''
    pageUrl = typeof data.page === 'string' ? data.page : ''
    moduleUrl = data.wasmUrl
    relayBase = data.relayUrl
    await ensureWasm()
    const code = data.oauth?.code
    const state = data.oauth?.state
    let notice = ''
    if (code && state) notice = finishOauth(code, state)
    open(data.cols ?? 1, data.rows ?? 1, data.dark === true, data.ratio ?? 0.5)
    if (notice) session?.notice(notice)
    reply()
  } catch (err) {
    fail(err)
  }
}

function dispatch(data: {
  type?: string
  bytes?: ArrayBuffer
  text?: string
  kind?: number
  col?: number
  row?: number
  button?: number
  cols?: number
  rows?: number
  ratio?: number
  dark?: boolean
}): void {
  if (!session) throw new Error('reader is not open')
  if (data.type === 'reopen') {
    open(data.cols ?? 1, data.rows ?? 1, data.dark === true, data.ratio ?? 0.5)
    return
  }
  if (data.type === 'key') {
    const bytes = data.bytes instanceof ArrayBuffer ? new Uint8Array(data.bytes) : new Uint8Array()
    session.key(bytes)
    return
  }
  if (data.type === 'paste') {
    session.paste(typeof data.text === 'string' ? data.text : '')
    return
  }
  if (data.type === 'pointer') {
    session.pointer(data.kind ?? 0, data.col ?? 0, data.row ?? 0, data.button ?? 0)
    return
  }
  if (data.type === 'resize') {
    session.resize(data.cols ?? 1, data.rows ?? 1, data.ratio ?? 0.5)
    return
  }
  if (data.type === 'tick') {
    session.tick()
    return
  }
  if (data.type === 'theme') {
    session.theme(data.dark === true)
    return
  }
  if (data.type === 'free') {
    session.free()
    session = null
  }
}

function open(cols: number, rows: number, dark: boolean, ratio: number): void {
  if (!wasm) throw new Error('wasm is not loaded')
  const previous = session
  session = null
  previous?.free()
  sentPictures.clear()
  session = new Session(wasm, cols, rows, dark)
  session.resize(cols, rows, ratio)
}

function reply(): void {
  if (!session) return
  const frame = session.frame()
  const pictures = session.pictures()
  const transfers: ArrayBuffer[] = [frame.buffer]
  for (const picture of pictures) {
    if (picture.png instanceof ArrayBuffer) transfers.push(picture.png)
  }
  const payload = {
    type: 'frame',
    frame: frame.buffer,
    text: session.text(),
    cursor: session.cursor(),
    clipboard: session.takeClipboard(),
    open: session.takeOpen(),
    pictures,
  }
  self.postMessage(payload, transfers)
}

function fail(err: unknown): void {
  const message = err instanceof Error && err.message ? err.message : 'reader failed'
  self.postMessage({ type: 'error', message })
}

async function ensureWasm(): Promise<void> {
  if (wasm) return
  const base = import.meta.env.BASE_URL || '/'
  const response = await fetch(moduleUrl ?? `${base}wasm/socli.wasm`)
  if (!response.ok) throw new Error(`wasm ${response.status}`)
  const bytes = await response.arrayBuffer()
  const { instance } = await WebAssembly.instantiate(bytes, {
    env: {
      socli_millis: () => Date.now(),
      socli_mono: () => performance.now(),
      socli_http: socliHttp,
      socli_config_read: socliConfigRead,
      socli_config_write: socliConfigWrite,
      socli_random: socliRandom,
      socli_page: socliPage,
    },
  })
  const exports = instance.exports as unknown as WasmExports
  if (!exports.memory || typeof exports.socli_new !== 'function') throw new Error('wasm exports are missing')
  wasm = exports
}

function memory(): WebAssembly.Memory {
  if (!wasm) throw new Error('wasm is not loaded')
  return wasm.memory
}

function readText(ptr: number, len: number): string {
  if (len === 0 || ptr === 0) return ''
  return decoder.decode(new Uint8Array(memory().buffer, ptr, len).slice())
}

function readBytes(ptr: number, len: number): Uint8Array {
  if (len === 0 || ptr === 0) return new Uint8Array()
  return new Uint8Array(memory().buffer, ptr, len).slice()
}

function socliConfigRead(ptr: number, cap: number): number {
  const bytes = encoder.encode(configText)
  if (bytes.length > cap) return CONFIG_TOO_BIG
  if (bytes.length === 0) return 0
  new Uint8Array(memory().buffer, ptr, bytes.length).set(bytes)
  return bytes.length
}

function socliConfigWrite(ptr: number, len: number): void {
  configText = readText(ptr, len)
  self.postMessage({ type: 'config', text: configText })
}

function socliPage(ptr: number, cap: number): number {
  const bytes = encoder.encode(pageUrl)
  if (bytes.length === 0 || bytes.length > cap) return 0
  new Uint8Array(memory().buffer, ptr, bytes.length).set(bytes)
  return bytes.length
}

function finishOauth(code: string, state: string): string {
  if (!wasm) return 'reader is not loaded'
  const codeBytes = encoder.encode(code)
  const stateBytes = encoder.encode(state)
  const outCap = 1024
  const codePtr = wasm.socli_alloc(codeBytes.length)
  const statePtr = wasm.socli_alloc(stateBytes.length)
  const outPtr = wasm.socli_alloc(outCap)
  new Uint8Array(wasm.memory.buffer, codePtr, codeBytes.length).set(codeBytes)
  new Uint8Array(wasm.memory.buffer, statePtr, stateBytes.length).set(stateBytes)
  try {
    const n = wasm.socli_finish_oauth(
      codePtr,
      codeBytes.length,
      statePtr,
      stateBytes.length,
      outPtr,
      outCap,
    )
    if (n === 0) return ''
    if (n > outCap) return 'the sign-in error did not fit'
    return decoder.decode(new Uint8Array(wasm.memory.buffer, outPtr, n).slice())
  } finally {
    wasm.socli_free_bytes(codePtr, codeBytes.length)
    wasm.socli_free_bytes(statePtr, stateBytes.length)
    wasm.socli_free_bytes(outPtr, outCap)
  }
}

function socliRandom(ptr: number, len: number): void {
  if (len <= 0) return
  const view = new Uint8Array(memory().buffer, ptr, len)
  const max = 65536
  for (let at = 0; at < view.length; at += max) {
    crypto.getRandomValues(view.subarray(at, Math.min(view.length, at + max)))
  }
}

function socliHttp(
  methodPtr: number,
  methodLen: number,
  urlPtr: number,
  urlLen: number,
  headersPtr: number,
  headersLen: number,
  bodyPtr: number,
  bodyLen: number,
  outPtr: number,
  outCap: number,
): number {
  const method = readText(methodPtr, methodLen)
  const url = readText(urlPtr, urlLen)
  const headerText = readText(headersPtr, headersLen)
  const body = readBytes(bodyPtr, bodyLen)
  const key = `${method}\n${url}\n${headerText}\n${body.length}:${fnv(body)}`
  if (stash && stash.key === key) {
    if (stash.bytes.length > outCap) return CONFIG_TOO_BIG
    const bytes = stash.bytes
    stash = null
    writeOut(outPtr, bytes)
    return bytes.length
  }
  stash = null
  let encoded = perform(method, url, headerText, body)
  if (encoded.length > MAX_RESPONSE) {
    encoded = encodeResponse(0, [], encoder.encode('the response is larger than 8 MB'))
  }
  if (encoded.length > outCap) {
    stash = { key, bytes: encoded }
    return CONFIG_TOO_BIG
  }
  writeOut(outPtr, encoded)
  return encoded.length
}

function writeOut(ptr: number, bytes: Uint8Array): void {
  new Uint8Array(memory().buffer, ptr, bytes.length).set(bytes)
}

/** Same-origin forward of api.x.com. The browser cannot read that host. Nothing else is rewritten. */
function relayUrl(url: string): string | null {
  if (relayBase === null) return null
  let parsed: URL
  try {
    parsed = new URL(url)
  } catch {
    return null
  }
  if (parsed.protocol !== 'https:' || parsed.hostname !== 'api.x.com') return null
  if (parsed.username || parsed.password || parsed.pathname.includes('..')) return null
  const base = import.meta.env.BASE_URL || '/'
  const root = base.endsWith('/') ? base : `${base}/`
  const prefix = relayBase ?? `${root}api/relay/api.x.com`
  return new URL(
    `${prefix.replace(/\/$/, '')}${parsed.pathname}${parsed.search}`,
    self.location.origin,
  ).href
}

function perform(method: string, url: string, headerText: string, body: Uint8Array): Uint8Array {
  const relay = relayUrl(url)
  const target = relay ?? url
  let xhr: XMLHttpRequest
  try {
    xhr = new XMLHttpRequest()
    xhr.open(method || 'GET', target, false)
    try {
      xhr.timeout = 20_000
    } catch {
      // A browser that refuses a timeout on this request still sends it.
    }
    const blocked = applyHeaders(xhr, headerText)
    if (blocked) return encodeResponse(0, [], encoder.encode(blocked))
    xhr.send(body.length === 0 ? null : body)
  } catch (err) {
    const message = err instanceof Error && err.message ? err.message : 'the browser could not reach the network'
    return encodeResponse(0, [], encoder.encode(message))
  }
  if (xhr.status === 0) {
    return encodeResponse(0, [], encoder.encode('the browser could not reach the network'))
  }
  if (relay && xhr.status === 404) {
    const type = xhr.getResponseHeader('content-type') ?? ''
    const bodyText = xhr.responseText ?? ''
    if (!type.includes('json') && !bodyText.trimStart().startsWith('{')) {
      return encodeResponse(
        404,
        [],
        encoder.encode('this page has no sign-in server. GitHub Pages does not forward /api/relay.'),
      )
    }
  }
  return encodeResponse(xhr.status, responseHeaders(xhr), encoder.encode(xhr.responseText ?? ''))
}

function applyHeaders(xhr: XMLHttpRequest, headerText: string): string | null {
  if (!headerText) return null
  let parsed: unknown
  try {
    parsed = JSON.parse(headerText)
  } catch {
    return 'the request headers were not valid json'
  }
  if (!Array.isArray(parsed)) return null
  for (const pair of parsed) {
    if (!Array.isArray(pair) || pair.length < 2) continue
    const name = String(pair[0])
    const value = String(pair[1])
    const lower = name.toLowerCase()
    if (lower === 'user-agent') {
      try {
        xhr.setRequestHeader(name, value)
      } catch {
        // The browser keeps its own User-Agent.
      }
      continue
    }
    if (lower === 'cookie') {
      try {
        xhr.setRequestHeader(name, value)
      } catch {
        return COOKIE_BLOCKED
      }
      continue
    }
    xhr.setRequestHeader(name, value)
  }
  return null
}

function responseHeaders(xhr: XMLHttpRequest): [string, string][] {
  const pairs: [string, string][] = []
  for (const line of xhr.getAllResponseHeaders().split('\r\n')) {
    if (!line) continue
    const colon = line.indexOf(':')
    if (colon <= 0) continue
    pairs.push([line.slice(0, colon).trim(), line.slice(colon + 1).trim()])
  }
  return pairs
}

function encodeResponse(status: number, headers: [string, string][], body: Uint8Array): Uint8Array {
  const encoded = headers
    .map(([name, value]) => [encoder.encode(name), encoder.encode(value)] as const)
    .filter(([name, value]) => name.length <= 65535 && value.length <= 65535)
  let size = 8 + body.length
  for (const [name, value] of encoded) size += 4 + name.length + value.length
  const out = new Uint8Array(size)
  const view = new DataView(out.buffer)
  view.setUint16(0, status, true)
  view.setUint16(2, encoded.length, true)
  let at = 4
  for (const [name, value] of encoded) {
    view.setUint16(at, name.length, true)
    at += 2
    out.set(name, at)
    at += name.length
    view.setUint16(at, value.length, true)
    at += 2
    out.set(value, at)
    at += value.length
  }
  view.setUint32(at, body.length, true)
  at += 4
  out.set(body, at)
  return out
}

function fnv(bytes: Uint8Array): number {
  let hash = 2166136261
  for (let i = 0; i < bytes.length; i += 1) {
    hash ^= bytes[i]
    hash = Math.imul(hash, 16777619)
  }
  return hash >>> 0
}
