#!/usr/bin/env node
// MCP server for the `scry` Scryfall CLI.
//
// It shells out to the `scry` binary so the MCP tools share exactly the CLI's
// behaviour: full pagination, double-faced front-face normalization, decklist
// parsing, and Scryfall rate limiting. Point at a specific binary with the
// SCRY_BIN env var; otherwise `scry` is resolved from PATH.

import { spawn } from "node:child_process";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";

const SCRY_BIN = process.env.SCRY_BIN || "scry";

// Run the scry binary, optionally feeding `stdin`. Resolves with
// { code, stdout, stderr } and never rejects on a non-zero exit.
function runScry(args, stdin) {
  return new Promise((resolve, reject) => {
    const child = spawn(SCRY_BIN, args, { stdio: ["pipe", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (d) => (stdout += d));
    child.stderr.on("data", (d) => (stderr += d));
    child.on("error", (err) =>
      reject(
        new Error(
          `failed to run '${SCRY_BIN}': ${err.message}. ` +
            `Install the CLI or set SCRY_BIN to its path.`
        )
      )
    );
    child.on("close", (code) => resolve({ code, stdout, stderr }));
    if (stdin) child.stdin.write(stdin);
    child.stdin.end();
  });
}

const server = new McpServer({ name: "scry", version: "0.1.0" });

server.registerTool(
  "scryfall_search",
  {
    title: "Scryfall search",
    description:
      "Run a raw Scryfall search using exact Scryfall query syntax (e.g. " +
      '"f:c t:land id:rg produces:r,g") and return the total match count plus ' +
      "the list of card names. Use the same operators as scryfall.com.",
    inputSchema: {
      query: z
        .string()
        .describe('Scryfall query in exact Scryfall syntax, e.g. "t:land id:rg".'),
      limit: z
        .number()
        .int()
        .positive()
        .optional()
        .describe("Cap the number of card names returned (count is always exact)."),
    },
  },
  async ({ query, limit }) => {
    const { code, stdout, stderr } = await runScry(["search", query, "--json"]);
    if (code !== 0) {
      return {
        isError: true,
        content: [{ type: "text", text: stderr.trim() || `scry exited ${code}` }],
      };
    }
    const cards = JSON.parse(stdout);
    const total = cards.length;
    const shown = limit ? cards.slice(0, limit) : cards;
    const lines = [`${total} card${total === 1 ? "" : "s"} matching \`${query}\``, ""];
    for (const c of shown) lines.push(c.name);
    if (limit && total > limit) lines.push(`... (${total - limit} more)`);
    return { content: [{ type: "text", text: lines.join("\n") }] };
  }
);

server.registerTool(
  "scryfall_oracle",
  {
    title: "Scryfall oracle text",
    description:
      "Return the full oracle text of one or more Magic cards, including every " +
      "face of double-faced/split/adventure cards. Accepts a list of card names " +
      "and/or a raw decklist (Moxfield/MTGA/plain export).",
    inputSchema: {
      names: z
        .array(z.string())
        .optional()
        .describe("Card names, each a full card name."),
      decklist: z
        .string()
        .optional()
        .describe(
          "A decklist to resolve. Quantities, set tags, foil markers, comments " +
            "and section headers are stripped automatically."
        ),
    },
  },
  async ({ names, decklist }) => {
    if ((!names || names.length === 0) && (!decklist || !decklist.trim())) {
      return {
        isError: true,
        content: [{ type: "text", text: "provide `names` and/or a `decklist`." }],
      };
    }
    const { code, stdout, stderr } = await runScry(
      ["oracle", ...(names || [])],
      decklist
    );
    if (code !== 0) {
      return {
        isError: true,
        content: [{ type: "text", text: stderr.trim() || `scry exited ${code}` }],
      };
    }
    // scry prints "not found: ..." to stderr; surface it with the results.
    const text = stderr.trim() ? `${stdout.trimEnd()}\n\n${stderr.trim()}` : stdout;
    return { content: [{ type: "text", text }] };
  }
);

server.registerTool(
  "scryfall_rulings",
  {
    title: "Scryfall rulings",
    description:
      "Return the official rulings (with dates) for one or more Magic cards.",
    inputSchema: {
      names: z
        .array(z.string())
        .nonempty()
        .describe("Card names, each a full card name."),
    },
  },
  async ({ names }) => {
    const { code, stdout, stderr } = await runScry(["rulings", ...names]);
    if (code !== 0) {
      return {
        isError: true,
        content: [{ type: "text", text: stderr.trim() || `scry exited ${code}` }],
      };
    }
    const text = stderr.trim() ? `${stdout.trimEnd()}\n\n${stderr.trim()}` : stdout;
    return { content: [{ type: "text", text }] };
  }
);

server.registerTool(
  "scryfall_card_details",
  {
    title: "Scryfall card details",
    description:
      "Return card details beyond oracle text: format legalities and current " +
      "prices (USD/EUR/tix), plus mana cost and type line. Accepts card names " +
      "and/or a raw decklist.",
    inputSchema: {
      names: z
        .array(z.string())
        .optional()
        .describe("Card names, each a full card name."),
      decklist: z
        .string()
        .optional()
        .describe("A decklist to resolve (quantities/headers stripped)."),
    },
  },
  async ({ names, decklist }) => {
    if ((!names || names.length === 0) && (!decklist || !decklist.trim())) {
      return {
        isError: true,
        content: [{ type: "text", text: "provide `names` and/or a `decklist`." }],
      };
    }
    const { code, stdout, stderr } = await runScry(
      ["oracle", "--json", ...(names || [])],
      decklist
    );
    if (code !== 0) {
      return {
        isError: true,
        content: [{ type: "text", text: stderr.trim() || `scry exited ${code}` }],
      };
    }
    const cards = JSON.parse(stdout);
    const blocks = cards.map((c) => {
      const lines = [
        [c.name, c.mana_cost].filter(Boolean).join("  "),
        c.type_line,
      ].filter(Boolean);
      const legal = Object.entries(c.legalities || {})
        .filter(([, v]) => v === "legal")
        .map(([f]) => f);
      lines.push(legal.length ? `legal in: ${legal.join(", ")}` : "legal in: (nothing)");
      const p = c.prices || {};
      const prices = [
        p.usd && `$${p.usd}`,
        p.usd_foil && `$${p.usd_foil} foil`,
        p.eur && `${p.eur} EUR`,
        p.tix && `${p.tix} tix`,
      ].filter(Boolean);
      if (prices.length) lines.push(`prices: ${prices.join(" / ")}`);
      return lines.join("\n");
    });
    const text = blocks.join("\n\n" + "-".repeat(40) + "\n\n");
    const tail = stderr.trim();
    return {
      content: [{ type: "text", text: tail ? `${text}\n\n${tail}` : text }],
    };
  }
);

const transport = new StdioServerTransport();
await server.connect(transport);
