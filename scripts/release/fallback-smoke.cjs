const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const {execFileSync} = require('node:child_process');
const [packages, archives, key] = process.argv.slice(2);
const platforms = require('./platforms.json');
const meta = platforms[key];
const extension = key.startsWith('win32') ? 'zip' : 'tar.gz';
const name = `jazyk-${meta.target}.${extension}`;
const archive = fs.readFileSync(path.join(archives, name));
const sha = crypto.createHash('sha256').update(archive).digest('hex');
const {fetchBinary} = require(path.resolve(packages, 'jazyk/bin/jazyk.js'));
const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'jazyk fallback '));
(async () => {
  try {
    const binary = await fetchBinary(key, {
      cache: path.join(temp, 'cache'), temporaryRoot: temp,
      fetchImpl: async url => {
        if (url.endsWith('/SHA256SUMS')) return new Response(`${sha}  ${name}\n`);
        if (url.endsWith('/' + name)) return new Response(archive);
        throw new Error(`Unexpected download ${url}`);
      },
    });
    const env = {...process.env};
    delete env.JAZYK_PLANTUML;
    delete env.JAZYK_PDFIUM;
    delete env.JAZYK_PROOF_PDFIUM;
    const output = execFileSync(binary, ['--help'], {env, encoding:'utf8'});
    if (!output.includes('natural language compiler')) throw new Error('Cached CLI failed');
    const suffix = process.platform === 'win32' ? '.exe' : '';
    const smoke = path.join(temp, 'cache', `smoke${suffix}`);
    fs.copyFileSync(`bootstrap/target/release/examples/distribution_smoke${suffix}`, smoke);
    fs.chmodSync(smoke, 0o755);
    execFileSync(smoke, [], {env, stdio:'inherit'});
    console.log('Checksummed download fallback runs CLI, PDF, DOCX and diagrams');
  } finally {
    fs.rmSync(temp, {recursive:true, force:true});
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
