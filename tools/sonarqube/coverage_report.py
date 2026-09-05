"""Normalize same-snapshot coverage and measure the exact Git feature diff."""

import re
from pathlib import Path
import xml.etree.ElementTree as ET


def relative_source(filename: str, root: Path) -> str:
    """Normalize report source paths, rejecting files outside the analyzed checkout."""
    path = Path(filename)
    if not path.is_absolute():
        path = root / path
    try:
        return path.resolve().relative_to(root.resolve()).as_posix()
    except ValueError:
        raise RuntimeError("SONAR_COVERAGE_FOREIGN_PATH") from None


def read_lcov(path: Path, root: Path) -> dict[str, dict[int, bool]]:
    """Read line hit counts from cargo-llvm-cov or c8 LCOV output."""
    coverage, current = {}, None
    for line in path.read_text().splitlines():
        if line.startswith("SF:"):
            current = relative_source(line[3:], root)
            coverage.setdefault(current, {})
        elif line.startswith("DA:") and current:
            number, hits, *_ = line[3:].split(",")
            previous = coverage[current].get(int(number), False)
            coverage[current][int(number)] = previous or int(hits) > 0
    if not coverage:
        raise RuntimeError("SONAR_COVERAGE_EMPTY")
    return coverage


def read_python(path: Path, root: Path) -> dict[str, dict[int, bool]]:
    """Read coverage.py's Cobertura line data for the scanned checkout."""
    coverage = {}
    document = ET.parse(path)
    sources = [
        Path(node.text) for node in document.findall("./sources/source") if node.text
    ]
    for node in document.findall(".//class"):
        filename = node.attrib["filename"]
        matches = [
            source / filename for source in sources if (source / filename).is_file()
        ]
        candidate = str(matches[0]) if len(matches) == 1 else filename
        name = relative_source(candidate, root)
        coverage[name] = {
            int(line.attrib["number"]): int(line.attrib["hits"]) > 0
            for line in node.findall("./lines/line")
        }
    if not coverage:
        raise RuntimeError("SONAR_COVERAGE_EMPTY")
    return coverage


def changed_coverage(
    changed: dict, coverage: dict, nonexecutable: set | None = None
) -> tuple[int, int]:
    """Count changed executable lines; absent file coverage is not zero code."""
    covered = total = 0
    for name, lines in changed.items():
        if name not in coverage:
            if name in (nonexecutable or set()):
                continue
            raise RuntimeError("SONAR_COVERAGE_MISSING: " + name)
        for number in lines.intersection(coverage[name]):
            total += 1
            covered += int(coverage[name][number])
    return covered, total


def write_generic(coverage: dict, path: Path) -> None:
    """Emit Sonar's generic coverage format with checkout-relative source paths."""
    document = ET.Element("coverage", version="1")
    for name, lines in sorted(coverage.items()):
        file_node = ET.SubElement(document, "file", path=name)
        for number, hit in sorted(lines.items()):
            ET.SubElement(
                file_node,
                "lineToCover",
                lineNumber=str(number),
                covered=str(hit).lower(),
            )
    ET.ElementTree(document).write(path, encoding="utf-8", xml_declaration=True)


def rust_declarations_only(source: str) -> bool:
    """Recognize only imports/module declarations after the Rust compiler succeeds.

    This deliberately narrow grammar rejects macros, functions, constants, inline
    modules, block comments and unfamiliar attributes rather than guessing that
    an absent LLVM file record means zero executable lines.
    """
    text = re.sub(r"//[^\n]*", "", source)
    visibility = r"(?:pub(?:\(crate\))?\s+)?"
    declaration = (
        r"(?:#\[cfg\(test\)\]\s*)?"
        + visibility
        + r"(?:mod\s+[A-Za-z_][A-Za-z_0-9]*|use\s+[A-Za-z_][A-Za-z_0-9:{},*\s]*)\s*;"
    )
    return re.fullmatch(r"\s*(?:" + declaration + r"\s*)*", text) is not None
