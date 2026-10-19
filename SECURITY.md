# Security policy

Inkboard is in alpha. Please report vulnerabilities **privately** using GitHub's *Report a vulnerability* button (Security tab → *Advisories*), not in a public issue.

Areas that deserve particular attention:

- **The MCP server** (`inkboard-mcp`) reads and writes files for an AI agent. It confines every path to `--root` and only writes `.inkboard` (or an existing `.inkboard.json` / `.tableau.json`) and export formats; a way around that confinement is a vulnerability.
- **The live-mode local server** inside the app listens on `127.0.0.1` only, behind a random token stored in the per-user data directory; a way to use it without the token is a vulnerability.
- **File import** (draw.io, Excalidraw, JSON Canvas, Mermaid, SVG, images): a crafted file that crashes the app or reads files is a vulnerability.

There is no bug bounty. We will acknowledge reports and fix confirmed issues as quickly as a small project can.
