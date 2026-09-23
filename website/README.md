# Newton website

A static, single-page site for Newton. It has no build step: `index.html`, `style.css` and `copy.js` are served as they are.

```
website/
├── index.html                  the page (the workflow graph is inlined in it)
├── style.css                   design tokens, light + dark
├── copy.js                     "copy" buttons on code panels
├── favicon.svg
├── examples/scan-and-fix.yaml  the workflow shown in §3
├── assets/scan-and-fix.dot     raw `newton workflow graph` output
├── assets/scan-and-fix.svg     the styled graph, standalone
└── scripts/render-graph.ts     regenerates the two assets and the inlined SVG
```

## Preview

```bash
cd website && bunx serve .
```

Opening `index.html` directly in a browser also works.

## The workflow graph

The illustration in §3 comes from Newton itself. `scripts/render-graph.ts` does the following:

1. Runs `newton workflow graph examples/scan-and-fix.yaml`.
2. Parses the DOT it prints.
3. Restyles the graph in the site's palette:
   - the main path runs top to bottom;
   - retries loop back on the left;
   - early exits leave on the right.
4. Lays out the graph with Graphviz compiled to WebAssembly (`@viz-js/viz`), so no system Graphviz is needed.
5. Rewrites the colours as CSS variables, so the SVG follows light and dark mode.

After changing the example workflow, regenerate the graph. This needs `newton` on `PATH`:

```bash
cd website
pnpm install
newton workflow validate examples/scan-and-fix.yaml
pnpm run graph
```

The script replaces everything between `<!-- graph:start -->` and `<!-- graph:end -->` in `index.html`. Don't edit the inlined SVG by hand.

## Design

This is direction "B · Notebook":

- Instrument Serif for display text, JetBrains Mono for everything else.
- A 720px reading column.
- `§N` section markers.
- Code panels in a sage-and-green palette. Amber is used only for human gates and blocked work.

All colours are tokens on `:root` in `style.css`. Dark mode follows `prefers-color-scheme`. You can also force it with `data-theme="dark"` or `data-theme="light"` on `<html>`.
