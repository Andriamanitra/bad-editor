# bad-editor: terminal-based text editor

The world obviously needs more [text editors](https://github.com/stars/Andriamanitra/lists/editors) so here's one.

> [!WARNING]
> This project is in *very* early stages of development, even basic editing is not really usable yet.
> Once it actually works I might change the name to *mediocre-editor* or something.

## Goals:

> [!IMPORTANT]
> These are long term goals, none of them are implemented yet.

* Multicursors
* Mouse support
* Usable for editing LARGE files and LONG lines
* Familiar keyboard shortcuts (Ctrl-z to undo, Ctrl-c to copy, etc.)
* Proper handling of Unicode grapheme clusters
* Syntax highlighting based on existing standard (probably TextMate grammars)
* Support plugins written with [Janet](https://github.com/janet-lang/janet)

## Non-goals

* Modal editing – I want something more like nano or [micro](https://github.com/zyedidia/micro) and less like vim
* Split view – I think that is a job for the terminal emulator or a dedicated tool like [tmux](https://github.com/tmux/tmux) or [zellij](https://github.com/zellij-org/zellij)
* Cross-platform support – I only care about Linux support
* Customizability – I don't need to customize the layout or colorschemes if the defaults are good
* Performance – Performance is a secondary concern in a terminal-based text editor


If this editor is not terrible enough for your taste you might enjoy [sht-editor](https://github.com/Andriamanitra/sht-editor).
