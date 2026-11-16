#!/usr/bin/env python3
"""List the devices attached to this computer and say which have a card.

Standard library only, and it runs on the Python that ships with macOS.
It reads; it never changes a device.

    python3 tools/observe.py              # list devices, numbered
    python3 tools/observe.py --json       # the same, for a program to read
    python3 tools/observe.py --draft 3    # print a draft card for device 3
    python3 tools/observe.py --no-network # skip the home network
    python3 tools/observe.py --json > <private-snapshot-path>
    python3 tools/observe.py --from-file <private-snapshot-path> --select missing --json
    python3 tools/observe.py --from-file <private-snapshot-path> --select 1,3-5 --json

It knows how to look on macOS and Linux. Each device is reported with the bus
it is on and the identifiers a card's [[match]] section needs.

Things on the home network, such as a television or a smart plug, are found
by listening for a few seconds to what they announce. Nothing is sent to
them. They are reported by the model they announce, never by an address or
by the name someone gave them.
"""
import concurrent.futures
import argparse
import datetime
import json
import os
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


def seen(bus, name, vendor_id=None, product_id=None, vendor="", model=None, service=None):
    return {"bus": bus, "name": (name or "").strip(), "vendor": (vendor or "").strip(),
            "vendor_id": vendor_id, "product_id": product_id, "model": model, "service": service}


# The announcements smart-home devices commonly make. Any service a card in
# the catalog names is listened for as well.
SERVICES = ("_hap._tcp", "_matter._tcp", "_matterc._udp", "_airplay._tcp", "_googlecast._tcp", "_hue._tcp", "_sonos._tcp")


def listen(command, seconds):
    """Run something that keeps listening, and return what it printed in that time."""
    try:
        return subprocess.run(command, capture_output=True, text=True, timeout=seconds).stdout
    except subprocess.TimeoutExpired as exc:
        out = exc.stdout or ""
        return out.decode(errors="replace") if isinstance(out, bytes) else out
    except OSError:
        return ""


def announced(service):
    """What each device announcing `service` says about itself, as a list of dictionaries."""
    if platform.system() == "Darwin":
        names = []
        for line in listen(["dns-sd", "-B", service, "local."], 4).splitlines():
            found = re.match(r"\s*\S+\s+Add\s+\d+\s+\d+\s+\S+\s+\S+\s+(.+)$", line)
            if found and found.group(1).strip() not in names:
                names.append(found.group(1).strip())
        texts = [listen(["dns-sd", "-L", name, service, "local."], 3) for name in names]
        return [dict((k, v.replace("\\ ", " ")) for k, v in re.findall(r"(\w+)=((?:\\ |[^\s])*)", text)) for text in texts]
    lines = listen(["avahi-browse", "--resolve", "--parsable", "--terminate", service], 6).splitlines()
    return [dict(re.findall(r'"(\w+)=([^"]*)"', line)) for line in lines if line.startswith("=")]


def network(services, own_model):
    found = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=len(services) or 1) as pool:
        for service, devices in zip(services, pool.map(announced, services)):
            for said in devices:
                model = said.get("md") or said.get("model") or said.get("am")
                ids = re.match(r"(\d+)\+(\d+)$", said.get("VP", ""))  # Matter: vendor and product numbers
                if model == own_model or not (model or ids):
                    continue
                found.append(seen("network", model or "Matter device", int(ids.group(1)) if ids else None,
                                  int(ids.group(2)) if ids else None, said.get("manufacturer", ""), model, service))
    return found


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


def observe(catalog, look_on_network=True):
    system = platform.system()
    if system == "Darwin":
        found = macos()
    elif system == "Linux":
        found = linux()
    else:
        sys.exit(f"observe.py does not know how to look on {system} yet. Open an issue with your device's name and identifiers.")
    if look_on_network:
        named = [match["service"] for card in catalog for match in card.get("match", []) if match.get("service")]
        own = next((device["model"] for device in found if device["bus"] == "builtin"), None)
        found += network(sorted(set(SERVICES) | set(named)), own)
    unique, keys = [], set()
    for device in found:
        key = (device["bus"], device["vendor_id"], device["product_id"], device["model"], device["service"])
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
    # Personal cards participate in matching but are never copied or published.
    own = pathlib.Path(os.environ.get("XDG_CONFIG_HOME") or pathlib.Path.home() / ".config") / "hardpoint" / "cards"
    found = {}
    for scope, directory in (("shared", DEVICES), ("private", own)):
        for path in sorted(directory.glob("*/*.toml")):
            card = read_card(path)
            card["_scope"] = scope
            found[card["device"]["id"]] = card
    return list(found.values())


def card_for(device, catalog):
    card = matching_card(device, catalog)
    return card["device"]["id"] if card else None


def matching_card(device, catalog):
    for card in catalog:
        for match in card.get("match", []):
            if match.get("bus") != device["bus"]:
                continue
            if all(match.get(k) is None or match.get(k) == device.get(k) for k in ("vendor_id", "product_id", "model", "service")):
                return card
    return None


def slug(text):
    return re.sub(r"-+", "-", re.sub(r"[^a-z0-9]+", "-", (text or "").lower())).strip("-") or "unknown"


def draft_destination(device):
    vendor = device["vendor"] or "Vendor"
    # Friendly USB/Bluetooth names may identify their owner. Never use them as
    # public names, ids or filenames; a person supplies the public model name.
    model = device["model"] or "model-{}-{}".format(
        format(device["vendor_id"], "04x") if device["vendor_id"] is not None else "unknown",
        format(device["product_id"], "04x") if device["product_id"] is not None else "unknown")
    return "{}/{}".format(slug(vendor.split(",")[0]), slug(model))


def draft(device):
    vendor = device["vendor"] or "TODO: public maker"
    identity = draft_destination(device)
    quote = lambda value: json.dumps(value, ensure_ascii=False)
    here = {"Darwin": "macos", "Linux": "linux", "Windows": "windows"}.get(platform.system())
    if not here:
        raise ValueError("choose a verification platform from the schema before drafting on this operating system")
    version = platform.mac_ver()[0] if here == "macos" else platform.release()
    today = datetime.date.today().isoformat()
    if device["bus"] == "builtin":
        if not device["model"]:
            raise ValueError("this observation needs a public model identifier before a draft can be made")
        match = f'bus = "builtin"\nmodel = {quote(device["model"])}'
    elif device["bus"] == "network":
        if not device["service"] or not (device["model"] or device["vendor_id"] is not None and device["product_id"] is not None):
            raise ValueError("this observation needs a service and public model or vendor/product identifiers")
        match = f'bus = "network"\nservice = {quote(device["service"])}'
        match += f'\nmodel = {quote(device["model"])}' if device["model"] else ""
        if device["vendor_id"] is not None and device["product_id"] is not None:
            match += f'\nvendor_id = 0x{device["vendor_id"]:04x}\nproduct_id = 0x{device["product_id"]:04x}'
    else:
        if device["vendor_id"] is None or device["product_id"] is None:
            raise ValueError("this observation needs public vendor/product identifiers before a draft can be made")
        match = f'bus = {quote(device["bus"])}\nvendor_id = 0x{device["vendor_id"]:04x}\nproduct_id = 0x{device["product_id"]:04x}'
    return f"""# Save this as catalog/devices/{identity}.toml, then fill in
# every line marked TODO. The rules are in docs/SPEC.md.
card = 1

[device]
id = "{identity}"
vendor = {quote(vendor)}
name = {quote(device["model"] or "TODO: public model name")}
kind = "TODO"          # Choose a kind from the registry/schema.
summary = "TODO: one sentence saying what it is."

[[match]]
{match}

# TODO: one [[capability]] for each thing the device can do. Use only ids
# from catalog/vocabulary/capabilities.toml and adapters from
# catalog/adapters/adapters.toml. Never put a command or a path in a card.
#
# [[capability]]
# id = "<ability id from the registry>"
# adapter = "<adapter id from the registry>"
# risk = "<label at or above the vocabulary floor>"
# verification = [{{ status = "unverified", platform = "{here}", note = "Where this claim comes from." }}]
#
# Use "observed" only for something you ran on this computer and saw work,
# and say what you ran:
#
# verification = [{{ status = "observed", platform = "{here}", os = "{version}", date = {today}, note = "What you ran, and what it showed." }}]
"""


def snapshot(path):
    """Reload a private discovery result as data, never as code or commands."""
    rows = json.loads(pathlib.Path(path).read_text())
    if not isinstance(rows, list):
        raise ValueError("a discovery snapshot must be the JSON array from observe.py --json")
    result = []
    for index, row in enumerate(rows, 1):
        if not isinstance(row, dict) or not isinstance(row.get("bus"), str):
            raise ValueError("each snapshot entry must be a device observation")
        if "number" in row and (type(row["number"]) is not int or row["number"] != index):
            raise ValueError("use the original unfiltered snapshot; do not renumber a filtered selection export")
        for key in ("name", "vendor", "model", "service"):
            if row.get(key) is not None and not isinstance(row[key], str):
                raise ValueError("snapshot text fields must be strings")
        for key in ("vendor_id", "product_id"):
            value = row.get(key)
            if value is not None and (isinstance(value, bool) or not isinstance(value, int) or not 0 <= value <= 0xffff):
                raise ValueError("snapshot vendor/product identifiers must be unsigned 16-bit numbers")
        result.append(seen(row["bus"], row.get("name"), row.get("vendor_id"), row.get("product_id"),
                           row.get("vendor"), row.get("model"), row.get("service")))
    return result


def select(devices, selection):
    numbered = list(enumerate(devices, 1))
    if selection == "all":
        return numbered
    if selection == "missing":
        return [(n, d) for n, d in numbered if not d.get("card")]
    chosen = set()
    for part in selection.split(","):
        match = re.fullmatch(r"\s*(\d+)(?:\s*-\s*(\d+))?\s*", part)
        if not match:
            raise ValueError("select all, missing, or snapshot numbers/ranges such as 1,3-5")
        start, end = int(match[1]), int(match[2] or match[1])
        if not 1 <= start <= end <= len(devices):
            raise ValueError("selection numbers must refer to entries in this discovery snapshot")
        chosen.update(range(start, end + 1))
    return [(n, d) for n, d in numbered if n in chosen]


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--no-network", action="store_true", help="skip network discovery")
    parser.add_argument("--from-file", metavar="PATH", help="reuse a private JSON discovery snapshot without rescanning")
    parser.add_argument("--select", metavar="SELECTION", help="all, missing, or snapshot numbers/ranges such as 1,3-5")
    output = parser.add_mutually_exclusive_group()
    output.add_argument("--json", action="store_true", help="print selected observations as JSON")
    output.add_argument("--draft", type=int, metavar="NUMBER", help="print a draft for an uncatalogued snapshot entry; writes no files")
    args = parser.parse_args(argv)
    catalog = cards()
    if args.draft is not None and args.select is not None:
        parser.error("use --select to review entries, then --draft with one original snapshot number")
    try:
        devices = snapshot(args.from_file) if args.from_file is not None else observe(catalog, not args.no_network)
    except (ValueError, OSError) as error:
        parser.error(str(error))
    for device in devices:
        match = matching_card(device, catalog)
        device["card"] = match["device"]["id"] if match else None
        device["card_scope"] = match.get("_scope", "shared") if match else None
    try:
        chosen = select(devices, args.select if args.select is not None else "all")
    except ValueError as error:
        parser.error(str(error))
    if args.json:
        print(json.dumps([dict(device, number=n) for n, device in chosen], indent=2))
        return 0
    if args.draft is not None:
        if not 1 <= args.draft <= len(devices):
            parser.error("give an original device number from this discovery snapshot")
        device = devices[args.draft - 1]
        if device["card"]:
            sys.exit(f'{device["name"]} already has a card: {device["card"]}')
        try:
            print(draft(device))
        except ValueError as error:
            parser.error(str(error))
        return 0
    print(f"{len(chosen)} selected from {len(devices)} discovered device models.\n")
    for index, device in chosen:
        ids = (device["service"] if device["bus"] == "network" else
               f'{device["vendor_id"]:04x}:{device["product_id"]:04x}' if device["vendor_id"] is not None and device["product_id"] is not None else device["model"])
        ids = ids or "(unknown)"
        state = f'has a card: {device["card"]}' if device["card"] else "no card yet"
        print(f'{index:>3}  {device["bus"]:<9} {ids:<16} {device["name"] or "(no name)":<34} {state}')
    missing = sum(1 for _, device in chosen if not device["card"])
    if missing:
        print(f"\n{missing} with no card. To start one: python3 tools/observe.py --draft <number>")
    return 0


if __name__ == "__main__":
    sys.exit(main())
