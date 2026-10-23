---
title: Data deletion
description: How to remove Threads and Facebook data from SoCLI, and the address Meta can call.
---

# Data deletion

SoCLI does not keep a copy of your Threads or Facebook data on socli.sh.

A Threads sign-in sends an authorization code to `https://socli.sh/threads/token`. The site exchanges it with Meta and returns the token to your device. The code, the token, and the Threads user id are not written to a database.

To remove the copy on your device:

- Sign out of Threads with `socli logout threads`, or sign out of every network with `socli logout all`.
- Or delete the config file, usually `~/.config/socli/config.toml`.
- On the website, sign out, or delete the local storage key `socli.config` for the site you used, socli.sh or web.socli.sh.

To disconnect SoCLI at Meta, remove it under Settings, then Apps and Websites, on Facebook or Threads. If you then ask Meta to delete the data, Meta can POST a signed request to `https://socli.sh/threads/delete`. `https://socli.sh/threads/deletion` answers the same request. SoCLI checks the signature with the app secret, does not store the user id, and replies with a confirmation code and this page. The code means the request arrived. There is no server record to erase, so there is nothing further to look up.

Questions go to the [GitHub issue form](https://github.com/massimodeluisa/socli/issues/new). The [privacy policy](https://socli.sh/privacy.html) describes the rest of sign-in. The [terms](https://socli.sh/terms.html) cover use of the program.
