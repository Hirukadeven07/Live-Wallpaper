<#
  Clawd's Ears: the helper for the Clawd Tunes wallpaper.

  It listens to Spotify, and only Spotify. Several times a second it asks Windows what Spotify is playing
  (song, artist, album cover, position, play or pause, shuffle, repeat) and how loud Spotify is in the
  volume mixer, and hands that to the wallpaper:

    - It writes clawd-tunes-data.js (and clawd-tunes-art.js for the cover) inside the wallpaper's folder in
      Lively's library. The wallpaper reads those files.
    - It answers the wallpaper on http://127.0.0.1:47823, on this PC only, so the player's buttons work:
      play, pause, skip, previous, shuffle, repeat, seek and Spotify's volume. Every request has to carry a
      random key that's only written into the wallpaper's folder, so web pages can't use it.

  It never sees your account, nothing leaves your PC, and it never changes anything you didn't click.

  Run it with "Start Clawd Ears.cmd". It sits in the system tray; right-click Clawd there to quit.
    -Also <folder>  also write the data files into this folder (for previewing the wallpaper in a browser)
    -Once           print one reading and exit
#>
param(
  [string[]]$Also = @(),
  [switch]$Once,
  [int]$Port = 47823
)
$ErrorActionPreference = 'Stop'
$Title = 'Clawd Tunes'
$DataFile = 'clawd-tunes-data.js'
$ArtFile = 'clawd-tunes-art.js'
$AppName = 'Spotify'

# ---------- Spotify's slider in the Windows volume mixer, and how loud it is right now ----------
Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Runtime.InteropServices;

namespace ClawdEars {
  [ComImport, Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")] class MMDeviceEnumeratorCo { }
  [ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IMMDeviceEnumerator {
    [PreserveSig] int EnumAudioEndpoints(int dataFlow, int stateMask, out IMMDeviceCollection devices);
  }
  [ComImport, Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IMMDeviceCollection {
    [PreserveSig] int GetCount(out int count);
    [PreserveSig] int Item(int index, out IMMDevice device);
  }
  [ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IMMDevice {
    [PreserveSig] int Activate(ref Guid iid, int clsCtx, IntPtr activationParams, [MarshalAs(UnmanagedType.IUnknown)] out object iface);
  }
  [ComImport, Guid("77AA99A0-1BD6-484F-8BC7-2C654C9A9B6F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IAudioSessionManager2 {
    void _GetAudioSessionControl(); void _GetSimpleAudioVolume();
    [PreserveSig] int GetSessionEnumerator(out IAudioSessionEnumerator sessions);
  }
  [ComImport, Guid("E2F5BB11-0570-40CA-ACDD-3AA01277DEE8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IAudioSessionEnumerator {
    [PreserveSig] int GetCount(out int count);
    [PreserveSig] int GetSession(int index, out IAudioSessionControl2 session);
  }
  // Only the slots we call are real; the rest keep the vtable order.
  [ComImport, Guid("BFB7FF88-7239-4FC9-8FA2-07C950BE9C6D"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IAudioSessionControl2 {
    [PreserveSig] int GetState(out int state);
    void _2(); void _3(); void _4(); void _5(); void _6(); void _7(); void _8(); void _9(); void _10(); void _11();
    [PreserveSig] int GetProcessId(out uint pid);
  }
  [ComImport, Guid("87CE5498-68D6-44E5-9215-6DA47EF883D8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface ISimpleAudioVolume {
    [PreserveSig] int SetMasterVolume(float level, ref Guid context);
    [PreserveSig] int GetMasterVolume(out float level);
    [PreserveSig] int SetMute(int mute, ref Guid context);
    [PreserveSig] int GetMute(out int mute);
  }
  [ComImport, Guid("C02216F6-8C67-4B5B-9D00-D008E73E0064"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IAudioMeterInformation {
    [PreserveSig] int GetPeakValue(out float peak);
  }

  public class Level { public bool Found; public float Volume = 1; public bool Mute; public float Peak; }

  public static class Mixer {
    static List<IAudioSessionControl2> sessions = new List<IAudioSessionControl2>();
    static DateTime scannedAt = DateTime.MinValue;
    static Dictionary<uint, string> names = new Dictionary<uint, string>();
    static Guid ctx = Guid.Empty;

    static string NameOf(uint pid) {
      string n;
      if (names.TryGetValue(pid, out n)) return n;
      try { n = Process.GetProcessById((int)pid).ProcessName; } catch { n = ""; }
      names[pid] = n;
      return n;
    }

    // Every audio session on every speaker, kept if it belongs to the app.
    static void Scan(string app) {
      sessions.Clear();
      scannedAt = DateTime.Now;
      if (names.Count > 400) names.Clear();
      var en = (IMMDeviceEnumerator)new MMDeviceEnumeratorCo();
      IMMDeviceCollection devs;
      if (en.EnumAudioEndpoints(0 /* render */, 1 /* active */, out devs) != 0) return;
      int n; devs.GetCount(out n);
      Guid iid = typeof(IAudioSessionManager2).GUID;
      for (int i = 0; i < n; i++) {
        IMMDevice d;
        if (devs.Item(i, out d) != 0) continue;
        object o;
        if (d.Activate(ref iid, 23 /* CLSCTX_ALL */, IntPtr.Zero, out o) != 0) continue;
        IAudioSessionEnumerator se;
        if (((IAudioSessionManager2)o).GetSessionEnumerator(out se) != 0) continue;
        int c; se.GetCount(out c);
        for (int k = 0; k < c; k++) {
          IAudioSessionControl2 s;
          if (se.GetSession(k, out s) != 0 || s == null) continue;
          uint pid;
          if (s.GetProcessId(out pid) < 0 || pid == 0) continue;
          if (string.Equals(NameOf(pid), app, StringComparison.OrdinalIgnoreCase)) sessions.Add(s);
        }
      }
    }

    public static Level Read(string app) {
      double age = (DateTime.Now - scannedAt).TotalSeconds;
      if (age > (sessions.Count == 0 ? 3 : 20)) { try { Scan(app); } catch { sessions.Clear(); } }
      var lv = new Level();
      foreach (var s in sessions) {
        try {
          int st; s.GetState(out st);
          if (st == 2) { scannedAt = DateTime.MinValue; continue; }   // expired: Spotify restarted
          var v = (ISimpleAudioVolume)s;
          float vol; int mute;
          v.GetMasterVolume(out vol); v.GetMute(out mute);
          float pk = 0;
          try { ((IAudioMeterInformation)s).GetPeakValue(out pk); } catch { }
          if (!lv.Found || st == 1) { lv.Volume = vol; lv.Mute = mute != 0; }
          lv.Found = true;
          if (pk > lv.Peak) lv.Peak = pk;
        } catch { scannedAt = DateTime.MinValue; }
      }
      return lv;
    }

    // volume < 0 leaves it alone; mute < 0 leaves it alone.
    public static bool Set(string app, float volume, int mute) {
      if (sessions.Count == 0) { try { Scan(app); } catch { } }
      bool any = false;
      foreach (var s in sessions) {
        try {
          var v = (ISimpleAudioVolume)s;
          if (volume >= 0) v.SetMasterVolume(Math.Min(1f, volume), ref ctx);
          if (mute >= 0) v.SetMute(mute, ref ctx);
          any = true;
        } catch { scannedAt = DateTime.MinValue; }
      }
      return any;
    }
  }
}
'@

# ---------- what Spotify is playing, from Windows' media controls ----------
Add-Type -AssemblyName System.Runtime.WindowsRuntime, System.Web
$null = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager, Windows.Media.Control, ContentType = WindowsRuntime]
$null = [Windows.Storage.Streams.IInputStream, Windows.Storage.Streams, ContentType = WindowsRuntime]
$null = [Windows.Media.MediaPlaybackAutoRepeatMode, Windows.Media, ContentType = WindowsRuntime]
$AsTask = [System.WindowsRuntimeSystemExtensions].GetMethods() |
  Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' } |
  Select-Object -First 1
function Wait-Op($op, [Type]$type, [int]$ms = 1500) {
  $task = $AsTask.MakeGenericMethod($type).Invoke($null, @($op))
  if (-not $task.Wait($ms)) { throw 'Windows took too long to answer' }
  return $task.Result
}
$MgrType = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]
$PropsType = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionMediaProperties]
$StreamType = [Windows.Storage.Streams.IRandomAccessStreamWithContentType]
$AsStream = [System.IO.WindowsRuntimeStreamExtensions].GetMethod('AsStreamForRead', [Type[]]@([Windows.Storage.Streams.IInputStream]))
$StatusNames = @{ 0 = 'closed'; 1 = 'opened'; 2 = 'changing'; 3 = 'stopped'; 4 = 'playing'; 5 = 'paused' }

function New-Key { -join ((1..16) | ForEach-Object { '{0:x2}' -f (Get-Random -Maximum 256) }) }
$M = @{
  Mgr = $null; Session = $null; Running = $false; RunningAt = [datetime]::MinValue
  Song = ''; Title = ''; Artist = ''; Album = ''; PropsAt = [datetime]::MinValue
  ArtId = ''; ArtSrc = ''; ArtHash = ''; ArtTries = 0; ArtFor = ''
  Play = $null; Level = $null; PeakMax = 0.0; Energy = 0.0
  Key = (New-Key); Http = $false
}

function Get-Hash([byte[]]$b) {
  [uint32]$h = 2166136261
  $step = [math]::Max(1, [math]::Floor($b.Length / 4096))
  for ($i = 0; $i -lt $b.Length; $i += $step) { $h = [uint32](([uint64]($h -bxor $b[$i]) * 16777619) % 4294967296) }
  return ('{0:x8}' -f $h) + $b.Length.ToString('x')
}
function Read-Art($props) {
  if (-not $props.Thumbnail) { return $null }
  $stream = Wait-Op ($props.Thumbnail.OpenReadAsync()) $StreamType
  # PowerShell only sees a bare COM object here, so .NET's stream adapter is called by reflection.
  $net = $AsStream.Invoke($null, @($stream))
  try {
    $ms = New-Object IO.MemoryStream
    $net.CopyTo($ms)
    if ($ms.Length -le 0 -or $ms.Length -gt 8MB) { return $null }
    return ,$ms.ToArray()
  } finally {
    $net.Dispose()
    try { ([IDisposable]$stream).Dispose() } catch { }
  }
}

function Find-Spotify {
  if (-not $M.Mgr) { $M.Mgr = Wait-Op ($MgrType::RequestAsync()) $MgrType }
  foreach ($s in $M.Mgr.GetSessions()) { if ([string]$s.SourceAppUserModelId -match 'spotify') { return $s } }
  return $null
}

# Song, cover, position and so on. Cheap parts every call; the song and cover about once a second.
function Read-Media([switch]$Full) {
  $now = Get-Date
  if (($now - $M.RunningAt).TotalSeconds -ge 2) {
    $M.Running = [bool](Get-Process -Name $AppName -ErrorAction SilentlyContinue)
    $M.RunningAt = $now
  }
  $s = $null
  if ($M.Running) { try { $s = Find-Spotify } catch { $M.Mgr = $null } }
  $M.Session = $s
  if (-not $s) { $M.Play = $null; $M.Song = ''; return }

  if ($Full -or ($now - $M.PropsAt).TotalSeconds -ge 1) {
    $M.PropsAt = $now
    $props = Wait-Op ($s.TryGetMediaPropertiesAsync()) $PropsType
    $M.Title = [string]$props.Title; $M.Artist = [string]$props.Artist; $M.Album = [string]$props.AlbumTitle
    $song = "$($M.Title)|$($M.Artist)|$($M.Album)"
    if ($song -ne $M.Song) { $M.Song = $song; $M.ArtTries = 0; $M.ArtFor = '' }
    # Spotify sometimes hands over the new song before its cover, so ask again a few times.
    if ($M.ArtFor -ne $song -and $M.ArtTries -lt 4) {
      $M.ArtTries++
      $bytes = $null
      try { $bytes = Read-Art $props } catch { $bytes = $null }
      if ($bytes) {
        $hash = Get-Hash $bytes
        if ($hash -ne $M.ArtHash -or $M.ArtTries -ge 3) {
          $mime = if ($bytes[0] -eq 0x89) { 'image/png' } else { 'image/jpeg' }
          $M.ArtHash = $hash; $M.ArtId = $hash; $M.ArtFor = $song
          $M.ArtSrc = "data:$mime;base64," + [Convert]::ToBase64String($bytes)
        }
      } elseif ($M.ArtTries -ge 4) { $M.ArtId = ''; $M.ArtSrc = ''; $M.ArtFor = $song }
    }
  }

  $pi = $s.GetPlaybackInfo()
  $tl = $s.GetTimelineProperties()
  $st = [int]$pi.PlaybackStatus
  $playing = $st -eq 4
  $dur = [math]::Max(0, $tl.EndTime.TotalSeconds - $tl.StartTime.TotalSeconds)
  $pos = $tl.Position.TotalSeconds
  if ($playing -and $tl.LastUpdatedTime.Year -gt 2000) { $pos += ([DateTimeOffset]::Now - $tl.LastUpdatedTime).TotalSeconds }
  if ($dur -gt 0) { $pos = [math]::Min($pos, $dur) }
  $c = $pi.Controls
  $rep = $null
  if ($null -ne $pi.AutoRepeatMode) { $rep = [int]$pi.AutoRepeatMode }
  $shuf = $null
  if ($null -ne $pi.IsShuffleActive) { $shuf = [bool]$pi.IsShuffleActive }
  $M.Play = [ordered]@{
    status = $StatusNames[$st]; playing = $playing; pos = [math]::Round([math]::Max(0, $pos), 2); dur = [math]::Round($dur, 2)
    shuffle = $shuf; repeat = $rep
    can = [ordered]@{ play = [bool]$c.IsPlayEnabled; pause = [bool]$c.IsPauseEnabled; next = [bool]$c.IsNextEnabled; prev = [bool]$c.IsPreviousEnabled;
                      seek = [bool]$c.IsPlaybackPositionEnabled; shuffle = [bool]$c.IsShuffleEnabled; repeat = [bool]$c.IsRepeatEnabled }
  }
}

# Spotify's loudness, sampled often so short beats aren't missed between the wallpaper's visits.
function Read-Level {
  try { $M.Level = [ClawdEars.Mixer]::Read($AppName) } catch { $M.Level = $null }
  $pk = if ($M.Level -and $M.Level.Found -and -not $M.Level.Mute) { [double]$M.Level.Peak } else { 0.0 }
  if ($pk -gt $M.PeakMax) { $M.PeakMax = $pk }
  $M.Energy = $M.Energy * 0.92 + $pk * 0.08
}

function Get-State([switch]$KeepPeak) {
  $d = [ordered]@{ v = 1; on = $true; key = $M.Key; port = $(if ($M.Http) { $Port } else { 0 }); running = $M.Running; has = [bool]$M.Session }
  if ($M.Session -and $M.Play) {
    $d.title = $M.Title; $d.artist = $M.Artist; $d.album = $M.Album; $d.art = $M.ArtId
    foreach ($k in $M.Play.Keys) { $d[$k] = $M.Play[$k] }
  }
  $lv = $M.Level
  if ($lv -and $lv.Found) { $d.vol = [math]::Round($lv.Volume, 3); $d.mute = $lv.Mute } else { $d.vol = $null; $d.mute = $false }
  $d.peak = [math]::Round($M.PeakMax, 3); $d.energy = [math]::Round($M.Energy, 3)
  if (-not $KeepPeak) { $M.PeakMax = 0.0 }
  $d.t = [DateTimeOffset]::Now.ToUnixTimeMilliseconds()
  return $d
}
function ConvertTo-JsonText($o) { ($o | ConvertTo-Json -Depth 4 -Compress) -replace [char]0x2028, ' ' -replace [char]0x2029, ' ' }

# ---------- the buttons on the player ----------
function Invoke-Button($q) {
  $s = $M.Session
  $what = [string]$q['do']
  if ($what -eq 'open') {
    if (-not $M.Running) { Start-Process 'spotify:' }
    return $true
  }
  if ($what -eq 'vol') { return [ClawdEars.Mixer]::Set($AppName, [float]([double]$q['v']), -1) }
  if ($what -eq 'mute') { return [ClawdEars.Mixer]::Set($AppName, -1, $(if ($q['on'] -eq '1') { 1 } else { 0 })) }
  if (-not $s) { return $false }
  $op = switch ($what) {
    'toggle'  { $s.TryTogglePlayPauseAsync() }
    'play'    { $s.TryPlayAsync() }
    'pause'   { $s.TryPauseAsync() }
    'next'    { $s.TrySkipNextAsync() }
    'prev'    { $s.TrySkipPreviousAsync() }
    'shuffle' { $s.TryChangeShuffleActiveAsync($q['on'] -eq '1') }
    'repeat'  { $s.TryChangeAutoRepeatModeAsync([Windows.Media.MediaPlaybackAutoRepeatMode][int]$q['mode']) }
    'seek'    { $s.TryChangePlaybackPositionAsync([long]([math]::Max(0, [double]$q['s']) * 10000000)) }
    default   { $null }
  }
  if (-not $op) { return $false }
  return [bool](Wait-Op $op ([bool]))
}

# ---------- a tiny web server on 127.0.0.1, for the wallpaper only ----------
$listener = $null
function Start-Http {
  try {
    $l = New-Object System.Net.Sockets.TcpListener ([System.Net.IPAddress]::Loopback), $Port
    $l.Start()
    $script:listener = $l
    $M.Http = $true
  } catch { $M.Http = $false; Write-Log "Can't open port $Port, so the player's buttons won't work: $($_.Exception.Message)" }
}
function Send-Http($ns, [int]$code, [string]$body) {
  $reason = @{ 200 = 'OK'; 204 = 'No Content'; 400 = 'Bad Request'; 403 = 'Forbidden'; 404 = 'Not Found' }[$code]
  $bytes = [Text.Encoding]::UTF8.GetBytes($body)
  $head = "HTTP/1.1 $code $reason`r`nContent-Type: application/json; charset=utf-8`r`nContent-Length: $($bytes.Length)`r`n" +
          "Access-Control-Allow-Origin: *`r`nAccess-Control-Allow-Methods: GET, OPTIONS`r`nAccess-Control-Allow-Headers: *`r`n" +
          "Access-Control-Allow-Private-Network: true`r`nCache-Control: no-store`r`nConnection: close`r`n`r`n"
  $hb = [Text.Encoding]::ASCII.GetBytes($head)
  $ns.Write($hb, 0, $hb.Length)
  if ($bytes.Length) { $ns.Write($bytes, 0, $bytes.Length) }
}
function Answer-Http($client) {
  $client.ReceiveTimeout = 500; $client.SendTimeout = 500
  $ns = $client.GetStream()
  $buf = New-Object byte[] 4096
  $req = ''
  while ($req.IndexOf("`r`n`r`n") -lt 0 -and $req.Length -lt 16384) {
    $n = $ns.Read($buf, 0, $buf.Length)
    if ($n -le 0) { break }
    $req += [Text.Encoding]::ASCII.GetString($buf, 0, $n)
  }
  $lines = $req -split "`r`n"
  if ($lines[0] -notmatch '^(GET|OPTIONS) (/\S*) HTTP/1\.[01]$') { Send-Http $ns 400 '{"ok":false}'; return }
  $method = $Matches[1]; $target = $Matches[2]
  # Only answer requests addressed to this PC by number or name (no DNS rebinding).
  $hostLine = $lines | Where-Object { $_ -match '^Host:\s*(\S+)' } | Select-Object -First 1
  if (-not $hostLine -or $hostLine -notmatch "^Host:\s*(127\.0\.0\.1|localhost):$Port\s*$") { Send-Http $ns 403 '{"ok":false}'; return }
  if ($method -eq 'OPTIONS') { Send-Http $ns 204 ''; return }
  $qi = $target.IndexOf('?')
  $path = if ($qi -ge 0) { $target.Substring(0, $qi) } else { $target }
  $q = [System.Web.HttpUtility]::ParseQueryString($(if ($qi -ge 0) { $target.Substring($qi + 1) } else { '' }))
  if ($q['key'] -ne $M.Key) { Send-Http $ns 403 '{"ok":false,"why":"key"}'; return }
  switch ($path) {
    '/state' { Send-Http $ns 200 (ConvertTo-JsonText (Get-State)) }
    '/art'   { Send-Http $ns 200 (ConvertTo-JsonText ([ordered]@{ id = $M.ArtId; src = $M.ArtSrc })) }
    '/cmd'   {
      $ok = $false
      try { $ok = Invoke-Button $q } catch { Write-Log "Button '$($q['do'])': $($_.Exception.Message)" }
      try { Read-Media -Full } catch { }
      Send-Http $ns 200 (ConvertTo-JsonText ([ordered]@{ ok = $ok; state = (Get-State) }))
    }
    default  { Send-Http $ns 404 '{"ok":false}' }
  }
}
function Serve-Http {
  if (-not $listener) { return }
  $n = 0
  while ($n -lt 8 -and $listener.Pending()) {
    $n++
    $c = $listener.AcceptTcpClient()
    try { Answer-Http $c } catch { } finally { $c.Close() }
  }
}

# ---------- which wallpaper folders to write to ----------
function Get-LivelyRoots {
  $roots = New-Object System.Collections.Generic.List[string]
  $settings = @("$env:LOCALAPPDATA\Lively Wallpaper\Settings.json")
  $settings += @(Get-ChildItem "$env:LOCALAPPDATA\Packages" -Directory -Filter '*LivelyWallpaper*' -ErrorAction SilentlyContinue |
                 ForEach-Object { Join-Path $_.FullName 'LocalCache\Local\Lively Wallpaper\Settings.json' })
  foreach ($s in $settings) {
    if (-not (Test-Path -LiteralPath $s)) { continue }
    $roots.Add((Join-Path (Split-Path $s) 'Library'))
    try { $d = (Get-Content -LiteralPath $s -Raw | ConvertFrom-Json).WallpaperDir; if ($d) { $roots.Add($d) } } catch { }
  }
  return $roots | Select-Object -Unique
}
function Test-OurFolder([string]$dir) {
  $info = Join-Path $dir 'LivelyInfo.json'
  if (-not (Test-Path -LiteralPath $info)) { return $false }
  try { return (Get-Content -LiteralPath $info -Raw | ConvertFrom-Json).Title -eq $Title } catch { return $false }
}
function Find-Targets {
  $found = @{}
  foreach ($r in Get-LivelyRoots) {
    Get-ChildItem -LiteralPath (Join-Path $r 'wallpapers') -Directory -ErrorAction SilentlyContinue |
      Where-Object { Test-OurFolder $_.FullName } | ForEach-Object { $found[$_.FullName.ToLowerInvariant()] = $_.FullName }
  }
  foreach ($p in @(Get-CimInstance Win32_Process -Filter "Name LIKE 'Lively.Player%'" -ErrorAction SilentlyContinue)) {
    if ([string]$p.CommandLine -notmatch '--wallpaper-url\s+"([^"]+)"') { continue }
    $dir = Split-Path $Matches[1]
    if (Test-OurFolder $dir) { $found[$dir.ToLowerInvariant()] = $dir }
  }
  foreach ($a in $Also) { if ($a) { $d = (Resolve-Path -LiteralPath $a).Path; $found[$d.ToLowerInvariant()] = $d } }
  return @($found.Values)
}

# ---------- writing the files ----------
$W = @{ Targets = @(); TargetsAt = [datetime]::MinValue; Last = @{}; WrittenAt = @{}; Art = @{} }
$utf8 = New-Object Text.UTF8Encoding $false
function Write-File([string]$file, [string]$text) {
  # Write a temp file and swap it in, so the wallpaper never reads half a file.
  # ([NullString]::Value, because PowerShell would pass $null to .NET as an empty path.)
  $tmp = "$file.tmp"
  try {
    [IO.File]::WriteAllText($tmp, $text, $utf8)
    if (Test-Path -LiteralPath $file) { [IO.File]::Replace($tmp, $file, [NullString]::Value) } else { [IO.File]::Move($tmp, $file) }
  } catch {
    try { [IO.File]::WriteAllText($file, $text, $utf8) } catch { }
    Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
  }
}
function Write-Data {
  if (((Get-Date) - $W.TargetsAt).TotalSeconds -ge 10) { $W.Targets = @(Find-Targets); $W.TargetsAt = Get-Date }
  if (-not $W.Targets.Count) { return }
  $state = Get-State -KeepPeak
  $body = ConvertTo-JsonText $state
  foreach ($dir in $W.Targets) {
    $key = $dir.ToLowerInvariant()
    if ($W.Art[$key] -ne $M.ArtId) {
      $W.Art[$key] = $M.ArtId
      $art = ConvertTo-JsonText ([ordered]@{ id = $M.ArtId; src = $M.ArtSrc })
      Write-File (Join-Path $dir $ArtFile) "window.clawdTunesArt && window.clawdTunesArt($art);`n"
    }
    Write-File (Join-Path $dir $DataFile) "window.clawdTunes && window.clawdTunes($body);`n"
  }
}

$LogFile = Join-Path $env:TEMP 'clawd-ears.log'
$LastError = ''
function Write-Log([string]$msg) {
  # Log each new problem once, so it's there to look at without filling the disk.
  if ($msg -eq $script:LastError) { return }
  $script:LastError = $msg
  try { Add-Content -LiteralPath $LogFile -Value "$(Get-Date -Format s)  $msg" } catch { }
}
function Step([scriptblock]$what) {
  try { & $what } catch { Write-Log "$($_.Exception.Message) (line $($_.InvocationInfo.ScriptLineNumber))" }
}

if ($Once) {
  Read-Level; Start-Sleep -Milliseconds 120; Read-Level
  Read-Media -Full
  $s = Get-State
  if ($M.ArtSrc) { $s.artBytes = [math]::Round($M.ArtSrc.Length * 0.75) }
  $s | ConvertTo-Json -Depth 4
  "wallpaper folders: " + ((Find-Targets) -join '; ')
  return
}

# ---------- one copy only ----------
$created = $false
$mutex = New-Object System.Threading.Mutex($true, 'Local\ClawdEarsForClawdTunes', [ref]$created)
if (-not $created) { return }

# ---------- tray icon ----------
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
function New-ClawdIcon {
  # Clawd with headphones on.
  $rows = @(
    '.....hhhhhh.....',
    '...hh......hh...',
    '..h..........h..',
    '..h.oooooooo.h..',
    '.pp.oooooooo.pp.',
    '.pp.okooooko.pp.',
    '.pp.okooooko.pp.',
    'oooooooooooooooo',
    'oooooooooooooooo',
    '...oooooooooo...',
    '...oooooooooo...',
    '...oooooooooo...',
    '....o.o..o.o....',
    '....o.o..o.o....',
    '....o.o..o.o....',
    '................')
  $bmp = New-Object System.Drawing.Bitmap 16, 16
  $col = @{ o = [System.Drawing.Color]::FromArgb(217, 119, 87); k = [System.Drawing.Color]::FromArgb(20, 20, 19)
            h = [System.Drawing.Color]::FromArgb(61, 64, 69); p = [System.Drawing.Color]::FromArgb(30, 215, 96) }
  for ($y = 0; $y -lt 16; $y++) { for ($x = 0; $x -lt 16; $x++) { $ch = [string]$rows[$y][$x]; if ($col.ContainsKey($ch)) { $bmp.SetPixel($x, $y, $col[$ch]) } } }
  return [System.Drawing.Icon]::FromHandle($bmp.GetHicon())
}
$shell = New-Object -ComObject WScript.Shell
$startup = Join-Path ([Environment]::GetFolderPath('Startup')) "Clawd's Ears.lnk"
function Set-StartWithWindows([bool]$on) {
  if ($on) {
    $l = $shell.CreateShortcut($startup)
    # conhost --headless keeps it windowless even when Windows Terminal is the default console.
    $l.TargetPath = "$env:WINDIR\System32\conhost.exe"
    $l.Arguments = "--headless `"$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe`" -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -STA -File `"$PSCommandPath`""
    $l.WorkingDirectory = $PSScriptRoot
    $l.Description = "Clawd's Ears, for the Clawd Tunes wallpaper"
    $l.Save()
  } elseif (Test-Path -LiteralPath $startup) { Remove-Item -LiteralPath $startup -Force }
}

$tray = New-Object System.Windows.Forms.NotifyIcon
$tray.Icon = New-ClawdIcon
$tray.Text = "Clawd's Ears"
$menu = New-Object System.Windows.Forms.ContextMenuStrip
$statusItem = $menu.Items.Add("Clawd's Ears: starting...")
$statusItem.Enabled = $false
$where = $menu.Items.Add('Wallpaper not found yet')
$where.Enabled = $false
[void]$menu.Items.Add('-')
$auto = New-Object System.Windows.Forms.ToolStripMenuItem 'Start with Windows'
$auto.Checked = Test-Path -LiteralPath $startup
$auto.add_Click({ $auto.Checked = -not $auto.Checked; try { Set-StartWithWindows $auto.Checked } catch { $auto.Checked = Test-Path -LiteralPath $startup } })
[void]$menu.Items.Add($auto)
$quit = $menu.Items.Add('Quit')
$tray.ContextMenuStrip = $menu
$tray.Visible = $true

Start-Http
$ticks = 0
$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 50
$timer.add_Tick({
  $script:ticks++
  Step { Serve-Http }
  Step { Read-Level }
  if ($ticks % 5 -eq 0) { Step { Read-Media } }
  if ($ticks % 20 -eq 0) {
    Step { Write-Data }
    $statusItem.Text = if (-not $M.Running) { 'Spotify is closed' } elseif (-not $M.Session) { 'Spotify is open, nothing playing' }
                   elseif ($M.Play -and $M.Play.playing) { 'Listening to Spotify' } else { 'Spotify is paused' }
    $where.Text = if ($W.Targets.Count) { 'Telling Clawd Tunes' } else { 'Add Clawd Tunes to Lively to start' }
    $tray.Text = "Clawd's Ears: " + $(if ($W.Targets.Count) { $statusItem.Text.ToLower() } else { 'waiting for the wallpaper' })
  }
})
$quit.add_Click({
  $timer.Stop()
  if ($listener) { try { $listener.Stop() } catch { } }
  # Tell the wallpaper we've gone.
  foreach ($dir in $W.Targets) {
    Write-File (Join-Path $dir $DataFile) "window.clawdTunes && window.clawdTunes({`"v`":1,`"on`":false,`"t`":$([DateTimeOffset]::Now.ToUnixTimeMilliseconds())});`n"
  }
  $tray.Visible = $false
  [System.Windows.Forms.Application]::Exit()
})
Step { Read-Media -Full }
Step { Write-Data }
$timer.Start()
$tray.ShowBalloonTip(4000, "Clawd's Ears is on", 'Clawd can hear Spotify now. Right-click him here to quit.', 'None')
[System.Windows.Forms.Application]::Run()
$mutex.ReleaseMutex()
