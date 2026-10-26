# dev/null: signing in

SoCLI has two ways to act as a dev/null user, and either works without the other. A device login registers an ed25519 key with your account, after which SoCLI signs every API request with it. An SSH login only tells SoCLI which of your GitHub SSH keys to sign commits with, and it supports posting and replying. Public reading needs neither. The rest of this folder: [overview](README.md), [endpoints](endpoints.md), [mapping](mapping.md), [testing](testing.md).

## Device login

```
socli login devnull [--device-name <name>]
socli auth devnull login [--device-name <name>]
```

`--device-name` sets the label the device shows in your dev/null settings. The default is `SoCLI on <short hostname>` (from the `hostname` command, else `HOSTNAME`, else `COMPUTERNAME`, cut at the first dot), or `SoCLI` when none is available. Names are limited to 100 characters.

Steps:

1. SoCLI loads the device key from `<config dir>/devnull_ed25519`, or creates it (OpenSSH ed25519, mode 0600 in a 0700 directory). An existing key is never overwritten. It records the key file name as `device_key` and shows the SHA256 fingerprint.
2. It generates a PKCE verifier and S256 challenge and a random `state`.
3. On macOS, SoCLI opens a native browser authentication session for `https://dev-null.sh/mobile/authorize`, with the complete fingerprint, device name, PKCE challenge, state and `devnull://auth/callback` redirect. The session captures the custom callback directly; SoCLI does not register a global `devnull://` handler or rely on Safari opening that address itself.
4. Sign in with GitHub if needed, then approve the device. The native session returns the callback to SoCLI, which validates its destination and state. Canceling the browser session cancels the procedure.
5. On other platforms, or with `SOCLI_AUTH_BROWSER=manual`, SoCLI prints a clickable authorization URL, opens the browser and prompts for the callback. Paste the complete `devnull://auth/callback?code=…&state=…` address, text containing it, or a bare code. A browser without a handler for that scheme may require copying the approval response's `Location` header. Each wrapped URL segment opens the complete authorization request, including its fingerprint. An `error` parameter or mismatched state stops the attempt.
6. It posts `{code, code_verifier}` to `/api/v1/devices`, signed with the device key. When the answer carries no username, it calls `/api/v1/me`.
7. It saves `device_approved = true` and the `username`.

The provider currently allows only the `devnull://` redirect prefix ([provider configuration](https://github.com/dev-null-sh/dev-null.sh/blob/production/config/mobile.php)). macOS uses [ASWebAuthenticationSession](https://developer.apple.com/documentation/authenticationservices/aswebauthenticationsession) to receive that callback without a localhost redirect. The helper is compiled with SoCLI and stored inside the SoCLI configuration directory when first needed; end users do not need Swift installed. Building SoCLI on macOS requires Xcode Command Line Tools, including `swiftc`.

When launched from the reader, an error stays visible with Retry and Cancel actions. Success waits for Continue before loading Discover. An explicit `socli login devnull` command prints its result and exits.

## SSH login

```
socli auth devnull ssh [--key <path>]
```

This path needs `ssh-keygen` on the machine, and a public key that is registered on your GitHub account.

1. SoCLI collects candidate public keys: with `--key`, the `.pub` file (or the `.pub` next to the private key you name); otherwise the keys listed by `ssh-add -L` plus the `~/.ssh/*.pub` files that sit next to a private key. It keeps `ssh-ed25519` and `ssh-rsa` keys only, removes duplicates and sends at most 20, which is as many as the server reads.
2. It posts them as plain text, one `<type> <base64>` per line, to `/api/cli/key-lookups`.
3. The first line of the answer must equal one of the candidates. SoCLI stores that full public key line as `ssh_key`.
4. If the key is in neither the agent nor `~/.ssh`, SoCLI warns you to `ssh-add` it.

Each commit is then signed at posting time: the payload `timestamp\nnonce\nreply_to\nmessage` is signed with `ssh-keygen -Y sign -n devnull-commit` and sent to `/api/cli/commits` in the `X-SSH-Signature` header. A key held only in the agent is written as a temporary `.pub` file for the call. The `reply_to` value may contain only ASCII letters, digits and `:/._@-`.

## What is stored

The `[devnull]` section of `config.toml` (same location rules as the other networks: `$SOCLI_CONFIG_DIR`, else `$XDG_CONFIG_HOME/socli`, else `~/.config/socli`, file mode 0600):

| Key | Content |
| --- | --- |
| `username` | Your dev/null username, updated when a signed answer reports another one. |
| `device_key` | File name of the private key inside the config directory. Default `devnull_ed25519`. |
| `device_approved` | `true` once the device login finished. |
| `ssh_key` | The public key line chosen by the SSH login. |

The private key is the separate file `<config dir>/devnull_ed25519`. `socli auth status` shows the account (`not signed in`, `@user` or `signed in`), the device key (`approved (<file>)`, `waiting for approval (<file>)` or `missing`) and the SSH key (redacted).

## Environment variables

No variable selects a dev/null credential. SoCLI reads `HOSTNAME` or `COMPUTERNAME` for the default device name, and `HOME` to find `~/.ssh`. The shared variables for the configuration directory and agent mode are documented in [../README.md](../README.md).

## How requests are signed

With an approved device, every `/api/v1/` request, reads included, carries three headers: `X-Devnull-Timestamp` (Unix seconds), `X-Devnull-Nonce` (32 lowercase hex characters) and `X-SSH-Signature`. The signature is an SSHSIG (sha512, namespace `devnull-api`, one line of base64) over `timestamp`, `nonce`, the method, the path and query as sent, and the raw body, joined by newlines. Signed reads are what return per-user fields such as `starred`.

## Token lifetime

SoCLI holds no token that expires. The device stays approved until you revoke it in the dev/null settings. The server rejects a signature whose timestamp is stale, so each request is signed again.

## Sign out

```
socli logout devnull
socli auth devnull logout
socli logout all
```

This clears the `[devnull]` section. The key file stays, and SoCLI prints `The device key stays at <path>. Revoke the device at https://dev-null.sh/settings/devices`.

## Agents and scripts

The device login needs a person at a browser, because step 4 happens in a browser and the callback has to be pasted. With agent mode on (`SOCLI_AGENT=1`, or a detected AI agent) it exits with status 2 and `signing in to dev/null needs a person at a browser. Run socli login devnull in a terminal`. An agent can use a device that a person approved earlier, because the key and `config.toml` are plain files. The SSH login is not blocked in agent mode, so `socli auth devnull ssh` works in a script when `ssh-keygen` is available.

Back to [network overview](../README.md).
