<p align="center">
  <img src="assets/logo.svg" width="84" height="84" alt="The Hardpoint mark: a mounting plate with four fixing holes and one attach point, ringed in the four label colours">
</p>

<h1 align="center">Hardpoint</h1>

<p align="center"><b>Let your agent manage your devices. You stay in charge.</b></p>

<p align="center">Bring your own agent: Claude Code, Codex, OpenCode, or any LLM with a terminal.</p>

<p align="center">
  <a href="https://dotcipher.github.io/hardpoint/">Site</a> ·
  <a href="#what-it-is">What it is</a> ·
  <a href="#why-not-let-the-agent-work-it-out">Why not let the agent work it out?</a> ·
  <a href="#the-four-labels">The four labels</a> ·
  <a href="#install">Install</a> ·
  <a href="#add-a-device">Add a device</a> ·
  <a href="docs/DESIGN.md">Design</a> ·
  <a href="docs/SPEC.md">Card format</a> ·
  <a href="docs/ROADMAP.md">Roadmap</a>
</p>

<p align="center">
  <img src="assets/hero.svg" width="820" alt="A split keyboard on which three unused keys light up blue, pulse amber, flash white, turn green and turn red as agent sessions start, ask for an answer, finish and fail">
</p>

## What it is

Your agent already works in your files and your terminal. Hardpoint adds
your devices: a checked card for each one says what it can read, what it may
change, and what needs your yes first.

The name comes from engineering. A hardpoint is the reinforced, load-rated
spot on a vehicle where equipment is allowed to attach. A device's card is
the same idea: the places software may attach, each rated for what it can
safely carry.

| Part | What it does |
|---|---|
| **The catalog** | The source of truth for device models, matching, abilities, labels, requirements and verification. Data only; coverage is read from the registry rather than repeated here. |
| **The daemon** | Runs on your computer, enforces labels, and reaches hardware through known adapters or helper tools. Everything it knows about a device comes from that device's card. |
| **The site** | Generated from the catalog, with a machine-readable feed for any tool. |

A card works both ways. The first thing built on it lets your devices reach
you: agent sessions shown on keyboard keys you never use. Each session gets a
key, the colour is its state, and pressing the key runs an action of your own
for that session.

Hardpoint should sit **over existing tools, not replace them**. Display
automation, mouse configuration, agent-status lights and physical permission
buttons already exist. Its focus is a shared card and enforced labels around
the actions sent through it. See the [ecosystem review](docs/ECOSYSTEM.md) for
integration research, prior art and ownership principles. The registry, not
that document, records which integrations and devices are available.

## Why not let the agent work it out?

Claude Code, Codex or any LLM with a terminal can already try to control a
device: it searches, writes a command and runs it. This is what changes with
Hardpoint.

| | Claude Code, Codex or any LLM agent | The same agent, with Hardpoint |
|---|---|---|
| **Knowing your device** | Has never seen your exact device. It searches, then works from whatever it finds. | Reads a card written for that exact model. |
| **Knowing the risk** | Reading a battery and replacing firmware look the same to it: a command to run. | Every ability on a card has one of four labels, set before anyone asks. |
| **Proof** | Reports what it did. Nothing says whether that was ever checked on your model. | Each claim on a card says whether a person tested it on a real device, and links to the record. |
| **Touching the device** | Writes its own commands and runs them as you. | A card holds no commands. Hardpoint checks the label, then uses a known adapter or helper tool. |
| **When it goes wrong** | If a change locks you out, you are on your own. | A risky change is labelled to go back by itself unless you keep it. One that cannot be undone waits for your yes. |
| **What it learns** | Gone when the session ends. The next agent works it out again. | A card is checked once and shared. Everyone with that device gets it, in any agent. |

That table describes the approach, not a device-support promise. Check the
registry and your local runtime for the specific operation you need.

## The four labels

Every ability on a card gets one of four labels.

<p align="center">
  <img src="assets/ladder.svg" width="820" alt="The four risk levels: read, reversible, guarded and gated, each with its meaning">
</p>

| Label | What happens when an agent asks |
|---|---|
| `read` | It looks, and tells you. |
| `reversible` | It makes the change, and can put it back. |
| `guarded` | It makes the change, then asks you to keep it. If you do not answer, it goes back. |
| `gated` | It gets everything ready and asks first. Nothing happens until you say yes. |

A card cannot claim a lower risk than the shared vocabulary allows for that
capability, and anything `guarded` or `gated` has to say, in plain words, what
goes wrong if it is done carelessly.

## Find current coverage

Do not use this README as an inventory or support matrix. Read the
[device registry](https://dotcipher.github.io/hardpoint/devices.html) for
per-ability evidence and the
[adapter registry](https://dotcipher.github.io/hardpoint/adapters.html) for
routes, platforms, helper requirements and limitations. Both are generated
from `catalog/`, as are the machine-readable catalog and search index.

The public catalog is not a live inventory. Adapter implementation,
historical verification and current read/write availability are different
facts. See [current architecture](docs/DESIGN.md) for limits and the
[functionality roadmap](docs/ROADMAP.md) for the next useful workflows.

### For coding and search agents

- Repository contributors: read [AGENTS.md](AGENTS.md).
- Agent users: start with the [agent guide](https://dotcipher.github.io/hardpoint/agents.html)
  or [llms.txt](https://dotcipher.github.io/hardpoint/llms.txt).
- Every generated HTML page has a Markdown alternative. The public
  [capability index](https://dotcipher.github.io/hardpoint/search-index.json)
  links individual claims to their device page, verification and WoT model.
- [docs/DISCOVERY.md](docs/DISCOVERY.md) documents canonical URLs, structured
  metadata, crawler/subpath behavior and tests. These help navigation and
  citation; they are not an indexing or AI-ranking guarantee.

## Install

**With your agent:** open the [Install](https://dotcipher.github.io/hardpoint/install.html)
page, press Copy, and paste the prompt into Claude Code, Codex or OpenCode.
It checks what is needed, builds Hardpoint from source, connects itself to
it, and asks you before each choice that is yours.

**By hand:**

```sh
git clone https://github.com/dotCipher/hardpoint ~/.local/share/hardpoint/src
cd ~/.local/share/hardpoint/src
sh tools/install.sh           # builds it, puts it at ~/.local/bin/hardpoint
hardpoint service install     # start it now and at every login
hardpoint doctor              # read-only: helpers and permission requirements
hardpoint setup               # retry supported helper setup without sudo
hardpoint update              # later: bring the cards up to date
```

How it stays current:

- **Cards** can update themselves once a day if you turn that on
  (`[update] catalog = true`). They are data, never code.
- By itself the catalog can only stay as strict or get stricter. An update
  that would loosen any label waits until you run
  `hardpoint update --allow-looser` yourself.
- **The program** never updates on its own. `hardpoint update --program`
  rebuilds it from source when you ask.

A config file only has to say what differs from the
[built-in settings](config/default.toml).

### Helper tools, not device firmware

The daemon resolves helpers from the adapter registry for the current OS and
architecture, reusing installed executables where possible. Consult that
registry and `hardpoint doctor` for actual requirements rather than a package
list in this document. Disable automatic setup with:

```toml
[tools]
auto_install = false
```

Recipes live in `catalog/adapters/adapters.toml` and are **compiled into the
program**. A downloaded or private card cannot supply an executable installer;
a new recipe requires a program update. Setup never elevates, installs a
package manager or GUI app, upgrades an existing helper, grants permissions,
or flashes device firmware. A failed install is not retried every discovery
cycle; run `hardpoint setup` to retry, and inspect `hardpoint status`'s `setup`
field for the daemon's last report.

Unavailable backends or permissions are not solved by installing an unrelated
package. Follow the resolver's report, keep one owner per setting, and respect
the selected adapter's routing limitations.

## Try it from a checkout

It needs Rust to build the daemon, and Python 3.11 or newer to build the site.

```sh
cargo build --release

# What is attached, and which cards describe it
./target/release/hardpoint inventory

# No hardware needed: prints which keys would be lit
./target/release/hardpoint run --device dry --config <example-config-path>

# In another terminal, report an agent's state the way a hook or script would
./target/release/hardpoint emit --source my-tool --session 1 --state needs-you
./target/release/hardpoint status
```

Agent states are `idle`, `running`, `needs-you`, `done` and `failed`.

### Let an agent connect

With `hardpoint run` running, any agent that speaks the Model Context
Protocol can use the gatekeeper. The command to give it is `hardpoint mcp`:

```sh
claude mcp add hardpoint -- /path/to/hardpoint mcp     # Claude Code
codex mcp add hardpoint -- /path/to/hardpoint mcp      # Codex
```

The agent can list devices, read, propose a change and undo it. It cannot
keep or approve one: that is yours, on a key you choose (`[gate] key` in
your config) or with `hardpoint keep` and `hardpoint approve`.

To drive a real keyboard, copy an example from [`examples/`](examples) to
`~/.config/hardpoint/config.toml`, choose which keys are slots, and run
`hardpoint run`. Only one program can hold a keyboard's control channel, so
the vendor's own app has to be closed while Hardpoint runs.

Nothing about a device or a person is built into the program. The built-in
settings in [`config/default.toml`](config/default.toml) name no device and
no keys.

## Add a device

A card is one small file. Adding one needs no code and changes nothing on
your device.

**With Claude Code, Codex or any coding agent:** open the
[Add a device](https://dotcipher.github.io/hardpoint/add.html) page, press
Copy, and paste. The prompt has it list what is plugged in, ask which device
you want, and write the cards with you. Choose all uncatalogued devices, all
detected devices for review, or specific entries from a discovery snapshot.
Existing cards are reviewed rather than duplicated; publication stays your choice.

**By hand:**

```sh
python3 tools/observe.py             # what is plugged in, and what has no card yet
python3 tools/observe.py --draft 3   # start a card for one of them
python3 tools/build.py --check       # check it
```

Be exact about what was checked: `observed` is for something you ran and saw
work, `unverified` for something you only read about.
[CONTRIBUTING.md](CONTRIBUTING.md) has the rest, and
[docs/SPEC.md](docs/SPEC.md) the full rules.

## What keeps it in your hands

| | |
|---|---|
| **Cards are data** | A card names an adapter and gives it typed values. It can never carry a command, a script or a path, and the validator rejects any field it does not know. |
| **Risk has a floor** | A card cannot give an ability a lower label than the shared vocabulary allows. |
| **No undo, no surprise** | Anything that cannot be undone waits for your approval of that exact action, every time. A step that needs your hands, such as a button on the device, stays yours. |
| **Local control** | No Hardpoint account or cloud service. The daemon listens locally; updates and helper setup contact repositories. Readings sent to your agent follow that agent's privacy settings. |
| **Counts, not keystrokes** | To act on a press the daemon sees which key went down. It keeps per-key counts only if you turn that on, and never the order, the timing or the text. |

Arbitrary shell commands can come only from your own config file: the action
you choose to run when you press a slot. Helper setup uses fixed package-manager
arguments from the program's embedded registry, not commands in device cards.

## Layout

```text
catalog/        device cards, the vocabulary, the adapter list, the schema
src/            the daemon
config/         built-in settings
examples/       complete setups to copy
site-src/       the site's words, styles, scripts and heading typeface
tools/build.py  validates the catalog; builds the site, the feed and assets/
tools/observe.py  lists attached devices and drafts a card for one
docs/           the design and the card format
```

## Licence

- **Catalog data** in `catalog/`: CC0 1.0, in [LICENSE-DATA](LICENSE-DATA).
  No attribution is required, so any tool can ship it.
- **Everything else**: Apache 2.0, in [LICENSE](LICENSE).
- **The heading typeface** on the site is Space Grotesk, under the SIL Open
  Font License, in [site-src/fonts/OFL.txt](site-src/fonts/OFL.txt).

By contributing a card you agree to release it under CC0. Write cards from
your own observation and from documentation; do not copy data files from
projects whose licences would not allow that.

Product names belong to their owners. This project is not affiliated with them.
