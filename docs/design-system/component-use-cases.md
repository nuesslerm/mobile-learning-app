# Component → Use-Case Catalog

Quick reference for mapping common UI patterns to existing design-system primitives.
This is a living catalog, not a skill. Update it when primitives change; skills should not
embed specific prop names.

| Pattern                         | DS component             | Key prop / notes                         |
| ------------------------------- | ------------------------ | ---------------------------------------- |
| Segmented control / mode picker | `ToggleGroup`            | `segmented` — pill track + 3D depth      |
| Single action button            | `Button`                 | `variant="default"`, default `depth={2}` |
| List row with trailing action   | `ListItem`               | `ListItem.TrailingSlot`                  |
| On/off setting                  | `Switch`                 | —                                        |
| Card surface                    | `Card`                   | `variant="raised"` or `"inviting"`       |
| Tab bar                         | `Tabs` + `Tabs.MultiRow` | —                                        |

Rule of thumb: if the pattern is in this table, use the DS primitive. Do not build a custom
Pressable-based alternative at the feature callsite.
