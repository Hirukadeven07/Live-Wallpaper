# Clawd Desk Stations

A shelf of slots sits on your desktop's icon grid, as many as you like. Drag any app's icon into a slot and Clawd works out what kind of app it is, climbs over and plays with it. He surfs your browser, dances to your music, plays your games, codes in your editor, chats on Discord, paints in Photoshop, watches Netflix and throws paper balls into the Recycle Bin. He walks along the planks and climbs the ladders, so he never stands in front of one of your apps.

![Clawd Desk Stations](source/thumbnail.png)

## Download

You need both files:

- **The wallpaper (Lively):** [Clawd-Desk-Stations.zip](https://github.com/Hirukadeven07/Live-Wallpaper/raw/main/wallpapers/clawd-desk-stations/Clawd-Desk-Stations.zip). Don't unzip it; drag it onto the Lively window.
- **Clawd's Eyes (the helper):** [Clawd-Eyes.zip](https://github.com/Hirukadeven07/Live-Wallpaper/raw/main/wallpapers/clawd-desk-stations/Clawd-Eyes.zip). This one you do unzip. It's what lets Clawd see which app is in which slot.

New to Lively? Follow the [step-by-step instructions](../../README.md#how-to-use-a-live-wallpaper) on the main page first.

## Set it up

1. **Add the wallpaper to Lively** and click its tile.
2. **Let Clawd see your mouse.** In Lively, open **Settings**, go to **Wallpaper**, and set **Wallpaper input** to **Mouse**.
3. **Start Clawd's Eyes.** Unzip `Clawd-Eyes.zip` somewhere you'll keep it, such as your Documents folder. Then double-click **Start Clawd Eyes.cmd**. A small Clawd appears in the system tray (bottom right, maybe behind the `^` arrow). To have it start by itself every time, right-click that Clawd and tick **Start with Windows**.
4. **Show your icons, and let them stay where you put them.** Right-click an empty spot on the desktop and open **View**. Make sure **Show desktop icons** is ticked, turn **Auto arrange icons** off, and turn **Align icons to grid** on. If your icons are hidden, a note under the shelf reminds you.
5. **Pick how many slots you want.** Right-click the wallpaper in Lively, choose **Customise**, and set **How many slots** (1 to 100) and **Slots in each row**.
6. **Fill the slots.** Drag an app's icon from your desktop into a slot. Only the icons on your desktop count; to add an app, drag it there from the Start menu first.

> [!NOTE]
> The shelf finds a free spot on your icon grid by itself, near the bottom right if there's room. If it lands somewhere you don't like, pick a different **Where the shelf goes** in the settings, or press **Find a new spot**. If you ask for more slots than fit, the rows get wider first, then Clawd tells you how many fit.

## The shelf

Each slot is one cell of your icon grid. The slots sit in rows like cubbies. On top of each row is a plank that Clawd walks along, and a ladder at each end takes him between the rows. Windows always draws your icons on top of the wallpaper, so Clawd stays on the planks, the ladders and the open desktop around the shelf, and never stands in front of a slot. Under a low row he swaps tall moves for shorter ones, and his speech bubbles move beside him when there's no room above.

The tag at the bottom of each slot shows what Clawd thinks is in it.

| Drop in | Tag | Clawd does this |
| --- | --- | --- |
| Chrome, Edge, Firefox, Brave, Opera, Vivaldi, or a website app | web | Rides a surfboard: "surfing the web!" |
| Spotify, Apple Music, VLC, foobar2000, MusicBee, FL Studio, Ableton and the like | music | Dances and break-dances while notes fly off the app |
| Steam, Epic Games, Riot, Battle.net, Minecraft, Roblox, any game shortcut | ARCADE | Sits down to play. "GG!" or "rage quit!" |
| VS Code, Cursor, JetBrains IDEs, Visual Studio, Notepad++, Windows Terminal, Unity Hub, Claude | code | Opens his laptop and codes: "it compiles!" (Claude gets "hey, that's me!") |
| Discord, WhatsApp, Telegram, Slack, Teams, Zoom | chat | Waves at his friends while little "..." bubbles pop out |
| Word, Excel, PowerPoint, OneNote, Notion, Obsidian, Acrobat, Notepad, Mail | notes | Thinks it over, then types it up while pages fly out |
| Photoshop, Illustrator, Figma, Blender, Krita, GIMP, Paint, DaVinci Resolve, Premiere, OBS | art | Paints: splashes of colour fly over the app. "masterpiece!" |
| Netflix, YouTube, Twitch, Disney+, Prime Video, Plex, PotPlayer | video | Sits down to watch while popcorn pops, and the slot flickers like a screen |
| The Recycle Bin | trash | Throws three paper balls in. If the bin is getting full, he asks you to empty it. |
| A folder | folder | Peeks in: "what's in here?" |
| A file | file | Reads it |
| This PC, Control Panel and other system icons | system | Spins around running diagnostics: "beep boop" |
| Any other app | app | Pokes it to see what it does |

He also notices:

| When | Clawd does this |
| --- | --- |
| You drop an app in a slot | Runs straight over, even from a nap: "ooh, Spotify!" |
| You take an app out of a slot | "hey, where did Steam go?" |
| You move an app from one slot to another | Goes to play with it in its new slot |
| A new app shows up on your desktop | Runs over to sniff it and asks you to put it on the shelf |
| You empty the Recycle Bin | "so clean!" |
| Every so often | Goes back to play with one of your apps, strolls along a plank, or points out an empty slot |
| You click a slot | He comes over to play with it, or explains that it's empty |

Everything Clawd Desk Buddy does works here too: petting, cookies, the ball, high fives, picking him up, naps, and the clock and battery visits. The ball rolls along the planks, and if you drop Clawd over the shelf he lands on the plank instead of in front of a slot. See [Clawd Desk Buddy](../clawd-desk-buddy#things-to-try).

## Without Clawd's Eyes

The wallpaper still works without the helper, but Clawd has to guess. He can see the mouse but not your icons. So a drag that ends inside a slot counts as "something's in there now" (its tag says `app?`), and a drag that starts in one and ends outside it empties it. He can't tell which app it is, so he just pokes it. He also can't spot new apps or the Recycle Bin being emptied.

Without the helper, the shelf also assumes Windows' usual icon grid (medium icons). If yours is different, it may not line up with your icons. Clawd's Eyes fixes this, because it reads the real grid from Windows.

Hover over the shelf to see whether Clawd's Eyes is on.

## Settings

Right-click the wallpaper in Lively and choose **Customise**.

| Setting | Options | Default |
| --- | --- | --- |
| How many slots | 1 to 100 | 12 |
| Slots in each row | 1 to 20 | 6 |
| Where the shelf goes | Wherever there's space, Bottom right, Bottom left, Top right, Top left | Wherever there's space |
| Move the shelf | **Find a new spot** button | |
| Show "drop any app here" hints | On or off | On |
| Which screen this wallpaper is on | Automatic, Main screen, Screen 2, Screen 3 | Automatic |
| Background | Paper, Graph paper, Midnight terminal, Dunes, Clay glow, Your own picture | Graph paper |
| Your own picture | Any picture you add with the **+** button | Claude Dunes at sunset |
| Appearance | Match Windows, Light, Dark, Follow the clock | Match Windows |
| Clawd size | Tiny, Small, Medium | Tiny |
| Your cursor | Ignore it, Look at it, Follow it when it stops, Chase it everywhere | Follow it when it stops |
| Your clicks | Ignore them, React to them, React and run to them | React and run to them |
| Taskbar visits (clock and battery) | Never, Sometimes, Often | Sometimes |
| Taskbar height | Auto, Hidden, 40, 48, 56 or 64 px | Auto |
| Naps when the mouse is idle | Never, after 1, 3, 5 or 10 minutes | After 3 minutes |
| Reacts to the hour and to your battery | On or off | On |
| Toys: snacks, ball, paper planes and coffee | On or off | On |
| Speech bubbles | On or off | On |
| Frame rate | 15, 30 or 60 fps | 30 fps |

**How many slots:** changing the number grows or shrinks the shelf where it is, so apps already in the slots stay in them. If the bigger shelf doesn't fit there, it finds a new spot.

**Where the shelf goes:** the shelf stays where it is until you change this or press **Find a new spot**. Then it picks a new place, and anything that was in the old slots stays on your desktop where it is.

**Clawd size:** the shelf leaves room for him, so a bigger Clawd gets wider ladders.

**Which screen:** with more than one screen, Clawd's Eyes asks Lively which screen the wallpaper is on. If you show the wallpaper on several screens at once, pick the screen for each one by hand.

## About Clawd's Eyes

Clawd's Eyes is a short PowerShell script (`clawd-eyes.ps1`) that you can open and read. About once a second it:

1. Asks Windows for the names and grid positions of your desktop icons, and how many items are in the Recycle Bin.
2. Works out what kind of app each shortcut opens (browser, music, game, code, chat, notes, art, video and so on).
3. Writes the result to `clawd-eyes-data.js` inside the wallpaper's folder in Lively's library, where the wallpaper reads it.

It only reads. It never moves, opens or changes your icons or files. File and folder names are never written out, only app names. Nothing leaves your PC.

- **Stop it:** right-click the Clawd in the system tray and choose **Quit**. The wallpaper switches back to guessing.
- **Start it with Windows:** right-click the tray Clawd and tick **Start with Windows**. This adds a shortcut to your Startup folder; untick it to remove the shortcut.
- **If something's wrong:** problems are written to `clawd-eyes.log` in your temp folder (type `%TEMP%` in the File Explorer address bar).

> [!TIP]
> If Windows won't run **Start Clawd Eyes.cmd** after you download it, right-click `Clawd-Eyes.zip`, choose **Properties**, tick **Unblock**, click **OK**, and unzip it again.

## Notes

Windows always draws desktop icons on top of the wallpaper, which is why Clawd keeps to the planks and ladders. His notes, sparks and paper balls still fly up around the apps.

There are no still images for this one. The shelf only makes sense live, with your own icons in it.

Clawd's sprites and moves come from the Claude desktop app, and the character belongs to Anthropic. The surfboard, paper balls, sleeping and happy poses, chat bubbles, pages and the shelf are drawn for these wallpapers. Clawd remembers where the shelf is and what was in it in the wallpaper's own storage on your PC. The sample picture is the sunset still from [Claude Dunes](../claude-dunes). This is an unofficial fan wallpaper for personal use. See the [notices](../../README.md#license-and-notice).
