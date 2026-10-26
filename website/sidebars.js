// The order of the docs. Every page in ../docs is listed here by its id
// (its path without .md), so a page left out shows up as a broken build.
// @ts-check

/** @type {import('@docusaurus/plugin-content-docs').SidebarsConfig} */
const sidebars = {
  docs: [
    'index',
    'intro-video',
    {
      type: 'category',
      label: 'Getting started',
      collapsed: false,
      items: [
        'getting-started/install',
        'getting-started/quick-start',
        'getting-started/first-task-in-the-app',
      ],
    },
    'testing',
    {
      type: 'category',
      label: 'Concepts',
      items: [
        'concepts/how-kule-works',
        'concepts/tasks-and-copies',
        'concepts/sandbox',
        'concepts/policy',
        'concepts/network-and-questions',
        'concepts/review-commit-undo',
        'concepts/sync-and-merging',
        'concepts/git',
        'concepts/home-layer',
        'concepts/secrets',
        'concepts/budgets-and-limits',
        'concepts/groups',
      ],
    },
    {
      type: 'category',
      label: 'The desktop app',
      items: [
        'app/window',
        'app/live',
        'app/review',
        'app/timeline',
        'app/files-and-preview',
        'app/terminals-and-servers',
        'app/settings',
        'app/keyboard',
      ],
    },
    'using-agents',
    'extensions',
    {
      type: 'category',
      label: 'CLI reference',
      link: { type: 'doc', id: 'cli/index' },
      items: [
        {
          type: 'category',
          label: 'Tasks',
          items: ['cli/new', 'cli/list', 'cli/name', 'cli/color', 'cli/rm'],
        },
        {
          type: 'category',
          label: 'Working in a task',
          items: ['cli/run', 'cli/exec', 'cli/shell', 'cli/closed', 'cli/path', 'cli/open'],
        },
        {
          type: 'category',
          label: 'Review and history',
          items: [
            'cli/status',
            'cli/steps',
            'cli/commit',
            'cli/rollback',
            'cli/undo',
            'cli/sync',
            'cli/history',
          ],
        },
        {
          type: 'category',
          label: 'Cost and limits',
          items: ['cli/budget', 'cli/daily-budget', 'cli/limits'],
        },
        {
          type: 'category',
          label: 'Groups',
          items: ['cli/group'],
        },
        {
          type: 'category',
          label: 'Policy and access',
          items: ['cli/policy', 'cli/rules', 'cli/reveal', 'cli/isolation'],
        },
        {
          type: 'category',
          label: 'Home and secrets',
          items: ['cli/home', 'cli/secrets'],
        },
        {
          type: 'category',
          label: 'Agents and extensions',
          items: ['cli/agents', 'cli/ext'],
        },
        {
          type: 'category',
          label: 'Daemon',
          items: ['cli/daemon'],
        },
      ],
    },
    {
      type: 'category',
      label: 'Reference',
      items: ['reference/files', 'reference/environment', 'reference/policy-rules'],
    },
    'platforms',
    'troubleshooting',
    'security',
  ],
};

export default sidebars;
