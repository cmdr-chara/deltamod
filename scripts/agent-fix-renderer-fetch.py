from pathlib import Path

path = Path("tests/e2e/community-smoke.spec.js")
text = path.read_text(encoding="utf-8")
old = """        await window.evaluate(() => {\n            window.fetch = window.__deltamodOriginalRendererFetch;\n            delete window.__deltamodOriginalRendererFetch;\n        });\n\n"""
new = """        expect(await window.evaluate(() => typeof window.fetch)).toBe('function');\n\n"""

if text.count(old) != 1:
    raise SystemExit(f"expected exactly one stale renderer-fetch teardown, found {text.count(old)}")

text = text.replace(old, new, 1)
if "__deltamodOriginalRendererFetch" in text:
    raise SystemExit("obsolete renderer fetch fixture reference remains after patch")

path.write_text(text, encoding="utf-8")
