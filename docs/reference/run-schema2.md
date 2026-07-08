# Running `schema2.graphql`

This is the shortest path to run the framework on a new schema without target-
specific tweaks.

Current local inputs:

- schema: `schema2.graphql`
- output dir: `output/schema2/`

## 1. Build

Use the release binary for repeat runs:

```bash
cargo build --release
```

## 2. Stage 0: SDL -> IR

```bash
target/release/graphql-static-bac stage s0 \
  --input schema2.graphql \
  --format sdl \
  --output output/schema2/schema_ir.json
```

Quick sanity check:

```bash
jq '{roots:.data.roots, type_count:(.data.types|length)}' output/schema2/schema_ir.json
```

What to verify:

- `roots.query` exists
- `roots.mutation` exists if the schema has mutations
- interfaces/unions have `possible_types`
- arguments have full `type_ref`

## 3. Stage 2: argument classification

```bash
target/release/graphql-static-bac stage s2 \
  --schema-ir output/schema2/schema_ir.json \
  --policy config/lexicons/argument-classifier-v1.json \
  --output output/schema2/args.json
```

Quick stats:

```bash
jq '{field_records:(.data.fields|length), arg_records:([.data.fields[].arguments[]]|length), class_breakdown:([.data.fields[].arguments[]|.classifications[]] | group_by(.) | map({classification:.[0],count:length}))}' \
  output/schema2/args.json
```

## 4. Stage 3: route analysis for one target type

Example target:

```bash
target/release/graphql-static-bac route \
  --schema-ir output/schema2/schema_ir.json \
  --args output/schema2/args.json \
  --policy config/profiles/route-analysis-v1.json \
  --target Event \
  --output output/schema2/routes.event.json
```

Quick summary:

```bash
jq '.data.targets["type:Event"] | {reachability, best_verdict, route_count:(.routes|length), verdicts:(.routes|group_by(.verdict)|map({verdict:.[0].verdict,count:length}))}' \
  output/schema2/routes.event.json
```

Show only `open + unknown`:

```bash
jq '.data.targets["type:Event"].routes | map(select(.verdict=="open" or .verdict=="unknown")) | length' \
  output/schema2/routes.event.json
```

Show the `open` routes:

```bash
jq -r '.data.targets["type:Event"].routes[] | select(.verdict=="open") | [.selector.arg_ref, .witness.display_projection] | @tsv' \
  output/schema2/routes.event.json
```

## 5. Stage 4: seed planning for one target type

```bash
target/release/graphql-static-bac stage s4 \
  --schema-ir output/schema2/schema_ir.json \
  --args output/schema2/args.json \
  --routes output/schema2/routes.event.json \
  --output output/schema2/seed-plans.event.json
```

Quick summary:

```bash
jq '{route_count:(.data.routes|length), executable_count:([.data.routes[]|select(.executable==true)]|length)}' \
  output/schema2/seed-plans.event.json
```

## 6. Re-run on another target

Only change the target name and output file:

```bash
target=replaced_type_name

target/release/graphql-static-bac route \
  --schema-ir output/schema2/schema_ir.json \
  --args output/schema2/args.json \
  --policy config/profiles/route-analysis-v1.json \
  --target "$target" \
  --output "output/schema2/routes.${target}.json"
```

If you do not know the exact target name yet:

```bash
rg '^type |^interface |^union ' schema2.graphql
```

## 7. Current framework limits

These matter for any new schema, not just `schema2.graphql`:

1. S3 currently starts from `Query` only.
   - Mutation reachability is not in the route graph yet.

2. S2 keeps `possible_selector` for audit, but S3/S4 only execute definite
   selectors.

3. Relation filters can still look like direct selectors.
   - Example from this schema: `EventFilterInput.communityIds` is currently
     classified as an `object_selector` and yields an `open` route to `Event`.
   - That is likely too optimistic and should be reviewed case by case.

4. Custom scalar naming may need lexicon tuning.
   - Examples to watch: `UUID`, `ObjectID`, `ULID`, app-specific IDs.

## 8. Files already generated locally

These have already been produced in this workspace:

- `output/schema2/schema_ir.json`
- `output/schema2/args.json`
- `output/schema2/routes.event.json`
