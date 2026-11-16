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

**Proposal:** stable session affinity, a priority attention queue, a clear
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

**Proposal:** exact target routing, backend choice and ownership, then a
temporary focus/review preset that preserves a later manual adjustment.
Use BetterDisplay or Lunar when selected by the owner rather than compete
with its sensor/schedule loop. Check the registry for available routes.

**Demo:** a review session requests a bounded backlight setting and keyboard
attention cue. Ending the session restores only settings Hardpoint still
owns. Use the selected card's effective label; never assume a disconnected
route can recover itself through a timer.

### Mouse: task-scoped precision, through an owning daemon where possible

**Proposal:** an owning application's API with explicit owner selection, then
precision/review presets with supported-value discovery and a manual escape.
Do not expand direct HID++ remapping into a competing full mouse suite.

**Demo:** entering a deliberate precision task proposes the selected card's
precision setting under its effective label. Ending the task restores owned values.
Do not infer an active app is consent to change pointing behavior. Host
switching and button diversion need their own explicit implementation/tests.

### Laptop and storage: a local preflight, not a failure oracle

**Proposal:** one concise preflight combining available connection, power, thermal
state and backup-destination presence. Explain missing or unsupported SMART
data. Optional short local baselines could detect a changed condition;
history/retention would need an explicit opt-in and retention policy.

**Demo:** before a long local build or backup, the agent gathers actual
readings and reports blockers. A warning can go to a spare key. It never
claims that SMART predicts a failure, starts a backup without authorization,
unmounts a disk, changes charging limits or kills a workload automatically.

### Dock and USB devices: connection context

**Proposal:** a public device-containment vocabulary plus private topology
mapping and connection events. Distinguish missing peripheral, dock path and
unsupported capability without publishing ports/identities that identify a
particular installation.

**Demo:** explain why a display or backup target disappeared after undocking,
pause dependent rules and resume only after validating the exact target.
Do not promise cable diagnosis or firmware compatibility from a version alone.

### Headphones: availability-aware handoff and useful warnings

**Proposal:** connected/active-output state and consistent per-part battery
representation, then explicitly selected output handoff through supported
OS interfaces, only where the registry and runtime expose the required abilities.

**Demo:** before a call, explain which output is actually active and whether
earbuds/case can be read; surface a low usable-earbud warning on a key. Do
not connect Bluetooth, change microphones or switch outputs on assumption.

### Network workflows: start from registry evidence and a test environment

Card presence does not imply working control. Choose an existing owning
system's documented integration, inspect its registry entry and verify it in
a test environment. Use that system's device integrations and automation
engine. Apply the effective label from each selected card; do not keep a
second risk or availability matrix in this roadmap.

## Recommended first demonstration

Build **read-only workstation preflight + the keyboard attention queue** first.
Choose the readings and attention surface from the registry and runtime;
do not assume a fixed set of devices or introduce new device writes.
Pair it with the P0 readiness work so blocked permissions and disconnected
headphones are reported honestly. After identity/ownership/recovery work,
add the temporary display/mouse review preset. Do not implement all device
classes or install new controller applications in one unreviewed batch.
