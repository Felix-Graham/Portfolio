const { app, BrowserWindow, ipcMain } = require('electron');
const path = require('path');

const PlayerCtl = require('./playerctl');
const Cava = require('./cava');

let mainWindow = null;
let playerctl = null;
let cava = null;

const WINDOW_WIDTH = 360;
const WINDOW_HEIGHT = 420;

function createWindow() {
  mainWindow = new BrowserWindow({
    width: WINDOW_WIDTH,
    height: WINDOW_HEIGHT,
    minWidth: 240,
    minHeight: 280,
    frame: false,
    transparent: true,
    backgroundColor: '#00000000',
    resizable: true,
    alwaysOnTop: false,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

  mainWindow.loadFile(path.join(__dirname, '..', 'renderer', 'index.html'));

  // Uncomment while developing to inspect the renderer:
  // mainWindow.webContents.openDevTools({ mode: 'detach' });

  mainWindow.on('closed', () => {
    mainWindow = null;
  });
}

function wireSubprocesses() {
  playerctl = new PlayerCtl();
  cava = new Cava({ configPath: path.join(__dirname, 'config', 'cava.conf') });

  // --- playerctl -> renderer -----------------------------------------
  playerctl.on('metadata', (data) => {
    if (mainWindow) mainWindow.webContents.send('metadata', data);
  });

  playerctl.on('status', (status) => {
    if (mainWindow) mainWindow.webContents.send('status', status);
  });

  playerctl.on('no-player', () => {
    if (mainWindow) mainWindow.webContents.send('no-player');
  });

  playerctl.on('error', (err) => {
    console.error('[playerctl]', err);
  });

  // --- cava -> renderer -------------------------------------------------
  cava.on('frame', (bins) => {
    if (mainWindow) mainWindow.webContents.send('viz-frame', bins);
  });

  cava.on('error', (err) => {
    console.error('[cava]', err);
  });

  playerctl.start();
  cava.start();
}

// --- renderer -> playerctl (control commands) ----------------------------
ipcMain.on('control', (_event, action) => {
  if (!playerctl) return;
  switch (action) {
    case 'play-pause':
      playerctl.playPause();
      break;
    case 'next':
      playerctl.next();
      break;
    case 'previous':
      playerctl.previous();
      break;
    default:
      console.warn('[main] unknown control action:', action);
  }
});

app.whenReady().then(() => {
  createWindow();
  wireSubprocesses();

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit();
});

app.on('before-quit', () => {
  if (playerctl) playerctl.stop();
  if (cava) cava.stop();
});
