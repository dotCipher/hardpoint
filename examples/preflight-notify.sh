#!/usr/bin/env sh
# Example Hardpoint fixed-key action: run a read-only workstation preflight
# through the running daemon and show a concise desktop notification.
# Never changes any device setting or assumes missing readings are healthy.

set -eu

bin="${HARDPOINT_BIN:-$HOME/.local/bin/hardpoint}"
if [ ! -x "$bin" ]; then
    bin="$(command -v hardpoint || true)"
fi
[ -n "$bin" ] || exit 1

report="$("$bin" preflight --json 2>/dev/null || true)"
[ -n "$report" ] || exit 1

summary=$(printf '%s' "$report" | python3 -c '
import json, sys
d = json.load(sys.stdin)
ok, unavail = 0, 0
highlights = []
for dev in d.get("devices", []):
    for r in dev.get("readings", []):
        if r.get("status") == "ok":
            ok += 1
            cap, val = r["capability"], r["value"]
            if cap in ("battery.level", "thermal.pressure", "storage.health"):
                highlights.append(f"{cap.split(\".\")[0]}={val}")
        else:
            unavail += 1
parts = [f"{ok} ok"]
if unavail:
    parts.append(f"{unavail} unavailable")
if highlights:
    parts.append(", ".join(highlights[:4]))
print(" · ".join(parts))
' 2>/dev/null || echo "Preflight complete")

if command -v osascript >/dev/null 2>&1; then
    osascript - "Hardpoint preflight" "$summary" <<'APPLESCRIPT' >/dev/null 2>&1 || true
on run argv
    display notification (item 2 of argv) with title (item 1 of argv)
end run
APPLESCRIPT
else
    printf '%s\n' "$summary"
fi
