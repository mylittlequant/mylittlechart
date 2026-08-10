#!/usr/bin/env node
// export-tokens.mjs — read the REAL mlc theme out of the Rust sources and emit
// tokens.json + tokens.css for the JS simulation lab next to this file.
//
// Direction of fit (owner rule, 2026-08-10): the DOM sim conforms to uzor and mlc,
// never the reverse. Nothing here writes back into Rust — this is a one-way export,
// and it is re-runnable, so "pixel-perfect" is a number comparison instead of an
// argument about who remembers a padding.
//
// Sources (each is a plain literal block in Rust, so parsing is deterministic):
//   mlc-core/src/theme/preset.rs            UITheme::dark() / ::light()
//   mlc-app-layout/src/ui/widgets/types.rs  WidgetTheme::default() / ::light()
//   mlc-app-layout/src/ui/widgets/modal.rs  ModalTheme::default()
//   mlc-app-layout/src/layout/modals/*.rs   per-modal `const NAME: f64 = X;` metrics
//
// Usage:  node crates/mlc-ui-lab/sim/tools/export-tokens.mjs [--out <dir>]
// Default out: the sim/ directory that owns this tool.

import { readFileSync, writeFileSync, mkdirSync, readdirSync } from 'node:fs';
import { join, dirname, basename } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const CRATES = join(HERE, '..', '..', '..');

const outFlag = process.argv.indexOf('--out');
const OUT = outFlag > -1 ? process.argv[outFlag + 1] : join(HERE, '..');

// ---- tiny Rust-literal reader ------------------------------------------------

/** Content between the first `{` at or after `from` and its matching `}`. */
function block(src, from) {
  const open = src.indexOf('{', from);
  if (open < 0) return '';
  let depth = 0;
  for (let i = open; i < src.length; i++) {
    const c = src[i];
    if (c === '{') depth++;
    else if (c === '}') {
      depth--;
      if (depth === 0) return src.slice(open + 1, i);
    }
  }
  return '';
}

/** The block of `marker` (e.g. "pub fn dark() -> Self"), or '' when absent. */
function blockAfter(src, marker) {
  const at = src.indexOf(marker);
  return at < 0 ? '' : block(src, at + marker.length);
}

/**
 * `field: <literal>` pairs at the TOP level of `blk` — nested struct blocks and
 * any non-literal value (`ChartTheme::dark().chart`) are skipped rather than
 * guessed at, so a field that silently stops being a literal shows up as a
 * missing token instead of a wrong one.
 */
function fields(blk) {
  const out = {};
  let depth = 0;
  for (const raw of blk.split('\n')) {
    const line = raw.trim();
    const opens = (line.match(/\{/g) || []).length;
    const closes = (line.match(/\}/g) || []).length;
    if (depth === 0) {
      const m = line.match(/^([a-z_][a-z0-9_]*)\s*:\s*(.+?),?$/i);
      if (m && opens === 0) {
        const key = m[1];
        const v = m[2].replace(/\.to_string\(\)$/, '').trim();
        if (/^".*"$/.test(v)) out[key] = v.slice(1, -1);
        else if (/^-?\d+(\.\d+)?$/.test(v)) out[key] = Number(v);
      }
    }
    depth += opens - closes;
    if (depth < 0) depth = 0;
  }
  return out;
}

/**
 * A constructor body is `Self { … }` — descend one level so `fields()` reads the
 * struct literal and not the function block wrapping it.
 */
function selfBlock(blk) {
  const at = blk.indexOf('Self {');
  return at < 0 ? blk : block(blk, at + 4);
}

const read = (rel) => readFileSync(join(CRATES, rel), 'utf8');

// ---- 1. UITheme (colours, fonts, sizing, effects) ----------------------------

// Slice at `impl UITheme` first: `ChartTheme` in the same file also has a
// `pub fn dark() -> Self`, and it comes EARLIER — searching the whole file
// silently parses the chart theme and yields an empty UI theme.
const presetSrc = read('mlc-core/src/theme/preset.rs');
const presetUI = presetSrc.slice(presetSrc.indexOf('impl UITheme'));

function uiTheme(fnMarker) {
  const fn = blockAfter(presetUI, fnMarker);
  const out = {};
  for (const [key, marker] of [
    ['colors', 'colors: UIColors'],
    ['fonts', 'fonts: UIFonts'],
    ['sizing', 'sizing: UISizing'],
    ['effects', 'effects: UIEffects'],
  ]) {
    out[key] = fields(blockAfter(fn, marker));
  }
  const name = fn.match(/name:\s*"([^"]+)"/);
  if (name) out.name = name[1];
  return out;
}

// ---- 2. WidgetTheme / 3. ModalTheme -----------------------------------------

const widgetSrc = read('mlc-app-layout/src/ui/widgets/types.rs');
const modalSrc = read('mlc-app-layout/src/ui/widgets/modal.rs');

const widget = {
  dark: fields(selfBlock(blockAfter(widgetSrc.slice(widgetSrc.indexOf('impl Default for WidgetTheme')), 'fn default() -> Self'))),
  light: fields(selfBlock(blockAfter(widgetSrc.slice(widgetSrc.indexOf('impl WidgetTheme')), 'pub fn light() -> Self'))),
};
const modal = {
  dark: fields(selfBlock(blockAfter(modalSrc.slice(modalSrc.indexOf('impl Default for ModalTheme')), 'fn default() -> Self'))),
};

// ---- 4. Per-modal metric constants -------------------------------------------

const CONST_RE = /const\s+([A-Z][A-Z0-9_]*)\s*:\s*f64\s*=\s*(-?\d+(?:\.\d+)?)\s*;/g;

function consts(src) {
  const found = {};
  CONST_RE.lastIndex = 0;
  let m;
  while ((m = CONST_RE.exec(src))) found[m[1]] = Number(m[2]);
  return found;
}

const modalsDir = join(CRATES, 'mlc-app-layout/src/layout/modals');
const metrics = {};
for (const file of readdirSync(modalsDir).filter((f) => f.endsWith('.rs'))) {
  const found = consts(readFileSync(join(modalsDir, file), 'utf8'));
  if (Object.keys(found).length) metrics[basename(file, '.rs')] = found;
}

// The shell's own geometry does not live in `layout/modals` — the modal chrome is
// in the shared widget, and the rails/toolbars are core layout constants. Without
// these two the sim would be hand-measured exactly where it matters most.
metrics.modal_shell = consts(read('mlc-app-layout/src/ui/widgets/modal.rs'));
metrics.layout = consts(read('mlc-core/src/types.rs'));

// ---- 5. Icons ----------------------------------------------------------------

// `mlc-core/src/state/icons.rs` holds every toolbar icon as a `pub const ICON_X:
// &str = r##"<svg …>"##`. They are plain `currentColor` SVGs on a 24-box, so the
// DOM sim can use the REAL icons instead of stand-in glyphs — no redraw, no
// second source of truth.
const iconSrc = read('mlc-core/src/state/icons.rs');
const icons = {};
const ICON_RE = /pub const ICON_([A-Z0-9_]+)\s*:\s*&str\s*=\s*r#*"([\s\S]*?)"#*\s*;/g;
let im;
while ((im = ICON_RE.exec(iconSrc))) {
  icons[im[1].toLowerCase()] = im[2].trim();
}

// ---- 6. i18n -----------------------------------------------------------------

// `mlc-core/src/i18n/tables.rs` holds every translated string as
// `static X_KEY_TABLE: [[&str; N_LANG]; N] = [ /* i VariantName */ [ "En", "Ru", … ] ];`
// The sim renders its chrome through these keys instead of carrying its own
// Russian literals — the same rule the Rust side already follows.
const i18nSrc = read('mlc-core/src/i18n/tables.rs');
const i18n = {};
const TABLE_RE = /static\s+([A-Z0-9_]+_TABLE)\s*:\s*\[\[&str;\s*N_LANG\];\s*\d+\]\s*=\s*\[([\s\S]*?)\n\];/g;
let tm;
while ((tm = TABLE_RE.exec(i18nSrc))) {
  const table = {};
  const ROW_RE = /\/\*\s*\d+\s+([A-Za-z0-9_]+)\s*\*\/\s*\[\s*"((?:[^"\\]|\\.)*)"\s*,\s*"((?:[^"\\]|\\.)*)"/g;
  let rm;
  while ((rm = ROW_RE.exec(tm[2]))) {
    table[rm[1]] = { en: rm[2].replace(/\\"/g, '"'), ru: rm[3].replace(/\\"/g, '"') };
  }
  if (Object.keys(table).length) i18n[tm[1].replace(/_TABLE$/, '')] = table;
}

// ---- emit --------------------------------------------------------------------

const tokens = {
  generated_by: 'crates/mlc-ui-lab/sim/tools/export-tokens.mjs',
  source: 'mylittlechart (Rust) — the DOM sim conforms to this, never the reverse',
  ui: { dark: uiTheme('pub fn dark() -> Self'), light: uiTheme('pub fn light() -> Self') },
  widget,
  modal,
  metrics,
};

mkdirSync(OUT, { recursive: true });
writeFileSync(join(OUT, 'tokens.json'), JSON.stringify(tokens, null, 2) + '\n');
writeFileSync(
  join(OUT, 'icons.js'),
  '/* GENERATED by sim/tools/export-tokens.mjs — the app\'s own SVGs, not redrawn. */\n' +
    'window.SIM_ICONS = ' + JSON.stringify(icons, null, 1) + ';\n'
);
writeFileSync(
  join(OUT, 'i18n.js'),
  '/* GENERATED by sim/tools/export-tokens.mjs — the app\'s own translation tables.\n' +
    '   The sim has no strings of its own: every label resolves through a key here. */\n' +
    'window.SIM_I18N = ' + JSON.stringify(i18n, null, 1) + ';\n'
);

// CSS custom properties for the dark skin — the sim's default and the only one the
// concepts were drawn against so far.
const css = [];
css.push('/* GENERATED by sim/tools/export-tokens.mjs — do not hand-edit.');
css.push('   Source of truth is the Rust theme in mylittlechart; re-run the exporter. */');
css.push(':root {');
for (const [section, vals] of Object.entries(tokens.ui.dark)) {
  if (typeof vals !== 'object') continue;
  for (const [k, v] of Object.entries(vals)) {
    // A length gets `px` wherever it lives — `button_rounding` sits in `colors`
    // and is still a radius. Weights, scales and durations stay unitless.
    const isLength = /size|width|height|padding|radius|rounding|gap|offset/.test(k) && !/weight|scale/.test(k);
    const unit = typeof v === 'number' && isLength ? 'px' : '';
    css.push(`  --mlc-${section}-${k.replace(/_/g, '-')}: ${v}${typeof v === 'number' ? unit : ''};`);
  }
}
for (const [k, v] of Object.entries(modal.dark)) css.push(`  --mlc-modal-${k.replace(/_/g, '-')}: ${v};`);
for (const [k, v] of Object.entries(widget.dark)) css.push(`  --mlc-widget-${k.replace(/_/g, '-')}: ${v};`);
// Metric constants: every f64 const is a length, so all of them carry px.
for (const [group, prefix] of [
  ['layout', 'layout'],
  ['modal_shell', 'shell'],
  ['primitive_settings', 'ps'],
  ['primitive_settings_levels', 'ps-lv'],
  ['primitive_settings_vid', 'ps-vid'],
  ['primitive_settings_placement', 'ps-pl'],
  ['primitive_settings_labels', 'ps-lb'],
  ['primitive_settings_cells', 'ps-cl'],
]) {
  for (const [k, v] of Object.entries(metrics[group] || {})) {
    css.push(`  --mlc-${prefix}-${k.toLowerCase().replace(/_/g, '-')}: ${v}px;`);
  }
}
css.push('}');
writeFileSync(join(OUT, 'tokens.css'), css.join('\n') + '\n');

const count = (o) => Object.values(o).reduce((n, v) => n + (typeof v === 'object' ? Object.keys(v).length : 0), 0);
console.log(`tokens.json + tokens.css -> ${OUT}`);
console.log(`  ui.dark   ${count(tokens.ui.dark)} fields`);
console.log(`  ui.light  ${count(tokens.ui.light)} fields`);
console.log(`  widget    ${count(widget)} fields`);
console.log(`  modal     ${count(modal)} fields`);
console.log(`  metrics   ${Object.keys(metrics).length} modal files, ${count(metrics)} constants`);
console.log(`  icons     ${Object.keys(icons).length} svg`);
console.log(`  i18n      ${Object.keys(i18n).length} tables, ${Object.values(i18n).reduce((n,t)=>n+Object.keys(t).length,0)} keys`);
