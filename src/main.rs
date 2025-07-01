#![allow(dead_code)]
use std::fs::{create_dir, remove_dir_all};

use clap::{Parser, Subcommand};
use cli_util::{apply_changes, build_change_structures, process_diff_tree, start_hashmap_build};
use hash::hash;
use hashrules::HashRules;
use hashtab::{merge_hash_file, serialize_hashtab, HashTab, InvHashTab};
use slots::Slots;

#[path = "util/cli_util.rs"]
mod cli_util;
mod hash;
mod hashrules;
mod hashtab;
mod parser;
mod processor;
mod refcell_translation;
mod slots;
mod util;

/// qmldiff
/// A tool for applying diffs to QML trees
#[derive(Parser)]
#[command(
    name = "qmldiff",
    version = "0.1.0",
    author = "asivery",
    about = "A tool for applying diffs to QML trees"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create a hashtab for a given QML root path
    CreateHashtab {
        /// The root path of the QML
        qml_root_path: String,
        /// The name of the rules file to pa
        #[arg(default_value = None, required = false, long)]
        hashrules_name: Option<String>,
        /// The version of the QML environment to encode in hashtab
        #[arg(default_value = None, required = false, long)]
        version: Option<String>,
        /// The name of the hashtab to create
        #[arg(default_value = "hashtab")]
        hashtab_name: String,
    },
    /// Dump the contents of a hashtab in a human-readable form
    DumpHashtab {
        /// The path to the hashtab
        hashtab: String,
    },
    /// Hash a string
    HashString {
        /// The string to hash
        string: String,
    },
    /// Hash the diffs for a given hashtab
    HashDiffs {
        /// The hashtab to use
        hashtab: String,
        /// The list of diff files or directories
        #[arg(required = true)]
        diff_list: Vec<String>,
        /// Instead, revert the hashing operation
        #[arg(short, long, action = clap::ArgAction::SetTrue)]
        revert: bool,
    },
    /// Apply the diffs for a given hashtab and QML root path
    ApplyDiffs {
        /// The hashtab to use
        #[arg(long)]
        hashtab: Option<String>,
        /// The root path of the QML tree
        qml_root_path: String,
        /// The destination root path
        qml_destination_path: String,
        /// The list of diff files or directories
        diff_list: Vec<String>,
        /// Do not create subdirectories in destination
        #[arg(short, long, action = clap::ArgAction::SetTrue)]
        flatten: bool,
        /// Clean the destination
        #[arg(short, long, action = clap::ArgAction::SetTrue)]
        clean: bool,
        // /// Watch the diff_list for changes
        // #[arg(short, long, action = clap::ArgAction::SetTrue)]
        // watch_for_changes: bool,
        /// The QML environment version
        #[arg(default_value = None, required = false, long)]
        version: Option<String>,
    },
    /// Create the greatest-common-divisor of a list of hashtabs
    GCDHashtab {
        output_hashtab: String,
        hashtabs: Vec<String>,
    }
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::CreateHashtab {
            qml_root_path,
            hashtab_name,
            hashrules_name,
            version,
        } => {
            let mut hashtab = start_hashmap_build(qml_root_path);
            if let Some(hashrules) = hashrules_name {
                println!(
                    "Started processing hashtab rules from file {}...",
                    hashrules
                );
                let rules =
                    HashRules::compile(&std::fs::read_to_string(hashrules).unwrap()).unwrap();
                rules.process(&mut hashtab);
            }
            let hashtab_data = serialize_hashtab(&hashtab, version.clone());
            std::fs::write(hashtab_name, hashtab_data).unwrap()
        }
        Commands::DumpHashtab { hashtab } => {
            let mut tab = HashTab::new();
            merge_hash_file(hashtab, &mut tab, None, None).unwrap();
            for (i, v) in tab {
                println!("{} = {}", v, i);
            }
        }
        Commands::HashString { string } => {
            println!("hash({}) = {}", string, hash(string));
        }
        Commands::HashDiffs {
            hashtab,
            diff_list,
            revert,
        } => {
            let mut hashtab_value = HashTab::new();
            let mut inv_hashtab = InvHashTab::new();
            merge_hash_file(hashtab, &mut hashtab_value, None, Some(&mut inv_hashtab)).unwrap();
            process_diff_tree(diff_list, &hashtab_value, &inv_hashtab, !*revert);
        }
        Commands::ApplyDiffs {
            hashtab,
            qml_root_path,
            qml_destination_path,
            diff_list,
            flatten,
            clean,
            version,
        } => {
            let mut hashtab_value = HashTab::new();
            if let Some(hashtab) = hashtab {
                merge_hash_file(hashtab, &mut hashtab_value, None, None).unwrap();
            }
            if *clean {
                // Ignore result
                {
                    let _ = remove_dir_all(qml_destination_path);
                }
            }
            let _ = create_dir(qml_destination_path);
            let mut slots = Slots::new();
            let mut changes =
                build_change_structures(diff_list, &hashtab_value, &mut slots, version.clone())
                    .unwrap();
            slots.process_slots(&mut changes);
            apply_changes(
                qml_root_path,
                qml_destination_path,
                *flatten,
                &mut slots,
                &changes,
            )
            .unwrap();
            let not_read_slots: Vec<&String> = slots
                .0
                .iter()
                .filter_map(|e| {
                    if !e.1.read_back && !e.1.template {
                        Some(e.0)
                    } else {
                        None
                    }
                })
                .collect();
            if !not_read_slots.is_empty() {
                println!(
                    "Warning! {} slots have been written to, but never read from:",
                    not_read_slots.len(),
                );
                for slot in not_read_slots {
                    println!("- {}", slot);
                }
            }
        },
        Commands::GCDHashtab { output_hashtab, hashtabs } => {
            let mut out = HashTab::new();
            for file in hashtabs {
                let mut val = HashTab::new();
                merge_hash_file(file, &mut val, None, None).unwrap();
                if out.is_empty() {
                    out = val;
                } else {
                    out.retain(|key, _value| {
                        val.contains_key(key)
                    });
                }
            }
            std::fs::write(output_hashtab, serialize_hashtab(&out, None)).unwrap();
        }
    }
}
