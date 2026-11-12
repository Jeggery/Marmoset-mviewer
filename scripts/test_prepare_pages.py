import contextlib
import io
import tempfile
from pathlib import Path
import unittest

from prepare_pages import prepare


class PreparePagesTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.source = self.root / 'docs'
        (self.source / 'images').mkdir(parents=True)
        (self.root / 'Cargo.toml').write_text(
            '[package]\nhomepage = "https://example.com/project/"\n', encoding='utf-8'
        )
        (self.source / 'site.css').write_text('body { margin: 0; }', encoding='utf-8')
        (self.source / 'images' / 'preview.webp').write_bytes(b'preview fixture')
        (self.root / 'README.md').write_text('Repository-only file', encoding='utf-8')

    def write_page(self, url):
        (self.source / 'index.html').write_text(
            '<link rel="canonical" href="https://example.com/project/">'
            '<meta property="og:url" content="https://example.com/project/">'
            f'<a href="{url}">Link</a>', encoding='utf-8'
        )

    def test_relative_home_and_asset_links_are_accepted(self):
        for url in ('./', '.', './#features', './?source=logo', './index.html',
                    './images/preview.webp?size=large#preview',
                    './images/../index.html', '#features'):
            with self.subTest(url=url), contextlib.redirect_stdout(io.StringIO()):
                self.write_page(url)
                prepare(self.root, None)

    def test_missing_and_non_project_relative_files_are_rejected(self):
        for url in ('missing.html', './images/missing.webp', '../README.md',
                    './images/../../README.md', '%2e%2e/README.md', '/index.html'):
            with self.subTest(url=url):
                self.write_page(url)
                with self.assertRaisesRegex(ValueError, 'Local URL is missing'):
                    prepare(self.root, None)

    def test_home_link_stages_only_public_files(self):
        self.write_page('./')
        references = self.source / 'reverse-engineering'
        references.mkdir()
        (references / 'reference.js').write_text('reference', encoding='utf-8')
        output = self.root / 'dist' / 'pages'
        with contextlib.redirect_stdout(io.StringIO()):
            prepare(self.root, output)
        self.assertEqual(
            {path.relative_to(output).as_posix()
             for path in output.rglob('*') if path.is_file()},
            {'index.html', 'site.css', 'images/preview.webp', '.nojekyll'},
        )
        self.assertEqual((output / 'index.html').read_bytes(),
                         (self.source / 'index.html').read_bytes())


if __name__ == '__main__':
    unittest.main()
