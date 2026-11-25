# neolint website

The public project website is built with Astro and Starlight and deployed to GitHub Pages.

## Local development

Requirements: Node.js 22.23.4 or newer.

```sh
npm ci
npm run dev
```

Create a production build and preview it locally:

```sh
npm run build
npm run preview
```

## Content ownership

The root `docs/` directory is the canonical documentation source.

Before the dev server starts or a production build runs, `scripts/build_site_content.mjs`:

- mirrors the Markdown documentation into Starlight's generated content directory;
- copies the existing neolint logo and icon from `assets/branding/`;
- generates the searchable diagnostics data from `docs/diagnostics.md`;
- verifies that every diagnostic code found in Rust source is present in the reference.

Do not edit generated files under `src/content/docs/docs/` or `src/data/diagnostics.json`. They are ignored by Git and recreated automatically.

## Deployment

The `pages-deploy.yml` workflow builds and deploys the site after pushes to `main`. In repository settings, configure **Settings → Pages → Build and deployment → Source → GitHub Actions**.

The site is configured for the repository URL:

`https://mindwork64.github.io/neolint/`

The `/neolint` base path is intentional; keep it in `astro.config.mjs` while using the repository's default GitHub Pages address.
