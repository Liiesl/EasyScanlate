# what i need

## done (milestone 0.1.2 release)

  #### addition
  - implement hide text

  #### fixes
  - fix long loading time on startup (torch import)

  #### modification
  - 

## currently in progress
  - add inpainting.
    - [X] inpainting base logic (initiation, lifecycle, etc.)
    - [X] inpainting restore
    - [ ] advanced inpaint
      - [ ] free form selection (pen tools/draw)
      - [ ] switch on/off from selection
      - [ ] undo/redo
    - known bugs caused by this
      - 

## not yet started

  #### addition
  - add mica effect
  - add manual textbox insertion
  - implement watermarking
  - textbox styles
    - add stroke to typography
    - 

  #### fixes
  - add stroke on typography
  - fix skew/free transform
  - fix stitched img not saved as stitched
  - fix find and replace bugs :
    - roman character not working for some reason if there are other profile in other type of character (non roman)
    - profile creation/switching crashes the app when on find

  #### modification
  - rework how gradient work
  - implement titlebar to all apps
  - save edited state of textboxitem
  - dynamic link between import export ocr and translation
  - change how translation Work
    - make translations into non blocking window
      - or integrate it into main window
      - easier retranslate workflow
    - change translation format to xml
    - characters, places, and lore name dictionaries
  - hide textboximage button
  - profile improvement:
      - manual creation, deletion, rename
      - two pane view
  - make it possible to edit straight from textbox