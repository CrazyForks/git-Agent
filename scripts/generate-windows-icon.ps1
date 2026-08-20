param(
    [string]$Source = "assets/icons/logo-ga.png",
    [string]$Destination = "assets/icons/git-agent.ico"
)

$ErrorActionPreference = "Stop"
$png = [System.IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Source))

# ICO supports PNG-compressed images. Keep the source artwork lossless and embed
# its 256x256 variant directly so Explorer, installer shortcuts, and the taskbar
# can all select a crisp icon.
$header = [byte[]](0, 0, 1, 0, 1, 0)
$entry = New-Object byte[] 16
$entry[0] = 0 # 0 means 256 pixels in an ICO directory entry.
$entry[1] = 0
$entry[2] = 0
$entry[3] = 0
[System.BitConverter]::GetBytes([uint16]1).CopyTo($entry, 4)
[System.BitConverter]::GetBytes([uint16]32).CopyTo($entry, 6)
[System.BitConverter]::GetBytes([uint32]$png.Length).CopyTo($entry, 8)
[System.BitConverter]::GetBytes([uint32]22).CopyTo($entry, 12)

$ico = New-Object byte[] ($header.Length + $entry.Length + $png.Length)
$header.CopyTo($ico, 0)
$entry.CopyTo($ico, $header.Length)
$png.CopyTo($ico, $header.Length + $entry.Length)
[System.IO.File]::WriteAllBytes($Destination, $ico)
