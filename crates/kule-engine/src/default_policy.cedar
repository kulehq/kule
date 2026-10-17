// Kule's built-in policy. Each effect is asked twice: may it pass on its own
// (Action "auto"), and if not, may a person approve it (Action "ask").
// A forbid always wins over a permit.
//
// Attributes of resource (Kule::Effect):
//   kind          "fs.create" | "fs.modify" | "fs.delete" |
//                 "git.ref" | "git.config" | "git.hook" | "git.other"
//   path, name    workspace path and its last component ("" for git.ref)
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
    "package.json", "package-lock.json", "npm-shrinkwrap.json", "yarn.lock", "pnpm-lock.yaml", "bun.lockb",
    "Cargo.toml", "Cargo.lock",
    "Directory.Packages.props", "Directory.Build.props", "Directory.Build.targets", "NuGet.Config", "nuget.config", "packages.lock.json", "global.json",
    "go.mod", "go.sum",
    "pyproject.toml", "setup.py", "setup.cfg", "poetry.lock", "uv.lock", "Pipfile", "Pipfile.lock",
    "Gemfile", "Gemfile.lock", "build.gradle", "build.gradle.kts", "settings.gradle", "pom.xml"
  ].contains(resource.name)
};

@id("project-files")
forbid (principal, action == Kule::Action::"auto", resource)
when { resource.kind like "fs.*" && (resource.name like "*.csproj" || resource.name like "*.fsproj" || resource.name like "requirements*.txt") };

// CI, editor tasks, dev containers, direnv and Kule's own rules run code or
// change what is checked.
@id("automation")
forbid (principal, action == Kule::Action::"auto", resource)
when {
  resource.kind like "fs.*" &&
  (resource.path like ".github/*" || resource.path like ".gitlab-ci*" || resource.path like ".circleci/*" ||
   resource.path like ".devcontainer/*" || resource.path like ".vscode/*" || resource.path like ".idea/*" ||
   resource.path like ".kule/*" || resource.path == ".envrc" || resource.path == ".gitattributes" ||
   resource.path == ".gitmodules")
};

@id("mass-delete")
forbid (principal, action == Kule::Action::"auto", resource)
when { context.deletes > 200 };

// Outside a git repository nothing but the task's own undo brings a deleted
// file back, and that goes with the task: more than a few deletions wait.
@id("deletes-outside-git")
forbid (principal, action == Kule::Action::"auto", resource)
when { !context.repository && context.deletes > 20 };

// Git: moving a branch forward and creating one pass on their own; rewriting
// history, deleting a branch or switching HEAD wait for a person.
@id("branch-forward")
permit (principal, action == Kule::Action::"auto", resource)
when { resource.kind == "git.ref" && resource.change == "modified" && resource.fast_forward && resource.ref != "HEAD" };

@id("branch-created")
permit (principal, action == Kule::Action::"auto", resource)
when { resource.kind == "git.ref" && resource.change == "created" };

// Network: package registries and model APIs over HTTPS. Anything else, git
// hosts included (a push cannot be told apart from a fetch inside TLS),
// waits for a person.
@id("package-registries")
permit (principal, action == Kule::Action::"auto", resource)
when {
  resource.kind == "net.connect" && resource.port == 443 &&
  [
    "registry.npmjs.org", "registry.yarnpkg.com",
    "crates.io", "index.crates.io", "static.crates.io", "static.rust-lang.org",
    "api.nuget.org", "globalcdn.nuget.org",
    "pypi.org", "files.pythonhosted.org",
    "proxy.golang.org", "sum.golang.org",
    "rubygems.org", "repo.maven.apache.org", "repo1.maven.org"
  ].contains(resource.host)
};

@id("model-apis")
permit (principal, action == Kule::Action::"auto", resource)
when {
  resource.kind == "net.connect" && resource.port == 443 &&
  ["api.anthropic.com", "api.openai.com", "generativelanguage.googleapis.com"].contains(resource.host)
};

// Hooks run on the next git command, as the person who runs it.
@id("git-hooks")
forbid (principal, action, resource)
when { resource.kind == "git.hook" };
