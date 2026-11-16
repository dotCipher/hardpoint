# Current architecture

This repository holds the catalog, daemon and generated public site. This
document describes current code; proposed work is in [ROADMAP.md](ROADMAP.md).

```text
catalog                    daemon (on your computer)          clients
  device cards       ->      discovery                  <-      AI agents over MCP
  vocabulary                 adapters                           command line
  adapter registry           change protocol                    rules you write
                             journal and policy
```

- **The catalog** says what a device can do. It is data, shared and versioned.
- **The daemon** finds the devices that are present, reaches them through
  adapters or known helper tools, after enforcing the label.
- **Clients** ask the daemon. They never talk to a device directly.

The daemon has discovery, four hardware/system adapters, a gatekeeper, MCP,
agent-session slots, watches, and `at`/`while` rules. Model-specific routes
come from cards. It currently selects a supported keyboard at startup;
running without a supported keyboard is still an architecture improvement
to make, not a supported general headless workflow.

## Discovery

1. At start, on reload, and periodically (about once a minute), enumerate HID
   devices and what macOS reports about USB, Bluetooth, displays and the
   computer itself. Discovery is not yet a complete native hot-plug event API.
2. Each observation is matched against the `[[match]]` sections of the cards.
3. A match becomes an instance: the card plus local facts such as firmware.
4. Anything unmatched is listed as uncatalogued, with what was observed. That
   listing is the starting point for a new card.

`hardpoint inventory` can run independently of the daemon. Paired Bluetooth
devices can appear even when disconnected; a match is not proof that a
reading is available or a permission has been granted. The current matching
can collapse identical models; per-instance identity/routing needs work.

## Adapters

An adapter is code in the daemon that implements some of: read a value,
set a value, subscribe to events. The catalog registers each adapter's name,
platforms and parameter names; cards refer to them.

| Adapter | Scope | State |
|---|---|---|
| `oryx-hid` | Keyboard lights, key positions and layer reports | Implemented and exercised on macOS; not every advertised ability is writable |
| `hidpp` | Direct-Bluetooth Logitech battery, DPI, wheel mode and host reading | Implemented and exercised on macOS; Input Monitoring needed; host switching and button remapping writes are not implemented |
| `macos-system` | Field-driven power, firmware, thermal, storage and audio | Implemented on macOS; some values require connection or OS permissions |
| `ddc-ci` | Display controls via `m1ddc`/`ddcutil` | Apple Silicon path exercised; Linux path untested on hardware; currently single/default-display routing |

`home-assistant`, `fwupd` and network `mdns` adapter implementations remain
planned. BetterDisplay, Lunar and OpenLogi are integration candidates, not
working adapters. Adapter-level implementation does not prove every ability
on every card or every declared platform works.

Things macOS does not hand over without elevated access, and which are
therefore out of scope for now: individual temperatures, fan speeds, the
ambient light sensor and the keyboard backlight.

## Changing things

A client never writes to a device. It proposes a change, and the daemon:

1. Looks up the capability and checks that this build can change it. Current
   adapters validate the types/ranges they implement; a universal enforcement
   layer for every card `range`/`choices` constraint is still needed.
2. Takes the strictest label from the card, vocabulary floor and owner's
   `[[stricter]]` entries. Per-client authorization policies are not implemented.
3. For `reversible`: reads the current value, applies the change, records both.
4. For `guarded`: does the same, then waits for a confirmation. Without one
   within a few seconds it restores the previous value.
5. For `gated`: prepares the change and shows exactly what will happen.
   Nothing is sent to the device until a person approves that exact action;
   an approval is never remembered for next time. A step that needs hands,
   such as a button on the device, stays with the person, and the daemon
   says when. Where only a person can do the whole thing, the card names
   the `manual` adapter and the daemon explains the steps.

Every change is written to a journal: what changed, the previous value, and
what became of it. The journal lives in the daemon's memory for now and is
lost when it stops.

The label that applies is the strictest of three: the card's, the
vocabulary's floor, and any the owner has set for that device. If a person
cannot be asked, or does not answer, the answer is no.

A person answers by pressing a key on the keyboard that lights up for the
purpose, or by typing `hardpoint keep` or `hardpoint approve` in a terminal.
An agent is offered neither. Only a press that came from the keyboard itself
counts; one sent over the daemon's socket does not. This is not a sandbox:
an agent that can run commands as you could talk to the socket directly. It
protects against an agent that is wrong, or that has been misled through its
tools, not against one determined to get round it.

Routine changes can come from `at` (clock time) and `while` (any agent in a
state) rules. Other triggers and a compound workflow engine are not built.
Rules go through the gatekeeper and never make gated changes. A `while` rule
requests undo when its condition ends. Watches can raise a reading on a
session key when it crosses a threshold. Configuration is loaded at startup;
`reload` refreshes cards, not all running config/watch workers.

Some adapter writes still run synchronously in the device loop and can delay
lighting/approval handling. The in-memory journal does not recover a guarded
change after a process crash. Undo can overwrite an external human adjustment,
and overlapping rules need an ownership model. These are limits to fix before
advertising unattended multi-device scenes as robust or atomic.

## Staying current

An installed copy has two parts that change, and they are treated
differently. The catalog is data: it can update by itself, and by itself it
can only stay as strict or get stricter. Before a new catalog is taken it is
read by the running program, and every label is compared with the one in
use; if any would become less strict, the update waits for a person. The
program is code: it never updates by itself, and is rebuilt from source when
a person asks. Both come over git from the project's repository, so every
change is a commit that can be read.

Host helpers are separate from both device firmware and catalog updates.
`src/tools.rs` uses trusted recipes embedded from the adapter registry. It
sets up helpers off the device loop on discovery/reload, reuses installed
executables and attempts a missing helper once per daemon lifetime.
Existing Homebrew can install `m1ddc` on Apple Silicon. No elevation,
package-manager bootstrap, permission grants or firmware flashing occurs.
`doctor` is read-only; `setup` retries and respects the opt-out. The last
daemon report is in `status.setup`.

## Asking things

MCP exposes five operations over the same local request path:

| Tool | Answers |
|---|---|
| `devices` | Matched models, abilities, labels, evidence and advertised code-path reachability |
| `read` | An actual current value, or a concrete connection/permission/read error |
| `propose` | A change handled under its effective label |
| `undo` | Request restoration of a recorded earlier value |
| `changes` | The current daemon's in-memory change journal |

`reachable` is not a live permission probe or a separate read/write guarantee.
No reading-history tool or persistent measurement store exists yet.

Answers are small and labelled so that a small local model can use them.
Capabilities marked `aggregate` or `presence` only ever return counts or
coarse states.

## Interfaces

- A local socket with one JSON request per line (exists; only its owner can write to it).
- An MCP server over the same operations, so any agent and any model provider
  can connect, including local ones.
- A command-line tool for people.

Owner label overrides are local configuration and cannot lower vocabulary
floors. A general per-client authorization policy remains planned.

## Public discovery

`tools/build.py` generates HTML, per-page Markdown, catalog and capability
feeds, W3C Thing Models, canonical URLs, JSON-LD and a sitemap. `llms.txt`
links to concise guides and model evidence. These files contain public model
data, not local inventories or a public MCP endpoint. See
[DISCOVERY.md](DISCOVERY.md) for the contract and search limitations.

## Order of work

See [ROADMAP.md](ROADMAP.md). Identity, ownership, operation-specific readiness
and crash recovery come before broad unattended multi-device automation.
