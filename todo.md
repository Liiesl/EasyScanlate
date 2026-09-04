# what i need

## done (milestone 0.4.0 release)

#### addition

- 2 pane view of manhwa (for original and translated view side by side)
- tabbed multi project workflow
- auto detect and apply textbox style
- auto filter sfx
- lama and aot-gan inpaint backend
- directml backend for ml inference
- auto multi page ocr (seam handling)
- auto multi page inpaint
- add anthropic, openai, openrouter, moonshotai, zai, xai, deepseek, minimax, mistral, nvidia, opencode, kilo, ollama, vllm, llama.cpp, and custom openai/anthropic compatible
- add onboarding workflow.

#### fixes

- fix skew/free transform

#### modification

- rewrite app to Rust
- save edited state of textboxitem to .mmtl
- make font to be from system
- highlight sync ocr
- deprecate split/stitch page in favor of auto ocr detection/rendering.
- improved translation provider and auto model listing handling
- separate models from binary installer (cause of growing number of ml models)

## currently in progress

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

#### fixes

- fix translation panel card styling

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
- enhance ocr result merging
