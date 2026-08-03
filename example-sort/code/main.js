#!/usr/bin/env node
'use strict';

const readline = require('node:readline');

function parseArgs(argv) {
  const reverse = argv.includes('-r');
  return { reverse };
}

function processRawLines(rawLines) {
  const lines = [];
  for (const rawLine of rawLines) {
    const stripped = rawLine.trim();
    if (stripped === '') continue;
    lines.push(stripped);
  }
  return lines;
}

async function readStdinLines(stream) {
  const rl = readline.createInterface({ input: stream });
  const rawLines = [];
  for await (const line of rl) {
    rawLines.push(line);
  }
  return processRawLines(rawLines);
}

// NOTE: docs contradict each other on sort direction (see diagnostics
// diag:contradiction-1..4). CLI args ("-r reverses sorting order to
// descending") and both documented test cases imply: default ascending,
// -r descending. The Execution pseudo code line "Sort lines descending.
// Or ascending if reverse order" states the opposite and is treated as
// the outlier. This implementation follows the CLI args + test suite.
function sortLines(lines, reverse) {
  const sorted = lines.slice().sort();
  if (reverse) sorted.reverse();
  return sorted;
}

function printLines(lines, out) {
  out.write(lines.join('\n') + (lines.length > 0 ? '\n' : ''));
}

async function main() {
  const { reverse } = parseArgs(process.argv.slice(2));
  const lines = await readStdinLines(process.stdin);
  printLines(sortLines(lines, reverse), process.stdout);
}

if (require.main === module) {
  main().catch((err) => {
    console.error(err);
    process.exit(1);
  });
}

module.exports = { parseArgs, processRawLines, readStdinLines, sortLines, printLines };
