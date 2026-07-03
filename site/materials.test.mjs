import assert from 'node:assert/strict';
import { disposeMaterialsOnce } from './materials.js';

let mapDisposes = 0;
let materialDisposes = 0;
const sharedMap = { dispose() { mapDisposes++; } };
const shared = { map: sharedMap, dispose() { materialDisposes++; } };
const topMap = { dispose() { mapDisposes++; } };
const top = { map: topMap, dispose() { materialDisposes++; } };
const noMap = { dispose() { materialDisposes++; } };

const result = disposeMaterialsOnce([shared, shared, shared, shared, top, noMap]);
assert.deepEqual(result, { materials: 3, maps: 2 });
assert.equal(materialDisposes, 3, 'shared material is disposed once despite repeated faces');
assert.equal(mapDisposes, 2, 'shared texture map is disposed once despite repeated faces');

assert.deepEqual(disposeMaterialsOnce(null), { materials: 0, maps: 0 });
console.log('materials disposal regression: PASS');
