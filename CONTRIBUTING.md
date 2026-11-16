# Adding a device

A device's card is one small file. Adding one needs no code and changes
nothing on your device.

Coding agents should read [AGENTS.md](AGENTS.md) before changing the program
or generated site. [docs/DISCOVERY.md](docs/DISCOVERY.md) explains the public
agent/search files and how to keep their claims aligned with the catalog.

## The quick way

Open the [Add a device](https://dotcipher.github.io/hardpoint/add.html) page,
press **Copy**, and paste the prompt into Claude Code, Codex or whichever
coding agent you use. It takes a private discovery snapshot, checks the
registry and asks whether to work on all uncatalogued entries, all entries
for review, or selected numbers/ranges. It reuses existing cards and asks
before publication. The prompt itself is in
[`site-src/add-device-prompt.md`](site-src/add-device-prompt.md).

## By hand

```sh
# What is plugged in, and what has no card yet
python3 tools/observe.py

# Keep one private snapshot so selection numbers cannot shift during a rescan
python3 tools/observe.py --json > <private-snapshot-path>
python3 tools/observe.py --from-file <private-snapshot-path> --select missing --json
python3 tools/observe.py --from-file <private-snapshot-path> --select 1,3-5 --json

# Print a template for one selected uncatalogued entry; no files are written
python3 tools/observe.py --from-file <private-snapshot-path> --draft <original-number>

# Check it
python3 tools/build.py --check
```

`observe.py` runs on the Python that ships with macOS, and on Linux. It only
reads. It also finds things on your home network, such as a television or a
smart plug, by listening for a few seconds to what they announce; it sends
them nothing, and reports the model they announce, never an address or the
name you gave them. `--no-network` skips that. `build.py` needs Python 3.11 or newer; if you do not have it, open the
pull request anyway and the check runs there.

Then open a pull request with the new file.

## Try it on your own machine first

A card in `~/.config/hardpoint/cards/<maker>/<model>.toml` works at once,
and updates never touch it:

```sh
hardpoint reload                                # read the cards again
hardpoint read <device id> <ability id>         # does it return the value?
```

Use the current adapter registry and implementation to find an existing
route and its parameters; this guide does not maintain a parallel coverage
list. Where a suitable route exists, the card can reuse it. If a read returns the
value, you have exercised that read through the adapter and may record the
read as `tested`. Say explicitly if setting/undoing was not tested; never
promote an entire card, a write operation or another platform on that basis.

## Short on time?

[Open a device request](../../issues/new?template=device.yml) and paste what
`observe.py` printed. Someone else can write the card.

## What makes a good card

- **Say exactly what was checked.** `observed` means you ran something on a
  real device and saw it work; say what you ran, and give the date.
  `unverified` means you read about it; say where. `tested` is only for
  something exercised through an adapter that exists in the daemon.
- **Only read.** Do not change a setting on a device just to see whether it
  can be changed.
- **Use the shared words.** Capabilities come from
  [`catalog/vocabulary/capabilities.toml`](catalog/vocabulary/capabilities.toml).
  If the word you need is missing, add it in the same change.
- **Never go below the floor.** A card cannot claim a lower risk than the
  vocabulary allows for that capability.
- **A card is information only.** It can never contain a command, a script or
  a path. The check rejects any field it does not know.
- **Write it yourself.** Work from what you see and from the maker's
  documentation. Do not copy data files from other projects.

Nobody's name goes in a card. The commit that adds a record says who checked
it, and the site links each record to its commit.

The full rules are in [docs/SPEC.md](docs/SPEC.md). By contributing a card you
agree to release it under CC0, as in [LICENSE-DATA](LICENSE-DATA).
