const fs = require('fs');
const path = require('path');
const assert = require('assert');

function walk(dir, acc = []) {
  if (!fs.existsSync(dir)) return acc;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === 'node_modules' || entry.name === '.git' || entry.name === 'tests') continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full, acc);
    else acc.push(full);
  }
  return acc;
}

function crit_main_4_56255dbe() {
  const root = path.resolve(__dirname, '..');
  const packagePath = path.join(root, 'package.json');
  assert.ok(fs.existsSync(packagePath), 'package.json must exist to show the frontend build dependencies');

  const pkg = JSON.parse(fs.readFileSync(packagePath, 'utf8'));
  const deps = { ...(pkg.dependencies || {}), ...(pkg.devDependencies || {}) };
  assert.ok(deps.react, 'React must be declared as a dependency or devDependency');
  assert.ok(deps.typescript, 'TypeScript must be declared as a dependency or devDependency');

  const sourceFiles = walk(root).map((file) => path.relative(root, file));
  const tsxOrTsFiles = sourceFiles.filter((file) => /\.(tsx|ts)$/.test(file));
  assert.ok(tsxOrTsFiles.length > 0, 'the frontend must include TypeScript source files');

  const reactUsingFiles = tsxOrTsFiles.filter((file) => {
    const text = fs.readFileSync(path.join(root, file), 'utf8');
    return /from ['"]react['"]|require\(['"]react['"]\)|<\w/.test(text);
  });
  assert.ok(reactUsingFiles.length > 0, 'the TypeScript frontend source must use React');
}

if (require.main === module) {
  const name = process.argv[2];
  if (name && name !== 'crit_main_4_56255dbe') {
    throw new Error(`Unknown test name: ${name}`);
  }
  crit_main_4_56255dbe();
  console.log('crit_main_4_56255dbe passed');
}

module.exports = { crit_main_4_56255dbe };
