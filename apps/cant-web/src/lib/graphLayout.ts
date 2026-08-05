/**
 * Draw a Cant flow graph as SVG, in the browser.
 *
 * The documentation's pictures come from Graphviz, which is a native binary and
 * not available here. This is a layout engine for the shapes Cant can actually
 * produce, which is a much smaller problem than general graph drawing: a program
 * is a *chain* of stages, and the only nesting is a fork's branches (side by
 * side) and an orbit's body (indented, with one edge back).
 *
 * So there is no force simulation and no crossing minimisation. Blocks are
 * measured bottom-up and placed top-down, which makes the output deterministic —
 * the same program always draws the same picture — and the code short enough to
 * read.
 *
 * Colours match `grammar/palette.json` and `cant graph --format dot`, so a
 * diagram here and one from the CLI are recognisably the same object.
 */

import type { CantGraph, GraphNode } from "./studio";

const ACCENT = "#ff7edb"; // structural operators
const CAPABILITY = "#7ee0ff"; // anything effectful
const TEXT = "#e2e8f0";
const MUTED = "#8b9bb4";
const EDGE = "#64748b";
const CLUSTER = "#1e293b";

const CHAR_W = 6.8; // IBM Plex Mono at 12px
const PAD_X = 14;
const LINE_H = 15;
const BOX_MIN_W = 96;
/** Room above the caption for the node's identifier, so the two never collide. */
const BOX_HEAD = 18;
const GAP_Y = 30; // vertical space between stages, where the arrow goes
const GAP_X = 26; // horizontal space between fork branches
const CLUSTER_PAD = 14;
const MAX_CHARS = 26;

type Block = {
  w: number;
  h: number;
  /** Where an incoming arrow should land, relative to the block's left edge. */
  cx: number;
  draw: (x: number, y: number) => string;
};

function esc(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function truncate(text: string): string {
  const flat = text.split(/\s+/).join(" ");
  return flat.length <= MAX_CHARS ? flat : `${flat.slice(0, MAX_CHARS - 1)}…`;
}

/** The lines a node shows, and the colour it shows them in. */
function caption(node: GraphNode): { lines: string[]; colour: string } {
  switch (node.kind) {
    case "source":
    case "stage": {
      const effectful = node.expr?.effectful;
      return {
        lines: [truncate(node.expr?.text ?? "")],
        colour: effectful ? CAPABILITY : TEXT,
      };
    }
    case "scatter":
      return { lines: ["* scatter"], colour: ACCENT };
    case "collect":
      return { lines: ["[] collect"], colour: ACCENT };
    case "ward":
      return {
        lines: [`?{ ${truncate(node.predicate?.text ?? "")} }`],
        colour: node.predicate?.effectful ? CAPABILITY : ACCENT,
      };
    case "fork":
      return { lines: [`|{ ${node.branches?.length ?? 0} branches }`], colour: ACCENT };
    case "orbit": {
      const lines = ["~orbit"];
      if (node.identity) lines.push(`:by ${truncate(node.identity.text)}`);
      lines.push(`:max ${node.max_items ?? ""}`);
      return { lines, colour: ACCENT };
    }
    default:
      return { lines: [node.kind], colour: TEXT };
  }
}

function box(node: GraphNode): Block {
  const { lines, colour } = caption(node);
  const label = `n${node.id}`;
  const widest = Math.max(...lines.map((l) => l.length), label.length + 2);
  const w = Math.max(BOX_MIN_W, widest * CHAR_W + PAD_X * 2);
  const h = BOX_HEAD + lines.length * LINE_H + 10;
  return {
    w,
    h,
    cx: w / 2,
    draw(x, y) {
      const text = lines
        .map(
          (line, i) =>
            `<text x="${x + w / 2}" y="${y + BOX_HEAD + 12 + i * LINE_H}" text-anchor="middle" ` +
            `font-family="IBM Plex Mono, monospace" font-size="12" fill="${colour}">${esc(line)}</text>`
        )
        .join("");
      return (
        `<g><rect x="${x}" y="${y}" width="${w}" height="${h}" rx="7" ` +
        `fill="#161d28" stroke="${colour}" stroke-opacity="0.55"/>` +
        `<text x="${x + 8}" y="${y + 13}" font-family="IBM Plex Mono, monospace" font-size="9" ` +
        `fill="${MUTED}">${label}</text>${text}</g>`
      );
    },
  };
}

function arrow(x1: number, y1: number, x2: number, y2: number, colour = EDGE, dashed = false): string {
  const dash = dashed ? ` stroke-dasharray="4 3"` : "";
  return (
    `<line x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}" stroke="${colour}" stroke-width="1.4"${dash} ` +
    `marker-end="url(#cant-arrow)"/>`
  );
}

/** A labelled container: a fork branch or an orbit body. */
function cluster(x: number, y: number, w: number, h: number, label: string): string {
  return (
    `<g><rect x="${x}" y="${y}" width="${w}" height="${h}" rx="9" fill="none" ` +
    `stroke="${CLUSTER}" stroke-width="1"/>` +
    `<text x="${x + 8}" y="${y + 12}" font-family="IBM Plex Mono, monospace" font-size="9" ` +
    `fill="${MUTED}">${esc(label)}</text></g>`
  );
}

/**
 * A chain of blocks, stacked with an arrow between each pair.
 *
 * Empty chains are real — an empty fork branch is a validation error that still
 * has to be drawable, because seeing the hole is how someone understands the
 * error.
 */
function chain(blocks: Block[]): Block {
  if (blocks.length === 0) {
    return { w: BOX_MIN_W, h: 24, cx: BOX_MIN_W / 2, draw: () => "" };
  }
  const w = Math.max(...blocks.map((b) => b.w));
  const h = blocks.reduce((sum, b) => sum + b.h, 0) + GAP_Y * (blocks.length - 1);
  return {
    w,
    h,
    cx: w / 2,
    draw(x, y) {
      let out = "";
      let cursor = y;
      blocks.forEach((block, i) => {
        const bx = x + (w - block.w) / 2;
        out += block.draw(bx, cursor);
        if (i < blocks.length - 1) {
          out += arrow(x + w / 2, cursor + block.h, x + w / 2, cursor + block.h + GAP_Y - 6);
        }
        cursor += block.h + GAP_Y;
      });
      return out;
    },
  };
}

export function renderGraphSvg(graph: CantGraph): string {
  const byId = new Map<number, GraphNode>(graph.nodes.map((n) => [n.id, n]));
  const subgraphs = new Map(graph.subgraphs.map((s) => [s.id, s]));

  /** Nodes of a subgraph, in the order it lists them. */
  const membersOf = (id: number): GraphNode[] =>
    (subgraphs.get(id)?.nodes ?? []).map((n) => byId.get(n)).filter((n): n is GraphNode => !!n);

  function blockFor(node: GraphNode): Block {
    if (node.kind === "fork") {
      return forkBlock(node);
    }
    if (node.kind === "orbit") {
      return orbitBlock(node);
    }
    return box(node);
  }

  function forkBlock(node: GraphNode): Block {
    const head = box(node);
    const branches = (node.branches ?? []).map((id, i) => ({
      id,
      index: i,
      body: chain(membersOf(id).map(blockFor)),
    }));
    if (branches.length === 0) return head;

    // Each branch sits in its own cluster, side by side, in branch order —
    // the order emissions join in, whether or not the fork carries `:par` and
    // runs its branches at the same time.
    const clusterW = branches.map((b) => b.body.w + CLUSTER_PAD * 2);
    const clusterH = Math.max(...branches.map((b) => b.body.h)) + CLUSTER_PAD * 2 + 8;
    const rowW = clusterW.reduce((a, b) => a + b, 0) + GAP_X * (branches.length - 1);
    const w = Math.max(head.w, rowW);
    const h = head.h + GAP_Y + clusterH + GAP_Y;

    return {
      w,
      h,
      cx: w / 2,
      draw(x, y) {
        let out = head.draw(x + (w - head.w) / 2, y);
        const rowY = y + head.h + GAP_Y;
        let cursor = x + (w - rowW) / 2;
        const centreX = x + w / 2;
        branches.forEach((branch, i) => {
          const cw = clusterW[i];
          out += cluster(cursor, rowY, cw, clusterH, `branch ${branch.index}`);
          out += branch.body.draw(cursor + CLUSTER_PAD, rowY + CLUSTER_PAD + 8);
          // In and out: dashed, because entering a branch is not the main flow.
          out += arrow(centreX, y + head.h, cursor + cw / 2, rowY - 4, EDGE, true);
          out += arrow(
            cursor + cw / 2,
            rowY + clusterH,
            centreX,
            rowY + clusterH + GAP_Y - 6,
            EDGE,
            true
          );
          cursor += cw + GAP_X;
        });
        return out;
      },
    };
  }

  function orbitBlock(node: GraphNode): Block {
    const head = box(node);
    const body = chain(node.body === undefined ? [] : membersOf(node.body).map(blockFor));
    const clusterW = body.w + CLUSTER_PAD * 2;
    const clusterH = body.h + CLUSTER_PAD * 2 + 8;
    // Room on the right for the feedback edge to come back up the outside, plus
    // its label — 34px fitted the line and clipped the word to "feedbac".
    const FEEDBACK = 74;
    const w = Math.max(head.w, clusterW) + FEEDBACK;
    const h = head.h + GAP_Y + clusterH;

    return {
      w,
      h,
      cx: (Math.max(head.w, clusterW) + FEEDBACK) / 2 - FEEDBACK / 2,
      draw(x, y) {
        const innerW = w - FEEDBACK;
        let out = head.draw(x + (innerW - head.w) / 2, y);
        const bodyY = y + head.h + GAP_Y;
        const bodyX = x + (innerW - clusterW) / 2;
        out += cluster(bodyX, bodyY, clusterW, clusterH, "orbit body");
        out += body.draw(bodyX + CLUSTER_PAD, bodyY + CLUSTER_PAD + 8);
        out += arrow(x + innerW / 2, y + head.h, x + innerW / 2, bodyY - 4, EDGE, true);
        // The one cycle in the language, drawn as one: down the right-hand side
        // and back into the orbit node.
        const right = bodyX + clusterW + 10;
        const top = y + head.h / 2;
        const bottom = bodyY + clusterH - 12;
        out +=
          `<path d="M ${bodyX + clusterW} ${bottom} L ${right} ${bottom} L ${right} ${top} ` +
          `L ${x + innerW / 2 + head.w / 2} ${top}" fill="none" stroke="${ACCENT}" ` +
          `stroke-width="1.6" marker-end="url(#cant-arrow-accent)"/>` +
          `<text x="${right + 4}" y="${(top + bottom) / 2}" font-family="IBM Plex Mono, monospace" ` +
          `font-size="9" fill="${ACCENT}">feedback</text>`;
        return out;
      },
    };
  }

  // The top-level flow: every node that is not inside a subgraph, in id order.
  // Identifiers are assigned by a depth-first walk in source order, so that is
  // already flow order.
  const top = graph.nodes
    .filter((n) => n.subgraph === undefined || n.subgraph === null)
    .sort((a, b) => a.id - b.id);
  const root = chain(top.map(blockFor));

  const M = 18;
  const width = Math.ceil(root.w + M * 2);
  const height = Math.ceil(root.h + M * 2);
  const defs =
    `<defs>` +
    `<marker id="cant-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" ` +
    `markerHeight="6" orient="auto-start-reverse">` +
    `<path d="M 0 0 L 10 5 L 0 10 z" fill="${EDGE}"/></marker>` +
    `<marker id="cant-arrow-accent" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" ` +
    `markerHeight="6" orient="auto-start-reverse">` +
    `<path d="M 0 0 L 10 5 L 0 10 z" fill="${ACCENT}"/></marker>` +
    `</defs>`;

  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" ` +
    `viewBox="0 0 ${width} ${height}" role="img" aria-label="The flow graph for this program">` +
    `${defs}${root.draw(M, M)}</svg>`
  );
}
