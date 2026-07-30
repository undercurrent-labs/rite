//! Rite Language Server — stdio transport.

use dashmap::DashMap;
use rite_analysis::{AnalysisEngine, WorkspaceIndex};
use rite_core::SourceMap;
use rite_fmt::{convert_source, format_with_dialect, Dialect};
use rite_syntax::{keyword_or_ident, lex, Token, TokenKind};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct Backend {
    client: Client,
    docs: DashMap<Url, (i32, String)>,
    engine: Arc<Mutex<AnalysisEngine>>,
    workspace: Arc<Mutex<WorkspaceIndex>>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "rite-lsp".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    resolve_provider: Some(false),
                    trigger_characters: Some(vec![".".into(), "@".into(), "#".into(), ":".into()]),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                document_formatting_provider: Some(OneOf::Left(true)),
                // Range formatting is deliberately not advertised: the only thing
                // we could do is reformat the whole document, which is not what
                // "format selection" means. See `range_formatting` below.
                document_range_formatting_provider: None,
                rename_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: SemanticTokensLegend {
                                token_types: vec![
                                    SemanticTokenType::KEYWORD,
                                    SemanticTokenType::FUNCTION,
                                    SemanticTokenType::VARIABLE,
                                    SemanticTokenType::STRING,
                                    SemanticTokenType::NUMBER,
                                    SemanticTokenType::COMMENT,
                                    SemanticTokenType::TYPE,
                                    SemanticTokenType::NAMESPACE,
                                ],
                                token_modifiers: vec![],
                            },
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: Some(false),
                            ..Default::default()
                        },
                    ),
                ),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        "rite.convertGlyph".into(),
                        "rite.convertAscii".into(),
                        "rite.addEffectMarker".into(),
                    ],
                    ..Default::default()
                }),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "rite-lsp initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let text = params.text_document.text;
        let ver = params.text_document.version;
        self.docs.insert(uri.clone(), (ver, text.clone()));
        {
            let mut ws = self.workspace.lock().await;
            ws.upsert_document(uri.as_str(), &text);
        }
        self.publish_diagnostics(&uri, &text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let ver = params.text_document.version;
        if let Some(change) = params.content_changes.into_iter().last() {
            self.docs.insert(uri.clone(), (ver, change.text.clone()));
            {
                let mut ws = self.workspace.lock().await;
                ws.upsert_document(uri.as_str(), &change.text);
            }
            self.publish_diagnostics(&uri, &change.text).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.docs.remove(&params.text_document.uri);
        {
            let mut ws = self.workspace.lock().await;
            ws.remove_document(params.text_document.uri.as_str());
        }
        self.client
            .publish_diagnostics(params.text_document.uri, vec![], None)
            .await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let text = self.docs.get(&uri).map(|e| e.1.clone()).unwrap_or_default();
        let engine = self.engine.lock().await;
        let items = engine.completions(&text, pos.line + 1, pos.character);
        let lsp_items: Vec<CompletionItem> = items
            .into_iter()
            .map(|c| CompletionItem {
                label: c.label,
                kind: Some(match c.kind.as_str() {
                    "function" => CompletionItemKind::FUNCTION,
                    "keyword" => CompletionItemKind::KEYWORD,
                    "capability" => CompletionItemKind::MODULE,
                    "constant" => CompletionItemKind::CONSTANT,
                    _ => CompletionItemKind::TEXT,
                }),
                detail: Some(c.detail),
                insert_text: Some(c.insert_text),
                ..Default::default()
            })
            .collect();
        Ok(Some(CompletionResponse::Array(lsp_items)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let text = self.docs.get(&uri).map(|e| e.1.clone()).unwrap_or_default();
        let engine = self.engine.lock().await;
        Ok(engine
            .hover(&text, pos.line + 1, pos.character)
            .map(|h| Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: h.markdown,
                }),
                range: None,
            }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let text = self.docs.get(&uri).map(|e| e.1.clone()).unwrap_or_default();
        let engine = self.engine.lock().await;
        let name = match engine.hover(&text, pos.line + 1, pos.character) {
            Some(h) => h.title,
            None => return Ok(None),
        };
        drop(engine);
        let ws = self.workspace.lock().await;
        if let Some(sym) = ws.find_definition(&name) {
            let def_uri = Url::parse(&sym.uri).unwrap_or(uri);
            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                uri: def_uri,
                range: Range {
                    start: Position {
                        line: sym.line.saturating_sub(1),
                        character: sym.character,
                    },
                    end: Position {
                        line: sym.line.saturating_sub(1),
                        character: sym.character + sym.name.len() as u32,
                    },
                },
            })));
        }
        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let text = self.docs.get(&uri).map(|e| e.1.clone()).unwrap_or_default();
        let engine = self.engine.lock().await;
        let name = match engine.hover(&text, pos.line + 1, pos.character) {
            Some(h) => h.title,
            None => return Ok(Some(vec![])),
        };
        drop(engine);
        let ws = self.workspace.lock().await;
        let locs: Vec<Location> = ws
            .find_references(&name)
            .into_iter()
            .filter_map(|r| {
                let u = Url::parse(&r.uri).ok()?;
                Some(Location {
                    uri: u,
                    range: Range {
                        start: Position {
                            line: r.line.saturating_sub(1),
                            character: r.character,
                        },
                        end: Position {
                            line: r.line.saturating_sub(1),
                            character: r.end_character,
                        },
                    },
                })
            })
            .collect();
        Ok(Some(locs))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        let ws = self.workspace.lock().await;
        let symbols = ws.workspace_symbols(&params.query);
        let out: Vec<SymbolInformation> = symbols
            .into_iter()
            .filter_map(|s| {
                let uri = Url::parse(&s.uri).ok()?;
                #[allow(deprecated)]
                Some(SymbolInformation {
                    name: s.name.clone(),
                    kind: SymbolKind::FUNCTION,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri,
                        range: Range {
                            start: Position {
                                line: s.line.saturating_sub(1),
                                character: s.character,
                            },
                            end: Position {
                                line: s.line.saturating_sub(1),
                                character: s.character + s.name.len() as u32,
                            },
                        },
                    },
                    container_name: Some(s.module),
                })
            })
            .collect();
        Ok(Some(out))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let text = self.docs.get(&uri).map(|e| e.1.clone()).unwrap_or_default();
        let mut engine = self.engine.lock().await;
        let snap = engine.analyze(uri.as_str(), &text);
        let symbols: Vec<SymbolInformation> = snap
            .symbols
            .into_iter()
            .map(|s| {
                #[allow(deprecated)]
                SymbolInformation {
                    name: s.name,
                    kind: SymbolKind::FUNCTION,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri: uri.clone(),
                        range: Range {
                            start: Position {
                                line: s.line.saturating_sub(1),
                                character: s.character,
                            },
                            end: Position {
                                line: s.line.saturating_sub(1),
                                character: s.character + 1,
                            },
                        },
                    },
                    container_name: None,
                }
            })
            .collect();
        Ok(Some(DocumentSymbolResponse::Flat(symbols)))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = params.text_document.uri;
        let text = self.docs.get(&uri).map(|e| e.1.clone()).unwrap_or_default();
        if text.is_empty() {
            return Ok(None);
        }
        // `Preserve` keeps the dialect the file is written in — formatting on save
        // must not force an ASCII-dialect file into glyphs. It also leaves files
        // with parse errors (and anything the formatter refuses) untouched.
        match format_with_dialect(&text, Dialect::Preserve) {
            Ok(r) if r.text != text => Ok(Some(vec![TextEdit {
                range: full_range(&text),
                new_text: r.text,
            }])),
            _ => Ok(None),
        }
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        // The formatter only works on whole programs, so a range request could
        // only be served by rewriting the entire document — a nasty surprise for
        // "format selection". Do nothing instead (and see `initialize`, which no
        // longer advertises the capability).
        let _ = params;
        Ok(None)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let mut actions = Vec::new();
        let uri = params.text_document.uri;
        let text = self.docs.get(&uri).map(|e| e.1.clone()).unwrap_or_default();
        if let Ok(r) = convert_source(&text, Dialect::Glyph) {
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Convert to glyph syntax".into(),
                kind: Some(CodeActionKind::REFACTOR),
                edit: Some(WorkspaceEdit {
                    changes: Some(
                        [(
                            uri.clone(),
                            vec![TextEdit {
                                range: full_range(&text),
                                new_text: r.text,
                            }],
                        )]
                        .into_iter()
                        .collect(),
                    ),
                    ..Default::default()
                }),
                ..Default::default()
            }));
        }
        if let Ok(r) = convert_source(&text, Dialect::Ascii) {
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: "Convert to ASCII syntax".into(),
                kind: Some(CodeActionKind::REFACTOR),
                edit: Some(WorkspaceEdit {
                    changes: Some(
                        [(
                            uri.clone(),
                            vec![TextEdit {
                                range: full_range(&text),
                                new_text: r.text,
                            }],
                        )]
                        .into_iter()
                        .collect(),
                    ),
                    ..Default::default()
                }),
                ..Default::default()
            }));
        }
        // Effect marker fix when diagnostic present
        for d in params.context.diagnostics {
            if d.message.contains("effect") || d.message.contains("E021") {
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: "Add effect marker `!`".into(),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![d.clone()]),
                    edit: Some(WorkspaceEdit {
                        changes: Some(
                            [(
                                uri.clone(),
                                vec![TextEdit {
                                    range: Range {
                                        start: d.range.start,
                                        end: d.range.start,
                                    },
                                    new_text: "! ".into(),
                                }],
                            )]
                            .into_iter()
                            .collect(),
                        ),
                        ..Default::default()
                    }),
                    ..Default::default()
                }));
            }
        }
        Ok(Some(actions))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let text = self.docs.get(&uri).map(|e| e.1.clone()).unwrap_or_default();
        let pos = params.text_document_position.position;
        let new_name = params.new_name;
        if !is_valid_ident(&new_name) {
            return Err(tower_lsp::jsonrpc::Error::invalid_params(
                "invalid identifier",
            ));
        }
        let Some(edits) = rename_edits(&text, pos, &new_name) else {
            return Ok(None);
        };
        if edits.is_empty() {
            return Ok(None);
        }
        Ok(Some(WorkspaceEdit {
            changes: Some([(uri, edits)].into_iter().collect()),
            ..Default::default()
        }))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        let _ = params;
        // Minimal empty tokens; clients fall back to TextMate
        Ok(Some(SemanticTokensResult::Tokens(SemanticTokens {
            result_id: None,
            data: vec![],
        })))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        let uri = params.text_document.uri;
        let text = self.docs.get(&uri).map(|e| e.1.clone()).unwrap_or_default();
        let mut ranges = Vec::new();
        let mut stack = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.contains("⟦") || line.contains("[[") || line.contains('{') {
                stack.push(i as u32);
            }
            if line.contains("⟧") || line.contains("]]") || line.contains('}') {
                if let Some(start) = stack.pop() {
                    ranges.push(FoldingRange {
                        start_line: start,
                        end_line: i as u32,
                        ..Default::default()
                    });
                }
            }
        }
        Ok(Some(ranges))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let text = self.docs.get(&uri).map(|e| e.1.clone()).unwrap_or_default();
        let mut hints = Vec::new();
        for (i, line) in text.lines().enumerate() {
            if line.contains('!') || line.contains(" do ") {
                hints.push(InlayHint {
                    position: Position {
                        line: i as u32,
                        character: line.len() as u32,
                    },
                    label: InlayHintLabel::String(" effect".into()),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: Some(true),
                    padding_right: None,
                    data: None,
                });
            }
        }
        Ok(Some(hints))
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        Ok(Some(serde_json::json!({
            "command": params.command,
            "ok": true
        })))
    }
}

impl Backend {
    async fn publish_diagnostics(&self, uri: &Url, text: &str) {
        let mut engine = self.engine.lock().await;
        let snap = engine.analyze(uri.as_str(), text);
        let mut out = Vec::new();
        for d in snap.diagnostics {
            let severity = match d.get("severity").and_then(|s| s.as_str()) {
                Some("warning") => DiagnosticSeverity::WARNING,
                Some("note") | Some("help") => DiagnosticSeverity::INFORMATION,
                _ => DiagnosticSeverity::ERROR,
            };
            let message = d
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("error")
                .to_string();
            let code_str = d
                .get("code")
                .and_then(|c| {
                    if let Some(n) = c.as_u64() {
                        Some(format!("E{:03}", n))
                    } else if let Some(obj) = c.as_object() {
                        obj.get("0")
                            .and_then(|v| v.as_u64())
                            .map(|n| format!("E{:03}", n))
                    } else {
                        c.as_str().map(|s| s.to_string())
                    }
                })
                .unwrap_or_else(|| "E000".into());
            let range = range_from_diagnostic_json(&d, text);
            out.push(Diagnostic {
                range,
                severity: Some(severity),
                code: Some(NumberOrString::String(code_str)),
                code_description: Some(CodeDescription {
                    href: Url::parse("https://rite.dev/docs/diagnostics")
                        .unwrap_or_else(|_| Url::parse("https://example.com").unwrap()),
                }),
                source: Some("rite".into()),
                message,
                related_information: None,
                tags: None,
                data: None,
            });
        }
        self.client
            .publish_diagnostics(uri.clone(), out, None)
            .await;
    }
}

/// Map diagnostic JSON (prefer enriched start_line fields) to LSP ranges.
fn range_from_diagnostic_json(d: &Value, text: &str) -> Range {
    if let (Some(sl), Some(sc), Some(el), Some(ec)) = (
        d.get("start_line").and_then(|v| v.as_u64()),
        d.get("start_character").and_then(|v| v.as_u64()),
        d.get("end_line").and_then(|v| v.as_u64()),
        d.get("end_character").and_then(|v| v.as_u64()),
    ) {
        return Range {
            start: Position {
                line: sl.saturating_sub(1) as u32,
                character: sc as u32,
            },
            end: Position {
                line: el.saturating_sub(1) as u32,
                character: ec.max(sc + 1) as u32,
            },
        };
    }
    let labels = d.get("labels").and_then(|l| l.as_array());
    if let Some(labels) = labels {
        for lab in labels {
            if lab.get("primary").and_then(|p| p.as_bool()) == Some(false) {
                continue;
            }
            if let Some(inner) = lab.get("span") {
                if let (Some(s), Some(e)) = (
                    nested_u32(inner, &["span", "start", "0"])
                        .or_else(|| nested_u32(inner, &["span", "start"]))
                        .or_else(|| nested_u32(inner, &["start", "0"]))
                        .or_else(|| nested_u32(inner, &["start"])),
                    nested_u32(inner, &["span", "end", "0"])
                        .or_else(|| nested_u32(inner, &["span", "end"]))
                        .or_else(|| nested_u32(inner, &["end", "0"]))
                        .or_else(|| nested_u32(inner, &["end"])),
                ) {
                    return byte_range_to_lsp(text, s as usize, (e as usize).max(s as usize + 1));
                }
            }
        }
    }
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 0,
            character: 1,
        },
    }
}

fn nested_u32(v: &Value, path: &[&str]) -> Option<u32> {
    let mut cur = v;
    for p in path {
        cur = cur.get(*p)?;
    }
    cur.as_u64()
        .map(|n| n as u32)
        .or_else(|| cur.get("0").and_then(|x| x.as_u64()).map(|n| n as u32))
}

fn byte_range_to_lsp(text: &str, start: usize, end: usize) -> Range {
    let start = start.min(text.len());
    let end = end.min(text.len()).max(start);
    Range {
        start: byte_to_position(text, start),
        end: byte_to_position(text, end),
    }
}

fn byte_to_position(text: &str, byte: usize) -> Position {
    let mut line = 0u32;
    let mut col_utf16 = 0u32;
    let mut i = 0usize;
    for ch in text.chars() {
        let bl = ch.len_utf8();
        if i + bl > byte {
            break;
        }
        i += bl;
        if ch == '\n' {
            line += 1;
            col_utf16 = 0;
        } else {
            col_utf16 += ch.len_utf16() as u32;
        }
    }
    Position {
        line,
        character: col_utf16,
    }
}

/// The exact end-of-document position (utf16 columns, trailing newline included).
fn full_range(text: &str) -> Range {
    let mut line = 0u32;
    let mut character = 0u32;
    for ch in text.chars() {
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position { line, character },
    }
}

fn position_to_byte(text: &str, pos: Position) -> usize {
    let mut line = 0u32;
    let mut character = 0u32;
    for (i, ch) in text.char_indices() {
        if line == pos.line && character >= pos.character {
            return i;
        }
        if ch == '\n' {
            if line == pos.line {
                return i; // position points past the end of its line
            }
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }
    text.len()
}

fn is_valid_ident(s: &str) -> bool {
    let mut chars = s.chars();
    let shape_ok = match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => chars.all(|c| c.is_alphanumeric() || c == '_'),
        _ => false,
    };
    // A keyword would change the meaning of every site we rewrite.
    shape_ok && keyword_or_ident(s) == TokenKind::Ident
}

/// What kind of name an identifier token is. A rename never crosses classes:
/// renaming the local `id` must not touch `rec.id`, and nothing may rewrite the
/// segments of a capability path such as `@fs.read`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdentClass {
    /// Plain name: binding, parameter, function, import alias, record key.
    Binding,
    /// Name after a `.`: member access or module path segment.
    Field,
    /// Segment of an `@…` / `host.…` capability path.
    Capability,
}

fn lex_document(text: &str) -> Vec<Token> {
    let mut sources = SourceMap::new();
    let id = sources.add_file("rename.rite", text);
    match sources.get(id) {
        Some(file) => lex(file).0,
        None => Vec::new(),
    }
}

/// Classify every token; `None` for tokens that are not identifiers.
///
/// Comments, strings and keywords lex to their own token kinds, so they are
/// simply not identifiers and can never be rewritten by a rename.
fn ident_classes(tokens: &[Token]) -> Vec<Option<IdentClass>> {
    let mut out = Vec::with_capacity(tokens.len());
    let mut prev_kind: Option<TokenKind> = None;
    let mut in_capability = false;
    for t in tokens {
        if t.kind.is_trivia() {
            out.push(None);
            continue;
        }
        let class = match t.kind {
            TokenKind::Ident if in_capability => Some(IdentClass::Capability),
            TokenKind::Ident if prev_kind == Some(TokenKind::Dot) => Some(IdentClass::Field),
            TokenKind::Ident => Some(IdentClass::Binding),
            _ => None,
        };
        match t.kind {
            // `@` / `host.` opens a capability path; `.` and its idents continue it.
            TokenKind::Host => in_capability = true,
            TokenKind::Dot | TokenKind::Ident => {}
            _ => in_capability = false,
        }
        prev_kind = Some(t.kind);
        out.push(class);
    }
    out
}

/// Single-document, token-boundary rename.
///
/// Returns one edit per identifier *token* that names the same thing as the token
/// under the cursor, so occurrences inside strings, comments and longer
/// identifiers are impossible to hit. Scope is not modelled (a known gap): every
/// same-class occurrence of the name in this document is renamed, and references
/// in other files are left to the user.
fn rename_edits(text: &str, pos: Position, new_name: &str) -> Option<Vec<TextEdit>> {
    let tokens = lex_document(text);
    let classes = ident_classes(&tokens);
    let offset = position_to_byte(text, pos);
    let idx = tokens.iter().position(|t| {
        t.kind == TokenKind::Ident
            && t.span.start.as_usize() <= offset
            && offset <= t.span.end.as_usize()
    })?;
    let class = classes[idx]?;
    if class == IdentClass::Capability {
        // Renaming `@fs`/`@fs.read` would silently retarget a capability.
        return None;
    }
    let old = tokens[idx].text.as_str();
    if old == new_name {
        return Some(Vec::new());
    }
    Some(
        tokens
            .iter()
            .zip(&classes)
            .filter(|(t, c)| t.kind == TokenKind::Ident && t.text == old && **c == Some(class))
            .map(|(t, _)| TextEdit {
                range: byte_range_to_lsp(text, t.span.start.as_usize(), t.span.end.as_usize()),
                new_text: new_name.to_string(),
            })
            .collect(),
    )
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter("info")
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let roots = std::env::current_dir().map(|d| vec![d]).unwrap_or_default();
    let (service, socket) = LspService::new(|client| Backend {
        client,
        docs: DashMap::new(),
        engine: Arc::new(Mutex::new(AnalysisEngine::new())),
        workspace: Arc::new(Mutex::new(WorkspaceIndex::new(roots))),
    });
    Server::new(stdin, stdout, socket).serve(service).await;
    let _ = PathBuf::new();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos_of(text: &str, needle: &str) -> Position {
        let byte = text.find(needle).expect("needle in text");
        byte_to_position(text, byte)
    }

    /// Apply non-overlapping edits the way a client would.
    fn apply(text: &str, edits: &[TextEdit]) -> String {
        let mut spans: Vec<(usize, usize, &str)> = edits
            .iter()
            .map(|e| {
                (
                    position_to_byte(text, e.range.start),
                    position_to_byte(text, e.range.end),
                    e.new_text.as_str(),
                )
            })
            .collect();
        spans.sort_by_key(|(s, _, _)| *s);
        let mut out = String::new();
        let mut cursor = 0usize;
        for (start, end, new_text) in spans {
            assert!(start >= cursor, "overlapping edits");
            out.push_str(&text[cursor..start]);
            out.push_str(new_text);
            cursor = end;
        }
        out.push_str(&text[cursor..]);
        out
    }

    fn rename(text: &str, at: &str, new_name: &str) -> String {
        let edits = rename_edits(text, pos_of(text, at), new_name).expect("renameable");
        apply(text, &edits)
    }

    #[test]
    fn rename_does_not_touch_substrings_strings_or_comments() {
        let src =
            "x ← 1\nmax ← x + 1\ns ← \"x marks x\"\n// x in a comment\n! @console.println(x)\n";
        let out = rename(src, "x ← 1", "y");
        assert_eq!(
            out,
            "y ← 1\nmax ← y + 1\ns ← \"x marks x\"\n// x in a comment\n! @console.println(y)\n"
        );
    }

    #[test]
    fn rename_from_a_use_site_renames_the_binding_too() {
        let src = "count ← 1\n! @console.println(count)\n";
        let out = rename(src, "count)", "total");
        assert_eq!(out, "total ← 1\n! @console.println(total)\n");
    }

    #[test]
    fn rename_leaves_capability_paths_alone() {
        let src = "db ← 1\nconn ← @db.open()\n";
        let out = rename(src, "db ←", "handle");
        assert_eq!(out, "handle ← 1\nconn ← @db.open()\n");
    }

    #[test]
    fn cursor_inside_a_capability_path_is_not_renameable() {
        let src = "conn ← @db.open()\n";
        assert!(rename_edits(src, pos_of(src, "db.open"), "x").is_none());
        assert!(rename_edits(src, pos_of(src, "open()"), "x").is_none());
    }

    #[test]
    fn rename_separates_locals_from_fields() {
        let src = "id ← 1\nrec ← ⟨id: id⟩\nv ← rec.id\n";
        // `⟨id: …⟩` is a record key (a plain name), `rec.id` is a field.
        let out = rename(src, "id ← 1", "key");
        assert_eq!(out, "key ← 1\nrec ← ⟨key: key⟩\nv ← rec.id\n");
        let out = rename(src, "id\n", "field");
        assert_eq!(out, "id ← 1\nrec ← ⟨id: id⟩\nv ← rec.field\n");
    }

    #[test]
    fn rename_handles_glyph_columns() {
        // `←` and `⟨⟩` are multi-byte: edit ranges must be utf16 columns.
        let src = "α ← 1\nβ ← α + α\n";
        let out = rename(src, "α ←", "gamma");
        assert_eq!(out, "gamma ← 1\nβ ← gamma + gamma\n");
    }

    #[test]
    fn rename_on_a_keyword_or_literal_is_declined() {
        let src = "x ← 1\n";
        assert!(rename_edits(src, pos_of(src, "←"), "y").is_none());
        assert!(rename_edits(src, pos_of(src, "1"), "y").is_none());
    }

    #[test]
    fn new_name_must_be_an_identifier() {
        assert!(is_valid_ident("total"));
        assert!(is_valid_ident("_x1"));
        assert!(!is_valid_ident("1x"));
        assert!(!is_valid_ident("has space"));
        // Rite keywords would change the meaning of every rewritten site.
        assert!(!is_valid_ident("def"));
        assert!(!is_valid_ident("match"));
    }

    #[test]
    fn full_range_covers_the_trailing_newline() {
        let r = full_range("x ← 1\n");
        assert_eq!(
            r.end,
            Position {
                line: 1,
                character: 0
            }
        );
        // utf16 columns, not bytes: `⟧` is three bytes and one unit.
        let r = full_range("◆ f() ⟦ ⟧");
        assert_eq!(
            r.end,
            Position {
                line: 0,
                character: 9
            }
        );
    }

    /// What "format on save" does: keep the dialect, keep the comments.
    #[test]
    fn formatting_preserves_dialect_and_comments() {
        let ascii = "// keep me\ndef f(n) [[\n  return n * 2 // and me\n]]\n";
        let out = format_with_dialect(ascii, Dialect::Preserve).unwrap().text;
        assert!(out.contains("def f(n) [["), "forced glyphs: {out}");
        assert!(!out.contains('◆'), "forced glyphs: {out}");
        assert!(out.contains("// keep me"), "{out}");
        assert!(out.contains("// and me"), "{out}");

        let glyph = "// keep me\n◆ f(n) ⟦\n  ^ n * 2\n⟧\n";
        let out = format_with_dialect(glyph, Dialect::Preserve).unwrap().text;
        assert!(out.contains("◆ f(n) ⟦"), "lost glyphs: {out}");
        assert!(out.contains("// keep me"), "{out}");
    }

    /// A document the parser rejects must never be rewritten on save.
    #[test]
    fn formatting_declines_broken_documents() {
        let broken = "def f( [[\n";
        let r = format_with_dialect(broken, Dialect::Preserve).unwrap();
        assert_eq!(r.text, broken);
    }
}
