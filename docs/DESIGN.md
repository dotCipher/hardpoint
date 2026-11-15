# System design (draft)

This repository holds two of the three parts: the catalog and the daemon.

```text
catalog                    daemon (on your computer)          clients
  device cards       ->      discovery                  <-      AI agents over MCP
  vocabulary                 adapters                           command line
  adapter registry           change protocol                    rules you write
                             journal and policy
```

- **The catalog** says what a device can do. It is data, shared and versioned.
- **The daemon** finds the devices that are present, reaches them through
  adapters, and is the only thing that touches hardware.
- **Clients** ask the daemon. They never talk to a device directly.

The daemon already has discovery, the `oryx-hid` adapter, a rules engine
for keys and lights, and a local socket. It takes everything it knows about
a device from that device's card; nothing about a particular keyboard is
written in the code.

## Discovery

1. At start, and whenever a device comes or goes, each bus is enumerated: USB
   and Bluetooth HID, displays, audio, and the computer's own model.
2. Each observation is matched against the `[[match]]` sections of the cards.
3. A match becomes an instance: the card plus local facts such as firmware.
4. Anything unmatched is listed as uncatalogued, with what was observed. That
   listing is the starting point for a new card.

`hardpoint inventory` does this today for USB and Bluetooth input devices
and the computer itself. Displays and audio devices are not looked for yet.

## Adapters

An adapter is code in the daemon that implements some of: read a value,
set a value, subscribe to events. The catalog registers each adapter's name,
platforms and parameter names; cards refer to them.

On the first computer, a MacBook Pro:

| Device | Adapter | State |
|---|---|---|
| ZSA Moonlander | `oryx-hid` | Implemented. Lights, key positions and layer reports work |
| Logitech MX Master 3 | `hidpp` | Not written. macOS refuses to open the control channel until the program has Input Monitoring permission. With it, one-off queries read the battery level, DPI, host slots and the list of features. Nothing has been changed on the mouse yet |
| The MacBook itself | `macos-system` | Not written. Battery, storage health, thermal warning level and output volume were all read by hand without special permission |
| LG UltraWide | `ddc-ci` | Not written, and not tried on this display |
| Dock, camera, microphone, AirPods | none yet | Seen during enumeration; no card |

Things macOS does not hand over without elevated access, and which are
therefore out of scope for now: individual temperatures, fan speeds, the
ambient light sensor and the keyboard backlight.

## Changing things

A client never writes to a device. It proposes a change, and the daemon:

1. Checks the change against the card: the capability exists, the value has
   the right type and is within `range` or `choices`.
2. Checks policy: this client is allowed changes up to this risk.
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

Routine changes come from rules, not from a model deciding live: by app, by
computer, by time of day. An agent's job is to write and revise the rules.

## Staying current

An installed copy has two parts that change, and they are treated
differently. The catalog is data: it can update by itself, and by itself it
can only stay as strict or get stricter. Before a new catalog is taken it is
read by the running program, and every label is compared with the one in
use; if any would become less strict, the update waits for a person. The
program is code: it never updates by itself, and is rebuilt from source when
a person asks. Both come over git from the project's repository, so every
change is a commit that can be read.

## Asking things

Three read tools are enough to start:

| Tool | Answers |
|---|---|
| `inventory` | What is attached, and what each device can do |
| `read` | The current value of one capability, with its unit |
| `history` | How a reading has changed over a period |

Answers are small and labelled so that a small local model can use them.
Capabilities marked `aggregate` or `presence` only ever return counts or
coarse states.

## Interfaces

- A local socket with one JSON request per line (exists; only its owner can write to it).
- An MCP server over the same operations, so any agent and any model provider
  can connect, including local ones.
- A command-line tool for people.

Policy is a local file: for each client, the highest risk it may request and
any capabilities it may never touch. It is enforced in code, never in a prompt.

## Order of work

1. The catalog, with cards for the devices on the first computer. (Done.)
2. Discovery and `inventory` in the daemon, reading cards. (Done for input devices.)
3. `macos-system` readings, so "ask about the machine" works.
4. The change protocol, first for lights, which are already `reversible`.
5. `hidpp` for the mouse, once the permission question is settled.
6. The MCP server.
