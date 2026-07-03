// Material disposal helpers shared by the pixel world and regression tests.
// Three.js materials may appear more than once in a mesh.material array; maps
// may also be shared. Dispose each object at most once to avoid GPU leaks while
// avoiding double-dispose surprises in tests and browser devtools.
export function disposeMaterialsOnce(materials) {
	const list = Array.isArray(materials) ? materials : [materials];
	const seenMaterials = new Set();
	const seenMaps = new Set();
	for (const material of list) {
		if (!material || seenMaterials.has(material)) continue;
		seenMaterials.add(material);
		const map = material.map;
		if (map && !seenMaps.has(map)) {
			seenMaps.add(map);
			map.dispose?.();
		}
		material.dispose?.();
	}
	return { materials: seenMaterials.size, maps: seenMaps.size };
}
