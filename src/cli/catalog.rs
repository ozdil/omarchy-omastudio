//! `catalog list|query|cull` CLI subcommands backed by SQLite (WAL).

use omastudio_engine::recipe::{Catalog, CatalogFilter};

use super::{ResponseWrapper, print_json};

pub fn run(args: &[String]) {
    if args.is_empty() || args[0] == "list" {
        // Entries of photos that are offline stay in the database, but are not listed
        let cat = Catalog::load();
        let listed = Catalog {
            version: cat.version,
            items: cat.present_items(),
            unreadable: false,
        };
        print_json(&ResponseWrapper::ok(listed));
    } else if args[0] == "query" {
        let mut filter = CatalogFilter::default();
        let mut idx = 1;
        while idx < args.len() {
            match args[idx].as_str() {
                "--rating" if idx + 1 < args.len() => {
                    filter.min_rating = args[idx + 1].parse().ok();
                    idx += 2;
                }
                "--flag" if idx + 1 < args.len() => {
                    filter.flag = Some(args[idx + 1].clone());
                    idx += 2;
                }
                "--search" if idx + 1 < args.len() => {
                    filter.search_query = Some(args[idx + 1].clone());
                    idx += 2;
                }
                "--model" if idx + 1 < args.len() => {
                    filter.camera_model = Some(args[idx + 1].clone());
                    idx += 2;
                }
                _ => idx += 1,
            }
        }
        match Catalog::query_indexed(&filter) {
            Ok(results) => print_json(&ResponseWrapper::ok(results)),
            Err(_) => {
                let cat = Catalog::load();
                let results = cat.filter_items(&filter);
                print_json(&ResponseWrapper::ok(results));
            }
        }
    } else if args[0] == "cull" {
        if args.len() < 2 {
            print_json::<()>(&ResponseWrapper::err(
                "Usage: catalog cull <path> [--rating <0-5>] [--flag <pick|reject>] [--color <label>]",
            ));
            return;
        }
        let path = &args[1];
        let mut rating = None;
        let mut flag = None;
        let mut color = None;
        let mut idx = 2;
        while idx < args.len() {
            match args[idx].as_str() {
                "--rating" if idx + 1 < args.len() => {
                    rating = args[idx + 1].parse().ok();
                    idx += 2;
                }
                "--flag" if idx + 1 < args.len() => {
                    flag = Some(args[idx + 1].as_str());
                    idx += 2;
                }
                "--color" if idx + 1 < args.len() => {
                    color = Some(args[idx + 1].as_str());
                    idx += 2;
                }
                _ => idx += 1,
            }
        }
        match Catalog::update_culling_indexed(path, rating, flag, color) {
            Ok(true) => print_json(&ResponseWrapper::ok("Culling metadata saved")),
            Ok(false) => print_json::<()>(&ResponseWrapper::err("Item not found in catalog")),
            Err(e) => print_json::<()>(&ResponseWrapper::err(format!("Cannot write the catalogue: {e}"))),
        }
    } else {
        print_json::<()>(&ResponseWrapper::err("Unknown catalog subcommand: list, query, cull"));
    }
}
