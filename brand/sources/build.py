"""Writes the HTML each brand image is drawn from, one per size, into this
directory; render.sh turns them into the PNGs one level up."""
from pathlib import Path

HERE = Path(__file__).parent

GLYPH = '''<svg class="{cls}" viewBox="306 222 412 624" aria-hidden="true"><g fill="currentColor">
<circle cx="512" cy="262" r="30"/><rect x="336" y="318" width="352" height="112" rx="56"/>
<path d="M466 470 L558 470 L588 806 Q590 826 570 826 L454 826 Q434 826 436 806 Z"/></g></svg>'''

PLATE = '''<div class="plate" style="width:{s}px;height:{s}px;border-radius:{r}px">{glyph}</div>'''

BASE = '''<!doctype html><html><head><meta charset="utf-8">
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:opsz,wght@12..96,700;12..96,800&family=IBM+Plex+Sans:wght@400;500&family=IBM+Plex+Mono:wght@400;500&display=block">
<style>
:root {{ --ink:#131418; --coral:#ff5a3c; --white:#f4f5f7; --muted:#9aa0ac; --line:#2a2d35;
  --display:"Bricolage Grotesque", "Avenir Next", sans-serif; --body:"IBM Plex Sans", sans-serif; --mono:"IBM Plex Mono", monospace; }}
* {{ box-sizing:border-box; margin:0; }}
html, body {{ width:{w}px; height:{h}px; overflow:hidden; background:var(--ink); color:var(--white); font-family:var(--body); }}
.plate {{ background:var(--coral); color:#fff; display:grid; place-items:center; flex:none; }}
.plate svg {{ height:62%; width:auto; }}
.ghost {{ position:absolute; color:var(--coral); opacity:.09; }}
.word {{ font:800 {word}px/0.9 var(--display); letter-spacing:-0.02em; }}
.tag {{ font:400 {tag}px/1.3 var(--body); color:var(--white); }}
.mono {{ font:500 {mono}px var(--mono); color:var(--muted); letter-spacing:.02em; }}
.dot {{ color:var(--coral); }}
{extra}
</style></head><body>{body}</body></html>'''

def page(name, w, h, body, word=0, tag=0, mono=0, extra=""):
    (HERE / f"{name}.html").write_text(BASE.format(w=w, h=h, body=body, word=word, tag=tag, mono=mono, extra=extra))

# Square marks: the GitHub avatar and the LinkedIn logo. Coral to the edge;
# both sites round the corners themselves.
for name, size in [("avatar", 500), ("linkedin-logo", 400)]:
    page(name, size, size,
         f'<div class="plate" style="width:{size}px;height:{size}px">{GLYPH.format(cls="")}</div>',
         extra=".plate svg { height:58%; }")

# Social preview: what a link to the repository shows.
page("social-preview", 1280, 640, f'''
<div class="ghost" style="right:-90px;top:-40px;height:760px">{GLYPH.format(cls="g")}</div>
<main style="position:absolute;inset:0;padding:88px 96px;display:flex;flex-direction:column;justify-content:space-between">
  <div style="display:flex;align-items:center;gap:40px">
    {PLATE.format(s=150, r=34, glyph=GLYPH.format(cls=""))}
    <h1 class="word">Kule</h1>
  </div>
  <div style="display:grid;gap:22px;max-width:820px">
    <p class="tag">A control room for AI agents that work on your folders.</p>
    <p class="mono" style="color:var(--white)">sandboxed copy <span class="dot">·</span> policy <span class="dot">·</span> review <span class="dot">·</span> commit</p>
  </div>
  <p class="mono">github.com/kulehq/kule</p>
</main>''', word=150, tag=44, mono=24, extra=".ghost svg{height:100%;width:auto}")

# README header.
page("readme-header", 1280, 320, f'''
<div class="ghost" style="right:-40px;top:-120px;height:560px">{GLYPH.format(cls="g")}</div>
<main style="position:absolute;inset:0;padding:0 72px;display:flex;align-items:center;gap:44px">
  {PLATE.format(s=150, r=34, glyph=GLYPH.format(cls=""))}
  <div style="display:grid;gap:16px">
    <h1 class="word">Kule</h1>
    <p class="tag">A control room for AI agents that work on your folders.</p>
  </div>
</main>''', word=112, tag=30, mono=20, extra=".ghost svg{height:100%;width:auto}")

# LinkedIn cover: the page's logo sits over its lower left, so the words keep
# to the right.
page("linkedin-cover", 1128, 191, f'''
<div class="ghost" style="left:120px;top:-70px;height:330px;opacity:.07">{GLYPH.format(cls="g")}</div>
<main style="position:absolute;inset:0;padding:0 64px 0 400px;display:flex;flex-direction:column;justify-content:center;gap:10px">
  <p class="tag">A control room for AI agents that work on your folders.</p>
  <p class="mono">sandboxed copy <span class="dot">·</span> policy <span class="dot">·</span> review <span class="dot">·</span> commit <span style="margin-left:18px">github.com/kulehq/kule</span></p>
</main>''', tag=27, mono=15, extra=".ghost svg{height:100%;width:auto}")
print("written")
