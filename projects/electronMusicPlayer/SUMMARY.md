# Project Summary

## What Is This Project

This project is a simple audio player. The player uses playerctl to control music. The player shows the album cover in a circle. The player shows a sound visualizer around the circle. The visualizer uses dots, like the btop tool.

## What We Did

We built the app with Electron.

We made these files:

- `src/main.js` — This file starts the app window. This file connects the other parts of the app.
- `src/preload.js` — This file lets the window talk to the main app in a safe way.
- `src/playerctl.js` — This file runs playerctl. This file sends song data (title, artist, cover, status) to the window. This file sends play, pause, next, and previous commands to playerctl.
- `src/cava.js` — This file runs cava. Cava reads the audio and sends sound levels to the window. If cava is not present, this file makes a simple test animation instead.
- `src/config/cava.conf` — This file sets up cava. Cava sends 32 numbers for each sound frame.

We also made the window parts:

- `renderer/index.html` — This file shows the circle, the visualizer, the buttons, and the song text.
- `renderer/style.css` — This file makes the circle shape. This file makes the button bar cover the bottom of the circle.
- `renderer/app.js` — This file connects all window parts. This file starts the draw loop.
- `renderer/visualizer.js` — This file draws the dots around the circle. This file smooths the dot sizes between frames.
- `renderer/albumArt.js` — This file shows the album cover. This file shows a default image if there is no cover.
- `renderer/controls.js` — This file connects the buttons to the play, pause, next, and previous commands.

We added `package.json` to start the app, and a default cover image in `assets/`.

## Current State

The app opens with no errors, even with no music player and no cava installed.

- If there is no music player, the app shows "No player detected."
- If cava is not installed, the app shows a simple test animation instead of real sound data.

We checked all main files for code errors. We found no errors.

## Next Steps

- Install the app and test it with a real music player.
- Test the app with real cava sound data.
- Check the button bar position. Change the visualizer gap size if needed.
- Test album covers from different music players.
