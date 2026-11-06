#!/usr/bin/env node
'use strict';

// Launcher for the platform-native jazyk binary.
//
// Resolution order:
//   1. JAZYK_BINARY env override
//   2. The jazyk-<platform> optional dependency
//   3. Cached download in the user cache dir
//   4. Download from GitHub Releases (base overridable via JAZYK_DOWNLOAD_BASE)
//
// This script must NEVER write to stdout: in `--mcp` mode stdout carries
// JSON-RPC framing for the MCP client. All diagnostics go to stderr.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, spawnSync } = require('child_process');
const { Readable, Transform } = require('stream');
const crypto = require('crypto');
const { pipeline } = require('stream/promises');

const pkg = require('../package.json');
const VERSION = pkg.version;
const DOWNLOAD_BASE =
  process.env.JAZYK_DOWNLOAD_BASE ||
  'https://github.com/JazykOrg/Jazyk/releases/download';

const PLATFORMS = {
  'darwin-arm64': { triple: 'aarch64-apple-darwin', ext: 'tar.gz' },
  'darwin-x64': { triple: 'x86_64-apple-darwin', ext: 'tar.gz' },
  'linux-x64': { triple: 'x86_64-unknown-linux-gnu', ext: 'tar.gz' },
  'linux-arm64': { triple: 'aarch64-unknown-linux-gnu', ext: 'tar.gz' },
  'win32-x64': { triple: 'x86_64-pc-windows-msvc', ext: 'zip' },
};

function isMusl() {
  // glibcVersionRuntime is absent on musl-based systems (e.g. Alpine).
  try {
    const report = process.report.getReport();
    return !report.header.glibcVersionRuntime;
  } catch {
    return false;
  }
}

function platformKey() {
  const { platform, arch } = process;
  if (platform === 'darwin' && arch === 'arm64') return 'darwin-arm64';
  if (platform === 'darwin' && arch === 'x64') return 'darwin-x64';
  if (platform === 'linux' && arch === 'x64')
    return isMusl() ? null : 'linux-x64';
  if (platform === 'linux' && arch === 'arm64') {
    if (isMusl()) return null;
    return 'linux-arm64';
  }
  if (platform === 'win32' && arch === 'x64') return 'win32-x64';
  return null;
}

function fail(message) {
  process.stderr.write(`jazyk: ${message}\n`);
  process.exit(1);
}

function binName() {
  return process.platform === 'win32' ? 'jazyk.exe' : 'jazyk';
}

function resolveOptionalDep(key) {
  try {
    return require.resolve(`jazyk-${key}/bin/${binName()}`);
  } catch {
    return null;
  }
}

function cacheDir(key) {
  if (process.platform === 'win32' && process.env.LOCALAPPDATA) {
    return path.join(process.env.LOCALAPPDATA, 'jazyk', 'bin', VERSION, key);
  }
  const base =
    process.env.XDG_CACHE_HOME || path.join(os.homedir(), '.cache');
  return path.join(base, 'jazyk', VERSION, key);
}

async function download(url, dest, { timeoutMs = 120_000, maxBytes = 512 * 1024 * 1024, fetchImpl = fetch } = {}) {
  const signal = AbortSignal.timeout(timeoutMs);
  const res = await fetchImpl(url, { redirect: 'follow', signal });
  if (!res.ok) throw new Error(`download failed: ${res.status} ${res.statusText} (${url})`);
  if (!res.body) throw new Error(`download had no body (${url})`);
  const hash = crypto.createHash('sha256');
  let received = 0;
  const digest = new Transform({
    transform(chunk, encoding, callback) {
      received += chunk.length;
      if (received > maxBytes) return callback(new Error(`download exceeds ${maxBytes} bytes`));
      hash.update(chunk);
      callback(null, chunk);
    },
  });
  await pipeline(Readable.fromWeb(res.body), digest, fs.createWriteStream(dest, { flags: 'wx' }), { signal });
  return hash.digest('hex');
}

function expectedChecksum(manifest, archiveName) {
  const matches = manifest.split(/\r?\n/).flatMap(line => {
    const match = /^([a-fA-F0-9]{64}) [ *](.+)$/.exec(line);
    return match && match[2] === archiveName ? [match[1].toLowerCase()] : [];
  });
  if (matches.length !== 1) throw new Error(`SHA256SUMS must contain exactly one checksum for ${archiveName}; install the platform package or use a release with a checksum manifest`);
  return matches[0];
}

async function fetchBinary(key, { base = DOWNLOAD_BASE, timeoutMs = 120_000, cache = cacheDir(key), temporaryRoot = os.tmpdir(), fetchImpl = fetch } = {}) {
  const { triple, ext } = PLATFORMS[key];
  const dir = cache;
  const cached = path.join(dir, binName());
  const complete = path.join(dir, '.complete');
  if (fs.existsSync(complete) && fs.lstatSync(cached).isFile()) return cached;

  const archiveName = `jazyk-${triple}.${ext}`;
  const url = `${base}/v${VERSION}/${archiveName}`;
  process.stderr.write(`jazyk: downloading ${url}\n`);
  const parent = path.dirname(dir);
  fs.mkdirSync(parent, { recursive: true });
  const staging = fs.mkdtempSync(path.join(parent, '.jazyk-'));
  const tmp = fs.mkdtempSync(path.join(temporaryRoot, 'jazyk-'));
  try {
    const archive = path.join(tmp, archiveName);
    const manifestPath = path.join(tmp, 'SHA256SUMS');
    await download(`${base}/v${VERSION}/SHA256SUMS`, manifestPath, { timeoutMs, maxBytes: 128 * 1024, fetchImpl });
    const expected = expectedChecksum(fs.readFileSync(manifestPath, 'utf8'), archiveName);
    const actual = await download(url, archive, { timeoutMs, fetchImpl });
    if (actual !== expected) throw new Error(`SHA-256 mismatch for ${archiveName}; refusing to extract or execute it`);
    // Only these release-owned paths are extracted. PDFium lives beside the binary.
    const tar = spawnSync('tar', ['-xf', archive, '-C', staging, '--', binName(), 'lib', 'LICENSE', 'THIRD_PARTY_NOTICES'], {
      stdio: ['ignore', 'ignore', 'inherit'], timeout: timeoutMs, killSignal: 'SIGKILL',
    });
    if (tar.status !== 0) throw new Error('failed to extract archive');
    const binary = path.join(staging, binName());
    if (!fs.lstatSync(binary).isFile() || !fs.statSync(path.join(staging, 'lib')).isDirectory()) {
      throw new Error('archive is missing its native distribution');
    }
    if (process.platform !== 'win32') fs.chmodSync(binary, 0o755);
    fs.writeFileSync(path.join(staging, '.complete'), VERSION);
    try { fs.renameSync(staging, dir); }
    catch (error) {
      if (!fs.existsSync(complete)) throw error;
      // A concurrent launcher installed the same immutable version first.
    }
    return cached;
  } finally {
    fs.rmSync(staging, { recursive: true, force: true });
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

async function resolveBinary() {
  if (process.env.JAZYK_BINARY) return process.env.JAZYK_BINARY;

  const key = platformKey();
  if (!key) {
    fail(
      `unsupported platform ${process.platform}-${process.arch}; ` +
        `build from source: https://github.com/JazykOrg/Jazyk`
    );
  }
  const fromDep = resolveOptionalDep(key);
  if (fromDep) return fromDep;

  try {
    return await fetchBinary(key);
  } catch (err) {
    fail(
      `could not locate the jazyk-${key} package or download the ` +
        `binary (${err.message}). Try reinstalling without --ignore-scripts/` +
        `--omit=optional, or set JAZYK_BINARY to a jazyk binary.`
    );
  }
}

async function main() {
  const bin = await resolveBinary();
  const child = spawn(bin, process.argv.slice(2), { stdio: 'inherit' });

  // Forward termination signals; let the child drive shutdown so MCP servers
  // exit cleanly when the client (e.g. Claude Code) stops them.
  for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    process.on(sig, () => {
      if (child.exitCode === null && child.signalCode === null) child.kill(sig);
    });
  }

  child.on('error', (err) => fail(`failed to run ${bin}: ${err.message}`));
  child.on('exit', (code, signal) => {
    if (signal) {
      // Re-raise so our own exit status reflects the child's signal death.
      process.removeAllListeners(signal);
      process.kill(process.pid, signal);
    } else {
      process.exit(code == null ? 1 : code);
    }
  });
}

if (require.main === module) main().catch(error => fail(error.message));
module.exports = { download, expectedChecksum, fetchBinary, platformKey };
