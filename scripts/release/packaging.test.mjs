import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {execFileSync,spawnSync} from 'node:child_process';
import {prepare,version} from './prepare.mjs';
const platforms=JSON.parse(fs.readFileSync('scripts/release/platforms.json'));
test('all distributions pack with exact dependencies, platform gates and verified release hashes',()=>{
 const temp=fs.mkdtempSync(path.join(os.tmpdir(),'jazyk packages '));
 try {
  const input=path.join(temp,'input'),output=path.join(temp,'packages'),assets=path.join(temp,'assets');
  for(const [key,p] of Object.entries(platforms)) {
   const dist=path.join(input,key);fs.mkdirSync(path.join(dist,'lib'),{recursive:true});
   for(const file of [key.startsWith('win32')?'jazyk.exe':'jazyk',`lib/${p.library}`,'LICENSE','THIRD_PARTY_NOTICES']) fs.writeFileSync(path.join(dist,file),'release fixture');
  }
  const manifest=prepare(input,output);
  assert.equal(Object.keys(manifest.sha256).length,6);
  const pkg=JSON.parse(fs.readFileSync(path.join(output,'jazyk/package.json')));
  assert.equal(pkg.homepage,'https://jazyk.org');
  assert.deepEqual(pkg.optionalDependencies,Object.fromEntries(Object.keys(platforms).map(k=>[`jazyk-${k}`,version()])));
  for(const key of Object.keys(platforms)) {
   const meta=JSON.parse(fs.readFileSync(path.join(output,`jazyk-${key}/package.json`)));
   assert.equal(meta.homepage,'https://jazyk.org');
   assert.deepEqual(meta.os,[key.split('-')[0]]); assert.deepEqual(meta.cpu,[key.split('-')[1]]);
   if(key.startsWith('linux')) assert.deepEqual(meta.libc,['glibc']);
   const names=execFileSync('tar',['-tf',path.join(output,`jazyk-${key}-${version()}.tgz`)],{encoding:'utf8'});
   assert.ok(names.includes('THIRD_PARTY_NOTICES'));assert.ok(names.includes('package/bin/lib/'));
   assert.ok(!names.includes('.env'));
  }
  fs.mkdirSync(assets);
  for(const file of fs.readdirSync(output).filter(f=>f.endsWith('.tgz'))) fs.copyFileSync(path.join(output,file),path.join(assets,file));
  const python=(...args)=>execFileSync('python3',['scripts/release/artifacts.py',...args],{stdio:'pipe'});
  python('archive',input,assets);python('seal',assets,version(),manifest.commit);python('verify',assets,version(),manifest.commit);
  fs.appendFileSync(path.join(assets,`jazyk-${version()}.tgz`),'tamper');
  assert.throws(()=>python('verify',assets,version(),manifest.commit));
 }finally{fs.rmSync(temp,{recursive:true,force:true});}
});
