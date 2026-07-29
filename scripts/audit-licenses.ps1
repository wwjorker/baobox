# Read the license field of every dependency from the local registry cache.
# The point is catching GPL / AGPL: they are viral and would drag an MIT
# project into having to ship under the same terms.
$lock = Get-Content "F:\dev\baobox\src-tauri\Cargo.lock" -Raw
$reg = Get-ChildItem "C:\Users\wty\.cargo\registry\src" -Directory | Select-Object -First 1

$crates = @()
$name = $null
foreach ($line in ($lock -split "`r?`n")) {
    if ($line -match '^name = "(.+)"$') { $name = $Matches[1] }
    elseif ($line -match '^version = "(.+)"$') {
        if ($name) { $crates += [pscustomobject]@{ Name = $name; Ver = $Matches[1] }; $name = $null }
    }
}

$results = @()
foreach ($c in $crates) {
    $toml = Join-Path $reg.FullName "$($c.Name)-$($c.Ver)\Cargo.toml"
    $lic = "NOT-CACHED"
    if (Test-Path $toml) {
        $t = Get-Content $toml -Raw
        if ($t -match '(?m)^license\s*=\s*"([^"]+)"') { $lic = $Matches[1] }
        elseif ($t -match '(?m)^license-file\s*=\s*"([^"]+)"') { $lic = "FILE:" + $Matches[1] }
        else { $lic = "UNDECLARED" }
    }
    $results += [pscustomobject]@{ Name = $c.Name; Version = $c.Ver; License = $lic }
}

"Total dependencies: $($results.Count)"
""
"=== License distribution ==="
$results | Group-Object License | Sort-Object Count -Descending |
    Select-Object -First 20 | ForEach-Object { "{0,5}  {1}" -f $_.Count, $_.Name }
""
"=== Viral license check (GPL / AGPL / SSPL / CDDL / EUPL) ==="
$viral = $results | Where-Object { $_.License -match 'GPL|SSPL|CDDL|EUPL' }
if ($viral) {
    "FOUND $($viral.Count):"
    $viral | ForEach-Object { "   {0} {1} -> {2}" -f $_.Name, $_.Version, $_.License }
} else {
    "CLEAN - no viral licenses found"
}
""
"=== Undeclared / uncached (need manual check) ==="
$unknown = $results | Where-Object { $_.License -eq 'UNDECLARED' -or $_.License -eq 'NOT-CACHED' }
if ($unknown) {
    "{0} entries:" -f $unknown.Count
    $unknown | Select-Object -First 15 | ForEach-Object { "   {0} {1}  {2}" -f $_.Name, $_.Version, $_.License }
} else { "CLEAN - all declared" }

$results | Sort-Object Name | Export-Csv "F:\dev\baobox\THIRD-PARTY-LICENSES.csv" -NoTypeInformation -Encoding UTF8
""
"Inventory written to THIRD-PARTY-LICENSES.csv"
