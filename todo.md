# what i need

## done (milestone 0.2.0 release)

  #### addition
  - add context fill.
  - implement hide text
  - add ocr override dialog confirmation
  - add custom state aware button class

  #### fixes
  - fix long loading time on startup (torch import)
  - fix stitched img not saved as stitched
  - fix too much recent project

  #### modification
  - make app sync to be on its own manager
  - make menu button to be layout class
  - make further ocr to override old ocr
  - change translation format to xml like
  - make manual ocr overlay presistent

## currently in progress
  - 

## not yet started

  #### addition
  - add mica effect
  - add manual textbox insertion
  - implement watermarking
  - textbox styles
    - add stroke to typography
    - add directional blur to typography
    - add drop shadow to both
    - add 
  - add link to github issues for when there is crash
    - add issue template
    - 

  #### fixes
  - add stroke on typography
  - fix skew/free transform
  - fix stitched img not saved as stitched
  - fix find and replace bugs :
    - roman character not working for some reason if there are other profile in other type of character
    - profile creation/switching crashes the app when on find

  #### modification
  - rework how gradient work
  - implement titlebar to all apps
  - save edited state of textboxitem
  - dynamic link between import export ocr and translation 
  - make translations into non blocking window
  - hide textboximage button
  - profile improvement:
      - manual creation, deletion, rename
      - two pane view