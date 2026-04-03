use clap::{Parser, Subcommand};

/// exfetch — Browser-aware web execution layer for LLM agents
#[derive(Parser, Debug)]
#[command(name = "exfetch", version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Fetch a URL and extract content
    Fetch(FetchArgs),

    /// Search the web using a search engine
    Search(SearchArgs),

    /// Start the exfetch server (HTTP / MCP)
    Serve(ServeArgs),

    /// Show exfetch runtime status
    Status,
}

#[derive(Parser, Debug)]
pub struct FetchArgs {
    /// URL to fetch
    pub url: String,

    /// Output raw HTML without processing
    #[arg(long)]
    pub raw: bool,

    /// Output as Markdown
    #[arg(short = 'm', long)]
    pub markdown: bool,

    /// Output as JSON
    #[arg(short = 'j', long)]
    pub json: bool,

    /// Force browser-based fetching
    #[arg(short = 'b', long)]
    pub browser: bool,

    /// Disable browser-based fetching
    #[arg(long)]
    pub no_browser: bool,

    /// CSS selector to extract specific elements
    #[arg(long)]
    pub selector: Option<String>,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 15)]
    pub timeout: u64,

    /// Maximum content length (bytes)
    #[arg(long)]
    pub max_length: Option<usize>,

    /// User-Agent header
    #[arg(long, default_value = "exfetch/0.1")]
    pub user_agent: String,

    /// Suppress non-essential output
    #[arg(short = 'q', long)]
    pub quiet: bool,

    /// Enable verbose output
    #[arg(short = 'v', long)]
    pub verbose: bool,
}

#[derive(Parser, Debug)]
pub struct SearchArgs {
    /// Search query
    pub query: String,

    /// Also fetch top result content
    #[arg(short = 'f', long)]
    pub fetch: bool,

    /// Number of results to return
    #[arg(long, default_value_t = 5)]
    pub results: u32,

    /// Search engine to use
    #[arg(long, default_value = "duckduckgo")]
    pub engine: String,

    /// SearXNG instance URL (used when engine is "searxng")
    #[arg(long)]
    pub searxng_url: Option<String>,

    /// Output as JSON
    #[arg(short = 'j', long)]
    pub json: bool,

    /// Output as Markdown
    #[arg(short = 'm', long)]
    pub markdown: bool,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 15)]
    pub timeout: u64,

    /// Suppress non-essential output
    #[arg(short = 'q', long)]
    pub quiet: bool,
}

#[derive(Parser, Debug)]
pub struct ServeArgs {
    /// Port to listen on
    #[arg(long, default_value_t = 9876)]
    pub port: u16,

    /// Run as MCP server over stdio
    #[arg(long)]
    pub mcp_stdio: bool,

    /// Run as MCP server over SSE on the given port
    #[arg(long)]
    pub mcp_sse: Option<u16>,

    /// Run as a background daemon
    #[arg(short = 'd', long)]
    pub daemon: bool,
}
