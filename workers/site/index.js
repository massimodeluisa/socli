/**
Routes only, not a custom domain. The apex route has to end in * or a query string
never reaches this Worker. /threads/token, /threads/deletion, /threads/delete
and /threads/deauthorize stay on the other Worker.
The terminal cannot register an http://127.0.0.1 redirect with Meta, so its authorize
redirect is https://socli.sh/. A state that starts with x. or th. plus 64 characters
is the terminal, and this Worker sends the browser on to the loopback the process
is already listening on. A page sign-in uses a state without that prefix and stays here.
A Tufty badge cannot register its LAN address either. Its state is xb. plus 8 hex
chars of a private IPv4 plus 64 characters, and this Worker sends the phone to
http://that-address:8080/x/callback. Any other xb. state is dropped, so the code
is not an open redirect.
Privacy, terms, and data deletion are the pages bundled in this Worker. The reader,
its wasm, and the docs are static assets. The homepage is the built site when those
assets are bound, and the bundled page when they are not. Anything else is passed
to the origin when the origin answers 2xx or 3xx.
The browser cannot read api.x.com, so /api/relay/api.x.com forwards that host only.
*/

import { deletion, home, privacy, terms } from "./pages.js"

const LOOPBACK = {
  x: "http://127.0.0.1:8741/callback",
  th: "http://127.0.0.1:8742/threads/callback",
}

/** Terminal state is `x.` or `th.` plus a 64-character verifier. A page state is 64 characters with no prefix, so a page state that happens to start with `x.` or `th.` is shorter after that prefix and stays on the page. */
const CLI_STATE = /^(x|th)\.[A-Za-z0-9._~-]{64}$/
/** Badge state is `xb.` plus the private IPv4 in hex plus a 64-character nonce. `x.` is the terminal, so this must not match that. */
const BADGE_STATE = /^xb\.([0-9a-f]{8})\.([A-Za-z0-9._~-]{64})$/
const CLI_CODE = /^[A-Za-z0-9._~+/-]{1,512}$/
const CLI_ERROR = /^[A-Za-z0-9_]{1,64}$/

function oauthTarget(url, target) {
  const code = url.searchParams.get("code")
  const error = url.searchParams.get("error")
  if (code && CLI_CODE.test(code)) target.searchParams.set("code", code)
  else if (error && CLI_ERROR.test(error)) {
    target.searchParams.set("error", error)
    const desc = url.searchParams.get("error_description") || ""
    if (desc && desc.length <= 200 && !/[\r\n]/.test(desc)) {
      target.searchParams.set("error_description", desc)
    }
  } else return null
  target.searchParams.set("state", url.searchParams.get("state") || "")
  return target.toString()
}

/** The terminal's loopback, or null when this request is an ordinary page visit. */
export function cliLoopback(url) {
  if (url.pathname !== "/" && url.pathname !== "/index.html") return null
  const state = url.searchParams.get("state") || ""
  const match = CLI_STATE.exec(state)
  if (!match) return null
  return oauthTarget(url, new URL(LOOPBACK[match[1]]))
}

/** A private IPv4 from 8 hex chars, or null. Loopback, public and CGNAT addresses are not private here. */
export function badgeHost(hex) {
  if (!/^[0-9a-f]{8}$/.test(hex)) return null
  const bytes = [0, 1, 2, 3].map((i) => Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16))
  const [a, b] = bytes
  const priv =
    a === 10 ||
    (a === 172 && b >= 16 && b <= 31) ||
    (a === 192 && b === 168) ||
    (a === 169 && b === 254)
  if (!priv) return null
  return bytes.join(".")
}

/**
The badge's sign-in callback, "drop" when the state is shaped like a badge
state but must not be forwarded, or null for an ordinary visit.
Port 8080 is the badge's sign-in port. The address comes only from the hex
in the state, so the redirect cannot point at an arbitrary host.
*/
export function badgeLoopback(url) {
  if (url.pathname !== "/" && url.pathname !== "/index.html") return null
  const state = url.searchParams.get("state") || ""
  const match = BADGE_STATE.exec(state)
  if (!match) return null
  const host = badgeHost(match[1])
  if (!host) return "drop"
  const forwarded = oauthTarget(url, new URL(`http://${host}:8080/x/callback`))
  return forwarded || "drop"
}

const PAGES = new Map([
  ["/", home],
  ["/index.html", home],
  ["/privacy", privacy],
  ["/privacy.html", privacy],
  ["/terms", terms],
  ["/terms.html", terms],
  ["/data-deletion", deletion],
  ["/data-deletion.html", deletion],
])

const SECURITY = {
  "content-security-policy":
    "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
  "strict-transport-security": "max-age=31536000; includeSubDomains",
  "x-content-type-options": "nosniff",
  "referrer-policy": "no-referrer",
  "cache-control": "no-store",
}

const HSTS = "max-age=31536000; includeSubDomains"

/** Adds HSTS without the legal-page policy. Asset responses keep their own content type. */
function stamp(response) {
  const headers = new Headers(response.headers)
  if (!headers.has("strict-transport-security")) headers.set("strict-transport-security", HSTS)
  return new Response(response.body, {
    status: response.status,
    statusText: response.statusText,
    headers,
  })
}

export function fallbackHtml(pathname) {
  return PAGES.get(pathname) ?? null
}

/** Legal pages stay on the copy bundled in this Worker. The built site's copies are older. */
const LEGAL = new Set(["/privacy", "/privacy.html", "/terms", "/terms.html", "/data-deletion", "/data-deletion.html"])

const READER_FILES = new Set([
  "/favicon.ico",
  "/favicon.svg",
  "/favicon-16.png",
  "/favicon-32.png",
  "/apple-touch-icon.png",
  "/site.webmanifest",
  "/og.png",
  "/icon-192.png",
  "/icon-512.png",
  "/icon-maskable-512.png",
  "/vp-icons.css",
  "/hashmap.json",
  "/llms.txt",
  "/llms-full.txt",
  "/sitemap.xml",
  "/design.html",
  "/design.md",
  "/docs.html",
  "/docs.md",
  "/404.html",
  "/install.sh",
])

/** True when this path is a file from the built site, including the homepage and the reader. */
export function readerAsset(pathname) {
  if (typeof pathname !== "string" || pathname[0] !== "/") return false
  if (pathname.includes("\\") || pathname.includes("\0")) return false
  if (LEGAL.has(pathname)) return false
  if (pathname === "/" || pathname === "/index.html") return true
  if (pathname === "/web" || pathname.startsWith("/web/")) return true
  if (pathname.startsWith("/assets/")) return true
  if (pathname.startsWith("/wasm/")) return true
  if (pathname.startsWith("/fonts/")) return true
  if (pathname.startsWith("/brand/")) return true
  if (pathname === "/docs" || pathname.startsWith("/docs/")) return true
  return READER_FILES.has(pathname)
}

/**
The asset pathname for a reader request.
`/web` and `/web/` are the file `web/index.html`. Fetching that file returns its body.
A redirect to add the slash would move the OAuth query onto a second URL.
*/
export function readerAssetPath(pathname) {
  if (!readerAsset(pathname)) return null
  if (pathname === "/" || pathname === "/index.html") return "/index.html"
  if (pathname === "/web" || pathname === "/web/") return "/web/index.html"
  if (pathname === "/docs" || pathname === "/docs/") return "/docs/index.html"
  return pathname
}

/** A live origin wins. 4xx and 5xx, including a Cloudflare 522, fall back to the bundled page. */
export function useOrigin(status) {
  return status >= 200 && status < 400
}

const RELAY_HOST = "api.x.com"
const RELAY_PREFIX = `/api/relay/${RELAY_HOST}`
const RELAY_MAX_BODY = 1024 * 1024
const RELAY_METHODS = new Set(["GET", "POST", "PUT", "DELETE"])
const RELAY_HEADERS = ["authorization", "content-type", "accept"]
const RELAY_TRICK = /%2e%2e|%5c|%40|%2f%2f/i

/** The api.x.com URL for a /api/relay/api.x.com request, or null when it must not be forwarded. */
export function relayTarget(url) {
  const raw = url instanceof URL ? url.href : typeof url === "string" ? url : ""
  if (!raw) return null
  if (url instanceof URL && (url.username || url.password)) return null
  const pathQuery = relayPathQuery(raw)
  if (pathQuery == null) return null
  const queryAt = pathQuery.indexOf("?")
  const path = queryAt === -1 ? pathQuery : pathQuery.slice(0, queryAt)
  const search = queryAt === -1 ? "" : pathQuery.slice(queryAt)
  if (path !== RELAY_PREFIX && !path.startsWith(`${RELAY_PREFIX}/`)) return null
  const rest = path.slice(RELAY_PREFIX.length) || "/"
  if (!rest.startsWith("/")) return null
  const whole = rest + search
  if (whole.includes("@") || whole.includes("\\") || whole.includes("..")) return null
  if (RELAY_TRICK.test(whole)) return null
  let target
  try {
    target = new URL(`https://${RELAY_HOST}${rest}${search}`)
  } catch {
    return null
  }
  if (target.protocol !== "https:" || target.hostname !== RELAY_HOST || target.username || target.password) {
    return null
  }
  return target.href
}

function relayPathQuery(raw) {
  const scheme = raw.indexOf("://")
  let rest = raw
  if (scheme !== -1) {
    rest = raw.slice(scheme + 3)
    const slash = rest.indexOf("/")
    if (slash === -1) return "/"
    const authority = rest.slice(0, slash)
    if (authority.includes("@") || authority.includes("\\") || RELAY_TRICK.test(authority)) return null
    rest = rest.slice(slash)
  }
  const hash = rest.indexOf("#")
  if (hash !== -1) rest = rest.slice(0, hash)
  if (!rest.startsWith("/")) return null
  return rest
}

function isRelayPath(pathname) {
  return pathname === RELAY_PREFIX || pathname.startsWith(`${RELAY_PREFIX}/`)
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url)
    if (request.method === "GET" || request.method === "HEAD") {
      const loop = cliLoopback(url) || badgeLoopback(url)
      if (loop) {
        return stamp(
          new Response(null, {
            status: 302,
            headers: {
              location: loop === "drop" ? "https://socli.sh/" : loop,
              "cache-control": "no-store",
              "referrer-policy": "no-referrer",
            },
          }),
        )
      }
    }
    const relayed = await forwardRelay(request, url)
    if (relayed) return stamp(relayed)
    const html = fallbackHtml(url.pathname)
    const home = url.pathname === "/" || url.pathname === "/index.html"
    if (html && !home) return page(request, html)
    const asset = await serveAsset(request, env, url)
    if (asset) return stamp(asset)
    if (html) return page(request, html)
    const origin = await passOrigin(request)
    if (origin) return stamp(origin)
    return text("not found", 404)
  },
}

function page(request, html) {
  if (request.method !== "GET" && request.method !== "HEAD") return text("method not allowed", 405)
  return new Response(request.method === "HEAD" ? null : html, {
    status: 200,
    headers: { "content-type": "text/html; charset=utf-8", ...SECURITY },
  })
}

async function serveAsset(request, env, url) {
  const path = readerAssetPath(url.pathname)
  if (!path) return null
  if (request.method !== "GET" && request.method !== "HEAD") return text("method not allowed", 405)
  const assets = env?.ASSETS
  if (!assets || typeof assets.fetch !== "function") return null
  const target = new URL(request.url)
  target.pathname = path
  let response
  try {
    response = await assets.fetch(path === url.pathname ? request : new Request(target, request))
  } catch {
    /* The asset binding is missing this file. The bundled page or the origin may have it. */
    return null
  }
  if (!response || response.status === 404) {
    try {
      await response?.body?.cancel()
    } catch {
      /* The unused 404 body is discarded. */
    }
    return null
  }
  return response
}

function text(body, status) {
  return new Response(body, {
    status,
    headers: { "content-type": "text/plain; charset=utf-8", ...SECURITY },
  })
}

async function forwardRelay(request, url) {
  if (!isRelayPath(url.pathname)) return null
  if (request.method === "OPTIONS") {
    return new Response(null, {
      status: 204,
      headers: {
        allow: "GET, POST, PUT, DELETE, OPTIONS",
        "cache-control": "no-store",
      },
    })
  }
  const target = relayTarget(request.url)
  if (!target) return text("this relay only forwards api.x.com", 400)
  if (!RELAY_METHODS.has(request.method)) return text("method not forwarded", 405)
  let body
  if (request.method !== "GET") {
    let bytes
    try {
      bytes = await readLimited(request, RELAY_MAX_BODY)
    } catch {
      /* The body could not be read. Do not include it in the response. */
      return text("could not reach api.x.com", 502)
    }
    if (bytes === null) return text("the request is larger than 1 MB", 413)
    if (bytes.byteLength > 0) body = bytes
  }
  const headers = new Headers()
  for (const name of RELAY_HEADERS) {
    const value = request.headers.get(name)
    if (value) headers.set(name, value)
  }
  let response
  try {
    response = await fetch(target, {
      method: request.method,
      headers,
      body,
      redirect: "manual",
      credentials: "omit",
      signal: AbortSignal.timeout(20000),
    })
  } catch {
    /* api.x.com timed out or refused the connection. */
    return text("could not reach api.x.com", 502)
  }
  if (response.type === "opaqueredirect" || (response.status >= 300 && response.status < 400)) {
    return text("api.x.com redirected", 502)
  }
  if (response.status < 200 || response.status > 599) return text("could not reach api.x.com", 502)
  let payload
  try {
    payload = await response.arrayBuffer()
  } catch {
    /* The upstream body stopped before it finished. */
    return text("could not reach api.x.com", 502)
  }
  const out = new Headers({ "cache-control": "no-store" })
  const type = response.headers.get("content-type")
  if (type) out.set("content-type", type)
  const upstreamBody = response.status === 204 || response.status === 205 ? null : payload
  return new Response(upstreamBody, { status: response.status, headers: out })
}

async function readLimited(request, limit) {
  const declared = request.headers.get("content-length")
  if (declared !== null) {
    const size = Number(declared)
    if (Number.isFinite(size) && size > limit) return null
  }
  if (!request.body) return new Uint8Array()
  const reader = request.body.getReader()
  const chunks = []
  let size = 0
  for (;;) {
    const { done, value } = await reader.read()
    if (done) break
    const bytes = value.byteLength
    if (size + bytes > limit) {
      try {
        await reader.cancel()
      } catch {
        /* The extra bytes are discarded with the request. */
      }
      return null
    }
    chunks.push(value.slice())
    size += bytes
  }
  const out = new Uint8Array(size)
  let at = 0
  for (const chunk of chunks) {
    out.set(chunk, at)
    at += chunk.byteLength
  }
  return out
}

async function passOrigin(request) {
  try {
    const response = await fetch(request, { signal: AbortSignal.timeout(4000) })
    if (useOrigin(response.status)) return response
  } catch {
    /* The origin timed out or refused the connection. Serve the bundled page. */
  }
  return null
}
