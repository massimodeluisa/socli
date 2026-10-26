/**
 * Trades a Threads authorization code for a user token.
 * The Threads app secret stays in the Worker secret THREADS_APP_SECRET.
 * THREADS_APP_ID is the Threads app id, not the Facebook app id.
 * Keep the redirect list in step with src/net/threads/auth.rs.
 * A failed 60-day swap still returns the one-hour token. The code was already spent.
 *
 * POST /threads/deletion and POST /threads/delete answer Meta's data-deletion callback.
 * POST /threads/deauthorize answers the uninstall callback.
 * The signed user id is checked and then discarded. Nothing is stored.
 */

const REDIRECTS = new Set([
  "http://127.0.0.1:8742/threads/callback",
  "http://127.0.0.1:5174/",
  "http://127.0.0.1:5174/web/",
  "https://socli.sh/",
  "https://socli.sh/web/",
  "https://web.socli.sh/web/",
])

const ORIGINS = new Set([
  "http://127.0.0.1:5174",
  "https://socli.sh",
  "https://web.socli.sh",
])

const DELETION_STATUS = "https://socli.sh/data-deletion.html"

export default {
  async fetch(request, env) {
    return handle(request, env)
  },
}

export async function handle(request, env) {
  const origin = request.headers.get("Origin")
  const path = new URL(request.url).pathname
  if (request.method === "OPTIONS" && path === "/threads/token") {
    return new Response(null, { status: 204, headers: cors(origin) })
  }
  if (path === "/threads/deletion" || path === "/threads/delete") return deletion(request, env)
  if (path === "/threads/deauthorize") return deauthorize(request, env)
  if (request.method !== "POST" || path !== "/threads/token") {
    return json({ error: "not found" }, 404, origin)
  }
  let body
  try {
    body = await request.json()
  } catch {
    return json({ error: "the body must be JSON" }, 400, origin)
  }
  const code = typeof body.code === "string" ? body.code.trim() : ""
  const redirect = typeof body.redirect_uri === "string" ? body.redirect_uri : ""
  if (!code || code.length > 512 || /\s/.test(code) || !REDIRECTS.has(redirect)) {
    return json({ error: "this sign-in has expired. start it again." }, 400, origin)
  }
  const appId = env.THREADS_APP_ID || ""
  const secret = env.THREADS_APP_SECRET || ""
  if (!appId || !secret) {
    return json({ error: "the Threads exchange is not configured" }, 500, origin)
  }
  const exchanged = await postForm("https://graph.threads.com/oauth/access_token", {
    client_id: appId,
    client_secret: secret,
    grant_type: "authorization_code",
    redirect_uri: redirect,
    code,
  })
  if (!exchanged.ok) {
    const text = scrub(exchanged.error, secret, code)
    return json({ error: text, error_message: text }, 502, origin)
  }
  const longLived = await longLivedToken(secret, exchanged.access_token)
  const token = longLived.ok ? longLived : exchanged
  return json(
    {
      access_token: token.access_token,
      expires_in: seconds(token.expires_in, longLived.ok ? 60 * 86400 : 3600),
      user_id: exchanged.user_id,
    },
    200,
    origin,
  )
}

async function deletion(request, env) {
  const signed = await readSigned(request, env)
  if (signed.response) return signed.response
  const code = confirmationCode()
  return json(
    {
      url: `${DELETION_STATUS}?code=${code}`,
      confirmation_code: code,
    },
    200,
    null,
  )
}

async function deauthorize(request, env) {
  const signed = await readSigned(request, env)
  if (signed.response) return signed.response
  return json({ ok: true }, 200, null)
}

async function readSigned(request, env) {
  if (request.method !== "POST") return { response: json({ error: "method not allowed" }, 405, null) }
  const secret = env.THREADS_APP_SECRET || ""
  if (!secret) return { response: json({ error: "the deletion callback is not configured" }, 500, null) }
  let raw
  try {
    raw = await request.text()
  } catch {
    return { response: json({ error: "bad request" }, 400, null) }
  }
  if (raw.length > 8192) return { response: json({ error: "bad request" }, 400, null) }
  const signed = signedRequestFrom(raw, request.headers.get("content-type") || "")
  const data = signed ? await verifySignedRequest(signed, secret) : null
  if (!data) return { response: json({ error: "bad request" }, 400, null) }
  return { data }
}

function signedRequestFrom(raw, contentType) {
  if (contentType.includes("application/json")) {
    try {
      const body = JSON.parse(raw)
      return typeof body.signed_request === "string" ? body.signed_request : ""
    } catch {
      return ""
    }
  }
  return new URLSearchParams(raw).get("signed_request") || ""
}

export async function verifySignedRequest(signed, secret) {
  const parts = signed.split(".")
  if (parts.length !== 2 || !secret) return null
  const [encodedSig, payload] = parts
  const sig = bytesFromBase64Url(encodedSig)
  const body = bytesFromBase64Url(payload)
  if (!sig || !body || body.length > 4096) return null
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(secret),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["verify"],
  )
  const ok = await crypto.subtle.verify("HMAC", key, sig, new TextEncoder().encode(payload))
  if (!ok) return null
  let data
  try {
    data = JSON.parse(new TextDecoder().decode(body))
  } catch {
    return null
  }
  if (!data || data.algorithm !== "HMAC-SHA256" || !acceptableUserId(data.user_id)) return null
  return { ok: true }
}

function acceptableUserId(value) {
  if (typeof value === "number") return Number.isSafeInteger(value) && value > 0
  return typeof value === "string" && value.length > 0 && value.length <= 128 && !/\s/.test(value)
}

function bytesFromBase64Url(input) {
  if (!input || !/^[A-Za-z0-9_-]+$/.test(input)) return null
  const pad = "=".repeat((4 - (input.length % 4)) % 4)
  try {
    const binary = atob(input.replaceAll("-", "+").replaceAll("_", "/") + pad)
    const out = new Uint8Array(binary.length)
    for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i)
    return out
  } catch {
    return null
  }
}

function confirmationCode() {
  const bytes = new Uint8Array(16)
  crypto.getRandomValues(bytes)
  return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("")
}

function cors(origin) {
  const headers = {
    "access-control-allow-methods": "POST, OPTIONS",
    "access-control-allow-headers": "content-type",
    "cache-control": "no-store",
  }
  if (origin && ORIGINS.has(origin)) {
    headers["access-control-allow-origin"] = origin
    headers.vary = "Origin"
  }
  return headers
}

function json(body, status, origin) {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json", ...cors(origin) },
  })
}

function scrub(text, secret, token) {
  let clean = String(text || "Threads refused the code")
  if (secret) clean = clean.split(secret).join("[secret]")
  if (token) clean = clean.split(token).join("[token]")
  return clean.slice(0, 300)
}

/** Meta's code, subcode, and message. Either error shape, nothing else. */
export function describeGraphFailure(payload) {
  const nested = payload && typeof payload.error === "object" && payload.error ? payload.error : null
  const message =
    text(payload?.error_message) ||
    text(nested?.error_user_msg) ||
    text(nested?.message) ||
    text(payload?.error) ||
    "Threads refused the code"
  const code = nested?.code ?? payload?.code
  const sub = nested?.error_subcode ?? payload?.error_subcode
  const bits = [message]
  if (code !== undefined && code !== null && code !== "") bits.push(`code ${code}`)
  if (sub !== undefined && sub !== null && sub !== "") bits.push(`subcode ${sub}`)
  return bits.join(", ")
}

function text(value) {
  return typeof value === "string" && value.trim() ? value.trim() : ""
}

function seconds(value, fallback) {
  const n = typeof value === "number" ? value : Number(value)
  return Number.isFinite(n) && n > 0 ? Math.floor(n) : fallback
}

async function postForm(url, fields) {
  const body = new URLSearchParams(fields)
  const response = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body,
  })
  return readToken(response)
}

async function longLivedToken(secret, token) {
  let last = { ok: false, error: "Threads refused the code" }
  for (const host of ["https://graph.threads.com/access_token", "https://graph.threads.net/access_token"]) {
    const url = new URL(host)
    url.searchParams.set("grant_type", "th_exchange_token")
    url.searchParams.set("client_secret", secret)
    url.searchParams.set("access_token", token)
    last = await getToken(url)
    if (last.ok) return last
    last = { ...last, error: scrub(last.error, secret, token) }
  }
  return last
}

async function getToken(url) {
  return readToken(await fetch(url))
}

async function readToken(response) {
  const raw = await response.text()
  let payload = null
  if (raw) {
    try {
      payload = JSON.parse(raw)
    } catch {
      const params = new URLSearchParams(raw)
      const token = params.get("access_token")
      if (token) {
        payload = {
          access_token: token,
          expires_in: params.get("expires_in"),
          user_id: params.get("user_id"),
        }
      }
    }
  }
  if (!response.ok || !payload || typeof payload.access_token !== "string" || !payload.access_token) {
    return { ok: false, error: payload ? describeGraphFailure(payload) : "Threads refused the code" }
  }
  return {
    ok: true,
    access_token: payload.access_token,
    expires_in: payload.expires_in,
    user_id: payload.user_id,
  }
}
