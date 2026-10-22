import { defineConfig, passthroughImageService } from 'astro/config';
import svelte from '@astrojs/svelte';

export default defineConfig({
  // The public address (used for the link-preview image). SITE_URL overrides it, e.g. for a domain later.
  site: process.env.SITE_URL ?? 'https://morphconvert.pages.dev',
  integrations: [svelte()],
  image: { service: passthroughImageService() },
  build: { inlineStylesheets: 'always' },
  devToolbar: { enabled: false },
});
