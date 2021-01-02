# DiNoScore*

A digital music stand written with GTK in Rust. Includes an editor to import your sheet. Touch-friendly thanks to Libhandy. Everything is messy and work in progress!

\* **Di**gital **No**te **Score**. Never let anything stop your from a good acronym :D

## Try it out

Don't forget to clone the submodules! (`git clone --recurse-submodules https://gitlab.gnome.org/piegames/dinoscore.git`)

If you've installed Nix, simply type `nix-shell` and you're good to go. Power users may prefer `direnv allow`.

- **Run the application**: `cargo run --locked --release`
- Run the editor: `cargo run --locked --release --bin editor`

Songs are packed as zip files. The program lists everything in `$XDG_DATA_DIRS/dinoscore/library`, so simply put your songs into `$XDG_DATA_HOME/dinoscore/library`.

When using a foot switch to turn the page, bind the page turning actions to `Alt+n` (next) and `Alt+p` (previous).

## License

Until everything is sorted out, all parts that depend on GPL code are GPL-licensed as well.
