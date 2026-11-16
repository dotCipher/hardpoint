Please install Hardpoint on this computer for me: {repo}

Hardpoint lets you, my agent, read and change my devices through labelled cards, and asks me first where it matters. Work one step at a time and tell me what you are about to do before you do it.

Rules for this install:
- Never use sudo, and never change a setting on any of my devices.
- Where a step says "ask me", stop and wait for my answer.
- If a step fails, stop and show me the error. Do not work around it.

1. Check what is here. It needs macOS or Linux, git, and Rust (`cargo --version`). If Rust is missing, ask me before installing it from https://rustup.rs. On Linux it also needs pkg-config and the libudev development package.
2. Get the source: `git clone {repo} "${XDG_DATA_HOME:-$HOME/.local/share}/hardpoint/src"`. If that folder already exists, Hardpoint is installed: go to step 9 instead.
3. Read `tools/install.sh` in that folder and tell me in two sentences what it will do. Then run it from that folder: `sh tools/install.sh`. It builds the program and puts it at `~/.local/bin/hardpoint`.
4. If `~/.local/bin` is not on my PATH, tell me the line to add to my shell profile. Do not edit the profile yourself. Use the full path `~/.local/bin/hardpoint` in the steps below.
5. Run `hardpoint inventory` and show me which of my devices have a card. Run `hardpoint doctor` for a read-only report of helper tools and permission requirements, using the current adapter registry rather than a package/platform list in this prompt. Before starting the daemon, ask me: "Allow automatic setup of the host-side helper tools these cards need?" Explain the specific plan from that report, that installed tools are reused, and that setup never elevates, installs a package manager or changes device firmware. If no, set `[tools] auto_install = false` in `~/.config/hardpoint/config.toml`, preserving existing settings. If yes, run `hardpoint setup`, which installs only supported missing helpers. If it needs a person, report that without trying to elevate privileges. This is not permission to change any device setting.
6. Ask me: "Start Hardpoint whenever I log in?" If yes, run `hardpoint service install`. If no, tell me it is started with `hardpoint run`.
7. Ask me: "Let the catalog of device cards update itself once a day?" Explain that only the cards update by themselves, that an update can never make a label less strict without me, and that the program never updates on its own. If yes, put these two lines in `~/.config/hardpoint/config.toml` (create the file if it is not there; leave anything already in it alone), then run `hardpoint service restart` if the service is installed:

   [update]
   catalog = true

8. Connect yourself to it, so you can use it as a tool. The server is the command `hardpoint mcp`, given with its full path:
   - Claude Code: `claude mcp add --scope user hardpoint -- "$HOME/.local/bin/hardpoint" mcp`
   - Codex: `codex mcp add hardpoint -- "$HOME/.local/bin/hardpoint" mcp`
   - OpenCode: `opencode mcp add hardpoint --global -- "$HOME/.local/bin/hardpoint" mcp`
   - Any other agent: add a local (stdio) MCP server with that command. If a command above is refused, check that tool's own help rather than guessing.
9. To update later: `hardpoint update` brings the cards up to date and says whether the program has a newer version. Ask me before running `hardpoint update --program`, which rebuilds the program from source; then run `hardpoint service restart`.
10. Run `hardpoint doctor` again. Finish by telling me: what was installed and where (including helper packages); what still needs a person; whether it starts at login; whether the cards update themselves; how to remove Hardpoint (`hardpoint service uninstall`, then delete `~/.local/bin/hardpoint`, `~/.local/share/hardpoint` and `~/.config/hardpoint`); and that keeping or approving a change is mine to do, on a key I choose or with `hardpoint keep` and `hardpoint approve`. Helpers remain managed by their package manager; do not remove a package another app might use.
