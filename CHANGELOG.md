# Changelog

## Unpublished changes

General:

- Rewrote everything in Gtk 4 with shiny Libadwaita
    - Performance is a lot better now thanks to hardware acceleration

Viewer:

- Added crash a crash dialog and some proper logging (we still crash a lot lol)
- Support for annotations (WIP)
- Song statistics (WIP)
- Night mode
- Search for songs by name
- Some more cool minor changes

Editor:

- Removed tensorflow for staff detection, saving 200 MiB
    - The replacement is now using a self hosted web service, so that users don't have to deal with all those dependencies and large models
- Manually add and modify staff annotations (WIP)

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
