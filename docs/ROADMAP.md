# Useful device workflows, built on evidence

This is a prioritized design backlog, not a list of shipped features. Model
coverage and actual verification come from the catalog; live availability
must be checked on the owner's machine. Never commit personal inventories,
readings, session titles or chosen key assignments to this document.

## First: foundations that make the interesting workflows safe

| Priority | Work | Acceptance evidence |
|---|---|---|
| P0 | Operation-specific readiness | Distinguish readable, writable, permission-blocked, offline, unsupported and unverified. A read-only host selector must not appear writable; missing data must not look like a zero value. |
| P0 | Per-instance target identity | Keep route identifiers private; distinguish identical models; never default to the wrong monitor or peripheral. Test two devices and ambiguous selection without writes. |
| P0 | Setting ownership and conditional restore | One owner per ability; observe a manual override; do not undo over a later human adjustment. Overlapping rules settle deterministically and recover from a missing device. |
| P0 | Durable recovery and nonblocking I/O | Record a guarded change before its write; recover or clearly report interrupted work after restart. Slow DDC/HID/system calls cannot freeze approval timers or key rendering. No promise that a killed daemon can revert without a recovery design. |
| P1 | Rules/watches as a reliable service | Validate clock ranges; define sleep/wake, missed times, failures and cooldowns. Reload configuration atomically, refresh watch workers, and avoid log/retry storms. |
| P1 | Run without a keyboard | Peripheral readings and the MCP gatekeeper work headlessly; keyboards/control surfaces are optional UI plugins, not required to start. |
| P1 | Complete discovered model coverage | Add read-only cards for uncatalogued storage and readers using existing system reports. Match models precisely, return unsupported health data honestly, and keep private mount/route details local. |

These are bounded implementation batches with fixture tests first. A runtime
readiness report should disclose uncertainty rather than probe by changing a
device. Live writes need separate consent and a restoration plan.

## Workflows worth building

### Keyboard: an attention router, not another RGB effect

**Already present:** per-session keys, colors, key events, read watches,
agent-state rules and an owner-configured press action on the supported
keyboard. A configured action is not a universal native answer integration.

**Build next:** stable session affinity, a priority attention queue, a clear
summary of what each key answers, optional terminal focusing, and distinct
signals for agent questions versus Hardpoint device approvals. Coalesce low
priority health warnings so they do not evict urgent approval requests.

Start with the narrower, verifiable action: pressing a session key focuses
the correct terminal/session, without submitting an answer. Only then add
host-specific human response hooks with explicit request identity, queueing,
expiry and tests that one press cannot answer a different/newer request.

**Demo:** several agents work in parallel; the key for the one needing a
decision pulses. A device warning waits in the same attention queue without
pretending to be an agent permission. A key press affects only the selected
request, never an unseen approval. Configure the person's answer key explicitly.

### Display: a scoped focus preset that respects the existing owner

**Already present:** brightness read/change/undo through the current single
external-display backend; simple scheduled rules.

**Build next:** exact target routing, backend choice and ownership, then a
temporary focus/review preset that preserves a later manual adjustment.
Use BetterDisplay or Lunar when selected by the owner rather than compete
with its sensor/schedule loop. Those adapters do not exist yet.

**Demo:** a review session requests a bounded backlight setting and keyboard
attention cue. Ending the session restores only settings Hardpoint still
owns. Input switching remains gated, never a timed "safe trial."

### Mouse: task-scoped precision, through an owning daemon where possible

**Already present:** battery, DPI and wheel-mode paths for direct Bluetooth;
DPI trials are guarded and wheel mode is reversible. Permissions are local.

**Build next:** an OpenLogi JSON adapter with explicit owner selection, then
precision/review presets with supported-value discovery and a manual escape.
Do not expand direct HID++ remapping into a competing full mouse suite.

**Demo:** entering a deliberate precision task requests a guarded DPI change;
long-review mode requests free-spin. Ending the task restores owned values.
Do not infer an active app is consent to change pointing behavior. Host
switching and button diversion need their own explicit implementation/tests.

### Laptop and storage: a local preflight, not a failure oracle

**Already present:** battery health/cycles, coarse thermal state, storage's
own health verdict, firmware readings and simple threshold watches.

**Build next:** one concise preflight combining connection, power, thermal
state and backup-destination presence. Explain missing or unsupported SMART
data. Optional short local baselines could detect a changed condition;
history/retention are not implemented and must be opt-in.

**Demo:** before a long local build or backup, the agent gathers actual
readings and reports blockers. A warning can go to a spare key. It never
claims that SMART predicts a failure, starts a backup without authorization,
unmounts a disk, changes charging limits or kills a workload automatically.

### Dock and USB devices: connection context

**Already present:** model enumeration and the supported dock's firmware read.

**Build next:** a public device-containment vocabulary plus private topology
mapping and connection events. Distinguish missing peripheral, dock path and
unsupported capability without publishing ports/identities that identify a
particular installation.

**Demo:** explain why a display or backup target disappeared after undocking,
pause dependent rules and resume only after validating the exact target.
Do not promise cable diagnosis or firmware compatibility from a version alone.

### Headphones: availability-aware handoff and useful warnings

**Already present:** firmware reads and battery fields when macOS provides
them. A paired but disconnected device can legitimately yield no battery.

**Build next:** connected/active-output state and consistent per-part battery
representation, then explicitly selected output handoff through supported
OS interfaces. Audio route control and microphone control are not built.

**Demo:** before a call, explain which output is actually active and whether
earbuds/case can be read; surface a low usable-earbud warning on a key. Do
not connect Bluetooth, change microphones or switch outputs on assumption.

### Home-network cards: wait for a real integration and test environment

TV, media, climate, light, lock and outlet cards do not imply working control.
Build Home Assistant discovery/routing first when a test server is available.
Use its device integrations and automation engine. Firmware belongs behind
fwupd on Linux when test hardware is available. Power cuts, locks, firmware
and host/input switches must retain explicit per-action approval boundaries.

## Recommended first demonstration

Build **read-only workstation preflight + the keyboard attention queue** first.
It uses the widest existing coverage without introducing new device writes.
Pair it with the P0 readiness work so blocked permissions and disconnected
headphones are reported honestly. After identity/ownership/recovery work,
add the temporary display/mouse review preset. Do not implement all device
classes or install new controller applications in one unreviewed batch.
