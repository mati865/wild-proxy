# wild-proxy (temporary name)

This repository contains impostor binaries for GCC and Clang that redirect compilation and linking tasks to the Wild
linker.
This allows you to use Wild as your default linker without modifying your existing build processes.

Note: `-fuse-ld` and `--ld-path` will be ignored when using these impostor binaries, at least for now.

## Usage

There are a couple of ways to use these impostor binaries:

- prepend `fakes/` (symlinks to target/release) or `fakes-debug/` (symlinks to target/debug) to your `PATH`:
  `PATH=~/Projects/wild-proxy/fakes:$PATH cmake -B build/ -DCMAKE_BUILD_TYPE=Release -GNinja`
- use either one of the impostor symlinks or `wild-proxy` directly:
  `~/Projects/wild-proxy/target/debug/wild-proxy hello.c`
- use the original compiler to build and wild-proxy to link:
  `g++ hello.cc -c; ~/Projects/wild-proxy/target/debug/wild-proxy hello.o -lstdc++`

For Rust, you can set the linker in `.cargo/config.toml`:

```toml
[target.x86_64-unknown-linux-gnu]
linker = "wild-proxy"
```

or use `RUSTFLAGS=-Clinker=wild-proxy`.

## Performance

### Direct mode

Not yet implemented.

### Fallback mode (relies on the system compiler to provide linker arguments)

#### Compile and link:

Even though wild-proxy has to call the compiler additional time in this mode with `-###` argument, the performance is
roughly on par to using `-B<path to wild>` with GCC/Clang directly, presumably because of the linker being integrated
into this binary.

For reference, with `hello.c -### -o /tmp/bin` the tested GCC 15.2.1 takes just 381.5 µs and Clang takes an order
of
magnitude
more at
3.9 ms. But for various reasons like more parsing and calling two additional processes when using GCC (one to create
assembly and another one to assemble an object) the differences almost cancel out.

C:

```
❯ powerprofilesctl launch -p performance hyperfine --sort command -N -w 100 "gcc hello.c -B$HOME/Projects/wild -o /tmp/bin" "$HOME/Projects/wild-proxy/fakes/gcc hello.c -o /tmp/bin" "clang hello.c -B$HOME/Projects/wild -o /tmp/bin" "$HOME/Projects/wild-proxy/fakes/clang++ hello.c -o /tmp/bin"
Benchmark 1: gcc hello.c -B/home/mateusz/Projects/wild -o /tmp/bin
  Time (mean ± σ):      11.7 ms ±   0.4 ms    [User: 5.5 ms, System: 1.5 ms]
  Range (min … max):    11.0 ms …  13.3 ms    257 runs

Benchmark 2: /home/mateusz/Projects/wild-proxy/fakes/gcc hello.c -o /tmp/bin
  Time (mean ± σ):      11.3 ms ±   0.2 ms    [User: 31.7 ms, System: 29.8 ms]
  Range (min … max):    10.8 ms …  12.2 ms    255 runs

Benchmark 3: clang hello.c -B/home/mateusz/Projects/wild -o /tmp/bin
  Time (mean ± σ):      15.6 ms ±   0.3 ms    [User: 7.0 ms, System: 3.9 ms]
  Range (min … max):    14.9 ms …  16.7 ms    187 runs

Benchmark 4: /home/mateusz/Projects/wild-proxy/fakes/clang++ hello.c -o /tmp/bin
  Time (mean ± σ):      16.0 ms ±   0.2 ms    [User: 33.9 ms, System: 26.2 ms]
  Range (min … max):    15.4 ms …  16.7 ms    183 runs

Relative speed comparison
        1.04 ±  0.04  gcc hello.c -B/home/mateusz/Projects/wild -o /tmp/bin
        1.00          /home/mateusz/Projects/wild-proxy/fakes/gcc hello.c -o /tmp/bin
        1.38 ±  0.03  clang hello.c -B/home/mateusz/Projects/wild -o /tmp/bin
        1.42 ±  0.03  /home/mateusz/Projects/wild-proxy/fakes/clang++ hello.c -o /tmp/bin
```

C++:

```
❯ powerprofilesctl launch -p performance hyperfine --sort command -N -w 100 "g++ hello.cc -B$HOME/Projects/wild -o /tmp/bin" "$HOME/Projects/wild-proxy/fakes/g++ hello.cc -o /tmp/bin" "clang++ hello.cc -B$HOME/Projects/wild -o /tmp/bin" "$HOME/Projects/wild-proxy/fakes/clang++ hello.cc -o /tmp/bin"
Benchmark 1: g++ hello.cc -B/home/mateusz/Projects/wild -o /tmp/bin
  Time (mean ± σ):      78.2 ms ±   1.2 ms    [User: 67.4 ms, System: 6.0 ms]
  Range (min … max):    75.9 ms …  80.2 ms    39 runs

Benchmark 2: /home/mateusz/Projects/wild-proxy/fakes/g++ hello.cc -o /tmp/bin
  Time (mean ± σ):      78.7 ms ±   0.5 ms    [User: 97.6 ms, System: 25.3 ms]
  Range (min … max):    77.7 ms …  79.9 ms    38 runs

Benchmark 3: clang++ hello.cc -B/home/mateusz/Projects/wild -o /tmp/bin
  Time (mean ± σ):     111.1 ms ±   0.8 ms    [User: 95.7 ms, System: 10.3 ms]
  Range (min … max):   109.8 ms … 113.0 ms    26 runs

Benchmark 4: /home/mateusz/Projects/wild-proxy/fakes/clang++ hello.cc -o /tmp/bin
  Time (mean ± σ):     111.1 ms ±   0.8 ms    [User: 121.2 ms, System: 35.0 ms]
  Range (min … max):   110.1 ms … 112.6 ms    27 runs

Relative speed comparison
        1.00          g++ hello.cc -B/home/mateusz/Projects/wild -o /tmp/bin
        1.01 ±  0.02  /home/mateusz/Projects/wild-proxy/fakes/g++ hello.cc -o /tmp/bin
        1.42 ±  0.02  clang++ hello.cc -B/home/mateusz/Projects/wild -o /tmp/bin
        1.42 ±  0.02  /home/mateusz/Projects/wild-proxy/fakes/clang++ hello.cc -o /tmp/bin
```

#### Compile only:

Even when only compiling the unrealistically small source files (hello world), the impostor binaries have minimal
overhead.
Which is negligible for real-world projects.

C:

```
❯ powerprofilesctl launch -p performance hyperfine --sort command -N -w 100 "gcc hello.c -B$HOME/Projects/wild -c -o /tmp/hello.o" "$HOME/Projects/wild-proxy/fakes/gcc hello.c -c -o /tmp/hello.o" "clang hello.c -B$HOME/Projects/wild -c -o /tmp/hello.o" "$HOME/Projects/wild-proxy/fakes/clang hello.c -c -o /tmp/hello.o"
Benchmark 1: gcc hello.c -B/home/mateusz/Projects/wild -c -o /tmp/hello.o
  Time (mean ± σ):       5.9 ms ±   0.1 ms    [User: 4.6 ms, System: 1.3 ms]
  Range (min … max):     5.7 ms …   6.4 ms    512 runs

Benchmark 2: /home/mateusz/Projects/wild-proxy/fakes/gcc hello.c -c -o /tmp/hello.o
  Time (mean ± σ):       6.1 ms ±   0.1 ms    [User: 4.7 ms, System: 1.4 ms]
  Range (min … max):     5.9 ms …   6.8 ms    485 runs

Benchmark 3: clang hello.c -B/home/mateusz/Projects/wild -c -o /tmp/hello.o
  Time (mean ± σ):       6.8 ms ±   0.1 ms    [User: 4.0 ms, System: 2.7 ms]
  Range (min … max):     6.3 ms …   7.2 ms    449 runs

Benchmark 4: /home/mateusz/Projects/wild-proxy/fakes/clang hello.c -c -o /tmp/hello.o
  Time (mean ± σ):       7.0 ms ±   0.1 ms    [User: 4.3 ms, System: 2.7 ms]
  Range (min … max):     6.6 ms …   7.4 ms    445 runs

Relative speed comparison
        1.00          gcc hello.c -B/home/mateusz/Projects/wild -c -o /tmp/hello.o
        1.04 ±  0.03  /home/mateusz/Projects/wild-proxy/fakes/gcc hello.c -c -o /tmp/hello.o
        1.15 ±  0.03  clang hello.c -B/home/mateusz/Projects/wild -c -o /tmp/hello.o
        1.19 ±  0.03  /home/mateusz/Projects/wild-proxy/fakes/clang hello.c -c -o /tmp/hello.o
```

C++:

```
❯ powerprofilesctl launch -p performance hyperfine --sort command -N -w 100 "g++ hello.cc -B$HOME/Projects/wild -c -o /tmp/hello.o" "$HOME/Projects/wild-proxy/fakes/g++ hello.cc -c -o /tmp/hello.o" "clang++ hello.cc -B$HOME/Projects/wild -c -o /tmp/hello.o" "$HOME/Projects/wild-proxy/fakes/clang++ hello.cc -c -o /tmp/hello.o"
Benchmark 1: g++ hello.cc -B/home/mateusz/Projects/wild -c -o /tmp/hello.o
  Time (mean ± σ):      72.5 ms ±   0.5 ms    [User: 65.9 ms, System: 6.3 ms]
  Range (min … max):    71.5 ms …  73.9 ms    41 runs

Benchmark 2: /home/mateusz/Projects/wild-proxy/fakes/g++ hello.cc -c -o /tmp/hello.o
  Time (mean ± σ):      72.9 ms ±   0.5 ms    [User: 66.1 ms, System: 6.5 ms]
  Range (min … max):    71.9 ms …  74.2 ms    41 runs

Benchmark 3: clang++ hello.cc -B/home/mateusz/Projects/wild -c -o /tmp/hello.o
  Time (mean ± σ):      99.5 ms ±   0.7 ms    [User: 91.7 ms, System: 7.3 ms]
  Range (min … max):    98.1 ms … 100.7 ms    30 runs

Benchmark 4: /home/mateusz/Projects/wild-proxy/fakes/clang++ hello.cc -c -o /tmp/hello.o
  Time (mean ± σ):      99.5 ms ±   0.5 ms    [User: 91.1 ms, System: 8.0 ms]
  Range (min … max):    98.5 ms … 100.4 ms    30 runs

Relative speed comparison
        1.00          g++ hello.cc -B/home/mateusz/Projects/wild -c -o /tmp/hello.o
        1.00 ±  0.01  /home/mateusz/Projects/wild-proxy/fakes/g++ hello.cc -c -o /tmp/hello.o
        1.37 ±  0.01  clang++ hello.cc -B/home/mateusz/Projects/wild -c -o /tmp/hello.o
        1.37 ±  0.01  /home/mateusz/Projects/wild-proxy/fakes/clang++ hello.cc -c -o /tmp/hello.o
```

## Testing

There are no proper tests yet, I've only tested this manually on Arch Linux.
