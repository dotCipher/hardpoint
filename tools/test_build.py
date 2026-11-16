import copy
import unittest

from build import check_adapter_tools


class AdapterTools(unittest.TestCase):
    def setUp(self):
        self.adapter = {"platforms": ["macos"], "tool": [{
            "platform": "macos", "arches": ["aarch64"], "binary": "m1ddc",
            "packages": {"brew": "m1ddc"}, "note": "A supported display connection is needed.",
        }]}

    def errors(self, adapter=None):
        errors = []
        check_adapter_tools("example", adapter or self.adapter, errors)
        return errors

    def test_plain_recipe(self):
        self.assertEqual(self.errors(), [])

    def test_commands_paths_urls_options_and_other_managers_are_rejected(self):
        for key, value in [("command", "sh install.sh"), ("url", "https://example.invalid"),
                           ("binary", "../tool"), ("arches", ["arm64"]),
                           ("packages", {"brew": "--cask"}), ("packages", {"brew": "some/tap/tool"}),
                           ("packages", {"apt-get": "m1ddc"})]:
            with self.subTest(key=key, value=value):
                adapter = copy.deepcopy(self.adapter)
                adapter["tool"][0][key] = value
                self.assertTrue(self.errors(adapter))

    def test_overlapping_selectors_are_rejected(self):
        self.adapter["tool"].append(copy.deepcopy(self.adapter["tool"][0]))
        self.adapter["tool"][1]["arches"] = []
        self.assertTrue(self.errors())


if __name__ == "__main__":
    unittest.main()
