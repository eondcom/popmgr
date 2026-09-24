"""원고(md) → ebook-writer 템플릿 HTML. 사용: python3 build.py (이 폴더에서). PDF 는 ebook-writer 의 build.sh 로."""
import html
import pathlib
import re

import markdown

HERE = pathlib.Path(__file__).parent
TEMPLATE = pathlib.Path.home() / ".claude/skills/ebook-writer/assets/template.html"
SRC = HERE / "프롬프트사용법4-popmgr.md"
OUT = HERE / "프롬프트사용법4-popmgr.html"

TITLE = "프롬프트 사용법 4"
SUBTITLE = "Pop!_OS 노트북 관리 앱을 만들며"
KICKER = "프롬프트 사용법 시리즈"
META = "이온디 · 2026년 9월 25일 · 발화 23건(2026-09-24 하루)"
FOOTER = "프롬프트 사용법 4 — Pop!_OS 노트북 관리 앱을 만들며 · 이온디 · 코드: github.com/eondcom/popmgr"

body = markdown.markdown(SRC.read_text(encoding="utf-8"), extensions=["tables", "fenced_code"])

toc = []
n = 0


def number(m: re.Match) -> str:
    """h2 에 id 를 달고 목차 항목을 모은다."""
    global n
    n += 1
    title = m.group(1)
    toc.append(f'<li><a href="#c{n}">{title}</a></li>')
    return f'<h2 id="c{n}">{title}</h2>'


body = re.sub(r"<h2>(.*?)</h2>", number, body)

page = TEMPLATE.read_text(encoding="utf-8")
for key, val in {
    "{{TITLE}}": html.escape(TITLE),
    "{{SUBTITLE}}": html.escape(SUBTITLE),
    "{{KICKER}}": html.escape(KICKER),
    "{{META}}": html.escape(META),
    "{{TOC}}": "".join(toc),
    "{{BODY}}": body,
    "{{FOOTER}}": html.escape(FOOTER),
}.items():
    page = page.replace(key, val)
OUT.write_text(page, encoding="utf-8")
print(OUT, len(toc), "장")
