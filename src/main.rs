use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use exfetch::bridge::connection::ConnectionManager;
use exfetch::bridge::ws_server;
use exfetch::cli::commands::{Cli, Commands};
use exfetch::config;
use exfetch::engine::policy::PolicyEngine;
use exfetch::fetch::http::{fetch_bytes_with_policy, fetch_url_with_policy};
use exfetch::fetch::pdf;
use exfetch::mcp;
use exfetch::output;
use exfetch::search;

#[tokio::main]
async fn main() -> Result<()> {
    // Logs must go to stderr: stdout carries JSON-RPC in `serve --mcp-stdio`.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let cli = Cli::parse();
    let config = config::load()?;

    match cli.command {
        Commands::Fetch(args) => {
            let timeout = Duration::from_secs(args.timeout.unwrap_or(config.defaults.timeout));
            let user_agent = args
                .user_agent
                .as_deref()
                .unwrap_or(&config.defaults.user_agent);
            let policy = PolicyEngine::from_config(&config.policy);
            if let exfetch::engine::policy::PolicyVerdict::Block(reason) =
                policy.check_domain(&args.url)
            {
                anyhow::bail!(reason);
            }
            if args.browser {
                let mut response = ws_server::fetch_from_running_bridge(&args.url, timeout).await?;
                if let Some(selector) = args.selector.as_deref() {
                    response.body = exfetch::extract::select_html(&response.body, selector)
                        .map_err(anyhow::Error::msg)?;
                }
                let result = if args.raw {
                    output::text::format_raw(&response.body, args.max_length)
                } else if args.json {
                    output::json::format(&response, args.max_length)
                } else if args.markdown {
                    output::markdown::format(&response.body, args.max_length)
                } else {
                    output::text::format(&response.body, args.max_length)
                };
                println!("{}", result);
                return Ok(());
            }

            // Detect PDF by URL extension (before fetching)
            let is_pdf_url = args.url.to_lowercase().ends_with(".pdf");

            if is_pdf_url {
                // PDF path: fetch raw bytes and extract text
                match fetch_bytes_with_policy(&args.url, timeout, user_agent, &policy).await {
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
                match fetch_url_with_policy(&args.url, timeout, user_agent, &policy).await {
                    Ok(mut resp) => {
                        if let Some(selector) = args.selector.as_deref() {
                            resp.body = exfetch::extract::select_html(&resp.body, selector)
                                .map_err(anyhow::Error::msg)?;
                        }
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

                            match fetch_bytes_with_policy(&args.url, timeout, user_agent, &policy)
                                .await
                            {
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
            let timeout = Duration::from_secs(args.timeout.unwrap_or(config.defaults.timeout));
            let num_results = args.results as usize;
            let engine = args
                .engine
                .as_deref()
                .unwrap_or(&config.defaults.search_engine);
            let results = match engine {
                "searxng" => {
                    let instance = args.searxng_url.as_deref().or(config.defaults.searxng_url.as_deref())
                        .ok_or_else(|| anyhow::anyhow!("Set --searxng-url or defaults.searxng_url to your SearXNG instance"))?;
                    search::engine::search_searxng(&args.query, num_results, timeout, instance)
                        .await?
                }
                "duckduckgo" => {
                    search::engine::search_ddg(&args.query, num_results, timeout).await?
                }
                _ => anyhow::bail!("Unknown search engine: {}", engine),
            };
            if results.is_empty() && !args.quiet {
                eprintln!("[exfetch] no results found for {:?}", args.query);
            }
            let output = if args.fetch {
                let policy = PolicyEngine::from_config(&config.policy);
                let results = search::fetch_search_results_with_policy(
                    results,
                    std::cmp::min(3, num_results),
                    timeout,
                    Some(&policy),
                )
                .await?;
                if args.json {
                    serde_json::to_string_pretty(&results)?
                } else if args.markdown {
                    search::format_fetched_results_markdown(&results)
                } else {
                    search::format_fetched_results_text(&results)
                }
            } else if args.json {
                serde_json::to_string_pretty(&results)?
            } else if args.markdown {
                search::format_results_markdown(&results)
            } else {
                search::format_results_text(&results)
            };
            println!("{}", output);
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
            let policy = Arc::new(PolicyEngine::from_config(&config.policy));

            if args.mcp_stdio {
                // The extension bridge must run in-process: browser tools resolve
                // against this process's ConnectionManager. Best-effort, so a busy
                // port range never takes the MCP server down with it.
                let token = ws_server::generate_token();
                match ws_server::start_with_policy(
                    args.port,
                    token,
                    connections.clone(),
                    policy.clone(),
                )
                .await
                {
                    Ok(actual_port) => eprintln!(
                        "[exfetch] WebSocket bridge listening on 127.0.0.1:{}",
                        actual_port
                    ),
                    Err(e) => eprintln!("[exfetch] WebSocket bridge unavailable: {}", e),
                }

                // Run MCP server over stdin/stdout
                mcp::server::run_stdio(connections, policy).await?;
            } else {
                // Start WebSocket server for browser extension bridge
                let token = ws_server::generate_token();
                let actual_port = ws_server::start_with_policy(
                    args.port,
                    token.clone(),
                    connections.clone(),
                    policy.clone(),
                )
                .await?;

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
