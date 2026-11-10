#!/usr/bin/env python3
import re
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parents[1]
markdown = sorted(root.glob("*.md")) + sorted((root / "docs").rglob("*.md"))
prose = markdown + sorted((root / "src").rglob("*.rs")) + sorted((root / "web").rglob("*.js"))
for path in prose:
    if chr(0x2014) in path.read_text():
        raise SystemExit(f"Forbidden punctuation: {path.relative_to(root)}")
for path in markdown:
    for href in re.findall(r"\[[^]]+\]\(([^)\s]+)\)", path.read_text()):
        if href.startswith(("https://", "http://", "mailto:", "#")):
            continue
        relative = href.split("#", 1)[0]
        if relative and not (path.parent / relative).exists():
            raise SystemExit(f"Missing local link: {path.relative_to(root)} -> {href}")

destination = root / "target/docs"
destination.mkdir(parents=True, exist_ok=True)
diagrams = sorted((root / "docs/diagrams").glob("*.dot"))
if not shutil.which("dot") or len(diagrams) < 12:
    raise SystemExit("Graphviz and at least twelve checked diagram sources are required")
for path in diagrams:
    published = path.with_suffix(".svg")
    if "<svg" not in published.read_text():
        raise SystemExit(f"Missing published SVG for {path.name}")
    subprocess.run(["dot", "-Tsvg", str(path), "-o", str(destination / published.name)], check=True, timeout=10)
print(f"{len(markdown)} Markdown files and {len(diagrams)} rendered diagrams checked")
