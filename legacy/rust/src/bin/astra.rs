use research_cli::app::App;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // The Pi launcher is the product runtime. This binary remains only for the
    // migration window and Rust conformance tests; keep the boundary explicit.
    if args.first().map(String::as_str) == Some("migration") {
        println!(
            "{{\"runtime\":\"pi\",\"legacy_runtime\":\"rust\",\"status\":\"compatibility-only\"}}"
        );
        return;
    }
    if std::env::var_os("ASTRA_LEGACY_SILENCE").is_none() {
        eprintln!(
            "astra: legacy Rust runtime; use `npm run astra -- ...` for the Pi-native runtime"
        );
    }

    let app = App::new();

    let code = match app.run(&args) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("{err}");
            1
        }
    };

    std::process::exit(code);
}
