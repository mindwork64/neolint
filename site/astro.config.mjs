import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://mindwork64.github.io',
  base: '/neolint',
  integrations: [
    starlight({
      title: 'neolint',
      description: 'A deterministic, offline-by-default linter for Linden Scripting Language.',
      logo: {
        src: './src/assets/neolint-logo.png',
        alt: 'neolint — code quality linter',
      },
      favicon: '/favicon.ico',
      disable404Route: true,
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/mindwork64/neolint',
        },
      ],
      expressiveCode: {
        shiki: {
          langs: [import('./src/langs/lsl.mjs')],
        },
      },
      customCss: ['./src/styles/brand.css'],
      sidebar: [
        { label: 'Home', link: '/' },
        { label: 'Diagnostics', link: '/diagnostics/' },
        {
          label: 'Documentation',
          items: [{ autogenerate: { directory: 'docs' } }],
        },
      ],
      editLink: {
        baseUrl: 'https://github.com/mindwork64/neolint/edit/main/docs/',
      },
      tableOfContents: {
        minHeadingLevel: 2,
        maxHeadingLevel: 3,
      },
    }),
  ],
});
