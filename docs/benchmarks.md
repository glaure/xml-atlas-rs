# Benchmarks

How fast XML Atlas opens files and responds once they are open, compared with XML Marker 1.1.
The latest measurements, of XML Atlas 1.2 on Linux and Windows 11, come first
([1.2](#12-2026-11-20)). Everything after them is history: the Linux results of 0.1 (XML
Marker under wine) and the Windows results of 0.1 and 0.1.1, which were measured on another
machine and must not be compared with the 1.2 tables.

## 1.2 (2026-11-20)

XML Atlas at commit `5dae1d9` (1.2.0 and nine later commits: the structure diagram, an XPath
nesting limit and CI fixes) and XML Marker 1.1, on one machine that boots Linux and Windows 11. The files are
the fixtures of `cargo xtask gen-fixture`; the tiny file is the 11 bytes `<a><b/></a>`.
Median of three runs. The raw output is in [benchmark-data/2026-11-20](benchmark-data/2026-11-20).

### Opening, Windows 11

Time from starting the program with the file until it has finished loading, and peak working
set (`tools/bench_windows.py open`).

| File                  | XML Marker 1.1    | XML Atlas 1.2    | Atlas is faster by |
|-----------------------|-------------------|------------------|--------------------|
| tiny (start-up only)  | 0.33 s, 20 MiB    | 0.52 s, 79 MiB   | 0.6 x (slower)     |
| 10 MiB                | 0.91 s, 61 MiB    | 0.56 s, 106 MiB  | 1.6 x              |
| 100 MiB               | 10.55 s, 437 MiB  | 0.59 s, 345 MiB  | 18 x               |
| 300 MiB               | 56.49 s, 1271 MiB | 0.82 s, 885 MiB  | 69 x               |

XML Marker's time grows faster than the size of the file (about 10 s for 100 MiB, 56 s for
300 MiB); XML Atlas's does not, because the index is built on several threads and the views
only look at what is on screen. XML Marker still starts sooner and uses far less memory with a
tiny file.

### Opening, Linux

XML Atlas only (`tools/bench_open.py --repeat 3`). XML Marker could not be measured: under
wine on this machine it uses about half a core even when idle, so the harness never sees it
finish loading. The wine results of 0.1 are in [Result (2026-11-14)](#result-2026-11-14).

| File                  | XML Atlas 1.2    |
|-----------------------|------------------|
| tiny (start-up only)  | 0.30 s, 140 MiB  |
| 10 MiB                | 0.35 s, 170 MiB  |
| 100 MiB               | 0.50 s, 406 MiB  |
| 300 MiB               | 0.82 s, 950 MiB  |

The index alone, without a window (`xml-atlas-cli stats --repeat 5`, best of five):

| File     | Read   | Index  | Lines (alongside) | Total  | Index memory | Elements  |
|----------|--------|--------|-------------------|--------|--------------|-----------|
| 100 MiB  | 54 ms  | 47 ms  | 24 ms             | 101 ms | 97 MiB       | 2,346,420 |
| 300 MiB  | 152 ms | 154 ms | 84 ms             | 306 ms | 290 MiB      | 7,034,241 |

The Linux console output was not kept; these values were copied from it.

### Once the file is open, Windows 11

How long the program is busy after each action, in milliseconds, as defined in
[Once the file is open](#once-the-file-is-open) below. XML Atlas's *first* and *settled*
were equal in every run, so one number is shown. 0 means under about 10 ms.

| Action                          | Marker 10 | Atlas 10 | Marker 100 | Atlas 100 | Marker 300 | Atlas 300 |
|---------------------------------|-----------|----------|------------|-----------|------------|-----------|
| tree: expand the root           | 20        | 17       | 72         | 18        | 186        | 19        |
| tree: End                       | 0         | 17       | 0          | 17        | 0          | 18        |
| tree: Home                      | 0         | 20       | 0          | 33        | 0          | 62        |
| tree: 20 x Page Down            | 0         | 37       | 0          | 39        | 0          | 39        |
| tree: click a row near the end  | 29        | 17       | 32         | 17        | 34         | 17        |
| text: Ctrl+End                  | 13        | 13       | 13         | 15        | 13         | 16        |
| text: Ctrl+Home                 | 14        | 15       | 14         | 15        | 15         | 15        |
| text: 20 x Page Down            | 43        | 35       | 43         | 37        | 42         | 40        |
| find: last order, from the top  | 239       | 18       | 2248       | 34        | 6764       | 67        |

What this says:

- **Search** is where the difference is largest: XML Marker reads the file at about 45 MiB/s
  (2.2 s for 100 MiB, 6.8 s for 300 MiB), XML Atlas finds the last order of the 300 MiB file in
  67 ms, 100 times faster.
- **Expanding the root** costs XML Marker more the larger the file (20, 72, 186 ms); XML
  Atlas stays at one or two frames (17-19 ms).
- **Text** scrolling is about equal, and a frame or two in both programs.
- **Tree End, Home and Page Down** are where XML Marker is ahead: they take it under 10 ms at
  every size, XML Atlas 17 to 39 ms (62 ms for Home at 300 MiB, the only value that grows with
  the file). In practice both feel instant.
- **Tree click near the end:** XML Atlas 17 ms, XML Marker about 30 ms.

Compared with 0.1.1 at the same sizes (2026-11-16, another machine, so only a direction):
tree Home at 300 MiB was 196 ms and is 62 ms, and no tree action takes 200 ms any more.

What this does not say:

- The Windows tables come from one session on one machine. Differences under about 10 ms
  are probably noise; the three runs of each value are in the raw files.
- XML Marker's tree rows come from a second run
  (`windows-interact-marker-rerun.txt`) made after the harness clicked the wrong row on this
  screen (`bench_windows.py`, PR #134). The first run's tree rows for XML Marker, all 0 ms, are
  not valid; its other rows agree with the second run.
- The numbers are CPU activity seen from outside, not the moment pixels change; a program
  that draws without using the CPU, or works without drawing, is timed wrong.
- The files are synthetic: regular, shallow (depth 5), UTF-8.
- Linux has no scrolling or search measurements: the harness for them is Windows only.

### Machine (1.2)

AMD Ryzen 9 3900X (12 cores), 31 GiB RAM; Ubuntu with Linux 7.0, GNOME on Wayland, Rust 1.99.1,
release build with thin LTO; the same machine booted into Windows 11.

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

*Since (2026-11-16, XML Atlas 0.1.1):* everything merged that day, above all the scanner
tuning (D14) and the index on several threads with the line starts alongside (D25). Same
method, median of three runs; the tiny file is a two-element document:

| File                  | XML Marker 1.1    | XML Atlas 0.1    | XML Atlas 0.1.1  |
|-----------------------|-------------------|------------------|------------------|
| tiny (start-up only)  | 0.53 s, 22 MiB    | 0.50 s, 111 MiB  | 0.42 s, 103 MiB  |
| 10 MiB                | 1.48 s, 62 MiB    | 0.78 s, 131 MiB  | 0.36 s, 128 MiB  |
| 100 MiB               | 15.6 s, 438 MiB   | 1.14 s, 307 MiB  | 0.52 s, 374 MiB  |
| 300 MiB               | 56.5 s, 1271 MiB  | 2.56 s, 700 MiB  | 0.80 s, 913 MiB  |

The peak memory at 100 and 300 MiB is higher by about what D25 predicts: the nodes are held
twice while the index is built. The memory once the file is open is the same as before.

At 10 MiB the index takes only about 4 ms less (D25), so most of the gain from 0.78 to 0.36 s
comes from elsewhere. The "ready" rule waits for the program to go idle, and that day also
removed egui's animations and the scroll spring that kept large files busy. These runs do
not tell the changes apart. Start-up alone varies by about 0.1 s between runs on this laptop
(0.35 to 0.45 s for the tiny file here).

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

- **It does not always go idle on large files.** At 100 MiB one search settled only after
  15 s; at 300 MiB one search settled after 77 s, one after 0.8 s, and in two further runs
  the program was still busy after 240 s and 300 s, at about 30 % of a core.
  *Fixed (2026-11-16):* not the grid's column scan, as first suspected, but egui's spring
  back from past the end of a scroll area. Revealing one of the last rows scrolls the tree
  past its end; above 2^23 points (the tree of the 300 MiB file is about 13 million) an
  `f32` offset moves in whole points, so the spring never came closer than one point to the
  edge and asked for frames forever. Scroll areas now stop at the edge instead
  (`stop_at_the_edge` in `apps/xml-atlas/src/app.rs`). The search, before and after, median
  of three runs (settled):

  | File    | Before                              | After  |
  |---------|-------------------------------------|--------|
  | 10 MiB  | 1.16 s                              | 0.16 s |
  | 100 MiB | 0.56 s (one run 15 s)               | 0.14 s |
  | 300 MiB | 0.8 s, 77 s, twice over the limit   | 0.33 s |

  The tree actions did not change (215-234 ms at every size), so their time is not this
  spring.
- **Tree Home and text Page Down cost more as the file grows**, against rule 2 (per-frame
  work scales with what is on screen).
- **A tree action takes about 220 ms at every file size** where XML Marker takes one frame.
  *Fixed (2026-11-16):* the grid's scroll areas belong to the element shown, so every new
  selection started with hidden scroll bars that egui faded in over about 15 frames. egui's
  animations are now off (`no_animations` in `apps/xml-atlas/src/app.rs`). Before and
  after, median of three runs (settled):

  | Action                          | 10 MiB       | 100 MiB      | 300 MiB      |
  |---------------------------------|--------------|--------------|--------------|
  | tree: expand the root           | 232 -> 41 ms | 231 -> 47 ms | 230 -> 101 ms |
  | tree: End                       | 217 -> 29 ms | 215 -> 50 ms | 216 -> 100 ms |
  | tree: Home                      | 49 -> 29 ms  | 218 -> 234 ms | 648 -> 634 ms |
  | tree: 20 x Page Down            | 227 -> 49 ms | 232 -> 46 ms | 234 -> 79 ms |
  | tree: click a row near the end  | 233 -> 59 ms | 217 -> 45 ms | 220 -> 140 ms |
  | text: 20 x Page Down            | 49 -> 62 ms  | 96 -> 65 ms  | 184 -> 185 ms |

  What is left grows with the file, and is work rather than animation: tree Home selects
  the root, whose grid lists every order and then looks at them for columns over about 36
  frames (GRID-3); the path of an element near the end counts all its siblings
  (`Index::path`, 66 ms for the last of 633,000 orders); collapsing or expanding the root
  rebuilds the tree's rows (71 ms); and text Page Down at 300 MiB is not explained yet.
  *Since (2026-11-16, `docs/decisions.md`, D24):* the path and the root's grid take a list
  of the root's child elements instead of walking them. Text Page Down at 300 MiB was the
  selections near the end of the file paying for the path, and is down from 185 to 62 ms;
  tree End from 100 to 33 ms. Expanding the root and tree Home are unchanged.
  *Since (2026-11-16):* expanding or collapsing an element changes only the rows under it,
  and a wide parent's rows come from the same list of child elements, instead of every row
  of the tree being made again. In the engine, expanding the root at 300 MiB went from
  54-64 to 4-5 ms, and expanding one order while the root is expanded from 54-64 ms to
  under 0.03 ms. In the window, median of three runs at 300 MiB: expanding the root
  101 -> 47 ms, about what it takes at 10 and 100 MiB. A click near the end is down from
  140 to 58 ms since the table above, but that run came before D24 too, so the two changes
  cannot be told apart.
  Tree Home is unchanged (634 -> 694 ms, within the noise): it is the grid's column scan.
  *Since (2026-11-16):* the grid examines a wide parent's rows on up to 8 threads, and a
  frame spends 6 ms on it instead of 3 (GRID-3). The 633,506 orders at 300 MiB take 22-25 ms
  instead of 80 ms in the engine, a few frames instead of about 27. Tree Home, median of
  three runs: 234 -> 80 ms at 100 MiB, 694 -> 196 ms at 300 MiB.

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
