use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use exfetch::bridge::connection::ConnectionManager;
use exfetch::bridge::ws_server;
use exfetch::cli::commands::{Cli, Commands};
use exfetch::config;
use exfetch::engine::policy::PolicyEngine;
use exfetch::fetch::http::{fetch_bytes, fetch_url};
use exfetch::fetch::pdf;
use exfetch::mcp;
use exfetch::output;
use exfetch::search;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Fetch(args) => {
            let timeout = Duration::from_secs(args.timeout);

            // Detect PDF by URL extension (before fetching)
            let is_pdf_url = args.url.to_lowercase().ends_with(".pdf");

            if is_pdf_url {
                // PDF path: fetch raw bytes and extract text
                match fetch_bytes(&args.url, timeout, &args.user_agent).await {
                    Ok(resp) => {
                        if args.verbose {
                            eprintln!(
                                "[exfetch] {} {} ({} ms)",
                                resp.status, resp.final_url, resp.fetch_time_ms
                            );
                            eprintln!("[exfetch] content-type: {}", resp.content_type);
                            eprintln!(
                                "[exfetch] mode: PDF extraction ({} bytes)",
                                resp.bytes.len()
                            );
                        }

                        match pdf::extract_text(&resp.bytes) {
                            Ok(text) => {
                                let out = output::text::format_raw(&text, args.max_length);
                                println!("{}", out);
                            }
                            Err(e) => {
                                eprintln!("error: PDF text extraction failed: {}", e);
                                std::process::exit(1);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                // Standard text/HTML path
                match fetch_url(&args.url, timeout, &args.user_agent).await {
                    Ok(resp) => {
                        // If the server responded with application/pdf content-type
                        // even though the URL didn't end in .pdf, re-fetch as bytes
                        if resp.content_type.to_lowercase().contains("application/pdf") {
                            if args.verbose {
                                eprintln!(
                                    "[exfetch] {} {} ({} ms)",
                                    resp.status, resp.final_url, resp.fetch_time_ms
                                );
                                eprintln!("[exfetch] content-type: {} (detected as PDF, re-fetching as bytes)", resp.content_type);
                            }

                            match fetch_bytes(&args.url, timeout, &args.user_agent).await {
                                Ok(bytes_resp) => match pdf::extract_text(&bytes_resp.bytes) {
                                    Ok(text) => {
                                        let out = output::text::format_raw(&text, args.max_length);
                                        println!("{}", out);
                                    }
                                    Err(e) => {
                                        eprintln!("error: PDF text extraction failed: {}", e);
                                        std::process::exit(1);
                                    }
                                },
                                Err(e) => {
                                    eprintln!("error: {}", e);
                                    std::process::exit(1);
                                }
                            }
                        } else {
                            if args.verbose {
                                eprintln!(
                                    "[exfetch] {} {} ({} ms)",
                                    resp.status, resp.final_url, resp.fetch_time_ms
                                );
                                eprintln!("[exfetch] content-type: {}", resp.content_type);
                            }

                            let out = if args.raw {
                                resp.body.clone()
                            } else if resp
                                .content_type
                                .to_lowercase()
                                .contains("application/json")
                            {
                                // JSON content: pretty-print directly without extraction
                                match serde_json::from_str::<serde_json::Value>(&resp.body) {
                                    Ok(parsed) => serde_json::to_string_pretty(&parsed)
                                        .unwrap_or_else(|_| resp.body.clone()),
                                    Err(_) => resp.body.clone(),
                                }
                            } else if args.json {
                                output::json::format(&resp, args.max_length)
                            } else if args.markdown {
                                output::markdown::format(&resp.body, args.max_length)
                            } else {
                                output::text::format(&resp.body, args.max_length)
                            };

                            println!("{}", out);
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Commands::Search(args) => {
            let timeout = Duration::from_secs(args.timeout);
            let num_results = args.results as usize;

            if args.fetch {
                // Search + fetch top results
                let fetch_count = std::cmp::min(3, num_results);
                match search::search_and_fetch(&args.query, num_results, fetch_count, timeout).await
                {
                    Ok(results) => {
                        if results.is_empty() {
                            if !args.quiet {
                                eprintln!("[exfetch] no results found for \"{}\"", args.query);
                            }
                        } else if args.json {
                            let json = serde_json::to_string_pretty(&results)
                                .unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e));
                            println!("{}", json);
                        } else {
                            println!("{}", search::format_fetched_results_text(&results));
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {}", e);
                        std::process::exit(1);
                    }
                }
            } else {
                // Search only (no fetch) -- select engine based on --engine flag
                let search_result = match args.engine.as_str() {
                    "searxng" => {
                        let instance_url =
                            args.searxng_url.as_deref().unwrap_or("https://searx.be");
                        search::engine::search_searxng(
                            &args.query,
                            num_results,
                            timeout,
                            instance_url,
                        )
                        .await
                    }
                    _ => {
                        // Default: DuckDuckGo
                        search::engine::search_ddg(&args.query, num_results, timeout).await
                    }
                };

                match search_result {
                    Ok(results) => {
                        if results.is_empty() {
                            if !args.quiet {
                                eprintln!("[exfetch] no results found for \"{}\"", args.query);
                            }
                        } else if args.json {
                            let json = serde_json::to_string_pretty(&results)
                                .unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e));
                            println!("{}", json);
                        } else {
                            println!("{}", search::format_results_text(&results));
                        }
                    }
                    Err(e) => {
                        eprintln!("error: {}", e);
                        std::process::exit(1);
                    }
                }
            }
        }
        Commands::Serve(args) => {
            // Daemon mode: fork to background before starting anything
            if args.daemon && !args.mcp_stdio {
                let config_dir = config::config_dir();
                std::fs::create_dir_all(&config_dir)?;

                let data_dir = dirs::data_local_dir()
                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                    .join("exfetch");
                std::fs::create_dir_all(&data_dir)?;

                let pid_file = config_dir.join("exfetch.pid");
                let log_file = data_dir.join("exfetch.log");

                let stdout = std::fs::File::create(&log_file)?;
                let stderr = stdout.try_clone()?;

                let daemonize = daemonize::Daemonize::new()
                    .pid_file(&pid_file)
                    .working_directory(".")
                    .stdout(stdout)
                    .stderr(stderr);

                match daemonize.start() {
                    Ok(_) => {
                        // We are now in the child process -- fall through to server startup
                    }
                    Err(e) => {
                        eprintln!("error: failed to daemonize: {}", e);
                        std::process::exit(1);
                    }
                }
            }

            let connections = ConnectionManager::new();
            let policy = Arc::new(PolicyEngine::new());

            if args.mcp_stdio {
                // Run MCP server over stdin/stdout
                mcp::server::run_stdio(connections, policy).await?;
            } else {
                // Start WebSocket server for browser extension bridge
                let token = ws_server::generate_token();
                let actual_port =
                    ws_server::start(args.port, token.clone(), connections.clone()).await?;

                eprintln!(
                    "[exfetch] WebSocket server listening on 127.0.0.1:{}",
                    actual_port
                );
                eprintln!("[exfetch] auth token: {}", token);

                // Optionally spawn SSE MCP server alongside the WebSocket server
                if let Some(sse_port) = args.mcp_sse {
                    let sse_conns = connections.clone();
                    let sse_policy = policy.clone();
                    tokio::spawn(async move {
                        if let Err(e) = mcp::server::run_sse(sse_port, sse_conns, sse_policy).await
                        {
                            eprintln!("[exfetch] SSE server error: {}", e);
                        }
                    });
                    eprintln!(
                        "[exfetch] MCP SSE server listening on 127.0.0.1:{}",
                        sse_port
                    );
                }

                // Wait for ctrl-c
                tokio::signal::ctrl_c().await?;
                eprintln!("\n[exfetch] shutting down");
            }
        }
        Commands::Status => {
            let pid_file = config::config_dir().join("exfetch.pid");
            if pid_file.exists() {
                match std::fs::read_to_string(&pid_file) {
                    Ok(contents) => {
                        let pid_str = contents.trim();
                        match pid_str.parse::<u32>() {
                            Ok(pid) => {
                                // Check if the process is still running
                                let sys = sysinfo::System::new_all();
                                let pid_val = sysinfo::Pid::from_u32(pid);
                                if sys.process(pid_val).is_some() {
                                    println!("exfetch daemon: running (PID {})", pid);
                                } else {
                                    println!("exfetch daemon: not running (stale PID file, PID {} not found)", pid);
                                }
                            }
                            Err(_) => {
                                println!("exfetch daemon: unknown (invalid PID file)");
                            }
                        }
                    }
                    Err(e) => {
                        println!("exfetch daemon: unknown (cannot read PID file: {})", e);
                    }
                }
            } else {
                println!("exfetch daemon: not running (no PID file)");
            }

            // Show port file if it exists
            let port_file = config::config_dir().join("port");
            if let Ok(port_str) = std::fs::read_to_string(&port_file) {
                println!("  WebSocket port: {}", port_str.trim());
            }

            println!("  version: {}", env!("CARGO_PKG_VERSION"));
        }
    }

    Ok(())
}
