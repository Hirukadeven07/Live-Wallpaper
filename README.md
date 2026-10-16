<div align="center">

<img src="assets/banner.jpg" alt="Claude Dunes and Clawd Dunes live wallpapers" width="100%">

# Live Wallpaper

**Animated desktop wallpapers for Windows.**

Each one runs inside [Lively Wallpaper](https://github.com/rocksdanister/lively), a free and open-source app.<br>
Every wallpaper also comes with still images, so you can use it without installing anything.

<p>
  <img alt="Platform: Windows 10 and 11" src="https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-D97757?style=flat-square">
  <img alt="Made for Lively Wallpaper" src="https://img.shields.io/badge/made%20for-Lively%20Wallpaper-141413?style=flat-square">
  <img alt="WebGL 2" src="https://img.shields.io/badge/WebGL-2-D97757?style=flat-square">
  <a href="LICENSE"><img alt="License: PolyForm Noncommercial 1.0.0" src="https://img.shields.io/badge/license-PolyForm%20Noncommercial%201.0.0-141413?style=flat-square"></a>
  <img alt="Unofficial fan project" src="https://img.shields.io/badge/fan%20project-unofficial-D97757?style=flat-square">
</p>

<p>
  <a href="#wallpapers">Wallpapers</a> ·
  <a href="#what-you-need">What you need</a> ·
  <a href="#how-to-use-a-live-wallpaper">How to use</a> ·
  <a href="#no-lively-use-the-still-images">Still images</a> ·
  <a href="#troubleshooting">Troubleshooting</a> ·
  <a href="#license-and-notice">License</a>
</p>

</div>

> [!NOTE]
> Unofficial fan art. Not affiliated with or endorsed by Anthropic. See [License and notice](#license-and-notice).

---

## Wallpapers

| Preview | Wallpaper | What it is | Download |
| --- | --- | --- | --- |
| <a href="wallpapers/claude-dunes"><img src="wallpapers/claude-dunes/source/thumbnail.png" width="260" alt="Claude Dunes"></a> | **[Claude Dunes](wallpapers/claude-dunes)** | A clay-orange spark rising over slowly drifting dunes. Day, sunset and night looks. | [**Claude-Dunes.zip**](https://github.com/Hirukadeven07/Live-Wallpaper/raw/main/wallpapers/claude-dunes/Claude-Dunes.zip) |
| <a href="wallpapers/clawd-dunes"><img src="wallpapers/clawd-dunes/source/thumbnail.png" width="260" alt="Clawd Dunes"></a> | **[Clawd Dunes](wallpapers/clawd-dunes)** | Clawd, the Claude Code mascot, wandering the same dunes. He walks, waves, dances, meditates and sits down at a laptop. | [**Clawd-Dunes.zip**](https://github.com/Hirukadeven07/Live-Wallpaper/raw/main/wallpapers/clawd-dunes/Clawd-Dunes.zip) |
| <a href="wallpapers/clawd-desk-buddy"><img src="wallpapers/clawd-desk-buddy/source/thumbnail.png" width="260" alt="Clawd Desk Buddy"></a> | **[Clawd Desk Buddy](wallpapers/clawd-desk-buddy)** | A tiny Clawd who lives on your desktop. He follows your cursor, reacts to clicks, taps the clock and battery on the taskbar, and naps when you're away. Five backgrounds, or use your own picture. | [**Clawd-Desk-Buddy.zip**](https://github.com/Hirukadeven07/Live-Wallpaper/raw/main/wallpapers/clawd-desk-buddy/Clawd-Desk-Buddy.zip) |

Click a wallpaper's name to see its settings and still images.

## What you need

- Windows 10 or 11
- [Lively Wallpaper](#step-1-install-lively-wallpaper) (free)
- A graphics card or integrated GPU that supports WebGL 2. Any PC from the last several years does. If yours doesn't, the wallpaper shows a simpler version of the scene instead of failing.

## How to use a live wallpaper

### Step 1: Install Lively Wallpaper

Pick one way. Skip this step if you already have Lively.

- **Microsoft Store:** [Lively Wallpaper on the Microsoft Store](https://apps.microsoft.com/detail/9ntm2qc6qws7) (publisher: Sketches SRL, the developer's company). Click **Get**.
- **Direct download:** [Lively releases on GitHub](https://github.com/rocksdanister/lively/releases). Under the newest release, download the file named `lively_setup_x86_full_...exe` and run it.
- **Command line:** open PowerShell and run:

  ```powershell
  winget install --id rocksdanister.LivelyWallpaper --source winget
  ```

If the Microsoft Store install fails with an error code, use the direct download or the command line instead.

### Step 2: Download the wallpaper

Click the download link in the [table above](#wallpapers), or use the one from the wallpaper's own folder. You get a `.zip` file.

> [!IMPORTANT]
> **Don't unzip it.** Lively reads the zip as it is.

### Step 3: Add it to Lively

1. Open Lively. If you can't see it, click its icon in the system tray (bottom-right of the taskbar, behind the `^` arrow).
2. **Drag the `.zip` file onto the Lively window.**
   Or click the **+** button at the top of Lively, choose to add a wallpaper from a file, and select the `.zip`.
3. Click **Import** or **Add** if Lively asks you to confirm.

### Step 4: Set it as your wallpaper

Click the wallpaper's tile in Lively's library. It becomes your desktop background straight away.

> [!TIP]
> To see it, minimize your windows or press **Win + D**.

### Step 5: Change the settings (optional)

Right-click the wallpaper's tile in Lively and choose **Customise**. The options for each wallpaper are listed on its own page:

- [Claude Dunes settings](wallpapers/claude-dunes#settings)
- [Clawd Dunes settings](wallpapers/clawd-dunes#settings)
- [Clawd Desk Buddy settings](wallpapers/clawd-desk-buddy#settings)

### Go back to your old wallpaper

In Lively, right-click the wallpaper that is playing and choose **Close wallpaper**, or pick another tile. To remove a wallpaper completely, right-click its tile and choose **Delete**.

## No Lively? Use the still images

Each wallpaper folder has a `still-images` folder with three pictures (2560 x 1600). To use one:

1. Open the wallpaper's folder on GitHub and open the `still-images` folder.
2. Click an image, then click the download button (the arrow icon) on the right.
3. Right-click the downloaded file and choose **Set as desktop background**.

If your screen has a different shape, open **Settings > Personalization > Background** and set **Choose a fit** to **Fill**.

## Troubleshooting

<details>
<summary><b>The wallpaper doesn't move.</b></summary>

<br>

Lively pauses wallpapers by design when another window is maximized or covering the desktop, and on battery power if you've enabled that. Minimize your windows or press **Win + D** and it will start moving. You can change these rules in Lively's settings.

</details>

<details>
<summary><b>Clawd Desk Buddy doesn't react to my cursor or clicks.</b></summary>

<br>

Lively only passes the mouse to a wallpaper when you allow it. Open Lively's **Settings**, go to **Wallpaper**, and set **Wallpaper input** to **Mouse**. The wallpaper only sees the mouse while it's over the desktop, not over other windows.

</details>

<details>
<summary><b>It uses too much battery or GPU.</b></summary>

<br>

Right-click the tile, choose **Customise**, and set **Frame rate** to **15 fps (battery saver)**.

</details>

<details>
<summary><b>The wallpaper looks plain or flat.</b></summary>

<br>

Your graphics driver may not support WebGL 2, so you're seeing the fallback version. Updating your graphics driver usually fixes it.

</details>

<details>
<summary><b>The tile doesn't appear after adding the zip.</b></summary>

<br>

Close Lively fully from its tray icon (right-click > **Exit**), then open it again.

</details>

## License and notice

These are unofficial fan wallpapers. You can use and share them for noncommercial purposes under the [PolyForm Noncommercial License 1.0.0](LICENSE).

The Claude spark and the Clawd character belong to Anthropic, which is not affiliated with or endorsing this project, and they are not covered by that license. Keep them to personal use. Details are in [NOTICE.md](NOTICE.md).
