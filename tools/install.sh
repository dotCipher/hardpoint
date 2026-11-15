#!/bin/sh
# Build Hardpoint from this checkout and put it in place for the person
# running it. Nothing here needs sudo, and nothing touches a device.
#
#   sh tools/install.sh
#
# It does four things:
#   1. builds the program with cargo;
#   2. copies it to ~/.local/bin/hardpoint;
#   3. copies the catalog of device cards to ~/.local/share/hardpoint/catalog;
#   4. records which commit both came from, so `hardpoint update` knows.
#
# Running it again is safe: it rebuilds and replaces all three.
set -eu

here=$(cd "$(dirname "$0")/.." && pwd)
data="${XDG_DATA_HOME:-$HOME/.local/share}/hardpoint"
bin="${HARDPOINT_BIN:-$HOME/.local/bin}"

if ! command -v cargo >/dev/null 2>&1; then
    echo "Hardpoint is built with Rust, and cargo was not found. Install it from https://rustup.rs and run this again." >&2
    exit 1
fi

echo "Building Hardpoint (a few minutes the first time)..."
(cd "$here" && cargo build --release --locked)

mkdir -p "$bin" "$data"
cp "$here/target/release/hardpoint" "$bin/hardpoint.new"
mv "$bin/hardpoint.new" "$bin/hardpoint"

# Put the catalog in place in one move, keeping the one before it.
rm -rf "$data/catalog.incoming" "$data/catalog.previous"
cp -R "$here/catalog" "$data/catalog.incoming"
if [ -d "$data/catalog" ]; then
    mv "$data/catalog" "$data/catalog.previous"
fi
mv "$data/catalog.incoming" "$data/catalog"

commit=$(git -C "$here" rev-parse HEAD 2>/dev/null || echo "")
printf '{\n  "catalog": "%s",\n  "program": "%s"\n}\n' "$commit" "$commit" > "$data/installed.json"

echo
echo "Installed:"
echo "  program  $bin/hardpoint"
echo "  catalog  $data/catalog"
case ":$PATH:" in
    *":$bin:"*) ;;
    *) echo "  note     $bin is not on your PATH yet" ;;
esac
echo
echo "Next: $bin/hardpoint inventory"
