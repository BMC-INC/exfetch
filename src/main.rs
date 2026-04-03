use anyhow::Result;
use clap::Parser;
use exfetch::cli::commands::{Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Fetch(args) => {
            println!("fetch: url={} timeout={}s", args.url, args.timeout);
            println!("  [placeholder] fetch not yet implemented");
        }
        Commands::Search(args) => {
            println!(
                "search: query=\"{}\" engine={} results={}",
                args.query, args.engine, args.results
            );
            println!("  [placeholder] search not yet implemented");
        }
        Commands::Serve(args) => {
            println!("serve: port={}", args.port);
            if args.mcp_stdio {
                println!("  mode: MCP over stdio");
            } else if let Some(sse_port) = args.mcp_sse {
                println!("  mode: MCP over SSE (port {})", sse_port);
            } else {
                println!("  mode: HTTP");
            }
            if args.daemon {
                println!("  daemon: true");
            }
            println!("  [placeholder] serve not yet implemented");
        }
        Commands::Status => {
            println!("exfetch status");
            println!("  [placeholder] status not yet implemented");
        }
    }

    Ok(())
}
