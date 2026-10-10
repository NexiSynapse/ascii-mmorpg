# Authored ASCII asset manifest

These terminal-native frames are original, workspace-authored ASCII assets for
the alpha client. They are plain text, embedded at compile time, and contain no
third-party artwork or generated binary data.

| File | Use | Frames |
| --- | --- | ---: |
| `player_warrior.txt` | Player/warrior animation | 2 |
| `monster_mossling.txt` | Mossling monster animation | 2 |
| `npc_herbalist.txt` | NPC fallback animation | 2 |
| `tree.txt` | Forest prop animation | 2 |
| `ruin.txt` | Ruin prop animation | 2 |
| `item_icons.txt` | Sword, shield, and generic item icons | 3 |
| `spell_effect.txt` | Spell trail/impact glyphs | 4 |

Frames in each file are separated by a line containing `---`. The client
selects them from the authoritative render frame; it does not load files from
the current working directory at runtime.

## Optional source references

The staged `assets/source/kenney/` CC0 packs remain optional source references
for future art tooling. None of those packs are required by, embedded in, or
loaded by this renderer. The files listed above are the complete visual input
for the current ASCII slice.
