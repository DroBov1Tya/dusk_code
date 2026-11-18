Changed
`read_file` can read several files in one call: with `paths` (2 to 8 files, each from its start) every file comes back in a section headed by its path, a file that cannot be read gets its own error line, and the sections share the 64 KiB cap equally, with a note on each that was cut. It is behind `limits.read_many_files` (off by default); with it off the tool is offered exactly as before.
