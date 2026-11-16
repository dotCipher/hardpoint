# Where Hardpoint fits

**Current ecosystem review and implementation priorities:**
[ECOSYSTEM.md](ECOSYSTEM.md). The earlier notes below are not an exhaustive
market survey. In particular, status lights, physical agent approvals and
display automation already exist; none is a uniqueness claim for Hardpoint.

Research notes from November 2026: what already exists, what is missing, and
what Hardpoint should build so that it adds value instead of repeating it.
Sources are at the end. Where a claim is my reading rather than something a
source states, it says so.

## The short version

Controlling devices is a solved problem many times over. Deciding **whether
an agent should be allowed to, and what happens if it was wrong**, is not.
Hardpoint should stop writing device drivers and become the layer that sits
between any agent and the tools that already control hardware:

1. a shared catalog that says how risky each ability is, what goes wrong, and
   whether a person has checked it;
2. a local gatekeeper that enforces that catalog, with undo, keep-or-revert
   and approve-first built in;
3. thin adapters onto the projects that already talk to the hardware.

## What the evidence says

### Agents take irreversible actions, and approval is mostly missing

- A November 2026 study measured the official MCP registry. About 90% of
  destructive tools had no confirmation on the server side, 32 of 41 popular
  hosts and agent SDKs had no approval step by default, and 60.7% of
  tool-and-host pairs had no enforced human approval. In tests, low-cost
  models carried out an injected deletion in 479 of 960 runs. The authors'
  title is the finding: neither side confirms. [1]
- MCP's own risk vocabulary (`readOnlyHint`, `destructiveHint`,
  `idempotentHint`, `openWorldHint`) is, in MCP's words, hints and not
  contracts: a server can claim anything. [2][3]
- Clients do not reliably act on those hints. A request for Claude Code's
  permission engine to use them was closed as not planned in September 2026;
  it describes users choosing between approving every call and bypassing
  permissions altogether. [4]
- MCP has no way to say "this can be undone". A `reversibleHint` was proposed
  in March 2026 and the discussion was closed without adoption. [5]
- The failures are public: Replit's agent deleted a production database
  during a declared freeze (July 2025); Gemini CLI destroyed a user's files
  the same month; Claude Code in auto mode has deleted home directories. [6][7][8]

Hardware makes each of these worse, because there is no backup of a bricked
board and no "undo" for a cut power feed.

### Home Assistant controls the home, with one coarse switch for risk

- Home Assistant has shipped an MCP server since 2025.2. An agent gets
  "Device control" over whatever entities the owner has exposed, or full
  admin access. The only risk control is a read-only option. [9]
- Exposure is all-or-nothing per entity. Its own guidance is to expose as
  little as possible. One owner found 69 network-switch ports exposed to the
  assistant after adding a switch; another unexposed more than 200 entities. [10]
- Owners work around it by hand: hiding the front-door lock with a
  third-party filter and marking every write tool "needs approval" in the
  client. [11]
- The incident already cited on the site happened on this stack: "turn off
  the office" also cut power to a server rack. [12]

Home Assistant should be wrapped, never rebuilt. What it lacks is a grade
between "exposed" and "not exposed".

### The tools that talk to hardware already exist

| Project | Covers | How to reach it | Licence |
|---|---|---|---|
| Home Assistant | the smart home | its MCP server or API | Apache 2.0 |
| fwupd and LVFS | firmware: 140+ vendors, about 145 million updates delivered [13] | D-Bus, or `fwupdmgr --json` | LGPL |
| OpenRGB | lighting on boards, memory, cards, peripherals | its network SDK | GPL-2 |
| libratbag | settings on gaming mice | its D-Bus daemon | MIT |
| Solaar | Logitech devices | command line | GPL-2 |
| ddcutil | monitors over DDC/CI | command line or library | GPL-2 |
| VIA and QMK | lighting and layers on hundreds of keyboards | VIA's protocol over HID | GPL |

Two things follow. Hardpoint can call these at run time without copying
their data, which keeps the catalog CC0. And none of them grades risk for an
agent: an MCP server for OpenRGB already exists and exposes every control
alike. [14]

OpenRGB is the clearest case for why grading matters. Its own README warns
that probing some boards has bricked the lighting or the whole board, and
users report memory modules killed by it. [15] "Set the lights" is
reversible on a USB keyboard and dangerous on a memory module's bus. That
difference belongs in a card.

### There is a standard to align with

W3C's Web of Things "Thing Description" already describes a device's
properties, actions and events, and marks each action `safe` and
`idempotent`. Research shows language models can plan from these
descriptions. [16][17] It has no notion of reversibility, of what goes
wrong, or of whether a claim was tested. Hardpoint should be able to export
a card as a Thing Description with its own terms added, rather than be
another closed format.

## What is Hardpoint's own value

Nothing I found combines these (my reading; the search was not exhaustive):

| | Exists elsewhere | Hardpoint |
|---|---|---|
| Describe what a device can do | Thing Descriptions, Home Assistant, VIA | reuse them |
| Read-only versus not | MCP hints, Home Assistant, Thing Descriptions | reuse, and map onto them |
| **Reversible versus not, with a floor nobody can lower** | proposed for MCP, not adopted | yes |
| **Enforced in the gatekeeper, not left as a hint** | rare, per the study above | yes |
| **Undo, keep-or-revert, approve-first for hardware** | individual tools offer subsets; not unique | combine under shared labels |
| **What goes wrong, in plain words** | device documentation and tool warnings | put it beside each ability |
| **Whether a person tested the claim, and where** | project/device compatibility reports | structured per-ability evidence |
| **Approval on a physical key** | agentsd, yolodeck and other control surfaces | optional surface; not a sandbox or an unspoofable boundary |

## Use cases worth building for

Each maps to a real incident or a real gap above.

1. **"Turn off the office."** Hardpoint in front of Home Assistant. Lights
   are reversible. The plug feeding the rack needs a yes first. This is the
   demonstration to build and publish: the same sentence, the same home,
   a different outcome.
2. **"Switch the screen to my other computer."** Through a display backend.
   Ask first: after switching, the old input may no longer accept commands,
   so an automatic revert cannot be relied on. Input switching already
   exists in display tools; the per-model label is Hardpoint's contribution.
3. **"Update the dock's firmware."** Through fwupd. Approve-first, showing
   the exact version and what could go wrong; the result is recorded.
4. **"Make everything blue."** Through OpenRGB. Applied to the keyboard and
   fans; the memory modules are skipped or need approval, per their cards.
5. **"Why is my mouse so slow?"** Through libratbag or Solaar. Reads freely;
   changing speed is keep-or-revert.
6. **Agent sessions on spare keys.** Already working. It is also the approval
   surface for everything above.

## What to change in the design

- **Class cards.** For Home Assistant, describe kinds of thing (an outlet, a
  lock, a light, a garage door), not thousands of models. About twenty cards
  cover most homes. Model cards refine them.
- **Stricter, never looser, per device.** The owner must be able to raise a
  label for one device ("this plug feeds the server"). Nobody can lower one
  below the vocabulary's floor.
- **`power.outlet` should be gated, not guarded.** Putting power back ten
  seconds later does not undo a crashed server. Keep-or-revert only fits
  changes that reverting actually repairs.
- **Fail closed.** The study found servers that ask for confirmation often
  go ahead when the host cannot show the question. If Hardpoint cannot get
  an answer, the answer is no.
- **Firmware as a field.** The same device behaves differently on different
  firmware. A card needs to say which firmware a claim holds for.
- **Replace the planned HomeKit, Cast and USB-DFU adapters** with
  `home-assistant` and `fwupd`. Keep `manual` for what only a person can do.
- **Map onto MCP's hints.** read is `readOnlyHint`; reversible is not
  destructive; guarded and gated are destructive. Hosts that honour hints
  then behave sensibly, and Hardpoint still enforces for those that do not.

## Earlier sequence (superseded)

The gatekeeper/MCP interface, macOS system/HID++/DDC adapters, watches, basic
rules, W3C export and host-helper setup now exist. Home Assistant and fwupd
remain planned. The current order is [ROADMAP.md](ROADMAP.md): strengthen
readiness, instance identity, ownership and recovery, then expand workflows.
Do not use these earlier research notes as a live implementation checklist.

A measure of success, borrowed from the study: run its injected-deletion
test against Hardpoint with a gated ability. The count of unapproved actions
should be zero.

## What Hardpoint should not build

Its own firmware writer, its own HomeKit, Cast or Matter clients, its own
Logitech protocol, pairing, or discovery of smart-home devices. Each exists,
is maintained by people who do only that, and is one adapter away.

## Sources

1. Yao et al., "When Neither Side Confirms: Destructive Tool Approval Across MCP Servers and Hosts", arXiv:2611.02527. https://arxiv.org/abs/2611.02527
2. MCP blog, "Tool Annotations as Risk Vocabulary: What Hints Can and Can't Do", March 2026. https://blog.modelcontextprotocol.io/posts/2026-03-16-tool-annotations
3. Stacklok, "Tool annotations are becoming the risk vocabulary for agentic systems". https://stacklok.com/blog/tool-annotations-are-becoming-the-risk-vocabulary-for-agentic-systems-that-matters-more-than-it-might-seem/
4. Claude Code issue 87452, closed as not planned. https://github.com/anthropics/claude-code/issues/87452
5. MCP discussion 2382, proposing `reversibleHint`. https://github.com/modelcontextprotocol/modelcontextprotocol/discussions/2382
6. Ars Technica, "Two major AI coding tools wiped out user data after making cascading mistakes". https://arstechnica.com/information-technology/2025/07/ai-coding-assistants-chase-phantoms-destroy-real-user-data
7. MIT AI Incident 1178, Gemini CLI. https://airisk.mit.edu/ai-incidents/1178-google-gemini-cli-reportedly-deletes-user-files-after
8. Claude Code issues 74539 and 84107. https://github.com/anthropics/claude-code/issues/74539
9. Home Assistant, Model Context Protocol Server. https://www.home-assistant.io/integrations/mcp_server
10. Home Assistant community, "WTH are all new entities exposed to Assist by default?". https://community.home-assistant.io/t/wth-are-all-new-entities-exposed-to-assist-by-default/803889
11. Home Assistant community, "How do you use AI with Home Assistant?". https://community.home-assistant.io/t/how-do-you-use-ai-with-home-assistant/1028637
12. Joe Karlsson, local voice AI with Home Assistant. https://www.joekarlsson.com/blog/local-voice-ai-home-assistant-gpu/
13. Techzine on LVFS, and the LVFS site. https://www.techzine.eu/news/infrastructure/141114/dell-and-lenovo-are-making-significant-investments-in-the-linux-firmware-platform-lvfs/ and https://fwupd.org/
14. openrgb-mcp. https://github.com/PurimDEVY/openrgb-mcp
15. OpenRGB README warning. https://gitlab.com/CalcProgrammer1/OpenRGB/-/commit/c4d096e60c165b2311a2be0d5f3cc72dc14bd2a3
16. W3C, Web of Things Thing Description. https://w3c.github.io/wot-thing-description
17. "Towards Improving Large Language Models' Planning Capabilities on WoT Thing Descriptions", CEUR-WS vol. 3749. https://ceur-ws.org/Vol-3749/akr3-06.pdf
