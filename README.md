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

Even though wild-proxy has to call the compiler additional time in this mode, the performance is roughly on par to using
`-B<path to wild>` with GCC/Clang directly, presumably because of the linker being integrated into this binary:

C:

```
❯ powerprofilesctl launch -p performance hyperfine --sort command 'g++ hello.cc -B$HOME/Projects/wild -o /tmp/bin' '$HOME/Projects/wild-proxy/fakes/g++ hello.cc -o /tmp/bin' 'clang++ hello.cc -B$HOME/Projects/wild -o /tmp/bin' '$HOME/Projects/wild-proxy/fakes/clang++ hello.cc -o /tmp/bin'
Benchmark 1: g++ hello.cc -B$HOME/Projects/wild -o /tmp/bin
  Time (mean ± σ):      79.0 ms ±   1.4 ms    [User: 67.3 ms, System: 6.8 ms]
  Range (min … max):    76.7 ms …  84.3 ms    37 runs

Benchmark 2: $HOME/Projects/wild-proxy/fakes/g++ hello.cc -o /tmp/bin
  Time (mean ± σ):      78.0 ms ±   1.0 ms    [User: 85.9 ms, System: 47.1 ms]
  Range (min … max):    76.4 ms …  80.7 ms    37 runs

Benchmark 3: clang++ hello.cc -B$HOME/Projects/wild -o /tmp/bin
  Time (mean ± σ):     109.8 ms ±   1.0 ms    [User: 94.4 ms, System: 10.3 ms]
  Range (min … max):   108.3 ms … 112.1 ms    26 runs

Benchmark 4: $HOME/Projects/wild-proxy/fakes/clang++ hello.cc -o /tmp/bin
  Time (mean ± σ):     110.0 ms ±   1.1 ms    [User: 113.8 ms, System: 55.0 ms]
  Range (min … max):   108.2 ms … 113.0 ms    27 runs

Relative speed comparison
        1.01 ±  0.02  g++ hello.cc -B$HOME/Projects/wild -o /tmp/bin
        1.00          $HOME/Projects/wild-proxy/fakes/g++ hello.cc -o /tmp/bin
        1.41 ±  0.02  clang++ hello.cc -B$HOME/Projects/wild -o /tmp/bin
        1.41 ±  0.02  $HOME/Projects/wild-proxy/fakes/clang++ hello.cc -o /tmp/bin
```

C++:

```
❯ powerprofilesctl launch -p performance hyperfine --sort command 'g++ hello.cc -B$HOME/Projects/wild -o /tmp/bin' '$HOME/Projects/wild-proxy/fakes/g++ hello.cc -o /tmp/bin' 'clang++ hello.cc -B$HOME/Projects/wild -o /tmp/bin' '$HOME/Projects/wild-proxy/fakes/clang++ hello.cc -o /tmp/bin'
Benchmark 1: g++ hello.cc -B$HOME/Projects/wild -o /tmp/bin
  Time (mean ± σ):      79.0 ms ±   1.4 ms    [User: 67.3 ms, System: 6.8 ms]
  Range (min … max):    76.7 ms …  84.3 ms    37 runs

Benchmark 2: $HOME/Projects/wild-proxy/fakes/g++ hello.cc -o /tmp/bin
  Time (mean ± σ):      78.0 ms ±   1.0 ms    [User: 85.9 ms, System: 47.1 ms]
  Range (min … max):    76.4 ms …  80.7 ms    37 runs

Benchmark 3: clang++ hello.cc -B$HOME/Projects/wild -o /tmp/bin
  Time (mean ± σ):     109.8 ms ±   1.0 ms    [User: 94.4 ms, System: 10.3 ms]
  Range (min … max):   108.3 ms … 112.1 ms    26 runs

Benchmark 4: $HOME/Projects/wild-proxy/fakes/clang++ hello.cc -o /tmp/bin
  Time (mean ± σ):     110.0 ms ±   1.1 ms    [User: 113.8 ms, System: 55.0 ms]
  Range (min … max):   108.2 ms … 113.0 ms    27 runs

Relative speed comparison
        1.01 ±  0.02  g++ hello.cc -B$HOME/Projects/wild -o /tmp/bin
        1.00          $HOME/Projects/wild-proxy/fakes/g++ hello.cc -o /tmp/bin
        1.41 ±  0.02  clang++ hello.cc -B$HOME/Projects/wild -o /tmp/bin
        1.41 ±  0.02  $HOME/Projects/wild-proxy/fakes/clang++ hello.cc -o /tmp/bin
```

#### Compile only:

When only compiling the unrealistically small source files (hello world), the impostor binaries suffer from compiler's
`-###` overhead.
While for the tested GCC 15.2.1 that's just 308.4 µs, for Clang that's an order of magnitude more at
3.9 ms.
That means each time, you use the impostor binary to compile a file, this overhead is added, although in the real-world
projects this is negligible.

C:

```
❯ powerprofilesctl launch -p performance hyperfine --sort command -w 300 'gcc hello.c -B$HOME/Projects/wild -c -o /tmp/hello.o' '$HOME/Projects/wild-proxy/fakes/gcc hello.c -c -o /tmp/hello.o' 'clang hello.c -B$HOME/Projects/wild -c -o /tmp/hello.o' '$HOME/Projects/wild-proxy/fakes/clang hello.c -c -o /tmp/hello.o'
Benchmark 1: gcc hello.c -B$HOME/Projects/wild -c -o /tmp/hello.o
  Time (mean ± σ):       5.9 ms ±   0.1 ms    [User: 4.5 ms, System: 1.3 ms]
  Range (min … max):     5.7 ms …   6.3 ms    482 runs

Benchmark 2: $HOME/Projects/wild-proxy/fakes/gcc hello.c -c -o /tmp/hello.o
  Time (mean ± σ):       6.2 ms ±   0.1 ms    [User: 4.8 ms, System: 1.3 ms]
  Range (min … max):     6.0 ms …   6.6 ms    465 runs

Benchmark 3: clang hello.c -B$HOME/Projects/wild -c -o /tmp/hello.o
  Time (mean ± σ):       6.7 ms ±   0.1 ms    [User: 4.0 ms, System: 2.7 ms]
  Range (min … max):     6.4 ms …   7.1 ms    435 runs

Benchmark 4: $HOME/Projects/wild-proxy/fakes/clang hello.c -c -o /tmp/hello.o
  Time (mean ± σ):      10.8 ms ±   0.2 ms    [User: 6.8 ms, System: 4.1 ms]
  Range (min … max):    10.2 ms …  11.6 ms    282 runs

Relative speed comparison
        1.00          gcc hello.c -B$HOME/Projects/wild -c -o /tmp/hello.o
        1.05 ±  0.02  $HOME/Projects/wild-proxy/fakes/gcc hello.c -c -o /tmp/hello.o
        1.15 ±  0.03  clang hello.c -B$HOME/Projects/wild -c -o /tmp/hello.o
        1.83 ±  0.05  $HOME/Projects/wild-proxy/fakes/clang hello.c -c -o /tmp/hello.o
```

C++:

```
❯ powerprofilesctl launch -p performance hyperfine --sort command -w 300 'g++ hello.cc -B$HOME/Projects/wild -c -o /tmp/hello.o' '$HOME/Projects/wild-proxy/fakes/g++ hello.cc -c -o /tmp/hello.o' 'clang++ hello.cc -B$HOME/Projects/wild -c -o /tmp/hello.o' '$HOME/Projects/wild-proxy/fakes/clang++ hello.cc -c -o /tmp/hello.o'
Benchmark 1: g++ hello.cc -B$HOME/Projects/wild -c -o /tmp/hello.o
  Time (mean ± σ):      71.8 ms ±   0.6 ms    [User: 66.0 ms, System: 5.5 ms]
  Range (min … max):    70.8 ms …  73.3 ms    41 runs

Benchmark 2: $HOME/Projects/wild-proxy/fakes/g++ hello.cc -c -o /tmp/hello.o
  Time (mean ± σ):      72.3 ms ±   0.6 ms    [User: 66.2 ms, System: 5.8 ms]
  Range (min … max):    71.0 ms …  74.4 ms    41 runs

Benchmark 3: clang++ hello.cc -B$HOME/Projects/wild -c -o /tmp/hello.o
  Time (mean ± σ):      99.1 ms ±   1.1 ms    [User: 91.1 ms, System: 7.5 ms]
  Range (min … max):    97.6 ms … 101.8 ms    30 runs

Benchmark 4: $HOME/Projects/wild-proxy/fakes/clang++ hello.cc -c -o /tmp/hello.o
  Time (mean ± σ):     103.7 ms ±   0.8 ms    [User: 94.2 ms, System: 9.3 ms]
  Range (min … max):   102.0 ms … 105.0 ms    28 runs

Relative speed comparison
        1.00          g++ hello.cc -B$HOME/Projects/wild -c -o /tmp/hello.o
        1.01 ±  0.01  $HOME/Projects/wild-proxy/fakes/g++ hello.cc -c -o /tmp/hello.o
        1.38 ±  0.02  clang++ hello.cc -B$HOME/Projects/wild -c -o /tmp/hello.o
        1.44 ±  0.02  $HOME/Projects/wild-proxy/fakes/clang++ hello.cc -c -o /tmp/hello.o
```

## Testing

There are no proper tests yet, I've only tested this manually on Arch Linux.
