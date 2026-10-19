// A small seeded random generator: the same scatter on every load.

let rngState = 7;
export function rand() {
	rngState = (rngState * 1103515245 + 12345) & 0x7fffffff;
	return rngState / 0x7fffffff;
}
