import * as vscode from "vscode";
import * as path from "path";
import * as os from "os";
import { LanguageClient, LanguageClientOptions, ServerOptions, TransportKind } from "vscode-languageclient/node";
import { spawn } from "child_process";
import { cantLenses, capabilitiesNamed } from "./lenses";
import * as sigil from "./sigil";
import { toDiagnostics } from "./diagnostics";

let client: LanguageClient | undefined;
let glyphPresentation = false;
const glyphDecorations: vscode.TextEditorDecorationType[] = [];

function findBinary(setting: string, fallbacks: string[]): string {
  const configured = vscode.workspace.getConfiguration("rite").get<string>(setting)?.trim();
  if (configured) return configured;
  for (const f of fallbacks) {
    // PATH resolution is deferred to spawn; return bare name
    if (f) return f;
  }
  return fallbacks[0] || "rite";
}

function runRite(args: string[], cwd?: string): Promise<string> {
  const bin = findBinary("binaryPath", ["rite"]);
  return new Promise((resolve, reject) => {
    const child = spawn(bin, args, { cwd, shell: false });
    let out = "";
    let err = "";
    child.stdout.on("data", (d: Buffer) => (out += d.toString()));
    child.stderr.on("data", (d: Buffer) => (err += d.toString()));
    child.on("error", reject);
    child.on("close", (code: number | null) => {
      if (code === 0) resolve(out);
      else reject(new Error(err || out || `exit ${code}`));
    });
  });
}

function runCant(args: string[], cwd?: string): Promise<string> {
  const bin = findBinary("cantBinaryPath", ["cant"]);
  return new Promise((resolve, reject) => {
    const child = spawn(bin, args, { cwd, shell: false });
    let out = "";
    let err = "";
    child.stdout.on("data", (d: Buffer) => (out += d.toString()));
    child.stderr.on("data", (d: Buffer) => (err += d.toString()));
    child.on("error", reject);
    // `cant check` prints "ok" on stdout; a failure puts the diagnostic on
    // stderr and exits non-zero. Both are worth showing, so the rejection
    // carries whichever stream has text.
    child.on("close", (code: number | null) => {
      if (code === 0) resolve(out);
      else reject(new Error(err || out || `exit ${code}`));
    });
  });
}

/**
 * Whether a lens-driven run grants everything.
 *
 * Off by default. A lens is one click, and a program you opened to *read*
 * should not get the filesystem because you clicked Run on it. The tooltip
 * names the capabilities the source mentions so the exit-5 is not a surprise.
 */
function lensPermissions(text: string): string[] {
  const allowAll = vscode.workspace.getConfiguration("rite").get<boolean>("codeLens.allowAll");
  if (allowAll) return ["--allow-all"];
  void capabilitiesNamed(text);
  return [];
}

const output = (name: string) => {
  const ch = vscode.window.createOutputChannel(name);
  return (text: string) => {
    ch.appendLine(text);
    ch.show(true);
  };
};

async function withActiveFile(
  languageId: string,
  what: string,
  cb: (doc: vscode.TextDocument) => Promise<void>,
) {
  const doc = vscode.window.activeTextEditor?.document;
  if (!doc || doc.languageId !== languageId) {
    vscode.window.showErrorMessage(`Open a ${what} file first`);
    return;
  }
  await doc.save();
  await cb(doc);
}

async function withActiveRiteFile(cb: (doc: vscode.TextDocument) => Promise<void>) {
  const doc = vscode.window.activeTextEditor?.document;
  if (!doc || doc.languageId !== "rite") {
    vscode.window.showErrorMessage("Open a .rite file first");
    return;
  }
  await doc.save();
  await cb(doc);
}

export async function activate(context: vscode.ExtensionContext) {
  const lspPath = findBinary("lspPath", ["rite-lsp", "rite"]);
  const serverOptions: ServerOptions = {
    run: { command: lspPath === "rite" ? "rite" : lspPath, args: lspPath === "rite" ? ["lsp"] : [], transport: TransportKind.stdio },
    debug: { command: lspPath === "rite" ? "rite" : lspPath, args: lspPath === "rite" ? ["lsp"] : [], transport: TransportKind.stdio },
  };
  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "rite" }],
    synchronize: { configurationSection: "rite" },
  };

  try {
    client = new LanguageClient("rite", "Rite Language Server", serverOptions, clientOptions);
    await client.start();
  } catch (e) {
    vscode.window.showWarningMessage(
      `Rite LSP not started (${e}). Syntax highlighting still works. Install rite-lsp or set rite.lspPath.`
    );
  }

  const cmds: [string, () => Promise<void>][] = [
    ["rite.runFile", async () =>
      withActiveRiteFile(async (doc) => {
        const out = await runRite(["run", doc.fileName, "--allow-all"], path.dirname(doc.fileName));
        const ch = vscode.window.createOutputChannel("Rite");
        ch.appendLine(out);
        ch.show();
      })],
    ["rite.runSelection", async () => {
      const ed = vscode.window.activeTextEditor;
      if (!ed) return;
      const sel = ed.document.getText(ed.selection);
      const tmp = path.join(os.tmpdir(), "rite-selection.rite");
      await vscode.workspace.fs.writeFile(vscode.Uri.file(tmp), Buffer.from(sel, "utf8"));
      const out = await runRite(["run", tmp, "--allow-all"]);
      vscode.window.showInformationMessage(out.slice(0, 200));
    }],
    ["rite.checkFile", async () =>
      withActiveRiteFile(async (doc) => {
        const out = await runRite(["check", doc.fileName]);
        vscode.window.showInformationMessage(out.trim() || "ok");
      })],
    ["rite.compileFile", async () =>
      withActiveRiteFile(async (doc) => {
        const out = await runRite(["build", doc.fileName, "--allow-all", "--emit-rust"]);
        vscode.window.showInformationMessage(out.trim());
      })],
    ["rite.formatDocument", async () =>
      withActiveRiteFile(async (doc) => {
        await runRite(["fmt", doc.fileName]);
        await vscode.commands.executeCommand("workbench.action.files.revert");
      })],
    ["rite.convertGlyph", async () => convertDialect("glyph")],
    ["rite.convertAscii", async () => convertDialect("ascii")],
    ["rite.toggleGlyphPresentation", async () => {
      glyphPresentation = !glyphPresentation;
      const conf = vscode.workspace.getConfiguration("rite");
      await conf.update("renderOnlyGlyph", glyphPresentation, true);
      applyGlyphDecorations();
      vscode.window.showInformationMessage(
        glyphPresentation ? "Glyph presentation ON (source unchanged)" : "Glyph presentation OFF"
      );
    }],
    ["rite.openRepl", async () => {
      const term = vscode.window.createTerminal("Rite REPL");
      term.sendText(`${findBinary("binaryPath", ["rite"])} repl --allow-all`);
      term.show();
    }],
    ["rite.openStudio", async () => {
      const url = vscode.workspace.getConfiguration("rite").get<string>("studioUrl") || "http://127.0.0.1:4041";
      await vscode.env.openExternal(vscode.Uri.parse(url));
    }],
    ["rite.showGeneratedRust", async () =>
      withActiveRiteFile(async (doc) => {
        const out = await runRite(["emit-rust", doc.fileName]);
        const doc2 = await vscode.workspace.openTextDocument({ language: "rust", content: out });
        await vscode.window.showTextDocument(doc2);
      })],
    ["rite.showSyntaxTree", async () =>
      withActiveRiteFile(async (doc) => {
        const out = await runRite(["syntax-tree", doc.fileName, "--json"]);
        const doc2 = await vscode.workspace.openTextDocument({ language: "json", content: out });
        await vscode.window.showTextDocument(doc2);
      })],
    ["rite.showSemanticIr", async () =>
      withActiveRiteFile(async (doc) => {
        const out = await runRite(["semantic-ir", doc.fileName, "--json"]);
        const doc2 = await vscode.workspace.openTextDocument({ language: "json", content: out });
        await vscode.window.showTextDocument(doc2);
      })],
    ["rite.openDocumentation", async () => {
      await vscode.env.openExternal(vscode.Uri.file(path.join(vscode.workspace.workspaceFolders?.[0]?.uri.fsPath || "", "docs/generated/html/index.html")));
    }],
    ["rite.restartLsp", async () => {
      if (client) {
        await client.stop();
        await client.start();
        vscode.window.showInformationMessage("Rite LSP restarted");
      }
    }],
    ["rite.showLspLogs", async () => {
      await vscode.commands.executeCommand("workbench.action.output.show");
    }],
  ];

  // ---- Cant
  //
  // Cant has no language server, so its lenses come from the extension. Rite's
  // come from `rite-lsp`, which resolves the program properly — see
  // `code_lens` there.
  const cantOut = output("Cant");

  const cantCmds: [string, () => Promise<void>][] = [
    ["cant.runFile", async () =>
      withActiveFile("cant", "Cant", async (doc) => {
        const perms = lensPermissions(doc.getText());
        cantOut(await runCant(["run", doc.fileName, ...perms], path.dirname(doc.fileName)));
      })],
    ["cant.checkFile", async () =>
      withActiveFile("cant", "Cant", async (doc) => {
        const out = await runCant(["check", doc.fileName], path.dirname(doc.fileName));
        vscode.window.showInformationMessage(out.trim() || "ok");
      })],
    ["cant.explainFile", async () =>
      withActiveFile("cant", "Cant", async (doc) => {
        cantOut(await runCant(["explain", doc.fileName], path.dirname(doc.fileName)));
      })],
    ["cant.expandFile", async () =>
      withActiveFile("cant", "Cant", async (doc) => {
        const out = await runCant(["expand", doc.fileName], path.dirname(doc.fileName));
        const shown = await vscode.workspace.openTextDocument({ language: "rite", content: out });
        await vscode.window.showTextDocument(shown, vscode.ViewColumn.Beside);
      })],
    ["cant.graphFile", async () =>
      withActiveFile("cant", "Cant", async (doc) => {
        const out = await runCant(["graph", doc.fileName, "--format", "dot"], path.dirname(doc.fileName));
        const shown = await vscode.workspace.openTextDocument({ language: "dot", content: out });
        await vscode.window.showTextDocument(shown, vscode.ViewColumn.Beside);
      })],
    ["cant.showSigil", async () =>
      withActiveFile("cant", "Cant", async (doc) => {
        await sigil.show(context, renderSigil, doc.fileName);
      })],
    ["cant.openSigilPreview", async () =>
      withActiveFile("cant", "Cant", async (doc) => {
        await sigil.openPreview(context, renderSigil, doc.fileName);
      })],
    ["cant.openRepl", async () => {
      const term = vscode.window.createTerminal("Cant REPL");
      term.sendText(`${findBinary("cantBinaryPath", ["cant"])} repl`);
      term.show();
    }],
    ["rite.runAt", async () => {
      // Invoked by an LSP lens, which passes the line it sits on. Running from
      // a point is not a thing the CLI does, so this runs the file — the lens
      // exists to be one click, not to slice the program.
      await vscode.commands.executeCommand("rite.runFile");
    }],
  ];

  for (const [id, fn] of cantCmds) {
    context.subscriptions.push(
      vscode.commands.registerCommand(id, () => fn().catch((e) => vscode.window.showErrorMessage(String(e))))
    );
  }

  // Inline errors for Cant. See `diagnostics.ts` for why this is a CLI call
  // rather than a language server.
  const cantDiagnostics = vscode.languages.createDiagnosticCollection("cant");
  context.subscriptions.push(cantDiagnostics);

  const checkCant = async (doc: vscode.TextDocument) => {
    if (doc.languageId !== "cant" || doc.uri.scheme !== "file") return;
    let json = "";
    try {
      json = await runCant(["check", doc.fileName, "--json-errors"], path.dirname(doc.fileName));
      // Exit 0 with no diagnostics: clear whatever was there.
      cantDiagnostics.set(doc.uri, []);
      if (!json.trim()) return;
    } catch (e) {
      // `cant check` exits non-zero *because* there are diagnostics, and puts
      // them on stdout as JSON. The rejection carries that text.
      json = String((e as Error).message ?? e);
    }
    const found = toDiagnostics(json, doc.getText());
    cantDiagnostics.set(
      doc.uri,
      found.map((d) => {
        const diag = new vscode.Diagnostic(
          new vscode.Range(d.range.start.line, d.range.start.character, d.range.end.line, d.range.end.character),
          d.message,
          d.severity === "warning" ? vscode.DiagnosticSeverity.Warning : vscode.DiagnosticSeverity.Error,
        );
        diag.code = d.code;
        diag.source = "cant";
        return diag;
      }),
    );
  };

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((d) => void checkCant(d)),
    vscode.workspace.onDidSaveTextDocument((d) => void checkCant(d)),
    vscode.workspace.onDidCloseTextDocument((d) => cantDiagnostics.delete(d.uri)),
  );
  for (const doc of vscode.workspace.textDocuments) void checkCant(doc);

  context.subscriptions.push(
    vscode.languages.registerCodeLensProvider({ scheme: "file", language: "cant" }, {
      provideCodeLenses(doc) {
        if (!vscode.workspace.getConfiguration("rite").get<boolean>("codeLens.enabled")) return [];
        const text = doc.getText();
        const caps = capabilitiesNamed(text);
        const allowAll = vscode.workspace.getConfiguration("rite").get<boolean>("codeLens.allowAll");
        return cantLenses(text).map((l) => {
          const needsGrant = l.command === "cant.runFile" && caps.length > 0 && !allowAll;
          return new vscode.CodeLens(new vscode.Range(l.line, 0, l.line, 0), {
            title: needsGrant ? `${l.title} (ungranted)` : l.title,
            command: l.command,
            tooltip: needsGrant
              ? `${l.tooltip}\n\nThis program names ${caps.map((c) => "@" + c).join(", ")}. ` +
                `Lenses run without permissions; set rite.codeLens.allowAll to change that.`
              : l.tooltip,
          });
        });
      },
    })
  );

  for (const [id, fn] of cmds) {
    context.subscriptions.push(vscode.commands.registerCommand(id, () => fn().catch((e) => vscode.window.showErrorMessage(String(e)))));
  }

  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(() => applyGlyphDecorations()),
    vscode.workspace.onDidChangeTextDocument(() => applyGlyphDecorations())
  );
  applyGlyphDecorations();
}

/** `cant sigil --format svg -o -` for one file. */
async function renderSigil(file: string): Promise<string> {
  return runCant(["sigil", file, "--format", "svg", "-o", "-"], path.dirname(file));
}

const ALIASES: [RegExp, string][] = [
  [/\bdef\b/g, "◆"],
  [/<-/g, "←"],
  [/<~/g, "↢"],
  [/->/g, "→"],
];

function applyGlyphDecorations() {
  // Clear previous
  while (glyphDecorations.length) {
    glyphDecorations.pop()?.dispose();
  }
  const enabled = vscode.workspace.getConfiguration("rite").get<boolean>("renderOnlyGlyph");
  const ed = vscode.window.activeTextEditor;
  if (!enabled || !ed || ed.document.languageId !== "rite") return;

  // Render-only: underline ASCII aliases (non-destructive hint; full glyph overlay is decoration-limited)
  const deco = vscode.window.createTextEditorDecorationType({
    after: { color: "#7ee0ff", margin: "0 0 0 2px" },
    rangeBehavior: vscode.DecorationRangeBehavior.ClosedClosed,
  });
  glyphDecorations.push(deco);
  const ranges: vscode.DecorationOptions[] = [];
  const text = ed.document.getText();
  for (const [re, glyph] of ALIASES) {
    re.lastIndex = 0;
    let m: RegExpExecArray | null;
    const r = new RegExp(re.source, re.flags);
    while ((m = r.exec(text))) {
      const start = ed.document.positionAt(m.index);
      const end = ed.document.positionAt(m.index + m[0].length);
      ranges.push({
        range: new vscode.Range(start, end),
        renderOptions: { after: { contentText: ` ${glyph}`, color: "#7ee0ff88" } },
      });
    }
  }
  ed.setDecorations(deco, ranges);
}

async function convertDialect(to: "glyph" | "ascii") {
  const ed = vscode.window.activeTextEditor;
  if (!ed || ed.document.languageId !== "rite") {
    vscode.window.showErrorMessage("Open a .rite file first");
    return;
  }
  const doc = ed.document;
  await doc.save();
  const oldPos = ed.selection.active;
  const oldText = doc.getText();
  await runRite(["convert", doc.fileName, "--to", to]);
  await vscode.commands.executeCommand("workbench.action.files.revert");
  // Best-effort cursor restore after reload
  const newEd = vscode.window.activeTextEditor;
  if (newEd) {
    const line = Math.min(oldPos.line, newEd.document.lineCount - 1);
    const col = Math.min(oldPos.character, newEd.document.lineAt(line).text.length);
    const pos = new vscode.Position(line, col);
    newEd.selection = new vscode.Selection(pos, pos);
    newEd.revealRange(new vscode.Range(pos, pos));
  }
  void oldText;
}

export async function deactivate() {
  if (client) await client.stop();
}
