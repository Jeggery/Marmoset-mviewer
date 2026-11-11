"""Validate the project website and optionally stage its public files (Python 3.11+)."""

import argparse
from html.parser import HTMLParser
from pathlib import Path
import shutil
import tomllib
from urllib.parse import unquote, urlsplit


class PageLinks(HTMLParser):
    def __init__(self):
        super().__init__()
        self.local_urls = []
        self.canonical = []
        self.og_urls = []

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        if tag == 'link' and attrs.get('rel') == 'canonical':
            self.canonical.append(attrs.get('href'))
        if tag == 'meta' and attrs.get('property') == 'og:url':
            self.og_urls.append(attrs.get('content'))
        for name in ('src', 'href'):
            value = attrs.get(name)
            if value and not urlsplit(value).scheme and not urlsplit(value).netloc:
                self.local_urls.append(value)


def prepare(root: Path, output: Path | None) -> None:
    source = root / 'docs'
    # Keep format reference sources in the repository, outside the website artifact.
    files = [source / 'index.html', source / 'site.css']
    files.extend(path for path in (source / 'images').rglob('*') if path.is_file())
    for path in files:
        if path.is_symlink() or not path.is_file():
            raise ValueError(f'Missing or symbolic-link website file: {path}')
    published = {path.relative_to(source).as_posix() for path in files}
    page = PageLinks()
    page.feed((source / 'index.html').read_text(encoding='utf-8'))
    package = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['package']
    homepage = package['homepage']
    if page.canonical != [homepage] or page.og_urls != [homepage]:
        raise ValueError('Canonical and Open Graph URLs must match Cargo.toml homepage')
    for url in page.local_urls:
        path = unquote(urlsplit(url).path)
        if not path:
            continue
        if path not in published:
            raise ValueError(f'Local URL is missing or not relative to the project site: {url}')
    if output is not None:
        output = output.resolve()
        if output.exists():
            raise ValueError(f'Output already exists; choose a fresh directory: {output}')
        if output == source.resolve() or source.resolve() in output.parents:
            raise ValueError('Output must be outside docs/')
        output.mkdir(parents=True)
        for path in files:
            destination = output / path.relative_to(source)
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(path, destination)
        (output / '.nojekyll').touch()
    print(f'Validated {len(files)} public files for {homepage}')
    if output is not None:
        print(f'Staged website: {output}')


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, help='Stage files into a new directory; omit to validate only')
    args = parser.parse_args()
    try:
        prepare(Path(__file__).resolve().parent.parent, args.output)
    except (OSError, ValueError, KeyError) as error:
        parser.error(str(error))


if __name__ == '__main__':
    main()
