// Kule's built-in policy. Each effect is asked twice: may it pass on its own
// (Action "auto"), and if not, may a person approve it (Action "ask").
// A forbid always wins over a permit.
//
// Attributes of resource (Kule::Effect):
//   kind          "fs.create" | "fs.modify" | "fs.delete" |
//                 "git.ref" | "git.config" | "git.hook" | "git.staged" | "git.other"
//   path, name    workspace path and its last component ("" for git.ref)
//   folded_path, folded_name
//                 the same in lower case: a case-insensitive disk (macOS's
//                 by default) and the tools reading it see them so
//   parts         the folded path's components, to match a folder anywhere
//   entry         "file" | "dir" | "symlink" for fs.*, "" otherwise
//   change        "created" | "modified" | "deleted"
//   ref           reference name for git.ref, e.g. "refs/heads/main"
//   fast_forward  for git.ref: the new value keeps the old one in its history
//   commits       for git.ref: commits added
//   host, port    for net.connect: where a command wants to connect
// Context: deletes (paths deleted in the transaction; a move is not one),
//   files (paths changed), repository (the workspace is a git repository).
//
// Network connections (kind "net.connect") are judged while the command
// waits, so for now only "auto" lets one through; one that would wait for a
// person is refused.

// Anything may be put to a person unless a forbid says otherwise.
@id("ask-by-default")
permit (principal, action == Kule::Action::"ask", resource);

// Changes to files pass on their own...
@id("files")
permit (principal, action == Kule::Action::"auto", resource)
when { resource.kind like "fs.*" };

// ...except the ones that decide what gets installed, built or run.
@id("manifests-and-lockfiles")
forbid (principal, action == Kule::Action::"auto", resource)
when {
  resource.kind like "fs.*" &&
  [
    "package.json", "package-lock.json", "npm-shrinkwrap.json", "yarn.lock", "pnpm-lock.yaml", "bun.lockb", "bun.lock",
    "cargo.toml", "cargo.lock",
    "directory.packages.props", "directory.build.props", "directory.build.targets", "nuget.config", "packages.lock.json", "global.json",
    "go.mod", "go.sum",
    "pyproject.toml", "setup.py", "setup.cfg", "poetry.lock", "uv.lock", "pipfile", "pipfile.lock",
    "gemfile", "gemfile.lock", "build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts", "pom.xml"
  ].contains(resource.folded_name)
};

@id("project-files")
forbid (principal, action == Kule::Action::"auto", resource)
when { resource.kind like "fs.*" && (resource.folded_name like "*.csproj" || resource.folded_name like "*.fsproj" || resource.folded_name like "requirements*.txt") };

// What runs code, or steers an agent, without anyone running the project:
// CI, editor tasks, dev containers, git hooks managers, toolchain and
// package manager settings and the version pins a shell prompt reads on
// entering a folder, build scripts an editor runs on opening, an agent's
// settings, instructions and servers, and Kule's own rules. In any folder,
// as the tools look for them. Kule's terminal and editor refuse a copy
// where one of these changed.
@id("automation")
forbid (principal, action == Kule::Action::"auto", resource)
when {
  resource.kind like "fs.*" &&
  (resource.parts.containsAny([
     ".github", ".circleci", ".devcontainer", ".vscode", ".idea", ".zed", ".kule",
     ".husky", ".claude", ".cursor", ".gemini", ".codex", ".cargo", ".mvn", ".yarn",
     ".buildkite", ".gitea", ".forgejo", ".woodpecker",
     ".windsurf", ".continue", ".roo", ".clinerules", ".mise"
   ]) ||
   resource.folded_path like "gradle/wrapper/*" || resource.folded_path like "*/gradle/wrapper/*" ||
   resource.folded_path like "mise/config.toml" || resource.folded_path like "*/mise/config.toml" ||
   resource.folded_path like ".config/mise.toml" || resource.folded_path like "*/.config/mise.toml" ||
   resource.folded_name like ".gitlab-ci*" ||
   resource.folded_name like "*.code-workspace" ||
   [
     ".envrc", ".gitattributes", ".gitmodules", ".npmrc", ".yarnrc", ".yarnrc.yml", ".pnpmfile.cjs",
     "build.rs", "gradlew", "gradlew.bat", "mise.toml", ".mise.toml", ".tool-versions",
     "mise.local.toml", ".mise.local.toml", ".rtx.toml",
     ".nvmrc", ".node-version", ".python-version", ".ruby-version", ".java-version",
     ".go-version", ".sdkmanrc", ".terraform-version",
     ".pre-commit-config.yaml", "lefthook.yml", ".lefthook.yml",
     ".mcp.json", "claude.md", "claude.local.md", "agents.md", "gemini.md", ".cursorrules", ".windsurfrules",
     ".devcontainer.json", "rust-toolchain", "rust-toolchain.toml", "mvnw", "mvnw.cmd", "bunfig.toml",
     "dotnet-tools.json", "azure-pipelines.yml", ".travis.yml", "jenkinsfile", "bitbucket-pipelines.yml",
     ".clinerules", "opencode.json", ".aider.conf.yml"
   ].contains(resource.folded_name))
};

// Another version control system's repository, at any depth: its config
// and hooks run on its next command, as the person who runs it (Mercurial's
// and Sapling's hooks and extensions, Jujutsu's, Darcs's and Pijul's
// settings), and Kule reads none of them as a repository, so whatever
// changes in one waits. Kule's terminal and editor refuse a copy where one
// changed.
@id("other-vcs")
forbid (principal, action == Kule::Action::"auto", resource)
when {
  resource.kind like "fs.*" &&
  resource.parts.containsAny([
    ".hg", ".sl", ".jj", "_darcs", ".pijul", ".bzr", ".svn",
    ".fslckout", "_fossil_", ".fossil-settings"
  ])
};

// A repository inside the folder: its config and a `.git` file pointing
// elsewhere decide what git runs there; its hooks run on the next command.
@id("nested-repository")
forbid (principal, action == Kule::Action::"auto", resource)
when { resource.kind like "fs.*" && resource.parts.contains(".git") };

@id("nested-git-hooks")
forbid (principal, action, resource)
when { resource.kind like "fs.*" && resource.parts.contains(".git") && resource.parts.contains("hooks") && resource.change != "deleted" };

// A link leads wherever it points, and is judged by a person.
@id("links")
forbid (principal, action == Kule::Action::"auto", resource)
when { resource.kind like "fs.*" && resource.entry == "symlink" };

@id("mass-delete")
forbid (principal, action == Kule::Action::"auto", resource)
when { context.deletes > 200 };

// Outside a git repository nothing but the task's own undo brings a deleted
// file back, and that goes with the task: more than a few deletions wait.
@id("deletes-outside-git")
forbid (principal, action == Kule::Action::"auto", resource)
when { !context.repository && context.deletes > 20 };

// Git: moving a branch forward and creating one pass on their own; rewriting
// history, deleting a branch, switching HEAD, and moving any other ref (a
// tag, a note, a replacement) wait for a person.
@id("branch-forward")
permit (principal, action == Kule::Action::"auto", resource)
when { resource.kind == "git.ref" && resource.change == "modified" && resource.fast_forward && resource.ref like "refs/heads/*" };

// A new branch alone: a tag, a note or a replacement (which makes git show
// other content in place of a commit or a file) waits.
@id("branch-created")
permit (principal, action == Kule::Action::"auto", resource)
when { resource.kind == "git.ref" && resource.change == "created" && resource.ref like "refs/heads/*" };

// Network: package registries over HTTPS. A model's API is opened by the
// agent's profile, for its own runs alone: open to every command, it would
// carry data to anyone's account there. Anything else, git hosts included
// (a push cannot be told apart from a fetch inside TLS), waits for a
// person. Go's module proxy is not here: it fetches from whatever address
// a request names.
@id("package-registries")
permit (principal, action == Kule::Action::"auto", resource)
when {
  resource.kind == "net.connect" && resource.port == 443 &&
  [
    "registry.npmjs.org", "registry.yarnpkg.com",
    "crates.io", "index.crates.io", "static.crates.io", "static.rust-lang.org",
    "api.nuget.org", "globalcdn.nuget.org",
    "pypi.org", "files.pythonhosted.org",
    "rubygems.org", "repo.maven.apache.org", "repo1.maven.org"
  ].contains(resource.host)
};

// Hooks run on the next git command, as the person who runs it.
@id("git-hooks")
forbid (principal, action, resource)
when { resource.kind == "git.hook" };
