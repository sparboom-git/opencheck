# Classification systems

The lists of codes that OpenCheck can check the classification codes of a
model against. See
[docs/information-requirements.md](../../../docs/information-requirements.md#classification-codes)
for what the check does.

A system in one edition is three files with the same name:

| File | What it is | Whose |
|---|---|---|
| `<id>.txt` | The list: code, name and level, separated by tabs, one code per line | The publisher's, as published |
| `<id>.toml` | What the list is (name, version, edition, publisher, source, licence) and where a model keeps the code and the type id, written as the sources of an information standard | OpenCheck's |
| `<id>.expect.toml` | Which IFC classes a code is expected on | OpenCheck's |

With a system chosen for a project, the sources in `<id>.toml` are also
where the information rule reads the classification code and the type id
(the items with `role = "code"` and `role = "type_id"` in the information
standard), in place of the standard's own sources, which cover several
systems.

The list is kept as the publisher wrote it. What OpenCheck adds lives in the
other two files, so the list itself is never an adapted version.

## BIMTypeCode 2022 DK

`bimtypecode-2022-dk.txt` is the file *BIMTypeCode - Assembly Codes -
2022DK.txt* from <https://github.com/bimtypecode/bimtypecode> (revision
`e8ba472`), converted from Windows-1252 to UTF-8 with LF line endings and otherwise
unchanged.
"BIMTypeCode 2022 publications" by [BIM7AA](http://BIM7AA.dk) is licensed
under [CC BY-ND 4.0](https://creativecommons.org/licenses/by-nd/4.0/). The
notice that ships with OpenCheck is
[BIMTypeCode-NOTICE.txt](BIMTypeCode-NOTICE.txt).

Do not edit the list: the licence does not allow sharing a changed version.
A correction belongs with the publisher, and a newer edition is added as a
new system with its own id. The value list of *BIMTypeNumber* in the same
repository is under another licence (CC BY-SA 4.0) and is not included.

## Adding a system or an edition

1. Add the three files and an entry in `BUILTIN_CLASSIFICATIONS`
   (`src/check/classification.rs`). The id names system and edition, e.g.
   `bimtypecode-2022-dk`; it is what a project stores.
2. Check the licence of the list first, and add its notice here, to the
   licence section of the README and to the files the release scripts copy
   (`.github/workflows/release.yml`, `scripts/pack-msix.ps1`).
3. Write the expectations wide: only a clear mismatch should be reported.
   Run the check on models coded by people who know the system, and loosen
   what they trip over.
