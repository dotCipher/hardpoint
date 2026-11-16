import contextlib
import io
import json
import os
import pathlib
import tempfile
import tomllib
import unittest
from unittest.mock import patch

import observe


class Selection(unittest.TestCase):
    def setUp(self):
        self.devices = [dict(observe.seen("usb", "local name", 1, n, "Fixture Maker"), card="fixture/known" if n == 1 else None) for n in range(1, 5)]

    def test_all_missing_specific_and_ranges_preserve_snapshot_numbers(self):
        for value, expected in (("all", [1, 2, 3, 4]), ("missing", [2, 3, 4]), ("3,1", [1, 3]), ("2-4,3", [2, 3, 4])):
            self.assertEqual([n for n, _ in observe.select(self.devices, value)], expected)
        self.assertEqual(observe.select([], "all"), [])
        self.assertEqual(observe.select([], "missing"), [])

    def test_invalid_selection_never_falls_back_to_all_or_a_negative_index(self):
        for value in ("0", "-1", "5", "3-2", "1,", "all,2", "a", ""):
            with self.subTest(value=value), self.assertRaises(ValueError):
                observe.select(self.devices, value)

    def test_snapshot_selection_never_rescans_and_rechecks_registry_matches(self):
        known = {"device": {"id": "fixture/current"}, "match": [{"bus": "usb", "vendor_id": 1, "product_id": 1}], "_scope": "private"}
        with tempfile.TemporaryDirectory() as temp:
            path = pathlib.Path(temp) / "private.json"
            path.write_text(json.dumps(self.devices))
            out = io.StringIO()
            with patch.object(observe, "cards", return_value=[known]), patch.object(observe, "observe", side_effect=AssertionError("must not rescan")), contextlib.redirect_stdout(out):
                self.assertEqual(observe.main(["--from-file", str(path), "--select", "1,3", "--json"]), 0)
            rows = json.loads(out.getvalue())
            self.assertEqual([r["number"] for r in rows], [1, 3])
            self.assertEqual((rows[0]["card"], rows[0]["card_scope"]), ("fixture/current", "private"))
            self.assertIsNone(rows[1]["card"])
            self.assertEqual(list(pathlib.Path(temp).iterdir()), [path], "selection writes no cards")
            for options in (["--select", "", "--json"], ["--draft", "0"], ["--draft", "-1"], ["--draft", "1"]):
                with self.subTest(options=options), patch.object(observe, "cards", return_value=[known]), patch.object(observe, "observe", side_effect=AssertionError("must not rescan")), contextlib.redirect_stderr(io.StringIO()):
                    with self.assertRaises(SystemExit):
                        observe.main(["--from-file", str(path), *options])

    def test_drafts_do_not_publish_friendly_names_and_quote_observed_text(self):
        device = observe.seen("usb", 'Private owner device "name"', 1, 2, 'Maker"\n[untrusted]')
        with patch.object(observe.platform, "system", return_value="Linux"):
            text = observe.draft(device)
        card = tomllib.loads(text)
        self.assertEqual(card["device"]["vendor"], device["vendor"])
        self.assertNotIn("untrusted", card, "quoted data must not inject a TOML table")
        self.assertNotIn(device["name"], text)
        self.assertNotIn("private-owner", observe.draft_destination(device))
        self.assertNotIn("capability", card, "a template must not invent a supported ability")

    def test_snapshot_rejects_invalid_data(self):
        for value in ({"command": "no"}, ["not an observation"], [{"bus": "usb", "vendor_id": True}], [{"bus": "usb", "product_id": -1}],
                      [dict(self.devices[0], number=1), dict(self.devices[2], number=3)]):
            with self.subTest(value=value), tempfile.TemporaryDirectory() as temp:
                p = pathlib.Path(temp) / "snapshot.json"
                p.write_text(json.dumps(value))
                with self.assertRaises(ValueError):
                    observe.snapshot(p)

    def test_personal_cards_override_without_becoming_shared(self):
        with tempfile.TemporaryDirectory() as temp:
            base = pathlib.Path(temp)
            shared, own = base / "shared", base / "config/hardpoint/cards"
            source = '[device]\nid = "fixture/model"\nname = "{name}"\n[[match]]\nbus = "usb"\nvendor_id = 1\nproduct_id = 1\n'
            for directory, name in ((shared, "Shared"), (own, "Private")):
                (directory / "fixture").mkdir(parents=True)
                (directory / "fixture/model.toml").write_text(source.format(name=name))
            with patch.object(observe, "DEVICES", shared), patch.dict(os.environ, {"XDG_CONFIG_HOME": str(base / "config")}):
                cards = observe.cards()
            self.assertEqual(len(cards), 1)
            self.assertEqual((cards[0]["device"]["name"], cards[0]["_scope"]), ("Private", "private"))


if __name__ == "__main__":
    unittest.main()
