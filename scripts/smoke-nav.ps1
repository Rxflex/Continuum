# Navigation smoke test: point the daemon at the Continuum repo itself, let it
# index, then exercise the code-navigation tools against real parsed symbols.

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$daemon = Join-Path $root "target\x86_64-pc-windows-gnullvm\debug\continuum-daemon.exe"
if (-not (Test-Path $daemon)) { Write-Output "FAIL: daemon binary missing"; exit 1 }

Remove-Item -Recurse -Force (Join-Path $root ".continuum") -ErrorAction SilentlyContinue
$proc = Start-Process $daemon -ArgumentList "--workspace", $root, "--idle-minutes", "0" `
    -PassThru -WindowStyle Hidden

try {
    $lock = Join-Path $root ".continuum\daemon.lock"
    $ready = $false
    for ($i = 0; $i -lt 50; $i++) {
        if (Test-Path $lock) { $ready = $true; break }
        Start-Sleep -Milliseconds 100
    }
    if (-not $ready) { Write-Output "FAIL: lockfile never appeared"; exit 1 }

    $lf = Get-Content $lock -Raw | ConvertFrom-Json
    $hostPort = $lf.endpoint.Split(":")
    $client = New-Object System.Net.Sockets.TcpClient
    $client.Connect($hostPort[0], [int]$hostPort[1])
    $stream = $client.GetStream()
    $writer = New-Object System.IO.StreamWriter($stream)
    $writer.NewLine = "`n"
    $reader = New-Object System.IO.StreamReader($stream)
    function Send($obj) { $writer.WriteLine($obj); $writer.Flush() }

    Send (@{ protocol_version = $lf.protocol_version; token = $lf.token } | ConvertTo-Json -Compress)
    $reader.ReadLine() | Out-Null

    # Give the background indexer time to finish scanning the repo.
    Start-Sleep -Seconds 2

    Send '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"get_file_outline","arguments":{"path":"crates/continuum-graph/src/graph.rs"}}}'
    Write-Output "=== get_file_outline graph.rs ==="
    Write-Output $reader.ReadLine()

    Send '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_symbol_definition","arguments":{"symbol_name":"resolve_calls"}}}'
    Write-Output "=== get_symbol_definition resolve_calls ==="
    Write-Output $reader.ReadLine()

    Send '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"find_callers","arguments":{"symbol_name":"insert_node"}}}'
    Write-Output "=== find_callers insert_node ==="
    Write-Output $reader.ReadLine()

    Send '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"search_code","arguments":{"query":"resolve calls graph","limit":5}}}'
    Write-Output "=== search_code 'resolve calls graph' ==="
    Write-Output $reader.ReadLine()

    $client.Close()
    Write-Output "DONE"
}
finally {
    if (-not $proc.HasExited) { $proc.Kill() }
}
