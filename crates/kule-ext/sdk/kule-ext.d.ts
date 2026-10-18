// Types for Kule's extension SDK: the `kule` object every frame of an
// extension has once it loads `_kule/sdk.js`. Every call is a request to
// Kule, answered or refused by the permissions the person approved; a
// refused call rejects with an Error that says why.

declare namespace Kule {
  type Permission =
    | "tasks.read"
    | "events.read"
    | "questions.read"
    | "review.read"
    | "review.annotate"
    | "files.read"
    | "storage"
    | "notify"
    | "net";

  interface Task {
    id: string;
    label: string | null;
    workspace: string;
    created_unix: number;
    /** Uncommitted changes in the task's copy. */
    changes: number;
    /** What the policy says about them: "allow", "ask" or "deny". */
    verdict: "allow" | "ask" | "deny" | null;
    /** An agent is at work. */
    running: boolean;
    /** Questions the agent waits on. */
    waiting: number;
  }

  interface Effect {
    path: string;
    change: "created" | "modified" | "deleted";
    [more: string]: unknown;
  }

  interface Review {
    /** The review's digest: it changes whenever the changes do. */
    digest: string;
    effects: { effects: Effect[]; moves?: unknown[]; git?: unknown };
    ruling: { judgements: { subject: string; verdict: "allow" | "ask" | "deny"; rules: string[] }[] };
    outline?: unknown[];
  }

  interface Note {
    path?: string;
    level?: "info" | "warn";
    /** At most 500 characters, shown as plain text. */
    text: string;
  }

  interface Fetched {
    status: number;
    headers: Record<string, string>;
    body: string;
  }

  type TaskChange = "created" | "committed" | "rolled_back" | "undone" | "removed";

  interface Events {
    /** An agent's events while it runs (events.read). */
    run: { task: string; seq: number; event: unknown };
    /** Something happened to a task as a whole (events.read). */
    task: { task: string; change: TaskChange };
    /** The questions waiting for the person (questions.read). */
    questions: unknown[];
    /** A task's review was drawn, with new changes (review.read). */
    review: { task: string; digest: string };
    /** The person chose another task (or none). */
    "task-selected": { task: string | null };
    /** Kule's colours changed; the SDK already set the --kule-* CSS variables. */
    theme: { scheme: "light" | "dark"; colors: Record<string, string> };
    /** The person changed one of the extension's settings. */
    config: { key: string; value: unknown };
  }

  interface Sdk {
    /** Resolves once Kule has connected the frame. */
    readonly ready: Promise<void>;
    extension: { id: string; name: string; version: string } | null;
    /** `{ main: true }` in the main script's frame; `{ panel, task }` in a panel. */
    context: { main?: boolean; panel?: string; task?: string | null };
    commands: {
      /** Runs `run` when the person picks the manifest's command `id`. */
      register(id: string, run: (args: unknown) => unknown): void;
      /** One of this extension's commands (`ext.<id>.<command>`), or one of Kule's that only moves the view. */
      execute(id: string, args?: unknown): Promise<null>;
    };
    events: {
      on<K extends keyof Events>(name: K, handler: (data: Events[K]) => void): () => void;
    };
    tasks: {
      list(): Promise<Task[]>;
      get(task: string): Promise<Task>;
    };
    review: {
      get(task: string): Promise<Review>;
      /** Replaces this extension's notes on the task's review; [] takes them off. */
      annotate(task: string, notes: Note[]): Promise<null>;
    };
    files: {
      list(task: string, path?: string): Promise<{ entries: { name: string; kind: "file" | "dir" | "link"; size: number }[]; cut: boolean }>;
      read(task: string, path: string): Promise<{ kind: string; text?: string; cut: boolean; size: number }>;
    };
    storage: {
      get(): Promise<unknown>;
      /** At most 1 MiB of JSON. */
      set(value: unknown): Promise<null>;
    };
    config: {
      get(): Promise<Record<string, unknown>>;
    };
    ui: {
      /** Five a minute at most. */
      notify(title: string, body?: string): Promise<null>;
    };
    net: {
      /** https to the manifest's hosts only; no redirects or cookies; 60 a minute. */
      fetch(url: string, init?: { method?: string; headers?: Record<string, string>; body?: string }): Promise<Fetched>;
    };
  }
}

declare const kule: Kule.Sdk;
