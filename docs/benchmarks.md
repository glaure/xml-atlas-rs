# Benchmarks

How fast XML Atlas opens files and responds once they are open, compared with XML Marker 1.1.
The Linux results come first; the Windows results, with XML Marker running natively and the
first measurements of scrolling, selecting and searching, follow in
[Windows](#windows-2026-11-16).

## Result (2026-11-14)

Time from starting the program with a file until it has finished loading, and peak memory.
Median of three runs, except where noted.

| File                     | XML Marker 1.1 (wine) | XML Atlas 0.1     |
|--------------------------|-----------------------|-------------------|
| tiny (start-up only)     | 0.49 s, 59 MiB        | 0.59 s, 193 MiB   |
| 10 MiB, 0.34 M nodes     | 1.11 s, 102 MiB       | 0.59 s, 222 MiB   |
| 100 MiB, 3.4 M nodes     | 10.7 s, 490 MiB       | 1.02 s, 470 MiB   |
| 300 MiB, 10.3 M nodes    | 58 s, 1352 MiB (1 run)| 1.94 s, 1023 MiB  |

What this says:

- **Large files:** XML Atlas opens the 100 MiB file about 10 times faster and the 300 MiB file
  about 30 times faster. XML Marker's time grows faster than the file size.
- **Small files and start-up:** XML Marker wins. It starts in 0.49 s with 59 MiB even under
  wine. XML Atlas needs 0.59 s and 193 MiB before it has read a single byte. That is the cost
  of the egui window and its GPU renderer, and it is what users of small files will notice.
- **Memory:** about equal at 100 MiB. XML Marker is a 32-bit program, so it cannot go far beyond
  the 300 MiB file (1.35 GiB of its 2 GiB address space).

What this does not say:

- Nothing about how the programs feel once the file is open: scrolling, selecting, searching.
  Those are not measured yet.
- Nothing about XML Marker on Windows. Under wine its code runs natively, but memory
  allocation and file access go through wine and may be slower.
- XML Marker edits; XML Atlas only views so far. An editor's data structures cost more.
- The files are synthetic: regular, shallow (depth 5), UTF-8. Real files will differ.
- The Qt XML Atlas is not in the table. It has no command-line file argument, so the harness
  cannot drive it.

## Start-up with OpenGL (2026-11-15)

Since D21 the window draws with OpenGL instead of wgpu. Same harness and machine, median of
two runs, two alternating rounds against the wgpu build:

| File     | XML Atlas, wgpu       | XML Atlas, OpenGL     | XML Marker 1.1 (wine) |
|----------|-----------------------|-----------------------|-----------------------|
| tiny     | 0.55-0.60 s, 195 MiB  | 0.31-0.33 s, 132 MiB  | 0.49 s, 59 MiB        |
| 10 MiB   | 0.59-0.61 s, 216 MiB  | 0.35-0.38 s, 176 MiB  | 1.11 s, 102 MiB       |
| 100 MiB  | 0.93-0.96 s, 390 MiB  | 0.73 s, 349 MiB       | 10.7 s, 490 MiB       |

XML Atlas now shows a small file sooner than XML Marker; XML Marker still uses less memory
with small files.

## Method

`tools/bench_open.py` starts the program with the file as its argument and samples the CPU
time of every process in the program's session from `/proc`. The file counts as open at the
last moment the program used more than 30 % of one core, before a settle period (1 to 3 s)
in which it stayed below that. Peak memory is the high-water mark of resident memory.

The threshold is 30 % and not zero because XML Marker under wine never goes idle: it uses
about 9 % of a core with a tiny file open.

After each run the harness lists the program's window titles. All XML Marker runs showed the
file name in the title and no error dialog.

Timer resolution is 10 ms. Files were in the page cache.

## Reproduce

```sh
cargo build --release
cargo xtask gen-fixture --mib 10  --out fixtures/orders-10.xml
cargo xtask gen-fixture --mib 100 --out fixtures/orders.xml
cargo xtask gen-fixture --mib 300 --out fixtures/orders-300.xml

# XML Atlas
tools/bench_open.py --repeat 3 -- target/release/xml-atlas fixtures/orders.xml

# XML Marker under wine (Z: maps to /)
tools/bench_open.py --repeat 3 --settle 2 --timeout 120 -- \
    wine 'C:\Program Files (x86)\XML Marker\xmlmarker.exe' \
    "Z:$(pwd | tr / '\\')\\fixtures\\orders.xml"
```

The index alone, without a window:

```sh
target/release/xml-atlas-cli stats fixtures/orders.xml --repeat 5
```

| File     | Read   | Index  | Lines | Total  | Index memory |
|----------|--------|--------|-------|--------|--------------|
| 100 MiB  | 53 ms  | 319 ms | 14 ms | 386 ms | 182 MiB      |
| 300 MiB  | 155 ms | 961 ms | 66 ms | 1.18 s | 545 MiB      |

Index memory is the node index plus the line index.

So of the 1.02 s to open 100 MiB in the window, about 0.4 s is loading and about 0.6 s is
start-up.

## Machine

AMD Ryzen 9 3900X (12 cores), 31 GiB RAM, Ubuntu with Linux 7.0, GNOME on Wayland, wine 10.0,
Rust 1.99.1, release build with thin LTO.

## Windows (2026-11-16)

The same fixtures on Windows 11, with XML Marker 1.1 running natively instead of under wine,
measured by `tools/bench_windows.py`. Median of three runs unless noted.

### Opening

| File                  | XML Marker 1.1    | XML Atlas 0.1    |
|-----------------------|-------------------|------------------|
| tiny (start-up only)  | 0.53 s, 22 MiB    | 0.50 s, 111 MiB  |
| 10 MiB                | 1.48 s, 62 MiB    | 0.78 s, 131 MiB  |
| 100 MiB               | 15.6 s, 438 MiB   | 1.14 s, 307 MiB  |
| 300 MiB               | 56.5 s, 1271 MiB  | 2.56 s, 700 MiB  |

Memory is the peak working set. Natively, XML Marker needs far less memory with small files
than under wine (22 MiB against 59 MiB) but is not faster to open large ones: 15.6 s against
10.7 s for 100 MiB on the Linux machine.

### Once the file is open

How long the program is busy after each action, in milliseconds. *First* ends at the
program's first pause: the first frame with the result. *Settled* ends when it has been idle
for 0.3 s, so it also covers animations and work done in the background between frames.
XML Marker shows no difference between the two. Lower is better; 0 means less than about
10 ms.

| Action                          | Marker 10 | Atlas 10  | Marker 100 | Atlas 100  | Marker 300 | Atlas 300 (2 runs) |
|---------------------------------|-----------|-----------|------------|------------|------------|--------------------|
| tree: expand the root           | 44        | 234 / 234 | 166        | 50 / 195   | 391        | 230                |
| tree: End                       | 0         | 218 / 218 | 0          | 49 / 199   | 0          | 218                |
| tree: Home                      | 7         | 51 / 51   | 0          | 183 / 183  | 0          | 665-713            |
| tree: 20 x Page Down            | 27        | 144 / 231 | 32         | 34 / 214   | 20         | 228-231            |
| tree: click a row near the end  | 61        | 225 / 225 | 66         | 42 / 192   | 59         | 216                |
| text: Ctrl+End                  | 23        | 30 / 30   | 21         | 13 / 13    | 21         | 32-34              |
| text: Ctrl+Home                 | 26        | 33 / 33   | 26         | 13 / 13    | 18         | 28-33              |
| text: 20 x Page Down            | 61        | 48 / 48   | 67         | 33 / 33    | 61         | 145-160            |
| find: last order, from the top  | 439       | 996 / 1163| 3845       | 49 / 563   | 10934      | see below          |

XML Atlas columns show *first / settled*; the 300 MiB column shows the settled range of the
two runs that finished.

What this says:

- **Search:** XML Marker searches at about 27 MiB/s (3.8 s for 100 MiB, 10.9 s for 300 MiB).
  XML Atlas's first pause came after about 50 ms at 100 MiB but about 1 s at 10 MiB, so its
  search time is not measured well yet: when frames follow each other closely, *first* only
  ends at the first gap between them.
- **Scrolling the text** is about equal, and a frame or two in both programs, except XML
  Atlas's 20 x Page Down at 300 MiB (145-160 ms against 33-48 ms at smaller sizes).
- **The tree:** XML Marker answers within a frame or two at every size. Most of XML Atlas's
  200-230 ms is an animation of a few frames after a quick first frame (the *first* column at
  100 MiB). Its Home at 300 MiB (665-713 ms) is not: it grows with the file.

Problems in XML Atlas that this found, to look into before the tokenizer (D14):

- **It does not always go idle on large files.** While the grid's columns are not all known,
  every frame spends up to 3 ms looking at more rows (GRID-3) and asks for the next frame.
  At 100 MiB one search settled only after 15 s; at 300 MiB one search settled after 77 s,
  one after 0.8 s, and in two further runs the program was still busy after 240 s and
  300 s. Looking at 633,000 rows in 3 ms slices should take seconds, so something makes it
  start again or take far longer; a core stays busy, which drains a laptop's battery.
- **Tree Home and text Page Down cost more as the file grows**, against rule 2 (per-frame
  work scales with what is on screen).
- **Animations** make a tree action take about 200 ms where XML Marker takes one frame.

What this does not say:

- The numbers are CPU activity seen from outside, not the moment pixels change. A program that
  draws without using the CPU, or works without drawing, is timed wrong.
- The machine is a laptop with a hybrid processor; the same action varies up to two-fold
  between sessions (XML Atlas's Ctrl+Home took 28-34 ms at 10 and 300 MiB in one session and
  13 ms at 100 MiB in another, though it does not depend on the file). Differences smaller
  than that are noise.
- XML Atlas's 10 MiB and 300 MiB columns come from one session, its 100 MiB column from a
  second one; XML Marker's from the first.

### Method (Windows)

`tools/bench_windows.py open` starts the program with the file and applies the rule of
`bench_open.py`: the file is open at the last moment the program used more than 30 % of one
core (over 0.2 s, from `GetProcessTimes`) before 1 s below that.

`tools/bench_windows.py interact` then maximizes the window and sends real keyboard and mouse
input (`SendInput`) for each action, timing it with `QueryProcessCycleTime`, which counts CPU
cycles exactly: busy means above 30 % of one core over 10 ms. The idle period that ends an
action is 0.3 s, shorter than the 0.5 s blink of a text cursor, which wakes XML Atlas for a
frame. Before each action a setup (such as clicking into the pane) runs and settles untimed.
Both programs start each tree action with the root selected and collapsed. The search
starts at the top of the text and looks for the last order's id, which occurs once, so it
reads the whole file; F3 afterwards is left out, because XML Atlas wraps around to the top
and XML Marker stops.

XML Atlas runs with a settings file of its own (`XML_ATLAS_SETTINGS`), so every run starts
from the default layout and the user's settings stay untouched. The click positions are
fractions of the window found on a 1920 x 1200 screen at 100 % scale; XML Marker's root row
sits below its toolbar, so on another screen height its fraction may need adjusting.

### Reproduce (Windows)

Do not touch the mouse or keyboard while `interact` runs.

```powershell
cargo build --release
cargo xtask gen-fixture --mib 10  --out fixtures/orders-10.xml
cargo xtask gen-fixture --mib 100 --out fixtures/orders.xml
cargo xtask gen-fixture --mib 300 --out fixtures/orders-300.xml
$marker = 'C:\Program Files (x86)\XML Marker\xmlmarker.exe'
$file = (Resolve-Path fixtures\orders.xml).Path

python tools\bench_windows.py open -- target\release\xml-atlas.exe $file
python tools\bench_windows.py open -- $marker $file
python tools\bench_windows.py interact --profile atlas --file $file -- target\release\xml-atlas.exe $file
python tools\bench_windows.py interact --profile marker --file $file -- $marker $file
```

### Machine (Windows)

Intel Core Ultra 7 268V (4 performance and 4 efficient cores), 32 GiB RAM, Windows 11
Enterprise, 1920 x 1200 at 100 %, Rust 1.99.0, release build with thin LTO.
