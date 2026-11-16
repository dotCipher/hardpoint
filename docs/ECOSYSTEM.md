# The ecosystem, and Hardpoint's place in it

Reviewed 2026-11-15. These are integration decisions based on the projects'
own documentation, not a claim that every integration is implemented or that
no other project offers similar safety features.

## Positioning

**Bring your own agent. Keep your existing device tools. Add a shared catalog
and a local policy/verification layer for the actions sent through Hardpoint.**

Neither changing brightness nor putting agent status on a key is new. The
useful combination is a portable description of an ability, evidence about
its behavior, enforced labels, read-back, an undo path, keep-or-revert, and
approval of an exact action. A physical button is an optional surface, not
a new security boundary. An agent with unrestricted shell/device access can
bypass Hardpoint; this is not a sandbox.

## What to reuse

| Area | Existing systems and their interfaces | Hardpoint's role |
|---|---|---|
| Display color and adaptation | f.lux, Night Shift; Lunar offers sensor/sun/clock modes, Shortcuts, CLI and an event stream [1][2] | Leave adaptation with its owner. Do not rebuild color science, ambient-light algorithms, HDR or display layout management. |
| Display control | BetterDisplay's CLI (Intel and Apple Silicon), Lunar CLI, MonitorControl; `m1ddc` on Apple Silicon; `ddcutil` on Linux; Twinkle Tray/ControlMyMonitor on Windows [2–6] | Wrap a selected backend with exact display selection, value bounds, read-back and labels. Current implementation: `m1ddc`/`ddcutil` only; only macOS hardware tested. |
| Logitech peripherals | OpenLogi has a versioned JSON automation API through its running agent; Solaar and libratbag serve Linux devices [7][8] | Prefer an owning daemon's API to adding more raw HID++ code. Keep the small existing Bluetooth adapter as a limited fallback, not a competing remapping suite. |
| Keyboard/RGB control | QMK/VIA, ZSA's published protocol, OpenRGB's SDK, SteelSeries GameSense and the Stream Deck SDK [9–12] | Translate state and enforce labels. Use SDKs/plugins when another application owns the channel; do not compete for exclusive HID access. |
| Agent status and answers | claude-busylight, agentsd on macOS, yolodeck on Windows already show sessions and/or offer physical approval [13–15] | Reuse hook/event conventions. A Hardpoint device approval and a Claude tool permission are different decisions; never conflate them or auto-answer one with the other. |
| General automation | Home Assistant automations, Node-RED, Hammerspoon and Shortcuts [16–19] | Keep `at`/`while` rules small for local coordination. Do not build a flow editor, another smart-home engine, or arbitrary scripts in shared cards. |
| Smart home | Home Assistant already discovers, pairs and controls devices, with an API and MCP server [16] | Put per-ability policy in front of an existing server. Never install a whole home server as a side effect of loading an outlet card. Planned, not implemented. |
| Firmware | fwupd/LVFS knows the update protocols, artifacts and device quirks [20] | Show the exact proposed update and require approval. Never treat device firmware as an automatic helper dependency. Planned, not implemented. |
| Descriptions and agent transport | W3C WoT Thing Models/Descriptions and MCP [21][22] | Export interoperable descriptions; use MCP as transport. Hints do not replace server-side enforcement. |

### Important corrections to the earlier pitch

- **Backlight control is not unique.** Lunar and BetterDisplay already do it,
  including automation. "Only Hardpoint reaches it" was wrong.
- **Physical agent approval is not unique.** agentsd and yolodeck already
  hold permission requests open for a button answer. Agent-status lighting
  exists on inexpensive busylights as well as keyboards/decks.
- **Revert is not a universal remedy.** Changing monitor input can remove
  the path needed to change it back; cutting power cannot undo a crashed
  server. Those should be gated, not marketed as a ten-second safe trial.
- **SMART is not a failure predictor.** An adapter can report a drive's own
  health verdict; it cannot promise a backup destination is safe or forecast
  when a drive will die.

## Ownership before automation

Two writers can fight even if neither opens a device exclusively: a sensor
loop may immediately overwrite an agent's brightness change. OpenLogi
explicitly warns about competing HID++ managers [8]; Hammerspoon's direct
Stream Deck module requires the vendor app to be closed [18].

The design to work toward is **one owner per setting**:

1. Prefer the user's chosen running application's documented API.
2. Otherwise offer a narrow direct helper for supported devices.
3. Do not silently quit, uninstall, reconfigure or replace the owner's app.
4. Refuse ambiguous targets. Keep private route IDs local; do not publish
   serial numbers or user-given names in a shared card.
5. Detect changed state before undoing so a later human adjustment is not
   overwritten. Overlapping rules need an ownership/lease model, not a stack
   of blind restores.

Items 1 and 5 are **design work still to do** for general third-party
backends. BetterDisplay, Lunar and OpenLogi are researched integration
targets, not working Hardpoint adapters. Current DDC code still assumes a
single/default monitor: do not advertise general multi-monitor safety until
exact routing is implemented and tested.

## Host tools: automatic setup without arbitrary installers

The helper is software on the computer, **not firmware on the device**.
For a matched, attached card, the program collects its adapters and consults
the trusted adapter registry **embedded at build time**:

- Apple Silicon macOS: `ddc-ci` needs `m1ddc`; install the Homebrew core
  formula using an existing Homebrew installation. Homebrew verifies its
  downloaded artifacts. Do not download a random executable or run a remote
  installation script. `m1ddc` explicitly does not support Intel [3][4].
- Linux: select `ddcutil` and the installed distribution package manager.
  System packages need a person; show the command rather than elevating.
  I2C permissions and the kernel module are separate setup, not something a
  package being present proves [5]. No Linux hardware has been tested here.
- Intel macOS/Windows: report that a backend is missing. BetterDisplay and
  Windows tools are viable candidates, but installing them does not make
  an unwritten adapter work.

New cards using an existing adapter need no install instructions or program
change. New executable recipes require a program rebuild/review. Catalog
auto-updates can select existing adapters but cannot add executable install
authority. This intentionally limits what a compromised feed can cause.

The daemon sets up helpers in the background on startup, card reload and
periodic attachment discovery. Existing executables are reused. Only one
attempt per helper per daemon lifetime; an explicit `hardpoint setup` retries.
`hardpoint doctor` never installs, and `[tools] auto_install = false` disables
installation. No sudo, package-manager bootstrap, app casks, permission
grants, firmware flashing or automatic helper upgrades. Removal of a card
does not uninstall software that another app may use.

## Build next, in order

1. Host-tool setup has now been exercised on Apple Silicon with Homebrew.
   Keep its Linux/Intel/Windows boundaries explicit. Follow
   [ROADMAP.md](ROADMAP.md) for current implementation priorities.
2. Add exact DDC target routing and a selected backend interface before
   growing monitor features. Prototype BetterDisplay/Lunar behind that
   interface with an explicit owner choice.
3. Prototype OpenLogi's JSON API using only its running agent. Avoid its
   diagnostic commands, some of which write hardware even when named `diag`.
4. Define ownership for overlapping rules and manual overrides; keep complex
   triggers in an existing automation system.
5. Build the Home Assistant adapter when test hardware is available; then
   fwupd on Linux. Keep firmware approvals separate from package setup.
6. Integrate Stream Deck/status-light surfaces through existing SDKs rather
   than claiming to invent physical agent interaction.

## Sources (primary documentation)

1. f.lux: https://justgetflux.com/
2. Lunar features, schedules, CLI and Shortcuts: https://lunar.fyi/
3. BetterDisplay CLI and exact selectors: https://betterdisplay.pro/guide/integration/cli-examples
4. m1ddc, supported hardware and CLI: https://github.com/waydabber/m1ddc ; Homebrew formula/architecture: https://formulae.brew.sh/formula/m1ddc
5. ddcutil packages and prerequisites: https://www.ddcutil.com/install/ ; https://www.ddcutil.com/config_steps/
6. Windows candidates: https://github.com/xanderfrangos/twinkle-tray ; https://www.nirsoft.net/utils/control_my_monitor.html
7. OpenLogi automation contract and diagnostic caveats: https://openlogi.org/docs/cli
8. OpenLogi access conflicts: https://openlogi.org/docs/troubleshooting ; Solaar: https://pwr-solaar.github.io/Solaar/ ; libratbag: https://github.com/libratbag/libratbag
9. QMK/VIA: https://docs.qmk.fm/ ; https://www.caniusevia.com/docs/specification
10. OpenRGB SDK: https://openrgb.org/sdk.html
11. SteelSeries GameSense: https://github.com/SteelSeries/gamesense-sdk
12. Stream Deck SDK: https://docs.elgato.com/streamdeck/sdk/introduction/getting-started/
13. claude-busylight: https://github.com/alexx-avdeev/claude-busylight
14. agentsd: https://github.com/paultyng/agentsd
15. yolodeck: https://github.com/cruftbox/yolodeck
16. Home Assistant automation and MCP: https://www.home-assistant.io/docs/automation/ ; https://www.home-assistant.io/integrations/mcp_server/
17. Node-RED: https://nodered.org/
18. Hammerspoon, Stream Deck and Shortcuts: https://www.hammerspoon.org/docs/hs.streamdeck.html ; https://www.hammerspoon.org/docs/hs.shortcuts.html
19. macOS Shortcuts command line: https://support.apple.com/guide/shortcuts-mac/run-shortcuts-from-the-command-line-apd455c82f02/mac
20. fwupd/LVFS: https://fwupd.org/ ; https://github.com/fwupd/fwupd
21. W3C WoT: https://www.w3.org/TR/wot-thing-description11/
22. MCP tool annotations: https://modelcontextprotocol.io/specification/2025-06-18/schema#toolannotations
