"""Static discovery helpers. No crawling, model calls, or runtime device data."""

import json
import re
import urllib.parse
import xml.etree.ElementTree as ET
from html.parser import HTMLParser


def script_json(value):
    """JSON inside an HTML script must not be able to close the script tag."""
    return json.dumps(value, ensure_ascii=False).replace("<", "\\u003c").replace(">", "\\u003e").replace("&", "\\u0026")


def canonical(home, path):
    if not home:
        return None
    return home if path == "index.html" else urllib.parse.urljoin(home, path)


class MainMarkdown(HTMLParser):
    """Extract semantic main content, not navigation, SVGs or scripted demos.

    Use the generated HTML itself so details, evidence and prose cannot drift
    into a second set of product claims. This is deliberately not a general
    HTML renderer.
    """

    def __init__(self, url):
        super().__init__(convert_charrefs=True)
        self.url = url
        self.parts = []
        self.main = False
        self.skip = 0
        self.pre = False
        self.links = []
        self.header_row = False
        self.table = False
        self.columns = 0

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "main":
            self.main = True
        if not self.main:
            return
        if tag in ("svg", "script", "style", "button", "select"):
            self.skip += 1
        if self.skip:
            return
        if tag == "table":
            self.table = True
            self.parts.append("\n\n")
        elif re.fullmatch(r"h[1-6]", tag):
            self.parts.append("\n\n" + "#" * int(tag[1]) + " ")
        elif tag in ("p", "div", "section", "dl", "dt", "dd", "details", "ul", "ol"):
            self.parts.append(" " if self.table else "\n\n")
        elif tag == "li":
            self.parts.append("\n- ")
        elif tag == "pre":
            self.pre = True
            self.parts.append("\n\n```\n")
        elif tag == "code" and not self.pre:
            self.parts.append("`")
        elif tag == "br":
            self.parts.append("\n")
        elif tag == "a":
            href = urllib.parse.urljoin(self.url, attrs.get("href", ""))
            href = href if urllib.parse.urlsplit(href).scheme in ("http", "https", "") else ""
            # Encode Markdown delimiters; the target remains the same URL.
            href = href.replace("(", "%28").replace(")", "%29")
            self.links.append(href)
            if href:
                self.parts.append("[")
        elif tag == "tr":
            self.header_row = False
            self.columns = 0
            self.parts.append("| " if self.parts and self.parts[-1].endswith("\n") else "\n| ")
        elif tag in ("td", "th"):
            self.columns += 1
            self.header_row |= tag == "th"
        elif tag == "span" and "sub" in attrs.get("class", "").split():
            self.parts.append(" ")

    def handle_endtag(self, tag):
        if not self.main:
            return
        if tag in ("svg", "script", "style", "button", "select") and self.skip:
            self.skip -= 1
            return
        if self.skip:
            return
        if tag == "main":
            self.main = False
        elif tag == "pre":
            self.pre = False
            self.parts.append("\n```\n\n")
        elif tag == "code" and not self.pre:
            self.parts.append("`")
        elif tag == "a" and self.links:
            href = self.links.pop()
            if href:
                self.parts.append(f"]({href})")
        elif tag in ("td", "th"):
            self.parts.append(" | ")
        elif tag == "tr":
            self.parts.append("\n")
            if self.header_row:
                self.parts.append("| " + " | ".join(["---"] * self.columns) + " |\n")
        elif tag == "table":
            self.table = False
            self.parts.append("\n\n")
        elif tag in ("summary", "dt"):
            self.parts.append(": ")
        elif tag in ("p", "section", "details") or re.fullmatch(r"h[1-6]", tag):
            self.parts.append(" " if self.table else "\n\n")

    def handle_data(self, data):
        if self.main and not self.skip:
            data = data.replace("|", "\\|") if self.table else data
            self.parts.append(data if self.pre else re.sub(r"\s+", " ", data))

    def text(self):
        text = "".join(self.parts)
        return re.sub(r"\n{3,}", "\n\n", re.sub(r"[ \t]+\n", "\n", text)).strip() + "\n"


def markdown(html, url):
    parser = MainMarkdown(url)
    parser.feed(html)
    return parser.text()


def sitemap(urls):
    ET.register_namespace("", "http://www.sitemaps.org/schemas/sitemap/0.9")
    ns = "{http://www.sitemaps.org/schemas/sitemap/0.9}"
    root = ET.Element(ns + "urlset")
    for url in sorted(set(urls)):
        ET.SubElement(ET.SubElement(root, ns + "url"), ns + "loc").text = url
    # No invented last-modified dates, priority or change-frequency claims.
    return ET.tostring(root, encoding="unicode", xml_declaration=True) + "\n"


def origin_robots(home):
    """robots.txt is authoritative only at an origin root, not /project/."""
    if not home or urllib.parse.urlsplit(home).path not in ("", "/"):
        return None
    return f"User-agent: *\nAllow: /\nSitemap: {home}sitemap.xml\n"
