const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('player', {
  // --- outgoing: renderer -> main -----------------------------------
  playPause: () => ipcRenderer.send('control', 'play-pause'),
  next: () => ipcRenderer.send('control', 'next'),
  previous: () => ipcRenderer.send('control', 'previous'),

  // --- incoming: main -> renderer -------------------------------------
  onMetadata: (callback) => {
    ipcRenderer.on('metadata', (_event, data) => callback(data));
  },
  onStatus: (callback) => {
    ipcRenderer.on('status', (_event, status) => callback(status));
  },
  onNoPlayer: (callback) => {
    ipcRenderer.on('no-player', () => callback());
  },
  onVizFrame: (callback) => {
    ipcRenderer.on('viz-frame', (_event, bins) => callback(bins));
  },
});
