"""Make a copy of a Barlow TTF whose digits are the font's own tabular figures.

Usage: python make-tabular.py INPUT.ttf OUTPUT.ttf

Reads the GSUB `tnum` single substitutions, points the cmap entries U+0030..U+0039 at the tabular glyphs,
and renames the family from "Barlow" to "Barlow Tabular" (name IDs 1, 3, 4, 6, 16). Nothing else changes.
Needs fonttools (see ../README.md for the version used).
"""

import sys

from fontTools.ttLib import TTFont

DIGITS = range(0x30, 0x3A)


def tnum_mapping(font):
    """The merged `tnum` single-substitution mapping, old glyph name to new glyph name."""
    gsub = font["GSUB"].table
    lookup_indexes = set()
    for record in gsub.FeatureList.FeatureRecord:
        if record.FeatureTag == "tnum":
            lookup_indexes.update(record.Feature.LookupListIndex)
    if not lookup_indexes:
        sys.exit("no tnum feature in GSUB")
    mapping = {}
    for index in sorted(lookup_indexes):
        for subtable in gsub.LookupList.Lookup[index].SubTable:
            if subtable.LookupType == 7:  # extension wrapper
                subtable = subtable.ExtSubTable
            if subtable.LookupType != 1:
                sys.exit(f"tnum lookup {index} is not a single substitution (type {subtable.LookupType})")
            mapping.update(subtable.mapping)
    return mapping


def rename(font):
    for record in font["name"].names:
        if record.nameID in (1, 3, 4, 16):
            record.string = record.toUnicode().replace("Barlow", "Barlow Tabular")
        elif record.nameID == 6:
            record.string = record.toUnicode().replace("Barlow", "BarlowTabular")


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    font = TTFont(sys.argv[1])
    mapping = tnum_mapping(font)
    cmap_tables = [t for t in font["cmap"].tables if t.isUnicode()]
    for code in DIGITS:
        old = font.getBestCmap()[code]
        new = mapping.get(old)
        if new is None:
            sys.exit(f"no tnum glyph for {old} (U+{code:04X})")
        for table in cmap_tables:
            if code in table.cmap:
                table.cmap[code] = new
        print(f"U+{code:04X}  {old} -> {new}")
    rename(font)
    font.save(sys.argv[2])


main()
