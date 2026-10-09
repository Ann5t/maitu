#!/usr/bin/env python3
"""Check that the authentication heading asserted by the browser spec is still rendered.

`tests/browser/security.spec.js` asserts the text of the authentication page `h1`.
That heading does not come from `assets/`: `src/security.rs` passes it as the first
argument of `auth_page(title, ...)`, which renders `<h1>(title)</h1>`. Renaming the
heading in Rust without updating the spec therefore stays invisible until the
Playwright spec runs near the end of `scripts/quality-gate.sh`, where it fails with a
message that does not name the Rust literal. This check compares the two file-local
literals directly and fails within the first seconds of the gate instead.

The check is deliberately narrow and literal-based: it reads the first string
argument of every `auth_page(...)` call and requires every `h1` text assertion in
`tests/browser/security.spec.js` to be a substring of one of them. It does not render
a page, does not follow redirects and does not claim to prove what the browser sees.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


REPOSITORY = Path(__file__).resolve().parent.parent
SECURITY_SOURCE = REPOSITORY / "src" / "security.rs"
BROWSER_SPEC = REPOSITORY / "tests" / "browser" / "security.spec.js"

AUTH_PAGE_TITLE = re.compile(r'auth_page\(\s*"((?:[^"\\]|\\.)*)"')
H1_ASSERTION = re.compile(r"locator\('h1'\)\)\s*\.\s*toContainText\(\s*'((?:[^'\\]|\\.)*)'")


def unescape(literal: str) -> str:
    return literal.replace("\\'", "'").replace('\\"', '"').replace("\\\\", "\\")


def main() -> int:
    for path in (SECURITY_SOURCE, BROWSER_SPEC):
        if not path.is_file():
            print(f"认证页文案契约检查失败：找不到 {path.relative_to(REPOSITORY)}")
            return 1

    source = SECURITY_SOURCE.read_text(encoding="utf-8")
    spec = BROWSER_SPEC.read_text(encoding="utf-8")

    titles = [unescape(match) for match in AUTH_PAGE_TITLE.findall(source)]
    if not titles:
        print("认证页文案契约检查失败：src/security.rs 里没有解析到 auth_page 标题字面量；请确认解析规则仍与代码一致。")
        return 1

    assertions = [unescape(match) for match in H1_ASSERTION.findall(spec)]
    if not assertions:
        print(
            "认证页文案契约检查失败："
            f"{BROWSER_SPEC.relative_to(REPOSITORY)} 里没有解析到 h1 文案断言；请确认解析规则仍与测试一致。"
        )
        return 1

    missing = [text for text in assertions if not any(text in title for title in titles)]
    if missing:
        print("认证页文案契约检查失败：浏览器断言的字样不再由认证页渲染。")
        for text in missing:
            print(f"  断言 {text!r}：src/security.rs 的 auth_page 标题没有包含它。")
        print("  src/security.rs 现有 auth_page 标题：" + "、".join(repr(title) for title in titles))
        print(f"  断言来自 {BROWSER_SPEC.relative_to(REPOSITORY)}；改名时这两处必须一起更新。")
        return 1

    print(f"认证页文案契约检查通过：auth_page 标题 {len(titles)} 条，h1 断言 {len(assertions)} 条")
    return 0


if __name__ == "__main__":
    sys.exit(main())