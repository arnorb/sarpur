# Spike 5: ffmpeg on Windows (sarpur plan, S2.0)
#
# Checks, on a real Windows PC:
#   1. the pinned BtbN ffmpeg build downloads, matches its SHA-256 and runs
#      (Microsoft Defender / SmartScreen don't block it);
#   2. ffmpeg writes to a path over 260 characters with Icelandic letters;
#   3. `-progress pipe:1` reports progress;
#   4. sending "q" stops ffmpeg cleanly and leaves a playable file;
#   5. closing a Job Object kills ffmpeg (how sarpur will stop it).
#
# Run in PowerShell:
#   powershell -ExecutionPolicy Bypass -File spike5.ps1
# Results are written to the Desktop as sarpur-spike5-results.json.
# Nothing is installed; everything goes in %TEMP%\sarpur-spike5.
#
# The script file is plain ASCII on purpose: Icelandic letters are built from
# character codes so Windows PowerShell 5.1 can't misread the file's encoding.

$ErrorActionPreference = 'Stop'

$FfmpegUrl    = 'https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-11-06-12-50/ffmpeg-n9.0.2-47-gb204e94e90-win64-gpl-9.0.zip'
$FfmpegSha256 = '8436c89156c3695659bf968e26bab5b08351ed8c86c3e9dbe3d6cc66a23af0a8'
# Vedur (weather), 7 Nov 2026 (pid bfcj7b). RUV removes episodes after a while; if this
# 404s, ask for a fresh URL.
$PlaylistBase = 'https://ruv-vod.akamaized.net/opid/5458120-AQMTO5'
$VideoUrl     = "$PlaylistBase/270p/stream.m3u8"
$AudioUrl     = "$PlaylistBase/ISL/stream.m3u8"

$Work    = Join-Path $env:TEMP 'sarpur-spike5'
$Desktop = [Environment]::GetFolderPath('Desktop')
$ResultFile = Join-Path $Desktop 'sarpur-spike5-results.json'
$Results = [ordered]@{
    started   = (Get-Date).ToString('o')
    windows   = (Get-CimInstance Win32_OperatingSystem | Select-Object -ExpandProperty Caption) + ' ' + [Environment]::OSVersion.Version.ToString()
    powershell = $PSVersionTable.PSVersion.ToString()
    long_paths_enabled = $null
    steps     = [ordered]@{}
}

function Save-Results { $Results | ConvertTo-Json -Depth 6 | Set-Content -Path $ResultFile -Encoding UTF8 }
function Step($name, [scriptblock]$body) {
    Write-Host "== $name" -ForegroundColor Cyan
    $started = Get-Date
    try {
        $value = & $body
        $Results.steps[$name] = [ordered]@{ ok = $true; seconds = [math]::Round(((Get-Date) - $started).TotalSeconds, 1); detail = $value }
        Write-Host "   OK" -ForegroundColor Green
    } catch {
        $Results.steps[$name] = [ordered]@{ ok = $false; seconds = [math]::Round(((Get-Date) - $started).TotalSeconds, 1); error = $_.Exception.Message }
        Write-Host "   FAILED: $($_.Exception.Message)" -ForegroundColor Red
    }
    Save-Results
}
function Long([string]$path) { if ($path.StartsWith('\\?\')) { $path } else { '\\?\' + $path } }

try {
    $Results.long_paths_enabled = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem' -Name LongPathsEnabled -ErrorAction Stop).LongPathsEnabled
} catch { $Results.long_paths_enabled = 'not set' }

New-Item -ItemType Directory -Force -Path $Work | Out-Null

# "Thattur med islenskum stofum" written with the real Icelandic letters
$Icelandic = [string][char]0x00DE + [char]0x00E1 + 'ttur me' + [char]0x00F0 + ' ' + [char]0x00ED + 'slenskum st' + [char]0x00F6 + 'fum'
$LongDir = $Work
while ($LongDir.Length -lt 250) { $LongDir = Join-Path $LongDir $Icelandic }
$LongDir = Join-Path $LongDir 'Season 01'
$FinalName = "$Icelandic - s01e299.mp4"
$FinalPath = Join-Path $LongDir $FinalName
$PartPath  = "$FinalPath.part"
$Results.long_path_length = $FinalPath.Length

$Ffmpeg = $null
$Ffprobe = $null

Step 'download_and_verify_ffmpeg' {
    $zip = Join-Path $Work 'ffmpeg.zip'
    if (-not (Test-Path $zip)) {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        $ProgressPreference = 'SilentlyContinue'
        Invoke-WebRequest -Uri $FfmpegUrl -OutFile $zip -UseBasicParsing
    }
    $hash = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
    if ($hash -ne $FfmpegSha256) { throw "SHA-256 mismatch: $hash" }
    $dest = Join-Path $Work 'ffmpeg'
    if (-not (Test-Path $dest)) { Expand-Archive -Path $zip -DestinationPath $dest }
    $script:Ffmpeg  = (Get-ChildItem -Recurse -Path $dest -Filter ffmpeg.exe | Select-Object -First 1).FullName
    $script:Ffprobe = (Get-ChildItem -Recurse -Path $dest -Filter ffprobe.exe | Select-Object -First 1).FullName
    "sha256 ok; $script:Ffmpeg"
}

Step 'ffmpeg_runs_under_defender' {
    if (-not $Ffmpeg) { throw 'ffmpeg not available' }
    $zone = Get-Content -Path "$Ffmpeg`:Zone.Identifier" -ErrorAction SilentlyContinue
    $version = & $Ffmpeg -hide_banner -version 2>&1 | Select-Object -First 1
    if ($LASTEXITCODE -ne 0) { throw "ffmpeg -version exit code $LASTEXITCODE" }
    [ordered]@{ version = "$version"; mark_of_the_web = [bool]$zone }
}

Step 'long_icelandic_path_with_progress' {
    if (-not $Ffmpeg) { throw 'ffmpeg not available' }
    New-Item -ItemType Directory -Force -Path (Long $LongDir) | Out-Null
    $progressLines = 0
    $ffArgs = @('-hide_banner', '-nostats', '-loglevel', 'error', '-progress', 'pipe:1', '-y',
              '-i', $VideoUrl, '-i', $AudioUrl, '-map', '0:v:0', '-map', '1:a:0',
              '-c', 'copy', '-bsf:a', 'aac_adtstoasc',
              '-metadata', "title=$Icelandic", '-metadata', 'comment=ruvinfo:bfcj7b:30766',
              '-f', 'mp4', $PartPath)
    & $Ffmpeg @ffArgs | ForEach-Object { if ($_ -like 'out_time_ms=*') { $progressLines++ } }
    $exit = $LASTEXITCODE
    if ($exit -ne 0) { throw "ffmpeg exit code $exit (path length $($PartPath.Length))" }
    Move-Item -LiteralPath (Long $PartPath) -Destination (Long $FinalPath) -Force
    if (-not (Test-Path -LiteralPath (Long $FinalPath))) { throw 'final file missing after rename' }
    $duration = & $Ffprobe -v error -show_entries format=duration -of csv=p=0 $FinalPath
    [ordered]@{ progress_lines = $progressLines; path_length = $FinalPath.Length; duration_seconds = "$duration" }
}

Step 'stop_with_q' {
    if (-not $Ffmpeg) { throw 'ffmpeg not available' }
    $out = Join-Path $Work 'stop-with-q.mp4'
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Ffmpeg
    $psi.Arguments = "-hide_banner -loglevel error -y -i `"$VideoUrl`" -i `"$AudioUrl`" -map 0:v:0 -map 1:a:0 -c copy -bsf:a aac_adtstoasc -f mp4 `"$out`""
    $psi.RedirectStandardInput = $true
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $p = [System.Diagnostics.Process]::Start($psi)
    Start-Sleep -Seconds 6
    $p.StandardInput.Write('q')
    $p.StandardInput.Flush()
    $stoppedInTime = $p.WaitForExit(15000)
    if (-not $stoppedInTime) { $p.Kill(); throw 'ffmpeg did not stop within 15 s of q' }
    $duration = & $Ffprobe -v error -show_entries format=duration -of csv=p=0 $out
    if (-not $duration) { throw 'output after q is not a readable MP4' }
    [ordered]@{ exit_code = $p.ExitCode; duration_seconds = "$duration" }
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class SpikeJob {
    [StructLayout(LayoutKind.Sequential)] struct BASIC { public long a, b; public uint LimitFlags; public UIntPtr c, d; public uint e; public UIntPtr f; public uint g, h; }
    [StructLayout(LayoutKind.Sequential)] struct IO { public ulong a, b, c, d, e, f; }
    [StructLayout(LayoutKind.Sequential)] struct EXT { public BASIC Basic; public IO Io; public UIntPtr p, j, pp, pj; }
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr CreateJobObject(IntPtr a, string n);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool SetInformationJobObject(IntPtr j, int c, ref EXT i, uint l);
    [DllImport("kernel32.dll", SetLastError = true)] static extern bool AssignProcessToJobObject(IntPtr j, IntPtr p);
    [DllImport("kernel32.dll", SetLastError = true)] public static extern bool CloseHandle(IntPtr h);
    public static IntPtr KillOnClose(IntPtr process) {
        IntPtr job = CreateJobObject(IntPtr.Zero, null);
        EXT info = new EXT();
        info.Basic.LimitFlags = 0x2000; // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if (!SetInformationJobObject(job, 9, ref info, (uint)Marshal.SizeOf(typeof(EXT)))) throw new System.ComponentModel.Win32Exception();
        if (!AssignProcessToJobObject(job, process)) throw new System.ComponentModel.Win32Exception();
        return job;
    }
}
'@

Step 'kill_with_job_object' {
    if (-not $Ffmpeg) { throw 'ffmpeg not available' }
    $out = Join-Path $Work 'job-object.mp4'
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Ffmpeg
    $psi.Arguments = "-hide_banner -loglevel error -y -i `"$VideoUrl`" -c copy -f mp4 `"$out`""
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $p = [System.Diagnostics.Process]::Start($psi)
    $job = [SpikeJob]::KillOnClose($p.Handle)
    Start-Sleep -Seconds 4
    [SpikeJob]::CloseHandle($job) | Out-Null
    $gone = $p.WaitForExit(5000)
    if (-not $gone) { $p.Kill(); throw 'ffmpeg still running 5 s after the job handle was closed' }
    [ordered]@{ exit_code = $p.ExitCode }
}

$Results.finished = (Get-Date).ToString('o')
Save-Results
Write-Host ''
Write-Host "Done. Please send this file back: $ResultFile" -ForegroundColor Yellow
Write-Host "You can delete $Work afterwards."
