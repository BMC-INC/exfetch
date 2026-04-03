use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use exfetch::cli::commands::{Cli, Commands};
use exfetch::fetch::http::fetch_url;
use exfetch::output;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Fetch(args) => {
            let timeout = Duration::from_secs(args.timeout);
            match fetch_url(&args.url, timeout, &args.user_agent).await {
                Ok(resp) => {
                    if args.verbose {
                        eprintln!(
                            "[exfetch] {} {} ({} ms)",
                            resp.status, resp.final_url, resp.fetch_time_ms
                        );
                        eprintln!("[exfetch] content-type: {}", resp.content_type);
                    }

                    let out = if args.raw {
                        resp.body.clone()
                    } else if args.json {
                        output::json::format(&resp, args.max_length)
                    } else if args.markdown {
                        output::markdown::format(&resp.body, args.max_length)
                    } else {
                        output::text::format(&resp.body, args.max_length)
                    };

                    println!("{}", out);
                }
                Err(e) => {
                    eprintln!("error: {}", e);
                    std::process::exit(1);
                }
            }
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
