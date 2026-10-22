import { defineConfig, passthroughImageService } from 'astro/config';
import svelte from '@astrojs/svelte';

export default defineConfig({
  // The public address, once there is a domain (used for the link-preview image).
  site: process.env.SITE_URL,
  integrations: [svelte()],
  image: { service: passthroughImageService() },
  build: { inlineStylesheets: 'always' },
  devToolbar: { enabled: false },
});
