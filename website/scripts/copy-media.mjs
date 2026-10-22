// The full demo video lives in docs/ (the README uses it too); copy it in instead of keeping two.
import { copyFileSync, mkdirSync } from 'node:fs';

mkdirSync(new URL('../public/media/', import.meta.url), { recursive: true });
copyFileSync(new URL('../../docs/morph_intro.mp4', import.meta.url), new URL('../public/media/morph_intro.mp4', import.meta.url));
