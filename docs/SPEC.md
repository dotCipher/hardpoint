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
| `kind` | `keyboard`, `mouse`, `laptop`, `desktop`, `display`, `dock`, `audio`, `cooling`, `lighting`, `storage` or `other` |
| `summary` | One sentence |
| `links` | Optional `https` links |

### `[[match]]`

How software recognises the device. At least one. `bus` is `usb`,
`bluetooth`, `builtin`, `display` or `audio`. The other fields narrow it:
`vendor_id`, `product_id`, `usage_page`, `model` (for built-in hardware, the
computer's model identifier) and `name`.

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
| `ask` | Optional. Something a person would really say to an AI. Leave it out for abilities nobody asks for in everyday words |
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
| `gated` | Cannot be undone by software, or needs a physical step | Never does it; explains the steps |

A reading or an event is always `read`. A `gated` capability always uses the
`manual` adapter.

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
