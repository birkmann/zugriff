#!/usr/bin/env python3
"""Check static page references, duplicate IDs and local anchors without dependencies."""
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urlsplit, unquote

root = Path(__file__).resolve().parents[1] / 'website'
class Page(HTMLParser):
    def __init__(self):
        super().__init__()
        self.ids, self.references = [], []
    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if 'id' in attrs: self.ids.append(attrs['id'])
        for key in ('href', 'src'):
            if key in attrs: self.references.append(attrs[key])
        if tag == 'img': assert 'alt' in attrs, 'Image missing alternative text'
p = Page()
p.feed((root / 'index.html').read_text())
assert len(p.ids) == len(set(p.ids)), 'Duplicate element IDs'
for reference in p.references:
    url = urlsplit(reference)
    if url.scheme or url.netloc: continue
    if url.path:
        assert (root / unquote(url.path)).is_file(), f'Missing file: {reference}'
    elif url.fragment:
        assert url.fragment in p.ids, f'Missing anchor: {reference}'
print(f'Website: {len(p.references)} links/assets and {len(p.ids)} IDs checked.')
