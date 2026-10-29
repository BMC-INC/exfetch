#!/usr/bin/env python3
"""Test stdio using the official MCP Python SDK (test dependency: mcp==1.30.0)."""
import argparse
import asyncio
import importlib.metadata
import json
from pathlib import Path
import tempfile

from mcp import ClientSession, StdioServerParameters
from mcp.client.stdio import stdio_client


async def smoke(binary):
    with tempfile.TemporaryDirectory(prefix="exfetch-sdk-") as directory:
        parameters = StdioServerParameters(command=str(binary.resolve()),
            args=["serve", "--mcp-stdio", "--port", "0"],
            cwd=directory, env={"EXFETCH_CONFIG_DIR": directory})
        async with stdio_client(parameters) as (read, write):
            async with ClientSession(read, write) as client:
                initialized = await client.initialize()
                names = {tool.name for tool in (await client.list_tools()).tools}
                assert names == {"fetch_page", "search_web", "browser_action", "connection_status"}
                status = await client.call_tool("connection_status", {})
                assert not status.isError and "connected" in status.model_dump_json()
                result = await client.call_tool("fetch_page", {"url": "https://example.com",
                    "format": "text", "selector": "p", "use_browser": False})
                assert not result.isError
                assert "documentation examples" in result.model_dump_json()
                return {"ok": True, "client": "official MCP Python SDK",
                    "sdk_version": importlib.metadata.version("mcp"),
                    "protocol": initialized.protocolVersion,
                    "server": initialized.serverInfo.model_dump(), "tools": len(names),
                    "https_fetch_and_selector": True, "live_bridge_used": False}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(asyncio.run(asyncio.wait_for(smoke(args.binary), 60)), indent=2))
