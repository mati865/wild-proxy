Example:
```
/tmp
❯ bat -n hello.cpp
   1 #include <iostream>
   2
   3 int main() {
   4     std::cout << "Hello, World!" << std::endl;
   5     return 0;
   6 }

/tmp
❯ PATH=~/Projects/wild-proxy:$PATH wild-g++ hello.cpp
WARNING: wild: --plugin /usr/lib/gcc/x86_64-pc-linux-gnu/14.2.1/liblto_plugin.so is not yet supported

/tmp
❯ ./a.out
Hello, World!

/tmp
❯ readelf -p .comment a.out

String dump of section '.comment':
  [     0]  GCC: (GNU) 14.2.1 20250207
  [    1c]  Linker: Wild version 0.4.0


/tmp
❯ rm a.out

/tmp
❯ PATH=~/Projects/wild-proxy:$PATH wild-clang++ hello.cpp

/tmp
❯ ./a.out
Hello, World!

/tmp
❯ readelf -p .comment a.out

String dump of section '.comment':
  [     0]  clang version 19.1.7
  [    15]  GCC: (GNU) 14.2.1 20250207
  [    31]  Linker: Wild version 0.4.0


```
