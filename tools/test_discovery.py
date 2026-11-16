import json
import copy
import pathlib
import re
import tempfile
import unittest
import urllib.parse
import xml.etree.ElementTree as ET
from html.parser import HTMLParser
from unittest.mock import patch

import build
from discovery import canonical, markdown, origin_robots, script_json


class Head(HTMLParser):
    def __init__(self, html):
        super().__init__()
        self.links, self.meta, self.ids, self.graph, self.references = [], {}, set(), [], []
        self.in_schema = False
        self.schema = ""
        self.feed(html)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if "id" in attrs:
            self.ids.add(attrs["id"])
        if tag in ("a", "link", "script", "img"):
            ref = attrs.get("href", attrs.get("src"))
            if ref:
                self.references.append(ref)
        if tag == "link":
            self.links.append(attrs)
        if tag == "meta":
            self.meta[attrs.get("name", attrs.get("property"))] = attrs.get("content")
        if tag == "script" and attrs.get("type") == "application/ld+json":
            self.in_schema = True
            self.schema = ""

    def handle_data(self, text):
        if self.in_schema:
            self.schema += text

    def handle_endtag(self, tag):
        if tag == "script" and self.in_schema:
            self.graph.extend(json.loads(self.schema)["@graph"])
            self.in_schema = False


class DiscoveryUnits(unittest.TestCase):
    def test_canonical_and_robots_respect_project_subpaths(self):
        self.assertEqual(canonical("https://example.test/project/", "index.html"), "https://example.test/project/")
        self.assertEqual(canonical("https://example.test/project/", "devices/example.html"), "https://example.test/project/devices/example.html")
        self.assertIsNone(canonical(None, "index.html"))
        self.assertIsNone(origin_robots("https://example.test/project/"))
        self.assertIn("Sitemap: https://example.test/sitemap.xml", origin_robots("https://example.test/"))

    def test_json_cannot_close_the_script_element(self):
        value = {"description": "</script><script>untrusted()</script>&"}
        encoded = script_json(value)
        self.assertNotIn("<", encoded)
        self.assertEqual(json.loads(encoded), value)

    def test_explicit_public_origin_is_validated_and_forks_keep_their_path(self):
        for url in ("/relative/", "javascript:bad", "https://user:secret@example.test/", "https://example.test/?q=1", "https://example.test/#fragment"):
            with self.subTest(url=url), patch.dict("os.environ", {"HARDPOINT_SITE_URL": url}):
                with self.assertRaises(ValueError):
                    build.site_url(None)
        with patch.dict("os.environ", {"HARDPOINT_SITE_URL": ""}):
            self.assertEqual(build.site_url("https://github.com/Example/Fork"), "https://example.github.io/Fork/")

    def test_markdown_preserves_details_code_and_targets_without_ui_noise(self):
        text = markdown('<header>navigation</header><main><h1>A guide</h1><svg><title>key 27</title></svg>'
                        '<script>doNotKeep()</script><button>Copy</button><p>Use <code>read</code>.</p>'
                        '<details><summary>Evidence</summary><p>Only a read was tested.</p></details>'
                        '<a href="../agents.html#guide">Guide</a><pre>one\n  two\n</pre></main><footer>footer</footer>',
                        "https://example.test/project/devices/model.html")
        for unwanted in ("navigation", "key 27", "doNotKeep", "Copy", "footer"):
            self.assertNotIn(unwanted, text)
        for expected in ("# A guide", "`read`", "Only a read was tested.", "https://example.test/project/agents.html#guide", "one\n  two"):
            self.assertIn(expected, text)

    def test_markdown_tables_remain_one_contiguous_table(self):
        text = markdown('<main><table><tr><th>Ability</th><th>Evidence</th></tr>'
                        '<tr><td>Read<span class="sub mono">battery.level</span></td><td>tested</td></tr></table></main>', "https://example.test/")
        self.assertIn("| --- | --- |\n| Read battery.level | tested |", text)


class GeneratedDiscovery(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp = tempfile.TemporaryDirectory(prefix="hardpoint-site-tests-")
        cls.output = pathlib.Path(cls.temp.name) / "site"
        cls.catalog = build.Catalog()
        cls.content = build.load_content(cls.catalog)
        if cls.catalog.errors:
            raise AssertionError(cls.catalog.errors)
        with patch.object(build, "SITE", cls.output), patch.dict("os.environ", {"HARDPOINT_SITE_URL": "https://example.test/project/"}):
            cls.site = build.Site(cls.catalog, cls.content, "https://github.com/example/hardpoint")
            cls.site.write()

    @classmethod
    def tearDownClass(cls):
        cls.temp.cleanup()

    def test_every_page_has_distinct_metadata_schema_and_a_markdown_alternate(self):
        urls, descriptions = set(), set()
        for path, meta in self.site.page_metadata.items():
            with self.subTest(path=path):
                head = Head((self.output / path).read_text())
                url = canonical(self.site.home, path)
                self.assertEqual([l["href"] for l in head.links if l.get("rel") == "canonical"], [url])
                self.assertEqual(head.meta["og:url"], url)
                self.assertEqual(head.meta["description"], meta["description"])
                self.assertEqual(head.meta["og:description"], meta["description"])
                urls.add(url)
                descriptions.add(head.meta["description"])
                alt = next(l for l in head.links if l.get("type") == "text/markdown")
                target = urllib.parse.urljoin(url, alt["href"])
                self.assertTrue((self.output / urllib.parse.urlsplit(target).path.removeprefix("/project/")).is_file())
                page = next(n for n in head.graph if n["@type"] == "WebPage")
                self.assertEqual(page["description"], meta["description"])
                self.assertEqual(page["url"], url)
        self.assertEqual(len(urls), len(self.site.page_metadata))
        self.assertEqual(len(descriptions), len(self.site.page_metadata))

    def test_sitemap_covers_only_canonical_html_and_has_no_fake_freshness(self):
        root = ET.fromstring((self.output / "sitemap.xml").read_text())
        ns = {"s": "http://www.sitemaps.org/schemas/sitemap/0.9"}
        urls = [n.text for n in root.findall("s:url/s:loc", ns)]
        self.assertEqual(set(urls), {canonical(self.site.home, path) for path in self.site.page_metadata})
        self.assertEqual(len(urls), len(set(urls)))
        self.assertFalse(root.findall("s:url/s:lastmod", ns))
        self.assertFalse((self.output / "robots.txt").exists(), "a project cannot set its origin robots policy")

    def test_unknown_origin_does_not_invent_canonical_schema_or_sitemap_links(self):
        with patch.dict("os.environ", {"HARDPOINT_SITE_URL": ""}):
            site = build.Site(self.catalog, self.content, None)
            html = site.agents()
        head = Head(html)
        self.assertFalse(head.graph)
        self.assertFalse([l for l in head.links if l.get("rel") == "canonical"])
        self.assertNotIn('href="sitemap.xml"', html)

    def test_all_local_html_resources_and_fragments_resolve(self):
        for path in self.site.page_metadata:
            html = (self.output / path).read_text()
            for href in Head(html).references:
                url = urllib.parse.urlsplit(urllib.parse.urljoin(canonical(self.site.home, path), href))
                if url.netloc != "example.test":
                    continue
                rel = url.path.removeprefix("/project/") or "index.html"
                target = self.output / rel
                self.assertTrue(target.is_file(), (path, href, rel))
                if url.fragment and target.suffix == ".html":
                    self.assertIn(urllib.parse.unquote(url.fragment), Head(target.read_text()).ids, (path, href))

    def test_search_entries_cite_real_claims_and_preserve_evidence(self):
        feed = json.loads((self.output / "search-index.json").read_text())
        self.assertEqual(feed["scope"], "catalog-not-runtime-inventory")
        caps = {(card["device"]["id"], cap["id"]): cap for card in self.catalog.cards for cap in card["capability"]}
        self.assertEqual(len(feed["entries"]), len(caps))
        for entry in feed["entries"]:
            self.assertNotIn("reachable", entry)
            self.assertNotIn("readable", entry)
            self.assertNotIn("writable", entry)
            cap = caps[(entry["device"], entry["capability"])]
            self.assertEqual(entry["verification"], cap["verification"])
            self.assertEqual(entry["label"], cap["risk"])
            url = urllib.parse.urlsplit(entry["url"])
            html = self.output / url.path.removeprefix("/project/")
            self.assertIn(url.fragment, Head(html.read_text()).ids)
            for key in ("markdown", "model"):
                self.assertTrue((self.output / urllib.parse.urlsplit(entry[key]).path.removeprefix("/project/")).is_file())

    def test_llms_navigation_targets_exist_and_exclude_local_information(self):
        text = (self.output / "llms.txt").read_text()
        self.assertTrue(text.startswith("# " + self.content["name"]))
        for href in re.findall(r"\]\(([^)]+)\)", text):
            url = urllib.parse.urlsplit(href)
            if url.netloc == "example.test":
                self.assertTrue((self.output / url.path.removeprefix("/project/")).is_file(), href)
        self.assertFalse((self.output / "local").exists())
        for path in self.content["discovery"]["published_docs"]:
            self.assertEqual((self.output / path).read_text(), (build.ROOT / path).read_text())
        self.assertIn("not a live inventory", text)
        guide = (self.output / "agents.md").read_text()
        self.assertIn("adapter registry", guide)
        self.assertIn("not a universal implementation", guide)
        self.assertIn("configured action", guide)

    def test_registry_changes_propagate_without_editing_prose(self):
        c = copy.deepcopy(self.catalog)
        card = copy.deepcopy(next(card for card in c.cards if not card.get("keys")))
        card["device"].update(id="fixture/future-model", name="FutureRegistryModel", vendor="FixtureMaker", summary="A synthetic registry addition.")
        card["_path"], card["_source"], card["_commits"] = "catalog/devices/fixture/future-model.toml", "# Synthetic registry fixture", {}
        card["capability"] = card["capability"][:1]
        cap = card["capability"][0]
        cap["verification"] = [{"status": "unverified", "platform": c.adapters[cap["adapter"]]["platforms"][0], "note": "Synthetic registry evidence"}]
        c.cards.append(card)
        c.adapters[cap["adapter"]]["notes"] = "Fixture routing constraint from registry"
        c.adapters[cap["adapter"]]["tool"] = [{"platform": "macos", "arches": [], "binary": "fixture-helper",
                                               "packages": {"brew": "fixture-package"}, "note": "Fixture helper requirement"}]
        with tempfile.TemporaryDirectory() as temp, patch.object(build, "SITE", pathlib.Path(temp) / "site"):
            content = build.load_content(c)
            self.assertFalse(c.errors)
            build.Site(c, content, "https://github.com/example/hardpoint").write()
            output = pathlib.Path(temp) / "site"
            self.assertIn(card["device"]["name"], (output / "llms.txt").read_text())
            self.assertIn("Synthetic registry evidence", (output / "devices/fixture-future-model.md").read_text())
            self.assertIn(c.adapters[cap["adapter"]]["notes"], (output / "adapters.html").read_text())
            self.assertIn("fixture-helper", (output / "adapters.html").read_text())
            self.assertIn("fixture-package", (output / "adapters.md").read_text())
            row = next(e for e in json.loads((output / "search-index.json").read_text())["entries"] if e["device"] == card["device"]["id"])
            self.assertEqual(row["verification"], cap["verification"])

    def test_authored_docs_and_site_prose_do_not_pin_registry_devices(self):
        raw = build.load_toml(build.SOURCE / "content.toml")
        self.assertNotIn("card", raw["demo"])
        self.assertNotIn("config", raw["demo"])
        for example in raw["ladder"]:
            self.assertNotIn("device", example)
            self.assertNotIn("capability", example)
        paths = [build.ROOT / p for p in ("README.md", "AGENTS.md", "CONTRIBUTING.md")]
        paths += list((build.ROOT / "docs").glob("*.md")) + list(build.SOURCE.glob("*.md"))
        paths.append(build.SOURCE / "content.toml")
        for path in paths:
            text = path.read_text()
            for card in self.catalog.cards:
                self.assertNotIn(card["device"]["id"], text, f"{path}: link to the registry instead of pinning a model")
                self.assertNotIn(card["device"]["name"], text, f"{path}: model facts belong to the registry")
            self.assertNotRegex(text, r"\b\d+ (?:devices|cards|capabilities|models)\b", str(path))

    def test_page_and_evidence_survive_without_javascript(self):
        for card in self.catalog.cards:
            path = self.output / f'devices/{build.slug(card)}.md'
            text = path.read_text()
            for cap in card["capability"]:
                self.assertIn(cap["id"], text)
                self.assertIn(cap["risk"], text)
                self.assertIn(self.catalog.best_status(cap), text)
        self.assertIn(self.content["install"]["title"], (self.output / "install.md").read_text())


if __name__ == "__main__":
    unittest.main()
