# omniplan_covers_ding

> Convert OmniPlan CSV exports to DingTalk-compatible Excel documents.

## What It Does

Takes a CSV file exported from OmniPlan and converts it into a structured Excel file suitable for import into DingTalk project management (tasks or requirements).

## Usage

```bash
omniplan_covers_ding convert <csv-file> <doc-type> [-p <parent>] [-t <liter>] [-l <limit>]
```

### Arguments

- `<csv-file>`: Path to the input CSV file
- `<doc-type>`: Output document type — `task` or `require`
- `-p, --parent <value>`: Parent task value
- `-t, --liter <value>`: Liter assignment
- `-l, --limit <value>`: Limit filter in `k=v` format

### Examples

```bash
# Convert to task format
omniplan_covers_ding convert plan.csv task -p "Sprint 1"

# Convert to requirement format
omniplan_covers_ding convert plan.csv require -t "Team A"
```

## Build

```bash
cargo build --release
```

**Note:** This tool depends on `cli-common` from an external path (`/Users/admin/data0/private_work/crate-r-svr-api/cli-common`), so it is not portable to other machines without adjusting the dependency path.

## License

See LICENSE file.
