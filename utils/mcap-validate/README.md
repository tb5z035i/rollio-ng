# mcap-validate

Rust port of [`../py/validation.py`](../py/README.md) with batch parallelism.
Validates one MCAP file or a tree of `*.mcap` files against a per-station
spec TOML from [`../../configs/`](../../configs/README.md).

## Build

```sh
cargo build --release
# binary: target/release/mcap-validate
```

## Use

```sh
# single file
mcap-validate path/to/recording.mcap --spec ../../configs/collect_verify/data_spec_door.toml

# directory, parallel, JSON-Lines output
mcap-validate path/to/recordings/ \
    --spec ../../configs/collect_verify/data_spec_door.toml \
    --format json --jobs 8

# all three formats at once
mcap-validate path/ --spec spec.toml --format text --format json --format summary
```

Exit code is 0 if every file passed, 1 otherwise. `--no-constraints` skips the
sync_group / tf_pair checks. `--fail-fast` stops scheduling new files after the
first failure.

## Vendored flatbuffer code

`src/fbs/*_generated.rs` is the verbatim output of `flatc --rust` against the
Foxglove `.fbs` schemas. They are declared at crate root via `#[path]` in
`src/lib.rs` because flatc-generated code refers to its sibling modules as
`crate::Foo_generated::*`. To upgrade schemas, drop fresh files into
`src/fbs/` (and add `#[path]` lines to `lib.rs` if any new generated files
were added).
