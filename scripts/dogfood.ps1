# Dogfood evaluation: drive the daemon through a realistic agent exploration
# session and print each tool result with its size, to judge real usefulness.

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$daemon = Join-Path $root "target\x86_64-pc-windows-gnullvm\debug\continuum-daemon.exe"

Remove-Item -Recurse -Force (Join-Path $root ".continuum") -ErrorAction SilentlyContinue
$proc = Start-Process $daemon -ArgumentList "--workspace", $root, "--idle-minutes", "0" `
    -PassThru -WindowStyle Hidden
try {
    $lock = Join-Path $root ".continuum\daemon.lock"
    for ($i = 0; $i -lt 100; $i++) { if (Test-Path $lock) { break }; Start-Sleep -Milliseconds 50 }
    $lf = Get-Content $lock -Raw | ConvertFrom-Json
    $hp = $lf.endpoint.Split(":")
    $client = New-Object System.Net.Sockets.TcpClient
    $client.Connect($hp[0], [int]$hp[1])
    $stream = $client.GetStream()
    $w = New-Object System.IO.StreamWriter($stream); $w.NewLine = "`n"
    $r = New-Object System.IO.StreamReader($stream)
    function Send($o) { $w.WriteLine($o); $w.Flush() }

    Send (@{ protocol_version = $lf.protocol_version; token = $lf.token } | ConvertTo-Json -Compress)
    $r.ReadLine() | Out-Null
    Send '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}'
    $r.ReadLine() | Out-Null
    Start-Sleep -Seconds 4  # let indexing, model load, and back-fill settle

    $script:id = 1
    function Tool($name, $arguments) {
        $script:id++
        Send (@{ jsonrpc = "2.0"; id = $script:id; method = "tools/call";
                 params = @{ name = $name; arguments = $arguments } } | ConvertTo-Json -Compress -Depth 10)
        $resp = $r.ReadLine() | ConvertFrom-Json
        $text = $resp.result.content[0].text
        Write-Output ("=== {0} {1}  ->  {2} chars" -f $name, (($arguments | ConvertTo-Json -Compress)), $text.Length)
        Write-Output $text
        Write-Output ""
    }

    Tool "get_stats" @{}
    Tool "search_code" @{ query = "rank and score symbols by relevance"; limit = 5 }
    Tool "find_callers" @{ symbol_name = "tokenize" }
    Tool "get_local_graph" @{ symbol_name = "index_workspace"; depth = 2 }
    Tool "commit_intent" @{ agent_id = "claude"; intent = "evaluated the Continuum tools"; files_touched = @("scripts/dogfood.ps1") }
    Tool "get_recent_changes" @{ limit = 3 }

    $client.Close()
}
finally {
    if (-not $proc.HasExited) { $proc.Kill() }
    Remove-Item -Recurse -Force (Join-Path $root ".continuum") -ErrorAction SilentlyContinue
}
