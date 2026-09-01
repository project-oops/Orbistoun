# D013 - Lints live in a workspace table, with unsafe discipline at deny

**decided** · 2026-08-19

`undocumented_unsafe_blocks`, `multiple_unsafe_ops_per_block`, and
`unsafe_op_in_unsafe_fn` are **deny**. A workspace table rather than CI flags alone,
so rust-analyzer applies the same strictness while typing. CI adds `-D warnings`.

The pedantic opt-outs each carry a stated reason; removing one should be a
deliberate cleanup. `cast_possible_truncation` and friends are allowed because guest
values are fixed-width by ABI - the truncation *is* the contract.

