import React from 'react';
import Layout from '@theme/Layout';
import Link from '@docusaurus/Link';
import CodeBlock from '@theme/CodeBlock';
import Translate, { translate } from '@docusaurus/Translate';

const loop = [
  translate({ id: 'home.loop.task', message: 'make a task' }),
  translate({ id: 'home.loop.prompt', message: 'give the agent a prompt' }),
  translate({ id: 'home.loop.watch', message: 'watch' }),
  translate({ id: 'home.loop.review', message: 'review' }),
  translate({ id: 'home.loop.commit', message: 'commit or roll back' }),
];

const features = [
  {
    title: translate({ id: 'home.f.copy.title', message: 'Work happens on a copy' }),
    text: translate({
      id: 'home.f.copy.text',
      message:
        'Every task is a copy-on-write copy of your folder. The agent changes the copy; your folder changes only when you commit, and undo puts it back.',
    }),
  },
  {
    title: translate({ id: 'home.f.sandbox.title', message: 'A sandbox around every command' }),
    text: translate({
      id: 'home.f.sandbox.text',
      message:
        'Commands may write only to the copy. Your credentials, your own folders and the projects beside the workspace stay out of reach.',
    }),
  },
  {
    title: translate({ id: 'home.f.policy.title', message: 'Judged by what it did' }),
    text: translate({
      id: 'home.f.policy.text',
      message:
        'A policy looks at the effects of the work: which files, which git refs, which hosts. Ordinary edits pass, risky ones wait for you, git hooks are denied.',
    }),
  },
  {
    title: translate({ id: 'home.f.network.title', message: 'Every connection goes through a proxy' }),
    text: translate({
      id: 'home.f.network.text',
      message:
        'Package registries pass, the agent reaches its own API, and anything else waits for your answer. Every connection is logged.',
    }),
  },
  {
    title: translate({ id: 'home.f.secrets.title', message: 'Secrets the agent never sees' }),
    text: translate({
      id: 'home.f.secrets.text',
      message:
        'The agent sees a placeholder; the proxy puts your API key into requests to the hosts you named, and nowhere else.',
    }),
  },
  {
    title: translate({ id: 'home.f.any.title', message: 'Any folder, not only code' }),
    text: translate({
      id: 'home.f.any.text',
      message:
        'Notes, documents, photos and data work the same way. Kule needs no git, and reads .git as a repository when there is one.',
    }),
  },
];

const session = `kule new ~/code/app                  # make a task: a copy of the folder
kule run 1a2b3c4d "Fix the failing test"   # an agent works in the copy, in the sandbox
kule status 1a2b3c4d                 # what changed, and what the policy says
kule commit 1a2b3c4d                 # apply it to the folder, keeping a checkpoint
kule undo 1a2b3c4d                   # changed your mind? put the folder back`;

export default function Home() {
  return (
    <Layout
      title={translate({ id: 'home.title', message: 'A control room for AI agents' })}
      description={translate({
        id: 'home.description',
        message:
          'Kule runs AI agents on a sandboxed copy of your folder and lets you review what they did before anything reaches it.',
      })}>
      <header className="kule-hero">
        <h1>Kule</h1>
        <p>
          <Translate id="home.hero">
            A control room for AI agents that work on your folders. Agents do the work in a
            sandboxed copy; you watch, approve and decide what reaches the real folder.
          </Translate>
        </p>
        <Link className="button button--primary button--lg" to="/docs/getting-started/install">
          <Translate id="home.cta.install">Install</Translate>
        </Link>
        <Link className="button button--secondary button--lg" to="/docs/getting-started/quick-start">
          <Translate id="home.cta.quickstart">Five-minute quick start</Translate>
        </Link>
        <Link className="button button--secondary button--lg" to="/docs/intro-video">
          <Translate id="home.cta.video">Watch the tour</Translate>
        </Link>
        <div className="kule-loop" aria-label="The loop">
          {loop.map((step, i) => (
            <React.Fragment key={step}>
              {i > 0 && <span className="arrow">→</span>}
              <span>{step}</span>
            </React.Fragment>
          ))}
        </div>
      </header>
      <main>
        <section className="kule-features container">
          <div className="row">
            {features.map((f) => (
              <div key={f.title} className="col col--4">
                <h3>{f.title}</h3>
                <p>{f.text}</p>
              </div>
            ))}
          </div>
        </section>
        <section className="kule-terminal">
          <h2>
            <Translate id="home.terminal.title">The same from a terminal</Translate>
          </h2>
          <CodeBlock language="sh">{session}</CodeBlock>
          <p>
            <Translate id="home.terminal.more">Every command is explained in the</Translate>{' '}
            <Link to="/docs/cli">
              <Translate id="home.terminal.link">CLI reference</Translate>
            </Link>
            .
          </p>
        </section>
      </main>
    </Layout>
  );
}
