/**
Same-origin forward of `api.x.com` only. The browser cannot read that host.
The path is the API path. No other host, and no cookies, are forwarded.
*/

const HOST = 'api.x.com'
const MAX_BODY = 1024 * 1024
const TIMEOUT_MS = 20_000
const METHODS = new Set(['GET', 'POST', 'PUT', 'DELETE'])
const FORWARD = new Set(['authorization', 'content-type', 'accept'])

type RelayReq = {
  url?: string
  method?: string
  headers: Record<string, string | string[] | undefined>
}

type RelayRes = {
  statusCode: number
  headersSent: boolean
  setHeader: (name: string, value: string) => void
  end: (body?: string | Uint8Array) => void
}

type RelayNext = () => void

function prefixFor(base: string): string {
  const root = base.endsWith('/') ? base.slice(0, -1) : base
  return `${root}/api/relay/${HOST}`
}

function targetOf(rawUrl: string, prefix: string): URL | null {
  const queryAt = rawUrl.indexOf('?')
  const path = queryAt === -1 ? rawUrl : rawUrl.slice(0, queryAt)
  const search = queryAt === -1 ? '' : rawUrl.slice(queryAt)
  if (path !== prefix && !path.startsWith(`${prefix}/`)) return null
  const rest = path.slice(prefix.length) || '/'
  if (!rest.startsWith('/')) return null
  const whole = rest + search
  if (whole.includes('@') || whole.includes('\\') || whole.includes('..')) return null
  if (/%2e%2e|%5c|%40|%2f%2f/i.test(whole)) return null
  let target: URL
  try {
    target = new URL(`https://${HOST}${rest}${search}`)
  } catch {
    return null
  }
  if (target.protocol !== 'https:' || target.hostname !== HOST || target.username || target.password) {
    return null
  }
  return target
}

async function readBody(req: RelayReq, limit: number): Promise<Uint8Array | null> {
  const chunks: Uint8Array[] = []
  let size = 0
  for await (const chunk of req as unknown as AsyncIterable<Uint8Array | string>) {
    const bytes = typeof chunk === 'string' ? new TextEncoder().encode(chunk) : chunk
    size += bytes.byteLength
    if (size > limit) return null
    chunks.push(bytes)
  }
  const out = new Uint8Array(size)
  let at = 0
  for (const chunk of chunks) {
    out.set(chunk, at)
    at += chunk.byteLength
  }
  return out
}

function send(res: RelayRes, status: number, body: string): void {
  res.statusCode = status
  res.setHeader('content-type', 'text/plain; charset=utf-8')
  res.end(body)
}

async function relay(req: RelayReq, res: RelayRes, prefix: string): Promise<void> {
  const method = (req.method ?? 'GET').toUpperCase()
  if (method === 'OPTIONS') {
    res.statusCode = 204
    res.setHeader('allow', 'GET, POST, PUT, DELETE, OPTIONS')
    res.end()
    return
  }
  const target = targetOf(req.url ?? '', prefix)
  if (!target) {
    send(res, 400, 'this relay only forwards api.x.com')
    return
  }
  if (!METHODS.has(method)) {
    send(res, 405, 'method not forwarded')
    return
  }
  const body = method === 'GET' ? new Uint8Array() : await readBody(req, MAX_BODY)
  if (body === null) {
    send(res, 413, 'the request is larger than 1 MB')
    return
  }
  const headers = new Headers()
  for (const [name, value] of Object.entries(req.headers)) {
    if (!FORWARD.has(name.toLowerCase()) || value === undefined) continue
    headers.set(name, Array.isArray(value) ? value.join(', ') : value)
  }
  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), TIMEOUT_MS)
  let response: Response
  try {
    response = await fetch(target, {
      method,
      headers,
      body: method === 'GET' || body.byteLength === 0 ? undefined : body,
      redirect: 'manual',
      signal: controller.signal,
    })
  } catch {
    send(res, 502, 'could not reach api.x.com')
    return
  } finally {
    clearTimeout(timer)
  }
  if (response.status >= 300 && response.status < 400) {
    send(res, 502, 'api.x.com redirected')
    return
  }
  const type = response.headers.get('content-type')
  const payload = new Uint8Array(await response.arrayBuffer())
  res.statusCode = response.status
  if (type) res.setHeader('content-type', type)
  res.end(payload)
}

export function xRelay(base: string) {
  const prefix = prefixFor(base)
  const onRequest = (req: RelayReq, res: RelayRes, next: RelayNext) => {
    const raw = req.url ?? ''
    const path = raw.split('?')[0] ?? ''
    if (path !== prefix && !path.startsWith(`${prefix}/`)) {
      next()
      return
    }
    void relay(req, res, prefix).catch(() => {
      if (!res.headersSent) send(res, 502, 'could not reach api.x.com')
    })
  }
  return {
    name: 'socli-x-relay',
    configureServer(server: { middlewares: { use: (fn: typeof onRequest) => void } }) {
      server.middlewares.use(onRequest)
    },
    configurePreviewServer(server: { middlewares: { use: (fn: typeof onRequest) => void } }) {
      server.middlewares.use(onRequest)
    },
  }
}
