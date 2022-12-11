# afx

This thing is my attempt at a soundboard program. afx currently runs on
Windows, Linux, and macOS, you can [download it
here](https://github.com/viluon/afx/releases/tag/latest). New versions are not
backwards-compatible, this will change with afx v1.0.0. I can only reliably
test the Linux build for now, so support for other platforms is on a
best-effort basis. Please help me improve afx by [opening an
issue](https://github.com/viluon/afx/issues/new) when you encounter a bug!

https://user-images.githubusercontent.com/7235381/204112926-93e98e67-3e9c-4bbb-be89-b4468943da80.mp4

### Why?

I tried some [prior art](#prior-art) and decided that none of the existing
options fit my needs.

### Prior art

Note that this specifically only considers offline soundboard software that
works out of the box on Linux, which is a pretty niche subset.

- [Soundux](https://github.com/Soundux/Soundux)
  - pros:
    - efficient
    - can import entire folders
    - has global search
  - cons:
    - list view wastes tremendous amounts of visual space
    - I couldn't get the tiled view to work
    - player controls are hard to reach, 2 clicks required to pause a playing sound
    - setting keybindings is clunky
    - volume controls are hard to reach (can't be changed in the list of playing
      sounds, you need to find the given sound in its tab, right click, select
      change volume, tick one or two checkboxes, set the volume, click OK)
  - many Soundux issues will be addressed in the ongoing rewrite, but I need a
    solution now
- [kenku.fm](https://kenku.fm)
  - pros:
    - pretty user interface with custom backgrounds
    - interesting split between playlists and soundboards, could work well
    - support for many audio formats (e.g. m4a containers)
  - cons:
    - no search
    - no bulk import, gotta go through a dialog for each file
    - looping audio stutters when seeking to the start of the file
    - large soundboards are hard to navigate, all tiles are the same colour
- [CasterSoundboard](https://github.com/JupiterBroadcasting/CasterSoundboard)
  - pros:
    - straightforward keyboard shortcuts
    - clear visual indication of track progress in each tile
  - cons:
    - a broken user interface, overlapping icons, poor contrast in dark mode
    - playback errors are silent

