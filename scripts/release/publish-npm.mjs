import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import {pathToFileURL} from 'node:url';
import {execFileSync} from 'node:child_process';

const registry='https://registry.npmjs.org/';
export async function publishGroup(names,{version,dir,fetchImpl=fetch,runNpm=args=>execFileSync('npm',args,{stdio:'inherit'}),sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms)),now=Date.now,timeoutMs=30*60*1000,log=console.log}) {
  const tag=version.includes('-')?'next':'latest';
  const pending=new Map();
  for(const name of names) {
    const tarball=path.resolve(dir,`${name}-${version}.tgz`);
    const integrity='sha512-'+crypto.createHash('sha512').update(fs.readFileSync(tarball)).digest('base64');
    const url=`${registry}${name}/${version}`;
    const response=await fetchImpl(url,{signal:AbortSignal.timeout(30000)});
    if(response.ok) {
      const existing=await response.json();
      if(existing.dist?.integrity!==integrity) throw new Error(`Published ${name}@${version} has different bytes`);
      log(`Verified existing ${name}@${version}`);
    } else if(response.status===404) {
      runNpm(['publish',tarball,'--access','public',`--provenance=${process.env.SOURCE_PUBLIC === 'true'}`,'--tag',tag,`--registry=${registry}`]);
      pending.set(name,{url,integrity});
    } else throw new Error(`npm registry returned ${response.status} for ${name}`);
  }
  // Submit all platforms before waiting for npm's publish-time scanning.
  const deadline=now()+timeoutMs;
  while(pending.size) {
    for(const [name,{url,integrity}] of pending) {
      const response=await fetchImpl(url,{signal:AbortSignal.timeout(30000)});
      if(response.ok) {
        const published=await response.json();
        if(published.dist?.integrity!==integrity) throw new Error(`Registry integrity mismatch for ${name}`);
        pending.delete(name);log(`Verified public ${name}@${version}`);
      } else if(response.status!==404) throw new Error(`Registry verification failed for ${name}: ${response.status}`);
    }
    if(!pending.size) break;
    if(now()>=deadline) throw new Error(`npm scanning did not finish for ${[...pending.keys()].join(', ')}; wait for public visibility before retrying`);
    log(`Waiting for npm scanning: ${[...pending.keys()].join(', ')}`);
    await sleep(10000);
  }
}

if(process.argv[1] && import.meta.url===pathToFileURL(path.resolve(process.argv[1])).href) {
  const [version,dir]=process.argv.slice(2);
  if(!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$/.test(version)) throw new Error('Invalid version');
  const platforms=JSON.parse(fs.readFileSync('scripts/release/platforms.json'));
  await publishGroup(Object.keys(platforms).map(k=>`jazyk-${k}`),{version,dir});
  await publishGroup(['jazyk'],{version,dir});
  // Verify public installation with a fresh cache after the launcher is available.
  execFileSync('npm',['exec','--yes','--cache',path.resolve('.local/published-npm-cache'),`--package=jazyk@${version}`,'--','jazyk','--help'],{stdio:'inherit'});
}
