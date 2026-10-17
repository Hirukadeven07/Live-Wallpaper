<#
  Clawd's Eyes: the helper for the Clawd Desk Stations wallpaper.

  About once a second it asks Windows for the names and grid positions of your desktop icons,
  works out what kind of app each one is (browser, music, game, code, chat, notes, art, video,
  Recycle Bin...), and writes that to clawd-eyes-data.js inside the wallpaper's folder in Lively's
  library. The wallpaper reads that file to see which app you dropped into which slot.

  It only reads. It never moves, opens or changes your icons, and nothing leaves your PC.
  File and folder names are never written out, only app names.

  Run it with "Start Clawd Eyes.cmd". It sits in the system tray; right-click Clawd there to quit.
    -Also <folder>  also write the data file into this folder (for previewing the wallpaper in a browser)
    -Once           print one reading and exit
#>
param(
  [string[]]$Also = @(),
  [switch]$Once
)
$ErrorActionPreference = 'Stop'
$Title = 'Clawd Desk Stations'
$DataFile = 'clawd-eyes-data.js'

Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

namespace ClawdEyes {
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  struct MONITORINFOEX {
    public int cbSize; public RECT rcMonitor; public RECT rcWork; public int dwFlags;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string szDevice;
  }
  [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
  struct DISPLAY_DEVICE {
    public int cb;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 32)] public string DeviceName;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string DeviceString;
    public int StateFlags;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string DeviceID;
    [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)] public string DeviceKey;
  }

  // Only the slots we call are real; the rest keep the vtable order.
  [ComImport, Guid("85CB6900-4D95-11CF-960C-0080C7F4EE85"), InterfaceType(ComInterfaceType.InterfaceIsDual)]
  interface IShellWindows {
    void _1(); void _2(); void _3(); void _4(); void _5(); void _6(); void _7(); void _8();
    [return: MarshalAs(UnmanagedType.IDispatch)]
    object FindWindowSW([MarshalAs(UnmanagedType.Struct)] ref object loc, [MarshalAs(UnmanagedType.Struct)] ref object root,
                        int swClass, out int hwnd, int options);
  }
  [ComImport, Guid("6D5140C1-7436-11CE-8034-00AA006009FA"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IOleServiceProvider {
    [PreserveSig] int QueryService(ref Guid sid, ref Guid iid, [MarshalAs(UnmanagedType.IUnknown)] out object obj);
  }
  [ComImport, Guid("000214E2-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IShellBrowser {
    void _1(); void _2(); void _3(); void _4(); void _5(); void _6(); void _7(); void _8(); void _9(); void _10(); void _11(); void _12();
    [return: MarshalAs(UnmanagedType.IUnknown)] object QueryActiveShellView();
  }
  [ComImport, Guid("000214E3-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IShellView { IntPtr GetWindow(); }
  [ComImport, Guid("1af3a467-214f-4298-908e-06b03e0b39f9"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IFolderView2 {
    void _GetCurrentViewMode(); void _SetCurrentViewMode();
    [return: MarshalAs(UnmanagedType.IUnknown)] object GetFolder(ref Guid iid);
    IntPtr Item(int i);
    int ItemCount(uint flags);
    void _Items(); void _GetSelectionMarkedItem(); void _GetFocusedItem();
    POINT GetItemPosition(IntPtr pidl);
    POINT GetSpacing();
    void _GetDefaultSpacing();
    [PreserveSig] int GetAutoArrange();
    void _SelectItem(); void _SelectAndPositionItems();
    void _SetGroupBy(); void _GetGroupBy(); void _SetViewProperty(); void _GetViewProperty(); void _SetTileViewProperties();
    void _SetExtendedTileViewProperties(); void _SetText(); void _SetCurrentFolderFlags();
    uint GetCurrentFolderFlags();
    void _GetSortColumnCount(); void _SetSortColumns(); void _GetSortColumns(); void _GetItem(); void _GetVisibleItem();
    void _GetSelectedItem(); void _GetSelection(); void _GetSelectionState(); void _InvokeVerbOnSelection(); void _SetViewModeAndIconSize();
    void GetViewModeAndIconSize(out int mode, out int iconSize);
  }
  [ComImport, Guid("000214E6-0000-0000-C000-000000000046"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
  interface IShellFolder {
    void _1(); void _2(); void _3(); void _4(); void _5(); void _6(); void _7(); void _8();
    [PreserveSig] int GetDisplayNameOf(IntPtr pidl, uint flags, IntPtr strret);
  }

  public class Icon { public string Name, Path; public int X, Y; }
  public class Monitor { public string Dev; public int X, Y, W, H, WX, WY, WW, WH, Dpi; public bool Primary; }
  public class Snapshot {
    public List<Icon> Icons = new List<Icon>();
    public List<Monitor> Monitors = new List<Monitor>();
    public double Space96X, Space96Y;
    public int IconSize;
    public bool Snap, AutoArrange, Hidden;
  }

  public static class Desk {
    [DllImport("shlwapi.dll", CharSet = CharSet.Unicode)] static extern int StrRetToBufW(IntPtr strret, IntPtr pidl, StringBuilder buf, int cch);
    [DllImport("ole32.dll")] static extern void CoTaskMemFree(IntPtr p);
    [DllImport("user32.dll")] static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern IntPtr FindWindowEx(IntPtr parent, IntPtr after, string cls, string title);
    [DllImport("user32.dll")] static extern bool SetProcessDpiAwarenessContext(IntPtr v);
    [DllImport("user32.dll")] static extern uint GetDpiForSystem();
    delegate bool MonitorEnumProc(IntPtr mon, IntPtr hdc, ref RECT r, IntPtr data);
    [DllImport("user32.dll")] static extern bool EnumDisplayMonitors(IntPtr hdc, IntPtr clip, MonitorEnumProc cb, IntPtr data);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool GetMonitorInfo(IntPtr mon, ref MONITORINFOEX mi);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool EnumDisplayDevices(string dev, uint i, ref DISPLAY_DEVICE dd, uint flags);
    [DllImport("shcore.dll")] static extern int GetDpiForMonitor(IntPtr mon, int type, out uint dx, out uint dy);
    [DllImport("shell32.dll", CharSet = CharSet.Unicode)] static extern int SHQueryRecycleBin(string root, IntPtr info);

    // Physical pixels everywhere, so positions match what the wallpaper draws.
    public static void DpiAware() { try { SetProcessDpiAwarenessContext(new IntPtr(-4)); } catch { } }

    static string DisplayName(IShellFolder f, IntPtr pidl, uint flags) {
      IntPtr sr = Marshal.AllocCoTaskMem(1024);
      try {
        if (f.GetDisplayNameOf(pidl, flags, sr) != 0) return "";
        var sb = new StringBuilder(1024);
        StrRetToBufW(sr, pidl, sb, sb.Capacity);
        return sb.ToString();
      } finally { Marshal.FreeCoTaskMem(sr); }
    }

    public static List<Monitor> Monitors() {
      var list = new List<Monitor>();
      EnumDisplayMonitors(IntPtr.Zero, IntPtr.Zero, (IntPtr h, IntPtr hdc, ref RECT r, IntPtr d) => {
        var mi = new MONITORINFOEX(); mi.cbSize = Marshal.SizeOf(typeof(MONITORINFOEX));
        if (!GetMonitorInfo(h, ref mi)) return true;
        uint dx = 96, dy = 96;
        try { GetDpiForMonitor(h, 0, out dx, out dy); } catch { }
        var dd = new DISPLAY_DEVICE(); dd.cb = Marshal.SizeOf(typeof(DISPLAY_DEVICE));
        string dev = EnumDisplayDevices(mi.szDevice, 0, ref dd, 1) ? dd.DeviceID : mi.szDevice;
        list.Add(new Monitor {
          Dev = dev ?? "", Primary = (mi.dwFlags & 1) != 0, Dpi = (int)dx,
          X = mi.rcMonitor.Left, Y = mi.rcMonitor.Top, W = mi.rcMonitor.Right - mi.rcMonitor.Left, H = mi.rcMonitor.Bottom - mi.rcMonitor.Top,
          WX = mi.rcWork.Left, WY = mi.rcWork.Top, WW = mi.rcWork.Right - mi.rcWork.Left, WH = mi.rcWork.Bottom - mi.rcWork.Top
        });
        return true;
      }, IntPtr.Zero);
      list.Sort((a, b) => a.Primary != b.Primary ? (a.Primary ? -1 : 1) : a.X != b.X ? a.X.CompareTo(b.X) : a.Y.CompareTo(b.Y));
      return list;
    }

    // Items in the Recycle Bin, or -1 if Windows won't say.
    public static long TrashCount() {
      int size = IntPtr.Size == 8 ? 24 : 20;
      IntPtr p = Marshal.AllocHGlobal(size);
      try {
        for (int i = 0; i < size; i++) Marshal.WriteByte(p, i, 0);
        Marshal.WriteInt32(p, 0, size);
        if (SHQueryRecycleBin(null, p) != 0) return -1;
        return Marshal.ReadInt64(p, size - 8);
      } finally { Marshal.FreeHGlobal(p); }
    }

    // The desktop's own folder view, as in Raymond Chen's "Manipulating the positions of desktop icons".
    public static Snapshot Read() {
      var snap = new Snapshot();
      snap.Monitors = Monitors();
      var sw = (IShellWindows)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("9BA05972-F6A8-11CF-A442-00A0C90A8F39")));
      object loc = 0, root = null; int hwnd;
      object disp = sw.FindWindowSW(ref loc, ref root, 8 /* SWC_DESKTOP */, out hwnd, 1 /* SWFO_NEEDDISPATCH */);
      Guid sid = new Guid("4C96BE40-915C-11CF-99D3-00AA004AE837"), iid = typeof(IShellBrowser).GUID;
      object o;
      Marshal.ThrowExceptionForHR(((IOleServiceProvider)disp).QueryService(ref sid, ref iid, out o));
      object view = ((IShellBrowser)o).QueryActiveShellView();
      var fv = (IFolderView2)view;
      Guid isf = typeof(IShellFolder).GUID;
      var folder = (IShellFolder)fv.GetFolder(ref isf);

      double sys = Math.Max(96, GetDpiForSystem());
      var sp = fv.GetSpacing();
      snap.Space96X = sp.X * 96.0 / sys; snap.Space96Y = sp.Y * 96.0 / sys;
      int mode, size; fv.GetViewModeAndIconSize(out mode, out size);
      snap.IconSize = size;
      snap.Snap = (fv.GetCurrentFolderFlags() & 0x4) != 0;   // FWF_SNAPTOGRID
      snap.AutoArrange = fv.GetAutoArrange() == 0;

      IntPtr defView = ((IShellView)view).GetWindow();
      IntPtr lv = FindWindowEx(defView, IntPtr.Zero, "SysListView32", null);
      var org = new POINT(); ClientToScreen(lv != IntPtr.Zero ? lv : defView, ref org);
      // "Show desktop icons" off: Windows still lists the icons and their places, it just doesn't draw them.
      snap.Hidden = lv != IntPtr.Zero && !IsWindowVisible(lv);
      int n = fv.ItemCount(2 /* SVGIO_ALLVIEW */);
      for (int i = 0; i < n; i++) {
        IntPtr pidl = fv.Item(i);
        try {
          var p = fv.GetItemPosition(pidl);
          snap.Icons.Add(new Icon { Name = DisplayName(folder, pidl, 0), Path = DisplayName(folder, pidl, 0x8000), X = p.X + org.X, Y = p.Y + org.Y });
        } finally { CoTaskMemFree(pidl); }
      }
      return snap;
    }
  }
}
'@
[ClawdEyes.Desk]::DpiAware()

# ---------- what kind of app is it? ----------
# First match wins, so the more specific lists come first (Roblox Studio is code, Roblox Player is a game; a website
# app for Discord or YouTube Music is chat or music, not a browser).
$Kinds = @(
  @('code',    '\\code\.exe|visual studio|vscode|\bcursor\b|windsurf|antigravity|idea64|intellij|pycharm|webstorm|phpstorm|rider|clion|goland|rubymine|datagrip|android studio|studio64|devenv|sublime|notepad\+\+|\bzed\b|neovim|nvim|\bvim\b|emacs|\\wt\.exe|windows terminal|powershell|pwsh|\\cmd\.exe|git bash|git-bash|github desktop|claude|unity hub|\\unity\.exe|godot|roblox ?studio|robloxstudio|eclipse|netbeans|jupyter|anaconda|spyder|rstudio|\\rgui\.exe|^r [0-9]|scenebuilder|postman|docker|\bwsl\b|ubuntu'),
  @('chat',    'discord|whatsapp|telegram|\bslack\b|\bteams\b|\bzoom\b|skype|\bsignal\b|messenger|viber|wechat'),
  @('music',   'spotify|itunes|apple music|\btidal\b|deezer|foobar2000|\baimp\b|winamp|musicbee|\bvlc\b|youtube music|amazon music|soundcloud|groove|media player|plexamp|qobuz|pandora|audacious|fl studio|ableton|\breaper\b|audacity|cubase'),
  @('video',   '^(netflix|youtube|twitch|disney\+?|prime video|hulu|crunchyroll|plex|max|hbo max|stremio|kodi|apple tv|paramount\+?|peacock|mubi)\b|\\(potplayer\w*|mpc-hc\w*|mpc-be\w*|mpv|kodi|plex|stremio)\.exe'),
  @('browser', '\\(chrome|chrome_proxy|msedge|firefox|brave|opera|launcher\.exe.*opera|vivaldi|arc|zen|waterfox|librewolf|floorp|thorium|chromium|iexplore|duckduckgo)\.exe|\b(google chrome|microsoft edge|firefox|brave|opera( gx)?|vivaldi|tor browser|chromium|browser)\b'),
  @('game',    'steam|epic ?games|com\.epicgames|\briot\b|valorant|league of legends|battle\.net|blizzard|ubisoft|uplay|\bea app\b|\borigin\b|gog galaxy|minecraft|tlauncher|roblox|xbox|playnite|heroic|itch\.io|among us|fortnite|forza|\bgames?\b|steamapps|rockstar|genshin|hoyoplay|\bosu\b|terraria|retroarch|dolphin|ryujinx|pcsx|emulator'),
  @('notes',   '\b(winword|word|excel|powerpnt|powerpoint|onenote|outlook|notion|obsidian|evernote|logseq|joplin|libreoffice|openoffice|soffice|onlyoffice|wps office|acrobat|adobe reader|foxit|sumatrapdf|wordpad|notepad|todoist|calendar|mail|thunderbird)\b'),
  @('art',     '\b(photoshop|illustrator|lightroom|indesign|figma|blender|krita|gimp|inkscape|aseprite|clip ?studio|clipstudiopaint|affinity|paint\.net|mspaint|paint|procreate|canva|davinci resolve|premiere|after effects|capcut|vegas|kdenlive|shotcut|obs|obs64|zbrush|maya|3ds max|cinema 4d|substance|sketchup|fusion 360|autocad|pixlr|photopea)\b')
)
$shell = New-Object -ComObject WScript.Shell
$linkCache = @{}
function Get-Target([string]$path) {
  if ($linkCache.ContainsKey($path)) { return $linkCache[$path] }
  $t = ''
  try {
    if ($path -match '\.lnk$') { $l = $shell.CreateShortcut($path); $t = "$($l.TargetPath) $($l.Arguments)" }
    elseif ($path -match '\.url$') { $t = ((Get-Content -LiteralPath $path -TotalCount 20) -match '^URL=') -join ' ' }
    else { $t = $path }
  } catch { $t = '' }
  $linkCache[$path] = $t
  return $t
}
function Get-Kind([string]$name, [string]$path) {
  if ($path -match '^::\{645FF040-5081-101B-9F08-00AA002F954E\}') { return 'trash' }
  if ($path -match '^::\{') { return 'system' }
  if ($path -notmatch '\.(lnk|url|exe|appref-ms)$') {
    if (Test-Path -LiteralPath $path -PathType Container) { return 'folder' }
    return 'file'
  }
  $hay = ($name + ' | ' + (Get-Target $path)).ToLowerInvariant()
  foreach ($k in $Kinds) { if ($hay -match $k[1]) { return $k[0] } }
  return 'app'
}
# A short stable id per icon, so the wallpaper can tell icons apart without seeing their paths.
function Get-Id([string]$s) {
  [uint32]$h = 2166136261
  foreach ($ch in $s.ToLowerInvariant().ToCharArray()) { $h = [uint32](([uint64]($h -bxor [uint32]$ch) * 16777619) % 4294967296) }
  return $h.ToString('x8')
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
# Our wallpaper folders, and which screen Lively is showing each one on.
function Find-Targets($monitors) {
  $found = @{}
  foreach ($r in Get-LivelyRoots) {
    Get-ChildItem -LiteralPath (Join-Path $r 'wallpapers') -Directory -ErrorAction SilentlyContinue |
      Where-Object { Test-OurFolder $_.FullName } | ForEach-Object { $found[$_.FullName.ToLowerInvariant()] = @{ Dir = $_.FullName; Screens = @() } }
  }
  $players = @(Get-CimInstance Win32_Process -Filter "Name LIKE 'Lively.Player%'" -ErrorAction SilentlyContinue)
  foreach ($p in $players) {
    $cl = [string]$p.CommandLine
    if ($cl -notmatch '--wallpaper-url\s+"([^"]+)"') { continue }
    $dir = Split-Path $Matches[1]
    if (-not (Test-OurFolder $dir)) { continue }
    $key = $dir.ToLowerInvariant()
    if (-not $found.ContainsKey($key)) { $found[$key] = @{ Dir = $dir; Screens = @() } }
    if ($cl -match '--wallpaper-display\s+"([^"]+)"') {
      $dev = $Matches[1]
      for ($i = 0; $i -lt $monitors.Count; $i++) { if ($monitors[$i].Dev -eq $dev) { $found[$key].Screens += $i } }
    }
  }
  foreach ($a in $Also) { if ($a) { $found[$a.ToLowerInvariant()] = @{ Dir = (Resolve-Path -LiteralPath $a).Path; Screens = @() } } }
  return $found.Values
}

# ---------- one reading ----------
$state = @{ Targets = @(); TargetsAt = [datetime]::MinValue; Trash = -1; TrashAt = [datetime]::MinValue; Last = @{}; WrittenAt = @{}; Count = 0; Error = '' }
function Read-Desk {
  $s = [ClawdEyes.Desk]::Read()
  $mons = @()
  foreach ($m in $s.Monitors) {
    $k = $m.Dpi / 96.0
    $sx = [math]::Round($s.Space96X * $k); $sy = [math]::Round($s.Space96Y * $k)
    $mons += [ordered]@{ x = $m.X; y = $m.Y; w = $m.W; h = $m.H; wx = $m.WX; wy = $m.WY; ww = $m.WW; wh = $m.WH; dpi = $m.Dpi; p = $m.Primary;
                         sx = $sx; sy = $sy; ip = [math]::Round($s.IconSize * $k);
                         cols = [math]::Max(1, [math]::Floor($m.WW / [math]::Max(1, $sx))); rows = [math]::Max(1, [math]::Floor($m.WH / [math]::Max(1, $sy))) }
  }
  $icons = @()
  foreach ($ic in $s.Icons) {
    $mi = 0
    for ($i = 0; $i -lt $mons.Count; $i++) {
      $m = $mons[$i]
      if ($ic.X + 8 -ge $m.x -and $ic.X + 8 -lt $m.x + $m.w -and $ic.Y + 8 -ge $m.y -and $ic.Y + 8 -lt $m.y + $m.h) { $mi = $i; break }
    }
    $m = $mons[$mi]
    $cx = $ic.X + $m.ip / 2; $cy = $ic.Y + $m.ip / 2
    $kind = Get-Kind $ic.Name $ic.Path
    $named = $kind -notin @('file', 'folder')
    $icons += [ordered]@{ id = (Get-Id $ic.Path); k = $kind; n = $(if ($named) { $ic.Name } else { '' });
                          m = $mi; c = [math]::Floor(($cx - $m.wx) / $m.sx); r = [math]::Floor(($cy - $m.wy) / $m.sy); x = $cx; y = $cy }
  }
  if (((Get-Date) - $state.TrashAt).TotalSeconds -ge 5) { $state.Trash = [ClawdEyes.Desk]::TrashCount(); $state.TrashAt = Get-Date }
  if (((Get-Date) - $state.TargetsAt).TotalSeconds -ge 10) { $state.Targets = @(Find-Targets $s.Monitors); $state.TargetsAt = Get-Date }
  $state.Count = $icons.Count
  return [ordered]@{ v = 1; on = $true; snap = $s.Snap; arrange = $s.AutoArrange; hidden = $s.Hidden; monitors = $mons; icons = $icons; trash = $state.Trash }
}
function Write-Data($data) {
  foreach ($t in $state.Targets) {
    $d = $data
    $d.screen = $(if (@($t.Screens).Count -eq 1) { @($t.Screens)[0] } else { -1 })
    $body = ($d | ConvertTo-Json -Depth 5 -Compress) -replace [char]0x2028, ' ' -replace [char]0x2029, ' '
    $key = $t.Dir.ToLowerInvariant()
    $stale = -not $state.WrittenAt.ContainsKey($key) -or ((Get-Date) - $state.WrittenAt[$key]).TotalSeconds -ge 5
    if (-not $stale -and $state.Last[$key] -eq $body) { continue }
    $state.Last[$key] = $body
    $state.WrittenAt[$key] = Get-Date
    $now = [DateTimeOffset]::Now.ToUnixTimeMilliseconds()
    $js = "window.clawdEyes && window.clawdEyes($($body.Substring(0, $body.Length - 1)),`"t`":$now});`n"
    $file = Join-Path $t.Dir $DataFile
    # Write a temp file and swap it in, so the wallpaper never reads half a file.
    # ([NullString]::Value, because PowerShell would pass $null to .NET as an empty path.)
    $utf8 = New-Object Text.UTF8Encoding $false
    $tmp = "$file.tmp"
    try {
      [IO.File]::WriteAllText($tmp, $js, $utf8)
      if (Test-Path -LiteralPath $file) { [IO.File]::Replace($tmp, $file, [NullString]::Value) } else { [IO.File]::Move($tmp, $file) }
    } catch {
      try { [IO.File]::WriteAllText($file, $js, $utf8) } catch { }
      Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue
    }
  }
}
$LogFile = Join-Path $env:TEMP 'clawd-eyes.log'
function Tick {
  try { Write-Data (Read-Desk); $state.Error = '' }
  catch {
    $msg = "$($_.Exception.Message) (line $($_.InvocationInfo.ScriptLineNumber))"
    # Log each new problem once, so it's there to look at without filling the disk.
    if ($msg -ne $state.Error) { try { Add-Content -LiteralPath $LogFile -Value "$(Get-Date -Format s)  $msg" } catch { } }
    $state.Error = $msg
  }
}

if ($Once) {
  $d = Read-Desk
  $d | ConvertTo-Json -Depth 5
  "wallpaper folders: " + (($state.Targets | ForEach-Object { "$($_.Dir) (screens: $(@($_.Screens) -join ','))" }) -join '; ')
  return
}

# ---------- one copy only ----------
$created = $false
$mutex = New-Object System.Threading.Mutex($true, 'Local\ClawdEyesForDeskStations', [ref]$created)
if (-not $created) { return }

# ---------- tray icon ----------
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
function New-ClawdIcon {
  $rows = @(
    '................',
    '................',
    '..oooooooooooo..',
    '..oooooooooooo..',
    '..ookoooooooko..',
    '..ookoooooooko..',
    'oooooooooooooooo',
    'oooooooooooooooo',
    '..oooooooooooo..',
    '..oooooooooooo..',
    '..oooooooooooo..',
    '...o.o....o.o...',
    '...o.o....o.o...',
    '...o.o....o.o...',
    '................',
    '................')
  $bmp = New-Object System.Drawing.Bitmap 16, 16
  $col = @{ o = [System.Drawing.Color]::FromArgb(217, 119, 87); k = [System.Drawing.Color]::FromArgb(20, 20, 19) }
  for ($y = 0; $y -lt 16; $y++) { for ($x = 0; $x -lt 16; $x++) { $ch = [string]$rows[$y][$x]; if ($col.ContainsKey($ch)) { $bmp.SetPixel($x, $y, $col[$ch]) } } }
  return [System.Drawing.Icon]::FromHandle($bmp.GetHicon())
}
$startup = Join-Path ([Environment]::GetFolderPath('Startup')) "Clawd's Eyes.lnk"
function Set-StartWithWindows([bool]$on) {
  if ($on) {
    $l = $shell.CreateShortcut($startup)
    # conhost --headless keeps it windowless even when Windows Terminal is the default console.
    $l.TargetPath = "$env:WINDIR\System32\conhost.exe"
    $l.Arguments = "--headless `"$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe`" -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -STA -File `"$PSCommandPath`""
    $l.WorkingDirectory = $PSScriptRoot
    $l.Description = "Clawd's Eyes, for the Clawd Desk Stations wallpaper"
    $l.Save()
  } elseif (Test-Path -LiteralPath $startup) { Remove-Item -LiteralPath $startup -Force }
}
# Rewrite an existing shortcut so older ones, which could open a window, get the windowless launch.
if (Test-Path -LiteralPath $startup) { try { Set-StartWithWindows $true } catch { } }

$tray = New-Object System.Windows.Forms.NotifyIcon
$tray.Icon = New-ClawdIcon
$tray.Text = "Clawd's Eyes"
$menu = New-Object System.Windows.Forms.ContextMenuStrip
$status = $menu.Items.Add("Clawd's Eyes: starting...")
$status.Enabled = $false
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

$timer = New-Object System.Windows.Forms.Timer
$timer.Interval = 1000
$timer.add_Tick({
  Tick
  $n = @($state.Targets).Count
  $status.Text = $(if ($state.Error) { "Can't see the desktop right now" } else { "Watching $($state.Count) desktop icons" })
  $where.Text = $(if ($n) { "Sending them to Clawd Desk Stations" } else { 'Add Clawd Desk Stations to Lively to start' })
  $tray.Text = "Clawd's Eyes: " + $(if ($n) { 'watching' } else { 'waiting for the wallpaper' })
})
$quit.add_Click({
  $timer.Stop()
  # Tell the wallpaper we've gone, so Clawd switches back to guessing.
  foreach ($t in $state.Targets) {
    try { [IO.File]::WriteAllText((Join-Path $t.Dir $DataFile), "window.clawdEyes && window.clawdEyes({`"v`":1,`"on`":false,`"t`":$([DateTimeOffset]::Now.ToUnixTimeMilliseconds())});`n") } catch { }
  }
  $tray.Visible = $false
  [System.Windows.Forms.Application]::Exit()
})
Tick
$timer.Start()
$tray.ShowBalloonTip(4000, "Clawd's Eyes is on", 'Clawd can now see which apps you put in his stations. Right-click him here to quit.', 'None')
[System.Windows.Forms.Application]::Run()
$mutex.ReleaseMutex()
