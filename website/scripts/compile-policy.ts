/**
Writes the PolicyStack draft to the VitePress cache.
That draft is a review aid. website/privacy.md is the page that ships.
The renderer invents an essential-cookie row and transfer clauses this
project has not signed, so the draft must not be copied onto the site.
*/

import { mkdir } from "node:fs/promises"
import { dirname, join } from "node:path"
import { fileURLToPath } from "node:url"
import { compilePolicy } from "@policystack/renderers"
import policy from "../policystack.ts"

const root = dirname(fileURLToPath(import.meta.url))
const out = join(root, "../.vitepress/cache/policy-compiled.md")
const files = await compilePolicy(policy, "privacy", { formats: ["markdown"] })
const markdown = files[0]?.content
if (!markdown) throw new Error("PolicyStack returned no privacy markdown")
await mkdir(dirname(out), { recursive: true })
await Bun.write(
  out,
  "<!-- Review only. Do not publish this file. The site page is website/privacy.md. -->\n\n" +
    markdown,
)
console.log(out)
