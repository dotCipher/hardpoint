# Card format, version 1 (draft)

A card describes one device model. It is a TOML file at
`catalog/devices/<vendor>/<model>.toml`. The machine-checkable form of these
rules is `catalog/schema/device.schema.json` plus the cross-checks in
`tools/build.py`. The meanings of the risk levels, verification statuses and
privacy levels are kept in `catalog/vocabulary/terms.toml`.

## Principles

1. **Cards are data.** A card names an adapter and gives it typed parameters.
   It never contains a command, a script or a path. Unknown fields are errors.
2. **Risk is declared, and has a floor.** Each capability states its risk. The
   vocabulary sets the lowest risk a card may claim for that capability.
3. **Verification is explicit.** Every capability says where it was checked,
   when, and how thoroughly. Unchecked entries are welcome but are labelled as
   such. A card names no person: the commit that adds a record says who
   checked it, and the site links each record to that commit.
4. **One vocabulary.** A capability id means the same thing on every device.

## Sections

### `card`

The format version. Always `1`.

### `[device]`

| Field | Meaning |
|---|---|
| `id` | `<vendor>/<model>`, lower case, matching the file path |
| `vendor`, `name` | Canonical maker plus the reviewed public product name, without repeating the maker prefix |
| `kind` | A category key from `catalog/vocabulary/devices.json` |
| `identity` | An identity scope from that registry; distinguish model, family and integration class evidence |
| `aliases` | Optional reviewed public model names/identifiers; never private device labels |
| `icon` | Optional visual variant allowed by the category registry |
| `summary` | One sentence |
| `links` | Optional `https` links |

Maker names/aliases, namespaces, category labels/icons and identity scopes
are defined once in `catalog/vocabulary/devices.json`. New categories do not
require a copied enum in the schema or a second scene-name list. The schema
checks structure; the validator resolves those semantic values against the
registry. A class-card namespace identifies its integration, not a physical
manufacturer. Family references must not imply an exact model.

Normalize reviewed public text with `tools/observe.py --normalize` before
adding it. The helper normalizes Unicode/spacing, removes trademark marks,
resolves maker/category aliases and detects known public model names. It
does not infer identity from a private discovery label, change files, add
support, or automatically certify research. Its result still requires review.
Use `--card-id` when correcting metadata to preserve existing references;
never rename stable ids simply because the display name improves. Use a
reviewed public `--model-code` to distinguish same-named variants and retain
it as a public alias; a serial number is never an acceptable discriminator.

### `[[match]]`

How software recognises the device. At least one. Allowed `bus` values and
matching fields come from the schema. The other fields
narrow it: `vendor_id`, `product_id`, `usage_page`, `model` (for built-in
hardware, the computer's model identifier) and `name`.

A **class card** describes a kind of thing rather than one model. Its match
has `bus = "home-assistant"` with a `domain`, and optionally a
`device_class`, as Home Assistant names them: every outlet, every light,
every lock. It gives all such devices the same labels until a card for the
exact model says more.

A device on the home network is recognised by what it announces there:
`service` is the kind of announcement, such as `_hap._tcp`, and `model` is the
model it names. A card never holds an address, a host name or the name
someone gave the device.

### `[keys]`

Optional, for keyboards. It says how key numbers, switch positions and lights
line up, so software can light the right key and recognise a press. Each list
has one entry per key, in key-number order.

| Field | Meaning |
|---|---|
| `numbering` | Whose key numbers these are, in words |
| `matrix` | `[row, column]` of each key's switch, as the keyboard reports presses |
| `leds` | The light under each key, numbered from 0 |
| `grid` | `[x, y]` cell of each key, for drawing the board |

No two keys may share a switch position, a light or a cell.

### `[[requires]]`

Optional. Something that must be true before the capabilities work, such as a
permission or a program that has to be closed. Allowed `platform` values come
from the schema; actual requirements come from the selected card/adapter.

### `[[capability]]`

| Field | Meaning |
|---|---|
| `id` | A word from `catalog/vocabulary/capabilities.toml` |
| `adapter` | A name from `catalog/adapters/adapters.toml` |
| `risk` | A label from the risk vocabulary, respecting the ability's floor |
| `privacy` | Optional privacy level from the shared terms |
| `params` | Optional. Only parameters the adapter declares |
| `range`, `step`, `choices` | Optional limits, for controls only |
| `note` | Optional. Anything a person should know |
| `firmware` | Optional. The firmware the device must be running for this claim to hold: `family`, and optionally `min` and `max` versions |
| `verification` | One or more records, described below |

### `verification`

| Field | Meaning |
|---|---|
| `status` | A verification status from the shared terms |
| `platform` | The OS key allowed by the schema for this record |
| `os`, `firmware` | Versions it was checked on |
| `date` | The day it was checked. Required for `tested` and `observed` |
| `note` | What exactly was and was not checked |

### `[[sources]]`

Optional. Where the information came from: a `title` and an `https` `url`.

## The vocabulary

Each word in `catalog/vocabulary/capabilities.toml` has:

| Field | Meaning |
|---|---|
| `kind` | An ability kind from the shared terms |
| `value` | The type of the value, with its unit |
| `summary` | The exact definition |
| `min_risk` | The lowest risk a card may claim for it |
| `privacy` | Optional. The least a card must promise |
| `ask` | Optional. Something a person would really say to an agent. Leave it out for abilities nobody asks for in everyday words |
| `example` | Required with `ask`: an illustrative answer. Not a reading |
| `example_before` | Optional. For something you can change, an illustrative value before |
| `swatch` | Optional. For colours, the colour the example names |
| `harm` | Required when `min_risk` is `guarded` or `gated`: what goes wrong, in plain words |

The site builds a scene for every device from the abilities that have an
`ask`, so a new card needs no site work.

`catalog/vocabulary/terms.toml` holds the catalog's own terms. Each risk level
has a `meaning` and a `does` (what happens when something asks for a change at
that level); each status, privacy level and kind has a `meaning`.

## Risk

Canonical label names and meanings live in
`catalog/vocabulary/terms.toml`; ability floors live in
`catalog/vocabulary/capabilities.toml`. The generated vocabulary/label views
render those values rather than maintaining a second definition table here.

A reading or an event is always `read`. The `manual` adapter, for steps only
a person can take, is only ever `gated`.

The owner of a device can make a label stricter for that device in their own
configuration. Nothing can make one looser: the gatekeeper takes the strictest
of the card's label, the vocabulary's floor and the owner's.

## Reusing existing routes

Consult the adapter registry and matching cards before writing code. Where
an adapter already exposes the needed system report or protocol operation,
a new card can name that route and its declared parameters. The supported
sources, fields, platforms and prerequisites belong to the registry and
adapter implementation, not a copied table here. The generated format page
selects its worked example directly from a registry card.

Fields that identify a unit or person must not be exposed or published.

The owner's own cards live in `~/.config/hardpoint/cards`, laid out like the
catalog. They work at once, replace a catalog card with the same id, and are
never touched by an update. The vocabulary's floors apply to them too.

## Rules

A rule in the owner's settings is a standing instruction: "when this, set
that".

```toml
[[rule]]
at = "20:00"                     # once, when the clock gets there
device = "<device id from the registry>"
capability = "<ability id from that card>"
value = "<value accepted by that ability>"

[[rule]]
while = "needs-you"              # for as long as any agent session is in this state
device = "<device id from the registry>"
capability = "<ability id from that card>"
value = "<value accepted by that ability>"
```

A rule is not a way round the labels. Its change goes through the gatekeeper
like any other: a reversible one is made, a guarded one is held until a
person keeps it, and a gated one is never made by a rule at all. A `while`
rule puts its change back when no session is in that state any more.

## Host helper requirements

An adapter may declare `[[adapter.<id>.tool]]` entries in
`catalog/adapters/adapters.toml`, with `platform`, Rust `arches` (empty for
any), `binary`, `packages` by supported package manager, and a prerequisite
`note`. Selectors must not overlap. Executable and package names are plain
names, never paths, command flags, URLs or third-party taps. Device cards
cannot declare installers.

The daemon uses the registry **embedded when it was built**, not mutable
recipes from a catalog download. Only attached/matched cards select tools;
shared requirements are deduplicated. Setup runs outside the device loop on
startup, reload and periodic attachment discovery. Existing executables are
reused. New helper recipes need a reviewed program update; a new card using
an existing adapter does not.

`[tools] auto_install = false` opts out. `hardpoint doctor` is read-only;
`hardpoint setup` explicitly retries setup (and respects the opt-out).
The daemon reports its last result under `hardpoint status` → `setup`.
Automatic-install eligibility and package-manager selection come from the
trusted resolver and registry. Follow its local report. No privilege
elevation, package-manager bootstrap, unrelated app installation,
firmware flashing, permission grants, or automatic helper upgrades.

## Other standards

**W3C Web of Things.** The site publishes every card as a Thing Model
(Thing Description 1.1), at `models/<maker>-<model>.tm.jsonld`. Readings and
controls become properties, with `readOnly` set for readings; actions carry
the standard's `safe` and `idempotent`; events are events. What the standard
has no word for is added under the prefix `hp`, defined on the site's
`ns.html`: the label, its floor, what goes wrong, privacy, the adapter, and
how far the claim was checked. `tools/check_models.py` validates every model
against the W3C's schema.

**Model Context Protocol.** `hardpoint mcp` offers an agent five tools. The
protocol's hints are set from what each tool can do, and the label is
enforced by the daemon whatever a host makes of the hints:

| Label | MCP hint on a change | What the daemon does |
|---|---|---|
| `read` | `readOnlyHint` | Reads |
| `reversible` | not destructive | Applies, and can undo |
| `guarded` | destructive | Applies, reverts unless a person keeps it |
| `gated` | destructive | Does nothing until a person approves |

An agent is given no tool to keep or approve a change.

**Public discovery.** The site also generates `llms.txt`, per-page Markdown,
`search-index.json`, canonical/schema metadata and a human-page sitemap.
The search index contains one record per card/ability, preserving label and
verification records and linking a stable `#cap-<id>` HTML citation. Its
scope is catalog descriptions, never runtime availability or private device
instances. See [DISCOVERY.md](DISCOVERY.md) for the complete contract.

## Verification status

Read the status vocabulary for canonical definitions. A record describes the
specific operation and context exercised; never promote a complete card or
another platform from a single result. The validator cross-checks these
records against adapter declarations and required evidence fields.

## Privacy

The privacy vocabulary defines what may leave an adapter. Refer to those
canonical definitions and the selected ability's declaration rather than a
parallel list in this document.

Where the vocabulary sets a privacy level, a card must declare that level or
a stricter one.

## Extensions

For additions such as containment or firmware constraints, inspect the current
schema and implementation, then propose the data/version change explicitly.
Design documents are not a second tracker of which registry features exist.
