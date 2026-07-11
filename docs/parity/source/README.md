# Canonical socket-v2 source catalog

`canonical_v2.json` is generated from the frozen canonical cmux commit
`e1825d40d52b4ae4f4bcb0b7e0dfc744dd20a452`. The generator reads Git objects,
not working-tree files, so later Windows-port edits cannot change the baseline.

Regenerate and verify it with:

```powershell
python scripts/parity/extract_canonical_v2.py
python scripts/parity/extract_canonical_v2.py --check
```

The source of truth contains exactly 251 methods in the unconditional Release
capability array. A Debug build appends 42 additional methods, for 293 advertised
methods in total. There is therefore no discrepancy with the stated 251-method
Release target. The catalog uses `test_only` only for endpoints defined solely
inside test targets; none exist at this commit. Debug-only diagnostic and test
probe endpoints remain classified as `debug_only`, even when tests exercise them,
because they are compiled into and served by the Debug app.

Each entry records its advertised source line, domain/family, every detected
production dispatch arm, implementation entrypoint, and exact-string references
under canonical contract and test roots. An empty `contract_test_pointers` array
means no exact method-string reference exists in those roots; it is not evidence
that the behavior lacks indirect coverage.
