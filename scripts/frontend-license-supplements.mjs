import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';

// Reviewed against npm 1.3.6 gitHead; upstream ships only an SPDX identifier.
const moleculeNoticeSha256 = '225a1baa202e4828277b2b752a7f8ed7270d07b8d00580caf3e9c7f065fca625';
export function frontendLicenseSupplement(root, name, version) {
  if (name !== '@dtinsight/molecule') return null;
  if (version !== '1.3.6') throw new Error('Re-review Molecule license provenance for version ' + version);
  const file = 'apps/desktop/public/licenses/molecule.txt';
  const bytes = fs.readFileSync(path.join(root, file));
  if (createHash('sha256').update(bytes).digest('hex') !== moleculeNoticeSha256) {
    throw new Error('Molecule license supplement is missing, truncated or changed; review it before release');
  }
  return {file, text: bytes.toString('utf8')};
}
