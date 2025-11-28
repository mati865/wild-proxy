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

## Testing

There are no proper tests yet, I've only tested this manually on Arch Linux.
