#!/usr/bin/env python3
"""Validate the catalog, then build the static site and the JSON feed.

Standard library only, so anyone can run it:

    python3 tools/build.py            # validate and build ./site
    python3 tools/build.py --check    # validate only
"""
import datetime
import html
import json
import pathlib
import re
import shutil
import sys
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SITE = ROOT / "site"
NAME = "Hardpoint"
REPO = "https://github.com/dotCipher/hardpoint-catalog"
TAGLINE = "Hardware your AI can read and change, with the risk written down."

# Definitions shared by the validator and the site, so they cannot drift apart.
RISKS = {
    "read": "Nothing changes. The value is only read.",
    "reversible": "Changes take effect at once, can be undone at once, and cannot stop you using the machine.",
    "guarded": "Could get in the way of using the machine, so the change is reverted unless you confirm it within a few seconds.",
    "gated": "Cannot be undone by software, or needs a physical step. Software never does this alone; it only explains what to do.",
}
STATUSES = {
    "tested": "Someone exercised this through the named adapter on that platform.",
    "observed": "Seen working or present on a real device, but not yet through the adapter.",
    "imported": "Taken from another project's data. Nobody here has tried it.",
    "unverified": "Documented somewhere. Nobody here has tried it.",
}
PRIVACY = {
    "open": "The value can be passed on as it is.",
    "aggregate": "Only counts and coarse states may leave the adapter, never the raw stream.",
    "presence": "Only whether it is in use or present may leave the adapter, never the content.",
}
KINDS = {
    "reading": "A value you can read.",
    "event": "A stream of things that happen.",
    "control": "A value you can set.",
    "action": "Something you can trigger once.",
}
RISK_ORDER = list(RISKS)
STATUS_ORDER = list(STATUSES)
PRIVACY_ORDER = list(PRIVACY)


# ---------------------------------------------------------------- validation

def check_schema(schema, value, path, errors):
    """A small subset of JSON Schema: enough for schema/device.schema.json."""
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
    """TOML dates become ISO strings so cards are plain JSON."""
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


def load_catalog():
    errors = []
    schema = json.loads((ROOT / "schema" / "device.schema.json").read_text())
    vocabulary = load_toml(ROOT / "vocabulary" / "capabilities.toml")["capability"]
    adapters = load_toml(ROOT / "adapters" / "adapters.toml")["adapter"]

    for cap_id, cap in vocabulary.items():
        where = f"vocabulary {cap_id}"
        if cap.get("kind") not in KINDS:
            errors.append(f"{where}: kind must be one of {', '.join(KINDS)}")
        if cap.get("min_risk") not in RISKS:
            errors.append(f"{where}: min_risk must be one of {', '.join(RISKS)}")
        if cap.get("kind") in ("reading", "event") and cap.get("min_risk") != "read":
            errors.append(f"{where}: a {cap.get('kind')} can only be 'read'")
        if cap.get("kind") in ("control", "action") and cap.get("min_risk") == "read":
            errors.append(f"{where}: a {cap.get('kind')} changes something, so it cannot be 'read'")
        if "privacy" in cap and cap["privacy"] not in PRIVACY:
            errors.append(f"{where}: privacy must be one of {', '.join(PRIVACY)}")
    for name, adapter in adapters.items():
        if adapter.get("status") not in ("implemented", "planned"):
            errors.append(f"adapter {name}: status must be implemented or planned")

    cards = []
    for path in sorted((ROOT / "devices").glob("*/*.toml")):
        rel = path.relative_to(ROOT)
        try:
            card = load_toml(path)
        except tomllib.TOMLDecodeError as exc:
            errors.append(f"{rel}: {exc}")
            continue
        found = []
        check_schema(schema, card, "card", found)
        errors.extend(f"{rel}: {e}" for e in found)
        if found:
            continue
        expected_id = f"{path.parent.name}/{path.stem}"
        if card["device"]["id"] != expected_id:
            errors.append(f"{rel}: device.id must be {expected_id!r} to match its path")
        seen = set()
        for cap in card["capability"]:
            where = f"{rel}: capability {cap['id']}"
            if cap["id"] in seen:
                errors.append(f"{where}: listed twice")
            seen.add(cap["id"])
            word = vocabulary.get(cap["id"])
            if word is None:
                errors.append(f"{where}: not in vocabulary/capabilities.toml")
                continue
            adapter = adapters.get(cap["adapter"])
            if adapter is None:
                errors.append(f"{where}: unknown adapter {cap['adapter']!r}")
                continue
            if RISK_ORDER.index(cap["risk"]) < RISK_ORDER.index(word["min_risk"]):
                errors.append(f"{where}: risk {cap['risk']!r} is below the vocabulary minimum {word['min_risk']!r}")
            if word["kind"] in ("reading", "event") and cap["risk"] != "read":
                errors.append(f"{where}: a {word['kind']} can only be 'read'")
            if cap["risk"] == "gated" and cap["adapter"] != "manual":
                errors.append(f"{where}: gated capabilities must use the 'manual' adapter")
            if "privacy" in word:
                declared = cap.get("privacy", "open")
                if PRIVACY_ORDER.index(declared) < PRIVACY_ORDER.index(word["privacy"]):
                    errors.append(f"{where}: must declare privacy {word['privacy']!r} or stricter")
            for key in cap.get("params", {}):
                if key not in adapter.get("params", []):
                    errors.append(f"{where}: adapter {cap['adapter']!r} has no parameter {key!r}")
            if ("range" in cap or "choices" in cap) and word["kind"] != "control":
                errors.append(f"{where}: only a control can have a range or choices")
            for record in cap["verification"]:
                if record["platform"] not in adapter.get("platforms", []):
                    errors.append(f"{where}: adapter {cap['adapter']!r} does not run on {record['platform']}")
                if record["status"] == "tested" and adapter.get("status") != "implemented":
                    errors.append(f"{where}: cannot be 'tested' through adapter {cap['adapter']!r}, which is not implemented")
                if record["status"] in ("tested", "observed") and not (record.get("date") and record.get("by")):
                    errors.append(f"{where}: a {record['status']} record needs a date and who checked it")
        card["_path"] = str(rel)
        card["_source"] = path.read_text()
        cards.append(card)
    return cards, vocabulary, adapters, errors


def best_status(cap):
    return min((r["status"] for r in cap["verification"]), key=STATUS_ORDER.index)


# ---------------------------------------------------------------------- site

STYLE = """
:root{--bg:#0b0d10;--panel:#12161b;--line:#222a33;--text:#d7dde5;--muted:#8a94a3;
--read:#00d060;--reversible:#0090ff;--guarded:#ffa000;--gated:#ff3030;--accent:#ffa000}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--text);font:16px/1.55 system-ui,-apple-system,"Segoe UI",sans-serif}
a{color:var(--text);text-decoration-color:var(--muted);text-underline-offset:3px}
a:hover{text-decoration-color:var(--accent)}
code,pre,.mono{font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace;font-size:.86em}
header{border-bottom:1px solid var(--line)}
.wrap{max-width:1040px;margin:0 auto;padding:0 24px}
.bar{display:flex;align-items:center;gap:28px;height:64px}
.brand{display:flex;align-items:center;gap:10px;font-weight:650;letter-spacing:.01em;text-decoration:none}
.brand svg{width:26px;height:26px}
nav{display:flex;gap:20px;margin-left:auto;font-size:.94em}
nav a{text-decoration:none;color:var(--muted)}
nav a.on,nav a:hover{color:var(--text)}
main{padding:44px 0 72px}
h1{font-size:2.1rem;line-height:1.2;margin:0 0 12px;letter-spacing:-.01em}
h2{font-size:1.15rem;margin:44px 0 14px}
.lede{color:var(--muted);max-width:62ch;margin:0 0 8px}
.stats{display:flex;gap:36px;margin:30px 0 8px;flex-wrap:wrap}
.stat b{display:block;font-size:1.7rem;font-variant-numeric:tabular-nums}
.stat span{color:var(--muted);font-size:.9em}
input[type=search]{width:100%;background:var(--panel);border:1px solid var(--line);color:var(--text);
padding:11px 14px;border-radius:8px;font:inherit;margin:0 0 14px}
input[type=search]:focus{outline:2px solid var(--accent);outline-offset:1px}
.list{border:1px solid var(--line);border-radius:10px;overflow:hidden}
.row{display:grid;grid-template-columns:1fr auto;gap:8px 24px;padding:16px 18px;border-top:1px solid var(--line);
text-decoration:none;background:var(--panel)}
.row:first-child{border-top:0}
.row:hover{background:#171c23}
.row .name{font-weight:600}
.row .sub{color:var(--muted);font-size:.92em;grid-column:1}
.row .meta{grid-row:1/3;grid-column:2;align-self:center;display:flex;gap:6px;flex-wrap:wrap;justify-content:flex-end;max-width:340px}
table{width:100%;border-collapse:collapse;font-size:.93em}
.tablebox{border:1px solid var(--line);border-radius:10px;overflow-x:auto;background:var(--panel)}
th,td{text-align:left;padding:11px 14px;border-top:1px solid var(--line);vertical-align:top}
tr:first-child th{border-top:0;color:var(--muted);font-weight:500;font-size:.88em}
td .sub{display:block;color:var(--muted);font-size:.92em;margin-top:2px}
.chip{display:inline-block;padding:1px 9px;border-radius:99px;font-size:.8em;line-height:1.7;white-space:nowrap;
border:1px solid currentColor}
.risk-read{color:var(--read)}.risk-reversible{color:var(--reversible)}
.risk-guarded{color:var(--guarded)}.risk-gated{color:var(--gated)}
.st-tested{color:var(--bg);background:var(--text);border-color:var(--text)}
.st-observed{color:var(--text)}
.st-imported{color:var(--muted);border-style:dashed}
.st-unverified{color:var(--muted);border-color:var(--line)}
.plain{color:var(--muted);border-color:var(--line)}
dl.defs{display:grid;grid-template-columns:max-content 1fr;gap:10px 18px;margin:0}
dl.defs dt{margin:0}dl.defs dd{margin:0;color:var(--muted)}
pre{background:var(--panel);border:1px solid var(--line);border-radius:10px;padding:16px 18px;overflow-x:auto;line-height:1.5}
.crumb{color:var(--muted);font-size:.92em;margin:0 0 14px}
.crumb a{color:var(--muted)}
footer{border-top:1px solid var(--line);color:var(--muted);font-size:.9em;padding:22px 0}
@media (max-width:640px){.row{grid-template-columns:1fr}.row .meta{grid-row:auto;grid-column:1;justify-content:flex-start}
nav{gap:14px}h1{font-size:1.7rem}}
"""

# A mounting plate: four fixing holes and the point a load attaches to.
LOGO = (
    '<svg viewBox="0 0 32 32" aria-hidden="true"><rect x="3" y="3" width="26" height="26" rx="6" fill="none" '
    'stroke="#d7dde5" stroke-width="2"/><circle cx="16" cy="16" r="5.5" fill="none" stroke="#ffa000" stroke-width="2.5"/>'
    '<circle cx="9" cy="9" r="1.6" fill="#d7dde5"/><circle cx="23" cy="9" r="1.6" fill="#d7dde5"/>'
    '<circle cx="9" cy="23" r="1.6" fill="#d7dde5"/><circle cx="23" cy="23" r="1.6" fill="#d7dde5"/></svg>'
)

e = html.escape


def chip(kind, value):
    return f'<span class="chip {kind}-{e(value)}">{e(value)}</span>'


def page(title, body, root, active):
    links = [("Devices", "index.html"), ("Vocabulary", "vocabulary.html"), ("Adapters", "adapters.html"), ("Format", "format.html")]
    nav = "".join(
        f'<a href="{root}{href}"{" class=on" if label == active else ""}>{label}</a>' for label, href in links
    )
    return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{e(title)}</title><meta name="description" content="{e(TAGLINE)}">
<link rel="stylesheet" href="{root}style.css"></head><body>
<header><div class="wrap bar"><a class="brand" href="{root}index.html">{LOGO}<span>{NAME}</span></a><nav>{nav}<a href="{REPO}">GitHub</a></nav></div></header>
<main><div class="wrap">{body}</div></main>
<footer><div class="wrap">{NAME} catalog, format 1. Machine-readable copy: <a href="{root}catalog.json">catalog.json</a>.
<a href="{REPO}#adding-a-device">Add a device</a>.
Product names belong to their owners; this catalog is not affiliated with them.</div></footer>
</body></html>"""


def slug(card):
    return card["device"]["id"].replace("/", "-")


def hexid(value):
    return f"{value:04x}" if isinstance(value, int) else str(value)


def render_index(cards, vocabulary):
    caps = [c for card in cards for c in card["capability"]]
    tested = sum(1 for c in caps if best_status(c) == "tested")
    rows = []
    for card in cards:
        d = card["device"]
        counts = {s: sum(1 for c in card["capability"] if best_status(c) == s) for s in STATUS_ORDER}
        chips = "".join(
            f'<span class="chip st-{s}">{n} {s}</span>' for s, n in counts.items() if n
        )
        search = " ".join([d["name"], d["vendor"], d["kind"]] + [c["id"] for c in card["capability"]]).lower()
        rows.append(
            f'<a class="row" href="devices/{slug(card)}.html" data-search="{e(search)}">'
            f'<span class="name">{e(d["vendor"])} {e(d["name"])}</span>'
            f'<span class="sub">{e(d["kind"])} · {len(card["capability"])} capabilities · {e(d["summary"])}</span>'
            f'<span class="meta">{chips}</span></a>'
        )
    body = f"""
<h1>{e(TAGLINE)}</h1>
<p class="lede">{NAME} is an open catalog of what devices expose to software: what can be read, what can be changed,
by which route, and how risky each change is. Every entry says whether a person has actually tried it.</p>
<div class="stats">
<div class="stat"><b>{len(cards)}</b><span>devices</span></div>
<div class="stat"><b>{len(caps)}</b><span>capabilities listed</span></div>
<div class="stat"><b>{tested}</b><span>tested by a person</span></div>
<div class="stat"><b>{len(vocabulary)}</b><span>words in the vocabulary</span></div>
</div>
<h2>Devices</h2>
<input type="search" id="q" placeholder="Filter by name, vendor, kind or capability" aria-label="Filter devices">
<div class="list" id="list">{''.join(rows)}</div>
<h2>How much has been checked</h2>
<dl class="defs">{''.join(f'<dt>{chip("st", s)}</dt><dd>{e(text)}</dd>' for s, text in STATUSES.items())}</dl>
<h2>How risky a change is</h2>
<dl class="defs">{''.join(f'<dt>{chip("risk", r)}</dt><dd>{e(text)}</dd>' for r, text in RISKS.items())}</dl>
<script>
const q=document.getElementById('q'),rows=[...document.querySelectorAll('#list .row')];
q.addEventListener('input',()=>{{const t=q.value.trim().toLowerCase();
rows.forEach(r=>r.style.display=r.dataset.search.includes(t)?'':'none')}});
</script>"""
    return page(f"{NAME}: {TAGLINE}", body, "", "Devices")


def render_device(card, vocabulary, adapters):
    d = card["device"]
    match_rows = "".join(
        "<tr>" + f'<td>{e(m["bus"])}</td>'
        + f'<td class="mono">{e(" ".join(f"{k}={hexid(v)}" for k, v in m.items() if k != "bus"))}</td></tr>'
        for m in card["match"]
    )
    requires = "".join(
        f'<tr><td>{e(r["platform"])}</td><td>{e(r["what"])}<span class="sub">{e(r.get("note", ""))}</span></td></tr>'
        for r in card.get("requires", [])
    )
    cap_rows = []
    for cap in card["capability"]:
        word = vocabulary[cap["id"]]
        params = " ".join(f"{k}={hexid(v) if k in ('feature', 'vcp') else v}" for k, v in cap.get("params", {}).items())
        limits = ""
        if "range" in cap:
            limits = f'range {cap["range"][0]:g} to {cap["range"][1]:g}'
        record = min(cap["verification"], key=lambda r: STATUS_ORDER.index(r["status"]))
        detail = " · ".join(
            x for x in [
                record["platform"] + (" " + record["os"] if record.get("os") else ""),
                record.get("firmware", ""), record.get("date", ""), record.get("by", ""),
            ] if x
        )
        privacy = f'<span class="chip plain">{e(cap["privacy"])}</span>' if "privacy" in cap else ""
        cap_rows.append(
            f'<tr><td><span class="mono">{e(cap["id"])}</span><span class="sub">{e(word["summary"])}</span>'
            f'<span class="sub">{e(cap.get("note", ""))}</span></td>'
            f'<td>{e(word["kind"])}<span class="sub">{e(word["value"])}</span><span class="sub">{e(limits)}</span></td>'
            f'<td>{chip("risk", cap["risk"])} {privacy}</td>'
            f'<td><span class="mono">{e(cap["adapter"])}</span><span class="sub mono">{e(params)}</span>'
            f'<span class="sub">adapter {e(adapters[cap["adapter"]]["status"])}</span></td>'
            f'<td>{chip("st", record["status"])}<span class="sub">{e(detail)}</span>'
            f'<span class="sub">{e(record.get("note", ""))}</span></td></tr>'
        )
    sources = "".join(f'<li><a href="{e(s["url"])}">{e(s["title"])}</a></li>' for s in card.get("sources", []))
    body = f"""
<p class="crumb"><a href="../index.html">Devices</a> / {e(d["vendor"])}</p>
<h1>{e(d["name"])}</h1>
<p class="lede">{e(d["summary"])}</p>
<p class="lede"><span class="chip plain">{e(d["kind"])}</span> <span class="mono">{e(d["id"])}</span></p>
<h2>What it can do</h2>
<div class="tablebox"><table><tr><th>Capability</th><th>Kind</th><th>Risk</th><th>Route</th><th>Checked</th></tr>
{''.join(cap_rows)}</table></div>
{f'<h2>Before it will work</h2><div class="tablebox"><table><tr><th>Platform</th><th>Needs</th></tr>{requires}</table></div>' if requires else ''}
<h2>How it is recognised</h2>
<div class="tablebox"><table><tr><th>Bus</th><th>Identifiers</th></tr>{match_rows}</table></div>
{f'<h2>Sources</h2><ul>{sources}</ul>' if sources else ''}
<h2>The card</h2>
<p class="lede">This page is generated from <a class="mono" href="{REPO}/blob/main/{e(card["_path"])}">{e(card["_path"])}</a>.
If something here is wrong or you have checked more of it, edit that file.</p>
<pre>{e(card["_source"])}</pre>"""
    return page(f'{d["vendor"]} {d["name"]} · {NAME}', body, "../", "Devices")


def render_vocabulary(vocabulary, cards):
    used = {}
    for card in cards:
        for cap in card["capability"]:
            used.setdefault(cap["id"], []).append(card)
    rows = ""
    for cid, w in vocabulary.items():
        privacy = f'<span class="chip plain">{e(w["privacy"])}</span>' if "privacy" in w else ""
        devices = ", ".join(
            f'<a href="devices/{slug(c)}.html">{e(c["device"]["name"])}</a>' for c in used.get(cid, [])
        ) or '<span class="sub">none yet</span>'
        rows += (
            f'<tr><td><span class="mono">{e(cid)}</span><span class="sub">{e(w["summary"])}</span></td>'
            f'<td>{e(w["kind"])}<span class="sub">{e(w["value"])}</span></td>'
            f'<td>{chip("risk", w["min_risk"])} {privacy}</td><td>{devices}</td></tr>'
        )
    body = f"""
<h1>Vocabulary</h1>
<p class="lede">A capability id means the same thing on every device, so a rule or an AI can ask for
<span class="mono">battery.level</span> without knowing the hardware.</p>
<h2>Kinds</h2><dl class="defs">{''.join(f'<dt class="mono">{e(k)}</dt><dd>{e(t)}</dd>' for k, t in KINDS.items())}</dl>
<h2>Privacy</h2><dl class="defs">{''.join(f'<dt><span class="chip plain">{e(k)}</span></dt><dd>{e(t)}</dd>' for k, t in PRIVACY.items())}</dl>
<h2>Capabilities</h2>
<div class="tablebox"><table><tr><th>Capability</th><th>Kind</th><th>Lowest risk allowed</th><th>Devices</th></tr>{rows}</table></div>"""
    return page(f"Vocabulary · {NAME}", body, "", "Vocabulary")


def render_adapters(adapters, cards):
    rows = []
    for name, a in adapters.items():
        count = sum(1 for card in cards for cap in card["capability"] if cap["adapter"] == name)
        rows.append(
            f'<tr><td><span class="mono">{e(name)}</span><span class="sub">{e(a["summary"])}</span>'
            f'<span class="sub">{e(a.get("notes", ""))}</span></td>'
            f'<td><span class="chip {"st-tested" if a["status"] == "implemented" else "st-unverified"}">{e(a["status"])}</span>'
            f'<span class="sub">{e(a.get("implementation", ""))}</span></td>'
            f'<td>{e(", ".join(a["platforms"]))}</td><td class="mono">{e(", ".join(a.get("params", [])) or "none")}</td>'
            f"<td>{count}</td></tr>"
        )
    body = f"""
<h1>Adapters</h1>
<p class="lede">Adapters are the code that talks to hardware. They live in the daemon, not in the catalog.
A device card can only name an adapter and give it typed parameters; it can never carry a command or a script.</p>
<div class="tablebox"><table><tr><th>Adapter</th><th>Status</th><th>Platforms</th><th>Parameters</th><th>Capabilities using it</th></tr>
{''.join(rows)}</table></div>"""
    return page(f"Adapters · {NAME}", body, "", "Adapters")


def render_format(cards):
    example = next((c for c in cards if c["device"]["id"] == "zsa/moonlander-mk1"), cards[0])
    body = f"""
<h1>The card format</h1>
<p class="lede">One TOML file per device model, at <span class="mono">devices/&lt;vendor&gt;/&lt;model&gt;.toml</span>.
Cards are data only. The full rules are in <span class="mono">SPEC.md</span> and
<span class="mono">schema/device.schema.json</span>.</p>
<h2>What a card says</h2>
<dl class="defs">
<dt class="mono">[device]</dt><dd>What the device is: vendor, name, kind and a one-line summary.</dd>
<dt class="mono">[[match]]</dt><dd>How software recognises it when it is plugged in: bus and identifiers.</dd>
<dt class="mono">[[requires]]</dt><dd>What has to be true before any of it works, such as a permission or a closed program.</dd>
<dt class="mono">[[capability]]</dt><dd>One thing the device can do: a word from the vocabulary, the adapter that reaches it, its risk, and who has checked it.</dd>
<dt class="mono">[[sources]]</dt><dd>Where the information came from.</dd>
</dl>
<h2>Risk</h2><dl class="defs">{''.join(f'<dt>{chip("risk", r)}</dt><dd>{e(t)}</dd>' for r, t in RISKS.items())}</dl>
<h2>Checked</h2><dl class="defs">{''.join(f'<dt>{chip("st", s)}</dt><dd>{e(t)}</dd>' for s, t in STATUSES.items())}</dl>
<h2>An example</h2>
<p class="lede"><span class="mono">{e(example["_path"])}</span></p>
<pre>{e(example["_source"])}</pre>"""
    return page(f"Format · {NAME}", body, "", "Format")


def build(cards, vocabulary, adapters):
    if SITE.exists():
        shutil.rmtree(SITE)
    (SITE / "devices").mkdir(parents=True)
    (SITE / "style.css").write_text(STYLE.strip() + "\n")
    (SITE / "index.html").write_text(render_index(cards, vocabulary))
    (SITE / "vocabulary.html").write_text(render_vocabulary(vocabulary, cards))
    (SITE / "adapters.html").write_text(render_adapters(adapters, cards))
    (SITE / "format.html").write_text(render_format(cards))
    for card in cards:
        (SITE / "devices" / f"{slug(card)}.html").write_text(render_device(card, vocabulary, adapters))
    feed = {
        "format": 1,
        "risks": RISKS,
        "statuses": STATUSES,
        "privacy": PRIVACY,
        "vocabulary": vocabulary,
        "adapters": adapters,
        "devices": [{k: v for k, v in card.items() if not k.startswith("_")} for card in cards],
    }
    (SITE / "catalog.json").write_text(json.dumps(feed, indent=2) + "\n")


def main():
    cards, vocabulary, adapters, errors = load_catalog()
    if errors:
        print(f"{len(errors)} problem(s):", file=sys.stderr)
        for error in errors:
            print(f"  {error}", file=sys.stderr)
        return 1
    caps = [c for card in cards for c in card["capability"]]
    counts = {s: sum(1 for c in caps if best_status(c) == s) for s in STATUS_ORDER}
    print(f"valid: {len(cards)} devices, {len(caps)} capabilities ({', '.join(f'{n} {s}' for s, n in counts.items())})")
    if "--check" not in sys.argv:
        build(cards, vocabulary, adapters)
        print(f"built {SITE.relative_to(ROOT)}/ ({sum(1 for _ in SITE.rglob('*') if _.is_file())} files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
