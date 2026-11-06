#!/usr/bin/env python3
"""Keep README repository and Pages links aligned with its GitHub checkout."""
import argparse
from pathlib import Path
import re


def synchronize(path: Path, repository: str, base_url: str | None = None) -> None:
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("Repository must be owner/repository")
    owner, name = repository.split("/")
    base_url = (base_url or f"https://{owner.lower()}.github.io/"
                + ("" if name.lower() == owner.lower() + ".github.io" else name + "/"))
    from urllib.parse import urlsplit
    parsed = urlsplit(base_url)
    if (parsed.scheme != "https" or not parsed.hostname or parsed.query or parsed.fragment
            or parsed.username or parsed.password or any(c.isspace() or c in '<>"\\' for c in base_url)):
        raise ValueError("Pages URL must be HTTPS without credentials, query, or fragment")
    base_url = base_url.rstrip("/") + "/"
    text = path.read_text()
    previous = re.search(r"\]\((https://[^)]+/)play/\)", text)
    if previous:
        text = text.replace(previous.group(1), base_url)
    # Match only application links, preserving external documentation/badges.
    text = re.sub(r"https://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+(?=/releases/)",
                  "https://github.com/" + repository, text)
    text = re.sub(r"/releases/tag/v[0-9]+\.[0-9]+\.[0-9]+(?=\))", "/releases/latest", text)
    text = re.sub(r"(?<=\]\()https://[^)]+/play/(?=\))", base_url + "play/", text)
    text = re.sub(r"To enable publishing for `[^`]+`:",
                  f"To enable publishing for `{repository}`:", text)
    text = re.sub(r"<https://[A-Za-z0-9_.-]+\.github\.io/[^>]*>",
                  lambda m: "<" + base_url + ("play/" if m[0].endswith("/play/>") else "") + ">", text)
    path.write_text(text)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", required=True)
    parser.add_argument("--base-url")
    parser.add_argument("--readme", type=Path, default=Path("README.md"))
    args = parser.parse_args()
    synchronize(args.readme, args.repository, args.base_url or None)
