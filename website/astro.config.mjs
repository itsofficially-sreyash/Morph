import { defineConfig, passthroughImageService } from 'astro/config';
import svelte from '@astrojs/svelte';

export default defineConfig({
  site: 'https://morph.example',
  integrations: [svelte()],
  image: { service: passthroughImageService() },
  build: { inlineStylesheets: 'always' },
  devToolbar: { enabled: false },
});
