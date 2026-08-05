/**
 * The Sigil webview.
 *
 * `cant sigil --format svg` writes the artifact; this puts it on screen and
 * keeps it current. Two entry points, one panel: `show` renders once,
 * `openPreview` re-renders on the trigger the user configured.
 *
 * # The SVG is not trusted markup
 *
 * `rite-sigil` escapes user text and has a hostile-input test suite
 * (`svg_security.rs`), but a webview that runs scripts would make any gap there
 * a code-execution bug in the editor. So the panel sets a CSP with no
 * `script-src`, and `enableScripts` stays false. Nothing in a sigil needs to
 * run — it is a picture.
 */

import * as vscode from "vscode";
import * as path from "path";

export type Renderer = (file: string) => Promise<string>;

let panel: vscode.WebviewPanel | undefined;
let previewFile: string | undefined;
let debounce: NodeJS.Timeout | undefined;

/** No script-src at all, and images only as inline data. */
function page(svg: string, title: string): string {
  return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy"
      content="default-src 'none'; style-src 'unsafe-inline'; img-src data:;">
<title>${escapeHtml(title)}</title>
<style>
  html, body { height: 100%; margin: 0; background: #04060a; }
  body { display: grid; place-items: center; padding: 1rem; box-sizing: border-box; }
  svg { max-width: 100%; max-height: 100%; height: auto; }
  .err { color: #ff5f6e; font: 13px ui-monospace, monospace; white-space: pre-wrap; padding: 1rem; }
</style>
</head>
<body>${svg}</body>
</html>`;
}

function escapeHtml(s: string): string {
  return s.replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] as string,
  );
}

function errorPage(message: string): string {
  return page(`<div class="err">${escapeHtml(message)}</div>`, "Sigil");
}

function ensurePanel(context: vscode.ExtensionContext, title: string): vscode.WebviewPanel {
  if (panel) {
    panel.title = title;
    return panel;
  }
  panel = vscode.window.createWebviewPanel("cantSigil", title, vscode.ViewColumn.Beside, {
    // Deliberately false. See the module comment.
    enableScripts: false,
    retainContextWhenHidden: true,
  });
  panel.onDidDispose(
    () => {
      panel = undefined;
      previewFile = undefined;
    },
    null,
    context.subscriptions,
  );
  return panel;
}

/** Render `file` once into the panel. */
export async function show(
  context: vscode.ExtensionContext,
  render: Renderer,
  file: string,
): Promise<void> {
  const p = ensurePanel(context, `Sigil — ${path.basename(file)}`);
  try {
    p.webview.html = page(await render(file), path.basename(file));
  } catch (e) {
    p.webview.html = errorPage(String(e));
  }
}

/**
 * Render `file` and keep the panel current as it changes.
 *
 * `onType` is debounced at 400ms. A sigil is a whole-program layout, so
 * rendering per keystroke is both slow and useless — the picture is unreadable
 * mid-edit, and most keystrokes leave a program that does not parse.
 */
export async function openPreview(
  context: vscode.ExtensionContext,
  render: Renderer,
  file: string,
): Promise<void> {
  previewFile = file;
  await show(context, render, file);

  const refresh = () => {
    if (!panel || !previewFile) return;
    void show(context, render, previewFile);
  };

  context.subscriptions.push(
    vscode.workspace.onDidSaveTextDocument((doc) => {
      const mode = vscode.workspace.getConfiguration("cant").get<string>("sigil.preview.refresh");
      if (mode === "onSave" || mode === "onType") {
        if (doc.fileName === previewFile) refresh();
      }
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      const mode = vscode.workspace.getConfiguration("cant").get<string>("sigil.preview.refresh");
      if (mode !== "onType" || e.document.fileName !== previewFile) return;
      if (debounce) clearTimeout(debounce);
      debounce = setTimeout(refresh, 400);
    }),
  );
}
