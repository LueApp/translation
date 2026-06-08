/**
 * web2local client library
 * Include this in any website that wants to talk to the local daemon.
 *
 * Usage:
 *   const w2l = new Web2Local();
 *   if (await w2l.isRunning()) {
 *     const r = await w2l.run("ls", ["-la", "/tmp"]);
 *     console.log(r.stdout);
 *   }
 */
class Web2Local {
  /**
   * @param {number} port - daemon port (default 7878)
   */
  constructor(port = 7878) {
    this.base = `http://127.0.0.1:${port}`;
  }

  /**
   * Check whether the local daemon is reachable.
   * @returns {Promise<boolean>}
   */
  async isRunning() {
    try {
      const r = await fetch(`${this.base}/status`);
      return r.ok;
    } catch {
      return false;
    }
  }

  /**
   * Return daemon status info.
   * @returns {Promise<{status:string, version:string}>}
   */
  async status() {
    const r = await fetch(`${this.base}/status`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return r.json();
  }

  /**
   * Execute a command on the local machine.
   * - Whitelist origins: executed immediately.
   * - Graylist origins: user sees an approval dialog first.
   *
   * @param {string}   command  - executable name (no shell, no PATH tricks)
   * @param {string[]} args     - argument list
   * @returns {Promise<{stdout:string, stderr:string, exit_code:number}>}
   * @throws  on network error or if denied / not authorised
   */
  async run(command, args = []) {
    const r = await fetch(`${this.base}/run`, {
      method:  "POST",
      headers: { "Content-Type": "application/json" },
      body:    JSON.stringify({ command, args }),
    });
    const data = await r.json();
    if (!r.ok) throw new Error(data.error || `HTTP ${r.status}`);
    return data;
  }

  /**
   * Read current whitelist / graylist config from the daemon.
   * @returns {Promise<{port:number, whitelist:string[], graylist:string[]}>}
   */
  async getConfig() {
    const r = await fetch(`${this.base}/config`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return r.json();
  }

  /**
   * Add an origin to the whitelist (removes it from graylist if present).
   * @param {string} origin - e.g. "https://mysite.com"
   */
  async addToWhitelist(origin) {
    return this._postConfig("/config/whitelist", { origin });
  }

  /**
   * Add an origin to the graylist (removes it from whitelist if present).
   * @param {string} origin - e.g. "https://mysite.com"
   */
  async addToGraylist(origin) {
    return this._postConfig("/config/graylist", { origin });
  }

  /**
   * Remove an origin from both lists.
   * @param {string} origin
   */
  async removeOrigin(origin) {
    return this._postConfig("/config/remove", { origin });
  }

  /**
   * Fetch the last 200 audit log entries.
   * @returns {Promise<{entries:string[]}>}
   */
  async getLog() {
    const r = await fetch(`${this.base}/log`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return r.json();
  }

  /**
   * Start a long-running process. Returns immediately with its PID.
   * Output is captured to a log file the daemon can tail via tailLog().
   *
   * @param {string}   command
   * @param {string[]} args
   * @returns {Promise<{pid:number, started_at:string, log_path:string}>}
   */
  async spawn(command, args = []) {
    const r = await fetch(`${this.base}/spawn`, {
      method:  "POST",
      headers: { "Content-Type": "application/json" },
      body:    JSON.stringify({ command, args }),
    });
    const data = await r.json();
    if (!r.ok) throw new Error(data.error || `HTTP ${r.status}`);
    return data;
  }

  /**
   * List processes spawned via spawn() that are still alive.
   * @returns {Promise<{processes: Array}>}
   */
  async ps() {
    const r = await fetch(`${this.base}/ps`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return r.json();
  }

  /**
   * Stop a process by PID. Sends SIGTERM, then SIGKILL after 3 s.
   * @param {number} pid
   * @returns {Promise<{status:string, signal?:string}>}
   */
  async stop(pid) {
    const r = await fetch(`${this.base}/stop`, {
      method:  "POST",
      headers: { "Content-Type": "application/json" },
      body:    JSON.stringify({ pid }),
    });
    const data = await r.json();
    if (!r.ok) throw new Error(data.error || `HTTP ${r.status}`);
    return data;
  }

  /**
   * Tail the last 200 lines of a spawned process's output.
   * @param {number} pid
   * @returns {Promise<{pid:number, tail:string}>}
   */
  async tailLog(pid) {
    const r = await fetch(`${this.base}/logs?pid=${encodeURIComponent(pid)}`);
    if (!r.ok) throw new Error(`HTTP ${r.status}`);
    return r.json();
  }

  /**
   * Fetch a script from the page's own origin, verify its SHA-256, hand
   * the source to the daemon, and spawn it — all in one round-trip.
   *
   * The daemon will ALWAYS show a native approval dialog before writing or
   * running anything, regardless of whitelist status.
   *
   * @param {object} opts
   * @param {string}   opts.source   - Full source of the script (UTF-8 string).
   * @param {string}   opts.sha256   - Expected SHA-256 hex digest (64 chars).
   *                                   Must match sha256(source); daemon rejects
   *                                   on mismatch before showing any dialog.
   * @param {string}   [opts.filename="agent.py"] - Suggested filename on disk.
   * @param {string}   opts.command  - Interpreter (e.g. "python3").
   * @param {string[]} [opts.args=[]] - Arguments passed after the script path.
   *
   * @returns {Promise<{pid:number, path:string, started_at:string,
   *                    log_path:string, already_running?:boolean}>}
   * @throws  if denied by user, sha256 mismatches, or daemon unreachable.
   */
  async deploy({ source, sha256, filename = "agent.py", command, args = [] }) {
    const r = await fetch(`${this.base}/deploy`, {
      method:  "POST",
      headers: { "Content-Type": "application/json" },
      body:    JSON.stringify({ source, sha256, filename, command, args }),
    });
    const data = await r.json();
    if (!r.ok) throw new Error(data.error || `HTTP ${r.status}`);
    return data;
  }

  async _postConfig(path, body) {
    const r = await fetch(`${this.base}${path}`, {
      method:  "POST",
      headers: { "Content-Type": "application/json" },
      body:    JSON.stringify(body),
    });
    const data = await r.json();
    if (!r.ok) throw new Error(data.error || `HTTP ${r.status}`);
    return data;
  }
}

// Expose as global for plain <script> inclusion
if (typeof window !== "undefined") window.Web2Local = Web2Local;
// Also export for module bundlers
if (typeof module !== "undefined") module.exports = { Web2Local };
