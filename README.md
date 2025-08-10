# luanti-mts

Rust library for working with Luanti Schematics files (MTS). Luanti was
formerly known as Minetest.

More information about schematics and how to use :

* [https://docs.luanti.org/for-creators/schematic/]()
* [https://content.luanti.org/packages/Wuzzy/schemedit/]()

This was born from one of my personal projects in which I wanted to merge
(thousands of) smaller schematics into a bigger one. That's why the main
feature of the library is `Schematic::merge()` method.

There are some other editing methods but they haven't been as fleshed out, and
there are probably plenty of other schematic manipulations that could be
included.

This is very much in development (so the public API might break at any moment).
My reason for making it open source this early is that I'm hoping someone will
find this and either find it immediately useful, or is willing to open a
constructive dialogue about what's missing (or maybe even contribute code.)

Please feel free to use GitHub issues to communicate.
