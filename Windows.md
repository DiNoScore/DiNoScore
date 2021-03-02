# Windows

If you really have no other choice, here are some build instructions for Windows.

## Build instructions

- Get Windows
- Install MinGW-64 (Also called Msys2 somehow? Dunno). Always use the 64 bit version
- Clone the repository and `cargo build --locked --release`
- Get through all the compiler errors and install missing dependencies
- Congrats, you got a binary. Except that it won't run outside of MinGW

## Package instructions

If you want to make the binary portable, so that users can simply double-click to launch it. These instructions are heavily copied from the [Xournal++ package script](https://github.com/xournalpp/xournalpp/blob/81982f8af782efb0718d033633c35efda58f66f7/windows-setup/package.sh)

**Warning:** the code below is not tested, run every command manually and check out if it worked. This is **not** a script to paste and execute!

```sh
out=./out
viewer_file=./target/release/viewer.exe

# Create our output directory
rm -r $out
mkdir $out
mkdir $out/bin
mkdir $out/lib
mkdir $out/share

# Copy the binary. If you don't use --release or cross-compile, your actual target path might be different
cp $viewer_file $out/bin

# Copy the resources
cp -T ./res ./bin/res

# Copy all the dependency DLLs
ldd $viewer_file | grep '\/mingw.*\.dll' -o | sort -u | xargs -I{} cp "{}" "$out/bin/"

# Copy additional stuff for the GdkPixbuf dependency
cp -rT "/mingw64/lib/gdk-pixbuf-2.0" "$out/lib/gdk-pixbuf-2.0"
ldd /mingw64/lib/gdk-pixbuf-2.0/2.10.0/loaders/*.dll | grep '\/mingw.*\.dll' -o | xargs -I{} cp "{}" "$out/bin/"

# Copy icons and other shared resources
cp -rT "/mingw64/share/icons" "$out/share/icons"
cp -rT "/mingw64/share/glib-2.0" "$out/share/glib-2.0"
```

You're done. Now you should have a huge folder full of DLL hell which contains an executable in `bin` that *should* work. Congrats. Next time, please use Linux.

## Cross compiling for windows

- You need to install a lot of `mingw-w64-*` packages into your system. Probably the same as on Windows.
- Uncomment the `[target.x86_64-pc-windows-gnu]` section in `./.cargo/config`
- Pass `--target=x86_64-pc-windows-gnu` to `cargo build`
- If you then figure out how to make the resulting binary work on Windows (from within Linux), let me know. I think there's [`peldd`](https://github.com/gsauthof/pe-util) that can be used to replace the `ldd | grep | cp` stuff we did above.
- There's also [this tutorial](https://nivethan.dev/devlog/cross-compiling-rust-gtk-projects-for-windows.html) with a Dockerfile, that is definitely worth a read. If you get it to work, please let me know as well.
