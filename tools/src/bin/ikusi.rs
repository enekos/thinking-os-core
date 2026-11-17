use os::ikusi::{self, api};
use os::kg::{Ontology, Paths};

const USAGE: &str = "\
ikusi — the thinking-os graph viewer, served

  ikusi                        ensure the server is up, then open the browser
  ikusi open [node]            the same, focused on one node
  ikusi serve [--port N]       run the server in the foreground
  ikusi url [node]             print the deep link, start nothing
  ikusi status                 is it answering?

  --port N     default 4747, or $IKUSI_PORT
  --root DIR   the wiki root, default: the one containing graph/ontology.yaml
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    };
    let positional: Vec<&String> = {
        let mut out = Vec::new();
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--port" || args[i] == "--root" {
                i += 2;
                continue;
            }
            if !args[i].starts_with('-') {
                out.push(&args[i]);
            }
            i += 1;
        }
        out
    };

    let command = positional.first().map(|s| s.as_str()).unwrap_or("open");
    let target = match command {
        "open" | "url" => positional.get(1).map(|s| s.as_str()),
        _ => None,
    };
    let root = value("--root");
    let port = value("--port").and_then(|p| p.parse().ok()).unwrap_or_else(ikusi::port);

    let code = match command {
        "serve" => serve(root.as_deref(), port),
        "url" => {
            println!("{}", ikusi::node_url(port, target));
            0
        }
        "status" => {
            if ikusi::is_up(port, std::time::Duration::from_millis(600)) {
                println!("up   {}", ikusi::base(port));
                0
            } else {
                println!("down {}", ikusi::base(port));
                1
            }
        }
        "open" => match ikusi::ensure_up(port, root.as_deref()) {
            Ok(started) => {
                let url = ikusi::node_url(port, target);
                if started {
                    eprintln!("ikusi: started on {}", ikusi::base(port));
                }
                match std::process::Command::new(ikusi::opener()).arg(&url).status() {
                    Ok(s) if s.success() => 0,
                    _ => {
                        println!("{url}");
                        0
                    }
                }
            }
            Err(e) => {
                eprintln!("ikusi: {e}");
                1
            }
        },
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            0
        }
        other => {
            eprintln!("ikusi: unknown command {other}\n\n{USAGE}");
            1
        }
    };
    std::process::exit(code);
}

fn serve(root: Option<&str>, port: u16) -> i32 {
    let paths = Paths::resolve(root);
    let ont = match Ontology::load(&paths.ontology()) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("ikusi: {e}");
            return 1;
        }
    };
    let live = api::Live::new(paths, ont);
    {
        let snap = live.current();
        eprintln!(
            "ikusi: {} nodes, {} edges from {} — http://127.0.0.1:{port}/",
            snap.store.nodes.len(),
            snap.store.edges.len(),
            snap.store.root.display(),
        );
    }
    match api::app(live).run_graceful(&format!("127.0.0.1:{port}")) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("ikusi: cannot listen on 127.0.0.1:{port}: {e}");
            1
        }
    }
}
