<#
  Draws app.ico: a clay-orange tile with a cream sun over two dark dunes.
  Each size is drawn on its own (not scaled down) so the small ones stay crisp.
  The whole square scene is painted first, then the rounded tile shape is cut out of it as a soft alpha mask,
  so the edge pixels take the colour of whatever is under them (no orange rim around the dark dunes).
  Run it from this folder with Windows PowerShell:  powershell -ExecutionPolicy Bypass -File make-icon.ps1
#>
Add-Type -AssemblyName System.Drawing
$sizes = 16, 20, 24, 32, 40, 48, 64, 256
$clay = [System.Drawing.Color]::FromArgb(217, 119, 87)
$cream = [System.Drawing.Color]::FromArgb(240, 238, 230)
$ink = [System.Drawing.Color]::FromArgb(20, 20, 19)
$dusk = [System.Drawing.Color]::FromArgb(92, 52, 40)
$fmt = [System.Drawing.Imaging.PixelFormat]::Format32bppArgb

function Pixels($bmp) {
  $r = New-Object System.Drawing.Rectangle 0, 0, $bmp.Width, $bmp.Height
  $d = $bmp.LockBits($r, 'ReadOnly', $fmt)
  $a = New-Object byte[] ($d.Stride * $bmp.Height)
  [Runtime.InteropServices.Marshal]::Copy($d.Scan0, $a, 0, $a.Length)
  $bmp.UnlockBits($d)
  return ,$a
}

function Draw([int]$s) {
  $k = $s / 32.0
  # 1. The scene, filling the whole square.
  $scene = New-Object System.Drawing.Bitmap $s, $s, $fmt
  $g = [System.Drawing.Graphics]::FromImage($scene)
  $g.SmoothingMode = 'AntiAlias'
  $g.PixelOffsetMode = 'HighQuality'
  $g.Clear($clay)
  $sun = 9.5 * $k
  $g.FillEllipse((New-Object System.Drawing.SolidBrush $cream), [float](17.5 * $k), [float](5.5 * $k), [float]$sun, [float]$sun)
  $d1 = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d1.AddBezier([float](-2 * $k), [float](21 * $k), [float](8 * $k), [float](13 * $k), [float](17 * $k), [float](15 * $k), [float](34 * $k), [float](20 * $k))
  $d1.AddLine([float](34 * $k), [float](20 * $k), [float](34 * $k), [float](34 * $k))
  $d1.AddLine([float](34 * $k), [float](34 * $k), [float](-2 * $k), [float](34 * $k))
  $d1.CloseFigure()
  $g.FillPath((New-Object System.Drawing.SolidBrush $dusk), $d1)
  $d2 = New-Object System.Drawing.Drawing2D.GraphicsPath
  $d2.AddBezier([float](-2 * $k), [float](27 * $k), [float](10 * $k), [float](20 * $k), [float](22 * $k), [float](20 * $k), [float](34 * $k), [float](26 * $k))
  $d2.AddLine([float](34 * $k), [float](26 * $k), [float](34 * $k), [float](34 * $k))
  $d2.AddLine([float](34 * $k), [float](34 * $k), [float](-2 * $k), [float](34 * $k))
  $d2.CloseFigure()
  $g.FillPath((New-Object System.Drawing.SolidBrush $ink), $d2)
  $g.Dispose()

  # 2. The rounded tile, as a soft-edged mask.
  $mask = New-Object System.Drawing.Bitmap $s, $s, $fmt
  $g = [System.Drawing.Graphics]::FromImage($mask)
  $g.SmoothingMode = 'AntiAlias'
  $g.PixelOffsetMode = 'HighQuality'
  $g.Clear([System.Drawing.Color]::Transparent)
  $r = [math]::Max(2, 7 * $k)
  $w = [float]$s
  $path = New-Object System.Drawing.Drawing2D.GraphicsPath
  $path.AddArc(0, 0, 2 * $r, 2 * $r, 180, 90)
  $path.AddArc($w - 2 * $r, 0, 2 * $r, 2 * $r, 270, 90)
  $path.AddArc($w - 2 * $r, $w - 2 * $r, 2 * $r, 2 * $r, 0, 90)
  $path.AddArc(0, $w - 2 * $r, 2 * $r, 2 * $r, 90, 90)
  $path.CloseFigure()
  $g.FillPath([System.Drawing.Brushes]::White, $path)
  $g.Dispose()

  # 3. Scene colours with the mask's alpha.
  $sp = Pixels $scene
  $mp = Pixels $mask
  for ($i = 0; $i -lt $sp.Length; $i += 4) { $sp[$i + 3] = $mp[$i + 3] }
  $out = New-Object System.Drawing.Bitmap $s, $s, $fmt
  $rect = New-Object System.Drawing.Rectangle 0, 0, $s, $s
  $d = $out.LockBits($rect, 'WriteOnly', $fmt)
  [Runtime.InteropServices.Marshal]::Copy($sp, 0, $d.Scan0, $sp.Length)
  $out.UnlockBits($d)
  $scene.Dispose(); $mask.Dispose()

  $ms = New-Object System.IO.MemoryStream
  $out.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
  $out.Dispose()
  return ,$ms.ToArray()
}

$pngs = @($sizes | ForEach-Object { ,(Draw $_) })
$out = New-Object System.IO.MemoryStream
$bw = New-Object System.IO.BinaryWriter $out
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
  $s = $sizes[$i]
  $bw.Write([byte]($s % 256)); $bw.Write([byte]($s % 256)); $bw.Write([byte]0); $bw.Write([byte]0)
  $bw.Write([uint16]1); $bw.Write([uint16]32)
  $bw.Write([uint32]$pngs[$i].Length); $bw.Write([uint32]$offset)
  $offset += $pngs[$i].Length
}
foreach ($p in $pngs) { $bw.Write($p) }
$bw.Flush()
[IO.File]::WriteAllBytes((Join-Path $PSScriptRoot 'app.ico'), $out.ToArray())
# A 256 px PNG of the same icon, for the app's own page.
[IO.File]::WriteAllBytes((Join-Path $PSScriptRoot '..\ui\icon.png'), $pngs[-1])
"wrote app.ico ($($out.Length) bytes)"
