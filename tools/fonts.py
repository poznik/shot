"""Build the fonts embedded in shot-core from the upstream Inter variable font.

    python tools/fonts.py <Inter-Variable.ttf>

Source: Inter 4.001 (https://github.com/rsms/inter), SIL Open Font License 1.1, no reserved
font name. Writes static Regular (400) and Semi Bold (600) instances at the text optical size,
subset to Latin and Cyrillic, into crates/shot-core/assets/fonts/.

ab_glyph does no OpenType shaping, so the subset carries no layout tables. Digits are mapped
straight to the tabular glyphs (`tnum`): sizes and counters do not jitter while they change.
Requires fontTools (`pip install fonttools`).
"""

import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont
from fontTools.varLib import instancer

OUT = Path(__file__).resolve().parent.parent / "crates" / "shot-core" / "assets" / "fonts"

WEIGHTS = {"Regular": 400, "SemiBold": 600}

DIGITS = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"]

UNICODES = [
    *range(0x20, 0x7F),  # Basic Latin
    *range(0xA0, 0x100),  # Latin-1: ×, ·, «», №-like signs
    *range(0x400, 0x460),  # Cyrillic
    *range(0x2010, 0x2028),  # dashes, quotes, bullet, ellipsis
    *range(0x2030, 0x203B),  # per mille, angle quotes
    0x2116,  # №
    *range(0x2190, 0x2194),  # arrows
    0x2212,  # minus
    0x2264,  # ≤
    0x2265,  # ≥
]


def build(source: Path, style: str, weight: int) -> Path:
    font = instancer.instantiateVariableFont(TTFont(source), {"wght": weight, "opsz": 14})

    tabular = {digit: f"{digit}.tf" for digit in DIGITS}
    for table in font["cmap"].tables:
        if table.isUnicode():
            for code, glyph in table.cmap.items():
                if glyph in tabular:
                    table.cmap[code] = tabular[glyph]

    options = subset.Options()
    options.layout_features = []
    options.drop_tables += ["GSUB", "GPOS", "GDEF", "STAT"]
    options.hinting = False
    options.name_IDs = ["*"]
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=UNICODES)
    subsetter.subset(font)

    path = OUT / f"Inter-{style}.ttf"
    font.save(path)
    return path


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    OUT.mkdir(parents=True, exist_ok=True)
    for style, weight in WEIGHTS.items():
        path = build(Path(sys.argv[1]), style, weight)
        print(f"{path.name}: {path.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
