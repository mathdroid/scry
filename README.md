# scry

A small Scryfall CLI for Magic: The Gathering, plus an MCP server and a Claude
Code skill that wrap it.

- **`scry search`** - run a raw Scryfall search (exact Scryfall syntax) and list
  every matching card.
- **`scry oracle`** - resolve a card name, a list of names, or a whole decklist
  to full oracle text, including the back face of double-faced cards.

```
scry search "f:c t:land id:rg produces:r,g"   # -> 173 cards + the list
scry search "f:c t:land id:rg produces:r,g" | scry oracle   # oracle text for all
scry oracle "Sol Ring" "Delver of Secrets"
cat deck.txt | scry oracle
```

## Layout

```
scry/
  src/main.rs       the CLI (Rust)
  Cargo.toml
  mcp/              MCP server (Node) that shells out to the scry binary
    index.js
    package.json
  skill/SKILL.md    Claude Code skill
  README.md
```

## 1. Install the CLI

Requires a Rust toolchain (`cargo`).

```bash
cd ~/Code/scry
cargo build --release
ln -sf "$PWD/target/release/scry" ~/bin/scry   # ~/bin must be on your PATH
scry --help
```

Rebuild after changes with `cargo build --release`; the symlink picks up the new
binary automatically.

### CLI usage

```
scry search <query...>     Run a raw Scryfall search.
  -c, --count              Print only the total match count.
  -l, --long               Include mana cost and type line per card.
      --json               Print the full Scryfall card objects.

scry oracle [names...]      Print full oracle text (incl. back faces).
  -d, --deck <FILE>        Read a decklist file (Moxfield/MTGA/plain).
      --json               Print the full Scryfall card objects.
                           A decklist can also be piped on stdin.
```

When `scry search` is piped, it prints bare card names only (no count header) so
it drops straight into `scry oracle`. Double-faced `Front // Back` names are
looked up by front face automatically.

## 2. Install the MCP server

Exposes two tools, `scryfall_search` and `scryfall_oracle`, over stdio. It calls
the `scry` binary, so install the CLI first (or set `SCRY_BIN`).

```bash
cd ~/Code/scry/mcp
npm install
```

Register it with Claude Code (one of):

```bash
# user scope, available in every project
claude mcp add scry --scope user -- node ~/Code/scry/mcp/index.js
```

Or add it to `~/.claude.json` under `mcpServers`:

```json
{
  "mcpServers": {
    "scry": {
      "type": "stdio",
      "command": "node",
      "args": ["/Users/pacaya/Code/scry/mcp/index.js"],
      "env": { "SCRY_BIN": "/Users/pacaya/bin/scry" }
    }
  }
}
```

`SCRY_BIN` is optional; without it the server resolves `scry` from `PATH`.
Restart Claude Code (or reconnect MCP servers) to pick it up.

### MCP tools

- `scryfall_search({ query, limit? })` - returns the match count and card names.
  `query` is exact Scryfall syntax; `limit` caps the names returned (the count
  is always exact).
- `scryfall_oracle({ names?, decklist? })` - returns full oracle text for the
  given card names and/or a decklist string. Unresolved names are appended as
  `not found: ...`.

## 3. Install the Claude Code skill

The skill in `skill/SKILL.md` teaches Claude when and how to use `scry`. Symlink
it into your skills directory:

```bash
ln -sfn ~/Code/scry/skill ~/.claude/skills/scry
```

It triggers on Scryfall searches, oracle-text lookups, decklist resolution, and
Scryfall-style queries.

## Notes

- Honors Scryfall's rate-limit guidance (100ms between requests).
- Sets a descriptive `User-Agent` as Scryfall requests.
- Network only; no API key required.
