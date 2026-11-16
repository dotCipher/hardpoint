# Agent and search discovery

## What is generated

The static build publishes the same public facts in focused forms:

| Resource | Purpose |
|---|---|
| `agents.html` / `agents.md` | A concise user-facing agent guide: local tools, approvals, setup, privacy and current limits |
| `llms.txt` | A small navigation index into guides, model cards, machine feeds and public technical documents |
| `<page>.md`, `devices/<model>.md` | Main content extracted from each generated HTML page, without navigation, SVGs or scripts |
| `catalog.json` | Full catalog, vocabulary, terms and adapter declarations |
| `search-index.json` | One record per card/ability, with summary, label, adapter status, verification and citation URLs |
| `models/*.tm.jsonld` | W3C Thing Models, not device control endpoints |
| `sitemap.xml` | Canonical human-readable pages, not every duplicate representation |
| `docs/*.md` | An explicit allowlist of public documentation, never a glob over local files |

Each HTML page has its own description, canonical URL, Open Graph URL,
Markdown alternate and link to the covering `llms.txt`. JSON-LD describes the
website/page, the source repository on the landing page, and the model a
device article is about. No fake ratings, offers, downloads, certifications
or aggregate "all devices supported" claims are generated.

`site-src/content.toml` supplies discovery copy, descriptions and the public
document allowlist. Model descriptions and verification come from `catalog/`.
`tools/build.py` and `tools/discovery.py` generate the output. Do not edit
`site/` or maintain a second, stronger pitch in a machine-only file.

## Evidence and availability

The search feed explicitly has scope `catalog-not-runtime-inventory`.
`adapter_status = implemented` means code exists, not that every capability
is readable/writable. `declared_platforms` is not a tested-platform list.
Use each verification record's platform/date/note for historical evidence,
and the installed daemon for current availability. A tested read is not
proof of a tested write. The public site contains no local inventory,
addresses, serial numbers, runtime values or agent-session data.

For a question such as "can this mouse change sensitivity?", start at
`llms.txt`, locate the capability in `search-index.json`, follow the card's
Markdown/evidence and then ask the local daemon. Each ability has a stable
HTML fragment (`#cap-<capability id>`) so a citation can point to the claim
rather than only a product page. Do not interpret a staged animation as a
runtime result, and do not try to call this static site as an MCP server.

## Hosting and crawlers

`HARDPOINT_SITE_URL` selects the canonical base URL; otherwise the build
derives a GitHub Pages project URL from the repository. Links must preserve
that project subpath. Without a known public origin, the build omits absolute
canonical/schema/sitemap claims rather than invent a deployment address.

`robots.txt` only controls crawling at the **origin root**. A file at
`/<project>/robots.txt` cannot change an origin's crawler policy. The build
therefore emits it only when the configured site is actually at `/`.
For a GitHub Pages project, check the host's root policy and submit the project
sitemap using the verified site's search tooling where appropriate. This
repository does not silently edit a different repository's root robots file.

`llms.txt` is a navigation proposal, not an access-control mechanism,
accepted instruction hierarchy, mandatory crawler standard or ranking boost.
Google's guidance says normal indexing and helpful, accessible content remain
the basis for AI Search eligibility; no special AI file or schema is required.
Search Console verification/submission requires the owner's account and is
not something a successful static build proves. No analytics are added here.

Sources:
- https://llmstxt.org/
- https://developers.google.com/search/docs/appearance/ai-features
- https://developers.google.com/search/docs/crawling-indexing/robots/intro
- https://www.sitemaps.org/protocol.html

## Validation

Run the build and Python test suite from the repository root:

```sh
python3 tools/build.py
python3 -m unittest discover -s tools -p 'test_*.py'
python3 tools/check_models.py
```

Tests check canonical/subpath handling, matching visible descriptions,
parseable JSON-LD, safe JSON serialization, Markdown extraction, feed evidence,
stable citation targets, sitemap coverage and local discovery links. The
generation is deterministic: no fake build-time `lastmod` or freshness badge.
Browser checks still need desktop/mobile geometry, no-JavaScript content,
copy buttons and the existing animation interactions.
