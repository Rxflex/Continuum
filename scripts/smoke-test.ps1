# Smoke test: start the daemon, connect over TCP, run the MCP handshake,
# then exercise initialize / tools/list / tools/call. No adapter involved --
# the adapter is a transparent pipe, so this covers the daemon end to end.

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$daemon = Join-Path $root "target\x86_64-pc-windows-gnullvm\debug\continuum-daemon.exe"
if (-not (Test-Path $daemon)) { Write-Output "FAIL: daemon binary missing"; exit 1 }

$ws = Join-Path $env:TEMP "continuum-smoke"
New-Item -ItemType Directory -Force $ws | Out-Null
Remove-Item -Recurse -Force (Join-Path $ws ".continuum") -ErrorAction SilentlyContinue

$proc = Start-Process $daemon -ArgumentList "--workspace", $ws, "--idle-minutes", "0" `
    -PassThru -WindowStyle Hidden

try {
    $lock = Join-Path $ws ".continuum\daemon.lock"
    $ready = $false
    for ($i = 0; $i -lt 50; $i++) {
        if (Test-Path $lock) { $ready = $true; break }
        Start-Sleep -Milliseconds 100
    }
    if (-not $ready) { Write-Output "FAIL: lockfile never appeared"; exit 1 }

    $lf = Get-Content $lock -Raw | ConvertFrom-Json
    Write-Output "lockfile: endpoint=$($lf.endpoint) protocol=$($lf.protocol_version)"
    $hostPort = $lf.endpoint.Split(":")

    $client = New-Object System.Net.Sockets.TcpClient
    $client.Connect($hostPort[0], [int]$hostPort[1])
    $stream = $client.GetStream()
    $writer = New-Object System.IO.StreamWriter($stream)
    $writer.NewLine = "`n"
    $reader = New-Object System.IO.StreamReader($stream)

    function Send($obj) { $writer.WriteLine($obj); $writer.Flush() }

    Send (@{ protocol_version = $lf.protocol_version; token = $lf.token } | ConvertTo-Json -Compress)
    Write-Output "handshake -> $($reader.ReadLine())"

    Send '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"1"}}}'
    Write-Output "initialize -> $($reader.ReadLine())"

    Send '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
    $tools = $reader.ReadLine()
    Write-Output "tools/list -> $($tools.Length) bytes"

    Send '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"store_architectural_decision","arguments":{"topic":"transport","description":"TCP loopback + token"}}}'
    Write-Output "store_decision -> $($reader.ReadLine())"

    Send '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"write_scratchpad","arguments":{"agent_id":"smoke","message":"hello from smoke test"}}}'
    Write-Output "write_scratchpad -> $($reader.ReadLine())"

    Send '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"read_scratchpad","arguments":{"limit":5}}}'
    Write-Output "read_scratchpad -> $($reader.ReadLine())"

    $client.Close()
    Write-Output "DONE"
}
finally {
    if (-not $proc.HasExited) { $proc.Kill() }
}
