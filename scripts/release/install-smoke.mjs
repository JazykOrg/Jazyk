import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {execFileSync} from 'node:child_process';
import {version} from './prepare.mjs';
const [directory,key]=process.argv.slice(2);
const dir=path.resolve(directory);
const temp=fs.mkdtempSync(path.join(os.tmpdir(),'jazyk install '));
try {
  fs.writeFileSync(path.join(temp,'package.json'),JSON.stringify({name:'jazyk-install-proof',private:true}));
  const options={cwd:temp,stdio:'inherit',shell:process.platform==='win32'};
  const args=['install','--ignore-scripts','--offline','--no-audit','--no-fund',path.join(dir,`jazyk-${key}-${version()}.tgz`),path.join(dir,`jazyk-${version()}.tgz`)];
  execFileSync('npm',process.platform==='win32'?args.map(a=>`"${a}"`):args,options);
  const launcher=path.join(temp,'node_modules/jazyk/bin/jazyk.js');
  const output=execFileSync(process.execPath,[launcher,'--help'],{cwd:temp,env:{...process.env,JAZYK_BINARY:'',JAZYK_DOWNLOAD_BASE:'http://127.0.0.1:1'},encoding:'utf8'});
  if(!output.includes('natural language compiler')) throw new Error('Packaged CLI did not run');
  console.log('Clean offline npm install passed, including path with spaces');
} finally {fs.rmSync(temp,{recursive:true,force:true});}
