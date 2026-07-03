db_infra: lightweight service demonstrating DB routing, sharding, and admin endpoints

Quick start:

- Build and run:

```bash
cd tools/db_infra
cargo run --release
```

- Environment variables:
  - `DB_WRITE_SHARD_1` - write connection URL for shard 1
  - `DB_READ_1` - read replica URL for shard 1

Testing:

- Unit tests:

```bash
cd tools/db_infra
cargo test -p db_infra
```

Notes:
- This is a scaffold demonstrating filesystem-level implementation of shard locator, routing, metrics, and admin endpoints. Integrate into the main service as needed.
