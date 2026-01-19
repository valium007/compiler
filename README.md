`bril2json.py` to convert example.bril to json,
currently only a single function analysis is supported, so i just paste the json in parse.rs.

there are three IRs:
```
1. BrilIR -> json to brilir
2. HIR -> brilir to hir
3. SSA -> hir to ssa
```

idk why i made HIR, in future i will probably do BrilIR to SSA and remove the HIR :/