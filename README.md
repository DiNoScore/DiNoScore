# DiNoScore*

A digital music stand written with GTK in Rust using [WoAB](https://github.com/idanarye/woab). Includes an editor to import your sheet. Touch-friendly thanks to Libhandy. Everything is messy and work in progress!

\* **Di**gital **No**te **Score**. Never let anything stop your from a good acronym :D

## Try it out

If you've installed Nix, simply type `nix-shell` and you're good to go. Power users may prefer `direnv allow`.

- **Run the application**: `cargo run --locked --release`
- Run the editor: `cargo run --locked --release --bin editor`
	- You need to download the models first: `cd res && wget "https://github.com/OMR-Research/MeasureDetector/releases/download/v1.0/2019-05-16_faster-rcnn-inception-resnet-v2.pb"`

Songs are packed as zip files. The program lists everything in `$XDG_DATA_DIRS/dinoscore/songs`, so simply put your songs into `$XDG_DATA_HOME/dinoscore/songs`. If youd don't know what `XDG_DATA_HOME` is, use `~/.local/share/dinoscore/songs` instead.

When using a foot switch to turn the page, bind the page turning actions to `Alt+n` (next) and `Alt+p` (previous).

There's also a small CLI for utility stuff. At the moment, the only feature it has is to bulk-upgrade song files to the newest version of the format. Run it with `cargo run --locked --release --bin cli -- upgrade --help`

Windows users please checkout the [separate documentation](./Windows.md)

## License

Until everything is sorted out, all parts that depend on GPL code are GPL-licensed as well. All DiNoScore applications will be licensed under the GPL anyways. But the plan is to make a library with common functionality that can be used under the MPL.
