import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {execFileSync} from 'node:child_process';
const [version,dir]=process.argv.slice(2);
if(!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/.test(version)) throw new Error('Invalid version');
const platforms=JSON.parse(fs.readFileSync('scripts/release/platforms.json'));
const tag=version.includes('-')?'next':'latest';
for(const name of [...Object.keys(platforms).map(k=>`jazyk-${k}`),'jazyk']) {
  const tarball=path.resolve(dir,`${name}-${version}.tgz`);
  const integrity='sha512-'+crypto.createHash('sha512').update(fs.readFileSync(tarball)).digest('base64');
  const url=`https://registry.npmjs.org/${name}/${version}`;
  let response=await fetch(url,{signal:AbortSignal.timeout(30000)});
  if(response.ok) {
    const existing=await response.json();
    if(existing.dist?.integrity!==integrity) throw new Error(`Published ${name}@${version} has different bytes`);
    console.log(`Verified existing ${name}@${version}`);
  } else if(response.status===404) {
    execFileSync('npm',['publish',tarball,'--access','public',`--provenance=${process.env.SOURCE_PUBLIC === 'true'}`,'--tag',tag,'--registry=https://registry.npmjs.org/'],{stdio:'inherit'});
  } else throw new Error(`npm registry returned ${response.status} for ${name}`);
  // Registry visibility may lag publication briefly.
  let verified=false;
  for(let attempt=0;attempt<12;attempt++) {
    response=await fetch(url,{signal:AbortSignal.timeout(30000)});
    if(response.ok) {
      const published=await response.json();
      if(published.dist?.integrity!==integrity) throw new Error(`Registry integrity mismatch for ${name}`);
      verified=true; break;
    }
    if(response.status!==404) throw new Error(`Registry verification failed: ${response.status}`);
    await new Promise(resolve=>setTimeout(resolve,5000));
  }
  if(!verified) throw new Error(`Registry did not expose ${name}@${version}`);
}
// Verify public installation with a fresh cache after the launcher is available.
execFileSync('npm',['exec','--yes','--cache',path.resolve('.local/published-npm-cache'),`--package=jazyk@${version}`,'--','jazyk','--help'],{stdio:'inherit'});
