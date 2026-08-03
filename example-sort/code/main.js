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
