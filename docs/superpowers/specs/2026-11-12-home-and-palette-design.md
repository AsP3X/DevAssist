# Welcome screen and tool palette

Date: 2026-11-12
Status: implemented, not yet released

## Goal

DevAssist will hold many tools. Switching between them gets one way that scales: a searchable palette. A welcome screen is where the program starts and where every tool can be reached from.

## What changes

| 0.4.0 | After |
|---|---|
| Starts in the Repos tool | Starts on the welcome screen |
| Top line lists the tools: `1 Repos  2 Docker` | Top line says where you are: `DevAssist › Docker` |
| `1`–`9` and a click on a name switch tools | Removed. The number keys are free for the tools |
| `Ctrl-P` lists the tools | The palette lists Home and every tool with its description; its key can be set |

## The welcome screen

- Shown at start. It is also "Home": an entry in the palette, and a click on `DevAssist` in the top line, bring it back.
- A large logo in a colour gradient, drawn column by column in the first half second. On a screen under 80 columns or 24 rows the logo is one line of spaced letters.
- One card per tool: its name, its description, and a status line the tool supplies ("56 clones · 3 behind"). A tool that has nothing to say yet shows no status.
- `←` `→` `↑` `↓` and `Tab` choose a card, `Enter` opens it, a click opens it.
- Typing a letter opens the palette with that letter already in it.
- `?` help, `v` version, `q` quit, as in a tool.

The Docker tool still asks Docker nothing until it is opened, so its card has no status before that.

## The palette

- Opened with the palette key from the welcome screen and from any tool that is not in a dialog or a text field.
- Lists Home, then every tool, each with its description. Typing narrows the list by name and description. `↑` `↓` move, `Enter` goes there, `Esc` closes, a click goes there.
- It lists places only. Actions inside a tool are not in it.

## Settings

Two keys in `devassist.toml`, both optional:

| Key | Default | Meaning |
|---|---|---|
| `palette_key` | `ctrl-p` | The key that opens the palette: `ctrl-`, `alt-` or `shift-` joined to a letter or digit, or `f1` to `f12` alone or with a modifier. A letter needs `ctrl` or `alt`, so that typing stays typing. |
| `start` | `home` | Where the program starts: `home`, or a tool's name in lowercase. An unknown name starts at home. |

Both are kept when the settings dialog saves the file. Neither is in the dialog yet.

## In the code

- `Feature` gains `about()` (the description) and `status()` (the live line). A tool registers once, in `main.rs`; the palette, the welcome cards, the top line and help all read the same trait.
- `shell/home.rs` draws the welcome screen and handles its keys. `shell/keys.rs` reads and matches a key name. `shell/tools.rs` becomes the palette. `shell/chrome.rs` draws the slim top line.
- The shell has one more state: Home is on screen, or a tool is. Tools keep working in the background either way.

## Testing

- `keys`: names that parse, names that are refused and why, matching a key event, the label shown in hints.
- `home`: moving between cards, Enter and a click open one, a letter opens the palette, the big and the small logo, cards wrapping on a narrow screen.
- Palette: Home is listed first, typing narrows by description too, picking Home.
- Shell: starts at home or at the tool named in `start`; the palette key from the settings works and `ctrl-p` then does not; digits reach the tool; the top line names the tool.
- Config: the two keys are read, refused when wrong, and written back.
