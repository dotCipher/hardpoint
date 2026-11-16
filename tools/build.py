#!/usr/bin/env python3
"""Validate the catalog, then build the site, the JSON feed and the brand images.

Standard library only, so anyone can run it:

    python3 tools/build.py            # validate, write ./site and ./assets
    python3 tools/build.py --check    # validate only

Nothing a visitor reads is written in this file. Device facts come from
catalog/, the catalog's own terms from catalog/vocabulary/terms.toml, and the
words and demo scripts from site-src/content.toml.
"""
import datetime
import html
import json
import math
import os
import pathlib
import re
import shutil
import struct
import subprocess
import sys
import urllib.parse
import zlib

from discovery import canonical, markdown, origin_robots, script_json, sitemap

if sys.version_info < (3, 11):
    sys.exit(f"tools/build.py needs Python 3.11 or newer, and this is {sys.version.split()[0]}. "
             "A pull request is checked for you either way.")

import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
CATALOG = ROOT / "catalog"
SOURCE = ROOT / "site-src"
SITE = ROOT / "site"
ASSETS = ROOT / "assets"

# Brand colours. The site's stylesheet declares the same four risk colours.
INK, PANEL, LINE, TEXT, MUTED, DIM = "#0a0c0f", "#11151a", "#232b35", "#dde3ea", "#8792a2", "#5b6675"
PAPER = "#f6f4ef"  # the page in a light setting
FONT = "space-grotesk-latin.woff2"  # the heading typeface, kept in site-src/fonts with its licence
RISK_COLOR = {"read": "#00d060", "reversible": "#0090ff", "guarded": "#ffa000", "gated": "#ff3b3b"}
STATE_COLOR = {"running": RISK_COLOR["reversible"], "needs-you": RISK_COLOR["guarded"],
               "done": RISK_COLOR["read"], "failed": RISK_COLOR["gated"]}

e = html.escape


# ---------------------------------------------------------------- validation

def check_schema(schema, value, path, errors):
    """A small subset of JSON Schema: enough for catalog/schema/device.schema.json."""
    types = {"object": dict, "array": list, "string": str, "integer": int, "number": (int, float), "boolean": bool}
    expected = schema.get("type")
    if expected:
        ok = isinstance(value, types[expected]) and not (expected in ("integer", "number") and isinstance(value, bool))
        if not ok:
            errors.append(f"{path}: expected {expected}")
            return
    if "const" in schema and value != schema["const"]:
        errors.append(f"{path}: must be {schema['const']!r}")
    if "enum" in schema and value not in schema["enum"]:
        errors.append(f"{path}: {value!r} is not one of {', '.join(map(str, schema['enum']))}")
    if "pattern" in schema and isinstance(value, str) and not re.search(schema["pattern"], value):
        errors.append(f"{path}: {value!r} does not match {schema['pattern']}")
    if isinstance(value, dict):
        for key in schema.get("required", []):
            if key not in value:
                errors.append(f"{path}: missing {key!r}")
        properties = schema.get("properties", {})
        for key, item in value.items():
            if key in properties:
                check_schema(properties[key], item, f"{path}.{key}", errors)
            elif schema.get("additionalProperties") is False:
                errors.append(f"{path}: unknown field {key!r}")
    if isinstance(value, list):
        if len(value) < schema.get("minItems", 0):
            errors.append(f"{path}: needs at least {schema['minItems']} item(s)")
        if "maxItems" in schema and len(value) > schema["maxItems"]:
            errors.append(f"{path}: allows at most {schema['maxItems']} item(s)")
        if "items" in schema:
            for i, item in enumerate(value):
                check_schema(schema["items"], item, f"{path}[{i}]", errors)


def plain(value):
    """TOML dates become ISO strings so everything is plain JSON."""
    if isinstance(value, dict):
        return {k: plain(v) for k, v in value.items()}
    if isinstance(value, list):
        return [plain(v) for v in value]
    if isinstance(value, (datetime.date, datetime.datetime)):
        return value.isoformat()
    return value


def load_toml(path):
    with path.open("rb") as handle:
        return plain(tomllib.load(handle))


def check_adapter_tools(name, adapter, errors):
    """Recipes are data, but only the compiled copy is allowed to install.
    Reject command/URL fields, ambiguous selectors and package-manager options.
    """
    managers = {"macos": {"brew"}, "linux": {"apt-get", "dnf", "pacman", "zypper", "apk"}}
    selectors = set()
    for tool in adapter.get("tool", []):
        where = f"adapter {name}: tool"
        fields = {"platform", "arches", "binary", "packages", "note"}
        if not isinstance(tool, dict) or set(tool) != fields:
            errors.append(f"{where}: needs exactly {', '.join(sorted(fields))}; no commands, paths or URLs")
            continue
        os_name, arches = tool["platform"], tool["arches"]
        if os_name not in adapter.get("platforms", []) or os_name not in managers:
            errors.append(f"{where}: unsupported platform {os_name!r}")
        if not isinstance(arches, list) or any(a not in ("aarch64", "x86_64", "arm", "x86") for a in arches):
            errors.append(f"{where}: arches must be a list of Rust architecture names (empty means any)")
            continue
        if any(os_name == os and (not arches or not old or set(arches) & set(old)) for os, old in selectors):
            errors.append(f"{where}: overlapping OS/architecture selectors")
        selectors.add((os_name, tuple(arches)))
        if not isinstance(tool["binary"], str) or not re.fullmatch(r"[a-z0-9][a-z0-9._+-]*", tool["binary"]):
            errors.append(f"{where}: binary must be a plain executable name")
        packages = tool["packages"]
        if not isinstance(packages, dict) or not packages:
            errors.append(f"{where}: packages must name at least one supported package manager")
            continue
        for manager, package in packages.items():
            if manager not in managers.get(os_name, set()):
                errors.append(f"{where}: unsupported package manager {manager!r} for {os_name}")
            if not isinstance(package, str) or not re.fullmatch(r"[a-z0-9][a-z0-9._+-]*", package):
                errors.append(f"{where}: package must be a plain repository package name, not options, a tap or a URL")
        if not isinstance(tool["note"], str) or not tool["note"].strip():
            errors.append(f"{where}: explain prerequisites in note")


class Catalog:
    def __init__(self):
        self.errors = []
        terms = load_toml(CATALOG / "vocabulary" / "terms.toml")
        self.risks = {k: v["meaning"] for k, v in terms["risk"].items()}
        self.statuses = {k: v["meaning"] for k, v in terms["status"].items()}
        self.privacy = {k: v["meaning"] for k, v in terms["privacy"].items()}
        self.kinds = {k: v["meaning"] for k, v in terms["kind"].items()}
        self.terms = terms
        self.vocabulary = load_toml(CATALOG / "vocabulary" / "capabilities.toml")["capability"]
        self.adapters = load_toml(CATALOG / "adapters" / "adapters.toml")["adapter"]
        self.schema = json.loads((CATALOG / "schema" / "device.schema.json").read_text())
        self.cards = []
        self._check_terms()
        self._load_cards()

    def rank(self, kind, value):
        return list(getattr(self, kind)).index(value)

    def _check_terms(self):
        schema_risks = self.schema["properties"]["capability"]["items"]["properties"]["risk"]["enum"]
        if schema_risks != list(self.risks):
            self.errors.append("terms.toml and the schema list different risk levels")
        for cap_id, cap in self.vocabulary.items():
            where = f"vocabulary {cap_id}"
            if cap.get("kind") not in self.kinds:
                self.errors.append(f"{where}: kind must be one of {', '.join(self.kinds)}")
            if cap.get("min_risk") not in self.risks:
                self.errors.append(f"{where}: min_risk must be one of {', '.join(self.risks)}")
                continue
            changes = cap.get("kind") in ("control", "action")
            if not changes and cap["min_risk"] != "read":
                self.errors.append(f"{where}: a {cap.get('kind')} can only be 'read'")
            if changes and cap["min_risk"] == "read":
                self.errors.append(f"{where}: a {cap.get('kind')} changes something, so it cannot be 'read'")
            if "privacy" in cap and cap["privacy"] not in self.privacy:
                self.errors.append(f"{where}: privacy must be one of {', '.join(self.privacy)}")
            if cap.get("ask") and not cap.get("example"):
                self.errors.append(f"{where}: an 'ask' needs an 'example' answer to go with it")
            if self.rank("risks", cap["min_risk"]) >= self.rank("risks", "guarded") and not cap.get("harm"):
                self.errors.append(f"{where}: anything guarded or gated must say what goes wrong, in 'harm'")
            if "swatch" in cap and not re.fullmatch(r"#[0-9a-fA-F]{6}", cap["swatch"]):
                self.errors.append(f"{where}: swatch must be a colour like #ffa000")
        for group, fields in (("risk", ("meaning", "does")), ("status", ("meaning",))):
            for name, term in self.terms[group].items():
                for field in fields:
                    if not term.get(field):
                        self.errors.append(f"terms.toml: {group}.{name} needs {field!r}")
        for name, adapter in self.adapters.items():
            if adapter.get("status") not in ("implemented", "planned"):
                self.errors.append(f"adapter {name}: status must be implemented or planned")
            check_adapter_tools(name, adapter, self.errors)

    def _load_cards(self):
        for path in sorted((CATALOG / "devices").glob("*/*.toml")):
            rel = path.relative_to(ROOT)
            try:
                card = load_toml(path)
            except tomllib.TOMLDecodeError as exc:
                self.errors.append(f"{rel}: {exc}")
                continue
            found = []
            check_schema(self.schema, card, "card", found)
            self.errors.extend(f"{rel}: {problem}" for problem in found)
            if found:
                continue
            if card["device"]["id"] != f"{path.parent.name}/{path.stem}":
                self.errors.append(f"{rel}: device.id must be '{path.parent.name}/{path.stem}' to match its path")
            self._check_capabilities(card, rel)
            self._check_keys(card, rel)
            card["_path"] = str(rel)
            card["_source"] = path.read_text()
            card["_commits"] = record_commits(path, card["_source"])
            self.cards.append(card)

    def _check_capabilities(self, card, rel):
        seen = set()
        for cap in card["capability"]:
            where = f"{rel}: capability {cap['id']}"
            if cap["id"] in seen:
                self.errors.append(f"{where}: listed twice")
            seen.add(cap["id"])
            word = self.vocabulary.get(cap["id"])
            adapter = self.adapters.get(cap["adapter"])
            if word is None:
                self.errors.append(f"{where}: not in the vocabulary")
                continue
            if adapter is None:
                self.errors.append(f"{where}: unknown adapter {cap['adapter']!r}")
                continue
            if self.rank("risks", cap["risk"]) < self.rank("risks", word["min_risk"]):
                self.errors.append(f"{where}: risk {cap['risk']!r} is below the vocabulary minimum {word['min_risk']!r}")
            if word["kind"] in ("reading", "event") and cap["risk"] != "read":
                self.errors.append(f"{where}: a {word['kind']} can only be 'read'")
            if cap["adapter"] == "manual" and cap["risk"] != "gated":
                self.errors.append(f"{where}: the 'manual' adapter is for gated capabilities only")
            if "privacy" in word:
                if self.rank("privacy", cap.get("privacy", "open")) < self.rank("privacy", word["privacy"]):
                    self.errors.append(f"{where}: must declare privacy {word['privacy']!r} or stricter")
            for key in cap.get("params", {}):
                if key not in adapter.get("params", []):
                    self.errors.append(f"{where}: adapter {cap['adapter']!r} has no parameter {key!r}")
            # A card may not ask for anything that identifies one unit or one person.
            asked = str(cap.get("params", {}).get("field", "")).lower().replace("_", "")
            if any(word in asked for word in ("serial", "address", "uuid", "lot", "location")):
                self.errors.append(f"{where}: field {cap['params']['field']!r} would identify one particular unit, so a card may not ask for it")
            if ("range" in cap or "choices" in cap) and word["kind"] != "control":
                self.errors.append(f"{where}: only a control can have a range or choices")
            for record in cap["verification"]:
                if record["platform"] not in adapter.get("platforms", []):
                    self.errors.append(f"{where}: adapter {cap['adapter']!r} does not run on {record['platform']}")
                if record["status"] == "tested" and adapter.get("status") != "implemented":
                    self.errors.append(f"{where}: cannot be 'tested' through an adapter that is not implemented")
                if record["status"] in ("tested", "observed") and not record.get("date"):
                    self.errors.append(f"{where}: a {record['status']} record needs the date it was checked")

    def _check_keys(self, card, rel):
        keys = card.get("keys")
        if not keys:
            return
        count = len(keys["matrix"])
        if len(keys["leds"]) != count or len(keys["grid"]) != count:
            self.errors.append(f"{rel}: keys.matrix, keys.leds and keys.grid must be the same length")
            return
        for name, values in (("matrix", map(tuple, keys["matrix"])), ("leds", keys["leds"]), ("grid", map(tuple, keys["grid"]))):
            if len(set(values)) != count:
                self.errors.append(f"{rel}: keys.{name} has two keys in the same place")
        if any(not 0 <= led < count for led in keys["leds"]):
            self.errors.append(f"{rel}: keys.leds must number the lights 0 to {count - 1}")

    def card(self, device_id):
        return next((c for c in self.cards if c["device"]["id"] == device_id), None)

    def best_status(self, cap):
        return min((r["status"] for r in cap["verification"]), key=lambda s: self.rank("statuses", s))


def record_commits(path, source):
    """For each capability, the commit that last touched its verification record.

    The catalog does not store who checked something. The commit does, so the
    site links to it instead.
    """
    try:
        blame = subprocess.run(["git", "-C", str(ROOT), "blame", "--porcelain", "--", str(path)],
                               capture_output=True, text=True, timeout=20).stdout
    except (OSError, subprocess.SubprocessError):
        return {}
    by_line = {}
    for line in blame.splitlines():
        found = re.match(r"([0-9a-f]{40}) \d+ (\d+)", line)
        if found and set(found.group(1)) != {"0"}:
            by_line[int(found.group(2))] = found.group(1)
    commits, current = {}, None
    for number, line in enumerate(source.splitlines(), 1):
        named = re.match(r'id = "([a-z]+\.[a-z_]+)"', line)
        if named:
            current = named.group(1)
        elif line.startswith("verification") and current and number in by_line:
            commits[current] = by_line[number]
    return commits


# The ways the catalog page can be narrowed, and the orders it can be put in.
# content.toml chooses which to show and what to call them.
FACETS = ("kind", "maker", "can", "label", "checked", "bus", "platform")
SORTS = ("checked", "name", "abilities")


def load_content(catalog):
    """The words and demo scripts, checked against the catalog they describe."""
    content = load_toml(SOURCE / "content.toml")
    errors = catalog.errors
    for path in content["discovery"]["published_docs"]:
        allowed = path in ("README.md", "CONTRIBUTING.md", "AGENTS.md") or re.fullmatch(r"docs/[A-Z][A-Z_-]*\.md", path)
        file = ROOT / path
        if not allowed or not file.resolve().is_relative_to(ROOT) or not file.is_file():
            errors.append(f"content.toml: published_docs entry {path!r} must name an existing public Markdown document")
    for facet in content["catalog"]["facet"]:
        if facet["key"] not in FACETS:
            errors.append(f"content.toml: unknown catalog facet {facet['key']!r}; it can be {', '.join(FACETS)}")
    for order in content["catalog"]["sort_by"]:
        if order["key"] not in SORTS:
            errors.append(f"content.toml: unknown catalog order {order['key']!r}; it can be {', '.join(SORTS)}")
    demo = content["demo"]
    card = catalog.card(demo["card"])
    config_path = ROOT / demo["config"]
    if card is None or not card.get("keys"):
        errors.append(f"content.toml: demo.card {demo['card']!r} is not a card with a key layout")
    if not config_path.exists():
        errors.append(f"content.toml: demo.config {demo['config']!r} does not exist")
    else:
        slots = load_toml(config_path).get("slot", [])
        demo["slots"] = [{"key": s["key"], "pressable": bool(s.get("pressable"))} for s in slots if s.get("kind") == "agent"]
        if card and card.get("keys") and any(s["key"] >= len(card["keys"]["grid"]) for s in demo["slots"]):
            errors.append("content.toml: the demo config uses a key the card does not have")
    for step in demo["step"]:
        if "state" in step and step["state"] not in STATE_COLOR:
            errors.append(f"content.toml: unknown state {step['state']!r} in the demo")
    for item in content["ladder"]:
        target = catalog.card(item["device"])
        cap = next((c for c in (target or {}).get("capability", []) if c["id"] == item["capability"]), None)
        if cap is None:
            errors.append(f"content.toml: ladder example {item['device']} {item['capability']} is not in the catalog")
        elif cap["risk"] != item["risk"]:
            errors.append(f"content.toml: {item['device']} {item['capability']} is {cap['risk']!r}, not {item['risk']!r}")
        else:
            word = catalog.vocabulary[item["capability"]]
            item["device_name"] = f'{target["device"]["vendor"]} {target["device"]["name"]}'
            item["ask"] = word.get("ask") or word["summary"]
            item["before"] = word.get("example_before", "")
            item["after"] = word["example"]
    if [item["risk"] for item in content["ladder"]] != list(catalog.risks):
        errors.append("content.toml: the ladder needs one example per risk level, in order")
    return content


def site_url(repo):
    """Where the built site will be served from, when that can be known. Used for links that must be absolute."""
    explicit = os.environ.get("HARDPOINT_SITE_URL")
    if explicit:
        parsed = urllib.parse.urlsplit(explicit)
        if parsed.scheme not in ("http", "https") or not parsed.netloc or parsed.username or parsed.password or parsed.query or parsed.fragment:
            raise ValueError("HARDPOINT_SITE_URL must be an absolute HTTP(S) base URL without credentials, query or fragment")
        return explicit.rstrip("/") + "/"
    found = re.match(r"https://github\.com/([^/]+)/([^/]+)$", repo or "")
    return f"https://{found.group(1).lower()}.github.io/{found.group(2)}/" if found else None


def repo_url():
    """Where this copy of the project lives, worked out at build time."""
    explicit = os.environ.get("HARDPOINT_REPO_URL")
    if explicit:
        return explicit.rstrip("/")
    if os.environ.get("GITHUB_REPOSITORY"):
        return f'{os.environ.get("GITHUB_SERVER_URL", "https://github.com")}/{os.environ["GITHUB_REPOSITORY"]}'
    try:
        remote = subprocess.run(["git", "-C", str(ROOT), "remote", "get-url", "origin"],
                                capture_output=True, text=True, timeout=5).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return None
    found = re.match(r"(?:git@|ssh://git@|https://)([^/:]+)[/:](.+?)(?:\.git)?$", remote)
    return f"https://{found.group(1)}/{found.group(2)}" if found else None


# -------------------------------------------------------------------- images

# The mark: a mounting plate, four fixing holes, and one attach point whose
# ring runs through the four label colours. It is drawn the same way
# everywhere: page header, favicon, app icons and the README.
MARK_TILE, MARK_EDGE, MARK_HOLE = "#0d1116", "#2f3945", "#8d98a8"
# The four label colours as one band, in the order of the colour wheel so that
# every blend between neighbours stays vivid. The stylesheet's --band is the same.
MARK_STOPS = list(zip((0, 0.34, 0.68, 1), (RISK_COLOR[risk] for risk in ("reversible", "read", "guarded", "gated"))))


def mark(size=32, ring_class=""):
    stops = "".join(f'<stop offset="{at}" stop-color="{colour}"/>' for at, colour in MARK_STOPS)
    cls = f' class="{ring_class}"' if ring_class else ""
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="{size}" height="{size}" aria-hidden="true">'
        f'<defs><linearGradient id="hp-ring" x1="0" y1="0" x2="1" y2="1">{stops}</linearGradient></defs>'
        f'<rect x="1" y="1" width="30" height="30" rx="7.5" fill="{MARK_TILE}" stroke="{MARK_EDGE}"/>'
        + "".join(f'<circle cx="{x}" cy="{y}" r="1.5" fill="{MARK_HOLE}"/>' for x in (8.5, 23.5) for y in (8.5, 23.5))
        + f'<circle{cls} cx="16" cy="16" r="5.6" fill="none" stroke="url(#hp-ring)" stroke-width="3"/></svg>'
    )


def mark_png(size, bleed=False):
    """The same mark as a PNG, drawn pixel by pixel so the build needs no image library.

    `bleed` fills the whole square, for the icons a phone crops to its own shape.
    """
    rgb = lambda colour: tuple(int(colour[n:n + 2], 16) for n in (1, 3, 5))
    tile, edge, hole = rgb(MARK_TILE), rgb(MARK_EDGE), rgb(MARK_HOLE)
    stops = [(at, rgb(colour)) for at, colour in MARK_STOPS]
    scale = size / 32
    cover = lambda distance: min(1.0, max(0.0, 0.5 - distance * scale))  # how much of a pixel a shape covers
    mix = lambda a, b, t: tuple(a[n] + (b[n] - a[n]) * t for n in range(3))

    def ring_colour(x, y):
        t = min(1.0, max(0.0, (x + y - 20.8) / 22.4))
        for (a_at, a), (b_at, b) in zip(stops, stops[1:]):
            if t <= b_at:
                return mix(a, b, (t - a_at) / (b_at - a_at))
        return stops[-1][1]

    raw = bytearray()
    for py in range(size):
        raw.append(0)
        y = (py + 0.5) / scale
        for px in range(size):
            x = (px + 0.5) / scale
            colour, alpha = tile, 1.0
            if not bleed:
                qx, qy = abs(x - 16) - 7.5, abs(y - 16) - 7.5
                outside = math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - 7.5
                alpha = cover(outside - 0.5)
                colour = mix(colour, edge, cover(abs(outside) - 0.5))
            if 5 < x < 27 and 5 < y < 27:
                for cx in (8.5, 23.5):
                    for cy in (8.5, 23.5):
                        colour = mix(colour, hole, cover(math.hypot(x - cx, y - cy) - 1.5))
                colour = mix(colour, ring_colour(x, y), cover(abs(math.hypot(x - 16, y - 16) - 5.6) - 1.5))
            raw += bytes((round(colour[0]), round(colour[1]), round(colour[2]), round(alpha * 255)))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b""))


def icns():
    """The mark as a macOS icon file, so the program shows the same face in
    the system's permission lists as on the site. It is a plain container of
    PNGs, written directly."""
    kinds = (("icp4", 16), ("icp5", 32), ("ic07", 128), ("ic08", 256), ("ic09", 512), ("ic11", 32), ("ic12", 64), ("ic13", 256), ("ic14", 512))
    drawn = {size: mark_png(size) for size in sorted({size for _, size in kinds})}
    body = b"".join(kind.encode() + struct.pack(">I", len(drawn[size]) + 8) + drawn[size] for kind, size in kinds)
    return b"icns" + struct.pack(">I", len(body) + 8) + body


def ico(sizes):
    """A .ico file holding the mark at each size, for browsers that ask for one."""
    images = [mark_png(size) for size in sizes]
    offset, entries = 6 + 16 * len(images), b""
    for size, image in zip(sizes, images):
        entries += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(image), offset)
        offset += len(image)
    return struct.pack("<HHH", 0, 1, len(images)) + entries + b"".join(images)


UNIT, KEY, PAD = 46, 40, 16


def board_geometry(keys):
    """Pixel positions for each key, and a backing plate for each cluster of keys."""
    grid = keys["grid"]
    columns = sorted({x for x, _ in grid})
    # A missing column is the gap between the two halves of a split keyboard.
    breaks = [b for a, b in zip(columns, columns[1:]) if b - a > 1]
    groups = [[x for x in columns if x < breaks[0]], [x for x in columns if x >= breaks[0]]] if breaks else [columns]
    place = lambda x, y: (PAD + x * UNIT, PAD + y * UNIT)
    plates = []
    for group in groups:
        cells = [(x, y) for x, y in grid if x in group]
        x0, y0 = place(min(x for x, _ in cells), min(y for _, y in cells))
        x1, y1 = place(max(x for x, _ in cells), max(y for _, y in cells))
        plates.append((x0 - 9, y0 - 9, x1 - x0 + KEY + 18, y1 - y0 + KEY + 18))
    width = PAD * 2 + max(x for x, _ in grid) * UNIT + KEY
    height = PAD * 2 + max(y for _, y in grid) * UNIT + KEY
    return [place(x, y) for x, y in grid], plates, width, height


def board_svg(card, classes, slots=(), numbers=False, interactive=False, lamp=None):
    keys = card["keys"]
    places, plates, width, height = board_geometry(keys)
    slot_by_key = {s["key"]: s for s in slots}
    lamp_class = ' class="lamp"'
    parts = [f'<svg class="board {classes}" viewBox="0 0 {width} {height}" role="img" '
             f'aria-label="{e(card["device"]["name"])} key layout">']
    parts += [f'<rect class="half" x="{x}" y="{y}" width="{w}" height="{h}" rx="16"/>' for x, y, w, h in plates]
    for index, (x, y) in enumerate(places):
        slot = slot_by_key.get(index)
        names = ["key"] + (["slot"] if slot else []) + (["pressable"] if slot and slot["pressable"] else [])
        focus = ' tabindex="0"' if interactive or (slot and slot["pressable"]) else ""
        role = ' role="button"' if slot and slot["pressable"] else ""
        row, col = keys["matrix"][index]
        label = f'<text x="{x + KEY / 2}" y="{y + KEY / 2 + 4}">{index}</text>' if numbers else ""
        parts.append(
            f'<g class="{" ".join(names)}" data-key="{index}" data-row="{row}" data-col="{col}" '
            f'data-led="{keys["leds"][index]}"{focus}{role}><title>key {index}</title>'
            f'<rect{lamp_class if index == lamp else ""} x="{x}" y="{y}" width="{KEY}" height="{KEY}" rx="8"/>{label}</g>'
        )
    return "".join(parts) + "</svg>"


def hero_image(card, slots, content):
    """An animated picture for the README: the board, with three slots living through a session."""
    places, plates, width, height = board_geometry(card["keys"])
    blue, amber, green, red = (STATE_COLOR[s] for s in ("running", "needs-you", "done", "failed"))
    off = "#161c24"
    scripts = [
        f"0%,24%{{fill:{blue}}}25%,27%{{fill:{amber}}}31%{{fill:#5a3a00}}35%{{fill:{amber}}}39%{{fill:#5a3a00}}"
        f"43%{{fill:{amber}}}47%{{fill:#5a3a00}}51%,53%{{fill:{amber}}}54%,56%{{fill:#fff}}57%,92%{{fill:{blue}}}96%,100%{{fill:{off}}}",
        f"0%,9%{{fill:{off}}}10%,58%{{fill:{blue}}}59%,62%{{fill:{green}}}82%{{fill:#06351c}}86%,100%{{fill:{off}}}",
        f"0%,19%{{fill:{off}}}20%,64%{{fill:{blue}}}65%,84%{{fill:{red}}}85%,87%{{fill:#fff}}88%,100%{{fill:{off}}}",
    ]
    ox, oy, top = 40, 86, 40
    total_w, total_h = width + ox * 2, height + oy + 74
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {total_w} {total_h}" width="{total_w}" height="{total_h}" '
        f'role="img" aria-label="{e(content["demo"]["title"])}">',
        "<style>"
        + "".join(f"@keyframes k{i}{{{script}}}.s{i}{{animation:k{i} 14s linear infinite}}" for i, script in enumerate(scripts))
        + f"text{{font-family:system-ui,-apple-system,'Segoe UI',sans-serif}}.m{{font-family:ui-monospace,Menlo,Consolas,monospace}}"
        + "@media (prefers-reduced-motion:reduce){.s0,.s1,.s2{animation:none}}"
        + "</style>",
        f'<rect width="{total_w}" height="{total_h}" rx="18" fill="{INK}"/>',
        f'<rect x="1" y="1" width="{total_w - 2}" height="{total_h - 2}" rx="17" fill="none" stroke="{LINE}" stroke-width="2"/>',
    ]
    out += [f'<circle cx="{x}" cy="{y}" r="3" fill="{LINE}"/>' for x in (18, total_w - 18) for y in (18, total_h - 18)]
    out.append(f'<text class="m" x="{ox}" y="{top}" fill="{RISK_COLOR["guarded"]}" font-size="13" letter-spacing="2">'
               f'{e(content["eyebrow"].upper())}</text>')
    out.append(f'<text x="{ox}" y="{top + 28}" fill="{TEXT}" font-size="21" font-weight="650">{e(content["demo"]["title"])}</text>')
    out.append(f'<g transform="translate({ox},{oy})">')
    out += [f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="16" fill="#0d1116" stroke="{LINE}" stroke-width="1.5"/>'
            for x, y, w, h in plates]
    animated = {slot["key"]: n for n, slot in enumerate(slots[: len(scripts)])}
    slot_keys = {slot["key"] for slot in slots}
    for index, (x, y) in enumerate(places):
        cls = f' class="s{animated[index]}"' if index in animated else ""
        dash = ' stroke-dasharray="3 3"' if index in slot_keys and index not in animated else ""
        stroke = "#3b4654" if index in slot_keys else "#2a3340"
        out.append(f'<rect{cls} x="{x}" y="{y}" width="{KEY}" height="{KEY}" rx="8" fill="{off}" stroke="{stroke}" stroke-width="1.2"{dash}/>')
    out.append("</g>")
    legend_y = oy + height + 40
    x = ox
    for state, color in STATE_COLOR.items():
        label = state.replace("-", " ")
        out.append(f'<circle cx="{x + 6}" cy="{legend_y - 5}" r="6" fill="{color}"/>')
        out.append(f'<text x="{x + 20}" y="{legend_y}" fill="{MUTED}" font-size="15">{e(label)}</text>')
        x += 46 + len(label) * 9
    out.append("</svg>")
    return "".join(out)


def ladder_image(catalog):
    """The four risk levels as one picture, for the README."""
    row, width = 54, 1000
    height = 28 + row * len(catalog.risks)
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" height="{height}" role="img" '
           f'aria-label="The four risk levels">',
           "<style>text{font-family:system-ui,-apple-system,'Segoe UI',sans-serif}</style>",
           f'<rect width="{width}" height="{height}" rx="16" fill="{INK}"/>',
           f'<rect x="1" y="1" width="{width - 2}" height="{height - 2}" rx="15" fill="none" stroke="{LINE}" stroke-width="2"/>']
    for n, (risk, meaning) in enumerate(catalog.risks.items()):
        y = 14 + n * row
        color = RISK_COLOR[risk]
        if n:
            out.append(f'<line x1="24" x2="{width - 24}" y1="{y}" y2="{y}" stroke="{LINE}" stroke-dasharray="4 4"/>')
        out.append(f'<rect x="24" y="{y + 13}" width="118" height="28" rx="14" fill="none" stroke="{color}" stroke-width="1.6"/>')
        out.append(f'<text x="83" y="{y + 32}" fill="{color}" font-size="14" text-anchor="middle">{e(risk)}</text>')
        out.append(f'<text x="164" y="{y + 32}" fill="{TEXT}" font-size="14">{e(meaning)}</text>')
    out.append("</svg>")
    return "".join(out)


def write_assets(catalog, content):
    ASSETS.mkdir(exist_ok=True)
    card = catalog.card(content["demo"]["card"])
    files = {
        "logo.svg": mark(96),
        "hero.svg": hero_image(card, content["demo"]["slots"], content),
        "ladder.svg": ladder_image(catalog),
    }
    for name, body in files.items():
        (ASSETS / name).write_text(body + "\n")
    # The icon the program wears on a Mac is built into the program, so it lives with the packaging.
    (ROOT / "packaging").mkdir(exist_ok=True)
    (ROOT / "packaging" / "Hardpoint.icns").write_bytes(icns())
    return len(files)


# ------------------------------------------------- W3C Web of Things export

# Each card is also published as a W3C Web of Things Thing Model, the
# standard way to describe what a kind of device can do, so other tools can
# read the catalog without learning its own format. What the standard has no
# word for (the label, what goes wrong, how much was checked) is added under
# Hardpoint's own namespace, whose terms are listed here.
WOT_CONTEXT = "https://www.w3.org/2022/wot/td/v1.1"
WOT_TERMS = {
    "card": "The id of the Hardpoint card this model was made from.",
    "kind": "What kind of device it is.",
    "match": "How software recognises the device when it is attached.",
    "risk": "The label on this ability: read, reversible, guarded or gated.",
    "floor": "The lowest label the shared vocabulary allows this ability to have.",
    "harm": "What goes wrong if this is done carelessly.",
    "privacy": "The most that may leave the adapter: open, aggregate or presence.",
    "adapter": "The code that reaches this ability on the device.",
    "checked": "How far this claim has been checked: tested, observed, imported or unverified.",
    "verification": "The records behind that: platform, versions, date and what exactly was checked.",
    "firmware": "The firmware the device has to be running for this claim to hold.",
}
# How the vocabulary's plain-language value types map onto the standard's data types.
WOT_TYPES = {
    "percent": {"type": "number", "minimum": 0, "maximum": 100, "unit": "percent"},
    "boolean": {"type": "boolean"},
    "count": {"type": "integer", "minimum": 0},
    "string": {"type": "string"},
    "degrees": {"type": "number", "unit": "degree Celsius"},
    "dots per inch": {"type": "integer", "minimum": 0, "unit": "dots per inch"},
}


def thing_model(card, catalog, namespace, page=None):
    """One card as a Web of Things Thing Model (W3C Recommendation, version 1.1)."""
    d = card["device"]
    model = {
        "@context": [WOT_CONTEXT, {"hp": namespace}],
        "@type": "tm:ThingModel",
        "title": f'{d["vendor"]} {d["name"]}',
        "description": d["summary"],
        "hp:card": d["id"],
        "hp:kind": d["kind"],
        "hp:match": card["match"],
    }
    if page:
        model["links"] = [{"rel": "alternate", "href": page, "type": "text/html"}]
    groups = {"properties": {}, "actions": {}, "events": {}}
    for cap in card["capability"]:
        word = catalog.vocabulary[cap["id"]]
        entry = {"description": word["summary"]}
        typed = dict(WOT_TYPES.get(word["value"], {}))
        if "range" in cap and typed.get("type") in ("number", "integer"):
            typed["minimum"], typed["maximum"] = cap["range"]
        if word["kind"] in ("reading", "control"):
            entry.update(typed)
            entry["readOnly"] = word["kind"] == "reading"
            group = "properties"
        elif word["kind"] == "action":
            # The standard's two words for an action: does it change anything, and is repeating it harmless.
            entry.update({"safe": False, "idempotent": False})
            group = "actions"
        else:
            if typed:
                entry["data"] = typed
            group = "events"
        entry.update({"hp:risk": cap["risk"], "hp:floor": word["min_risk"], "hp:adapter": cap["adapter"],
                      "hp:checked": catalog.best_status(cap), "hp:verification": cap["verification"]})
        for term, value in (("harm", word.get("harm")), ("privacy", cap.get("privacy")), ("firmware", cap.get("firmware"))):
            if value:
                entry[f"hp:{term}"] = value
        groups[group][cap["id"]] = entry
    model.update({name: found for name, found in groups.items() if found})
    return model


# ---------------------------------------------------------------------- site

def chip(kind, value):
    return f'<span class="chip {kind}-{e(value)}">{e(value)}</span>'


def defs(pairs):
    return '<dl class="defs">' + "".join(f"<dt>{term}</dt><dd>{e(text)}</dd>" for term, text in pairs) + "</dl>"


def slug(card):
    return card["device"]["id"].replace("/", "-")


def hexid(value):
    return f"{value:04x}" if isinstance(value, int) else str(value)


def inline_code(text):
    """Escape text, turning `quoted` spans into code."""
    return re.sub(r"`([^`]+)`", r"<code>\1</code>", e(text))


# One family of pictures, all drawn on the same 24-unit grid with the same
# line. Each has two hooks: `glow` outlines light up while something is being
# asked, and one `lamp` shows the result. They are used small beside a
# device's name and large in the scenes.
G, T, L = 'class="glow"', 'class="glow thin"', 'class="lamp"'
ART = {
    "keyboard": f'<rect {G} x="2" y="6" width="20" height="12" rx="2.5"/><path {T} d="M6 10h.01M10 10h.01M14 10h.01M6 14h.01M9.5 14h5"/><circle {L} cx="18" cy="10" r="1.1"/>',
    "mouse": f'<rect {G} x="7" y="2" width="10" height="20" rx="5"/><path {T} d="M7 10.5h10"/><rect {L} x="11" y="4.8" width="2" height="3.8" rx="1"/>',
    "laptop": f'<rect {G} x="4" y="4.5" width="16" height="11" rx="2"/><path {G} d="M2 19.5h20"/><circle {L} cx="12" cy="7" r=".9"/>',
    "desktop": f'<rect {G} x="7" y="2.5" width="10" height="19" rx="2.5"/><path {T} d="M10 6.5h4M10 9.5h4"/><circle {L} cx="12" cy="17" r="1.2"/>',
    "display": f'<rect {G} x="2" y="4" width="20" height="13" rx="2.5"/><path {G} d="M12 17v3.5M8 20.5h8"/><circle {L} cx="19" cy="14" r=".9"/>',
    "television": f'<rect {G} x="2" y="6" width="20" height="12.5" rx="2.5"/><path {G} d="M7 21.5h10"/><path {T} d="M9 2.5l3 3 3-3"/><circle {L} cx="19" cy="15.5" r=".9"/>',
    "media": f'<circle {G} cx="12" cy="10" r="7"/><path {T} d="M10.5 7.6v4.8l3.8-2.4z"/><path {G} d="M12 17v2"/><circle {L} cx="12" cy="21" r="1.1"/>',
    "dock": f'<rect {G} x="3" y="9" width="18" height="7" rx="2.5"/><path {T} d="M7 12.5h2M11.5 12.5h2"/><circle {L} cx="17.5" cy="12.5" r="1"/>',
    "audio": f'<path {G} d="M4 14v-2a8 8 0 0 1 16 0v2"/><rect {G} x="2.5" y="13.5" width="4.5" height="7" rx="2"/><rect {G} x="17" y="13.5" width="4.5" height="7" rx="2"/><circle {L} cx="19.25" cy="18.4" r=".9"/>',
    "speaker": f'<rect {G} x="6" y="2" width="12" height="20" rx="3"/><circle {G} cx="12" cy="14" r="3.5"/><circle {L} cx="12" cy="6.5" r="1.2"/>',
    "storage": f'<rect {G} x="5" y="3" width="14" height="18" rx="2.5"/><path {T} d="M8.5 17h4.5"/><circle {L} cx="16" cy="17" r="1.1"/>',
    "lighting": f'<path {G} d="M12 2.5a6.5 6.5 0 0 0-4.2 11.4c.8.7 1.2 1.6 1.2 2.6h6c0-1 .4-1.9 1.2-2.6A6.5 6.5 0 0 0 12 2.5z"/><path {T} d="M9.5 19h5M10.5 21.5h3"/><circle {L} cx="12" cy="9" r="2"/>',
    "plug": f'<rect {G} x="5" y="4" width="14" height="13" rx="3.5"/><path {G} d="M10 9v3M14 9v3M12 17v4.5"/><circle {L} cx="12" cy="14.3" r=".9"/>',
    "climate": f'<rect {G} x="2.5" y="5" width="19" height="9" rx="2.5"/><path {T} d="M6 11h8M7 17.5v2.5M12 17.5v3.5M17 17.5v2.5"/><circle {L} cx="18" cy="8.6" r="1"/>',
    "cooling": f'<circle {G} cx="12" cy="12" r="9"/><path {T} d="M12 12c0-4 1-6 3-6s2 3-3 6c4 0 6 1 6 3s-3 2-6-3c0 4-1 6-3 6s-2-3 3-6c-4 0-6-1-6-3s3-2 6 3z"/><circle {L} cx="12" cy="12" r="1.1"/>',
    "hub": f'<rect {G} x="3" y="13" width="18" height="7" rx="2.5"/><path {T} d="M8.5 9.5a5 5 0 0 1 7 0M6 7a8.5 8.5 0 0 1 12 0"/><circle {L} cx="7.2" cy="16.5" r="1"/>',
    "sensor": f'<circle {G} cx="12" cy="12" r="3.2"/><path {T} d="M6.3 6.3a8 8 0 0 0 0 11.4M17.7 6.3a8 8 0 0 1 0 11.4"/><circle {L} cx="12" cy="12" r="1.1"/>',
    "lock": f'<rect {G} x="5" y="10.5" width="14" height="10.5" rx="2.5"/><path {G} d="M8 10.5v-3a4 4 0 0 1 8 0v3"/><circle {L} cx="12" cy="15.7" r="1.3"/>',
    "camera": f'<rect {G} x="3" y="6.5" width="13" height="11" rx="2.5"/><path {G} d="M16 10.5l5-2.5v8l-5-2.5"/><circle {L} cx="6.4" cy="9.8" r=".9"/>',
    "other": f'<rect {G} x="4" y="7" width="16" height="10" rx="2.5"/><path {T} d="M8 12h4"/><circle {L} cx="16" cy="12" r="1.1"/>',
    # the three steps a request passes through
    "agent": f'<rect {G} x="3" y="4.5" width="18" height="15" rx="2.5"/><path {G} d="M7.5 9.5l3 2.5-3 2.5M13 14.5h3.5"/>',
    "card": f'<rect {G} x="3" y="5" width="18" height="14" rx="2.5"/><path {T} d="M7 10h6M7 14h4"/><circle {L} cx="16.6" cy="13.6" r="1.3"/>',
    "device": f'<rect {G} x="6.5" y="6.5" width="11" height="11" rx="2"/><path {T} d="M10 3.5v3M14 3.5v3M10 17.5v3M14 17.5v3M3.5 10h3M3.5 14h3M17.5 10h3M17.5 14h3"/>',
}


def art(name, classes="ico", label=""):
    """One picture from the family. Small ones are decoration; a labelled one is an image."""
    role = f'role="img" aria-label="{e(label)}"' if label else 'aria-hidden="true"'
    return f'<svg class="{classes}" viewBox="0 0 24 24" {role}>{ART.get(name, ART["other"])}</svg>'


def drawing(card, label):
    """A picture of a device for the scenes: its own key layout if the card has one, or its kind."""
    if card.get("keys"):
        picture = board_svg(card, "drawing", lamp=len(card["keys"]["grid"]) // 2)
        return re.sub(r'aria-label="[^"]*"', f'aria-label="{e(label)}"', picture, count=1)
    return art(card["device"]["kind"], "drawing art", label)


def how_to_show(word):
    """Pick how to draw a value from what kind of value it is."""
    if word.get("swatch"):
        return "swatch"
    if word["example"].rstrip().endswith("%"):
        return "meter"
    return "pulse" if word["kind"] == "event" else "text"


class Site:
    def __init__(self, catalog, content, repo):
        self.c, self.t, self.repo = catalog, content, repo
        self.name = content["name"]
        self.risk = catalog.terms["risk"]
        self.home = site_url(repo)
        self.page_metadata = {}
        # Where Hardpoint's own terms are defined, for anything that reads the Web of Things models.
        self.namespace = f"{self.home}ns.html#" if self.home else "urn:hardpoint:ns#"
        # The devices with the most checked come first, wherever devices are listed.
        count = lambda card, status: sum(1 for cap in card["capability"] if catalog.best_status(cap) == status)
        self.cards = sorted(catalog.cards, key=lambda card: [-count(card, s) for s in catalog.statuses][:-1] + [card["device"]["name"]])

    def page(self, title, body, root, active, scripts="", *, path="index.html", description=None, about=None):
        description = description or self.t["discovery"]["pages"][path.removesuffix(".html")]
        self.page_metadata[path] = {"title": title, "description": description}
        url = canonical(self.home, path)
        md_path = path.removesuffix(".html") + ".md"
        discovery = self.t["discovery"]
        metadata = ""
        if url:
            metadata = f'<link rel="canonical" href="{e(url)}"><meta property="og:url" content="{e(url)}">'
            website = {"@type": "WebSite", "@id": self.home + "#website", "name": self.name, "url": self.home}
            page = {"@type": "WebPage", "@id": url + "#page", "url": url, "name": title,
                    "description": description, "isPartOf": {"@id": website["@id"]},
                    "inLanguage": "en", "encodingFormat": "text/html"}
            if about:
                page["about"] = about
            graph = [website, page]
            if path == "index.html" and self.repo:
                graph.append({"@type": "SoftwareSourceCode", "@id": self.home + "#source", "name": self.name,
                              "description": discovery["summary"], "codeRepository": self.repo,
                              "programmingLanguage": discovery["programming_languages"], "url": self.home,
                              "license": self.repo + "/blob/main/LICENSE"})
            metadata += '<script type="application/ld+json">' + script_json({"@context": "https://schema.org", "@graph": graph}) + '</script>'
        n = self.t["nav"]
        links = [(n["install"], "install.html"), (n["devices"], "devices.html"), (n["how"], "index.html#how"), (n["add"], "add.html"), (n["builders"], "format.html")]
        nav = ""
        for label, href in links:
            current = ' class="on"' if label == active else ""
            nav += f'<a href="{root}{href}"{current}>{e(label)}</a>'
        if self.repo:
            nav += f'<a href="{e(self.repo)}">GitHub</a>'
        home = site_url(self.repo)
        picture = f'<meta property="og:image" content="{e(home)}icon-512.png">' if home else ""
        return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{e(title)}</title><meta name="description" content="{e(description)}">{metadata}
<meta name="color-scheme" content="dark light">
<meta name="theme-color" content="{INK}" media="(prefers-color-scheme: dark)"><meta name="theme-color" content="{PAPER}" media="(prefers-color-scheme: light)">
<meta property="og:type" content="website"><meta property="og:title" content="{e(title)}"><meta property="og:description" content="{e(description)}">{picture}
<meta name="twitter:card" content="summary">
<link rel="icon" href="{root}favicon.ico" sizes="32x32"><link rel="icon" href="{root}favicon.svg" type="image/svg+xml">
<link rel="apple-touch-icon" href="{root}apple-touch-icon.png"><link rel="manifest" href="{root}site.webmanifest">
<link rel="preload" href="{root}fonts/{FONT}" as="font" type="font/woff2" crossorigin>
<link rel="stylesheet" href="{root}style.css">
<link rel="alternate" type="text/markdown" href="{root}{md_path}" title="{e(discovery['markdown_label'])}">
<link rel="describedby" type="text/plain" href="{root}llms.txt"><link rel="help" href="{root}agents.html">
<script>if ("IntersectionObserver" in window) document.documentElement.className = "js"</script></head><body>
<div class="sky" aria-hidden="true"><i></i><i></i><i></i><b></b></div>
<header><div class="wrap bar"><a class="brand" href="{root}index.html">{mark(32, "ring")}<span>{e(self.name)}</span></a><nav>{nav}</nav></div>
<div class="progress" aria-hidden="true"></div></header>
<main>{body}</main>
<footer><div class="wrap">{e(self.name)}. The whole catalog as one file: <a href="{root}catalog.json">catalog.json</a>.
Product names belong to their owners; this project is not affiliated with them.
<a href="{root}agents.html">{e(discovery['footer'])}</a> · <a href="{root}{md_path}">{e(discovery['markdown_label'])}</a></div></footer>
{scripts}<script src="{root}app.js" defer></script>
</body></html>"""

    # ---- pieces shared between pages

    def checked(self, status):
        """How much has been checked, as one word with a mark."""
        return f'<span class="check {e(status)}">{e(status)}</span>'

    def facet_values(self, card):
        """What a card offers under each way of narrowing the catalog."""
        caps = card["capability"]
        return {
            "kind": [card["device"]["kind"]],
            "maker": [card["device"]["vendor"]],
            "can": sorted({cap["id"].split(".")[0] for cap in caps}),
            "label": [r for r in self.c.risks if any(cap["risk"] == r for cap in caps)],
            "checked": [s for s in self.c.statuses if any(self.c.best_status(cap) == s for cap in caps)],
            "bus": sorted({m["bus"] for m in card["match"]}),
            "platform": sorted({r["platform"] for cap in caps for r in cap["verification"]}),
        }

    def facets(self):
        """The facets to show, each with its choices and how many devices have each."""
        settings, found = self.t["catalog"], []
        fixed = {"label": list(self.c.risks), "checked": list(self.c.statuses)}
        for facet in settings["facet"]:
            counts = {}
            for card in self.cards:
                for value in self.facet_values(card)[facet["key"]]:
                    counts[value] = counts.get(value, 0) + 1
            order = fixed.get(facet["key"]) or sorted(counts, key=lambda v: (-counts[v], v.lower()))
            options = [(value, counts[value]) for value in order if value in counts]
            if len(options) >= settings["min_options"]:
                found.append({"key": facet["key"], "title": facet["title"], "options": options})
        return found

    def device_tile(self, card, root="", rank=None):
        d, caps = card["device"], card["capability"]
        counts = {s: sum(1 for c in caps if self.c.best_status(c) == s) for s in self.c.statuses}
        meter = "".join(f'<i class="{s}" style="width:{100 * n / len(caps):.1f}%"></i>' for s, n in counts.items() if n and s != "unverified")
        risks = "".join(f'<i class="{r}" title="{e(r)}"></i>' for r in self.c.risks if any(c["risk"] == r for c in caps))
        summary = ", ".join(f"{n} {s}" for s, n in counts.items() if n)
        extra = ""
        if rank is not None:
            words = [d["name"], d["vendor"], d["kind"], d["summary"]] + [m["bus"] for m in card["match"]]
            for cap in caps:
                word = self.c.vocabulary[cap["id"]]
                words += [cap["id"], word["summary"], word.get("ask", ""), cap["risk"], self.c.best_status(cap)]
            extra = (f' data-search="{e(" ".join(words).lower())}" data-facets="{e(json.dumps(self.facet_values(card)))}"'
                     f' data-name="{e(d["name"])}" data-abilities="{len(caps)}" data-rank="{rank}"')
        return (f'<a class="card plate" href="{root}devices/{slug(card)}.html"{extra}>'
                f'<span class="kind">{art(d["kind"])}{e(d["kind"])}</span><span class="name">{e(d["name"])}</span>'
                f'<span class="vendor">{e(d["vendor"])}</span><div class="meter">{meter}</div>'
                f'<span class="counts">{e(summary)}</span><div class="riskdots">{risks}</div></a>')

    def scene_data(self):
        """One scene per kind of device, made from the cards of that kind, using
        only the abilities people would ask for in everyday words. The cards
        with the most checked are read first, and an ability is shown once.
        Nothing here knows any device by name."""
        names = self.t["scenes"].get("kinds", {})
        kinds = {}
        for card in self.cards:
            kind = card["device"]["kind"]
            scene = kinds.setdefault(kind, {"card": card["device"]["id"], "name": names.get(kind, kind), "caps": [], "ids": set()})
            for cap in card["capability"]:
                word = self.c.vocabulary[cap["id"]]
                if not word.get("ask") or cap["id"] in scene["ids"]:
                    continue
                scene["ids"].add(cap["id"])
                scene["caps"].append({
                    "ask": word["ask"], "risk": cap["risk"], "does": self.risk[cap["risk"]]["does"],
                    "show": how_to_show(word), "after": word["example"], "before": word.get("example_before", ""),
                    "swatch": word.get("swatch", ""), "harm": word.get("harm", ""), "status": self.c.best_status(cap),
                })
        devices = [{k: v for k, v in scene.items() if k != "ids"} for scene in kinds.values() if scene["caps"]]
        s = self.t["scenes"]
        words = ("pause", "play", "kept", "undone", "waiting", "keep", "undo", "asking", "approve", "decline", "done", "declined")
        return {"devices": devices, "words": {k: s[k] for k in words}}

    def why_data(self):
        """Every everyday request in the catalog that can go wrong, with what goes wrong."""
        items, seen = [], set()
        for card in self.c.cards:
            for cap in card["capability"]:
                word = self.c.vocabulary[cap["id"]]
                if word.get("harm") and word.get("ask") and cap["id"] not in seen:
                    seen.add(cap["id"])
                    items.append({"ask": word["ask"], "harm": word["harm"], "risk": cap["risk"], "does": self.risk[cap["risk"]]["does"]})
        return {"items": items}

    def travel_block(self):
        t, travel = self.t, self.t["travel"]
        tabs = "".join(f'<button class="tab" role="tab" aria-selected="false" style="--c:var(--{i["risk"]})">{e(i["risk"])}</button>'
                       for i in t["ladder"])
        stops = "".join(f'<div class="stop"><b>{e(name)}</b><span>{e(note)}</span></div>'
                        for name, note in zip(travel["stops"], [travel["asked"], travel["checked"], "", ""]))
        fields = ("risk", "ask", "capability", "gate", "reach", "outcome", "before", "after", "kept", "reverted", "approved")
        data = {"items": [{k: i.get(k, "") for k in fields} | {"device": i["device_name"]} for i in t["ladder"]],
                "words": {k: travel[k] for k in ("confirm", "approve")}}
        block = f"""<div id="travel" class="travel plate pad">
    <div class="tabs" role="tablist">{tabs}</div>
    <p class="ask" id="ask">{e(t["ladder"][0]["ask"])}</p>
    <p class="target mono" id="target">&nbsp;</p>
    <div class="lane"><span class="token"></span>{stops}</div>
    <div class="result"><p id="outcome"></p><div class="line"><span class="value" id="value"></span>
      <span class="timer"><span class="barbox"><span class="fill"></span></span>
      <button class="btn small" id="keep" type="button">{e(travel["confirm"])}</button></span></div></div>
    <p class="note">{inline_code(t["ladder_note"]["text"])}</p></div>"""
        return block, f'<script type="application/json" id="ladder-data">{json.dumps(data)}</script>'

    def steps(self, items):
        rows = ""
        for item in items:
            run = item["run"].replace("{repo}", self.repo or "<this repository's address>")
            rows += f'<div class="step"><span>{e(item["say"])}</span>' + (f"<pre>{e(run)}</pre>" if run else "") + "</div>"
        return f'<div class="steps">{rows}</div>'

    # ---- pages

    def landing(self):
        t, c = self.t, self.c
        s, why, labels, demo, sec = t["scenes"], t["why"], t["labels"], t["demo"], t["sections"]
        scenes, whys = self.scene_data(), self.why_data()
        caps = [cap for card in c.cards for cap in card["capability"]]
        pills = "".join(f'<button class="pill" type="button" role="tab" aria-selected="false">{e(d["name"])}</button>' for d in scenes["devices"])
        drawings = "".join(f'<div class="drawing-wrap" hidden>{drawing(c.card(d["card"]), d["name"])}</div>' for d in scenes["devices"])
        label_cards = "".join(
            f'<div class="plate label" style="--c:var(--{risk})"><span class="swatchbar"></span><h3>{e(risk)}</h3>'
            f'<p class="does">{e(term["does"])}</p><p class="tally"><a href="devices.html?label={e(risk)}"><b>{sum(1 for cap in caps if cap["risk"] == risk)}</b> {e(labels["count"])}</a></p></div>'
            for risk, term in self.risk.items())
        versus = "".join(
            f'<div class="vs-row"><h3>{e(row["topic"])}</h3><p class="without" data-label="{e(why["without"])}">{e(row["without"])}</p>'
            f'<p class="with" data-label="{e(why["with"])}">{e(row["with"])}</p></div>' for row in why["row"])
        why_pills = "".join(f'<button class="pill" type="button" aria-pressed="false">{e(i["ask"])}</button>' for i in whys["items"])
        questions = "".join(f'<details class="plate q"><summary>{e(q["ask"])}</summary><p>{inline_code(q["answer"])}</p></details>' for q in t["question"])
        more = sec["devices_more"].replace("{count}", str(len(self.cards)))
        uses = t["uses"]
        use_cards = "".join(f'<div class="plate use">{art(u["art"])}<h3>{e(u["title"])}</h3><p>{e(u["text"])}</p></div>' for u in uses["item"])
        # A request passes through three hands: the agent, the card, the device.
        flow = '<i class="wire"></i>'.join(f'<span class="node">{art(name)}{e(text)}</span>' for name, text in zip(("agent", "card", "device"), s["flow"]))
        # The last sentence of the headline carries the promise, so it carries the colour.
        first, stop, last = t["tagline"].rstrip(".").rpartition(". ")
        headline = f'{e(first)}. <span class="grad">{e(last)}.</span>' if stop else e(t["tagline"])
        byo = (f'<p class="byo"><span>{e(t["byo"]["title"])}</span>'
               + "".join(f"<i>{e(agent)}</i>" for agent in t["byo"]["agents"]) + "</p>")
        legend = "".join(f'<span><i style="background:var(--{k})"></i>{e(v)}</span>'
                         for k, v in (("reversible", "working"), ("guarded", "needs you"), ("read", "done"), ("gated", "problem")))
        body = f"""
<div class="wrap">
<div class="hero">
  <div>
    <p class="eyebrow">{e(t["eyebrow"])}</p>
    <h1>{headline}</h1>
    <p class="lede">{e(t["lede"])}</p>
    <div class="cta"><a class="btn primary" href="install.html">{e(t["cta"])}</a><a class="btn" href="#why">{e(t["cta_second"])}</a></div>
    {byo}
  </div>
  <div class="plate stage scenes" id="scenes">
    <div class="stage-top"><h3>{e(s["title"])}</h3>
      <button class="btn small fixed" id="scene-toggle" type="button">{e(s["pause"])}</button>
      <button class="btn small" id="scene-next" type="button">{e(s["next"])}</button></div>
    <div class="pills scroll" role="tablist" id="scene-pills">{pills}</div>
    <div class="flow" aria-hidden="true">{flow}</div>
    <div class="drawings">{drawings}</div>
    <div class="talk" aria-live="polite">
      <p class="bubble" id="scene-ask">&nbsp;</p>
      <p class="verdict" id="scene-verdict"><span class="chip fixed" id="scene-chip"></span><span id="scene-does"></span></p>
      <p class="readout" id="scene-readout"></p>
      <p class="confirm" id="scene-confirm"><span id="scene-question">{e(s["waiting"])}</span><span class="barbox"><span class="fill"></span></span>
        <span class="choices"><button class="btn small" id="scene-keep" type="button">{e(s["keep"])}</button>
        <button class="btn small" id="scene-undo" type="button">{e(s["undo"])}</button></span></p>
    </div>
    <p class="scene-foot"><span class="check" id="scene-status">&nbsp;</span><span class="dots" id="scene-dots"></span></p>
    <p class="caption">{e(s["caption"])}</p>
  </div>
</div>

<section class="sec" id="why">
  <div class="sec-head"><p class="eyebrow">{e(why["eyebrow"])}</p><h2>{e(why["title"])}</h2><p class="lede">{e(why["lede"])}</p></div>
  <div class="plate versus"><div class="vs-row vs-head"><span>{e(why["topic"])}</span><span>{e(why["without"])}</span><span>{e(why["with"])}</span></div>{versus}</div>
  <h3 class="try">{e(why["try"])}</h3>
  <div class="pills wide" id="why-pills">{why_pills}</div>
  <div class="cols compare">
    <div class="plate pad bad"><h3>{e(why["without_short"])}</h3><p class="does">{e(why["without_does"])}</p><p class="harm" id="why-harm"></p></div>
    <div class="plate pad good" id="why-good"><h3>{e(why["with_short"])} <span class="chip fixed" id="why-chip"></span></h3><p class="does" id="why-does"></p>
      <div class="barbox" id="why-bar"><span class="fill"></span></div></div>
  </div>
  <p class="note">{e(why["proof"])} <a href="{e(why["proof_url"])}">{e(why["proof_link"])}</a>.</p>
</section>

<section class="sec" id="uses">
  <div class="sec-head"><p class="eyebrow">{e(uses["eyebrow"])}</p><h2>{e(uses["title"])}</h2><p class="lede">{e(uses["lede"])}</p></div>
  <div class="cards uses">{use_cards}</div>
</section>

<section class="sec" id="how">
  <div class="sec-head"><p class="eyebrow">{e(labels["eyebrow"])}</p><h2>{e(labels["title"])}</h2><p class="lede">{e(labels["lede"])}</p></div>
  <div class="cards four">{label_cards}</div>
</section>

<section class="sec" id="today">
  <div class="sec-head"><p class="eyebrow">{e(demo["eyebrow"])}</p><h2>{e(demo["title"])}</h2><p class="lede">{e(demo["lede"])}</p></div>
  <div class="plate stage">
    <div class="stage-top"><span class="legend">{legend}</span><button class="btn small" id="replay" type="button">{e(sec["replay"])}</button></div>
    {board_svg(c.card(demo["card"]), "replay", demo["slots"])}
    <p class="feed" id="feed" aria-live="polite">&nbsp;</p>
    <p class="caption">{e(demo["caption"])}</p>
  </div>
</section>

<section class="sec" id="devices">
  <div class="sec-head"><p class="eyebrow">{e(sec["devices_eyebrow"])}</p><h2>{e(sec["devices_title"])}</h2><p class="lede">{e(sec["devices_lede"])}</p></div>
  <div class="cards">{''.join(self.device_tile(card) for card in self.cards[:sec["devices_shown"]])}</div>
  <p class="after"><a class="btn primary" href="devices.html">{e(more)}</a> <a class="btn" href="add.html">{e(sec["devices_add"])}</a></p>
</section>

<section class="sec" id="questions">
  <div class="sec-head"><p class="eyebrow">{e(sec["questions_eyebrow"])}</p><h2>{e(sec["questions_title"])}</h2></div>
  <div class="qs">{questions}</div>
</section>

</div>"""
        demo_data = {"slots": demo["slots"], "loop": demo["loop_secs"], "pressed": demo["pressed"], "steps": demo["step"]}
        scripts = "".join(f'<script type="application/json" id="{name}">{json.dumps(value)}</script>'
                          for name, value in (("scene-data", scenes), ("why-data", whys), ("demo-data", demo_data)))
        return self.page(f'{self.name}: {t["tagline"]}', body, "", "", scripts)

    def add(self):
        a = self.t["add"]
        prompt = (SOURCE / "add-device-prompt.md").read_text().strip().replace("{repo}", self.repo or "(the Hardpoint repository)")
        rules = '<ul class="ticks built">' + "".join(f"<li>{inline_code(r['text'])}</li>" for r in a["rule"]) + "</ul>"
        issue = (f'<h2 class="small">{e(a["issue_title"])}</h2><p class="lede">{e(a["issue_text"])}</p>'
                 f'<p class="after"><a class="btn" href="{e(self.repo)}/issues/new?template=device.yml">{e(a["issue_link"])}</a></p>') if self.repo else ""
        body = f"""<div class="wrap narrow"><div class="page-head"><p class="eyebrow">{e(self.t["sections"]["devices_eyebrow"])}</p><h1>{e(a["title"])}</h1>
<p class="lede">{e(a["lede"])}</p></div>
<h2 class="small">{e(a["ai_title"])}</h2><p class="lede">{e(a["ai_lede"])}</p>
<div class="plate prompt"><button class="btn small fixed" id="copy" type="button" data-done="{e(a["copied"])}">{e(a["copy"])}</button>
<pre id="prompt">{e(prompt)}</pre></div>
<h2 class="small">{e(a["hand_title"])}</h2>
<div class="plate pad">{self.steps(a["step"])}</div>
<h2 class="small">{e(a["rules_title"])}</h2>
<div class="plate pad">{rules}</div>
{issue}</div>"""
        return self.page(f'{a["title"]} · {self.name}', body, "", self.t["nav"]["add"], path="add.html")

    def install(self):
        a = self.t["install"]
        prompt = (SOURCE / "install-prompt.md").read_text().strip().replace("{repo}", self.repo or "(the Hardpoint repository)")
        listed = lambda items: '<ul class="ticks built">' + "".join(f"<li>{inline_code(i['text'])}</li>" for i in items) + "</ul>"
        body = f"""<div class="wrap narrow"><div class="page-head"><p class="eyebrow">{e(self.t["eyebrow"])}</p><h1>{e(a["title"])}</h1>
<p class="lede">{e(a["lede"])}</p></div>
<h2 class="small">{e(a["ai_title"])}</h2><p class="lede">{e(a["ai_lede"])}</p>
<div class="plate prompt"><button class="btn small fixed" id="copy" type="button" data-done="{e(a["copied"])}">{e(a["copy"])}</button>
<pre id="prompt">{e(prompt)}</pre></div>
<h2 class="small">{e(a["does_title"])}</h2>
<div class="plate pad">{listed(a["does"])}</div>
<h2 class="small">{e(a["updates_title"])}</h2>
<div class="plate pad">{listed(a["updates"])}</div>
<h2 class="small">{e(a["hand_title"])}</h2>
<div class="plate pad">{self.steps(a["step"])}</div></div>"""
        return self.page(f'{a["title"]} · {self.name}', body, "", self.t["nav"]["install"], path="install.html")

    def devices(self):
        c, sec, words = self.c, self.t["sections"], self.t["catalog"]
        groups = ""
        for facet in self.facets():
            options = "".join(f'<label class="opt"><input type="checkbox" value="{e(value)}"><span>{e(value)}</span><b>{count}</b></label>'
                              for value, count in facet["options"])
            groups += (f'<div class="facet" role="group" aria-labelledby="facet-{e(facet["key"])}" data-facet="{e(facet["key"])}">'
                       f'<p class="facet-title" id="facet-{e(facet["key"])}">{e(facet["title"])}</p>{options}</div>')
        orders = "".join(f'<option value="{e(o["key"])}">{e(o["text"])}</option>' for o in words["sort_by"])
        tiles = "".join(self.device_tile(card, rank=rank) for rank, card in enumerate(self.cards))
        found = words["found"].replace("{n}", str(len(self.cards))).replace("{total}", str(len(self.cards)))
        body = f"""<div class="wrap"><div class="page-head"><p class="eyebrow">{e(sec["devices_eyebrow"])}</p><h1>{e(self.t["nav"]["devices"])}</h1>
<p class="lede">{e(sec["devices_lede"])}</p></div>
<div class="catalog" id="catalog" data-found="{e(words["found"])}">
<details class="facets" id="facets" hidden><summary>{e(words["filters"])}</summary>{groups}</details>
<div class="results">
<div class="toolbar"><input type="search" id="q" placeholder="{e(words["search"])}" aria-label="{e(words["search"])}">
<label class="sort"><span>{e(words["sort"])}</span><select id="sort">{orders}</select></label></div>
<p class="found"><span id="found" aria-live="polite">{e(found)}</span><button class="btn small" id="clear" type="button" hidden>{e(words["clear"])}</button>
<a class="btn small primary" href="add.html">{e(sec["devices_add"])}</a></p>
<div class="cards" id="cards">{tiles}</div>
<p class="none" id="none" hidden>{e(words["none"])} <a href="add.html">{e(words["none_link"])}</a></p>
</div></div>
<h2 class="small">Checked</h2>{defs((self.checked(s), text) for s, text in c.statuses.items())}
<h2 class="small">Labels</h2>{defs((chip("risk", r), text) for r, text in c.risks.items())}</div>"""
        return self.page(f'{self.t["nav"]["devices"]} · {self.name}', body, "", self.t["nav"]["devices"], path="devices.html")

    def device(self, card):
        c, d = self.c, card["device"]
        match_rows = "".join(
            f'<tr><td>{e(m["bus"])}</td><td class="mono">{e(" ".join(f"{k}={hexid(v)}" for k, v in m.items() if k != "bus"))}</td></tr>'
            for m in card["match"])
        requires = "".join(
            f'<tr><td>{e(r["platform"])}</td><td>{e(r["what"])}<span class="sub">{e(r.get("note", ""))}</span></td></tr>'
            for r in card.get("requires", []))
        rows = []
        for cap in card["capability"]:
            word = c.vocabulary[cap["id"]]
            record = min(cap["verification"], key=lambda r: c.rank("statuses", r["status"]))
            params = " ".join(f"{k}={hexid(v) if isinstance(v, int) else v}" for k, v in cap.get("params", {}).items())
            on = " ".join(x for x in [record["platform"], record.get("os", ""), record.get("firmware", "")] if x)
            facts = [("Checked on", " · ".join(x for x in [on, record.get("date", "")] if x)), ("What was checked", record.get("note", "")),
                     ("Note", cap.get("note", "")), ("If done carelessly", word.get("harm", "")),
                     ("Route", f'{cap["adapter"]} {params}'.strip() + f' ({c.adapters[cap["adapter"]]["status"]})'),
                     ("Value", f'{word["kind"]}, {word["value"]}' + (f', {cap["range"][0]:g} to {cap["range"][1]:g}' if "range" in cap else "")),
                     ("Privacy", cap.get("privacy", "")),
                     ("Needs firmware", " ".join(f"{k} {v}" for k, v in cap.get("firmware", {}).items()).replace("family ", ""))]
            details = "".join(f"<dt>{e(k)}</dt><dd>{e(v)}</dd>" for k, v in facts if v)
            commit = card["_commits"].get(cap["id"])
            link = f' <a class="commit" href="{e(self.repo)}/commit/{commit}">commit</a>' if commit and self.repo else ""
            rows.append(
                f'<tr id="cap-{e(cap["id"])}"><td><b>{e(word.get("ask") or word["summary"])}</b><span class="sub mono">{e(cap["id"])}</span>'
                f'<details class="more"><summary>Details</summary><dl>{details}</dl></details></td>'
                f'<td>{chip("risk", cap["risk"])}</td>'
                f'<td>{self.checked(record["status"])}{link}</td></tr>')
        keymap = ""
        if card.get("keys"):
            keymap = (f'<h2 class="small">Key numbers</h2><p class="lede">{e(card["keys"]["numbering"])}.</p>'
                      f'<div class="plate stage">{board_svg(card, "keymap", numbers=True)}'
                      f'<p class="keyinfo" id="keyinfo">{len(card["keys"]["grid"])} keys</p></div>')
        sources = "".join(f'<li><a href="{e(s["url"])}">{e(s["title"])}</a></li>' for s in card.get("sources", []))
        where = (f'<a class="mono" href="{e(self.repo)}/blob/main/{e(card["_path"])}">{e(card["_path"])}</a>'
                 if self.repo else f'<span class="mono">{e(card["_path"])}</span>')
        body = f"""<div class="wrap"><div class="page-head">
<p class="crumb"><a href="../devices.html">{e(self.t["nav"]["devices"])}</a> / <a href="../devices.html?kind={urllib.parse.quote(d["kind"])}">{e(d["kind"])}</a> / <a href="../devices.html?maker={urllib.parse.quote(d["vendor"])}">{e(d["vendor"])}</a></p>
<h1>{e(d["name"])}</h1><p class="lede">{e(d["summary"])}</p></div>
<h2 class="small">What it can do</h2>
<div class="tablebox"><table><tr><th>Ability</th><th>Label</th><th>Checked</th></tr>{''.join(rows)}</table></div>
{keymap}
{f'<h2 class="small">Before it will work</h2><div class="tablebox"><table><tr><th>Platform</th><th>Needs</th></tr>{requires}</table></div>' if requires else ''}
<h2 class="small">How it is recognised</h2>
<div class="tablebox"><table><tr><th>Bus</th><th>Identifiers</th></tr>{match_rows}</table></div>
{f'<h2 class="small">Sources</h2><ul>{sources}</ul>' if sources else ''}
<h2 class="small">In a standard form</h2>
<p class="lede">The same card as a <a href="../models/{slug(card)}.tm.jsonld">W3C Web of Things Thing Model</a>, for tools that read that standard. <a href="../ns.html">Hardpoint's added terms</a>.</p>
<h2 class="small">The card</h2>
<p class="lede">This page is made from {where}. If something is wrong, or you have checked more of it, <a href="../add.html">edit that file</a>.</p>
<pre>{e(card["_source"])}</pre></div>"""
        return self.page(f'{d["vendor"]} {d["name"]} · {self.name}', body, "../", self.t["nav"]["devices"],
                         path=f"devices/{slug(card)}.html", description=d["summary"],
                         about={"@type": "Thing", "name": f'{d["vendor"]} {d["name"]}', "identifier": d["id"]})

    def namespace_page(self):
        """The terms Hardpoint adds to the Web of Things models, each with an address of its own."""
        rows = "".join(f'<tr id="{e(term)}"><td class="mono">hp:{e(term)}</td><td>{e(meaning)}</td></tr>' for term, meaning in WOT_TERMS.items())
        body = f"""<div class="wrap"><div class="page-head"><p class="crumb"><a href="format.html">{e(self.t["nav"]["builders"])}</a> / Terms</p><h1>Terms for the Web of Things</h1>
<p class="lede">Every card is also published as a W3C Web of Things Thing Model, so other tools can read the catalog in a standard form.
The standard already says whether a value can be changed and whether an action is safe to repeat. These are the terms Hardpoint adds,
under the prefix <span class="mono">hp</span>, which stands for <span class="mono">{e(self.namespace)}</span></p></div>
<div class="tablebox"><table><tr><th>Term</th><th>Meaning</th></tr>{rows}</table></div>
<h2 class="small">Labels</h2>{defs((chip("risk", r), text) for r, text in self.c.risks.items())}
<h2 class="small">Checked</h2>{defs((self.checked(s), text) for s, text in self.c.statuses.items())}
<h2 class="small">Models</h2><p class="lede">One file per card, at <span class="mono">models/&lt;maker&gt;-&lt;model&gt;.tm.jsonld</span>. Each device's page links to its own.</p></div>"""
        return self.page(f"Terms · {self.name}", body, "", self.t["nav"]["builders"], path="ns.html")

    def vocabulary(self):
        c = self.c
        used = {}
        for card in c.cards:
            for cap in card["capability"]:
                used.setdefault(cap["id"], []).append(card)
        rows = ""
        for cid, word in c.vocabulary.items():
            privacy = f' <span class="chip plain">{e(word["privacy"])}</span>' if "privacy" in word else ""
            devices = ", ".join(f'<a href="devices/{slug(card)}.html">{e(card["device"]["name"])}</a>'
                                for card in used.get(cid, [])) or '<span class="sub">none yet</span>'
            rows += (f'<tr><td><span class="mono">{e(cid)}</span><span class="sub">{e(word["summary"])}</span></td>'
                     f'<td>{e(word["kind"])}<span class="sub">{e(word["value"])}</span></td>'
                     f'<td>{chip("risk", word["min_risk"])}{privacy}</td><td>{devices}</td></tr>')
        body = f"""<div class="wrap"><div class="page-head"><p class="crumb"><a href="format.html">{e(self.t["nav"]["builders"])}</a> / Vocabulary</p><h1>Vocabulary</h1>
<p class="lede">A capability id means the same thing on every device, so a rule or an AI can ask for
<span class="mono">battery.level</span> without knowing the hardware.</p></div>
<h2 class="small">Kinds</h2>{defs((f'<span class="mono">{e(k)}</span>', text) for k, text in c.kinds.items())}
<h2 class="small">Privacy</h2>{defs((f'<span class="chip plain">{e(k)}</span>', text) for k, text in c.privacy.items())}
<h2 class="small">Capabilities</h2>
<div class="tablebox"><table><tr><th>Capability</th><th>Kind</th><th>Lowest risk allowed</th><th>Devices</th></tr>{rows}</table></div></div>"""
        return self.page(f"Vocabulary · {self.name}", body, "", self.t["nav"]["builders"], path="vocabulary.html")

    def adapters(self):
        c = self.c
        rows = ""
        for name, adapter in c.adapters.items():
            count = sum(1 for card in c.cards for cap in card["capability"] if cap["adapter"] == name)
            rows += (f'<tr><td><span class="mono">{e(name)}</span><span class="sub">{e(adapter["summary"])}</span>'
                     f'<span class="sub">{e(adapter.get("notes", ""))}</span></td>'
                     f'<td>{e(adapter["status"])}<span class="sub">{e(adapter.get("implementation", ""))}</span></td>'
                     f'<td>{e(", ".join(adapter["platforms"]))}</td><td class="mono">{e(", ".join(adapter.get("params", [])) or "none")}</td>'
                     f"<td>{count}</td></tr>")
        body = f"""<div class="wrap"><div class="page-head"><p class="crumb"><a href="format.html">{e(self.t["nav"]["builders"])}</a> / Adapters</p><h1>Adapters</h1>
<p class="lede">Adapters are the code that talks to hardware. They live in the daemon, not in the catalog.
A device card can only name an adapter and give it typed values; it can never carry a command or a script.</p></div>
<h2 class="small">{len(c.adapters)} adapters</h2>
<div class="tablebox"><table><tr><th>Adapter</th><th>Status</th><th>Platforms</th><th>Parameters</th><th>Capabilities using it</th></tr>{rows}</table></div></div>"""
        return self.page(f"Adapters · {self.name}", body, "", self.t["nav"]["builders"], path="adapters.html")

    def format(self):
        c, travel, sec = self.c, self.t["travel"], self.t["sections"]
        example = c.card(self.t["demo"]["card"]) or c.cards[0]
        block, script = self.travel_block()
        parts = [("[device]", "What the device is: vendor, name, kind and a one-line summary."),
                 ("[[match]]", "How software recognises it when it is attached: bus and identifiers."),
                 ("[keys]", "For keyboards: how key numbers, switch positions and lights line up."),
                 ("[[requires]]", "What has to be true before any of it works, such as a permission or a closed program."),
                 ("[[capability]]", "One thing the device can do: a word from the vocabulary, the adapter that reaches it, its risk, and how much has been checked."),
                 ("[[sources]]", "Where the information came from.")]
        body = f"""<div class="wrap"><div class="page-head"><p class="eyebrow">{e(self.t["nav"]["builders"])}</p><h1>The card format</h1>
<p class="lede">Each device has a card: one TOML file per model, at
<span class="mono">catalog/devices/&lt;vendor&gt;/&lt;model&gt;.toml</span>. Cards are data only.</p>
<p class="after"><a class="btn" href="vocabulary.html">Vocabulary</a> <a class="btn" href="adapters.html">Adapters</a> <a class="btn" href="add.html">{e(self.t["nav"]["add"])}</a></p></div>
<h2 class="small">{e(travel["title"])}</h2><p class="lede">{e(travel["lede"])}</p>
{block}
<h2 class="small">What a card says</h2>{defs((f'<span class="mono">{e(k)}</span>', text) for k, text in parts)}
<h2 class="small">Risk</h2>{defs((chip("risk", r), text) for r, text in c.risks.items())}
<h2 class="small">Checked</h2>{defs((self.checked(s), text) for s, text in c.statuses.items())}
<h2 class="small">{e(sec["start_title"])}</h2><p class="lede">{e(sec["start_lede"])}</p>
<div class="plate pad">{self.steps(self.t["start"])}</div>
<h2 class="small">An example</h2><p class="lede"><span class="mono">{e(example["_path"])}</span>. The full rules are in
<span class="mono">docs/SPEC.md</span> and <span class="mono">catalog/schema/device.schema.json</span>.</p>
<pre>{e(example["_source"])}</pre></div>"""
        return self.page(f"Format · {self.name}", body, "", self.t["nav"]["builders"], script, path="format.html")

    def agents(self):
        a, d = self.t["agent"], self.t["discovery"]
        sections = "".join(f'<section><h2 class="small">{e(s["title"])}</h2><p>{inline_code(s["text"])}</p>'
                           + (f'<pre>{e(s["code"])}</pre>' if s.get("code") else "") + '</section>' for s in a["section"])
        docs = "".join(f'<li><a href="{e(path)}">{e(path)}</a></li>' for path in d["published_docs"])
        body = (f'<div class="wrap narrow"><div class="page-head"><h1>{e(a["title"])}</h1><p class="lede">{e(a["lede"])}</p></div>'
                f'<p><a href="llms.txt">llms.txt</a> · <a href="catalog.json">{e(d["feed_label"])}</a> · '
                f'<a href="search-index.json">{e(d["search_label"])}</a>'
                + (' · <a href="sitemap.xml">sitemap.xml</a>' if self.home else '') + f'</p>{sections}'
                f'<h2 class="small">{e(d["docs_title"])}</h2><ul>{docs}</ul></div>')
        return self.page(f'{a["title"]} · {self.name}', body, "", "", path="agents.html")

    def write_discovery(self):
        d = self.t["discovery"]
        link = lambda path: urllib.parse.urljoin(self.home, path) if self.home else path
        for path in self.page_metadata:
            html = (SITE / path).read_text()
            (SITE / (path.removesuffix(".html") + ".md")).write_text(markdown(html, link(path)))
        for path in d["published_docs"]:
            dest = SITE / path
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(ROOT / path, dest)
        file_list = lambda entries: "\n".join(f'- [{title}]({link(path)})' + (f': {note}' if note else '') for title, path, note in entries)
        guides = [(self.page_metadata[path]["title"], path.removesuffix(".html") + ".md", self.page_metadata[path]["description"])
                  for path in ("agents.html", "install.html", "devices.html", "add.html", "format.html", "adapters.html", "vocabulary.html", "ns.html")]
        devices = [(f'{c["device"]["vendor"]} {c["device"]["name"]}', f'devices/{slug(c)}.md', c["device"]["summary"]) for c in self.cards]
        data = [(d["feed_label"], "catalog.json", d["index_note"]), (d["search_label"], "search-index.json", d["index_note"])]
        docs = [(path, path, "") for path in d["published_docs"]]
        if self.repo:
            docs += [(path, f"{self.repo}/blob/main/{path}", "") for path in d["repository_docs"]]
        text = f'# {self.name}\n\n> {d["summary"]}\n\n' + "\n\n".join(d["notes"]) + "\n\n"
        text += "\n\n".join(f'## {title}\n\n{file_list(entries)}' for title, entries in
                             [(d["guide_title"], guides), (d["data_title"], data), (d["devices_title"], devices), (d["docs_title"], docs)])
        (SITE / "llms.txt").write_text(text + "\n")
        if self.home:
            (SITE / "sitemap.xml").write_text(sitemap(canonical(self.home, path) for path in self.page_metadata))
        robots = origin_robots(self.home)
        if robots:
            (SITE / "robots.txt").write_text(robots)
        entries = []
        for card in self.cards:
            for cap in card["capability"]:
                word, adapter = self.c.vocabulary[cap["id"]], self.c.adapters[cap["adapter"]]
                entries.append({"device": card["device"]["id"], "name": card["device"]["name"], "maker": card["device"]["vendor"],
                                "kind": card["device"]["kind"], "capability": cap["id"], "summary": word["summary"],
                                "question": word.get("ask"), "label": cap["risk"], "checked": self.c.best_status(cap),
                                "adapter": cap["adapter"], "adapter_status": adapter["status"],
                                "declared_platforms": adapter["platforms"], "verification": cap["verification"],
                                "url": link(f'devices/{slug(card)}.html#cap-{cap["id"]}'),
                                "markdown": link(f'devices/{slug(card)}.md'), "model": link(f'models/{slug(card)}.tm.jsonld')})
        (SITE / "search-index.json").write_text(json.dumps({"format": 1, "scope": "catalog-not-runtime-inventory",
                                                         "note": d["index_note"], "entries": entries}, indent=2) + "\n")

    def write(self):
        c = self.c
        if SITE.exists():
            shutil.rmtree(SITE)
        (SITE / "devices").mkdir(parents=True)
        for name in ("style.css", "app.js"):
            shutil.copy(SOURCE / name, SITE / name)
        (SITE / "fonts").mkdir()
        shutil.copy(SOURCE / "fonts" / FONT, SITE / "fonts" / FONT)
        # One mark, in every form a browser or a phone asks for.
        (SITE / "favicon.svg").write_text(mark())
        (SITE / "favicon.ico").write_bytes(ico((16, 32, 48)))
        for name, size in (("apple-touch-icon.png", 180), ("icon-192.png", 192), ("icon-512.png", 512)):
            (SITE / name).write_bytes(mark_png(size, bleed=True))
        (SITE / "site.webmanifest").write_text(json.dumps({
            "name": self.name, "short_name": self.name, "description": self.t["tagline"], "start_url": ".", "display": "minimal-ui",
            "background_color": INK, "theme_color": INK,
            "icons": [{"src": f"icon-{size}.png", "sizes": f"{size}x{size}", "type": "image/png", "purpose": "any maskable"} for size in (192, 512)],
        }, indent=2) + "\n")
        pages = {"index": self.landing, "install": self.install, "add": self.add, "devices": self.devices, "vocabulary": self.vocabulary,
                 "adapters": self.adapters, "format": self.format, "ns": self.namespace_page, "agents": self.agents}
        (SITE / "models").mkdir()
        for card in c.cards:
            page = f"{self.home}devices/{slug(card)}.html" if self.home else None
            (SITE / "models" / f"{slug(card)}.tm.jsonld").write_text(json.dumps(thing_model(card, c, self.namespace, page), indent=2) + "\n")
        for name, render in pages.items():
            (SITE / f"{name}.html").write_text(render())
        for card in c.cards:
            (SITE / "devices" / f"{slug(card)}.html").write_text(self.device(card))
        feed = {"format": 1, "terms": c.terms, "vocabulary": c.vocabulary, "adapters": c.adapters,
                "devices": [{k: v for k, v in card.items() if not k.startswith("_")} for card in c.cards]}
        (SITE / "catalog.json").write_text(json.dumps(feed, indent=2) + "\n")
        self.write_discovery()
        return sum(1 for path in SITE.rglob("*") if path.is_file())


def main():
    catalog = Catalog()
    content = load_content(catalog) if not catalog.errors else None
    if catalog.errors:
        print(f"{len(catalog.errors)} problem(s):", file=sys.stderr)
        for error in catalog.errors:
            print(f"  {error}", file=sys.stderr)
        return 1
    caps = [cap for card in catalog.cards for cap in card["capability"]]
    counts = {s: sum(1 for cap in caps if catalog.best_status(cap) == s) for s in catalog.statuses}
    print(f"valid: {len(catalog.cards)} devices, {len(caps)} capabilities ({', '.join(f'{n} {s}' for s, n in counts.items())})")
    if "--check" not in sys.argv:
        images = write_assets(catalog, content)
        files = Site(catalog, content, repo_url()).write()
        print(f"built {SITE.relative_to(ROOT)}/ ({files} files) and {ASSETS.relative_to(ROOT)}/ ({images} images)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
