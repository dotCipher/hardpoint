# Where Hardpoint fits

Hardpoint is a shared device catalog and a local policy/verification layer
over existing device tools. Bring your own agent; keep the person's chosen
device applications in charge of their settings.

## Do not maintain another inventory

The registry is the source of device names, categories, matching rules,
abilities, labels, prerequisites, adapter status and verification. Read its
generated pages/feeds and the local daemon for availability. This document
contains no copied support matrix, coverage count or per-model status list.
Earlier research snapshots remain in version history, not a parallel source
of current product claims.

## Build on existing owners

Prefer an existing device application's documented interface to duplicating
its driver, pairing, discovery or automation engine. Do not silently stop or
replace that application. Integration research and primary references are in
[ECOSYSTEM.md](ECOSYSTEM.md); a research candidate is not a working adapter.

One owner per setting is the design target. Exact device selection, manual
overrides, reconnects and failures must be considered before adding compound
automation. A broad natural-language request is not consent for unrelated
changes, and restoring a value is not proof that an interruption was repaired.

## The useful contract

- A card is data, not a command or installer.
- Resolve the effective label from the registry and owner policy, not prose.
- Separate historical verification from a successful live operation.
- Read before proposing where the protocol allows it; report missing data
  rather than treating it as a zero or a success.
- Keep a person's answer tied to the exact pending action. Never reinterpret
  an attention signal as permission for another operation.
- Treat program authority, helper setup and device firmware as different
  boundaries. A new card must not add arbitrary executable authority.
- Keep private unit identities and runtime state local. Public descriptions
  and evidence should remain useful without identifying an installation.

## Interoperability and useful workflows

Use standard descriptions and transports where they fit. Generated models
and machine-readable discovery should express the same facts as the visible
registry, without claiming stronger support or guarantees.

Develop workflow proposals around a measurable user outcome, then resolve
their required abilities against the registry. Demonstrate refusals, missing
permissions, unavailable routes and recovery—not only the happy path. An
agent playbook, an owner-configured rule and an integration idea are distinct
things and should be labelled accordingly.

[ROADMAP.md](ROADMAP.md) describes design work and acceptance evidence.
[DESIGN.md](DESIGN.md) explains the architecture, and
[DISCOVERY.md](DISCOVERY.md) defines the public discovery contract. None is a
substitute for the device and adapter registry.
