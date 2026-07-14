#!/usr/bin/env python3
"""Validate the static website without third-party dependencies."""

from __future__ import annotations

from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
SITE = ROOT / "site"
FORBIDDEN = {
    "gitlab.com": "GitLab is not a project host",
    "cargo add digest strata lethe": "workspace crates are not published under these names",
}


class Links(HTMLParser):
    def __init__(self) -> None:
        super().__init__()
        self.links: list[str] = []

    def handle_starttag(self, tag: str, attrs: list[tuple[str, str | None]]) -> None:
        values = dict(attrs)
        if tag in {"a", "img", "link", "script"}:
            value = values.get("href") or values.get("src")
            if value:
                self.links.append(value)


def main() -> None:
    failures: list[str] = []
    pages = sorted(SITE.glob("*.html"))
    if not pages:
        raise SystemExit("site contains no HTML pages")
    for page in pages:
        text = page.read_text(encoding="utf-8")
        for forbidden, reason in FORBIDDEN.items():
            if forbidden.lower() in text.lower():
                failures.append(f"{page.relative_to(ROOT)}: {reason}")
        parser = Links()
        try:
            parser.feed(text)
            parser.close()
        except (
            Exception
        ) as error:  # HTMLParser errors include useful page context here.
            failures.append(f"{page.relative_to(ROOT)}: invalid HTML: {error}")
            continue
        for link in parser.links:
            parsed = urlsplit(link)
            if parsed.scheme or link.startswith(("#", "//")):
                continue
            if parsed.path and not (page.parent / parsed.path).exists():
                failures.append(
                    f"{page.relative_to(ROOT)}: missing local target {parsed.path!r}"
                )
    if failures:
        raise SystemExit("\n".join(failures))
    print(f"validated {len(pages)} HTML pages and their local links")


if __name__ == "__main__":
    main()
