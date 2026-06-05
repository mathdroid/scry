---
name: scry
description: Query Magic: The Gathering cards via Scryfall from the terminal - run raw Scryfall searches (exact Scryfall syntax) and pull full oracle text (including back faces) for a card, a list of cards, or a whole decklist. Use when the user wants to search Scryfall, count/list cards matching a query, look up a card's rules text, resolve a decklist to oracle text, or mentions "scryfall" / "oracle text" / a Scryfall query like "f:c t:land id:rg".
---

# scry

`scry` is a CLI at `~/bin/scry` wrapping the Scryfall API. Two subcommands.

## search - raw Scryfall query

Pass an exact Scryfall query (same operators as scryfall.com). Prints the total
count and every matching card name (it paginates through all results).

```bash
scry search "f:c t:land id:rg produces:r,g"   # count + full name list
scry search "..." -c                           # count only
scry search "..." -l                           # name + mana cost + type line
scry search "..." --json                       # full Scryfall card objects
```

When stdout is piped, `search` prints bare card names only (no count header),
so it feeds straight into `oracle`:

```bash
scry search "f:c t:land id:rg produces:r,g" | scry oracle
```

Common Scryfall operators: `t:` type, `o:` oracle text, `id:` color identity,
`c:` color, `produces:` mana produced, `f:` format legality, `mv:` mana value,
`r:` rarity, `is:` (e.g. `is:commander`). Quote the whole query.

## oracle - full oracle text

Resolves card names to full oracle text, including every face of
double-faced / split / adventure cards (faces separated by `//`).

```bash
scry oracle "Sol Ring" "Delver of Secrets"     # names as args
scry oracle --deck mydeck.txt                    # a decklist file
cat deck.txt | scry oracle                        # decklist on stdin
scry oracle "..." --json                          # full Scryfall card objects
```

The decklist parser strips quantities (`4`, `1x`), set/collector tags
(`(NEO) 141`), foil markers (`*F*`), comments (`//`, `#`) and section headers
(`Deck`, `Sideboard`, `Commander`). Names are deduped case-insensitively.
Unresolved names are reported on stderr as `not found: ...`.

## Tips

- For "what do all the cards matching X do?", pipe: `scry search "X" | scry oracle`.
- Double-faced names like `Front // Back` are looked up by their front face
  automatically, so search output pipes into oracle cleanly.
- Use `--json` when you need structured data (prices, sets, legalities, etc.).

An MCP server (tools `scryfall_search`, `scryfall_oracle`) wrapping the same
binary is also available; see the project README to enable it.
