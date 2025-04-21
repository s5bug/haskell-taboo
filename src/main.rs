use std::collections::HashSet;
use std::{fs, io};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;
use clap::Parser;
use colored::Colorize;
use memmap2::Mmap;
use tree_sitter::{Query, QueryCursor, StreamingIterator};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Location of banned words list
    #[arg()]
    taboo: String,

    /// Files to check against
    #[arg()]
    files: Vec<String>,
}

fn main() -> ExitCode {
    let args = Args::parse_from(wild::args());

    match File::open(&args.taboo) {
        Ok(taboo_file) => {
            let banned_words_set: HashSet<String> = banned_words_from(taboo_file);

            let test_paths: Vec<PathBuf> = if args.files.is_empty() {
                fs::read_dir("src").unwrap()
                    .map(|di| { di.unwrap().path() })
                    .collect()
            } else {
                args.files.iter().map( |s| { PathBuf::from(s) } ).collect()
            };

            let found_banned_word =
                check_paths_for_banned_words(banned_words_set, test_paths).unwrap();

            if found_banned_word { ExitCode::FAILURE }
            else { ExitCode::SUCCESS }
        }
        Err(e) => {
            eprintln!("Error opening taboo file {}: {}", args.taboo, e);
            ExitCode::FAILURE
        }
    }
}

fn banned_words_from(file: File) -> HashSet<String> {
    let buf_read = BufReader::new(file);
    buf_read.lines()
        .map(|l| l.unwrap().trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

fn check_paths_for_banned_words(banned_words_set: HashSet<String>, paths: Vec<PathBuf>) -> std::io::Result<bool> {
    let query = Query::new(
        &tree_sitter_haskell::LANGUAGE.into(),
        "(variable) @variable-name"
    ).expect("Error constructing name query");

    let mut parser = tree_sitter::Parser::new();

    parser.set_language(&tree_sitter_haskell::LANGUAGE.into())
        .expect("Error loading Haskell grammar");

    let mut seen_banned_word = false;

    for path in paths {
        let file = File::open(&path)?;
        let mmap = unsafe { Mmap::map(&file)? };
        let mmap_slice: &[u8] = &mmap;

        // skip checking the file if parsing as Haskell fails
        if let Some(tree) = parser.parse(mmap_slice, None) {
            let mut query_cursor = QueryCursor::new();
            let mut names = query_cursor.matches(&query, tree.root_node(), mmap_slice);

            while let Some(name) = names.next() {
                for capture in name.captures {
                    let text = capture.node.utf8_text(mmap_slice).unwrap();

                    if banned_words_set.contains(text) {
                        if !seen_banned_word {
                            println!("ERROR: Banned identifiers found");
                            println!("Found the following issues:");
                            
                            seen_banned_word = true;
                        }
                        
                        let start_byte = capture.node.start_byte();
                        let end_byte = capture.node.end_byte();
                        let slice_before = &mmap_slice[0..start_byte];
                        let line_first_char =
                            slice_before.iter()
                                .rposition(|b| *b == '\n' as u8 || *b == '\r' as u8)
                                .map(|b| b + 1)
                                .unwrap_or(0);
                        let slice_after = &mmap_slice[end_byte..];
                        let line_last_char =
                            end_byte + slice_after.iter()
                                .position(|b| *b == '\n' as u8 || *b == '\r' as u8)
                                .unwrap_or(mmap_slice.len());
                        
                        let pre_banned = &mmap_slice[line_first_char..start_byte];
                        let post_banned = &mmap_slice[end_byte..line_last_char];
                        
                        eprintln!(
                            "({}:{}:{}) {}{}{}",
                            path.display(),
                            capture.node.start_position().row + 1,
                            capture.node.start_position().column,
                            String::from_utf8_lossy(pre_banned),
                            text.bright_red().bold(),
                            String::from_utf8_lossy(post_banned)
                        )
                    }
                }
            }
        }
    }

    io::Result::Ok(seen_banned_word)
}
