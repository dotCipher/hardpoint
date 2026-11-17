import contextlib
import copy
import io
import json
import pathlib
import tempfile
import tomllib
import unittest
from unittest.mock import patch

import build
import naming
import observe


def fixture():
    return {"makers": {"acme": {"name": "Acme", "aliases": ["ACME, Inc.", "Acme Incorporated"]}},
            "categories": {"display": {"label": "Monitor", "icon": "display", "aliases": ["screen"]},
                           "audio": {"label": "Audio device", "icon": "audio", "icons": ["earbuds"], "aliases": ["headset", "earbuds"], "alias_icons": {"earbuds": "earbuds"}}},
            "identity": {"model": {}, "family": {}, "class": {}}}


class PublicNaming(unittest.TestCase):
    def test_reviewed_public_names_normalize_without_guessing_capabilities(self):
        result = naming.normalize(" ACME, Inc. ", "ACME Focus™ Pro+", " SCREEN ", "model", data=fixture())
        self.assertEqual(result["device"], {"id": "acme/focus-pro-plus", "vendor": "Acme", "name": "Focus Pro+", "kind": "display", "identity": "model"})
        self.assertTrue(result["review_required"])
        self.assertNotIn("capability", result["device"])
        self.assertEqual(naming.slug("Écran + Dock & Hub"), "ecran-plus-dock-and-hub")

    def test_existing_aliases_reuse_stable_ids_and_canonical_spelling(self):
        card = {"device": {"id": "acme/legacy-id", "vendor": "Acme", "name": "Focus Pro+", "kind": "display", "identity": "model", "aliases": ["Acme Focus Display"]}}
        result = naming.normalize("acme incorporated", "acme focus display", "screen", "family", [card], data=fixture())
        self.assertEqual((result["device"]["id"], result["device"]["name"], result["device"]["identity"]), ("acme/legacy-id", "Focus Pro+", "model"))
        self.assertTrue(result["existing_card"])
        correction = naming.normalize("Acme", "Corrected public name", "display", "family", [card], "acme/legacy-id", fixture())
        self.assertEqual(correction["device"]["id"], "acme/legacy-id")
        self.assertEqual(correction["device"]["name"], "Corrected public name")

    def test_reviewed_form_factor_alias_selects_the_registry_icon(self):
        result = naming.normalize("Acme", "Focus Buds", "earbuds", "family", data=fixture())
        self.assertEqual((result["device"]["kind"], result["device"]["icon"]), ("audio", "earbuds"))

    def test_unregistered_makers_and_ambiguous_names_require_review(self):
        result = naming.normalize("New Maker", "Reviewed Product", "screen", "family", data=fixture())
        self.assertTrue(result["maker_registration_needed"])
        card = {"device": {"id": "acme/one", "vendor": "Acme", "name": "Focus", "kind": "display"}}
        second = copy.deepcopy(card)
        second["device"]["id"] = "acme/two"
        with self.assertRaisesRegex(ValueError, "several registry cards"):
            naming.normalize("Acme", "Focus", "screen", "family", [card, second], data=fixture())

    def test_public_model_codes_disambiguate_same_named_variants(self):
        cards = [{"device": {"id": "acme/stable-one", "vendor": "Acme", "name": "Focus", "kind": "display", "identity": "model", "aliases": ["Public-A1"]}}]
        existing = naming.normalize("Acme", "Focus", "screen", "model", cards, data=fixture(), model_code="Public-A1")
        self.assertEqual(existing["device"]["id"], "acme/stable-one")
        another = naming.normalize("Acme", "Focus", "screen", "model", cards, data=fixture(), model_code="Public-A2")
        self.assertFalse(another["existing_card"])
        self.assertEqual(another["device"]["id"], "acme/focus-public-a2")
        self.assertEqual(another["device"]["aliases"], ["Public-A2"])
        international = naming.normalize("Acme", "表示器", "screen", "family", data=fixture(), model_code="Public-A3")
        self.assertEqual((international["device"]["id"], international["device"]["name"]), ("acme/public-a3", "表示器"))

    def test_invisible_text_placeholders_and_unknown_categories_are_rejected(self):
        for text in ("TODO", "<model>", "", "name\n[table]", "name\u202e", "name\u200b"):
            with self.subTest(text=text), self.assertRaises(ValueError):
                naming.clean(text)
        with self.assertRaises(ValueError):
            naming.normalize("Acme", "Focus", "made-up category", "family", data=fixture())
        with self.assertRaises(ValueError):
            naming.slug("未確認 3")

    def test_cli_normalization_never_discovers_or_writes(self):
        out = io.StringIO()
        with patch.object(naming, "registry", return_value=fixture()), patch.object(observe, "cards", return_value=[]), patch.object(observe, "observe", side_effect=AssertionError("no device discovery")), contextlib.redirect_stdout(out):
            self.assertEqual(observe.main(["--normalize", "--vendor", "ACME, Inc.", "--name", "Acme Focus+", "--kind", "screen"]), 0)
        self.assertEqual(json.loads(out.getvalue())["device"]["id"], "acme/focus-plus")

    def test_reviewed_draft_fields_use_the_same_normalizer(self):
        with tempfile.TemporaryDirectory() as temp:
            path = pathlib.Path(temp) / "snapshot.json"
            path.write_text(json.dumps([observe.seen("usb", "Private label", 1, 2)]))
            out = io.StringIO()
            with patch.object(naming, "registry", return_value=fixture()), patch.object(observe, "cards", return_value=[]), contextlib.redirect_stdout(out):
                observe.main(["--from-file", str(path), "--draft", "1", "--vendor", "Acme", "--name", "Acme Focus+", "--kind", "screen"])
            d = tomllib.loads(out.getvalue())["device"]
            self.assertEqual((d["id"], d["name"], d["kind"], d["identity"]), ("acme/focus-plus", "Focus+", "display", "family"))
            self.assertNotIn("Private label", out.getvalue())
            self.assertEqual(list(pathlib.Path(temp).iterdir()), [path])

    def test_older_python_card_reader_preserves_quoted_public_names(self):
        with tempfile.TemporaryDirectory() as temp:
            path = pathlib.Path(temp) / "card.toml"
            path.write_text('[device]\nid = "acme/legacy"\nvendor = \'Acme\'\nname = "Focus \\"Pro\\""\naliases = ["Focus Display"]\n[[match]]\nbus = "usb"\nvendor_id = 0x0001\nproduct_id = 2\n')
            with patch.object(observe, "tomllib", None):
                card = observe.read_card(path)
            self.assertEqual(card["device"]["name"], 'Focus "Pro"')
            self.assertEqual(card["device"]["aliases"], ["Focus Display"])
            self.assertEqual(card["match"][0]["vendor_id"], 1)


class RegistryPresentation(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.catalog = build.Catalog()

    def test_all_registered_identities_are_canonical_without_renaming_ids(self):
        self.assertFalse(self.catalog.errors)
        for card in self.catalog.cards:
            d = card["device"]
            normalized = naming.normalize(d["vendor"], d["name"], d["kind"], d["identity"], self.catalog.cards, data=self.catalog.identities)
            self.assertEqual(normalized["device"]["id"], d["id"])
            self.assertEqual(normalized["device"]["name"], d["name"])

    def test_every_category_icon_and_override_has_a_real_drawing(self):
        for card in self.catalog.cards:
            category = self.catalog.device_types[card["device"]["kind"]]
            self.assertIn(card["device"].get("icon", category["icon"]), build.ART)

    def test_all_maker_and_category_aliases_round_trip_to_the_registry(self):
        data = self.catalog.identities
        for id, entry in data["makers"].items():
            for alias in [id, entry["name"], *entry.get("aliases", [])]:
                self.assertEqual(naming.maker(alias, data), (id, entry["name"]))
        for id, entry in data["categories"].items():
            for alias in [id, entry["label"], *entry.get("aliases", [])]:
                self.assertEqual(naming.category(alias, data), id)

    def test_invalid_identity_metadata_and_reference_urls_fail_validation(self):
        template = self.catalog.cards[0]
        for field, value in (("kind", "invented"), ("identity", "unknown-scope"), ("vendor", "Unknown Maker"),
                             ("name", "  Not normalized  "), ("icon", "invented"), ("links", ["https://user:secret@example.test/"]),
                             ("links", ["https://example.test/?utm_source=no"]), ("links", ["https://[broken/"]),
                             ("links", ["https://192.168.1.2/"]), ("links", ["https://private-device.local/"])):
            c = copy.deepcopy(self.catalog)
            card = copy.deepcopy(template)
            card["device"][field] = value
            c._check_identity(card, "fixture")
            self.assertTrue(c.errors, (field, value))


if __name__ == "__main__":
    unittest.main()
