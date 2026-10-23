/**
SoCLI privacy facts for PolicyStack.
The published page is website/privacy.md. This file is the compiler input.
PolicyStack is not a law firm. Do not send this config to PolicyStack Cloud.
*/

import { defineConfig, LegalBases, Voluntary } from "@policystack/sdk"

export default defineConfig({
  company: {
    name: "SoCLI",
    legalName: "Massimo De Luisa",
    address: "Italy",
    url: "https://socli.sh",
    // The compiler requires an email. The published pages use the GitHub issue form.
    contact: { email: "hello@socli.sh" },
  },
  effectiveDate: "2026-10-24",
  jurisdictions: ["eea", "uk"],
  data: {
    collected: {
      "Threads sign-in": ["Authorization code", "Threads user id"],
    },
    context: {
      "Threads sign-in": {
        purpose:
          "To finish a Threads sign-in you started. The code is exchanged for a token and the token is returned to your own SoCLI client.",
        lawfulBasis: LegalBases.Contract,
        retention: "Not stored. Kept in memory only for that request.",
        provision: Voluntary(
          "That Threads sign-in does not finish. X, Bluesky, Mastodon and dev/null still work without it.",
        ),
      },
    },
  },
  thirdParties: [
    {
      name: "Cloudflare",
      purpose: "Fronts socli.sh and runs the Threads token exchange.",
      policyUrl: "https://www.cloudflare.com/privacypolicy/",
    },
    {
      name: "Meta Platforms",
      purpose: "Receives the Threads authorization code and returns a Threads user token.",
      policyUrl: "https://www.facebook.com/privacy/policy/",
    },
  ],
  /*
  No cookies block and no trackingTechnologies. The renderer turns
  trackingTechnologies into an essential-cookie row with an empty lawful basis.
  The site does not set that cookie. Local storage is described in privacy.md.
  */
  automatedDecisionMaking: [],
})
