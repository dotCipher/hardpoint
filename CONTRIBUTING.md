# Adding a device

A device's label is one small file, called a card. Adding one needs no code
and changes nothing on your device.

## The quick way

Open the [Add a device](https://dotcipher.github.io/hardpoint/add.html) page,
press **Copy**, and paste the prompt into the AI coding assistant you already
use. It looks at what is plugged in, asks which device you mean, and writes
the card with you. The prompt itself is in
[`site-src/add-device-prompt.md`](site-src/add-device-prompt.md).

## By hand

```sh
# What is plugged in, and what has no card yet
python3 tools/observe.py

# Start a card for one of them
python3 tools/observe.py --draft 3

# Check it
python3 tools/build.py --check
```

`observe.py` runs on the Python that ships with macOS, and on Linux. It only
reads. `build.py` needs Python 3.11 or newer; if you do not have it, open the
pull request anyway and the check runs there.

Then open a pull request with the new file.

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
