use os::kg::{Ontology, Paths};
use os::sare::{extract, fuse, retype, serve, viz};

const USAGE: &str = "\
sare — the thinking-os graph pipeline

  sare build [pages|local]        extract + fuse + serve (local: pages + git, no network)
  sare extract [all|pages|git|github|linear]   (linear joins `all` once os.yaml has a `linear:` key)
  sare fuse [--strict]
  sare serve
  sare hop <node> [-d N]
  sare why <node>
  sare retype [page] [--apply]
  sare viz [--out FILE] [--dot] [--iterations N]
  sare --version
";

fn extract_all(paths: &Paths, ont: &Ontology, which: &str) -> i32 {
    let order: Vec<&str> = if which == "all" {
        // Linear is opt-in (`linear:` in os.yaml); naming it explicitly always runs it.
        let mut all = vec!["pages", "git", "github"];
        if paths.linear().is_some() {
            all.push("linear");
        }
        all
    } else if which == "local" {
        vec!["pages", "git"]
    } else {
        vec![which]
    };
    let mut problems: Vec<String> = Vec::new();
    let mut repo_ids: Vec<String> = Vec::new();

    for name in order {
        let pull = match name {
            "pages" => extract::pages(paths, ont),
            "git" => {
                let p = extract::git(paths, ont);
                repo_ids = p.nodes.iter().map(|n| n.id.clone()).collect();
                p
            }
            "github" => {
                if repo_ids.is_empty() {
                    repo_ids = os::kg::load_raw(paths, "git").nodes.iter().map(|n| n.id.clone()).collect();
                }
                extract::github(paths, ont, &repo_ids)
            }
            "linear" => extract::linear(paths, ont),
            other => {
                eprintln!("sare: unknown extractor: {other}");
                return 1;
            }
        };
        let tail = if pull.problems.is_empty() {
            String::new()
        } else {
            format!("  {} problems", pull.problems.len())
        };
        println!("{name:8} {:5} nodes  {:5} edges{tail}", pull.nodes.len(), pull.edges.len());
        problems.extend(pull.problems);
    }

    if !problems.is_empty() {
        println!("\nproblems:");
        for p in problems.iter().take(40) {
            println!("  - {p}");
        }
        if problems.len() > 40 {
            println!("  ... {} more", problems.len() - 40);
        }
    }
    0
}

fn run_fuse(paths: &Paths, ont: &Ontology, strict: bool, quiet: bool) -> i32 {
    let fused = fuse::fuse(paths, ont);
    if let Err(e) = fuse::write(paths, &fused) {
        eprintln!("sare: cannot write the graph: {e}");
        return 1;
    }
    if quiet {
        println!("nodes {}   edges {}   rejected {}", fused.nodes.len(), fused.edges.len(), fused.rejected.len());
        return 0;
    }
    fuse::report(ont, &fused, strict)
}

fn run_serve(paths: &Paths, ont: &Ontology) -> i32 {
    match serve::write_views(paths, ont) {
        Ok(written) => {
            for (path, lines) in written {
                println!("wrote {path:28} {lines:5} lines");
            }
            0
        }
        Err(e) => {
            eprintln!("sare: cannot write views: {e}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().cloned().unwrap_or_default();
    if matches!(command.as_str(), "--version" | "-V") {
        println!("sare {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let mut positional: Vec<String> = Vec::new();
    let mut i = 1;
    while i < args.len() {
        let token = &args[i];
        if token == "--root" || token == "-d" || token == "--out" || token == "--iterations" {
            i += 2;
            continue;
        }
        if !token.starts_with('-') {
            positional.push(token.clone());
        }
        i += 1;
    }
    let flag = |name: &str| args.iter().any(|a| a == name);
    let value = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    let paths = Paths::resolve(value("--root").as_deref());
    let ont = match Ontology::load(&paths.ontology()) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("sare: {e}");
            std::process::exit(1);
        }
    };

    let code = match command.as_str() {
        "build" => {
            let which = positional.first().map(String::as_str).unwrap_or("all");
            let which = match which {
                "pages" => "pages",
                "local" => "local",
                _ => "all",
            };
            let mut code = extract_all(&paths, &ont, which);
            if code == 0 {
                code = run_fuse(&paths, &ont, false, false);
            }
            if code == 0 {
                code = run_serve(&paths, &ont);
            }
            code
        }
        "extract" => {
            let which = positional.first().map(String::as_str).unwrap_or("all");
            extract_all(&paths, &ont, which)
        }
        "fuse" => run_fuse(&paths, &ont, flag("--strict"), flag("--quiet")),
        "serve" => run_serve(&paths, &ont),
        "hop" => {
            let Some(node) = positional.first() else {
                eprintln!("sare: hop needs a node id");
                std::process::exit(1);
            };
            let depth = value("-d").and_then(|d| d.parse().ok()).unwrap_or(2);
            serve::Graph::load(&paths).hop(node, depth)
        }
        "why" => {
            let Some(node) = positional.first() else {
                eprintln!("sare: why needs a node id");
                std::process::exit(1);
            };
            serve::Graph::load(&paths).why(node)
        }
        "viz" => {
            let iterations = value("--iterations").and_then(|n| n.parse().ok()).unwrap_or(260);
            viz::run(&paths, value("--out").as_deref(), flag("--dot"), iterations)
        }
        "retype" => {
            let only = positional.first().map(|s| os::kg::slug(s));
            retype::run(&paths, &ont, only.as_deref(), flag("--apply"))
        }
        "help" | "--help" | "-h" | "" => {
            print!("{USAGE}");
            0
        }
        other => {
            eprintln!("sare: unknown command {other}\n\n{USAGE}");
            1
        }
    };
    std::process::exit(code);
}
