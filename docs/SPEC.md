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
| `vendor`, `name` | As the maker writes them |
| `kind` | `keyboard`, `mouse`, `laptop`, `desktop`, `display`, `dock`, `audio`, `cooling`, `lighting`, `storage`, `television`, `media`, `speaker`, `plug`, `climate`, `hub`, `sensor`, `lock`, `camera` or `other` |
| `summary` | One sentence |
| `links` | Optional `https` links |

### `[[match]]`

How software recognises the device. At least one. `bus` is `usb`,
`bluetooth`, `builtin`, `display`, `audio` or `network`. The other fields
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
permission or a program that has to be closed. `platform` is `any`, `macos`,
`linux` or `windows`.

### `[[capability]]`

| Field | Meaning |
|---|---|
| `id` | A word from `catalog/vocabulary/capabilities.toml` |
| `adapter` | A name from `catalog/adapters/adapters.toml` |
| `risk` | `read`, `reversible`, `guarded` or `gated` |
| `privacy` | Optional: `open`, `aggregate` or `presence` |
| `params` | Optional. Only parameters the adapter declares |
| `range`, `step`, `choices` | Optional limits, for controls only |
| `note` | Optional. Anything a person should know |
| `firmware` | Optional. The firmware the device must be running for this claim to hold: `family`, and optionally `min` and `max` versions |
| `verification` | One or more records, described below |

### `verification`

| Field | Meaning |
|---|---|
| `status` | `tested`, `observed`, `imported` or `unverified` |
| `platform` | `macos`, `linux` or `windows` |
| `os`, `firmware` | Versions it was checked on |
| `date` | The day it was checked. Required for `tested` and `observed` |
| `note` | What exactly was and was not checked |

### `[[sources]]`

Optional. Where the information came from: a `title` and an `https` `url`.

## The vocabulary

Each word in `catalog/vocabulary/capabilities.toml` has:

| Field | Meaning |
|---|---|
| `kind` | `reading`, `event`, `control` or `action` |
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

| Risk | Meaning | What the daemon does |
|---|---|---|
| `read` | Nothing changes | Reads the value |
| `reversible` | Takes effect at once, can be undone at once, cannot stop you using the machine | Applies it and records it |
| `guarded` | Could get in the way of using the machine | Applies it, then reverts unless you confirm within a few seconds |
| `gated` | Cannot be undone, or needs a physical step | Gets it ready and asks first; does nothing until a person approves that exact action |

A reading or an event is always `read`. The `manual` adapter, for steps only
a person can take, is only ever `gated`.

The owner of a device can make a label stricter for that device in their own
configuration. Nothing can make one looser: the gatekeeper takes the strictest
of the card's label, the vocabulary's floor and the owner's.

## Abilities that need no code

The `macos-system` adapter finds a named field in what macOS reports about a
device, so a card for something the system already knows about works without
any new code.

| `source` | What it reads | Example `field` |
|---|---|---|
| `power` | the computer's battery | `state_of_charge`, `is_charging`, `cycle_count`, `maximum_capacity` |
| `bluetooth` | a paired Bluetooth device | `firmware_version`, `battery_level` |
| `thunderbolt`, `usb`, `displays` | a device on that bus | `switch_version` |
| `storage` | a drive's own health verdict | `smart_status` |
| `thermal` | whether heat is limiting the computer | `warning_level` |
| `audio` | output volume, which can also be set | `output_volume` |

A `field` matches a name in the system's report whatever its spelling:
`firmware_version` finds `device_firmwareVersion`. If several parts report
it, such as each earbud and the case, they come back together. A field that
would identify one unit or one person, such as a serial number or an
address, is refused by the validator and by the adapter.

The owner's own cards live in `~/.config/hardpoint/cards`, laid out like the
catalog. They work at once, replace a catalog card with the same id, and are
never touched by an update. The vocabulary's floors apply to them too.

## Rules

A rule in the owner's settings is a standing instruction: "when this, set
that".

```toml
[[rule]]
at = "20:00"                     # once, when the clock gets there
device = "lg/ultrawide-76fe"
capability = "display.brightness"
value = 40

[[rule]]
while = "needs-you"              # for as long as any agent session is in this state
device = "zsa/moonlander-mk1"
capability = "light.global"
value = "#402000"
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
Only existing Homebrew formula installation is automatic today. Linux
system package managers are detected, but their commands are left for a
person. No privilege elevation, package-manager bootstrap, app installation,
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

| Status | Meaning |
|---|---|
| `tested` | Exercised through the named adapter on that platform. Only allowed when the adapter is implemented |
| `observed` | Seen working or present on a real device, but not through the adapter |
| `imported` | Taken from another project's data |
| `unverified` | Documented somewhere; nobody here has tried it |

## Privacy

| Privacy | Meaning |
|---|---|
| `open` | The value can be passed on as it is |
| `aggregate` | Only counts and coarse states may leave the adapter |
| `presence` | Only whether it is in use or present may leave the adapter |

Where the vocabulary sets a privacy level, a card must declare that level or
a stricter one.

## Open questions

- There is no way to say that one device contains another, such as a dock and
  its audio interface.
- Version ranges for firmware are free text.
