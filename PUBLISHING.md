# Publishing v3.0 and GitHub Pages

Repository: https://github.com/Jeggery/Marmoset-mviewer

Website: https://Jeggery.github.io/Marmoset-mviewer/

The website is the project introduction and download page. The model preview
continues to run in the desktop application.

## One-time GitHub configuration

1. Open the repository's **Actions** tab. If GitHub has disabled workflows for
   this fork, enable them.
2. Open **Settings > Pages > Build and deployment** and select **GitHub Actions**
   as the source. No custom domain is required.
3. If the `github-pages` environment has deployment branch restrictions, allow
   `master`. Complete any approval required by that environment when deploying.

The workflow uses the built-in `GITHUB_TOKEN`; no personal access token or
additional repository secret is required. Only the deployment job receives
`pages: write` and `id-token: write` permissions.

## Local checks and preview

Use Python 3.11 or later from the repository root:

```text
python scripts/prepare_pages.py
python scripts/prepare_pages.py --output dist/pages
python -m http.server 8000 --bind 127.0.0.1 --directory dist
```

Visit http://127.0.0.1:8000/pages/ to verify relative assets under a subpath.
Stop the server with Ctrl+C. Staging requires a fresh output directory; use a
different `--output` path for another preview if `dist/pages` already exists.
The script validates local HTML asset links and checks canonical/Open Graph URLs
against the package homepage. It stages `index.html`, `site.css`, `images/`, and
`.nojekyll`; reverse-engineering references stay outside the website artifact.

## Deploy the website

1. Review, commit and push the Pages changes to `master`.
2. Open **Actions > GitHub Pages** and watch the validation and deployment jobs.
   Pull requests validate the site without deploying it. Website changes pushed
   to `master` deploy automatically.
3. For a retry or an initial deployment without another file change, select
   **Run workflow** on the `master` branch.
4. Open the URL shown in the `github-pages` deployment environment. Verify the
   page, screenshot, repository link and download link.

If configuration fails with a Pages-not-found error, check the source setting
and that Actions are enabled. Manual runs on branches other than `master`
validate files but skip production deployment.

## Publish the desktop release

Pages deployment and binary releases are separate workflows. The download link
uses `/releases/latest`; publish v3.0 before announcing it so it resolves to the
new release.

```text
cargo test --release --locked --lib
python -m unittest discover -s scripts -p "test_*.py"
python scripts/release_metadata.py --tag v3.0
```

After committing and pushing the intended release commit, create `v3.0` on that
commit and push that tag. The Release workflow validates the version and release
notes, builds the platform binaries, and publishes them to this repository.
The Windows assets include a standalone `.exe` and a `.zip` archive.

Confirm that all release jobs succeed and the assets are available before
announcing the website and release. To restore an older website, revert its
changes on `master` and let the Pages workflow deploy that reviewed revision.
