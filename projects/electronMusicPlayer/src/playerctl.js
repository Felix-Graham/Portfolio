const { spawn, execFile } = require('child_process');
const { EventEmitter } = require('events');
const readline = require('readline');

// Format string for `playerctl --follow metadata`. Fields are pipe-separated
// so we can split on '|' — track titles/artists essentially never contain it.
const METADATA_FORMAT =
  '{{title}}|{{artist}}|{{mpris:artUrl}}|{{position}}|{{mpris:length}}';

const NO_PLAYER_GRACE_MS = 1500;

class PlayerCtl extends EventEmitter {
  constructor() {
    super();
    this._metaProc = null;
    this._statusProc = null;
    this._noPlayerTimer = null;
    this._stopped = false;
  }

  start() {
    this._stopped = false;
    this._spawnMetadataStream();
    this._spawnStatusStream();
  }

  stop() {
    this._stopped = true;
    if (this._noPlayerTimer) clearTimeout(this._noPlayerTimer);
    if (this._metaProc) this._metaProc.kill();
    if (this._statusProc) this._statusProc.kill();
    this._metaProc = null;
    this._statusProc = null;
  }

  // --- control commands (fire-and-forget one-off calls) ------------------

  playPause() {
    this._run(['play-pause']);
  }

  next() {
    this._run(['next']);
  }

  previous() {
    this._run(['previous']);
  }

  _run(args) {
    execFile('playerctl', args, (err) => {
      if (err) this.emit('error', err);
    });
  }

  // --- streams -------------------------------------------------------------

  _spawnMetadataStream() {
    this._metaProc = spawn('playerctl', [
      '--follow',
      'metadata',
      '--format',
      METADATA_FORMAT,
    ]);

    this._metaProc.on('error', (err) => this.emit('error', err));

    const rl = readline.createInterface({ input: this._metaProc.stdout });
    rl.on('line', (line) => {
      this._resetNoPlayerTimer();
      const [title, artist, artUrl, position, length] = line.split('|');
      this.emit('metadata', {
        title: title || '',
        artist: artist || '',
        artUrl: artUrl || '',
        position: Number(position) || 0,
        length: Number(length) || 0,
      });
    });

    this._metaProc.on('exit', (code) => {
      if (!this._stopped) {
        // playerctl exits when the player it was tracking disappears;
        // restart after a short delay to pick up the next active player.
        this._armNoPlayerFallback();
        setTimeout(() => {
          if (!this._stopped) this._spawnMetadataStream();
        }, 1000);
      }
    });
  }

  _spawnStatusStream() {
    this._statusProc = spawn('playerctl', ['--follow', 'status']);

    this._statusProc.on('error', (err) => this.emit('error', err));

    const rl = readline.createInterface({ input: this._statusProc.stdout });
    rl.on('line', (line) => {
      const status = line.trim();
      if (status) this.emit('status', status);
    });

    this._statusProc.on('exit', () => {
      if (!this._stopped) {
        setTimeout(() => {
          if (!this._stopped) this._spawnStatusStream();
        }, 1000);
      }
    });
  }

  // If no metadata line arrives shortly after (re)starting, assume there's
  // no active player and tell the renderer to fall back to its defaults.
  _armNoPlayerFallback() {
    if (this._noPlayerTimer) clearTimeout(this._noPlayerTimer);
    this._noPlayerTimer = setTimeout(() => {
      this.emit('no-player');
    }, NO_PLAYER_GRACE_MS);
  }

  _resetNoPlayerTimer() {
    if (this._noPlayerTimer) {
      clearTimeout(this._noPlayerTimer);
      this._noPlayerTimer = null;
    }
  }
}

module.exports = PlayerCtl;
