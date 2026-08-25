const { spawn } = require('child_process');
const { EventEmitter } = require('events');
const readline = require('readline');

const BAR_COUNT = 32; // must match `bars =` in cava.conf
const IDLE_FRAME_MS = 1000 / 30;

class Cava extends EventEmitter {
  constructor({ configPath, bars = BAR_COUNT } = {}) {
    super();
    this._configPath = configPath;
    this._bars = bars;
    this._proc = null;
    this._idleTimer = null;
    this._stopped = false;
    this._usingIdleFallback = false;
  }

  start() {
    this._stopped = false;
    this._spawnCava();
  }

  stop() {
    this._stopped = true;
    this._stopIdleFallback();
    if (this._proc) this._proc.kill();
    this._proc = null;
  }

  _spawnCava() {
    this._proc = spawn('cava', ['-p', this._configPath]);

    this._proc.on('error', (err) => {
      // Binary not found or failed to launch — fall back so the UI still
      // has *something* to animate instead of sitting dead.
      this.emit('error', err);
      this._startIdleFallback();
    });

    const rl = readline.createInterface({ input: this._proc.stdout });
    rl.on('line', (line) => {
      this._stopIdleFallback();
      const bins = line
        .trim()
        .split(';')
        .filter(Boolean)
        .map(Number);
      if (bins.length) this.emit('frame', bins);
    });

    this._proc.on('exit', (code) => {
      if (!this._stopped) {
        this._startIdleFallback();
        setTimeout(() => {
          if (!this._stopped) this._spawnCava();
        }, 2000);
      }
    });
  }

  // Gentle synthetic "breathing" animation so the ring isn't static when
  // cava isn't installed, isn't running, or there's no active audio sink.
  _startIdleFallback() {
    if (this._usingIdleFallback) return;
    this._usingIdleFallback = true;
    let t = 0;
    this._idleTimer = setInterval(() => {
      t += 0.05;
      const bins = Array.from({ length: this._bars }, (_, i) => {
        const wave = Math.sin(t + i * 0.4) * 0.5 + 0.5;
        return Math.round(wave * 40); // low-amplitude idle motion (0-255 scale)
      });
      this.emit('frame', bins);
    }, IDLE_FRAME_MS);
  }

  _stopIdleFallback() {
    if (this._idleTimer) {
      clearInterval(this._idleTimer);
      this._idleTimer = null;
    }
    this._usingIdleFallback = false;
  }
}

module.exports = Cava;
