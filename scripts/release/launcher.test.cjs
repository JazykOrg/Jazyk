const {test}=require('node:test');
const assert=require('node:assert/strict');
const fs=require('node:fs');
const os=require('node:os');
const path=require('node:path');
const crypto=require('node:crypto');
const {spawn,spawnSync}=require('node:child_process');
const {download,expectedChecksum,fetchBinary}=require('../../npm/jazyk/bin/jazyk.js');
const launcher=path.resolve('npm/jazyk/bin/jazyk.js');
const hash='a'.repeat(64);
test('checksums must identify one exact archive',()=>{
 assert.equal(expectedChecksum(`${hash}  archive.tgz\n`,'archive.tgz'),hash);
 assert.throws(()=>expectedChecksum(`${hash}  other.tgz`,'archive.tgz'));
 assert.throws(()=>expectedChecksum(`${hash}  a\n${hash}  a`,'a'));
});
test('override preserves arguments, stdin and exit code without stdout contamination',()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'jazyk args '));
 try {
  const script=path.join(dir,'args.cjs');
  fs.writeFileSync(script,"process.stdout.write(JSON.stringify({args:process.argv.slice(2),input:require('fs').readFileSync(0,'utf8')})); process.exitCode=7;");
  const result=spawnSync(process.execPath,[launcher,script,'hello world','--flag'],{input:'protocol input',env:{...process.env,JAZYK_BINARY:process.execPath},encoding:'utf8'});
  assert.equal(result.status,7); assert.equal(result.stderr,'');
  assert.deepEqual(JSON.parse(result.stdout),{args:['hello world','--flag'],input:'protocol input'});
 } finally {fs.rmSync(dir,{recursive:true,force:true});}
});
test('termination reaches the child and launcher exits with its signal',{skip:process.platform==='win32'},async()=>{
 const child=spawn(process.execPath,[launcher,'-e',"process.stdout.write('ready');setInterval(()=>{},1000)"],{env:{...process.env,JAZYK_BINARY:process.execPath}});
 const timer=setTimeout(()=>child.kill('SIGKILL'),10000);
 try {
  await new Promise(resolve=>child.stdout.once('data',resolve));
  const ended=new Promise(resolve=>child.once('exit',(code,signal)=>resolve({code,signal})));
  child.kill('SIGTERM'); assert.deepEqual(await ended,{code:null,signal:'SIGTERM'});
 } finally {clearTimeout(timer);child.kill('SIGKILL');}
});
test('download rejects oversized data and checksums before extraction',async()=>{
 const temp=fs.mkdtempSync(path.join(os.tmpdir(),'jazyk checksum '));
 try {
  await assert.rejects(download('https://example.test',path.join(temp,'large'),{maxBytes:1,fetchImpl:async()=>new Response('too large')}),/exceeds/);
  await assert.rejects(fetchBinary('darwin-arm64',{cache:path.join(temp,'cache','version'),temporaryRoot:temp,fetchImpl:async url=>new Response(url.endsWith('SHA256SUMS')?`${hash}  jazyk-aarch64-apple-darwin.tar.gz\n`:'incorrect archive')}),/SHA-256 mismatch/);
  assert.equal(fs.existsSync(path.join(temp,'cache','version')),false);
 } finally {fs.rmSync(temp,{recursive:true,force:true});}
});
test('fallback installs full distribution atomically and reuses it',{skip:process.platform==='win32'},async()=>{
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'jazyk archive '));
 try {
  const files=path.join(dir,'source');fs.mkdirSync(path.join(files,'lib'),{recursive:true});
  for(const name of ['jazyk','LICENSE','THIRD_PARTY_NOTICES','lib/libpdfium.dylib']) fs.writeFileSync(path.join(files,name),'fixture');
  const archive=path.join(dir,'release.tar.gz');
  assert.equal(spawnSync('tar',['-czf',archive,'-C',files,'jazyk','lib','LICENSE','THIRD_PARTY_NOTICES']).status,0);
  const bytes=fs.readFileSync(archive);const sha=crypto.createHash('sha256').update(bytes).digest('hex');
  const fetchImpl=async url=>new Response(url.endsWith('SHA256SUMS')?`${sha}  jazyk-aarch64-apple-darwin.tar.gz\n`:bytes);
  const cache=path.join(dir,'cache','version');
  const options={cache,temporaryRoot:dir,fetchImpl};
  const binaries=await Promise.all([fetchBinary('darwin-arm64',options),fetchBinary('darwin-arm64',options)]);
  assert.equal(binaries[0],binaries[1]);assert.ok(fs.existsSync(path.join(cache,'lib/libpdfium.dylib')));
  assert.equal(await fetchBinary('darwin-arm64',{...options,fetchImpl:()=>{throw new Error('must use cache');}}),binaries[0]);
 } finally {fs.rmSync(dir,{recursive:true,force:true});}
});
