<#
.SYNOPSIS
  End-to-end MCP check with a real agent: `claude -p` builds 3 connected shapes on a temporary board through `inkboard-mcp`,
  then `ink info` checks the resulting file.

.DESCRIPTION
  WARNING: this calls the Claude API (`claude -p`), which is PAID. It is never run by CI nor by `cargo test`; run it by hand.

  Steps: create a temporary folder and an empty board, (optionally) open the application on it, write an MCP configuration that
  points at `inkboard-mcp --root <temp folder>`, run `claude -p` with a fixed instruction, then run `ink info` on the board and
  require at least 3 shapes and 2 connectors. The temporary folder is deleted at the end (kept with -Keep, or on failure).

  Prerequisites: `claude` (Claude Code CLI, logged in) on the PATH, and a build of the workspace:
      cargo build --release -p inkboard-app -p inkboard-cli -p inkboard-mcp

.PARAMETER BinDir
  Folder with inkboard.exe, ink.exe, inkboard-mcp.exe. Default: <repo>\target\release.

.PARAMETER NoApp
  Do not start the application: the agent works in file mode (the file is written by `inkboard-mcp`). Without this switch the
  application is started on the board (live mode) and the agent must call `save_board` for the file to be written.

.PARAMETER Model
  Model passed to `claude --model` (default: the CLI default).

.PARAMETER Keep
  Keep the temporary folder.

.EXAMPLE
  # file mode, no window
  .\scripts\mcp_e2e_claude.ps1 -NoApp

.EXAMPLE
  # live mode against the real application
  .\scripts\mcp_e2e_claude.ps1 -Model sonnet

.NOTES
  Exit code: 0 = the board has >= 3 shapes and >= 2 connectors; 1 = check failed; 2 = prerequisite missing.
#>
[CmdletBinding()]
param(
    [string]$BinDir = (Join-Path (Split-Path $PSScriptRoot -Parent) 'target\release'),
    [switch]$NoApp,
    [string]$Model,
    [switch]$Keep
)
$ErrorActionPreference = 'Stop'

$app = Join-Path $BinDir 'inkboard.exe'
$ink = Join-Path $BinDir 'ink.exe'
$mcp = Join-Path $BinDir 'inkboard-mcp.exe'
$needed = @($ink, $mcp)
if (-not $NoApp) { $needed += $app }
foreach ($f in $needed) {
    if (-not (Test-Path $f)) { Write-Error "Missing $f (cargo build --release -p inkboard-app -p inkboard-cli -p inkboard-mcp)"; exit 2 }
}
if (-not (Get-Command claude -ErrorAction SilentlyContinue)) { Write-Error 'claude (Claude Code CLI) not found on the PATH'; exit 2 }

$dir = Join-Path ([IO.Path]::GetTempPath()) ("inkboard-e2e-" + [Guid]::NewGuid().ToString('N').Substring(0, 8))
New-Item -ItemType Directory -Force $dir | Out-Null
$dir = (Resolve-Path $dir).Path
$board = Join-Path $dir 'e2e.inkboard.json'
$proc = $null
$ok = $false

try {
    # empty board, created by the MCP server itself (one request, then EOF ends the server)
    $init = '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"create_board","arguments":{"path":"e2e.inkboard.json","name":"E2E"}}}'
    $init | & $mcp --root $dir --no-live | Out-Null
    if (-not (Test-Path $board)) { throw "Could not create $board" }

    if (-not $NoApp) {
        $proc = Start-Process -FilePath $app -ArgumentList "`"$board`"" -PassThru
        Start-Sleep -Seconds 5   # the application publishes live.json once its window is up
    }

    # MCP configuration for this run only (--strict-mcp-config ignores the user's own servers)
    $cfg = Join-Path $dir 'mcp.json'
    $mcpArgs = @('--root', $dir)
    if ($NoApp) { $mcpArgs += '--no-live' }
    @{ mcpServers = @{ inkboard = @{ command = $mcp; args = $mcpArgs } } } | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 $cfg

    $save = if ($NoApp) { '' } else { ' Finish by calling save_board.' }
    $prompt = "Using only the inkboard MCP tools, on the board $board create 3 shapes connected by connectors (A to B, B to C), each with a short text.$save Do not do anything else."

    $cl = @('-p', $prompt, '--mcp-config', $cfg, '--strict-mcp-config', '--allowedTools', 'mcp__inkboard', '--permission-mode', 'default', '--max-turns', '15')
    if ($Model) { $cl += @('--model', $Model) }
    Write-Host "Running claude -p (paid call)..."
    & claude @cl
    if ($LASTEXITCODE -ne 0) { throw "claude exited with code $LASTEXITCODE" }

    # check the result with the CLI
    $out = & $ink info $board
    Write-Host $out
    $line = $out | Where-Object { $_ -match '(\d+) shape\(s\), (\d+) connector\(s\)' } | Select-Object -First 1
    if (-not $line) { throw 'ink info: no page summary found' }
    $null = $line -match '(\d+) shape\(s\), (\d+) connector\(s\)'
    $shapes = [int]$Matches[1]; $conns = [int]$Matches[2]
    if ($shapes -lt 3 -or $conns -lt 2) { throw "Expected >= 3 shapes and >= 2 connectors, got $shapes and $conns" }
    Write-Host "OK: $shapes shape(s), $conns connector(s)"
    $ok = $true
}
catch {
    Write-Host "FAILED: $_" -ForegroundColor Red
}
finally {
    if ($proc -and -not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    if ($ok -and -not $Keep) { Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue }
    else { Write-Host "Kept: $dir" }
}
exit ($(if ($ok) { 0 } else { 1 }))
