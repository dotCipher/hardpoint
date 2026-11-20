#!/usr/bin/env sh
# Example Hardpoint `actions.agent_press` helper.
#
# Pressing a lit agent key focuses the matching tmux pane and raises your
# terminal window without injecting keystrokes or submitting an answer.
# Pressing a device-watch alert key posts a desktop notification with the
# alert summary.
#
# Environment variables provided by Hardpoint:
#   HARDPOINT_KEY, HARDPOINT_SOURCE, HARDPOINT_SESSION,
#   HARDPOINT_TITLE, HARDPOINT_DIRECTORY, HARDPOINT_STATE
#
# Optional: set HARDPOINT_TERMINAL="Alacritty" (or Ghostty, iTerm2, Terminal, WezTerm).

set -eu

notify() {
    # Pass text via argv so arbitrary titles cannot break AppleScript quoting.
    if command -v osascript >/dev/null 2>&1; then
        osascript - "$1" "$2" <<'APPLESCRIPT' >/dev/null 2>&1 || true
on run argv
    display notification (item 2 of argv) with title (item 1 of argv)
end run
APPLESCRIPT
    fi
}

source_name="${HARDPOINT_SOURCE:-hardpoint}"
title="${HARDPOINT_TITLE:-${HARDPOINT_SESSION:-session}}"
state="${HARDPOINT_STATE:-}"
dir="${HARDPOINT_DIRECTORY:-}"

if [ "$source_name" = "hardpoint" ] || [ "$state" = "alert" ]; then
    notify "Hardpoint alert" "$title"
    exit 0
fi

focused=""
if [ -n "$dir" ] && command -v tmux >/dev/null 2>&1; then
    # Prefer a pane in the exact directory running an agent/node/opencode process,
    # then any pane in that directory.
    pane=$(
        tmux list-panes -a -F '#{session_name}:#{window_index}.#{pane_index}	#{pane_current_command}	#{pane_current_path}' 2>/dev/null \
        | awk -F '\t' -v dir="$dir" '
            $3 == dir && ($2 ~ /opencode|claude|codex|node/) { print $1; found=1; exit }
            $3 == dir && !fallback { fallback=$1 }
            END { if (!found && fallback) print fallback }
        '
    )
    if [ -n "$pane" ]; then
        target_session="${pane%%:*}"
        target_window="${pane%.*}"
        client=$(tmux list-clients -F '#{client_tty}' 2>/dev/null | head -n 1 || true)
        if [ -n "$client" ]; then
            tmux switch-client -c "$client" -t "$target_session" 2>/dev/null || true
        fi
        tmux select-window -t "$target_window" 2>/dev/null || true
        tmux select-pane -t "$pane" 2>/dev/null || true
        focused="$pane"
    fi
fi

# Raise the configured terminal (or the first running common terminal on macOS).
if command -v osascript >/dev/null 2>&1; then
    term="${HARDPOINT_TERMINAL:-}"
    if [ -z "$term" ]; then
        for candidate in Alacritty Ghostty WezTerm iTerm2 Terminal; do
            if pgrep -xi "$candidate" >/dev/null 2>&1; then
                term="$candidate"
                break
            fi
        done
    fi
    if [ -n "$term" ]; then
        osascript - "$term" <<'APPLESCRIPT' >/dev/null 2>&1 || true
on run argv
    tell application (item 1 of argv) to activate
end run
APPLESCRIPT
    fi
fi

if [ -n "$focused" ]; then
    notify "Hardpoint · $state" "$title ($focused)"
else
    notify "Hardpoint · $state" "$title"
fi
