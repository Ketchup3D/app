#!/usr/bin/env python3
"""Stage the landing page and discovery files after Trunk builds the editor."""
import argparse
import html
import json
import os
import re
from pathlib import Path
import shutil
from urllib.parse import urlsplit
from xml.sax.saxutils import escape

DEFAULT_URL = "https://ketchup3d.github.io/app/"
FILES = ("index.html", "styles.css", "llms.txt", "robots.txt", "sitemap.xml")


def stage(source: Path, destination: Path, base_url: str, repository: str | None = None) -> None:
    repository = repository or os.environ.get("GITHUB_REPOSITORY", "Ketchup3D/app")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("Repository must be a GitHub owner/repository")
    repository_url = "https://github.com/" + repository
    base_url = base_url.rstrip("/") + "/"
    parsed = urlsplit(base_url)
    if (parsed.scheme != "https" or not parsed.hostname or parsed.query
            or parsed.fragment or parsed.username or parsed.password
            or any(c.isspace() or c in '<>"\\' for c in base_url)):
        raise ValueError("GitHub Pages must supply an HTTPS public URL without credentials, query, or fragment")
    destination.mkdir(parents=True, exist_ok=True)
    for name in FILES:
        content = (source / name).read_text().replace("https://github.com/Ketchup3D/app", repository_url)
        if name == "index.html":
            # JSON-LD needs JSON escaping; HTML attributes need HTML escaping.
            before, script = content.split('<script type="application/ld+json">', 1)
            structured, after = script.split('</script>', 1)
            structured = structured.replace(DEFAULT_URL, json.dumps(base_url)[1:-1])
            json.loads(structured)
            content = (before.replace(DEFAULT_URL, html.escape(base_url, quote=True))
                       + '<script type="application/ld+json">' + structured
                       + '</script>' + after.replace(DEFAULT_URL, html.escape(base_url, quote=True)))
        else:
            content = content.replace(DEFAULT_URL, escape(base_url) if name.endswith('.xml') else base_url)
        (destination / name).write_text(content)
    for directory in ("branding", "screenshots"):
        shutil.copytree(source.parent / "assets" / directory,
                        destination / "assets" / directory, dirs_exist_ok=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--dist", type=Path, default=Path("dist/site"))
    args = parser.parse_args()
    stage(Path(__file__).resolve().parents[1] / "site", args.dist, args.base_url)
