import {test} from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {frontendLicenseSupplement} from './frontend-license-supplements.mjs';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
test('reviewed Molecule supplement retains provenance, attribution and MIT terms', () => {
  const notice = frontendLicenseSupplement(root, '@dtinsight/molecule', '1.3.6');
  assert.match(notice.text, /DTStack Corporation/);
  assert.match(notice.text, /Permission is hereby granted/);
  assert.match(notice.text, /THE SOFTWARE IS PROVIDED/);
  assert.match(notice.text, /a114a2adc3c88bdc12e8b0693d09386a1d199ea7/);
  assert.equal(frontendLicenseSupplement(root, 'unrelated', '1'), null);
  assert.throws(() => frontendLicenseSupplement(root, '@dtinsight/molecule', '1.3.7'), /Re-review/);
});
test('an identifier-only replacement or absent file blocks release collection', () => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'simple-license-'));
  try {
    assert.throws(() => frontendLicenseSupplement(temp, '@dtinsight/molecule', '1.3.6'), /ENOENT/);
    const folder = path.join(temp, 'apps/desktop/public/licenses');
    fs.mkdirSync(folder, {recursive:true});
    fs.writeFileSync(path.join(folder, 'molecule.txt'), 'MIT');
    assert.throws(() => frontendLicenseSupplement(temp, '@dtinsight/molecule', '1.3.6'), /truncated/);
  } finally {
    fs.unlinkSync(path.join(temp, 'apps/desktop/public/licenses/molecule.txt'));
    for (const relative of ['apps/desktop/public/licenses', 'apps/desktop/public', 'apps/desktop', 'apps', '']) fs.rmdirSync(path.join(temp, relative));
  }
});
