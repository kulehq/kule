"""Vendors the window's file icons and syntax colouring.

Downloads two pinned npm packages, checks their sha256, and writes into
crates/kule-desktop/ui/vendor:

- material-icons/: the SVGs of Material Icon Theme (MIT) for the file kinds
  and folder names below, and icons.js, which maps names to them;
- prism/prism.js: Prism's core (MIT) and the languages below, in one file.

Run it again after changing a list or a version:

    python3 -I scripts/vendor-ui-assets.py
"""

import hashlib
import io
import json
import re
import shutil
import tarfile
import urllib.request
from pathlib import Path

ICONS = ("material-icon-theme", "5.39.0", "73bd2c6bfeea79ec1f7122445b7ff808c46e8b30097a11ae779ff683d7ad1ba4")
PRISM = ("prismjs", "1.30.0", "ac16a9106a28c53b6a6313993c816a3a02525fdfefd7614fc1703df915a9fc11")

EXTENSIONS = """
rs ts tsx mts cts d.ts spec.ts test.ts spec.tsx test.tsx js jsx mjs cjs spec.js test.js json jsonc json5
xml xsd xsl xslt plist md mdx markdown py pyi ipynb cs csproj sln fs fsproj vb go java kt kts swift
c h cpp cc cxx hpp m mm rb php html htm css scss sass less vue svelte astro sh bash zsh fish ps1 bat cmd
sql prisma toml yaml yml ini cfg conf properties env lock txt log csv tsv xlsx xls docx doc pptx pdf
zip gz tar tgz 7z rar png jpg jpeg gif webp svg ico bmp tiff mp4 mov webm mp3 wav flac ttf woff woff2
otf eot proto graphql gql dart lua r jl ex exs erl hrl hs ml clj scala tf hcl wasm diff patch key pem
crt cert gradle sbt cabal nix zig odin v elm purs re rst tex bib epub
""".split()

FILE_NAMES = """
package.json package-lock.json yarn.lock pnpm-lock.yaml tsconfig.json jsconfig.json dockerfile
containerfile docker-compose.yml docker-compose.yaml compose.yaml compose.yml readme.md readme
readme.txt license license.md license.txt licence changelog.md changelog .gitignore .gitattributes
.gitmodules .gitkeep makefile gnumakefile .editorconfig .env .env.local .env.example .npmrc .nvmrc
.prettierrc .prettierignore .eslintrc .eslintrc.json .eslintrc.js eslint.config.js eslint.config.mjs
vite.config.ts vite.config.js webpack.config.js claude.md agents.md go.mod go.sum requirements.txt
pyproject.toml setup.py pipfile gemfile gemfile.lock rust-toolchain.toml rust-toolchain .dockerignore
security.md contributing.md code_of_conduct.md cargo.toml cargo.lock build.gradle settings.gradle
pom.xml justfile procfile vercel.json netlify.toml .travis.yml .gitlab-ci.yml jenkinsfile
""".split()

FOLDERS = """
src source lib libs crates packages docs doc test tests __tests__ spec specs .git .github .vscode .idea
node_modules assets images img icons public static dist build out target bin scripts script tools config
configs .config examples example samples data db database migrations ui components styles css js api
server client app web site www vendor include templates fixtures i18n locales logs tmp temp cache .cache
docker .devcontainer infra deploy ci .circleci benches bench fonts media video audio home models views
controllers routes middleware hooks utils helpers types interfaces services store plugins android ios
""".split()

# Prism's languages, each after the ones it builds on.
LANGUAGES = """
markup css clike javascript typescript jsx tsx json json5 rust python go csharp java kotlin swift c cpp
ruby markup-templating php bash shell-session powershell batch sql toml yaml ini properties markdown
docker makefile diff lua dart scala graphql protobuf hcl r haskell elixir erlang clojure ocaml zig
latex groovy fsharp objectivec scss less
""".split()

ROOT = Path(__file__).resolve().parent.parent
VENDOR = ROOT / "crates/kule-desktop/ui/vendor"


def package(name, version, sha256):
    url = f"https://registry.npmjs.org/{name}/-/{name}-{version}.tgz"
    data = urllib.request.urlopen(url, timeout=60).read()
    got = hashlib.sha256(data).hexdigest()
    if got != sha256:
        raise SystemExit(f"{name} {version}: sha256 {got}, expected {sha256}")
    return tarfile.open(fileobj=io.BytesIO(data), mode="r:gz")


def read(tar, path):
    return tar.extractfile(f"package/{path}").read()


SVG_REFUSED = re.compile(rb"<script|\bon[a-z]+\s*=|<foreignObject|xlink:href\s*=\s*\"(?!#)|href\s*=\s*\"(?!#)", re.I)


def icons():
    tar = package(*ICONS)
    theme = json.loads(read(tar, "dist/material-icons.json"))
    light = theme.get("light", {})
    used = {theme["file"], theme["folder"], theme["folderExpanded"]}

    def pick(table, keys):
        out = {}
        for key in keys:
            icon = table.get(key)
            if icon:
                out[key] = icon
                used.add(icon)
        return out

    extensions = pick(theme["fileExtensions"], EXTENSIONS)
    # Kinds VS Code knows by language rather than by extension.
    by_language = {"rs": "rust", "ts": "typescript", "js": "javascript", "py": "python", "go": "go",
                   "java": "java", "cs": "csharp", "c": "c", "cpp": "cpp", "html": "html", "css": "css",
                   "json": "json", "xml": "xml", "md": "markdown", "sh": "shellscript", "yaml": "yaml",
                   "yml": "yaml", "sql": "sql", "rb": "ruby", "php": "php", "swift": "swift", "lua": "lua"}
    for ext, language in by_language.items():
        icon = theme["languageIds"].get(language)
        if ext not in extensions and icon:
            extensions[ext] = icon
            used.add(icon)
    names = pick(theme["fileNames"], FILE_NAMES)
    folders = pick(theme["folderNames"], FOLDERS)
    folders_open = pick(theme["folderNamesExpanded"], FOLDERS)
    # Light themes swap some icons for ones drawn for a light background.
    swaps = {}
    for table in ("fileExtensions", "fileNames", "folderNames", "folderNamesExpanded"):
        for key, icon in light.get(table, {}).items():
            if key in extensions or key in names or key in folders or key in folders_open:
                swaps[f"{table}:{key}"] = icon
                used.add(icon)

    out = VENDOR / "material-icons"
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    for icon in sorted(used):
        path = theme["iconDefinitions"][icon]["iconPath"].removeprefix("./../")
        svg = read(tar, path)
        if SVG_REFUSED.search(svg):
            raise SystemExit(f"{path}: an SVG with script, a handler or an outside link")
        (out / f"{icon}.svg").write_bytes(svg)
    (out / "LICENSE").write_bytes(read(tar, "LICENSE"))
    table = {
        "file": theme["file"],
        "folder": theme["folder"],
        "folderOpen": theme["folderExpanded"],
        "extensions": dict(sorted(extensions.items())),
        "names": dict(sorted(names.items())),
        "folders": dict(sorted(folders.items())),
        "foldersOpen": dict(sorted(folders_open.items())),
        "light": dict(sorted(swaps.items())),
    }
    (out / "icons.js").write_text(
        f"// Generated by scripts/vendor-ui-assets.py from {ICONS[0]} {ICONS[1]} (MIT, see LICENSE).\n"
        f"// Which icon a file or folder gets, as VS Code picks it.\n"
        f'"use strict";\n\nconst MATERIAL_ICONS = {json.dumps(table, indent=1)};\n'
    )
    print(f"{len(used)} icons")


def prism():
    tar = package(*PRISM)
    parts = [read(tar, "components/prism-core.min.js")]
    for language in LANGUAGES:
        parts.append(read(tar, f"components/prism-{language}.min.js"))
    out = VENDOR / "prism"
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True)
    header = (f"/* Generated by scripts/vendor-ui-assets.py from {PRISM[0]} {PRISM[1]} (MIT, see LICENSE):\n"
              f"   the core and {' '.join(LANGUAGES)}. */\n").encode()
    (out / "prism.js").write_bytes(header + b"\n".join(parts) + b"\n")
    (out / "LICENSE").write_bytes(read(tar, "LICENSE"))
    print(f"prism with {len(LANGUAGES)} languages")


icons()
prism()
