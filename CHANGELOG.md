# Changelog

## Version 0.4.0

General:

- Rewrote everything in Gtk 4 with shiny Libadwaita
    - Performance is a lot better now thanks to hardware acceleration
- Windows support
    - Download a huge zip file with everything from the releases page, extract and run
- New file format version
    - Please update your scores with the CLI to reduce the loading time

Viewer:

- Added crash a crash dialog and some proper logging (we still crash a lot lol)
- Support for annotations (WIP)
- Night mode
- New fancy song preview
- Song statistics
- Search for songs by name
- Lazy loading of content with progress bar
- Less visual artifacts when the original score has tightly packed staves which overlap vertically
- Some more cool minor changes

Editor:

- Removed tensorflow for staff detection, saving 200 MiB
    - The replacement is now using a self hosted web service, so that users don't have to deal with all those dependencies and large models
    - This might have improved the results, especially for non-standard systems (e.g. multiple instruments or guitar tabs)
- Manually add and modify staff annotations
- Optionally extract images from PDFs that just embed images, to increase performance
- More lazy loading and progress bars instead of freezing the UI

CLI:

- New commands (`re-recognize` and `v4-extract-images`) that help with the v3→v4 format migration
- New `regenerate-thumbnail` command

## Version 0.3.0

- Rewritten the layout engine
    - It's not perfect, but it leads to significantly more consistent results.
- Proper support for zooming
    - Including shortcuts to automatically calculate some appropriate sizes.
    - The behavior when changing the size to keep the current page got improved as well.
- New file format version
    - It now stores scores in a one file per page fashion
    - Input files are passed along as untreated as possible. The only exception are PDF files, which are split into one PDF per page.
- Rewritten a lot of the UI partially due to a WoAB update
    - The editor should be working again
- Removed trained model files from the repository.
    - You need to manually download them as documented in the README if you want to use automatic detection in the editor.

## Version 0.2.0

- Rewritten a lot of the UI using WoAB
    - Maybe the editor is broken now
- New file format version 2
- Improvements to loading times and responsiveness
- Songs are now sorted alphabetically

## Version 0.1.0

### Changes

- Initial crappy release
