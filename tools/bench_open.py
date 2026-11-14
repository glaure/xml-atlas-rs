#!/usr/bin/env python3
"""Time how long a GUI program takes to open a file.

Launches the command, then watches the CPU time of every process in its session. The file
counts as open at the last moment the program was busy (above a share of one core) before a
settle period in which it stayed below that. Reports that time, measured from launch, and
peak memory.

Linux only: reads /proc.
"""
import argparse, os, signal, statistics, subprocess, sys, time

TICK = os.sysconf("SC_CLK_TCK")


def session_pids(session):
    """Pids of every live process in the session.

    The launched program gets its own session. Tracking the session instead of the child
    process matters for launchers that exit early and leave the real program running, as
    wine does.
    """
    found = set()
    for name in os.listdir("/proc"):
        if not name.isdigit():
            continue
        try:
            with open(f"/proc/{name}/stat", "rb") as f:
                data = f.read()
        except OSError:
            continue
        # The command name is in parentheses and may contain spaces; split after it.
        rest = data[data.rindex(b")") + 2:].split()
        if int(rest[3]) == session and rest[0] != b"Z":
            found.add(int(name))
    return found


def sample(pids):
    """(cpu seconds, peak resident MiB) summed over pids."""
    cpu, peak = 0.0, 0.0
    for pid in pids:
        try:
            with open(f"/proc/{pid}/stat", "rb") as f:
                data = f.read()
            rest = data[data.rindex(b")") + 2:].split()
            cpu += (int(rest[11]) + int(rest[12])) / TICK
            with open(f"/proc/{pid}/status") as f:
                for line in f:
                    if line.startswith("VmHWM:"):
                        peak += int(line.split()[1]) / 1024
                        break
        except (OSError, ValueError):
            pass
    return cpu, peak


def windows(pids):
    """Titles of X11 windows that belong to pids (best effort; needs xwininfo and xprop)."""
    titles = []
    try:
        tree = subprocess.run(["xwininfo", "-root", "-tree"], capture_output=True, text=True, timeout=5).stdout
    except (OSError, subprocess.TimeoutExpired):
        return titles
    for line in tree.splitlines():
        parts = line.split()
        if not parts or not parts[0].startswith("0x") or '"' not in line:
            continue
        title = line.split('"')[1]
        if not title:
            continue
        try:
            prop = subprocess.run(["xprop", "-id", parts[0], "_NET_WM_PID"], capture_output=True, text=True, timeout=5).stdout
            pid = int(prop.rsplit("=", 1)[1])
        except (OSError, ValueError, IndexError, subprocess.TimeoutExpired):
            continue
        if pid in pids and title not in titles:
            titles.append(title)
    return titles


def run_once(cmd, settle, timeout, busy, poll=0.005, window=0.2):
    start = time.monotonic()
    proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    history = []  # (elapsed, cpu seconds) over the last `window` seconds
    pids = {proc.pid}
    last_scan = 0.0
    last_busy = 0.0
    try:
        while True:
            now = time.monotonic() - start
            if now - last_scan > 0.1:
                pids = session_pids(proc.pid)
                last_scan = now
            cpu, _ = sample(pids)
            history.append((now, cpu))
            while len(history) > 2 and history[1][0] <= now - window:
                history.pop(0)
            span = now - history[0][0]
            # Busy: the share of one core used over the last `window` seconds is above `busy`.
            # Some programs never go fully idle (XML Marker under wine sits at about 9 %).
            if span > 0 and (cpu - history[0][1]) / span >= busy:
                last_busy = now
            if cpu > 0.02 and now - last_busy >= settle:
                ready = last_busy
                break
            if proc.poll() is not None and not pids and now - last_scan < poll * 2:
                raise RuntimeError(f"program exited with code {proc.returncode} before going idle")
            if now > timeout:
                raise RuntimeError(f"still busy after {timeout} s")
            time.sleep(poll)
        pids = session_pids(proc.pid)
        cpu, peak = sample(pids)
        return {"ready": ready, "cpu": cpu, "peak": peak, "windows": windows(pids)}
    finally:
        try:
            os.killpg(proc.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        for pid in pids:
            try:
                os.kill(pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        proc.wait()


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--label", default=None)
    ap.add_argument("--repeat", type=int, default=5)
    ap.add_argument("--settle", type=float, default=1.0, help="idle seconds that count as done")
    ap.add_argument("--busy", type=float, default=0.3, help="share of one core that counts as busy")
    ap.add_argument("--timeout", type=float, default=300.0)
    ap.add_argument("cmd", nargs=argparse.REMAINDER, help="-- program and arguments")
    args = ap.parse_args()
    cmd = args.cmd[1:] if args.cmd[:1] == ["--"] else args.cmd
    if not cmd:
        ap.error("no command given")
    runs = []
    for i in range(args.repeat):
        r = run_once(cmd, args.settle, args.timeout, args.busy)
        runs.append(r)
        print(f"  run {i + 1}: ready {r['ready']:.2f} s  cpu {r['cpu']:.2f} s  peak {r['peak']:.0f} MiB  windows {r['windows']}", flush=True)
        time.sleep(0.5)
    ready = sorted(r["ready"] for r in runs)
    print(f"{args.label or cmd[0]}: ready best {ready[0]:.2f} s, median {statistics.median(ready):.2f} s; "
          f"cpu median {statistics.median(r['cpu'] for r in runs):.2f} s; "
          f"peak median {statistics.median(r['peak'] for r in runs):.0f} MiB")


if __name__ == "__main__":
    main()
