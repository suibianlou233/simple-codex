// Generate accompanying notices from installed, locked dependencies; no network.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const output = path.join(root, 'target', 'release-licenses');
fs.mkdirSync(output, { recursive: true });
const notices = ['# Locked dependency license notices', '',
  'Generated from local Cargo metadata (Windows, including build/development dependencies)',
  'and installed frontend production dependencies. This is a conservative inventory,',
  'not a claim that every listed dependency is present in the shipped executable.',
  'Pinned Codex provenance and license notices are supplied separately.', ''];
const inventory = [];
const seenCargo = new Set();

function add(ecosystem, name, version, expression, directory) {
  const files = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (!/^(licen[cs]e|notice|copyright|copying)([.\-_]|$)/i.test(entry.name)) continue;
    if (entry.isFile()) files.push(entry.name);
    if (entry.isDirectory()) {
      for (const child of fs.readdirSync(path.join(directory, entry.name), { withFileTypes: true })) {
        if (child.isFile()) files.push(path.join(entry.name, child.name));
      }
    }
  }
  inventory.push({ ecosystem, name, version, license: expression ?? null, noticeFiles: files });
  notices.push(`## ${ecosystem}: ${name} ${version}`, '', `License expression: ${expression ?? 'see package source'}`, '');
  for (const file of files.sort()) {
    notices.push(`### ${file.replaceAll('\\', '/')}`, '', fs.readFileSync(path.join(directory, file), 'utf8'), '');
  }
  if (!files.length) notices.push('No root license text in this cached package; refer to its declared license and upstream source.', '');
}

function collectCargo(directory) {
  const cargo = JSON.parse(execFileSync('cargo', [
    'metadata', '--locked', '--offline', '--format-version', '1',
    '--filter-platform', 'x86_64-pc-windows-msvc'
  ], { cwd: directory, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }));
  for (const pkg of cargo.packages.filter(p => p.source).sort((a, b) => a.name.localeCompare(b.name))) {
    if (seenCargo.has(pkg.id)) continue;
    seenCargo.add(pkg.id);
    add('cargo', pkg.name, pkg.version, pkg.license, path.dirname(pkg.manifest_path));
  }
}
collectCargo(root);
if (process.argv[2]) {
  const kernelRoot = path.resolve(process.argv[2]);
  const revision = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: kernelRoot, encoding: 'utf8' }).trim();
  if (revision !== '28327355b861ab6cc76b01c7248663eb1be440cf') throw new Error('Unexpected kernel source revision');
  collectCargo(path.join(kernelRoot, 'codex-rs'));
}

const visited = new Set();
function visit(packageFile) {
  const pkg = JSON.parse(fs.readFileSync(packageFile, 'utf8'));
  const require = createRequire(packageFile);
  for (const name of Object.keys(pkg.dependencies ?? {}).sort()) {
    const resolved = require.resolve.paths(name).map(p => path.join(p, name, 'package.json')).find(p => fs.existsSync(p));
    if (!resolved) throw new Error(`Missing installed production dependency: ${name}`);
    const real = fs.realpathSync(resolved);
    if (visited.has(real)) continue;
    visited.add(real);
    const dependency = JSON.parse(fs.readFileSync(real, 'utf8'));
    add('npm', dependency.name, dependency.version, dependency.license, path.dirname(real));
    visit(real);
  }
}
visit(path.join(root, 'apps', 'desktop', 'package.json'));
fs.writeFileSync(path.join(output, 'DEPENDENCY_LICENSES.md'), notices.join('\n'));
fs.writeFileSync(path.join(output, 'dependency-inventory.json'), JSON.stringify(inventory, null, 2) + '\n');
console.log(JSON.stringify({ packages: inventory.length, missingRootNotices: inventory.filter(p => !p.noticeFiles.length).map(p => `${p.ecosystem}:${p.name}@${p.version}`), output }, null, 2));
