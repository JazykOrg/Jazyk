'use strict';

const { test } = require('node:test');
const assert = require('node:assert');
const { execFileSync } = require('node:child_process');
const path = require('node:path');

const MAIN = path.join(__dirname, '..', 'main.js');
const { parseArgs, processRawLines, sortLines } = require(MAIN);

function runCli(args, input) {
  return execFileSync(process.execPath, [MAIN, ...args], {
    input,
    encoding: 'utf8',
  });
}

test('req_main_js_1_37d40d50', () => {
  // A CLI utility: invocable as a command, reads stdin, writes stdout, exits 0.
  const out = runCli([], 'b\na\n');
  assert.strictEqual(typeof out, 'string');
  assert.ok(out.length > 0, 'CLI produced output');
});

test('req_main_js_4_78c6e15e', () => {
  // Written in JavaScript: node accepts it as valid JS syntax.
  execFileSync(process.execPath, ['--check', MAIN], { encoding: 'utf8' });
});

test('req_main_js_5_d8e51eea', () => {
  // -r reverses sorting order to descending.
  assert.strictEqual(runCli(['-r'], 'a\nc\nb\n'), 'c\nb\na\n');
});

test('req_main_js_6_ca55a88e', () => {
  // CLI arguments are read: passing -r changes the observable behavior.
  const input = 'a\nb\n';
  assert.notStrictEqual(runCli(['-r'], input), runCli([], input));
});

test('req_main_js_7_3d5ac8eb', () => {
  // Reverse order is tracked from the -r argument.
  assert.strictEqual(parseArgs(['-r']).reverse, true);
  assert.strictEqual(parseArgs([]).reverse, false);
});

test('req_main_js_8_9701cd86', () => {
  // Input lines accumulate in an array of strings.
  const lines = processRawLines(['one', 'two']);
  assert.ok(Array.isArray(lines));
  assert.ok(lines.every((l) => typeof l === 'string'));
  assert.deepStrictEqual(lines, ['one', 'two']);
});

test('req_main_js_9_489de1ea', () => {
  // Input is read from STDIN line by line: every stdin line is processed.
  assert.strictEqual(runCli([], 'delta\nalpha\ncharlie\nbravo\n'), 'alpha\nbravo\ncharlie\ndelta\n');
});

test('req_main_js_10_4112a225', () => {
  // Leading and trailing whitespace is stripped from each line.
  assert.strictEqual(runCli([], '  b  \n\ta \n'), 'a\nb\n');
});

test('req_main_js_11_2fea0114', () => {
  // Lines that are empty after stripping are skipped.
  assert.strictEqual(runCli([], 'b\n\n   \n\t\na\n'), 'a\nb\n');
});

test('req_main_js_12_881a9320', () => {
  // Every non-empty stripped line is added to the accumulated lines.
  assert.deepStrictEqual(processRawLines([' x ', '', 'y', '   ']), ['x', 'y']);
});

test('req_main_js_15_31c641ea', () => {
  // All sorted lines are printed delimited by newline.
  const out = runCli([], 'b\na\nc\n');
  assert.deepStrictEqual(out.split('\n').filter((l) => l !== ''), ['a', 'b', 'c']);
  assert.strictEqual(out, 'a\nb\nc\n');
});

test('req_main_js_16_6c7e91df', () => {
  // Default sort order (no -r) is ascending.
  assert.strictEqual(runCli([], 'c\na\nb\n'), 'a\nb\nc\n');
  assert.deepStrictEqual(sortLines(['c', 'a', 'b'], false), ['a', 'b', 'c']);
});

test('req_main_test_js_1_5f95d5d9', () => {
  // Simple case: documented input/expected output.
  assert.strictEqual(runCli([], '321\n654\n453\n'), '321\n453\n654\n');
});

test('req_main_test_js_2_1c6d8c96', () => {
  // Reverse order case: documented input/expected output with the reverse flag on.
  assert.strictEqual(runCli(['-r'], '321\n654\n453\n'), '654\n453\n321\n');
});
