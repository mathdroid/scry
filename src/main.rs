//! scry - a small Scryfall CLI.
//!
//! Two subcommands:
//!   search  Run a raw Scryfall search query and list the matching cards.
//!   oracle  Resolve card names (args, a decklist file, or stdin) to their
//!           full oracle text, including every face of multi-faced cards.

use std::io::Read;
use std::process::exit;
use std::thread::sleep;
use std::time::Duration;

use clap::{Parser, Subcommand};
use serde::Deserialize;
use serde_json::json;

const API: &str = "https://api.scryfall.com";
const UA: &str = "scry/0.1 (Scryfall CLI; +https://scryfall.com/docs/api)";
// Scryfall asks for 50-100ms between requests.
const DELAY: Duration = Duration::from_millis(100);

#[derive(Parser)]
#[command(
    name = "scry",
    about = "Search Scryfall and pull full oracle text from the command line.",
    version
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a raw Scryfall search (exact Scryfall syntax) and list the matches.
    Search {
        /// The Scryfall query, e.g. "f:c t:land id:rg produces:r,g".
        /// Multiple words are joined with spaces.
        #[arg(required = true)]
        query: Vec<String>,
        /// Only print the total match count, not the card list.
        #[arg(short, long)]
        count: bool,
        /// Long output: include mana cost and type line per card.
        #[arg(short, long)]
        long: bool,
        /// Print the raw Scryfall JSON for every matching card.
        #[arg(long)]
        json: bool,
    },
    /// Resolve card names to full oracle text (including back faces).
    Oracle {
        /// Card names. Each argument is one full card name.
        names: Vec<String>,
        /// Read a decklist from this file (e.g. Moxfield/MTGA export).
        #[arg(short, long)]
        deck: Option<String>,
        /// Print the raw Scryfall JSON for every resolved card.
        #[arg(long)]
        json: bool,
    },
}

// ---- Scryfall response types ----
//
// Cards are kept as raw serde_json::Value so `--json` emits the complete
// Scryfall object; typed fields are read via the small accessors below.

#[derive(Deserialize)]
struct SearchPage {
    total_cards: Option<u64>,
    has_more: Option<bool>,
    next_page: Option<String>,
    #[serde(default)]
    data: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Collection {
    #[serde(default)]
    data: Vec<serde_json::Value>,
    #[serde(default)]
    not_found: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct ScryError {
    details: String,
}

/// Borrow a non-empty string field from a card/face Value.
fn field<'a>(v: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(|x| x.as_str()).filter(|s| !s.is_empty())
}

fn main() {
    // Exit quietly when our output pipe is closed (e.g. `scry ... | head`)
    // instead of panicking on a broken-pipe write.
    unsafe { reset_sigpipe() };
    let cli = Cli::parse();
    let result = match cli.cmd {
        Cmd::Search {
            query,
            count,
            long,
            json,
        } => run_search(&query.join(" "), count, long, json),
        Cmd::Oracle { names, deck, json } => run_oracle(names, deck, json),
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        exit(1);
    }
}

// ---- HTTP helpers ----

/// Turn a ureq error into a readable message, surfacing Scryfall's `details`
/// field when the API returns a structured error body.
fn explain(err: ureq::Error) -> String {
    match err {
        ureq::Error::Status(code, resp) => match resp.into_json::<ScryError>() {
            Ok(e) => e.details,
            Err(_) => format!("HTTP {code}"),
        },
        ureq::Error::Transport(t) => t.to_string(),
    }
}

fn get_page(url: &str) -> Result<SearchPage, String> {
    let resp = ureq::get(url)
        .set("User-Agent", UA)
        .set("Accept", "application/json")
        .call();
    match resp {
        Ok(r) => r.into_json::<SearchPage>().map_err(|e| e.to_string()),
        // A search with zero results is a 404 from Scryfall; treat it as empty.
        Err(ureq::Error::Status(404, _)) => Ok(SearchPage {
            total_cards: Some(0),
            has_more: Some(false),
            next_page: None,
            data: vec![],
        }),
        Err(e) => Err(explain(e)),
    }
}

// ---- search ----

fn run_search(query: &str, count_only: bool, long: bool, as_json: bool) -> Result<(), String> {
    let mut url = format!(
        "{API}/cards/search?unique=cards&order=name&q={}",
        urlencode(query)
    );
    let mut cards: Vec<serde_json::Value> = Vec::new();
    let mut total: u64 = 0;
    let mut first = true;

    loop {
        let page = get_page(&url)?;
        if first {
            total = page.total_cards.unwrap_or(0);
            first = false;
        }
        cards.extend(page.data);
        match (page.has_more.unwrap_or(false), page.next_page) {
            (true, Some(next)) => {
                url = next;
                sleep(DELAY);
            }
            _ => break,
        }
    }

    if as_json {
        println!("{}", serde_json::to_string_pretty(&cards).unwrap());
        return Ok(());
    }

    // When piped (stdout not a terminal), emit only bare card names so the
    // output drops straight into `scry oracle`. The count header is for humans.
    let piped = !is_stdout_tty();
    if !piped {
        let noun = if total == 1 { "card" } else { "cards" };
        println!("{total} {noun} matching `{query}`");
    }
    if count_only || total == 0 {
        return Ok(());
    }
    if !piped {
        println!();
    }
    for c in &cards {
        let name = field(c, "name").unwrap_or("?");
        if long {
            let kind = field(c, "type_line").unwrap_or("");
            match field(c, "mana_cost") {
                Some(cost) => println!("{name}  {cost}  ({kind})"),
                None => println!("{name}  ({kind})"),
            }
        } else {
            println!("{name}");
        }
    }
    Ok(())
}

// ---- oracle ----

fn run_oracle(names: Vec<String>, deck: Option<String>, as_json: bool) -> Result<(), String> {
    let mut wanted: Vec<String> = Vec::new();
    wanted.extend(names);

    if let Some(path) = deck {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
        wanted.extend(parse_decklist(&text));
    }

    // If stdin is piped, read a decklist from it too.
    if !is_stdin_tty() {
        let mut buf = String::new();
        if std::io::stdin().read_to_string(&mut buf).is_ok() && !buf.trim().is_empty() {
            wanted.extend(parse_decklist(&buf));
        }
    }

    let wanted = dedup_ci(wanted);
    if wanted.is_empty() {
        return Err("no card names given (pass names, --deck FILE, or pipe a decklist)".into());
    }

    let mut cards: Vec<serde_json::Value> = Vec::new();
    let mut not_found: Vec<String> = Vec::new();

    // Scryfall's collection endpoint accepts up to 75 identifiers per call.
    for chunk in wanted.chunks(75) {
        // Scryfall matches multi-faced cards by their front-face name, so a
        // full "Front // Back" name (as printed by `scry search`) must be
        // reduced to the part before " // " before lookup.
        let identifiers: Vec<serde_json::Value> = chunk
            .iter()
            .map(|n| json!({ "name": front_face(n) }))
            .collect();
        let body = json!({ "identifiers": identifiers });
        let resp = ureq::post(&format!("{API}/cards/collection"))
            .set("User-Agent", UA)
            .set("Accept", "application/json")
            .set("Content-Type", "application/json")
            .send_json(body)
            .map_err(explain)?;
        let coll: Collection = resp.into_json().map_err(|e| e.to_string())?;
        cards.extend(coll.data);
        for nf in coll.not_found {
            if let Some(n) = nf.get("name").and_then(|v| v.as_str()) {
                not_found.push(n.to_string());
            }
        }
        sleep(DELAY);
    }

    if as_json {
        println!("{}", serde_json::to_string_pretty(&cards).unwrap());
    } else {
        for (i, c) in cards.iter().enumerate() {
            if i > 0 {
                println!();
                println!("{}", "-".repeat(40));
                println!();
            }
            print_card(c);
        }
    }

    if !not_found.is_empty() {
        eprintln!("\nnot found: {}", not_found.join(", "));
    }
    Ok(())
}

/// Print one card's full oracle text, descending into faces when present.
fn print_card(c: &serde_json::Value) {
    match c.get("card_faces").and_then(|f| f.as_array()) {
        Some(faces) if !faces.is_empty() => {
            for (i, f) in faces.iter().enumerate() {
                if i > 0 {
                    println!("\n  //\n");
                }
                print_block(f);
            }
        }
        _ => print_block(c),
    }
}

fn print_block(v: &serde_json::Value) {
    let name = field(v, "name").unwrap_or("?");
    match field(v, "mana_cost") {
        Some(cost) => println!("{name}  {cost}"),
        None => println!("{name}"),
    }
    if let Some(kind) = field(v, "type_line") {
        println!("{kind}");
    }
    match field(v, "oracle_text") {
        Some(text) => println!("{text}"),
        None => println!("(no oracle text)"),
    }
}

// ---- helpers ----

/// Parse a decklist (Moxfield / MTGA / plain) into card names.
/// Handles leading quantities ("4 ", "4x "), trailing set/collector info
/// ("(NEO) 123"), foil markers ("*F*"), comments and section headers.
fn parse_decklist(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") || line.starts_with('#') {
            continue;
        }
        // Section headers like "Deck", "Sideboard", "Commander", "Sideboard:".
        let lower = line.to_ascii_lowercase();
        if matches!(
            lower.trim_end_matches(':'),
            "deck" | "sideboard" | "commander" | "companion" | "maybeboard" | "tokens"
        ) {
            continue;
        }

        let mut s = line;
        // Strip a leading quantity: digits, optional 'x', whitespace.
        s = strip_quantity(s);
        // Cut off trailing set info "(XXX) 123" or foil "*F*" markers.
        s = cut_trailing(s);
        let name = s.trim();
        if !name.is_empty() {
            out.push(name.to_string());
        }
    }
    out
}

fn strip_quantity(s: &str) -> &str {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return s; // no leading number
    }
    let mut j = i;
    if j < bytes.len() && (bytes[j] == b'x' || bytes[j] == b'X') {
        j += 1;
    }
    if j < bytes.len() && bytes[j] == b' ' {
        return s[j..].trim_start();
    }
    s
}

/// Front-face name of a card: the part before " // " for multi-faced cards.
fn front_face(name: &str) -> &str {
    match name.split_once(" // ") {
        Some((front, _)) => front,
        None => name,
    }
}

fn cut_trailing(s: &str) -> &str {
    let mut end = s.len();
    if let Some(p) = s.find(" (") {
        end = end.min(p);
    }
    if let Some(p) = s.find(" *") {
        end = end.min(p);
    }
    &s[..end]
}

/// Deduplicate names case-insensitively, preserving first-seen order.
fn dedup_ci(names: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for n in names {
        let key = n.to_ascii_lowercase();
        if seen.insert(key) {
            out.push(n);
        }
    }
    out
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn is_stdin_tty() -> bool {
    // 0 == STDIN_FILENO. isatty returns 1 for a terminal.
    unsafe { libc_isatty(0) == 1 }
}

fn is_stdout_tty() -> bool {
    // 1 == STDOUT_FILENO.
    unsafe { libc_isatty(1) == 1 }
}

extern "C" {
    #[link_name = "isatty"]
    fn libc_isatty(fd: i32) -> i32;
    fn signal(signum: i32, handler: usize) -> usize;
}

/// Restore the default SIGPIPE disposition so a closed downstream pipe
/// terminates the process normally rather than triggering a Rust panic.
unsafe fn reset_sigpipe() {
    const SIGPIPE: i32 = 13;
    const SIG_DFL: usize = 0;
    signal(SIGPIPE, SIG_DFL);
}
