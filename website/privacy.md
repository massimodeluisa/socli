---
title: Privacy policy
description: What SoCLI stores when you sign in, and the one Threads request that reaches socli.sh.
---

# Privacy policy

SoCLI is a free open-source client for X, Bluesky, Threads, Mastodon, and dev/null. Massimo De Luisa maintains it from Italy. This page says what happens to personal data when you sign in. It describes the program as it works on 24 October 2026. It is not a lawyer's advice.

## What stays with you

The terminal keeps a config file on the computer, usually `~/.config/socli/config.toml`. The file is mode 0600, in a directory that is mode 0700. The reader on the website keeps the same kind of record in that site's local storage, under the key `socli.config`. Either record can hold access tokens, account ids, and usernames for the networks you sign in to. SoCLI has no account server that receives those copies.

A script that runs on the same site can read `socli.config`. The pages do not load a third-party script. The theme choice is a separate local storage key, `socli-theme`.

Signing in to X from the terminal reads the session cookie already stored by Chrome, Brave, Edge, Arc, or Chromium on that computer, then copies it into the config. The cookie is not sent to socli.sh. Signing in to X on the website uses X's own approval page. The token from that approval stays in local storage. The website asks X for permission to read and post, to read the account, and to use bookmarks.

A Bluesky sign-in sends the handle and an app password to the service you name. That is `https://bsky.social` unless you set another. The app password is not written into the config. The session tokens are.

A Mastodon sign-in registers SoCLI as an application on the server you name and keeps that server's token in the config. The token is sent to that server.

dev/null keeps a device key. In the terminal, signing out forgets the account in the config and leaves the key file on disk. Revoke the device at [dev-null.sh/settings/devices](https://dev-null.sh/settings/devices). On the website, the key is in local storage, and signing out clears that record.

Signing out of X removes the tokens and the copied browser cookie from the config. It does not, by itself, revoke the session at X. Signing out of Threads clears the Threads section of the config. Signing out of Bluesky also asks Bluesky to delete the session. Signing out of Mastodon also asks that server to revoke the token.

## The request that reaches socli.sh

Threads is the exception. To finish a sign-in you started, the terminal or the website sends the authorization code and the redirect address to `https://socli.sh/threads/token`. A Cloudflare Worker exchanges that code with Meta and returns a token, its expiry, and the Threads user id to the same caller. The Worker does not write the code, the token, or the user id to a database, and it does not log them. The app secret stays on the Worker.

The redirect address has to be one already registered for that sign-in. A code that does not match is refused. The Threads token Meta returns lasts about 60 days, and then it lives only in your config until you sign out or it expires.

Meta's long-lived token request is a GET, which is the call Meta documents. The app secret is in that request's query string, so the URL can appear in Meta's logs and in Cloudflare's logs of the subrequest. The Worker adds no log line of its own.

This exchange runs because you asked SoCLI to sign you in to Threads. That is GDPR Article 6(1)(b), the steps you requested. You can use SoCLI without Threads. If you do not send a code, that sign-in does not finish.

SoCLI does not sell personal data. It does not use sign-in data for advertising. It does not run profiling, and it does not make an automated decision that produces a legal effect.

The code is sent to Meta through Cloudflare so the sign-in can finish. SoCLI does not keep a copy after the request. SoCLI has not signed separate transfer clauses with Meta or Cloudflare. Their own policies cover what they do with that request: [Cloudflare](https://www.cloudflare.com/privacypolicy/) and [Meta](https://www.facebook.com/privacy/policy/).

## Everyone else you talk to

X, Bluesky, the Mastodon server you chose, and dev-null.sh receive what your own client sends them when you read or post. SoCLI does not sit in the middle of those calls, except for the Threads exchange above.

## Cookies

SoCLI's pages do not set an analytics cookie or an advertising cookie. The theme and the reader config are local storage, not HTTP cookies. Cloudflare may set a security cookie on socli.sh because the domain is proxied there. That cookie is Cloudflare's.

## Children

SoCLI is not made for children. The project does not knowingly take personal data from anyone under 16. If you think a child used the Threads sign-in, use the [GitHub issue form](https://github.com/massimodeluisa/socli/issues/new).

## Deleting data

Sign out of the network, delete the config file, or clear `socli.config` in the browser for the site you used, socli.sh or web.socli.sh. The dev/null key file in the terminal remains until you delete it or revoke the device.

SoCLI does not keep a copy of Facebook or Threads data on the server. Instructions for removing the app, and the address Meta can call, are on the [data deletion](https://socli.sh/data-deletion.html) page. The [terms](https://socli.sh/terms.html) cover use of the program.

## Asking

You can ask what personal data this project holds, ask for a correction, or ask for deletion. For the server, the Threads exchange holds nothing once the request has finished. The copy on your device is yours to delete.

Questions go to the [GitHub issue form](https://github.com/massimodeluisa/socli/issues/new). Massimo De Luisa is the person responsible, in Italy. SoCLI has no data protection officer, and it publishes no separate postal address.

If you are in the EEA, you can complain to the authority where you live. Italy's authority is the [Garante per la protezione dei dati personali](https://www.garanteprivacy.it/). A list of EEA authorities is on the [European Data Protection Board's site](https://www.edpb.europa.eu/about-edpb/about-edpb/members_en). In the UK, you can complain to the [ICO](https://ico.org.uk/make-a-complaint/).

This page took effect on 24 October 2026.
