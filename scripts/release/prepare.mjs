import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const platforms = JSON.parse(fs.readFileSync(path.join(root, 'scripts/release/platforms.json')));
export function version() {
  return /^version = "([^"]+)"/m.exec(fs.readFileSync(path.join(root, 'bootstrap/Cargo.toml'), 'utf8'))[1];
}
export function prepare(input, output, keys = Object.keys(platforms)) {
  const v = version();
  fs.mkdirSync(output, {recursive:true});
  const manifest = {version:v, commit:execFileSync('git', ['rev-parse', 'HEAD'], {cwd:root, encoding:'utf8'}).trim(), sha256:{}};
  const deps = {};
  for (const key of keys) {
    const meta = platforms[key];
    if (!meta) throw new Error(`Unknown platform ${key}`);
    const source = path.join(input, key);
    const binary = key.startsWith('win32') ? 'jazyk.exe' : 'jazyk';
    for (const file of [binary, `lib/${meta.library}`, 'LICENSE', 'THIRD_PARTY_NOTICES']) {
      if (!fs.statSync(path.join(source, file)).isFile()) throw new Error(`Missing ${key}/${file}`);
    }
    const pkg = path.join(output, `jazyk-${key}`);
    fs.mkdirSync(pkg, {recursive:true});
    fs.cpSync(source, path.join(pkg, 'bin'), {recursive:true});
    if (!key.startsWith('win32')) fs.chmodSync(path.join(pkg,'bin',binary),0o755);
    fs.copyFileSync(path.join(root,'LICENSE'), path.join(pkg,'LICENSE'));
    const [os,cpu] = key.split('-');
    fs.writeFileSync(path.join(pkg,'package.json'), JSON.stringify({name:`jazyk-${key}`, version:v, description:`Jazyk native distribution for ${key}`,license:'MIT',homepage:'https://jazyk.org',repository:{type:'git',url:'git+https://github.com/JazykOrg/Jazyk.git'}, os:[os],cpu:[cpu],...(os==='linux'?{libc:['glibc']}:{}),files:['bin/','LICENSE']},null,2));
    deps[`jazyk-${key}`]=v;
  }
  const main = path.join(output,'jazyk');
  fs.cpSync(path.join(root,'npm/jazyk'),main,{recursive:true});
  fs.copyFileSync(path.join(root,'LICENSE'),path.join(main,'LICENSE'));
  const pkg = JSON.parse(fs.readFileSync(path.join(main,'package.json')));
  pkg.version=v; pkg.optionalDependencies=deps;
  fs.writeFileSync(path.join(main,'package.json'),JSON.stringify(pkg,null,2));
  fs.chmodSync(path.join(main,'bin/jazyk.js'),0o755);
  for (const dir of [...Object.keys(deps),'jazyk']) {
    const result=JSON.parse(execFileSync('npm',['pack','--json','--ignore-scripts','--pack-destination',path.resolve(output)],{cwd:path.join(output,dir),encoding:'utf8',shell:process.platform==='win32'}));
    const file=result[0].filename;
    manifest.sha256[file]=crypto.createHash('sha256').update(fs.readFileSync(path.join(output,file))).digest('hex');
  }
  return manifest;
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [input,output,key]=process.argv.slice(2);
  if (!input||!output) throw new Error('Usage: prepare.mjs <distributions> <output> [single-platform]');
  const manifest=prepare(path.resolve(input),path.resolve(output),key?[key]:undefined);
  fs.writeFileSync(path.join(output,'npm-manifest.json'),JSON.stringify(manifest,null,2)+'\n');
}
