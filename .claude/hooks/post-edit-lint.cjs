#!/usr/bin/env node
/**
 * Project-level PostToolUse hook: format + lint edited source files.
 *
 * Installed into <project>/.claude/hooks/ by cct's scaffold.sh and wired up in
 * <project>/.claude/settings.json. Self-contained — no dependencies.
 *
 * Per edited file:
 *   .go          : goimports -w (fallback gofmt -w), then `go vet .` on the package
 *   .rs          : rustfmt, then `cargo clippy` on the crate
 *   .py          : ruff format + import sort, then `ruff check` on the file
 *   .ts/.js(x)   : prettier --write, then `tsc --noEmit` (when tsconfig.json found)
 *   .cpp/.h/...  : clang-format -i, then clang-tidy (when compile_commands.json found)
 *   .swift       : swiftformat, then swiftlint --fix + report remaining violations
 *
 * Formatting is silent (it just fixes the file). Lint findings are written to
 * stderr as warnings. Set WARN_ONLY = false to turn findings into blocking
 * feedback (exit code 2 feeds stderr back to Claude for immediate fixing).
 * Tools that aren't installed are skipped silently.
 */

const { execFileSync, spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const WARN_ONLY = true; // false → exit 2 on lint findings (stricter gate)
const LINT_TIMEOUT = 30000;
const MAX_LINES = 25;

function commandExists(cmd) {
  const probe = process.platform === 'win32' ? 'where' : 'which';
  return spawnSync(probe, [cmd], { stdio: 'pipe' }).status === 0;
}

function run(bin, args, opts = {}) {
  execFileSync(bin, args, {
    stdio: ['pipe', 'pipe', 'pipe'],
    timeout: LINT_TIMEOUT,
    ...opts,
  });
}

// Run a linter, returning its combined output when it exits non-zero.
function lint(bin, args, opts = {}) {
  try {
    run(bin, args, { encoding: 'utf8', ...opts });
    return '';
  } catch (err) {
    if (err.killed) return ''; // timeout — don't report partial noise
    return ((err.stdout || '') + (err.stderr || '')).trim();
  }
}

// Walk up from `dir` until a directory containing `marker` is found.
function findUp(dir, marker) {
  let d = dir;
  for (let i = 0; i < 20 && d !== path.dirname(d); i++) {
    if (fs.existsSync(path.join(d, marker))) return d;
    d = path.dirname(d);
  }
  return null;
}

function checkGo(file) {
  if (commandExists('goimports')) run('goimports', ['-w', file]);
  else if (commandExists('gofmt')) run('gofmt', ['-w', file]);

  if (!commandExists('go')) return '';
  // Vet only the edited file's package — `./...` is too slow per-edit.
  return lint('go', ['vet', '.'], { cwd: path.dirname(file) });
}

function checkRust(file) {
  // --edition 2021 so modern syntax parses when rustfmt runs standalone.
  if (commandExists('rustfmt')) run('rustfmt', ['--edition', '2021', file]);

  if (!commandExists('cargo')) return '';
  const crateDir = findUp(path.dirname(file), 'Cargo.toml');
  if (!crateDir) return '';
  // -D warnings: clippy warnings exit 0 and would be silently dropped by
  // lint() otherwise — promote them to errors so they're captured.
  return lint(
    'cargo',
    ['clippy', '--quiet', '--message-format', 'short', '--', '-D', 'warnings'],
    { cwd: crateDir },
  );
}

function checkPython(file) {
  if (!commandExists('ruff')) return '';
  run('ruff', ['format', file]);
  run('ruff', ['check', '--fix', '--select', 'I', file]); // import sorting
  return lint('ruff', ['check', file]);
}

// Locally-installed binary check — never let npx hit its interactive
// "Need to install…" prompt, which would stall every edit until timeout.
function hasLocalBin(pkgDir, tool) {
  const name = process.platform === 'win32' ? `${tool}.cmd` : tool;
  return fs.existsSync(path.join(pkgDir, 'node_modules', '.bin', name));
}

function checkTsJs(file) {
  const npx = process.platform === 'win32' ? 'npx.cmd' : 'npx';
  const pkgDir = findUp(path.dirname(file), 'package.json');
  if (!pkgDir) return '';
  if (hasLocalBin(pkgDir, 'prettier')) {
    try {
      run(npx, ['prettier', '--write', file], { cwd: pkgDir });
    } catch {
      // prettier failed (e.g. syntax error in file) — skip formatting
    }
  }

  if (!/\.tsx?$/.test(file)) return '';
  const tscDir = findUp(path.dirname(file), 'tsconfig.json');
  if (!tscDir || !hasLocalBin(pkgDir, 'tsc')) return '';
  const out = lint(npx, ['tsc', '--noEmit', '--pretty', 'false'], { cwd: tscDir });
  // Whole-project check — report only lines about the edited file.
  const rel = path.relative(tscDir, file);
  return out
    .split('\n')
    .filter((l) => l.includes(rel) || l.includes(file))
    .join('\n');
}

function checkCpp(file) {
  if (commandExists('clang-format')) run('clang-format', ['-i', file]);

  if (!commandExists('clang-tidy')) return '';
  // clang-tidy without a compilation database produces pure noise — only run
  // when one is found (project root or conventional build/ dir).
  const dir = path.dirname(file);
  let dbDir = findUp(dir, 'compile_commands.json');
  if (!dbDir) {
    const root = findUp(dir, 'CMakeLists.txt');
    if (root && fs.existsSync(path.join(root, 'build', 'compile_commands.json'))) {
      dbDir = path.join(root, 'build');
    }
  }
  if (!dbDir) return '';
  // --warnings-as-errors: same as clippy — warnings alone exit 0 and would
  // be silently dropped by lint().
  return lint('clang-tidy', ['-p', dbDir, '--quiet', '--warnings-as-errors=*', file]);
}

function checkSwift(file) {
  if (commandExists('swiftformat')) run('swiftformat', [file]);

  if (!commandExists('swiftlint')) return '';
  try {
    run('swiftlint', ['lint', '--fix', '--quiet', file]);
  } catch {
    // --fix can exit non-zero on unfixable violations — the report below covers them
  }
  return lint('swiftlint', ['lint', '--strict', '--quiet', file]);
}

function readStdin() {
  return new Promise((resolve) => {
    let data = '';
    const timer = setTimeout(() => resolve(data), 5000);
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', (c) => { if (data.length < 1024 * 1024) data += c; });
    process.stdin.on('end', () => { clearTimeout(timer); resolve(data); });
    process.stdin.on('error', () => { clearTimeout(timer); resolve(data); });
  });
}

(async () => {
  let findings = '';
  let file = '';
  try {
    const input = JSON.parse((await readStdin()) || '{}');
    file = input.tool_input?.file_path || '';
    if (file && fs.existsSync(file)) {
      if (/\.go$/.test(file)) findings = checkGo(file);
      else if (/\.rs$/.test(file)) findings = checkRust(file);
      else if (/\.pyi?$/.test(file)) findings = checkPython(file);
      else if (/\.(ts|tsx|js|jsx)$/.test(file)) findings = checkTsJs(file);
      else if (/\.(cpp|cc|cxx|hpp|h|hxx)$/.test(file)) findings = checkCpp(file);
      else if (/\.swift$/.test(file)) findings = checkSwift(file);
    }
  } catch {
    // Malformed input or formatter failure — never block the edit itself.
  }

  if (findings) {
    const lines = findings.split('\n').slice(0, MAX_LINES);
    console.error(`[Hook] Lint findings after editing ${path.basename(file)}:`);
    lines.forEach((l) => console.error(l));
    if (!WARN_ONLY) process.exit(2);
  }
  process.exit(0);
})();
