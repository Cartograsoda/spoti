# Installs (or with -Remove, uninstalls) the portable Windows Terminal that spoti reopens itself in.
# Run through `spoti setup-window`, which passes the parameters.
param(
    [Parameter(Mandatory)] [string] $InstallDir,
    [Parameter(Mandatory)] [string] $Exe,
    [switch] $Remove
)
$ErrorActionPreference = 'Stop'
# The progress bar slows Invoke-WebRequest down by an order of magnitude in Windows PowerShell.
$ProgressPreference = 'SilentlyContinue'

$name = [IO.Path]::GetFileNameWithoutExtension($Exe)
$terminalDir = Join-Path $InstallDir 'terminal'
$terminal = Join-Path $terminalDir 'WindowsTerminal.exe'
$icon = Join-Path $InstallDir 'spoti.ico'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Spoti.lnk'

if ($Remove) {
    if (Get-Process WindowsTerminal -ErrorAction SilentlyContinue | Where-Object Path -eq $terminal) {
        throw 'Close the spoti window first, its terminal is still running.'
    }
    Remove-Item $shortcut -ErrorAction SilentlyContinue
    Remove-Item $InstallDir -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "Removed. $name runs in whatever terminal you start it from again."
    return
}

if (Test-Path $terminal) {
    Write-Host 'Portable Windows Terminal is already installed, keeping it.'
} else {
    $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'x64' }
    $release = Invoke-RestMethod 'https://api.github.com/repos/microsoft/terminal/releases/latest'
    $asset = $release.assets | Where-Object name -like "Microsoft.WindowsTerminal_*_$arch.zip" | Select-Object -First 1
    if (-not $asset) { throw "Windows Terminal $($release.tag_name) has no portable $arch build." }
    Write-Host "Downloading Windows Terminal $($release.tag_name) ($([math]::Round($asset.size / 1MB)) MB) from Microsoft's GitHub releases..."

    $download = Join-Path $InstallDir 'download'
    Remove-Item $download, $terminalDir -Recurse -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory $download | Out-Null
    $zip = Join-Path $download 'terminal.zip'
    Invoke-WebRequest $asset.browser_download_url -OutFile $zip -UseBasicParsing
    Expand-Archive $zip -DestinationPath $download
    $extracted = Get-ChildItem $download -Directory | Select-Object -First 1

    $signature = Get-AuthenticodeSignature (Join-Path $extracted.FullName 'WindowsTerminal.exe')
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
        Remove-Item $download -Recurse -Force
        throw 'The download is not signed by Microsoft, not installing it.'
    }
    Move-Item $extracted.FullName $terminalDir
    Remove-Item $download -Recurse -Force
    # Portable mode keeps settings next to the exe instead of sharing the Store Terminal's.
    New-Item -ItemType File (Join-Path $terminalDir '.portable') | Out-Null
    New-Item -ItemType Directory (Join-Path $terminalDir 'settings') | Out-Null
}

if (-not (Test-Path $icon)) {
    # A green circle with a note, stored as a PNG inside a single-image .ico.
    Add-Type -AssemblyName System.Drawing
    $bitmap = New-Object System.Drawing.Bitmap 256, 256
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    $graphics.SmoothingMode = 'AntiAlias'
    $graphics.TextRenderingHint = 'AntiAliasGridFit'
    $graphics.FillEllipse((New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(30, 215, 96))), 8, 8, 240, 240)
    $font = New-Object System.Drawing.Font 'Segoe UI Symbol', 130, ([System.Drawing.FontStyle]::Bold), ([System.Drawing.GraphicsUnit]::Pixel)
    $format = New-Object System.Drawing.StringFormat
    $format.Alignment = 'Center'
    $format.LineAlignment = 'Center'
    $graphics.DrawString([string][char]0x266B, $font, [System.Drawing.Brushes]::Black, (New-Object System.Drawing.RectangleF 0, 8, 256, 256), $format)
    $png = New-Object IO.MemoryStream
    $bitmap.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
    $data = $png.ToArray()
    $writer = New-Object IO.BinaryWriter ([IO.File]::Create($icon))
    # ICONDIR, then one ICONDIRENTRY (a 0 size means 256 px) pointing at the PNG right after it.
    $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]1)
    $writer.Write([byte[]](0, 0, 0, 0)); $writer.Write([uint16]1); $writer.Write([uint16]32)
    $writer.Write([uint32]$data.Length); $writer.Write([uint32]22)
    $writer.Write($data)
    $writer.Close()
}

# Windows matches running windows to a pinned shortcut by the exe it points at, so pinning this
# one puts the green icon on the portable Terminal's taskbar button.
# Without -p, Terminal only picks a profile whose command line matches, so the window would get
# the plain default colours instead of the user's.
$profileArg = ''
$settings = Join-Path $env:LOCALAPPDATA 'Packages\Microsoft.WindowsTerminal_8wekyb3d8bbwe\LocalState\settings.json'
if ((Test-Path $settings) -and ((Get-Content $settings -Raw) -match '"defaultProfile"\s*:\s*"([^"]+)"')) {
    $profileArg = "-p `"$($Matches[1])`" "
}
$link = (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut)
$link.TargetPath = $terminal
$link.Arguments = "-w new new-tab --title Spoti --suppressApplicationTitle $profileArg`"$Exe`" --in-place"
$link.WorkingDirectory = $env:USERPROFILE
$link.IconLocation = "$icon,0"
$link.Save()

Write-Host "Done. Start $name from Windows Terminal and it opens in its own window, with its own taskbar button."
Write-Host 'For the green icon: Start menu > Spoti > right-click > Pin to taskbar.'
Write-Host "To undo: $name setup-window --remove"
