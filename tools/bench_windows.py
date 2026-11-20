#!/usr/bin/env python3
"""Time how fast a GUI program opens a file and responds to input, on Windows.

open      Starts the program with the file and reports when it has finished loading, and its
          peak memory: the same rule as bench_open.py on Linux, so the numbers compare.
interact  Starts the program with the file, waits until it is loaded, then sends real
          keyboard and mouse input (SendInput) for a fixed list of actions and times each
          one until the program is idle again.

"Idle" is measured from the outside, from the CPU the program's process uses, so every
program is measured the same way and none needs to be changed. For `open` that is CPU time
(GetProcessTimes, about 15 ms resolution); for `interact` it is CPU cycles
(QueryProcessCycleTime), which count exactly, so short actions can be told apart.

While `interact` runs it moves the mouse and types: do not touch the machine.

Windows only; standard library only.
"""
import argparse, ctypes, json, math, os, re, statistics, subprocess, sys, tempfile, time
from ctypes import wintypes as wt

# A settings file of its own for XML Atlas, so every run starts from the default layout.
SETTINGS = os.path.join(tempfile.gettempdir(), "xml-atlas-bench-settings.toml")

PROFILES = {
    # Where to click, as fractions of the maximized window's client area (x, y), found on a
    # 1920 x 1200 screen at 100 % scale (XML Marker's root row sits below its toolbar, so its
    # fraction depends on the screen height). `tree_setup` leaves the tree's root selected
    # and collapsed; then both programs get the same keys.
    "atlas": {
        # Default layout: tree on the left, text at the top right, grid below it.
        "tree_setup": [("click", (0.10, 0.50)), ("key", "home"), ("key", "left")],
        "tree_low_row": (0.10, 0.85),
        "text": (0.50, 0.30),
        "find_open": "ctrl+f",  # a find bar; Enter finds the next match
    },
    "marker": {
        # Tree at the top left, text at the top right, selection browsers below. The tree
        # starts collapsed; a click on the root row selects it.
        "tree_setup": [("click", (0.03, 0.035))],
        "tree_low_row": (0.04, 0.60),
        "text": (0.70, 0.30),
        "find_open": "ctrl+f",  # a Find dialog; Enter finds the next match, it stays open
    },
}

PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
PROCESS_VM_READ = 0x0010
INPUT_MOUSE, INPUT_KEYBOARD = 0, 1
KEYEVENTF_KEYUP, KEYEVENTF_UNICODE = 0x0002, 0x0004
MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP = 0x0002, 0x0004
SW_MAXIMIZE = 3
VK = {
    "ctrl": 0x11, "shift": 0x10, "alt": 0x12, "enter": 0x0D, "escape": 0x1B,
    "home": 0x24, "end": 0x23, "pagedown": 0x22, "pageup": 0x21, "f3": 0x72,
    "down": 0x28, "up": 0x26, "left": 0x25, "right": 0x27,
}
EXTENDED = {"home", "end", "pagedown", "pageup", "down", "up", "left", "right"}
for c in "abcdefghijklmnopqrstuvwxyz":
    VK[c] = ord(c.upper())

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
user32 = ctypes.WinDLL("user32", use_last_error=True)
psapi = ctypes.WinDLL("psapi", use_last_error=True)


class PROCESS_MEMORY_COUNTERS(ctypes.Structure):
    _fields_ = [("cb", wt.DWORD), ("PageFaultCount", wt.DWORD),
                ("PeakWorkingSetSize", ctypes.c_size_t), ("WorkingSetSize", ctypes.c_size_t),
                ("QuotaPeakPagedPoolUsage", ctypes.c_size_t), ("QuotaPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t), ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                ("PagefileUsage", ctypes.c_size_t), ("PeakPagefileUsage", ctypes.c_size_t)]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [("dx", wt.LONG), ("dy", wt.LONG), ("mouseData", wt.DWORD), ("dwFlags", wt.DWORD),
                ("time", wt.DWORD), ("dwExtraInfo", ctypes.c_size_t)]


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", wt.WORD), ("wScan", wt.WORD), ("dwFlags", wt.DWORD), ("time", wt.DWORD),
                ("dwExtraInfo", ctypes.c_size_t)]


class _INPUTUNION(ctypes.Union):
    _fields_ = [("mi", MOUSEINPUT), ("ki", KEYBDINPUT), ("pad", ctypes.c_byte * 32)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wt.DWORD), ("u", _INPUTUNION)]


kernel32.OpenProcess.restype = wt.HANDLE
kernel32.QueryProcessCycleTime.argtypes = [wt.HANDLE, ctypes.POINTER(ctypes.c_ulonglong)]
kernel32.GetProcessTimes.argtypes = [wt.HANDLE] + [ctypes.POINTER(wt.FILETIME)] * 4
kernel32.GetCurrentProcess.restype = wt.HANDLE
psapi.GetProcessMemoryInfo.argtypes = [wt.HANDLE, ctypes.POINTER(PROCESS_MEMORY_COUNTERS), wt.DWORD]
user32.SendInput.argtypes = [wt.UINT, ctypes.POINTER(INPUT), ctypes.c_int]
WNDENUMPROC = ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)


class Process:
    """A started program: its CPU cycles, CPU time, memory and main window."""

    def __init__(self, cmd):
        # XML Atlas remembers its layout and recent files; keep the user's settings out of it.
        if os.path.exists(SETTINGS):
            os.remove(SETTINGS)
        env = dict(os.environ, XML_ATLAS_SETTINGS=SETTINGS)
        self.proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                     env=env)
        self.start = time.perf_counter()
        self.handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
                                           False, self.proc.pid)
        if not self.handle:
            raise OSError(ctypes.get_last_error(), "OpenProcess")

    def cycles(self):
        n = ctypes.c_ulonglong()
        kernel32.QueryProcessCycleTime(self.handle, ctypes.byref(n))
        return n.value

    def cpu_seconds(self):
        c, e, k, u = (wt.FILETIME() for _ in range(4))
        kernel32.GetProcessTimes(self.handle, *(ctypes.byref(t) for t in (c, e, k, u)))
        return sum((t.dwHighDateTime << 32 | t.dwLowDateTime) for t in (k, u)) / 1e7

    def peak_mib(self):
        m = PROCESS_MEMORY_COUNTERS()
        m.cb = ctypes.sizeof(m)
        psapi.GetProcessMemoryInfo(self.handle, ctypes.byref(m), m.cb)
        return m.PeakWorkingSetSize / 2**20

    def window(self):
        """The largest visible top-level window of the process, or None."""
        found = []

        def each(hwnd, _):
            pid = wt.DWORD()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            if pid.value == self.proc.pid and user32.IsWindowVisible(hwnd):
                r = wt.RECT()
                user32.GetWindowRect(hwnd, ctypes.byref(r))
                found.append(((r.right - r.left) * (r.bottom - r.top), hwnd))
            return True

        user32.EnumWindows(WNDENUMPROC(each), 0)
        return max(found)[1] if found else None

    def title(self, hwnd):
        buf = ctypes.create_unicode_buffer(512)
        user32.GetWindowTextW(hwnd, buf, 512)
        return buf.value

    def alive(self):
        return self.proc.poll() is None

    def kill(self):
        if self.alive():
            self.proc.kill()
        self.proc.wait()
        kernel32.CloseHandle(self.handle)


def core_rate():
    """CPU cycles per second of one busy core, measured by spinning this process."""
    me = kernel32.GetCurrentProcess()
    n0 = ctypes.c_ulonglong()
    kernel32.QueryProcessCycleTime(me, ctypes.byref(n0))
    t0 = time.perf_counter()
    while time.perf_counter() - t0 < 0.3:
        pass
    n1 = ctypes.c_ulonglong()
    kernel32.QueryProcessCycleTime(me, ctypes.byref(n1))
    return (n1.value - n0.value) / (time.perf_counter() - t0)


def wait_loaded(p, settle, timeout, busy, poll=0.005, window=0.2):
    """Seconds from start until the program was last busy before `settle` idle seconds.

    The rule of bench_open.py: busy means above `busy` of one core over the last `window`
    seconds, from CPU time.
    """
    history, last_busy = [], 0.0
    while True:
        now = time.perf_counter() - p.start
        cpu = p.cpu_seconds()
        history.append((now, cpu))
        while len(history) > 2 and history[1][0] <= now - window:
            history.pop(0)
        span = now - history[0][0]
        if span > 0 and (cpu - history[0][1]) / span >= busy:
            last_busy = now
        if cpu > 0.02 and now - last_busy >= settle:
            return last_busy
        if not p.alive():
            raise RuntimeError(f"program exited with code {p.proc.returncode} before going idle")
        if now > timeout:
            raise RuntimeError(f"still busy after {timeout} s")
        time.sleep(poll)


def wait_idle(p, t0, rate, settle, timeout, busy, pause=0.015, step=0.002, window=0.010,
              give_up=False):
    """(first, settled): seconds from `t0` until the program's first pause and until it was
    last busy before `settle` idle seconds. With `give_up`, settled is infinite when the
    program is still busy after `timeout` seconds; otherwise that raises.

    Busy means above `busy` of one core over the last `window` seconds, from CPU cycles.
    `first` ends at the first pause of `pause` seconds after any work: the first frame with
    the result. `settled` also covers what follows, such as animations. Both are 0 when the
    program never got busy: the action took less than one window.
    """
    history, last_busy, first = [], t0, None
    while True:
        now = time.perf_counter()
        history.append((now, p.cycles()))
        while len(history) > 2 and history[1][0] <= now - window:
            history.pop(0)
        span = now - history[0][0]
        if span >= window / 2 and (history[-1][1] - history[0][1]) / span / rate >= busy:
            last_busy = now
        if first is None and last_busy > t0 and now - last_busy >= pause:
            first = last_busy - t0
        if now - last_busy >= settle:
            return (first or 0.0), last_busy - t0
        if not p.alive():
            raise RuntimeError("program exited")
        if now - t0 > timeout:
            if give_up:
                return (first or 0.0), math.inf
            raise RuntimeError(f"still busy after {timeout} s")
        time.sleep(step)


# --- input -----------------------------------------------------------------------------

def _send(*inputs):
    arr = (INPUT * len(inputs))(*inputs)
    if user32.SendInput(len(inputs), arr, ctypes.sizeof(INPUT)) != len(inputs):
        raise OSError(ctypes.get_last_error(), "SendInput (is another window in front?)")


def _key(name, up):
    i = INPUT(type=INPUT_KEYBOARD)
    i.u.ki.wVk = VK[name]
    i.u.ki.dwFlags = (KEYEVENTF_KEYUP if up else 0) | (1 if name in EXTENDED else 0)
    return i


def press(chord):
    """A chord such as "ctrl+end" or "f3"."""
    names = chord.split("+")
    _send(*[_key(n, False) for n in names], *[_key(n, True) for n in reversed(names)])


def type_text(text):
    inputs = []
    for ch in text:
        for up in (False, True):
            i = INPUT(type=INPUT_KEYBOARD)
            i.u.ki.wScan = ord(ch)
            i.u.ki.dwFlags = KEYEVENTF_UNICODE | (KEYEVENTF_KEYUP if up else 0)
            inputs.append(i)
    _send(*inputs)


def click(hwnd, fx, fy):
    """Left click at a fraction of the window's client area."""
    r = wt.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(r))
    pt = wt.POINT(int(r.right * fx), int(r.bottom * fy))
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    user32.SetCursorPos(pt.x, pt.y)
    down, up = INPUT(type=INPUT_MOUSE), INPUT(type=INPUT_MOUSE)
    down.u.mi.dwFlags, up.u.mi.dwFlags = MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP
    _send(down, up)


def to_front(hwnd):
    user32.ShowWindow(hwnd, SW_MAXIMIZE)
    # Windows lets a process bring a window to the front only right after input of its own.
    press("alt")
    user32.SetForegroundWindow(hwnd)


# --- runs ------------------------------------------------------------------------------

def last_order_id(path):
    """The id of the fixture's last order: it occurs once, at the very end."""
    with open(path, "rb") as f:
        f.seek(0, 2)
        f.seek(max(0, f.tell() - (1 << 16)))
        ids = re.findall(rb'<order id="(\d+)"', f.read())
    if not ids:
        sys.exit(f"{path}: no <order id=...> near the end; is it a gen-fixture file?")
    return ids[-1].decode()


def actions(profile, term):
    """(name, setup inputs, timed inputs). Setup runs and settles before the clock starts.

    The search starts at the top of the text and finds the last order, so it reads the whole
    file. F3 after it is left out: XML Atlas wraps around to the top, XML Marker stops.
    """
    text = profile["text"]
    many_pages = [("key", "pagedown")] * 20
    return [
        ("tree: expand the root", profile["tree_setup"], [("key", "right")]),
        ("tree: End", [], [("key", "end")]),
        ("tree: Home", [], [("key", "home")]),
        ("tree: 20 x Page Down", [], many_pages),
        ("tree: click a row near the end", [("key", "end")],
         [("click", profile["tree_low_row"])]),
        ("text: Ctrl+End", [("click", text)], [("key", "ctrl+end")]),
        ("text: Ctrl+Home", [], [("key", "ctrl+home")]),
        ("text: 20 x Page Down", [], many_pages),
        ("find: last order, from the top",
         [("key", "ctrl+home"), ("key", profile["find_open"]), ("type", f'id="{term}"')],
         [("key", "enter")]),
    ]


def do(hwnd, steps, gap=0.0):
    for kind, arg in steps:
        if kind == "key":
            press(arg)
        elif kind == "type":
            type_text(arg)
        elif kind == "click":
            click(hwnd, *arg)
        if gap:
            time.sleep(gap)


def interact_once(cmd, profile, term, rate, args):
    p = Process(cmd)
    try:
        loaded = wait_loaded(p, args.settle, args.timeout, args.busy)
        hwnd = p.window()
        if not hwnd:
            raise RuntimeError("no window")
        to_front(hwnd)
        wait_idle(p, time.perf_counter(), rate, args.idle, args.timeout, args.busy)
        times = {}
        for name, setup, timed in actions(profile, term):
            front = user32.GetForegroundWindow()
            pid = wt.DWORD()
            user32.GetWindowThreadProcessId(front, ctypes.byref(pid))
            if pid.value != p.proc.pid:
                raise RuntimeError(f"before {name!r}: the program is not in front "
                                   f"(front: {p.title(front)!r})")
            if setup:
                do(hwnd, setup, gap=0.05)
                wait_idle(p, time.perf_counter(), rate, args.idle, args.timeout, args.busy)
            t0 = time.perf_counter()
            do(hwnd, timed)
            times[name] = wait_idle(p, t0, rate, args.idle, args.action_timeout, args.busy,
                                    give_up=True)
            if times[name][1] == math.inf:
                break  # still busy: the actions after it would not start from a known state
        return {"loaded": loaded, "peak": p.peak_mib(), "title": p.title(hwnd), "times": times}
    finally:
        p.kill()


def open_once(cmd, args):
    p = Process(cmd)
    try:
        ready = wait_loaded(p, args.settle, args.timeout, args.busy)
        hwnd = p.window()
        return {"ready": ready, "cpu": p.cpu_seconds(), "peak": p.peak_mib(),
                "title": p.title(hwnd) if hwnd else None}
    finally:
        p.kill()


def ms(s):
    return "over limit" if s == math.inf else f"{s * 1000:.0f} ms"


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("mode", choices=["open", "interact"])
    ap.add_argument("--profile", choices=sorted(PROFILES), help="interact: whose keys and panes")
    ap.add_argument("--file", help="interact: the gen-fixture file the program opens (for the search term)")
    ap.add_argument("--label", default=None)
    ap.add_argument("--repeat", type=int, default=3)
    ap.add_argument("--settle", type=float, default=1.0, help="idle seconds that count as loaded")
    # Shorter than a text cursor's blink (0.5 s), which wakes the program for a frame.
    ap.add_argument("--idle", type=float, default=0.3, help="interact: idle seconds that end an action")
    ap.add_argument("--busy", type=float, default=0.3, help="share of one core that counts as busy")
    ap.add_argument("--timeout", type=float, default=300.0, help="seconds to load")
    ap.add_argument("--action-timeout", type=float, default=120.0,
                    help="interact: seconds after which an action counts as over the limit")
    ap.add_argument("--json", help="also write the runs to this file")
    ap.usage = "%(prog)s {open,interact} [options] -- program [arguments]"
    argv = sys.argv[1:]
    if "--" not in argv:
        ap.error("no command given after --")
    split = argv.index("--")
    args, cmd = ap.parse_args(argv[:split]), argv[split + 1:]
    if not cmd:
        ap.error("no command given after --")
    ctypes.windll.user32.SetProcessDpiAwarenessContext(ctypes.c_void_p(-4))  # physical pixels
    label = args.label or cmd[0]
    runs = []
    if args.mode == "open":
        for i in range(args.repeat):
            r = open_once(cmd, args)
            runs.append(r)
            print(f"  run {i + 1}: ready {r['ready']:.2f} s  cpu {r['cpu']:.2f} s  "
                  f"peak {r['peak']:.0f} MiB  title {r['title']!r}", flush=True)
            time.sleep(0.5)
        ready = sorted(r["ready"] for r in runs)
        print(f"{label}: ready best {ready[0]:.2f} s, median {statistics.median(ready):.2f} s; "
              f"peak median {statistics.median(r['peak'] for r in runs):.0f} MiB")
    else:
        if not args.profile or not args.file:
            ap.error("interact needs --profile and --file")
        term = last_order_id(args.file)
        rate = core_rate()
        print(f"  one core: {rate / 1e9:.2f} GHz of cycles; search term id=\"{term}\"", flush=True)
        for i in range(args.repeat):
            r = interact_once(cmd, PROFILES[args.profile], term, rate, args)
            runs.append(r)
            print(f"  run {i + 1}: loaded {r['loaded']:.2f} s, peak {r['peak']:.0f} MiB, "
                  f"title {r['title']!r}", flush=True)
            for name, (first, settled) in r["times"].items():
                print(f"    {name:34} first {ms(first):>9}  settled {ms(settled):>9}", flush=True)
            time.sleep(0.5)
        print(f"{label}: median of {len(runs)} runs, loaded "
              f"{statistics.median(r['loaded'] for r in runs):.2f} s, peak "
              f"{statistics.median(r['peak'] for r in runs):.0f} MiB")
        for name, _, _ in actions(PROFILES[args.profile], term):
            done = [r["times"][name] for r in runs if name in r["times"]]
            if not done:
                continue
            first = statistics.median(t[0] for t in done)
            settled = statistics.median(t[1] for t in done)
            print(f"  {name:34} first {ms(first):>9}  settled {ms(settled):>9}")
    if args.json:
        with open(args.json, "w") as f:
            json.dump({"label": label, "mode": args.mode, "cmd": cmd, "runs": runs}, f, indent=1)


if __name__ == "__main__":
    main()
