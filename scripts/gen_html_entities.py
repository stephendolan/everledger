#!/usr/bin/env python3
"""Generate Python HTML5 unescape tables for the native catalogue parser."""
import html
import json
from pathlib import Path

out = Path(__file__).resolve().parents[1] / 'src/generated/html_entities.json'
out.write_text(json.dumps({'entities': html.entities.html5,
                           'invalid_refs': {str(k): v for k, v in html._invalid_charrefs.items()},
                           'invalid_points': sorted(html._invalid_codepoints)},
                          ensure_ascii=False, separators=(',', ':')) + '\n')
