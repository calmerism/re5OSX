// The Studio's code editor: CodeMirror 6 with Luau highlighting and the
// game's type checker as a language server (over the /lsp WebSocket). One
// view; each open file keeps its own state (text, undo history, cursor).
// Used from Rust (src/editor.rs).

import { EditorState, Compartment, Prec } from "@codemirror/state";
import {
  EditorView, keymap, lineNumbers, highlightActiveLine, highlightActiveLineGutter,
  drawSelection, rectangularSelection, crosshairCursor, highlightSpecialChars, dropCursor,
  placeholder, tooltips,
} from "@codemirror/view";
import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands";
import {
  StreamLanguage, HighlightStyle, syntaxHighlighting, bracketMatching, indentOnInput, indentUnit,
} from "@codemirror/language";
import { autocompletion, closeBrackets, closeBracketsKeymap, completionKeymap, completionStatus } from "@codemirror/autocomplete";
import { lintGutter, lintKeymap, forEachDiagnostic } from "@codemirror/lint";
import { searchKeymap, highlightSelectionMatches, search } from "@codemirror/search";
import { LSPClient, languageServerExtensions } from "@codemirror/lsp-client";
import { tags as t } from "@lezer/highlight";

// ---- Luau ---------------------------------------------------------------

// What opens a block and what closes one (elseif closes the branch its
// `then` opens again; else leaves the depth as it was).
const OPENS = new Set(["function", "do", "then", "repeat"]);
const CLOSES = new Set(["end", "until", "elseif"]);
const KEYWORDS = new Set(("and break do else elseif end false for function if in local nil not or repeat " +
  "return then true until while continue").split(" "));
const CONTEXTUAL = new Set(["type", "export", "typeof"]);
const BUILTIN_TYPES = new Set(("number string boolean any unknown never nil vector thread buffer " +
  "Entity Ped Vehicle Object Player Cam Blip Pickup Interior ScrHandle FireId Hash").split(" "));
const GLOBALS = new Set(("print wait joaat require mod math string table coroutine bit32 utf8 os vector " +
  "pairs ipairs next select type typeof tostring tonumber assert error pcall xpcall setmetatable getmetatable " +
  "rawget rawset rawequal rawlen unpack").split(" "));

function longBracket(stream) {
  const m = stream.match(/^\[(=*)\[/);
  return m ? m[1].length : -1;
}

function readLong(level, style) {
  return (stream, state) => {
    const close = "]" + "=".repeat(level) + "]";
    while (!stream.eol()) {
      if (stream.match(close)) { state.tokenize = null; return style; }
      stream.next();
    }
    return style;
  };
}

function readQuoted(quote) {
  return (stream, state) => {
    let escaped = false, ch;
    while ((ch = stream.next()) != null) {
      if (ch === quote && !escaped) { state.tokenize = null; return "string"; }
      escaped = !escaped && ch === "\\";
    }
    if (!escaped && quote !== "`") state.tokenize = null;
    return "string";
  };
}

const luau = StreamLanguage.define({
  name: "luau",
  startState: () => ({ tokenize: null, typeContext: false, afterDot: false, depth: 0, ifExpr: false, levels: [] }),
  token(stream, state) {
    const style = luauToken(stream, state);
    if (stream.eol()) endLine(state);
    return style;
  },
  // A line is indented by the levels open before it: a line that leaves
  // blocks or brackets open adds one, however many (`f(x, function()`),
  // and they close together (`end)`). One that starts by closing (end,
  // until, a bracket) or with else / elseif sits a level out.
  indent(state, textAfter, cx) {
    let depth = state.depth, rest = textAfter, out = 0, m;
    while ((m = /^\s*(\bend\b|\buntil\b|\belseif\b|\belse\b|[)}\]])/.exec(rest))) {
      if (m[1] === "else") { out = 1; break; }
      depth--;
      rest = rest.slice(m[0].length);
    }
    return Math.max(0, state.levels.filter(level => level <= depth).length - out) * cx.unit;
  },
  languageData: {
    commentTokens: { line: "--", block: { open: "--[[", close: "]]" } },
    closeBrackets: { brackets: ["(", "[", "{", '"', "'", "`"] },
    indentOnInput: /^\s*(end|else|elseif|until|[)}\]])$/,
  },
  tokenTable: { namespace: t.namespace, typeName: t.typeName, builtin: t.standard(t.variableName), function: t.function(t.variableName), property: t.propertyName },
});

// The levels at a line's end: those closed go, and one more when the line
// left more open than before.
function endLine(state) {
  while (state.levels.length && state.levels[state.levels.length - 1] > state.depth) state.levels.pop();
  if (state.depth > (state.levels.length ? state.levels[state.levels.length - 1] : 0)) state.levels.push(state.depth);
}

function luauToken(stream, state) {
  // An if-expression (`x = if a then b else c`) stays on its line.
  if (stream.sol()) state.ifExpr = false;
  if (state.tokenize) return state.tokenize(stream, state);
  if (stream.eatSpace()) return null;
  if (stream.match("--")) {
    const level = longBracket(stream);
    if (level >= 0) { state.tokenize = readLong(level, "comment"); return state.tokenize(stream, state); }
    stream.skipToEnd();
    return "comment";
  }
  const level = longBracket(stream);
  if (level >= 0) { state.tokenize = readLong(level, "string"); return state.tokenize(stream, state); }
  const ch = stream.peek();
  if (ch === '"' || ch === "'" || ch === "`") {
    stream.next();
    state.tokenize = readQuoted(ch);
    return state.tokenize(stream, state);
  }
  if (stream.match(/^0x[\da-f_]+/i) || stream.match(/^0b[01_]+/i) || stream.match(/^\d[\d_]*(\.[\d_]*)?(e[+-]?\d+)?/i)) {
    state.afterDot = false;
    return "number";
  }
  if (stream.match(/^[A-Za-z_][A-Za-z0-9_]*/)) {
    const word = stream.current();
    const afterDot = state.afterDot;
    state.afterDot = false;
    if (KEYWORDS.has(word)) {
      state.typeContext = false;
      if (word === "if" && !afterDot) {
        // In an expression (after `=`, `(`, `,`, `return`, an operator...),
        // it is an if-expression: its `then` and `else` open nothing.
        state.ifExpr = /([=(,{[+\-*/%^<>~]|\.\.|\breturn|\band|\bor|\bnot)\s*$/.test(stream.string.slice(0, stream.start));
      } else if (!afterDot && !(state.ifExpr && (word === "then" || word === "elseif"))) {
        if (OPENS.has(word)) state.depth++;
        else if (CLOSES.has(word)) state.depth = Math.max(0, state.depth - 1);
      }
      return word === "true" || word === "false" || word === "nil" ? "atom" : "keyword";
    }
    if (CONTEXTUAL.has(word) && !afterDot && /^\s+[A-Za-z_]/.test(stream.string.slice(stream.pos))) return "keyword";
    if (state.typeContext || BUILTIN_TYPES.has(word) && /[:>|?]\s*$/.test(stream.string.slice(0, stream.start))) {
      return "typeName";
    }
    if (afterDot) return /^\s*\(/.test(stream.string.slice(stream.pos)) ? "function" : "property";
    if (stream.peek() === ".") return "namespace";
    if (/^\s*\(/.test(stream.string.slice(stream.pos))) return GLOBALS.has(word) ? "builtin" : "function";
    return GLOBALS.has(word) ? "builtin" : "variable";
  }
  if (stream.match("->")) { state.typeContext = true; return "operator"; }
  const op = stream.next();
  if (op === ".") { state.afterDot = true; return "punctuation"; }
  if (op === ":") {
    // `local x: Type`, `(a: T)`: a type follows (method calls `a:b()` do not).
    state.typeContext = /^\s*[A-Za-z_][A-Za-z0-9_]*\s*[,)=?|]/.test(stream.string.slice(stream.pos)) ||
      /^\s*[A-Za-z_][A-Za-z0-9_]*\s*$/.test(stream.string.slice(stream.pos));
    if (!state.typeContext) state.afterDot = true;
    return "punctuation";
  }
  if (/[,=)(\]{}]/.test(op)) state.typeContext = false;
  if (/[({[]/.test(op)) state.depth++;
  else if (/[)}\]]/.test(op)) state.depth = Math.max(0, state.depth - 1);
  return /[()[\]{},;]/.test(op) ? "punctuation" : "operator";
}

// ---- JSON (a mod's mod.json) --------------------------------------------

const json = StreamLanguage.define({
  name: "json",
  token(stream) {
    if (stream.eatSpace()) return null;
    if (stream.match(/^"(?:[^"\\]|\\.)*"?/)) return /^\s*:/.test(stream.string.slice(stream.pos)) ? "property" : "string";
    if (stream.match(/^-?\d+(\.\d+)?([eE][+-]?\d+)?/)) return "number";
    if (stream.match(/^(true|false|null)\b/)) return "atom";
    stream.next();
    return "punctuation";
  },
  tokenTable: { property: t.propertyName },
});

// The app's palette: its status and overlay colours.
const highlight = HighlightStyle.define([
  { tag: t.keyword, color: "#f0b464" },
  { tag: [t.string], color: "#97c459" },
  { tag: [t.number, t.atom, t.bool], color: "#85b7eb" },
  { tag: t.typeName, color: "#5dcaa5" },
  { tag: t.namespace, color: "#b9bcc2" },
  { tag: [t.function(t.variableName), t.propertyName], color: "#ffffff" },
  { tag: t.standard(t.variableName), color: "#e3c9a0" },
  { tag: t.comment, color: "#6b6f76", fontStyle: "italic" },
  { tag: [t.operator, t.punctuation], color: "#9da0a6" },
  { tag: t.variableName, color: "#d5d7db" },
]);

const theme = EditorView.theme({
  "&": { color: "#d5d7db", backgroundColor: "transparent", height: "100%", fontSize: "var(--code-size)" },
  ".cm-content": { fontFamily: "var(--mono)", caretColor: "#fff", padding: "8px 0" },
  ".cm-scroller": { fontFamily: "var(--mono)", lineHeight: "1.65" },
  ".cm-gutters": { backgroundColor: "transparent", color: "#4a4d53", border: "none" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "#d5d7db" },
  ".cm-activeLine": { backgroundColor: "rgba(255,255,255,0.035)" },
  "&.cm-focused .cm-cursor": { borderLeftColor: "#fff", borderLeftWidth: "2px" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": { backgroundColor: "rgba(255,255,255,0.14) !important" },
  ".cm-selectionMatch": { backgroundColor: "rgba(255,255,255,0.07)" },
  ".cm-matchingBracket": { backgroundColor: "rgba(255,255,255,0.12)", outline: "none" },
  // No overflow clipping here: the completion's notes open beside the list.
  ".cm-tooltip": { backgroundColor: "#151619", border: "1px solid #4c4c4c", borderRadius: "10px", boxShadow: "0 14px 40px rgba(0,0,0,.5)" },
  // As specific as CodeMirror's own, or its widths win.
  ".cm-tooltip.cm-tooltip-autocomplete > ul": { fontFamily: "var(--mono)", maxHeight: "16em", borderRadius: "9px", maxWidth: "min(520px, 92vw)" },
  // A long signature is cut here; the details show it whole.
  ".cm-tooltip-autocomplete .cm-completionLabel": { flex: "none" },
  ".cm-tooltip-autocomplete .cm-completionDetail": { flex: "1 1 auto", minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" },
  ".cm-tooltip.cm-completionInfo.cm-completionInfo-under": { left: "0" },
  ".cm-tooltip.cm-completionInfo.cm-completionInfo-right": { marginLeft: "4px" },
  ".cm-tooltip.cm-completionInfo.cm-completionInfo-left": { marginRight: "4px" },
  ".cm-tooltip-autocomplete > ul > li": { display: "flex", alignItems: "baseline", padding: "4px 10px !important", lineHeight: "1.5" },
  ".cm-tooltip-autocomplete > ul > li[aria-selected]": { backgroundColor: "rgba(255,255,255,0.10)", color: "#fff" },
  ".cm-completionDetail": { color: "#6b6f76", fontStyle: "normal", marginLeft: "1em" },
  ".cm-tooltip.cm-completionInfo": {
    padding: "10px 12px", maxWidth: "420px", overflowY: "auto", boxSizing: "border-box", fontFamily: "var(--ui)", color: "#9da0a6",
    backgroundColor: "#151619", border: "1px solid #4c4c4c", borderRadius: "10px", boxShadow: "0 14px 40px rgba(0,0,0,.5)",
  },
  ".cm-completionMatchedText": { textDecoration: "none", color: "#fff", fontWeight: "700" },
  ".cm-lsp-documentation, .cm-tooltip-hover": { padding: "10px 12px", fontFamily: "var(--ui)", color: "#d5d7db", maxWidth: "520px" },
  ".cm-lsp-documentation pre, .cm-tooltip-hover pre": { fontFamily: "var(--mono)", whiteSpace: "pre-wrap", margin: "0 0 8px" },
  ".cm-tooltip a": { color: "#fff", textDecorationColor: "rgba(255,255,255,.35)", textUnderlineOffset: "3px", overflowWrap: "anywhere" },
  ".cm-tooltip code": { fontFamily: "var(--mono)", fontSize: "12px", color: "#d5d7db" },
  ".cm-signature-tooltip, .cm-lsp-signature-tooltip": { padding: "8px 12px", fontFamily: "var(--mono)", fontSize: "12.5px", maxWidth: "620px" },
  ".cm-lsp-active-parameter": { color: "#fff", fontWeight: "700", textDecoration: "underline", textUnderlineOffset: "3px" },
  ".cm-diagnostic": { fontFamily: "var(--ui)", padding: "8px 12px" },
  ".cm-diagnostic-error": { borderLeft: "3px solid #f09595" },
  ".cm-diagnostic-warning": { borderLeft: "3px solid #f0b464" },
  ".cm-lintRange-error": { backgroundImage: "none", textDecoration: "underline wavy #f09595", textUnderlineOffset: "4px" },
  ".cm-lintRange-warning": { backgroundImage: "none", textDecoration: "underline wavy #f0b464", textUnderlineOffset: "4px" },
  ".cm-lint-marker-error": { content: "none" },
  // The search bar (Ctrl/Cmd+F), in the Studio's look.
  ".cm-panels": { backgroundColor: "rgba(20,21,24,0.96)", color: "#d5d7db", fontFamily: "var(--ui)" },
  ".cm-panels-top": { borderBottom: "1px solid rgba(255,255,255,0.08)" },
  ".cm-panel.cm-search": { display: "flex", flexWrap: "wrap", alignItems: "center", gap: "6px", padding: "8px 40px 8px 12px", fontSize: "12.5px" },
  ".cm-panel.cm-search br": { flexBasis: "100%", height: 0, content: "''" },
  ".cm-panel.cm-search input.cm-textfield": {
    font: "400 12.5px var(--mono)", color: "#fff", backgroundColor: "rgba(36,36,36,0.9)", border: "1px solid rgba(255,255,255,0.08)",
    borderRadius: "7px", padding: "5px 8px", margin: 0, minWidth: "180px", outline: "none",
  },
  ".cm-panel.cm-search input.cm-textfield:focus": { borderColor: "rgba(255,255,255,0.3)" },
  ".cm-panel.cm-search button.cm-button": {
    font: "600 12px var(--ui)", color: "#d5d7db", backgroundImage: "none", backgroundColor: "rgba(255,255,255,0.06)",
    border: "1px solid rgba(255,255,255,0.08)", borderRadius: "7px", padding: "5px 10px", margin: 0, textTransform: "none",
  },
  ".cm-panel.cm-search button.cm-button:hover": { backgroundColor: "rgba(255,255,255,0.11)" },
  ".cm-panel.cm-search label": { display: "inline-flex", alignItems: "center", gap: "5px", color: "#9da0a6", margin: "0 4px", whiteSpace: "nowrap" },
  ".cm-panel.cm-search input[type=checkbox]": { accentColor: "#fff", margin: 0 },
  ".cm-panel.cm-search button[name=close]": { top: "8px", right: "10px", fontSize: "18px", color: "#9da0a6", cursor: "pointer" },
  ".cm-searchMatch": { backgroundColor: "rgba(240,180,100,0.25)" },
  ".cm-searchMatch-selected": { backgroundColor: "rgba(240,180,100,0.5)" },
}, { dark: true });

// The console's input: one line (more with Shift-Enter), no gutters, its
// problems not shown (an expression is not a statement).
const replTheme = EditorView.theme({
  "&": { height: "auto", fontSize: "12.5px" },
  ".cm-content": { padding: "9px 0" },
  ".cm-scroller": { lineHeight: "1.5" },
  ".cm-activeLine": { backgroundColor: "transparent" },
  "& .cm-lintRange-error, & .cm-lintRange-warning": { backgroundImage: "none", textDecoration: "none" },
  ".cm-placeholder": { color: "#6b6f76" },
});

// The search bar's words, as the Studio writes them.
const phrases = EditorState.phrases.of({
  "Find": "Find", "Replace": "Replace", "next": "Next", "previous": "Previous", "all": "All",
  "match case": "Match case", "regexp": "Regex", "by word": "Whole word", "replace": "Replace",
  "replace all": "Replace all", "close": "Close",
});

// The completion's details beside the list, as VS Code has them: from the
// list's top, as tall as it, scrolling on their own; never over the list
// (CodeMirror puts them under the chosen option, over the rest, when the
// side has no room). Without room on either side, under the whole list.
function infoBeside(view, list, option, info, space, tooltip) {
  const scaleY = (list.bottom - list.top) / tooltip.offsetHeight || 1;
  const scaleX = (list.right - list.left) / tooltip.offsetWidth || 1;
  const spaceLeft = list.left - space.left, spaceRight = space.right - list.right;
  const right = spaceRight >= Math.min(420, spaceLeft);
  const width = Math.min(420, (right ? spaceRight : spaceLeft) - 6);
  const height = Math.max(list.bottom - list.top, 180);
  if (width >= 240) {
    return {
      style: `top: 0; width: ${width / scaleX}px; max-height: ${height / scaleY}px`,
      class: "cm-completionInfo-" + (right ? "right" : "left"),
    };
  }
  const below = space.bottom - list.bottom >= 140 || space.bottom - list.bottom > list.top - space.top;
  const room = below ? space.bottom - list.bottom : list.top - space.top;
  return {
    style: `${below ? "top" : "bottom"}: ${(list.bottom - list.top) / scaleY + 4}px; width: ${Math.min(420, space.right - list.left - 8) / scaleX}px; max-height: ${Math.max(room - 8, 100) / scaleY}px`,
    class: "cm-completionInfo-under",
  };
}

const completion = autocompletion({ positionInfo: infoBeside });

// ---- The language server ------------------------------------------------

function webSocketTransport(url, onState) {
  let handlers = [];
  let socket = null;
  let queue = [];
  const transport = {
    send(message) { if (socket && socket.readyState === 1) socket.send(message); else queue.push(message); },
    subscribe(handler) { handlers.push(handler); },
    unsubscribe(handler) { handlers = handlers.filter(h => h !== handler); },
  };
  return new Promise(resolve => {
    let resolved = false;
    const open = () => {
      // No address while the page waits for a new code: an old one would
      // count as a wrong guess, and the server stops taking any after ten.
      const address = url();
      if (!address) { setTimeout(open, 1000); return; }
      socket = new WebSocket(address);
      socket.onmessage = e => { for (const h of handlers) h(String(e.data)); };
      socket.onopen = () => {
        onState(true);
        for (const m of queue) socket.send(m);
        queue = [];
        if (!resolved) { resolved = true; resolve(transport); }
      };
      socket.onclose = () => { onState(false); setTimeout(open, 2000); };
    };
    open();
  });
}

// ---- The editor ----------------------------------------------------------

export class StudioEditor {
  constructor(parent, lspUrl) {
    this.files = new Map(); // path -> { state, saved, readOnly }
    this.current = null;
    this.changeHandler = () => {};
    this.saveHandler = () => {};
    this.diagnosticsHandler = () => {};
    this.cursorHandler = () => {};
    this.client = new LSPClient({ extensions: languageServerExtensions(), timeout: 5000 });
    this.readOnly = new Compartment();
    this.lspUrl = lspUrl;
    // A new server (the game started again) knows nothing: connecting again
    // initialises it and opens the files anew.
    webSocketTransport(() => this.lspUrl, up => {
      this.lspUp = up;
      if (!this.transport) return;
      if (up) this.client.connect(this.transport); else this.client.disconnect();
    }).then(transport => { this.transport = transport; this.client.connect(transport); });
    this.view = new EditorView({ parent, state: EditorState.create({ doc: "" }) });
  }

  // The code is no longer good (the game started again): no more tries
  // until `attach` brings the new one.
  pause() {
    this.lspUrl = null;
  }

  // Into a new page (paired again): the same editor, its files and history.
  attach(parent, lspUrl) {
    this.lspUrl = lspUrl;
    parent.appendChild(this.view.dom);
    this.view.requestMeasure();
  }

  extensions(path, readOnly) {
    const isJson = path.endsWith(".json");
    return [
      lineNumbers(), highlightActiveLineGutter(), highlightSpecialChars(), history(), drawSelection(),
      dropCursor(), EditorState.allowMultipleSelections.of(true), indentOnInput(), bracketMatching(),
      closeBrackets(), rectangularSelection(), crosshairCursor(), highlightActiveLine(),
      highlightSelectionMatches(), indentUnit.of("    "), EditorState.tabSize.of(4), lintGutter(),
      // Over the page: the panes' edges would clip them.
      tooltips({ parent: document.body }),
      completion,
      isJson ? json : luau, syntaxHighlighting(highlight), theme, search({ top: true }), phrases,
      this.readOnly.of([EditorState.readOnly.of(readOnly), EditorView.editable.of(!readOnly)]),
      keymap.of([
        // Save: a file's, not the read-only definitions'.
        { key: "Mod-s", preventDefault: true, run: () => { if (!readOnly) this.saveHandler(path); return true; } },
        ...closeBracketsKeymap, ...defaultKeymap, ...searchKeymap, ...historyKeymap, ...completionKeymap,
        ...lintKeymap, indentWithTab,
      ]),
      EditorView.updateListener.of(update => {
        if (update.docChanged) this.changeHandler(path, this.isDirty(path, update.state));
        if (update.selectionSet || update.docChanged) {
          const head = update.state.selection.main.head, line = update.state.doc.lineAt(head);
          this.cursorHandler(line.number, head - line.from + 1);
        }
        this.reportDiagnostics(path, update.state);
      }),
      readOnly ? [] : this.client.plugin("file:///mods/" + path, isJson ? "json" : "luau"),
    ];
  }

  // The console's input: Luau with highlighting and completions (the same
  // language server, a document of its own), Enter runs it, Up and Down
  // go through what ran. Made once; moved into `parent` when the console
  // shows again.
  repl(parent, onRun) {
    if (this.replView) {
      parent.appendChild(this.replView.dom);
      this.replRun = onRun;
      return;
    }
    this.replRun = onRun;
    const past = [];
    let back = 0;
    const set = (view, text) => view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: text }, selection: { anchor: text.length } });
    const run = view => {
      const code = view.state.doc.toString().trim();
      if (!code) return true;
      if (past[past.length - 1] !== code) past.push(code);
      back = past.length;
      set(view, "");
      this.replRun(code);
      return true;
    };
    const walk = (view, by) => {
      if (completionStatus(view.state) === "active" || !past.length) return false;
      back = Math.max(0, Math.min(past.length, back + by));
      set(view, past[back] ?? "");
      return true;
    };
    this.replView = new EditorView({
      parent,
      state: EditorState.create({
        doc: "",
        extensions: [
          history(), drawSelection(), closeBrackets(), bracketMatching(), luau, syntaxHighlighting(highlight), theme,
          Prec.highest(replTheme),
          // A long line goes on below, not off the side (a phone's prompt is narrow).
          EditorView.lineWrapping,
          // Over the page: the panel's blur would clip them.
          tooltips({ parent: document.body }),
          completion,
          Prec.highest(keymap.of([
            ...completionKeymap,
            { key: "Enter", run },
            { key: "Shift-Enter", run: view => { view.dispatch(view.state.replaceSelection("\n")); return true; } },
            { key: "ArrowUp", run: view => view.state.doc.lines === 1 && walk(view, -1) },
            { key: "ArrowDown", run: view => view.state.doc.lines === 1 && walk(view, 1) },
          ])),
          keymap.of([...closeBracketsKeymap, ...defaultKeymap, ...historyKeymap]),
          EditorView.contentAttributes.of({ "aria-label": "Run Luau in the game", autocapitalize: "off", autocorrect: "off", spellcheck: "false" }),
          placeholder("Run Luau in the game: entity.getEntityCoords(player.playerPedId(), true)"),
          this.client.plugin("file:///mods/~console/input.luau", "luau"),
        ],
      }),
    });
  }

  focusRepl() {
    this.replView?.focus();
  }

  runRepl() {
    if (!this.replView) return;
    const view = this.replView, code = view.state.doc.toString().trim();
    if (!code) return;
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: "" } });
    this.replRun(code);
  }


  reportDiagnostics(path, state) {
    const list = [];
    forEachDiagnostic(state, (d, from) => {
      const line = state.doc.lineAt(from);
      list.push({ line: line.number, column: from - line.from + 1, severity: d.severity, message: d.message });
    });
    const key = JSON.stringify(list);
    const file = this.files.get(path);
    if (file && file.diagnostics !== key) {
      file.diagnostics = key;
      this.diagnosticsHandler(path, key);
    }
  }

  // Shows `path` (made from `text` the first time).
  open(path, text, readOnly) {
    if (this.current && this.files.has(this.current)) {
      const was = this.files.get(this.current);
      was.state = this.view.state;
      // Where it was scrolled to, for when it shows again.
      was.scroll = this.view.scrollSnapshot();
    }
    let file = this.files.get(path);
    if (!file) {
      file = { state: EditorState.create({ doc: text, extensions: this.extensions(path, readOnly) }), saved: text, diagnostics: "" };
      this.files.set(path, file);
    }
    this.current = path;
    this.view.setState(file.state);
    if (file.scroll) this.view.dispatch({ effects: file.scroll });
    const head = file.state.selection.main.head, line = file.state.doc.lineAt(head);
    this.cursorHandler(line.number, head - line.from + 1);
  }

  close(path) {
    this.files.delete(path);
    if (this.current === path) {
      this.current = null;
      this.view.setState(EditorState.create({ doc: "" }));
    }
  }

  stateOf(path) {
    return path === this.current ? this.view.state : this.files.get(path)?.state;
  }

  text(path) {
    return this.stateOf(path)?.doc.toString() ?? "";
  }

  isDirty(path, state) {
    const file = this.files.get(path);
    return !!file && (state ?? this.stateOf(path)).doc.toString() !== file.saved;
  }

  saved(path, text) {
    const file = this.files.get(path);
    if (file) file.saved = text;
    this.changeHandler(path, this.isDirty(path));
  }

  // Replaces the text (the file changed on disk): only the part that
  // differs, so the cursor and the rest stay where they were; nothing when
  // it is the text shown (a save of this page coming back).
  replace(path, text) {
    const file = this.files.get(path);
    if (!file) return;
    const state = this.stateOf(path);
    const old = state.doc.toString();
    // Typed into since the reload was asked for: that stays (it was dirty).
    if (old !== file.saved) return;
    file.saved = text;
    if (old !== text) {
      let from = 0;
      while (from < old.length && from < text.length && old[from] === text[from]) from++;
      let end = 0;
      while (end < old.length - from && end < text.length - from && old[old.length - 1 - end] === text[text.length - 1 - end]) end++;
      const tr = state.update({ changes: { from, to: old.length - end, insert: text.slice(from, text.length - end) } });
      if (path === this.current) this.view.dispatch(tr); else file.state = tr.state;
    }
    this.changeHandler(path, false);
  }

  reveal(line, column) {
    const doc = this.view.state.doc;
    const l = doc.line(Math.max(1, Math.min(line, doc.lines)));
    const pos = Math.min(l.from + Math.max(0, column - 1), l.to);
    this.view.dispatch({ selection: { anchor: pos }, effects: EditorView.scrollIntoView(pos, { y: "center" }) });
    this.view.focus();
  }

  insert(text) {
    const { from, to } = this.view.state.selection.main;
    this.view.dispatch({ changes: { from, to, insert: text }, selection: { anchor: from + text.length } });
    this.view.focus();
  }

  focus() { this.view.focus(); }
  onChange(f) { this.changeHandler = f; }
  onSave(f) { this.saveHandler = f; }
  onDiagnostics(f) { this.diagnosticsHandler = f; }
  onCursor(f) { this.cursorHandler = f; }
}
