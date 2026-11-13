# Hardpoint catalog

Hardware your AI can read and change, with the risk written down.

This repository is an open catalog of what devices expose to software: what
can be read, what can be changed, by which route, and how risky each change
is. Every entry says whether a person has actually tried it. A website and a
JSON feed are generated from it:

- Site: https://dotcipher.github.io/hardpoint-catalog/
- Feed: https://dotcipher.github.io/hardpoint-catalog/catalog.json

"Hardpoint" is a working name. A hardpoint is the reinforced, load-rated spot
on a vehicle where equipment is allowed to attach, which is what a device card
describes: the places software may attach to a device, and how much each can
safely carry.

Status: a first draft with four devices, all from one MacBook. See
[docs/DESIGN.md](docs/DESIGN.md) for the system this catalog is part of.

## Layout

```text
devices/<vendor>/<model>.toml   one card per device model
vocabulary/capabilities.toml    the shared words for what a device can do
adapters/adapters.toml          the routes a card may name
schema/device.schema.json       the card format, for editors and validators
tools/build.py                  validates everything and builds the site
SPEC.md                         the format rules
```

## Build

```sh
python3 tools/build.py          # validate, then write ./site
python3 tools/build.py --check  # validate only
```

It needs Python 3.11 or newer and nothing else. Open `site/index.html` to see
the result; `site/catalog.json` is the machine-readable copy.

## Adding a device

1. Copy an existing card to `devices/<vendor>/<model>.toml`.
2. Fill in how the device is recognised under `[[match]]`.
3. List each capability using a word from the vocabulary. If the word you need
   is missing, add it to `vocabulary/capabilities.toml` in the same change.
4. Be honest in `verification`. Use `tested` only if you exercised the
   capability through the named adapter. Use `observed` if you saw it work
   some other way, and `unverified` if you only read about it.
5. Run `python3 tools/build.py --check`.

Cards are data only. A card can name an adapter and give it typed parameters;
it can never carry a command or a script, and the validator rejects any field
it does not know.

## Licence

- **Catalog data** (`devices/`, `vocabulary/`, `adapters/`): CC0 1.0, in
  [LICENSE-DATA](LICENSE-DATA). Anyone can use it for anything, with no
  attribution required, so any tool can ship it.
- **Everything else** (`tools/`, `schema/`, documentation): Apache 2.0, in
  [LICENSE](LICENSE).

By contributing a card you agree to release it under CC0. Write cards from
your own observation and from documentation. Do not copy data files from
projects under licences that would not allow that, such as the GPL.
