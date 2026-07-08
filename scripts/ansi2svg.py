#!/usr/bin/env python3
"""Convert ANSI-escaped terminal output to a styled SVG for GitHub README embedding."""

import sys
import re
import html

# ── ANSI color map (basic 16 + 256 ignored → mapped to nearest theme color) ──
PALETTE = {
    # Standard colors
    "30": "#6c7086",  # black  → muted
    "31": "#f38ba8",  # red
    "32": "#a6e3a1",  # green  (OPEN badge)
    "33": "#f9e2af",  # yellow (UNKNOWN badge)
    "34": "#89b4fa",  # blue   (GUARDED badge)
    "35": "#cba6f7",  # magenta
    "36": "#94e2d5",  # cyan
    "37": "#cdd6f4",  # white
    # Bright
    "1;32": "#a6e3a1",  # bold green
    "32;1": "#a6e3a1",
    "1;33": "#f9e2af",
    "1;34": "#89b4fa",
    "1;37": "#ffffff",
}
DEFAULT_FG = "#cdd6f4"
BG_COLOR   = "#1e1e2e"
FONT       = "JetBrains Mono, Cascadia Code, Fira Code, Consolas, monospace"
FONT_SIZE  = 13
LINE_H     = 20
PAD_X      = 18
PAD_Y      = 14
WIDTH      = 820


def parse_ansi(text: str):
    """Yield (fg_color | None, bold, text_chunk) tuples."""
    # ESC = \x1b
    token_re = re.compile(r'\x1b\[([0-9;]*)m|([^\x1b]+)', re.DOTALL)
    fg = None
    bold = False
    for m in token_re.finditer(text):
        code_group, text_group = m.group(1), m.group(2)
        if text_group is not None:
            if text_group:
                yield (fg, bold, text_group)
        else:
            code = code_group.strip()
            if code in ("", "0"):
                fg = None
                bold = False
            elif code == "1":
                bold = True
            else:
                # Try bold variants
                if "1" in code.split(";"):
                    bold = True
                    remaining = ";".join(c for c in code.split(";") if c != "1")
                    fg = PALETTE.get(remaining, PALETTE.get(code, fg))
                else:
                    fg = PALETTE.get(code, fg)


def spans_for_line(line: str) -> str:
    """Convert a single line of ANSI text to SVG <tspan> elements."""
    out = []
    for (color, bold, chunk) in parse_ansi(line):
        chunk_esc = html.escape(chunk)
        styles = []
        if color:
            styles.append(f"fill:{color}")
        if bold:
            styles.append("font-weight:bold")
        if styles:
            out.append(f'<tspan style="{";".join(styles)}">{chunk_esc}</tspan>')
        else:
            out.append(f'<tspan>{chunk_esc}</tspan>')
    return "".join(out)


def render_svg(lines: list[str], title: str = "") -> str:
    text_rows = []
    for line in lines:
        text_rows.append(spans_for_line(line))

    n_lines = len(text_rows)
    height  = PAD_Y * 2 + LINE_H * n_lines + (28 if title else 0)
    title_offset = 28 if title else 0

    rows_svg = []
    for i, row in enumerate(text_rows):
        y = PAD_Y + title_offset + i * LINE_H + LINE_H - 4
        rows_svg.append(
            f'  <text x="{PAD_X}" y="{y}" xml:space="preserve">{row}</text>'
        )

    title_el = ""
    if title:
        title_el = (
            f'  <text x="{WIDTH // 2}" y="{PAD_Y + 14}" '
            f'text-anchor="middle" style="fill:#7f849c;font-size:11px">'
            f'{html.escape(title)}</text>\n'
        )

    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{height}"
     viewBox="0 0 {WIDTH} {height}">
  <style>
    text {{
      font-family: {FONT};
      font-size: {FONT_SIZE}px;
      fill: {DEFAULT_FG};
      white-space: pre;
    }}
  </style>
  <rect width="{WIDTH}" height="{height}" rx="8" ry="8" fill="{BG_COLOR}"/>
{title_el}{"".join(r + chr(10) for r in rows_svg)}</svg>
"""


if __name__ == "__main__":
    title = sys.argv[1] if len(sys.argv) > 1 else ""
    raw = sys.stdin.buffer.read().decode("utf-8", errors="replace")
    lines = raw.splitlines()
    # Trim trailing empty lines
    while lines and not lines[-1].strip():
        lines.pop()
    print(render_svg(lines, title))
