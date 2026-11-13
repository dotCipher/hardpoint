# Card format, version 1 (draft)

A card describes one device model. It is a TOML file at
`devices/<vendor>/<model>.toml`. The machine-checkable form of these rules is
`schema/device.schema.json` plus the cross-checks in `tools/build.py`.

## Principles

1. **Cards are data.** A card names an adapter and gives it typed parameters.
   It never contains a command, a script or a path. Unknown fields are errors.
2. **Risk is declared, and has a floor.** Each capability states its risk. The
   vocabulary sets the lowest risk a card may claim for that capability.
3. **Verification is explicit.** Every capability says who checked it, where,
   and how thoroughly. Unchecked entries are welcome but are labelled as such.
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

### `[[requires]]`

Optional. Something that must be true before the capabilities work, such as a
permission or a program that has to be closed. `platform` is `any`, `macos`,
`linux` or `windows`.

### `[[capability]]`

| Field | Meaning |
|---|---|
| `id` | A word from `vocabulary/capabilities.toml` |
| `adapter` | A name from `adapters/adapters.toml` |
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
| `date`, `by` | Required for `tested` and `observed` |
| `note` | What exactly was and was not checked |

### `[[sources]]`

Optional. Where the information came from: a `title` and an `https` `url`.

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

- Per-device data that is larger than a capability list, such as a keyboard's
  key-to-light map, has no home yet.
- There is no way to say that one device contains another, such as a dock and
  its audio interface.
- Version ranges for firmware are free text.
