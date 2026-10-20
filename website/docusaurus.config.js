// Kule's documentation site. The pages are the Markdown files in ../docs,
// so they read on GitHub as they are; this folder holds only the site.
// @ts-check

import { themes as prismThemes } from 'prism-react-renderer';

// Docusaurus builds each locale on its own and names it here, so the few
// strings the theme cannot translate from i18n/ follow it.
const tr = process.env.DOCUSAURUS_CURRENT_LOCALE === 'tr';

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'Kule',
  tagline: tr
    ? 'Klasörlerinizde çalışan yapay zekâ ajanları için bir kontrol odası'
    : 'A control room for AI agents that work on your folders',
  favicon: 'img/favicon.svg',

  url: 'https://kulehq.github.io',
  baseUrl: '/kule/',
  organizationName: 'kulehq',
  projectName: 'kule',
  trailingSlash: false,

  onBrokenLinks: 'throw',
  onBrokenAnchors: 'throw',
  markdown: {
    format: 'detect',
    hooks: { onBrokenMarkdownLinks: 'throw' },
  },

  i18n: {
    defaultLocale: 'en',
    locales: ['en', 'tr'],
    localeConfigs: {
      en: { label: 'English', htmlLang: 'en' },
      tr: { label: 'Türkçe', htmlLang: 'tr' },
    },
  },

  presets: [
    [
      'classic',
      /** @type {import('@docusaurus/preset-classic').Options} */
      ({
        docs: {
          path: '../docs',
          routeBasePath: 'docs',
          sidebarPath: './sidebars.js',
          editUrl: 'https://github.com/kulehq/kule/edit/main/',
          showLastUpdateTime: false,
        },
        blog: false,
        theme: { customCss: './src/css/custom.css' },
      }),
    ],
  ],

  themeConfig:
    /** @type {import('@docusaurus/preset-classic').ThemeConfig} */
    ({
      image: 'img/social-preview.png',
      colorMode: { respectPrefersColorScheme: true },
      announcementBar: {
        id: 'preview',
        content: tr
          ? 'Kule bir <b>önizleme</b> (preview) sürümüdür: macOS ve Linux; imzalı bir sürüm henüz yok.'
          : 'Kule is a <b>preview</b>: macOS and Linux, no signed release yet.',
        isCloseable: true,
      },
      navbar: {
        title: 'Kule',
        logo: { alt: 'Kule', src: 'img/kule-mark.svg' },
        items: [
          { type: 'docSidebar', sidebarId: 'docs', position: 'left', label: 'Docs' },
          { to: '/docs/cli', label: 'CLI reference', position: 'left' },
          { to: '/docs/getting-started/install', label: 'Install', position: 'left' },
          { type: 'localeDropdown', position: 'right' },
          { href: 'https://github.com/kulehq/kule', label: 'GitHub', position: 'right' },
        ],
      },
      footer: {
        style: 'dark',
        links: [
          {
            title: 'Start',
            items: [
              { label: 'Install', to: '/docs/getting-started/install' },
              { label: 'Quick start', to: '/docs/getting-started/quick-start' },
              { label: 'How Kule works', to: '/docs/concepts/how-kule-works' },
            ],
          },
          {
            title: 'Reference',
            items: [
              { label: 'CLI', to: '/docs/cli' },
              { label: 'Files and settings', to: '/docs/reference/files' },
              { label: 'Extensions', to: '/docs/extensions' },
            ],
          },
          {
            title: 'Project',
            items: [
              { label: 'GitHub', href: 'https://github.com/kulehq/kule' },
              { label: 'Changelog', href: 'https://github.com/kulehq/kule/blob/main/CHANGELOG.md' },
              { label: 'Report a vulnerability', href: 'https://github.com/kulehq/kule/security/policy' },
            ],
          },
        ],
        copyright: tr
          ? 'Kule, Apache-2.0 lisansıyla açık kaynaktır.'
          : 'Kule is open source under the Apache-2.0 license.',
      },
      prism: {
        theme: prismThemes.github,
        darkTheme: prismThemes.dracula,
        additionalLanguages: ['bash', 'toml', 'json', 'rust'],
      },
    }),
};

export default config;
