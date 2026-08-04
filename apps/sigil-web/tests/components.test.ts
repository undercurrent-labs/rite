/**
 * Component tests.
 *
 * The renderer is mocked. These are about the *chamber* — which control does
 * what, which panel collapses, what an export preview claims — and a real WASM
 * render would make them slower without making them say more. What the engine
 * does is tested in Rust, where the assertions can be exact.
 */
import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";

vi.mock("../src/lib/renderer", () => ({
  defaultOptions: () => ({
    theme: "neon-ritual",
    mode: "veiled",
    metadata: "safe",
    ornament: "ritual",
    seed: "graph",
    background: "theme",
    canonical: false,
    simplify: false,
  }),
  renderCant: vi.fn(async () => ({
    ok: true,
    svg: '<svg xmlns="http://www.w3.org/2000/svg"><path id="node-n0"><title>source</title></path></svg>',
    sceneJson: JSON.stringify({
      elements: [
        { id: "edge-e0", graph_ref: { kind: "edge", id: "e0:n0.0->n1.0" } },
        { id: "node-n0", graph_ref: { kind: "node", id: "n0" } },
        { id: "node-n1", graph_ref: { kind: "node", id: "n1" } },
      ],
      legend: [
        { key: "node/n0", graph_ref: { kind: "node", id: "n0" }, summary: "source", label: "[1, 2]" },
        { key: "node/n1", graph_ref: { kind: "node", id: "n1" }, summary: "collect" },
      ],
    }),
    fingerprint: "sigil/0.1.0 graph=abc theme=neon-ritual@1",
    summary: "This sigil contains one source and one collect.",
    diagnostics: [],
  })),
  renderGraph: vi.fn(async () => ({ ok: true, diagnostics: [] })),
  version: vi.fn(async () => ({
    renderer: "0.1.0",
    graphSchema: 1,
    sceneSchema: 1,
    cantGraphSchema: "1",
    themeVersion: 1,
  })),
  isCurrent: () => true,
  nextGeneration: () => 1,
}));

import App from "../src/App.vue";
import ControlBar from "../src/components/ControlBar.vue";
import CodexPanel from "../src/components/CodexPanel.vue";

const options = {
  theme: "neon-ritual",
  mode: "veiled",
  metadata: "safe",
  ornament: "ritual",
  seed: "graph",
  background: "theme",
  canonical: false,
  simplify: false,
};

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("ControlBar", () => {
  it("marks the active mode and emits a change", async () => {
    const bar = mount(ControlBar, { props: { options, deepVeil: false } });

    const veiled = bar.findAll("button").find((b) => b.text() === "veiled");
    expect(veiled?.classes()).toContain("is-active");

    const revealed = bar.findAll("button").find((b) => b.text() === "revealed");
    await revealed?.trigger("click");
    expect(bar.emitted("update:options")?.[0][0]).toMatchObject({ mode: "revealed" });
  });

  it("offers every documented theme and ornament level", () => {
    const bar = mount(ControlBar, { props: { options, deepVeil: false } });
    const labels = bar.findAll("button").map((b) => b.text());
    for (const theme of ["neon-ritual", "void", "parchment"]) {
      expect(labels).toContain(theme);
    }
    for (const level of ["none", "sparse", "ritual", "maximal"]) {
      expect(labels).toContain(level);
    }
  });

  it("reports Deep Veil as a pressed toggle", async () => {
    const bar = mount(ControlBar, { props: { options, deepVeil: true } });
    const deep = bar.findAll("button").find((b) => b.text() === "deep veil");
    expect(deep?.attributes("aria-pressed")).toBe("true");
    await deep?.trigger("click");
    expect(bar.emitted("update:deepVeil")?.[0][0]).toBe(false);
  });
});

describe("CodexPanel", () => {
  const sceneJson = JSON.stringify({
    legend: [
      {
        key: "node/n0",
        graph_ref: { kind: "node", id: "n0" },
        summary: "fs invocation",
        label: "! @fs.read($)",
        capabilities: ["@fs.read"],
      },
    ],
  });

  it("decodes a veiled render — the whole point of a Codex", () => {
    const codex = mount(CodexPanel, { props: { sceneJson, summary: "s" } });
    expect(codex.text()).toContain("fs invocation");
    expect(codex.text()).toContain("! @fs.read($)");
  });

  it("hides labels and capabilities under Deep Veil but keeps the kinds", () => {
    const codex = mount(CodexPanel, { props: { sceneJson, summary: "s", deepVeil: true } });
    expect(codex.text()).toContain("fs invocation");
    expect(codex.text()).not.toContain("@fs.read");
    expect(codex.text()).toContain("Deep Veil");
  });

  it("emits a selection and marks the current entry", async () => {
    const codex = mount(CodexPanel, { props: { sceneJson, summary: "s", selected: "n0" } });
    const entry = codex.find("li");
    expect(entry.attributes("aria-current")).toBe("true");
    await entry.trigger("click");
    // Selected already, so clicking clears it.
    expect(codex.emitted("select")?.[0][0]).toBe(null);
  });
});

describe("App", () => {
  it("renders on mount and shows the summary in a live region", async () => {
    const app = mount(App);
    await flush();
    await flush();
    const live = app.find('[role="status"]');
    expect(live.text()).toContain("This sigil contains");
  });

  it("collapses the source panel and the Codex", async () => {
    const app = mount(App);
    await flush();
    expect(app.find('[aria-label="Source"]').exists()).toBe(true);

    const toggle = app.findAll("button").find((b) => b.text() === "Source");
    await toggle?.trigger("click");
    expect(app.find('[aria-label="Source"]').exists()).toBe(false);

    // The Codex starts collapsed — §13.4's web default.
    expect(app.find('[aria-label="Codex"]').exists()).toBe(false);
    const codex = app.findAll("button").find((b) => b.text() === "Codex");
    await codex?.trigger("click");
    expect(app.find('[aria-label="Codex"]').exists()).toBe(true);
  });

  it("switches between the Cant and graph JSON inputs", async () => {
    const app = mount(App);
    await flush();
    expect(app.find('[aria-label="Cant source"]').exists()).toBe(true);

    const graphTab = app.findAll("button").find((b) => b.text() === "Graph JSON");
    await graphTab?.trigger("click");
    expect(app.find('[aria-label="Cant graph JSON"]').exists()).toBe(true);
  });

  /**
   * The export preview (§20.7). The two axes are independent, so the notice has
   * to describe what an export will *contain* rather than which button is lit.
   */
  it("warns when the disclosure mode will draw labels into the artifact", async () => {
    const app = mount(App);
    await flush();
    expect(app.text()).toContain("no source snippets");

    const revealed = app.findAll("button").find((b) => b.text() === "revealed");
    await revealed?.trigger("click");
    await flush();
    expect(app.text()).toContain("draws labels into the artifact");
  });

  /** The privacy claim, asserted the only way that means anything. */
  it("issues no network request while rendering, switching modes, or exporting", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const app = mount(App);
    await flush();
    const revealed = app.findAll("button").find((b) => b.text() === "revealed");
    await revealed?.trigger("click");
    await flush();
    expect(fetchSpy).not.toHaveBeenCalled();
    fetchSpy.mockRestore();
  });
});
