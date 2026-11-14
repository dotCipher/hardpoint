#!/usr/bin/env python3
"""List the devices attached to this computer and say which have a card.

Standard library only, and it runs on the Python that ships with macOS.
It reads; it never changes a device.

    python3 tools/observe.py              # list devices, numbered
    python3 tools/observe.py --json       # the same, for a program to read
    python3 tools/observe.py --draft 3    # print a draft card for device 3

It knows how to look on macOS and Linux. Each device is reported with the bus
it is on and the identifiers a card's [[match]] section needs.
"""
import datetime
import json
import pathlib
import platform
import re
import subprocess
import sys

try:
    import tomllib
except ModuleNotFoundError:  # before Python 3.11
    tomllib = None

ROOT = pathlib.Path(__file__).resolve().parent.parent
DEVICES = ROOT / "catalog" / "devices"


def run(*command):
    try:
        return subprocess.run(command, capture_output=True, text=True, timeout=30).stdout
    except (OSError, subprocess.SubprocessError):
        return ""


def number(text):
    """'0x046D', '046d' or '0x046D (Logitech)' -> 0x046d."""
    found = re.match(r"\s*(?:0x)?([0-9a-fA-F]{1,4})\b", str(text or ""))
    return int(found.group(1), 16) if found else None


def seen(bus, name, vendor_id=None, product_id=None, vendor="", model=None):
    return {"bus": bus, "name": (name or "").strip(), "vendor": (vendor or "").strip(),
            "vendor_id": vendor_id, "product_id": product_id, "model": model}


def walk(node):
    if isinstance(node, dict):
        yield node
        for value in node.values():
            yield from walk(value)
    elif isinstance(node, list):
        for value in node:
            yield from walk(value)


def macos():
    found = []
    model = run("/usr/sbin/sysctl", "-n", "hw.model").strip()
    if model:
        found.append(seen("builtin", model, vendor="Apple", model=model))
    try:
        report = json.loads(run("/usr/sbin/system_profiler", "-json", "SPUSBHostDataType", "SPUSBDataType",
                                "SPBluetoothDataType", "SPDisplaysDataType") or "{}")
    except json.JSONDecodeError:
        report = {}
    for kind in ("SPUSBHostDataType", "SPUSBDataType"):
        for node in walk(report.get(kind, [])):
            vendor_id = number(node.get("USBDeviceKeyVendorID") or node.get("vendor_id"))
            product_id = number(node.get("USBDeviceKeyProductID") or node.get("product_id"))
            if vendor_id is not None and product_id is not None:
                found.append(seen("usb", node.get("_name"), vendor_id, product_id,
                                  node.get("USBDeviceKeyVendorName") or node.get("manufacturer")))
    for section in report.get("SPBluetoothDataType", []):
        for group in ("device_connected", "device_not_connected"):
            for entry in section.get(group, []):
                for name, facts in entry.items():
                    vendor_id, product_id = number(facts.get("device_vendorID")), number(facts.get("device_productID"))
                    if vendor_id is not None and product_id is not None:
                        found.append(seen("bluetooth", name, vendor_id, product_id))
    for node in walk(report.get("SPDisplaysDataType", [])):
        vendor_id = number(node.get("_spdisplays_display-vendor-id"))
        product_id = number(node.get("_spdisplays_display-product-id"))
        if vendor_id is not None and product_id is not None and node.get("spdisplays_connection_type") != "spdisplays_internal":
            found.append(seen("display", node.get("_name"), vendor_id, product_id))
    return found


def linux():
    found = []
    read = lambda path: path.read_text().strip() if path.exists() else ""
    model = read(pathlib.Path("/sys/class/dmi/id/product_name"))
    if model:
        found.append(seen("builtin", model, vendor=read(pathlib.Path("/sys/class/dmi/id/sys_vendor")), model=model))
    for device in sorted(pathlib.Path("/sys/bus/usb/devices").glob("*")):
        vendor_id, product_id = number(read(device / "idVendor")), number(read(device / "idProduct"))
        if vendor_id is None or product_id is None or vendor_id == 0x1d6b:  # 1d6b is the computer's own USB hubs
            continue
        found.append(seen("usb", read(device / "product"), vendor_id, product_id, read(device / "manufacturer")))
    for line in run("bluetoothctl", "devices").splitlines():
        parts = line.split(" ", 2)
        if len(parts) < 3:
            continue
        info = run("bluetoothctl", "info", parts[1])
        ids = re.search(r"Modalias: \w+:v([0-9A-Fa-f]{4})p([0-9A-Fa-f]{4})", info)
        if ids:
            found.append(seen("bluetooth", parts[2], int(ids.group(1), 16), int(ids.group(2), 16)))
    return found


def observe():
    system = platform.system()
    if system == "Darwin":
        found = macos()
    elif system == "Linux":
        found = linux()
    else:
        sys.exit(f"observe.py does not know how to look on {system} yet. Open an issue with your device's name and identifiers.")
    unique, keys = [], set()
    for device in found:
        key = (device["bus"], device["vendor_id"], device["product_id"], device["model"])
        if key not in keys:
            keys.add(key)
            unique.append(device)
    return unique


def read_card(path):
    if tomllib:
        with path.open("rb") as handle:
            return tomllib.load(handle)
    # An older Python has no TOML reader. Only the device's id and its
    # [[match]] sections are needed here, and those are simple lines.
    card, section = {"device": {}, "match": []}, None
    for line in path.read_text().splitlines():
        line = line.strip()
        if line.startswith("["):
            section = line.strip("[]")
            if section == "match":
                card["match"].append({})
            continue
        found = re.match(r'(\w+)\s*=\s*(?:"([^"]*)"|(0x[0-9a-fA-F]+|\d+))\s*(?:#.*)?$', line)
        if found and section in ("device", "match"):
            target = card["device"] if section == "device" else card["match"][-1]
            target[found.group(1)] = found.group(2) if found.group(2) is not None else int(found.group(3), 0)
    return card


def cards():
    return [read_card(path) for path in sorted(DEVICES.glob("*/*.toml"))]


def card_for(device, catalog):
    for card in catalog:
        for match in card.get("match", []):
            if match.get("bus") != device["bus"]:
                continue
            if all(match.get(k) is None or match.get(k) == device.get(k) for k in ("vendor_id", "product_id", "model")):
                return card["device"]["id"]
    return None


def slug(text):
    return re.sub(r"-+", "-", re.sub(r"[^a-z0-9]+", "-", (text or "").lower())).strip("-") or "unknown"


def draft(device):
    vendor = device["vendor"] or "Vendor"
    vendor_slug, model_slug = slug(vendor.split(",")[0]), slug(device["name"])
    here = "macos" if platform.system() == "Darwin" else "linux"
    version = platform.mac_ver()[0] if here == "macos" else platform.release()
    today = datetime.date.today().isoformat()
    if device["bus"] == "builtin":
        match = f'bus = "builtin"\nmodel = "{device["model"]}"'
    else:
        match = f'bus = "{device["bus"]}"\nvendor_id = 0x{device["vendor_id"]:04x}\nproduct_id = 0x{device["product_id"]:04x}'
    return f"""# Save this as catalog/devices/{vendor_slug}/{model_slug}.toml, then fill in
# every line marked TODO. The rules are in docs/SPEC.md.
card = 1

[device]
id = "{vendor_slug}/{model_slug}"
vendor = "{vendor}"
name = "{device["name"]}"
kind = "other"          # TODO: keyboard, mouse, laptop, desktop, display, dock, audio, cooling, lighting, storage or other
summary = "TODO: one sentence saying what it is."

[[match]]
{match}

# TODO: one [[capability]] for each thing the device can do. Use only ids
# from catalog/vocabulary/capabilities.toml and adapters from
# catalog/adapters/adapters.toml. Never put a command or a path in a card.
#
# [[capability]]
# id = "battery.level"
# adapter = "macos-system"
# risk = "read"
# verification = [{{ status = "unverified", platform = "{here}", note = "Where this claim comes from." }}]
#
# Use "observed" only for something you ran on this computer and saw work,
# and say what you ran:
#
# verification = [{{ status = "observed", platform = "{here}", os = "{version}", date = {today}, note = "What you ran, and what it showed." }}]
"""


def main():
    devices = observe()
    catalog = cards()
    for device in devices:
        device["card"] = card_for(device, catalog)
    if "--json" in sys.argv:
        print(json.dumps(devices, indent=2))
        return 0
    if "--draft" in sys.argv:
        try:
            device = devices[int(sys.argv[sys.argv.index("--draft") + 1]) - 1]
        except (IndexError, ValueError):
            sys.exit("give the number of a device from the list, for example: --draft 3")
        if device["card"]:
            sys.exit(f'{device["name"]} already has a card: {device["card"]}')
        print(draft(device))
        return 0
    print(f"{len(devices)} devices found. Displays are only looked for on macOS.\n")
    for index, device in enumerate(devices, 1):
        ids = f'{device["vendor_id"]:04x}:{device["product_id"]:04x}' if device["vendor_id"] is not None else device["model"]
        state = f'has a card: {device["card"]}' if device["card"] else "no card yet"
        print(f'{index:>3}  {device["bus"]:<9} {ids:<10} {device["name"] or "(no name)":<34} {state}')
    missing = sum(1 for device in devices if not device["card"])
    if missing:
        print(f"\n{missing} with no card. To start one: python3 tools/observe.py --draft <number>")
    return 0


if __name__ == "__main__":
    sys.exit(main())
