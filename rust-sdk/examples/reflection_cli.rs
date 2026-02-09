//! CLI tool for exploring gRPC server reflection on the RFQv2 Ingestion Service.

use market_maker_client_sdk::reflection::ReflectionClient;
use std::process;

const DEFAULT_ENDPOINT: &str = "http://localhost:2408";

// ── Minimal arg parsing (no extra deps) ─────────────────────────────────────

struct Args {
    endpoint: String,
    command: Command,
}

enum Command {
    ListServices,
    DescribeService(String),
    DescribeMessage(String),
    Verify,
    Inspect,
    Help,
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    let mut endpoint = DEFAULT_ENDPOINT.to_string();
    let mut positional: Vec<String> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--endpoint" | "-e" => {
                i += 1;
                if i < args.len() {
                    endpoint = args[i].clone();
                } else {
                    eprintln!("Error: --endpoint requires a value");
                    process::exit(1);
                }
            }
            "--help" | "-h" => {
                return Args {
                    endpoint,
                    command: Command::Help,
                };
            }
            other => positional.push(other.to_string()),
        }
        i += 1;
    }

    let command = match positional.first().map(|s| s.as_str()) {
        Some("list-services" | "ls") => Command::ListServices,
        Some("describe-service" | "ds") => {
            let name = positional.get(1).cloned().unwrap_or_else(|| {
                eprintln!("Error: describe-service requires a service name");
                eprintln!("  Example: describe-service market_maker.MarketMakerIngestionService");
                process::exit(1);
            });
            Command::DescribeService(name)
        }
        Some("describe-message" | "dm") => {
            let name = positional.get(1).cloned().unwrap_or_else(|| {
                eprintln!("Error: describe-message requires a message name");
                eprintln!("  Example: describe-message market_maker.MarketMakerQuote");
                process::exit(1);
            });
            Command::DescribeMessage(name)
        }
        Some("verify") => Command::Verify,
        Some("inspect") => Command::Inspect,
        Some("help") => Command::Help,
        Some(unknown) => {
            eprintln!("Error: unknown command '{}'", unknown);
            eprintln!("Run with --help for usage information");
            process::exit(1);
        }
        None => Command::Help,
    };

    Args { endpoint, command }
}

fn print_help() {
    println!(
        r#"
RFQv2 Server Reflection CLI
============================

Explore gRPC services and message types exposed by the RFQv2 Ingestion Service
using server reflection.

USAGE:
    cargo run --example reflection_cli -- [OPTIONS] <COMMAND> [ARGS]

OPTIONS:
    -e, --endpoint <URL>    gRPC server endpoint (default: {DEFAULT_ENDPOINT})
    -h, --help              Print this help message

COMMANDS:
    list-services (ls)                        List all gRPC services on the server
    describe-service (ds) <SERVICE_NAME>      Show methods for a service
    describe-message (dm) <MESSAGE_NAME>      Show fields of a protobuf message
    verify                                    Verify MarketMakerIngestionService is available
    inspect                                   Full introspection of all services and methods
    help                                      Print this help message

EXAMPLES:
    # List all available services
    cargo run --example reflection_cli -- -e https://my-server.com list-services

    # Describe the MarketMaker service
    cargo run --example reflection_cli -- -e https://my-server.com ds market_maker.MarketMakerIngestionService

    # Inspect a message type
    cargo run --example reflection_cli -- -e https://my-server.com dm market_maker.MarketMakerQuote

    # Quick health-check: verify the expected service is present
    cargo run --example reflection_cli -- -e https://my-server.com verify

    # Dump everything the server exposes
    cargo run --example reflection_cli -- -e https://my-server.com inspect
"#
    );
}

// ── Commands ────────────────────────────────────────────────────────────────

async fn cmd_list_services(client: &ReflectionClient) {
    match client.list_services().await {
        Ok(services) => {
            println!("Services ({}):", services.len());
            for svc in &services {
                println!("  • {}", svc);
            }
        }
        Err(e) => {
            eprintln!("Error listing services: {}", e);
            process::exit(1);
        }
    }
}

async fn cmd_describe_service(client: &ReflectionClient, name: &str) {
    match client.get_service_info(name).await {
        Ok(info) => {
            println!("╭─ Service: {}", info.name);
            println!("│");
            for (i, method) in info.methods.iter().enumerate() {
                let prefix = if i + 1 == info.methods.len() {
                    "╰─"
                } else {
                    "├─"
                };
                let streaming = match (method.client_streaming, method.server_streaming) {
                    (true, true) => "  ⇄  bidirectional stream",
                    (true, false) => "  →  client stream",
                    (false, true) => "  ←  server stream",
                    (false, false) => "     unary",
                };
                println!(
                    "{} rpc {}({}) → ({}){streaming}",
                    prefix,
                    method.name,
                    short_type(&method.input_type),
                    short_type(&method.output_type),
                );
            }
        }
        Err(e) => {
            eprintln!("Error describing service '{}': {}", name, e);
            process::exit(1);
        }
    }
}

async fn cmd_describe_message(client: &ReflectionClient, name: &str) {
    match client.get_message_info(name).await {
        Ok(info) => {
            println!("message {} {{", info.name);
            for field in &info.fields {
                let label = if field.is_required {
                    "required"
                } else if field.is_repeated {
                    "repeated"
                } else {
                    "optional"
                };
                println!(
                    "  {} {} {} = {};",
                    label,
                    short_type(&field.type_name),
                    field.name,
                    field.number
                );
            }
            println!("}}");
        }
        Err(e) => {
            eprintln!("Error describing message '{}': {}", name, e);
            process::exit(1);
        }
    }
}

async fn cmd_verify(client: &ReflectionClient) {
    match client.verify_market_maker_service().await {
        Ok(info) => {
            println!("✅ MarketMakerIngestionService is available!");
            println!();
            println!("   Methods:");
            for method in &info.methods {
                let badge = match (method.client_streaming, method.server_streaming) {
                    (true, true) => "⇄ ",
                    (true, false) => "→ ",
                    (false, true) => "← ",
                    (false, false) => "  ",
                };
                println!("     {badge}{}", method.name);
            }
        }
        Err(e) => {
            eprintln!("❌ MarketMakerIngestionService NOT found: {}", e);
            process::exit(1);
        }
    }
}

async fn cmd_inspect(client: &ReflectionClient) {
    let services = match client.get_all_service_info().await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    };

    if services.is_empty() {
        println!("No services found (the server may not expose full reflection metadata).");
        // Fall back to just listing service names
        if let Ok(names) = client.list_services().await {
            println!("\nAdvertised service names:");
            for n in &names {
                println!("  • {}", n);
            }
        }
        return;
    }

    println!("Server Introspection");
    println!("====================\n");

    for svc in &services {
        println!("╭─ {}", svc.name);
        println!("│");
        for (i, method) in svc.methods.iter().enumerate() {
            let is_last = i + 1 == svc.methods.len();
            let (branch, cont) = if is_last {
                ("╰─", "  ")
            } else {
                ("├─", "│ ")
            };
            let streaming = match (method.client_streaming, method.server_streaming) {
                (true, true) => "[bidi-stream] ",
                (true, false) => "[client-stream] ",
                (false, true) => "[server-stream] ",
                (false, false) => "",
            };
            println!(
                "{branch} {streaming}{}",
                method.name
            );
            println!(
                "{cont}     request:  {}",
                short_type(&method.input_type)
            );
            println!(
                "{cont}     response: {}",
                short_type(&method.output_type)
            );
        }
        println!();
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Strips the leading dot and package prefix for display, e.g.
/// `.market_maker.MarketMakerQuote` → `MarketMakerQuote`
fn short_type(fq: &str) -> &str {
    fq.rsplit('.').next().unwrap_or(fq)
}

// ── Entry ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    // Initialise tracing (honour RUST_LOG, default to warn)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_target(false)
        .init();

    let args = parse_args();

    if matches!(args.command, Command::Help) {
        print_help();
        return;
    }

    println!("Connecting to {}…\n", args.endpoint);

    let client = match ReflectionClient::connect(&args.endpoint).await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to connect to {}: {}", args.endpoint, e);
            process::exit(1);
        }
    };

    match args.command {
        Command::ListServices => cmd_list_services(&client).await,
        Command::DescribeService(ref name) => cmd_describe_service(&client, name).await,
        Command::DescribeMessage(ref name) => cmd_describe_message(&client, name).await,
        Command::Verify => cmd_verify(&client).await,
        Command::Inspect => cmd_inspect(&client).await,
        Command::Help => unreachable!(),
    }
}
