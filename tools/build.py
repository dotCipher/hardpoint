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
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
CATALOG = ROOT / "catalog"
SOURCE = ROOT / "site-src"
SITE = ROOT / "site"
ASSETS = ROOT / "assets"

# Brand colours. The site's stylesheet declares the same four risk colours.
INK, PANEL, LINE, TEXT, MUTED, DIM = "#0a0c0f", "#11151a", "#232b35", "#dde3ea", "#8792a2", "#5b6675"
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


class Catalog:
    def __init__(self):
        self.errors = []
        terms = load_toml(CATALOG / "vocabulary" / "terms.toml")
        self.risks = {k: v["meaning"] for k, v in terms["risk"].items()}
        self.statuses = {k: v["meaning"] for k, v in terms["status"].items()}
        self.privacy = {k: v["meaning"] for k, v in terms["privacy"].items()}
        self.kinds = {k: v["meaning"] for k, v in terms["kind"].items()}
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
        for name, adapter in self.adapters.items():
            if adapter.get("status") not in ("implemented", "planned"):
                self.errors.append(f"adapter {name}: status must be implemented or planned")

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
            if cap["risk"] == "gated" and cap["adapter"] != "manual":
                self.errors.append(f"{where}: gated capabilities must use the 'manual' adapter")
            if "privacy" in word:
                if self.rank("privacy", cap.get("privacy", "open")) < self.rank("privacy", word["privacy"]):
                    self.errors.append(f"{where}: must declare privacy {word['privacy']!r} or stricter")
            for key in cap.get("params", {}):
                if key not in adapter.get("params", []):
                    self.errors.append(f"{where}: adapter {cap['adapter']!r} has no parameter {key!r}")
            if ("range" in cap or "choices" in cap) and word["kind"] != "control":
                self.errors.append(f"{where}: only a control can have a range or choices")
            for record in cap["verification"]:
                if record["platform"] not in adapter.get("platforms", []):
                    self.errors.append(f"{where}: adapter {cap['adapter']!r} does not run on {record['platform']}")
                if record["status"] == "tested" and adapter.get("status") != "implemented":
                    self.errors.append(f"{where}: cannot be 'tested' through an adapter that is not implemented")
                if record["status"] in ("tested", "observed") and not (record.get("date") and record.get("by")):
                    self.errors.append(f"{where}: a {record['status']} record needs a date and who checked it")

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


def load_content(catalog):
    """The words and demo scripts, checked against the catalog they describe."""
    content = load_toml(SOURCE / "content.toml")
    errors = catalog.errors
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
            item["device_name"] = f'{target["device"]["vendor"]} {target["device"]["name"]}'
    if [item["risk"] for item in content["ladder"]] != list(catalog.risks):
        errors.append("content.toml: the ladder needs one example per risk level, in order")
    return content


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

def mark(plate, ring, size=32):
    """The Hardpoint mark: a mounting plate, four fixing holes, one attach point."""
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" width="{size}" height="{size}" aria-hidden="true">'
        f'<rect x="3" y="3" width="26" height="26" rx="6" fill="none" stroke="{plate}" stroke-width="2"/>'
        f'<circle class="ring" cx="16" cy="16" r="5.5" fill="none" stroke="{ring}" stroke-width="2.5"/>'
        + "".join(f'<circle cx="{x}" cy="{y}" r="1.6" fill="{plate}"/>' for x in (9, 23) for y in (9, 23))
        + "</svg>"
    )


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


def board_svg(card, classes, slots=(), numbers=False, interactive=False):
    keys = card["keys"]
    places, plates, width, height = board_geometry(keys)
    slot_by_key = {s["key"]: s for s in slots}
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
            f'<rect x="{x}" y="{y}" width="{KEY}" height="{KEY}" rx="8"/>{label}</g>'
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
        "logo-dark.svg": mark(TEXT, RISK_COLOR["guarded"], 96),
        "logo-light.svg": mark("#1c2430", "#d98600", 96),
        "hero.svg": hero_image(card, content["demo"]["slots"], content),
        "ladder.svg": ladder_image(catalog),
    }
    for name, body in files.items():
        (ASSETS / name).write_text(body + "\n")
    return len(files)


# ---------------------------------------------------------------------- site

def chip(kind, value):
    return f'<span class="chip {kind}-{e(value)}">{e(value)}</span>'


def defs(pairs):
    return "<dl class=\"defs\">" + "".join(f"<dt>{term}</dt><dd>{e(text)}</dd>" for term, text in pairs) + "</dl>"


def slug(card):
    return card["device"]["id"].replace("/", "-")


def hexid(value):
    return f"{value:04x}" if isinstance(value, int) else str(value)


def inline_code(text):
    """Escape text, turning `quoted` spans into code."""
    return re.sub(r"`([^`]+)`", r"<code>\1</code>", e(text))


class Site:
    def __init__(self, catalog, content, repo):
        self.c, self.t, self.repo = catalog, content, repo
        self.name = content["name"]

    def page(self, title, body, root, active, scripts=""):
        links = [("Devices", "devices.html"), ("Vocabulary", "vocabulary.html"), ("Adapters", "adapters.html"), ("Format", "format.html")]
        nav = ""
        for label, href in links:
            current = ' class="on"' if label == active else ""
            nav += f'<a href="{root}{href}"{current}>{label}</a>'
        if self.repo:
            nav += f'<a href="{e(self.repo)}">GitHub</a>'
        add = f' <a href="{e(self.repo)}#add-a-device">Add a device</a>.' if self.repo else ""
        return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{e(title)}</title><meta name="description" content="{e(self.t["tagline"])}">
<meta name="theme-color" content="{INK}"><link rel="icon" href="{root}logo.svg" type="image/svg+xml">
<link rel="stylesheet" href="{root}style.css"></head><body>
<header><div class="wrap bar"><a class="brand" href="{root}index.html">{mark(TEXT, RISK_COLOR["guarded"])}<span>{e(self.name)}</span></a><nav>{nav}</nav></div></header>
<main>{body}</main>
<footer><div class="wrap">{e(self.name)} catalog, format 1. Machine-readable copy: <a href="{root}catalog.json">catalog.json</a>.{add}
Product names belong to their owners; this project is not affiliated with them.</div></footer>
{scripts}<script src="{root}app.js" defer></script>
</body></html>"""

    def device_tile(self, card, root=""):
        d = card["device"]
        caps = card["capability"]
        counts = {s: sum(1 for c in caps if self.c.best_status(c) == s) for s in self.c.statuses}
        meter = "".join(f'<i class="{s}" style="width:{100 * n / len(caps):.1f}%"></i>' for s, n in counts.items() if n and s != "unverified")
        risks = "".join(f'<i class="{r}" title="{r}"></i>' for r in self.c.risks if any(c["risk"] == r for c in caps))
        summary = ", ".join(f"{n} {s}" for s, n in counts.items() if n)
        search = " ".join([d["name"], d["vendor"], d["kind"]] + [c["id"] for c in caps]).lower()
        return (f'<a class="card plate reveal" href="{root}devices/{slug(card)}.html" data-search="{e(search)}">'
                f'<span class="kind">{e(d["kind"])}</span><span class="name">{e(d["name"])}</span>'
                f'<span class="vendor">{e(d["vendor"])}</span><div class="meter">{meter}</div>'
                f'<span class="counts">{len(caps)} capabilities · {e(summary)}</span><div class="riskdots">{risks}</div></a>')

    def landing(self):
        t, c = self.t, self.c
        demo, sec, travel = t["demo"], t["sections"], t["travel"]
        caps = [cap for card in c.cards for cap in card["capability"]]
        tested = sum(1 for cap in caps if c.best_status(cap) == "tested")
        card = c.card(demo["card"])
        stats = [(len(c.cards), "devices"), (len(caps), "capabilities"), (tested, "tested by a person"), (len(c.vocabulary), "shared words")]
        cta = f'<a class="btn primary" href="devices.html">{e(sec["devices_more"])}</a><a class="btn" href="#travel">{e(travel["title"])}</a>'
        tabs = "".join(
            f'<button class="tab" role="tab" aria-selected="false" style="--c:var(--{item["risk"]})">{e(item["risk"])}</button>'
            for item in t["ladder"]
        )
        stops = "".join(f'<div class="stop"><b>{e(name)}</b><span>{e(note)}</span></div>'
                        for name, note in zip(travel["stops"], [travel["asked"], travel["checked"], "", ""]))
        ticks = lambda items, cls: f'<ul class="ticks {cls}">' + "".join(f"<li>{inline_code(i['text'])}</li>" for i in items) + "</ul>"
        steps = "".join(f'<div class="step"><span>{e(s["say"])}</span><pre>{e(s["run"])}</pre></div>' for s in t["start"])
        promises = "".join(f'<div class="plate pad reveal"><h3>{e(p["title"])}</h3><p class="sub" style="margin:0">{e(p["text"])}</p></div>'
                           for p in t["promise"])
        body = f"""
<div class="wrap">
<div class="hero">
  <div>
    <p class="eyebrow">{e(t["eyebrow"])}</p>
    <h1>{e(t["tagline"])}</h1>
    <p class="lede">{e(t["lede"])}</p>
    <div class="cta">{cta}</div>
    <div class="stats">{''.join(f'<div class="stat"><b>{n}</b><span>{e(label)}</span></div>' for n, label in stats)}</div>
  </div>
  <div class="plate stage">
    <div class="stage-top"><h3>{e(demo["title"])}</h3><button class="btn small" id="replay" type="button">{e(sec["replay"])}</button></div>
    {board_svg(card, "replay", demo["slots"])}
    <ul class="feed" id="feed" aria-live="polite"></ul>
    <p class="caption">{e(demo["caption"])}</p>
  </div>
</div>

<section id="travel" class="travel">
  <div class="sec-head reveal"><p class="eyebrow">01 · {e(travel["stops"][2])}</p><h2>{e(travel["title"])}</h2><p class="lede">{e(travel["lede"])}</p></div>
  <div class="plate pad reveal">
    <div class="tabs" role="tablist">{tabs}</div>
    <p class="ask" id="ask">{e(t["ladder"][0]["ask"])}</p>
    <p class="target mono" id="target"></p>
    <div class="lane"><span class="token"></span>{stops}</div>
    <div class="result">
      <p id="outcome"></p>
      <span class="value" id="value"></span>
      <span class="timer"><span class="barbox"><span class="fill" style="display:block"></span></span>
      <button class="btn small" id="keep" type="button">{e(travel["confirm"])}</button></span>
    </div>
    <p class="note">{e(t["ladder_note"]["text"])}</p>
  </div>
</section>

<section id="devices">
  <div class="sec-head reveal"><p class="eyebrow">02 · Device cards</p><h2>{e(sec["devices_title"])}</h2><p class="lede">{e(sec["devices_lede"])}</p></div>
  <div class="cards">{''.join(self.device_tile(card) for card in c.cards)}</div>
  <p style="margin:22px 0 0"><a class="btn" href="devices.html">{e(sec["devices_more"])}</a> <a class="btn" href="format.html">Format</a></p>
</section>

<section id="status">
  <div class="sec-head reveal"><p class="eyebrow">03 · Status</p><h2>{e(sec["status_title"])}</h2></div>
  <div class="cols">
    <div class="plate pad reveal"><h3>{e(sec["built_label"])}</h3>{ticks(t["status"]["built"], "built")}</div>
    <div class="plate pad reveal"><h3>{e(sec["next_label"])}</h3>{ticks(t["status"]["next"], "")}</div>
  </div>
</section>

<section id="start">
  <div class="sec-head reveal"><p class="eyebrow">04 · Start</p><h2>{e(sec["start_title"])}</h2><p class="lede">{e(sec["start_lede"])}</p></div>
  <div class="plate pad reveal"><div class="steps">{steps}</div></div>
</section>

<section id="promises">
  <div class="sec-head reveal"><p class="eyebrow">05 · Limits</p><h2>{e(sec["promise_title"])}</h2></div>
  <div class="cols">{promises}</div>
</section>
</div>"""
        demo_data = {"slots": demo["slots"], "loop": demo["loop_secs"], "pressed": demo["pressed"],
                     "steps": [{k: v for k, v in step.items()} for step in demo["step"]]}
        ladder_data = {"items": [
            {"risk": i["risk"], "ask": i["ask"], "device": i["device_name"], "capability": i["capability"], "gate": i["gate"],
             "reach": i["reach"], "outcome": i["outcome"], "before": i["before"], "after": i["after"],
             "kept": i.get("kept", ""), "reverted": i.get("reverted", "")} for i in t["ladder"]]}
        scripts = (f'<script type="application/json" id="demo-data">{json.dumps(demo_data)}</script>'
                   f'<script type="application/json" id="ladder-data">{json.dumps(ladder_data)}</script>')
        return self.page(f'{self.name}: {t["tagline"]}', body, "", "", scripts)

    def devices(self):
        c = self.c
        body = f"""<div class="wrap"><div class="page-head"><p class="eyebrow">Catalog</p><h1>Devices</h1>
<p class="lede">{e(self.t["sections"]["devices_lede"])}</p></div>
<h2 class="small">{len(c.cards)} devices</h2>
<input type="search" id="q" placeholder="Filter by name, vendor, kind or capability" aria-label="Filter devices">
<div class="cards">{''.join(self.device_tile(card) for card in c.cards)}</div>
<h2 class="small">How much has been checked</h2>{defs((chip("st", s), text) for s, text in c.statuses.items())}
<h2 class="small">How risky a change is</h2>{defs((chip("risk", r), text) for r, text in c.risks.items())}</div>"""
        return self.page(f"Devices · {self.name}", body, "", "Devices")

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
            params = " ".join(f"{k}={hexid(v) if isinstance(v, int) else v}" for k, v in cap.get("params", {}).items())
            limits = f'range {cap["range"][0]:g} to {cap["range"][1]:g}' if "range" in cap else ""
            record = min(cap["verification"], key=lambda r: c.rank("statuses", r["status"]))
            detail = " · ".join(x for x in [record["platform"] + (" " + record["os"] if record.get("os") else ""),
                                            record.get("firmware", ""), record.get("date", ""), record.get("by", "")] if x)
            privacy = f' <span class="chip plain">{e(cap["privacy"])}</span>' if "privacy" in cap else ""
            rows.append(
                f'<tr><td><span class="mono">{e(cap["id"])}</span><span class="sub">{e(word["summary"])}</span>'
                f'<span class="sub">{e(cap.get("note", ""))}</span></td>'
                f'<td>{e(word["kind"])}<span class="sub">{e(word["value"])}</span><span class="sub">{e(limits)}</span></td>'
                f'<td>{chip("risk", cap["risk"])}{privacy}</td>'
                f'<td><span class="mono">{e(cap["adapter"])}</span><span class="sub mono">{e(params)}</span>'
                f'<span class="sub">adapter {e(c.adapters[cap["adapter"]]["status"])}</span></td>'
                f'<td>{chip("st", record["status"])}<span class="sub">{e(detail)}</span><span class="sub">{e(record.get("note", ""))}</span></td></tr>')
        keymap = ""
        if card.get("keys"):
            keymap = (f'<h2 class="small">Key numbers</h2><p class="lede">{e(card["keys"]["numbering"])}. '
                      f'Point at a key to read its switch position and light.</p>'
                      f'<div class="plate stage">{board_svg(card, "keymap", numbers=True)}'
                      f'<p class="keyinfo" id="keyinfo">{len(card["keys"]["grid"])} keys</p></div>')
        sources = "".join(f'<li><a href="{e(s["url"])}">{e(s["title"])}</a></li>' for s in card.get("sources", []))
        where = (f'<a class="mono" href="{e(self.repo)}/blob/main/{e(card["_path"])}">{e(card["_path"])}</a>'
                 if self.repo else f'<span class="mono">{e(card["_path"])}</span>')
        body = f"""<div class="wrap"><div class="page-head">
<p class="crumb"><a href="../devices.html">Devices</a> / {e(d["vendor"])}</p>
<h1>{e(d["name"])}</h1><p class="lede">{e(d["summary"])}</p>
<p class="lede"><span class="chip plain">{e(d["kind"])}</span> <span class="mono">{e(d["id"])}</span></p></div>
<h2 class="small">What it can do</h2>
<div class="tablebox"><table><tr><th>Capability</th><th>Kind</th><th>Risk</th><th>Route</th><th>Checked</th></tr>{''.join(rows)}</table></div>
{keymap}
{f'<h2 class="small">Before it will work</h2><div class="tablebox"><table><tr><th>Platform</th><th>Needs</th></tr>{requires}</table></div>' if requires else ''}
<h2 class="small">How it is recognised</h2>
<div class="tablebox"><table><tr><th>Bus</th><th>Identifiers</th></tr>{match_rows}</table></div>
{f'<h2 class="small">Sources</h2><ul>{sources}</ul>' if sources else ''}
<h2 class="small">The card</h2>
<p class="lede">This page is generated from {where}. If something here is wrong, or you have checked more of it, edit that file.</p>
<pre>{e(card["_source"])}</pre></div>"""
        return self.page(f'{d["vendor"]} {d["name"]} · {self.name}', body, "../", "Devices")

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
        body = f"""<div class="wrap"><div class="page-head"><p class="eyebrow">Catalog</p><h1>Vocabulary</h1>
<p class="lede">A capability id means the same thing on every device, so a rule or an AI can ask for
<span class="mono">battery.level</span> without knowing the hardware.</p></div>
<h2 class="small">Kinds</h2>{defs((f'<span class="mono">{e(k)}</span>', text) for k, text in c.kinds.items())}
<h2 class="small">Privacy</h2>{defs((f'<span class="chip plain">{e(k)}</span>', text) for k, text in c.privacy.items())}
<h2 class="small">Capabilities</h2>
<div class="tablebox"><table><tr><th>Capability</th><th>Kind</th><th>Lowest risk allowed</th><th>Devices</th></tr>{rows}</table></div></div>"""
        return self.page(f"Vocabulary · {self.name}", body, "", "Vocabulary")

    def adapters(self):
        c = self.c
        rows = ""
        for name, adapter in c.adapters.items():
            count = sum(1 for card in c.cards for cap in card["capability"] if cap["adapter"] == name)
            state = "st-tested" if adapter["status"] == "implemented" else "st-unverified"
            rows += (f'<tr><td><span class="mono">{e(name)}</span><span class="sub">{e(adapter["summary"])}</span>'
                     f'<span class="sub">{e(adapter.get("notes", ""))}</span></td>'
                     f'<td><span class="chip {state}">{e(adapter["status"])}</span><span class="sub">{e(adapter.get("implementation", ""))}</span></td>'
                     f'<td>{e(", ".join(adapter["platforms"]))}</td><td class="mono">{e(", ".join(adapter.get("params", [])) or "none")}</td>'
                     f"<td>{count}</td></tr>")
        body = f"""<div class="wrap"><div class="page-head"><p class="eyebrow">Catalog</p><h1>Adapters</h1>
<p class="lede">Adapters are the code that talks to hardware. They live in the daemon, not in the catalog.
A device card can only name an adapter and give it typed values; it can never carry a command or a script.</p></div>
<h2 class="small">{len(c.adapters)} adapters</h2>
<div class="tablebox"><table><tr><th>Adapter</th><th>Status</th><th>Platforms</th><th>Parameters</th><th>Capabilities using it</th></tr>{rows}</table></div></div>"""
        return self.page(f"Adapters · {self.name}", body, "", "Adapters")

    def format(self):
        c = self.c
        example = c.card(self.t["demo"]["card"]) or c.cards[0]
        parts = [("[device]", "What the device is: vendor, name, kind and a one-line summary."),
                 ("[[match]]", "How software recognises it when it is attached: bus and identifiers."),
                 ("[keys]", "For keyboards: how key numbers, switch positions and lights line up."),
                 ("[[requires]]", "What has to be true before any of it works, such as a permission or a closed program."),
                 ("[[capability]]", "One thing the device can do: a word from the vocabulary, the adapter that reaches it, its risk, and who has checked it."),
                 ("[[sources]]", "Where the information came from.")]
        body = f"""<div class="wrap"><div class="page-head"><p class="eyebrow">Catalog</p><h1>The card format</h1>
<p class="lede">One TOML file per device model, at <span class="mono">catalog/devices/&lt;vendor&gt;/&lt;model&gt;.toml</span>.
Cards are data only. The full rules are in <span class="mono">docs/SPEC.md</span> and
<span class="mono">catalog/schema/device.schema.json</span>.</p></div>
<h2 class="small">What a card says</h2>{defs((f'<span class="mono">{e(k)}</span>', text) for k, text in parts)}
<h2 class="small">Risk</h2>{defs((chip("risk", r), text) for r, text in c.risks.items())}
<h2 class="small">Checked</h2>{defs((chip("st", s), text) for s, text in c.statuses.items())}
<h2 class="small">An example</h2><p class="lede"><span class="mono">{e(example["_path"])}</span></p>
<pre>{e(example["_source"])}</pre></div>"""
        return self.page(f"Format · {self.name}", body, "", "Format")

    def write(self):
        c = self.c
        if SITE.exists():
            shutil.rmtree(SITE)
        (SITE / "devices").mkdir(parents=True)
        for name in ("style.css", "app.js"):
            shutil.copy(SOURCE / name, SITE / name)
        (SITE / "logo.svg").write_text(mark(TEXT, RISK_COLOR["guarded"]))
        (SITE / "index.html").write_text(self.landing())
        (SITE / "devices.html").write_text(self.devices())
        (SITE / "vocabulary.html").write_text(self.vocabulary())
        (SITE / "adapters.html").write_text(self.adapters())
        (SITE / "format.html").write_text(self.format())
        for card in c.cards:
            (SITE / "devices" / f"{slug(card)}.html").write_text(self.device(card))
        feed = {"format": 1, "risks": c.risks, "statuses": c.statuses, "privacy": c.privacy, "kinds": c.kinds,
                "vocabulary": c.vocabulary, "adapters": c.adapters,
                "devices": [{k: v for k, v in card.items() if not k.startswith("_")} for card in c.cards]}
        (SITE / "catalog.json").write_text(json.dumps(feed, indent=2) + "\n")
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
