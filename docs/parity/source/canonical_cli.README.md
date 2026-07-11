# Frozen canonical CLI evidence

`canonical_cli.json` is generated from canonical cmux commit
`e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452`. The generator reads the Swift
source with `git show`, so results do not depend on the checked-out branch.

Regenerate or validate it with:

```text
python scripts/extract_canonical_cli.py
python scripts/extract_canonical_cli.py --check
python -m unittest tests.test_extract_canonical_cli
```

The catalog records all 174 accepted top-level command tokens. It marks the 16
tokens with no canonical Usage/help contract—private diagnostics,
implementation helpers, internal transport entrypoints, and explicitly hidden
compatibility hooks—without deleting them from the evidence. The remaining 158
public/help-discoverable commands exactly match the parity audit target. Help-discoverable
subcommands and flags are extracted conservatively; an empty list means the
frozen help source did not expose that detail, not that the command takes none.
