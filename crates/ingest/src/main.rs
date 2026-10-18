//! Fetches a thewowdb.com/wow-forever spell page and emits a SpellDef-shaped
//! JSON object, ready to paste into a data pack after review. Network I/O
//! lives only here; `wowforever_ingest::parse_spell_page` (the tested part)
//! takes raw HTML, not a URL.
//!
//! Usage: wowforever-ingest --spell-id 11661 --id shadow_bolt --name "Shadow Bolt"

use std::env;
use ureq::ResponseExt;
use wowforever_ingest::parse_spell_page;

struct Args {
    spell_id: String,
    id: String,
    name: String,
}

fn parse_args() -> Args {
    let mut spell_id = None;
    let mut id = None;
    let mut name = None;
    let mut args = env::args().skip(1);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--spell-id" => spell_id = Some(args.next().expect("--spell-id requires a value")),
            "--id" => id = Some(args.next().expect("--id requires a value")),
            "--name" => name = Some(args.next().expect("--name requires a value")),
            other => panic!("unknown argument `{other}` (expected --spell-id, --id, --name)"),
        }
    }
    Args {
        spell_id: spell_id.expect("--spell-id is required, e.g. --spell-id 11661"),
        id: id.expect("--id is required, e.g. --id shadow_bolt"),
        name: name.expect("--name is required, e.g. --name \"Shadow Bolt\""),
    }
}

fn main() {
    let args = parse_args();
    let url = format!("https://thewowdb.com/wow-forever/spell/{}", args.spell_id);

    eprintln!("Fetching {url} ...");
    let mut response = ureq::get(&url).call().unwrap_or_else(|e| panic!("request to {url} failed: {e}"));
    let final_url = response.get_uri().to_string();
    let html = response
        .body_mut()
        .read_to_string()
        .unwrap_or_else(|e| panic!("failed to read response body from {url}: {e}"));

    eprintln!("Resolved to {final_url}");

    match parse_spell_page(&html, &args.id, &args.name, &final_url) {
        Ok(parsed) => {
            for w in &parsed.warnings {
                eprintln!("[!] {w}");
            }
            println!("{}", serde_json::to_string_pretty(&parsed.spell).expect("SpellDef must serialize"));
        }
        Err(e) => {
            eprintln!("FAILED to parse {final_url}: {e}");
            std::process::exit(1);
        }
    }
}
