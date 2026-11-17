"""Shared public-identity normalization. Never infer names from private labels.

Python 3.9 compatible; all maker aliases and category labels live in the registry.
"""

import json
import pathlib
import re
import unicodedata


REGISTRY = pathlib.Path(__file__).resolve().parent.parent / "catalog/vocabulary/devices.json"


def registry():
    return json.loads(REGISTRY.read_text())


def clean(value):
    value = str(value).translate(str.maketrans("", "", "™®©"))
    value = unicodedata.normalize("NFKC", value)
    if any(unicodedata.category(c) in ("Cc", "Cf", "Cs") for c in value):
        raise ValueError("public names cannot contain control, invisible or directional-format characters")
    value = " ".join(value.split()).strip()
    if not value or any(c in value for c in "<>") or re.match(r"^(todo|unknown|vendor|device|model)(?::|$)", value, re.I):
        raise ValueError("a nonempty public name is required; placeholders and markup are not public identities")
    return value


def key(value):
    return "".join(c for c in unicodedata.normalize("NFKC", value).casefold().replace("+", "plus") if c.isalnum())


def slug(value):
    value = clean(value).replace("+", " plus ").replace("&", " and ")
    value = unicodedata.normalize("NFKD", value.casefold())
    if any(ord(c) > 127 and c.isalnum() for c in value):
        raise ValueError("a reviewed ASCII public identifier is needed; do not silently discard non-Latin model letters")
    value = value.encode("ascii", "ignore").decode().lower()
    value = re.sub(r"[^a-z0-9]+", "-", value).strip("-")
    if not value:
        raise ValueError("a reviewed ASCII identifier is needed for this public name")
    return value


def maker(value, data):
    value = clean(value)
    matches = [(id, m["name"]) for id, m in data["makers"].items()
               if key(value) in {key(v) for v in [id, m["name"], *m.get("aliases", [])]}]
    if len(matches) > 1:
        raise ValueError("maker alias is ambiguous in the registry")
    return matches[0] if matches else (slug(value), value)


def category(value, data):
    value = clean(value)
    matches = [id for id, c in data["categories"].items()
               if key(value) in {key(v) for v in [id, c["label"], *c.get("aliases", [])]}]
    if len(matches) != 1:
        raise ValueError("choose an unambiguous category from the registry")
    return matches[0]


def product_name(value, vendor, data):
    value = clean(value)
    id, canonical = maker(vendor, data)
    aliases = [canonical, *data["makers"].get(id, {}).get("aliases", [])]
    # Strip only a whole maker prefix, never a substring in a model name.
    for alias in sorted(aliases, key=len, reverse=True):
        prefix = clean(alias)
        if value.casefold().startswith(prefix.casefold() + " "):
            value = value[len(prefix):].strip(" -–—:")
            break
    return clean(value)


def full_name(device):
    vendor, name = device["vendor"], device["name"]
    return name if key(name) == key(vendor) or name.casefold().startswith(vendor.casefold() + " ") else vendor + " " + name


def normalize(vendor, name, kind, identity, cards=(), existing_id=None, data=None, model_code=None):
    data = data or registry()
    vendor_id, vendor = maker(vendor, data)
    name = product_name(name, vendor, data)
    supplied_kind = kind
    kind = category(kind, data)
    preferred_icon = next((icon for alias, icon in data["categories"][kind].get("alias_icons", {}).items()
                           if key(alias) == key(supplied_kind)), None)
    if identity not in data["identity"]:
        raise ValueError("choose the identity scope from the registry")
    if model_code is not None:
        model_code = clean(model_code)
    candidates = [c for c in cards if c["device"]["id"].split("/")[0] == vendor_id
                  and key(name) in {key(product_name(v, vendor, data)) for v in [c["device"]["name"], *c["device"].get("aliases", [])]}]
    if model_code and not existing_id:
        candidates = [c for c in candidates if model_code.casefold() in
                      {clean(v).casefold() for v in [*c["device"].get("aliases", []), *(m["model"] for m in c.get("match", []) if m.get("model"))]}]
    if existing_id:
        candidates = [c for c in cards if c["device"]["id"] == existing_id]
        if not candidates or existing_id.split("/")[0] != vendor_id:
            raise ValueError("an existing id must resolve to a card in the selected maker namespace")
    if len(candidates) > 1:
        raise ValueError("several registry cards share that public name; review their matching evidence and select an existing id")
    if candidates:
        d = candidates[0]["device"]
        if kind != d["kind"] and not existing_id:
            raise ValueError("the selected category disagrees with the existing card; review it rather than creating a duplicate")
        if existing_id:
            result = {"id": existing_id, "vendor": vendor, "name": name, "kind": kind, "identity": identity}
            icon = preferred_icon or d.get("icon")
            if icon in [data["categories"][kind]["icon"], *data["categories"][kind].get("icons", [])]:
                result["icon"] = icon
        else:
            result = {k: d[k] for k in ("id", "vendor", "name", "kind")}
            result["identity"] = d.get("identity", identity)
            if d.get("icon"):
                result["icon"] = d["icon"]
        reused = True
    else:
        try:
            identifier = slug(name) + ("-" + slug(model_code) if model_code else "")
        except ValueError:
            if not model_code:
                raise
            identifier = slug(model_code)
        result = {"id": vendor_id + "/" + identifier, "vendor": vendor, "name": name, "kind": kind, "identity": identity}
        if any(c["device"]["id"] == result["id"] for c in cards):
            raise ValueError("the proposed id already exists with a different public identity; review that card explicitly")
        if preferred_icon:
            result["icon"] = preferred_icon
        reused = False
    if model_code:
        result["aliases"] = list(dict.fromkeys([*(candidates[0]["device"].get("aliases", []) if candidates else []), model_code]))
    return {"device": result, "existing_card": reused, "maker_registration_needed": vendor_id not in data["makers"],
            "review_required": True}
