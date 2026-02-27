# what i need

## done (milestone 0.3.0 release)

  #### addition
  - add tool bar on the left
  - add mistral provider

  #### fixes
  - 

  #### modification
  - unify installer
  - migrate to neverliie ai sdk
  - migrate to rapidocr as the ocr backend
  - redesign the ui/ux
  - varius ui/ux improvement

## currently in progress
  - add direct retranslate on main window
  - deprecate result widgets and table (be replaced by direct text editing on image textbox)
    - it should be merged into translation feature cause it still needed there

## not yet started

  #### addition
  - add manual textbox insertion
  - implement watermarking
  - textbox styles
    - add stroke to typography
    - add directional blur to typography
    - add drop shadow to both
    - add 
  - 2 pane view of manhwa (for original and translated view side by side)(layers and overlays can be individualy toggle off and on)
  - add more items for ocr export
    - ocr tagging
    - pdf
    - docs
  - add window pos and size saves (remember from last session)
  - theme 
    - light mode
    - contrast
    - background gradient
  - split ocr result
  - add z index and reordering of textbox on the same img

  #### fixes
  - fix skew/free transform

  #### modification
  - rework how gradient work
  - implement titlebar to all apps
  - save edited state of textboxitem
  - dynamic link between import export ocr and translation
  - change how translation Work
    - [X] integrate it into main window
      - easier retranslate workflow
    - characters, places, and lore name dictionaries
  - hide textboximage button
  - profile improvement:
      - manual creation, deletion, rename
      - two pane view
  - make it possible to edit straight from textbox
  - advanced inpaint
    - free form selection (pen tools/draw)
    - switch on/off from selection
    - undo/redo
  - make font to be from system
  - briefly highlight sync ocr
  - enhance ocr result merging