# what i need

## done (milestone 0.5.0 release)

#### addition

- font fixed size
- font line height and letter spacing
- case toggle
- gradient support for stroke and bg
- 

#### fixes

- cap free transform to prevent cowtie/flipped/collapsed quad
- cache model json as fallback and delta update only when starting up
- app level presist of color picker swatch and recent
- 

#### modification

- move layer and edit to the left of main area
- rework color picker ux
- 

## currently in progress

#### addition

- proper non panic error rfd
- text entry alignment and auto align
- 

#### fixes

- duplicate inpaint introduced in 0.4.2
- scroll reset on minimize/focus lost.
- under heavy computation, switching tabs make ui froze, interaction seem to work but the ui graphics themself froze.
- case not working as intended
- 

#### modification

- retire status bar
- rework gradient ux
- 

## not yet started

#### addition

- add manual textbox insertion
- implement watermarking
- textbox styles
  - add directional blur to typography
  - add drop shadow to both
  - add
- add more items for ocr export
  - ocr tagging
  - pdf
  - docs
- add window pos and size saves (remember from last session)
- split ocr result
- add z index and reordering of textbox on the same img
- watermark detection
  - custom model for detecting watermark
  - auto inpaint detected watermark with backed based on the bg
- 

#### fixes

- 

#### modification

- rework how gradient work
- dynamic link between import export ocr and translation
- change how translation Work
  - characters, places, and lore name dictionaries
- hide textboximage button
- profile improvement:
  - manual creation, deletion, rename
  - two pane view
- advanced inpaint
  - free form selection (pen tools/draw)
  - switch on/off from selection
  - undo/redo
-
