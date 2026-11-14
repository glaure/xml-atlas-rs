# Benchmarks

How fast XML Atlas opens files, compared with XML Marker 1.1.

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
