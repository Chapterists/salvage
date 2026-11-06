# Salvage site

The source for the Salvage website: a static site built with [Astro](https://astro.build). It isn't hosted anywhere yet.

## Run it

You need Node.js 22.23.4 or newer.

```sh
cd site
npm ci
npm run dev       # http://localhost:4321, reloads as you edit
npm run build     # writes the static site to site/dist/
```

CI builds the site on every pull request that touches it (`.github/workflows/site.yml`). It doesn't deploy.

## Where things are

| Path | What it is |
| --- | --- |
| `src/pages/` | One file per page. `contributing.astro` renders `CONTRIBUTING.md` from the repository root, so edit that file, not the page. |
| `src/content/logbook/` | Logbook entries, one markdown file each. |
| `src/pages/charts/` | One page per chart. `capturing-memory-runs.astro` renders `docs/benchmark-recipe.md`, so edit that file, not the page. |
| `src/examples/` | Real terminal output shown in the charts, captured with `--color never` and `COLUMNS=80`. Doctor's comes from one Mac; soundings' comes from the sample captures in `crates/salvage/tests/fixtures/`. Regenerate them when the output changes. |
| `src/pages/manifest.astro` | The toolbox, one row per tool. To add a tool, draw its icon as a component (see `IconSalvageCli.astro`: a 128-unit rounded tile, waterline, ship colors) and add an entry. |
| `src/components/GetSalvage.astro` | The one place the site says how to install `salvage`. Change it (and the README's "Get it" section) when there's a release. |
| `src/layouts/` | `Base.astro` (header, footer, theme) and `Page.astro` (inner pages with a title band). |
| `src/components/` | The mark, wordmark, waterline dividers, the hero bow and the workstream icons. All hand-drawn SVG. |
| `src/styles/tokens.css` | Colors, type, spacing, and the night and day themes. |
| `public/fonts/` | Self-hosted fonts, each with its license. |
| `public/` (icons) | `favicon.svg`, `favicon.ico`, `favicon-32.png`, `apple-touch-icon.png`, `icon-192.png`, `icon-512.png` and `site.webmanifest`, all drawn from the lifebuoy. The PNGs are rendered from `favicon.svg` and `assets/brand/salvage-app-icon.svg`. |
| `public/social-preview.png` | The 1280 x 640 image link previews use, a copy of `assets/brand/social-preview.png`. Its source is `assets/brand/social-preview.html`. |

## Add a logbook entry

Create `src/content/logbook/YYYY-MM-DD-NN-short-name.md`, where `NN` orders entries made on the same day:

```md
---
title: What changed, in a few words
date: 2026-11-06
---

One or two plain sentences: what changed and why it matters.
```

## Design notes

- **Colors** come from a working ship: Night Sea ground, Hull Navy surfaces, Antifouling Red below the waterline, Rescue Orange for the one loud moment, Crane Yellow for highlights and status, Sea Glass for links and data, Foam for text. `tokens.css` lists the contrast ratio of each pairing used for text. Antifouling Red is only for fills and large display type on dark grounds.
- **Night is the default.** Day mode changes the reading ground. The header, hero, title bands and footer keep their ship colors in both modes.
- **Type:** Archivo (SIL Open Font License) set at its widest for hull-lettering headlines and at normal width for reading, and Martian Mono (SIL Open Font License) for labels, code and numbers. Numbers use tabular figures.
- **The waterline** is the section break: a flat, crisp line between two colors, angled or gently rolling.
- **Rescue Orange, big, once per page.** The home page uses it for one full-width band. Everywhere else it stays small: the lifebuoy, the bulwark, a list marker.
- **Labels:** headings carry the structure. No small spaced-capital labels above them. The one mono label style is for dates.
- **Motion** is small and optional: the lifebuoy bobs, and the workstream icons move on hover. All of it stops under `prefers-reduced-motion`.
- **Copy:** plain and specific, numbers over adjectives, American spelling, no em dashes. Never describe something as available before it ships.
