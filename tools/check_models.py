#!/usr/bin/env python3
"""Check that every card's Web of Things model is valid against the W3C's own schema.

The site build writes one Thing Model per card to site/models. This checks
each against the schema published with the Thing Description 1.1
Recommendation. It needs the `jsonschema` package and a network connection,
so it is a check for CI and for anyone changing the export, not part of the
build.

    python3 tools/build.py && python3 tools/check_models.py
"""
import json
import pathlib
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCHEMA = "https://raw.githubusercontent.com/w3c/wot-thing-description/REC1.1/validation/tm-json-schema-validation.json"


def main():
    try:
        from jsonschema import Draft7Validator
    except ImportError:
        sys.exit("this check needs the jsonschema package: pip install jsonschema")
    models = sorted((ROOT / "site" / "models").glob("*.tm.jsonld"))
    if not models:
        sys.exit("no models found; run python3 tools/build.py first")
    with urllib.request.urlopen(SCHEMA, timeout=30) as response:
        schema = json.load(response)
    validator = Draft7Validator(schema)
    # The check has to be able to fail, or passing means nothing.
    broken = json.loads(models[0].read_text())
    broken["@type"] = "Thing"
    if not list(validator.iter_errors(broken)):
        sys.exit("the schema accepted a model that is deliberately wrong; the check is not working")
    failed = 0
    for path in models:
        for error in validator.iter_errors(json.loads(path.read_text())):
            failed += 1
            print(f"{path.name}: {'/'.join(map(str, error.path))}: {error.message[:160]}", file=sys.stderr)
    if failed:
        return 1
    print(f"{len(models)} models are valid Thing Models ({schema['title']} {schema['version']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
