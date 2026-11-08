# Embedded JavaScript generators

The desktop app runs a few JavaScript bundles inside its QuickJS interpreter,
so their sources stay JavaScript/TypeScript. Each folder is a standalone pnpm
project that writes a generated asset into `crates/monocode-gpui/assets/`.
Change the source here and regenerate; never edit the bundle.

| Folder | Writes | What it is |
| --- | --- | --- |
| `native-editor/` | `native-editor.js`, `native-format.js`, `native-editor.LICENSE.txt` | CodeMirror analysis (highlighting, lint, completion, search) and Prettier formatting for the native editor |
| `native-settings/` | `settings-arcade.js` | The terminal arcade games and the composer runner mascot |

```sh
cd scripts/generators/native-editor && pnpm install && pnpm build
cd scripts/generators/native-settings && pnpm install && pnpm build
```

The icon tables and SVGs are generated in Rust: `cargo xtask icons`.
