# afx

This thing is my attempt at a soundboard program.

![](https://cdn.discordapp.com/attachments/808286848946274324/1046140622064582748/recording-2022-11-26T190558UTC.mp4)

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

