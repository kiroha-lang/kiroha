#!/usr/bin/env python3
"""Check generated documentation's internal links, fragments and assets."""
from html.parser import HTMLParser
from pathlib import Path
import sys
from urllib.parse import unquote, urljoin, urlsplit

BASE = 'https://kiroha-lang.github.io/kiroha/'


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__(convert_charrefs=True)
        self.ids = set()
        self.links = []
        self.feed(text)

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if 'id' in attrs:
            self.ids.add(attrs['id'])
        for key in ('href', 'src'):
            if key in attrs:
                self.links.append(attrs[key])


def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else 'site').resolve()
    pages = {p.resolve(): Page(p.read_text()) for p in root.rglob('*.html')}
    assert pages, 'No generated HTML pages found'
    checked = 0
    errors = []
    for path, page in pages.items():
        relative = path.relative_to(root).as_posix()
        route = relative[:-10] if relative.endswith('index.html') else relative
        current = urljoin(BASE, route)
        for link in page.links:
            target = urlsplit(urljoin(current, link))
            if target.scheme not in ('http', 'https') or target.netloc != urlsplit(BASE).netloc:
                continue
            prefix = urlsplit(BASE).path
            if not target.path.startswith(prefix):
                errors.append(f'{relative}: outside project path: {link}')
                continue
            dest = root / unquote(target.path[len(prefix):])
            if target.path.endswith('/'):
                dest /= 'index.html'
            dest = dest.resolve()
            if not dest.is_relative_to(root) or not dest.is_file():
                errors.append(f'{relative}: missing target: {link}')
                continue
            if target.fragment and dest in pages and unquote(target.fragment) not in pages[dest].ids:
                errors.append(f'{relative}: missing anchor: {link}')
            checked += 1
    if errors:
        raise SystemExit('\n'.join(sorted(set(errors))))
    print(f'{len(pages)} HTML pages and {checked} internal links/assets passed.')


if __name__ == '__main__':
    main()
