# Working on Hardpoint

Hardpoint is a shared device catalog, a local Rust daemon/gatekeeper, and a
generated public site. Bring your own agent; reuse existing device tools.
Read `docs/ECOSYSTEM.md` before proposing another driver or automation engine.

## Start here

- Check `git status` before editing. Preserve unrelated work.
- `README.md`: current behavior and install/update paths.
- `docs/DESIGN.md`: implemented architecture and its limitations.
- `docs/SPEC.md`: cards, labels, verification, rules and helper recipes.
- `docs/ROADMAP.md`: prioritized functionality; proposals are not shipped features.
- `docs/DISCOVERY.md`: the site's agent/search discovery contract and tests.
- Do not create goals, loops or subagents unless explicitly requested.

## Source of truth

- `catalog/devices/<maker>/<model>.toml`: model facts and verification records.
- `catalog/vocabulary/`: capability meanings, label floors and terminology.
- `catalog/adapters/adapters.toml`: adapter declarations and helper recipes.
- `catalog/vocabulary/devices.json`: canonical maker aliases, category labels,
  icon choices and public identity scopes. Use `tools/naming.py` for identity
  normalization; preserve existing card ids and keep uncertain models explicit.
- `src/`: daemon, adapters, gatekeeper, MCP, rules, updates and services.
- `config/default.toml`: generic defaults, with no device, key or person selected.
- `site-src/content.toml`: site copy and discovery metadata.
- `tools/build.py`, `tools/discovery.py`: validators and static generation.
- `site/` is generated and ignored. Never patch it as the source of a change.
- Do not maintain device inventories, coverage counts, adapter/platform/helper
  matrices, per-model abilities, labels or verification snapshots in docs,
  prompts or site prose. The registry owns those facts. Link to its generated
  views, or derive examples at build time; use placeholders in authored
  templates. A registry change must not require a parallel documentation edit.
- Brand assets and the macOS icon come from the same generator; do not hand
  replace one with a different mark. Preserve the animations and fixed geometry.

## Safety and honesty

- A card is data, never executable code. Host helper recipes are embedded in
  the program; catalog updates and private cards cannot add install authority.
- Use the strictest label: card, vocabulary floor, owner's stricter override.
  Unknown abilities are gated. Rules never execute gated actions.
- MCP offers only `devices`, `read`, `propose`, `undo`, `changes`. Never add a
  model-callable keep/approve path. Human approval is for the exact action.
- Session-key presses run the owner's configured action. Do not advertise
  universal agent-question answering without a tested host integration;
  keep session actions separate from Hardpoint's device approval key.
- Hardpoint is not a sandbox against an agent with unrestricted local access.
- A catalog entry, an implemented adapter, a tested ability and a successful
  live operation are different facts. `reachable` is not proof of permissions,
  connection, success, write support or support for every ability on the adapter.
- Record `tested` only for the operation actually exercised through its adapter,
  on the recorded OS/firmware. A read does not verify a write or an entire card.
- Never publish serials, addresses, host names, private device names, routes,
  local configs, logs or session information. `local/` stays private.
- Helper setup is not firmware flashing. Never elevate, bootstrap package
  managers, grant OS permissions or install unrelated apps to bypass a blocker.

## Hardware work

- Default to fixtures and read-only inspection. Do not change live hardware
  just to test documentation or a page. Keep live observations out of git.
- Input/host switching, power cuts, firmware and security controls need explicit
  approval. Do not assume automatic revert can recover a disconnected device.
- Do not stop, replace or seize another app's device channel without consent.
- Consult registry notes and runtime results for routing limitations. Do not
  choose the first device when identity is ambiguous.
- Existing rules/undo do not yet handle every competing writer or survive a
  daemon crash. Treat ownership, manual overrides and durable recovery as work
  to implement, not guarantees to advertise.
- Do not restart an installed daemon or rebuild its app for a docs-only change.
  Program changes can require new macOS permission grants after ad-hoc signing.

## Check changes

```sh
cargo test --locked
python3 -m unittest discover -s tools -p 'test_*.py'
python3 tools/build.py --check
python3 tools/build.py
python3 tools/check_models.py   # requires jsonschema
git diff --check
```

`build.py` needs Python 3.11+. `observe.py` also supports macOS's Python 3.9.
On Linux, building HID support needs pkg-config and libudev development files.
Use a separate socket/config/state directory for an isolated dry daemon; never
point a test at the person's running service. Dry mode must not install helpers.

Site changes need link/metadata/Markdown/feed tests, plus browser checks at
desktop and mobile widths for overflow and animation-induced resizing. Test
without JavaScript too. The public catalog is not a live inventory or an MCP
HTTP endpoint. Search metadata and Markdown must not invent stronger claims
than the visible page or its verification records. `llms.txt` is navigation,
not an indexing/ranking promise. Respect project-subpath hosting.

Report separately: implemented, verified, blocked, and requiring a person's
decision. Include evidence and the next concrete step; never call a launched
job or a proposed integration complete.
