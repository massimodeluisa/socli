---
title: Terms
description: What you get when you use SoCLI, and what socli.sh does not store.
---

# Terms

SoCLI is a free open-source client for X, Bluesky, Threads, Mastodon, and dev/null. Massimo De Luisa maintains it from Italy. This page is the terms for the program and for socli.sh. It describes the program as it works on 25 October 2026.

The source is under the MIT license. You can use it, copy it, and change it. The program is provided as it is, with no warranty. X, Bluesky, Threads, Mastodon, and dev/null keep their own rules.

socli.sh does not keep an account, and it does not store a sign-in. The record stays on your computer, in the config file, or in the browser's local storage if you use the reader on the web. Delete that file, or the local storage key `socli.config`, and that copy is gone.

A Threads sign-in is the one request that reaches socli.sh. The site sends the authorization code to Meta and returns the token to your device. The code, the token, and the Threads user id are not written to a database and are not logged. SoCLI does not sell that request, and it does not use it for anything else.

The [privacy policy](https://socli.sh/privacy.html) describes sign-in. [Data deletion](https://socli.sh/data-deletion.html) describes how to remove the copy on your device.

Questions go to the [GitHub issue form](https://github.com/massimodeluisa/socli/issues/new).

This page took effect on 25 October 2026.
