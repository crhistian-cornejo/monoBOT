#!/usr/bin/env python3
"""Clean the text of a document you wrote, as a person would have typed it.

Removes characters nobody can see (zero-width marks, hidden tag characters,
direction overrides, soft hyphens, a stray BOM), makes odd spaces plain and,
in Markdown or plain text, trims trailing spaces and runs of blank lines.
Emoji keep their joiners and code fences keep their spacing. In Word, Excel
and PowerPoint files only the invisible characters go: their XML, styles and
non-breaking spaces are left as they are.

  python clean_text.py FILE [FILE ...]        clean in place
  python clean_text.py FILE --out NEW_FILE    write the clean copy elsewhere
  python clean_text.py FILE --check           change nothing; list filler
                                              phrases and dash density

Standard library only. Prints one line per file.
"""

import argparse
import re
import sys
import zipfile
from pathlib import Path

INVISIBLE = re.compile(
    "[­͏؜ᅟᅠ឴឵᠋-᠏​‎‏"
    "‪-‮⁠-⁤⁦-⁯ㅤ﻿ﾠ￹-￻"
    "\U0001d173-\U0001d17a\U000e0100-\U000e01ef]"
)
TAGS = re.compile("(\U0001f3f4[\U000e0020-\U000e007e]+\U000e007f)|[\U000e0000-\U000e007f]")
ODD_SPACES = re.compile("[   -   ]")
ODD_SPACES_OFFICE = re.compile("[  -  ]")
LINE_SEPARATORS = re.compile("[  \u0085]")
FENCE = re.compile(r"^\s{0,3}(`{3,}|~{3,})")


def _emoji(char):
    if not char:
        return False
    code = ord(char)
    return code >= 0x1F000 or 0x2600 <= code <= 0x27BF or code == 0xFE0F


def _joiners(text):
    """Drop a ZWJ outside an emoji, and a ZWNJ outside the scripts using it."""
    out = []
    for index, char in enumerate(text):
        if char in "‌‍":
            before = text[index - 1] if index else ""
            after = text[index + 1] if index + 1 < len(text) else ""
            if char == "‍" and _emoji(before) and _emoji(after):
                out.append(char)
            elif (
                char == "‌"
                and before.isalpha()
                and after.isalpha()
                and ord(before) > 0x24F
                and ord(after) > 0x24F
            ):
                out.append(char)
            continue
        out.append(char)
    return "".join(out)


def strip_invisible(text):
    text = TAGS.sub(lambda match: match.group(1) or "", text)
    return _joiners(INVISIBLE.sub("", text))


def clean_text(text):
    text = LINE_SEPARATORS.sub("\n", strip_invisible(text).replace("\r\n", "\n").replace("\r", "\n"))
    out, fence, blank = [], None, 0
    for raw in text.split("\n"):
        match = FENCE.match(raw)
        marker = match.group(1) if match else None
        if fence:
            out.append(raw)
            if marker and marker[0] == fence[0] and len(marker) >= len(fence):
                fence = None
            continue
        if marker:
            fence, blank = marker, 0
            out.append(raw.rstrip())
            continue
        line = ODD_SPACES.sub(" ", raw).rstrip()
        if not line:
            blank += 1
            if blank > 1:
                continue
        else:
            blank = 0
        out.append(line)
    return "\n".join(out).strip("\n") + "\n"


OFFICE_PARTS = re.compile(r"^(word|ppt|xl|docProps)/.*\.xml$")


def clean_office(source, target):
    removed = 0
    with zipfile.ZipFile(source) as zin:
        items = [(info, zin.read(info.filename)) for info in zin.infolist()]
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as zout:
        for info, data in items:
            if OFFICE_PARTS.match(info.filename):
                xml = data.decode("utf-8")
                clean = ODD_SPACES_OFFICE.sub(" ", strip_invisible(xml))
                removed += len(xml) - len(clean)
                data = clean.encode("utf-8")
            zout.writestr(info, data)
    return removed


FILLER = [
    "cabe destacar", "cabe señalar", "es importante señalar", "es importante destacar",
    "en resumen", "en conclusión", "en definitiva", "profundizar en", "sin lugar a dudas",
    "juega un papel crucial", "juega un rol fundamental", "en el panorama actual",
    "it is worth noting", "it's worth noting", "in summary", "in conclusion", "delve into",
    "plays a crucial role", "in today's fast-paced", "a testament to", "navigate the complexities",
]


def check(text):
    lines = text.split("\n")
    found = []
    for number, line in enumerate(lines, 1):
        lower = line.lower()
        for phrase in FILLER:
            if phrase in lower:
                found.append(f"  line {number}: \"{phrase}\"")
    words = max(1, len(re.findall(r"\w+", text)))
    dashes = text.count("—")
    hidden = len(text) - len(strip_invisible(text))
    report = [f"  invisible characters: {hidden}", f"  em dashes: {dashes} ({dashes * 1000 // words} per 1000 words)"]
    return report + (["  filler phrases (rewrite them):"] + found if found else ["  filler phrases: none"])


def office_text(path):
    with zipfile.ZipFile(path) as zin:
        parts = [zin.read(name).decode("utf-8") for name in zin.namelist() if OFFICE_PARTS.match(name)]
    return "\n".join(re.sub(r"<[^>]+>", " ", part) for part in parts)


def main():
    # A Windows console is not UTF-8 by default; a report must not crash on it.
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8", errors="replace")
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("files", nargs="+", type=Path)
    parser.add_argument("--out", type=Path, help="write the clean copy here (one input file only)")
    parser.add_argument("--check", action="store_true", help="report only; change nothing")
    args = parser.parse_args()
    if args.out and len(args.files) != 1:
        parser.error("--out takes one input file")
    failed = False
    for path in args.files:
        suffix = path.suffix.lower()
        office = suffix in (".docx", ".xlsx", ".pptx")
        try:
            if args.check:
                text = office_text(path) if office else path.read_text(encoding="utf-8")
                print(f"{path}:")
                print("\n".join(check(text)))
                continue
            target = args.out or path
            if office:
                temp = target.with_name(target.name + ".tmp")
                removed = clean_office(path, temp)
                temp.replace(target)
            else:
                text = path.read_text(encoding="utf-8")
                clean = clean_text(text)
                removed = len(text) - len(clean)
                target.write_text(clean, encoding="utf-8", newline="\n")
            print(f"{target}: clean ({max(removed, 0)} characters removed)")
        except (OSError, UnicodeDecodeError, zipfile.BadZipFile) as error:
            failed = True
            print(f"{path}: could not clean ({error})", file=sys.stderr)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
