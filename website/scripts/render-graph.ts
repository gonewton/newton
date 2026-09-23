// Renders the example workflow as the site's illustration.
//
// Newton's own `workflow graph` output is the source of truth: this script
// parses that DOT, restyles it in the site's palette, lays it out with
// Graphviz (via @viz-js/viz, no system install needed) and writes:
//
//   assets/scan-and-fix.dot   raw `newton workflow graph` output
//   assets/scan-and-fix.svg   styled, standalone (light + dark)
//   index.html                the same SVG inlined between the graph markers
//
// Usage: pnpm run graph   (needs `newton` on PATH)

import { instance } from "@viz-js/viz";

const root = new URL("..", import.meta.url).pathname;
const workflow = "examples/scan-and-fix.yaml";
const name = "scan-and-fix";

// Graphviz can't read CSS variables, so the layout uses sentinel colours that
// are swapped for var(--token) afterwards. The page (or the standalone
// file's <style>) then decides the real colour, including in dark mode.
const TOKENS = {
  "#000001": "ink",
  "#000002": "text",
  "#000003": "muted",
  "#000004": "panel",
  "#000005": "line",
  "#000006": "accent",
  "#000007": "amber",
  "#000008": "bg",
} as const;
const C = Object.fromEntries(
  Object.entries(TOKENS).map(([hex, token]) => [token, hex]),
) as Record<(typeof TOKENS)[keyof typeof TOKENS], string>;

// Layout font: Courier has the same 0.6em advance as JetBrains Mono, so
// Graphviz sizes boxes correctly for the font the page actually renders.
const LAYOUT_FONT = "Courier";
const PAGE_FONT = "'JetBrains Mono', ui-monospace, Menlo, monospace";

type Node = { id: string; task: string; operator: string };
type Edge = { from: string; to: string; when?: string };

function newtonGraph(): string {
  const proc = Bun.spawnSync(["newton", "workflow", "graph", workflow], {
    cwd: root,
  });
  if (proc.exitCode !== 0) {
    throw new Error(`newton workflow graph failed:\n${proc.stderr.toString()}`);
  }
  return proc.stdout.toString();
}

function parse(dot: string): { nodes: Node[]; edges: Edge[] } {
  const nodes: Node[] = [];
  const edges: Edge[] = [];
  for (const line of dot.split("\n")) {
    const node = line.match(/^\s*(\d+) \[ label = "(.*)" \]$/);
    if (node) {
      const [task, operator] = node[2].split("\\l");
      nodes.push({ id: node[1], task, operator });
      continue;
    }
    const edge = line.match(/^\s*(\d+) -> (\d+) \[ label = "(.*)" \]$/);
    if (edge) {
      const when = edge[3].match(/^when:(.*) priority=\d+$/)?.[1];
      edges.push({ from: edge[1], to: edge[2], when: when && condition(when) });
    }
  }
  if (nodes.length === 0) throw new Error("no nodes parsed from newton's DOT output");
  return { nodes, edges };
}

// `tasks.test.output.stdout` -> `stdout`; unescape newton's DOT quoting.
function condition(expr: string): string {
  return expr
    .replace(/\\+"/g, '"')
    .replace(/tasks\.[\w-]+\.output\./g, "")
    .replace(/"TEST_STATUS: (\w+)"/, '"$1"');
}

const esc = (s: string) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

function styled({ nodes, edges }: { nodes: Node[]; edges: Edge[] }): string {
  const entry = nodes[0].id;
  const outDegree = new Map<string, number>();
  for (const e of edges) outDegree.set(e.from, (outDegree.get(e.from) ?? 0) + 1);

  const lines = [
    "digraph workflow {",
    `  graph [rankdir=TB, nodesep=0.5, ranksep=0.42, bgcolor="transparent", pad=0.1, fontname="${LAYOUT_FONT}"]`,
    `  node [shape=box, style="rounded,filled", fillcolor="${C.panel}", color="${C.line}", penwidth=1.2, fontname="${LAYOUT_FONT}", margin="0.22,0.08", width=2.5]`,
    `  edge [color="${C.muted}", fontcolor="${C.muted}", fontname="${LAYOUT_FONT}", fontsize=11, arrowsize=0.6, penwidth=1.1]`,
  ];

  for (const n of nodes) {
    const human = n.operator.startsWith("Human");
    const terminal = n.operator === "NoOpOperator";
    const border = n.id === entry || terminal ? C.accent : human ? C.amber : C.line;
    const title = terminal ? C.bg : C.ink;
    const sub = terminal ? C.bg : human ? C.amber : C.muted;
    const fill = terminal ? C.accent : C.panel;
    const label =
      `<<font point-size="14" color="${title}"><b>${esc(n.task)}</b></font><br/>` +
      `<font point-size="10.5" color="${sub}">${esc(n.operator)}</font>>`;
    lines.push(
      `  ${n.id} [label=${label}, color="${border}", fillcolor="${fill}", class="task ${terminal ? "terminal" : human ? "human" : "op"}", group="spine"]`,
    );
  }

  for (const e of edges) {
    const back = Number(e.to) < Number(e.from);
    const attrs: string[] = [];
    if (e.when) {
      attrs.push(`xlabel=" ${esc(e.when)} "`, `color="${C.accent}"`, `fontcolor="${C.accent}"`, "weight=10");
    } else if ((outDegree.get(e.from) ?? 0) > 1) {
      attrs.push(`xlabel=" else "`, `style=dashed`);
    } else {
      attrs.push(`color="${C.text}"`, "weight=10");
    }
    // The spine reads top to bottom; loops return on the left, early exits
    // leave on the right, so neither crosses it.
    if (back) attrs.push("constraint=false", "tailport=w", "headport=w");
    else if (!e.when && (outDegree.get(e.from) ?? 0) > 1) attrs.push("tailport=e", "headport=e");
    lines.push(`  ${e.from} -> ${e.to} [${attrs.join(", ")}]`);
  }

  lines.push("}");
  return lines.join("\n");
}

function themed(svg: string): string {
  let out = svg
    .replace(/<\?xml[^>]*>\s*/, "")
    .replace(/<!DOCTYPE[^>]*>\s*/, "")
    .replace(/<!--[\s\S]*?-->\s*/g, "")
    .replace(/<title>[^<]*<\/title>\s*/g, "")
    .replace(/font-family="[^"]*"/g, `font-family="${PAGE_FONT}"`);
  for (const [hex, token] of Object.entries(TOKENS)) {
    for (const attr of ["fill", "stroke"]) {
      out = out.replaceAll(`${attr}="${hex}"`, `${attr}="var(--${token})"`);
    }
  }
  // Graphviz's transparent background polygon.
  out = out.replace(/<polygon fill="transparent"[^>]*\/>\s*/, "");
  const leftover = out.match(/#00000\d/);
  if (leftover) throw new Error(`unmapped sentinel colour ${leftover[0]}`);
  return out.replace(
    /<svg /,
    `<svg role="img" aria-labelledby="graph-title" class="workflow-graph" `,
  ).replace(/(<svg[^>]*>)/, `$1\n<title id="graph-title">scan-and-fix: scan, triage, approve, develop, test (retries develop until tests pass), done</title>`);
}

const STANDALONE_STYLE = `<style>
svg{font-variant-ligatures:none;--bg:#eef0ec;--panel:#e3e7e1;--line:#d6dbd3;--ink:#1f2a24;--text:#3e4a43;--muted:#5f6a63;--accent:#1f7a55;--amber:#9a5b12;background:var(--bg)}
@media (prefers-color-scheme:dark){svg{--bg:#151916;--panel:#1e231f;--line:#2c332d;--ink:#e6ebe5;--text:#c3cbc4;--muted:#8f9a91;--accent:#4fb487;--amber:#d49a4a}}
</style>`;

const raw = newtonGraph();
const viz = await instance();
const svg = themed(viz.renderString(styled(parse(raw)), { format: "svg", engine: "dot" }));

await Bun.write(`${root}assets/${name}.dot`, raw);
await Bun.write(
  `${root}assets/${name}.svg`,
  `<?xml version="1.0" encoding="UTF-8"?>\n` +
    svg.replace(/(<svg[^>]*>)/, `$1\n${STANDALONE_STYLE}`),
);

const indexPath = `${root}index.html`;
const html = await Bun.file(indexPath).text();
const start = "<!-- graph:start -->";
const end = "<!-- graph:end -->";
const a = html.indexOf(start);
const b = html.indexOf(end);
if (a < 0 || b < a) throw new Error(`index.html is missing ${start} … ${end}`);
await Bun.write(indexPath, html.slice(0, a + start.length) + "\n" + svg.trim() + "\n" + html.slice(b));

console.log(`wrote assets/${name}.dot, assets/${name}.svg and inlined the graph in index.html`);
