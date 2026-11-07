import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import {publishGroup} from './publish-npm.mjs';

async function fixture(fn) {
 const dir=fs.mkdtempSync(path.join(os.tmpdir(),'jazyk publish '));
 const names=['jazyk-test-arm64','jazyk-test-x64'];
 const bytes=Buffer.from('release tarball fixture');
 const integrity='sha512-'+crypto.createHash('sha512').update(bytes).digest('base64');
 for(const name of names) fs.writeFileSync(path.join(dir,`${name}-0.2.0.tgz`),bytes);
 try {await fn({dir,names,integrity});}finally{fs.rmSync(dir,{recursive:true,force:true});}
}
const missing={ok:false,status:404};
const present=integrity=>({ok:true,status:200,json:async()=>({dist:{integrity}})});
test('all platform publishes precede scanning waits longer than one minute',()=>fixture(async({dir,names,integrity})=>{
 let time=0;const submitted=[];const calls=new Map();
 await publishGroup(names,{version:'0.2.0',dir,log:()=>{},now:()=>time,sleep:async ms=>{time+=ms;},runNpm:args=>submitted.push(path.basename(args[1])),fetchImpl:async url=>{
  const count=(calls.get(url)||0)+1;calls.set(url,count);
  if(count>1) assert.equal(submitted.length,names.length);
  return time<90000?missing:present(integrity);
 }});
 assert.equal(submitted.length,2);assert.equal(time,90000);
}));
test('existing versions must match exact tarball integrity',()=>fixture(async({dir,names})=>{
 let published=false;
 await assert.rejects(publishGroup(names,{version:'0.2.0',dir,fetchImpl:async()=>present('wrong bytes'),runNpm:()=>{published=true;}}),/different bytes/);
 assert.equal(published,false);
}));
test('processing timeout fails without claiming package availability',()=>fixture(async({dir,names})=>{
 let time=0;
 await assert.rejects(publishGroup(names,{version:'0.2.0',dir,fetchImpl:async()=>missing,runNpm:()=>{},log:()=>{},now:()=>time,sleep:async ms=>{time+=ms;},timeoutMs:20000}),/wait for public visibility before retrying/);
 assert.equal(time,20000);
}));
