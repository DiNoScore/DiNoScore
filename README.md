# DiNoScore*

A digital music stand written in Rust with Gtk 4 and Libadwaita. It is comparable to *forScore*, but completely free\*\*. Touch screen is optional but recommended.

\* **Di**gital **No**te **Score**. Never let anything stop your from a good acronym :D

\*\* Not only is it free, it also works on relatively low budget hardware.

## Features

- Probably the only open source digital music stand software out there
- Adaptive layout that shows a configurable amount of staves per page
  - It can also show columns at once if you wish
  - Supports different staff sizes for different environmental conditions
- Annotations support through Xournal++
- Editor to import your sheets
- Night mode

## Planned features (help appreciated)

- Setlists
- Synchronized or remote page turning
	- Automatic page turning via score following?
- Four hands mode/duet on single device?

## Try it out

If you've installed Nix, simply type `nix-shell` and you're good to go. Power users may prefer [`direnv allow`](http://direnv.net/).

- **Run the application**: `cargo run --locked --release`
- Run the editor: `cargo run --locked --release --bin editor`

Songs are packed as zip files. The program lists everything in `$XDG_DATA_DIRS/dinoscore/songs`, so simply put your songs into `$XDG_DATA_HOME/dinoscore/songs`. If youd don't know what `XDG_DATA_HOME` is, use `~/.local/share/dinoscore/songs` instead.

When using a foot switch to turn the page, bind the page turning actions to `Alt+n` (next) and `Alt+p` (previous).

There's also a small CLI for utility stuff. At the moment, the only feature it has is to bulk-upgrade song files to the newest version of the format. Run it with `cargo run --locked --release --bin cli -- upgrade --help`

Windows users please checkout the [separate documentation](./Windows.md)

## License

This work is licensed under the EUPL v1.2 or later. Parts of the library, notably the file format, the layout engine and the staff recognition, are additionally dual-licensed under the MPL.

Contact the owner(s) for use in proprietary software.
