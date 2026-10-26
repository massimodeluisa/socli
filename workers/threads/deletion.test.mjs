import assert from "node:assert/strict"
import { createHmac } from "node:crypto"
import test from "node:test"
import { describeGraphFailure, handle, verifySignedRequest } from "./index.js"

const SECRET = "test-secret"
const USER = "218471"

function b64url(bytes) {
  return Buffer.from(bytes).toString("base64url")
}

function sign(payload, secret = SECRET) {
  const body = b64url(JSON.stringify(payload))
  const sig = b64url(createHmac("sha256", secret).update(body).digest())
  return `${sig}.${body}`
}

function post(path, body, headers = {}) {
  return new Request(`https://socli.sh${path}`, { method: "POST", body, headers })
}

test("a signed deletion request returns a code and does not echo the user id", async () => {
  const signed = sign({ algorithm: "HMAC-SHA256", user_id: USER })
  const response = await handle(
    post("/threads/deletion", new URLSearchParams({ signed_request: signed }), {
      "content-type": "application/x-www-form-urlencoded",
    }),
    { THREADS_APP_SECRET: SECRET },
  )
  assert.equal(response.status, 200)
  const body = await response.json()
  assert.match(body.confirmation_code, /^[a-f0-9]{32}$/)
  assert.equal(body.url, `https://socli.sh/data-deletion.html?code=${body.confirmation_code}`)
  assert.equal(JSON.stringify(body).includes(USER), false)
  assert.equal(response.headers.get("access-control-allow-origin"), null)
})

test("a tampered signature is refused", async () => {
  const signed = sign({ algorithm: "HMAC-SHA256", user_id: USER }, "other-secret")
  assert.equal(await verifySignedRequest(signed, SECRET), null)
  const response = await handle(
    post("/threads/deletion", new URLSearchParams({ signed_request: signed })),
    { THREADS_APP_SECRET: SECRET },
  )
  assert.equal(response.status, 400)
})

test("delete matches deletion, and deauthorize confirms without keeping the user", async () => {
  const signed = sign({ algorithm: "HMAC-SHA256", user_id: USER })
  const form = new URLSearchParams({ signed_request: signed })
  const headers = { "content-type": "application/x-www-form-urlencoded" }
  const deleted = await handle(post("/threads/delete", form, headers), { THREADS_APP_SECRET: SECRET })
  assert.equal(deleted.status, 200)
  const body = await deleted.json()
  assert.match(body.confirmation_code, /^[a-f0-9]{32}$/)
  assert.equal(body.url, `https://socli.sh/data-deletion.html?code=${body.confirmation_code}`)
  assert.equal(JSON.stringify(body).includes(USER), false)

  const gone = await handle(post("/threads/deauthorize", form, headers), { THREADS_APP_SECRET: SECRET })
  assert.equal(gone.status, 200)
  assert.deepEqual(await gone.json(), { ok: true })

  const env = { THREADS_APP_SECRET: SECRET }
  assert.equal((await handle(new Request("https://socli.sh/threads/delete"), env)).status, 405)
  assert.equal((await handle(new Request("https://socli.sh/threads/deauthorize"), env)).status, 405)
  assert.equal((await handle(post("/threads/deauthorize", ""), env)).status, 400)
  const tampered = sign({ algorithm: "HMAC-SHA256", user_id: USER }, "other-secret")
  assert.equal(
    (await handle(post("/threads/delete", new URLSearchParams({ signed_request: tampered })), env)).status,
    400,
  )
  assert.equal(
    (await handle(post("/threads/deauthorize", new URLSearchParams({ signed_request: signed })), {})).status,
    500,
  )
})

test("a probe body is just a sign-in with no code", async () => {
  const response = await handle(
    post("/threads/token", JSON.stringify({ probe: "app" }), {
      "content-type": "application/json",
    }),
    { THREADS_APP_ID: "1", THREADS_APP_SECRET: SECRET },
  )
  assert.equal(response.status, 400)
  const body = await response.json()
  assert.equal(body.error, "this sign-in has expired. start it again.")
  assert.equal(body.ok, undefined)
})

test("graph failures keep Meta's message and subcode", () => {
  assert.equal(
    describeGraphFailure({
      error: { message: "Invalid verification code", type: "OAuthException", code: 1, error_subcode: 36006 },
    }),
    "Invalid verification code, code 1, subcode 36006",
  )
  assert.equal(
    describeGraphFailure({
      error_type: "OAuthException",
      code: 400,
      error_message: "Matching code was not found or was already used",
    }),
    "Matching code was not found or was already used, code 400",
  )
  assert.equal(
    describeGraphFailure({
      error: {
        message: "Invalid parameter",
        code: 100,
        error_subcode: 36008,
        error_user_msg: "redirect uri does not match",
      },
    }),
    "redirect uri does not match, code 100, subcode 36008",
  )
})

test("deletion ignores a browser origin and token exchange still rejects a bad redirect", async () => {
  const denied = await handle(
    post(
      "/threads/token",
      JSON.stringify({ code: "abc", redirect_uri: "https://evil.example/steal" }),
      { "content-type": "application/json", origin: "https://evil.example" },
    ),
    { THREADS_APP_ID: "1", THREADS_APP_SECRET: SECRET },
  )
  assert.equal(denied.status, 400)
  assert.equal(denied.headers.get("access-control-allow-origin"), null)

  const missing = await handle(new Request("https://socli.sh/threads/token"), {
    THREADS_APP_SECRET: SECRET,
  })
  assert.equal(missing.status, 404)

  const open = await handle(
    new Request("https://socli.sh/threads/deletion", { method: "GET" }),
    { THREADS_APP_SECRET: SECRET },
  )
  assert.equal(open.status, 405)
})
